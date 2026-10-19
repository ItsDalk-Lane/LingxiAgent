//! R06独立改前存档恢复与T08全局元数据列表；不混入会话分支检查点。
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

    /// 现役 CheckpointStore.save（lib/checkpoint-store.ts:19-59）的用户编辑
    /// 面（engine.ts createUserEditCheckpoint :2987-2995：sessionPath=null、
    /// tool/source="user-edit"）。读不到/非文件 → None（现役 statSync 失败
    /// 或 readFileSync 抛错被调用方 .catch(()=>null) 吞掉）；写盘失败同样
    /// None（调用方不感知存档失败，主写入照旧）。
    pub fn save_user_edit(&self, file_path: &Path, reason: &str, now_ms: u64) -> Option<String> {
        let meta = std::fs::metadata(file_path).ok()?;
        if !meta.is_file() {
            return None;
        }
        let bytes = std::fs::read(file_path).ok()?;
        // 全量 round-trip：损坏字节序列经替换字符往返后与原始不同 → 判二进制
        // 走 base64；文本继续内嵌字符串。
        let roundtrip = String::from_utf8_lossy(&bytes);
        let is_text = roundtrip.as_bytes() == bytes.as_slice();
        std::fs::create_dir_all(&self.dir).ok()?;
        let suffix = {
            let mut buf = [0u8; 2];
            let filled = std::fs::File::open("/dev/urandom")
                .and_then(|mut f| {
                    use std::io::Read as _;
                    f.read_exact(&mut buf)
                })
                .is_ok();
            if filled {
                format!("{:02x}{:02x}", buf[0], buf[1])
            } else {
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0);
                format!("{:04x}", nanos & 0xffff)
            }
        };
        let id = format!("{now_ms}_{suffix}");
        let mut data = json!({
            "ts": now_ms,
            "sessionPath": Value::Null,
            "tool": "user-edit",
            "source": "user-edit",
            "reason": reason,
            "path": file_path.to_string_lossy(),
            "size": meta.len(),
        });
        if is_text {
            data["content"] = Value::String(roundtrip.into_owned());
        } else {
            data["content_b64"] =
                Value::String(base64::engine::general_purpose::STANDARD.encode(&bytes));
        }
        let text = serde_json::to_string(&data).ok()?;
        crate::paths::atomic_write(&self.dir.join(format!("{id}.json")), text.as_bytes()).ok()?;
        Some(id)
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

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserEditBody {
    file_path: String,
    reason: String,
}

/// 用户开始编辑与自动保存的改前备份，独立于会话分支检查点。
pub async fn user_edit_route(
    State(state): State<ServiceState>,
    body: Result<Json<UserEditBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return crate::EndpointError::from_json_rejection(&error).into_response(),
    };
    if !Path::new(&body.file_path).is_absolute()
        || !matches!(body.reason.as_str(), "edit-start" | "autosave-interval")
    {
        return crate::EndpointError::invalid_message(
            "absolute filePath and edit-start/autosave-interval reason required",
        )
        .into_response();
    }
    let home = state.config().data_home.clone();
    let now = state.clock.now_unix_ms();
    let saved = tokio::task::spawn_blocking(move || -> Result<Value, String> {
        use std::io::Read;
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
        }
        let mut file = options.open(&body.file_path).map_err(|error| format!("cannot read edited file: {error}"))?;
        let before = file.metadata().map_err(|error| error.to_string())?;
        if !before.is_file() { return Err("edited path is not a regular file".into()); }
        let identity = crate::filetools::fsio::FileIdentity::of(&file).map_err(|error|error.to_string())?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(|error|error.to_string())?;
        if crate::filetools::fsio::FileIdentity::of(&file).map_err(|error|error.to_string())? != identity
            || file.metadata().and_then(|meta|meta.modified()).ok() != before.modified().ok() {
            return Err("edited file changed while capturing checkpoint".into());
        }
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|error|error.to_string())?.as_nanos();
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = format!("{now}_{:x}_{}_{}", nonce, std::process::id(), SEQ.fetch_add(1,std::sync::atomic::Ordering::Relaxed));
        let mut value = json!({"ts":now,"sessionPath":null,"tool":"user-edit","source":"user-edit","reason":body.reason,"path":body.file_path,"size":bytes.len()});
        match std::str::from_utf8(&bytes) {
            Ok(text) => value["content"] = json!(text),
            Err(_) => value["content_b64"] = json!(base64::engine::general_purpose::STANDARD.encode(&bytes)),
        }
        let target = home.join("checkpoints").join(format!("{id}.json"));
        let encoded = serde_json::to_string(&value).map_err(|error|error.to_string())?;
        lingxi_adapters::storage::agent_memory::atomic_write(&target,&encoded).map_err(|error|error.to_string())?;
        Ok(json!({"id":id,"path":body.file_path,"reason":body.reason}))
    }).await;
    match saved {
        Ok(Ok(checkpoint)) => Json(json!({"ok":true,"checkpoint":checkpoint})).into_response(),
        Ok(Err(error)) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &error),
        Err(error) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
    }
}

#[cfg(windows)]
type ArchiveReadDirectory = crate::discovery_tools::windows_readonly::DirFd;
#[cfg(not(windows))]
type ArchiveReadDirectory = crate::filetools::fsio::DirFd;

// serde流式跳过content/content_b64及未知字段，不把存档正文复制到列表或返回值中。
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArchiveMetadataOnly {
    ts: u64,
    tool: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    path: String,
    size: u64,
    #[serde(default)]
    session_path: Option<String>,
}
impl CheckpointStore {
    /// 全局改前存档列表，复用既有checkpoints目录；缺失不创建，损坏/链接明确失败。
    pub fn list(
        &self,
    ) -> Result<Vec<lingxi_protocol::CheckpointArchiveMetadata>, CheckpointRestoreError> {
        let directory = match ArchiveReadDirectory::open(&self.dir) {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => {
                return Err(CheckpointRestoreError::Io(
                    "checkpoint archive directory is unsafe or unreadable".into(),
                ))
            }
        };
        let names = directory.entry_names(|| false).map_err(|_| {
            CheckpointRestoreError::Io("cannot list checkpoint archive directory".into())
        })?;
        let mut output = Vec::new();
        for name in names {
            let Some(name_text) = name.to_str() else {
                return Err(CheckpointRestoreError::Corrupt(
                    "checkpoint archive name is not UTF-8".into(),
                ));
            };
            let Some(id) = name_text.strip_suffix(".json") else {
                continue;
            };
            let mut file = directory
                .open_attachment_nofollow(&name)
                .ok_or_else(|| {
                    CheckpointRestoreError::Io("checkpoint archive changed while listing".into())
                })?
                .map_err(|_| {
                    CheckpointRestoreError::Io(
                        "checkpoint archive is not a safe regular file".into(),
                    )
                })?;
            let before = file
                .metadata()
                .map_err(|_| CheckpointRestoreError::Io("cannot stat checkpoint archive".into()))?;
            if !before.is_file() {
                return Err(CheckpointRestoreError::Corrupt(
                    "checkpoint archive is not a regular file".into(),
                ));
            }
            #[cfg(windows)]
            let identity = crate::discovery_tools::windows_readonly::file_identity(&file);
            #[cfg(not(windows))]
            let identity = crate::filetools::fsio::file_identity(&file);
            let identity = identity.map_err(|_| {
                CheckpointRestoreError::Io("cannot identify checkpoint archive".into())
            })?;
            let metadata: ArchiveMetadataOnly =
                serde_json::from_reader(std::io::BufReader::new(&mut file)).map_err(|_| {
                    CheckpointRestoreError::Corrupt(
                        "invalid checkpoint archive metadata or JSON".into(),
                    )
                })?;
            let after = file.metadata().map_err(|_| {
                CheckpointRestoreError::Io("cannot restat checkpoint archive".into())
            })?;
            #[cfg(windows)]
            let final_identity = crate::discovery_tools::windows_readonly::file_identity(&file);
            #[cfg(not(windows))]
            let final_identity = crate::filetools::fsio::file_identity(&file);
            if final_identity.ok() != Some(identity)
                || before.len() != after.len()
                || before.modified().ok() != after.modified().ok()
                || !directory.same_directory(&self.dir).unwrap_or(false)
            {
                return Err(CheckpointRestoreError::Io(
                    "checkpoint archive changed while listing".into(),
                ));
            }
            if metadata.tool.is_empty() || metadata.path.is_empty() {
                return Err(CheckpointRestoreError::Corrupt(
                    "checkpoint archive metadata is incomplete".into(),
                ));
            }
            output.push(lingxi_protocol::CheckpointArchiveMetadata {
                id: id.to_owned(),
                ts: metadata.ts,
                tool: metadata.tool.clone(),
                source: metadata
                    .source
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| "llm".into()),
                reason: metadata
                    .reason
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| format!("tool-{}", metadata.tool)),
                path: metadata.path,
                size: metadata.size,
                session_path: metadata.session_path,
            });
        }
        if !directory.same_directory(&self.dir).unwrap_or(false) {
            return Err(CheckpointRestoreError::Io(
                "checkpoint archive directory changed while listing".into(),
            ));
        }
        output.sort_by(|a, b| b.ts.cmp(&a.ts).then_with(|| a.id.cmp(&b.id)));
        Ok(output)
    }
}

/// 安全页的全局存档面；与sessions/{id}/checkpoints的分支检查点分开。
pub async fn list_route(
    State(state): State<ServiceState>,
    axum::Extension(principal): axum::Extension<crate::auth::Principal>,
) -> Response {
    let authorized = |state: &ServiceState| {
        principal.is_local_owner() && state.auth().validate_principal(&principal).is_ok()
    };
    if !authorized(&state) {
        return json_error(StatusCode::FORBIDDEN, "checkpoint_archive_owner_required");
    }
    let home = state.config().data_home.clone();
    let result = tokio::task::spawn_blocking(move || CheckpointStore::new(&home).list()).await;
    if !authorized(&state) {
        return json_error(
            StatusCode::FORBIDDEN,
            "checkpoint_archive_authority_revoked",
        );
    }
    match result {
        Ok(Ok(checkpoints)) => {
            Json(lingxi_protocol::CheckpointArchiveList { checkpoints }).into_response()
        }
        Ok(Err(error)) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "checkpoint archive listing task failed",
        ),
    }
}
