// 快捷窗交接：只有主渲染页确认真实会话与来源后，才聚焦主窗并收起快捷窗。
const { randomUUID } = require('node:crypto');
function createQuickChatSessionHandoff({ getMainWindow, getQuickChatWindow, hideQuickChatWindow, activateApp = () => {}, timeoutMs = 15000, focusTimeoutMs = 2000 }) {
  let pending = null;
  const alive = win => !!win && !win.isDestroyed() && !win.webContents.isDestroyed();
  const trusted = (event, win) => alive(win) && event?.sender === win.webContents
    && event.senderFrame?.processId === win.webContents.mainFrame.processId
    && event.senderFrame?.routingId === win.webContents.mainFrame.routingId;
  const result = (ok, request, code, message) => ({ ok, requestId: request?.requestId || null, sessionPath: request?.sessionPath || null, sessionId: request?.sessionId || null, ...(code ? { code, message } : {}) });
  function settle(item, value) {
    if (pending !== item) return false;
    pending = null; clearTimeout(item.timer);
    for (const [emitter, name, handler] of item.listeners) emitter.removeListener(name, handler);
    item.resolve(value); return true;
  }
  function fail(item, code, message) { return settle(item, result(false, item.request, code, message)); }
  async function open(event, input) {
    const quick = getQuickChatWindow(), main = getMainWindow();
    if (!trusted(event, quick)) return result(false, null, 'QUICK_CHAT_SENDER_REJECTED', '快捷聊天窗口已失效');
    if (!alive(main)) return result(false, null, 'QUICK_CHAT_MAIN_UNAVAILABLE', '主窗口已关闭，请重新打开后再试');
    if (!input || typeof input !== 'object' || typeof input.sessionPath !== 'string' || !input.sessionPath.trim()
      || input.sessionPath.length > 8192 || !/^[a-f0-9]{64}$/.test(input.ownerKey || '')
      || (input.sessionId != null && (typeof input.sessionId !== 'string' || !input.sessionId.trim()))) {
      return result(false, null, 'QUICK_CHAT_INVALID_TARGET', '目标会话资料不完整');
    }
    const sessionPath = input.sessionPath.trim(), sessionId = input.sessionId || null;
    if (sessionPath.startsWith('native:') && (!sessionId || sessionPath !== `native:${sessionId}`)) {
      return result(false, null, 'QUICK_CHAT_INVALID_TARGET', '目标会话标识不一致');
    }
    if (pending) fail(pending, 'QUICK_CHAT_SUPERSEDED', '已改为处理另一条打开请求');
    const request = { requestId: randomUUID(), sessionPath, sessionId, ownerKey: input.ownerKey, expiresAt: Date.now() + timeoutMs };
    return new Promise(resolve => {
      const item = { request, main, quick, resolve, listeners: [], timer: null, completing: false };
      pending = item;
      item.timer = setTimeout(() => fail(item, 'QUICK_CHAT_OPEN_TIMEOUT', '主窗口尚未确认打开会话，请重试'), timeoutMs);
      const listen = (emitter, name, code, accept = () => true) => {
        const handler = (...args) => accept(...args) && fail(item, code, '窗口状态已改变，未完成会话交接');
        emitter.on(name, handler); item.listeners.push([emitter, name, handler]);
      };
      listen(main, 'closed', 'QUICK_CHAT_MAIN_DESTROYED');
      listen(main.webContents, 'render-process-gone', 'QUICK_CHAT_MAIN_DESTROYED');
      listen(main.webContents, 'did-start-navigation', 'QUICK_CHAT_MAIN_NAVIGATED', (_event, _url, _inPlace, isMainFrame) => isMainFrame);
      listen(quick, 'closed', 'QUICK_CHAT_SOURCE_DESTROYED');
      try { main.webContents.send('quick-chat-open-session', request); }
      catch { fail(item, 'QUICK_CHAT_DELIVERY_FAILED', '无法向主窗口发送打开请求'); }
    });
  }
  async function acknowledge(event, response) {
    const item = pending;
    if (!item || item.completing || !trusted(event, item.main) || getMainWindow() !== item.main
      || response?.requestId !== item.request.requestId) return { accepted: false };
    const request = item.request;
    if (response.ok !== true) {
      fail(item, response.code || 'QUICK_CHAT_SWITCH_FAILED', response.message || '目标会话未能打开');
      return { accepted: true };
    }
    if (response.sessionPath !== request.sessionPath || response.ownerKey !== request.ownerKey
      || typeof response.sessionId !== 'string' || !response.sessionId
      || (request.sessionId && response.sessionId !== request.sessionId)) {
      fail(item, 'QUICK_CHAT_TARGET_MISMATCH', '主窗口返回的会话或来源不一致'); return { accepted: false };
    }
    item.completing = true;
    try {
      if (!alive(item.quick) || getQuickChatWindow() !== item.quick || !alive(item.main)) throw Error('window gone');
      if (item.main.isMinimized()) item.main.restore();
      activateApp(); item.main.show(); item.main.focus(); item.main.webContents.focus();
      const deadline = Date.now() + focusTimeoutMs;
      while (pending === item && alive(item.main) && !item.main.isFocused() && Date.now() < deadline) {
        await new Promise(resolve => setTimeout(resolve, 25));
      }
      if (pending !== item) return { accepted: false };
      if (!alive(item.main) || !item.main.isFocused()) throw Error('focus unavailable');
      if (!alive(item.quick) || getQuickChatWindow() !== item.quick) throw Error('source gone');
      hideQuickChatWindow();
      if (item.quick.isVisible()) throw Error('source still visible');
      settle(item, { ...result(true, request), sessionId: response.sessionId }); return { accepted: true };
    } catch {
      fail(item, 'QUICK_CHAT_FOCUS_FAILED', '主窗口无法获得焦点，快捷聊天窗口已保留'); return { accepted: false };
    }
  }
  return { open, acknowledge, dispose() { if (pending) fail(pending, 'QUICK_CHAT_CLOSED', '应用正在关闭'); } };
}
module.exports = { createQuickChatSessionHandoff };
