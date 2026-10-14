use std::collections::BTreeMap;
use std::path::PathBuf;

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::auth::Principal;
use crate::{EndpointError, ServiceState};

const MAX_ENTRY_UNITS: usize = 512 * 1024;
const MAX_SESSIONS: usize = 200;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Surface {
    Electron,
    Pwa,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    doc: Option<serde_json::Map<String, Value>>,
    updated_at: u64,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Bucket {
    home: Option<Entry>,
    sessions: BTreeMap<String, Entry>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Surfaces {
    electron: Bucket,
    pwa: Bucket,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct File {
    version: u32,
    surfaces: Surfaces,
}

impl Default for File {
    fn default() -> Self {
        Self {
            version: 1,
            surfaces: Surfaces::default(),
        }
    }
}

impl File {
    fn bucket_mut(&mut self, surface: Surface) -> &mut Bucket {
        match surface {
            Surface::Electron => &mut self.surfaces.electron,
            Surface::Pwa => &mut self.surfaces.pwa,
        }
    }
}

pub struct InputDraftsService {
    path: PathBuf,
    mutation: Mutex<()>,
}

impl InputDraftsService {
    pub fn new(home: &std::path::Path) -> Self {
        Self {
            path: home.join("input-drafts.v1.json"),
            mutation: Mutex::new(()),
        }
    }

    fn read(&self) -> Result<File, EndpointError> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(File::default())
            }
            Err(error) => return Err(storage_error(format!("cannot read input drafts: {error}"))),
        };
        let file: File = serde_json::from_slice(&bytes)
            .map_err(|error| storage_error(format!("invalid input drafts: {error}")))?;
        if file.version != 1 {
            return Err(storage_error("unsupported input drafts version"));
        }
        for bucket in [&file.surfaces.electron, &file.surfaces.pwa] {
            if bucket.sessions.len() > MAX_SESSIONS
                || bucket.sessions.keys().any(|id| id.trim().is_empty())
                || bucket
                    .home
                    .iter()
                    .chain(bucket.sessions.values())
                    .any(|entry| entry.text.trim().is_empty())
            {
                return Err(storage_error("invalid saved input draft entries"));
            }
        }
        Ok(file)
    }

    fn save(&self, file: &File) -> Result<(), EndpointError> {
        let text = serde_json::to_string(file)
            .map_err(|error| storage_error(format!("cannot encode input drafts: {error}")))?;
        lingxi_adapters::storage::agent_memory::atomic_write(&self.path, &text)
            .map_err(|error| storage_error(format!("cannot save input drafts: {error}")))
    }

    /// R06 永久删除工作台会话的草稿清理：从两个 surface 移除指定会话的
    /// 草稿条目并落盘；返回移除条数。文件缺失/无命中为无操作。
    pub async fn remove_session_entries(&self, session_ids: &[&str]) -> Result<usize, String> {
        let _guard = self.mutation.lock().await;
        let mut file = self
            .read()
            .map_err(|_| "cannot read input drafts for cleanup".to_string())?;
        let mut removed = 0usize;
        for bucket in [&mut file.surfaces.electron, &mut file.surfaces.pwa] {
            for id in session_ids {
                if bucket.sessions.remove(*id).is_some() {
                    removed += 1;
                }
            }
        }
        if removed > 0 {
            self.save(&file)
                .map_err(|_| "cannot save input drafts after cleanup".to_string())?;
        }
        Ok(removed)
    }
}

fn storage_error(detail: impl Into<String>) -> EndpointError {
    EndpointError::storage(&lingxi_kernel::ports::StorageError::Io {
        detail: detail.into(),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftQuery {
    surface: Surface,
}

pub async fn get_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    query: Result<Query<DraftQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Query(query) = match query {
        Ok(query) => query,
        Err(error) => {
            return EndpointError::invalid_message(format!("invalid draft query: {error}"))
                .into_response()
        }
    };
    let _guard = state.input_drafts.mutation.lock().await;
    let mut file = match state.input_drafts.read() {
        Ok(file) => file,
        Err(error) => return error.into_response(),
    };
    let bucket = file.bucket_mut(query.surface);
    let mut visible = BTreeMap::new();
    for (id, entry) in &bucket.sessions {
        match state.sessions.get_for(&principal, id).await {
            Ok(crate::sessions::SessionAccess::Ok(_)) => {
                visible.insert(id, entry);
            }
            Ok(
                crate::sessions::SessionAccess::Forbidden
                | crate::sessions::SessionAccess::NotFound,
            ) => {}
            Err(error) => return EndpointError::storage(&error).into_response(),
        }
    }
    Json(json!({"home":bucket.home,"sessions":visible})).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PutBody {
    surface: Surface,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    session_path: Option<String>,
    text: String,
    #[serde(default)]
    doc: Option<serde_json::Map<String, Value>>,
}

pub async fn put_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<PutBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return EndpointError::from_json_rejection(&error).into_response(),
    };
    let home = body.scope.as_deref() == Some("home");
    if (body.scope.is_some() && !home)
        || (home && (body.session_id.is_some() || body.session_path.is_some()))
        || (!home
            && body
                .session_id
                .as_ref()
                .is_none_or(|id| id.trim().is_empty()))
        || body.session_path.is_some()
    {
        return EndpointError::invalid_message(
            "draft requires home or a sessionId; sessionPath has no registered session locator",
        )
        .into_response();
    }
    let _guard = state.input_drafts.mutation.lock().await;
    let session_id = body.session_id.as_deref().map(str::trim);
    if let Some(id) = session_id {
        if let Err(response) = crate::gate_session_owned(&state, &principal, id).await {
            return response;
        }
    }
    let mut file = match state.input_drafts.read() {
        Ok(file) => file,
        Err(error) => return error.into_response(),
    };
    let bucket = file.bucket_mut(body.surface);
    let mut entry = (!body.text.trim().is_empty()).then_some(Entry {
        text: body.text,
        doc: body.doc,
        updated_at: state.clock.now_unix_ms(),
    });
    let mut doc_dropped = false;
    let mut text_truncated = false;
    if let Some(entry) = &mut entry {
        let encoded = match serde_json::to_string(entry) {
            Ok(encoded) => encoded,
            Err(error) => {
                return storage_error(format!("cannot encode draft entry: {error}")).into_response()
            }
        };
        if encoded.encode_utf16().count() > MAX_ENTRY_UNITS {
            doc_dropped = entry.doc.take().is_some();
            let mut units = 0;
            let text: String = entry
                .text
                .chars()
                .take_while(|ch| {
                    units += ch.len_utf16();
                    units <= MAX_ENTRY_UNITS
                })
                .collect();
            text_truncated = text.len() != entry.text.len();
            entry.text = text;
        }
    }
    let mut evicted = Vec::new();
    if home {
        bucket.home = entry;
    } else if let Some(id) = session_id {
        match entry {
            Some(entry) => {
                bucket.sessions.insert(id.to_string(), entry);
                while bucket.sessions.len() > MAX_SESSIONS {
                    let mut oldest: Option<(&str, u64)> = None;
                    for (candidate, entry) in &bucket.sessions {
                        if candidate == id {
                            continue;
                        }
                        match state.sessions.get_for(&principal, candidate).await {
                            Ok(crate::sessions::SessionAccess::Ok(_)) => {
                                if oldest
                                    .is_none_or(|(_, updated_at)| entry.updated_at < updated_at)
                                {
                                    oldest = Some((candidate, entry.updated_at));
                                }
                            }
                            Ok(
                                crate::sessions::SessionAccess::Forbidden
                                | crate::sessions::SessionAccess::NotFound,
                            ) => {}
                            Err(error) => return EndpointError::storage(&error).into_response(),
                        }
                    }
                    let Some((oldest, _)) = oldest else {
                        return EndpointError::invalid_message(
                            "draft surface limit reached; no owned entry may be evicted",
                        )
                        .into_response();
                    };
                    let oldest = oldest.to_string();
                    bucket.sessions.remove(&oldest);
                    evicted.push(oldest);
                }
            }
            None => {
                bucket.sessions.remove(id);
            }
        }
    }
    match state.input_drafts.save(&file) {
        Ok(()) => Json(json!({"ok":true,"docDropped":doc_dropped,"textTruncated":text_truncated,"evictedSessionIds":evicted})).into_response(),
        Err(error) => error.into_response(),
    }
}
