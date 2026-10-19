// @vitest-environment jsdom
import React from 'react';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useSettingsStore } from '../../settings/store';
import { useStore } from '../../stores';
import { useBridgeState } from '../../settings/tabs/bridge/useBridgeState';
import { WechatQrcodeOverlay } from '../../settings/overlays/WechatQrcodeOverlay';
import { BridgePanel } from '../../components/BridgePanel';
import { SessionRegistryFilesPanel } from '../../components/right-workspace/SessionRegistryFilesPanel';
import { bridgeMediaOutcome, bridgeMediaRequestId, finishBridgeMediaAction } from '../../services/bridge-media-action';

const connection = (token = 'owner-token') => ({ connectionId: 'local', kind: 'local', serverId: 'local', serverNodeId: 'native', serverNodeKind: 'lingxi-service', studioId: 'local', userId: 'user_local', label: '本机', baseUrl: 'http://127.0.0.1:3210', wsUrl: 'ws://127.0.0.1:3210', token, authState: 'paired', trustState: 'local', credentialKind: 'loopback_token', capabilities: ['chat', 'files'] });
const status = () => ({ agentId: 'hana', telegram: { enabled: true, token: '***', configured: true, status: 'connected' }, feishu: { configured: true, enabled: true, status: 'connected' }, qq: {}, dingtalk: {}, wechat: {}, owner: {}, knownUsers: {}, permissionMode: 'auto', readOnly: false, receiptEnabled: true, richStreamingEnabled: true });
const response = (value: unknown, code = 200) => new Response(JSON.stringify(value), { status: code, headers: { 'Content-Type': 'application/json' } });
const fetchMock = vi.fn(); const toast = vi.fn();
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(done => { resolve = done; }); return { promise, resolve }; }

function Probe() {
  const bridge = useBridgeState();
  return <div><span data-testid="selected">{bridge.selectedAgentId}</span><span data-testid="status">{bridge.status?.telegram.status}</span><span role="alert">{bridge.statusError}</span>
    <button onClick={() => bridge.loadStatus()}>reload</button>
    <button onClick={() => bridge.saveBridgeConfig('telegram', { token: 'candidate' }, true)}>save</button>
    <button onClick={() => bridge.testPlatform('telegram', {}, true)}>verify</button>
    <button onClick={() => bridge.setOwner('telegram', '')}>clear owner</button></div>;
}

beforeEach(() => {
  localStorage.setItem('hana_bridge_tab', 'feishu');
  window.t = ((key: string) => key) as any;
  window.matchMedia = vi.fn().mockReturnValue({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() });
  const source = connection();
  useStore.setState({ currentAgentId: 'hana', agents: [{ id: 'hana', name: 'Hana' }], activePanel: 'bridge', serverConnections: { local: source }, activeServerConnectionId: 'local', bridgeLatestMessage: null } as never);
  // 连接所有者会在连接切换时同步清空旧助手；先安装连接，再加载其权威资料。
  useSettingsStore.setState({ serverConnections: { local: source }, activeServerConnectionId: 'local' } as never);
  useSettingsStore.setState({ ready: true, currentAgentId: 'hana', agents: [{ id: 'hana', name: 'Hana' }], serverConnections: { local: source }, activeServerConnectionId: 'local', showToast: toast,
    settingsSnapshot: { status: 'ready', data: { agentId: 'hana', bridgeStatus: status(), publicAgents: '' } } } as never);
  fetchMock.mockReset().mockImplementation(async (url: string) => {
    if (new URL(url).pathname === '/lingxi/v1/bridge/status') return response(status());
    if (new URL(url).pathname === '/lingxi/v1/bridge/sessions') return response({ sessions: [{ sessionKey: 'bridge-key', chatId: 'chat', sessionPath: 'native:session', displayName: '已有联系人' }] });
    // 权威历史加载同时读取当前媒体任务，空列表不替代原历史/失败保留断言。
    if (new URL(url).pathname === '/lingxi/v1/sessions/session/media/jobs') return response({ jobs: [] });
    throw new Error(`unmapped Rust UI route: ${new URL(url).pathname}`);
  });
  toast.mockReset(); vi.stubGlobal('fetch', fetchMock);
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

describe('R07 原生消息平台页面，真实UI与HTTP协议边界', () => {
  it('状态读取失败保留已有状态并显示原因，HTTP 200 error也不能覆盖成空状态', async () => {
    expect(useSettingsStore.getState().currentAgentId).toBe('hana');
    render(<Probe />); await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    fetchMock.mockResolvedValueOnce(response({ error: '平台暂时不可用' }));
    fireEvent.click(screen.getByText('reload'));
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('平台暂时不可用'));
    expect(screen.getByTestId('status')).toHaveTextContent('connected');
    expect(fetchMock.mock.calls.every(([url]) => String(url).includes('/lingxi/v1/'))).toBe(true);
    expect(fetchMock.mock.calls[0][1].headers).toMatchObject({ Authorization: 'Bearer owner-token' });
  });

  it('保存、验证和清除主人均走原生路由，业务失败不能弹成功提示', async () => {
    render(<Probe />); await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    for (const button of ['save', 'verify', 'clear owner']) {
      fetchMock.mockResolvedValueOnce(response({ ok: false, error: '真实失败' }));
      fireEvent.click(screen.getByText(button));
      await waitFor(() => expect(toast).toHaveBeenCalledWith(expect.stringContaining('真实失败'), 'error'));
      toast.mockClear();
    }
    const requests = fetchMock.mock.calls.filter(([, init]) => init?.method === 'POST');
    expect(requests.map(([url]) => new URL(url).pathname)).toEqual(['/lingxi/v1/bridge/config', '/lingxi/v1/bridge/test', '/lingxi/v1/bridge/owner']);
    expect(JSON.parse(requests[2][1].body).userId).toBeNull();
  });

  it('令牌变化后丢弃同一助手的旧状态请求', async () => {
    render(<Probe />); await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    const old = deferred<Response>(); fetchMock.mockReturnValueOnce(old.promise);
    fireEvent.click(screen.getByText('reload'));
    act(() => useSettingsStore.setState({ serverConnections: { local: connection('new-token') } } as never));
    await waitFor(() => expect(fetchMock.mock.calls.some(([, init]) => init.headers.Authorization === 'Bearer new-token')).toBe(true));
    await act(async () => old.resolve(response({ ...status(), telegram: { status: 'old-identity' } })));
    expect(screen.getByTestId('status')).toHaveTextContent('connected');
  });

  it('联系人刷新失败保留列表并明确显示错误', async () => {
    render(<BridgePanel />);
    await screen.findByText('已有联系人');
    fetchMock.mockRejectedValue(new Error('会话读取失败'));
    act(() => useStore.getState().addBridgeMessage({ platform: 'feishu', sessionKey: 'bridge-key', direction: 'in', sender: 'user', text: 'new', isGroup: false, ts: 1, agentId: 'hana' }));
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('会话读取失败'));
    expect(screen.getByText('已有联系人')).toBeInTheDocument();
  });

  it('会话详情读取原生权威历史，重置失败保留已读内容和会话', async () => {
    const original = fetchMock.getMockImplementation()!;
    fetchMock.mockImplementation(async (url, init) => {
      const path = new URL(url).pathname;
      if (path.endsWith('/history')) return new Response(JSON.stringify({ schemaVersion: 'lingxi.canonical-history.v1', sessionId: 'session', revision: 'rev-1', items: [{ kind: 'user_message', messageId: 'native-user-message', seq: '1', committedAtUnixMs: 1, text: '真实历史正文', run: { state: 'unknown', reason: 'legacy' } }], page: { hasMore: false, nextBefore: null } }), { headers: { ETag: 'rev-1' } });
      if (path.endsWith('/reset')) return response({ ok: false, error: '正在运行，不能重置' });
      return original(url, init);
    });
    render(<BridgePanel />); fireEvent.click(await screen.findByText('已有联系人'));
    await screen.findByText('真实历史正文');
    fireEvent.click(screen.getByText('bridge.resetContext'));
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('正在运行'));
    expect(screen.getByText('真实历史正文')).toBeInTheDocument();
    expect(fetchMock.mock.calls.some(([url]) => new URL(url).pathname === '/lingxi/v1/sessions/session/history')).toBe(true);
    expect(fetchMock.mock.calls.some(([url]) => new URL(url).pathname === '/lingxi/v1/bridge/sessions/bridge-key/reset')).toBe(true);
  });

  it('微信先停用保存，再绑定主人，最后启用；主人失败时保持停用并显示错误', async () => {
    const writes: any[] = [];
    fetchMock.mockImplementation(async (url, init) => {
      const path = new URL(url).pathname;
      if (path.endsWith('/qrcode')) return response({ ok: true, qrcodeUrl: 'data:image/png;base64,AA==', qrcodeId: 'qr' });
      if (path.endsWith('/qrcode-status')) return response({ status: 'confirmed', botToken: 'test-token', userId: 'owner-id' });
      writes.push({ path, body: JSON.parse(init.body) });
      return path.endsWith('/owner') ? response({ error: '主人绑定失败' }, 409) : response({ ok: true });
    });
    render(<WechatQrcodeOverlay />);
    act(() => window.dispatchEvent(new CustomEvent('hana-show-wechat-qrcode', { detail: { agentId: 'hana' } })));
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('主人绑定失败'));
    expect(writes.map(row => [row.path, row.body.enabled])).toEqual([['/lingxi/v1/bridge/config', false], ['/lingxi/v1/bridge/owner', undefined]]);
    expect(screen.queryByText('settings.bridge.wechatLoginSuccess')).not.toBeInTheDocument();
    expect(fetchMock.mock.calls.every(([url]) => new URL(url).searchParams.get('agentId') === 'hana')).toBe(true);
  });

  it('微信完整绑定确实成功后才显示成功', async () => {
    const writes: any[] = [];
    fetchMock.mockImplementation(async (url, init) => {
      const path = new URL(url).pathname;
      if (path.endsWith('/qrcode')) return response({ ok: true, qrcodeUrl: 'data:image/png;base64,AA==', qrcodeId: 'qr' });
      if (path.endsWith('/qrcode-status')) return response({ status: 'confirmed', botToken: 'test-token', userId: 'owner-id' });
      writes.push({ path, body: JSON.parse(init.body) }); return response({ ok: true });
    });
    render(<WechatQrcodeOverlay />);
    act(() => window.dispatchEvent(new CustomEvent('hana-show-wechat-qrcode', { detail: { agentId: 'hana' } })));
    await screen.findByText('settings.bridge.wechatLoginSuccess');
    expect(writes.map(row => [row.path, row.body.enabled])).toEqual([['/lingxi/v1/bridge/config', false], ['/lingxi/v1/bridge/owner', undefined], ['/lingxi/v1/bridge/config', true]]);
  });

  it('扫码期间连接切换后不能把旧凭据写到新连接', async () => {
    const confirmation = deferred<Response>();
    fetchMock.mockImplementation(async (url) => new URL(url).pathname.endsWith('/qrcode')
      ? response({ ok: true, qrcodeUrl: 'data:image/png;base64,AA==', qrcodeId: 'qr' }) : confirmation.promise);
    render(<WechatQrcodeOverlay />);
    act(() => window.dispatchEvent(new CustomEvent('hana-show-wechat-qrcode', { detail: { agentId: 'hana' } })));
    await waitFor(() => expect(fetchMock.mock.calls.some(([url]) => String(url).includes('qrcode-status'))).toBe(true));
    act(() => useSettingsStore.setState({ serverConnections: { local: connection('other-token') } } as never));
    await act(async () => confirmation.resolve(response({ status: 'confirmed', botToken: 'old-token', userId: 'old-owner' })));
    expect(fetchMock.mock.calls.some(([url]) => String(url).includes('/config'))).toBe(false);
    expect(screen.getByRole('alert')).toHaveTextContent('连接已切换');
  });

  it('Unknown与排队结果都不能当成功，重试保留同一个操作编号', () => {
    const action = 'synthetic-test-action';
    const id = bridgeMediaRequestId(action, 'sf_a');
    expect(() => bridgeMediaOutcome({ ok: true, status: 'unknown' }, true)).toThrow('尚未确认');
    expect(bridgeMediaRequestId(action, 'sf_a')).toBe(id);
    expect(bridgeMediaOutcome({ ok: true, status: 'queued' }, true)).toBe('pending');
    expect(bridgeMediaRequestId(action, 'sf_a')).toBe(id);
    expect(() => bridgeMediaOutcome({ ok: true, status: 'confirmed' }, true)).toThrow('有效');
    expect(bridgeMediaOutcome({ ok: true, status: 'confirmed', receipt: { messageIds: ['platform-native-id'] } }, true)).toBe('confirmed');
    finishBridgeMediaAction(action);
    expect(bridgeMediaRequestId(action, 'sf_a')).not.toBe(id);
    finishBridgeMediaAction(action);
  });

  it('真实文件菜单以fileId发原生请求，网络失败重试沿同一编号并查询平台回执', async () => {
    const addToast = vi.fn(); let sends = 0;
    useStore.setState({ currentSessionPath: 'native:source', currentSessionId: 'source', addToast } as never);
    useStore.getState().setSessionRegistryFiles('native:source', [{ fileId: 'sf_file', filePath: 'resource:sf_file', label: 'note.txt', status: 'available' }]);
    fetchMock.mockImplementation(async (url, init) => {
      const target = new URL(url); const path = target.pathname;
      if (path.endsWith('/sessions')) return response({ sessions: target.searchParams.get('platform') === 'feishu' ? [{ sessionKey: 'canonical-key', chatId: 'chat', displayName: '原生联系人' }] : [] });
      if (path.endsWith('/send-media')) { if (++sends === 1) throw new Error('连接中断'); return response({ ok: true, status: 'queued', deliveryId: 'delivery-native' }); }
      if (path.includes('/deliveries/')) return response({ ok: true, status: 'confirmed', deliveryId: 'delivery-native', receipt: { platform: 'feishu', messageIds: ['real-platform-id'] } });
      throw new Error(`unexpected native file UI route ${path}`);
    });
    render(<SessionRegistryFilesPanel />);
    const choose = async () => {
      fireEvent.contextMenu(screen.getByText('note.txt'));
      await waitFor(() => expect(fetchMock.mock.calls.filter(([url]) => String(url).includes('/bridge/sessions')).length).toBeGreaterThanOrEqual(5));
      fireEvent.mouseEnter(screen.getByText('rightWorkspace.sessionFiles.actions.sendToBridge'));
      fireEvent.click(await screen.findByText(/原生联系人/));
    };
    await choose(); await waitFor(() => expect(addToast).toHaveBeenCalledWith(expect.any(String), 'error'));
    expect(addToast.mock.calls.some(call => call[1] === 'success')).toBe(false);
    await choose(); await waitFor(() => expect(addToast).toHaveBeenCalledWith(expect.any(String), 'success'));
    const sendCalls = fetchMock.mock.calls.filter(([url]) => String(url).includes('/send-media'));
    expect(sendCalls).toHaveLength(2);
    const first = JSON.parse(sendCalls[0][1].body); const second = JSON.parse(sendCalls[1][1].body);
    expect(first).toEqual(second); expect(first).toMatchObject({ fileId: 'sf_file', sessionId: 'source', sessionKey: 'canonical-key', requestId: expect.any(String) });
    expect(first).not.toHaveProperty('filePath'); expect(first).not.toHaveProperty('sessionPath');
    expect(fetchMock.mock.calls.some(([url]) => String(url).includes('/api/'))).toBe(false);
    expect(fetchMock.mock.calls.some(([url]) => new URL(url).searchParams.get('platform') === 'whatsapp')).toBe(false);
  });
});
