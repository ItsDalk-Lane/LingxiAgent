//! Bridge 权限只在宿主解析；旧开关、界面状态和实际执行共用同一个结果。
use crate::memory::MemoryService;
use lingxi_kernel::subagent::SessionPermissionMode;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BridgePermissionMode {
    Auto,
    ReadOnly,
    Operate,
}
impl BridgePermissionMode {
    pub(crate) fn wire_name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::ReadOnly => "read_only",
            Self::Operate => "operate",
        }
    }
    pub(crate) fn session_mode(self) -> SessionPermissionMode {
        match self {
            Self::Auto => SessionPermissionMode::Ask,
            Self::ReadOnly => SessionPermissionMode::ReadOnly,
            Self::Operate => SessionPermissionMode::Operate,
        }
    }
    pub(crate) fn narrows(self, frozen: SessionPermissionMode) -> bool {
        let rank = |mode| match mode {
            SessionPermissionMode::ReadOnly => 0,
            SessionPermissionMode::Ask => 1,
            SessionPermissionMode::Operate => 2,
        };
        rank(self.session_mode()) < rank(frozen)
    }
}

pub(crate) fn parse(root: &Value) -> Result<BridgePermissionMode, String> {
    if !root.is_object()
        || root
            .get("bridge")
            .is_some_and(|bridge| !bridge.is_null() && !bridge.is_object())
    {
        return Err("Bridge permission configuration is invalid".into());
    }
    match root.pointer("/bridge/permissionMode") {
        Some(Value::String(mode)) => match mode.as_str() {
            "auto" => Ok(BridgePermissionMode::Auto),
            "read_only" => Ok(BridgePermissionMode::ReadOnly),
            "operate" => Ok(BridgePermissionMode::Operate),
            _ => Err("Bridge permission mode is invalid".into()),
        },
        None | Some(Value::Null) => match root.get("bridge_read_only") {
            Some(Value::Bool(true)) => Ok(BridgePermissionMode::ReadOnly),
            None | Some(Value::Null) | Some(Value::Bool(false)) => Ok(BridgePermissionMode::Auto),
            _ => Err("Legacy Bridge read-only setting is invalid".into()),
        },
        _ => Err("Bridge permission mode is invalid".into()),
    }
}

pub(crate) fn read_current(home: Option<&Path>) -> Result<BridgePermissionMode, String> {
    let home = home
        .filter(|path| path.is_absolute())
        .ok_or_else(|| "Bridge permission home unavailable".to_string())?;
    parse(&MemoryService::read_preferences_root_at_home(home)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn legacy_and_current_modes_share_one_strict_result() {
        assert_eq!(
            parse(&json!({"bridge_read_only":true}))
                .unwrap()
                .session_mode(),
            SessionPermissionMode::ReadOnly
        );
        assert_eq!(
            parse(&json!({"bridge_read_only":true,"bridge":{"permissionMode":"operate"}})).unwrap(),
            BridgePermissionMode::Operate
        );
        assert!(parse(&json!({"bridge":{"permissionMode":false}})).is_err());
        assert!(parse(&json!({"bridge":"read_only"})).is_err());
        assert!(read_current(None).is_err());
        assert!(BridgePermissionMode::ReadOnly.narrows(SessionPermissionMode::Operate));
        assert!(BridgePermissionMode::Auto.narrows(SessionPermissionMode::Operate));
        assert!(!BridgePermissionMode::Operate.narrows(SessionPermissionMode::ReadOnly));
    }
}
