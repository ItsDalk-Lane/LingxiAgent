import fs from "fs";
import path from "path";
import { atomicWriteSync } from "../shared/safe-fs.ts";
import { createModuleLogger } from "./debug-log.ts";

export type TaskStatus = "pending" | "running" | "paused" | "blocked" | "recovering" | "completed" | "failed" | "canceled" | "aborted";
const STATUS_KIND: Record<TaskStatus, "active" | "final"> = {
  pending: "active", running: "active", paused: "active", blocked: "active", recovering: "active",
  completed: "final", failed: "final", canceled: "final", aborted: "final",
};
export interface AttemptOptions { expectedAttempt?: number | null | undefined }
export interface TaskRecord {
  taskId: string; type: string; attempt: number; status: TaskStatus; aborted: boolean;
  parentSessionId: string | null; parentSessionPath: string | null; parentSessionRef: Record<string, unknown> | null;
  pluginId: string | null; agentId: string | null; meta: Record<string, unknown>; progress: unknown;
  createdAt: number; updatedAt: number; completedAt?: number; result?: unknown; error?: string; persist: boolean;
}
export interface TaskPatch {
  status?: TaskStatus; result?: unknown; error?: unknown; progress?: unknown; meta?: unknown;
  parentSessionPath?: string | null; parentSessionId?: string | null; parentSessionRef?: Record<string, unknown> | null;
  agentId?: string | null; pluginId?: string | null;
}
export interface TaskRegistration {
  type?: string | undefined; parentSessionPath?: string | null | undefined; parentSessionId?: string | null | undefined;
  parentSessionRef?: unknown; sessionId?: string | null | undefined; sessionRef?: unknown; legacySessionPath?: string | null | undefined;
  meta?: unknown; pluginId?: string | null | undefined; agentId?: string | null | undefined; persist?: boolean | undefined;
}
export interface TaskRegistryOptions { persistencePath?: string; getSessionIdForPath?: (path: string | null) => string | null }
export interface TaskHandler { abort: (taskId: string) => unknown; run?: ((schedule: unknown) => unknown) | undefined }
export interface PersistenceFeedback { durable: boolean; status: "disabled" | "saved" | "failed"; error: string | null }
const log = createModuleLogger("task-registry");

/**
 * TaskRegistry — plugin-safe background task registry.
 *
 * Runtime handlers stay in memory because they are plugin functions. Task and
 * schedule metadata can be persisted so the host can show diagnostics and let
 * plugins recover work after restart.
 *
 * Attempt 栅栏（P02-T01/A03）：同一业务 taskId 允许在终态后重新 register 承接
 * 下一次执行（合法 task 复用）；每次复用递增 task.attempt。终态写入方
 * 必须在启动时捕获并携带 expectedAttempt；只对从未复用的首次执行兼容缺省。批次不匹配的迟到回调被
 * 拒绝（返回 null、不落盘），防止上一次执行的迟到结果覆盖下一次执行的状态。
 */

const ACTIVE_STATUSES = new Set<TaskStatus>((Object.keys(STATUS_KIND) as TaskStatus[]).filter((status) => STATUS_KIND[status] === "active"));
export const ACTIVE_TASK_STATUSES = ACTIVE_STATUSES;
const FINAL_STATUSES = new Set<TaskStatus>((Object.keys(STATUS_KIND) as TaskStatus[]).filter((status) => STATUS_KIND[status] === "final"));
const KNOWN_STATUSES = new Set([...ACTIVE_STATUSES, ...FINAL_STATUSES]);
const MAX_TIMER_DELAY = 2_147_483_647;

function textOrNull(value: unknown) {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function normalizeParentSessionRef(value: unknown = {}, resolveSessionIdForPath: ((path: string | null) => string | null) | null = null) {
  if (typeof value === "string") {
    const parentSessionPath = textOrNull(value);
    const parentSessionId = textOrNull(resolveSessionIdForPath?.(parentSessionPath));
    const parentSessionRef = parentSessionId
      ? { sessionId: parentSessionId, ...(parentSessionPath ? { sessionPath: parentSessionPath, legacySessionPath: parentSessionPath } : {}) }
      : null;
    return { parentSessionId, parentSessionPath, parentSessionRef };
  }
  const input = objectOrEmpty(value);
  const rawRef = objectOrEmpty(input.parentSessionRef || input.sessionRef);
  const parentSessionId =
    textOrNull(input.parentSessionId)
    || textOrNull(input.sessionId)
    || textOrNull(rawRef?.sessionId);
  const parentSessionPath =
    textOrNull(input.parentSessionPath)
    || textOrNull(input.sessionPath)
    || textOrNull(rawRef?.sessionPath)
    || textOrNull(rawRef?.path);
  const resolvedParentSessionId = parentSessionId || textOrNull(resolveSessionIdForPath?.(parentSessionPath));
  const legacySessionPath =
    textOrNull(input.legacySessionPath)
    || textOrNull(rawRef?.legacySessionPath)
    || (resolvedParentSessionId && parentSessionPath ? parentSessionPath : null);
  const parentSessionRef = resolvedParentSessionId
    ? {
      sessionId: resolvedParentSessionId,
      ...(parentSessionPath ? { sessionPath: parentSessionPath } : {}),
      ...(legacySessionPath ? { legacySessionPath } : {}),
    }
    : null;
  return { parentSessionId: resolvedParentSessionId, parentSessionPath, parentSessionRef };
}

function matchesParentSession(task: TaskRecord, input: unknown, resolveSessionIdForPath: ((path: string | null) => string | null) | null = null) {
  const target = normalizeParentSessionRef(input, resolveSessionIdForPath);
  if (target.parentSessionId) {
    return task.parentSessionId === target.parentSessionId
      || task.parentSessionRef?.sessionId === target.parentSessionId;
  }
  return !!target.parentSessionPath && task.parentSessionPath === target.parentSessionPath;
}

export class TaskRegistry {
  declare _handlers: Map<string, TaskHandler>;
  declare _getSessionIdForPath: (path: string | null) => string | null;
  declare _persistencePath: string | null;
  declare _scheduleTimers: Map<string, ReturnType<typeof setTimeout>>;
  declare _schedules: any;
  declare _tasks: Map<string, TaskRecord>;
  // 删除记录的有限墓碑；淘汰时提升分配下限，旧批次仍不能冒认新执行。
  private _lastAttempts = new Map<string, number>();
  private _attemptFloor = 1;
  private _maxAttempt = 1;
  private _persistence: PersistenceFeedback = { durable: false, status: "disabled", error: null };
  persistenceStatus(task?: TaskRecord | null): PersistenceFeedback {
    if (task?.persist === false) return { durable: false, status: "disabled", error: null };
    return { ...this._persistence };
  }
  constructor(options: TaskRegistryOptions = {}) {
    this._persistencePath = typeof options.persistencePath === "string" ? options.persistencePath : null;
    this._getSessionIdForPath = typeof options.getSessionIdForPath === "function" ? options.getSessionIdForPath : () => null;
    /** @type {Map<string, { abort: (taskId: string) => void, run?: Function }>} */
    this._handlers = new Map();
    /** @type {Map<string, object>} */
    this._tasks = new Map();
    /** @type {Map<string, object>} */
    this._schedules = new Map();
    /** @type {Map<string, NodeJS.Timeout>} */
    this._scheduleTimers = new Map();
    this._loadPersisted();
  }

  // ── 类型处理器注册（启动时调用） ──

  registerHandler(type: string, handler: TaskHandler) {
    const key = assertText(type, "task handler type");
    if (!handler?.abort || typeof handler.abort !== "function") {
      throw new Error(`TaskRegistry: handler for "${key}" must have an abort(taskId) method`);
    }
    if (handler.run !== undefined && typeof handler.run !== "function") {
      throw new Error(`TaskRegistry: handler for "${key}" run must be a function`);
    }
    this._handlers.set(key, handler);
    this._armSchedulesForType(key);
  }

  unregisterHandler(type: string) {
    this._handlers.delete(type);
  }

  // ── 任务实例生命周期 ──

  register(taskId: string, { type, parentSessionPath = null, parentSessionId = null, parentSessionRef = null, sessionId = null, sessionRef = null, legacySessionPath = null, meta = {}, pluginId = null, agentId = null, persist = true }: TaskRegistration = {}) {
    const id = assertText(taskId, "taskId");
    const taskType = assertText(type, "task type");
    if (!this._handlers.has(taskType)) {
      log.warn(`no handler for type "${taskType}", task ${id} registered without abort support`);
    }
    const existing = this._tasks.get(id);
    const now = Date.now();
    const parentRef = normalizeParentSessionRef({
      parentSessionId,
      parentSessionPath,
      parentSessionRef,
      sessionId,
      sessionRef,
      legacySessionPath,
    }, this._getSessionIdForPath);
    const reactivating = !!existing && FINAL_STATUSES.has(existing.status);
    const task: TaskRecord = {
      taskId: id,
      type: taskType,
      parentSessionId: parentRef.parentSessionId || existing?.parentSessionId || null,
      parentSessionPath: parentRef.parentSessionPath || existing?.parentSessionPath || null,
      parentSessionRef: parentRef.parentSessionRef || existing?.parentSessionRef || null,
      pluginId: pluginId || existing?.pluginId || null,
      agentId: agentId || existing?.agentId || null,
      meta: objectOrEmpty(existing?.meta),
      progress: existing?.progress || null,
      status: normalizeStatus(existing?.status, "running"),
      aborted: Boolean(existing?.aborted),
      attempt: existing ? taskAttempt(existing) + (reactivating ? 1 : 0) : Math.max(this._attemptFloor, (this._lastAttempts.get(id) || 0) + 1),
      createdAt: existing?.createdAt || now,
      updatedAt: now,
      persist: persist !== false,
    };
    task.meta = { ...task.meta, ...objectOrEmpty(meta) };
    if (reactivating) {
      // 合法 task 复用：终态清零进入新一次执行；attempt 已递增，旧 attempt 的
      // 迟到终态回调会被 complete/fail/update 的 expectedAttempt 栅栏拒绝。
      task.status = "running";
      task.aborted = false;
      delete task.completedAt;
      delete task.error;
      delete task.result;
      task.progress = null;
    }
    if (!Number.isSafeInteger(task.attempt)) throw new Error("TaskRegistry: attempt capacity exhausted");
    this._tasks.set(id, task);
    this._maxAttempt = Math.max(this._maxAttempt, task.attempt);
    this._lastAttempts.delete(id);
    this._persist();
    return clone(task);
  }

  update(taskId: string, patch: TaskPatch = {}, options: AttemptOptions = {}) {
    const task = this._requireTask(taskId);
    if (isStaleAttempt(task, options.expectedAttempt)) return null;
    if (FINAL_STATUSES.has(task.status) && Object.keys(patch).some(key => key !== "meta")) return clone(task);
    const now = Date.now();
    const next: TaskRecord = {
      ...task,
      updatedAt: now,
    };
    if (patch.status !== undefined) {
      next.status = normalizeStatus(patch.status, task.status);
      if (FINAL_STATUSES.has(next.status)) {
        next.completedAt = now;
        next.aborted = next.status === "aborted" || next.status === "canceled";
      }
    }
    if (patch.progress !== undefined) next.progress = normalizeProgress(patch.progress);
    if (patch.meta !== undefined) next.meta = { ...objectOrEmpty(task.meta), ...objectOrEmpty(patch.meta) };
    if (patch.result !== undefined) next.result = patch.result;
    if (patch.error !== undefined) next.error = normalizeError(patch.error);
    if (patch.parentSessionPath !== undefined) next.parentSessionPath = patch.parentSessionPath || null;
    if (patch.parentSessionId !== undefined) next.parentSessionId = patch.parentSessionId || null;
    if (patch.parentSessionRef !== undefined) next.parentSessionRef = patch.parentSessionRef || null;
    if (patch.parentSessionPath !== undefined && patch.parentSessionId === undefined) {
      const parentRef = normalizeParentSessionRef(next, this._getSessionIdForPath);
      next.parentSessionId = parentRef.parentSessionId || null;
      next.parentSessionRef = parentRef.parentSessionRef || null;
    }
    if (patch.agentId !== undefined) next.agentId = patch.agentId || null;
    if (patch.pluginId !== undefined) next.pluginId = patch.pluginId || null;
    this._tasks.set(task.taskId, next);
    this._persist();
    return clone(next);
  }

  complete(taskId: string, result: unknown = null, options: AttemptOptions = {}) {
    const task = this._requireTask(taskId);
    if (isStaleAttempt(task, options.expectedAttempt)) return null;
    // 终态 first-write-wins：已完成/失败的 task 不接受再 complete/fail 改写；
    // 需要新执行走 register（合法复用，attempt+1）。取消直接写入其最终分类。
    if (FINAL_STATUSES.has(task.status)) return clone(task);
    const now = Date.now();
    const next: TaskRecord = {
      ...task,
      status: "completed",
      result,
      updatedAt: now,
      completedAt: now,
    };
    delete next.error;
    this._tasks.set(task.taskId, next);
    this._persist();
    return clone(next);
  }

  fail(taskId: string, error: unknown = "failed", options: AttemptOptions = {}) {
    const task = this._requireTask(taskId);
    if (isStaleAttempt(task, options.expectedAttempt)) return null;
    if (FINAL_STATUSES.has(task.status)) return clone(task);
    const now = Date.now();
    const next: TaskRecord = {
      ...task,
      status: "failed",
      error: normalizeError(error),
      updatedAt: now,
      completedAt: now,
    };
    this._tasks.set(task.taskId, next);
    this._persist();
    return clone(next);
  }

  cancel(taskId: string, reason = "canceled", options: AttemptOptions = {}) {
    const result = this._abort(taskId, reason, options, "canceled");
    return { result, canceled: result === "aborted" };
  }

  abort(taskId: string, reason = "aborted", options: AttemptOptions = {}) {
    return this._abort(taskId, reason, options, "aborted");
  }

  private _abort(taskId: string, reason: string, options: AttemptOptions, status: "aborted" | "canceled") {
    const task = this._tasks.get(taskId);
    if (!task) return "not_found";
    if (isStaleAttempt(task, options.expectedAttempt)) return "stale_attempt";
    if (FINAL_STATUSES.has(task.status)) return task.aborted ? "already_aborted" : "already_final";
    const handler = this._handlers.get(task.type);
    if (!handler) return "no_handler";
    task.aborted = true;
    task.status = status;
    task.updatedAt = Date.now();
    task.completedAt = task.updatedAt;
    task.error = normalizeError(reason);
    // 状态只表示已发起停止；执行器/外部动作是否停止由对应域负责证明。
    try { handler.abort(taskId); } catch (err) {
      log.error(`abort handler error for ${taskId}: ${normalizeError(err)}`);
    }
    this._persist();
    return "aborted";
  }

  abortByParentSession(parentSessionPath: string | { sessionId?: string; sessionPath?: string; parentSessionId?: string; parentSessionPath?: string }, reason = "parent session aborted") {
    const summary = {
      matched: 0,
      aborted: 0,
      alreadyAborted: 0,
      noHandler: 0,
      skippedFinal: 0,
    };
    if (!parentSessionPath) return summary;

    for (const task of this._tasks.values()) {
      if (!matchesParentSession(task, parentSessionPath, this._getSessionIdForPath)) continue;
      summary.matched++;
      if (FINAL_STATUSES.has(task.status)) {
        summary.skippedFinal++;
        continue;
      }
      const result = this.abort(task.taskId, reason, { expectedAttempt: task.attempt });
      if (result === "aborted") {
        summary.aborted++;
        continue;
      }
      if (result === "already_aborted") {
        summary.alreadyAborted++;
        continue;
      }
      if (result === "no_handler") {
        task.aborted = true;
        task.status = "aborted";
        task.updatedAt = Date.now();
        task.completedAt = task.updatedAt;
        task.error = normalizeError(reason);
        summary.noHandler++;
      }
    }
    if (summary.noHandler) this._persist();
    return summary;
  }

  remove(taskId: string, options: AttemptOptions = {}) {
    const task = this._tasks.get(taskId);
    if (!task || isStaleAttempt(task, options.expectedAttempt)) return false;
    this._tasks.delete(taskId);
    this._lastAttempts.set(taskId, task.attempt);
    if (this._lastAttempts.size > 1024) {
      const oldest = this._lastAttempts.keys().next().value;
      if (oldest !== undefined) this._lastAttempts.delete(oldest);
      this._attemptFloor = this._maxAttempt + 1;
    }
    this._persist();
    return true;
  }

  query(taskId: string) {
    const task = this._tasks.get(taskId);
    return task ? clone(task) : null;
  }

  listByType(type: string) {
    const result = [];
    for (const task of this._tasks.values()) {
      if (task.type === type) result.push(clone(task));
    }
    return result;
  }

  listAll(filter: { type?: string; status?: TaskStatus; pluginId?: string | null; parentSessionId?: string | null; sessionId?: string; parentSessionPath?: string | null } = {}) {
    const tasks = [...this._tasks.values()].filter((task) => {
      if (filter.type && task.type !== filter.type) return false;
      if (filter.status && task.status !== filter.status) return false;
      if (filter.pluginId && task.pluginId !== filter.pluginId) return false;
      if ((filter.parentSessionId || filter.sessionId) && !matchesParentSession(task, filter, this._getSessionIdForPath)) return false;
      if (filter.parentSessionPath && !matchesParentSession(task, filter, this._getSessionIdForPath)) return false;
      return true;
    });
    return tasks.map(clone);
  }

  /** 该会话是否还有未到终态的后台任务（循环守恒检查与闹钟护栏共用）。 */
  hasActiveForParentSession(parentSessionPath: string) {
    if (!parentSessionPath) return false;
    return this.listAll({ parentSessionPath })
      .some((task) => ACTIVE_STATUSES.has(task.status));
  }

  // ── 计划任务 ──

  schedule(scheduleId: string, input: any = {}) {
    const id = assertText(scheduleId, "scheduleId");
    const type = assertText(input.type, "schedule type");
    const existing = this._schedules.get(id);
    const now = Date.now();
    const intervalMs = input.intervalMs === undefined ? existing?.intervalMs : normalizePositiveNumber(input.intervalMs, "intervalMs");
    const runAt = input.runAt === undefined ? existing?.runAt : normalizeOptionalTime(input.runAt, "runAt");
    if (!intervalMs && !runAt) {
      throw new Error("TaskRegistry: schedule requires intervalMs or runAt");
    }
    const enabled = input.enabled === undefined ? existing?.enabled !== false : Boolean(input.enabled);
    const nextRunAt = enabled ? resolveNextRunAt({ intervalMs, runAt, existing, now }) : null;
    const schedule = {
      scheduleId: id,
      type,
      pluginId: input.pluginId || existing?.pluginId || null,
      agentId: input.agentId || existing?.agentId || null,
      parentSessionPath: input.parentSessionPath || existing?.parentSessionPath || null,
      payload: input.payload === undefined ? clone(existing?.payload || {}) : clone(input.payload),
      meta: input.meta === undefined ? clone(existing?.meta || {}) : objectOrEmpty(input.meta),
      intervalMs: intervalMs || null,
      runAt: runAt || null,
      enabled,
      nextRunAt,
      lastRunAt: existing?.lastRunAt || null,
      lastResult: existing?.lastResult,
      lastError: existing?.lastError || null,
      runCount: existing?.runCount || 0,
      createdAt: existing?.createdAt || now,
      updatedAt: now,
    };
    this._schedules.set(id, schedule);
    this._persist();
    this._armSchedule(id);
    return clone(schedule);
  }

  unschedule(scheduleId: string) {
    const id = assertText(scheduleId, "scheduleId");
    this._clearScheduleTimer(id);
    const deleted = this._schedules.delete(id);
    this._persist();
    return deleted;
  }

  querySchedule(scheduleId: string) {
    const schedule = this._schedules.get(scheduleId);
    return schedule ? clone(schedule) : null;
  }

  listSchedules( filter: any = {}) {
    return [...this._schedules.values()]
      .filter((schedule) => {
        if (filter.type && schedule.type !== filter.type) return false;
        if (filter.pluginId && schedule.pluginId !== filter.pluginId) return false;
        if (filter.enabled !== undefined && schedule.enabled !== Boolean(filter.enabled)) return false;
        return true;
      })
      .map(clone);
  }

  clearTimers() {
    for (const scheduleId of this._scheduleTimers.keys()) {
      this._clearScheduleTimer(scheduleId);
    }
  }

  _requireTask(taskId: string) {
    const task = this._tasks.get(taskId);
    if (!task) throw new Error(`TaskRegistry: task "${taskId}" not found`);
    return task;
  }

  _armSchedulesForType(type: string) {
    for (const schedule of this._schedules.values()) {
      if (schedule.type === type) this._armSchedule(schedule.scheduleId);
    }
  }

  _armSchedule(scheduleId: string) {
    this._clearScheduleTimer(scheduleId);
    const schedule = this._schedules.get(scheduleId);
    if (!schedule?.enabled || !schedule.nextRunAt) return;
    const delay = Math.max(0, Math.min(MAX_TIMER_DELAY, schedule.nextRunAt - Date.now()));
    const timer = setTimeout(() => {
      this._scheduleTimers.delete(scheduleId);
      this._runSchedule(scheduleId).catch((err) => {
        log.error(`schedule ${scheduleId} failed: ${normalizeError(err)}`);
      });
    }, delay);
    if (typeof timer.unref === "function") timer.unref();
    this._scheduleTimers.set(scheduleId, timer);
  }

  _clearScheduleTimer(scheduleId: string) {
    const timer = this._scheduleTimers.get(scheduleId);
    if (timer) clearTimeout(timer);
    this._scheduleTimers.delete(scheduleId);
  }

  async _runSchedule(scheduleId: string) {
    const schedule = this._schedules.get(scheduleId);
    if (!schedule?.enabled) return;
    const handler = this._handlers.get(schedule.type);
    if (!handler?.run) {
      schedule.lastError = `No schedule runner for type "${schedule.type}"`;
      schedule.updatedAt = Date.now();
      this._persist();
      return;
    }

    const now = Date.now();
    try {
      const result = await handler.run(clone(schedule));
      schedule.lastRunAt = now;
      schedule.lastResult = result ?? null;
      schedule.lastError = null;
      schedule.runCount = (schedule.runCount || 0) + 1;
      if (schedule.intervalMs) {
        schedule.nextRunAt = now + schedule.intervalMs;
      } else {
        schedule.enabled = false;
        schedule.nextRunAt = null;
      }
    } catch (err) {
      schedule.lastRunAt = now;
      schedule.lastError = normalizeError(err);
      if (schedule.intervalMs) {
        schedule.nextRunAt = now + schedule.intervalMs;
      } else {
        schedule.enabled = false;
        schedule.nextRunAt = null;
      }
    } finally {
      schedule.updatedAt = Date.now();
      this._schedules.set(scheduleId, schedule);
      this._persist();
      this._armSchedule(scheduleId);
    }
  }

  _loadPersisted() {
    if (!this._persistencePath || !fs.existsSync(this._persistencePath)) return;
    try {
      const raw = JSON.parse(fs.readFileSync(this._persistencePath, "utf8"));
      for (const task of Array.isArray(raw.tasks) ? raw.tasks : []) {
        if (!task?.taskId || !task?.type) continue;
        const restored = { ...task, attempt: taskAttempt(task) };
        if (ACTIVE_STATUSES.has(restored.status)) {
          restored.status = "recovering";
          restored.updatedAt = Date.now();
        }
        this._tasks.set(restored.taskId, restored);
        this._maxAttempt = Math.max(this._maxAttempt, restored.attempt);
      }
      for (const schedule of Array.isArray(raw.schedules) ? raw.schedules : []) {
        if (!schedule?.scheduleId || !schedule?.type) continue;
        this._schedules.set(schedule.scheduleId, { ...schedule });
        this._armSchedule(schedule.scheduleId);
      }
    } catch (err) {
      log.warn(`failed to load persisted tasks: ${normalizeError(err)}`);
    }
  }

  _persist() {
    if (!this._persistencePath) return;
    try {
      fs.mkdirSync(path.dirname(this._persistencePath), { recursive: true });
      const tasks = [...this._tasks.values()]
        .filter((task) => task.persist !== false)
        .map(stripRuntimeTaskFields);
      const schedules = [...this._schedules.values()];
      atomicWriteSync(this._persistencePath, JSON.stringify({ tasks, schedules }, null, 2));
      this._persistence = { durable: true, status: "saved", error: null };
    } catch (err) {
      this._persistence = { durable: false, status: "failed", error: normalizeError(err) };
      log.warn(`failed to persist tasks: ${normalizeError(err)}`);
    }
  }
}

function assertText(value: unknown, label: string) {
  const text = typeof value === "string" ? value.trim() : "";
  if (!text) throw new Error(`TaskRegistry: ${label} is required`);
  return text;
}

/** attempt 归一化：缺失/非法的旧记录按 1 读（旧值缺失不可猜成其他身份）。 */
function taskAttempt(task: { attempt?: number } | null | undefined) {
  return typeof task?.attempt === "number" && Number.isSafeInteger(task.attempt) && task.attempt > 0 ? task.attempt : 1;
}

/**
 * 迟到回调栅栏：调用方在启动执行时从 register() 返回值捕获 attempt，终态时
 * 以 expectedAttempt 回传。attempt 不匹配 → 拒绝写入（返回 true 表示 stale）。
 */
function isStaleAttempt(task: TaskRecord, expectedAttempt: number | null | undefined) {
  if (expectedAttempt === undefined) return taskAttempt(task) !== 1;
  if (expectedAttempt === null) return true;
  if (!Number.isSafeInteger(expectedAttempt) || expectedAttempt < 1) {
    throw new Error(`TaskRegistry: expectedAttempt must be a positive integer (got ${expectedAttempt})`);
  }
  return taskAttempt(task) !== expectedAttempt;
}

function normalizeStatus(value: unknown, fallback: TaskStatus): TaskStatus {
  const status = typeof value === "string" ? value.trim() : "";
  if (!status) return fallback;
  if (!KNOWN_STATUSES.has(status as TaskStatus)) {
    throw new Error(`TaskRegistry: unknown task status "${status}"`);
  }
  return status as TaskStatus;
}

function normalizeProgress(value: unknown) {
  if (value === null) return null;
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("TaskRegistry: progress must be an object or null");
  }
  const record = value as Record<string, unknown>;
  const current = normalizeOptionalNumber(record.current, "progress.current");
  const total = normalizeOptionalNumber(record.total, "progress.total");
  const percent = record.percent !== undefined
    ? normalizeOptionalNumber(record.percent, "progress.percent")
    : derivePercent(current, total);
  return {
    ...(current !== undefined ? { current } : {}),
    ...(total !== undefined ? { total } : {}),
    ...(percent !== undefined ? { percent } : {}),
    ...(typeof record.message === "string" ? { message: record.message } : {}),
  };
}

function normalizeOptionalNumber(value: unknown, label: string) {
  if (value === undefined || value === null || value === "") return undefined;
  const number = Number(value);
  if (!Number.isFinite(number)) throw new Error(`TaskRegistry: ${label} must be a finite number`);
  return number;
}

function normalizePositiveNumber(value: unknown, label: string) {
  if (value === undefined || value === null || value === "") return null;
  const number = Number(value);
  if (!Number.isFinite(number) || number <= 0) {
    throw new Error(`TaskRegistry: ${label} must be a positive number`);
  }
  return number;
}

function normalizeOptionalTime(value: unknown, label: string) {
  if (value === undefined || value === null || value === "") return null;
  if (value instanceof Date) return value.getTime();
  const number = typeof value === "number" ? value : Date.parse(String(value));
  if (!Number.isFinite(number)) throw new Error(`TaskRegistry: ${label} must be a valid time`);
  return number;
}

function derivePercent(current: number | undefined, total: number | undefined) {
  if (current === undefined || total === undefined || total <= 0) return undefined;
  return Math.max(0, Math.min(100, Math.round((current / total) * 100)));
}

function normalizeError(error: unknown) {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "message" in error && typeof error.message === "string") return error.message;
  return String(error);
}

function objectOrEmpty(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? { ...value } : {};
}

function clone<T>(value: T): T {
  return structuredClone(value);
}

function stripRuntimeTaskFields(task: TaskRecord) {
  const { persist: _persist, ...rest } = task;
  return rest;
}

function resolveNextRunAt({ intervalMs, runAt, existing, now }: { intervalMs: number | null; runAt: number | null; existing?: {nextRunAt?: number}; now: number }) {
  if (existing?.nextRunAt && existing.nextRunAt > now) return existing.nextRunAt;
  if (runAt) return runAt;
  return now + (intervalMs || 0);
}
