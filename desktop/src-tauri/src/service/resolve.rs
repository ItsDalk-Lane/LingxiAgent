//! R09-T02 解析层：数据根、开发/打包二进制、启动前互斥门（C02/C04/C05/C07/C08）。
//!
//! 所有解析都是一次性的：`LINGXI_HOME` 在启动早期确定并 canonicalize，spawn、私有
//! 记录读取、诊断写入共用同一个值（不在各处重复解析）。打包版拒绝 env 替换服务
//! 二进制（与 Electron `resolveRustBinary` 一致：安装包固定使用随包程序）。

use std::fs;
use std::path::{Path, PathBuf};

use super::env::EnvSnapshot;
use super::error::{codes, ServiceResult, StartError};
use super::manifest::{self, VerifiedService};

pub const HOME_ENV_VAR: &str = "LINGXI_HOME";
pub const SERVICE_BIN_ENV_VAR: &str = "LINGXI_SERVICE_BIN";

/// 平台名与 Electron `process.platform` 同词表（renderer 的 win32 分支依赖）。
pub fn platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "win32"
    } else {
        "linux"
    }
}

fn home_error(message: impl Into<String>, detail: serde_json::Value) -> StartError {
    StartError::new(codes::HOME_INVALID, message).with_detail(detail)
}

fn absolute(current_dir: &Path, raw: &Path) -> PathBuf {
    if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        current_dir.join(raw)
    }
}

/// `LINGXI_HOME` 优先，缺省 `$HOME/.lingxi`；必须是可绝对化、可 canonicalize 的目录。
pub fn home_from_env(env: &EnvSnapshot) -> ServiceResult<PathBuf> {
    let current_dir = std::env::current_dir().map_err(|error| {
        home_error(
            format!("current directory is unavailable: {error}"),
            serde_json::Value::Null,
        )
    })?;
    let raw = match env.get_non_empty(HOME_ENV_VAR) {
        Some(value) => {
            let path = PathBuf::from(value);
            if !path.is_absolute() {
                return Err(home_error(
                    "LINGXI_HOME must be an absolute path",
                    serde_json::json!({"LINGXI_HOME": value}),
                ));
            }
            path
        }
        None => {
            let user_home = env
                .get_non_empty("HOME")
                .map(PathBuf::from)
                .ok_or_else(|| {
                    home_error(
                        "HOME is required when LINGXI_HOME is not set",
                        serde_json::Value::Null,
                    )
                })?;
            user_home.join(".lingxi")
        }
    };
    let raw = absolute(&current_dir, &raw);
    fs::create_dir_all(&raw).map_err(|error| {
        home_error(
            format!("data root cannot be created: {error}"),
            serde_json::json!({"home": raw.display().to_string()}),
        )
    })?;
    let canonical = fs::canonicalize(&raw).map_err(|error| {
        home_error(
            format!("data root cannot be canonicalized: {error}"),
            serde_json::json!({"home": raw.display().to_string()}),
        )
    })?;
    Ok(canonical)
}

/// C08：数据降级必须由显式恢复流程处理，桌面宿主拒绝静默降级。
pub fn refuse_data_downgrade(env: &EnvSnapshot) -> Option<StartError> {
    if env.get("LINGXI_ALLOW_DATA_DOWNGRADE") == Some("1") {
        Some(StartError::new(
            codes::DATA_DOWNGRADE_REFUSED,
            "Rust desktop refuses LINGXI_ALLOW_DATA_DOWNGRADE; use an explicit recovery workflow",
        ))
    } else {
        None
    }
}

/// C07：同宅旧 Node server 的线索存在时，拒绝再起 Rust 内核，也不删除他人线索。
pub fn node_server_info_present(home: &Path) -> bool {
    home.join("server-info.json").is_file()
}

/// C04 dev：`LINGXI_SERVICE_BIN`（必须绝对路径）或仓库默认 debug 二进制。
pub fn resolve_dev_binary(env: &EnvSnapshot, default_binary: &Path) -> ServiceResult<PathBuf> {
    let binary = match env.get_non_empty(SERVICE_BIN_ENV_VAR) {
        Some(value) => {
            let path = PathBuf::from(value);
            if !path.is_absolute() {
                return Err(StartError::new(
                    codes::SERVICE_BINARY_UNAVAILABLE,
                    "Rust desktop runtime needs an absolute LINGXI_SERVICE_BIN",
                )
                .with_detail(serde_json::json!({"LINGXI_SERVICE_BIN": value})));
            }
            path
        }
        None => default_binary.to_path_buf(),
    };
    let metadata = fs::symlink_metadata(&binary).map_err(|error| {
        StartError::new(
            codes::SERVICE_BINARY_UNAVAILABLE,
            format!("Rust service binary is unavailable: {error}"),
        )
        .with_detail(serde_json::json!({"binary": binary.display().to_string()}))
    })?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(StartError::new(
            codes::SERVICE_BINARY_UNAVAILABLE,
            "Rust service binary is not a regular file",
        )
        .with_detail(serde_json::json!({"binary": binary.display().to_string()})));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(StartError::new(
                codes::SERVICE_BINARY_UNAVAILABLE,
                "Rust service binary is not executable",
            )
            .with_detail(serde_json::json!({"binary": binary.display().to_string()})));
        }
    }
    Ok(binary)
}

/// C05 packaged：只接受随包 sidecar + Resources 清单，环境变量不能替换。
pub fn resolve_packaged_service(
    current_exe: &Path,
    app_version: &str,
) -> ServiceResult<VerifiedService> {
    let layout = manifest::PackageLayout::from_current_exe(current_exe);
    manifest::verify_package(&layout, app_version)
}

/// 启动前把 home 传给子进程（spawn env 与私有记录读取共用同一 canonical 值）。
pub fn spawn_home_env(home: &Path) -> (String, String) {
    (
        HOME_ENV_VAR.to_string(),
        home.to_string_lossy().into_owned(),
    )
}

/// 服务协议门的期望描述（失败记录用）。
pub fn host_protocol_description() -> String {
    manifest::ProtocolIdentity::host().describe()
}

pub fn server_kind() -> &'static str {
    super::manifest::SERVER_KIND
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lingxi-r09-resolve-{name}-{}-{}",
            std::process::id(),
            super::super::diagnostics::now_unix_ms()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn home_env_is_canonicalized_and_created() {
        let root = temp_dir("home");
        let requested = root.join("nested/home");
        let env =
            EnvSnapshot::from_pairs([("LINGXI_HOME", requested.to_string_lossy().into_owned())]);
        let home = home_from_env(&env).unwrap();
        assert!(home.is_absolute());
        assert_eq!(home, fs::canonicalize(&requested).unwrap());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn relative_home_is_rejected() {
        let env = EnvSnapshot::from_pairs([("LINGXI_HOME", "relative-home")]);
        let error = home_from_env(&env).unwrap_err();
        assert_eq!(error.code, codes::HOME_INVALID);
    }

    #[test]
    fn downgrade_env_is_refused() {
        let env = EnvSnapshot::from_pairs([("LINGXI_ALLOW_DATA_DOWNGRADE", "1")]);
        assert!(refuse_data_downgrade(&env).is_some());
        let env = EnvSnapshot::from_pairs([("LINGXI_ALLOW_DATA_DOWNGRADE", "0")]);
        assert!(refuse_data_downgrade(&env).is_none());
    }

    #[test]
    fn node_server_info_blocks_rust_start() {
        let home = temp_dir("server-info");
        assert!(!node_server_info_present(&home));
        fs::write(home.join("server-info.json"), b"{}").unwrap();
        assert!(node_server_info_present(&home));
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn dev_binary_requires_absolute_env_and_regular_file() {
        let home = temp_dir("binary");
        let binary = home.join("lingxi-service");
        fs::write(&binary, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let env = EnvSnapshot::from_pairs([(SERVICE_BIN_ENV_VAR, "relative/lingxi-service")]);
        let error = resolve_dev_binary(&env, &binary).unwrap_err();
        assert_eq!(error.code, codes::SERVICE_BINARY_UNAVAILABLE);
        let env =
            EnvSnapshot::from_pairs([(SERVICE_BIN_ENV_VAR, binary.to_string_lossy().into_owned())]);
        assert_eq!(resolve_dev_binary(&env, &binary).unwrap(), binary);
        let env = EnvSnapshot::from_pairs([] as [(&str, &str); 0]);
        assert_eq!(resolve_dev_binary(&env, &binary).unwrap(), binary);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn platform_name_matches_electron_vocabulary() {
        assert!(matches!(platform_name(), "darwin" | "win32" | "linux"));
    }
}
