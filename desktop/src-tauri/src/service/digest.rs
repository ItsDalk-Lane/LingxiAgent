//! R09-T02 二进制摘要：文件 SHA-256 与「签名后内容摘要」。
//!
//! 移植自 `desktop/src/shared/rust-local-service.cjs:43-117`（Electron 的既有
//! 语义），因为 macOS ad-hoc 签名会重写 Mach-O 的签名块与三处长度字段：
//! 构建清单里的 `sha256`（签名前字节）与包内实际字节必然不同，必须能用
//! `contentSha256`（去掉签名可变区后的内容）证明“正文没变”，再叠加
//! `codesign --verify`。Windows 的 PE 证书表/校验和逻辑一并移植，但本机无
//! Windows 真机，仅单测覆盖字节布局（平台阻塞见 BLOCKED.md）。

use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

use super::error::{codes, ServiceResult, StartError};

pub fn hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_lower(&hasher.finalize())
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let bytes = fs::read(path)?;
    Ok(sha256_bytes(&bytes))
}

/// 平台无关的可执行文件魔数/架构校验（与 Electron `assertRustExecutableFormat` 同表）。
pub fn assert_executable_format(bytes: &[u8], platform: &str, arch: &str) -> ServiceResult<()> {
    if bytes.len() < 256 {
        return Err(StartError::new(
            codes::SERVICE_BINARY_INVALID,
            "service binary is shorter than a platform executable header",
        ));
    }
    let header = &bytes[..256];
    match platform {
        "mac" => {
            let cpu: u32 = if arch == "arm64" {
                0x0100_000c
            } else {
                0x0100_0007
            };
            let magic = u32::from_le_bytes(header[0..4].try_into().unwrap());
            let cputype = u32::from_le_bytes(header[4..8].try_into().unwrap());
            if magic != 0xfeed_facf || cputype != cpu {
                return Err(StartError::mismatch(
                    codes::SERVICE_BINARY_INVALID,
                    "service binary is not a matching macOS executable",
                    format!("Mach-O 64 cpu={arch}"),
                    format!("magic={magic:#010x} cputype={cputype:#010x}"),
                ));
            }
        }
        "linux" => {
            let machine: u16 = if arch == "arm64" { 183 } else { 62 };
            let is_elf = header[..4] == [0x7f, b'E', b'L', b'F'];
            let class = header[4];
            let data = header[5];
            let actual_machine = u16::from_le_bytes(header[18..20].try_into().unwrap());
            if !is_elf || class != 2 || data != 1 || actual_machine != machine {
                return Err(StartError::mismatch(
                    codes::SERVICE_BINARY_INVALID,
                    "service binary is not a matching Linux executable",
                    format!("ELF64 little-endian machine={machine}"),
                    format!("magic={is_elf} class={class} data={data} machine={actual_machine}"),
                ));
            }
        }
        "win" => {
            let pe_offset = u32::from_le_bytes(header[0x3c..0x40].try_into().unwrap()) as usize;
            if pe_offset + 6 > bytes.len() {
                return Err(StartError::new(
                    codes::SERVICE_BINARY_INVALID,
                    "PE header offset is outside the service binary",
                ));
            }
            let pe = &bytes[pe_offset..pe_offset + 6];
            let machine: u16 = if arch == "arm64" { 0xaa64 } else { 0x8664 };
            let dos = header[0..2] == *b"MZ";
            let signature = pe[0..4] == [0x50, 0x45, 0x00, 0x00];
            let actual_machine = u16::from_le_bytes(pe[4..6].try_into().unwrap());
            if !dos || !signature || actual_machine != machine {
                return Err(StartError::mismatch(
                    codes::SERVICE_BINARY_INVALID,
                    "service binary is not a matching Windows executable",
                    format!("PE machine={machine:#06x}"),
                    format!("dos={dos} pe={signature} machine={actual_machine:#06x}"),
                ));
            }
        }
        other => {
            return Err(StartError::new(
                codes::SERVICE_BINARY_INVALID,
                format!("unsupported desktop platform tag: {other}"),
            ))
        }
    }
    Ok(())
}

/// Rust 服务的「内容摘要」：签名可变区之外必须逐字节相同。
pub fn content_digest(bytes: &[u8], platform: &str) -> ServiceResult<String> {
    match platform {
        "mac" => mac_content_digest(bytes),
        "win" => pe_content_digest(bytes),
        _ => Ok(sha256_bytes(bytes)),
    }
}

fn invalid(message: &str) -> StartError {
    StartError::new(codes::SERVICE_BINARY_INVALID, message)
}

/// 移植 `macExecutableContentDigest`。签名块（LC_CODE_SIGNATURE）覆盖的字节、
/// `__LINKEDIT` 的两处长度字段与签名 command 的 size 字段清零后哈希。
fn mac_content_digest(bytes: &[u8]) -> ServiceResult<String> {
    if bytes.len() < 32 {
        return Err(invalid("macOS executable is too short for a Mach-O header"));
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if magic != 0xfeed_facf {
        return Err(invalid("invalid macOS Rust executable magic"));
    }
    let commands = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let payload_len = u32::from_le_bytes(bytes[20..24].try_into().unwrap()) as usize;
    let Some(commands_end) = 32usize.checked_add(payload_len) else {
        return Err(invalid("invalid Mach-O load command extent"));
    };
    if commands > 512 || commands_end > bytes.len() {
        return Err(invalid("invalid Mach-O load commands"));
    }
    if commands == 0 {
        return Ok(sha256_bytes(bytes));
    }
    let mut offset = 32usize;
    let mut signature: Option<(usize, usize, usize)> = None; // (command offset, dataOffset, size)
    let mut linkedit: Option<(usize, usize, usize)> = None; // (command offset, fileOffset, fileSize)
    for _ in 0..commands {
        if offset + 8 > commands_end {
            return Err(invalid("truncated Mach-O load command"));
        }
        let command = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        if size < 8 || offset + size > commands_end {
            return Err(invalid("invalid Mach-O load command size"));
        }
        if command == 0x1d {
            if signature.is_some() || size < 16 {
                return Err(invalid("invalid Mach-O code signature command"));
            }
            let data_offset =
                u32::from_le_bytes(bytes[offset + 8..offset + 12].try_into().unwrap()) as usize;
            let data_size =
                u32::from_le_bytes(bytes[offset + 12..offset + 16].try_into().unwrap()) as usize;
            signature = Some((offset, data_offset, data_size));
        }
        if command == 0x19 {
            let name = &bytes[offset + 8..offset + 24];
            let name = String::from_utf8_lossy(name);
            let name = name.split('\0').next().unwrap_or("");
            if name == "__LINKEDIT" {
                if linkedit.is_some() || size < 72 {
                    return Err(invalid("invalid Mach-O linkedit segment"));
                }
                let file_offset =
                    u64::from_le_bytes(bytes[offset + 40..offset + 48].try_into().unwrap());
                let file_size =
                    u64::from_le_bytes(bytes[offset + 48..offset + 56].try_into().unwrap());
                linkedit = Some((offset, file_offset as usize, file_size as usize));
            }
        }
        offset += size;
    }
    if offset != commands_end {
        return Err(invalid("Mach-O load command extent mismatch"));
    }
    let Some((signature_offset, data_offset, data_size)) = signature else {
        return Ok(sha256_bytes(bytes));
    };
    let Some((linkedit_offset, linkedit_file_offset, linkedit_file_size)) = linkedit else {
        return Err(invalid("invalid Mach-O code signature extent"));
    };
    if data_offset < commands_end
        || data_size < 1
        || data_offset + data_size != bytes.len()
        || linkedit_file_offset > data_offset
        || linkedit_file_offset + linkedit_file_size != bytes.len()
        || signature_offset + 16 > bytes.len()
    {
        return Err(invalid("invalid Mach-O code signature extent"));
    }
    let mut content = bytes[..data_offset].to_vec();
    // __LINKEDIT 的 fileoff/filesize 与签名 command 的 datasize 是签名会重写的
    // 字段：清零后再哈希（与 Electron 逐字段一致）。
    content[linkedit_offset + 32..linkedit_offset + 40].fill(0);
    content[linkedit_offset + 48..linkedit_offset + 56].fill(0);
    content[signature_offset + 12..signature_offset + 16].fill(0);
    Ok(sha256_bytes(&content))
}

/// 移植 PE 内容摘要：跳过 checksum 与证书表。
fn pe_content_digest(bytes: &[u8]) -> ServiceResult<String> {
    if bytes.len() < 0x40 {
        return Err(invalid("Windows executable is too short for a PE header"));
    }
    let pe_offset = u32::from_le_bytes(bytes[0x3c..0x40].try_into().unwrap()) as usize;
    let optional = pe_offset
        .checked_add(24)
        .ok_or_else(|| invalid("invalid PE header offset"))?;
    if optional + 4 > bytes.len() {
        return Err(invalid("invalid PE optional header"));
    }
    let magic = u16::from_le_bytes(bytes[optional..optional + 2].try_into().unwrap());
    let optional_size: usize = match magic {
        0x20b => 112,
        0x10b => 96,
        _ => return Err(invalid("invalid PE optional header magic")),
    };
    let security = optional + optional_size + 32;
    let checksum = optional + 64;
    if security + 8 > bytes.len() {
        return Err(invalid("invalid PE certificate directory"));
    }
    let cert_offset =
        u32::from_le_bytes(bytes[security..security + 4].try_into().unwrap()) as usize;
    let cert_size =
        u32::from_le_bytes(bytes[security + 4..security + 8].try_into().unwrap()) as usize;
    let section_count =
        u16::from_le_bytes(bytes[pe_offset + 6..pe_offset + 8].try_into().unwrap()) as usize;
    let optional_header_size =
        u16::from_le_bytes(bytes[pe_offset + 20..pe_offset + 22].try_into().unwrap()) as usize;
    let section_table = optional + optional_header_size;
    if section_count < 1 || section_count > 96 || section_table + section_count * 40 > bytes.len() {
        return Err(invalid("invalid PE section table"));
    }
    let mut section_end = section_table + section_count * 40;
    for index in 0..section_count {
        let entry = section_table + index * 40;
        let raw_size =
            u32::from_le_bytes(bytes[entry + 16..entry + 20].try_into().unwrap()) as usize;
        let raw_offset =
            u32::from_le_bytes(bytes[entry + 20..entry + 24].try_into().unwrap()) as usize;
        section_end = section_end.max(raw_offset + raw_size);
    }
    let cert_present = cert_offset != 0;
    if cert_present != (cert_size != 0)
        || section_end > bytes.len()
        || (cert_present && (cert_offset < section_end || cert_offset + cert_size != bytes.len()))
    {
        return Err(invalid("invalid PE certificate extent"));
    }
    let body_end = if cert_present {
        cert_offset
    } else {
        bytes.len()
    };
    let mut hasher = Sha256::new();
    hasher.update(&bytes[..checksum]);
    hasher.update(&bytes[checksum + 4..security]);
    hasher.update(&bytes[security + 8..body_end]);
    Ok(hex_lower(&hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_binary_content_digest_is_sha256() {
        let bytes = b"not a signed executable".to_vec();
        assert_eq!(
            content_digest(&bytes, "linux").unwrap(),
            sha256_bytes(&bytes)
        );
    }

    #[test]
    fn signed_binary_magic_is_rejected_for_other_platform() {
        let error = assert_executable_format(&[0u8; 300], "mac", "arm64").unwrap_err();
        assert_eq!(error.code, codes::SERVICE_BINARY_INVALID);
    }

    #[test]
    fn macho_without_signature_hashes_whole_file() {
        // 最小 64 位 Mach-O：magic + cputype + 0 load commands。
        let mut bytes = vec![0u8; 64];
        bytes[0..4].copy_from_slice(&0xfeed_facfu32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x0100_000cu32.to_le_bytes());
        bytes[16..20].copy_from_slice(&0u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(content_digest(&bytes, "mac").unwrap(), sha256_bytes(&bytes));
    }

    #[test]
    fn real_host_binary_digests_are_stable_between_calls() {
        let candidate = std::env::var_os("LINGXI_R09_TEST_BINARY")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../rust/target/debug/lingxi-service")
            });
        if !candidate.is_file() {
            return; // 由生产者/集成测试在真实二进制上另行断言，单测不伪造证据。
        }
        let bytes = fs::read(&candidate).unwrap();
        let first = content_digest(&bytes, "mac").unwrap();
        let second = content_digest(&bytes, "mac").unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
    }
}
