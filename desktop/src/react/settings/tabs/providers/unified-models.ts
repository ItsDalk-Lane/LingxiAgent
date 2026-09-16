/**
 * unified-models.ts — 供应商详情页「已添加的模型」统一 ViewModel
 *
 * 把对话、图片、视频、语音合成和语音识别合并成一个展示层列表。
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
import { mediaModelCapabilities, isMediaOnlyModel } from '../../../../../../shared/media-model-classification';

export type UnifiedModelKind = 'chat' | 'image' | 'video' | 'speech' | 'speechGen';
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
  speechGen: ['text'],
};

export const KIND_DEFAULT_OUTPUTS: Record<UnifiedModelKind, Modality[]> = {
  chat: ['text'],
  image: ['image'],
  video: ['video'],
  speech: ['text'],
  speechGen: ['audio'],
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

function isMediaDefault(config: MediaConfig | null | undefined, capability: 'image' | 'video' | 'speechGen', runtimeProviderId: string, modelId: string): boolean {
  const key = capability === 'video' ? 'defaultVideoModel' : capability === 'speechGen' ? 'defaultSpeechModel' : 'defaultImageModel';
  const defaultModel = config?.[key];
  return defaultModel?.id === modelId && defaultModel?.provider === runtimeProviderId;
}

/** 复用前后端统一的能力分类，仅纯媒体模型从聊天列表移出。 */
export function chatEntryMediaKind(id: string, raw: Record<string, unknown>, providerId: string): 'image' | 'video' | 'audio' | null {
  const model = { ...raw, id };
  const known = lookupModelMeta(id, providerId);
  if (!isMediaOnlyModel(model, known)) return null;
  const capabilities = mediaModelCapabilities(model, known);
  if (capabilities.includes('imageGeneration')) return 'image';
  if (capabilities.includes('videoGeneration')) return 'video';
  return capabilities.length ? 'audio' : null;
}

export function kindForCapability(capability: ProviderMediaCapabilityBinding['capability']): Exclude<UnifiedModelKind, 'chat'> {
  if (capability === 'imageGeneration') return 'image';
  if (capability === 'videoGeneration') return 'video';
  return capability === 'speechGeneration' ? 'speechGen' : 'speech';
}

/**
 * 媒体 binding 的「已添加」模型列表。
 *
 * 生效模型有两个来源：
 * 1. 媒体目录（registry.updateMediaModelEntry 的用户 overlay）——正常路径；
 * 2. chat 槽误存的纯媒体模型（见 chatEntryMediaKind）——其 id 不在媒体目录
 *    时才认领进该类别，让统一列表与默认参数按钮都能看到这个模型。
 *
 * runtime-discovered binding（provider.runtimeCapability 存在）
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
  const runtimeManaged = !!(provider as MediaProvider).runtimeCapability;
  const seen = new Set<string>();
  const collected: Array<{ model: Record<string, unknown>; claimedFromChat: boolean }> = [];
  for (const model of provider.models || []) {
    if (!model || seen.has(model.id)) continue;
    seen.add(model.id);
    collected.push({ model: model as unknown as Record<string, unknown>, claimedFromChat: (model as unknown as Record<string, unknown>).claimedFromChat === true });
  }
  if (runtimeManaged) return collected;
  for (const entry of chatEntries) {
    if (seen.has(entry.id)) continue;
    if (!mediaModelCapabilities({ ...entry.raw, id: entry.id }, lookupModelMeta(entry.id, binding.runtime_provider_id)).includes(binding.capability)) continue;
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
    if (binding.capability === 'imageGeneration' || binding.capability === 'videoGeneration' || binding.capability === 'speechGeneration') {
      const kind = kindForCapability(binding.capability) as 'image' | 'video' | 'speechGen';
      const resource = media[kind];
      const provider = resource.providers[binding.runtime_provider_id] as MediaProvider | undefined;
      if (!provider) continue;
      const runtimeDiscovered = !!provider.runtimeCapability;
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
      for (const { model: speechModel, claimedFromChat } of collectMediaAddedModels(binding, provider, chatEntries)) {
        const model = speechModel as unknown as { id: string; name?: string; displayName?: string };
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
          claimedFromChat,
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
 * 条目并入媒体目录）。混合文本输出保留聊天入口；专门的语音识别模型仍进入识别类别。
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
    const kind = kindForCapability(binding.capability);
    const provider = media[kind].providers[binding.runtime_provider_id];
    if (!provider) continue;
    const runtimeManaged = !!(provider as MediaProvider).runtimeCapability;
    const catalogIds = new Set((provider.models || []).map(m => m.id));
    for (const entry of chatEntries) {
      const known = lookupModelMeta(entry.id, binding.runtime_provider_id);
      if (!isMediaOnlyModel({ ...entry.raw, id: entry.id }, known)) continue;
      if (!mediaModelCapabilities({ ...entry.raw, id: entry.id }, known).includes(binding.capability) && !catalogIds.has(entry.id)) continue;
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
  const counts: Record<UnifiedModelKind, number> = { chat: 0, image: 0, video: 0, speech: 0, speechGen: 0 };
  for (const item of items) counts[item.kind] += 1;
  return counts;
}
