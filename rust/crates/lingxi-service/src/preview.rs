//! R06-T05 Round 5：HTML 预览面（叶 #26-28）。
//!
//! 对照现役（RC-3，逐项锚点进 07_diff_ledger）：
//! - `server/routes/html-preview.ts` 全文 385 行：
//!   - 建立面 :24-79（invalid_json/missing_content 400、2MB → 413
//!     html_preview_too_large、`pv_`+16B hex id :18、32B base64url token
//!     :19、10min TTL :7、title 截 240 :65、每次请求先清扫过期 :25）。
//!   - 素材域 :42 + resolvePreviewAssetScope :182-200（sourceFilePath 必须
//!     是绝对路径且是文件；assetRoot = 请求的 sourceRootPath 含 sourceDir
//!     时为该根，否则 sourceDir 自身 realpath）。
//!   - 引用改写 rewriteLocalAssetReferences :252-268 +
//!     resolveLocalAssetReference :278-296（只改绝对本地引用：file: URL
//!     与绝对路径；相对引用交给 <base> 注入 injectAssetBase :221-227）。
//!   - CSP buildHtmlPreviewCsp :139-167 逐行镜像。
//!   - 读取面 servePreview :89-107（无效 id/token/过期一律 404 空体；
//!     五响应头 + CORP cross-origin）与 servePreviewAsset :109-131
//!     （resolveAssetPath :366-384 拒绝 空段/./../绝对/反斜杠/NUL/symlink
//!     逃逸，50MB 上限 :9/:124，CORP same-origin）。
//! - 路由前缀按 D5：/api/preview/html → /lingxi/v1/preview/html；
//!   /preview/html/:id → /lingxi/v1/preview/html/{id}。
//! - auth 分类（design.md 路由表）：POST 建立面 Scope("chat")；
//!   GET|HEAD 读取/素材面 Public + token 即凭证（无效/过期 404，
//!   绝不回 401/403 泄露存在性）。

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use axum::extract::{OriginalUri, Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine as _;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde_json::Value;

use crate::{EndpointError, ServiceState};

pub const DEFAULT_TTL_MS: u64 = 10 * 60 * 1000;
pub const DEFAULT_MAX_CONTENT_BYTES: usize = 2 * 1024 * 1024;
pub const DEFAULT_MAX_ASSET_BYTES: u64 = 50 * 1024 * 1024;

/// JS `encodeURIComponent` 的保留集（RFC 3986 unreserved + ! ' ( ) * ~）。
const JS_URI_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// 现役 encodeURIComponent 逐段编码（encodeAssetRoutePath :229-235）。
pub(crate) fn encode_uri_component(value: &str) -> String {
    utf8_percent_encode(value, JS_URI_COMPONENT).to_string()
}

/// 现役 decodeURIComponent（extractAssetPath :353-356）：畸形序列 → 调用方
/// 落 catch 给空串。
fn decode_uri_component(value: &str) -> Option<String> {
    let bytes = percent_decode_strict(value)?;
    String::from_utf8(bytes).ok()
}

/// 现役 decodeURI（decodePathname :321-327）：保留子界 ; / ? : @ & = + $ , #
/// 不解码；畸形 → 原样返回。
fn decode_uri(value: &str) -> String {
    const RESERVED: &[u8] = b";/?:@&=+$,#";
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let mut malformed = false;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                malformed = true;
                break;
            }
            let hi = hex_val(bytes[i + 1]);
            let lo = hex_val(bytes[i + 2]);
            match (hi, lo) {
                (Some(hi), Some(lo)) => {
                    let byte = (hi << 4) | lo;
                    if RESERVED.contains(&byte) {
                        out.extend_from_slice(&bytes[i..i + 3]);
                    } else {
                        out.push(byte);
                    }
                    i += 3;
                }
                _ => {
                    malformed = true;
                    break;
                }
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    if malformed {
        return value.to_string();
    }
    String::from_utf8(out).unwrap_or_else(|_| value.to_string())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// 严格百分号解码（%XX 全部解码；任何畸形 → None）。
fn percent_decode_strict(value: &str) -> Option<Vec<u8>> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let hi = hex_val(bytes[i + 1])?;
            let lo = hex_val(bytes[i + 2])?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(out)
}

// ── 随机身份（pv_ id :18 / 32B token :19） ──

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    if getrandom::getrandom(&mut buf).is_err() {
        // 与 resourceio uuid_v4 同一纪律：系统熵失败无降级余地。
        panic!("getrandom failed: cannot mint preview id/token");
    }
    buf
}

fn random_preview_id() -> String {
    let bytes = random_bytes::<16>();
    let mut out = String::with_capacity(3 + 32);
    out.push_str("pv_");
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn random_preview_token() -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes::<32>())
}

// ── 服务 ──

#[derive(Debug)]
pub enum PreviewError {
    MissingContent,
    TooLarge,
}

impl PreviewError {
    pub fn code(&self) -> &'static str {
        match self {
            PreviewError::MissingContent => "missing_content",
            PreviewError::TooLarge => "html_preview_too_large",
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            PreviewError::MissingContent => StatusCode::BAD_REQUEST,
            PreviewError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        }
    }

    fn into_response(self) -> Response {
        EndpointError::resource_failure(self.status(), self.code(), self.code()).into_response()
    }
}

#[derive(Debug)]
pub struct CreatedPreview {
    pub id: String,
    pub url_path: String,
    pub expires_at: u64,
}

#[derive(Debug)]
pub struct ServedPreview {
    pub content: String,
    pub csp: String,
}

#[derive(Debug)]
pub struct ServedAsset {
    pub real_path: PathBuf,
    pub mime: String,
    pub size: u64,
}

struct PreviewEntry {
    token: String,
    content: String,
    asset_root: Option<PathBuf>,
    csp: String,
    expires_at: u64,
}

struct AssetScope {
    asset_root: PathBuf,
    source_relative_dir: String,
}

pub struct PreviewService {
    ttl_ms: u64,
    max_content_bytes: usize,
    max_asset_bytes: u64,
    now: Box<dyn Fn() -> u64 + Send + Sync>,
    previews: Mutex<HashMap<String, PreviewEntry>>,
}

impl PreviewService {
    pub fn new(now: impl Fn() -> u64 + Send + Sync + 'static) -> Self {
        Self::with_limits(
            DEFAULT_TTL_MS,
            DEFAULT_MAX_CONTENT_BYTES,
            DEFAULT_MAX_ASSET_BYTES,
            now,
        )
    }

    pub fn with_limits(
        ttl_ms: u64,
        max_content_bytes: usize,
        max_asset_bytes: u64,
        now: impl Fn() -> u64 + Send + Sync + 'static,
    ) -> Self {
        PreviewService {
            ttl_ms,
            max_content_bytes,
            max_asset_bytes,
            now: Box::new(now),
            previews: Mutex::new(HashMap::new()),
        }
    }

    /// POST 建立面（html-preview.ts :24-79）。
    pub fn create(&self, body: &Value, origin: &str) -> Result<CreatedPreview, PreviewError> {
        self.cleanup_expired();
        let content = match body.get("content").and_then(Value::as_str) {
            Some(content) => content,
            None => return Err(PreviewError::MissingContent),
        };
        if content.len() > self.max_content_bytes {
            return Err(PreviewError::TooLarge);
        }
        let id = random_preview_id();
        let token = random_preview_token();
        let asset_scope = resolve_preview_asset_scope(
            body.get("sourceFilePath").and_then(Value::as_str),
            body.get("sourceRootPath").and_then(Value::as_str),
        );
        let asset_root_url = asset_scope
            .as_ref()
            .map(|_| build_preview_asset_url(origin, &id, &token, "", true));
        let asset_base_url = asset_scope.as_ref().map(|scope| {
            build_preview_asset_url(origin, &id, &token, &scope.source_relative_dir, true)
        });
        let served_content = match (&asset_scope, &asset_base_url) {
            (Some(scope), Some(base)) => inject_asset_base(
                &rewrite_local_asset_references(content, &scope.asset_root, origin, &id, &token),
                base,
            ),
            _ => content.to_string(),
        };
        let expires_at = (self.now)() + self.ttl_ms;
        let csp = build_html_preview_csp(asset_root_url.as_deref());
        let entry = PreviewEntry {
            token: token.clone(),
            content: served_content,
            asset_root: asset_scope.map(|scope| scope.asset_root),
            csp,
            expires_at,
        };
        self.previews
            .lock()
            .expect("preview store")
            .insert(id.clone(), entry);
        let url_path = format!(
            "/lingxi/v1/preview/html/{}?previewToken={}",
            encode_uri_component(&id),
            encode_uri_component(&token),
        );
        Ok(CreatedPreview {
            id,
            url_path,
            expires_at,
        })
    }

    /// 读取面（servePreview :89-107）：无效 id/token/过期一律 None。
    pub fn serve(&self, id: &str, token: &str) -> Option<ServedPreview> {
        self.cleanup_expired();
        let previews = self.previews.lock().expect("preview store");
        let entry = previews.get(id)?;
        if entry.token != token {
            return None;
        }
        Some(ServedPreview {
            content: entry.content.clone(),
            csp: entry.csp.clone(),
        })
    }

    /// 素材面（servePreviewAsset :109-131）：raw_rel 是已剥离前缀且
    /// decodeURIComponent 后的相对路径。
    pub fn serve_asset(&self, id: &str, token: &str, raw_rel: &str) -> Option<ServedAsset> {
        self.cleanup_expired();
        let asset_root = {
            let previews = self.previews.lock().expect("preview store");
            let entry = previews.get(id)?;
            if entry.token != token {
                return None;
            }
            entry.asset_root.clone()?
        };
        let asset_path = resolve_asset_path(&asset_root, raw_rel)?;
        let meta = std::fs::metadata(&asset_path).ok()?;
        if !meta.is_file() || meta.len() > self.max_asset_bytes {
            return None;
        }
        Some(ServedAsset {
            mime: guess_mime(&asset_path).to_string(),
            size: meta.len(),
            real_path: asset_path,
        })
    }

    fn cleanup_expired(&self) {
        let now = (self.now)();
        self.previews
            .lock()
            .expect("preview store")
            .retain(|_, entry| entry.expires_at > now);
    }
}

// ── CSP（buildHtmlPreviewCsp :139-167 逐行镜像） ──

fn csp_source_from_asset_base(asset_base_url: &str) -> Option<String> {
    // 现役只认 http(s)，剥掉 query/hash（cspSourceFromAssetBase :169-180）。
    if !(asset_base_url.starts_with("http://") || asset_base_url.starts_with("https://")) {
        return None;
    }
    let mut end = asset_base_url.len();
    for (idx, ch) in asset_base_url.char_indices() {
        if ch == '?' || ch == '#' {
            end = idx;
            break;
        }
    }
    Some(asset_base_url[..end].to_string())
}

pub fn build_html_preview_csp(asset_base_url: Option<&str>) -> String {
    let asset_source = asset_base_url.and_then(csp_source_from_asset_base);
    let base_sources = match &asset_source {
        Some(source) => vec![source.clone()],
        None => vec!["'self'".to_string()],
    };
    let mut script_sources = vec!["'unsafe-inline'".to_string(), "https:".to_string()];
    let mut style_sources = vec!["'unsafe-inline'".to_string(), "https:".to_string()];
    let mut font_sources = vec!["https:".to_string(), "data:".to_string()];
    let image_sources: Vec<String> = match &asset_source {
        Some(source) => vec![
            source.clone(),
            "https:".into(),
            "data:".into(),
            "blob:".into(),
        ],
        None => vec![
            "'self'".into(),
            "https:".into(),
            "data:".into(),
            "blob:".into(),
        ],
    };
    let media_sources: Vec<String> = match &asset_source {
        Some(source) => vec![
            source.clone(),
            "https:".into(),
            "data:".into(),
            "blob:".into(),
        ],
        None => vec![
            "'self'".into(),
            "https:".into(),
            "data:".into(),
            "blob:".into(),
        ],
    };
    if let Some(source) = &asset_source {
        script_sources.push(source.clone());
        style_sources.push(source.clone());
        font_sources.push(source.clone());
    }
    [
        "default-src 'none'".to_string(),
        format!("base-uri {}", base_sources.join(" ")),
        "form-action 'none'".to_string(),
        "object-src 'none'".to_string(),
        "connect-src 'none'".to_string(),
        format!("script-src {}", script_sources.join(" ")),
        format!("style-src {}", style_sources.join(" ")),
        format!("font-src {}", font_sources.join(" ")),
        format!("img-src {}", image_sources.join(" ")),
        format!("media-src {}", media_sources.join(" ")),
        "frame-ancestors 'self' file: http://127.0.0.1:* http://localhost:*".to_string(),
    ]
    .join("; ")
}

// ── 素材域解析（resolvePreviewAssetScope :182-200） ──

fn resolve_preview_asset_scope(
    source_file_path: Option<&str>,
    source_root_path: Option<&str>,
) -> Option<AssetScope> {
    let source_file = source_file_path?;
    if !Path::new(source_file).is_absolute() {
        return None;
    }
    let meta = std::fs::metadata(source_file).ok()?;
    if !meta.is_file() {
        return None;
    }
    let source_dir = std::fs::canonicalize(Path::new(source_file).parent()?).ok()?;
    let requested_root = resolve_requested_asset_root(source_root_path);
    let asset_root = match requested_root {
        Some(root) if is_inside_root(&source_dir, &root) => root,
        _ => source_dir.clone(),
    };
    let source_relative_dir = to_asset_route_path(&rel_slash(&asset_root, &source_dir));
    Some(AssetScope {
        asset_root,
        source_relative_dir,
    })
}

fn resolve_requested_asset_root(source_root_path: Option<&str>) -> Option<PathBuf> {
    let root = source_root_path?;
    if !Path::new(root).is_absolute() {
        return None;
    }
    let meta = std::fs::metadata(root).ok()?;
    if !meta.is_dir() {
        return None;
    }
    std::fs::canonicalize(root).ok()
}

/// 现役 path.relative 的候选子集（两侧同为绝对路径时）。
fn rel_slash(from: &Path, to: &Path) -> String {
    match to.strip_prefix(from) {
        Ok(rel) => rel
            .components()
            .filter_map(|c| match c {
                Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/"),
        Err(_) => String::new(),
    }
}

/// 现役 toAssetRoutePath（:237-240）："." → ""，去首尾斜杠。
fn to_asset_route_path(rel: &str) -> String {
    let normalized = rel.replace('\\', "/");
    if normalized == "." {
        return String::new();
    }
    normalized.trim_matches('/').to_string()
}

/// 现役 encodeAssetRoutePath（:229-235）：逐段 encodeURIComponent。
fn encode_asset_route_path(rel: &str) -> String {
    to_asset_route_path(rel)
        .split('/')
        .filter(|part| !part.is_empty())
        .map(encode_uri_component)
        .collect::<Vec<_>>()
        .join("/")
}

/// 现役 buildPreviewAssetUrl（:242-250）→ 候选 D5 前缀。
fn build_preview_asset_url(
    origin: &str,
    id: &str,
    token: &str,
    rel: &str,
    trailing_slash: bool,
) -> String {
    let encoded = encode_asset_route_path(rel);
    let suffix = if encoded.is_empty() {
        String::new()
    } else {
        format!("/{encoded}")
    };
    let slash = if trailing_slash { "/" } else { "" };
    format!(
        "{}/lingxi/v1/preview/html/{}/assets/{}{}{}",
        origin.trim_end_matches('/'),
        encode_uri_component(id),
        encode_uri_component(token),
        suffix,
        slash
    )
}

// ── 引用改写（rewriteLocalAssetReferences :252-268） ──

fn escape_html_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_unquoted_attr(value: &str) -> String {
    escape_html_attr(value)
        .replace('`', "&#96;")
        .chars()
        .map(|ch| {
            if ch.is_whitespace() {
                format!("&#{};", ch as u32)
            } else {
                ch.to_string()
            }
        })
        .collect()
}

fn rewrite_local_asset_references(
    content: &str,
    asset_root: &Path,
    origin: &str,
    id: &str,
    token: &str,
) -> String {
    // 现役正则 /\b(src|href|poster)\s*=\s*("([^"]*)"|'([^']*)'|([^\s"'=<>`]+))/gi
    let re =
        regex::Regex::new(r#"(?i)\b(src|href|poster)\s*=\s*("([^"]*)"|'([^']*)'|([^\s"'=<>`]+))"#)
            .expect("asset attr regex");
    re.replace_all(content, |caps: &regex::Captures<'_>| {
        let attr_name = caps.get(1).expect("attr").as_str();
        let (value, quote) = if let Some(m) = caps.get(3) {
            (m.as_str(), '"')
        } else if let Some(m) = caps.get(4) {
            (m.as_str(), '\'')
        } else if let Some(m) = caps.get(5) {
            (m.as_str(), '\0')
        } else {
            ("", '"')
        };
        let Some(rewritten) = rewrite_local_asset_url(value, asset_root, origin, id, token) else {
            return caps.get(0).expect("full").as_str().to_string();
        };
        if rewritten == value {
            return caps.get(0).expect("full").as_str().to_string();
        }
        match quote {
            '"' => format!("{attr_name}=\"{}\"", escape_html_attr(&rewritten)),
            '\'' => format!("{attr_name}='{}'", escape_html_attr(&rewritten)),
            _ => format!("{attr_name}={}", escape_unquoted_attr(&rewritten)),
        }
    })
    .into_owned()
}

fn rewrite_local_asset_url(
    raw_value: &str,
    asset_root: &Path,
    origin: &str,
    id: &str,
    token: &str,
) -> Option<String> {
    let (file_path, suffix) = resolve_local_asset_reference(raw_value, asset_root)?;
    let rel = rel_slash(asset_root, &file_path);
    let url = build_preview_asset_url(origin, id, token, &rel, false);
    Some(format!("{url}{suffix}"))
}

/// 现役 resolveLocalAssetReference（:278-296）。
fn resolve_local_asset_reference(raw_value: &str, asset_root: &Path) -> Option<(PathBuf, String)> {
    let value = raw_value.trim();
    if value.is_empty() || value.starts_with('#') || value.starts_with("//") {
        return None;
    }
    // (?i)^(https?|data|blob|mailto|tel):
    let lower = value.to_ascii_lowercase();
    for scheme in ["http:", "https:", "data:", "blob:", "mailto:", "tel:"] {
        if lower.starts_with(scheme) {
            return None;
        }
    }
    if lower.starts_with("file:") {
        let (path, suffix) = file_url_to_path_and_suffix(value)?;
        let resolved = resolve_asset_file_for_local_path(asset_root, &path)?;
        return Some((resolved, suffix));
    }
    let (pathname, suffix) = split_reference_suffix(value);
    let decoded = decode_uri(&pathname);
    if !is_absolute_local_path(&decoded) {
        return None;
    }
    let resolved = resolve_asset_file_for_local_path(asset_root, &decoded)?;
    Some((resolved, suffix))
}

/// 现役 splitReferenceSuffix（:298-305）。
fn split_reference_suffix(value: &str) -> (String, String) {
    let hash = value.find('#');
    let query = value.find('?');
    let split_at = match (hash, query) {
        (Some(h), Some(q)) => h.min(q),
        (Some(h), None) => h,
        (None, Some(q)) => q,
        (None, None) => return (value.to_string(), String::new()),
    };
    (value[..split_at].to_string(), value[split_at..].to_string())
}

/// 现役 fileUrlToPathAndSuffix（:307-319）的候选子集（无 url crate：
/// file: URL 结构固定，手解）。
fn file_url_to_path_and_suffix(value: &str) -> Option<(String, String)> {
    // 前缀已确认 file:（大小写不敏感）。
    let rest = &value[5..];
    let (without_suffix, suffix) = {
        let hash = rest.find('#');
        let query = rest.find('?');
        let split_at = match (hash, query) {
            (Some(h), Some(q)) => h.min(q),
            (Some(h), None) => h,
            (None, Some(q)) => q,
            (None, None) => rest.len(),
        };
        (&rest[..split_at], rest[split_at..].to_string())
    };
    let (host, raw_path) = if let Some(after_slashes) = without_suffix.strip_prefix("//") {
        match after_slashes.find('/') {
            Some(idx) => (&after_slashes[..idx], &after_slashes[idx..]),
            None => (after_slashes, ""),
        }
    } else {
        ("", without_suffix)
    };
    let decoded = decode_uri_component(raw_path).unwrap_or_else(|| raw_path.to_string());
    let file_path = if !host.is_empty() {
        format!("//{host}{decoded}")
    } else {
        // Windows 盘符修正：/^\/([A-Za-z]:\/)/ → $1。
        let bytes = decoded.as_bytes();
        if bytes.len() >= 4
            && bytes[0] == b'/'
            && bytes[1].is_ascii_alphabetic()
            && bytes[2] == b':'
            && bytes[3] == b'/'
        {
            decoded[1..].to_string()
        } else {
            decoded
        }
    };
    Some((file_path, suffix))
}

/// 现役 isAbsoluteLocalPath（:329-331）：POSIX 绝对、win32 绝对或 // 开头。
fn is_absolute_local_path(value: &str) -> bool {
    value.starts_with('/') || is_win32_absolute(value)
}

fn is_win32_absolute(value: &str) -> bool {
    let bytes = value.as_bytes();
    if value.starts_with("//") || value.starts_with("\\\\") {
        return true;
    }
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

/// 现役 resolveAssetFileForLocalPath（:333-344）：非文件/symlink → None；
/// realpath 必须在素材根内。
fn resolve_asset_file_for_local_path(asset_root: &Path, file_path: &str) -> Option<PathBuf> {
    let candidate = PathBuf::from(file_path);
    let meta = std::fs::symlink_metadata(&candidate).ok()?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return None;
    }
    let real = std::fs::canonicalize(&candidate).ok()?;
    if !is_inside_root(&real, asset_root) {
        return None;
    }
    Some(real)
}

/// 现役 isInsideRoot（:360-364）。
fn is_inside_root(candidate: &Path, root: &Path) -> bool {
    candidate == root || candidate.starts_with(root)
}

/// 现役 resolveAssetPath（:366-384）：空段/./../绝对/反斜杠/NUL 全拒，
/// symlink 末段拒，realpath 必须在根内。
fn resolve_asset_path(source_dir: &Path, relative_path: &str) -> Option<PathBuf> {
    if relative_path.is_empty() || relative_path.contains('\0') || relative_path.contains('\\') {
        return None;
    }
    if Path::new(relative_path).is_absolute() {
        return None;
    }
    let parts: Vec<&str> = relative_path.split('/').collect();
    if parts
        .iter()
        .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        return None;
    }
    let candidate = source_dir.join(relative_path);
    if !is_inside_root(&candidate, source_dir) {
        return None;
    }
    let meta = std::fs::symlink_metadata(&candidate).ok()?;
    if meta.file_type().is_symlink() {
        return None;
    }
    let real = std::fs::canonicalize(&candidate).ok()?;
    if !is_inside_root(&real, source_dir) {
        return None;
    }
    Some(real)
}

/// 现役 injectAssetBase（:221-227）。
fn inject_asset_base(content: &str, asset_base_url: &str) -> String {
    let base_tag = format!("<base href=\"{}\">", escape_html_attr(asset_base_url));
    let re = regex::Regex::new(r"(?i)<head\b[^>]*>").expect("head regex");
    if let Some(m) = re.find(content) {
        let mut out = String::with_capacity(content.len() + base_tag.len());
        out.push_str(&content[..m.end()]);
        out.push_str(&base_tag);
        out.push_str(&content[m.end()..]);
        out
    } else {
        format!("{base_tag}{content}")
    }
}

/// 现役 guessMime（server/http/file-content.ts MIME_BY_EXT）。
fn guess_mime(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("txt") => "text/plain; charset=utf-8",
        Some("md") | Some("markdown") => "text/markdown; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("js") | Some("mjs") | Some("cjs") => "text/javascript; charset=utf-8",
        Some("ts") | Some("tsx") => "text/typescript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("wasm") => "application/wasm",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mov") => "video/quicktime",
        Some("pdf") => "application/pdf",
        _ => "application/octet-stream",
    }
}

// ── 路由（D5 前缀 /lingxi/v1/preview/html*） ──

/// `POST /lingxi/v1/preview/html`（Scope("chat")）。
pub async fn create_preview_route(
    State(state): State<ServiceState>,
    headers: HeaderMap,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let body = match body {
        Ok(Json(value)) => value,
        Err(rejection) => return EndpointError::from_json_rejection(&rejection).into_response(),
    };
    // 现役取 requestUrl.origin（:43）；候选人用 Host 头重建（http 明文面）。
    let origin = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|host| format!("http://{host}"))
        .unwrap_or_else(|| "http://localhost".to_string());
    match state.preview().create(&body, &origin) {
        Ok(created) => {
            let preview_url = format!("{}{}", origin, created.url_path);
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "id": created.id,
                    "previewUrl": preview_url,
                    "expiresAt": created.expires_at,
                })),
            )
                .into_response()
        }
        Err(err) => err.into_response(),
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct PreviewQuery {
    #[serde(rename = "previewToken")]
    preview_token: Option<String>,
}

/// `GET|HEAD /lingxi/v1/preview/html/{id}`（Public + token 即凭证，
/// 无效/过期 404 空体——servePreview :89-107）。
pub async fn serve_preview_route(
    State(state): State<ServiceState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<PreviewQuery>,
) -> Response {
    let token = query.preview_token.unwrap_or_default();
    let Some(served) = state.preview().serve(&id, &token) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // HEAD 由 axum 的 get() 路由自动去体（headers 与 GET 一致）。
    (
        [
            (axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (
                axum::http::header::CONTENT_SECURITY_POLICY,
                served.csp.as_str(),
            ),
            (axum::http::header::REFERRER_POLICY, "no-referrer"),
            (axum::http::header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (axum::http::header::CACHE_CONTROL, "no-store"),
            (
                axum::http::header::HeaderName::from_static("cross-origin-resource-policy"),
                "cross-origin",
            ),
        ],
        served.content,
    )
        .into_response()
}

/// `GET|HEAD /lingxi/v1/preview/html/{id}/assets/{token}/{*path}`（Public +
/// 路径内 token——servePreviewAsset :109-131）。
pub async fn serve_preview_asset_route(
    State(state): State<ServiceState>,
    OriginalUri(uri): OriginalUri,
    AxumPath((id, token, _wild)): AxumPath<(String, String, String)>,
) -> Response {
    // 现役 extractAssetPath（:350-358）：从原始请求路径剥前缀后
    // decodeURIComponent；畸形编码 → 空串（随后被 resolveAssetPath 拒）。
    let prefix = format!("/lingxi/v1/preview/html/{id}/assets/{token}");
    let raw = uri
        .path()
        .strip_prefix(&prefix)
        .map(|rest| rest.strip_prefix('/').unwrap_or(rest))
        .unwrap_or("");
    let rel = decode_uri_component(raw).unwrap_or_default();
    let Some(asset) = state.preview().serve_asset(&id, &token, &rel) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let bytes = match std::fs::read(&asset.real_path) {
        Ok(bytes) => bytes,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    let mut response = bytes.into_response();
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        asset.mime.parse().expect("mime header value"),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    headers.insert(
        axum::http::header::X_CONTENT_TYPE_OPTIONS,
        axum::http::HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        axum::http::header::HeaderName::from_static("cross-origin-resource-policy"),
        axum::http::HeaderValue::from_static("same-origin"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csp_baseline_matches_incumbent() {
        let csp = build_html_preview_csp(None);
        assert_eq!(
            csp,
            "default-src 'none'; base-uri 'self'; form-action 'none'; object-src 'none'; \
             connect-src 'none'; script-src 'unsafe-inline' https:; \
             style-src 'unsafe-inline' https:; font-src https: data:; \
             img-src 'self' https: data: blob:; media-src 'self' https: data: blob:; \
             frame-ancestors 'self' file: http://127.0.0.1:* http://localhost:*"
        );
    }

    #[test]
    fn uri_component_encoding_mirrors_js() {
        // encodeURIComponent 保留 A-Za-z0-9 - _ . ! ~ * ' ( )。
        assert_eq!(
            encode_uri_component("a b/要?q#"),
            "a%20b%2F%E8%A6%81%3Fq%23"
        );
        assert_eq!(encode_uri_component("-_.!~*'()"), "-_.!~*'()");
        // decodeURIComponent 全解。
        assert_eq!(
            decode_uri_component("a%20b%2F%E8%A6%81").as_deref(),
            Some("a b/要")
        );
        // decodeURI 保留 ; / ? : @ & = + $ , #（%2F/%3F/%23 原样保留）。
        assert_eq!(decode_uri("a%20b%2Fc%3F%23d"), "a b%2Fc%3F%23d");
        assert_eq!(decode_uri("%zz"), "%zz");
    }

    #[test]
    fn file_url_parsing() {
        let (path, suffix) =
            file_url_to_path_and_suffix("file:///tmp/a%20b.png?x=1#f").expect("file url");
        assert_eq!(path, "/tmp/a b.png");
        assert_eq!(suffix, "?x=1#f");
        let (path, _) = file_url_to_path_and_suffix("file://host/share/x").expect("unc");
        assert_eq!(path, "//host/share/x");
    }
}
