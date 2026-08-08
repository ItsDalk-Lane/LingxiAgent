/**
 * Auxiliary Slot IDs — shared single source of truth for the Slot identity.
 *
 * 本模块只承载 Slot 的「身份」（id 列表 + 类型），不依赖任何 runtime（无 i18n、
 * 无 model-capabilities），因此 server/core、desktop renderer、CLI 都可以 import。
 *
 * 完整的 canonical descriptor（preferenceKey / fallback / capability）定义在
 * core/auxiliary-slots.ts 的 AUXILIARY_SLOTS，那里是 runtime 单一真理源。
 * 本文件的存在是为了让跨层（尤其 desktop renderer）共享同一份 Slot 身份，
 * 而不是各自手写硬编码字符串数组——新增第 7 个 Slot 时，所有层编译期即可发现遗漏。
 */

/**
 * 6 个语义 Slot 的 id。新增 Slot 时在此追加，core descriptor 与 UI metadata
 * 会被 TypeScript 的 exhaustive Record 检查强迫补齐。
 */
export const AUXILIARY_SLOT_IDS = [
  "title",
  "summarize",
  "memory",
  "vision",
  "approval",
  "guard",
] as const;

export type AuxiliarySlot = (typeof AUXILIARY_SLOT_IDS)[number];
