import crypto from "crypto";
import {
  CACHE_STRATEGIES,
  buildCacheStrategyMetadata,
} from "./cache-strategy-contract.ts";
import {
  createSemanticInputProvenance,
  type ModelSemanticInputProvenance,
  type SemanticInputProvenanceSection,
} from "./semantic-input-provenance.ts";

function hashStablePrefix(parts) {
  return crypto
    .createHash("sha256")
    .update(JSON.stringify(parts))
    .digest("hex");
}

/**
 * Phase 5：utility 调用的 Semantic Input Provenance 组装。
 *
 * - systemPrompt：template identity 复用 cacheGroup/templateVersion（§二十三：
 *   source.id=cacheGroup、source.version=templateVersion，不重算内容 hash，
 *   cachePrefixHash 行为不变）。
 * - userContent：caller 已用 renderProvenancedText 渲染（段级 span 相对
 *   userContent 字符串）时传 userProvenanceSections——locator 统一改写为
 *   root=messages path=[0]（span 原样成立）；未传时诚实 fallback 单段
 *   structural task_input（§六十一：fallback 不得 exact）。
 * - 输出 provenance 与 layout 请求同形（callText 传入形状），caller 直接把它
 *   交给 callText 的 semanticInputProvenance；本函数不改任何请求字段。
 */
function buildUtilityPromptProvenance({
  cacheGroup,
  templateVersion,
  systemPrompt,
  userContent,
  userProvenanceSections,
}): ModelSemanticInputProvenance | null {
  const sections = [];
  const stableSystemPrompt = String(systemPrompt || "");
  if (stableSystemPrompt.length > 0) {
    sections.push({
      category: "task_instruction",
      role: "system",
      precision: "exact",
      locator: { root: "systemPrompt", span: { start: 0, end: stableSystemPrompt.length } },
      source: {
        type: "template",
        id: String(cacheGroup || "utility.unknown"),
        ...(templateVersion ? { version: String(templateVersion) } : {}),
      },
    });
  }
  const stableUserContent = String(userContent || "");
  if (Array.isArray(userProvenanceSections) && userProvenanceSections.length > 0) {
    for (const section of userProvenanceSections) {
      if (!section || typeof section !== "object") continue;
      sections.push({
        ...section,
        locator: { root: "messages", path: [0], span: section.locator?.span ?? null },
      });
    }
  } else if (stableUserContent.length > 0) {
    sections.push({
      category: "task_input",
      role: "user",
      precision: "structural",
      locator: { root: "messages", path: [0], span: { start: 0, end: stableUserContent.length } },
      source: null,
    });
  }
  return createSemanticInputProvenance("calltext", sections);
}

export function buildUtilityPromptLayout({
  cacheGroup,
  templateVersion,
  systemPrompt,
  userContent,
  userProvenanceSections = null,
  includeProvenance = true,
}) {
  const stableSystemPrompt = String(systemPrompt || "");
  const stableTemplateVersion = String(templateVersion || "v1");
  const stableCacheGroup = String(cacheGroup || "utility.unknown");
  return {
    systemPrompt: stableSystemPrompt,
    messages: [{ role: "user", content: String(userContent || "") }],
    usageMetadata: buildCacheStrategyMetadata({
      cacheStrategy: CACHE_STRATEGIES.UTILITY_TEMPLATE,
      cacheGroup: stableCacheGroup,
      templateVersion: stableTemplateVersion,
      cachePrefixHash: hashStablePrefix({
        cacheStrategy: CACHE_STRATEGIES.UTILITY_TEMPLATE,
        cacheGroup: stableCacheGroup,
        templateVersion: stableTemplateVersion,
        systemPrompt: stableSystemPrompt,
      }),
      strict: true,
    }),
    ...(includeProvenance
      ? {
          semanticInputProvenance: buildUtilityPromptProvenance({
            cacheGroup: stableCacheGroup,
            templateVersion: stableTemplateVersion,
            systemPrompt: stableSystemPrompt,
            userContent,
            userProvenanceSections,
          }),
        }
      : {}),
  };
}

export function attachPromptLayoutMetadata(usageContext, usageMetadata) {
  return {
    ...(usageContext || {}),
    metadata: {
      ...(usageContext?.metadata || {}),
      ...(usageMetadata || {}),
    },
  };
}
