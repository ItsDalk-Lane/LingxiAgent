/**
 * F3 / P1.7 恢复工具测试：旧迁移归档与逐操作完成范围。
 * 默认行为必须是 dry-run；真实恢复必须接收明确批准的源文件与
 * 条目决策，并走与迁移相同的批量写入与收据机制。
 *
 * 对应任务书 M13；R02/S2 将批准绑定源摘要、目标快照与 operationId。
 * 本轮调整理由：旧手造 version=1 completed 不是有效收据，改由真实迁移生成；
 * completed 仅覆盖本计划，不能隐藏其他归档；同 operation 重试返回原摘要。
 * 拒绝与不默认恢复断言仍保留，只更新为新批准 schema，避免旧参数先失败
 * 而未实际验证路径、源、内容与决策边界。
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  scanPinnedTenetsRecovery,
  applyPinnedTenetsRecovery,
} from "../core/pinned-tenets-recovery.ts";
import { activeTenets, listTenets, addTenetDirect, addTenetProposal, tenetsFilePath, removeTenet } from "../lib/memory/tenets.ts";
import { migrateAgentPinnedTenets, readPinnedTenetsMigrationReceipt } from "../core/pinned-tenets-migration.ts";

const homes: string[] = [];

afterEach(() => {
  for (const home of homes.splice(0)) fs.rmSync(home, { recursive: true, force: true });
});

function makeAgentHome(): { home: string; agentDir: string } {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "pins-recovery-"));
  homes.push(home);
  const agentDir = path.join(home, "agents", "hana");
  fs.mkdirSync(path.join(agentDir, "memory"), { recursive: true });
  return { home, agentDir };
}

function snapshotTree(dir: string): string[] {
  const out: string[] = [];
  const walk = (cur: string) => {
    for (const entry of fs.readdirSync(cur, { withFileTypes: true })) {
      const full = path.join(cur, entry.name);
      const rel = path.relative(dir, full);
      if (entry.isDirectory()) walk(full);
      else out.push(`${rel}:${fs.statSync(full).size}`);
    }
  };
  walk(dir);
  return out.sort();
}

describe("M13：旧 .migrated 无收据 → 默认仅 dry-run", () => {
  it("scan 报告可恢复项但不改任何文件、不写收据", () => {
    const { home, agentDir } = makeAgentHome();
    fs.writeFileSync(path.join(agentDir, "pinned-memory.json.migrated"), JSON.stringify({
      version: 1,
      items: [
        { id: "pin_a", content: "对花生过敏", createdAt: "2026-08-01T00:00:00.000Z" },
        { id: "pin_b", content: "生日是 3 月 5 日" },
      ],
    }, null, 2), "utf-8");
    const before = snapshotTree(agentDir);

    const report = scanPinnedTenetsRecovery(home);

    expect(report.agents).toHaveLength(1);
    const agent = report.agents[0];
    expect(agent.agentId).toBe("hana");
    expect(agent.receiptState).toBeNull();
    expect(agent.archivedSources.map((s: any) => s.file)).toEqual(["pinned-memory.json.migrated"]);
    expect(agent.candidates).toHaveLength(2);
    expect(agent.candidates.every((c: any) => c.classification === "missing")).toBe(true);
    expect(agent.recoverableCount).toBe(2);
    // dry-run：目录零变化、无收据、无 tenets.json
    expect(snapshotTree(agentDir)).toEqual(before);
    expect(readPinnedTenetsMigrationReceipt(agentDir)).toBeNull();
    expect(fs.existsSync(tenetsFilePath(agentDir))).toBe(false);
  });

  it("completed 仅覆盖本计划：删除的条目不复活，其他归档仍为候选", () => {
    const { home, agentDir } = makeAgentHome();
    fs.writeFileSync(path.join(agentDir, "pinned-memory.json"), JSON.stringify({
      version: 1, items: [{ id: "pin_a", content: "旧条目" }],
    }), "utf-8");
    migrateAgentPinnedTenets(agentDir, "hana");
    const receipt = readPinnedTenetsMigrationReceipt(agentDir)!;
    expect(receipt.state).toBe("completed");
    const imported = activeTenets(agentDir).find(item => item.content === "旧条目")!;
    expect(removeTenet(agentDir, imported.id)).toBe(true);
    fs.writeFileSync(path.join(agentDir, "pinned-memory.json.migrated-other"), JSON.stringify({
      version: 1, items: [{ id: "pin_other", content: "另一个尚未恢复的条目" }],
    }), "utf-8");
    const before = snapshotTree(agentDir);

    const report = scanPinnedTenetsRecovery(home);
    expect(report.agents).toHaveLength(1);
    expect(report.agents[0].candidates.find(item => item.legacyId === "pin_a"))
      .toMatchObject({ classification: "previously_restored_now_missing", legacyRestored: true });
    expect(report.agents[0].candidates.find(item => item.legacyId === "pin_other"))
      .toMatchObject({ classification: "missing" });
    expect(report.agents[0].recoverableCount).toBe(1);
    expect(report.agents[0].approvalTemplate.decisions.every(item => item.action === "skip")).toBe(true);
    migrateAgentPinnedTenets(agentDir, "hana");
    expect(activeTenets(agentDir)).toHaveLength(0);
    expect(readPinnedTenetsMigrationReceipt(agentDir)).toEqual(receipt);
    expect(snapshotTree(agentDir)).toEqual(before);
  });

  it("分类：present / inactive / similar / missing", () => {
    const { home, agentDir } = makeAgentHome();
    addTenetDirect(agentDir, { content: "完全一致的" });
    addTenetProposal(agentDir, { content: "待审状态条目" });
    addTenetDirect(agentDir, { content: "回复要简短" });
    fs.writeFileSync(path.join(agentDir, "pinned-memory.json.migrated"), JSON.stringify({
      version: 1,
      items: [
        { id: "pin_present", content: "完全一致的" },
        { id: "pin_inactive", content: "待审状态条目" },
        { id: "pin_similar", content: "回复要简短。" },
        { id: "pin_missing", content: "全新条目" },
      ],
    }, null, 2), "utf-8");

    const report = scanPinnedTenetsRecovery(home);
    const byLegacy = new Map(report.agents[0].candidates.map((c: any) => [c.legacyId, c]));
    expect(byLegacy.get("pin_present").classification).toBe("present");
    expect(byLegacy.get("pin_inactive").classification).toBe("inactive");
    expect(byLegacy.get("pin_similar").classification).toBe("similar");
    expect(byLegacy.get("pin_missing").classification).toBe("missing");
    expect(report.agents[0].recoverableCount).toBe(2); // inactive + missing
  });
});

describe("恢复 apply：明确批准后走同一批量写入与收据机制", () => {
  function setupArchived(agentDir: string) {
    fs.writeFileSync(path.join(agentDir, "pinned-memory.json.migrated"), JSON.stringify({
      version: 1,
      items: [
        { id: "pin_a", content: "对花生过敏", createdAt: "2026-08-01T00:00:00.000Z" },
        { id: "pin_b", content: "生日是 3 月 5 日", createdAt: "2026-08-02T00:00:00.000Z" },
        { id: "pin_c", content: "不要恢复我" },
      ],
    }, null, 2), "utf-8");
  }

  it("按批准清单恢复选中条目；幂等；收据 kind=recovery", () => {
    const { home, agentDir } = makeAgentHome();
    setupArchived(agentDir);
    const report = scanPinnedTenetsRecovery(home);
    const template = report.agents[0].approvalTemplate;
    expect(template.decisions.every(decision => decision.action === "skip")).toBe(true);
    const approval = {
      ...template,
      decisions: template.decisions.map(decision => ({
        ...decision,
        action: decision.sourceEntryKey === "pin_c" ? "skip" as const : "restore" as const,
      })),
    };
    const result = applyPinnedTenetsRecovery(home, approval);

    expect(result.restored).toBe(2);
    const contents = activeTenets(agentDir).map(t => t.content);
    expect(contents).toContain("对花生过敏");
    expect(contents).toContain("生日是 3 月 5 日");
    expect(contents).not.toContain("不要恢复我");
    // 创建时间合法保留
    const kept = activeTenets(agentDir).find(t => t.content === "对花生过敏")!;
    expect(kept.createdAt).toBe("2026-08-01T00:00:00.000Z");
    // 收据
    const receiptFile = path.join(agentDir, "memory", "pinned-recovery-operations", `${approval.operationId}.json`);
    const receipt = JSON.parse(fs.readFileSync(receiptFile, "utf-8"));
    expect(readPinnedTenetsMigrationReceipt(agentDir)).toBeNull();
    expect(receipt.operationId).toBe(approval.operationId);
    expect(receipt.version).toBe(3);
    expect(receipt!.kind).toBe("recovery");
    expect(receipt!.state).toBe("completed");
    expect(receipt!.plan).toHaveLength(2);
    // 归档源不删除（用户数据不自动删除）
    expect(fs.existsSync(path.join(agentDir, "pinned-memory.json.migrated"))).toBe(true);
    // 幂等：同一批准再次 apply 不产生新条目
    const again = applyPinnedTenetsRecovery(home, approval);
    expect(again).toEqual(result);
    expect(again.restored).toBe(2);
    expect(JSON.parse(fs.readFileSync(receiptFile, "utf-8"))).toEqual(receipt);
    expect(activeTenets(agentDir)).toHaveLength(2);
  });

  it("拒绝：路径穿越、未知源、未知内容哈希、空决策、非 dry-run 默认", () => {
    const { home, agentDir } = makeAgentHome();
    setupArchived(agentDir);
    const report = scanPinnedTenetsRecovery(home);
    const template = report.agents[0].approvalTemplate;
    const decision = { ...template.decisions.find(item => item.sourceEntryKey === "pin_a")!, action: "restore" as const };
    const approval = { ...template, decisions: [decision] };
    const source = template.sources[0];

    expect(() => applyPinnedTenetsRecovery(home, {
      ...approval, sources: [{ ...source, file: "../pinned-memory.json.migrated" }],
      decisions: [{ ...decision, source: "../pinned-memory.json.migrated" }],
    })).toThrowError(/source/i);

    expect(() => applyPinnedTenetsRecovery(home, {
      ...approval, sources: [{ ...source, file: "pinned-memory.json" }],
      decisions: [{ ...decision, source: "pinned-memory.json" }],
    })).toThrowError(/source/i);

    expect(() => applyPinnedTenetsRecovery(home, {
      ...approval, decisions: [{ ...decision, contentHash: "sha256:deadbeef" }],
    })).toThrowError(/contentHash|decision/i);
    // 形状合法但内容身份错误也必须拒绝，不能只通过 schema 拒绝伪哈希。
    expect(() => applyPinnedTenetsRecovery(home, {
      ...approval, decisions: [{ ...decision, contentHash: `sha256:${"0".repeat(64)}` }],
    })).toThrowError(/source entry mismatch/i);

    expect(() => applyPinnedTenetsRecovery(home, {
      ...approval, decisions: [],
    })).toThrowError(/invalid approval schema/i);
    // 模板默认全 skip，不构成恢复批准。
    expect(() => applyPinnedTenetsRecovery(home, template)).toThrowError(/no restore decisions/i);

    // 未知 agent 仍拒绝；错误可以来自不存在的 agents/nobody 目录。
    expect(() => applyPinnedTenetsRecovery(home, {
      ...approval, agentId: "nobody",
    })).toThrowError(/agent/i);

    expect(listTenets(agentDir)).toHaveLength(0);
  });

  it("恢复不默认全量复活：只恢复 restore 决策，inactive/similar 需显式选择", () => {
    const { home, agentDir } = makeAgentHome();
    addTenetDirect(agentDir, { content: "回复要简短" });
    fs.writeFileSync(path.join(agentDir, "pinned-memory.json.migrated"), JSON.stringify({
      version: 1,
      items: [
        { id: "pin_similar", content: "回复要简短。" },
        { id: "pin_missing", content: "全新条目" },
      ],
    }, null, 2), "utf-8");

    const report = scanPinnedTenetsRecovery(home);
    const candidates = report.agents[0].candidates;
    // 只批准 missing 的；similar 不给决策即不恢复
    const missing = candidates.find(c => c.legacyId === "pin_missing")!;
    applyPinnedTenetsRecovery(home, {
      ...report.agents[0].approvalTemplate,
      decisions: [{ source: missing.source, sourceEntryKey: missing.sourceEntryKey,
        contentHash: missing.contentHash, action: "restore" as const }],
    });

    const contents = activeTenets(agentDir).map(t => t.content);
    expect(contents).toContain("全新条目");
    expect(contents).not.toContain("回复要简短。");
    expect(contents).toContain("回复要简短");
  });
});
