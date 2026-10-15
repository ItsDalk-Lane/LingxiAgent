use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

pub const MAX_TEXT_BYTES: u64 = 5 * 1024 * 1024;
pub const MAX_BINARY_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FileVersion {
    pub mtime_ms: u64,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct TextSnapshot {
    pub content: String,
    pub version: FileVersion,
}

#[derive(Clone, Debug, Serialize)]
pub struct SaveResult {
    pub ok: bool,
    pub conflict: bool,
    pub version: Option<FileVersion>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub path: String,
    pub size: u64,
    pub is_file: bool,
}

pub fn absolute(path: &str) -> io::Result<&Path> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "需要绝对文件路径",
        ));
    }
    Ok(path)
}

fn limited_bytes(path: &str, limit: u64) -> io::Result<Vec<u8>> {
    let file = fs::File::open(absolute(path)?)?;
    let stat = file.metadata()?;
    if !stat.is_file() || stat.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "不是普通文件或文件过大",
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "读取时文件超过限制",
        ));
    }
    Ok(bytes)
}

pub fn info(path: &str) -> io::Result<FileInfo> {
    let stat = fs::metadata(absolute(path)?)?;
    if !stat.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "不是普通文件"));
    }
    Ok(FileInfo {
        path: path.into(),
        size: stat.len(),
        is_file: true,
    })
}

pub fn snapshot(path: &str) -> io::Result<TextSnapshot> {
    let bytes = limited_bytes(path, MAX_TEXT_BYTES)?;
    if bytes.iter().take(8192).any(|b| *b == 0) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "二进制文件"));
    }
    let content = String::from_utf8(bytes.clone())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let modified = fs::metadata(absolute(path)?)?
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?;
    let sha256 = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(TextSnapshot {
        content,
        version: FileVersion {
            mtime_ms: u64::try_from(modified.as_millis()).map_err(io::Error::other)?,
            size: bytes.len() as u64,
            sha256,
        },
    })
}

pub fn base64(path: &str) -> io::Result<String> {
    Ok(STANDARD.encode(limited_bytes(path, MAX_BINARY_BYTES)?))
}

pub fn write(path: &str, content: &str) -> bool {
    absolute(path)
        .and_then(|p| fs::write(p, content.as_bytes()))
        .is_ok()
}

pub fn write_binary(path: &str, data: &str, home: &Path) -> bool {
    let result = (|| -> io::Result<()> {
        let expanded = if path == "~" {
            home.to_path_buf()
        } else if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
            home.join(rest)
        } else {
            PathBuf::from(path)
        };
        if !expanded.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "需要绝对文件路径",
            ));
        }
        let bytes = STANDARD
            .decode(data)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::create_dir_all(
            expanded
                .parent()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "没有父目录"))?,
        )?;
        fs::write(expanded, bytes)
    })();
    result.is_ok()
}

pub fn copy(source: &str, destination: &str) -> bool {
    (|| -> io::Result<()> {
        let source = absolute(source)?;
        let destination = absolute(destination)?;
        let stat = fs::symlink_metadata(source)?;
        if !stat.is_file() || stat.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "源必须是真实普通文件",
            ));
        }
        if fs::canonicalize(destination).ok().as_ref() == Some(&fs::canonicalize(source)?) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "源和目标是同一文件",
            ));
        }
        fs::create_dir_all(
            destination
                .parent()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "没有父目录"))?,
        )?;
        fs::copy(source, destination)?;
        Ok(())
    })()
    .is_ok()
}

pub fn save(path: &str, content: &str, expected: Option<&FileVersion>) -> SaveResult {
    let failed = SaveResult {
        ok: false,
        conflict: false,
        version: None,
    };
    if absolute(path).is_err() {
        return failed;
    }
    if let Some(expected) = expected {
        let current = snapshot(path).ok();
        if current.as_ref().is_none_or(|s| s.version != *expected) {
            return SaveResult {
                ok: false,
                conflict: true,
                version: current.map(|s| s.version),
            };
        }
    }
    // 保留先读后写的原契约；不宣称这个过程具有跨进程原子性。
    if !write(path, content) {
        return failed;
    }
    match snapshot(path) {
        Ok(next) if next.content == content => SaveResult {
            ok: true,
            conflict: false,
            version: Some(next.version),
        },
        _ => failed,
    }
}

pub fn open_file(path: &str) -> Result<(), String> {
    info(path).map_err(|e| e.to_string())?;
    // 等待系统打开请求的结果；出错不能算作成功。窗口实际出现仍由界面检查证明。
    open::that(path).map_err(|e| format!("系统无法打开文件：{e}"))
}

pub fn trash_item(path: &str) -> bool {
    let Ok(path) = absolute(path) else {
        return false;
    };
    if fs::symlink_metadata(path).is_err() {
        return false;
    }
    // 这里只调用系统废纸篓，不提供永久删除回退。macOS 直接调用系统
    // FileManager，避免经 Finder 自动化弹出额外权限请求或长期等待。
    let context = {
        let context = trash::TrashContext::new();
        #[cfg(target_os = "macos")]
        {
            use trash::macos::{DeleteMethod, TrashContextExtMacos};
            let mut context = context;
            context.set_delete_method(DeleteMethod::NsFileManager);
            context
        }
        #[cfg(not(target_os = "macos"))]
        context
    };
    context.delete(path).is_ok() && fs::symlink_metadata(path).is_err()
}

pub fn edit_script(command: &str) -> Result<&'static str, String> {
    match command {
        "cut" => Ok("document.execCommand('cut')"),
        "copy" => Ok("document.execCommand('copy')"),
        "paste" => Ok("document.execCommand('paste')"),
        "selectAll" => Ok("document.execCommand('selectAll')"),
        _ => Err(format!("Unknown edit command: {command}")),
    }
}
