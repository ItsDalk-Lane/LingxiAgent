/**
 * FloatSidebar — 左侧栏折叠时 hover 滑入的全高面板
 *
 * 内容 = 完整 ChatSidebarContent 新结构（顶部功能图标行、Session 区、工作台区）。
 * 原右侧浮出（旧 RightWorkspacePanel 的 hover 展示路径）已随固定右栏移除而退役。
 */

import { useState, useCallback } from 'react';
import { useStore } from '../stores';
import { useAnimatePresence } from '../hooks/use-animate-presence';
import { openSettingsModal } from '../stores/settings-modal-actions';
import { ChatSidebarContent } from './app/ChatSidebar';

import type { ActivePanel } from '../types';

type FloatSidebarSide = 'left';

let _enterTimer: ReturnType<typeof setTimeout> | null = null;
let _leaveTimer: ReturnType<typeof setTimeout> | null = null;

export function useFloatSidebar() {
  const [side, setSide] = useState<FloatSidebarSide | null>(null);

  const show = useCallback(() => {
    if (_leaveTimer) clearTimeout(_leaveTimer);
    if (_enterTimer) clearTimeout(_enterTimer);
    _enterTimer = setTimeout(() => {
      if (useStore.getState().sidebarOpen) return;
      setSide('left');
    }, 200);
  }, []);

  const scheduleHide = useCallback(() => {
    if (_enterTimer) clearTimeout(_enterTimer);
    _leaveTimer = setTimeout(() => setSide(null), 200);
  }, []);

  const cancelHide = useCallback(() => {
    if (_leaveTimer) clearTimeout(_leaveTimer);
  }, []);

  const hide = useCallback(() => {
    if (_enterTimer) clearTimeout(_enterTimer);
    if (_leaveTimer) clearTimeout(_leaveTimer);
    setSide(null);
  }, []);

  return { side, show, scheduleHide, cancelHide, hide };
}

const FLOAT_SIDEBAR_ANIM_DURATION = 250;

function togglePanel(panel: ActivePanel) {
  const s = useStore.getState();
  s.setActivePanel(s.activePanel === panel ? null : panel);
}

export function FloatSidebar({
  side,
  onMouseEnter,
  onMouseLeave,
  onAction,
}: {
  side: FloatSidebarSide | null;
  onMouseEnter: () => void;
  onMouseLeave: () => void;
  onAction: () => void;
}) {
  const { mounted, stage } = useAnimatePresence(side !== null, {
    duration: FLOAT_SIDEBAR_ANIM_DURATION,
  });

  if (!mounted) return null;

  return (
    <div
      className="float-sidebar"
      data-side="left"
      data-stage={stage}
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
    >
      <div className="float-sidebar-panel float-sidebar-panel-left">
        <LeftPanel onAction={onAction} />
      </div>
    </div>
  );
}

function LeftPanel({ onAction }: { onAction: () => void }) {
  const handleOpenSettings = useCallback(() => {
    onAction();
    openSettingsModal();
  }, [onAction]);

  return (
    <div className="sidebar-chat-content">
      <ChatSidebarContent
        showSettingsButton
        showActivityBars
        onOpenSettings={handleOpenSettings}
        onTogglePanel={togglePanel}
        region="float-sidebar"
      />
    </div>
  );
}
