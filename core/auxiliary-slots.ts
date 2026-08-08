/**
 * Auxiliary Model Slots — canonical single source of truth.
 *
 * 业务层只认识语义 Slot（title/summarize/memory/vision/approval/guard），
 * 不再关心 utility/utility_large 或模型大小。
 *
 * Slot 不拥有 credential；Provider credential 基础设施是唯一执行凭证来源。
 * 一次 resolve 只解析一个 Slot。
 */

import { modelSupportsImageInput } from "../shared/model-capabilities.ts";
import { t } from "../lib/i18n.ts";

export type AuxiliarySlot =
  | "title"
  | "summarize"
  | "memory"
  | "vision"
  | "approval"
  | "guard";

export type AuxiliarySlotFallback = "chat" | "image_capable_chat" | "none";
export type AuxiliarySlotCapability = "text" | "image";

export interface AuxiliarySlotDescriptor {
  readonly id: AuxiliarySlot;
  readonly preferenceKey: string;
  readonly fallback: AuxiliarySlotFallback;
  readonly capability: AuxiliarySlotCapability;
}

/**
 * 6 个语义 Slot 的 canonical 定义。
 *
 * 所有层（config / server / UI / tests）都从这里派生 Slot 名单，
 * 禁止在多处手写硬编码数组。
 */
export const AUXILIARY_SLOTS: Record<AuxiliarySlot, AuxiliarySlotDescriptor> = {
  title: {
    id: "title",
    preferenceKey: "title_model",
    fallback: "chat",
    capability: "text",
  },
  summarize: {
    id: "summarize",
    preferenceKey: "summarize_model",
    fallback: "chat",
    capability: "text",
  },
  memory: {
    id: "memory",
    preferenceKey: "memory_model",
    fallback: "chat",
    capability: "text",
  },
  vision: {
    id: "vision",
    preferenceKey: "vision_model",
    fallback: "image_capable_chat",
    capability: "image",
  },
  approval: {
    id: "approval",
    preferenceKey: "approval_model",
    fallback: "none",
    capability: "text",
  },
  guard: {
    id: "guard",
    preferenceKey: "guard_model",
    fallback: "none",
    capability: "text",
  },
};

export const AUXILIARY_SLOT_IDS: readonly AuxiliarySlot[] =
  Object.keys(AUXILIARY_SLOTS) as AuxiliarySlot[];

/** vision feature flag 与 model slot 是两个概念，独立 key。 */
export const VISION_AUXILIARY_ENABLED_PREF_KEY = "vision_auxiliary_enabled";

/**
 * 从 canonical 定义派生 [field, preferenceKey] 对，
 * 供 getSharedModels/setSharedModels 使用。
 */
export const AUXILIARY_SLOT_PREF_ENTRIES: ReadonlyArray<
  readonly [AuxiliarySlot, string]
> = AUXILIARY_SLOT_IDS.map((id) => [id, AUXILIARY_SLOTS[id].preferenceKey] as const);

export function isAuxiliarySlot(slot: string): slot is AuxiliarySlot {
  return slot in AUXILIARY_SLOTS;
}

export function getAuxiliarySlotDescriptor(
  slot: string,
): AuxiliarySlotDescriptor | undefined {
  return AUXILIARY_SLOTS[slot as AuxiliarySlot];
}

/**
 * 校验已解析的模型是否满足 Slot 所需 capability。
 *
 * vision slot 的模型必须支持 image input，否则报配置错误（不 fallback）。
 */
export function validateAuxiliaryModelCapability(
  slot: AuxiliarySlot,
  model: any,
): void {
  const descriptor = AUXILIARY_SLOTS[slot];
  if (descriptor.capability === "image" && model && !modelSupportsImageInput(model)) {
    throw new Error(t("error.auxiliarySlotCapabilityMismatch", { slot }));
  }
}

/**
 * 校验用户传入的 patch 中的 Slot 模型引用。
 * 在设置层拒绝 wrong-capability 模型。
 */
export function validateAuxiliaryModelRef(
  slot: AuxiliarySlot,
  modelRef: any,
  resolveModel: (ref: any) => any,
): void {
  const descriptor = AUXILIARY_SLOTS[slot];
  if (descriptor.capability !== "image") return;
  const model = resolveModel(modelRef);
  if (model && !modelSupportsImageInput(model)) {
    throw new Error(t("error.auxiliarySlotCapabilityMismatch", { slot }));
  }
}
