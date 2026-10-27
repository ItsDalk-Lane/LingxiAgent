//! 清理仅沿固定目录句柄操作；不跟随站点或外部替换的符号链接。
use super::{error, BrowserError};
use std::{
    ffi::{CStr, CString},
    fs::File,
    os::fd::{AsRawFd, FromRawFd},
    path::Path,
    time::{Duration, Instant},
};

pub(super) fn directory(path: &Path) -> Result<File, BrowserError> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| error("browser_profile", "无法固定浏览器资料根目录", false))
}
fn name(value: &str) -> Result<CString, BrowserError> {
    if value.is_empty() || matches!(value, "." | "..") || value.contains(['/', '\\']) {
        return Err(error("browser_profile", "资料名称不是单一目录项", false));
    }
    CString::new(value).map_err(|_| error("browser_profile", "资料名称无效", false))
}
fn metadata(parent: &File, name: &CStr) -> Result<Option<libc::stat>, BrowserError> {
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } == 0
    {
        return Ok(Some(unsafe { metadata.assume_init() }));
    }
    if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
        return Ok(None);
    }
    Err(error("browser_profile", "无法确认待清理资料", true))
}
fn open_child(parent: &File, name: &CStr) -> Result<Option<File>, BrowserError> {
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd >= 0 {
        return Ok(Some(unsafe { File::from_raw_fd(fd) }));
    }
    if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
        return Ok(None);
    }
    Err(error("browser_profile", "资料目录包含链接或不可访问", true))
}
fn entries(dir: &File, limit: usize) -> Result<Vec<CString>, BrowserError> {
    let cloned = dir
        .try_clone()
        .map_err(|_| error("browser_profile", "无法枚举资料目录", true))?;
    // dup共享目录偏移，每次枚举显式回到起点；调用方独占相应资料清理。
    let raw = unsafe { libc::fdopendir(cloned.as_raw_fd()) };
    if raw.is_null() {
        return Err(error("browser_profile", "无法枚举资料目录", true));
    }
    std::mem::forget(cloned);
    struct Directory(*mut libc::DIR);
    impl Drop for Directory {
        fn drop(&mut self) {
            unsafe {
                libc::closedir(self.0);
            }
        }
    }
    let directory = Directory(raw);
    unsafe {
        libc::rewinddir(directory.0);
    }
    let mut result = Vec::new();
    loop {
        let entry = unsafe { libc::readdir(directory.0) };
        if entry.is_null() {
            break;
        }
        let value = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        if value.to_bytes() == b"." || value.to_bytes() == b".." {
            continue;
        }
        if result.len() >= limit {
            return Err(error(
                "browser_profile_budget",
                "资料目录条目超过清理预算",
                true,
            ));
        }
        result.push(value.to_owned());
    }
    Ok(result)
}
pub(super) fn profile_names(root: &File) -> Result<Vec<String>, BrowserError> {
    entries(root, 10_000)?
        .into_iter()
        .map(|value| {
            let value = value
                .to_str()
                .map_err(|_| error("browser_profile", "资料目录身份无效", false))?;
            if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(error("browser_profile", "资料目录身份无效", false));
            }
            Ok(value.to_owned())
        })
        .collect()
}
struct Budget {
    remaining: usize,
    deadline: Instant,
}
fn erase(
    parent: &File,
    value: &CStr,
    links: bool,
    depth: usize,
    budget: &mut Budget,
) -> Result<(), BrowserError> {
    if depth > 128 || budget.remaining == 0 || Instant::now() >= budget.deadline {
        return Err(error(
            "browser_profile_budget",
            "资料清理超过预算，请重试",
            true,
        ));
    }
    budget.remaining -= 1;
    let Some(before) = metadata(parent, value)? else {
        return Ok(());
    };
    let kind = before.st_mode & libc::S_IFMT;
    if kind == libc::S_IFLNK && !links {
        return Err(error("browser_profile", "资料路径包含链接，拒绝清理", true));
    }
    let flags = if kind == libc::S_IFDIR {
        let Some(child) = open_child(parent, value)? else {
            return Ok(());
        };
        use std::os::unix::fs::MetadataExt;
        let opened = child
            .metadata()
            .map_err(|_| error("browser_profile", "无法确认资料句柄", true))?;
        if u128::from(opened.dev()) != before.st_dev as u128
            || u128::from(opened.ino()) != u128::from(before.st_ino)
        {
            return Err(error("browser_profile", "资料目录在清理前被替换", true));
        }
        for entry in entries(&child, budget.remaining)? {
            erase(&child, &entry, true, depth + 1, budget)?;
        }
        libc::AT_REMOVEDIR
    } else {
        0
    };
    let Some(after) = metadata(parent, value)? else {
        return Ok(());
    };
    if after.st_dev != before.st_dev
        || after.st_ino != before.st_ino
        || after.st_mode & libc::S_IFMT != kind
    {
        return Err(error("browser_profile", "资料名称在清理期间被替换", true));
    }
    if unsafe { libc::unlinkat(parent.as_raw_fd(), value.as_ptr(), flags) } != 0
        && std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOENT)
    {
        return Err(error("browser_profile", "资料删除尚未完成", true));
    }
    Ok(())
}
pub(super) fn purge(root: &File, components: &[&str]) -> Result<(), BrowserError> {
    let Some((last, parents)) = components.split_last() else {
        return Err(error("browser_profile", "不能清理整个资料根", false));
    };
    let mut parent = root
        .try_clone()
        .map_err(|_| error("browser_profile", "无法固定资料根", true))?;
    for component in parents {
        let value = name(component)?;
        let Some(child) = open_child(&parent, &value)? else {
            return Ok(());
        };
        parent = child;
    }
    erase(
        &parent,
        &name(last)?,
        false,
        0,
        &mut Budget {
            remaining: 100_000,
            deadline: Instant::now() + Duration::from_secs(10),
        },
    )
}

fn ensure_child(parent: &File, component: &str) -> Result<File, BrowserError> {
    let component = name(component)?;
    if let Some(child) = open_child(parent, &component)? {
        return Ok(child);
    }
    if unsafe { libc::mkdirat(parent.as_raw_fd(), component.as_ptr(), 0o700) } != 0
        && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST)
    {
        return Err(error("browser_profile", "无法建立原生资料目录", false));
    }
    open_child(parent, &component)?
        .ok_or_else(|| error("browser_profile", "原生资料目录未建立", false))
}
fn object<'a>(
    parent: &'a mut serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<&'a mut serde_json::Map<String, serde_json::Value>, BrowserError> {
    parent
        .entry(key.to_owned())
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| {
            error(
                "browser_profile",
                "原生偏好对象格式不兼容，未覆盖旧值",
                false,
            )
        })
}
fn cookie_preferences(value: &mut serde_json::Value, accept: bool) -> Result<(), BrowserError> {
    use serde_json::{json, Value};
    const BACKUP: &str = "lingxi_browser_cookie_policy";
    let root = value
        .as_object_mut()
        .ok_or_else(|| error("browser_profile", "原生偏好不是对象", false))?;
    let saved = match root.get(BACKUP) {
        None => None,
        Some(policy) if policy["version"] == 1 => {
            if let Some(saved) = policy.get("savedExceptions") {
                if !saved["present"].is_boolean()
                    || (saved["present"] == true && !saved["value"].is_object())
                {
                    return Err(error(
                        "browser_profile",
                        "Cookie例外备份不完整，未覆盖旧值",
                        false,
                    ));
                }
                Some(saved.clone())
            } else {
                None
            }
        }
        Some(_) => {
            return Err(error(
                "browser_profile",
                "Cookie备份名称已存在不兼容内容",
                false,
            ))
        }
    };
    let backup = {
        let profile = object(root, "profile")?;
        object(profile, "default_content_setting_values")?
            .insert("cookies".into(), json!(if accept { 1 } else { 2 }));
        let exceptions = object(object(profile, "content_settings")?, "exceptions")?;
        if !accept {
            let previous = exceptions.get("cookies").cloned();
            if previous.as_ref().is_some_and(|old| !old.is_object()) {
                return Err(error("browser_profile", "Cookie例外格式不兼容", false));
            }
            exceptions.insert("cookies".into(), json!({}));
            Some(saved.unwrap_or_else(|| json!({"present":previous.is_some(),"value":previous})))
        } else {
            if let Some(saved) = saved {
                if saved["present"] == true {
                    exceptions.insert("cookies".into(), saved["value"].clone());
                } else {
                    exceptions.remove("cookies");
                }
            }
            None
        }
    };
    if let Some(backup) = backup {
        let policy = object(root, BACKUP)?;
        policy.insert("version".into(), json!(1));
        policy.insert("savedExceptions".into(), backup);
    } else if let Some(Value::Object(policy)) = root.get_mut(BACKUP) {
        policy.remove("savedExceptions");
        if policy.len() == 1 && policy["version"] == 1 {
            root.remove(BACKUP);
        }
    }
    Ok(())
}
/// 只在对应Chrome进程确认退出后调用；原生默认策略与例外均在同一Preferences中原子切换。
pub(super) fn configure_cookies(
    root: &File,
    profile: &str,
    accept: bool,
) -> Result<(), BrowserError> {
    use std::io::{Read, Write};
    use std::os::unix::fs::MetadataExt;
    let profile = ensure_child(root, profile)?;
    let directory = ensure_child(&profile, "Default")?;
    let filename = name("Preferences")?;
    let original = metadata(&directory, &filename)?;
    let mut value = if let Some(before) = original.as_ref() {
        if before.st_mode & libc::S_IFMT != libc::S_IFREG
            || before.st_size < 0
            || before.st_size > 16 * 1024 * 1024
        {
            return Err(error("browser_profile", "原生偏好不是有界常规文件", false));
        }
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                filename.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(error("browser_profile", "无法固定原生偏好", false));
        }
        let mut file = unsafe { File::from_raw_fd(fd) };
        let opened = file
            .metadata()
            .map_err(|_| error("browser_profile", "无法确认原生偏好句柄", false))?;
        if u128::from(opened.dev()) != before.st_dev as u128
            || u128::from(opened.ino()) != u128::from(before.st_ino)
        {
            return Err(error("browser_profile", "原生偏好已被替换", false));
        }
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| error("browser_profile", "读取原生偏好失败", false))?;
        if bytes.len() > 16 * 1024 * 1024
            || file.metadata().ok().is_none_or(|after| {
                after.len() != opened.len()
                    || after.mtime() != opened.mtime()
                    || after.mtime_nsec() != opened.mtime_nsec()
            })
        {
            return Err(error("browser_profile", "读取时原生偏好发生变化", false));
        }
        serde_json::from_slice(&bytes)
            .map_err(|_| error("browser_profile", "原生偏好JSON无效，未覆盖旧值", false))?
    } else {
        serde_json::json!({})
    };
    cookie_preferences(&mut value, accept)?;
    let encoded = serde_json::to_vec(&value)
        .map_err(|_| error("browser_profile", "无法编码原生偏好", false))?;
    if encoded.len() > 16 * 1024 * 1024 {
        return Err(error("browser_profile", "原生偏好超过写入预算", false));
    }
    let mut random = [0u8; 16];
    getrandom::getrandom(&mut random)
        .map_err(|_| error("browser_random", "无法分配原生偏好事务", false))?;
    let temporary = name(&format!(
        ".lingxi-cookie-{}",
        random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            temporary.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(error("browser_profile", "无法建立原生偏好事务", false));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let result = (|| {
        file.write_all(&encoded)
            .and_then(|_| file.sync_all())
            .map_err(|_| error("browser_profile", "原生偏好未完整落盘", false))?;
        let actual = metadata(&directory, &filename)?;
        if match (original.as_ref(), actual.as_ref()) {
            (None, None) => false,
            (Some(a), Some(b)) => {
                a.st_dev != b.st_dev
                    || a.st_ino != b.st_ino
                    || a.st_size != b.st_size
                    || a.st_mtime != b.st_mtime
            }
            _ => true,
        } {
            return Err(error("browser_profile", "原生偏好在写入前被替换", false));
        }
        if unsafe {
            libc::renameat(
                directory.as_raw_fd(),
                temporary.as_ptr(),
                directory.as_raw_fd(),
                filename.as_ptr(),
            )
        } != 0
        {
            return Err(error("browser_profile", "原生偏好原子提交失败", false));
        }
        directory
            .sync_all()
            .map_err(|_| error("browser_profile", "原生偏好提交尚未确认", true))?;
        Ok(())
    })();
    if result.is_err() {
        unsafe {
            libc::unlinkat(directory.as_raw_fd(), temporary.as_ptr(), 0);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn purge_is_pinned_to_owned_parent_and_never_follows_replaced_links() {
        use std::os::unix::fs::symlink;
        let mut random = [0u8; 16];
        getrandom::getrandom(&mut random).unwrap();
        let root = std::env::temp_dir().join(format!(
            "browser-purge-{}",
            random
                .iter()
                .map(|n| format!("{n:02x}"))
                .collect::<String>()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("owned")).unwrap();
        std::fs::create_dir(root.join("external")).unwrap();
        std::fs::write(root.join("external/keep"), b"external").unwrap();
        let pinned = directory(&root.join("owned")).unwrap();
        std::fs::create_dir(root.join("owned/profile")).unwrap();
        symlink(root.join("external"), root.join("owned/profile/link")).unwrap();
        std::fs::rename(root.join("owned"), root.join("moved")).unwrap();
        symlink(root.join("external"), root.join("owned")).unwrap();
        purge(&pinned, &["profile"]).unwrap();
        assert!(!root.join("moved/profile").exists());
        assert_eq!(
            std::fs::read(root.join("external/keep")).unwrap(),
            b"external"
        );
        symlink(root.join("external"), root.join("moved/target")).unwrap();
        assert!(purge(&pinned, &["target"]).is_err());
        assert_eq!(
            std::fs::read(root.join("external/keep")).unwrap(),
            b"external"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_cookie_preferences_keep_unknown_data_and_restore_site_exceptions() {
        let mut random = [0u8; 16];
        getrandom::getrandom(&mut random).unwrap();
        let root = std::env::temp_dir().join(format!(
            "browser-pref-{}",
            random
                .iter()
                .map(|n| format!("{n:02x}"))
                .collect::<String>()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir_all(root.join("profile/Default")).unwrap();
        let path = root.join("profile/Default/Preferences");
        let original = serde_json::json!({"unknown":{"keep":[1,"future"]},"profile":{"other":42,"content_settings":{"exceptions":{"cookies":{"https://local.invalid,*":{"setting":1,"future":true}},"javascript":{"keep":7}}}}});
        std::fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
        let pinned = directory(&root).unwrap();
        configure_cookies(&pinned, "profile", false).unwrap();
        configure_cookies(&pinned, "profile", false).unwrap();
        let blocked: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(blocked["unknown"], original["unknown"]);
        assert_eq!(
            blocked["profile"]["default_content_setting_values"]["cookies"],
            2
        );
        assert_eq!(
            blocked["profile"]["content_settings"]["exceptions"]["cookies"],
            serde_json::json!({})
        );
        configure_cookies(&pinned, "profile", true).unwrap();
        let allowed: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(allowed["unknown"], original["unknown"]);
        assert_eq!(allowed["profile"]["other"], 42);
        assert_eq!(
            allowed["profile"]["content_settings"],
            original["profile"]["content_settings"]
        );
        assert_eq!(
            allowed["profile"]["default_content_setting_values"]["cookies"],
            1
        );
        std::fs::write(&path, b"invalid existing JSON").unwrap();
        assert!(configure_cookies(&pinned, "profile", false).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"invalid existing JSON");
        std::fs::remove_file(&path).unwrap();
        std::fs::write(root.join("external"), b"outside").unwrap();
        std::os::unix::fs::symlink(root.join("external"), &path).unwrap();
        assert!(configure_cookies(&pinned, "profile", false).is_err());
        assert_eq!(std::fs::read(root.join("external")).unwrap(), b"outside");
        std::fs::remove_dir_all(root).unwrap();
    }
}
