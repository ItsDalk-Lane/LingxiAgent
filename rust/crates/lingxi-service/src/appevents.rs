//! app 级事件面（server/app-events.ts emitAppEvent → WS 广播的 Rust 端口）。
//!
//! 原实现把 `agent-created` / `agent-switched` / `agent-deleted` /
//! `skills-changed` 等事件经引擎事件管道推送到 app WebSocket。Rust 服务的
//! 事件中枢（EventService/EventHub）以会话为流边界（"a stream id IS a
//! session id"），app 级事件没有会话归属，不能借流伪装。本模块提供独立的
//! 进程内事件日志与 HTTP 检索面：
//!
//! - `publish(event_type, payload)`：服务侧操作成功后追加，全局单调序号；
//! - `GET /lingxi/v1/app-events?since=<seq>`：按游标回放，语义与
//!   resource-io 事件面一致（游标 < 最旧-1 → `stale: true` + 空事件；
//!   否则回送 `seq > 游标` 的事件），客户端轮询或后续 WS 桥接均可消费。
//!
//! 边界：日志为进程内环形缓冲（不持久化，重启后从 0 重新计序），与原实现
//! 的进程内广播语义对齐；不宣称跨重启投递。

use std::collections::VecDeque;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::ServiceState;

/// 与 resource 事件保留窗口同量级的环形容量。
pub const APP_EVENT_RETENTION: usize = 1000;

pub struct AppEventLog {
    inner: std::sync::Mutex<LogState>,
}

struct LogState {
    sequence: u64,
    recent: VecDeque<Value>,
}

impl Default for AppEventLog {
    fn default() -> Self {
        Self::new()
    }
}

impl AppEventLog {
    pub fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new(LogState {
                sequence: 0,
                recent: VecDeque::new(),
            }),
        }
    }

    /// 追加一条 app 事件并返回其序号。载荷必须是对象（与原 emitAppEvent
    /// 的 normalizePayload 一致：非对象载荷直接拒绝，不猜造包装）。
    pub fn publish(&self, event_type: &str, payload: Value) -> Option<u64> {
        if event_type.is_empty() || !payload.is_object() {
            return None;
        }
        let at_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_millis() as u64)
            .unwrap_or(0);
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.sequence += 1;
        let sequence = state.sequence;
        state.recent.push_back(json!({
            "seq": sequence,
            "type": event_type,
            "payload": payload,
            "source": "server",
            "atUnixMs": at_unix_ms,
        }));
        while state.recent.len() > APP_EVENT_RETENTION {
            state.recent.pop_front();
        }
        Some(sequence)
    }

    /// 现役 since（与 resourceio 事件面同语义）：游标 < 最旧-1 →
    /// `stale: true` + 空事件；否则回送 `seq > 游标` 的事件。
    pub fn since(&self, cursor: u64) -> Value {
        let state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let latest_sequence = state.sequence;
        let Some(oldest) = state.recent.front() else {
            return json!({
                "stale": false,
                "latestSequence": latest_sequence,
                "events": [],
            });
        };
        let oldest_sequence = oldest["seq"].as_u64().unwrap_or(latest_sequence);
        if cursor + 1 < oldest_sequence {
            return json!({
                "stale": true,
                "latestSequence": latest_sequence,
                "events": [],
            });
        }
        let events: Vec<Value> = state
            .recent
            .iter()
            .filter(|event| event["seq"].as_u64().unwrap_or(0) > cursor)
            .cloned()
            .collect();
        json!({
            "stale": false,
            "latestSequence": latest_sequence,
            "events": events,
        })
    }
}

pub async fn events_route(
    State(state): State<ServiceState>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let since = match query.get("since") {
        None => 0u64,
        Some(raw) => match raw.parse::<u64>() {
            Ok(value) => value,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "since must be a non-negative integer" })),
                )
                    .into_response()
            }
        },
    };
    Json(state.app_events().since(since)).into_response()
}
