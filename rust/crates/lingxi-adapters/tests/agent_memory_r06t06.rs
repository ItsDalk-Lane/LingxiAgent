//! R06-T06 adapters 测试：per-agent 记忆存储（A11 存储层隔离）+ 编译记忆 +
//! 经验/梦修订 + config patcher + PII 脱敏 + 技能库（A12 审核层存储面）。
//!
//! 对照纪律（RC-3）：金样与语义逐条锚定现役文件（锚点见 01_design §3）；
//! 禁恒真断言。

use std::path::{Path, PathBuf};

use lingxi_adapters::storage::agent_memory as am;
use lingxi_adapters::storage::skill_library as sl;
use serde_json::{json, Value};

fn uniq(tag: &str) -> String {
    format!(
        "lingxi-r06t06-{}-{}-{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

fn temp_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(uniq(tag));
    std::fs::create_dir_all(&dir).expect("mkdir home");
    dir
}

fn make_agent(home: &Path, id: &str) -> PathBuf {
    let agent_dir = home.join("agents").join(id);
    std::fs::create_dir_all(agent_dir.join("memory")).expect("mkdir agent");
    std::fs::write(
        agent_dir.join("config.yaml"),
        "# LingxiAgent 系统配置\nlocale: zh\nagent:\n  name: 测试助手\nmemory:\n  enabled: true\n",
    )
    .expect("write config");
    agent_dir
}

fn fact_insert<'a>(fact: &'a str, tags: &'a [String]) -> am::FactInsert<'a> {
    am::FactInsert {
        fact,
        tags,
        time: None,
        session_id: Some("sess-1"),
    }
}

// ── A11（存储层）：物理 per-agent 隔离 ──────────────────────────────────────

#[test]
fn a11_facts_are_physically_isolated_per_agent() {
    let home = temp_home("a11-facts");
    make_agent(&home, "agentA");
    make_agent(&home, "agentB");
    let roots_a = am::resolve_agent_roots(&home.join("agents"), "agentA").unwrap();
    let roots_b = am::resolve_agent_roots(&home.join("agents"), "agentB").unwrap();
    assert_ne!(
        roots_a.memory_dir, roots_b.memory_dir,
        "独立目录（现役 core/agent.ts:223）"
    );

    let store_a = am::FactStore::open(&roots_a.memory_dir.join("facts.db")).unwrap();
    let store_b = am::FactStore::open(&roots_b.memory_dir.join("facts.db")).unwrap();

    let tags_a = vec!["偏好".to_string()];
    store_a
        .add(&fact_insert("AgentA 独有的事实：喜欢 Vim", &tags_a))
        .unwrap();
    store_a
        .add(&fact_insert("AgentA 另一条：咖啡因依赖", &[]))
        .unwrap();
    store_b
        .add(&fact_insert("AgentB 独有的事实：喜欢 Emacs", &[]))
        .unwrap();

    // 分别询问：只读所属范围
    let all_a = store_a.get_all().unwrap();
    let all_b = store_b.get_all().unwrap();
    assert_eq!(all_a.len(), 2);
    assert_eq!(all_b.len(), 1);
    assert!(all_a.iter().all(|f| f.fact.contains("AgentA")));
    assert!(all_b.iter().all(|f| f.fact.contains("AgentB")));

    // 检索（FTS + CJK 兜底）也隔离
    let hits_a = store_a.search_full_text("Vim", 10).unwrap();
    let hits_b = store_b.search_full_text("Vim", 10).unwrap();
    assert_eq!(hits_a.len(), 1);
    assert!(hits_b.is_empty(), "B 的检索绝不命中 A 的事实");
    let hits_a_cjk = store_a.search_full_text("咖啡因", 10).unwrap();
    assert_eq!(hits_a_cjk.len(), 1, "CJK 词走 LIKE 兜底路径");

    // 写入（清空）也隔离：B 清空不动 A
    store_b.clear_all().unwrap();
    assert_eq!(store_b.size().unwrap(), 0);
    assert_eq!(store_a.size().unwrap(), 2, "B 的清空绝不清 A");

    // import 只进目标库
    let tags = vec!["导入".to_string()];
    store_a
        .import_all(&[fact_insert("导入 A 的事实", &tags)])
        .unwrap();
    assert_eq!(store_a.size().unwrap(), 3);
    assert_eq!(store_b.size().unwrap(), 0);

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn a11_tenets_are_isolated_per_agent() {
    let home = temp_home("a11-tenets");
    make_agent(&home, "agentA");
    make_agent(&home, "agentB");
    let store_a = am::TenetsStore::new(&home.join("agents").join("agentA").join("memory"));
    let store_b = am::TenetsStore::new(&home.join("agents").join("agentB").join("memory"));

    store_a
        .add_direct("永远用中文回复", None, "pin-1".into())
        .unwrap();
    store_a
        .add_proposal("提案原则", None, None, "tenet-1".into())
        .unwrap();

    // B 读不到 A 的条目；B 的 clear 不动 A
    assert_eq!(store_a.list().tenets.len(), 2);
    assert_eq!(store_b.list().tenets.len(), 0);
    store_b
        .add_direct("B 自己的原则", None, "pin-2".into())
        .unwrap();
    assert_eq!(store_b.list().tenets.len(), 1);

    // decide 只影响目标 agent
    let decided = store_a.decide("tenet-1", true).unwrap();
    assert_eq!(decided.status, lingxi_kernel::memory::TenetStatus::Active);
    assert!(store_b.list().tenets.iter().all(|t| t.id != "tenet-1"));

    // JSON 金样：schemaVersion + 归一化内容（tenets.ts 序列化契约）
    let raw = std::fs::read_to_string(store_a.file_path()).unwrap();
    let value: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(value["schemaVersion"], 1);
    assert!(value["tenets"].as_array().unwrap().len() == 2);

    // 严格读：损坏文件响亮拒绝（tenets.ts readTenetsFileStrict）
    std::fs::write(store_b.file_path(), "{not json").unwrap();
    let err = store_b.read_strict().unwrap_err();
    assert_eq!(err.tenet_code(), Some("TENET_STORE_CORRUPTED"));
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn a11_cross_agent_ids_are_rejected() {
    let home = temp_home("a11-ids");
    make_agent(&home, "agentA");
    for bad in ["../agentA", "agentA/../../etc", "a.b", "", "a\\b"] {
        let err = am::resolve_agent_roots(&home.join("agents"), bad).unwrap_err();
        assert!(
            matches!(err, am::MemoryError::InvalidRequest(_)),
            "path-shaped id {bad:?} rejected (config.ts:425)"
        );
    }
    let err = am::resolve_agent_roots(&home.join("agents"), "ghost").unwrap_err();
    assert!(
        matches!(err, am::MemoryError::AgentNotFound(_)),
        "未知 agent 404 语义"
    );
    let _ = std::fs::remove_dir_all(&home);
}

// ── compiled memory（compile.ts 金样）──────────────────────────────────────

#[test]
fn compiled_sections_assemble_memory_md_golden() {
    let home = temp_home("compiled");
    let agent = make_agent(&home, "agentA");
    let store = am::CompiledMemoryStore::new(&agent.join("memory"), true);

    store.write_facts("- 事实一\n- 事实二").unwrap();
    store.write_today("今天做了什么").unwrap();
    // week 由 daily 装配：写两天
    let _ = store.list_week_days().unwrap();
    std::fs::create_dir_all(agent.join("memory").join("daily")).unwrap();
    std::fs::write(
        agent.join("memory").join("daily").join("2026-01-01.md"),
        "元旦日记\n",
    )
    .unwrap();
    std::fs::write(
        agent.join("memory").join("daily").join("2026-01-02.md"),
        "第二天日记\n",
    )
    .unwrap();
    store.assemble_week().unwrap();
    store.write_longterm("长期背景").unwrap();

    let content = store.read_assembled().unwrap();
    assert!(content.starts_with("## 重要事实\n\n- 事实一\n- 事实二\n\n"));
    assert!(content.contains("## 今天\n\n今天做了什么"));
    assert!(content.contains("## 本周早些时候\n\n元旦日记\n\n第二天日记"));
    assert!(content.contains("## 长期情况\n\n长期背景"));
    assert!(content.ends_with('\n'));

    // memory.md 落盘与装配一致（assemble 语义）
    let on_disk = std::fs::read_to_string(agent.join("memory").join("memory.md")).unwrap();
    assert_eq!(on_disk, content);

    // 空栏占位符（buildCompiledMemoryMarkdown）
    let store2 = am::CompiledMemoryStore::new(&agent.join("memory"), false);
    std::fs::remove_file(agent.join("memory").join("facts.md")).unwrap();
    let content_en = store2.read_assembled().unwrap();
    assert!(content_en.contains("## Key facts\n\n(none)"));
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn compiled_reset_and_clear_and_legacy_migration() {
    let home = temp_home("compiled-reset");
    let agent = make_agent(&home, "agentA");
    let store = am::CompiledMemoryStore::new(&agent.join("memory"), true);
    store.write_facts(" Facts ").unwrap();
    assert!(store.read_reset_at().is_none());
    store.write_reset_marker().unwrap();
    assert!(store.read_reset_at().is_some());
    store.clear_artifacts().unwrap();
    for name in [
        "memory.md",
        "facts.md",
        "today.md",
        "week.md",
        "longterm.md",
    ] {
        let text = std::fs::read_to_string(agent.join("memory").join(name)).unwrap();
        assert!(text.is_empty(), "{name} cleared");
    }
    // legacy editable-facts 迁移（compile.ts:905-955）
    std::fs::write(
        agent.join("memory").join("editable-facts.md"),
        "Editable 独有条目\n",
    )
    .unwrap();
    std::fs::write(agent.join("memory").join("facts.md"), "已有条目\n").unwrap();
    let result = store.migrate_legacy_editable_facts().unwrap();
    assert_eq!(result["migrated"], true);
    assert_eq!(result["reason"], "merged");
    assert!(!agent.join("memory").join("editable-facts.md").exists());
    let merged = std::fs::read_to_string(agent.join("memory").join("facts.md")).unwrap();
    assert!(merged.contains("Editable 独有条目") && merged.contains("已有条目"));
    // 幂等
    let again = store.migrate_legacy_editable_facts().unwrap();
    assert_eq!(again["migrated"], false);
    let _ = std::fs::remove_dir_all(&home);
}

// ── experience + dream revisions ────────────────────────────────────────────

#[test]
fn experience_record_and_index_and_feedback_shape() {
    let home = temp_home("experience");
    let agent = make_agent(&home, "agentA");
    let store = am::ExperienceStore::new(&agent);

    let (added, reason) = store
        .record_entry("用户认可的做法", "[2026-01-01] ✓ 先读文档")
        .unwrap();
    assert!(added);
    assert_eq!(reason, None);
    // 去重（experience.ts:80-82 substring dedup）
    let (added2, reason2) = store
        .record_entry("用户认可的做法", "[2026-01-01] ✓ 先读文档")
        .unwrap();
    assert!(!added2);
    assert_eq!(reason2.as_deref(), Some("duplicate"));
    // 编号递增（experience.ts:84-91）
    store.record_entry("用户认可的做法", "第二条").unwrap();

    let (merged, _files) = store.read_merged();
    assert!(merged.contains("1. [2026-01-01] ✓ 先读文档"));
    assert!(merged.contains("2. 第二条"));

    let index = store.index_text();
    assert!(
        index.contains("# 用户认可的做法（2 条）"),
        "索引由 rebuildIndex 生成（experience.ts:54）: {index}"
    );
    assert!(index.contains("→ experience/"));

    // 非法分类拒绝（experience.ts normalizeExperienceCategory）
    let err = store.record_entry("../escape", "x").unwrap_err();
    assert!(matches!(err, am::MemoryError::InvalidRequest(_)));
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn dream_revisions_list_read_restore() {
    let home = temp_home("dream");
    let agent = make_agent(&home, "agentA");
    let compiled = am::CompiledMemoryStore::new(&agent.join("memory"), true);
    compiled.write_facts("原始事实").unwrap();
    let store = am::DreamRevisionStore::new(&agent.join("memory"));

    let revision = store.save("run-1", "manual", "dream", None).unwrap();
    let id = revision["revisionId"].as_str().unwrap().to_string();
    assert_eq!(revision["schemaVersion"], 1);
    assert_eq!(revision["kind"], "dream");
    let before = &revision["before"];
    assert_eq!(before["facts"], "原始事实");

    // 修改后再读/恢复（revision-store.ts restore）
    compiled.write_facts("被改掉的事实").unwrap();
    assert_ne!(compiled.read_sections().unwrap().facts, "原始事实");
    let list = store.list().unwrap();
    assert_eq!(list.len(), 1);
    let read = store.read(&id).unwrap();
    assert_eq!(read["revisionId"], id.as_str());

    let result = store.restore(&id).unwrap();
    assert_eq!(result["restored"], id.as_str());
    // pre_restore 修订被记录，且它的 before = 恢复前状态（revision-store.ts
    // :244-253 安全修订是撤销恢复的句柄，绝不能存恢复后内容）。
    let list_after = store.list().unwrap();
    assert_eq!(list_after.len(), 2);
    let safety = list_after
        .iter()
        .find(|r| r["kind"] == "pre_restore" && r["restoresRevisionId"] == id.as_str())
        .expect("pre_restore revision recorded");
    assert_eq!(
        safety["before"]["facts"], "被改掉的事实",
        "安全修订快照的是被替换的恢复前状态"
    );
    assert_eq!(compiled.read_sections().unwrap().facts, "原始事实");

    // 相同内容恢复 → 短路，不新增修订（revision-store.ts:246-247 hash 守卫）
    let again = store.restore(&id).unwrap();
    assert_eq!(again["unchanged"], json!(true), "{again}");
    assert_eq!(store.list().unwrap().len(), 2, "短路不落新修订");

    // 未知修订 404 语义
    let err = store.read("nope").unwrap_err();
    assert!(matches!(err, am::MemoryError::NotFound(_)));
    // id 白名单（revision-store.ts:56-58）
    assert!(store.read("../escape").is_err());
    assert!(store.read("has/slash").is_err());
    let _ = std::fs::remove_dir_all(&home);
}

// ── config patcher + PII ────────────────────────────────────────────────────

#[test]
fn config_patcher_preserves_unrelated_content_and_lists() {
    let text = "# 头注释\nlocale: zh\nagent:\n  name: A\nskills:\n  enabled:\n    - \"one\"\n    - \"two\"\n  workspace_disabled: []\nmemory:\n  enabled: true\n";
    let (enabled, disabled) = am::AgentConfigPatch::read_skill_lists(text).unwrap();
    assert_eq!(enabled, vec!["one", "two"]);
    assert!(disabled.is_empty());

    let patched = am::AgentConfigPatch::write_skill_lists(
        text,
        &["one".into(), "three".into()],
        &["ws1".into()],
    )
    .unwrap();
    assert!(patched.contains("locale: zh"), "无关行保留");
    assert!(patched.contains("name: A"));
    let (enabled2, disabled2) = am::AgentConfigPatch::read_skill_lists(&patched).unwrap();
    assert_eq!(enabled2, vec!["one", "three"]);
    assert_eq!(disabled2, vec!["ws1"]);
    // 空列表写 []（incumbent YAML 形状）
    let emptied = am::AgentConfigPatch::write_skill_lists(&patched, &[], &[]).unwrap();
    let (enabled3, _) = am::AgentConfigPatch::read_skill_lists(&emptied).unwrap();
    assert!(enabled3.is_empty());

    // flags 读写（memory/experience/workspace_context）
    let flags = am::AgentConfigFlags {
        memory_enabled: Some(false),
        experience_enabled: Some(true),
        discover_project_skills: Some(false),
        discover_compatible_project_skills: Some(true),
    };
    let patched_flags = am::AgentConfigPatch::write_flags(&emptied, &flags).unwrap();
    let read_back = am::AgentConfigPatch::read_flags(&patched_flags).unwrap();
    assert_eq!(read_back.memory_enabled, Some(false));
    assert_eq!(read_back.experience_enabled, Some(true));
    assert_eq!(read_back.discover_project_skills, Some(false));
    assert_eq!(read_back.discover_compatible_project_skills, Some(true));
}

#[test]
fn pii_scrub_matches_incumbent_patterns() {
    // api_key（pii-guard.ts HARD_PATTERNS[0]）
    let (cleaned, detected) = am::scrub_pii("key is sk-abcdef1234567890abcdef end");
    assert_eq!(detected, vec!["api_key"]);
    assert!(cleaned.contains("[REDACTED]"));
    assert!(!cleaned.contains("sk-abcdef"));
    // AKIA（16 大写数字尾）
    let (cleaned, detected) = am::scrub_pii("AKIAABCDEFGHIJKLMNOP in text");
    assert_eq!(detected, vec!["api_key"]);
    assert!(cleaned.contains("[REDACTED]"));
    // private key header
    let (cleaned, detected) =
        am::scrub_pii("-----BEGIN RSA PRIVATE KEY-----\nMIIB\n-----END RSA PRIVATE KEY-----");
    assert_eq!(detected, vec!["private_key"]);
    assert!(cleaned.contains("[REDACTED]"));
    // credit card
    let (cleaned, detected) = am::scrub_pii("卡号 4242 4242 4242 4242 请保存");
    assert_eq!(detected, vec!["credit_card"]);
    assert!(cleaned.contains("[REDACTED]"));
    // 干净文本不动
    let (cleaned, detected) = am::scrub_pii("普通文本，无敏感信息");
    assert_eq!(detected.len(), 0);
    assert_eq!(cleaned, "普通文本，无敏感信息");
}

// ── 技能库（A12 审核层存储面 + A11 白名单面）────────────────────────────────

fn write_skill(dir: &Path, name: &str, description: &str, body: &str) -> PathBuf {
    std::fs::create_dir_all(dir.join(name)).expect("mkdir skill");
    let path = dir.join(name).join("SKILL.md");
    std::fs::write(
        &path,
        format!("---\nname: {name}\ndescription: \"{description}\"\n---\n\n{body}"),
    )
    .expect("write SKILL.md");
    dir.join(name)
}

#[test]
fn catalog_scan_and_runtime_selection_whitelist() {
    let home = temp_home("catalog");
    make_agent(&home, "agentA");
    write_skill(&home.join("skills"), "alpha", "Alpha skill", "body");
    write_skill(&home.join("skills"), "beta", "Beta skill", "body");

    let service_dir = home.join("skills");
    let entries = sl::SkillCatalog::scan_user_pool(&service_dir, &Default::default());
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|e| e.source == "user" && !e.readonly));

    // 白名单：只有 enabled 的非 workspace 技能进入运行时（skill-manager.ts:199-252）
    let enabled: std::collections::HashSet<String> = ["alpha".to_string()].into_iter().collect();
    let disabled: std::collections::HashSet<String> = Default::default();
    let policy = lingxi_kernel::memory::DEFAULT_WORKSPACE_SKILL_POLICY;
    let selection = sl::resolve_runtime_selection(&entries, &enabled, &disabled, &policy);
    assert_eq!(selection.active, vec!["alpha".to_string()]);
    assert_eq!(selection.entries.len(), 2);

    // 新 agent 默认 enabled = 全部非 opt-out（skill-manager.ts:260-264）
    let defaults = sl::compute_default_enabled(&entries);
    assert_eq!(defaults.len(), 2);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn install_package_validates_structure_and_rewrites_metadata() {
    let home = temp_home("install");
    make_agent(&home, "agentA");
    let source = temp_home("install-src");
    let src_dir = write_skill(&source, "packaged", "A packaged skill", "body");

    let installed =
        sl::install_skill_package_from_directory(&src_dir, &home.join("skills"), "", Some(false))
            .unwrap();
    assert_eq!(installed.name, "packaged");
    assert!(installed.file_path.exists());
    let content = std::fs::read_to_string(&installed.file_path).unwrap();
    assert!(
        content.contains("default-enabled: false"),
        "安装技能不默认启用（install-skill.ts:427-434）"
    );

    // 缺 SKILL.md 拒绝
    let empty_src = temp_home("install-empty");
    let err = sl::install_skill_package_from_directory(&empty_src, &home.join("skills"), "", None)
        .unwrap_err();
    assert!(
        matches!(err, am::MemoryError::Conflict { ref code, .. } if code == sl::SKILL_MISSING_SKILL_MD),
        "缺 SKILL.md 拒绝（SKILL_MISSING_SKILL_MD）"
    );

    // symlink 拒绝（skill-package-installer.ts:117-130）
    let link_src = temp_home("install-link");
    let dir = write_skill(&link_src, "linked", "Has symlink", "body");
    #[cfg(unix)]
    std::os::unix::fs::symlink("/etc", dir.join("evil")).unwrap();
    let err = sl::install_skill_package_from_directory(&link_src, &home.join("skills"), "", None)
        .unwrap_err();
    assert!(
        matches!(err, am::MemoryError::Conflict { ref code, .. } if code == sl::SKILL_PACKAGE_SYMLINK)
    );

    // 相对路径拒绝（SKILL_SOURCE_MUST_BE_ABSOLUTE）
    let err = sl::install_skill_package_from_path(
        std::path::Path::new("relative/path"),
        &home.join("skills"),
        None,
    )
    .unwrap_err();
    assert!(
        matches!(err, am::MemoryError::Conflict { ref code, .. } if code == sl::SKILL_SOURCE_MUST_BE_ABSOLUTE)
    );
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&source);
    let _ = std::fs::remove_dir_all(&empty_src);
    let _ = std::fs::remove_dir_all(&link_src);
}

#[test]
fn bundles_store_crud_and_skill_removal_scrobble() {
    let home = temp_home("bundles");
    make_agent(&home, "agentA");
    make_agent(&home, "agentB");
    let bundles = sl::BundlesStore::new(&home);

    let bundle = bundles
        .create(
            "My Bundle",
            &["alpha".to_string(), "beta".to_string()],
            "user",
            None,
        )
        .unwrap();
    assert_eq!(bundle["skillNames"].as_array().unwrap().len(), 2);
    assert!(bundle["schemaVersion"].is_null()); // schemaVersion lives at store level

    // 删除技能 → scrub agent config + bundle 引用（skill-removal.ts:126-135）
    let config_a = home.join("agents").join("agentA").join("config.yaml");
    std::fs::write(
        &config_a,
        "skills:\n  enabled:\n    - \"alpha\"\n    - \"gamma\"\n",
    )
    .unwrap();
    let scrubbed =
        sl::collect_enabled_skills_of_other_agents(&home.join("agents"), "agentB").unwrap();
    assert!(scrubbed.contains("alpha"));
    bundles.remove_skills(&["alpha".to_string()]).unwrap();
    let after = bundles.load();
    // "My Bundle" 还剩 beta；只有被清空的组合才淘汰（store.ts:221-238）
    assert_eq!(after.len(), 1);
    assert_eq!(
        after[0]["skillNames"],
        json!(["beta"]),
        "被移除技能退出组合"
    );

    // 清掉 My Bundle（reorder 要求全量列表）
    let _ = bundles.delete(bundle["id"].as_str().unwrap());
    // reorder 全量校验（store.ts:171-186）
    let b1 = bundles
        .create("B1", &["x".to_string()], "user", None)
        .unwrap();
    let b2 = bundles
        .create("B2", &["y".to_string()], "user", None)
        .unwrap();
    let id1 = b1["id"].as_str().unwrap().to_string();
    let id2 = b2["id"].as_str().unwrap().to_string();
    let reordered = bundles.reorder(&[id2.clone(), id1.clone()]).unwrap();
    assert_eq!(reordered[0]["id"], id2.as_str());
    assert!(
        bundles.reorder(std::slice::from_ref(&id1)).is_err(),
        "缺条 reorder 拒绝"
    );

    // delete
    assert!(bundles.delete(&id1).unwrap());
    assert!(!bundles.delete(&id1).unwrap());
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn context_notes_cap_and_roundtrip() {
    let home = temp_home("notes");
    let store = am::ContextNotesStore::new(&home);
    store.write("sess-1", "第一条笔记").unwrap();
    assert_eq!(store.read("sess-1").unwrap(), "第一条笔记");
    // append 溢出拒绝（context-notes-tool.ts:86-96）
    let big = "x".repeat(am::CONTEXT_NOTES_MAX_BYTES + 1);
    let err = store.write("sess-1", &big).unwrap_err();
    assert_eq!(err.tenet_code(), Some("CONTEXT_NOTES_OVERSIZE"));
    // 读侧兜底截断带 marker（context-notes-tool.ts:25-28）
    std::fs::create_dir_all(home.join("context-notes")).unwrap();
    std::fs::write(
        home.join("context-notes").join("sess-2.md"),
        "y".repeat(am::CONTEXT_NOTES_MAX_BYTES + 100),
    )
    .unwrap();
    let read = store.read("sess-2").unwrap();
    assert!(read.starts_with("yyyy"));
    assert!(read.ends_with(am::CONTEXT_NOTES_TRUNCATION_MARKER));
    // 非法 session id
    assert!(store.write("../evil", "x").is_err());
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn persona_fallback_chain_and_avatar_formats() {
    let home = temp_home("persona");
    make_agent(&home, "agentA");
    // 模板回落（persona-source.ts:87-116）
    let templates = home.join("identity-templates");
    std::fs::create_dir_all(templates.join("en")).unwrap();
    std::fs::write(templates.join("en").join("yuan.md"), "EN TEMPLATE").unwrap();
    let persona = am::PersonaFiles::new(&home.join("agents"), &home);
    let (content, from_template) = persona
        .resolve_persona("agentA", "yuan", "en", am::PersonaKind::Identity)
        .unwrap();
    assert_eq!(content, "EN TEMPLATE");
    assert!(from_template);
    // 落盘优先（用户定制）
    persona
        .write_persona("agentA", am::PersonaKind::Identity, "自定义身份")
        .unwrap();
    let (content, from_template) = persona
        .resolve_persona("agentA", "yuan", "en", am::PersonaKind::Identity)
        .unwrap();
    assert_eq!(content, "自定义身份");
    assert!(!from_template);

    // avatar：写入 jpeg（.jpg 归一）+ 读取顺序 + 删除幂等
    persona
        .write_avatar("agentA", "jpg", b"jpeg-bytes")
        .unwrap();
    let (_path, mime, bytes) = persona.read_avatar("agentA").unwrap().unwrap();
    assert_eq!(mime, "image/jpeg");
    assert_eq!(bytes, b"jpeg-bytes");
    persona.write_avatar("agentA", "png", b"png-bytes").unwrap();
    let (_path, mime, _) = persona.read_avatar("agentA").unwrap().unwrap();
    assert_eq!(mime, "image/png", "png 优先于 jpeg（avatar 读取顺序）");
    persona.delete_avatar("agentA").unwrap();
    assert!(persona.read_avatar("agentA").unwrap().is_none());
    persona.delete_avatar("agentA").unwrap(); // 幂等 ok
    let _ = std::fs::remove_dir_all(&home);
}
