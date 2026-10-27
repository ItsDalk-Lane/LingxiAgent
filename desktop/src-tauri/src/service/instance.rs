//! R09-T02 实例/令牌私有记录读取（C13/C16/C10）。
//!
//! 语义移植自 Electron `desktop/src/shared/rust-local-service.cjs:217-317`：
//! - `instance.json` 只是定位线索，存活权威是服务自己的 OS 文件锁；
//! - `local-token.json` 必须 owner-only（0600、uid 必须是本进程）；
//! - 两个文件都以 O_NOFOLLOW 打开，打开前后 dev/ino 必须一致（拒绝竞态替换）；
//! - 记录里的 pid/home/instanceId/bindAddr 必须与 READY 行和真实 home 一致；
//! - 记录里的 `wireProtocolMin/Max`、`dataEpoch` 是运行时协议门（A03 负向）。
//!
//! 宿主绝不写/删 `instance.json`/`instance.lock`，也绝不按裸 PID 杀进程。

use std::fs;
use std::io::Read;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::error::{codes, ServiceResult, StartError};
use super::manifest::{ProtocolIdentity, SERVER_KIND};

pub const RUNTIME_DIR_NAME: &str = "lingxi-service";
pub const RECORD_FILE_NAME: &str = "instance.json";
pub const TOKEN_FILE_NAME: &str = "local-token.json";

#[derive(Debug, Clone)]
pub struct InstanceRecord {
    pub schema_version: u64,
    pub server_kind: String,
    pub wire_protocol_min: u64,
    pub wire_protocol_max: u64,
    pub data_epoch: u64,
    pub instance_id: String,
    pub pid: u32,
    pub bind_addr: Option<String>,
    pub transport: Option<String>,
    pub home_path: String,
}

#[derive(Debug, Clone)]
pub struct OwnedConnection {
    /// `127.0.0.1` / `[::1]`。
    pub host: String,
    pub port: u16,
    pub transport: String,
    pub base_url: String,
    pub token: String,
    pub instance_id: String,
    pub pid: u32,
    pub record: InstanceRecord,
}

fn record_error(message: impl Into<String>, detail: Value) -> StartError {
    StartError::new(codes::SERVICE_INSTANCE_INVALID, message).with_detail(detail)
}

fn io_error(context: &str, path: &Path, error: std::io::Error) -> StartError {
    record_error(
        format!("{context}: {error}"),
        serde_json::json!({"path": path.display().to_string(), "kind": error.kind().to_string()}),
    )
}

/// 私有 JSON 读取（owner-only 校验 + O_NOFOLLOW + 身份竞态检查）。
pub fn read_private_json(
    path: &Path,
    require_private: bool,
    windows_reader: Option<&Path>,
) -> ServiceResult<Value> {
    #[cfg(windows)]
    {
        return read_private_json_windows(path, require_private, windows_reader);
    }
    #[cfg(not(windows))]
    {
        let _ = windows_reader;
        read_private_json_unix(path, require_private)
    }
}

#[cfg(not(windows))]
fn read_private_json_unix(path: &Path, require_private: bool) -> ServiceResult<Value> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let before = fs::symlink_metadata(path)
        .map_err(|error| io_error("runtime record stat failed", path, error))?;
    if !before.file_type().is_file() || before.file_type().is_symlink() {
        return Err(record_error(
            "runtime record is a symbolic link or not a regular file",
            serde_json::json!({"path": path.display().to_string()}),
        ));
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| io_error("runtime record open failed", path, error))?;
    let stat = file
        .metadata()
        .map_err(|error| io_error("runtime record fstat failed", path, error))?;
    let after = fs::symlink_metadata(path)
        .map_err(|error| io_error("runtime record re-stat failed", path, error))?;
    if !after.file_type().is_file()
        || before.dev() != stat.dev()
        || before.ino() != stat.ino()
        || after.dev() != stat.dev()
        || after.ino() != stat.ino()
    {
        return Err(record_error(
            "runtime record changed while opening",
            serde_json::json!({"path": path.display().to_string()}),
        ));
    }
    if !stat.is_file() || stat.len() == 0 || stat.len() > 65536 {
        return Err(record_error(
            "runtime record is not a bounded regular file",
            serde_json::json!({"path": path.display().to_string(), "bytes": stat.len()}),
        ));
    }
    if require_private && ((stat.mode() & 0o077) != 0 || stat.uid() != unsafe { libc::getuid() }) {
        return Err(record_error(
            "local token record is not owner only",
            serde_json::json!({
                "path": path.display().to_string(),
                "mode": format!("{:o}", stat.mode() & 0o777),
                "ownerMatches": stat.uid() == unsafe { libc::getuid() },
            }),
        ));
    }
    let mut text = String::new();
    let mut file = file;
    file.read_to_string(&mut text)
        .map_err(|error| io_error("runtime record read failed", path, error))?;
    serde_json::from_str(&text).map_err(|error| {
        record_error(
            format!("runtime record is not valid JSON: {error}"),
            serde_json::json!({"path": path.display().to_string()}),
        )
    })
}

#[cfg(windows)]
fn read_private_json_windows(
    path: &Path,
    _require_private: bool,
    windows_reader: Option<&Path>,
) -> ServiceResult<Value> {
    // Windows 私读交给服务自身以同一句柄核权限：`lingxi-service --read-private-runtime-json`。
    let before = fs::symlink_metadata(path)
        .map_err(|error| io_error("runtime record stat failed", path, error))?;
    if !before.file_type().is_file() || before.file_type().is_symlink() {
        return Err(record_error(
            "runtime record is a symbolic link or not a regular file",
            serde_json::json!({"path": path.display().to_string()}),
        ));
    }
    let runtime = path
        .parent()
        .ok_or_else(|| record_error("runtime record has no parent directory", Value::Null))?;
    if runtime
        .file_name()
        .map(|name| name != RUNTIME_DIR_NAME)
        .unwrap_or(true)
    {
        return Err(record_error(
            "runtime record is not inside a lingxi-service directory",
            serde_json::json!({"path": path.display().to_string()}),
        ));
    }
    let name = match path.file_name().and_then(|name| name.to_str()) {
        Some(RECORD_FILE_NAME) => "instance",
        Some(TOKEN_FILE_NAME) => "local-token",
        _ => {
            return Err(record_error(
                "runtime record name is not recognised",
                serde_json::json!({"path": path.display().to_string()}),
            ))
        }
    };
    let reader = windows_reader.ok_or_else(|| {
        record_error(
            "trusted Windows Rust runtime reader is unavailable",
            serde_json::json!({"path": path.display().to_string()}),
        )
    })?;
    let home = runtime
        .parent()
        .ok_or_else(|| record_error("runtime directory has no home", Value::Null))?;
    let output = std::process::Command::new(reader)
        .arg("--read-private-runtime-json")
        .arg(home)
        .arg(name)
        .output()
        .map_err(|error| record_error(format!("trusted reader failed: {error}"), Value::Null))?;
    if !output.status.success() {
        return Err(record_error(
            "Windows Rust runtime record failed private handle verification or read",
            serde_json::json!({"status": output.status.code()}),
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| {
        record_error(
            format!("trusted reader returned invalid JSON: {error}"),
            serde_json::json!({"path": path.display().to_string()}),
        )
    })
}

fn expect_object<'a>(
    value: &'a Value,
    path: &Path,
) -> ServiceResult<&'a serde_json::Map<String, Value>> {
    value.as_object().ok_or_else(|| {
        record_error(
            "runtime record is not a JSON object",
            serde_json::json!({"path": path.display().to_string()}),
        )
    })
}

fn get_u64(map: &serde_json::Map<String, Value>, key: &str, path: &Path) -> ServiceResult<u64> {
    map.get(key).and_then(Value::as_u64).ok_or_else(|| {
        record_error(
            format!("runtime record field `{key}` is missing or not an integer"),
            serde_json::json!({"path": path.display().to_string()}),
        )
    })
}

fn get_str<'a>(
    map: &'a serde_json::Map<String, Value>,
    key: &str,
    path: &Path,
) -> ServiceResult<&'a str> {
    map.get(key).and_then(Value::as_str).ok_or_else(|| {
        record_error(
            format!("runtime record field `{key}` is missing or not a string"),
            serde_json::json!({"path": path.display().to_string()}),
        )
    })
}

pub fn parse_instance_record(value: &Value, path: &Path) -> ServiceResult<InstanceRecord> {
    let map = expect_object(value, path)?;
    let pid = get_u64(map, "pid", path)?;
    if pid == 0 || pid > u32::MAX as u64 {
        return Err(record_error(
            "runtime record pid is outside the valid range",
            serde_json::json!({"pid": pid}),
        ));
    }
    let bind_addr = map
        .get("bindAddr")
        .and_then(Value::as_str)
        .map(str::to_string);
    let transport = map
        .get("transport")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(InstanceRecord {
        schema_version: map
            .get("schemaVersion")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        server_kind: get_str(map, "serverKind", path)?.to_string(),
        wire_protocol_min: get_u64(map, "wireProtocolMin", path)?,
        wire_protocol_max: get_u64(map, "wireProtocolMax", path)?,
        data_epoch: get_u64(map, "dataEpoch", path)?,
        instance_id: get_str(map, "instanceId", path)?.to_string(),
        pid: pid as u32,
        bind_addr,
        transport,
        home_path: get_str(map, "homePath", path)?.to_string(),
    })
}

fn is_hex_32(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// 解析 READY/bindAddr 的本地地址（必须是 loopback + 显式端口）。
pub fn parse_local_address(bind_addr: &str) -> ServiceResult<(String, u16)> {
    let address: SocketAddr = bind_addr.parse().map_err(|_| {
        record_error(
            "runtime record bind address is not a socket address",
            serde_json::json!({"bindAddr": bind_addr}),
        )
    })?;
    if !address.ip().is_loopback() {
        return Err(record_error(
            "runtime record bind address is not loopback",
            serde_json::json!({"bindAddr": bind_addr}),
        ));
    }
    Ok((address.ip().to_string(), address.port()))
}

fn runtime_path(home: &Path, file: &str) -> PathBuf {
    home.join(RUNTIME_DIR_NAME).join(file)
}

/// 读取并验证 owned 实例记录（pid + READY 地址 + token + 协议字段）。
pub fn read_owned(
    home: &Path,
    pid: u32,
    ready_addr: &str,
    windows_reader: Option<&Path>,
) -> ServiceResult<OwnedConnection> {
    let record_path = runtime_path(home, RECORD_FILE_NAME);
    let token_path = runtime_path(home, TOKEN_FILE_NAME);
    let record_value = read_private_json(&record_path, false, windows_reader)?;
    let record = parse_instance_record(&record_value, &record_path)?;

    let canonical_home = fs::canonicalize(home).map_err(|error| {
        record_error(
            format!("home directory cannot be canonicalized: {error}"),
            serde_json::json!({"home": home.display().to_string()}),
        )
    })?;
    if record.server_kind != SERVER_KIND {
        return Err(StartError::mismatch(
            codes::SERVICE_INSTANCE_INVALID,
            "runtime instance record is not a lingxi-service record",
            SERVER_KIND,
            record.server_kind.clone(),
        ));
    }
    if record.pid != pid {
        return Err(StartError::mismatch(
            codes::SERVICE_INSTANCE_INVALID,
            "runtime instance record pid does not match the owned child",
            pid.to_string(),
            record.pid.to_string(),
        ));
    }
    if record.home_path != canonical_home.to_string_lossy() {
        return Err(StartError::mismatch(
            codes::SERVICE_INSTANCE_INVALID,
            "runtime instance record home does not match this data root",
            canonical_home.display().to_string(),
            record.home_path.clone(),
        ));
    }
    if !is_hex_32(&record.instance_id) {
        return Err(record_error(
            "runtime instance id is not 32 lowercase hex characters",
            serde_json::json!({"instanceId": record.instance_id}),
        ));
    }
    let credential_value = read_private_json(&token_path, true, windows_reader)?;
    let credential = expect_object(&credential_value, &token_path)?;
    let kind = get_str(credential, "kind", &token_path)?;
    let credential_instance = get_str(credential, "instanceId", &token_path)?;
    let token = get_str(credential, "token", &token_path)?;
    if kind != "local_token" || credential_instance != record.instance_id || !is_hex_32(token) {
        return Err(record_error(
            "runtime local token record does not match the instance record",
            serde_json::json!({
                "kind": kind,
                "instanceMatches": credential_instance == record.instance_id,
                "tokenShape": is_hex_32(token),
            }),
        ));
    }
    // 运行时协议门（A03 runtime-identity-mismatch 的核心）。
    let host_protocol = ProtocolIdentity::host();
    if record.wire_protocol_min != host_protocol.wire_protocol_min as u64
        || record.wire_protocol_max != host_protocol.wire_protocol_max as u64
        || record.data_epoch != host_protocol.data_epoch as u64
    {
        return Err(StartError::mismatch(
            codes::RUNTIME_IDENTITY_MISMATCH,
            "runtime instance record announces an incompatible protocol/data epoch",
            format!(
                "wire {}..={} epoch {}",
                host_protocol.wire_protocol_min,
                host_protocol.wire_protocol_max,
                host_protocol.data_epoch
            ),
            format!(
                "wire {}..={} epoch {}",
                record.wire_protocol_min, record.wire_protocol_max, record.data_epoch
            ),
        )
        .with_detail(serde_json::json!({"instanceId": record.instance_id})));
    }
    let bind_addr = record.bind_addr.as_deref().ok_or_else(|| {
        record_error(
            "runtime instance record has no bind address",
            serde_json::json!({"instanceId": record.instance_id}),
        )
    })?;
    if bind_addr != ready_addr {
        return Err(StartError::mismatch(
            codes::SERVICE_INSTANCE_INVALID,
            "runtime instance bind address does not match the READY line",
            ready_addr.to_string(),
            bind_addr.to_string(),
        ));
    }
    let (host, port) = parse_local_address(bind_addr)?;
    let transport = match record.transport.as_deref().unwrap_or("http") {
        transport @ ("http" | "https") => transport.to_string(),
        other => {
            return Err(record_error(
                "runtime instance record has an invalid transport",
                serde_json::json!({"transport": other}),
            ))
        }
    };
    Ok(OwnedConnection {
        host: host.clone(),
        port,
        transport: transport.clone(),
        base_url: format!("{transport}://{host}:{port}"),
        token: token.to_string(),
        instance_id: record.instance_id.clone(),
        pid,
        record,
    })
}

fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        if pid == 0 || pid > i32::MAX as u32 {
            return false;
        }
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true // 非 Unix 由服务记录的 Windows 私读分支覆盖；本机无 Windows 真机（BLOCKED）。
    }
}

/// 已存在实例（定位线索）：记录可读、pid 存活、token/身份可读。
/// 记录存在但 pid 已死 → `Ok(None)`（Electron 同语义，不清理记录，交由服务接管）。
pub fn read_existing(
    home: &Path,
    windows_reader: Option<&Path>,
) -> ServiceResult<Option<OwnedConnection>> {
    let record_path = runtime_path(home, RECORD_FILE_NAME);
    let record_value = match read_private_json(&record_path, false, windows_reader) {
        Ok(value) => value,
        Err(error)
            if error.message.contains("No such file") || error.message.contains("os error 2") =>
        {
            return Ok(None)
        }
        Err(error) => return Err(error),
    };
    let record = parse_instance_record(&record_value, &record_path)?;
    let bind_addr = match record.bind_addr.clone() {
        Some(addr) if !addr.is_empty() => addr,
        _ => {
            return Err(record_error(
                "existing runtime instance record has no bind address",
                serde_json::json!({"pid": record.pid}),
            ))
        }
    };
    if !pid_alive(record.pid) {
        return Ok(None);
    }
    read_owned(home, record.pid, &bind_addr, windows_reader).map(Some)
}

/// 读取已存在实例的原始记录（不要求能取得 token；供失败诊断引用 pid）。
pub fn read_record(
    home: &Path,
    windows_reader: Option<&Path>,
) -> ServiceResult<Option<InstanceRecord>> {
    let record_path = runtime_path(home, RECORD_FILE_NAME);
    match read_private_json(&record_path, false, windows_reader) {
        Ok(value) => Ok(Some(parse_instance_record(&value, &record_path)?)),
        Err(error)
            if error.message.contains("No such file") || error.message.contains("os error 2") =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_home(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lingxi-r09-instance-{name}-{}-{}",
            std::process::id(),
            super::super::diagnostics::now_unix_ms()
        ));
        fs::create_dir_all(dir.join(RUNTIME_DIR_NAME)).unwrap();
        dir
    }

    fn write_record(home: &Path, pid: u32, wire_max: u64, epoch: u64, bind: &str) {
        let canonical = fs::canonicalize(home).unwrap();
        let record = serde_json::json!({
            "schemaVersion": 1,
            "serverKind": "lingxi-service",
            "serverVersion": "0.0.0",
            "wireProtocolMin": 1,
            "wireProtocolMax": wire_max,
            "dataEpoch": epoch,
            "instanceId": "0123456789abcdef0123456789abcdef",
            "startNonce": "deadbeef",
            "pid": pid,
            "bindAddr": bind,
            "transport": "http",
            "startedAtUnixMs": 1,
            "homePath": canonical.to_string_lossy(),
        });
        fs::write(
            home.join(RUNTIME_DIR_NAME).join(RECORD_FILE_NAME),
            serde_json::to_vec_pretty(&record).unwrap(),
        )
        .unwrap();
        let token = serde_json::json!({
            "schemaVersion": 1,
            "kind": "local_token",
            "token": "fedcba9876543210fedcba9876543210",
            "instanceId": "0123456789abcdef0123456789abcdef",
            "createdAtUnixMs": 1,
        });
        let path = home.join(RUNTIME_DIR_NAME).join(TOKEN_FILE_NAME);
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(&serde_json::to_vec_pretty(&token).unwrap())
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }

    #[test]
    fn owned_record_matches_host_protocol() {
        let home = temp_home("owned");
        let pid = std::process::id();
        write_record(&home, pid, 1, 2, "127.0.0.1:43111");
        let connection = read_owned(&home, pid, "127.0.0.1:43111", None).unwrap();
        assert_eq!(connection.port, 43111);
        assert_eq!(connection.base_url, "http://127.0.0.1:43111");
        assert_eq!(connection.transport, "http");
        assert_eq!(connection.instance_id, "0123456789abcdef0123456789abcdef");
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn runtime_protocol_mismatch_is_rejected() {
        let home = temp_home("wire");
        let pid = std::process::id();
        write_record(&home, pid, 3, 2, "127.0.0.1:43112");
        let error = read_owned(&home, pid, "127.0.0.1:43112", None).unwrap_err();
        assert_eq!(error.code, codes::RUNTIME_IDENTITY_MISMATCH);
        assert!(error.actual.unwrap().contains("wire 1..=3"));
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn runtime_data_epoch_mismatch_is_rejected() {
        let home = temp_home("epoch");
        let pid = std::process::id();
        write_record(&home, pid, 1, 3, "127.0.0.1:43113");
        let error = read_owned(&home, pid, "127.0.0.1:43113", None).unwrap_err();
        assert_eq!(error.code, codes::RUNTIME_IDENTITY_MISMATCH);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn ready_addr_mismatch_is_rejected() {
        let home = temp_home("addr");
        let pid = std::process::id();
        write_record(&home, pid, 1, 2, "127.0.0.1:43114");
        let error = read_owned(&home, pid, "127.0.0.1:43115", None).unwrap_err();
        assert_eq!(error.code, codes::SERVICE_INSTANCE_INVALID);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn token_file_must_be_owner_only() {
        let home = temp_home("mode");
        let pid = std::process::id();
        write_record(&home, pid, 1, 2, "127.0.0.1:43116");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                home.join(RUNTIME_DIR_NAME).join(TOKEN_FILE_NAME),
                fs::Permissions::from_mode(0o644),
            )
            .unwrap();
        }
        let error = read_owned(&home, pid, "127.0.0.1:43116", None).unwrap_err();
        assert!(error.message.contains("owner only"));
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn symlinked_record_is_rejected() {
        let home = temp_home("symlink");
        let target = home.join("elsewhere.json");
        fs::write(&target, b"{}").unwrap();
        let link = home.join(RUNTIME_DIR_NAME).join(RECORD_FILE_NAME);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let error = read_private_json(&link, false, None).unwrap_err();
        assert_eq!(error.code, codes::SERVICE_INSTANCE_INVALID);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn stale_pid_means_no_existing_instance() {
        let home = temp_home("stale");
        // 不存在的 pid：read_existing 返回 None，不报错、不清理记录。
        write_record(&home, 999_999, 1, 2, "127.0.0.1:43117");
        let existing = read_existing(&home, None).unwrap();
        assert!(existing.is_none());
        assert!(home.join(RUNTIME_DIR_NAME).join(RECORD_FILE_NAME).is_file());
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn missing_record_is_none() {
        let home = temp_home("missing");
        assert!(read_existing(&home, None).unwrap().is_none());
        let _ = fs::remove_dir_all(home);
    }
}
