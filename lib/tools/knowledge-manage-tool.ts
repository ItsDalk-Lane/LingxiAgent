/**
 * knowledge_manage 工具 —— 知识库笔记本的修改性操作入口（Phase 11，任务书
 * §二十三 Agent Knowledge 工具体系）。action ∈ add / remove / refresh / reindex，
 * 全部委托 KnowledgeManager 既有方法（importFile / importPastedText /
 * importWebSnapshot / removeSourceFromNotebook / refreshFileSource /
 * requeueSourceIngestion / enqueueNotebookRebuild），本工具不新建业务逻辑。
 *
 * 权限边界（任务书 §二十一~§二十二 + 修改性工具纪律）：
 * - 修改性工具：sessionPermission.resolveInvocation 返回 kind "review"
 *   （session-permission-mode 的审批档——auto→review / ask→prompt /
 *   operate→allow / read_only→拒绝；现有 kind 枚举只有 read/routine/review，
 *   "review" 即必须审批档），且进 SUBAGENT_BLOCKED_TOOLS（子 Agent 不可用）；
 * - studio 隔离：所有委托调用都带 studioId（store 层逐次校验 notebook/source
 *   归属），不信任模型传入的任何 id——notebookId/sourceId 非法归属由
 *   KnowledgeManager 抛 KNOWLEDGE_NOT_FOUND 等显式错误；
 * - 不受 KnowledgeTurnScope 约束（管理面操作，非本轮读取；scope 冻结语义由
 *   读取侧保证——watcher 新版本下一轮 scope 才生效，§四十三）；
 * - 参数校验严格：未知 action / kind 与字段不匹配显式报 KNOWLEDGE_INVALID_ARGUMENT。
 */
import { Type } from "../pi-sdk/index.ts";
import { isKnowledgeError, KnowledgeError } from "../knowledge/errors.ts";
import type { KnowledgeManager } from "../knowledge/knowledge-manager.ts";
import { toolError, toolOk } from "./tool-result.ts";

const MANAGE_ACTIONS = ["add", "remove", "refresh", "reindex"] as const;
const ADD_KINDS = ["file", "pasted_text", "web_snapshot"] as const;

export interface KnowledgeManageToolDeps {
  /** engine 级 KnowledgeManager（跨会话）；null = Knowledge 不可用。 */
  getKnowledge: () => KnowledgeManager | null;
  /** 当前 runtime studioId；null = 运行时上下文不可用。 */
  getStudioId: () => string | null;
}

function requireNonEmptyString(value: unknown, label: string): string {
  if (typeof value !== "string" || !value.trim()) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${label} is required`);
  }
  return value.trim();
}

function optionalTrimmedString(value: unknown, label: string): string | null {
  if (value == null) return null;
  if (typeof value !== "string" || !value.trim()) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${label} must be a non-empty string when provided`);
  }
  return value.trim();
}

/**
 * refresh 的 owning notebook 解析：notebookId 给出时直接用（membership 由
 * refreshFileSource 内部校验）；缺失时按 listNotebooks 顺序找第一个引用该源的
 * notebook（file 源的 refresh 是源级新快照，任一 owning notebook 等价）。
 */
function resolveRefreshNotebookId(
  knowledge: KnowledgeManager,
  studioId: string,
  sourceId: string,
  notebookId: string | null,
): string {
  if (notebookId) return notebookId;
  for (const notebook of knowledge.listNotebooks({ studioId })) {
    const hit = knowledge.listNotebookSources({ studioId, notebookId: notebook.id })
      .find(entry => entry.source.id === sourceId);
    if (hit) return notebook.id;
  }
  throw new KnowledgeError(
    "KNOWLEDGE_NOT_FOUND",
    "Knowledge source is not referenced by any notebook in this studio",
  );
}

export function createKnowledgeManageTool(deps: KnowledgeManageToolDeps) {
  return {
    name: "knowledge_manage",
    label: "Knowledge Manage",
    description: "Curate Knowledge notebooks: add a source (file path / pasted text / web url), remove a source from "
      + "a notebook, refresh a file source from its original file, or reindex (re-ingest one source or rebuild a whole "
      + "notebook). This tool MUTATES the knowledge library and always requires approval; use it only when the user "
      + "explicitly asks to change their knowledge notebooks. Not available inside subagents.",
    parameters: Type.Object({
      action: Type.String({
        description: `One of: ${MANAGE_ACTIONS.join(" | ")}.`,
      }),
      notebookId: Type.Optional(Type.String({
        description: "Target notebook. Required for add/remove/reindex; optional for refresh (defaults to the first notebook referencing the source).",
      })),
      sourceId: Type.Optional(Type.String({
        description: "Target source. Required for remove/refresh; optional for reindex (omit to rebuild the whole notebook).",
      })),
      kind: Type.Optional(Type.String({
        description: `Source kind for action=add: ${ADD_KINDS.join(" | ")}.`,
      })),
      path: Type.Optional(Type.String({
        description: "For kind=file: absolute path of the file to import (server-side validated by the Knowledge import boundary).",
      })),
      text: Type.Optional(Type.String({
        description: "For kind=pasted_text: the text content to store as a source.",
      })),
      url: Type.Optional(Type.String({
        description: "For kind=web_snapshot: the URL to fetch and store as a snapshot source.",
      })),
      displayName: Type.Optional(Type.String({
        description: "Optional display name for the new source (action=add only).",
      })),
    }),
    sessionPermission: {
      // 修改性工具：kind "review" = 现有审批档（auto→review、ask→prompt、
      // operate→allow、read_only→拒绝）。kind 枚举只有 read/routine/review，
      // 无独立 "write"/"manage" 档，"review" 即必须审批档（对齐 install_skill）。
      resolveInvocation: (params: any = {}) => {
        const action = MANAGE_ACTIONS.includes(params?.action) ? params.action : "execute";
        return {
          action,
          kind: "review",
          capability: `knowledge_manage.${action}`,
        };
      },
    },
    execute: async (_toolCallId: any, params: Record<string, any> = {}, _signal?: any, _onUpdate?: any, _ctx?: any) => {
      const knowledge = deps.getKnowledge();
      const studioId = deps.getStudioId();
      if (!knowledge || !studioId) {
        return toolError("knowledge_manage unavailable: Knowledge is not accessible in this runtime.", {
          errorCode: "KNOWLEDGE_MODEL_UNAVAILABLE",
        });
      }
      try {
        const action = requireNonEmptyString(params.action, "action");
        if (!(MANAGE_ACTIONS as readonly string[]).includes(action)) {
          throw new KnowledgeError(
            "KNOWLEDGE_INVALID_ARGUMENT",
            `action must be one of: ${MANAGE_ACTIONS.join(", ")}`,
          );
        }

        if (action === "add") {
          const notebookId = requireNonEmptyString(params.notebookId, "notebookId");
          const kind = requireNonEmptyString(params.kind, "kind");
          if (!(ADD_KINDS as readonly string[]).includes(kind)) {
            throw new KnowledgeError(
              "KNOWLEDGE_INVALID_ARGUMENT",
              `kind must be one of: ${ADD_KINDS.join(", ")}`,
            );
          }
          const displayName = optionalTrimmedString(params.displayName, "displayName");
          const provided = ["path", "text", "url"].filter(field => params[field] != null);
          if (provided.length > 1) {
            throw new KnowledgeError(
              "KNOWLEDGE_INVALID_ARGUMENT",
              `pass exactly one of path/text/url for kind=${kind} (received: ${provided.join(", ")})`,
            );
          }
          const imported = kind === "file"
            ? await knowledge.importFile({
              studioId,
              notebookId,
              filePath: requireNonEmptyString(params.path, "path"),
              ...(displayName ? { displayName } : {}),
            })
            : kind === "pasted_text"
              ? await knowledge.importPastedText({
                studioId,
                notebookId,
                text: (() => {
                  if (typeof params.text !== "string" || !params.text.trim()) {
                    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "text is required for kind=pasted_text");
                  }
                  return params.text;
                })(),
                ...(displayName ? { displayName } : {}),
              })
              : await knowledge.importWebSnapshot({
                studioId,
                notebookId,
                url: requireNonEmptyString(params.url, "url"),
                ...(displayName ? { displayName } : {}),
              });
          return toolOk(JSON.stringify({
            action,
            notebookId,
            sourceId: imported.source.id,
            sourceName: imported.source.displayName,
            sourceType: imported.source.sourceType,
            contentSnapshotId: imported.snapshot.id,
            note: "source imported; parsing and indexing run in the background ingestion queue",
          }, null, 2), { action, sourceId: imported.source.id });
        }

        if (action === "remove") {
          const notebookId = requireNonEmptyString(params.notebookId, "notebookId");
          const sourceId = requireNonEmptyString(params.sourceId, "sourceId");
          const membership = knowledge.removeSourceFromNotebook({ studioId, notebookId, sourceId });
          return toolOk(JSON.stringify({
            action,
            notebookId: membership.notebookId,
            sourceId: membership.sourceId,
            note: "membership removed; the source itself is retained (orphan GC applies only after retention)",
          }, null, 2), { action, sourceId });
        }

        if (action === "refresh") {
          const sourceId = requireNonEmptyString(params.sourceId, "sourceId");
          const notebookId = optionalTrimmedString(params.notebookId, "notebookId");
          const resolvedNotebookId = resolveRefreshNotebookId(knowledge, studioId, sourceId, notebookId);
          const refreshed = await knowledge.refreshFileSource({ studioId, notebookId: resolvedNotebookId, sourceId });
          return toolOk(JSON.stringify({
            action,
            sourceId,
            notebookId: resolvedNotebookId,
            changed: refreshed.changed,
            ...(refreshed.changed && refreshed.parseArtifact
              ? { contentSnapshotId: refreshed.snapshot.id, parseArtifactId: refreshed.parseArtifact.id }
              : {}),
            note: refreshed.changed
              ? "file changed on disk; new snapshot stored and re-ingestion enqueued (frozen turn scopes keep reading the previous version this turn)"
              : "file unchanged since the stored snapshot; nothing re-ingested",
          }, null, 2), { action, sourceId });
        }

        // action === "reindex"
        const notebookId = requireNonEmptyString(params.notebookId, "notebookId");
        const sourceId = optionalTrimmedString(params.sourceId, "sourceId");
        if (sourceId) {
          const { job, retried } = knowledge.requeueSourceIngestion({ studioId, notebookId, sourceId });
          return toolOk(JSON.stringify({
            action,
            notebookId,
            sourceId,
            jobId: job.id,
            retried,
            note: retried
              ? "latest failed ingestion job requeued (attempt counter reset, resumes from the failed phase)"
              : "ingestion job enqueued (no prior job existed)",
          }, null, 2), { action, sourceId });
        }
        const jobs = knowledge.enqueueNotebookRebuild({ studioId, notebookId });
        return toolOk(JSON.stringify({
          action,
          notebookId,
          enqueuedJobs: jobs.length,
          note: "notebook rebuild enqueued for all active sources (new chunk/embedding profiles are built alongside existing variants)",
        }, null, 2), { action });
      } catch (error) {
        if (isKnowledgeError(error)) {
          return toolError(`knowledge_manage failed: ${error.code}: ${error.message}`, {
            errorCode: error.code,
          });
        }
        const message = error instanceof Error ? error.message : String(error);
        return toolError(`knowledge_manage failed: ${message}`, {
          errorCode: "KNOWLEDGE_INTERNAL_ERROR",
        });
      }
    },
  };
}
