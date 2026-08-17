/**
 * unified-models.ts — 供应商详情页「已添加的模型」统一 ViewModel
 *
 * 把 chat / image / video / speech recognition 四类模型合并成一个展示层列表。
 * 只统一 View/UI：底层数据存储、runtime、protocol、credential lane 与 API
 * 仍然各归各（chat 走 provider catalog，媒体走 media/speech endpoint）。
 *
 * 唯一 key：`kind:runtimeProviderId:id`。不能只用 kind:id——媒体 binding 具有
 * 独立 runtime_provider_id，不同 runtime provider 中允许存在相同模型 ID。
 */

import type { ProviderSummary, ProviderMediaCapabilityBinding } from '../../store';
import type {
  MediaProvider,
  MediaConfig,
  SpeechProvider,
  SpeechConfig,
  UseMediaSettingsDataResult,
} from '../../hooks/useMediaSettingsData';
import { lookupModelMeta } from '../../helpers';

export type UnifiedModelKind = 'chat' | 'image' | 'video' | 'speech';
export type Modality = 'text' | 'image' | 'video' | 'audio';

export const MODALITY_ORDER: Modality[] = ['text', 'image', 'video', 'audio'];

export interface UnifiedModelItem {
  key: string;
  kind: UnifiedModelKind;
  ownerProviderId: string;
  runtimeProviderId: string;
  id: string;
  displayName: string;
  inputs: Modality[];
  outputs: Modality[];
  context?: number;
  isDefault?: boolean;
  editable: boolean;
  removable: boolean;
  runtimeDiscovered: boolean;
  /** 该行的真实数据在 chat 槽（provider.models）而非媒体目录：移除必须走 config 而不是媒体 DELETE。 */
  claimedFromChat?: boolean;
  sourceModel: unknown;
}

export const KIND_DEFAULT_INPUTS: Record<UnifiedModelKind, Modality[]> = {
  chat: ['text'],
  image: ['text'],
  video: ['text'],
  speech: ['audio'],
};

export const KIND_DEFAULT_OUTPUTS: Record<UnifiedModelKind, Modality[]> = {
  chat: ['text'],
  image: ['image'],
  video: ['video'],
  speech: ['text'],
};

function isModality(value: unknown): value is Modality {
  return typeof value === 'string' && (MODALITY_ORDER as string[]).includes(value);
}

/** 宽松读取合法模态数组（去重 + canonical 排序）；非法返回 null。 */
export function readModalityList(value: unknown): Modality[] | null {
  if (!Array.isArray(value) || value.length === 0) return null;
  const seen = new Set<string>();
  for (const item of value) {
    if (!isModality(item)) return null;
    seen.add(item);
  }
  return MODALITY_ORDER.filter((modality) => seen.has(modality));
}

/** legacy image/vision/video/audio 布尔 → inputs（无任何定义时返回 null）。 */
export function inputsFromLegacyFlags(flags: {
  image?: unknown;
  vision?: unknown;
  video?: unknown;
  audio?: unknown;
}): Modality[] | null {
  const { image, vision, video, audio } = flags;
  if (image === undefined && vision === undefined && video === undefined && audio === undefined) {
    return null;
  }
  const enabled = new Set<Modality>(['text']);
  if (image === true || vision === true) enabled.add('image');
  if (video === true) enabled.add('video');
  if (audio === true) enabled.add('audio');
  return MODALITY_ORDER.filter((modality) => enabled.has(modality));
}

function numberFromMeta(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

type ProviderModelEntry = string | { id: string; [key: string]: unknown };

function modelIdOf(model: ProviderModelEntry): string {
  return typeof model === 'object' ? model.id : model;
}

function unifiedKey(kind: UnifiedModelKind, runtimeProviderId: string, id: string): string {
  return `${kind}:${runtimeProviderId}:${id}`;
}

export function buildChatUnifiedItems(providerId: string, summary: ProviderSummary): UnifiedModelItem[] {
  const rawModels: ProviderModelEntry[] = (summary.models || []) as ProviderModelEntry[];
  return rawModels.map((raw) => {
    const id = modelIdOf(raw);
    const entryMeta: Record<string, unknown> = raw && typeof raw === 'object' ? raw : {};
    const knownMeta: Record<string, any> = (lookupModelMeta(id, providerId) as Record<string, any>) || {};
    const merged: Record<string, any> = { ...knownMeta, ...entryMeta };
    const inputs = readModalityList(entryMeta.inputs)
      ?? readModalityList(knownMeta.inputs)
      ?? inputsFromLegacyFlags(merged)
      ?? KIND_DEFAULT_INPUTS.chat;
    const outputs = readModalityList(entryMeta.outputs)
      ?? readModalityList(knownMeta.outputs)
      ?? KIND_DEFAULT_OUTPUTS.chat;
    const context = numberFromMeta(entryMeta.context)
      ?? numberFromMeta(entryMeta.contextWindow)
      ?? numberFromMeta(knownMeta.context)
      ?? numberFromMeta(knownMeta.contextWindow);
    return {
      key: unifiedKey('chat', providerId, id),
      kind: 'chat' as const,
      ownerProviderId: providerId,
      runtimeProviderId: providerId,
      id,
      // 用户保存的 name 优先于 known catalog，避免 catalog 旧值遮蔽用户编辑
      displayName: String(entryMeta.displayName || entryMeta.name || knownMeta.displayName || knownMeta.name || id),
      inputs,
      outputs,
      context,
      editable: true,
      removable: true,
      runtimeDiscovered: false,
      sourceModel: raw,
    };
  });
}

function isMediaDefault(config: MediaConfig | null | undefined, capability: 'image' | 'video', runtimeProviderId: string, modelId: string): boolean {
  const key = capability === 'video' ? 'defaultVideoModel' : 'defaultImageModel';
  const defaultModel = config?.[key];
  return defaultModel?.id === modelId && defaultModel?.provider === runtimeProviderId;
}

function typedMediaKind(type: unknown): 'image' | 'video' | 'audio' | null {
  if (type === 'image') return 'image';
  if (type === 'video') return 'video';
  if (type === 'audio' || type === 'speech' || type === 'asr') return 'audio';
  return null;
}

/**
 * 知名媒体生成家族的 id 模式（生成语义 token，非输入语义）。
 * 只收「生成模型」的家族名/专用词，避免误伤 vision/多模态 chat 模型：
 * qwen2-audio（音频理解对话模型）不含 asr token → 仍是 chat；
 * gpt-4o / qwen-vl 这类命名不命中任何模式 → 仍是 chat。
 */
const MEDIA_ID_FAMILY_PATTERNS: Array<[RegExp, 'image' | 'video' | 'audio']> = [
  [/gpt-image|dall-e|cogview|seedream|imagen|imagegen|stable-diffusion|ideogram|recraft|qwen-image|wan[\d.]*-image|z-image|(?:^|[-_.])t2i(?:$|[-_.])|(?:^|[-_.])flux(?:$|[-_.])/i, 'image'],
  [/(?:^|[-_.])sora(?:$|[-_.])|seedance|(?:^|[-_.])kling|hailuo|(?:^|[-_.])vidu(?:$|[-_.])|pixverse|runway|luma[-_]?(?:dream|ray)|qwen-video|wan[\d.]*-video|videox|(?:^|[-_.])t2v(?:$|[-_.])|(?:^|[-_.])i2v(?:$|[-_.])/i, 'video'],
  [/whisper|paraformer|sensevoice|fun-asr|qwen-audio-transcribe|qwen-audio[.\d-]*realtime|livetranslate|(?:^|[-_./])asr(?:$|[-_.])|(?:^|[-_./])transcribe(?:$|[-_.])|(?:^|[-_./])speech(?:$|[-_.])|(?:^|[-_./])tts(?:$|[-_.])/i, 'audio'],
];

/**
 * chat 槽条目的媒体类别判定（与后端 getModelType 的分类维度对齐 + 家族模式兜底）：
 * 1. 显式 outputs 不含 text 且含 image/video → 纯媒体输出；
 * 2. 条目 type 字段（早期版本的类别标记）；
 * 3. known-models 词典的 type（后端 normalizeUserMediaModels 就是按它把
 *    chat 槽模型并入媒体目录的；前端不认就会双行重复渲染）；
 * 4. 知名媒体生成家族的 id 模式（词典没收录的新变体/带日期后缀的模型，
 *    如 dashscope 的 qwen-image-3.0、qwen-audio-3.0-asr-flash）。
 *
 * 返回 'audio' 表示语音识别家族（不属于 chat；语音类模型只从语音目录添加）。
 * 注意：条目的 image/vision/video 布尔不是媒体证据——compactDiscoveredModelEntry
 * 写入这些布尔表示「视觉/视频输入能力」（多模态 chat 模型也有），与
 * inputsFromLegacyFlags 的输入语义一致；把它们当生成语义会把 gpt-4o 这类
 * vision chat 模型错认成图片生成模型。
 */
export function chatEntryMediaKind(
  id: string,
  raw: Record<string, unknown>,
  providerId: string,
): 'image' | 'video' | 'audio' | null {
  const outputs = readModalityList(raw.outputs);
  if (outputs) {
    if (outputs.includes('text')) return null;
    if (outputs.includes('image')) return 'image';
    if (outputs.includes('video')) return 'video';
    return null;
  }
  const entryType = typedMediaKind(raw.type);
  if (entryType) return entryType;
  const knownType = typedMediaKind(lookupModelMeta(id, providerId)?.type);
  if (knownType) return knownType;
  for (const [pattern, kind] of MEDIA_ID_FAMILY_PATTERNS) {
    if (pattern.test(id)) return kind;
  }
  return null;
}

/**
 * 媒体 binding 的「已添加」模型列表。
 *
 * 生效模型有两个来源：
 * 1. 媒体目录（registry.updateMediaModelEntry 的用户 overlay）——正常路径；
 * 2. chat 槽误存的纯媒体模型（见 chatEntryMediaKind）——其 id 不在媒体目录
 *    时才认领进该类别，让统一列表与默认参数按钮都能看到这个模型。
 *
 * runtime-discovered binding（provider.runtimeCapability 存在且非 pending）
 * 的目录由 runtime 拥有，chat 槽同 id 条目是用户自己加的 chat 模型，不认领。
 *
 * 返回携带 claimedFromChat：认领行的真实数据在 chat 槽，删除/编辑必须回写
 * provider.models，走媒体 DELETE 会变成假成功。
 */
export function collectMediaAddedModels(
  binding: ProviderMediaCapabilityBinding,
  provider: MediaProvider | SpeechProvider,
  chatEntries: Array<{ id: string; raw: Record<string, unknown> }>,
): Array<{ model: Record<string, unknown>; claimedFromChat: boolean }> {
  const runtimeManaged = !!((provider as MediaProvider).runtimeCapability?.status
    && (provider as MediaProvider).runtimeCapability?.status !== 'pending');
  const seen = new Set<string>();
  const collected: Array<{ model: Record<string, unknown>; claimedFromChat: boolean }> = [];
  for (const model of provider.models || []) {
    if (!model || seen.has(model.id)) continue;
    seen.add(model.id);
    collected.push({ model: model as unknown as Record<string, unknown>, claimedFromChat: false });
  }
  const kind = binding.capability === 'videoGeneration' ? 'video'
    : binding.capability === 'imageGeneration' ? 'image' : null;
  if (!kind || runtimeManaged) return collected;
  for (const entry of chatEntries) {
    if (seen.has(entry.id)) continue;
    if (chatEntryMediaKind(entry.id, entry.raw, binding.runtime_provider_id) !== kind) continue;
    seen.add(entry.id);
    collected.push({ model: entry.raw, claimedFromChat: true });
  }
  return collected;
}

export function buildMediaUnifiedItems({
  bindings,
  media,
  chatEntries = [],
}: {
  bindings?: ProviderMediaCapabilityBinding[];
  media: UseMediaSettingsDataResult;
  chatEntries?: Array<{ id: string; raw: Record<string, unknown> }>;
}): UnifiedModelItem[] {
  const items: UnifiedModelItem[] = [];
  for (const binding of bindings || []) {
    if (binding.capability === 'imageGeneration' || binding.capability === 'videoGeneration') {
      const kind: 'image' | 'video' = binding.capability === 'videoGeneration' ? 'video' : 'image';
      const resource = kind === 'video' ? media.video : media.image;
      const provider = resource.providers[binding.runtime_provider_id] as MediaProvider | undefined;
      if (!provider) continue;
      const runtimeDiscovered = !!provider.runtimeCapability
        && provider.runtimeCapability.status !== 'pending';
      for (const { model, claimedFromChat } of collectMediaAddedModels(binding, provider, chatEntries)) {
        items.push({
          key: unifiedKey(kind, binding.runtime_provider_id, String(model.id)),
          kind,
          ownerProviderId: binding.runtime_provider_id,
          runtimeProviderId: binding.runtime_provider_id,
          id: String(model.id),
          displayName: String(model.displayName || model.name || model.id),
          inputs: readModalityList(model.inputs) ?? KIND_DEFAULT_INPUTS[kind],
          outputs: readModalityList(model.outputs) ?? KIND_DEFAULT_OUTPUTS[kind],
          isDefault: isMediaDefault(resource.config as MediaConfig | null, kind, binding.runtime_provider_id, String(model.id)),
          editable: !runtimeDiscovered,
          removable: !runtimeDiscovered,
          runtimeDiscovered,
          claimedFromChat,
          sourceModel: model,
        });
      }
    } else if (binding.capability === 'speechRecognition') {
      const resource = media.speech;
      const provider = resource.providers[binding.runtime_provider_id] as SpeechProvider | undefined;
      if (!provider) continue;
      const config = resource.config as SpeechConfig | null;
      for (const model of (provider.models || []).filter(m => m.adapterAvailable !== false)) {
        const speechModel = model as unknown as Record<string, unknown>;
        items.push({
          key: unifiedKey('speech', binding.runtime_provider_id, model.id),
          kind: 'speech' as const,
          ownerProviderId: binding.runtime_provider_id,
          runtimeProviderId: binding.runtime_provider_id,
          id: model.id,
          displayName: String(model.displayName || model.name || model.id),
          inputs: readModalityList(speechModel.inputs) ?? KIND_DEFAULT_INPUTS.speech,
          outputs: readModalityList(speechModel.outputs) ?? KIND_DEFAULT_OUTPUTS.speech,
          isDefault: config?.defaultModel?.id === model.id
            && config?.defaultModel?.provider === binding.runtime_provider_id,
          editable: true,
          removable: true,
          runtimeDiscovered: false,
          sourceModel: model,
        });
      }
    }
  }
  return items;
}

/**
 * 被媒体绑定认领的 chat 槽纯媒体模型 id（用于从 chat 列表里移除，避免重复展示）。
 *
 * id 已在媒体目录中的纯媒体条目也要认领：否则同一模型会以 chat 行 + 媒体行
 * 重复出现（后端 normalizeUserMediaModels 也会把词典 type=image 的 chat 槽
 * 条目并入媒体目录）。显式含 text 输出的条目永远不会被认领（chat 领域）。
 *
 * runtime 发现型供应商的目录由 runtime 拥有：只在与 runtime 快照同 id 时认领
 * （纯去重），否则该模型只存在于 chat 槽，认领会把它从列表里整个藏掉。
 */
function claimedChatMediaIds(
  bindings: ProviderMediaCapabilityBinding[] | undefined,
  media: UseMediaSettingsDataResult,
  chatEntries: Array<{ id: string; raw: Record<string, unknown> }>,
): Set<string> {
  const claimed = new Set<string>();
  for (const binding of bindings || []) {
    const kind = binding.capability === 'videoGeneration' ? 'video'
      : binding.capability === 'imageGeneration' ? 'image' : null;
    if (!kind) continue;
    const resource = kind === 'video' ? media.video : media.image;
    const provider = resource.providers[binding.runtime_provider_id] as MediaProvider | undefined;
    if (!provider) continue;
    const runtimeManaged = !!provider.runtimeCapability?.status
      && provider.runtimeCapability.status !== 'pending';
    const catalogIds = new Set((provider.models || []).map(m => m.id));
    for (const entry of chatEntries) {
      // 显式含 text 输出的条目永远属于 chat 领域
      if (readModalityList(entry.raw.outputs)?.includes('text')) continue;
      if (chatEntryMediaKind(entry.id, entry.raw, binding.runtime_provider_id) !== kind) {
        // 未被分类命中的条目：同 id 已在媒体目录时也认领（该 id 的媒体行一定会
        // 渲染，chat 行不认领就是双行重复）；不在目录则保持 chat 行
        if (!catalogIds.has(entry.id)) continue;
      }
      if (runtimeManaged && !catalogIds.has(entry.id)) continue;
      claimed.add(entry.id);
    }
  }
  return claimed;
}

export function buildUnifiedModelItems({
  providerId,
  summary,
  media,
}: {
  providerId: string;
  summary: ProviderSummary;
  media: UseMediaSettingsDataResult;
}): UnifiedModelItem[] {
  // chat 槽条目同时提供给媒体认领：早期版本误把图片/视频生成模型写进
  // provider.models，这些「纯媒体模型」会在统一列表里按媒体类别展示，
  // 并从 chat 列表移除，避免同一模型以 chat 和媒体两行重复出现。
  const chatEntries = ((summary.models || []) as ProviderModelEntry[])
    .map((raw) => ({ id: modelIdOf(raw), raw: raw && typeof raw === 'object' ? raw as Record<string, unknown> : { id: modelIdOf(raw) } }));
  const claimed = claimedChatMediaIds(summary.media_capability_bindings, media, chatEntries);
  return [
    ...buildChatUnifiedItems(providerId, summary).filter(item => !claimed.has(item.id)),
    ...buildMediaUnifiedItems({ bindings: summary.media_capability_bindings, media, chatEntries }),
  ];
}

/** 该类别当前已添加的模型数量（默认参数按钮的显示条件）。 */
export function countAddedByKind(items: UnifiedModelItem[]): Record<UnifiedModelKind, number> {
  const counts: Record<UnifiedModelKind, number> = { chat: 0, image: 0, video: 0, speech: 0 };
  for (const item of items) counts[item.kind] += 1;
  return counts;
}
