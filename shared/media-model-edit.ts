/**
 * media-model-edit.ts — 已添加媒体模型的编辑校验（image / video / speech recognition）
 *
 * PUT /media/{image|video}/providers/:providerId/models/:modelId 与
 * PUT /speech-recognition/providers/:providerId/models/:modelId 共用。
 *
 * 规则：
 * - 只允许编辑 displayName（兼容 name / display_name 别名）、inputs、outputs；
 *   provider/runtime/protocol 契约字段（id、protocolId、modes、aliases 等）不可编辑，
 *   出现在请求体中时显式 400，而不是静默忽略；
 * - 模态数组必须是合法 Modality 数组（非空、无未知值），保存前按 canonical 顺序去重排序；
 * - 媒体种类 invariant：编辑结果不能与所属类别矛盾（如 image generation 的 outputs
 *   必须包含 image），违反时 400，禁止静默补回；
 * - PUT 只表示编辑已添加的模型；模型不存在时 404（HTTP 层语义，见各 route）。
 */

import {
  KIND_DEFAULT_INPUTS,
  KIND_DEFAULT_OUTPUTS,
  mediaCapabilityModalityError,
  normalizeModalityList,
} from "./modality.ts";

const EDITABLE_FIELDS = new Set(["displayName", "display_name", "name", "inputs", "outputs"]);

const KIND_BY_CAPABILITY = {
  image_generation: "image",
  video_generation: "video",
  speech_recognition: "speech",
  speech_generation: "speech",
};

export class MediaModelEditValidationError extends Error {
  declare code: string;
  declare statusCode: number;

  constructor(detail, statusCode = 400) {
    super(detail);
    this.name = "MediaModelEditValidationError";
    this.code = "INVALID_MEDIA_MODEL_EDIT";
    this.statusCode = statusCode;
  }
}

/**
 * 从请求体构造 registry.updateMediaModelEntry 的安全 patch。
 * existingModel 是当前生效模型（declared + user overlay），用于 invariant 合并检查。
 *
 * @returns {{ displayName?: string, inputs?: string[], outputs?: string[] }}
 */
export function buildMediaModelEditPatch({ capability, body, existingModel }) {
  if (!body || typeof body !== "object" || Array.isArray(body)) {
    throw new MediaModelEditValidationError("request body must be an object with displayName, inputs, or outputs");
  }
  for (const key of Object.keys(body)) {
    if (!EDITABLE_FIELDS.has(key)) {
      throw new MediaModelEditValidationError(
        `field "${key}" is not editable on an added model (editable: displayName, inputs, outputs)`,
      );
    }
  }

  const patch: Record<string, any> = {};
  const rawName = body.displayName ?? body.display_name ?? body.name;
  if (rawName !== undefined) {
    if (typeof rawName !== "string" || !rawName.trim()) {
      throw new MediaModelEditValidationError("displayName must be a non-empty string");
    }
    patch.displayName = rawName.trim();
  }
  for (const field of ["inputs", "outputs"]) {
    if (body[field] === undefined) continue;
    const normalized = normalizeModalityList(body[field]);
    if (!normalized) {
      throw new MediaModelEditValidationError(
        `${field} must be a non-empty modality array with only known values (text|image|video|audio)`,
      );
    }
    patch[field] = normalized;
  }
  if (Object.keys(patch).length === 0) {
    throw new MediaModelEditValidationError("nothing to update: provide displayName, inputs, or outputs");
  }

  const kind = KIND_BY_CAPABILITY[capability] || "chat";
  const baseInputs = normalizeModalityList(existingModel?.inputs || null) || KIND_DEFAULT_INPUTS[kind];
  const baseOutputs = normalizeModalityList(existingModel?.outputs || null) || KIND_DEFAULT_OUTPUTS[kind];
  const invariantError = mediaCapabilityModalityError(
    capability,
    patch.inputs ?? baseInputs,
    patch.outputs ?? baseOutputs,
  );
  if (invariantError) {
    throw new MediaModelEditValidationError(invariantError);
  }
  return patch;
}

/**
 * 在生效模型列表中定位已添加模型；不存在时抛 404（PUT 不允许偷偷创建）。
 */
export function requireExistingMediaModel({ models, providerId, modelId, capability }) {
  const existing = (Array.isArray(models) ? models : [])
    .find((model) => (typeof model === "object" && model !== null ? model.id : model) === modelId);
  if (!existing) {
    throw new MediaModelEditValidationError(
      `model "${providerId}/${modelId}" is not added under ${capability}; use POST to add it first`,
      404,
    );
  }
  return existing;
}
