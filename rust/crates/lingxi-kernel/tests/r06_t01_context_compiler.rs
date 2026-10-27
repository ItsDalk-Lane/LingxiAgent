//! R06-T01 上下文编译器验收测试（kernel 侧）。
//!
//! 锁定对象：现役 `core/agent.ts::buildSystemPromptArtifact` 的段组成、顺序、
//! 文案（zh/en 逐字）、token 估算口径与 golden 夹具（tests/fixtures/
//! system-prompt-golden-{zh,en}.txt，由现役 vitest 等价测试同一份输入产出）。
//!
//! 验收锚点：
//! - R06-A01 同源：artifact.render() 即发送文本；观测视图只携带 Full 段正文，
//!   DigestOnly 段无任何内容副本。
//! - R06-A02 不暗增：每段冻结 token 预算（派生自现役实测，见
//!   artifacts/rust-tauri/R06/T01/incumbent-budget-measurement.raw.txt）；
//!   闭合段注册表（新增/重复段即编译或锁测试失败）。

use lingxi_kernel::context::{
    estimate_text_tokens, ContextCompileInput, ContextCompiler, ProjectContextFile, RosterEntry,
    SkillCatalogEntry, TOTAL_SYSTEM_TOKEN_BUDGET,
};
use serde_json::Value;

const GOLDEN_ZH: &str = include_str!("../../../../tests/fixtures/system-prompt-golden-zh.txt");
const GOLDEN_EN: &str = include_str!("../../../../tests/fixtures/system-prompt-golden-en.txt");

const SESSION_LABEL: &str = "Thursday, June 4, 2026 at 15:53 GMT+8";

/// 与 tests/agent-system-prompt-equivalence.test.ts 的 makeAgent 完全同料的输入。
fn golden_input(locale: &str) -> ContextCompileInput {
    let zh = locale.starts_with("zh");
    ContextCompileInput {
        locale: locale.to_string(),
        for_subagent: false,
        memory_enabled: true,
        user_name: if zh { "黎" } else { "Li" }.to_string(),
        resolved_user_name: if zh { "用户" } else { "User" }.to_string(),
        agent_name: "Hanako".to_string(),
        agent_id: "hana".to_string(),
        environment_note: "FIXED-PLATFORM-NOTE-LINE".to_string(),
        user_profile_md: Some("PROFILE-TOP_SECRET 简介𝐀\n".to_string()),
        identity_md: String::new(),
        yuan_md: "AGENTSMD-TEMPLATE-TOP_SECRET_PERSONA 你是{{userName}}的伙伴🎉".to_string(),
        agents_md: String::new(),
        inject_appearance: true,
        appearance_summary: Some("APPEARANCE-TOP_SECRET 样貌".to_string()),
        computer_use_available: true,
        learn_skills_enabled: true,
        proactive_delegation: false,
        roster: vec![
            RosterEntry {
                id: "hana".to_string(),
                name: "Hanako".to_string(),
                model: Some("gpt-test".to_string()),
                summary: Some("主 agent TOP_SECRET".to_string()),
            },
            RosterEntry {
                id: "beta".to_string(),
                name: "Beta".to_string(),
                model: Some("claude-test".to_string()),
                summary: Some("副 agent".to_string()),
            },
        ],
        memory_md: "MEMORY-TOP_SECRET 记忆🇨🇳\n".to_string(),
        tenets: vec!["PINNED-TOP_SECRET 置顶".to_string()],
        session_started_label: SESSION_LABEL.to_string(),
        append_system_lines: Vec::new(),
        project_context_files: Vec::new(),
        skills: Vec::new(),
        cwd: "/tmp/r06-t01-ws".to_string(),
    }
}

fn segment_ids(artifact: &lingxi_kernel::context::ContextArtifact) -> Vec<&'static str> {
    artifact.segments().iter().map(|s| s.id).collect()
}

#[test]
fn golden_preamble_zh_byte_equal() {
    let artifact = ContextCompiler::compile(&golden_input("zh-CN")).expect("compile zh");
    assert_eq!(
        artifact.preamble_text(),
        GOLDEN_ZH,
        "zh preamble 必须与现役 golden 逐字节一致"
    );
    // golden 文件本身无尾换行；preamble 也不得有。
    assert!(!artifact.preamble_text().ends_with('\n'));
    // 段顺序 = 现役冻结顺序（provenance 源 id 数组，见等价测试断言）。
    assert_eq!(
        segment_ids(&artifact),
        vec![
            "platform.intro",
            "platform.environment",
            "user.profile",
            "persona",
            "agent.appearance",
            "platform.output-discipline",
            "platform.tool-discipline",
            "platform.session-files",
            "platform.ui-context",
            "platform.subagent-collaboration",
            "platform.computer-use",
            "platform.action-discipline",
            "platform.web-tool-priority",
            "platform.learn-skills",
            "platform.skill-usage",
            "agent.roster",
            "memory.rules",
            "memory.tenets",
            "memory.longterm",
            "session.time",
            // wrapper 带（SDK 位置等价物）：cwd 恒在。
            "session.cwd",
        ]
    );
    // token 口径 = 现役实测（zh 总 1747 / 静态带 1462）。
    assert_eq!(artifact.preamble_tokens(), 1747);
    assert_eq!(artifact.static_band_tokens(), 1462);
    // 段 span 平铺全文本：preamble 内分隔 1 字节（\n），跨带 2 字节（\n\n）。
    let text = artifact.render();
    let mut cursor = 0usize;
    let mut prev_band_preamble = true;
    for seg in artifact.segments() {
        let gap = if prev_band_preamble && seg.band_is_preamble() {
            1
        } else {
            2
        };
        if cursor == 0 {
            assert_eq!(seg.byte_start, 0);
        } else {
            assert_eq!(seg.byte_start, cursor + gap, "段 {} 的 span 未平铺", seg.id);
        }
        assert!(seg.byte_end > seg.byte_start);
        cursor = seg.byte_end;
        prev_band_preamble = seg.band_is_preamble();
    }
    assert_eq!(cursor, text.len());
    // persona / memory 段内容定位（等价测试的 slice 断言）。
    let persona = artifact
        .segments()
        .iter()
        .find(|s| s.id == "persona")
        .expect("persona segment");
    assert!(
        text[persona.byte_start..persona.byte_end].contains("AGENTSMD-TEMPLATE-TOP_SECRET_PERSONA")
    );
    let longterm = artifact
        .segments()
        .iter()
        .find(|s| s.id == "memory.longterm")
        .expect("memory segment");
    assert!(text[longterm.byte_start..longterm.byte_end].contains("MEMORY-TOP_SECRET 记忆🇨🇳"));
}

#[test]
fn golden_preamble_en_byte_equal() {
    let artifact = ContextCompiler::compile(&golden_input("en")).expect("compile en");
    assert_eq!(artifact.preamble_text(), GOLDEN_EN);
    assert_eq!(artifact.preamble_tokens(), 1529);
    assert_eq!(artifact.static_band_tokens(), 1299);
}

/// 每段 token = 现役实测值（artifacts/rust-tauri/R06/T01 测量记录）。
#[test]
fn per_segment_tokens_match_incumbent_measurement() {
    let zh = ContextCompiler::compile(&golden_input("zh-CN")).expect("compile zh");
    let expected_zh: Vec<(&str, u32)> = vec![
        ("platform.intro", 15),
        ("platform.environment", 13),
        ("user.profile", 38),
        ("persona", 18),
        ("agent.appearance", 14),
        ("platform.output-discipline", 28),
        ("platform.tool-discipline", 373),
        ("platform.session-files", 163),
        ("platform.ui-context", 62),
        ("platform.subagent-collaboration", 124),
        ("platform.computer-use", 53),
        ("platform.action-discipline", 167),
        ("platform.web-tool-priority", 56),
        ("platform.learn-skills", 110),
        ("platform.skill-usage", 141),
        ("agent.roster", 91),
        ("memory.rules", 132),
        ("memory.tenets", 44),
        ("memory.longterm", 31),
        ("session.time", 79),
    ];
    for (id, tokens) in expected_zh {
        let seg = zh.segments().iter().find(|s| s.id == id).expect(id);
        assert_eq!(seg.tokens, tokens, "zh 段 {id} token 与现役实测不符");
    }
    let en = ContextCompiler::compile(&golden_input("en")).expect("compile en");
    let expected_en: Vec<(&str, u32)> = vec![
        ("platform.intro", 13),
        ("platform.environment", 12),
        ("user.profile", 31),
        ("persona", 17),
        ("agent.appearance", 13),
        ("platform.output-discipline", 22),
        ("platform.tool-discipline", 330),
        ("platform.session-files", 148),
        ("platform.ui-context", 58),
        ("platform.subagent-collaboration", 112),
        ("platform.computer-use", 49),
        ("platform.action-discipline", 143),
        ("platform.web-tool-priority", 46),
        ("platform.learn-skills", 93),
        ("platform.skill-usage", 137),
        ("agent.roster", 78),
        ("memory.rules", 98),
        ("memory.tenets", 36),
        ("memory.longterm", 28),
        ("session.time", 70),
    ];
    for (id, tokens) in expected_en {
        let seg = en.segments().iter().find(|s| s.id == id).expect(id);
        assert_eq!(seg.tokens, tokens, "en 段 {id} token 与现役实测不符");
    }
}

#[test]
fn estimator_matches_incumbent_vectors() {
    // 与 lib/llm/estimate-text-tokens.ts 同口径：CJK×1.1，其余 /4，ceil；
    // 代理对（emoji/𝐀/🇨🇳）按码点计一次。
    assert_eq!(estimate_text_tokens(""), 0);
    assert_eq!(estimate_text_tokens("abcd"), 1);
    assert_eq!(estimate_text_tokens("你"), 2); // ceil(1.1)
    assert_eq!(estimate_text_tokens("你运行在灵犀（Lingxi）平台上。"), 15);
    // 23 码点：CJK 2（简介/记忆），其余 21 → ceil(2.2 + 5.25) = 8。
    assert_eq!(estimate_text_tokens("PROFILE-TOP_SECRET 简介𝐀\n"), 8);
    assert_eq!(estimate_text_tokens("MEMORY-TOP_SECRET 记忆🇨🇳\n"), 8);
}

#[test]
fn subagent_trim_drops_memory_roster_appearance() {
    let mut input = golden_input("zh-CN");
    input.for_subagent = true;
    let artifact = ContextCompiler::compile(&input).expect("compile subagent");
    let ids = segment_ids(&artifact);
    for dropped in [
        "agent.appearance",
        "platform.subagent-collaboration",
        "agent.roster",
        "memory.rules",
        "memory.tenets",
        "memory.longterm",
    ] {
        assert!(!ids.contains(&dropped), "子代理裁剪不应含 {dropped}");
    }
    assert!(artifact
        .render()
        .starts_with("你运行在灵犀（Lingxi）平台上。"));
    // 观测视图类别断言（等价测试的 category 维度）。
    let view = artifact.observation_view();
    let cats: Vec<&str> = view["segments"]
        .as_array()
        .expect("segments array")
        .iter()
        .map(|s| s["category"].as_str().expect("category"))
        .collect();
    assert!(!cats.contains(&"memory_context"));
    assert!(!cats.contains(&"agent_roster"));
}

#[test]
fn memory_disabled_drops_memory_band() {
    let mut input = golden_input("zh-CN");
    input.memory_enabled = false;
    let artifact = ContextCompiler::compile(&input).expect("compile");
    let ids = segment_ids(&artifact);
    assert!(!ids.iter().any(|id| id.starts_with("memory.")));
}

#[test]
fn empty_memory_and_no_tenets_drops_whole_memory_band() {
    let mut input = golden_input("zh-CN");
    input.memory_md = String::new();
    input.tenets = Vec::new();
    let artifact = ContextCompiler::compile(&input).expect("compile");
    let ids = segment_ids(&artifact);
    assert!(
        !ids.iter().any(|id| id.starts_with("memory.")),
        "无记忆内容时规则段也不注入（现役同规则）"
    );
}

#[test]
fn placeholder_memory_text_counts_as_empty() {
    let mut input = golden_input("zh-CN");
    input.memory_md = "（暂无记忆）".to_string();
    input.tenets = Vec::new();
    let artifact = ContextCompiler::compile(&input).expect("compile");
    assert!(!segment_ids(&artifact)
        .iter()
        .any(|id| id.starts_with("memory.")));
}

#[test]
fn missing_yuan_is_a_loud_error() {
    let mut input = golden_input("zh-CN");
    input.yuan_md = String::new();
    let err = ContextCompiler::compile(&input).expect_err("yuan 缺失必须响亮失败");
    assert_eq!(
        err.to_string(),
        "persona source incomplete: yuan template is empty (agent id hana)"
    );
}

#[test]
fn budget_exceeded_is_a_loud_error() {
    let mut input = golden_input("zh-CN");
    // user.profile 预算 400 tokens；构造 ~5000 CJK 字符（≈5500 tokens）的材料。
    input.user_profile_md = Some("档".repeat(5000));
    let err = ContextCompiler::compile(&input).expect_err("超预算必须响亮失败");
    let msg = err.to_string();
    assert!(msg.contains("user.profile"), "错误须点名超预算段：{msg}");
    assert!(msg.contains("budget"), "错误须含预算语义：{msg}");
}

#[test]
fn registry_is_frozen_at_24_segments() {
    // 闭合注册表锁：任何新增/删除/重排段都会使本测试失败（A02「新增冗余段」
    // 的机器防线；golden 字节等价测试同时锁内容）。
    let ids: Vec<&'static str> = lingxi_kernel::context::SEGMENT_SPECS
        .iter()
        .map(|spec| spec.id)
        .collect();
    assert_eq!(
        ids,
        vec![
            "platform.intro",
            "platform.environment",
            "user.profile",
            "persona",
            "agent.appearance",
            "platform.output-discipline",
            "platform.tool-discipline",
            "platform.session-files",
            "platform.ui-context",
            "platform.subagent-collaboration",
            "platform.computer-use",
            "platform.action-discipline",
            "platform.web-tool-priority",
            "platform.learn-skills",
            "platform.skill-usage",
            "agent.roster",
            "memory.rules",
            "memory.tenets",
            "memory.longterm",
            "session.time",
            "session.append-system",
            "session.project-context",
            "skill.catalog",
            "session.cwd",
        ]
    );
    // 每段预算为正且总量上限 = 段预算之和。
    let sum: u32 = lingxi_kernel::context::SEGMENT_SPECS
        .iter()
        .map(|spec| spec.token_budget)
        .sum();
    assert_eq!(sum, TOTAL_SYSTEM_TOKEN_BUDGET);
    for spec in lingxi_kernel::context::SEGMENT_SPECS {
        assert!(spec.token_budget > 0, "段 {} 预算必须为正", spec.id);
    }
}

#[test]
fn static_prefix_stable_under_dynamic_drift() {
    let a = ContextCompiler::compile(&golden_input("zh-CN")).expect("compile a");
    let mut input_b = golden_input("zh-CN");
    input_b.memory_md = "另一条记忆内容，完全不同的长度。\n".to_string();
    input_b.tenets = vec!["原则甲".to_string(), "原则乙\n续行".to_string()];
    input_b.session_started_label = "Friday, June 5, 2026 at 09:01 GMT+8".to_string();
    let b = ContextCompiler::compile(&input_b).expect("compile b");
    assert_eq!(
        a.static_prefix(),
        b.static_prefix(),
        "动态材料漂移不得改变静态前缀（prefix cache 保护）"
    );
    assert_eq!(
        a.static_prefix().len(),
        a.render()[..a.static_prefix_byte_end()].len()
    );
}

#[test]
fn observation_view_carries_no_digest_only_content() {
    let artifact = ContextCompiler::compile(&golden_input("zh-CN")).expect("compile");
    let view: Value = artifact.observation_view();
    let serialized = serde_json::to_string(&view).expect("serialize");
    for marker in [
        "TOP_SECRET_PERSONA",
        "MEMORY-TOP_SECRET",
        "PROFILE-TOP_SECRET",
        "APPEARANCE-TOP_SECRET",
    ] {
        assert!(
            !serialized.contains(marker),
            "观测视图不得携带 digest-only 段正文（{marker}）"
        );
    }
    // Full 披露段（平台固定文案）在视图里有正文；digest-only 段没有 text 字段。
    let segments = view["segments"].as_array().expect("segments array");
    let intro = segments
        .iter()
        .find(|s| s["id"] == "platform.intro")
        .expect("intro");
    assert_eq!(intro["text"], "你运行在灵犀（Lingxi）平台上。");
    let persona = segments
        .iter()
        .find(|s| s["id"] == "persona")
        .expect("persona");
    assert!(persona.get("text").is_none(), "persona 段为 digest-only");
    assert_eq!(persona["disclosure"], "digest_only");
}

#[test]
fn wrapper_bands_render_sdk_shape() {
    let mut input = golden_input("zh-CN");
    input.append_system_lines = vec!["附加行一".to_string(), "附加行二".to_string()];
    input.project_context_files = vec![
        ProjectContextFile {
            path: "AGENTS.md".to_string(),
            content: "项目指引<一>".to_string(),
        },
        ProjectContextFile {
            path: "docs/AGENTS.md".to_string(),
            content: "子目录指引 & 注".to_string(),
        },
    ];
    input.skills = vec![SkillCatalogEntry {
        name: "demo<skill>".to_string(),
        description: "演示 & 技能".to_string(),
        location: "/skills/demo/SKILL.md".to_string(),
    }];
    input.cwd = "C:\\work\\space".to_string();
    let artifact = ContextCompiler::compile(&input).expect("compile");
    let text = artifact.render();
    let preamble = artifact.preamble_text();
    assert!(text.starts_with(preamble));
    let tail = &text[preamble.len()..];
    let expected_tail = concat!(
        "\n\n<addendum>\n附加行一\n\n附加行二\n</addendum>",
        "\n\n<project_context>\nProject-specific instructions and guidelines:\n\n",
        "<project_instructions path=\"AGENTS.md\">\n项目指引<一>\n</project_instructions>\n\n",
        "<project_instructions path=\"docs/AGENTS.md\">\n子目录指引 & 注\n</project_instructions>\n</project_context>",
        "\n\n<skills>\nThe following skills provide specialized instructions for specific tasks.\n",
        "Use the read tool to load a skill's file when the task matches its description.\n",
        "When a skill file references a relative path, resolve it against the skill directory ",
        "(parent of SKILL.md / dirname of the path) and use that absolute path in tool commands.\n\n",
        "<available_skills>\n  <skill>\n    <name>demo&lt;skill&gt;</name>\n",
        "    <description>演示 &amp; 技能</description>\n",
        "    <location>/skills/demo/SKILL.md</location>\n  </skill>\n</available_skills>\n</skills>",
        "\n\n<cwd>\nC:/work/space\n</cwd>",
    );
    assert_eq!(tail, expected_tail, "wrapper 带必须与 SDK 拼接形状逐字一致");
}

#[test]
fn empty_wrapper_bands_are_omitted_but_cwd_stays() {
    let artifact = ContextCompiler::compile(&golden_input("en")).expect("compile");
    let text = artifact.render();
    assert!(!text.contains("<addendum>"));
    assert!(!text.contains("<project_context>"));
    assert!(!text.contains("<skills>"));
    assert!(text.ends_with("<cwd>\n/tmp/r06-t01-ws\n</cwd>"));
}

#[test]
fn roster_format_matches_incumbent() {
    let artifact = ContextCompiler::compile(&golden_input("zh-CN")).expect("compile");
    let text = artifact.render();
    assert!(text.contains("- `hana`（Hanako）（你） [gpt-test] — 主 agent TOP_SECRET"));
    assert!(text.contains("- `beta`（Beta） [claude-test] — 副 agent"));
    let en = ContextCompiler::compile(&golden_input("en")).expect("compile en");
    assert!(en
        .render()
        .contains("- `hana`（Hanako） (you) [gpt-test] — 主 agent TOP_SECRET"));
}

#[test]
fn roster_absent_when_no_other_agents() {
    let mut input = golden_input("zh-CN");
    input.roster = vec![input.roster[0].clone()];
    let artifact = ContextCompiler::compile(&input).expect("compile");
    assert!(!segment_ids(&artifact).contains(&"agent.roster"));
}

#[test]
fn tenets_render_with_continuation_indent() {
    let mut input = golden_input("zh-CN");
    input.tenets = vec!["第一原则\n续行内容".to_string()];
    let artifact = ContextCompiler::compile(&input).expect("compile");
    let seg = artifact
        .segments()
        .iter()
        .find(|s| s.id == "memory.tenets")
        .expect("tenets");
    let text = artifact.render();
    assert_eq!(
        &text[seg.byte_start..seg.byte_end],
        "# 置顶与原则\n以下是用户钉住的内容与经用户确认的行为原则，始终遵守：\n- 第一原则\n  续行内容"
    );
}

#[test]
fn appearance_sanitization_drops_external_description() {
    let mut input = golden_input("zh-CN");
    input.appearance_summary = Some("这张图片里是一个角色".to_string());
    let artifact = ContextCompiler::compile(&input).expect("compile");
    assert!(
        !segment_ids(&artifact).contains(&"agent.appearance"),
        "外部观察口吻的样貌摘要必须被丢弃（现役 sanitize 语义）"
    );
}

#[test]
fn total_budget_constant_matches_spec_sum() {
    assert_eq!(TOTAL_SYSTEM_TOKEN_BUDGET, 25_068);
}
