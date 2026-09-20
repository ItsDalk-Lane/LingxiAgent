/**
 * last-project-identity — 「上次活跃项目身份」的前端本地持久化。
 *
 * 侧栏项目分组的展开规则跟随当前聊天；软件重启后产品停在欢迎页（启动流程
 * 显式置 pendingNewSession，loadSessions 的自动恢复分支因此不触发）。此时
 * 展开规则的 fallback 是「选定的工作台身份」，若不持久化，重启后 selected*
 * 回落 homeFolder，用户上次工作的项目组会被折叠。本模块把最后活跃的项目
 * 身份（mountId / cwd）写到 localStorage，供 app-init 启动时恢复 selected*，
 * 让「当前工作台」与侧栏展开状态都回到上次的项目。
 *
 * 选 localStorage 而非 server 偏好通道：会话列表本身不跨设备同步，「上次在
 * 哪个项目工作」是设备局部状态；且不新增 server 路由与 shared 类型。
 */

export interface LastProjectIdentity {
  workspaceMountId: string | null;
  cwd: string | null;
  /** mount 工作台的显示名；目录身份无 label。 */
  workspaceLabel: string | null;
}

const LAST_PROJECT_IDENTITY_KEY = 'hana-last-project-identity';

function cleanIdentityPart(value: unknown): string | null {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  return trimmed || null;
}

/**
 * 记录一次项目身份变化。mount 与 cwd 同时给出时都保存（读取方以 mount 优先）。
 * 两者都不可靠（空/非字符串）时不覆盖旧值——坏输入不应抹掉上次的记忆。
 * localStorage 不可用（隐私模式/测试环境）时静默跳过：身份恢复退回 homeFolder
 * 默认，不影响任何功能路径。
 */
export function persistLastProjectIdentity(
  identity: { workspaceMountId?: string | null; cwd?: string | null; workspaceLabel?: string | null } | null | undefined,
): void {
  if (!identity) return;
  const mountId = cleanIdentityPart(identity.workspaceMountId);
  const cwd = cleanIdentityPart(identity.cwd);
  const label = cleanIdentityPart(identity.workspaceLabel);
  if (!mountId && !cwd) return;
  try {
    window.localStorage?.setItem(
      LAST_PROJECT_IDENTITY_KEY,
      JSON.stringify({ workspaceMountId: mountId, cwd, workspaceLabel: label }),
    );
  } catch { /* localStorage 不可用：静默 */ }
}

export function readLastProjectIdentity(): LastProjectIdentity | null {
  try {
    const raw = window.localStorage?.getItem(LAST_PROJECT_IDENTITY_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return null;
    const source = parsed as Record<string, unknown>;
    const mountId = cleanIdentityPart(source.workspaceMountId);
    const cwd = cleanIdentityPart(source.cwd);
    const label = cleanIdentityPart(source.workspaceLabel);
    if (!mountId && !cwd) return null;
    return { workspaceMountId: mountId, cwd, workspaceLabel: label };
  } catch {
    // 缓存损坏：等同无记录，走 homeFolder 默认。
    return null;
  }
}
