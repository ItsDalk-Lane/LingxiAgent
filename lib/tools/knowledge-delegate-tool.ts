import { Type } from "../pi-sdk/index.ts";
import { randomUUID } from "node:crypto";
import { isKnowledgeError, KnowledgeError } from "../knowledge/errors.ts";
import {
  requireResearchToolContext,
  notifyResearchProgress,
  type KnowledgeResearchActorContext,
  type KnowledgeResearchToolDeps,
} from "../knowledge/research/research-tool-budget.ts";
import { toolError, toolOk } from "./tool-result.ts";

const WORKER_TOOLS = ["knowledge_outline", "knowledge_search", "knowledge_read", "knowledge_grep", "knowledge_research_update"];

/** 沿用现有隔离执行器参数；研究身份和专用入口由后续会话装配层接线。 */
export interface KnowledgeResearchWorkerOptions {
  agentId: string;
  parentSessionPath: string;
  permissionMode: "read_only";
  approvalPolicy: "deny_on_prompt";
  allowHumanApproval: false;
  subagentContext: true;
  toolFilter: string[];
  builtinFilter: string[];
  surface: "knowledge_research_worker";
  researchContext: KnowledgeResearchActorContext;
  signal: AbortSignal;
}

export interface KnowledgeDelegateToolDeps extends KnowledgeResearchToolDeps {
  /** 由 AgentManager 的可用成员名单提供；不切换当前 Agent。 */
  listAgents: () => Array<{ id: string; status?: string; archived?: boolean }>;
  executeIsolated: (prompt: string, options: KnowledgeResearchWorkerOptions) => Promise<unknown>;
}

interface DelegatedTask {
  label: string;
  needIds: string[];
  task: string;
  agentId: string;
}

interface DelegatedTaskResult {
  label: string;
  needIds: string[];
  agentId: string;
  status: "completed" | "failed" | "cancelled";
  errorCode?: string;
}

function invalid(): never {
  throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Research delegation requires one to four tasks with current-run need IDs and active agents");
}

function taskText(value: unknown): string {
  if (typeof value !== "string" || !value.trim()) invalid();
  return value.trim();
}

function workerStatus(result: unknown, signal: AbortSignal, stopReason?: string | null): Pick<DelegatedTaskResult, "status" | "errorCode"> {
  // 预算停止与用户取消分别报告，不能把已取得部分材料的超限任务说成用户取消。
  if (stopReason && ["tool_budget_exhausted", "wall_clock_exhausted", "round_budget_exhausted"].includes(stopReason)) {
    return { status: "failed", errorCode: stopReason };
  }
  if (signal.aborted) return { status: "cancelled", errorCode: "KNOWLEDGE_RESEARCH_CANCELLED" };
  if (!result || typeof result !== "object") return { status: "failed", errorCode: "KNOWLEDGE_RESEARCH_WORKER_FAILED" };
  const outcome = result as { error?: unknown; stopReason?: unknown };
  if (outcome.error === "aborted" || outcome.stopReason === "aborted") {
    return { status: "cancelled", errorCode: "KNOWLEDGE_RESEARCH_CANCELLED" };
  }
  if (outcome.error || (outcome.stopReason != null && outcome.stopReason !== "stop")) {
    return { status: "failed", errorCode: "KNOWLEDGE_RESEARCH_WORKER_FAILED" };
  }
  return { status: "completed" };
}

export function createKnowledgeDelegateTool(deps: KnowledgeDelegateToolDeps) {
  return {
    name: "knowledge_delegate",
    label: "Knowledge Delegate",
    description: "将最多四项证据任务委派给隔离的只读研究会话，每项必须绑定本轮已有的需求编号。等待所有任务结束并返回状态，证据写入共享账本。"
      + "默认省略 agentId，所有任务即可使用当前助手并行调查；agentId 只能指定已存在的助手编号，不能填写新工作会话名称，任务名称应放在 label。",
    parameters: Type.Object({
      runId: Type.String(),
      tasks: Type.Array(Type.Object({
        label: Type.String(),
        needIds: Type.Array(Type.String()),
        task: Type.String(),
        agentId: Type.Optional(Type.String({ description: "可省略，默认当前助手。仅在明确选择其他已有助手时填写其真实编号，不得编造工作会话编号。" })),
      })),
    }),
    sessionPermission: { resolveInvocation: () => ({ action: "read", kind: "read", capability: "knowledge_delegate.read" }) },
    execute: async (_toolCallId: string, params: Record<string, unknown> = {}, signal?: AbortSignal, _onUpdate?: unknown, ctx?: unknown) => {
      try {
        const context = requireResearchToolContext(deps, ctx, params.runId);
        if (context.role !== "root") {
          throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Research workers cannot delegate");
        }
        const result = await deps.budget.execute({ context, toolName: "knowledge_delegate", requestSummary: {}, signal }, async workerSignal => {
          if (!params || typeof params !== "object" || Array.isArray(params)
            || (Object.getPrototypeOf(params) !== Object.prototype && Object.getPrototypeOf(params) !== null)
            || Object.keys(params).some(key => !["runId", "tasks"].includes(key))) invalid();
          if (!Array.isArray(params.tasks) || params.tasks.length === 0 || params.tasks.length > 4) invalid();
          const run = deps.research.requireRun(context.runId);
          const agents = deps.listAgents();
          // 真实成员名单不带 status；已在管理器排除删除项。有显式状态的列表仍须拒绝停用项。
          const activeAgentIds = new Set(agents.filter(agent => !agent.archived
            && (agent.status === undefined || agent.status === "active")).map(agent => agent.id));
          const tasks: DelegatedTask[] = params.tasks.map(value => {
            if (!value || typeof value !== "object" || Array.isArray(value)) invalid();
            const task = value as Record<string, unknown>;
            if (Object.keys(task).some(key => !["label", "needIds", "task", "agentId"].includes(key))) invalid();
            const label = taskText(task.label), instructions = taskText(task.task);
            if (!Array.isArray(task.needIds) || task.needIds.length === 0) invalid();
            const needIds = [...new Set(task.needIds.map(taskText))];
            for (const needId of needIds) {
              deps.research.getNeed(run.id, needId);
              if (context.allowedNeedIds !== undefined && !context.allowedNeedIds.includes(needId)) {
                throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Delegated need is outside the root assignment");
              }
            }
            const agentId = task.agentId === undefined || (typeof task.agentId === "string" && !task.agentId.trim())
              ? context.actorAgentId : taskText(task.agentId);
            if (!activeAgentIds.has(agentId)) {
              throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "agentId 必须是已存在且可用的助手编号，不能使用新工作会话名称。请省略 agentId 以使用当前助手，并将任务名称填写在 label 中。");
            }
            return { label, needIds, task: instructions, agentId };
          });
          workerSignal.throwIfAborted();
          const statuses = await deps.budget.withWorkerSlots(run.id, tasks.length, async () => {
            // 先完成整批验证再并行启动；某个工作会话失败也要等待其它会话清理结束。
            return Promise.all(tasks.map(async task => {
              const identity = { label: task.label, needIds: task.needIds, agentId: task.agentId };
              const progress = { taskId: randomUUID(), label: task.label.replace(/[\r\n\t]/g, " ").slice(0, 100) };
              let status: DelegatedTaskResult["status"] = "failed";
              let stopReason: string | null = null;
              let started = false;
              try {
                workerSignal.throwIfAborted();
                started = true;
                notifyResearchProgress(deps.onProgress, { type: "knowledge_research_worker_started", ...progress });
                const needs = task.needIds.map(needId => deps.ledger.evaluateNeed(run.id, needId));
                const prompt = "你是知识研究工作会话。只能读取分配范围的知识资料，并通过知识研究更新工具登记已核验的证据。"
                  + "不得修改资料、访问外部网络、再次委派任务或调用研究完成工具。检索摘要不是证据，必须先读取原文取得凭据。\n"
                  + "根会话和所有工作会话共用同一份次数与截止时间，remainingBudget 是即时快照，不是你的个人额度。"
                  + "优先用命中的 sectionId 阅读完整父章节，同章命中合并阅读；没有章节定位时使用 aroundChunkId。"
                  + "每批读取或原文匹配返回凭据后，下一步先调用 knowledge_research_update，立即登记相关的准确引文，再继续采集。"
                  + "引文必须逐字位于同一凭据的 text 中，不得自行补齐被截断的字句；跨原始段落时分成多条登记。"
                  + "材料全部无关时用 unresolvedGaps 说明真实缺口，没有新增缺口可提交空更新。"
                  + "部分接受时已通过条目已经保存，只纠正被拒项；更新失败先纠错重试，不得绕过错误继续搜索。余量不足时优先登记已有材料。\n"
                  + JSON.stringify({ question: run.question, scopeId: context.scopeId, runId: run.id,
                    label: task.label, task: task.task, needIds: task.needIds, needs,
                    remainingBudget: deps.budget.remainingBudget(run.id),
                    ...(context.allowedSourceIds !== undefined ? { allowedSourceIds: context.allowedSourceIds } : {}),
                  });
                const outcome = await deps.executeIsolated(prompt, {
                  agentId: task.agentId, parentSessionPath: run.parentSessionPath,
                  permissionMode: "read_only", approvalPolicy: "deny_on_prompt", allowHumanApproval: false,
                  subagentContext: true, toolFilter: [...WORKER_TOOLS], builtinFilter: [],
                  surface: "knowledge_research_worker",
                  // 工作会话的实际会话编号须由隔离装配层生成，不能冒用父会话编号。
                  researchContext: { ...context, actorSessionId: null, actorAgentId: task.agentId, role: "worker", allowedNeedIds: [...task.needIds],
                    ...(context.allowedSourceIds !== undefined ? { allowedSourceIds: [...context.allowedSourceIds] } : {}) },
                  signal: workerSignal,
                });
                stopReason = deps.research.requireRun(run.id).stopReason;
                const completed = workerStatus(outcome, workerSignal, stopReason);
                status = completed.status;
                stopReason ??= completed.errorCode ?? null;
                return { ...identity, ...completed };
              } catch {
                stopReason = deps.research.requireRun(run.id).stopReason;
                const interrupted = workerStatus(null, workerSignal, stopReason);
                status = interrupted.status;
                stopReason ??= interrupted.errorCode ?? null;
                return { ...identity, ...interrupted } as DelegatedTaskResult;
              } finally {
                // 隔离执行器返回前已等待工作会话清理，完成事件不提前报到。
                if (started) notifyResearchProgress(deps.onProgress, { type: "knowledge_research_worker_completed", ...progress, status, stopReason });
              }
            }));
          });
          const completed = statuses.filter(task => task.status === "completed").length;
          const status = statuses.every(task => task.status === "cancelled") ? "cancelled"
            : completed === statuses.length ? "completed" : completed > 0 ? "partial" : "failed";
          return { value: { runId: run.id, status, tasks: statuses }, summary: { count: statuses.length, status } };
        });
        return toolOk(JSON.stringify({ ...result, remainingBudget: deps.budget.remainingBudget(context.runId) }));
      } catch (error) {
        const errorCode = isKnowledgeError(error) ? error.code
          : signal?.aborted ? "KNOWLEDGE_RESEARCH_CANCELLED" : "KNOWLEDGE_INTERNAL_ERROR";
        return toolError(isKnowledgeError(error)
          && ["KNOWLEDGE_INVALID_ARGUMENT", "KNOWLEDGE_SCOPE_VIOLATION", "KNOWLEDGE_CONFLICT"].includes(error.code)
          ? error.message : "Knowledge research delegation was rejected or interrupted.", { errorCode });
      }
    },
  };
}
