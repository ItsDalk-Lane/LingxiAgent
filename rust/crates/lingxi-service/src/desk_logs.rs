//! 工作台开发日志：每个服务实例独立保留最近 200 条，重启后清空。
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

use crate::{Principal, ServiceState};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde_json::{json, Value};

const RETENTION: usize = 200;
const MAX_TEXT_CHARS: usize = 4096;

#[derive(Default)]
struct LogState {
    logs: VecDeque<Value>,
    activities: VecDeque<(String, String)>,
}

#[derive(Default)]
pub struct DeskLog {
    inner: Mutex<LogState>,
}

fn redact(text: &str) -> String {
    static SECRET: OnceLock<regex::Regex> = OnceLock::new();
    let text = text.chars().take(MAX_TEXT_CHARS).collect::<String>();
    let text = SECRET.get_or_init(|| regex::Regex::new(
        r#"(?i)(bearer\s+|(?:api[_-]?key|access[_-]?token|refresh[_-]?token|password|secret|token)\s*[:=]\s*["']?)[^\s,"';}]+"#,
    ).expect("固定日志脱敏表达式"))
    .replace_all(&text, "${1}[REDACTED]");
    lingxi_adapters::storage::agent_memory::scrub_pii(&text).0
}

impl DeskLog {
    pub fn record(&self, text: &str, level: &str, now: u64) -> Option<Value> {
        let text = redact(text);
        if text.trim().is_empty() {
            return None;
        }
        let level = match level {
            "error" => "error",
            "heartbeat" => "heartbeat",
            _ => "info",
        };
        let entry = json!({"text":text,"level":level,"ts":now});
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.logs.push_back(entry.clone());
        while inner.logs.len() > RETENTION {
            inner.logs.pop_front();
        }
        Some(entry)
    }

    /// 只记录状态变化，不因列表轮询重复制造运行消息；不收集活动正文。
    pub fn record_activity(&self, activity: &Value, now: u64) -> Option<Value> {
        let id = activity["id"].as_str()?;
        let status = activity["status"].as_str()?;
        let fingerprint = format!("{status}:{}", activity["error"]);
        {
            let mut inner = self
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(index) = inner.activities.iter().position(|(key, _)| key == id) {
                if inner.activities[index].1 == fingerprint {
                    return None;
                }
                inner.activities.remove(index);
            }
            inner.activities.push_back((id.into(), fingerprint));
            while inner.activities.len() > RETENTION {
                inner.activities.pop_front();
            }
        }
        let kind = if activity["type"] == "heartbeat" {
            "心跳"
        } else {
            "定时任务"
        };
        let error = activity["error"].as_str().unwrap_or("");
        let level = if status == "error" || status == "unknown" {
            "error"
        } else {
            "heartbeat"
        };
        self.record(
            &format!(
                "{kind} {id}: {status}{}",
                if error.is_empty() {
                    String::new()
                } else {
                    format!(" ({error})")
                }
            ),
            level,
            now,
        )
    }

    pub fn list(&self) -> Vec<Value> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .logs
            .iter()
            .cloned()
            .collect()
    }
}

pub async fn list_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
) -> Response {
    if !principal.is_local_owner() {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"local_owner_required"})),
        )
            .into_response();
    }
    Json(json!({"logs":state.desk_logs().list()})).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ring_is_bounded_redacted_and_per_instance() {
        let log = DeskLog::default();
        for n in 0..205 {
            log.record(&format!("记录{n}"), "info", n);
        }
        let logs = log.list();
        assert_eq!(logs.len(), 200);
        assert_eq!(logs[0]["ts"], 5);
        assert!(DeskLog::default().list().is_empty());
        let entry = log
            .record(
                "Bearer sensitive-token api_key=secret-value password: hidden",
                "error",
                300,
            )
            .unwrap();
        let text = entry["text"].as_str().unwrap();
        assert!(!text.contains("sensitive-token"));
        assert!(!text.contains("secret-value"));
        assert!(!text.contains("hidden"));
        assert!(log.record("", "info", 301).is_none());
    }
    #[test]
    fn repeated_activity_poll_does_not_duplicate_log_or_include_body() {
        let log = DeskLog::default();
        let mut activity =
            json!({"id":"trigger1","type":"heartbeat","status":"running","summary":"private body"});
        assert!(log.record_activity(&activity, 1).is_some());
        assert!(log.record_activity(&activity, 2).is_none());
        activity["status"] = json!("done");
        assert!(log.record_activity(&activity, 3).is_some());
        assert_eq!(log.list().len(), 2);
        assert!(!serde_json::to_string(&log.list())
            .unwrap()
            .contains("private body"));
    }
}
