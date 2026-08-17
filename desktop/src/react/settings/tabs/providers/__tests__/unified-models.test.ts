/**
 * unified-models 纯函数测试
 *
 * @vitest-environment jsdom
 */

import { describe, expect, it, vi, beforeEach } from 'vitest';

const lookupModelMetaMock = vi.hoisted(() => vi.fn((..._args: unknown[]): unknown => null));

vi.mock('../../../helpers', () => ({
  lookupModelMeta: (...args: unknown[]) => lookupModelMetaMock(...args),
}));

import {
  buildUnifiedModelItems,
  buildChatUnifiedItems,
  buildMediaUnifiedItems,
  chatEntryMediaKind,
  countAddedByKind,
  readModalityList,
  inputsFromLegacyFlags,
  type UnifiedModelItem,
} from '../unified-models';
import type { ProviderSummary } from '../../../store';

function mediaResource(providers: Record<string, any> = {}, config: any = null) {
  return { providers, config, loading: false, error: null };
}

function makeMedia({
  image = {},
  video = {},
  speech = {},
}: Record<string, any> = {}) {
  return {
    image: mediaResource(image.providers ?? image, image.config ?? null),
    video: mediaResource(video.providers ?? video, video.config ?? null),
    speech: mediaResource(speech.providers ?? speech, speech.config ?? null),
    allImageModels: [],
    allVideoModels: [],
    allSpeechModels: [],
    speechEnabled: false,
    refreshImage: vi.fn(),
    refreshVideo: vi.fn(),
    refreshSpeech: vi.fn(),
    refreshAll: vi.fn(),
    saveImageConfig: vi.fn(),
    saveVideoConfig: vi.fn(),
    saveSpeechConfig: vi.fn(),
  } as any;
}

describe('readModalityList', () => {
  it('normalizes dedupe and canonical order', () => {
    expect(readModalityList(['audio', 'text', 'image', 'text'])).toEqual(['text', 'image', 'audio']);
    expect(readModalityList(['video', 'text'])).toEqual(['text', 'video']);
  });

  it('rejects invalid shapes and unknown members', () => {
    expect(readModalityList('text')).toBeNull();
    expect(readModalityList([])).toBeNull();
    expect(readModalityList(['text', 'hologram'])).toBeNull();
    expect(readModalityList(null)).toBeNull();
  });
});

describe('chatEntryMediaKind', () => {
  beforeEach(() => {
    lookupModelMetaMock.mockImplementation(() => null);
  });

  it('classifies pure media outputs and keeps text-output models in the chat domain', () => {
    expect(chatEntryMediaKind('m', { outputs: ['image'] }, 'prov')).toBe('image');
    expect(chatEntryMediaKind('m', { outputs: ['video'] }, 'prov')).toBe('video');
    expect(chatEntryMediaKind('m', { outputs: ['text'] }, 'prov')).toBeNull();
    // 多模态 chat 模型（文本+图片输出）仍属 chat 领域
    expect(chatEntryMediaKind('m', { outputs: ['text', 'image'] }, 'prov')).toBeNull();
  });

  it('recognizes the legacy type field and the known-models dictionary type', () => {
    expect(chatEntryMediaKind('m', { type: 'image' }, 'prov')).toBe('image');
    expect(chatEntryMediaKind('m', { type: 'video' }, 'prov')).toBe('video');
    lookupModelMetaMock.mockImplementation((id: unknown) => (
      id === 'gpt-image-1' ? { type: 'image' } : null
    ));
    expect(chatEntryMediaKind('gpt-image-1', { id: 'gpt-image-1' }, 'openai')).toBe('image');
    expect(chatEntryMediaKind('gpt-4o', { id: 'gpt-4o' }, 'openai')).toBeNull();
  });

  it('does not treat vision/video input booleans as media-generation evidence', () => {
    // image/vision/video 布尔是输入能力（compactDiscoveredModelEntry / inputsFromLegacyFlags
    // 的语义），gpt-4o 这类 vision chat 模型不能被错认成图片生成模型
    expect(chatEntryMediaKind('m', { image: true, vision: true, video: true }, 'prov')).toBeNull();
  });

  it('classifies well-known media generation families by id pattern (dictionary-free fallback)', () => {
    // 词典没收录的新变体/带日期的模型靠家族模式识别，所有供应商统一生效
    expect(chatEntryMediaKind('qwen-image-3.0', {}, 'dashscope')).toBe('image');
    expect(chatEntryMediaKind('qwen-image-2.0-pro-2026-06-22', {}, 'dashscope')).toBe('image');
    expect(chatEntryMediaKind('wan2.7-image', {}, 'dashscope')).toBe('image');
    expect(chatEntryMediaKind('cogview-4', {}, 'zhipu')).toBe('image');
    expect(chatEntryMediaKind('z-image-turbo', {}, 'zhipu')).toBe('image');
    expect(chatEntryMediaKind('doubao-seedream-9-0-270101', {}, 'volcengine')).toBe('image');
    expect(chatEntryMediaKind('foo-t2i-bar', {}, 'any')).toBe('image');
    expect(chatEntryMediaKind('wan2.7-video', {}, 'dashscope')).toBe('video');
    expect(chatEntryMediaKind('sora-2', {}, 'openai')).toBe('video');
    expect(chatEntryMediaKind('kling-v2', {}, 'custom')).toBe('video');
    expect(chatEntryMediaKind('qwen-audio-3.0-asr-flash', {}, 'dashscope')).toBe('audio');
    expect(chatEntryMediaKind('qwen-audio-3.0-realtime-plus', {}, 'dashscope')).toBe('audio');
    expect(chatEntryMediaKind('MiniMax/speech-2.8-hd', {}, 'dashscope')).toBe('audio');
    expect(chatEntryMediaKind('fun-asr-flash-2026-06-15', {}, 'dashscope')).toBe('audio');
    expect(chatEntryMediaKind('whisper-large-v3', {}, 'any')).toBe('audio');
    expect(chatEntryMediaKind('qwen3-tts-instruct-flash-realtime', {}, 'dashscope')).toBe('audio');
    expect(chatEntryMediaKind('qwen3.5-livetranslate-flash-realtime', {}, 'dashscope')).toBe('audio');
  });

  it('keeps multimodal-understanding chat models in the chat domain', () => {
    // 音频/图像「理解」类对话模型不含生成语义 token，不能被家族模式误伤
    expect(chatEntryMediaKind('qwen2-audio', {}, 'dashscope')).toBeNull();
    expect(chatEntryMediaKind('qwen-vl-max', {}, 'dashscope')).toBeNull();
    expect(chatEntryMediaKind('gpt-4o', {}, 'openai')).toBeNull();
    expect(chatEntryMediaKind('deepseek-v4-pro-0813', {}, 'dashscope')).toBeNull();
    // 显式文本输出永远豁免
    expect(chatEntryMediaKind('qwen-image-3.0', { outputs: ['text', 'image'] }, 'dashscope')).toBeNull();
  });
});

describe('inputsFromLegacyFlags', () => {
  it('returns null when no legacy flag is defined', () => {
    expect(inputsFromLegacyFlags({})).toBeNull();
    expect(inputsFromLegacyFlags({ image: undefined, video: undefined, audio: undefined })).toBeNull();
  });

  it('unions enabled flags with the implicit text baseline', () => {
    expect(inputsFromLegacyFlags({ image: true, video: true })).toEqual(['text', 'image', 'video']);
    expect(inputsFromLegacyFlags({ vision: true })).toEqual(['text', 'image']);
    expect(inputsFromLegacyFlags({ image: false, video: true, audio: true })).toEqual(['text', 'video', 'audio']);
  });
});

describe('buildChatUnifiedItems', () => {
  const summary = {
    models: [
      'bare-id',
      { id: 'with-meta', name: 'With Meta', context: 131072 },
      { id: 'canonical', inputs: ['text', 'audio'] },
      { id: 'legacy', image: true, video: true },
    ],
  } as unknown as ProviderSummary;

  it('upgrades bare strings and resolves modalities per entry', () => {
    const items = buildChatUnifiedItems('prov', summary);
    expect(items.map(i => [i.kind, i.id, i.inputs, i.outputs, i.context])).toEqual([
      ['chat', 'bare-id', ['text'], ['text'], undefined],
      ['chat', 'with-meta', ['text'], ['text'], 131072],
      ['chat', 'canonical', ['text', 'audio'], ['text'], undefined],
      ['chat', 'legacy', ['text', 'image', 'video'], ['text'], undefined],
    ]);
    expect(items.every(i => i.editable && i.removable && !i.runtimeDiscovered)).toBe(true);
  });
});

describe('buildMediaUnifiedItems', () => {
  it('collects image/video/speech models per runtime binding with kind defaults', () => {
    const media = makeMedia({
      image: { providers: { dashscope: {
        providerId: 'dashscope', hasCredentials: true,
        models: [
          { id: 'wan-image-x', displayName: 'Wan Image Pro', inputs: ['text'], outputs: ['image'] },
          { id: 'legacy-image-no-modalities' },
        ],
        availableModels: [],
      } } },
      video: { providers: { agnes: {
        providerId: 'agnes', hasCredentials: true,
        models: [{ id: 'video-x', outputs: ['video'] }],
        availableModels: [],
      } } },
      speech: { providers: { 'volcengine-speech': {
        providerId: 'volcengine-speech', hasCredentials: true,
        models: [
          { id: 'whisper-x', inputs: ['audio'], outputs: ['text'] },
          { id: 'adapter-missing', adapterAvailable: false },
        ],
        availableModels: [],
        catalogModels: [],
      } } },
    });
    const items = buildMediaUnifiedItems({
      bindings: [
        { capability: 'imageGeneration', runtime_provider_id: 'dashscope' },
        { capability: 'videoGeneration', runtime_provider_id: 'agnes' },
        { capability: 'speechRecognition', runtime_provider_id: 'volcengine-speech' },
      ] as any,
      media,
    });
    expect(items.map(i => [i.kind, i.runtimeProviderId, i.id, i.inputs, i.outputs])).toEqual([
      ['image', 'dashscope', 'wan-image-x', ['text'], ['image']],
      ['image', 'dashscope', 'legacy-image-no-modalities', ['text'], ['image']],
      ['video', 'agnes', 'video-x', ['text'], ['video']],
      // adapterAvailable:false 的语音模型不进「已添加」列表
      ['speech', 'volcengine-speech', 'whisper-x', ['audio'], ['text']],
    ]);
  });

  it('marks runtime-discovered models read-only and flags defaults per runtime provider', () => {
    const media = makeMedia({
      image: {
        providers: { 'jimeng-cli': {
          providerId: 'jimeng-cli', hasCredentials: true, runtimeCapability: { status: 'ready' },
          models: [{ id: 'cli-image', outputs: ['image'] }],
          availableModels: [],
        } },
        config: { defaultImageModel: { id: 'cli-image', provider: 'jimeng-cli' } },
      },
    });
    const items = buildMediaUnifiedItems({
      bindings: [{ capability: 'imageGeneration', runtime_provider_id: 'jimeng-cli' }] as any,
      media,
    });
    expect(items[0].runtimeDiscovered).toBe(true);
    expect(items[0].editable).toBe(false);
    expect(items[0].removable).toBe(false);
    expect(items[0].isDefault).toBe(true);
  });

  it('allows identical model ids across kinds and runtime providers without key collision', () => {
    const media = makeMedia({
      image: { providers: {
        dashscope: { providerId: 'dashscope', hasCredentials: true, models: [{ id: 'shared', outputs: ['image'] }], availableModels: [] },
        agnes: { providerId: 'agnes', hasCredentials: true, models: [{ id: 'shared', outputs: ['image'] }], availableModels: [] },
      } },
      video: { providers: { agnes: {
        providerId: 'agnes', hasCredentials: true, models: [{ id: 'shared', outputs: ['video'] }], availableModels: [],
      } } },
    });
    const bindings = [
      { capability: 'imageGeneration', runtime_provider_id: 'dashscope' },
      { capability: 'imageGeneration', runtime_provider_id: 'agnes' },
      { capability: 'videoGeneration', runtime_provider_id: 'agnes' },
    ] as any;
    const items = buildMediaUnifiedItems({ bindings, media });
    const keys = items.map(i => i.key);
    expect(new Set(keys).size).toBe(keys.length);
    expect(keys).toEqual(['image:dashscope:shared', 'image:agnes:shared', 'video:agnes:shared']);
  });
});

describe('buildUnifiedModelItems + countAddedByKind', () => {
  it('orders chat first and counts kinds', () => {
    const media = makeMedia({
      image: { providers: { dashscope: { providerId: 'dashscope', hasCredentials: true, models: [{ id: 'i1', outputs: ['image'] }], availableModels: [] } } },
    });
    const items: UnifiedModelItem[] = buildUnifiedModelItems({
      providerId: 'prov',
      summary: {
        models: ['chat-1', 'chat-2'],
        media_capability_bindings: [{ capability: 'imageGeneration', runtime_provider_id: 'dashscope' }],
      } as unknown as ProviderSummary,
      media,
    });
    expect(items.map(i => i.kind)).toEqual(['chat', 'chat', 'image']);
    expect(countAddedByKind(items)).toEqual({ chat: 2, image: 1, video: 0, speech: 0 });
  });

  it('reclassifies a media model mis-stored in the chat slot into its media kind for counting', () => {
    // 早期版本把图片模型误写进 provider.models（chat 槽），媒体目录为空。
    // 统一列表应把它归类为 image，让「图片生成默认参数」按钮能据此显示。
    const media = makeMedia({
      image: { providers: { dashscope: { providerId: 'dashscope', hasCredentials: true, models: [], availableModels: [] } } },
    });
    const items: UnifiedModelItem[] = buildUnifiedModelItems({
      providerId: 'dashscope',
      summary: {
        models: [{ id: 'qwen-image-3.0-pro', outputs: ['image'], inputs: ['text', 'image'] }],
        media_capability_bindings: [{ capability: 'imageGeneration', runtime_provider_id: 'dashscope' }],
      } as unknown as ProviderSummary,
      media,
    });
    // 纯图片模型被认领进 image 后，不再以 chat 形式重复展示
    expect(items.map(i => [i.kind, i.id])).toEqual([
      ['image', 'qwen-image-3.0-pro'],
    ]);
    expect(countAddedByKind(items)).toEqual({ chat: 0, image: 1, video: 0, speech: 0 });
  });

  it('does not reclassify a plain chat model whose outputs default to text', () => {
    const media = makeMedia({
      image: { providers: { dashscope: { providerId: 'dashscope', hasCredentials: true, models: [], availableModels: [] } } },
    });
    const items: UnifiedModelItem[] = buildUnifiedModelItems({
      providerId: 'dashscope',
      summary: {
        models: ['qwen3.8-max'],
        media_capability_bindings: [{ capability: 'imageGeneration', runtime_provider_id: 'dashscope' }],
      } as unknown as ProviderSummary,
      media,
    });
    expect(items.map(i => i.kind)).toEqual(['chat']);
    expect(countAddedByKind(items).image).toBe(0);
  });

  it('claims a chat-slot model whose id already exists in the media catalog (no duplicate rows)', () => {
    // 后端 normalizeUserMediaModels 会把词典 type=image 的 chat 槽条目并入媒体目录；
    // 前端若因「已在目录中」而不认领，同一模型会渲染成 chat 行 + image 行两行
    lookupModelMetaMock.mockImplementation((id: unknown) => (
      id === 'gpt-image-1' ? { type: 'image' } : null
    ));
    const media = makeMedia({
      image: { providers: { openai: {
        providerId: 'openai', hasCredentials: true,
        models: [{ id: 'gpt-image-1', displayName: 'GPT Image 1', outputs: ['image'] }],
        availableModels: [],
      } } },
    });
    const items: UnifiedModelItem[] = buildUnifiedModelItems({
      providerId: 'openai',
      summary: {
        models: ['gpt-4o', 'gpt-image-1'],
        media_capability_bindings: [{ capability: 'imageGeneration', runtime_provider_id: 'openai' }],
      } as unknown as ProviderSummary,
      media,
    });
    expect(items.map(i => `${i.kind}:${i.id}`)).toEqual(['chat:gpt-4o', 'image:gpt-image-1']);
    // 目录里的条目不是认领行（删除走媒体端点）
    expect(items.find(i => i.id === 'gpt-image-1')?.claimedFromChat).toBe(false);
  });

  it('claims a dictionary-typed chat entry even when the media catalog has not loaded it yet', () => {
    lookupModelMetaMock.mockImplementation((id: unknown) => (
      id === 'gpt-image-1' ? { type: 'image' } : null
    ));
    const media = makeMedia({
      image: { providers: { openai: { providerId: 'openai', hasCredentials: true, models: [], availableModels: [] } } },
    });
    const items: UnifiedModelItem[] = buildUnifiedModelItems({
      providerId: 'openai',
      summary: {
        models: ['gpt-image-1'],
        media_capability_bindings: [{ capability: 'imageGeneration', runtime_provider_id: 'openai' }],
      } as unknown as ProviderSummary,
      media,
    });
    // 词典 type=image 的裸 chat 条目按 image 类别展示，且标记为 chat 槽认领行
    expect(items.map(i => `${i.kind}:${i.id}:${i.claimedFromChat}`)).toEqual(['image:gpt-image-1:true']);
  });

  it('dedupes a pure-media chat entry against a runtime-discovered catalog without hiding chat-only ids', () => {
    const media = makeMedia({
      image: { providers: { 'jimeng-cli': {
        providerId: 'jimeng-cli', hasCredentials: true,
        runtimeCapability: { status: 'ready' },
        models: [{ id: 'jimeng-image-shared', displayName: '即梦共享模型', outputs: ['image'] }],
        availableModels: [],
      } } },
    });
    const summary = {
      // chat 槽里有两个纯媒体条目：一个与 runtime 快照同 id（应去重），一个只在 chat 槽（必须保留展示）
      models: [
        { id: 'jimeng-image-shared', outputs: ['image'] },
        { id: 'jimeng-image-chat-only', outputs: ['image'] },
        'plain-chat-model',
      ],
      media_capability_bindings: [{ capability: 'imageGeneration', runtime_provider_id: 'jimeng-cli' }],
    } as unknown as ProviderSummary;
    const items: UnifiedModelItem[] = buildUnifiedModelItems({ providerId: 'jimeng-cli', summary, media });
    expect(items.map(i => `${i.kind}:${i.id}`)).toEqual([
      'chat:jimeng-image-chat-only',
      'chat:plain-chat-model',
      'image:jimeng-image-shared',
    ]);
  });
});
