/**
 * useGitEnv — Git 环境探测共享层
 *
 * Git 入口分散在三处（工作台首行变更按钮、输入框左上角分支下拉、源代码管理面板），
 * 但探测的是同一目录的同一份数据：status / branches / worktrees。
 * 这里用模块级缓存 + in-flight 去重把探测收敛成一次：
 *   - 挂载时有缓存先秒出、后台再刷新（stale-while-revalidate，原 GitEnvironmentCard 语义）
 *   - 同目录并发请求只发一份；操作（切换分支/提交/推送）后用 refresh() 强制刷新并回写缓存
 * 缓存仅保留每个目录最新一份；测试用 resetGitEnvCache 隔离模块级状态。
 */
import { useCallback, useEffect, useState } from 'react';
import {
  fetchGitBranches,
  fetchGitStatus,
  fetchGitWorktrees,
  type GitBranches,
  type GitStatus,
  type GitWorktrees,
} from '../utils/git-env-api';

export interface GitEnvData {
  status: GitStatus | null;
  branches: GitBranches | null;
  worktrees: GitWorktrees | null;
}

interface GitEnvSnapshot extends GitEnvData {
  at: number;
}

export type GitEnvLoadState = 'idle' | 'loading' | 'error';

const snapshotCache = new Map<string, GitEnvSnapshot>();
const inflight = new Map<string, Promise<GitEnvSnapshot>>();

/** 测试隔离用：清空缓存与在途请求（模块级状态，vitest 用例间不会自动重置） */
export function resetGitEnvCache(): void {
  snapshotCache.clear();
  inflight.clear();
}

function loadEnv(dir: string, agentId: string | null): Promise<GitEnvSnapshot> {
  const running = inflight.get(dir);
  if (running) return running;
  const task = (async (): Promise<GitEnvSnapshot> => {
    try {
      const [status, branches, worktrees] = await Promise.all([
        fetchGitStatus(dir, agentId),
        fetchGitBranches(dir, agentId),
        fetchGitWorktrees(dir, agentId),
      ]);
      const snap: GitEnvSnapshot = { status, branches, worktrees, at: Date.now() };
      snapshotCache.set(dir, snap);
      return snap;
    } finally {
      inflight.delete(dir);
    }
  })();
  inflight.set(dir, task);
  return task;
}

const EMPTY: GitEnvData = { status: null, branches: null, worktrees: null };

export function useGitEnv(dir: string | null, agentId: string | null): GitEnvData & { loadState: GitEnvLoadState; refresh: () => Promise<GitStatus | null> } {
  const [data, setData] = useState<GitEnvData>(() => {
    const snap = dir ? snapshotCache.get(dir) : undefined;
    return snap ? { status: snap.status, branches: snap.branches, worktrees: snap.worktrees } : EMPTY;
  });
  // 有缓存先按 idle 出（后台刷新），无缓存标 loading
  const [loadState, setLoadState] = useState<GitEnvLoadState>(() => (dir && !snapshotCache.has(dir) ? 'loading' : 'idle'));

  // 目录 / agent 切换 → 缓存先出再后台刷新；无目录 → 清空
  useEffect(() => {
    if (!dir) {
      setData(EMPTY);
      setLoadState('idle');
      return;
    }
    const snap = snapshotCache.get(dir);
    if (snap) {
      setData({ status: snap.status, branches: snap.branches, worktrees: snap.worktrees });
      setLoadState('idle');
    } else {
      setData(EMPTY);
      setLoadState('loading');
    }
    let alive = true;
    loadEnv(dir, agentId).then(next => {
      if (!alive) return;
      setData({ status: next.status, branches: next.branches, worktrees: next.worktrees });
      setLoadState('idle');
    }).catch(() => {
      if (alive) setLoadState('error');
    });
    return () => { alive = false; };
  }, [dir, agentId]);

  /** 操作（checkout / 提交 / 推送 / 新建 worktree）后刷新：写缓存并更新本 hook 状态 */
  const refresh = useCallback(async (): Promise<GitStatus | null> => {
    if (!dir) return null;
    try {
      const snap = await loadEnv(dir, agentId);
      setData({ status: snap.status, branches: snap.branches, worktrees: snap.worktrees });
      setLoadState('idle');
      return snap.status;
    } catch {
      setLoadState('error');
      return null;
    }
  }, [dir, agentId]);

  return { ...data, loadState, refresh };
}
