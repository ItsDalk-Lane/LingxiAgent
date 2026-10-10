//! R06-T05 Round 5：fs 直读（叶 #23-24；对照 `server/routes/fs.ts`）。
//!
//! 对照现役（RC-3，逐项锚点进 07_diff_ledger）：
//! - resolveAllowedPath（:25-54）：词法 resolve → 首个命中根定生死；
//!   lstat 命中 symlink 一律拒绝；realpath 必须仍在 realRoot 内；
//!   ENOENT 时父目录 realpath 在根内则放行词法路径（保留 404 语义）。
//! - GET /fs/read（:89-99）：missing path 400 / path not allowed 403 /
//!   file not found 404；成功 c.text（utf-8 有损替换，safe-fs.ts :9）。
//! - GET /fs/read-base64（:102-115）：同闸；读出任何错误 404；成功
//!   base64 标准表文本。
//! - 授权根（:77-86 getAllowedRoots）的候选映射：data_home
//!   （≈lingxiHome）+ workspace 根（≈agent desk），bootstrap 一次构建
//!   （现役逐请求现算是为了运行时增删 agent；候选单工作区实例根集合
//!   静态，台账登记）。
//!
//! 设计登记（台账）：
//! - 候选人给两个读面都加 20MB 上限（01_design :87），413
//!   "file too large"——与现役 docx/xlsx 面（:128/:149）上限词汇一致；
//!   现役 read/read-base64 无上限。
//! - D12 延展：query 未知键/重复键 → 400。
//! - docx-html/xlsx-html 依赖 mammoth/ExcelJS，不在叶 #23-24 范围
//!   （叶图 deferred）。

use std::path::{Component, Path, PathBuf};

use axum::extract::{RawQuery, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::ServiceState;

/// 设计登记的读面上限（01_design :87；词汇与 fs.ts :128/:149 一致）。
pub const MAX_FS_READ_BYTES: u64 = 20 * 1024 * 1024;

fn is_inside_root(candidate: &Path, root: &Path) -> bool {
    candidate == root || candidate.starts_with(root)
}

/// path.resolve 的词法映射：相对路径基于 cwd 绝对化；折叠 `.`/`..`
/// （`..` 越过根时现役保留上跳——path.resolve("/..") === "/"，根处
/// `..` 被吞掉）。
fn lexical_resolve(input: &str) -> PathBuf {
    let path = Path::new(input);
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path)
    };
    let mut out = PathBuf::new();
    let mut rooted = false;
    for comp in abs.components() {
        match comp {
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => {
                rooted = true;
                out.push(comp.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() && !rooted {
                    out.push("..");
                }
            }
            Component::Normal(seg) => out.push(seg),
        }
    }
    out
}

/// resolveAllowedPath（fs.ts :25-54）逐行映射。
pub fn resolve_allowed_path(file_path: &str, allowed_roots: &[PathBuf]) -> Option<PathBuf> {
    let resolved = lexical_resolve(file_path);
    for root in allowed_roots {
        let resolved_root = lexical_resolve(&root.to_string_lossy());
        if !is_inside_root(&resolved, &resolved_root) {
            continue;
        }
        let Ok(real_root) = std::fs::canonicalize(&resolved_root) else {
            continue;
        };
        match std::fs::symlink_metadata(&resolved) {
            Ok(meta) => {
                // symlink 一律拒绝（:37-38）。
                if meta.file_type().is_symlink() {
                    return None;
                }
                let real_path = std::fs::canonicalize(&resolved).ok()?;
                if is_inside_root(&real_path, &real_root) {
                    return Some(real_path);
                }
                return None;
            }
            Err(err) => {
                if err.kind() != std::io::ErrorKind::NotFound {
                    return None;
                }
                // ENOENT：父目录 realpath 在根内 → 放行词法路径（:43-49）。
                let parent = resolved.parent()?;
                let real_parent = std::fs::canonicalize(parent).ok()?;
                if is_inside_root(&real_parent, &real_root) {
                    return Some(resolved);
                }
                return None;
            }
        }
    }
    None
}

/// 授权根持有者（现役 getAllowedRoots 的候选映射）。
pub struct FsReadService {
    allowed_roots: Vec<PathBuf>,
}

impl FsReadService {
    pub fn new(data_home: &Path, workspace_root: Option<&Path>) -> Self {
        let mut roots = vec![data_home.to_path_buf()];
        if let Some(root) = workspace_root {
            if !roots.contains(&root.to_path_buf()) {
                roots.push(root.to_path_buf());
            }
        }
        FsReadService {
            allowed_roots: roots,
        }
    }

    pub fn resolve(&self, file_path: &str) -> Option<PathBuf> {
        resolve_allowed_path(file_path, &self.allowed_roots)
    }
}

// ── 路由（fs.ts :89-115 两叶面） ──

fn json_error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error": message}))).into_response()
}

fn text_response(body: String) -> Response {
    let mut response = body.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

/// D12 延展：只认 `path`，未知/重复键 → 400；缺/空 → 400 missing path。
#[allow(clippy::result_large_err)] // Err 直携构造好的响应；拒绝路径非热路径。
fn extract_path(query: Option<&str>) -> Result<String, Response> {
    let pairs = crate::ws::parse_query_pairs(query.unwrap_or(""));
    let mut path: Option<String> = None;
    for (key, value) in pairs {
        if key != "path" {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                &format!("unknown fs query parameter: {key}"),
            ));
        }
        if path.is_some() {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "duplicate path query parameter",
            ));
        }
        path = Some(value);
    }
    match path {
        Some(value) if !value.is_empty() => Ok(value),
        _ => Err(json_error(StatusCode::BAD_REQUEST, "missing path")),
    }
}

/// 尺寸闸（设计登记 :87）：metadata 失败 → 404；超限 → 413。
#[allow(clippy::result_large_err)] // 同 extract_path 的响应直携纪律。
fn size_gate(path: &Path) -> Result<(), Response> {
    match std::fs::metadata(path) {
        Ok(meta) => {
            if meta.len() > MAX_FS_READ_BYTES {
                Err(json_error(StatusCode::PAYLOAD_TOO_LARGE, "file too large"))
            } else {
                Ok(())
            }
        }
        Err(_) => Err(json_error(StatusCode::NOT_FOUND, "file not found")),
    }
}

/// GET /lingxi/v1/fs/read（fs.ts :89-99）。
pub async fn fs_read_route(
    State(state): State<ServiceState>,
    RawQuery(query): RawQuery,
) -> Response {
    let path = match extract_path(query.as_deref()) {
        Ok(path) => path,
        Err(resp) => return resp,
    };
    let Some(allowed) = state.fs_read().resolve(&path) else {
        return json_error(StatusCode::FORBIDDEN, "path not allowed");
    };
    if let Err(resp) = size_gate(&allowed) {
        return resp;
    }
    // safeReadFile（safe-fs.ts :7-19）：任何读错误 → fallback null → 404；
    // utf-8 有损替换（Node utf-8 解码语义）。
    match std::fs::read(&allowed) {
        Ok(bytes) => text_response(String::from_utf8_lossy(&bytes).into_owned()),
        Err(_) => json_error(StatusCode::NOT_FOUND, "file not found"),
    }
}

/// GET /lingxi/v1/fs/read-base64（fs.ts :102-115）。
pub async fn fs_read_base64_route(
    State(state): State<ServiceState>,
    RawQuery(query): RawQuery,
) -> Response {
    let path = match extract_path(query.as_deref()) {
        Ok(path) => path,
        Err(resp) => return resp,
    };
    let Some(allowed) = state.fs_read().resolve(&path) else {
        return json_error(StatusCode::FORBIDDEN, "path not allowed");
    };
    if let Err(resp) = size_gate(&allowed) {
        return resp;
    }
    match std::fs::read(&allowed) {
        Ok(bytes) => {
            use base64::Engine as _;
            text_response(base64::engine::general_purpose::STANDARD.encode(bytes))
        }
        Err(_) => json_error(StatusCode::NOT_FOUND, "file not found"),
    }
}
