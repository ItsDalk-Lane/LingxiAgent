/**
 * useMediaSettingsData — 统一媒体数据层
 *
 * 统一管理 image / video / speech 三套资源的 providers + config + load/refresh/save。
 * 单一职责：数据获取与写入，不做任何 UI 布局。
 *
 * 语义契约（迁移自原媒体数据层）：
 *   - undefined → HTTP null（清除配置必须发 { values: { defaultImageModel: null } }）
 *   - speech 保存后同步 settingsSnapshot.preferences.speechRecognition
 *   - refresh 失败保留 last-known-good，不清空已有 provider 列表
 *   - 首次挂载 image/video/speech 并发加载、各自独立失败（Promise.allSettled）
 *   - 每个资源独立 mutation queue，写入按序提交，后发修改基于最新本地快照
 */
import { useCallback, useEffect, useRef, useState } from 'react';
import { useSettingsStore } from '../store';
import { lingxiFetch } from '../api';
import { updateSettingsSnapshot } from '../actions';
import { t } from '../helpers';

export interface MediaModel {
  id: string;
  name?: string;
  displayName?: string;
  protocolId?: string;
  adapterAvailable?: boolean;
  aliases?: string[];
  [key: string]: unknown;
}

export interface MediaProvider {
  providerId: string;
  displayName?: string;
  hasCredentials: boolean;
  unavailableReason?: string | null;
  unavailableMessage?: string | null;
  runtimeCapability?: {
    status?: string;
    error?: { code?: string; message?: string } | null;
  } | null;
  models: MediaModel[];
  availableModels: { id: string; name: string }[];
}

export interface MediaConfig {
  defaultImageModel?: { id: string; provider: string };
  defaultVideoModel?: { id: string; provider: string };
  providerDefaults?: Record<string, any>;
}

export interface SpeechModel {
  id: string;
  name?: string;
  displayName?: string;
  protocolId?: string;
  adapterAvailable?: boolean;
}

export interface SpeechProvider {
  providerId: string;
  displayName?: string;
  hasCredentials: boolean;
  unavailableReason?: string | null;
  models: SpeechModel[];
  availableModels?: { id: string; name: string }[];
  /** 内置声明（未被用户添加）的候选模型，仅用于「添加模型」下拉 */
  catalogModels?: { id: string; name: string }[];
}

export interface SpeechConfig {
  enabled: boolean;
  defaultModel?: { id: string; provider: string };
}

type SpeechConfigPatch = {
  enabled?: boolean;
  defaultModel?: SpeechConfig['defaultModel'] | null;
};

type MediaConfigPatch = Partial<MediaConfig>;

/** 更新函数形式：基于最新本地快照计算 patch，规避旧 React closure。 */
export type MediaConfigUpdater = MediaConfigPatch | ((latest: MediaConfig) => MediaConfigPatch);
export type SpeechConfigUpdater = SpeechConfigPatch | ((latest: SpeechConfig) => SpeechConfigPatch);

interface ResourceState<P, C> {
  providers: Record<string, P>;
  config: C | null;
  loading: boolean;
  error: string | null;
}

const LOADING_SELECT_VALUE = '__loading';

export function encodeConfigPatch(updates: Partial<MediaConfig>): Record<string, unknown> {
  return Object.fromEntries(
    Object.entries(updates).map(([key, value]) => [key, value === undefined ? null : value]),
  );
}

export function applyConfigPatch(prev: MediaConfig, updates: Partial<MediaConfig>): MediaConfig {
  const next: MediaConfig = { ...prev };
  for (const [key, value] of Object.entries(updates) as Array<[keyof MediaConfig, MediaConfig[keyof MediaConfig]]>) {
    if (value === undefined) delete next[key];
    else next[key] = value as any;
  }
  return next;
}

function encodeSpeechConfigPatch(updates: SpeechConfigPatch): Record<string, unknown> {
  return Object.fromEntries(
    Object.entries(updates).map(([key, value]) => [key, value === undefined ? null : value]),
  );
}

export function applySpeechConfigPatch(prev: SpeechConfig, updates: SpeechConfigPatch): SpeechConfig {
  const next: SpeechConfig = { ...prev };
  if (typeof updates.enabled === 'boolean') next.enabled = updates.enabled;
  if ('defaultModel' in updates) {
    if (updates.defaultModel) next.defaultModel = updates.defaultModel;
    else delete next.defaultModel;
  }
  return next;
}

export function mergeSpeechConfig(prev: SpeechConfig, incoming: any): SpeechConfig {
  const next: SpeechConfig = { ...prev };
  if (typeof incoming?.enabled === 'boolean') next.enabled = incoming.enabled;
  if (incoming && Object.prototype.hasOwnProperty.call(incoming, 'defaultModel')) {
    if (incoming.defaultModel) next.defaultModel = incoming.defaultModel;
    else delete next.defaultModel;
  }
  return next;
}

export function speechModelLabel(model: SpeechModel | { id: string; name: string }): string {
  return 'displayName' in model && model.displayName ? model.displayName : model.name || model.id;
}

export function getRunnableSpeechModels(provider: SpeechProvider): Array<{ id: string; name: string }> {
  if (!provider.hasCredentials) return [];
  if (Array.isArray(provider.availableModels)) {
    return provider.availableModels.map(model => ({ id: model.id, name: model.name || model.id }));
  }
  return (provider.models || [])
    .filter(model => model.adapterAvailable !== false)
    .map(model => ({ id: model.id, name: speechModelLabel(model) }));
}

export function resolveMediaConfigUpdater<C, P>(
  updates: P | ((latest: C) => P),
  latest: C,
): P {
  return typeof updates === 'function' ? (updates as (latest: C) => P)(latest) : updates;
}

export interface UseMediaSettingsDataResult {
  image: ResourceState<MediaProvider, MediaConfig>;
  video: ResourceState<MediaProvider, MediaConfig>;
  speech: ResourceState<SpeechProvider, SpeechConfig>;

  allImageModels: Array<MediaModel & { provider: string }>;
  allVideoModels: Array<MediaModel & { provider: string }>;
  allSpeechModels: Array<{ id: string; name: string; provider: string }>;
  speechEnabled: boolean;

  refreshImage: () => Promise<void>;
  refreshVideo: () => Promise<void>;
  refreshSpeech: () => Promise<void>;
  refreshAll: () => Promise<void>;

  saveImageConfig: (updates: MediaConfigUpdater) => Promise<void>;
  saveVideoConfig: (updates: MediaConfigUpdater) => Promise<void>;
  saveSpeechConfig: (updates: SpeechConfigUpdater) => Promise<void>;
}

const EMPTY_IMAGE: ResourceState<MediaProvider, MediaConfig> = {
  providers: {}, config: null, loading: true, error: null,
};
const EMPTY_VIDEO: ResourceState<MediaProvider, MediaConfig> = {
  providers: {}, config: null, loading: true, error: null,
};
const EMPTY_SPEECH: ResourceState<SpeechProvider, SpeechConfig> = {
  providers: {}, config: null, loading: true, error: null,
};

export function useMediaSettingsData(): UseMediaSettingsDataResult {
  const showToast = useSettingsStore(s => s.showToast);
  const snapshotSpeechConfig = useSettingsStore(s => s.settingsSnapshot.data?.preferences?.speechRecognition);

  const [image, setImage] = useState<ResourceState<MediaProvider, MediaConfig>>(EMPTY_IMAGE);
  const [video, setVideo] = useState<ResourceState<MediaProvider, MediaConfig>>(EMPTY_VIDEO);
  const [speech, setSpeech] = useState<ResourceState<SpeechProvider, SpeechConfig>>(() => ({
    providers: {},
    config: snapshotSpeechConfig ? mergeSpeechConfig({ enabled: false }, snapshotSpeechConfig) : null,
    loading: !snapshotSpeechConfig,
    error: null,
  }));

  // 权威最新快照 refs：save 基于最新本地快照，而不是旧 React closure。
  const imageConfigRef = useRef<MediaConfig>({});
  const videoConfigRef = useRef<MediaConfig>({});
  const speechConfigRef = useRef<SpeechConfig>({ enabled: false });
  // 每个资源独立的 mutation queue，序列化写入。
  const imageWriteQueueRef = useRef<Promise<unknown>>(Promise.resolve());
  const videoWriteQueueRef = useRef<Promise<unknown>>(Promise.resolve());
  const speechWriteQueueRef = useRef<Promise<unknown>>(Promise.resolve());

  useEffect(() => {
    if (!snapshotSpeechConfig) return;
    setSpeech(prev => ({
      ...prev,
      config: mergeSpeechConfig({ enabled: false }, snapshotSpeechConfig),
    }));
  }, [snapshotSpeechConfig]);

  const applyImageConfig = useCallback((next: MediaConfig) => {
    imageConfigRef.current = next;
    setImage(prev => ({ ...prev, config: next }));
  }, []);

  const applyVideoConfig = useCallback((next: MediaConfig) => {
    videoConfigRef.current = next;
    setVideo(prev => ({ ...prev, config: next }));
  }, []);

  const applySpeechConfig = useCallback((next: SpeechConfig) => {
    speechConfigRef.current = next;
    setSpeech(prev => ({ ...prev, config: next }));
    updateSettingsSnapshot(snapshot => ({
      ...snapshot,
      preferences: { ...snapshot.preferences, speechRecognition: next },
    }));
  }, []);

  const loadImageResource = useCallback(async () => {
    setImage(prev => ({ ...prev, loading: prev.config === null && prev.error === null }));
    try {
      const res = await lingxiFetch('/api/media/image/providers');
      const data = await res.json();
      const nextProviders = data.providers || {};
      const nextConfig = data.config || {};
      imageConfigRef.current = nextConfig;
      setImage({ providers: nextProviders, config: nextConfig, loading: false, error: null });
    } catch (err: any) {
      // refresh 失败保留 last-known-good，不清空已有 provider 列表。
      setImage(prev => ({
        ...prev,
        loading: false,
        error: err?.message || String(err),
      }));
    }
  }, []);

  const loadVideoResource = useCallback(async () => {
    setVideo(prev => ({ ...prev, loading: prev.config === null && prev.error === null }));
    try {
      const res = await lingxiFetch('/api/media/video/providers');
      const data = await res.json();
      const nextProviders = data.providers || {};
      const nextConfig = data.config || {};
      videoConfigRef.current = nextConfig;
      setVideo({ providers: nextProviders, config: nextConfig, loading: false, error: null });
    } catch (err: any) {
      setVideo(prev => ({
        ...prev,
        loading: false,
        error: err?.message || String(err),
      }));
    }
  }, []);

  const loadSpeechResource = useCallback(async () => {
    setSpeech(prev => ({ ...prev, loading: prev.config === null && prev.error === null }));
    try {
      const res = await lingxiFetch('/api/speech-recognition/providers');
      const data = await res.json();
      const nextProviders = data.providers || {};
      const nextConfig = mergeSpeechConfig({ enabled: false }, data.config || {});
      speechConfigRef.current = nextConfig;
      setSpeech({ providers: nextProviders, config: nextConfig, loading: false, error: null });
    } catch (err: any) {
      setSpeech(prev => ({
        ...prev,
        loading: false,
        error: err?.message || String(err),
      }));
      showToast(err?.message || 'Failed to load speech recognition providers', 'error');
    }
  }, [showToast]);

  // 首次挂载：image/video/speech 并发加载，各自独立失败。
  useEffect(() => {
    void Promise.allSettled([loadImageResource(), loadVideoResource(), loadSpeechResource()]);
    const refreshRuntimeMediaProviders = () => {
      void loadImageResource();
      void loadVideoResource();
    };
    window.addEventListener('focus', refreshRuntimeMediaProviders);
    return () => window.removeEventListener('focus', refreshRuntimeMediaProviders);
  }, [loadImageResource, loadVideoResource, loadSpeechResource]);

  const refreshImage = useCallback(() => loadImageResource(), [loadImageResource]);
  const refreshVideo = useCallback(() => loadVideoResource(), [loadVideoResource]);
  const refreshSpeech = useCallback(() => loadSpeechResource(), [loadSpeechResource]);
  const refreshAll = useCallback(async () => {
    await Promise.allSettled([loadImageResource(), loadVideoResource(), loadSpeechResource()]);
  }, [loadImageResource, loadVideoResource, loadSpeechResource]);

  const saveImageConfig = useCallback((updates: MediaConfigUpdater): Promise<void> => {
    const write = async () => {
      const patch = resolveMediaConfigUpdater(updates, imageConfigRef.current);
      try {
        const res = await lingxiFetch('/api/media/image/config', {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ values: encodeConfigPatch(patch) }),
        });
        const data = await res.json().catch(() => null);
        applyImageConfig(data?.values ? data.values : applyConfigPatch(imageConfigRef.current, patch));
        showToast(t('settings.saved'), 'success');
      } catch (err: any) {
        showToast(err?.message || 'Save failed', 'error');
      }
    };
    const queued = imageWriteQueueRef.current.then(write, write);
    imageWriteQueueRef.current = queued;
    return queued;
  }, [applyImageConfig, showToast]);

  const saveVideoConfig = useCallback((updates: MediaConfigUpdater): Promise<void> => {
    const write = async () => {
      const patch = resolveMediaConfigUpdater(updates, videoConfigRef.current);
      try {
        const res = await lingxiFetch('/api/media/video/config', {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ values: encodeConfigPatch(patch) }),
        });
        const data = await res.json().catch(() => null);
        applyVideoConfig(data?.values ? data.values : applyConfigPatch(videoConfigRef.current, patch));
        showToast(t('settings.saved'), 'success');
      } catch (err: any) {
        showToast(err?.message || 'Save failed', 'error');
      }
    };
    const queued = videoWriteQueueRef.current.then(write, write);
    videoWriteQueueRef.current = queued;
    return queued;
  }, [applyVideoConfig, showToast]);

  const saveSpeechConfig = useCallback((updates: SpeechConfigUpdater): Promise<void> => {
    const write = async () => {
      const patch = resolveMediaConfigUpdater(updates, speechConfigRef.current);
      try {
        const res = await lingxiFetch('/api/speech-recognition/config', {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ values: encodeSpeechConfigPatch(patch) }),
        });
        const data = await res.json().catch(() => null);
        const base = speechConfigRef.current || { enabled: false };
        const next = data?.config
          ? mergeSpeechConfig(base, data.config)
          : data?.values
            ? mergeSpeechConfig(base, data.values)
            : applySpeechConfigPatch(base, patch);
        applySpeechConfig(next);
        showToast(t('settings.saved'), 'success');
      } catch (err: any) {
        showToast(err?.message || 'Save failed', 'error');
      }
    };
    const queued = speechWriteQueueRef.current.then(write, write);
    speechWriteQueueRef.current = queued;
    return queued;
  }, [applySpeechConfig, showToast]);

  const allImageModels = Object.keys(image.providers).flatMap(pid =>
    (image.providers[pid].models || []).map(m => ({ ...m, provider: pid })),
  );
  const allVideoModels = Object.keys(video.providers).flatMap(pid =>
    (video.providers[pid].models || []).map(m => ({ ...m, provider: pid })),
  );
  const allSpeechModels = Object.keys(speech.providers).flatMap(pid =>
    getRunnableSpeechModels(speech.providers[pid]).map(m => ({ ...m, provider: pid })),
  );
  const speechEnabled = speech.config?.enabled === true;

  return {
    image,
    video,
    speech,
    allImageModels,
    allVideoModels,
    allSpeechModels,
    speechEnabled,
    refreshImage,
    refreshVideo,
    refreshSpeech,
    refreshAll,
    saveImageConfig,
    saveVideoConfig,
    saveSpeechConfig,
  };
}

export { LOADING_SELECT_VALUE };
