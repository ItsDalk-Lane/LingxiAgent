//! 远端资源只读传输：逐跳校验公网地址、固定本次DNS结果并限制时间和体积。
use serde_json::{json, Value};
use std::{
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    time::Duration,
};

pub const MAX_URL_BYTES: usize = 8 * 1024 * 1024;
pub struct UrlRead {
    pub bytes: Vec<u8>,
    pub meta: Value,
}
pub struct UrlProvider {
    root: PathBuf,
    fixture_http: Option<SocketAddr>,
}
impl UrlProvider {
    pub fn new(home: &Path) -> Self {
        Self {
            root: home.join("resource-url-cache"),
            fixture_http: None,
        }
    }
    /// 仅可信宿主测试构造外部HTTP传输替身；请求正文和生产构造器不能开启。
    /// 固定保留域名与本机端口，任何重定向仍逐跳回到真实公网闸。
    #[doc(hidden)]
    pub fn with_fixture_http(home: &Path, address: SocketAddr) -> Result<Self, String> {
        if !address.ip().is_loopback() || address.port() == 0 {
            return Err("fixture transport must use a bound loopback socket".into());
        }
        Ok(Self {
            root: home.join("resource-url-cache"),
            fixture_http: Some(address),
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub async fn read(&self, source: &str) -> Result<UrlRead, String> {
        tokio::time::timeout(Duration::from_secs(10), self.read_inner(source))
            .await
            .map_err(|_| "url_read_timeout: remote read exceeded 10 seconds".to_string())?
    }
    async fn read_inner(&self, source: &str) -> Result<UrlRead, String> {
        let original = reqwest::Url::parse(source).map_err(|e| format!("invalid_url: {e}"))?;
        let mut current = original.clone();
        for redirect in 0..=3 {
            let sockets = if current.scheme() == "http"
                && current.host_str() == Some("r06-fixture.invalid")
                && current.username().is_empty()
                && current.password().is_none()
                && self
                    .fixture_http
                    .is_some_and(|a| current.port_or_known_default() == Some(a.port()))
            {
                vec![self.fixture_http.ok_or("fixture transport unavailable")?]
            } else {
                public_addresses(&current).await?
            };
            let host = current.host_str().ok_or("invalid_url: missing host")?;
            // 禁止代理在校验后重新解析或替换目的地；TLS仍验证原始域名。
            let client = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .resolve_to_addrs(host, &sockets)
                .timeout(Duration::from_secs(10))
                .build()
                .map_err(|e| e.to_string())?;
            let mut response = client
                .get(current.clone())
                .send()
                .await
                .map_err(|e| format!("url_fetch_failed: {e}"))?;
            if [301, 302, 303, 307, 308].contains(&response.status().as_u16()) {
                if redirect == 3 {
                    return Err("url_redirect_limit: more than three redirects".into());
                }
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or("invalid_url_redirect: missing location")?;
                current = current
                    .join(location)
                    .map_err(|e| format!("invalid_url_redirect: {e}"))?;
                continue;
            }
            if !response.status().is_success() {
                return Err(format!(
                    "url_fetch_failed: HTTP {}",
                    response.status().as_u16()
                ));
            }
            if response
                .content_length()
                .is_some_and(|n| n > MAX_URL_BYTES as u64)
            {
                return Err("url_response_too_large: above 8 MiB".into());
            }
            let mime_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(|value| {
                    value
                        .split(';')
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .to_ascii_lowercase()
                });
            let etag = response
                .headers()
                .get(reqwest::header::ETAG)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|e| format!("url_fetch_failed: {e}"))?
            {
                if bytes.len().saturating_add(chunk.len()) > MAX_URL_BYTES {
                    return Err("url_response_too_large: above 8 MiB".into());
                }
                bytes.extend_from_slice(&chunk);
            }
            let meta = json!({"resourceKey":format!("url:{source}"),"resource":{"kind":"url","provider":"url","url":source,"displayName":current.as_str()},"originalUrl":source,"finalUrl":current.as_str(),"fetchedAt":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis,true),"mimeType":mime_type,"version":{"size":bytes.len(),"etag":etag,"sha256":lingxi_kernel::session_tree::sha256_hex(&bytes)}});
            return Ok(UrlRead { bytes, meta });
        }
        Err("url_redirect_limit".into())
    }
    pub fn stage(
        &self,
        source: &str,
        mut read: UrlRead,
        safety: &crate::file_safety::FileSafety,
        session: &str,
        active: impl Fn() -> bool,
    ) -> Result<Value, String> {
        if !active() {
            return Err("resource_materialize_cancelled".into());
        }
        // 只有已完整校验的字节才可建立副本；目录名由可信宿主固定，模型不能指定。
        let parent = self.root.parent().ok_or("invalid staging root")?;
        let parent = crate::filetools::fsio::DirFd::open(parent).map_err(|e| e.to_string())?;
        let _directory = parent
            .child_directory(self.root.file_name().ok_or("invalid staging root")?, true)
            .map_err(|e| e.to_string())?;
        let root = self.root.canonicalize().map_err(|e| e.to_string())?;
        if root != self.root {
            return Err("resource_staging_redirected".into());
        }
        let path = root.join(format!(
            "{}-{}{}",
            lingxi_kernel::session_tree::sha256_hex(source.as_bytes()),
            crate::auth::hex_random_public(8).map_err(|e| e.to_string())?,
            extension(source)
        ));
        safety
            .create(session, &path, &read.bytes, || {
                active() && self.root.canonicalize().ok().as_deref() == Some(self.root.as_path())
            })
            .map_err(|e| format!("resource_staging_failed: {e}"))?;
        read.meta["filePath"] = json!(path);
        read.meta["isDirectory"] = json!(false);
        Ok(read.meta)
    }
}
fn extension(source: &str) -> String {
    reqwest::Url::parse(source)
        .ok()
        .and_then(|u| {
            Path::new(u.path())
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_string)
        })
        .filter(|e| !e.is_empty() && e.len() <= 12 && e.bytes().all(|b| b.is_ascii_alphanumeric()))
        .map(|e| format!(".{e}"))
        .unwrap_or_else(|| ".bin".into())
}
pub(crate) fn blocked(ip: IpAddr) -> bool {
    lingxi_adapters::models::egress::socket_addr_is_guarded(&SocketAddr::new(ip, 0))
}
pub(crate) async fn public_addresses(url: &reqwest::Url) -> Result<Vec<SocketAddr>, String> {
    if !["http", "https"].contains(&url.scheme())
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("invalid_url_scheme_or_credentials".into());
    }
    let host = url.host_str().ok_or("invalid_url: missing host")?;
    if host.eq_ignore_ascii_case("localhost") || host.to_ascii_lowercase().ends_with(".localhost") {
        return Err("blocked_private_url".into());
    }
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let port = url
        .port_or_known_default()
        .ok_or("invalid_url: missing port")?;
    let sockets = if let Ok(ip) = host.parse::<IpAddr>() {
        vec![SocketAddr::new(ip, port)]
    } else {
        tokio::net::lookup_host((host, port))
            .await
            .map_err(|e| format!("url_dns_failed: {e}"))?
            .collect::<Vec<_>>()
    };
    if sockets.is_empty() || sockets.iter().any(|addr| blocked(addr.ip())) {
        return Err("blocked_private_url".into());
    }
    Ok(sockets)
}
