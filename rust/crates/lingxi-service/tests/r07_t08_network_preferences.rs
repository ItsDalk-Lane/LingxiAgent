//! 保存代理后下一次真实模型调用使用共享网络平面；所有连接均到自有本地代理。
use axum::{extract::State, http::HeaderMap, response::IntoResponse, routing::post, Json, Router};
use lingxi_adapters::models::{
    config::ModelPlaneConfig, gateway::ConfigModelGateway, network::NetworkPlane,
};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
async fn proxy(
    State(count): State<Arc<AtomicUsize>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    assert_eq!(
        headers.get("authorization").unwrap(),
        "Bearer local-only-fixture-key"
    );
    assert_eq!(headers.get("host").unwrap(), "model-fixture.invalid");
    let index = count.fetch_add(1, Ordering::SeqCst);
    assert_eq!(
        body["model"],
        ["health", "knowledge-live", "summary-live"][index],
        "每次调用必须实际使用自己的目录模型"
    );
    let content=json!({"id":"proxy-fixture","object":"chat.completion.chunk","model":body["model"],"choices":[{"index":0,"delta":{"role":"assistant","content":"OK"},"finish_reason":null}]}).to_string();
    let end=json!({"id":"proxy-fixture","object":"chat.completion.chunk","model":body["model"],"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}}).to_string();
    (
        [("content-type", "text/event-stream")],
        format!("data: {content}\n\ndata: {end}\n\ndata: [DONE]\n\n"),
    )
}
async fn request(
    state: &ServiceState,
    base: &str,
    method: reqwest::Method,
    path: &str,
    body: Value,
) -> (u16, Value) {
    let response = reqwest::Client::new()
        .request(method, format!("{base}{path}"))
        .bearer_auth(state.auth().local_token())
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = response.bytes().await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"raw":String::from_utf8_lossy(&bytes)})),
    )
}
#[tokio::test]
async fn saved_network_proxy_reaches_actual_next_model_dispatch_without_restart_or_loopback_bypass()
{
    let home = std::env::temp_dir().join(format!(
        "lingxi-r07-t08-network-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    std::fs::create_dir_all(&home).unwrap();
    let home = home.canonicalize().unwrap();
    let layout = prepare_layout(&home).unwrap();
    let plane = ModelPlaneConfig::from_value(json!({"providers":{},"models":{}})).unwrap();
    let clock = Arc::new(lingxi_service::inject::SystemClock);
    let credentials = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &layout.runtime_dir,
            clock.clone(),
        )
        .unwrap(),
    );
    let state = ServiceState::bootstrap_with_deps(
        ServiceConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            data_home: home.clone(),
            home_source: HomeSource::Cli,
            network_mode: NetworkMode::Loopback,
            shutdown_timeout_ms: 1000,
        },
        &layout,
        ServiceDeps {
            model_gateway: Some(Arc::new(ConfigModelGateway::from_validated(plane))),
            credential_service: Some(credentials),
            network_plane: Some(NetworkPlane::direct_isolated()),
            model_plane_activation: Some(Arc::new(AtomicBool::new(false))),
            clock,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let proxy_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route("/v1/chat/completions", post(proxy))
        .with_state(count.clone());
    let proxy_task = tokio::spawn(async move { axum::serve(proxy_listener, app).await.unwrap() });
    let (stop, stop_rx) = tokio::sync::oneshot::channel();
    let (ready, address) = tokio::sync::oneshot::channel();
    let serving = state.clone();
    let server = tokio::spawn(async move {
        lingxi_service::run(
            serving,
            async {
                let _ = stop_rx.await;
            },
            |addr| {
                let _ = ready.send(addr);
            },
            None,
        )
        .await
        .unwrap()
    });
    let base = format!("http://{}", address.await.unwrap());
    let (status,result)=request(&state,&base,reqwest::Method::PUT,"/lingxi/v1/settings/config",json!({"network_proxy":{"mode":"manual","httpProxy":format!("http://{proxy_addr}"),"noProxy":""},"providers":{"fixture":{"api":"openai-completions","base_url":"http://model-fixture.invalid/v1","api_key":"local-only-fixture-key","models":[{"id":"health","inputs":["text"]},{"id":"knowledge-live","inputs":["text"]},{"id":"summary-live","inputs":["text"]}]}}})).await;
    assert_eq!(status, 200, "{result}");
    let (status, result) = request(
        &state,
        &base,
        reqwest::Method::POST,
        "/lingxi/v1/models/health",
        json!({"provider":"fixture","model":"health"}),
    )
    .await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let (status,result) = request(&state,&base,reqwest::Method::PUT,"/lingxi/v1/preferences/models",json!({"models":{"knowledge":{"provider":"fixture","id":"knowledge-live"},"summarize":{"provider":"fixture","id":"summary-live"}}})).await;
    assert_eq!(status, 200, "{result}");
    let auxiliary = lingxi_adapters::models::auxiliary::AuxiliaryExecutor::new_with_network(
        state.model_gateway().unwrap().clone(),
        state.credential_service().unwrap().clone(),
        Default::default(),
        state.network_plane().unwrap().clone(),
    )
    .unwrap();
    let context = lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("settings-auxiliary-proof"),
        run_id: lingxi_protocol::RunId::new("settings-auxiliary-proof"),
        attempt: lingxi_protocol::AttemptId::new("settings-auxiliary-proof#1"),
        generation: 1,
    };
    for (slot, model) in [
        (
            lingxi_kernel::model_exchange::AuxiliarySlot::Knowledge,
            "knowledge-live",
        ),
        (
            lingxi_kernel::model_exchange::AuxiliarySlot::Summarize,
            "summary-live",
        ),
    ] {
        let outcome = auxiliary
            .complete(
                &context,
                slot,
                &lingxi_protocol::ModelCallId::new(format!("settings-{}", slot.config_key())),
                &lingxi_adapters::models::auxiliary::AuxiliaryRequest {
                    prompt: format!("证明{}独立线路", slot.config_key()),
                    images: vec![],
                    max_output_tokens: Some(16),
                    deadline_unix_ms: Some(lingxi_adapters::models::dispatch::unix_ms_now() + 5000),
                    system_prompt: None,
                    prior: vec![],
                    tools: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(outcome.text, "OK");
        assert_eq!(outcome.served_by.model, model);
        assert_eq!(outcome.transport_attempts, 1);
    }
    assert_eq!(count.load(Ordering::SeqCst), 3);
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let closed_addr = closed.local_addr().unwrap();
    drop(closed);
    let (status,result)=request(&state,&base,reqwest::Method::PUT,"/lingxi/v1/settings/config",json!({"network_proxy":{"mode":"manual","httpProxy":format!("http://{closed_addr}"),"noProxy":""}})).await;
    assert_eq!(status, 200, "{result}");
    let (_, result) = request(
        &state,
        &base,
        reqwest::Method::POST,
        "/lingxi/v1/models/health",
        json!({"provider":"fixture","model":"health"}),
    )
    .await;
    assert_eq!(
        result["ok"], false,
        "下一请求必须实际走已不可用的新代理: {result}"
    );
    assert_eq!(
        count.load(Ordering::SeqCst),
        3,
        "旧代理没有收到代理切换后的真实调用"
    );
    let saved: Value =
        serde_json::from_slice(&std::fs::read(home.join("user/preferences.json")).unwrap())
            .unwrap();
    assert_eq!(
        saved["network_proxy"]["httpProxy"],
        format!("http://{closed_addr}")
    );
    let _ = stop.send(());
    server.await.unwrap();
    proxy_task.abort();
    let _ = proxy_task.await;
    state.storage().close().await.unwrap();
    drop(state);
    std::fs::remove_dir_all(home).unwrap();
}
