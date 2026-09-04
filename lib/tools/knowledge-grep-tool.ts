/**
 * knowledge_grep 工具 —— 在当前 KnowledgeTurnScope 冻结集合的原文上做
 * literal / regexp 匹配（Phase 11，任务书 §二十三 Agent Knowledge 工具体系）。
 *
 * 与 knowledge_read 的检索模式互补：read.query 走 FTS/向量检索（相关性排序），
 * grep 是确定性的原文子串/正则扫描——找「精确出现的字面量」（编号、人名、
 * 配置键）时比检索更可信。数据源直接读冻结 parseArtifact 的 blocks
 * （knowledge.db，冻结 artifactId 锚定，不依赖检索索引是否已建）。
 *
 * 权限边界（与 knowledge_read 同链，任务书 §二十~§二十二，校验链在
 * lib/tools/knowledge-scope.ts）：
 * - 只读；studio 隔离；scopeId 服务端逐次复核（存在/active/studio/会话归属，
 *   subagent 子会话经 manifest provenance 认父会话）；
 * - sourceIds 给出时必须全部在 scope 冻结集合内，任一越权 →
 *   KNOWLEDGE_SCOPE_VIOLATION（不部分放行）；
 * - 匹配锚定冻结 artifact 的 blocks，每条命中带 provenance
 *   （sourceId/parseArtifactId/blockId/blockOrdinal/offset/lineNumber/headingPath）；
 * - 防护：pattern 长度上限、regexp 构造失败显式报错、全文扫描字符预算
 *   （超限显式标注 scanTruncated，不静默丢结果）、maxResults 封顶 +
 *   超出计数提示；
 * - 未解析/未就绪的源进 unavailableSources 显式单列，不静默省略。
 */
import { Type } from "../pi-sdk/index.ts";
import { isKnowledgeError, KnowledgeError } from "../knowledge/errors.ts";
import { EvidenceReceiptService, type KnowledgeResearchToolContext } from "../knowledge/evidence-receipt-service.ts";
import { ResearchStore } from "../knowledge/research/research-store.ts";
import type { KnowledgeManager } from "../knowledge/knowledge-manager.ts";
import type { KnowledgeBlock } from "../knowledge/types.ts";
import {
  knowledgeBlockHeadingPath,
  knowledgeScopeViolation,
  requireKnowledgeScopeSource,
  resolveKnowledgeTurnScope,
  type KnowledgeToolSessionContext,
} from "./knowledge-scope.ts";
import { toolError, toolOk } from "./tool-result.ts";

/** pattern 长度上限（literal 与 regexp 同限）：防超长模式拖垮匹配。 */
const MAX_PATTERN_CHARS = 512;
/** 默认返回命中条数封顶。 */
const DEFAULT_MAX_RESULTS = 50;
/** maxResults 参数上限（非法值显式报错，不静默夹取）。 */
const MAX_RESULTS_LIMIT = 200;
/** 单条命中 snippet 的上下文窗口（命中点前/后字符数）。 */
const SNIPPET_CONTEXT_CHARS = 96;
/** 单条命中 snippet 总长上限。 */
const SNIPPET_MAX_CHARS = 360;
/** 全文扫描字符预算（≈4M chars）：超限显式停止并标注。 */
const MAX_SCAN_CHARS = 4_000_000;
/** headingFilter 长度上限。 */
const MAX_HEADING_FILTER_CHARS = 256;

/** 宿主可在研究会话中据此生成读取凭据；字段不由模型提供。 */
export interface KnowledgeGrepReadSpan {
  sourceId: string;
  contentSnapshotId: string;
  parseArtifactId: string;
  blockId: string;
  startOffset: number;
  endOffset: number;
  canonicalText: string;
}

export interface KnowledgeGrepToolDeps {
  /** engine 级 KnowledgeManager（跨会话）；null = Knowledge 不可用。 */
  getKnowledge: () => KnowledgeManager | null;
  /** 当前 runtime studioId；null = 运行时上下文不可用。 */
  getStudioId: () => string | null;
  /** 工具执行会话的 scope 归属上下文（与 knowledge_read 同一接线契约）。 */
  resolveSessionContext?: (ctx: unknown) => KnowledgeToolSessionContext;
  /** 研究身份来自宿主；普通调用仍只返回原有扫描结果。 */
  resolveResearchContext?: (ctx: unknown) => KnowledgeResearchToolContext | null;
}

function optionalTrimmedString(value: unknown, label: string, maxChars: number): string | null {
  if (value == null) return null;
  if (typeof value !== "string") {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${label} must be a string`);
  }
  const trimmed = value.trim();
  if (!trimmed) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${label} must not be empty when provided`);
  }
  if (trimmed.length > maxChars) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${label} exceeds the ${maxChars} character limit`);
  }
  return trimmed;
}

/**
 * headingFilter 前缀匹配：headingPath 以 " > " 连接后的字符串以前缀命中
 * （"指南" 命中 ["指南"] 与 ["指南","安装"]；"指南 > 安装" 只命中后者）。
 */
function headingPathMatches(headingPath: string[], headingFilter: string): boolean {
  if (headingPath.length === 0) return false;
  return headingPath.join(" > ").startsWith(headingFilter);
}

interface GrepMatchRecord {
  sourceId: string;
  sourceName: string;
  parseArtifactId: string;
  blockId: string;
  blockOrdinal: number;
  /** 命中起点在 block.text 内的 0-based 字符偏移。 */
  offset: number;
  endOffset: number;
  matchTruncated: boolean;
  /** 命中起点在 block 内的 1-based 行号。 */
  lineNumber: number;
  /** 命中的原文子串（封顶截断）。 */
  match: string;
  /** 命中点上下文片段（封顶截断）。 */
  snippet: string;
  headingPath: string[];
  receiptId?: string;
  receiptStartOffset?: number;
  receiptEndOffset?: number;
}

function buildSnippet(text: string, offset: number, matchLength: number) {
  const start = Math.max(0, offset - SNIPPET_CONTEXT_CHARS);
  const requestedEnd = Math.min(text.length, offset + matchLength + SNIPPET_CONTEXT_CHARS);
  const prefix = start > 0 ? "…" : "";
  const end = Math.min(requestedEnd, start + SNIPPET_MAX_CHARS - prefix.length);
  const canonicalText = text.slice(start, end);
  return { snippet: `${prefix}${canonicalText}${end < text.length ? "…" : ""}`, start, end, canonicalText };
}

export function createKnowledgeGrepTool(deps: KnowledgeGrepToolDeps) {
  return {
    name: "knowledge_grep",
    label: "Knowledge Grep",
    description: "Search the raw text of the current turn's knowledge scope for a literal substring or a regular "
      + "expression pattern. Deterministic scanning of the frozen source text — use it for exact strings "
      + "(ids, names, config keys) where knowledge_read's ranked retrieval is the wrong tool. "
      + "Each match carries provenance (sourceId, blockId, offset, headingPath). The scopeId is this turn's "
      + "knowledge permission ceiling: sources outside it are rejected. Read-only.",
    parameters: Type.Object({
      scopeId: Type.String({
        description: "Knowledge turn scope id from the [KnowledgeContext] block header (the Scope line). Required.",
      }),
      pattern: Type.String({
        description: "Text to find: a literal substring (default) or a JavaScript regex source when regexp=true. "
          + `At most ${MAX_PATTERN_CHARS} characters.`,
      }),
      sourceIds: Type.Optional(Type.Array(Type.String({
        description: "Restrict the scan to these sourceIds (all must be inside the scope). Defaults to every source in the scope.",
      }))),
      regexp: Type.Optional(Type.Boolean({
        description: "Treat pattern as a regular expression source (case-sensitive, no flags). Default false (literal substring).",
      })),
      headingFilter: Type.Optional(Type.String({
        description: 'Only scan blocks whose headingPath (joined with " > ") starts with this prefix, e.g. "安装指南" or "安装指南 > Linux".',
      })),
      maxResults: Type.Optional(Type.Number({
        description: `Maximum number of matches to return (1-${MAX_RESULTS_LIMIT}). Default ${DEFAULT_MAX_RESULTS}; `
          + "totalMatches always reports the full count.",
      })),
    }),
    sessionPermission: {
      // 只读原文；研究调用额外登记位置凭据，不修改资料、不调用检索模型或外部请求。
      resolveInvocation: () => ({
        action: "read",
        kind: "read",
        capability: "knowledge_grep.read",
      }),
    },
    execute: async (_toolCallId: any, params: Record<string, any> = {}, _signal?: any, _onUpdate?: any, ctx?: any) => {
      const knowledge = deps.getKnowledge();
      const studioId = deps.getStudioId();
      if (!knowledge || !studioId) {
        return toolError("knowledge_grep unavailable: Knowledge is not accessible in this runtime.", {
          errorCode: "KNOWLEDGE_MODEL_UNAVAILABLE",
        });
      }
      try {
        // scopeId 缺失 = 契约违例：显式拒绝，不得回落到旧的全 studio 行为（§二十二）。
        const scopeId = typeof params.scopeId === "string" && params.scopeId.trim()
          ? params.scopeId.trim()
          : null;
        if (!scopeId) {
          throw knowledgeScopeViolation(
            "scopeId is required: pass the scope id from the [KnowledgeContext] block header (Scope line)",
          );
        }
        if (typeof params.pattern !== "string" || !params.pattern) {
          throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "pattern is required");
        }
        const pattern = params.pattern;
        if (pattern.length > MAX_PATTERN_CHARS) {
          throw new KnowledgeError(
            "KNOWLEDGE_INVALID_ARGUMENT",
            `pattern exceeds the ${MAX_PATTERN_CHARS} character limit`,
          );
        }
        const useRegexp = params.regexp == null ? false : params.regexp;
        if (typeof useRegexp !== "boolean") {
          throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "regexp must be a boolean");
        }
        const headingFilter = optionalTrimmedString(params.headingFilter, "headingFilter", MAX_HEADING_FILTER_CHARS);
        let maxResults = DEFAULT_MAX_RESULTS;
        if (params.maxResults != null) {
          if (typeof params.maxResults !== "number" || !Number.isSafeInteger(params.maxResults)
            || params.maxResults < 1 || params.maxResults > MAX_RESULTS_LIMIT) {
            throw new KnowledgeError(
              "KNOWLEDGE_INVALID_ARGUMENT",
              `maxResults must be an integer between 1 and ${MAX_RESULTS_LIMIT}`,
            );
          }
          maxResults = params.maxResults;
        }
        // regexp 模式：安全受限构造（长度已上限；构造失败/语法非法显式报错，
        // 不回落 literal）。matchAll 需要 g 标志；[Symbol.matchAll] 内部克隆
        // regex，无 lastIndex 污染。零宽命中由规范自动前进，无死循环。
        let regex: RegExp | null = null;
        if (useRegexp) {
          try {
            regex = new RegExp(pattern, "g");
          } catch (error) {
            throw new KnowledgeError(
              "KNOWLEDGE_INVALID_ARGUMENT",
              `pattern is not a valid regular expression: ${error instanceof Error ? error.message : String(error)}`,
            );
          }
        }
        const sessionContext = deps.resolveSessionContext?.(ctx) ?? {
          sessionPath: null,
          parentSessionPath: null,
        };
        const scope = resolveKnowledgeTurnScope({ knowledge, studioId, scopeId, sessionContext });
        const researchContext = deps.resolveResearchContext?.(ctx) ?? null;
        const research = researchContext ? new ResearchStore(knowledge.store) : null;
        if (research && researchContext) {
          const run = research.requireRun(researchContext.runId);
          if (run.turnScopeId !== scopeId || !["planning", "running", "synthesizing"].includes(run.status)
            || (researchContext.allowedSourceIds !== undefined
              && (!Array.isArray(researchContext.allowedSourceIds)
                || researchContext.allowedSourceIds.some(id => !scope.sources.some(source => source.sourceId === id))))) {
            throw knowledgeScopeViolation("Knowledge grep is outside the research scope");
          }
        }

        // sourceIds 全量在 scope 冻结集合内才放行；任一越权整单拒绝（§二十二）。
        let requestedSourceIds: string[] | null = null;
        if (params.sourceIds != null) {
          if (!Array.isArray(params.sourceIds) || params.sourceIds.length === 0
            || params.sourceIds.some(entry => typeof entry !== "string" || !entry.trim())) {
            throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "sourceIds must be a non-empty array of strings");
          }
          requestedSourceIds = [...new Set(params.sourceIds.map(entry => entry.trim()))];
          for (const sourceId of requestedSourceIds) {
            // 越权即抛 KNOWLEDGE_SCOPE_VIOLATION（服务端复核冻结集合）。
            requireKnowledgeScopeSource(scope, sourceId);
            if (researchContext?.allowedSourceIds !== undefined && !researchContext.allowedSourceIds.includes(sourceId)) {
              throw knowledgeScopeViolation("Knowledge grep source is outside the research worker scope");
            }
          }
        }
        const frozenSources = requestedSourceIds
          ? requestedSourceIds
            .map(sourceId => scope.sources.find(frozen => frozen.sourceId === sourceId)!)
            .filter(frozen => frozen != null)
          : scope.sources.filter(source => researchContext?.allowedSourceIds === undefined
            || researchContext.allowedSourceIds.includes(source.sourceId));

        const matches: GrepMatchRecord[] = [];
        const readSpans: KnowledgeGrepReadSpan[] = [];
        const matchedSources = new Set<string>();
        const unavailableSources: Array<{ sourceId: string; reason: string }> = [];
        const scannedSources: Array<{ sourceId: string; sourceName: string; scannedChars: number; matchCount: number }> = [];
        let totalMatches = 0;
        let scannedChars = 0;
        let scanTruncated = false;
        let stoppedAtSourceId: string | null = null;

        outer: for (const frozen of frozenSources) {
          if (frozen.parseArtifactId == null) {
            unavailableSources.push({ sourceId: frozen.sourceId, reason: "KNOWLEDGE_PARSE_NOT_READY: no frozen parse artifact" });
            continue;
          }
          const artifact = knowledge.store.getParseArtifact({
            studioId,
            parseArtifactId: frozen.parseArtifactId,
          });
          if (artifact.status !== "ready") {
            unavailableSources.push({
              sourceId: frozen.sourceId,
              reason: `KNOWLEDGE_PARSE_NOT_READY: frozen artifact status is ${artifact.status}`,
            });
            continue;
          }
          const sourceName = knowledge.getSource({ studioId, sourceId: frozen.sourceId }).displayName;
          const blocks: KnowledgeBlock[] = knowledge.listArtifactBlocks({
            studioId,
            parseArtifactId: frozen.parseArtifactId,
          });
          let sourceMatchCount = 0;
          let sourceScannedChars = 0;
          for (const block of blocks) {
            if (headingFilter && !headingPathMatches(knowledgeBlockHeadingPath(block), headingFilter)) continue;
            if (scannedChars + block.text.length > MAX_SCAN_CHARS) {
              // 显式降级并标注：停在预算边界，不静默丢结果。
              scanTruncated = true;
              stoppedAtSourceId = frozen.sourceId;
              scannedSources.push({ sourceId: frozen.sourceId, sourceName, scannedChars: sourceScannedChars, matchCount: sourceMatchCount });
              break outer;
            }
            sourceScannedChars += block.text.length;
            scannedChars += block.text.length;
            const recordHits = (offset: number, matchLength: number) => {
              matchedSources.add(frozen.sourceId);
              totalMatches += 1;
              sourceMatchCount += 1;
              if (matches.length < maxResults) {
                const snippet = buildSnippet(block.text, offset, matchLength);
                readSpans.push({ sourceId: frozen.sourceId, contentSnapshotId: frozen.contentSnapshotId,
                  parseArtifactId: frozen.parseArtifactId!, blockId: block.id,
                  startOffset: snippet.start, endOffset: snippet.end, canonicalText: snippet.canonicalText });
                matches.push({
                  sourceId: frozen.sourceId,
                  sourceName,
                  parseArtifactId: frozen.parseArtifactId!,
                  blockId: block.id,
                  blockOrdinal: block.ordinal,
                  offset, endOffset: offset + matchLength, matchTruncated: matchLength > 200,
                  lineNumber: 1 + (block.text.slice(0, offset).match(/\n/gu)?.length ?? 0),
                  match: block.text.slice(offset, offset + matchLength).slice(0, 200),
                  snippet: snippet.snippet,
                  headingPath: knowledgeBlockHeadingPath(block),
                });
              }
            };
            if (regex) {
              for (const hit of block.text.matchAll(regex)) {
                recordHits(hit.index ?? 0, hit[0].length);
              }
            } else {
              let cursor = block.text.indexOf(pattern);
              while (cursor !== -1) {
                recordHits(cursor, pattern.length);
                cursor = block.text.indexOf(pattern, cursor + pattern.length);
              }
            }
          }
          scannedSources.push({ sourceId: frozen.sourceId, sourceName, scannedChars: sourceScannedChars, matchCount: sourceMatchCount });
        }

        const truncated = totalMatches > matches.length;
        if (research && researchContext) {
          _signal?.throwIfAborted();
          const receipts = new EvidenceReceiptService(research);
          research.transaction(() => {
            for (const [index, span] of readSpans.entries()) {
              const receipt = receipts.issue({ ...researchContext, ...span, channel: "knowledge_grep" });
              const { text } = receipts.read({ runId: researchContext.runId, receiptId: receipt.id,
                allowedSourceIds: researchContext.allowedSourceIds, actorSessionId: researchContext.actorSessionId });
              if (text !== span.canonicalText) {
                throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Research grep result no longer matches frozen text");
              }
              matches[index].receiptId = receipt.id;
              matches[index].receiptStartOffset = receipt.startOffset;
              matches[index].receiptEndOffset = receipt.endOffset;
            }
          });
        }
        return toolOk(JSON.stringify({
          scopeId,
          mode: useRegexp ? "regexp" : "literal",
          pattern,
          ...(headingFilter ? { headingFilter } : {}),
          ...(requestedSourceIds ? { sourceIds: requestedSourceIds } : {}),
          matches,
          totalMatches,
          ...(truncated
            ? { truncated: true, notice: `result list capped at maxResults=${maxResults}; totalMatches=${totalMatches}. Narrow the pattern or raise maxResults (<= ${MAX_RESULTS_LIMIT}).` }
            : {}),
          scannedChars, matchedSourceCount: matchedSources.size,
          scannedSources,
          ...(unavailableSources.length > 0 ? { unavailableSources } : {}),
          ...(scanTruncated
            ? {
              scanTruncated: true,
              scanBudgetChars: MAX_SCAN_CHARS,
              notice: `scan stopped at the ${MAX_SCAN_CHARS} character budget while processing source ${stoppedAtSourceId}; results are partial for that source — re-run with sourceIds to narrow the scan.`,
            }
            : {}),
        }, null, 2), { scopeId, mode: useRegexp ? "regexp" : "literal", totalMatches, readSpans });
      } catch (error) {
        if (isKnowledgeError(error)) {
          return toolError(`knowledge_grep failed: ${error.code}: ${error.message}`, {
            errorCode: error.code,
          });
        }
        const message = error instanceof Error ? error.message : String(error);
        return toolError(`knowledge_grep failed: ${message}`, {
          errorCode: "KNOWLEDGE_INTERNAL_ERROR",
        });
      }
    },
  };
}
