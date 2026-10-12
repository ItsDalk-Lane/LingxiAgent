//! R06-T06 R1 审查修复（REPAIR-R06-T06-R1）：技能材料进入上下文编译器的
//! 生产路径测试（F-01）与工具注册响亮失败测试（F-03）。
//!
//! F-01 生产链：真实 bootstrap（SkillService 构造 + 编译器技能材料端口
//! 接线）→ 真实 `FileContextMaterialSource` → 真实 run 驱动（drive_run 的
//! `compiled_for_run` 是发送 system 文本的唯一来源）→ RecordingProvider 只
//! 扮演外部模型应答并逐字记录 system_prompt。镜像现役
//! `core/session-coordinator.ts:2102-2110`（getSkillsForAgent → skillsResult
//! 快照 → resourceLoader.getSkills 代理）与 pi `agent-session.js:784-789`
//! （loadedSkills 进 buildSystemPromptOptions）：已启用技能的目录材料按
//! 来源进入 `skill.catalog` 段（T01 冻结段注册表，digest_only wrapper 带，
//! 不新造 system 特权通道）；禁用/未启用技能绝不出现。
//!
//! F-03：记忆/技能工具注册失败是响亮启动错误（filetools.rs `.expect`
//! 既有先例），绝不静默缩窄模型工具面。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use lingxi_kernel::model_exchange::ModelTurnInput;
use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, TurnDeltaSink, TurnProviderPort,
};
use lingxi_kernel::toolcatalog::{SchemaBudget, ToolRegistry};
use lingxi_protocol::{ContentBlock, NormalizedMessage};
use lingxi_service::context_compiler::{
    ContextCompilerService, FileContextMaterialSource, HostFacts,
};
use lingxi_service::inject::SystemClock;
use lingxi_service::{
    approval_service, memory::MemoryService, memorytools, prepare_layout, skills::SkillService,
    toolgateway::ToolInvocationGateway, HomeSource, NetworkMode, ServiceConfig, ServiceDeps,
    ServiceState, LOCAL_OWNER_USER_ID,
};

const NOW_MS: u64 = 1_790_409_600_000;

// ─── 夹具 ───────────────────────────────────────────────────────────────────

fn fresh_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r06t06-skillctx-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    dir
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(path, content).expect("write fixture");
}

fn write_skill(home: &Path, name: &str, description: &str) {
    write(
        &home.join("skills").join(name).join("SKILL.md"),
        &format!("---\nname: {name}\ndescription: \"{description}\"\n---\n\nWhen activated: say {name}.\n"),
    );
}

/// $LINGXI_HOME 最小材料夹具（agent "lingxi"，与种子会话 sess_local_* 同 id）。
fn write_home_fixture(home: &Path) {
    write(
        &home.join("agents/lingxi/config.yaml"),
        "locale: zh-CN\nagent:\n  name: 灵犀\nmemory:\n  enabled: true\n",
    );
    write(&home.join("user/preferences.json"), r#"{"userName":"黎"}"#);
    write(&home.join("user/user.md"), "PROFILE-TOP_SECRET\n");
}

fn write_product_fixture(product: &Path) {
    write(
        &product.join("yuan/lingxi.md"),
        "YUAN-TOP_SECRET 你是{{userName}}的伙伴，名叫{{agentName}}。",
    );
    write(&product.join("identity-templates/lingxi.md"), "IDENTITY-ZH");
    write(&product.join("agents-templates/lingxi.md"), "AGENTS-ZH");
    write(&product.join("identity.example.md"), "EXAMPLE-IDENTITY");
    write(&product.join("agents.example.md"), "EXAMPLE-AGENTS");
}

fn test_host() -> HostFacts {
    HostFacts {
        platform: "darwin".to_string(),
        os_type: "Darwin".to_string(),
        os_release: "25.0.0".to_string(),
        shell_label: "zsh".to_string(),
        workspace: "/tmp/r06-t06-skillctx-ws".to_string(),
    }
}

// ─── RecordingProvider（外部模型替身：逐字记录 system_prompt）───────────────

struct RecordingProvider {
    script: Mutex<VecDeque<ProviderTurn>>,
    system_prompts: Mutex<Vec<Option<String>>>,
}

impl RecordingProvider {
    fn new(script: Vec<ProviderTurn>) -> Arc<Self> {
        Arc::new(Self {
            script: Mutex::new(script.into_iter().collect()),
            system_prompts: Mutex::new(Vec::new()),
        })
    }

    fn prompts(&self) -> Vec<Option<String>> {
        self.system_prompts.lock().unwrap().clone()
    }
}

fn final_turn(text: &str) -> ProviderTurn {
    ProviderTurn::Final {
        message: NormalizedMessage {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            model_call_id: None,
        },
    }
}

impl TurnProviderPort for RecordingProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.provider".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }
    fn next_turn<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        _call: &'a lingxi_protocol::ModelCallId,
        _input: &'a ModelTurnInput,
        _deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        self.system_prompts
            .lock()
            .unwrap()
            .push(_input.system_prompt.clone());
        let next = self
            .script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| final_turn("done"));
        let ctx = ctx.clone();
        Box::pin(async move { ProviderTurnResult::of_ctx(&ctx, next) })
    }
}

// ─── 组合根级启动（真实 bootstrap；编译器技能材料端口由 bootstrap 接线）─────

struct Booted {
    state: ServiceState,
    provider: Arc<RecordingProvider>,
    home: PathBuf,
    product: PathBuf,
}

async fn boot(tag: &str) -> Booted {
    let home = fresh_dir(tag);
    let product = fresh_dir(&format!("{tag}-product"));
    write_home_fixture(&home);
    write_product_fixture(&product);
    write_skill(&home, "alpha", "Alpha skill for context wiring proof");
    write_skill(&home, "beta", "Beta skill must stay hidden unless enabled");
    let home = home.canonicalize().expect("canonical home");
    let layout = prepare_layout(&home).expect("layout");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    // 生产组合根形状：真实 FileContextMaterialSource（非测试专用构造）。
    let compiler = Arc::new(ContextCompilerService::new(Arc::new(
        FileContextMaterialSource::from_home(&layout.home, product.clone(), test_host()),
    )));
    let provider = RecordingProvider::new(vec![final_turn("done")]);
    let deps = ServiceDeps {
        turn_provider: Some(provider.clone() as Arc<dyn TurnProviderPort>),
        context_compiler: Some(compiler),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");
    Booted {
        state,
        provider,
        home,
        product,
    }
}

async fn teardown(booted: Booted) {
    booted.state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(&booted.home);
    let _ = std::fs::remove_dir_all(&booted.product);
}

fn owner_principal() -> lingxi_service::Principal {
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_local".to_string(),
        kind: lingxi_service::PrincipalKind::LocalUser,
        user_id: Some(LOCAL_OWNER_USER_ID.to_string()),
        studio_id: None,
        server_node_id: None,
        device_id: None,
        credential_id: None,
        web_session_id: None,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Local,
        credential_kind: lingxi_service::CredentialKind::LoopbackToken,
        trust_state: lingxi_service::TrustState::Local,
        scopes: vec!["chat".to_string()],
    }
}

async fn drive_alpha_session(booted: &Booted) {
    booted
        .state
        .sessions()
        .execute_for(
            booted.state.storage().as_ref(),
            booted.state.events(),
            booted.state.runs(),
            &owner_principal(),
            "sess_local_alpha",
            "你好",
            NOW_MS,
        )
        .await
        .expect("user run drives");
}

// ─── F-01：启用技能经生产编译链进入 system 文本 ─────────────────────────────

#[tokio::test]
async fn enabled_skill_catalog_reaches_the_production_system_prompt() {
    let booted = boot("enabled").await;
    // 启用 alpha = PUT /lingxi/v1/agents/{id}/skills 的同一服务调用
    // （路由→服务路径已由 r06_t06_skills 的真 HTTP 套件锁定）。
    let enabled = booted
        .state
        .skills()
        .set_enabled_skills(
            "lingxi",
            &["alpha".to_string()],
            booted.state.skills().workspace_root().as_deref(),
        )
        .expect("enable alpha");
    assert_eq!(enabled, vec!["alpha".to_string()]);

    drive_alpha_session(&booted).await;

    let prompts = booted.provider.prompts();
    assert_eq!(prompts.len(), 1, "一次 final 应答 = 一次模型调用");
    let sent = prompts[0].as_deref().expect("system_prompt 必须被发送");
    assert!(
        sent.contains("<available_skills>"),
        "已启用技能必须以 skill.catalog 段进入发送文本"
    );
    assert!(
        sent.contains("<name>alpha</name>"),
        "启用技能的 name 进入目录: {sent}"
    );
    assert!(
        sent.contains("<description>Alpha skill for context wiring proof</description>"),
        "启用技能的 description 进入目录"
    );
    assert!(
        sent.contains("skills/alpha/SKILL.md"),
        "location = 来源 SKILL.md 路径（pi skills.js:293 filePath 语义）"
    );
    assert!(
        !sent.contains("Beta skill must stay hidden"),
        "未启用技能绝不进入目录"
    );
    // 观测同源：skill.catalog 段出现在观测视图（R06-A01 同一 artifact）。
    let view = booted
        .state
        .context_compiler()
        .expect("compiler wired")
        .observation_for_session("sess_local_alpha")
        .expect("observation exists");
    assert!(
        view["segments"]
            .as_array()
            .expect("segments")
            .iter()
            .any(|s| s["id"] == "skill.catalog"),
        "观测视图含 skill.catalog 段"
    );
    teardown(booted).await;
}

#[tokio::test]
async fn disabled_skill_never_reaches_the_production_system_prompt() {
    let booted = boot("disabled").await;
    // 不启用任何技能（新 agent 默认白名单为空——skills.ts:284-310 的空集）。
    drive_alpha_session(&booted).await;

    let prompts = booted.provider.prompts();
    let sent = prompts[0].as_deref().expect("system_prompt 必须被发送");
    assert!(
        !sent.contains("<available_skills>"),
        "无启用技能时 skill.catalog 段整段缺席（kernel 空段省略语义）"
    );
    assert!(
        !sent.contains("Alpha skill for context wiring proof"),
        "禁用技能的材料绝不进入发送文本"
    );
    let view = booted
        .state
        .context_compiler()
        .expect("compiler wired")
        .observation_for_session("sess_local_alpha")
        .expect("observation exists");
    assert!(
        !view["segments"]
            .as_array()
            .expect("segments")
            .iter()
            .any(|s| s["id"] == "skill.catalog"),
        "观测视图无 skill.catalog 段"
    );
    teardown(booted).await;
}

// ─── F-03：工具注册失败是响亮启动错误 ───────────────────────────────────────

#[tokio::test]
async fn memory_tool_registration_failure_is_loud_not_silent() {
    let home = fresh_dir("f03-dup");
    write_home_fixture(&home);
    let registry = Arc::new(ToolRegistry::new());
    let approvals = Arc::new(approval_service::ApprovalService::new(Arc::new(
        SystemClock,
    )));
    let gateway = Arc::new(ToolInvocationGateway::new(
        Arc::clone(&registry),
        Arc::clone(&approvals) as Arc<dyn lingxi_service::toolgateway::ToolPolicyPort>,
        Arc::new(SystemClock),
        SchemaBudget::default(),
        lingxi_service::toolgateway::DEFAULT_PREPARED_TTL_MS,
        lingxi_service::toolgateway::DEFAULT_LIVE_PREPARED_CAP,
    ));
    let memory = Arc::new(MemoryService::new(home.clone(), home.join("runtime")));
    let skills = Arc::new(SkillService::new(home.clone(), home.join("product")));

    // 第一次注册成功（六个记忆/技能工具全部就位）。
    memorytools::register_memory_tools(
        &registry,
        gateway.as_ref(),
        Arc::clone(&memory),
        Arc::clone(&skills),
        "lingxi".to_string(),
        Arc::new(SystemClock),
        &SchemaBudget::default(),
    );

    // 第二次注册（同名 target 冲突）必须响亮失败——工具面静默缩窄正是
    // 红线「禁止静默降级」与 A11 工具面最不想要的失败模式。
    let registry_copy = Arc::clone(&registry);
    let gateway_copy = Arc::clone(&gateway);
    let memory_copy = Arc::clone(&memory);
    let skills_copy = Arc::clone(&skills);
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        memorytools::register_memory_tools(
            &registry_copy,
            gateway_copy.as_ref(),
            memory_copy,
            skills_copy,
            "lingxi".to_string(),
            Arc::new(SystemClock),
            &SchemaBudget::default(),
        );
    }));
    std::panic::set_hook(prev_hook);
    let err = result.expect_err("重复注册必须 panic（响亮），不得静默跳过");
    let message = err
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default();
    assert!(
        message.contains("register"),
        "panic 消息必须点名注册失败: {message}"
    );

    let _ = std::fs::remove_dir_all(&home);
}

// ─── F-01 回归：服务级目录解析（白名单顺序 / disable-model-invocation / 遮蔽）──

#[test]
fn runtime_skill_catalog_respects_whitelist_filter_and_workspace_shadow() {
    let home = fresh_dir("catalog-unit");
    write_home_fixture(&home);
    write_skill(&home, "alpha", "Alpha catalog");
    write_skill(&home, "delta", "Delta catalog");
    // gamma 已启用但声明 disable-model-invocation → 不得进入目录
    // （现役在冻结渲染前过滤，pi skills.js:276；kernel 段冻结，过滤在服务侧）。
    write(
        &home.join("skills").join("gamma").join("SKILL.md"),
        "---\nname: gamma\ndescription: \"Gamma hidden from model\"\ndisable-model-invocation: true\n---\n\nWhen activated: say gamma.\n",
    );
    // workspace 技能：ws-only（标准目录、默认 policy 开启）+ 与用户池同名的
    // shadowed alpha（遮蔽三态，不得重复进入目录）。
    let workspace = fresh_dir("catalog-unit-ws");
    write(
        &workspace.join(".agents/skills/ws-only/SKILL.md"),
        "---\nname: ws-only\ndescription: \"Workspace only skill\"\n---\n\nWhen activated: say ws-only.\n",
    );
    write(
        &workspace.join(".agents/skills/alpha/SKILL.md"),
        "---\nname: alpha\ndescription: \"Shadowed workspace copy\"\n---\n\nWhen activated: say shadowed alpha.\n",
    );

    let service = SkillService::new(home.clone(), home.join("product"));
    service.set_workspace_root(Some(workspace.clone()));

    // PUT 白名单语义（skills.ts:284-310 同一写面）。
    service
        .set_enabled_skills(
            "lingxi",
            &[
                "gamma".to_string(),
                "delta".to_string(),
                "alpha".to_string(),
            ],
            Some(workspace.as_path()),
        )
        .expect("enable whitelist");

    let catalog = service
        .runtime_skill_catalog("lingxi")
        .expect("runtime catalog");
    let names: Vec<&str> = catalog.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["alpha", "delta", "ws-only"],
        "目录 = 用户池白名单（catalog 序 = 用户覆盖顺序的载体）+ 未遮蔽 workspace 活跃技能；disable-model-invocation 的 gamma 被过滤: {catalog:?}"
    );
    let alpha = catalog.iter().find(|s| s.name == "alpha").unwrap();
    assert!(
        alpha.location.ends_with("skills/alpha/SKILL.md"),
        "location 指向用户池来源（同名 workspace 副本被遮蔽）: {alpha:?}"
    );
    let ws = catalog.iter().find(|s| s.name == "ws-only").unwrap();
    assert!(ws.location.contains(".agents/skills/ws-only"));

    // 缺 config.yaml 的 agent = 现役 `|| []` 空白名单形状（不响亮、不虚构）：
    // 用户池技能全部缺席，但 workspace 活跃技能按默认 policy 仍解析
    // （workspace 不走白名单，走 policy/遮蔽契约——skill-manager.ts:199-252）。
    std::fs::create_dir_all(home.join("agents/bare")).expect("bare agent dir");
    let bare = service.runtime_skill_catalog("bare").expect("bare catalog");
    let bare_names: Vec<&str> = bare.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        bare_names,
        vec!["ws-only"],
        "空白名单 agent：用户池缺席、workspace 按 policy 活跃: {bare:?}"
    );

    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&workspace);
}
