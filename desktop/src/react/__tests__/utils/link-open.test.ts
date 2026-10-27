/**
 * @vitest-environment jsdom
 */
import { afterEach, describe, expect, it, vi } from 'vitest';
import { openInternalLink, resolveLinkTarget } from '../../utils/link-open';
import { currentRustBrowserConnection, openRustBrowserViewer } from '../../services/rust-browser';
import { showError } from '../../utils/ui-helpers';
import { useStore } from '../../stores';
import { resetDesktopHostForTests } from '../../host/desktop-host';

vi.mock('../../services/rust-browser', () => ({ currentRustBrowserConnection: vi.fn(() => null), openRustBrowserViewer: vi.fn() }));
vi.mock('../../utils/ui-helpers', () => ({ showError: vi.fn() }));

afterEach(() => {
  document.documentElement.removeAttribute('data-platform');
  resetDesktopHostForTests();
  vi.mocked(currentRustBrowserConnection).mockReset().mockReturnValue(null);
  vi.mocked(openRustBrowserViewer).mockReset();
  vi.mocked(showError).mockReset();
});

describe('resolveLinkTarget', () => {
  it('routes http links to the internal browser preview target', () => {
    expect(resolveLinkTarget('https://example.com/a?b=1#top')).toEqual({
      kind: 'web',
      url: 'https://example.com/a?b=1#top',
    });
  });

  it('resolves relative markdown links against the previewed file path', () => {
    expect(resolveLinkTarget('./assets/Cover%20Image.png#fig-1', {
      baseFilePath: '/vault/notes/chapter.md',
    })).toEqual({
      kind: 'file',
      filePath: '/vault/notes/assets/Cover Image.png',
      ext: 'png',
      label: 'Cover Image.png',
    });
  });

  it('decodes file URLs into local file targets', () => {
    expect(resolveLinkTarget('file:///tmp/hana-link-fixture/demo.md')).toEqual({
      kind: 'file',
      filePath: '/tmp/hana-link-fixture/demo.md',
      ext: 'md',
      label: 'demo.md',
    });
  });

  it('leaves in-document anchors to the browser default behavior', () => {
    expect(resolveLinkTarget('#section-2', {
      baseFilePath: '/vault/notes/chapter.md',
    })).toEqual({ kind: 'anchor', href: '#section-2' });
  });

  it('falls back to a normal browser tab when the internal browser viewer is unavailable on web clients', async () => {
    const openBrowserViewer = vi.fn();
    const openExternal = vi.fn();
    document.documentElement.setAttribute('data-platform', 'web');
    window.platform = {
      openBrowserViewer,
      openExternal,
    } as unknown as typeof window.platform;

    await openInternalLink('https://example.com/');

    expect(openBrowserViewer).not.toHaveBeenCalled();
    expect(openExternal).toHaveBeenCalledWith('https://example.com/');
  });

  it('passes the current session path to the internal browser viewer on desktop', async () => {
    const openBrowserViewer = vi.fn();
    const openExternal = vi.fn();
    window.platform = {
      openBrowserViewer,
      openExternal,
    } as unknown as typeof window.platform;

    await openInternalLink('https://example.com/', {
      sessionPath: '/tmp/hana-session.jsonl',
    });

    expect(openBrowserViewer).toHaveBeenCalledWith({
      url: 'https://example.com/',
      sessionPath: '/tmp/hana-session.jsonl',
    });
    expect(openExternal).not.toHaveBeenCalled();
  });
  it('routes Rust URL previews through the trusted service-session entry', async () => {
    vi.mocked(currentRustBrowserConnection).mockReturnValue({ serverNodeKind: 'lingxi-service' } as ReturnType<typeof currentRustBrowserConnection>);
    const legacyOpen = vi.fn(); const external = vi.fn();
    window.platform = { openBrowserViewer: legacyOpen, openExternal: external } as unknown as typeof window.platform;
    await openInternalLink('https://example.com/new', { sessionPath: 'native:message-session' });
    expect(openRustBrowserViewer).toHaveBeenCalledWith('native:message-session', false, 'https://example.com/new');
    expect(legacyOpen).not.toHaveBeenCalled(); expect(external).not.toHaveBeenCalled();
  });
  it('preserves an explicit Rust open error without a local fallback', async () => {
    vi.mocked(currentRustBrowserConnection).mockReturnValue({ serverNodeKind: 'lingxi-service' } as ReturnType<typeof currentRustBrowserConnection>);
    vi.mocked(openRustBrowserViewer).mockRejectedValueOnce(new Error('browser_native_session_required'));
    const legacyOpen = vi.fn(); const external = vi.fn();
    window.platform = { openBrowserViewer: legacyOpen, openExternal: external } as unknown as typeof window.platform;
    expect(await openInternalLink('https://example.com/new')).toBe(false);
    expect(showError).toHaveBeenCalledWith('browser_native_session_required');
    expect(openRustBrowserViewer).toHaveBeenCalledWith('', false, 'https://example.com/new');
    expect(legacyOpen).not.toHaveBeenCalled(); expect(external).not.toHaveBeenCalled();
  });
  it('does not report a delayed open failure into a newly selected assistant', async () => {
    vi.mocked(currentRustBrowserConnection).mockReturnValue({ serverNodeKind: 'lingxi-service' } as ReturnType<typeof currentRustBrowserConnection>);
    window.platform = { openBrowserViewer: vi.fn() } as unknown as typeof window.platform;
    useStore.setState({ currentAgentId: 'before' });
    let reject!: (error: Error) => void;
    vi.mocked(openRustBrowserViewer).mockImplementationOnce(() => new Promise<void>((_, fail) => { reject = fail; }));
    const pending = openInternalLink('https://example.com/new', { sessionPath: 'native:s' });
    useStore.setState({ currentAgentId: 'after' });
    reject(new Error('old connection failed'));
    expect(await pending).toBe(false);
    expect(showError).not.toHaveBeenCalled();
  });
});
