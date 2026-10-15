//! R06 上下文材料每 Run 重读，运行内冻结，真实调用观测同源且有界。
//! 缺失材料遵循现役默认；其它读取/解析错误响亮传播。历史观测来自
//! 实际调用的 key_events，绝不为查询临时重新编译。

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use lingxi_kernel::context::{
    ContextArtifact, ContextCompileError, ContextCompileInput, ContextCompiler, RosterEntry,
    SkillCatalogEntry,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

// ─── 错误 ───

/// 材料读取失败（缺失不算失败；其余一律响亮）。
#[derive(Debug)]
pub enum ContextMaterialError {
    Read {
        path: PathBuf,
        detail: String,
    },
    Parse {
        path: PathBuf,
        detail: String,
    },
    /// 运行时技能材料解析失败（SkillMaterialPort 端口；响亮失败，
    /// 不静默降级为空目录——R06-T06 R1 F-03/F-01 同一响亮纪律）。
    SkillMaterial {
        detail: String,
    },
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
            ContextMaterialError::SkillMaterial { detail } => {
                write!(f, "cannot resolve runtime skill material: {detail}")
            }
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
    /// 会话真实创建时刻（从权威会话行读取；冻结进本 Run artifact）。
    pub session_started_unix_ms: u64,
}

/// 材料采集端口（生产实现 = FileContextMaterialSource；测试可注入替身）。
pub trait ContextMaterialSource: Send + Sync {
    /// 只在新会话创建时采集；默认端口不启用实验。
    fn new_session_proactive_delegation(&self) -> Result<bool, ContextMaterialError> {
        Ok(false)
    }
    fn gather(&self, query: &MaterialQuery) -> Result<ContextCompileInput, ContextMaterialError>;
    fn publication_epoch(
        &self,
    ) -> Option<Arc<lingxi_adapters::storage::agent_memory::ContextMaterialEpoch>> {
        None
    }
}

/// 运行时技能材料端口（R06-T06 R1 F-01）：编译期对「该 agent 已启用技能」
/// 的解析只经此端口——生产实现绑定 SkillService（启停、白名单、工作区
/// 边界、用户覆盖顺序都在服务侧解析，镜像现役 getSkillsForAgent →
/// skillsResult → resourceLoader.getSkills 语义槽位）。解析出的条目是
/// `skill.catalog` 段的**来源材料**：走 T01 冻结的段注册表与预算钳制进入
/// 发送文本，不新造任何 system 特权通道。
pub trait SkillMaterialPort: Send + Sync {
    fn skill_catalog(&self, agent_id: &str)
        -> Result<Vec<SkillCatalogEntry>, ContextMaterialError>;
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
/// - agents/{id}/config.yaml（完整 YAML 解析与敏感字段类型验证）；
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
    material_epoch: Arc<lingxi_adapters::storage::agent_memory::ContextMaterialEpoch>,
}

impl FileContextMaterialSource {
    pub fn new(
        agents_root: PathBuf,
        user_dir: PathBuf,
        product_dir: PathBuf,
        host: HostFacts,
    ) -> Self {
        let material_epoch = lingxi_adapters::storage::agent_memory::context_material_epoch(
            agents_root.parent().unwrap_or(&agents_root),
        );
        Self {
            material_epoch,
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

/// 新会话创建路径共用同一解析规则；坏文件显式失败。
pub(crate) fn proactive_delegation_from_user_dir(
    user_dir: &Path,
) -> Result<bool, ContextMaterialError> {
    Ok(
        scan_preferences(parse_json_material(&user_dir.join("preferences.json"))?.as_ref())
            .proactive_delegation,
    )
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

/// 配置读取与记忆、技能服务共用真正的 YAML 解析和类型验证。
pub use lingxi_adapters::storage::agent_memory::config::AgentConfigScan;

pub fn scan_agent_config(
    text: &str,
) -> Result<AgentConfigScan, lingxi_adapters::storage::agent_memory::MemoryError> {
    lingxi_adapters::storage::agent_memory::config::AgentConfigDocument::parse(text)?.scan()
}

fn parse_agent_material(path: &Path) -> Result<AgentConfigScan, ContextMaterialError> {
    lingxi_adapters::storage::agent_memory::config::read_agent_config(path)
        .and_then(|doc| doc.scan())
        .map_err(|err| ContextMaterialError::Parse {
            path: path.to_path_buf(),
            detail: err.to_string(),
        })
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
                let scan = parse_agent_material(&config_path)?;
                others.push(RosterEntry {
                    name: scan.agent_name.unwrap_or_else(|| id.clone()),
                    id,
                    model: None,
                    summary: crate::persona::read_description(&dir).map_err(|error| {
                        ContextMaterialError::Read {
                            path: dir.join("description.md"),
                            detail: error.to_string(),
                        }
                    })?,
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
        summary: crate::persona::read_description(&agents_root.join(current_agent_id)).map_err(
            |error| ContextMaterialError::Read {
                path: agents_root.join(current_agent_id).join("description.md"),
                detail: error.to_string(),
            },
        )?,
    }];
    roster.append(&mut others);
    Ok(roster)
}

impl ContextMaterialSource for FileContextMaterialSource {
    fn new_session_proactive_delegation(&self) -> Result<bool, ContextMaterialError> {
        proactive_delegation_from_user_dir(&self.user_dir)
    }
    fn publication_epoch(
        &self,
    ) -> Option<Arc<lingxi_adapters::storage::agent_memory::ContextMaterialEpoch>> {
        Some(Arc::clone(&self.material_epoch))
    }
    fn gather(&self, query: &MaterialQuery) -> Result<ContextCompileInput, ContextMaterialError> {
        let agent_dir = self.agents_root.join(&query.agent_id);
        let config = parse_agent_material(&agent_dir.join("config.yaml"))?;
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
            // SDK 会话态补充段（appendSystemPrompt/contextFiles）在 Rust 侧
            // 尚无生产来源（差异报告登记）；skills 的材料归属权在
            // SkillMaterialPort 绑定后整体归该端口（R06-T06 R1 F-01：生产
            // bootstrap 绑定 SkillService，见 compiled_for_run 的端口覆盖），
            // 本材料源一侧按 T01 形状恒空。
            append_system_lines: Vec::new(),
            project_context_files: Vec::new(),
            skills: Vec::new(),
            cwd: self.host.workspace.clone(),
        })
    }
}

// ─── 编译服务（每 Run 冻结 + 实际调用观测） ───

/// 一次编译的冻结产物：发送文本与观测视图同源。
pub struct CompiledContext {
    workspace: Option<String>,
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
            workspace: None,
            artifact,
            observation,
        }
    }

    /// 发往模型的 system 文本（drive_run 的唯一来源）。
    pub fn workspace(&self) -> Option<&str> {
        self.workspace.as_deref()
    }

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

/// 只缓存已经提交的真实调用观测。材料始终在下一 Run 重读；当前 Run
/// 持有自己的 Arc 产物，不靠全局缓存维持冻结，也不因淘汰改写它。
pub const CONTEXT_OBSERVATION_CACHE_CAP: usize = 128;
pub const CONTEXT_OBSERVATION_CACHE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Default)]
struct ObservationCache {
    entries: HashMap<String, Value>,
    order: VecDeque<String>,
    bytes: usize,
    evictions: u64,
    reclaimed: u64,
}

pub struct ContextCompilerService {
    materials: Arc<dyn ContextMaterialSource>,
    skill_materials: Mutex<Option<Arc<dyn SkillMaterialPort>>>,
    cache: Mutex<ObservationCache>,
}

impl ContextCompilerService {
    pub fn new_session_proactive_delegation(&self) -> Result<bool, ContextMaterialError> {
        self.materials.new_session_proactive_delegation()
    }
    pub fn new(materials: Arc<dyn ContextMaterialSource>) -> Self {
        Self {
            materials,
            skill_materials: Mutex::new(None),
            cache: Mutex::new(ObservationCache::default()),
        }
    }
    pub fn set_skill_materials(&self, port: Arc<dyn SkillMaterialPort>) {
        *self
            .skill_materials
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(port);
    }

    fn gather(&self, query: &MaterialQuery) -> Result<ContextCompileInput, ContextCompileFailure> {
        let mut input = self
            .materials
            .gather(query)
            .map_err(ContextCompileFailure::Material)?;
        if let Some(port) = self
            .skill_materials
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            input.skills = port
                .skill_catalog(&query.agent_id)
                .map_err(ContextCompileFailure::Material)?;
        }
        Ok(input)
    }

    /// 无身份复用：每次调用新采集并编译，材料更新失败不会沿用旧版本。
    /// 此端口本身不把未发送的编译结果登记成观测。
    pub fn compiled_for_run(
        &self,
        session_id: &str,
        agent_id: &str,
        for_subagent: bool,
        session_started_unix_ms: u64,
    ) -> Result<Arc<CompiledContext>, ContextCompileFailure> {
        self.compile_snapshot(
            session_id,
            agent_id,
            for_subagent,
            session_started_unix_ms,
            true,
        )
    }

    /// 同步最终保存时保留资料发布栅栏；活动写者存在时显式拒绝。
    pub fn with_material_commit<T>(
        &self,
        commit: impl FnOnce() -> Result<T, lingxi_kernel::ports::StorageError>,
    ) -> Result<T, lingxi_kernel::ports::StorageError> {
        match self.materials.publication_epoch() {
            Some(epoch) => epoch.with_stable_commit(commit).unwrap_or_else(|| {
                Err(lingxi_kernel::ports::StorageError::Conflict {
                    detail: "context material publication active at manual commit".into(),
                })
            }),
            None => commit(),
        }
    }

    pub fn compile_snapshot(
        &self,
        _session_id: &str,
        agent_id: &str,
        for_subagent: bool,
        session_started_unix_ms: u64,
        session_memory_enabled: bool,
    ) -> Result<Arc<CompiledContext>, ContextCompileFailure> {
        self.compile_snapshot_with_workspace(
            agent_id,
            for_subagent,
            session_started_unix_ms,
            session_memory_enabled,
            None,
            None,
        )
    }

    /// 实际会话运行使用持久工作区；读取失败响亮退出，不偷偷使用另一目录。
    pub fn compile_session_snapshot(
        &self,
        storage: Option<&lingxi_adapters::storage::RunDatabase>,
        session_id: &str,
        agent_id: &str,
        for_subagent: bool,
        session_started_unix_ms: u64,
        session_memory_enabled: bool,
    ) -> Result<Arc<CompiledContext>, ContextCompileFailure> {
        let workspace = storage
            .map(|storage| crate::session_ui::workspace(storage, session_id))
            .transpose()
            .map_err(|error| {
                ContextCompileFailure::Material(ContextMaterialError::SkillMaterial {
                    detail: format!("session workspace read failed: {error:?}"),
                })
            })?
            .flatten();
        let proactive = storage
            .map(|db| {
                lingxi_adapters::storage::session_admin::session_proactive_delegation(
                    db, session_id,
                )
            })
            .transpose()
            .map_err(|error| {
                ContextCompileFailure::Material(ContextMaterialError::SkillMaterial {
                    detail: format!("session creation policy read failed: {error:?}"),
                })
            })?
            .unwrap_or(false);
        self.compile_snapshot_with_workspace(
            agent_id,
            for_subagent,
            session_started_unix_ms,
            session_memory_enabled,
            workspace,
            Some(proactive && !for_subagent),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn compile_snapshot_with_workspace(
        &self,
        agent_id: &str,
        for_subagent: bool,
        session_started_unix_ms: u64,
        session_memory_enabled: bool,
        workspace: Option<String>,
        proactive: Option<bool>,
    ) -> Result<Arc<CompiledContext>, ContextCompileFailure> {
        let query = MaterialQuery {
            agent_id: agent_id.to_string(),
            for_subagent,
            session_started_unix_ms,
        };
        // 文件直接合法更新没有服务栅栏，第二次实际采集校验同一份内容；服务写入
        // 则同时核验发布代次和活动写者。有限重试后响亮失败，不假称新版本成功。
        for _ in 0..8 {
            let epoch = self.materials.publication_epoch();
            let Some(before) = epoch
                .as_ref()
                .map_or(Some(0), |epoch| epoch.stable_revision())
            else {
                std::thread::yield_now();
                continue;
            };
            let mut input = self.gather(&query)?;
            let confirm = self.gather(&query)?;
            let after = epoch
                .as_ref()
                .map_or(Some(0), |epoch| epoch.stable_revision());
            if after != Some(before) || input != confirm {
                std::thread::yield_now();
                continue;
            }
            if let Some(workspace) = workspace.as_ref() {
                input.cwd = workspace.clone();
            }
            input.memory_enabled &= session_memory_enabled;
            if let Some(proactive) = proactive {
                input.proactive_delegation = proactive;
            }
            let material_sha = sha256_hex(&format!("{input:?}"));
            let artifact =
                ContextCompiler::compile(&input).map_err(ContextCompileFailure::Compile)?;
            let mut compiled = CompiledContext::new(artifact);
            compiled.workspace = (!input.cwd.is_empty()).then(|| input.cwd.clone());
            compiled.observation["materialSha256"] = Value::String(material_sha);
            compiled.observation["materialRevision"] = Value::from(before);
            compiled.observation["sessionMemoryEnabled"] = Value::Bool(session_memory_enabled);
            return Ok(Arc::new(compiled));
        }
        Err(ContextCompileFailure::Material(
            ContextMaterialError::SkillMaterial {
                detail:
                    "context materials changed during snapshot acquisition; no snapshot published"
                        .to_string(),
            },
        ))
    }

    /// 只在真实调用的持久记录提交后发布，key 为真实 snapshot+call，不是 session。
    pub fn record_call_observation(&self, view: Value) {
        let key = format!(
            "{}:{}",
            view["snapshotId"].as_str().unwrap_or(""),
            view["modelCallId"].as_str().unwrap_or("")
        );
        let size = view.to_string().len();
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        if cache.entries.contains_key(&key) {
            return;
        }
        // A single view larger than the whole byte budget is refused
        // BEFORE any eviction: rejecting an insert must never destroy
        // already-committed call observations. The cache stays bounded
        // either way; the refusal is observable via the surviving entries.
        if size > CONTEXT_OBSERVATION_CACHE_BYTES {
            return;
        }
        while !cache.order.is_empty()
            && (cache.entries.len() >= CONTEXT_OBSERVATION_CACHE_CAP
                || cache.bytes.saturating_add(size) > CONTEXT_OBSERVATION_CACHE_BYTES)
        {
            let oldest = cache.order.pop_front().expect("非空队列");
            if let Some(old) = cache.entries.remove(&oldest) {
                cache.bytes -= old.to_string().len();
                cache.evictions += 1;
            }
        }
        cache.bytes += size;
        cache.order.push_back(key.clone());
        cache.entries.insert(key, view);
    }

    /// 内存读面只用于可信宿主；HTTP 历史读面以实际持久化调用为准。
    pub fn observation_for_session(&self, session_id: &str) -> Option<Value> {
        let cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        for shape in [false, true] {
            if let Some(view) = cache
                .order
                .iter()
                .rev()
                .filter_map(|key| cache.entries.get(key))
                .find(|v| v["sessionId"] == session_id && v["forSubagent"] == shape)
            {
                return Some(view.clone());
            }
        }
        None
    }
    pub fn invalidate_session(&self, session_id: &str) {
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        let removed: Vec<String> = cache
            .entries
            .iter()
            .filter(|(_, v)| v["sessionId"] == session_id)
            .map(|(k, _)| k.clone())
            .collect();
        for key in &removed {
            if let Some(v) = cache.entries.remove(key) {
                cache.bytes -= v.to_string().len();
                cache.reclaimed += 1;
            }
        }
        cache.order.retain(|k| !removed.contains(k));
    }
    pub fn cached_session_ids(&self) -> Vec<String> {
        let cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        let mut ids: Vec<String> = cache
            .entries
            .values()
            .filter_map(|v| v["sessionId"].as_str().map(str::to_string))
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }
    pub fn cache_stats(&self) -> Value {
        let cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        serde_json::json!({"entries":cache.entries.len(),"bytes":cache.bytes,"capacity":CONTEXT_OBSERVATION_CACHE_CAP,"byteCapacity":CONTEXT_OBSERVATION_CACHE_BYTES,"evictions":cache.evictions,"reclaimed":cache.reclaimed,"policy":"oldest_committed_call_first"})
    }
}

impl fmt::Debug for ContextCompilerService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContextCompilerService")
            .field("cache", &self.cache_stats())
            .finish()
    }
}
