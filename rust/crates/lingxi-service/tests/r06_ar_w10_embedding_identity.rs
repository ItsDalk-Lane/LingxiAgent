//! W10：仅替换外部供应商应答；网关、凭证、用量与知识库均为实际实现。
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

// 服务夹具使用独立合成 home；供应商替身在下文返回 A/B 同为 2 维的不同向量。
fn unique_dir(tag: &str) -> PathBuf {
    let dir = PathBuf::from(std::env::var("LINGXI_HOME").expect("isolated home")).join(format!(
        "lingxi-r06-ar-w10-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));

    std::fs::create_dir_all(&dir).expect("create test dir");
    dir
}

fn config_for(home: &Path) -> ServiceConfig {
    ServiceConfig {
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().expect("static addr"),
        data_home: home.to_path_buf(),
        home_source: HomeSource::Env,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    }
}

/// 带/不带 embedding 模型面（指向 loopback 替身）的 bootstrap；`model_id` 决定替身维度。
async fn boot(_tag: &str, home: &Path, embedding: Option<(&str, SocketAddr)>) -> ServiceState {
    let layout = prepare_layout(home).expect("prepare layout");
    let deps = match embedding {
        Some((model_id, stub_addr)) => {
            let config_root = home.to_path_buf();
            let config_path = config_root.join("service.json");
            let endpoint = format!("http://{stub_addr}/v1");
            let config_json = format!(
                r#"{{"home": {}, "providers": {{"embed": {{"protocol": "openai-embeddings", "endpoint": {}, "auth": {{"kind": "apiKey", "apiKey": "R06T07_STUB_KEY"}}}}}}, "models": {{"embedding": {{"provider": "embed", "model": {}}}}}}}"#,
                serde_json::to_string(&home.to_string_lossy()).expect("home json"),
                serde_json::to_string(&endpoint).expect("endpoint json"),
                serde_json::to_string(model_id).expect("model json"),
            );
            std::fs::write(&config_path, &config_json).expect("write config");
            let _file =
                lingxi_service::config::read_service_config(&config_path).expect("service config");
            let (source, plane) = lingxi_service::config::resolve_model_plane(
                Some(&config_path),
                &layout.runtime_dir,
            )
            .expect("plane resolves")
            .expect("plane present");
            let credential_service = Arc::new(
                lingxi_service::credentials::CredentialService::bootstrap(
                    &plane,
                    &layout.runtime_dir,
                    Arc::new(lingxi_service::inject::SystemClock),
                )
                .expect("credential service"),
            );
            ServiceDeps {
                model_gateway: Some(Arc::new(
                    lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
                )),
                model_plane_source: Some(source),
                credential_service: Some(credential_service),
                ..ServiceDeps::default()
            }
        }
        None => ServiceDeps::default(),
    };
    ServiceState::bootstrap_with_deps(config_for(home), &layout, deps)
        .await
        .expect("bootstrap")
}

struct ApiClient {
    addr: SocketAddr,
    token: String,
}

struct TestServer {
    api: ApiClient,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

async fn serve(state: ServiceState) -> TestServer {
    let token = state.auth().local_token();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let handle = tokio::spawn(async move {
        run(
            state,
            async {
                let _ = stop_rx.await;
            },
            |addr| {
                let _ = ready_tx.send(addr);
            },
            None,
        )
        .await
    });
    let addr = match ready_rx.await {
        Ok(addr) => addr,
        Err(err) => panic!("readiness: {err}; service result: {:?}", handle.await),
    };
    TestServer {
        api: ApiClient { addr, token },
        stop: stop_tx,
        handle,
    }
}

impl std::ops::Deref for TestServer {
    type Target = ApiClient;
    fn deref(&self) -> &Self::Target {
        &self.api
    }
}

impl ApiClient {
    async fn request(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let (status, _, body) = self.request_raw(method, path, body).await;
        if std::env::var("R06T07_DEBUG").is_ok() {
            eprintln!(
                "DEBUG {method} {path} -> {status}: {}",
                String::from_utf8_lossy(&body)
            );
        }
        let parsed = if body.is_empty() {
            Value::Null
        } else {
            match serde_json::from_slice(&body) {
                Ok(value) => value,
                Err(_) => {
                    eprintln!(
                        "DEBUG non-JSON body for {method} {path}: {}",
                        String::from_utf8_lossy(&body)
                    );
                    Value::Null
                }
            }
        };
        (status, parsed)
    }

    async fn request_raw(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Vec<(String, String)>, Vec<u8>) {
        let payload = body.map(|b| b.to_string()).unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            self.token,
            payload.len()
        );
        let mut stream = TcpStream::connect(self.addr).await.expect("connect");
        stream.write_all(request.as_bytes()).await.expect("write");
        let mut buf = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(20), stream.read_to_end(&mut buf)).await;
        let header_end = buf
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("http header end");
        let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);
        let headers: Vec<(String, String)> = head
            .split("\r\n")
            .skip(1)
            .filter_map(|line| {
                line.split_once(':')
                    .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
            })
            .collect();
        (status, headers, buf[header_end + 4..].to_vec())
    }
}

impl TestServer {
    async fn stop(self) {
        let _ = self.stop.send(());
        let _ = tokio::time::timeout(Duration::from_secs(10), self.handle).await;
    }
}

async fn wait_until_job(
    server: &ApiClient,
    notebook_id: &str,
    source_id: &str,
    expect_status: &str,
) -> Value {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let (status, jobs) = server
            .request(
                "GET",
                &format!(
                    "/lingxi/v1/knowledge/ingestion?notebookId={notebook_id}&sourceId={source_id}"
                ),
                None,
            )
            .await;
        assert_eq!(status, 200, "{jobs}");
        let hit = jobs["jobs"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .find(|j| j["status"] == expect_status)
            .cloned();
        if let Some(job) = hit {
            return job;
        }
        if std::time::Instant::now() > deadline {
            panic!("job never reached {expect_status}: {jobs}");
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

struct Stub {
    addr: SocketAddr,
    requests: Arc<std::sync::Mutex<Vec<Value>>>,
    entered: Arc<tokio::sync::Semaphore>,
    release: Arc<tokio::sync::Semaphore>,
    task: tokio::task::JoinHandle<()>,
}
impl Stub {
    async fn start(gate_at: Option<usize>, fail_at: Option<usize>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let entered = Arc::new(tokio::sync::Semaphore::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let (seen, notify, resume) = (requests.clone(), entered.clone(), release.clone());
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut raw = Vec::new();
                let (start, length) = loop {
                    let mut buf = [0_u8; 65536];
                    let n = socket.read(&mut buf).await.unwrap();
                    assert!(n > 0);
                    raw.extend_from_slice(&buf[..n]);
                    if let Some(start) = raw.windows(4).position(|x| x == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&raw[..start]);
                        let len: usize = headers
                            .lines()
                            .find_map(|line| {
                                line.split_once(':')
                                    .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                                    .map(|(_, v)| v.trim().parse().unwrap())
                            })
                            .unwrap();
                        break (start + 4, len);
                    }
                };
                while raw.len() < start + length {
                    let mut buf = [0_u8; 65536];
                    let n = socket.read(&mut buf).await.unwrap();
                    assert!(n > 0);
                    raw.extend_from_slice(&buf[..n]);
                }
                let request: Value = serde_json::from_slice(&raw[start..start + length]).unwrap();
                let index = {
                    let mut seen = seen.lock().unwrap();
                    seen.push(json!({"endpoint":addr.to_string(), "body":request}));
                    seen.len()
                };
                if gate_at == Some(index) {
                    notify.add_permits(1);
                    resume.acquire().await.unwrap().forget();
                }
                let (status, body) = if fail_at == Some(index) {
                    (
                        400,
                        json!({"error":{"message":"W10 external injected failure"}}),
                    )
                } else {
                    let model = request["model"].as_str().unwrap();
                    let vector = match model {
                        "model-b" => vec![0.0, 1.0],
                        "model-c" => vec![0.0, 0.0, 1.0],
                        _ => vec![1.0, 0.0],
                    };
                    let inputs = request["input"].as_array().unwrap();
                    (
                        200,
                        json!({"data": inputs.iter().enumerate().map(|(i,_)| json!({"index":i,"embedding":vector})).collect::<Vec<_>>(),"usage":{"prompt_tokens":inputs.len()*5,"total_tokens":inputs.len()*5}}),
                    )
                };
                let body = body.to_string();
                let response=format!("HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        Self {
            addr,
            requests,
            entered,
            release,
            task,
        }
    }
    async fn gated(&self) {
        tokio::time::timeout(Duration::from_secs(10), self.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }
    fn release(&self) {
        self.release.add_permits(1);
    }
    fn capture(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }
    fn stop(&self) {
        self.task.abort();
    }
}

fn record(tag: &str, value: Value) {
    let root = PathBuf::from(std::env::var("W10_EVIDENCE").expect("evidence root"));
    std::fs::write(
        root.join(format!("{tag}.json")),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
    eprintln!("W10 {tag}: 原始请求与数据库收据已写入独立证据");
}
async fn reload(server: &ApiClient, home: &Path, model: &str, addr: SocketAddr) {
    let path = home.join("service.json");
    let mut config: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    config["models"]["embedding"]["model"] = json!(model);
    config["providers"]["embed"]["endpoint"] = json!(format!("http://{addr}/v1"));
    std::fs::write(path, serde_json::to_vec(&config).unwrap()).unwrap();
    let (status, receipt) = server
        .request("POST", "/lingxi/v1/models/reload", Some(json!({})))
        .await;
    assert_eq!(status, 200, "{receipt}");
}
async fn import(server: &ApiClient, large: bool) -> (String, String, String) {
    let (status, created) = server
        .request(
            "POST",
            "/lingxi/v1/knowledge/notebooks",
            Some(json!({"name":"W10资料"})),
        )
        .await;
    assert_eq!(status, 201, "{created}");
    let notebook = created["notebook"]["id"].as_str().unwrap().to_string();
    let text = if large {
        (0..160)
            .map(|i| {
                format!(
                    "# 独立章节{i}\n\n来源隔离{i} {}\n\n",
                    "资料验证".repeat(260)
                )
            })
            .collect::<String>()
    } else {
        "# 资料\n\n来源隔离与模型身份必须一致。".to_string()
    };
    let (status, source) = server
        .request(
            "POST",
            &format!("/lingxi/v1/knowledge/notebooks/{notebook}/sources"),
            Some(json!({"kind":"pasted_text","displayName":"W10合成资料","text":text})),
        )
        .await;
    assert_eq!(status, 201, "{source}");
    (
        notebook,
        source["source"]["id"].as_str().unwrap().to_string(),
        source["parseArtifact"]["id"].as_str().unwrap().to_string(),
    )
}
fn vector_snapshot(home: &Path) -> Value {
    let path = home.join("knowledge/knowledge_vector.db");
    let conn =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let mut statement=conn.prepare("SELECT v.id,v.model_key,v.status,v.dimensions,hex(c.vector) FROM vector_index_variants v LEFT JOIN chunk_vectors c ON c.vector_index_variant_id=v.id ORDER BY v.id,c.chunk_id").unwrap();
    let rows=statement.query_map([],|row|Ok(json!({"variant":row.get::<_,String>(0)?,"key":row.get::<_,String>(1)?,"status":row.get::<_,String>(2)?,"dimensions":row.get::<_,i64>(3)?,"vector":row.get::<_,Option<String>>(4)?}))).unwrap().map(Result::unwrap).collect::<Vec<_>>();
    json!(rows)
}
fn usage_snapshot(home: &Path) -> Value {
    let conn = rusqlite::Connection::open_with_flags(
        home.join("lingxi-service/data/runs.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let mut statement=conn.prepare("SELECT provider,model,outcome,protocol,usage_state,input_tokens,output_tokens,transport_attempts FROM model_call_usage WHERE purpose='embedding' ORDER BY started_at_unix_ms,model_call_id").unwrap();
    let rows=statement.query_map([],|row|Ok(json!({"provider":row.get::<_,String>(0)?,"model":row.get::<_,String>(1)?,"outcome":row.get::<_,String>(2)?,"protocol":row.get::<_,String>(3)?,"usageState":row.get::<_,String>(4)?,"inputTokens":row.get::<_,Option<i64>>(5)?,"outputTokens":row.get::<_,Option<i64>>(6)?,"transportAttempts":row.get::<_,Option<i64>>(7)?}))).unwrap().map(Result::unwrap).collect::<Vec<_>>();
    json!(rows)
}
#[tokio::test]
async fn positive_production_ingestion_four_way_identity() {
    let home = unique_dir("positive");
    let stub = Stub::start(None, None).await;
    let state = boot("positive", &home, Some(("model-a", stub.addr))).await;
    let server = serve(state).await;
    let (n, s, _) = import(&server, false).await;
    let job = wait_until_job(&server, &n, &s, "done").await;
    let rows = vector_snapshot(&home);
    let usage = usage_snapshot(&home);
    record(
        "positive",
        json!({"home":home,"ready":server.addr.to_string(),"requests":stub.capture(),"job":job,"vectors":rows,"usage":usage}),
    );
    assert_eq!(job["embeddingStats"]["model"]["modelId"], "model-a");
    assert!(rows
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["vector"] == "0000803F00000000"));
    assert!(usage
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["model"] == "model-a"));
    assert_eq!(usage.as_array().unwrap().len(), stub.capture().len());
    for (row, wire) in usage.as_array().unwrap().iter().zip(stub.capture()) {
        assert_eq!(row["model"], wire["body"]["model"]);
        assert_eq!(
            row["inputTokens"],
            wire["body"]["input"].as_array().unwrap().len() * 5
        );
        assert_eq!(row["transportAttempts"], 1);
        assert_eq!(row["outcome"], "succeeded");
    }
    server.stop().await;
    stub.stop();
}
#[tokio::test]
async fn c02_same_dimension_reload_never_poisons_first_variant() {
    let home = unique_dir("c02");
    let stub = Stub::start(Some(1), None).await;
    let state = boot("c02", &home, Some(("model-a", stub.addr))).await;
    let server = serve(state).await;
    let (n, s, _) = import(&server, true).await;
    stub.gated().await;
    reload(&server, &home, "model-b", stub.addr).await;
    stub.release();
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let job = loop {
        let (_, jobs) = server
            .request(
                "GET",
                &format!("/lingxi/v1/knowledge/ingestion?notebookId={n}&sourceId={s}"),
                None,
            )
            .await;
        if let Some(j) = jobs["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["status"] == "failed" || j["status"] == "done")
        {
            break j.clone();
        }
        assert!(std::time::Instant::now() < deadline, "{jobs}");
        tokio::time::sleep(Duration::from_millis(30)).await;
    };
    let rows = vector_snapshot(&home);
    record(
        "c02",
        json!({"home":home,"ready":server.addr.to_string(),"requests":stub.capture(),"job":job,"vectors":rows,"usage":usage_snapshot(&home)}),
    );
    assert_eq!(
        job["status"], "failed",
        "changed space must refuse old build"
    );
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .all(|r| r["vector"] == "0000803F00000000"),
        "B vectors entered A variant: {rows}"
    );
    server.stop().await;
    stub.stop();
}

async fn hybrid(
    state: &ServiceState,
    notebook: &str,
    tag: &str,
) -> lingxi_service::knowledge::SearchResult {
    let knowledge = state.knowledge();
    let scope = knowledge
        .create_turn_scope(tag, tag, &[notebook.to_string()])
        .unwrap();
    knowledge
        .search_in_scope(&lingxi_service::knowledge::SearchRequest {
            scope_id: scope.id,
            session_id: tag.into(),
            query: "来源隔离".into(),
            channel: lingxi_service::knowledge::SearchChannel::Hybrid,
            notebook_ids: vec![],
            source_ids: vec![],
            section_keys: vec![],
            limit: 12,
        })
        .await
        .unwrap()
}
fn search_record(result: &lingxi_service::knowledge::SearchResult) -> Value {
    json!({"mode":result.mode,"searchedVectorVariants":result.searched_vector_variants,"degraded":result.degraded.iter().map(|d| json!({"reason":d.reason,"detail":d.detail})).collect::<Vec<_>>()})
}

#[tokio::test]
async fn c01_resolved_route_is_immutable_across_pre_send_reload() {
    use lingxi_service::operations::{LedgerOperationUsageSink, OperationService};
    use lingxi_service::quotas::{QuotaLimits, QuotaManager, QuotaResource};
    let home = unique_dir("c01");
    let stub = Stub::start(None, None).await;
    let state = boot("c01", &home, Some(("model-a", stub.addr))).await;
    let gateway = state.model_gateway().unwrap().clone();
    let quotas = Arc::new(QuotaManager::new(QuotaLimits::default()));
    // 占满真实配额；等待人数证明路由及凭证已经解析、请求尚未发送。
    let mut holds = Vec::new();
    for i in 0..8 {
        holds.push(
            quotas
                .acquire(
                    QuotaResource::Model,
                    &format!("hold{i}"),
                    &format!("hold{i}"),
                )
                .await
                .unwrap(),
        );
    }
    let operations = Arc::new(
        OperationService::new(
            gateway.clone(),
            state.credential_service().unwrap().clone(),
            quotas.clone(),
            vec![format!("http://{}/v1", stub.addr)],
            home.join("products-c01"),
        )
        .unwrap()
        .with_usage_sink(Arc::new(LedgerOperationUsageSink::new(
            state.storage().clone(),
            Arc::new(lingxi_service::inject::SystemClock),
        ))),
    );
    state.knowledge().set_embedder(Some(Arc::new(
        lingxi_service::knowledge::GatewayKnowledgeEmbedder::new(gateway, operations),
    )));
    use lingxi_service::knowledge::KnowledgeEmbedder as _;
    let configured_a = lingxi_service::knowledge::GatewayKnowledgeEmbedder::new(
        state.model_gateway().unwrap().clone(),
        state.operations().unwrap().clone(),
    )
    .identity()
    .unwrap()
    .unwrap();
    let expected_a = lingxi_service::knowledge::EmbeddingModelIdentity::with_parameters(
        &configured_a.provider,
        &configured_a.model_id,
        &configured_a.protocol,
        2,
        configured_a.parameter_fingerprint.clone(),
    );
    let server = serve(state.clone()).await;
    let (n, s, a) = import(&server, false).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while quotas.waiting(QuotaResource::Model) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(stub.capture().is_empty());
    reload(&server, &home, "model-b", stub.addr).await;
    drop(holds);
    let job = wait_until_job(&server, &n, &s, "done").await;
    let requests = stub.capture();
    let rows = vector_snapshot(&home);
    let usage = usage_snapshot(&home);
    record(
        "c01",
        json!({"home":home,"ready":server.addr.to_string(),"barrier":"actual quota waiting after resolve and credentials before send","requests":requests,"job":job,"vectors":rows,"usage":usage,"variants":state.knowledge().vector_variant_report(&a).unwrap().iter().map(|v|json!({"id":v.variant_id,"key":v.model_key,"status":v.status})).collect::<Vec<_>>()}),
    );
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .all(|r| r["key"] == expected_a.key),
        "variant must use actual A identity: {rows}"
    );
    assert_eq!(
        requests.len(),
        1,
        "single planned chunk has exactly one physical call"
    );
    assert_eq!(usage.as_array().unwrap().len(), requests.len());
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(requests[0]["body"]["model"], "model-a");
    assert_eq!(usage[0]["model"], "model-a");
    assert!(rows
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["vector"] == "0000803F00000000"));
    server.stop().await;
    stub.stop();
}

#[tokio::test]
async fn c03_query_uses_served_identity_during_reload_and_isolates_dimensions() {
    let home = unique_dir("c03");
    let stub = Stub::start(Some(2), None).await;
    let state = boot("c03", &home, Some(("model-a", stub.addr))).await;
    let server = serve(state.clone()).await;
    let (n, s, _) = import(&server, false).await;
    let job = wait_until_job(&server, &n, &s, "done").await;
    // 单块只发一次，第二个真实请求是查询；精确保持来源和顺序。
    assert_eq!(stub.capture().len(), 1);
    let query_state = state.clone();
    let query_notebook = n.clone();
    let query =
        tokio::spawn(async move { hybrid(&query_state, &query_notebook, "c03-inflight").await });
    stub.gated().await;
    reload(&server, &home, "model-b", stub.addr).await;
    stub.release();
    let in_flight = query.await.unwrap();
    assert_eq!(in_flight.mode, "hybrid");
    assert_eq!(in_flight.searched_vector_variants.len(), 1);
    let b = hybrid(&state, &n, "c03-b").await;
    assert_eq!(b.mode, "fts");
    assert!(b.searched_vector_variants.is_empty());
    assert!(b
        .degraded
        .iter()
        .any(|d| d.reason == "KNOWLEDGE_VECTOR_NOT_READY"));
    reload(&server, &home, "model-c", stub.addr).await;
    let c = hybrid(&state, &n, "c03-c").await;
    assert_eq!(c.mode, "fts");
    assert!(c.searched_vector_variants.is_empty());
    let wire = stub.capture();
    assert_eq!(
        wire.len(),
        4,
        "one ingestion, inflight A query, B query, C query"
    );
    assert_eq!(
        wire.iter()
            .map(|r| r["body"]["model"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["model-a", "model-a", "model-b", "model-c"]
    );
    assert_eq!(usage_snapshot(&home).as_array().unwrap().len(), wire.len());
    record(
        "c03",
        json!({"home":home,"ready":server.addr.to_string(),"requests":stub.capture(),"job":job,"inflight":search_record(&in_flight),"sameDimensionB":search_record(&b),"differentDimensionC":search_record(&c),"vectors":vector_snapshot(&home),"usage":usage_snapshot(&home)}),
    );
    server.stop().await;
    stub.stop();
}

#[tokio::test]
async fn c04_failed_build_restarts_under_changed_identity_without_reusing_old_space() {
    let home = unique_dir("c04");
    let stub = Stub::start(None, Some(2)).await;
    let state = boot("c04-a", &home, Some(("model-a", stub.addr))).await;
    let server = serve(state).await;
    let (n, s, a) = import(&server, true).await;
    let failed = wait_until_job(&server, &n, &s, "failed").await;
    let before = vector_snapshot(&home);
    record(
        "c04-before",
        json!({"failedJob":failed,"vectors":before,"requests":stub.capture()}),
    );
    assert!(before.as_array().unwrap().len() >= 64);
    server.stop().await;
    // 同一个实际home重启，保留付费产物/原文；模型换为同维度B。
    let state = boot("c04-b", &home, Some(("model-b", stub.addr))).await;
    let server = serve(state.clone()).await;
    let (status, receipt) = server
        .request(
            "POST",
            &format!("/lingxi/v1/knowledge/notebooks/{n}/sources/{s}/reingest"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 200, "{receipt}");
    let done = wait_until_job(&server, &n, &s, "done").await;
    let variants = state.knowledge().vector_variant_report(&a).unwrap();
    assert_eq!(variants.len(), 2);
    assert!(variants.iter().any(|v| v.status == "failed"));
    assert!(variants.iter().any(|v| v.status == "ready"));
    let after = vector_snapshot(&home);
    for old in before.as_array().unwrap() {
        assert!(
            after.as_array().unwrap().contains(old),
            "old paid vector changed"
        );
    }
    let ready_key = done["embeddingStats"]["model"]["key"].as_str().unwrap();
    assert!(after
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["key"] == ready_key)
        .all(|r| r["vector"] == "000000000000803F"));
    record(
        "c04",
        json!({"home":home,"ready":server.addr.to_string(),"requests":stub.capture(),"failedJob":failed,"retryReceipt":receipt,"doneJob":done,"before":before,"after":after,"usage":usage_snapshot(&home)}),
    );
    server.stop().await;
    stub.stop();
}

#[tokio::test]
async fn same_model_endpoint_change_is_incompatible_but_unchanged_reload_is_compatible() {
    let home = unique_dir("parameters");
    let stub = Stub::start(None, None).await;
    let state = boot("parameters", &home, Some(("model-a", stub.addr))).await;
    let server = serve(state.clone()).await;
    let (n, s, _) = import(&server, false).await;
    wait_until_job(&server, &n, &s, "done").await;
    let a = hybrid(&state, &n, "parameters-a").await;
    assert_eq!(a.mode, "hybrid");
    reload(&server, &home, "model-a", stub.addr).await;
    let unchanged = hybrid(&state, &n, "parameters-unchanged").await;
    assert_eq!(unchanged.mode, "hybrid");
    assert_eq!(
        a.searched_vector_variants,
        unchanged.searched_vector_variants
    );
    let new_endpoint = Stub::start(None, None).await;
    reload(&server, &home, "model-a", new_endpoint.addr).await;
    let changed = hybrid(&state, &n, "parameters-changed").await;
    assert_eq!(changed.mode, "fts");
    assert!(changed.searched_vector_variants.is_empty());
    record(
        "parameters",
        json!({"requestsA":stub.capture(),"requestsNewEndpoint":new_endpoint.capture(),"original":search_record(&a),"unchanged":search_record(&unchanged),"changed":search_record(&changed),"vectors":vector_snapshot(&home),"usage":usage_snapshot(&home)}),
    );
    server.stop().await;
    stub.stop();
    new_endpoint.stop();
}

struct BinaryService {
    child: tokio::process::Child,
    api: ApiClient,
    ready: String,
    drain: tokio::task::JoinHandle<()>,
}
impl BinaryService {
    async fn start(home: &Path, addr: SocketAddr, model: &str) -> Self {
        use tokio::io::AsyncBufReadExt as _;
        let cfg = home.with_extension("binary-config.json");
        std::fs::write(&cfg,serde_json::to_vec(&json!({"home":home,"providers":{"embed":{"protocol":"openai-embeddings","endpoint":format!("http://{addr}/v1"),"auth":{"kind":"apiKey","apiKey":"W10_SYNTHETIC_STUB"}}},"models":{"embedding":{"provider":"embed","model":model}}})).unwrap()).unwrap();
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_lingxi-service"))
            .args(["--config", cfg.to_str().unwrap(), "--bind", "127.0.0.1:0"])
            .env("LINGXI_HOME", home)
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .unwrap();
        let mut stdout = tokio::io::BufReader::new(child.stdout.take().unwrap());
        let ready = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let mut line = String::new();
                let n = stdout.read_line(&mut line).await.unwrap();
                assert!(n > 0, "binary exited before READY");
                if line.starts_with("LINGXI_SERVICE_READY ") {
                    break line.trim().to_string();
                }
            }
        })
        .await
        .unwrap();
        let canonical = std::fs::canonicalize(home).unwrap();
        assert!(
            ready.contains(&format!("home={}", canonical.display())),
            "{ready}"
        );
        assert!(ready.contains("source=env"), "{ready}");
        let service_addr = ready
            .split_whitespace()
            .find_map(|x| x.strip_prefix("addr="))
            .unwrap()
            .parse()
            .unwrap();
        let token_file: Value = serde_json::from_slice(
            &std::fs::read(home.join("lingxi-service/local-token.json")).unwrap(),
        )
        .unwrap();
        let drain = tokio::spawn(async move {
            let mut bytes = Vec::new();
            stdout.read_to_end(&mut bytes).await.unwrap();
        });
        Self {
            child,
            api: ApiClient {
                addr: service_addr,
                token: token_file["token"].as_str().unwrap().to_string(),
            },
            ready,
            drain,
        }
    }
    async fn kill(mut self) -> std::process::ExitStatus {
        self.child.start_kill().unwrap();
        let status = self.child.wait().await.unwrap();
        let _ = self.drain.await;
        status
    }
}

#[tokio::test]
async fn crash_running_job_recovers_distinct_identity_and_preserves_paid_vectors() {
    let home = unique_dir("crash-running");
    let stub = Stub::start(Some(2), None).await;
    let service = BinaryService::start(&home, stub.addr, "model-a").await;
    let (n, s, _) = import(&service.api, true).await;
    stub.gated().await;
    let before = vector_snapshot(&home);
    assert!(before.as_array().unwrap().len() >= 64);
    let ready_a = service.ready.clone();
    let exit_a = service.kill().await;
    // 外部连接只作为应答替身，旧进程已经退出，释放屏障不再产生旧库写入。
    stub.release();
    let service = BinaryService::start(&home, stub.addr, "model-b").await;
    let (status, retry) = service
        .api
        .request(
            "POST",
            &format!("/lingxi/v1/knowledge/notebooks/{n}/sources/{s}/reingest"),
            Some(json!({})),
        )
        .await;
    let done = wait_until_job(&service.api, &n, &s, "done").await;
    let after = vector_snapshot(&home);
    let (_, jobs) = service
        .api
        .request(
            "GET",
            &format!("/lingxi/v1/knowledge/ingestion?notebookId={n}&sourceId={s}"),
            None,
        )
        .await;
    record(
        "crash-running",
        json!({"home":home,"readyA":ready_a,"exitA":exit_a.to_string(),"readyB":service.ready,"retryStatus":status,"retry":retry,"jobs":jobs,"before":before,"after":after,"requests":stub.capture(),"binaryPath":env!("CARGO_BIN_EXE_lingxi-service"),"binarySha256":lingxi_kernel::knowledge::sha256_hex(&std::fs::read(env!("CARGO_BIN_EXE_lingxi-service")).unwrap()),"done":done,"state":"W09 recovered; F10 independent acceptance still required"}),
    );
    assert_eq!(
        status, 409,
        "active or completed job still rejects redundant manual retry"
    );
    let prior = before.as_array().unwrap();
    let current = after.as_array().unwrap();
    let keys = prior
        .iter()
        .map(|r| r["key"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    let unchanged = current
        .iter()
        .filter(|r| keys.contains(r["key"].as_str().unwrap()))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        prior, &unchanged,
        "all old paid IDs/bytes retained, no extra A rows"
    );
    let ready_key = done["embeddingStats"]["model"]["key"].as_str().unwrap();
    assert_eq!(done["embeddingStats"]["model"]["modelId"], "model-b");
    let new = current
        .iter()
        .filter(|r| r["key"] == ready_key)
        .collect::<Vec<_>>();
    let total = done["progressTotal"].as_u64().unwrap() as usize;
    assert_eq!(new.len(), total);
    assert_eq!(current.len(), prior.len() + total);
    assert!(new.iter().all(|r| r["vector"] == "000000000000803F"));
    let wire = stub.capture();
    assert_eq!(
        wire.len(),
        2 + total.div_ceil(64),
        "two A attempts including unknown plus all new B batches"
    );
    assert!(wire[..2].iter().all(|r| r["body"]["model"] == "model-a"));
    assert!(wire[2..].iter().all(|r| r["body"]["model"] == "model-b"));
    assert_eq!(
        wire[2..]
            .iter()
            .map(|r| r["body"]["input"].as_array().unwrap().len())
            .sum::<usize>(),
        total
    );
    service.kill().await;
    stub.stop();
}

#[tokio::test]
async fn configured_unknown_dimensions_and_parameter_compatibility_are_explicit() {
    use lingxi_adapters::models::operations::embedding::{
        embedding_space_fingerprint, EmbeddingInputType, EmbeddingRequest,
    };
    use lingxi_kernel::model_exchange::{
        ModelGatewayPort as _, ModelOperation, ModelRouteRequest, ProtocolFamily,
    };
    use lingxi_service::knowledge::KnowledgeEmbedder as _;
    let home = unique_dir("identity-contract");
    let stub = Stub::start(None, None).await;
    let state = boot("identity-contract", &home, Some(("model-a", stub.addr))).await;
    let embedder = lingxi_service::knowledge::GatewayKnowledgeEmbedder::new(
        state.model_gateway().unwrap().clone(),
        state.operations().unwrap().clone(),
    );
    let configured = embedder.identity().unwrap().unwrap();
    assert_eq!(configured.dimensions, 0);
    assert_eq!(configured.provider, "embed");
    assert_eq!(configured.model_id, "model-a");
    assert!(!configured.parameter_fingerprint.is_empty());
    assert!(stub.capture().is_empty());
    let route = state
        .model_gateway()
        .unwrap()
        .resolve_route(&ModelRouteRequest::for_operation(ModelOperation::Embedding))
        .unwrap();
    let mut request = EmbeddingRequest {
        inputs: vec!["合成请求".into()],
        dimensions: None,
        context_window: None,
        input_type: EmbeddingInputType::Document,
    };
    let original = embedding_space_fingerprint(&route, &request);
    request.input_type = EmbeddingInputType::Query;
    assert_eq!(original, embedding_space_fingerprint(&route, &request));
    request.dimensions = Some(2);
    assert_ne!(original, embedding_space_fingerprint(&route, &request));
    let mut ollama = route.clone();
    ollama.protocol = ProtocolFamily::OllamaEmbed;
    let default_context = embedding_space_fingerprint(&ollama, &request);
    request.context_window = Some(32768);
    assert_ne!(
        default_context,
        embedding_space_fingerprint(&ollama, &request)
    );
    let mut minimax = route.clone();
    minimax.protocol = ProtocolFamily::MinimaxEmbeddings;
    let no_group = embedding_space_fingerprint(&minimax, &request);
    minimax.group_id = Some("other-group".into());
    assert_ne!(no_group, embedding_space_fingerprint(&minimax, &request));
    // 五协议只验证真实构建器与身份算法，不把未发出的计划写成供应商运行证明。
    let mut protocol_checks = Vec::new();
    for protocol in [
        "openai-embeddings",
        "ollama-embed",
        "gemini-embed",
        "voyage-embeddings",
        "minimax-embeddings",
    ] {
        let mut paired_route = route.clone();
        paired_route.protocol = ProtocolFamily::parse(protocol).unwrap();
        paired_route.group_id = Some("group-a".into());
        let mut paired_request = EmbeddingRequest {
            inputs: vec!["协议合成资料".into()],
            dimensions: None,
            context_window: None,
            input_type: EmbeddingInputType::Document,
        };
        let doc_fingerprint = embedding_space_fingerprint(&paired_route, &paired_request);
        let auth = lingxi_adapters::models::credentials::ApplicableAuth::Bearer(
            "W10_SYNTHETIC_STUB".into(),
        );
        let document_plan = lingxi_adapters::models::operations::embedding::build_embedding(
            &paired_route,
            &auth,
            &paired_request,
        )
        .unwrap();
        paired_request.input_type = EmbeddingInputType::Query;
        let query_plan = lingxi_adapters::models::operations::embedding::build_embedding(
            &paired_route,
            &auth,
            &paired_request,
        )
        .unwrap();
        assert_eq!(
            doc_fingerprint,
            embedding_space_fingerprint(&paired_route, &paired_request)
        );
        let mut unchanged = paired_route.clone();
        unchanged.config_generation += 1;
        paired_request.inputs = vec!["另一条长短不同的合成查询".into()];
        assert_eq!(
            doc_fingerprint,
            embedding_space_fingerprint(&unchanged, &paired_request)
        );
        let mut changed = paired_route.clone();
        changed.model = "different-model".into();
        assert_ne!(
            doc_fingerprint,
            embedding_space_fingerprint(&changed, &paired_request)
        );
        changed = paired_route.clone();
        changed.provider = "different-provider".into();
        assert_ne!(
            doc_fingerprint,
            embedding_space_fingerprint(&changed, &paired_request)
        );
        changed = paired_route.clone();
        changed.endpoint = "http://127.0.0.1:9/other-endpoint".into();
        assert_ne!(
            doc_fingerprint,
            embedding_space_fingerprint(&changed, &paired_request)
        );
        protocol_checks.push(json!({"protocol":protocol,"evidenceLevel":"ADAPTER_BUILD_AND_FINGERPRINT_ONLY_NO_DISPATCH","documentUrl":document_plan.url,"documentBody":serde_json::from_slice::<Value>(&document_plan.body).unwrap(),"queryUrl":query_plan.url,"queryBody":serde_json::from_slice::<Value>(&query_plan.body).unwrap(),"fingerprint":doc_fingerprint,"compatibility":"paired role, text, generation unchanged; model/provider/endpoint changed refused"}));
    }
    record(
        "configured-identity",
        json!({"provider":configured.provider,"model":configured.model_id,"dimensions":"UNKNOWN_NOT_SENT","fingerprint":configured.parameter_fingerprint,"actualRequests":stub.capture(),"protocolChecks":protocol_checks}),
    );
    let server = serve(state).await;
    server.stop().await;
    stub.stop();
}

#[tokio::test]
async fn legacy_paid_identity_is_preserved_and_requires_explicit_rebuild() {
    let home = unique_dir("legacy");
    let stub = Stub::start(None, None).await;
    let state = boot("legacy", &home, Some(("model-a", stub.addr))).await;
    let server = serve(state.clone()).await;
    let (n, s, a) = import(&server, false).await;
    wait_until_job(&server, &n, &s, "done").await;
    let variant = state
        .knowledge()
        .vector_variant_report(&a)
        .unwrap()
        .remove(0);
    let legacy = lingxi_service::knowledge::EmbeddingModelIdentity::new(
        "embed",
        "model-a",
        "openai-embeddings",
        2,
    );
    let legacy_variant = lingxi_kernel::knowledge::vector_index_variant_id(
        &variant.chunk_index_variant_id,
        &legacy.key,
    );
    // 构造旧版已支付产物夹具：只有元数据形态切回旧key，原向量字节完全保留。
    let mut conn = rusqlite::Connection::open(home.join("knowledge/knowledge_vector.db")).unwrap();
    let tx = conn.transaction().unwrap();
    tx.execute_batch("PRAGMA defer_foreign_keys=ON").unwrap();
    tx.execute(
        "UPDATE chunk_vectors SET vector_index_variant_id=?1 WHERE vector_index_variant_id=?2",
        rusqlite::params![legacy_variant, variant.variant_id],
    )
    .unwrap();
    tx.execute(
        "UPDATE vector_index_variants SET id=?1,model_key=?2 WHERE id=?3",
        rusqlite::params![legacy_variant, legacy.key, variant.variant_id],
    )
    .unwrap();
    tx.commit().unwrap();
    drop(conn);
    let before = vector_snapshot(&home);
    let refused = hybrid(&state, &n, "legacy-read").await;
    assert_eq!(refused.mode, "fts");
    assert!(refused.searched_vector_variants.is_empty());
    assert_eq!(
        before,
        vector_snapshot(&home),
        "query must not overwrite unverified legacy identity"
    );
    state.knowledge().rebuild_notebook(&n).unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let variants = state.knowledge().vector_variant_report(&a).unwrap();
            if variants.len() == 2 && variants.iter().all(|v| v.status == "ready") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let rebuilt = hybrid(&state, &n, "legacy-rebuilt").await;
    assert_eq!(rebuilt.mode, "hybrid");
    let after = vector_snapshot(&home);
    for old in before.as_array().unwrap() {
        assert!(after.as_array().unwrap().contains(old));
    }
    record(
        "legacy",
        json!({"before":before,"refused":search_record(&refused),"after":after,"rebuilt":search_record(&rebuilt),"requests":stub.capture(),"usage":usage_snapshot(&home)}),
    );
    server.stop().await;
    stub.stop();
}
