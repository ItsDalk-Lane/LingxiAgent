/** @vitest-environment jsdom */
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
vi.mock('../../bootstrap', () => ({ initTheme: vi.fn() }));
import { BrowserViewerApp } from '../../browser-viewer/BrowserViewerApp';
const context = { connectionKey: 'handle', sessionPath: 'native:s', executionNode: 'server-a', serviceLabel: '实际服务', serviceUrl: 'https://service.example' };
const getContext = vi.fn(); const listen = vi.fn(); const unsubscribe = vi.fn(); const request = vi.fn();
beforeEach(() => {
  history.replaceState(null, '', '/');
  getContext.mockReset().mockResolvedValue(context); listen.mockReset().mockReturnValue(unsubscribe); unsubscribe.mockReset(); request.mockReset().mockResolvedValue({ workspaces: [] });
  window.t = key => key;
  Object.defineProperty(window, 'hana', { configurable: true, value: { getBrowserViewerContext: getContext, onBrowserUpdate: listen, rustBrowserRequest: request } });
});
afterEach(() => { cleanup(); history.replaceState(null, '', '/'); });
describe('production browser viewer mode entry', () => {
  it('keeps the Node toolbar listening immediately without an asynchronous Rust context lookup', () => {
    const view = render(<BrowserViewerApp />);
    expect(listen).toHaveBeenCalledTimes(1);
    expect(getContext).not.toHaveBeenCalled();
    view.unmount(); expect(unsubscribe).toHaveBeenCalledTimes(1);
  });
  it('renders the native status panel from trusted context and does not attach the legacy toolbar', async () => {
    history.replaceState(null, '', '/?mode=native');
    render(<BrowserViewerApp />);
    expect(await screen.findByText(/实际服务 · server-a/)).toBeInTheDocument();
    expect(listen).not.toHaveBeenCalled();
    await waitFor(() => expect(request).toHaveBeenCalledWith(context, 'workspaces', undefined));
  });
  it('reports a missing native context instead of falling back to the legacy viewer', async () => {
    history.replaceState(null, '', '/?mode=native');
    getContext.mockResolvedValueOnce(null);
    render(<BrowserViewerApp />);
    expect(await screen.findByRole('alert')).toHaveTextContent('browser_connection_required');
    expect(listen).not.toHaveBeenCalled(); expect(request).not.toHaveBeenCalled();
  });
});
