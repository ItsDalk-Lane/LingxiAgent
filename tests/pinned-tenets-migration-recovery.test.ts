/**
 * F3 / P1.7 恢复工具测试：旧迁移跑过（.migrated 存在）且没有新版本 completed
 * 收据的目录。默认行为必须是 dry-run；真实恢复必须接收明确批准的源文件与
 * 条目决策，并走与迁移相同的批量写入与收据机制。
 *
 * 对应任务书 M13。
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  scanPinnedTenetsRecovery,
  applyPinnedTenetsRecovery,
} from "../core/pinned-tenets-recovery.ts";
import { activeTenets, listTenets, addTenetDirect, addTenetProposal, tenetsFilePath } from "../lib/memory/tenets.ts";
import { readPinnedTenetsMigrationReceipt } from "../core/pinned-tenets-migration.ts";

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

  it("completed 收据的 agent 不是恢复候选（防复活用户主动删除）", () => {
    const { home, agentDir } = makeAgentHome();
    fs.writeFileSync(path.join(agentDir, "pinned-memory.json.migrated"), JSON.stringify({
      version: 1, items: [{ id: "pin_a", content: "旧条目" }],
    }), "utf-8");
    fs.writeFileSync(path.join(agentDir, "memory", "pinned-tenets-migration.receipt.json"), JSON.stringify({
      version: 1, kind: "migration", agentId: "hana", state: "completed",
    }), "utf-8");

    const report = scanPinnedTenetsRecovery(home);
    expect(report.agents).toHaveLength(0);
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
    const candidates = report.agents[0].candidates;
    const hashOf = (legacyId: string) => candidates.find((c: any) => c.legacyId === legacyId).contentHash;

    const approval = {
      agentId: "hana",
      source: "pinned-memory.json.migrated",
      decisions: [
        { contentHash: hashOf("pin_a"), action: "restore" as const },
        { contentHash: hashOf("pin_b"), action: "restore" as const },
        { contentHash: hashOf("pin_c"), action: "skip" as const },
      ],
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
    const receipt = readPinnedTenetsMigrationReceipt(agentDir);
    expect(receipt).not.toBeNull();
    expect(receipt!.kind).toBe("recovery");
    expect(receipt!.state).toBe("completed");
    expect(receipt!.plan).toHaveLength(2);
    // 归档源不删除（用户数据不自动删除）
    expect(fs.existsSync(path.join(agentDir, "pinned-memory.json.migrated"))).toBe(true);
    // 幂等：同一批准再次 apply 不产生新条目
    const again = applyPinnedTenetsRecovery(home, approval);
    expect(again.restored).toBe(0);
    expect(activeTenets(agentDir)).toHaveLength(2);
  });

  it("拒绝：路径穿越、未知源、未知内容哈希、空决策、非 dry-run 默认", () => {
    const { home, agentDir } = makeAgentHome();
    setupArchived(agentDir);
    const report = scanPinnedTenetsRecovery(home);
    const hashOf = (legacyId: string) => report.agents[0].candidates.find((c: any) => c.legacyId === legacyId).contentHash;

    expect(() => applyPinnedTenetsRecovery(home, {
      agentId: "hana", source: "../pinned-memory.json.migrated",
      decisions: [{ contentHash: hashOf("pin_a"), action: "restore" as const }],
    })).toThrowError(/source/i);

    expect(() => applyPinnedTenetsRecovery(home, {
      agentId: "hana", source: "pinned-memory.json",
      decisions: [{ contentHash: hashOf("pin_a"), action: "restore" as const }],
    })).toThrowError(/source/i);

    expect(() => applyPinnedTenetsRecovery(home, {
      agentId: "hana", source: "pinned-memory.json.migrated",
      decisions: [{ contentHash: "sha256:deadbeef", action: "restore" as const }],
    })).toThrowError(/contentHash|decision/i);

    expect(() => applyPinnedTenetsRecovery(home, {
      agentId: "hana", source: "pinned-memory.json.migrated", decisions: [],
    })).toThrowError(/decision/i);

    // 未知 agent
    expect(() => applyPinnedTenetsRecovery(home, {
      agentId: "nobody", source: "pinned-memory.json.migrated",
      decisions: [{ contentHash: hashOf("pin_a"), action: "restore" as const }],
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
    const missing = candidates.find((c: any) => c.legacyId === "pin_missing");
    applyPinnedTenetsRecovery(home, {
      agentId: "hana",
      source: "pinned-memory.json.migrated",
      decisions: [{ contentHash: missing.contentHash, action: "restore" as const }],
    });

    const contents = activeTenets(agentDir).map(t => t.content);
    expect(contents).toContain("全新条目");
    expect(contents).not.toContain("回复要简短。");
    expect(contents).toContain("回复要简短");
  });
});
