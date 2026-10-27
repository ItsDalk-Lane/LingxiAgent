//! 保存发送入口冻结的会话提示词和工具名；读取只使用侧车，不重新编译。
use std::path::PathBuf;

use crate::{EndpointError, Principal, ServiceState};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Extension, Json,
};
use lingxi_adapters::storage::RunDatabase;
use lingxi_kernel::{model_exchange::ModelTurnInput, ports::StorageError};
use serde_json::{json, Value};

fn path(storage: &RunDatabase, session: &str) -> Result<PathBuf, StorageError> {
    let parent = storage
        .db_path()
        .parent()
        .ok_or_else(|| StorageError::Internal {
            detail: "session snapshot storage has no parent".into(),
        })?;
    Ok(parent.join("session-prompt-snapshots").join(format!(
        "{}.json",
        lingxi_kernel::session_tree::sha256_hex(session.as_bytes())
    )))
}
fn io(error: impl std::fmt::Display) -> StorageError {
    StorageError::Io {
        detail: format!("session prompt snapshot: {error}"),
    }
}

/// 参数直接来自即将发送的 ModelTurnInput；无论界面何时读取都不再读取助手新配置。
pub async fn record(
    storage: &RunDatabase,
    session_id: &str,
    run_id: &str,
    input: &ModelTurnInput,
) -> Result<(), StorageError> {
    let target = path(storage, session_id)?;
    let prompt = input
        .system_prompt
        .as_ref()
        .map(|text| json!({"version":1,"systemPrompt":text,"appendSystemPrompt":[]}));
    let value = json!({"version":1,"stage":"prepared_model_call","sessionId":session_id,"runId":run_id,"turn":input.turn,"promptSnapshot":prompt,"toolNames":input.tools.declarations.iter().map(|d|d.wire_name.clone()).collect::<Vec<_>>()});
    tokio::task::spawn_blocking(move || {
        let text = serde_json::to_string_pretty(&value).map_err(io)?;
        lingxi_adapters::storage::agent_memory::atomic_write(&target, &text).map_err(io)
    })
    .await
    .map_err(io)?
}

pub fn read(storage: &RunDatabase, session_id: &str) -> Result<Option<Value>, StorageError> {
    let target = path(storage, session_id)?;
    match std::fs::symlink_metadata(&target) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io(error)),
        Ok(_) => {}
    }
    let snapshot = crate::file_safety::Snapshot::read(&target).map_err(io)?;
    let value: Value = serde_json::from_slice(&snapshot.bytes).map_err(io)?;
    if value["version"] != 1
        || value["sessionId"] != session_id
        || !value["toolNames"].is_array()
        || !value["toolNames"]
            .as_array()
            .is_some_and(|names| names.iter().all(Value::is_string))
        || (!value["promptSnapshot"].is_null()
            && !value["promptSnapshot"]["systemPrompt"].is_string())
    {
        return Err(io("invalid saved snapshot"));
    }
    Ok(Some(value))
}

pub async fn get_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session): Path<String>,
) -> Response {
    if let Err(response) = crate::gate_session_owned(&state, &principal, &session).await {
        return response;
    }
    match read(state.storage(), &session) {
        Ok(Some(value)) => Json(value).into_response(),
        Ok(None) => Json(json!({"promptSnapshot":null,"toolNames":[],"available":false,"reason":"no_frozen_prompt_snapshot"})).into_response(),
        Err(error) => EndpointError::storage(&error).into_response(),
    }
}
