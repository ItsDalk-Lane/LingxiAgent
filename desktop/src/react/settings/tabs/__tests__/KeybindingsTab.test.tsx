/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';

const keybindingsReloadGlobal = vi.fn();
const quickChatReloadShortcut = vi.fn();
const settingsChanged = vi.fn();
const updateSettingsSnapshot = vi.fn();
const lingxiFetch = vi.fn();

vi.mock('../../api', () => ({
  lingxiFetch: (...args: unknown[]) => lingxiFetch(...args),
}));

vi.mock('../../helpers', () => ({
  t: (key: string) => key,
  autoSaveConfig: vi.fn(),
}));

vi.mock('../../actions', () => ({
  loadSettingsConfig: vi.fn(),
  updateSettingsSnapshot: (...args: unknown[]) => updateSettingsSnapshot(...args),
}));

import { KeybindingsTab } from '../KeybindingsTab';
import { useSettingsStore } from '../../store';
import { KEYBINDING_COMMAND_IDS } from '../../../../../../shared/keybindings-preferences.ts';

function jsonResponse(data: unknown) {
  return {
    json: vi.fn(async () => data),
  };
}

function installHana() {
  vi.stubGlobal('window', Object.assign(window, {
    hana: {
      keybindingsReloadGlobal,
      quickChatReloadShortcut,
      settingsChanged,
    },
  }));
}

beforeEach(() => {
  keybindingsReloadGlobal.mockResolvedValue({ ok: true, shortcut: 'Alt+Space' });
  quickChatReloadShortcut.mockResolvedValue({ ok: true, shortcut: 'Alt+Space' });
  lingxiFetch.mockResolvedValue(jsonResponse({ keybindings: {} }));
  useSettingsStore.setState({
    settingsSnapshot: {
      key: null,
      status: 'idle',
      data: null,
      error: null,
      requestId: 0,
      updatedAt: null,
    },
    toastMessage: '',
    toastType: '',
    toastVisible: false,
  });
});

afterEach(() => {
  cleanup();
  keybindingsReloadGlobal.mockReset();
  quickChatReloadShortcut.mockReset();
  settingsChanged.mockReset();
  updateSettingsSnapshot.mockReset();
  lingxiFetch.mockReset();
  vi.unstubAllGlobals();
});

/** 渲染并 flush 初始 GET（stored 加载完成后按钮才解除 disabled）。 */
async function renderTab() {
  render(<KeybindingsTab />);
  await waitFor(() => expect(screen.getByRole('table')).toBeTruthy());
  await act(async () => {
    await new Promise(resolve => setTimeout(resolve, 0));
  });
}

function rowEl(commandId: string): HTMLElement {
  const row = document.querySelector(`[data-command="${commandId}"]`);
  if (!row) throw new Error(`command row not found: ${commandId}`);
  return row as HTMLElement;
}

function rowQuery(row: Element) {
  return {
    allByLabelText: (label: string) =>
      Array.from(row.querySelectorAll<HTMLElement>('[aria-label]')).filter(n => n.getAttribute('aria-label') === label),
  };
}

describe('KeybindingsTab', () => {
  it('fetches stored keybindings and renders every registered command with its scope', async () => {
    lingxiFetch.mockResolvedValue(jsonResponse({
      keybindings: { 'quick-chat.toggle': ['F9'] },
    }));
    await renderTab();

    // 共享注册表的每条命令都要出现在列表里（+ 1 行表头）
    expect(screen.getAllByRole('row')).toHaveLength(KEYBINDING_COMMAND_IDS.length + 1);
    for (const id of KEYBINDING_COMMAND_IDS) {
      expect(document.querySelector(`[data-command="${id}"]`)).toBeTruthy();
    }
    // 覆盖项生效（呼出快捷对话显示 F9），未覆盖命令回落默认键位
    expect(rowEl('quick-chat.toggle').textContent).toContain('F9');
    expect(rowEl('app.open-settings').textContent).toContain(',');
    // 作用域徽标：全局 / 应用 / 局部
    expect(rowEl('quick-chat.toggle').textContent).toContain('settings.keybindings.scope.global');
    expect(rowEl('app.restart').textContent).toContain('settings.keybindings.scope.app');
    expect(rowEl('voice.record-toggle').textContent).toContain('settings.keybindings.scope.local');
  });

  it('filters commands by search query', async () => {
    await renderTab();

    const input = screen.getByLabelText('settings.keybindings.searchPlaceholder');
    fireEvent.change(input, { target: { value: 'settings.keybindings.commands.appRestart' } });
    await waitFor(() => expect(document.querySelectorAll('[data-command]')).toHaveLength(1));
    expect(document.querySelector('[data-command="app.restart"]')).toBeTruthy();
  });

  it('edits the default binding, saves it, and keeps global re-registration out of app scope', async () => {
    installHana();
    lingxiFetch
      .mockResolvedValueOnce(jsonResponse({ keybindings: {} }))
      .mockResolvedValueOnce(jsonResponse({ ok: true, keybindings: { 'app.restart': ['CommandOrControl+Shift+R'] } }));
    await renderTab();

    fireEvent.click(rowQuery(rowEl('app.restart')).allByLabelText('settings.keybindings.editBinding')[0]);
    fireEvent.keyDown(window, { key: 'r', ctrlKey: true, shiftKey: true });

    await waitFor(() => expect(lingxiFetch).toHaveBeenLastCalledWith('/api/preferences/keybindings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ keybindings: { 'app.restart': ['CommandOrControl+Shift+R'] } }),
    }));
    // app 作用域不触发全局重注册
    expect(keybindingsReloadGlobal).not.toHaveBeenCalled();
    expect(settingsChanged).toHaveBeenCalledWith('keybindings-changed', {
      keybindings: { 'app.restart': ['CommandOrControl+Shift+R'] },
    });
  });

  it('appends a second binding through the add-binding slot', async () => {
    installHana();
    lingxiFetch
      .mockResolvedValueOnce(jsonResponse({ keybindings: {} }))
      .mockResolvedValueOnce(jsonResponse({ ok: true, keybindings: { 'app.restart': ['CommandOrControl+Alt+R', 'CommandOrControl+Shift+R'] } }));
    await renderTab();

    const addButton = Array.from(rowEl('app.restart').querySelectorAll('button'))
      .find(b => b.textContent === '+ settings.keybindings.addBinding');
    if (!addButton) throw new Error('add-binding button not found');
    fireEvent.click(addButton);
    fireEvent.keyDown(window, { key: 'r', ctrlKey: true, shiftKey: true });

    await waitFor(() => expect(lingxiFetch).toHaveBeenLastCalledWith('/api/preferences/keybindings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ keybindings: { 'app.restart': ['CommandOrControl+Alt+R', 'CommandOrControl+Shift+R'] } }),
    }));
  });

  it('re-registers the global shortcut when quick chat toggle is rebound', async () => {
    installHana();
    lingxiFetch
      .mockResolvedValueOnce(jsonResponse({ keybindings: {} }))
      .mockResolvedValueOnce(jsonResponse({ ok: true, keybindings: { 'quick-chat.toggle': ['F9'] } }));
    await renderTab();

    fireEvent.click(rowQuery(rowEl('quick-chat.toggle')).allByLabelText('settings.keybindings.editBinding')[0]);
    fireEvent.keyDown(window, { key: 'F9' });

    await waitFor(() => expect(keybindingsReloadGlobal).toHaveBeenCalledOnce());
  });

  it('rejects a binding that conflicts with another command without saving', async () => {
    installHana();
    lingxiFetch.mockResolvedValue(jsonResponse({
      keybindings: { 'app.new-session': ['CommandOrControl+Shift+R'] },
    }));
    await renderTab();

    fireEvent.click(rowQuery(rowEl('app.restart')).allByLabelText('settings.keybindings.editBinding')[0]);
    fireEvent.keyDown(window, { key: 'r', ctrlKey: true, shiftKey: true });

    await waitFor(() => expect(useSettingsStore.getState().toastMessage).toContain('settings.keybindings.conflict'));
    // 只有初始 GET，没有 PUT
    expect(lingxiFetch).toHaveBeenCalledTimes(1);
  });

  it('removes a binding and stores an explicit empty override', async () => {
    installHana();
    lingxiFetch
      .mockResolvedValueOnce(jsonResponse({ keybindings: { 'app.toggle-sidebar': ['CommandOrControl+Shift+S'] } }))
      .mockResolvedValueOnce(jsonResponse({ ok: true, keybindings: { 'app.toggle-sidebar': [] } }));
    await renderTab();

    fireEvent.click(rowQuery(rowEl('app.toggle-sidebar')).allByLabelText('settings.keybindings.removeBinding')[0]);

    await waitFor(() => expect(lingxiFetch).toHaveBeenLastCalledWith('/api/preferences/keybindings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ keybindings: { 'app.toggle-sidebar': [] } }),
    }));
  });

  it('resets a single command to default by dropping its override', async () => {
    installHana();
    lingxiFetch
      .mockResolvedValueOnce(jsonResponse({ keybindings: { 'app.toggle-sidebar': ['CommandOrControl+Alt+S'] } }))
      .mockResolvedValueOnce(jsonResponse({ ok: true, keybindings: {} }));
    await renderTab();

    fireEvent.click(rowQuery(rowEl('app.toggle-sidebar')).allByLabelText('settings.keybindings.restoreCommand')[0]);

    await waitFor(() => expect(lingxiFetch).toHaveBeenLastCalledWith('/api/preferences/keybindings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ keybindings: {} }),
    }));
  });

  it('resets all commands with the reset-all button', async () => {
    installHana();
    lingxiFetch
      .mockResolvedValueOnce(jsonResponse({ keybindings: { 'app.toggle-sidebar': ['CommandOrControl+Alt+S'] } }))
      .mockResolvedValueOnce(jsonResponse({ ok: true, keybindings: {} }));
    await renderTab();

    fireEvent.click(screen.getByText('settings.keybindings.resetAll'));

    await waitFor(() => expect(lingxiFetch).toHaveBeenLastCalledWith('/api/preferences/keybindings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ keybindings: {} }),
    }));
    await waitFor(() => expect(keybindingsReloadGlobal).toHaveBeenCalledOnce());
  });

  it('normalizes macOS Option+Space recording to Alt+Space instead of an invisible character', async () => {
    installHana();
    lingxiFetch
      .mockResolvedValueOnce(jsonResponse({ keybindings: {} }))
      .mockResolvedValueOnce(jsonResponse({ ok: true, keybindings: { 'app.restart': ['CommandOrControl+Alt+R', 'CommandOrControl+Shift+Space'] } }));
    await renderTab();

    // Option+Space 的归一结果 Alt+Space 会被快捷对话默认键位撞冲突，
    // 故走追加路径用无冲突的 Ctrl+Shift+Space 验证 Space 归一不变成不可见字符
    const addButton = Array.from(rowEl('app.restart').querySelectorAll('button'))
      .find(b => b.textContent === '+ settings.keybindings.addBinding');
    if (!addButton) throw new Error('add-binding button not found');
    fireEvent.click(addButton);
    fireEvent.keyDown(window, { key: '\u00A0', code: 'Space', ctrlKey: true, shiftKey: true });

    await waitFor(() => expect(lingxiFetch).toHaveBeenLastCalledWith('/api/preferences/keybindings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ keybindings: { 'app.restart': ['CommandOrControl+Alt+R', 'CommandOrControl+Shift+Space'] } }),
    }));
  });
});

// ── 用户路径集成：修改 → 恢复默认 → 全部恢复 ────────────────────
describe('KeybindingsTab user flows', () => {
  it('enables reset-all after an override, and restores the binding to default via the row action', async () => {
    installHana();
    lingxiFetch
      .mockResolvedValueOnce(jsonResponse({ keybindings: {} })) // 初始 GET
      .mockResolvedValueOnce(jsonResponse({ ok: true, keybindings: { 'app.restart': ['CommandOrControl+Shift+R'] } })) // 修改保存
      .mockResolvedValueOnce(jsonResponse({ ok: true, keybindings: {} })); // 恢复保存
    await renderTab();

    // 初始：没有覆盖项，全部恢复按钮禁用
    const resetAllButton = screen.getByText('settings.keybindings.resetAll') as HTMLButtonElement;
    expect(resetAllButton.disabled).toBe(true);

    // 修改重启键位（编辑默认绑定）
    fireEvent.click(rowQuery(rowEl('app.restart')).allByLabelText('settings.keybindings.editBinding')[0]);
    fireEvent.keyDown(window, { key: 'r', ctrlKey: true, shiftKey: true });
    await waitFor(() => expect(rowEl('app.restart').textContent).toContain('Shift'));
    await act(async () => { await new Promise(r => setTimeout(r, 0)); });

    // 修改后：全部恢复启用、单命令恢复启用
    expect(resetAllButton.disabled).toBe(false);
    const restoreButton = rowQuery(rowEl('app.restart')).allByLabelText('settings.keybindings.restoreCommand')[0] as HTMLButtonElement;
    expect(restoreButton.disabled).toBe(false);

    // 恢复默认：键位回到 ⌘⌥R，覆盖项清除
    fireEvent.click(restoreButton);
    await waitFor(() => expect(lingxiFetch).toHaveBeenLastCalledWith('/api/preferences/keybindings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ keybindings: {} }),
    }));
    await act(async () => { await new Promise(r => setTimeout(r, 0)); });
    expect(rowEl('app.restart').textContent).toContain('R');
    expect(resetAllButton.disabled).toBe(true); // 覆盖清零后回到禁用
  });
});
