//! R08-T01：现役 `GET/PUT /api/preferences/workspace-ui-state` 端口。
//!
//! 现役来源（逐行对齐）：
//! - server/routes/preferences.ts:490-522 —— GET/PUT 路由（400 同文案、safeJson
//!   体解析、surface 校验、响应包装 `{state}` / `{ok:true,state}`）。
//! - core/engine.ts:3036 —— 引擎绑定 `getWorkspaceUiState → _prefs.getWorkspaceUiState`，
//!   即现役 GET 会经过 preferences-manager 的 GC 调用。
//! - core/preferences-manager.ts:668-699 —— getWorkspaceUiState（669-672：先
//!   normalizeWorkspacePath，随后 gcWorkspaceUiState()，再读条目）、
//!   setWorkspaceUiState（679-687：upsert 落盘后回读条目）。
//! - shared/workspace-ui-state.ts —— 全部规范化（cleanString/normalizeRelativePath/
//!   normalizePreviewSourceRootPath/uniqueRelativePaths/normalizePreviewTab/
//!   normalizeWorkspaceUiSurface/normalizeRightWorkspaceTab/normalizeWorkspaceUiEntry/
//!   normalizeWorkspaceUiRecord/normalizeWorkspaceUiState/getWorkspaceUiStateEntry/
//!   upsertWorkspaceUiState）与常量（VERSION=2、MAX_WORKSPACES=50、MAX_PATHS=256、
//!   MAX_TABS=32、MAX_STRING=1024、surfaces=["electron","pwa"]）。
//! - shared/workspace-history.ts:3-11 —— normalizeWorkspacePath。
//! - shared/preview-reading-position.ts —— readingPosition 规范化
//!   （MAX_STRING=512、MAX_SCROLL=10_000_000、scrollLeft/scrollHeight/clientHeight
//!   仅 truthy 才保留、ratio 0..1、updatedAt 0..2^53-1）。
//! - shared/workspace-persistence-gc.ts:51-63 —— pruneMissingWorkspaceUiState
//!   （ENOENT/ENOTDIR 判定 missing 并裁剪，其余错误保留，changed 才落盘）。
//!
//! 端口决策（均非静默降级，逐项标注）：
//! 1. GC 忠实移植：现役 GET 必经 gcWorkspaceUiState（core/engine.ts:3036 →
//!    preferences-manager.ts:671），逐 workspace 键做 fs stat；NotFound(ENOENT)/
//!    NotADirectory(ENOTDIR) 判 missing 裁剪，仅裁剪发生时重写 preferences.json；
//!    EACCES 等未知错误一律保留。normalize 幂等 ⇒ 「有裁剪」等价于旧端
//!    JSON.stringify(next)!==JSON.stringify(state) 的 changed 判定。
//! 2. PUT 体校验 400 同文案："workspace must be a non-empty path"、
//!    "workspace UI surface is invalid"、"invalid JSON body"。旧 safeJson 口径：
//!    空体/不可解析体回落 {} → 走 workspace 校验 400；JSON null/标量 →
//!    "invalid JSON body"；JSON 数组按旧 `typeof []==="object"` 口径视为无字段
//!    对象 → 同样走 workspace 校验 400。
//! 3. 旧端 PUT catch-all→400、GET catch-all→500 的不对称，按本族既有端口约定
//!    （userprefs.rs appearance 端口先例）统一为：存储损坏/IO 响亮 500，体校验
//!    400；不复制 catch-all 静默口径。
//! 4. 时间戳：ServiceClock::now_unix_ms()（u64 毫秒）对应 TS Date.now()；
//!    测试中 ManualClock 固定 1_800_000_000_000。
//! 5. 数字线形：JS JSON.stringify 对整数值不输出小数点；js_num 对有限、整数值、
//!    |v|≤2^53-1 发整数 Number，其余发 f64（如 1e20 一类纯损坏输入线形为指数形，
//!    解析值相等）。
//! 6. JS trim 集合（\t\n\v\f\r、空格、\u00a0、\u1680、\u2000-\u200a、\u2028、
//!    \u2029、\u202f、\u205f、\u3000、\ufeff）≠ Rust str::trim；字符串长度/截断
//!    按 UTF-16 代码单元计，astral 字符跨界时向下取整到 char 边界（不制造孤立
//!    代理项）。
//! 7. workspaces 为 JSON 数组时按旧 Object.entries 口径以索引字符串为键；为
//!    字符串/标量等纯损坏形态时折叠为空（TS 会按字符索引造出垃圾记录，本端口
//!    在该损坏形态下取空并在此显式标注）。
//! 8. workspaces 对象键序：serde_json 未启用 preserve_order（BTreeMap 字母序）；
//!    影响仅限于 >50 个 workspace 且 updatedAt 全相等时的裁剪成员选择（旧端按
//!    稳定插入序，本端口按字母序），updatedAt 各异的正常路径无差异。
//! 9. 授权沿现役 server/http/route-security.ts:74-78：GET files.read /
//!    PUT files.write / 其余动词 LocalOnly（auth.rs 注册）。

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

use crate::memory::MemoryService;
use crate::ServiceState;

const MAX_WORKSPACES: usize = 50;
const MAX_PATHS: usize = 256;
const MAX_TABS: usize = 32;
const MAX_STRING: usize = 1024;
const VERSION: i64 = 2;
const SURFACES: [&str; 2] = ["electron", "pwa"];
const DEFAULT_SURFACE: &str = "electron";
const MAX_SCROLL: f64 = 10_000_000.0;
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

// ── JS 语义基元 ─────────────────────────────────────────────────────────────

/// JS WhiteSpace + LineTerminator 集合（ECMA-262）。
fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

/// String.prototype.slice(0, max)：按 UTF-16 代码单元截断；astral 字符跨界时
/// 向下取整到 char 边界（不制造孤立代理项，见模块头决策 6）。
fn utf16_take(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    for c in s.chars() {
        let width = c.len_utf16();
        if used + width > max {
            break;
        }
        out.push(c);
        used += width;
    }
    out
}

/// JS 数字线形：整数值（|v|≤2^53-1）发整数 Number，其余发 f64（决策 5）。
fn js_num(value: f64) -> Value {
    if value.is_finite() && value.fract() == 0.0 && value.abs() <= MAX_SAFE_INTEGER {
        json!(value as i64)
    } else {
        json!(value)
    }
}

/// Number.isFinite：仅 JSON 数字通过；字符串/布尔等一律 None。
fn finite(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64).filter(|v| v.is_finite())
}

/// cleanNumber：finite 后 clamp(min, max)（Math.min(max, Math.max(min, v))）。
fn clean_number(value: Option<&Value>, min: f64, max: f64) -> Option<f64> {
    finite(value).map(|v| v.max(min).min(max))
}

/// cleanString：仅字符串通过；trim 后按 UTF-16 单元截到 max。
fn clean_string(value: Option<&Value>, max: usize) -> String {
    match value.and_then(Value::as_str) {
        Some(s) => utf16_take(js_trim(s), max),
        None => String::new(),
    }
}

/// JS truthiness（GC/字段保留判定的 falsy 排除）。
fn js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|v| v != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

// ── shared/workspace-history.ts:3-11 ────────────────────────────────────────

fn normalize_workspace_path(value: Option<&str>) -> Option<String> {
    let trimmed = js_trim(value?);
    if trimmed.is_empty() {
        return None;
    }
    let slashed = trimmed.replace('\\', "/");
    if slashed == "/" {
        return Some(slashed);
    }
    // /^[A-Za-z]:\/?$/ → 盘符根保证尾斜杠。
    let bytes = slashed.as_bytes();
    if bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Some(format!("{slashed}/"));
    }
    if bytes.len() == 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/' {
        return Some(slashed);
    }
    Some(slashed.trim_end_matches('/').to_string())
}

// ── shared/workspace-ui-state.ts ────────────────────────────────────────────

fn normalize_relative_path(value: Option<&Value>) -> String {
    let raw = clean_string(value, MAX_STRING);
    if raw.is_empty() {
        return raw;
    }
    let slashed = raw.replace('\\', "/");
    let slashed = slashed.trim_matches('/');
    if slashed.is_empty() || slashed == "." || slashed == ".." {
        return String::new();
    }
    let parts: Vec<&str> = slashed.split('/').filter(|part| !part.is_empty()).collect();
    if parts.iter().any(|part| *part == "." || *part == "..") {
        return String::new();
    }
    parts.join("/")
}

fn normalize_preview_source_root_path(value: Option<&Value>) -> String {
    let root = normalize_workspace_path(value.and_then(Value::as_str)).unwrap_or_default();
    if root.is_empty() {
        return root;
    }
    let bytes = root.as_bytes();
    let windows_drive =
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/';
    if root.starts_with('/') || windows_drive || root.starts_with("studio:") {
        return root;
    }
    String::new()
}

fn unique_relative_paths(value: Option<&Value>, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    if let Some(items) = value.and_then(Value::as_array) {
        for item in items {
            let normalized = normalize_relative_path(Some(item));
            if normalized.is_empty() || !seen.insert(normalized.clone()) {
                continue;
            }
            out.push(normalized);
            if out.len() >= limit {
                break;
            }
        }
    }
    out
}

fn normalize_workspace_ui_surface(value: Option<&str>) -> Option<&'static str> {
    let raw = value.map(|s| js_trim(s).to_lowercase()).unwrap_or_default();
    if raw.is_empty() {
        return Some(DEFAULT_SURFACE);
    }
    match raw.as_str() {
        "electron" => Some("electron"),
        "pwa" => Some("pwa"),
        _ => None,
    }
}

fn normalize_right_workspace_tab(value: Option<&Value>) -> String {
    let raw = clean_string(value, 128);
    if raw == "session-files" || raw == "workspace" || raw.starts_with("plugin-widget:") {
        return raw;
    }
    "workspace".to_string()
}

// ── shared/preview-reading-position.ts ──────────────────────────────────────

fn normalize_scroll_snapshot(raw: Option<&Value>) -> Option<Value> {
    let raw = raw?;
    if !(raw.is_object() || raw.is_array()) {
        return None;
    }
    let scroll_top = clean_number(raw.get("scrollTop"), 0.0, MAX_SCROLL)?;
    let mut out = serde_json::Map::new();
    out.insert("scrollTop".to_string(), js_num(scroll_top));
    // scrollLeft/scrollHeight/clientHeight 仅 truthy 才保留（0 被丢弃）。
    for key in ["scrollLeft", "scrollHeight", "clientHeight"] {
        if let Some(v) = clean_number(raw.get(key), 0.0, MAX_SCROLL) {
            if v != 0.0 {
                out.insert(key.to_string(), js_num(v));
            }
        }
    }
    if let Some(v) = clean_number(raw.get("ratio"), 0.0, 1.0) {
        out.insert("ratio".to_string(), js_num(v));
    }
    for (key, max) in [("anchorId", 256), ("anchorText", 256), ("contentHash", 128)] {
        let cleaned = clean_string(raw.get(key), max);
        if !cleaned.is_empty() {
            out.insert(key.to_string(), json!(cleaned));
        }
    }
    if let Some(v) = clean_number(raw.get("updatedAt"), 0.0, MAX_SAFE_INTEGER) {
        out.insert("updatedAt".to_string(), js_num(v));
    }
    Some(Value::Object(out))
}

fn normalize_preview_reading_position(raw: Option<&Value>) -> Option<Value> {
    let raw = raw?;
    if !(raw.is_object() || raw.is_array()) {
        return None;
    }
    let preview = normalize_scroll_snapshot(raw.get("preview"));
    let edit = normalize_scroll_snapshot(raw.get("edit"));
    let current_heading_id = clean_string(raw.get("currentHeadingId"), 256);
    let current_heading_text = clean_string(raw.get("currentHeadingText"), 256);
    let content_hash = clean_string(raw.get("contentHash"), 128);
    if preview.is_none()
        && edit.is_none()
        && current_heading_id.is_empty()
        && current_heading_text.is_empty()
        && content_hash.is_empty()
    {
        return None;
    }
    let mut out = serde_json::Map::new();
    if let Some(v) = preview {
        out.insert("preview".to_string(), v);
    }
    if let Some(v) = edit {
        out.insert("edit".to_string(), v);
    }
    if !current_heading_id.is_empty() {
        out.insert("currentHeadingId".to_string(), json!(current_heading_id));
    }
    if !current_heading_text.is_empty() {
        out.insert(
            "currentHeadingText".to_string(),
            json!(current_heading_text),
        );
    }
    if !content_hash.is_empty() {
        out.insert("contentHash".to_string(), json!(content_hash));
    }
    if let Some(v) = clean_number(raw.get("updatedAt"), 0.0, MAX_SAFE_INTEGER) {
        out.insert("updatedAt".to_string(), js_num(v));
    }
    Some(Value::Object(out))
}

// ── entry / record / state 规范化 ───────────────────────────────────────────

fn normalize_preview_tab(raw: &Value) -> Option<Value> {
    if !(raw.is_object() || raw.is_array()) {
        return None;
    }
    let id = clean_string(raw.get("id"), MAX_STRING);
    if id.is_empty() {
        return None;
    }
    let file_path =
        normalize_workspace_path(raw.get("filePath").and_then(Value::as_str)).unwrap_or_default();
    let relative_path = normalize_relative_path(raw.get("relativePath"));
    let source_root_path = normalize_preview_source_root_path(raw.get("sourceRootPath"));
    let reading_position = normalize_preview_reading_position(raw.get("readingPosition"));
    if file_path.is_empty() && relative_path.is_empty() {
        return None;
    }
    let mut out = serde_json::Map::new();
    out.insert("id".to_string(), json!(id));
    if !file_path.is_empty() {
        out.insert("filePath".to_string(), json!(file_path));
    }
    if !relative_path.is_empty() {
        out.insert("relativePath".to_string(), json!(relative_path));
    }
    out.insert(
        "title".to_string(),
        json!(clean_string(raw.get("title"), 256)),
    );
    let tab_type = clean_string(raw.get("type"), 64);
    out.insert(
        "type".to_string(),
        json!(if tab_type.is_empty() {
            "file-info".to_string()
        } else {
            tab_type
        }),
    );
    out.insert(
        "ext".to_string(),
        json!(clean_string(raw.get("ext"), 32).to_lowercase()),
    );
    let language = clean_string(raw.get("language"), 64);
    out.insert(
        "language".to_string(),
        if language.is_empty() {
            Value::Null
        } else {
            json!(language)
        },
    );
    if !source_root_path.is_empty() {
        out.insert("sourceRootPath".to_string(), json!(source_root_path));
    }
    if let Some(position) = reading_position {
        out.insert("readingPosition".to_string(), position);
    }
    Some(Value::Object(out))
}

fn normalize_workspace_ui_entry(raw: Option<&Value>, now: f64) -> Value {
    let raw = raw.unwrap_or(&Value::Null);
    let mut preview_tabs: Vec<Value> = Vec::new();
    let mut seen_tabs: HashSet<String> = HashSet::new();
    if let Some(items) = raw.get("previewTabs").and_then(Value::as_array) {
        for item in items {
            let Some(tab) = normalize_preview_tab(item) else {
                continue;
            };
            let id = tab["id"].as_str().unwrap_or_default().to_string();
            if !seen_tabs.insert(id) {
                continue;
            }
            preview_tabs.push(tab);
            if preview_tabs.len() >= MAX_TABS {
                break;
            }
        }
    }

    let tab_ids: HashSet<&str> = preview_tabs
        .iter()
        .filter_map(|tab| tab["id"].as_str())
        .collect();
    let mut open_tabs: Vec<String> = Vec::new();
    if let Some(items) = raw.get("openTabs").and_then(Value::as_array) {
        for raw_id in items {
            let id = clean_string(Some(raw_id), MAX_STRING);
            if id.is_empty() || !tab_ids.contains(id.as_str()) || open_tabs.contains(&id) {
                continue;
            }
            open_tabs.push(id);
            if open_tabs.len() >= MAX_TABS {
                break;
            }
        }
    }
    if open_tabs.is_empty() && !preview_tabs.is_empty() {
        open_tabs.push(
            preview_tabs[0]["id"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        );
    }
    let requested_active = clean_string(raw.get("activeTabId"), MAX_STRING);
    let active_tab_id = if open_tabs.contains(&requested_active) {
        json!(requested_active)
    } else if let Some(first) = open_tabs.first() {
        json!(first)
    } else {
        Value::Null
    };

    let updated_at = finite(raw.get("updatedAt")).unwrap_or(now);
    let jian_view = clean_string(raw.get("jianView"), 128);
    let mut out = serde_json::Map::new();
    out.insert("updatedAt".to_string(), js_num(updated_at));
    out.insert(
        "deskCurrentPath".to_string(),
        json!(normalize_relative_path(raw.get("deskCurrentPath"))),
    );
    out.insert(
        "deskExpandedPaths".to_string(),
        json!(unique_relative_paths(
            raw.get("deskExpandedPaths"),
            MAX_PATHS
        )),
    );
    out.insert(
        "deskSelectedPath".to_string(),
        json!(normalize_relative_path(raw.get("deskSelectedPath"))),
    );
    out.insert(
        "rightWorkspaceTab".to_string(),
        json!(normalize_right_workspace_tab(raw.get("rightWorkspaceTab"))),
    );
    out.insert(
        "jianView".to_string(),
        json!(if jian_view.is_empty() {
            "desk".to_string()
        } else {
            jian_view
        }),
    );
    out.insert(
        "jianDrawerOpen".to_string(),
        json!(raw.get("jianDrawerOpen") == Some(&Value::Bool(true))),
    );
    out.insert(
        "previewOpen".to_string(),
        json!(raw.get("previewOpen") == Some(&Value::Bool(true))),
    );
    out.insert("openTabs".to_string(), json!(open_tabs));
    out.insert("activeTabId".to_string(), active_tab_id);
    out.insert("previewTabs".to_string(), Value::Array(preview_tabs));
    Value::Object(out)
}

fn normalize_workspace_ui_record(raw: &Value, now: f64) -> Value {
    let mut surfaces = serde_json::Map::new();
    if let Some(field) = raw
        .get("surfaces")
        .filter(|v| v.is_object() || v.is_array())
    {
        for surface in SURFACES {
            if let Some(entry_raw) = field.get(surface).filter(|v| js_truthy(v)) {
                surfaces.insert(
                    surface.to_string(),
                    normalize_workspace_ui_entry(Some(entry_raw), now),
                );
            }
        }
    } else {
        let entry = normalize_workspace_ui_entry(Some(raw), now);
        let has_legacy_state = entry["deskCurrentPath"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
            || entry["deskExpandedPaths"]
                .as_array()
                .is_some_and(|a| !a.is_empty())
            || entry["deskSelectedPath"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
            || entry["previewTabs"]
                .as_array()
                .is_some_and(|a| !a.is_empty())
            || entry["previewOpen"].as_bool() == Some(true)
            || entry["jianDrawerOpen"].as_bool() == Some(true)
            || entry["rightWorkspaceTab"].as_str() != Some("workspace")
            || entry["jianView"].as_str() != Some("desk")
            || finite(raw.get("updatedAt")).is_some();
        if has_legacy_state {
            surfaces.insert(DEFAULT_SURFACE.to_string(), entry);
        }
    }

    // updatedAt = max(0, entries…)；为 0 时回落 source.updatedAt（finite）否则 now。
    let mut updated_at = 0.0f64;
    for entry in surfaces.values() {
        updated_at = updated_at.max(entry["updatedAt"].as_f64().unwrap_or(0.0));
    }
    let updated_at = if updated_at != 0.0 {
        updated_at
    } else {
        finite(raw.get("updatedAt")).unwrap_or(now)
    };
    let mut record = serde_json::Map::new();
    record.insert("updatedAt".to_string(), js_num(updated_at));
    record.insert("surfaces".to_string(), Value::Object(surfaces));
    Value::Object(record)
}

fn normalize_workspace_ui_state(raw: Option<&Value>, now: f64) -> Value {
    let source = raw.unwrap_or(&Value::Null);
    let mut entries: Vec<(String, f64, Value)> = Vec::new();
    let mut push = |key: &str, value: &Value| {
        if let Some(workspace) = normalize_workspace_path(Some(key)) {
            let record = normalize_workspace_ui_record(value, now);
            let updated_at = record["updatedAt"].as_f64().unwrap_or(0.0);
            entries.push((workspace, updated_at, record));
        }
    };
    match source.get("workspaces") {
        Some(Value::Object(map)) => {
            for (key, value) in map {
                push(key, value);
            }
        }
        Some(Value::Array(items)) => {
            for (index, value) in items.iter().enumerate() {
                push(&index.to_string(), value);
            }
        }
        _ => {}
    }
    // 旧端为稳定降序（updatedAt 大者在前）；slice(0, MAX_WORKSPACES)。
    entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut workspaces = serde_json::Map::new();
    for (workspace, _, record) in entries.into_iter().take(MAX_WORKSPACES) {
        workspaces.insert(workspace, record);
    }
    let mut out = serde_json::Map::new();
    out.insert("version".to_string(), json!(VERSION));
    out.insert("workspaces".to_string(), Value::Object(workspaces));
    Value::Object(out)
}

fn get_workspace_ui_state_entry(raw: &Value, workspace: &str, surface: &str, now: f64) -> Value {
    let state = normalize_workspace_ui_state(Some(raw), now);
    state["workspaces"]
        .get(workspace)
        .and_then(|record| record.get("surfaces"))
        .and_then(|surfaces| surfaces.get(surface))
        .cloned()
        .unwrap_or(Value::Null)
}

fn upsert_workspace_ui_state(
    raw: &Value,
    workspace: &str,
    entry: &Value,
    surface: &str,
    now: f64,
) -> Value {
    let state = normalize_workspace_ui_state(Some(raw), now);
    let mut record = state["workspaces"]
        .get(workspace)
        .cloned()
        .unwrap_or_else(|| json!({ "updatedAt": 0, "surfaces": {} }));
    let normalized_entry = normalize_workspace_ui_entry(Some(entry), now);
    let mut surfaces = record
        .get("surfaces")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    surfaces.insert(surface.to_string(), normalized_entry.clone());
    // record.updatedAt = max(entry.updatedAt || 0, 各 surface updatedAt || 0)。
    let mut updated_at = normalized_entry["updatedAt"].as_f64().unwrap_or(0.0);
    for item in surfaces.values() {
        updated_at = updated_at.max(item.get("updatedAt").and_then(Value::as_f64).unwrap_or(0.0));
    }
    if let Some(object) = record.as_object_mut() {
        object.insert("surfaces".to_string(), Value::Object(surfaces));
        object.insert("updatedAt".to_string(), js_num(updated_at));
    }
    let mut workspaces = state["workspaces"].as_object().cloned().unwrap_or_default();
    workspaces.insert(workspace.to_string(), record);
    let mut merged = serde_json::Map::new();
    merged.insert("version".to_string(), json!(VERSION));
    merged.insert("workspaces".to_string(), Value::Object(workspaces));
    normalize_workspace_ui_state(Some(&Value::Object(merged)), now)
}

// ── GC：shared/workspace-persistence-gc.ts pruneMissingWorkspaceUiState ──────

/// 现役 gcWorkspaceUiState：NotFound/NotADirectory 判 missing 裁剪，仅裁剪时落盘。
fn gc_workspace_ui_state(
    memory: &MemoryService,
    root: &mut Value,
    now: f64,
) -> Result<Value, Response> {
    let raw = root
        .get("workspace_ui_state")
        .cloned()
        .unwrap_or(Value::Null);
    let state = normalize_workspace_ui_state(Some(&raw), now);
    let mut kept = serde_json::Map::new();
    let mut pruned = false;
    if let Some(map) = state["workspaces"].as_object() {
        for (workspace, record) in map {
            let missing = match std::fs::metadata(workspace) {
                Ok(_) => false,
                Err(err) => matches!(
                    err.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ),
            };
            if missing {
                pruned = true;
            } else {
                kept.insert(workspace.clone(), record.clone());
            }
        }
    }
    if !pruned {
        return Ok(state);
    }
    let mut next = serde_json::Map::new();
    next.insert("version".to_string(), json!(VERSION));
    next.insert("workspaces".to_string(), Value::Object(kept));
    let next = Value::Object(next);
    root["workspace_ui_state"] = next.clone();
    write_root(memory, root)?;
    Ok(next)
}

// ── 存储与路由 ──────────────────────────────────────────────────────────────

#[allow(clippy::result_large_err)]
fn read_root(memory: &MemoryService) -> Result<Value, Response> {
    // 与 agents_admin/userprefs 同源：缺失为空对象，损坏/非对象响亮 500。
    memory.read_preferences_root().map_err(|message| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": message })),
        )
            .into_response()
    })
}

#[allow(clippy::result_large_err)]
fn write_root(memory: &MemoryService, root: &Value) -> Result<(), Response> {
    memory.write_preferences_root(root).map_err(|message| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": message })),
        )
            .into_response()
    })
}

fn bad_request(message: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": message }))).into_response()
}

/// GET /lingxi/v1/preferences/workspace-ui-state?workspace=…&surface=…
/// 现役 preferences.ts:490-503：校验 → GC（engine 绑定路径）→ `{state}`。
pub async fn workspace_ui_state_get_route(
    State(state): State<ServiceState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let Some(workspace) = normalize_workspace_path(query.get("workspace").map(String::as_str))
    else {
        return bad_request("workspace must be a non-empty path");
    };
    let Some(surface) = normalize_workspace_ui_surface(query.get("surface").map(String::as_str))
    else {
        return bad_request("workspace UI surface is invalid");
    };
    let now = state.clock().now_unix_ms() as f64;
    let memory = state.memory().clone();
    let _lock = crate::agents_admin::preferences_locked();
    let mut root = match read_root(&memory) {
        Ok(root) => root,
        Err(response) => return response,
    };
    let state_view = match gc_workspace_ui_state(&memory, &mut root, now) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let entry = get_workspace_ui_state_entry(&state_view, &workspace, surface, now);
    Json(json!({ "state": entry })).into_response()
}

/// PUT /lingxi/v1/preferences/workspace-ui-state
/// 现役 preferences.ts:505-522：safeJson → 体校验 → upsert 落盘 → `{ok,state}`。
pub async fn workspace_ui_state_put_route(
    State(state): State<ServiceState>,
    body: axum::body::Bytes,
) -> Response {
    // 旧 safeJson 口径：空体/不可解析体回落 {}（随后 workspace 校验 400）；
    // null/标量 → "invalid JSON body"；数组按旧 typeof 口径视为无字段对象。
    let parsed: Value = serde_json::from_slice(&body).unwrap_or_else(|_| json!({}));
    if !(parsed.is_object() || parsed.is_array()) {
        return bad_request("invalid JSON body");
    }
    let Some(workspace) = normalize_workspace_path(parsed.get("workspace").and_then(Value::as_str))
    else {
        return bad_request("workspace must be a non-empty path");
    };
    let Some(surface) =
        normalize_workspace_ui_surface(parsed.get("surface").and_then(Value::as_str))
    else {
        return bad_request("workspace UI surface is invalid");
    };
    let now = state.clock().now_unix_ms() as f64;
    // 现役路由层先 normalizeWorkspaceUiEntry(body.state || {})，upsert 内二次
    // 规范化幂等；此处一次等价（entry.updatedAt 已 finite，upsert 原样保留）。
    let entry = normalize_workspace_ui_entry(parsed.get("state"), now);
    let memory = state.memory().clone();
    let _lock = crate::agents_admin::preferences_locked();
    let mut root = match read_root(&memory) {
        Ok(root) => root,
        Err(response) => return response,
    };
    let raw = root
        .get("workspace_ui_state")
        .cloned()
        .unwrap_or(Value::Null);
    let next = upsert_workspace_ui_state(&raw, &workspace, &entry, surface, now);
    root["workspace_ui_state"] = next.clone();
    if let Err(response) = write_root(&memory, &root) {
        return response;
    }
    let result = get_workspace_ui_state_entry(&next, &workspace, surface, now);
    Json(json!({ "ok": true, "state": result })).into_response()
}
