import React, { useState, useEffect, useRef } from 'react';
import { useSettingsStore } from '../../store';
import { t, lookupModelMeta, CONTEXT_PRESETS, OUTPUT_PRESETS } from '../../helpers';
import { lingxiFetchJson } from '../../api';
import { ComboInput } from '../../widgets/ComboInput';
import {
  type UnifiedModelKind,
  type Modality,
  MODALITY_ORDER,
  KIND_DEFAULT_INPUTS,
  KIND_DEFAULT_OUTPUTS,
  readModalityList,
  inputsFromLegacyFlags,
} from './unified-models';
import {
  structuredOutputSupportState,
  nativeWebSearchSupportState,
} from './capability-contracts';
import styles from '../../Settings.module.css';

function finiteNumber(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

function firstNumber(meta: Record<string, unknown>, keys: string[]): number | undefined {
  for (const key of keys) {
    const value = finiteNumber(meta[key]);
    if (value !== undefined) return value;
  }
  return undefined;
}

const MODALITY_LABEL_KEYS: Record<Modality, string> = {
  text: 'settings.api.modality.text',
  image: 'settings.api.modality.image',
  video: 'settings.api.modality.video',
  audio: 'settings.api.modality.audio',
};

/** 按 model.api 推导默认 toolUse 契约（不得无条件按 OpenAI 构造）。 */
function defaultToolUseContract(api?: string | null): { supportsTools: true; dialect: string; toolResultFormat: string } | null {
  if (api === 'anthropic-messages') {
    return { supportsTools: true, dialect: 'anthropic', toolResultFormat: 'content_block' };
  }
  if (api === 'google-generative-ai') {
    return { supportsTools: true, dialect: 'gemini', toolResultFormat: 'part' };
  }
  if (api === 'openai-completions' || api === 'openai-responses' || api === 'openai-codex-responses') {
    return { supportsTools: true, dialect: 'openai', toolResultFormat: 'message' };
  }
  // 未知协议不 fabricate 具体 dialect，交由服务端按 provider 声明推导。
  return null;
}

function ModalityChipGroup({ labelKey, value, onChange }: {
  labelKey: string;
  value: Modality[];
  onChange: (next: Modality[]) => void;
}) {
  return (
    <div className={styles['pv-model-edit-field']}>
      <div className={styles['pv-modality-chip-row']} role="group" aria-label={t(labelKey)}>
        {MODALITY_ORDER.map(modality => {
          const active = value.includes(modality);
          const label = t(MODALITY_LABEL_KEYS[modality]);
          return (
            <button
              key={modality}
              type="button"
              className={`${styles['pv-modality-chip']}${active ? ' ' + styles['active'] : ''}`}
              aria-pressed={active}
              title={label}
              onClick={() => {
                onChange(active ? value.filter(m => m !== modality) : [...value, modality]);
              }}
            >
              {label}
            </button>
          );
        })}
      </div>
    </div>
  );
}

function CapabilityChip({ labelKey, tooltip, on, disabled, onChange }: {
  labelKey: string;
  tooltip?: string;
  on: boolean;
  disabled?: boolean;
  onChange: (next: boolean) => void;
}) {
  const label = t(labelKey);
  return (
    <button
      type="button"
      className={`${styles['pv-modality-chip']}${on ? ' ' + styles['active'] : ''}`}
      aria-pressed={on}
      aria-disabled={disabled || undefined}
      disabled={disabled}
      title={tooltip || label}
      onClick={() => { if (!disabled) onChange(!on); }}
    >
      {label}
    </button>
  );
}

export function ModelEditPanel({
  kind,
  providerId,
  runtimeProviderId,
  modelId,
  modelMeta,
  summaryApi,
  summaryBaseUrl,
  anchorEl,
  onClose,
  onRefresh,
}: {
  kind: UnifiedModelKind;
  providerId: string;
  runtimeProviderId: string;
  modelId: string;
  modelMeta?: Record<string, unknown>;
  summaryApi?: string | null;
  summaryBaseUrl?: string | null;
  anchorEl: HTMLElement | null;
  onClose: () => void;
  onRefresh?: () => Promise<void>;
}) {
  const showToast = useSettingsStore(s => s.showToast);
  const isChat = kind === 'chat';
  const knownMeta: Record<string, any> = isChat ? ((lookupModelMeta(modelId, providerId) as Record<string, any>) || {}) : {};
  const userMeta: Record<string, unknown> = modelMeta || {};
  const meta: Record<string, any> = { ...knownMeta, ...userMeta };
  // Resolve aliases inside each ownership layer before falling through to the
  // known catalog. Otherwise known `context`/`maxOutput` can mask a persisted
  // user `contextWindow`/`maxTokens` value after the objects are merged.
  const initialContext = firstNumber(userMeta, ['context', 'contextWindow'])
    ?? firstNumber(knownMeta, ['context', 'contextWindow']);
  const initialMaxOutput = firstNumber(userMeta, ['maxOutput', 'maxTokens', 'maxOutputTokens'])
    ?? firstNumber(knownMeta, ['maxOutput', 'maxTokens', 'maxOutputTokens']);

  // 自动检测初始化：用户已保存 > discovered/known metadata > kind 默认值。
  const initialInputs = readModalityList(userMeta.inputs)
    ?? readModalityList(knownMeta.inputs)
    ?? inputsFromLegacyFlags(meta)
    ?? KIND_DEFAULT_INPUTS[kind];
  const initialOutputs = readModalityList(userMeta.outputs)
    ?? readModalityList(knownMeta.outputs)
    ?? KIND_DEFAULT_OUTPUTS[kind];

  const [displayName, setDisplayName] = useState(
    String(meta.displayName || meta.name || modelMeta?.displayName || ''),
  );
  const [ctxVal, setCtxVal] = useState(String(initialContext ?? ''));
  const [outVal, setOutVal] = useState(String(initialMaxOutput ?? ''));
  // 「最大输出」口径：''=自动（按线协议家族推导）、'true'=输出长度包含思维链、
  // 'false'=输出长度仅指最终回答。只反映用户覆盖，known 目录声明不回填。
  const [outputComp, setOutputComp] = useState<string>(
    userMeta.outputIncludesThinking === true ? 'true'
      : userMeta.outputIncludesThinking === false ? 'false' : '',
  );
  const [inputs, setInputs] = useState<Modality[]>(initialInputs);
  const [outputs, setOutputs] = useState<Modality[]>(initialOutputs);
  const [reasoning, setReasoning] = useState<boolean>(meta.reasoning === true);
  // structuredOutput 没有可靠目录检测源时默认 OFF，不因 provider 大概率支持而自动开启
  const [web, setWeb] = useState<boolean>(isChat && meta.web === true);
  const [structuredOutput, setStructuredOutput] = useState<boolean>(isChat && meta.structuredOutput === true);
  // toolUse 契约：只读 supportsTools 布尔；保存时 clone 原契约只改 supportsTools
  const initialToolUse = meta.toolUse && typeof meta.toolUse === 'object'
    ? meta.toolUse.supportsTools === true
    : false;
  const [toolUse, setToolUse] = useState<boolean>(initialToolUse);
  // 模型类型标签：嵌入/重排只是用户打的标记，不代表模型真的支持；
  // 打了标签的模型进入知识库笔记本设置的对应下拉，协议方言由供应商自动推断
  const userOperations: string[] = Array.isArray(userMeta.operations) ? (userMeta.operations as string[]) : [];
  const initialOperationType = userOperations.includes('embedding')
    ? 'embedding'
    : userOperations.includes('rerank') ? 'rerank' : '';
  const [operationType, setOperationType] = useState(initialOperationType);
  const [dimensionsVal, setDimensionsVal] = useState(String(finiteNumber(userMeta.dimensions) ?? ''));
  const [dirty, setDirty] = useState<Record<string, boolean>>({});
  const markDirty = (field: string) => setDirty(prev => ({ ...prev, [field]: true }));
  const panelRef = useRef<HTMLDivElement>(null);
  const [style, setStyle] = useState<React.CSSProperties>({});

  // 协议支持状态（与 core/provider-compat 契约镜像）：已知不支持 → OFF + disabled
  const modelApi = typeof meta.api === 'string' && meta.api ? meta.api : summaryApi || null;
  const contractIdentity = { providerId: runtimeProviderId || providerId, api: modelApi, baseUrl: summaryBaseUrl };
  const structuredSupport = structuredOutputSupportState(contractIdentity);
  const webSupport = nativeWebSearchSupportState(contractIdentity);

  useEffect(() => {
    setStyle({
      position: 'fixed',
      left: '50%',
      top: '50%',
      transform: 'translate(-50%, -50%)',
      zIndex: 9999,
      width: 360,
    });
  }, [anchorEl]);

  const save = async () => {
    const name = displayName.trim();
    if (kind === 'chat') {
      if (readModalityList(inputs) === null || readModalityList(outputs) === null) {
        showToast(t('settings.api.modalityRequired'), 'error');
        return;
      }
      const entry: Record<string, any> = {};
      if (name) entry.name = name;
      const ctx = ctxVal.trim();
      const maxOut = outVal.trim();
      if (ctx) entry.context = parseInt(ctx);
      if (maxOut) entry.maxOutput = parseInt(maxOut);
      // null（''=自动）显式清除覆盖，回到按线协议家族的自动推导。
      if (dirty.outputComp) entry.outputIncludesThinking = outputComp === '' ? null : outputComp === 'true';
      if (dirty.inputs) entry.inputs = MODALITY_ORDER.filter(m => inputs.includes(m));
      if (dirty.outputs) entry.outputs = MODALITY_ORDER.filter(m => outputs.includes(m));
      if (dirty.reasoning) entry.reasoning = reasoning;
      if (dirty.web) entry.web = web;
      if (dirty.structuredOutput) entry.structuredOutput = structuredOutput;
      if (dirty.toolUse) {
        const existing = meta.toolUse && typeof meta.toolUse === 'object'
          ? { ...meta.toolUse } as Record<string, unknown>
          : null;
        const derived = toolUse ? defaultToolUseContract(modelApi) : null;
        // 已有 contract 时 clone 后只改 supportsTools；否则按 model.api 推导；
        // 无法推导的未知协议不 fabricate（关闭时保留 dialect 字段的合法默认）。
        entry.toolUse = toolUse
          ? { ...(existing || derived), supportsTools: true }
          : {
            ...(existing || { dialect: 'none', toolResultFormat: 'message' }),
            supportsTools: false,
          };
        // 开启但既无既有 contract 也无法从 api 推导：不要写入一个错误的工具契约
        if (toolUse && !existing && !derived) delete entry.toolUse;
      }
      if (dirty.operationType) entry.operations = operationType ? [operationType] : [];
      if (dirty.dimensions) {
        const parsed = parseInt(dimensionsVal.trim());
        // null 显式清除维度记录；非法输入原样拒绝由数字输入框约束
        entry.dimensions = dimensionsVal.trim() && Number.isSafeInteger(parsed) && parsed > 0 ? parsed : null;
      }
      try {
        await lingxiFetchJson(`/api/providers/${encodeURIComponent(providerId)}/models/${encodeURIComponent(modelId)}`, {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(entry),
        });
        await onRefresh?.();
        showToast(t('settings.saved'), 'success');
        onClose();
      } catch (err: any) {
        showToast(t('settings.saveFailed') + ': ' + err.message, 'error');
      }
      return;
    }

    // 媒体/语音识别：只允许编辑 displayName / inputs / outputs
    const patch: Record<string, unknown> = {};
    if (name) patch.displayName = name;
    if (dirty.inputs) patch.inputs = MODALITY_ORDER.filter(m => inputs.includes(m));
    if (dirty.outputs) patch.outputs = MODALITY_ORDER.filter(m => outputs.includes(m));
    if (Object.keys(patch).length === 0) {
      onClose();
      return;
    }
    const base = kind === 'speech'
      ? `/api/speech-recognition/providers/${encodeURIComponent(runtimeProviderId)}`
      : `/api/media/${kind === 'video' ? 'video' : 'image'}/providers/${encodeURIComponent(runtimeProviderId)}`;
    try {
      await lingxiFetchJson(`${base}/models/${encodeURIComponent(modelId)}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(patch),
      });
      await onRefresh?.();
      showToast(t('settings.saved'), 'success');
      onClose();
    } catch (err: any) {
      showToast(t('settings.saveFailed') + ': ' + err.message, 'error');
    }
  };

  return (
    <>
    <div className={styles['pv-model-edit-overlay']} onClick={onClose} />
    <div ref={panelRef} className={styles['pv-model-edit-card']} style={style} data-model-edit-kind={kind}>
      <div className={styles['pv-model-edit-field']}>
        <label className={styles['pv-model-edit-label']}>ID</label>
        <span className={styles['pv-model-edit-id']}>{modelId}</span>
      </div>
      <div className={styles['pv-model-edit-field']}>
        <label className={styles['pv-model-edit-label']}>{t('settings.api.displayName')}</label>
        <input
          className={styles['settings-input']}
          type="text"
          value={displayName}
          onChange={(e) => setDisplayName(e.target.value)}
          placeholder={modelId}
        />
      </div>
      {isChat && (
        <>
          <div className={styles['pv-model-edit-section-title']}>{t('settings.api.contextConfig')}</div>
          <div className={styles['pv-model-edit-row']}>
            <div className={styles['pv-model-edit-field']}>
              <label className={styles['pv-model-edit-label']}>{t('settings.api.contextLength')}</label>
              <ComboInput presets={CONTEXT_PRESETS} value={ctxVal} onChange={setCtxVal} placeholder="131072" />
            </div>
            <div className={styles['pv-model-edit-field']}>
              <label className={styles['pv-model-edit-label']}>{t('settings.api.maxOutput')}</label>
              <ComboInput presets={OUTPUT_PRESETS} value={outVal} onChange={setOutVal} placeholder="65536" />
            </div>
          </div>
          <div className={styles['pv-model-edit-field']}>
            <label className={styles['pv-model-edit-label']}>{t('settings.api.outputComposition')}</label>
            <select
              className={styles['settings-input']}
              value={outputComp}
              onChange={(e) => { setOutputComp(e.target.value); markDirty('outputComp'); }}
            >
              <option value="">{t('settings.api.outputCompositionAuto')}</option>
              <option value="false">{t('settings.api.outputCompositionSeparate')}</option>
              <option value="true">{t('settings.api.outputCompositionIncluded')}</option>
            </select>
            <span className={styles['pv-model-edit-hint']}>{t('settings.api.outputCompositionHint')}</span>
          </div>
        </>
      )}
      <div className={styles['pv-model-edit-section-title']}>{t('settings.api.inputModalities')}</div>
      <ModalityChipGroup labelKey="settings.api.inputModalities" value={inputs} onChange={(next) => { setInputs(next); markDirty('inputs'); }} />
      <div className={styles['pv-model-edit-section-title']}>{t('settings.api.outputModalities')}</div>
      <ModalityChipGroup labelKey="settings.api.outputModalities" value={outputs} onChange={(next) => { setOutputs(next); markDirty('outputs'); }} />
      {isChat && (
        <>
          <div className={styles['pv-model-edit-section-title']}>{t('settings.api.modelCapabilities')}</div>
          <div className={styles['pv-modality-chip-row']} role="group" aria-label={t('settings.api.modelCapabilities')}>
            <CapabilityChip
              labelKey="settings.api.reasoning"
              on={reasoning}
              onChange={(value) => { setReasoning(value); markDirty('reasoning'); }}
            />
            <CapabilityChip
              labelKey="settings.api.toolUse"
              on={toolUse}
              onChange={(value) => { setToolUse(value); markDirty('toolUse'); }}
            />
            <CapabilityChip
              labelKey="settings.api.capability.web"
              tooltip={webSupport === 'supported' ? t('settings.api.webTooltip') : t('settings.api.webUnsupported')}
              on={webSupport === 'supported' && web}
              disabled={webSupport !== 'supported'}
              onChange={(value) => { setWeb(value); markDirty('web'); }}
            />
            <CapabilityChip
              labelKey="settings.api.capability.structuredOutput"
              tooltip={structuredSupport === 'supported' ? t('settings.api.structuredOutputTooltip') : t('settings.api.structuredOutputUnsupported')}
              on={structuredSupport === 'supported' && structuredOutput}
              disabled={structuredSupport !== 'supported'}
              onChange={(value) => { setStructuredOutput(value); markDirty('structuredOutput'); }}
            />
          </div>
          <div className={styles['pv-model-edit-section-title']}>{t('settings.api.modelOperationType')}</div>
          <div className={styles['pv-model-edit-field']}>
            <select
              className={styles['settings-input']}
              value={operationType}
              onChange={(e) => { setOperationType(e.target.value); markDirty('operationType'); }}
            >
              <option value="">{t('settings.api.modelOperationNone')}</option>
              <option value="embedding">{t('settings.api.modelOperationEmbedding')}</option>
              <option value="rerank">{t('settings.api.modelOperationRerank')}</option>
            </select>
            <span className={styles['pv-model-edit-hint']}>{t('settings.api.modelOperationHint')}</span>
          </div>
          {operationType === 'embedding' && (
            <div className={styles['pv-model-edit-field']}>
              <label className={styles['pv-model-edit-label']}>{t('settings.api.embeddingDimensions')}</label>
              <input
                className={styles['settings-input']}
                type="number"
                min="1"
                value={dimensionsVal}
                onChange={(e) => { setDimensionsVal(e.target.value); markDirty('dimensions'); }}
                placeholder="1024"
              />
              <span className={styles['pv-model-edit-hint']}>{t('settings.api.embeddingDimensionsHint')}</span>
            </div>
          )}
        </>
      )}
      <div className={styles['pv-model-edit-actions']}>
        <button type="button" className={styles['pv-add-form-btn']} onClick={onClose}>{t('settings.api.cancel')}</button>
        <button type="button" className={`${styles['pv-add-form-btn']} ${styles['primary']}`} onClick={save}>{t('settings.api.save')}</button>
      </div>
    </div>
    </>
  );
}
