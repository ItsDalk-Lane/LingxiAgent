//! R06 会话项目/文件夹目录（server/routes/session-projects.ts +
//! core/session-project-catalog-store.ts 的端口）。存储镜像现役
//! `user/session-projects.json` 契约格式（version 1 + camelCase），
//! 归属语义映射：现役未分类 `"cwd:"` ↔ 端口 NULL。
//!
//! 排序契约（compareCatalogItems）：order 升序 → 名称 → id；
//! 重排（reorderScopedItems）：给定顺序中已知项在前，剩余项按现有
//! 排序追加，作用域内 order 重编 0..n。文件夹删除时其下项目移到根
//! 末尾；项目删除时活动会话归属清空（由路由层配合会话存储执行）。
use std::path::PathBuf;
use std::sync::Mutex;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use lingxi_adapters::storage::agent_memory as am;
use serde_json::{json, Value};

use crate::ServiceState;

const MAX_NAME_LENGTH: usize = 80;
const MAX_ID_LENGTH: usize = 240;
/// 现役未分类项目 id（端口内以 NULL 表达，读取时保留该常量用于响应对照）。
pub const UNCATEGORIZED_PROJECT_ID: &str = "cwd:";

pub struct SessionProjectsService {
    path: PathBuf,
    mutation: Mutex<()>,
}

pub struct ProjectsError(pub String);

impl ProjectsError {
    fn response(&self) -> Response {
        // 现役 session-projects 路由把全部业务失败按 400 家族返回。
        (StatusCode::BAD_REQUEST, Json(json!({ "error": self.0 }))).into_response()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FolderRecord {
    id: String,
    name: String,
    order: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectRecord {
    id: String,
    name: String,
    folder_id: Option<String>,
    order: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogFile {
    version: u32,
    #[serde(default)]
    folders: Vec<FolderRecord>,
    #[serde(default)]
    projects: Vec<ProjectRecord>,
}

fn normalize_name(raw: &str) -> Option<String> {
    let collapsed: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    Some(collapsed.chars().take(MAX_NAME_LENGTH).collect())
}

fn normalize_id(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_ID_LENGTH {
        return None;
    }
    Some(trimmed.to_string())
}

fn is_auto_project_id(id: &str) -> bool {
    id.starts_with("cwd:")
}

fn compare_items(a: &Value, b: &Value) -> std::cmp::Ordering {
    let order = a["order"].as_i64().cmp(&b["order"].as_i64());
    if order != std::cmp::Ordering::Equal {
        return order;
    }
    let name = a["name"].as_str().cmp(&b["name"].as_str());
    if name != std::cmp::Ordering::Equal {
        return name;
    }
    a["id"].as_str().cmp(&b["id"].as_str())
}

/// 比较器投影到结构体记录（与 compare_items 同键序）。
fn compare_records<T>(key: fn(&T) -> (&i64, &String, &String), a: &T, b: &T) -> std::cmp::Ordering {
    let (oa, na, ia) = key(a);
    let (ob, nb, ib) = key(b);
    oa.cmp(ob).then_with(|| na.cmp(nb)).then_with(|| ia.cmp(ib))
}

impl SessionProjectsService {
    pub fn new(home: &std::path::Path) -> Self {
        Self {
            path: home.join("user").join("session-projects.json"),
            mutation: Mutex::new(()),
        }
    }

    fn read(&self) -> Result<CatalogFile, ProjectsError> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(CatalogFile {
                    version: 1,
                    folders: Vec::new(),
                    projects: Vec::new(),
                })
            }
            Err(err) => return Err(ProjectsError(format!("cannot read catalog: {err}"))),
        };
        let file: CatalogFile = serde_json::from_slice(&bytes)
            .map_err(|err| ProjectsError(format!("invalid session-projects.json: {err}")))?;
        if file.version != 1 {
            return Err(ProjectsError("unsupported catalog version".into()));
        }
        for item in file
            .folders
            .iter()
            .map(|f| (&f.id, &f.name))
            .chain(file.projects.iter().map(|p| (&p.id, &p.name)))
        {
            if item.0.trim().is_empty() || item.1.trim().is_empty() {
                return Err(ProjectsError("invalid catalog entry".into()));
            }
        }
        Ok(file)
    }

    fn write(&self, file: &CatalogFile) -> Result<(), ProjectsError> {
        let text = serde_json::to_string_pretty(file)
            .map_err(|err| ProjectsError(format!("cannot encode catalog: {err}")))?;
        am::atomic_write(&self.path, &text)
            .map_err(|err| ProjectsError(format!("cannot save catalog: {err}")))
    }

    fn folder_json(folder: &FolderRecord) -> Value {
        json!({ "id": folder.id, "name": folder.name, "order": folder.order })
    }

    fn project_json(project: &ProjectRecord) -> Value {
        json!({
            "id": project.id,
            "name": project.name,
            "folderId": project.folder_id,
            "order": project.order,
        })
    }

    fn catalog_json(file: &CatalogFile) -> Value {
        let mut folders: Vec<Value> = file.folders.iter().map(Self::folder_json).collect();
        folders.sort_by(compare_items);
        let mut projects: Vec<Value> = file.projects.iter().map(Self::project_json).collect();
        projects.sort_by(compare_items);
        json!({ "folders": folders, "projects": projects })
    }

    fn next_id(prefix: &str) -> String {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_millis() as u64)
            .unwrap_or(0);
        format!("{prefix}-{}-{:x}-{seq}", std::process::id(), now)
    }

    fn next_order(projects: &[ProjectRecord], folder: Option<&str>) -> i64 {
        projects
            .iter()
            .filter(|item| item.folder_id.as_deref() == folder)
            .map(|item| item.order)
            .max()
            .unwrap_or(-1)
            + 1
    }

    fn resolve_folder(
        file: &CatalogFile,
        folder_id: Option<&str>,
    ) -> Result<Option<String>, ProjectsError> {
        let Some(normalized) = normalize_id(folder_id) else {
            return Ok(None);
        };
        if !file.folders.iter().any(|folder| folder.id == normalized) {
            return Err(ProjectsError("folder not found".into()));
        }
        Ok(Some(normalized))
    }

    fn normalized_ids(raw: Option<&Vec<String>>) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(ids) = raw {
            for id in ids {
                if let Some(normalized) = normalize_id(Some(id)) {
                    if !out.contains(&normalized) {
                        out.push(normalized);
                    }
                }
            }
        }
        out
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.mutation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn catalog(&self) -> Result<Value, ProjectsError> {
        Ok(Self::catalog_json(&self.read()?))
    }

    pub fn create_folder(&self, raw_name: &str) -> Result<Value, ProjectsError> {
        let _guard = self.lock();
        let mut file = self.read()?;
        let name = normalize_name(raw_name)
            .ok_or_else(|| ProjectsError("folder name is required".into()))?;
        let folder = FolderRecord {
            id: Self::next_id("folder"),
            name,
            order: file
                .folders
                .iter()
                .map(|item| item.order)
                .max()
                .unwrap_or(-1)
                + 1,
        };
        let value = Self::folder_json(&folder);
        file.folders.push(folder);
        self.write(&file)?;
        Ok(value)
    }

    pub fn update_folder(
        &self,
        raw_id: &str,
        raw_name: Option<&str>,
    ) -> Result<Value, ProjectsError> {
        let _guard = self.lock();
        let mut file = self.read()?;
        let id =
            normalize_id(Some(raw_id)).ok_or_else(|| ProjectsError("folder not found".into()))?;
        let folder = file
            .folders
            .iter_mut()
            .find(|folder| folder.id == id)
            .ok_or_else(|| ProjectsError("folder not found".into()))?;
        if let Some(raw) = raw_name {
            folder.name = normalize_name(raw)
                .ok_or_else(|| ProjectsError("folder name is required".into()))?;
        }
        let value = Self::folder_json(folder);
        self.write(&file)?;
        Ok(value)
    }

    /// 删除文件夹：其下项目移到根（order 接在现有根项目之后），全表重排。
    pub fn delete_folder(&self, raw_id: &str) -> Result<Value, ProjectsError> {
        let _guard = self.lock();
        let mut file = self.read()?;
        let id =
            normalize_id(Some(raw_id)).ok_or_else(|| ProjectsError("folder not found".into()))?;
        if !file.folders.iter().any(|folder| folder.id == id) {
            return Err(ProjectsError("folder not found".into()));
        }
        let mut order = Self::next_order(&file.projects, None);
        for project in file.projects.iter_mut() {
            if project.folder_id.as_deref() == Some(id.as_str()) {
                project.folder_id = None;
                project.order = order;
                order += 1;
            }
        }
        file.folders.retain(|folder| folder.id != id);
        file.folders
            .sort_by(|a, b| compare_records(|f| (&f.order, &f.name, &f.id), a, b));
        file.projects
            .sort_by(|a, b| compare_records(|p| (&p.order, &p.name, &p.id), a, b));
        let value = Self::catalog_json(&file);
        self.write(&file)?;
        Ok(value)
    }

    pub fn reorder_folders(&self, raw_ids: Option<&Vec<String>>) -> Result<Value, ProjectsError> {
        let _guard = self.lock();
        let mut file = self.read()?;
        let ids = Self::normalized_ids(raw_ids);
        let mut ordered: Vec<FolderRecord> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for id in &ids {
            if let Some(folder) = file.folders.iter().find(|folder| &folder.id == id) {
                let mut next = folder.clone();
                next.order = ordered.len() as i64;
                seen.push(id.clone());
                ordered.push(next);
            }
        }
        file.folders
            .sort_by(|a, b| compare_records(|f| (&f.order, &f.name, &f.id), a, b));
        for folder in file.folders.iter() {
            if !seen.contains(&folder.id) {
                let mut next = folder.clone();
                next.order = ordered.len() as i64;
                ordered.push(next);
            }
        }
        file.folders = ordered;
        let value = Self::catalog_json(&file);
        self.write(&file)?;
        Ok(value)
    }

    pub fn create_project(
        &self,
        raw_name: &str,
        raw_folder: Option<&str>,
    ) -> Result<Value, ProjectsError> {
        let _guard = self.lock();
        let mut file = self.read()?;
        let folder = Self::resolve_folder(&file, raw_folder)?;
        let name = normalize_name(raw_name)
            .ok_or_else(|| ProjectsError("project name is required".into()))?;
        let order = Self::next_order(&file.projects, folder.as_deref());
        let project = ProjectRecord {
            id: Self::next_id("project"),
            name,
            folder_id: folder,
            order,
        };
        let value = Self::project_json(&project);
        file.projects.push(project);
        self.write(&file)?;
        Ok(value)
    }

    /// 更新项目；未知 id 但为 auto（cwd: 前缀）时按 upsert 创建。
    pub fn update_project(
        &self,
        raw_id: &str,
        raw_name: Option<&str>,
        raw_folder: Option<Option<&str>>,
    ) -> Result<Value, ProjectsError> {
        let _guard = self.lock();
        let mut file = self.read()?;
        let id =
            normalize_id(Some(raw_id)).ok_or_else(|| ProjectsError("project not found".into()))?;
        let existing = file.projects.iter().position(|p| p.id == id);
        match existing {
            None if is_auto_project_id(&id) => {
                let folder = match raw_folder {
                    Some(value) => Self::resolve_folder(&file, value)?,
                    None => None,
                };
                let name = raw_name
                    .and_then(normalize_name)
                    .ok_or_else(|| ProjectsError("project name is required".into()))?;
                let order = Self::next_order(&file.projects, folder.as_deref());
                let project = ProjectRecord {
                    id,
                    name,
                    folder_id: folder,
                    order,
                };
                let value = Self::project_json(&project);
                file.projects.push(project);
                self.write(&file)?;
                Ok(value)
            }
            None => Err(ProjectsError("project not found".into())),
            Some(index) => {
                let current = file.projects[index].clone();
                let mut next = current.clone();
                if let Some(raw) = raw_name {
                    next.name = normalize_name(raw)
                        .ok_or_else(|| ProjectsError("project name is required".into()))?;
                }
                if let Some(raw) = raw_folder {
                    let folder = Self::resolve_folder(&file, raw)?;
                    if folder != current.folder_id {
                        next.folder_id = folder.clone();
                        let base: Vec<ProjectRecord> = file
                            .projects
                            .iter()
                            .filter(|item| item.id != current.id && item.folder_id == folder)
                            .cloned()
                            .collect();
                        next.order = Self::next_order(&base, folder.as_deref());
                    }
                }
                let value = Self::project_json(&next);
                file.projects[index] = next;
                self.write(&file)?;
                Ok(value)
            }
        }
    }

    /// 目录级删除项目（auto id 为无操作）；会话改派由路由层执行。
    pub fn delete_project(&self, raw_id: &str) -> Result<Value, ProjectsError> {
        let _guard = self.lock();
        let mut file = self.read()?;
        let id =
            normalize_id(Some(raw_id)).ok_or_else(|| ProjectsError("project not found".into()))?;
        let Some(index) = file.projects.iter().position(|p| p.id == id) else {
            if is_auto_project_id(&id) {
                return Ok(Self::catalog_json(&file));
            }
            return Err(ProjectsError("project not found".into()));
        };
        file.projects.remove(index);
        let value = Self::catalog_json(&file);
        self.write(&file)?;
        Ok(value)
    }

    /// 作用域内重排项目（folderId 必须存在；null = 根）。
    pub fn reorder_projects(
        &self,
        raw_folder: Option<&str>,
        raw_ids: Option<&Vec<String>>,
    ) -> Result<Value, ProjectsError> {
        let _guard = self.lock();
        let mut file = self.read()?;
        let folder = Self::resolve_folder(&file, raw_folder)?;
        let ids = Self::normalized_ids(raw_ids);
        let mut ordered: Vec<ProjectRecord> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for id in &ids {
            if let Some(project) = file
                .projects
                .iter()
                .find(|p| &p.id == id && p.folder_id == folder)
            {
                let mut next = project.clone();
                next.order = ordered.len() as i64;
                seen.push(id.clone());
                ordered.push(next);
            }
        }
        let mut rest: Vec<ProjectRecord> = file
            .projects
            .iter()
            .filter(|p| p.folder_id == folder && !seen.contains(&p.id))
            .cloned()
            .collect();
        rest.sort_by(|a, b| compare_records(|p| (&p.order, &p.name, &p.id), a, b));
        for mut project in rest {
            project.order = ordered.len() as i64;
            ordered.push(project);
        }
        let ordered_by_id: std::collections::HashMap<String, ProjectRecord> = ordered
            .into_iter()
            .map(|project| (project.id.clone(), project))
            .collect();
        for project in file.projects.iter_mut() {
            if project.folder_id == folder {
                if let Some(next) = ordered_by_id.get(&project.id) {
                    *project = next.clone();
                }
            }
        }
        let value = Self::catalog_json(&file);
        self.write(&file)?;
        Ok(value)
    }
}

// ── HTTP 处理器 ─────────────────────────────────────────────────────────────

fn json_field<'a>(body: &'a Value, key: &str) -> Option<&'a str> {
    body.get(key).and_then(Value::as_str)
}

pub async fn catalog_route(State(state): State<ServiceState>) -> Response {
    match state.session_projects().catalog() {
        Ok(value) => Json(json!({ "catalog": value })).into_response(),
        Err(err) => err.response(),
    }
}

pub async fn create_project_route(
    State(state): State<ServiceState>,
    body: axum::body::Bytes,
) -> Response {
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => json!({}),
    };
    match state.session_projects().create_project(
        json_field(&payload, "name").unwrap_or(""),
        json_field(&payload, "folderId"),
    ) {
        Ok(project) => Json(json!({ "ok": true, "project": project })).into_response(),
        Err(err) => err.response(),
    }
}

pub async fn create_folder_route(
    State(state): State<ServiceState>,
    body: axum::body::Bytes,
) -> Response {
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => json!({}),
    };
    match state
        .session_projects()
        .create_folder(json_field(&payload, "name").unwrap_or(""))
    {
        Ok(folder) => Json(json!({ "ok": true, "folder": folder })).into_response(),
        Err(err) => err.response(),
    }
}

pub async fn update_folder_route(
    State(state): State<ServiceState>,
    AxumPath(folder_id): AxumPath<String>,
    body: axum::body::Bytes,
) -> Response {
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => json!({}),
    };
    match state
        .session_projects()
        .update_folder(&folder_id, json_field(&payload, "name"))
    {
        Ok(folder) => Json(json!({ "ok": true, "folder": folder })).into_response(),
        Err(err) => err.response(),
    }
}

pub async fn delete_folder_route(
    State(state): State<ServiceState>,
    AxumPath(folder_id): AxumPath<String>,
) -> Response {
    match state.session_projects().delete_folder(&folder_id) {
        Ok(catalog) => Json(json!({ "ok": true, "catalog": catalog })).into_response(),
        Err(err) => err.response(),
    }
}

pub async fn reorder_folders_route(
    State(state): State<ServiceState>,
    body: axum::body::Bytes,
) -> Response {
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => json!({}),
    };
    let ids = payload.get("folderIds").and_then(Value::as_array);
    let owned: Option<Vec<String>> = ids.map(|list| {
        list.iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect()
    });
    match state.session_projects().reorder_folders(owned.as_ref()) {
        Ok(catalog) => Json(json!({ "ok": true, "catalog": catalog })).into_response(),
        Err(err) => err.response(),
    }
}

pub async fn update_project_route(
    State(state): State<ServiceState>,
    AxumPath(project_id): AxumPath<String>,
    body: axum::body::Bytes,
) -> Response {
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => json!({}),
    };
    let folder: Option<Option<&str>> = if payload.get("folderId").is_some() {
        Some(json_field(&payload, "folderId"))
    } else {
        None
    };
    match state
        .session_projects()
        .update_project(&project_id, json_field(&payload, "name"), folder)
    {
        Ok(project) => Json(json!({ "ok": true, "project": project })).into_response(),
        Err(err) => err.response(),
    }
}

/// 项目删除：目录删除 + 活动会话归属清空（NULL ↔ 现役 "cwd:"）。
pub async fn delete_project_route(
    axum::Extension(principal): axum::Extension<crate::auth::Principal>,
    State(state): State<ServiceState>,
    AxumPath(project_id): AxumPath<String>,
) -> Response {
    let _ = &principal;
    let sessions = match lingxi_adapters::storage::session_admin::session_ids_for_project(
        state.storage(),
        &project_id,
    )
    .await
    {
        Ok(value) => value,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("cannot list project sessions: {err}") })),
            )
                .into_response();
        }
    };
    match state.session_projects().delete_project(&project_id) {
        Ok(catalog) => {
            let mut cleared = Vec::new();
            for session_id in &sessions {
                match state
                    .session_admin
                    .set_session_project(session_id, None)
                    .await
                {
                    Ok(Some(_)) => cleared.push(session_id.clone()),
                    Ok(None) => {}
                    Err(err) => {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({
                                "error": "project deleted; session reassignment pending",
                                "detail": format!("{err:?}"),
                            })),
                        )
                            .into_response();
                    }
                }
            }
            Json(json!({
                "ok": true,
                "catalog": catalog,
                "assignment": {
                    "projectId": UNCATEGORIZED_PROJECT_ID,
                    "sessionIds": cleared,
                },
            }))
            .into_response()
        }
        Err(err) => err.response(),
    }
}

pub async fn reorder_projects_route(
    State(state): State<ServiceState>,
    body: axum::body::Bytes,
) -> Response {
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => json!({}),
    };
    let ids = payload.get("projectIds").and_then(Value::as_array);
    let owned: Option<Vec<String>> = ids.map(|list| {
        list.iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect()
    });
    match state
        .session_projects()
        .reorder_projects(json_field(&payload, "folderId"), owned.as_ref())
    {
        Ok(catalog) => Json(json!({ "ok": true, "catalog": catalog })).into_response(),
        Err(err) => err.response(),
    }
}
