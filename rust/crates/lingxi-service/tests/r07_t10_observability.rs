//! R07-T10 观测查询/筛选/详情/导出/旧库导入：真实 HTTP 面 + 真实 SQLite
//! 旧库文件 + 真实账本写入。无模型面（数据由导入与新行直写造出）。
use lingxi_kernel::ports::StoragePort;
use lingxi_kernel::usage::{CallOutcome, ModelCallUsageRecord, ModelCallUsage};
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

fn unique_dir(tag: &str) -> PathBuf {
    let dir = PathBuf::from(std::env::var("LINGXI_HOME").expect("isolated home")).join(format!(
        "lingxi-r07-t10-{tag}-{}-{}",
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

struct Api {
    addr: SocketAddr,
    token: String,
}

struct Server {
    api: Api,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
    state: ServiceState,
    home: PathBuf,
}

impl std::ops::Deref for Server {
    type Target = Api;
    fn deref(&self) -> &Self::Target {
        &self.api
    }
}

impl Api {
    async fn call(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Vec<u8>) {
        self.call_with_auth(method, path, body, Some(self.token.clone())).await
    }

    async fn call_with_auth(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        token: Option<String>,
    ) -> (u16, Vec<u8>) {
        let payload = body.map(|b| b.to_string()).unwrap_or_default();
        let auth = token
            .map(|t| format!("Authorization: Bearer {t}\r\n"))
            .unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
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
        (status, buf[header_end + 4..].to_vec())
    }

    async fn json(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let (status, raw) = self.call(method, path, body).await;
        let parsed = serde_json::from_slice(&raw)
            .unwrap_or_else(|_| panic!("{method} {path} -> {status} non-JSON: {}", String::from_utf8_lossy(&raw)));
        (status, parsed)
    }
}

async fn boot(tag: &str) -> Server {
    let home = unique_dir(tag);
    let layout = prepare_layout(&home).expect("layout");
    let state = ServiceState::bootstrap_with_deps(config_for(&home), &layout, ServiceDeps::default())
        .await
        .expect("bootstrap");
    let token = state.auth().local_token();
    let state_clone = state.clone();
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
    let addr = ready_rx.await.expect("readiness");
    Server {
        api: Api { addr, token },
        stop: stop_tx,
        handle,
        state: state_clone,
        home,
    }
}

impl Server {
    async fn stop(self) {
        let _ = self.stop.send(());
        let _ = tokio::time::timeout(Duration::from_secs(10), self.handle).await;
        self.state.storage().close().await.expect("close storage");
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

/// 旧 observability.sqlite（v7 形状子集）：多日期/provider/purpose，含一个
/// 三调用父子链 trace、一个单调用 trace、一个无 trace 孤儿行。
fn build_legacy_store(path: &Path) {
    let conn = rusqlite::Connection::open(path).expect("create legacy store");
    conn.execute_batch(
        "CREATE TABLE traces (trace_id TEXT PRIMARY KEY, origin TEXT, first_seen_at TEXT, last_seen_at TEXT, call_count INTEGER, created_at TEXT, updated_at TEXT);
         CREATE TABLE model_calls (call_id TEXT PRIMARY KEY, trace_id TEXT, parent_call_id TEXT, provider TEXT, model_id TEXT, api TEXT, subsystem TEXT, operation TEXT, surface TEXT, trigger_word TEXT, attribution_kind TEXT, session_id TEXT, session_path TEXT, conversation_id TEXT, conversation_type TEXT, agent_id TEXT, child_agent_id TEXT, child_session_id TEXT, child_session_path TEXT, task_id TEXT, call_purpose TEXT, started_at TEXT, semantic_completed_at TEXT, ended_at TEXT, terminal_status TEXT, error_name TEXT, error_code TEXT, input_shape TEXT, provenance_precision TEXT, provenance_section_count INTEGER, provenance_categories_json TEXT, provenance_opaque_count INTEGER, attribution_json TEXT, source_json TEXT, safe_details_json TEXT, persistence_completeness TEXT DEFAULT 'partial', interrupted_by_restart INTEGER DEFAULT 0, payload_availability TEXT, usage_correlation_state TEXT);
         CREATE TABLE model_call_usage (model_call_id TEXT PRIMARY KEY, usage_request_id TEXT, started_at TEXT, ended_at TEXT, duration_ms INTEGER, usage_status TEXT, input_total_tokens INTEGER, input_uncached_tokens INTEGER, output_total_tokens INTEGER, reasoning_tokens INTEGER, cache_read_tokens INTEGER, cache_write_tokens INTEGER, cache_miss_tokens INTEGER, cache_hit INTEGER, cache_created INTEGER, cache_hit_ratio REAL, total_tokens INTEGER, cost_total REAL, raw_usage_shape TEXT, created_at TEXT, updated_at TEXT);
         CREATE TABLE observability_meta (key TEXT PRIMARY KEY, value_json TEXT);
         INSERT INTO observability_meta VALUES ('schemaVersion', '7');",
    )
    .expect("legacy schema");
    let day1 = "2026-09-10T12:00:00.000Z";
    let day1b = "2026-09-10T12:01:00.000Z";
    let day1c = "2026-09-10T12:02:00.000Z";
    let day2 = "2026-10-01T08:00:00.000Z";
    let day2b = "2026-10-01T09:00:00.000Z";
    // (call_id, trace, parent, provider, model, api, subsystem, purpose, session, started, ended, status)
    let calls = [
        ("legacy-c1", Some("trace-multi"), None, "alpha", "alpha-model", "chat_completions", "chat", "chat", Some("sess-old-1"), day1, Some(day1), "ok"),
        ("legacy-c2", Some("trace-multi"), Some("legacy-c1"), "alpha", "alpha-model", "chat_completions", "chat", "chat", Some("sess-old-1"), day1b, Some(day1b), "ok"),
        ("legacy-c3", Some("trace-multi"), Some("legacy-c2"), "alpha", "alpha-model", "chat_completions", "chat", "chat", Some("sess-old-1"), day1c, Some(day1c), "ok"),
        ("legacy-c4", Some("trace-single"), None, "beta", "beta-model", "responses", "title", "title", Some("sess-old-2"), day2, Some(day2), "ok"),
        ("legacy-c5", None, None, "alpha", "alpha-model", "chat_completions", "chat", "chat", Some("sess-old-1"), day2b, Some(day2b), "error"),
    ];
    for (id, trace, parent, provider, model, api, subsystem, purpose, session, started, ended, status) in calls {
        conn.execute(
            "INSERT INTO model_calls (call_id, trace_id, parent_call_id, provider, model_id, api, subsystem, call_purpose, session_id, started_at, ended_at, terminal_status) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            rusqlite::params![id, trace, parent, provider, model, api, subsystem, purpose, session, started, ended, status],
        )
        .expect("legacy call");
    }
    // usage：c1/c2/c3 有（c1 100/50，c2 200/60，c3 300/70），c4/c5 无。
    for (id, input, output) in [("legacy-c1", 100i64, 50i64), ("legacy-c2", 200, 60), ("legacy-c3", 300, 70)] {
        conn.execute(
            "INSERT INTO model_call_usage (model_call_id, input_total_tokens, output_total_tokens) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, input, output],
        )
        .expect("legacy usage");
    }
}

fn new_kernel_record(id: &str, session: &str, purpose: &str, provider: &str, input: u64, output: u64) -> ModelCallUsageRecord {
    ModelCallUsageRecord {
        session_id: Some(session.to_string()),
        run_id: Some(format!("run-{id}")),
        attempt: Some("a1".to_string()),
        model_call_id: id.to_string(),
        purpose: purpose.to_string(),
        origin: "user".to_string(),
        parent_run_id: None,
        cause_ref: None,
        parent_tool_call_id: None,
        provider: provider.to_string(),
        model: "chat".to_string(),
        protocol: "openai-completions".to_string(),
        usage: Some(ModelCallUsage::reported(input, output)),
        invalid_detail: None,
        transport_attempts: Some(1),
        outcome: CallOutcome::Succeeded,
        started_at_unix_ms: None,
        settled_at_unix_ms: None,
        emitted_tool_calls: Vec::new(),
        cost_basis: None,
    }
}

#[tokio::test]
async fn r07_t10_query_page_aggregate_detail_export_import() {
    let server = boot("main").await;

    // 两条新内核行（recorded_at = now）。
    let now = server.state.clock().now_unix_ms();
    for (id, purpose, provider, input, output) in [
        ("new-n1", "chat", "remote", 10u64, 5u64),
        ("new-n2", "title", "remote", 20u64, 6u64),
    ] {
        server
            .state
            .storage()
            .record_model_call_usage(new_kernel_record(id, "sess-new", purpose, provider, input, output), now)
            .await
            .expect("record new row");
    }

    // 旧库导入。
    let legacy_path = server.home.join("legacy-observability.sqlite");
    build_legacy_store(&legacy_path);
    let (status, body) = server
        .json("POST", "/lingxi/v1/observability/import", Some(json!({ "path": legacy_path.to_string_lossy() })))
        .await;
    assert_eq!(status, 200, "import: {body}");
    assert_eq!(body["importedCalls"], 5);
    assert_eq!(body["skippedExisting"], 0);
    assert_eq!(body["traceRows"], 4);

    // 幂等重导。
    let (status, body) = server
        .json("POST", "/lingxi/v1/observability/import", Some(json!({ "path": legacy_path.to_string_lossy() })))
        .await;
    assert_eq!(status, 200, "re-import: {body}");
    assert_eq!(body["importedCalls"], 0);
    assert_eq!(body["skippedExisting"], 5);

    // A20：导入之后，旧会话上的「后续新调用」进同一本账。
    let mut n3 = new_kernel_record("new-n3", "sess-old-1", "chat", "gamma", 7, 3);
    n3.started_at_unix_ms = Some(now);
    server
        .state
        .storage()
        .record_model_call_usage(n3, now)
        .await
        .expect("record n3");

    // ── 查询与筛选 ──
    let (status, body) = server.json("POST", "/lingxi/v1/observability/query/calls", Some(json!({}))).await;
    assert_eq!(status, 200, "all: {body}");
    assert_eq!(body["calls"].as_array().unwrap().len(), 8);
    assert!(body["nextCursor"].is_null());

    // provider 单值 + 多值
    let (_, body) = server.json("POST", "/lingxi/v1/observability/query/calls", Some(json!({"filter": {"provider": "alpha"}}))).await;
    let ids: Vec<&str> = body["calls"].as_array().unwrap().iter().map(|c| c["callId"].as_str().unwrap()).collect();
    assert_eq!(ids, ["legacy-c1", "legacy-c2", "legacy-c3", "legacy-c5"]);
    let (_, body) = server.json("POST", "/lingxi/v1/observability/query/calls", Some(json!({"filter": {"provider": ["alpha", "beta"]}}))).await;
    assert_eq!(body["calls"].as_array().unwrap().len(), 5);

    // purpose 多值（category 别名）。排序按落库先后（rowid），新旧混合时按集合断言。
    let (_, body) = server.json("POST", "/lingxi/v1/observability/query/calls", Some(json!({"filter": {"category": ["title"]}}))).await;
    let mut ids: Vec<&str> = body["calls"].as_array().unwrap().iter().map(|c| c["callId"].as_str().unwrap()).collect();
    ids.sort_unstable();
    assert_eq!(ids, ["legacy-c4", "new-n2"]);

    // 日期窗口（since 含 / until 不含）
    let (_, body) = server.json("POST", "/lingxi/v1/observability/query/calls", Some(json!({"filter": {"since": "2026-10-01T00:00:00Z", "until": "2026-10-02T00:00:00Z"}}))).await;
    let ids: Vec<&str> = body["calls"].as_array().unwrap().iter().map(|c| c["callId"].as_str().unwrap()).collect();
    assert_eq!(ids, ["legacy-c4", "legacy-c5"]);

    // outcome 旧词汇映射
    let (_, body) = server.json("POST", "/lingxi/v1/observability/query/calls", Some(json!({"filter": {"terminalStatus": "error"}}))).await;
    assert_eq!(body["calls"].as_array().unwrap().len(), 1);
    assert_eq!(body["calls"][0]["callId"], "legacy-c5");
    assert_eq!(body["calls"][0]["terminalStatus"], "error");

    // 交叉筛选（日期 ∩ provider ∩ purpose）
    let (_, body) = server.json("POST", "/lingxi/v1/observability/query/calls", Some(json!({"filter": {"since": "2026-09-10T00:00:00Z", "until": "2026-09-11T00:00:00Z", "provider": "alpha", "purpose": "chat"}}))).await;
    assert_eq!(body["calls"].as_array().unwrap().len(), 3);

    // 会话筛选（含 native: 前缀兼容）
    let (_, body) = server.json("POST", "/lingxi/v1/observability/query/calls", Some(json!({"filter": {"sessionId": "native:sess-new"}}))).await;
    let ids: Vec<&str> = body["calls"].as_array().unwrap().iter().map(|c| c["callId"].as_str().unwrap()).collect();
    assert_eq!(ids, ["new-n1", "new-n2"]);

    // A20：旧会话上「既有关联 + 后续新调用」同面可查——旧 4 行与新 n3 同会话归拢。
    let (_, body) = server.json("POST", "/lingxi/v1/observability/query/calls", Some(json!({"filter": {"sessionId": "sess-old-1"}}))).await;
    let ids: Vec<&str> = body["calls"].as_array().unwrap().iter().map(|c| c["callId"].as_str().unwrap()).collect();
    assert_eq!(ids, ["legacy-c1", "legacy-c2", "legacy-c3", "legacy-c5", "new-n3"], "同会话新旧并存: {ids:?}");
    let (status, detail) = server.json("GET", "/lingxi/v1/observability/calls/new-n3", None).await;
    assert_eq!(status, 200, "后续新调用可追踪: {detail}");
    assert_eq!(detail["call"]["attribution"]["sessionId"], "sess-old-1");

    // ── 分页：翻页不重不漏；filter 变了 cursor 即 400 ──
    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut req = json!({"limit": 3});
        if let Some(c) = &cursor {
            req["cursor"] = json!(c);
        }
        let (status, body) = server.json("POST", "/lingxi/v1/observability/query/calls", Some(req)).await;
        assert_eq!(status, 200, "page: {body}");
        for call in body["calls"].as_array().unwrap() {
            seen.push(call["callId"].as_str().unwrap().to_string());
        }
        match body["nextCursor"].as_str() {
            Some(next) => cursor = Some(next.to_string()),
            None => break,
        }
    }
    let mut sorted = seen.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 8, "翻页有重/漏: {seen:?}");

    let (status, body) = server
        .json("POST", "/lingxi/v1/observability/query/calls", Some(json!({"filter": {"provider": "alpha"}, "limit": 3, "cursor": cursor.unwrap()})))
        .await;
    assert_eq!(status, 400, "stale cursor must fail: {body}");
    assert_eq!(body["error"], "invalid_query");

    // 未知 filter 字段响亮 400。
    let (status, _) = server
        .json("POST", "/lingxi/v1/observability/query/calls", Some(json!({"filter": {"nonsense": 1}})))
        .await;
    assert_eq!(status, 400);

    // ── 合计对账（A19 核心：合计 == 同筛选去重明细求和）──
    let (_, agg) = server.json("POST", "/lingxi/v1/observability/query/aggregate", Some(json!({}))).await;
    assert_eq!(agg["overall"]["callCount"], 8);
    assert_eq!(agg["overall"]["okCount"], 7);
    assert_eq!(agg["overall"]["errorCount"], 1);
    assert_eq!(agg["overall"]["inputTokens"], 100 + 200 + 300 + 10 + 20 + 7);
    assert_eq!(agg["overall"]["outputTokens"], 50 + 60 + 70 + 5 + 6 + 3);

    let (_, agg) = server.json("POST", "/lingxi/v1/observability/query/aggregate", Some(json!({"groupBy": ["date"]}))).await;
    let groups = agg["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 3, "{agg}");
    let by_date: std::collections::HashMap<&str, &Value> = groups
        .iter()
        .map(|g| (g["values"]["date"].as_str().unwrap(), &g["metrics"]))
        .collect();
    assert_eq!(by_date["2026-09-10"]["callCount"], 3);
    assert_eq!(by_date["2026-09-10"]["inputTokens"], 600);
    assert_eq!(by_date["2026-10-01"]["callCount"], 2);
    assert_eq!(by_date["2026-10-01"]["inputTokens"], Value::Null, "无 usage 的行保持缺失标记，不捏 0");
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    assert_eq!(by_date[today.as_str()]["callCount"], 3);
    assert_eq!(by_date[today.as_str()]["inputTokens"], 37);

    // 分组对账：组内明细之和 == 组合计（逐组用同筛选拉明细核对）。
    for (group_date, expect_calls) in [("2026-09-10", 3u64), ("2026-10-01", 2u64)] {
        let (_, detail) = server
            .json("POST", "/lingxi/v1/observability/query/calls", Some(json!({"filter": {"since": format!("{group_date}T00:00:00Z"), "until": format!("{group_date}T23:59:59.999Z")}})))
            .await;
        let detail_calls = detail["calls"].as_array().unwrap();
        assert_eq!(detail_calls.len() as u64, expect_calls);
        let detail_sum: u64 = detail_calls
            .iter()
            .filter_map(|c| c["usage"]["summary"]["inputTokens"].as_u64())
            .sum();
        assert_eq!(by_date[group_date]["inputTokens"].as_u64().unwrap_or(0), detail_sum, "组 {group_date} 合计≠明细");
    }

    // ── 详情：旧 trace 关联不丢（A20 核心）──
    let (status, detail) = server.json("GET", "/lingxi/v1/observability/calls/legacy-c2", None).await;
    assert_eq!(status, 200, "{detail}");
    assert_eq!(detail["call"]["traceId"], "trace-multi");
    assert_eq!(detail["call"]["parentCallId"], "legacy-c1");
    let siblings = detail["legacyTrace"]["calls"].as_array().unwrap();
    assert_eq!(siblings.len(), 3);
    // 新内核行：无 legacy trace，因果字段在账本自身。
    let (status, detail) = server.json("GET", "/lingxi/v1/observability/calls/new-n1", None).await;
    assert_eq!(status, 200, "{detail}");
    assert!(detail["legacyTrace"].is_null());
    assert_eq!(detail["call"]["callPurpose"], "chat");

    let (status, trace) = server.json("GET", "/lingxi/v1/observability/traces/trace-multi", None).await;
    assert_eq!(status, 200, "{trace}");
    assert_eq!(trace["calls"].as_array().unwrap().len(), 3);
    let edges = trace["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 2, "父子链不断裂: {edges:?}");
    assert!(edges.iter().any(|e| e["parentCallId"] == "legacy-c1" && e["childCallId"] == "legacy-c2"));
    assert!(edges.iter().any(|e| e["parentCallId"] == "legacy-c2" && e["childCallId"] == "legacy-c3"));

    let (status, _) = server.json("GET", "/lingxi/v1/observability/traces/trace-nope", None).await;
    assert_eq!(status, 404);
    let (status, _) = server.json("GET", "/lingxi/v1/observability/calls/nope", None).await;
    assert_eq!(status, 404);

    // ── 导出 ──
    let (status, raw) = server.call("POST", "/lingxi/v1/observability/export", Some(json!({}))).await;
    assert_eq!(status, 200);
    let text = String::from_utf8(raw).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 9, "manifest + 8 行: {text}");
    let manifest: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(manifest["type"], "manifest");
    assert_eq!(manifest["exportSchemaVersion"], 1);
    assert_eq!(manifest["totalCalls"], 8);
    let exported_ids: Vec<String> = lines[1..]
        .iter()
        .map(|l| serde_json::from_str::<Value>(l).unwrap()["call"]["callId"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(exported_ids.len(), 8);

    // A19：带交叉筛选的导出 == 同筛选查询明细（集合与计数双侧对账）。
    let filter = json!({"since": "2026-09-10T00:00:00Z", "until": "2026-09-11T00:00:00Z", "provider": "alpha", "purpose": "chat"});
    let (_, query) = server.json("POST", "/lingxi/v1/observability/query/calls", Some(json!({"filter": filter}))).await;
    let mut query_ids: Vec<&str> = query["calls"].as_array().unwrap().iter().map(|c| c["callId"].as_str().unwrap()).collect();
    query_ids.sort_unstable();
    let (status, raw) = server.call("POST", "/lingxi/v1/observability/export", Some(json!({"filter": filter}))).await;
    assert_eq!(status, 200);
    let text = String::from_utf8(raw).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let manifest: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(manifest["totalCalls"].as_u64().unwrap(), query_ids.len() as u64);
    let mut export_ids: Vec<String> = lines[1..]
        .iter()
        .map(|l| serde_json::from_str::<Value>(l).unwrap()["call"]["callId"].as_str().unwrap().to_string())
        .collect();
    export_ids.sort();
    assert_eq!(export_ids, query_ids, "导出与查询明细集合一致");

    // 导出上限：matchedCalls > maxCalls → 413 带计数。
    let (status, body) = server.json("POST", "/lingxi/v1/observability/export", Some(json!({"maxCalls": 3}))).await;
    assert_eq!(status, 413, "{body}");
    assert_eq!(body["matchedCalls"], 8);
    assert_eq!(body["maxCalls"], 3);

    // ── 鉴权：无凭证 401（LocalOnly）──
    let (status, _) = server
        .call_with_auth("POST", "/lingxi/v1/observability/query/calls", Some(json!({})), None)
        .await;
    assert_eq!(status, 401);

    server.stop().await;
}
