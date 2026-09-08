import { authorizeHttpRoute } from "./route-security.ts";

/**
 * HTTP 入口的 principal 解析 + 路由授权。server/index.ts 的全局鉴权中间件与
 * 测试共用这一条链路，避免测试侧重新实现生产逻辑后两边漂移。
 *
 * 顺序：
 * 1. 主鉴权：bearer / query token / web session cookie
 *    （serverAuthService.authenticateRequestDetailed）。
 * 2. authorizeHttpRoute 路由级授权。
 *
 * 返回 { ok: true, principal } 或 { ok: false, status, body }；调用方负责把
 * body 序列化为 JSON 响应，并在成功时自行 c.set("authPrincipal", principal)。
 */
interface DetailedAuthResult {
  principal: object | null;
  denied: {
    error?: string;
    reason?: string;
    credentialSource?: string;
    connectionKind?: string;
  } | null;
}

interface HttpAuthService {
  authenticateRequestDetailed: (input: {
    authorization?: string | null;
    queryToken?: string | null;
    cookieHeader?: string | null;
    allowQueryToken?: boolean;
    connectionKind?: string;
  }) => DetailedAuthResult;
}

export function resolveHttpRequestPrincipal(c, engine, {
  serverAuthService,
  wsTicketService = null,
  connectionKind,
}: {
  serverAuthService: HttpAuthService;
  wsTicketService?: any;
  connectionKind: string;
}) {
  const routePath = new URL(c.req.url).pathname;
  const authResult = serverAuthService.authenticateRequestDetailed({
    authorization: c.req.header("authorization"),
    queryToken: c.req.query("token"),
    cookieHeader: c.req.header("cookie"),
    allowQueryToken: true,
    connectionKind,
  });
  let principal = authResult.principal;
  if (!principal && routePath === "/ws") {
    const ticket = c.req.query("wsTicket");
    if (ticket) {
      principal = wsTicketService?.consumeTicket?.(ticket, {
        connectionKind,
        path: routePath,
      }) || null;
      if (!principal) {
        return {
          ok: false as const,
          status: 403,
          body: {
            error: "forbidden",
            reason: "invalid_ws_ticket",
            connectionKind,
          },
        };
      }
    }
  }
  if (!principal) {
    const denied = authResult.denied || {};
    return {
      ok: false as const,
      status: 403,
      body: {
        error: denied.error || "forbidden",
        reason: denied.reason || "auth_failed",
        ...(denied.credentialSource ? { credentialSource: denied.credentialSource } : {}),
        connectionKind: denied.connectionKind || connectionKind,
      },
    };
  }
  const authz: any = authorizeHttpRoute({
    method: c.req.method,
    path: routePath,
    principal,
  });
  if (!authz.allowed) {
    return {
      ok: false as const,
      status: authz.status,
      body: {
        error: authz.error,
        ...(authz.reason ? { reason: authz.reason } : {}),
        ...(authz.requiredScope ? { requiredScope: authz.requiredScope } : {}),
      },
    };
  }
  return { ok: true as const, principal };
}
