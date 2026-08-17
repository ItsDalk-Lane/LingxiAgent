/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { useSettingsStore } from '../../../store';

const mocks = vi.hoisted(() => ({
  lingxiFetch: vi.fn(),
  lookupModelMeta: vi.fn((_id: unknown, _provider?: unknown): unknown => null),
}));

vi.mock('../../../api', () => ({
  lingxiFetch: (...args: unknown[]) => mocks.lingxiFetch(...args),
  lingxiFetchJson: async (...args: unknown[]) => {
    const response = await mocks.lingxiFetch(...args);
    const data = await response.json();
    if (data?.error) throw new Error(data.error);
    return data;
  },
}));

vi.mock('../../../../hooks/use-config', () => ({
  invalidateConfigCache: vi.fn(),
}));

vi.mock('../../../helpers', () => ({
  t: (key: string) => key,
  formatContext: (n: number) => `${n}`,
  lookupModelMeta: (id: unknown, provider?: unknown) => mocks.lookupModelMeta(id, provider),
  CONTEXT_PRESETS: [],
  OUTPUT_PRESETS: [],
}));

import { ProviderModelList } from '../ProviderModelList';

function jsonResponse(body: unknown): Response {
  return { json: async () => body } as Response;
}

function rect(init: Partial<DOMRect>): DOMRect {
  return {
    x: init.left ?? 0,
    y: init.top ?? 0,
    left: init.left ?? 0,
    top: init.top ?? 0,
    right: init.right ?? (init.left ?? 0) + (init.width ?? 0),
    bottom: init.bottom ?? (init.top ?? 0) + (init.height ?? 0),
    width: init.width ?? 0,
    height: init.height ?? 0,
    toJSON: () => ({}),
  } as DOMRect;
}

function mediaResource(providers: any = {}, config: any = null) {
  return { providers, config, loading: false, error: null } as any;
}

function makeMedia({
  imageProviders = {},
  imageConfig = null,
  videoProviders = {},
  videoConfig = null,
  speechProviders = {},
  speechConfig = null,
}: Record<string, any> = {}) {
  return {
    image: mediaResource(imageProviders, imageConfig),
    video: mediaResource(videoProviders, videoConfig),
    speech: mediaResource(speechProviders, speechConfig),
    allImageModels: [],
    allVideoModels: [],
    allSpeechModels: [],
    speechEnabled: false,
    refreshImage: vi.fn(async () => {}),
    refreshVideo: vi.fn(async () => {}),
    refreshSpeech: vi.fn(async () => {}),
    refreshAll: vi.fn(async () => {}),
    saveImageConfig: vi.fn(async () => {}),
    saveVideoConfig: vi.fn(async () => {}),
    saveSpeechConfig: vi.fn(async () => {}),
  } as any;
}

function chatSummary(overrides: Record<string, any> = {}): any {
  return {
    type: 'api-key',
    auth_type: 'api-key',
    display_name: 'Kimi Coding Plan',
    base_url: 'https://api.kimi.com/coding/',
    api: 'anthropic-messages',
    api_key: '',
    models: ['kimi-for-coding'],
    custom_models: [],
    has_credentials: false,
    supports_oauth: false,
    is_coding_plan: true,
    can_delete: false,
    ...overrides,
  };
}

describe('ProviderModelList (chat)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.lingxiFetch.mockResolvedValue(jsonResponse({ models: [{ id: 'kimi-for-coding' }] }));
    mocks.lookupModelMeta.mockReturnValue(null);
    useSettingsStore.setState({ toastMessage: '', toastType: '', toastVisible: false });
    Object.defineProperty(window, 'innerWidth', { configurable: true, writable: true, value: 1200 });
    Object.defineProperty(window, 'innerHeight', { configurable: true, writable: true, value: 900 });
  });

  afterEach(() => {
    cleanup();
    document.body.innerHTML = '';
  });

  it('portals the add-model dropdown to body so fixed coordinates are viewport-relative', async () => {
    const onRefresh = vi.fn(async () => {});
    const { container } = render(
      <div data-testid="provider-host">
        <ProviderModelList
          providerId="kimi-coding"
          summary={chatSummary()}
          media={makeMedia()}
          onRefresh={onRefresh}
        />
      </div>,
    );

    const trigger = screen.getByRole('button', { name: 'settings.api.addModel' });
    vi.spyOn(trigger, 'getBoundingClientRect').mockReturnValue(rect({
      left: 120,
      top: 300,
      bottom: 332,
      width: 240,
      height: 32,
    }));

    fireEvent.click(trigger);

    const panel = await waitFor(() => {
      const found = document.body.querySelector('[data-provider-model-dropdown="true"]');
      expect(found).not.toBeNull();
      return found as HTMLElement;
    });

    expect(container).not.toContainElement(panel);
    expect(panel).toHaveStyle({
      position: 'fixed',
      left: '120px',
      top: '336px',
      width: '320px',
    });
  });

  it('shows the output-modality icons directly after the model id, before capability badges', () => {
    mocks.lookupModelMeta.mockImplementation((id: unknown, provider: unknown) => {
      if (id === 'doubao-seed-2-0-lite-260428' && provider === 'volcengine') {
        return {
          name: 'Doubao Seed 2.0 Lite',
          image: true,
          video: true,
          audio: true,
          reasoning: true,
          context: 256000,
        };
      }
      return null;
    });

    render(
      <ProviderModelList
        providerId="volcengine"
        summary={chatSummary({
          display_name: 'Volcengine',
          base_url: 'https://ark.cn-beijing.volces.com/api/v3',
          api: 'openai-completions',
          api_key: 'sk-test',
          models: ['doubao-seed-2-0-lite-260428'],
          has_credentials: true,
          can_delete: true,
          is_coding_plan: false,
        })}
        media={makeMedia()}
        onRefresh={vi.fn(async () => {})}
      />,
    );

    const id = screen.getByText('doubao-seed-2-0-lite-260428');
    // 输出模态图标紧跟 ID：chat 默认 outputs ["text"]
    expect(id.nextElementSibling).toHaveAttribute('data-output-modalities', 'text');
    // 图标之后是 flex spacer，再之后才轮到输入能力 badge
    const spacer = id.nextElementSibling?.nextElementSibling;
    expect(spacer).toHaveClass(/pv-fav-item-spacer/);
    const firstCapability = spacer?.nextElementSibling;
    expect(firstCapability).toHaveAttribute('title', 'settings.api.capability.image');
    expect(firstCapability?.nextElementSibling).toHaveAttribute('title', 'settings.api.capability.video');
    expect(firstCapability?.nextElementSibling?.nextElementSibling).toHaveAttribute('title', 'settings.api.capability.audio');
  });

  it('keeps the context badge on chat rows', () => {
    mocks.lookupModelMeta.mockReturnValue({ name: 'Known Model', context: 131072 });
    render(
      <ProviderModelList
        providerId="volcengine"
        summary={chatSummary({ models: ['known-model'] })}
        media={makeMedia()}
        onRefresh={vi.fn(async () => {})}
      />,
    );
    expect(screen.getByText('131072')).toBeInTheDocument();
  });

  it('opens fetched models in the add-model dropdown so they can be enabled', async () => {
    const onRefresh = vi.fn(async () => {});
    mocks.lingxiFetch
      .mockResolvedValueOnce(jsonResponse({ models: [] }))
      .mockResolvedValueOnce(jsonResponse({ models: [{ id: 'kimi-new-model' }] }))
      .mockResolvedValueOnce(jsonResponse({ ok: true }));

    render(
      <ProviderModelList
        providerId="kimi-coding"
        summary={chatSummary({ models: [] })}
        media={makeMedia()}
        onRefresh={onRefresh}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'settings.providers.fetchModels' }));

    const option = await screen.findByRole('button', { name: /kimi-new-model/ });
    fireEvent.click(option);

    await waitFor(() => expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/config', expect.objectContaining({
      method: 'PUT',
      body: JSON.stringify({ providers: { 'kimi-coding': { models: ['kimi-new-model'] } } }),
    })));
    expect(onRefresh).toHaveBeenCalled();
  });

  it('does not refresh or pretend success when enabling a model returns a JSON error', async () => {
    const onRefresh = vi.fn(async () => {});
    mocks.lingxiFetch
      .mockResolvedValueOnce(jsonResponse({ models: [] }))
      .mockResolvedValueOnce(jsonResponse({ models: [{ id: 'runtime-rejected' }] }))
      .mockResolvedValueOnce(jsonResponse({ error: 'model sync failed' }));

    render(
      <ProviderModelList
        providerId="custom-vllm"
        summary={chatSummary({
          display_name: 'Custom vLLM',
          base_url: 'http://127.0.0.1:8000/v1',
          api: 'openai-completions',
          api_key: 'sk-test',
          models: [],
          has_credentials: true,
          can_delete: true,
          is_coding_plan: false,
        })}
        media={makeMedia()}
        onRefresh={onRefresh}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'settings.providers.fetchModels' }));
    fireEvent.click(await screen.findByRole('button', { name: /runtime-rejected/ }));

    await waitFor(() => expect(useSettingsStore.getState().toastMessage).toContain('model sync failed'));
    expect(useSettingsStore.getState().toastType).toBe('error');
    expect(onRefresh).not.toHaveBeenCalled();
  });

  it('writes OAuth custom models through Provider Catalog instead of the legacy preferences route', async () => {
    const onRefresh = vi.fn(async () => {});
    mocks.lingxiFetch
      .mockResolvedValueOnce(jsonResponse({ models: [] }))
      .mockResolvedValueOnce(jsonResponse({ ok: true }));

    render(
      <ProviderModelList
        providerId="openai-codex-oauth"
        summary={chatSummary({
          type: 'oauth',
          auth_type: 'oauth',
          display_name: 'OpenAI Codex (OAuth)',
          base_url: 'https://chatgpt.com/backend-api',
          api: 'openai-codex-responses',
          models: ['gpt-5.6-sol'],
          has_credentials: true,
          supports_oauth: true,
        })}
        media={makeMedia()}
        onRefresh={onRefresh}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'settings.api.addModel' }));
    const input = await screen.findByPlaceholderText('settings.oauth.customModelPlaceholder');
    fireEvent.change(input, { target: { value: 'my-codex-model' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/config', expect.objectContaining({
      method: 'PUT',
      body: JSON.stringify({
        providers: {
          'openai-codex-oauth': { models: ['gpt-5.6-sol', 'my-codex-model'] },
        },
      }),
    })));
    expect(mocks.lingxiFetch.mock.calls.some(([url]) => String(url).includes('/auth/oauth/'))).toBe(false);
    expect(onRefresh).toHaveBeenCalled();
  });

  it('persists discovered model metadata when enabling a fetched model', async () => {
    const onRefresh = vi.fn(async () => {});
    mocks.lingxiFetch
      .mockResolvedValueOnce(jsonResponse({ models: [] }))
      .mockResolvedValueOnce(jsonResponse({
        models: [
          {
            id: 'custom-vllm-chat',
            name: 'Custom vLLM Chat',
            context: 32768,
            maxOutput: 4096,
            image: true,
            video: true,
            audio: true,
            reasoning: true,
          },
        ],
      }))
      .mockResolvedValueOnce(jsonResponse({ ok: true }));

    render(
      <ProviderModelList
        providerId="custom-vllm"
        summary={chatSummary({
          display_name: 'Custom vLLM',
          base_url: 'http://127.0.0.1:8000/v1',
          api: 'openai-completions',
          api_key: 'sk-test',
          models: ['existing-model'],
          has_credentials: true,
          can_delete: true,
          is_coding_plan: false,
        })}
        media={makeMedia()}
        onRefresh={onRefresh}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'settings.providers.fetchModels' }));

    const option = await screen.findByRole('button', { name: /custom-vllm-chat/ });
    fireEvent.click(option);

    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/config', expect.objectContaining({
        method: 'PUT',
        body: JSON.stringify({
          providers: {
            'custom-vllm': {
              models: [
                'existing-model',
                {
                  id: 'custom-vllm-chat',
                  name: 'Custom vLLM Chat',
                  context: 32768,
                  maxOutput: 4096,
                  image: true,
                  video: true,
                  audio: true,
                  reasoning: true,
                },
              ],
            },
          },
        }),
      }));
    });
    expect(onRefresh).toHaveBeenCalled();
  });

  it('does not serialize untouched modality/capability defaults as explicit overrides', async () => {
    const onRefresh = vi.fn(async () => {});
    mocks.lingxiFetch.mockResolvedValue(jsonResponse({ models: [] }));
    mocks.lookupModelMeta.mockImplementation((id: unknown, provider: unknown) => {
      expect(provider).toBe('mimo');
      if (id === 'mimo-v2.5-pro') {
        return {
          name: 'MiMo V2.5 Pro',
          reasoning: true,
          image: false,
          video: false,
          audio: false,
        };
      }
      return null;
    });

    render(
      <ProviderModelList
        providerId="mimo"
        summary={chatSummary({
          display_name: 'Xiaomi (MiMo)',
          base_url: 'https://api.xiaomimo.com/v1',
          api_key: 'sk-test',
          models: ['mimo-v2.5-pro'],
          has_credentials: true,
          can_delete: true,
          is_coding_plan: false,
        })}
        media={makeMedia()}
        onRefresh={onRefresh}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'settings.api.editModel' }));
    fireEvent.click(screen.getByRole('button', { name: 'settings.api.save' }));

    await waitFor(() => {
      const updateCall = mocks.lingxiFetch.mock.calls.find(([url, options]) => (
        String(url).includes('/api/providers/mimo/models/mimo-v2.5-pro')
        && options?.method === 'PUT'
      ));
      expect(updateCall).toBeTruthy();
      expect(JSON.parse(String(updateCall?.[1]?.body))).toEqual({
        name: 'MiMo V2.5 Pro',
      });
    });
  });

  it('keeps the model editor open and skips refresh when metadata mutation returns a JSON error', async () => {
    const onRefresh = vi.fn(async () => {});
    mocks.lingxiFetch
      .mockResolvedValueOnce(jsonResponse({ models: [] }))
      .mockResolvedValueOnce(jsonResponse({ error: 'metadata sync failed' }));

    render(
      <ProviderModelList
        providerId="mimo"
        summary={chatSummary({
          display_name: 'Xiaomi (MiMo)',
          base_url: 'https://api.xiaomimo.com/v1',
          api_key: 'sk-test',
          models: ['mimo-v2.5-pro'],
          has_credentials: true,
          can_delete: true,
          is_coding_plan: false,
        })}
        media={makeMedia()}
        onRefresh={onRefresh}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'settings.api.editModel' }));
    fireEvent.click(screen.getByRole('button', { name: 'settings.api.save' }));

    await waitFor(() => expect(useSettingsStore.getState().toastMessage).toContain('metadata sync failed'));
    expect(useSettingsStore.getState().toastType).toBe('error');
    expect(onRefresh).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'settings.api.save' })).toBeTruthy();
  });

  it('serializes canonical inputs after the user changes an input modality chip', async () => {
    const onRefresh = vi.fn(async () => {});
    mocks.lingxiFetch.mockResolvedValue(jsonResponse({ models: [] }));
    mocks.lookupModelMeta.mockImplementation((id: unknown, provider: unknown) => {
      expect(provider).toBe('mimo');
      if (id === 'mimo-v2.5-pro') {
        return {
          name: 'MiMo V2.5 Pro',
          reasoning: true,
          image: false,
          video: false,
          audio: false,
        };
      }
      return null;
    });

    render(
      <ProviderModelList
        providerId="mimo"
        summary={chatSummary({
          display_name: 'Xiaomi (MiMo)',
          base_url: 'https://api.xiaomimo.com/v1',
          api_key: 'sk-test',
          models: ['mimo-v2.5-pro'],
          has_credentials: true,
          can_delete: true,
          is_coding_plan: false,
        })}
        media={makeMedia()}
        onRefresh={onRefresh}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'settings.api.editModel' }));
    fireEvent.click(screen.getAllByRole('button', { name: 'settings.api.modality.audio' })[0]);
    fireEvent.click(screen.getByRole('button', { name: 'settings.api.save' }));

    await waitFor(() => {
      const updateCall = mocks.lingxiFetch.mock.calls.find(([url, options]) => (
        String(url).includes('/api/providers/mimo/models/mimo-v2.5-pro')
        && options?.method === 'PUT'
      ));
      expect(updateCall).toBeTruthy();
      expect(JSON.parse(String(updateCall?.[1]?.body))).toEqual({
        name: 'MiMo V2.5 Pro',
        inputs: ['text', 'audio'],
      });
    });
  });

  it.each(['maxOutput', 'maxTokens', 'maxOutputTokens'] as const)(
    'reopens the editor with persisted user %s metadata overriding known defaults',
    async (outputField) => {
      mocks.lingxiFetch.mockResolvedValue(jsonResponse({ models: [] }));
      mocks.lookupModelMeta.mockReturnValue({
        name: 'Known GPT',
        context: 1050000,
        maxOutput: 128000,
        image: true,
        reasoning: true,
      });

      render(
        <ProviderModelList
          providerId="openai"
          summary={chatSummary({
            display_name: 'OpenAI',
            base_url: 'https://api.openai.com/v1',
            api: 'openai-responses',
            api_key: 'sk-test',
            models: [{
              id: 'gpt-5.6-sol',
              name: 'My Sol Override',
              contextWindow: 777000,
              [outputField]: 64000,
              image: false,
              reasoning: false,
            }],
            has_credentials: true,
            can_delete: false,
            is_coding_plan: false,
          })}
          media={makeMedia()}
          onRefresh={vi.fn(async () => {})}
        />,
      );

      expect(screen.getByText('777000')).toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: 'settings.api.editModel' }));

      expect(screen.getByDisplayValue('My Sol Override')).toBeInTheDocument();
      expect(screen.getByDisplayValue('777000')).toBeInTheDocument();
      expect(screen.getByDisplayValue('64000')).toBeInTheDocument();
      expect(screen.getAllByRole('button', { name: 'settings.api.modality.image' })[0]).toHaveAttribute('aria-pressed', 'false');
      expect(screen.getAllByRole('button', { name: 'settings.api.reasoning' })[0]).toHaveAttribute('aria-pressed', 'false');

      fireEvent.click(screen.getByRole('button', { name: 'settings.api.save' }));
      await waitFor(() => {
        const updateCall = mocks.lingxiFetch.mock.calls.find(([url, options]) => (
          String(url).includes('/api/providers/openai/models/gpt-5.6-sol')
          && options?.method === 'PUT'
        ));
        expect(JSON.parse(String(updateCall?.[1]?.body))).toEqual({
          name: 'My Sol Override',
          context: 777000,
          maxOutput: 64000,
        });
      });
    },
  );
});

// ── 统一模型列表（chat + image + video + speech） ────────────────────────────

function unifiedMediaFixture({
  imageModels,
  videoModels,
  speechModels,
  imageAvailable,
  speechCatalog,
  imageConfig = null,
  videoConfig = null,
}: Record<string, any> = {}) {
  imageModels = imageModels ?? [{ id: 'wan-image-x', displayName: 'Wan 2.7 Image Pro', inputs: ['text'], outputs: ['image'], protocolId: 'dashscope-qwen-multimodal-image' }];
  videoModels = videoModels ?? [{ id: 'agnes-video-v2.0', displayName: 'Agnes Video V2.0', inputs: ['text'], outputs: ['video'], protocolId: 'agnes-videos' }];
  speechModels = speechModels ?? [{ id: 'whisper-x', displayName: 'Whisper X', inputs: ['audio'], outputs: ['text'], protocolId: 'asr' }];
  imageAvailable = imageAvailable ?? [{ id: 'qwen-image-candidate', name: 'Qwen Image Candidate' }];
  speechCatalog = speechCatalog ?? [{ id: 'catalog-asr', name: 'Catalog ASR' }];
  return makeMedia({
    imageProviders: {
      dashscope: {
        providerId: 'dashscope',
        displayName: 'DashScope',
        hasCredentials: true,
        models: imageModels,
        availableModels: imageAvailable,
      },
    },
    videoProviders: {
      agnes: {
        providerId: 'agnes',
        displayName: 'Agnes AI',
        hasCredentials: true,
        models: videoModels,
        availableModels: [],
      },
    },
    speechProviders: {
      'volcengine-speech': {
        providerId: 'volcengine-speech',
        displayName: 'Volcengine Speech',
        hasCredentials: true,
        models: speechModels,
        availableModels: [],
        catalogModels: speechCatalog,
      },
    },
    imageConfig,
    videoConfig,
  });
}

function unifiedSummary(overrides: Record<string, any> = {}) {
  return chatSummary({
    display_name: 'All-in-One Provider',
    base_url: 'https://api.example.com/v1',
    api: 'openai-completions',
    api_key: 'sk-test',
    models: ['qwen3.8-max'],
    has_credentials: true,
    can_delete: true,
    is_coding_plan: false,
    media_capability_bindings: [
      { capability: 'imageGeneration', runtime_provider_id: 'dashscope' },
      { capability: 'videoGeneration', runtime_provider_id: 'agnes' },
      { capability: 'speechRecognition', runtime_provider_id: 'volcengine-speech' },
    ],
    ...overrides,
  });
}

function renderUnified(summaryOverrides: Record<string, any> = {}, media = unifiedMediaFixture()) {
  return render(
    <ProviderModelList
      providerId="all-in-one"
      summary={unifiedSummary(summaryOverrides)}
      media={media}
      onRefresh={vi.fn(async () => {})}
    />,
  );
}

describe('ProviderModelList (unified chat/image/video/speech)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.lingxiFetch.mockResolvedValue(jsonResponse({ models: [] }));
    mocks.lookupModelMeta.mockReturnValue(null);
    useSettingsStore.setState({ toastMessage: '', toastType: '', toastVisible: false });
  });

  afterEach(() => {
    cleanup();
    document.body.innerHTML = '';
  });

  it('lists chat, image, video and speech models in one added-models section', () => {
    const { container } = renderUnified();
    const section = container.querySelector('[data-unified-model-list="true"]');
    expect(section).not.toBeNull();
    const kinds = Array.from(section!.querySelectorAll('[data-unified-kind]')).map(el => el.getAttribute('data-unified-kind'));
    // chat 在前，媒体按 binding 顺序跟随；都在同一个「已添加的模型」区域
    expect(kinds).toEqual(['chat', 'image', 'video', 'speech']);
    expect(container.querySelectorAll('[data-unified-model-list="true"]')).toHaveLength(1);
  });

  it('shows output icons adjacent to the model id with the right modality per kind', () => {
    renderUnified();
    const chatRow = document.querySelector('[data-unified-kind="chat"]')!;
    const imageRow = document.querySelector('[data-unified-kind="image"]')!;
    const videoRow = document.querySelector('[data-unified-kind="video"]')!;
    const speechRow = document.querySelector('[data-unified-kind="speech"]')!;

    const outputsOf = (row: Element) => row.querySelector('[data-output-modalities]') as HTMLElement | null;
    const chatOutputs = outputsOf(chatRow)!;
    // chat 模型名与 id 相同时，图标紧跟承担 ID 展示职责的名称元素
    expect(chatOutputs.previousElementSibling!.textContent).toBe('qwen3.8-max');
    expect(chatOutputs).toHaveAttribute('data-output-modalities', 'text');

    const imageOutputs = outputsOf(imageRow)!;
    expect(imageOutputs.previousElementSibling!.textContent).toBe('wan-image-x');
    expect(imageOutputs).toHaveAttribute('data-output-modalities', 'image');

    expect(outputsOf(videoRow)).toHaveAttribute('data-output-modalities', 'video');

    // 语音识别模型 outputs = ["text"]，即使它属于语音类别也显示文本输出图标
    expect(outputsOf(speechRow)).toHaveAttribute('data-output-modalities', 'text');
  });

  it('renders input capability icons on media model rows, not only chat rows', () => {
    // 图片模型接受 text+image 输入、语音模型接受 audio 输入：
    // 行尾必须显示对应的输入能力图标（此前只有 chat 行渲染输入图标）
    renderUnified({}, unifiedMediaFixture({
      imageModels: [{ id: 'img-edit-model', displayName: '图片编辑模型', inputs: ['text', 'image'], outputs: ['image'], protocolId: 'p' }],
      speechModels: [{ id: 'asr-model', displayName: 'ASR', inputs: ['audio'], outputs: ['text'], protocolId: 'asr' }],
    }));
    const imageRow = document.querySelector('[data-unified-kind="image"][data-model-id="img-edit-model"]')!;
    expect(imageRow.querySelector('[title="settings.api.capability.image"]')).not.toBeNull();
    const speechRow = document.querySelector('[data-unified-kind="speech"][data-model-id="asr-model"]')!;
    expect(speechRow.querySelector('[title="settings.api.capability.audio"]')).not.toBeNull();
    // 纯文生图模型（inputs 只有 text）没有输入能力图标
    const videoRow = document.querySelector('[data-unified-kind="video"]')!;
    expect(videoRow.querySelector('[class*="pv-capability-icon"]')).toBeNull();
  });

  it('renders multiple output icons for multi-output models', () => {
    renderUnified({
      // 聊天模型 outputs: ["text","image"]
    }, unifiedMediaFixture({ imageModels: [
      { id: 'multi-out', displayName: 'Multi Out', inputs: ['text'], outputs: ['text', 'image'], protocolId: 'p' },
    ] }));
    const outputs = document.querySelector('[data-model-id="multi-out"] [data-output-modalities]')!;
    expect(outputs).toHaveAttribute('data-output-modalities', 'text image');
    expect(outputs.querySelectorAll('svg')).toHaveLength(2);
  });

  it('renders default badges for media models matching the configured default', () => {
    renderUnified({}, unifiedMediaFixture({
      imageConfig: { defaultImageModel: { id: 'wan-image-x', provider: 'dashscope' } },
      videoConfig: { defaultVideoModel: { id: 'agnes-video-v2.0', provider: 'agnes' } },
    }));
    const imageRow = document.querySelector('[data-unified-kind="image"]')!;
    const videoRow = document.querySelector('[data-unified-kind="video"]')!;
    expect(imageRow.querySelector('[data-default-badge="true"]')).not.toBeNull();
    expect(videoRow.querySelector('[data-default-badge="true"]')).not.toBeNull();
  });

  it('dedupes a bare chat entry whose id already exists in the media catalog', () => {
    // 用户视角这是「重复的模型 id」：同 id 的 chat 条目 + 媒体目录条目只渲染媒体行
    renderUnified({ models: ['wan-image-x'] }, unifiedMediaFixture());
    const rows = document.querySelectorAll('[data-model-id="wan-image-x"]');
    expect(rows).toHaveLength(1);
    expect(document.querySelectorAll('[data-unified-kind="image"][data-model-id="wan-image-x"]')).toHaveLength(1);
  });

  it('hides edit/remove actions for runtime-discovered media models', () => {
    const media = unifiedMediaFixture({
      imageModels: [{ id: 'cli-image', displayName: 'CLI Image', inputs: ['text'], outputs: ['image'], protocolId: 'p' }],
    });
    (media.image.providers.dashscope as any).runtimeCapability = { status: 'ready' };
    renderUnified({}, media);
    const imageRow = document.querySelector('[data-unified-kind="image"]');
    expect(imageRow!.querySelector('[data-unified-edit="image"]')).toBeNull();
    expect(imageRow!.querySelector('[data-unified-remove="image"]')).toBeNull();
    // chat 模型不受影响
    expect(document.querySelector('[data-unified-edit="chat"]')).not.toBeNull();
  });

  it('groups add-model candidates by category in one dropdown and submits full metadata', async () => {
    renderUnified();
    fireEvent.click(screen.getByRole('button', { name: 'settings.api.addModel' }));
    const panel = await waitFor(() => document.body.querySelector('[data-provider-model-dropdown="true"]') as HTMLElement);

    // 分组标题按 对话 / 图片 / 视频 / 语音识别 排列
    const groups = Array.from(panel.querySelectorAll('[data-model-group]')).map(el => el.getAttribute('data-model-group'));
    expect(groups).toEqual(['chat', 'image', 'video', 'speech']);

    // 候选提交完整 metadata（catalog 声明的 displayName/protocolId），而不是裸 ID
    const imageCandidate = panel.querySelector('[data-media-candidate="image:dashscope:qwen-image-candidate"]') as HTMLElement;
    fireEvent.click(imageCandidate);
    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/media/image/providers/dashscope/models', expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ model: { id: 'qwen-image-candidate', name: 'Qwen Image Candidate' } }),
      }));
    });

    const speechCandidate = panel.querySelector('[data-media-candidate="speech:volcengine-speech:catalog-asr"]') as HTMLElement;
    fireEvent.click(speechCandidate);
    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/speech-recognition/providers/volcengine-speech/models', expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ model: { id: 'catalog-asr', name: 'Catalog ASR' } }),
      }));
    });
  });

  it('adds a custom model id with the selected category', async () => {
    const media = unifiedMediaFixture();
    renderUnified({}, media);
    fireEvent.click(screen.getByRole('button', { name: 'settings.api.addModel' }));
    const input = await screen.findByPlaceholderText('settings.oauth.customModelPlaceholder');
    fireEvent.change(input, { target: { value: 'brand-new-image-model' } });

    const category = document.body.querySelector('[data-custom-model-category]') as HTMLSelectElement;
    expect(category).not.toBeNull();
    fireEvent.change(category, { target: { value: 'image' } });

    fireEvent.click(document.body.querySelector('[data-custom-model-add="true"]') as HTMLElement);
    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/media/image/providers/dashscope/models', expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ model: { id: 'brand-new-image-model', inputs: ['text'], outputs: ['image'] } }),
      }));
    });
    expect(media.refreshImage).toHaveBeenCalled();
  });

  it('offers only manually manageable categories for custom models', async () => {
    const media = unifiedMediaFixture();
    (media.image.providers.dashscope as any).runtimeCapability = { status: 'ready' };
    renderUnified({}, media);
    fireEvent.click(screen.getByRole('button', { name: 'settings.api.addModel' }));
    const category = await waitFor(() => document.body.querySelector('[data-custom-model-category]') as HTMLSelectElement);
    const options = Array.from(category.querySelectorAll('option')).map(o => o.value);
    // runtime-discovered 的图片类别不允许自定义 mutation
    expect(options).toEqual(['chat', 'video', 'speech']);
  });

  it('removes media models through the media endpoints', async () => {
    const media = unifiedMediaFixture();
    renderUnified({}, media);
    fireEvent.click(document.querySelector('[data-unified-remove="video"]') as HTMLElement);
    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/media/video/providers/agnes/models/agnes-video-v2.0', { method: 'DELETE' });
    });
    expect(media.refreshVideo).toHaveBeenCalled();
  });

  it('keeps a compact credential status line when a listed media provider lacks credentials', () => {
    const media = unifiedMediaFixture();
    (media.image.providers.dashscope as any).hasCredentials = false;
    const { container } = renderUnified({}, media);
    expect(container.querySelector('[data-capability-status="imageGeneration:dashscope"]')).not.toBeNull();
  });

  it('keeps a compact unavailable status line when a declared binding fails to load', () => {
    const { container } = renderUnified({}, unifiedMediaFixture());
    // 已加载成功的不出现 unavailable 行；改用未加载 binding 验证
    expect(container.querySelector('[data-capability-unavailable]')).toBeNull();
    const media = unifiedMediaFixture();
    delete (media as any).speech.providers['volcengine-speech'];
    const second = render(
      <ProviderModelList
        providerId="all-in-one"
        summary={unifiedSummary()}
        media={media}
        onRefresh={vi.fn(async () => {})}
      />,
    );
    expect(second.container.querySelector('[data-capability-unavailable="speechRecognition:volcengine-speech"]')).not.toBeNull();
  });
});

// ── 默认参数按钮与 modal ─────────────────────────────────────────────────────

describe('ProviderModelList default params buttons and modal', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.lingxiFetch.mockResolvedValue(jsonResponse({ models: [] }));
    mocks.lookupModelMeta.mockReturnValue(null);
    useSettingsStore.setState({ toastMessage: '', toastType: '', toastVisible: false });
  });

  afterEach(() => {
    cleanup();
    document.body.innerHTML = '';
  });

  it('shows image/video default-params buttons in a single nowrap row when models exist', () => {
    const { container } = renderUnified();
    const row = container.querySelector('[data-default-params-row="true"]');
    expect(row).not.toBeNull();
    const buttons = row!.querySelectorAll('button');
    expect(buttons).toHaveLength(2);
    expect(buttons[0].textContent).toBe('settings.media.imageDefaultsButton');
    expect(buttons[1].textContent).toBe('settings.media.videoDefaultsButton');
  });

  it('hides default-params buttons entirely when no generation models are added', () => {
    const media = unifiedMediaFixture({ imageModels: [], videoModels: [] });
    const { container } = renderUnified({}, media);
    expect(container.querySelector('[data-default-params-row="true"]')).toBeNull();
    expect(screen.queryByText('settings.media.imageDefaultsButton')).toBeNull();
    expect(screen.queryByText('settings.media.videoDefaultsButton')).toBeNull();
    // 语音合成默认参数按钮不存在（当前没有 speech-generation runtime）
    expect(screen.queryByText('settings.media.speechSynthesisDefaultsButton')).toBeNull();
  });

  it('hides the image button when only video models are added', () => {
    const media = unifiedMediaFixture({ imageModels: [] });
    const { container } = renderUnified({}, media);
    const row = container.querySelector('[data-default-params-row="true"]');
    expect(row).not.toBeNull();
    const buttons = row!.querySelectorAll('button');
    expect(buttons).toHaveLength(1);
    expect(buttons[0].textContent).toBe('settings.media.videoDefaultsButton');
  });

  it('opens a modal instead of an inline defaults form', () => {
    const { container } = renderUnified();
    // ProviderDetail 原页面不 inline 出现参数表单（点击之前）
    expect(container.querySelector('[data-media-defaults="true"]')).toBeNull();
    fireEvent.click(screen.getByText('settings.media.imageDefaultsButton'));
    const modal = document.querySelector('[data-defaults-modal="imageGeneration"]');
    expect(modal).not.toBeNull();
    // modal 内部复用 ProviderMediaDefaults 表单逻辑
    expect(modal!.querySelector('[data-media-defaults="true"]')).not.toBeNull();
  });

  it('removes a claimed image row through the chat slot instead of a media DELETE', async () => {
    // 认领行的真实数据在 provider.models：走媒体 DELETE 是空操作，
    // 界面会弹「保存成功」但模型删不掉
    const onRefresh = vi.fn(async () => {});
    const media = unifiedMediaFixture({ imageModels: [] });
    render(
      <ProviderModelList
        providerId="all-in-one"
        summary={unifiedSummary({
          models: [
            'chat-model',
            { id: 'legacy-image-model', outputs: ['image'], inputs: ['text'] },
          ],
        })}
        media={media}
        onRefresh={onRefresh}
      />,
    );

    const claimedRow = document.querySelector('[data-unified-kind="image"][data-model-id="legacy-image-model"]');
    expect(claimedRow).not.toBeNull();
    fireEvent.click(claimedRow!.querySelector('[data-unified-remove="image"]')!);

    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/config', expect.objectContaining({
        method: 'PUT',
        body: JSON.stringify({
          providers: { 'all-in-one': { models: ['chat-model'] } },
        }),
      }));
    });
    // 不允许走媒体 DELETE 假装成功
    expect(mocks.lingxiFetch.mock.calls.some(([url, options]) => (
      String(url).includes('/api/media/image/providers/') && options?.method === 'DELETE'
    ))).toBe(false);
    // chat 槽变更同时刷新 summary 与媒体投影
    expect(onRefresh).toHaveBeenCalled();
    expect(media.refreshAll).toHaveBeenCalled();
  });

  it('excludes dictionary-typed media models from the chat candidate group', async () => {
    // 远端 /models 会返回图片生成模型；词典 type=image 的候选必须留在图片分组，
    // 不能出现在「对话模型」分组里（否则会被写进 chat 槽造成双行重复）
    mocks.lookupModelMeta.mockImplementation((id: unknown) => (
      id === 'gpt-image-1' ? { type: 'image' } : null
    ));
    mocks.lingxiFetch
      .mockResolvedValueOnce(jsonResponse({ models: [] }))
      .mockResolvedValueOnce(jsonResponse({
        models: [{ id: 'gpt-4o' }, { id: 'gpt-image-1' }],
      }));

    renderUnified({ models: [] });
    fireEvent.click(screen.getByRole('button', { name: 'settings.providers.fetchModels' }));

    await waitFor(() => {
      const panel = document.body.querySelector('[data-provider-model-dropdown="true"]');
      expect(panel).not.toBeNull();
      const optionTexts = [...panel!.querySelectorAll('button')].map(b => b.textContent || '');
      expect(optionTexts.some(t => t.includes('gpt-4o'))).toBe(true);
      // 词典 type=image 的候选不出现在任何分组（图片分组只展示媒体目录声明的候选）
      expect(optionTexts.some(t => t.includes('gpt-image-1'))).toBe(false);
    });
  });

  it('never lists the same model id in both the chat group and a media group', async () => {
    // agnes 场景：远端发现缓存里有媒体模型的裸 id（不在 known 词典中，词典过滤
    // 管不到），媒体分组又会展示同一模型——chat 分组必须按媒体目录互斥排除
    mocks.lingxiFetch
      .mockResolvedValueOnce(jsonResponse({
        models: [
          { id: 'agnes-2.0-flash' },
          { id: 'agnes-image-2.1-flash' },
          { id: 'agnes-video-v2.0' },
        ],
      }));

    const media = unifiedMediaFixture({
      imageModels: [{ id: 'agnes-image-2.1-flash', displayName: 'Agnes Image 2.1 Flash', inputs: ['text', 'image'], outputs: ['image'], protocolId: 'agnes-images' }],
      videoModels: [{ id: 'agnes-video-v2.0', displayName: 'Agnes Video V2.0', inputs: ['text'], outputs: ['video'], protocolId: 'agnes-videos' }],
    });
    renderUnified({ models: [] }, media);
    fireEvent.click(screen.getByRole('button', { name: 'settings.api.addModel' }));

    const panel = await waitFor(() => document.body.querySelector('[data-provider-model-dropdown="true"]') as HTMLElement);
    // chat 分组：只有真正的 chat 模型
    const chatOptions = [...panel.querySelectorAll('button')]
      .filter(b => b.querySelector('[class*="option-name"]') && !b.getAttribute('data-media-candidate'))
      .map(b => b.textContent || '');
    expect(chatOptions.some(t => t.includes('agnes-2.0-flash'))).toBe(true);
    expect(chatOptions.some(t => t.includes('agnes-image-2.1-flash'))).toBe(false);
    expect(chatOptions.some(t => t.includes('agnes-video-v2.0'))).toBe(false);
    // 媒体分组仍然展示它们（唯一添加入口）
    const mediaCandidates = [...panel.querySelectorAll('[data-media-candidate]')].map(b => b.getAttribute('data-media-candidate'));
    expect(mediaCandidates).toContain('image:dashscope:agnes-image-2.1-flash');
    expect(mediaCandidates).toContain('video:agnes:agnes-video-v2.0');
  });
});
