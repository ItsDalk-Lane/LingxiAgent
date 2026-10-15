//! 独立技能查看器的只读文件接口；查看文件绝不安装技能。
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
const MAX_TEXT: u64 = 2 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillViewerPayload {
    pub name: String,
    pub base_dir: String,
    pub file_path: Option<String>,
    pub installed: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeItem {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub children: Vec<TreeItem>,
}
pub fn root(path: &str) -> Option<PathBuf> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return None;
    }
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return None;
    }
    path.canonicalize().ok()
}
pub fn list(path: &str) -> Result<Vec<TreeItem>, String> {
    let Some(base) = root(path) else {
        return Ok(vec![]);
    };
    let mut budget = 10000usize;
    tree(&base, &base, 0, &mut budget).ok_or_else(|| "无法读取技能目录，或目录超过显示范围".into())
}
fn tree(base: &Path, path: &Path, depth: usize, budget: &mut usize) -> Option<Vec<TreeItem>> {
    if depth > 64 {
        return None;
    }
    let mut files = std::fs::read_dir(path)
        .ok()?
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    files.sort_by_key(|file| file.file_name());
    let mut out = Vec::new();
    for file in files {
        if *budget == 0 {
            return None;
        }
        *budget -= 1;
        let meta = file.file_type().ok()?;
        if meta.is_symlink() {
            continue;
        }
        let path = file.path();
        let canonical = path.canonicalize().ok()?;
        if !canonical.starts_with(base) {
            return None;
        }
        let name = file.file_name().into_string().ok()?;
        let children = if meta.is_dir() {
            tree(base, &path, depth + 1, budget)?
        } else {
            vec![]
        };
        if meta.is_file() || meta.is_dir() {
            out.push(TreeItem {
                name,
                path: path.to_str()?.into(),
                is_dir: meta.is_dir(),
                children,
            });
        }
    }
    Some(out)
}
pub fn read(path: &str, allowed_root: Option<&Path>) -> Option<String> {
    read_after_check(path, allowed_root, || {})
}
fn read_after_check(
    path: &str,
    allowed_root: Option<&Path>,
    after_check: impl FnOnce(),
) -> Option<String> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return None;
    }
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_TEXT {
        return None;
    }
    let canonical = path.canonicalize().ok()?;
    if allowed_root.is_some_and(|root| !canonical.starts_with(root)) {
        return None;
    }
    after_check();
    let mut file = std::fs::File::open(&canonical).ok()?;
    let opened = file.metadata().ok()?;
    // 先核对真正打开的文件身份，再读取字节，拒绝检查后替换的文件或目录链接。
    if !same_file(&metadata, &opened)
        || canonical.canonicalize().ok().as_deref() != Some(canonical.as_path())
    {
        return None;
    }
    if opened.len() > MAX_TEXT {
        return None;
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_TEXT + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    let after = file.metadata().ok()?;
    let current = std::fs::symlink_metadata(&canonical).ok()?;
    if !same_file(&opened, &after)
        || !same_file(&after, &current)
        || canonical.canonicalize().ok().as_deref() != Some(canonical.as_path())
    {
        return None;
    }
    if bytes.len() as u64 > MAX_TEXT || bytes.contains(&0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    if !a.is_file() || !b.is_file() || a.len() != b.len() || a.modified().ok() != b.modified().ok()
    {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if (a.dev(), a.ino(), a.ctime(), a.ctime_nsec())
            != (b.dev(), b.ino(), b.ctime(), b.ctime_nsec())
        {
            return false;
        }
    }
    true
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn native_skill_reader_rejects_replacement_link_and_binary_content() {
        let root = std::env::temp_dir().join(format!(
            "r06-skill-read-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let allowed = root.join("skill");
        std::fs::create_dir(&allowed).unwrap();
        let inside = allowed.join("guide.txt");
        let outside = root.join("outside.txt");
        std::fs::write(&inside, "原本获准的正文").unwrap();
        std::fs::write(&outside, "不得越界读取的合成内容").unwrap();
        assert_eq!(
            read(inside.to_str().unwrap(), Some(&allowed)),
            Some("原本获准的正文".into())
        );
        let result = read_after_check(inside.to_str().unwrap(), Some(&allowed), || {
            std::fs::rename(&inside, allowed.join("original.txt")).unwrap();
            std::os::unix::fs::symlink(&outside, &inside).unwrap();
        });
        assert_eq!(result, None, "路径核对后被替换成越界链接，不得返回目标正文");
        assert_eq!(
            std::fs::read_to_string(&outside).unwrap(),
            "不得越界读取的合成内容"
        );
        let binary = allowed.join("binary.bin");
        std::fs::write(&binary, [0, 1, 2, 3]).unwrap();
        assert_eq!(
            read(binary.to_str().unwrap(), Some(&allowed)),
            None,
            "含空字节的二进制不当作正文"
        );
    }
}
