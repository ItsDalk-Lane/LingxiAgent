/**
 * @vitest-environment jsdom
 *
 * 应用级快捷键 dispatcher 单测：键位索引、命令分发、作用域隔离与刷新。
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup } from '@testing-library/react';

const lingxiFetch = vi.fn();

vi.mock('../../settings/api', () => ({
  lingxiFetch: (...args: unknown[]) => lingxiFetch(...args),
}));

import { initKeybindings, VOICE_RECORD_TOGGLE_EVENT } from '../useKeybindings';

function jsonResponse(data: unknown) {
  return {
    json: vi.fn(async () => data),
  };
}

function press(key: string, mods: { ctrl?: boolean; meta?: boolean; alt?: boolean; shift?: boolean } = {}) {
  const event = new KeyboardEvent('keydown', {
    key,
    ctrlKey: !!mods.ctrl,
    metaKey: !!mods.meta,
    altKey: !!mods.alt,
    shiftKey: !!mods.shift,
    bubbles: true,
    cancelable: true,
  });
  document.dispatchEvent(event);
  return event;
}

beforeEach(() => {
  lingxiFetch.mockResolvedValue(jsonResponse({ keybindings: {} }));
});

afterEach(() => {
  cleanup();
  lingxiFetch.mockReset();
});

describe('initKeybindings', () => {
  it('dispatches app-scope commands and prevents default', async () => {
    const openSettings = vi.fn();
    const runtime = initKeybindings({ 'app.open-settings': openSettings });
    await new Promise(r => setTimeout(r, 0));

    const event = press(',', { meta: true });
    expect(openSettings).toHaveBeenCalledOnce();
    expect(event.defaultPrevented).toBe(true);
    runtime.dispose();
  });

  it('broadcasts a custom event for the local voice command instead of calling a handler', async () => {
    const runtime = initKeybindings({});
    await new Promise(r => setTimeout(r, 0));

    const listener = vi.fn();
    window.addEventListener(VOICE_RECORD_TOGGLE_EVENT, listener);
    press('m', { ctrl: true, shift: true });
    window.removeEventListener(VOICE_RECORD_TOGGLE_EVENT, listener);

    expect(listener).toHaveBeenCalledOnce();
    runtime.dispose();
  });

  it('never dispatches the global quick-chat command from the renderer', async () => {
    const handler = vi.fn();
    const runtime = initKeybindings({ 'app.open-settings': handler });
    await new Promise(r => setTimeout(r, 0));

    press(' ', { alt: true }); // Alt+Space 归主进程 globalShortcut
    expect(handler).not.toHaveBeenCalled();
    runtime.dispose();
  });

  it('ignores unbound combinations', async () => {
    const handler = vi.fn();
    const runtime = initKeybindings({ 'app.open-settings': handler });
    await new Promise(r => setTimeout(r, 0));

    const event = press('k', { ctrl: true });
    expect(handler).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(false);
    runtime.dispose();
  });

  it('picks up rebindings after refresh()', async () => {
    lingxiFetch
      .mockResolvedValueOnce(jsonResponse({ keybindings: {} }))
      .mockResolvedValueOnce(jsonResponse({ keybindings: { 'app.open-settings': ['CommandOrControl+Shift+P'] } }));
    const openSettings = vi.fn();
    const runtime = initKeybindings({ 'app.open-settings': openSettings });
    await new Promise(r => setTimeout(r, 0));

    // 未覆盖时默认键位开箱即用
    press(',', { meta: true });
    expect(openSettings).toHaveBeenCalledTimes(1);

    await runtime.refresh(); // 改绑到 ⌘⇧P
    press(',', { meta: true });
    expect(openSettings).toHaveBeenCalledTimes(1); // 旧键位失效

    press('p', { meta: true, shift: true });
    expect(openSettings).toHaveBeenCalledTimes(2); // 新键位生效
    runtime.dispose();
  });
});
