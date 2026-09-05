/**
 * workspace-switch — 工作台切换的共享路由逻辑
 *
 * 欢迎页选择器（FolderPicker）与工作台标题下拉（WorkspaceSwitcher）共用同一套
 * 语义，保证「同一个目录只有一种会话身份」：
 * - 挂载工作台 → applyStudioWorkspace（mount 形态）；
 * - Agent 主目录 → 切到该 Agent + mount "default"（绝不用本地路径形态进入，
 *   否则同一目录的 cwd 形态会话与 mount 形态会话分裂成两本账）；
 * - 其余历史目录 → applyFolder（cwd 形态）。
 */

import { lingxiFetch } from '../hooks/use-hana-fetch';
import { useStore } from '../stores';
import { applyFolder, applyStudioWorkspace, createLocalStudioWorkspaceFromFolder, removeRecentWorkspace, removeStudioWorkspace } from '../stores/desk-actions';
import { countArchivedSessionsForWorkspace, disposeWorkspaceSessions } from '../stores/session-actions';
import { loadModels } from './ui-helpers';
import type { Agent } from '../types';
import { isSameWorkspacePath as isSamePath } from './agent-workspace';
import { normalizeWorkspacePath } from '../../../../shared/workspace-history.ts';

export function collectAgentHomeFolders(agents: Agent[]): string[] {
  const folders: string[] = [];
  for (const agent of agents) {
    const folder = normalizeWorkspacePath(agent.homeFolder);
    if (folder && !folders.includes(folder)) folders.push(folder);
  }
  return folders;
}

export function findAgentByHomeFolder(agents: Agent[], folder: string): Agent | null {
  const normalized = normalizeWorkspacePath(folder);
  if (!normalized) return null;
  return agents.find(agent => normalizeWorkspacePath(agent.homeFolder) === normalized) || null;
}

export function refreshModelsAfterAgentModelSwitch(agent: Agent | undefined): void {
  if (agent?.chatModel?.id && agent.chatModel.provider) {
    lingxiFetch('/api/models/set', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ modelId: agent.chatModel.id, provider: agent.chatModel.provider }),
    }).then(() => {
      loadModels();
    }).catch(() => {});
    return;
  }
}

/**
 * 历史目录条目的切换语义（欢迎页 handleSelectHistory 与工作台下拉共用）：
 * 命中某 Agent 主目录时切到该 Agent 并以 mount "default" 进入其默认工作台。
 */
export async function applyWorkspaceHistoryFolder(agents: Agent[], currentAgentId: string | null, folder: string): Promise<void> {
  const agent = findAgentByHomeFolder(agents, folder);
  if (agent) {
    // Agent 主目录 = 默认工作台（mount "default"）的解析根。这里统一走 mount 形态：
    // 若以本地路径形态进入草稿，之后创建的会话只带 cwd，与经切换器创建的 mount 形态
    // 会话分裂成两本账（左栏作用域严格分家），同一目录的对话不聚合。
    // 跨 Agent 选择时 desk 的 mount 根在会话落到目标 Agent 前按当前 Agent 解析，
    // 属草稿期瞬态；首条消息后随 switch 回包归位。
    useStore.setState({
      selectedAgentId: agent.id === currentAgentId ? null : agent.id,
      workspaceFolders: [],
    });
    void applyStudioWorkspace({ mountId: 'default' });
    refreshModelsAfterAgentModelSwitch(agent);
    return;
  }
  void applyFolder(folder);
}

/**
 * 「选择其他文件夹」：本地 folder picker → 建/取 mount 工作台 → 以 mount 形态进入；
 * 同路径下若有被归档的对话，提示可在归档记录中找回（移除工作台即归档的恢复闭环）。
 */
export async function browseAndApplyLocalWorkspace(): Promise<void> {
  const folder = await window.platform?.selectFolder?.();
  if (!folder) return;
  const workspace = await createLocalStudioWorkspaceFromFolder(folder);
  if (!workspace) return;
  await applyStudioWorkspace(workspace);
  const t = window.t ?? ((p: string) => p);
  void countArchivedSessionsForWorkspace({
    workspaceMountId: workspace.mountId,
    cwd: workspace.nativeRootPath ?? undefined,
  }).then((count) => {
    if (count > 0) {
      useStore.getState().addToast(t('workspace.archivedHint', { count }), 'info', 6000);
    }
  });
}

// ── 移除工作台 ──

let removingWorkspaceMountId: string | null = null;

/**
 * 移除一个挂载工作台（欢迎页选择器与工作台下切菜单共用，用户裁决语义）：
 * 名下对话直接归档（不弹二选一，可在归档记录中按工作台分组找回），成功后移除；
 * 无对话直接移除。移除当前活跃工作台时 removeStudioWorkspace 会回落到默认工作台。
 */
export async function removeStudioWorkspaceWithDisposal(mountId: string): Promise<void> {
  if (removingWorkspaceMountId === mountId) return;
  const state = useStore.getState();
  const workspace = (state.studioWorkspaces || []).find((item) => item.mountId === mountId) || null;
  const root = workspace?.nativeRootPath || null;
  // 身份口径与服务端 workspace-disposal 一致：mount 形态 + 同目录的 cwd 形态老会话。
  const count = (state.sessions || []).filter((session: { workspaceMountId?: string | null; cwd?: string | null }) => (
    (session.workspaceMountId && session.workspaceMountId === mountId)
    || (!session.workspaceMountId && !!root && !!session.cwd
      && isSamePath(session.cwd, root))
  )).length;
  const t = window.t ?? ((p: string) => p);
  if (count === 0) {
    void removeStudioWorkspace(mountId);
    return;
  }
  removingWorkspaceMountId = mountId;
  try {
    const result = await disposeWorkspaceSessions({ workspaceMountId: mountId, cwd: root ?? undefined }, 'archive');
    if (!result) {
      useStore.getState().addToast(t('workspace.disposal.failed'), 'error', 6000);
      return;
    }
    await removeStudioWorkspace(mountId);
    useStore.getState().addToast(t('workspace.disposal.archivedToast', { count: result.disposed }), 'success', 6000);
  } finally {
    removingWorkspaceMountId = null;
  }
}

/** 从使用历史列表移除一条目录记录（不切换工作台、不动对话）。 */
export async function removeWorkspaceHistoryEntry(folder: string): Promise<void> {
  await removeRecentWorkspace(folder);
}
