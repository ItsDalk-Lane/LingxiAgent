import { afterEach, describe, expect, it } from "vitest";
import fs from "node:fs";
import { KnowledgeResearchOrchestrator } from "../lib/knowledge/research/knowledge-research-orchestrator.ts";
import { createResearchAgentFixture, recordSourceEvidence, researchNeed, requestFinish } from "./helpers/knowledge-research-agent-fixture.ts";

const fixtures: Awaited<ReturnType<typeof createResearchAgentFixture>>[] = [];
afterEach(async () => { for (const fixture of fixtures.splice(0)) await fixture.close(); });

async function setup(omitLast = false) {
  let needId: string | undefined;
  const failures: unknown[] = [], finishes: boolean[] = [];
  const f = await createResearchAgentFixture(async turn => {
    try {
      if (turn.options.surface === "knowledge_completeness_worker") {
        const { completenessCheckId: checkId, completenessShardId: shardId } = turn.options.research;
        const read = await turn.call("knowledge_coverage_read", { runId: turn.runId, checkId, shardId });
        expect(read.isError).toBeUndefined();
        const available = read.units.filter((unit: { status: string }) => unit.status === "available");
        const selected = omitLast ? available.slice(0, -1) : available;
        if (selected.length) {
          const marked = await turn.call("knowledge_completeness_mark", { checkId,
            results: selected.map((unit: { unitId: string; receiptId: string }) => ({
              unitId: unit.unitId, receiptId: unit.receiptId, status: "irrelevant",
            })),
          });
          expect(marked.isError).toBeUndefined();
          // 最后一条登记完成时，会话仍未退出，不允许提前声称完整。
          expect(marked.exact).toBe(false);
        }
        return;
      }
      if (!needId) {
        await turn.call("knowledge_outline", { scopeId: turn.scopeId });
        const created = await turn.call("knowledge_research_update", { runId: turn.runId,
          createNeeds: [researchNeed("核实项目原始交付日期", { requireAllRelevantUnits: true })] });
        needId = created.needs[0].id;
        await recordSourceEvidence(turn, needId!, f.sources[0].sourceId, "九月十五日");
      }
      finishes.push((await requestFinish(turn)).accepted === true);
    } catch (error) { failures.push(error); throw error; }
  }, "检查全文中的项目交付记录。");
  fixtures.push(f);
  return { ...f, failures, finishes,
    run: () => new KnowledgeResearchOrchestrator({ research: f.research, executeIsolated: f.executeIsolated }).run(f.request) };
}

describe("完整性结果约束最终否定措辞", () => {
  it("真实完整性工作会话逐单元读完并退出后，才允许条件式完整范围否定措辞", async () => {
    const f = await setup();
    const result = await f.run();
    expect(f.failures).toEqual([]);
    expect(f.finishes).toEqual([true]);
    expect(result.run.status).toBe("completed");
    expect(result.packet.completeness).toMatchObject({ exact: true, totalUnits: 3, checkedUnits: 3,
      unavailableUnits: 0, failedUnits: 0, coverageRatio: 1 });
    expect(result.block).toContain("exact=true");
    expect(result.block).toContain("只有原文证据也支持否定结论时");
    expect(result.block).toContain("在所选完整范围中不存在");
    expect(result.block).not.toContain("完整性尚未证明，只能说");
    expect(f.calls.filter(turn => turn.options.surface === "knowledge_completeness_worker")).toHaveLength(1);
    const actions = f.research.listActions(result.run.id);
    expect(actions.filter(action => action.actionType === "knowledge_coverage_read")).toHaveLength(1);
    expect(result.run.readCalls).toBe(2);
    expect(JSON.stringify(actions)).not.toContain("私有模型推理不得传给下轮");
    for (const sessionPath of f.sessionPaths) {
      expect(f.manifests.resolveByLocatorPath(sessionPath)?.lifecycle).toBe("deleted");
      expect(fs.existsSync(sessionPath)).toBe(false);
    }
  });

  it("遗漏一条原文单元不能获得完整证明，部分结果必须限制到已检查范围", async () => {
    const f = await setup(true);
    const result = await f.run();
    expect(f.failures).toEqual([]);
    expect(f.finishes.every(accepted => !accepted)).toBe(true);
    expect(result.run.status).toBe("partial");
    expect(result.packet.completeness).toMatchObject({ exact: false, totalUnits: 3, checkedUnits: 2, failedUnits: 1 });
    expect(result.block).toContain("exact=false");
    expect(result.block).toContain("在已检查的范围内未发现");
    expect(result.block).toContain("无法证明完整不存在");
    expect(result.block).not.toContain("才允许说“在所选完整范围中不存在");
  });

  it("未解析来源保留在分母与最终限制中，不能因没有可读块而消失", async () => {
    const f = await setup();
    const original = f.request.compiledScope;
    const unavailable = await f.manager.importPastedText({ studioId: original.studioId,
      notebookId: original.notebookIds[0], displayName: "尚未解析的资料", text: "本资料尚未可读。" });
    const scope = f.manager.createTurnScope({ studioId: original.studioId, notebookIds: original.notebookIds,
      sessionPath: original.sessionPath });
    f.request.compiledScope = await f.manager.compileTurnScope(scope);
    f.request.turnId = scope.turnId;
    const result = await f.run();
    expect(f.failures).toEqual([]);
    expect(result.run.status).toBe("partial");
    expect(result.packet.completeness).toMatchObject({ exact: false, totalUnits: 4, checkedUnits: 3,
      unavailableUnits: 1, coverageRatio: 0.75, unavailableSources: [{ sourceId: unavailable.source.id }] });
    expect(f.manager.store.db.prepare("SELECT COUNT(*) AS count FROM knowledge_completeness_units WHERE check_id=?")
      .get(result.packet.completeness!.checkId).count).toBe(3);
    expect(result.block).toContain("Unavailable source entries: 1; canonical units: 3");
    expect(result.block).toContain("由于 1 个单元/来源不可用");
    expect(result.block).toContain("无法证明完整不存在");
  });
});
