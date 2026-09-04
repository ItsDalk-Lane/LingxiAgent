import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { Agent } from "../../core/agent.ts";
import { SessionManifestStore } from "../../core/session-manifest/store.ts";
import { KnowledgeManager } from "../../lib/knowledge/knowledge-manager.ts";
import { ResearchStore } from "../../lib/knowledge/research/research-store.ts";
import { resolveKnowledgeChunkerConfig } from "../../lib/knowledge/chunker.ts";
import { resolveKnowledgeExecutionPolicy } from "../../shared/knowledge-execution.ts";
import type { KnowledgeResearchRequest } from "../../lib/knowledge/research/knowledge-research-orchestrator.ts";

export interface ResearchModelTurn {
  role: "root" | "worker";
  prompt: string;
  options: Record<string, any>;
  call: (name: string, params: Record<string, unknown>) => Promise<any>;
  runId: string;
  scopeId: string;
}

/** 仅替换模型执行边界；真实摄入、索引、冻结范围、会话清单、Agent工具、委派和台账全部贯通。 */
export async function createResearchAgentFixture(driver: (turn: ResearchModelTurn) => Promise<unknown>, question = "项目进度和预算是什么？") {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-round-agent-"));
  const manager = new KnowledgeManager({ lingxiHome: root });
  const manifests = new SessionManifestStore({ dbPath: path.join(root, "manifests.db") });
  const studioId = "research-round-studio";
  const notebook = manager.createNotebook({ studioId, name: "项目原文" });
  const sources = [];
  for (const [name, text] of [["进度", "苹果项目交付日期是九月十五日。"], ["预算", "苹果项目预算是三十二万元。"], ["变更", "由于审批延迟，苹果项目交付改为九月二十日。"]]) {
    const imported = await manager.importPastedText({ studioId, notebookId: notebook.id, displayName: name, text });
    const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
    const blocks = manager.store.listArtifactBlocks({ studioId, parseArtifactId: artifact.id });
    const targetChars = manager.getNotebookEffectiveChunkTargetChars({ studioId, notebookId: notebook.id });
    manager.store.resolveNotebookRetrievalProfile({ studioId, notebookId: notebook.id,
      strategy: resolveKnowledgeChunkerConfig(blocks, { targetChars }).strategy });
    manager.queryService.indexArtifactForIngestion(studioId, artifact.id, { targetChars });
    sources.push({ sourceId: imported.source.id, text });
  }
  const mainPath = path.join(root, "main.jsonl"); fs.writeFileSync(mainPath, "");
  const main = manifests.createForPath({ sessionPath: mainPath, ownerAgentId: "agent-a", domain: "desktop", kind: "chat", provenance: { studioId } });
  const scope = manager.createTurnScope({ studioId, sessionPath: mainPath, notebookIds: [notebook.id] });
  const compiledScope = await manager.compileTurnScope(scope);
  const research = new ResearchStore(manager.store);
  const calls: ResearchModelTurn[] = [];
  const sessionPaths: string[] = [];
  const executeIsolated = async (prompt: string, options: Record<string, any>) => {
    const role = options.surface === "knowledge_research_root" ? "root" : "worker";
    const sessionPath = path.join(root, `${role}-${sessionPaths.length}.jsonl`);
    fs.writeFileSync(sessionPath, ""); sessionPaths.push(sessionPath);
    const actor = { runId: options.research.runId, scopeId: options.research.scopeId, role,
      ...(options.research.allowedNeedIds ? { allowedNeedIds: options.research.allowedNeedIds } : {}),
      ...(options.research.allowedSourceIds ? { allowedSourceIds: options.research.allowedSourceIds } : {}) };
    if (options.surface === "knowledge_completeness_worker") Object.assign(actor, {
      completenessCheckId: options.research.completenessCheckId, completenessShardId: options.research.completenessShardId,
    });
    const manifest = manifests.createForPath({ sessionPath, ownerAgentId: options.agentId, domain: "subagent", kind: options.surface,
      provenance: { studioId, parentSessionId: options.parentSessionId, researchContext: actor } });
    try {
      const tools = agent.getToolsSnapshot({ surface: options.surface,
        research: { ...options.research, sessionPath, actorContext: { ...actor, actorSessionId: manifest.sessionId, actorAgentId: options.agentId } } });
      const turn: ResearchModelTurn = { role, prompt, options, runId: actor.runId, scopeId: actor.scopeId,
        call: async (name, params) => {
          const tool = tools.find(tool => tool.name === name);
          if (!tool) throw new Error(`Test model requested unavailable tool: ${name}`);
          const result = await tool.execute(`call-${calls.length}`, params, options.signal, undefined,
            { sessionManager: { getSessionFile: () => sessionPath } });
          if (result.isError) return { isError: true, ...result.details };
          return name === "knowledge_coverage_read" ? { ...result.details, text: result.content[0].text }
            : JSON.parse(result.content[0].text);
        },
      };
      calls.push(turn);
      return await driver(turn) ?? { stopReason: "stop", replyText: "私有模型推理不得传给下轮或最终回答" };
    } finally {
      fs.unlinkSync(sessionPath);
      manifests.updateLocatorLifecycle(manifest.sessionId, sessionPath, "deleted", "research_test_complete");
    }
  };
  const engine = { knowledge: manager, runtimeContext: { studioId },
    getSessionIdForPath: (sessionPath: string) => manifests.resolveByLocatorPath(sessionPath)?.sessionId ?? null,
    getSessionManifest: (sessionId: string) => manifests.getBySessionId(sessionId) };
  const agent = Object.assign(Object.create(Agent.prototype) as Agent, {
    _cb: { getEngine: () => engine, listActiveAgents: () => [{ id: "agent-a" }, { id: "agent-b" }], executeIsolated },
  });
  const request: KnowledgeResearchRequest = { question, compiledScope,
    policy: resolveKnowledgeExecutionPolicy({ mode: "detailed", question, selectedNotebookCount: 1, selectedSourceCount: sources.length }),
    parentSessionId: main.sessionId, parentSessionPath: mainPath, agentId: "agent-a", turnId: scope.turnId };
  return { manager, research, sources, calls, sessionPaths, manifests, executeIsolated, request,
    async close() { await manager.close(); manifests.close(); fs.rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 }); },
  };
}

export const researchNeed = (claim: string, overrides: Record<string, unknown> = {}) => ({ claim, kind: "fact", required: true,
  minIndependentSources: 1, requireCounterEvidence: false, requireAllRelevantUnits: false, ...overrides });

export async function recordSourceEvidence(turn: ResearchModelTurn, needId: string, sourceId: string, quote: string,
  relation: "supports" | "contradicts" = "supports") {
  const read = await turn.call("knowledge_read", { scopeId: turn.scopeId, sourceId });
  const span = read.chunks[0].spans.find((span: { text: string }) => span.text.includes(quote));
  return turn.call("knowledge_research_update", { runId: turn.runId,
    linkEvidence: [{ needId, receiptId: span.receiptId, quote, relation, rationale: "冻结原文明示" }] });
}

export const requestFinish = (turn: ResearchModelTurn) => turn.call("knowledge_research_finish", {
  runId: turn.runId, conclusionSummary: "私有总结不得转交", requestedStopReason: "complete",
});
