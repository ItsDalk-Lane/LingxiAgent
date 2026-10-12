//! 实际 YAML 读写、敏感类型与失败后原配置保护。
use lingxi_adapters::storage::agent_memory::{
    config::AgentConfigDocument, patch_agent_config, AgentConfigFlags, AgentConfigPatch,
};
use serde_yaml_ng::Value;

#[test]
fn c04_updates_preserve_all_unknown_nested_values_and_escaped_lists() {
    let text = "agent: {name: '助手 #1', yuan: lingxi, extra: [中文, 42]}\nmemory: {enabled: false, token_budget: 1234, custom: {a: [null, true, 7], b: '中文 #1'}}\nskills: {enabled: [one], unknown: {x: [1, 2]}}\nworkspace_context: {discover_project_skills: false, extra: keep}\nproviders: {custom: {future: value}}\n";
    let before: Value = serde_yaml_ng::from_str(text).unwrap();
    let patched = AgentConfigPatch::write_flags(
        text,
        &AgentConfigFlags {
            memory_enabled: Some(true),
            experience_enabled: Some(false),
            ..Default::default()
        },
    )
    .unwrap();
    let list = vec!["路径\\含引号\" #中文".into(), "colon: inside".into()];
    let patched = AgentConfigPatch::write_skill_lists(&patched, &list, &[]).unwrap();
    let after: Value = serde_yaml_ng::from_str(&patched).unwrap();
    for key in ["agent", "providers", "workspace_context"] {
        assert_eq!(before[key], after[key], "无关字段 {key} 必须保留");
    }
    for key in ["token_budget", "custom"] {
        assert_eq!(before["memory"][key], after["memory"][key]);
    }
    assert_eq!(before["skills"]["unknown"], after["skills"]["unknown"]);
    assert_eq!(
        AgentConfigPatch::read_skill_lists(&patched).unwrap().0,
        list
    );
    assert_eq!(after["memory"]["enabled"].as_bool(), Some(true));
    assert_eq!(after["experience"]["enabled"].as_bool(), Some(false));
}

#[test]
fn c03_duplicate_sensitive_keys_and_nested_keys_fail_before_writing() {
    let root = std::env::temp_dir().join(format!("lingxi-f06-config-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("config.yaml");
    for text in [
        "memory: {enabled: false, enabled: true}",
        "memory: {enabled: false}\nmemory: {enabled: true}",
        "skills: {enabled: [one], enabled: [two]}",
        "workspace_context: {discover_project_skills: 'false'}",
        "memory: {enabled: false",
    ] {
        std::fs::write(&path, text).unwrap();
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
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }
    let doc = AgentConfigDocument::parse(
        "memory: {custom: {enabled: false}}\nagent: {custom: {name: wrong}}",
    )
    .unwrap();
    assert_eq!(doc.scan().unwrap().memory_enabled, None);
    assert_eq!(doc.scan().unwrap().agent_name, None);
    // 补丁本身产生损坏配置也必须保留原件。
    std::fs::write(&path, "memory: {enabled: false}").unwrap();
    assert!(patch_agent_config(&path, |_| Ok("memory: [bad".into())).is_err());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "memory: {enabled: false}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn c03_aliases_and_merge_keys_follow_standard_yaml_semantics() {
    for yaml in [
        "defaults: &d {enabled: false}\nmemory: *d",
        "defaults: &d {enabled: true}\nmemory: {<<: *d, enabled: false}",
        "memory: {enabled: FALSE}",
    ] {
        assert_eq!(
            AgentConfigDocument::parse(yaml)
                .unwrap()
                .flags()
                .unwrap()
                .memory_enabled,
            Some(false)
        );
    }
    assert!(AgentConfigDocument::parse("memory: &recursive {enabled: *recursive}").is_err());
    assert!(
        AgentConfigDocument::parse("memory: {enabled: false}\n---\nmemory: {enabled: true}")
            .is_err()
    );
}

#[test]
fn p01_all_owned_key_types_and_merge_sources_reject_before_defaults_and_patch() {
    let cases = [
        "? [memory]\n: {enabled: false}",
        "memory:\n  ? [enabled]\n  : false",
        "? [agent]\n: {name: 姓名}",
        "agent:\n  ? [name]\n  : 姓名",
        "agent:\n  ? [yuan]\n  : custom",
        "? [locale]\n: en",
        "experience:\n  ? [enabled]\n  : false",
        "skills:\n  ? [enabled]\n  : [shared]",
        "skills:\n  ? [workspace_disabled]\n  : [project]",
        "workspace_context:\n  ? [discover_project_skills]\n  : false",
        "workspace_context:\n  ? [discover_compatible_project_skills]\n  : false",
        "memory: {enabled: false}\n? [memory]\n: {enabled: true}",
        "memory: {enabled: false, ? [enabled]: true}",
        "memory: {42: false}",
        "agent: {null: name}",
        "skills: {? {enabled: value}: [shared]}",
        "defaults: &d {? [enabled]: false}\nmemory: *d",
        "defaults: &d {? [enabled]: false}\nmemory: {<<: *d}",
        "defaults: &d {? [memory]: {enabled: false}}\n<<: *d",
        "defaults: &d {? [enabled]: false}\nmemory: {<<: [*d, {enabled: true}], enabled: false}",
        "defaults: &d {memory: {? [enabled]: false}}\n<<: *d\nmemory: {enabled: false}",
        "memory: {!custom enabled: false}",
    ];
    let root =
        std::env::temp_dir().join(format!("lingxi-p01-invalid-patch-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("config.yaml");
    let mut failures = Vec::new();
    for yaml in cases {
        std::fs::write(&path, yaml).unwrap();
        let persisted = patch_agent_config(&path, |text| {
            AgentConfigPatch::write_flags(
                text,
                &AgentConfigFlags {
                    memory_enabled: Some(true),
                    ..Default::default()
                },
            )
        });
        let parsed = AgentConfigDocument::parse(yaml);
        let patched = AgentConfigPatch::write_flags(
            yaml,
            &AgentConfigFlags {
                memory_enabled: Some(true),
                ..Default::default()
            },
        );
        if parsed.is_ok() || patched.is_ok() || persisted.is_ok() {
            failures.push(yaml);
        } else {
            assert!(parsed.err().unwrap().to_string().contains("config.yaml"));
            assert!(patched.err().unwrap().to_string().contains("config.yaml"));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), yaml);
        }
    }
    std::fs::remove_dir_all(root).unwrap();
    assert!(
        failures.is_empty(),
        "不支持的键不得默认开启或被更新掩盖: {failures:?}"
    );
}

#[test]
fn p01_string_key_alias_merge_controls_and_unknown_values_survive_patch() {
    let original = "defaults: &d {enabled: true}\nmemory: {<<: [*d, {enabled: false}], enabled: false, custom: {? [enabled]: false}}\nagent: {name: '姓名 # 中文', yuan: custom}\nskills: {enabled: [shared], workspace_disabled: [project]}\nworkspace_context: {discover_project_skills: false, discover_compatible_project_skills: false}\nfuture: {? [memory]: {enabled: false}, 42: [true, null]}\n";
    let mut before: Value = serde_yaml_ng::from_str(original).unwrap();
    before.apply_merge().unwrap();
    let doc = AgentConfigDocument::parse(original).unwrap();
    assert_eq!(doc.flags().unwrap().memory_enabled, Some(false));
    assert_eq!(doc.flags().unwrap().discover_project_skills, Some(false));
    assert_eq!(
        doc.flags().unwrap().discover_compatible_project_skills,
        Some(false)
    );
    assert_eq!(
        doc.scan().unwrap().agent_name.as_deref(),
        Some("姓名 # 中文")
    );
    assert_eq!(
        doc.skill_lists().unwrap(),
        (vec!["shared".into()], vec!["project".into()])
    );
    let patched = AgentConfigPatch::write_flags(
        original,
        &AgentConfigFlags {
            memory_enabled: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    let patched =
        AgentConfigPatch::write_skill_lists(&patched, &["new".into()], &["disabled".into()])
            .unwrap();
    let after: Value = serde_yaml_ng::from_str(&patched).unwrap();
    for key in ["defaults", "agent", "workspace_context", "future"] {
        assert_eq!(before[key], after[key], "未知兼容值必须保真 {key}");
    }
    assert_eq!(before["memory"]["custom"], after["memory"]["custom"]);
}

#[test]
fn p02_literal_merge_keys_and_supported_tags_survive_repeated_updates() {
    for key in ["\"<<\"", "'<<'", "!!str <<", "!<tag:yaml.org,2002:str> <<"] {
        let text = format!(
            "{key}: 7\nagent: {{{key}: {{name: 不得提升}}}}\nmemory: {{{key}: {{enabled: false}}}}\nexperience: {{{key}: {{enabled: true}}}}\nskills: {{{key}: {{enabled: [shared]}}}}\nworkspace_context: {{{key}: {{discover_project_skills: false}}}}\nfuture: {{{key}: {{marker: keep}}, other: value}}\n"
        );
        let before: Value = serde_yaml_ng::from_str(&text).unwrap();
        let doc = AgentConfigDocument::parse(&text).unwrap();
        assert_eq!(doc.scan().unwrap().agent_name, None);
        assert_eq!(doc.flags().unwrap().memory_enabled, None);
        assert_eq!(doc.flags().unwrap().experience_enabled, None);
        assert_eq!(doc.flags().unwrap().discover_project_skills, None);
        assert!(doc.skill_lists().unwrap().0.is_empty());
        let patched = AgentConfigPatch::write_flags(
            &text,
            &AgentConfigFlags {
                memory_enabled: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        let patched = AgentConfigPatch::write_skill_lists(&patched, &["new".into()], &[]).unwrap();
        let after: Value = serde_yaml_ng::from_str(&patched).unwrap();
        for section in ["agent", "experience", "workspace_context", "future"] {
            assert_eq!(before[section], after[section]);
        }
        for section in ["memory", "skills"] {
            assert_eq!(before[section]["<<"], after[section]["<<"]);
        }
        assert_eq!(before["<<"], after["<<"]);
        let reread = AgentConfigDocument::parse(&patched).unwrap();
        assert_eq!(reread.flags().unwrap().memory_enabled, Some(false));
        assert_eq!(reread.flags().unwrap().experience_enabled, None);
        assert_eq!(reread.scan().unwrap().agent_name, None);
        assert_eq!(reread.skill_lists().unwrap().0, vec!["new"]);
        println!("p02_literal_key={key} input={text:?} persisted={patched:?}");
    }
    let doc = AgentConfigDocument::parse("locale: !!str en\nagent: !!map {name: !!str true, yuan: !!str lingxi}\nmemory: !!map {enabled: !!bool FALSE}\nskills: {enabled: !!seq [!!str one]}\nfuture: {bool: true, number: 7, text: !!str false, nested: [null, 中文]}").unwrap();
    assert_eq!(doc.scan().unwrap().locale.as_deref(), Some("en"));
    assert_eq!(doc.scan().unwrap().agent_name.as_deref(), Some("true"));
    assert_eq!(doc.flags().unwrap().memory_enabled, Some(false));
    assert_eq!(doc.skill_lists().unwrap().0, vec!["one"]);
    let rendered = doc.render().unwrap();
    assert_eq!(
        AgentConfigDocument::parse(&rendered)
            .unwrap()
            .scan()
            .unwrap(),
        doc.scan().unwrap()
    );
}

#[test]
fn p02_unknown_tags_reject_all_positions_and_preserve_original_bytes() {
    let root = std::env::temp_dir().join(format!("lingxi-p02-tag-patch-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("config.yaml");
    for text in [
        "!custom {memory: {enabled: false}}",
        "memory: !custom {enabled: true}",
        "memory: {enabled: !custom true}",
        "experience: {enabled: !custom false}",
        "agent: {name: !custom 姓名}",
        "agent: {yuan: !custom lingxi}",
        "locale: !custom en",
        "skills: !custom {enabled: [one]}",
        "skills: {enabled: !custom [one]}",
        "skills: {enabled: [!custom one]}",
        "skills: {workspace_disabled: [!custom one]}",
        "workspace_context: {discover_project_skills: !custom false}",
        "workspace_context: {discover_compatible_project_skills: !custom false}",
        "future: {value: !custom string}",
        "future: !custom {value: false}",
        "defaults: &d {enabled: !custom true}\nmemory: {<<: *d, enabled: false}",
        "defaults: &d {memory: !custom {enabled: true}}\n<<: *d\nmemory: {enabled: false}",
        "%TAG !t! tag:example.org,2026:\n---\nmemory: {enabled: !t!bool true}",
        "memory: !!str {enabled: false}",
        "memory: {enabled: !!str false}",
        "memory: {enabled: !!merge true}",
    ] {
        std::fs::write(&path, text).unwrap();
        assert!(
            AgentConfigDocument::parse(text).is_err(),
            "不得接受未知或不匹配类型: {text}"
        );
        assert!(
            patch_agent_config(&path, |text| AgentConfigPatch::write_flags(
                text,
                &AgentConfigFlags {
                    memory_enabled: Some(false),
                    ..Default::default()
                }
            ))
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn p02_true_merge_precedence_explicit_tags_and_overridden_sources_remain_checked() {
    for text in [
        "a: &a {enabled: false}\nb: &b {enabled: true}\nmemory: {<<: [*a, *b]}",
        "a: &a {enabled: true}\nmemory: {!!merge <<: *a, enabled: false}",
        "%TAG !m! tag:yaml.org,2002:me\n---\na: &a {enabled: false}\nmemory: {!m!rge <<: *a}",
        "a: &a {enabled: false}\nb: &b {<<: *a}\nmemory: {<<: *b}",
    ] {
        let doc = AgentConfigDocument::parse(text).unwrap();
        assert_eq!(doc.flags().unwrap().memory_enabled, Some(false));
        assert_eq!(
            AgentConfigDocument::parse(&doc.render().unwrap())
                .unwrap()
                .flags()
                .unwrap()
                .memory_enabled,
            Some(false)
        );
    }
    for text in [
        "a: &a {? [enabled]: true}\nb: &b {enabled: false}\nmemory: {<<: [*b, *a], enabled: false}",
        "a: &a {memory: {? [enabled]: true}}\n<<: *a\nmemory: {enabled: false}",
        "a: &a {enabled: false, enabled: true}\nmemory: {<<: *a, enabled: false}",
    ] {
        assert!(
            AgentConfigDocument::parse(text).is_err(),
            "被覆盖的来源也必须验证: {text}"
        );
    }
}

#[test]
fn p02_mixed_literal_and_merge_keys_and_alias_key_identity_remain_distinct() {
    for text in [
        "d: &d {enabled: false}\nmemory: {<<: *d, \"<<\": {enabled: true}}\nfuture: {\"<<\": [{nested: keep}]}",
        "key: &k '<<'\nmemory: {*k : {enabled: true}, enabled: false}",
        "%TAG !s! tag:yaml.org,2002:s\n---\nmemory: {!s!tr <<: {enabled: true}, enabled: false}",
        "memory: {! enabled: false}",
    ] {
        let doc = AgentConfigDocument::parse(text).unwrap_or_else(|err| panic!("{text}: {err}"));
        assert_eq!(doc.flags().unwrap().memory_enabled, Some(false));
        let rendered = doc.render().unwrap();
        let reread = AgentConfigDocument::parse(&rendered).unwrap();
        assert_eq!(reread.flags().unwrap().memory_enabled, Some(false));
        if text.contains("future:") {
            let after: Value = serde_yaml_ng::from_str(&rendered).unwrap();
            let expected: Value =
                serde_yaml_ng::from_str("future: {\"<<\": [{nested: keep}]} ").unwrap();
            assert_eq!(expected["future"], after["future"]);
        }
    }
}

#[test]
fn p03_tags_cannot_erase_type_and_unsupported_data_never_rewrites() {
    let root = std::env::temp_dir().join(format!("lingxi-p03-type-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("config.yaml");
    let mut accepted = Vec::new();
    for text in [
        "memory: {enabled: !bool true}",
        "agent: {name: !str 姓名}",
        "memory: !map {enabled: false}",
        "skills: {enabled: !seq [one]}",
        "future: {n: !int 7}",
        "agent: {name: !!binary SGVsbG8=}",
        "locale: !!binary ZW4=",
        "skills: {enabled: [!!binary b25l]}",
        "d: &d {enabled: !bool true}\nmemory: {<<: *d, enabled: false}",
        "d: &d {enabled: 'true'}\nmemory: {<<: *d, enabled: false}",
        "future: !!int 1_000",
        "future: !!float 1e999",
        "future: !!float 1_000.2",
        "future: !!binary 'SG Vs'",
    ] {
        std::fs::write(&path, text).unwrap();
        let actual = patch_agent_config(&path, |text| {
            AgentConfigPatch::write_flags(
                text,
                &AgentConfigFlags {
                    memory_enabled: Some(true),
                    ..Default::default()
                },
            )
        });
        println!("p03_type input={text:?} actual={actual:?}");
        if actual.is_ok() {
            accepted.push(text);
        } else {
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        }
    }
    let normal = "agent: {name: !!str 日期 2026-10-11}\nmemory: {enabled: !!bool false}\nfuture: {date: '2026-10-11', n: 012, yes: yes}";
    assert_eq!(
        AgentConfigDocument::parse(normal)
            .unwrap()
            .flags()
            .unwrap()
            .memory_enabled,
        Some(false)
    );
    assert!(AgentConfigPatch::write_flags(
        normal,
        &AgentConfigFlags {
            memory_enabled: Some(true),
            ..Default::default()
        }
    )
    .is_ok());
    std::fs::remove_dir_all(root).unwrap();
    assert!(
        accepted.is_empty(),
        "不得擦类型或被覆盖来源后悄悄保存: {accepted:?}"
    );
}

#[test]
fn p03_standard_unknown_types_roundtrip_without_erasure() {
    for text in [
        "future: {utf8: !!binary SGVsbG8=, bytes: !!binary /wABAA==}",
        "future: {date: 2026-10-11, broad: 2026-1-2t3:04:05.123456 +8, zoneless: 2026-1-2 3:04:05, z: 2026-10-11T03:04:05Z, malformed: 2026-10-11x, explicit: !!timestamp 2026-10-11, time: 2026-10-11T13:42:23.456+08:00, quoted: '2026-10-11', string: !!str 2026-10-11}",
        "future: {set: !!set {one: null, two: null}, omap: !!omap [{one: 1}, {two: 2}], pairs: !!pairs [{one: 1}, {one: 2}]}",
        "future: {num: 012, neg: -012, large: 9007199254740993, beyond_u64: 18446744073709551616, negative_zero: -0, tagged_negative_zero: !!int -0, huge_exponent: 1e999, positive_zero: +0, octal: 0o12, hex: 0xFF, binary_int: 0b10, leading_decimal: 0378, fraction: 1.25, exponent: 1e3, plus_exp: +1.2e+3, under: 1_000, underfloat: 1_000.2, signed_dot: -.2, unsigned_dot: .2, word_inf: inf, negative_float_zero: -0.0, inf: .inf, neg_inf: -.Inf, explicit_inf: !!float .inf, nan: .nan, yes: yes, off: off}\nmultiline: |+\n  中文 # :\n\n",
        "%TAG !b! tag:yaml.org,2002:bi\n---\nfuture: {bytes: !b!nary /wABAA==}\nmemory: {enabled: false}",
        "future: {noncanonical: !!binary cB==, empty: !!binary ''}\nblock: !!binary |\n  SGVs\n  bG8=\n",
    ] {
        let text=if text.starts_with("%TAG") {text.to_string()} else {format!("memory: {{enabled: false}}\n{text}")};
        let patched=AgentConfigPatch::write_flags(&text,&AgentConfigFlags{memory_enabled:Some(true),..Default::default()}).unwrap();
        let patched=AgentConfigPatch::write_skill_lists(&patched,&["中文 skill".into()],&["project".into()]).unwrap();
        let reloaded=AgentConfigDocument::parse(&patched).unwrap();
        assert_eq!(reloaded.flags().unwrap().memory_enabled,Some(true));
        assert_eq!(reloaded.skill_lists().unwrap().0,vec!["中文 skill"]);
        println!("p03_roundtrip={}",serde_json::json!({"before":text,"after":patched}));
    }
}

#[test]
fn p03_resource_limits_precede_graph_and_alias_allocations() {
    use lingxi_adapters::storage::agent_memory::config::read_agent_config;
    let ordinary = format!(
        "memory: {{enabled: false}}\nfuture:\n{}",
        (0..2000)
            .map(|i| format!("  key{i}: 中文 # ordinary\n"))
            .collect::<String>()
    );
    assert_eq!(
        AgentConfigDocument::parse(&ordinary)
            .unwrap()
            .flags()
            .unwrap()
            .memory_enabled,
        Some(false)
    );
    let aliases = "d: &d {marker: 中文}\nfuture: [*d, *d, *d]\nmemory: {enabled: false}";
    assert!(AgentConfigDocument::parse(aliases).is_ok());
    let deep = format!("future: {}0{}", "[".repeat(65), "]".repeat(65));
    assert!(AgentConfigDocument::parse(&deep)
        .err()
        .unwrap()
        .to_string()
        .contains("config.yaml"));
    let expanded = (0..20)
        .map(|i| {
            if i == 0 {
                "d0: &d0 [0]\n".to_string()
            } else {
                format!("d{i}: &d{i} [*d{}, *d{}]\n", i - 1, i - 1)
            }
        })
        .collect::<String>();
    assert!(AgentConfigDocument::parse(&expanded).is_err());
    let shallow = format!(
        "future: [{}]",
        (0..250_000).map(|_| "0").collect::<Vec<_>>().join(",")
    );
    let err = AgentConfigDocument::parse(&shallow)
        .err()
        .unwrap()
        .to_string();
    assert!(err.contains("budget"), "{err}");
    let root = std::env::temp_dir().join(format!("lingxi-p03-budget-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("config.yaml");
    std::fs::File::create(&path)
        .unwrap()
        .set_len(256 * 1024 * 1024 + 1)
        .unwrap();
    assert!(read_agent_config(&path)
        .err()
        .unwrap()
        .to_string()
        .contains("byte budget"));
    assert_eq!(
        std::fs::metadata(&path).unwrap().len(),
        256 * 1024 * 1024 + 1
    );
    std::fs::remove_dir_all(root).unwrap();
}
