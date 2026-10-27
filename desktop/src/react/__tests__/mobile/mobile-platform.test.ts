// @vitest-environment jsdom

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { installMobilePlatform } from '../../mobile/mobile-platform';
import { getDesktopHost } from '../../host/desktop-host';

describe('mobile platform capability contract', () => {
  beforeEach(() => {
    document.documentElement.removeAttribute('data-platform');
    Reflect.deleteProperty(window, 'platform');
  });

  it('浏览器旧返回值保持void，不把noopener的窗口返回值冒充系统回执', () => {
    const open = vi.spyOn(window, 'open').mockReturnValue(window);
    try {
      installMobilePlatform();
      expect(getDesktopHost().api?.openExternal('https://example.invalid/owned')).toBeUndefined();
      expect(open).toHaveBeenCalledExactlyOnceWith('https://example.invalid/owned', '_blank', 'noopener,noreferrer');
    } finally { open.mockRestore(); }
  });

  it('does not expose desktop-only file system capabilities as callable no-ops', () => {
    installMobilePlatform();

    expect(document.documentElement.getAttribute('data-platform')).toBe('web');
    expect(getDesktopHost().api?.selectFolder).toBeUndefined();
    expect(getDesktopHost().api?.selectFiles).toBeUndefined();
    expect(getDesktopHost().api?.writeFileIfUnchanged).toBeUndefined();
    expect(getDesktopHost().api?.openFolder).toBeUndefined();
    expect(getDesktopHost().api?.showInFinder).toBeUndefined();
    expect(getDesktopHost().api?.trashItem).toBeUndefined();
    expect(getDesktopHost().api?.getFilePath).toBeUndefined();
  });

  it('routes platform settings requests through an explicit mobile event contract', () => {
    installMobilePlatform();
    const listener = vi.fn();
    const unsubscribe = getDesktopHost().api?.onOpenSettingsModal?.(listener);

    getDesktopHost().api?.openSettings?.('providers');

    expect(listener).toHaveBeenCalledWith('providers');
    unsubscribe?.();
    getDesktopHost().api?.openSettings?.('work');
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it('replaces stale partial platform globals with the mobile event contract', () => {
    window.platform = {
      openSettings: vi.fn(),
      selectFolder: vi.fn(),
    } as unknown as typeof window.platform;

    installMobilePlatform();
    const listener = vi.fn();
    getDesktopHost().api?.onOpenSettingsModal?.(listener);

    getDesktopHost().api?.openSettings?.('providers');

    expect(listener).toHaveBeenCalledWith('providers');
    expect(getDesktopHost().api?.selectFolder).toBeUndefined();
  });
});
