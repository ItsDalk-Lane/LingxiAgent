//! MCP 连接器独立 OAuth：官方状态机、同一私密 Source、原主体和配置身份约束。
use super::*;
use axum::extract::Path;
use lingxi_protocol::{
    McpOAuthCancelResponse, McpOAuthLogoutResponse, McpOAuthPollResponse, McpOAuthStartResponse,
    McpSettingsState,
};
use rmcp::transport::auth::{
    self as sdk, AuthError, AuthorizationManager, AuthorizationRequest, AuthorizationSession,
    CredentialRefreshGuard, CredentialStore, OAuthClientConfig, OAuthHttpClient,
    OAuthHttpClientFuture, OAuthHttpRequest, StoredCredentials,
};
use serde::Deserialize;
use std::sync::atomic::AtomicU8;
use std::time::{Duration, Instant};

const FLOW_LIMIT: usize = 32;
// 与现役页面100次、每次3秒的浏览器等待边界一致；过期事务不再写入凭据。
const FLOW_BUDGET: Duration = Duration::from_secs(300);
#[derive(Default)]
struct RegistrationCapture {
    endpoint: Option<String>,
    response: Option<Value>,
}
#[derive(Clone)]
struct OAuthNetwork {
    network: mcpbridge::http::NetworkMcpClient,
    registration: Arc<std::sync::Mutex<RegistrationCapture>>,
}
impl OAuthHttpClient for OAuthNetwork {
    fn execute(&self, request: OAuthHttpRequest) -> OAuthHttpClientFuture<'_> {
        Box::pin(async move {
            let endpoint = request.request.uri().to_string();
            let response = self.network.execute(request).await?;
            let mut capture = self.registration.lock().unwrap_or_else(|p| p.into_inner());
            if capture.endpoint.as_deref() == Some(endpoint.as_str())
                && response.status().is_success()
            {
                capture.response = serde_json::from_slice(response.body()).ok();
            }
            Ok(response)
        })
    }
}
#[derive(Clone)]
struct ConnectorStore(Arc<StoreInner>);
struct StoreInner {
    manager: Weak<McpSettingsManager>,
    id: String,
    expected: std::sync::Mutex<Value>,
    flow: Option<Weak<OAuthFlow>>,
    credential_epoch: AtomicU64,
    registration: Arc<std::sync::Mutex<RegistrationCapture>>,
    refresh: Arc<Mutex<()>>,
}
#[derive(Clone)]
struct FlowStatus {
    status: &'static str,
    error: Option<String>,
}
pub(super) struct OAuthFlow {
    connector_id: String,
    flow_epoch: AtomicU64,
    authority: SettingsAuthority,
    cancel: crate::cancel::CancelScope,
    created: Instant,
    status: std::sync::Mutex<FlowStatus>,
    store: ConnectorStore,
    session: Mutex<Option<AuthorizationSession>>,
    // 0尚未写，1已确认写，2写入结果未知；取消不能把后两种说成未发生。
    committed: AtomicU8,
    active: AtomicBool,
    changed: Notify,
    redirect_uri: String,
}
pub(super) fn connection_identity(connector: &Value) -> Value {
    let mut identity = connector.clone();
    if let Some(fields) = identity.as_object_mut() {
        fields.remove("oauth");
    }
    identity
}
fn credential_identity(connector: &Value) -> Value {
    let mut fields = serde_json::Map::new();
    for key in [
        "id",
        "transport",
        "url",
        "baseUrl",
        "headers",
        "authType",
        "oauthClientId",
        "oauthClientSecret",
    ] {
        if let Some(value) = connector.get(key) {
            fields.insert(key.to_owned(), value.clone());
        }
    }
    Value::Object(fields)
}
fn epoch(table: &RwLock<BTreeMap<String, u64>>, id: &str) -> u64 {
    table
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .get(id)
        .copied()
        .unwrap_or(0)
}
fn bump(table: &RwLock<BTreeMap<String, u64>>, id: &str) {
    *table
        .write()
        .unwrap_or_else(|p| p.into_inner())
        .entry(id.into())
        .or_default() += 1;
}
fn binding_hash(connector: &Value) -> String {
    lingxi_protocol::canon::canonical_sha256_hex(&credential_identity(connector))
}

fn stored(connector: &Value) -> Result<Option<StoredCredentials>> {
    let Some(value) = connector.pointer("/oauth/sdk").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    if connector
        .pointer("/oauth/bindingSha256")
        .and_then(Value::as_str)
        != Some(binding_hash(connector).as_str())
    {
        return Err(McpSettingsError::new(
            409,
            "mcp_oauth_binding_changed",
            "已保存的OAuth凭据不属于当前连接配置，请重新登录",
        ));
    }
    serde_json::from_value(value.clone())
        .map(Some)
        .map_err(|_| storage("已保存的MCP OAuth凭据损坏"))
}
pub(super) fn credential_status(connector: &Value) -> &'static str {
    match stored(connector) {
        Ok(Some(credentials)) if credentials.token_response.is_some() => "connected",
        Err(_) => "error",
        _ => "disconnected",
    }
}
impl ConnectorStore {
    fn manager(&self) -> std::result::Result<Arc<McpSettingsManager>, AuthError> {
        self.0
            .manager
            .upgrade()
            .ok_or_else(|| AuthError::CredentialStoreError("MCP 管理器已退出".into()))
    }
    fn checked(&self, manager: &McpSettingsManager) -> Result<Value> {
        if manager.closing.load(Ordering::SeqCst) {
            return Err(McpSettingsError::new(409, "mcp_stopping", "服务正在退出"));
        }
        if let Some(flow) = self.0.flow.as_ref().and_then(Weak::upgrade) {
            flow.validate()?;
        }
        let config = McpSettingsManager::config(&manager.read()?)?;
        let current = config["connectors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == self.0.id)
            .ok_or_else(|| {
                McpSettingsError::new(404, "mcp_connector_missing", "OAuth连接器已移除")
            })?;
        if epoch(&manager.oauth_credential_epochs, &self.0.id)
            != self.0.credential_epoch.load(Ordering::SeqCst)
            || credential_identity(current)
                != *self.0.expected.lock().unwrap_or_else(|p| p.into_inner())
        {
            return Err(McpSettingsError::new(
                409,
                "mcp_oauth_binding_changed",
                "OAuth连接配置已改变，旧流程不能保存凭据",
            ));
        }
        Ok(current.clone())
    }
    fn write(&self, credentials: Option<StoredCredentials>) -> std::result::Result<(), AuthError> {
        let manager = self.manager()?;
        let _writer = manager
            .source_write
            .lock()
            .map_err(|_| AuthError::CredentialStoreError("MCP Source写锁损坏".into()))?;
        self.checked(&manager)
            .map_err(|e| AuthError::CredentialStoreError(e.message))?;
        let (mut document, version) = manager
            .read_versioned()
            .map_err(|e| AuthError::CredentialStoreError(e.message))?;
        let mut config = McpSettingsManager::config(&document)
            .map_err(|e| AuthError::CredentialStoreError(e.message))?;
        let connector = config["connectors"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|c| c["id"] == self.0.id)
            .ok_or_else(|| AuthError::CredentialStoreError("MCP连接器已移除".into()))?;
        if !connector["oauth"].is_object() {
            connector["oauth"] = json!({});
        }
        if let Some(credentials) = credentials {
            if let Some(flow) = self.0.flow.as_ref().and_then(Weak::upgrade) {
                let capture = self
                    .0
                    .registration
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                if let Some(registration) = capture
                    .response
                    .as_ref()
                    .filter(|r| r["client_id"] == credentials.client_id)
                {
                    connector["oauthClientId"] = json!(credentials.client_id);
                    if let Some(secret) = registration["client_secret"].as_str() {
                        connector["oauthClientSecret"] = json!(secret);
                    }
                }
                connector["oauth"]["redirectUri"] = json!(flow.redirect_uri);
            }
            connector["oauth"]["sdk"] = serde_json::to_value(credentials)
                .map_err(|_| AuthError::CredentialStoreError("OAuth凭据不能序列化".into()))?;
            connector["oauth"]["bindingSha256"] = json!(binding_hash(connector));
        } else {
            connector["oauth"]["sdk"] = Value::Null;
            connector["oauth"]["bindingSha256"] = Value::Null;
        }
        let next = credential_identity(connector);
        let before = self
            .0
            .expected
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let check = || {
            if manager.closing.load(Ordering::SeqCst)
                || epoch(&manager.oauth_credential_epochs, &self.0.id)
                    != self.0.credential_epoch.load(Ordering::SeqCst)
                || self
                    .0
                    .flow
                    .as_ref()
                    .and_then(Weak::upgrade)
                    .is_some_and(|flow| flow.validate().is_err())
            {
                return false;
            }
            let Ok(config) = manager
                .read()
                .and_then(|doc| McpSettingsManager::config(&doc))
            else {
                return false;
            };
            config["connectors"]
                .as_array()
                .unwrap()
                .iter()
                .any(|current| {
                    current["id"] == self.0.id && {
                        let identity = credential_identity(current);
                        identity == before || identity == next
                    }
                })
        };
        let result = manager.persist_locked(&mut document, &config, &version, &check);
        if self.0.flow.is_some()
            && (result.is_ok()
                || result
                    .as_ref()
                    .is_err_and(|e| e.code == "mcp_write_unconfirmed"))
        {
            // 浏览器的新凭据一经提交或结果未知，就撤销旧SDK存储器的写资格；
            // 同一写锁内发布代次，迟到刷新不能覆盖或清空新登录。
            bump(&manager.oauth_credential_epochs, &self.0.id);
            self.0.credential_epoch.store(
                epoch(&manager.oauth_credential_epochs, &self.0.id),
                Ordering::SeqCst,
            );
        }
        if result.is_ok() {
            *self.0.expected.lock().unwrap_or_else(|p| p.into_inner()) = next;
            if let Some(flow) = self.0.flow.as_ref().and_then(Weak::upgrade) {
                flow.committed.store(1, Ordering::SeqCst);
            }
        } else if result
            .as_ref()
            .is_err_and(|e| e.code == "mcp_write_unconfirmed")
        {
            if let Some(flow) = self.0.flow.as_ref().and_then(Weak::upgrade) {
                flow.committed.store(2, Ordering::SeqCst);
            }
        }
        result.map_err(|e| AuthError::CredentialStoreError(e.message))
    }
}
#[async_trait::async_trait]
impl CredentialStore for ConnectorStore {
    async fn load(&self) -> std::result::Result<Option<StoredCredentials>, AuthError> {
        let manager = self.manager()?;
        let current = self
            .checked(&manager)
            .map_err(|e| AuthError::CredentialStoreError(e.message))?;
        stored(&current).map_err(|e| AuthError::CredentialStoreError(e.message))
    }
    async fn save(&self, credentials: StoredCredentials) -> std::result::Result<(), AuthError> {
        self.write(Some(credentials))
    }
    async fn clear(&self) -> std::result::Result<(), AuthError> {
        self.write(None)
    }
    async fn acquire_refresh_guard(
        &self,
    ) -> std::result::Result<Option<CredentialRefreshGuard>, AuthError> {
        Ok(Some(CredentialRefreshGuard::new(
            self.0.refresh.clone().lock_owned().await,
        )))
    }
}
impl OAuthFlow {
    fn validate(&self) -> Result<()> {
        self.authority.validate()?;
        let manager = self
            .store
            .0
            .manager
            .upgrade()
            .ok_or_else(|| storage("MCP管理器已退出"))?;
        if manager.closing.load(Ordering::SeqCst)
            || epoch(&manager.oauth_flow_epochs, &self.connector_id)
                != self.flow_epoch.load(Ordering::SeqCst)
        {
            return Err(McpSettingsError::new(
                409,
                "mcp_oauth_changed",
                "OAuth连接器配置或生命周期已改变",
            ));
        }
        if self.cancel.is_cancelled() {
            return Err(McpSettingsError::new(
                409,
                "mcp_oauth_cancelled",
                "OAuth等待已取消",
            ));
        }
        if self.created.elapsed() >= FLOW_BUDGET {
            return Err(McpSettingsError::new(
                409,
                "mcp_oauth_expired",
                "OAuth等待已过期",
            ));
        }
        Ok(())
    }
    fn fail(&self, message: &str) {
        let mut status = self.status.lock().unwrap_or_else(|p| p.into_inner());
        if status.status != "cancelled" {
            status.status = "error";
            status.error = Some(message.into());
        }
    }
    fn view(&self) -> McpOAuthPollResponse {
        let status = self
            .status
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let committed = self.committed.load(Ordering::SeqCst);
        let mut error = status.error;
        if committed == 1 && status.status != "done" {
            error = Some(format!(
                "登录凭据已保存；后续收尾未确认。{}",
                error.unwrap_or_default()
            ));
        } else if committed == 2 {
            error = Some("OAuth凭据写入结果尚未确认；不能假定未保存，也不会自动重放。".into());
        }
        McpOAuthPollResponse {
            status: status.status.into(),
            error,
            result: (committed == 1).then(|| json!({"connectorId":self.connector_id})),
            credential_persistence: match committed {
                1 => Some("confirmed".into()),
                2 => Some("unknown".into()),
                _ => None,
            },
        }
    }
}
fn cancel_flow(flow: &OAuthFlow, reason: &str) -> bool {
    let mut status = flow.status.lock().unwrap_or_else(|p| p.into_inner());
    if matches!(status.status, "pending" | "exchanging")
        && flow.committed.load(Ordering::SeqCst) == 0
    {
        status.status = "cancelled";
        flow.cancel.cancel(reason);
        true
    } else {
        false
    }
}
pub(super) fn cancel_connector(
    manager: &McpSettingsManager,
    id: &str,
    reason: &str,
    credentials: bool,
) {
    let _writer = manager
        .source_write
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    cancel_connector_locked(manager, id, reason, credentials);
}
fn cancel_connector_locked(
    manager: &McpSettingsManager,
    id: &str,
    reason: &str,
    credentials: bool,
) {
    bump(&manager.oauth_flow_epochs, id);
    if credentials {
        bump(&manager.oauth_credential_epochs, id);
    }
    for flow in manager
        .oauth_flows
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .values()
    {
        if flow.connector_id == id {
            cancel_flow(flow, reason);
        }
    }
}
pub(super) fn cancel_all(manager: &McpSettingsManager, reason: &str) {
    let _writer = manager
        .source_write
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    for flow in manager
        .oauth_flows
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .values()
    {
        flow.cancel.cancel(reason);
        cancel_flow(flow, reason);
    }
}
async fn guarded<T>(
    flow: &OAuthFlow,
    request: impl std::future::Future<Output = std::result::Result<T, AuthError>>,
) -> Result<T> {
    tokio::pin!(request);
    let invalid = async {
        loop {
            if let Err(error) = flow.validate() {
                return error;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::select! {
        result=tokio::time::timeout(CONNECT_BUDGET,&mut request)=>match result{
            Ok(Ok(value))=>{flow.validate()?;Ok(value)},
            Ok(Err(_))=>Err(McpSettingsError::new(502,"mcp_oauth_failed","MCP OAuth外端未完成授权；不会自动重放")),
            Err(_)=>Err(McpSettingsError::new(504,"mcp_oauth_timeout","MCP OAuth请求超时；远端结果未知，不会自动重放")),
        },error=invalid=>Err(error),
    }
}
fn network(manager: &McpSettingsManager) -> Result<mcpbridge::http::NetworkMcpClient> {
    let plane = manager
        .network
        .get()
        .and_then(Weak::upgrade)
        .ok_or_else(|| {
            McpSettingsError::new(503, "mcp_network_unavailable", "共享MCP网络未接通")
        })?;
    mcpbridge::http::NetworkMcpClient::new(&plane)
        .map_err(|_| McpSettingsError::new(503, "mcp_network_unavailable", "共享MCP网络不可用"))
}
fn endpoint_url(connector: &Value) -> Result<String> {
    string(
        connector
            .get("url")
            .or_else(|| connector.get("baseUrl"))
            .unwrap_or(&Value::Null),
        "url",
    )
}
pub(super) async fn runtime_endpoint(
    manager: &Arc<McpSettingsManager>,
    connector: &Value,
) -> Result<McpEndpoint> {
    let ordinary = McpSettingsManager::endpoint(connector)?;
    if !matches!(ordinary, McpEndpoint::Unsupported { .. }) || connector["transport"] == "sse" {
        return Ok(ordinary);
    }
    let url = endpoint_url(connector)?;
    let client = network(manager)?;
    let headers = connector
        .get("headers")
        .filter(|v| !v.is_null())
        .map(|v| {
            serde_json::from_value::<BTreeMap<String, String>>(v.clone())
                .map_err(|_| invalid("headers必须为文本映射"))
        })
        .transpose()?
        .unwrap_or_default();
    let mut custom = std::collections::HashMap::new();
    for (key, value) in headers {
        let key = reqwest::header::HeaderName::from_bytes(key.as_bytes())
            .map_err(|_| invalid("MCP header名称无效"))?;
        let value = reqwest::header::HeaderValue::from_str(&value)
            .map_err(|_| invalid("MCP header内容无效"))?;
        custom.insert(key, value);
    }
    let oauth = if connector["authType"] == "oauth" {
        let current = stored(connector)?.ok_or_else(|| {
            McpSettingsError::new(409, "mcp_authorization_required", "MCP连接器需要先登录")
        })?;
        let registration = Arc::new(std::sync::Mutex::new(RegistrationCapture::default()));
        let store = ConnectorStore(Arc::new(StoreInner {
            manager: Arc::downgrade(manager),
            id: connector["id"].as_str().unwrap().into(),
            expected: std::sync::Mutex::new(credential_identity(connector)),
            credential_epoch: AtomicU64::new(epoch(
                &manager.oauth_credential_epochs,
                connector["id"].as_str().unwrap(),
            )),
            flow: None,
            registration: registration.clone(),
            refresh: Arc::new(Mutex::new(())),
        }));
        let http = OAuthNetwork {
            network: client.clone(),
            registration,
        };
        let mut auth =
            AuthorizationManager::new_with_oauth_http_client(url.clone(), Arc::new(http))
                .await
                .map_err(|_| invalid("MCP OAuth配置无效"))?;
        auth.set_credential_store(store);
        if !auth.initialize_from_store().await.map_err(|_| {
            McpSettingsError::new(
                409,
                "mcp_authorization_required",
                "MCP OAuth凭据未通过当前授权服务验证",
            )
        })? {
            return Err(McpSettingsError::new(
                409,
                "mcp_authorization_required",
                "MCP连接器需要重新登录",
            ));
        }
        let redirect = connector
            .pointer("/oauth/redirectUri")
            .and_then(Value::as_str)
            .ok_or_else(|| storage("MCP OAuth回调配置缺失"))?;
        let mut config = OAuthClientConfig::new(current.client_id, redirect);
        if let Some(secret) = connector["oauthClientSecret"]
            .as_str()
            .filter(|s| !s.is_empty())
        {
            config = config.with_client_secret(secret);
        }
        auth.configure_client(config)
            .map_err(|_| invalid("MCP OAuth客户端配置无效"))?;
        Some(sdk::AuthClient::new(client.clone(), auth))
    } else {
        None
    };
    let bearer = if connector["authType"] == "bearer" {
        connector["authorizationToken"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    } else {
        None
    };
    Ok(McpEndpoint::StreamableHttp {
        url,
        client,
        headers: custom,
        bearer,
        oauth,
    })
}
async fn start(
    manager: &Arc<McpSettingsManager>,
    authority: SettingsAuthority,
    id: &str,
    redirect: String,
) -> Result<McpOAuthStartResponse> {
    authority.validate()?;
    validate_id(id)?;
    let connector = {
        let _runtime = manager.runtime.lock().await;
        authority.validate()?;
        let config = McpSettingsManager::config(&manager.read()?)?;
        config["connectors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .cloned()
            .ok_or_else(|| McpSettingsError::new(404, "mcp_connector_missing", "连接器不存在"))?
    };
    if connector["authType"] != "oauth"
        || connector["transport"] == "stdio"
        || connector["transport"] == "sse"
    {
        return Err(invalid("此连接器不支持当前OAuth HTTP授权"));
    }
    let client = network(manager)?;
    let id = id.to_owned();
    let registration = Arc::new(std::sync::Mutex::new(RegistrationCapture::default()));
    let flow = Arc::new_cyclic(|weak| OAuthFlow {
        connector_id: id.clone(),
        flow_epoch: AtomicU64::new(epoch(&manager.oauth_flow_epochs, &id)),
        authority,
        cancel: crate::cancel::CancelScope::background_root("mcp-oauth"),
        created: Instant::now(),
        status: std::sync::Mutex::new(FlowStatus {
            status: "pending",
            error: None,
        }),
        store: ConnectorStore(Arc::new(StoreInner {
            manager: Arc::downgrade(manager),
            id: id.clone(),
            expected: std::sync::Mutex::new(credential_identity(&connector)),
            credential_epoch: AtomicU64::new(epoch(&manager.oauth_credential_epochs, &id)),
            flow: Some(weak.clone()),
            registration: registration.clone(),
            refresh: Arc::new(Mutex::new(())),
        })),
        session: Mutex::new(None),
        committed: AtomicU8::new(0),
        active: AtomicBool::new(false),
        changed: Notify::new(),
        redirect_uri: redirect.clone(),
    });
    let provisional = am::random_token_suffix();
    {
        let _writer = manager
            .source_write
            .lock()
            .map_err(|_| storage("MCP Source写锁损坏"))?;
        flow.store.checked(manager).map_err(|_| {
            McpSettingsError::new(409, "mcp_oauth_changed", "OAuth连接器配置已改变")
        })?;
        let mut flows = manager
            .oauth_flows
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let waiting = |f: &OAuthFlow| {
            f.active.load(Ordering::SeqCst)
                || matches!(
                    f.status.lock().unwrap_or_else(|p| p.into_inner()).status,
                    "pending" | "exchanging"
                )
        };
        flows.retain(|_, f| f.created.elapsed() < FLOW_BUDGET || f.active.load(Ordering::SeqCst));
        if flows.values().filter(|f| waiting(f)).count() >= FLOW_LIMIT {
            return Err(McpSettingsError::new(
                429,
                "mcp_oauth_busy",
                "等待中的MCP登录过多，请先取消",
            ));
        }
        // 等待上限与历史回执分别有界；取消确实释放等待名额，历史不改成未保存。
        let mut history = flows
            .iter()
            .filter(|(_, f)| !waiting(f))
            .map(|(key, f)| (key.clone(), f.created))
            .collect::<Vec<_>>();
        history.sort_by_key(|(_, created)| *created);
        let excess = history.len().saturating_sub(FLOW_LIMIT.saturating_sub(1));
        for (key, _) in history.into_iter().take(excess) {
            flows.remove(&key);
        }
        // 同连接器的新用户意图使旧浏览器state失效，尚未登录时继续保留原凭据。
        bump(&manager.oauth_flow_epochs, &id);
        flow.flow_epoch
            .store(epoch(&manager.oauth_flow_epochs, &id), Ordering::SeqCst);
        for previous in flows
            .values()
            .filter(|previous| previous.connector_id == id)
        {
            cancel_flow(previous, "新的OAuth登录已替换旧等待");
        }
        flows.insert(provisional.clone(), flow.clone());
    }
    let result = async {
        let mut auth = guarded(
            &flow,
            AuthorizationManager::new_with_oauth_http_client(
                endpoint_url(&connector)?,
                Arc::new(OAuthNetwork {
                    network: client,
                    registration: registration.clone(),
                }),
            ),
        )
        .await?;
        let metadata = guarded(&flow, auth.resolve_metadata()).await?;
        registration
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .endpoint = metadata.metadata.registration_endpoint.clone();
        auth.set_metadata(metadata.metadata);
        auth.set_credential_store(flow.store.clone());
        let mut request = AuthorizationRequest::new(redirect).with_client_name("Lingxi MCP");
        if let Some(client_id) = connector["oauthClientId"]
            .as_str()
            .filter(|s| !s.is_empty())
        {
            request = request.with_preregistered_client(client_id);
            if let Some(secret) = connector["oauthClientSecret"]
                .as_str()
                .filter(|s| !s.is_empty())
            {
                request = request.with_client_secret(secret);
            }
        }
        let session = guarded(&flow, async {
            AuthorizationSession::new(auth, request)
                .await
                .map_err(|(_, error)| error)
        })
        .await?;
        let url = session.get_authorization_url().to_owned();
        let state = reqwest::Url::parse(&url)
            .ok()
            .and_then(|url| {
                url.query_pairs()
                    .find(|(key, _)| key == "state")
                    .map(|(_, value)| value.into_owned())
            })
            .filter(|s| !s.is_empty())
            .ok_or_else(|| storage("官方OAuth没有返回state"))?;
        {
            let _writer = manager
                .source_write
                .lock()
                .map_err(|_| storage("MCP Source写锁损坏"))?;
            flow.store.checked(manager).map_err(|_| {
                McpSettingsError::new(409, "mcp_oauth_changed", "授权或连接配置已改变")
            })?;
            let mut flows = manager
                .oauth_flows
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            flows.remove(&provisional);
            flows.insert(state.clone(), flow.clone());
        }
        *flow.session.lock().await = Some(session);
        Ok::<_, McpSettingsError>(McpOAuthStartResponse {
            session_id: state,
            url,
        })
    }
    .await;
    if let Err(error) = &result {
        flow.fail(&error.message);
    }
    result
}
struct ActiveFlow(Arc<OAuthFlow>);
impl Drop for ActiveFlow {
    fn drop(&mut self) {
        self.0.active.store(false, Ordering::SeqCst);
        self.0.changed.notify_waiters();
    }
}
async fn complete(
    manager: &Arc<McpSettingsManager>,
    state: &str,
    code: &str,
    error: &str,
    issuer: Option<&str>,
) -> Result<()> {
    let flow = manager
        .oauth_flows
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(state)
        .cloned()
        .ok_or_else(|| invalid("OAuth session not found"))?;
    {
        let _writer = manager
            .source_write
            .lock()
            .map_err(|_| storage("MCP Source写锁损坏"))?;
        flow.store.checked(manager).map_err(|_| {
            McpSettingsError::new(409, "mcp_oauth_changed", "OAuth原授权或连接配置已失效")
        })?;
        let mut status = flow.status.lock().unwrap_or_else(|p| p.into_inner());
        if status.status != "pending" {
            return Err(McpSettingsError::new(
                409,
                "mcp_oauth_replayed",
                "OAuth流程已处理或取消",
            ));
        }
        status.status = "exchanging";
        flow.active.store(true, Ordering::SeqCst);
    }
    let _active = ActiveFlow(flow.clone());
    let result = async {
        if !error.is_empty() {
            return Err(invalid("OAuth外端拒绝授权"));
        }
        if code.is_empty() {
            return Err(invalid("OAuth code is required"));
        }
        let session = flow.session.lock().await;
        let session = session
            .as_ref()
            .ok_or_else(|| invalid("OAuth流程尚未就绪"))?;
        guarded(
            &flow,
            session.handle_callback_with_issuer(code, state, issuer),
        )
        .await?;
        if flow.committed.load(Ordering::SeqCst) != 1 {
            return Err(storage("OAuth凭据保存未确认"));
        }
        manager.cancel_pending(Some(&flow.connector_id), "OAuth凭据已更新");
        manager.wait_pending(Some(&flow.connector_id)).await?;
        let mut runtime = manager.runtime.lock().await;
        flow.validate()?;
        flow.store
            .checked(manager)
            .map_err(|_| McpSettingsError::new(409, "mcp_oauth_changed", "OAuth连接配置已改变"))?;
        manager
            .stop_connector(&mut runtime, &flow.connector_id)
            .await
            .map_err(McpSettingsError::saved)?;
        let mut status = flow.status.lock().unwrap_or_else(|p| p.into_inner());
        status.status = "done";
        Ok(())
    }
    .await;
    if let Err(error) = &result {
        flow.fail(&error.message);
    }
    result
}
fn require_owner(state: &ServiceState, principal: &Principal) -> Result<SettingsAuthority> {
    let authority = SettingsAuthority {
        auth: state.auth.clone(),
        original: principal.clone(),
    };
    authority.validate()?;
    Ok(authority)
}
fn response_error(error: McpSettingsError) -> Response {
    (
        StatusCode::from_u16(error.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(json!({"error":error.code,"message":error.message,"saved":error.saved})),
    )
        .into_response()
}
async fn start_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Extension(secure): Extension<crate::serve::SecureTransport>,
    Path(id): Path<String>,
) -> Response {
    let authority = match require_owner(&state, &principal) {
        Ok(v) => v,
        Err(e) => return response_error(e),
    };
    let ip = if state.config().bind_addr.ip().is_ipv6() {
        "[::1]"
    } else {
        "127.0.0.1"
    };
    let redirect = format!(
        "{}://{ip}:{}/lingxi/v1/mcp/oauth/callback",
        if secure.0 { "https" } else { "http" },
        state.actual_port()
    );
    match start(state.mcp_settings(), authority, &id, redirect).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => response_error(e),
    }
}
async fn poll_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
) -> Response {
    if let Err(e) = require_owner(&state, &principal) {
        return response_error(e);
    }
    let flow = state
        .mcp_settings()
        .oauth_flows
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(&id)
        .cloned();
    match flow {
        None => Json(McpOAuthPollResponse {
            status: "missing".into(),
            error: None,
            result: None,
            credential_persistence: None,
        })
        .into_response(),
        Some(flow) => {
            let pending = matches!(
                flow.status.lock().unwrap_or_else(|p| p.into_inner()).status,
                "pending" | "exchanging"
            );
            if pending {
                if let Err(error) = flow.validate() {
                    flow.fail(&error.message);
                }
            }
            Json(flow.view()).into_response()
        }
    }
}
async fn cancel_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
) -> Response {
    let result = (|| {
        let authority = require_owner(&state, &principal)?;
        let manager = state.mcp_settings();
        let _writer = manager
            .source_write
            .lock()
            .map_err(|_| storage("MCP Source写锁损坏"))?;
        authority.validate()?;
        let config = McpSettingsManager::config(&manager.read()?)?;
        if !config["connectors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == id)
        {
            return Err(McpSettingsError::new(
                404,
                "mcp_connector_missing",
                "连接器不存在",
            ));
        }
        let mut count = 0u32;
        for flow in manager
            .oauth_flows
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
        {
            if flow.connector_id == id && cancel_flow(flow, "用户取消OAuth等待") {
                count += 1;
            }
        }
        Ok(McpOAuthCancelResponse { cancelled: count })
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => response_error(e),
    }
}
async fn logout_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
) -> Response {
    let result = async {
        let authority = require_owner(&state, &principal)?;
        let manager = state.mcp_settings();
        validate_id(&id)?;
        {
            let _runtime = manager.runtime.lock().await;
            authority.validate()?;
            let config = McpSettingsManager::config(&manager.read()?)?;
            if !config["connectors"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["id"] == id)
            {
                return Err(McpSettingsError::new(
                    404,
                    "mcp_connector_missing",
                    "连接器不存在",
                ));
            }
            cancel_connector(manager, &id, "用户退出OAuth登录", true);
            manager.cancel_pending(Some(&id), "用户退出OAuth登录");
        }
        manager.wait_pending(Some(&id)).await?;
        let mut runtime = manager.runtime.lock().await;
        authority.validate()?;
        {
            let _writer = manager
                .source_write
                .lock()
                .map_err(|_| storage("MCP Source写锁损坏"))?;
            cancel_connector_locked(manager, &id, "用户退出OAuth登录", true);
            let (mut doc, version) = manager.read_versioned()?;
            let mut config = McpSettingsManager::config(&doc)?;
            let connector = config["connectors"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|c| c["id"] == id)
                .ok_or_else(|| {
                    McpSettingsError::new(404, "mcp_connector_missing", "连接器不存在")
                })?;
            connector["oauth"] = json!({});
            connector["authorizationToken"] = json!("");
            manager.persist_locked(&mut doc, &config, &version, &|| {
                authority.validate().is_ok()
            })?;
        }
        manager
            .stop_connector(&mut runtime, &id)
            .await
            .map_err(McpSettingsError::saved)?;
        drop(runtime);
        let view = manager.state().await?;
        authority.validate()?;
        let connector = view["config"]["connectors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .cloned()
            .unwrap_or(Value::Null);
        let state = serde_json::from_value::<McpSettingsState>(view)
            .map_err(|_| storage("MCP退出状态不符合公共合同"))?;
        Ok::<_, McpSettingsError>(McpOAuthLogoutResponse { connector, state })
    }
    .await;
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => response_error(e),
    }
}
#[derive(Deserialize)]
struct Callback {
    #[serde(default)]
    state: String,
    #[serde(default)]
    code: String,
    #[serde(default)]
    error: String,
    iss: Option<String>,
}
async fn callback_route(
    State(state): State<ServiceState>,
    Extension(connection): Extension<crate::ConnectionKind>,
    Query(query): Query<Callback>,
) -> Response {
    if connection != crate::ConnectionKind::Local {
        return response_error(McpSettingsError::new(
            403,
            "mcp_callback_local_only",
            "OAuth回调只接受本机浏览器",
        ));
    }
    let Some(tasks) = state.mcp_settings().tasks.get().and_then(Weak::upgrade) else {
        return response_error(storage("OAuth回调监管者不可用"));
    };
    let manager = state.mcp_settings().clone();
    let task = tasks.spawn_detached("mcp-oauth-callback".into(), async move {
        complete(
            &manager,
            &query.state,
            &query.code,
            &query.error,
            query.iss.as_deref(),
        )
        .await
    });
    let result = match task {
        Ok(task) => task
            .wait()
            .await
            .map_err(|_| storage("OAuth回调执行结果未知"))
            .and_then(|v| v),
        Err(_) => Err(storage("OAuth回调任务未被接受")),
    };
    let (ok, status) = match result {
        Ok(()) => (true, StatusCode::OK),
        Err(_) => (false, StatusCode::BAD_REQUEST),
    };
    let text = if ok {
        "连接器已登录。可以关闭此窗口返回灵犀。"
    } else {
        "连接器登录未完成。请返回灵犀查看实际状态；此回调不会自动重放。"
    };
    (
        status,
        [
            (axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (axum::http::header::CACHE_CONTROL, "no-store"),
            (
                axum::http::header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; frame-ancestors 'none'",
            ),
        ],
        format!("<!doctype html><meta charset=utf-8><title>MCP OAuth</title><p>{text}</p>"),
    )
        .into_response()
}
pub(super) async fn drain(manager: &McpSettingsManager) -> Result<()> {
    let flows = manager
        .oauth_flows
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .values()
        .cloned()
        .collect::<Vec<_>>();
    tokio::time::timeout(CONNECT_BUDGET, async {
        for flow in flows {
            loop {
                let changed = flow.changed.notified();
                if !flow.active.load(Ordering::SeqCst) {
                    break;
                }
                changed.await;
            }
        }
    })
    .await
    .map_err(|_| McpSettingsError::new(500, "mcp_cleanup_unconfirmed", "OAuth回调退出未确认"))
}
pub(super) fn router() -> Router<ServiceState> {
    Router::new()
        .route(
            "/lingxi/v1/mcp/connectors/{id}/oauth/start",
            post(start_route),
        )
        .route(
            "/lingxi/v1/mcp/connectors/{id}/oauth/cancel",
            post(cancel_route),
        )
        .route(
            "/lingxi/v1/mcp/connectors/{id}/oauth/logout",
            post(logout_route),
        )
        .route("/lingxi/v1/mcp/oauth/poll/{id}", get(poll_route))
        .route("/lingxi/v1/mcp/oauth/callback", get(callback_route))
}
