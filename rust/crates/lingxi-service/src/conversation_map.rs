//! R06 收口：会话地图画布布局持久化面（原 conversation-map 路由的
//! layout 叶）。
//!
//! 语义源（端口式移植，不猜造）：`server/routes/conversation-map.ts`
//! - `GET /lingxi/v1/sessions/{id}/conversation-map/layout` — 现役
//!   `GET /conversation-map/layout`（:211-223）：返回持久化布局或默认
//!   空布局 `{version:1, positions:{}, collapsed:[], updatedAt:null}`。
//! - `PUT /lingxi/v1/sessions/{id}/conversation-map/layout` — 现役
//!   `PUT /conversation-map/layout`（:225-262）：body 必须是对象且至少
//!   含 `positions` / `collapsed` 之一（否则 400 invalid_body）；
//!   `replacePositions===true` 时整体替换坐标表（未指定坐标被删除），
//!   默认按 key 合并（仅指定卡片变化）；`collapsed` 一旦出现即整体
//!   替换；写成功返回 `{ok:true, updatedAt}`。
//! - 落盘契约（现役 conversation-map.ts:26/33-35/253-257）：
//!   `lingxiHome/conversation-map.json`（Rust 侧 = `data_home`），
//!   形状 `{version:1, positions:{key:{x,y}}, collapsed:[key], updatedAt}`，
//!   临时文件 + rename 原子写入，`JSON.stringify(next, null, 2)` 缩进。
//!
//! 鉴权口径：与 `conversation-map/turns`（history_content.rs）同族——
//! chat scope（auth.rs classify_route）+ 会话归属逐请求复检（归属闸在
//! handler 内先于一切读写）。现役 layout 是 studio 级 sessions.read/
//! write；Rust 会话面把该族统一挂在会话 id 路径下，归属即读/写边界。
//!
//! 已知端口偏差（显式标注，不静默）：
//! - 读回默认布局的「任何读/解析失败 → 默认布局」是现役契约本身
//!   （conversation-map.ts:61-68 防御性归一化），照端口保留。
//! - `Math.round` 半数行为：JS `floor(x+0.5)`（.5 朝 +Infinity）；
//!   Rust `f64::round` 是远离零，故用 [`js_round`] 保持一致。
//! - 键长 200 的度量：JS 按 UTF-16 码元，Rust 按 Unicode 标量计数
//!   （仅星面字符差 1，卡片 id 实际不含）。
//! - 超 5000 条的「先来先得」：JS 按插入序，serde_json Map 按键序；
//!   上限本身是防御边界，语义（超量丢弃、不报错）一致。

use std::path::{Path, PathBuf};

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Map, Value};

use crate::{EndpointError, Principal, ServiceState};

/// 现役 conversation-map.ts:28 `POSITION_KEY_LIMIT`。
pub const POSITION_KEY_LIMIT: usize = 200;
/// 现役 conversation-map.ts:29 `POSITION_COORD_LIMIT`。
pub const POSITION_COORD_LIMIT: f64 = 100_000.0;
/// 现役 conversation-map.ts:30 `MAX_POSITION_ENTRIES`。
pub const MAX_POSITION_ENTRIES: usize = 5000;
/// 现役 conversation-map.ts:31 `MAX_COLLAPSED_ENTRIES`。
pub const MAX_COLLAPSED_ENTRIES: usize = 5000;

/// 现役 DEFAULT_LAYOUT（conversation-map.ts:26）。
pub fn default_layout() -> Value {
    json!({
        "version": 1,
        "positions": {},
        "collapsed": [],
        "updatedAt": null,
    })
}

/// 布局文件路径：`lingxiHome/conversation-map.json`（Rust = data_home）。
pub fn layout_file_path(home: &Path) -> PathBuf {
    home.join("conversation-map.json")
}

/// JS `Math.round` = `floor(x + 0.5)`（.5 朝 +Infinity；Rust `f64::round`
/// 是远离零——半数坐标的取整结果会差 1，端口保持 JS 行为）。
fn js_round(n: f64) -> f64 {
    (n + 0.5).floor()
}

/// 坐标取整并夹取（conversation-map.ts:81）。
fn clamp_coord(n: f64) -> i64 {
    js_round(n).clamp(-POSITION_COORD_LIMIT, POSITION_COORD_LIMIT) as i64
}

fn finite_pair(position: &Value) -> Option<(f64, f64)> {
    let x = position.get("x").and_then(Value::as_f64)?;
    let y = position.get("y").and_then(Value::as_f64)?;
    if x.is_finite() && y.is_finite() {
        Some((x, y))
    } else {
        None
    }
}

/// 防御性归一化磁盘上的布局文件（conversation-map.ts:38-59）：
/// 任何字段非法都回退默认形状；positions 只留有限坐标（原值不取整），
/// collapsed 只留字符串项。
pub fn normalize_layout(value: &Value) -> Value {
    let Value::Object(fields) = value else {
        return default_layout();
    };
    let mut positions = Map::new();
    if let Some(Value::Object(entries)) = fields.get("positions") {
        for (key, position) in entries {
            if finite_pair(position).is_none() {
                continue;
            }
            // 保留原始数值表示（整数读回仍是整数，不产生 12.0 漂移）。
            positions.insert(
                key.clone(),
                json!({
                    "x": position.get("x").cloned().unwrap_or(Value::Null),
                    "y": position.get("y").cloned().unwrap_or(Value::Null),
                }),
            );
        }
    }
    let collapsed = match fields.get("collapsed") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    json!({
        "version": 1,
        "positions": Value::Object(positions),
        "collapsed": collapsed,
        "updatedAt": fields.get("updatedAt").and_then(Value::as_str),
    })
}

/// 读布局文件；缺失/损坏一律回默认布局（conversation-map.ts:61-68）。
pub fn read_layout_file(home: &Path) -> Value {
    let raw = match std::fs::read_to_string(layout_file_path(home)) {
        Ok(raw) => raw,
        Err(_) => return default_layout(),
    };
    match serde_json::from_str::<Value>(&raw) {
        Ok(value) => normalize_layout(&value),
        Err(_) => default_layout(),
    }
}

/// 归一化 PUT 进来的 positions（conversation-map.ts:71-85）：键长限制、
/// 坐标取整并夹取、非有限值丢弃、超量按先来先得丢弃。
pub fn sanitize_positions_input(value: Option<&Value>) -> Map<String, Value> {
    let mut positions = Map::new();
    let Some(Value::Object(entries)) = value else {
        return positions;
    };
    for (key, position) in entries {
        if positions.len() >= MAX_POSITION_ENTRIES {
            break;
        }
        if key.is_empty() || key.chars().count() > POSITION_KEY_LIMIT {
            continue;
        }
        let Some((x, y)) = finite_pair(position) else {
            continue;
        };
        positions.insert(
            key.clone(),
            json!({ "x": clamp_coord(x), "y": clamp_coord(y) }),
        );
    }
    positions
}

/// 归一化 PUT 进来的 collapsed（conversation-map.ts:87-96）：非字符串/
/// 空/超长项丢弃、超量丢弃。
pub fn sanitize_collapsed_input(value: Option<&Value>) -> Vec<String> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    let mut collapsed = Vec::new();
    for item in items {
        if collapsed.len() >= MAX_COLLAPSED_ENTRIES {
            break;
        }
        match item.as_str() {
            Some(entry) if !entry.is_empty() && entry.chars().count() <= POSITION_KEY_LIMIT => {
                collapsed.push(entry.to_string());
            }
            _ => {}
        }
    }
    collapsed
}

// ── 路由 ────────────────────────────────────────────────────────────────────

/// 会话归属闸（history_content.rs gate_owned 同口径）：未知 404 / 跨主体
/// 403，先于一切布局读写，不向未授权主体泄露布局状态。
/// Err 载荷是即用响应（axum 处理器惯用形），尺寸告警按仓库先例放行。
#[allow(clippy::result_large_err)]
async fn gate_owned(
    state: &ServiceState,
    principal: &Principal,
    session_id: &str,
) -> Result<(), Response> {
    match state.sessions.get_for(principal, session_id).await {
        Ok(crate::sessions::SessionAccess::Ok(_)) => Ok(()),
        Ok(crate::sessions::SessionAccess::NotFound) => {
            Err(EndpointError::not_found().into_response())
        }
        Ok(crate::sessions::SessionAccess::Forbidden) => {
            Err(EndpointError::forbidden("cross_principal_access").into_response())
        }
        Err(err) => Err(EndpointError::storage(&err).into_response()),
    }
}

fn invalid_body() -> Response {
    EndpointError::resource_failure(StatusCode::BAD_REQUEST, "invalid_body", "invalid_body")
        .into_response()
}

/// `GET /lingxi/v1/sessions/{session_id}/conversation-map/layout` —
/// 返回持久化布局或默认空布局。
pub async fn layout_get_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    AxumPath(session_id): AxumPath<String>,
) -> Response {
    if let Err(resp) = gate_owned(&state, &principal, &session_id).await {
        return resp;
    }
    Json(read_layout_file(state.config().data_home.as_path())).into_response()
}

/// `PUT /lingxi/v1/sessions/{session_id}/conversation-map/layout` —
/// positions 合并/整体替换与 collapsed 整体替换（现役 :225-262）。
pub async fn layout_put_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    AxumPath(session_id): AxumPath<String>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(j) => j,
        Err(rejection) => return EndpointError::from_json_rejection(&rejection).into_response(),
    };
    if let Err(resp) = gate_owned(&state, &principal, &session_id).await {
        return resp;
    }
    let Value::Object(fields) = &body else {
        return invalid_body();
    };
    let has_positions = fields.contains_key("positions");
    let has_collapsed = fields.contains_key("collapsed");
    if !has_positions && !has_collapsed {
        return invalid_body();
    }
    let replace_positions = fields.get("replacePositions").and_then(Value::as_bool) == Some(true);
    let current = read_layout_file(state.config().data_home.as_path());
    let mut positions = match current.get("positions") {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    };
    if has_positions {
        let incoming = sanitize_positions_input(fields.get("positions"));
        if replace_positions {
            // 整体替换（布局重置）：未指定的坐标被删除。
            positions = incoming;
        } else {
            // 默认合并：仅指定卡片变化。
            for (key, value) in incoming {
                positions.insert(key, value);
            }
        }
    }
    let collapsed = if has_collapsed {
        sanitize_collapsed_input(fields.get("collapsed"))
    } else {
        current
            .get("collapsed")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let updated_at = match iso_millis(state.clock.now_unix_ms()) {
        Ok(text) => text,
        Err(message) => return internal_error(message),
    };
    let next = json!({
        "version": 1,
        "positions": Value::Object(positions),
        "collapsed": collapsed,
        "updatedAt": updated_at,
    });
    if let Err(message) = write_layout_atomic(state.config().data_home.as_path(), &next) {
        return internal_error(message);
    }
    Json(json!({ "ok": true, "updatedAt": updated_at })).into_response()
}

/// `new Date().toISOString()` 形状：恒三位毫秒 + Z（resources.rs iso_millis
/// 同款）。
fn iso_millis(ms: u64) -> Result<String, String> {
    let ms_i64 = i64::try_from(ms).map_err(|_| "timestamp overflow".to_string())?;
    chrono::DateTime::from_timestamp_millis(ms_i64)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .ok_or_else(|| "timestamp out of range".to_string())
}

/// 原子写入（conversation-map.ts:254-257）：mkdir -p → 写
/// `{file}.{pid}.tmp` → rename。失败响亮返回，绝不静默。
fn write_layout_atomic(home: &Path, next: &Value) -> Result<(), String> {
    let file = layout_file_path(home);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("layout mkdir failed: {err}"))?;
    }
    let tmp = PathBuf::from(format!("{}.{}.tmp", file.display(), std::process::id()));
    let body = serde_json::to_string_pretty(next)
        .map_err(|err| format!("layout serialize failed: {err}"))?;
    std::fs::write(&tmp, body).map_err(|err| format!("layout write failed: {err}"))?;
    std::fs::rename(&tmp, &file).map_err(|err| {
        let _ = std::fs::remove_file(&tmp);
        format!("layout rename failed: {err}")
    })
}

fn internal_error(message: String) -> Response {
    EndpointError::resource_failure(StatusCode::INTERNAL_SERVER_ERROR, "layout_io", message)
        .into_response()
}
