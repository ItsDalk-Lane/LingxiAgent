//! R06-T01 服务侧上下文编译：材料采集 + 会话级冻结缓存 + 观测装饰。
//!
//! 职责切分（R01 依赖规则）：
//! - kernel `lingxi_kernel::context` 拥有纯确定性的编译/渲染/预算/观测视图；
//! - 本模块拥有「材料从哪来」（$LINGXI_HOME 文件布局、preferences、config.yaml
//!   窄扫描、platform note 组装、会话开始标签格式化）与「观测的 digest 装饰」
//!   （sha256 在本 crate 的锁内依赖里，kernel 不引）。
//!
//! 同源契约（R06-A01）：`ContextCompilerService::compiled_for_run` 返回的
//! `CompiledContext` 同时是 (a) drive_run 发给 provider 的 system 文本来源
//! （`render()`）与 (b) `GET /lingxi/v1/sessions/{id}/context-observation`
//! 的观测来源（`observation()`）——不存在第二套 prompt 重建。
//!
//! 会话级冻结：编译结果按 (session_id, for_subagent) 缓存，与现役
//! session-coordinator 的会话快照同语义；跨重启重编译（材料漂移经 digest
//! 变化可观测）。
//!
//! 读取纪律：材料文件缺失按现役 `safeReadFile` 语义读为空；缺失以外的 I/O
//! 错误与 JSON 损坏一律响亮失败（本栈 R02 完整性立场，禁止静默降级）。

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use lingxi_kernel::context::{
    ContextArtifact, ContextCompileError, ContextCompileInput, ContextCompiler, RosterEntry,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

// ─── 错误 ───

/// 材料读取失败（缺失不算失败；其余一律响亮）。
#[derive(Debug)]
pub enum ContextMaterialError {
    Read { path: PathBuf, detail: String },
    Parse { path: PathBuf, detail: String },
}

impl fmt::Display for ContextMaterialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContextMaterialError::Read { path, detail } => {
                write!(
                    f,
                    "cannot read context material {}: {detail}",
                    path.display()
                )
            }
            ContextMaterialError::Parse { path, detail } => write!(
                f,
                "cannot parse context material {}: {detail}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ContextMaterialError {}

/// 编译链失败：材料层或 kernel 编译层（映射为 run 的响亮失败，
/// code 前缀 `context_compile:`）。
#[derive(Debug)]
pub enum ContextCompileFailure {
    Material(ContextMaterialError),
    Compile(ContextCompileError),
}

impl fmt::Display for ContextCompileFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContextCompileFailure::Material(err) => write!(f, "material: {err}"),
            ContextCompileFailure::Compile(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ContextCompileFailure {}

// ─── 材料查询与端口 ───

/// 一次编译所需的会话上下文坐标。
pub struct MaterialQuery {
    pub agent_id: String,
    pub for_subagent: bool,
    /// 会话开始时刻（会话内首次编译的 run 受理时刻；冻结进 artifact）。
    pub session_started_unix_ms: u64,
}

/// 材料采集端口（生产实现 = FileContextMaterialSource；测试可注入替身）。
pub trait ContextMaterialSource: Send + Sync {
    fn gather(&self, query: &MaterialQuery) -> Result<ContextCompileInput, ContextMaterialError>;
}

// ─── 主机事实（组合根解析一次；kernel/材料源不直接读 env） ───

/// platform note 与 cwd 段所需的主机事实（现役 getPlatformPromptNote 的
/// 入参等价物；cwd 在 environment note 中恒为空串——现役同形状，真实工作
/// 目录经 `<cwd>` 包装段到达模型）。
#[derive(Debug, Clone)]
pub struct HostFacts {
    /// process.platform 等价物（darwin/linux/win32/...）。
    pub platform: String,
    /// Node os.type() 等价物（Darwin/Linux/Windows_NT）。
    pub os_type: String,
    /// Node os.release() 等价物（非 unix 平台当前为空串，见 T01 报告差异节）。
    pub os_release: String,
    /// 登录 shell 基名（win32 恒 powershell；其余 $SHELL 基名，缺省 bash）。
    pub shell_label: String,
    /// `<cwd>` 包装段值：config 声明的 workspace；未配置时为 home 根。
    pub workspace: String,
}

/// 现役 SANDBOX_MODE_LABEL（lib/sandbox/policy.ts 的冻结常量）。
const SANDBOX_MODE_LABEL: &str = "read-all_write-scoped_network-on";

impl HostFacts {
    /// 现役 getPlatformPromptNote 逐行移植（cwd 恒空——现役调用点不传 cwd）。
    pub fn platform_note(&self) -> String {
        [
            "<environment_context>".to_string(),
            format!("  <platform>{}</platform>", self.platform),
            "  <cwd></cwd>".to_string(),
            format!("  <shell>{}</shell>", self.shell_label),
            format!("  <os>{} {}</os>", self.os_type, self.os_release),
            format!("  <sandbox_mode>{SANDBOX_MODE_LABEL}</sandbox_mode>"),
            "</environment_context>".to_string(),
            "Server execution environment; the user's display device may differ.".to_string(),
        ]
        .join("\n")
    }
}

/// 从运行环境采集 HostFacts（仅 main 组合根调用）。
pub fn probe_host_facts(workspace: Option<&Path>, home: &Path) -> HostFacts {
    let platform = match std::env::consts::OS {
        "macos" => "darwin".to_string(),
        "windows" => "win32".to_string(),
        other => other.to_string(),
    };
    let (os_type, os_release) = probe_os_type_release(&platform);
    let shell_label = if platform == "win32" {
        "powershell".to_string()
    } else {
        std::env::var("SHELL")
            .ok()
            .and_then(|shell| {
                let base = shell.rsplit('/').next().unwrap_or("");
                let base = base.strip_suffix(".exe").unwrap_or(base);
                if base.is_empty() {
                    None
                } else {
                    Some(base.to_string())
                }
            })
            .unwrap_or_else(|| "bash".to_string())
    };
    let workspace = workspace
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| home.to_string_lossy().to_string());
    HostFacts {
        platform,
        os_type,
        os_release,
        shell_label,
        workspace,
    }
}

#[cfg(unix)]
fn probe_os_type_release(_platform: &str) -> (String, String) {
    // uname(2) 与 Node os.type()/os.release() 同源（sysname/release）。
    // 失败时保持空串——environment note 是 digest-only 段，缺失不阻断编译，
    // 但观测 digest 会如实变化（可观测，非静默）。
    let mut uts: libc::utsname = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::uname(&mut uts) };
    if ok != 0 {
        return (String::new(), String::new());
    }
    let read = |field: &[libc::c_char]| {
        let bytes: Vec<u8> = field
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u8)
            .collect();
        String::from_utf8_lossy(&bytes).to_string()
    };
    (read(&uts.sysname), read(&uts.release))
}

#[cfg(not(unix))]
fn probe_os_type_release(platform: &str) -> (String, String) {
    // Windows：Node os.type() = "Windows_NT"；release 需要版本 FFI，
    // T01 留空（差异报告登记）。
    let os_type = if platform == "win32" {
        "Windows_NT"
    } else {
        platform
    };
    (os_type.to_string(), String::new())
}

// ─── product_dir 解析（纯函数，可测） ───

/// 人格/模板目录（现役 productDir = 仓库 `lib/` 等价物）解析链：
/// 1. LINGXI_PRODUCT_DIR 显式覆盖；
/// 2. 从 exe 位置向上找含 `lib/yuan/lingxi.md` 的祖先（开发布局）；
/// 3. `<home>/product`（打包布局占位）。
///
/// 找不到时返回 `<home>/product`——yuan 缺失在编译期响亮失败（现役 throw
/// 对齐），不在此处静默兜底。
pub fn resolve_product_dir(env_override: Option<&str>, exe_path: &Path, home: &Path) -> PathBuf {
    if let Some(value) = env_override.filter(|v| !v.trim().is_empty()) {
        return PathBuf::from(value);
    }
    let mut dir = exe_path.parent();
    while let Some(candidate_dir) = dir {
        let candidate = candidate_dir.join("lib");
        if candidate.join("yuan").join("lingxi.md").is_file() {
            return candidate;
        }
        dir = candidate_dir.parent();
    }
    home.join("product")
}

// ─── 会话开始时间标签（现役 Intl en-US 形状） ───

/// 现役 `new Intl.DateTimeFormat("en-US", {weekday/year/month/day long-numeric,
/// hour/minute 2-digit, timeZoneName short, hourCycle h23})` 的输出形状：
/// "Thursday, June 4, 2026 at 15:53 GMT+8"。T01 用系统本地时区
/// （IANA 命名时区库不在锁内；prefs 命名时区偏好为已知差距，见差异报告）。
pub fn format_session_started_label(unix_ms: u64) -> String {
    use chrono::{Datelike, Local, TimeZone, Timelike};
    let millis = i64::try_from(unix_ms).unwrap_or(i64::MAX);
    let dt = Local
        .timestamp_millis_opt(millis)
        .single()
        .unwrap_or_else(|| {
            Local
                .timestamp_millis_opt(0)
                .single()
                .unwrap_or_else(Local::now)
        });
    const WEEKDAYS: [&str; 7] = [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ];
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let weekday = WEEKDAYS[dt.weekday().num_days_from_monday() as usize];
    let month = MONTHS[(dt.month() - 1) as usize];
    let offset_seconds = dt.offset().local_minus_utc();
    let sign = if offset_seconds < 0 { "-" } else { "+" };
    let abs = offset_seconds.unsigned_abs();
    let offset_hours = abs / 3600;
    let offset_minutes = (abs % 3600) / 60;
    let zone = if offset_minutes == 0 {
        format!("GMT{sign}{offset_hours}")
    } else {
        format!("GMT{sign}{offset_hours}:{offset_minutes:02}")
    };
    format!(
        "{weekday}, {month} {}, {} at {:02}:{:02} {zone}",
        dt.day(),
        dt.year(),
        dt.hour(),
        dt.minute()
    )
}

// ─── 文件材料源（现役 $LINGXI_HOME 布局） ───

/// 从 $LINGXI_HOME 文件布局采集编译材料：
/// - agents/{id}/config.yaml（窄扫描 locale/agent.name/agent.yuan/memory.enabled）；
/// - agents/{id}/identity.md、AGENTS.md（resolvePersonaSource 回落链）；
/// - agents/{id}/memory/memory.md、memory/tenets.json、appearance-summary.json；
/// - user/preferences.json（userName/locale/experiments/learn_skills）；
/// - user/user.md；
/// - product_dir 的 yuan/identity-templates/agents-templates/example 回落。
pub struct FileContextMaterialSource {
    agents_root: PathBuf,
    user_dir: PathBuf,
    product_dir: PathBuf,
    host: HostFacts,
}

impl FileContextMaterialSource {
    pub fn new(
        agents_root: PathBuf,
        user_dir: PathBuf,
        product_dir: PathBuf,
        host: HostFacts,
    ) -> Self {
        Self {
            agents_root,
            user_dir,
            product_dir,
            host,
        }
    }

    /// 组合根便捷构造：从 home 根派生现役布局。
    pub fn from_home(home: &Path, product_dir: PathBuf, host: HostFacts) -> Self {
        Self::new(home.join("agents"), home.join("user"), product_dir, host)
    }
}

/// 缺失 → 空串（现役 safeReadFile 语义）；其余错误响亮。
fn read_material(path: &Path) -> Result<String, ContextMaterialError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(ContextMaterialError::Read {
            path: path.to_path_buf(),
            detail: err.to_string(),
        }),
    }
}

fn parse_json_material(path: &Path) -> Result<Option<Value>, ContextMaterialError> {
    let text = read_material(path)?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|err| ContextMaterialError::Parse {
            path: path.to_path_buf(),
            detail: err.to_string(),
        })
}

/// config.yaml 的 T01 窄扫描结果。现役用完整 YAML 解析；本扫描器只识别
/// 两格缩进的四个键（顶层 locale、agent.name、agent.yuan、memory.enabled），
/// 注释与引号按行内规则剥离。这是已登记的子集实现（T01 报告差异节）。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct AgentConfigScan {
    pub locale: Option<String>,
    pub agent_name: Option<String>,
    pub yuan: Option<String>,
    pub memory_enabled: Option<bool>,
}

pub fn scan_agent_config(text: &str) -> AgentConfigScan {
    fn clean_value(raw: &str) -> String {
        // 剥离行内注释（" #" 起）与成对引号。
        let without_comment = raw.split(" #").next().unwrap_or(raw);
        let trimmed = without_comment.trim();
        trimmed
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| {
                trimmed
                    .strip_prefix('\'')
                    .and_then(|v| v.strip_suffix('\''))
            })
            .unwrap_or(trimmed)
            .to_string()
    }
    let mut scan = AgentConfigScan::default();
    let mut section = String::new();
    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() || trimmed.trim_start().starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let body = line.trim();
        if indent == 0 {
            if let Some(name) = body.strip_suffix(':') {
                section = name.trim().to_string();
                continue;
            }
            if let Some((key, value)) = body.split_once(':') {
                section.clear();
                if key.trim() == "locale" {
                    let value = clean_value(value);
                    if !value.is_empty() {
                        scan.locale = Some(value);
                    }
                }
            }
            continue;
        }
        let Some((key, value)) = body.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = clean_value(value);
        match (section.as_str(), key) {
            ("agent", "name") if !value.is_empty() => scan.agent_name = Some(value),
            ("agent", "yuan") if !value.is_empty() => scan.yuan = Some(value),
            ("memory", "enabled") => {
                scan.memory_enabled = match value.as_str() {
                    "true" => Some(true),
                    "false" => Some(false),
                    _ => None,
                };
            }
            _ => {}
        }
    }
    scan
}

/// preferences.json 的 T01 读取面（现役 PreferencesManager 子集）。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PreferencesScan {
    pub user_name: Option<String>,
    pub locale: Option<String>,
    pub proactive_delegation: bool,
    pub learn_skills_enabled: bool,
}

/// 从已解析的 preferences JSON 提取 T01 读取面（`None` = 文件缺失）。
pub fn scan_preferences(value: Option<&Value>) -> PreferencesScan {
    let mut scan = PreferencesScan::default();
    let Some(prefs) = value else {
        return scan;
    };
    if let Some(name) = prefs.get("userName").and_then(Value::as_str) {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            scan.user_name = Some(trimmed.to_string());
        }
    }
    if let Some(locale) = prefs.get("locale").and_then(Value::as_str) {
        let trimmed = locale.trim();
        if !trimmed.is_empty() {
            scan.locale = Some(trimmed.to_string());
        }
    }
    // 现役 getExperimentValue：experiments[id] 严格布尔才生效。
    scan.proactive_delegation = prefs
        .get("experiments")
        .and_then(|exp| exp.get("subagent.proactive_delegation"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // 现役门槛：learnCfg.enabled && learnCfg.allow_github_fetch；
    // enabled 默认 true，allow_github_fetch 默认 false（缺省不注入该段）。
    let learn = prefs.get("learn_skills");
    let enabled = learn
        .and_then(|cfg| cfg.get("enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let allow_github = learn
        .and_then(|cfg| cfg.get("allow_github_fetch"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    scan.learn_skills_enabled = enabled && allow_github;
    scan
}

/// 现役 resolvePersonaSource 回落链（无 migration-degraded 分支——那是
/// Electron 启动改名失败记录，本服务没有该状态来源；差异报告登记）。
fn resolve_persona_source(
    agent_dir: &Path,
    product_dir: &Path,
    yuan_type: &str,
    zh: bool,
    identity: bool,
) -> Result<String, ContextMaterialError> {
    let (file_name, template_dir, example_file) = if identity {
        ("identity.md", "identity-templates", "identity.example.md")
    } else {
        ("AGENTS.md", "agents-templates", "agents.example.md")
    };
    let lang_dir = if zh { "" } else { "en/" };
    let own = read_material(&agent_dir.join(file_name))?;
    if !own.is_empty() {
        return Ok(own);
    }
    let lang_template = read_material(
        &product_dir
            .join(template_dir)
            .join(format!("{lang_dir}{yuan_type}.md")),
    )?;
    if !lang_template.is_empty() {
        return Ok(lang_template);
    }
    let generic = read_material(
        &product_dir
            .join(template_dir)
            .join(format!("{yuan_type}.md")),
    )?;
    if !generic.is_empty() {
        return Ok(generic);
    }
    read_material(&product_dir.join(example_file))
}

/// 现役 _readYuan：语言专属优先，空串落通用版。
fn read_yuan(
    product_dir: &Path,
    yuan_type: &str,
    zh: bool,
) -> Result<String, ContextMaterialError> {
    let lang_dir = if zh { "" } else { "en/" };
    let localized = read_material(
        &product_dir
            .join("yuan")
            .join(format!("{lang_dir}{yuan_type}.md")),
    )?;
    if !localized.is_empty() {
        return Ok(localized);
    }
    read_material(&product_dir.join("yuan").join(format!("{yuan_type}.md")))
}

/// 现役 activeTenets 读取（读路径降级语义的响亮化版本：文件缺失=空库，
/// 损坏=响亮失败；normalizeTenet 的 priority/status 归一化保留）。
fn read_active_tenets(agent_dir: &Path) -> Result<Vec<String>, ContextMaterialError> {
    let path = agent_dir.join("memory").join("tenets.json");
    let Some(value) = parse_json_material(&path)? else {
        return Ok(Vec::new());
    };
    let entries = value
        .get("tenets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut active: Vec<(u32, String, String)> = Vec::new();
    for entry in entries {
        let content = entry
            .get("content")
            .and_then(Value::as_str)
            .map(|text| {
                text.replace("\r\n", "\n")
                    .replace('\r', "\n")
                    .trim()
                    .to_string()
            })
            .unwrap_or_default();
        if content.is_empty() {
            continue;
        }
        let status = entry.get("status").and_then(Value::as_str).unwrap_or("");
        let status = if status == "active" || status == "rejected" {
            status
        } else {
            "pending"
        };
        if status != "active" {
            continue;
        }
        let priority = entry
            .get("priority")
            .and_then(Value::as_str)
            .unwrap_or("medium");
        let weight = match priority {
            "critical" => 0,
            "high" => 1,
            "medium" => 2,
            "low" => 3,
            _ => 2,
        };
        // 现役 normalizeTenet 对缺失 createdAt 回填当前时间（排序键不确定）；
        // 本实现用空串保持确定性（差异报告登记）。
        let created_at = entry
            .get("createdAt")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        active.push((weight, created_at, content));
    }
    active.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    Ok(active.into_iter().map(|(_, _, content)| content).collect())
}

fn read_appearance_summary(agent_dir: &Path) -> Result<Option<String>, ContextMaterialError> {
    let path = agent_dir.join("appearance-summary.json");
    let Some(value) = parse_json_material(&path)? else {
        return Ok(None);
    };
    Ok(value
        .get("summary")
        .and_then(Value::as_str)
        .map(str::to_string))
}

/// 花名册扫描：agents_root 下含 config.yaml 的子目录各为一个 agent；
/// 当前 agent 永远在内（目录缺失时合成自身条目）。顺序：自身在前，
/// 其余按 id 字典序（现役为注册表顺序，本实现取确定性序；差异报告登记）。
fn scan_roster(
    agents_root: &Path,
    current_agent_id: &str,
    current_agent_name: &str,
) -> Result<Vec<RosterEntry>, ContextMaterialError> {
    let mut others: Vec<RosterEntry> = Vec::new();
    match std::fs::read_dir(agents_root) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(|err| ContextMaterialError::Read {
                    path: agents_root.to_path_buf(),
                    detail: err.to_string(),
                })?;
                let dir = entry.path();
                if !dir.is_dir() {
                    continue;
                }
                let config_path = dir.join("config.yaml");
                if !config_path.is_file() {
                    continue;
                }
                let id = entry.file_name().to_string_lossy().to_string();
                if id == current_agent_id {
                    continue;
                }
                let scan = scan_agent_config(&read_material(&config_path)?);
                others.push(RosterEntry {
                    name: scan.agent_name.unwrap_or_else(|| id.clone()),
                    id,
                    model: None,
                    summary: None,
                });
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(ContextMaterialError::Read {
                path: agents_root.to_path_buf(),
                detail: err.to_string(),
            })
        }
    }
    others.sort_by(|a, b| a.id.cmp(&b.id));
    let mut roster = vec![RosterEntry {
        id: current_agent_id.to_string(),
        name: current_agent_name.to_string(),
        model: None,
        summary: None,
    }];
    roster.append(&mut others);
    Ok(roster)
}

impl ContextMaterialSource for FileContextMaterialSource {
    fn gather(&self, query: &MaterialQuery) -> Result<ContextCompileInput, ContextMaterialError> {
        let agent_dir = self.agents_root.join(&query.agent_id);
        let config = scan_agent_config(&read_material(&agent_dir.join("config.yaml"))?);
        let prefs_path = self.user_dir.join("preferences.json");
        let prefs = scan_preferences(parse_json_material(&prefs_path)?.as_ref());

        // 现役 resolveLocale：agent config locale → 全局 prefs locale → "en"。
        let locale = config
            .locale
            .clone()
            .or(prefs.locale.clone())
            .unwrap_or_else(|| "en".to_string());
        let zh = locale.to_lowercase().starts_with("zh");
        // 现役 init()：agent.userName = resolveUserName()（全局 prefs → 语言兜底），
        // 两处用户名同源（R06-T01 取证：agent.ts L352/L378）。
        let resolved_user_name = prefs
            .user_name
            .clone()
            .unwrap_or_else(|| if zh { "用户" } else { "User" }.to_string());
        let agent_name = config
            .agent_name
            .clone()
            .unwrap_or_else(|| "Lingxi".to_string());
        let yuan_type = config.yuan.clone().unwrap_or_else(|| "lingxi".to_string());

        let user_profile = read_material(&self.user_dir.join("user.md"))?;
        let memory_md = read_material(&agent_dir.join("memory").join("memory.md"))?;
        let tenets = read_active_tenets(&agent_dir)?;
        let appearance_summary = read_appearance_summary(&agent_dir)?;
        let roster = scan_roster(&self.agents_root, &query.agent_id, &agent_name)?;

        Ok(ContextCompileInput {
            locale,
            for_subagent: query.for_subagent,
            // 现役：config.memory?.enabled !== false（默认开启）。
            memory_enabled: config.memory_enabled.unwrap_or(true),
            user_name: resolved_user_name.clone(),
            resolved_user_name,
            agent_name,
            agent_id: query.agent_id.clone(),
            environment_note: self.host.platform_note(),
            user_profile_md: if user_profile.is_empty() {
                None
            } else {
                Some(user_profile)
            },
            identity_md: resolve_persona_source(
                &agent_dir,
                &self.product_dir,
                &yuan_type,
                zh,
                true,
            )?,
            yuan_md: read_yuan(&self.product_dir, &yuan_type, zh)?,
            agents_md: resolve_persona_source(
                &agent_dir,
                &self.product_dir,
                &yuan_type,
                zh,
                false,
            )?,
            // T01 简化：现役另有 vision 模型能力门控；本实现摘要存在即注入
            // （差异报告登记）。
            inject_appearance: appearance_summary.is_some(),
            appearance_summary,
            // 无桌面 engine 的 headless 服务：computer-use 恒不可用（差异报告登记）。
            computer_use_available: false,
            learn_skills_enabled: prefs.learn_skills_enabled,
            proactive_delegation: prefs.proactive_delegation,
            roster,
            memory_md,
            tenets,
            session_started_label: format_session_started_label(query.session_started_unix_ms),
            // SDK 会话态补充段（appendSystemPrompt/contextFiles/skills）在 Rust
            // 侧尚无生产来源（技能目录归 T06 SkillService）；T01 恒空，
            // kernel 格式支持完整（差异报告登记）。
            append_system_lines: Vec::new(),
            project_context_files: Vec::new(),
            skills: Vec::new(),
            cwd: self.host.workspace.clone(),
        })
    }
}

// ─── 编译服务（会话级冻结 + digest 装饰） ───

/// 一次编译的冻结产物：发送文本与观测视图同源。
pub struct CompiledContext {
    artifact: ContextArtifact,
    observation: Value,
}

fn sha256_hex(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

impl CompiledContext {
    fn new(artifact: ContextArtifact) -> Self {
        let mut observation = artifact.observation_view();
        // digest 装饰：每段 sha256（对发送文本的段切片计算——观测与发送
        // 同一份字节的机器可验证明）；整文 digest 供一次性对照。
        if let Some(segments) = observation
            .get_mut("segments")
            .and_then(Value::as_array_mut)
        {
            for segment in segments.iter_mut() {
                let start = segment
                    .get("byteStart")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize;
                let end = segment.get("byteEnd").and_then(Value::as_u64).unwrap_or(0) as usize;
                let digest = sha256_hex(&artifact.render()[start..end]);
                segment["sha256"] = Value::String(digest);
            }
        }
        observation["renderSha256"] = Value::String(sha256_hex(artifact.render()));
        Self {
            artifact,
            observation,
        }
    }

    /// 发往模型的 system 文本（drive_run 的唯一来源）。
    pub fn render(&self) -> &str {
        self.artifact.render()
    }

    /// 观测视图（观测端点的唯一来源）。
    pub fn observation(&self) -> &Value {
        &self.observation
    }
}

impl fmt::Debug for CompiledContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompiledContext")
            .field("artifact", &self.artifact)
            .finish_non_exhaustive()
    }
}

/// 会话级上下文编译器：同一 (session_id, for_subagent) 的重复编译返回
/// 同一份冻结 artifact（现役会话快照语义；跨重启重编译）。
pub struct ContextCompilerService {
    materials: Arc<dyn ContextMaterialSource>,
    cache: Mutex<HashMap<(String, bool), Arc<CompiledContext>>>,
}

impl ContextCompilerService {
    pub fn new(materials: Arc<dyn ContextMaterialSource>) -> Self {
        Self {
            materials,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// drive_run 的编译入口：provider 检查之后、首个模型调用之前调用。
    /// 编译失败 = 响亮 run 失败（调用方映射 FailureCause），不静默降级。
    pub fn compiled_for_run(
        &self,
        session_id: &str,
        agent_id: &str,
        for_subagent: bool,
        now_unix_ms: u64,
    ) -> Result<Arc<CompiledContext>, ContextCompileFailure> {
        let key = (session_id.to_string(), for_subagent);
        if let Some(hit) = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&key)
        {
            return Ok(Arc::clone(hit));
        }
        let input = self
            .materials
            .gather(&MaterialQuery {
                agent_id: agent_id.to_string(),
                for_subagent,
                session_started_unix_ms: now_unix_ms,
            })
            .map_err(ContextCompileFailure::Material)?;
        let artifact = ContextCompiler::compile(&input).map_err(ContextCompileFailure::Compile)?;
        let compiled = Arc::new(CompiledContext::new(artifact));
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key, Arc::clone(&compiled));
        Ok(compiled)
    }

    /// 观测端点读面：只返回已冻结的 artifact（没有 = 该会话尚未发送过任何
    /// 请求，观测不存在第二份构建）。主 agent 形态优先。
    pub fn observation_for_session(&self, session_id: &str) -> Option<Value> {
        let cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache
            .get(&(session_id.to_string(), false))
            .or_else(|| cache.get(&(session_id.to_string(), true)))
            .map(|compiled| compiled.observation().clone())
    }
}

impl fmt::Debug for ContextCompilerService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cached = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len();
        f.debug_struct("ContextCompilerService")
            .field("cached_sessions", &cached)
            .finish()
    }
}
