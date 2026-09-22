import { isPlanFileWrite } from "../lib/plan-mode/plan-file.ts";

export const SESSION_PERMISSION_MODES = Object.freeze({
  AUTO: "auto",
  OPERATE: "operate",
  ASK: "ask",
  READ_ONLY: "read_only",
});

export const SESSION_APPROVAL_POLICIES = Object.freeze({
  INTERACTIVE: "interactive",
  DENY_ON_PROMPT: "deny_on_prompt",
  NEVER: "never",
});

export const DEFAULT_SESSION_PERMISSION_MODE = SESSION_PERMISSION_MODES.AUTO;
const BRIDGE_PERMISSION_MODE_VALUES = new Set<string>([
  SESSION_PERMISSION_MODES.AUTO,
  SESSION_PERMISSION_MODES.OPERATE,
  SESSION_PERMISSION_MODES.READ_ONLY,
]);
const AUTOMATION_PERMISSION_MODE_VALUES = BRIDGE_PERMISSION_MODE_VALUES;

const INFORMATION_TOOLS = new Set([
  "read",
  "grep",
  "find",
  "ls",
  "web_search",
  "web_fetch",
  "current_status",
  "search_memory",
  "recall_experience",
  "ask_user",
]);

const SIDE_EFFECT_TOOLS = new Set([
  "bash",
  "exec_command",
  "write_stdin",
  "write",
  "edit",
  "computer",
  "automation",
  "cron",
  "dm",
  "channel",
  "install_skill",
  "learn_lesson",
  "update_settings",
  "todo_write",
  "stage_files",
  "subagent",
  "workflow",
  "notify",
  "record_experience",
  "pin_memory",
  "unpin_memory",
]);

const AUTO_REVIEW_TOOLS = new Set([
  "automation",
  "browser",
  "channel",
  "dm",
  "notify",
  "pin_memory",
  "record_experience",
  "stage_files",
  "terminal",
  "write_stdin",
  "unpin_memory",
  "update_settings",
]);

// subagent 上下文固定边界（与 permission mode 无关）：哪怕 operate 也拦。收口在拦截层而非剥离——
// subagent 工具对模型仍可见，调用时被拦（Codex 式甲），保证缓存前缀统一。未来加禁用工具加到这里。
// 范畴：① 防自递归与间接扇出；② 长期记忆（subagent 不碰）；③ agent 一生/对外副作用。
// 不含 computer（有独立全局开关兜底）、search_memory/recall_experience（只读记忆，允许查）。
const SUBAGENT_BLOCKED_TOOLS = new Set([
  // ① 扇出
  "subagent",          // 防自递归
  "workflow",          // 间接扇出
  "session",           // 跨 session 扇出（触发别的 session 跑回合）
  // ② 长期记忆（与「subagent 不带长期记忆」原则一致：可读不可写）
  "pin_memory",
  "unpin_memory",
  "record_experience",
  "tenet_propose",
  // ③ agent 生命周期 / 对外副作用
  "automation",
  "cron",
  "channel",
  "dm",
  "notify",
  "install_skill",
  "learn_lesson",      // 与 install_skill 同类：写共享技能池
  "ask_user",          // 阻塞等用户作答；子代理应把不确定上报给父会话而非隔空提问
  "update_settings",
  "session_folders",
  "loop_control",      // 循环归主会话管，子代理不得约闹钟/收束循环
  "knowledge_manage",  // 知识库修改面：导入/移除/刷新/重建只归主会话（读侧 knowledge_read/outline/grep 不拦）
]);

// session 工具（跨 session 协作）：读侧零副作用；send/create 的 execute 只产草稿卡，
// 真正副作用发生在用户点击确认卡之后——卡即权限关卡（spec 决策 3），
// 故不进 AUTO_REVIEW（LLM 审查双重把关且非确定，灰测已实证会误拒）。
const SESSION_COLLAB_READ_ACTIONS = new Set(["?", "list", "read"]);

// session_folders 的免审动作集。shared/tool-categories.ts 用字面集合镜像了
// 这两个集合（shared 层不依赖 core），tests/on-demand-first-party.test.ts
// 逐字比对两侧防止漂移；这里导出以便该测试直接引用权威定义。
export const SESSION_FOLDERS_READ_ACTIONS = new Set(["list"]);

export const FILE_READ_ACTIONS = new Set([
  "stat",
]);

const DECLARED_READ_KINDS = new Set([
  "read",
  "readonly",
  "read_only",
]);

const DECLARED_AUTO_ALLOW_KINDS = new Set([
  "plugin_output",
  "session_file_output",
]);

const EXTERNAL_ROUTINE_TARGET_TYPES = new Set([
  "url",
  "browser_tab",
  "channel",
  "channel_draft",
  "agent",
  "notification_route",
]);

export type SessionPermissionMode = typeof SESSION_PERMISSION_MODES[keyof typeof SESSION_PERMISSION_MODES];
type PermissionContext = Record<string, unknown>;
type PermissionDecision = {
  action: 'allow' | 'deny' | 'prompt' | 'review';
  code?: string;
  message?: string;
  kind?: string;
  details?: { toolName: string; layer?: string };
};

function objectFields(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === 'object' ? value as Record<string, unknown> : {};
}

export function normalizeSessionPermissionMode(input: unknown): SessionPermissionMode {
  const raw = typeof input === 'string' ? input : objectFields(input);
  if (typeof raw === "string") return normalizeSessionPermissionMode({ permissionMode: raw });
  if (raw?.permissionMode === SESSION_PERMISSION_MODES.AUTO) return SESSION_PERMISSION_MODES.AUTO;
  if (raw?.permissionMode === SESSION_PERMISSION_MODES.OPERATE) return SESSION_PERMISSION_MODES.OPERATE;
  if (raw?.permissionMode === SESSION_PERMISSION_MODES.ASK) return SESSION_PERMISSION_MODES.ASK;
  if (raw?.permissionMode === SESSION_PERMISSION_MODES.READ_ONLY) return SESSION_PERMISSION_MODES.READ_ONLY;
  if (raw?.accessMode === "operate") return SESSION_PERMISSION_MODES.OPERATE;
  if (raw?.accessMode === "read_only") return SESSION_PERMISSION_MODES.READ_ONLY;
  if (raw?.planMode === true) return SESSION_PERMISSION_MODES.READ_ONLY;
  return DEFAULT_SESSION_PERMISSION_MODE;
}

export function normalizeBridgePermissionMode(input: unknown) {
  const raw = objectFields(input);
  const source = typeof input === "string" ? input : raw.permissionMode;
  if (typeof source === "string" && BRIDGE_PERMISSION_MODE_VALUES.has(source)) return source;
  if (raw?.readOnly === true) return SESSION_PERMISSION_MODES.READ_ONLY;
  return SESSION_PERMISSION_MODES.AUTO;
}

export function normalizeAutomationPermissionMode(input: unknown) {
  const raw = objectFields(input);
  const source = typeof input === "string" ? input : raw.permissionMode;
  if (typeof source === "string" && AUTOMATION_PERMISSION_MODE_VALUES.has(source)) return source;
  return SESSION_PERMISSION_MODES.AUTO;
}

export function normalizeSessionApprovalPolicy(input: unknown) {
  const raw = objectFields(input);
  const source = typeof input === "string" ? input : raw.approvalPolicy;
  if (source === SESSION_APPROVAL_POLICIES.INTERACTIVE) return SESSION_APPROVAL_POLICIES.INTERACTIVE;
  if (source === SESSION_APPROVAL_POLICIES.DENY_ON_PROMPT) return SESSION_APPROVAL_POLICIES.DENY_ON_PROMPT;
  if (source === SESSION_APPROVAL_POLICIES.NEVER) return SESSION_APPROVAL_POLICIES.NEVER;
  return SESSION_APPROVAL_POLICIES.INTERACTIVE;
}

export function resolveSessionApprovalPolicy({ mode, approvalPolicy, allowHumanApproval }: { mode?: unknown; approvalPolicy?: unknown; allowHumanApproval?: boolean } = {}) {
  const normalizedMode = normalizeSessionPermissionMode(mode);
  if (normalizedMode === SESSION_PERMISSION_MODES.OPERATE) return SESSION_APPROVAL_POLICIES.NEVER;
  if (normalizedMode === SESSION_PERMISSION_MODES.AUTO) return SESSION_APPROVAL_POLICIES.DENY_ON_PROMPT;
  if (approvalPolicy != null) return normalizeSessionApprovalPolicy(approvalPolicy);
  if (allowHumanApproval === false) return SESSION_APPROVAL_POLICIES.DENY_ON_PROMPT;
  return SESSION_APPROVAL_POLICIES.INTERACTIVE;
}

export function legacyAccessModeFromPermissionMode(mode: unknown) {
  return normalizeSessionPermissionMode(mode) === SESSION_PERMISSION_MODES.READ_ONLY ? "read_only" : "operate";
}

export function isReadOnlyPermissionMode(mode: unknown) {
  return normalizeSessionPermissionMode(mode) === SESSION_PERMISSION_MODES.READ_ONLY;
}

// 拦截分层（#1614）：deny 必须标明是哪一层拦的 + 怎么解锁，让模型/用户能自助走出去。
//   - subagent_blocklist：subagent 固定边界（任何档位都不可用）
//   - subagent_access：subagent 只读档（出路：access:"write" 重派 + 父会话可操作）
//   - conversation：conversation tool mode（出路：会话设置面板切到 write）
//   - session：普通会话只读档，如 plan 模式（出路：切换会话权限档）
function blocked(toolName: string, { code = "ACTION_BLOCKED_BY_READ_ONLY", message, layer = "session" }: { code?: string; message?: string; layer?: string } = {}): PermissionDecision {
  return {
    action: "deny",
    code,
    message: message || `${toolName} is blocked in read-only mode.`,
    details: { toolName, layer },
  };
}

function blockedByReadOnly(toolName: string, context: PermissionContext): PermissionDecision {
  if (context?.isSubagent) {
    return blocked(toolName, {
      layer: "subagent_access",
      message: `${toolName} is blocked: this subagent runs in read-only mode. `
        + `For write access, re-dispatch the subagent with access:"write" — this requires the parent session to be in an operable (non read-only) mode; a subagent's permission can never exceed its parent session.`,
    });
  }
  if (context?.surface === "conversation") {
    return blocked(toolName, {
      layer: "conversation",
      message: `${toolName} is blocked: this conversation's tool permission is read-only. `
        + `The user can switch this conversation to write mode in its conversation settings panel.`,
    });
  }
  return blocked(toolName, {
    layer: "session",
    message: `${toolName} is blocked: this session is in read-only mode. `
      + `Switch the session permission mode out of read-only (e.g. leave plan mode) to use this tool.`,
  });
}

function prompt(toolName: string): PermissionDecision {
  return {
    action: "prompt",
    kind: "tool_action_approval",
    details: { toolName },
  };
}

function review(toolName: string): PermissionDecision {
  return {
    action: "review",
    kind: "tool_action_approval",
    details: { toolName },
  };
}

function declaredToolSessionPermission(context: PermissionContext) {
  const value = context?.toolSessionPermission || context?.sessionPermission;
  return value && typeof value === "object" ? objectFields(value) : null;
}

function hasDeclaredPermissionBoundary(permission: Record<string, unknown> | null) {
  if (!permission) return false;
  return permission.readOnly === true
    || typeof permission.kind === "string"
    || permission.auto === "allow"
    || permission.auto === "review";
}

function isDeclaredReadOnly(permission: Record<string, unknown> | null) {
  if (!permission) return false;
  if (permission.readOnly === true) return true;
  return typeof permission.kind === "string" && DECLARED_READ_KINDS.has(permission.kind);
}

function isDeclaredAutoAllow(permission: Record<string, unknown> | null) {
  if (!permission) return false;
  if (permission.auto === "allow") return true;
  if (permission.auto === "review") return false;
  return typeof permission.kind === "string" && DECLARED_AUTO_ALLOW_KINDS.has(permission.kind);
}

function classifyDeclaredToolPermission(mode: SessionPermissionMode, toolName: string, context: PermissionContext): PermissionDecision | null {
  const permission = declaredToolSessionPermission(context);
  if (!hasDeclaredPermissionBoundary(permission)) return null;
  if (isDeclaredReadOnly(permission)) return { action: "allow" };
  if (mode === SESSION_PERMISSION_MODES.OPERATE) return { action: "allow" };
  if (mode === SESSION_PERMISSION_MODES.READ_ONLY) return blockedByReadOnly(toolName, context);
  if (mode === SESSION_PERMISSION_MODES.AUTO) {
    return isDeclaredAutoAllow(permission) ? { action: "allow" } : review(toolName);
  }
  return prompt(toolName);
}

function classifyResolvedToolInvocation(mode: SessionPermissionMode, toolName: string, context: PermissionContext): PermissionDecision | null {
  const invocation = objectFields(context.toolInvocation);
  if (!context.toolInvocation || typeof context.toolInvocation !== "object") return null;
  if (invocation.kind === "read") return { action: "allow" };
  const routineIsHostPreAuthorized =
    invocation.kind === "routine"
    && Array.isArray(context?.preAuthorizedRoutineCapabilities)
    && context.preAuthorizedRoutineCapabilities.includes(invocation.capability);
  if (routineIsHostPreAuthorized) {
    return { action: "allow" };
  }
  // Session-scoped pre-authorization, granted by an explicit user decision
  // earlier in this same session. Unlike the routine list above this is
  // kind-agnostic: a "review" descriptor is precisely what the user was asked
  // about, so honouring the grant only for routine work would make it useless.
  // The capability string is the whole key, so a grant never widens past the
  // exact invocation it was issued for.
  const invocationIsSessionPreAuthorized =
    typeof invocation.capability === "string"
    && !!invocation.capability
    && Array.isArray(context?.preAuthorizedInvocationCapabilities)
    && context.preAuthorizedInvocationCapabilities.includes(invocation.capability);
  if (invocationIsSessionPreAuthorized) {
    return { action: "allow" };
  }
  if (mode === SESSION_PERMISSION_MODES.OPERATE) return { action: "allow" };
  if (mode === SESSION_PERMISSION_MODES.READ_ONLY) return blockedByReadOnly(toolName, context);
  // Codex-style Auto: actions already contained by the current workspace and
  // hard safety policy are routine work, so they continue without a reviewer.
  // Only boundary-crossing actions use automatic approval review.
  if (invocation.kind === "routine") {
    const targetType = objectFields(invocation.target).type;
    if (
      context?.isPluginTool === true
      || (typeof targetType === "string" && EXTERNAL_ROUTINE_TARGET_TYPES.has(targetType))
    ) {
      return mode === SESSION_PERMISSION_MODES.AUTO
        ? review(toolName)
        : prompt(toolName);
    }
    return mode === SESSION_PERMISSION_MODES.AUTO
      ? { action: "allow" }
      : prompt(toolName);
  }
  if (mode === SESSION_PERMISSION_MODES.AUTO) {
    return review(toolName);
  }
  return prompt(toolName);
}

function classifyExecCommandAction(mode: SessionPermissionMode, params: Record<string, unknown>, context: PermissionContext): PermissionDecision {
  if (mode === SESSION_PERMISSION_MODES.READ_ONLY) return blockedByReadOnly("exec_command", context);
  if (params?.tty === true) {
    if (mode === SESSION_PERMISSION_MODES.ASK) return prompt("exec_command");
  }
  if (mode === SESSION_PERMISSION_MODES.ASK) return prompt("exec_command");
  return { action: "allow" };
}

function classifySessionFoldersAction(mode: SessionPermissionMode, action: unknown, context: PermissionContext): PermissionDecision {
  if (typeof action === "string" && SESSION_FOLDERS_READ_ACTIONS.has(action)) return { action: "allow" };
  if (mode === SESSION_PERMISSION_MODES.READ_ONLY) return blockedByReadOnly("session_folders", context);
  return { action: "allow" };
}

function classifyFileAction(mode: SessionPermissionMode, action: unknown, context: PermissionContext): PermissionDecision {
  if (typeof action === "string" && FILE_READ_ACTIONS.has(action)) return { action: "allow" };
  if (mode === SESSION_PERMISSION_MODES.READ_ONLY) return blockedByReadOnly("file", context);
  if (mode === SESSION_PERMISSION_MODES.ASK) return prompt("file");
  return { action: "allow" };
}

function classifySessionCollabAction(mode: SessionPermissionMode, action: unknown, context: PermissionContext): PermissionDecision {
  if (typeof action === "string" && SESSION_COLLAB_READ_ACTIONS.has(action)) return { action: "allow" };
  if (mode === SESSION_PERMISSION_MODES.READ_ONLY) return blockedByReadOnly("session", context);
  return { action: "allow" };
}

export function classifySessionPermission({ mode, toolName, params: rawParams, context: rawContext }: { mode?: unknown; toolName?: unknown; params?: unknown; context?: unknown } = {}): PermissionDecision {
  const params = objectFields(rawParams);
  const context = objectFields(rawContext);
  let normalized = normalizeSessionPermissionMode(mode);
  const name = typeof toolName === "string" ? toolName : "";
  if (!name) return { action: "allow" };
  // subagent 上下文固定边界（与 mode 无关，优先于其它判定）：防自递归 + 禁越权工具。
  if (context?.isSubagent && SUBAGENT_BLOCKED_TOOLS.has(name)) {
    return blocked(name, {
      code: "ACTION_BLOCKED_IN_SUBAGENT",
      layer: "subagent_blocklist",
      message: `${name} is not available inside a subagent. `
        + `This tool is always blocked in subagent context regardless of access level; perform this action from the parent session instead.`,
    });
  }
  const resolvedInvocation = classifyResolvedToolInvocation(normalized, name, context);
  if (resolvedInvocation) return resolvedInvocation;
  const declared = classifyDeclaredToolPermission(normalized, name, context);
  if (declared) return declared;
  if (INFORMATION_TOOLS.has(name)) return { action: "allow" };
  if (name === "exec_command") return classifyExecCommandAction(normalized, params, context);
  if (name === "session_folders") return classifySessionFoldersAction(normalized, params?.action, context);
  if (name === "file") return classifyFileAction(normalized, params?.action, context);
  if (name === "session") return classifySessionCollabAction(normalized, params?.action, context);
  if (name === "computer") {
    if (normalized === SESSION_PERMISSION_MODES.READ_ONLY) return blockedByReadOnly(name, context);
    return { action: "allow" };
  }
  if (normalized === SESSION_PERMISSION_MODES.OPERATE) return { action: "allow" };
  // 计划模式（只读档）唯一放行写：本会话旁 <sessionId>.plan.md。
  // 模型在收工提醒里拿到的是绝对路径；相对路径不猜 cwd，不命中即照旧全拒。
  if (normalized === SESSION_PERMISSION_MODES.READ_ONLY
    && isPlanFileWrite(name, params, context?.sessionPath)) {
    return { action: "allow" };
  }
  if (normalized === SESSION_PERMISSION_MODES.READ_ONLY) return blockedByReadOnly(name, context);
  if (normalized === SESSION_PERMISSION_MODES.AUTO) {
    if (AUTO_REVIEW_TOOLS.has(name)) return review(name);
    if (SIDE_EFFECT_TOOLS.has(name)) return { action: "allow" };
    return review(name);
  }
  if (SIDE_EFFECT_TOOLS.has(name)) return prompt(name);
  return prompt(name);
}
