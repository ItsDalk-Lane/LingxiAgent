export function registerTaskRegistryBusHandlers(eventBus, taskRegistry) {
  eventBus.handle("task:register-handler", ({ type, abort, run }) => {
    const handler: { abort: any; run?: any } = { abort };
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
    return { ok: true, task };
  });
  eventBus.handle("task:update", ({ taskId, expectedAttempt, ...patch }) => {
    return { ok: true, task: taskRegistry.update(taskId, patch, { expectedAttempt }) };
  });
  eventBus.handle("task:complete", ({ taskId, result, expectedAttempt }) => {
    return { ok: true, task: taskRegistry.complete(taskId, result, { expectedAttempt }) };
  });
  eventBus.handle("task:fail", ({ taskId, reason, error, expectedAttempt }) => {
    return { ok: true, task: taskRegistry.fail(taskId, reason ?? error, { expectedAttempt }) };
  });
  eventBus.handle("task:remove", ({ taskId }) => {
    taskRegistry.remove(taskId);
    return { ok: true };
  });
  eventBus.handle("task:query", ({ taskId }) => {
    return taskRegistry.query(taskId);
  });
  eventBus.handle("task:list", (filter = {}) => {
    return taskRegistry.listAll(filter);
  });
  eventBus.handle("task:abort", ({ taskId }) => {
    return { result: taskRegistry.abort(taskId) };
  });
  eventBus.handle("task:cancel", ({ taskId, reason }) => {
    return taskRegistry.cancel(taskId, reason);
  });
  eventBus.handle("task:schedule", ({ scheduleId, ...input }) => {
    return { ok: true, schedule: taskRegistry.schedule(scheduleId, input) };
  });
  eventBus.handle("task:unschedule", ({ scheduleId }) => {
    return { ok: true, removed: taskRegistry.unschedule(scheduleId) };
  });
  eventBus.handle("task:list-schedules", (filter = {}) => {
    return taskRegistry.listSchedules(filter);
  });
}
