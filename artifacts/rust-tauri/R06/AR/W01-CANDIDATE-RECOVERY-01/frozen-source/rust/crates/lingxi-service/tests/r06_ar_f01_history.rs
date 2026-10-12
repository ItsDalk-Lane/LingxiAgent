//! F01：独立进程、真实认证HTTP/存储/网关；只有外部模型由loopback应答。
use lingxi_test_guard as _;
use std::process::Command;
fn probe(case: &str) {
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/r06_ar_f01_probe.py");
    let result = Command::new("python3")
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_lingxi-service"))
        .arg(case)
        .output()
        .expect("必须具备Python3以执行真实进程探针");
    println!("{}", String::from_utf8_lossy(&result.stdout));
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
#[test]
fn f01_c01_real_authenticated_history_and_isolation() {
    probe("c01");
}
#[test]
fn f01_c02_fork_restart_retry_rewind_source_delete() {
    probe("c02");
}
#[test]
fn f01_c03_projected_tools_opaque_origins_corruption() {
    probe("c03");
}
#[test]
fn f01_c04_cross_run_compaction_failure_restart() {
    probe("c04");
}

#[test]
fn f01_all_five_families_actual_wire_roles() {
    probe("families");
}

#[test]
fn f01_unknown_outcome_actual_crash_wire_diagnostic() {
    probe("uncertain");
}

/// 真实child入口及真实编译器/网关；共享用户材料允许继承，父会话及其他Agent材料不可见。
#[tokio::test]
async fn f01_child_inherits_authorized_material_only() {
    use lingxi_service::subagents::{ParentRunFacts, SubagentLauncher};
    use lingxi_service::{
        prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
    };
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let dir = std::env::temp_dir().join(format!("lingxi-f01-child-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(dir.join("product/yuan")).unwrap();
    std::fs::write(dir.join("product/yuan/lingxi.md"), "AUTHORIZED-YUAN").unwrap();
    std::fs::write(
        dir.join("product/identity.example.md"),
        "AUTHORIZED-IDENTITY",
    )
    .unwrap();
    let home = dir.join("home");
    std::fs::create_dir_all(home.join("user")).unwrap();
    std::fs::write(home.join("user/user.md"), "AUTHORIZED-SHARED-PROFILE").unwrap();
    std::fs::create_dir_all(home.join("agents/other/memory")).unwrap();
    std::fs::write(
        home.join("agents/other/memory/memory.md"),
        "OTHER-AGENT-PRIVATE",
    )
    .unwrap();
    std::fs::write(dir.join("parent-tool.txt"), "MIXED-REAL-READ-CONTENT").unwrap();
    std::fs::write(
        home.join("agents/other/identity.md"),
        "AUTHORIZED-OTHER-IDENTITY",
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let captured = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let capture = captured.clone();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let capture = capture.clone();
            tokio::spawn(async move {
                let mut raw = Vec::new();
                let mut buf = [0u8; 8192];
                let header;
                loop {
                    let n = socket.read(&mut buf).await.unwrap();
                    assert!(n > 0);
                    raw.extend_from_slice(&buf[..n]);
                    if let Some(i) = raw.windows(4).position(|b| b == b"\r\n\r\n") {
                        header = i + 4;
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&raw[..header]);
                let len = text
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|x| x.trim().parse::<usize>().ok())
                    })
                    .unwrap();
                while raw.len() < header + len {
                    let n = socket.read(&mut buf).await.unwrap();
                    assert!(n > 0);
                    raw.extend_from_slice(&buf[..n]);
                }
                let body: serde_json::Value =
                    serde_json::from_slice(&raw[header..header + len]).unwrap();
                let child = body.to_string().contains("EXPLICIT-CHILD-TASK")
                    || body.to_string().contains("DIFFERENT-AGENT-CHILD");
                let messages = body["messages"].as_array().unwrap();
                let current = messages.iter().rposition(|m| m["role"] == "user").unwrap();
                let mixed_tool = (messages[current]["content"] == "MIXED-TOOL-PARENT"
                    || messages[current]["content"] == "OWNER-ALTERNATING-0")
                    && !messages[current + 1..].iter().any(|m| m["role"] == "tool");
                capture.lock().unwrap().push(body);
                let reply = if child {
                    "CHILD-FINAL-PRIVATE"
                } else {
                    "PARENT-ANSWER"
                };
                let data = if mixed_tool {
                    format!(
                        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                        serde_json::json!({"choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"mixed_read_provider","type":"function","function":{"name":"read","arguments":"{\"path\":\"parent-tool.txt\"}"}}]},"finish_reason":null}]}),
                        serde_json::json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":3}})
                    )
                } else {
                    format!(
                        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                        serde_json::json!({"choices":[{"index":0,"delta":{"role":"assistant","content":reply},"finish_reason":null}]}),
                        serde_json::json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":3}})
                    )
                };
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",data.len(),data).as_bytes()).await.unwrap();
            });
        }
    });
    let layout = prepare_layout(&home).unwrap();
    let config = dir.join("config.json");
    std::fs::write(&config,serde_json::json!({"home":home,"workspace":dir,"providers":{"fixture":{"protocol":"openai-completions","endpoint":format!("http://{addr}/v1"),"auth":{"kind":"none"}}},"models":{"chat":{"provider":"fixture","model":"fixture","capabilities":{"tools":true},"compat":{"contextWindow":1000000}}}}).to_string()).unwrap();
    let (source, plane) =
        lingxi_service::config::resolve_model_plane(Some(&config), &layout.runtime_dir)
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
    let state = ServiceState::bootstrap_with_deps(
        ServiceConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            data_home: home.clone(),
            home_source: HomeSource::Cli,
            network_mode: NetworkMode::Loopback,
            shutdown_timeout_ms: 10000,
        },
        &layout,
        ServiceDeps {
            context_compiler: Some(Arc::new(
                lingxi_service::context_compiler::ContextCompilerService::new(Arc::new(
                    lingxi_service::context_compiler::FileContextMaterialSource::from_home(
                        &home,
                        dir.join("product"),
                        lingxi_service::context_compiler::probe_host_facts(Some(&dir), &home),
                    ),
                )),
            )),
            model_gateway: Some(Arc::new(
                lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
            )),
            model_plane_source: Some(source),
            credential_service: Some(credentials),
            workspace_root: Some(dir.clone()),
            ..ServiceDeps::default()
        },
    )
    .await
    .unwrap();
    let principal:lingxi_service::Principal=serde_json::from_value(serde_json::json!({"schemaVersion":1,"principalId":"principal_local","kind":"local_user","userId":lingxi_service::LOCAL_OWNER_USER_ID,"studioId":null,"serverNodeId":null,"deviceId":null,"credentialId":null,"webSessionId":null,"connectionKind":"local","credentialKind":"loopback_token","trustState":"local","scopes":["chat"]})).unwrap();
    async fn wait(state: &ServiceState, id: &str) {
        for _ in 0..500 {
            let v = state
                .storage()
                .query_one_text(
                    "SELECT status FROM runs WHERE run_id=?1",
                    vec![id.to_string()],
                )
                .await
                .unwrap();
            if v.as_deref()
                .is_some_and(|s| !matches!(s, "queued" | "running" | "waiting"))
            {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("真实Run未结束");
    }
    let actual_session = state
        .storage()
        .get_session("sess_local_alpha")
        .await
        .unwrap()
        .unwrap();
    state
        .context_compiler()
        .unwrap()
        .compiled_for_run(
            "sess_local_alpha",
            &actual_session.agent_id,
            false,
            1790409600000,
        )
        .unwrap();
    let history = lingxi_service::model_history::ModelHistory::new(Arc::clone(state.storage()));
    assert!(history
        .load(
            &lingxi_kernel::Principal::WebSession {
                account_id: "unauthorized".into()
            },
            &actual_session.agent_id,
            "sess_local_alpha",
            "nonexistent"
        )
        .await
        .is_err());
    assert!(history
        .load(
            &lingxi_kernel::Principal::LocalUser,
            "other",
            "sess_local_alpha",
            "nonexistent"
        )
        .await
        .is_err());
    let parent = state
        .sessions()
        .execute_for(
            state.storage().as_ref(),
            state.events(),
            state.runs(),
            &principal,
            "sess_local_alpha",
            "PARENT-HISTORY-PRIVATE",
            1790409600000,
        )
        .await
        .unwrap();
    wait(&state, &parent.run_id).await;
    let session = state
        .storage()
        .get_session("sess_local_alpha")
        .await
        .unwrap()
        .unwrap();
    let child = Arc::clone(state.subagents())
        .dispatch(
            ParentRunFacts {
                principal: lingxi_kernel::Principal::LocalUser,
                session_id: session.session_id.clone(),
                agent_id: session.agent_id.clone(),
                parent_run_id: lingxi_protocol::RunId::new(&parent.run_id),
                parent_scope: lingxi_service::CancelScope::run_root(&parent.run_id),
                source_model_call: lingxi_protocol::ModelCallId::new(format!(
                    "{}-mc0001",
                    parent.run_id
                )),
                cause_tool_call: lingxi_protocol::ToolCallId::new(format!(
                    "{}-tc0001",
                    parent.run_id
                )),
                now_ms: 1790409600001,
            },
            lingxi_kernel::ports::DelegationRequest {
                task: "EXPLICIT-CHILD-TASK".into(),
                access: Some(lingxi_kernel::subagent::AccessRequest::Read),
                label: None,
                agent_id: None,
                model: None,
                thread_id: None,
            },
        )
        .await
        .unwrap();
    wait(&state, &child.child_run_id).await;
    let child_wire = captured
        .lock()
        .unwrap()
        .iter()
        .find(|v| v.to_string().contains("EXPLICIT-CHILD-TASK"))
        .unwrap()
        .to_string();
    assert!(child_wire.contains("AUTHORIZED-SHARED-PROFILE"));
    assert!(!child_wire.contains("PARENT-HISTORY-PRIVATE"));
    assert!(!child_wire.contains("OTHER-AGENT-PRIVATE"));
    // 原正式子任务可指定Agent；受信任授权裁剪仍允许，普通伪override则在零wire前拒绝。
    let override_child = Arc::clone(state.subagents())
        .dispatch(
            ParentRunFacts {
                principal: lingxi_kernel::Principal::LocalUser,
                session_id: session.session_id.clone(),
                agent_id: session.agent_id.clone(),
                parent_run_id: lingxi_protocol::RunId::new(&parent.run_id),
                parent_scope: lingxi_service::CancelScope::run_root(&parent.run_id),
                source_model_call: lingxi_protocol::ModelCallId::new(format!(
                    "{}-mc0001",
                    parent.run_id
                )),
                cause_tool_call: lingxi_protocol::ToolCallId::new(format!(
                    "{}-tc0002",
                    parent.run_id
                )),
                now_ms: 1790409600001,
            },
            lingxi_kernel::ports::DelegationRequest {
                task: "DIFFERENT-AGENT-CHILD".into(),
                access: Some(lingxi_kernel::subagent::AccessRequest::Read),
                label: None,
                agent_id: Some("other".into()),
                model: None,
                thread_id: None,
            },
        )
        .await
        .unwrap();
    wait(&state, &override_child.child_run_id).await;
    let override_wire = captured
        .lock()
        .unwrap()
        .iter()
        .find(|v| v.to_string().contains("DIFFERENT-AGENT-CHILD"))
        .unwrap()
        .to_string();
    // EX03已有缓存维度缺陷单独留真实诊断；F01只核可信override受理与历史授权裁剪。
    if let Ok(out) = std::env::var("W01_EVIDENCE") {
        std::fs::write(std::path::Path::new(&out).join("ex03-other-agent-persona-diagnostic.json"),
            serde_json::to_vec_pretty(&serde_json::json!({"status":"OPEN_FAIL_PERSONA_CACHE","rootAuthorization":"F01 excludes persona repair; retain actual final-12 red","requestedAgent":"other","grant":"subagent/read","principal":"local_user","parentRun":parent.run_id,"childRun":override_child.child_run_id,"actualReachedWire":true,"expectedPersonaPresent":override_wire.contains("AUTHORIZED-OTHER-IDENTITY"),"wire":serde_json::from_str::<serde_json::Value>(&override_wire).unwrap()})).unwrap()).unwrap();
    }
    assert!(override_wire.contains("AUTHORIZED-SHARED-PROFILE"));
    assert!(!override_wire.contains("PARENT-HISTORY-PRIVATE"));
    assert!(!override_wire.contains("OTHER-AGENT-PRIVATE"));
    let wires_before_override = captured.lock().unwrap().len();
    assert!(state
        .runs()
        .drive_run(
            state.storage().as_ref(),
            state.events(),
            &lingxi_kernel::Principal::LocalUser,
            "sess_local_alpha",
            "other",
            "forged-agent-override",
            "FORGED-OVERRIDE",
            1,
            1790409600001,
            None,
            None,
            lingxi_service::runs::DriveAuthorization::user_submission(
                None,
                lingxi_kernel::subagent::SessionPermissionMode::Operate
            ),
            None,
            "sess_local_alpha"
        )
        .await
        .is_err());
    assert_eq!(captured.lock().unwrap().len(), wires_before_override);
    assert!(state
        .storage()
        .query_one_text(
            "SELECT run_id FROM runs WHERE run_id=?1",
            vec!["forged-agent-override".into()]
        )
        .await
        .unwrap()
        .is_none());
    let next = state
        .sessions()
        .execute_for(
            state.storage().as_ref(),
            state.events(),
            state.runs(),
            &principal,
            "sess_local_alpha",
            "NEXT-PARENT",
            1790409600002,
        )
        .await
        .unwrap();
    wait(&state, &next.run_id).await;
    // child正文只允许通过既有显式结果交付通道出现，不能冒充父的历史助手回合。
    let last = captured.lock().unwrap().last().unwrap().clone();
    assert!(last.to_string().contains("PARENT-HISTORY-PRIVATE"));
    assert!(!last["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["role"] == "assistant" && m["content"] == "CHILD-FINAL-PRIVATE"));
    // 真旧普通父/child混合fork：删除副本专档模拟旧格式，准确usage谱系排除child。
    state
        .session_tree()
        .fork_session(
            &principal,
            "sess_local_alpha",
            "legacy_child_fork",
            &format!("{}-final", next.run_id),
            false,
            1790409600003,
        )
        .await
        .unwrap();
    let strip_archives = |target: &str| {
        Command::new("python3").arg("-c").arg(r#"
import sqlite3,sys
with sqlite3.connect(sys.argv[1]) as db:
 rows=db.execute('SELECT message_id,parent_message_id,entry_type FROM messages WHERE session_id=? ORDER BY seq',(sys.argv[2],)).fetchall()
 parents={r[0]:r[1] for r in rows};archives={r[0] for r in rows if r[2]=='lingxi-model-exchange-v1'}
 for mid,parent,kind in rows:
  while parent in archives:parent=parents[parent]
  if mid not in archives:db.execute('UPDATE messages SET parent_message_id=? WHERE session_id=? AND message_id=?',(parent,sys.argv[2],mid))
 db.execute("DELETE FROM messages WHERE session_id=? AND entry_type='lingxi-model-exchange-v1'",(sys.argv[2],))
"#).arg(home.join("lingxi-service/data/runs.db")).arg(target).output().unwrap()
    };
    let strip = strip_archives("legacy_child_fork");
    assert!(
        strip.status.success(),
        "{}",
        String::from_utf8_lossy(&strip.stderr)
    );
    let legacy = state
        .sessions()
        .execute_for(
            state.storage().as_ref(),
            state.events(),
            state.runs(),
            &principal,
            "legacy_child_fork",
            "LEGACY-PARENT-CONTINUE",
            1790409600003,
        )
        .await
        .unwrap();
    wait(&state, &legacy.run_id).await;
    let last = captured.lock().unwrap().last().unwrap().clone();
    assert!(last.to_string().contains("PARENT-HISTORY-PRIVATE"));
    assert!(!last.to_string().contains("EXPLICIT-CHILD-TASK"));
    assert!(!last["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["role"] == "assistant" && m["content"] == "CHILD-FINAL-PRIVATE"));
    let restored_legacy = lingxi_service::model_history::ModelHistory::new(state.storage().clone())
        .load(
            &lingxi_kernel::Principal::LocalUser,
            "lingxi",
            "legacy_child_fork",
            "none-current",
        )
        .await
        .unwrap();
    assert!(restored_legacy.iter().any(|item|matches!(item,lingxi_kernel::model_exchange::ExchangeItem::AssistantTurn{call,..} if call.as_str()==format!("{}-mc0001",parent.run_id))));
    // 旧raw-only工具与真实child同一分叉：拒绝缺失结构，不能拼装UI正文伪造父工具回合。
    let mixed = state
        .sessions()
        .execute_for(
            state.storage().as_ref(),
            state.events(),
            state.runs(),
            &principal,
            "sess_local_alpha",
            "MIXED-TOOL-PARENT",
            1790409600003,
        )
        .await
        .unwrap();
    wait(&state, &mixed.run_id).await;
    let started_before = state
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM key_events WHERE event_type='tool_call_started'",
            vec![],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(started_before, "1");
    assert!(captured
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .to_string()
        .contains("MIXED-REAL-READ-CONTENT"));
    state
        .session_tree()
        .fork_session(
            &principal,
            "sess_local_alpha",
            "legacy_mixed_fork",
            &format!("{}-final", mixed.run_id),
            false,
            1790409600003,
        )
        .await
        .unwrap();
    let strip = strip_archives("legacy_mixed_fork");
    assert!(
        strip.status.success(),
        "{}",
        String::from_utf8_lossy(&strip.stderr)
    );
    let wires_before_mixed = captured.lock().unwrap().len();
    let mixed_rejected = state
        .sessions()
        .execute_for(
            state.storage().as_ref(),
            state.events(),
            state.runs(),
            &principal,
            "legacy_mixed_fork",
            "MIXED-LEGACY-CONTINUE",
            1790409600003,
        )
        .await;
    assert!(mixed_rejected.is_err());
    assert_eq!(captured.lock().unwrap().len(), wires_before_mixed);
    assert_eq!(
        state
            .storage()
            .query_one_text(
                "SELECT COUNT(*) FROM key_events WHERE event_type='tool_call_started'",
                vec![]
            )
            .await
            .unwrap()
            .unwrap(),
        started_before
    );
    assert!(state
        .storage()
        .query_one_text(
            "SELECT event_id FROM key_events WHERE run_id=?1 AND event_type='tool_call_completed'",
            vec![mixed.run_id.clone()]
        )
        .await
        .unwrap()
        .is_some());
    // 有效专档的child排除身份随fork保存，源删除不能让child正文冒充父历史。
    state
        .session_tree()
        .fork_session(
            &principal,
            "sess_local_alpha",
            "child_fork",
            &format!("{}-final", next.run_id),
            false,
            1790409600003,
        )
        .await
        .unwrap();
    state
        .session_admin()
        .archive_session(
            "sess_local_alpha",
            Some(lingxi_adapters::storage::session_admin::ChildMode::DetachChildren),
            false,
            &|_| false,
            1790409600004,
        )
        .await
        .unwrap();
    state
        .session_admin()
        .delete_archived_session("sess_local_alpha", false)
        .await
        .unwrap();
    assert!(state
        .storage()
        .get_session("sess_local_alpha")
        .await
        .unwrap()
        .is_none());
    let wires_before_missing = captured.lock().unwrap().len();
    let missing = state
        .sessions()
        .execute_for(
            state.storage().as_ref(),
            state.events(),
            state.runs(),
            &principal,
            "legacy_child_fork",
            "LEGACY-SOURCE-MISSING",
            1790409600005,
        )
        .await;
    assert!(missing.is_err());
    assert_eq!(captured.lock().unwrap().len(), wires_before_missing);
    let after_delete = state
        .sessions()
        .execute_for(
            state.storage().as_ref(),
            state.events(),
            state.runs(),
            &principal,
            "child_fork",
            "AFTER-SOURCE-DELETE",
            1790409600005,
        )
        .await
        .unwrap();
    wait(&state, &after_delete.run_id).await;
    let last = captured.lock().unwrap().last().unwrap().clone();
    assert!(last.to_string().contains("PARENT-HISTORY-PRIVATE"));
    assert!(!last["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["role"] == "assistant" && m["content"] == "CHILD-FINAL-PRIVATE"));
    // 本地主人保留广域访问；其他主体只能读取自己的会话，历史绑定原调用者而非当前读取者。
    lingxi_adapters::storage::session_tree::seed_session(
        state.storage(),
        lingxi_adapters::storage::session_tree::SeedSession {
            session_id: "sess_other_owner",
            agent_id: "lingxi",
            owner_user_id: "user_other",
            title: "合成另主体",
            now_unix_ms: 1790409600003,
            permission_mode: None,
            authorized_folders: vec![],
        },
    )
    .await
    .unwrap();
    let wires_before = captured.lock().unwrap().len();
    for (index, actor) in [
        lingxi_kernel::Principal::LocalUser,
        lingxi_kernel::Principal::WebSession {
            account_id: "user_other".into(),
        },
        lingxi_kernel::Principal::LocalUser,
        lingxi_kernel::Principal::WebSession {
            account_id: "user_other".into(),
        },
    ]
    .into_iter()
    .enumerate()
    {
        let input = format!("OWNER-ALTERNATING-{index}");
        state
            .runs()
            .drive_run(
                state.storage().as_ref(),
                state.events(),
                &actor,
                "sess_other_owner",
                "lingxi",
                &format!("owner-alternating-run-{index}"),
                &input,
                1,
                1790409600003 + index as u64,
                None,
                None,
                lingxi_service::runs::DriveAuthorization::user_submission(
                    None,
                    lingxi_kernel::subagent::SessionPermissionMode::Operate,
                ),
                None,
                "sess_other_owner",
            )
            .await
            .unwrap();
        let wire = captured.lock().unwrap().last().unwrap().clone();
        let messages = wire["messages"].as_array().unwrap();
        for previous in 0..=index {
            assert_eq!(
                messages
                    .iter()
                    .filter(|m| m["role"] == "user"
                        && m["content"] == format!("OWNER-ALTERNATING-{previous}"))
                    .count(),
                1
            );
        }
        assert!(!wire.to_string().contains("PARENT-HISTORY-PRIVATE"));
    }
    assert_eq!(captured.lock().unwrap().len(), wires_before + 5);
    let archives = lingxi_adapters::storage::session_tree::list_all_messages(
        state.storage(),
        "sess_other_owner",
    )
    .await
    .unwrap();
    for row in archives
        .iter()
        .filter(|row| row.entry_type == "lingxi-model-exchange-v1")
    {
        let body: serde_json::Value = serde_json::from_str(&row.content_json).unwrap();
        assert_eq!(body["owner"], "user_other");
        assert_eq!(body["boundSession"], "sess_other_owner");
        assert_eq!(body["sourceSession"], "sess_other_owner");
        let source = body["sourceRun"].as_str().unwrap();
        let actor = state.storage().query_one_text("SELECT json_object('kind',owner_kind,'subject',owner_subject) FROM runs WHERE run_id=?1", vec![source.into()]).await.unwrap().unwrap();
        let actor: serde_json::Value = serde_json::from_str(&actor).unwrap();
        assert_eq!(body["actorKind"], actor["kind"]);
        assert_eq!(body["actorSubject"], actor["subject"]);
    }
    // fork保留双方原执行身份；源删除后完整专档仍自足，模型只回放受限工具投影。
    state
        .session_tree()
        .fork_session(
            &principal,
            "sess_other_owner",
            "owner_actor_fork",
            "owner-alternating-run-3-final",
            false,
            1790409600010,
        )
        .await
        .unwrap();
    let web_owner = lingxi_kernel::Principal::WebSession {
        account_id: "user_other".into(),
    };
    let drive_owner_fork = |run: &'static str, input: &'static str| {
        state.runs().drive_run(
            state.storage().as_ref(),
            state.events(),
            &web_owner,
            "owner_actor_fork",
            "lingxi",
            run,
            input,
            1,
            1790409600011,
            None,
            None,
            lingxi_service::runs::DriveAuthorization::user_submission(
                None,
                lingxi_kernel::subagent::SessionPermissionMode::Operate,
            ),
            None,
            "owner_actor_fork",
        )
    };
    drive_owner_fork("owner-fork-first", "OWNER-FORK-AFTER-ALTERNATION")
        .await
        .unwrap();
    let original_wire = captured.lock().unwrap().last().unwrap().clone();
    assert_eq!(
        original_wire["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["role"] == "tool")
            .count(),
        1
    );
    assert!(original_wire
        .to_string()
        .contains("MIXED-REAL-READ-CONTENT"));
    let rows = lingxi_adapters::storage::session_tree::list_all_messages(
        state.storage(),
        "owner_actor_fork",
    )
    .await
    .unwrap();
    let archive = rows
        .iter()
        .find(|r| r.message_id == "model-history:owner-alternating-run-0:1")
        .unwrap();
    let mut forged: serde_json::Value = serde_json::from_str(&archive.content_json).unwrap();
    forged["actorKind"] = serde_json::json!("web_session");
    forged["actorSubject"] = serde_json::json!("user_other");
    forged.as_object_mut().unwrap().remove("digest");
    forged["digest"] = serde_json::json!(lingxi_protocol::canon::sha256_hex(
        &lingxi_protocol::canon::canonical_json_bytes(&forged)
    ));
    let change_archive = |content: &str| {
        let result = Command::new("python3").arg("-c").arg(
            r#"import sqlite3,sys; db=sqlite3.connect(sys.argv[1]); db.execute("UPDATE messages SET content_json=?1 WHERE session_id='owner_actor_fork' AND message_id=?2",(sys.argv[2],sys.argv[3])); db.commit()"#)
            .arg(state.storage().db_path()).arg(content).arg(&archive.message_id).status().unwrap();
        assert!(result.success());
    };
    change_archive(&forged.to_string());
    let wire_count = captured.lock().unwrap().len();
    assert!(
        drive_owner_fork("owner-forged-actor", "FORGED-ORIGINAL-ACTOR")
            .await
            .is_err()
    );
    assert_eq!(captured.lock().unwrap().len(), wire_count);
    assert_eq!(state.storage().query_one_text("SELECT content_json FROM messages WHERE session_id='owner_actor_fork' AND message_id=?1", vec![archive.message_id.clone()]).await.unwrap().unwrap(), forged.to_string());
    change_archive(&archive.content_json);
    state
        .session_admin()
        .archive_session(
            "sess_other_owner",
            Some(lingxi_adapters::storage::session_admin::ChildMode::DetachChildren),
            false,
            &|_| false,
            1790409600012,
        )
        .await
        .unwrap();
    state
        .session_admin()
        .delete_archived_session("sess_other_owner", false)
        .await
        .unwrap();
    drive_owner_fork("owner-source-deleted", "OWNER-SOURCE-DELETED")
        .await
        .unwrap();
    let after_delete = captured.lock().unwrap().last().unwrap().clone();
    assert_eq!(
        after_delete["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["role"] == "tool")
            .count(),
        1
    );
    assert!(after_delete.to_string().contains("MIXED-REAL-READ-CONTENT"));
    for (index, actor) in [
        lingxi_kernel::Principal::WebSession {
            account_id: "foreign_user".into(),
        },
        lingxi_kernel::Principal::Device {
            device_id: "foreign-device".into(),
            user_id: "foreign_user".into(),
        },
    ]
    .into_iter()
    .enumerate()
    {
        let count = captured.lock().unwrap().len();
        let run = format!("foreign-owner-run-{index}");
        let before = lingxi_adapters::storage::session_tree::list_all_messages(
            state.storage(),
            "owner_actor_fork",
        )
        .await
        .unwrap();
        let wrong = state
            .runs()
            .drive_run(
                state.storage().as_ref(),
                state.events(),
                &actor,
                "owner_actor_fork",
                "lingxi",
                &run,
                "FOREIGN-OWNER-TASK",
                1,
                1790409600010,
                None,
                None,
                lingxi_service::runs::DriveAuthorization::user_submission(
                    None,
                    lingxi_kernel::subagent::SessionPermissionMode::Operate,
                ),
                None,
                "owner_actor_fork",
            )
            .await;
        assert!(wrong.is_err());
        assert_eq!(captured.lock().unwrap().len(), count);
        assert!(state
            .storage()
            .query_one_text("SELECT run_id FROM runs WHERE run_id=?1", vec![run])
            .await
            .unwrap()
            .is_none());
        let after = lingxi_adapters::storage::session_tree::list_all_messages(
            state.storage(),
            "owner_actor_fork",
        )
        .await
        .unwrap();
        assert_eq!(before, after);
    }
    if let Ok(out) = std::env::var("W01_EVIDENCE") {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(
            std::path::Path::new(&out).join("child-actual-wire.json"),
            serde_json::to_vec_pretty(&*captured.lock().unwrap()).unwrap(),
        )
        .unwrap();
    }
    server.abort();
    let _ = server.await;
    state.storage().close().await.unwrap();
    if let Ok(out) = std::env::var("W01_EVIDENCE") {
        std::fs::copy(
            home.join("lingxi-service/data/runs.db"),
            std::path::Path::new(&out).join("child-actual.db"),
        )
        .unwrap();
    }
    drop(state);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// 真SQLite事务并发正反控：只有已持久child谱系追加可以重新接头。
#[tokio::test]
async fn f01_archive_child_append_race_and_reset_foreign_writer_controls() {
    use lingxi_adapters::storage::{session_tree as tree, RunDatabase, StoreOptions};
    use lingxi_kernel::ports::{RunOutcome, StoragePort};
    use lingxi_kernel::{Principal, RunContext};
    use lingxi_protocol::{
        AttemptId, ContentBlock, NormalizedMessage, RunId, RunStatus, SessionId,
    };
    let home = std::env::temp_dir().join(format!("lingxi-f01-race-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let db = Arc::new(
        RunDatabase::open(&home.join("runs.db"), StoreOptions::default())
            .await
            .unwrap(),
    );
    use std::sync::Arc;
    tree::seed_session(
        &db,
        tree::SeedSession {
            session_id: "race",
            agent_id: "lingxi",
            owner_user_id: "user_local",
            title: "合成并发",
            now_unix_ms: 1,
            permission_mode: None,
            authorized_folders: vec![],
        },
    )
    .await
    .unwrap();
    let ctx = RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new("race"),
        run_id: RunId::new("parent"),
        attempt: AttemptId::new("parent#a1"),
        generation: 1,
    };
    db.record_run_started(&ctx, 1).await.unwrap();
    tree::append_message(
        &db,
        tree::AppendMessage {
            session_id: "race",
            message_id: "user:parent",
            parent_message_id: None,
            run_id: None,
            role: "user",
            content_json: r#"{"text":"根"}"#,
            entry_type: "message",
            model_call_id: None,
            now_unix_ms: 1,
        },
    )
    .await
    .unwrap();
    let history = lingxi_service::model_history::ModelHistory::new(db.clone());
    let items = [lingxi_kernel::model_exchange::ExchangeItem::UserTurn {
        text: "合法父投影".into(),
    }];
    let mut observations = Vec::new();
    for i in 1..=20 {
        let child = RunContext {
            run_id: RunId::new(format!("child{i}")),
            attempt: AttemptId::new(format!("child{i}#a1")),
            ..ctx.clone()
        };
        db.record_run_started(&child, 2).await.unwrap();
        db.record_run_lineage(
            &child,
            lingxi_kernel::subagent::RunLineage {
                parent_run_id: Some(ctx.run_id.clone()),
                origin: lingxi_kernel::subagent::RunOrigin::Subagent,
                source_message_id: Some("parent-mc0001".into()),
                cause_id: Some(format!("parent-tc{i}")),
            },
            2,
        )
        .await
        .unwrap();
        let outcome = RunOutcome {
            status: RunStatus::Completed,
            reason: Some("completed.with_final".into()),
            key_events: vec![],
            final_message: Some(NormalizedMessage {
                role: "assistant".into(),
                content: vec![ContentBlock::Text {
                    text: format!("child{i}正文"),
                }],
                model_call_id: None,
            }),
        };
        let (saved, finished) = tokio::join!(
            history.record(&ctx, "lingxi", &items, false, i, 3, None),
            db.commit_run_outcome(&child, outcome, 3)
        );
        saved.unwrap();
        finished.unwrap();
        let chain = tree::list_branch_messages(&db, "race").await.unwrap();
        let a = chain
            .iter()
            .position(|r| r.message_id == format!("model-history:parent:{i}"))
            .unwrap();
        let c = chain
            .iter()
            .position(|r| r.message_id == format!("child{i}-final"))
            .unwrap();
        observations.push(serde_json::json!({"sequence":i,"archiveIndex":a,"childIndex":c}));
    }
    assert!(observations
        .iter()
        .any(|o| o["archiveIndex"].as_u64() > o["childIndex"].as_u64()));
    let (saved, reset) = tokio::join!(
        history.record(&ctx, "lingxi", &items, false, 21, 4, None),
        tree::reset_branch_head(
            &db,
            tree::ResetBranchHead {
                session_id: "race",
                new_head_message_id: Some("user:parent"),
                reason: "真实反控reset",
                source_message_id: None,
                now_unix_ms: 4
            }
        )
    );
    reset.unwrap();
    assert!(saved.is_err());
    let head = tree::list_branch_messages(&db, "race")
        .await
        .unwrap()
        .last()
        .unwrap()
        .message_id
        .clone();
    let (saved, foreign) = tokio::join!(
        history.record(&ctx, "lingxi", &items, false, 22, 5, None),
        tree::append_message(
            &db,
            tree::AppendMessage {
                session_id: "race",
                message_id: "foreign",
                parent_message_id: Some(&head),
                run_id: None,
                role: "user",
                content_json: r#"{"text":"非child写者"}"#,
                entry_type: "message",
                model_call_id: None,
                now_unix_ms: 5
            }
        )
    );
    foreign.unwrap();
    assert!(saved.is_err());
    assert!(db.query_one_text("SELECT message_id FROM messages WHERE message_id IN ('model-history:parent:21','model-history:parent:22')",vec![]).await.unwrap().is_none());
    db.close().await.unwrap();
    if let Ok(out) = std::env::var("W01_EVIDENCE") {
        std::fs::copy(
            home.join("runs.db"),
            std::path::Path::new(&out).join("child-race-actual.db"),
        )
        .unwrap();
        std::fs::write(
            std::path::Path::new(&out).join("child-race-observations.json"),
            serde_json::to_vec_pretty(&observations).unwrap(),
        )
        .unwrap();
    }
    std::fs::remove_dir_all(home).unwrap();
}
