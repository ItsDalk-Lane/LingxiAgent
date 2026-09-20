/**
 * CompactionAskDialog — 50% 压缩询问线（shared/compaction-thresholds.ts）的渲染端。
 *
 * 服务端在用量跨越 50% 时发 `compaction_suggested`（不阻塞进行中的 run），
 * 这里只对「当前正在查看的会话」弹窗；其它会话的询问保留在 store 里，
 * 用户切回去时再弹。确认后的行为分两路：
 *   - 会话空闲：立即发 compact（与 ContextRing 手动压缩同一协议）
 *   - 会话正在回复：登记 pendingAutoCompact，等 assistant_run_end 后自动压缩
 *     （服务端在 streaming 中会拒绝压缩请求）
 */
import { useCallback } from 'react';
import { useStore } from '../../stores';
import { sessionScopedListIncludes } from '../../stores/session-slice';
import { useScopedSessionPath } from '../session-scope-context';
import { useI18n } from '../../hooks/use-i18n';
import { getWebSocket } from '../../services/websocket';
import { ConfirmDialog } from '../../ui';

export function CompactionAskDialog() {
  const { t } = useI18n();
  const currentSessionPath = useScopedSessionPath();
  const ask = useStore(s => (
    currentSessionPath ? s.compactionAskBySession?.[currentSessionPath] ?? null : null
  ));
  const streaming = useStore(s => (
    currentSessionPath
      ? sessionScopedListIncludes(s, s.streamingSessions, currentSessionPath)
      : false
  ));
  const currentSessionId = useStore(s => (
    currentSessionPath && s.currentSessionPath === currentSessionPath
      ? s.currentSessionId
      : null
  ));
  const addToast = useStore(s => s.addToast);
  const clearCompactionAsk = useStore(s => s.clearCompactionAsk);
  const addPendingAutoCompact = useStore(s => s.addPendingAutoCompact);

  const open = !!ask && !!currentSessionPath;

  const dismiss = useCallback(() => {
    if (currentSessionPath) clearCompactionAsk?.(currentSessionPath);
  }, [clearCompactionAsk, currentSessionPath]);

  const handleCancel = useCallback(() => {
    dismiss();
  }, [dismiss]);

  const handleConfirm = useCallback(() => {
    if (!currentSessionPath) return;
    if (streaming) {
      addPendingAutoCompact?.(currentSessionPath);
      addToast?.(t('compaction.deferredToast'), 'info', 6000, {
        dedupeKey: `compaction-deferred:${currentSessionPath}`,
      });
      dismiss();
      return;
    }
    if (!currentSessionId) {
      addToast?.(t('error.noActiveSession'), 'error', 6000);
      dismiss();
      return;
    }
    const ws = getWebSocket();
    if (!ws || ws.readyState !== WebSocket.OPEN) {
      addToast?.(t('status.disconnected'), 'error', 6000);
      dismiss();
      return;
    }
    ws.send(JSON.stringify({ type: 'compact', sessionId: currentSessionId }));
    dismiss();
  }, [addPendingAutoCompact, addToast, currentSessionId, currentSessionPath, dismiss, streaming, t]);

  return (
    <ConfirmDialog
      open={open}
      scope="window"
      title={t('compaction.askTitle')}
      confirmLabel={t('compaction.askConfirm')}
      cancelLabel={t('compaction.askCancel')}
      onConfirm={handleConfirm}
      onCancel={handleCancel}
      closeOnBackdrop={false}
      closeOnEsc
      zIndex={9999}
    >
      <p>{t('compaction.askBody', {
        percent: String(ask?.percent ?? ''),
        askPercent: String(ask?.askPercent ?? 50),
        forcePercent: String(ask?.forcePercent ?? 80),
      })}</p>
    </ConfirmDialog>
  );
}
