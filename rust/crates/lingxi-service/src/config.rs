//! Service configuration resolution (R02-T02 step 1).
//!
//! Precedence protocol (documented, tested, shown in the safe log):
//!
//! ```text
//!   --test-mode  >  --home <DIR>  >  LINGXI_HOME (env)  >  --config <FILE>.home
//! ```
//!
//! - `--test-mode` forces a fresh isolated synthetic home under the system
//!   temp dir; every other home source is IGNORED (and reported as ignored),
//!   so a test run can never fall into a production directory.
//! - The environment variable name mirrors the incumbent Node service
//!   (`LINGXI_HOME`); unlike the Node resolver there is no `~/.lingxi`
//!   default and no `~` expansion here — the composition root refuses to
//!   guess a real user directory, and shell-level tilde must be expanded by
//!   the invoking shell (a `~…` value is relative and rejected).
//! - The config file is opt-in via an explicit `--config <PATH>`; there is
//!   no conventional-location discovery, because silently reading files out
//!   of a real user directory is exactly what this task forbids. The file is
//!   strict JSON: exactly `{"home": "<absolute path>"}` — unknown keys,
//!   missing `home`, non-string values and unreadable/unparsable files are
//!   loud errors, never silently skipped.
//! - No source at all => [`ConfigError::MissingHome`] (exit 2 in the binary).
//!
//! CLI parsing is strict (T01 REVIEW_R1 F03): duplicate flags, flag-shaped
//! values (`--bind --home /x`) and unknown tokens (including `--home=/x`)
//! are explicit errors. All illegal input fails loudly; nothing is silently
//! overwritten or consumed.
//!
//! Every resource-limit flag additionally has a documented INCLUSIVE
//! supported range with a checked conversion into its consumer's native
//! type (R02 stage-repair R3 / R3-F02): out-of-range values fail at parse
//! time with [`ConfigError::InvalidLimit`] (exit 2 in the binary) instead
//! of truncating (`as u32` made `4294967296` a permanent 429), panicking
//! (a queue bound above tokio's semaphore maximum died with exit 101
//! inside `mpsc::channel`), or saturating (the 2x connection-cap
//! derivation). The numeric bounds live next to each flag arm below and
//! in the binary's `--help` text.
//!
//! This module deliberately takes the environment value and temp base as
//! *parameters* instead of calling `std::env` itself, so tests inject values
//! without polluting the outer process environment.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use lingxi_adapters::storage::{MAX_QUEUE_CAPACITY, MAX_QUEUE_WAIT_TIMEOUT_MS};

/// Inclusive upper bound of every millisecond time-budget flag
/// (`--shutdown-timeout-ms`, `--http-request-budget-ms`,
/// `--db-wait-budget-ms`) — R02 stage-repair R3 / R3-F02. 30 days exceeds
/// any legitimate per-shutdown / per-request / per-queue-wait budget by
/// orders of magnitude, and keeps every downstream `Instant::now() +
/// duration` computation unambiguously safe: platform monotonic clocks
/// overflow for absurd durations (tokio's own docs note ~1000 years
/// overflows macOS and ~100 years FreeBSD), so an unbounded u64 here was
/// a latent runtime panic, not a real "feature".
pub const MAX_TIME_BUDGET_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// Machine-checked relationship: the CLI time-budget bound must never
/// exceed the library bound of the DB queue wait (a CLI-accepted value
/// must always pass `StoreOptions::validate`).
const _: () = assert!(MAX_TIME_BUDGET_MS <= MAX_QUEUE_WAIT_TIMEOUT_MS);

/// Inclusive upper bound of `--http-max-in-flight` (R02 stage-repair R3 /
/// R3-F02): the composition root derives the transport CONNECTION cap as
/// 2× this value, so the flag is only supported up to `usize::MAX / 2` —
/// the derivation then uses a CHECKED multiplication and the overflow
/// class is rejected at parse time instead of being masked by a
/// saturating one.
pub const MAX_HTTP_MAX_IN_FLIGHT: u64 = (usize::MAX / 2) as u64;

/// Where the effective data root came from (displayed in the safe log and
/// the readiness line as `source=`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeSource {
    Cli,
    Env,
    ConfigFile,
    TestMode,
}

impl fmt::Display for HomeSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            HomeSource::Cli => "cli",
            HomeSource::Env => "env",
            HomeSource::ConfigFile => "config-file",
            HomeSource::TestMode => "test-mode",
        })
    }
}

/// Raw (still unvalidated-as-home) values from strict CLI parsing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CliOptions {
    pub bind: Option<String>,
    pub home: Option<PathBuf>,
    pub config: Option<PathBuf>,
    pub test_mode: bool,
    /// Explicit network mode (`--network-mode loopback|lan`); `None` = the
    /// loopback default. LAN exposure exists ONLY through this flag
    /// (R02-T03).
    pub network_mode: Option<String>,
    /// 显式 HTTPS 证书与私钥必须成对提供，路径均为绝对路径。
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    /// Explicit graceful-shutdown deadline in milliseconds
    /// (`--shutdown-timeout-ms`, R02-T06); `None` = the production default.
    pub shutdown_timeout_ms: Option<u64>,
    /// Resource-limit overrides (R02-T07); `None` = the documented
    /// production default of the corresponding limit.
    pub max_ws_connections: Option<usize>,
    pub db_queue_bound: Option<usize>,
    pub event_subscriber_queue: Option<usize>,
    pub event_reorder_bound: Option<usize>,
    pub max_subscribers: Option<usize>,
    pub log_max_bytes: Option<u64>,
    pub log_max_files: Option<usize>,
    /// Per-peer HTTP request budget per rate window (R02-T07).
    pub http_rate_max: Option<u32>,
    /// HTTP admission overrides (R02 stage-repair R1 / F07): in-flight cap
    /// and per-request budget; `None` = the documented production default.
    pub http_max_in_flight: Option<usize>,
    pub http_request_budget_ms: Option<u64>,
    /// DB queue wait budget in milliseconds (F07): how long ONE submission
    /// may wait for queue capacity before surfacing QueueFull → 503.
    pub db_wait_budget_ms: Option<u64>,
}

/// Result of home-source precedence resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedHome {
    /// The effective (pre-canonicalization) data root.
    pub path: PathBuf,
    pub source: HomeSource,
    /// Sources that were present but ignored because a higher-precedence
    /// source won (or because test mode overrides everything). Reported in
    /// the safe log so "why did it pick this root" is always answerable.
    pub ignored: Vec<IgnoredHomeSource>,
}

/// A lower-precedence home source that was present but not used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoredHomeSource {
    pub origin: HomeSource,
    pub path: PathBuf,
}

/// Environment variable consulted for the data root (same name as the
/// incumbent Node service; documented divergence: no default, no `~`).
pub const HOME_ENV_VAR: &str = "LINGXI_HOME";

/// Errors raised while assembling the service configuration. Display
/// strings are user-facing and carry no secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    MissingHome,
    RelativeHome {
        value: PathBuf,
    },
    HomeIsFilesystemRoot {
        value: PathBuf,
    },
    HomeIsNotADirectory {
        value: PathBuf,
    },
    HomeCreateFailed {
        value: PathBuf,
        source: String,
    },
    BadBind {
        value: String,
        source: String,
    },
    /// `--network-mode` value outside the strict `loopback|lan` vocabulary.
    BadNetworkMode {
        value: String,
    },
    BadTlsConfig {
        detail: String,
    },
    StoredNetworkInvalid {
        detail: String,
    },
    /// `--shutdown-timeout-ms` value that is not a positive decimal integer.
    InvalidShutdownTimeout {
        value: String,
    },
    /// A resource-limit flag (R02-T07) whose value is not a positive
    /// decimal integer within its documented constraint.
    InvalidLimit {
        flag: String,
        value: String,
        constraint: String,
    },
    /// Non-loopback bind while the (default) loopback network mode is in
    /// effect: LAN exposure must be an explicit choice, never a silent
    /// side effect of `--bind`.
    NetworkModeBindMismatch {
        bind: std::net::SocketAddr,
        mode: crate::transport::NetworkMode,
    },
    UnknownArgument {
        value: String,
    },
    MissingArgumentValue {
        flag: String,
    },
    DuplicateArgument {
        flag: String,
    },
    FlagShapedValue {
        flag: String,
        value: String,
    },
    ConfigFileUnreadable {
        path: PathBuf,
        source: String,
    },
    ConfigFileInvalid {
        path: PathBuf,
        detail: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::MissingHome => write!(
                f,
                "missing data root: pass --home <DIR>, set {HOME_ENV_VAR}, or point \
                 --config at a config file with an absolute \"home\"; there is no \
                 default into a real user directory"
            ),
            ConfigError::RelativeHome { value } => write!(
                f,
                "the data root must be an absolute path, got {:?} (tilde must be \
                 expanded by the invoking shell)",
                value.display()
            ),
            ConfigError::HomeIsFilesystemRoot { value } => write!(
                f,
                "the data root must not be the filesystem root, got {:?}",
                value.display()
            ),
            ConfigError::HomeIsNotADirectory { value } => write!(
                f,
                "data root {:?} exists but is not a directory",
                value.display()
            ),
            ConfigError::HomeCreateFailed { value, source } => {
                write!(f, "cannot create data root {:?}: {source}", value.display())
            }
            ConfigError::BadBind { value, source } => {
                write!(f, "invalid --bind {value:?}: {source}")
            }
            ConfigError::BadNetworkMode { value } => write!(
                f,
                "invalid --network-mode {value:?}: must be \"loopback\" or \"lan\" \
                 (LAN exposure is an explicit opt-in, the default is loopback)"
            ),
            ConfigError::BadTlsConfig { detail } => {
                write!(f, "invalid TLS configuration: {detail}")
            }
            ConfigError::StoredNetworkInvalid { detail } => {
                write!(f, "saved network configuration invalid: {detail}")
            }
            ConfigError::InvalidShutdownTimeout { value } => write!(
                f,
                "invalid --shutdown-timeout-ms {value:?}: must be a positive \
                 decimal integer in the supported range 1..={MAX_TIME_BUDGET_MS} \
                 (milliseconds, at most 30 days; the graceful-shutdown deadline \
                 for each shutdown phase)"
            ),
            ConfigError::InvalidLimit {
                flag,
                value,
                constraint,
            } => write!(
                f,
                "invalid {flag} {value:?}: must be a positive decimal integer \
                 ({constraint})"
            ),
            ConfigError::NetworkModeBindMismatch { bind, mode } => write!(
                f,
                "--bind {bind} is not a loopback address while the network mode is \
                 {mode}: pass --network-mode lan explicitly to expose the service \
                 beyond loopback (never a silent LAN bind)"
            ),
            ConfigError::UnknownArgument { value } => {
                write!(f, "unknown argument {value:?}")
            }
            ConfigError::MissingArgumentValue { flag } => {
                write!(f, "flag {flag} requires a value")
            }
            ConfigError::DuplicateArgument { flag } => {
                write!(f, "flag {flag} given more than once")
            }
            ConfigError::FlagShapedValue { flag, value } => write!(
                f,
                "flag {flag} requires a value, got flag-shaped value {value:?} \
                 (values starting with -- are rejected to avoid silently \
                 consuming the next flag)"
            ),
            ConfigError::ConfigFileUnreadable { path, source } => {
                write!(f, "cannot read --config {:?}: {source}", path.display())
            }
            ConfigError::ConfigFileInvalid { path, detail } => {
                write!(f, "invalid --config {:?}: {detail}", path.display())
            }
        }
    }
}

impl std::error::Error for ConfigError {}

fn is_flag_shaped(value: &str) -> bool {
    value.starts_with("--")
}

/// Parses CLI arguments (excluding the program name) strictly.
///
/// Contract (pinned by tests):
/// - every recognized value flag (`--bind`, `--home`, `--config`) consumes
///   exactly the next token; that token must not itself start with `--`
///   (loud [`ConfigError::FlagShapedValue`] instead of a confusing BadBind);
/// - giving any flag more than once is [`ConfigError::DuplicateArgument`]
///   (no silent last-wins — REVIEW_R1 F03);
/// - any other token (including `--home=/x` and `--test-mode=1`) is
///   [`ConfigError::UnknownArgument`];
/// - `--test-mode` is a boolean flag.
pub fn parse_cli<I, S>(args: I) -> Result<CliOptions, ConfigError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut options = CliOptions::default();
    let mut iter = args.into_iter().map(Into::into).peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--bind" => {
                if options.bind.is_some() {
                    return Err(ConfigError::DuplicateArgument {
                        flag: "--bind".to_string(),
                    });
                }
                let value = iter.next().ok_or(ConfigError::MissingArgumentValue {
                    flag: "--bind".to_string(),
                })?;
                if is_flag_shaped(&value) {
                    return Err(ConfigError::FlagShapedValue {
                        flag: "--bind".to_string(),
                        value,
                    });
                }
                options.bind = Some(value);
            }
            "--home" => {
                if options.home.is_some() {
                    return Err(ConfigError::DuplicateArgument {
                        flag: "--home".to_string(),
                    });
                }
                let value = iter.next().ok_or(ConfigError::MissingArgumentValue {
                    flag: "--home".to_string(),
                })?;
                if is_flag_shaped(&value) {
                    return Err(ConfigError::FlagShapedValue {
                        flag: "--home".to_string(),
                        value,
                    });
                }
                options.home = Some(PathBuf::from(value));
            }
            "--config" => {
                if options.config.is_some() {
                    return Err(ConfigError::DuplicateArgument {
                        flag: "--config".to_string(),
                    });
                }
                let value = iter.next().ok_or(ConfigError::MissingArgumentValue {
                    flag: "--config".to_string(),
                })?;
                if is_flag_shaped(&value) {
                    return Err(ConfigError::FlagShapedValue {
                        flag: "--config".to_string(),
                        value,
                    });
                }
                options.config = Some(PathBuf::from(value));
            }
            "--test-mode" => {
                if options.test_mode {
                    return Err(ConfigError::DuplicateArgument {
                        flag: "--test-mode".to_string(),
                    });
                }
                options.test_mode = true;
            }
            "--network-mode" => {
                if options.network_mode.is_some() {
                    return Err(ConfigError::DuplicateArgument {
                        flag: "--network-mode".to_string(),
                    });
                }
                let value = iter.next().ok_or(ConfigError::MissingArgumentValue {
                    flag: "--network-mode".to_string(),
                })?;
                if is_flag_shaped(&value) {
                    return Err(ConfigError::FlagShapedValue {
                        flag: "--network-mode".to_string(),
                        value,
                    });
                }
                options.network_mode = Some(value);
            }
            "--tls-cert" | "--tls-key" => {
                let target = if arg == "--tls-cert" {
                    &mut options.tls_cert
                } else {
                    &mut options.tls_key
                };
                if target.is_some() {
                    return Err(ConfigError::DuplicateArgument { flag: arg });
                }
                let value = iter
                    .next()
                    .ok_or_else(|| ConfigError::MissingArgumentValue { flag: arg.clone() })?;
                if is_flag_shaped(&value) {
                    return Err(ConfigError::FlagShapedValue { flag: arg, value });
                }
                *target = Some(PathBuf::from(value));
            }
            "--shutdown-timeout-ms" => {
                if options.shutdown_timeout_ms.is_some() {
                    return Err(ConfigError::DuplicateArgument {
                        flag: "--shutdown-timeout-ms".to_string(),
                    });
                }
                let value = iter.next().ok_or(ConfigError::MissingArgumentValue {
                    flag: "--shutdown-timeout-ms".to_string(),
                })?;
                if is_flag_shaped(&value) {
                    return Err(ConfigError::FlagShapedValue {
                        flag: "--shutdown-timeout-ms".to_string(),
                        value,
                    });
                }
                let parsed: u64 = match value.parse::<u64>() {
                    Ok(parsed) => parsed,
                    Err(_) => {
                        return Err(ConfigError::InvalidShutdownTimeout { value });
                    }
                };
                if parsed == 0 || parsed > MAX_TIME_BUDGET_MS {
                    return Err(ConfigError::InvalidShutdownTimeout { value });
                }
                options.shutdown_timeout_ms = Some(parsed);
            }
            // ── Resource-limit flags (R02-T07): strict positive integers ──
            // R3-F02: every flag has a documented INCLUSIVE supported range
            // and a checked conversion into its consumer's native type —
            // an out-of-range value is this same loud InvalidLimit (exit 2),
            // never a truncation (`as u32`), a wrap (`as i64`), a saturation
            // (`saturating_mul`), or a panic deep inside tokio.
            "--max-ws-connections" => {
                if options.max_ws_connections.is_some() {
                    return Err(ConfigError::DuplicateArgument {
                        flag: "--max-ws-connections".to_string(),
                    });
                }
                options.max_ws_connections = Some(parse_limit_usize(
                    &mut iter,
                    "--max-ws-connections",
                    "concurrently-upgraded WebSocket connections; supported range \
                     1..=usize::MAX (a pure count ceiling — nothing is allocated \
                     per configured slot)",
                    1,
                    usize::MAX as u64,
                )?);
            }
            "--db-queue-bound" => {
                if options.db_queue_bound.is_some() {
                    return Err(ConfigError::DuplicateArgument {
                        flag: "--db-queue-bound".to_string(),
                    });
                }
                options.db_queue_bound = Some(parse_limit_usize(
                    &mut iter,
                    "--db-queue-bound",
                    "pending single-writer DB queue jobs; supported range \
                     1..=MAX_QUEUE_CAPACITY (tokio's bounded-channel semaphore \
                     maximum — more would PANIC inside mpsc::channel; mirrored \
                     by StoreOptions::validate at the library layer)",
                    1,
                    MAX_QUEUE_CAPACITY as u64,
                )?);
            }
            "--event-subscriber-queue" => {
                if options.event_subscriber_queue.is_some() {
                    return Err(ConfigError::DuplicateArgument {
                        flag: "--event-subscriber-queue".to_string(),
                    });
                }
                options.event_subscriber_queue = Some(parse_limit_usize(
                    &mut iter,
                    "--event-subscriber-queue",
                    "per-subscriber event mailbox frames; supported range \
                     2..=usize::MAX (one event slot + one reserved signal slot; \
                     the mailbox is a lazily-grown VecDeque, so a large ceiling \
                     allocates nothing eagerly)",
                    2,
                    usize::MAX as u64,
                )?);
            }
            "--event-reorder-bound" => {
                if options.event_reorder_bound.is_some() {
                    return Err(ConfigError::DuplicateArgument {
                        flag: "--event-reorder-bound".to_string(),
                    });
                }
                options.event_reorder_bound = Some(parse_limit_usize(
                    &mut iter,
                    "--event-reorder-bound",
                    "per-stream event reorder buffer entries; supported range \
                     1..=usize::MAX (entries are allocated per published event, \
                     not per configured slot)",
                    1,
                    usize::MAX as u64,
                )?);
            }
            "--max-subscribers" => {
                if options.max_subscribers.is_some() {
                    return Err(ConfigError::DuplicateArgument {
                        flag: "--max-subscribers".to_string(),
                    });
                }
                options.max_subscribers = Some(parse_limit_usize(
                    &mut iter,
                    "--max-subscribers",
                    "concurrently live event subscribers; supported range \
                     1..=usize::MAX (a registry count ceiling — entries are \
                     allocated per live subscriber)",
                    1,
                    usize::MAX as u64,
                )?);
            }
            "--log-max-bytes" => {
                if options.log_max_bytes.is_some() {
                    return Err(ConfigError::DuplicateArgument {
                        flag: "--log-max-bytes".to_string(),
                    });
                }
                // R02-T07 REVIEW_R1 F04: the declared lower bound is enforced
                // HERE, at parse time, exactly like the other limit flags —
                // a below-range value is a loud exit-2 startup error, not a
                // pass-through that only fails (or silently degrades) later
                // at log-attach time. The bound mirrors
                // logging::LogRotationConfig::validate (defense in depth).
                // u64 is the consumer's native type (a file-size ceiling), so
                // no upper bound is needed beyond the type itself.
                options.log_max_bytes = Some(parse_limit_u64(
                    &mut iter,
                    "--log-max-bytes",
                    "bytes per log file before rotation; supported range \
                     64..=u64::MAX (one diagnostic line must always fit)",
                    64,
                    u64::MAX,
                )?);
            }
            "--log-max-files" => {
                if options.log_max_files.is_some() {
                    return Err(ConfigError::DuplicateArgument {
                        flag: "--log-max-files".to_string(),
                    });
                }
                // R02-T07 REVIEW_R1 F04: parse-time lower bound (rotation
                // needs a successor file), same exit-2 semantics.
                options.log_max_files = Some(parse_limit_usize(
                    &mut iter,
                    "--log-max-files",
                    "log files kept on disk; supported range 2..=usize::MAX \
                     (rotation needs a successor; pruning walks EXISTING files, \
                     so a large ceiling allocates nothing)",
                    2,
                    usize::MAX as u64,
                )?);
            }
            "--http-rate-max" => {
                if options.http_rate_max.is_some() {
                    return Err(ConfigError::DuplicateArgument {
                        flag: "--http-rate-max".to_string(),
                    });
                }
                // R3-F02: the rate limiter counts in u32 — the pre-fix
                // `as u32` cast silently truncated 4294967296 to 0 (every
                // request then 429'd). The range IS the consumer's type.
                options.http_rate_max = Some(parse_limit_u32(
                    &mut iter,
                    "--http-rate-max",
                    "per-peer HTTP requests per rate window; supported range \
                     1..=4294967295 (the limiter's native u32 counter)",
                    1,
                    u32::MAX as u64,
                )?);
            }
            "--http-max-in-flight" => {
                if options.http_max_in_flight.is_some() {
                    return Err(ConfigError::DuplicateArgument {
                        flag: "--http-max-in-flight".to_string(),
                    });
                }
                options.http_max_in_flight = Some(parse_limit_usize(
                    &mut iter,
                    "--http-max-in-flight",
                    "concurrently in-flight HTTP requests; supported range \
                     1..=usize::MAX/2 (the composition root derives the \
                     transport connection cap as 2x this value with a CHECKED \
                     multiplication — the bound keeps the derivation \
                     overflow-free instead of saturating silently)",
                    1,
                    MAX_HTTP_MAX_IN_FLIGHT,
                )?);
            }
            "--http-request-budget-ms" => {
                if options.http_request_budget_ms.is_some() {
                    return Err(ConfigError::DuplicateArgument {
                        flag: "--http-request-budget-ms".to_string(),
                    });
                }
                options.http_request_budget_ms = Some(parse_limit_u64(
                    &mut iter,
                    "--http-request-budget-ms",
                    "per-request wall-clock budget in ms; supported range \
                     1..=MAX_TIME_BUDGET_MS (30 days — larger deadlines risk \
                     overflowing platform monotonic-clock arithmetic)",
                    1,
                    MAX_TIME_BUDGET_MS,
                )?);
            }
            "--db-wait-budget-ms" => {
                if options.db_wait_budget_ms.is_some() {
                    return Err(ConfigError::DuplicateArgument {
                        flag: "--db-wait-budget-ms".to_string(),
                    });
                }
                options.db_wait_budget_ms = Some(parse_limit_u64(
                    &mut iter,
                    "--db-wait-budget-ms",
                    "DB queue wait budget in ms; supported range \
                     1..=MAX_TIME_BUDGET_MS (30 days; never exceeds the \
                     library StoreOptions bound — compile-time asserted)",
                    1,
                    MAX_TIME_BUDGET_MS,
                )?);
            }
            other => {
                return Err(ConfigError::UnknownArgument {
                    value: other.to_string(),
                })
            }
        }
    }
    match (&options.tls_cert, &options.tls_key) {
        (None, None) => {}
        (Some(cert), Some(key)) if cert.is_absolute() && key.is_absolute() => {}
        (Some(_), Some(_)) => {
            return Err(ConfigError::BadTlsConfig {
                detail: "--tls-cert and --tls-key must be absolute paths".into(),
            })
        }
        _ => {
            return Err(ConfigError::BadTlsConfig {
                detail: "--tls-cert and --tls-key must be provided together".into(),
            })
        }
    }
    Ok(options)
}

/// Strictly parses one limit-flag value into u64 within the inclusive
/// range `[min, max]` (R02 stage-repair R3 / R3-F02 — every resource flag
/// has a documented supported range): exactly the next token, not
/// flag-shaped, a decimal integer inside the range. Any deviation is a
/// loud [`ConfigError::InvalidLimit`] (or the shared missing/flag-shaped
/// errors) — never a silent default, truncation, wrap or saturation.
fn parse_limit_u64<I>(
    iter: &mut I,
    flag: &str,
    constraint: &str,
    min: u64,
    max: u64,
) -> Result<u64, ConfigError>
where
    I: Iterator<Item = String>,
{
    let value = iter
        .next()
        .ok_or_else(|| ConfigError::MissingArgumentValue {
            flag: flag.to_string(),
        })?;
    if is_flag_shaped(&value) {
        return Err(ConfigError::FlagShapedValue {
            flag: flag.to_string(),
            value,
        });
    }
    let parsed = value
        .parse::<u64>()
        .map_err(|_| ConfigError::InvalidLimit {
            flag: flag.to_string(),
            value: value.clone(),
            constraint: constraint.to_string(),
        })?;
    if parsed < min || parsed > max {
        return Err(ConfigError::InvalidLimit {
            flag: flag.to_string(),
            value,
            constraint: constraint.to_string(),
        });
    }
    Ok(parsed)
}

/// Same as [`parse_limit_u64`] plus a CHECKED conversion into `usize`:
/// on a 64-bit platform every value up to `usize::MAX as u64` converts
/// losslessly; on a 32-bit platform values above the 32-bit range are
/// rejected loudly here (the conversion can never truncate — the 32-bit
/// behaviour is correct by construction, not by a faked cross-platform
/// test).
fn parse_limit_usize<I>(
    iter: &mut I,
    flag: &str,
    constraint: &str,
    min: u64,
    max: u64,
) -> Result<usize, ConfigError>
where
    I: Iterator<Item = String>,
{
    let parsed = parse_limit_u64(iter, flag, constraint, min, max)?;
    usize::try_from(parsed).map_err(|_| ConfigError::InvalidLimit {
        flag: flag.to_string(),
        value: parsed.to_string(),
        constraint: constraint.to_string(),
    })
}

/// Same as [`parse_limit_u64`] plus a CHECKED conversion into `u32` (the
/// rate limiter's native counter type; the pre-fix `as u32` truncation of
/// `4294967296` to `0` — a permanent 429 for every peer — is the R3-F02
/// case this rejects loudly instead).
fn parse_limit_u32<I>(
    iter: &mut I,
    flag: &str,
    constraint: &str,
    min: u64,
    max: u64,
) -> Result<u32, ConfigError>
where
    I: Iterator<Item = String>,
{
    let parsed = parse_limit_u64(iter, flag, constraint, min, max)?;
    u32::try_from(parsed).map_err(|_| ConfigError::InvalidLimit {
        flag: flag.to_string(),
        value: parsed.to_string(),
        constraint: constraint.to_string(),
    })
}

/// Reads the `home` value from a strict JSON config file
/// (`{"home": "/absolute/path"}`). Anything else — missing file, unparsable
/// JSON, missing `home` key, non-string value, unknown extra keys — is a
/// loud error: the caller pointed at this file explicitly, so silently
/// skipping it would be a silent downgrade.
pub fn read_config_home(path: &Path) -> Result<PathBuf, ConfigError> {
    let raw =
        std::fs::read_to_string(path).map_err(|source| ConfigError::ConfigFileUnreadable {
            path: path.to_path_buf(),
            source: source.to_string(),
        })?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|source| ConfigError::ConfigFileInvalid {
            path: path.to_path_buf(),
            detail: format!("not valid JSON: {source}"),
        })?;
    let detail_of = |detail: String| ConfigError::ConfigFileInvalid {
        path: path.to_path_buf(),
        detail,
    };
    let object = value
        .as_object()
        .ok_or_else(|| detail_of("top level must be a JSON object".to_string()))?;
    if object.len() != 1 || !object.contains_key("home") {
        return Err(detail_of(format!(
            "expected exactly one key \"home\", got {}",
            object
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let home = object["home"].as_str().ok_or_else(|| {
        detail_of("\"home\" must be a string containing an absolute path".to_string())
    })?;
    Ok(PathBuf::from(home))
}

/// Builds a unique synthetic home directory name for test mode.
///
/// Uniqueness mixes the pid, a per-process monotonically increasing
/// counter, and nanosecond time: two calls in the same process never
/// collide — the counter alone guarantees that even when clock
/// resolution collapses back-to-back calls into the same nanosecond
/// (observed in practice on loaded hosts). No random source is required
/// (zero new dependencies). The *contents* of this name never feed any
/// other path — it becomes the home itself, nothing is appended to user
/// strings.
pub fn test_mode_home_name(pid: u32) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("lingxi-service-test-{pid}-{seq}-{nanos:x}")
}

/// Resolves the effective data root per the documented precedence:
/// test-mode > CLI `--home` > env `LINGXI_HOME` > config-file `home`.
///
/// The config file is read and validated EAGERLY whenever `--config` is
/// present, even when a higher-precedence source wins: explicitly provided
/// input is never silently skipped (a broken config + valid `--home` is an
/// error, not a shadowed warning).
///
/// `env_home` and `temp_base` are injected by the caller (the binary passes
/// `std::env::var` and `std::env::temp_dir()`; tests pass synthetic values)
/// so this function itself never touches process state.
pub fn resolve_effective_home(
    cli: &CliOptions,
    env_home: Option<&str>,
    temp_base: &Path,
) -> Result<ResolvedHome, ConfigError> {
    // Collect the ordinary sources first so ignored ones can be reported
    // even (especially) when test mode overrides them.
    let config_home = match &cli.config {
        Some(path) => Some(read_config_home(path)?),
        None => None,
    };

    if cli.test_mode {
        let path = temp_base.join(test_mode_home_name(std::process::id()));
        let mut ignored = Vec::new();
        if let Some(path) = cli.home.clone() {
            ignored.push(IgnoredHomeSource {
                origin: HomeSource::Cli,
                path,
            });
        }
        if let Some(value) = env_home {
            ignored.push(IgnoredHomeSource {
                origin: HomeSource::Env,
                path: PathBuf::from(value),
            });
        }
        if let Some(path) = config_home {
            ignored.push(IgnoredHomeSource {
                origin: HomeSource::ConfigFile,
                path,
            });
        }
        return Ok(ResolvedHome {
            path,
            source: HomeSource::TestMode,
            ignored,
        });
    }

    if let Some(path) = cli.home.clone() {
        return Ok(ResolvedHome {
            path,
            source: HomeSource::Cli,
            ignored: collect_ignored(env_home, config_home),
        });
    }
    if let Some(value) = env_home {
        return Ok(ResolvedHome {
            path: PathBuf::from(value),
            source: HomeSource::Env,
            ignored: collect_ignored(None, config_home),
        });
    }
    if let Some(path) = config_home {
        return Ok(ResolvedHome {
            path,
            source: HomeSource::ConfigFile,
            ignored: Vec::new(),
        });
    }
    Err(ConfigError::MissingHome)
}

fn collect_ignored(env_home: Option<&str>, config_home: Option<PathBuf>) -> Vec<IgnoredHomeSource> {
    let mut ignored = Vec::new();
    if let Some(value) = env_home {
        ignored.push(IgnoredHomeSource {
            origin: HomeSource::Env,
            path: PathBuf::from(value),
        });
    }
    if let Some(path) = config_home {
        ignored.push(IgnoredHomeSource {
            origin: HomeSource::ConfigFile,
            path,
        });
    }
    ignored
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    // ---- strict CLI parsing (REVIEW_R1 F03 closure) ----

    #[test]
    fn cli_rejects_duplicate_home() {
        let err = parse_cli(args(&["--home", "/a", "--home", "/b"])).unwrap_err();
        assert_eq!(
            err,
            ConfigError::DuplicateArgument {
                flag: "--home".to_string()
            }
        );
    }

    #[test]
    fn cli_rejects_duplicate_bind_and_config_and_test_mode() {
        assert!(matches!(
            parse_cli(args(&["--bind", "127.0.0.1:0", "--bind", "127.0.0.1:1"])),
            Err(ConfigError::DuplicateArgument { flag }) if flag == "--bind"
        ));
        assert!(matches!(
            parse_cli(args(&["--config", "/a.json", "--config", "/b.json"])),
            Err(ConfigError::DuplicateArgument { flag }) if flag == "--config"
        ));
        assert!(matches!(
            parse_cli(args(&["--test-mode", "--test-mode"])),
            Err(ConfigError::DuplicateArgument { flag }) if flag == "--test-mode"
        ));
    }

    #[test]
    fn cli_rejects_flag_shaped_values_instead_of_consuming_them() {
        // T01 REVIEW_R1 F03 exact case: previously consumed as the bind
        // value and surfaced as a confusing BadBind; now explicit.
        let err = parse_cli(args(&["--bind", "--home", "/x"])).unwrap_err();
        assert_eq!(
            err,
            ConfigError::FlagShapedValue {
                flag: "--bind".to_string(),
                value: "--home".to_string(),
            }
        );
        let err = parse_cli(args(&["--home", "--bind"])).unwrap_err();
        assert!(matches!(err, ConfigError::FlagShapedValue { .. }));
        let err = parse_cli(args(&["--config", "--test-mode"])).unwrap_err();
        assert!(matches!(err, ConfigError::FlagShapedValue { .. }));
    }

    #[test]
    fn cli_rejects_equals_form_and_unknown_tokens() {
        let err = parse_cli(args(&["--home=/tmp/x"])).unwrap_err();
        assert_eq!(
            err,
            ConfigError::UnknownArgument {
                value: "--home=/tmp/x".to_string(),
            }
        );
        assert!(matches!(
            parse_cli(args(&["--lan"])),
            Err(ConfigError::UnknownArgument { .. })
        ));
        assert!(matches!(
            parse_cli(args(&["positional"])),
            Err(ConfigError::UnknownArgument { .. })
        ));
    }

    #[test]
    fn cli_rejects_missing_values() {
        assert!(matches!(
            parse_cli(args(&["--home"])),
            Err(ConfigError::MissingArgumentValue { .. })
        ));
        assert!(matches!(
            parse_cli(args(&["--bind"])),
            Err(ConfigError::MissingArgumentValue { .. })
        ));
        assert!(matches!(
            parse_cli(args(&["--config"])),
            Err(ConfigError::MissingArgumentValue { .. })
        ));
    }

    #[test]
    fn cli_parses_all_flags() {
        let options = parse_cli(args(&[
            "--bind",
            "127.0.0.1:8080",
            "--home",
            "/tmp/h",
            "--config",
            "/tmp/c.json",
            "--test-mode",
            "--network-mode",
            "lan",
        ]))
        .unwrap();
        assert_eq!(options.bind.as_deref(), Some("127.0.0.1:8080"));
        assert_eq!(options.home, Some(PathBuf::from("/tmp/h")));
        assert_eq!(options.config, Some(PathBuf::from("/tmp/c.json")));
        assert!(options.test_mode);
        assert_eq!(options.network_mode.as_deref(), Some("lan"));
    }

    #[test]
    fn cli_network_mode_strictness() {
        // Duplicate / flag-shaped / missing value follow the same strict
        // rules as every value flag.
        assert!(matches!(
            parse_cli(args(&["--network-mode", "lan", "--network-mode", "loopback"])),
            Err(ConfigError::DuplicateArgument { flag }) if flag == "--network-mode"
        ));
        assert!(matches!(
            parse_cli(args(&["--network-mode", "--home"])),
            Err(ConfigError::FlagShapedValue { .. })
        ));
        assert!(matches!(
            parse_cli(args(&["--network-mode"])),
            Err(ConfigError::MissingArgumentValue { .. })
        ));
        assert!(matches!(
            parse_cli(args(&["--network-mode=lan"])),
            Err(ConfigError::UnknownArgument { .. })
        ));
    }

    #[test]
    fn tls_certificate_and_key_are_explicit_absolute_pairs() {
        assert!(matches!(
            parse_cli(args(&["--tls-cert", "/tmp/cert.pem"])),
            Err(ConfigError::BadTlsConfig { .. })
        ));
        assert!(matches!(
            parse_cli(args(&["--tls-key", "/tmp/key.pem"])),
            Err(ConfigError::BadTlsConfig { .. })
        ));
        assert!(matches!(
            parse_cli(args(&[
                "--tls-cert",
                "cert.pem",
                "--tls-key",
                "/tmp/key.pem"
            ])),
            Err(ConfigError::BadTlsConfig { .. })
        ));
        assert!(matches!(
            parse_cli(args(&[
                "--tls-cert",
                "/tmp/cert.pem",
                "--tls-key",
                "--bind"
            ])),
            Err(ConfigError::FlagShapedValue { .. })
        ));
        assert!(matches!(
            parse_cli(args(&[
                "--tls-cert",
                "/tmp/cert.pem",
                "--tls-cert",
                "/tmp/other.pem"
            ])),
            Err(ConfigError::DuplicateArgument { .. })
        ));
        let parsed = parse_cli(args(&[
            "--tls-cert",
            "/tmp/cert.pem",
            "--tls-key",
            "/tmp/key.pem",
        ]))
        .unwrap();
        assert_eq!(parsed.tls_cert, Some(PathBuf::from("/tmp/cert.pem")));
        assert_eq!(parsed.tls_key, Some(PathBuf::from("/tmp/key.pem")));
    }

    #[test]
    fn cli_empty_is_no_flags_not_an_error() {
        let options = parse_cli(args(&[])).unwrap();
        assert_eq!(options, CliOptions::default());
    }

    #[test]
    fn cli_shutdown_timeout_strictness() {
        // R02-T06: explicit shutdown deadline, same strict rules as every
        // value flag plus a positive-integer value check.
        let options = parse_cli(args(&["--shutdown-timeout-ms", "250"])).unwrap();
        assert_eq!(options.shutdown_timeout_ms, Some(250));
        assert!(matches!(
            parse_cli(args(&["--shutdown-timeout-ms", "10", "--shutdown-timeout-ms", "20"])),
            Err(ConfigError::DuplicateArgument { flag }) if flag == "--shutdown-timeout-ms"
        ));
        assert!(matches!(
            parse_cli(args(&["--shutdown-timeout-ms", "--home"])),
            Err(ConfigError::FlagShapedValue { .. })
        ));
        assert!(matches!(
            parse_cli(args(&["--shutdown-timeout-ms"])),
            Err(ConfigError::MissingArgumentValue { .. })
        ));
        assert!(matches!(
            parse_cli(args(&["--shutdown-timeout-ms", "abc"])),
            Err(ConfigError::InvalidShutdownTimeout { .. })
        ));
        assert!(matches!(
            parse_cli(args(&["--shutdown-timeout-ms", "0"])),
            Err(ConfigError::InvalidShutdownTimeout { .. })
        ));
        assert!(matches!(
            parse_cli(args(&["--shutdown-timeout-ms=-5"])),
            Err(ConfigError::UnknownArgument { .. })
        ));
    }

    // ---- precedence resolution (acceptance R02-A04, resolution half) ----

    fn temp_base() -> PathBuf {
        std::env::temp_dir()
    }

    #[test]
    fn precedence_cli_wins_over_env_and_config() {
        // The config file is read eagerly whenever --config is given (an
        // explicitly provided source is never silently skipped, even when a
        // higher-precedence source wins the home), so it must be a real
        // strict-valid file here.
        let dir = std::env::temp_dir().join(format!(
            "lingxi-r02t02-cfg5-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("service.json");
        std::fs::write(&cfg, br#"{"home": "/tmp/r02t02-config"}"#).unwrap();

        let cli = parse_cli(args(&[
            "--home",
            "/tmp/r02t02-cli",
            "--config",
            cfg.to_str().unwrap(),
        ]))
        .unwrap();
        let resolved = resolve_effective_home(&cli, Some("/tmp/r02t02-env"), &temp_base()).unwrap();
        assert_eq!(resolved.path, PathBuf::from("/tmp/r02t02-cli"));
        assert_eq!(resolved.source, HomeSource::Cli);
        assert_eq!(
            resolved.ignored,
            vec![
                IgnoredHomeSource {
                    origin: HomeSource::Env,
                    path: PathBuf::from("/tmp/r02t02-env"),
                },
                IgnoredHomeSource {
                    origin: HomeSource::ConfigFile,
                    path: PathBuf::from("/tmp/r02t02-config"),
                },
            ]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn precedence_cli_wins_but_broken_config_is_still_loud() {
        // Explicit input is validated even when it loses precedence: a
        // broken --config plus a valid --home is a configuration error, not
        // a silent skip.
        let dir = std::env::temp_dir().join(format!(
            "lingxi-r02t02-cfg6-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("broken.json");
        std::fs::write(&cfg, r#"{"home": "/tmp/x", "extra": 1}"#).unwrap();

        let cli = parse_cli(args(&[
            "--home",
            "/tmp/r02t02-cli",
            "--config",
            cfg.to_str().unwrap(),
        ]))
        .unwrap();
        let err = resolve_effective_home(&cli, Some("/tmp/r02t02-env"), &temp_base()).unwrap_err();
        assert!(matches!(err, ConfigError::ConfigFileInvalid { .. }));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn precedence_env_wins_over_config() {
        // Write a synthetic config file (test's own temp scope).
        let dir = std::env::temp_dir().join(format!(
            "lingxi-r02t02-cfg-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("service.json");
        std::fs::write(&cfg, br#"{"home": "/tmp/r02t02-config"}"#).unwrap();

        let cli = parse_cli(args(&["--config", cfg.to_str().unwrap()])).unwrap();
        let resolved = resolve_effective_home(&cli, Some("/tmp/r02t02-env"), &temp_base()).unwrap();
        assert_eq!(resolved.path, PathBuf::from("/tmp/r02t02-env"));
        assert_eq!(resolved.source, HomeSource::Env);
        assert_eq!(
            resolved.ignored,
            vec![IgnoredHomeSource {
                origin: HomeSource::ConfigFile,
                path: PathBuf::from("/tmp/r02t02-config"),
            }]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn precedence_only_cli_only_env_only_config_and_none() {
        // only CLI
        let cli = parse_cli(args(&["--home", "/tmp/only-cli"])).unwrap();
        let r = resolve_effective_home(&cli, None, &temp_base()).unwrap();
        assert_eq!(
            (r.path.clone(), r.source),
            (PathBuf::from("/tmp/only-cli"), HomeSource::Cli)
        );

        // only env
        let cli = parse_cli(args(&[])).unwrap();
        let r = resolve_effective_home(&cli, Some("/tmp/only-env"), &temp_base()).unwrap();
        assert_eq!(
            (r.path.clone(), r.source),
            (PathBuf::from("/tmp/only-env"), HomeSource::Env)
        );

        // only config
        let dir = std::env::temp_dir().join(format!(
            "lingxi-r02t02-cfg2-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("service.json");
        std::fs::write(&cfg, br#"{"home": "/tmp/only-config"}"#).unwrap();
        let cli = parse_cli(args(&["--config", cfg.to_str().unwrap()])).unwrap();
        let r = resolve_effective_home(&cli, None, &temp_base()).unwrap();
        assert_eq!(
            (r.path.clone(), r.source),
            (PathBuf::from("/tmp/only-config"), HomeSource::ConfigFile)
        );

        // none
        let cli = parse_cli(args(&[])).unwrap();
        assert_eq!(
            resolve_effective_home(&cli, None, &temp_base()).unwrap_err(),
            ConfigError::MissingHome
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_mode_overrides_all_sources_and_is_unique_per_call() {
        // Real (strict-valid) config so the eager read succeeds; its home
        // must still be reported as ignored under test mode.
        let dir = std::env::temp_dir().join(format!(
            "lingxi-r02t02-cfg7-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("service.json");
        std::fs::write(&cfg, br#"{"home": "/tmp/prod-lookalike-config"}"#).unwrap();

        let cli = parse_cli(args(&[
            "--home",
            "/tmp/prod-lookalike-cli",
            "--config",
            cfg.to_str().unwrap(),
            "--test-mode",
        ]))
        .unwrap();
        let resolved =
            resolve_effective_home(&cli, Some("/tmp/prod-lookalike-env"), &temp_base()).unwrap();
        assert_eq!(resolved.source, HomeSource::TestMode);
        assert_ne!(resolved.path, PathBuf::from("/tmp/prod-lookalike-cli"));
        assert_ne!(resolved.path, PathBuf::from("/tmp/prod-lookalike-env"));
        assert_ne!(resolved.path, PathBuf::from("/tmp/prod-lookalike-config"));
        assert!(resolved.path.starts_with(temp_base()));
        let ignored: Vec<_> = resolved
            .ignored
            .iter()
            .map(|i| (i.origin, i.path.clone()))
            .collect();
        assert_eq!(
            ignored,
            vec![
                (HomeSource::Cli, PathBuf::from("/tmp/prod-lookalike-cli")),
                (HomeSource::Env, PathBuf::from("/tmp/prod-lookalike-env")),
                (
                    HomeSource::ConfigFile,
                    PathBuf::from("/tmp/prod-lookalike-config")
                ),
            ]
        );

        // Uniqueness: two resolutions in the same process never collide.
        let a = resolve_effective_home(&cli, None, &temp_base()).unwrap();
        let b = resolve_effective_home(&cli, None, &temp_base()).unwrap();
        assert_ne!(a.path, b.path, "test-mode homes must be unique per call");
        std::fs::remove_dir_all(&dir).ok();
    }

    // ---- config file strictness ----

    #[test]
    fn config_file_missing_is_loud() {
        let err =
            read_config_home(Path::new("/tmp/r02t02-definitely-missing-config.json")).unwrap_err();
        assert!(matches!(err, ConfigError::ConfigFileUnreadable { .. }));
    }

    #[test]
    fn config_file_rejects_unknown_keys_missing_home_and_bad_types() {
        let dir = std::env::temp_dir().join(format!("lingxi-r02t02-cfg3-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cases: Vec<(&str, &str)> = vec![
            ("unknown-key", r#"{"home": "/tmp/a", "port": 1}"#),
            ("missing-home", r#"{"dataRoot": "/tmp/a"}"#),
            ("empty-object", r#"{}"#),
            ("non-string-home", r#"{"home": 42}"#),
            ("not-json", "not json at all"),
            ("array-top-level", r#"["/tmp/a"]"#),
        ];
        for (name, content) in cases {
            let path = dir.join(format!("{name}.json"));
            std::fs::write(&path, content).unwrap();
            let err = read_config_home(&path).unwrap_err();
            assert!(
                matches!(err, ConfigError::ConfigFileInvalid { .. }),
                "{name}: expected ConfigFileInvalid, got {err:?}"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn config_file_reads_absolute_home() {
        let dir = std::env::temp_dir().join(format!("lingxi-r02t02-cfg4-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ok.json");
        std::fs::write(&path, br#"{"home": "/tmp/r02t02-from-config"}"#).unwrap();
        assert_eq!(
            read_config_home(&path).unwrap(),
            PathBuf::from("/tmp/r02t02-from-config")
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_mode_home_name_is_unique_and_prefixed() {
        // Tight-loop N generations: even when the clock never advances
        // between calls, the in-process counter must keep every name
        // distinct (guards the same-nanosecond collision seen in the wild).
        const N: usize = 512;
        let mut seen = std::collections::HashSet::with_capacity(N);
        for _ in 0..N {
            let name = test_mode_home_name(std::process::id());
            assert!(name.starts_with("lingxi-service-test-"));
            assert!(
                seen.insert(name.clone()),
                "duplicate test_mode_home_name: {name}"
            );
        }
        assert_eq!(seen.len(), N);
    }

    // ── R02-T07: resource-limit flags ───────────────────────────────────────

    #[test]
    fn limit_flags_parse_strictly() {
        let cli = parse_cli([
            "--home",
            "/tmp/h",
            "--max-ws-connections",
            "4",
            "--db-queue-bound",
            "2",
            "--event-subscriber-queue",
            "8",
            "--event-reorder-bound",
            "16",
            "--max-subscribers",
            "32",
            "--log-max-bytes",
            "1048576",
            "--log-max-files",
            "3",
        ])
        .expect("valid limits");
        assert_eq!(cli.max_ws_connections, Some(4));
        assert_eq!(cli.db_queue_bound, Some(2));
        assert_eq!(cli.event_subscriber_queue, Some(8));
        assert_eq!(cli.event_reorder_bound, Some(16));
        assert_eq!(cli.max_subscribers, Some(32));
        assert_eq!(cli.log_max_bytes, Some(1048576));
        assert_eq!(cli.log_max_files, Some(3));
    }

    // R02-T07 REVIEW_R1 F04: the declared lower bounds of the two log-rotation
    // flags are enforced at PARSE time (loud InvalidLimit, exit-2 path), the
    // same as every other limit flag — never "accepted here, failed (or
    // silently degraded) later at log-attach time".
    #[test]
    fn log_flag_range_violations_are_parse_time_errors() {
        // Below the declared bound -> rejected at parse.
        for (flag, value) in [
            ("--log-max-bytes", "1"),
            ("--log-max-bytes", "32"),
            ("--log-max-bytes", "63"),
            ("--log-max-files", "1"),
        ] {
            let err = parse_cli(["--home", "/tmp/h", flag, value]).unwrap_err();
            assert!(
                matches!(err, ConfigError::InvalidLimit { .. }),
                "{flag} {value:?}: expected InvalidLimit, got {err:?}"
            );
        }
        // The declared bounds themselves parse fine.
        let cli = parse_cli([
            "--home",
            "/tmp/h",
            "--log-max-bytes",
            "64",
            "--log-max-files",
            "2",
        ])
        .expect("declared lower bounds are accepted");
        assert_eq!(cli.log_max_bytes, Some(64));
        assert_eq!(cli.log_max_files, Some(2));
    }

    // ── R3-F02: every resource flag has a documented supported range ───────
    //
    // The matrix below is PARSE-ONLY: it proves the boundary values are
    // accepted and the out-of-range values rejected without ever allocating
    // a huge resource (parsing only inspects the number).

    #[test]
    fn limit_flag_ranges_accept_the_documented_boundaries() {
        let cli = parse_cli([
            "--home",
            "/tmp/h",
            "--max-ws-connections",
            "1",
            "--db-queue-bound",
            &MAX_QUEUE_CAPACITY.to_string(),
            "--event-subscriber-queue",
            "2",
            "--event-reorder-bound",
            "1",
            "--max-subscribers",
            "1",
            "--log-max-bytes",
            "64",
            "--log-max-files",
            "2",
            "--http-rate-max",
            &u32::MAX.to_string(),
            "--http-max-in-flight",
            &MAX_HTTP_MAX_IN_FLIGHT.to_string(),
            "--http-request-budget-ms",
            &MAX_TIME_BUDGET_MS.to_string(),
            "--db-wait-budget-ms",
            "1",
            "--shutdown-timeout-ms",
            &MAX_TIME_BUDGET_MS.to_string(),
        ])
        .expect("every documented boundary value must parse");
        assert_eq!(cli.max_ws_connections, Some(1));
        assert_eq!(cli.db_queue_bound, Some(MAX_QUEUE_CAPACITY));
        assert_eq!(cli.event_subscriber_queue, Some(2));
        assert_eq!(cli.event_reorder_bound, Some(1));
        assert_eq!(cli.max_subscribers, Some(1));
        assert_eq!(cli.log_max_bytes, Some(64));
        assert_eq!(cli.log_max_files, Some(2));
        assert_eq!(cli.http_rate_max, Some(u32::MAX));
        assert_eq!(
            cli.http_max_in_flight,
            Some(MAX_HTTP_MAX_IN_FLIGHT as usize)
        );
        assert_eq!(cli.http_request_budget_ms, Some(MAX_TIME_BUDGET_MS));
        assert_eq!(cli.db_wait_budget_ms, Some(1));
        assert_eq!(cli.shutdown_timeout_ms, Some(MAX_TIME_BUDGET_MS));
    }

    #[test]
    fn limit_flag_ranges_accept_native_maxima_losslessly() {
        // usize-native count ceilings accept the platform maximum (a pure
        // count ceiling allocates nothing per configured slot — documented
        // per flag); the conversion is a checked try_from, so on a 32-bit
        // platform the same test value would be REJECTED loudly instead of
        // truncating (32-bit correctness by construction).
        let native_max = (usize::MAX as u64).to_string();
        for flag in [
            "--max-ws-connections",
            "--event-reorder-bound",
            "--max-subscribers",
            "--log-max-files",
        ] {
            let cli = parse_cli(["--home", "/tmp/h", flag, &native_max])
                .unwrap_or_else(|err| panic!("{flag} {native_max}: {err}"));
            let parsed = match flag {
                "--max-ws-connections" => cli.max_ws_connections,
                "--event-reorder-bound" => cli.event_reorder_bound,
                "--max-subscribers" => cli.max_subscribers,
                "--log-max-files" => cli.log_max_files,
                _ => unreachable!(),
            };
            assert_eq!(parsed, Some(usize::MAX), "{flag} must convert losslessly");
        }
        // u64-native log byte ceiling takes the full u64 range.
        let cli = parse_cli(["--home", "/tmp/h", "--log-max-bytes", &u64::MAX.to_string()])
            .expect("u64::MAX is a valid log byte ceiling");
        assert_eq!(cli.log_max_bytes, Some(u64::MAX));
        // The event subscriber queue (min 2) also takes usize::MAX.
        let cli = parse_cli(["--home", "/tmp/h", "--event-subscriber-queue", &native_max])
            .expect("usize::MAX is a valid mailbox ceiling");
        assert_eq!(cli.event_subscriber_queue, Some(usize::MAX));
    }

    #[test]
    fn limit_flag_ranges_reject_above_the_documented_maxima() {
        let cases: Vec<(&str, String)> = vec![
            // The R3-F02 case-1 value: above tokio's semaphore maximum —
            // pre-fix this PANICKED with exit 101 inside mpsc::channel.
            (
                "--db-queue-bound",
                (MAX_QUEUE_CAPACITY as u64 + 1).to_string(),
            ),
            ("--db-queue-bound", u64::MAX.to_string()),
            // The R3-F02 case-2 value: u32::MAX + 1 — pre-fix `as u32`
            // truncated it to 0 (permanent 429 for every peer).
            ("--http-rate-max", "4294967296".to_string()),
            ("--http-rate-max", u64::MAX.to_string()),
            // Above usize::MAX/2 the 2x connection-cap derivation could
            // overflow — pre-fix it saturated silently.
            (
                "--http-max-in-flight",
                (MAX_HTTP_MAX_IN_FLIGHT + 1).to_string(),
            ),
            ("--http-max-in-flight", (usize::MAX as u64).to_string()),
            // Time budgets above 30 days risk overflowing platform
            // monotonic-clock arithmetic at runtime.
            (
                "--http-request-budget-ms",
                (MAX_TIME_BUDGET_MS + 1).to_string(),
            ),
            ("--db-wait-budget-ms", (MAX_TIME_BUDGET_MS + 1).to_string()),
            ("--db-wait-budget-ms", u64::MAX.to_string()),
        ];
        for (flag, value) in cases {
            let err = parse_cli(["--home", "/tmp/h", flag, &value]).unwrap_err();
            assert!(
                matches!(err, ConfigError::InvalidLimit { .. }),
                "{flag} {value}: expected InvalidLimit, got {err:?}"
            );
        }
        // --shutdown-timeout-ms keeps its dedicated variant, same loudness.
        let err = parse_cli([
            "--home",
            "/tmp/h",
            "--shutdown-timeout-ms",
            &(MAX_TIME_BUDGET_MS + 1).to_string(),
        ])
        .unwrap_err();
        assert!(
            matches!(err, ConfigError::InvalidShutdownTimeout { .. }),
            "expected InvalidShutdownTimeout, got {err:?}"
        );
    }

    #[test]
    fn limit_flag_ranges_reject_below_the_documented_minima() {
        for (flag, value) in [
            ("--max-ws-connections", "0"),
            ("--db-queue-bound", "0"),
            // The mailbox needs one event slot + one reserved signal slot.
            ("--event-subscriber-queue", "1"),
            ("--event-reorder-bound", "0"),
            ("--max-subscribers", "0"),
            ("--log-max-bytes", "63"),
            ("--log-max-files", "1"),
            ("--http-rate-max", "0"),
            ("--http-max-in-flight", "0"),
            ("--http-request-budget-ms", "0"),
            ("--db-wait-budget-ms", "0"),
        ] {
            let err = parse_cli(["--home", "/tmp/h", flag, value]).unwrap_err();
            assert!(
                matches!(err, ConfigError::InvalidLimit { .. }),
                "{flag} {value:?}: expected InvalidLimit, got {err:?}"
            );
        }
    }

    #[test]
    fn limit_flags_reject_garbage_zero_and_flag_shaped_values() {
        for (flag, value) in [
            ("--max-ws-connections", "zero"),
            ("--max-ws-connections", "0"),
            ("--max-ws-connections", "-1"),
            ("--db-queue-bound", "0"),
            ("--event-subscriber-queue", "abc"),
            ("--event-reorder-bound", "0x10"),
            ("--max-subscribers", ""),
            ("--log-max-bytes", "0"),
            ("--log-max-files", "1.5"),
        ] {
            let err = parse_cli(["--home", "/tmp/h", flag, value]).unwrap_err();
            assert!(
                matches!(err, ConfigError::InvalidLimit { .. }),
                "{flag} {value:?}: expected InvalidLimit, got {err:?}"
            );
        }
        // Flag-shaped values are still rejected.
        let err = parse_cli([
            "--home",
            "/tmp/h",
            "--max-ws-connections",
            "--db-queue-bound",
        ])
        .unwrap_err();
        assert!(
            matches!(err, ConfigError::FlagShapedValue { .. }),
            "{err:?}"
        );
        // Missing values too.
        let err = parse_cli(["--home", "/tmp/h", "--max-subscribers"]).unwrap_err();
        assert!(
            matches!(err, ConfigError::MissingArgumentValue { .. }),
            "{err:?}"
        );
        // Duplicates too.
        let err = parse_cli([
            "--home",
            "/tmp/h",
            "--max-subscribers",
            "2",
            "--max-subscribers",
            "3",
        ])
        .unwrap_err();
        assert!(
            matches!(err, ConfigError::DuplicateArgument { .. }),
            "{err:?}"
        );
    }
}
