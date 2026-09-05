import type { KnowledgeReferenceMode } from "../../../shared/knowledge-refs.ts";
import type { KnowledgeCompletenessPolicy } from "../../../shared/knowledge-execution.ts";

const CHINESE_SECTION_INTENT = /逐章|每一章|逐节|每个章节|前后章节|所有相关章节/gu;
const ENGLISH_SECTION_INTENT = /\b(?:chapter[\s-]+by[\s-]+chapter|section[\s-]+by[\s-]+section|(?:each|every)\s+(?:relevant\s+)?(?:chapters?|sections?)|all\s+relevant\s+(?:chapters?|sections?)|(?:previous|preceding)\s+and\s+(?:next|following)\s+chapters?|chapters?\s+before\s+and\s+after)\b/giu;
const CHINESE_SCOPE_INTENT = /全文|全书|整本|全部|所有|每一个|有没有任何|是否存在任何|是否从未|是否没有|有没有遗漏|列出所有|所有出现|所有提到|从头到尾/u;
const ENGLISH_SCOPE_INTENT = /\b(?:all|every|entire|whole|any|never|omissions?|full[\s-]+text|(?:from\s+)?(?:beginning|start)\s+to\s+(?:end|finish)|anything\s+(?:(?:is|was|(?:has|have)\s+been)\s+)?(?:missing|omitted)|(?:is|are)\s+there\s+no)\b/iu;

/** 确定性最低要求：范围词提高核查要求，资料数量不代替用户的完整性意图。 */
export function deriveKnowledgeCompletenessPolicy(input: {
  mode: KnowledgeReferenceMode;
  question: string;
  selectedNotebookCount: number;
  selectedSourceCount: number;
}): KnowledgeCompletenessPolicy {
  if (input.mode === "fast") return "best_effort";
  let sectionsRequested = false;
  const maskSection = () => { sectionsRequested = true; return " "; };
  // 完整章节短语内的“所有”只限定相关章节；短语外的全文要求仍须检查整个范围。
  const remaining = input.question.normalize("NFKC")
    .replace(CHINESE_SECTION_INTENT, maskSection).replace(ENGLISH_SECTION_INTENT, maskSection);
  if (CHINESE_SCOPE_INTENT.test(remaining) || ENGLISH_SCOPE_INTENT.test(remaining)) return "scope_complete";
  return sectionsRequested ? "relevant_sections_complete" : "source_diverse";
}
