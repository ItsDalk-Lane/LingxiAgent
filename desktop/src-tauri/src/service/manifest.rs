//! R09-T02 构建身份与协议门（A03 核心）。
//!
//! 打包版服务二进制由 Tauri `externalBin` 分发，清单由 `resources/` 随包：
//! ```text
//!   Contents/MacOS/lingxi-service      （externalBin，bundler 签名后 sha 必变）
//!   Contents/Resources/rust-service/build.json
//! ```
//! 门禁顺序（任何一步失败都在 spawn 之前拒绝，且不回退任何旧服务）：
//!   1. 清单自身字段（schema/platform/arch/target/toolchain/appVersion/binary/hash 形状）；
//!   2. **协议/数据版本**：清单 `protocol` 必须与本机 `lingxi-protocol` 编译期常量
//!      逐项相等（wire 1..=1、data_epoch 2）——唯一来源是 Rust 常量，
//!      不是 `shared/contract-versions.json`（旧 Node 轴 epoch=1 会误拒 Rust 服务）；
//!   3. 二进制格式与摘要：签名前 sha 相等，或签名后内容摘要相等 + `codesign` 验签。
//!
//! 错误码经 `error::codes`，`expected`/`actual` 直接进失败记录（A03 断言）。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::{json, Value};

use super::digest;
use super::error::{codes, ServiceResult, StartError};

/// 服务自身公告的节点类型（`lingxi_service::SERVER_KIND` 的线值）。
/// 宿主刻意不依赖 lingxi-service crate（ADR-001：service 是独立进程，宿主不链接
/// 业务内核）；该字符串是协议常量的一部分，出现在 health/identity/清单三处。
pub const SERVER_KIND: &str = "lingxi-service";

/// 宿主编译期的协议/数据版本权威（唯一来源：lingxi-protocol）。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolIdentity {
    pub server_kind: String,
    pub wire_protocol_min: u32,
    pub wire_protocol_max: u32,
    pub data_epoch: u32,
}

impl ProtocolIdentity {
    pub fn host() -> Self {
        Self {
            server_kind: SERVER_KIND.to_string(),
            wire_protocol_min: lingxi_protocol::handshake::WIRE_PROTOCOL_MIN_SUPPORTED,
            wire_protocol_max: lingxi_protocol::handshake::WIRE_PROTOCOL_MAX_SUPPORTED,
            data_epoch: lingxi_protocol::ContractVersions::R08_CUTOVER.data_epoch,
        }
    }

    /// 机器可读描述，写进 expected/actual。
    pub fn describe(&self) -> String {
        format!(
            "{} wire {}..={} data-epoch {}",
            self.server_kind, self.wire_protocol_min, self.wire_protocol_max, self.data_epoch
        )
    }

    /// 与宿主常量的逐项相等门（A03 的 manifest-protocol-mismatch）。
    pub fn check_against_host(&self) -> ServiceResult<()> {
        let host = Self::host();
        if *self != host {
            return Err(StartError::mismatch(
                codes::SERVICE_PROTOCOL_INCOMPATIBLE,
                "bundled Rust service protocol/data epoch is incompatible with this host",
                host.describe(),
                self.describe(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildManifest {
    pub schema_version: u32,
    pub platform: String,
    pub arch: String,
    pub target: String,
    pub toolchain: String,
    pub app_version: String,
    pub binary: String,
    pub sha256: String,
    pub content_sha256: String,
    #[serde(default)]
    pub source_sha256: Option<String>,
    pub protocol: ProtocolIdentity,
    /// Tauri stage 追加的渲染层树摘要（记录用；页面已内嵌，不参与启动门）。
    #[serde(default)]
    pub renderer_sha256: Option<String>,
}

pub fn host_platform_tag() -> &'static str {
    if cfg!(target_os = "macos") {
        "mac"
    } else if cfg!(target_os = "windows") {
        "win"
    } else {
        "linux"
    }
}

pub fn host_arch_tag() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    }
}

pub fn executable_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "lingxi-service.exe"
    } else {
        "lingxi-service"
    }
}

/// 打包布局：主程序同级 sidecar + Resources 下清单。
#[derive(Debug, Clone)]
pub struct PackageLayout {
    pub binary: PathBuf,
    pub build_json: PathBuf,
    pub app_bundle: Option<PathBuf>,
}

impl PackageLayout {
    /// 由主程序 `current_exe()` 推导 Tauri 打包布局。
    pub fn from_current_exe(current_exe: &Path) -> Self {
        let macos_dir = current_exe.parent().unwrap_or_else(|| Path::new("/"));
        let contents = macos_dir.parent();
        let binary = macos_dir.join(executable_name());
        let build_json = contents
            .map(|contents| contents.join("Resources/rust-service/build.json"))
            .unwrap_or_else(|| PathBuf::from("Resources/rust-service/build.json"));
        let app_bundle = contents.and_then(Path::parent).map(Path::to_path_buf);
        Self {
            binary,
            build_json,
            app_bundle,
        }
    }
}

#[derive(Debug, Clone)]
pub struct VerifiedService {
    pub binary: PathBuf,
    pub manifest: BuildManifest,
    pub binary_sha256: String,
    /// true = 包内字节已被签名重写，通过内容摘要 + codesign 验证放行。
    pub signed_mutation: bool,
}

fn manifest_invalid(message: impl Into<String>, detail: Value) -> StartError {
    StartError::new(codes::SERVICE_MANIFEST_INVALID, message).with_detail(detail)
}

fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn is_target_triple(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn is_version(value: &str) -> bool {
    let mut parts = value.splitn(3, '.');
    let major = parts.next().unwrap_or("");
    let minor = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("");
    if major.is_empty()
        || minor.is_empty()
        || rest.is_empty()
        || !major.bytes().all(|b| b.is_ascii_digit())
        || !minor.bytes().all(|b| b.is_ascii_digit())
    {
        return false;
    }
    // patch 可带 -pre/+build 后缀
    let patch_end = rest
        .find(|character: char| character == '-' || character == '+')
        .unwrap_or(rest.len());
    let (patch, suffix) = rest.split_at(patch_end);
    !patch.is_empty()
        && patch.bytes().all(|b| b.is_ascii_digit())
        && (suffix.is_empty()
            || suffix[1..]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'+'))
}

/// 仅校验清单字段与协议门（不读二进制）；单测与启动前快速门共用。
pub fn verify_manifest_fields(manifest: &BuildManifest, app_version: &str) -> ServiceResult<()> {
    let platform = host_platform_tag();
    let arch = host_arch_tag();
    if manifest.schema_version != 1 {
        return Err(manifest_invalid(
            "unsupported Rust service build manifest schema version",
            json!({"schemaVersion": manifest.schema_version, "expected": 1}),
        ));
    }
    if manifest.platform != platform || manifest.arch != arch {
        return Err(StartError::mismatch(
            codes::SERVICE_MANIFEST_INVALID,
            "Rust service build manifest does not match this platform and architecture",
            format!("{platform}-{arch}"),
            format!("{}-{}", manifest.platform, manifest.arch),
        ));
    }
    if manifest.binary != executable_name() {
        return Err(StartError::mismatch(
            codes::SERVICE_MANIFEST_INVALID,
            "Rust service build manifest names an unexpected binary",
            executable_name(),
            manifest.binary.clone(),
        ));
    }
    if !is_target_triple(&manifest.target) {
        return Err(manifest_invalid(
            "Rust service build manifest target triple is invalid",
            json!({"target": manifest.target}),
        ));
    }
    if !is_version(&manifest.toolchain) {
        return Err(manifest_invalid(
            "Rust service build manifest toolchain is not a pinned version",
            json!({"toolchain": manifest.toolchain}),
        ));
    }
    if !is_version(&manifest.app_version) {
        return Err(manifest_invalid(
            "Rust service build manifest appVersion is invalid",
            json!({"appVersion": manifest.app_version}),
        ));
    }
    if manifest.app_version != app_version {
        return Err(StartError::mismatch(
            codes::SERVICE_MANIFEST_INVALID,
            "bundled Rust service was built for a different app version",
            app_version.to_string(),
            manifest.app_version.clone(),
        ));
    }
    if !is_lower_hex(&manifest.sha256, 64) || !is_lower_hex(&manifest.content_sha256, 64) {
        return Err(manifest_invalid(
            "Rust service build manifest digests are not 64-digit lowercase hex",
            json!({"sha256": manifest.sha256, "contentSha256": manifest.content_sha256}),
        ));
    }
    // 协议门：唯一来源是本机 lingxi-protocol 编译期常量。
    manifest.protocol.check_against_host()
}

fn read_manifest(build_json: &Path) -> ServiceResult<BuildManifest> {
    let metadata = fs::symlink_metadata(build_json).map_err(|error| {
        manifest_invalid(
            format!("Rust service build manifest is missing: {error}"),
            json!({"path": build_json.display().to_string()}),
        )
    })?;
    if !metadata.file_type().is_file() {
        return Err(manifest_invalid(
            "Rust service build manifest is not a regular file",
            json!({"path": build_json.display().to_string()}),
        ));
    }
    if metadata.len() == 0 || metadata.len() > 16 * 1024 {
        return Err(manifest_invalid(
            "Rust service build manifest size is outside the accepted bounds",
            json!({"bytes": metadata.len()}),
        ));
    }
    let text = fs::read_to_string(build_json).map_err(|error| {
        manifest_invalid(
            format!("Rust service build manifest is unreadable: {error}"),
            json!({"path": build_json.display().to_string()}),
        )
    })?;
    serde_json::from_str(&text).map_err(|error| {
        manifest_invalid(
            format!("Rust service build manifest is not valid JSON: {error}"),
            json!({"path": build_json.display().to_string()}),
        )
    })
}

fn codesign_verify(binary: &Path, app_bundle: Option<&Path>) -> Result<(), String> {
    let binary_output = Command::new("/usr/bin/codesign")
        .args(["--verify", "--strict"])
        .arg(binary)
        .output()
        .map_err(|error| format!("codesign unavailable: {error}"))?;
    if !binary_output.status.success() {
        return Err(format!(
            "codesign --verify --strict failed: {}",
            String::from_utf8_lossy(&binary_output.stderr).trim()
        ));
    }
    let Some(app_bundle) = app_bundle else {
        return Ok(());
    };
    let app_output = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(app_bundle)
        .output()
        .map_err(|error| format!("codesign unavailable: {error}"))?;
    if !app_output.status.success() {
        return Err(format!(
            "codesign --verify --deep --strict failed: {}",
            String::from_utf8_lossy(&app_output.stderr).trim()
        ));
    }
    Ok(())
}

/// 打包版启动门：清单 → 协议 → 二进制格式 → 摘要/签名。
pub fn verify_package(layout: &PackageLayout, app_version: &str) -> ServiceResult<VerifiedService> {
    let manifest = read_manifest(&layout.build_json)?;
    verify_manifest_fields(&manifest, app_version)?;

    let metadata = fs::symlink_metadata(&layout.binary).map_err(|error| {
        StartError::mismatch(
            codes::SERVICE_BINARY_UNAVAILABLE,
            format!("bundled Rust service binary is missing: {error}"),
            layout.binary.display().to_string(),
            "missing".to_string(),
        )
    })?;
    if !metadata.file_type().is_file() || metadata.len() < 1024 {
        return Err(StartError::new(
            codes::SERVICE_BINARY_INVALID,
            "bundled Rust service is not a regular executable",
        )
        .with_detail(
            json!({"path": layout.binary.display().to_string(), "bytes": metadata.len()}),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(StartError::new(
                codes::SERVICE_BINARY_INVALID,
                "bundled Rust service is not executable",
            )
            .with_detail(json!({"path": layout.binary.display().to_string()})));
        }
    }
    let bytes = fs::read(&layout.binary).map_err(|error| {
        StartError::new(
            codes::SERVICE_BINARY_INVALID,
            format!("bundled Rust service is unreadable: {error}"),
        )
    })?;
    digest::assert_executable_format(&bytes, &manifest.platform, &manifest.arch)?;
    let actual_sha256 = digest::sha256_bytes(&bytes);
    let mut signed_mutation = false;
    if actual_sha256 != manifest.sha256 {
        // macOS 打包会重新 ad-hoc 签名 sidecar：sha 必变，但内容摘要必须相同，
        // 并叠加 codesign 验签（设计 Q-02 / 风险 R-02）。
        let actual_content = digest::content_digest(&bytes, &manifest.platform)?;
        let verified = actual_content == manifest.content_sha256
            && manifest.platform == "mac"
            && codesign_verify(&layout.binary, layout.app_bundle.as_deref()).is_ok();
        if !verified {
            let detail = json!({
                "binarySha256": actual_sha256,
                "manifestSha256": manifest.sha256,
                "contentSha256": actual_content,
                "manifestContentSha256": manifest.content_sha256,
                "codesign": if manifest.platform == "mac" {
                    codesign_verify(&layout.binary, layout.app_bundle.as_deref()).err()
                } else {
                    Some("content digest gate only applies to macOS signed mutation".to_string())
                },
            });
            return Err(StartError::mismatch(
                codes::SERVICE_DIGEST_MISMATCH,
                "bundled Rust service bytes do not match the build manifest",
                manifest.sha256.clone(),
                actual_sha256,
            )
            .with_detail(detail));
        }
        signed_mutation = true;
    } else {
        // 未签名/签名未改变字节：内容摘要也必须与清单一致（清单自洽性）。
        let actual_content = digest::content_digest(&bytes, &manifest.platform)?;
        if actual_content != manifest.content_sha256 {
            return Err(StartError::mismatch(
                codes::SERVICE_DIGEST_MISMATCH,
                "bundled Rust service content digest does not match the build manifest",
                manifest.content_sha256.clone(),
                actual_content,
            ));
        }
    }
    Ok(VerifiedService {
        binary: layout.binary.clone(),
        manifest,
        binary_sha256: actual_sha256,
        signed_mutation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lingxi-r09-manifest-{name}-{}-{}",
            std::process::id(),
            super::super::diagnostics::now_unix_ms()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn macho_placeholder() -> Vec<u8> {
        let mut bytes = vec![0u8; 4096];
        bytes[0..4].copy_from_slice(&0xfeed_facfu32.to_le_bytes());
        let cpu = if host_arch_tag() == "arm64" {
            0x0100_000cu32
        } else {
            0x0100_0007
        };
        bytes[4..8].copy_from_slice(&cpu.to_le_bytes());
        bytes[16..20].copy_from_slice(&0u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&0u32.to_le_bytes());
        bytes
    }

    fn manifest_value(bytes: &[u8]) -> Value {
        let protocol = ProtocolIdentity::host();
        json!({
            "schemaVersion": 1,
            "platform": host_platform_tag(),
            "arch": host_arch_tag(),
            "target": if cfg!(target_os = "macos") { "aarch64-apple-darwin" } else { "x86_64-unknown-linux-gnu" },
            "toolchain": "1.98.1",
            "appVersion": "0.1.43",
            "binary": executable_name(),
            "sha256": digest::sha256_bytes(bytes),
            "contentSha256": digest::content_digest(bytes, host_platform_tag()).unwrap(),
            "sourceSha256": "0".repeat(64),
            "protocol": {
                "serverKind": protocol.server_kind,
                "wireProtocolMin": protocol.wire_protocol_min,
                "wireProtocolMax": protocol.wire_protocol_max,
                "dataEpoch": protocol.data_epoch,
            }
        })
    }

    fn write_layout(dir: &Path, manifest: &Value) -> PackageLayout {
        let macos = dir.join("Contents/MacOS");
        let resources = dir.join("Contents/Resources/rust-service");
        fs::create_dir_all(&macos).unwrap();
        fs::create_dir_all(&resources).unwrap();
        let binary = macos.join(executable_name());
        fs::write(&binary, macho_placeholder()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let build_json = resources.join("build.json");
        fs::write(&build_json, serde_json::to_vec_pretty(manifest).unwrap()).unwrap();
        PackageLayout {
            binary,
            build_json,
            app_bundle: Some(dir.join("Test.app")),
        }
    }

    #[test]
    fn verified_manifest_passes_protocol_and_digest_gates() {
        let dir = temp_dir("ok");
        let bytes = macho_placeholder();
        let layout = write_layout(&dir, &manifest_value(&bytes));
        let verified = verify_package(&layout, "0.1.43").unwrap();
        assert!(!verified.signed_mutation);
        assert_eq!(verified.manifest.protocol, ProtocolIdentity::host());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn protocol_mismatch_is_rejected_before_digest() {
        let dir = temp_dir("protocol");
        let bytes = macho_placeholder();
        let mut manifest = manifest_value(&bytes);
        manifest["protocol"]["wireProtocolMax"] = json!(2);
        let layout = write_layout(&dir, &manifest);
        let error = verify_package(&layout, "0.1.43").unwrap_err();
        assert_eq!(error.code, codes::SERVICE_PROTOCOL_INCOMPATIBLE);
        assert!(error.expected.unwrap().contains("1..=1"));
        assert!(error.actual.unwrap().contains("1..=2"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn data_epoch_mismatch_is_rejected() {
        let dir = temp_dir("epoch");
        let bytes = macho_placeholder();
        let mut manifest = manifest_value(&bytes);
        manifest["protocol"]["dataEpoch"] = json!(1);
        let layout = write_layout(&dir, &manifest);
        let error = verify_package(&layout, "0.1.43").unwrap_err();
        assert_eq!(error.code, codes::SERVICE_PROTOCOL_INCOMPATIBLE);
        assert!(error.actual.unwrap().contains("data-epoch 1"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn digest_mismatch_is_rejected() {
        let dir = temp_dir("digest");
        let bytes = macho_placeholder();
        let mut manifest = manifest_value(&bytes);
        manifest["sha256"] = json!("a".repeat(64));
        manifest["contentSha256"] = json!("b".repeat(64));
        let layout = write_layout(&dir, &manifest);
        let error = verify_package(&layout, "0.1.43").unwrap_err();
        assert_eq!(error.code, codes::SERVICE_DIGEST_MISMATCH);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_app_version_is_rejected() {
        let dir = temp_dir("appversion");
        let bytes = macho_placeholder();
        let layout = write_layout(&dir, &manifest_value(&bytes));
        let error = verify_package(&layout, "0.1.44").unwrap_err();
        assert_eq!(error.code, codes::SERVICE_MANIFEST_INVALID);
        assert!(error.message.contains("app version"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_manifest_is_rejected() {
        let dir = temp_dir("missing");
        let layout = PackageLayout {
            binary: dir.join("lingxi-service"),
            build_json: dir.join("build.json"),
            app_bundle: None,
        };
        let error = verify_package(&layout, "0.1.43").unwrap_err();
        assert_eq!(error.code, codes::SERVICE_MANIFEST_INVALID);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn manifest_rejects_trailing_garbage() {
        let dir = temp_dir("garbage");
        let bytes = macho_placeholder();
        let layout = write_layout(&dir, &manifest_value(&bytes));
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&layout.build_json)
            .unwrap();
        file.write_all(b"garbage").unwrap();
        let error = verify_package(&layout, "0.1.43").unwrap_err();
        assert_eq!(error.code, codes::SERVICE_MANIFEST_INVALID);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn host_protocol_comes_from_lingxi_protocol_constants() {
        let host = ProtocolIdentity::host();
        assert_eq!(host.wire_protocol_min, 1);
        assert_eq!(host.wire_protocol_max, 1);
        assert_eq!(host.data_epoch, 2);
        assert_eq!(host.server_kind, "lingxi-service");
    }
}
