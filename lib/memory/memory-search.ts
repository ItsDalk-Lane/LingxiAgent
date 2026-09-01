/**
 * memory-search.js — search_memory 工具（v2 标签检索）
 *
 * 替代 v1 的 embedding KNN + 混合排序 + 链接展开。
 * v2 用标签匹配 + 日期过滤 + FTS5 全文搜索兜底。
 *
 * 标签由 LLM 在元事实拆分时生成，也由 LLM 在搜索时生成查询标签，
 * 两边的"语言习惯"天然接近，一致性有保障。
 */

import { Type } from "../pi-sdk/index.ts";
import { t } from "../i18n.ts";
import { createModuleLogger } from "../debug-log.ts";
import { rrfFuse } from "./fact-embeddings.ts";

const log = createModuleLogger("memory-search");

const CHANNEL_SESSION_PREFIX = "channel-";

/**
 * 语义检索状态（details.semantic + 零结果诊断用）。
 * 禁静默降级：任何跳过语义路径的情形都显式带状态出结果。
 */
export type MemorySemanticStatus =
  | "used"
  | "unavailable_no_model"
  | "unavailable_unresolvable"
  | "skipped_timeout"
  | "skipped_no_coverage"
  | "not_configured";

export interface MemoryEmbedQueryResult {
  status: "ok";
  vector: number[];
  modelKey: string;
}

export type MemoryEmbedQuery =
  | MemoryEmbedQueryResult
  | { status: "unavailable"; reason: "no_model" | "unresolvable" }
  | { status: "timeout" };

function semanticStatusKey(status: MemorySemanticStatus): string {
  switch (status) {
    case "unavailable_no_model":
    case "unavailable_unresolvable":
      return "error.memorySearchSemanticUnavailable";
    case "skipped_timeout":
      return "error.memorySearchSemanticTimeout";
    case "skipped_no_coverage":
      return "error.memorySearchSemanticNoCoverage";
    default:
      return "";
  }
}

/**
 * 会话作用域过滤：频道 phone 会话默认看不到「其它频道」的事实。
 * 通用事实（session_id 为空或非频道）和当前频道的事实始终可见，
 * 跨频道检索必须显式传 cross_channel: true（#1670 群聊记忆混淆）。
 */
function factVisibleInConversationScope(row, scope, crossChannel) {
  if (!scope || scope.kind !== "channel") return true;
  const sessionId = typeof row?.session_id === "string" ? row.session_id : "";
  if (!sessionId.startsWith(CHANNEL_SESSION_PREFIX)) return true;
  if (sessionId === `${CHANNEL_SESSION_PREFIX}${scope.channelId}`) return true;
  return crossChannel === true;
}

const ZERO_RESULT_DIAGNOSTIC_MAX_TERMS = 6;

/**
 * 零结果诊断：把 query 拆词后逐词独立计数，让模型能自己定位
 * 「哪个词断了」并改写查询重试（借鉴 nuphus 的逐词命中数反馈）。
 * 只做诊断输出，不影响检索路径本身。
 */
function buildZeroResultDiagnostics(factStore, query) {
  const terms = Array.from(
    new Set(
      String(query || "")
        .split(/\s+/)
        .map((s) => s.trim())
        .filter(Boolean),
    ),
  ).slice(0, ZERO_RESULT_DIAGNOSTIC_MAX_TERMS);
  if (terms.length === 0) return null;

  const counts = terms.map((term) => ({
    term,
    count: factStore.countFullTextMatches(term),
  }));
  const countsText = counts.map(({ term, count }) => `『${term}』${count} 条`).join("、");
  return { counts, countsText, hasZeroTerm: counts.some((c) => c.count === 0) };
}

/**
 * 创建 search_memory 工具定义
 * @param {import('./fact-store.ts').FactStore} factStore
 * @param {object} [opts]
 * @param {function} [opts.getMemoryMasterEnabled] - 返回 agent 级别记忆总开关状态
 * @param {{kind:"channel", channelId:string}} [opts.conversationScope]
 *   - 会话作用域。频道 phone 会话注入后，默认排除其它频道的事实；
 *     scoped 实例的 schema 额外暴露 cross_channel 参数供显式跨频道检索
 * @param {(query: string) => Promise<import('./memory-search.ts').MemoryEmbedQuery>} [opts.embedQuery]
 *   - 语义检索闭包（engine 注入；未传 = 语义路径未接线，行为同旧版）
 * @returns {import('../pi-sdk/index.ts').ToolDefinition}
 */
export function createMemorySearchTool(factStore, opts: any = {}) {
  const conversationScope = opts.conversationScope?.kind === "channel" && opts.conversationScope.channelId
    ? { kind: "channel" as const, channelId: String(opts.conversationScope.channelId) }
    : null;
  return {
    name: "search_memory",
    label: t("error.memorySearchLabel"),
    description: t("error.memorySearchDesc"),
    parameters: Type.Object({
      query: Type.String({ description: t("error.memorySearchQueryDesc") }),
      tags: Type.Optional(
        Type.Array(Type.String(), {
          description: t("error.memorySearchTagsDesc"),
        }),
      ),
      date_from: Type.Optional(
        Type.String({ description: t("error.memorySearchDateFromDesc") }),
      ),
      date_to: Type.Optional(
        Type.String({ description: t("error.memorySearchDateToDesc") }),
      ),
      ...(conversationScope ? {
        cross_channel: Type.Optional(
          Type.Boolean({ description: t("error.memorySearchCrossChannelDesc") }),
        ),
      } : {}),
    }),
    execute: async (_toolCallId, params) => {
      try {
        const t0 = performance.now();

        if (factStore.size === 0) {
          return {
            content: [{ type: "text", text: t("error.memorySearchEmpty") }],
            details: {},
          };
        }

        const dateRange: { from?: string; to?: string } = {};
        if (params.date_from) dateRange.from = params.date_from;
        if (params.date_to) dateRange.to = params.date_to + "T23:59";

        let results = [];
        const seenIds = new Set();

        const crossChannel = conversationScope ? params.cross_channel === true : false;
        const visibleInScope = (row) => factVisibleInConversationScope(row, conversationScope, crossChannel);

        // 策略 1：标签匹配（优先）
        if (params.tags && params.tags.length > 0) {
          const tagResults = factStore.searchByTags(
            params.tags,
            Object.keys(dateRange).length > 0 ? dateRange : undefined,
            15,
          );
          for (const r of tagResults) {
            if (!visibleInScope(r)) continue;
            seenIds.add(r.id);
            results.push({ ...r, source: "tag" });
          }
        }

        // 策略 2：全文 + 语义混合（标签结果不足 3 条时）
        // 语义路径由 engine 注入的 embedQuery 闭包驱动；未接线/未配置/超时/
        // 零覆盖时按旧版 FTS 单路走，状态显式进 details（禁静默降级）。
        let semanticStatus: MemorySemanticStatus = "not_configured";
        if (results.length < 3 && params.query) {
          const ftsResults = factStore.searchFullText(params.query, 10);

          let semanticRows: any[] = [];
          if (typeof opts.embedQuery === "function") {
            let embedResult: MemoryEmbedQuery | null = null;
            try {
              embedResult = await opts.embedQuery(params.query);
            } catch {
              embedResult = { status: "timeout" };
            }
            if (embedResult?.status === "ok") {
              const coverage = factStore.embeddingCoverage(embedResult.modelKey);
              if (coverage.embedded === 0) {
                semanticStatus = "skipped_no_coverage";
              } else {
                semanticStatus = "used";
                semanticRows = factStore
                  .semanticSearch(embedResult.modelKey, embedResult.vector, 15)
                  .filter((r) => visibleInScope(r));
              }
            } else if (embedResult?.status === "unavailable") {
              semanticStatus = embedResult.reason === "unresolvable"
                ? "unavailable_unresolvable"
                : "unavailable_no_model";
            } else {
              semanticStatus = "skipped_timeout";
            }
          }

          if (semanticRows.length > 0) {
            // RRF 融合：FTS 与语义两路排名 → 统一顺序；命中两路的标 semantic+fts
            const ftsById = new Map(ftsResults.filter(visibleInScope).map((r) => [r.id, r]));
            const semById = new Map(semanticRows.map((r) => [r.id, r]));
            const fused = rrfFuse([
              [...ftsById.keys()],
              [...semById.keys()],
            ]);
            for (const [id] of fused) {
              if (seenIds.has(id)) continue;
              const fromFts = ftsById.has(id);
              const fromSem = semById.has(id);
              const row = fromSem ? semById.get(id) : ftsById.get(id);
              if (!row) continue;
              seenIds.add(id);
              results.push({
                ...row,
                source: fromFts && fromSem ? "semantic+fts" : fromSem ? "semantic" : "fts",
              });
            }
          } else {
            for (const r of ftsResults) {
              if (seenIds.has(r.id)) continue;
              if (!visibleInScope(r)) continue;
              seenIds.add(r.id);
              results.push({ ...r, source: "fts" });
            }
          }
        }

        // 日期过滤（对 FTS 结果也应用）
        if (dateRange.from || dateRange.to) {
          results = results.filter((r) => {
            if (!r.time) return true; // 无时间的不过滤
            if (dateRange.from && r.time < dateRange.from) return false;
            if (dateRange.to && r.time > dateRange.to) return false;
            return true;
          });
        }

        const elapsed = performance.now() - t0;
        log.log(
          `${elapsed.toFixed(0)}ms | ` +
          `hits: ${results.length} (tag: ${results.filter((r) => r.source === "tag").length}, ` +
          `fts: ${results.filter((r) => r.source === "fts").length}, ` +
          `semantic: ${results.filter((r) => String(r.source).startsWith("semantic")).length}, ` +
          `path: ${semanticStatus})`,
        );

        if (results.length === 0) {
          const diagnostics = params.query
            ? buildZeroResultDiagnostics(factStore, params.query)
            : null;
          const textParts = [t("error.memorySearchEmpty")];
          if (diagnostics) {
            textParts.push(t("error.memorySearchTermCounts", { counts: diagnostics.countsText }));
            if (diagnostics.hasZeroTerm) {
              textParts.push(t("error.memorySearchRetryHint"));
            }
          }
          if (semanticStatus !== "used" && semanticStatus !== "not_configured") {
            // 显式降级标注：语义路径被跳过的原因要让模型知道
            textParts.push(t(semanticStatusKey(semanticStatus)));
          }
          return {
            content: [{ type: "text", text: textParts.join("\n") }],
            details: {
              ...(diagnostics ? { diagnostics: { terms: diagnostics.counts } } : {}),
              ...(semanticStatus !== "not_configured" ? { semantic: semanticStatus } : {}),
            },
          };
        }

        // 格式化输出
        const lines = results.map((r, i) => {
          const tagsStr = r.tags.length > 0 ? ` (${r.tags.join(", ")})` : "";
          const timeStr = r.time ? ` — ${r.time}` : "";
          return `${i + 1}. ${r.fact}${tagsStr}${timeStr}`;
        });

        return {
          content: [{ type: "text", text: lines.join("\n") }],
          details: {
            resultCount: results.length,
            ...(semanticStatus !== "not_configured" ? { semantic: semanticStatus } : {}),
          },
        };
      } catch (err) {
        return {
          content: [{ type: "text", text: t("error.memorySearchError", { msg: err.message }) }],
          details: {},
        };
      }
    },
  };
}
