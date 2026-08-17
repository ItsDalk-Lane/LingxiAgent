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

vi.mock('../../../helpers', () => ({
  t: (key: string) => key,
  formatContext: (n: number) => `${n}`,
  lookupModelMeta: (id: unknown, provider?: unknown) => mocks.lookupModelMeta(id, provider),
  CONTEXT_PRESETS: [],
  OUTPUT_PRESETS: [],
}));

import { ModelEditPanel } from '../ModelEditPanel';

function jsonResponse(body: unknown): Response {
  return { json: async () => body } as Response;
}

function renderPanel(props: Record<string, any>) {
  return render(
    <ModelEditPanel
      kind="chat"
      providerId="prov"
      runtimeProviderId="prov"
      modelId="model-x"
      anchorEl={null}
      onClose={vi.fn()}
      onRefresh={vi.fn(async () => {})}
      {...props}
    />,
  );
}

describe('ModelEditPanel (chat)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.lookupModelMeta.mockReturnValue(null);
    mocks.lingxiFetch.mockResolvedValue(jsonResponse({ ok: true }));
    useSettingsStore.setState({ toastMessage: '', toastType: '', toastVisible: false });
  });

  afterEach(() => cleanup());

  it('renders context config, input/output modality chips and capability chips in order', () => {
    renderPanel({ summaryApi: 'openai-completions', summaryBaseUrl: 'https://api.openai.com/v1' });
    expect(screen.getByText('settings.api.contextConfig')).toBeInTheDocument();
    expect(screen.getAllByText('settings.api.inputModalities').length).toBeGreaterThan(0);
    expect(screen.getAllByText('settings.api.outputModalities').length).toBeGreaterThan(0);
    expect(screen.getByText('settings.api.modelCapabilities')).toBeInTheDocument();
    // 能力 chips：推理 / 工具 / 联网 / 结构化输出
    for (const label of ['settings.api.reasoning', 'settings.api.toolUse', 'settings.api.capability.web', 'settings.api.capability.structuredOutput']) {
      expect(screen.getAllByRole('button', { name: label }).length).toBeGreaterThan(0);
    }
    // 模态 chips 一行四个
    for (const label of ['settings.api.modality.text', 'settings.api.modality.image', 'settings.api.modality.video', 'settings.api.modality.audio']) {
      const chips = screen.getAllByRole('button', { name: label });
      expect(chips.length).toBe(2); // 输入 + 输出两组
      chips.forEach(chip => expect(chip).toHaveAttribute('aria-pressed', expect.any(String)));
    }
  });

  it('seeds inputs from discovered/known metadata before falling back to kind defaults', () => {
    mocks.lookupModelMeta.mockReturnValue({ image: true, video: true, reasoning: true });
    renderPanel({});
    expect(screen.getAllByRole('button', { name: 'settings.api.modality.image' })[0]).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getAllByRole('button', { name: 'settings.api.modality.video' })[0]).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getAllByRole('button', { name: 'settings.api.reasoning' })[0]).toHaveAttribute('aria-pressed', 'true');
  });

  it('keeps structured output and web disabled+off for unsupported protocols', () => {
    renderPanel({ summaryApi: 'anthropic-messages' });
    const structured = screen.getByRole('button', { name: 'settings.api.capability.structuredOutput' });
    const web = screen.getByRole('button', { name: 'settings.api.capability.web' });
    expect(structured).toBeDisabled();
    expect(structured).toHaveAttribute('aria-pressed', 'false');
    expect(structured).toHaveAttribute('title', 'settings.api.structuredOutputUnsupported');
    expect(web).toBeDisabled();
    expect(web).toHaveAttribute('aria-pressed', 'false');
    expect(web).toHaveAttribute('title', 'settings.api.webUnsupported');
  });

  it('enables web/structured output chips only for supported protocols and defaults them OFF', () => {
    renderPanel({ summaryApi: 'openai-completions', summaryBaseUrl: 'https://api.openai.com/v1' });
    const structured = screen.getByRole('button', { name: 'settings.api.capability.structuredOutput' });
    const web = screen.getByRole('button', { name: 'settings.api.capability.web' });
    expect(structured).not.toBeDisabled();
    // structuredOutput 没有可靠检测源 → 默认 OFF，不自动开启
    expect(structured).toHaveAttribute('aria-pressed', 'false');
    // OpenAI Chat Completions 官方 host 不构成原生联网证明 → web 仍禁用
    expect(web).toBeDisabled();
  });

  it('preserves the existing toolUse contract and only flips supportsTools', async () => {
    renderPanel({
      modelMeta: {
        toolUse: {
          supportsTools: false,
          dialect: 'anthropic',
          toolResultFormat: 'content_block',
          supportsParallelToolCalls: true,
        },
      },
      summaryApi: 'anthropic-messages',
    });
    fireEvent.click(screen.getByRole('button', { name: 'settings.api.toolUse' }));
    fireEvent.click(screen.getByRole('button', { name: 'settings.api.save' }));
    await waitFor(() => {
      const call = mocks.lingxiFetch.mock.calls.find(([url, options]) => (
        String(url).includes('/api/providers/prov/models/model-x') && options?.method === 'PUT'
      ));
      expect(JSON.parse(String(call?.[1]?.body)).toolUse).toEqual({
        supportsTools: true,
        dialect: 'anthropic',
        toolResultFormat: 'content_block',
        supportsParallelToolCalls: true,
      });
    });
  });

  it('derives a default toolUse contract from the model api when none exists', async () => {
    renderPanel({ summaryApi: 'google-generative-ai' });
    fireEvent.click(screen.getByRole('button', { name: 'settings.api.toolUse' }));
    fireEvent.click(screen.getByRole('button', { name: 'settings.api.save' }));
    await waitFor(() => {
      const call = mocks.lingxiFetch.mock.calls.find(([url, options]) => (
        String(url).includes('/api/providers/prov/models/model-x') && options?.method === 'PUT'
      ));
      expect(JSON.parse(String(call?.[1]?.body)).toolUse).toEqual({
        supportsTools: true,
        dialect: 'gemini',
        toolResultFormat: 'part',
      });
    });
  });

  it('saves canonical inputs/outputs and web/structuredOutput only when changed', async () => {
    renderPanel({ summaryApi: 'openai-completions', summaryBaseUrl: 'https://api.openai.com/v1' });
    // 输入模态加 image；输出模态关 audio（默认只有 text，无 audio）
    fireEvent.click(screen.getAllByRole('button', { name: 'settings.api.modality.image' })[0]);
    fireEvent.click(screen.getByRole('button', { name: 'settings.api.capability.structuredOutput' }));
    fireEvent.click(screen.getByRole('button', { name: 'settings.api.save' }));
    await waitFor(() => {
      const call = mocks.lingxiFetch.mock.calls.find(([url, options]) => (
        String(url).includes('/api/providers/prov/models/model-x') && options?.method === 'PUT'
      ));
      const body = JSON.parse(String(call?.[1]?.body));
      expect(body.inputs).toEqual(['text', 'image']);
      expect(body.structuredOutput).toBe(true);
      // 未触碰的 outputs / web / reasoning 不序列化
      expect(body.outputs).toBeUndefined();
      expect(body.web).toBeUndefined();
      expect(body.reasoning).toBeUndefined();
    });
  });
});

describe('ModelEditPanel (image/video/speech)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.lookupModelMeta.mockReturnValue(null);
    mocks.lingxiFetch.mockResolvedValue(jsonResponse({ ok: true }));
    useSettingsStore.setState({ toastMessage: '', toastType: '', toastVisible: false });
  });

  afterEach(() => cleanup());

  it('hides context config and capability chips for media kinds', () => {
    renderPanel({ kind: 'image', runtimeProviderId: 'dashscope', modelId: 'wan-image-x' });
    expect(screen.queryByText('settings.api.contextConfig')).toBeNull();
    expect(screen.queryByText('settings.api.modelCapabilities')).toBeNull();
    expect(screen.queryByRole('button', { name: 'settings.api.capability.web' })).toBeNull();
    expect(screen.getAllByText('settings.api.inputModalities').length).toBeGreaterThan(0);
    expect(screen.getAllByText('settings.api.outputModalities').length).toBeGreaterThan(0);
  });

  it('PUTs image model edits through the media route', async () => {
    const onRefresh = vi.fn(async () => {});
    renderPanel({
      kind: 'image',
      runtimeProviderId: 'dashscope',
      modelId: 'wan-image-x',
      modelMeta: { id: 'wan-image-x', displayName: 'Wan Image Pro', inputs: ['text'], outputs: ['image'] },
      onRefresh,
    });
    fireEvent.change(screen.getByDisplayValue('Wan Image Pro'), { target: { value: 'Wan Image Pro 2' } });
    fireEvent.click(screen.getByRole('button', { name: 'settings.api.save' }));
    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/media/image/providers/dashscope/models/wan-image-x', expect.objectContaining({
        method: 'PUT',
        body: JSON.stringify({ displayName: 'Wan Image Pro 2' }),
      }));
    });
    expect(onRefresh).toHaveBeenCalled();
  });

  it('PUTs video model edits with dirty modality fields', async () => {
    renderPanel({
      kind: 'video',
      runtimeProviderId: 'agnes',
      modelId: 'video-x',
      modelMeta: { id: 'video-x', inputs: ['text'], outputs: ['video'] },
    });
    fireEvent.click(screen.getAllByRole('button', { name: 'settings.api.modality.image' })[0]);
    fireEvent.click(screen.getByRole('button', { name: 'settings.api.save' }));
    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/media/video/providers/agnes/models/video-x', expect.objectContaining({
        method: 'PUT',
        body: JSON.stringify({ inputs: ['text', 'image'] }),
      }));
    });
  });

  it('PUTs speech recognition model edits through the speech route', async () => {
    renderPanel({
      kind: 'speech',
      runtimeProviderId: 'volcengine-speech',
      modelId: 'whisper-x',
      modelMeta: { id: 'whisper-x', inputs: ['audio'], outputs: ['text'] },
    });
    fireEvent.change(screen.getByPlaceholderText('whisper-x'), { target: { value: 'Whisper X' } });
    fireEvent.click(screen.getByRole('button', { name: 'settings.api.save' }));
    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/speech-recognition/providers/volcengine-speech/models/whisper-x', expect.objectContaining({
        method: 'PUT',
        body: JSON.stringify({ displayName: 'Whisper X' }),
      }));
    });
  });

  it('seeds speech edits with audio input + text output', () => {
    renderPanel({ kind: 'speech', runtimeProviderId: 'v', modelId: 'm' });
    expect(screen.getAllByRole('button', { name: 'settings.api.modality.audio' })[0]).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getAllByRole('button', { name: 'settings.api.modality.text' })[1]).toHaveAttribute('aria-pressed', 'true');
  });
});
