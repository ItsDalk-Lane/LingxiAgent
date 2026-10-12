//! 将已提交的管理操作意图投影到旧服务共用的安全审计 JSONL。
//! 注册簿中的意图是持久来源；日志写入失败后可在下一次写入或重启时补齐。

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

static APPEND_LOCK: Mutex<()> = Mutex::new(());
const MAX_LOG_BYTES: u64 = 64 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 64 * 1024;
const MAX_INTENTS: usize = 100_000;

struct OpenAuditLog {
    file: File,
    // 目录句柄保留到日志 fsync 之后，Windows 禁止目录被重命名或删除。
    _home: File,
    _logs: File,
}

fn open_log(home: &Path) -> Result<OpenAuditLog, String> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let home_path = CString::new(home.as_os_str().as_bytes())
            .map_err(|_| "security audit home contains NUL".to_string())?;
        let descriptor = unsafe {
            libc::open(
                home_path.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(format!(
                "security audit home open failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let home_directory = unsafe { File::from_raw_fd(descriptor) };
        let name = CString::new("logs").expect("fixed audit directory");
        let created = unsafe { libc::mkdirat(home_directory.as_raw_fd(), name.as_ptr(), 0o700) };
        if created < 0
            && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(format!(
                "security audit log directory creation failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let descriptor = unsafe {
            libc::openat(
                home_directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(format!(
                "security audit log directory open failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let directory = unsafe { File::from_raw_fd(descriptor) };
        let meta = directory
            .metadata()
            .map_err(|err| format!("security audit log directory metadata failed: {err}"))?;
        if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } {
            return Err("security audit log directory is not an owned directory".into());
        }
        if meta.permissions().mode() & 0o777 != 0o700 {
            directory
                .set_permissions(std::fs::Permissions::from_mode(0o700))
                .map_err(|err| format!("security audit log directory permissions failed: {err}"))?;
        }
        let name = CString::new("security-audit.jsonl").expect("fixed audit filename");
        let descriptor = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDWR | libc::O_APPEND | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if descriptor < 0 {
            return Err(format!(
                "security audit log open failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let file = unsafe { File::from_raw_fd(descriptor) };
        let meta = file
            .metadata()
            .map_err(|err| format!("security audit log metadata failed: {err}"))?;
        if !meta.is_file() || meta.nlink() != 1 || meta.uid() != unsafe { libc::geteuid() } {
            return Err("security audit log is not an owned independent regular file".into());
        }
        if meta.permissions().mode() & 0o777 != 0o600 {
            file.set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|err| format!("security audit log permissions failed: {err}"))?;
        }
        Ok(OpenAuditLog {
            file,
            _home: home_directory,
            _logs: directory,
        })
    }
    #[cfg(windows)]
    {
        windows_audit::open_log(home)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = home;
        Err("security audit log is unsupported on this platform".into())
    }
}

#[cfg(windows)]
mod windows_audit {
    use std::fs::File;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::MetadataExt;
    use std::os::windows::io::FromRawHandle;
    use std::path::Path;

    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::{
        NtCreateFile, RtlDosPathNameToNtPathName_U_WithStatus, FILE_DIRECTORY_FILE,
        FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_IF, FILE_SYNCHRONOUS_IO_NONALERT,
    };
    use windows_sys::Win32::Foundation::{
        HANDLE, INVALID_HANDLE_VALUE, OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE, UNICODE_STRING,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY, FILE_APPEND_DATA, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE,
        READ_CONTROL, SYNCHRONIZE, WRITE_DAC,
    };
    use windows_sys::Win32::System::WindowsProgramming::RtlFreeUnicodeString;
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    struct NtPath(UNICODE_STRING);

    impl Drop for NtPath {
        fn drop(&mut self) {
            // RtlDosPathNameToNtPathName_U_WithStatus 分配的缓冲区只能由配对函数释放。
            unsafe { RtlFreeUnicodeString(&mut self.0) };
        }
    }

    fn nt_path(home: &Path) -> Result<NtPath, String> {
        let mut dos_name: Vec<u16> = home.as_os_str().encode_wide().collect();
        if dos_name.contains(&0) {
            return Err("security audit home contains NUL".into());
        }
        dos_name.push(0);
        let mut name = UNICODE_STRING::default();
        let status = unsafe {
            RtlDosPathNameToNtPathName_U_WithStatus(
                dos_name.as_ptr(),
                &mut name,
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        if status < 0 {
            return Err(format!(
                "security audit home path conversion failed: NTSTATUS {status:#010x}"
            ));
        }
        Ok(NtPath(name))
    }

    fn open_relative(
        root: HANDLE,
        name: &mut UNICODE_STRING,
        access: u32,
        disposition: u32,
        options: u32,
    ) -> Result<File, String> {
        let attributes = OBJECT_ATTRIBUTES {
            Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: root,
            ObjectName: name,
            Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
            SecurityDescriptor: std::ptr::null(),
            SecurityQualityOfService: std::ptr::null(),
        };
        let mut status_block = IO_STATUS_BLOCK::default();
        let mut handle: HANDLE = std::ptr::null_mut();
        let status = unsafe {
            NtCreateFile(
                &mut handle,
                access,
                &attributes,
                &mut status_block,
                std::ptr::null(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                disposition,
                options | FILE_SYNCHRONOUS_IO_NONALERT,
                std::ptr::null(),
                0,
            )
        };
        if status < 0 || handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return Err(format!(
                "security audit path open failed: NTSTATUS {status:#010x}"
            ));
        }
        // NtCreateFile 成功后句柄归 File 管理；关闭句柄前不会经路径重新解析。
        Ok(unsafe { File::from_raw_handle(handle) })
    }

    fn child_name(name: &str) -> (Vec<u16>, UNICODE_STRING) {
        let mut wide: Vec<u16> = name.encode_utf16().collect();
        let bytes = (wide.len() * 2) as u16;
        let unicode = UNICODE_STRING {
            Length: bytes,
            MaximumLength: bytes,
            Buffer: wide.as_mut_ptr(),
        };
        (wide, unicode)
    }

    fn check_real(file: &File, directory: bool) -> Result<(), String> {
        let meta = file
            .metadata()
            .map_err(|err| format!("security audit handle metadata failed: {err}"))?;
        let correct_type = if directory {
            meta.is_dir()
        } else {
            meta.is_file()
        };
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !correct_type {
            return Err("security audit path is a reparse point or wrong file type".into());
        }
        if !directory && meta.number_of_links() != Some(1) {
            return Err("security audit log is not an independent regular file".into());
        }
        Ok(())
    }

    pub(super) fn open_log(home: &Path) -> Result<super::OpenAuditLog, String> {
        // 绝对路径用 OBJ_DONT_REPARSE 拒绝任一祖先链接；其后仅用固定目录句柄逐层打开。
        let mut root_name = nt_path(home)?;
        let home_dir = open_relative(
            std::ptr::null_mut(),
            &mut root_name.0,
            FILE_TRAVERSE
                | FILE_ADD_SUBDIRECTORY
                | FILE_READ_ATTRIBUTES
                | READ_CONTROL
                | SYNCHRONIZE
                | WRITE_DAC,
            FILE_OPEN,
            FILE_DIRECTORY_FILE,
        )?;
        check_real(&home_dir, true)?;
        lingxi_adapters::storage::windows_acl::verify_private_handle(&home_dir, true)
            .map_err(|err| format!("security audit home ACL failed: {err}"))?;

        let (_wide, mut name) = child_name("logs");
        let logs = open_relative(
            std::os::windows::io::AsRawHandle::as_raw_handle(&home_dir),
            &mut name,
            FILE_TRAVERSE
                | FILE_ADD_FILE
                | FILE_READ_ATTRIBUTES
                | READ_CONTROL
                | SYNCHRONIZE
                | WRITE_DAC,
            FILE_OPEN_IF,
            FILE_DIRECTORY_FILE,
        )?;
        check_real(&logs, true)?;
        lingxi_adapters::storage::windows_acl::protect_handle(&logs, true)
            .map_err(|err| format!("security audit directory ACL failed: {err}"))?;

        let (_wide, mut name) = child_name("security-audit.jsonl");
        let log = open_relative(
            std::os::windows::io::AsRawHandle::as_raw_handle(&logs),
            &mut name,
            FILE_READ_DATA
                | FILE_APPEND_DATA
                | FILE_READ_ATTRIBUTES
                | READ_CONTROL
                | SYNCHRONIZE
                | WRITE_DAC,
            FILE_OPEN_IF,
            FILE_NON_DIRECTORY_FILE,
        )?;
        check_real(&log, false)?;
        lingxi_adapters::storage::windows_acl::protect_handle(&log, false)
            .map_err(|err| format!("security audit file ACL failed: {err}"))?;
        Ok(super::OpenAuditLog {
            file: log,
            _home: home_dir,
            _logs: logs,
        })
    }
}

/// 旧记录没有 eventId，按来源、序号和不变的事件身份推导稳定值，重试不重复落日志。
pub(crate) fn project<T: Serialize>(
    home: &Path,
    source: &str,
    intents: &[T],
) -> Result<(), String> {
    let _guard = APPEND_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if intents.len() > MAX_INTENTS {
        return Err("security audit outbox exceeds bounded count".into());
    }
    let mut opened = open_log(home)?;
    let file = &mut opened.file;
    let mut length = file
        .metadata()
        .map_err(|err| format!("security audit log metadata failed: {err}"))?
        .len();
    if length > MAX_LOG_BYTES {
        return Err("security audit log exceeds bounded size".into());
    }
    let mut seen = HashSet::new();
    let mut reader = BufReader::new(&*file);
    let mut line = Vec::new();
    loop {
        let chunk = reader
            .fill_buf()
            .map_err(|err| format!("security audit log read failed: {err}"))?;
        if chunk.is_empty() {
            if !line.is_empty() {
                return Err("security audit log ends with an incomplete line".into());
            }
            break;
        }
        let newline = chunk.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(chunk.len(), |index| index + 1);
        if line.len().saturating_add(take) > MAX_LINE_BYTES + 1 {
            return Err("security audit log line exceeds bounded size".into());
        }
        line.extend_from_slice(&chunk[..take]);
        reader.consume(take);
        if newline.is_some() {
            line.pop();
            let value: Value = serde_json::from_slice(&line)
                .map_err(|err| format!("security audit log contains invalid JSON: {err}"))?;
            if let Some(id) = value.get("eventId").and_then(Value::as_str) {
                seen.insert(id.to_string());
            }
            line.clear();
        }
    }
    drop(reader);
    for (index, intent) in intents.iter().enumerate() {
        let value = serde_json::to_value(intent)
            .map_err(|e| format!("security audit intent invalid: {e}"))?;
        let action = value
            .get("action")
            .and_then(Value::as_str)
            .ok_or("security audit action missing")?;
        let target = value
            .get("target")
            .and_then(Value::as_str)
            .ok_or("security audit target missing")?;
        let at = value
            .get("atUnixMs")
            .and_then(Value::as_u64)
            .ok_or("security audit time missing")?;
        let timestamp = DateTime::<Utc>::from_timestamp_millis(
            i64::try_from(at).map_err(|_| "security audit time overflow")?,
        )
        .ok_or("security audit time invalid")?
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let mut digest = Sha256::new();
        digest.update(source.as_bytes());
        digest.update(index.to_le_bytes());
        digest.update(action.as_bytes());
        digest.update(target.as_bytes());
        digest.update(at.to_le_bytes());
        let mut event_id = String::from("sec_");
        for byte in digest.finalize() {
            use std::fmt::Write as _;
            let _ = write!(event_id, "{byte:02x}");
        }
        if seen.contains(&event_id) {
            continue;
        }
        let metadata = value
            .get("metadata")
            .filter(|entry| entry.is_object())
            .cloned()
            .unwrap_or_else(|| json!({}));
        let actor = value
            .get("actor")
            .filter(|entry| entry.is_object())
            .cloned()
            .unwrap_or_else(|| {
                json!({
                    "principalId": "principal_local_user_user_local_no_studio_no_node",
                    "kind": "local_user", "userId": "user_local", "studioId": null,
                    "serverId": null, "serverNodeId": null, "deviceId": null,
                    "credentialId": null, "agentId": null, "pluginId": null,
                    "bridgeAccountId": null, "platformAccountId": null,
                    "officialServiceKind": null, "connectionKind": "local",
                    "credentialKind": "loopback_token", "trustState": "local"
                })
            });
        let secret_fields =
            if action.ends_with("credential.issue") || action == "devices.pairing.approve" {
                vec!["secret"]
            } else if action == "access.account.password.update" {
                vec!["password"]
            } else {
                Vec::new()
            };
        // 普通管理写入在受信端核验本地主人；密码迁移保留实际登录连接身份。
        let event = json!({
            "schemaVersion": 1, "eventId": event_id.clone(), "timestamp": timestamp,
            "action": action, "target": target, "result": "success",
            "actor": actor,
            "decision": null, "leaseId": null, "errorCode": null,
            "secretFields": secret_fields, "metadata": metadata
        });
        let mut line =
            serde_json::to_vec(&event).map_err(|e| format!("security audit event invalid: {e}"))?;
        line.push(b'\n');
        if line.len() > MAX_LINE_BYTES {
            return Err("security audit event exceeds bounded line size".into());
        }
        length = length
            .checked_add(line.len() as u64)
            .ok_or("security audit log size overflow")?;
        if length > MAX_LOG_BYTES {
            return Err("security audit log would exceed bounded size".into());
        }
        file.write_all(&line)
            .map_err(|err| format!("security audit log append failed: {err}"))?;
        seen.insert(event_id);
    }
    file.sync_all()
        .map_err(|e| format!("security audit log sync failed: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(name: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "lingxi-security-audit-{name}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }

    fn intent() -> Value {
        json!({"action":"devices.credential.issue","target":"device_test","atUnixMs":1,"metadata":{"credentialId":"cred_test"}})
    }

    #[test]
    fn replay_is_idempotent_and_never_writes_secret() {
        let root = home("replay");
        let mut item = intent();
        item["secret"] = json!("private-test-secret");
        project(&root, "device", &[item.clone()]).unwrap();
        project(&root, "device", &[item]).unwrap();
        let log = std::fs::read_to_string(root.join("logs/security-audit.jsonl")).unwrap();
        assert_eq!(log.lines().count(), 1);
        assert!(!log.contains("private-test-secret"));
        let event: Value = serde_json::from_str(log.lines().next().unwrap()).unwrap();
        assert_eq!(event["metadata"]["credentialId"], "cred_test");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_and_oversized_logs_fail_without_appending() {
        let root = home("bounds");
        let logs = root.join("logs");
        std::fs::create_dir(&logs).unwrap();
        let path = logs.join("security-audit.jsonl");
        for content in [
            b"{broken\n".as_slice(),
            b"{\"eventId\":\"partial\"}".as_slice(),
        ] {
            std::fs::write(&path, content).unwrap();
            assert!(project(&root, "device", &[intent()]).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), content);
        }
        let oversized = vec![b'x'; MAX_LINE_BYTES + 1];
        std::fs::write(&path, &oversized).unwrap();
        assert!(project(&root, "device", &[intent()]).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), oversized);
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(MAX_LOG_BYTES + 1).unwrap();
        assert!(project(&root, "device", &[intent()]).is_err());
        assert_eq!(std::fs::metadata(&path).unwrap().len(), MAX_LOG_BYTES + 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn log_and_directory_symlinks_are_rejected_without_touching_target() {
        let root = home("symlink");
        let target = root.join("other.jsonl");
        std::fs::write(&target, b"").unwrap();
        let logs = root.join("logs");
        std::fs::create_dir(&logs).unwrap();
        std::os::unix::fs::symlink(&target, logs.join("security-audit.jsonl")).unwrap();
        assert!(project(&root, "device", &[intent()]).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"");
        std::fs::remove_dir_all(&logs).unwrap();
        let alternate = root.join("alternate");
        std::fs::create_dir(&alternate).unwrap();
        std::os::unix::fs::symlink(&alternate, &logs).unwrap();
        assert!(project(&root, "device", &[intent()]).is_err());
        assert!(std::fs::read_dir(&alternate).unwrap().next().is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_reparse_points_and_hardlinks_never_redirect_audit() {
        use std::os::windows::fs::{symlink_dir, symlink_file};

        let root = home("windows-links");
        let logs = root.join("logs");
        std::fs::create_dir(&logs).unwrap();
        let target = root.join("other.jsonl");
        std::fs::write(&target, b"").unwrap();
        let log = logs.join("security-audit.jsonl");

        // Windows 真机必须能够创建测试链接，否则本项环境验收不能记 PASS。
        symlink_file(&target, &log).expect("Windows test requires file symlink capability");
        assert!(project(&root, "device", &[intent()]).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"");
        std::fs::remove_file(&log).unwrap();

        std::fs::hard_link(&target, &log).unwrap();
        assert!(project(&root, "device", &[intent()]).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"");
        std::fs::remove_file(&log).unwrap();

        std::fs::remove_dir(&logs).unwrap();
        let alternate = root.join("alternate");
        std::fs::create_dir(&alternate).unwrap();
        symlink_dir(&alternate, &logs).expect("Windows test requires directory symlink capability");
        assert!(project(&root, "device", &[intent()]).is_err());
        assert!(std::fs::read_dir(&alternate).unwrap().next().is_none());
        std::fs::remove_dir(&logs).unwrap();

        let alias = root.with_extension("root-link");
        symlink_dir(&root, &alias).expect("Windows test requires root symlink capability");
        assert!(project(&alias, "device", &[intent()]).is_err());
        std::fs::remove_dir(&alias).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_open_handle_prevents_path_replacement_until_closed() {
        let root = home("windows-handle");
        let file = windows_audit::open_log(&root).unwrap();
        let log = root.join("logs/security-audit.jsonl");
        assert!(std::fs::remove_file(&log).is_err());
        assert!(std::fs::rename(&log, root.join("replacement.jsonl")).is_err());
        drop(file);
        project(&root, "device", &[intent()]).unwrap();
        assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}
