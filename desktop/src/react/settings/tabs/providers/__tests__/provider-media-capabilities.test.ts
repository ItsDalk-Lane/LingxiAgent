import { describe, expect, it } from 'vitest';
import { resolveProviderMediaCapabilities } from '../provider-media-capabilities';
import type { ProviderMediaCapabilityBinding } from '../../../store';
import type { UseMediaSettingsDataResult } from '../../../hooks/useMediaSettingsData';

function media(): UseMediaSettingsDataResult {
  return {
    image: {
      providers: {
        agnes: { providerId: 'agnes', displayName: 'Agnes AI', hasCredentials: true, models: [], availableModels: [] },
        minimax: { providerId: 'minimax', displayName: 'MiniMax', hasCredentials: true, models: [], availableModels: [] },
      },
      config: {},
      loading: false,
      error: null,
    },
    video: {
      providers: {
        agnes: { providerId: 'agnes', displayName: 'Agnes AI', hasCredentials: true, models: [], availableModels: [] },
      },
      config: {},
      loading: false,
      error: null,
    },
    speech: {
      providers: {
        'volcengine-speech': { providerId: 'volcengine-speech', displayName: '火山引擎语音 (BigASR)', hasCredentials: true, models: [], availableModels: [] },
      },
      config: { enabled: false },
      loading: false,
      error: null,
    },
    allImageModels: [],
    allVideoModels: [],
    allSpeechModels: [],
    speechEnabled: false,
    refreshImage: async () => {},
    refreshVideo: async () => {},
    refreshSpeech: async () => {},
    refreshAll: async () => {},
    saveImageConfig: async () => {},
    saveVideoConfig: async () => {},
    saveSpeechConfig: async () => {},
  };
}

describe('resolveProviderMediaCapabilities', () => {
  it('resolves agnes image and video capabilities to their runtime providers', () => {
    const bindings: ProviderMediaCapabilityBinding[] = [
      { capability: 'imageGeneration', runtime_provider_id: 'agnes' },
      { capability: 'videoGeneration', runtime_provider_id: 'agnes' },
    ];

    const resolved = resolveProviderMediaCapabilities(bindings, media());

    expect(resolved).toEqual([
      expect.objectContaining({ capability: 'imageGeneration', runtimeProviderId: 'agnes', available: true }),
      expect.objectContaining({ capability: 'videoGeneration', runtimeProviderId: 'agnes', available: true }),
    ]);
  });

  it('maps minimax-token-plan to the minimax runtime provider through its credential lane', () => {
    const bindings: ProviderMediaCapabilityBinding[] = [
      { capability: 'imageGeneration', runtime_provider_id: 'minimax', credential_lane_id: 'minimax-token-plan' },
    ];

    const resolved = resolveProviderMediaCapabilities(bindings, media());

    expect(resolved[0]).toMatchObject({
      capability: 'imageGeneration',
      runtimeProviderId: 'minimax',
      credentialLaneId: 'minimax-token-plan',
      available: true,
    });
    // runtime media 操作目标是 minimax，不是 credential provider。
    expect(resolved[0].runtimeProviderId).toBe('minimax');
  });

  it('maps volcengine-speech to itself and never to volcengine', () => {
    const bindings: ProviderMediaCapabilityBinding[] = [
      { capability: 'speechRecognition', runtime_provider_id: 'volcengine-speech' },
    ];

    const resolved = resolveProviderMediaCapabilities(bindings, media());

    expect(resolved[0]).toMatchObject({
      capability: 'speechRecognition',
      runtimeProviderId: 'volcengine-speech',
    });
    expect(resolved[0].runtimeProviderId).not.toBe('volcengine');
  });

  it('keeps a declared capability visible (available=false) when its endpoint is still loading', () => {
    const bindings: ProviderMediaCapabilityBinding[] = [
      { capability: 'imageGeneration', runtime_provider_id: 'agnes' },
    ];
    const state = media();
    state.image.loading = true;
    state.image.providers = {};

    const resolved = resolveProviderMediaCapabilities(bindings, state);

    expect(resolved[0]).toMatchObject({
      capability: 'imageGeneration',
      available: false,
      loading: true,
    });
  });

  it('returns [] for a provider with no media capability', () => {
    expect(resolveProviderMediaCapabilities(undefined, media())).toEqual([]);
    expect(resolveProviderMediaCapabilities([], media())).toEqual([]);
  });
});
