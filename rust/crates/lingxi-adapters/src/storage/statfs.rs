//! R08-T05：文件系统可用空间探测（预检六项之磁盘空间项的取证原料）。
//!
//! - unix：`libc::statvfs`（`f_bavail × f_frsize`——非 root 可用字节数，
//!   与 df 的 Avail 列同口径）。
//! - windows：`GetDiskFreeSpaceExW` 的 `lpFreeBytesAvailableToCaller`。
//! 两侧依赖（libc / windows-sys 0.61.2 + Win32_Storage_FileSystem feature）
//! 均已为 lingxi-adapters 直接依赖——**零新 Cargo.lock `[[package]]` 条目**
//! （裁决点 P2 批准口径，以 lock diff 举证）。
//!
//! 失败即响亮 io::Error，绝不返回估算值或静默降级为「跳过检查」。

use std::io;
use std::path::Path;

/// 指定路径所在卷的调用者可用字节数。路径本身可以尚不存在
/// （沿父链向上解析到第一个已存在组件，与 df 对未存在路径的行为一致）。
pub fn available_bytes(path: &Path) -> io::Result<u64> {
    let anchor = resolve_existing_anchor(path)?;
    platform_available_bytes(&anchor)
}

/// 沿父链找到第一个已存在的绝对锚点；全部不存在（连根都没有）= 响亮错误。
fn resolve_existing_anchor(path: &Path) -> io::Result<std::path::PathBuf> {
    let mut candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    loop {
        if candidate.exists() {
            return Ok(candidate);
        }
        if !candidate.pop() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no existing ancestor for {}", path.display()),
            ));
        }
    }
}

#[cfg(unix)]
fn platform_available_bytes(anchor: &Path) -> io::Result<u64> {
    use std::os::unix::ffi::OsStrExt as _;
    let c_path = std::ffi::CString::new(anchor.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path contains NUL byte: {}", anchor.display()),
        )
    })?;
    // statvfs 结构体必须先零初始化再由内核填充。
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(stat.f_bavail as u64 * stat.f_frsize as u64)
}

#[cfg(windows)]
fn platform_available_bytes(anchor: &Path) -> io::Result<u64> {
    use std::os::windows::ffi::OsStrExt as _;
    let wide: Vec<u16> = anchor
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut free_available: u64 = 0;
    let mut total: u64 = 0;
    let mut total_free: u64 = 0;
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free_available,
            &mut total,
            &mut total_free,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(free_available)
}

#[cfg(not(any(unix, windows)))]
fn platform_available_bytes(_anchor: &Path) -> io::Result<u64> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "available_bytes: unsupported platform (no statvfs equivalent wired)",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_dir_volume_reports_plausible_space() {
        let avail = available_bytes(std::env::temp_dir().as_path()).unwrap();
        // 任何能跑测试的机器都应有 >1MiB 可用；只是 sanity，不钉阈值。
        assert!(avail > 1 << 20, "avail={avail}");
    }

    #[test]
    fn nonexistent_leaf_resolves_to_existing_ancestor() {
        let anchor = std::env::temp_dir().join(format!(
            "t05-statfs-no-such-leaf-{}",
            std::process::id()
        ));
        assert!(!anchor.exists());
        let avail = available_bytes(&anchor).unwrap();
        assert!(avail > 1 << 20);
    }

    #[cfg(unix)]
    #[test]
    fn matches_df_avail_kbytes_within_one_percent() {
        // 与 df -k 同口径交叉验证（允许 1% 漂移：两次采样间文件系统活动）。
        let dir = std::env::temp_dir();
        let ours = available_bytes(&dir).unwrap();
        let out = std::process::Command::new("df")
            .arg("-k")
            .arg(&dir)
            .output()
            .unwrap();
        assert!(out.status.success());
        let stdout = String::from_utf8(out.stdout).unwrap();
        let last = stdout.lines().last().unwrap();
        let df_kib: u64 = last
            .split_whitespace()
            .nth(3)
            .expect("df -k avail column")
            .parse()
            .unwrap();
        let df_bytes = df_kib * 1024;
        let drift = ours.abs_diff(df_bytes) as f64 / df_bytes.max(1) as f64;
        assert!(drift < 0.01, "ours={ours} df={df_bytes} drift={drift}");
    }
}
