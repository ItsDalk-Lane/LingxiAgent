//! W09：生产程序、认证HTTP与真实数据库；仅外部应答使用本机替身。
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

fn unique_dir(tag: &str) -> PathBuf {
    let dir = PathBuf::from(std::env::var("LINGXI_HOME").expect("isolated home")).join(format!(
        "lingxi-r06-ar-w09-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));

    std::fs::create_dir_all(&dir).expect("create test dir");
    dir
}

struct ApiClient {
    addr: SocketAddr,
    token: String,
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
        tokio::time::timeout(Duration::from_secs(20), stream.read_to_end(&mut buf))
            .await
            .expect("HTTP response read deadline")
            .expect("HTTP response read");
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
                        json!({"error":{"message":"W09 external injected failure"}}),
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
                let mut body = body;
                if let Some(rows) = body["data"].as_array_mut() {
                    match request["model"].as_str().unwrap() {
                        "invalid-short" => {
                            rows.pop();
                        }
                        "invalid-duplicate" => {
                            if rows.len() > 1 {
                                rows[1]["index"] = json!(0);
                            }
                        }
                        "invalid-dimension" => {
                            rows.last_mut().unwrap()["embedding"] = json!([1.0, 2.0, 3.0]);
                        }
                        "invalid-finite" => {
                            rows[0]["embedding"] = json!([1e300, 0.0]);
                        }
                        "invalid-index" => {
                            rows[0]["index"] = json!(99999);
                        }
                        "invalid-empty" => {
                            rows[0]["embedding"] = json!([]);
                        }
                        _ => {}
                    }
                }
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
    let root = PathBuf::from(std::env::var("W09_EVIDENCE").expect("evidence root"));
    std::fs::write(
        root.join(format!("{tag}.json")),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
    eprintln!("W09 {tag}: 原始请求与数据库收据已写入独立证据");
}

struct BinaryService {
    child: tokio::process::Child,
    api: ApiClient,
    ready: String,
    drain: tokio::task::JoinHandle<()>,
}
impl BinaryService {
    async fn start(home: &Path, addr: SocketAddr, model: &str) -> Self {
        Self::start_protocol(home, addr, model, "openai-embeddings").await
    }
    async fn start_protocol(home: &Path, addr: SocketAddr, model: &str, protocol: &str) -> Self {
        Self::start_auth(home, addr, model, protocol, "W09_SYNTHETIC_STUB").await
    }
    async fn start_auth(
        home: &Path,
        addr: SocketAddr,
        model: &str,
        protocol: &str,
        key: &str,
    ) -> Self {
        use tokio::io::AsyncBufReadExt as _;
        let cfg = home.with_extension("binary-config.json");
        std::fs::write(&cfg,serde_json::to_vec(&json!({"home":home,"providers":{"embed":{"protocol":protocol,"endpoint":format!("http://{addr}/v1"),"auth":{"kind":"apiKey","apiKey":key}}},"models":{"embedding":{"provider":"embed","model":model}}})).unwrap()).unwrap();
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
    async fn stop_sigterm(mut self) -> std::process::ExitStatus {
        let pid = self.child.id().expect("live owned child");
        let signal = tokio::process::Command::new("/bin/kill")
            .args(["-TERM", &pid.to_string()])
            .status()
            .await
            .unwrap();
        assert!(signal.success());
        let status = tokio::time::timeout(Duration::from_secs(10), self.child.wait())
            .await
            .expect("bounded graceful stop")
            .unwrap();
        self.drain.await.expect("stdout task reaped");
        status
    }
    async fn kill(mut self) -> std::process::ExitStatus {
        self.child.start_kill().unwrap();
        let status = self.child.wait().await.unwrap();
        self.drain.await.expect("shipping stdout task reaped");
        status
    }
}

async fn import_n(server: &ApiClient, n: usize) -> (String, String, String) {
    let (status, created) = server
        .request(
            "POST",
            "/lingxi/v1/knowledge/notebooks",
            Some(json!({"name":"W09合成资料"})),
        )
        .await;
    assert_eq!(status, 201, "{created}");
    let notebook = created["notebook"]["id"].as_str().unwrap().to_string();
    let text = (0..n)
        .map(|i| format!("Chapter {i:04} W09唯一章节\n\n唯一计划块{i:04}的独立合成资料。\n\n"))
        .collect::<String>();
    let (status, source) = server
        .request(
            "POST",
            &format!("/lingxi/v1/knowledge/notebooks/{notebook}/sources"),
            Some(json!({"kind":"pasted_text","displayName":"W09合成来源","text":text})),
        )
        .await;
    assert_eq!(status, 201, "{source}");
    (
        notebook,
        source["source"]["id"].as_str().unwrap().into(),
        source["parseArtifact"]["id"].as_str().unwrap().into(),
    )
}
fn read_db(home: &Path, file: &str, sql: &str) -> Vec<Value> {
    let conn = rusqlite::Connection::open_with_flags(
        home.join(file),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let mut stmt = conn.prepare(sql).unwrap();
    let cols = stmt
        .column_names()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    stmt.query_map([], |r| {
        let mut v = serde_json::Map::new();
        for (i, col) in cols.iter().enumerate() {
            let value = match r.get_ref(i)? {
                rusqlite::types::ValueRef::Null => Value::Null,
                rusqlite::types::ValueRef::Integer(n) => json!(n),
                rusqlite::types::ValueRef::Text(s) => json!(String::from_utf8_lossy(s)),
                _ => panic!("unexpected evidence type"),
            };
            v.insert(col.clone(), value);
        }
        Ok(Value::Object(v))
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}
fn plan(home: &Path) -> Vec<Value> {
    read_db(
        home,
        "knowledge/knowledge_index.db",
        "SELECT chunk_id,text,ordinal FROM knowledge_chunks ORDER BY ordinal",
    )
}
fn vectors(home: &Path) -> Vec<Value> {
    read_db(home,"knowledge/knowledge_vector.db","SELECT vector_index_variant_id,chunk_id,ordinal,hex(vector) AS bytes FROM chunk_vectors ORDER BY vector_index_variant_id,ordinal")
}
fn ledger(home: &Path) -> Vec<Value> {
    read_db(home,"lingxi-service/data/runs.db","SELECT model_call_id,provider,model,outcome,usage_state,input_tokens,transport_attempts FROM model_call_usage WHERE purpose='embedding' ORDER BY started_at_unix_ms,model_call_id")
}
#[tokio::test]
async fn c01_exact_batch_boundaries() {
    let mut failures = Vec::new();
    for n in [1usize, 64, 65, 128, 129] {
        let home = unique_dir(&format!("exact-{n}"));
        let stub = Stub::start(None, None).await;
        let service = BinaryService::start(&home, stub.addr, "model-a").await;
        let (nb, source, _) = import_n(&service.api, n).await;
        let job = wait_until_job(&service.api, &nb, &source, "done").await;
        let planned = plan(&home);
        let rows = vectors(&home);
        let usage = ledger(&home);
        let requests = stub.capture();
        record(
            &format!("c01-{n}"),
            json!({"ready":service.ready,"plan":planned,"requests":requests,"job":job,"vectors":rows,"ledger":usage}),
        );
        assert_eq!(
            planned.len(),
            n,
            "fixture must create precise distinguishable plan"
        );
        let sent = requests
            .iter()
            .flat_map(|r| r["body"]["input"].as_array().unwrap().iter().cloned())
            .collect::<Vec<_>>();
        let expected = planned
            .iter()
            .map(|r| r["text"].clone())
            .collect::<Vec<_>>();
        assert_eq!(usage.len(), n.div_ceil(64));
        assert_eq!(
            usage
                .iter()
                .map(|row| row["input_tokens"].as_u64().unwrap())
                .sum::<u64>(),
            (n * 5) as u64
        );
        assert!(usage
            .iter()
            .zip(&requests)
            .all(|(row, request)| row["transport_attempts"] == 1
                && row["input_tokens"].as_u64()
                    == Some((request["body"]["input"].as_array().unwrap().len() * 5) as u64)
                && row["usage_state"] == "partial"
                && row["outcome"] == "succeeded"));
        assert_eq!(job["embeddingStats"]["requestCount"], n.div_ceil(64));
        assert_eq!(
            job["embeddingStats"]["transportAttemptCount"],
            n.div_ceil(64)
        );
        assert_eq!(
            job["embeddingStats"]["confirmedResponseRequestCount"],
            n.div_ceil(64)
        );
        assert_eq!(job["embeddingStats"]["knownNotSentAttemptCount"], 0);
        assert_eq!(job["embeddingStats"]["pendingRequestCount"], 0);
        if requests.len() != n.div_ceil(64)
            || sent != expected
            || rows.len() != n
            || job["embeddingStats"]["chunksNewlyEmbedded"] != n
            || job["progressDone"] != n
        {
            failures.push(json!({"n":n,"requests":requests.len(),"sent":sent.len(),"job":job}));
        }
        service.kill().await;
        stub.stop();
    }
    assert!(
        failures.is_empty(),
        "exact wire/plan/count regression: {failures:?}"
    );
}
#[tokio::test]
async fn c02_failed_checkpoint_resumes_only_gaps() {
    for fault in [2usize, 3] {
        let home = unique_dir(&format!("failed-{fault}"));
        let stub = Stub::start(None, Some(fault)).await;
        let service = BinaryService::start(&home, stub.addr, "model-a").await;
        let (nb, source, _) = import_n(&service.api, 129).await;
        let failed = wait_until_job(&service.api, &nb, &source, "failed").await;
        let before = vectors(&home);
        let prior_requests = stub.capture();
        service.kill().await;
        let service = BinaryService::start(&home, stub.addr, "model-a").await;
        let (status, retry) = service
            .api
            .request(
                "POST",
                &format!("/lingxi/v1/knowledge/notebooks/{nb}/sources/{source}/reingest"),
                Some(json!({})),
            )
            .await;
        assert_eq!(status, 200, "{retry}");
        let done = wait_until_job(&service.api, &nb, &source, "done").await;
        let requests = stub.capture();
        let after = vectors(&home);
        let planned = plan(&home);
        record(
            &format!("c02-failed-{fault}"),
            json!({"before":before,"after":after,"failed":failed,"done":done,"requests":requests,"ledger":ledger(&home),"ready":service.ready}),
        );
        service.kill().await;
        stub.stop();
        assert!(
            before.iter().all(|r| after.contains(r)),
            "paid vector bytes must survive unchanged"
        );
        let resumed = requests[prior_requests.len()..]
            .iter()
            .flat_map(|r| r["body"]["input"].as_array().unwrap().iter().cloned())
            .collect::<Vec<_>>();
        let missing = planned
            .iter()
            .filter(|r| !before.iter().any(|b| b["chunk_id"] == r["chunk_id"]))
            .map(|r| r["text"].clone())
            .collect::<Vec<_>>();
        assert_eq!(
            resumed, missing,
            "only confirmed compatible gaps may be resent"
        );
        assert_eq!(
            done["embeddingStats"]["chunksResumedFromCheckpoint"],
            before.len()
        );
    }
}
#[tokio::test]
async fn c02_sigkill_running_recovers_only_gaps() {
    for gate in [1usize, 2, 3] {
        let home = unique_dir(&format!("sigkill-{gate}"));
        let stub = Stub::start(Some(gate), None).await;
        let service = BinaryService::start(&home, stub.addr, "model-a").await;
        let (nb, source, _) = import_n(&service.api, 129).await;
        stub.gated().await;
        let before = vectors(&home);
        let exit = service.kill().await;
        stub.release();
        let service = BinaryService::start(&home, stub.addr, "model-a").await;
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        let mut observed = Value::Null;
        while std::time::Instant::now() < deadline {
            let (_, jobs) = service
                .api
                .request(
                    "GET",
                    &format!("/lingxi/v1/knowledge/ingestion?notebookId={nb}&sourceId={source}"),
                    None,
                )
                .await;
            observed = jobs["jobs"][0].clone();
            if observed["status"] == "done" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let requests = stub.capture();
        let after = vectors(&home);
        record(
            &format!("c02-sigkill-{gate}"),
            json!({"exit":exit.to_string(),"before":before,"after":after,"job":observed,"requests":requests,"ledger":ledger(&home),"ready":service.ready}),
        );
        service.kill().await;
        stub.stop();
        assert_eq!(
            observed["status"], "done",
            "startup must recover orphan running job with single writer ownership"
        );
        assert!(before.iter().all(|r| after.contains(r)));
        assert_eq!(
            requests.len(),
            gate + (129 - before.len()).div_ceil(64),
            "original physical attempts including unknown plus only missing batches"
        );
        assert_eq!(
            observed["embeddingStats"]["chunksResumedFromCheckpoint"],
            before.len()
        );
        assert_eq!(before.len(), (gate - 1) * 64);
        assert_eq!(observed["embeddingStats"]["previousRequestCount"], gate);
        assert_eq!(
            observed["embeddingStats"]["unknownRequestsFromPreviousAttempt"],
            1
        );
    }
}

async fn latest_job(server: &ApiClient, nb: &str, source: &str, want: &str) -> Value {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let (_, jobs) = server
            .request(
                "GET",
                &format!("/lingxi/v1/knowledge/ingestion?notebookId={nb}&sourceId={source}"),
                None,
            )
            .await;
        if jobs["jobs"][0]["status"] == want {
            return jobs["jobs"][0].clone();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "latest job did not become {want}: {jobs}"
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
}
async fn trigger_rebuild(server: &ApiClient, nb: &str, label: &str) {
    let (status, body) = server
        .request(
            "PUT",
            &format!("/lingxi/v1/knowledge/notebooks/{nb}/settings"),
            Some(json!({"embeddingModelRef":{"provider":"embed","id":label}})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
}
#[tokio::test]
async fn c04_ready_reuse_and_changed_model_endpoint_profile_keep_paid_bytes() {
    let home = unique_dir("ready-reuse");
    let stub = Stub::start(None, None).await;
    let service = BinaryService::start(&home, stub.addr, "model-a").await;
    let (nb, source, _) = import_n(&service.api, 65).await;
    let first = wait_until_job(&service.api, &nb, &source, "done").await;
    let before = vectors(&home);
    let initial_count = stub.capture().len();
    trigger_rebuild(&service.api, &nb, "same-actual-route").await;
    let reused = latest_job(&service.api, &nb, &source, "done").await;
    assert_ne!(first["id"], reused["id"]);
    assert_eq!(
        stub.capture().len(),
        initial_count,
        "ready reuse must not pay for a dimension probe"
    );
    assert_eq!(reused["embeddingStats"]["requestCount"], 0);
    assert_eq!(reused["embeddingStats"]["chunksReusedFromReadyVariant"], 65);
    service.kill().await;
    let service = BinaryService::start(&home, stub.addr, "model-b").await;
    trigger_rebuild(&service.api, &nb, "changed-model").await;
    let changed = latest_job(&service.api, &nb, &source, "done").await;
    assert_eq!(changed["embeddingStats"]["chunksNewlyEmbedded"], 65);
    assert_eq!(stub.capture().len(), initial_count + 2);
    let after_model = vectors(&home);
    assert!(before.iter().all(|r| after_model.contains(r)));
    service.kill().await;
    let new_endpoint = Stub::start(None, None).await;
    let service = BinaryService::start(&home, new_endpoint.addr, "model-b").await;
    trigger_rebuild(&service.api, &nb, "changed-endpoint").await;
    let endpoint = latest_job(&service.api, &nb, &source, "done").await;
    assert_eq!(new_endpoint.capture().len(), 2);
    assert!(after_model.iter().all(|r| vectors(&home).contains(r)));
    let (status, receipt) = service
        .api
        .request(
            "PUT",
            &format!("/lingxi/v1/knowledge/notebooks/{nb}/settings"),
            Some(json!({"chunkTargetChars":1000})),
        )
        .await;
    assert_eq!(status, 200, "{receipt}");
    let profile = latest_job(&service.api, &nb, &source, "done").await;
    assert_eq!(profile["embeddingStats"]["chunksNewlyEmbedded"], 65);
    assert_eq!(new_endpoint.capture().len(), 4);
    let before_parameter = vectors(&home);
    service.kill().await;
    let service =
        BinaryService::start_protocol(&home, new_endpoint.addr, "model-b", "voyage-embeddings")
            .await;
    trigger_rebuild(&service.api, &nb, "changed-protocol-parameter").await;
    let parameter = latest_job(&service.api, &nb, &source, "done").await;
    assert_eq!(new_endpoint.capture().len(), 6);
    assert_eq!(
        parameter["embeddingStats"]["chunksResumedFromCheckpoint"],
        0
    );
    assert!(before_parameter.iter().all(|r| vectors(&home).contains(r)));
    let wire = new_endpoint.capture();
    assert!(wire[4..]
        .iter()
        .all(|r| r["body"]["input_type"] == "document"));
    record(
        "c04-ready",
        json!({"first":first,"reused":reused,"changedModel":changed,"changedEndpoint":endpoint,"changedChunkProfile":profile,"changedProtocolParameter":parameter,"oldPaid":before,"vectors":vectors(&home),"wireA":stub.capture(),"wireNewEndpoint":new_endpoint.capture(),"ledger":ledger(&home),"ready":service.ready}),
    );
    service.kill().await;
    stub.stop();
    new_endpoint.stop();
}
#[tokio::test]
async fn c03_progress_and_storage_failure_windows_never_ready() {
    for mode in [
        "progress",
        "write-abort",
        "complete-abort",
        "wrong-id",
        "missing-row",
    ] {
        let home = unique_dir(mode);
        let stub = Stub::start(Some(2), None).await;
        let service = BinaryService::start(&home, stub.addr, "model-a").await;
        let (nb, source, _) = import_n(&service.api, 65).await;
        stub.gated().await;
        let (_, mid) = service
            .api
            .request(
                "GET",
                &format!("/lingxi/v1/knowledge/ingestion?notebookId={nb}&sourceId={source}"),
                None,
            )
            .await;
        let paid = vectors(&home);
        let db = rusqlite::Connection::open(home.join("knowledge/knowledge_vector.db")).unwrap();
        match mode {
            "write-abort"=>db.execute_batch("CREATE TRIGGER w09_fail_write BEFORE INSERT ON chunk_vectors WHEN NEW.ordinal>=64 BEGIN SELECT RAISE(ABORT,'W09 synthetic write failure'); END;").unwrap(),
            "complete-abort"=>db.execute_batch("CREATE TRIGGER w09_fail_complete BEFORE UPDATE ON vector_index_variants WHEN NEW.status='ready' BEGIN SELECT RAISE(ABORT,'W09 synthetic complete failure'); END;").unwrap(),
            "wrong-id"=>{db.execute("UPDATE chunk_vectors SET chunk_id='W09-unplanned' WHERE ordinal=0",[]).unwrap();},
            "missing-row"=>{db.execute("DELETE FROM chunk_vectors WHERE ordinal=0",[]).unwrap();},
            _=>{},
        }
        drop(db);
        stub.release();
        let job = latest_job(
            &service.api,
            &nb,
            &source,
            if mode == "progress" { "done" } else { "failed" },
        )
        .await;
        let variants = read_db(
            &home,
            "knowledge/knowledge_vector.db",
            "SELECT id,status FROM vector_index_variants ORDER BY id",
        );
        let after = vectors(&home);
        let wire = stub.capture();
        let usage = ledger(&home);
        record(
            &format!("c03-{mode}"),
            json!({"mid":mid,"final":job,"before":paid,"after":after,"variants":variants,"wire":wire,"usage":usage}),
        );
        service.kill().await;
        stub.stop();
        assert_eq!(mid["jobs"][0]["progressDone"], 64);
        assert_eq!(mid["jobs"][0]["progressTotal"], 65);
        assert!(job["progressDone"].as_u64().unwrap() <= 65);
        assert_eq!(wire.len(), 2);
        assert_eq!(usage.len(), 2);
        if mode == "progress" {
            assert_eq!(job["progressDone"], 65);
        } else {
            assert!(variants.iter().all(|v| v["status"] != "ready"));
        }
        if mode == "write-abort" {
            assert_eq!(
                after, paid,
                "aborted batch transaction must preserve prior bytes"
            );
        }
    }
}
#[tokio::test]
async fn c02_worker_ownership_and_source_delete_win() {
    let home = unique_dir("worker-delete");
    let stub = Stub::start(Some(2), None).await;
    let service = BinaryService::start(&home, stub.addr, "model-a").await;
    let (nb, source, _) = import_n(&service.api, 129).await;
    stub.gated().await;
    let other = Arc::new(
        lingxi_service::knowledge::KnowledgeService::new(&home, "synthetic".into()).unwrap(),
    );
    let refused = other.start_worker().unwrap_err();
    assert_eq!(refused.code, "KNOWLEDGE_CONFLICT");
    assert_eq!(
        stub.capture().len(),
        2,
        "second live worker must not take over running job"
    );
    let (status, deleted) = service
        .api
        .request(
            "DELETE",
            &format!("/lingxi/v1/knowledge/sources/{source}"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{deleted}");
    stub.release();
    tokio::time::sleep(Duration::from_millis(150)).await;
    let rows = vectors(&home);
    let wire = stub.capture();
    record(
        "c02-worker-delete",
        json!({"refusal":refused.to_string(),"deleted":deleted,"remainingVectors":rows,"wire":wire,"ledger":ledger(&home)}),
    );
    service.kill().await;
    stub.stop();
    assert!(
        rows.is_empty(),
        "late response must not resurrect deleted source"
    );
    assert_eq!(wire.len(), 2, "no post-delete third request");
    let empty = Stub::start(None, None).await;
    let service = BinaryService::start(&home, empty.addr, "model-a").await;
    let (status, retry) = service
        .api
        .request(
            "POST",
            &format!("/lingxi/v1/knowledge/notebooks/{nb}/sources/{source}/reingest"),
            Some(json!({})),
        )
        .await;
    service.kill().await;
    empty.stop();
    assert!(status >= 400, "deleted source must not be queued: {retry}");
}
#[tokio::test]
async fn c03_invalid_external_vector_family_cannot_complete() {
    for mode in [
        "invalid-short",
        "invalid-duplicate",
        "invalid-dimension",
        "invalid-finite",
        "invalid-index",
        "invalid-empty",
    ] {
        let home = unique_dir(mode);
        let stub = Stub::start(None, None).await;
        let service = BinaryService::start(&home, stub.addr, mode).await;
        let (nb, source, _) = import_n(&service.api, 65).await;
        let job = latest_job(&service.api, &nb, &source, "failed").await;
        let variants = read_db(
            &home,
            "knowledge/knowledge_vector.db",
            "SELECT id,status FROM vector_index_variants",
        );
        let wire = stub.capture();
        let usage = ledger(&home);
        record(
            &format!("c03-{mode}"),
            json!({"job":job,"variants":variants,"wire":wire,"ledger":usage,"vectors":vectors(&home)}),
        );
        service.kill().await;
        stub.stop();
        assert_eq!(wire.len(), 1);
        assert_eq!(usage.len(), 1);
        assert!(variants.iter().all(|v| v["status"] != "ready"));
        assert_eq!(job["embeddingStats"]["requestCount"], 1);
        assert_eq!(job["embeddingStats"]["chunksNewlyEmbedded"], 0);
    }
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

impl TestServer {
    async fn stop(self) {
        self.stop.send(()).expect("owned service stop notification");
        let outcome = tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("owned service bounded exit")
            .expect("owned service task reaped")
            .expect("owned service stopped successfully");
        assert!(
            !outcome.drain_timed_out,
            "owned service exit was forced: {outcome:?}"
        );
    }
}

#[tokio::test]
async fn c02_presend_quota_wait_cancel_records_no_wire_request() {
    use lingxi_service::operations::{LedgerOperationUsageSink, OperationService};
    use lingxi_service::quotas::{QuotaLimits, QuotaManager, QuotaResource};
    let home = unique_dir("quota-cancel");
    let stub = Stub::start(None, None).await;
    let state = boot("quota-cancel", &home, Some(("model-a", stub.addr))).await;
    let gateway = state.model_gateway().unwrap().clone();
    let quotas = Arc::new(QuotaManager::new(QuotaLimits::default()));
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
            home.join("products"),
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
    let server = serve(state.clone()).await;
    let (nb, source, _) = import_n(&server, 65).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while quotas.waiting(QuotaResource::Model) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let (_, jobs) = server
        .request(
            "GET",
            &format!("/lingxi/v1/knowledge/ingestion?notebookId={nb}&sourceId={source}"),
            None,
        )
        .await;
    let stats = &jobs["jobs"][0]["embeddingStats"];
    assert_eq!(stats["callAttemptCount"], 1);
    assert_eq!(stats["requestCount"], 0);
    assert_eq!(stats["pendingCallIntentCount"], 1);
    assert_eq!(stats["pendingRequestCount"], 0);
    assert!(stub.capture().is_empty());
    assert!(ledger(&home).is_empty());
    let (status, deleted) = server
        .request(
            "DELETE",
            &format!("/lingxi/v1/knowledge/sources/{source}"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{deleted}");
    tokio::time::timeout(Duration::from_secs(5), async {
        while quotas.waiting(QuotaResource::Model) != 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    drop(holds);
    tokio::time::sleep(Duration::from_millis(100)).await;
    record(
        "c02-quota-cancel",
        json!({"before":jobs,"deleted":deleted,"wire":stub.capture(),"ledger":ledger(&home),"waiting":quotas.waiting(QuotaResource::Model)}),
    );
    assert!(stub.capture().is_empty());
    assert!(vectors(&home).is_empty());
    // 取消发生在网关等待；本例没有已发送未知费用，不虚构0-token行。
    assert!(ledger(&home).is_empty());
    server.stop().await;
    stub.stop();
}
#[tokio::test]
async fn c02_post_commit_statistics_and_job_completion_fault_resume_paid_checkpoint() {
    for mode in ["stats-after-write", "job-complete"] {
        let home = unique_dir(mode);
        let stub = Stub::start(Some(1), None).await;
        let service = BinaryService::start(&home, stub.addr, "model-a").await;
        let (nb, source, _) = import_n(&service.api, 65).await;
        stub.gated().await;
        let db = rusqlite::Connection::open(home.join("knowledge/knowledge.db")).unwrap();
        match mode {
            "stats-after-write"=>db.execute_batch("CREATE TRIGGER fault BEFORE UPDATE OF embedding_stats ON ingestion_jobs WHEN json_extract(NEW.embedding_stats,'$.actualWrittenChunkCount')>0 BEGIN SELECT RAISE(ABORT,'W09 statistics write interrupted after vector commit'); END;").unwrap(),
            _=>db.execute_batch("CREATE TRIGGER fault BEFORE UPDATE OF status ON ingestion_jobs WHEN NEW.status='done' BEGIN SELECT RAISE(ABORT,'W09 job completion failure'); END;").unwrap(),
        }
        stub.release();
        let failed = latest_job(&service.api, &nb, &source, "failed").await;
        let before = vectors(&home);
        let count = stub.capture().len();
        assert_eq!(
            before.len(),
            if mode == "stats-after-write" { 64 } else { 65 }
        );
        assert_eq!(count, if mode == "stats-after-write" { 1 } else { 2 });
        if mode == "stats-after-write" {
            assert!(failed["error"]
                .as_str()
                .unwrap()
                .contains("confirmed checkpoint count=64"));
        }
        db.execute_batch("DROP TRIGGER fault;").unwrap();
        drop(db);
        service.kill().await;
        let service = BinaryService::start(&home, stub.addr, "model-a").await;
        let (status, retry) = service
            .api
            .request(
                "POST",
                &format!("/lingxi/v1/knowledge/notebooks/{nb}/sources/{source}/reingest"),
                Some(json!({})),
            )
            .await;
        assert_eq!(status, 200, "{retry}");
        let done = latest_job(&service.api, &nb, &source, "done").await;
        let after = vectors(&home);
        let wire = stub.capture();
        assert!(before.iter().all(|r| after.contains(r)));
        assert_eq!(after.len(), 65);
        assert_eq!(wire.len() - count, (65 - before.len()).div_ceil(64));
        assert_eq!(
            done["embeddingStats"]["chunksResumedFromCheckpoint"],
            before.len()
        );
        record(
            &format!("c02-{mode}"),
            json!({"failed":failed,"done":done,"before":before,"after":after,"wire":wire,"ledger":ledger(&home)}),
        );
        service.kill().await;
        stub.stop();
    }
}
#[tokio::test]
async fn c04_content_refresh_creates_new_plan_and_preserves_previous_paid_bytes() {
    let home = unique_dir("content-refresh");
    let stub = Stub::start(None, None).await;
    let service = BinaryService::start(&home, stub.addr, "model-a").await;
    let (_, created) = service
        .api
        .request(
            "POST",
            "/lingxi/v1/knowledge/notebooks",
            Some(json!({"name":"合成文件库"})),
        )
        .await;
    let nb = created["notebook"]["id"].as_str().unwrap();
    let file = home.parent().unwrap().join("W09-generated-content.txt");
    std::fs::write(&file, "Chapter 1 合成章节\n\n第一版独立资料。\n").unwrap();
    let (status, imported) = service
        .api
        .request(
            "POST",
            &format!("/lingxi/v1/knowledge/notebooks/{nb}/sources"),
            Some(json!({"kind":"file","filePath":file,"displayName":"合成文件"})),
        )
        .await;
    assert_eq!(status, 201, "{imported}");
    let source = imported["source"]["id"].as_str().unwrap();
    let first = latest_job(&service.api, nb, source, "done").await;
    let before = vectors(&home);
    let (status, unchanged) = service
        .api
        .request(
            "POST",
            &format!("/lingxi/v1/knowledge/notebooks/{nb}/sources/{source}/refresh"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 200, "{unchanged}");
    assert_eq!(unchanged["changed"], false);
    assert_eq!(stub.capture().len(), 1);
    std::fs::write(&file, "Chapter 1 合成章节\n\n第二版不同的合成资料。\n").unwrap();
    let (status, refreshed) = service
        .api
        .request(
            "POST",
            &format!("/lingxi/v1/knowledge/notebooks/{nb}/sources/{source}/refresh"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 200, "{refreshed}");
    assert_eq!(refreshed["changed"], true);
    let new_source = refreshed["source"]["id"].as_str().unwrap();
    let done = latest_job(&service.api, nb, new_source, "done").await;
    let after = vectors(&home);
    assert!(before.iter().all(|r| after.contains(r)));
    assert_eq!(after.len(), 2);
    assert_eq!(stub.capture().len(), 2);
    assert_eq!(done["embeddingStats"]["chunksNewlyEmbedded"], 1);
    assert_ne!(first["artifactId"], done["artifactId"]);
    record(
        "c04-content-refresh",
        json!({"first":first,"unchanged":unchanged,"refreshed":refreshed,"done":done,"before":before,"after":after,"wire":stub.capture(),"ledger":ledger(&home)}),
    );
    service.kill().await;
    stub.stop();
}

#[tokio::test]
async fn c01_presend_request_construction_failure_is_not_a_physical_request() {
    let home = unique_dir("pre-send-build");
    let stub = Stub::start(None, None).await;
    let service = BinaryService::start_auth(
        &home,
        stub.addr,
        "model-a",
        "openai-embeddings",
        // 构造失败的触发器：CR/LF 自配置校验收紧（启动即拒）后改用 BEL——
        // 通过配置校验（非空、无 CR/LF），在发送期 HeaderValue::from_str 失败，
        // 保持「发送前构造失败」这一原语义不变。
        "W09_SYNTHETIC\u{7}INVALID_HEADER",
    )
    .await;
    let (nb, source, _) = import_n(&service.api, 1).await;
    let failed = latest_job(&service.api, &nb, &source, "failed").await;
    let wire = stub.capture();
    let usage = ledger(&home);
    record(
        "c01-presend-build",
        json!({"failed":failed,"wire":wire,"ledger":usage,"vectors":vectors(&home)}),
    );
    service.kill().await;
    stub.stop();
    assert!(wire.is_empty());
    assert!(vectors(&home).is_empty());
    assert_eq!(failed["embeddingStats"]["requestCount"], 0);
    assert_eq!(failed["embeddingStats"]["callAttemptCount"], 1);
    assert_eq!(failed["embeddingStats"]["failedCallCount"], 1);
    assert_eq!(failed["embeddingStats"]["failedRequestCount"], 0);
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0]["transport_attempts"], 0);
    assert_eq!(usage[0]["input_tokens"], Value::Null);
}

#[tokio::test]
async fn c02_last_service_owner_drop_stops_worker_and_releases_single_writer() {
    let home = unique_dir("owner-drop");
    let stub = Stub::start(None, None).await;
    let state = boot("owner-drop", &home, Some(("model-a", stub.addr))).await;
    let database = state.storage().clone();
    let old_weak = Arc::downgrade(state.knowledge());
    let last_owner = state.clone();
    drop(state);
    let successor = Arc::new(
        lingxi_service::knowledge::KnowledgeService::new(&home, "synthetic".into()).unwrap(),
    );
    assert_eq!(
        successor.start_worker().unwrap_err().code,
        "KNOWLEDGE_CONFLICT"
    );
    drop(last_owner);
    let probe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(home.join("knowledge/ingestion-worker.lock"))
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if probe.try_lock().is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    drop(probe);
    tokio::time::timeout(Duration::from_secs(5), async {
        while old_weak.upgrade().is_some() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    successor.start_worker().unwrap();
    successor.start_worker().unwrap();
    database.close().await.unwrap();
    let probe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(home.join("knowledge/ingestion-worker.lock"))
        .unwrap();
    assert!(
        probe.try_lock().is_err(),
        "expired weak callback cannot stop a successor worker"
    );
    drop(probe);
    successor.wait_worker_stopped().await.unwrap();
    let next = Arc::new(
        lingxi_service::knowledge::KnowledgeService::new(&home, "synthetic".into()).unwrap(),
    );
    next.start_worker().unwrap();
    next.wait_worker_stopped().await.unwrap();
    record(
        "c02-owner-drop",
        json!({"clone_kept_live_worker_protected":true,"last_owner_drop_released_os_lock":true,"duplicate_start_did_not_spawn":true,"expired_weak_callback_did_not_stop_successor":true,"both_successors_stopped_and_joined":true,"wire":stub.capture()}),
    );
    assert!(stub.capture().is_empty());
    stub.stop();
}
#[tokio::test]
async fn c02_bind_failure_joins_worker_before_returning_error() {
    let home = unique_dir("bind-failure");
    let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut cfg = config_for(&home);
    cfg.bind_addr = occupied.local_addr().unwrap();
    let layout = prepare_layout(&home).unwrap();
    let state = ServiceState::bootstrap_with_deps(cfg, &layout, ServiceDeps::default())
        .await
        .unwrap();
    let knowledge = state.knowledge().clone();
    let outcome = run(
        state,
        std::future::pending::<()>(),
        |_| panic!("failed bind must never publish READY"),
        None,
    )
    .await;
    assert!(
        matches!(&outcome, Err(ServiceError::Bind { .. })),
        "{outcome:?}"
    );
    let successor = Arc::new(
        lingxi_service::knowledge::KnowledgeService::new(&home, "synthetic".into()).unwrap(),
    );
    successor.start_worker().unwrap();
    successor.wait_worker_stopped().await.unwrap();
    knowledge.wait_worker_stopped().await.unwrap();
    record(
        "c02-bind-failure",
        json!({"return":format!("{outcome:?}"),"no_ready_published":true,"original_worker_joined_before_return":true,"new_owner_immediate_lock_acquisition":true}),
    );
}

#[tokio::test]
async fn c01_closed_loopback_port_is_an_attempt_but_proves_not_sent() {
    let home = unique_dir("connect-failure");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let service = BinaryService::start(&home, addr, "model-a").await;
    let (nb, source, _) = import_n(&service.api, 1).await;
    let failed = latest_job(&service.api, &nb, &source, "failed").await;
    let usage = ledger(&home);
    record(
        "c01-connect-failure",
        json!({"listenerClosedBeforeImport":true,"endpoint":addr.to_string(),"serverWire":0,"failed":failed,"ledger":usage,"vectors":vectors(&home),"deliveryClassification":"known_not_sent_connect_failure; usage_unknown_is_not_zero"}),
    );
    service.kill().await;
    let stats = &failed["embeddingStats"];
    assert_eq!(stats["callAttemptCount"], 1);
    assert_eq!(stats["transportAttemptCount"], 1);
    assert_eq!(stats["requestCount"], 0);
    assert_eq!(stats["knownNotSentAttemptCount"], 1);
    assert_eq!(stats["confirmedResponseRequestCount"], 0);
    assert_eq!(stats["pendingRequestCount"], 0);
    assert_eq!(stats["failedTransportAttemptCount"], 1);
    assert_eq!(stats["failedRequestCount"], 0);
    assert!(vectors(&home).is_empty());
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0]["transport_attempts"], 1);
    assert_eq!(usage[0]["input_tokens"], Value::Null);
    assert_eq!(usage[0]["usage_state"], "unknown");
    assert!(
        failed.to_string().contains("failed before a response"),
        "{failed}"
    );
    let stub = Stub::start(None, None).await;
    let service = BinaryService::start(&home, stub.addr, "model-a").await;
    let (status, retry) = service
        .api
        .request(
            "POST",
            &format!("/lingxi/v1/knowledge/notebooks/{nb}/sources/{source}/reingest"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 200, "{retry}");
    let done = wait_until_job(&service.api, &nb, &source, "done").await;
    let resumed_ledger = ledger(&home);
    record(
        "c01-connect-failure-restart",
        json!({"done":done,"wire":stub.capture(),"ledger":resumed_ledger,"knownNotSentNeverDeductedTwice":true}),
    );
    assert_eq!(done["embeddingStats"]["requestCount"], 1);
    assert_eq!(done["embeddingStats"]["previousRequestCount"], 0);
    assert_eq!(done["embeddingStats"]["previousTransportAttemptCount"], 1);
    assert_eq!(
        done["embeddingStats"]["previousKnownNotSentAttemptCount"],
        1
    );
    assert_eq!(
        done["embeddingStats"]["unknownRequestsFromPreviousAttempt"],
        0
    );
    assert_eq!(stub.capture().len(), 1);
    assert_eq!(resumed_ledger.len(), 2);
    assert!(resumed_ledger.iter().all(|r| r["transport_attempts"] == 1));
    service.stop_sigterm().await;
    stub.stop();
}

#[tokio::test]
async fn c02_graceful_stop_joins_active_worker_before_compatible_gap_recovery() {
    let home = unique_dir("sigterm-active");
    let stub = Stub::start(Some(2), None).await;
    let service = BinaryService::start(&home, stub.addr, "model-a").await;
    let (nb, source, _) = import_n(&service.api, 129).await;
    stub.entered.acquire().await.unwrap().forget();
    let before = vectors(&home);
    assert_eq!(before.len(), 64);
    let exit = service.stop_sigterm().await;
    assert!(exit.success(), "{exit}");
    let stopped = read_db(
        &home,
        "knowledge/knowledge.db",
        "SELECT id,status,attempt,embedding_stats FROM ingestion_jobs ORDER BY updated_at",
    );
    assert_eq!(stopped.len(), 1);
    assert_eq!(stopped[0]["status"], "running");
    let stopped_stats: Value =
        serde_json::from_str(stopped[0]["embedding_stats"].as_str().unwrap()).unwrap();
    assert_eq!(stopped_stats["pendingRequestCount"], 1);
    assert_eq!(stopped_stats["requestCount"], 2);
    assert_eq!(stopped_stats["confirmedResponseRequestCount"], 1);
    // 旧进程已经 wait/reap；迟到的响应不能提交，接管者只补实际缺口。
    stub.release.add_permits(1);
    let service = BinaryService::start(&home, stub.addr, "model-a").await;
    let done = wait_until_job(&service.api, &nb, &source, "done").await;
    let after = vectors(&home);
    let wire = stub.capture();
    record(
        "c02-sigterm-active",
        json!({"exit":exit.to_string(),"stopped":stopped,"stoppedStats":stopped_stats,"before":before,"after":after,"done":done,"wire":wire,"ledger":ledger(&home)}),
    );
    assert_eq!(wire.len(), 4);
    assert_eq!(after.len(), 129);
    assert!(before.iter().all(|r| after.contains(r)));
    assert_eq!(done["embeddingStats"]["chunksResumedFromCheckpoint"], 64);
    assert_eq!(done["embeddingStats"]["previousRequestCount"], 2);
    assert_eq!(
        done["embeddingStats"]["unknownRequestsFromPreviousAttempt"],
        1
    );
    service.stop_sigterm().await;
    stub.stop();
}

#[tokio::test]
async fn c02_storage_close_joins_its_active_worker_even_with_live_state_clones_and_spares_other_home(
) {
    let home_a = unique_dir("close-owned-a");
    let home_b = unique_dir("close-owned-b");
    let stub_a = Stub::start(Some(2), None).await;
    let stub_b = Stub::start(Some(2), None).await;
    let state_a = boot("close-owned-a", &home_a, Some(("model-a", stub_a.addr))).await;
    let state_b = boot("close-owned-b", &home_b, Some(("model-a", stub_b.addr))).await;
    let server_a = serve(state_a.clone()).await;
    let server_b = serve(state_b.clone()).await;
    let (nb_a, source_a, _) = import_n(&server_a, 129).await;
    let (nb_b, source_b, _) = import_n(&server_b, 65).await;
    stub_a.gated().await;
    stub_b.gated().await;
    let before = vectors(&home_a);
    assert_eq!(before.len(), 64);
    assert_eq!(vectors(&home_b).len(), 64);
    let (first, second) = tokio::join!(state_a.storage().close(), state_a.storage().close());
    first.unwrap();
    second.unwrap();
    state_a.storage().close().await.unwrap();
    assert!(state_a.storage().run_fact_summary().await.is_err());
    // 仍活的 ServiceState/HTTP clone 不改变关闭含义，锁只在 worker 真实退出后释放。
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(home_a.join("knowledge/ingestion-worker.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    drop(lock);
    let lock_b = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(home_b.join("knowledge/ingestion-worker.lock"))
        .unwrap();
    assert!(lock_b.try_lock().is_err());
    drop(lock_b);
    stub_a.release();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        vectors(&home_a),
        before,
        "old worker cannot commit late response after storage.close returned"
    );
    stub_b.release();
    let done_b = wait_until_job(&server_b, &nb_b, &source_b, "done").await;
    assert_eq!(done_b["embeddingStats"]["requestCount"], 2);
    assert_eq!(vectors(&home_b).len(), 65);
    let successor = boot(
        "close-owned-successor",
        &home_a,
        Some(("model-a", stub_a.addr)),
    )
    .await;
    let next = serve(successor).await;
    let done_a = wait_until_job(&next, &nb_a, &source_a, "done").await;
    let after = vectors(&home_a);
    assert_eq!(stub_a.capture().len(), 4);
    assert_eq!(after.len(), 129);
    assert!(before.iter().all(|r| after.contains(r)));
    assert_eq!(done_a["embeddingStats"]["chunksResumedFromCheckpoint"], 64);
    assert_eq!(
        done_a["embeddingStats"]["unknownRequestsFromPreviousAttempt"],
        1
    );
    record(
        "c02-storage-close-owned",
        json!({"dbCloseWasConcurrentAndRepeated":true,"oldStateClonesStillAlive":true,"oldWorkerJoinedAndOSLockFreeBeforeReturn":true,"before":before,"after":after,"doneA":done_a,"doneB":done_b,"wireA":stub_a.capture(),"wireB":stub_b.capture(),"otherHomeKeptLockedUntilItsOwnStop":true,"ledgerA":ledger(&home_a),"ledgerB":ledger(&home_b)}),
    );
    next.stop().await;
    server_a.stop().await;
    server_b.stop().await;
    state_b.storage().close().await.unwrap();
    stub_a.stop();
    stub_b.stop();
}
