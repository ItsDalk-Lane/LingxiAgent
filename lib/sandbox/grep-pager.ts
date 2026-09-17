/**
 * grep-pager.ts — grep 工具的输出模式与翻页增强
 *
 * 对照 openclaude GrepTool（content/files_with_matches/count 三模式 +
 * offset 翻页 + 只在真截断时给翻页标记）与 grok grep（limit+1 诚实计数）：
 * - output_mode=files：从匹配结果提取去重文件清单，按修改时间新→旧排序；
 * - output_mode=count：逐文件计数 + 总数；
 * - offset：对匹配（或派生行）分页，只报「至少 N 条」式的诚实计数；
 * - 找不到路径时报错附「你是不是想找」。
 *
 * 包装 pi grep：content 模式复用其 ripgrep 执行与沙盒 operations，
 * files/count 模式由包装层从匹配行派生，不重复起进程。
 */

import { stat as fsStat } from "node:fs/promises";
import { Type } from "../pi-sdk/index.ts";
import { formatDidYouMean, isPathMissingError, suggestSimilarPaths } from "./path-suggestions.ts";

const CONTENT_LINE_PATTERN = /^(.+):(\d+): (.*)$/;
const MAX_SCAN_MATCHES = 1000;
const DEFAULT_FILES_LIMIT = 100;
const MAX_FILES_SCAN = 500;
const DEFAULT_SNIPPET = "No matches found";

function numberOr(value: unknown, fallback: number) {
  const num = Number(value);
  return Number.isFinite(num) && num > 0 ? Math.floor(num) : fallback;
}

type MatchLine = { path: string; line: number; text: string };

function parseContentLines(text: string): MatchLine[] {
  const matches: MatchLine[] = [];
  for (const rawLine of text.split("\n")) {
    const match = rawLine.match(CONTENT_LINE_PATTERN);
    if (!match) continue;
    matches.push({ path: match[1], line: Number(match[2]), text: rawLine });
  }
  return matches;
}

async function sortByMtimeDesc(paths: string[]): Promise<Array<{ path: string; mtimeMs: number }>> {
  const stamped = await Promise.all(paths.map(async (filePath) => {
    try {
      const stats = await fsStat(filePath);
      return { path: filePath, mtimeMs: stats.mtimeMs };
    } catch {
      return { path: filePath, mtimeMs: 0 };
    }
  }));
  return stamped.sort((a, b) => b.mtimeMs - a.mtimeMs || a.path.localeCompare(b.path));
}

export function wrapGrepToolWithModes(innerTool: any) {
  if (!innerTool?.parameters?.properties) return innerTool;
  const parameters = Type.Object({
    ...innerTool.parameters.properties,
    output_mode: Type.Optional(Type.String({
      description: "Result format: \"content\" (default) prints each matching line as path:line: text; \"files\" lists only matching file paths sorted newest-first; \"count\" prints per-file match counts plus a total.",
    })),
    offset: Type.Optional(Type.Number({
      description: "Number of results to skip before printing (0-based pagination over matches). Default 0. Combine with limit to page; the result footer reports whether more remain.",
    })),
  });
  const description = `${innerTool.description || "Search file contents."} output_mode=files/count derive compact summaries from the same search; use offset to page through results when the footer says more remain.`;

  return {
    ...innerTool,
    parameters,
    description,
    async execute(toolCallId: string, params: any = {}, signal: any, onUpdate: any, ctx: any) {
      const outputMode = typeof params.output_mode === "string" && ["content", "files", "count"].includes(params.output_mode)
        ? params.output_mode
        : "content";
      const offset = numberOr(params.offset, 0);
      const effectiveParams = { ...params };
      delete effectiveParams.output_mode;
      delete effectiveParams.offset;

      let result: any;
      try {
        if (outputMode === "content") {
          const limit = numberOr(params.limit, 100);
          const contextValue = Number(params.context) > 0 ? Number(params.context) : 0;
          if (offset > 0 && contextValue > 0) {
            return {
              isError: true,
              content: [{ type: "text", text: "offset pagination requires context=0 (context blocks are not paginated). Drop context or narrow the pattern instead." }],
              details: { errorCode: "GREP_OFFSET_WITH_CONTEXT" },
            };
          }
          effectiveParams.limit = Math.min(offset + limit, MAX_SCAN_MATCHES);
          result = await innerTool.execute(toolCallId, effectiveParams, signal, onUpdate, ctx);
          return paginateContentResult(result, { offset, limit });
        }

        const scanLimit = outputMode === "files" ? MAX_FILES_SCAN : MAX_SCAN_MATCHES;
        effectiveParams.limit = scanLimit;
        result = await innerTool.execute(toolCallId, effectiveParams, signal, onUpdate, ctx);
        return await deriveSummaryResult(result, {
          outputMode,
          offset,
          limit: numberOr(params.limit, outputMode === "files" ? DEFAULT_FILES_LIMIT : 100),
        });
      } catch (err) {
        if (isPathMissingError(err)) {
          const missingPath = typeof params.path === "string" ? params.path : "";
          const candidates = missingPath ? await suggestSimilarPaths(missingPath) : [];
          const suffix = formatDidYouMean(candidates, missingPath);
          if (suffix) {
            const message = err instanceof Error ? err.message : String(err);
            throw new Error(`${message}${suffix}`, { cause: err });
          }
        }
        throw err;
      }
    },
  };
}

function paginateContentResult(result: any, { offset, limit }: { offset: number; limit: number }) {
  if (!result || result.isError) return result;
  const block = result.content?.find?.((item: any) => item?.type === "text");
  const text = typeof block?.text === "string" ? block.text : "";
  if (!text || text === DEFAULT_SNIPPET) return result;
  const lines = text.split("\n");
  // 找到截断脚标（[...] 结尾段）——正文行与提示行分开处理。
  let footerStart = -1;
  if (result.details?.matchLimitReached || result.details?.truncation) {
    for (let i = lines.length - 1; i >= 0; i--) {
      if (lines[i].startsWith("[")) {
        footerStart = i;
        break;
      }
    }
  }
  const footerLines = footerStart >= 0 ? lines.slice(footerStart) : [];
  const bodyLines = footerStart >= 0 ? lines.slice(0, footerStart) : lines;
  const page = bodyLines.slice(offset, offset + limit);
  if (page.length === 0) {
    if (offset === 0) return result;
    return {
      ...result,
      content: [{ type: "text", text: `[No results in range ${offset + 1}-${offset + limit}. The search found ${bodyLines.length} match lines; use a smaller offset.]` }],
    };
  }
  const shownEnd = offset + page.length;
  // 内层命中上限、或正文行数超过本页末尾（内层未截断但还有剩余）都算还有更多。
  const moreRemain = result.details?.matchLimitReached === true || shownEnd < bodyLines.length;
  const footerBits: string[] = [];
  if (offset > 0 || moreRemain) {
    footerBits.push(moreRemain
      ? `Showing matches ${offset + 1}-${shownEnd}; more may remain — use offset=${shownEnd} for the next page`
      : `Showing matches ${offset + 1}-${shownEnd} (all results)`);
  }
  const parts = [page.join("\n")];
  if (footerBits.length > 0) parts.push(`[${footerBits.join(". ")}]`);
  parts.push(...footerLines);
  return {
    ...result,
    content: [{ type: "text", text: parts.join("\n\n") }],
  };
}

async function deriveSummaryResult(result: any, { outputMode, offset, limit }: { outputMode: string; offset: number; limit: number }) {
  if (!result || result.isError) return result;
  const block = result.content?.find?.((item: any) => item?.type === "text");
  const text = typeof block?.text === "string" ? block.text : "";
  if (!text || text === DEFAULT_SNIPPET) {
    return {
      ...result,
      content: [{ type: "text", text: outputMode === "files" ? "No files found" : "No matches found" }],
    };
  }
  const matchLines = parseContentLines(text);
  const truncatedNotice = Boolean(result.details?.matchLimitReached);
  if (outputMode === "count") {
    const counts = new Map<string, number>();
    for (const match of matchLines) {
      counts.set(match.path, (counts.get(match.path) || 0) + 1);
    }
    const rows = [...counts.entries()]
      .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
      .map(([filePath, count]) => `${filePath}: ${count}`);
    const page = rows.slice(offset, offset + limit);
    const totalMatches = matchLines.length;
    const header = `Found ${totalMatches}${truncatedNotice ? "+" : ""} total match${totalMatches === 1 ? "" : "es"} across ${counts.size} file${counts.size === 1 ? "" : "s"}.`;
    const footer = truncatedNotice || offset > 0
      ? `\n\n[${truncatedNotice ? `Scan capped at ${MAX_SCAN_MATCHES} matches — counts are a lower bound. ` : ""}${offset > 0 ? `Showing file rows ${offset + 1}-${offset + page.length}.` : ""}]`
      : "";
    return {
      ...result,
      content: [{ type: "text", text: [header, ...page].join("\n") + footer }],
    };
  }

  const uniquePaths: string[] = [];
  const seen = new Set<string>();
  for (const match of matchLines) {
    if (!seen.has(match.path)) {
      seen.add(match.path);
      uniquePaths.push(match.path);
    }
  }
  const sorted = await sortByMtimeDesc(uniquePaths);
  const names = sorted.map((item) => item.path);
  const page = names.slice(offset, offset + limit);
  if (page.length === 0) {
    return {
      ...result,
      content: [{ type: "text", text: `[No files in range ${offset + 1}-${offset + limit}; the search found ${names.length} files. Use a smaller offset.]` }],
    };
  }
  const footerBits: string[] = [`Found ${names.length}${truncatedNotice ? "+" : ""} matching file${names.length === 1 ? "" : "s"} (newest first)`];
  if (offset + page.length < names.length || truncatedNotice) {
    footerBits.push(truncatedNotice
      ? "scan capped — list may be incomplete"
      : `showing ${offset + 1}-${offset + page.length}; use offset=${offset + page.length} for more`);
  }
  return {
    ...result,
    content: [{ type: "text", text: `${footerBits.join(" — ")}:\n${page.join("\n")}` }],
  };
}
