//! 活动根指针（T05_DESIGN §8；裁决点 P4）。
//!
//! - 常规 JSON 文件（symlink 一律响亮拒绝），`paths::atomic_write_private`
//!   同目录 tmp+rename = 读者永远见到完整旧或完整新，**无先删后建窗口**。
//! - 写后读回逐字节校验 + 父目录 fsync（atomic_write_private 内置）。
//! - 首次切换前指针不存在 = 合法「首迁」状态（None，不是错误）。
//! - 指针语义只声明 activeDataRoot；新栈启动链消费它见 Phase D/E。

use crate::paths;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

pub const POINTER_SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActiveRootPointer {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u64,
    #[serde(rename = "activeDataRoot")]
    pub active_data_root: String,
    #[serde(rename = "cutoverId")]
    pub cutover_id: String,
    #[serde(rename = "committedAt")]
    pub committed_at: String,
}

#[derive(Debug)]
pub enum PointerError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Corrupt {
        path: PathBuf,
        detail: String,
    },
    /// 裁决点 P4：指针路径是 symlink = 响亮拒绝（绝不跟随）。
    Symlink {
        path: PathBuf,
    },
}

impl fmt::Display for PointerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PointerError::Io { path, source } => {
                write!(f, "pointer io error at {}: {source}", path.display())
            }
            PointerError::Corrupt { path, detail } => {
                write!(f, "pointer corrupt at {}: {detail}", path.display())
            }
            PointerError::Symlink { path } => {
                write!(
                    f,
                    "pointer path is a symlink (P4 refusal): {}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for PointerError {}

fn reject_symlink(path: &Path) -> Result<(), PointerError> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(PointerError::Symlink {
            path: path.to_path_buf(),
        }),
        Ok(_) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(PointerError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn validate(pointer: &ActiveRootPointer, path: &Path) -> Result<(), PointerError> {
    let corrupt = |detail: String| PointerError::Corrupt {
        path: path.to_path_buf(),
        detail,
    };
    if pointer.schema_version != POINTER_SCHEMA_VERSION {
        return Err(corrupt(format!(
            "schemaVersion {} != {POINTER_SCHEMA_VERSION}",
            pointer.schema_version
        )));
    }
    if pointer.active_data_root.is_empty() {
        return Err(corrupt("activeDataRoot is empty".into()));
    }
    if pointer.cutover_id.is_empty() {
        return Err(corrupt("cutoverId is empty".into()));
    }
    Ok(())
}

/// 读指针：不存在 = Ok(None)（合法首迁前状态）；损坏 = 响亮 Err。
pub fn read_pointer(path: &Path) -> Result<Option<ActiveRootPointer>, PointerError> {
    reject_symlink(path)?;
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(PointerError::Io {
                path: path.to_path_buf(),
                source,
            })
        }
    };
    let pointer: ActiveRootPointer =
        serde_json::from_str(&raw).map_err(|err| PointerError::Corrupt {
            path: path.to_path_buf(),
            detail: format!("JSON parse: {err}"),
        })?;
    validate(&pointer, path)?;
    Ok(Some(pointer))
}

/// 原子切换指针：symlink 拒 → atomic_write_private（rename 原子）→
/// 读回反序列化 + 逐字节语义对拍（旧读者撞见半文件在此设计下不可能）。
pub fn commit_pointer(
    path: &Path,
    active_data_root: &Path,
    cutover_id: &str,
) -> Result<ActiveRootPointer, PointerError> {
    reject_symlink(path)?;
    let pointer = ActiveRootPointer {
        schema_version: POINTER_SCHEMA_VERSION,
        active_data_root: active_data_root.display().to_string(),
        cutover_id: cutover_id.to_string(),
        committed_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| PointerError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let mut body = serde_json::to_string_pretty(&pointer).map_err(|err| PointerError::Corrupt {
        path: path.to_path_buf(),
        detail: format!("serialize: {err}"),
    })?;
    body.push('\n');
    paths::atomic_write_private(path, body.as_bytes()).map_err(|source| PointerError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let readback = read_pointer(path)?.ok_or_else(|| PointerError::Corrupt {
        path: path.to_path_buf(),
        detail: "pointer vanished immediately after atomic write".into(),
    })?;
    if readback.active_data_root != pointer.active_data_root
        || readback.cutover_id != pointer.cutover_id
    {
        return Err(PointerError::Corrupt {
            path: path.to_path_buf(),
            detail: format!(
                "readback mismatch: wrote {:?}/{} read {:?}/{}",
                pointer.active_data_root,
                pointer.cutover_id,
                readback.active_data_root,
                readback.cutover_id
            ),
        });
    }
    Ok(readback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_dir(tag: &str) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "t05-pointer-test-{}-{}-{tag}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn absent_pointer_is_legal_first_cutover_state() {
        let dir = temp_dir("absent");
        assert!(read_pointer(&dir.join("ptr.json")).unwrap().is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn commit_and_readback_roundtrip() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("ptr.json");
        let committed = commit_pointer(&path, Path::new("/tmp/new-root"), "cut-9").unwrap();
        assert_eq!(committed.active_data_root, "/tmp/new-root");
        assert_eq!(committed.cutover_id, "cut-9");
        let again = commit_pointer(&path, Path::new("/tmp/newer-root"), "cut-10").unwrap();
        assert_eq!(again.active_data_root, "/tmp/newer-root");
        assert_eq!(read_pointer(&path).unwrap().unwrap(), again);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_pointer_is_loudly_rejected_on_read_and_write() {
        let dir = temp_dir("symlink");
        let real = dir.join("real.json");
        std::fs::write(&real, b"{}").unwrap();
        let link = dir.join("ptr.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let err = read_pointer(&link).unwrap_err();
        assert!(err.to_string().contains("symlink"), "{err}");
        let err = commit_pointer(&link, Path::new("/tmp/x"), "cut-1").unwrap_err();
        assert!(err.to_string().contains("symlink"), "{err}");
        // 拒绝后 symlink 目标内容未被触碰（零写穿透）。
        assert_eq!(std::fs::read(&real).unwrap(), b"{}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn corrupt_pointer_is_loud_error() {
        let dir = temp_dir("corrupt");
        let path = dir.join("ptr.json");
        std::fs::write(&path, b"{\"schemaVersion\": 999}").unwrap();
        let err = read_pointer(&path).unwrap_err();
        assert!(err.to_string().contains("corrupt"), "{err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn pointer_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = temp_dir("private");
        let path = dir.join("ptr.json");
        commit_pointer(&path, Path::new("/tmp/x"), "cut-1").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "mode={mode:o}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
