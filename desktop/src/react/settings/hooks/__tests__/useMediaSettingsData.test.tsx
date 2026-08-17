/**
 * @vitest-environment jsdom
 */

import { renderHook, waitFor, act } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useSettingsStore } from '../../store';

const mocks = vi.hoisted(() => ({
  lingxiFetch: vi.fn(),
  updateSettingsSnapshot: vi.fn(),
}));

vi.mock('../../api', () => ({
  lingxiFetch: (...args: unknown[]) => mocks.lingxiFetch(...args),
}));

vi.mock('../../actions', () => ({
  updateSettingsSnapshot: (mutator: (snapshot: any) => any) => mocks.updateSettingsSnapshot(mutator),
}));

vi.mock('../../helpers', () => ({
  t: (key: string) => key,
}));

import { useMediaSettingsData } from '../useMediaSettingsData';

function jsonResponse(body: unknown): Response {
  return { json: async () => body } as Response;
}

describe('useMediaSettingsData', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useSettingsStore.setState({
      settingsSnapshot: {
        key: 'snapshot:agent-a',
        status: 'ready',
        data: {
          preferences: { speechRecognition: { enabled: false } },
        },
        error: null,
        requestId: 1,
        updatedAt: Date.now(),
      } as any,
      toastMessage: '',
      toastType: '',
      toastVisible: false,
    });
    mocks.lingxiFetch.mockImplementation((path: string) => {
      if (path === '/api/media/image/providers') {
        return Promise.resolve(jsonResponse({ providers: {}, config: {} }));
      }
      if (path === '/api/media/video/providers') {
        return Promise.resolve(jsonResponse({ providers: {}, config: {} }));
      }
      if (path === '/api/speech-recognition/providers') {
        return Promise.resolve(jsonResponse({ providers: {}, config: { enabled: false } }));
      }
      return Promise.resolve(jsonResponse({ values: {} }));
    });
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('loads image, video, and speech resources independently on mount', async () => {
    renderHook(() => useMediaSettingsData());

    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/media/image/providers');
      expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/media/video/providers');
      expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/speech-recognition/providers');
    });
  });

  it('does not fail the whole resource layer when one endpoint rejects', async () => {
    mocks.lingxiFetch.mockImplementation((path: string) => {
      if (path === '/api/media/video/providers') return Promise.reject(new Error('video down'));
      if (path === '/api/speech-recognition/providers') return Promise.resolve(jsonResponse({ providers: {}, config: { enabled: false } }));
      return Promise.resolve(jsonResponse({
        providers: { agnes: { providerId: 'agnes', displayName: 'Agnes', hasCredentials: true, models: [{ id: 'a1' }], availableModels: [] } },
        config: {},
      }));
    });

    const { result } = renderHook(() => useMediaSettingsData());

    await waitFor(() => {
      expect(result.current.image.providers.agnes).toBeTruthy();
    });
    expect(result.current.video.error).toBe('video down');
  });

  it('keeps last-known-good providers when a refresh fails', async () => {
    let failImage = false;
    mocks.lingxiFetch.mockImplementation((path: string) => {
      if (path === '/api/media/image/providers') {
        if (failImage) return Promise.reject(new Error('cli upgraded'));
        return Promise.resolve(jsonResponse({
          providers: { 'jimeng-cli': { providerId: 'jimeng-cli', displayName: '即梦 CLI', hasCredentials: true, models: [{ id: 'm1' }], availableModels: [] } },
          config: {},
        }));
      }
      return Promise.resolve(jsonResponse({ providers: {}, config: {} }));
    });

    const { result } = renderHook(() => useMediaSettingsData());
    await waitFor(() => expect(result.current.image.providers['jimeng-cli']).toBeTruthy());

    failImage = true;
    await act(async () => {
      await result.current.refreshImage();
    });

    // 刷新失败不清空已有 provider 列表。
    expect(result.current.image.providers['jimeng-cli']).toBeTruthy();
    expect(result.current.image.error).toBe('cli upgraded');
  });

  it('sends null (not {}) to clear the default image model', async () => {
    mocks.lingxiFetch.mockImplementation((path: string) => {
      if (path === '/api/media/image/providers') return Promise.resolve(jsonResponse({ providers: {}, config: {} }));
      if (path === '/api/media/video/providers') return Promise.resolve(jsonResponse({ providers: {}, config: {} }));
      if (path === '/api/speech-recognition/providers') return Promise.resolve(jsonResponse({ providers: {}, config: {} }));
      return Promise.resolve(jsonResponse({ values: {} }));
    });

    const { result } = renderHook(() => useMediaSettingsData());
    await waitFor(() => expect(result.current.image.loading).toBe(false));

    await act(async () => {
      await result.current.saveImageConfig({ defaultImageModel: undefined });
    });

    expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/media/image/config', expect.objectContaining({
      method: 'PUT',
      body: JSON.stringify({ values: { defaultImageModel: null } }),
    }));
  });

  it('syncs the speech snapshot preference after saving', async () => {
    mocks.lingxiFetch.mockImplementation((path: string) => {
      if (path === '/api/media/image/providers') return Promise.resolve(jsonResponse({ providers: {}, config: {} }));
      if (path === '/api/media/video/providers') return Promise.resolve(jsonResponse({ providers: {}, config: {} }));
      if (path === '/api/speech-recognition/providers') return Promise.resolve(jsonResponse({ providers: {}, config: { enabled: false } }));
      if (path === '/api/speech-recognition/config') return Promise.resolve(jsonResponse({ values: { enabled: true } }));
      return Promise.resolve(jsonResponse({ values: {} }));
    });

    const { result } = renderHook(() => useMediaSettingsData());
    await waitFor(() => expect(result.current.speech.loading).toBe(false));

    await act(async () => {
      await result.current.saveSpeechConfig({ enabled: true });
    });

    expect(mocks.updateSettingsSnapshot).toHaveBeenCalled();
    const mutator = mocks.updateSettingsSnapshot.mock.calls[0][0];
    const next = mutator({ preferences: { speechRecognition: { enabled: false } } });
    expect(next.preferences.speechRecognition).toMatchObject({ enabled: true });
  });

  it('serializes writes so a slower earlier response cannot overwrite a later save', async () => {
    let resolveFirst: (r: Response) => void = () => {};
    let resolveSecond: (r: Response) => void = () => {};
    const configCalls: Array<{ patch: Record<string, unknown>; resolve: (r: Response) => void }> = [];
    mocks.lingxiFetch.mockImplementation((path: string, opts?: RequestInit) => {
      if (path === '/api/media/image/providers') return Promise.resolve(jsonResponse({ providers: {}, config: {} }));
      if (path === '/api/media/video/providers') return Promise.resolve(jsonResponse({ providers: {}, config: {} }));
      if (path === '/api/speech-recognition/providers') return Promise.resolve(jsonResponse({ providers: {}, config: {} }));
      if (path === '/api/media/image/config') {
        const body = JSON.parse(String((opts as RequestInit).body));
        return new Promise<Response>((resolve) => {
          configCalls.push({ patch: body.values, resolve });
        });
      }
      return Promise.resolve(jsonResponse({ values: {} }));
    });

    const { result } = renderHook(() => useMediaSettingsData());
    await waitFor(() => expect(result.current.image.loading).toBe(false));

    const p1 = result.current.saveImageConfig({ defaultImageModel: { provider: 'a', id: 'm1' } });
    await waitFor(() => expect(configCalls).toHaveLength(1));
    const p2 = result.current.saveImageConfig({ defaultImageModel: { provider: 'b', id: 'm2' } });

    // 先完成第一次写入（慢），再触发第二次。第二次必须在第一次之后排队提交。
    configCalls[0].resolve(jsonResponse({ values: { defaultImageModel: { provider: 'a', id: 'm1' } } }));
    await p1;

    await waitFor(() => expect(configCalls).toHaveLength(2));
    configCalls[1].resolve(jsonResponse({ values: { defaultImageModel: { provider: 'b', id: 'm2' } } }));
    await p2;

    await waitFor(() => {
      expect(result.current.image.config?.defaultImageModel).toEqual({ provider: 'b', id: 'm2' });
    });
    expect(configCalls[1].patch).toEqual({ defaultImageModel: { provider: 'b', id: 'm2' } });
  });
});
