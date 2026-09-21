/**
 * 身份品牌类型与运行时守卫（P01-T04）。
 *
 * 三层纪律（对齐任务书 T04-4）：
 *   1. 品牌 ≠ 安全验证：品牌只阻止编译期混用（把 mt_ trace 传给要 mc_ call 的
 *      参数）。进入系统的字符串必须先经 asXxx / requireXxx 运行时守卫（前缀+形态）。
 *   2. 品牌方向：branded → string 恒可赋值（既有消费者零破坏）；
 *      string → branded 只能经守卫构造，任意字符串直接赋给品牌是编译错误。
 *   3. 铸造厂返回品牌：mint 出来的身份天生可信，不需要二次校验。
 *
 * 覆盖的身份（P00 OWNERSHIP_MAP 已核对各自唯一铸造厂）：
 *   - SessionId    `sess_`  SessionManifestStore（core/session-manifest/id.ts）
 *   - ModelCallId  `mc_`    model-call-identity（本仓唯一铸造厂）
 *   - ModelAttemptId `ma_`  同上
 *   - ModelTraceId `mt_`    ModelTraceScope（lib/llm/model-trace-scope.ts）
 *   - ToolCallId            Provider 分配（形状随供应商：tc_/call_/…），守卫只
 *                           要求非空字符串，不伪造前缀约束。
 */

export type Brand<T, B extends string> = T & { readonly __brand: B };

export type SessionId = Brand<string, "SessionId">;
export type ModelCallId = Brand<string, "ModelCallId">;
export type ModelAttemptId = Brand<string, "ModelAttemptId">;
export type ModelTraceId = Brand<string, "ModelTraceId">;
export type ToolCallId = Brand<string, "ToolCallId">;

// 前缀 + 至少一段 [a-z0-9] 段（形态目标：mc_{ts36}_{seq36}_{rand6} 等）。
// 守卫的职责是拦截跨身份误用（mt_/sess_/ma_ 互串），不是完整形状校验。
const SESSION_ID_PATTERN = /^sess_[a-z0-9]+(?:_[a-z0-9]+)*$/;
const MODEL_CALL_ID_PATTERN = /^mc_[a-z0-9]+(?:_[a-z0-9]+)*$/;
const MODEL_ATTEMPT_ID_PATTERN = /^ma_[a-z0-9]+(?:_[a-z0-9]+)*$/;
const MODEL_TRACE_ID_PATTERN = /^mt_[a-z0-9]+(?:_[a-z0-9]+)*$/;

function brandedString<T extends Brand<string, string>>(
  value: unknown,
  pattern: RegExp,
): T | null {
  return typeof value === "string" && pattern.test(value) ? (value as T) : null;
}

export function asSessionId(value: unknown): SessionId | null {
  return brandedString<SessionId>(value, SESSION_ID_PATTERN);
}

export function asModelCallId(value: unknown): ModelCallId | null {
  return brandedString<ModelCallId>(value, MODEL_CALL_ID_PATTERN);
}

export function asModelAttemptId(value: unknown): ModelAttemptId | null {
  return brandedString<ModelAttemptId>(value, MODEL_ATTEMPT_ID_PATTERN);
}

export function asModelTraceId(value: unknown): ModelTraceId | null {
  return brandedString<ModelTraceId>(value, MODEL_TRACE_ID_PATTERN);
}

export function asToolCallId(value: unknown): ToolCallId | null {
  return typeof value === "string" && value.length > 0 ? (value as ToolCallId) : null;
}

/**
 * 严格构造器：校验失败抛 TypeError（而不是返回 null）。
 * 用于"这里必须是该身份"的边界——把畸形值显式暴露，而不是静默降级。
 */
function requireBranded<T extends Brand<string, string>>(
  kind: string,
  value: unknown,
  guard: (v: unknown) => T | null,
): T {
  const branded = guard(value);
  if (branded === null) {
    throw new TypeError(`invalid ${kind}: ${typeof value === "string" ? value.slice(0, 32) : String(value)}`);
  }
  return branded;
}

export function requireSessionId(value: unknown): SessionId {
  return requireBranded("SessionId (sess_…)", value, asSessionId);
}

export function requireModelCallId(value: unknown): ModelCallId {
  return requireBranded("ModelCallId (mc_…)", value, asModelCallId);
}

export function requireModelAttemptId(value: unknown): ModelAttemptId {
  return requireBranded("ModelAttemptId (ma_…)", value, asModelAttemptId);
}

export function requireModelTraceId(value: unknown): ModelTraceId {
  return requireBranded("ModelTraceId (mt_…)", value, asModelTraceId);
}

export function requireToolCallId(value: unknown): ToolCallId {
  return requireBranded("ToolCallId (non-empty)", value, asToolCallId);
}
