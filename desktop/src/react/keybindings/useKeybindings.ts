/**
 * 应用级快捷键 dispatcher。
 *
 * 主窗口唯一的全局 keydown 分发器：从服务端拉取快捷键偏好，把
 * app 作用域命令的键位编成索引，document 捕获阶段匹配后调用
 * 对应 handler 并拦截事件。作用域约定：
 * - global（quick-chat.toggle）：主进程 globalShortcut 注册，不经这里；
 * - app：本 dispatcher 分发（打开设置/新建会话/切换侧边栏/重启）；
 * - local（voice.record-toggle）：本 dispatcher 只负责键位匹配，
 *   命中后广播 window CustomEvent，由 InputArea 按自身上下文决定是否响应。
 *
 * 偏好变更（设置页保存 / 跨窗广播 keybindings-changed）后调 refresh() 重建索引。
 */

import { lingxiFetch } from '../settings/api';
import { effectiveBindings, eventMatchesBinding } from './commands';

export type KeybindingCommandHandler = () => void;

export interface KeybindingHandlers {
  'app.restart'?: KeybindingCommandHandler;
  'app.open-settings'?: KeybindingCommandHandler;
  'app.new-session'?: KeybindingCommandHandler;
  'app.toggle-sidebar'?: KeybindingCommandHandler;
}

/** 语音录制键命中后广播的事件名，InputArea 监听这个而不是裸 keydown。 */
export const VOICE_RECORD_TOGGLE_EVENT = 'hana-voice-record-toggle';

export interface KeybindingRuntime {
  refresh: () => Promise<void>;
  dispose: () => void;
}

export function initKeybindings(handlers: KeybindingHandlers): KeybindingRuntime {
  // 键位 → 命令 id 索引；refresh 时整体重建，不做增量修补。
  let comboIndex = new Map<string, string>();
  let disposed = false;

  const rebuild = (stored: unknown) => {
    const bindings = effectiveBindings(stored);
    const next = new Map<string, string>();
    for (const [commandId, combos] of Object.entries(bindings)) {
      if (commandId === 'quick-chat.toggle') continue; // 主进程负责
      for (const combo of combos || []) next.set(combo, commandId);
    }
    comboIndex = next;
  };

  const fetchAndRebuild = async () => {
    try {
      const res = await lingxiFetch('/api/preferences/keybindings');
      const data = await res.json();
      if (!disposed) rebuild(data?.keybindings ?? {});
    } catch {
      // 拉取失败保持旧索引：键位回退默认值至少可用。
      if (!disposed && comboIndex.size === 0) rebuild({});
    }
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (comboIndex.size === 0) return;
    // 输入法组合中的按键不参与匹配。
    if (event.isComposing) return;
    let matched: string | null = null;
    for (const [combo, commandId] of comboIndex) {
      if (eventMatchesBinding(event, [combo])) {
        matched = commandId;
        break;
      }
    }
    if (!matched) return;
    event.preventDefault();
    event.stopPropagation();

    if (matched === 'voice.record-toggle') {
      window.dispatchEvent(new CustomEvent(VOICE_RECORD_TOGGLE_EVENT));
      return;
    }
    const handler = (handlers as Record<string, KeybindingCommandHandler | undefined>)[matched];
    if (handler) handler();
  };

  void fetchAndRebuild();
  document.addEventListener('keydown', onKeyDown, true);

  return {
    refresh: fetchAndRebuild,
    dispose: () => {
      disposed = true;
      document.removeEventListener('keydown', onKeyDown, true);
    },
  };
}
