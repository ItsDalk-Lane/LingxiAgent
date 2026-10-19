/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';

const mocks = vi.hoisted(() => ({
  lingxiFetch: vi.fn(),
}));

vi.mock('../../api', () => ({
  lingxiFetch: (...args: unknown[]) => mocks.lingxiFetch(...args),
}));

vi.mock('../../actions', () => ({
  updateSettingsSnapshot: vi.fn(),
}));

vi.mock('../../helpers', () => ({
  t: (key: string) => key,
  autoSaveGlobalModels: vi.fn(),
  lookupModelMeta: vi.fn(),
  formatContext: vi.fn(),
}));

vi.mock('../../components/SettingsSection', () => ({
  SettingsSection: ({ children }: { children: React.ReactNode }) => <section>{children}</section>,
}));

vi.mock('../../components/SettingsRow', () => ({
  SettingsRow: ({ label, control }: { label: string; control: React.ReactNode }) => (
    <label>
      <span>{label}</span>
      {control}
    </label>
  ),
}));

vi.mock('../providers/AuxiliaryModelsSection', () => ({
  AuxiliaryModelsSection: () => <div data-testid="auxiliary-models-section" />,
}));

vi.mock('@/ui', () => ({
  Toggle: ({ on, onChange, ariaLabel }: {
    on: boolean | undefined;
    onChange: (next: boolean) => void;
    ariaLabel?: string;
  }) => (
    <button
      type="button"
      role="switch"
      aria-label={ariaLabel}
      aria-checked={on === undefined ? 'mixed' : on}
      disabled={on === undefined}
      onClick={() => { if (on !== undefined) onChange(!on); }}
    />
  ),
  SelectWidget: ({ value, onChange, options, disabled }: {
    value: string;
    onChange: (value: string) => void;
    options: Array<{ value: string; label: string; disabled?: boolean }>;
    disabled?: boolean;
  }) => (
    <select value={value} disabled={disabled} onChange={(event) => onChange(event.target.value)}>
      {options.map(option => (
        <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>
      ))}
    </select>
  ),
}));

import { ModelsTab } from '../ModelsTab';
import { useSettingsStore } from '../../store';

function jsonResponse(body: unknown): Response {
  return { json: async () => body } as Response;
}

// 组件检查仅替代请求应答，验证实际页面和数据hook；真实Rust写入/消费由独立集成检查负责。
function installSpeechGenerationResponses(rejectWrite = false) {
  useSettingsStore.setState({ toastMessage: '', toastType: '', toastVisible: false });
  mocks.lingxiFetch.mockImplementation((path: string, init?: RequestInit) => {
    if (path === '/api/media/speech/providers') {
      return Promise.resolve(jsonResponse({
        providers: {
          'system-speech': {
            providerId: 'system-speech', displayName: 'System speech', hasCredentials: true,
            models: [
              { id: 'system-say-settings-model', name: 'System speech', adapterAvailable: true },
              { id: 'alternate-say-model', name: 'Alternate speech', adapterAvailable: true },
            ],
          },
        },
        config: { defaultSpeechModel: { provider: 'system-speech', id: 'system-say-settings-model' }, speed: 1.2 },
      }));
    }
    if (path === '/api/speech-recognition/providers') {
      return Promise.resolve(jsonResponse({
        providers: {
          'volcengine-speech': { providerId: 'volcengine-speech', displayName: 'BigASR', hasCredentials: true, models: [{ id: 'bigasr', adapterAvailable: true }] },
        },
        config: { enabled: true, defaultModel: { provider: 'volcengine-speech', id: 'bigasr' } },
      }));
    }
    if (path === '/api/media/speech/config' && init?.method === 'PUT') {
      if (rejectWrite) return Promise.reject(new Error('speech-generation-save-rejected'));
      const patch = JSON.parse(String(init.body)) as { values: { defaultSpeechModel: unknown } };
      return Promise.resolve(jsonResponse({ values: { speed: 1.2, ...patch.values } }));
    }
    return Promise.resolve(jsonResponse({ providers: {}, config: {} }));
  });
}

describe('ModelsTab', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.lingxiFetch.mockImplementation((path: string) => {
      if (path === '/api/media/image/providers') return Promise.resolve(jsonResponse({ providers: {}, config: {} }));
      if (path === '/api/media/video/providers') return Promise.resolve(jsonResponse({ providers: {}, config: {} }));
      if (path === '/api/speech-recognition/providers') return Promise.resolve(jsonResponse({ providers: {}, config: {} }));
      return Promise.resolve(jsonResponse({ values: {} }));
    });
  });

  afterEach(() => {
    cleanup();
  });

  it('renders auxiliary models section and global media defaults', async () => {
    render(<ModelsTab />);
    expect(screen.getByTestId('auxiliary-models-section')).toBeInTheDocument();
    expect(await screen.findByLabelText('settings.media.defaultModel')).toBeInTheDocument();
    expect(screen.getByLabelText('settings.media.defaultVideoModel')).toBeInTheDocument();
  });

  it('saves the default image model through the generic config envelope', async () => {
    mocks.lingxiFetch.mockImplementation((path: string) => {
      if (path === '/api/media/image/providers') {
        return Promise.resolve(jsonResponse({
          providers: { volcengine: { providerId: 'volcengine', displayName: 'Volcengine', hasCredentials: true, models: [{ id: 'seedream-5', name: 'Seedream 5.0' }], availableModels: [] } },
          config: {},
        }));
      }
      return Promise.resolve(jsonResponse({ values: {} }));
    });

    render(<ModelsTab />);
    const select = await screen.findByLabelText('settings.media.defaultModel');
    await waitFor(() => {
      expect(Array.from(select.querySelectorAll('option')).some(o => o.value === 'volcengine/seedream-5')).toBe(true);
    });
    fireEvent.change(select, { target: { value: 'volcengine/seedream-5' } });

    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/media/image/config', expect.objectContaining({ method: 'PUT' }));
    });
    const saveCall = mocks.lingxiFetch.mock.calls.find(([path]) => path === '/api/media/image/config');
    expect(JSON.parse(String((saveCall?.[1] as RequestInit).body))).toEqual({
      values: { defaultImageModel: { provider: 'volcengine', id: 'seedream-5' } },
    });
  });

  it('sends null to clear the default image model', async () => {
    mocks.lingxiFetch.mockImplementation((path: string) => {
      if (path === '/api/media/image/providers') {
        return Promise.resolve(jsonResponse({
          providers: { volcengine: { providerId: 'volcengine', displayName: 'Volcengine', hasCredentials: true, models: [{ id: 'seedream-5' }], availableModels: [] } },
          config: { defaultImageModel: { provider: 'volcengine', id: 'seedream-5' } },
        }));
      }
      return Promise.resolve(jsonResponse({ values: {} }));
    });

    render(<ModelsTab />);
    const select = await screen.findByLabelText('settings.media.defaultModel');
    fireEvent.change(select, { target: { value: '' } });

    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/media/image/config', expect.objectContaining({
        body: JSON.stringify({ values: { defaultImageModel: null } }),
      }));
    });
  });

  it('does not offer image models with missing runtime adapters as selectable defaults', async () => {
    mocks.lingxiFetch.mockImplementation((path: string) => {
      if (path === '/api/media/image/providers') {
        return Promise.resolve(jsonResponse({
          providers: { axis: { providerId: 'axis', displayName: 'Axis', hasCredentials: true, models: [{ id: 'gpt-image-2', name: 'GPT Image 2', adapterAvailable: false }], availableModels: [] } },
          config: {},
        }));
      }
      return Promise.resolve(jsonResponse({ values: {} }));
    });

    render(<ModelsTab />);
    const select = await screen.findByLabelText('settings.media.defaultModel');
    const option = Array.from(select.querySelectorAll('option')).find((item) => item.value === 'axis/gpt-image-2');
    expect(option).toBeDisabled();
    expect(option?.textContent).toContain('settings.media.adapterMissing');
  });

  it('surfaces runtime CLI discovery errors on model choices', async () => {
    mocks.lingxiFetch.mockImplementation((path: string) => {
      if (path === '/api/media/image/providers') {
        return Promise.resolve(jsonResponse({
          providers: { 'jimeng-cli': { providerId: 'jimeng-cli', displayName: '即梦 CLI', hasCredentials: false, unavailableReason: 'output_unparseable', unavailableMessage: 'Dreamina CLI help changed', runtimeCapability: { status: 'stale' }, models: [{ id: 'jimeng-image-5.0', name: '即梦图片 5.0', adapterAvailable: true }], availableModels: [] } },
          config: {},
        }));
      }
      return Promise.resolve(jsonResponse({ values: {} }));
    });

    render(<ModelsTab />);
    const select = await screen.findByLabelText('settings.media.defaultModel');
    const option = Array.from(select.querySelectorAll('option')).find((item) => item.value === 'jimeng-cli/jimeng-image-5.0');
    expect(option).toBeDisabled();
    expect(option?.textContent).toContain('Dreamina CLI help changed');
  });

  it('refreshes runtime CLI model choices when the app regains focus', async () => {
    let imageLoads = 0;
    mocks.lingxiFetch.mockImplementation((path: string) => {
      if (path === '/api/media/image/providers') {
        imageLoads += 1;
        const modelId = imageLoads === 1 ? 'jimeng-image-4.7' : 'jimeng-image-5.0';
        return Promise.resolve(jsonResponse({
          providers: { 'jimeng-cli': { providerId: 'jimeng-cli', displayName: '即梦 CLI', hasCredentials: true, runtimeCapability: { status: 'ready' }, models: [{ id: modelId, name: modelId, adapterAvailable: true }], availableModels: [] } },
          config: {},
        }));
      }
      return Promise.resolve(jsonResponse({ providers: {}, config: {} }));
    });

    render(<ModelsTab />);
    const select = await screen.findByLabelText('settings.media.defaultModel');
    await waitFor(() => {
      expect(Array.from(select.querySelectorAll('option')).some(option => option.value.endsWith('/jimeng-image-4.7'))).toBe(true);
    });

    window.dispatchEvent(new Event('focus'));

    await waitFor(() => {
      expect(Array.from(select.querySelectorAll('option')).some(option => option.value.endsWith('/jimeng-image-5.0'))).toBe(true);
    });
    expect(imageLoads).toBeGreaterThanOrEqual(2);
  });

  it('saves the default video model through the video config endpoint', async () => {
    mocks.lingxiFetch.mockImplementation((path: string) => {
      if (path === '/api/media/video/providers') {
        return Promise.resolve(jsonResponse({
          providers: { agnes: { providerId: 'agnes', displayName: 'Agnes AI', hasCredentials: true, models: [{ id: 'agnes-video-v2.0', name: 'Agnes Video V2.0', adapterAvailable: true }], availableModels: [] } },
          config: {},
        }));
      }
      return Promise.resolve(jsonResponse({ values: {} }));
    });

    render(<ModelsTab />);
    const select = await screen.findByLabelText('settings.media.defaultVideoModel');
    fireEvent.change(select, { target: { value: 'agnes/agnes-video-v2.0' } });

    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/media/video/config', expect.objectContaining({
        method: 'PUT',
        body: JSON.stringify({ values: { defaultVideoModel: { provider: 'agnes', id: 'agnes-video-v2.0' } } }),
      }));
    });
  });

  it('saves speech-recognition enabled state and default model', async () => {
    mocks.lingxiFetch.mockImplementation((path: string) => {
      if (path === '/api/speech-recognition/providers') {
        return Promise.resolve(jsonResponse({
          providers: { openai: { providerId: 'openai', displayName: 'OpenAI Speech', hasCredentials: true, models: [{ id: 'whisper-1', name: 'Whisper 1', adapterAvailable: true }] } },
          config: { enabled: true },
        }));
      }
      return Promise.resolve(jsonResponse({ values: {} }));
    });

    render(<ModelsTab />);
    const toggle = await screen.findByRole('switch', { name: '发送语音条时转录' });
    fireEvent.click(toggle);

    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/speech-recognition/config', expect.objectContaining({
        method: 'PUT',
        body: JSON.stringify({ values: { enabled: false } }),
      }));
    });

    const select = await screen.findByLabelText('语音条转录模型');
    fireEvent.change(select, { target: { value: 'openai/whisper-1' } });

    await waitFor(() => {
      expect(mocks.lingxiFetch.mock.calls.some(([path]) => path === '/api/speech-recognition/config')).toBe(true);
    });
  });

  it('does not offer speech models without runnable adapters as selectable defaults', async () => {
    mocks.lingxiFetch.mockImplementation((path: string) => {
      if (path === '/api/speech-recognition/providers') {
        return Promise.resolve(jsonResponse({
          providers: { openai: { providerId: 'openai', displayName: 'OpenAI Speech', hasCredentials: true, models: [{ id: 'whisper-1', name: 'Whisper 1', adapterAvailable: false }], availableModels: [] } },
          config: { enabled: true },
        }));
      }
      return Promise.resolve(jsonResponse({ values: {} }));
    });

    render(<ModelsTab />);
    const select = await screen.findByLabelText('语音条转录模型');
    const option = Array.from(select.querySelectorAll('option')).find((item) => item.value === 'openai/whisper-1');
    expect(option).toBeDisabled();
    expect(option).toHaveTextContent('settings.media.adapterMissing');
  });

  it('saves the speechGeneration default through the speech config envelope and keeps ASR independent', async () => {
    installSpeechGenerationResponses();
    render(<ModelsTab />);
    const speech = await screen.findByLabelText('语音合成模型（朗读/语音消息）');
    const asr = screen.getByLabelText('语音条转录模型');
    await waitFor(() => {
      expect(speech).toHaveValue('system-speech/system-say-settings-model');
      expect(asr).toHaveValue('volcengine-speech/bigasr');
    });
    fireEvent.change(speech, { target: { value: 'system-speech/alternate-say-model' } });
    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/media/speech/config', expect.objectContaining({
        method: 'PUT',
        body: JSON.stringify({ values: { defaultSpeechModel: { provider: 'system-speech', id: 'alternate-say-model' } } }),
      }));
      expect(speech).toHaveValue('system-speech/alternate-say-model');
      expect(useSettingsStore.getState().toastType).toBe('success');
    });
    expect(asr).toHaveValue('volcengine-speech/bigasr');
    expect(mocks.lingxiFetch.mock.calls.filter(([path, init]) => path === '/api/speech-recognition/config' && (init as RequestInit)?.method === 'PUT')).toHaveLength(0);
  });

  it('clears defaultSpeechModel with explicit null and keeps the speech-recognition default', async () => {
    installSpeechGenerationResponses();
    render(<ModelsTab />);
    const speech = await screen.findByLabelText('语音合成模型（朗读/语音消息）');
    await waitFor(() => expect(speech).toHaveValue('system-speech/system-say-settings-model'));
    fireEvent.change(speech, { target: { value: '' } });
    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/media/speech/config', expect.objectContaining({
        method: 'PUT', body: JSON.stringify({ values: { defaultSpeechModel: null } }),
      }));
      expect(speech).toHaveValue('');
      expect(useSettingsStore.getState().toastType).toBe('success');
    });
    expect(screen.getByLabelText('语音条转录模型')).toHaveValue('volcengine-speech/bigasr');
    expect(mocks.lingxiFetch.mock.calls.filter(([path, init]) => path === '/api/speech-recognition/config' && (init as RequestInit)?.method === 'PUT')).toHaveLength(0);
  });

  it.each([
    { name: 'selection', value: 'system-speech/alternate-say-model', patch: { provider: 'system-speech', id: 'alternate-say-model' } },
    { name: 'clear', value: '', patch: null },
  ])('keeps the saved speech default after a failed $name and exposes the error', async ({ value, patch }) => {
    installSpeechGenerationResponses(true);
    render(<ModelsTab />);
    const speech = await screen.findByLabelText('语音合成模型（朗读/语音消息）');
    await waitFor(() => expect(speech).toHaveValue('system-speech/system-say-settings-model'));
    fireEvent.change(speech, { target: { value } });
    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/media/speech/config', expect.objectContaining({
        method: 'PUT', body: JSON.stringify({ values: { defaultSpeechModel: patch } }),
      }));
      expect(useSettingsStore.getState().toastType).toBe('error');
      expect(useSettingsStore.getState().toastMessage).toBe('speech-generation-save-rejected');
      expect(speech).toHaveValue('system-speech/system-say-settings-model');
    });
    expect(screen.getByLabelText('语音条转录模型')).toHaveValue('volcengine-speech/bigasr');
    expect(mocks.lingxiFetch.mock.calls.filter(([path, init]) => path === '/api/speech-recognition/config' && (init as RequestInit)?.method === 'PUT')).toHaveLength(0);
  });

});
