//! 浏览器命令只描述操作；会话身份、权限与执行节点由服务授予。
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BrowserAction {
    Start,
    Stop,
    Navigate,
    Snapshot,
    Screenshot,
    Click,
    Type,
    Scroll,
    Select,
    Key,
    Wait,
    Evaluate,
    Show,
    Back,
    Forward,
    Reload,
    NewTab,
    SwitchTab,
    CloseTab,
    Suspend,
    Resume,
    Destroy,
    Takeover,
    Release,
}
impl BrowserAction {
    pub fn is_tool_action(self) -> bool {
        matches!(
            self,
            Self::Start
                | Self::Stop
                | Self::Navigate
                | Self::Snapshot
                | Self::Screenshot
                | Self::Click
                | Self::Type
                | Self::Scroll
                | Self::Select
                | Self::Key
                | Self::Wait
                | Self::Evaluate
                | Self::Show
        )
    }
    pub fn requires_exact_tab(self) -> bool {
        matches!(
            self,
            Self::Click
                | Self::Type
                | Self::Select
                | Self::Key
                | Self::Evaluate
                | Self::SwitchTab
                | Self::CloseTab
        )
    }
    pub fn modifies_page(self) -> bool {
        matches!(
            self,
            Self::Navigate
                | Self::Click
                | Self::Type
                | Self::Select
                | Self::Key
                | Self::Evaluate
                | Self::Back
                | Self::Forward
                | Self::Reload
                | Self::NewTab
                | Self::CloseTab
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserCommand {
    pub action: BrowserAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub element_ref: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub press_enter: Option<bool>,
}
impl BrowserCommand {
    pub fn new(action: BrowserAction) -> Self {
        Self {
            action,
            tab_id: None,
            url: None,
            element_ref: None,
            text: None,
            direction: None,
            amount: None,
            value: None,
            key: None,
            expression: None,
            timeout: None,
            state: None,
            press_enter: None,
        }
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.tab_id.as_ref().is_some_and(|s| {
            s.is_empty()
                || s.len() > 4096
                || s.trim() != s
                || s.chars().any(char::is_control)
                || s.contains(['*', '?'])
        }) {
            return Err("invalid exact browser tab");
        }
        if self.action.requires_exact_tab() && self.tab_id.is_none() {
            return Err("browser action requires an exact tabId");
        }
        if self
            .url
            .as_ref()
            .is_some_and(|s| s.len() > 8192 || s.chars().any(char::is_control))
        {
            return Err("invalid browser URL");
        }
        if self.text.as_ref().is_some_and(|s| s.len() > 100_000)
            || self.expression.as_ref().is_some_and(|s| s.len() > 10_000)
            || self.value.as_ref().is_some_and(|s| s.len() > 100_000)
            || self.key.as_ref().is_some_and(|s| s.len() > 256)
        {
            return Err("browser text exceeds budget");
        }
        if self.timeout.is_some_and(|n| n == 0 || n > 30_000)
            || self
                .amount
                .is_some_and(|n| !n.is_finite() || n < 0.0 || n > 10_000.0)
        {
            return Err("browser duration or amount exceeds budget");
        }
        match self.action {
            BrowserAction::Navigate if self.url.is_none() => Err("navigate requires URL"),
            BrowserAction::Click | BrowserAction::Select if self.element_ref.is_none() => {
                Err("browser action requires ref")
            }
            BrowserAction::Type if self.text.is_none() => Err("type requires text"),
            BrowserAction::Select if self.value.is_none() => Err("select requires value"),
            BrowserAction::Key if self.key.is_none() => Err("key requires key"),
            BrowserAction::Evaluate if self.expression.is_none() => {
                Err("evaluate requires expression")
            }
            BrowserAction::Scroll if !matches!(self.direction.as_deref(), Some("up" | "down")) => {
                Err("scroll requires up or down")
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum BrowserOpenBehavior {
    #[default]
    Smart,
    CurrentTab,
    NewTab,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserSettings {
    pub accept_cookies: bool,
    pub agent_open_behavior: BrowserOpenBehavior,
}
impl Default for BrowserSettings {
    fn default() -> Self {
        Self {
            accept_cookies: true,
            agent_open_behavior: BrowserOpenBehavior::Smart,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserTab {
    pub tab_id: String,
    pub title: String,
    pub url: Option<String>,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub created_at: String,
    pub updated_at: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserWorkspace {
    pub session_id: String,
    pub session_path: String,
    pub execution_node: String,
    pub running: bool,
    pub resumable: bool,
    pub unavailable_reason: Option<String>,
    pub url: Option<String>,
    pub title: Option<String>,
    pub active_tab_id: Option<String>,
    pub tabs: Vec<BrowserTab>,
    pub suspended: bool,
    pub user_takeover: bool,
    pub revoked: bool,
    pub visible_capable: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserReply {
    pub state: BrowserWorkspace,
    pub result: Value,
    /// 文件已通过正式会话登记；元数据保留兼容字段，不含原始字节。
    #[serde(default)]
    pub files: Vec<Value>,
    /// 下载是否完成与是否登记为文件分别保留真实回执。
    #[serde(default)]
    pub downloads: Vec<Value>,
}
