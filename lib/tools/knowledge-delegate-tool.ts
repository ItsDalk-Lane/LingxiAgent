import { Type } from "../pi-sdk/index.ts";
import { isKnowledgeError, KnowledgeError } from "../knowledge/errors.ts";
import {
  requireResearchToolContext,
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

function workerStatus(result: unknown, signal: AbortSignal): Pick<DelegatedTaskResult, "status" | "errorCode"> {
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
    description: "Delegate up to four evidence tasks to isolated read-only research workers. Each task must bind current-run need IDs. Waits for every worker and returns task statuses; evidence is recorded through the shared ledger.",
    parameters: Type.Object({
      runId: Type.String(),
      tasks: Type.Array(Type.Object({
        label: Type.String(),
        needIds: Type.Array(Type.String()),
        task: Type.String(),
        agentId: Type.Optional(Type.String()),
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
            const agentId = task.agentId === undefined ? context.actorAgentId : taskText(task.agentId);
            if (!activeAgentIds.has(agentId)) invalid();
            return { label, needIds, task: instructions, agentId };
          });
          workerSignal.throwIfAborted();
          const statuses = await deps.budget.withWorkerSlots(run.id, tasks.length, async () => {
            // 先完成整批验证再并行启动；某个工作会话失败也要等待其它会话清理结束。
            return Promise.all(tasks.map(async task => {
              const identity = { label: task.label, needIds: task.needIds, agentId: task.agentId };
              try {
                workerSignal.throwIfAborted();
                const needs = task.needIds.map(needId => deps.ledger.evaluateNeed(run.id, needId));
                const prompt = "你是知识研究工作会话。只能读取分配范围的知识资料，并通过知识研究更新工具登记已核验的证据。"
                  + "不得修改资料、访问外部网络、再次委派任务或调用研究完成工具。检索摘要不是证据，必须先读取原文取得凭据。\n"
                  + JSON.stringify({ question: run.question, scopeId: context.scopeId, runId: run.id,
                    label: task.label, task: task.task, needIds: task.needIds, needs,
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
                return { ...identity, ...workerStatus(outcome, workerSignal) };
              } catch {
                return { ...identity, status: workerSignal.aborted ? "cancelled" : "failed",
                  errorCode: workerSignal.aborted ? "KNOWLEDGE_RESEARCH_CANCELLED" : "KNOWLEDGE_RESEARCH_WORKER_FAILED" } as DelegatedTaskResult;
              }
            }));
          });
          const completed = statuses.filter(task => task.status === "completed").length;
          const status = statuses.every(task => task.status === "cancelled") ? "cancelled"
            : completed === statuses.length ? "completed" : completed > 0 ? "partial" : "failed";
          return { value: { runId: run.id, status, tasks: statuses }, summary: { count: statuses.length, status } };
        });
        return toolOk(JSON.stringify(result));
      } catch (error) {
        const errorCode = isKnowledgeError(error) ? error.code
          : signal?.aborted ? "KNOWLEDGE_RESEARCH_CANCELLED" : "KNOWLEDGE_INTERNAL_ERROR";
        return toolError("Knowledge research delegation was rejected or interrupted.", { errorCode });
      }
    },
  };
}
