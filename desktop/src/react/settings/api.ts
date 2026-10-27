/**
 * Settings window API utilities
 * 从 settings store 读 port/token，独立于主窗口
 */
import { useSettingsStore } from './store';
import { rustBridgeRoute } from '../services/rust-bridge';
import { rustBrowserRoute } from '../services/rust-browser';
import { rustSettingsRoute } from '../services/rust-settings';
import {
  appendConnectionAuth,
  buildConnectionUrl,
  requireServerConnection,
  resolveServerConnection,
} from '../services/server-connection';
import { errorWithCode } from '../errors/error-presenter';
import { normalizeSessionRouteError } from '../../../../shared/error-user-messages.ts';
import { captureSettingsConnection, ownsSettingsConnection } from './connection-owner';

const DEFAULT_TIMEOUT = 30_000;

function routeForConnection(connection: { serverNodeKind?: string }, path: string): string {
  if (connection.serverNodeKind !== 'lingxi-service') return path;
  return rustSettingsRoute(connection, rustBrowserRoute(connection, rustBridgeRoute(connection, path)));
}

/** 原生设置读写的资料所有者必须与 fetch 使用同一有效连接坐标。 */
export function settingsConnectionKey(): string | null {
  const state = useSettingsStore.getState();
  const connection = resolveServerConnection(state);
  if (connection?.serverNodeKind !== 'lingxi-service') return state.activeServerConnectionId;
  return JSON.stringify([connection.connectionId, connection.kind, connection.baseUrl, connection.token, connection.serverNodeKind, connection.serverNodeTransport, connection.serverId, connection.serverNodeId, connection.userId, connection.studioId]);
}

export function lingxiUrl(path: string): string {
  const connection = requireServerConnection(
    useSettingsStore.getState(),
    `settings lingxiUrl ${path}: server connection not ready`,
  );
  return buildConnectionUrl(connection, routeForConnection(connection, path), { includeTokenQuery: true });
}

export async function lingxiFetch(
  path: string,
  opts: RequestInit & { timeout?: number } = {},
): Promise<Response> {
  const connection = requireServerConnection(
    useSettingsStore.getState(),
    `settings lingxiFetch ${path}: server connection not ready`,
  );
  const headers = appendConnectionAuth(connection, opts.headers);
  const requestOwner = captureSettingsConnection();

  const { timeout = DEFAULT_TIMEOUT, signal: callerSignal, ...fetchOpts } = opts;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeout);
  const forwardAbort = () => controller.abort();

  // If caller provided a signal, forward its abort to our controller
  if (callerSignal) {
    if (callerSignal.aborted) { controller.abort(); }
    else { callerSignal.addEventListener('abort', forwardAbort, { once: true }); }
  }

  try {
    const res = await fetch(buildConnectionUrl(connection, routeForConnection(connection, path)), {
      ...fetchOpts,
      headers,
      signal: controller.signal,
    });
    if (!res.ok) {
      if (res.status === 401 && connection.serverNodeKind === 'lingxi-service' && ownsSettingsConnection(requestOwner)) {
        // 仅当前连接的凭证失效撤回页面身份；旧连接迟到响应不能清掉新身份。
        useSettingsStore.setState({ settingsIdentityOwner: null, rustSettingsUnavailable: true });
      }
      // 错误码在这里就得挂到异常上。调用方拿到的是抛出来的 Error，不是响应体——
      // 一旦这里只带走那句英文，后面再想把失败翻成人话就没有依据了。
      const { message, code, mcp } = await readErrorResponse(res);
      throw Object.assign(errorWithCode(message || `lingxiFetch ${path}: ${res.status} ${res.statusText}`, code), mcp || {});
    }
    return res;
  } finally {
    clearTimeout(timer);
    callerSignal?.removeEventListener('abort', forwardAbort);
  }
}

/**
 * 读取 JSON 成功响应，并把 HTTP 200 中的业务错误同样按失败处理。
 * HTTP 非成功状态已经由 lingxiFetch 拒绝，这里补齐路由返回 `{ error }` 的边界。
 */
export async function lingxiFetchJson<T = Record<string, unknown>>(
  path: string,
  opts: RequestInit & { timeout?: number } = {},
): Promise<T> {
  const res = await lingxiFetch(path, opts);
  const data = await res.json() as T & { error?: unknown };
  if (data && typeof data === 'object' && 'error' in data && data.error) {
    const message = typeof data.error === 'string' ? data.error : JSON.stringify(data.error);
    throw new Error(withSavedSettingsFacts(data, message || `lingxiFetch ${path}: request failed`));
  }
  return data;
}

/** 部分成功也必须传到页面文案，不能把已经生效的设置显示成从未保存。 */
function withSavedSettingsFacts(data: unknown, message: string): string {
  if (!data || typeof data !== 'object') return message;
  const facts = data as { preferencesSaved?: unknown; settingsUpdate?: { saved?: unknown; runtimeApplied?: unknown }; result?: { saved?: unknown; applied?: unknown }; modelConfiguration?: { saved?: unknown; applied?: unknown } };
  const model = facts.result || facts.modelConfiguration || (facts.settingsUpdate && { saved: facts.settingsUpdate.saved, applied: facts.settingsUpdate.runtimeApplied });
  const prefix = model?.applied === true
    ? ['settings.transfer.partialApplied', '设置已生效，但后续步骤未完成。']
    : model?.saved === true
      ? ['settings.transfer.savedNotApplied', '设置已保存，但尚未生效。']
      : facts.preferencesSaved === true
        ? ['settings.transfer.partialSaved', '部分设置已保存，请重新读取后再操作。']
        : null;
  if (!prefix) return message;
  const translated = typeof window !== 'undefined' ? window.t?.(prefix[0]) : null;
  return `${typeof translated === 'string' && translated !== prefix[0] ? translated : prefix[1]} ${message}`;
}

/**
 * 读错误响应：给调用方一句话，外加一个错误码。
 *
 * 同一个失败会有两种响应形状：route handler 自己应答的是扁平的 `{error, code}`，
 * 而没被 route 接住、冒到服务端顶层错误处理器的异常会被包成嵌套的
 * `{error: {code, message, traceId}}`。只认扁平形状的话，嵌套那种会把整段 JSON
 * 文本当成"错误消息"显示出来，错误码也一并丢掉。归一交给 shared 的
 * normalizeSessionRouteError——它两种都认，跟其它消费点用的是同一个归一器。
 *
 * 归一取不到消息时才往下退：先看 `{message}` 这种不带 error 字段的写法，
 * 最后退回整段响应文本。body 读不出来或根本不是 JSON 时没有码，文案跟以前一样。
 */
async function readErrorResponse(res: Response): Promise<{ message: string | null; code: string | null; mcp?: { settingsUpdate: unknown; results?: unknown[] } }> {
  let text: string;
  try {
    text = await res.text();
  } catch {
    return { message: null, code: null };
  }
  if (!text) return { message: null, code: null };

  let data: unknown;
  try {
    data = JSON.parse(text);
  } catch {
    // try 只围 JSON.parse：底下的字段读取若有缺陷，应该原地炸出来让人看见，
    // 而不是被这个 catch 收编成"body 不是 JSON"，把 bug 伪装成正常分支。
    return { message: text.trim() || null, code: null };
  }

  const mcp = data && typeof data === 'object' ? data as { settingsUpdate?: { status?: unknown; summary?: unknown; error?: { message?: unknown; code?: unknown } }; results?: unknown[] } : null;
  if (mcp?.settingsUpdate && ['failed', 'cancelled'].includes(String(mcp.settingsUpdate.status))) {
    const update = mcp.settingsUpdate;
    const message = typeof update.error?.message === 'string' ? update.error.message
      : typeof update.summary === 'string' ? update.summary : 'MCP 设置未完成';
    return { message: withSavedSettingsFacts(data, message), code: typeof update.error?.code === 'string' ? update.error.code : null,
      mcp: { settingsUpdate: update, ...(Array.isArray(mcp.results) ? { results: mcp.results } : {}) } };
  }

  const routeError = normalizeSessionRouteError(data);
  if (routeError.message) return { message: withSavedSettingsFacts(data, routeError.message), code: routeError.code };

  const legacyMessage = (data as { message?: unknown } | null)?.message;
  if (typeof legacyMessage === 'string' && legacyMessage.trim()) {
    return { message: withSavedSettingsFacts(data, legacyMessage.trim()), code: routeError.code };
  }
  return { message: text.trim() || null, code: routeError.code };
}

/** 根据 yuan 类型返回 fallback 头像路径 */
export function yuanFallbackAvatar(yuan?: string): string {
  const t = window.t || ((k: string) => k);
  const types = (t('yuan.types') || {}) as Record<string, { avatar?: string }>;
  const entry = types[yuan || 'lingxi'];
  return `assets/${entry?.avatar || 'Lingxi.png'}`;
}
