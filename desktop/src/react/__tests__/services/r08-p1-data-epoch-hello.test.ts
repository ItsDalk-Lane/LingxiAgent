/**
 * R08-P1 客户端数据纪元对账：hello.dataEpoch 接受/拒绝语义钉死。
 *
 * 背景：T05 已批准 Rust 侧 R08_CUTOVER 把 data_epoch 真实升到 2（独立常量，
 * shared/contract-versions.json 不动）。客户端握手校验从严格相等改为显式枚举
 * 接受 {1, 2}；窗外纪元仍拒绝，且拒绝理由区分「过旧/过新」两方向
 * （过新 = 数据目录已被更新版本升级，提示升级客户端而不是继续写入 —— A11）。
 *
 * 本文件只测纪元语义；不复用也不断言 R02RustWebSocket.test.ts 的 mock 形状
 * （其基线红根因是 session-connection-owner stub 形状，与 epoch 无关）。
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';

const fixture = vi.hoisted(() => {
  const state: Record<string, unknown> = {};
  const connection = {
    connectionId: 'lan:rust-server:studio-rust', kind: 'lan', serverId: 'rust-server',
    serverNodeId: 'rust-server', serverNodeKind: 'lingxi-service', studioId: 'studio-rust',
    label: 'Rust Server', baseUrl: 'https://192.168.31.75:14500',
    wsUrl: 'wss://192.168.31.75:14500', token: 'fixture-key', authState: 'paired',
    trustState: 'lan', credentialKind: 'device_credential', capabilities: ['chat'],
  };
  return { state, connection, setStatus: vi.fn(), requestTicket: vi.fn(async () => 'hana_ws_fixture') };
});

vi.mock('../../stores', () => ({
  useStore: {
    getState: () => fixture.state,
    setState: (patch: Record<string, unknown>) => Object.assign(fixture.state, patch),
  },
}));
vi.mock('../../utils/ui-helpers', () => ({ setStatus: fixture.setStatus }));
vi.mock('../../services/server-connection', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../services/server-connection')>();
  return {
    ...actual,
    resolveServerConnection: () => fixture.connection,
    requestConnectionWsTicket: fixture.requestTicket,
  };
});
vi.mock('../../services/resource-events', () => ({
  bindResourceEventForegroundCatchUp: () => () => {},
  catchUpResourceEventsAfterReconnect: vi.fn(async () => {}),
  recordResourceEventCursor: vi.fn(),
}));
vi.mock('../../services/composer-send-coordinator', () => ({
  composerOriginConnectionKey: () => 'rust-fixture',
  reconcilePendingComposerSessions: vi.fn(),
  unresolvedComposerSessionPaths: () => [],
  noteComposerConnectionClosed: vi.fn(),
  noteComposerConnectionOpened: vi.fn(),
}));
vi.mock('../../services/session-connection-owner', () => ({
  captureSessionConnection: () => null,
}));
vi.mock('../../services/rust-automation', () => ({
  startRustAutomationNotifications: () => () => {},
}));
vi.mock('../../services/rust-media', () => ({ startRustMediaPolling: () => () => {} }));
vi.mock('../../services/rust-workbench', () => ({ startRustResourceEvents: () => () => {} }));

import {
  connectWebSocket,
  isRustWireServerHello,
  rustWireHelloDataEpochDirection,
} from '../../services/websocket';

class FakeSocket {
  static readonly OPEN = 1;
  static instances: FakeSocket[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  readyState = FakeSocket.OPEN;
  send = vi.fn();
  close = vi.fn();

  constructor(readonly url: string) { FakeSocket.instances.push(this); }
}

function validHello(dataEpoch: unknown): Record<string, unknown> {
  return {
    protocol: 'lingxi.wire', selectedProtocol: 1,
    wireProtocolMin: 1, wireProtocolMax: 1,
    dataEpoch,
    serverKind: 'lingxi-service', serverVersion: '0.1.0',
  };
}

async function handshakeWith(hello: Record<string, unknown>, socketIndex = 0): Promise<FakeSocket> {
  connectWebSocket();
  // 票据 await 之后才创建 socket（异步）；陈旧 attempt 在建 socket 前被丢弃，
  // 所以每次有效 connect 恰好新增一个实例。
  await vi.waitFor(() => expect(FakeSocket.instances.length).toBeGreaterThan(socketIndex));
  const socket = FakeSocket.instances[socketIndex];
  socket.onopen?.();
  socket.onmessage?.({ data: JSON.stringify(hello) });
  return socket;
}

describe('R08-P1 data epoch 握手对账', () => {
  beforeEach(() => {
    FakeSocket.instances = [];
    Object.keys(fixture.state).forEach((key) => delete fixture.state[key]);
    fixture.setStatus.mockClear();
    fixture.requestTicket.mockReset();
    fixture.requestTicket.mockResolvedValue('hana_ws_fixture');
    vi.stubGlobal('WebSocket', FakeSocket);
  });

  describe('isRustWireServerHello 纯校验：显式枚举 {1, 2}', () => {
    it.each([1, 2])('dataEpoch=%i 接受', (epoch) => {
      expect(isRustWireServerHello(validHello(epoch))).toBe(true);
      expect(rustWireHelloDataEpochDirection(epoch)).toBeNull();
    });

    it('dataEpoch=0 拒绝且方向为 too_old', () => {
      expect(isRustWireServerHello(validHello(0))).toBe(false);
      expect(rustWireHelloDataEpochDirection(0)).toBe('too_old');
    });

    it('dataEpoch=3 拒绝且方向为 too_new（提示升级而不是继续写入）', () => {
      expect(isRustWireServerHello(validHello(3))).toBe(false);
      expect(rustWireHelloDataEpochDirection(3)).toBe('too_new');
    });

    it.each([
      ['缺失', undefined],
      ['非整数', 1.5],
      ['字符串', '2'],
      ['null', null],
    ])('dataEpoch %s 拒绝但不归纪元方向（通用不兼容理由）', (_label, epoch) => {
      expect(isRustWireServerHello(validHello(epoch))).toBe(false);
      expect(rustWireHelloDataEpochDirection(epoch)).toBeNull();
    });
  });

  describe('握手集成：连接面 observable 行为', () => {
    it('dataEpoch=2（R08_CUTOVER 升级后服务）握手成功进入 connected', async () => {
      await handshakeWith(validHello(2));
      await vi.waitFor(() => expect(fixture.state.wsState).toBe('connected'));
      expect(fixture.state.wsFailureReasonKey).toBeNull();
    });

    it('dataEpoch=1（切换前服务）握手仍成功进入 connected', async () => {
      await handshakeWith(validHello(1));
      await vi.waitFor(() => expect(fixture.state.wsState).toBe('connected'));
    });

    it('dataEpoch=0 拒绝：4409 + too_old 理由，绝不进入 connected', async () => {
      const socket = await handshakeWith(validHello(0));
      expect(socket.close).toHaveBeenCalledWith(
        4409, 'Rust data epoch too old for this client (accepted: 1,2)');
      expect(fixture.state.wsFailureReasonKey).toBe('status.wsHandshakeFailed');
      expect(fixture.state.wsState).toBe('reconnecting');
      expect(fixture.setStatus).not.toHaveBeenCalledWith('status.connected', true);
    });

    it('dataEpoch=3 拒绝：4409 + too_new 理由（升级提示方向），绝不进入 connected', async () => {
      const socket = await handshakeWith(validHello(3));
      expect(socket.close).toHaveBeenCalledWith(
        4409, 'Rust data epoch too new for this client (accepted: 1,2); upgrade the client');
      expect(fixture.state.wsFailureReasonKey).toBe('status.wsHandshakeFailed');
      expect(fixture.state.wsState).toBe('reconnecting');
      expect(fixture.setStatus).not.toHaveBeenCalledWith('status.connected', true);
    });

    it('过旧与过新的关闭理由必须可区分', async () => {
      const oldSocket = await handshakeWith(validHello(0), 0);
      const newSocket = await handshakeWith(validHello(3), 1);
      const oldReason = oldSocket.close.mock.calls[0]?.[1];
      const newReason = newSocket.close.mock.calls[0]?.[1];
      expect(oldReason).toContain('too old');
      expect(newReason).toContain('too new');
      expect(oldReason).not.toBe(newReason);
    });
  });
});
