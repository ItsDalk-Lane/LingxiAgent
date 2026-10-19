//! 只从当前真实 SDK 目录派生应用；远端 ui:// 内容不转换成本地文件授权。
use super::*;
use axum::extract::Path;
use base64::Engine as _;
use lingxi_protocol::{
    McpAppLaunchRequest as LaunchInput, McpAppLaunchResponse, McpAppsQuery as AppQuery,
    McpAppsResponse, McpResourceQuery as ResourceQuery,
};

fn error_response(error: McpSettingsError) -> Response {
    (
        StatusCode::from_u16(error.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(json!({"error":error.code,"message":error.message})),
    )
        .into_response()
}
fn owner(state: &ServiceState, principal: &Principal) -> Result<SettingsAuthority> {
    let authority = SettingsAuthority {
        auth: state.auth.clone(),
        original: principal.clone(),
    };
    authority.validate()?;
    Ok(authority)
}
fn descriptor(id: &str, tool: &rmcp::model::Tool) -> Option<Value> {
    let wire = serde_json::to_value(tool).ok()?;
    let meta = wire.get("_meta");
    let ui = meta.and_then(|m| m.get("ui"));
    let uri = ui
        .and_then(|u| u.get("resourceUri"))
        .and_then(Value::as_str)
        .or_else(|| {
            meta.and_then(|m| m.get("openai/outputTemplate"))
                .and_then(Value::as_str)
        })?;
    if !uri.starts_with("ui://") {
        return None;
    }
    let visibility = match ui.and_then(|u| u.get("visibility")) {
        None => vec!["model".to_owned(), "app".to_owned()],
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect(),
        _ => vec![],
    };
    if !visibility.iter().any(|v| v == "app") {
        return None;
    }
    let mut app = json!({"connectorId":id,"serverId":id,"toolName":tool.name,
        "title":wire.get("title").and_then(Value::as_str).unwrap_or(tool.name.as_ref()),
        "description":tool.description.as_deref().unwrap_or(""),"resourceUri":uri,
        "visibility":visibility,"inputSchema":wire["inputSchema"]});
    for key in ["outputSchema", "_meta"] {
        if let Some(value) = wire.get(key) {
            app[key] = value.clone();
        }
    }
    Some(app)
}
struct LiveProof {
    id: String,
    server: Arc<McpServer>,
    epoch: u64,
    connector: Value,
    agent: Option<String>,
}
impl LiveProof {
    async fn validate(
        &self,
        manager: &McpSettingsManager,
        authority: &SettingsAuthority,
    ) -> Result<()> {
        authority.validate()?;
        if manager.closing.load(Ordering::SeqCst) {
            return Err(McpSettingsError::new(409, "mcp_stopping", "服务正在退出"));
        }
        let runtime = manager.runtime.lock().await;
        let config = McpSettingsManager::config(&manager.read()?)?;
        let current = config["connectors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == self.id)
            .ok_or_else(|| McpSettingsError::new(404, "mcp_connector_missing", "连接器不存在"))?;
        if config["enabled"] != true || current["enabled"] == false {
            return Err(McpSettingsError::new(
                409,
                "mcp_connector_disabled",
                "连接器已停用，请在MCP设置中启用",
            ));
        }
        if oauth::connection_identity(current) != self.connector
            || self.server.metadata_epoch() != self.epoch
            || !runtime
                .running
                .get(&self.id)
                .is_some_and(|r| Arc::ptr_eq(&r.server, &self.server))
        {
            return Err(McpSettingsError::new(
                409,
                "mcp_resource_changed",
                "连接器或应用目录已改变，旧资源结果未返回",
            ));
        }
        if let Some(agent) = &self.agent {
            manager.agent(agent)?;
            manager.allowed(agent, &self.id, None)?;
        }
        Ok(())
    }
}
async fn live(
    manager: &Arc<McpSettingsManager>,
    authority: &SettingsAuthority,
    id: &str,
    agent: Option<&str>,
    ensure_started: bool,
) -> Result<LiveProof> {
    validate_id(id)?;
    authority.validate()?;
    // 资源按旧含义可以为尚未连接的已启用连接器按需建立真实连接。
    if manager.has_pending(id) {
        manager.wait_pending(Some(id)).await?;
    }
    let mut runtime = manager.runtime.lock().await;
    authority.validate()?;
    let config = McpSettingsManager::config(&manager.read()?)?;
    let connector = config["connectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .ok_or_else(|| McpSettingsError::new(404, "mcp_connector_missing", "连接器不存在"))?;
    if config["enabled"] != true || connector["enabled"] == false {
        return Err(McpSettingsError::new(
            409,
            "mcp_connector_disabled",
            "连接器已停用，请在MCP设置中启用",
        ));
    }
    if let Some(agent) = agent {
        manager.agent(agent)?;
    }
    if !runtime
        .running
        .get(id)
        .is_some_and(|r| r.server.is_connected())
    {
        if !ensure_started {
            return Err(McpSettingsError::new(
                409,
                "mcp_app_directory_unavailable",
                "当前连接尚无实际应用目录，请先启动连接器",
            ));
        }
        manager
            .start_connector(&mut runtime, connector, None, Some(authority))
            .await?;
    }
    if let Some(agent) = agent {
        manager.allowed(agent, id, None)?;
    }
    let server = runtime.running.get(id).unwrap().server.clone();
    Ok(LiveProof {
        id: id.into(),
        epoch: server.metadata_epoch(),
        server,
        connector: oauth::connection_identity(connector),
        agent: agent.map(str::to_owned),
    })
}
async fn apps_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<AppQuery>,
) -> Response {
    let result = async {
        let authority = owner(&state, &principal)?;
        let manager = state.mcp_settings();
        manager.gateway()?;
        let mut apps = Vec::new();
        let mut proofs = Vec::new();
        {
            let runtime = manager.runtime.lock().await;
            authority.validate()?;
            let config = McpSettingsManager::config(&manager.read()?)?;
            if let Some(agent) = query.agent_id.as_deref() {
                manager.agent(agent)?;
            }
            for connector in config["connectors"].as_array().unwrap() {
                let id = connector["id"].as_str().unwrap();
                if query
                    .connector_id
                    .as_deref()
                    .is_some_and(|requested| requested != id)
                {
                    continue;
                }
                if config["enabled"] != true || connector["enabled"] == false {
                    continue;
                }
                let Some(registered) = runtime.running.get(id).filter(|r| r.server.is_connected())
                else {
                    continue;
                };
                proofs.push(LiveProof {
                    id: id.into(),
                    server: registered.server.clone(),
                    epoch: registered.server.metadata_epoch(),
                    connector: oauth::connection_identity(connector),
                    agent: query.agent_id.clone(),
                });
                for tool in registered.server.wire_tools().values() {
                    if let Some(agent) = query.agent_id.as_deref() {
                        if let Err(error) = manager.allowed(agent, id, Some(tool.name.as_ref())) {
                            if error.status == 403 {
                                continue;
                            }
                            return Err(error);
                        }
                    }
                    if let Some(app) = descriptor(id, tool) {
                        apps.push(app);
                    }
                }
            }
        }
        let view = manager.state_for_agent(query.agent_id.as_deref()).await?;
        authority.validate()?;
        for proof in proofs {
            proof.validate(manager, &authority).await?;
        }
        serde_json::from_value::<McpAppsResponse>(json!({"apps":apps,"state":view}))
            .map_err(|_| storage("MCP应用目录回执不符合公共合同"))
    }
    .await;
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => error_response(error),
    }
}
async fn launch_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path((id, tool)): Path<(String, String)>,
    body: std::result::Result<Json<LaunchInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let result=async {
        let input=match body{Ok(Json(input))=>input,Err(rejection) if rejection.status()==StatusCode::UNSUPPORTED_MEDIA_TYPE=>LaunchInput::default(),Err(_)=>return Err(invalid("invalid JSON request body"))};
        let authority=owner(&state,&principal)?;let manager=state.mcp_settings();manager.gateway()?;
        let proof=live(manager,&authority,&id,input.agent_id.as_deref(),false).await?;
        let app={
            let runtime=manager.runtime.lock().await;authority.validate()?;
            if !runtime.running.get(&id).is_some_and(|r|Arc::ptr_eq(&r.server,&proof.server)){return Err(McpSettingsError::new(409,"mcp_app_changed","连接器已改变"));}
            if let Some(agent)=input.agent_id.as_deref(){manager.allowed(agent,&id,Some(&tool))?;}
            let tools=proof.server.wire_tools();let declared=tools.get(&tool).ok_or_else(||McpSettingsError::new(404,"mcp_app_missing","连接器工具不存在"))?;
            descriptor(&id,declared).ok_or_else(||McpSettingsError::new(403,"mcp_app_not_visible","该工具未向应用公开ui://资源"))?
        };
        proof.validate(manager,&authority).await?;
        let uri=app["resourceUri"].as_str().unwrap();
        let enc=|s:&str|percent_encoding::utf8_percent_encode(s,percent_encoding::NON_ALPHANUMERIC).to_string();
        let binding=json!({"kind":"mcp-app","connectorId":id,"serverId":id,"toolName":tool,"resourceUri":uri,
            "resourceUrl":format!("/lingxi/v1/mcp/connectors/{}/resources?uri={}",enc(&id),enc(uri)),
            "callToolUrl":format!("/lingxi/v1/mcp/connectors/{}/app-tools/{}/call",enc(&id),enc(&tool))});
        // 现役launch只创建描述与输入绑定，不在缺少用户会话时冒充一次工具调用。
        serde_json::from_value::<McpAppLaunchResponse>(json!({"type":"mcp_app","connectorId":id,"serverId":id,"toolName":tool,
            "title":app["title"],"description":app["description"],"resourceUri":uri,"visibility":app["visibility"],
            "launchInput":input.launch_input.or(input.input).unwrap_or(json!({})),"binding":binding,"app":app}))
            .map_err(|_|storage("MCP应用启动描述不符合公共合同"))
    }.await;
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => error_response(error),
    }
}
async fn resource_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
    query: std::result::Result<Query<ResourceQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let result=async {
        let Query(query)=query.map_err(|_|invalid("uri is required"))?;
        if !query.uri.starts_with("ui://") || query.uri.len()>8192 {return Err(invalid("MCP app resource uri must start with ui://"));}
        let authority=owner(&state,&principal)?;let manager=state.mcp_settings();
        let proof=live(manager,&authority,&id,query.agent_id.as_deref(),true).await?;
        let cancel=crate::cancel::CancelScope::background_root("mcp-resource-read");
        let request=proof.server.read_resource(&query.uri,mcpbridge::MCP_DEFAULT_CALL_DEADLINE_MS,Some(cancel.clone()));
        tokio::pin!(request);
        let interrupted=async {
            loop {
                if let Err(error)=proof.validate(manager,&authority).await{return error;}
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        };
        let read=tokio::select!{
            result=&mut request=>result.map_err(|failure|McpSettingsError::new(502,"mcp_resource_read_failed",failure.reason()))?,
            error=interrupted=>{cancel.cancel("资源请求的授权或连接已改变");let _=request.await;return Err(error);}
        };
        proof.validate(manager,&authority).await?;
        let contents=serde_json::to_value(read).map_err(|_|storage("MCP资源序列化失败"))?;
        let items=contents["contents"].as_array().ok_or_else(||McpSettingsError::new(404,"mcp_resource_missing","resource content not found"))?;
        let content=items.iter().find(|item|item["mimeType"].as_str().is_some_and(|mime|mime.starts_with("text/html")))
            .or_else(||items.iter().find(|item|item["uri"]==query.uri)).or_else(||items.first())
            .ok_or_else(||McpSettingsError::new(404,"mcp_resource_missing","resource content not found"))?;
        let mime=content["mimeType"].as_str().filter(|s|!s.is_empty()).unwrap_or("text/plain; charset=utf-8");
        let bytes=if let Some(text)=content["text"].as_str(){text.as_bytes().to_vec()}
            else if let Some(blob)=content["blob"].as_str(){base64::engine::general_purpose::STANDARD.decode(blob).map_err(|_|McpSettingsError::new(502,"mcp_resource_invalid","resource blob is not base64"))?}
            else{return Err(McpSettingsError::new(502,"mcp_resource_invalid","resource content has no text or blob"));};
        if bytes.len()>1024*1024{return Err(McpSettingsError::new(502,"mcp_resource_too_large","MCP资源超过1MiB"));}
        let content_type=axum::http::HeaderValue::from_str(mime).map_err(|_|McpSettingsError::new(502,"mcp_resource_invalid","resource mime is invalid"))?;
        let mut response=bytes.into_response();
        response.headers_mut().insert(axum::http::header::CONTENT_TYPE,content_type);
        response.headers_mut().insert(axum::http::header::CACHE_CONTROL,axum::http::HeaderValue::from_static("no-store"));
        response.headers_mut().insert(axum::http::header::CONTENT_SECURITY_POLICY,axum::http::HeaderValue::from_static("sandbox allow-scripts; base-uri 'none'; form-action 'none'; frame-ancestors 'self'"));
        response.headers_mut().insert(axum::http::header::X_CONTENT_TYPE_OPTIONS,axum::http::HeaderValue::from_static("nosniff"));
        Ok::<_,McpSettingsError>(response)
    }.await;
    match result {
        Ok(response) => response,
        Err(error) => error_response(error),
    }
}
pub(super) fn router() -> Router<ServiceState> {
    Router::new()
        .route("/lingxi/v1/mcp/apps", get(apps_route))
        .route(
            "/lingxi/v1/mcp/connectors/{id}/apps/{tool}/launch",
            post(launch_route),
        )
        .route(
            "/lingxi/v1/mcp/connectors/{id}/resources",
            get(resource_route),
        )
}
