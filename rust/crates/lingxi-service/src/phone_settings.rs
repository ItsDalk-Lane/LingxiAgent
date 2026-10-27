//! 手机权限和回复偏好；只解析已知字段，文件适配层保留未知兼容字段。
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PhoneSettings {
    pub mode: String,
    pub reply_min_chars: Option<u32>,
    pub reply_max_chars: Option<u32>,
    pub proactive_enabled: bool,
    pub reminder_interval_minutes: u32,
    pub guard_limit: u32,
    pub model_override_enabled: bool,
    pub model_override_model: Option<Value>,
}
impl PhoneSettings {
    pub fn read(meta: &Value, members: usize) -> Result<Self, String> {
        let mapped = json!({
            "mode":meta.get("agentPhoneToolMode").cloned().unwrap_or(json!("read_only")),
            "replyMinChars":meta.get("agentPhoneReplyMinChars").cloned().unwrap_or(Value::Null),
            "replyMaxChars":meta.get("agentPhoneReplyMaxChars").cloned().unwrap_or(Value::Null),
            "proactiveEnabled":meta.get("agentPhoneProactiveEnabled").cloned().unwrap_or(json!(true)),
            "reminderIntervalMinutes":meta.get("agentPhoneReminderIntervalMinutes").cloned().unwrap_or(json!(31)),
            "guardLimit":meta.get("agentPhoneGuardLimit").cloned().unwrap_or(json!((members.max(1)*12).min(64))),
            "modelOverrideEnabled":meta.get("agentPhoneModelOverrideEnabled").cloned().unwrap_or(json!(false)),
            "modelOverrideModel":meta.get("agentPhoneModelOverrideModel").cloned().unwrap_or_else(|| match (meta.get("agentPhoneModelOverrideId"),meta.get("agentPhoneModelOverrideProvider")) { (Some(id),Some(provider)) => json!({"id":id,"provider":provider}), _=>Value::Null })
        });
        Self::parse(mapped, true)
    }
    fn parse(mut value: Value, legacy: bool) -> Result<Self, String> {
        if legacy {
            for key in [
                "replyMinChars",
                "replyMaxChars",
                "reminderIntervalMinutes",
                "guardLimit",
            ] {
                if let Some(text) = value[key].as_str() {
                    value[key] =
                        json!(text.parse::<u32>().map_err(|_| format!("invalid {key}"))?);
                }
            }
            for key in ["proactiveEnabled", "modelOverrideEnabled"] {
                if let Some(text) = value[key].as_str() {
                    value[key] = json!(match text {
                        "true" => true,
                        "false" => false,
                        _ => return Err(format!("invalid {key}")),
                    });
                }
            }
        }
        let result: Self = serde_json::from_value(value).map_err(|e| e.to_string())?;
        result.validate()?;
        Ok(result)
    }
    pub fn update(&self, patch: &Value) -> Result<Self, String> {
        let patch = patch
            .as_object()
            .ok_or("phone settings must be an object")?;
        let mut value = serde_json::to_value(self).expect("手机配置序列化");
        for (key, item) in patch {
            value[key] = item.clone();
        }
        Self::parse(value, false)
    }
    fn validate(&self) -> Result<(), String> {
        if !matches!(self.mode.as_str(), "read_only" | "write")
            || self.reminder_interval_minutes == 0
            || self.reminder_interval_minutes.checked_mul(60_000).is_none()
            || !(1..=64).contains(&self.guard_limit)
            || self.reply_min_chars == Some(0)
            || self.reply_max_chars == Some(0)
            || matches!((self.reply_min_chars,self.reply_max_chars),(Some(a),Some(b)) if a>b)
        {
            return Err("invalid phone permission, interval, budget or reply range".into());
        }
        if self.model_override_enabled && self.model_override_model.is_none() {
            return Err("model override requires a model".into());
        }
        if let Some(model) = &self.model_override_model {
            for key in ["id", "provider"] {
                if model[key].as_str().is_none_or(|s| s.trim().is_empty()) {
                    return Err(format!("model {key} required"));
                }
            }
        }
        Ok(())
    }
    pub fn patch(&self) -> Value {
        json!({"agentPhoneToolMode":self.mode,"agentPhoneReplyMinChars":self.reply_min_chars,
            "agentPhoneReplyMaxChars":self.reply_max_chars,"agentPhoneProactiveEnabled":self.proactive_enabled,
            "agentPhoneReminderIntervalMinutes":self.reminder_interval_minutes,"agentPhoneGuardLimit":self.guard_limit,
            "agentPhoneModelOverrideEnabled":self.model_override_enabled,
            "agentPhoneModelOverrideId":self.model_override_model.as_ref().and_then(|v|v["id"].as_str()),"agentPhoneModelOverrideProvider":self.model_override_model.as_ref().and_then(|v|v["provider"].as_str())})
    }
    pub fn read_dm(meta: &Value) -> Result<Self, String> {
        let mut mapped = json!({});
        for (plain, prefixed) in Self::fields() {
            if let Some(v) = meta.get(plain) {
                mapped[prefixed] = v.clone();
            }
        }
        Self::read(&mapped, 2)
    }
    fn fields() -> [(&'static str, &'static str); 9] {
        [
            ("toolMode", "agentPhoneToolMode"),
            ("replyMinChars", "agentPhoneReplyMinChars"),
            ("replyMaxChars", "agentPhoneReplyMaxChars"),
            ("proactiveEnabled", "agentPhoneProactiveEnabled"),
            (
                "reminderIntervalMinutes",
                "agentPhoneReminderIntervalMinutes",
            ),
            ("guardLimit", "agentPhoneGuardLimit"),
            ("modelOverrideEnabled", "agentPhoneModelOverrideEnabled"),
            ("modelOverrideId", "agentPhoneModelOverrideId"),
            ("modelOverrideProvider", "agentPhoneModelOverrideProvider"),
        ]
    }
    pub fn patch_dm(&self) -> Value {
        let mut result = json!({});
        let patch = self.patch();
        for (plain, prefixed) in Self::fields() {
            result[plain] = patch[prefixed].clone();
        }
        result
    }
    pub fn permission(&self) -> lingxi_kernel::subagent::SessionPermissionMode {
        if self.mode == "write" {
            lingxi_kernel::subagent::SessionPermissionMode::Operate
        } else {
            lingxi_kernel::subagent::SessionPermissionMode::ReadOnly
        }
    }
}

pub(crate) fn forbidden_tool(name: &str) -> bool {
    let name = name.rsplit(':').next().unwrap_or(name);
    matches!(name, "browser" | "chrome" | "channel" | "dm")
}
pub(crate) fn control_tool(name: &str) -> bool {
    matches!(
        name.rsplit(':').next().unwrap_or(name),
        "channel_reply" | "channel_pass" | "channel_read_context"
    )
}
