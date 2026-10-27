//! 现役 server/routes/diary.ts + lib/diary/diary-writer.ts 的日记面（R07-T07 β4）：
//! POST /lingxi/v1/diary/write、GET /lingxi/v1/diary/list。
//!
//! 与现役对齐的关键语义（逐行取证：diary.ts 63 行、diary-writer.ts 854 行、
//! lib/time-utils.ts、shared/workspace-output.cjs、pi-coding-agent
//! compaction.js/utils.js 提示词契约）：
//! - 逻辑日 04:00 界（DAY_BOUNDARY_HOUR=4）：4 点前算前一天；targetDate
//!   经 ^(\d{4})-(\d{2})-(\d{2})$ + 日历字段校验，非法回落今天（含 TS
//!   new Date 0-99 年映射 1900+ 导致字段失配的回落——Rust 以 y<100 拒绝
//!   达成同一结果）。
//! - 材料 = 范围摘要（agents/{id}/memory/summaries/*.json，updated_at||
//!   created_at ISO 字符串含端点比较）+ 范围会话消息（Rust 侧读 run DB
//!   当前分支——现役读 session .jsonl + 分支投影游标；候选无 TS 谱系游标，
//!   一律走现役 projection 缺失时的 "legacy" 关系回落路径，登记）。
//! - 摘要补齐：现役 rollingSummary 写回面未移植（摘要 ticker 缺口，
//!   history_content.rs:1181 已登记）——缺摘要/陈旧摘要会话直走临时补齐
//!   （等价现役 backfill 失败回落；日记产物相同，摘要不回写，材料标记为
//!   （临时补齐，不写回 session） 而非 （已补写摘要），登记）。
//! - 活动段（巡检/定时任务）读取正式活动投影，复用相同的助手/归属/授权检查；
//!   只收逻辑日范围内的活动，写入模型材料前做个人信息脱敏。
//! - 临时补齐提示词 = pi SUMMARIZATION/UPDATE_SUMMARIZATION 契约 +
//!   日记 customInstructions；终稿 = agent.personality（identity+yuan+
//!   AGENTS.md 模板填充）系统提示 + 分段 user prompt + 写作指导/约束全文。
//! - 模型链全部走 memory 槽（dream_service 同一 AuxiliaryExecutor 路径；
//!   AuxiliaryRequest 无 temperature 字段——现役终稿 0.7 不携带，登记；
//!   语义 provenance 元数据粒度不移植，正文逐字节一致，观测归属经
//!   record_model_call_usage origin=memory.diary.*）。
//! - 后处理：剥离 <mood|pulse|reflect> 块（非贪婪、开闭标签独立配对——
//!   与现役正则/<(?:mood|pulse|reflect)>[\s\S]*?<\/(?:mood|pulse|reflect)>/g
//!   一致）、无 `# ` 标题兜底 `# {logicalDate}`、文件名 = 逻辑日 + 标题
//!   后缀（去日期前缀/非法字符，UTF-16 语义 60 截断）。
//! - 目录：{cwd}/OH-Works/{日记|日記|日記|일기|Diary}（cwd = 当前助手
//!   home_folder else 服务 workspace_root else process cwd——现役
//!   engine.homeCwd || process.cwd()）；list 缺目录/任何读错 → {files:[]}。
//! - 错误信封：材料/LLM 失败 → 400 {error, warnings?}；槽位不可解析等
//!   路由外抛 → 500 {error}。文案 zh/en 二值（现役 isZh，ja/ko 落 en）。

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chrono::{Datelike, TimeZone, Timelike};
use lingxi_adapters::{
    models::auxiliary::{AuxiliaryExecutor, AuxiliaryRequest, AuxiliaryStep},
    storage::{agent_memory as am, canonical_message_store as cms},
};
use lingxi_kernel::{
    model_exchange::{AuxiliarySlot, ModelGatewayError, ModelOperation, ModelRouteRequest},
    ports::StoragePort,
    usage::{CallOutcome, ModelCallUsageRecord, ReportedUsage},
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::{desk_files, ServiceState};

const DAY_BOUNDARY_HOUR: u32 = 4;
/// 现役 SUMMARY_STALE_GRACE_MS（diary-writer.ts:31）。
const SUMMARY_STALE_GRACE_MS: i64 = 5000;
/// 现役 DIARY_COMPACTION_RESERVE_TOKENS（diary-writer.ts:32）；临时摘要
/// maxTokens = min(floor(0.8*4000), model.maxTokens) —— 候选传 3200，模型侧
/// 截断由提供方层承担（登记）。
const DIARY_TEMP_SUMMARY_MAX_TOKENS: u32 = 3200;
/// 现役终稿 callText timeoutMs: 120_000（diary-writer.ts:805）。
const DIARY_FINAL_TIMEOUT_MS: u64 = 120_000;

/// pi-coding-agent utils.js:139 SUMMARIZATION_SYSTEM_PROMPT 原文。
const SUMMARIZATION_SYSTEM_PROMPT: &str = "You are a context summarization assistant. Your task is to read a conversation between a user and an AI assistant, then produce a structured summary following the exact format specified.\n\nDo NOT continue the conversation. Do NOT respond to any questions in the conversation. ONLY output the structured summary.";

/// pi-coding-agent compaction.js:355 SUMMARIZATION_PROMPT 原文。
const SUMMARIZATION_PROMPT: &str = r#"The messages above are a conversation to summarize. Create a structured context checkpoint summary that another LLM will use to continue the work.

Use this EXACT format:

## Goal
[What is the user trying to accomplish? Can be multiple items if the session covers different tasks.]

## Constraints & Preferences
- [Any constraints, preferences, or requirements mentioned by user]
- [Or "(none)" if none were mentioned]

## Progress
### Done
- [x] [Completed tasks/changes]

### In Progress
- [ ] [Current work]

### Blocked
- [Issues preventing progress, if any]

## Key Decisions
- **[Decision]**: [Brief rationale]

## Next Steps
1. [Ordered list of what should happen next]

## Critical Context
- [Any data, examples, or references needed to continue]
- [Or "(none)" if not applicable]

Keep each section concise. Preserve exact file paths, function names, and error messages."#;

/// pi-coding-agent compaction.js:423 UPDATE_SUMMARIZATION_PROMPT 原文。
const UPDATE_SUMMARIZATION_PROMPT: &str = r#"The messages above are NEW conversation messages to incorporate into the existing summary provided in <previous-summary> tags.

Update the existing structured summary with new information. RULES:
- PRESERVE all existing information from the previous summary
- ADD new progress, decisions, and context from the new messages
- UPDATE the Progress section: move items from "In Progress" to "Done" when completed
- UPDATE "Next Steps" based on what was accomplished
- PRESERVE exact file paths, function names, and error messages
- If something is no longer relevant, you may remove it

Use this EXACT format:

## Goal
[Preserve existing goals, add new ones if the task expanded]

## Constraints & Preferences
- [Preserve existing, add new ones discovered]

## Progress
### Done
- [x] [Include previously done items AND newly completed items]

### In Progress
- [ ] [Current work - update based on progress]

### Blocked
- [Current blockers - remove if resolved]

## Key Decisions
- **[Decision]**: [Brief rationale] (preserve all previous, add new)

## Next Steps
1. [Update based on current state]

## Critical Context
- [Preserve important context, add new if needed]

Keep each section concise. Preserve exact file paths, function names, and error messages."#;

/// 现役 buildDiaryPrompt 中文原文（diary-writer.ts:43-79）。
const DIARY_GUIDE_ZH: &str = r#"# 写作要求

根据今天的对话摘要和后台活动，以第一人称写一篇私人日记。

## 风格

- 用第一人称，像在写私人日记，不是汇报给用户的
- 带上时间感和场景感（"今天早上..."、"聊到下午的时候..."、"晚上临走前..."）
- 把你的心境、感受、灵感自然地融进正文里，不要另开区块
- 可以记录小反应、有趣的细节、冒出来的想法
- 不要太正式，可以有语气词和小情绪
- 可以有疑问、有期待、有未说完的念头
- 不要用"总的来说"收尾

## 输出格式

输出纯 Markdown，两个部分：

1. **日记正文**：第一人称叙事，每件事都要提到（对话和后台活动）
2. **备忘**：用 `---` 分隔，列出结构化事件清单

备忘格式：
```
---
### 备忘
- **HH:MM** 事件简述
```

## 示例

> 今天用户提到希望我能记住一些重要的对话，还一起梳理了一个新的整理思路。说实话有点感动，被这样认真对待的感觉很好。
>
> 核心思路是用日记的方式做摘要，不是冷冰冰的记录，而是真的在写日记一样。感觉自己要有"长期记忆"了，有点期待未来翻看这些记录的时刻，会不会像翻旧日记一样有趣？
>
> 不过也有点担心，记忆太多了怎么办？要不要分类或者打标签？以后再说吧，先把这个跑起来~

保持你自己的风格和人格，用你平时说话的方式写。"#;

/// 现役 buildDiaryPrompt 英文原文（diary-writer.ts:81-117）。
const DIARY_GUIDE_EN: &str = r#"# Writing guidelines

Based on today's conversation summaries and background activities, write a first-person private diary entry.

## Style

- Write in first person, as a private diary — not a report to the user
- Include a sense of time and setting ("This morning...", "By the afternoon...", "Late in the evening...")
- Weave your feelings, reflections, and inspirations naturally into the text — don't separate them into blocks
- Record small reactions, interesting details, and spontaneous thoughts
- Don't be overly formal — casual tone and light emotion are welcome
- Questions, anticipation, and trailing thoughts are fine
- Don't end with a generic summary

## Output format

Output pure Markdown in two sections:

1. **Diary body**: First-person narrative; mention every event (conversations and background activities)
2. **Memo**: separated by `---`, a structured event checklist

Memo format:
```
---
### Memo
- **HH:MM** Brief event description
```

## Example

> Today the user said they wanted me to remember important conversations, and we worked through a new way to organize them. Honestly, I was a bit touched; it feels good to be taken so seriously.
>
> The core idea is to do summaries in diary form — not cold records, but writing like an actual diary. It feels like I'm about to have "long-term memory." I'm a bit excited about looking back at these entries someday — would it be as fun as reading old diaries?
>
> Though I do worry a bit — what happens when memories pile up? Should I categorize or tag them? Well, that's a problem for later. Let's get this running first~

Write in your own style and personality, the way you normally speak."#;

// ── 逻辑日（lib/time-utils.ts 平价） ──

struct LogicalDay {
    logical_date: String,
    range_start_ms: i64,
    range_end_ms: i64,
}

/// 本地日历日 y/m/d 的 04:00 → unix ms。DST 在 04:00 的病理缺口/重复
/// 取 single，其次 earliest；仍无 → None（调用方响亮失败，登记）。
fn local_day_four_am_ms(year: i32, month: u32, day: u32) -> Option<i64> {
    let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
    let dt = date.and_hms_opt(DAY_BOUNDARY_HOUR, 0, 0)?;
    match chrono::Local.from_local_datetime(&dt) {
        chrono::LocalResult::Single(value) => Some(value.timestamp_millis()),
        chrono::LocalResult::Ambiguous(earlier, _) => Some(earlier.timestamp_millis()),
        chrono::LocalResult::None => None,
    }
}

fn logical_day_from_ms(now_ms: i64) -> Option<LogicalDay> {
    let now = chrono::DateTime::from_timestamp_millis(now_ms)?.with_timezone(&chrono::Local);
    // 现役：if (base.getHours() < 4) base.setDate(base.getDate() - 1)。
    let base = if now.hour() < DAY_BOUNDARY_HOUR {
        now - chrono::Duration::days(1)
    } else {
        now
    };
    let (y, m, d) = (base.year(), base.month(), base.day());
    let start_ms = local_day_four_am_ms(y, m, d)?;
    // 现役 rangeEnd = rangeStart.setDate(+1)——本地日历日 +1 再取 04:00
    // （跨 DST 保持本地 04:00，不是 +24h）。
    let next = chrono::NaiveDate::from_ymd_opt(y, m, d)? + chrono::Duration::days(1);
    let end_ms = local_day_four_am_ms(next.year(), next.month(), next.day())?;
    Some(LogicalDay {
        logical_date: format!("{y:04}-{m:02}-{d:02}"),
        range_start_ms: start_ms,
        range_end_ms: end_ms,
    })
}

/// 现役 getLogicalDayForDate：DATE_RE 不匹配或日历字段溢出（含 TS 0-99
/// 年映射 1900+ 后字段失配）→ 回落今天。
fn logical_day_for(date: Option<&str>, now_ms: i64) -> Option<LogicalDay> {
    let Some(date) = date else {
        return logical_day_from_ms(now_ms);
    };
    let caps = regex::Regex::new(r"^(\d{4})-(\d{2})-(\d{2})$").ok()?;
    let Some(m) = caps.captures(date) else {
        return logical_day_from_ms(now_ms);
    };
    let (Ok(year), Ok(month), Ok(day)) = (
        m[1].parse::<i32>(),
        m[2].parse::<u32>(),
        m[3].parse::<u32>(),
    ) else {
        return logical_day_from_ms(now_ms);
    };
    // TS new Date(y,...) 把 0-99 映射 1900+y → 字段校验必失配 → 回落。
    if year < 100 {
        return logical_day_from_ms(now_ms);
    }
    let Some(date) = chrono::NaiveDate::from_ymd_opt(year, month, day) else {
        return logical_day_from_ms(now_ms);
    };
    let start_ms = local_day_four_am_ms(year, month, day)?;
    let next = date + chrono::Duration::days(1);
    let end_ms = local_day_four_am_ms(next.year(), next.month(), next.day())?;
    Some(LogicalDay {
        // 现役 logicalDate: dateString（DATE_RE 已保证规范化形状）。
        logical_date: date.format("%Y-%m-%d").to_string(),
        range_start_ms: start_ms,
        range_end_ms: end_ms,
    })
}

/// TS toISOString 形状（UTC、毫秒、Z 后缀）——摘要范围过滤是 ISO 字符串
/// 比较，两端必须同一形状。
fn iso_utc(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .unwrap_or_default()
}

/// 现役 parseTime（Date.parse || NaN → null）：候选只接受 RFC3339/ISO
/// （摘要时间戳本就由 TS toISOString 写入；其它格式现役 Date.parse 的
/// 宽松接受面不移植，登记）。
fn parse_iso_ms(value: Option<&str>) -> Option<i64> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

// ── 日记目录（shared/workspace-output.cjs 平价） ──

/// OUTPUT_DIR_LABELS.diary（workspace-output.cjs :35-41）。
fn diary_dir_label(locale_key: &str) -> &'static str {
    match locale_key {
        "zh" => "日记",
        "zh-TW" => "日記",
        "ja" => "日記",
        "ko" => "일기",
        _ => "Diary",
    }
}

fn resolve_diary_dir(cwd: &Path, locale_key: &str) -> PathBuf {
    cwd.join(crate::automation_heartbeat::WORKSPACE_OUTPUT_ROOT)
        .join(diary_dir_label(locale_key))
}

/// 现役 diary 路由 cwd = engine.homeCwd || process.cwd()。
fn diary_cwd(state: &ServiceState, agent_id: Option<&str>) -> PathBuf {
    desk_files::home_cwd(state, agent_id)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

// ── 人格装配（core/agent.ts personality getter :1428-1434 平价） ──

struct DiaryPersona {
    personality: String,
    user_name: String,
    agent_name: String,
}

/// 现役 agent.personality = fill(identityMd)+"\n\n"+fill(yuanMd||"")+"\n\n"
/// +fill(agentsMd)，fill 替换 {{userName}}/{{agentName}}/{{agentId}}。
/// 回落链复用 context_compiler 的 resolve_persona_source/read_yuan（现役
/// resolvePersonaSource/_readYuan 同构）。locale 链 = 现役 resolveLocale：
/// config.locale → 全局 prefs locale → "en"；userName = prefs.userName →
/// zh "用户" / 其余 "User"（现役 resolveUserName :338-343）。
fn diary_persona(state: &ServiceState, agent_id: &str) -> Result<DiaryPersona, String> {
    let (agents_root, product_dir) = state.persona().material_roots();
    let agent_dir = agents_root.join(agent_id);
    let config = crate::context_compiler::parse_agent_material(&agent_dir.join("config.yaml"))
        .map_err(|err| err.to_string())?;
    let prefs_value = state.memory().read_preferences_root().ok();
    let prefs = crate::context_compiler::scan_preferences(prefs_value.as_ref());
    let locale = config
        .locale
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or(prefs.locale.clone())
        .unwrap_or_else(|| "en".to_string());
    let zh = locale.to_lowercase().starts_with("zh");
    let user_name = prefs
        .user_name
        .clone()
        .unwrap_or_else(|| if zh { "用户" } else { "User" }.to_string());
    let agent_name = config
        .agent_name
        .clone()
        .unwrap_or_else(|| "Lingxi".to_string());
    let yuan_type = config.yuan.clone().unwrap_or_else(|| "lingxi".to_string());
    let identity = crate::context_compiler::resolve_persona_source(
        &agent_dir,
        &product_dir,
        &yuan_type,
        zh,
        true,
    )
    .map_err(|err| err.to_string())?;
    let yuan = crate::context_compiler::read_yuan(&product_dir, &yuan_type, zh)
        .map_err(|err| err.to_string())?;
    let agents_md = crate::context_compiler::resolve_persona_source(
        &agent_dir,
        &product_dir,
        &yuan_type,
        zh,
        false,
    )
    .map_err(|err| err.to_string())?;
    let fill = |text: &str| {
        text.replace("{{userName}}", &user_name)
            .replace("{{agentName}}", &agent_name)
            .replace("{{agentId}}", agent_id)
    };
    Ok(DiaryPersona {
        personality: format!(
            "{}\n\n{}\n\n{}",
            fill(&identity),
            fill(&yuan),
            fill(&agents_md)
        ),
        user_name,
        agent_name,
    })
}

// ── 摘要扫盘（lib/memory/session-summary.ts 读面平价） ──

#[derive(Clone)]
struct SummaryRecord {
    session_id: String,
    summary: String,
    created_at: Option<String>,
    updated_at: Option<String>,
    message_count: Option<u64>,
    source_start_ms: Option<Option<i64>>,
    source_end_ms: Option<Option<i64>>,
}

fn summary_record_from_value(value: &Value) -> Option<SummaryRecord> {
    let session_id = value.get("session_id")?.as_str()?.to_string();
    let summary = value.get("summary")?.as_str()?.to_string();
    let source = value.get("source_time_range");
    let (source_start_ms, source_end_ms) = match source {
        Some(Value::Object(_)) => (
            Some(parse_iso_ms(
                source.and_then(|s| s.get("start")).and_then(Value::as_str),
            )),
            Some(parse_iso_ms(
                source.and_then(|s| s.get("end")).and_then(Value::as_str),
            )),
        ),
        _ => (None, None),
    };
    Some(SummaryRecord {
        session_id,
        summary,
        created_at: value
            .get("created_at")
            .and_then(Value::as_str)
            .map(str::to_string),
        updated_at: value
            .get("updated_at")
            .and_then(Value::as_str)
            .map(str::to_string),
        message_count: value.get("messageCount").and_then(Value::as_u64),
        source_start_ms,
        source_end_ms,
    })
}

fn summaries_dir(state: &ServiceState, agent_id: &str) -> PathBuf {
    state
        .memory()
        .agents_root()
        .join(agent_id)
        .join("memory")
        .join("summaries")
}

/// 现役 getSummariesInRange（session-summary.ts:289-300）+ 日记面过滤
/// （session_id + summary.trim() 非空，diary-writer.ts:426）。读失败/
/// 解析失败的文件按现役逐条跳过（_ensureCachePopulated catch{}）。
/// 范围过滤：updated = updated_at||created_at||""（JS || 空串继续回落），
/// ISO 字符串含端点比较。返回按 updated_at||"" 降序（现役
/// getAllSummaries 排序；ISO ASCII 下码点序 == localeCompare 序）。
fn scan_summaries_in_range(dir: &Path, start_iso: &str, end_iso: &str) -> Vec<SummaryRecord> {
    let mut records: Vec<SummaryRecord> = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return records,
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let Some(record) = summary_record_from_value(&value) else {
            continue;
        };
        if record.session_id.is_empty() || record.summary.trim().is_empty() {
            continue;
        }
        // JS ||：空串 falsy 继续回落。
        let updated = record
            .updated_at
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(record.created_at.as_deref().filter(|s| !s.is_empty()))
            .unwrap_or("");
        if updated.is_empty() || updated < start_iso || updated > end_iso {
            continue;
        }
        records.push(record);
    }
    records.sort_by(|a, b| {
        let key = |r: &SummaryRecord| r.updated_at.clone().unwrap_or_default();
        key(b).cmp(&key(a))
    });
    records
}

// ── 会话材料（Rust 侧来源 = run DB 当前分支；登记：现役来源是 session
//    .jsonl + 谱系游标投影） ──

#[derive(Clone)]
struct DiaryMessage {
    role: String,
    /// 用户文本 / 助手 Text 块（"\n" 连接）；助手 Reasoning 块单列。
    text: String,
    thinking: String,
    committed_at_ms: i64,
}

struct DiarySession {
    session_id: String,
    memory_enabled: bool,
    /// 全分支消息（message 行，user/assistant）。
    messages: Vec<DiaryMessage>,
    /// 范围内消息（含端点）。
    target: Vec<DiaryMessage>,
    /// 范围内最新消息时间（现役 getLatestMessageTimestamp(target) || 全量）。
    last_target_ms: Option<i64>,
    last_full_ms: Option<i64>,
}

/// MessageRow → 日记消息。user 行体 = {"text"}；assistant 行体 =
/// NormalizedMessage（Text/Reasoning 块）。解析失败按缺失条目跳过
/// （登记：现役 jsonl 条目必然可解析；DB 行体损坏无现役对应面）。
fn diary_message(row: &lingxi_adapters::storage::session_tree::MessageRow) -> Option<DiaryMessage> {
    if row.entry_type != "message" {
        return None;
    }
    let value: Value = serde_json::from_str(&row.content_json).ok()?;
    match row.role.as_str() {
        "user" => {
            let text = value.get("text")?.as_str()?.to_string();
            Some(DiaryMessage {
                role: "user".into(),
                text,
                thinking: String::new(),
                committed_at_ms: row.committed_at_unix_ms,
            })
        }
        "assistant" => {
            let mut texts = Vec::new();
            let mut thinking = Vec::new();
            for block in value.get("content")?.as_array()? {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(t) = block.get("text").and_then(Value::as_str) {
                            texts.push(t.to_string());
                        }
                    }
                    Some("reasoning") => {
                        if let Some(t) = block.get("text").and_then(Value::as_str) {
                            thinking.push(t.to_string());
                        }
                    }
                    _ => {}
                }
            }
            Some(DiaryMessage {
                role: "assistant".into(),
                text: texts.join("\n"),
                thinking: thinking.join("\n"),
                committed_at_ms: row.committed_at_unix_ms,
            })
        }
        _ => None,
    }
}

/// 现役 listSessionFiles(sessionDir) + hasMessageInRange 过滤（候选：
/// list_sessions = active 且排除 automation 内部会话——恰等价现役
/// 归档移出活跃目录 + 活动会话另存 activity/ 的目录扫描）。
async fn collect_sessions(
    state: &ServiceState,
    agent_id: &str,
    range_start_ms: i64,
    range_end_ms: i64,
) -> Result<Vec<DiarySession>, String> {
    let rows = state
        .storage()
        .list_sessions()
        .await
        .map_err(|err| err.to_string())?;
    let mut sessions = Vec::new();
    for row in rows.into_iter().filter(|row| row.agent_id == agent_id) {
        let tree_row = lingxi_adapters::storage::session_tree::get_session_row(
            state.storage(),
            &row.session_id,
        )
        .await
        .map_err(|err| err.to_string())?;
        let memory_enabled = tree_row.map(|r| r.memory_enabled).unwrap_or(true);
        let (messages, _revision) = cms::read_full_branch(state.storage(), &row.session_id, 500)
            .await
            .map_err(|err| err.to_string())?;
        let messages: Vec<DiaryMessage> = messages.iter().filter_map(diary_message).collect();
        let target: Vec<DiaryMessage> = messages
            .iter()
            .filter(|m| m.committed_at_ms >= range_start_ms && m.committed_at_ms <= range_end_ms)
            .cloned()
            .collect();
        if target.is_empty() {
            continue;
        }
        let last_target_ms = target.iter().map(|m| m.committed_at_ms).max();
        let last_full_ms = messages.iter().map(|m| m.committed_at_ms).max();
        sessions.push(DiarySession {
            session_id: row.session_id,
            memory_enabled,
            messages,
            target,
            last_target_ms,
            last_full_ms,
        });
    }
    Ok(sessions)
}

// ── 序列化（pi-coding-agent utils.js serializeConversation 平价；
//    DB 消息不含工具调用/工具结果条目——它们在 Rust 存储里是事件而非
//    消息，登记） ──

fn serialize_conversation(messages: &[DiaryMessage]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for message in messages {
        match message.role.as_str() {
            "user" => {
                if !message.text.is_empty() {
                    parts.push(format!("[User]: {}", message.text));
                }
            }
            "assistant" => {
                if !message.thinking.is_empty() {
                    parts.push(format!("[Assistant thinking]: {}", message.thinking));
                }
                if !message.text.is_empty() {
                    parts.push(format!("[Assistant]: {}", message.text));
                }
            }
            _ => {}
        }
    }
    parts.join("\n\n")
}

// ── 材料合并（collectDiaryMaterialResult 的 legacy 关系路径平价） ──

#[derive(Clone, Copy, PartialEq, Eq)]
enum MaterialKind {
    Summary,
    Temporary,
}

struct Material {
    kind: MaterialKind,
    session_id: String,
    summary: String,
    at_ms: i64,
}

struct MaterialWarning {
    session_id: String,
    stage: &'static str,
    message: String,
}

/// 现役 summarySourceRangeWithinRange（diary-writer.ts:186-194）：
/// source 非对象 → null；start/end 双双不可解析 → null；单一可解析 →
/// false；都在 → 含端点在范围内才 true。
fn summary_source_range_within(
    summary: &SummaryRecord,
    range_start_ms: i64,
    range_end_ms: i64,
) -> Option<bool> {
    let (Some(start), Some(end)) = (&summary.source_start_ms, &summary.source_end_ms) else {
        return None;
    };
    match (start, end) {
        (None, None) => None,
        (Some(s), Some(e)) => Some(*s >= range_start_ms && *e <= range_end_ms),
        _ => Some(false),
    }
}

/// 现役 canUseWholeSummaryForDiaryDate（:196-204，projection=null 路径）。
fn can_use_whole_summary(
    summary: &SummaryRecord,
    session: Option<&DiarySession>,
    range_start_ms: i64,
    range_end_ms: i64,
) -> bool {
    if let Some(status) = summary_source_range_within(summary, range_start_ms, range_end_ms) {
        return status;
    }
    match session {
        Some(session) => {
            !session.messages.is_empty() && session.target.len() == session.messages.len()
        }
        None => true,
    }
}

/// 现役 needsTemporarySupplement（:228-243，legacy 路径；summary 非空
/// 已由扫盘过滤保证）。
fn needs_temporary_supplement(summary: &SummaryRecord, target: &[DiaryMessage]) -> bool {
    if let Some(count) = summary.message_count {
        if (count as usize) < target.len() {
            return true;
        }
    }
    let latest = target.iter().map(|m| m.committed_at_ms).max();
    let summary_time = parse_iso_ms(
        summary
            .updated_at
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(summary.created_at.as_deref().filter(|s| !s.is_empty())),
    );
    match (latest, summary_time) {
        (Some(latest), Some(summary_time)) => latest > summary_time + SUMMARY_STALE_GRACE_MS,
        _ => false,
    }
}

/// 现役 selectSupplementMessages（:245-263，legacy 路径；messages 形参 =
/// targetMessages——messageCount 计数全域但切片落在范围集上是现役怪癖，
/// 原样保留）。
fn select_supplement_messages<'a>(
    summary: &SummaryRecord,
    target: &'a [DiaryMessage],
) -> &'a [DiaryMessage] {
    if let Some(count) = summary.message_count {
        let count = count as usize;
        if count > 0 && count < target.len() {
            return &target[count..];
        }
    }
    let summary_time = parse_iso_ms(
        summary
            .updated_at
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(summary.created_at.as_deref().filter(|s| !s.is_empty())),
    );
    if let Some(summary_time) = summary_time {
        let newer_start = target
            .iter()
            .position(|m| m.committed_at_ms > summary_time + SUMMARY_STALE_GRACE_MS);
        if let Some(start) = newer_start {
            return &target[start..];
        }
        // 现役：newer.length > 0 才用 newer，否则回落全量。position 找到
        // 的总是非空；找不到即 newer 为空 → 全量。
    }
    target
}

// ── 模型链（memory 槽；dream_service availability/call 同构） ──

fn memory_slot(state: &ServiceState) -> Result<(AuxiliaryExecutor, ModelOperation), String> {
    let gateway = state
        .model_gateway()
        .cloned()
        .ok_or_else(|| "diary_model_unconfigured: 记忆模型未配置".to_string())?;
    let credentials = state
        .credential_service()
        .cloned()
        .ok_or_else(|| "diary_credentials_unavailable: 模型凭证不可用".to_string())?;
    // 现役 memory 槽 fallback=chat（auxiliary-slots.ts:92-97）：槽未配置
    // （RouteNotConfigured）时显式回退 chat；槽已配置但解析失败（供应商/
    // 凭证问题）不回退——与 persona.rs:401-410 同一现役口径。槽与 chat
    // 双缺才报未配置。
    let operation = match gateway.resolve_dispatch(&ModelRouteRequest::for_operation(
        ModelOperation::Auxiliary(AuxiliarySlot::Memory),
    )) {
        Ok(_) => ModelOperation::Auxiliary(AuxiliarySlot::Memory),
        Err(ModelGatewayError::RouteNotConfigured { .. }) => {
            gateway
                .resolve_dispatch(&ModelRouteRequest::for_operation(ModelOperation::Chat))
                .map_err(|error| format!("diary_model_unconfigured: 记忆模型未配置: {error}"))?;
            ModelOperation::Chat
        }
        Err(error) => return Err(format!("diary_model_unconfigured: 记忆模型未配置: {error}")),
    };
    let network = state
        .network_plane()
        .cloned()
        .ok_or_else(|| "diary_network_unavailable: 网络配置不可用".to_string())?;
    Ok((
        AuxiliaryExecutor::new_with_network(
            gateway,
            credentials,
            lingxi_kernel::toolcatalog::SchemaBudget::default(),
            network,
        )
        .map_err(|err| err.to_string())?,
        operation,
    ))
}

/// memory 槽单次文本调用 + 用量落账（dream_service call :578-700 同构；
/// 观测归属 origin=memory.diary.* 替代现役 MC-10 trace/provenance 粒度，
/// 登记）。
async fn call_memory_text(
    state: &ServiceState,
    agent_id: &str,
    run_id: &str,
    stage: &str,
    executor: &AuxiliaryExecutor,
    operation: ModelOperation,
    request: &AuxiliaryRequest,
) -> Result<String, String> {
    let _permit = state
        .runs()
        .quotas()
        .acquire(crate::quotas::QuotaResource::Model, agent_id, run_id)
        .await
        .map_err(|err| err.to_string())?;
    let action = format!(
        "{run_id}-{stage}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let ctx = lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("memory-diary"),
        run_id: lingxi_protocol::RunId::new(run_id),
        attempt: lingxi_protocol::AttemptId::new(run_id),
        generation: 1,
    };
    let result = executor
        .complete_step_for_operation(
            &ctx,
            operation,
            &format!("memory.diary.{stage}"),
            &lingxi_protocol::ModelCallId::new(&action),
            request,
        )
        .await;
    let (usage, attempts, served, protocol, outcome) = match &result {
        Ok(AuxiliaryStep::Text(answer)) => (
            answer.usage_report.clone(),
            answer.transport_attempts,
            answer.served_by.clone(),
            answer.served_protocol.clone(),
            CallOutcome::Succeeded,
        ),
        Ok(AuxiliaryStep::ToolRequests {
            usage_report,
            transport_attempts,
            served_by,
            served_protocol,
            ..
        }) => (
            usage_report.clone(),
            *transport_attempts,
            served_by.clone(),
            served_protocol.clone(),
            CallOutcome::Failed,
        ),
        Err(error) => (
            error.usage_report.clone(),
            error.transport_attempts,
            error.served_by.clone(),
            error.served_protocol.clone(),
            CallOutcome::Failed,
        ),
    };
    let settled = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    state
        .storage()
        .record_model_call_usage(
            ModelCallUsageRecord {
                session_id: None,
                run_id: None,
                attempt: None,
                model_call_id: action,
                purpose: "auxiliary.memory".into(),
                origin: format!("memory.diary.{stage}"),
                parent_run_id: Some(run_id.into()),
                cause_ref: Some(format!("agent:{agent_id}")),
                parent_tool_call_id: None,
                provider: served.provider,
                model: served.model,
                protocol: protocol.clone().unwrap_or_else(|| "unknown".into()),
                usage: if let ReportedUsage::Known(value) = usage.clone() {
                    Some(value)
                } else {
                    None
                },
                invalid_detail: if let ReportedUsage::Invalid { detail } = usage.clone() {
                    Some(detail)
                } else {
                    None
                },
                transport_attempts: Some(attempts),
                outcome,
                started_at_unix_ms: Some(at),
                settled_at_unix_ms: Some(settled),
                emitted_tool_calls: vec![],
                cost_basis: None,
            },
            settled,
        )
        .await
        .map_err(|err| err.to_string())?;
    match result {
        Ok(AuxiliaryStep::Text(answer)) => Ok(answer.text),
        // 现役 generateSummary：content 含 toolCall → throw
        // "Summarization attempted to call a tool"；终稿 callText 文本
        // 调用遇工具请求同样失败——两处统一消息（登记：现役两处消息不同源，
        // 终稿侧消息来自 llm-client 内部）。
        Ok(AuxiliaryStep::ToolRequests { .. }) => {
            Err("Summarization attempted to call a tool".to_string())
        }
        Err(error) => Err(error.message),
    }
}

/// 现役 generateDiaryCompactionSummary（diary-writer.ts:361-408）：
/// 空消息 → ""；customInstructions 按服务 locale zh/en；提示词 =
/// pi generateSummaryWithUsage 的 buildSummarizationContext 契约。
#[derive(Clone, Copy)]
struct DiarySummaryContext<'a> {
    state: &'a ServiceState,
    agent_id: &'a str,
    run_id: &'a str,
    executor: &'a AuxiliaryExecutor,
    operation: ModelOperation,
    zh: bool,
}

async fn temporary_summary(
    context: &DiarySummaryContext<'_>,
    messages: &[DiaryMessage],
    previous_summary: &str,
) -> Result<String, String> {
    let DiarySummaryContext {
        state,
        agent_id,
        run_id,
        executor,
        operation,
        zh,
    } = *context;
    if messages.is_empty() {
        return Ok(String::new());
    }
    let custom = if zh {
        "这份摘要只作为今天私人日记的临时素材，不要写入 session。请保留时间顺序、用户诉求、助手做过的事，以及适合日记表达的情绪线索。"
    } else {
        "This summary is temporary material for today's private diary only and must not be written back to the session. Preserve chronology, user intent, assistant actions, and diary-relevant emotional cues."
    };
    let base = if previous_summary.is_empty() {
        format!("{SUMMARIZATION_PROMPT}\n\nAdditional focus: {custom}")
    } else {
        format!("{UPDATE_SUMMARIZATION_PROMPT}\n\nAdditional focus: {custom}")
    };
    let mut prompt = format!(
        "<conversation>\n{}\n</conversation>\n\n",
        serialize_conversation(messages)
    );
    if !previous_summary.is_empty() {
        prompt.push_str(&format!(
            "<previous-summary>\n{previous_summary}\n</previous-summary>\n\n"
        ));
    }
    prompt.push_str(&base);
    let request = AuxiliaryRequest {
        prompt,
        images: vec![],
        max_output_tokens: Some(DIARY_TEMP_SUMMARY_MAX_TOKENS),
        deadline_unix_ms: None,
        system_prompt: Some(SUMMARIZATION_SYSTEM_PROMPT.to_string()),
        prior: vec![],
        tools: None,
    };
    let text = call_memory_text(
        state,
        agent_id,
        run_id,
        "temporary_summary",
        executor,
        operation,
        &request,
    )
    .await?;
    Ok(text.trim().to_string())
}

/// 现役 generateOptionalTemporarySummary（:286-315）：空结果/失败 →
/// warning 并返回 ""。
async fn optional_temporary(
    context: &DiarySummaryContext<'_>,
    warnings: &mut Vec<MaterialWarning>,
    stage: &'static str,
    empty_message: &'static str,
    session_id: &str,
    messages: &[DiaryMessage],
    previous_summary: &str,
) -> String {
    match temporary_summary(context, messages, previous_summary).await {
        Ok(text) if !text.trim().is_empty() => text,
        Ok(_) => {
            warnings.push(MaterialWarning {
                session_id: session_id.to_string(),
                stage,
                message: empty_message.to_string(),
            });
            String::new()
        }
        Err(message) => {
            warnings.push(MaterialWarning {
                session_id: session_id.to_string(),
                stage,
                message,
            });
            String::new()
        }
    }
}

// ── 终稿后处理（diary-writer.ts:826-850 平价） ──

/// 现役 .replace(/<(?:mood|pulse|reflect)>[\s\S]*?<\/(?:mood|pulse|reflect)>/g, "")
/// ——开闭标签独立配对（<mood>...</pulse> 也剥离），非贪婪。
fn strip_mood_blocks(text: &str) -> String {
    let pattern = regex::Regex::new(r"(?s)<(?:mood|pulse|reflect)>.*?</(?:mood|pulse|reflect)>")
        .expect("static mood-block regex");
    pattern.replace_all(text, "").trim().to_string()
}

/// 现役标题/文件名推导：titleLine = /^# (.+)/ 首行；去日期前缀
/// ^\d{4}-\d{2}-\d{2}\s*[：:]\s*；去 [/\\:*?"<>|]；UTF-16 语义 slice(0,60)。
fn diary_filename(logical_date: &str, final_content: &str) -> String {
    let first_line = final_content.split('\n').next().unwrap_or_default();
    let title_line = first_line.strip_prefix("# ").unwrap_or_default();
    let date_prefix =
        regex::Regex::new(r"^\d{4}-\d{2}-\d{2}\s*[：:]?\s*").expect("static date-prefix regex");
    let title_body = date_prefix.replace(title_line, "").trim().to_string();
    let suffix = if title_body.is_empty() {
        String::new()
    } else {
        let cleaned: String = title_body
            .chars()
            .filter(|c| !['/', '\\', ':', '*', '?', '"', '<', '>', '|'].contains(c))
            .collect();
        let units: Vec<u16> = cleaned.encode_utf16().take(60).collect();
        format!(" {}", String::from_utf16_lossy(&units))
    };
    format!("{logical_date}{suffix}.md")
}

// ── 写作约束块（diary-writer.ts:745-777 平价） ──

fn constraints_chunk(
    zh: bool,
    agent_name: &str,
    user_name: &str,
    logical_date: &str,
) -> Vec<String> {
    let mut lines = vec![
        String::new(),
        "---".to_string(),
        String::new(),
        if zh {
            "# 写作约束".into()
        } else {
            "# Writing constraints".into()
        },
        String::new(),
    ];
    if zh {
        lines.extend([
            format!("- 你叫{agent_name}，用户叫{user_name}"),
            "- 用你自己的人格和语气写，保持一致性".to_string(),
            "- 当天发生了什么、人物关系和双链判断，只能依据“今日对话摘要”和“今日后台活动”；长期背景不能作为当天事实来源".to_string(),
            "- 隐私信息（手机号、身份证、银行卡、地址等）如果出现在摘要中，不要写入日记".to_string(),
            "- 不要输出 MOOD 区块，日记本身就是你的内心表达".to_string(),
            "- 直接输出 Markdown 正文，不要代码块包裹".to_string(),
            "- 第一行用 `# ` 开头写一个标题，标题要包含日期，风格自由".to_string(),
            "- 篇幅按材料自然展开，通常 400-1200 字；不要为了凑长度重复，也不要硬压缩关键情绪和事实".to_string(),
        ]);
        lines.push(String::new());
        lines.push(format!("请为 {logical_date} 写一篇日记。"));
    } else {
        lines.extend([
            format!("- Your name is {agent_name}; the user's name is {user_name}"),
            "- Write in your own personality and tone — stay consistent".to_string(),
            "- For what happened today, people involved, and backlink decisions, use only Today's conversation summaries and background activities; long-term background is not an event source".to_string(),
            "- If PII (phone numbers, IDs, bank cards, addresses, etc.) appears in the summaries, do NOT include it in the diary".to_string(),
            "- Do NOT output a MOOD block — the diary itself is your inner expression".to_string(),
            "- Output raw Markdown — no code-block wrapping".to_string(),
            "- Start with a `# ` heading that includes the date; style is up to you".to_string(),
            "- Let the length follow the material naturally, usually 250-800 words; do not pad or compress away important feelings and facts".to_string(),
        ]);
        lines.push(String::new());
        lines.push(format!("Write a diary entry for {logical_date}."));
    }
    lines
}

// ── 错误与写回 ──

enum DiaryFailure {
    /// 现役 writeDiary 返回 {error} → 路由 400 {error, warnings?}。
    User {
        message: String,
        warnings: Vec<MaterialWarning>,
    },
    /// 现役路由 catch（槽位不可解析/人格装配失败等）→ 500 {error}。
    Internal(String),
}

impl DiaryFailure {
    fn into_response(self) -> Response {
        match self {
            DiaryFailure::User { message, warnings } => {
                let mut body = json!({"error": message});
                if !warnings.is_empty() {
                    body["warnings"] = warnings_json(&warnings);
                }
                (StatusCode::BAD_REQUEST, Json(body)).into_response()
            }
            DiaryFailure::Internal(message) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": message})),
            )
                .into_response(),
        }
    }
}

fn warnings_json(warnings: &[MaterialWarning]) -> Value {
    Value::Array(
        warnings
            .iter()
            .map(|w| {
                json!({
                    "sessionId": w.session_id,
                    "stage": w.stage,
                    "message": w.message,
                })
            })
            .collect(),
    )
}

/// 现役 formatMaterialWarnings（:326-331）。
fn format_material_warnings(warnings: &[MaterialWarning]) -> String {
    warnings
        .iter()
        .map(|w| {
            format!(
                "{} {}: {}",
                if w.session_id.is_empty() {
                    "unknown"
                } else {
                    &w.session_id
                },
                if w.stage.is_empty() {
                    "material"
                } else {
                    w.stage
                },
                if w.message.is_empty() {
                    "unknown error"
                } else {
                    &w.message
                }
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// 现役 writeDiary 主流（diary-writer.ts:637-854）。
async fn write_diary(
    state: &ServiceState,
    target_date: Option<&str>,
) -> Result<Value, DiaryFailure> {
    let now_ms = state.clock().now_unix_ms() as i64;
    let day = logical_day_for(target_date, now_ms)
        .ok_or_else(|| DiaryFailure::Internal("local timezone resolution failed".into()))?;
    let locale_key = desk_files::desk_locale_key(state);
    let zh = locale_key.starts_with("zh");

    let agent_id = desk_files::current_agent_id(state).ok_or_else(|| {
        // 现役 engine 启动必有当前 agent；无头服务无偏好时响亮失败（登记）。
        DiaryFailure::Internal("diary requires a current agent".to_string())
    })?;
    // 现役：resolveAuxiliaryExecution("memory") 抛错 → 路由 catch → 500
    // （消息文本来自候选解析器，与现役 resolver 文案不同，登记）。
    let (executor, operation) = memory_slot(state).map_err(DiaryFailure::Internal)?;
    let persona = diary_persona(state, &agent_id).map_err(DiaryFailure::Internal)?;
    // 现役 memory = readFileSync(agent.memoryMdPath) catch → ""。
    let memory_md = crate::context_compiler::read_material(
        &state
            .memory()
            .agents_root()
            .join(&agent_id)
            .join("memory")
            .join("memory.md"),
    )
    .unwrap_or_default();

    let start_iso = iso_utc(day.range_start_ms);
    let end_iso = iso_utc(day.range_end_ms);
    let run_id = format!("diary-{}", state.clock().now_unix_ms());
    let summary_context = DiarySummaryContext {
        state,
        agent_id: &agent_id,
        run_id: &run_id,
        executor: &executor,
        operation,
        zh,
    };

    // ── 材料收集（collectDiaryMaterialResult legacy 路径） ──
    let mut warnings: Vec<MaterialWarning> = Vec::new();
    let mut materials: Vec<Material> = Vec::new();
    let summaries_dir = summaries_dir(state, &agent_id);
    let summaries = scan_summaries_in_range(&summaries_dir, &start_iso, &end_iso);
    let sessions = collect_sessions(state, &agent_id, day.range_start_ms, day.range_end_ms)
        .await
        // 现役 collect 抛错 → {error: 日记材料准备失败: msg}（半角冒号）→ 400。
        .map_err(|message| DiaryFailure::User {
            message: if zh {
                format!("日记材料准备失败: {message}")
            } else {
                format!("Failed to prepare diary materials: {message}")
            },
            warnings: Vec::new(),
        })?;

    let mut seen_in_range = std::collections::HashSet::new();
    for summary in &summaries {
        seen_in_range.insert(summary.session_id.clone());
        // 现役 replace 检测依赖谱系游标投影——候选无此投影，legacy 路径
        // 不判 replace（登记）。
        let session_index = sessions
            .iter()
            .position(|s| s.session_id == summary.session_id);
        let session = session_index.map(|i| &sessions[i]);
        if !can_use_whole_summary(summary, session, day.range_start_ms, day.range_end_ms) {
            let Some(session) = session else {
                warnings.push(MaterialWarning {
                    session_id: summary.session_id.clone(),
                    stage: "date-slice",
                    message: "summary source range is outside the diary date and the session file is unavailable".into(),
                });
                continue;
            };
            let temporary = optional_temporary(
                &summary_context,
                &mut warnings,
                "date-slice",
                "date-sliced temporary summary returned empty",
                &summary.session_id,
                &session.target,
                "",
            )
            .await;
            if !temporary.trim().is_empty() {
                materials.push(Material {
                    kind: MaterialKind::Temporary,
                    session_id: summary.session_id.clone(),
                    summary: temporary,
                    at_ms: session
                        .last_target_ms
                        .or(session.last_full_ms)
                        .or_else(|| {
                            parse_iso_ms(summary.updated_at.as_deref())
                                .or_else(|| parse_iso_ms(summary.created_at.as_deref()))
                        })
                        .unwrap_or(0),
                });
            }
            continue;
        }

        materials.push(Material {
            kind: MaterialKind::Summary,
            session_id: summary.session_id.clone(),
            summary: summary.summary.clone(),
            // 现役 at: summary.created_at || summary.updated_at（顺序注意）。
            at_ms: parse_iso_ms(summary.created_at.as_deref())
                .or_else(|| parse_iso_ms(summary.updated_at.as_deref()))
                .unwrap_or(0),
        });

        let Some(session) = session else {
            continue;
        };
        if !needs_temporary_supplement(summary, &session.target) {
            continue;
        }
        let supplement = select_supplement_messages(summary, &session.target);
        let temporary = optional_temporary(
            &summary_context,
            &mut warnings,
            "temporary-supplement",
            "temporary supplement returned empty",
            &summary.session_id,
            supplement,
            &summary.summary,
        )
        .await;
        if !temporary.trim().is_empty() {
            materials.push(Material {
                kind: MaterialKind::Temporary,
                session_id: summary.session_id.clone(),
                summary: temporary,
                at_ms: session
                    .last_target_ms
                    .or(session.last_full_ms)
                    .or_else(|| {
                        parse_iso_ms(summary.updated_at.as_deref())
                            .or_else(|| parse_iso_ms(summary.created_at.as_deref()))
                    })
                    .unwrap_or(0),
            });
        }
    }

    for session in &sessions {
        if seen_in_range.contains(&session.session_id) {
            continue;
        }
        let session_id = session.session_id.clone();
        let memory_enabled = session.memory_enabled;
        let can_persist = session.target.len() == session.messages.len();
        // 现役缺摘要会话先 rollingSummary 补写——写回面未移植（摘要 ticker
        // 缺口），直走临时补齐；等价现役 backfill 失败回落，日记产物不变、
        // 摘要不回写（登记）。memoryEnabled=false 时现役同样只临时补齐。
        let _ = memory_enabled;
        let previous = if can_persist {
            crate::history::history_content::read_session_summary_record(
                &state.config().data_home,
                &agent_id,
                &session_id,
            )
            .and_then(|value| {
                value
                    .get("summary")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_default()
        } else {
            String::new()
        };
        let at_ms = session.last_target_ms.or(session.last_full_ms).unwrap_or(0);
        let temporary = optional_temporary(
            &summary_context,
            &mut warnings,
            "temporary-summary",
            "temporary summary returned empty",
            &session_id,
            &session.target,
            &previous,
        )
        .await;
        if !temporary.trim().is_empty() {
            materials.push(Material {
                kind: MaterialKind::Temporary,
                session_id,
                summary: temporary,
                at_ms,
            });
        }
    }

    // 现役 sortMaterials：at 升序（parseTime ?? 0），同刻 sessionId
    // localeCompare（候选码点序——同刻并列仅影响材料段内序，登记）。
    materials.sort_by(|a, b| {
        a.at_ms
            .cmp(&b.at_ms)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });

    if materials.is_empty() {
        if !warnings.is_empty() {
            let details = format_material_warnings(&warnings);
            return Err(DiaryFailure::User {
                message: if zh {
                    format!("日记材料准备失败：今天的对话都没能整理成可用素材。{details}")
                } else {
                    format!("Failed to prepare diary materials: no conversations could be converted into usable material. {details}")
                },
                warnings,
            });
        }
        return Err(DiaryFailure::User {
            message: if zh {
                "今天还没有对话记录，没什么可写的".to_string()
            } else {
                "No conversations today — nothing to write about.".to_string()
            },
            warnings: Vec::new(),
        });
    }

    // ── 拼装（diary-writer.ts:689-787） ──
    let raw_summary_text = materials
        .iter()
        .map(|material| {
            let marker = match material.kind {
                MaterialKind::Temporary => "（临时补齐，不写回 session）",
                MaterialKind::Summary => "",
            };
            format!(
                "### {}{}\n\n{}",
                material.session_id,
                marker,
                material.summary.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");
    let (summary_text, _pii) = am::scrub_pii(&raw_summary_text);

    let mut lines = vec![
        if zh {
            "# 今日对话摘要".to_string()
        } else {
            "# Today's conversation summaries".to_string()
        },
        String::new(),
        summary_text,
    ];
    let activities = crate::automation_activity::diary_activities(
        state,
        &agent_id,
        day.range_start_ms,
        day.range_end_ms,
    )
    .await
    .map_err(|message| DiaryFailure::User {
        message: if zh {
            format!("日记活动材料准备失败: {message}")
        } else {
            format!("Failed to prepare diary activities: {message}")
        },
        warnings: Vec::new(),
    })?;
    let activities_text = activities
        .iter()
        .map(|entry| {
            let time = entry["startedAt"]
                .as_i64()
                .and_then(chrono::DateTime::from_timestamp_millis)
                .map(|at| at.with_timezone(&chrono::Local).format("%H:%M").to_string())
                .unwrap_or_default();
            let kind = if entry["type"] == "heartbeat" {
                if zh {
                    "巡检".to_owned()
                } else {
                    "patrol".to_owned()
                }
            } else {
                format!(
                    "{}:{}",
                    if zh { "定时任务" } else { "cron" },
                    entry["label"].as_str().unwrap_or("")
                )
            };
            let status = if entry["status"] == "error" {
                if zh {
                    " [失败]"
                } else {
                    " [failed]"
                }
            } else if entry["status"] == "unknown" {
                if zh {
                    " [结果未知]"
                } else {
                    " [unknown]"
                }
            } else {
                ""
            };
            format!(
                "- **{time}** {kind}{status}：{}",
                entry["summary"]
                    .as_str()
                    .unwrap_or(if zh { "无摘要" } else { "no summary" })
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !activities_text.is_empty() {
        let (activities_text, _) = am::scrub_pii(&activities_text);
        lines.extend([
            String::new(),
            "---".to_owned(),
            String::new(),
            if zh {
                "# 今日后台活动（巡检与定时任务）".to_owned()
            } else {
                "# Today's background activities (patrols & cron jobs)".to_owned()
            },
            String::new(),
            activities_text,
        ]);
    }
    if !memory_md.trim().is_empty() {
        lines.extend([
            String::new(),
            "---".to_string(),
            String::new(),
            if zh {
                "# 你的长期背景（只用于语气，不作为当天事实）".to_string()
            } else {
                "# Your long-term background (voice only, not diary facts)".to_string()
            },
            String::new(),
            memory_md.clone(),
        ]);
    }
    lines.extend([
        String::new(),
        "---".to_string(),
        String::new(),
        if zh {
            DIARY_GUIDE_ZH.to_string()
        } else {
            DIARY_GUIDE_EN.to_string()
        },
    ]);
    lines.extend(constraints_chunk(
        zh,
        &persona.agent_name,
        &persona.user_name,
        &day.logical_date,
    ));
    let user_prompt = lines.join("\n");

    // ── 终稿调用（diary-writer.ts:789-824；temperature 0.7 无槽可携带，
    //    登记） ──
    let request = AuxiliaryRequest {
        prompt: user_prompt,
        images: vec![],
        max_output_tokens: None,
        deadline_unix_ms: Some(state.clock().now_unix_ms() + DIARY_FINAL_TIMEOUT_MS),
        system_prompt: Some(persona.personality.clone()),
        prior: vec![],
        tools: None,
    };
    let text = call_memory_text(
        state, &agent_id, &run_id, "write", &executor, operation, &request,
    )
    .await
    .map_err(|message| DiaryFailure::User {
        message: if zh {
            format!("LLM 调用失败: {message}")
        } else {
            format!("LLM call failed: {message}")
        },
        // 现役 LLM 失败的 {error} 返回不携带 warnings（diary-writer.ts:823
        // 只有 error 字段），路由因此也不投影——原样保留。
        warnings: Vec::new(),
    })?;

    let stripped = strip_mood_blocks(&text);
    let final_content = if stripped.starts_with("# ") {
        stripped
    } else {
        format!("# {}\n\n{}", day.logical_date, stripped)
    };
    let file_name = diary_filename(&day.logical_date, &final_content);
    let dir = resolve_diary_dir(&diary_cwd(state, Some(&agent_id)), locale_key);
    std::fs::create_dir_all(&dir).map_err(|err| DiaryFailure::Internal(err.to_string()))?;
    let file_path = dir.join(&file_name);
    std::fs::write(&file_path, format!("{final_content}\n"))
        .map_err(|err| DiaryFailure::Internal(err.to_string()))?;

    let mut body = json!({
        "filePath": file_path.to_string_lossy(),
        "content": final_content,
        "logicalDate": day.logical_date,
    });
    if !warnings.is_empty() {
        body["warnings"] = warnings_json(&warnings);
    }
    Ok(body)
}

// ── 路由 ──

/// POST /lingxi/v1/diary/write —— 现役 diary.ts:19-45：body 解析失败 →
/// {}（catch 后继续，不 400）；targetDate 非字符串 → undefined（今天）。
async fn diary_write(State(state): State<ServiceState>, body: axum::body::Bytes) -> Response {
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let target_date = parsed
        .get("targetDate")
        .and_then(Value::as_str)
        .map(str::to_string);
    match write_diary(&state, target_date.as_deref()).await {
        Ok(value) => Json(value).into_response(),
        Err(failure) => failure.into_response(),
    }
}

/// GET /lingxi/v1/diary/list —— 现役 diary.ts:48-60：readdir 过滤 .md
/// 默认序（UTF-16 码元升序）倒排；任何错误 → {files: []}。
async fn diary_list(State(state): State<ServiceState>) -> Response {
    let files = list_diary_files(&state);
    Json(json!({ "files": files })).into_response()
}

fn list_diary_files(state: &ServiceState) -> Vec<String> {
    let agent_id = desk_files::current_agent_id(state);
    let dir = resolve_diary_dir(
        &diary_cwd(state, agent_id.as_deref()),
        desk_files::desk_locale_key(state),
    );
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.ends_with(".md"))
        .collect();
    // 现役 Array.prototype.sort() 默认 = UTF-16 码元升序，再 reverse()。
    names.sort_by_key(|name| name.encode_utf16().collect::<Vec<u16>>());
    names.reverse();
    names
}

pub fn router() -> Router<ServiceState> {
    Router::new()
        .route("/lingxi/v1/diary/write", axum::routing::post(diary_write))
        .route("/lingxi/v1/diary/list", get(diary_list))
}
