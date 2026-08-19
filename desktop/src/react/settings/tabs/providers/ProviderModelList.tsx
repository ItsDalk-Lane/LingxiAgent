import React, { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { createPortal } from 'react-dom';
import { useSettingsStore, type ProviderSummary } from '../../store';
import { lingxiFetch, lingxiFetchJson } from '../../api';
import { invalidateConfigCache } from '../../../hooks/use-config';
import { t, formatContext, lookupModelMeta } from '../../helpers';
import { useAnchoredDropdown } from '../../hooks/useAnchoredDropdown';
import type { UseMediaSettingsDataResult, MediaProvider } from '../../hooks/useMediaSettingsData';
import { ModelEditPanel } from './ModelEditPanel';
import { OutputModalityIcons } from './OutputModalityIcons';
import { ProviderMediaDefaultsModal } from './ProviderMediaDefaultsModal';
import { resolveProviderMediaCapabilities, type ResolvedMediaCapability } from './provider-media-capabilities';
import {
  buildUnifiedModelItems,
  chatEntryMediaKind,
  countAddedByKind,
  KIND_DEFAULT_INPUTS,
  KIND_DEFAULT_OUTPUTS,
  type UnifiedModelItem,
  type UnifiedModelKind,
} from './unified-models';
import styles from '../../Settings.module.css';

interface DiscoveredModel {
  id: string;
  name?: string;
  context?: number | null;
  contextWindow?: number | null;
  maxOutput?: number | null;
  maxTokens?: number | null;
  maxOutputTokens?: number | null;
  image?: boolean;
  vision?: boolean;
  video?: boolean;
  audio?: boolean;
  reasoning?: boolean;
  xhigh?: boolean;
  type?: string;
  defaultThinkingLevel?: string;
  thinkingLevels?: string[];
  compat?: Record<string, unknown>;
  toolUse?: Record<string, unknown>;
  visionCapabilities?: Record<string, unknown>;
  inputs?: unknown;
  outputs?: unknown;
  web?: boolean;
  structuredOutput?: boolean;
}

type CapabilityKind = 'image' | 'video' | 'audio' | 'reasoning' | 'tools';
type ProviderModelEntry = string | { id: string; [key: string]: unknown };
type DefaultParamsCapability = 'imageGeneration' | 'videoGeneration';

const CUSTOM_CATEGORY_OPTIONS: Array<{ value: UnifiedModelKind; labelKey: string }> = [
  { value: 'chat', labelKey: 'settings.api.customModelCategory.chat' },
  { value: 'image', labelKey: 'settings.api.customModelCategory.image' },
  { value: 'video', labelKey: 'settings.api.customModelCategory.video' },
  { value: 'speech', labelKey: 'settings.api.customModelCategory.speech' },
];

function modelIdOf(model: ProviderModelEntry): string {
  return typeof model === 'object' ? model.id : model;
}

function numberFromMeta(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

function boolFromMeta(value: unknown): boolean | undefined {
  return typeof value === 'boolean' ? value : undefined;
}

function plainObjectFromMeta(value: unknown): Record<string, unknown> | undefined {
  return !!value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : undefined;
}

function compactDiscoveredModelEntry(model: DiscoveredModel): ProviderModelEntry {
  const id = model.id.trim();
  if (!id) return model.id;

  const entry: Record<string, unknown> = { id };
  const name = typeof model.name === 'string' ? model.name.trim() : '';
  if (name && name !== id) entry.name = name;

  const context = numberFromMeta(model.context) ?? numberFromMeta(model.contextWindow);
  if (context !== undefined) entry.context = context;

  const maxOutput = numberFromMeta(model.maxOutput)
    ?? numberFromMeta(model.maxTokens)
    ?? numberFromMeta(model.maxOutputTokens);
  if (maxOutput !== undefined) entry.maxOutput = maxOutput;

  const image = boolFromMeta(model.image ?? model.vision);
  if (image !== undefined) entry.image = image;
  for (const key of ['video', 'audio', 'reasoning', 'xhigh', 'web', 'structuredOutput'] as const) {
    const value = boolFromMeta(model[key]);
    if (value !== undefined) entry[key] = value;
  }
  if (Array.isArray(model.inputs)) entry.inputs = [...model.inputs];
  if (Array.isArray(model.outputs)) entry.outputs = [...model.outputs];

  if (typeof model.type === 'string' && model.type.trim()) entry.type = model.type.trim();
  if (typeof model.defaultThinkingLevel === 'string' && model.defaultThinkingLevel.trim()) {
    entry.defaultThinkingLevel = model.defaultThinkingLevel.trim();
  }
  if (Array.isArray(model.thinkingLevels)) entry.thinkingLevels = [...model.thinkingLevels];
  for (const key of ['compat', 'toolUse', 'visionCapabilities'] as const) {
    const value = plainObjectFromMeta(model[key]);
    if (value) entry[key] = value;
  }

  return Object.keys(entry).length === 1 ? id : entry as ProviderModelEntry;
}

function CapabilityIcon({ kind }: { kind: CapabilityKind }) {
  const label = t(`settings.api.capability.${kind}`);
  return (
    <span className={styles['pv-capability-icon']} title={label} aria-label={label}>
      {kind === 'image' ? (
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
          <circle cx="8.5" cy="8.5" r="1.5" />
          <path d="M21 15l-5-5L5 21" />
        </svg>
      ) : kind === 'video' ? (
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <rect x="3" y="5" width="13" height="14" rx="2" />
          <path d="m16 9 5-3v12l-5-3" />
        </svg>
      ) : kind === 'audio' ? (
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M4 10v4" />
          <path d="M8 7v10" />
          <path d="M12 4v16" />
          <path d="M16 8v8" />
          <path d="M20 11v2" />
        </svg>
      ) : kind === 'tools' ? (
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z" />
        </svg>
      ) : (
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M9 18h6" />
          <path d="M10 22h4" />
          <path d="M12 2a7 7 0 0 0-4 12.74V16a1 1 0 0 0 1 1h6a1 1 0 0 0 1-1v-1.26A7 7 0 0 0 12 2Z" />
        </svg>
      )}
    </span>
  );
}

/** 媒体增删走 manager/service 生命周期（route → manager/service → registry）。 */
function mediaRouteOf(kind: UnifiedModelKind): 'image' | 'video' | null {
  if (kind === 'image') return 'image';
  if (kind === 'video') return 'video';
  return null;
}

export function ProviderModelList({ providerId, summary, media, onRefresh }: {
  providerId: string;
  summary: ProviderSummary;
  media: UseMediaSettingsDataResult;
  onRefresh: () => Promise<void>;
}) {
  const showToast = useSettingsStore(s => s.showToast);
  const [search, setSearch] = useState('');
  const [customInput, setCustomInput] = useState('');
  const [customCategory, setCustomCategory] = useState<UnifiedModelKind>('chat');
  const [discoveredModels, setDiscoveredModels] = useState<DiscoveredModel[]>([]);
  const [defaultsModal, setDefaultsModal] = useState<DefaultParamsCapability | null>(null);

  const loadDiscoveredModels = async () => {
    try {
      const res = await lingxiFetch(`/api/providers/${encodeURIComponent(providerId)}/discovered-models`);
      const data = await res.json();
      setDiscoveredModels(data.models || []);
    } catch {
      // cache miss is fine
    }
  };

  useEffect(() => { loadDiscoveredModels(); }, [providerId]);

  // ── 统一 ViewModel ────────────────────────────────────────────────────────
  const unifiedItems = useMemo(
    () => buildUnifiedModelItems({ providerId, summary, media }),
    [providerId, summary, media],
  );
  const addedCounts = useMemo(() => countAddedByKind(unifiedItems), [unifiedItems]);
  const resolvedCapabilities = useMemo(
    () => resolveProviderMediaCapabilities(summary.media_capability_bindings, media),
    [summary.media_capability_bindings, media],
  );
  // 可人工管理的媒体类别（runtime-discovered 目录不允许自定义 mutation）
  const manageableMediaBindings = useMemo(
    () => resolvedCapabilities.filter(cap => cap.available
      && !!cap.provider
      && !(cap.provider as MediaProvider).runtimeCapability),
    [resolvedCapabilities],
  );

  const rawModels = summary.models || [];
  const currentModelIds = rawModels.map(modelIdOf);

  const refreshMediaKind = useCallback(async (kind: UnifiedModelKind) => {
    if (kind === 'image') await media.refreshImage();
    else if (kind === 'video') await media.refreshVideo();
    else if (kind === 'speech') await media.refreshSpeech();
  }, [media]);

  const addMediaModel = async (kind: UnifiedModelKind, runtimeProviderId: string, model: Record<string, unknown>) => {
    const route = mediaRouteOf(kind);
    const url = route
      ? `/api/media/${route}/providers/${encodeURIComponent(runtimeProviderId)}/models`
      : `/api/speech-recognition/providers/${encodeURIComponent(runtimeProviderId)}/models`;
    try {
      await lingxiFetchJson(url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ model }),
      });
      invalidateConfigCache();
      await refreshMediaKind(kind);
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      showToast(msg, 'error');
    }
  };

  const removeUnifiedModel = async (item: UnifiedModelItem) => {
    const route = mediaRouteOf(item.kind);
    // 认领行的真实数据在 chat 槽（provider.models）：走媒体 DELETE 是空操作，
    // 会弹「保存成功」却删不掉（服务端现在也会如实报 not found）。
    if ((route || item.kind === 'speech') && !item.claimedFromChat) {
      const url = route
        ? `/api/media/${route}/providers/${encodeURIComponent(item.runtimeProviderId)}/models/${encodeURIComponent(item.id)}`
        : `/api/speech-recognition/providers/${encodeURIComponent(item.runtimeProviderId)}/models/${encodeURIComponent(item.id)}`;
      try {
        await lingxiFetchJson(url, { method: 'DELETE' });
        invalidateConfigCache();
        await refreshMediaKind(item.kind);
      } catch (err: unknown) {
        const msg = err instanceof Error ? err.message : String(err);
        showToast(msg, 'error');
      }
      return;
    }
    try {
      const next = rawModels.filter((m: ProviderModelEntry) => modelIdOf(m) !== item.id);
      await lingxiFetchJson('/api/config', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ providers: { [providerId]: { models: next } } }),
      });
      invalidateConfigCache();
      // chat 槽变更会同时改变后端的媒体投影（词典 type 合并）与前端认领结果，
      // 必须两边一起刷新，否则出现幽灵行 / image 行延迟出现。
      await Promise.all([onRefresh(), media.refreshAll()]);
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      showToast(msg, 'error');
    }
  };

  const addModelToProvider = async (mid: string) => {
    if (currentModelIds.includes(mid)) return;
    try {
      const discovered = discoveredModels.find(model => model.id === mid);
      const nextEntry = discovered ? compactDiscoveredModelEntry(discovered) : mid;
      await lingxiFetchJson('/api/config', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ providers: { [providerId]: { models: [...rawModels, nextEntry] } } }),
      });
      invalidateConfigCache();
      await Promise.all([onRefresh(), media.refreshAll()]);
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      showToast(msg, 'error');
    }
  };

  const addCustomModel = async () => {
    const id = customInput.trim();
    if (!id) return;
    if (customCategory === 'chat') {
      if (currentModelIds.includes(id)) {
        setCustomInput('');
        return;
      }
      try {
        await lingxiFetchJson('/api/config', {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ providers: { [providerId]: { models: [...rawModels, id] } } }),
        });
        invalidateConfigCache();
        setCustomInput('');
        await Promise.all([onRefresh(), media.refreshAll()]);
      } catch (err: unknown) {
        const msg = err instanceof Error ? err.message : String(err);
        showToast(msg, 'error');
      }
      return;
    }
    // 媒体自定义模型：提交 ID + 类别默认 seed 模态（协议由 ProviderRegistry
    // 按 capability 推导），让已添加模型在统一列表/编辑面板里立即有合理初始模态。
    const binding = manageableMediaBindings.find((cap) => {
      if (customCategory === 'image') return cap.capability === 'imageGeneration';
      if (customCategory === 'video') return cap.capability === 'videoGeneration';
      return cap.capability === 'speechRecognition';
    });
    if (!binding) return;
    const alreadyAdded = unifiedItems.some(
      (item) => item.kind === customCategory && item.runtimeProviderId === binding.runtimeProviderId && item.id === id,
    );
    if (alreadyAdded) {
      setCustomInput('');
      return;
    }
    await addMediaModel(customCategory, binding.runtimeProviderId, {
      id,
      inputs: KIND_DEFAULT_INPUTS[customCategory],
      outputs: KIND_DEFAULT_OUTPUTS[customCategory],
    });
    setCustomInput('');
  };

  const [fetchHint, setFetchHint] = useState<{ msg: string; ok: boolean } | null>(null);
  const fetchHintTimer = useRef<ReturnType<typeof setTimeout>>(null);

  const showFetchHint = (msg: string, ok: boolean) => {
    if (fetchHintTimer.current) clearTimeout(fetchHintTimer.current);
    setFetchHint({ msg, ok });
    fetchHintTimer.current = setTimeout(() => setFetchHint(null), 2500);
  };

  const fetchModels = async (btn: HTMLButtonElement | null) => {
    if (btn) btn.classList.add(styles['spinning']);
    try {
      // 优先用面板里的草稿凭证：key 可能刚输入还没保存（onBlur 保存与点击有竞态），
      // 脱敏占位由服务端回落到已保存明文。
      const draft = useSettingsStore.getState().providerCredentialDrafts?.[providerId];
      const body: Record<string, unknown> = {
        name: providerId,
        base_url: draft?.base_url || summary.base_url,
        api: draft?.api || summary.api,
      };
      if (draft?.api_key) body.api_key = draft.api_key;
      if (draft?.headers) body.headers = draft.headers;
      const res = await lingxiFetch('/api/providers/fetch-models', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      const data = await res.json();
      if (data.error) {
        const detail = typeof data.error === 'string' && data.error.trim() ? `: ${data.error.trim()}` : '';
        showFetchHint(t('settings.providers.fetchFailed') + detail, false);
        return;
      }
      const models = (data.models || []) as DiscoveredModel[];
      if (models.length === 0) { showFetchHint(t('settings.providers.fetchFailed'), false); return; }
      // Backend already cached the results; just refresh the dropdown
      setDiscoveredModels(models);
      setSearch('');
      setDropdownOpen(true);
      showFetchHint(t('settings.providers.fetchSuccess', { name: providerId, n: models.length }), true);
      // 顺带同步刷新媒体 catalog 数据；不改变 media provider 原有 discovery 机制
      media.refreshAll().catch(() => {});
    } catch {
      showFetchHint(t('settings.providers.fetchFailed'), false);
    } finally {
      if (btn) btn.classList.remove(styles['spinning']);
    }
  };

  const [dropdownOpen, setDropdownOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const closeDropdown = useCallback(() => setDropdownOpen(false), []);
  const panelStyle = useAnchoredDropdown({
    open: dropdownOpen,
    triggerRef,
    panelRef,
    onClose: closeDropdown,
    widthOffset: 80,
  });

  const [editing, setEditing] = useState<{ item: UnifiedModelItem; anchor: HTMLElement } | null>(null);

  // ── 统一候选（分组下拉） ──────────────────────────────────────────────────
  const query = search.toLowerCase();

  interface MediaCandidate {
    kind: UnifiedModelKind;
    runtimeProviderId: string;
    id: string;
    displayName: string;
    model: Record<string, unknown>;
    added: boolean;
  }
  // 媒体候选（未过搜索词）先算：chat 候选要拿它的 id 集合做互斥
  const allMediaCandidates = useMemo<MediaCandidate[]>(() => {
    const candidates: MediaCandidate[] = [];
    for (const cap of resolvedCapabilities) {
      if (!cap.available || !cap.provider) continue;
      const kind: UnifiedModelKind | null = cap.capability === 'imageGeneration'
        ? 'image'
        : cap.capability === 'videoGeneration' ? 'video' : cap.capability === 'speechRecognition' ? 'speech' : null;
      if (!kind) continue;
      const runtimeProviderId = cap.runtimeProviderId;
      const addedIds = new Set(
        unifiedItems.filter(item => item.kind === kind && item.runtimeProviderId === runtimeProviderId).map(item => item.id),
      );
      const seen = new Set<string>();
      const pushCandidate = (id: string, displayName: string, model: Record<string, unknown>) => {
        if (!id || seen.has(id)) return;
        seen.add(id);
        const candidateModel = { ...(model && typeof model === 'object' ? model : {}), id };
        candidates.push({
          kind,
          runtimeProviderId,
          id,
          displayName: displayName || id,
          model: candidateModel,
          added: addedIds.has(id),
        });
      };
      const provider = cap.provider as MediaProvider & { catalogModels?: { id: string; name: string }[] };
      for (const model of provider.models || []) {
        pushCandidate(model.id, String(model.displayName || model.name || model.id), model as Record<string, unknown>);
      }
      for (const model of provider.availableModels || []) {
        pushCandidate(model.id, model.name || model.id, model as Record<string, unknown>);
      }
      if (kind === 'speech') {
        for (const model of provider.catalogModels || []) {
          pushCandidate(model.id, model.name || model.id, model as Record<string, unknown>);
        }
      }
    }
    return candidates;
  }, [resolvedCapabilities, unifiedItems]);
  const mediaCandidateIds = useMemo(() => new Set(allMediaCandidates.map(c => c.id)), [allMediaCandidates]);
  const mediaCandidates = useMemo(() => (
    query
      ? allMediaCandidates.filter(c => c.id.toLowerCase().includes(query) || c.displayName.toLowerCase().includes(query))
      : allMediaCandidates
  ), [allMediaCandidates, query]);

  const chatCandidates = useMemo(() => {
    const discoveredIds = discoveredModels.map(m => m.id);
    const all = [...new Set([...currentModelIds, ...discoveredIds, ...(summary.custom_models || [])])];
    const matched = query ? all.filter(m => m.toLowerCase().includes(query)) : all;
    // 媒体模型的唯一入口是「图片/视频/语音识别模型」分组：远端 /models 目录和
    // known 词典都可能把它们混进 chat 候选（agnes-image-2.1-flash 这类不在词典
    // 里的模型靠媒体目录互斥），进 chat 组就会被写进 chat 槽造成双行重复。
    return matched.filter(mid => !mediaCandidateIds.has(mid)
      && chatEntryMediaKind(
        mid,
        rawModels.find((m: ProviderModelEntry) => modelIdOf(m) === mid) as Record<string, unknown> | undefined
          ?? (discoveredModels.find(d => d.id === mid) as unknown as Record<string, unknown> | undefined)
          ?? {},
        providerId,
      ) === null);
  }, [currentModelIds.join('\n'), discoveredModels, summary.custom_models, query, rawModels, providerId, mediaCandidateIds]);

  const chatCandidatesById = new Set(chatCandidates);
  const mediaCandidateGroups = useMemo(() => {
    const groups: Array<{ kind: UnifiedModelKind; labelKey: string; items: MediaCandidate[] }> = [];
    for (const kind of ['image', 'video', 'speech'] as UnifiedModelKind[]) {
      const items = mediaCandidates.filter(c => c.kind === kind);
      if (items.length > 0) {
        groups.push({
          kind,
          labelKey: `settings.api.modelGroup.${kind}`,
          items,
        });
      }
    }
    return groups;
  }, [mediaCandidates]);

  const candidateCategoryOptions = useMemo(() => {
    const options = [CUSTOM_CATEGORY_OPTIONS[0]];
    for (const option of CUSTOM_CATEGORY_OPTIONS.slice(1)) {
      const manageable = manageableMediaBindings.some((cap) => {
        if (option.value === 'image') return cap.capability === 'imageGeneration';
        if (option.value === 'video') return cap.capability === 'videoGeneration';
        return cap.capability === 'speechRecognition';
      });
      if (manageable) options.push(option);
    }
    return options;
  }, [manageableMediaBindings]);

  const defaultParamsCapabilities = useCallback((capability: DefaultParamsCapability): ResolvedMediaCapability[] => (
    resolvedCapabilities.filter(cap =>
      cap.capability === capability
      && (capability === 'videoGeneration' ? media.video : media.image).providers[cap.runtimeProviderId],
    )
  ), [resolvedCapabilities, media]);

  return (
    <div className={styles['pv-models']} data-unified-model-list="true">
      {/* 已添加的模型：chat / image / video / speech 统一列表 */}
      {unifiedItems.length > 0 && (
        <div className={styles['pv-fav-section']}>
          <div className={styles['pv-fav-title']}>
            {t('settings.api.addedModels')}
            <span className={styles['pv-models-count']}>{unifiedItems.length}</span>
          </div>
          <div className={styles['pv-fav-list']}>
            {unifiedItems.map(item => {
              // chat 行：lookupModelMeta（known catalog）+ 用户条目合并，保持旧行为
              const rawEntry = item.kind === 'chat'
                ? rawModels.find((m: ProviderModelEntry) => modelIdOf(m) === item.id)
                : null;
              const entryMeta: Record<string, unknown> = rawEntry && typeof rawEntry === 'object' ? rawEntry : {};
              const knownMeta: Record<string, any> = item.kind === 'chat' ? (lookupModelMeta(item.id, providerId) || {}) : {};
              const meta = { ...knownMeta, ...entryMeta };
              const modelContext = item.kind === 'chat'
                ? (numberFromMeta(entryMeta.context)
                  ?? numberFromMeta(entryMeta.contextWindow)
                  ?? numberFromMeta(knownMeta.context)
                  ?? numberFromMeta(knownMeta.contextWindow))
                : undefined;
              return (
                <div key={item.key} className={styles['pv-fav-item']} data-unified-kind={item.kind} data-model-id={item.id}>
                  <span className={styles['pv-fav-item-name']} title={String(item.displayName)}>{item.displayName}</span>
                  {/* 输出模态图标：紧邻显示名，剩余空间由 spacer 吸收 */}
                  <OutputModalityIcons outputs={item.outputs} />
                  <span className={styles['pv-fav-item-spacer']} aria-hidden="true" />
                  {/* 输入能力图标：所有类别都渲染——媒体模型同样要表达它接受的输入模态
                      （图片模型 text/image 输入、视频模型 text/image、语音模型 audio） */}
                  {item.inputs.includes('image') && <CapabilityIcon kind="image" />}
                  {item.inputs.includes('video') && <CapabilityIcon kind="video" />}
                  {item.inputs.includes('audio') && <CapabilityIcon kind="audio" />}
                  {item.kind === 'chat' && (
                    <>
                      {meta.reasoning === true && <CapabilityIcon kind="reasoning" />}
                      {meta.toolUse && typeof meta.toolUse === 'object' && meta.toolUse.supportsTools === true && <CapabilityIcon kind="tools" />}
                    </>
                  )}
                  {item.isDefault && <span className={styles['settings-default-badge']} data-default-badge="true">{t('settings.media.default')}</span>}
                  {modelContext !== undefined && <span className={styles['pv-model-ctx']}>{formatContext(modelContext)}</span>}
                  {(item.editable || item.removable) && (
                    <div className={styles['pv-fav-item-actions']}>
                      {item.editable && (
                        <button
                          className={styles['pv-fav-item-edit']}
                          title={t('settings.api.editModel')}
                          data-unified-edit={item.kind}
                          onClick={(e) => setEditing({ item, anchor: e.currentTarget })}
                        >
                          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                            <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
                            <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
                          </svg>
                        </button>
                      )}
                      {item.removable && (
                        <button
                          className={styles['pv-fav-item-remove']}
                          data-unified-remove={item.kind}
                          onClick={() => removeUnifiedModel(item)}
                          title={t('settings.api.removeModel')}
                        >
                          <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                            <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
                          </svg>
                        </button>
                      )}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
          {/* 媒体能力异常状态：紧凑提示，不重建独立媒体区域（禁止静默降级） */}
          {resolvedCapabilities.map((cap) => {
            if (!cap.available || !cap.provider) return null;
            const credentialOk = !!cap.provider.hasCredentials;
            if (credentialOk) return null;
            const provider = cap.provider as MediaProvider;
            // unavailableReason 是内部错误码（如 no_credentials），不能直接渲染给用户
            const message = cap.capability === 'speechRecognition'
              ? t('settings.media.credentialMissing')
              : (provider.unavailableMessage
                || provider.runtimeCapability?.error?.message
                || t('settings.media.credentialMissing'));
            return (
              <div
                key={`status:${cap.capability}:${cap.runtimeProviderId}`}
                className={styles['settings-credential-status']}
                data-capability-status={`${cap.capability}:${cap.runtimeProviderId}`}
              >
                <span className={styles['settings-credential-dot']} />
                {message}
              </div>
            );
          })}
          {resolvedCapabilities.map((cap) => {
            if (cap.available || cap.loading) return null;
            const label = cap.capability === 'imageGeneration'
              ? t('settings.api.modelGroup.image')
              : cap.capability === 'videoGeneration'
                ? t('settings.api.modelGroup.video')
                : t('settings.api.modelGroup.speech');
            return (
              <div
                key={`unavailable:${cap.capability}:${cap.runtimeProviderId}`}
                className={styles['settings-credential-status']}
                data-capability-unavailable={`${cap.capability}:${cap.runtimeProviderId}`}
              >
                <span className={styles['settings-credential-dot']} />
                {label}：{t('settings.media.runtimeUnavailable')}
              </div>
            );
          })}
          {editing && (
            <ModelEditPanel
              kind={editing.item.kind}
              providerId={providerId}
              runtimeProviderId={editing.item.runtimeProviderId}
              modelId={editing.item.id}
              modelMeta={editing.item.sourceModel && typeof editing.item.sourceModel === 'object'
                ? editing.item.sourceModel as Record<string, unknown>
                : undefined}
              summaryApi={summary.api}
              summaryBaseUrl={summary.base_url}
              anchorEl={editing.anchor}
              onClose={() => setEditing(null)}
              onRefresh={async () => {
                if (editing.item.kind === 'chat') await onRefresh();
                else await refreshMediaKind(editing.item.kind);
              }}
            />
          )}
        </div>
      )}

      {/* 添加模型 + 读取模型 */}
      <div className={styles['pv-models-action-row']}>
        <button ref={triggerRef} className={styles['pv-model-dropdown-trigger']} onClick={() => setDropdownOpen(!dropdownOpen)}>
          <span>{t('settings.api.addModel')}</span>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <polyline points="6 9 12 15 18 9" />
          </svg>
        </button>
        <button
          className={styles['pv-fetch-btn-inline']}
          title={t('settings.providers.fetchModels')}
          onClick={(e) => fetchModels(e.currentTarget)}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
            <polyline points="23 4 23 10 17 10" /><polyline points="1 20 1 14 7 14" />
            <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
          </svg>
          {t('settings.providers.fetchModels')}
        </button>
      </div>

      {/* 默认参数按钮：同一行、nowrap；只在存在已添加的对应类别模型时显示 */}
      {(addedCounts.image > 0 || addedCounts.video > 0) && (
        <div className={styles['pv-default-params-row']} data-default-params-row="true">
          {addedCounts.image > 0 && (
            <button
              className={styles['pv-default-params-btn']}
              onClick={() => setDefaultsModal('imageGeneration')}
            >
              {t('settings.media.imageDefaultsButton')}
            </button>
          )}
          {addedCounts.video > 0 && (
            <button
              className={styles['pv-default-params-btn']}
              onClick={() => setDefaultsModal('videoGeneration')}
            >
              {t('settings.media.videoDefaultsButton')}
            </button>
          )}
        </div>
      )}

      {fetchHint && <div className={`${styles['pv-fetch-hint']} ${fetchHint.ok ? styles['ok'] : styles['fail']}`}>{fetchHint.msg}</div>}
      {dropdownOpen && createPortal(
          <div
            className={styles['pv-model-dropdown-panel']}
            ref={panelRef}
            style={panelStyle}
            data-provider-model-dropdown="true"
            onKeyDown={(e) => { if (e.key === 'Escape') closeDropdown(); }}
          >
            <input
              className={styles['pv-model-dropdown-search']}
              type="text"
              placeholder={t('settings.api.searchModel')}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              autoFocus
            />
            <div className={styles['pv-model-dropdown-list']}>
              {chatCandidates.length > 0 && (
                <>
                  <div className={styles['pv-model-dropdown-group']} data-model-group="chat">{t('settings.api.modelGroup.chat')}</div>
                  {chatCandidates.map(mid => renderChatOption(mid))}
                </>
              )}
              {mediaCandidateGroups.map(group => (
                <React.Fragment key={group.kind}>
                  <div className={styles['pv-model-dropdown-group']} data-model-group={group.kind}>{t(group.labelKey)}</div>
                  {group.items.map(candidate => (
                    <button
                      key={`${candidate.kind}:${candidate.runtimeProviderId}:${candidate.id}`}
                      className={`${styles['pv-model-dropdown-option']}${candidate.added ? ' ' + styles['added'] : ''}`}
                      data-media-candidate={`${candidate.kind}:${candidate.runtimeProviderId}:${candidate.id}`}
                      onClick={() => {
                        if (!candidate.added) {
                          // 候选模型尽量提交完整 metadata，而不是只提交 ID
                          addMediaModel(candidate.kind, candidate.runtimeProviderId, candidate.model);
                        }
                      }}
                    >
                      <span className={styles['pv-model-dropdown-option-name']}>{candidate.displayName}</span>
                      {candidate.added && <span className={styles['pv-model-dropdown-option-check']}>{'\u2713'}</span>}
                    </button>
                  ))}
                </React.Fragment>
              ))}
              {chatCandidates.length === 0 && mediaCandidateGroups.length === 0 && (
                <div className={styles['pv-model-dropdown-empty']}>{t('settings.providers.noModels')}</div>
              )}
            </div>
            <div className={styles['pv-model-dropdown-custom']}>
              <input
                className={styles['pv-model-dropdown-custom-input']}
                type="text"
                placeholder={t('settings.oauth.customModelPlaceholder')}
                value={customInput}
                onChange={(e) => setCustomInput(e.target.value)}
                onKeyDown={(e) => { if (e.key === 'Enter') { addCustomModel(); } }}
              />
              <select
                className={styles['pv-custom-category-select']}
                value={customCategory}
                onChange={(e) => setCustomCategory(e.target.value as UnifiedModelKind)}
                aria-label={t('settings.api.customModelCategory.label')}
                data-custom-model-category={customCategory}
                disabled={candidateCategoryOptions.length <= 1}
              >
                {candidateCategoryOptions.map(option => (
                  <option key={option.value} value={option.value}>{t(option.labelKey)}</option>
                ))}
              </select>
              <button className={styles['pv-model-add-btn']} data-custom-model-add="true" onClick={addCustomModel}>{'\u21B5'}</button>
            </div>
          </div>,
          document.body,
        )}
      {defaultsModal && (
        <ProviderMediaDefaultsModal
          capability={defaultsModal}
          capabilities={defaultParamsCapabilities(defaultsModal)}
          media={media}
          onClose={() => setDefaultsModal(null)}
        />
      )}
    </div>
  );

  function renderChatOption(mid: string) {
    const isAdded = currentModelIds.includes(mid);
    const meta: Record<string, any> = lookupModelMeta(mid, providerId) || {};
    const rawEntry = rawModels.find((model: ProviderModelEntry) => modelIdOf(model) === mid);
    const userMeta: Record<string, unknown> = rawEntry && typeof rawEntry === 'object' ? rawEntry : {};
    const discovered = discoveredModels.find(d => d.id === mid);
    const ctx = numberFromMeta(userMeta.context)
      ?? numberFromMeta(userMeta.contextWindow)
      ?? numberFromMeta(meta.context)
      ?? numberFromMeta(meta.contextWindow)
      ?? numberFromMeta(discovered?.context)
      ?? numberFromMeta(discovered?.contextWindow);
    return (
      <button
        key={`chat:${mid}`}
        className={`${styles['pv-model-dropdown-option']}${isAdded ? ' ' + styles['added'] : ''}`}
        onClick={() => { if (!isAdded) { addModelToProvider(mid); } }}
      >
        <span className={styles['pv-model-dropdown-option-name']}>{mid}</span>
        {isAdded && <span className={styles['pv-model-dropdown-option-check']}>{'\u2713'}</span>}
        {ctx && <span className={styles['pv-model-ctx']}>{formatContext(ctx)}</span>}
      </button>
    );
  }
}
