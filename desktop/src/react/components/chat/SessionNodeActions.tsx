import { useCallback, useRef, useState } from 'react';
import type { MouseEvent, ReactNode } from 'react';
import type { ChatMessage } from '../../stores/chat-types';
import { useStore } from '../../stores';
import {
  activateForkedSession,
  forkSessionTurn,
  previewWorkspaceRollback,
  retrySessionTurn,
  type ForkedSessionHandler,
  type SessionNodeTarget,
  type WorkspaceRollbackPreview,
} from '../../stores/message-turn-actions';
import { presentError } from '../../errors/error-presenter';
import { ConfirmDialog, ContextMenu } from '@/ui';
import type { MessageFooterAction } from './MessageFooterActions';
import styles from './SessionRollback.module.css';

interface Options {
  sessionPath: string;
  target: SessionNodeTarget | null;
  retryMessage?: ChatMessage;
  onForkCreated?: ForkedSessionHandler;
  disabled?: boolean;
}

export function useSessionNodeActions({
  sessionPath,
  target,
  retryMessage,
  onForkCreated,
  disabled = false,
}: Options): { actions: MessageFooterAction[]; busy: boolean; overlay: ReactNode } {
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const [menuPosition, setMenuPosition] = useState<{ x: number; y: number } | null>(null);
  const [preview, setPreview] = useState<WorkspaceRollbackPreview | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const t: (key: string, vars?: Record<string, string | number>) => string = window.t ?? ((key: string) => key);

  const runRetry = useCallback(async (fileRollback: 'none' | 'workspace') => {
    if (!target || busyRef.current || disabled) return;
    busyRef.current = true;
    setBusy(true);
    try {
      // fileRollback='none' 时完全保持现状调用形状（等价于没有这个功能）。
      const rollbackOptions = fileRollback === 'workspace' ? { fileRollback: 'workspace' as const } : null;
      if (retryMessage) {
        await retrySessionTurn(sessionPath, target, rollbackOptions
          ? { message: retryMessage, ...rollbackOptions }
          : { message: retryMessage });
      } else if (rollbackOptions) {
        await retrySessionTurn(sessionPath, target, rollbackOptions);
      } else {
        await retrySessionTurn(sessionPath, target);
      }
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }, [disabled, retryMessage, sessionPath, target]);

  const openRollbackMenu = useCallback(async (event: MouseEvent<HTMLButtonElement>) => {
    if (!target || busyRef.current || disabled) return;
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect?.();
    setMenuPosition({ x: rect?.left ?? 0, y: (rect?.bottom ?? 0) + 4 });
    setPreview(null);
    if (typeof previewWorkspaceRollback !== 'function') return;
    try {
      const next = await previewWorkspaceRollback(sessionPath, target);
      setPreview(next);
    } catch {
      // 预览失败按「开关关闭」处理：菜单只剩默认选项。
      setPreview({ enabled: false, available: false, degraded: false, commit: null, reason: 'preview_failed', files: [], fileCount: 0 });
    }
  }, [disabled, sessionPath, target]);

  const handleFork = useCallback(async () => {
    if (!target || busyRef.current || disabled) return;
    busyRef.current = true;
    setBusy(true);
    try {
      const forked = await forkSessionTurn(sessionPath, target);
      if (!forked) return;
      await (onForkCreated || activateForkedSession)(forked);
      if (target.role === 'user' && retryMessage) {
        await retrySessionTurn(forked.sessionPath, target, { message: retryMessage });
      }
    } catch (error) {
      useStore.getState().setInlineError?.(sessionPath, presentError(error), 6000);
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }, [disabled, onForkCreated, retryMessage, sessionPath, target]);

  const actions: MessageFooterAction[] = target ? [
    {
      id: 'regenerate',
      title: t('common.regenerate'),
      icon: <RegenerateIcon />,
      onClick: (event) => { void openRollbackMenu(event); },
      disabled: disabled || busy,
    },
    {
      id: 'fork-session',
      title: t('common.forkSession'),
      icon: <ForkIcon />,
      onClick: () => { void handleFork(); },
      disabled: disabled || busy,
    },
  ] : [];

  // 第二个选项的可用性完全由服务端预览裁决：开关关闭 → 置灰并给出开启位置；
  // 无检查点 / 工作区不可用 → 置灰并说明；快照降级 → 可用但提示将用备份倒推。
  const withFilesItem = (() => {
    if (!preview) return { label: t('chat.fileRollback.withFiles'), disabled: true };
    if (!preview.enabled) {
      return { label: `${t('chat.fileRollback.withFiles')}（${t('chat.fileRollback.disabledOff')}）`, disabled: true };
    }
    if (!preview.available) {
      const reason = preview.reason === 'no_checkpoint'
        ? t('chat.fileRollback.disabledNoCheckpoint')
        : t('chat.fileRollback.disabledUnavailable');
      return { label: `${t('chat.fileRollback.withFiles')}（${reason}）`, disabled: true };
    }
    return {
      label: preview.degraded
        ? t('chat.fileRollback.withFilesDegraded')
        : t('chat.fileRollback.withFilesCount', { count: preview.fileCount }),
      disabled: false,
    };
  })();

  const menuItems: Array<{ label: string; action: () => void; disabled?: boolean }> = [
    {
      label: t('chat.fileRollback.conversationOnly'),
      action: () => { void runRetry('none'); },
    },
  ];
  // 开关关闭时界面不出这个选项（拍板 #3 / 完成条件 1），而不是给一个必然 4xx 的入口。
  if (preview?.enabled === true) {
    menuItems.push({
      label: withFilesItem.label,
      disabled: withFilesItem.disabled,
      action: () => setConfirmOpen(true),
    });
  }

  const overlay = (
    <>
      {menuPosition && (
        <ContextMenu
          position={menuPosition}
          onClose={() => setMenuPosition(null)}
          items={menuItems}
        />
      )}
      <ConfirmDialog
        open={confirmOpen}
        scope="window"
        title={t('chat.fileRollback.confirmTitle')}
        confirmLabel={t('chat.fileRollback.confirmAction')}
        cancelLabel={t('common.cancel')}
        confirmTone="danger"
        busy={busy}
        onCancel={() => setConfirmOpen(false)}
        onConfirm={() => { setConfirmOpen(false); void runRetry('workspace'); }}
      >
        <p>{t('chat.fileRollback.confirmBody', { count: preview?.fileCount ?? 0 })}</p>
        {!!preview?.files?.length && (
          <ul className={styles.confirmFiles} data-testid="file-rollback-confirm-list">
            {preview.files.map((file) => <li key={file.path}>{file.path}</li>)}
          </ul>
        )}
        <p className={styles.confirmWarning} data-testid="file-rollback-confirm-warning">{t('chat.fileRollback.confirmWarning')}</p>
      </ConfirmDialog>
    </>
  );

  return { actions, busy, overlay };
}

function RegenerateIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M21 3v5m0 0h-5m5 0-3-2.708A9 9 0 1 0 20.777 14" />
    </svg>
  );
}

function ForkIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 512 512" fill="currentColor" aria-hidden="true">
      <path d="M124,166.291V345.709a76,76,0,1,0,32,0V282H308a80.091,80.091,0,0,0,80-80V165.311a75.983,75.983,0,1,0-32,1.733V202a48.055,48.055,0,0,1-48,48H156V166.291a76,76,0,1,0-32,0ZM324,92a44,44,0,1,1,44,44A44.049,44.049,0,0,1,324,92ZM184,420a44,44,0,1,1-44-44A44.049,44.049,0,0,1,184,420ZM140,48A44,44,0,1,1,96,92,44.049,44.049,0,0,1,140,48Z" />
    </svg>
  );
}
