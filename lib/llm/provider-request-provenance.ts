/**
 * provider-request-provenance.ts — Provider-Wire Provenance（Phase 6，§五十三～§六十一）。
 *
 * 回答：Semantic Request 的每个 section（Phase 5 locator）最终去了 Provider
 * Request body 的哪个位置。原则：
 *
 *   - Mapping 在 transformation 发生时由构造代码产生（§五十九）——callText 的
 *     body 由 llm-client.ts 自己构造，四个协议的落点（body.system / messages[0]
 *     / instructions / input[i]）在构造分支内已知，builder 消费同一份构造事实。
 *   - 严禁内容匹配反推（§五十八）：validateProviderProvenanceAgainstBody 只做
 *     locator 路径存在性 + 长度一致性检查（构造产物自检），绝不在 body 里搜索
 *     semantic 文本。
 *   - normalizeProviderPayload 会做结构变换（stripEmptyTools/orphan toolResult/
 *     reasoning replay/provider 子模块）——mapping 构造后校验 locator 是否仍在
 *     最终 body 上解析；失配降级 structural（§一三八），不伪造。
 *   - Pi 路径（MC-01/02/03）的 body 由 vendor SDK 构造，Lingxi 无法在
 *     transformation 处产生 sidecar → 不出 mapping（record.providerRequestProvenance
 *     = null，矩阵如实标注），不为矩阵全绿重写 provider serializer（§六十一）。
 */

import {
  MODEL_CALL_PAYLOAD_SCHEMA_VERSION,
  type ProviderMappingPrecision,
  type ProviderPayloadLocator,
  type ProviderRequestProvenance,
  type ProviderRequestProvenanceMapping,
  type ProviderRequestTransformation,
} from "./model-call-payload-types.ts";
import type {
  ModelSemanticInputProvenance,
  SemanticInputProvenanceSection,
} from "./semantic-input-provenance.ts";

/** callText 四协议的构造事实（由 llm-client.ts 各构造分支提供）。 */
export type CallTextProvenanceBuildArgs = {
  protocol: string;
  /** semantic provenance（remap 后，locator 相对 semantic request）。 */
  provenance: ModelSemanticInputProvenance | null;
  /** anthropic-messages：normalizedMessages 原始 index → body.messages index；-1 = 被过滤。 */
  anthropicMessageIndex?: ReadonlyArray<number> | null;
  /** openai-completions：body.messages[0] 是否被系统消息占用（后续消息平移 +1）。 */
  systemMessageSlot?: boolean;
  /** 语义 system prompt 的最终长度（span 长度校验用；codex 注入含默认 instruction）。 */
  semanticSystemPromptLength: number;
};

/**
 * 构造 callText 各协议的 provider mapping。sections 下标即
 * semanticSectionOrdinal（与 ModelSemanticInputProvenance.sections 对齐）。
 */
export function buildCallTextProviderProvenance(
  args: CallTextProvenanceBuildArgs,
): ProviderRequestProvenance | null {
  if (!args.provenance || args.provenance.sections.length === 0) return null;
  const mappings: ProviderRequestProvenanceMapping[] = [];
  const sections = args.provenance.sections;

  for (let ordinal = 0; ordinal < sections.length; ordinal++) {
    const section = sections[ordinal];
    const mapping = mapSection(section, ordinal, args);
    if (mapping) mappings.push(mapping);
  }
  if (mappings.length === 0) return null;
  return {
    schemaVersion: MODEL_CALL_PAYLOAD_SCHEMA_VERSION,
    protocol: args.protocol,
    mappings,
  };
}

function mapSection(
  section: SemanticInputProvenanceSection,
  ordinal: number,
  args: CallTextProvenanceBuildArgs,
): ProviderRequestProvenanceMapping | null {
  const locator = section.locator;

  if (locator.root === "systemPrompt") {
    // span 相对语义 systemPrompt（含 codex adapter_injected 注入段）。
    if (locator.span && locator.span.end > args.semanticSystemPromptLength) return null;
    let providerLocator: ProviderPayloadLocator | null;
    let transformation: ProviderRequestTransformation;
    switch (args.protocol) {
      case "anthropic-messages":
        providerLocator = { path: ["system"], span: locator.span ?? null };
        transformation = "pass_through";
        break;
      case "openai-completions":
        // 系统消息占用 body.messages[0]（mergedSystem 非空才有本段）。
        providerLocator = { path: ["messages", 0, "content"], span: locator.span ?? null };
        transformation = "moved";
        break;
      case "openai-responses":
      case "openai-codex-responses":
        providerLocator = { path: ["instructions"], span: locator.span ?? null };
        transformation = section.category === "adapter_injected" ? "injected" : "renamed";
        break;
      case "google-generative-ai":
        providerLocator = { path: ["systemInstruction", "parts", 0, "text"], span: locator.span ?? null };
        transformation = "moved";
        break;
      default:
        return null;
    }
    return {
      semanticSectionOrdinal: ordinal,
      providerLocator,
      transformation,
      // §一三八：semantic 段本身只有 structural 精度时，映射不宣称 exact。
      mappingPrecision: section.precision === "exact" ? "exact" : "structural",
    };
  }

  if (locator.root === "messages" && Array.isArray(locator.path) && typeof locator.path[0] === "number") {
    const index = locator.path[0];
    switch (args.protocol) {
      case "anthropic-messages": {
        const mapped = args.anthropicMessageIndex?.[index];
        if (mapped === undefined) return null;
        if (mapped < 0) {
          // role 过滤（仅 user/assistant 入 body.messages）。
          return {
            semanticSectionOrdinal: ordinal,
            providerLocator: null,
            transformation: "filtered",
            mappingPrecision: "structural",
          };
        }
        return {
          semanticSectionOrdinal: ordinal,
          providerLocator: { path: ["messages", mapped] },
          transformation: "moved",
          mappingPrecision: section.precision === "exact" ? "exact" : "structural",
        };
      }
      case "openai-completions": {
        const offset = args.systemMessageSlot ? 1 : 0;
        return {
          semanticSectionOrdinal: ordinal,
          providerLocator: { path: ["messages", index + offset] },
          transformation: offset ? "moved" : "pass_through",
          mappingPrecision: section.precision === "exact" ? "exact" : "structural",
        };
      }
      case "openai-responses":
      case "openai-codex-responses":
        return {
          semanticSectionOrdinal: ordinal,
          providerLocator: { path: ["input", index] },
          transformation: "renamed",
          mappingPrecision: section.precision === "exact" ? "exact" : "structural",
        };
      case "google-generative-ai":
        return {
          semanticSectionOrdinal: ordinal,
          providerLocator: { path: ["contents", index] },
          transformation: "moved",
          mappingPrecision: section.precision === "exact" ? "exact" : "structural",
        };
      default:
        return null;
    }
  }

  return null;
}

/**
 * post-compat 校验（§一三八）：normalizeProviderPayload 之后的最终 body 上，
 * 每条 mapping 的 locator 必须仍可解析；带 span 的还要求目标值为 string 且
 * 长度与语义原文一致（构造产物自检，非内容搜索）。失配降级 structural。
 */
export function validateProviderProvenanceAgainstBody(
  provenance: ProviderRequestProvenance | null,
  body: unknown,
): ProviderRequestProvenance | null {
  if (!provenance) return null;
  const mappings = provenance.mappings.map((mapping) => {
    if (!mapping.providerLocator) return mapping;
    const resolved = resolvePath(body, mapping.providerLocator.path);
    if (resolved.missing) {
      return {
        ...mapping,
        providerLocator: null,
        mappingPrecision: "structural" as ProviderMappingPrecision,
      };
    }
    if (mapping.providerLocator.span) {
      if (typeof resolved.value !== "string" || mapping.providerLocator.span.end > resolved.value.length) {
        return {
          ...mapping,
          providerLocator: { path: mapping.providerLocator.path, span: null },
          mappingPrecision: "structural" as ProviderMappingPrecision,
        };
      }
    }
    return mapping;
  });
  return { ...provenance, mappings };
}

function resolvePath(root: unknown, path: Array<string | number>): { missing: boolean; value: unknown } {
  let cursor: unknown = root;
  for (const key of path) {
    if (cursor === null || cursor === undefined) return { missing: true, value: undefined };
    if (typeof key === "number") {
      if (!Array.isArray(cursor) || key >= cursor.length) return { missing: true, value: undefined };
      cursor = cursor[key];
    } else {
      if (typeof cursor !== "object" || !(key in (cursor as Record<string, unknown>))) {
        return { missing: true, value: undefined };
      }
      cursor = (cursor as Record<string, unknown>)[key];
    }
  }
  return { missing: false, value: cursor };
}
