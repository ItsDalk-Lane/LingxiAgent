//! F13：真实认证提交铸造知识范围；旧范围和非法选择不能冒认当前授权。
use std::collections::BTreeMap;
use std::future::IntoFuture;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{extract::State, http::header, response::IntoResponse, routing::post, Json};
use lingxi_adapters::storage::knowledge::KnowledgeFactStore;
use lingxi_kernel::ports::{ReceiptOutcome, StoragePort};
use lingxi_protocol::{RunId, RunStatus};
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Default)]
struct Captures {
    requests: Vec<Value>,
    steps: BTreeMap<String, usize>,
    previous_scope: Option<String>,
}

fn content_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn wire_selection(body: &Value) -> Value {
    let system = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "system")
        .map(|message| content_text(&message["content"]))
        .collect::<Vec<_>>()
        .join("\n");
    let tail = system
        .split_once("本轮知识资料范围：")
        .expect("实际模型请求必须带有本轮服务铸造范围")
        .1;
    serde_json::Deserializer::from_str(tail)
        .into_iter::<Value>()
        .next()
        .unwrap()
        .unwrap()
}

async fn provider(
    State(captures): State<Arc<Mutex<Captures>>>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    let input = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|message| message["role"] == "user")
        .map(|message| content_text(&message["content"]))
        .unwrap();
    let mode = if input.contains("F13_FIRST") {
        "first"
    } else if input.contains("F13_SECOND") {
        "second"
    } else {
        "invalid"
    };
    // 非法选择若意外上网也必须先留下真实捕获，不能靠替身崩溃假装零请求。
    let selection = if mode == "invalid" {
        Value::Null
    } else {
        wire_selection(&body)
    };
    let (step, old_scope) = {
        let mut captures = captures.lock().unwrap();
        let step = *captures.steps.get(mode).unwrap_or(&0);
        captures.steps.insert(mode.to_string(), step + 1);
        let old_scope = captures.previous_scope.clone();
        captures
            .requests
            .push(json!({"mode":mode,"selection":selection,"body":body}));
        (if mode == "invalid" { 2 } else { step }, old_scope)
    };
    let delta = match step {
        0 => {
            json!({"role":"assistant","tool_calls":[{"index":0,"id":format!("{mode}-describe"),"type":"function","function":{"name":"mcp_describe_tool","arguments":json!({"name":"knowledge_outline"}).to_string()}}]})
        }
        1 => {
            // 第一轮用真实提示内的新句柄；第二轮刻意重放上一轮真实句柄。
            let scope = if mode == "second" {
                old_scope.expect("上一轮句柄来自实际生产请求")
            } else {
                selection["scopeId"].as_str().unwrap().to_string()
            };
            json!({"role":"assistant","tool_calls":[{"index":0,"id":format!("{mode}-outline"),"type":"function","function":{"name":"mcp_call","arguments":json!({"tool":"knowledge_outline","arguments":{"scopeId":scope}}).to_string()}}]})
        }
        2 => json!({"role":"assistant","content":"本轮已完成"}),
        _ => panic!("范围专项调用未按预期结束"),
    };
    let finish = if step < 2 { "tool_calls" } else { "stop" };
    let frames = [
        json!({"id":format!("{mode}-{step}"),"choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
        json!({"id":format!("{mode}-{step}"),"choices":[{"index":0,"delta":{},"finish_reason":finish}]}),
        json!({"choices":[],"usage":{"prompt_tokens":20,"completion_tokens":4}}),
    ];
    let stream = frames
        .iter()
        .map(|frame| format!("data: {frame}\n\n"))
        .collect::<String>()
        + "data: [DONE]\n\n";
    ([(header::CONTENT_TYPE, "text/event-stream")], stream).into_response()
}

async fn boot(home: &Path, endpoint: &str) -> ServiceState {
    let layout = prepare_layout(home).unwrap();
    let config_path = home.join("synthetic-service.json");
    std::fs::write(
        &config_path,
        json!({"home":home,"providers":{"main":{"protocol":"openai-completions","endpoint":endpoint,"auth":{"kind":"apiKey","apiKey":"F13_SYNTHETIC_ONLY"}}},"models":{"chat":{"provider":"main","model":"synthetic","capabilities":{"tools":true}}}}).to_string(),
    ).unwrap();
    let (source, plane) =
        lingxi_service::config::resolve_model_plane(Some(&config_path), &layout.runtime_dir)
            .unwrap()
            .unwrap();
    let credentials = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &layout.runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .unwrap(),
    );
    ServiceState::bootstrap_with_deps(
        ServiceConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            data_home: home.to_path_buf(),
            home_source: HomeSource::Cli,
            network_mode: NetworkMode::Loopback,
            shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
        },
        &layout,
        ServiceDeps {
            model_gateway: Some(Arc::new(
                lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
            )),
            model_plane_source: Some(source),
            credential_service: Some(credentials),
            workspace_root: Some(home.to_path_buf()),
            ..ServiceDeps::default()
        },
    )
    .await
    .unwrap()
}

fn source(state: &ServiceState, name: &str, text: &str) -> (String, String) {
    let notebook = state.knowledge().create_notebook(name).unwrap();
    let notebook = notebook["id"].as_str().unwrap().to_string();
    let imported = state
        .knowledge()
        .import_pasted_text(&notebook, text, Some(name))
        .unwrap();
    (
        notebook,
        imported["source"]["id"].as_str().unwrap().to_string(),
    )
}

async fn execute(endpoint: &str, token: &str, body: Value) -> Value {
    let task = async {
        let address = endpoint.strip_prefix("http://").unwrap();
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        let payload = body.to_string();
        let request = format!(
            "POST /lingxi/v1/sessions/sess_local_alpha/execute HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len(),
        );
        socket.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        socket.read_to_end(&mut response).await.unwrap();
        let boundary = response
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .unwrap();
        let headers = String::from_utf8_lossy(&response[..boundary]);
        let status: u16 = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
        let result: Value = serde_json::from_slice(&response[boundary + 4..]).unwrap();
        assert_eq!(status, 200, "真实提交必须得到明确结果：{result}");
        result
    };
    tokio::time::timeout(Duration::from_secs(20), task)
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_submission_freezes_selected_sources_and_rejects_old_or_foreign_scope() {
    let home =
        PathBuf::from(std::env::var_os("LINGXI_HOME").expect("仅使用专项合成HOME")).join(format!(
            "f13-submission-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&home).unwrap();
    let captures = Arc::new(Mutex::new(Captures::default()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider_endpoint = format!("http://{}", listener.local_addr().unwrap());
    let upstream = tokio::spawn(
        axum::serve(
            listener,
            axum::Router::new()
                .route("/chat/completions", post(provider))
                .with_state(captures.clone()),
        )
        .into_future(),
    );
    let state = boot(&home, &provider_endpoint).await;
    let (notebook_a, source_a) = source(&state, "本轮A", "# A\n\nF13只授权A资料。");
    let (notebook_b, source_b) = source(&state, "下一轮B", "# B\n\nF13只授权B资料。");
    let token = state.auth().local_token();
    let serving_state = state.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let service = tokio::spawn(async move {
        run(
            serving_state,
            async {
                let _ = shutdown_rx.await;
            },
            |addr| {
                let _ = ready_tx.send(addr);
            },
            None,
        )
        .await
    });
    let endpoint = format!("http://{}", ready_rx.await.unwrap());
    let first = execute(&endpoint, &token, json!({"input":"F13_FIRST","knowledge":{"notebookIds":[notebook_a],"sourceIds":[source_a]}})).await;
    let first_run = first["runId"].as_str().unwrap();
    let first_requests = captures.lock().unwrap().requests.clone();
    assert_eq!(
        first_requests.len(),
        3,
        "必须真实描述并成功执行当前范围的工具"
    );
    let first_selection = &first_requests[0]["selection"];
    assert_eq!(first_selection["notebookIds"], json!([notebook_a]));
    assert_eq!(first_selection["sources"].as_array().unwrap().len(), 1);
    assert_eq!(first_selection["sources"][0]["sourceId"], source_a);
    let first_scope_id = first_selection["scopeId"].as_str().unwrap();
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let first_scope = facts.get_turn_scope(first_scope_id).unwrap().unwrap();
    let owner = first_scope
        .run_binding
        .as_ref()
        .expect("运行范围必须保留可信归属");
    let journal = state
        .storage()
        .load_invocation_journal(&RunId::new(first_run.to_string()))
        .await
        .unwrap();
    let outline = journal
        .iter()
        .find(|entry| entry.target.ends_with("knowledge_outline"))
        .expect("必须执行真实知识工具");
    assert_eq!(
        outline.receipt.as_ref().unwrap().outcome,
        ReceiptOutcome::Succeeded
    );
    assert_eq!(first_scope.turn_id, first_run);
    assert_eq!(owner.run_id, first_run);
    assert_eq!(owner.session_id, "sess_local_alpha");
    assert_eq!(owner.agent_id, "lingxi");
    assert_eq!(owner.principal_kind, outline.owner_kind);
    assert_eq!(owner.principal_subject, outline.owner_subject);
    assert_eq!(owner.principal_subject, lingxi_kernel::LOCAL_OWNER_SUBJECT);
    assert_eq!(owner.attempt, outline.attempt);
    assert_eq!(owner.generation, outline.generation);
    captures.lock().unwrap().previous_scope = Some(first_scope_id.to_string());

    let second = execute(&endpoint, &token, json!({"input":"F13_SECOND","knowledge":{"notebookIds":[notebook_b],"sourceIds":[source_b]}})).await;
    let requests = captures.lock().unwrap().requests.clone();
    assert_eq!(requests.len(), 6);
    let selection = &requests[3]["selection"];
    let second_scope_id = selection["scopeId"].as_str().unwrap();
    assert_ne!(first_scope_id, second_scope_id);
    assert_eq!(selection["notebookIds"], json!([notebook_b]));
    assert_eq!(selection["sources"][0]["sourceId"], source_b);
    assert_eq!(
        facts
            .get_turn_scope(first_scope_id)
            .unwrap()
            .unwrap()
            .status,
        "closed"
    );
    let second_journal = state
        .storage()
        .load_invocation_journal(&RunId::new(second["runId"].as_str().unwrap().to_string()))
        .await
        .unwrap();
    let denied = second_journal
        .iter()
        .find(|entry| entry.target.ends_with("knowledge_outline"))
        .expect("旧范围拒绝必须留下真实调用记录");
    assert_eq!(
        denied.receipt.as_ref().unwrap().outcome,
        ReceiptOutcome::Failed
    );
    assert!(
        requests[5]["body"]
            .to_string()
            .contains("KNOWLEDGE_SCOPE_VIOLATION"),
        "实际下一次模型请求必须收到范围拒绝"
    );

    // source B 真实存在，但不在所选笔记本 A；拒绝发生在发出模型请求之前。
    let invalid = execute(&endpoint, &token, json!({"input":"F13_INVALID","knowledge":{"notebookIds":[notebook_a],"sourceIds":[source_b]}})).await;
    assert_eq!(
        captures.lock().unwrap().requests.len(),
        6,
        "非法选择必须零模型请求"
    );
    let invalid_run = state
        .storage()
        .load_run(&RunId::new(invalid["runId"].as_str().unwrap().to_string()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(invalid_run.status, RunStatus::Failed);
    assert_eq!(
        facts
            .get_turn_scope(second_scope_id)
            .unwrap()
            .unwrap()
            .status,
        "active",
        "非法选择必须回滚，不能破坏上一份已授权范围"
    );
    // 同一会话后续未选资料不继承检索声明；显式空选择仍按原合同在发请求前拒绝。
    for (input, knowledge) in [
        ("F13_NONE", None),
        ("F13_EMPTY", Some(json!({"notebookIds":[],"sourceIds":[]}))),
    ] {
        let mut body = json!({"input":input});
        if let Some(selection) = knowledge {
            body["knowledge"] = selection;
        }
        let next = execute(&endpoint, &token, body).await;
        let next_run = state
            .storage()
            .load_run(&RunId::new(next["runId"].as_str().unwrap()))
            .await
            .unwrap()
            .unwrap();
        if input == "F13_EMPTY" {
            assert_eq!(next_run.status, RunStatus::Failed);
            assert_eq!(
                captures.lock().unwrap().requests.len(),
                7,
                "空选择拒绝必须零模型请求"
            );
            continue;
        }
        assert_eq!(next_run.status, RunStatus::Completed);
        let capture = captures.lock().unwrap().requests.last().unwrap().clone();
        let declarations = capture["body"]["tools"].as_array().unwrap();
        // 默认工具面按名钉住（1be910755 恢复可信工具曝光后由 7 扩到 18；
        // 后续有意增减默认面时必须显式更新本清单，不许静默漂移）。
        let mut names: Vec<&str> = declarations
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "automation",
                "browser",
                "channel",
                "dm",
                "edit",
                "exec_command",
                "jian_update_status",
                "mcp_call",
                "mcp_describe_tool",
                "mcp_search_tools",
                "notify",
                "patrol_update_log",
                "read",
                "update_settings",
                "web_fetch",
                "web_search",
                "write",
                "write_stdin",
            ]
        );
        assert!(!declarations
            .iter()
            .any(|tool| tool["function"]["name"] == "knowledge_search"));
        let system = capture["body"]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "system")
            .map(|message| content_text(&message["content"]))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!system.contains("本轮知识资料范围："));
    }
    shutdown_tx.send(()).unwrap();
    service.await.unwrap().unwrap();
    state.storage().close().await.unwrap();
    drop(facts);
    upstream.abort();
    let _ = upstream.await;
    std::fs::remove_dir_all(home).unwrap();
}

#[derive(Default)]
struct RetryCaptures {
    requests: Vec<Value>,
    steps: BTreeMap<String, usize>,
    on_failure: Option<Arc<dyn Fn() + Send + Sync>>,
}

async fn retry_provider(
    State(captures): State<Arc<Mutex<RetryCaptures>>>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    let input = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|message| message["role"] == "user")
        .map(|message| content_text(&message["content"]))
        .unwrap();
    let mode = if input.contains("F13_RETRY") {
        "retry"
    } else {
        "control"
    };
    let selection = wire_selection(&body);
    let (step, old_scope, on_failure) = {
        let mut captures = captures.lock().unwrap();
        let step = *captures.steps.get(mode).unwrap_or(&0);
        captures.steps.insert(mode.into(), step + 1);
        let old_scope = captures
            .requests
            .iter()
            .find(|request| request["mode"] == mode)
            .map(|request| {
                request["selection"]["scopeId"]
                    .as_str()
                    .unwrap()
                    .to_string()
            });
        captures
            .requests
            .push(json!({"mode":mode,"selection":selection,"body":body}));
        (step, old_scope, captures.on_failure.clone())
    };
    if mode == "retry" && step == 0 {
        on_failure.expect("合成失败时修改当前资料集合")();
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":{"message":"合成供应商暂时失败"}})),
        )
            .into_response();
    }
    let logical = step - usize::from(mode == "retry");
    let delta = match logical {
        0 => {
            json!({"role":"assistant","tool_calls":[{"index":0,"id":format!("{mode}-describe"),"type":"function","function":{"name":"mcp_describe_tool","arguments":json!({"name":"knowledge_outline"}).to_string()}}]})
        }
        1 | 2 if logical == 1 || mode == "retry" => {
            let scope = if logical == 2 {
                old_scope.unwrap()
            } else {
                selection["scopeId"].as_str().unwrap().to_string()
            };
            json!({"role":"assistant","tool_calls":[{"index":0,"id":format!("{mode}-outline-{logical}"),"type":"function","function":{"name":"mcp_call","arguments":json!({"tool":"knowledge_outline","arguments":{"scopeId":scope}}).to_string()}}]})
        }
        _ => json!({"role":"assistant","content":"明确处理知识范围后完成"}),
    };
    let tool_call = delta.get("tool_calls").is_some();
    let frames = [
        json!({"id":format!("{mode}-{step}"),"choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
        json!({"id":format!("{mode}-{step}"),"choices":[{"index":0,"delta":{},"finish_reason":if tool_call { "tool_calls" } else { "stop" }}]}),
        json!({"choices":[],"usage":{"prompt_tokens":20,"completion_tokens":4}}),
    ];
    let stream = frames
        .iter()
        .map(|frame| format!("data: {frame}\n\n"))
        .collect::<String>()
        + "data: [DONE]\n\n";
    ([(header::CONTENT_TYPE, "text/event-stream")], stream).into_response()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_http_500_retry_advances_only_attempt_and_preserves_frozen_sources() {
    let home =
        PathBuf::from(std::env::var_os("LINGXI_HOME").expect("仅使用专项合成HOME")).join(format!(
            "f13-retry-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&home).unwrap();
    let captures = Arc::new(Mutex::new(RetryCaptures::default()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider_endpoint = format!("http://{}", listener.local_addr().unwrap());
    let upstream = tokio::spawn(
        axum::serve(
            listener,
            axum::Router::new()
                .route("/chat/completions", post(retry_provider))
                .with_state(captures.clone()),
        )
        .into_future(),
    );
    let state = boot(&home, &provider_endpoint).await;
    let (notebook, source_id) = source(
        &state,
        "冻结资料",
        "# 原内容\n\nF13已授权且必须冻结的来源。",
    );
    let knowledge = state.knowledge().clone();
    let (mutation_home, mutation_notebook, mutation_source) =
        (home.clone(), notebook.clone(), source_id.clone());
    captures.lock().unwrap().on_failure = Some(Arc::new(move || {
        // 失败期间模拟用户资料更新；重试不得重选新来源或丢掉原冻结来源。
        let facts = KnowledgeFactStore::open(&mutation_home.join("knowledge")).unwrap();
        facts
            .remove_source_from_notebook(&mutation_notebook, &mutation_source)
            .unwrap();
        knowledge
            .import_pasted_text(
                &mutation_notebook,
                "# 新内容\n\n本轮未授权的新来源。",
                Some("新来源"),
            )
            .unwrap();
    }));
    let token = state.auth().local_token();
    let serving_state = state.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let service = tokio::spawn(async move {
        run(
            serving_state,
            async {
                let _ = shutdown_rx.await;
            },
            |addr| {
                let _ = ready_tx.send(addr);
            },
            None,
        )
        .await
    });
    let endpoint = format!("http://{}", ready_rx.await.unwrap());
    let submission = |input| json!({"input":input,"knowledge":{"notebookIds":[notebook],"sourceIds":[source_id]}});
    let control = execute(&endpoint, &token, submission("F13_CONTROL")).await;
    let control_journal = state
        .storage()
        .load_invocation_journal(&RunId::new(control["runId"].as_str().unwrap()))
        .await
        .unwrap();
    assert_eq!(
        control_journal
            .iter()
            .find(|entry| entry.target.ends_with("knowledge_outline"))
            .unwrap()
            .receipt
            .as_ref()
            .unwrap()
            .outcome,
        ReceiptOutcome::Succeeded
    );
    let retried = execute(&endpoint, &token, submission("F13_RETRY")).await;
    let run_id = retried["runId"].as_str().unwrap();
    assert_eq!(
        state
            .storage()
            .load_run(&RunId::new(run_id))
            .await
            .unwrap()
            .unwrap()
            .status,
        RunStatus::Completed
    );
    let requests = captures.lock().unwrap().requests.clone();
    let retry_requests = requests
        .iter()
        .filter(|request| request["mode"] == "retry")
        .collect::<Vec<_>>();
    assert_eq!(retry_requests.len(), 5);
    let old_id = retry_requests[0]["selection"]["scopeId"].as_str().unwrap();
    let current_id = retry_requests[1]["selection"]["scopeId"].as_str().unwrap();
    assert_ne!(old_id, current_id, "旧attempt句柄不能成为新attempt授权");
    for request in &retry_requests[1..] {
        assert_eq!(request["selection"], retry_requests[1]["selection"]);
        assert_eq!(
            request["selection"]["sources"],
            retry_requests[0]["selection"]["sources"]
        );
    }
    let facts = KnowledgeFactStore::open(&home.join("knowledge")).unwrap();
    let old = facts.get_turn_scope(old_id).unwrap().unwrap();
    let current = facts.get_turn_scope(current_id).unwrap().unwrap();
    assert_eq!(old.status, "closed");
    assert_eq!(current.status, "active");
    assert_eq!(current.notebook_ids, old.notebook_ids);
    assert_eq!(current.sources[0].source_id, source_id);
    assert_eq!(
        current.sources[0].content_snapshot_id,
        old.sources[0].content_snapshot_id
    );
    assert_eq!(
        current.sources[0].parse_artifact_id,
        old.sources[0].parse_artifact_id
    );
    let previous = old.run_binding.as_ref().unwrap();
    let binding = current.run_binding.as_ref().unwrap();
    let mut expected = previous.clone();
    expected.attempt = format!("{run_id}#a2");
    assert_eq!(binding, &expected);
    let journal = state
        .storage()
        .load_invocation_journal(&RunId::new(run_id))
        .await
        .unwrap();
    let outlines = journal
        .iter()
        .filter(|entry| entry.target.ends_with("knowledge_outline"))
        .collect::<Vec<_>>();
    assert_eq!(outlines.len(), 2);
    assert_eq!(
        outlines[0].receipt.as_ref().unwrap().outcome,
        ReceiptOutcome::Succeeded
    );
    assert_eq!(
        outlines[1].receipt.as_ref().unwrap().outcome,
        ReceiptOutcome::Failed
    );
    assert!(outlines
        .iter()
        .all(|entry| entry.attempt == binding.attempt));
    assert!(retry_requests[4]["body"]
        .to_string()
        .contains("KNOWLEDGE_SCOPE_VIOLATION"));
    // 被跳过的attempt与身份变化不能使用重试接口；每次拒绝均不破坏当前范围。
    let mut valid_next = binding.clone();
    valid_next.attempt = format!("{run_id}#a3");
    let mut denied = Vec::new();
    for field in [
        "principal_kind",
        "principal_subject",
        "agent_id",
        "session_id",
        "run_id",
        "generation",
        "attempt",
    ] {
        let mut next = valid_next.clone();
        match field {
            "principal_kind" => next.principal_kind = "web_session".into(),
            "principal_subject" => next.principal_subject = "foreign-subject".into(),
            "agent_id" => next.agent_id = "foreign-agent".into(),
            "session_id" => next.session_id = "foreign-session".into(),
            "run_id" => next.run_id = "foreign-run".into(),
            "generation" => next.generation += 1,
            _ => next.attempt = format!("{run_id}#a4"),
        }
        assert!(
            facts
                .advance_run_scope_attempt(&current.studio_id, current_id, binding, &next)
                .is_err(),
            "{field}"
        );
        denied.push(field);
    }
    assert!(facts
        .advance_run_scope_attempt(&old.studio_id, old_id, previous, binding)
        .is_err());
    assert_eq!(facts.get_turn_scope(current_id).unwrap().unwrap(), current);
    if let Ok(out) = std::env::var("LINGXI_F13_EVIDENCE") {
        let out = PathBuf::from(out);
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("retry-scope.json"), serde_json::to_vec_pretty(&json!({"control":control,"retried":retried,"wire":requests,"oldScope":state.knowledge().get_turn_scope(old_id).unwrap(),"currentScope":state.knowledge().get_turn_scope(current_id).unwrap(),"deniedBindingChanges":denied})).unwrap()).unwrap();
    }
    shutdown_tx.send(()).unwrap();
    service.await.unwrap().unwrap();
    state.storage().close().await.unwrap();
    drop(facts);
    upstream.abort();
    let _ = upstream.await;
    std::fs::remove_dir_all(home).unwrap();
}
