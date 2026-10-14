//! R06 F 块：技能组合包导出（POST /skills/bundles/:id/export 的 Rust 端口）。
//!
//! 语义源：`lib/skill-bundles/package-service.ts`（exportSkillBundlePackage：
//! skills/<name>/ 目录复制 + bundle.json 清单 + 目标目录唯一化文件名
//! `<slug>-skillbundle.zip`，缺技能降级为 warnings、零可导出 400）与
//! `lib/zip-writer.ts`（writeZipFromDirectory：store-mode zip——仅 stored
//! 不压缩条目，local file header + central directory + EOCD，CRC32 表驱动，
//! UTF-8 文件名）。
//!
//! 依赖决策（R01 DEPENDENCY_DECISIONS D-14）：不引入 zip/flate2/tar 外部
//! crate，zip 写入器按 zip-writer.ts 逐字节端口自写；边界 = 仅 stored 条目、
//! 单文件受 u32 尺寸字段上限、整包一次性读入内存（无大文件流式）。
//!
//! lib.rs 不动（叶子约束）：本模块经 skills.rs 的 `#[path]` 声明挂载为
//! `lingxi_service::skills::skill_export`。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use regex::Regex;
use serde_json::{json, Value};

use lingxi_adapters::storage::agent_memory as am;
use lingxi_adapters::storage::skill_library as sl;
use lingxi_kernel::memory::sanitize_skill_name;

use super::SkillService;
use crate::ServiceState;

/// package-service.ts SCHEMA_VERSION。
const MANIFEST_SCHEMA_VERSION: u64 = 1;

// ── error face（package-service.ts SkillBundlePackageError + 路由
//    `err.status || 500` 的原样映射）──────────────────────────────────────

pub struct SkillExportError {
    pub status: u16,
    pub message: String,
}

impl SkillExportError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: 400,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: 404,
            message: message.into(),
        }
    }

    /// 打包失败的响亮报错（拷贝/zip 写入/存储读取等基础设施失败 → 500，
    /// 与原实现 raw Error 无 status 时回退 500 一致）。
    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: 500,
            message: message.into(),
        }
    }
}

// ── store-mode zip（lib/zip-writer.ts 逐字节端口）─────────────────────────

const LOCAL_FILE_HEADER_SIG: u32 = 0x0403_4b50;
const CENTRAL_DIR_HEADER_SIG: u32 = 0x0201_4b50;
const END_OF_CENTRAL_DIR_SIG: u32 = 0x0605_4b50;
/// zip-writer.ts version 20（写入与读取两侧均为 20）。
const ZIP_VERSION: u16 = 20;
/// 通用位 11：文件名为 UTF-8。
const ZIP_FLAG_UTF8: u16 = 0x0800;
/// method 0 = stored（不压缩）。
const ZIP_METHOD_STORED: u16 = 0;

/// zip-writer.ts CRC_TABLE：多项式 0xedb88320 的 256 项查表（const fn
/// 编译期生成，无运行时初始化，也不引入 crc32fast crate）。
const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xedb88320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

static CRC_TABLE: [u32; 256] = crc_table();

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc = CRC_TABLE[((crc ^ byte as u32) & 0xff) as usize] ^ (crc >> 8);
    }
    crc ^ 0xffff_ffff
}

/// zip-writer.ts dosDateTime：本地时钟，年份下限 1980，秒数按 2 秒粒度
/// 落位（DOS 时间字段语义）。
fn dos_date_time() -> (u16, u16) {
    use chrono::{Datelike, Local, Timelike};
    let now = Local::now();
    let year = now.year().max(1980);
    let dos_time =
        ((now.hour() as u16) << 11) | ((now.minute() as u16) << 5) | (now.second() / 2) as u16;
    let dos_date = (((year - 1980) as u16) << 9) | ((now.month() as u16) << 5) | now.day() as u16;
    (dos_time, dos_date)
}

/// zip-writer.ts normalizeZipPath：反斜杠归一为 "/"、剥前导 "/"、拒绝
/// 空段与 "."/".." 段。
fn normalize_zip_path(value: &str) -> Result<String, SkillExportError> {
    let normalized = value.replace('\\', "/");
    let normalized = normalized.trim_start_matches('/');
    if normalized.is_empty()
        || normalized
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(SkillExportError::internal(format!(
            "invalid zip entry path: {value}"
        )));
    }
    Ok(normalized.to_string())
}

struct ZipFileEntry {
    path: PathBuf,
    entry_name: String,
}

/// zip-writer.ts listFiles：递归收集常规文件（空目录不产生条目——zip 按
/// 文件路径还原目录）；任一 symlink → 拒绝；目录内条目按文件名排序保证
/// 包内容确定（原实现依赖 readdir 顺序，语义等价）。
fn list_zip_files(root: &Path) -> Result<Vec<ZipFileEntry>, SkillExportError> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<ZipFileEntry>) -> Result<(), SkillExportError> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .map_err(|error| SkillExportError::internal(format!("read zip source: {error}")))?
            .collect::<Result<_, _>>()
            .map_err(|error| {
                SkillExportError::internal(format!("read zip source entry: {error}"))
            })?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let full = entry.path();
            let stat = std::fs::symlink_metadata(&full)
                .map_err(|error| SkillExportError::internal(format!("stat zip source: {error}")))?;
            let file_type = stat.file_type();
            if file_type.is_symlink() {
                return Err(SkillExportError::internal(
                    "zip source cannot contain symlinks",
                ));
            }
            if file_type.is_dir() {
                walk(&full, root, out)?;
            } else if file_type.is_file() {
                let relative = full.strip_prefix(root).map_err(|error| {
                    SkillExportError::internal(format!("zip source path: {error}"))
                })?;
                let entry_name = normalize_zip_path(&relative.to_string_lossy())?;
                out.push(ZipFileEntry {
                    path: full,
                    entry_name,
                });
            }
            // 其他文件类型（fifo/socket 等）与 TS entry.isFile() 一致跳过。
        }
        Ok(())
    }

    let mut out = Vec::new();
    walk(root, root, &mut out)?;
    Ok(out)
}

/// zip-writer.ts writeZipFromDirectory 的逐字节端口（store-mode）：
/// 每条目 30 字节 local file header + UTF-8 文件名 + 原始字节，46 字节
/// central directory header，22 字节 EOCD。条目尺寸受 u32 字段上限约束
/// （超限显式报错，不静默截断）。
fn write_zip_from_directory(source_dir: &Path, output_path: &Path) -> Result<(), SkillExportError> {
    let files = list_zip_files(source_dir)?;
    let (dos_time, dos_date) = dos_date_time();
    let mut body: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    let mut offset: u32 = 0;
    for file in &files {
        let data = std::fs::read(&file.path).map_err(|error| {
            SkillExportError::internal(format!("read zip source file: {error}"))
        })?;
        let name = file.entry_name.as_bytes();
        let name_len = u16::try_from(name.len())
            .map_err(|_| SkillExportError::internal("zip entry name is too long"))?;
        let size = u32::try_from(data.len()).map_err(|_| {
            SkillExportError::internal(format!(
                "zip entry exceeds the stored-size limit (u32): {}",
                file.entry_name
            ))
        })?;
        let crc = crc32(&data);
        // local file header（30 字节，偏移与 zip-writer.ts 逐字段一致）
        body.extend_from_slice(&LOCAL_FILE_HEADER_SIG.to_le_bytes());
        body.extend_from_slice(&ZIP_VERSION.to_le_bytes());
        body.extend_from_slice(&ZIP_FLAG_UTF8.to_le_bytes());
        body.extend_from_slice(&ZIP_METHOD_STORED.to_le_bytes());
        body.extend_from_slice(&dos_time.to_le_bytes());
        body.extend_from_slice(&dos_date.to_le_bytes());
        body.extend_from_slice(&crc.to_le_bytes());
        body.extend_from_slice(&size.to_le_bytes());
        body.extend_from_slice(&size.to_le_bytes());
        body.extend_from_slice(&name_len.to_le_bytes());
        body.extend_from_slice(&0u16.to_le_bytes());
        body.extend_from_slice(name);
        body.extend_from_slice(&data);
        // central directory header（46 字节）
        central.extend_from_slice(&CENTRAL_DIR_HEADER_SIG.to_le_bytes());
        central.extend_from_slice(&ZIP_VERSION.to_le_bytes());
        central.extend_from_slice(&ZIP_VERSION.to_le_bytes());
        central.extend_from_slice(&ZIP_FLAG_UTF8.to_le_bytes());
        central.extend_from_slice(&ZIP_METHOD_STORED.to_le_bytes());
        central.extend_from_slice(&dos_time.to_le_bytes());
        central.extend_from_slice(&dos_date.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&name_len.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
        offset = offset
            .checked_add(30 + name_len as u32 + size)
            .ok_or_else(|| SkillExportError::internal("zip offset overflow"))?;
    }
    let central_offset = offset;
    let central_size = u32::try_from(central.len())
        .map_err(|_| SkillExportError::internal("zip central directory is too large"))?;
    let count = u16::try_from(files.len())
        .map_err(|_| SkillExportError::internal("zip entry count exceeds the u16 limit"))?;
    // end of central directory record（22 字节）
    body.extend_from_slice(&central);
    body.extend_from_slice(&END_OF_CENTRAL_DIR_SIG.to_le_bytes());
    body.extend_from_slice(&0u16.to_le_bytes());
    body.extend_from_slice(&0u16.to_le_bytes());
    body.extend_from_slice(&count.to_le_bytes());
    body.extend_from_slice(&count.to_le_bytes());
    body.extend_from_slice(&central_size.to_le_bytes());
    body.extend_from_slice(&central_offset.to_le_bytes());
    body.extend_from_slice(&0u16.to_le_bytes());
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            SkillExportError::internal(format!("create zip target dir: {error}"))
        })?;
    }
    std::fs::write(output_path, &body)
        .map_err(|error| SkillExportError::internal(format!("write zip package: {error}")))?;
    Ok(())
}

// ── 打包辅助（package-service.ts 私有函数端口）────────────────────────────

/// package-service.ts slugify（导出文件名专用；与 store.ts 的 bundle id
/// slugify 不同源）：小写 + 保留 `[\w\u4e00-\u9fff-]`、非法字符连续段折叠为
/// 单个 "-"、去首尾 "-"、截 56 字符、回退 "skill-bundle"。偏差：不做
/// NFKD 规范化（锁树内无 unicode-normalization crate；ASCII 与中日韩名字
/// 的行为与原实现一致，含组合附加符号的名字可能少一个 "e-" 前缀段）。
fn export_slugify(value: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for c in value.chars().flat_map(|c| c.to_lowercase()) {
        let allowed = c.is_ascii_alphanumeric()
            || c == '_'
            || c == '-'
            || ('\u{4e00}'..='\u{9fff}').contains(&c);
        if allowed {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(c);
        } else if !out.is_empty() {
            pending_dash = true;
        }
    }
    let trimmed: String = out.trim_matches('-').chars().take(56).collect();
    if trimmed.is_empty() {
        "skill-bundle".into()
    } else {
        trimmed
    }
}

fn export_file_name_for_bundle(bundle_name: &str) -> String {
    format!("{}-skillbundle.zip", export_slugify(bundle_name))
}

/// package-service.ts resolveUniqueExportPath：目标目录内文件名唯一化
/// （`<name>-2.zip`、`<name>-3.zip`…）。
fn resolve_unique_export_path(target_dir: &Path, file_name: &str) -> PathBuf {
    let mut candidate = target_dir.join(file_name);
    let mut attempt = 2u32;
    while candidate.exists() {
        let stem = Path::new(file_name)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| file_name.to_string());
        let ext = Path::new(file_name)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        candidate = target_dir.join(format!("{stem}-{attempt}{ext}"));
        attempt += 1;
    }
    candidate
}

/// parseSkillName 的三个模式（package-service.ts:63-76）。
static FRONTMATTER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)^---\s*\n(.*?)\n---").expect("frontmatter pattern"));
static NAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^name:\s*(.+)$").expect("name pattern"));
static QUOTE_STRIP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^["']|["']$"#).expect("quote strip pattern"));
static HEADING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^#\s+(.+?)$").expect("heading pattern"));

/// package-service.ts parseSkillName：frontmatter `name:`（剥首尾引号）
/// 优先——命中即返回（可为空串，由调用方按 falsy 回退，与 `||` 一致）；
/// 否则首个 `#` 标题；读取失败回 None。
fn parse_skill_name(skill_md_path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(skill_md_path).ok()?;
    if let Some(frontmatter) = FRONTMATTER_RE
        .captures(&content)
        .and_then(|captures| captures.get(1))
    {
        if let Some(name) = NAME_RE
            .captures(frontmatter.as_str())
            .and_then(|captures| captures.get(1))
        {
            return Some(
                QUOTE_STRIP_RE
                    .replace_all(name.as_str().trim(), "")
                    .into_owned(),
            );
        }
    }
    HEADING_RE
        .captures(&content)
        .and_then(|captures| captures.get(1))
        .map(|m| m.as_str().trim().to_string())
}

/// package-service.ts ensureNoSymlinks：源树内任一 symlink → 400。
fn ensure_no_symlinks(dir: &Path) -> Result<(), SkillExportError> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries: Vec<_> = std::fs::read_dir(&current)
            .map_err(|error| SkillExportError::internal(format!("scan skill package: {error}")))?
            .collect::<Result<_, _>>()
            .map_err(|error| {
                SkillExportError::internal(format!("scan skill package entry: {error}"))
            })?;
        for entry in entries {
            let full = entry.path();
            let stat = std::fs::symlink_metadata(&full).map_err(|error| {
                SkillExportError::internal(format!("stat skill package entry: {error}"))
            })?;
            if stat.file_type().is_symlink() {
                return Err(SkillExportError::bad_request(format!(
                    "skill contains symlink: {}",
                    entry.file_name().to_string_lossy()
                )));
            }
            if stat.is_dir() {
                stack.push(full);
            }
        }
    }
    Ok(())
}

/// safeCopyDir（shared/safe-fs.ts）面向全新目标的端口：目标位于一次性
/// `.ephemeral/skill-bundle-exports/<token>/` 下（原实现的 tmp/bak 换名
/// 舞步只对已存在目标有意义，这里天然不需要）；symlink 二道闸覆盖
/// ensureNoSymlinks 之后的窗口。
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), SkillExportError> {
    std::fs::create_dir_all(dst)
        .map_err(|error| SkillExportError::internal(format!("copy skill package: {error}")))?;
    let mut entries: Vec<_> = std::fs::read_dir(src)
        .map_err(|error| SkillExportError::internal(format!("copy skill package: {error}")))?
        .collect::<Result<_, _>>()
        .map_err(|error| {
            SkillExportError::internal(format!("copy skill package entry: {error}"))
        })?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let full = entry.path();
        let target = dst.join(entry.file_name());
        let stat = std::fs::symlink_metadata(&full)
            .map_err(|error| SkillExportError::internal(format!("copy skill package: {error}")))?;
        let file_type = stat.file_type();
        if file_type.is_symlink() {
            return Err(SkillExportError::bad_request(format!(
                "skill contains symlink: {}",
                entry.file_name().to_string_lossy()
            )));
        }
        if file_type.is_dir() {
            copy_dir_recursive(&full, &target)?;
        } else if file_type.is_file() {
            std::fs::copy(&full, &target).map_err(|error| {
                SkillExportError::internal(format!("copy skill package file: {error}"))
            })?;
        } else {
            // fifo/socket 等特殊文件无法按文件语义复制，显式失败不静默跳过。
            return Err(SkillExportError::internal(format!(
                "cannot copy non-regular file: {}",
                entry.file_name().to_string_lossy()
            )));
        }
    }
    Ok(())
}

/// package-service.ts buildManifest。
fn build_manifest(bundle: &Value, exported_skills: &[Value], file_name: &str) -> Value {
    json!({
        "kind": "SkillBundle",
        "schemaVersion": MANIFEST_SCHEMA_VERSION,
        "package": {
            "name": file_name,
            "exportedAt": am::now_iso(),
        },
        "bundle": {
            "name": bundle["name"],
            "source": bundle["source"].as_str().filter(|s| !s.is_empty()).unwrap_or("user"),
            "sourcePackage": bundle["sourcePackage"].as_str().filter(|s| !s.is_empty()),
        },
        "skills": {
            "bundles": [
                {
                    "name": bundle["name"],
                    "skills": exported_skills,
                },
            ],
        },
    })
}

/// 一次性导出根的 token（package-service.ts:177：pid-时间-随机）。
fn ephemeral_token() -> String {
    let mut bytes = [0u8; 8];
    getrandom::getrandom(&mut bytes).expect("OS RNG unavailable");
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis())
        .unwrap_or_default();
    format!(
        "{}-{}-{}",
        std::process::id(),
        now_ms,
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    )
}

// ── service（package-service.ts exportSkillBundlePackage 端口）─────────────

impl SkillService {
    /// POST /skills/bundles/:id/export 的服务面：加载组合 → 解析可导出技能
    /// → 在一次性 `.ephemeral/skill-bundle-exports/<token>/package` 下组装
    /// `skills/<name>/` + `bundle.json` → store-mode zip 写入目标目录 →
    /// 返回结果元数据。失败显式报错（状态码见 SkillExportError），临时根
    /// 尽力清理。
    pub fn export_skill_bundle(&self, bundle_id: &str) -> Result<Value, SkillExportError> {
        // loadBundle（package-service.ts:133-140）：空 id 400，未知 id 404。
        let id = bundle_id.trim();
        if id.is_empty() {
            return Err(SkillExportError::bad_request("bundle id is required"));
        }
        let bundles = self
            .bundles_store()
            .load_checked()
            .map_err(|error| SkillExportError::internal(format!("load skill bundles: {error}")))?;
        let bundle = bundles
            .iter()
            .find(|item| item["id"].as_str() == Some(id))
            .ok_or_else(|| SkillExportError::not_found("skill bundle not found"))?
            .clone();

        // resolveDefaultExportTargetDir（package-service.ts:50-55）：
        // [engine.cwd, engine.lingxiHome] 的首个绝对路径——端口映射：
        // cwd = 已配置工作区根（desk 语义），回退 lingxi home。
        let target_dir = self
            .workspace_root()
            .filter(|dir| dir.is_absolute())
            .unwrap_or_else(|| self.home().to_path_buf());
        if !target_dir.is_absolute() {
            return Err(SkillExportError::bad_request(
                "export target directory must be absolute",
            ));
        }
        std::fs::create_dir_all(&target_dir).map_err(|error| {
            SkillExportError::internal(format!("prepare export target directory: {error}"))
        })?;

        let token = ephemeral_token();
        let export_root = self
            .home()
            .join(".ephemeral")
            .join("skill-bundle-exports")
            .join(&token);
        let package_root = export_root.join("package");

        let outcome = self.package_skill_bundle(&bundle, &package_root, &target_dir);
        // finally（package-service.ts:225-227）：临时导出根尽力清理。
        let _ = std::fs::remove_dir_all(&export_root);
        outcome
    }

    fn package_skill_bundle(
        &self,
        bundle: &Value,
        package_root: &Path,
        target_dir: &Path,
    ) -> Result<Value, SkillExportError> {
        // buildSkillIndex（package-service.ts:93-106）：getAllSkills() 的
        // 无 agent 全目录视图 = scan_catalog（用户池+内置+外部；workspace
        // 技能不进 getAllSkills）。
        let skill_index: HashMap<String, sl::SkillEntry> = self
            .scan_catalog()
            .into_iter()
            .map(|skill| (skill.name.clone(), skill))
            .collect();
        let mut exported_skills: Vec<Value> = Vec::new();
        let mut warnings: Vec<Value> = Vec::new();
        std::fs::create_dir_all(package_root.join("skills")).map_err(|error| {
            SkillExportError::internal(format!("prepare package root: {error}"))
        })?;
        let member_names: Vec<String> = bundle["skillNames"]
            .as_array()
            .map(|names| {
                names
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        for skill_name in &member_names {
            let safe_name = sanitize_skill_name(skill_name);
            let source_dir = if safe_name.is_some() {
                self.resolve_exportable_skill_dir(skill_name, &skill_index)
            } else {
                None
            };
            // 缺技能（名字非法 / 不在池内 / 无 SKILL.md）降级为 warning
            //（package-service.ts:189-192），不让整单失败。
            let (Some(safe_name), Some(source_dir)) = (safe_name, source_dir) else {
                warnings.push(json!({ "type": "missing-skill", "name": skill_name }));
                continue;
            };
            ensure_no_symlinks(&source_dir)?;
            let target_rel = format!("skills/{safe_name}");
            copy_dir_recursive(&source_dir, &package_root.join(&target_rel))?;
            let skill_md = source_dir.join("SKILL.md");
            let exported_name = parse_skill_name(&skill_md)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| skill_name.clone());
            let final_name =
                sanitize_skill_name(&exported_name).unwrap_or_else(|| safe_name.clone());
            exported_skills.push(json!({ "name": final_name, "path": target_rel }));
        }
        if exported_skills.is_empty() {
            return Err(SkillExportError::bad_request(
                "skill bundle has no exportable skills",
            ));
        }
        let bundle_name = bundle["name"].as_str().unwrap_or("Skill Bundle");
        let file_name = export_file_name_for_bundle(bundle_name);
        let file_path = resolve_unique_export_path(target_dir, &file_name);
        let resolved_file_name = file_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or(file_name);
        let manifest = build_manifest(bundle, &exported_skills, &resolved_file_name);
        let manifest_text = serde_json::to_string_pretty(&manifest).map_err(|error| {
            SkillExportError::internal(format!("serialize bundle manifest: {error}"))
        })? + "\n";
        std::fs::write(package_root.join("bundle.json"), manifest_text).map_err(|error| {
            SkillExportError::internal(format!("write bundle manifest: {error}"))
        })?;
        write_zip_from_directory(package_root, &file_path)?;
        Ok(json!({
            "ok": true,
            "filePath": file_path,
            "fileName": resolved_file_name,
            "bundle": {
                "id": bundle["id"],
                "name": bundle_name,
                "skillCount": exported_skills.len(),
            },
            "warnings": warnings,
        }))
    }

    /// candidateSkillDirs + resolveExportableSkillDir
    ///（package-service.ts:108-131）：候选 = 技能条目 baseDir /
    /// filePath 父目录 / 用户技能池 `<name>`（TS 的 userSkillsDir 与
    /// skillsDir 是同一目录，core/engine.ts:3204-3205，故只有一份）；
    /// 候选必须在用户技能池根内（词法比较，对应 path.resolve +
    /// relative 的纯词法语义）且带真实 SKILL.md 文件。
    fn resolve_exportable_skill_dir(
        &self,
        skill_name: &str,
        skill_index: &HashMap<String, sl::SkillEntry>,
    ) -> Option<PathBuf> {
        let skills_dir = self.skills_dir();
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(skill) = skill_index.get(skill_name) {
            candidates.push(skill.base_dir.clone());
            if let Some(parent) = skill.file_path.parent() {
                candidates.push(parent.to_path_buf());
            }
        }
        candidates.push(skills_dir.join(skill_name));
        for candidate in candidates {
            if !candidate.starts_with(&skills_dir) {
                continue;
            }
            if candidate.join("SKILL.md").is_file() {
                return Some(candidate);
            }
        }
        None
    }
}

// ── HTTP 路由（skills.ts:255-262）──────────────────────────────────────────

/// POST /lingxi/v1/skills/bundles/{bundle_id}/export — 构建可下载组合包
/// 及结果元数据（zip 落盘 + filePath/fileName/warnings 回执）。失败按
/// 原语义 `err.status || 500` 显式报错。
pub async fn skills_bundle_export_route(
    State(state): State<ServiceState>,
    AxumPath(bundle_id): AxumPath<String>,
) -> Response {
    match state.skills().export_skill_bundle(&bundle_id) {
        Ok(value) => Json(value).into_response(),
        Err(error) => (
            StatusCode::from_u16(error.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(json!({ "error": error.message })),
        )
            .into_response(),
    }
}
