const { randomUUID } = require('node:crypto');

const ACTIONS = new Set(['back', 'forward', 'reload', 'new_tab', 'switch_tab', 'close_tab', 'show', 'takeover', 'release']);
function connectionIdentity(connection) {
  return JSON.stringify([connection.connectionId, connection.kind, connection.baseUrl, connection.token, connection.serverId, connection.serverNodeId, connection.serverNodeKind, connection.serverNodeTransport, connection.userId, connection.studioId, connection.authState, connection.trustState, connection.credentialKind]);
}
function nativeSession(value) {
  if (typeof value !== 'string' || !/^native:[^\s/?#\u0000-\u001f]{1,256}$/.test(value)) throw new Error('browser_native_session_required');
  return value;
}
/** 只有主窗口可登记连接；命令只接受随机句柄，不能自行指定联网地址或凭据。 */
function createRustBrowserControl({ fetch: request, onChange = () => {} }) {
  let current = null;
  const pending = new Set();
  function setConnection(value) {
    if (value == null) {
      if (current) { for (const item of pending) item.abort(); current = null; onChange(); }
      return null;
    }
    if (value.serverNodeKind !== 'lingxi-service' || typeof value.connectionId !== 'string' || !value.connectionId || typeof value.serverId !== 'string' || !value.serverId) throw new Error('browser_connection_invalid');
    const url = new URL(value.baseUrl);
    if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password || url.search || url.hash) throw new Error('browser_connection_invalid');
    if (value.token != null && (typeof value.token !== 'string' || /[\r\n]/.test(value.token))) throw new Error('browser_connection_invalid');
    const identity = connectionIdentity(value);
    if (current?.identity === identity) return current.key;
    for (const item of pending) item.abort();
    current = { identity, key: randomUUID(), connection: structuredClone(value) };
    onChange();
    return current.key;
  }
  function context(target) {
    if (!current || target?.connectionKey !== current.key) throw new Error('browser_connection_changed');
    return { connectionKey: current.key, sessionPath: nativeSession(target.sessionPath), executionNode: current.connection.serverId, serviceLabel: current.connection.label || current.connection.serverId, serviceUrl: current.connection.baseUrl };
  }
  async function call(target, action, { tabId, url } = {}) {
    const scope = context(target);
    if (!ACTIONS.has(action) && !['workspaces', 'stop', 'open'].includes(action)) throw new Error('browser_action_invalid');
    if (tabId !== undefined && (typeof tabId !== 'string' || !tabId || tabId.length > 4096 || /[\s*?\u0000-\u001f]/.test(tabId))) throw new Error('browser_tab_invalid');
    if (url !== undefined && (typeof url !== 'string' || url.length > 8192)) throw new Error('browser_url_invalid');
    const saved = current;
    const controller = new AbortController();
    pending.add(controller);
    const timer = setTimeout(() => controller.abort(), 30_000);
    const path = action === 'workspaces' ? 'workspaces' : action === 'stop' ? 'close-session' : action === 'open' ? 'open-session' : 'command';
    try {
      const response = await request(`${saved.connection.baseUrl.replace(/\/+$/, '')}/lingxi/v1/browser/${path}`, {
        method: action === 'workspaces' ? 'GET' : 'POST', credentials: 'include', redirect: 'error', signal: controller.signal,
        headers: { ...(saved.connection.token ? { Authorization: `Bearer ${saved.connection.token}` } : {}), ...(action === 'workspaces' ? {} : { 'Content-Type': 'application/json' }) },
        ...(action === 'workspaces' ? {} : { body: JSON.stringify(action === 'stop' ? { sessionPath: scope.sessionPath, revoke: true } : action === 'open' ? { sessionPath: scope.sessionPath } : { sessionPath: scope.sessionPath, action, ...(tabId !== undefined ? { tabId } : {}), ...(url !== undefined ? { url } : {}) }) }),
      });
      const data = await response.json();
      context(scope);
      if (current !== saved) throw new Error('browser_connection_changed');
      if (!response.ok || data?.error) throw new Error(typeof data?.error === 'string' ? data.error : `browser_request_failed:${response.status}`);
      if (!data || typeof data !== 'object') throw new Error('browser_response_invalid');
      return data;
    } finally { clearTimeout(timer); pending.delete(controller); }
  }
  return { setConnection, context, call, isActive: () => current !== null };
}
module.exports = { createRustBrowserControl, nativeSession };
