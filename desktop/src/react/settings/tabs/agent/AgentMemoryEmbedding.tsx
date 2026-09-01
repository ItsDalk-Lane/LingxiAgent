/**
 * AgentMemoryEmbedding — 记忆语义检索的嵌入模型选择器
 *
 * 配置落 agent config.yaml memory.embedding_model（{provider, id}）。
 * 未配置 = 语义检索关闭（search_memory 走标签+FTS 单路并显式留痕）。
 * 数据源与知识库笔记本设置同源：GET /api/preferences/models 的 operation_models。
 */

import { useEffect, useMemo, useState } from 'react';
import { useSettingsStore } from '../../store';
import { t } from '../../helpers';
import { lingxiFetch } from '../../api';
import styles from '../../Settings.module.css';

interface OperationModel {
  id: string;
  provider: string;
  label?: string;
  operations?: string[];
}

function refValue(ref: { provider: string; id: string } | null | undefined): string {
  return ref?.provider && ref?.id ? `${ref.provider}/${ref.id}` : '';
}

async function fetchJson(path: string): Promise<any> {
  const response = await lingxiFetch(path);
  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
  return response.json();
}

export function AgentMemoryEmbedding({ agentId }: { agentId: string }) {
  const showToast = useSettingsStore(s => s.showToast);
  const [models, setModels] = useState<OperationModel[] | null>(null);
  const [current, setCurrent] = useState('');
  const [original, setOriginal] = useState('');
  const [saving, setSaving] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    setLoadError(null);
    void (async () => {
      try {
        const [prefs, config] = await Promise.all([
          fetchJson('/api/preferences/models'),
          fetchJson(`/api/agents/${encodeURIComponent(agentId)}/config`),
        ]);
        if (!active) return;
        const operationModels: OperationModel[] = Array.isArray(prefs?.operation_models)
          ? prefs.operation_models
          : [];
        setModels(operationModels.filter((m) => m?.operations?.includes('embedding')));
        const ref = config?.memory?.embedding_model;
        const value = refValue(ref);
        setCurrent(value);
        setOriginal(value);
      } catch (err: any) {
        if (!active) return;
        setLoadError(err?.message || String(err));
      }
    })();
    return () => { active = false; };
  }, [agentId]);

  const options = useMemo(() => {
    const opts = (models || []).map((m) => ({
      value: `${m.provider}/${m.id}`,
      label: m.label || m.id,
      group: m.provider,
    }));
    // 当前值已不在目录（模型被删）时保留只读项，避免静默改配置
    if (original && !opts.some((o) => o.value === original)) {
      opts.unshift({ value: original, label: original, group: '' });
    }
    return opts;
  }, [models, original]);

  const save = async (value: string) => {
    if (saving) return;
    setSaving(true);
    setCurrent(value);
    try {
      const body = value
        ? { memory: { embedding_model: { provider: value.split('/')[0], id: value.slice(value.indexOf('/') + 1) } } }
        // 显式置空：deepMerge 需要显式 null 才能清除已有引用
        : { memory: { embedding_model: null } };
      const response = await lingxiFetch(`/api/agents/${encodeURIComponent(agentId)}/config`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      const data = await response.json();
      if (!response.ok || data?.error) throw new Error(data?.error || `${response.status}`);
      setOriginal(value);
      showToast(t('settings.memory.embeddingSaved'), 'success');
    } catch (err: any) {
      setCurrent(original);
      showToast(t('settings.saveFailed') + ': ' + (err?.message || String(err)), 'error');
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className={styles['settings-subsection']}>
      <div className={styles['settings-subsection-header']}>
        <h3 className={styles['settings-subsection-title']}>{t('settings.memory.embeddingTitle')}</h3>
        <span className={styles['settings-subsection-hint']}>{t('settings.memory.embeddingHint')}</span>
      </div>
      {loadError && <p className={`${styles['settings-inline-note']} ${styles['tenets-error']}`}>{loadError}</p>}
      {!loadError && models === null && (
        <p className={styles['settings-inline-note']}>{t('settings.memory.embeddingLoading')}</p>
      )}
      {models !== null && models.length === 0 && (
        <p className={styles['settings-inline-note']}>{t('settings.memory.embeddingNoModels')}</p>
      )}
      {models !== null && models.length > 0 && (
        <select
          value={current}
          disabled={saving}
          onChange={(e) => { void save(e.target.value); }}
          className={`${styles['settings-input']} ${styles['tenets-input-full']}`}
        >
          <option value="">{t('settings.memory.embeddingNone')}</option>
          {options.map((o) => (
            <option key={o.value} value={o.value}>
              {o.group ? `${o.group} / ` : ''}{o.label}
            </option>
          ))}
        </select>
      )}
      <p className={`${styles['settings-inline-note']} ${styles['tenets-note']}`}>
        {current ? t('settings.memory.embeddingActiveNote') : t('settings.memory.embeddingOffNote')}
      </p>
    </div>
  );
}
