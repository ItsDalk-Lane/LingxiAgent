/**
 * websocket.ts — WebSocket 连接管理（从 app-ws-shim.ts 迁移）
 *
 * 模块级 singleton，管理 WS 连接生命周期、重连逻辑。
 * 不依赖 ctx 注入，不依赖 React 组件生命周期。
 */


import { handleServerMessage, applyStreamingStatus } from './ws-message-handler';
import { requestStreamResume, injectHandlers, injectWebSocketGetter } from './stream-resume';
import {
  bindResourceEventForegroundCatchUp,
  catchUpResourceEventsAfterReconnect,
  recordResourceEventCursor,
} from './resource-events';
import { useStore } from '../stores';
import { setStatus } from '../utils/ui-helpers';
import {
  buildConnectionWsUrl,
  createLocalServerConnection,
  requestConnectionWsTicket,
  resolveServerConnection,
  type ServerConnection,
} from './server-connection';
import { AppError } from '../../../../shared/errors.ts';
import { errorBus } from '../../../../shared/error-bus.ts';
import { version as desktopVersion } from '../../../../package.json';
import { DATA_EPOCH } from '../../../../shared/contract-versions.ts';
import {
  configureTerminalClientWebSocketGetter,
  requestTerminalSnapshot,
} from './terminal-client';
import { configureBackgroundProcessWebSocketGetter } from './background-process-control';
import {
  composerOriginConnectionKey,
  reconcilePendingComposerSessions,
  unresolvedComposerSessionPaths,
  noteComposerConnectionClosed,
  noteComposerConnectionOpened,
} from './composer-send-coordinator';

// ── 模块级 WS 实例 ──
let _ws: WebSocket | null = null;
let _wsConnectionIsRust = false;
let _wsConnectAttempt = 0;
let _wsHandshakeTimer: ReturnType<typeof setTimeout> | null = null;
const RUST_WIRE_PROTOCOL = 1;
const RUST_HANDSHAKE_TIMEOUT_MS = 10_000;

// ── WS 重连状态 ──
let _wsRetryDelay = 1000;
const WS_RETRY_MAX = 30000;
let _wsRetryTimer: ReturnType<typeof setTimeout> | null = null;
let _wsResumeVersion = 0;
const WS_FAST_RETRY_LIMIT = 20;
const WS_SLOW_RETRY_DELAY = 60_000;
let _wsRetryCount = 0;
let _resourceForegroundCatchUpCleanup: (() => void) | null = null;

// 注入循环依赖的 handlers
injectHandlers(handleServerMessage, applyStreamingStatus);
injectWebSocketGetter(() => _ws);
configureTerminalClientWebSocketGetter(() => _ws);
configureBackgroundProcessWebSocketGetter(() => _ws);

export function resolveStreamingSessionResumeTargets(state: {
  streamingSessions?: string[];
  sessionLocatorsById?: Record<string, { path?: string | null }>;
}): string[] {
  const locators = state.sessionLocatorsById || {};
  const targets = new Set<string>();
  for (const key of state.streamingSessions || []) {
    if (!key) continue;
    if (Object.prototype.hasOwnProperty.call(locators, key)) {
      const path = locators[key]?.path;
      if (typeof path === 'string' && path.trim()) targets.add(path);
      continue;
    }
    targets.add(key);
  }
  return Array.from(targets);
}

export function requestTerminalSnapshotForCurrentSession(state: {
  currentSessionId?: string | null;
  currentSessionPath?: string | null;
}): boolean {
  const sessionPath = typeof state.currentSessionPath === 'string' && state.currentSessionPath.trim()
    ? state.currentSessionPath
    : null;
  if (!sessionPath) return false;
  return requestTerminalSnapshot({
    sessionId: state.currentSessionId || null,
    sessionPath,
  });
}

export function isRustWireServerHello(value: unknown): boolean {
  if (!value || typeof value !== 'object') return false;
  const hello = value as Record<string, unknown>;
  return hello.protocol === 'lingxi.wire'
    && hello.selectedProtocol === RUST_WIRE_PROTOCOL
    && Number.isInteger(hello.wireProtocolMin)
    && Number.isInteger(hello.wireProtocolMax)
    && (hello.wireProtocolMin as number) >= 1
    && (hello.wireProtocolMin as number) <= RUST_WIRE_PROTOCOL
    && (hello.wireProtocolMax as number) >= RUST_WIRE_PROTOCOL
    && (hello.wireProtocolMax as number) >= (hello.wireProtocolMin as number)
    && hello.dataEpoch === DATA_EPOCH
    && hello.serverKind === 'lingxi-service'
    && typeof hello.serverVersion === 'string'
    && hello.serverVersion.trim().length > 0
    && (hello.rejectedCaps === undefined
      || (Array.isArray(hello.rejectedCaps)
        && hello.rejectedCaps.every((cap) => typeof cap === 'string')));
}

function clearRustHandshakeTimer(): void {
  if (_wsHandshakeTimer) clearTimeout(_wsHandshakeTimer);
  _wsHandshakeTimer = null;
}

function noteWsFailure(reasonKey: string): void {
  setStatus('status.disconnected', false);
  useStore.setState({ wsFailureReasonKey: reasonKey, wsRecoveryNotice: false });
}

/** 服务重启时先废弃旧连接；等待本轮有效端口和令牌后再重新连接。 */
export function disconnectWebSocketForRestart(): void {
  _wsConnectAttempt++;
  clearRustHandshakeTimer();
  if (_wsRetryTimer) clearTimeout(_wsRetryTimer);
  _wsRetryTimer = null;
  if (_ws) {
    const previous = _ws;
    _ws = null;
    previous.onclose = null;
    try { previous.close(); } catch { /* 已关闭的旧连接 */ }
    if (!_wsConnectionIsRust) noteComposerConnectionClosed();
  }
  _wsRetryCount = 0;
  _wsRetryDelay = 1000;
  noteWsFailure('status.serverRestarting');
  useStore.setState({ wsState: 'disconnected', wsReconnectAttempt: 0 });
}

/** 获取当前 WebSocket 实例 */
export function getWebSocket(): WebSocket | null {
  return _ws;
}

/** 发起 WebSocket 连接 */
export function connectWebSocket(port?: string, token?: string): void {
  const attempt = ++_wsConnectAttempt;
  // 如果没有传参，从 Zustand store 获取
  const storeState = useStore.getState();
  let connection: ServerConnection | null;
  try {
    connection = port !== undefined || token !== undefined
      ? createLocalServerConnection({
          serverPort: port !== undefined ? port : storeState.serverPort,
          serverToken: token !== undefined ? token : storeState.serverToken,
        })
      : resolveServerConnection(storeState);
  } catch {
    disconnectWebSocketForRestart();
    noteWsFailure('status.wsConnectionFailed');
    return;
  }

  if (!connection) {
    disconnectWebSocketForRestart();
    noteWsFailure('status.wsConnectionFailed');
    useStore.setState({ wsState: 'disconnected' });
    return;
  }
  if (connection.serverNodeKind === 'lingxi-service') {
    setStatus('status.disconnected', false);
    useStore.setState({ wsState: 'reconnecting', wsRecoveryNotice: false });
  }
  ensureResourceForegroundCatchUp();

  void openConnectionWebSocket(connection, attempt).catch((err) => {
    if (attempt !== _wsConnectAttempt) return;
    console.error('[ws] connection setup failed:', err);
    errorBus.report(new AppError('WS_DISCONNECTED'));
    if (!useStore.getState().wsFailureReasonKey) noteWsFailure('status.wsConnectionFailed');
    scheduleReconnect();
  });
}

function ensureResourceForegroundCatchUp(): void {
  if (_resourceForegroundCatchUpCleanup) return;
  _resourceForegroundCatchUpCleanup = bindResourceEventForegroundCatchUp((event) => handleServerMessage(event));
}

async function openConnectionWebSocket(connection: ServerConnection, attempt: number): Promise<void> {
  const isRustService = connection.serverNodeKind === 'lingxi-service';
  let rustHandshakeComplete = false;

  if (_wsRetryTimer) { clearTimeout(_wsRetryTimer); _wsRetryTimer = null; }
  clearRustHandshakeTimer();
  if (_ws) {
    try { _ws.onclose = null; _ws.close(); } catch { /* 已失效的旧连接 */ }
    if (!_wsConnectionIsRust) noteComposerConnectionClosed();
    _ws = null;
  }
  if (connection.kind === 'local' && connection.credentialKind === 'loopback_token' && !connection.token) {
    noteWsFailure('status.wsTicketFailed');
    throw new Error('local server token missing');
  }
  let wsTicket: string | null;
  try {
    wsTicket = await requestConnectionWsTicket(connection);
    if (isRustService && connection.kind !== 'local' && !wsTicket) {
      throw new Error('Rust remote WebSocket ticket missing');
    }
  } catch (err) {
    if (attempt === _wsConnectAttempt) noteWsFailure('status.wsTicketFailed');
    throw err;
  }
  if (attempt !== _wsConnectAttempt) return;

  const url = buildConnectionWsUrl(connection, isRustService ? '/lingxi/v1/ws' : '/ws', { wsTicket });
  _ws = new WebSocket(url);
  _wsConnectionIsRust = isRustService;
  const socket = _ws;

  const failRustSocket = (reasonKey: string, code: number, reason: string) => {
    if (_ws !== socket) return;
    clearRustHandshakeTimer();
    noteWsFailure(reasonKey);
    try { socket.close(code, reason); } catch { /* 浏览器会继续走重连 */ }
    scheduleReconnect();
  };

  const markConnectionReady = () => {
    if (_ws !== socket) return;
    clearRustHandshakeTimer();
    const recovered = _wsRetryCount > 0 || Boolean(useStore.getState().wsFailureReasonKey);
    _wsRetryDelay = 1000;
    _wsRetryCount = 0;
    setStatus(isRustService ? 'status.rustCoreUnavailable' : 'status.connected', !isRustService);
    useStore.setState({
      wsState: 'connected',
      wsReconnectAttempt: 0,
      wsFailureReasonKey: isRustService ? 'status.rustCoreUnavailable' : null,
      wsRecoveryNotice: recovered,
      compactingSessions: [],
      compactionModeBySession: {},
    });

    if (isRustService) return;
    // 只有旧服务协议可接收当前桌面聊天发送，Rust 仅声明其 wire 连接已就绪。
    noteComposerConnectionOpened();
    const s = useStore.getState();
    requestTerminalSnapshotForCurrentSession(s);
    const streamingPaths = [...new Set([...resolveStreamingSessionResumeTargets(s), ...unresolvedComposerSessionPaths(composerOriginConnectionKey(connection))])];
    reconcilePendingComposerSessions();
    if (streamingPaths.length > 0) {
      const myVersion = ++_wsResumeVersion;
      Promise.resolve().then(async () => {
        if (myVersion !== _wsResumeVersion) return;
        for (const targetPath of streamingPaths) {
          requestStreamResume(targetPath);
        }
      }).catch((err) => {
        console.error('[ws] reconnect resume failed:', err);
      });
    }

    // 重连后无条件刷新 ContextRing：覆盖 models-changed IPC 在 WS 关闭窗口
    // 期内到达、服务端重启、长时间挂起后唤醒等所有可能造成 context 数据
    // 与后端实际状态偏离的场景。不依赖 _pendingContextRefresh 队列。
    if (s.currentSessionPath && _ws?.readyState === WebSocket.OPEN) {
      _ws.send(JSON.stringify({
        type: 'context_usage',
        sessionPath: s.currentSessionPath,
        ...(s.currentSessionId ? { sessionId: s.currentSessionId } : {}),
      }));
    }

    void catchUpResourceEventsAfterReconnect((event) => handleServerMessage(event)).catch((err) => {
      console.warn('[ws] resource event catch-up failed:', err);
    });
  };

  _ws.onopen = () => {
    if (_ws !== socket) return;
    if (!isRustService) {
      markConnectionReady();
      return;
    }
    try {
      socket.send(JSON.stringify({
        protocol: 'lingxi.wire',
        clientKind: 'desktop',
        clientVersion: desktopVersion,
        protocolMin: RUST_WIRE_PROTOCOL,
        protocolMax: RUST_WIRE_PROTOCOL,
        caps: [],
      }));
    } catch {
      failRustSocket('status.wsConnectionFailed', 4400, 'Rust handshake send failed');
      return;
    }
    _wsHandshakeTimer = setTimeout(() => {
      if (!rustHandshakeComplete) {
        failRustSocket('status.wsHandshakeTimedOut', 4408, 'Rust wire handshake timeout');
      }
    }, RUST_HANDSHAKE_TIMEOUT_MS);
    (_wsHandshakeTimer as unknown as { unref?: () => void }).unref?.();
  };

  _ws.onmessage = (event: MessageEvent) => {
    if (_ws !== socket) return;
    try {
      const msg = JSON.parse(event.data);
      if (isRustService && !rustHandshakeComplete) {
        if (!isRustWireServerHello(msg)) {
          failRustSocket('status.wsHandshakeFailed', 4409, 'incompatible Rust wire handshake');
          return;
        }
        rustHandshakeComplete = true;
        markConnectionReady();
        return;
      }
      if (isRustService) return;
      recordResourceEventCursor(msg);
      handleServerMessage(msg, composerOriginConnectionKey(connection));
    } catch (err) {
      console.error('[ws] message parse error:', err);
      if (isRustService && !rustHandshakeComplete) {
        failRustSocket('status.wsHandshakeFailed', 4409, 'invalid Rust wire handshake');
      }
    }
  };

  _ws.onclose = () => {
    if (_ws !== socket) return;
    clearRustHandshakeTimer();
    if (!useStore.getState().wsFailureReasonKey) noteWsFailure('status.wsConnectionClosed');
    else setStatus('status.disconnected', false);
    // 断连后再不会有后续事件来清「等待助手」pending；streamingSessions 保留
    // （重连 resume 靠它圈目标），pending 必须就地全清，否则挂出永久指示器。
    useStore.getState().clearAllTurnPending?.();
    // 在途发送的回执随连接消失：标记 delivery_unknown（禁止自动重发），
    // 连接代次递增使旧代次的准备任务在提交时被拒绝。
    if (!isRustService) noteComposerConnectionClosed();
    scheduleReconnect();
  };

  _ws.onerror = () => {
    errorBus.report(new AppError('WS_DISCONNECTED'));
    if (isRustService) {
      failRustSocket('status.wsConnectionFailed', 4400, 'Rust connection error');
    } else {
      noteWsFailure('status.wsConnectionFailed');
    }
  };
}

function scheduleReconnect(): void {
  if (_wsRetryTimer) return;
  _wsRetryCount++;

  useStore.setState({ wsState: 'reconnecting', wsReconnectAttempt: _wsRetryCount });
  if (_wsRetryCount <= WS_FAST_RETRY_LIMIT) {
    _wsRetryTimer = setTimeout(() => connectWebSocket(), _wsRetryDelay);
    _wsRetryDelay = Math.min(_wsRetryDelay * 2, WS_RETRY_MAX);
  } else {
    _wsRetryTimer = setTimeout(() => connectWebSocket(), WS_SLOW_RETRY_DELAY);
  }
  (_wsRetryTimer as unknown as { unref?: () => void })?.unref?.();
}

/** 手动重连（由 StatusBar 重连按钮调用），重置重试计数 */
export function manualReconnect(): void {
  _wsRetryCount = 0;
  connectWebSocket();
}
