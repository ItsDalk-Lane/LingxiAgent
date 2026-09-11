/**
 * core/media/task-store.ts
 *
 * In-memory task metadata store with debounced atomic write to disk.
 * Memory is the authority; disk is a snapshot for restart recovery.
 *
 * Extracted from the dreamina plugin with two key changes:
 *   - submitId renamed to taskId throughout
 *   - adapterId added as a required field
 */

import fs from "node:fs";
import path from "node:path";
import { randomUUID } from "node:crypto";
import { collectRetainedMediaTaskState } from "./session-fork.ts";

const DEBOUNCE_MS = 300;
const TERMINAL_STATUSES = new Set(["done", "failed", "cancelled", "aborted"]);
const TASK_STATUSES = new Set(["pending", ...TERMINAL_STATUSES]);
const DELIVERY_STATES = new Set(["pending", "handed_off", "not_required"]);

export function mediaTaskAttempt(task) {
  return Number.isSafeInteger(task?.attempt) && task.attempt > 0 ? task.attempt : 1;
}

export function isMediaTaskTerminal(task) {
  return TERMINAL_STATUSES.has(task?.status);
}
const LEGACY_PROTOCOL_BY_ADAPTER = {
  openai: "openai-images",
  "openai-codex-oauth": "openai-codex-responses-image",
  volcengine: "volcengine-images",
  "volcengine-coding": "volcengine-images",
  dashscope: "dashscope-images",
  minimax: "minimax-images",
  gemini: "gemini-generate-content-image",
};

function normalizeLoadedTask(task) {
  if (!task || typeof task !== "object" || Array.isArray(task)
    || typeof task.taskId !== "string" || !task.taskId.trim()
    || typeof task.adapterId !== "string" || !task.adapterId.trim()
    || !TASK_STATUSES.has(task.status)) {
    throw new Error("TaskStore: invalid persisted task identity or status");
  }
  if (task.attempt !== undefined && (!Number.isSafeInteger(task.attempt) || task.attempt < 1)) {
    throw new Error(`TaskStore: invalid attempt for ${task.taskId}`);
  }
  if (task.deliveryState !== undefined && !DELIVERY_STATES.has(task.deliveryState)) {
    throw new Error(`TaskStore: invalid delivery state for ${task.taskId}`);
  }
  for (const field of ["files", "sessionFiles"]) {
    if (task[field] !== undefined && !Array.isArray(task[field])) {
      throw new Error(`TaskStore: invalid ${field} for ${task.taskId}`);
    }
  }
  if (task.files?.some((file) => typeof file !== "string" || !file.trim())) {
    throw new Error(`TaskStore: invalid files for ${task.taskId}`);
  }
  if (task.params !== undefined && (!task.params || typeof task.params !== "object" || Array.isArray(task.params))) {
    throw new Error(`TaskStore: invalid params for ${task.taskId}`);
  }
  const sessionId = task.sessionId || task.sessionRef?.sessionId || null;
  const sessionPath = task.sessionPath || task.sessionRef?.sessionPath || null;
  const sessionRef = sessionId
    ? task.sessionRef || {
      sessionId,
      ...(sessionPath ? { sessionPath } : {}),
    }
    : null;
  const providerId = task.providerId || task.adapterId || task.params?.providerId || null;
  const modelId = task.modelId || task.params?.modelId || task.params?.model || null;
  const protocolId = task.protocolId || task.params?.protocolId || LEGACY_PROTOCOL_BY_ADAPTER[providerId] || null;
  const deliveryMode = task.deliveryMode || task.delivery?.mode || "session";
  const params = {
    ...(task.params || {}),
    ...(providerId ? { providerId } : {}),
    ...(modelId ? { modelId, model: task.params?.model || modelId } : {}),
    ...(protocolId ? { protocolId } : {}),
    ...(task.credentialLaneId ? { credentialLaneId: task.credentialLaneId } : {}),
  };
  return {
    ...task,
    sessionId,
    sessionPath,
    sessionRef,
    providerId,
    modelId,
    protocolId,
    credentialLaneId: task.credentialLaneId || task.params?.credentialLaneId || null,
    deliveryMode,
    delivery: task.delivery || { mode: deliveryMode },
    attempt: mediaTaskAttempt(task),
    // 旧终态没有待交付证据，不能在升级后凭空重放；旧 pending 继续原任务。
    deliveryState: task.deliveryState || (deliveryMode === "response"
      ? "not_required" : task.status === "pending" ? "pending" : "handed_off"),
    files: task.files || [],
    sessionFiles: task.sessionFiles || [],
    params,
  };
}

function text(value) {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function deepClone(value) {
  return value == null ? value : structuredClone(value);
}

function fileSystemErrorCode(error: unknown): string {
  const code = (error as NodeJS.ErrnoException)?.code;
  return typeof code === "string" && code ? code : "FS_ERROR";
}

/**
 * R09：同步 TTS 产物的共同校验原语。
 *
 * realpath 解析根目录和链接的真实目标，path.relative 证明目标仍在 canonical
 * root 内，stat 再确认最终目标是普通文件。lstat 只描述链接本身，不能证明链接
 * 指向哪里，因此不用于最终接受判断。返回值只带原文件名和错误码，避免泄漏
 * canonical 绝对路径。
 */
export function validateMediaOutputs(
  files: unknown,
  generatedDir: unknown,
): { ok: true; files: string[] } | { ok: false; error: string } {
  if (typeof generatedDir !== "string" || !generatedDir.trim()) {
    return { ok: false, error: "speech output generated dir is invalid" };
  }
  let realRoot: string;
  try {
    realRoot = fs.realpathSync(generatedDir);
    if (!fs.statSync(realRoot).isDirectory()) {
      return { ok: false, error: "speech output generated root is not a directory" };
    }
  } catch (error) {
    return { ok: false, error: `speech output generated root is unavailable (${fileSystemErrorCode(error)})` };
  }
  if (!Array.isArray(files) || files.length === 0) {
    return { ok: false, error: "speech generation returned no output files" };
  }

  const accepted: string[] = [];
  for (const entry of files) {
    if (typeof entry !== "string" || !entry.trim() || entry.includes("\0")) {
      return { ok: false, error: "speech generation returned an invalid file entry" };
    }
    const file = entry;
    // 当前 files[] 合同是 generated 根下的文件名；不借本修复新增子目录合同。
    if (path.isAbsolute(file) || file.includes("\\") || file === "." || file === ".." || path.basename(file) !== file) {
      return { ok: false, error: `speech output has an invalid file name: ${file}` };
    }
    let realTarget: string;
    try {
      realTarget = fs.realpathSync(path.join(realRoot, file));
    } catch (error) {
      return { ok: false, error: `speech output file unavailable: ${file} (${fileSystemErrorCode(error)})` };
    }
    const relative = path.relative(realRoot, realTarget);
    if (!relative || relative === ".." || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
      return { ok: false, error: `speech output escapes generated dir: ${file}` };
    }
    try {
      const stat = fs.statSync(realTarget);
      if (!stat.isFile()) {
        return { ok: false, error: `speech output is not a regular file: ${file}` };
      }
      if (stat.size === 0) return { ok: false, error: `media output file is empty: ${file}` };
    } catch (error) {
      return { ok: false, error: `speech output file unavailable: ${file} (${fileSystemErrorCode(error)})` };
    }
    accepted.push(file);
  }
  return { ok: true, files: accepted };
}

// 兼容原来的语音入口；图片、视频、重启恢复共用同一产物判断。
export const validateSynchronousSpeechOutputs = validateMediaOutputs;

// 这里刻意只做纯字符串归一，不走文件系统归一原语：比较的是 session JSONL 的
// 定位路径（可能已经不存在，甚至属于另一台机器上的备份），一旦引入 realpath 就
// 会平白多出磁盘 I/O，还会让 fork 的来源/目标映射依赖当下磁盘状态。
function samePath(left, right) {
  const normalizedLeft = text(left);
  const normalizedRight = text(right);
  if (!normalizedLeft || !normalizedRight) return false;
  return path.resolve(normalizedLeft) === path.resolve(normalizedRight);
}

function requireSessionIdentity(value, fieldName) {
  const sessionId = text(value);
  if (!sessionId) throw new Error(`TaskStore: ${fieldName} is required for Session Fork`);
  return sessionId;
}

function requireSessionPath(value, fieldName) {
  const sessionPath = text(value);
  if (!sessionPath) throw new Error(`TaskStore: ${fieldName} is required for Session Fork`);
  if (!path.isAbsolute(sessionPath)) {
    throw new Error(`TaskStore: ${fieldName} must be an absolute path`);
  }
  return sessionPath;
}

function mediaKindForTask(task) {
  const type = text(task?.type)?.toLowerCase() || "";
  if (type === "video" || type.includes("video")) return "video";
  if (type === "image" || type.includes("image")) return "image";
  return null;
}

function taskBelongsToSession(task, sessionId, sessionPath) {
  const ownerSessionId = text(task?.sessionId) || text(task?.sessionRef?.sessionId);
  if (ownerSessionId) return ownerSessionId === sessionId;
  const ownerSessionPath = text(task?.sessionPath)
    || text(task?.sessionRef?.sessionPath)
    || text(task?.sessionRef?.path);
  return samePath(ownerSessionPath, sessionPath);
}

function forkedMediaTaskId() {
  return `media-fork-${randomUUID()}`;
}

function forkedMediaBatchId() {
  return `media-fork-batch-${randomUUID()}`;
}

function childFileIdentityMaps(forkedSessionFiles) {
  const bySourceId = new Map();
  const bySourcePath = new Map();
  for (const file of Array.isArray(forkedSessionFiles) ? forkedSessionFiles : []) {
    if (!file || typeof file !== "object") continue;
    for (const fileId of [
      ...(Array.isArray(file.legacyFileIds) ? file.legacyFileIds : []),
      file.forkedFromFileId,
    ]) {
      const normalized = text(fileId);
      if (normalized) bySourceId.set(normalized, file);
    }
    for (const filePath of [
      ...(Array.isArray(file.legacyFilePaths) ? file.legacyFilePaths : []),
      file.forkedFromFilePath,
    ]) {
      const normalized = text(filePath);
      if (normalized) bySourcePath.set(path.resolve(normalized), file);
    }
  }
  return { bySourceId, bySourcePath };
}

function resolveChildSessionFile(sourceFile, maps) {
  if (!sourceFile || typeof sourceFile !== "object") return null;
  const sourceId = text(sourceFile.fileId) || text(sourceFile.id);
  if (sourceId && maps.bySourceId.has(sourceId)) return maps.bySourceId.get(sourceId);
  const sourcePath = text(sourceFile.filePath) || text(sourceFile.realPath);
  if (sourcePath && maps.bySourcePath.has(path.resolve(sourcePath))) {
    return maps.bySourcePath.get(path.resolve(sourcePath));
  }
  return null;
}

function mapResultSessionFiles(sourceFiles, maps) {
  if (!Array.isArray(sourceFiles) || sourceFiles.length === 0) return null;
  const mapped = [];
  for (const sourceFile of sourceFiles) {
    const childFile = resolveChildSessionFile(sourceFile, maps);
    if (!childFile) return null;
    mapped.push(deepClone(childFile));
  }
  return mapped;
}

function childFilePathMap(forkedSessionFiles) {
  const result = new Map();
  for (const file of Array.isArray(forkedSessionFiles) ? forkedSessionFiles : []) {
    const targetPath = text(file?.filePath) || text(file?.realPath);
    if (!targetPath) continue;
    for (const sourcePath of Array.isArray(file?.legacyFilePaths) ? file.legacyFilePaths : []) {
      const normalized = text(sourcePath);
      if (normalized) result.set(normalized, targetPath);
    }
  }
  return result;
}

function rewriteExactFilePaths(value, filePathMap) {
  if (typeof value === "string") return filePathMap.get(value) || value;
  if (Array.isArray(value)) return value.map((item) => rewriteExactFilePaths(item, filePathMap));
  if (!value || typeof value !== "object") return value;
  return Object.fromEntries(Object.entries(value).map(([key, child]) => [
    key,
    rewriteExactFilePaths(child, filePathMap),
  ]));
}

function retainedTaskOutcome(task, retainedResult, childFileMaps, forkedAt) {
  const kind = mediaKindForTask(task);
  const deferredType = kind === "video" ? "video-generation" : "image-generation";
  const independentAction = kind === "video"
    ? "Start a new video generation in this Session."
    : "Retry to generate an independent copy.";
  if (retainedResult?.status === "success" || retainedResult?.status === "resolved") {
    const sourceSessionFiles = Array.isArray(retainedResult?.result?.sessionFiles)
      ? retainedResult.result.sessionFiles
      : task.sessionFiles;
    const childSessionFiles = mapResultSessionFiles(sourceSessionFiles, childFileMaps);
    if (childSessionFiles) {
      return {
        taskStatus: "done",
        failReason: null,
        submitState: "completed",
        sessionFiles: childSessionFiles,
        deferredRecord: {
          schemaVersion: 1,
          taskId: null,
          status: "success",
          type: deferredType,
          result: {
            ...(retainedResult.result && typeof retainedResult.result === "object"
              ? deepClone(retainedResult.result)
              : {}),
            sessionFiles: childSessionFiles,
          },
        },
      };
    }
    const reason = `Forked media output is unavailable in this Session. ${independentAction}`;
    return {
      taskStatus: "failed",
      failReason: reason,
      submitState: "failed",
      sessionFiles: [],
      deferredRecord: {
        schemaVersion: 1,
        taskId: null,
        status: "failed",
        type: deferredType,
        reason,
      },
    };
  }

  if (retainedResult?.status === "failed" || retainedResult?.status === "aborted") {
    const status = retainedResult.status === "aborted" ? "aborted" : "failed";
    const reason = text(retainedResult.reason)
      || (status === "aborted" ? "Media generation was stopped." : "Media generation failed.");
    return {
      taskStatus: status,
      failReason: reason,
      submitState: "failed",
      sessionFiles: [],
      deferredRecord: {
        schemaVersion: 1,
        taskId: null,
        status,
        type: deferredType,
        reason,
      },
    };
  }

  const reason = `Media generation was still running at the Fork point. ${independentAction}`;
  return {
    taskStatus: "aborted",
    failReason: reason,
    submitState: "failed",
    sessionFiles: [],
    completedAt: forkedAt,
    deferredRecord: {
      schemaVersion: 1,
      taskId: null,
      status: "aborted",
      type: deferredType,
      reason,
    },
  };
}

export class TaskStore {
  declare _dataDir: any;
  declare _debounceTimer: any;
  declare _filePath: any;
  declare _tasks: any;
  declare _dirty: boolean;
  declare _lastWriteError: Error | null;
  /**
   * @param {string} dataDir  Directory where tasks.json lives (created if absent)
   */
  constructor(dataDir) {
    this._dataDir = dataDir;
    this._filePath = path.join(dataDir, "tasks.json");
    /** @type {Map<string, object>} keyed by taskId */
    this._tasks = new Map();
    this._debounceTimer = null;
    this._dirty = false;
    this._lastWriteError = null;
    this._load();
  }

  // ---------------------------------------------------------------------------
  // Write
  // ---------------------------------------------------------------------------

  /**
   * Add a new task. Throws if taskId already exists.
   *
   * @param {{ taskId: string, adapterId: string, providerId?: string|null, modelId?: string|null, protocolId?: string|null, credentialLaneId?: string|null, batchId: string, type: string, prompt: string, params: object, sessionId?: string|null, sessionPath?: string|null, sessionRef?: object|null, deliveryMode?: string, delivery?: object|null, deliveryTarget?: object|null, metadata?: object|null, adapterTaskId?: string|null, submitState?: string }} opts
   */
  add({ taskId, adapterId, providerId = null, modelId = null, protocolId = null, credentialLaneId = null, credentialProviderId = null, batchId, type, prompt, params, sessionId = null, sessionPath = null, sessionRef = null, deliveryMode = "session", delivery = null, deliveryTarget = null, metadata = null, adapterTaskId = null, submitState = "submitted" }) {
    if (this._tasks.has(taskId)) {
      throw new Error(`TaskStore: duplicate taskId "${taskId}"`);
    }
    const task = {
      taskId,
      adapterId,
      providerId: providerId || adapterId || null,
      modelId: modelId || params?.modelId || params?.model || null,
      protocolId: protocolId || params?.protocolId || null,
      credentialLaneId: credentialLaneId || params?.credentialLaneId || null,
      credentialProviderId: credentialProviderId || params?.credentialProviderId || null,
      batchId,
      type,
      prompt,
      params,
      sessionId: sessionId || sessionRef?.sessionId || null,
      sessionPath: sessionPath || null,
      sessionRef: sessionRef || (sessionId ? {
        sessionId,
        ...(sessionPath ? { sessionPath } : {}),
      } : null),
      deliveryMode: deliveryMode || "session",
      delivery: delivery && typeof delivery === "object" && !Array.isArray(delivery)
        ? delivery
        : { mode: deliveryMode || "session" },
      deliveryTarget: deliveryTarget || null,
      metadata: metadata && typeof metadata === "object" && !Array.isArray(metadata) ? metadata : null,
      adapterTaskId: adapterTaskId || null,
      submitState,
      attempt: 1,
      status: "pending",
      deliveryState: (delivery?.mode || deliveryMode) === "response" ? "not_required" : "pending",
      failReason: null,
      files: [],
      sessionFiles: [],
      favorited: false,
      imageWidth: null,
      imageHeight: null,
      createdAt: new Date().toISOString(),
      completedAt: null,
    };
    this._tasks.set(taskId, task);
    this._scheduleSave();
    return { ...task };
  }

  /**
   * F12/P8.3：同步语音产物的收尾原语（仅 response 投递使用）。
   *
   * 1. files 必须非空、每个条目位于 generated 根目录内且真实存在（不信任
   *    任意路径，拒绝 .. 逃逸）；
   * 2. 验证通过 → status='done'、submitState='completed'、completedAt 定格、
   *    清除 failReason；调用返回前内存即终态（沿用 debounce 持久化，不谎称
   *    同步 fsync）；
   * 3. 幂等：已 done 的任务重复完成不改变完成时间、不新增任务；
   *    已 cancelled/failed/aborted 的任务不得被无条件改 done；
   * 4. 验证失败 → 任务标明确失败并返回 { ok:false }，由调用方把错误抛出。
   */
  completeSynchronousSpeechTask(taskId, { files = [], generatedDir }: Record<string, any> = {}) {
    return this.settleTask(taskId, { status: "done", files, generatedDir });
  }

  isCurrentAttempt(taskId, attempt) {
    const task = this._tasks.get(taskId);
    return !!task && mediaTaskAttempt(task) === attempt;
  }

  /** 只有明确重试才开启新尝试；先持久化，再允许新的供应商调用。 */
  beginAttempt(taskId, patch: Record<string, any> = {}) {
    const task = this._tasks.get(taskId);
    if (!task || !isMediaTaskTerminal(task)) throw new Error(`TaskStore: task is not retryable: ${taskId}`);
    Object.assign(task, patch, {
      attempt: mediaTaskAttempt(task) + 1,
      status: "pending",
      deliveryState: task.deliveryMode === "response" ? "not_required" : "pending",
    });
    this._scheduleSave();
    this.requireFlush();
    return { ...task };
  }

  /** 所有完成路径先证明产物、写入不可倒退的终态，再将交付交给 poller。 */
  settleTask(taskId, { status, expectedAttempt, files = [], generatedDir, ...patch }: Record<string, any>) {
    const task = this._tasks.get(taskId);
    if (!task) return { ok: false, stale: true, error: `TaskStore: task not found: ${taskId}` };
    if (expectedAttempt !== undefined && mediaTaskAttempt(task) !== expectedAttempt) {
      return { ok: false, stale: true, error: `TaskStore: stale attempt: ${taskId}` };
    }
    if (isMediaTaskTerminal(task)) {
      return task.status === status
        ? { ok: true, idempotent: true, task: { ...task } }
        : { ok: false, stale: true, error: `TaskStore: cannot complete ${task.status} task ${taskId}` };
    }
    if (!TERMINAL_STATUSES.has(status)) throw new Error(`TaskStore: invalid terminal status: ${status}`);
    const validation = status === "done" ? validateMediaOutputs(files, generatedDir) : null;
    const outputError = validation?.ok === false ? validation.error : null;
    const finalStatus = outputError ? "failed" : status;
    this.update(taskId, {
      ...patch,
      status: finalStatus,
      submitState: finalStatus === "done" ? "completed" : "failed",
      completedAt: patch.completedAt || new Date().toISOString(),
      failReason: outputError || (finalStatus === "done" ? null : patch.failReason || "generation failed"),
      ...(validation?.ok === true ? { files: [...validation.files] } : {}),
      deliveryState: task.deliveryMode === "response" ? "not_required" : "pending",
    });
    this.requireFlush();
    return outputError
      ? { ok: false, error: outputError, task: { ...this._tasks.get(taskId) } }
      : { ok: true, task: { ...this._tasks.get(taskId) } };
  }

  markDeliveryHandedOff(taskId, expectedAttempt) {
    const task = this._tasks.get(taskId);
    if (!task || !isMediaTaskTerminal(task) || mediaTaskAttempt(task) !== expectedAttempt) return false;
    this.update(taskId, { deliveryState: "handed_off" });
    try { this.requireFlush(); } catch (error) {
      task.deliveryState = "pending";
      this._scheduleSave();
      throw error;
    }
    return true;
  }

  /**
   * F12/P8.4：旧 pending 记录恢复（幂等收尾）。
   *
   * 只处理 type='speech' && delivery.mode='response' && status='pending' 的
   * 记录：文件全部真实存在 → 标 done；文件缺失 → 标明确失败。不重新合成、
   * 不向会话通知、不产生新的模型调用/用量。已终态的记录不改写；图片/视频
   * pending 任务一律不动。
   */
  recoverSynchronousSpeechTasks({ generatedDir, now = () => new Date().toISOString() }: Record<string, any> = {}) {
    if (!generatedDir) return 0;
    let changed = 0;
    for (const task of this._tasks.values()) {
      if (task.type !== "speech") continue;
      const mode = task.deliveryMode || task.delivery?.mode;
      if (mode !== "response") continue;
      if (task.status !== "pending") continue;
      this.settleTask(task.taskId, {
        status: "done", files: task.files, generatedDir, completedAt: task.completedAt || now(),
      });
      changed += 1;
    }
    return changed;
  }

  /**
   * Merge partial fields into an existing task.
   * Returns the updated shallow copy, or null if not found.
   *
   * @param {string} taskId
   * @param {object} patch
   */
  update(taskId, patch) {
    const task = this._tasks.get(taskId);
    if (!task) return null;
    if (isMediaTaskTerminal(task) && patch.status !== undefined && patch.status !== task.status) return null;
    Object.assign(task, patch);
    this._scheduleSave();
    return { ...task };
  }

  /**
   * Delete a task by taskId.
   * Returns true if removed, false if not found.
   *
   * @param {string} taskId
   */
  remove(taskId) {
    const existed = this._tasks.delete(taskId);
    if (existed) this._scheduleSave();
    return existed;
  }

  /**
   * Clone only media tasks structurally referenced by one retained Session
   * branch. Every clone owns a fresh taskId and child SessionRef. A task that
   * had no terminal receipt at the Fork point is converted to an explicit,
   * retryable aborted result instead of sharing the source provider job.
   */
  forkSessionTasks({
    sourceSessionId,
    sourceSessionPath,
    targetSessionId,
    targetSessionPath,
    retainedEntries = [],
    forkedSessionFiles = [],
    createTaskId = forkedMediaTaskId,
    createBatchId = forkedMediaBatchId,
  }: Record<string, any> = {}) {
    const sourceId = requireSessionIdentity(sourceSessionId, "sourceSessionId");
    const targetId = requireSessionIdentity(targetSessionId, "targetSessionId");
    const sourcePath = requireSessionPath(sourceSessionPath, "sourceSessionPath");
    const targetPath = requireSessionPath(targetSessionPath, "targetSessionPath");
    if (sourceId === targetId || samePath(sourcePath, targetPath)) {
      throw new Error("TaskStore: Session Fork target must be independent from its source");
    }
    if (typeof createTaskId !== "function" || typeof createBatchId !== "function") {
      throw new Error("TaskStore: Session Fork id factories must be functions");
    }

    const retainedState = collectRetainedMediaTaskState(retainedEntries);
    const childFileMaps = childFileIdentityMaps(forkedSessionFiles);
    const filePathMap = childFilePathMap(forkedSessionFiles);
    const createdTaskIds = [];
    const taskIdMap: Record<string, string> = {};
    const deferredRecords = [];
    const skipped = [];
    const forkedAt = new Date().toISOString();
    const batchIdBySource = new Map();

    try {
      for (const sourceTaskId of retainedState.taskIds) {
        const sourceTask = this._tasks.get(sourceTaskId);
        if (!sourceTask) {
          skipped.push({ taskId: sourceTaskId, reason: "task_not_found" });
          continue;
        }
        const kind = mediaKindForTask(sourceTask);
        if (!kind) {
          skipped.push({ taskId: sourceTaskId, reason: "not_media_task" });
          continue;
        }
        if (!taskBelongsToSession(sourceTask, sourceId, sourcePath)) {
          throw new Error(`TaskStore: retained media task ownership mismatch for ${sourceTaskId}`);
        }

        const targetTaskId = text(createTaskId(sourceTask, createdTaskIds.length));
        if (!targetTaskId) throw new Error("TaskStore: forked media taskId is required");
        if (targetTaskId === sourceTaskId || this._tasks.has(targetTaskId)) {
          throw new Error(`TaskStore: forked media taskId is not unique: ${targetTaskId}`);
        }
        const sourceBatchId = text(sourceTask.batchId) || sourceTaskId;
        let targetBatchId = batchIdBySource.get(sourceBatchId);
        if (!targetBatchId) {
          targetBatchId = text(createBatchId(sourceTask, batchIdBySource.size));
          if (!targetBatchId) throw new Error("TaskStore: forked media batchId is required");
          batchIdBySource.set(sourceBatchId, targetBatchId);
        }

        const retainedResult = retainedState.resultsByTaskId[sourceTaskId] || null;
        const outcome = retainedTaskOutcome(sourceTask, retainedResult, childFileMaps, forkedAt);
        const params = rewriteExactFilePaths(deepClone(sourceTask.params || {}), filePathMap);
        this.add({
          taskId: targetTaskId,
          adapterId: sourceTask.adapterId,
          providerId: sourceTask.providerId,
          modelId: sourceTask.modelId,
          protocolId: sourceTask.protocolId,
          credentialLaneId: sourceTask.credentialLaneId,
          batchId: targetBatchId,
          type: sourceTask.type,
          prompt: sourceTask.prompt,
          params,
          sessionId: targetId,
          sessionPath: targetPath,
          sessionRef: { sessionId: targetId, sessionPath: targetPath },
          deliveryMode: "session",
          delivery: { mode: "session" },
          deliveryTarget: null,
          metadata: deepClone(sourceTask.metadata),
          adapterTaskId: null,
          submitState: outcome.submitState,
        });
        createdTaskIds.push(targetTaskId);
        this.update(targetTaskId, {
          status: outcome.taskStatus,
          deliveryState: "handed_off",
          failReason: outcome.failReason,
          submitState: outcome.submitState,
          adapterTaskId: null,
          files: [],
          sessionFiles: outcome.sessionFiles,
          favorited: sourceTask.favorited === true,
          imageWidth: outcome.taskStatus === "done" ? sourceTask.imageWidth ?? null : null,
          imageHeight: outcome.taskStatus === "done" ? sourceTask.imageHeight ?? null : null,
          createdAt: sourceTask.createdAt || forkedAt,
          completedAt: outcome.completedAt || sourceTask.completedAt || forkedAt,
          retryCount: 0,
          forkedFromTaskId: sourceTaskId,
          forkedAt,
          sourceTaskStatusAtFork: sourceTask.status || null,
          ...(sourceTask.credentialProviderId
            ? { credentialProviderId: sourceTask.credentialProviderId }
            : {}),
        });
        taskIdMap[sourceTaskId] = targetTaskId;
        deferredRecords.push({
          ...outcome.deferredRecord,
          taskId: targetTaskId,
        });
      }
      if (createdTaskIds.length > 0 && !this.flushSync()) {
        throw new Error("TaskStore: forked media tasks could not be persisted");
      }
      return {
        tasks: createdTaskIds.length,
        taskIds: createdTaskIds,
        taskIdMap,
        deferredRecords,
        skipped,
      };
    } catch (error) {
      for (const taskId of createdTaskIds) this._tasks.delete(taskId);
      if (createdTaskIds.length > 0) this.flushSync();
      throw error;
    }
  }

  discardForkedSessionTasks({ targetSessionId, taskIds = [] }: Record<string, any> = {}) {
    const targetId = requireSessionIdentity(targetSessionId, "targetSessionId");
    let discarded = 0;
    const skipped = [];
    const removed = [];
    for (const taskId of Array.isArray(taskIds) ? taskIds : []) {
      const normalizedTaskId = text(taskId);
      const task = normalizedTaskId ? this._tasks.get(normalizedTaskId) : null;
      if (!task) continue;
      const ownerSessionId = text(task.sessionId) || text(task.sessionRef?.sessionId);
      if (ownerSessionId !== targetId || !text(task.forkedFromTaskId)) {
        skipped.push(normalizedTaskId);
        continue;
      }
      removed.push([normalizedTaskId, task]);
      this._tasks.delete(normalizedTaskId);
      discarded += 1;
    }
    if (discarded > 0 && !this.flushSync()) {
      for (const [taskId, task] of removed) this._tasks.set(taskId, task);
      throw new Error("TaskStore: forked media task cleanup could not be persisted");
    }
    return { discarded, skipped };
  }

  /**
   * Remove all non-pending, non-favorited tasks.
   * Returns an array of the removed task shallow copies so the caller
   * can clean up associated files on disk.
   *
   * @returns {object[]}
   */
  removeUnfavorited() {
    const removed = [];
    for (const [taskId, task] of this._tasks) {
      if (!task.favorited && task.status !== "pending") {
        removed.push({ ...task });
        this._tasks.delete(taskId);
      }
    }
    if (removed.length > 0) this._scheduleSave();
    return removed;
  }

  // ---------------------------------------------------------------------------
  // Read
  // ---------------------------------------------------------------------------

  /**
   * @param {string} taskId
   * @returns {object|null} shallow copy or null
   */
  get(taskId) {
    const task = this._tasks.get(taskId);
    return task ? { ...task } : null;
  }

  /**
   * All tasks belonging to a batchId, as shallow copies.
   *
   * @param {string} batchId
   * @returns {object[]}
   */
  getByBatch(batchId) {
    return this._filter((t) => t.batchId === batchId);
  }

  /**
   * All tasks belonging to a given adapterId, as shallow copies.
   *
   * @param {string} adapterId
   * @returns {object[]}
   */
  getByAdapter(adapterId) {
    return this._filter((t) => t.adapterId === adapterId);
  }

  /**
   * All tasks as shallow copies, insertion order.
   *
   * @returns {object[]}
   */
  listAll() {
    return this._filter(() => true);
  }

  /**
   * Tasks with status === "pending" as shallow copies.
   *
   * @returns {object[]}
   */
  listPending() {
    return this._filter((t) => t.status === "pending");
  }

  /**
   * Tasks with favorited === true as shallow copies.
   *
   * @returns {object[]}
   */
  listFavorited() {
    return this._filter((t) => t.favorited === true);
  }

  // ---------------------------------------------------------------------------
  // Persistence
  // ---------------------------------------------------------------------------

  /**
   * Flush pending write immediately (synchronous).
   * Useful before process exit or in tests.
   */
  flushSync() {
    if (this._debounceTimer !== null) {
      clearTimeout(this._debounceTimer);
      this._debounceTimer = null;
    }
    return !this._dirty || this._writeSync();
  }

  requireFlush() {
    if (!this.flushSync()) throw this._lastWriteError || new Error("TaskStore: persistence failed");
  }

  /**
   * Cancel any pending debounce timer. Call on plugin unload.
   */
  destroy() {
    this.requireFlush();
  }

  // ---------------------------------------------------------------------------
  // Private
  // ---------------------------------------------------------------------------

  /** @param {(task: object) => boolean} predicate */
  _filter(predicate) {
    const result = [];
    for (const task of this._tasks.values()) {
      if (predicate(task)) result.push({ ...task });
    }
    return result;
  }

  _load() {
    let raw;
    try {
      raw = fs.readFileSync(this._filePath, "utf8");
    } catch (error) {
      if (fileSystemErrorCode(error) === "ENOENT") return;
      throw error;
    }
    const rows = JSON.parse(raw);
    if (!Array.isArray(rows)) throw new Error("TaskStore: persisted tasks must be an array");
    const loaded = new Map();
    for (const row of rows) {
      const task = normalizeLoadedTask(row);
      if (loaded.has(task.taskId)) throw new Error(`TaskStore: duplicate persisted taskId: ${task.taskId}`);
      loaded.set(task.taskId, task);
    }
    // 整份验证成功才采用；读取兼容转换不写回，也不会覆盖损坏原件。
    this._tasks = loaded;
  }

  _scheduleSave() {
    this._dirty = true;
    if (this._debounceTimer !== null) clearTimeout(this._debounceTimer);
    this._debounceTimer = setTimeout(() => {
      this._debounceTimer = null;
      this._writeSync();
    }, DEBOUNCE_MS);
  }

  _writeSync() {
    try {
      fs.mkdirSync(this._dataDir, { recursive: true });
      const tmp = this._filePath + ".tmp";
      fs.writeFileSync(tmp, JSON.stringify([...this._tasks.values()]), "utf8");
      fs.renameSync(tmp, this._filePath);
      this._dirty = false;
      this._lastWriteError = null;
      return true;
    } catch (err) {
      // Ordinary task updates keep memory authoritative; transactional callers
      // can treat the false return as a hard persistence failure.
      process.stderr.write(`TaskStore: write failed: ${err.message}\n`);
      this._lastWriteError = err;
      return false;
    }
  }
}
