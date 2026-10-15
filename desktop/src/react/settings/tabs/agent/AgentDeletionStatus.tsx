import { useEffect, useState } from 'react';
import { lingxiFetchJson } from '../../api';
import { useSettingsStore } from '../../store';
import { captureSettingsConnection, ownsSettingsConnection, settingsConnectionRevision, type SettingsConnectionOwner } from '../../connection-owner';
import { loadRustAgents } from '../../rust-agents';
import { loadSettingsConfig } from '../../actions';
import { t } from '../../helpers';

interface DeletionReceipt {
  deletionId: string;
  agentId: string;
  logicalDeleted: boolean;
  cleanupPending: boolean;
  referencesPending: boolean;
  journalCheckpointPending?: boolean;
  cleanupError?: string;
  referenceError?: string;
  journalError?: string;
}

/** 删除已经生效与后续磁盘清理是两个真实状态；待清理收据提供同连接重试。 */
export function AgentDeletionStatus() {
  useSettingsStore();
  const revision = settingsConnectionRevision();
  const [snapshot, setSnapshot] = useState<{ owner: SettingsConnectionOwner; receipts: DeletionReceipt[] } | null>(null);
  const receipts = snapshot && ownsSettingsConnection(snapshot.owner) ? snapshot.receipts : [];
  const [error, setError] = useState('');
  const [retrying, setRetrying] = useState<string | null>(null);
  useEffect(() => {
    const owner = captureSettingsConnection();
    const controller = new AbortController();
    setError('');
    setSnapshot(previous => previous && ownsSettingsConnection(previous.owner) ? previous : { owner, receipts: [] });
    const refresh = async () => {
      if (!ownsSettingsConnection(owner)) return;
      try {
        const result = await lingxiFetchJson<{ deletions: DeletionReceipt[] }>('/api/agent-deletions', { signal: controller.signal });
        if (!Array.isArray(result.deletions) || !result.deletions.every(receipt => typeof receipt.deletionId === 'string' && typeof receipt.agentId === 'string' && typeof receipt.logicalDeleted === 'boolean' && typeof receipt.cleanupPending === 'boolean' && typeof receipt.referencesPending === 'boolean')) throw new Error(t('settings.agentDeletion.formatError'));
        if (!controller.signal.aborted && ownsSettingsConnection(owner)) { setSnapshot({ owner, receipts: result.deletions }); setError(''); }
      } catch (failure) {
        if (!controller.signal.aborted && ownsSettingsConnection(owner)) setError(failure instanceof Error ? failure.message : String(failure));
      }
    };
    void refresh();
    window.addEventListener('hana-agent-deletion-changed', refresh);
    return () => { controller.abort(); window.removeEventListener('hana-agent-deletion-changed', refresh); };
  }, [revision]);
  const retry = async (receipt: DeletionReceipt) => {
    const owner = snapshot?.owner;
    if (retrying || !owner || !ownsSettingsConnection(owner) || !receipts.includes(receipt)) return;
    let changed = false;
    const unsubscribe = useSettingsStore.subscribe(() => { if (!ownsSettingsConnection(owner)) changed = true; });
    setRetrying(receipt.deletionId); setError('');
    try {
      // 同步确认与请求目标捕获之间无 await；等待后即使连接又恢复也不能消费旧结果。
      if (changed || !ownsSettingsConnection(owner)) throw new Error(t('settings.agentDeletion.connectionChanged'));
      const result = await lingxiFetchJson<DeletionReceipt>(`/api/agent-deletions/${encodeURIComponent(receipt.deletionId)}/cleanup`, { method: 'POST' });
      if (changed || !ownsSettingsConnection(owner)) throw new Error(t('settings.agentDeletion.resultChanged'));
      if (result.deletionId !== receipt.deletionId || typeof result.logicalDeleted !== 'boolean') throw new Error(t('settings.agentDeletion.formatError'));
      setSnapshot(previous => previous && ownsSettingsConnection(previous.owner) ? { ...previous, receipts: previous.receipts.map(item => item.deletionId === result.deletionId ? result : item) } : previous);
      await loadRustAgents();
      if (changed || !ownsSettingsConnection(owner)) throw new Error(t('settings.agentDeletion.connectionChanged'));
      await loadSettingsConfig();
    } catch (failure) { if (ownsSettingsConnection(owner)) setError(failure instanceof Error ? failure.message : String(failure)); }
    finally { unsubscribe(); setRetrying(null); }
  };
  const pending = receipts.filter(receipt => receipt.logicalDeleted && (receipt.cleanupPending || receipt.referencesPending || receipt.journalCheckpointPending));
  if (!snapshot || !ownsSettingsConnection(snapshot.owner)) return null;
  if (!pending.length && !error) return null;
  return <section aria-label={t('settings.agentDeletion.progressTitle')}>
    {error && <p role="alert">{t('settings.agentDeletion.loadFailure', { error })}</p>}
    {pending.map(receipt => <div key={receipt.deletionId}>
      <p>{t('settings.agentDeletion.deleted', { id: receipt.agentId })}</p>
      {receipt.cleanupPending && <p>{t('settings.agentDeletion.cleanupPending')}{receipt.cleanupError}</p>}
      {receipt.referencesPending && <p>{t('settings.agentDeletion.referencesPending')}{receipt.referenceError}</p>}
      {receipt.journalCheckpointPending && <p>{t('settings.agentDeletion.journalPending')}{receipt.journalError}</p>}
      <button type="button" disabled={!!retrying} onClick={() => { void retry(receipt); }}>{t('settings.agentDeletion.retry')}</button>
    </div>)}
  </section>;
}
