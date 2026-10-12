use lingxi_test_guard as _;
#[derive(Debug, Default, Clone, PartialEq)]
pub struct AgentConfigScan {
    pub locale: Option<String>,
    pub agent_name: Option<String>,
    pub yuan: Option<String>,
    pub memory_enabled: Option<bool>,
}

pub fn scan_agent_config(text: &str) -> AgentConfigScan {
    fn clean_value(raw: &str) -> String {
        // 剥离行内注释（" #" 起）与成对引号。
        let without_comment = raw.split(" #").next().unwrap_or(raw);
        let trimmed = without_comment.trim();
        trimmed
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| {
                trimmed
                    .strip_prefix('\'')
                    .and_then(|v| v.strip_suffix('\''))
            })
            .unwrap_or(trimmed)
            .to_string()
    }
    let mut scan = AgentConfigScan::default();
    let mut section = String::new();
    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() || trimmed.trim_start().starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let body = line.trim();
        if indent == 0 {
            if let Some(name) = body.strip_suffix(':') {
                section = name.trim().to_string();
                continue;
            }
            if let Some((key, value)) = body.split_once(':') {
                section.clear();
                if key.trim() == "locale" {
                    let value = clean_value(value);
                    if !value.is_empty() {
                        scan.locale = Some(value);
                    }
                }
            }
            continue;
        }
        let Some((key, value)) = body.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = clean_value(value);
        match (section.as_str(), key) {
            ("agent", "name") if !value.is_empty() => scan.agent_name = Some(value),
            ("agent", "yuan") if !value.is_empty() => scan.yuan = Some(value),
            ("memory", "enabled") => {
                scan.memory_enabled = match value.as_str() {
                    "true" => Some(true),
                    "false" => Some(false),
                    _ => None,
                };
            }
            _ => {}
        }
    }
    scan
}


#[test]fn old_block_positive(){ assert_eq!(scan_agent_config("memory:\n  enabled: false\n").memory_enabled,Some(false));}
#[test]fn old_flow_contract(){ assert_eq!(scan_agent_config("memory: { enabled: false }\n").memory_enabled,Some(false));}
#[test]fn old_quote_contract(){assert_eq!(scan_agent_config("agent:\n  name: \"助手 #1: 中文\" # comment\n").agent_name.as_deref(),Some("助手 #1: 中文"));}
