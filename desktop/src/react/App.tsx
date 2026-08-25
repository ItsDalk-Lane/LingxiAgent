/**
 * App.tsx — React 根组件（纯布局编排）
 *
 * 初始化逻辑在 app-init.ts，拖拽/主内容区在 MainContent.tsx。
 * 此文件只负责 titlebar + sidebar + 主区域 + overlays 的组装。
 */

import { useEffect, lazy, Suspense } from 'react';
import { useStore } from './stores';
import type { ActivePanel } from './types';
import { ErrorBoundary } from './components/ErrorBoundary';
import { RegionalErrorBoundary } from './components/RegionalErrorBoundary';

const SkillViewerOverlay = lazy(() => import('./components/SkillViewerOverlay').then(m => ({ default: m.SkillViewerOverlay })));
import { ChannelsPanel } from './components/ChannelsPanel';
import { ChannelCreateOverlay } from './components/channels/ChannelCreateOverlay';
import { SidebarLayout, toggleSidebar } from './components/SidebarLayout';
import { FloatSidebar, useFloatSidebar } from './components/FloatSidebar';
import { useSidebarResize } from './hooks/use-sidebar-resize';
import { ToastContainer } from './components/ToastContainer';
import { InputContextMenu } from './components/InputContextMenu';
import { StatusBar } from './components/StatusBar';
import { LeavesOverlay } from './components/LeavesOverlay';
import { SelectionQuoteActionSurface } from './components/selection/SelectionQuoteActionSurface';
import { MediaViewer } from './components/shared/MediaViewer/MediaViewer';
import { SettingsModalShell } from './components/SettingsModalShell';
import { FileHistoryModal } from './components/file-history/FileHistoryModal';
import { initTheme, initDragPrevention } from './bootstrap';
import { initApp } from './app-init';
import { openSettingsModal } from './stores/settings-modal-actions';
import { AppTitlebar } from './components/app/AppTitlebar';
import { ChatSidebar } from './components/app/ChatSidebar';
import { AppPages } from './components/app/AppPages';
import { ChatSearchOverlay } from './components/search/ChatSearchOverlay';

declare function t(key: string, vars?: Record<string, string | number>): string;

// ── 主题 + drag 阻止（import 时立即执行） ──
initTheme();
initDragPrevention();

// ── 面板切换 ──

function togglePanel(panel: ActivePanel) {
  const s = useStore.getState();
  s.setActivePanel(s.activePanel === panel ? null : panel);
}

function ConnectionStatus() {
  const connected = useStore(s => s.connected);
  const statusKey = useStore(s => s.statusKey);
  const statusVars = useStore(s => s.statusVars);
  return (
    <div className={`connection-status${connected ? ' connected' : ''}`}>
      <span className="status-dot"></span>
      <span className="status-text">{statusKey ? t(statusKey, statusVars) : ''}</span>
    </div>
  );
}

// ── App 根组件 ──

function App() {
  useSidebarResize();
  // 订阅 locale 变化，驱动整棵树重渲染
  useStore(s => s.locale);
  const sidebarOpen = useStore(s => s.sidebarOpen);
  const chatSearchOpen = useStore(s => s.chatSearchOpen);
  const setChatSearchOpen = useStore(s => s.setChatSearchOpen);
  const currentTab = useStore(s => s.currentTab);
  const isPluginTab = typeof currentTab === 'string' && currentTab.startsWith('plugin:');
  const { side: floatSide, show: showFloat, scheduleHide: scheduleFloatHide, cancelHide: cancelFloatHide, hide: hideFloat } = useFloatSidebar();

  useEffect(() => {
    console.info('[hana-launch] init-start');
    initApp()
      .then(() => {
        console.info('[hana-launch] init-finished');
      })
      .catch((err: unknown) => {
        console.error('[init] 初始化异常:', err);
        console.error('[hana-launch] init-failed', err);
        console.info('[hana-launch] app-ready', JSON.stringify({ reason: 'init-failed' }));
        window.platform?.appReady?.();
      });
  }, []);

  return (
    <ErrorBoundary>
      {/* Headless behavior components */}
      <SidebarLayout />
      <ChannelsPanel />

      {/* ── App shell: titlebar 作为独立布局行（flex column 第一行），
           app body 占剩余高度。取代旧的 fixed overlay + 各内容区 padding-top 避让。 ── */}
      <div className="app-shell">
        {/* ── Titlebar ── */}
        <AppTitlebar
          sidebarOpen={sidebarOpen}
          onToggleSidebar={() => { hideFloat(); toggleSidebar(); }}
          onLeftMouseEnter={() => showFloat()}
          onToggleMouseLeave={scheduleFloatHide}
          chatSearchOpen={chatSearchOpen}
          onOpenChatSearch={() => setChatSearchOpen(!chatSearchOpen)}
        />

        {/* ── App body ── */}
        <div className="app">
          <ChatSidebar
            open={sidebarOpen && currentTab !== 'knowledge' && !isPluginTab}
            onOpenSettings={() => openSettingsModal()}
            onTogglePanel={togglePanel}
          />

          <RegionalErrorBoundary region="app-pages" resetKeys={[currentTab]}>
            <AppPages />
          </RegionalErrorBoundary>
        </div>
      </div>

      {/* Connection status */}
      <ConnectionStatus />

      {/* Channel create overlay */}
      <ChannelCreateOverlay />

      {/* Centered chat search overlay (desktop titlebar entry) */}
      <ChatSearchOverlay />

      {/* Skill viewer overlay */}
      <Suspense fallback={null}><SkillViewerOverlay /></Suspense>

      {/* Float sidebar */}
      <FloatSidebar
        side={floatSide}
        onMouseEnter={cancelFloatHide}
        onMouseLeave={scheduleFloatHide}
        onAction={hideFloat}
      />

      {/* Connection status bar */}
      <StatusBar />

      {/* Leaves shadow overlay */}
      <LeavesOverlay />

      {/* Media viewer overlay */}
      <MediaViewer />

      {/* In-window settings overlay */}
      <SettingsModalShell />

      {/* Workspace file history overlay */}
      <FileHistoryModal />

      {/* Input context menu (cut/copy/paste) */}
      <InputContextMenu />

      {/* Selection quote action */}
      <SelectionQuoteActionSurface />

      {/* Toast notifications */}
      <ToastContainer />

    </ErrorBoundary>
  );
}

export default App;
