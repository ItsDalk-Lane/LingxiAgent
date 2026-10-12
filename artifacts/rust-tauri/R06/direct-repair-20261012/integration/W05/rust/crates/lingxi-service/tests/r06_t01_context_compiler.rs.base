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
use lingxi_kernel::subagent::{RunLineage, RunOrigin, ToolAccessTier};
use lingxi_protocol::{ContentBlock, NormalizedMessage};
use lingxi_service::context_compiler::{
    scan_agent_config, scan_preferences, ContextCompileFailure, ContextCompilerService,
    ContextMaterialSource, FileContextMaterialSource, HostFacts, MaterialQuery,
};
use lingxi_service::runs::{DriveAuthorization, RunGrant};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
    LOCAL_OWNER_USER_ID,
};
use serde_json::Value;
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

// ─── 单元级：窄扫描 / 解析 / 路径 ───

#[test]
fn scan_agent_config_reads_supported_keys() {
    let scan = scan_agent_config(
        "# comment\nlocale: zh-CN  # trailing comment\nagent:\n  name: \"灵犀\"  # bot\n  yuan: 'butter'\nmemory:\n  enabled: false\nother:\n  name: ignored\n",
    );
    assert_eq!(scan.locale.as_deref(), Some("zh-CN"));
    assert_eq!(scan.agent_name.as_deref(), Some("灵犀"));
    assert_eq!(scan.yuan.as_deref(), Some("butter"));
    assert_eq!(scan.memory_enabled, Some(false));
    // 未知段里的同名字段不得串扰。
    let scan = scan_agent_config("other:\n  name: wrong\nagent:\n  yuan: ming\n");
    assert_eq!(scan.agent_name, None);
    assert_eq!(scan.yuan.as_deref(), Some("ming"));
}

#[test]
fn scan_preferences_defaults_and_gates() {
    // 缺失/空 → 全默认（learn_skills 门槛默认关闭：allow_github_fetch 默认假）。
    let scan = scan_preferences(None);
    assert_eq!(scan.user_name, None);
    assert_eq!(scan.locale, None);
    assert!(!scan.proactive_delegation);
    assert!(!scan.learn_skills_enabled);
    // userName/locale 去空白；experiments 严格布尔；learn_skills 双门槛。
    let value = serde_json::json!({
        "userName": "  黎  ",
        "locale": "zh-CN",
        "experiments": {"subagent.proactive_delegation": true},
        "learn_skills": {"enabled": true, "allow_github_fetch": true}
    });
    let scan = scan_preferences(Some(&value));
    assert_eq!(scan.user_name.as_deref(), Some("黎"));
    assert_eq!(scan.locale.as_deref(), Some("zh-CN"));
    assert!(scan.proactive_delegation);
    assert!(scan.learn_skills_enabled);
    // 只开 enabled 不开 allow_github_fetch → 门槛仍关（现役同规则）。
    let value = serde_json::json!({"learn_skills": {"enabled": true}});
    assert!(!scan_preferences(Some(&value)).learn_skills_enabled);
    // 非布尔的 experiments 值不生效。
    let value = serde_json::json!({"experiments": {"subagent.proactive_delegation": "yes"}});
    assert!(!scan_preferences(Some(&value)).proactive_delegation);
}

#[test]
fn resolve_product_dir_prefers_env_override() {
    let home = fresh_dir("pd-env");
    let exe = home.join("bin/lingxi-service");
    write(&exe, "");
    let picked = lingxi_service::context_compiler::resolve_product_dir(
        Some("/explicit/product"),
        &exe,
        &home,
    );
    assert_eq!(picked, PathBuf::from("/explicit/product"));
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn resolve_product_dir_ascends_from_exe() {
    let root = fresh_dir("pd-ascend");
    write(&root.join("lib/yuan/lingxi.md"), "yuan");
    let exe = root.join("rust/target/debug/lingxi-service");
    write(&exe, "");
    let picked =
        lingxi_service::context_compiler::resolve_product_dir(None, &exe, &root.join("home"));
    assert_eq!(picked, root.join("lib"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn resolve_product_dir_falls_back_to_home_product() {
    let root = fresh_dir("pd-fallback");
    let exe = root.join("bin/lingxi-service");
    write(&exe, "");
    let home = root.join("home");
    let picked = lingxi_service::context_compiler::resolve_product_dir(None, &exe, &home);
    assert_eq!(picked, home.join("product"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn format_session_started_label_has_incumbent_shape() {
    let label = lingxi_service::context_compiler::format_session_started_label(NOW_MS);
    // 现役 Intl en-US 形状："{Weekday}, {Month} {D}, {YYYY} at {HH}:{MM} GMT±H[:MM]"。
    // 具体星期/时区随测试机本地时区变化，只锁形状。
    let weekday = [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ]
    .iter()
    .any(|day| label.starts_with(day));
    assert!(weekday, "label must start with an English weekday: {label}");
    assert!(label.contains(" at "), "label must contain ' at ': {label}");
    assert!(
        label.contains("GMT"),
        "label must carry a GMT offset: {label}"
    );
    assert!(label.contains("2026"), "label must carry the year: {label}");
}

#[test]
fn platform_note_matches_incumbent_byte_shape() {
    let note = test_host().platform_note();
    let expected = concat!(
        "<environment_context>\n",
        "  <platform>darwin</platform>\n",
        "  <cwd></cwd>\n",
        "  <shell>zsh</shell>\n",
        "  <os>Darwin 25.0.0</os>\n",
        "  <sandbox_mode>read-all_write-scoped_network-on</sandbox_mode>\n",
        "</environment_context>\n",
        "Server execution environment; the user's display device may differ."
    );
    assert_eq!(note, expected);
}

// ─── 单元级：材料采集链 ───

#[test]
fn gather_collects_zh_materials_through_fallbacks() {
    let home = fresh_dir("gather-zh");
    let product = fresh_dir("gather-zh-product");
    write_home_fixture(&home);
    write_product_fixture(&product);
    let input = gather(&fixture_source(&home, &product), "lingxi", false);

    assert_eq!(input.locale, "zh-CN");
    assert_eq!(input.user_name, "黎");
    assert_eq!(input.resolved_user_name, "黎");
    assert_eq!(input.agent_name, "灵犀");
    assert_eq!(input.agent_id, "lingxi");
    assert!(input.memory_enabled);
    assert!(!input.for_subagent);
    // 人格回落链：agentDir 无 identity/AGENTS → 产品 zh 模板。
    assert_eq!(input.identity_md, "IDENTITY-ZH");
    assert_eq!(input.agents_md, "AGENTS-ZH");
    assert_eq!(
        input.yuan_md,
        "YUAN-TOP_SECRET 你是{{userName}}的伙伴，名叫{{agentName}}。"
    );
    assert_eq!(
        input.user_profile_md.as_deref(),
        Some("PROFILE-TOP_SECRET 用户档案正文\n")
    );
    assert_eq!(input.memory_md, "MEMORY-TOP_SECRET 长期记忆内容\n");
    // tenets：只取 active，按 priority 权重排序（critical 在 low 前）。
    assert_eq!(
        input.tenets,
        vec![
            "TENET-TOP_SECRET 置顶原则".to_string(),
            "TENET-LOW 低优先级".to_string()
        ]
    );
    assert!(input.inject_appearance);
    assert_eq!(
        input.appearance_summary.as_deref(),
        Some("APPEARANCE-TOP_SECRET 银白色头发")
    );
    assert!(!input.computer_use_available);
    assert!(!input.learn_skills_enabled);
    assert!(!input.proactive_delegation);
    // 花名册：仅自身一个 agent（含 config.yaml 的目录只有 lingxi）。
    assert_eq!(input.roster.len(), 1);
    assert_eq!(input.roster[0].id, "lingxi");
    assert_eq!(input.roster[0].name, "灵犀");
    assert_eq!(input.environment_note, test_host().platform_note());
    assert!(input.session_started_label.contains("GMT"));
    assert!(input.append_system_lines.is_empty());
    assert!(input.project_context_files.is_empty());
    assert!(input.skills.is_empty());
    assert_eq!(input.cwd, "/tmp/r06-t01-service-ws");
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&product);
}

#[test]
fn gather_uses_en_templates_for_en_locale() {
    let home = fresh_dir("gather-en");
    let product = fresh_dir("gather-en-product");
    write_home_fixture(&home);
    write_product_fixture(&product);
    write(&home.join("agents/lingxi/config.yaml"), "locale: en\n");
    let input = gather(&fixture_source(&home, &product), "lingxi", false);
    assert_eq!(input.locale, "en");
    assert_eq!(input.identity_md, "IDENTITY-EN");
    assert_eq!(input.agents_md, "AGENTS-EN");
    assert!(input.yuan_md.starts_with("YUAN-EN-TOP_SECRET"));
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&product);
}

#[test]
fn gather_falls_back_to_generic_yuan_when_lang_variant_missing() {
    let home = fresh_dir("gather-yuan-fallback");
    let product = fresh_dir("gather-yuan-fallback-product");
    write_home_fixture(&home);
    write_product_fixture(&product);
    // en 语言但 butter 只有通用模板。
    write(
        &home.join("agents/lingxi/config.yaml"),
        "locale: en\nagent:\n  yuan: butter\n",
    );
    write(&product.join("yuan/butter.md"), "YUAN-BUTTER-GENERIC");
    let input = gather(&fixture_source(&home, &product), "lingxi", false);
    assert_eq!(input.yuan_md, "YUAN-BUTTER-GENERIC");
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&product);
}

#[test]
fn corrupt_tenets_json_is_a_loud_material_error() {
    let home = fresh_dir("corrupt-tenets");
    let product = fresh_dir("corrupt-tenets-product");
    write_home_fixture(&home);
    write_product_fixture(&product);
    write(&home.join("agents/lingxi/memory/tenets.json"), "{ not json");
    let err = fixture_source(&home, &product)
        .gather(&MaterialQuery {
            agent_id: "lingxi".to_string(),
            for_subagent: false,
            session_started_unix_ms: NOW_MS,
        })
        .expect_err("损坏的 tenets.json 必须响亮失败");
    let msg = err.to_string();
    assert!(msg.contains("tenets.json"), "错误须点名文件: {msg}");
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&product);
}

#[test]
fn corrupt_preferences_is_a_loud_material_error() {
    let home = fresh_dir("corrupt-prefs");
    let product = fresh_dir("corrupt-prefs-product");
    write_home_fixture(&home);
    write_product_fixture(&product);
    write(&home.join("user/preferences.json"), "not json at all");
    let err = fixture_source(&home, &product)
        .gather(&MaterialQuery {
            agent_id: "lingxi".to_string(),
            for_subagent: false,
            session_started_unix_ms: NOW_MS,
        })
        .expect_err("损坏的 preferences.json 必须响亮失败");
    assert!(err.to_string().contains("preferences.json"));
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&product);
}

#[test]
fn missing_memory_files_read_as_empty_and_drop_the_band() {
    let home = fresh_dir("no-memory");
    let product = fresh_dir("no-memory-product");
    write_home_fixture(&home);
    write_product_fixture(&product);
    let _ = std::fs::remove_dir_all(home.join("agents/lingxi/memory"));
    let input = gather(&fixture_source(&home, &product), "lingxi", false);
    assert!(input.memory_md.is_empty());
    assert!(input.tenets.is_empty());
    let artifact =
        lingxi_kernel::context::ContextCompiler::compile(&input).expect("compile without memory");
    assert!(
        !artifact
            .segments()
            .iter()
            .any(|segment| segment.id.starts_with("memory.")),
        "无记忆材料时整条记忆带不注入"
    );
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&product);
}

// ─── 单元级：编译服务（冻结缓存 + 观测装饰） ───

fn fixture_service(tag: &str) -> (PathBuf, PathBuf, Arc<ContextCompilerService>) {
    let home = fresh_dir(&format!("svc-{tag}"));
    let product = fresh_dir(&format!("svc-{tag}-product"));
    write_home_fixture(&home);
    write_product_fixture(&product);
    let service = Arc::new(ContextCompilerService::new(Arc::new(fixture_source(
        &home, &product,
    ))));
    (home, product, service)
}

#[test]
fn compiled_for_run_freezes_per_session_and_shape() {
    let (home, product, service) = fixture_service("cache");
    let a = service
        .compiled_for_run("sess_a", "lingxi", false, NOW_MS)
        .expect("compile a");
    let b = service
        .compiled_for_run("sess_a", "lingxi", false, NOW_MS + 60_000)
        .expect("compile b");
    assert!(
        Arc::ptr_eq(&a, &b),
        "同一会话同一形态的重复编译必须返回同一份冻结 artifact"
    );
    let sub = service
        .compiled_for_run("sess_a", "lingxi", true, NOW_MS)
        .expect("compile subagent shape");
    assert!(!Arc::ptr_eq(&a, &sub), "子代理形态是另一份缓存条目");
    // 观测读面：主形态优先；未知会话无观测。
    let view = service
        .observation_for_session("sess_a")
        .expect("observation");
    assert_eq!(view["forSubagent"], Value::Bool(false));
    assert!(service.observation_for_session("sess_unknown").is_none());
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&product);
}

#[test]
fn observation_digests_match_the_sent_render_bytes() {
    let (home, product, service) = fixture_service("digest");
    let compiled = service
        .compiled_for_run("sess_digest", "lingxi", false, NOW_MS)
        .expect("compile");
    let render = compiled.render();
    let view = compiled.observation();
    assert_eq!(
        view["renderSha256"].as_str().expect("renderSha256"),
        sha256_hex(render),
        "整文 digest 必须对得上发送文本"
    );
    let segments = view["segments"].as_array().expect("segments array");
    assert!(!segments.is_empty());
    for segment in segments {
        let start = segment["byteStart"].as_u64().expect("byteStart") as usize;
        let end = segment["byteEnd"].as_u64().expect("byteEnd") as usize;
        let slice = &render[start..end];
        assert_eq!(
            segment["sha256"].as_str().expect("segment sha256"),
            sha256_hex(slice),
            "段 {} 的 digest 必须对得上发送文本切片",
            segment["id"]
        );
        if segment["disclosure"] == "full" {
            assert_eq!(
                segment["text"].as_str().expect("full segment text"),
                slice,
                "full 段的观测正文必须等于发送切片"
            );
        } else {
            assert!(
                segment.get("text").is_none(),
                "digest-only 段 {} 不得携带正文",
                segment["id"]
            );
        }
    }
    // 脱敏纪律：观测 JSON 全文不出现任何用户派生内容标记。
    let serialized = serde_json::to_string(view).expect("serialize observation");
    for marker in [
        "MEMORY-TOP_SECRET",
        "PROFILE-TOP_SECRET",
        "APPEARANCE-TOP_SECRET",
        "TENET-TOP_SECRET",
        "YUAN-TOP_SECRET",
    ] {
        assert!(!serialized.contains(marker), "观测视图不得泄漏 {marker}");
    }
    // 而发送文本确实携带这些材料（观测不是「发了个寂寞」）。
    for marker in ["MEMORY-TOP_SECRET", "YUAN-TOP_SECRET"] {
        assert!(render.contains(marker), "发送文本应含 {marker}");
    }
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&product);
}

#[test]
fn missing_yuan_surfaces_a_loud_compile_failure() {
    let home = fresh_dir("svc-no-yuan");
    let product = fresh_dir("svc-no-yuan-product");
    write_home_fixture(&home);
    // 产品目录没有 yuan/lingxi.md。
    write(&product.join("identity-templates/lingxi.md"), "IDENTITY-ZH");
    write(&product.join("agents-templates/lingxi.md"), "AGENTS-ZH");
    let service = ContextCompilerService::new(Arc::new(fixture_source(&home, &product)));
    let failure = service
        .compiled_for_run("sess_no_yuan", "lingxi", false, NOW_MS)
        .expect_err("yuan 缺失必须响亮失败");
    match failure {
        ContextCompileFailure::Compile(err) => {
            assert_eq!(
                err.to_string(),
                "persona source incomplete: yuan template is empty (agent id lingxi)"
            );
        }
        other => panic!("期望编译层失败，实际: {other}"),
    }
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&product);
}

// ─── 组合根级：发送与观测同源（R06-A01 服务腿） ───

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

#[tokio::test]
async fn sent_system_prompt_is_byte_identical_to_observed_artifact() {
    let booted = boot_with_fixtures("same-origin", vec![final_turn("done")]).await;
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
    let prompts = booted.provider.prompts();
    assert_eq!(prompts.len(), 1, "一次 final 应答 = 一次模型调用");
    let sent = prompts[0].as_deref().expect("system_prompt 必须被发送");

    let view = booted
        .state
        .context_compiler()
        .expect("compiler wired")
        .observation_for_session("sess_local_alpha")
        .expect("observation exists after a sent request");
    // R06-A01：观测的整文 digest 必须等于实际发送字节的 digest。
    assert_eq!(
        view["renderSha256"].as_str().expect("renderSha256"),
        sha256_hex(sent),
        "观测与发送必须同源（整文 digest 一致）"
    );
    for segment in view["segments"].as_array().expect("segments") {
        let start = segment["byteStart"].as_u64().unwrap() as usize;
        let end = segment["byteEnd"].as_u64().unwrap() as usize;
        assert_eq!(
            segment["sha256"].as_str().unwrap(),
            sha256_hex(&sent[start..end]),
            "段 {} 的观测 digest 与发送切片不一致",
            segment["id"]
        );
    }
    // 发送文本携带真实材料；观测 JSON 对 digest-only 段不携带正文。
    assert!(sent.contains("MEMORY-TOP_SECRET"));
    assert!(sent.contains("灵犀"));
    let serialized = serde_json::to_string(&view).expect("serialize");
    for marker in [
        "MEMORY-TOP_SECRET",
        "PROFILE-TOP_SECRET",
        "APPEARANCE-TOP_SECRET",
    ] {
        assert!(!serialized.contains(marker), "观测不得泄漏 {marker}");
    }
    teardown(&booted).await;
}

#[tokio::test]
async fn session_freeze_spans_runs_and_turns() {
    let script = vec![
        ProviderTurn::Continue {
            process_note: "thinking".to_string(),
        },
        final_turn("done"),
        final_turn("done again"),
    ];
    let booted = boot_with_fixtures("freeze", script).await;
    for input in ["第一句", "第二句"] {
        booted
            .state
            .sessions()
            .execute_for(
                booted.state.storage().as_ref(),
                booted.state.events(),
                booted.state.runs(),
                &owner_principal(),
                "sess_local_alpha",
                input,
                NOW_MS,
            )
            .await
            .expect("user run drives");
    }
    let prompts = booted.provider.prompts();
    assert_eq!(prompts.len(), 3, "两个 run、首个 run 两 turn");
    let first = prompts[0].as_deref().expect("prompt 0");
    assert_eq!(
        prompts[1].as_deref(),
        Some(first),
        "同一 run 的 turn 间冻结"
    );
    assert_eq!(prompts[2].as_deref(), Some(first), "同一会话跨 run 冻结");
    teardown(&booted).await;
}

#[tokio::test]
async fn subagent_run_sends_trimmed_context_and_observation_prefers_main() {
    let booted = boot_with_fixtures("subagent", vec![final_turn("done")]).await;
    // 先在 alpha 上跑主形态（缓存主视图），再在 beta 上只跑子代理形态。
    booted
        .state
        .sessions()
        .execute_for(
            booted.state.storage().as_ref(),
            booted.state.events(),
            booted.state.runs(),
            &owner_principal(),
            "sess_local_alpha",
            "主会话",
            NOW_MS,
        )
        .await
        .expect("main run drives");
    let run_id =
        lingxi_adapters::storage::RunDatabase::allocate_run_id(booted.state.storage(), NOW_MS)
            .expect("allocate run id");
    let authorization = DriveAuthorization {
        lineage: RunLineage {
            parent_run_id: None,
            origin: RunOrigin::Subagent,
            source_message_id: None,
            cause_id: None,
        },
        grant: RunGrant::Subagent {
            tier: ToolAccessTier::Operate,
        },
        session_mode: lingxi_kernel::subagent::SessionPermissionMode::Operate,
    };
    let finish = booted
        .state
        .runs()
        .drive_run(
            booted.state.storage().as_ref(),
            booted.state.events(),
            &lingxi_kernel::Principal::LocalUser,
            "sess_local_beta",
            "lingxi",
            &run_id,
            "child task",
            1,
            NOW_MS,
            None,
            None,
            authorization,
            None,
            "sess_local_beta::subagent::thread-t01",
        )
        .await
        .expect("subagent run drives");
    assert!(
        matches!(finish, lingxi_kernel::RunFinish::CompletedWithFinal { .. }),
        "子代理 run 应完成: {finish:?}"
    );

    let prompts = booted.provider.prompts();
    assert_eq!(prompts.len(), 2);
    let main_sent = prompts[0].as_deref().expect("main prompt");
    let sub_sent = prompts[1].as_deref().expect("subagent prompt");
    // 子代理裁剪：人格保留，记忆/花名册/外观/子代理协作段剥离。
    assert!(sub_sent.contains("YUAN-TOP_SECRET"), "人格段保留");
    assert!(sub_sent.starts_with("你运行在灵犀（Lingxi）平台上。"));
    for marker in [
        "MEMORY-TOP_SECRET",
        "APPEARANCE-TOP_SECRET",
        "TENET-TOP_SECRET",
    ] {
        assert!(!sub_sent.contains(marker), "子代理上下文不得含 {marker}");
        assert!(main_sent.contains(marker), "主上下文应含 {marker}");
    }
    // 观测读面：beta 只有子代理形态 → 返回裁剪视图；alpha 是主视图。
    let compiler = booted.state.context_compiler().expect("compiler");
    let beta_view = compiler
        .observation_for_session("sess_local_beta")
        .expect("beta observation");
    assert_eq!(beta_view["forSubagent"], Value::Bool(true));
    let beta_ids: Vec<&str> = beta_view["segments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert!(!beta_ids.iter().any(|id| id.starts_with("memory.")));
    let alpha_view = compiler
        .observation_for_session("sess_local_alpha")
        .expect("alpha observation");
    assert_eq!(alpha_view["forSubagent"], Value::Bool(false));
    teardown(&booted).await;
}

#[tokio::test]
async fn budget_exceeded_fails_the_run_loudly() {
    let booted = boot_with_fixtures("budget", vec![final_turn("done")]).await;
    // user.profile 预算 400 token：写入远超预算的用户档案（5000 个 CJK ≈ 5500）。
    write(&booted.layout_home.join("user/user.md"), &"档".repeat(5000));
    let run_id =
        lingxi_adapters::storage::RunDatabase::allocate_run_id(booted.state.storage(), NOW_MS)
            .expect("allocate run id");
    let finish = booted
        .state
        .runs()
        .drive_run(
            booted.state.storage().as_ref(),
            booted.state.events(),
            &lingxi_kernel::Principal::LocalUser,
            "sess_local_alpha",
            "lingxi",
            &run_id,
            "触发预算",
            1,
            NOW_MS,
            None,
            None,
            DriveAuthorization::user_submission(
                None,
                lingxi_kernel::subagent::SessionPermissionMode::Operate,
            ),
            None,
            "sess_local_alpha",
        )
        .await
        .expect("drive returns");
    match finish {
        lingxi_kernel::RunFinish::Failed { cause } => {
            let msg = format!("{cause:?}");
            assert!(
                msg.contains("context_compile:"),
                "失败码须含 context_compile 前缀: {msg}"
            );
            assert!(msg.contains("user.profile"), "失败须点名超预算段: {msg}");
        }
        other => panic!("超预算必须使 run 响亮失败，实际: {other:?}"),
    }
    // 从未发送过任何请求 → 观测不存在（404 语义的服务内形态）。
    assert!(booted
        .state
        .context_compiler()
        .expect("compiler")
        .observation_for_session("sess_local_alpha")
        .is_none());
    assert!(
        booted.provider.prompts().is_empty(),
        "预算失败前不得发出模型调用"
    );
    teardown(&booted).await;
}

#[tokio::test]
async fn missing_yuan_fails_the_run_loudly() {
    // 产品目录没有 yuan 模板（也不回落出非空 yuan）。
    let home = fresh_dir("boot-no-yuan");
    let product = fresh_dir("boot-no-yuan-product");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let layout = prepare_layout(&home).expect("layout");
    write_home_fixture(&layout.home);
    write(&product.join("identity-templates/lingxi.md"), "IDENTITY-ZH");
    write(&product.join("agents-templates/lingxi.md"), "AGENTS-ZH");
    let provider = RecordingProvider::new(vec![final_turn("done")]);
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
    let run_id = lingxi_adapters::storage::RunDatabase::allocate_run_id(state.storage(), NOW_MS)
        .expect("allocate run id");
    let finish = state
        .runs()
        .drive_run(
            state.storage().as_ref(),
            state.events(),
            &lingxi_kernel::Principal::LocalUser,
            "sess_local_alpha",
            "lingxi",
            &run_id,
            "人格缺失",
            1,
            NOW_MS,
            None,
            None,
            DriveAuthorization::user_submission(
                None,
                lingxi_kernel::subagent::SessionPermissionMode::Operate,
            ),
            None,
            "sess_local_alpha",
        )
        .await
        .expect("drive returns");
    match finish {
        lingxi_kernel::RunFinish::Failed { cause } => {
            let msg = format!("{cause:?}");
            assert!(msg.contains("context_compile:"), "须含前缀: {msg}");
            assert!(
                msg.contains("persona source incomplete"),
                "须保留人格缺失语义: {msg}"
            );
        }
        other => panic!("yuan 缺失必须使 run 响亮失败，实际: {other:?}"),
    }
    assert!(provider.prompts().is_empty());
    state.storage().close().await.expect("close");
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&product);
}
