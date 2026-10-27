//! R08-T04: 源根清单（manifest）与 checksum 模型。
//!
//! 逐文件记录 `relpath + size + sha256`，汇总成 `source_root_sha256`
//! （规范序哈希：按 relpath 字节序排序后逐行 `"{sha}  {size}  {relpath}\n"`
//! 再整体 sha256）。导入前后各算一次，逐字节比对即 A08「源文件哈希不变」
//! 的证据原料。目录遍历确定性排序；普通文件之外（symlink/socket/fifo/…）
//! 不读取内容，进 skipped_special 清单响亮上报。

use sha2::{Digest, Sha256};
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    pub relpath: String,
    pub size: u64,
    pub sha256: String,
}

/// 非普通文件条目（symlink/特殊文件），记类型不取内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedSpecial {
    pub relpath: String,
    pub kind: &'static str,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    pub entries: Vec<ManifestEntry>,
    pub skipped_special: Vec<SkippedSpecial>,
}

#[derive(Debug)]
pub struct ManifestError {
    pub relpath: String,
    pub message: String,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.relpath, self.message)
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

fn file_sha256(path: &Path) -> std::io::Result<(u64, String)> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok((size, sha256_hex(&hasher.finalize())))
}

/// 递归扫描 root，返回确定性排序的清单。任何 IO 错误都是响亮错误
/// （不静默跳过——那是清单完整性的红线）。
pub fn build_manifest(root: &Path) -> Result<Manifest, ManifestError> {
    let mut manifest = Manifest::default();
    let mut stack: Vec<PathBuf> = vec![PathBuf::new()];
    while let Some(rel_dir) = stack.pop() {
        let abs_dir = root.join(&rel_dir);
        let mut entries: Vec<_> = std::fs::read_dir(&abs_dir)
            .map_err(|err| ManifestError {
                relpath: rel_dir.display().to_string(),
                message: format!("read_dir: {err}"),
            })?
            .collect::<Result<_, _>>()
            .map_err(|err| ManifestError {
                relpath: rel_dir.display().to_string(),
                message: format!("read_dir entry: {err}"),
            })?;
        // 确定性：按文件名字节序排序，与文件系统返回顺序无关。
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name();
            let rel = if rel_dir.as_os_str().is_empty() {
                PathBuf::from(&name)
            } else {
                rel_dir.join(&name)
            };
            let relpath = rel.to_string_lossy().replace('\\', "/");
            let meta = std::fs::symlink_metadata(entry.path()).map_err(|err| ManifestError {
                relpath: relpath.clone(),
                message: format!("symlink_metadata: {err}"),
            })?;
            if meta.is_dir() {
                stack.push(rel);
            } else if meta.is_file() {
                let (size, sha256) =
                    file_sha256(&entry.path()).map_err(|err| ManifestError {
                        relpath: relpath.clone(),
                        message: format!("read: {err}"),
                    })?;
                manifest.entries.push(ManifestEntry {
                    relpath,
                    size,
                    sha256,
                });
            } else if meta.file_type().is_symlink() {
                manifest.skipped_special.push(SkippedSpecial {
                    relpath,
                    kind: "symlink",
                });
            } else {
                manifest.skipped_special.push(SkippedSpecial {
                    relpath,
                    kind: "special",
                });
            }
        }
    }
    manifest.entries.sort_by(|a, b| a.relpath.cmp(&b.relpath));
    manifest
        .skipped_special
        .sort_by(|a, b| a.relpath.cmp(&b.relpath));
    Ok(manifest)
}

/// 规范哈希：对排序后的清单行整体 sha256。两次扫描同根必得同值。
pub fn source_root_sha256(manifest: &Manifest) -> String {
    let mut canonical = String::new();
    for entry in &manifest.entries {
        canonical.push_str(&format!(
            "{}  {}  {}\n",
            entry.sha256, entry.size, entry.relpath
        ));
    }
    sha256_hex(canonical.as_bytes())
}

/// 清单序列化（写入 legacy-import/source-manifest.json；前后两份逐字节可比对）。
pub fn manifest_json(manifest: &Manifest) -> serde_json::Value {
    serde_json::json!({
        "files": manifest.entries.iter().map(|e| serde_json::json!({
            "relpath": e.relpath, "size": e.size, "sha256": e.sha256,
        })).collect::<Vec<_>>(),
        "skippedSpecial": manifest.skipped_special.iter().map(|s| serde_json::json!({
            "relpath": s.relpath, "kind": s.kind,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_root(tag: &str) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "t04i-manifest-test-{}-{}-{tag}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn manifest_is_deterministic_and_change_sensitive() {
        let root = temp_root("det");
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("a/b/x.jsonl"), b"{\"k\":1}\n").unwrap();
        std::fs::write(root.join("a/y.md"), b"hello").unwrap();

        let first = build_manifest(&root).unwrap();
        let second = build_manifest(&root).unwrap();
        assert_eq!(first, second);
        assert_eq!(source_root_sha256(&first), source_root_sha256(&second));
        assert_eq!(first.entries.len(), 2);

        std::fs::write(root.join("a/y.md"), b"hellp").unwrap();
        let third = build_manifest(&root).unwrap();
        assert_ne!(source_root_sha256(&first), source_root_sha256(&third));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_skipped_not_followed() {
        let root = temp_root("symlink");
        std::fs::write(root.join("real.txt"), b"data").unwrap();
        std::os::unix::fs::symlink(root.join("real.txt"), root.join("link.txt")).unwrap();
        let manifest = build_manifest(&root).unwrap();
        assert_eq!(manifest.entries.len(), 1);
        assert_eq!(manifest.skipped_special.len(), 1);
        assert_eq!(manifest.skipped_special[0].kind, "symlink");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
