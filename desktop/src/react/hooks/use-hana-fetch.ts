import { useStore } from '../stores';
import { rustAutomationRoute } from '../services/rust-automation';
import { rustBridgeRoute } from '../services/rust-bridge';
import { rustBrowserRoute } from '../services/rust-browser';
import { rustKnowledgeRoute } from '../services/rust-knowledge';
import { rustObservabilityRoute } from '../services/rust-observability';
import { rustWorkbenchRoute } from '../services/rust-workbench';
import { rustSettingsRoute } from '../services/rust-settings';
import {
  appendConnectionAuth,
  buildConnectionUrl,
  requireServerConnection,
} from '../services/server-connection';

const DEFAULT_TIMEOUT = 30_000;
function serviceRoute(connection: { serverNodeKind?: string }, path: string): string {
  if (connection.serverNodeKind === 'lingxi-service' && (
    /^\/api\/skills(?:[/?]|$)/.test(path)
    || /^\/api\/agents\/[^/]+\/(?:skills|skill-bundles)\/[^/?]+(?:[?]|$)/.test(path)
    || /^\/api\/(?:channels|dm)(?:[/?]|$)/.test(path)
    || /^\/api\/desk\/beautify(?:[/?]|$)/.test(path)
    || /^\/api\/conversations\/[^/?]+\/(?:agent-phone-settings|agent-phone-tool-mode|agent-activities|export)(?:\?|$)/.test(path)
    || /^\/api\/session-collab\/(?:apply|reject)(?:\?|$)/.test(path)
    || /^\/api\/media(?:[/?]|$)/.test(path)
    || /^\/api\/speech-recognition(?:[/?]|$)/.test(path)
    || /^\/api\/file-history(?:[/?]|$)/.test(path)
    || /^\/api\/session-projects(?:[/?]|$)/.test(path)
    || /^\/api\/sessions\/(?:search|find|new-detached)(?:[/?]|$)/.test(path)
    || /^\/api\/session-thinking-level(?:[/?]|$)/.test(path)
    || /^\/api\/session-permission-mode(?:[/?]|$)/.test(path)
    || /^\/api\/preferences\/appearance(?:[/?]|$)/.test(path)
    || /^\/api\/preferences\/workspace-ui-state(?:[/?]|$)/.test(path)
    || /^\/api\/system\/env-deps(?:\/refresh)?(?:[/?]|$)/.test(path)
  )) return `/lingxi/v1${path.slice('/api'.length)}`;
  return rustKnowledgeRoute(connection, rustSettingsRoute(connection, rustWorkbenchRoute(connection, rustBrowserRoute(connection, rustBridgeRoute(connection, rustObservabilityRoute(connection, rustAutomationRoute(connection, path)))))));
}

/**
 * 构建带认证的 LingxiAgent Server URL
 */
export function lingxiUrl(path: string): string {
  const connection = requireServerConnection(
    useStore.getState(),
    `lingxiUrl ${path}: server connection not ready`,
  );
  return buildConnectionUrl(connection, serviceRoute(connection, path), { includeTokenQuery: true });
}

/**
 * 旧会话域按 path/sessionId query 寻址的读端口（summary/memory/
 * prompt-snapshot/history-overview/authorized-folders/content/:id）与
 * 写端口（memory、authorized-folders 的 PATCH body.path）→ Rust
 * /lingxi/v1/sessions/{id}/ 子路由。纯协议转换：
 * - 会话定位只接受 `native:<id>`（或 prompt-snapshot 既有调用使用的
 *   `sessionId` query）；旧 JSONL 路径不在此猜映射，触达时由
 *   server-connection 硬闸显式报错。
 * - 写端口剥离旧 body.path 字段（Rust 子路由按 URL 段定位且
 *   deny_unknown_fields）；权限与数据裁决全部在 Rust。
 */
export function translateLegacySessionCall(
  path: string,
  init: { method?: string; body?: unknown },
): { path: string; body?: unknown } | null {
  const method = (init.method ?? 'GET').toUpperCase();
  const bodyRecord = typeof init.body === 'object' && init.body !== null ? (init.body as Record<string, unknown>) : null;
  const nativeIdOf = (locator: unknown): string | null => {
    if (typeof locator !== 'string' || !locator.startsWith('native:')) return null;
    return locator.slice('native:'.length);
  };
  // R08-T07 F-T07-2 修复（加性）：会话管理写端口的旧→Rust 翻译。T01 绑定表
  // 已判这些路由 MAPPED_ROUTE_EXISTS（Rust 路由同名、body 用 sessionId +
  // camelCase + deny_unknown_fields），但客户端边界此前只翻译读端口与 memory/
  // authorized-folders —— 旧 UI 接 Rust 时 rename/pin/archive/restore/delete/
  // cleanup/sweep/pin-order/todos/fresh-compact 全部被 buildConnectionUrl 响亮拒
  // （t07-ab-drill-02 A/B 实证：A 腿旧内核全过、B 腿全拒）。旧内核连接不经此分支。
  if (method === 'GET' && path === '/api/sessions/archived') {
    return { path: '/lingxi/v1/sessions/archived' };
  }
  if (method === 'POST' && bodyRecord) {
    // 仅当会话定位符是 rust 根形态（native:<id>）才翻译——旧内核连接的定位符
    // 是文件路径，直连旧端点（否则旧腿会被误翻译到 /lingxi/v1/* 而 404，
    // t07-ab-drill-03 A 腿实证）。
    const sid = nativeIdOf(bodyRecord.path);
    const rust = (suffix: string, body: Record<string, unknown>) => ({ path: `/lingxi/v1/sessions/${suffix}`, body });
    switch (path) {
      case '/api/sessions/rename':
        return sid ? rust('rename', { sessionId: sid, title: bodyRecord.title }) : null;
      case '/api/sessions/pin':
        return sid ? rust('pin', { sessionId: sid, pinned: Boolean(bodyRecord.pinned) }) : null;
      case '/api/sessions/archive':
        return sid ? rust('archive', { sessionId: sid, ...(bodyRecord.childMode ? { childMode: bodyRecord.childMode } : {}) }) : null;
      case '/api/sessions/restore':
        return sid ? rust('restore', { sessionId: sid }) : null;
      case '/api/sessions/archived/delete':
        return sid ? rust('archived/delete', { sessionId: sid }) : null;
      case '/api/sessions/cleanup':
        return rust('cleanup', typeof bodyRecord.maxAgeDays === 'number' ? { maxAgeDays: bodyRecord.maxAgeDays } : {});
      case '/api/sessions/sweep-orphaned-workspaces':
        return { path: '/lingxi/v1/sessions/sweep-orphaned-workspaces', body: {} };
      case '/api/sessions/pin-order':
        return Array.isArray(bodyRecord.sessionIds) ? rust('pin-order', { sessionIds: bodyRecord.sessionIds }) : null;
      case '/api/sessions/todos/complete':
      case '/api/sessions/todos/cancel':
      case '/api/sessions/todos/dismiss':
        return sid ? rust(path.slice('/api/sessions/'.length), { sessionId: sid, ...(bodyRecord.version != null ? { version: bodyRecord.version } : {}) }) : null;
      case '/api/sessions/fresh-compact':
        return sid ? { path: `/lingxi/v1/sessions/${encodeURIComponent(sid)}/fresh-compact`, body: {} } : null;
      default:
        break;
    }
  }
  // 写端口（memory / authorized-folders）的会话定位来自 body.path。
  const patchTarget = path === '/api/sessions/memory' || path === '/api/sessions/authorized-folders';
  if (patchTarget && method === 'PATCH' && typeof init.body === 'object' && init.body !== null) {
    const record = init.body as Record<string, unknown>;
    const bodyLocator = typeof record.path === 'string' ? record.path : '';
    if (!bodyLocator.startsWith('native:')) return null;
    const patchId = encodeURIComponent(bodyLocator.slice('native:'.length));
    const { path: _drop, ...restBody } = record;
    return {
      path: `/lingxi/v1/sessions/${patchId}/${path === '/api/sessions/memory' ? 'memory' : 'authorized-folders'}`,
      body: restBody,
    };
  }
  // 读端口的会话定位来自 query（native: 前缀或 prompt-snapshot 的 sessionId）。
  const query = path.includes('?') ? new URLSearchParams(path.slice(path.indexOf('?') + 1)) : new URLSearchParams();
  const locator = query.get('path') ?? query.get('sessionId') ?? '';
  const sessionId = locator.startsWith('native:')
    ? locator.slice('native:'.length)
    : (query.get('sessionId') && !locator.includes('/') ? locator : '');
  if (!sessionId) return null;
  const keepQuery = () => {
    const rest = new URLSearchParams(query);
    rest.delete('path');
    rest.delete('sessionId');
    const encoded = rest.toString();
    return encoded ? `?${encoded}` : '';
  };
  const id = encodeURIComponent(sessionId);
  const readMatch = path.match(/^\/api\/sessions\/(summary|memory|prompt-snapshot|history-overview|authorized-folders)(?:\?|$)/);
  if (readMatch && method === 'GET') {
    return { path: `/lingxi/v1/sessions/${id}/${readMatch[1]}${keepQuery()}` };
  }
  const contentMatch = path.match(/^\/api\/sessions\/content\/([^/?]+)/);
  if (contentMatch && method === 'GET') {
    return { path: `/lingxi/v1/sessions/${id}/content/${contentMatch[1]}${keepQuery()}` };
  }
  return null;
}

/**
 * 带认证的 fetch 封装
 * - 默认 30s 超时
 * - 自动校验 res.ok，非 2xx 抛错
 */
export async function lingxiFetch(
  path: string,
  opts: RequestInit & { timeout?: number; throwOnHttpError?: boolean } = {},
): Promise<Response> {
  const connection = requireServerConnection(
    useStore.getState(),
    `lingxiFetch ${path}: server connection not ready`,
  );
  const headers = appendConnectionAuth(connection, opts.headers);

  const {
    timeout = DEFAULT_TIMEOUT,
    signal: callerSignal,
    throwOnHttpError = true,
    ...fetchOpts
  } = opts;
  // 旧会话域协议翻译（仅字符串 JSON body；其余原样）。
  let bodyCandidate: unknown = fetchOpts.body;
  if (typeof fetchOpts.body === 'string' && fetchOpts.body.length > 0) {
    try { bodyCandidate = JSON.parse(fetchOpts.body); } catch { bodyCandidate = fetchOpts.body; }
  }
  // 旧→新路由翻译仅对 Rust 预览连接生效；旧内核连接直连旧端点（t07-ab-drill-03
  // A 腿实证：无连接门时旧腿被误翻译到 /lingxi/v1/* 而 404）。
  const translated = connection.serverNodeKind === 'lingxi-service'
    ? translateLegacySessionCall(path, { method: fetchOpts.method, body: bodyCandidate })
    : null;
  if (translated) {
    fetchOpts.body = 'body' in translated && translated.body !== undefined
      ? JSON.stringify(translated.body)
      : fetchOpts.body;
    path = translated.path;
  }
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeout);
  if (callerSignal) {
    if (callerSignal.aborted) controller.abort();
    else callerSignal.addEventListener('abort', () => controller.abort(), { once: true });
  }

  try {
    const res = await fetch(buildConnectionUrl(connection, serviceRoute(connection, path)), {
      ...fetchOpts,
      headers,
      signal: controller.signal,
    });
    if (throwOnHttpError && !res.ok) {
      throw new Error(`lingxiFetch ${path}: ${res.status} ${res.statusText}`);
    }
    return res;
  } finally {
    clearTimeout(timer);
  }
}
