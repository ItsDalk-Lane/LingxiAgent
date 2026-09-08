/**
 * pinned-memory.ts — pin_memory / unpin_memory 自定义工具
 *
 * 让 agent 通过工具调用来管理「置顶与原则」库（tenets）。工具名保持不变
 * （权限名单与历史会话的工具调用记录零成本兼容），写入侧已统一到
 * tenets 存储：pin = 直接生效的 user_direct 条目（写前 PII 脱敏）；
 * unpin = 按 id 或关键词模糊删除 active 条目。
 */

import { Type } from "../pi-sdk/index.ts";
import { t } from "../i18n.ts";
import { scrubPII } from "../pii-guard.ts";
import { createModuleLogger } from "../debug-log.ts";
import { toolError } from "./tool-result.ts";
import {
  activeTenets,
  addTenetDirect,
  removeTenet,
  isTenetError,
} from "../memory/tenets.ts";

const log = createModuleLogger("pin_memory");

function stableMemoryTargetId(value: unknown) {
  return Buffer.from(JSON.stringify(value), "utf-8").toString("base64url");
}

/**
 * 创建 pin_memory + unpin_memory 工具
 * @param {string} agentDir - agent 数据目录（tenets.json 在 memory/ 下）
 */
export function createPinnedMemoryTools(agentDir: string) {
  const pinTool = {
    name: "pin_memory",
    label: "Pin Memory",
    description: "Save an item to pinned memory. Use when the user says 'remember this', 'note this down', 'don't forget this later'. Pinned memories are always kept in context.",
    sessionPermission: {
      resolveInvocation: (params: any = {}) => {
        if (typeof params.content !== "string" || !params.content.trim()) return null;
        return {
          action: "pin",
          kind: "routine",
          capability: "pin_memory.pin",
          target: { type: "memory_store", id: "pinned", label: "Pinned memory" },
        };
      },
    },
    parameters: Type.Object({
      content: Type.String({ description: "Content to remember" }),
    }),
    execute: async (_toolCallId, params) => {
      const { cleaned, detected } = scrubPII(params.content);
      if (detected.length > 0) {
        log.warn(`PII detected (${detected.join(", ")}), redacted before storage`);
      }

      try {
        const result = addTenetDirect(agentDir, { content: cleaned });
        if (result.duplicate) {
          return {
            content: [{ type: "text", text: t("error.pinnedAlreadyExists") }],
            details: {},
          };
        }
        return {
          content: [{ type: "text", text: t("error.pinnedAdded", { content: cleaned }) }],
          details: { item: { id: result.tenet.id, content: result.tenet.content, createdAt: result.tenet.createdAt } },
        };
      } catch (err: any) {
        // 写入失败必须按工具失败契约返回并保留具体错误码；
        // 不得返回「已添加」式成功文案（旧实现会把 INVALID 错报成容量满的假成功）。
        if (isTenetError(err)) {
          return toolError(err?.message || "failed to pin memory", { errorCode: err?.code });
        }
        throw err;
      }
    },
  };

  const unpinTool = {
    name: "unpin_memory",
    label: "Unpin Memory",
    description: "Remove an item from pinned memory. Use when the user says 'forget xxx' or 'delete this memory'. Supports fuzzy matching: any entry containing the keyword you provide will be removed.",
    sessionPermission: {
      resolveInvocation: (params: any = {}) => {
        const id = typeof params.id === "string" ? params.id.trim() : "";
        const keyword = typeof params.keyword === "string" ? params.keyword.trim() : "";
        if (!id && !keyword) return null;
        if (id && !keyword) {
          return {
            action: "unpin",
            kind: "review",
            capability: "unpin_memory.unpin",
            target: { type: "pinned_memory_item", id, label: id },
          };
        }
        return {
          action: "unpin",
          kind: "review",
          capability: "unpin_memory.unpin",
          target: {
            type: "pinned_memory_query",
            id: stableMemoryTargetId({ ...(id ? { id } : {}), keyword }),
            label: keyword || id,
          },
        };
      },
    },
    parameters: Type.Object({
      id: Type.Optional(Type.String({ description: "Pinned memory entity id returned by pin_memory" })),
      keyword: Type.Optional(Type.String({ description: "Keyword of the memory to remove, matched fuzzily" })),
    }),
    execute: async (_toolCallId, params) => {
      const existing = activeTenets(agentDir);
      if (existing.length === 0) {
        return {
          content: [{ type: "text", text: t("error.pinnedEmpty") }],
          details: {},
        };
      }

      const normalizedId = typeof params.id === "string" ? params.id.trim() : "";
      const normalizedKeyword = typeof params.keyword === "string"
        ? params.keyword.replace(/\r\n?/g, "\n").trim().toLowerCase()
        : "";
      if (!normalizedId && !normalizedKeyword) {
        return {
          content: [{ type: "text", text: t("error.pinnedNotFound", { keyword: "" }) }],
          details: {},
        };
      }

      const removed: Array<{ id: string; content: string }> = [];
      for (const tenet of existing) {
        const matchesId = normalizedId && tenet.id === normalizedId;
        const matchesKeyword = normalizedKeyword && tenet.content.toLowerCase().includes(normalizedKeyword);
        if (matchesId || matchesKeyword) {
          removeTenet(agentDir, tenet.id);
          removed.push({ id: tenet.id, content: tenet.content });
        }
      }

      if (removed.length === 0) {
        const keyword = params.keyword || params.id || "";
        return {
          content: [{ type: "text", text: t("error.pinnedNotFound", { keyword }) }],
          details: {},
        };
      }

      return {
        content: [{ type: "text", text: t("error.pinnedRemoved", { count: removed.length, items: removed.map(item => item.content).join(", ") }) }],
        details: { removedCount: removed.length, removedItems: removed },
      };
    },
  };

  return [pinTool, unpinTool];
}
