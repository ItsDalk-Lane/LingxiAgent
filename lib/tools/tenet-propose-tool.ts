/**
 * tenet-propose-tool.js — tenet_propose 工具（用户原则提案）
 *
 * 模型只能「提议」，不能直接生效：execute 写入 pending 后由用户在聊天审批卡
 * 或设置页批准/拒绝（持久等待，不走 confirm-store 的限时阻塞）。
 * 与既有条目归一化重复时幂等返回现状；pending/active 上限显式报错不静默。
 */

import { Type } from "../pi-sdk/index.ts";
import { t } from "../i18n.ts";
import {
  TENET_ERRORS,
  MAX_TENET_CONTENT_CHARS,
  TENET_PRIORITIES,
  addTenetProposal,
} from "../memory/tenets.ts";

/**
 * @param {string} agentDir
 * @param {object} [opts]
 * @param {() => boolean} [opts.isEnabled] - 记忆开关（关闭时工具返回暂停提示）
 * @param {(tenet: object) => void} [opts.onProposed] - 新提案落地后的回调（通知前端刷新审批卡）
 * @returns {import('../pi-sdk/index.ts').ToolDefinition}
 */
export function createTenetProposeTool(agentDir, opts: {
  isEnabled?: () => boolean;
  onProposed?: (tenet: any) => void;
} = {}) {
  const { isEnabled, onProposed } = opts;
  return {
    name: "tenet_propose",
    label: t("error.tenetProposeLabel"),
    description: t("error.tenetProposeDesc"),
    sessionPermission: {
      resolveInvocation: (params: any = {}) => {
        if (typeof params.content !== "string") return null;
        const content = params.content.trim();
        if (!content) return null;
        // 提案只是写 pending、等用户批准，不是即时生效的长期记忆写入：
        // 归 routine 档（审批由独立的持久 UI 承载，不绑限时 confirm）
        return {
          action: "record",
          kind: "routine",
          capability: "tenet_propose.propose",
          target: {
            type: "tenet_proposal",
            id: "tenet_proposal",
            label: content.slice(0, 60),
          },
        };
      },
    },
    parameters: Type.Object({
      content: Type.String({ description: t("error.tenetProposeContentDesc") }),
      priority: Type.Optional(
        Type.String({
          description: t("error.tenetProposePriorityDesc"),
        }),
      ),
    }),
    execute: async (_toolCallId, params) => {
      if (isEnabled && !isEnabled()) {
        return {
          content: [{ type: "text", text: t("error.tenetProposePaused") }],
          details: {},
        };
      }
      const content = String(params.content || "").trim();
      if (!content) {
        return {
          content: [{ type: "text", text: t("error.tenetProposeInvalid") }],
          details: {},
        };
      }
      if (content.length > MAX_TENET_CONTENT_CHARS) {
        return {
          content: [{ type: "text", text: t("error.tenetProposeTooLong", { max: MAX_TENET_CONTENT_CHARS }) }],
          details: {},
        };
      }
      const priority = TENET_PRIORITIES.includes(params.priority) ? params.priority : undefined;
      try {
        const result = addTenetProposal(agentDir, { content, priority });
        if (result.duplicate) {
          return {
            content: [{ type: "text", text: t("error.tenetProposeDuplicate", { status: result.existingStatus || "pending" }) }],
            details: { duplicate: true, tenetId: result.tenet.id, status: result.tenet.status },
          };
        }
        onProposed?.(result.tenet);
        return {
          content: [{ type: "text", text: t("error.tenetProposeSubmitted") }],
          details: { tenetId: result.tenet.id, status: "pending", priority: result.tenet.priority },
        };
      } catch (err: any) {
        if (err?.code === TENET_ERRORS.PENDING_FULL) {
          return {
            content: [{ type: "text", text: t("error.tenetProposePendingFull") }],
            details: { code: TENET_ERRORS.PENDING_FULL },
          };
        }
        return {
          content: [{ type: "text", text: t("error.tenetProposeError", { msg: err?.message || String(err) }) }],
          details: {},
        };
      }
    },
  };
}
