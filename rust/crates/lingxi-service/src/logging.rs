//! Bounded, redacted log/trace pipeline (R02-T07 step 2).
//!
//! The composition root installs ONE global `tracing` subscriber whose
//! writer is this module's [`LogRouter`]: every formatted event line passes
//! through [`crate::redaction::redact_text`] and then fans out to
//!
//! - stderr (always — the harness/marker contract surface), and
//! - a size-capped rotating file under `{home}/lingxi-service/logs/`
//!   (bounded accumulation: `--log-max-bytes` per file, `--log-max-files`
//!   files kept, oldest pruned — the bounded counterpart of the incumbent
//!   debug-log's 5MB truncate-and-stop).
//!
//! Rotation is size-based and count-pruned, so the log directory can never
//! grow without bound regardless of how long the service runs. Files are
//! created 0600 inside the private runtime dir (0700), mirroring the
//! storage conventions of R02-T04/T06. Zero new dependencies: the rotating
//! writer is plain `std::fs`.
//!
//! Body recording (taskbook step: "正文记录遵循现有设置"): the service
//! never logs request/message bodies — there is no debug flag that turns
//! body recording on, matching the incumbent default where bodies appear
//! only inside the (redacted) business event stream, never in diagnostics.
//!
//! Redaction vs. the safe-log contract (T02): the effective data-root path
//! stays VISIBLE in logs (the T02/A04 acceptance greps it), so the
//! long-random-token rule here diverges deliberately from the incumbent
//! regex — its character class excludes `/` and `.` and its boundary
//! accepts `/`, so opaque credential material (base64url/hex) is redacted
//! while structured path diagnostics survive. Every credential this stack
//! mints is caught by the prefix rules (`hana_dev_`, `hana_ws_`) or the
//! header/assignment rules.

use std::fs;
use std::io::{self, IsTerminal as _, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use tracing_subscriber::fmt::MakeWriter;

use crate::redaction::redact_text;

/// Rotation knobs. Defaults mirror the incumbent debug-log's 5 MiB
/// per-file ceiling; the file COUNT is the added bound (the incumbent
/// truncates and stops, which loses everything after 5 MiB — rotation
/// keeps the newest bounded window instead).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRotationConfig {
    /// Rotate the active file once it exceeds this many bytes.
    pub max_bytes: u64,
    /// Maximum number of rotated files kept on disk (active file included).
    pub max_files: usize,
}

pub const DEFAULT_LOG_MAX_BYTES: u64 = 5 * 1024 * 1024;
pub const DEFAULT_LOG_MAX_FILES: usize = 7;

impl Default for LogRotationConfig {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_LOG_MAX_BYTES,
            max_files: DEFAULT_LOG_MAX_FILES,
        }
    }
}

impl LogRotationConfig {
    /// Degenerate rotation settings are loud configuration errors.
    pub fn validate(&self) -> Result<(), String> {
        // One line must always fit: keep a floor well above any single
        // diagnostic line.
        if self.max_bytes < 64 {
            return Err(format!(
                "log max bytes must be >= 64, got {}",
                self.max_bytes
            ));
        }
        if self.max_files < 2 {
            return Err(format!(
                "log max files must be >= 2 (rotation needs a successor), got {}",
                self.max_files
            ));
        }
        Ok(())
    }
}

/// One rotating file sequence inside a private directory. File names are
/// `service-{seq:06}.log`; the sequence is discovered from the directory at
/// open (max existing + 1) so pruning can order files deterministically by
/// sequence instead of wall-clock mtime (no clock dependency, stable in
/// tests).
pub struct RotatingLogFile {
    dir: PathBuf,
    max_bytes: u64,
    max_files: usize,
    seq: u64,
    file: Option<fs::File>,
    written: u64,
}

impl RotatingLogFile {
    /// Opens the log directory (0700, created if missing), prunes older
    /// files beyond `max_files` and opens the next sequence.
    pub fn open(dir: PathBuf, config: &LogRotationConfig) -> io::Result<Self> {
        config
            .validate()
            .map_err(|detail| io::Error::new(io::ErrorKind::InvalidInput, detail))?;
        crate::paths::ensure_private_dir(&dir)
            .map_err(|err| io::Error::other(format!("cannot prepare log dir: {err}")))?;
        let mut log = Self {
            dir,
            max_bytes: config.max_bytes,
            max_files: config.max_files,
            seq: 0,
            file: None,
            written: 0,
        };
        log.seq = log.next_seq()?;
        log.prune()?;
        log.open_current()?;
        Ok(log)
    }

    fn next_seq(&self) -> io::Result<u64> {
        let mut max = 0u64;
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some(rest) = name.strip_prefix("service-") else {
                continue;
            };
            let Some(digits) = rest.strip_suffix(".log") else {
                continue;
            };
            if let Ok(seq) = digits.parse::<u64>() {
                max = max.max(seq);
            }
        }
        Ok(max + 1)
    }

    /// Deletes the oldest `service-*.log` files beyond `max_files` (the
    /// active file included in the count — the bound is on the whole set).
    fn prune(&self) -> io::Result<()> {
        let mut seqs: Vec<u64> = fs::read_dir(&self.dir)?
            .filter_map(|entry| {
                let name = entry.ok()?.file_name();
                let name = name.to_str()?;
                let digits = name.strip_prefix("service-")?.strip_suffix(".log")?;
                digits.parse::<u64>().ok()
            })
            .collect();
        seqs.sort_unstable();
        let keep_from = seqs.len().saturating_sub(self.max_files);
        for seq in &seqs[..keep_from] {
            let path = self.path_for(*seq);
            if let Err(err) = fs::remove_file(&path) {
                if err.kind() != io::ErrorKind::NotFound {
                    return Err(err);
                }
            }
        }
        Ok(())
    }

    fn path_for(&self, seq: u64) -> PathBuf {
        self.dir.join(format!("service-{seq:06}.log"))
    }

    fn open_current(&mut self) -> io::Result<()> {
        let path = self.path_for(self.seq);
        let file = fs::File::options().create(true).append(true).open(&path)?;
        // Owner-only, mirroring the storage conventions (explicit chmod, not
        // umask luck).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = file.metadata()?.permissions();
            perms.set_mode(0o600);
            file.set_permissions(perms)?;
        }
        self.written = file.metadata()?.len();
        self.file = Some(file);
        Ok(())
    }

    /// Appends one already-redacted line. Rotates when the byte budget is
    /// exhausted; a rotation or write failure is surfaced to the caller
    /// (the router degrades explicitly to stderr-only, never silently).
    fn write_line(&mut self, line: &str) -> io::Result<()> {
        let line_len = line.len() + 1;
        if self.file.is_none() || self.written + line_len as u64 > self.max_bytes {
            self.rotate()?;
        }
        let Some(file) = self.file.as_mut() else {
            return Err(io::Error::other("log file missing after rotate"));
        };
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        file.flush()?;
        self.written += line_len as u64;
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        if let Some(file) = self.file.take() {
            file.sync_all().ok();
        }
        self.seq += 1;
        self.open_current()?;
        self.prune()?;
        Ok(())
    }
}

/// The tracing writer: redacts every complete event line, then writes it to
/// stderr and (when attached) the rotating file. Installed once via
/// [`init_tracing`]; the file half attaches later (the data home is only
/// known after configuration/layout resolution — pre-attach runs log to
/// stderr alone).
#[derive(Clone)]
pub struct LogRouter {
    stderr: bool,
    home: std::sync::Arc<Mutex<Option<RotatingLogFile>>>,
}

impl LogRouter {
    /// Creates the router. `stderr` is true for the binary (markers and
    /// diagnostics stay on stderr); tests can build a file-only router.
    pub fn new(stderr: bool) -> Self {
        Self {
            stderr,
            home: std::sync::Arc::new(Mutex::new(None)),
        }
    }

    /// Attaches the rotating file half (explicit degradation, never a
    /// silent one: a failure is returned so the caller can log it loudly
    /// and continue stderr-only — losing the file log must not refuse
    /// service startup, matching the incumbent "log failures must not
    /// block business" rule, but it must be VISIBLE).
    pub fn attach_file(&self, dir: PathBuf, config: &LogRotationConfig) -> io::Result<()> {
        let file = RotatingLogFile::open(dir, config)?;
        *self
            .home
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(file);
        Ok(())
    }

    /// Detaches (and flushes) the file half; used by tests and by a
    /// graceful close.
    pub fn detach_file(&self) {
        *self
            .home
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }

    fn emit_line(&self, line: &str) {
        let redacted = redact_text(line, None);
        if self.stderr {
            let mut err = io::stderr().lock();
            let _ = err.write_all(redacted.as_bytes());
            let _ = err.write_all(b"\n");
            let _ = err.flush();
        }
        // File-write failure degrades EXPLICITLY to a stderr error — never
        // silently swallowed (red line: no silent downgrades).
        let write_error = {
            let mut guard = self
                .home
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match guard.as_mut() {
                Some(file) => file.write_line(&redacted).err(),
                None => None,
            }
        };
        if let Some(err) = write_error {
            eprintln!("LINGXI_SERVICE_LOG_WRITE_FAILED error={err} (continuing stderr-only)");
        }
    }
}

impl<'a> MakeWriter<'a> for LogRouter {
    type Writer = LogSink<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        LogSink {
            router: self,
            buf: Vec::new(),
        }
    }
}

/// Line-buffering sink: `tracing_subscriber::fmt` writes one event as
/// several `write` calls; the sink accumulates bytes, redacts on line
/// completion, and flushes any unterminated remainder on drop.
pub struct LogSink<'a> {
    router: &'a LogRouter,
    buf: Vec<u8>,
}

impl Write for LogSink<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(buf);
        while let Some(pos) = self.buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=pos).collect();
            let text = String::from_utf8_lossy(&line[..line.len() - 1]);
            self.router.emit_line(&text);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for LogSink<'_> {
    fn drop(&mut self) {
        if !self.buf.is_empty() {
            let text = String::from_utf8_lossy(&self.buf);
            self.router.emit_line(&text);
            self.buf.clear();
        }
    }
}

/// Installs the global default subscriber (fmt + env filter + this
/// router). The filter vocabulary is the incumbent tracing one:
/// `RUST_LOG` when set, `info` otherwise. Idempotent-safe: returns an
/// error instead of panicking if a subscriber is already installed (the
/// binary maps it to a loud startup failure).
pub fn init_tracing(router: LogRouter) -> Result<(), tracing::subscriber::SetGlobalDefaultError> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        // ANSI only on a real terminal: the safe log must stay
        // machine-greppable when redirected (evidence scripts key on it).
        .with_ansi(std::io::stderr().is_terminal())
        .with_writer(router)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lingxi-service-t07-logs-{}-{tag}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn lines_of(path: &std::path::Path) -> Vec<String> {
        fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn all_log_files(dir: &std::path::Path) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| {
                let p = e.ok()?.path();
                let n = p.file_name()?.to_str()?.to_string();
                (n.starts_with("service-") && n.ends_with(".log")).then_some(p)
            })
            .collect();
        out.sort();
        out
    }

    #[test]
    fn rotation_respects_max_bytes_and_files_are_complete_lines() {
        let dir = temp_dir("rotate");
        // max_files high enough that pruning cannot interfere: this test
        // pins the lossless-rotation property (the prune bound is pinned
        // separately below).
        let config = LogRotationConfig {
            max_bytes: 256,
            max_files: 40,
        };
        {
            let mut log = RotatingLogFile::open(dir.clone(), &config).expect("open");
            for i in 0..40 {
                log.write_line(&format!("log line {i:03} payload aaaaabbbbbbbbb"))
                    .expect("write");
            }
        }
        let files = all_log_files(&dir);
        assert!(
            files.len() >= 2,
            "writes must have rotated: {} files",
            files.len()
        );
        for path in &files {
            let meta = fs::metadata(path).unwrap();
            assert!(
                meta.len() <= 256 + 64,
                "file {} exceeds the rotation budget: {}",
                path.display(),
                meta.len()
            );
            for line in lines_of(path) {
                assert!(!line.is_empty(), "torn line in {}", path.display());
            }
        }
        // Every line survives exactly once across the rotation set.
        let total: usize = files.iter().map(|p| lines_of(p).len()).sum();
        assert_eq!(total, 40, "no line may be lost across rotation");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_keeps_only_max_files() {
        let dir = temp_dir("prune");
        let config = LogRotationConfig {
            max_bytes: 128,
            max_files: 3,
        };
        {
            let mut log = RotatingLogFile::open(dir.clone(), &config).expect("open");
            for i in 0..60 {
                log.write_line(&format!("line {i:03} aaaaaabbbbbbccccc"))
                    .expect("write");
            }
        }
        let files = all_log_files(&dir);
        assert_eq!(
            files.len(),
            3,
            "the directory must stay bounded at max_files: {:?}",
            files
        );
        // The kept files are the NEWEST sequences.
        let last_line = lines_of(&files[2]).into_iter().last().unwrap();
        assert_eq!(last_line, "line 059 aaaaaabbbbbbccccc");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reopened_sequence_continues_and_never_truncates_old_files() {
        let dir = temp_dir("reopen");
        let config = LogRotationConfig::default();
        {
            let mut log = RotatingLogFile::open(dir.clone(), &config).expect("open");
            log.write_line("first run line").expect("write");
        }
        {
            let mut log = RotatingLogFile::open(dir.clone(), &config).expect("reopen");
            log.write_line("second run line").expect("write");
        }
        let files = all_log_files(&dir);
        assert_eq!(files.len(), 2, "reopen must start a NEW file");
        assert!(lines_of(&files[0]).contains(&"first run line".to_string()));
        assert!(lines_of(&files[1]).contains(&"second run line".to_string()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn log_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("perms");
        let mut log =
            RotatingLogFile::open(dir.clone(), &LogRotationConfig::default()).expect("open");
        log.write_line("permission probe").expect("write");
        let files = all_log_files(&dir);
        let mode = fs::metadata(&files[0]).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "log files must be 0600");
        let dir_mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700, "log dir must be 0700");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn router_redacts_lines_and_fans_out_to_file() {
        let dir = temp_dir("router");
        let router = LogRouter::new(false);
        router
            .attach_file(dir.clone(), &LogRotationConfig::default())
            .expect("attach");
        {
            let mut sink = router.make_writer();
            let _ = sink.write_all(b"token=supersecret12345 value kept\nplain tail stays\n");
        }
        let files = all_log_files(&dir);
        assert_eq!(files.len(), 1);
        let lines = lines_of(&files[0]);
        assert_eq!(
            lines.len(),
            2,
            "one line per newline, unterminated tail flushed"
        );
        assert!(
            lines[0].contains("token=[redacted]") && !lines[0].contains("supersecret"),
            "{lines:?}"
        );
        assert_eq!(lines[1], "plain tail stays");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotation_config_validation_is_loud() {
        assert!(LogRotationConfig::default().validate().is_ok());
        assert!(LogRotationConfig {
            max_bytes: 1,
            max_files: 7
        }
        .validate()
        .is_err());
        assert!(LogRotationConfig {
            max_bytes: 1024 * 1024,
            max_files: 1
        }
        .validate()
        .is_err());
    }
}
