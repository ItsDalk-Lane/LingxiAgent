import { describe, expect, it } from 'vitest';
import { getApiKeySavePlan } from '../../settings/tabs/providers/api-key-save-plan';

describe('getApiKeySavePlan', () => {
  it('allows clearing an edited api key without forcing remote verification', () => {
    expect(getApiKeySavePlan({
      keyEdited: true,
      keyVal: '',
      urlEdited: false,
      urlVal: 'https://api.example.com/v1',
      derivedBaseUrl: 'https://api.example.com/v1',
      isPresetSetup: false,
      isLocalPreset: false,
      api: 'openai-completions',
    })).toEqual({
      shouldSave: true,
      shouldVerify: false,
      payload: { api_key: '' },
      effectiveUrl: 'https://api.example.com/v1',
      api: 'openai-completions',
      key: '',
    });
  });

  it('preset setup saves credentials with an explicit empty model list instead of seeding defaults', () => {
    expect(getApiKeySavePlan({
      keyEdited: true,
      keyVal: 'sk-test',
      urlEdited: false,
      urlVal: '',
      derivedBaseUrl: 'https://api.xiaomimimo.com/v1',
      isPresetSetup: true,
      isLocalPreset: false,
      api: 'openai-completions',
    }).payload).toEqual({
      base_url: 'https://api.xiaomimimo.com/v1',
      api_key: 'sk-test',
      api: 'openai-completions',
      models: [],
    });
  });

  it('saving an existing provider touches only the api key', () => {
    expect(getApiKeySavePlan({
      keyEdited: true,
      keyVal: 'sk-test',
      urlEdited: false,
      urlVal: 'https://api.xiaomimimo.com/v1',
      derivedBaseUrl: 'https://api.xiaomimimo.com/v1',
      isPresetSetup: false,
      isLocalPreset: false,
      api: 'openai-completions',
    }).payload).toEqual({
      api_key: 'sk-test',
    });
  });
});
