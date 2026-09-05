/**
 * git-env-api — 「环境信息」卡的 renderer 侧 HTTP 客户端
 *
 * 与 server/routes/git-environment.ts 一一对应。所有请求都以 dir（工作台
 * 本地目录绝对路径）定位仓库。只读端点对非 git 目录返回 isRepo:false，
 * 调用方按降级展示；操作端点（commit/push/checkout）返回结构化结果对象，
 * httpOk=false 时携带 error / code，由 UI 决定如何提示。
 */

import { lingxiFetch } from '../hooks/use-hana-fetch';

export type GitFileState = 'modified' | 'added' | 'deleted' | 'untracked' | 'binary';

export interface GitFileChange {
  path: string;
  additions: number;
  deletions: number;
  state: GitFileState;
  staged: boolean;
}

export interface GitChangeTotals {
  additions: number;
  deletions: number;
}

export interface GitStatus {
  isRepo: boolean;
  currentBranch: string | null;
  detached: boolean;
  total: GitChangeTotals;
  stagedTotal: GitChangeTotals;
  unstagedTotal: GitChangeTotals;
  files: GitFileChange[];
  hasUpstream: boolean;
  hasRemote: boolean;
  ahead: number;
  behind: number;
  commitable: boolean;
  pushable: boolean;
}

export interface GitBranchEntry {
  name: string;
  current: boolean;
  checkedOutElsewhere: boolean;
}

export interface GitBranches {
  isRepo: boolean;
  branches: GitBranchEntry[];
  detached: boolean;
  current: string | null;
}

export interface GitWorktreeInfo {
  isRepo: boolean;
  isMain: boolean;
  name: string | null;
  branch: string | null;
  path: string | null;
  mainPath: string | null;
}

export interface GitFileDiff {
  path: string;
  patch: string;
  binary: boolean;
}

export interface GitActionResult {
  httpOk: boolean;
  ok?: boolean;
  code?: string;
  error?: string;
  message?: string;
  head?: string;
}

function dirQuery(dir: string, extra: Record<string, string> = {}): string {
  const params = new URLSearchParams({ dir, ...extra });
  return params.toString();
}

function agentFields(agentId?: string | null): Record<string, string> {
  return agentId ? { agentId } : {};
}

export async function fetchGitStatus(dir: string, agentId?: string | null): Promise<GitStatus> {
  const res = await lingxiFetch(`/api/git/status?${dirQuery(dir, agentFields(agentId))}`);
  return res.json();
}

export async function fetchGitBranches(dir: string, agentId?: string | null): Promise<GitBranches> {
  const res = await lingxiFetch(`/api/git/branches?${dirQuery(dir, agentFields(agentId))}`);
  return res.json();
}

export async function fetchGitWorktreeInfo(dir: string, agentId?: string | null): Promise<GitWorktreeInfo> {
  const res = await lingxiFetch(`/api/git/worktree-info?${dirQuery(dir, agentFields(agentId))}`);
  return res.json();
}

export async function fetchGitFileDiff(dir: string, file: string): Promise<GitFileDiff> {
  const res = await lingxiFetch(`/api/git/file-diff?${dirQuery(dir, { file })}`);
  return res.json();
}

export async function gitCheckout(dir: string, branch: string, agentId?: string | null): Promise<GitActionResult> {
  const res = await lingxiFetch('/api/git/checkout', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ dir, branch, ...agentFields(agentId) }),
    throwOnHttpError: false,
  });
  return { httpOk: res.ok, ...(await res.json()) };
}

export async function gitCommit(
  dir: string,
  opts: { message: string; includeUnstaged: boolean; agentId?: string | null },
): Promise<GitActionResult> {
  const res = await lingxiFetch('/api/git/commit', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ dir, ...opts }),
    throwOnHttpError: false,
    timeout: 60_000,
  });
  return { httpOk: res.ok, ...(await res.json()) };
}

export async function gitPush(dir: string, agentId?: string | null): Promise<GitActionResult> {
  const res = await lingxiFetch('/api/git/push', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ dir, ...agentFields(agentId) }),
    throwOnHttpError: false,
    // push 走网络，服务端上限 120s，客户端稍宽
    timeout: 150_000,
  });
  return { httpOk: res.ok, ...(await res.json()) };
}

export async function generateGitCommitMessage(
  dir: string,
  opts: { includeUnstaged: boolean; sessionPath?: string | null; agentId?: string | null },
): Promise<{ httpOk: boolean; message?: string; error?: string }> {
  const res = await lingxiFetch('/api/git/ai-commit-message', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ dir, ...opts }),
    throwOnHttpError: false,
    timeout: 60_000,
  });
  return { httpOk: res.ok, ...(await res.json()) };
}
