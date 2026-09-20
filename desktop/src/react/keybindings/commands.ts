/**
 * 快捷键命令注册表（渲染层视图）。
 *
 * 键位与作用域的单一事实源在 shared/keybindings-preferences.cjs（主进程与
 * 服务端共用）；本文件在共享表之上补渲染层的展示信息（i18n key）与工具
 * 函数（事件归一、键帽展示、冲突检测），保证设置页与 dispatcher 拿到的
 * 命令清单永远同源。
 */

import {
  KEYBINDING_COMMANDS,
  findKeybindingConflicts,
  getEffectiveKeybindings,
  normalizeKeybindings,
} from '../../../../shared/keybindings-preferences.ts';
import type { KeybindingCommandDef } from '../../../../shared/keybindings-preferences.ts';

export type { KeybindingScope } from '../../../../shared/keybindings-preferences.ts';

export interface KeybindingCommandView {
  id: string;
  scope: 'global' | 'app' | 'local';
  labelKey: string;
  hintKey: string;
  defaultKeys: string[];
}

/**
 * 命令 id → locale 键名的显式映射。不自动推导：kebab/点号分隔的 id
 * 与驼峰键名的边界规则容易错配（quick-chat.toggle ≠ quickChattoggle），
 * 显式表一眼可核对，新增命令时必须在这里登记。
 */
const COMMAND_I18N_KEYS: Record<string, string> = {
  'quick-chat.toggle': 'quickChatToggle',
  'app.restart': 'appRestart',
  'app.open-settings': 'appOpenSettings',
  'app.new-session': 'appNewSession',
  'app.toggle-sidebar': 'appToggleSidebar',
  'voice.record-toggle': 'voiceRecordToggle',
};

/** 每条命令的展示文案 key + 共享层的键位/作用域定义。 */
export const KEYBINDING_COMMAND_VIEWS: KeybindingCommandView[] = KEYBINDING_COMMANDS.map((command: KeybindingCommandDef) => {
  const i18nKey = COMMAND_I18N_KEYS[command.id] ?? command.id;
  return {
    id: command.id,
    scope: command.scope,
    defaultKeys: command.defaultKeys.slice(),
    labelKey: `settings.keybindings.commands.${i18nKey}`,
    hintKey: `settings.keybindings.commands.${i18nKey}Hint`,
  };
});

/** 显式覆盖 → 有效键位表（未覆盖命令回落默认值）。 */
export function effectiveBindings(stored: unknown): Record<string, string[]> {
  return getEffectiveKeybindings(normalizeKeybindings(stored ?? {}));
}

/** 给定命令的新键位是否与其他命令冲突。返回冲突命令 id 列表。 */
export function conflictsFor(commandId: string, bindings: string[], stored: unknown): string[] {
  return findKeybindingConflicts(commandId, bindings, normalizeKeybindings(stored ?? {}));
}

/** 键位字符串 → keycap 展示序列（CommandOrControl 按平台展开）。 */
export function comboLabelParts(combo: string): string[] {
  return String(combo || '')
    .split('+')
    .map((part) => part.trim())
    .filter(Boolean)
    .map(keyLabel);
}

export function keyLabel(key: string): string {
  const isMac = typeof navigator !== 'undefined' && navigator.platform.toLowerCase().includes('mac');
  if (key === 'CommandOrControl') return isMac ? '⌘' : 'Ctrl';
  if (key === 'Control') return 'Ctrl';
  if (key === 'Alt') return isMac ? '⌥' : 'Alt';
  if (key === 'Meta') return isMac ? '⌘' : 'Win';
  if (key === 'Shift') return isMac ? '⇧' : 'Shift';
  if (key === 'Space') return 'Space';
  if (key === ',') return ',';
  return key.length === 1 ? key.toUpperCase() : key;
}

/**
 * KeyboardEvent → 键位字符串（与录制规则一致）：
 * CommandOrControl+Alt+Shift+主键；纯修饰键按无效处理（返回 null），
 * 无修饰符的键只接受功能键（F1-F24），避免劫持普通输入。
 */
export function comboFromEvent(event: KeyboardEvent): string | null {
  if (event.key === 'Escape') return null;
  if (['Shift', 'Control', 'Alt', 'Meta'].includes(event.key)) return null;

  const parts: string[] = [];
  if (event.metaKey || event.ctrlKey) parts.push('CommandOrControl');
  if (event.altKey) parts.push('Alt');
  if (event.shiftKey) parts.push('Shift');

  const rawKey = comboTokenFromKeyboardEvent(event);
  if (!rawKey) return null;
  const key = rawKey.length === 1 ? rawKey.toUpperCase() : rawKey;
  const isFunctionKey = /^F([1-9]|1[0-9]|2[0-4])$/.test(key);
  if (parts.length === 0 && !isFunctionKey) return null;
  parts.push(key);
  return parts.join('+');
}

/** KeyboardEvent 主键 → 归一键名（Space/方向键/字母/数字/功能键）。 */
export function comboTokenFromKeyboardEvent(event: KeyboardEvent): string | null {
  if (event.code === 'Space' || event.key === ' ' || event.key === '\u00A0' || event.key === 'Spacebar') {
    return 'Space';
  }
  const keyMap: Record<string, string> = {
    ArrowUp: 'Up',
    ArrowDown: 'Down',
    ArrowLeft: 'Left',
    ArrowRight: 'Right',
    Enter: 'Enter',
    Tab: 'Tab',
    Backspace: 'Backspace',
    Delete: 'Delete',
  };
  if (keyMap[event.code]) return keyMap[event.code];
  if (keyMap[event.key]) return keyMap[event.key];
  if (/^Key[A-Z]$/.test(event.code)) return event.code.slice(3);
  if (/^Digit[0-9]$/.test(event.code)) return event.code.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(event.code)) return event.code;
  const key = event.key || '';
  return key.length === 1 ? key : key || null;
}

/**
 * 已触发事件的键位是否命中记录中的某组绑定。
 * index 里的键位是 CommandOrControl 形态；event 侧把 metaKey/ctrlKey
 * 归一成 CommandOrControl 后做字符串精确比较。
 */
export function eventMatchesBinding(event: KeyboardEvent, bindings: string[]): boolean {
  if (!bindings?.length) return false;
  const combo = comboFromEvent(event);
  return !!combo && bindings.includes(combo);
}
