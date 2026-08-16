/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';

const mocks = vi.hoisted(() => ({
  lingxiFetch: vi.fn(),
  invalidateConfigCache: vi.fn(),
  showToast: vi.fn(),
}));

vi.mock('../../../api', () => ({
  lingxiFetch: (...args: unknown[]) => mocks.lingxiFetch(...args),
}));

vi.mock('../../../../hooks/use-config', () => ({
  invalidateConfigCache: () => mocks.invalidateConfigCache(),
}));

vi.mock('../../../store', () => ({
  useSettingsStore: (selector: (state: { showToast: typeof mocks.showToast }) => unknown) =>
    selector({ showToast: mocks.showToast }),
}));

vi.mock('../../../helpers', () => ({
  t: (key: string) => key,
}));

vi.mock('../../../hooks/useAnchoredDropdown', () => ({
  useAnchoredDropdown: () => ({ position: 'fixed', left: 0, top: 0, width: 280 }),
}));

import { ProviderSpeechModels } from '../ProviderSpeechModels';

describe('ProviderSpeechModels', () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('lists added speech models, marks the default, and hides adapter-missing ones', () => {
    render(
      <ProviderSpeechModels
        runtimeProviderId="openai"
        provider={{
          providerId: 'openai',
          displayName: 'OpenAI Speech',
          hasCredentials: true,
          models: [
            { id: 'whisper-1', name: 'Whisper 1', adapterAvailable: true },
            { id: 'unrunnable', name: 'Unrunnable', adapterAvailable: false },
          ],
        }}
        config={{ enabled: true, defaultModel: { provider: 'openai', id: 'whisper-1' } }}
        onRefresh={vi.fn(async () => {})}
      />,
    );

    expect(screen.getByText('whisper-1')).toBeInTheDocument();
    expect(screen.queryByText('unrunnable')).not.toBeInTheDocument();
    expect(screen.getByText('settings.media.default')).toBeInTheDocument();
  });

  it('offers the built-in catalog as add candidates without pre-populating the added list', async () => {
    mocks.lingxiFetch.mockResolvedValue({ json: async () => ({ ok: true }) });
    const onRefresh = vi.fn(async () => {});

    render(
      <ProviderSpeechModels
        runtimeProviderId="volcengine-speech"
        provider={{
          providerId: 'volcengine-speech',
          displayName: '火山引擎语音',
          hasCredentials: true,
          models: [],
          catalogModels: [{ id: 'bigasr-flash', name: 'BigASR Flash' }],
        }}
        config={{ enabled: true }}
        onRefresh={onRefresh}
      />,
    );

    // 已添加列表为空：只有添加按钮，没有预置模型
    expect(screen.queryByTitle('settings.api.removeModel')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /settings\.media\.addSpeechModel/ }));
    fireEvent.click(screen.getByRole('button', { name: /BigASR Flash/i }));

    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith(
        '/api/speech-recognition/providers/volcengine-speech/models',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({ model: { id: 'bigasr-flash', name: 'BigASR Flash' } }),
        }),
      );
    });
    expect(onRefresh).toHaveBeenCalled();
  });

  it('dedupes catalog candidates against added models by id in the add dropdown', () => {
    render(
      <ProviderSpeechModels
        runtimeProviderId="volcengine-speech"
        provider={{
          providerId: 'volcengine-speech',
          displayName: '火山引擎语音',
          hasCredentials: true,
          models: [{ id: 'bigasr-flash', name: 'BigASR Flash', adapterAvailable: true }],
          // 旧服务端/缓存数据：目录仍含已添加 id，下拉不得出现重复条目
          catalogModels: [
            { id: 'bigasr-flash', name: 'BigASR Flash' },
            { id: 'other-asr', name: 'Other ASR' },
          ],
        }}
        config={{ enabled: true }}
        onRefresh={vi.fn(async () => {})}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: /settings\.media\.addSpeechModel/ }));
    const panel = document.querySelector('[data-speech-model-dropdown="true"]');
    expect(panel).not.toBeNull();
    expect(within(panel as HTMLElement).getAllByText('BigASR Flash')).toHaveLength(1);
    expect(within(panel as HTMLElement).getByText('Other ASR')).toBeInTheDocument();
  });

  it('removes an added speech model through the provider route', async () => {
    mocks.lingxiFetch.mockResolvedValue({ json: async () => ({ ok: true }) });
    const onRefresh = vi.fn(async () => {});

    render(
      <ProviderSpeechModels
        runtimeProviderId="mimo"
        provider={{
          providerId: 'mimo',
          displayName: 'MiMo',
          hasCredentials: true,
          models: [{ id: 'mimo-v2.5-asr', name: 'MiMo ASR', adapterAvailable: true }],
        }}
        config={{ enabled: true }}
        onRefresh={onRefresh}
      />,
    );

    fireEvent.click(screen.getByTitle('settings.api.removeModel'));

    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith(
        '/api/speech-recognition/providers/mimo/models/mimo-v2.5-asr',
        expect.objectContaining({ method: 'DELETE' }),
      );
    });
    expect(onRefresh).toHaveBeenCalled();
  });
});
