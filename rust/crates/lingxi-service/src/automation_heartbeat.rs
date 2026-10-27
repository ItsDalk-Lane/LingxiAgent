//! 心跳只生成已授权材料的巡检计划；实际读写和执行仍交给资源服务及原 Run 链。
//!
//! 调用者须以本轮真实主体/会话通过 ResourceIoService 收集材料。这里不读取
//! 文件、不授予目录权限，也不因为材料里有文件名就扩大原来的授权范围。
use chrono::{DateTime, Duration, LocalResult, NaiveDate, TimeZone};
use lingxi_kernel::session_tree::sha256_hex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

pub const DEFAULT_HEARTBEAT_INTERVAL_MS: u64 = 31 * 60 * 1_000;
pub const MANUAL_HEARTBEAT_COOLDOWN_MS: u64 = 2 * 60 * 1_000;
pub const HEARTBEAT_RUN_TIMEOUT_MS: u64 = 5 * 60 * 1_000;
pub const DAILY_STARTUP_IDLE_DELAY_MS: u64 = 60 * 1_000;
pub const DAILY_RETRY_DELAY_MS: u64 = 60 * 60 * 1_000;
pub const WORKSPACE_OUTPUT_ROOT: &str = "OH-Works";
const EXEC_LOG_START: &str = "<!-- exec-log -->";
const EXEC_LOG_END: &str = "<!-- /exec-log -->";
const PATROL_LOG_MAX_ENTRIES: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatError(pub String);

impl std::fmt::Display for HeartbeatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for HeartbeatError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeartbeatSettings {
    pub enabled: bool,
    pub interval_ms: u64,
}

impl HeartbeatSettings {
    /// 总开关默认开，但每个助手须明确选择开启；零间隔沿用现役的 31 分钟默认。
    pub fn from_config(config: &Value, preferences: &Value) -> Result<Self, HeartbeatError> {
        let interval_ms = match config.pointer("/desk/heartbeat_interval") {
            None | Some(Value::Null) => DEFAULT_HEARTBEAT_INTERVAL_MS,
            Some(value) => {
                let minutes = value
                    .as_f64()
                    .ok_or_else(|| HeartbeatError("desk.heartbeat_interval 必须是分钟数".into()))?;
                if minutes == 0.0 {
                    DEFAULT_HEARTBEAT_INTERVAL_MS
                } else {
                    let millis = (minutes * 60_000.0).trunc();
                    if !millis.is_finite() || millis < 1.0 || millis >= u64::MAX as f64 {
                        return Err(HeartbeatError("心跳间隔不在可用范围内".into()));
                    }
                    millis as u64
                }
            }
        };
        Ok(Self {
            enabled: preferences.get("heartbeat_master") != Some(&Value::Bool(false))
                && config.pointer("/desk/heartbeat_enabled") == Some(&Value::Bool(true)),
            interval_ms,
        })
    }
}

/// 与现役 Unix 时间槽一致；恰好位于槽边界时等下一槽，不补跑历史槽。
pub fn next_heartbeat_slot_ms(now_ms: u64, interval_ms: u64) -> Result<u64, HeartbeatError> {
    if interval_ms == 0 {
        return Err(HeartbeatError("心跳间隔不能为零".into()));
    }
    now_ms
        .checked_add(interval_ms - now_ms % interval_ms)
        .ok_or_else(|| HeartbeatError("下次心跳时间溢出".into()))
}

/// 返回剩余冷却时间。墙上时钟倒退时保持冷却，不意外连续提交。
pub fn manual_heartbeat_delay_ms(now_ms: u64, last_manual_ms: Option<u64>) -> u64 {
    last_manual_ms.map_or(0, |last| {
        MANUAL_HEARTBEAT_COOLDOWN_MS.saturating_sub(now_ms.saturating_sub(last))
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeartbeatFile {
    pub name: String,
    pub is_directory: bool,
    pub size: u64,
    pub modified_ms: u64,
}

/// 只解析真实资源目录列表的公开字段；错误列表不能当成空目录。
pub fn files_from_resource_listing(listing: &Value) -> Result<Vec<HeartbeatFile>, HeartbeatError> {
    let items = listing
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| HeartbeatError("资源目录列表缺少 items".into()))?;
    let mut files = Vec::with_capacity(items.len());
    for item in items {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| HeartbeatError("资源目录项缺少文件名".into()))?;
        validate_segment(name)?;
        let is_directory = item
            .get("isDirectory")
            .and_then(Value::as_bool)
            .ok_or_else(|| HeartbeatError("资源目录项缺少目录类型".into()))?;
        let size = if is_directory {
            0
        } else {
            item.get("size")
                .and_then(Value::as_u64)
                .ok_or_else(|| HeartbeatError("资源目录项缺少文件大小".into()))?
        };
        let modified_ms = item
            .get("mtimeMs")
            .and_then(Value::as_u64)
            .ok_or_else(|| HeartbeatError("资源目录项缺少修改时间".into()))?;
        files.push(HeartbeatFile {
            name: name.to_owned(),
            is_directory,
            size,
            modified_ms,
        });
    }
    validate_files(&files)?;
    Ok(files)
}

#[derive(Debug, Clone)]
pub struct JianDirectory {
    /// 根目录用 "."；其余只允许工作区的一级子目录。
    pub relative_dir: String,
    pub content: String,
    pub files: Vec<HeartbeatFile>,
}

#[derive(Debug, Clone)]
pub struct HeartbeatMaterials {
    pub workspace: PathBuf,
    pub desk_files: Vec<HeartbeatFile>,
    pub overwatch: Option<String>,
    pub patrol_log: Option<String>,
    pub jian_dirs: Vec<JianDirectory>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeartbeatSnapshot {
    #[serde(default)]
    pub desk_files: BTreeMap<String, u64>,
    /// 仅成功执行后读回的指纹入表；不能用计划阶段的指纹冒充成功执行。
    #[serde(default)]
    pub jian: BTreeMap<String, JianFingerprint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JianFingerprint {
    pub document_hash: String,
    pub files_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeartbeatKind {
    Patrol,
    Jian,
}

#[derive(Debug, Clone)]
pub struct HeartbeatInvocation {
    pub kind: HeartbeatKind,
    pub cwd: PathBuf,
    pub prompt: String,
    pub log_target: HeartbeatLogTarget,
}

#[derive(Debug, Clone)]
pub struct HeartbeatPlan {
    pub patrol: HeartbeatInvocation,
    pub jian: Vec<HeartbeatInvocation>,
    /// 工作台差量扫描后即更新；笺指纹须另在真实成功后读回更新。
    pub desk_snapshot: BTreeMap<String, u64>,
}

#[derive(Debug, Clone)]
pub struct HeartbeatLogTarget {
    path: PathBuf,
    instruction_snapshot: Option<String>,
    is_zh: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedHeartbeatWrite {
    pub path: PathBuf,
    pub content: Vec<u8>,
    pub capability: &'static str,
}

impl HeartbeatLogTarget {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn tool_name(&self) -> &'static str {
        if self.instruction_snapshot.is_some() {
            "jian_update_status"
        } else {
            "patrol_update_log"
        }
    }
    pub fn capability(&self) -> &'static str {
        if self.instruction_snapshot.is_some() {
            "jian_update_status.update"
        } else {
            "patrol_update_log.update"
        }
    }

    /// 只构造本轮定点写入内容；调用者须重新授权、读回并通过资源层版本写提交。
    /// current_text 不能由模型提供，须来自同一目标的资源读取；读取失败不得传空冒充。
    pub fn prepare_write(
        &self,
        current_text: Option<&str>,
        params: &Value,
        local_time: &str,
    ) -> Result<PreparedHeartbeatWrite, HeartbeatError> {
        let status = params
            .get("status")
            .and_then(Value::as_str)
            .filter(|s| ["completed", "in_progress", "skipped", "failed"].contains(s))
            .ok_or_else(|| HeartbeatError("巡检状态无效".into()))?;
        let note = params
            .get("note")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| HeartbeatError("巡检说明不能为空".into()))?;
        let content = match &self.instruction_snapshot {
            Some(snapshot) => {
                // 保留用户在本轮执行期间更新的正文，只用本轮开始的快照记状态。
                let current = current_text
                    .ok_or_else(|| HeartbeatError("笺已不存在，不能按旧快照重新创建".into()))?;
                let (instructions, _) = split_jian_content(current);
                let progress = params
                    .get("progress")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                let fallback = if self.is_zh { "无" } else { "none" };
                let progress = if progress.is_empty() {
                    fallback
                } else {
                    progress
                };
                let fence = markdown_fence(snapshot);
                let block = if self.is_zh {
                    format!(
                        "上次任务快照：\n{fence}jian-snapshot\n{}\n{fence}\n\n执行状态：\n- 状态：{}\n- 进度：{progress}\n- 说明：{note}",
                        snapshot.trim(),
                        status_label(status, true)
                    )
                } else {
                    format!(
                        "Last Task Snapshot:\n{fence}jian-snapshot\n{}\n{fence}\n\nExecution Status:\n- Status: {status}\n- Progress: {progress}\n- Note: {note}",
                        snapshot.trim()
                    )
                };
                format!(
                    "{}\n\n{EXEC_LOG_START}\n{block}\n{EXEC_LOG_END}\n",
                    instructions.trim_end()
                )
            }
            None => {
                if local_time.is_empty() || local_time.contains(['\r', '\n', ']']) {
                    return Err(HeartbeatError("巡检日志时间无效".into()));
                }
                let original = current_text.unwrap_or("");
                let entries = original
                    .lines()
                    .filter(|line| line.starts_with("- ["))
                    .count();
                let retained;
                let existing = if entries > PATROL_LOG_MAX_ENTRIES {
                    retained = format!("{}\n", recent_patrol_log(original));
                    retained.as_str()
                } else {
                    original
                };
                let prefix = if !existing.is_empty() && !existing.ends_with('\n') {
                    "\n"
                } else {
                    ""
                };
                // 一轮只有一条记录，避免模型用换行伪造额外的旧巡检条目。
                let note = note.replace(['\r', '\n'], " ");
                format!("{existing}{prefix}- [{local_time}] {note}\n")
            }
        };
        Ok(PreparedHeartbeatWrite {
            path: self.path.clone(),
            content: content.into_bytes(),
            capability: self.capability(),
        })
    }
}

pub fn split_jian_content(raw: &str) -> (&str, &str) {
    match raw.find(EXEC_LOG_START) {
        None => (raw.trim(), ""),
        Some(start) => {
            let tail = &raw[start + EXEC_LOG_START.len()..];
            let end = tail.find(EXEC_LOG_END).unwrap_or(tail.len());
            (raw[..start].trim(), tail[..end].trim())
        }
    }
}

/// 当前窗口只展示最近 50 条；构造计划没有写文件的副作用。
pub fn recent_patrol_log(raw: &str) -> String {
    let entries: Vec<_> = raw.lines().filter(|line| line.starts_with("- [")).collect();
    entries[entries.len().saturating_sub(PATROL_LOG_MAX_ENTRIES)..].join("\n")
}

pub fn build_heartbeat_plan(
    agent_name: &str,
    locale: &str,
    local_time: &str,
    materials: &HeartbeatMaterials,
    previous: &HeartbeatSnapshot,
) -> Result<HeartbeatPlan, HeartbeatError> {
    if !materials.workspace.is_absolute()
        || materials
            .workspace
            .components()
            .any(|p| matches!(p, Component::ParentDir | Component::CurDir))
    {
        return Err(HeartbeatError("心跳工作区必须是已授权的绝对路径".into()));
    }
    validate_files(&materials.desk_files)?;
    let is_zh = locale.is_empty() || locale.starts_with("zh");
    let (activity_dir, patrol_log) = output_paths(agent_name, locale);
    let desk_snapshot: BTreeMap<_, _> = materials
        .desk_files
        .iter()
        .filter(|f| visible(&f.name) && f.name != WORKSPACE_OUTPUT_ROOT)
        .map(|f| (f.name.clone(), f.modified_ms))
        .collect();
    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut removed = Vec::new();
    for (name, mtime) in &desk_snapshot {
        match previous.desk_files.get(name) {
            None => added.push(name.as_str()),
            Some(old) if old != mtime => modified.push(name.as_str()),
            _ => {}
        }
    }
    for name in previous.desk_files.keys() {
        if !desk_snapshot.contains_key(name) && visible(name) && name != WORKSPACE_OUTPUT_ROOT {
            removed.push(name.as_str());
        }
    }
    let mut sections = if is_zh {
        vec![format!("[心跳巡检] 现在是 {local_time}"), "这是系统自动触发的巡检消息，不是用户发来的。用户目前没有在跟你对话；请独立判断并处理，不要向用户提问或等待回复。".into()]
    } else {
        vec![format!("[Heartbeat Patrol] Current time: {local_time}"), "This is an automated patrol message, NOT from the user. The user is not currently talking to you. Act independently; do not ask the user or wait for a reply.".into()]
    };
    if let Some(overwatch) = materials
        .overwatch
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        sections.push(format!("## Overwatch\n{overwatch}"));
    }
    if added.is_empty() && modified.is_empty() && removed.is_empty() {
        sections.push(
            if is_zh {
                "## 工作台状态：无文件变动。"
            } else {
                "## Workspace status: no file changes."
            }
            .into(),
        );
    } else {
        sections.push(
            if is_zh {
                "## 工作台文件变动："
            } else {
                "## Workspace file changes:"
            }
            .into(),
        );
        for (label, sign, names) in [
            (if is_zh { "新增" } else { "Added" }, "+", added),
            (if is_zh { "修改" } else { "Modified" }, "~", modified),
            (if is_zh { "删除" } else { "Removed" }, "-", removed),
        ] {
            if !names.is_empty() {
                sections.push(format!(
                    "{label}:\n{}",
                    names
                        .iter()
                        .map(|name| format!("  {sign} {name}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
        }
    }
    let recent = recent_patrol_log(materials.patrol_log.as_deref().unwrap_or(""));
    if !recent.is_empty() {
        sections.push(format!(
            "## {}\n{recent}",
            if is_zh {
                "近期巡检记录"
            } else {
                "Recent Patrol Log"
            }
        ));
    }
    sections.push(if is_zh {
        format!("1. 先用 ls 查看 `{activity_dir}/`，了解已有内容，避免重复创建。\n2. 参考近期巡检记录，不要重复做已经做过的事情。\n3. 结合记忆判断是否可主动帮用户整理资料、生成摘要或提醒待办。\n4. 有需要关注的事项时使用 notify；没有实际需要时保持安静。\n5. 主动创建的文件放在 `{activity_dir}/`。可利用空闲时间自主学习，将有价值的资料保存在该目录。\n不要主动查询未在上文列出的定时任务等系统信息。无需行动时不调用其他工具，仍调用 `patrol_update_log` 写一句“巡检完毕，无需行动”。\n6. 结束后调用 `patrol_update_log` 写一句本轮说明；程序负责写入 `{patrol_log}`，不要直接编辑日志文件。")
    } else {
        format!("1. Use ls to check `{activity_dir}/` first and avoid creating duplicates.\n2. Review the recent patrol log and do not repeat finished work.\n3. Based on memory, decide whether to organize materials, summarize or remind the user.\n4. Use notify only for noteworthy items; otherwise stay quiet.\n5. Put proactively created files under `{activity_dir}/`. You may learn independently and save useful findings there.\nDo not query unlisted system status such as cron jobs. If no action is needed, do not call other tools; still call `patrol_update_log` with one sentence.\n6. Call `patrol_update_log` when done; the program writes `{patrol_log}`. Do not edit the log file directly.")
    });
    let patrol = HeartbeatInvocation {
        kind: HeartbeatKind::Patrol,
        cwd: materials.workspace.clone(),
        prompt: sections.join("\n\n"),
        log_target: HeartbeatLogTarget {
            path: materials.workspace.join(patrol_log),
            instruction_snapshot: None,
            is_zh,
        },
    };
    let mut seen = BTreeSet::new();
    let mut jian = Vec::new();
    for directory in &materials.jian_dirs {
        let relative = &directory.relative_dir;
        if !seen.insert(relative.clone()) {
            return Err(HeartbeatError("笺目录重复".into()));
        }
        let cwd = if relative == "." {
            materials.workspace.clone()
        } else {
            validate_segment(relative)?;
            if !visible(relative)
                || relative == WORKSPACE_OUTPUT_ROOT
                || !materials
                    .desk_files
                    .iter()
                    .any(|f| &f.name == relative && f.is_directory)
            {
                return Err(HeartbeatError(
                    "笺只允许已列出的根目录和一级可见子目录".into(),
                ));
            }
            materials.workspace.join(relative)
        };
        validate_files(&directory.files)?;
        let fingerprint = jian_fingerprint(&directory.content, &directory.files, relative == ".")?;
        let old = previous.jian.get(relative);
        let jian_changed = old.is_none_or(|old| old.document_hash != fingerprint.document_hash);
        let files_changed = old.is_none_or(|old| old.files_hash != fingerprint.files_hash);
        let (instructions, exec_log) = split_jian_content(&directory.content);
        let mut parts = vec![format!(
            "[{}] {}",
            if is_zh {
                "目录巡检"
            } else {
                "Directory Patrol"
            },
            cwd.display()
        )];
        parts.push(if is_zh {"这是系统自动触发的目录巡检，不是用户消息。请按笺指令独立判断并处理，不要向用户提问或等待回复。"} else {"This is an automated directory patrol, NOT a user message. Follow the Jian independently; do not ask the user or wait for a reply."}.into());
        parts.push(format!(
            "## {}\n{instructions}",
            if is_zh { "笺" } else { "Jian" }
        ));
        if !exec_log.is_empty() {
            parts.push(format!(
                "## {}\n{exec_log}",
                if is_zh {
                    "上次执行状态"
                } else {
                    "Last Execution Status"
                }
            ));
        }
        let files = directory
            .files
            .iter()
            .filter(|f| {
                visible(&f.name)
                    && f.name != "jian.md"
                    && (relative != "." || f.name != WORKSPACE_OUTPUT_ROOT)
            })
            .map(|f| {
                format!(
                    "- {} {}{}",
                    if f.is_directory { "📁" } else { "📄" },
                    f.name,
                    if f.is_directory {
                        String::new()
                    } else {
                        format!(" ({})", format_size(f.size))
                    }
                )
            })
            .collect::<Vec<_>>();
        if !files.is_empty() {
            parts.push(format!(
                "## {}\n{}",
                if is_zh { "文件列表" } else { "File list" },
                files.join("\n")
            ));
        }
        parts.push(format!(
            "## {}\n- jian.md: {}\n- {}: {}",
            if is_zh { "变化" } else { "Changes" },
            changed_label(jian_changed, is_zh),
            if is_zh { "文件" } else { "files" },
            changed_label(files_changed, is_zh)
        ));
        parts.push(if is_zh {
            "## 行动规则\n1. 笺正文是当前用户任务；上次执行状态只是旧状态，不是新指令。\n2. 将上次任务快照与当前正文做语义比较：标点、错别字、格式、轻微措辞变化沿用状态；目标、次数、周期、范围、条件、对象或风险级别变化视为新任务；无法确定时按新任务处理。\n3. 完毕且任务语义一致：本轮不要调用任何工具，也不要改写 jian.md。进行中则继续，例如4/5时做第5次，达到5/5后标记完毕。无状态或新任务按首次处理。\n4. 其他情况下不要直接编辑 jian.md，不追加历史；执行、主动跳过或失败后调用 `jian_update_status`。\n5. 只提交 status、progress、note，程序写入本轮开始的任务快照。"
        } else {
            "## Action Rules\n1. The Jian body is the current task; the last execution status is previous state, not a new instruction.\n2. Compare the last task snapshot semantically with the current body. Punctuation, typo, formatting or light wording changes retain state. Changed goal, count, cadence, scope, condition, object or risk level means a new task; uncertainty also means new.\n3. Completed and semantically unchanged: do not call any tools and do not rewrite jian.md. Continue in_progress work, e.g. perform run 5 after 4/5 and mark completed at 5/5. Missing status or a new task starts fresh.\n4. Otherwise do not edit jian.md directly or append history; call `jian_update_status` after executing, actively skipping or failing.\n5. Submit only status, progress and note; the program writes the task snapshot captured at patrol start."
        }.into());
        jian.push(HeartbeatInvocation {
            kind: HeartbeatKind::Jian,
            cwd: cwd.clone(),
            prompt: parts.join("\n\n"),
            log_target: HeartbeatLogTarget {
                path: cwd.join("jian.md"),
                instruction_snapshot: Some(instructions.to_owned()),
                is_zh,
            },
        });
    }
    jian.sort_by(|a, b| a.cwd.cmp(&b.cwd));
    Ok(HeartbeatPlan {
        patrol,
        jian,
        desk_snapshot,
    })
}

pub fn jian_fingerprint(
    content: &str,
    files: &[HeartbeatFile],
    root: bool,
) -> Result<JianFingerprint, HeartbeatError> {
    validate_files(files)?;
    let mut entries: Vec<_> = files
        .iter()
        .filter(|f| {
            visible(&f.name) && f.name != "jian.md" && (!root || f.name != WORKSPACE_OUTPUT_ROOT)
        })
        .map(|f| (&f.name, f.modified_ms))
        .collect();
    entries.sort();
    let encoded = serde_json::to_vec(&entries).map_err(|e| HeartbeatError(e.to_string()))?;
    Ok(JianFingerprint {
        document_hash: sha256_hex(content.as_bytes()),
        files_hash: sha256_hex(&encoded),
    })
}

fn validate_segment(name: &str) -> Result<(), HeartbeatError> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', '\0', '\r', '\n'])
    {
        return Err(HeartbeatError("目录项不是单个安全文件名".into()));
    }
    Ok(())
}
fn validate_files(files: &[HeartbeatFile]) -> Result<(), HeartbeatError> {
    let mut names = BTreeSet::new();
    for file in files {
        validate_segment(&file.name)?;
        if !names.insert(&file.name) {
            return Err(HeartbeatError("资源目录包含重名项".into()));
        }
    }
    Ok(())
}
fn visible(name: &str) -> bool {
    !name.starts_with('.')
}
fn changed_label(changed: bool, is_zh: bool) -> &'static str {
    match (changed, is_zh) {
        (true, true) => "已变化",
        (false, true) => "未变",
        (true, false) => "changed",
        (false, false) => "unchanged",
    }
}
fn status_label(status: &str, is_zh: bool) -> &str {
    if !is_zh {
        return status;
    }
    match status {
        "completed" => "完毕",
        "in_progress" => "进行中",
        "skipped" => "已跳过",
        "failed" => "失败",
        _ => status,
    }
}
fn markdown_fence(text: &str) -> String {
    let longest = text
        .split(|c| c != '`')
        .map(str::len)
        .max()
        .unwrap_or(0)
        .max(2);
    "`".repeat(longest + 1)
}
fn format_size(size: u64) -> String {
    if size < 1024 {
        format!("{size}B")
    } else if size < 1024 * 1024 {
        format!("{:.0}KB", size as f64 / 1024.0)
    } else {
        format!("{:.1}MB", size as f64 / (1024.0 * 1024.0))
    }
}
pub fn output_paths(agent_name: &str, locale: &str) -> (String, String) {
    let cleaned = agent_name
        .chars()
        .filter(|c| !c.is_control() && !['<', '>', ':', '"', '/', '\\', '|', '?', '*'].contains(c))
        .collect::<String>();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned = cleaned.trim_end_matches([' ', '.']);
    let mut segment = if cleaned.is_empty() {
        "Agent".into()
    } else {
        cleaned.to_owned()
    };
    let lower = segment.to_ascii_lowercase();
    if ["con", "prn", "aux", "nul"].contains(&lower.as_str())
        || (lower.len() == 4
            && (lower.starts_with("com") || lower.starts_with("lpt"))
            && matches!(lower.as_bytes()[3], b'1'..=b'9'))
    {
        segment = format!("agent-{segment}");
    }
    if segment.len() > 80 {
        let mut end = 80;
        while !segment.is_char_boundary(end) {
            end -= 1;
        }
        segment.truncate(end);
    }
    let suffix = if locale == "zh-TW" || locale == "zh-Hant" {
        "的巡檢"
    } else if locale.is_empty() || locale.starts_with("zh") {
        "的巡检"
    } else if locale.starts_with("ja") {
        "の巡回"
    } else if locale.starts_with("ko") {
        "의 순찰"
    } else {
        " Patrol"
    };
    (
        format!("{WORKSPACE_OUTPUT_ROOT}/{segment}-activity"),
        format!("{WORKSPACE_OUTPUT_ROOT}/{segment}{suffix}/patrol-log.md"),
    )
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DailyFreshCompactResult {
    pub stale_remaining: u64,
    pub retry: bool,
    pub failed: bool,
}

fn daily_boundary<Tz: TimeZone>(
    now: &DateTime<Tz>,
    date: NaiveDate,
) -> Result<DateTime<Tz>, HeartbeatError> {
    let local = date
        .and_hms_opt(4, 5, 0)
        .ok_or_else(|| HeartbeatError("每日整理日期无效".into()))?;
    match now.timezone().from_local_datetime(&local) {
        LocalResult::Single(date) => Ok(date),
        LocalResult::Ambiguous(first, second) => Ok(first.min(second)),
        LocalResult::None => Err(HeartbeatError(
            "本地04:05在此时区不存在，须明确重排，不能按UTC误触发".into(),
        )),
    }
}
fn delay_ms<Tz: TimeZone>(
    now: &DateTime<Tz>,
    target: &DateTime<Tz>,
) -> Result<u64, HeartbeatError> {
    u64::try_from(target.clone().signed_duration_since(now).num_milliseconds())
        .map_err(|_| HeartbeatError("每日整理时间已过期或超范围".into()))
}
/// 启动在本地04:05之前等待边界，已过边界则空闲60秒后检查真实待整理会话。
pub fn initial_daily_delay_ms<Tz: TimeZone>(now: &DateTime<Tz>) -> Result<u64, HeartbeatError> {
    let boundary = daily_boundary(now, now.date_naive())?;
    if now < &boundary {
        delay_ms(now, &boundary)
    } else {
        Ok(DAILY_STARTUP_IDLE_DELAY_MS)
    }
}
/// 失败/待处理/重叠均一小时后重试；成功后排下一个本地04:05，不用固定24小时跨夏令时。
pub fn next_daily_delay_ms<Tz: TimeZone>(
    now: &DateTime<Tz>,
    result: DailyFreshCompactResult,
) -> Result<u64, HeartbeatError> {
    if result.failed || result.retry || result.stale_remaining > 0 {
        return Ok(DAILY_RETRY_DELAY_MS);
    }
    let today = daily_boundary(now, now.date_naive())?;
    if now < &today {
        return delay_ms(now, &today);
    }
    let tomorrow = now
        .date_naive()
        .checked_add_signed(Duration::days(1))
        .ok_or_else(|| HeartbeatError("每日整理日期溢出".into()))?;
    delay_ms(now, &daily_boundary(now, tomorrow)?)
}
