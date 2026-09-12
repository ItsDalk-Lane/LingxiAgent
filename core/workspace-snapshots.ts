/**
 * core/workspace-snapshots.ts — 回退时撤销文件改动：影子快照核心
 *
 * 每轮输入提交前，把工作区当前状态 `git add -A + commit` 进一个**影子仓库**
 * （`~/.lingxi/workspace-snapshots/{工作区哈希}/repo`），commit 绑定该轮
 * turnInputEntryId 写进会话侧车 `{sessionPath}.snapshots.json`。
 *
 * 硬约束（任务书拍板）：
 *   - 影子仓库只经 `--git-dir` / `--work-tree` 访问：永不读写用户自己的 `.git`，
 *     永不 push，用户工作区内零新增文件（`core.worktree` 不落进用户目录）。
 *   - 排除清单写在影子仓库 `info/exclude`，快照只加不删（永远只追加 commit）。
 *   - 恢复按 `git diff --name-status <目标>`：新增→删除、修改/删除→恢复目标内容；
 *     恢复动作本身经 ResourceIO 写回（自动进文件历史 origin=restore，可再反悔）。
 *   - 拍照失败（异常 / 超大工作区 / 超时）→ 该轮降级为 write/edit 改前备份兜底，
 *     恢复时无快照的文件用 CheckpointStore 备份倒推，逐文件报告成功与失败。
 *
 * 对外只暴露 WorkspaceSnapshotService 与 getWorkspaceSnapshotService（按 lingxiHome 缓存）。
 */
import fs from "node:fs";
import fsp from "node:fs/promises";
import path from "node:path";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { promisify } from "node:util";
import { atomicWriteSync } from "../shared/safe-fs.ts";

const execFileAsync = promisify(execFile);

export const WORKSPACE_SNAPSHOT_VERSION = 1;
export const DEFAULT_SNAPSHOT_TIMEOUT_MS = 20_000;
const MAX_GIT_BUFFER = 64 * 1024 * 1024;

/**
 * 影子仓库排除清单（写进 info/exclude）。用户工作区里零新增文件，所以这些
 * 目录既不能进快照、也不能被恢复动作重建。`.git` 必须显式排除：影子仓库的
 * git-dir 在别处，git 不会自动把工作区里的 `.git` 当元数据。
 */
export const DEFAULT_SNAPSHOT_EXCLUDES = [
  ".git/",
  "**/.git/",
  "node_modules/",
  "**/node_modules/",
  "dist/",
  "dist-*/",
  "**/dist/",
  "build/",
  "**/build/",
  "out/",
  "**/out/",
  "coverage/",
  "**/coverage/",
  ".cache/",
  "**/.cache/",
  ".next/",
  "**/.next/",
  "target/",
  "**/target/",
  ".venv/",
  "**/.venv/",
  "__pycache__/",
  "**/__pycache__/",
  "workspace-snapshots/",
  ".DS_Store",
  "*.log",
];

export type WorkspaceSnapshotChange = {
  path: string;
  status: string;
};

export type WorkspaceSnapshotRecord = {
  turnInputEntryId: string | null;
  commit: string | null;
  capturedAt: number;
  degraded: boolean;
  reason?: string;
  workspaceRoot?: string;
};

export type WorkspaceSnapshotSidecar = {
  version: number;
  workspaceRoot: string | null;
  snapshots: WorkspaceSnapshotRecord[];
};

export type WorkspaceRollbackFileResult = {
  path: string;
  change: string;
  action: "deleted" | "restored" | "skipped" | "failed";
  source: "snapshot" | "backup";
  ok: boolean;
  reason?: string;
};

export type WorkspaceRollbackReport = {
  ok: boolean;
  reason: string | null;
  degraded: boolean;
  commit: string | null;
  turnInputEntryId: string | null;
  files: WorkspaceRollbackFileResult[];
  failures: WorkspaceRollbackFileResult[];
};

export type WorkspaceSnapshotPreview = {
  available: boolean;
  degraded: boolean;
  commit: string | null;
  reason: string | null;
  files: WorkspaceSnapshotChange[];
  fileCount: number;
};

type FileHistoryCapture = {
  captureNow(root: string, relPath: string, origin: "restore"): Promise<void>;
};

type ResourceIOWriter = {
  write(ref: { kind: "local-file"; path: string }, content: string | Buffer, options?: any): Promise<any>;
  delete(ref: { kind: "local-file"; path: string }, options?: any): Promise<any>;
};

type BackupStore = {
  list(): Promise<any[]>;
  restore(id: string): Promise<any>;
};

export function workspaceHashForSnapshot(root: string): string {
  const normalized = path.resolve(root).split(path.sep).join("/");
  return createHash("sha256").update(normalized).digest("hex").slice(0, 16);
}

export function workspaceSnapshotRoot(lingxiHome: string, workspaceRoot: string): string {
  return path.join(path.resolve(lingxiHome), "workspace-snapshots", workspaceHashForSnapshot(workspaceRoot));
}

export function workspaceSnapshotRepoDir(lingxiHome: string, workspaceRoot: string): string {
  return path.join(workspaceSnapshotRoot(lingxiHome, workspaceRoot), "repo");
}

export function snapshotSidecarPath(sessionPath: string): string {
  return `${sessionPath}.snapshots.json`;
}

/**
 * `git diff --name-status --no-renames -z` 输出：`<status>\0<path>\0` 序列。
 * 重命名/复制（`R*`/`C*`）带两个路径，这里防御性跳过（调用方始终传 --no-renames）。
 */
export function parseNameStatusZ(output: string): WorkspaceSnapshotChange[] {
  const tokens = String(output || "").split("\0");
  const changes: WorkspaceSnapshotChange[] = [];
  for (let index = 0; index < tokens.length; index += 1) {
    const status = tokens[index];
    if (!status) continue;
    const code = status[0];
    if (code === "R" || code === "C") {
      index += 2;
      continue;
    }
    const filePath = tokens[index + 1];
    index += 1;
    if (!filePath) continue;
    changes.push({ path: filePath, status: code });
  }
  return changes;
}

/** `status --porcelain -z` 输出：`XY <path>\0`；返回未跟踪文件路径。 */
export function parseUntrackedZ(output: string): string[] {
  const paths: string[] = [];
  for (const entry of String(output || "").split("\0")) {
    if (!entry.startsWith("??")) continue;
    const filePath = entry.slice(3);
    if (filePath) paths.push(filePath);
  }
  return paths;
}

function normalizeRelPath(root: string, absPath: string): string {
  const rel = path.relative(path.resolve(root), path.resolve(absPath));
  if (!rel || rel.startsWith("..")) return path.resolve(absPath).split(path.sep).join("/");
  return rel.split(path.sep).join("/");
}

async function defaultWriteFile(absPath: string, content: string | Buffer): Promise<void> {
  await fsp.mkdir(path.dirname(absPath), { recursive: true });
  await fsp.writeFile(absPath, content);
}

async function defaultDeleteFile(absPath: string): Promise<void> {
  await fsp.rm(absPath, { force: true });
}

export class WorkspaceSnapshotService {
  declare _lingxiHome: string;
  declare _now: () => number;
  declare _timeoutMs: number;
  declare _gitBin: string;
  declare _log: (message: string) => void;

  constructor({
    lingxiHome,
    now = () => Date.now(),
    timeoutMs = DEFAULT_SNAPSHOT_TIMEOUT_MS,
    gitBin = "git",
    log = () => {},
  }: {
    lingxiHome: string;
    now?: () => number;
    timeoutMs?: number;
    gitBin?: string;
    log?: (message: string) => void;
  }) {
    if (!lingxiHome) throw new Error("WorkspaceSnapshotService requires lingxiHome");
    this._lingxiHome = lingxiHome;
    this._now = now;
    this._timeoutMs = timeoutMs;
    this._gitBin = gitBin;
    this._log = log;
  }

  repoDirFor(workspaceRoot: string): string {
    return workspaceSnapshotRepoDir(this._lingxiHome, workspaceRoot);
  }

  sidecarPathFor(sessionPath: string): string {
    return snapshotSidecarPath(sessionPath);
  }

  // ────────────────────────── 影子仓库 ──────────────────────────

  _gitEnv(): NodeJS.ProcessEnv {
    return {
      ...process.env,
      GIT_OPTIONAL_LOCKS: "0",
      GIT_TERMINAL_PROMPT: "0",
    };
  }

  /**
   * 只经 --git-dir / --work-tree 访问影子仓库；`init` 不带 --work-tree，
   * 避免把 core.worktree 写进用户目录。返回 stdout（utf8 或 Buffer）。
   */
  async _run(workspaceRoot: string, args: string[], { encoding = "utf8", withWorkTree = true }: { encoding?: "utf8" | "buffer"; withWorkTree?: boolean } = {}): Promise<any> {
    const repoDir = this.repoDirFor(workspaceRoot);
    const argv = [
      `--git-dir=${repoDir}`,
      ...(withWorkTree ? [`--work-tree=${path.resolve(workspaceRoot)}`] : []),
      "-c",
      "core.quotepath=false",
      "-c",
      "core.autocrlf=false",
      "-c",
      "core.safecrlf=false",
      "-c",
      "commit.gpgsign=false",
      "-c",
      "user.name=Lingxi Snapshot",
      "-c",
      "user.email=lingxi-snapshot@local",
      ...args,
    ];
    const { stdout } = await execFileAsync(this._gitBin, argv, {
      cwd: path.resolve(workspaceRoot),
      timeout: this._timeoutMs,
      maxBuffer: MAX_GIT_BUFFER,
      windowsHide: true,
      shell: false,
      env: this._gitEnv(),
      encoding,
    } as any);
    return stdout;
  }

  async ensureRepo(workspaceRoot: string): Promise<void> {
    const root = path.resolve(workspaceRoot);
    const repoDir = this.repoDirFor(root);
    if (fs.existsSync(path.join(repoDir, "HEAD"))) return;
    fs.mkdirSync(repoDir, { recursive: true });
    await this._run(root, ["init", "--quiet"], { withWorkTree: false });
    const excludePath = path.join(repoDir, "info", "exclude");
    fs.mkdirSync(path.dirname(excludePath), { recursive: true });
    fs.writeFileSync(excludePath, `${DEFAULT_SNAPSHOT_EXCLUDES.join("\n")}\n`, "utf-8");
  }

  async _revParseHead(workspaceRoot: string): Promise<string | null> {
    try {
      const out = await this._run(workspaceRoot, ["rev-parse", "--verify", "--quiet", "HEAD"]);
      const hash = String(out || "").trim();
      return hash || null;
    } catch {
      return null;
    }
  }

  // ────────────────────────── 侧车 ──────────────────────────

  readSidecar(sessionPath: string): WorkspaceSnapshotSidecar {
    const filePath = snapshotSidecarPath(sessionPath);
    try {
      const raw = fs.readFileSync(filePath, "utf-8");
      const parsed = JSON.parse(raw);
      const snapshots = Array.isArray(parsed?.snapshots) ? parsed.snapshots : [];
      return {
        version: typeof parsed?.version === "number" ? parsed.version : WORKSPACE_SNAPSHOT_VERSION,
        workspaceRoot: typeof parsed?.workspaceRoot === "string" ? parsed.workspaceRoot : null,
        snapshots: snapshots.filter((entry: any) => entry && typeof entry === "object").map((entry: any) => ({
          turnInputEntryId: typeof entry.turnInputEntryId === "string" ? entry.turnInputEntryId : null,
          commit: typeof entry.commit === "string" ? entry.commit : null,
          capturedAt: Number(entry.capturedAt) || 0,
          degraded: entry.degraded === true,
          ...(typeof entry.reason === "string" ? { reason: entry.reason } : {}),
          ...(typeof entry.workspaceRoot === "string" ? { workspaceRoot: entry.workspaceRoot } : {}),
        })),
      };
    } catch {
      return { version: WORKSPACE_SNAPSHOT_VERSION, workspaceRoot: null, snapshots: [] };
    }
  }

  writeSidecar(sessionPath: string, sidecar: WorkspaceSnapshotSidecar): void {
    const filePath = snapshotSidecarPath(sessionPath);
    fs.mkdirSync(path.dirname(filePath), { recursive: true });
    atomicWriteSync(filePath, `${JSON.stringify({
      version: WORKSPACE_SNAPSHOT_VERSION,
      workspaceRoot: sidecar.workspaceRoot ?? null,
      snapshots: sidecar.snapshots,
    }, null, 2)}\n`);
  }

  listSnapshots(sessionPath: string): WorkspaceSnapshotRecord[] {
    return this.readSidecar(sessionPath).snapshots;
  }

  _upsertRecord(sessionPath: string, workspaceRoot: string, record: WorkspaceSnapshotRecord): void {
    const sidecar = this.readSidecar(sessionPath);
    sidecar.workspaceRoot = sidecar.workspaceRoot || path.resolve(workspaceRoot);
    sidecar.snapshots.push(record);
    this.writeSidecar(sessionPath, sidecar);
  }

  // ────────────────────────── 拍照 ──────────────────────────

  /**
   * 该轮输入提交前拍照。失败不抛：写一条 degraded 记录（无 commit），
   * 由恢复侧改走 write/edit 备份兜底。
   */
  async captureTurn({
    sessionPath,
    workspaceRoot,
    turnInputEntryId = null,
    label = null,
  }: {
    sessionPath: string;
    workspaceRoot: string;
    turnInputEntryId?: string | null;
    label?: string | null;
  }): Promise<WorkspaceSnapshotRecord> {
    const capturedAt = this._now();
    const root = path.resolve(workspaceRoot);
    try {
      const stat = await fsp.stat(root).catch(() => null);
      if (!stat?.isDirectory()) throw new Error(`workspace is not a directory: ${root}`);
      await this.ensureRepo(root);
      await this._run(root, ["add", "-A"]);
      const message = `snapshot ${label || turnInputEntryId || capturedAt}`;
      await this._run(root, ["commit", "--quiet", "--allow-empty", "--no-verify", "-m", message]);
      const commit = await this._revParseHead(root);
      if (!commit) throw new Error("snapshot commit was not created");
      const record: WorkspaceSnapshotRecord = {
        turnInputEntryId: turnInputEntryId || null,
        commit,
        capturedAt,
        degraded: false,
        workspaceRoot: root,
      };
      this._upsertRecord(sessionPath, root, record);
      return record;
    } catch (error: any) {
      const reason = error?.message || String(error);
      const record: WorkspaceSnapshotRecord = {
        turnInputEntryId: turnInputEntryId || null,
        commit: null,
        capturedAt,
        degraded: true,
        reason,
        workspaceRoot: root,
      };
      try {
        this._upsertRecord(sessionPath, root, record);
      } catch (writeError: any) {
        this._log(`snapshot sidecar write failed for ${sessionPath}: ${writeError?.message || writeError}`);
      }
      this._log(`workspace snapshot degraded for ${root}: ${reason}`);
      return record;
    }
  }

  /**
   * 拍照后补绑该轮 turnInputEntryId。
   *
   * SDK 在 prompt 落盘时才铸 entry id，所以语义是两段式：先是「输入提交前」
   * 抓 commit（capturedAt 唯一），回合启动后再把该轮 turn input entry id 绑上去。
   * 绑定时只认 turnInputEntryId 仍为空、且 capturedAt 命中的那条记录。
   */
  bindTurnInput({ sessionPath, capturedAt, turnInputEntryId }: { sessionPath: string; capturedAt: number; turnInputEntryId: string }): boolean {
    if (!turnInputEntryId) return false;
    const sidecar = this.readSidecar(sessionPath);
    const record = sidecar.snapshots.find((entry) => entry.capturedAt === capturedAt && !entry.turnInputEntryId);
    if (!record) return false;
    record.turnInputEntryId = turnInputEntryId;
    this.writeSidecar(sessionPath, sidecar);
    return true;
  }

  /**
   * 解析某轮的快照记录。优先精确绑定；绑定缺失时用 createdAtHint（该 turn
   * input entry 的落盘时间）取「不晚于它、且尚未绑定」的最近一条兜底。
   */
  getTurnSnapshot({
    sessionPath,
    turnInputEntryId,
    createdAtHint = null,
  }: {
    sessionPath: string;
    turnInputEntryId: string;
    createdAtHint?: number | null;
  }): WorkspaceSnapshotRecord | null {
    const snapshots = this.listSnapshots(sessionPath);
    const bound = snapshots.find((entry) => entry.turnInputEntryId === turnInputEntryId);
    if (bound) return bound;
    if (createdAtHint == null) return null;
    const candidates = snapshots
      .filter((entry) => !entry.turnInputEntryId && entry.capturedAt <= createdAtHint)
      .sort((a, b) => b.capturedAt - a.capturedAt);
    return candidates[0] || null;
  }

  // ────────────────────────── 变更清单 ──────────────────────────

  /** 目标 commit → 当前工作区的全部变化（含未跟踪文件）。 */
  async collectChanges(workspaceRoot: string, targetCommit: string): Promise<WorkspaceSnapshotChange[]> {
    const root = path.resolve(workspaceRoot);
    const diffOut = await this._run(root, ["diff", "--name-status", "-z", "--no-renames", targetCommit]);
    const changes = parseNameStatusZ(diffOut);
    const seen = new Set(changes.map((entry) => entry.path));
    const statusOut = await this._run(root, ["status", "--porcelain", "-z", "--untracked-files=all"]);
    for (const filePath of parseUntrackedZ(statusOut)) {
      if (seen.has(filePath)) continue;
      seen.add(filePath);
      changes.push({ path: filePath, status: "A" });
    }
    changes.sort((a, b) => a.path.localeCompare(b.path));
    return changes;
  }

  async _showBlob(workspaceRoot: string, commit: string, relPath: string): Promise<Buffer> {
    const out = await this._run(workspaceRoot, ["cat-file", "blob", `${commit}:${relPath}`], { encoding: "buffer" });
    return Buffer.isBuffer(out) ? out : Buffer.from(out);
  }

  // ────────────────────────── 预览 ──────────────────────────

  async previewTurn({
    sessionPath,
    workspaceRoot,
    turnInputEntryId,
    createdAtHint = null,
  }: {
    sessionPath: string;
    workspaceRoot: string;
    turnInputEntryId: string;
    createdAtHint?: number | null;
  }): Promise<WorkspaceSnapshotPreview> {
    const record = this.getTurnSnapshot({ sessionPath, turnInputEntryId, createdAtHint });
    if (!record) {
      return { available: false, degraded: false, commit: null, reason: "no_checkpoint", files: [], fileCount: 0 };
    }
    if (record.degraded || !record.commit) {
      return {
        available: true,
        degraded: true,
        commit: null,
        reason: record.reason || "snapshot_degraded",
        files: [],
        fileCount: 0,
      };
    }
    try {
      const changes = await this.collectChanges(workspaceRoot, record.commit);
      return { available: true, degraded: false, commit: record.commit, reason: null, files: changes, fileCount: changes.length };
    } catch (error: any) {
      return { available: true, degraded: false, commit: record.commit, reason: error?.message || String(error), files: [], fileCount: 0 };
    }
  }

  // ────────────────────────── 恢复 ──────────────────────────

  async restoreTurn({
    sessionPath,
    workspaceRoot,
    turnInputEntryId,
    createdAtHint = null,
    resourceIO = null,
    fileHistory = null,
    backupStore = null,
  }: {
    sessionPath: string;
    workspaceRoot: string;
    turnInputEntryId: string;
    createdAtHint?: number | null;
    resourceIO?: ResourceIOWriter | null;
    fileHistory?: FileHistoryCapture | null;
    backupStore?: BackupStore | null;
  }): Promise<WorkspaceRollbackReport> {
    const root = path.resolve(workspaceRoot);
    const record = this.getTurnSnapshot({ sessionPath, turnInputEntryId, createdAtHint });
    if (!record) {
      return {
        ok: false,
        reason: "no_checkpoint",
        degraded: false,
        commit: null,
        turnInputEntryId,
        files: [],
        failures: [],
      };
    }
    if (record.degraded || !record.commit) {
      return this._restoreFromBackups({
        sessionPath,
        workspaceRoot: root,
        turnInputEntryId,
        record,
        resourceIO,
        fileHistory,
        backupStore,
      });
    }

    const report: WorkspaceRollbackReport = {
      ok: true,
      reason: null,
      degraded: false,
      commit: record.commit,
      turnInputEntryId,
      files: [],
      failures: [],
    };
    let changes: WorkspaceSnapshotChange[];
    try {
      changes = await this.collectChanges(root, record.commit);
    } catch (error: any) {
      report.ok = false;
      report.reason = error?.message || String(error);
      return report;
    }

    for (const change of changes) {
      const result = await this._restoreChange({
        workspaceRoot: root,
        commit: record.commit,
        change,
        resourceIO,
        fileHistory,
      });
      report.files.push(result);
      if (!result.ok) report.failures.push(result);
    }
    report.ok = report.failures.length === 0;
    if (!report.ok) report.reason = "partial_failure";
    return report;
  }

  async _restoreChange({
    workspaceRoot,
    commit,
    change,
    resourceIO,
    fileHistory,
  }: {
    workspaceRoot: string;
    commit: string;
    change: WorkspaceSnapshotChange;
    resourceIO: ResourceIOWriter | null;
    fileHistory: FileHistoryCapture | null;
  }): Promise<WorkspaceRollbackFileResult> {
    const absPath = path.join(workspaceRoot, ...change.path.split("/"));
    if (change.status === "A") {
      try {
        if (resourceIO) await resourceIO.delete({ kind: "local-file", path: absPath }, {});
        else await defaultDeleteFile(absPath);
        return { path: change.path, change: "added", action: "deleted", source: "snapshot", ok: true };
      } catch (error: any) {
        return { path: change.path, change: "added", action: "failed", source: "snapshot", ok: false, reason: error?.message || String(error) };
      }
    }
    try {
      const content = await this._showBlob(workspaceRoot, commit, change.path);
      if (resourceIO) await resourceIO.write({ kind: "local-file", path: absPath }, content, {});
      else await defaultWriteFile(absPath, content);
      if (fileHistory) {
        await fileHistory.captureNow(workspaceRoot, change.path, "restore").catch((error: any) => {
          this._log(`file history capture after restore failed for ${change.path}: ${error?.message || error}`);
        });
      }
      return { path: change.path, change: statusToChange(change.status), action: "restored", source: "snapshot", ok: true };
    } catch (error: any) {
      return { path: change.path, change: statusToChange(change.status), action: "failed", source: "snapshot", ok: false, reason: error?.message || String(error) };
    }
  }

  /**
   * 拍照失败轮次的兜底：用 write/edit 改前备份倒推。只认同会话、且
   * 备份时间不早于本轮拍照时间的备份；同一路径取最早一条（最接近改前）。
   */
  async _restoreFromBackups({
    sessionPath,
    workspaceRoot,
    turnInputEntryId,
    record,
    resourceIO,
    fileHistory,
    backupStore,
  }: {
    sessionPath: string;
    workspaceRoot: string;
    turnInputEntryId: string;
    record: WorkspaceSnapshotRecord;
    resourceIO: ResourceIOWriter | null;
    fileHistory: FileHistoryCapture | null;
    backupStore: BackupStore | null;
  }): Promise<WorkspaceRollbackReport> {
    const report: WorkspaceRollbackReport = {
      ok: true,
      reason: null,
      degraded: true,
      commit: null,
      turnInputEntryId,
      files: [],
      failures: [],
    };
    if (!backupStore || typeof backupStore.list !== "function") {
      report.ok = false;
      report.reason = "degraded_no_backup_store";
      return report;
    }
    let backups: any[] = [];
    try {
      backups = await backupStore.list();
    } catch (error: any) {
      report.ok = false;
      report.reason = `backup_list_failed: ${error?.message || error}`;
      return report;
    }
    const candidates = new Map<string, any>();
    for (const entry of backups) {
      if (!entry || typeof entry.path !== "string" || !entry.path) continue;
      if (entry.sessionPath !== sessionPath) continue;
      if (!(Number(entry.ts) >= record.capturedAt)) continue;
      const existing = candidates.get(entry.path);
      if (!existing || Number(entry.ts) < Number(existing.ts)) candidates.set(entry.path, entry);
    }
    if (candidates.size === 0) {
      report.ok = false;
      report.reason = "degraded_no_backup_entries";
      return report;
    }
    for (const [filePath, entry] of candidates) {
      const relPath = normalizeRelPath(workspaceRoot, filePath);
      try {
        await backupStore.restore(entry.id);
        if (fileHistory && !path.isAbsolute(relPath)) {
          await fileHistory.captureNow(workspaceRoot, relPath, "restore").catch(() => {});
        }
        report.files.push({ path: relPath, change: "modified", action: "restored", source: "backup", ok: true });
      } catch (error: any) {
        const failure: WorkspaceRollbackFileResult = {
          path: relPath,
          change: "modified",
          action: "failed",
          source: "backup",
          ok: false,
          reason: error?.message || String(error),
        };
        report.files.push(failure);
        report.failures.push(failure);
      }
    }
    report.ok = report.failures.length === 0;
    if (!report.ok) report.reason = "partial_failure";
    void resourceIO;
    return report;
  }
}

function statusToChange(status: string): string {
  if (status === "A") return "added";
  if (status === "D") return "deleted";
  return "modified";
}

const serviceCache = new Map<string, WorkspaceSnapshotService>();

/** 按 lingxiHome 缓存单例；core 两侧（coordinator 拍照 / retry 恢复）共用同一实例。 */
export function getWorkspaceSnapshotService({ lingxiHome, log }: { lingxiHome: string; log?: (message: string) => void }): WorkspaceSnapshotService {
  const key = path.resolve(lingxiHome);
  let service = serviceCache.get(key);
  if (!service) {
    service = new WorkspaceSnapshotService({ lingxiHome: key, log });
    serviceCache.set(key, service);
  }
  return service;
}
