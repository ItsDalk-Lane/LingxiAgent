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
//! This module deliberately takes the environment value and temp base as
//! *parameters* instead of calling `std::env` itself, so tests inject values
//! without polluting the outer process environment.

use std::fmt;
use std::path::{Path, PathBuf};

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
            other => {
                return Err(ConfigError::UnknownArgument {
                    value: other.to_string(),
                })
            }
        }
    }
    Ok(options)
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
/// Uniqueness mixes the pid with nanosecond time: two calls in the same
/// process never collide, and no random source is required (zero new
/// dependencies). The *contents* of this name never feed any other path —
/// it becomes the home itself, nothing is appended to user strings.
pub fn test_mode_home_name(pid: u32) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("lingxi-service-test-{pid}-{nanos:x}")
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
    fn cli_empty_is_no_flags_not_an_error() {
        let options = parse_cli(args(&[])).unwrap();
        assert_eq!(options, CliOptions::default());
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
        let a = test_mode_home_name(std::process::id());
        let b = test_mode_home_name(std::process::id());
        assert_ne!(a, b);
        assert!(a.starts_with("lingxi-service-test-"));
    }
}
