/**
 * TenetApprovalBanner — 用户原则提案审批横幅
 *
 * 挂在输入区上方（与 SessionConfirmationPrompt 同区）：当前 agent 存在 pending
 * 提案时出现，逐条展示「批准 / 拒绝」。提案持久化在服务端 tenets.json，
 * 永不超时作废；数据刷新由 app-event-actions 在 tenets-changed / agent-switched
 * 事件里驱动，本组件纯渲染（不在挂载时发请求）。
 */

import { useMemo, useState } from 'react';
import { useStore } from '../../stores';
import { useTenetStore, type Tenet } from '../../stores/tenet-store';
import { lingxiFetch } from '../../hooks/use-hana-fetch';
import { useI18n } from '../../hooks/use-i18n';
import styles from './InputArea.module.css';

const PRIORITY_LABEL_KEYS: Record<string, string> = {
  critical: 'settings.memory.tenetsPriorityCritical',
  high: 'settings.memory.tenetsPriorityHigh',
  medium: 'settings.memory.tenetsPriorityMedium',
  low: 'settings.memory.tenetsPriorityLow',
};

async function decideTenet(agentId: string, tenetId: string, approve: boolean): Promise<boolean> {
  try {
    const response = await lingxiFetch(
      `/api/agents/${encodeURIComponent(agentId)}/tenets/${encodeURIComponent(tenetId)}/decide`,
      {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        throwOnHttpError: false,
        body: JSON.stringify({ approve }),
      },
    );
    return response.ok;
  } catch {
    return false;
  }
}

function TenetRow({ agentId, tenet, onDecided }: { agentId: string; tenet: Tenet; onDecided: (tenet: Tenet) => void }) {
  const { t } = useI18n();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);

  const handle = async (approve: boolean) => {
    if (busy) return;
    setBusy(true);
    const ok = await decideTenet(agentId, tenet.id, approve);
    setBusy(false);
    if (ok) {
      onDecided(tenet);
    } else {
      setError(true);
    }
  };

  const priorityLabel = t(PRIORITY_LABEL_KEYS[tenet.priority] || 'settings.memory.tenetsPriorityMedium');

  return (
    <div className={styles['tenet-row']}>
      <div className={styles['tenet-content']}>
        <span className={styles['tenet-priority-tag']}>{priorityLabel}</span>
        <span>{tenet.content}</span>
      </div>
      <div className={styles['tenet-actions']}>
        {error && <span className={styles['tenet-error-text']}>{t('settings.memory.tenetsDecideFailed')}</span>}
        <button
          type="button"
          className={`${styles['tenet-action-button']} ${styles['tenet-approve-button']}`}
          disabled={busy}
          onClick={() => { void handle(true); }}
        >
          {t('settings.memory.tenetsApprove')}
        </button>
        <button
          type="button"
          className={`${styles['tenet-action-button']} ${styles['tenet-reject-button']}`}
          disabled={busy}
          onClick={() => { void handle(false); }}
        >
          {t('settings.memory.tenetsReject')}
        </button>
      </div>
    </div>
  );
}

export function TenetApprovalBanner() {
  const { t } = useI18n();
  const currentAgentId = useStore(s => s.currentAgentId);
  // selector 必须返回稳定引用（zustand v5 默认 Object.is 比较），
  // 派生数组放 useMemo，否则每次 store 通知都触发重渲染循环。
  const entry = useTenetStore(s => (currentAgentId ? s.byAgent[currentAgentId] : undefined));
  const pending = useMemo(
    () => (entry?.loaded ? entry.tenets.filter(ten => ten.status === 'pending') : []),
    [entry],
  );

  // 本组件纯渲染：数据由 app-event-actions 在 tenets-changed / agent-switched
  // 事件里刷新（不在挂载时拉取，避免每个 InputArea 渲染都发请求）。
  if (!currentAgentId || pending.length === 0) return null;

  return (
    <div className={styles['tenet-banner']} data-testid="tenet-approval-banner">
      <div className={styles['tenet-banner-header']}>{t('settings.memory.tenetsBannerTitle', { count: pending.length })}</div>
      {pending.map((tenet) => (
        <TenetRow
          key={tenet.id}
          agentId={currentAgentId}
          tenet={tenet}
          onDecided={() => { void useTenetStore.getState().refresh(currentAgentId); }}
        />
      ))}
    </div>
  );
}
