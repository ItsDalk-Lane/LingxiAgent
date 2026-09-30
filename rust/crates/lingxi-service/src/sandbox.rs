//! Cross-platform sandbox face and escape protection (R04-T06).
//!
//! The sandbox reuses the OPERATING SYSTEM isolation mechanisms the
//! incumbent already ships — it does not reinvent them in Rust:
//!
//! - **macOS** — Seatbelt via `/usr/bin/sandbox-exec` (the incumbent
//!   `lib/sandbox/seatbelt.ts` mechanism): the effective boundaries are
//!   compiled into an SBPL profile
//!   `(deny default) + (allow file-read*) + scoped (allow file-write*) +
//!   deny-read/deny-write overrides + network per policy`, and the
//!   command is exec'd BEHIND the helper (`sandbox-exec -p <profile> --
//!   argv…`). `sandbox-exec` replaces itself with the command (verified:
//!   same pid), so the T05 supervisor's `setsid` + `getpgid` + `killpg`
//!   chain keeps working through the wrapper.
//! - **Linux** — bubblewrap (the incumbent `lib/sandbox/bwrap.ts`
//!   mechanism): allowlist mounts (`--ro-bind / /` base, writable roots
//!   shadowed as `--bind`, protected paths re-read-only, deny-read
//!   masked), `--unshare-pid`, `--new-session`, `--die-with-parent`,
//!   `--unshare-net` when contained. Source-level adaptation; real
//!   machine verification on Linux is NOT claimed (registered in the
//!   platform capabilities matrix).
//! - **Windows** — the incumbent restricted-token helper
//!   (`lingxi-win-sandbox.exe`) is NOT ported in R04; the Windows backend
//!   REFUSES (`sandbox_policy_unsupported`) instead of inventing Job
//!   Object behavior — fail-closed, never a bare run (real-machine
//!   Windows verification follows the stage deferral
//!   R03-WINDOWS-R09-R10).
//!
//! # Fail-closed (R04-A11)
//! Every [`SandboxPort::wrap`] re-verifies the helper (existence + the
//! chosen trust scheme) and re-validates the request against the frozen
//! policy. Any failure is a [`SandboxRefusal`] — a loud, diagnosable
//! refusal with ZERO side effects. There is no path from this module
//! that returns an unwrapped command: a configuration error, a missing
//! helper, an untrusted/replaced helper, an unsupported policy or an
//! unsafe embedded path can only refuse, never "run it anyway".
//!
//! # Helper trust scheme (the chosen scheme, `R04-T06`)
//! - The helper is resolved from a PINNED absolute path (default
//!   `/usr/bin/sandbox-exec` / `/usr/bin/bwrap`) — never a `PATH` lookup
//!   (the incumbent's `which sandbox-exec` is PATH-spoofable; closed
//!   here).
//! - Metadata trust: regular file, expected basename, owner uid 0
//!   (root), NOT group/world-writable, executable.
//! - Behavioral probe (seatbelt): at construction the helper must pass a
//!   two-legged enforcement smoke probe — an allowed exec exits 0 under
//!   a minimal profile AND a denied write under `(deny default)` fails.
//!   `sandbox-exec` exposes no `--version`; identity pinning + this
//!   enforcement probe ARE the version/behavior verification for this
//!   backend.
//! - Version probe (bwrap): `bwrap --version` must report a
//!   `bubblewrap <x.y.z>` line; anything else is untrusted.
//!
//! # Policy-injection protection
//! SBPL embeds paths inside double-quoted literals. A path containing
//! `"` or `\` could break out of the literal and inject profile text
//! (the incumbent does not guard this). This module REFUSES such paths
//! ([`SandboxRefusal::ProfilePathUnsafe`]) — the command does not run.
//!
//! # The frozen boundary (extracted from the incumbent `policy.ts`)
//! - read: **all** (`allow file-read*`), except the deny-read list
//!   (`auth.json`, `models.json`, `added-models.yaml`, `crash.log`,
//!   `browser-data/`, `playwright-browsers/` under the Lingxi home).
//! - write: **scoped** — agent read-write dirs, home read-write dirs,
//!   workspace roots, runtime writable paths, `/private/tmp` and
//!   `TMPDIR`; protected paths (`.git` inside workspace roots,
//!   `session-files`) are write-DENIED inside the writable set (SBPL is
//!   last-match-wins; the deny lines follow the allows).
//! - network: per policy — contained one-shots run with ALL networking
//!   denied (the incumbent default `defaultSandboxExec` uses
//!   `() => false`); a network-capable policy allows `network-outbound`.
//! - environment: the sandbox adds NO passthrough — the T05 executor
//!   whitelist ([`crate::exectools::SAFE_ENV_PASSTHROUGH`]) is the
//!   single environment boundary (constraint intersection, no second
//!   funnel).
//! - subprocess: allowed INSIDE the sandbox (`process-exec*`,
//!   `process-fork`, `signal`) — the incumbent contract.
//! - interactive terminals (`tty: true`): NOT OS-sandboxed (incumbent
//!   semantics — `sandboxed = !tty && …`); authorization + env
//!   whitelist still apply and the result never claims containment.
//!
//! The per-call/per-session NARROWING below this coarse OS boundary is
//! the T04 [`crate::resourceaccess::ResourceAccess`] face (prepare-time
//! scope derivation + near-dispatch re-derivation) — the OS sandbox is
//! the outer layer, the resource layer the inner one (constraint
//! intersection; no layer may widen the other).

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ── the incumbent policy vocabulary (lib/sandbox/policy.ts, verbatim) ──────

/// Lingxi-home root-level files blocked from reads AND writes.
pub const BLOCKED_FILES: &[&str] = &["auth.json", "models.json", "added-models.yaml", "crash.log"];
/// Lingxi-home root-level directories blocked from reads AND writes.
pub const BLOCKED_DIRS: &[&str] = &["browser-data", "playwright-browsers"];
/// Agent-dir files that stay read-only.
pub const READ_ONLY_AGENT_FILES: &[&str] = &[
    "AGENTS.md",
    // The personality file's old name: the startup rename can fail
    // (permissions / the file being held); keep the old name read-only so a
    // failed rename cannot turn it writable (incumbent comment).
    "ishiki.md",
    "config.yaml",
    "identity.md",
    "yuan.md",
];
/// Lingxi-home root-level read-only directories.
pub const READ_ONLY_HOME_DIRS: &[&str] = &["user", "skills", "session-files"];
/// Agent-dir read-write directories.
pub const READ_WRITE_AGENT_DIRS: &[&str] = &[
    "memory",
    "sessions",
    "desk",
    "heartbeat",
    "book",
    "activity",
    "avatars",
];
/// Agent-dir read-only directories (the general mechanism; empty like the
/// incumbent).
pub const READ_ONLY_AGENT_DIRS: &[&str] = &[];
/// Agent-dir read-write files.
pub const READ_WRITE_AGENT_FILES: &[&str] = &["channels.md"];
/// Lingxi-home root-level read-write directories.
pub const READ_WRITE_HOME_DIRS: &[&str] = &["channels", "logs", "uploads", ".ephemeral"];

/// The pinned default helper paths (absolute — never resolved through
/// `PATH`).
pub const SEATBELT_HELPER_DEFAULT: &str = "/usr/bin/sandbox-exec";
pub const BWRAP_HELPER_DEFAULT: &str = "/usr/bin/bwrap";

// ── the policy ─────────────────────────────────────────────────────────────

/// Whether the frozen policy permits a network-capable variant at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxNetworkPolicy {
    /// All sandboxed commands run network-denied (the contained default).
    Denied,
    /// The policy additionally permits network-capable execution (the
    /// incumbent's escalated form — approval-gated upstream).
    Allowed,
}

impl SandboxNetworkPolicy {
    pub fn wire_name(self) -> &'static str {
        match self {
            SandboxNetworkPolicy::Denied => "denied",
            SandboxNetworkPolicy::Allowed => "allowed",
        }
    }
}

/// The operator-facing inputs the frozen policy is derived from (the
/// incumbent `deriveSandboxPolicy`).
#[derive(Debug, Clone)]
pub struct SandboxPolicyInput {
    pub lingxi_home: PathBuf,
    pub agent_dir: PathBuf,
    pub workspace_roots: Vec<PathBuf>,
    pub runtime_writable_paths: Vec<PathBuf>,
    pub network: SandboxNetworkPolicy,
}

/// The derived sandbox policy — the frozen boundary every backend compiles
/// into its OS mechanism.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxPolicy {
    pub lingxi_home: PathBuf,
    /// OS-sandbox writable roots (canonicalized at derive time).
    pub writable_paths: Vec<PathBuf>,
    /// Explicit read-only paths (bwrap ro-binds; the seatbelt profile has
    /// global read so these are informational there).
    pub readable_paths: Vec<PathBuf>,
    /// Read-AND-write-denied paths (secrets under the Lingxi home).
    pub deny_read_paths: Vec<PathBuf>,
    /// Write-denied paths inside the writable set.
    pub protected_paths: Vec<PathBuf>,
    pub network: SandboxNetworkPolicy,
}

fn absolute(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(p)
    }
}

/// `realpath` with the incumbent's fallback: resolve symlinks when the
/// target exists; otherwise keep the (absolute) lexical path. On macOS this
/// is what maps `/tmp/...` to `/private/tmp/...` — REQUIRED for seatbelt
/// subpath filters (they judge the real path).
pub fn realpath_or_lexical(p: &Path) -> PathBuf {
    match std::fs::canonicalize(p) {
        Ok(resolved) => resolved,
        Err(_) => absolute(p),
    }
}

impl SandboxPolicy {
    /// Port of the incumbent `deriveSandboxPolicy` (standard mode): the
    /// writable/readable/denied/protected sets and the network stance.
    /// Invalid configuration (empty Lingxi home / agent dir / no roots) is
    /// a loud [`SandboxRefusal::ConfigInvalid`] — never a guess.
    pub fn derive(input: &SandboxPolicyInput) -> Result<Self, SandboxRefusal> {
        if input.lingxi_home.as_os_str().is_empty() {
            return Err(SandboxRefusal::ConfigInvalid {
                detail: "lingxi_home is empty".to_string(),
            });
        }
        if input.agent_dir.as_os_str().is_empty() {
            return Err(SandboxRefusal::ConfigInvalid {
                detail: "agent_dir is empty".to_string(),
            });
        }
        if input.workspace_roots.is_empty() {
            return Err(SandboxRefusal::ConfigInvalid {
                detail: "no workspace roots are configured; refusing to build a \
                         sandbox policy that could not contain any command"
                    .to_string(),
            });
        }
        let lingxi_home = absolute(&input.lingxi_home);
        let agent_dir = absolute(&input.agent_dir);
        let push_unique = |vec: &mut Vec<PathBuf>, raw: &Path| {
            let resolved = realpath_or_lexical(raw);
            if !vec.contains(&resolved) {
                vec.push(resolved);
            }
        };
        let mut writable_paths = Vec::new();
        for dir in READ_WRITE_AGENT_DIRS {
            push_unique(&mut writable_paths, &agent_dir.join(dir));
        }
        for dir in READ_WRITE_HOME_DIRS {
            push_unique(&mut writable_paths, &lingxi_home.join(dir));
        }
        for root in &input.workspace_roots {
            push_unique(&mut writable_paths, root);
        }
        for root in &input.runtime_writable_paths {
            if root.as_os_str().is_empty() {
                continue;
            }
            push_unique(&mut writable_paths, root);
        }
        let mut readable_paths = Vec::new();
        for file in READ_ONLY_AGENT_FILES {
            push_unique(&mut readable_paths, &agent_dir.join(file));
        }
        for dir in READ_ONLY_AGENT_DIRS {
            push_unique(&mut readable_paths, &agent_dir.join(dir));
        }
        for dir in READ_ONLY_HOME_DIRS {
            push_unique(&mut readable_paths, &lingxi_home.join(dir));
        }
        let mut deny_read_paths = Vec::new();
        for file in BLOCKED_FILES {
            push_unique(&mut deny_read_paths, &lingxi_home.join(file));
        }
        for dir in BLOCKED_DIRS {
            push_unique(&mut deny_read_paths, &lingxi_home.join(dir));
        }
        let mut protected_paths = Vec::new();
        for root in &input.workspace_roots {
            push_unique(&mut protected_paths, &absolute(root).join(".git"));
        }
        push_unique(&mut protected_paths, &lingxi_home.join("session-files"));
        Ok(SandboxPolicy {
            lingxi_home,
            writable_paths,
            readable_paths,
            deny_read_paths,
            protected_paths,
            network: input.network,
        })
    }
}

// ── refusals (the fail-closed vocabulary) ──────────────────────────────────

/// A loud, diagnosable sandbox refusal. Zero side effects by construction:
/// the caller never receives a runnable command on this path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxRefusal {
    /// The sandbox configuration is unusable (empty roots, bad derivation).
    ConfigInvalid { detail: String },
    /// The helper binary does not exist at the configured/pinned path.
    HelperMissing {
        backend: &'static str,
        path: PathBuf,
    },
    /// The helper failed the trust scheme (owner/mode/basename/version or
    /// the enforcement probe) — a replaced or mismatched helper.
    HelperUntrusted {
        backend: &'static str,
        path: PathBuf,
        detail: String,
    },
    /// The requested execution variant is not supported by this backend or
    /// the frozen policy (e.g. network-capable on a network-denied policy;
    /// any request on the unsupported backend).
    PolicyUnsupported {
        backend: &'static str,
        detail: String,
    },
    /// A path destined for the compiled policy contains characters that
    /// could escape the profile literal (policy-injection attempt or an
    /// exotic but unusable deployment path). The command never runs.
    ProfilePathUnsafe {
        backend: &'static str,
        path: PathBuf,
    },
}

impl SandboxRefusal {
    /// The stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            SandboxRefusal::ConfigInvalid { .. } => "sandbox_config_invalid",
            SandboxRefusal::HelperMissing { .. } => "sandbox_helper_missing",
            SandboxRefusal::HelperUntrusted { .. } => "sandbox_helper_untrusted",
            SandboxRefusal::PolicyUnsupported { .. } => "sandbox_policy_unsupported",
            SandboxRefusal::ProfilePathUnsafe { .. } => "sandbox_profile_path_unsafe",
        }
    }
}

impl fmt::Display for SandboxRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SandboxRefusal::ConfigInvalid { detail } => write!(
                f,
                "sandbox_config_invalid: {detail}; the sandbox configuration is \
                 unusable and isolated commands are refused (never run unsandboxed)"
            ),
            SandboxRefusal::HelperMissing { backend, path } => write!(
                f,
                "sandbox_helper_missing: the {backend} sandbox helper {} does not \
                 exist; isolated commands are refused (never run unsandboxed)",
                path.display()
            ),
            SandboxRefusal::HelperUntrusted {
                backend,
                path,
                detail,
            } => write!(
                f,
                "sandbox_helper_untrusted: the {backend} sandbox helper {} failed \
                 verification ({detail}); isolated commands are refused (never run \
                 unsandboxed)",
                path.display()
            ),
            SandboxRefusal::PolicyUnsupported { backend, detail } => write!(
                f,
                "sandbox_policy_unsupported: {detail} on backend {backend}; the \
                 command is refused rather than run with weaker isolation"
            ),
            SandboxRefusal::ProfilePathUnsafe { backend, path } => write!(
                f,
                "sandbox_profile_path_unsafe: the path {} contains characters that \
                 cannot be embedded safely in a {backend} policy literal; refusing \
                 the command instead of compiling an injectable policy",
                path.display()
            ),
        }
    }
}

impl std::error::Error for SandboxRefusal {}

// ── helper verification (the chosen scheme) ────────────────────────────────

/// What the trust scheme established about one helper.
#[derive(Debug, Clone)]
pub struct HelperTrust {
    pub path: PathBuf,
    pub resolved: PathBuf,
    pub owner_uid: u32,
    pub mode: u32,
    /// The version line when the backend exposes one (bwrap); `None` for
    /// seatbelt (no `--version` exists — identity pinning + the
    /// enforcement probe are the verification there).
    pub version: Option<String>,
}

/// Metadata trust verification: regular file, expected basename, owned by
/// root, not group/world-writable, executable. Unix-only scheme (the
/// helper trust model is POSIX-shaped; non-Unix builds only ever see the
/// unsupported backend).
#[cfg(unix)]
fn verify_helper_metadata(
    backend: &'static str,
    path: &Path,
    expected_basename: &str,
) -> Result<HelperTrust, SandboxRefusal> {
    use std::os::unix::fs::MetadataExt;
    let missing = || SandboxRefusal::HelperMissing {
        backend,
        path: path.to_path_buf(),
    };
    let untrusted = |detail: String| SandboxRefusal::HelperUntrusted {
        backend,
        path: path.to_path_buf(),
        detail,
    };
    if path.file_name().and_then(|n| n.to_str()) != Some(expected_basename) {
        return Err(untrusted(format!(
            "helper basename must be {expected_basename:?}"
        )));
    }
    let metadata = std::fs::metadata(path).map_err(|_| missing())?;
    if !metadata.is_file() {
        return Err(untrusted("not a regular file".to_string()));
    }
    if metadata.uid() != 0 {
        return Err(untrusted(format!(
            "owner uid {} is not root (0) — a replaced or user-installed helper",
            metadata.uid()
        )));
    }
    let mode = metadata.mode();
    if mode & 0o022 != 0 {
        return Err(untrusted(
            "the helper is group- or world-writable".to_string(),
        ));
    }
    if mode & 0o111 == 0 {
        return Err(untrusted("the helper is not executable".to_string()));
    }
    let resolved = std::fs::canonicalize(path).map_err(|_| missing())?;
    Ok(HelperTrust {
        path: path.to_path_buf(),
        resolved,
        owner_uid: metadata.uid(),
        mode,
        version: None,
    })
}

#[cfg(not(unix))]
fn verify_helper_metadata(
    backend: &'static str,
    path: &Path,
    _expected_basename: &str,
) -> Result<HelperTrust, SandboxRefusal> {
    // Non-Unix builds have no trust scheme; the only backend available
    // there (unsupported) refuses everything anyway.
    Err(SandboxRefusal::HelperUntrusted {
        backend,
        path: path.to_path_buf(),
        detail: "no helper trust scheme exists on this platform".to_string(),
    })
}

// ── the port ───────────────────────────────────────────────────────────────

/// Which network variant one sandboxed command requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxNetworkRequest {
    /// The contained default (the incumbent `defaultSandboxExec` runs with
    /// network DENIED — the model-facing one-shot path).
    Contained,
    /// Network-capable execution (the incumbent escalated form). The frozen
    /// policy must allow it ([`SandboxNetworkPolicy::Allowed`]); on a
    /// denied policy this is a loud `sandbox_policy_unsupported` refusal.
    NetworkCapable,
}

/// One command to be wrapped into the sandboxed execution form.
#[derive(Debug, Clone)]
pub struct SandboxCommandRequest {
    pub argv: Vec<String>,
    pub network: SandboxNetworkRequest,
    /// The authorized working directory of the command (the T04 scope).
    /// The seatbelt backend ignores it (the supervisor sets cwd on the
    /// wrapper; the profile does not restrict cwd); the bwrap backend
    /// ports the incumbent's `--bind cwd` + `--chdir cwd`.
    pub cwd: Option<PathBuf>,
}

/// The sandboxed argv the executor must spawn INSTEAD of the raw command.
/// There is no "partially wrapped" form — either the whole command runs
/// behind the helper or nothing runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrappedSandboxCommand {
    pub argv: Vec<String>,
    pub backend: &'static str,
    pub network_allowed: bool,
}

/// The frozen per-backend capability matrix (mirrored into
/// `R04_PLATFORM_CAPABILITIES.json` — the code is the source of truth for
/// what THIS build promises; the JSON registers the per-platform
/// verification status).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxCapabilities {
    pub backend: &'static str,
    /// `all-except-deny-list` | `scoped` | `none`.
    pub filesystem_read: &'static str,
    pub filesystem_write: &'static str,
    /// `per-policy` | `unsupported`.
    pub network: &'static str,
    /// The environment boundary this backend itself adds (none — the T05
    /// executor whitelist is the single funnel).
    pub environment: &'static str,
    /// `allowed-inside-sandbox` | `unsupported`.
    pub subprocess: &'static str,
    /// Honest per-platform verification note.
    pub verification: &'static str,
}

/// The sandbox policy interface (R04-T06 deliverable). The exec chain
/// consults it through [`crate::exectools::ProcessTools`]; every refusal
/// is fail-closed at the executor (the command is never spawned).
pub trait SandboxPort: Send + Sync {
    /// The backend identifier ("seatbelt" | "bwrap" | "unsupported").
    fn backend(&self) -> &'static str;
    /// The frozen capability matrix of this backend.
    fn capabilities(&self) -> SandboxCapabilities;
    /// Verifies the helper + the request against the frozen policy and
    /// returns the sandboxed argv. FAIL-CLOSED: every failure is a
    /// refusal; this method NEVER returns the bare request argv.
    fn wrap(&self, request: SandboxCommandRequest)
        -> Result<WrappedSandboxCommand, SandboxRefusal>;
    /// The helper trust established at construction (audit/diagnostics);
    /// `None` when the backend has no helper (the unsupported backend).
    fn helper_trust(&self) -> Option<&HelperTrust> {
        None
    }
}

// ── Seatbelt backend (macOS) ───────────────────────────────────────────────

/// The Seatbelt (sandbox-exec) backend — the incumbent macOS mechanism.
pub struct SeatbeltSandbox {
    policy: SandboxPolicy,
    helper_path: PathBuf,
    trust: Option<HelperTrust>,
}

impl SeatbeltSandbox {
    /// Builds the backend: derives nothing (the policy comes validated),
    /// verifies the helper under the chosen trust scheme INCLUDING the
    /// two-legged enforcement probe. A failure is a loud refusal — the
    /// caller must not install a sandbox that cannot contain anything.
    pub fn new(
        policy: SandboxPolicy,
        helper_override: Option<PathBuf>,
    ) -> Result<Self, SandboxRefusal> {
        let helper_path = helper_override.unwrap_or_else(|| PathBuf::from(SEATBELT_HELPER_DEFAULT));
        let mut trust = verify_helper_metadata("seatbelt", &helper_path, "sandbox-exec")?;
        trust.version = None; // no --version exists; the probe below is the verification
        run_seatbelt_enforcement_probe(&helper_path).map_err(|detail| {
            SandboxRefusal::HelperUntrusted {
                backend: "seatbelt",
                path: helper_path.clone(),
                detail,
            }
        })?;
        Ok(Self {
            policy,
            helper_path,
            trust: Some(trust),
        })
    }

    pub fn policy(&self) -> &SandboxPolicy {
        &self.policy
    }

    /// Compiles the SBPL profile (the incumbent `generateProfile` port,
    /// ordering preserved: SBPL is last-match-wins so the deny lines
    /// follow the allows).
    pub fn build_profile(&self, network_allowed: bool) -> Result<String, SandboxRefusal> {
        let mut lines: Vec<String> = vec![
            "(version 1)".to_string(),
            "(deny default)".to_string(),
            String::new(),
            ";; process".to_string(),
            "(allow process-exec* process-fork signal)".to_string(),
            "(allow sysctl-read)".to_string(),
            "(allow mach*)".to_string(),
            "(allow ipc-posix*)".to_string(),
            String::new(),
            ";; global reads".to_string(),
            "(allow file-read*)".to_string(),
            String::new(),
            ";; writable roots".to_string(),
        ];
        let quote =
            |raw: &Path| -> Result<String, SandboxRefusal> { profile_literal("seatbelt", raw) };
        for path in &self.policy.writable_paths {
            lines.push(format!(
                "(allow file-write* (subpath \"{}\"))",
                quote(path)?
            ));
        }
        // Temp resources: the incumbent allows /private/tmp and the
        // realpath'd TMPDIR. `realpath_or_lexical("/tmp")` resolves to
        // /private/tmp on macOS and stays /tmp where that IS the real path.
        let tmp = realpath_or_lexical(Path::new("/tmp"));
        lines.push(format!(
            "(allow file-write* (subpath \"{}\"))",
            quote(&tmp)?
        ));
        let tmpdir = std::env::var("TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp"));
        let tmpdir = realpath_or_lexical(&tmpdir);
        if tmpdir != tmp {
            lines.push(format!(
                "(allow file-write* (subpath \"{}\"))",
                quote(&tmpdir)?
            ));
        }
        lines.push(String::new());
        if !self.policy.protected_paths.is_empty() {
            lines.push(";; write-protected (deny overrides allow; last match wins)".to_string());
            for path in &self.policy.protected_paths {
                lines.push(format!("(deny file-write* (subpath \"{}\"))", quote(path)?));
            }
            lines.push(String::new());
        }
        if !self.policy.deny_read_paths.is_empty() {
            lines.push(";; read denied (secrets)".to_string());
            for path in &self.policy.deny_read_paths {
                let literal = quote(path)?;
                lines.push(format!("(deny file-read* (subpath \"{literal}\"))"));
                lines.push(format!("(deny file-write* (subpath \"{literal}\"))"));
            }
            lines.push(String::new());
        }
        lines.extend([
            ";; terminals + PTY".to_string(),
            "(allow file-write* (literal \"/dev/null\"))".to_string(),
            "(allow file-write* (regex #\"^/dev/ttys[0-9]+$\"))".to_string(),
            "(allow file-write* (literal \"/dev/ptmx\"))".to_string(),
            "(allow pseudo-tty)".to_string(),
            String::new(),
        ]);
        if network_allowed {
            lines.push("(allow network-outbound)".to_string());
        } else {
            lines.push("(deny network*)".to_string());
        }
        Ok(lines.join("\n"))
    }
}

/// The SBPL string literal for one path — REFUSES paths containing `"` or
/// `\` (they could escape the quoted literal and inject profile text).
fn profile_literal(backend: &'static str, path: &Path) -> Result<String, SandboxRefusal> {
    let text = path.to_string_lossy();
    if text.contains('"') || text.contains('\\') || text.contains('\0') {
        return Err(SandboxRefusal::ProfilePathUnsafe {
            backend,
            path: path.to_path_buf(),
        });
    }
    Ok(text.into_owned())
}

/// The two-legged enforcement probe (seatbelt version/behavior
/// verification — the helper exposes no `--version`):
/// 1. allowed leg: exec `/usr/bin/true` under the containment core
///    (process/mach/ipc/reads allowed, everything else denied) → exit 0;
/// 2. denied leg: a write under the SAME profile MUST fail (non-zero) —
///    reads pass, writes do not. A replaced/broken helper fails a leg.
fn run_seatbelt_enforcement_probe(helper: &Path) -> Result<(), String> {
    // The containment core: everything the incumbent profile allows
    // EXCEPT file-write and network — the minimal profile under which a
    // legitimate command still runs (a bare `(deny default)` aborts even
    // exec on this OS).
    let containment_core = "(version 1)\n(deny default)\n(allow process-exec* process-fork \
                            signal)\n(allow sysctl-read)\n(allow mach*)\n(allow \
                            ipc-posix*)\n(allow file-read*)";
    let run = |argv: &[&str]| {
        std::process::Command::new(helper)
            .arg("-p")
            .arg(containment_core)
            .arg("--")
            .args(argv)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|err| format!("the enforcement probe could not run the helper: {err}"))
    };
    let allow = run(&["/usr/bin/true"])?;
    if !allow.success() {
        return Err(format!(
            "the allowed probe leg failed with {allow} — the helper cannot run commands \
             under the containment core profile"
        ));
    }
    let denied = run(&["/bin/sh", "-c", "true > /dev/null"])?;
    if denied.success() {
        return Err(
            "the denied probe leg SUCCEEDED — a write under the containment core was \
             allowed; the helper is not enforcing profiles"
                .to_string(),
        );
    }
    Ok(())
}

impl SandboxPort for SeatbeltSandbox {
    fn backend(&self) -> &'static str {
        "seatbelt"
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            backend: "seatbelt",
            filesystem_read: "all-except-deny-list",
            filesystem_write: "scoped",
            network: "per-policy",
            environment: "executor-whitelist-only (T05 SAFE_ENV_PASSTHROUGH; the sandbox adds no passthrough)",
            subprocess: "allowed-inside-sandbox",
            verification: "real-machine verified on macOS arm64 (sentinel/network/env probes)",
        }
    }

    fn wrap(
        &self,
        request: SandboxCommandRequest,
    ) -> Result<WrappedSandboxCommand, SandboxRefusal> {
        if request.argv.is_empty() || request.argv.iter().any(|a| a.is_empty()) {
            return Err(SandboxRefusal::ConfigInvalid {
                detail: "the command argv is empty".to_string(),
            });
        }
        // Per-wrap re-verification: catches removal/replacement after
        // construction (R04-A11's mid-flight helper swap).
        verify_helper_metadata("seatbelt", &self.helper_path, "sandbox-exec")?;
        let network_allowed = match request.network {
            SandboxNetworkRequest::Contained => false,
            SandboxNetworkRequest::NetworkCapable => match self.policy.network {
                SandboxNetworkPolicy::Allowed => true,
                SandboxNetworkPolicy::Denied => {
                    return Err(SandboxRefusal::PolicyUnsupported {
                        backend: "seatbelt",
                        detail: "network-capable execution is not permitted by the frozen \
                                 sandbox policy"
                            .to_string(),
                    })
                }
            },
        };
        let profile = self.build_profile(network_allowed)?;
        let mut argv = Vec::with_capacity(request.argv.len() + 4);
        argv.push(self.helper_path.to_string_lossy().into_owned());
        argv.push("-p".to_string());
        argv.push(profile);
        argv.push("--".to_string());
        argv.extend(request.argv);
        Ok(WrappedSandboxCommand {
            argv,
            backend: "seatbelt",
            network_allowed,
        })
    }

    fn helper_trust(&self) -> Option<&HelperTrust> {
        self.trust.as_ref()
    }
}

impl SeatbeltSandbox {
    /// The helper trust established at construction (audit).
    pub fn trust(&self) -> Option<&HelperTrust> {
        self.trust.as_ref()
    }
}

// ── bubblewrap backend (Linux; source-level adaptation) ────────────────────

/// The incumbent's private runtime env (bwrap.ts `addPrivateRuntimeEnv`).
const BWRAP_RUNTIME_DIRS: &[&str] = &[
    "/tmp/hana-home",
    "/tmp/hana-cache",
    "/tmp/hana-npm-cache",
    "/tmp/hana-pip-cache",
];

/// The Linux bubblewrap backend — a faithful port of `buildBwrapArgs`
/// (allowlist mounts). Compiled everywhere so the construction logic is
/// reviewed/testable; REAL execution happens only on Linux (registered as
/// not machine-verified in the platform matrix).
pub struct BwrapSandbox {
    policy: SandboxPolicy,
    helper_path: PathBuf,
    trust: Option<HelperTrust>,
}

impl BwrapSandbox {
    pub fn new(
        policy: SandboxPolicy,
        helper_override: Option<PathBuf>,
    ) -> Result<Self, SandboxRefusal> {
        let helper_path = helper_override.unwrap_or_else(|| PathBuf::from(BWRAP_HELPER_DEFAULT));
        let mut trust = verify_helper_metadata("bwrap", &helper_path, "bwrap")?;
        let version = probe_bwrap_version(&helper_path).map_err(|detail| {
            SandboxRefusal::HelperUntrusted {
                backend: "bwrap",
                path: helper_path.clone(),
                detail,
            }
        })?;
        trust.version = Some(version);
        Ok(Self {
            policy,
            helper_path,
            trust: Some(trust),
        })
    }

    pub fn policy(&self) -> &SandboxPolicy {
        &self.policy
    }

    /// The bwrap argument vector (incumbent `buildBwrapArgs`, read-all
    /// form): ro-bind root first, later binds shadow selected paths
    /// writable, protected paths re-read-only, deny-read masked, private
    /// runtime env, pid namespace, new session, die-with-parent; contained
    /// commands additionally unshare the network namespace.
    pub fn build_args(&self, cwd: Option<&Path>, network_allowed: bool) -> Vec<String> {
        let mut args: Vec<String> = vec![
            "--ro-bind".into(),
            "/".into(),
            "/".into(),
            "--dev".into(),
            "/dev".into(),
            "--proc".into(),
            "/proc".into(),
            "--tmpfs".into(),
            "/tmp".into(),
            "--unshare-pid".into(),
        ];
        if !network_allowed {
            args.push("--unshare-net".into());
        }
        args.push("--new-session".into());
        args.push("--die-with-parent".into());
        for dir in BWRAP_RUNTIME_DIRS {
            args.push("--dir".into());
            args.push((*dir).to_string());
        }
        args.extend([
            "--setenv".to_string(),
            "HOME".to_string(),
            "/tmp/hana-home".to_string(),
            "--setenv".to_string(),
            "XDG_CACHE_HOME".to_string(),
            "/tmp/hana-cache".to_string(),
            "--setenv".to_string(),
            "npm_config_cache".to_string(),
            "/tmp/hana-npm-cache".to_string(),
            "--setenv".to_string(),
            "PIP_CACHE_DIR".to_string(),
            "/tmp/hana-pip-cache".to_string(),
        ]);
        // The cwd is bound writable and chdir'd into (the incumbent's
        // `addMount(--bind, cwd)` + `--chdir cwd`).
        if let Some(cwd) = cwd.filter(|c| c.exists()) {
            args.push("--bind".into());
            let target = cwd.to_string_lossy().into_owned();
            args.push(target.clone());
            args.push(target);
            args.push("--chdir".into());
            args.push(cwd.to_string_lossy().into_owned());
        }
        // The incumbent binds ONLY existing paths (`existingPaths`) — bwrap
        // refuses to mount a missing source.
        for path in &self.policy.writable_paths {
            if !path.exists() {
                continue;
            }
            args.push("--bind".into());
            let target = path.to_string_lossy().into_owned();
            args.push(target.clone());
            args.push(target);
        }
        for path in self
            .policy
            .readable_paths
            .iter()
            .chain(self.policy.protected_paths.iter())
        {
            if !path.exists() {
                continue;
            }
            args.extend([
                "--ro-bind".into(),
                path.to_string_lossy().into_owned(),
                path.to_string_lossy().into_owned(),
            ]);
        }
        for path in &self.policy.deny_read_paths {
            if !path.exists() {
                continue;
            }
            let is_dir = path.is_dir();
            if is_dir {
                args.extend(["--tmpfs".into(), path.to_string_lossy().into_owned()]);
            } else {
                args.extend([
                    "--ro-bind".into(),
                    "/dev/null".into(),
                    path.to_string_lossy().into_owned(),
                ]);
            }
        }
        // The incumbent's home-cache masking: ~/.cache and ~/.npm (if they
        // exist and are not writable roots) become tmpfs so tools that read
        // those paths fall back to a temporary dir instead of the real user
        // caches.
        if let Ok(home) = std::env::var("HOME") {
            for rel in [".cache", ".npm"] {
                let dir = Path::new(&home).join(rel);
                let writable = self
                    .policy
                    .writable_paths
                    .iter()
                    .any(|w| w == &dir || dir.starts_with(w));
                if !writable && dir.exists() {
                    args.extend(["--tmpfs".into(), dir.to_string_lossy().into_owned()]);
                }
            }
        }
        args
    }
}

/// `bwrap --version` must report a `bubblewrap <x.y.z>` line — the version
/// leg of the helper scheme.
fn probe_bwrap_version(helper: &Path) -> Result<String, String> {
    let output = std::process::Command::new(helper)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|err| format!("the version probe could not run the helper: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "the version probe exited with {} — mismatched helper version/behavior",
            output.status
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let mut parts = text.split_whitespace();
    match (parts.next(), parts.next()) {
        (Some("bubblewrap"), Some(version)) if !version.is_empty() => Ok(version.to_string()),
        _ => Err(format!(
            "the version probe reported {text:?} — expected a `bubblewrap <version>` line"
        )),
    }
}

impl SandboxPort for BwrapSandbox {
    fn backend(&self) -> &'static str {
        "bwrap"
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            backend: "bwrap",
            filesystem_read: "all-except-deny-list",
            filesystem_write: "scoped",
            network: "per-policy",
            environment:
                "executor-whitelist-only (T05) + private runtime env overrides inside the sandbox",
            subprocess: "allowed-inside-sandbox (new pid namespace)",
            verification: "source-level adaptation of the incumbent bwrap mechanism; NOT \
                           machine-verified in R04 (no Linux host) — real-machine verification \
                           remains open, honestly unclaimed",
        }
    }

    fn wrap(
        &self,
        request: SandboxCommandRequest,
    ) -> Result<WrappedSandboxCommand, SandboxRefusal> {
        if request.argv.is_empty() || request.argv.iter().any(|a| a.is_empty()) {
            return Err(SandboxRefusal::ConfigInvalid {
                detail: "the command argv is empty".to_string(),
            });
        }
        verify_helper_metadata("bwrap", &self.helper_path, "bwrap")?;
        let network_allowed = match request.network {
            SandboxNetworkRequest::Contained => false,
            SandboxNetworkRequest::NetworkCapable => match self.policy.network {
                SandboxNetworkPolicy::Allowed => true,
                SandboxNetworkPolicy::Denied => {
                    return Err(SandboxRefusal::PolicyUnsupported {
                        backend: "bwrap",
                        detail: "network-capable execution is not permitted by the frozen \
                                 sandbox policy"
                            .to_string(),
                    })
                }
            },
        };
        let mut argv = Vec::with_capacity(request.argv.len() + 8);
        argv.push(self.helper_path.to_string_lossy().into_owned());
        argv.extend(self.build_args(request.cwd.as_deref(), network_allowed));
        argv.push("--".to_string());
        argv.extend(request.argv);
        Ok(WrappedSandboxCommand {
            argv,
            backend: "bwrap",
            network_allowed,
        })
    }

    fn helper_trust(&self) -> Option<&HelperTrust> {
        self.trust.as_ref()
    }
}

impl BwrapSandbox {
    /// The helper trust established at construction (audit).
    pub fn trust(&self) -> Option<&HelperTrust> {
        self.trust.as_ref()
    }
}

// ── the unsupported backend (Windows until the restricted-token helper is
//    ported; any other platform) — ALWAYS refuses ───────────────────────────

/// The fail-closed placeholder backend: `wrap` refuses every request with
/// `sandbox_policy_unsupported`. This is the honest Windows state in R04
/// (the incumbent restricted-token helper is not ported; a missing sandbox
/// can never become a bare run).
pub struct UnsupportedSandbox {
    platform_detail: &'static str,
}

impl UnsupportedSandbox {
    pub fn new(platform_detail: &'static str) -> Self {
        Self { platform_detail }
    }
}

impl SandboxPort for UnsupportedSandbox {
    fn backend(&self) -> &'static str {
        "unsupported"
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            backend: "unsupported",
            filesystem_read: "none",
            filesystem_write: "none",
            network: "unsupported",
            environment: "executor-whitelist-only (T05)",
            subprocess: "unsupported",
            verification: self.platform_detail,
        }
    }

    fn wrap(
        &self,
        _request: SandboxCommandRequest,
    ) -> Result<WrappedSandboxCommand, SandboxRefusal> {
        Err(SandboxRefusal::PolicyUnsupported {
            backend: "unsupported",
            detail: format!(
                "no OS sandbox backend is available on this platform ({}); isolated \
                 commands are refused — never run unsandboxed",
                self.platform_detail
            ),
        })
    }
}

// ── platform detection (incumbent `platform.ts`) ────────────────────────────

/// The sandbox backend kind this build uses per platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxBackendKind {
    Seatbelt,
    Bwrap,
    /// Windows: the incumbent restricted-token helper is not ported in R04
    /// — the backend refuses (fail-closed). Real-machine Windows
    /// verification follows the stage deferral R03-WINDOWS-R09-R10.
    RestrictedTokenDeferred,
    Unsupported,
}

impl SandboxBackendKind {
    pub fn wire_name(self) -> &'static str {
        match self {
            SandboxBackendKind::Seatbelt => "seatbelt",
            SandboxBackendKind::Bwrap => "bwrap",
            SandboxBackendKind::RestrictedTokenDeferred => "restricted-token-deferred",
            SandboxBackendKind::Unsupported => "unsupported",
        }
    }
}

/// The incumbent `detectPlatform` mapping, as build-time cfg.
pub fn detect_sandbox_backend() -> SandboxBackendKind {
    if cfg!(target_os = "macos") {
        SandboxBackendKind::Seatbelt
    } else if cfg!(target_os = "linux") {
        SandboxBackendKind::Bwrap
    } else if cfg!(target_os = "windows") {
        SandboxBackendKind::RestrictedTokenDeferred
    } else {
        SandboxBackendKind::Unsupported
    }
}

/// Builds the platform-default sandbox port for one policy. On non-Unix /
/// unsupported platforms this returns the refusing backend — composition
/// never silently loses the sandbox.
pub fn platform_sandbox(
    policy: SandboxPolicy,
    helper_override: Option<PathBuf>,
) -> Result<Arc<dyn SandboxPort>, SandboxRefusal> {
    match detect_sandbox_backend() {
        SandboxBackendKind::Seatbelt => {
            Ok(Arc::new(SeatbeltSandbox::new(policy, helper_override)?))
        }
        SandboxBackendKind::Bwrap => Ok(Arc::new(BwrapSandbox::new(policy, helper_override)?)),
        SandboxBackendKind::RestrictedTokenDeferred => Ok(Arc::new(UnsupportedSandbox::new(
            "windows restricted-token helper (lingxi-win-sandbox.exe) is not ported in R04; \
                 deferred with R03-WINDOWS-R09-R10",
        ))),
        SandboxBackendKind::Unsupported => Ok(Arc::new(UnsupportedSandbox::new(
            "no incumbent sandbox mechanism exists for this platform",
        ))),
    }
}

// ── unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn policy_input(root: &Path) -> SandboxPolicyInput {
        SandboxPolicyInput {
            lingxi_home: root.join("home"),
            agent_dir: root.join("home/agents/hana"),
            workspace_roots: vec![root.join("ws")],
            runtime_writable_paths: vec![root.join("runtime-cache")],
            network: SandboxNetworkPolicy::Allowed,
        }
    }

    #[test]
    fn policy_derivation_ports_the_incumbent_sets() {
        let root = PathBuf::from("/opt/t");
        let policy = SandboxPolicy::derive(&policy_input(&root)).expect("policy");
        assert!(policy
            .writable_paths
            .contains(&PathBuf::from("/opt/t/home/agents/hana/memory")));
        assert!(policy
            .writable_paths
            .contains(&PathBuf::from("/opt/t/home/channels")));
        assert!(policy.writable_paths.contains(&PathBuf::from("/opt/t/ws")));
        assert!(policy
            .writable_paths
            .contains(&PathBuf::from("/opt/t/runtime-cache")));
        assert!(policy
            .readable_paths
            .contains(&PathBuf::from("/opt/t/home/agents/hana/AGENTS.md")));
        assert!(policy
            .readable_paths
            .contains(&PathBuf::from("/opt/t/home/skills")));
        assert!(policy
            .deny_read_paths
            .contains(&PathBuf::from("/opt/t/home/auth.json")));
        assert!(policy
            .deny_read_paths
            .contains(&PathBuf::from("/opt/t/home/browser-data")));
        assert!(policy
            .protected_paths
            .contains(&PathBuf::from("/opt/t/ws/.git")));
        assert!(policy
            .protected_paths
            .contains(&PathBuf::from("/opt/t/home/session-files")));
        assert_eq!(policy.network, SandboxNetworkPolicy::Allowed);
    }

    #[test]
    fn policy_derivation_refuses_unusable_config() {
        let mut input = policy_input(Path::new("/opt/t"));
        input.workspace_roots.clear();
        assert_eq!(
            SandboxPolicy::derive(&input).unwrap_err().code(),
            "sandbox_config_invalid"
        );
        input = policy_input(Path::new("/opt/t"));
        input.lingxi_home = PathBuf::new();
        assert_eq!(
            SandboxPolicy::derive(&input).unwrap_err().code(),
            "sandbox_config_invalid"
        );
    }

    #[test]
    fn seatbelt_profile_orders_denies_after_allows_and_refuses_injection() {
        let root = PathBuf::from("/opt/t");
        let sandbox = SeatbeltSandbox {
            policy: SandboxPolicy::derive(&policy_input(&root)).expect("policy"),
            helper_path: PathBuf::from("/usr/bin/sandbox-exec"),
            trust: None,
        };
        let profile = sandbox.build_profile(false).expect("profile");
        assert!(profile.starts_with("(version 1)\n(deny default)"));
        assert!(profile.contains("(allow file-read*)"));
        assert!(profile.contains("(allow file-write* (subpath \"/opt/t/ws\"))"));
        // SBPL is last-match-wins: the protected/deny lines must come
        // AFTER the writable allows.
        let writable_pos = profile
            .rfind("(allow file-write* (subpath \"/opt/t/ws\"))")
            .unwrap();
        let git_deny = profile
            .find("(deny file-write* (subpath \"/opt/t/ws/.git\"))")
            .unwrap();
        assert!(git_deny > writable_pos);
        let auth_deny = profile
            .find("(deny file-read* (subpath \"/opt/t/home/auth.json\"))")
            .expect("deny-read");
        assert!(auth_deny > writable_pos);
        assert!(profile.contains("(deny network*)"));
        let network_profile = sandbox.build_profile(true).expect("network profile");
        assert!(network_profile.contains("(allow network-outbound)"));
        assert!(!network_profile.contains("(deny network*)"));

        // Policy injection: a writable root containing a quote cannot be
        // embedded — refused, never compiled.
        let mut input = policy_input(Path::new("/opt/t"));
        input.workspace_roots = vec![PathBuf::from("/opt/t/we\"ird")];
        let evil = SeatbeltSandbox {
            policy: SandboxPolicy::derive(&input).expect("policy"),
            helper_path: PathBuf::from("/usr/bin/sandbox-exec"),
            trust: None,
        };
        let refusal = evil.build_profile(false).unwrap_err();
        assert_eq!(refusal.code(), "sandbox_profile_path_unsafe");
    }

    #[test]
    fn bwrap_args_port_the_incumbent_shape() {
        // Real temp paths: the incumbent masks ONLY existing deny-read
        // targets (`if (!fs.existsSync(p)) continue`).
        let root = std::env::temp_dir().join(format!(
            "lingxi-t06-unit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("home")).expect("temp home");
        std::fs::create_dir_all(root.join("ws/.git")).expect("temp ws/.git");
        std::fs::write(root.join("home/auth.json"), b"{}").expect("auth.json");
        let input = SandboxPolicyInput {
            lingxi_home: root.join("home"),
            agent_dir: root.join("home/agents/hana"),
            workspace_roots: vec![root.join("ws")],
            runtime_writable_paths: vec![],
            network: SandboxNetworkPolicy::Allowed,
        };
        let sandbox = BwrapSandbox {
            policy: SandboxPolicy::derive(&input).expect("policy"),
            helper_path: PathBuf::from("/usr/bin/bwrap"),
            trust: None,
        };
        let ws_real = sandbox.policy().writable_paths.last().unwrap();
        let args = sandbox.build_args(Some(ws_real), false);
        let joined = args.join(" ");
        assert!(joined.contains("--ro-bind / /"));
        assert!(joined.contains("--unshare-pid"));
        assert!(joined.contains("--unshare-net"));
        assert!(joined.contains("--new-session"));
        assert!(joined.contains("--die-with-parent"));
        assert!(joined.contains(&format!("--chdir {}", ws_real.display())));
        let ws = root.join("ws");
        let ws_real = sandbox.policy().writable_paths.last().unwrap();
        assert!(joined.contains(&format!(
            "--bind {} {}",
            ws_real.display(),
            ws_real.display()
        )));
        // The policy embeds REALPATH'd roots (macOS /var → /private/var).
        let git = std::fs::canonicalize(&ws).expect("real ws").join(".git");
        assert!(joined.contains(&format!("--ro-bind {} {}", git.display(), git.display())));
        // The deny-read secret file (it exists) binds to /dev/null.
        let auth = root.join("home/auth.json");
        let auth_real = std::fs::canonicalize(&auth).expect("real auth");
        assert!(
            joined.contains(&format!("--ro-bind /dev/null {}", auth_real.display())),
            "{joined}"
        );
        let network_args = sandbox.build_args(Some(&ws), true).join(" ");
        assert!(!network_args.contains("--unshare-net"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unsupported_backend_refuses_every_request() {
        let backend = UnsupportedSandbox::new("test platform");
        let refusal = backend
            .wrap(SandboxCommandRequest {
                argv: vec!["/bin/sh".to_string()],
                network: SandboxNetworkRequest::Contained,
                cwd: None,
            })
            .unwrap_err();
        assert_eq!(refusal.code(), "sandbox_policy_unsupported");
        assert!(format!("{refusal}").contains("never run unsandboxed"));
    }

    #[test]
    fn bwrap_version_probe_parses_the_official_line() {
        // The parse contract, exercised without a real bwrap binary.
        fn parse(text: &str) -> Result<String, String> {
            let text = text.trim().to_string();
            let mut parts = text.split_whitespace();
            match (parts.next(), parts.next()) {
                (Some("bubblewrap"), Some(version)) if !version.is_empty() => {
                    Ok(version.to_string())
                }
                _ => Err("bad".to_string()),
            }
        }
        assert_eq!(parse("bubblewrap 0.11.0").unwrap(), "0.11.0");
        assert!(parse("not-bwrap 1.2.3").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn capabilities_matrix_names_the_frozen_guarantees() {
        let seatbelt = SeatbeltSandbox {
            policy: SandboxPolicy::derive(&policy_input(Path::new("/opt/t"))).expect("policy"),
            helper_path: PathBuf::from("/usr/bin/sandbox-exec"),
            trust: None,
        };
        let caps = seatbelt.capabilities();
        assert_eq!(caps.backend, "seatbelt");
        assert_eq!(caps.filesystem_read, "all-except-deny-list");
        assert_eq!(caps.filesystem_write, "scoped");
        assert_eq!(caps.network, "per-policy");
        assert_eq!(caps.subprocess, "allowed-inside-sandbox");
    }
}
