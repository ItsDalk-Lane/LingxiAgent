import { ChannelTabBar } from '../channels/ChannelTabBar';
import { WidgetButtons } from '../plugin/WidgetButtons';
import { WindowControls } from '../WindowControls';

interface AppTitlebarProps {
  sidebarOpen: boolean;
  /** 移动端右栏（工作台抽屉）开关状态；桌面端不再传，#tbToggleRight 不渲染 */
  jianOpen?: boolean;
  onToggleSidebar: () => void;
  /** 传入时才渲染右栏开关（移动端保留；桌面端固定右栏已移除） */
  onToggleJian?: () => void;
  /** 传入时才渲染聊天搜索按钮（桌面端；移动端不传不渲染） */
  onOpenChatSearch?: () => void;
  chatSearchOpen?: boolean;
  onNewSession?: () => void;
  previewOpen?: boolean;
  onTogglePreview?: () => void;
  centerTitle?: string | null;
  showNewSessionButton?: boolean;
  showPreviewToggle?: boolean;
  showChannelTabs?: boolean;
  showWidgetButtons?: boolean;
  onLeftMouseEnter?: () => void;
  onRightMouseEnter?: () => void;
  onToggleMouseLeave?: () => void;
}

export function AppTitlebar({
  sidebarOpen,
  jianOpen = false,
  onToggleSidebar,
  onToggleJian,
  onOpenChatSearch,
  chatSearchOpen = false,
  onNewSession,
  previewOpen = false,
  onTogglePreview,
  centerTitle = null,
  showNewSessionButton = false,
  showPreviewToggle = false,
  showChannelTabs = true,
  showWidgetButtons = true,
  onLeftMouseEnter,
  onRightMouseEnter,
  onToggleMouseLeave,
}: AppTitlebarProps) {
  const t = window.t ?? ((p: string) => p);

  return (
    <div className="titlebar">
      <div className="tb-left-group">
        <button
          className={`tb-toggle tb-toggle-left${sidebarOpen ? ' active' : ''}`}
          id="tbToggleLeft"
          title={t('sidebar.toggle')}
          onMouseDown={(e) => e.preventDefault()}
          onClick={onToggleSidebar}
          onMouseEnter={onLeftMouseEnter}
          onMouseLeave={onToggleMouseLeave}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
            <rect x="3" y="3" width="18" height="18" rx="2" ry="2"></rect>
            <line x1="9" y1="3" x2="9" y2="21"></line>
          </svg>
        </button>
        {onOpenChatSearch && (
          <button
            className={`tb-toggle tb-toggle-search${chatSearchOpen ? ' active' : ''}`}
            id="tbToggleSearch"
            type="button"
            title={t('titlebar.search')}
            aria-label={t('titlebar.search')}
            data-titlebar-chat-search=""
            onMouseDown={(e) => e.preventDefault()}
            onClick={onOpenChatSearch}
          >
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <circle cx="11" cy="11" r="7.5" />
              <path d="m21 21-4.3-4.3" />
            </svg>
          </button>
        )}
        {showNewSessionButton && onNewSession && (
          <button
            className="tb-toggle tb-new-session"
            type="button"
            title={t('sidebar.newChat')}
            aria-label={t('sidebar.newChat')}
            data-mobile-titlebar-action="new-session"
            onMouseDown={(e) => e.preventDefault()}
            onClick={onNewSession}
          >
            <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
              <line x1="12" y1="5" x2="12" y2="19"></line>
              <line x1="5" y1="12" x2="19" y2="12"></line>
            </svg>
          </button>
        )}
      </div>
      {centerTitle && (
        <div className="tb-center-title" aria-label={t('titlebar.currentChatTitle')} title={centerTitle}>
          <span>{centerTitle}</span>
        </div>
      )}
      {showChannelTabs && <ChannelTabBar />}
      <div className="tb-right-group">
        {showWidgetButtons && <WidgetButtons />}
        {showPreviewToggle && onTogglePreview && (
          <button
            className={`tb-toggle tb-toggle-preview${previewOpen ? ' active' : ''}`}
            id="tbTogglePreview"
            title={t('preview.toggle')}
            onMouseDown={(e) => e.preventDefault()}
            onClick={onTogglePreview}
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
              <path d="M7 3.5h7l3 3v14H7z"></path>
              <path d="M14 3.5v3h3"></path>
              <path d="M9.5 11h5"></path>
              <path d="M9.5 14.5h5"></path>
            </svg>
          </button>
        )}
        {onToggleJian && (
          <button
            className={`tb-toggle tb-toggle-right${jianOpen ? ' active' : ''}`}
            id="tbToggleRight"
            title={t('sidebar.jian')}
            onMouseDown={(e) => e.preventDefault()}
            onClick={onToggleJian}
            onMouseEnter={onRightMouseEnter}
            onMouseLeave={onToggleMouseLeave}
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
              <rect x="3" y="3" width="18" height="18" rx="2" ry="2"></rect>
              <line x1="15" y1="3" x2="15" y2="21"></line>
            </svg>
          </button>
        )}
      </div>
      <WindowControls />
    </div>
  );
}
