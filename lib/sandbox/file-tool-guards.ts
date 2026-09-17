/**
 * file-tool-guards.ts — read/write/edit 工具的行为守卫包装
 *
 * 组合顺序（外→内）：
 *   路径建议（ENOENT 提示） → 新鲜度守卫（edit/write 陈旧拦截+登记） →
 *   编辑报错增强 → 既有 wrapFileTouchTool → Pi 原工具
 *
 * 这些包装只改「报错文本」与「是否拦截」，不改变匹配/写入语义。
 */

import fs from "node:fs";
import path from "node:path";
import {
  readIdentityKey,
  type FileFreshnessTracker,
} from "./file-freshness.ts";
import { buildEditErrorHintWithFile, normalizeEditList } from "./edit-error-hints.ts";
import { formatDidYouMean, isPathMissingError, suggestSimilarPaths } from "./path-suggestions.ts";

function textOf(result: any): string {
  if (!result || result.isError !== true) return "";
  return (result.content || [])
    .filter((item: any) => item?.type === "text")
    .map((item: any) => item?.text || "")
    .join("\n");
}

function resolveToolPath(rawPath: unknown, cwd: string): string {
  if (typeof rawPath !== "string" || !rawPath) return "";
  return path.isAbsolute(rawPath) ? rawPath : path.resolve(cwd, rawPath);
}

/** read：成功后登记指纹；紧邻相同读取返回存根（token 去重）。 */
export function wrapReadToolWithFreshness(tool: any, {
  tracker,
  getSessionPath,
  cwd,
}: {
  tracker: FileFreshnessTracker;
  getSessionPath?: () => string | null;
  cwd: string;
}) {
  return {
    ...tool,
    execute: async (toolCallId: string, params: any = {}, ...rest: any[]) => {
      const absolutePath = resolveToolPath(params.path, cwd);
      if (absolutePath) {
        const duplicate = await tracker.checkDuplicateRead(
          getSessionPath?.(),
          absolutePath,
          readIdentityKey(params),
        );
        if (duplicate.duplicate) {
          return {
            content: [{
              type: "text",
              text: `[Duplicate read: ${params.path} is unchanged since your identical read ${duplicate.ageSeconds}s ago. The earlier tool result is still current — refer to it instead of re-reading. Read with a different offset or limit to force fresh content.]`,
            }],
            details: { duplicateRead: true },
          };
        }
      }
      const result = await tool.execute(toolCallId, params, ...rest);
      const textOnly = Array.isArray(result?.content)
        && result.content.length > 0
        && result.content.every((item: any) => item?.type === "text");
      if (!result?.isError && textOnly && absolutePath) {
        await tracker.observeRead(getSessionPath?.(), absolutePath, readIdentityKey(params));
      }
      return result;
    },
  };
}

/** edit/write：写前发现文件被外部改过 → 拦截并教重读；成功后回写指纹。 */
export function wrapMutationToolWithFreshness(tool: any, {
  tracker,
  getSessionPath,
  cwd,
}: {
  tracker: FileFreshnessTracker;
  getSessionPath?: () => string | null;
  cwd: string;
}) {
  return {
    ...tool,
    execute: async (toolCallId: string, params: any = {}, ...rest: any[]) => {
      const absolutePath = resolveToolPath(params.path, cwd);
      if (absolutePath && fs.existsSync(absolutePath)) {
        const staleness = await tracker.checkFreshBeforeMutation(getSessionPath?.(), absolutePath);
        if (staleness.stale) {
          return {
            isError: true as const,
            content: [{
              type: "text",
              text: `File has been modified since it was last read (changed by the user or another process). Re-read the file, then retry this change.`,
            }],
            details: { errorCode: "FILE_STALE_SINCE_READ" },
          };
        }
      }
      const result = await tool.execute(toolCallId, params, ...rest);
      if (result?.isError !== true && absolutePath) {
        await tracker.observeMutation(getSessionPath?.(), absolutePath);
      }
      return result;
    },
  };
}

/** edit：匹配类失败（多匹配/找不到）的报错附行号与最近匹配提示。 */
export function wrapEditToolWithErrorHints(tool: any, { cwd }: { cwd: string }) {
  return {
    ...tool,
    execute: async (toolCallId: string, params: any = {}, ...rest: any[]) => {
      const result = await tool.execute(toolCallId, params, ...rest);
      const message = textOf(result);
      if (!message) return result;
      const duplicate = /^Found \d+ occurrences of (?:the text|edits\[\d+\]) in/m.test(message);
      const notFound = /^Could not find (?:the exact text|edits\[\d+\]) in/m.test(message);
      if (!duplicate && !notFound) return result;
      const absolutePath = resolveToolPath(params.path, cwd);
      if (!absolutePath) return result;
      try {
        const hint = await buildEditErrorHintWithFile(message, absolutePath, normalizeEditList(params));
        if (!hint) return result;
        return {
          ...result,
          content: [{ type: "text", text: hint }],
        };
      } catch {
        return result;
      }
    },
  };
}

/** read/edit：路径不存在类失败附「你是不是想找」候选。 */
export function wrapFileToolWithPathSuggestions(tool: any, { cwd }: { cwd: string }) {
  return {
    ...tool,
    execute: async (toolCallId: string, params: any = {}, ...rest: any[]) => {
      try {
        const result = await tool.execute(toolCallId, params, ...rest);
        const message = textOf(result);
        if (message && isPathMissingError(message)) {
          const enriched = await enrichWithSuggestions(message, params, cwd);
          if (enriched) {
            return { ...result, content: [{ type: "text", text: enriched }] };
          }
        }
        return result;
      } catch (err) {
        if (isPathMissingError(err)) {
          const enriched = await enrichWithSuggestions(err instanceof Error ? err.message : String(err), params, cwd);
          if (enriched) {
            throw new Error(enriched, { cause: err });
          }
        }
        throw err;
      }
    },
  };
}

async function enrichWithSuggestions(message: string, params: any, cwd: string): Promise<string | null> {
  const rawPath = typeof params?.path === "string" ? params.path : "";
  if (!rawPath) return null;
  const absolutePath = path.resolve(cwd, rawPath);
  const candidates = await suggestSimilarPaths(absolutePath);
  const suffix = formatDidYouMean(candidates, path.dirname(absolutePath));
  if (!suffix) return null;
  return `${message}${suffix}`;
}
