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
/** worktree add 要检出整棵树，给与 push 同级的宽限 */
const WORKTREE_TIMEOUT_MS = 120_000;
const MAX_BUFFER = 16 * 1024 * 1024;
/** 空树对象哈希：git diff --cached 在零提交仓库里的对照基线 */
const EMPTY_TREE = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
/** 未跟踪文件行数计数的读取上限（更大文件按截断内容计行，轻微低估可接受） */
const UNTRACKED_READ_CAP = 512 * 1024;
/** 未跟踪文件行数计数的双预算（文件数 / 耗时，先到为准）：超出部分仍列出路径但行数记 0。
 *  上万未跟踪产物的全量计数曾同步阻塞 3s+（环境信息卡首屏跟着变慢），预算后 ~0.3s。 */
const UNTRACKED_COUNT_FILE_BUDGET = 2000;
const UNTRACKED_COUNT_TIME_BUDGET_MS = 1_000;

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

export interface NameStatusEntry {
  /** git 状态字母：A 新增 / M 修改 / D 删除（--no-renames 下不会出现 R/C） */
  status: "A" | "M" | "D";
  path: string;
}

/**
 * `git diff --name-status -z --no-renames` 输出：`<status>\0<path>\0` 交替序列。
 * 真实状态字母：纯删行的内容修改不会被误判成 D（那是 numstat 猜不出来的）。
 */
export function parseNameStatusZ(output: string): NameStatusEntry[] {
  const fields = output.split("\0");
  const out: NameStatusEntry[] = [];
  for (let i = 0; i + 1 < fields.length; i += 2) {
    const status = fields[i];
    const filePath = fields[i + 1];
    if (!filePath) continue;
    if (status === "A" || status === "D" || status === "M") {
      out.push({ status, path: filePath });
    }
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

function nameStatusArgs(extra: string[]): string[] {
  return ["-c", "core.quotepath=false", "diff", "--name-status", "-z", "--no-renames", ...extra];
}

async function cachedNameStatus(dir: string): Promise<NameStatusEntry[]> {
  const direct = await tryGit(dir, nameStatusArgs(["--cached"]));
  if (direct.ok) return parseNameStatusZ(direct.stdout);
  const vsEmptyTree = await tryGit(dir, nameStatusArgs(["--cached", EMPTY_TREE]));
  return vsEmptyTree.ok ? parseNameStatusZ(vsEmptyTree.stdout) : [];
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

  const [showCurrent, stagedNumstat, unstagedNumstat, stagedNameStatus, unstagedNameStatus, untrackedOut, remoteOut] = await Promise.all([
    tryGit(dir, ["branch", "--show-current"]),
    cachedNumstat(dir),
    tryGit(dir, numstatArgs([])).then(r => parseNumstatZ(r.stdout)),
    cachedNameStatus(dir),
    tryGit(dir, nameStatusArgs([])).then(r => parseNameStatusZ(r.stdout)),
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
  // 真实状态字母（A/M/D）：纯删行的内容修改不该被误判成「文件已删除」
  const stagedLetter = new Map(stagedNameStatus.map(e => [e.path, e.status]));
  const unstagedLetter = new Map(unstagedNameStatus.map(e => [e.path, e.status]));

  const stateFor = (entry: NumstatEntry, letters: Map<string, "A" | "M" | "D">): GitFileState => {
    if (entry.binary) return "binary";
    const letter = letters.get(entry.path);
    if (letter === "A") return "added";
    if (letter === "D") return "deleted";
    if (letter === "M") return "modified";
    return stateFromNumstat(entry);
  };

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
      state: stateFor(entry, stagedLetter),
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
      state: entry.binary ? "binary" : stateFor(entry, unstagedLetter),
      staged: false,
    });
    addTotals(unstagedTotal, entry.additions, entry.deletions);
  }
  // 未跟踪：逐文件读内容数行（语义对齐 git numstat）。文件数 + 耗时双预算：
  // agent 工作区常见上万未跟踪产物文件，全量计数曾同步阻塞 3s+，环境信息卡
  // 与图谱首屏跟着变慢。超预算的文件仍列出（untracked 状态）但行数记 0——
  // 显式降级（本常量块即标注），总量在超大工作区为下界。
  const untrackedDeadline = Date.now() + UNTRACKED_COUNT_TIME_BUDGET_MS;
  for (const [index, relPath] of untrackedPaths.entries()) {
    let additions = 0;
    if (index < UNTRACKED_COUNT_FILE_BUDGET && Date.now() < untrackedDeadline) {
      additions = countUntrackedLines(path.join(dir, ...relPath.split("/")));
    }
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

/**
 * 提交（修改）：把当前暂存内容并入上一次提交（git commit --amend）。
 * message 为空 = --no-edit 保留原提交信息；零提交仓库无目标可改，结构化拒绝。
 * 注意：已推送过的提交被改写后本地/远程会分叉，与普通 git 语义一致，由使用者把握。
 */
export async function amendCommit(dir: string, message: string | null): Promise<GitCommitResult> {
  const verify = await tryGit(dir, ["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]);
  if (!verify.ok) return { ok: false, code: "nothing_to_commit", message: "no commit to amend" };
  const trimmed = typeof message === "string" ? message.trim() : "";
  const args = trimmed ? ["commit", "--amend", "-m", trimmed] : ["commit", "--amend", "--no-edit"];
  const result = await tryGit(dir, args);
  if (!result.ok) {
    if (/nothing to commit/i.test(result.stderr)) return { ok: false, code: "nothing_to_commit" };
    throw new GitError("amend failed", result.stderr, result.exitCode);
  }
  const head = await tryGit(dir, ["rev-parse", "HEAD"]);
  return { ok: true, head: head.ok ? head.stdout.trim() : undefined };
}

export interface GitStageResult {
  ok: boolean;
  code?: "invalid_path" | "stage_failed";
  message?: string;
  paths?: string[];
}

/** 暂存（git add）：paths 缺省 = 全部改动（含未跟踪）进暂存区；给了 paths 只加这些 */
export async function stagePaths(dir: string, paths?: string[] | null): Promise<GitStageResult> {
  const { paths: wanted, invalid } = normalizePathList(paths);
  if (invalid) return { ok: false, code: "invalid_path" };
  const result = wanted
    ? await tryGit(dir, ["add", "--", ...wanted.map(topLiteralPathspec)])
    : await tryGit(dir, ["add", "-A"]);
  if (!result.ok) {
    return { ok: false, code: "stage_failed", message: firstStderrLines(result.stderr), paths: wanted ?? [] };
  }
  return { ok: true, paths: wanted ?? [] };
}

export interface GitUnstageResult {
  ok: boolean;
  code?: "invalid_path" | "unstage_failed";
  message?: string;
  paths?: string[];
}

/**
 * 取消暂存：index 退回 HEAD（paths 缺省 = 全部退回）。
 * 零提交仓库没有 HEAD 可退，index 里的条目全是新增，用 rm --cached 清出。
 */
export async function unstagePaths(dir: string, paths?: string[] | null): Promise<GitUnstageResult> {
  const { paths: wanted, invalid } = normalizePathList(paths);
  if (invalid) return { ok: false, code: "invalid_path" };
  const spec = wanted ? wanted.map(topLiteralPathspec) : [":/"];
  const head = await tryGit(dir, ["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]);
  const result = head.ok
    ? await tryGit(dir, ["restore", "--staged", "--", ...spec])
    : await tryGit(dir, ["rm", "-r", "--cached", "--ignore-unmatch", "--quiet", "--", ...spec]);
  if (!result.ok) {
    return { ok: false, code: "unstage_failed", message: firstStderrLines(result.stderr), paths: wanted ?? [] };
  }
  return { ok: true, paths: wanted ?? [] };
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

export interface GitPullResult {
  ok: boolean;
  code?: "no_remote" | "no_upstream" | "fetch_failed" | "already_up_to_date" | "diverged" | "local_changes" | "pull_failed";
  message?: string;
  /** 本次实际并入本地的提交数 */
  pulled?: number;
}

export interface GitFetchResult {
  ok: boolean;
  code?: "no_remote" | "fetch_failed";
  message?: string;
}

/**
 * 只拉取不合并（git fetch）：把远程的真实状态（领先/落后计数）带回来，
 * 供「刷新」按钮和同步按钮在行动前校准；不产生合并、不改工作区。
 */
export async function fetchRemote(dir: string): Promise<GitFetchResult> {
  const remoteOut = await tryGit(dir, ["remote"]);
  const remote = remoteOut.ok ? remoteOut.stdout.split("\n").map(s => s.trim()).filter(Boolean)[0] : null;
  if (!remote) return { ok: false, code: "no_remote" };
  const result = await tryGit(dir, ["fetch", remote], PUSH_TIMEOUT_MS);
  if (!result.ok) return { ok: false, code: "fetch_failed", message: firstStderrLines(result.stderr) };
  return { ok: true };
}

/**
 * 拉取远程更新：先 fetch 再本地判断，仅在可快进时合并（--ff-only）。
 * 分桶语义（面向环境信息卡 UI，结构化返回而非抛错）：
 *   - no_remote / no_upstream：没有可拉的对象
 *   - fetch_failed：网络或凭据问题（GIT_TERMINAL_PROMPT=0 快速失败）
 *   - already_up_to_date：fetch 后没有新提交
 *   - diverged：本地与远程各有新提交。不自动造 merge 提交，留给用户在
 *     对话里让助手处理，避免给非技术用户留下半合并状态
 *   - local_changes：未提交改动挡住了快进（按文件重叠结构性预判，
 *     不靠 stderr 文本匹配：git 报错随 locale 本地化，中文环境没有
 *     "would be overwritten" 字样）
 */
export async function pullChanges(dir: string): Promise<GitPullResult> {
  const remoteOut = await tryGit(dir, ["remote"]);
  const remote = remoteOut.ok ? remoteOut.stdout.split("\n").map(s => s.trim()).filter(Boolean)[0] : null;
  if (!remote) return { ok: false, code: "no_remote" };

  const upstream = await tryGit(dir, ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"]);
  if (!upstream.ok || !upstream.stdout.trim()) return { ok: false, code: "no_upstream" };

  // fetch 与合并分开：fetch 失败是网络/凭据问题，单独成桶便于 UI 提示
  const fetch = await tryGit(dir, ["fetch", remote], PUSH_TIMEOUT_MS);
  if (!fetch.ok) return { ok: false, code: "fetch_failed", message: firstStderrLines(fetch.stderr) };

  const behindRes = await tryGit(dir, ["rev-list", "--count", "HEAD..@{upstream}"]);
  const behind = behindRes.ok ? Number.parseInt(behindRes.stdout.trim(), 10) || 0 : 0;
  if (behind === 0) return { ok: false, code: "already_up_to_date" };

  const aheadRes = await tryGit(dir, ["rev-list", "--count", "@{upstream}..HEAD"]);
  const ahead = aheadRes.ok ? Number.parseInt(aheadRes.stdout.trim(), 10) || 0 : 0;
  if (ahead > 0) return { ok: false, code: "diverged" };

  if (await dirtyFilesClashWithIncoming(dir)) return { ok: false, code: "local_changes" };

  // 快进合并不产生 merge 提交；走到这里仍失败的是意外情况，stderr 原样带给 UI
  const merge = await tryGit(dir, ["merge", "--ff-only", "@{upstream}"]);
  if (!merge.ok) return { ok: false, code: "pull_failed", message: firstStderrLines(merge.stderr) };
  return { ok: true, pulled: behind };
}

/**
 * 本地未提交/未跟踪文件与待并入文件重叠：重叠则快进会被 git 拒绝。
 * quotepath=false 让两侧都输出原始 UTF-8 路径，保证集合可比较。
 */
async function dirtyFilesClashWithIncoming(dir: string): Promise<boolean> {
  const [dirtyRes, untrackedRes, incomingRes] = await Promise.all([
    tryGit(dir, ["-c", "core.quotepath=false", "diff", "--name-only", "HEAD"]),
    tryGit(dir, ["-c", "core.quotepath=false", "ls-files", "--others", "--exclude-standard"]),
    tryGit(dir, ["-c", "core.quotepath=false", "diff", "--name-only", "HEAD..@{upstream}"]),
  ]);
  if (!incomingRes.ok) return false;
  const incoming = incomingRes.stdout.split("\n").map(s => s.trim()).filter(Boolean);
  const local = new Set([
    ...(dirtyRes.ok ? dirtyRes.stdout.split("\n") : []),
    ...(untrackedRes.ok ? untrackedRes.stdout.split("\n") : []),
  ].map(s => s.trim()).filter(Boolean));
  return incoming.some(p => local.has(p));
}

function firstStderrLines(stderr: string, maxLines = 4): string {
  return stderr.split("\n").map(s => s.trim()).filter(Boolean).slice(0, maxLines).join("\n");
}

// ────────────────────────── 分支创建 / 暂存 / 工作树 ──────────────────────────

export type GitCreateBranchCode = "invalid_name" | "invalid_base" | "already_exists" | "branch_failed";

export interface GitCreateBranchResult {
  ok: boolean;
  code?: GitCreateBranchCode;
  branch?: string;
  message?: string;
}

/**
 * 新建分支并检出：等价 `git checkout -b <name> [<base>]`，base 缺省为当前 HEAD。
 * 与 checkoutBranch 同为「操作」语义：结构化返回预期失败，只有 git 之外
 * 的意外才抛错。
 */
export async function createBranch(
  dir: string,
  name: string,
  base?: string | null,
): Promise<GitCreateBranchResult> {
  const branch = typeof name === "string" ? name.trim() : "";
  if (!isValidBranchName(branch)) return { ok: false, code: "invalid_name" };

  const existing = await tryGit(dir, ["rev-parse", "--verify", "--quiet", `refs/heads/${branch}`]);
  if (existing.ok) return { ok: false, code: "already_exists", branch };

  const args = ["checkout", "-b", branch];
  const start = typeof base === "string" ? base.trim() : "";
  if (start) {
    if (!isValidBranchName(start)) return { ok: false, code: "invalid_base", branch };
    const verify = await tryGit(dir, ["rev-parse", "--verify", "--quiet", `${start}^{commit}`]);
    if (!verify.ok) return { ok: false, code: "invalid_base", branch };
    args.push(start);
  }

  const result = await tryGit(dir, args);
  if (!result.ok) {
    return { ok: false, code: "branch_failed", branch, message: firstStderrLines(result.stderr) };
  }
  return { ok: true, branch };
}

export interface GitStashResult {
  ok: boolean;
  code?: "nothing_to_stash" | "stash_failed" | "invalid_path";
  message?: string;
  /** 有 pathspec 时实际收进储藏的路径 */
  paths?: string[];
}

/**
 * 路径来自 `git status` 输出（仓库根相对），但客户端可传任意串，所以：
 *   - `top`    ：按仓库根解释路径，子目录工作台也和在根目录一样
 *   - `literal`：关掉 glob/attr magic，避免 `*` / `:(glob)` 之类放大匹配面
 */
function topLiteralPathspec(rel: string): string {
  return `:(top,literal)${rel}`;
}

/** `null` = 不带 pathspec（整仓）；非法路径与空数组分别返回 invalid / null */
function normalizePathList(raw: unknown): { paths: string[] | null; invalid: boolean } {
  if (!Array.isArray(raw) || raw.length === 0) return { paths: null, invalid: false };
  const paths: string[] = [];
  for (const item of raw) {
    if (typeof item !== "string" || !isSafeRelPath(item)) return { paths: null, invalid: true };
    if (!paths.includes(item)) paths.push(item);
  }
  return { paths, invalid: false };
}

/**
 * 暂存当前修改（含未跟踪文件）：`git stash push --include-untracked [-m <message>] [-- <paths>]`。
 * paths 缺省 = 整个工作树；给了 paths 就只收这些路径（单个文件暂存）。
 * 干净工作树先探测后返回结构化 code，不把 git 的 "No local changes to save" 当成成功。
 */
export async function stashChanges(
  dir: string,
  message?: string | null,
  paths?: string[] | null,
): Promise<GitStashResult> {
  const { paths: wanted, invalid } = normalizePathList(paths);
  if (invalid) return { ok: false, code: "invalid_path" };
  const trimmed = typeof message === "string" ? message.trim() : "";
  const spec = wanted ? wanted.map(topLiteralPathspec) : null;

  const dirty = await tryGit(dir, [
    "status", "--porcelain",
    ...(spec ? ["--", ...spec] : []),
  ]);
  if (dirty.ok && !dirty.stdout.trim()) return { ok: false, code: "nothing_to_stash" };

  const args = ["stash", "push", "--include-untracked"];
  if (trimmed) args.push("-m", trimmed);
  if (spec) args.push("--", ...spec);
  const result = await tryGit(dir, args);
  if (!result.ok) {
    return { ok: false, code: "stash_failed", message: firstStderrLines(result.stderr) };
  }
  if (/No local changes to save/i.test(result.stdout)) return { ok: false, code: "nothing_to_stash" };
  return wanted ? { ok: true, paths: wanted } : { ok: true };
}

export interface GitStashEntry {
  ref: string;
  /** reflog 主题，如 "On main: wip demo" */
  message: string;
  /** 改动涉及的已跟踪路径（仓库根相对） */
  tracked: string[];
  /** 随储藏一起收进去的未跟踪路径 */
  untracked: string[];
}

/** 一条储藏覆盖的改动路径：已跟踪部分（stash show）+ 未跟踪部分（第三个父提交） */
async function stashChangedPaths(dir: string, ref: string): Promise<{ tracked: string[]; untracked: string[] }> {
  const [tracked, untracked] = await Promise.all([
    tryGit(dir, ["stash", "show", "--name-only", "--no-renames", ref]),
    tryGit(dir, ["ls-tree", "-r", "--full-tree", "--name-only", "-z", `${ref}^3`]),
  ]);
  const lines = (out: string, sep: string) => out.split(sep).map(s => s.trim()).filter(Boolean);
  return {
    tracked: tracked.ok ? lines(tracked.stdout, "\n") : [],
    untracked: untracked.ok ? lines(untracked.stdout, "\0") : [],
  };
}

export interface GitStashListResult {
  isRepo: boolean;
  stashes: GitStashEntry[];
}

/** 储藏栈（新→旧）。limit 限制条数：每条要走 git 取路径，别让卡片刷新拖垮 */
export async function listStashes(dir: string, limit = 20): Promise<GitStashListResult> {
  if (!(await isGitWorkTree(dir))) return { isRepo: false, stashes: [] };
  const list = await tryGit(dir, ["stash", "list", "--format=%gd%x00%gs"]);
  if (!list.ok) return { isRepo: true, stashes: [] };

  const entries: GitStashEntry[] = [];
  for (const line of list.stdout.split("\n").map(s => s.trim()).filter(Boolean).slice(0, limit)) {
    const [ref, ...rest] = line.split("\0");
    if (!ref) continue;
    const { tracked, untracked } = await stashChangedPaths(dir, ref);
    entries.push({ ref, message: rest.join("\0"), tracked, untracked });
  }
  return { isRepo: true, stashes: entries };
}

export type GitUnstashCode = "invalid_path" | "no_stash" | "not_in_stash" | "path_dirty"
  | "unstash_conflict" | "unstash_failed";

export interface GitUnstashResult {
  ok: boolean;
  code?: GitUnstashCode;
  message?: string;
  path?: string;
  /** 命中的储藏条目（单个取出时） */
  stash?: string;
}

/**
 * 把单个文件从储藏里取回工作区：从新到旧找第一条含该路径的储藏，用它那份内容
 * 覆盖工作区（只动 worktree，不动暂存区）。
 * 路径当前已有改动时拒绝——整文件覆盖不能静默吃掉用户的现行修改。
 * 只取单文件不会改动储藏条目本身，整条弹出时仍会带出它。
 */
export async function restoreStashedPath(dir: string, relPath: string): Promise<GitUnstashResult> {
  if (!isSafeRelPath(relPath)) return { ok: false, code: "invalid_path" };
  const spec = topLiteralPathspec(relPath);

  const dirty = await tryGit(dir, ["status", "--porcelain", "--", spec]);
  if (dirty.ok && dirty.stdout.trim()) return { ok: false, code: "path_dirty", path: relPath };

  const list = await tryGit(dir, ["stash", "list", "--format=%gd"]);
  const refs = list.ok ? list.stdout.split("\n").map(s => s.trim()).filter(Boolean) : [];
  if (refs.length === 0) return { ok: false, code: "no_stash", path: relPath };

  for (const ref of refs) {
    const { tracked, untracked } = await stashChangedPaths(dir, ref);
    const fromUntracked = !tracked.includes(relPath) && untracked.includes(relPath);
    if (!tracked.includes(relPath) && !fromUntracked) continue;

    const source = fromUntracked ? `${ref}^3` : ref;
    const restored = await tryGit(dir, ["restore", "--source", source, "--worktree", "--", spec]);
    if (!restored.ok) {
      return {
        ok: false, code: "unstash_failed", path: relPath, stash: ref,
        message: firstStderrLines(restored.stderr),
      };
    }
    return { ok: true, path: relPath, stash: ref };
  }
  return { ok: false, code: "not_in_stash", path: relPath };
}

/** 弹出最新一条储藏（全部文件）。冲突时储藏条目保留，原样把 git 的提示带出去 */
export async function popStash(dir: string): Promise<GitUnstashResult> {
  const list = await tryGit(dir, ["stash", "list", "--format=%gd"]);
  if (!list.ok || !list.stdout.trim()) return { ok: false, code: "no_stash" };

  const result = await tryGit(dir, ["stash", "pop"]);
  if (!result.ok) {
    const detail = `${result.stdout}\n${result.stderr}`;
    return {
      ok: false,
      code: /conflict/i.test(detail) ? "unstash_conflict" : "unstash_failed",
      message: firstStderrLines(result.stderr || result.stdout),
    };
  }
  return { ok: true };
}

export type GitDiscardCode = "invalid_path" | "nothing_to_discard" | "discard_failed";

export interface GitDiscardResult {
  ok: boolean;
  code?: GitDiscardCode;
  message?: string;
  paths?: string[];
}

/**
 * 回退未提交修改：把已跟踪文件恢复成 HEAD 内容，暂存区与工作区一起回。
 * paths 缺省 = 整个仓库（:/，子目录工作台也覆盖全仓）。
 * 未跟踪的新文件也参与：回退 = 删除该文件（git clean），UI 侧有确认弹窗挡着；
 * ignored（.gitignore 覆盖的）文件始终不动。
 */
export async function discardPaths(dir: string, paths?: string[] | null): Promise<GitDiscardResult> {
  const { paths: wanted, invalid } = normalizePathList(paths);
  if (invalid) return { ok: false, code: "invalid_path" };
  const spec = wanted ? wanted.map(topLiteralPathspec) : [":/"];

  // 零提交仓库拿空树当基线（与 cachedNumstat 同款兜底）
  const head = await tryGit(dir, ["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]);
  const source = head.ok ? "HEAD" : EMPTY_TREE;

  const diffBase = source === "HEAD" ? ["HEAD"] : [EMPTY_TREE];
  const [unstaged, staged, untracked] = await Promise.all([
    tryGit(dir, ["-c", "core.quotepath=false", "diff", "--name-only", ...diffBase, "--", ...spec]),
    tryGit(dir, ["-c", "core.quotepath=false", "diff", "--cached", "--name-only", ...diffBase, "--", ...spec]),
    tryGit(dir, ["-c", "core.quotepath=false", "ls-files", "--others", "--exclude-standard", "-z",
      ...(wanted ? ["--", ...spec] : [])]),
  ]);
  const hasTracked = Boolean(unstaged.stdout.trim() || staged.stdout.trim());
  const untrackedPaths = untracked.ok ? untracked.stdout.split("\0").filter(Boolean) : [];
  if (!hasTracked && untrackedPaths.length === 0) {
    return { ok: false, code: "nothing_to_discard", paths: wanted ?? [] };
  }

  // 已跟踪：恢复到基线
  if (hasTracked) {
    const result = await tryGit(dir, ["restore", "--source", source, "--staged", "--worktree", "--", ...spec]);
    if (!result.ok) {
      return { ok: false, code: "discard_failed", message: firstStderrLines(result.stderr), paths: wanted ?? [] };
    }
  }
  // 未跟踪：删除文件。整仓回退带 -d 连未跟踪目录一起清（ignored 默认不动）
  if (untrackedPaths.length > 0) {
    const clean = wanted
      ? await tryGit(dir, ["clean", "-f", "--", ...spec])
      : await tryGit(dir, ["clean", "-f", "-d"]);
    if (!clean.ok) {
      return { ok: false, code: "discard_failed", message: firstStderrLines(clean.stderr), paths: wanted ?? [] };
    }
  }
  return { ok: true, paths: wanted ?? [] };
}

export interface GitWorktreeListEntry extends WorktreeEntry {
  /** `git worktree list` 的首块永远是被检出仓库的主工作树 */
  isMain: boolean;
  /** 该条目就是 dir 所在的工作树 */
  current: boolean;
}

export interface GitWorktreeListResult {
  isRepo: boolean;
  worktrees: GitWorktreeListEntry[];
  /** 新建 worktree 的落地根：主工作树的父级 + /worktrees */
  root: string | null;
  mainPath: string | null;
}

/** worktree 落地根目录约定：<主工作树父级>/worktrees */
export function worktreesRootFor(mainWorktreePath: string): string {
  return path.join(path.dirname(mainWorktreePath), "worktrees");
}

export async function listWorktrees(dir: string): Promise<GitWorktreeListResult> {
  if (!(await isGitWorkTree(dir))) {
    return { isRepo: false, worktrees: [], root: null, mainPath: null };
  }
  const top = await tryGit(dir, ["rev-parse", "--show-toplevel"]);
  const selfPath = top.ok ? top.stdout.trim() : dir;
  const list = await tryGit(dir, ["worktree", "list", "--porcelain"]);
  if (!list.ok) return { isRepo: true, worktrees: [], root: null, mainPath: null };

  const entries = parseWorktreePorcelain(list.stdout);
  const resolvedSelf = realPath(selfPath) || selfPath;
  const main = entries[0] ?? null;
  return {
    isRepo: true,
    worktrees: entries.map((entry, index) => ({
      ...entry,
      isMain: index === 0,
      current: (realPath(entry.path) || entry.path) === resolvedSelf,
    })),
    root: main ? worktreesRootFor(main.path) : null,
    mainPath: main?.path ?? null,
  };
}

export type GitWorktreeCreateCode = "invalid_name" | "exists" | "branch_exists" | "invalid_base" | "create_failed";

export interface GitWorktreeCreateResult {
  ok: boolean;
  code?: GitWorktreeCreateCode;
  path?: string;
  branch?: string;
  message?: string;
}

/**
 * worktree 目录名准入：只允许字母数字开头、后随字母数字与 . _ -。
 * 目录名同时用来拼 `wt/<name>` 分支名，所以这里比 isValidBranchName 更严：
 * 任何路径分隔符、前导点、空白都在此拦下。
 */
export function isValidWorktreeName(name: string): boolean {
  return typeof name === "string"
    && /^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(name)
    && name !== "."
    && name !== "..";
}

/**
 * 在 <主工作树父级>/worktrees/<name> 建一个隔离工作树，携带新分支 wt/<name>。
 * 目标目录或 wt/<name> 已存在时结构化拒绝（不覆盖、不复用），保证调用方
 * 拿到的一定是「这个名称新开出来的」工作树。
 */
export async function createWorktree(
  dir: string,
  name: string,
  base?: string | null,
): Promise<GitWorktreeCreateResult> {
  const raw = typeof name === "string" ? name.trim() : "";
  if (!isValidWorktreeName(raw)) return { ok: false, code: "invalid_name" };

  const list = await listWorktrees(dir);
  if (!list.isRepo || !list.root || !list.mainPath) return { ok: false, code: "create_failed" };

  const targetPath = path.join(list.root, raw);
  const branch = `wt/${raw}`;
  if (fs.existsSync(targetPath)) return { ok: false, code: "exists", path: targetPath, branch };

  const branchExists = await tryGit(dir, ["rev-parse", "--verify", "--quiet", `refs/heads/${branch}`]);
  if (branchExists.ok) return { ok: false, code: "branch_exists", path: targetPath, branch };

  const args = ["worktree", "add", "-b", branch, targetPath];
  const start = typeof base === "string" ? base.trim() : "";
  if (start) {
    if (!isValidBranchName(start)) return { ok: false, code: "invalid_base", branch };
    const verify = await tryGit(dir, ["rev-parse", "--verify", "--quiet", `${start}^{commit}`]);
    if (!verify.ok) return { ok: false, code: "invalid_base", branch };
    args.push(start);
  }

  try {
    fs.mkdirSync(list.root, { recursive: true });
  } catch {
    // 建目录失败就交给 git worktree add 报错，保持单一失败来源
  }

  const result = await tryGit(dir, args, WORKTREE_TIMEOUT_MS);
  if (!result.ok) {
    return {
      ok: false,
      code: "create_failed",
      path: targetPath,
      branch,
      message: firstStderrLines(result.stderr),
    };
  }
  return { ok: true, path: targetPath, branch };
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
  /** 相对第一父的新增行数（合并提交=并入分支的增量） */
  additions: number;
  /** 相对第一父的删除行数 */
  deletions: number;
  /** 变更文件数；--numstat 不可用时为 0（UI 隐藏统计行） */
  changedFiles: number;
}

/** 把一个 numstat 统计块累加到提交上（覆盖语义：每条提交恰好对应一个块） */
function applyCommitStats(commit: GitCommit, lines: string[]): void {
  const stats = statsFromNumstatLines(lines);
  commit.additions = stats.additions;
  commit.deletions = stats.deletions;
  commit.changedFiles = stats.changedFiles;
}

function statsFromNumstatLines(lines: string[]): GitCommitStats {
  let additions = 0;
  let deletions = 0;
  let changedFiles = 0;
  for (const line of lines) {
    const tab1 = line.indexOf("\t");
    if (tab1 < 0) continue;
    const tab2 = line.indexOf("\t", tab1 + 1);
    if (tab2 < 0) continue;
    changedFiles++;
    const addRaw = line.slice(0, tab1);
    const delRaw = line.slice(tab1 + 1, tab2);
    if (addRaw !== "-") additions += Number.parseInt(addRaw, 10) || 0;
    if (delRaw !== "-") deletions += Number.parseInt(delRaw, 10) || 0;
  }
  return { additions, deletions, changedFiles };
}

/** log 记录格式：字段以 \x00 分隔、记录以 \x1e 结束（parseLogRecords 的输入） */
const LOG_FORMAT = "%H%x00%h%x00%s%x00%B%x00%an%x00%at%x00%D%x00%P%x1e";

/**
 * `git log --numstat --diff-merges=first-parent --pretty=format:…%x1e` 输出解析：
 * 记录以 \x1e 分隔、字段以 \x00 分隔；每条提交的 numstat 块紧跟在该提交 pretty
 * 段（含末尾 \x1e）之后，即出现在**下一个** \x1e 块的开头；最后一条提交的统计
 * 则是纯 numstat 尾块。因此逐块先扫过块首不含 \x00 的 numstat 行归给上一条
 * 提交，再按 \x00 切字段（%B 正文可含换行但不含 \x00，切分安全）。
 * refs 解析：%D 形如 `HEAD -> main, origin/main, tag: v1.0`，空串=无装饰。
 */
export function parseLogRecords(output: string): GitCommit[] {
  const commits: GitCommit[] = [];
  let last: GitCommit | null = null;
  for (const record of output.split("\x1e")) {
    if (!record.trim()) continue;
    const lines = record.replace(/^\n/, "").split("\n");
    let start = 0;
    const stats: string[] = [];
    while (start < lines.length && !lines[start].includes("\x00")) {
      if (lines[start].trim()) stats.push(lines[start]);
      start++;
    }
    if (start >= lines.length) {
      // 纯 numstat 尾块：属于最后一条已解析的提交
      if (last) applyCommitStats(last, stats);
      continue;
    }
    if (last) applyCommitStats(last, stats);
    const fields = lines.slice(start).join("\n").split("\x00");
    if (fields.length < 8) { last = null; continue; }
    const [hash, shortHash, subject, messageRaw, authorName, committedAtRaw, refsRaw, parentsRaw] = fields;
    if (!hash || !shortHash) { last = null; continue; }
    const refs: GitCommitRef[] = [];
    for (const entry of (refsRaw || "").split(",")) {
      const name = entry.trim();
      if (!name) continue;
      const headMatch = name.match(/^HEAD -> (.+)$/);
      if (headMatch) {
        refs.push({ kind: "head", name: headMatch[1] });
        continue;
      }
      if (name === "HEAD") {
        refs.push({ kind: "head", name: "HEAD" });
        continue;
      }
      const tagMatch = name.match(/^tag: (.+)$/);
      if (tagMatch) {
        refs.push({ kind: "tag", name: tagMatch[1] });
        continue;
      }
      refs.push({ kind: name.includes("/") ? "remote" : "branch", name });
    }
    last = {
      hash,
      shortHash,
      subject,
      message: (messageRaw || "").replace(/\n+$/, "").trim(),
      authorName,
      committedAt: Number.parseInt(committedAtRaw, 10) || 0,
      refs,
      parents: parentsRaw ? parentsRaw.split(" ").filter(Boolean) : [],
      additions: 0,
      deletions: 0,
      changedFiles: 0,
    };
    commits.push(last);
  }
  return commits;
}

export async function listCommits(dir: string, limit = 300): Promise<GitCommit[]> {
  const safeLimit = Math.min(Math.max(Math.floor(limit) || 1, 1), 1000);
  const maxCountArg = "--max-count=" + String(safeLimit);
  // 纯元数据查询（实测 <0.1s）；变更统计走 listCommitStats 两段加载，
  // 避免 --numstat 把整段历史的 diff 拖进首屏（实测本仓库 300 条 ≈16s）。
  const res = await tryGit(dir, ["log", "--date-order", maxCountArg, "--pretty=format:" + LOG_FORMAT]);
  return res.ok ? parseLogRecords(res.stdout) : [];
}

export interface GitCommitStats {
  additions: number;
  deletions: number;
  changedFiles: number;
}

/** 提交统计缓存：同一提交的 diff 永不变化，进程内缓存跨弹窗打开复用 */
const commitStatsCache = new Map<string, GitCommitStats>();
const COMMIT_STATS_CACHE_CAP = 20_000;

function commitStatsCacheKey(dir: string, hash: string): string {
  return dir + "\u0000" + hash;
}

/**
 * 批量取提交变更统计（相对第一父；合并提交=并入分支的增量）。
 * `git log --no-walk=unsorted --numstat <hash…>` 一次进程算一批，复用
 * parseLogRecords 归因；哈希必须严格 40 位十六进制（argv 数组 + 白名单
 * 校验，无 shell 注入面）。失败按空处理：UI 隐藏统计行（显式降级，
 * 字段缺省即不渲染），不影响提交列表本身。
 */
export async function listCommitStats(dir: string, hashes: unknown[]): Promise<Map<string, GitCommitStats>> {
  const result = new Map<string, GitCommitStats>();
  const pending: string[] = [];
  const seen = new Set<string>();
  for (const hash of hashes) {
    if (typeof hash !== "string" || !/^[0-9a-f]{40}$/.test(hash) || seen.has(hash)) continue;
    seen.add(hash);
    const cached = commitStatsCache.get(commitStatsCacheKey(dir, hash));
    if (cached) result.set(hash, cached);
    else pending.push(hash);
  }

  // 单批 ≤100 个哈希（argv 约 4KB，Windows 32KB 命令行上限内余量充足）
  for (let i = 0; i < pending.length; i += 100) {
    const chunk = pending.slice(i, i + 100);
    const res = await tryGit(
      dir,
      ["log", "--no-walk=unsorted", "--numstat", "--diff-merges=first-parent", "--pretty=format:" + LOG_FORMAT, ...chunk],
      120_000,
    );
    if (!res.ok) continue;
    for (const commit of parseLogRecords(res.stdout)) {
      if (commit.additions === 0 && commit.deletions === 0 && commit.changedFiles === 0) continue;
      const stats: GitCommitStats = {
        additions: commit.additions,
        deletions: commit.deletions,
        changedFiles: commit.changedFiles,
      };
      commitStatsCache.set(commitStatsCacheKey(dir, commit.hash), stats);
      result.set(commit.hash, stats);
    }
  }
  if (commitStatsCache.size > COMMIT_STATS_CACHE_CAP) {
    for (const key of commitStatsCache) {
      commitStatsCache.delete(key[0]);
      if (commitStatsCache.size <= COMMIT_STATS_CACHE_CAP) break;
    }
  }
  return result;
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
