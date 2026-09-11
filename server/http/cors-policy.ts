const DEFAULT_LOOPBACK_WEB_ORIGIN = /^https?:\/\/(localhost|127\.0\.0\.1)(:\d+)?$/;
const ELECTRON_FILE_ORIGINS = new Set(["file://", "file:///"]);

export function isCorsOriginAllowed({
  origin,
  configuredOrigin = "",
}: { origin?: string; configuredOrigin?: string } = {}) {
  const value = String(origin || "");
  if (!value) return false;
  if (configuredOrigin) return value === configuredOrigin;
  if (value === "null") return true;
  if (ELECTRON_FILE_ORIGINS.has(value)) return true;
  return DEFAULT_LOOPBACK_WEB_ORIGIN.test(value);
}

// E07：协议响应头契约（单一来源；server/index.ts 与真实链路 smoke 共用）。
// Expose-Headers 让渲染进程能读到 ETag/协议版本/推荐页大小；
// Allow-Headers 增补 If-None-Match（不影响 Authorization 等既有头）；
// 来源仍走白名单（isCorsOriginAllowed），绝不 credentials + 通配。
export function applyCorsResponseHeaders(
  c: any,
  { origin, configuredOrigin = "" }: { origin?: string; configuredOrigin?: string } = {},
): void {
  const isAllowed = isCorsOriginAllowed({ origin, configuredOrigin });
  const value = String(origin || "");
  if (value && isAllowed) {
    c.header("Access-Control-Allow-Origin", value);
    c.header("Access-Control-Allow-Credentials", "true");
  }
  c.header("Access-Control-Allow-Methods", "GET, POST, PUT, PATCH, DELETE, OPTIONS");
  c.header("Access-Control-Allow-Headers", "Content-Type, Authorization, If-None-Match");
  c.header("Access-Control-Expose-Headers", "ETag, Lingxi-History-Protocol, Lingxi-History-Page-Limit");
}
