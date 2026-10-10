//! R06-T05 Round 5：桥媒体 token 读面（叶 #18）。
//!
//! 对照现役（RC-3，逐项锚点进 07_diff_ledger）：
//! - `lib/bridge/media-publisher.ts`：publish :45-82（baseUrl 未配抛
//!   :46-48、id 必填 :49-51、realPath 实探 :56 + realFilePath :158-164、
//!   白名单 :57 + _assertAllowed :113-120、必须文件 :59-60、唯一 token
//!   重试 5 次 :122-128、entry 冻结 :65-74、publicUrl :79）；resolve
//!   :84-107（过期 :88-91 与下载预算 :92-95 都回收后 None、源文件
//!   重探 :96-104）；TTL 5min :6、maxDownloads 5 :7、token 32B
//!   base64url :24。
//! - `lib/bridge/media-roots.ts`：根集合 canonical + 去重 +
//!   拒绝文件系统根（normalizeBridgeMediaRoot :57-64）。
//! - `server/routes/bridge.ts` :676-701：GET /bridge/media/:token
//!   （404 "media not found"、413 "media too large"、50MB 上限 :42、
//!   content-disposition inline=image|video（:1112-1115）+
//!   filename* RFC5987（encodeRfc5987ValueChars :1106-1110）、
//!   no-store + nosniff）。
//! - 设计登记：D5（/api/bridge/media → /lingxi/v1/bridge/media）；
//!   publish 是服务内 API（叶 #18 只断言 token 读路径；桥外发调用方
//!   属后续阶段接线）。
//!
//! 差异（台账）：现役 `_assertAllowed` 用身份键（大小写折叠）比较；
//! 候选人比较 canonical 路径原样——大小写变体只会被**拒绝更多**，
//! 绝不扩大披露面。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::sessionfiles::SessionFileView;
use crate::ServiceState;

pub const DEFAULT_TTL_MS: u64 = 5 * 60 * 1000;
pub const DEFAULT_MAX_DOWNLOADS: u32 = 5;
pub const MAX_BRIDGE_MEDIA_SIZE: u64 = 50 * 1024 * 1024;

#[derive(Debug)]
pub struct BridgeMediaError(String);

impl BridgeMediaError {
    fn new(message: impl Into<String>) -> Self {
        BridgeMediaError(message.into())
    }
}

impl std::fmt::Display for BridgeMediaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BridgeMediaError {}

/// 现役 publish 返回形状（media-publisher.ts :77-81）。
#[derive(Debug, Clone)]
pub struct PublishedMedia {
    pub token: String,
    pub public_url: String,
    pub expires_at: u64,
}

/// 现役 entry 冻结形状（:65-74）。
#[derive(Debug, Clone)]
pub struct MediaEntry {
    pub token: String,
    pub file_id: String,
    pub file_path: String,
    pub real_path: PathBuf,
    pub filename: String,
    pub mime: String,
    pub size: u64,
    pub expires_at: u64,
}

struct TokenRecord {
    entry: MediaEntry,
    downloads: u32,
}

pub struct BridgeMediaService {
    base_url: String,
    allowed_roots: Vec<PathBuf>,
    ttl_ms: u64,
    max_downloads: u32,
    now: Box<dyn Fn() -> u64 + Send + Sync>,
    tokens: Mutex<HashMap<String, TokenRecord>>,
}

impl BridgeMediaService {
    pub fn new(
        base_url: &str,
        allowed_roots: &[PathBuf],
        now: impl Fn() -> u64 + Send + Sync + 'static,
    ) -> Result<Self, BridgeMediaError> {
        Self::with_limits(
            base_url,
            allowed_roots,
            DEFAULT_TTL_MS,
            DEFAULT_MAX_DOWNLOADS,
            now,
        )
    }

    pub fn with_limits(
        base_url: &str,
        allowed_roots: &[PathBuf],
        ttl_ms: u64,
        max_downloads: u32,
        now: impl Fn() -> u64 + Send + Sync + 'static,
    ) -> Result<Self, BridgeMediaError> {
        Ok(BridgeMediaService {
            base_url: normalize_base_url(base_url)?,
            allowed_roots: normalize_allowed_roots(allowed_roots)?,
            ttl_ms,
            max_downloads: max_downloads.max(1),
            now: Box::new(now),
            tokens: Mutex::new(HashMap::new()),
        })
    }

    /// 服务内 publish（media-publisher.ts :45-82）。桥外发调用方属后续
    /// 阶段；T05 只交付该原语与 token 读路由。
    pub fn publish(
        &self,
        session_file: &SessionFileView,
    ) -> Result<PublishedMedia, BridgeMediaError> {
        if self.base_url.is_empty() {
            return Err(BridgeMediaError::new(
                "public media base URL is not configured",
            ));
        }
        if session_file.file_id.is_empty() {
            return Err(BridgeMediaError::new("session file id is required"));
        }
        let requested = if !session_file.real_path.is_empty() {
            session_file.real_path.as_str()
        } else {
            session_file.file_path.as_str()
        };
        if requested.is_empty() {
            return Err(BridgeMediaError::new("session file local path is required"));
        }
        // realFilePath（:158-164）：不在就抛，canonical 归一。
        let real_path = std::fs::canonicalize(Path::new(requested))
            .map_err(|err| BridgeMediaError::new(format!("session file real path: {err}")))?;
        self.assert_allowed(&real_path)?;
        let meta = std::fs::metadata(&real_path)
            .map_err(|err| BridgeMediaError::new(format!("session file media stat: {err}")))?;
        if !meta.is_file() {
            return Err(BridgeMediaError::new(
                "session file media source is not a file",
            ));
        }
        let token = self.unique_token()?;
        let expires_at = (self.now)() + self.ttl_ms;
        // 现役 filename = filename || label || basename（:64）。
        let filename = if !session_file.filename.is_empty() {
            session_file.filename.clone()
        } else if let Some(label) = &session_file.label {
            label.clone()
        } else {
            real_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        let mime = if session_file.mime.is_empty() {
            "application/octet-stream".to_string()
        } else {
            session_file.mime.clone()
        };
        let entry = MediaEntry {
            token: token.clone(),
            file_id: session_file.file_id.clone(),
            file_path: if session_file.file_path.is_empty() {
                real_path.to_string_lossy().into_owned()
            } else {
                session_file.file_path.clone()
            },
            real_path,
            filename,
            mime,
            size: session_file.size_bytes,
            expires_at,
        };
        self.tokens.lock().expect("media tokens").insert(
            token.clone(),
            TokenRecord {
                entry,
                downloads: 0,
            },
        );
        Ok(PublishedMedia {
            public_url: format!(
                "{}/lingxi/v1/bridge/media/{}",
                self.base_url,
                crate::preview::encode_uri_component(&token)
            ),
            token,
            expires_at,
        })
    }

    /// 现役 resolve（:84-107）：过期/超预算回收后 None；源文件重探失败
    /// None（不删 token）；成功才计一次下载。
    pub fn resolve(&self, token: &str) -> Option<MediaEntry> {
        let now = (self.now)();
        let mut tokens = self.tokens.lock().expect("media tokens");
        let expired_or_exhausted = {
            let record = tokens.get(token)?;
            record.entry.expires_at <= now || record.downloads >= self.max_downloads
        };
        if expired_or_exhausted {
            tokens.remove(token);
            return None;
        }
        let real_path = tokens.get(token)?.entry.real_path.clone();
        // 源文件重探（:96-104）：realpath 漂移 / 移出白名单 / 非文件 → None。
        let re_real = std::fs::canonicalize(&real_path).ok()?;
        if re_real != real_path {
            return None;
        }
        self.assert_allowed(&re_real).ok()?;
        let meta = std::fs::metadata(&re_real).ok()?;
        if !meta.is_file() {
            return None;
        }
        let record = tokens.get_mut(token)?;
        record.downloads += 1;
        Some(record.entry.clone())
    }

    /// 现役 revoke（:109-111）。
    pub fn revoke(&self, token: &str) -> bool {
        self.tokens
            .lock()
            .expect("media tokens")
            .remove(token)
            .is_some()
    }

    /// 现役 _assertAllowed（:113-120）：空白名单拒绝；目标必须落在
    /// 某个 canonical 根内。
    fn assert_allowed(&self, real_path: &Path) -> Result<(), BridgeMediaError> {
        if self.allowed_roots.is_empty() {
            return Err(BridgeMediaError::new("media file is outside allowed roots"));
        }
        let allowed = self
            .allowed_roots
            .iter()
            .any(|root| real_path == root || real_path.starts_with(root));
        if allowed {
            Ok(())
        } else {
            Err(BridgeMediaError::new("media file is outside allowed roots"))
        }
    }

    /// 现役 _uniqueToken（:122-128）：重试 5 次。
    fn unique_token(&self) -> Result<String, BridgeMediaError> {
        for _ in 0..5 {
            let mut bytes = [0u8; 32];
            if getrandom::getrandom(&mut bytes).is_err() {
                panic!("getrandom failed: cannot mint bridge media token");
            }
            let token =
                base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes);
            if !token.is_empty()
                && !self
                    .tokens
                    .lock()
                    .expect("media tokens")
                    .contains_key(&token)
            {
                return Ok(token);
            }
        }
        Err(BridgeMediaError::new(
            "failed to generate unique media token",
        ))
    }
}

/// 现役 normalizeBaseUrl（:131-139）：trim；空串合法（publish 时才拒）；
/// 非 http(s) 抛错；去尾斜杠。
fn normalize_base_url(base_url: &str) -> Result<String, BridgeMediaError> {
    let value = base_url.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    let rest = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
        .ok_or_else(|| BridgeMediaError::new("public media base URL must be http or https"))?;
    if rest.is_empty() {
        return Err(BridgeMediaError::new(
            "public media base URL must be http or https",
        ));
    }
    Ok(value.trim_end_matches('/').to_string())
}

/// 现役 normalizeAllowedRoots（:141-150）+ media-roots.ts
/// normalizeBridgeMediaRoot :57-64：canonical、拒绝文件系统根、去重。
/// 不存在的根直接跳过（现役 canonicalFilesystemPathSync 会抛 → 候选
/// 在收集侧忽略该根；bootstrap 收集的根均真实存在）。
fn normalize_allowed_roots(roots: &[PathBuf]) -> Result<Vec<PathBuf>, BridgeMediaError> {
    let mut out: Vec<PathBuf> = Vec::new();
    for root in roots {
        let Ok(real) = std::fs::canonicalize(root) else {
            continue;
        };
        if real.parent().is_none() {
            return Err(BridgeMediaError::new(format!(
                "media allowed root refuses filesystem root: {}",
                root.display()
            )));
        }
        if !out.contains(&real) {
            out.push(real);
        }
    }
    Ok(out)
}

/// 现役 collectBridgeMediaAllowedRoots（media-roots.ts :6-28）的候选映射：
/// 现役 = lingxiHome + 全体 agent desk 工作台 + os.homedir + 系统临时根；
/// 候选单工作区实例 = data_home（≈lingxiHome）+ workspace 根（≈agent
/// desk）+ 用户主目录 + 临时根。只收真实存在的根。
pub fn collect_bridge_media_allowed_roots(
    data_home: &Path,
    workspace_root: Option<&Path>,
) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut add = |root: PathBuf| {
        if !root.is_absolute() || roots.contains(&root) {
            return;
        }
        roots.push(root);
    };
    add(data_home.to_path_buf());
    if let Some(workspace) = workspace_root {
        add(workspace.to_path_buf());
    }
    if let Some(home) = std::env::home_dir() {
        add(home);
    }
    add(std::env::temp_dir());
    // 现役在非 Windows 上额外收 /tmp（media-roots.ts :66-71）。
    #[cfg(not(windows))]
    {
        let tmp = PathBuf::from("/tmp");
        if tmp.exists() {
            add(tmp);
        }
    }
    roots
}

/// 现役 isInlineBridgeMediaMime（bridge.ts :1112-1115）。
fn is_inline_mime(mime: &str) -> bool {
    let value = mime.to_ascii_lowercase();
    value.starts_with("image/") || value.starts_with("video/")
}

/// 现役 encodeRfc5987ValueChars（:1106-1110）：encodeURIComponent 后
/// ' ( ) → %XX 大写、* → %2A。
fn encode_rfc5987_value_chars(value: &str) -> String {
    crate::preview::encode_uri_component(value)
        .replace('\'', "%27")
        .replace('(', "%28")
        .replace(')', "%29")
        .replace('*', "%2A")
}

/// `GET /lingxi/v1/bridge/media/{token}`（Public + token 即凭证，
/// bridge.ts :676-701）。
pub async fn bridge_media_route(
    State(state): State<ServiceState>,
    AxumPath(token): AxumPath<String>,
) -> Response {
    let Some(entry) = state.bridge_media().resolve(&token) else {
        return (StatusCode::NOT_FOUND, "media not found").into_response();
    };
    let meta = match std::fs::metadata(&entry.real_path) {
        Ok(meta) if meta.is_file() => meta,
        _ => return (StatusCode::NOT_FOUND, "media not found").into_response(),
    };
    if meta.len() > MAX_BRIDGE_MEDIA_SIZE {
        return (StatusCode::PAYLOAD_TOO_LARGE, "media too large").into_response();
    }
    let bytes = match std::fs::read(&entry.real_path) {
        Ok(bytes) => bytes,
        Err(_) => return (StatusCode::NOT_FOUND, "media not found").into_response(),
    };
    let filename = if entry.filename.is_empty() {
        entry
            .real_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        entry.filename.clone()
    };
    let disposition = if is_inline_mime(&entry.mime) {
        "inline"
    } else {
        "attachment"
    };
    let mut response = bytes.into_response();
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        entry
            .mime
            .parse()
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        axum::http::header::CONTENT_LENGTH,
        HeaderValue::from_str(&meta.len().to_string()).expect("content length"),
    );
    headers.insert(
        axum::http::header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "{disposition}; filename*=UTF-8''{}",
            encode_rfc5987_value_chars(&filename)
        ))
        .expect("content disposition"),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    );
    headers.insert(
        axum::http::header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}
