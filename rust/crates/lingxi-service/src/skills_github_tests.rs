//! 真HTTP归档替身只替换传输地址；开关/解压/审查/安装/网关均执行生产代码。
use super::*;
use crate::{
    cancel::CancelScope,
    skills::{SkillGuardPort, SkillReviewOutcome},
};
use lingxi_kernel::RunContext;
use std::sync::Mutex;
use std::{
    io::Write,
    sync::atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
struct Review {
    result: SkillReviewOutcome,
    calls: AtomicUsize,
    delayed: bool,
}
impl SkillGuardPort for Review {
    fn review<'a>(
        &'a self,
        _: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = SkillReviewOutcome> + Send + 'a>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.delayed {
                std::future::pending::<()>().await;
            }
            self.result.clone()
        })
    }
}
fn archive(name: &str, path: &str) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (file, bytes) in [
        (
            format!("repo-HEAD/{path}/SKILL.md"),
            format!("---\nname: {name}\ndescription: 测试技能\n---\n处理测试输入。\n").into_bytes(),
        ),
        (
            format!("repo-HEAD/{path}/references/readme.txt"),
            b"references-exact-payload".to_vec(),
        ),
        (
            format!("repo-HEAD/{path}/scripts/helper.py"),
            b"print('fixture only')\n".to_vec(),
        ),
    ] {
        writer
            .start_file(file, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
struct HttpFixture {
    url: String,
    count: Arc<AtomicUsize>,
    body: Arc<Mutex<Vec<u8>>>,
    status: Arc<AtomicUsize>,
    delay: Arc<AtomicUsize>,
    paths: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for HttpFixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl HttpFixture {
    async fn new(bytes: Vec<u8>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let body = Arc::new(Mutex::new(bytes));
        let status = Arc::new(AtomicUsize::new(200));
        let delay = Arc::new(AtomicUsize::new(0));
        let paths = Arc::new(Mutex::new(Vec::new()));
        let (c, b, s, d, p) = (
            count.clone(),
            body.clone(),
            status.clone(),
            delay.clone(),
            paths.clone(),
        );
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let (c, b, s, d, p) = (c.clone(), b.clone(), s.clone(), d.clone(), p.clone());
                tokio::spawn(async move {
                    let mut request = [0; 16384];
                    let n = socket.read(&mut request).await.unwrap();
                    let raw = String::from_utf8_lossy(&request[..n]);
                    assert!(!raw.to_lowercase().contains("authorization:"));
                    p.lock().unwrap().push(raw.lines().next().unwrap().into());
                    c.fetch_add(1, Ordering::SeqCst);
                    if d.load(Ordering::SeqCst) > 0 {
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    }
                    let bytes = b.lock().unwrap().clone();
                    let status = s.load(Ordering::SeqCst);
                    let response=format!("HTTP/1.1 {status} Test\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n",bytes.len(),if status==302 {"Location: http://127.0.0.1:9/private\r\n"}else{""});
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.write_all(&bytes).await;
                });
            }
        });
        Self {
            url,
            count,
            body,
            status,
            delay,
            paths,
            task,
        }
    }
}
struct Fixture {
    home: PathBuf,
    tools: MemoryTools,
    skills: Arc<SkillService>,
    http: HttpFixture,
}
impl Fixture {
    async fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-github-install-{}",
            am::random_token_suffix()
        ));
        std::fs::create_dir_all(home.join("user")).unwrap();
        for agent in ["agentA", "agentB"] {
            std::fs::create_dir_all(home.join("agents").join(agent)).unwrap();
            std::fs::write(
                home.join("agents").join(agent).join("config.yaml"),
                "agent:\n  name: 合成助手\nskills:\n  enabled: []\n",
            )
            .unwrap();
        }
        let memory = Arc::new(MemoryService::new(home.clone(), home.join("runtime")));
        let skills = Arc::new(SkillService::new(home.clone(), home.join("product")));
        let http = HttpFixture::new(archive("github-valid", "skills/test")).await;
        let guard = lingxi_adapters::models::egress::EgressGuard::from_provider_endpoints(
            std::slice::from_ref(&http.url),
        )
        .unwrap();
        skills.set_github_test_transport(http.url.clone(), Arc::new(guard));
        let tools = MemoryTools {
            memory,
            skills: skills.clone(),
            agent_id: "agentA".into(),
            clock: Arc::new(crate::inject::SystemClock),
        };
        let value = Self {
            home,
            tools,
            skills,
            http,
        };
        value.flags(true, true, true);
        value
    }
    fn flags(&self, enabled: bool, github: bool, safety: bool) {
        std::fs::write(self.home.join("user/preferences.json"),serde_json::to_vec(&serde_json::json!({"learn_skills":{"enabled":enabled,"allow_github_fetch":github,"safety_review":safety}})).unwrap()).unwrap();
    }
    fn cancel() -> CancelScope {
        CancelScope::background_root("github-install-proof")
    }
    async fn call(&self, args: Value) -> ToolOutcome {
        self.tools.run_install_skill(&args, &Self::cancel()).await
    }
    fn arg() -> Value {
        serde_json::json!({"github_url":"https://github.com/owner/repo/tree/main/skills/test","reason":"测试完整包"})
    }
    fn no_install(&self) {
        assert!(!self.home.join("skills/github-valid").exists());
        assert!(!self.home.join("user/skills/github-valid").exists());
        assert!(!self.skills.skill_exists("github-valid"));
    }
    fn reviewed(&self, result: SkillReviewOutcome, delayed: bool) -> Arc<Review> {
        let guard = Arc::new(Review {
            result,
            calls: AtomicUsize::new(0),
            delayed,
        });
        self.skills.set_guard(guard.clone());
        guard
    }
}
fn text(outcome: &ToolOutcome) -> String {
    match outcome {
        ToolOutcome::Success { result } => result
            .content
            .iter()
            .filter_map(|v| {
                if let ContentBlock::Text { text } = v {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        ToolOutcome::Failed { error } => error.message.clone(),
        other => format!("{other:?}"),
    }
}
fn failed(outcome: &ToolOutcome) {
    assert!(matches!(outcome, ToolOutcome::Failed { .. }), "{outcome:?}");
}
#[tokio::test]
async fn github_real_http_flags_full_package_review_and_only_current_agent() {
    let f = Fixture::new().await;
    let before = std::fs::read(f.home.join("agents/agentB/config.yaml")).unwrap();
    f.flags(false, true, true);
    failed(&f.call(Fixture::arg()).await);
    f.flags(true, false, true);
    failed(&f.call(Fixture::arg()).await);
    assert_eq!(f.http.count.load(Ordering::SeqCst), 0);
    f.no_install();
    f.flags(true, true, true);
    let guard = f.reviewed(SkillReviewOutcome::Safe, false);
    let result = f.call(Fixture::arg()).await;
    assert!(
        text(&result).contains("enabled for this agent"),
        "{result:?}"
    );
    assert_eq!(guard.calls.load(Ordering::SeqCst), 1);
    assert_eq!(f.http.count.load(Ordering::SeqCst), 1);
    let dir = f.skills.skills_dir().join("github-valid");
    assert_eq!(
        std::fs::read(dir.join("references/readme.txt")).unwrap(),
        b"references-exact-payload"
    );
    assert_eq!(
        std::fs::read(dir.join("scripts/helper.py")).unwrap(),
        b"print('fixture only')\n"
    );
    assert!(std::fs::read_to_string(dir.join("SKILL.md"))
        .unwrap()
        .contains("github-valid"));
    assert!(f
        .skills
        .runtime_skill_catalog("agentA")
        .unwrap()
        .iter()
        .any(|entry| entry.name == "github-valid"));
    assert!(!f
        .skills
        .runtime_skill_catalog("agentB")
        .unwrap()
        .iter()
        .any(|entry| entry.name == "github-valid"));
    let current = std::fs::read_to_string(f.home.join("agents/agentA/config.yaml")).unwrap();
    assert!(current.contains("github-valid"));
    assert_eq!(
        std::fs::read(f.home.join("agents/agentB/config.yaml")).unwrap(),
        before
    );
    assert!(std::fs::read_to_string(dir.join("SKILL.md"))
        .unwrap()
        .contains("false"));
    std::fs::write(f.home.join("proof.json"),serde_json::to_vec_pretty(&serde_json::json!({"source":"github","requestCount":1,"guardCount":1,"paths":*f.http.paths.lock().unwrap(),"otherAgentUnchanged":true,"package":dir,"outcome":text(&result)})).unwrap()).unwrap();
    println!("GITHUB_REAL_HTTP_PROOF {}", f.home.display());
}
#[tokio::test]
async fn github_review_rejection_token_changed_package_failure_and_cancellation_zero_install() {
    let f = Fixture::new().await;
    f.reviewed(
        SkillReviewOutcome::Suspicious("explicit-risk".into()),
        false,
    );
    let first = f.call(Fixture::arg()).await;
    assert!(text(&first).contains("Safety review FAILED"));
    f.no_install();
    let token = text(&first)
        .split("risk_confirmation_token=")
        .nth(1)
        .unwrap()
        .trim_end_matches('.')
        .to_string();
    let mut accept = Fixture::arg();
    accept["risk_accepted"] = Value::Bool(true);
    accept["risk_confirmation_token"] = Value::String(token);
    let accepted = f.call(accept).await;
    assert!(
        text(&accepted).contains("explicit user confirmation"),
        "{accepted:?}"
    );
    assert!(f.skills.skill_exists("github-valid"));
    let changed = Fixture::new().await;
    changed.reviewed(
        SkillReviewOutcome::Suspicious("explicit-risk".into()),
        false,
    );
    let original = changed.call(Fixture::arg()).await;
    let token = text(&original)
        .split("risk_confirmation_token=")
        .nth(1)
        .unwrap()
        .trim_end_matches('.')
        .to_string();
    *changed.http.body.lock().unwrap() = archive("changed-skill", "skills/test");
    let mut mismatch = Fixture::arg();
    mismatch["risk_accepted"] = Value::Bool(true);
    mismatch["risk_confirmation_token"] = Value::String(token);
    assert!(text(&changed.call(mismatch).await).contains("Safety review FAILED"));
    changed.no_install();
    assert!(!changed.skills.skill_exists("changed-skill"));

    let f = Fixture::new().await;
    f.reviewed(SkillReviewOutcome::Safe, false);
    f.http.status.store(302, Ordering::SeqCst);
    failed(&f.call(Fixture::arg()).await);
    f.no_install();
    assert_eq!(f.http.count.load(Ordering::SeqCst), 1);
    f.http.status.store(200, Ordering::SeqCst);
    *f.http.body.lock().unwrap() = b"invalid zip".to_vec();
    failed(&f.call(Fixture::arg()).await);
    f.no_install();
    *f.http.body.lock().unwrap() = archive("github-valid", "skills/test");
    f.http.delay.store(1, Ordering::SeqCst);
    let cancel = Fixture::cancel();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        trigger.cancel("test download cancel");
    });
    let result = f.tools.run_install_skill(&Fixture::arg(), &cancel).await;
    assert!(matches!(result,ToolOutcome::Failed{error}if error.code==ErrorCode::Cancelled));
    f.no_install();
    f.http.delay.store(0, Ordering::SeqCst);
    let guard = f.reviewed(SkillReviewOutcome::Safe, true);
    let cancel = Fixture::cancel();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        while guard.calls.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
        trigger.cancel("test review cancel");
    });
    failed(&f.tools.run_install_skill(&Fixture::arg(), &cancel).await);
    f.no_install();
}
#[tokio::test]
async fn github_invalid_sources_and_skipped_review_local_zip_skill_preserved() {
    let f = Fixture::new().await;
    for url in [
        "http://github.com/owner/repo",
        "https://github.com.evil/owner/repo",
        "https://github.com/owner/repo/tree/main/../escape",
        "https://github.com/owner/repo?secret=x",
        "https://github.com/a@b/repo",
        "https://github.com/o/r/tree/main/%2e%2e/secret",
    ] {
        let mut args = Fixture::arg();
        args["github_url"] = Value::String(url.into());
        failed(&f.call(args).await);
    }
    assert_eq!(f.http.count.load(Ordering::SeqCst), 0);
    f.no_install();
    f.flags(true, true, false);
    let guard = f.reviewed(
        SkillReviewOutcome::Suspicious("should-not-run".into()),
        false,
    );
    let result = f.call(Fixture::arg()).await;
    assert!(text(&result).contains("disabled by setting"));
    assert_eq!(guard.calls.load(Ordering::SeqCst), 0);
    for (name, extension) in [("local-zip", "zip"), ("local-skill", "skill")] {
        let path = f.home.join(format!("{name}.{extension}"));
        std::fs::write(&path, archive(name, "")).unwrap();
        let result = f
            .call(serde_json::json!({"local_path":path,"reason":"本地完整包"}))
            .await;
        assert!(
            text(&result).contains("enabled for this agent"),
            "{result:?}"
        );
        assert!(f.skills.skill_exists(name));
    }
}
#[tokio::test]
async fn github_actual_public_host_obeys_current_dns_refusal_without_install() {
    let f = Fixture::new().await;
    f.skills.clear_github_test_transport();
    f.flags(true, true, false);
    let result=f.call(serde_json::json!({"github_url":"https://github.com/anthropics/skills/tree/main/skills/skill-creator","reason":"实际公网保护核验"})).await;
    let report = text(&result);
    println!("GITHUB_PUBLIC_RUNTIME {}", report);
    std::fs::write(f.home.join("public-attempt.json"),serde_json::to_vec_pretty(&serde_json::json!({"outcome":report,"installed":f.skills.skill_exists("skill-creator")})).unwrap()).unwrap();
    if matches!(result, ToolOutcome::Failed { .. }) {
        assert!(!f.skills.skill_exists("skill-creator"));
    } else {
        assert!(f.skills.skill_exists("skill-creator"));
    }
}

#[derive(Default)]
struct Provider {
    script: Mutex<std::collections::VecDeque<lingxi_kernel::ports::ProviderTurn>>,
    outcomes: Mutex<Vec<ToolOutcome>>,
}
impl lingxi_kernel::ports::TurnProviderPort for Provider {
    fn descriptor(&self) -> lingxi_kernel::ports::ProviderDescriptor {
        lingxi_kernel::ports::ProviderDescriptor {
            provider: "fixture".into(),
            model: "fixture".into(),
            operation: "chat".into(),
        }
    }
    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        _: &'a lingxi_protocol::ModelCallId,
        input: &'a lingxi_kernel::model_exchange::ModelTurnInput,
        _: &'a dyn lingxi_kernel::ports::TurnDeltaSink,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = lingxi_kernel::ports::ProviderTurnResult> + Send + 'a>,
    > {
        if let Some(lingxi_kernel::model_exchange::ExchangeItem::ToolResult { outcome, .. }) =
            input.prior.last()
        {
            self.outcomes.lock().unwrap().push(outcome.clone());
        }
        let result = self
            .script
            .lock()
            .unwrap()
            .pop_front()
            .expect("真实驱动须消费完整脚本");
        let context = ctx.clone();
        Box::pin(async move { lingxi_kernel::ports::ProviderTurnResult::of_ctx(&context, result) })
    }
}
impl Provider {
    fn queue(&self) {
        self.script.lock().unwrap().extend([
            lingxi_kernel::ports::ProviderTurn::ToolRequests {
                content: Vec::new(),
                requests: vec![lingxi_kernel::ports::ToolRequest::from_effective_arguments(
                    "tool:first-party:mcp_describe_tool",
                    serde_json::json!({"name":"install_skill"}),
                    &SchemaBudget::default(),
                )
                .unwrap()],
            },
            lingxi_kernel::ports::ProviderTurn::ToolRequests {
                content: Vec::new(),
                requests: vec![lingxi_kernel::ports::ToolRequest::from_effective_arguments(
                    "tool:first-party:mcp_call",
                    serde_json::json!({"tool":"install_skill","arguments":Fixture::arg()}),
                    &SchemaBudget::default(),
                )
                .unwrap()],
            },
            lingxi_kernel::ports::ProviderTurn::Final {
                message: lingxi_protocol::NormalizedMessage {
                    role: "assistant".into(),
                    content: vec![ContentBlock::Text {
                        text: "审查完成".into(),
                    }],
                    model_call_id: None,
                },
            },
        ]);
    }
}
fn owner() -> crate::Principal {
    crate::Principal {
        schema_version: 1,
        principal_id: "principal_local".into(),
        kind: crate::PrincipalKind::LocalUser,
        user_id: Some(crate::LOCAL_OWNER_USER_ID.into()),
        studio_id: None,
        server_node_id: None,
        device_id: None,
        credential_id: None,
        web_session_id: None,
        connection_kind: crate::auth::ConnectionKindSerde::Local,
        credential_kind: crate::CredentialKind::LoopbackToken,
        trust_state: crate::TrustState::Local,
        scopes: vec!["chat".into()],
    }
}
fn approval_ctx() -> RunContext {
    RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("github-approval"),
        run_id: lingxi_protocol::RunId::new("approval-reader"),
        attempt: lingxi_protocol::AttemptId::new("approval-reader#a1"),
        generation: 1,
    }
}
#[tokio::test]
async fn github_registered_real_driver_requires_review_rejects_readonly_and_never_fetches_before_approval(
) {
    use crate::{
        approval_service::{Answer, AnswerOutcome, ApprovalService},
        inject::SystemClock,
        toolgateway::ToolInvocationGateway,
    };
    let f = Fixture::new().await;
    f.flags(true, true, false);
    let clock = Arc::new(SystemClock);
    let registry = Arc::new(ToolRegistry::new());
    let approvals = Arc::new(ApprovalService::with_bounds(clock.clone(), 8000, 64, 64));
    let gateway = Arc::new(ToolInvocationGateway::new(
        registry.clone(),
        approvals.clone(),
        clock.clone(),
        SchemaBudget::default(),
        30000,
        64,
    ));
    register_memory_tools(
        &registry,
        &gateway,
        f.tools.memory.clone(),
        f.skills.clone(),
        "agentA".into(),
        clock,
        &SchemaBudget::default(),
    );
    // 此验证使用完整目录流程，宿主注入工具面时也必须显式登记目录。
    crate::toolcatalog_bridge::register(&registry, &gateway, &SchemaBudget::default());
    let provider = Arc::new(Provider::default());
    let layout = crate::prepare_layout(&f.home).unwrap();
    let state = crate::ServiceState::bootstrap_with_deps(
        crate::ServiceConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            data_home: f.home.clone(),
            home_source: crate::HomeSource::Cli,
            network_mode: crate::NetworkMode::Loopback,
            shutdown_timeout_ms: 5000,
        },
        &layout,
        crate::ServiceDeps {
            turn_provider: Some(provider.clone()),
            tool_gateway: Some(gateway),
            approval_gate: Some(approvals.clone()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    state
        .session_admin()
        .create_session(
            "github-approval",
            "agentA",
            crate::LOCAL_OWNER_USER_ID,
            "安装技能",
            Some("operate"),
            &[],
            false,
            1790409600000,
        )
        .await
        .unwrap();
    for (index, answer) in [
        Answer::Reject {
            reason: "拒绝安装".into(),
        },
        Answer::Approve,
    ]
    .into_iter()
    .enumerate()
    {
        provider.queue();
        let runtime = state.clone();
        let task = tokio::spawn(async move {
            runtime
                .sessions()
                .execute_for(
                    runtime.storage().as_ref(),
                    runtime.events(),
                    runtime.runs(),
                    &owner(),
                    "github-approval",
                    "安装GitHub完整技能包",
                    1790409600000 + index as u64,
                )
                .await
                .unwrap()
        });
        let view = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Some(view) = approvals
                    .pending_of(&approval_ctx(), "github-approval")
                    .first()
                {
                    break view.clone();
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("operate模式仍必须实际确认");
        assert_eq!(f.http.count.load(Ordering::SeqCst), 0);
        f.no_install();
        assert!(matches!(
            approvals.answer(
                &approval_ctx(),
                "github-approval",
                &view.approval_id,
                answer
            ),
            AnswerOutcome::Settled(_)
        ));
        tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        if index == 0 {
            assert_eq!(f.http.count.load(Ordering::SeqCst), 0);
            f.no_install();
        } else {
            assert_eq!(f.http.count.load(Ordering::SeqCst), 1);
            assert!(f.skills.skill_exists("github-valid"));
        }
    }
    state
        .sessions()
        .set_permission_mode_for(
            &owner(),
            "github-approval",
            lingxi_kernel::subagent::SessionPermissionMode::ReadOnly,
        )
        .await
        .unwrap();
    provider.queue();
    state
        .sessions()
        .execute_for(
            state.storage().as_ref(),
            state.events(),
            state.runs(),
            &owner(),
            "github-approval",
            "只读不允许安装",
            1790409600003,
        )
        .await
        .unwrap();
    assert_eq!(f.http.count.load(Ordering::SeqCst), 1);
    assert!(matches!(
        provider.outcomes.lock().unwrap().last(),
        Some(ToolOutcome::Failed { .. })
    ));
    std::fs::write(f.home.join("driver-proof.json"),serde_json::to_vec_pretty(&serde_json::json!({"operateRequiresActualApproval":true,"rejectDownloadCount":0,"approvedDownloadCount":1,"readOnlyAdditionalDownloadCount":0,"outcomes":provider.outcomes.lock().unwrap().iter().map(text).collect::<Vec<_>>()})).unwrap()).unwrap();
    println!("GITHUB_REGISTERED_DRIVER_PROOF {}", f.home.display());
}

#[tokio::test]
async fn github_invalid_archive_and_install_destination_keep_pool_unchanged() {
    let f = Fixture::new().await;
    f.flags(true, true, false);
    let mut malformed = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    malformed
        .start_file("../escape", zip::write::SimpleFileOptions::default())
        .unwrap();
    malformed.write_all(b"escape").unwrap();
    *f.http.body.lock().unwrap() = malformed.finish().unwrap().into_inner();
    failed(&f.call(Fixture::arg()).await);
    f.no_install();
    let mut oversized = archive("github-valid", "skills/test");
    let at = oversized
        .windows(4)
        .position(|v| v == b"PK\x01\x02")
        .unwrap();
    oversized[at + 24..at + 28].copy_from_slice(&(33u32 * 1024 * 1024).to_le_bytes());
    *f.http.body.lock().unwrap() = oversized;
    failed(&f.call(Fixture::arg()).await);
    f.no_install();
    *f.http.body.lock().unwrap() = archive("github-valid", "skills/test");
    let pool = f.skills.skills_dir();
    std::fs::create_dir_all(&pool).unwrap();
    std::fs::write(pool.join("github-valid"), b"original-file-blocks-directory").unwrap();
    failed(&f.call(Fixture::arg()).await);
    assert_eq!(
        std::fs::read(pool.join("github-valid")).unwrap(),
        b"original-file-blocks-directory"
    );
    assert_eq!(std::fs::read_dir(pool).unwrap().count(), 1);
}
