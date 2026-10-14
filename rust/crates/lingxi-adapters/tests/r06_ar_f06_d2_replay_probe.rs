//! D 阶段重放探针：在当前源码上重放 R03 登记的 F06 运行时反例（先观察，后定论）。
use lingxi_adapters::storage::agent_memory::{
    config::AgentConfigDocument, AgentConfigFlags, AgentConfigPatch,
};

/// 断言 plain 标量按现役 oracle 的类型读回（string 用文字、number 用数值）。
fn plain_scalar(yaml: &str) -> Result<serde_yaml_ng::Value, String> {
    let doc = AgentConfigDocument::parse(yaml).map_err(|e| e.to_string())?;
    let rendered = doc.render().map_err(|e| e.to_string())?;
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&rendered)
        .map_err(|e| e.to_string())
        .map(|v| v["future"].clone())
}

#[test]
fn r03_replay_uppercase_prefixes_and_explicit_tags() {
    // R03-F01：plain 大写基数前缀在现役 js-yaml 下是字符串，round-trip 不得变数字。
    for v in [
        "0B10", "0O17", "0Xff", "+0B10", "-0B10", "+0O17", "-0O17", "+0Xff", "-0Xff", "1e999",
    ] {
        let text = format!("memory: {{enabled: false}}\nfuture: {v}\n");
        let scalar = plain_scalar(&text).unwrap_or_else(|err| panic!("{v}: {err}"));
        assert_eq!(
            scalar,
            serde_yaml_ng::Value::String(v.to_string()),
            "plain {v} 必须保持字符串: {scalar:?}"
        );
        // 与 flags 无关的 memory-only 更新不得改变 future 的类型和值（渲染路径允许规范化排版）。
        let patched = AgentConfigPatch::write_flags(
            &text,
            &AgentConfigFlags {
                memory_enabled: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        let after = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&patched).unwrap();
        assert_eq!(
            after["future"],
            serde_yaml_ng::Value::String(v.to_string()),
            "patch 后 future 保持字符串值: {patched}"
        );
    }
    // 正对照：小写前缀和普通十进制在现役下是数字。
    for (v, expect) in [
        ("0b10", serde_yaml_ng::Value::from(2)),
        ("0o17", serde_yaml_ng::Value::from(15)),
        ("0xff", serde_yaml_ng::Value::from(255)),
        ("017", serde_yaml_ng::Value::from(17)),
        ("0378", serde_yaml_ng::Value::from(378)),
        ("+0", serde_yaml_ng::Value::from(0)),
        ("-7", serde_yaml_ng::Value::from(-7)),
        (".5", serde_yaml_ng::Value::from(0.5)),
    ] {
        let scalar = plain_scalar(&format!("memory: {{enabled: false}}\nfuture: {v}\n"))
            .unwrap_or_else(|e| panic!("{v}: {e}"));
        assert_eq!(scalar, expect, "plain {v} 应为数字");
    }
    // R03-F01：显式 !!int 大写前缀现役拒绝，必须报错。
    for v in ["0B10", "0O17", "0Xff", "wrong", "false", "", "1_000"] {
        let text = format!("memory: {{enabled: false}}\nfuture: !!int {v}\n");
        assert!(
            AgentConfigDocument::parse(&text).is_err(),
            "!!int {v:?} 必须拒绝"
        );
    }
    // 显式 !!int 合法对照。
    for (v, expect) in [
        ("7", serde_yaml_ng::Value::from(7)),
        ("0x1F", serde_yaml_ng::Value::from(31)),
        ("0b10", serde_yaml_ng::Value::from(2)),
        ("0o17", serde_yaml_ng::Value::from(15)),
        ("-0", serde_yaml_ng::Value::from(0)),
    ] {
        let scalar = plain_scalar(&format!("memory: {{enabled: false}}\nfuture: !!int {v}\n"))
            .unwrap_or_else(|e| panic!("!!int {v}: {e}"));
        assert_eq!(scalar, expect, "!!int {v} 合法值");
    }
    // R03-F02：非法显式 !!null 字面必须拒绝。
    for v in ["wrong", "false", "7"] {
        let text = format!("memory: {{enabled: false}}\nfuture: !!null {v}\n");
        assert!(
            AgentConfigDocument::parse(&text).is_err(),
            "!!null {v} 必须拒绝"
        );
    }
    // 合法 !!null 对照（含裸 !!null）。
    for v in ["~", "null", "Null", "NULL", ""] {
        let text = if v.is_empty() {
            "memory: {enabled: false}\nfuture: !!null\n".to_string()
        } else {
            format!("memory: {{enabled: false}}\nfuture: !!null {v}\n")
        };
        let scalar = plain_scalar(&text).unwrap_or_else(|e| panic!("!!null {v:?}: {e}"));
        assert_eq!(scalar, serde_yaml_ng::Value::Null, "!!null {v:?} → null");
    }
}
