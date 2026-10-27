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
  return { state, connection, setStatus: vi.fn(), composerOpened: vi.fn(), requestTicket: vi.fn(async () => 'hana_ws_fixture') };
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
  noteComposerConnectionOpened: fixture.composerOpened,
}));

import { connectWebSocket } from '../../services/websocket';

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

describe('Rust desktop WebSocket connection', () => {
  beforeEach(() => {
    FakeSocket.instances = [];
    Object.keys(fixture.state).forEach((key) => delete fixture.state[key]);
    fixture.setStatus.mockClear();
    fixture.composerOpened.mockClear();
    fixture.requestTicket.mockReset();
    fixture.requestTicket.mockResolvedValue('hana_ws_fixture');
    vi.stubGlobal('WebSocket', FakeSocket);
  });

  it('stays offline until the Rust wire handshake succeeds and never opens the Node chat composer', async () => {
    connectWebSocket();
    await vi.waitFor(() => expect(FakeSocket.instances).toHaveLength(1));
    const socket = FakeSocket.instances[0];
    expect(socket.url).toBe('wss://192.168.31.75:14500/lingxi/v1/ws?wsTicket=hana_ws_fixture');
    expect(fixture.state.wsState).toBe('reconnecting');
    socket.onopen?.();
    expect(JSON.parse(socket.send.mock.calls[0][0])).toMatchObject({
      protocol: 'lingxi.wire', clientKind: 'desktop', protocolMin: 1, protocolMax: 1,
    });
    expect(fixture.state.wsState).toBe('reconnecting');
    socket.onmessage?.({ data: JSON.stringify({ protocol: 'lingxi.wire', selectedProtocol: 1,
      wireProtocolMin: 1, wireProtocolMax: 1, dataEpoch: 1,
      serverKind: 'lingxi-service', serverVersion: '0.1.0' }) });
    expect(fixture.state.wsState).toBe('connected');
    expect(fixture.composerOpened).not.toHaveBeenCalled();
  });

  it('rejects an incompatible handshake without showing a connected state', async () => {
    connectWebSocket();
    await vi.waitFor(() => expect(FakeSocket.instances).toHaveLength(1));
    const socket = FakeSocket.instances[0];
    socket.onopen?.();
    socket.onmessage?.({ data: JSON.stringify({ protocol: 'lingxi.wire', selectedProtocol: 2,
      serverKind: 'lingxi-service' }) });
    expect(socket.close).toHaveBeenCalledWith(4409, 'incompatible Rust wire handshake');
    expect(fixture.state.wsState).toBe('reconnecting');
    expect(fixture.setStatus).not.toHaveBeenCalledWith('status.connected', true);
    expect(fixture.state.wsFailureReasonKey).toBe('status.wsHandshakeFailed');
  });

  it('does not open a WebSocket when ticket issuance fails', async () => {
    fixture.requestTicket.mockRejectedValueOnce(new Error('websocket ticket response missing ticket'));
    connectWebSocket();
    await vi.waitFor(() => expect(fixture.state.wsFailureReasonKey).toBe('status.wsTicketFailed'));
    expect(FakeSocket.instances).toHaveLength(0);
    expect(fixture.state.wsState).toBe('reconnecting');
  });

  it('rejects a partial success hello instead of showing a false connected state', async () => {
    connectWebSocket();
    await vi.waitFor(() => expect(FakeSocket.instances).toHaveLength(1));
    const socket = FakeSocket.instances[0];
    socket.onopen?.();
    socket.onmessage?.({ data: JSON.stringify({ protocol: 'lingxi.wire', selectedProtocol: 1,
      serverKind: 'lingxi-service' }) });
    expect(socket.close).toHaveBeenCalledWith(4409, 'incompatible Rust wire handshake');
    expect(fixture.state.wsState).toBe('reconnecting');
    expect(fixture.state.wsFailureReasonKey).toBe('status.wsHandshakeFailed');
  });

  it('times out a socket that never returns ServerHello', async () => {
    connectWebSocket();
    await vi.waitFor(() => expect(FakeSocket.instances).toHaveLength(1));
    const socket = FakeSocket.instances[0];
    vi.useFakeTimers();
    socket.onopen?.();
    vi.advanceTimersToNextTimer();
    expect(socket.close).toHaveBeenCalledWith(4408, 'Rust wire handshake timeout');
    expect(fixture.state.wsFailureReasonKey).toBe('status.wsHandshakeTimedOut');
    expect(fixture.state.wsState).toBe('reconnecting');
    vi.useRealTimers();
  });

  it('never reuses stored local transport when an explicit port or token is invalid', async () => {
    fixture.state.serverPort = '62950';
    fixture.state.serverToken = 'stale-token';
    connectWebSocket('0', 'new-token');
    expect(FakeSocket.instances).toHaveLength(0);
    expect(fixture.state.wsState).toBe('disconnected');

    connectWebSocket('62950', '');
    await vi.waitFor(() => expect(fixture.state.wsFailureReasonKey).toBe('status.wsTicketFailed'));
    expect(FakeSocket.instances).toHaveLength(0);
  });
});
