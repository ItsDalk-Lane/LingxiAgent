import { useEffect, useState } from 'react';
import { ConfirmDialog } from '../../ui';
import { lingxiFetch } from '../../hooks/use-hana-fetch';
import {
  updateKnowledgeNotebookSettings,
  type KnowledgeModelRefDto,
  type KnowledgeNotebookDto,
} from './knowledge-api';
import styles from './KnowledgePage.module.css';

const tr = (key: string, vars?: Record<string, string | number>) => window.t?.(key, vars) ?? key;

const RETRIEVAL_TOP_K_MIN = 1;
const RETRIEVAL_TOP_K_MAX = 1000;
const VECTOR_RETENTION_DAYS_MIN = 1;
const VECTOR_RETENTION_DAYS_MAX = 3650;

interface OperationModel extends KnowledgeModelRefDto {
  displayName?: string;
  name?: string;
  operations?: string[];
}

interface ModelPreferences {
  operation_models?: OperationModel[];
}

function refValue(ref: KnowledgeModelRefDto | null): string {
  return ref?.provider && ref.id ? `${ref.provider} ${ref.id}` : '';
}

function modelLabel(ref: KnowledgeModelRefDto, models: OperationModel[]): string {
  const found = models.find(model => model.id === ref.id && model.provider === ref.provider);
  return found
    ? `${found.displayName || found.name || found.id} · ${found.provider}`
    : (ref.provider ? `${ref.provider}/${ref.id}` : ref.id);
}

function validateNumber(value: string, min: number, max: number): boolean {
  if (value.trim() === '') return true;
  const num = Number(value);
  return Number.isSafeInteger(num) && num >= min && num <= max;
}

interface ModelSelectRowProps {
  operation: 'embedding' | 'rerank';
  title: string;
  hint: string;
  value: string;
  original: KnowledgeModelRefDto | null;
  models: OperationModel[];
  onChange: (value: string) => void;
}

function ModelSelectRow({ operation, title, hint, value, original, models, onChange }: ModelSelectRowProps) {
  const available = models.filter(model => model.operations?.includes(operation));
  const selectedAvailable = available.some(model => refValue(model) === value);
  return (
    <label className={styles.settingsRow}>
      <span className={styles.settingsLabel}>{title}</span>
      <select
        className={styles.settingsInput}
        aria-label={title}
        value={value}
        onChange={event => onChange(event.target.value)}
      >
        <option value="">{tr('knowledge.settingsGlobalNotConfigured')}</option>
        {value && !selectedAvailable && original && (
          <option value={value} disabled>
            {tr('settings.api.knowledgeOperationUnavailable')}: {modelLabel(original, models)}
          </option>
        )}
        {available.map(model => (
          <option key={refValue(model)} value={refValue(model)}>
            {model.displayName || model.name || model.id} · {model.provider}
          </option>
        ))}
      </select>
      <span className={styles.settingsHint}>{hint}</span>
    </label>
  );
}

export interface NotebookSettingsDialogProps {
  notebook: KnowledgeNotebookDto;
  onClose: () => void;
  onSaved: () => void;
}

/**
 * 笔记本级配置弹窗（v8 语义）：嵌入/重排模型仅在笔记本级配置（无全局继承）；
 * 分块尺寸按嵌入模型上下文 ×80% 自动计算（只读展示生效值，遗留显式值仅作
 * 覆盖）；检索数量支持"无上限（默认）"与"最大召回数"两种模式。
 */
export function NotebookSettingsDialog({ notebook, onClose, onSaved }: NotebookSettingsDialogProps) {
  const [prefs, setPrefs] = useState<ModelPreferences | null>(null);
  const [prefsFailed, setPrefsFailed] = useState(false);
  const [embeddingValue, setEmbeddingValue] = useState(() => refValue(notebook.config.embeddingModelRef));
  const [rerankValue, setRerankValue] = useState(() => refValue(notebook.config.rerankModelRef));
  // 检索模式：null（无上限，默认）→ unlimited；正整数 → max。
  const [topKUnlimited, setTopKUnlimited] = useState(notebook.config.retrievalTopK == null);
  const [topK, setTopK] = useState(
    notebook.config.retrievalTopK == null ? '' : String(notebook.config.retrievalTopK),
  );
  // 向量保留：null（永久保留，默认）→ keep；正整数 → N 天未使用自动清理。
  const [retentionKeepForever, setRetentionKeepForever] = useState(notebook.config.vectorRetentionDays == null);
  const [retentionDays, setRetentionDays] = useState(
    notebook.config.vectorRetentionDays == null ? '' : String(notebook.config.vectorRetentionDays),
  );
  const [saving, setSaving] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    lingxiFetch('/api/preferences/models')
      .then(async response => {
        const body = await response.json() as ModelPreferences;
        if (active) setPrefs(body);
      })
      .catch(() => {
        if (active) setPrefsFailed(true);
      });
    return () => { active = false; };
  }, []);

  const operationModels = Array.isArray(prefs?.operation_models) ? prefs.operation_models : [];

  // 未改动的"当前不可用"引用按原样回传（不静默清成 NULL）；改动后必须是可选列表里的完整引用。
  const resolveRef = (
    value: string,
    original: KnowledgeModelRefDto | null,
  ): KnowledgeModelRefDto | null => {
    if (!value) return null;
    if (original && refValue(original) === value) return { id: original.id, provider: original.provider };
    const found = operationModels.find(model => refValue(model) === value);
    return found ? { id: found.id, provider: found.provider } : null;
  };

  const handleSave = async () => {
    if (!topKUnlimited && !validateNumber(topK, RETRIEVAL_TOP_K_MIN, RETRIEVAL_TOP_K_MAX)) {
      setFormError(tr('knowledge.settingsInvalidNumber', { min: RETRIEVAL_TOP_K_MIN, max: RETRIEVAL_TOP_K_MAX }));
      return;
    }
    if (!retentionKeepForever && !validateNumber(retentionDays, VECTOR_RETENTION_DAYS_MIN, VECTOR_RETENTION_DAYS_MAX)) {
      setFormError(tr('knowledge.settingsInvalidNumber', { min: VECTOR_RETENTION_DAYS_MIN, max: VECTOR_RETENTION_DAYS_MAX }));
      return;
    }
    setSaving(true);
    setFormError(null);
    try {
      await updateKnowledgeNotebookSettings(notebook.id, {
        embeddingModelRef: resolveRef(embeddingValue, notebook.config.embeddingModelRef),
        rerankModelRef: resolveRef(rerankValue, notebook.config.rerankModelRef),
        retrievalTopK: topKUnlimited || topK.trim() === '' ? null : Number(topK),
        vectorRetentionDays: retentionKeepForever || retentionDays.trim() === '' ? null : Number(retentionDays),
      });
      onSaved();
    } catch (error) {
      setFormError(error instanceof Error ? error.message : tr('knowledge.saveFailed'));
    } finally {
      setSaving(false);
    }
  };

  const effectiveChunkTarget = notebook.chunkTargetCharsEffective
    ?? (notebook.config.chunkTargetChars != null ? notebook.config.chunkTargetChars : null);

  return (
    <ConfirmDialog
      open
      scope="window"
      title={`${tr('knowledge.notebookSettings')} · ${notebook.name}`}
      confirmLabel={tr('knowledge.save')}
      cancelLabel={tr('knowledge.cancel')}
      onConfirm={() => void handleSave()}
      onCancel={onClose}
      busy={saving}
      closeOnEsc
    >
      <div className={styles.settingsForm}>
        {prefsFailed && <p className={styles.settingsError}>{tr('knowledge.loadFailed')}</p>}
        <ModelSelectRow
          operation="embedding"
          title={tr('knowledge.settingsEmbeddingModel')}
          hint={tr('knowledge.settingsEmbeddingHint')}
          value={embeddingValue}
          original={notebook.config.embeddingModelRef}
          models={operationModels}
          onChange={setEmbeddingValue}
        />
        <ModelSelectRow
          operation="rerank"
          title={tr('knowledge.settingsRerankModel')}
          hint={tr('knowledge.settingsRerankHint')}
          value={rerankValue}
          original={notebook.config.rerankModelRef}
          models={operationModels}
          onChange={setRerankValue}
        />
        <label className={styles.settingsRow}>
          <span className={styles.settingsLabel}>{tr('knowledge.settingsChunkTargetChars')}</span>
          <output className={styles.settingsInput} aria-label={tr('knowledge.settingsChunkTargetChars')}>
            {effectiveChunkTarget ?? tr('knowledge.settingsChunkAutoFallback')}
          </output>
          <span className={styles.settingsHint}>{tr('knowledge.settingsChunkHint')}</span>
        </label>
        <div className={styles.settingsRow}>
          <span className={styles.settingsLabel}>{tr('knowledge.settingsRetrievalTopK')}</span>
          <div className={styles.settingsTopKControls}>
            <label className={styles.settingsRadio}>
              <input
                type="radio"
                name={`topk-mode-${notebook.id}`}
                checked={topKUnlimited}
                onChange={() => setTopKUnlimited(true)}
              />
              <span>{tr('knowledge.settingsTopKUnlimited')}</span>
            </label>
            <label className={styles.settingsRadio}>
              <input
                type="radio"
                name={`topk-mode-${notebook.id}`}
                checked={!topKUnlimited}
                onChange={() => setTopKUnlimited(false)}
              />
              <span>{tr('knowledge.settingsTopKMaxRecall')}</span>
            </label>
            {!topKUnlimited && (
              <input
                className={styles.settingsInput}
                type="number"
                min={RETRIEVAL_TOP_K_MIN}
                max={RETRIEVAL_TOP_K_MAX}
                step={1}
                value={topK}
                placeholder="12"
                aria-label={tr('knowledge.settingsTopKMaxRecall')}
                onChange={event => setTopK(event.target.value)}
              />
            )}
          </div>
          <span className={styles.settingsHint}>{tr('knowledge.settingsTopKHint')}</span>
        </div>
        <div className={styles.settingsRow}>
          <span className={styles.settingsLabel}>{tr('knowledge.settingsVectorRetention')}</span>
          <div className={styles.settingsTopKControls}>
            <label className={styles.settingsRadio}>
              <input
                type="radio"
                name={`vector-retention-mode-${notebook.id}`}
                checked={retentionKeepForever}
                onChange={() => setRetentionKeepForever(true)}
              />
              <span>{tr('knowledge.settingsRetentionKeepForever')}</span>
            </label>
            <label className={styles.settingsRadio}>
              <input
                type="radio"
                name={`vector-retention-mode-${notebook.id}`}
                checked={!retentionKeepForever}
                onChange={() => setRetentionKeepForever(false)}
              />
              <span>{tr('knowledge.settingsRetentionDaysMode')}</span>
            </label>
            {!retentionKeepForever && (
              <input
                className={styles.settingsInput}
                type="number"
                min={VECTOR_RETENTION_DAYS_MIN}
                max={VECTOR_RETENTION_DAYS_MAX}
                step={1}
                value={retentionDays}
                placeholder="30"
                aria-label={tr('knowledge.settingsRetentionDaysMode')}
                onChange={event => setRetentionDays(event.target.value)}
              />
            )}
          </div>
          <span className={styles.settingsHint}>{tr('knowledge.settingsVectorRetentionHint')}</span>
        </div>
        {formError && <p className={styles.settingsError} role="alert">{formError}</p>}
      </div>
    </ConfirmDialog>
  );
}
