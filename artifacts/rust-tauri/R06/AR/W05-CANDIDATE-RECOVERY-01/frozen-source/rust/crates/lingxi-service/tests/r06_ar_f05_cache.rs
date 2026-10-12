//! F05：只有外部模型为 loopback 替身，编译、认证、存储与子代理均走生产链。
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{extract::State, routing::post, Json};
use lingxi_kernel::ports::DelegationRequest;
use lingxi_protocol::{RunId, ToolCallId};
use lingxi_service::context_compiler::{
    ContextCompilerService, FileContextMaterialSource, HostFacts,
};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use serde_json::{json, Value};

#[derive(Clone)]
struct WireState {
    sent: Arc<Mutex<Vec<Value>>>,
    parent_release: Arc<tokio::sync::Notify>,
    arrived: Arc<tokio::sync::Notify>,
}

async fn provider(State(w): State<WireState>, Json(body): Json<Value>) -> String {
    let parent = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["content"] == "PARENT_HOLD");
    w.sent.lock().unwrap().push(body);
    w.arrived.notify_one();
    if parent {
        w.parent_release.notified().await;
    }
    let frames = [
        json!({"id":"synthetic","choices":[{"index":0,"delta":{"role":"assistant","content":"done"},"finish_reason":null}]}),
        json!({"id":"synthetic","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
        json!({"id":"synthetic","choices":[],"usage":{"prompt_tokens":40,"completion_tokens":3}}),
    ];
    frames
        .iter()
        .map(|f| format!("data: {f}\n\n"))
        .collect::<String>()
        + "data: [DONE]\n\n"
}

fn write(root: &Path, file: &str, text: &str) {
    let p = root.join(file);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn principal() -> lingxi_service::Principal {
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_local".into(),
        kind: lingxi_service::PrincipalKind::LocalUser,
        user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.into()),
        studio_id: None,
        server_node_id: None,
        device_id: None,
        credential_id: None,
        web_session_id: None,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Local,
        credential_kind: lingxi_service::CredentialKind::LoopbackToken,
        trust_state: lingxi_service::TrustState::Local,
        scopes: vec!["chat".into()],
    }
}

async fn live_tools_provider(State(w): State<WireState>, Json(body): Json<Value>) -> String {
    let index = {
        let mut wire = w.sent.lock().unwrap();
        let index = wire.len();
        wire.push(body);
        index
    };
    w.arrived.notify_one();
    if index == 0 {
        w.parent_release.notified().await;
    }
    let frames = if index == 0 || index == 2 {
        vec![
            json!({"id":"synthetic","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":format!("live-read-{index}"),"type":"function","function":{"name":"read","arguments":"{\"path\":\"input.txt\"}"}}]},"finish_reason":null}]}),
            json!({"id":"synthetic","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
        ]
    } else {
        vec![
            json!({"id":"synthetic","choices":[{"index":0,"delta":{"role":"assistant","content":"done"},"finish_reason":null}]}),
            json!({"id":"synthetic","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
        ]
    };
    frames.iter().map(|f| format!("data: {f}\n\n")).collect::<String>()
        + "data: {\"id\":\"synthetic\",\"choices\":[],\"usage\":{\"prompt_tokens\":40,\"completion_tokens\":5}}\n\ndata: [DONE]\n\n"
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn c01_actual_calls_keep_system_frozen_and_tools_live_after_revocation() {
    use lingxi_kernel::toolcatalog::{Availability, ToolTargetRef};
    let home = own_home("tools-live");
    write(
        &home,
        "agents/lingxi/config.yaml",
        "locale: zh-CN\nagent:\n  name: 灵犀\n",
    );
    write(&home, "agents/lingxi/identity.md", "TOOLS_PERSONA_OLD");
    write(&home, "input.txt", "SYNTHETIC_READ_CONTROL");
    let w = WireState {
        sent: Arc::new(Mutex::new(Vec::new())),
        parent_release: Arc::new(tokio::sync::Notify::new()),
        arrived: Arc::new(tokio::sync::Notify::new()),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(
        axum::serve(
            listener,
            axum::Router::new()
                .route("/chat/completions", post(live_tools_provider))
                .with_state(w.clone()),
        )
        .into_future(),
    );
    let state = boot(&home, &endpoint).await;
    let first_state = state.clone();
    let first = tokio::spawn(async move {
        first_state
            .sessions()
            .execute_for(
                first_state.storage().as_ref(),
                first_state.events(),
                first_state.runs(),
                &principal(),
                "sess_local_alpha",
                "live tool first",
                1790409600000,
            )
            .await
            .unwrap()
    });
    wait_count(&w, 1).await;
    let first_observation = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(view) = state
                .storage()
                .model_context_observation("sess_local_alpha", None, None, None)
                .await
                .unwrap()
            {
                break view;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let registry = state.tool_gateway().unwrap().registry();
    let target = registry
        .resolve(&ToolTargetRef::ByName {
            name: "read".into(),
        })
        .unwrap();
    registry
        .set_availability(
            &target,
            Availability::Disabled {
                reason: "synthetic user revocation".into(),
            },
        )
        .unwrap();
    lingxi_adapters::storage::agent_memory::atomic_write(
        &home.join("agents/lingxi/identity.md"),
        "TOOLS_PERSONA_NEW",
    )
    .unwrap();
    w.parent_release.notify_one();
    let settled = first.await.unwrap();
    use lingxi_kernel::ports::StoragePort;
    let journal = state
        .storage()
        .load_invocation_journal(&RunId::new(
            first_observation["runId"].as_str().unwrap().to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(journal.len(), 1, "真实撤销拒绝必须留下一条执行账本");
    assert!(
        journal[0].receipt.as_ref().is_some_and(
            |r| !r.dispatched && r.outcome == lingxi_kernel::ports::ReceiptOutcome::Failed
        ),
        "禁用之后必须零派发的真实拒绝"
    );

    if let Ok(dir) = std::env::var("LINGXI_F05_EVIDENCE") {
        std::fs::create_dir_all(&dir).unwrap();
        let attempted = state
            .storage()
            .model_context_observation("sess_local_alpha", None, None, None)
            .await
            .unwrap();
        std::fs::write(Path::new(&dir).join("open-tool-history-wire.json"), serde_json::to_string_pretty(&json!({
            "findingId":"R06-AR-W05-EXTRA-TOOL-HISTORY-01", "wire":w.sent.lock().unwrap().clone(),
            "firstActualWireObservation":first_observation, "latestDriverAttemptObservation":attempted,
            "settledSubmission":settled, "invocationJournal":format!("{journal:?}"), "originalSqlite":home.join("lingxi-service/data/runs.db"),
            "warning":"second driver model call started, but protocol failed locally; no second HTTP wire exists"
        })).unwrap()).unwrap();
    }
    wait_count(&w, 2).await;
    let last = state
        .storage()
        .model_context_observation("sess_local_alpha", None, None, None)
        .await
        .unwrap()
        .unwrap();
    let run_id = first_observation["runId"].as_str().unwrap();
    let first_call = first_observation["modelCallId"]
        .as_str()
        .unwrap()
        .to_string();
    // 实际已发送的两次 call，各自按真实 callId 回读，不能拿会话最新值替代。
    assert_eq!(
        state
            .storage()
            .model_context_observation("sess_local_alpha", Some(run_id), Some(&first_call), None)
            .await
            .unwrap()
            .unwrap(),
        first_observation
    );
    let wire = w.sent.lock().unwrap().clone();
    assert_eq!(first_observation["renderSha256"], last["renderSha256"]);
    assert_ne!(
        first_observation["toolCatalogGeneration"],
        last["toolCatalogGeneration"]
    );
    assert_ne!(first_observation["snapshotId"], last["snapshotId"]);
    assert!(wire[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["function"]["name"] == "read"));
    assert!(wire[1]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .all(|t| t["function"]["name"] != "read"));
    assert_eq!(wire[0]["messages"][0], wire[1]["messages"][0]);
    assert!(
        wire[1].to_string().contains("disabled and not callable"),
        "撤销之后真实执行必须拒绝：{}",
        wire[1]
    );
    assert!(!wire[1].to_string().contains("SYNTHETIC_READ_CONTROL"));
    registry
        .set_availability(&target, Availability::Available)
        .unwrap();
    // 正对照改写实际输入，遵守原 FileTools 的重复读取去重规则。
    write(&home, "input.txt", "SYNTHETIC_READ_CONTROL_CHANGED");
    state
        .sessions()
        .execute_for(
            state.storage().as_ref(),
            state.events(),
            state.runs(),
            &principal(),
            "sess_local_alpha",
            "live tool positive control",
            1790409600000,
        )
        .await
        .unwrap();
    wait_count(&w, 4).await;
    let wire = w.sent.lock().unwrap().clone();
    assert!(wire[2]["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("TOOLS_PERSONA_NEW"));
    assert!(
        wire[3].to_string().contains("SYNTHETIC_READ_CONTROL"),
        "重新启用后必须真实读取，证明拒绝没有遮蔽假请求"
    );
    assert_eq!(
        state
            .storage()
            .model_context_observation("sess_local_alpha", Some(run_id), Some(&first_call), None)
            .await
            .unwrap()
            .unwrap(),
        first_observation
    );
    if let Ok(dir) = std::env::var("LINGXI_F05_EVIDENCE") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Path::new(&dir).join("c01-tools-wire.json"), serde_json::to_string_pretty(&json!({"wire":wire,"first":first_observation,"second":last,"revokedExecutionRefused":true,"reenabledExecutionReadPositiveControl":true})).unwrap()).unwrap();
    }
    state.storage().close().await.unwrap();
    server.abort();
    let _ = server.await;
    std::fs::remove_dir_all(home).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn c01_actual_calls_keep_system_frozen_and_tools_live_after_description_update() {
    use lingxi_kernel::toolcatalog::{Availability, ToolTargetRef};
    let home = own_home("tools-description");
    write(
        &home,
        "agents/lingxi/config.yaml",
        "locale: zh-CN\nagent:\n  name: 灵犀\n",
    );
    write(&home, "agents/lingxi/identity.md", "TOOLS_PERSONA_OLD");
    write(&home, "input.txt", "SYNTHETIC_READ_CONTROL");
    let w = WireState {
        sent: Arc::new(Mutex::new(Vec::new())),
        parent_release: Arc::new(tokio::sync::Notify::new()),
        arrived: Arc::new(tokio::sync::Notify::new()),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(
        axum::serve(
            listener,
            axum::Router::new()
                .route("/chat/completions", post(live_tools_provider))
                .with_state(w.clone()),
        )
        .into_future(),
    );
    let state = boot(&home, &endpoint).await;
    let first_state = state.clone();
    let first = tokio::spawn(async move {
        first_state
            .sessions()
            .execute_for(
                first_state.storage().as_ref(),
                first_state.events(),
                first_state.runs(),
                &principal(),
                "sess_local_alpha",
                "live tool first",
                1790409600000,
            )
            .await
            .unwrap()
    });
    wait_count(&w, 1).await;
    let first_observation = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(view) = state
                .storage()
                .model_context_observation("sess_local_alpha", None, None, None)
                .await
                .unwrap()
            {
                break view;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let registry = state.tool_gateway().unwrap().registry();
    let target = registry
        .resolve(&ToolTargetRef::ByName {
            name: "read".into(),
        })
        .unwrap();
    let full = registry.describe_full(&target).unwrap();
    let listing = full.listing;
    registry
        .update(
            &target,
            lingxi_kernel::toolcatalog::ToolManifest {
                origin: listing.origin,
                local_name: listing.local_name,
                display_name: "SYNTHETIC_UPDATED_READ_DESCRIPTION".into(),
                aliases: listing.aliases,
                version: listing.version,
                description: "SYNTHETIC_UPDATED_READ_DESCRIPTION".into(),
                input_schema: full.input_schema,
                output_schema: None,
                permission: listing.permission,
                availability: listing.availability,
                timeout_ms: listing.timeout_ms,
                max_concurrency: None,
                declared_permission: listing.declared_permission,
                recovery: listing.recovery,
            },
            &lingxi_kernel::toolcatalog::SchemaBudget::default(),
        )
        .unwrap();
    lingxi_adapters::storage::agent_memory::atomic_write(
        &home.join("agents/lingxi/identity.md"),
        "TOOLS_PERSONA_NEW",
    )
    .unwrap();
    w.parent_release.notify_one();
    first.await.unwrap();
    wait_count(&w, 2).await;
    let last = state
        .storage()
        .model_context_observation("sess_local_alpha", None, None, None)
        .await
        .unwrap()
        .unwrap();
    let run_id = first_observation["runId"].as_str().unwrap();
    let first_call = first_observation["modelCallId"]
        .as_str()
        .unwrap()
        .to_string();
    // 实际已发送的两次 call，各自按真实 callId 回读，不能拿会话最新值替代。
    assert_eq!(
        state
            .storage()
            .model_context_observation("sess_local_alpha", Some(run_id), Some(&first_call), None)
            .await
            .unwrap()
            .unwrap(),
        first_observation
    );
    let wire = w.sent.lock().unwrap().clone();
    assert_eq!(first_observation["renderSha256"], last["renderSha256"]);
    assert_ne!(
        first_observation["toolCatalogGeneration"],
        last["toolCatalogGeneration"]
    );
    assert_ne!(first_observation["snapshotId"], last["snapshotId"]);
    assert!(wire[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["function"]["name"] == "read"));
    assert!(wire[1]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["function"]["name"] == "read"
            && t["function"]["description"] == "SYNTHETIC_UPDATED_READ_DESCRIPTION"));
    assert_eq!(wire[0]["messages"][0], wire[1]["messages"][0]);
    assert!(
        wire[1].to_string().contains("SYNTHETIC_READ_CONTROL"),
        "真实读取的正对照"
    );
    registry
        .set_availability(&target, Availability::Available)
        .unwrap();
    // 正对照改写实际输入，遵守原 FileTools 的重复读取去重规则。
    write(&home, "input.txt", "SYNTHETIC_READ_CONTROL_CHANGED");
    state
        .sessions()
        .execute_for(
            state.storage().as_ref(),
            state.events(),
            state.runs(),
            &principal(),
            "sess_local_alpha",
            "live tool positive control",
            1790409600000,
        )
        .await
        .unwrap();
    wait_count(&w, 4).await;
    let wire = w.sent.lock().unwrap().clone();
    assert!(wire[2]["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("TOOLS_PERSONA_NEW"));
    assert!(
        wire[3].to_string().contains("SYNTHETIC_READ_CONTROL"),
        "重新启用后必须真实读取，证明拒绝没有遮蔽假请求"
    );
    assert_eq!(
        state
            .storage()
            .model_context_observation("sess_local_alpha", Some(run_id), Some(&first_call), None)
            .await
            .unwrap()
            .unwrap(),
        first_observation
    );
    if let Ok(dir) = std::env::var("LINGXI_F05_EVIDENCE") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Path::new(&dir).join("c01-tools-description-wire.json"), serde_json::to_string_pretty(&json!({"wire":wire,"first":first_observation,"second":last,"descriptionGenerationUpdatedAtActualSend":true,"reenabledExecutionReadPositiveControl":true})).unwrap()).unwrap();
    }
    state.storage().close().await.unwrap();
    server.abort();
    let _ = server.await;
    std::fs::remove_dir_all(home).unwrap();
}

async fn boot(home: &Path, endpoint: &str) -> ServiceState {
    boot_with_source(home, endpoint, None).await
}

async fn boot_with_source(
    home: &Path,
    endpoint: &str,
    source_override: Option<Arc<dyn lingxi_service::context_compiler::ContextMaterialSource>>,
) -> ServiceState {
    std::fs::create_dir_all(home).unwrap();
    let layout = prepare_layout(home).unwrap();
    let cfg = home.join("service.json");
    std::fs::write(&cfg,json!({"home":home,"providers":{"main":{"protocol":"openai-completions","endpoint":endpoint,"auth":{"kind":"apiKey","apiKey":"synthetic-only"}}},"models":{"chat":{"provider":"main","model":"synthetic","capabilities":{"tools":true}}}}).to_string()).unwrap();
    let (source, plane) =
        lingxi_service::config::resolve_model_plane(Some(&cfg), &layout.runtime_dir)
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
    let product = home.join("product");
    write(
        &product,
        "yuan/lingxi.md",
        "你是{{agentName}}，服务{{userName}}。",
    );
    write(&product, "agents.example.md", "shared persona template");
    let compiler = Arc::new(ContextCompilerService::new(source_override.unwrap_or_else(
        || {
            Arc::new(FileContextMaterialSource::from_home(
                home,
                product,
                HostFacts {
                    platform: "darwin".into(),
                    os_type: "Darwin".into(),
                    os_release: "synthetic".into(),
                    shell_label: "zsh".into(),
                    workspace: home.to_string_lossy().into(),
                },
            ))
        },
    )));
    ServiceState::bootstrap_with_deps(
        ServiceConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            data_home: home.into(),
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
            context_compiler: Some(compiler),
            workspace_root: Some(home.to_path_buf()),
            ..ServiceDeps::default()
        },
    )
    .await
    .unwrap()
}

async fn wait_count(w: &WireState, n: usize) {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let notified = w.arrived.notified();
            if w.sent.lock().unwrap().len() >= n {
                break;
            }
            notified.await;
        }
    })
    .await
    .unwrap();
}

fn own_home(tag: &str) -> PathBuf {
    let base = std::env::var_os("LINGXI_F05_LOCAL_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let p = base.join(format!(
        "w05-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p.canonicalize().unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn c02_real_runtime_other_agent_child_has_its_own_persona() {
    let home = own_home("c02");
    let w = WireState {
        sent: Arc::new(Mutex::new(Vec::new())),
        parent_release: Arc::new(tokio::sync::Notify::new()),
        arrived: Arc::new(tokio::sync::Notify::new()),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(
        axum::serve(
            listener,
            axum::Router::new()
                .route("/", post(provider))
                .route("/chat/completions", post(provider))
                .with_state(w.clone()),
        )
        .into_future(),
    );
    let state = boot(&home, &endpoint).await;
    for (id, marker) in [("lingxi", "PERSONA_A"), ("other", "PERSONA_B")] {
        write(
            &home,
            &format!("agents/{id}/config.yaml"),
            &format!("locale: zh-CN\nagent:\n  name: {id}\n"),
        );
        write(&home, &format!("agents/{id}/identity.md"), marker);
        write(
            &home,
            &format!("agents/{id}/memory/memory.md"),
            &format!("PRIVATE_MEMORY_{id}"),
        );
    }
    let parent_state = state.clone();
    let parent = tokio::spawn(async move {
        parent_state
            .sessions()
            .execute_for(
                parent_state.storage().as_ref(),
                parent_state.events(),
                parent_state.runs(),
                &principal(),
                "sess_local_alpha",
                "PARENT_HOLD",
                1790409600000,
            )
            .await
            .unwrap()
    });
    wait_count(&w, 1).await;
    let parent_id = state
        .storage()
        .query_one_text(
            "SELECT run_id FROM runs WHERE session_id=?1 AND status='running'",
            vec!["sess_local_alpha".into()],
        )
        .await
        .unwrap()
        .unwrap();
    let mut children = Vec::new();
    for (index, agent) in ["lingxi", "other"].iter().enumerate() {
        let launched = state
            .subagents()
            .dispatch_child(
                lingxi_service::subagents::ParentRunFacts {
                    principal: lingxi_kernel::Principal::LocalUser,
                    session_id: "sess_local_alpha".into(),
                    agent_id: "lingxi".into(),
                    parent_run_id: RunId::new(parent_id.clone()),
                    parent_scope: state.runs().run_scope(&parent_id).unwrap(),
                    source_model_call: lingxi_kernel::model_call_id(
                        &RunId::new(parent_id.clone()),
                        1,
                    ),
                    cause_tool_call: ToolCallId::new(format!("trusted-runtime-{index}")),
                    now_ms: 1790409600000,
                },
                DelegationRequest {
                    task: format!("CHILD_{agent}"),
                    agent_id: Some((*agent).into()),
                    access: None,
                    label: None,
                    model: None,
                    thread_id: None,
                },
            )
            .await
            .unwrap();
        children.push(launched.child_run_id);
        wait_count(&w, index + 2).await;
    }
    w.parent_release.notify_one();
    parent.await.unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        while state.subagents().active_counts("sess_local_alpha").1 != 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let wire = w.sent.lock().unwrap().clone();
    let a = wire[1]["messages"][0]["content"].as_str().unwrap();
    let b = wire[2]["messages"][0]["content"].as_str().unwrap();
    let current = (
        a.contains("PERSONA_A"),
        b.contains("PERSONA_B"),
        !b.contains("PERSONA_A"),
        !b.contains("PRIVATE_MEMORY_lingxi"),
    );
    if let Ok(dir) = std::env::var("LINGXI_F05_EVIDENCE") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Path::new(&dir).join("c02-wire.json"),serde_json::to_string_pretty(&json!({"wire":wire,"children":children,"parent":parent_id,"assertions":current,"mock":"external loopback only"})).unwrap()).unwrap();
    }
    state.storage().close().await.unwrap();
    server.abort();
    let _ = server.await;
    std::fs::remove_dir_all(home).unwrap();
    assert_eq!(
        current,
        (true, true, true, true),
        "合法不同助手子代理必须获得自己的材料，不能串人格或父私史"
    );
}

use std::future::IntoFuture;

// 屏障只控制实际材料采集与发布的时序；仍调用生产文件源、编译器、网关和存储。
struct AcquisitionBarrier {
    inner: FileContextMaterialSource,
    entered: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}
impl lingxi_service::context_compiler::ContextMaterialSource for AcquisitionBarrier {
    fn publication_epoch(
        &self,
    ) -> Option<Arc<lingxi_adapters::storage::agent_memory::ContextMaterialEpoch>> {
        self.inner.publication_epoch()
    }
    fn gather(
        &self,
        query: &lingxi_service::context_compiler::MaterialQuery,
    ) -> Result<
        lingxi_kernel::context::ContextCompileInput,
        lingxi_service::context_compiler::ContextMaterialError,
    > {
        let input = self.inner.gather(query)?;
        let once = self.entered.lock().unwrap().take();
        if let Some(entered) = once {
            entered.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(20))
                .unwrap();
        }
        Ok(input)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn c04_actual_acquisition_publish_barrier_never_sends_mixed_material() {
    use lingxi_adapters::storage::agent_memory as am;
    let home = own_home("c04");
    write(
        &home,
        "agents/lingxi/config.yaml",
        "locale: zh-CN\nagent:\n  name: 灵犀\nmemory:\n  enabled: true\n",
    );
    write(&home, "agents/lingxi/identity.md", "BARRIER_PERSONA_A");
    write(&home, "agents/lingxi/memory/memory.md", "BARRIER_MEMORY_A");
    let product = home.join("product");
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let source = Arc::new(AcquisitionBarrier {
        inner: FileContextMaterialSource::from_home(
            &home,
            product,
            HostFacts {
                platform: "darwin".into(),
                os_type: "Darwin".into(),
                os_release: "synthetic".into(),
                shell_label: "zsh".into(),
                workspace: home.to_string_lossy().into(),
            },
        ),
        entered: Mutex::new(Some(entered_tx)),
        release: Mutex::new(release_rx),
    });
    let w = WireState {
        sent: Arc::new(Mutex::new(Vec::new())),
        parent_release: Arc::new(tokio::sync::Notify::new()),
        arrived: Arc::new(tokio::sync::Notify::new()),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(
        axum::serve(
            listener,
            axum::Router::new()
                .route("/chat/completions", post(provider))
                .with_state(w.clone()),
        )
        .into_future(),
    );
    let state = boot_with_source(&home, &endpoint, Some(source)).await;
    let first_state = state.clone();
    let first = tokio::spawn(async move {
        first_state
            .sessions()
            .execute_for(
                first_state.storage().as_ref(),
                first_state.events(),
                first_state.runs(),
                &principal(),
                "sess_local_alpha",
                "acquire barrier",
                1790409600000,
            )
            .await
            .unwrap()
    });
    tokio::task::spawn_blocking(move || entered_rx.recv_timeout(Duration::from_secs(20)).unwrap())
        .await
        .unwrap();
    {
        let _publication = am::ContextMaterialMutation::for_home(&home);
        am::atomic_write(&home.join("agents/lingxi/identity.md"), "BARRIER_PERSONA_B").unwrap();
        am::atomic_write(
            &home.join("agents/lingxi/memory/memory.md"),
            "BARRIER_MEMORY_B",
        )
        .unwrap();
    }
    // 第二个真实编译在新代次上完成；第一个随后必须丢弃旧读并重取。
    state
        .sessions()
        .execute_for(
            state.storage().as_ref(),
            state.events(),
            state.runs(),
            &principal(),
            "sess_local_beta",
            "parallel new epoch",
            1790409600000,
        )
        .await
        .unwrap();
    release_tx.send(()).unwrap();
    first.await.unwrap();
    wait_count(&w, 2).await;
    let wire = w.sent.lock().unwrap().clone();
    let mut views = Vec::new();
    for sid in ["sess_local_alpha", "sess_local_beta"] {
        let view = state
            .storage()
            .model_context_observation(sid, None, None, None)
            .await
            .unwrap()
            .unwrap();
        let text = wire.iter().find_map(|request| {
            let text = request["messages"][0]["content"].as_str()?;
            (digest_text(text) == view["renderSha256"]).then_some(text)
        });
        assert!(text.is_some(), "真实观测应绑定实际发送字节");
        views.push(view);
    }
    let valid = wire.iter().all(|r| {
        let text = r["messages"][0]["content"].as_str().unwrap();
        text.contains("BARRIER_PERSONA_B")
            && text.contains("BARRIER_MEMORY_B")
            && !text.contains("BARRIER_PERSONA_A")
            && !text.contains("BARRIER_MEMORY_A")
    });
    if let Ok(dir) = std::env::var("LINGXI_F05_EVIDENCE") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Path::new(&dir).join("c04-wire.json"),serde_json::to_string_pretty(&json!({"wire":wire,"observations":views,"mixedMaterialRejected":valid,"barrier":"after production file acquisition, before revision confirmation/publication"})).unwrap()).unwrap();
    }
    state.storage().close().await.unwrap();
    server.abort();
    let _ = server.await;
    std::fs::remove_dir_all(home).unwrap();
    assert!(valid, "并发发布不得把旧人格与新记忆拼成一个请求");
}

fn digest_text(text: &str) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn c02_actor_owner_and_reader_remain_distinct_on_the_same_session() {
    let home = own_home("actors");
    let w = WireState {
        sent: Arc::new(Mutex::new(Vec::new())),
        parent_release: Arc::new(tokio::sync::Notify::new()),
        arrived: Arc::new(tokio::sync::Notify::new()),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(
        axum::serve(
            listener,
            axum::Router::new()
                .route("/chat/completions", post(provider))
                .with_state(w.clone()),
        )
        .into_future(),
    );
    let state = boot(&home, &endpoint).await;
    write(
        &home,
        "agents/other/config.yaml",
        "locale: zh-CN\nagent:\n  name: other\n",
    );
    write(&home, "agents/other/identity.md", "DEVICE_PERSONA_OLD");
    state
        .session_admin()
        .create_session(
            "f05-owner-b",
            "other",
            "owner_b",
            "synthetic",
            None,
            &[],
            true,
            1790409600000,
        )
        .await
        .unwrap();
    let mut device = principal();
    device.kind = lingxi_service::PrincipalKind::Device;
    device.user_id = Some("owner_b".into());
    device.device_id = Some("device_b".into());
    device.principal_id = "principal_device_device_b".into();
    device.credential_kind = lingxi_service::CredentialKind::DeviceCredential;
    device.trust_state = lingxi_service::TrustState::Lan;
    device.connection_kind = lingxi_service::auth::ConnectionKindSerde::Lan;
    state
        .sessions()
        .execute_for(
            state.storage().as_ref(),
            state.events(),
            state.runs(),
            &device,
            "f05-owner-b",
            "device-owned run",
            1790409600000,
        )
        .await
        .unwrap();
    let first = state
        .storage()
        .model_context_observation("f05-owner-b", None, None, None)
        .await
        .unwrap()
        .unwrap();
    write(&home, "agents/other/identity.md", "LOCAL_PERSONA_NEW");
    // LocalUser 原合同是广域可访问，不能为了缓存身份把合法读取收紧。
    assert!(matches!(
        state
            .sessions()
            .get_for(&principal(), "f05-owner-b")
            .await
            .unwrap(),
        lingxi_service::sessions::SessionAccess::Ok(_)
    ));
    state
        .sessions()
        .execute_for(
            state.storage().as_ref(),
            state.events(),
            state.runs(),
            &principal(),
            "f05-owner-b",
            "local broad owner access",
            1790409600000,
        )
        .await
        .unwrap();
    let second = state
        .storage()
        .model_context_observation("f05-owner-b", None, None, None)
        .await
        .unwrap()
        .unwrap();
    let mut outsider = device.clone();
    outsider.user_id = Some("outsider".into());
    outsider.device_id = Some("other_device".into());
    assert!(matches!(
        state
            .sessions()
            .get_for(&outsider, "f05-owner-b")
            .await
            .unwrap(),
        lingxi_service::sessions::SessionAccess::Forbidden
    ));
    let old = state
        .storage()
        .model_context_observation(
            "f05-owner-b",
            first["runId"].as_str(),
            first["modelCallId"].as_str(),
            first["snapshotId"].as_str(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(old, first);
    assert_eq!(first["actor"]["kind"], "device");
    assert_eq!(first["actor"]["deviceId"], "device_b");
    assert_eq!(second["actor"]["kind"], "local_user");
    assert_eq!(second["actor"]["subject"], "user_local");
    assert_eq!(second["sessionOwnerUserId"], "owner_b");
    assert_eq!(second["agentId"], "other");
    let wire = w.sent.lock().unwrap().clone();
    assert!(wire[0]["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("DEVICE_PERSONA_OLD"));
    assert!(wire[1]["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("LOCAL_PERSONA_NEW"));
    for (request, view) in wire.iter().zip([&first, &second]) {
        assert_eq!(
            digest_text(request["messages"][0]["content"].as_str().unwrap()),
            view["renderSha256"]
        );
    }
    if let Ok(dir) = std::env::var("LINGXI_F05_EVIDENCE") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Path::new(&dir).join("c02-actors-wire.json"), serde_json::to_string_pretty(&json!({"wire":wire,"first":first,"second":second,"localBroadAccess":true,"otherDeviceDenied":true})).unwrap()).unwrap();
    }
    state.storage().close().await.unwrap();
    server.abort();
    let _ = server.await;
    std::fs::remove_dir_all(home).unwrap();
}
