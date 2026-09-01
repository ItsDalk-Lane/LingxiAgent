/**
 * AgentTenets — 设置页「用户原则」区块
 *
 * pending 提案（批准/拒绝）+ active 原则（优先级排序、删除）+ 手动添加。
 * 设置窗口是独立 JS 上下文，不走主窗口 tenet-store，本地态 + REST。
 */

import { useCallback, useEffect, useState } from 'react';
import { t } from '../../helpers';
import { lingxiFetch } from '../../api';
import styles from '../../Settings.module.css';

type TenetPriority = 'critical' | 'high' | 'medium' | 'low';

interface Tenet {
  id: string;
  content: string;
  priority: TenetPriority;
  status: 'pending' | 'active' | 'rejected';
  source: 'model_proposed' | 'user_direct';
  createdAt: string;
}

const PRIORITY_KEYS: TenetPriority[] = ['critical', 'high', 'medium', 'low'];

function priorityLabel(priority: TenetPriority): string {
  const key = `settings.tenetsPriority${priority[0].toUpperCase()}${priority.slice(1)}`;
  const label = t(key);
  return label === key ? priority : label;
}

async function tenetsRequest(path: string, init: RequestInit = {}): Promise<any> {
  const response = await lingxiFetch(path, {
    ...init,
    headers: { 'content-type': 'application/json', ...(init.headers || {}) },
  });
  let data: any = null;
  try {
    data = await response.json();
  } catch {
    data = null;
  }
  if (!response.ok) {
    throw new Error(data?.error || `${response.status} ${response.statusText}`);
  }
  return data;
}

export function AgentTenets({ agentId }: { agentId: string }) {
  const [tenets, setTenets] = useState<Tenet[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [content, setContent] = useState('');
  const [priority, setPriority] = useState<TenetPriority>('medium');
  const [adding, setAdding] = useState(false);

  const refresh = useCallback(async (signal?: AbortSignal) => {
    try {
      const data = await tenetsRequest(`/api/agents/${encodeURIComponent(agentId)}/tenets`, { signal });
      setTenets(Array.isArray(data?.tenets) ? data.tenets : []);
      setError(null);
    } catch (err: any) {
      if (signal?.aborted) return;
      setError(err?.message || String(err));
    }
  }, [agentId]);

  useEffect(() => {
    const controller = new AbortController();
    setTenets(null);
    void refresh(controller.signal);
    return () => controller.abort();
  }, [refresh]);

  const pending = (tenets || []).filter((n) => n.status === 'pending');
  const active = (tenets || []).filter((n) => n.status === 'active');

  const decide = async (tenetId: string, approve: boolean) => {
    try {
      await tenetsRequest(
        `/api/agents/${encodeURIComponent(agentId)}/tenets/${encodeURIComponent(tenetId)}/decide`,
        { method: 'POST', body: JSON.stringify({ approve }) },
      );
      await refresh();
    } catch (err: any) {
      setError(err?.message || String(err));
    }
  };

  const remove = async (tenetId: string) => {
    try {
      await tenetsRequest(
        `/api/agents/${encodeURIComponent(agentId)}/tenets/${encodeURIComponent(tenetId)}`,
        { method: 'DELETE' },
      );
      await refresh();
    } catch (err: any) {
      setError(err?.message || String(err));
    }
  };

  const add = async () => {
    const value = content.trim();
    if (!value || adding) return;
    setAdding(true);
    try {
      await tenetsRequest(`/api/agents/${encodeURIComponent(agentId)}/tenets`, {
        method: 'POST',
        body: JSON.stringify({ content: value, priority }),
      });
      setContent('');
      await refresh();
    } catch (err: any) {
      setError(err?.message || String(err));
    } finally {
      setAdding(false);
    }
  };

  return (
    <div className={styles['settings-subsection']}>
      <div className={styles['settings-subsection-header']}>
        <h3 className={styles['settings-subsection-title']}>{t('settings.tenetsTitle')}</h3>
        <span className={styles['settings-subsection-hint']}>{t('settings.tenetsHint')}</span>
      </div>

      {error && <p className={`${styles['settings-inline-note']} ${styles['tenets-error']}`}>{error}</p>}
      {tenets === null && !error && <p className={styles['settings-inline-note']}>{t('settings.tenetsLoading')}</p>}
      {tenets !== null && tenets.length === 0 && (
        <p className={styles['settings-inline-note']}>{t('settings.tenetsEmpty')}</p>
      )}

      {pending.length > 0 && (
        <div className={styles['pin-list']}>
          {pending.map((tenet) => (
            <div key={tenet.id} className={`${styles['pin-item']} ${styles['tenets-row']}`}>
              <span className={styles['tenets-tag']}>
                {priorityLabel(tenet.priority)}
              </span>
              <span className={styles['tenets-content']}>{tenet.content}</span>
              <span className={styles['tenets-actions']}>
                <button className={styles['memory-action-btn']} onClick={() => { void decide(tenet.id, true); }}>
                  {t('settings.tenetsApprove')}
                </button>
                <button className={styles['memory-action-btn']} onClick={() => { void decide(tenet.id, false); }}>
                  {t('settings.tenetsReject')}
                </button>
              </span>
            </div>
          ))}
        </div>
      )}

      {active.length > 0 && (
        <div className={styles['pin-list']}>
          {active.map((tenet) => (
            <div key={tenet.id} className={`${styles['pin-item']} ${styles['tenets-row']}`}>
              <span className={styles['tenets-tag']}>
                {priorityLabel(tenet.priority)}
              </span>
              <span className={styles['tenets-content']}>{tenet.content}</span>
              <button className={`${styles['memory-action-btn']} ${styles['danger']}`} onClick={() => { void remove(tenet.id); }}>
                {t('settings.tenetsDelete')}
              </button>
            </div>
          ))}
        </div>
      )}

      <div className={styles['pin-add-row']}>
        <input
          className={`${styles['settings-input']} ${styles['pin-add-input']}`}
          type="text"
          value={content}
          maxLength={300}
          onChange={(e) => setContent(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); void add(); } }}
          placeholder={t('settings.tenetsAddPlaceholder')}
        />
        <select
          className={styles['settings-input']}
          value={priority}
          onChange={(e) => setPriority(e.target.value as TenetPriority)}
        >
          {PRIORITY_KEYS.map((p) => (
            <option key={p} value={p}>{priorityLabel(p)}</option>
          ))}
        </select>
        <button
          className={styles['pin-add-btn']}
          disabled={adding || !content.trim()}
          onClick={() => { void add(); }}
        >
          +
        </button>
      </div>
    </div>
  );
}
