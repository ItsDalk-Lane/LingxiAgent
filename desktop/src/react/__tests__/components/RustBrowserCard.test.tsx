/** @vitest-environment jsdom */
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { BrowserCard } from '../../components/BrowserCard';
import { useStore } from '../../stores';
import { setBrowserStateForPath } from '../../stores/browser-slice';
import type { ServerConnection } from '../../services/server-connection';
const connection: ServerConnection = { connectionId: 'remote', kind: 'custom_remote', serverId: 'server-a', serverNodeKind: 'lingxi-service', studioId: 'studio', label: '服务A', baseUrl: 'https://service.example', wsUrl: 'wss://service.example', token: 'secret', authState: 'paired', trustState: 'tunnel', credentialKind: 'device_credential', capabilities: [] };
const open = vi.fn(); const register = vi.fn();
beforeEach(() => {
  window.t = key => key;
  open.mockReset().mockResolvedValue({ opened: true }); register.mockReset().mockResolvedValue('handle');
  Object.defineProperty(window, 'platform', { configurable: true, value: { openBrowserViewer: open, setBrowserConnection: register } });
  useStore.setState({ serverConnections: { remote: connection }, activeServerConnectionId: 'remote', activeServerConnection: connection, currentSessionPath: 'native:s', currentSessionId: 's', sessions: [], sessionLocatorsById: {}, browserBySession: {} } as never);
});
afterEach(cleanup);
describe('Rust browser floating card routing', () => {
  it('opens the actual service browser through a private handle and explicit native session', async () => {
    setBrowserStateForPath('native:s', { executionNode: 'server-a', running: true, url: 'https://example.com', thumbnail: null });
    render(<BrowserCard />);
    expect(screen.getByText('server-a')).toBeInTheDocument();
    fireEvent.click(document.getElementById('browserFloatingCard')!);
    await waitFor(() => expect(open).toHaveBeenCalledWith({ sessionPath: 'native:s', connectionKey: 'handle' }));
    expect(JSON.stringify(open.mock.calls)).not.toContain('secret');
  });
  it('rejects a browser state from a different service, including foreign-session relay', () => {
    setBrowserStateForPath('native:s', { executionNode: 'server-b', running: true, url: 'https://wrong.example', thumbnail: null });
    useStore.setState({ browserBySession: { foreign: { executionNode: 'server-b', sessionPath: 'native:foreign', running: true, url: 'https://wrong.example', thumbnail: null } } });
    render(<BrowserCard />);
    expect(document.getElementById('browserFloatingCard')).toBeNull();
    expect(open).not.toHaveBeenCalled();
  });
  it('does not relay a same-node workspace owned by a different connection', () => {
    useStore.setState({ browserBySession: { foreign: { executionNode: 'server-a', connectionScope: 'another-connection', sessionPath: 'native:foreign', running: true, url: 'https://private-other.example', thumbnail: null } } });
    render(<BrowserCard />);
    expect(document.getElementById('browserFloatingCard')).toBeNull();
    expect(screen.queryByText('private-other.example')).not.toBeInTheDocument();
    expect(open).not.toHaveBeenCalled();
  });
  it('does not guess the current session for a foreign browser with no recorded owner', async () => {
    useStore.setState({ browserBySession: { foreign: { executionNode: 'server-a', running: true, url: null, thumbnail: null } } });
    render(<BrowserCard />);
    fireEvent.click(document.getElementById('browserFloatingCard')!);
    expect(await screen.findByRole('alert')).toHaveTextContent('browser_native_session_required');
    expect(open).not.toHaveBeenCalled();
  });
  it('shows explicit unavailability and never opens a local viewer without the connection bridge', async () => {
    Object.defineProperty(window, 'platform', { configurable: true, value: { openBrowserViewer: open } });
    setBrowserStateForPath('native:s', { executionNode: 'server-a', running: true, url: 'https://example.com', thumbnail: null });
    render(<BrowserCard />);
    fireEvent.click(document.getElementById('browserFloatingCard')!);
    expect(await screen.findByRole('alert')).toHaveTextContent('browser_desktop_unavailable');
    expect(open).not.toHaveBeenCalled();
  });
});
