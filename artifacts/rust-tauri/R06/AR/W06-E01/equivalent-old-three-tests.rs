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

