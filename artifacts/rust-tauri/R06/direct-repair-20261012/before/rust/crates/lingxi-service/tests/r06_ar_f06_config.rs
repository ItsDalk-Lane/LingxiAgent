//! R06-T01 服务侧验收测试：材料采集、会话级冻结缓存、同源观测（R06-A01）
//! 与响亮失败链。
//!
//! 夹具纪律：所有材料落在独立临时目录（LINGXI_HOME 替身）与临时
//! product_dir 内，绝不读写真实用户目录；注入测试 HostFacts（不探测真实
//! 环境），产品模板用 TOP_SECRET 标记的最小夹具而非仓库 lib/。
//!
//! 测试替身边界：RecordingProvider 只扮演「外部模型的应答」，逐字记录每次
//! 调用的 system_prompt；它从不决定权限、不写运行状态。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use lingxi_kernel::model_exchange::ModelTurnInput;
use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, TurnDeltaSink, TurnProviderPort,
};
use lingxi_protocol::{ContentBlock, NormalizedMessage};
use lingxi_service::context_compiler::{
    ContextCompilerService, ContextMaterialSource, FileContextMaterialSource, HostFacts,
    MaterialQuery,
};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
    LOCAL_OWNER_USER_ID,
};
use sha2::{Digest, Sha256};

const NOW_MS: u64 = 1_790_409_600_000;

fn sha256_hex(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

// ─── 夹具 ───

static DIR_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn fresh_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r06t01-{tag}-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
        DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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

/// 现役 $LINGXI_HOME 布局的最小材料夹具（zh 主调）。
fn write_home_fixture(home: &Path) {
    write(
        &home.join("agents/lingxi/config.yaml"),
        "locale: zh-CN\nagent:\n  name: 灵犀\nmemory:\n  enabled: true\n",
    );
    write(
        &home.join("agents/lingxi/memory/memory.md"),
        "MEMORY-TOP_SECRET 长期记忆内容\n",
    );
    write(
        &home.join("agents/lingxi/memory/tenets.json"),
        r#"{"version":1,"tenets":[
            {"id":"t-low","content":"TENET-LOW 低优先级","priority":"low","status":"active","createdAt":"2026-01-02T00:00:00.000Z"},
            {"id":"t-pending","content":"TENET-PENDING 不该出现","priority":"critical","status":"pending","createdAt":"2026-01-01T00:00:00.000Z"},
            {"id":"t-critical","content":"TENET-TOP_SECRET 置顶原则","priority":"critical","status":"active","createdAt":"2026-01-01T00:00:00.000Z"}
        ]}"#,
    );
    write(
        &home.join("agents/lingxi/appearance-summary.json"),
        r#"{"version":1,"avatarHash":"h","summary":"APPEARANCE-TOP_SECRET 银白色头发","model":"m","updatedAt":"2026-01-01T00:00:00.000Z"}"#,
    );
    write(&home.join("user/preferences.json"), r#"{"userName":"黎"}"#);
    write(
        &home.join("user/user.md"),
        "PROFILE-TOP_SECRET 用户档案正文\n",
    );
}

/// 产品模板夹具（yuan/identity/agents 的 zh 与 en 变体 + example 兜底）。
fn write_product_fixture(product: &Path) {
    write(
        &product.join("yuan/lingxi.md"),
        "YUAN-TOP_SECRET 你是{{userName}}的伙伴，名叫{{agentName}}。",
    );
    write(
        &product.join("yuan/en/lingxi.md"),
        "YUAN-EN-TOP_SECRET You are {{userName}}'s partner named {{agentName}}.",
    );
    write(&product.join("identity-templates/lingxi.md"), "IDENTITY-ZH");
    write(
        &product.join("identity-templates/en/lingxi.md"),
        "IDENTITY-EN",
    );
    write(&product.join("agents-templates/lingxi.md"), "AGENTS-ZH");
    write(&product.join("agents-templates/en/lingxi.md"), "AGENTS-EN");
    write(&product.join("identity.example.md"), "EXAMPLE-IDENTITY");
    write(&product.join("agents.example.md"), "EXAMPLE-AGENTS");
}

fn test_host() -> HostFacts {
    HostFacts {
        platform: "darwin".to_string(),
        os_type: "Darwin".to_string(),
        os_release: "25.0.0".to_string(),
        shell_label: "zsh".to_string(),
        workspace: "/tmp/r06-t01-service-ws".to_string(),
    }
}

fn fixture_source(home: &Path, product: &Path) -> FileContextMaterialSource {
    FileContextMaterialSource::from_home(home, product.to_path_buf(), test_host())
}

fn gather(
    source: &FileContextMaterialSource,
    agent_id: &str,
    for_subagent: bool,
) -> lingxi_kernel::context::ContextCompileInput {
    source
        .gather(&MaterialQuery {
            agent_id: agent_id.to_string(),
            for_subagent,
            session_started_unix_ms: NOW_MS,
        })
        .expect("materials gather")
}

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
        input: &'a ModelTurnInput,
        _deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        self.system_prompts
            .lock()
            .unwrap()
            .push(input.system_prompt.clone());
        let next = self
            .script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| final_turn("done"));
        let ctx_at_issue = ctx.clone();
        Box::pin(async move { ProviderTurnResult::of_ctx(&ctx_at_issue, next) })
    }
}

struct Booted {
    state: ServiceState,
    provider: Arc<RecordingProvider>,
    home: PathBuf,
    /// 规范化后的数据根（macOS 上 /var 会被规范到 /private/var）——材料
    /// 覆写必须落在这里。
    layout_home: PathBuf,
    product: PathBuf,
}

async fn boot_with_fixtures(tag: &str, script: Vec<ProviderTurn>) -> Booted {
    let home = fresh_dir(&format!("boot-{tag}"));
    let product = fresh_dir(&format!("boot-{tag}-product"));
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let layout = prepare_layout(&home).expect("layout");
    // 材料写进规范化后的 home（macOS 上 /var 会被规范到 /private/var）。
    write_home_fixture(&layout.home);
    write_product_fixture(&product);
    let provider = RecordingProvider::new(script);
    let compiler = Arc::new(ContextCompilerService::new(Arc::new(
        FileContextMaterialSource::from_home(&layout.home, product.clone(), test_host()),
    )));
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
        layout_home: layout.home.clone(),
        product,
    }
}

async fn teardown(booted: &Booted) {
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

async fn run_config(config: &str) -> (Vec<Option<String>>, PathBuf) {
    let booted = boot_with_fixtures("f06", vec![final_turn("done")]).await;
    write(
        &booted.layout_home.join("agents/lingxi/config.yaml"),
        config,
    );
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
        .expect("真实运行入口");
    let prompts = booted.provider.prompts();
    if !prompts.is_empty() {
        let view = booted
            .state
            .context_compiler()
            .unwrap()
            .observation_for_session("sess_local_alpha")
            .unwrap();
        assert_eq!(
            view["renderSha256"].as_str().unwrap(),
            sha256_hex(prompts[0].as_ref().unwrap())
        );
    }
    let home = booted.layout_home.clone();
    println!(
        "isolated_home={} captured_calls={}",
        home.display(),
        prompts.len()
    );
    teardown(&booted).await;
    (prompts, home)
}

#[tokio::test]
async fn positive_block_disabled_real_request() {
    let (prompts, _) =
        run_config("locale: zh-CN\nagent:\n  name: 灵犀\nmemory:\n  enabled: false\n").await;
    assert_eq!(prompts.len(), 1);
    assert!(!prompts[0].as_ref().unwrap().contains("MEMORY-TOP_SECRET"));
}

#[tokio::test]
async fn c01_flow_disabled_real_request() {
    let (prompts, _) =
        run_config("locale: zh-CN\nagent: {name: 灵犀}\nmemory: {enabled: false}\n").await;
    assert_eq!(prompts.len(), 1);
    assert!(
        !prompts[0].as_ref().unwrap().contains("MEMORY-TOP_SECRET"),
        "合法flow写法必须关闭模型请求里的记忆"
    );
}

#[test]
fn c02_quoted_hash_gather_preserved() {
    let home = fresh_dir("f06-hash");
    let product = fresh_dir("f06-hash-product");
    write_home_fixture(&home);
    write_product_fixture(&product);
    write(
        &home.join("agents/lingxi/config.yaml"),
        "locale: zh-CN\nagent:\n  name: \"助手 #1: 中文\" # 行外注释\n",
    );
    assert_eq!(
        gather(&fixture_source(&home, &product), "lingxi", false).agent_name,
        "助手 #1: 中文"
    );
    std::fs::remove_dir_all(home).unwrap();
    std::fs::remove_dir_all(product).unwrap();
}

#[test]
fn c02_c03_standard_semantics_and_fail_closed_gather() {
    let home = fresh_dir("syntax");
    let product = fresh_dir("syntax-product");
    write_home_fixture(&home);
    write_product_fixture(&product);
    let source = fixture_source(&home, &product);
    for (yaml, expected) in [
        (
            "agent: {name: \"助手 #1: 中文 \\\"双引号\\\"\"}\n",
            "助手 #1: 中文 \"双引号\"",
        ),
        (
            "\u{feff}agent:\r\n    name: '中文 #2: it''s fine' # 注释\r\n",
            "中文 #2: it's fine",
        ),
        ("agent:\n  name: |\n    中文 #3: 多行\n", "中文 #3: 多行\n"),
        (
            "unrelated: {name: wrong, memory: {enabled: false}}\nagent: {name: 正确}\n",
            "正确",
        ),
        (
            "base: &base {enabled: false}\nmemory: {<<: *base}\nagent: {name: 合并}\n",
            "合并",
        ),
    ] {
        write(&home.join("agents/lingxi/config.yaml"), yaml);
        let input = gather(&source, "lingxi", false);
        assert_eq!(input.agent_name, expected);
        if yaml.contains("&base") {
            assert!(!input.memory_enabled);
        }
    }
    for bad in [
        "memory: {enabled: [false]}",
        "memory: {enabled: null}",
        "memory: false",
        "agent: {name: 7}",
        "locale: [zh]",
        "memory: {enabled: no}",
        "skills: {enabled: false}",
        "skills: {enabled: [ok, 9]}",
        "workspace_context: {discover_project_skills: yes}",
        "experience: {enabled: 'false'}",
        "memory: {enabled: false, enabled: true}",
        "agent: {name: a, name: b}",
        "memory: {enabled: false",
        "null",
        "[]",
    ] {
        write(&home.join("agents/lingxi/config.yaml"), bad);
        let err = source
            .gather(&MaterialQuery {
                agent_id: "lingxi".into(),
                for_subagent: false,
                session_started_unix_ms: NOW_MS,
            })
            .expect_err("错误配置不得按默认放开");
        assert!(err.to_string().contains("config.yaml"), "{bad}: {err}");
    }
    // 文件读取失败与文件缺失不同，不能因此使用开启默认值。
    let config_path = home.join("agents/lingxi/config.yaml");
    std::fs::remove_file(&config_path).unwrap();
    std::fs::create_dir(&config_path).unwrap();
    assert!(
        lingxi_adapters::storage::agent_memory::config::read_agent_config(&config_path).is_err()
    );
    assert!(source
        .gather(&MaterialQuery {
            agent_id: "lingxi".into(),
            for_subagent: false,
            session_started_unix_ms: NOW_MS,
        })
        .is_err());
    std::fs::remove_dir_all(home).unwrap();
    std::fs::remove_dir_all(product).unwrap();
}

#[test]
fn c04_incumbent_sample_defaults_and_unknown_fields_survive_updates() {
    use lingxi_adapters::storage::agent_memory::{
        config::AgentConfigDocument, patch_agent_config, AgentConfigFlags, AgentConfigPatch,
    };
    let sample = include_str!("../../../../lib/config.example.yaml");
    let doc = AgentConfigDocument::parse(sample).unwrap();
    assert_eq!(doc.flags().unwrap().memory_enabled, Some(true));
    assert_eq!(doc.skill_lists().unwrap().0, vec!["skill-creator"]);
    for empty in ["", "# empty\n", "\u{feff}  \r\n"] {
        assert_eq!(
            AgentConfigDocument::parse(empty)
                .unwrap()
                .scan()
                .unwrap()
                .memory_enabled,
            None
        );
    }
    let original="agent: {name: '助手 #1', yuan: lingxi}\nmemory: {enabled: false, token_budget: 1234, custom: {unicode: 中文, nested: [a, false, 7]}}\nskills: {enabled: [one], custom: 保留}\nproviders: {custom: {unknown: [1, 2]}}\n";
    let changed = AgentConfigPatch::write_flags(
        original,
        &AgentConfigFlags {
            memory_enabled: Some(true),
            experience_enabled: Some(false),
            ..Default::default()
        },
    )
    .unwrap();
    let changed =
        AgentConfigPatch::write_skill_lists(&changed, &["路径\\含引号\" #中文".into()], &[])
            .unwrap();
    // 用同一真实 YAML 值逐字段核对未知数据，而不靠输出行形状猜保留。
    assert!(changed.contains("token_budget: 1234"));
    assert!(changed.contains("unicode: 中文"));
    assert!(changed.contains("unknown:"));
    assert!(changed.contains("custom: 保留"));
    assert_eq!(
        AgentConfigPatch::read_flags(&changed)
            .unwrap()
            .memory_enabled,
        Some(true)
    );
    assert_eq!(
        AgentConfigPatch::read_skill_lists(&changed).unwrap().0,
        vec!["路径\\含引号\" #中文"]
    );
    assert_eq!(
        AgentConfigDocument::parse(&changed)
            .unwrap()
            .scan()
            .unwrap()
            .agent_name
            .as_deref(),
        Some("助手 #1")
    );
    let dir = fresh_dir("patch-invalid");
    let path = dir.join("config.yaml");
    let bad = "memory: {enabled: false, enabled: true}";
    write(&path, bad);
    assert!(
        patch_agent_config(&path, |text| AgentConfigPatch::write_flags(
            text,
            &AgentConfigFlags {
                memory_enabled: Some(true),
                ..Default::default()
            }
        ))
        .is_err()
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), bad);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn adversarial_real_memory_tool_rechecks_flow_and_corrupt_config() {
    use lingxi_kernel::ports::{ToolOutcome, ToolRequest};
    use lingxi_kernel::toolcatalog::{SchemaBudget, ToolRegistry};
    use lingxi_service::inject::SystemClock;
    use lingxi_service::toolgateway::{InvocationPermissionContext, ToolInvocationGateway};
    use lingxi_service::{
        approval_service, memory::MemoryService, memorytools, skills::SkillService,
    };
    let home = fresh_dir("tool-switch");
    write_home_fixture(&home);
    write(
        &home.join("agents/lingxi/memory/tenets.json"),
        r#"{"version":1,"tenets":[]}"#,
    );
    let memory = Arc::new(MemoryService::new(home.clone(), home.join("runtime")));
    let registry = Arc::new(ToolRegistry::new());
    let budget = SchemaBudget::default();
    let approvals = Arc::new(approval_service::ApprovalService::new(Arc::new(
        SystemClock,
    )));
    let gateway = ToolInvocationGateway::new(
        Arc::clone(&registry),
        approvals,
        Arc::new(SystemClock),
        budget,
        lingxi_service::toolgateway::DEFAULT_PREPARED_TTL_MS,
        lingxi_service::toolgateway::DEFAULT_LIVE_PREPARED_CAP,
    );
    memorytools::register_memory_tools(
        &registry,
        &gateway,
        Arc::clone(&memory),
        Arc::new(SkillService::new(home.clone(), home.join("product"))),
        "lingxi".into(),
        Arc::new(SystemClock),
        &budget,
    );
    let ctx = lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("session-config"),
        run_id: lingxi_protocol::RunId::new("run-config"),
        attempt: lingxi_protocol::AttemptId::new("run-config#a1"),
        generation: 1,
    };
    let resource_depth = format!("future: {}x{}", "[".repeat(65), "]".repeat(65));
    for (i, yaml) in [
        "memory: {enabled: false}",
        "memory: {enabled: 'false'}",
        "memory: {enabled: false, enabled: true}",
        "? [memory]\n: {enabled: false}",
        "memory:\n  ? [enabled]\n  : false",
        "defaults: &d {? [enabled]: false}\nmemory: {<<: *d}",
        resource_depth.as_str(),
    ]
    .iter()
    .enumerate()
    {
        write(&home.join("agents/lingxi/config.yaml"), yaml);
        let call = lingxi_protocol::ToolCallId::new(format!("config-{i}"));
        let request = ToolRequest::from_effective_arguments(
            "tool:first-party:tenet_propose",
            serde_json::json!({"content":"不得落盘的提案"}),
            &budget,
        )
        .unwrap();
        let handle = gateway
            .prepare_from_request(
                &ctx,
                lingxi_service::toolgateway::CallerSurface::UserRun,
                "lingxi",
                InvocationPermissionContext::UserSession {
                    mode: lingxi_kernel::subagent::SessionPermissionMode::Operate,
                },
                &call,
                &request,
            )
            .unwrap()
            .handle;
        let result = gateway
            .execute_prepared(&ctx, &call, &handle)
            .await
            .unwrap();
        if i == 0 {
            match result.outcome {
                ToolOutcome::Success { result } => {
                    assert!(format!("{result:?}").contains("Memory is paused"))
                }
                other => panic!("{other:?}"),
            }
        } else {
            assert!(
                matches!(result.outcome, ToolOutcome::Failed { .. }),
                "损坏或重复开关应明确失败"
            );
        }
        assert!(
            memory.tenets_store("lingxi").unwrap().pending().is_empty(),
            "拒绝后没有提案落盘"
        );
    }
    std::fs::remove_dir_all(home).unwrap();
}

#[tokio::test]
async fn p02_real_gateway_rechecks_literal_merge_and_unknown_tags() {
    use lingxi_kernel::ports::{ToolOutcome, ToolRequest};
    use lingxi_kernel::toolcatalog::{SchemaBudget, ToolRegistry};
    use lingxi_service::inject::SystemClock;
    use lingxi_service::toolgateway::{InvocationPermissionContext, ToolInvocationGateway};
    use lingxi_service::{
        approval_service, memory::MemoryService, memorytools, skills::SkillService,
    };
    let home = fresh_dir("p02-tool-switch");
    write_home_fixture(&home);
    write(
        &home.join("agents/lingxi/memory/tenets.json"),
        r#"{"version":1,"tenets":[]}"#,
    );
    let memory = Arc::new(MemoryService::new(home.clone(), home.join("runtime")));
    let registry = Arc::new(ToolRegistry::new());
    let budget = SchemaBudget::default();
    let approvals = Arc::new(approval_service::ApprovalService::new(Arc::new(
        SystemClock,
    )));
    let gateway = ToolInvocationGateway::new(
        Arc::clone(&registry),
        approvals,
        Arc::new(SystemClock),
        budget,
        lingxi_service::toolgateway::DEFAULT_PREPARED_TTL_MS,
        lingxi_service::toolgateway::DEFAULT_LIVE_PREPARED_CAP,
    );
    memorytools::register_memory_tools(
        &registry,
        &gateway,
        Arc::clone(&memory),
        Arc::new(SkillService::new(home.clone(), home.join("product"))),
        "lingxi".into(),
        Arc::new(SystemClock),
        &budget,
    );
    let ctx = lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("session-config"),
        run_id: lingxi_protocol::RunId::new("run-config"),
        attempt: lingxi_protocol::AttemptId::new("run-config#a1"),
        generation: 1,
    };
    for (i, yaml) in [
        "memory: {enabled: false}",
        "a: &a {enabled: false}\nmemory: {<<: *a}",
        "memory: {\"<<\": {enabled: true}, enabled: false}",
        "memory: {enabled: !custom true}",
        "memory: !custom {enabled: true}",
        "agent: {name: !custom 姓名}",
        "skills: {enabled: [!custom one]}",
        "workspace_context: {discover_project_skills: !custom false}",
        "future: !custom {value: keep}",
    ]
    .iter()
    .enumerate()
    {
        write(&home.join("agents/lingxi/config.yaml"), yaml);
        let call = lingxi_protocol::ToolCallId::new(format!("config-{i}"));
        let request = ToolRequest::from_effective_arguments(
            "tool:first-party:tenet_propose",
            serde_json::json!({"content":"不得落盘的提案"}),
            &budget,
        )
        .unwrap();
        let handle = gateway
            .prepare_from_request(
                &ctx,
                lingxi_service::toolgateway::CallerSurface::UserRun,
                "lingxi",
                InvocationPermissionContext::UserSession {
                    mode: lingxi_kernel::subagent::SessionPermissionMode::Operate,
                },
                &call,
                &request,
            )
            .unwrap()
            .handle;
        let result = gateway
            .execute_prepared(&ctx, &call, &handle)
            .await
            .unwrap();
        if i < 3 {
            match result.outcome {
                ToolOutcome::Success { result } => {
                    assert!(format!("{result:?}").contains("Memory is paused"))
                }
                other => panic!("{other:?}"),
            }
        } else {
            assert!(
                matches!(result.outcome, ToolOutcome::Failed { .. }),
                "损坏或重复开关应明确失败"
            );
        }
        assert!(
            memory.tenets_store("lingxi").unwrap().pending().is_empty(),
            "拒绝后没有提案落盘"
        );
    }
    std::fs::remove_dir_all(home).unwrap();
}

#[tokio::test]
async fn p02_real_install_gateway_checks_config_before_install_and_enable() {
    use lingxi_kernel::ports::{ToolOutcome, ToolRequest};
    use lingxi_kernel::toolcatalog::{SchemaBudget, ToolRegistry};
    use lingxi_service::inject::SystemClock;
    use lingxi_service::toolgateway::{InvocationPermissionContext, ToolInvocationGateway};
    use lingxi_service::{
        approval_service, memory::MemoryService, memorytools, skills::SkillService,
    };
    let home = fresh_dir("p02-install-switch");
    write_home_fixture(&home);
    write(
        &home.join("user/preferences.json"),
        r#"{"capabilities":{"learn_skills":{"enabled":true,"safety_review":false}}}"#,
    );
    write(
        &home.join("agents/lingxi/memory/tenets.json"),
        r#"{"version":1,"tenets":[]}"#,
    );
    let memory = Arc::new(MemoryService::new(home.clone(), home.join("runtime")));
    let registry = Arc::new(ToolRegistry::new());
    let budget = SchemaBudget::default();
    let approvals = Arc::new(approval_service::ApprovalService::new(Arc::new(
        SystemClock,
    )));
    let gateway = ToolInvocationGateway::new(
        Arc::clone(&registry),
        approvals,
        Arc::new(SystemClock),
        budget,
        lingxi_service::toolgateway::DEFAULT_PREPARED_TTL_MS,
        lingxi_service::toolgateway::DEFAULT_LIVE_PREPARED_CAP,
    );
    memorytools::register_memory_tools(
        &registry,
        &gateway,
        Arc::clone(&memory),
        Arc::new(SkillService::new(home.clone(), home.join("product"))),
        "lingxi".into(),
        Arc::new(SystemClock),
        &budget,
    );
    let ctx = lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("session-config"),
        run_id: lingxi_protocol::RunId::new("run-config"),
        attempt: lingxi_protocol::AttemptId::new("run-config#a1"),
        generation: 1,
    };
    let mut failures = Vec::new();
    let resource_depth = format!("future: {}x{}", "[".repeat(65), "]".repeat(65));
    for (i, yaml) in [
        "memory: {enabled: false}\nskills: {\"<<\": {enabled: [must-not-enable]}}\nfuture: {\"<<\": {marker: keep}}",
        "memory: {enabled: !custom true}",
        "skills: {enabled: [!custom one]}",
        "agent: {name: !custom 姓名}",
        "memory: {enabled: !bool true}",
        "agent: {name: !str 姓名}",
        "memory: !map {enabled: false}",
        "agent: {name: !!binary SGVsbG8=}",
        resource_depth.as_str(),
    ].iter().enumerate() {
        let config = home.join("agents/lingxi/config.yaml");
        write(&config, yaml);
        let name = format!("p02-install-{i}");
        let source = home.join("source").join(&name);
        write(&source.join("SKILL.md"), &format!("---\nname: {name}\ndescription: synthetic local package\n---\nA harmless local skill."));
        let call = lingxi_protocol::ToolCallId::new(format!("install-{i}"));
        let request = ToolRequest::from_effective_arguments(
            "tool:first-party:install_skill",
            serde_json::json!({"local_path":source}), &budget,
        ).unwrap();
        let handle = gateway.prepare_from_request(
            &ctx, lingxi_service::toolgateway::CallerSurface::UserRun, "lingxi",
            InvocationPermissionContext::UserSession {mode:lingxi_kernel::subagent::SessionPermissionMode::Operate},
            &call,&request,
        ).unwrap().handle;
        let result = gateway.execute_prepared(&ctx,&call,&handle).await.unwrap();
        let installed = home.join("skills").join(&name).join("SKILL.md").is_file();
        let saved = std::fs::read_to_string(&config).unwrap();
        let ok = if i == 0 {
            matches!(result.outcome, ToolOutcome::Success {..}) && installed &&
            lingxi_adapters::storage::agent_memory::AgentConfigPatch::read_skill_lists(&saved).unwrap().0 == vec![name.clone()] &&
            saved.contains("marker: keep") && saved.contains("must-not-enable") &&
            saved.contains("<<")
        } else {
            matches!(result.outcome, ToolOutcome::Failed {..}) && !installed && saved == *yaml
        };
        println!("p02_install={name} original={yaml:?} saved={saved:?} installed={installed} outcome={:?} matches={ok}",result.outcome);
        if !ok {failures.push(name);}
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let config = home.join("agents/lingxi/config.yaml");
        let original =
            "memory: {enabled: false}\nskills: {enabled: []}\nfuture: {data: !!binary /w==}";
        write(&config, original);
        let name = "p03-late-write";
        let source = home.join("source").join(name);
        write(
            &source.join("SKILL.md"),
            "---\nname: p03-late-write\ndescription: synthetic\n---\nHarmless",
        );
        let parent = config.parent().unwrap();
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o555)).unwrap();
        let call = lingxi_protocol::ToolCallId::new("p03-late-write");
        let request = ToolRequest::from_effective_arguments(
            "tool:first-party:install_skill",
            serde_json::json!({"local_path":source}),
            &budget,
        )
        .unwrap();
        let handle = gateway
            .prepare_from_request(
                &ctx,
                lingxi_service::toolgateway::CallerSurface::UserRun,
                "lingxi",
                InvocationPermissionContext::UserSession {
                    mode: lingxi_kernel::subagent::SessionPermissionMode::Operate,
                },
                &call,
                &request,
            )
            .unwrap()
            .handle;
        let result = gateway
            .execute_prepared(&ctx, &call, &handle)
            .await
            .unwrap();
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o755)).unwrap();
        let installed = home.join("skills").join(name).join("SKILL.md").is_file();
        let saved = std::fs::read_to_string(&config).unwrap();
        println!(
            "p03_late_install installed={installed} saved={saved:?} outcome={:?}",
            result.outcome
        );
        assert!(installed);
        assert_eq!(saved, original);
        assert!(
            matches!(result.outcome,ToolOutcome::Failed{ref error} if error.message.contains("installed")&&error.message.contains("enabling failed"))
        );
    }
    std::fs::remove_dir_all(home).unwrap();
    assert!(
        failures.is_empty(),
        "安装/启用不得掩盖配置错误: {failures:?}"
    );
}

#[test]
fn p03_delete_preflights_every_peer_before_any_effect() {
    use lingxi_service::skills::SkillService;
    let mut failures = Vec::new();
    for (i, peer) in [
        "skills: {enabled: [owned]}",
        "skills: {enabled: !seq [owned]}",
        "skills: {enabled: [owned",
        "DIRECTORY",
    ]
    .iter()
    .enumerate()
    {
        let home = fresh_dir(&format!("p03-delete-{i}"));
        write(
            &home.join("agents/lingxi/config.yaml"),
            "skills: {enabled: [owned]}",
        );
        write(
            &home.join("skills/owned/SKILL.md"),
            "---\nname: owned\ndescription: synthetic\n---\nOwned synthetic skill",
        );
        let peer_path = home.join("agents/peer/config.yaml");
        if *peer == "DIRECTORY" {
            std::fs::create_dir_all(&peer_path).unwrap();
        } else {
            write(&peer_path, peer);
        }
        let before = std::fs::read_to_string(home.join("agents/lingxi/config.yaml")).unwrap();
        let service = SkillService::new(home.clone(), home.join("product"));
        let outcome = service.delete_skill("lingxi", "owned", None);
        let exists = home.join("skills/owned/SKILL.md").is_file();
        let after = std::fs::read_to_string(home.join("agents/lingxi/config.yaml")).unwrap();
        println!("p03_delete input={peer:?} outcome={outcome:?} exists={exists} current={after:?}");
        let ok = if i == 0 {
            outcome.is_ok() && !exists && after != before
        } else {
            outcome.is_err() && exists && after == before
        };
        if !ok {
            failures.push(i);
        }
        std::fs::remove_dir_all(home).unwrap();
    }
    assert!(
        failures.is_empty(),
        "删除前配置错误不得先删技能: {failures:?}"
    );
}
