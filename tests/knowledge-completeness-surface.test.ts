import fs from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";
import { KnowledgeResearchOrchestrator } from "../lib/knowledge/research/knowledge-research-orchestrator.ts";
import { wrapWithSessionPermission } from "../lib/tools/session-permission-wrapper.ts";
import { getKnowledgeResearchToolNames, isKnowledgeResearchSurface } from "../shared/tool-categories.ts";
import { createResearchAgentFixture, recordSourceEvidence, researchNeed, requestFinish,
  type ResearchModelTurn } from "./helpers/knowledge-research-agent-fixture.ts";

const fixtures: Awaited<ReturnType<typeof createResearchAgentFixture>>[] = [];
afterEach(async () => { for (const fixture of fixtures.splice(0)) await fixture.close(); });

describe("完整性真实Agent工具身份与生命周期", () => {
  it("第三入口只有两个冻结工具名，准确识别且不能通过数组修改扩大权限", () => {
    expect(isKnowledgeResearchSurface("knowledge_completeness_worker")).toBe(true);
    for (const value of ["knowledge_completeness_worker ", "KNOWLEDGE_COMPLETENESS_WORKER", "knowledge_completeness", ["knowledge_completeness_worker"]]) {
      expect(isKnowledgeResearchSurface(value)).toBe(false);
    }
    const names = getKnowledgeResearchToolNames("knowledge_completeness_worker");
    expect(names).toEqual(["knowledge_coverage_read", "knowledge_completeness_mark"]);
    expect(Object.isFrozen(names)).toBe(true);
    expect(() => (names as string[]).push("knowledge_delegate")).toThrow();
  });

  it.each([{ permissionMode: "operate", approvalPolicy: "deny_on_prompt" },
    { permissionMode: "read_only", approvalPolicy: "interactive" }])("完整性权限配置为 $permissionMode/$approvalPolicy 时，工具自报只读也不能放行", async config => {
    const tool = { name: "knowledge_coverage_read", execute: vi.fn(async () => ({ content: [{ type: "text", text: "原文" }] })),
      sessionPermission: { resolveInvocation: vi.fn(() => ({ action: "read", kind: "read", capability: "knowledge_coverage_read.read" })) } };
    const review = vi.fn(async () => ({ action: "allow" }));
    const [wrapped] = wrapWithSessionPermission([tool], { getPermissionMode: () => config.permissionMode,
      approvalPolicy: config.approvalPolicy, permissionContext: { knowledgeResearchSurface: "knowledge_completeness_worker" },
      approvalGateway: { review } });
    const result = await wrapped.execute("invalid-completeness-permission", {}, undefined, undefined,
      { sessionManager: { getSessionFile: () => "/tmp/completeness-permission.jsonl" } });
    expect(result.details.errorCode).toBe("KNOWLEDGE_RESEARCH_PERMISSION_INVALID");
    expect(tool.sessionPermission.resolveInvocation).not.toHaveBeenCalled();
    expect(tool.execute).not.toHaveBeenCalled();
    expect(review).not.toHaveBeenCalled();
  });

  it("真实Root完成申请启动专用Worker，只能读和标注；清理后旧工具闭包失效", async () => {
    let worker: ResearchModelTurn | undefined, workerError: unknown;
    const f = await createResearchAgentFixture(async turn => {
      if (turn.options.surface === "knowledge_completeness_worker") {
        worker = turn;
        try {
          for (const name of [...getKnowledgeResearchToolNames("knowledge_research_root"), "read", "write", "exec_command", "web_search", "subagent"]) {
            await expect(turn.call(name, {})).rejects.toThrow(/unavailable tool/);
          }
          const assignment = turn.options.research;
          const read = await turn.call("knowledge_coverage_read", { runId: turn.runId, checkId: assignment.completenessCheckId,
            shardId: assignment.completenessShardId });
          expect(read.isError).toBeUndefined();
          expect(read.units).toHaveLength(3);
          expect(read.text).toContain(f.sources[0].text);
          const mark = await turn.call("knowledge_completeness_mark", { checkId: assignment.completenessCheckId,
            results: read.units.map((unit: { unitId: string; receiptId: string }) => ({ unitId: unit.unitId, receiptId: unit.receiptId, status: "irrelevant" })) });
          expect(mark).toMatchObject({ status: "running", checkedUnits: 3, exact: false });
        } catch (error) { workerError = error; throw error; }
        return;
      }
      await turn.call("knowledge_outline", { scopeId: turn.scopeId });
      const update = await turn.call("knowledge_research_update", { runId: turn.runId,
        createNeeds: [researchNeed("确定交付日期", { requireAllRelevantUnits: true })] });
      await recordSourceEvidence(turn, update.needs[0].id, f.sources[0].sourceId, "九月十五日");
      expect((await requestFinish(turn)).accepted).toBe(true);
    }); fixtures.push(f);
    const result = await new KnowledgeResearchOrchestrator({ research: f.research, executeIsolated: f.executeIsolated })
      .run({ ...f.request, policy: { ...f.request.policy, completenessPolicy: "scope_complete" } });
    if (workerError) throw workerError;
    expect(result.run.status).toBe("completed");
    expect(worker).toBeDefined();
    expect(f.calls.map(turn => turn.options.surface)).toEqual(["knowledge_research_root", "knowledge_completeness_worker"]);
    expect(f.sessionPaths.every(sessionPath => !fs.existsSync(sessionPath))).toBe(true);
    const used = f.research.requireRun(result.run.id).toolCallsUsed, assignment = worker!.options.research;
    expect(await worker!.call("knowledge_coverage_read", { runId: result.run.id, checkId: assignment.completenessCheckId,
      shardId: assignment.completenessShardId })).toMatchObject({ isError: true, errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
    expect(await worker!.call("knowledge_completeness_mark", { checkId: assignment.completenessCheckId,
      results: [{ unitId: "former-unit", status: "unavailable" }] })).toMatchObject({ isError: true, errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
    expect(f.research.requireRun(result.run.id).toolCallsUsed).toBe(used);
  });

  it("跨run/check/shard参数和登记库分配漂移均使已装配工具拒绝，恢复正确身份后仍可真实核查", async () => {
    let workerError: unknown;
    const f = await createResearchAgentFixture(async turn => {
      if (turn.options.surface === "knowledge_completeness_worker") {
        try {
          const assignment = turn.options.research, args = { runId: turn.runId, checkId: assignment.completenessCheckId,
            shardId: assignment.completenessShardId };
          const used = f.research.requireRun(turn.runId).toolCallsUsed;
          for (const key of ["runId", "checkId", "shardId"]) expect(await turn.call("knowledge_coverage_read", { ...args, [key]: "another-id" }))
            .toMatchObject({ isError: true, errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
          const sessionPath = f.sessionPaths.at(-1)!, manifest = f.manifests.resolveByLocatorPath(sessionPath)!;
          for (const key of ["completenessCheckId", "completenessShardId"]) {
            try {
              f.manifests.db.prepare("UPDATE session_manifests SET provenance_json=? WHERE session_id=?")
                .run(JSON.stringify({ ...manifest.provenance, researchContext: { ...manifest.provenance.researchContext, [key]: "changed-id" } }), manifest.sessionId);
              expect(await turn.call("knowledge_coverage_read", args)).toMatchObject({ isError: true, errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
              expect(await turn.call("knowledge_completeness_mark", { checkId: args.checkId,
                results: [{ unitId: "unit-a", status: "unavailable" }] })).toMatchObject({ isError: true, errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
            } finally {
              f.manifests.db.prepare("UPDATE session_manifests SET provenance_json=? WHERE session_id=?")
                .run(JSON.stringify(manifest.provenance), manifest.sessionId);
            }
          }
          expect(f.research.requireRun(turn.runId).toolCallsUsed).toBe(used);
          const read = await turn.call("knowledge_coverage_read", args);
          expect(read.isError).toBeUndefined();
          const mark = await turn.call("knowledge_completeness_mark", { checkId: args.checkId,
            results: read.units.map((unit: { unitId: string; receiptId: string }) => ({ unitId: unit.unitId, receiptId: unit.receiptId, status: "irrelevant" })) });
          expect(mark).toMatchObject({ checkedUnits: 3, exact: false });
        } catch (error) { workerError = error; throw error; }
        return;
      }
      await turn.call("knowledge_outline", { scopeId: turn.scopeId });
      const update = await turn.call("knowledge_research_update", { runId: turn.runId,
        createNeeds: [researchNeed("日期", { requireAllRelevantUnits: true })] });
      await recordSourceEvidence(turn, update.needs[0].id, f.sources[0].sourceId, "九月十五日");
      expect((await requestFinish(turn)).accepted).toBe(true);
    }); fixtures.push(f);
    const result = await new KnowledgeResearchOrchestrator({ research: f.research, executeIsolated: f.executeIsolated })
      .run({ ...f.request, policy: { ...f.request.policy, completenessPolicy: "scope_complete" } });
    if (workerError) throw workerError;
    expect(result.run.status).toBe("completed");
    expect(f.research.listActions(result.run.id).every(action => action.status === "completed")).toBe(true);
  });
});
