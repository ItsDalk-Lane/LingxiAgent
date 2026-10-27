//! 搜索凭据的独立命名空间；持久来源仍为偏好，不能借模型路由读取其他供应商密钥。
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const PAID: [&str; 4] = ["anysearch", "tavily", "brave", "serper"];
pub const BROWSERS: [&str; 3] = ["bing_browser", "google_browser", "duckduckgo_browser"];
pub fn known(provider: &str) -> bool {
    provider == "auto"
        || provider == "anysearch_free"
        || PAID.contains(&provider)
        || BROWSERS.contains(&provider)
}

/// 不实现 Debug/Serialize，避免状态或错误输出把材料带到模型、日志或页面。
#[derive(Clone)]
pub struct SearchCredentials {
    pub provider: String,
    keys: BTreeMap<String, String>,
}
impl SearchCredentials {
    pub fn from_preferences(root: &Value) -> Result<Self, String> {
        let provider = match root.get("search_provider") {
            None | Some(Value::Null) => "auto".to_owned(),
            Some(Value::String(value)) if known(value.trim()) => value.trim().to_owned(),
            _ => return Err("saved search provider is invalid".into()),
        };
        let mut keys = BTreeMap::new();
        if let Some(value) = root.get("search_api_keys") {
            let map = value
                .as_object()
                .ok_or("saved search keys must be an object")?;
            for id in PAID {
                match map.get(id) {
                    None | Some(Value::Null) => {}
                    Some(Value::String(key)) => {
                        if !key.trim().is_empty() {
                            keys.insert(id.into(), checked_key(key)?);
                        }
                    }
                    _ => return Err(format!("saved {id} search key must be a string")),
                }
            }
        }
        if PAID.contains(&provider.as_str()) && !keys.contains_key(&provider) {
            match root.get("search_api_key") {
                Some(Value::String(key)) if !key.trim().is_empty() => {
                    keys.insert(provider.clone(), checked_key(key)?);
                }
                None | Some(Value::Null) | Some(Value::String(_)) => {}
                _ => return Err("saved legacy search key must be a string".into()),
            }
        }
        Ok(Self { provider, keys })
    }
    pub(crate) fn key(&self, provider: &str) -> Option<&str> {
        self.keys.get(provider).map(String::as_str)
    }
    pub(crate) fn candidate(&self, provider: &str, value: &str) -> Result<String, String> {
        if !PAID.contains(&provider) {
            return Ok(String::new());
        }
        if value == "********" {
            return self
                .key(provider)
                .map(String::from)
                .ok_or("masked key has no saved value for this provider".into());
        }
        checked_key(value)
    }
    pub(crate) fn save_into(
        &self,
        root: &mut Value,
        provider: &str,
        verified: &str,
        selected: &str,
    ) -> Result<(), String> {
        if !known(selected) || !known(provider) {
            return Err("unknown search provider".into());
        }
        let mut map = root.get("search_api_keys").cloned().unwrap_or(json!({}));
        let target = map
            .as_object_mut()
            .ok_or("saved search keys must be an object")?;
        // 补回旧格式中当前供应商的材料，同时保留新版本的未知字段。
        for (id, key) in &self.keys {
            target.insert(id.clone(), json!(key));
        }
        if PAID.contains(&provider) {
            target.insert(provider.into(), json!(checked_key(verified)?));
        }
        root["search_provider"] = json!(selected);
        root["search_api_key"] = if PAID.contains(&selected) {
            map.get(selected).cloned().unwrap_or(json!(""))
        } else {
            json!("")
        };
        root["search_api_keys"] = map;
        Ok(())
    }
    pub fn masked(&self) -> Value {
        let keys: BTreeMap<&str, &str> = PAID
            .into_iter()
            .map(|id| {
                (
                    id,
                    if self.key(id).is_some() {
                        "********"
                    } else {
                        ""
                    },
                )
            })
            .collect();
        json!({"provider":self.provider,"api_key":if self.key(&self.provider).is_some(){"********"}else{""},"api_keys":keys})
    }
}
fn checked_key(value: &str) -> Result<String, String> {
    let key = value.trim();
    if key.is_empty() || key.len() > 8192 || key == "********" || key.chars().any(char::is_control)
    {
        return Err("api_key must be a nonempty, unmasked key".into());
    }
    Ok(key.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn masks_are_bound_to_the_same_provider_and_preserve_other_preferences() {
        let mut root = json!({"search_provider":"tavily","search_api_key":"old-key","other":{"keep":true},"search_api_keys":{"future":"opaque","brave":"brave-key"}});
        let before = SearchCredentials::from_preferences(&root).unwrap();
        assert_eq!(before.candidate("tavily", "********").unwrap(), "old-key");
        assert!(before.candidate("serper", "********").is_err());
        before
            .save_into(&mut root, "tavily", "new-key", "auto")
            .unwrap();
        assert_eq!(root["search_api_keys"]["brave"], "brave-key");
        assert_eq!(root["search_api_keys"]["future"], "opaque");
        assert_eq!(root["search_api_key"], "");
        assert_eq!(root["other"], json!({"keep":true}));
        let mask = SearchCredentials::from_preferences(&root)
            .unwrap()
            .masked()
            .to_string();
        assert!(!mask.contains("new-key") && !mask.contains("brave-key"));
    }
}
