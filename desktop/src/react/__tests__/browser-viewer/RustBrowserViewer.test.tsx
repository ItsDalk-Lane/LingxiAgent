/** @vitest-environment jsdom */
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { RustBrowserViewer } from '../../browser-viewer/RustBrowserViewer';
import { parseRustBrowserWorkspaces, rustBrowserRoute } from '../../services/rust-browser';
const context = { connectionKey: 'private-handle', sessionPath: 'native:s', executionNode: 'server-a', serviceLabel: '客厅电脑', serviceUrl: 'https://service.example' };
const workspace = { sessionId: 's', sessionPath: 'native:s', executionNode: 'server-a', running: true, resumable: true, url: 'https://example.com', title: '实际网页', activeTabId: 'tab-a', tabs: [{ tabId: 'tab-a', title: '实际网页', url: 'https://example.com', canGoBack: true, canGoForward: false, createdAt: '2026-10-16T00:00:00Z', updatedAt: '2026-10-16T00:00:00Z' }], suspended: false, userTakeover: false, revoked: false, visibleCapable: true, unavailableReason: null };
let request: ReturnType<typeof vi.fn>;
beforeEach(() => { window.t = key => key; request = vi.fn(async () => ({ workspaces: [workspace] })); Object.defineProperty(window, 'hana', { configurable: true, value: { rustBrowserRequest: request } }); });
afterEach(cleanup);
describe('Rust controlled browser panel', () => {
  it('shows the actual execution node and sends takeover to the same native session', async () => {
    render(<RustBrowserViewer context={context} />);
    expect(await screen.findByText('实际网页', { selector: 'p' })).toBeInTheDocument();
    expect(screen.getByText(/客厅电脑 · server-a/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '接管' }));
    await waitFor(() => expect(request).toHaveBeenCalledWith(context, 'takeover', undefined));
    expect(document.querySelector('iframe')).toBeNull();
    expect(document.querySelector('webview')).toBeNull();
  });
  it('keeps an empty last-tab workspace running and allows a new tab', async () => {
    request.mockResolvedValue({ workspaces: [{ ...workspace, tabs: [], activeTabId: null }] });
    render(<RustBrowserViewer context={context} />);
    expect(await screen.findByText('没有打开的标签页')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '新标签页' })).toBeEnabled();
    fireEvent.click(screen.getByRole('button', { name: '新标签页' }));
    await waitFor(() => expect(request).toHaveBeenCalledWith(context, 'new_tab', undefined));
  });
  it('disables handover when the service explicitly cannot show a window', async () => {
    request.mockResolvedValue({ workspaces: [{ ...workspace, visibleCapable: false }] });
    render(<RustBrowserViewer context={context} />);
    expect(await screen.findByText('该服务不支持显示网页窗口，无法在这里手动接管。')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '接管' })).toBeDisabled();
    expect(screen.getByRole('button', { name: '显示浏览器' })).toBeDisabled();
  });
  it('rejects another service response and shows failures instead of an empty workspace', async () => {
    expect(() => parseRustBrowserWorkspaces({ workspaces: [{ ...workspace, executionNode: 'server-b' }] }, context)).toThrow();
    request.mockRejectedValue(new Error('service unavailable'));
    render(<RustBrowserViewer context={context} />);
    expect(await screen.findByRole('alert')).toHaveTextContent('service unavailable');
    expect(screen.queryByText('此聊天没有可用的浏览器状态')).toBeNull();
  });
  it('ignores responses received after unmount', async () => {
    let resolve!: (value: unknown) => void;
    request.mockImplementation(() => new Promise(done => { resolve = done; }));
    const rendered = render(<RustBrowserViewer context={context} />);
    rendered.unmount();
    await act(async () => { resolve({ workspaces: [workspace] }); });
    expect(request).toHaveBeenCalledTimes(1);
  });
  it('maps only the existing browser/settings/search verify HTTP surface in Rust mode', () => {
    for (const path of ['/api/browser/session-states', '/api/browser/open-session', '/api/browser/command', '/api/preferences/browser/clear-cookies', '/api/search/verify']) {
      expect(rustBrowserRoute({ serverNodeKind: 'lingxi-service' }, path)).toBe(`/lingxi/v1${path.slice(4)}`);
      expect(rustBrowserRoute({}, path)).toBe(path);
    }
    expect(rustBrowserRoute({ serverNodeKind: 'lingxi-service' }, '/api/browser-other')).toBe('/api/browser-other');
  });
});
