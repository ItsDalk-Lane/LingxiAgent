/**
 * knowledge-coverage-manifest —— Source Fidelity 判定（任务书 §四十六–§四十九
 * Phase 9 遗产；2026-08-31 exhaustive 档移除后仅保留 outline 工具消费的
 * fidelity 面，CoverageManifest/Sharding/ShardWorker 协议随执行链路删除）。
 *
 * parser locator 类型 → fidelity：text/markdown/pdf 偏移可直接回溯原文
 * （citation_grade）；html 块文本经空白归一、定位靠 DOM structuralPath
 * （structural，按现状评估）；未知类型保守 semantic_only（只有语义无反向定位）。
 * 混合时取最弱等级（宁低估勿虚标）。
 *
 * 纯函数化可测，无 IO。
 */

/** Source Fidelity 等级（§五十七/§五十九；与 Index/Text Coverage 分离的第二个维度）。 */
export type CoverageSourceFidelity =
  | "citation_grade"
  | "structural"
  | "semantic_only"
  | "needs_ocr"
  | "unavailable";

export const COVERAGE_SOURCE_FIDELITIES: readonly CoverageSourceFidelity[] = [
  "citation_grade",
  "structural",
  "semantic_only",
  "needs_ocr",
  "unavailable",
];

export function isCoverageSourceFidelity(value: unknown): value is CoverageSourceFidelity {
  return typeof value === "string"
    && (COVERAGE_SOURCE_FIDELITIES as readonly string[]).includes(value);
}

/**
 * parser locator 类型 → fidelity：text/markdown/pdf 偏移可直接回溯原文
 * （citation_grade）；html 块文本经空白归一、定位靠 DOM structuralPath
 * （structural，按现状评估）；未知类型保守 semantic_only（只有语义无反向定位）。
 * 混合时取最弱等级（宁低估勿虚标）。
 */
export function fidelityFromLocatorTypes(locatorTypes: readonly string[]): "citation_grade" | "structural" | "semantic_only" {
  let seenHtml = false;
  for (const type of locatorTypes) {
    if (type === "html") seenHtml = true;
    if (type !== "text" && type !== "markdown" && type !== "pdf" && type !== "html") return "semantic_only";
  }
  return seenHtml ? "structural" : "citation_grade";
}
