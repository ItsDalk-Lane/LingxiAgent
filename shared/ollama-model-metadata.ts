function isPlainObject(value: any): value is Record<string, any> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function normalizeProvider(provider: any) {
  return typeof provider === "string" ? provider.trim().toLowerCase() : "";
}

function normalizeModelId(modelId: any) {
  return typeof modelId === "string" ? modelId.trim().toLowerCase() : "";
}

function ollamaModelText(modelId: string) {
  const id = normalizeModelId(modelId);
  if (!id) return "";
  const slashBare = id.includes("/") ? id.split("/").pop() || id : id;
  const tagBare = slashBare.split(":")[0] || slashBare;
  return `${id} ${slashBare} ${tagBare}`;
}

/**
 * Ollama 视觉模型识别（按模型名启发式）。
 *
 * 注意：Ollama 的 capabilities 字段只暴露 completion/tools/thinking，
 * 不暴露 vision——所以视觉能力只能靠模型名推断，无法从 API 权威获取。
 * gemma[3-9] 覆盖 gemma3/gemma4/…（均为多模态），随版本升级无需逐个加。
 */
const OLLAMA_VISION_MODEL_PATTERNS = [
  /(^|[\s/_.:-])(?:llava|bakllava)(?=$|[\s/_.:-])/,
  /(^|[\s/_.:-])minicpm[-_.]?v(?=$|[\s/_.:-]|\d)/,
  /(^|[\s/_.:-])moondream(?=$|[\s/_.:-]|\d)/,
  /(^|[\s/_.:-])llama(?:3(?:\.2|p2)?|v3p2)?[\w_.:-]*vision(?=$|[\s/_.:-])/,
  /(^|[\s/_.:-])phi[\w_.:-]*vision(?=$|[\s/_.:-])/,
  /(^|[\s/_.:-])granite[\w_.:-]*vision(?=$|[\s/_.:-])/,
  /(^|[\s/_.:-])qwen[\w_.:-]*(?:vl|vision)(?=$|[\s/_.:-]|\d)/,
  /(^|[\s/_.:-])gemma[3-9](?=$|[\s/_.:-])/,
];

/**
 * Ollama 模型元数据推断。
 *
 * 数据源优先级（与 model-sync.ts 的投影优先级一致）：
 *   1. 模型对象上已显式设置的字段（用户手动覆盖）—— 本函数不覆盖
 *   2. Ollama /api/tags 返回的 capabilities（由 providers.ts fetch-models 探测写入 _ollamaCapabilities）
 *   3. 模型名启发式（仅 vision，capabilities 无 vision 字段）
 *
 * @param provider 供应商 id
 * @param model    模型 id 字符串（旧调用）或完整模型对象（含 _ollamaCapabilities）
 * @returns 推断出的元数据片段，或 null（无可推断项）
 */
export function inferOllamaModelMetadata(provider: any, model: any): Record<string, any> | null {
  if (normalizeProvider(provider) !== "ollama") return null;

  // 兼容两种调用：纯 id 字符串 / 完整模型对象
  const isObj = isPlainObject(model);
  const id = isObj ? model.id : model;
  const text = ollamaModelText(id);
  if (!text) return null;

  const inferred: Record<string, any> = {};

  // ── Vision（仅按模型名，Ollama API 无 vision 能力字段）──
  if (OLLAMA_VISION_MODEL_PATTERNS.some((pattern) => pattern.test(text))) {
    inferred.image = true;
  }

  // ── capabilities 推断（来自 /api/tags，由 fetch-models 探测写入）──
  const capabilities: string[] = isObj && Array.isArray(model._ollamaCapabilities)
    ? model._ollamaCapabilities
    : (isObj && Array.isArray(model.capabilities) ? model.capabilities : []);

  if (capabilities.length > 0) {
    // tools → toolUse 契约（OpenAI 兼容层标准 dialect）
    if (capabilities.includes("tools")) {
      inferred.toolUse = {
        supportsTools: true,
        dialect: "openai",
        toolResultFormat: "message",
      };
    }
    // thinking → reasoning 标记（响应侧复用 reasoning_content 解析，无需 thinkingFormat）
    if (capabilities.includes("thinking")) {
      inferred.reasoning = true;
    }
  }

  return Object.keys(inferred).length > 0 ? inferred : null;
}

export function enrichOllamaModelMetadata(provider: any, model: any) {
  const id = isPlainObject(model) ? model.id : model;
  const inferred = inferOllamaModelMetadata(provider, model);
  if (!inferred) return model;
  if (!isPlainObject(model)) return { id, ...inferred };

  // 尊重已显式设置的字段：用户手动覆盖 / known-models 已声明的不覆盖
  const merged = { ...model };
  if (model.image === undefined && model.vision === undefined && inferred.image !== undefined) {
    merged.image = inferred.image;
  }
  if (model.toolUse === undefined && inferred.toolUse !== undefined) {
    merged.toolUse = inferred.toolUse;
  }
  if (model.reasoning === undefined && inferred.reasoning !== undefined) {
    merged.reasoning = inferred.reasoning;
  }
  return merged;
}
