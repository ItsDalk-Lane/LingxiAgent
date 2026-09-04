import { KnowledgeError, isKnowledgeError } from "../errors.ts";
import { getKnowledgeResearchToolNames } from "../../../shared/tool-categories.ts";
import type { ResearchStore } from "./research-store.ts";
import { ResearchToolBudget } from "./research-tool-budget.ts";

export type ResearchExecuteIsolated = (prompt: string, options: Record<string, unknown>) => Promise<unknown>;

export interface ResearchRoundInput {
  runId: string;
  roundId: string;
  agentId: string;
  parentSessionId: string;
  parentSessionPath: string;
  studioId: string;
  scopeId: string;
  prompt: string;
  signal?: AbortSignal;
  searchPlan?: Array<{ query: string; needIds: string[]; purpose?: "counterexample" }>;
  forbiddenQueries?: string[];
  isCompletenessSatisfied?: (runId: string) => boolean;
}

/** 每轮只启动新的研究根会话，消费宿主结构化完成信号，不把会话回复或隐藏思考带入下一轮。 */
export class ResearchRoundRunner {
  private readonly budget: ResearchToolBudget;

  constructor(private readonly deps: { research: ResearchStore; executeIsolated: ResearchExecuteIsolated;
    budget?: ResearchToolBudget; nowMs?: () => number }) {
    this.budget = deps.budget ?? new ResearchToolBudget(deps.research, { nowMs: deps.nowMs });
  }

  async run(input: ResearchRoundInput): Promise<{ finishAccepted: boolean; errorCode: string | null }> {
    let finishAccepted = false;
    try {
      const run = this.deps.research.requireRun(input.runId);
      const scope = this.deps.research.knowledgeStore.getTurnScope({ scopeId: input.scopeId });
      const round = this.deps.research.listRounds(run.id).find(round => round.id === input.roundId);
      if (!scope || scope.status !== "active" || scope.id !== run.turnScopeId || scope.studioId !== input.studioId
        || input.parentSessionPath !== run.parentSessionPath || !input.parentSessionId || !input.agentId
        || !round || round.status !== "running") {
        throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Research round identity does not match its frozen scope");
      }
      const result = await this.budget.withRunController(run.id, input.signal, signal => this.deps.executeIsolated(input.prompt, {
        agentId: input.agentId, parentSessionId: input.parentSessionId, parentSessionPath: input.parentSessionPath,
        surface: "knowledge_research_root", permissionMode: "read_only", approvalPolicy: "deny_on_prompt",
        allowHumanApproval: false, subagentContext: true, memoryEnabled: false, forceMemoryEnabled: false,
        workspaceFolders: [], authorizedFolders: [], fileReadSessionPaths: [], persist: false,
        toolFilter: [...getKnowledgeResearchToolNames("knowledge_research_root")], builtinFilter: [], extraCustomTools: [], signal,
        research: { runId: run.id, scopeId: scope.id, studioId: scope.studioId,
          ...(input.searchPlan ? { searchPlan: input.searchPlan } : {}),
          ...(input.forbiddenQueries ? { forbiddenQueries: input.forbiddenQueries } : {}),
          ...(input.isCompletenessSatisfied ? { isCompletenessSatisfied: input.isCompletenessSatisfied } : {}),
          onFinishAccepted: (decision: unknown) => {
            if (decision && typeof decision === "object" && "runId" in decision && decision.runId === run.id
              && "accepted" in decision && decision.accepted === true) finishAccepted = true;
          },
        },
      }));
      if (!result || typeof result !== "object" || Array.isArray(result)) {
        return { finishAccepted: false, errorCode: "KNOWLEDGE_RESEARCH_EXECUTION_FAILED" };
      }
      const outcome = result as { error?: unknown; stopReason?: unknown };
      if (outcome.error || (outcome.stopReason != null && outcome.stopReason !== "stop")) {
        return { finishAccepted: false, errorCode: "KNOWLEDGE_RESEARCH_EXECUTION_FAILED" };
      }
      return { finishAccepted, errorCode: null };
    } catch (error) {
      const reason = isKnowledgeError(error) ? error.details.stopReason : undefined;
      if (reason === "wall_clock_exhausted" || reason === "tool_budget_exhausted") return { finishAccepted: false, errorCode: reason };
      if (input.signal?.aborted || reason === "cancelled") return { finishAccepted: false, errorCode: "KNOWLEDGE_RESEARCH_CANCELLED" };
      return { finishAccepted: false, errorCode: isKnowledgeError(error) ? error.code : "KNOWLEDGE_RESEARCH_EXECUTION_FAILED" };
    }
  }
}
