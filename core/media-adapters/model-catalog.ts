/**
 * 图片模型的已知别名和缺省值，供适配器解析短名称及界面展示候选。
 * 这不是可调用模型的白名单：供应商新增的完整模型 ID 应原样传递。
 * default 只影响未指定模型时的选择，不改写用户明确指定的模型。
 */

/**
 * @typedef {{ id: string, name: string, aliases?: string[], default?: boolean }} ModelEntry
 */

/** @type {Record<string, ModelEntry[]>} */
export const MODEL_CATALOG = {
  volcengine: [
    { id: "doubao-seedream-3-0-t2i", name: "Seedream 3.0", aliases: ["3.0"] },
    { id: "doubao-seedream-4-0-250828", name: "Seedream 4.0", aliases: ["4.0"] },
    { id: "doubao-seedream-4-5-251128", name: "Seedream 4.5", aliases: ["4.5"] },
    { id: "doubao-seedream-5-0-lite-260128", name: "Seedream 5.0 Lite", aliases: ["5.0-lite"], default: true },
    { id: "doubao-seedream-5-0-260128", name: "Seedream 5.0", aliases: ["5.0"] },
  ],
  openai: [
    { id: "dall-e-3", name: "DALL-E 3", aliases: ["dalle3", "dall-e-3"] },
    { id: "gpt-image-1-mini", name: "GPT Image 1 Mini", aliases: ["1-mini", "mini"] },
    { id: "gpt-image-1", name: "GPT Image 1", aliases: ["1"] },
    { id: "gpt-image-1.5", name: "GPT Image 1.5", aliases: ["1.5"], default: true },
    { id: "gpt-image-2", name: "GPT Image 2", aliases: ["2"] },
  ],
  "openai-codex-oauth": [
    { id: "gpt-image-2", name: "GPT Image 2", aliases: ["2"] },
  ],
};

function getDefaultEntry(catalog) {
  return catalog.find(entry => entry.default) || catalog[catalog.length - 1];
}

/**
 * 先匹配完整 ID，再解析已知短别名；仅未指定模型时使用默认值。
 * 完整的新模型 ID 交由供应商校验，不因内置候选目录过时而拒绝。
 * 无法解析的版本短名需明确报错，避免误当成完整模型 ID 或改用默认值。
 *
 * @param {string} provider 供应商名称
 * @param {string | undefined | null} raw 用户或配置指定的模型
 * @returns {string} 解析后的 API 模型 ID
 */
export function resolveModelId(provider, raw) {
  const catalog = MODEL_CATALOG[provider];
  if (!catalog?.length) return raw ?? "";

  if (raw == null || raw === "") {
    return getDefaultEntry(catalog).id;
  }

  // 优先保留已知完整 ID。
  const byId = catalog.find(m => m.id === raw);
  if (byId) return byId.id;

  // 已知短别名不区分大小写。
  const lower = raw.toLowerCase();
  for (const entry of catalog) {
    if (entry.aliases?.some(a => a.toLowerCase() === lower)) {
      return entry.id;
    }
  }

  if (/^\d+(?:\.\d+)*(?:-[a-z]+)?$/i.test(raw)) {
    throw new Error(
      `Unknown image model alias "${raw}" for provider "${provider}". Please specify the full model ID.`,
    );
  }
  return raw;
}

/**
 * Get the known models list for a provider, formatted for settings UI.
 * Returns [{id, name}] without aliases (aliases are an adapter concern).
 *
 * @param {string} provider
 * @returns {{ id: string, name: string }[]}
 */
export function getKnownModels(provider) {
  const catalog = MODEL_CATALOG[provider];
  if (!catalog) return [];
  return catalog.map(({ id, name }) => ({ id, name }));
}

/**
 * Get the default model ID for a provider.
 *
 * @param {string} provider
 * @returns {string | null}
 */
export function getDefaultModelId(provider) {
  const catalog = MODEL_CATALOG[provider];
  if (!catalog?.length) return null;
  return getDefaultEntry(catalog).id;
}
