//! 后台自动化的权限裁决：明确的日常操作继续执行，边界操作必须有真实审核。
//!
//! Ask 在此入口表示现役自动化设置中的 auto，不改变交互会话 Ask 的含义。
//! 本纯分类层不能伪造已审核；边界操作由统一网关调用真实异步审核器并绑定单次授权。
//! 未取得对应授权时明确拒绝，不能转成人工等待。资源和执行时复核仍由统一网关负责。

use lingxi_kernel::subagent::SessionPermissionMode;
use lingxi_kernel::toolcatalog::{tool_target_id, PermissionKind, ToolOrigin};

use crate::approval_service::{ACTION_BLOCKED_BY_READ_ONLY, TOOL_APPROVAL_UNAVAILABLE};
use crate::resourceaccess::ResourceOp;
use crate::toolgateway::{
    InvocationPermissionContext, InvocationPolicyMaterial, PolicyAdjudicationInput, PolicyVerdict,
};

fn unavailable(input: &PolicyAdjudicationInput, reason: &str) -> PolicyVerdict {
    PolicyVerdict::Denied {
        code: TOOL_APPROVAL_UNAVAILABLE.into(),
        layer: "automation_approval".into(),
        message: format!(
            "{} 未执行：{reason}；后台任务无法等待人工确认。",
            input.local_name
        ),
    }
}

fn read_only(input: &PolicyAdjudicationInput) -> PolicyVerdict {
    PolicyVerdict::Denied {
        code: ACTION_BLOCKED_BY_READ_ONLY.into(),
        layer: "automation".into(),
        message: format!("{} 未执行：后台任务继承的权限为只读。", input.local_name),
    }
}

/// 只接受可信入口生成的自动化上下文；调用者不能以参数提供权限或审核结果。
pub fn adjudicate(
    input: &PolicyAdjudicationInput,
    material: Option<&InvocationPolicyMaterial>,
) -> PolicyVerdict {
    let mode = match input.permission_context {
        InvocationPermissionContext::Automation { mode }
        | InvocationPermissionContext::Bridge {
            role: lingxi_protocol::BridgeRole::Owner,
            mode,
        } => mode,
        _ => return unavailable(input, "缺少可信后台自动化身份"),
    };
    if input.local_name == "ask_user" {
        return unavailable(input, "后台任务不能向用户提问并等待回答");
    }
    if input.local_name == "context_notes" && material.is_none() {
        return unavailable(input, "笔记工具的读写动作尚未绑定");
    }
    let action = material
        .and_then(|value| value.effective_arguments.get("action"))
        .and_then(serde_json::Value::as_str);
    // 现役 context_notes 的读写同属一个工具，不能把注册目录的 Read 当成写入豁免。
    let notes_write = input.local_name == "context_notes"
        && material.is_some_and(|value| value.origin == ToolOrigin::FirstParty)
        && matches!(action, Some("write" | "append"));
    let read = input.permission_kind == PermissionKind::Read && !notes_write;
    if mode == SessionPermissionMode::ReadOnly && !read {
        return read_only(input);
    }
    // Review 为已经冻结的强制范围确认边界，operate 也不能越过。
    if input.permission_kind == PermissionKind::Review {
        return unavailable(input, "该操作要求明确审阅具体范围");
    }
    if read {
        return PolicyVerdict::Allowed;
    }
    if mode == SessionPermissionMode::Operate {
        return PolicyVerdict::Allowed;
    }
    let Some(material) = material else {
        return unavailable(input, "自动审核缺少网关已校验的动作和参数");
    };
    if !material.effective_arguments.is_object()
        || lingxi_protocol::digest_arguments(&material.effective_arguments).hex
            != input.args_digest_hex
        || !tool_target_id(&material.origin, &input.local_name)
            .is_ok_and(|target| target.as_str() == input.target_id)
    {
        return unavailable(input, "自动审核材料与已绑定的工具或参数不一致");
    }
    // 插件和 MCP 的自报 read_only 不是授权；真正的 Read 已在上方按注册合同裁定。
    if material.origin != ToolOrigin::FirstParty {
        return unavailable(input, "插件或外部工具尚未取得本次动作的真实自动审核授权");
    }
    let routine = match input.local_name.as_str() {
        "write" | "edit" | "file" => material
            .resources
            .iter()
            .any(|resource| resource.op == ResourceOp::Write),
        // 这些第一方工具在现役权限解析器中属于日常操作；原有硬边界继续执行。
        "bash" | "exec_command" | "computer" | "todo_write" | "subagent" | "subagent_reply"
        | "subagent_close" | "workflow" | "stop_task" | "rewind" | "pin_memory"
        | "record_experience" | "tenet_propose" | "patrol_update_log" | "jian_update_status"
        | "stage_files" => true,
        "context_notes" => matches!(action, None | Some("read" | "write" | "append")),
        "checkpoint" => matches!(action, None | Some("list" | "create" | "drop")),
        "goal" => matches!(
            action,
            None | Some("status" | "create" | "pause" | "resume" | "complete" | "drop")
        ),
        "loop_control" => true,
        "run_code" => matches!(action, None | Some("status" | "run" | "restart")),
        "terminal" => matches!(action, Some("read" | "list" | "close")),
        // 对外发送、共享安装、设置变更和未知操作必须使用真实审核服务。
        _ => false,
    };
    if routine {
        PolicyVerdict::Allowed
    } else {
        unavailable(input, "该操作尚未取得本次动作的真实自动审核授权")
    }
}

/// 仅已知边界动作可以进入真实意图审核；缺少资源、未知动作与强制人工范围确认不能借此绕过。
pub fn requires_automatic_review(
    input: &PolicyAdjudicationInput,
    material: &InvocationPolicyMaterial,
) -> bool {
    if !matches!(
        input.permission_context,
        InvocationPermissionContext::Automation {
            mode: SessionPermissionMode::Ask
        } | InvocationPermissionContext::Bridge {
            role: lingxi_protocol::BridgeRole::Owner,
            mode: SessionPermissionMode::Ask
        }
    ) || matches!(
        input.permission_kind,
        PermissionKind::Read | PermissionKind::Review
    ) || !material.effective_arguments.is_object()
        || lingxi_protocol::digest_arguments(&material.effective_arguments).hex
            != input.args_digest_hex
        || !tool_target_id(&material.origin, &input.local_name)
            .is_ok_and(|target| target.as_str() == input.target_id)
    {
        return false;
    }
    if material.origin != ToolOrigin::FirstParty {
        return true;
    }
    matches!(
        input.local_name.as_str(),
        "notify"
            | "dm"
            | "channel"
            | "browser"
            | "automation"
            | "cron"
            | "install_skill"
            | "learn_lesson"
            | "update_settings"
            | "write_stdin"
            | "show_card"
    ) || (input.local_name == "terminal"
        && matches!(
            material.effective_arguments["action"].as_str(),
            Some("start" | "write")
        ))
}
