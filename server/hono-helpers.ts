/**
 * hono-helpers.js — Hono migration utilities
 */

/** Safe JSON body parse — returns fallback on empty body or non-JSON */
export async function safeJson(c, fallback = {}) {
  try {
    const text = await c.req.text();
    return text ? JSON.parse(text) : fallback;
  } catch {
    return fallback;
  }
}

/**
 * Strict JSON body parse for write boundaries（P01-T05 / A07）。
 *
 * 空 body 仍返回 fallback（既有客户端合法形态）；body 非空但不可解析时抛
 * 400 invalid_json——写入口把畸形载荷静默当空对象处理，会让调用方拿到
 * "按默认参数执行成功"的假象（例如不带 cwd 的会话创建）。
 */
export async function strictJson(c, fallback = {}) {
  const text = await c.req.text();
  if (!text) return fallback;
  try {
    return JSON.parse(text);
  } catch {
    throw strictJsonError();
  }
}

export function strictJsonError() {
  return httpJsonError(400, "invalid_json", "Request body is not valid JSON");
}

function httpJsonError(status: number, code: string, message: string) {
  const error: any = new Error(message);
  error.status = status;
  error.code = code;
  return error;
}

/**
 * 校验 strictJson 的解析结果是 plain object（P01-A07 验收修复）。
 *
 * `null` / `123` / `"str"` / `[]` 都是合法 JSON，但作为对象型契约入口的 body
 * 属于形状错误——静默透传会让后续解构拿到 undefined（按默认参数执行）或
 * 对 null 解构抛 TypeError 变 500。写入口必须显式 400。
 */
export function ensureJsonObjectBody(body: unknown): Record<string, unknown> {
  if (typeof body !== "object" || body === null || Array.isArray(body)) {
    throw httpJsonError(400, "invalid_body", "Request body must be a JSON object");
  }
  return body as Record<string, unknown>;
}

/**
 * 字段级类型校验（P01-A07 验收修复）。
 *
 * spec 声明入口契约要求的字段类型；字段缺省或显式 null 视为未提供（走默认），
 * 提供但类型不符则 400 invalid_field_type。只做类型层——值域/枚举校验归
 * 各契约自身的 normalizer。
 */
export function rejectWrongFieldTypes(
  body: Record<string, unknown>,
  spec: Record<string, "string" | "boolean" | "number">,
) {
  for (const [field, expectedType] of Object.entries(spec)) {
    const value = body[field];
    if (value === undefined || value === null) continue;
    if (typeof value !== expectedType) {
      throw httpJsonError(
        400,
        "invalid_field_type",
        `Field "${field}" must be of type ${expectedType} (got ${typeof value})`,
      );
    }
  }
}
