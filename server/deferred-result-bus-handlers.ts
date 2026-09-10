import { normalizeDeferredResolveResult } from "../lib/deferred-result-payload.ts";

function textOrNull(value) {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function sessionRefPayload(payload: any = {}) {
  const rawRef = payload.sessionRef && typeof payload.sessionRef === "object"
    ? payload.sessionRef
    : null;
  const sessionId = textOrNull(payload.sessionId) || textOrNull(rawRef?.sessionId);
  const sessionPath =
    textOrNull(payload.sessionPath)
    || textOrNull(rawRef?.sessionPath)
    || textOrNull(rawRef?.path);
  const legacySessionPath =
    textOrNull(payload.legacySessionPath)
    || textOrNull(rawRef?.legacySessionPath)
    || (sessionId && sessionPath ? sessionPath : null);
  const sessionRef = sessionId
    ? {
      sessionId,
      ...(sessionPath ? { sessionPath } : {}),
      ...(legacySessionPath ? { legacySessionPath } : {}),
    }
    : null;
  return { sessionId, sessionPath, sessionRef };
}

function requireDeferredTarget(payload: any = {}) {
  const target = sessionRefPayload(payload);
  if (!target.sessionId && !target.sessionPath) {
    return { ok: false, error: "sessionId or sessionPath is required" };
  }
  return target;
}

function isDeferredTargetError(target): target is { ok: false; error: string } {
  return target?.ok === false;
}

export function registerDeferredResultBusHandlers(eventBus, deferredResultStore) {
  const durableResult = (durable, result: any = { ok: true }) => {
    if (!durable) return result;
    return deferredResultStore.flushSync() === true
      ? { ...result, durable: true }
      : { ok: false, durable: false, error: "deferred result persistence failed" };
  };
  const checkAttempt = (taskId, expectedAttempt, terminalStatus) => {
    const task = deferredResultStore.query(taskId);
    if (!task) return { ok: false, error: "deferred task not registered" };
    if (expectedAttempt !== undefined && (task.meta?.mediaAttempt ?? 1) !== expectedAttempt) {
      return { ok: false, error: "stale media attempt" };
    }
    if (task.status !== "pending" && task.status !== terminalStatus) return { ok: false, error: "deferred terminal outcome conflicts with media result" };
    return null;
  };
  eventBus.handle("deferred:register", ({ taskId, meta, durable = false, ...payload }) => {
    const target = requireDeferredTarget(payload);
    if (isDeferredTargetError(target)) return target;
    const resolved = target as ReturnType<typeof sessionRefPayload>;
    const existing = deferredResultStore.query(taskId);
    if (durable && meta?.mediaAttempt !== undefined && existing && (existing.meta?.mediaAttempt ?? 1) > meta.mediaAttempt) {
      return { ok: false, error: "stale media attempt" };
    }
    deferredResultStore.defer(taskId, resolved, meta);
    return durableResult(durable, { ok: true, ...(resolved.sessionId ? { sessionId: resolved.sessionId, sessionRef: resolved.sessionRef } : {}), sessionPath: resolved.sessionPath });
  });
  eventBus.handle("deferred:retry", ({ taskId, meta, durable = false, ...payload }) => {
    const target = requireDeferredTarget(payload);
    if (isDeferredTargetError(target)) return target;
    const resolved = target as ReturnType<typeof sessionRefPayload>;
    const existing = deferredResultStore.query(taskId);
    if (durable && meta?.mediaAttempt !== undefined && existing) {
      const previousAttempt = existing.meta?.mediaAttempt ?? 1;
      if (previousAttempt > meta.mediaAttempt) return { ok: false, error: "stale media attempt" };
      if (previousAttempt === meta.mediaAttempt) {
        return existing.status === "pending"
          ? durableResult(durable)
          : { ok: false, error: "media attempt is already settled" };
      }
    }
    deferredResultStore.retry(taskId, resolved, meta);
    return durableResult(durable, { ok: true, ...(resolved.sessionId ? { sessionId: resolved.sessionId, sessionRef: resolved.sessionRef } : {}), sessionPath: resolved.sessionPath });
  });
  eventBus.handle("deferred:resolve", ({ taskId, result, files, sessionFiles, durable = false, expectedAttempt }) => {
    if (expectedAttempt !== undefined) {
      const denied = checkAttempt(taskId, expectedAttempt, "resolved");
      if (denied) return denied;
    }
    deferredResultStore.resolve(taskId, normalizeDeferredResolveResult({ result, files, sessionFiles }));
    return durableResult(durable);
  });
  eventBus.handle("deferred:fail", ({ taskId, reason, error, durable = false, expectedAttempt }) => {
    if (expectedAttempt !== undefined) {
      const denied = checkAttempt(taskId, expectedAttempt, "failed");
      if (denied) return denied;
    }
    deferredResultStore.fail(taskId, reason ?? error?.message ?? String(error));
    return durableResult(durable);
  });
  eventBus.handle("deferred:query", ({ taskId }) => {
    return deferredResultStore.query(taskId);
  });
  eventBus.handle("deferred:list-pending", (payload: any = {}) => {
    const target = requireDeferredTarget(payload);
    if (isDeferredTargetError(target)) return [];
    return deferredResultStore.listPending(target as ReturnType<typeof sessionRefPayload>);
  });
  eventBus.handle("deferred:abort", ({ taskId, reason, durable = false, expectedAttempt }) => {
    if (expectedAttempt !== undefined) {
      const denied = checkAttempt(taskId, expectedAttempt, "aborted");
      if (denied) return denied;
    }
    deferredResultStore.abort(taskId, reason);
    return durableResult(durable);
  });
}
