/**
 * Auxiliary Model Slots — canonical single source of truth.
 *
 * 业务层只认识语义 Slot（title/summarize/memory/knowledge/
 * vision/approval/guard），不再关心 utility/utility_large 或模型大小。
 *
 * Slot 不拥有 credential；Provider credential 基础设施是唯一执行凭证来源。
 * 一次 resolve 只解析一个 Slot。
 */

import { modelSupportsImageInput } from "../shared/model-capabilities.ts";
import { t } from "../lib/i18n.ts";
import {
  AUXILIARY_SLOT_IDS as SHARED_AUXILIARY_SLOT_IDS,
  type AuxiliarySlot as SharedAuxiliarySlot,
} from "../shared/auxiliary-slot-ids.ts";

/**
 * 结构化配置错误——「Slot 已配置但不可用」类型。
 *
 * 定义在 canonical auxiliary-slots 模块，避免 resolver ↔ slots 循环依赖。
 * 消费方用 isAuxiliaryConfigError(err) 识别这类错误：
 *   - 不得 fallback（不能偷偷改用 chat）
 *   - 应报告诊断信息（devlog / warning），而非静默吞掉
 *
 * 与「运行时失败」（timeout / 5xx / rate limit / empty）严格区分。
 * 运行时失败允许 best-effort 跳过；配置错误必须可观测。
 */
export class AuxiliaryConfigurationError extends Error {
  readonly code = "AUXILIARY_CONFIG_ERROR";
  readonly slot?: string;
  readonly reason: string;
  constructor(message: string, reason: string, slot?: string) {
    super(message);
    this.name = "AuxiliaryConfigurationError";
    this.reason = reason;
    if (slot) this.slot = slot;
  }
}

/**
 * 判断错误是否为「Slot 已配置但不可用」（配置错误）。
 * 这类错误不得 fallback，消费方应报告诊断而非静默吞掉。
 *
 * 不依赖 brittle i18n 文本匹配——靠结构化 code + instanceof 判定。
 */
export function isAuxiliaryConfigError(error: any): boolean {
  if (!error) return false;
  if (error instanceof AuxiliaryConfigurationError) return true;
  return error?.code === "AUXILIARY_CONFIG_ERROR";
}

/**
 * Slot 身份（id 列表 + 类型）来自 shared/auxiliary-slot-ids.ts，
 * server / core / desktop renderer 共用同一份，禁止在各层手写第二份。
 * 此处 re-export 以保持现有 import 路径稳定。
 */
export type AuxiliarySlot = SharedAuxiliarySlot;

export type AuxiliarySlotFallback = "chat" | "image_capable_chat" | "none";
export type AuxiliarySlotCapability = "text" | "image";

export interface AuxiliarySlotDescriptor {
  readonly id: AuxiliarySlot;
  readonly preferenceKey: string;
  readonly fallback: AuxiliarySlotFallback;
  readonly capability: AuxiliarySlotCapability;
}

/**
 * 7 个语义 Slot 的 canonical 定义。
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
  knowledge: {
    id: "knowledge",
    preferenceKey: "knowledge_model",
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

/**
 * Slot id 列表来自 shared 单一真理源（shared/auxiliary-slot-ids.ts）。
 * 同时用一个静态 exhaustive 校验保证 canonical descriptor 的 key 集合
 * 与 shared id 列表完全一致——任一侧新增 Slot 而忘记同步另一侧时编译失败。
 */
export const AUXILIARY_SLOT_IDS: readonly AuxiliarySlot[] = SHARED_AUXILIARY_SLOT_IDS;

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
    throw new AuxiliaryConfigurationError(
      t("error.auxiliarySlotCapabilityMismatch", { slot }),
      "capability_mismatch",
      slot,
    );
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
    throw new AuxiliaryConfigurationError(
      t("error.auxiliarySlotCapabilityMismatch", { slot }),
      "capability_mismatch",
      slot,
    );
  }
}
