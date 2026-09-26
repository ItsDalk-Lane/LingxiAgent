//! Normalized data-root layout, permission checks and atomic file writes
//! (R02-T02 step 2).
//!
//! Every sensitive path is derived from a CANONICALIZED data root plus a
//! fixed, non-user-controlled name — user-supplied strings are never
//! concatenated into sensitive paths, and path aliases (macOS `/tmp` vs
//! `/private/tmp` symlinks) collapse to one lock identity before the
//! single-writer lock is taken.
//!
//! Layout under the canonical home:
//!
//! ```text
//!   {home}/lingxi-service/          runtime dir (0700 on unix)
//!   {home}/lingxi-service/instance.lock    single-writer lock file
//!   {home}/lingxi-service/instance.json    instance record (atomic writes)
//!   {home}/lingxi-service/instance.stale.json  archive of the last stale
//!                                              record taken over
//!   {home}/lingxi-service/tmp/      staging dir for atomic renames
//! ```
//!
//! Permission policy (unix): the runtime dir is created with mode 0700 and
//! tightened back to 0700 if it exists with wider bits (logged by the
//! caller). Windows: directory existence/type checks only — mode ops are a
//! unix concept; the Windows branch is documented but not verified on this
//! machine (same platform boundary as R02-T01).

use std::io;
use std::path::{Path, PathBuf};

use crate::config::ConfigError;

/// Fixed runtime dir name under the data root (never user-derived).
pub const RUNTIME_DIR_NAME: &str = "lingxi-service";
/// Fixed names inside the runtime dir (never user-derived).
pub const LOCK_FILE_NAME: &str = "instance.lock";
pub const RECORD_FILE_NAME: &str = "instance.json";
pub const STALE_ARCHIVE_NAME: &str = "instance.stale.json";
pub const TMP_DIR_NAME: &str = "tmp";

/// Fully resolved, canonicalized data-root layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRootLayout {
    /// Canonical absolute home (symlinks resolved).
    pub home: PathBuf,
    /// `{home}/lingxi-service` (0700 on unix).
    pub runtime_dir: PathBuf,
    pub lock_path: PathBuf,
    pub record_path: PathBuf,
    pub stale_archive_path: PathBuf,
    pub tmp_dir: PathBuf,
}

/// Prepares the data root and the normalized runtime layout.
///
/// Rejections (all loud, none silent):
/// - relative path (re-checked here so every caller path shares one gate);
/// - the filesystem root itself as home;
/// - home exists but is not a directory;
/// - creation failure.
///
/// On success the home is canonicalized (created first if missing) and the
/// runtime + tmp dirs exist with private permissions (unix).
pub fn prepare_layout(home: &Path) -> Result<DataRootLayout, ConfigError> {
    if !home.is_absolute() {
        return Err(ConfigError::RelativeHome {
            value: home.to_path_buf(),
        });
    }
    if home == Path::new("/") {
        return Err(ConfigError::HomeIsFilesystemRoot {
            value: home.to_path_buf(),
        });
    }
    if home.exists() && !home.is_dir() {
        return Err(ConfigError::HomeIsNotADirectory {
            value: home.to_path_buf(),
        });
    }
    if !home.exists() {
        std::fs::create_dir_all(home).map_err(|source| ConfigError::HomeCreateFailed {
            value: home.to_path_buf(),
            source: source.to_string(),
        })?;
    }
    let canonical =
        std::fs::canonicalize(home).map_err(|source| ConfigError::HomeCreateFailed {
            value: home.to_path_buf(),
            source: format!("cannot canonicalize data root: {source}"),
        })?;

    let runtime_dir = canonical.join(RUNTIME_DIR_NAME);
    ensure_private_dir(&runtime_dir)?;
    let tmp_dir = runtime_dir.join(TMP_DIR_NAME);
    ensure_private_dir(&tmp_dir)?;

    Ok(DataRootLayout {
        home: canonical,
        lock_path: runtime_dir.join(LOCK_FILE_NAME),
        record_path: runtime_dir.join(RECORD_FILE_NAME),
        stale_archive_path: runtime_dir.join(STALE_ARCHIVE_NAME),
        runtime_dir,
        tmp_dir,
    })
}

/// Creates `path` (if missing) with private permissions and tightens an
/// existing dir back to 0700 on unix. Returns whether the mode was
/// tightened (for the caller's safe log).
pub fn ensure_private_dir(path: &Path) -> Result<bool, ConfigError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        if path.exists() {
            if !path.is_dir() {
                return Err(ConfigError::HomeIsNotADirectory {
                    value: path.to_path_buf(),
                });
            }
            let meta = std::fs::metadata(path).map_err(|source| dir_io_error(path, source))?;
            let mode = meta.permissions().mode();
            if mode & 0o777 != 0o700 {
                let mut perms = meta.permissions();
                perms.set_mode(0o700);
                std::fs::set_permissions(path, perms)
                    .map_err(|source| dir_io_error(path, source))?;
                return Ok(true);
            }
            return Ok(false);
        }
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .map_err(|source| dir_io_error(path, source))?;
        // The mode(2) syscall argument is masked by the process umask, so
        // verify and tighten after creation.
        let meta = std::fs::metadata(path).map_err(|source| dir_io_error(path, source))?;
        let mode = meta.permissions().mode();
        if mode & 0o777 != 0o700 {
            let mut perms = meta.permissions();
            perms.set_mode(0o700);
            std::fs::set_permissions(path, perms).map_err(|source| dir_io_error(path, source))?;
            return Ok(true);
        }
        Ok(false)
    }
    #[cfg(not(unix))]
    {
        if path.exists() {
            if !path.is_dir() {
                return Err(ConfigError::HomeIsNotADirectory {
                    value: path.to_path_buf(),
                });
            }
            return Ok(false);
        }
        std::fs::create_dir_all(path).map_err(|source| dir_io_error(path, source))?;
        Ok(false)
    }
}

fn dir_io_error(path: &Path, source: io::Error) -> ConfigError {
    ConfigError::HomeCreateFailed {
        value: path.to_path_buf(),
        source: source.to_string(),
    }
}

/// Atomically replaces `path` with `bytes`: write to a uniquely named temp
/// file in the same directory, fsync the file, rename over the target, then
/// fsync the directory so the rename itself is durable. A crash mid-write
/// leaves at most a temp file, never a torn target.
///
/// The temp name is derived from the pid, a nanosecond timestamp and a
/// process-wide counter — never from user input.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), io::Error> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = path.with_extension(format!("tmp-{seq}-{nanos:x}-{}", std::process::id()));

    // fsync the file before rename.
    {
        use std::io::Write as _;
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    // Rename is atomic on the same filesystem (same parent dir here).
    std::fs::rename(&tmp, path)?;
    // fsync the parent dir so the rename is durable (unix; Windows has no
    // directory fsync — documented platform boundary).
    #[cfg(unix)]
    {
        if let Some(parent) = path.parent() {
            if let Ok(dir) = std::fs::File::open(parent) {
                let _ = dir.sync_all();
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = seq;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("lingxi-r02t02-paths-{}-{tag}", std::process::id()))
    }

    #[test]
    fn layout_creates_normalized_dirs_with_private_mode() {
        let home = synthetic("layout-ok");
        let _ = std::fs::remove_dir_all(&home);
        let layout = prepare_layout(&home).unwrap();
        assert!(layout.runtime_dir.is_dir());
        assert!(layout.tmp_dir.is_dir());
        assert_eq!(layout.lock_path, layout.runtime_dir.join("instance.lock"));
        assert_eq!(layout.record_path, layout.runtime_dir.join("instance.json"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&layout.runtime_dir)
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o700, "runtime dir must be private");
        }
        // Canonicalization: a symlink alias of the home must collapse to
        // the same canonical root (this is what makes an aliased second
        // start hit the same single-writer lock).
        #[cfg(unix)]
        {
            let alias = home.with_extension("alias-link");
            std::os::unix::fs::symlink(&home, &alias).unwrap();
            let via_alias = prepare_layout(&alias).unwrap();
            assert_eq!(via_alias.home, layout.home, "alias must canonicalize equal");
            assert_eq!(via_alias.lock_path, layout.lock_path);
            std::fs::remove_file(&alias).ok();
        }
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn layout_rejects_relative_and_root_and_file_as_home() {
        assert!(matches!(
            prepare_layout(Path::new("relative/dir")),
            Err(ConfigError::RelativeHome { .. })
        ));
        // Filesystem root is never a valid home.
        assert!(matches!(
            prepare_layout(Path::new("/")),
            Err(ConfigError::HomeIsFilesystemRoot { .. })
        ));
        let file_home = synthetic("layout-file");
        let _ = std::fs::remove_dir_all(&file_home);
        std::fs::write(&file_home, b"blocker").unwrap();
        assert!(matches!(
            prepare_layout(&file_home),
            Err(ConfigError::HomeIsNotADirectory { .. })
        ));
        std::fs::remove_file(&file_home).ok();
    }

    #[cfg(unix)]
    #[test]
    fn layout_tightens_wide_runtime_dir() {
        use std::os::unix::fs::PermissionsExt as _;
        let home = synthetic("layout-wide");
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("lingxi-service")).unwrap();
        std::fs::set_permissions(home.join("lingxi-service"), {
            let mut p = std::fs::metadata(home.join("lingxi-service"))
                .unwrap()
                .permissions();
            p.set_mode(0o755);
            p
        })
        .unwrap();
        let tightened = prepare_layout(&home).unwrap();
        let mode = std::fs::metadata(&tightened.runtime_dir)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn atomic_write_replaces_content_and_leaves_no_tmp() {
        let home = synthetic("atomic");
        let _ = std::fs::remove_dir_all(&home);
        let layout = prepare_layout(&home).unwrap();
        let target = layout.runtime_dir.join("probe.json");
        atomic_write(&target, b"{\"v\":1}").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"{\"v\":1}");
        atomic_write(&target, b"{\"v\":2}").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"{\"v\":2}");
        let leftovers: Vec<_> = std::fs::read_dir(&layout.runtime_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp-"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "no temp residue expected, got {leftovers:?}"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn atomic_write_many_rounds_do_not_collide() {
        let home = synthetic("atomic-many");
        let _ = std::fs::remove_dir_all(&home);
        let layout = prepare_layout(&home).unwrap();
        let target = layout.runtime_dir.join("probe.json");
        for i in 0..64u32 {
            let payload = format!("{{\"round\":{i}}}");
            atomic_write(&target, payload.as_bytes()).unwrap();
            assert_eq!(std::fs::read_to_string(&target).unwrap(), payload);
        }
        let _ = std::fs::remove_dir_all(&home);
    }
}
