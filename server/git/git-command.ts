/**
 * server/git/git-command.ts — 环境信息卡的 git 底座
 *
 * execFile 数组参数直跑 git（无 shell，天然免疫注入），带超时与缓冲上限；
 * GIT_TERMINAL_PROMPT=0 保证 push 等远端操作在缺凭据时快速失败而不是挂起，
 * osxkeychain 等凭据助手仍然生效。所有解析器都是纯函数（单测覆盖）。
 *
 * 增删行语义（与「环境信息」卡 UI 对齐）：
 *   - 已跟踪文件：`git diff --numstat`（未暂存）+ `git diff --cached --numstat`（已暂存）
 *   - 未跟踪文件：整文件按新增行计（读取上限 512KB，超出部分不计，二进制记 0）
 *   - total = 已暂存 + 未暂存 + 未跟踪
 */
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import fs from "node:fs";
import path from "node:path";

const execFileAsync = promisify(execFile);

const DEFAULT_TIMEOUT_MS = 15_000;
const PUSH_TIMEOUT_MS = 120_000;
const MAX_BUFFER = 16 * 1024 * 1024;
/** 空树对象哈希：git diff --cached 在零提交仓库里的对照基线 */
const EMPTY_TREE = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
/** 未跟踪文件行数统计的读取上限（更大文件按截断内容计行，轻微低估可接受） */
const UNTRACKED_READ_CAP = 512 * 1024;

export class GitError extends Error {
  stderr: string;
  exitCode: number | null;

  constructor(message: string, stderr = "", exitCode: number | null = null) {
    super(message);
    this.name = "GitError";
    this.stderr = stderr;
    this.exitCode = exitCode;
  }
}

export interface GitRunResult {
  ok: boolean;
  stdout: string;
  stderr: string;
  exitCode: number | null;
}

function gitEnv(): NodeJS.ProcessEnv {
  return {
    ...process.env,
    // 只读命令不抢 index.lock；远端操作禁止终端交互提示（凭据助手不受影响）
    GIT_OPTIONAL_LOCKS: "0",
    GIT_TERMINAL_PROMPT: "0",
  };
}

/** 失败不抛错的探测型调用（探测 repo、upstream、暂存区等） */
export async function tryGit(dir: string, args: string[], timeoutMs = DEFAULT_TIMEOUT_MS): Promise<GitRunResult> {
  try {
    const { stdout, stderr } = await execFileAsync("git", args, {
      cwd: dir,
      timeout: timeoutMs,
      maxBuffer: MAX_BUFFER,
      windowsHide: true,
      shell: false,
      env: gitEnv(),
    });
    return { ok: true, stdout, stderr, exitCode: 0 };
  } catch (err: any) {
    return {
      ok: false,
      stdout: typeof err?.stdout === "string" ? err.stdout : "",
      stderr: typeof err?.stderr === "string" ? err.stderr : err?.message || String(err),
      exitCode: typeof err?.code === "number" ? err.code : null,
    };
  }
}

/** 面向用户的操作型调用：失败抛 GitError（stderr 一并带给前端） */
export async function runGit(dir: string, args: string[], timeoutMs = DEFAULT_TIMEOUT_MS): Promise<string> {
  const result = await tryGit(dir, args, timeoutMs);
  if (!result.ok) {
    throw new GitError(
      `git ${args[0]} failed${result.exitCode != null ? ` (exit ${result.exitCode})` : ""}`,
      result.stderr.trim(),
      result.exitCode,
    );
  }
  return result.stdout;
}

// ────────────────────────── 解析器（纯函数） ──────────────────────────

export interface NumstatEntry {
  additions: number;
  deletions: number;
  path: string;
  binary: boolean;
}

/** `git diff --numstat -z --no-renames` 输出：`<add>\t<del>\t<path>\0` 序列 */
export function parseNumstatZ(output: string): NumstatEntry[] {
  const out: NumstatEntry[] = [];
  for (const field of output.split("\0")) {
    if (!field) continue;
    const tab1 = field.indexOf("\t");
    if (tab1 < 0) continue;
    const tab2 = field.indexOf("\t", tab1 + 1);
    if (tab2 < 0) continue;
    const addRaw = field.slice(0, tab1);
    const delRaw = field.slice(tab1 + 1, tab2);
    const filePath = field.slice(tab2 + 1);
    if (!filePath) continue;
    const binary = addRaw === "-" || delRaw === "-";
    out.push({
      additions: binary ? 0 : Number.parseInt(addRaw, 10) || 0,
      deletions: binary ? 0 : Number.parseInt(delRaw, 10) || 0,
      path: filePath,
      binary,
    });
  }
  return out;
}

export interface BranchEntry {
  name: string;
  current: boolean;
  /** 分支被其他工作树检出（本工作树不能直接 checkout） */
  checkedOutElsewhere: boolean;
}

export interface BranchParseResult {
  branches: BranchEntry[];
  detached: boolean;
}

/**
 * `git for-each-ref --format=%(HEAD)%00%(refname:short) refs/heads` 输出：
 * 每行 `*\0current`（当前分支带 * 标记）或 `\0normal`。
 * detached HEAD 不产生任何带 * 的行，由调用方结合 branch --show-current 判断。
 */
export function parseForEachBranchRef(output: string): BranchEntry[] {
  const branches: BranchEntry[] = [];
  for (const line of output.split("\n")) {
    if (!line) continue;
    const nul = line.indexOf("\0");
    if (nul < 0) continue;
    const marker = line.slice(0, nul);
    const name = line.slice(nul + 1).trim();
    if (!name) continue;
    branches.push({ name, current: marker.includes("*"), checkedOutElsewhere: false });
  }
  return branches;
}

/**
 * 本地分支全集 + 当前标记 + 「他树检出」标记（后者来自 worktree 列表：
 * 分支出现在非本工作树的条目里时，本树无法 checkout）。
 */
export async function listBranches(dir: string): Promise<BranchParseResult & { current: string | null }> {
  const refOut = await runGit(dir, ["for-each-ref", "--format=%(HEAD)%00%(refname:short)", "refs/heads"]);
  const branches = parseForEachBranchRef(refOut);
  const current = branches.find(b => b.current)?.name ?? null;

  const detachedProbe = await tryGit(dir, ["branch", "--show-current"]);
  const detached = detachedProbe.ok && !detachedProbe.stdout.trim();

  const [wtOut, topOut] = await Promise.all([
    tryGit(dir, ["worktree", "list", "--porcelain"]),
    tryGit(dir, ["rev-parse", "--show-toplevel"]),
  ]);
  if (wtOut.ok && topOut.ok) {
    const selfReal = realPath(topOut.stdout.trim()) ?? topOut.stdout.trim();
    const elsewhere = new Set(
      parseWorktreePorcelain(wtOut.stdout)
        .filter(e => e.branch && (realPath(e.path) ?? e.path) !== selfReal)
        .map(e => e.branch!),
    );
    for (const branch of branches) {
      if (elsewhere.has(branch.name)) branch.checkedOutElsewhere = true;
    }
  }

  return { branches, detached, current };
}

export interface WorktreeEntry {
  path: string;
  head: string | null;
  branch: string | null;
  detached: boolean;
  bare: boolean;
}

/** `git worktree list --porcelain` 输出：空行分隔的块，首块为主工作树 */
export function parseWorktreePorcelain(output: string): WorktreeEntry[] {
  const entries: WorktreeEntry[] = [];
  let cur: WorktreeEntry | null = null;
  for (const line of output.split("\n")) {
    if (!line.trim()) {
      cur = null;
      continue;
    }
    const space = line.indexOf(" ");
    const key = space < 0 ? line : line.slice(0, space);
    const value = space < 0 ? "" : line.slice(space + 1);
    if (key === "worktree") {
      cur = { path: value, head: null, branch: null, detached: false, bare: false };
      entries.push(cur);
      continue;
    }
    if (!cur) continue;
    if (key === "branch") cur.branch = value.replace(/^refs\/heads\//, "");
    else if (key === "HEAD") cur.head = value;
    else if (key === "detached") cur.detached = true;
    else if (key === "bare") cur.bare = true;
  }
  return entries;
}

// ────────────────────────── 高层查询 ──────────────────────────

export type GitFileState = "modified" | "added" | "deleted" | "untracked" | "binary";

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

export interface GitStatusSummary {
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

function emptyTotals(): GitChangeTotals {
  return { additions: 0, deletions: 0 };
}

function addTotals(target: GitChangeTotals, additions: number, deletions: number): void {
  target.additions += additions;
  target.deletions += deletions;
}

function realPath(p: string): string | null {
  try {
    return fs.realpathSync(p);
  } catch {
    return null;
  }
}

/** 未跟踪文件按行计新增（对齐 git numstat：结尾换行不额外计行）；二进制/读不到记 0，>512KB 截断低估 */
function countUntrackedLines(absPath: string): number {
  try {
    const stat = fs.statSync(absPath);
    if (!stat.isFile()) return 0;
    const fd = fs.openSync(absPath, "r");
    try {
      const length = Math.min(stat.size, UNTRACKED_READ_CAP);
      const buf = Buffer.alloc(length);
      const read = length > 0 ? fs.readSync(fd, buf, 0, length, 0) : 0;
      const slice = buf.subarray(0, read);
      if (slice.length === 0) return 0;
      if (slice.includes(0)) return 0; // 二进制
      const lines = slice.toString("utf-8").split("\n");
      return lines[lines.length - 1] === "" ? lines.length - 1 : lines.length;
    } finally {
      fs.closeSync(fd);
    }
  } catch {
    return 0;
  }
}

function numstatArgs(extra: string[]): string[] {
  return ["-c", "core.quotepath=false", "diff", "--numstat", "-z", "--no-renames", ...extra];
}

async function cachedNumstat(dir: string): Promise<NumstatEntry[]> {
  const direct = await tryGit(dir, numstatArgs(["--cached"]));
  if (direct.ok) return parseNumstatZ(direct.stdout);
  // 零提交仓库：`--cached` 默认对照 HEAD 会报 ambiguous，改对照空树
  const vsEmptyTree = await tryGit(dir, numstatArgs(["--cached", EMPTY_TREE]));
  return vsEmptyTree.ok ? parseNumstatZ(vsEmptyTree.stdout) : [];
}

export async function isGitWorkTree(dir: string): Promise<boolean> {
  const probe = await tryGit(dir, ["rev-parse", "--is-inside-work-tree"]);
  return probe.ok && probe.stdout.trim() === "true";
}

/**
 * 汇总一个目录的 git 环境状态：分支、增删行、文件级明细、可提交/可推送。
 * 非 git 目录返回 isRepo:false 的占位（不抛错，前端按降级展示）。
 */
export async function collectGitStatus(dir: string): Promise<GitStatusSummary> {
  const empty: GitStatusSummary = {
    isRepo: false,
    currentBranch: null,
    detached: false,
    total: emptyTotals(),
    stagedTotal: emptyTotals(),
    unstagedTotal: emptyTotals(),
    files: [],
    hasUpstream: false,
    hasRemote: false,
    ahead: 0,
    behind: 0,
    commitable: false,
    pushable: false,
  };
  if (!(await isGitWorkTree(dir))) return empty;

  const [showCurrent, stagedNumstat, unstagedNumstat, untrackedOut, remoteOut] = await Promise.all([
    tryGit(dir, ["branch", "--show-current"]),
    cachedNumstat(dir),
    tryGit(dir, numstatArgs([])).then(r => parseNumstatZ(r.stdout)),
    tryGit(dir, ["-c", "core.quotepath=false", "ls-files", "--others", "--exclude-standard", "-z"]),
    tryGit(dir, ["remote"]),
  ]);

  let currentBranch: string | null = showCurrent.ok ? showCurrent.stdout.trim() || null : null;
  const detached = currentBranch == null;
  if (detached) {
    const short = await tryGit(dir, ["rev-parse", "--short", "HEAD"]);
    currentBranch = short.ok ? short.stdout.trim() : null;
  }

  const untrackedPaths = untrackedOut.ok
    ? untrackedOut.stdout.split("\0").filter(Boolean)
    : [];
  const stagedByPath = new Map(stagedNumstat.map(e => [e.path, e]));
  const unstagedByPath = new Map(unstagedNumstat.map(e => [e.path, e]));

  const files: GitFileChange[] = [];
  const total = emptyTotals();
  const stagedTotal = emptyTotals();
  const unstagedTotal = emptyTotals();

  const emit = (entry: Omit<GitFileChange, "additions" | "deletions" | "state" | "staged"> & Partial<Pick<GitFileChange, "additions" | "deletions" | "state" | "staged">>) => {
    const file: GitFileChange = {
      path: entry.path,
      additions: entry.additions ?? 0,
      deletions: entry.deletions ?? 0,
      state: entry.state ?? "modified",
      staged: entry.staged ?? false,
    };
    files.push(file);
  };

  // 已暂存（index vs HEAD）
  for (const entry of stagedNumstat) {
    const unstagedEntry = unstagedByPath.get(entry.path);
    const binary = entry.binary || (unstagedEntry?.binary ?? false);
    emit({
      path: entry.path,
      additions: entry.additions + (unstagedEntry?.additions ?? 0),
      deletions: entry.deletions + (unstagedEntry?.deletions ?? 0),
      state: binary ? "binary" : stateFromNumstat(entry),
      staged: true,
    });
    addTotals(stagedTotal, entry.additions, entry.deletions);
    if (unstagedEntry) addTotals(unstagedTotal, unstagedEntry.additions, unstagedEntry.deletions);
  }
  // 仅未暂存（worktree vs index，未出现在已暂存集合）
  for (const entry of unstagedNumstat) {
    if (stagedByPath.has(entry.path)) continue;
    emit({
      path: entry.path,
      additions: entry.additions,
      deletions: entry.deletions,
      state: entry.binary ? "binary" : stateFromNumstat(entry),
      staged: false,
    });
    addTotals(unstagedTotal, entry.additions, entry.deletions);
  }
  // 未跟踪
  for (const relPath of untrackedPaths) {
    const additions = countUntrackedLines(path.join(dir, ...relPath.split("/")));
    emit({ path: relPath, additions, deletions: 0, state: "untracked", staged: false });
    addTotals(unstagedTotal, additions, 0);
  }

  // 排序：路径字母序，弹窗列表稳定
  files.sort((a, b) => a.path.localeCompare(b.path));
  addTotals(total, stagedTotal.additions, stagedTotal.deletions);
  addTotals(total, unstagedTotal.additions, unstagedTotal.deletions);

  // upstream / ahead / behind / remote
  const upstream = await tryGit(dir, ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"]);
  const hasUpstream = upstream.ok && Boolean(upstream.stdout.trim());
  let ahead = 0;
  let behind = 0;
  if (hasUpstream) {
    const aheadRes = await tryGit(dir, ["rev-list", "--count", "@{upstream}..HEAD"]);
    const behindRes = await tryGit(dir, ["rev-list", "--count", "HEAD..@{upstream}"]);
    ahead = aheadRes.ok ? Number.parseInt(aheadRes.stdout.trim(), 10) || 0 : 0;
    behind = behindRes.ok ? Number.parseInt(behindRes.stdout.trim(), 10) || 0 : 0;
  }
  const hasRemote = remoteOut.ok && remoteOut.stdout.trim().length > 0;

  return {
    isRepo: true,
    currentBranch,
    detached,
    total,
    stagedTotal,
    unstagedTotal,
    files,
    hasUpstream,
    hasRemote,
    ahead,
    behind,
    commitable: files.length > 0,
    // 无 upstream 但有远程时允许推送（push -u 建立跟踪）；有 upstream 时需领先
    pushable: hasRemote && (!hasUpstream || ahead > 0),
  };
}

function stateFromNumstat(entry: NumstatEntry): GitFileState {
  if (entry.additions > 0 && entry.deletions === 0) return "added";
  if (entry.deletions > 0 && entry.additions === 0) return "deleted";
  return "modified";
}

export interface GitWorktreeInfo {
  isRepo: boolean;
  isMain: boolean;
  /** linked worktree 的展示名：优先检出分支名，退回目录名 */
  name: string | null;
  branch: string | null;
  path: string | null;
  mainPath: string | null;
}

export async function worktreeInfo(dir: string): Promise<GitWorktreeInfo> {
  if (!(await isGitWorkTree(dir))) {
    return { isRepo: false, isMain: true, name: null, branch: null, path: null, mainPath: null };
  }
  const top = await tryGit(dir, ["rev-parse", "--show-toplevel"]);
  const list = await tryGit(dir, ["worktree", "list", "--porcelain"]);
  const selfPath = top.ok ? top.stdout.trim() : dir;
  if (!list.ok) {
    return { isRepo: true, isMain: true, name: null, branch: null, path: selfPath, mainPath: selfPath };
  }
  const entries = parseWorktreePorcelain(list.stdout);
  const main = entries[0] ?? null;
  const resolvedSelf = realPath(selfPath) || selfPath;
  const self = entries.find(e => (realPath(e.path) || e.path) === resolvedSelf) ?? null;
  const isMain = main != null && self != null
    && (realPath(main.path) || main.path) === (realPath(self.path) || self.path);
  return {
    isRepo: true,
    isMain,
    name: isMain ? null : (self?.branch ?? path.basename(selfPath)),
    branch: self?.branch ?? null,
    path: selfPath,
    mainPath: main?.path ?? null,
  };
}

// ────────────────────────── 操作 ──────────────────────────

/** 校验分支名：拒绝选项注入（前导 -）与空白；存在性由 rev-parse 验证 */
export function isValidBranchName(name: string): boolean {
  return typeof name === "string"
    && name.length > 0
    && !name.startsWith("-")
    && !/\s/.test(name)
    && !name.includes("..")
    && !name.startsWith("/");
}

export async function checkoutBranch(dir: string, branch: string): Promise<void> {
  if (!isValidBranchName(branch)) throw new GitError("invalid branch name");
  const verify = await tryGit(dir, ["rev-parse", "--verify", "--quiet", `refs/heads/${branch}`]);
  if (!verify.ok) throw new GitError("branch not found", "", verify.exitCode);
  await runGit(dir, ["checkout", branch]);
}

export interface GitCommitResult {
  ok: boolean;
  code?: "nothing_staged" | "nothing_to_commit";
  message?: string;
  head?: string;
}

export async function commitChanges(dir: string, message: string, includeUnstaged: boolean): Promise<GitCommitResult> {
  const trimmed = message.trim();
  if (!trimmed) return { ok: false, code: "nothing_to_commit", message: "empty commit message" };

  if (includeUnstaged) {
    await runGit(dir, ["add", "-A"]);
  }

  // 暂存区检查：--quiet 退出码 1 = 有暂存差异；0 = 干净（零提交仓库报 128，
  // 视为有暂存内容，交给 commit 本身成败说话）
  const quiet = await tryGit(dir, ["diff", "--cached", "--quiet"]);
  if (quiet.ok) {
    // 勾选了「包含未暂存的更改」且树已干净 → 没什么可提交；否则提示暂存区为空
    return { ok: false, code: includeUnstaged ? "nothing_to_commit" : "nothing_staged" };
  }

  const result = await tryGit(dir, ["commit", "-m", trimmed]);
  if (!result.ok) {
    if (/nothing to commit/i.test(result.stderr)) return { ok: false, code: "nothing_to_commit" };
    throw new GitError("commit failed", result.stderr, result.exitCode);
  }
  const head = await tryGit(dir, ["rev-parse", "HEAD"]);
  return { ok: true, head: head.ok ? head.stdout.trim() : undefined };
}

export interface GitPushResult {
  ok: boolean;
  code?: "no_remote" | "nothing_to_push" | "push_failed";
  message?: string;
}

export async function pushChanges(dir: string): Promise<GitPushResult> {
  const remoteOut = await tryGit(dir, ["remote"]);
  const remote = remoteOut.ok ? remoteOut.stdout.split("\n").map(s => s.trim()).filter(Boolean)[0] : null;
  if (!remote) return { ok: false, code: "no_remote" };

  const upstream = await tryGit(dir, ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"]);
  const hasUpstream = upstream.ok && Boolean(upstream.stdout.trim());
  if (hasUpstream) {
    const aheadRes = await tryGit(dir, ["rev-list", "--count", "@{upstream}..HEAD"]);
    const ahead = aheadRes.ok ? Number.parseInt(aheadRes.stdout.trim(), 10) || 0 : 0;
    if (ahead === 0) return { ok: false, code: "nothing_to_push" };
    const result = await tryGit(dir, ["push"], PUSH_TIMEOUT_MS);
    if (!result.ok) return { ok: false, code: "push_failed", message: firstStderrLines(result.stderr) };
    return { ok: true };
  }

  // 无 upstream：push -u 建立跟踪
  const result = await tryGit(dir, ["push", "-u", remote, "HEAD"], PUSH_TIMEOUT_MS);
  if (!result.ok) return { ok: false, code: "push_failed", message: firstStderrLines(result.stderr) };
  return { ok: true };
}

function firstStderrLines(stderr: string, maxLines = 4): string {
  return stderr.split("\n").map(s => s.trim()).filter(Boolean).slice(0, maxLines).join("\n");
}

// ────────────────────────── 提交历史 ──────────────────────────

export type GitRefKind = "head" | "branch" | "remote" | "tag";

export interface GitCommitRef {
  kind: GitRefKind;
  name: string;
}

export interface GitCommit {
  hash: string;
  shortHash: string;
  subject: string;
  /** 完整提交信息（标题 + 正文，多行） */
  message: string;
  authorName: string;
  /** 提交时间戳（秒） */
  committedAt: number;
  refs: GitCommitRef[];
  /** 父提交哈希（多父=合并提交），限流截断处为空数组 */
  parents: string[];
}

/**
 * `git log --pretty=format:%H%x00%h%x00%s%x00%B%x00%an%x00%at%x00%D%x00%P%x1e`
 * 输出解析：记录以 \x1e 分隔、字段以 \x00 分隔。
 * refs 解析：%D 形如 `HEAD -> main, origin/main, tag: v1.0`，空串=无装饰。
 */
export function parseLogRecords(output: string): GitCommit[] {
  const commits: GitCommit[] = [];
  for (const record of output.split("\x1e")) {
    if (!record.trim()) continue;
    const fields = record.replace(/^\n/, "").split("\x00");
    if (fields.length < 8) continue;
    const [hash, shortHash, subject, messageRaw, authorName, committedAtRaw, refsRaw, parentsRaw] = fields;
    if (!hash || !shortHash) continue;
    const refs: GitCommitRef[] = [];
    for (const entry of (refsRaw || "").split(",")) {
      const name = entry.trim();
      if (!name) continue;
      const headMatch = /^HEAD -> (.+)$/.exec(name);
      if (headMatch) {
        refs.push({ kind: "head", name: headMatch[1] });
        continue;
      }
      if (name === "HEAD") {
        refs.push({ kind: "head", name: "HEAD" });
        continue;
      }
      const tagMatch = /^tag: (.+)$/.exec(name);
      if (tagMatch) {
        refs.push({ kind: "tag", name: tagMatch[1] });
        continue;
      }
      refs.push({ kind: name.includes("/") ? "remote" : "branch", name });
    }
    commits.push({
      hash,
      shortHash,
      subject,
      message: (messageRaw || "").replace(/\n+$/, "").trim(),
      authorName,
      committedAt: Number.parseInt(committedAtRaw, 10) || 0,
      refs,
      parents: parentsRaw ? parentsRaw.split(" ").filter(Boolean) : [],
    });
  }
  return commits;
}

export async function listCommits(dir: string, limit = 300): Promise<GitCommit[]> {
  const safeLimit = Math.min(Math.max(Math.floor(limit) || 1, 1), 1000);
  const fmt = "%H%x00%h%x00%s%x00%B%x00%an%x00%at%x00%D%x00%P%x1e";
  const res = await tryGit(dir, ["log", "--date-order", `--max-count=${safeLimit}`, `--pretty=format:${fmt}`]);
  if (!res.ok) return [];
  return parseLogRecords(res.stdout);
}

// ────────────────────────── 单文件 diff ──────────────────────────

export interface GitFileDiff {
  path: string;
  patch: string;
  binary: boolean;
}

/** 校验 repo 相对路径：拒绝绝对路径、`..` 穿越、反斜杠与前导 `-`（防 option 注入） */
export function isSafeRelPath(value: string): boolean {
  if (typeof value !== "string" || !value) return false;
  if (value.startsWith("-") || value.includes("\\") || path.isAbsolute(value)) return false;
  const segments = value.split("/");
  return !segments.some(seg => seg === "" || seg === "." || seg === "..");
}

/** 截断 diff 正文（网络与前端渲染安全上限） */
const PATCH_CHAR_CAP = 256 * 1024;

export async function fileDiff(dir: string, relPath: string): Promise<GitFileDiff> {
  if (!isSafeRelPath(relPath)) throw new GitError("invalid file path");

  // 未跟踪文件：git diff 不含它，合成全新增 patch
  const untracked = await tryGit(dir, ["ls-files", "--others", "--exclude-standard", "-z", "--", relPath]);
  const isUntracked = untracked.ok && untracked.stdout.split("\0").filter(Boolean).includes(relPath);

  if (isUntracked) {
    const abs = path.join(dir, ...relPath.split("/"));
    const patch = buildUntrackedPatch(dir, relPath, abs);
    if (patch == null) return { path: relPath, patch: "", binary: true };
    return { path: relPath, patch: capPatch(patch), binary: false };
  }

  const numstat = await tryGit(dir, numstatArgs(["HEAD", "--", relPath]));
  const entry = numstat.ok ? parseNumstatZ(numstat.stdout)[0] : undefined;

  let diffRes = await tryGit(dir, ["-c", "core.quotepath=false", "diff", "HEAD", "--", relPath]);
  if (!diffRes.ok) {
    // 零提交仓库：对照空树
    diffRes = await tryGit(dir, ["-c", "core.quotepath=false", "diff", EMPTY_TREE, "--", relPath]);
  }
  const patch = diffRes.ok ? diffRes.stdout : "";
  const binary = (entry?.binary ?? false) || /^Binary files .* differ$/m.test(patch);
  return { path: relPath, patch: capPatch(patch), binary };
}

function capPatch(patch: string): string {
  if (patch.length <= PATCH_CHAR_CAP) return patch;
  return `${patch.slice(0, PATCH_CHAR_CAP)}\n…`;
}

/** 未跟踪文件合成 unified diff：整文件作为新增行 */
function buildUntrackedPatch(dir: string, relPath: string, absPath: string): string | null {
  try {
    const stat = fs.statSync(absPath);
    if (!stat.isFile()) return null;
    const fd = fs.openSync(absPath, "r");
    try {
      const length = Math.min(stat.size, PATCH_CHAR_CAP);
      const buf = Buffer.alloc(length);
      const read = length > 0 ? fs.readSync(fd, buf, 0, length, 0) : 0;
      const slice = buf.subarray(0, read);
      if (slice.length > 0 && slice.includes(0)) return null; // 二进制
      const content = slice.toString("utf-8");
      const lines = content.split("\n");
      // 结尾换行产生的空尾行不算内容行
      if (lines.length > 0 && lines[lines.length - 1] === "") lines.pop();
      const body = lines.map(line => `+${line}`).join("\n");
      return [
        `diff --git a/${relPath} b/${relPath}`,
        "new file mode 100644",
        "--- /dev/null",
        `+++ b/${relPath}`,
        `@@ -0,0 +1,${lines.length} @@`,
        body,
      ].join("\n");
    } finally {
      fs.closeSync(fd);
    }
  } catch {
    return null;
  }
}
