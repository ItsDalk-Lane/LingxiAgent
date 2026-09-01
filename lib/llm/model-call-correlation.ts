/**
 * model-call-correlation.ts — assembled assistant message ↔ call 身份的
 * ephemeral 关联（任务书 §六十四 MC-01 Ledger Correlation）。
 *
 * 机制：Pi agent loop 对 `await streamFn(...)` 与 `emit message_end` 持有的是
 * **同一个 assembled message 对象**（promise resolve 语义），stream observer
 * 在 `result()` resolve 时以 WeakMap 登记 message → {callId, traceId,
 * parentCallId}；message_end 补账（session-coordinator / bridge-session-manager
 * / agent-executor 的 recordAssistantUsage）读取同一对象拿到身份，写入 ledger
 * metadata。对象被复制/替换时查不到 → 返回 null，不猜（fail-open，关联缺失
 * 不影响业务）。
 *
 * 内存：WeakMap 条目随 message 对象 GC 自动回收，无生命周期清理负担（§八十五
 * 禁止永不清理的 Map<callId,…>——本模块不持有对 callId 的强引用）。
 * 不写 session message schema、不污染 assistant content、不给 Provider 发
 * 任何标记（§六十五）。
 */

export type ModelCallIdentityTriple = {
  modelCallId: string;
  traceId: string | null;
  parentCallId: string | null;
};

/** 写入 Pi session 的隐藏关联条目；custom entry 不进入模型上下文。 */
export const MODEL_CALL_REFERENCE_RECORD_TYPE = "hana-model-call-reference-v1";

const MESSAGE_IDENTITY = new WeakMap<object, ModelCallIdentityTriple>();

/** stream observer 在 result() resolve 时登记（同对象幂等覆盖无害）。 */
export function noteModelCallMessageIdentity(
  message: unknown,
  identity: ModelCallIdentityTriple,
): void {
  if (!message || typeof message !== "object") return;
  try {
    MESSAGE_IDENTITY.set(message as object, identity);
  } catch {
    // frozen/不可扩展对象：登记失败 = 关联缺失，不影响业务。
  }
}

/**
 * message_end 补账处读取。查不到（对象复制 / 未观测调用）返回 null——
 * correlation 是增强，不是断言。
 */
export function modelCallIdentityForMessage(message: unknown): ModelCallIdentityTriple | null {
  if (!message || typeof message !== "object") return null;
  try {
    return MESSAGE_IDENTITY.get(message as object) ?? null;
  } catch {
    return null;
  }
}

/** ledger metadata 的安全投影：null 身份 → null metadata（不写空壳）。 */
export function modelCallLedgerMetadataForMessage(message: unknown): {
  modelCallId: string;
  traceId: string | null;
  parentCallId: string | null;
} | null {
  return modelCallIdentityForMessage(message);
}

/**
 * 必须在 SDK 落盘助手消息之前调用，使关联条目紧邻并位于助手消息之前。
 * 查不到运行时身份时不猜，也不写空记录。
 */
export function persistModelCallReferenceForMessage(
  sessionManager: unknown,
  message: unknown,
): ModelCallIdentityTriple | null {
  const identity = modelCallIdentityForMessage(message);
  const append = (sessionManager as any)?.appendCustomEntry;
  if (!identity || typeof append !== "function") return null;
  append.call(sessionManager, MODEL_CALL_REFERENCE_RECORD_TYPE, {
    schemaVersion: 1,
    modelCallId: identity.modelCallId,
    traceId: identity.traceId,
    parentCallId: identity.parentCallId,
  });
  return identity;
}
