//! Windows真实生产后端及父层文件/目录API接线的目标类型检查。
//! 不运行、不创建Run，不把本文件当作完整service交叉构建或Windows实机证明。
#![cfg(windows)]
#[path = "../../rust/crates/lingxi-service/src/discovery_windows.rs"]
pub mod windows_readonly;
use std::{
    ffi::OsStr,
    io::{self, Read},
    path::Path,
};

/// 与父Engine使用同一实际生产后端API及完整身份，保留DirFd直到后验核对结束。
pub fn parent_api_types(path: &Path, name: &OsStr, cancelled: impl Fn() -> bool) -> io::Result<()> {
    use windows_readonly::{file_identity, DirFd};
    let dir = DirFd::open(path)?;
    let _: (u64, u128) = dir.directory_identity()?;
    let _ = dir.entry_names(cancelled)?;
    let before = dir.entry_metadata(name)?;
    if before.kind != "file" {
        return Err(io::Error::other("native regular file required"));
    }
    let mut file = dir
        .open_attachment_nofollow(name)
        .unwrap_or_else(|| Err(io::Error::from(io::ErrorKind::NotFound)))?;
    if !file.metadata()?.is_file() || file_identity(&file)? != (before.device, before.inode) {
        return Err(io::Error::other("native file identity changed"));
    }
    let mut buffer = [0_u8; 64 * 1024];
    let _ = file.read(&mut buffer)?;
    if dir.entry_metadata(name)? != before || !dir.same_directory(path)? {
        return Err(io::Error::other("native directory or file changed"));
    }
    Ok(())
}
