/**
 * modality.ts — 统一输入/输出模态 canonical 数据模型
 *
 * chat 模型与媒体模型共用同一套 Modality 表达：
 *   inputs?: Modality[]
 *   outputs?: Modality[]
 *
 * 固定 canonical 顺序：text → image → video → audio。
 * 任何保存操作都必须按该顺序归一化（去重 + 排序）；
 * 非法值（非数组、空数组、未知成员）必须显式报错，禁止静默丢弃。
 */

export const MODALITY_ORDER = ["text", "image", "video", "audio"];

/**
 * @param {unknown} value
 * @returns {value is import("./modality.ts").Modality}
 */
export function isModality(value) {
  return typeof value === "string" && MODALITY_ORDER.includes(value);
}

/**
 * 校验并归一化一个模态数组：
 * - 必须是真正的数组（拒绝字符串 / null / 普通对象）；
 * - 必须非空；
 * - 成员必须是已知 Modality（拒绝未知值）；
 * - 去重并按 canonical 顺序排序。
 *
 * 非法时返回 null，由调用方决定显式报错（400），绝不静默修正。
 *
 * @param {unknown} value
 * @returns {string[] | null}
 */
export function normalizeModalityList(value) {
  if (!Array.isArray(value)) return null;
  if (value.length === 0) return null;
  const seen = new Set();
  for (const item of value) {
    if (!isModality(item)) return null;
    seen.add(item);
  }
  return MODALITY_ORDER.filter((modality) => seen.has(modality));
}

/**
 * 宽松读取（不报错）：值非法时返回 null。
 * 用于解析优先级链（用户显式值 > legacy boolean > known-models > ...）
 * 中每一层候选，只有合法数组才参与解析。
 *
 * @param {unknown} value
 * @returns {string[] | null}
 */
export function readModalityListLoose(value) {
  if (!Array.isArray(value) || value.length === 0) return null;
  const seen = new Set();
  for (const item of value) {
    if (!isModality(item)) return null;
    seen.add(item);
  }
  return MODALITY_ORDER.filter((modality) => seen.has(modality));
}

/**
 * 从 legacy 布尔字段（image/vision/video/audio）构造 inputs。
 * 只有至少一个布尔被显式定义时才认为该层存在候选。
 *
 * @param {{ image?: unknown, vision?: unknown, video?: unknown, audio?: unknown }} [flags]
 * @returns {string[] | null}
 */
export function modalitiesFromLegacyFlags(flags: Record<string, unknown> = {}) {
  const { image, vision, video, audio } = flags || {};
  if (image === undefined && vision === undefined && video === undefined && audio === undefined) {
    return null;
  }
  const enabled = new Set(["text"]);
  if (image === true || vision === true) enabled.add("image");
  if (video === true) enabled.add("video");
  if (audio === true) enabled.add("audio");
  return MODALITY_ORDER.filter((modality) => enabled.has(modality));
}

/**
 * 按 canonical 顺序合并多份候选（后者不覆盖前者，仅并集后排序）。
 *
 * @param {...(string[] | null | undefined)} lists
 * @returns {string[]}
 */
export function unionModalities(...lists) {
  const enabled = new Set();
  for (const list of lists) {
    if (!Array.isArray(list)) continue;
    for (const item of list) {
      if (isModality(item)) enabled.add(item);
    }
  }
  return MODALITY_ORDER.filter((modality) => enabled.has(modality));
}

/**
 * 各模型类别的默认模态 seed（用户只输入 ID 时使用）。
 */
export const KIND_DEFAULT_INPUTS = {
  chat: ["text"],
  image: ["text"],
  video: ["text"],
  speech: ["audio"],
};

export const KIND_DEFAULT_OUTPUTS = {
  chat: ["text"],
  image: ["image"],
  video: ["video"],
  speech: ["text"],
};

/**
 * 媒体类别 invariant：编辑后的模态不能与所属类别矛盾。
 * 返回 null 表示满足；否则返回错误说明（供 400 响应）。
 *
 * @param {string} capability - "image_generation" | "video_generation" | "speech_recognition" | "speech_generation"
 * @param {string[]} inputs
 * @param {string[]} outputs
 * @returns {string | null}
 */
export function mediaCapabilityModalityError(capability, inputs, outputs) {
  if (!Array.isArray(inputs) || !Array.isArray(outputs)) {
    return "inputs and outputs must be modality arrays";
  }
  switch (capability) {
    case "image_generation":
      if (!outputs.includes("image")) return "image generation models must include \"image\" in outputs";
      return null;
    case "video_generation":
      if (!outputs.includes("video")) return "video generation models must include \"video\" in outputs";
      return null;
    case "speech_recognition":
      if (!inputs.includes("audio")) return "speech recognition models must include \"audio\" in inputs";
      if (!outputs.includes("text")) return "speech recognition models must include \"text\" in outputs";
      return null;
    case "speech_generation":
      if (!outputs.includes("audio")) return "speech generation models must include \"audio\" in outputs";
      return null;
    default:
      return null;
  }
}
