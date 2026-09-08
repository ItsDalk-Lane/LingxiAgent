function isObject(value) {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function clone(value) {
  return value === undefined ? undefined : structuredClone(value);
}

function text(value) {
  return typeof value === "string" && value.trim() ? value.trim() : "";
}

function compactObject(value) {
  const out: Record<string, any> = {};
  for (const [key, item] of Object.entries(value || {})) {
    if (item !== undefined && item !== null && item !== "") out[key] = item;
  }
  return out;
}

function imageCount(input: any = {}) {
  const raw = input.referenceImages || input.images || input.image;
  if (!raw) return 0;
  const images = Array.isArray(raw) ? raw : [raw];
  return images.filter((item) => {
    if (typeof item === "string") return item.trim();
    return isObject(item);
  }).length;
}

export function inferMediaMode(kind, input: any = {}) {
  const requested = text(input.mode) || text(input.options?.mode);
  if (requested) return requested;
  const count = imageCount(input);
  if (kind === "video") {
    if (count > 1) return "multiframe2video";
    if (count === 1) return "image2video";
    return "text2video";
  }
  if (count > 0) return "image2image";
  return "text2image";
}

function findMode(model: any = {}, modeId) {
  if (!Array.isArray(model?.modes) || model.modes.length === 0) return null;
  return model.modes.find((mode) => mode?.id === modeId) || null;
}

function resolveInputLimits(model: any = {}, mode: any = null) {
  if (isObject(mode?.inputLimits)) return mode.inputLimits;
  if (isObject(model?.inputLimits)) return model.inputLimits;
  return null;
}

function withoutNestedDefaults(value: any = {}) {
  const out: Record<string, any> = {};
  for (const [key, item] of Object.entries(value || {})) {
    if (key === "models" || key === "modes" || key === "options") continue;
    out[key] = item;
  }
  return out;
}

function filterDefaultsBySchema(defaults: any = {}, schema: any = null) {
  if (!isObject(defaults)) return {};
  if (!isObject(schema?.properties)) return defaults;
  const props = schema.properties;
  const out: Record<string, any> = {};
  for (const [key, value] of Object.entries(defaults)) {
    if (Object.prototype.hasOwnProperty.call(props, key)) out[key] = value;
  }
  return out;
}

function providerDefaultsForMode(providerDefaults: any = {}, modelId, modeId) {
  if (!isObject(providerDefaults)) return {};
  const modelDefaults = isObject(providerDefaults.models?.[modelId])
    ? providerDefaults.models[modelId]
    : {};
  const modeDefaults = isObject(modelDefaults.modes?.[modeId])
    ? modelDefaults.modes[modeId]
    : isObject(providerDefaults.modes?.[modeId])
      ? providerDefaults.modes[modeId]
      : {};
  return {
    ...withoutNestedDefaults(providerDefaults),
    ...(isObject(providerDefaults.options) ? providerDefaults.options : {}),
    ...withoutNestedDefaults(modelDefaults),
    ...(isObject(modelDefaults.options) ? modelDefaults.options : {}),
    ...withoutNestedDefaults(modeDefaults),
    ...(isObject(modeDefaults.options) ? modeDefaults.options : {}),
  };
}

function explicitParameters(kind, input: any = {}, schema: any = {}) {
  const props = isObject(schema?.properties) ? schema.properties : {};
  const out: Record<string, any> = {};
  const add = (key, value) => {
    if (value !== undefined && value !== null && value !== "") out[key] = value;
  };
  add("duration", input.duration ?? input.seconds);
  add("ratio", input.ratio ?? input.aspect_ratio ?? input.aspectRatio);
  add("quality", input.quality);
  add("frame_rate", input.frame_rate ?? input.frameRate);
  add("num_frames", input.num_frames ?? input.numFrames);
  add("seed", input.seed);
  if (kind === "video") {
    add("video_resolution", input.video_resolution ?? input.videoResolution);
    if (input.resolution !== undefined) {
      if (props.video_resolution) add("video_resolution", input.resolution);
      else add("resolution", input.resolution);
    }
  } else {
    add("resolution", input.resolution);
    add("resolution_type", input.resolution_type ?? input.resolutionType);
    add("size", input.size);
    add("format", input.format);
  }
  return out;
}

function typeMatches(type, value) {
  if (!type) return true;
  if (Array.isArray(type)) return type.some((item) => typeMatches(item, value));
  if (type === "number") return typeof value === "number" && Number.isFinite(value);
  if (type === "integer") return Number.isInteger(value);
  if (type === "string") return typeof value === "string";
  if (type === "boolean") return typeof value === "boolean";
  if (type === "array") return Array.isArray(value);
  if (type === "object") return isObject(value);
  return true;
}

function validateSchemaValue(key, value, schema: any = {}) {
  if (value === undefined || value === null) return;
  if (!typeMatches(schema.type, value)) {
    throw new Error(`Media parameter "${key}" must be ${Array.isArray(schema.type) ? schema.type.join(" or ") : schema.type}`);
  }
  if (Array.isArray(schema.enum) && !schema.enum.includes(value)) {
    throw new Error(`Media parameter "${key}" must be one of: ${schema.enum.join(", ")}`);
  }
  if (typeof value === "number") {
    if (typeof schema.minimum === "number" && value < schema.minimum) {
      throw new Error(`Media parameter "${key}" must be >= ${schema.minimum}`);
    }
    if (typeof schema.maximum === "number" && value > schema.maximum) {
      throw new Error(`Media parameter "${key}" must be <= ${schema.maximum}`);
    }
  }
}

export function validateMediaParameters(parameters: any = {}, schema: any = {}) {
  if (!isObject(schema) || !isObject(schema.properties)) return;
  for (const [key, value] of Object.entries(parameters || {})) {
    const propertySchema = schema.properties[key];
    if (!propertySchema) continue;
    validateSchemaValue(key, value, propertySchema);
  }
}

function applyExplicitImageSizePrecedence(parameters: any = {}, explicit: any = {}) {
  if (!isObject(parameters) || !isObject(explicit)) return parameters;
  const hasExplicitResolution = Object.prototype.hasOwnProperty.call(explicit, "resolution");
  const hasExplicitSize = Object.prototype.hasOwnProperty.call(explicit, "size");
  const hasExplicitResolutionType = Object.prototype.hasOwnProperty.call(explicit, "resolution_type");

  if (hasExplicitResolution && !hasExplicitSize) delete parameters.size;
  if (hasExplicitResolution && !hasExplicitResolutionType) delete parameters.resolution_type;
  if (hasExplicitSize && !hasExplicitResolution) delete parameters.resolution;
  if (hasExplicitSize && !hasExplicitResolutionType) delete parameters.resolution_type;
  if (hasExplicitResolutionType && !hasExplicitResolution) delete parameters.resolution;
  if (hasExplicitResolutionType && !hasExplicitSize) delete parameters.size;
  return parameters;
}

function validateReferenceImageLimits({
  input = {},
  inputLimits = null,
  providerId = "",
  modelId = "",
  modeId = "",
}: any = {}) {
  if (!isObject(inputLimits?.referenceImages)) return;
  const limits = inputLimits.referenceImages;
  const count = imageCount(input);
  const label = `${providerId}/${modelId}`.replace(/^\/|\/$/g, "") || "selected media model";
  if (typeof limits.max === "number" && count > limits.max) {
    if (limits.max === 0) {
      throw new Error(`Media model "${label}" mode "${modeId}" does not support reference images`);
    }
    throw new Error(`Media model "${label}" mode "${modeId}" supports at most ${limits.max} reference images`);
  }
  if (typeof limits.min === "number" && count < limits.min) {
    throw new Error(`Media model "${label}" mode "${modeId}" requires at least ${limits.min} reference images`);
  }
}

export function resolveMediaParameters({
  kind,
  input = {},
  providerId = "",
  model = null,
  providerDefaults = {},
}: any = {}) {
  const modelId = text(model?.id) || text(input.modelId) || text(input.model);
  const modeId = inferMediaMode(kind, input);
  const mode = findMode(model, modeId);
  if (Array.isArray(model?.modes) && model.modes.length > 0 && !mode) {
    throw new Error(`Media model "${providerId}/${modelId}" does not support mode "${modeId}"`);
  }
  const parameterSchema = mode?.parameterSchema || model?.parameterSchema || null;
  const inputLimits = resolveInputLimits(model, mode);
  validateReferenceImageLimits({ input, inputLimits, providerId, modelId, modeId });
  const explicit = explicitParameters(kind, input, parameterSchema);
  const inheritedDefaults = providerDefaultsForMode(providerDefaults, modelId, modeId);
  const resolvedParameters = compactObject({
    ...(isObject(mode?.defaults) ? clone(mode.defaults) : {}),
    ...filterDefaultsBySchema(inheritedDefaults, parameterSchema),
    ...(isObject(input.options) ? input.options : {}),
    ...explicit,
  });
  if (kind === "image") applyExplicitImageSizePrecedence(resolvedParameters, explicit);
  validateMediaParameters(resolvedParameters, parameterSchema);
  return {
    modeId,
    mode: mode ? clone(mode) : null,
    parameterSchema: clone(parameterSchema),
    inputLimits: clone(inputLimits),
    resolvedParameters,
  };
}

const OPENAI_SPEECH_FORMATS = new Set(["mp3", "opus", "aac", "flac", "wav", "pcm"]);
const MINIMAX_SPEECH_FORMATS = new Set(["mp3", "wav", "pcm", "flac"]);
export const EFFECTIVE_SPEECH_PARAMETERS_VERSION = 1;

export type EffectiveSpeechParameters = Readonly<{
  effectiveSpeechParametersVersion: 1;
  protocolId: string;
  modelId: string;
  voice?: string;
  voiceMode?: "explicit" | "configured" | "protocol_default" | "system_default";
  speed?: number;
  rateWpm?: number;
  rateMode?: "explicit" | "configured" | "system_default";
  format?: string;
  formatMode?: "explicit" | "configured" | "protocol_default" | "protocol_output_default";
  groupId?: string;
}>;

function selectedValue(explicit: Record<string, unknown>, defaults: Record<string, unknown>, key: string) {
  return explicit[key] === undefined || explicit[key] === null ? defaults[key] : explicit[key];
}

function selectedMode(explicit: Record<string, unknown>, defaults: Record<string, unknown>, key: string) {
  if (explicit[key] !== undefined && explicit[key] !== null) return "explicit" as const;
  if (defaults[key] !== undefined && defaults[key] !== null) return "configured" as const;
  return null;
}

function normalizedText(value: unknown, fallback: string): string {
  return typeof value === "string" && value.trim() ? value.trim() : fallback;
}

function normalizedSpeed(value: unknown, fallback: number, min: number, max: number): number {
  if (typeof value !== "number" || !Number.isFinite(value)) return fallback;
  return Math.min(max, Math.max(min, value));
}

function targetText(target: Record<string, any>, key: string): string {
  return typeof target[key] === "string" && target[key].trim() ? target[key].trim() : "";
}

/**
 * R08：语音参数的唯一解析入口。这里先选择本次输入或 speech 域默认，再按实际
 * 协议归一化；返回值已冻结，可直接交给 adapter、TaskStore 和语义观测。
 * modelId 只读取 executionTarget，绝不读取 providerDefaults.model。
 */
export function resolveSpeechParameters({
  protocolId = "",
  executionTarget = {},
  explicitInput,
  speechProviderDefaults,
  // 兼容旧调用名；manager 已改用上面的明确字段。
  input,
  providerDefaults,
}: {
  protocolId?: string;
  executionTarget?: Record<string, unknown>;
  explicitInput?: Record<string, unknown>;
  speechProviderDefaults?: Record<string, unknown>;
  input?: Record<string, unknown>;
  providerDefaults?: Record<string, unknown>;
} = {}): EffectiveSpeechParameters {
  const explicit = isObject(explicitInput) ? explicitInput : (isObject(input) ? input : {});
  const defaults = isObject(speechProviderDefaults)
    ? speechProviderDefaults
    : (isObject(providerDefaults) ? providerDefaults : {});
  const target = isObject(executionTarget) ? executionTarget : {};
  const protocol = text(protocolId) || targetText(target, "protocolId");
  const modelId = targetText(target, "modelId");
  const selectedVoice = selectedValue(explicit, defaults, "voice");
  const selectedSpeed = selectedValue(explicit, defaults, "speed");
  const selectedFormat = selectedValue(explicit, defaults, "format");
  const voiceSource = selectedMode(explicit, defaults, "voice");
  const speedSource = selectedMode(explicit, defaults, "speed");
  const formatSource = selectedMode(explicit, defaults, "format");
  const base = {
    effectiveSpeechParametersVersion: EFFECTIVE_SPEECH_PARAMETERS_VERSION,
    protocolId: protocol,
    modelId,
  } as const;

  if (protocol === "system-speech") {
    const voice = normalizedText(selectedVoice, "");
    const hasRate = typeof selectedSpeed === "number" && Number.isFinite(selectedSpeed);
    const speed = hasRate ? normalizedSpeed(selectedSpeed, 1, 0.25, 4) : null;
    return Object.freeze({
      ...base,
      ...(voice ? { voice, voiceMode: voiceSource || "explicit" } : { voiceMode: "system_default" }),
      ...(speed !== null
        ? { rateWpm: Math.round(175 * speed), rateMode: speedSource || "explicit" }
        : { rateMode: "system_default" }),
      format: "m4a",
      formatMode: "protocol_output_default",
    });
  }

  if (protocol === "dashscope-qwen-tts") {
    return Object.freeze({
      ...base,
      voice: normalizedText(selectedVoice, "Cherry"),
      voiceMode: normalizedText(selectedVoice, "") ? (voiceSource || "explicit") : "protocol_default",
      // DashScope 的 format 是下载产物默认，不是请求体字段。
      format: "wav",
      formatMode: "protocol_output_default",
    });
  }

  if (protocol === "minimax-tts" || protocol === "minimax-t2a-v2") {
    const format = typeof selectedFormat === "string" && MINIMAX_SPEECH_FORMATS.has(selectedFormat)
      ? selectedFormat
      : "mp3";
    const groupId = targetText(target, "groupId") || targetText((target as any).model || {}, "groupId");
    return Object.freeze({
      ...base,
      voice: normalizedText(selectedVoice, "male-qn-qingse"),
      voiceMode: normalizedText(selectedVoice, "") ? (voiceSource || "explicit") : "protocol_default",
      speed: normalizedSpeed(selectedSpeed, 1, 0.5, 2),
      format,
      formatMode: format === selectedFormat ? (formatSource || "explicit") : "protocol_default",
      ...(groupId ? { groupId } : {}),
    });
  }

  const format = typeof selectedFormat === "string" && OPENAI_SPEECH_FORMATS.has(selectedFormat)
    ? selectedFormat
    : "mp3";
  return Object.freeze({
    ...base,
    voice: normalizedText(selectedVoice, "alloy"),
    voiceMode: normalizedText(selectedVoice, "") ? (voiceSource || "explicit") : "protocol_default",
    speed: normalizedSpeed(selectedSpeed, 1, 0.25, 4),
    format,
    formatMode: format === selectedFormat ? (formatSource || "explicit") : "protocol_default",
  });
}

/** adapter 直调也必须经同一解析器；manager 传入的已解析对象不会再归一化。 */
export function ensureEffectiveSpeechParameters(
  params: Record<string, any>,
  protocolId: string,
  executionTarget: Record<string, any> = {},
): EffectiveSpeechParameters & Record<string, any> {
  if (params?.effectiveSpeechParametersVersion === EFFECTIVE_SPEECH_PARAMETERS_VERSION) {
    return params as EffectiveSpeechParameters & Record<string, any>;
  }
  const effective = resolveSpeechParameters({
    protocolId,
    executionTarget: {
      protocolId,
      modelId: targetText(executionTarget, "modelId") || text(params?.modelId) || text(params?.model),
      groupId: targetText(executionTarget, "groupId") || text(params?.groupId),
      model: executionTarget?.model,
    },
    explicitInput: params,
    speechProviderDefaults: {},
  });
  return Object.freeze({ ...params, ...effective });
}
