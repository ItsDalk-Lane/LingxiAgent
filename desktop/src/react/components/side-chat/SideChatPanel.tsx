import { useCallback, useEffect, useRef, useState } from 'react';
import { useStore } from '../../stores';
import { sessionScopedValue } from '../../stores/session-slice';
import { loadMessages } from '../../stores/session-actions';
import { ensureSideChatSession } from '../../stores/side-chat-actions';
import { SessionScopeProvider } from '../session-scope-context';
import { SelectionQuoteActionSurface } from '../selection/SelectionQuoteActionSurface';
import { ChatMessageSurface } from '../chat/ChatMessageSurface';
import { InputArea } from '../InputArea';
import { RegionalErrorBoundary } from '../RegionalErrorBoundary';
import styles from './SideChatPanel.module.css';

/**
 * SideChatPanel — 右侧栏侧边对话面板。
 *
 * 组成与主聊天页一致：消息记录（ChatMessageSurface）+ 输入区（InputArea），
 * 两者都用 SessionScopeProvider 绑定到侧边会话，因此模型 / 思考级别 / 权限
 * 模式 / 附件 / 引用 / 草稿都各归各的，不会串到主对话。
 *
 * 会话由 /api/sessions/new-detached 创建（服务端建会话但不抢焦点），面板关闭
 * 只是从侧栏移除；会话本体与其记录保留在左侧会话列表中。
 */

const PANEL_DEFAULT_WIDTH = 460;
const PANEL_MIN_WIDTH = 320;
const PANEL_MAX_WIDTH = 720;
const PANEL_WIDTH_STORAGE_KEY = 'lingxi.sideChat.width';

function clampWidth(value: number): number {
    return Math.min(PANEL_MAX_WIDTH, Math.max(PANEL_MIN_WIDTH, Math.round(value)));
}

function readStoredWidth(): number {
    try {
        const raw = window.localStorage?.getItem(PANEL_WIDTH_STORAGE_KEY);
        const parsed = raw ? Number.parseInt(raw, 10) : Number.NaN;
        return Number.isFinite(parsed) ? clampWidth(parsed) : PANEL_DEFAULT_WIDTH;
    } catch {
        return PANEL_DEFAULT_WIDTH;
    }
}

export function SideChatPanel() {
    const open = useStore(s => s.sideChat.open);
    const status = useStore(s => s.sideChat.status);
    const sessionPath = useStore(s => s.sideChat.sessionPath);
    const error = useStore(s => s.sideChat.error);
    const closeSideChat = useStore(s => s.closeSideChat);
    const setCurrentSessionPathOverride = useStore(s => s.setCurrentSessionPathOverride);
    const itemCount = useStore(s => (sessionPath
        ? sessionScopedValue(s as never, s.chatSessions, sessionPath)?.items?.length ?? 0
        : 0));
    const [width, setWidth] = useState(readStoredWidth);
    const [hydrating, setHydrating] = useState(false);
    const resizeStateRef = useRef<{ startX: number; startWidth: number } | null>(null);

    // 创建会话：open 且尚无 path 时发起一次（换一次打开会换 createdAt，因此旧请求
    // 的结果会被丢弃，不会把上一次的会话塞进这一次的面板）。
    const createdAt = useStore(s => s.sideChat.createdAt);
    useEffect(() => {
        if (!open || sessionPath || !createdAt) return;
        void ensureSideChatSession(createdAt);
    }, [createdAt, open, sessionPath]);

    // 「默认会话」指向侧边会话：拖拽附件、技能斜杠菜单等不接收显式 sessionPath 的
    // 共享 UI 会落到用户正在看的这一面；面板关闭时归还主会话。
    useEffect(() => {
        if (!open) return undefined;
        setCurrentSessionPathOverride(sessionPath);
        return () => setCurrentSessionPathOverride(null);
    }, [open, sessionPath, setCurrentSessionPathOverride]);

    // 侧边会话历史首载：与主会话同源（loadMessages），不预种空缓存。
    useEffect(() => {
        if (!open || !sessionPath || itemCount > 0) return undefined;
        let cancelled = false;
        setHydrating(true);
        void loadMessages(sessionPath)
            .catch(err => console.warn('[side-chat] load messages failed:', err))
            .finally(() => { if (!cancelled) setHydrating(false); });
        return () => { cancelled = true; };
    }, [itemCount, open, sessionPath]);

    const handleResizeStart = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
        event.preventDefault();
        resizeStateRef.current = { startX: event.clientX, startWidth: width };
        const target = event.currentTarget;
        target.setPointerCapture?.(event.pointerId);
    }, [width]);

    const handleResizeMove = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
        const state = resizeStateRef.current;
        if (!state) return;
        setWidth(clampWidth(state.startWidth + (state.startX - event.clientX)));
    }, []);

    const handleResizeEnd = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
        const state = resizeStateRef.current;
        resizeStateRef.current = null;
        event.currentTarget.releasePointerCapture?.(event.pointerId);
        if (!state) return;
        setWidth(current => {
            try {
                window.localStorage?.setItem(PANEL_WIDTH_STORAGE_KEY, String(current));
            } catch {
                // 存储不可用只是丢一个宽度偏好，不影响面板行为。
            }
            return current;
        });
    }, []);

    if (!open) return null;

    const t = (key: string, vars?: Record<string, string | number>) => {
        const translated = window.t?.(key, vars);
        return translated && translated !== key ? translated : key;
    };

    return (
        <SessionScopeProvider sessionPath={sessionPath}>
            <aside
                className={styles.panel}
                style={{ '--side-chat-width': `${width}px` } as React.CSSProperties}
                data-side-chat-panel="true"
                aria-label={t('sideChat.title')}
            >
                <div
                    className={styles.resizeHandle}
                    role="separator"
                    aria-orientation="vertical"
                    onPointerDown={handleResizeStart}
                    onPointerMove={handleResizeMove}
                    onPointerUp={handleResizeEnd}
                    onPointerCancel={handleResizeEnd}
                />
                <div className={styles.header}>
                    <div className={styles.headerTitle}>
                        <svg className={styles.headerIcon} width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                            <rect x="3" y="4" width="18" height="16" rx="2" />
                            <line x1="14" y1="4" x2="14" y2="20" />
                            <path d="M6.5 9h4" />
                            <path d="M6.5 13h4" />
                        </svg>
                        <span className={styles.headerTitleText}>{t('sideChat.title')}</span>
                    </div>
                    <button
                        type="button"
                        className={styles.closeButton}
                        onClick={closeSideChat}
                        aria-label={t('sideChat.close')}
                        title={t('sideChat.close')}
                    >
                        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
                            <line x1="6" y1="6" x2="18" y2="18" />
                            <line x1="18" y1="6" x2="6" y2="18" />
                        </svg>
                    </button>
                </div>

                <div className={styles.body}>
                    {status === 'error' && (
                        <div className={`${styles.stateNotice} ${styles.stateError}`} role="status">
                            <span>{t('sideChat.createFailed')}</span>
                            <span>{error}</span>
                            <button
                                type="button"
                                className={styles.stateAction}
                                onClick={() => { void ensureSideChatSession(useStore.getState().sideChat.createdAt); }}
                            >
                                {t('sideChat.retry')}
                            </button>
                        </div>
                    )}
                    {status !== 'error' && !sessionPath && (
                        <div className={styles.stateNotice} role="status">
                            <span>{t('sideChat.creating')}</span>
                        </div>
                    )}
                    {sessionPath && (
                        <>
                            <div className={`${styles.messages} chat-area`}>
                                <RegionalErrorBoundary region="side-chat-transcript" resetKeys={[sessionPath]}>
                                    <ChatMessageSurface
                                        key={sessionPath}
                                        sessionPath={sessionPath}
                                        active
                                    />
                                </RegionalErrorBoundary>
                                {itemCount === 0 && !hydrating && (
                                    <div className={styles.emptyHint}>{t('sideChat.empty')}</div>
                                )}
                            </div>
                            <div className={styles.composer}>
                                <RegionalErrorBoundary region="side-chat-input" resetKeys={[sessionPath]}>
                                    <InputArea
                                        key={sessionPath}
                                        surface="desktop"
                                        sessionScope={{ sessionPath, isScoped: true }}
                                    />
                                </RegionalErrorBoundary>
                            </div>
                            {/* 在侧边面板里选中文本 → 引用/再开侧边对话：浮层归属本会话 */}
                            <SelectionQuoteActionSurface scopeSessionPath={sessionPath} />
                        </>
                    )}
                </div>
            </aside>
        </SessionScopeProvider>
    );
}
