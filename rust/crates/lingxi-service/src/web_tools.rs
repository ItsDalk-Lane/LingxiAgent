//! 搜索与网页读取共用正式网络平面；网页正文只在当前会话的文件登记内续读。
use crate::{
    credentials::search::{self, SearchCredentials},
    memory::MemoryService,
    sessionfiles::SessionFileService,
    ServiceClock, ServiceState,
};
use lingxi_adapters::models::{
    dispatch::HttpTimeouts,
    network::{NetworkClient, NetworkPlane},
};
use lingxi_kernel::{
    browser::BrowserPort,
    ports::{ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, ToolSuccess},
    toolcatalog::{
        Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
        ToolManifest, ToolOrigin,
    },
    RunContext,
};
use lingxi_protocol::{ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
    time::{Duration, Instant},
};

const MAX_BODY: usize = 8 * 1024 * 1024;
const API_TIMEOUT: Duration = Duration::from_secs(30);
const FETCH_TIMEOUT: Duration = Duration::from_secs(15);
#[derive(Debug, Clone)]
pub struct WebError {
    pub code: String,
    pub message: String,
    pub retry_after_ms: Option<u64>,
}
impl WebError {
    fn new(code: &str, message: impl ToString) -> Self {
        Self {
            code: code.into(),
            message: message.to_string(),
            retry_after_ms: None,
        }
    }
}
impl std::fmt::Display for WebError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for WebError {}
struct CancelFlag(Arc<AtomicBool>);
impl Drop for CancelFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
struct RateState {
    next: Instant,
    cooldown: Instant,
    failures: u32,
}
struct Limiter {
    slots: Arc<tokio::sync::Semaphore>,
    state: tokio::sync::Mutex<RateState>,
}
impl Limiter {
    fn new(cap: usize) -> Self {
        Self {
            slots: Arc::new(tokio::sync::Semaphore::new(cap)),
            state: tokio::sync::Mutex::new(RateState {
                next: Instant::now(),
                cooldown: Instant::now(),
                failures: 0,
            }),
        }
    }
}

pub struct WebTools {
    memory: Arc<MemoryService>,
    files: Arc<SessionFileService>,
    clock: Arc<dyn ServiceClock>,
    network: NetworkClient,
    browser: RwLock<Option<Arc<dyn BrowserPort>>>,
    limits: BTreeMap<&'static str, Limiter>,
    verify_lock: tokio::sync::Mutex<()>,
    fixture_endpoints: RwLock<BTreeMap<String, String>>,
    fixture_fetch: RwLock<Option<SocketAddr>>,
}
impl WebTools {
    pub fn production(state: &ServiceState) -> Result<Arc<Self>, String> {
        let network = state
            .network_plane()
            .cloned()
            .unwrap_or_else(NetworkPlane::from_environment);
        let tools = Self::new(
            state.memory().clone(),
            state.session_files().clone(),
            state.clock().clone(),
            network,
        )?;
        tools.bind_browser_search(state.browsers().port());
        Ok(tools)
    }
    pub fn new(
        memory: Arc<MemoryService>,
        files: Arc<SessionFileService>,
        clock: Arc<dyn ServiceClock>,
        network: Arc<NetworkPlane>,
    ) -> Result<Arc<Self>, String> {
        Self::construct(memory, files, clock, network, BTreeMap::new(), None)
    }
    fn construct(
        memory: Arc<MemoryService>,
        files: Arc<SessionFileService>,
        clock: Arc<dyn ServiceClock>,
        network: Arc<NetworkPlane>,
        fixture_endpoints: BTreeMap<String, String>,
        fixture_fetch: Option<SocketAddr>,
    ) -> Result<Arc<Self>, String> {
        let network = network
            .client_handle(HttpTimeouts {
                connect: Duration::from_secs(10),
                first_byte: API_TIMEOUT,
            })
            .map_err(|e| e.to_string())?;
        let limits = search::PAID
            .into_iter()
            .chain(["anysearch_free"])
            .chain(search::BROWSERS)
            .map(|id| {
                (
                    id,
                    Limiter::new(match id {
                        "anysearch" => 5,
                        "anysearch_free" => 3,
                        _ => 1,
                    }),
                )
            })
            .collect();
        Ok(Arc::new(Self {
            memory,
            files,
            clock,
            network,
            browser: RwLock::new(None),
            limits,
            verify_lock: tokio::sync::Mutex::new(()),
            fixture_endpoints: RwLock::new(fixture_endpoints),
            fixture_fetch: RwLock::new(fixture_fetch),
        }))
    }
    /// 只供可信集成测试替换外部响应地址；所有业务入口仍走正式网关与存储。
    #[doc(hidden)]
    pub fn set_fixture_http(
        &self,
        endpoints: BTreeMap<String, String>,
        fetch: Option<SocketAddr>,
    ) -> Result<(), String> {
        for (provider, source) in &endpoints {
            let url = reqwest::Url::parse(source).map_err(|_| "invalid fixture URL")?;
            if !search::PAID.contains(&provider.as_str()) && provider != "anysearch_free" {
                return Err("unknown fixture provider".into());
            }
            if url.scheme() != "http"
                || !url.username().is_empty()
                || url.password().is_some()
                || !url
                    .host_str()
                    .and_then(|h| h.trim_matches(['[', ']']).parse::<std::net::IpAddr>().ok())
                    .is_some_and(|ip| ip.is_loopback())
            {
                return Err("fixture endpoints must be loopback HTTP URLs".into());
            }
        }
        if fetch.is_some_and(|a| !a.ip().is_loopback() || a.port() == 0) {
            return Err("fixture fetch must be bound loopback".into());
        }
        *self.fixture_endpoints.write().expect("搜索测试端点锁") = endpoints;
        *self.fixture_fetch.write().expect("网页测试端点锁") = fetch;
        Ok(())
    }
    pub fn bind_browser_search(&self, port: Arc<dyn BrowserPort>) {
        *self.browser.write().expect("搜索浏览器端口锁") = Some(port);
    }
    fn credentials(&self) -> Result<SearchCredentials, WebError> {
        let _lock = crate::agents_admin::preferences_locked();
        SearchCredentials::from_preferences(
            &self
                .memory
                .read_preferences_root()
                .map_err(|e| WebError::new("WEB_SEARCH_CONFIG", e))?,
        )
        .map_err(|e| WebError::new("WEB_SEARCH_CONFIG", e))
    }
    pub fn settings(&self) -> Result<Value, WebError> {
        Ok(self.credentials()?.masked())
    }
    pub async fn verify(
        &self,
        provider: &str,
        candidate: &str,
        selected: &str,
    ) -> Result<Value, WebError> {
        if !search::known(provider) || !search::known(selected) {
            return Err(WebError::new(
                "WEB_SEARCH_CONFIG",
                "unknown search provider",
            ));
        }
        let _verify = self.verify_lock.lock().await;
        let before = self.credentials()?;
        let key = before
            .candidate(provider, candidate)
            .map_err(|e| WebError::new("WEB_SEARCH_AUTH", e))?;
        if search::PAID.contains(&provider) {
            self.provider(provider, "test", Some(1.0), &key, "en")
                .await?;
        }
        // 外部验证期间不持偏好锁；落盘前重新合并，不能丢掉并发更新的其他设置。
        {
            let _lock = crate::agents_admin::preferences_locked();
            let mut root = self
                .memory
                .read_preferences_root()
                .map_err(|e| WebError::new("WEB_SEARCH_CONFIG", e))?;
            let current = SearchCredentials::from_preferences(&root)
                .map_err(|e| WebError::new("WEB_SEARCH_CONFIG", e))?;
            if candidate == "********" && current.key(provider) != before.key(provider) {
                return Err(WebError::new(
                    "WEB_SEARCH_AUTH",
                    "saved key changed during verification; verify again",
                ));
            }
            current
                .save_into(&mut root, provider, &key, selected)
                .map_err(|e| WebError::new("WEB_SEARCH_CONFIG", e))?;
            self.memory
                .write_preferences_root(&root)
                .map_err(|e| WebError::new("WEB_SEARCH_SAVE", e))?;
        }
        Ok(
            json!({"ok":true,"verification":if search::PAID.contains(&provider){"provider_request"}else{"configuration_only"}}),
        )
    }
    pub async fn search(&self, query: &str, max: Option<f64>) -> Result<Value, WebError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(WebError::new(
                "WEB_SEARCH_EMPTY_QUERY",
                "search query is required",
            ));
        }
        if query.len() > 16 * 1024 {
            return Err(WebError::new(
                "WEB_SEARCH_ERROR",
                "search query exceeds 16 KiB",
            ));
        }
        let config = self.credentials()?;
        let locale = self
            .memory
            .read_preferences_root()
            .map_err(|e| WebError::new("WEB_SEARCH_CONFIG", e))?
            .get("locale")
            .and_then(Value::as_str)
            .unwrap_or("zh")
            .to_owned();
        if config.provider != "auto" {
            return self
                .provider(
                    &config.provider,
                    query,
                    max,
                    config.key(&config.provider).unwrap_or(""),
                    &locale,
                )
                .await;
        }
        let chain = search::PAID
            .into_iter()
            .filter(|id| config.key(id).is_some())
            .chain(["anysearch_free"])
            .chain(search::BROWSERS);
        let mut attempts = Vec::new();
        let mut low = None;
        for provider in chain {
            ensure_search_active()?;
            let source = if search::BROWSERS.contains(&provider) {
                "browser"
            } else {
                "api"
            };
            match self.provider(provider,query,max,config.key(provider).unwrap_or(""),&locale).await{
    Ok(mut payload)=>{
     let count=payload["results"].as_array().map_or(0,Vec::len);
     let status=if count==0{"empty"}else if low_quality(query,payload["results"].as_array().expect("搜索结果数组")){"low_quality"}else{"ok"};
     attempts.push(json!({"provider":provider,"source_type":source,"status":status,"result_count":count}));
     if status=="empty"{continue;}
     if status=="low_quality"{if low.is_none(){low=Some(payload);}continue;}
     payload["diagnostics"]["strategy"]=json!("auto");payload["diagnostics"]["attempts"]=json!(attempts);return Ok(payload);
    },
    Err(error) if matches!(error.code.as_str(), "WEB_SEARCH_CANCELLED" | "WEB_SEARCH_CLEANUP_UNCONFIRMED") => return Err(error),
    Err(error)=>attempts.push(json!({"provider":provider,"source_type":source,"status":"error","error_type":error.code.trim_start_matches("WEB_SEARCH_").to_ascii_lowercase(),"message":error.message,"retry_after_ms":error.retry_after_ms})),
   }
        }
        if let Some(mut payload) = low {
            payload["diagnostics"]["strategy"] = json!("auto");
            payload["diagnostics"]["attempts"] = json!(attempts);
            payload["diagnostics"]["selected_status"] = json!("low_quality");
            return Ok(payload);
        }
        Ok(
            json!({"query":query,"provider":"auto","source_type":"auto","results":[],"diagnostics":{"strategy":"auto","attempts":attempts,"status":"all_failed"}}),
        )
    }
    async fn provider(
        &self,
        provider: &str,
        query: &str,
        max: Option<f64>,
        key: &str,
        locale: &str,
    ) -> Result<Value, WebError> {
        if search::PAID.contains(&provider) && key.is_empty() {
            return Err(WebError::new(
                "WEB_SEARCH_AUTH",
                format!("{provider} API key is required"),
            ));
        }
        let limiter = self
            .limits
            .get(provider)
            .ok_or_else(|| WebError::new("WEB_SEARCH_ERROR", "unknown search provider"))?;
        let deadline = tokio::time::Instant::now() + API_TIMEOUT;
        let scope = tool_cancel_scope();
        ensure_search_active()?;
        let acquire = async {
            let slot = limiter
                .slots
                .acquire()
                .await
                .map_err(|_| WebError::new("WEB_SEARCH_ERROR", "search service stopped"))?;
            loop {
                let mut state = limiter.state.lock().await;
                let at = state.next.max(state.cooldown);
                if at > Instant::now() {
                    drop(state);
                    tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await;
                    continue;
                }
                let (base, jitter) = match provider {
                    "anysearch" | "anysearch_free" => (0, 0),
                    "tavily" => (650, 350),
                    "brave" => (1100, 400),
                    "serper" => (1000, 500),
                    "google_browser" => (6000, 8000),
                    _ => (3000, 4000),
                };
                let delay = if self
                    .fixture_endpoints
                    .read()
                    .expect("搜索测试端点锁")
                    .contains_key(provider)
                {
                    0
                } else {
                    base + random_jitter(jitter)?
                };
                state.next = Instant::now() + Duration::from_millis(delay);
                break;
            }
            Ok::<_, WebError>(slot)
        };
        let cancellation = async {
            match scope.as_ref() {
                Some(scope) => scope.cancelled().await,
                None => std::future::pending::<()>().await,
            }
        };
        let _slot = tokio::select! {
            biased;
            _ = cancellation => return Err(search_cancelled()),
            result = tokio::time::timeout_at(deadline, acquire) => result.map_err(|_| WebError::new(
                "WEB_SEARCH_ERROR", format!("{provider} search queue exceeded 30 seconds")))??,
        };
        ensure_search_active()?;
        let future = async {
            let count = provider_limit(provider, max);
            let result = if search::BROWSERS.contains(&provider) {
                self.browser_search(provider, query, count, locale, deadline)
                    .await
            } else {
                self.api_search(provider, query, count, key, locale).await
            };
            let mut state = limiter.state.lock().await;
            match &result {
                Ok(_) => state.failures = 0,
                Err(error) if error.code == "WEB_SEARCH_RATE_LIMITED" => {
                    let base = match provider {
                        "anysearch_free" | "bing_browser" | "duckduckgo_browser" => 10_000,
                        "google_browser" => 30_000,
                        _ => 2000,
                    };
                    let backoff = base * (1u64 << state.failures.min(8));
                    state.failures = state.failures.saturating_add(1);
                    let cap = if provider == "google_browser" {
                        600_000
                    } else {
                        300_000
                    };
                    state.cooldown = Instant::now()
                        + Duration::from_millis(
                            error
                                .retry_after_ms
                                .unwrap_or(backoff)
                                .saturating_add(random_jitter(1000)?)
                                .min(cap),
                        );
                }
                _ => {}
            }
            result
        };
        if search::BROWSERS.contains(&provider) {
            // 浏览器超时/取消必须先通知原端口并等待其回收，不能在外层直接丢弃端口future。
            future.await
        } else {
            let cancellation = async {
                match scope.as_ref() {
                    Some(scope) => scope.cancelled().await,
                    None => std::future::pending::<()>().await,
                }
            };
            tokio::select! {
                biased;
                _ = cancellation => Err(search_cancelled()),
                result = tokio::time::timeout_at(deadline, future) => result.map_err(|_| {
                    WebError::new("WEB_SEARCH_ERROR", format!("{provider} search exceeded 30 seconds"))
                })?,
            }
        }
    }
    async fn api_search(
        &self,
        provider: &str,
        query: &str,
        count: usize,
        key: &str,
        locale: &str,
    ) -> Result<Value, WebError> {
        let fixture = self
            .fixture_endpoints
            .read()
            .expect("搜索测试端点锁")
            .get(provider)
            .cloned();
        let endpoint = fixture.as_deref().unwrap_or(match provider {
            "anysearch" | "anysearch_free" => "https://api.anysearch.com/v1/search",
            "tavily" => "https://api.tavily.com/search",
            "serper" => "https://google.serper.dev/search",
            "brave" => "https://api.search.brave.com/res/v1/web/search",
            _ => return Err(WebError::new("WEB_SEARCH_ERROR", "unknown API provider")),
        });
        let client = self.network.client().map_err(|_| {
            WebError::new("WEB_SEARCH_ERROR", "network policy could not be applied")
        })?;
        let request = match provider {
            "anysearch" | "anysearch_free" => {
                let request = client
                    .post(endpoint)
                    .json(&json!({"query":query,"max_results":count,"language":language(locale)}));
                if provider == "anysearch" {
                    request.bearer_auth(key)
                } else {
                    request
                }
            }
            "tavily" => client
                .post(endpoint)
                .bearer_auth(key)
                .json(&json!({"query":query,"max_results":count,"search_depth":"basic"})),
            "serper" => client
                .post(endpoint)
                .header("X-API-KEY", key)
                .json(&json!({"q":query,"num":count})),
            "brave" => {
                let mut url = reqwest::Url::parse(endpoint)
                    .map_err(|_| WebError::new("WEB_SEARCH_ERROR", "invalid Brave endpoint"))?;
                url.query_pairs_mut()
                    .append_pair("q", query)
                    .append_pair("count", &count.to_string());
                client
                    .get(url)
                    .header("X-Subscription-Token", key)
                    .header("Accept", "application/json")
            }
            _ => return Err(WebError::new("WEB_SEARCH_ERROR", "unknown API provider")),
        };
        let response = request.send().await.map_err(|_| {
            WebError::new(
                "WEB_SEARCH_ERROR",
                format!("{provider} search transport failed"),
            )
        })?;
        let status = response.status().as_u16();
        if status == 402 || status == 429 {
            let retry = retry_after(response.headers(), self.clock.now_unix_ms());
            return Err(WebError {
                code: "WEB_SEARCH_RATE_LIMITED".into(),
                message: format!("{provider} API HTTP {status}"),
                retry_after_ms: retry,
            });
        }
        if status == 401 || status == 403 {
            return Err(WebError::new(
                "WEB_SEARCH_AUTH",
                format!("{provider} API HTTP {status}"),
            ));
        }
        if !response.status().is_success() {
            return Err(WebError::new(
                "WEB_SEARCH_ERROR",
                format!("{provider} API HTTP {status}; redirects are not followed"),
            ));
        }
        let bytes = bounded_body(response, MAX_BODY)
            .await
            .map_err(|e| WebError::new("WEB_SEARCH_ERROR", e))?;
        let data: Value = serde_json::from_slice(&bytes).map_err(|_| {
            WebError::new(
                "WEB_SEARCH_ERROR",
                format!("{provider} returned invalid JSON"),
            )
        })?;
        if matches!(provider, "anysearch" | "anysearch_free")
            && data.get("code").is_some_and(|c| c != 0)
        {
            return Err(WebError::new(
                "WEB_SEARCH_ERROR",
                "AnySearch returned an unsuccessful envelope",
            ));
        }
        let raw = match provider {
            "anysearch" | "anysearch_free" => data
                .pointer("/data/results")
                .or_else(|| data.get("results")),
            "tavily" => data.get("results"),
            "serper" => data.get("organic"),
            "brave" => data.pointer("/web/results"),
            _ => None,
        };
        let raw = raw.and_then(Value::as_array).ok_or_else(|| {
            WebError::new(
                "WEB_SEARCH_ERROR",
                format!("{provider} response has no results array"),
            )
        })?;
        let results=raw.iter().take(count).enumerate().map(|(index,row)|{
   let text=match provider{"serper"=>row["snippet"].as_str(),"brave"=>row["description"].as_str(),_=>row["content"].as_str().or_else(||row["description"].as_str()).or_else(||row["snippet"].as_str())}.unwrap_or("");
   json!({"title":row["title"].as_str().unwrap_or(""),"url":if provider=="serper"{row["link"].as_str().unwrap_or("")}else{row["url"].as_str().unwrap_or("")},"content":text,"rank":row.get("rank").filter(|value|value.is_number()).cloned().unwrap_or_else(||if matches!(provider,"anysearch"|"anysearch_free"){json!(index+1)}else{Value::Null}),"score":row.get("score").or_else(||row.get("quality_score")).filter(|value|value.is_number()).cloned().unwrap_or(Value::Null),"metadata":if matches!(provider,"anysearch"|"anysearch_free"){json!({"description":row["description"].as_str().unwrap_or(""),"quality_score":row.get("quality_score"),"signal_scores":row.get("signal_scores").cloned().unwrap_or(json!({})),"source":row["source"].as_str().unwrap_or(""),"published_at":row.get("published_at")})}else{json!({})}})
  }).collect::<Vec<_>>();
        let metadata = data
            .pointer("/data/metadata")
            .or_else(|| data.get("metadata"))
            .cloned()
            .unwrap_or(json!({}));
        let diagnostics = if matches!(provider, "anysearch" | "anysearch_free") {
            json!({"anonymous":provider=="anysearch_free","total_results":metadata.get("total_results"),"search_time_ms":metadata.get("search_time_ms"),"request_id":metadata["request_id"].as_str().unwrap_or(""),"cached":metadata.get("cached")})
        } else {
            json!({})
        };
        Ok(
            json!({"query":query,"provider":provider,"source_type":"api","results":results,"diagnostics":diagnostics}),
        )
    }
    async fn browser_search(
        &self,
        provider: &str,
        query: &str,
        count: usize,
        locale: &str,
        deadline: tokio::time::Instant,
    ) -> Result<Value, WebError> {
        let browser = self
            .browser
            .read()
            .expect("搜索浏览器端口锁")
            .clone()
            .ok_or_else(|| {
                WebError::new("WEB_SEARCH_ERROR", "controlled browser is unavailable")
            })?;
        let url = browser_url(provider, query, count, locale)?;
        let expression=format!("(function(){{const module={{exports:{{}}}};{};return eval(module.exports.buildBrowserSearchExtractionScript({},{}));}})()",include_str!("../../../../lib/browser/browser-search-extractors.cjs"),json!(provider),count);
        let cancel = CancelFlag(Arc::new(AtomicBool::new(false)));
        let scope = tool_cancel_scope();
        let mut future =
            browser.read_auxiliary_page(url, expression, Some(locale.to_owned()), cancel.0.clone());
        let cancellation = async {
            match scope.as_ref() {
                Some(scope) => scope.cancelled().await,
                None => std::future::pending::<()>().await,
            }
        };
        let response = tokio::select! {
            biased;
            _ = cancellation => {
                cancel.0.store(true, Ordering::Release);
                // 原任务监督器的取消时点及清理窗口仍为唯一期限；不另开任务或重置预算。
                confirm_search_cleanup(future.await)?;
                return Err(search_cancelled());
            }
            _ = tokio::time::sleep_until(deadline) => {
                cancel.0.store(true, Ordering::Release);
                if let Some(scope) = scope.as_ref() {
                    scope.cancel("browser search exceeded 30 seconds");
                    confirm_search_cleanup(future.await)?;
                } else {
                    let cleanup = tokio::time::timeout(Duration::from_secs(5), &mut future).await
                        .map_err(|_| WebError::new("WEB_SEARCH_CLEANUP_UNCONFIRMED", "browser search cleanup unconfirmed"))?;
                    confirm_search_cleanup(cleanup)?;
                }
                return Err(WebError::new("WEB_SEARCH_ERROR", "browser search exceeded 30 seconds"));
            }
            response = &mut future => response.map_err(|error| WebError::new(
                "WEB_SEARCH_ERROR", format!("browser search failed: {}", error.code)))?,
        };
        let status = response["status"].as_str().ok_or_else(|| {
            WebError::new(
                "WEB_SEARCH_EXTRACTION_FAILED",
                "browser extraction returned no status",
            )
        })?;
        if response["blocked"] == true || status == "blocked" {
            return Err(WebError::new(
                "WEB_SEARCH_BLOCKED",
                "search page requires human verification",
            ));
        }
        if status == "extraction_failed" {
            return Err(WebError::new(
                "WEB_SEARCH_EXTRACTION_FAILED",
                "search results could not be extracted",
            ));
        }
        if !matches!(status, "ok" | "no_results") {
            return Err(WebError::new(
                "WEB_SEARCH_EXTRACTION_FAILED",
                "unknown search extraction status",
            ));
        }
        let results=response["results"].as_array().ok_or_else(||WebError::new("WEB_SEARCH_EXTRACTION_FAILED","browser extraction returned no results array"))?.iter().take(count).map(|r|json!({"title":r["title"].as_str().unwrap_or(""),"url":r["url"].as_str().unwrap_or(""),"content":r["content"].as_str().or_else(||r["snippet"].as_str()).unwrap_or(""),"rank":r.get("rank").filter(|value|value.is_number()),"score":r.get("score").filter(|value|value.is_number()),"metadata":{}})).collect::<Vec<_>>();
        Ok(
            json!({"query":query,"provider":provider,"source_type":"browser","results":results,"diagnostics":{"status":status,"final_url":response.get("final_url"),"captcha":response.get("captcha")}}),
        )
    }
    pub async fn fetch(
        &self,
        ctx: &RunContext,
        url: &str,
        max: Option<f64>,
    ) -> Result<Value, WebError> {
        let url = url.trim();
        if url.is_empty() {
            return Err(WebError::new("WEB_FETCH_EMPTY_URL", "URL is required"));
        }
        let mut current = reqwest::Url::parse(url)
            .map_err(|_| WebError::new("WEB_FETCH_INVALID_URL", "URL is invalid"))?;
        if !matches!(current.scheme(), "http" | "https") {
            return Err(WebError::new(
                "WEB_FETCH_INVALID_SCHEME",
                "only HTTP and HTTPS are supported",
            ));
        }
        let result = tokio::time::timeout(FETCH_TIMEOUT, async {
            for hop in 0..=5 {
                let fixture_address = *self.fixture_fetch.read().expect("网页测试端点锁");
                let fixture = current.scheme() == "http"
                    && current.host_str() == Some("r07-web-fixture.invalid")
                    && fixture_address
                        .is_some_and(|a| current.port_or_known_default() == Some(a.port()));
                let addresses = if fixture {
                    vec![fixture_address.expect("可信测试端点")]
                } else {
                    crate::resource_url::public_addresses(&current)
                        .await
                        .map_err(|_| {
                            WebError::new(
                                "WEB_FETCH_SSRF_BLOCKED",
                                "URL must resolve only to public addresses",
                            )
                        })?
                };
                if !current.username().is_empty() || current.password().is_some() {
                    return Err(WebError::new(
                        "WEB_FETCH_INVALID_URL",
                        "URL credentials are not allowed",
                    ));
                }
                // 代理可能自行解析目标域名，普通DNS覆盖无法证明代理仍访问已验证IP。明确拒绝，绝不私自直连。
                let policy = self.network.plane().policy();
                if policy.proxies(&current) {
                    return Err(WebError::new(
                        "WEB_FETCH_PROXY_PIN_UNAVAILABLE",
                        "public-only URL fetch cannot safely pin the configured proxy destination",
                    ));
                }
                let client = lingxi_adapters::models::dispatch::build_client_pinned_under_policy(
                    &HttpTimeouts {
                        connect: Duration::from_secs(10),
                        first_byte: FETCH_TIMEOUT,
                    },
                    &policy,
                    current
                        .host_str()
                        .ok_or_else(|| WebError::new("WEB_FETCH_INVALID_URL", "URL has no host"))?,
                    &addresses,
                )
                .map_err(|_| {
                    WebError::new("WEB_FETCH_ERROR", "network policy could not be applied")
                })?;
                let response = client
                    .get(current.clone())
                    .header("User-Agent", "Mozilla/5.0 (compatible; LingxiAgentBot/1.0)")
                    .header(
                        "Accept",
                        "text/html,application/xhtml+xml,application/json,text/plain,*/*",
                    )
                    .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
                    .send()
                    .await
                    .map_err(|_| WebError::new("WEB_FETCH_ERROR", "web fetch transport failed"))?;
                let status = response.status().as_u16();
                if [301, 302, 307, 308].contains(&status) {
                    if hop == 5 {
                        return Err(WebError::new(
                            "WEB_FETCH_REDIRECT_LIMIT",
                            "web fetch exceeded five redirects",
                        ));
                    }
                    let location = response
                        .headers()
                        .get(reqwest::header::LOCATION)
                        .and_then(|v| v.to_str().ok())
                        .ok_or_else(|| {
                            WebError::new("WEB_FETCH_ERROR", "redirect has no valid Location")
                        })?;
                    current = current.join(location).map_err(|_| {
                        WebError::new("WEB_FETCH_INVALID_URL", "redirect URL is invalid")
                    })?;
                    continue;
                }
                if !response.status().is_success() {
                    return Err(WebError::new(
                        &format!("WEB_FETCH_HTTP_{status}"),
                        format!("web fetch HTTP {status}"),
                    ));
                }
                let mime = response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("text/plain")
                    .to_ascii_lowercase();
                let bytes = bounded_body(response, MAX_BODY)
                    .await
                    .map_err(|e| WebError::new("WEB_FETCH_ERROR", e))?;
                let raw = String::from_utf8_lossy(&bytes).to_string();
                let (text, format) = if mime.contains("json") {
                    (
                        serde_json::from_str::<Value>(&raw)
                            .and_then(|v| serde_json::to_string_pretty(&v))
                            .unwrap_or(raw),
                        "json",
                    )
                } else if mime.contains("html") {
                    (html_text(&raw, &current), "markdown")
                } else {
                    (raw, "text")
                };
                return Ok((text, format, current.to_string()));
            }
            Err(WebError::new(
                "WEB_FETCH_REDIRECT_LIMIT",
                "web fetch exceeded five redirects",
            ))
        })
        .await
        .map_err(|_| WebError::new("WEB_FETCH_TIMEOUT", "web fetch exceeded 15 seconds"))??;
        let (text, format, final_url) = result;
        let total = text.chars().count();
        let limit = clamp(max, 12000, 200, 200000);
        let truncated = total > limit;
        let mut output = text.chars().take(limit).collect::<String>();
        let mut details = json!({"originalUrl":url,"finalUrl":final_url,"format":format,"totalLength":total,"truncated":truncated});
        if truncated {
            let file = self
                .files
                .register_web_text(ctx.session_id.as_str(), "web-fetch.txt", text.as_bytes())
                .await
                .map_err(|_| {
                    WebError::new(
                        "WEB_FETCH_ERROR",
                        "full content could not be registered for continuation",
                    )
                })?;
            let file = serde_json::to_value(file).map_err(|_| {
                WebError::new(
                    "WEB_FETCH_ERROR",
                    "continuation file could not be serialized",
                )
            })?;
            output.push_str(&format!("\n\n[Truncated: showing first {limit} of {total} characters. Use materialize with session fileId {}, then read its returned filePath for the full content.]",file["fileId"].as_str().unwrap_or("")));
            details["sessionFile"] = file;
        }
        Ok(json!({"text":format!("Source: {final_url} ({format})\n\n{output}"),"details":details}))
    }
}

fn tool_cancel_scope() -> Option<crate::cancel::CancelScope> {
    crate::toolgateway::TOOL_LIFETIME
        .try_with(|lifetime| lifetime.cancel.clone())
        .ok()
}
fn search_cancelled() -> WebError {
    WebError::new("WEB_SEARCH_CANCELLED", "search operation cancelled")
}
fn confirm_search_cleanup(
    result: Result<Value, lingxi_kernel::browser::BrowserError>,
) -> Result<(), WebError> {
    if result.as_ref().is_err_and(|error| error.effect_unknown) {
        return Err(WebError::new(
            "WEB_SEARCH_CLEANUP_UNCONFIRMED",
            "browser search stopped waiting, but external cleanup remains unconfirmed",
        ));
    }
    Ok(())
}
fn ensure_search_active() -> Result<(), WebError> {
    if tool_cancel_scope().is_some_and(|scope| scope.is_cancelled()) {
        Err(search_cancelled())
    } else {
        Ok(())
    }
}

async fn bounded_body(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>, String> {
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err("response exceeds 8 MiB limit".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "response body could not be read")?
    {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err("response exceeds 8 MiB limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
fn random_jitter(max: u64) -> Result<u64, WebError> {
    if max == 0 {
        return Ok(0);
    }
    let token = crate::auth::hex_random_public(4).map_err(|_| {
        WebError::new(
            "WEB_SEARCH_ERROR",
            "search rate jitter could not be generated",
        )
    })?;
    Ok(u64::from_str_radix(&token, 16)
        .map_err(|_| WebError::new("WEB_SEARCH_ERROR", "search rate jitter was invalid"))?
        % max)
}
fn clamp(value: Option<f64>, default: usize, min: usize, max: usize) -> usize {
    value
        .filter(|v| v.is_finite())
        .map(|v| v.floor().clamp(min as f64, max as f64) as usize)
        .unwrap_or(default)
}
fn provider_limit(provider: &str, value: Option<f64>) -> usize {
    match provider {
        "anysearch" | "anysearch_free" => clamp(value, 20, 1, 100),
        "tavily" => clamp(value, 10, 1, 20),
        _ => clamp(value, 10, 1, 10),
    }
}
fn language(locale: &str) -> &'static str {
    if locale.starts_with("zh") {
        "zh-CN"
    } else if locale.starts_with("ja") {
        "ja"
    } else if locale.starts_with("ko") {
        "ko"
    } else {
        "en"
    }
}
fn retry_after(headers: &reqwest::header::HeaderMap, now: u64) -> Option<u64> {
    let raw = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    if let Ok(n) = raw.parse::<f64>() {
        return if n.is_finite() && n >= 0.0 {
            Some((n * 1000.0).min(u64::MAX as f64) as u64)
        } else {
            None
        };
    }
    chrono::DateTime::parse_from_rfc2822(raw)
        .ok()
        .and_then(|d| u64::try_from(d.timestamp_millis()).ok())
        .map(|at| at.saturating_sub(now))
}
fn browser_url(
    provider: &str,
    query: &str,
    count: usize,
    locale: &str,
) -> Result<String, WebError> {
    let source = match provider {
        "bing_browser" => "https://www.bing.com/search",
        "google_browser" => "https://www.google.com/search",
        "duckduckgo_browser" => "https://duckduckgo.com/",
        _ => {
            return Err(WebError::new(
                "WEB_SEARCH_ERROR",
                "unknown browser search provider",
            ))
        }
    };
    let mut url = reqwest::Url::parse(source).expect("固定搜索地址");
    {
        let mut args = url.query_pairs_mut();
        args.append_pair("q", query);
        if provider == "duckduckgo_browser" {
            args.append_pair("kl", "wt-wt");
        } else {
            args.append_pair(
                if provider == "bing_browser" {
                    "count"
                } else {
                    "num"
                },
                &count.to_string(),
            );
            if provider == "bing_browser" {
                let lang = language(locale);
                args.append_pair("mkt", if lang == "en" { "en-US" } else { lang });
                args.append_pair("setlang", lang);
            }
        }
    }
    Ok(url.to_string())
}
fn low_quality(query: &str, results: &[Value]) -> bool {
    let normalized = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let terms = query
        .split_whitespace()
        .filter(|s| s.chars().any(|c| ('\u{3400}'..='\u{9fff}').contains(&c)))
        .map(normalized)
        .filter(|s| s.chars().count() >= 2)
        .collect::<Vec<_>>();
    if terms.len() < 3 || results.is_empty() {
        return false;
    }
    let texts = results
        .iter()
        .take(3)
        .map(|r| {
            normalized(&format!(
                "{} {} {}",
                r["title"].as_str().unwrap_or(""),
                r["url"].as_str().unwrap_or(""),
                r["content"].as_str().unwrap_or("")
            ))
        })
        .collect::<Vec<_>>();
    let all = texts.join("");
    let matched = terms.iter().filter(|t| all.contains(t.as_str())).count();
    let dict = texts
        .iter()
        .filter(|s| {
            [
                "汉语文字",
                "汉典",
                "字典",
                "词典",
                "基本解释",
                "百度百科",
                "hanyu",
                "zdic",
                "zidian",
                "cidian",
            ]
            .iter()
            .any(|word| s.contains(word))
        })
        .count();
    dict >= 2.min(texts.len()) && matched <= 1
}
fn html_text(raw: &str, base: &reqwest::Url) -> String {
    let document = scraper::Html::parse_document(raw);
    let article = scraper::Selector::parse("article, main, [role=main]").expect("固定正文选择器");
    let body = scraper::Selector::parse("body").expect("固定正文选择器");
    let root = document
        .select(&article)
        .next()
        .or_else(|| document.select(&body).next())
        .unwrap_or_else(|| document.root_element());
    let mut text = String::new();
    markdown_element(root, base, &mut text, 0);
    let spaces = regex::Regex::new(r"[ \t]+").expect("固定空白表达式");
    let text = spaces.replace_all(&text, " ");
    let lines = regex::Regex::new(r"\n{3,}").expect("固定换行表达式");
    lines.replace_all(&text, "\n\n").trim().to_owned()
}
fn markdown_element(
    element: scraper::ElementRef<'_>,
    base: &reqwest::Url,
    out: &mut String,
    depth: usize,
) {
    let tag = element.value().name();
    if matches!(
        tag,
        "script" | "style" | "head" | "nav" | "footer" | "noscript" | "svg"
    ) {
        return;
    }
    if depth >= 64 {
        out.push_str("\n[Deeply nested HTML converted to plain text]\n");
        out.push_str(&element.text().collect::<String>());
        return;
    }
    if tag == "br" {
        out.push('\n');
        return;
    }
    if tag == "hr" {
        out.push_str("\n---\n");
        return;
    }
    if tag == "pre" {
        out.push_str("\n```\n");
        out.push_str(&element.text().collect::<String>());
        out.push_str("\n```\n");
        return;
    }
    let block = matches!(
        tag,
        "p" | "div"
            | "section"
            | "article"
            | "main"
            | "header"
            | "blockquote"
            | "ul"
            | "ol"
            | "li"
            | "tr"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
    );
    if block {
        out.push('\n');
    }
    if matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
        out.push_str(&"#".repeat((tag.as_bytes()[1] - b'0') as usize));
        out.push(' ');
    }
    if tag == "li" {
        out.push_str("- ");
    }
    if tag == "blockquote" {
        out.push_str("> ");
    }
    let marker = match tag {
        "strong" | "b" => "**",
        "em" | "i" => "*",
        "code" => "`",
        _ => "",
    };
    out.push_str(marker);
    let mut inner = String::new();
    for child in element.children() {
        match child.value() {
            scraper::node::Node::Text(text) => inner.push_str(text),
            scraper::node::Node::Element(_) => {
                if let Some(element) = scraper::ElementRef::wrap(child) {
                    markdown_element(element, base, &mut inner, depth + 1);
                }
            }
            _ => {}
        }
    }
    if tag == "a" {
        match element
            .value()
            .attr("href")
            .and_then(|raw| base.join(raw).ok())
            .filter(|url| matches!(url.scheme(), "http" | "https"))
        {
            Some(url) => out.push_str(&format!("[{}]({url})", inner.trim())),
            None => out.push_str(&inner),
        }
    } else {
        out.push_str(&inner);
    }
    out.push_str(marker);
    if matches!(tag, "td" | "th") {
        out.push_str(" | ");
    }
    if block {
        out.push('\n');
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Search,
    Fetch,
}
struct Executor {
    tools: Arc<WebTools>,
    kind: Kind,
}
impl ToolExecutorPort for Executor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            let args = request.arguments.as_value();
            let result = match self.kind {
                Kind::Search => {
                    self.tools
                        .search(
                            args["query"].as_str().unwrap_or(""),
                            args.get("maxResults").and_then(Value::as_f64),
                        )
                        .await
                }
                Kind::Fetch => {
                    self.tools
                        .fetch(
                            ctx,
                            args["url"].as_str().unwrap_or(""),
                            args.get("maxLength").and_then(Value::as_f64),
                        )
                        .await
                }
            };
            let outcome = match result {
                Ok(payload) => {
                    let (text, details) = match self.kind {
                        Kind::Fetch => (
                            payload["text"].as_str().unwrap_or("").into(),
                            payload["details"].clone(),
                        ),
                        Kind::Search => {
                            let shown =
                                clamp(args.get("maxResults").and_then(Value::as_f64), 10, 1, 100);
                            let rows = payload["results"].as_array().expect("搜索结果数组");
                            let mut text = rows
                                .iter()
                                .take(shown)
                                .enumerate()
                                .map(|(i, row)| {
                                    let content = row["content"].as_str().unwrap_or("");
                                    let mut snippet = content.chars().take(500).collect::<String>();
                                    if content.chars().count() > 500 {
                                        snippet.push('…');
                                    }
                                    format!(
                                        "{}. **{}**\n   {}\n   {snippet}",
                                        i + 1,
                                        row["title"].as_str().unwrap_or(""),
                                        row["url"].as_str().unwrap_or("")
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join("\n\n");
                            if rows.is_empty() {
                                text = if payload["diagnostics"]["status"] == "all_failed" {
                                    "All configured search providers failed; inspect diagnostics.attempts.".into()
                                } else {
                                    "No search results.".into()
                                };
                            }
                            if rows.len() > shown {
                                text.push_str(&format!("\n\n[Showing {shown} of {} results. Increase maxResults to see more.]",rows.len()));
                            }
                            (text, payload)
                        }
                    };
                    let mut success = ToolSuccess::text(text);
                    success.details = details.as_object().cloned();
                    ToolOutcome::Success { result: success }
                }
                Err(error) => ToolOutcome::Failed {
                    error: ProtocolError::new(
                        ErrorCode::UpstreamUnavailable,
                        error.to_string(),
                        false,
                    )
                    .with_details(
                        json!({"errorCode":error.code,"retryAfterMs":error.retry_after_ms})
                            .as_object()
                            .expect("错误详情对象")
                            .clone(),
                    ),
                },
            };
            ToolExecutionResult::of_ctx(ctx, outcome)
        })
    }
}
pub fn register(
    gateway: &crate::toolgateway::ToolInvocationGateway,
    tools: Arc<WebTools>,
    budget: &SchemaBudget,
) -> Result<(), String> {
    for (name, kind, capability, schema) in [
        (
            "web_search",
            Kind::Search,
            "web.search",
            json!({"type":"object","properties":{"query":{"type":"string","maxLength":16384},"maxResults":{"type":"number"}},"required":["query"],"additionalProperties":false}),
        ),
        (
            "web_fetch",
            Kind::Fetch,
            "web.fetch",
            json!({"type":"object","properties":{"url":{"type":"string","maxLength":8192},"maxLength":{"type":"number"}},"required":["url"],"additionalProperties":false}),
        ),
    ] {
        let receipt = gateway
            .registry()
            .register(
                ToolManifest {
                    origin: ToolOrigin::FirstParty,
                    local_name: name.into(),
                    display_name: name.into(),
                    aliases: vec![],
                    version: "1".into(),
                    description: if name == "web_search" {
                        "通过当前工作室配置的搜索供应商查找网页；失败和降级按真实诊断说明。"
                    } else {
                        "只读取公网HTTP网页；过长正文登记为本会话可续读文件。"
                    }
                    .into(),
                    input_schema: ToolSchemaDocument {
                        dialect: "json-schema/2020-12".into(),
                        schema,
                    },
                    output_schema: None,
                    permission: PermissionContract {
                        kind: PermissionKind::Read,
                        capability_base: capability.into(),
                    },
                    availability: Availability::Available,
                    timeout_ms: Some(if name == "web_search" { 300000 } else { 20000 }),
                    max_concurrency: Some(16),
                    declared_permission: DeclaredPermission::ReadOnly,
                    recovery: lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE,
                },
                budget,
            )
            .map_err(|e| format!("{e:?}"))?;
        gateway
            .registry()
            .add_model_resident(&receipt.target_id)
            .map_err(|e| format!("{e:?}"))?;
        gateway.bind_executor(
            receipt.target_id,
            Arc::new(Executor {
                tools: tools.clone(),
                kind,
            }),
            "正式搜索与网页读取",
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn limits_and_html_keep_full_readable_content() {
        assert_eq!(provider_limit("anysearch", None), 20);
        assert_eq!(provider_limit("anysearch_free", Some(1000.0)), 100);
        assert_eq!(provider_limit("tavily", Some(-1.0)), 1);
        assert_eq!(provider_limit("brave", Some(f64::NAN)), 10);
        let text=html_text("<head>secret</head><article><p>中文&amp;内容</p><a href='/docs'>继续</a><script>bad()</script></article>",&reqwest::Url::parse("https://example.org/a").unwrap());
        assert!(text.contains("中文&内容"));
        assert!(text.contains("[继续](https://example.org/docs)"));
        assert!(!text.contains("bad()") && !text.contains("secret"));
    }
    #[test]
    fn retry_after_and_browser_url_are_bounded() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("retry-after", "2".parse().unwrap());
        assert_eq!(retry_after(&headers, 0), Some(2000));
        assert!(browser_url("google_browser", "a&b 中文", 10, "zh-CN")
            .unwrap()
            .contains("q=a%26b+%E4%B8%AD%E6%96%87"));
        assert!(browser_url("unknown", "q", 1, "en").is_err());
    }
}
