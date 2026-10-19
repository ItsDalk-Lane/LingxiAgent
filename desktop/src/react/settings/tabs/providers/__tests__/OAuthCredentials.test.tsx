/** @vitest-environment jsdom */
import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { useSettingsStore, type ProviderSummary } from '../../../store';
import { createLocalServerConnection } from '../../../../services/server-connection';
const mocks = vi.hoisted(() => ({ request: vi.fn(), open: vi.fn(), toast: vi.fn() }));
vi.mock('../../../api', () => ({ lingxiFetchJson: (...args: unknown[]) => mocks.request(...args) }));
vi.mock('../../../helpers', () => ({ t: (key: string) => key }));
import { OAuthCredentials } from '../OAuthCredentials';
import { startOAuth, completeOAuth, pollOAuth, logoutOAuth } from '../oauth-service';
const summary = { logged_in: false } as ProviderSummary;
const future = () => Date.now() + 600_000;
const pkce = () => ({ ok: true, start: { flow: 'authorizationCodePkce', authorizeUrl: 'http://127.0.0.1:34567/authorize?state=original-state', expiresAtUnixMs: future() } });
const device = () => ({ ok: true, start: { flow: 'deviceCode', verificationUri: 'http://127.0.0.1:34567/device', userCode: 'LOCAL-CODE', intervalMs: 1000, expiresAtUnixMs: future() } });
const success = (persisted = true) => ({ ok: true, outcome: { status: 'loggedIn', persisted } });
function connection(id = 'local', kind = 'lingxi-service') {
  const connection = { ...createLocalServerConnection({ serverPort: 34567, serverToken: 'synthetic-test-token', serverNodeKind: kind, serverNodeTransport: 'http' })!, connectionId: id };
  useSettingsStore.setState({ serverConnections: { [id]: connection }, activeServerConnection: connection, activeServerConnectionId: id });
}
beforeEach(() => {
  vi.useRealTimers(); mocks.request.mockReset(); mocks.open.mockReset().mockResolvedValue(undefined); mocks.toast.mockReset();
  Object.defineProperty(window, 'platform', { configurable: true, value: { openExternal: mocks.open } });
  connection(); useSettingsStore.setState({ showToast: mocks.toast });
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

describe('实际 Rust OAuth 契约适配', () => {
  it('PKCE 使用正式 provider 路径与原 state，回调不发送旧 sessionId', async () => {
    mocks.request.mockResolvedValueOnce(pkce()).mockResolvedValueOnce(success());
    const attempt = await startOAuth('openai-codex-oauth', true, new AbortController().signal);
    expect(attempt.transaction).toBe('original-state');
    await completeOAuth(attempt, 'real-code', new AbortController().signal);
    expect(mocks.request.mock.calls[0][0]).toBe('/lingxi/v1/models/credentials/openai-codex-oauth/login');
    expect(mocks.request.mock.calls[1][0]).toBe('/lingxi/v1/models/credentials/openai-codex-oauth/login/callback');
    expect(JSON.parse(mocks.request.mock.calls[1][1].body)).toEqual({ state: 'original-state', code: 'real-code' });
  });
  it('device POST 轮询遵守真实 pending/slowDown 间隔和持久化结果', async () => {
    mocks.request.mockResolvedValueOnce(device()).mockResolvedValueOnce({ ok: true, outcome: { status: 'pending', slowDown: true, intervalMs: 6000 } }).mockResolvedValueOnce(success(false));
    const attempt = await startOAuth('xai-oauth', true, new AbortController().signal);
    expect(attempt.deviceCode).toBe('LOCAL-CODE');
    expect(await pollOAuth(attempt, new AbortController().signal)).toEqual({ done: false, persisted: false, intervalMs: 6000 });
    expect(await pollOAuth(attempt, new AbortController().signal)).toEqual({ done: true, persisted: false });
    expect(mocks.request.mock.calls[1][0]).toBe('/lingxi/v1/models/credentials/xai-oauth/login/poll');
    expect(mocks.request.mock.calls[1][1].method).toBe('POST');
  });
  it('未知、缺字段、已过期和脚本地址不能读成成功', async () => {
    for (const data of [{ ok: true, start: { flow: 'other' } }, { ok: true, start: { ...pkce().start, authorizeUrl: 'javascript:alert(1)' } }, { ok: true, start: { ...pkce().start, authorizeUrl: 'https://example.invalid' } }, { ok: true, start: { ...device().start, intervalMs: 0 } }, { ok: true, start: { ...pkce().start, expiresAtUnixMs: 1 } }]) {
      mocks.request.mockResolvedValueOnce(data);
      await expect(startOAuth('main', true, new AbortController().signal)).rejects.toThrow();
    }
    mocks.request.mockResolvedValueOnce(device()).mockResolvedValueOnce({ ok: true, outcome: {} });
    await expect(pollOAuth(await startOAuth('main', true, new AbortController().signal), new AbortController().signal)).rejects.toThrow();
  });
  it('保留旧 Node sessionId/start/callback/GET poll/logout 通路', async () => {
    mocks.request.mockResolvedValueOnce({ url: 'https://example.invalid/authorize', sessionId: 'node-session', polling: true }).mockResolvedValueOnce({ status: 'pending' }).mockResolvedValueOnce({ ok: true }).mockResolvedValueOnce({ ok: true });
    const attempt = await startOAuth('node-provider', false, new AbortController().signal);
    await pollOAuth(attempt, new AbortController().signal); await completeOAuth(attempt, 'code', new AbortController().signal); await logoutOAuth('node-provider', false, new AbortController().signal);
    expect(mocks.request.mock.calls.map(call => call[0])).toEqual(['/api/auth/oauth/start', '/api/auth/oauth/poll/node-session', '/api/auth/oauth/callback', '/api/auth/oauth/logout']);
    expect(mocks.request.mock.calls[1][1].method).toBeUndefined();
    expect(JSON.parse(mocks.request.mock.calls[2][1].body)).toEqual({ sessionId: 'node-session', code: 'code' });
  });
});

describe('当前设置组件的取消、撤销资料与真实结果提示', () => {
  it('系统明确拒绝打开浏览器时显示原因，不提示登录成功或开始轮询', async () => {
    mocks.request.mockResolvedValueOnce(device());
    mocks.open.mockResolvedValueOnce({ ok: false, error: '自有系统拒绝打开登录浏览器' });
    const refresh = vi.fn(async () => {});
    render(<OAuthCredentials providerId="main" summary={summary} onRefresh={refresh} />);
    fireEvent.click(screen.getByText('settings.oauth.login'));
    await waitFor(() => expect(mocks.toast).toHaveBeenCalledWith(expect.stringContaining('自有系统拒绝打开登录浏览器'), 'error'));
    expect(mocks.request).toHaveBeenCalledTimes(1);
    expect(refresh).not.toHaveBeenCalled();
  });
  it('用户确认授权码后才回调，并刷新状态', async () => {
    const refresh = vi.fn(async () => {}); mocks.request.mockResolvedValueOnce(pkce()).mockResolvedValueOnce(success());
    render(<OAuthCredentials providerId="main" summary={summary} onRefresh={refresh} />);
    fireEvent.click(screen.getByText('settings.oauth.login'));
    const input = await screen.findByPlaceholderText('settings.oauth.codePlaceholder');
    expect(mocks.request).toHaveBeenCalledTimes(1);
    fireEvent.change(input, { target: { value: 'user-code' } }); fireEvent.click(screen.getByText('settings.oauth.submit'));
    await waitFor(() => expect(refresh).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(mocks.toast).toHaveBeenCalledWith('settings.oauth.success', 'success'));
  });
  it('取消不假称远端撤销；忽略已经发出但迟到的 start', async () => {
    let release!: (value: unknown) => void; mocks.request.mockImplementationOnce(() => new Promise(resolve => { release = resolve; }));
    const refresh = vi.fn(async () => {}); render(<OAuthCredentials providerId="main" summary={summary} onRefresh={refresh} />);
    fireEvent.click(screen.getByText('settings.oauth.login')); fireEvent.click(screen.getByText('common.cancel'));
    expect(screen.getByRole('alert')).toHaveTextContent('settings.oauth.waitCancelled');
    await act(async () => release(pkce()));
    expect(mocks.open).not.toHaveBeenCalled(); expect(refresh).not.toHaveBeenCalled(); expect(mocks.request).toHaveBeenCalledTimes(1);
  });
  it('离开再返回原连接不能采用旧 callback 结果或刷新新连接', async () => {
    let release!: (value: unknown) => void;
    mocks.request.mockResolvedValueOnce(pkce()).mockImplementationOnce(() => new Promise(resolve => { release = resolve; }));
    const refresh = vi.fn(async () => {}); render(<OAuthCredentials providerId="main" summary={summary} onRefresh={refresh} />);
    fireEvent.click(screen.getByText('settings.oauth.login')); fireEvent.change(await screen.findByPlaceholderText('settings.oauth.codePlaceholder'), { target: { value: 'user-code' } }); fireEvent.click(screen.getByText('settings.oauth.submit'));
    await waitFor(() => expect(mocks.request).toHaveBeenCalledTimes(2));
    act(() => { connection('other'); connection(); }); await act(async () => release(success()));
    expect(refresh).not.toHaveBeenCalled(); expect(mocks.toast).not.toHaveBeenCalled(); expect(screen.queryByPlaceholderText('settings.oauth.codePlaceholder')).not.toBeInTheDocument();
  });
  it('卸载和切换供应商后都不打开迟到授权地址', async () => {
    let release!: (value: unknown) => void; mocks.request.mockImplementationOnce(() => new Promise(resolve => { release = resolve; }));
    const view = render(<OAuthCredentials providerId="a" summary={summary} onRefresh={async () => {}} />);
    fireEvent.click(screen.getByText('settings.oauth.login')); view.rerender(<OAuthCredentials providerId="b" summary={summary} onRefresh={async () => {}} />);
    await act(async () => release(pkce())); expect(mocks.open).not.toHaveBeenCalled();
    mocks.request.mockImplementationOnce(() => new Promise(resolve => { release = resolve; })); fireEvent.click(screen.getByText('settings.oauth.login')); view.unmount(); await act(async () => release(pkce())); expect(mocks.open).not.toHaveBeenCalled();
  });
  it('登录已生效但刷新失败如实说明；未保存凭证不可显示普通成功', async () => {
    mocks.request.mockResolvedValueOnce(pkce()).mockResolvedValueOnce(success(false));
    const refresh = vi.fn(async () => {}); const view=render(<OAuthCredentials providerId="main" summary={summary} onRefresh={refresh} />);
    fireEvent.click(screen.getByText('settings.oauth.login')); fireEvent.change(await screen.findByPlaceholderText('settings.oauth.codePlaceholder'), { target: { value: 'code' } }); fireEvent.click(screen.getByText('settings.oauth.submit'));
    await waitFor(() => expect(mocks.toast).toHaveBeenCalledWith('settings.oauth.notPersisted', 'error'));
    view.unmount(); mocks.toast.mockClear(); mocks.request.mockResolvedValueOnce(pkce()).mockResolvedValueOnce(success());
    render(<OAuthCredentials providerId="main" summary={summary} onRefresh={async () => { throw new Error('refresh-disk-error'); }} />);
    fireEvent.click(screen.getByText('settings.oauth.login')); fireEvent.change(await screen.findByPlaceholderText('settings.oauth.codePlaceholder'), { target: { value: 'code' } }); fireEvent.click(screen.getByText('settings.oauth.submit'));
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('settings.oauth.loginRefreshFailed: refresh-disk-error'));
    expect(mocks.toast).not.toHaveBeenCalledWith('settings.oauth.success', 'success');
  });
  it('取消 device 等待后不再轮询或刷新，计时器可释放', async () => {
    mocks.request.mockResolvedValueOnce(device()); const refresh=vi.fn(async()=>{});
    const view=render(<OAuthCredentials providerId="main" summary={summary} onRefresh={refresh} />);
    fireEvent.click(screen.getByText('settings.oauth.login')); await screen.findByText('LOCAL-CODE');
    vi.useFakeTimers(); fireEvent.click(screen.getByText('common.cancel'));
    await act(async()=>{await vi.advanceTimersByTimeAsync(60_000);});
    expect(mocks.request).toHaveBeenCalledTimes(1); expect(refresh).not.toHaveBeenCalled(); view.unmount(); expect(vi.getTimerCount()).toBe(0);
  });
  it('设备码按服务间隔轮询，slowDown 延长等待后才确认并刷新', async () => {
    vi.useFakeTimers();
    mocks.request.mockResolvedValueOnce(device()).mockResolvedValueOnce({ ok:true,outcome:{status:'pending',slowDown:true,intervalMs:6000} }).mockResolvedValueOnce(success());
    const refresh=vi.fn(async()=>{}); render(<OAuthCredentials providerId="main" summary={summary} onRefresh={refresh} />);
    await act(async()=>{fireEvent.click(screen.getByText('settings.oauth.login'));});
    expect(screen.getByText('LOCAL-CODE')).toBeInTheDocument();
    await act(async()=>{await vi.advanceTimersByTimeAsync(999);});expect(mocks.request).toHaveBeenCalledTimes(1);
    await act(async()=>{await vi.advanceTimersByTimeAsync(1);});expect(mocks.request).toHaveBeenCalledTimes(2);expect(refresh).not.toHaveBeenCalled();
    await act(async()=>{await vi.advanceTimersByTimeAsync(5999);});expect(mocks.request).toHaveBeenCalledTimes(2);
    await act(async()=>{await vi.advanceTimersByTimeAsync(1);});expect(mocks.request).toHaveBeenCalledTimes(3);expect(refresh).toHaveBeenCalledTimes(1);
    expect(mocks.toast).toHaveBeenCalledWith('settings.oauth.success','success');expect(vi.getTimerCount()).toBe(0);
  });
  it('启动和回调请求失败不伪造已登录，也不进入成功刷新', async () => {
    const refresh=vi.fn(async()=>{});mocks.request.mockRejectedValueOnce(new Error('real-http-403'));
    render(<OAuthCredentials providerId="main" summary={summary} onRefresh={refresh} />);
    fireEvent.click(screen.getByText('settings.oauth.login'));await screen.findByText('settings.oauth.failed: real-http-403');expect(mocks.open).not.toHaveBeenCalled();
    mocks.request.mockResolvedValueOnce(pkce()).mockRejectedValueOnce(new Error('invalid-state'));
    fireEvent.click(screen.getByText('settings.oauth.login'));fireEvent.change(await screen.findByPlaceholderText('settings.oauth.codePlaceholder'),{target:{value:'code'}});fireEvent.click(screen.getByText('settings.oauth.submit'));
    await screen.findByText('settings.oauth.failed: invalid-state');expect(refresh).not.toHaveBeenCalled();expect(mocks.toast).not.toHaveBeenCalledWith('settings.oauth.success','success');
  });
  it('正式登出契约完成后刷新；刷新失败保留已经登出的事实', async () => {
    mocks.request.mockResolvedValueOnce({ok:true,models:[]});
    render(<OAuthCredentials providerId="main" summary={{...summary,logged_in:true}} onRefresh={async()=>{throw new Error('status-read-failed');}} />);
    fireEvent.click(screen.getByText('settings.oauth.logout'));await screen.findByText('settings.oauth.logoutRefreshFailed: status-read-failed');
    expect(mocks.request.mock.calls[0][0]).toBe('/lingxi/v1/models/credentials/main/logout');expect(mocks.request.mock.calls[0][1].method).toBe('POST');
    expect(mocks.toast).not.toHaveBeenCalledWith('settings.oauth.failed: status-read-failed','error');
  });

});
