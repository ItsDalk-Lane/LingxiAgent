//! R06 收口：独立改前存档的 restore 面（原 checkpoints 路由的 restore 叶）。
//!
//! 语义源（端口式移植，不猜造）：
//! - `server/routes/checkpoints.ts:33-44` `POST /checkpoints/:id/restore`：
//!   调用恢复并返回写回位置 `{ok:true, restoredTo}`；存档文件缺失
//!   （ENOENT）→ 404 `checkpoint not found`；其余失败 → 500。
//! - `lib/checkpoint-store.ts:94-108` `CheckpointStore.restore(id)`：读
//!   `{lingxiHome}/checkpoints/{id}.json`（Rust 侧 = `data_home/checkpoints`），
//!   `content_b64` 非空则 base64 无损写回，否则按 utf-8 文本写回
//!   `obj.path`；目标目录缺失先 mkdir -p；返回 `{restoredTo: obj.path}`。
//!
//! 与会话 rewind 的区别（叶断言核心）：这是**独立快照恢复**——不移动
//! 分支头、不建 reset 标记、不碰会话树，只把存档字节写回存档时记录的
//! 绝对路径；session_tree.rs 的具名检查点/rewind 是另一套分支快照机制。
//!
//! 已知端口决策（显式标注）：
//! - 存档 id 不做白名单清洗：与现役一致由「单段路径参数 + 读不到即
//!   ENOENT 404」承载，id 里出现路径分隔符到不了本路由。
//! - base64 存档解码失败 → 显式 500（现役 Node `Buffer.from(str,
//!   "base64")` 对非法字符静默忽略——那是静默降级形状，按仓库红线
//!   不复刻：损坏存档必须响亮失败，不写半解码字节）。
//! - 鉴权：现役 `/api/checkpoints/*` 落到 route-security 的 `/api/`
//!   兜底 STUDIO_OWNER；Rust 侧无 studio 面，按 auth.rs 既有口径映射
//!   到最严格的 LocalOnly（本地属主凭证）。

use std::path::{Path, PathBuf};

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine as _;
use serde_json::{json, Value};

use crate::ServiceState;

/// 改前存档仓库（lib/checkpoint-store.ts 的读/恢复端口；save 面不在本叶）。
pub struct CheckpointStore {
    dir: PathBuf,
}

impl CheckpointStore {
    /// `{home}/checkpoints`（engine.ts:900-902 的落盘契约）。
    pub fn new(home: &Path) -> Self {
        Self {
            dir: home.join("checkpoints"),
        }
    }

    /// restore（checkpoint-store.ts:94-108）：存档字节写回 `obj.path`，
    /// 返回写回位置。缺失 → [`CheckpointRestoreError::NotFound`]。
    pub fn restore(&self, id: &str) -> Result<String, CheckpointRestoreError> {
        let raw = std::fs::read_to_string(self.dir.join(format!("{id}.json"))).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                CheckpointRestoreError::NotFound
            } else {
                CheckpointRestoreError::Io(format!("read checkpoint archive: {err}"))
            }
        })?;
        let obj: Value = serde_json::from_str(&raw).map_err(|err| {
            CheckpointRestoreError::Corrupt(format!("parse checkpoint archive: {err}"))
        })?;
        let target = obj
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty())
            .ok_or_else(|| {
                CheckpointRestoreError::Corrupt("checkpoint archive has no target path".into())
            })?;
        let bytes = match obj.get("content_b64").and_then(Value::as_str) {
            Some(encoded) if !encoded.is_empty() => base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|err| {
                    CheckpointRestoreError::Corrupt(format!("decode checkpoint archive: {err}"))
                })?,
            _ => obj
                .get("content")
                .and_then(Value::as_str)
                .map(|content| content.as_bytes().to_vec())
                .ok_or_else(|| {
                    CheckpointRestoreError::Corrupt(
                        "checkpoint archive has no content payload".into(),
                    )
                })?,
        };
        let target_path = Path::new(target);
        if let Some(parent) = target_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|err| {
                    CheckpointRestoreError::Io(format!("create restore target dir: {err}"))
                })?;
            }
        }
        std::fs::write(target_path, bytes)
            .map_err(|err| CheckpointRestoreError::Io(format!("write restore target: {err}")))?;
        Ok(target.to_string())
    }
}

/// restore 失败：每一支都响亮对调用方可见，绝不静默降级为成功。
#[derive(Debug)]
pub enum CheckpointRestoreError {
    /// 存档文件缺失（现役 ENOENT → 404 checkpoint not found）。
    NotFound,
    /// 下游 IO 失败（→ 500，message 如实外发）。
    Io(String),
    /// 存档损坏（非法 JSON / 缺目标路径 / 缺内容 / 非法 base64）→ 500。
    Corrupt(String),
}

impl std::fmt::Display for CheckpointRestoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckpointRestoreError::NotFound => write!(f, "checkpoint not found"),
            CheckpointRestoreError::Io(message) | CheckpointRestoreError::Corrupt(message) => {
                write!(f, "{message}")
            }
        }
    }
}

fn json_error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

/// `POST /lingxi/v1/checkpoints/{checkpoint_id}/restore` — 现役
/// `/api/checkpoints/:id/restore` 的路径形状（`/api` → `/lingxi/v1` 前缀
/// 映射，input-drafts / file-history 同款）；LocalOnly（auth.rs 分类）。
pub async fn restore_route(
    State(state): State<ServiceState>,
    AxumPath(checkpoint_id): AxumPath<String>,
) -> Response {
    let store = CheckpointStore::new(state.config().data_home.as_path());
    match store.restore(&checkpoint_id) {
        Ok(restored_to) => (
            StatusCode::OK,
            Json(json!({ "ok": true, "restoredTo": restored_to })),
        )
            .into_response(),
        Err(CheckpointRestoreError::NotFound) => {
            json_error(StatusCode::NOT_FOUND, "checkpoint not found")
        }
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
}
