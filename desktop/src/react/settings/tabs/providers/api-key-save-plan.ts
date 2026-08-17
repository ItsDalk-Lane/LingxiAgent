export interface ApiKeySavePlanInput {
  keyEdited: boolean;
  keyVal: string;
  urlEdited: boolean;
  urlVal: string;
  derivedBaseUrl: string;
  isPresetSetup: boolean;
  isLocalPreset: boolean;
  api: string;
}

export interface ApiKeySavePlan {
  shouldSave: boolean;
  shouldVerify: boolean;
  payload: Record<string, unknown>;
  effectiveUrl: string;
  api: string;
  key: string;
}

export function getApiKeySavePlan(input: ApiKeySavePlanInput): ApiKeySavePlan {
  const key = input.keyVal.trim();
  const effectiveUrl = input.urlVal.trim() || input.derivedBaseUrl;

  if (!input.keyEdited) {
    return {
      shouldSave: false,
      shouldVerify: false,
      payload: {},
      effectiveUrl,
      api: input.api,
      key,
    };
  }

  // 首次 preset 保存写显式 models: []（三态语义中的「明确关闭」）：
  // 内置默认模型列表不再出现，「已添加的模型」只由用户添加或「读取模型」填充。
  const payload: Record<string, unknown> = input.isPresetSetup
    ? { base_url: effectiveUrl, api_key: key, api: input.api, models: [] }
    : { api_key: key };

  if (input.urlEdited && !input.isPresetSetup) {
    payload.base_url = effectiveUrl;
  }

  return {
    shouldSave: true,
    shouldVerify: key !== "" || input.isLocalPreset,
    payload,
    effectiveUrl,
    api: input.api,
    key,
  };
}
