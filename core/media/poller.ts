/**
 * core/media/poller.ts
 *
 * Background poller with age-based smart intervals.
 * Every 5 s the poller ticks; how often each task is actually queried
 * depends on how old the submission is.
 *
 * Key difference from the dreamina poller: instead of calling runCli("query_result")
 * directly, this routes through the adapter registry — adapter.query(taskId, ctx).
 * Also supports "fake-async" detection: if the task already has files when polled
 * (e.g. a synchronous adapter populated files at submit time), it skips the query
 * and marks the task successful immediately.
 */

import { dirname, join as pathJoin } from "node:path";
import { readImageSize } from "./image-size.ts";
import { isResponseDelivery } from "./image-task-runner.ts";
import { isMediaTaskTerminal, mediaTaskAttempt, validateMediaOutputs } from "./task-store.ts";
// Control-plane poller：媒体任务查询不是 Model Call（§四十七），不使用
// model-request-accounting。usageLedger 构造参数保留以兼容既有装配，
// 但 query 不再写模型用量账本。

const TICK_MS = 5_000;
const TWO_MINUTES = 2 * 60 * 1000;
const TEN_MINUTES = 10 * 60 * 1000;
const MAX_CONSECUTIVE_ERRORS = 5;

function callLogger(logger, level, args, fallbackLevel = null) {
  const fn = typeof logger?.[level] === "function"
    ? logger[level]
    : fallbackLevel && typeof logger?.[fallbackLevel] === "function"
      ? logger[fallbackLevel]
      : null;
  if (!fn) return;
  try {
    fn.call(logger, ...args);
  } catch {
    // Logging must never break media task recovery, cancellation, or polling.
  }
}

function createSafeLogger(logger) {
  return {
    log: (...args) => callLogger(logger, "log", args, "info"),
    info: (...args) => callLogger(logger, "info", args, "log"),
    warn: (...args) => callLogger(logger, "warn", args),
    error: (...args) => callLogger(logger, "error", args),
  };
}

/**
 * Decide whether this tick should trigger a real adapter query for a task.
 *
 * @param {number} ageMs     Milliseconds since task was created
 * @param {number} tickCount Monotonically-increasing tick counter (starts at 1)
 * @returns {boolean}
 */
export function shouldCheckThisTick(ageMs, tickCount) {
  if (ageMs < TWO_MINUTES) return true;               // < 2 min: every tick
  if (ageMs < TEN_MINUTES) return tickCount % 3 === 0; // 2-10 min: every 3rd
  return tickCount % 6 === 0;                           // 10 min+: every 6th
}

export class Poller {
  _active = new Set<string>();
  _deliveryPending = new Set<string>();
  _errorCounts = new Map<string, number>();
  _inFlight = new Map<string, Promise<void>>();
  _queryControllers = new Map<string, { taskId: string; attempt: number; controller: AbortController }>();
  _handoffs = new Map<string, Promise<void>>();
  _submissions = new Map<string, { taskId: string; attempt: number; controller: AbortController; promise: Promise<any> }>();
  _timer: ReturnType<typeof setInterval> | null = null;
  _tickCount = 0;
  _generation = 0;
  _controller = new AbortController();
  _started = false;
  _emitted = new Set<string>();
  _store: any;
  _registry: any;
  _bus: any;
  _dataDir: string;
  _generatedDir: string;
  _log: any;
  _registerSessionFile: any;
  _usageLedger: any;

  constructor({ store, registry, bus, dataDir, generatedDir, log, registerSessionFile, usageLedger = null }) {
    this._store = store;
    this._registry = registry;
    this._bus = bus;
    this._dataDir = dataDir || dirname(generatedDir);
    this._generatedDir = generatedDir;
    this._log = createSafeLogger(log);
    this._registerSessionFile = registerSessionFile || null;
    this._usageLedger = usageLedger;
  }

  get running() { return this._started; }

  executionContext(signal?: AbortSignal) {
    const generation = this._generation;
    return {
      signal: signal ? AbortSignal.any([signal, this._controller.signal]) : this._controller.signal,
      isCurrent: () => this._started && this._generation === generation,
    };
  }

  /** 背景提交也由同一个运行实例持有；取消与停止后仍等待其真实清理完成。 */
  runSubmission(taskId, attempt, run, signal?: AbortSignal) {
    const controller = new AbortController();
    const lifecycle = this.executionContext(signal);
    const combined = AbortSignal.any([lifecycle.signal, controller.signal]);
    const key = `${taskId}:${attempt}`;
    const context = {
      signal: combined,
      isCurrent: () => lifecycle.isCurrent()
        && !combined.aborted && this._isPendingAttempt(taskId, attempt),
    };
    const promise = Promise.resolve().then(() => {
      combined.throwIfAborted();
      if (!context.isCurrent()) throw new Error("media submission is no longer current");
      return run(context);
    }).finally(() => {
      if (this._submissions.get(key)?.promise === promise) this._submissions.delete(key);
    });
    this._submissions.set(key, { taskId, attempt, controller, promise });
    return promise;
  }

  add(taskId) {
    if (!this._started) return;
    this._errorCounts.delete(taskId);
    const task = this._store.get(taskId);
    if (task?.status === "pending") this._active.add(taskId);
    else if (task?.deliveryState === "pending") this._deliveryPending.add(taskId);
  }

  hasPending(taskId) { return this._active.has(taskId); }

  cancel(taskId) {
    const task = this._store.get(taskId);
    if (!task || task.status !== "pending") return Promise.resolve();
    const attempt = mediaTaskAttempt(task);
    for (const submission of this._submissions.values()) {
      if (submission.taskId === taskId && submission.attempt === attempt) {
        submission.controller.abort(new DOMException("media task cancelled", "AbortError"));
      }
    }
    for (const query of this._queryControllers.values()) {
      if (query.taskId === taskId && query.attempt === attempt) query.controller.abort(new DOMException("media task cancelled", "AbortError"));
    }
    this._settle(taskId, task, { status: "cancelled", failReason: "user cancelled" });
    this._active.delete(taskId);
    this._errorCounts.delete(taskId);
    this._log.info(`[media] task ${taskId} cancelled by user`);
    const handoff = this._deliverTask(taskId);
    return Promise.allSettled([
      handoff,
      ...[...this._queryControllers.entries()].filter(([, item]) => item.taskId === taskId).map(([key]) => this._inFlight.get(key)),
      ...[...this._submissions.values()].filter(item => item.taskId === taskId).map(item => item.promise),
    ]).then(() => {});
  }

  start() {
    if (this._started) return;
    this._generation += 1;
    this._controller = new AbortController();
    this._started = true;
    this._active.clear();
    this._deliveryPending.clear();
    const tasks = this._store.listAll?.() || this._store.listPending();
    for (const task of tasks) {
      if (task.status === "pending") {
        if (task.submitState === "submitting" && !task.adapterTaskId && !(task.files?.length)) {
          this._settle(task.taskId, task, {
            status: "failed",
            failReason: "generation interrupted during submission; provider acceptance is unknown and generation was not retried",
          });
        } else {
          this._active.add(task.taskId);
          if (!isResponseDelivery(task)) {
            void this._registerDeferred(task).catch(error => this._log.warn(`[media] recovery registration failed: ${error.message}`));
            void this._bus.request("task:register", {
              taskId: task.taskId, type: "media-generation",
              sessionId: task.sessionId, sessionRef: task.sessionRef, parentSessionPath: task.sessionPath,
              meta: this._deferredMeta(task),
            }).catch(error => this._log.warn(`[media] task visibility recovery failed: ${error.message}`));
          }
        }
      }
      const latest = this._store.get(task.taskId);
      if (isMediaTaskTerminal(latest) && latest.deliveryState === "pending") this._deliveryPending.add(task.taskId);
    }
    if (tasks.length) this._log.info(`[media] poller recovered ${tasks.filter(task => task.status === "pending").length} pending task(s)`);
    this._timer = setInterval(() => this._tick(), TICK_MS);
  }

  /** 同步使旧代次失效，返回值等待已拥有的在途工作回收。 */
  stop() {
    this._started = false;
    this._generation += 1;
    this._controller.abort(new DOMException("media runtime stopped", "AbortError"));
    if (this._timer !== null) clearInterval(this._timer);
    this._timer = null;
    this._active.clear();
    this._deliveryPending.clear();
    return Promise.allSettled([
      ...this._inFlight.values(), ...this._handoffs.values(),
      ...[...this._submissions.values()].map(item => item.promise),
    ]).then(() => {});
  }

  checkNow(taskId) {
    if (!this._started) return Promise.resolve();
    const task = this._store.get(taskId);
    if (isMediaTaskTerminal(task)) return this._deliverTask(taskId);
    if (!task || !this._active.has(taskId) || task.status !== "pending") return Promise.resolve();
    const key = `${this._generation}:${taskId}:${mediaTaskAttempt(task)}`;
    const existing = this._inFlight.get(key);
    if (existing) return existing;
    const controller = new AbortController();
    const signal = AbortSignal.any([controller.signal, this._controller.signal]);
    this._queryControllers.set(key, { taskId, attempt: mediaTaskAttempt(task), controller });
    const promise = this._checkTask(taskId, task, signal).catch(error => {
      this._log.error(`[media] check failed for ${taskId}:`, error);
    }).finally(() => {
      if (this._inFlight.get(key) === promise) this._inFlight.delete(key);
      this._queryControllers.delete(key);
    });
    this._inFlight.set(key, promise);
    return promise;
  }

  _isPendingAttempt(taskId, attempt) {
    const task = this._store.get(taskId);
    return !!task && task.status === "pending" && mediaTaskAttempt(task) === attempt;
  }

  _isCurrent(taskId, attempt, generation, requirePending = true) {
    const task = this._store.get(taskId);
    return this._started && this._generation === generation && !!task
      && mediaTaskAttempt(task) === attempt && (!requirePending || task.status === "pending");
  }

  async _readImageDimensions(files) {
    if (!files?.length) return { imageWidth: null, imageHeight: null };
    const size = await readImageSize(pathJoin(this._generatedDir, files[0])).catch(() => null);
    return size ? { imageWidth: (size as any).width, imageHeight: (size as any).height }
      : { imageWidth: null, imageHeight: null };
  }

  _registerGeneratedFiles(task, files) {
    if (isResponseDelivery(task)) return [];
    const sessionId = task.sessionId || task.sessionRef?.sessionId || null;
    const sessionPath = task.sessionPath || task.sessionRef?.sessionPath || null;
    const sessionRef = task.sessionRef || (sessionId ? { sessionId, ...(sessionPath ? { sessionPath } : {}) } : null);
    if (!this._registerSessionFile || (!sessionId && !sessionPath)) return task.sessionFiles || [];
    const sessionFiles = [...(task.sessionFiles || [])];
    for (const file of files) {
      const filePath = pathJoin(this._generatedDir, file);
      if (sessionFiles.some(item => item?.filePath === filePath || item?.realPath === filePath)) continue;
      const registered = this._registerSessionFile({
        ...(sessionId ? { sessionId } : {}), ...(sessionPath ? { sessionPath } : {}), ...(sessionRef ? { sessionRef } : {}),
        filePath, label: file, origin: "plugin_output", storageKind: "plugin_data",
      });
      if (!registered) throw new Error(`media output registration returned no file: ${file}`);
      sessionFiles.push(registered);
      // 多文件部分登记后失败时保留已登记身份，下一次只补缺失的文件。
      this._store.update(task.taskId, { sessionFiles: [...sessionFiles] });
      this._store.requireFlush?.();
    }
    return sessionFiles;
  }

  _deferredMeta(task) {
    const kind = task.type === "video" ? "video" : task.type === "speech" ? "speech" : "image";
    return {
      type: `${kind}-generation`, mediaKind: kind, mediaAttempt: mediaTaskAttempt(task),
      deliveryIntent: "ui_only", triggerParentTurn: false,
      ...(kind === "image" ? { notifyAgentOnFailure: true } : {}),
      prompt: task.prompt,
      ...(task.deliveryTarget ? { deliveryTarget: task.deliveryTarget } : {}),
      ...(task.metadata ? { metadata: task.metadata } : {}),
    };
  }

  async _registerDeferred(task) {
    const existing = await this._bus.request("deferred:query", { taskId: task.taskId });
    const attempt = mediaTaskAttempt(task);
    if (existing?.status && (existing.meta?.mediaAttempt ?? 1) > attempt) throw new Error("stale media handoff attempt");
    const operation = existing?.status && (existing.meta?.mediaAttempt ?? 1) < attempt ? "deferred:retry" : "deferred:register";
    const result = await this._bus.request(operation, {
      taskId: task.taskId, sessionId: task.sessionId, sessionPath: task.sessionPath,
      sessionRef: task.sessionRef, meta: this._deferredMeta(task), durable: true,
    });
    if (result?.ok !== true || result?.durable !== true) throw new Error(result?.error || "media handoff registration is not durable");
  }

  _settle(taskId, task, result) {
    try {
      return this._store.settleTask(taskId, {
        ...result, expectedAttempt: mediaTaskAttempt(task), generatedDir: this._generatedDir,
      });
    } finally {
      const latest = this._store.get(taskId);
      if (isMediaTaskTerminal(latest) && latest.deliveryState === "pending") this._deliveryPending.add(taskId);
    }
  }

  _emitTaskDone(task, files, dims, sessionFiles) {
    const latest = this._store.get(task.taskId) || task;
    this._bus.emit({
      type: "media-gen:task-done", taskId: task.taskId, batchId: task.batchId || null,
      kind: task.type === "video" ? "video" : task.type === "speech" ? "speech" : "image",
      files, generatedDir: this._generatedDir, sessionFiles,
      imageWidth: dims?.imageWidth ?? latest.imageWidth ?? null, imageHeight: dims?.imageHeight ?? latest.imageHeight ?? null,
      providerId: latest.providerId || null, modelId: latest.modelId || null, protocolId: latest.protocolId || null,
      metadata: latest.metadata || null, task: latest,
      ...(latest.sessionId ? { sessionId: latest.sessionId, sessionRef: latest.sessionRef || null } : {}),
    }, latest.sessionPath || null);
  }

  _deliverTask(taskId) {
    const task = this._store.get(taskId);
    if (!this._started || !isMediaTaskTerminal(task) || task.deliveryState !== "pending") return Promise.resolve();
    const attempt = mediaTaskAttempt(task);
    const generation = this._generation;
    const key = `${generation}:${taskId}:${attempt}`;
    const existing = this._handoffs.get(key);
    if (existing) return existing;
    this._deliveryPending.add(taskId);
    const current = () => this._isCurrent(taskId, attempt, generation, false);
    const promise = (async () => {
      try {
        if (!current()) return;
        this._store.requireFlush?.();
        const files = task.files || [];
        const sessionFiles = task.status === "done" ? this._registerGeneratedFiles(task, files) : [];
        await this._registerDeferred(task);
        if (!current()) return;
        const operation = task.status === "done" ? "deferred:resolve"
          : task.status === "cancelled" || task.status === "aborted" ? "deferred:abort" : "deferred:fail";
        const receipt = await this._bus.request(operation, {
          taskId, expectedAttempt: attempt, durable: true,
          ...(task.status === "done" ? { files, ...(sessionFiles.length ? { sessionFiles } : {}) }
            : { reason: task.failReason, error: { message: task.failReason } }),
        });
        if (receipt?.ok !== true || receipt?.durable !== true) throw new Error(receipt?.error || "media result handoff is not durable");
        if (!current()) return;
        const emissionKey = `${taskId}:${attempt}`;
        if (task.status === "done" && !this._emitted.has(emissionKey)) {
          this._emitTaskDone(task, files, task, sessionFiles);
          this._emitted.add(emissionKey);
        }
        this._store.markDeliveryHandedOff(taskId, attempt);
        this._deliveryPending.delete(taskId);
        await this._bus.request("task:remove", { taskId });
      } catch (error) {
        this._log.warn(`[media] result delivery pending for ${taskId}:`, error?.message || error);
      }
    })().finally(() => {
      if (this._handoffs.get(key) === promise) this._handoffs.delete(key);
    });
    this._handoffs.set(key, promise);
    return promise;
  }

  _tick() {
    this._tickCount += 1;
    for (const taskId of [...this._active]) {
      const task = this._store.get(taskId);
      if (!task || task.status !== "pending") { this._active.delete(taskId); continue; }
      const ageMs = Date.now() - new Date(task.createdAt).getTime();
      if (shouldCheckThisTick(ageMs, this._tickCount)) void this.checkNow(taskId);
    }
    for (const taskId of [...this._deliveryPending]) void this._deliverTask(taskId);
  }

  async _checkTask(taskId, task, signal = this._controller.signal) {
    const attempt = mediaTaskAttempt(task);
    const generation = this._generation;
    const current = () => this._isCurrent(taskId, attempt, generation);
    if (!current()) return;
    let result;
    if (task.files?.length) result = { status: "success", files: task.files };
    else {
      if (task.submitState === "submitting" && !task.adapterTaskId) return;
      const adapter = (task.protocolId && this._registry.getProtocol?.(task.protocolId))
        || this._registry.get(task.adapterId) || this._registry.get(task.providerId);
      if (!adapter?.query) {
        result = { status: "failed", failReason: `no query adapter registered for "${task.adapterId}"` };
      } else {
        const base = { dataDir: this._dataDir, generatedDir: this._generatedDir, bus: this._bus,
          log: this._log, task, signal };
        const context = this._registry.createSubmitContextForAdapter?.(adapter, base) || base;
        try {
          result = await adapter.query(task.adapterTaskId || taskId, context);
          if (!current()) return;
          this._errorCounts.delete(taskId);
        } catch (error) {
          if (!current()) return;
          const count = (this._errorCounts.get(taskId) || 0) + 1;
          this._errorCounts.set(taskId, count);
          if (count < MAX_CONSECUTIVE_ERRORS) {
            this._log.warn(`[media] query ${taskId} failed (${count}/${MAX_CONSECUTIVE_ERRORS}), will retry: ${error?.message ?? error}`);
            return;
          }
          result = { status: "failed", failReason: error?.message || String(error) };
        }
      }
    }
    if (!current()) return;
    if (result?.status === "success" || result?.status === "done") {
      const files = result.files || [];
      const validation = validateMediaOutputs(files, this._generatedDir);
      if (validation.ok === false) {
        this._settle(taskId, task, { status: "failed", failReason: validation.error });
        this._active.delete(taskId);
        await this._deliverTask(taskId);
        return;
      }
      const dimensions = await this._readImageDimensions(files);
      if (!current()) return;
      this._settle(taskId, task, { status: "done", files, ...dimensions });
    } else if (result?.status === "failed") {
      this._settle(taskId, task, { status: "failed", failReason: result.failReason || result.error?.message || "generation failed" });
    } else return;
    this._active.delete(taskId);
    this._errorCounts.delete(taskId);
    const completed = this._store.get(taskId);
    if (isResponseDelivery(completed)) {
      if (completed?.status === "done") this._emitTaskDone(completed, completed.files, completed, []);
      return;
    }
    await this._deliverTask(taskId);
  }
}
