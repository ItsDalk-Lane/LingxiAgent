//! 会话和新建草稿共用唯一模型/思考设置；下一 Run 从这里读取实际选择。
use crate::ServiceState;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, sync::Mutex};
const MAX_STREAMS: usize = 1024;
static SETTINGS_LOCK: Mutex<()> = Mutex::new(());
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatChoice {
    pub model: Option<(String, String)>,
    pub thinking_level: Option<String>,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Choices {
    defaults: BTreeMap<String, ChatChoice>,
    sessions: BTreeMap<String, ChatChoice>,
}
fn read_choices(home: &Path) -> Result<Choices, String> {
    let home = &home
        .canonicalize()
        .map_err(|error| format!("chat settings root: {error}"))?;
    match std::fs::symlink_metadata(home.join("chat-input-settings.json")) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Choices::default()),
        Err(e) => Err(e.to_string()),
        Ok(_) => {
            let s = crate::file_safety::Snapshot::read(&home.join("chat-input-settings.json"))
                .map_err(|e| format!("chat preferences: {e:?}"))?;
            if s.bytes.len() > 1024 * 1024 {
                return Err("chat preferences exceed bound".into());
            }
            serde_json::from_slice(&s.bytes).map_err(|e| e.to_string())
        }
    }
}
pub(crate) fn choice(home: &Path, owner: &str, session: &str) -> Result<ChatChoice, String> {
    let rows = read_choices(home)?;
    Ok(rows
        .sessions
        .get(session)
        .or_else(|| rows.defaults.get(owner))
        .cloned()
        .unwrap_or_default())
}
pub(crate) fn save_choice(
    home: &Path,
    owner: &str,
    session: Option<&str>,
    choice: ChatChoice,
) -> Result<(), String> {
    let _lock = SETTINGS_LOCK
        .lock()
        .map_err(|_| "chat settings lock poisoned")?;
    let mut rows = read_choices(home)?;
    match session {
        Some(id) => {
            rows.sessions.insert(id.into(), choice);
        }
        None => {
            rows.defaults.insert(owner.into(), choice);
        }
    }
    if rows.sessions.len() > MAX_STREAMS || rows.defaults.len() > MAX_STREAMS {
        return Err("chat settings capacity exceeded".into());
    }
    write_choices(home, &rows)
}
/// 创建方持有新 SID admission；已有预备行也不得被覆盖。
pub(crate) fn save_new_session_choice(
    home: &Path,
    owner: &str,
    session: &str,
    choice: ChatChoice,
) -> Result<(), String> {
    if owner.is_empty() || session.is_empty() {
        return Err("new session identity missing".into());
    }
    let _lock = SETTINGS_LOCK
        .lock()
        .map_err(|_| "chat settings lock poisoned")?;
    let mut rows = read_choices(home)?;
    if rows.sessions.contains_key(session) {
        return Err("new session already has model settings".into());
    }
    if rows.sessions.len() >= MAX_STREAMS {
        return Err("chat settings capacity exceeded".into());
    }
    rows.sessions.insert(session.into(), choice);
    write_choices(home, &rows)
}
fn write_choices(home: &Path, rows: &Choices) -> Result<(), String> {
    let home = &home
        .canonicalize()
        .map_err(|error| format!("chat settings root: {error}"))?;
    let bytes = serde_json::to_vec(rows).map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("chat settings exceed byte bound".into());
    }
    match std::fs::symlink_metadata(home.join("chat-input-settings.json")) {
        Ok(_) => {
            let snapshot =
                crate::file_safety::Snapshot::read(&home.join("chat-input-settings.json"))
                    .map_err(|e| format!("{e:?}"))?;
            crate::file_safety::FileSafety::new(home).replace(
                "chat-settings",
                &home.join("chat-input-settings.json"),
                &snapshot,
                &bytes,
                || true,
            )
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            crate::file_safety::FileSafety::new(home).create(
                "chat-settings",
                &home.join("chat-input-settings.json"),
                &bytes,
                || true,
            )
        }
        Err(e) => return Err(e.to_string()),
    }
    .map_err(|e| format!("chat settings save: {e:?}"))
}
/// 仅供持有全新 SID admission、且尚未建立该会话的创建方撤回自己的预备行。
pub(crate) fn remove_new_session_choice(
    home: &Path,
    owner: &str,
    session: &str,
) -> Result<(), String> {
    if owner.is_empty() || session.is_empty() {
        return Err("new session cleanup identity missing".into());
    }
    let _lock = SETTINGS_LOCK
        .lock()
        .map_err(|_| "chat settings lock poisoned")?;
    let mut rows = read_choices(home)?;
    if rows.sessions.remove(session).is_none() {
        return Ok(());
    }
    write_choices(home, &rows)
}
/// 调用方必须先取得当前授权事实；此处只维护唯一会话模型/思考真相。
pub(crate) fn read_session_choice(
    state: &ServiceState,
    facts: &crate::sessions::SessionFacts,
) -> Result<ChatChoice, String> {
    effective_choice_for_agent(
        state,
        &facts.owner_user_id,
        &facts.session_id,
        Some(&facts.agent_id),
        None,
    )
}
/// 所有前台执行入口共同读取明确助手的默认模型；不依赖当前界面焦点。
pub(crate) fn choice_for_agent(
    home: &Path,
    owner: &str,
    session: &str,
    agent_id: Option<&str>,
) -> Result<ChatChoice, String> {
    let mut choice = choice(home, owner, session)?;
    if choice.model.is_none() {
        if let Some(agent_id) = agent_id {
            lingxi_adapters::storage::agent_memory::validate_agent_id(agent_id)
                .map_err(|e| e.to_string())?;
            let path = home.join("agents").join(agent_id).join("config.yaml");
            #[cfg(windows)]
            use crate::discovery_tools::windows_readonly::DirFd as ReadonlyDir;
            #[cfg(windows)]
            let root = ReadonlyDir::open(home).map_err(|e| e.to_string())?;
            #[cfg(not(windows))]
            let root = crate::filetools::fsio::DirFd::open(home).map_err(|e| e.to_string())?;
            // Windows复用现有只读实现逐组件拒绝reparse并保留祖先句柄，不触碰写入边界。
            #[cfg(windows)]
            let agents = match ReadonlyDir::open(&home.join("agents")) {
                Ok(dir) => dir,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(choice),
                Err(error) => return Err(error.to_string()),
            };
            #[cfg(not(windows))]
            let agents = match root.child_directory(std::ffi::OsStr::new("agents"), false) {
                Ok(dir) => dir,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(choice),
                Err(error) => return Err(error.to_string()),
            };
            #[cfg(windows)]
            let directory = match ReadonlyDir::open(&home.join("agents").join(agent_id)) {
                Ok(dir) => dir,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(choice),
                Err(error) => return Err(error.to_string()),
            };
            #[cfg(not(windows))]
            let directory = match agents.child_directory(std::ffi::OsStr::new(agent_id), false) {
                Ok(dir) => dir,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(choice),
                Err(error) => return Err(error.to_string()),
            };
            let directory_is_current = || {
                root.same_directory(home).unwrap_or(false)
                    && agents.same_directory(&home.join("agents")).unwrap_or(false)
                    && directory
                        .same_directory(path.parent().expect("固定助手配置路径"))
                        .unwrap_or(false)
            };
            // 只有数据库历史的旧会话可能没有助手文件；沿既有全局模型规则读取，绝不转到焦点助手。
            match std::fs::symlink_metadata(&path) {
                Err(error)
                    if error.kind() == std::io::ErrorKind::NotFound && directory_is_current() =>
                {
                    return Ok(choice)
                }
                Err(error) => return Err(error.to_string()),
                Ok(_) => {}
            }
            let snapshot = crate::file_safety::Snapshot::read_bounded(&path, 1024 * 1024)
                .map_err(|e| e.to_string())?;
            let text = std::str::from_utf8(&snapshot.bytes).map_err(|e| e.to_string())?;
            let document =
                lingxi_adapters::storage::agent_memory::config::AgentConfigDocument::parse(text)
                    .map_err(|e| e.to_string())?;
            let selected = document
                .model_reference("models", "chat")
                .map_err(|e| e.to_string())?;
            if !snapshot.path_is_current(&path) || !directory_is_current() {
                return Err("agent configuration changed while selecting model".into());
            }
            choice.model = selected;
        }
    }
    Ok(choice)
}

/// 明确目标助手的默认模型不能借服务焦点切换到另一位助手。
pub(crate) fn effective_choice_for_agent(
    state: &ServiceState,
    owner: &str,
    session: &str,
    agent_id: Option<&str>,
    model: Option<(String, String)>,
) -> Result<ChatChoice, String> {
    let mut choice = if model.is_some() {
        choice(state.memory().home(), owner, session)?
    } else {
        choice_for_agent(state.memory().home(), owner, session, agent_id)?
    };
    if let Some(model) = model {
        choice.model = Some(model);
    }
    if choice.model.is_none() {
        choice.model = state.model_gateway().and_then(|g| {
            g.configured_routes()
                .get("chat")
                .map(|b| (b.provider.clone(), b.model.clone()))
        });
    }
    if choice.thinking_level.is_none() {
        choice.thinking_level = Some(selected_thinking(state, &choice)?);
    }
    Ok(choice)
}
pub(crate) fn write_session_choice(
    state: &ServiceState,
    facts: &crate::sessions::SessionFacts,
    choice: ChatChoice,
) -> Result<(), String> {
    if let Some((provider, model)) = &choice.model {
        if provider.trim().is_empty() || model.trim().is_empty() {
            return Err("model identity must not be empty".into());
        }
        state
            .model_gateway()
            .ok_or("model plane unavailable")?
            .resolve_dispatch(&lingxi_kernel::model_exchange::ModelRouteRequest {
                operation: lingxi_kernel::model_exchange::ModelOperation::Chat,
                provider: Some(provider.clone()),
                model: Some(model.clone()),
            })
            .map_err(|e| e.to_string())?;
    }
    if choice
        .thinking_level
        .as_deref()
        .is_some_and(|level| !matches!(level, "off" | "low" | "medium" | "high" | "xhigh" | "max"))
    {
        return Err("invalid normalized thinking level".into());
    }
    check_thinking(state, &choice)?;
    save_choice(
        state.memory().home(),
        &facts.owner_user_id,
        Some(&facts.session_id),
        choice,
    )
}

/// 未配置的完整 pair 不获得能力；保存成功必须对应真实可派发模型。
pub(crate) fn dispatch_choice(
    state: &ServiceState,
    choice: &ChatChoice,
) -> Result<lingxi_adapters::models::gateway::ResolvedDispatch, String> {
    let (provider, model) = choice
        .model
        .as_ref()
        .map(|(p, m)| (Some(p.clone()), Some(m.clone())))
        .unwrap_or((None, None));
    let dispatch = state
        .model_gateway()
        .ok_or("model plane unavailable")?
        .resolve_dispatch(&lingxi_kernel::model_exchange::ModelRouteRequest {
            operation: lingxi_kernel::model_exchange::ModelOperation::Chat,
            provider,
            model,
        })
        .map_err(|e| e.to_string())?;
    if dispatch.capabilities.tools != Some(true) {
        return Err("selected chat model has not declared tool support".into());
    }
    Ok(dispatch)
}
pub(crate) fn check_thinking(state: &ServiceState, choice: &ChatChoice) -> Result<(), String> {
    let dispatch = dispatch_choice(state, choice)?;
    let Some(level) = choice.thinking_level.as_deref() else {
        return Ok(());
    };
    if dispatch.compat.as_ref().and_then(|c| c.reasoning) != Some(true) && level != "off" {
        return Err("selected model has not declared configurable reasoning".into());
    }
    let call = lingxi_adapters::models::compat::CompatCall {
        hints: dispatch.compat,
        options: lingxi_adapters::models::compat::CompatOptions {
            reasoning_level: Some(level.into()),
            ..Default::default()
        },
    };
    lingxi_adapters::models::compat::apply_for_call(
        serde_json::json!({"model":dispatch.route.model,"messages":[]}),
        &dispatch.route,
        Some(&call),
    )
    .map_err(|error| error.message)?;
    Ok(())
}
pub(crate) fn selected_thinking(
    state: &ServiceState,
    choice: &ChatChoice,
) -> Result<String, String> {
    let dispatch = dispatch_choice(state, choice)?;
    Ok(
        if dispatch.compat.as_ref().and_then(|c| c.reasoning) == Some(true) {
            choice
                .thinking_level
                .clone()
                .unwrap_or_else(|| "medium".into())
        } else {
            "off".into()
        },
    )
}

#[cfg(test)]
mod agent_default_tests {
    use super::*;
    #[test]
    fn explicit_session_and_owner_choices_precede_exact_agent_and_missing_legacy_never_uses_focus()
    {
        let scratch = std::env::temp_dir().join(format!(
            "lingxi-t08-chat-choice-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        std::fs::create_dir_all(scratch.join("agents/a")).unwrap();
        std::fs::create_dir_all(scratch.join("agents/b")).unwrap();
        let home = scratch.canonicalize().unwrap();
        for agent in ["a", "b"] {
            std::fs::write(
                home.join(format!("agents/{agent}/config.yaml")),
                format!("models:\n  chat:\n    provider: fixture\n    id: agent-{agent}\n"),
            )
            .unwrap();
        }
        assert_eq!(
            choice_for_agent(&home, "owner", "new", Some("b"))
                .unwrap()
                .model,
            Some(("fixture".into(), "agent-b".into()))
        );
        assert_eq!(
            choice_for_agent(&home, "owner", "legacy", Some("missing"))
                .unwrap()
                .model,
            None
        );
        #[cfg(unix)]
        {
            // 最终文件不存在也必须检查父链，悬空链接不能冒充旧会话的正常缺失。
            std::os::unix::fs::symlink(home.join("no-such-target"), home.join("agents/dangling"))
                .unwrap();
            assert!(choice_for_agent(&home, "owner", "legacy", Some("dangling")).is_err());
            std::fs::create_dir(home.join("outside-empty")).unwrap();
            std::os::unix::fs::symlink(home.join("outside-empty"), home.join("agents/linked"))
                .unwrap();
            assert!(choice_for_agent(&home, "owner", "legacy", Some("linked")).is_err());
        }
        std::fs::write(
            home.join("agents/b/config.yaml"),
            "models: [broken-shape]\n",
        )
        .unwrap();
        assert!(choice_for_agent(&home, "owner", "new", Some("b")).is_err());
        save_choice(
            &home,
            "owner",
            None,
            ChatChoice {
                model: Some(("fixture".into(), "user-default".into())),
                thinking_level: None,
            },
        )
        .unwrap();
        assert_eq!(
            choice_for_agent(&home, "owner", "new", Some("a"))
                .unwrap()
                .model,
            Some(("fixture".into(), "user-default".into()))
        );
        save_choice(
            &home,
            "owner",
            Some("explicit"),
            ChatChoice {
                model: Some(("fixture".into(), "session-choice".into())),
                thinking_level: Some("off".into()),
            },
        )
        .unwrap();
        let chosen = choice_for_agent(&home, "owner", "explicit", Some("b")).unwrap();
        assert_eq!(
            chosen.model,
            Some(("fixture".into(), "session-choice".into()))
        );
        assert_eq!(chosen.thinking_level.as_deref(), Some("off"));
        std::fs::remove_dir_all(home).unwrap();
    }
}
