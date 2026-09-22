import type { TaskRegistry, TaskHandler, TaskPatch, AttemptOptions } from "../lib/task-registry.ts";
interface TaskBusPayload extends TaskPatch, AttemptOptions {
  taskId: string; type: string; scheduleId: string; abort: TaskHandler["abort"]; run?: TaskHandler["run"];
  reason?: string; parentSessionId?: string | null; sessionId?: string; sessionRef?: unknown;
  legacySessionPath?: string; persist?: boolean;
}
interface TaskBus { handle(topic: string, handler: (payload: TaskBusPayload) => unknown): unknown }
export function registerTaskRegistryBusHandlers(eventBus: TaskBus, taskRegistry: TaskRegistry) {
  eventBus.handle("task:register-handler", ({ type, abort, run }) => {
    const handler: TaskHandler = { abort };
    if (run !== undefined) handler.run = run;
    taskRegistry.registerHandler(type, handler);
    return { ok: true };
  });
  eventBus.handle("task:unregister-handler", ({ type }) => {
    taskRegistry.unregisterHandler(type);
    return { ok: true };
  });
  eventBus.handle("task:register", ({ taskId, type, parentSessionPath, parentSessionId, parentSessionRef, sessionId, sessionRef, legacySessionPath, meta, pluginId, agentId, persist }) => {
    const task = taskRegistry.register(taskId, {
      type,
      parentSessionPath,
      parentSessionId,
      parentSessionRef,
      sessionId,
      sessionRef,
      legacySessionPath,
      meta,
      pluginId,
      agentId,
      persist,
    });
    // 返回含 attempt 的任务快照：调用方捕获本次执行的 attempt，终态回传
    // expectedAttempt 以启用迟到回调栅栏（P02-T01/A03）。
    return { ok: true, task, persistence: taskRegistry.persistenceStatus(task) };
  });
  eventBus.handle("task:update", ({ taskId, expectedAttempt, ...patch }) => {
    const task = taskRegistry.update(taskId, patch, { expectedAttempt });
    return { ok: task !== null, task, persistence: taskRegistry.persistenceStatus(task) };
  });
  eventBus.handle("task:complete", ({ taskId, result, expectedAttempt }) => {
    const task = taskRegistry.complete(taskId, result, { expectedAttempt });
    return { ok: task !== null, task, persistence: taskRegistry.persistenceStatus(task) };
  });
  eventBus.handle("task:fail", ({ taskId, reason, error, expectedAttempt }) => {
    const task = taskRegistry.fail(taskId, reason ?? error, { expectedAttempt });
    return { ok: task !== null, task, persistence: taskRegistry.persistenceStatus(task) };
  });
  eventBus.handle("task:remove", ({ taskId, expectedAttempt }) => {
    const removed = taskRegistry.remove(taskId, { expectedAttempt });
    return { ok: removed, removed, persistence: taskRegistry.persistenceStatus() };
  });
  eventBus.handle("task:query", ({ taskId }) => {
    return taskRegistry.query(taskId);
  });
  eventBus.handle("task:list", (filter) => {
    return taskRegistry.listAll(filter);
  });
  eventBus.handle("task:abort", ({ taskId, expectedAttempt }) => {
    return { result: taskRegistry.abort(taskId, "aborted", { expectedAttempt }) };
  });
  eventBus.handle("task:cancel", ({ taskId, reason, expectedAttempt }) => {
    return taskRegistry.cancel(taskId, reason, { expectedAttempt });
  });
  eventBus.handle("task:schedule", ({ scheduleId, ...input }) => {
    return { ok: true, schedule: taskRegistry.schedule(scheduleId, input) };
  });
  eventBus.handle("task:unschedule", ({ scheduleId }) => {
    return { ok: true, removed: taskRegistry.unschedule(scheduleId) };
  });
  eventBus.handle("task:list-schedules", (filter) => {
    return taskRegistry.listSchedules(filter);
  });
}
