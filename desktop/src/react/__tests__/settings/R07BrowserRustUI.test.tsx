/** @vitest-environment jsdom */
import React from 'react';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { BrowserTab } from '../../settings/tabs/BrowserTab';
import { SearchApiKeyConfig } from '../../settings/tabs/providers/SearchApiKeyConfig';
import { useSettingsStore } from '../../settings/store';
import { createLocalServerConnection, type ServerConnection } from '../../services/server-connection';
import { createRemoteResource } from '../../settings/resource-state';
import { t as settingsT } from '../../settings/helpers';
import zh from '../../../locales/zh.json';
import en from '../../../locales/en.json';
import ja from '../../../locales/ja.json';
import ko from '../../../locales/ko.json';
import zhTW from '../../../locales/zh-TW.json';
vi.mock('../../settings/helpers', () => ({ t: vi.fn((key: string) => key) }));
vi.mock('../../settings/actions', () => ({ updateSettingsSnapshot: vi.fn(), loadSettingsConfig: vi.fn() }));
vi.mock('@/ui', () => ({ Toggle: ({ on, onChange, ariaLabel, disabled }: any) => <button role="switch" aria-label={ariaLabel} aria-checked={on} disabled={disabled} onClick={() => onChange(!on)} />, SelectWidget: ({ options, value, onChange, disabled }: any) => <select aria-label="behavior" disabled={disabled} value={value} onChange={event => onChange(event.target.value)}>{options.map((option: any) => <option key={option.value} value={option.value}>{option.label}</option>)}</select> }));
const connection = (id = 'a'): ServerConnection => ({ connectionId: id, kind: 'custom_remote', serverId: `server-${id}`, serverNodeKind: 'lingxi-service', studioId: 'studio', label: id, baseUrl: `https://${id}.example`, wsUrl: `wss://${id}.example`, token: `${id}-token`, authState: 'paired', trustState: 'tunnel', credentialKind: 'device_credential', capabilities: [] });
const response = (value: unknown) => new Response(JSON.stringify(value), { headers: { 'Content-Type': 'application/json' } });
const fetchMock = vi.fn(); const toast = vi.fn();
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(done => { resolve = done; }); return { promise, resolve }; }
function select(id: string) { const conn = connection(id); useSettingsStore.setState({ serverConnections: { [id]: conn }, activeServerConnectionId: id, activeServerConnection: conn }); }
beforeEach(() => { vi.mocked(settingsT).mockImplementation(key => key); select('a'); useSettingsStore.setState({ settingsSnapshot: createRemoteResource(), showToast: toast }); toast.mockReset(); fetchMock.mockReset().mockResolvedValue(response({ browser: { acceptCookies: true, agentOpenBehavior: 'smart' } })); vi.stubGlobal('fetch', fetchMock); window.t = key => key; });
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

describe('Rust browser settings UI with actual HTTP mapping', () => {
  it.each([['zh', zh], ['en', en], ['ja', ja], ['ko', ko], ['zh-TW', zhTW]] as const)('shows the real Cookie policy restart impact beside the toggle in %s', async (_, locale) => {
    const notice = locale.settings.browser.cookiePolicyChangeHint;
    vi.mocked(settingsT).mockImplementation(key => key === 'settings.browser.cookiePolicyChangeHint' ? notice : key);
    render(<BrowserTab />);
    await waitFor(() => expect(screen.getByRole('switch')).toBeEnabled());
    const hint = screen.getByText(notice);
    expect(hint).toBeVisible();
    expect(screen.getByRole('switch').parentElement?.parentElement).toContainElement(hint);
  });
  it('also shows the policy impact for the bootstrapped local Rust connection', async () => {
    const local = createLocalServerConnection({ serverPort: 8844, serverToken: 'local-token', serverNodeKind: 'lingxi-service' })!;
    useSettingsStore.setState({ serverConnections: { [local.connectionId]: local }, activeServerConnectionId: local.connectionId, activeServerConnection: null, serverPort: 8844, serverToken: 'local-token' });
    render(<BrowserTab />);
    await waitFor(() => expect(screen.getByRole('switch')).toBeEnabled());
    expect(screen.getByText('settings.browser.cookiePolicyChangeHint')).toBeVisible();
    expect(fetchMock.mock.calls[0][0]).toBe('http://127.0.0.1:8844/lingxi/v1/preferences/browser');
  });
  it('loads and saves preferences at the selected Rust service', async () => {
    render(<BrowserTab />);
    const toggle = await screen.findByRole('switch');
    await waitFor(() => expect(toggle).toBeEnabled());
    fetchMock.mockResolvedValueOnce(response({ ok: true, browser: { acceptCookies: false, agentOpenBehavior: 'smart' } }));
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).toHaveAttribute('aria-checked', 'false'));
    expect(fetchMock.mock.calls[0][0]).toBe('https://a.example/lingxi/v1/preferences/browser');
    expect(fetchMock.mock.calls[1][1]).toMatchObject({ method: 'PUT', headers: { Authorization: 'Bearer a-token' }, body: JSON.stringify({ browser: { acceptCookies: false, agentOpenBehavior: 'smart' } }) });
  });
  it('does not treat malformed clear-cookie results as success', async () => {
    render(<BrowserTab />);
    await waitFor(() => expect(screen.getByRole('switch')).toBeEnabled());
    fetchMock.mockResolvedValueOnce(response({}));
    fireEvent.click(screen.getByRole('button', { name: 'settings.browser.clearCookiesButton' }));
    await waitFor(() => expect(toast).toHaveBeenCalledWith(expect.any(String), 'error'));
    expect(toast).not.toHaveBeenCalledWith('settings.browser.clearCookiesSuccess', 'success');
    expect(fetchMock.mock.calls[1][0]).toBe('https://a.example/lingxi/v1/preferences/browser/clear-cookies');
  });
  it('ignores old preferences after switching away and back', async () => {
    const old = deferred<Response>();
    fetchMock.mockReturnValueOnce(old.promise);
    render(<BrowserTab />);
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));
    act(() => select('b'));
    await waitFor(() => expect(screen.getByRole('switch')).toBeEnabled());
    act(() => select('a'));
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(3));
    await act(async () => old.resolve(response({ browser: { acceptCookies: false, agentOpenBehavior: 'new_tab' } })));
    expect(screen.getByRole('switch')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByRole('combobox')).toHaveValue('current_tab');
    expect((fetchMock.mock.calls[0][1].signal as AbortSignal).aborted).toBe(true);
  });
  it('keeps controls unavailable after a malformed preference response', async () => {
    fetchMock.mockResolvedValueOnce(response({}));
    render(<BrowserTab />);
    await waitFor(() => expect(toast).toHaveBeenCalledWith(expect.stringContaining('browser_preferences_invalid'), 'error'));
    expect(screen.getByRole('switch')).toBeDisabled();
    expect(screen.getByRole('button', { name: 'settings.browser.clearCookiesButton' })).toBeDisabled();
  });
  it('verifies the chosen provider through Rust and ignores late success after a connection switch', async () => {
    useSettingsStore.setState({ globalModelsConfig: { search: { provider: 'brave', api_keys: { brave: 'candidate-key' } } } } as never);
    const old = deferred<Response>(); fetchMock.mockReturnValueOnce(old.promise);
    render(<SearchApiKeyConfig />);
    await waitFor(() => expect(screen.getByRole('button', { name: 'settings.search.verify' })).toBeEnabled());
    fireEvent.click(screen.getByRole('button', { name: 'settings.search.verify' }));
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));
    expect(fetchMock.mock.calls[0][0]).toBe('https://a.example/lingxi/v1/search/verify');
    act(() => select('b'));
    await act(async () => old.resolve(response({ ok: true })));
    expect(toast).not.toHaveBeenCalledWith('settings.search.verified', 'success');
  });
});
