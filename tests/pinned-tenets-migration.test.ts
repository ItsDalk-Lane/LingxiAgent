/**
 * F3/F9 迁移测试：无损、权威来源、状态机、幂等与恢复。
 *
 * 既有断言变化说明（任务书 P1.5）：
 * - 原「与既有 tenets 内容重复的 pin 跳过」用 `"  回复要简短。 "`（带句尾句号）
 *   断言合并——那验证的是 dedupKey 的宽规则（小写+去句尾标点）。新规则要求迁移
 *   去重只做「换行归一+边界空白归一后的精确比较」，句号差异是不同事实，不得合并。
 *   该用例改为 M09 断言不合并；精确重复（仅边界空白差异）的合并由 M07 覆盖。
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  migratePinnedMemoryToTenets,
  migrateAgentPinnedTenets,
  readPinnedTenetsMigrationReceipt,
} from "../core/pinned-tenets-migration.ts";
import {
  activeTenets,
  listTenets,
  pendingTenets,
  buildTenetsPromptSection,
  addTenetDirect,
  removeTenet,
  tenetsFilePath,
  TENET_ERRORS,
  MAX_TENET_CONTENT_CHARS,
} from "../lib/memory/tenets.ts";

const homes: string[] = [];

afterEach(() => {
  vi.restoreAllMocks();
  for (const home of homes.splice(0)) fs.rmSync(home, { recursive: true, force: true });
});

function makeAgentHome(): { home: string; agentDir: string } {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "pins-migration-"));
  homes.push(home);
  const agentDir = path.join(home, "agents", "hana");
  fs.mkdirSync(path.join(agentDir, "memory"), { recursive: true });
  return { home, agentDir };
}

function writeJsonPins(agentDir: string, items: Array<{ id?: string; content: string; createdAt?: string }>, mtime?: Date): string {
  const file = path.join(agentDir, "pinned-memory.json");
  fs.writeFileSync(file, JSON.stringify({ version: 1, items }, null, 2), "utf-8");
  if (mtime) fs.utimesSync(file, mtime, mtime);
  return file;
}

function writeMdPins(agentDir: string, content: string, mtime?: Date): string {
  const file = path.join(agentDir, "pinned.md");
  fs.writeFileSync(file, content, "utf-8");
  if (mtime) fs.utimesSync(file, mtime, mtime);
  return file;
}

function receiptOf(agentDir: string) {
  const receipt = readPinnedTenetsMigrationReceipt(agentDir);
  expect(receipt).not.toBeNull();
  return receipt!;
}

describe("pinned → tenets 一次性迁移（既有行为保持）", () => {
  it("json 优先：pinned-memory.json 条目并入 tenets（active/user_direct），旧文件改名 .migrated", () => {
    const { home, agentDir } = makeAgentHome();
    writeMdPins(agentDir, "- md 里也有一行\n");
    writeJsonPins(agentDir, [
      { id: "pin_a", content: "对花生过敏", createdAt: "2026-08-01T00:00:00.000Z" },
      { id: "pin_b", content: "生日是 3 月 5 日", createdAt: "2026-08-02T00:00:00.000Z" },
    ]);

    migratePinnedMemoryToTenets(home);

    const active = activeTenets(agentDir);
    expect(new Set(active.map(t => t.content))).toEqual(new Set(["对花生过敏", "生日是 3 月 5 日"]));
    expect(active.every(t => t.source === "user_direct" && t.status === "active")).toBe(true);
    expect(fs.existsSync(path.join(agentDir, "pinned.md"))).toBe(false);
    expect(fs.existsSync(path.join(agentDir, "pinned-memory.json"))).toBe(false);
    expect(fs.existsSync(path.join(agentDir, "pinned.md.migrated"))).toBe(true);
    expect(fs.existsSync(path.join(agentDir, "pinned-memory.json.migrated"))).toBe(true);
    migratePinnedMemoryToTenets(home);
    expect(activeTenets(agentDir)).toHaveLength(2);
  });

  it("json 缺失时解析 pinned.md（含缩进续行）", () => {
    const { home, agentDir } = makeAgentHome();
    writeMdPins(agentDir, "- 第一行\n  第二行续\n- 单行条目\n");

    migratePinnedMemoryToTenets(home);

    const contents = activeTenets(agentDir).map(t => t.content);
    expect(contents).toContain("第一行\n第二行续");
    expect(contents).toContain("单行条目");
  });

  it("没有旧 pins 文件的 agent 目录完全不动", () => {
    const { home, agentDir } = makeAgentHome();
    migratePinnedMemoryToTenets(home);
    expect(fs.existsSync(path.join(agentDir, "memory", "tenets.json"))).toBe(false);
    expect(readPinnedTenetsMigrationReceipt(agentDir)).toBeNull();
  });
});

describe("M01/M02：旧版合法内容完整接纳（豁免普通新增限制）", () => {
  it("M01：301 字符、数千字符、多行旧 pin 全文保留并 active", () => {
    const { home, agentDir } = makeAgentHome();
    const long301 = "长".repeat(MAX_TENET_CONTENT_CHARS + 1);
    const huge = `开头\n${"中段很多内容。".repeat(400)}\n结尾`;
    const multiline = "第一行\n第二行\n第三行";
    writeJsonPins(agentDir, [
      { id: "pin_long", content: long301 },
      { id: "pin_huge", content: huge },
      { id: "pin_multi", content: multiline },
    ]);

    migratePinnedMemoryToTenets(home);

    const byContent = new Map(activeTenets(agentDir).map(t => [t.content, t]));
    expect(byContent.has(long301)).toBe(true);
    expect(byContent.has(huge)).toBe(true);
    expect(byContent.has(multiline)).toBe(true);
    expect(byContent.get(long301)!.status).toBe("active");
    expect(byContent.get(huge)!.source).toBe("user_direct");
    const receipt = receiptOf(agentDir);
    expect(receipt.state).toBe("completed");
    expect(receipt.counts.added).toBe(3);
    expect(receipt.plan.every((p: any) => p.exemption === "legacy_migration")).toBe(true);
  });

  it("M02：201 条旧 pin 全部迁移；普通第 202 条新增受新配额限制", () => {
    const { home, agentDir } = makeAgentHome();
    writeJsonPins(agentDir, Array.from({ length: 201 }, (_, i) => ({ id: `pin_${i}`, content: `旧条目 ${i}` })));

    migratePinnedMemoryToTenets(home);
    expect(activeTenets(agentDir)).toHaveLength(201);

    expect(() => addTenetDirect(agentDir, { content: "普通新增第 202 条" }))
      .toThrowError(expect.objectContaining({ code: TENET_ERRORS.LIMIT_REACHED }));
  });
});

describe("M03/M04：旧版权威来源规则（mtime）与删除语义", () => {
  it("M03：Markdown mtime 更新时以 Markdown 为准，否则以 JSON 为准", () => {
    const older = new Date("2026-08-01T00:00:00Z");
    const newer = new Date("2026-08-02T00:00:00Z");

    // md 新、json 旧 → md 权威
    const a = makeAgentHome();
    writeJsonPins(a.agentDir, [{ id: "pin_j", content: "json 条目" }], older);
    writeMdPins(a.agentDir, "- md 条目\n", newer);
    migratePinnedMemoryToTenets(a.home);
    expect(activeTenets(a.agentDir).map(t => t.content)).toEqual(["md 条目"]);

    // json 新、md 旧 → json 权威
    const b = makeAgentHome();
    writeMdPins(b.agentDir, "- md 条目\n", older);
    writeJsonPins(b.agentDir, [{ id: "pin_j", content: "json 条目" }], newer);
    migratePinnedMemoryToTenets(b.home);
    expect(activeTenets(b.agentDir).map(t => t.content)).toEqual(["json 条目"]);

    // mtime 相差 ≤1ms 不视为 md 更新 → json 权威
    const c = makeAgentHome();
    const base = new Date("2026-08-01T00:00:00.000Z");
    const plus1ms = new Date("2026-08-01T00:00:00.001Z");
    writeJsonPins(c.agentDir, [{ id: "pin_j", content: "json 条目" }], base);
    writeMdPins(c.agentDir, "- md 条目\n", plus1ms);
    migratePinnedMemoryToTenets(c.home);
    expect(activeTenets(c.agentDir).map(t => t.content)).toEqual(["json 条目"]);
  });

  it("M04：合法空 items 的 JSON 是权威时保留删除语义；md 更新时采用 md", () => {
    const older = new Date("2026-08-01T00:00:00Z");
    const newer = new Date("2026-08-02T00:00:00Z");

    // 空 JSON 权威（json 较新）：用户在旧版删光了 pins，不得用旧 md 复活
    const a = makeAgentHome();
    writeMdPins(a.agentDir, "- 已被删除的旧条目\n", older);
    writeJsonPins(a.agentDir, [], newer);
    migratePinnedMemoryToTenets(a.home);
    expect(activeTenets(a.agentDir)).toHaveLength(0);
    // 空迁移同样完成并归档，防旧读取路径复活
    expect(fs.existsSync(path.join(a.agentDir, "pinned.md"))).toBe(false);
    expect(receiptOf(a.agentDir).state).toBe("completed");
    expect(receiptOf(a.agentDir).counts.sourceItems).toBe(0);

    // md 较新 → md 权威
    const b = makeAgentHome();
    writeJsonPins(b.agentDir, [], older);
    writeMdPins(b.agentDir, "- md 新条目\n", newer);
    migratePinnedMemoryToTenets(b.home);
    expect(activeTenets(b.agentDir).map(t => t.content)).toEqual(["md 新条目"]);
  });
});

describe("M05/M06：严格读写边界与权威来源损坏", () => {
  it("M05：目标 tenets 损坏时不覆盖、不归档、明确失败；修复后可重试", () => {
    const { home, agentDir } = makeAgentHome();
    const corrupted = path.join(agentDir, "memory", "tenets.json");
    fs.writeFileSync(corrupted, "not-json{", "utf-8");
    const before = fs.readFileSync(corrupted, "utf-8");
    writeJsonPins(agentDir, [{ id: "pin_a", content: "条目" }]);

    migratePinnedMemoryToTenets(home);

    const receipt = receiptOf(agentDir);
    expect(receipt.state).toBe("failed");
    expect(receipt.error.code).toBe(TENET_ERRORS.STORE_CORRUPTED);
    expect(fs.readFileSync(corrupted, "utf-8")).toBe(before);
    expect(fs.existsSync(path.join(agentDir, "pinned-memory.json"))).toBe(true);

    // 修复目标后可重试成功
    fs.rmSync(corrupted);
    migratePinnedMemoryToTenets(home);
    expect(activeTenets(agentDir).map(t => t.content)).toEqual(["条目"]);
    expect(receiptOf(agentDir).state).toBe("completed");
  });

  it("M05b：目标为未来 schema 时不覆盖、明确失败", () => {
    const { home, agentDir } = makeAgentHome();
    fs.writeFileSync(tenetsFilePath(agentDir), JSON.stringify({ schemaVersion: 99, tenets: [] }), "utf-8");
    writeJsonPins(agentDir, [{ id: "pin_a", content: "条目" }]);

    migratePinnedMemoryToTenets(home);

    const receipt = receiptOf(agentDir);
    expect(receipt.state).toBe("failed");
    expect(receipt.error.code).toBe(TENET_ERRORS.STORE_UNSUPPORTED_SCHEMA);
    expect(fs.existsSync(path.join(agentDir, "pinned-memory.json"))).toBe(true);
    expect(JSON.parse(fs.readFileSync(tenetsFilePath(agentDir), "utf-8")).schemaVersion).toBe(99);
  });

  it.skipIf(process.platform === "win32")("M05c：目标 EACCES 时明确失败，不归档", () => {
    const { home, agentDir } = makeAgentHome();
    const target = tenetsFilePath(agentDir);
    fs.writeFileSync(target, JSON.stringify({ schemaVersion: 1, tenets: [] }), "utf-8");
    fs.chmodSync(target, 0o000);
    writeJsonPins(agentDir, [{ id: "pin_a", content: "条目" }]);
    try {
      migratePinnedMemoryToTenets(home);
      const receipt = receiptOf(agentDir);
      expect(receipt.state).toBe("failed");
      expect(receipt.error.code).toBe(TENET_ERRORS.STORE_READ_FAILED);
      expect(fs.existsSync(path.join(agentDir, "pinned-memory.json"))).toBe(true);
    } finally {
      fs.chmodSync(target, 0o644);
    }
  });

  it("M06：权威来源损坏时不静默按空库迁移、不回退另一份、不归档", () => {
    // 仅 JSON 存在且损坏
    const a = makeAgentHome();
    fs.writeFileSync(path.join(a.agentDir, "pinned-memory.json"), "{bad json", "utf-8");
    migratePinnedMemoryToTenets(a.home);
    const receiptA = receiptOf(a.agentDir);
    expect(receiptA.state).toBe("failed");
    expect(receiptA.error.code).toBe("MIGRATION_SOURCE_UNREADABLE");
    expect(fs.existsSync(tenetsFilePath(a.agentDir))).toBe(false);
    expect(fs.existsSync(path.join(a.agentDir, "pinned-memory.json"))).toBe(true);

    // JSON 权威（较新）但损坏；md 较旧且完好 —— 不得回退 md
    const b = makeAgentHome();
    writeMdPins(b.agentDir, "- md 旧条目\n", new Date("2026-08-01T00:00:00Z"));
    const badJson = path.join(b.agentDir, "pinned-memory.json");
    fs.writeFileSync(badJson, "{bad json", "utf-8");
    fs.utimesSync(badJson, new Date("2026-08-02T00:00:00Z"), new Date("2026-08-02T00:00:00Z"));
    migratePinnedMemoryToTenets(b.home);
    expect(receiptOf(b.agentDir).state).toBe("failed");
    expect(activeTenets(b.agentDir)).toHaveLength(0);
    expect(fs.existsSync(path.join(b.agentDir, "pinned.md"))).toBe(true);
  });
});

describe("M07–M09：迁移去重规则（精确内容比较）", () => {
  it("M07：与 active 精确重复（含边界空白/换行归一差异）不新增，映射记录到收据", () => {
    const { home, agentDir } = makeAgentHome();
    const existing = addTenetDirect(agentDir, { content: "对花生过敏" });
    writeJsonPins(agentDir, [
      { id: "pin_a", content: "  对花生过敏\r\n" },
      { id: "pin_b", content: "生日是 3 月 5 日" },
    ]);

    migratePinnedMemoryToTenets(home);

    expect(activeTenets(agentDir)).toHaveLength(2);
    const receipt = receiptOf(agentDir);
    const dupEntry = receipt.plan.find((p: any) => p.legacyId === "pin_a");
    expect(dupEntry.outcome).toBe("duplicate_active");
    expect(dupEntry.tenetId).toBe(existing.tenet.id);
    const addEntry = receipt.plan.find((p: any) => p.legacyId === "pin_b");
    expect(addEntry.outcome).toBe("added");
    expect(addEntry.tenetId).not.toBe(existing.tenet.id);
  });

  it("M08：只与 pending/rejected 重复时仍迁移为 active/user_direct，历史保留", () => {
    const { home, agentDir } = makeAgentHome();
    fs.writeFileSync(tenetsFilePath(agentDir), JSON.stringify({
      schemaVersion: 1,
      tenets: [
        { id: "t-pending", content: "要简洁", priority: "medium", status: "pending", source: "model_proposed", sessionId: null, createdAt: "2026-08-01T00:00:00.000Z", decidedAt: null },
        { id: "t-rejected", content: "少用表情", priority: "medium", status: "rejected", source: "model_proposed", sessionId: null, createdAt: "2026-08-01T00:00:00.000Z", decidedAt: "2026-08-02T00:00:00.000Z" },
      ],
    }, null, 2), "utf-8");
    writeJsonPins(agentDir, [
      { id: "pin_a", content: "要简洁" },
      { id: "pin_b", content: "少用表情" },
    ]);

    migratePinnedMemoryToTenets(home);

    const active = activeTenets(agentDir);
    expect(active.map(t => t.content).sort()).toEqual(["少用表情", "要简洁"]);
    expect(active.every(t => t.source === "user_direct")).toBe(true);
    // 历史审批状态不被篡改
    expect(pendingTenets(agentDir).map(t => t.id)).toEqual(["t-pending"]);
    const all = listTenets(agentDir);
    expect(all.filter(t => t.status === "rejected").map(t => t.id)).toEqual(["t-rejected"]);
    const receipt = receiptOf(agentDir);
    expect(receipt.plan.find((p: any) => p.legacyId === "pin_a").outcome).toBe("added_over_pending_history");
    expect(receipt.plan.find((p: any) => p.legacyId === "pin_a").historyTenetId).toBe("t-pending");
    expect(receipt.plan.find((p: any) => p.legacyId === "pin_b").outcome).toBe("added_over_rejected_history");
  });

  it("M09：仅大小写/句尾标点相似不合并（不同事实各自保留）", () => {
    const { home, agentDir } = makeAgentHome();
    addTenetDirect(agentDir, { content: "回复要简短" });
    addTenetDirect(agentDir, { content: "我喜欢猫" });
    writeJsonPins(agentDir, [
      { id: "pin_a", content: "回复要简短。" },
      { id: "pin_b", content: "我喜欢猫" },
    ]);

    migratePinnedMemoryToTenets(home);

    const contents = activeTenets(agentDir).map(t => t.content);
    expect(contents).toContain("回复要简短");
    expect(contents).toContain("回复要简短。");
    // 完全相同的才合并
    expect(contents.filter(c => c === "我喜欢猫")).toHaveLength(1);
    expect(activeTenets(agentDir)).toHaveLength(3);
  });
});

describe("M10/M11/M14：状态机崩溃恢复与幂等", () => {
  const checkpoints = [
    "receipt:prepared",
    "commit:before",
    "commit:after",
    "archive:before:pinned-memory.json",
    "completed:before",
  ];

  for (const point of checkpoints) {
    it(`M10：在 ${point} 注入崩溃后重启可恢复，无丢失无重复`, () => {
      const { agentDir } = makeAgentHome();
      const jsonTime = new Date("2026-08-02T00:00:00Z");
      const mdTime = new Date("2026-08-01T00:00:00Z");
      writeJsonPins(agentDir, [
        { id: "pin_a", content: "条目 A" },
        { id: "pin_b", content: "条目 B" },
      ], jsonTime);
      writeMdPins(agentDir, "- md 条目\n", mdTime);

      expect(() => migrateAgentPinnedTenets(agentDir, "hana", {
        at: (checkpoint) => { if (checkpoint === point) throw new Error(`boom at ${point}`); },
      })).toThrowError(/boom at/);

      // 恢复运行（无故障）必须走到 completed
      migrateAgentPinnedTenets(agentDir, "hana");

      const contents = activeTenets(agentDir).map(t => t.content).sort();
      expect(contents).toEqual(["条目 A", "条目 B"]);
      expect(receiptOf(agentDir).state).toBe("completed");
      expect(fs.existsSync(path.join(agentDir, "pinned-memory.json"))).toBe(false);
      expect(fs.existsSync(path.join(agentDir, "pinned.md"))).toBe(false);
      // 再跑幂等
      migrateAgentPinnedTenets(agentDir, "hana");
      expect(activeTenets(agentDir)).toHaveLength(2);
    });
  }

  it("M11：目标已提交但归档失败，下次只完成归档，不重写目标、不丢后续数据", () => {
    const { agentDir } = makeAgentHome();
    writeJsonPins(agentDir, [{ id: "pin_a", content: "条目 A" }], new Date("2026-08-02T00:00:00Z"));
    writeMdPins(agentDir, "- md 条目\n", new Date("2026-08-01T00:00:00Z"));

    expect(() => migrateAgentPinnedTenets(agentDir, "hana", {
      at: (checkpoint) => { if (checkpoint === "archive:before:pinned.md") throw new Error("archive boom"); },
    })).toThrowError(/archive boom/);
    expect(receiptOf(agentDir).state).toBe("target_committed");

    // 崩溃后用户继续写入新 tenet
    addTenetDirect(agentDir, { content: "崩溃后用户新增" });

    migrateAgentPinnedTenets(agentDir, "hana");

    const contents = activeTenets(agentDir).map(t => t.content);
    expect(contents).toContain("条目 A");
    expect(contents).toContain("崩溃后用户新增");
    expect(contents).toHaveLength(2);
    expect(fs.existsSync(path.join(agentDir, "pinned.md"))).toBe(false);
    expect(fs.existsSync(path.join(agentDir, "pinned.md.migrated"))).toBe(true);
    expect(receiptOf(agentDir).state).toBe("completed");
  });

  it("M12：completed 后用户删除迁移条目，重启不复活", () => {
    const { home, agentDir } = makeAgentHome();
    writeJsonPins(agentDir, [
      { id: "pin_a", content: "条目 A" },
      { id: "pin_b", content: "条目 B" },
    ]);
    migratePinnedMemoryToTenets(home);
    expect(activeTenets(agentDir)).toHaveLength(2);

    const victim = activeTenets(agentDir).find(t => t.content === "条目 A")!;
    expect(removeTenet(agentDir, victim.id)).toBe(true);

    migratePinnedMemoryToTenets(home);
    migratePinnedMemoryToTenets(home);
    const contents = activeTenets(agentDir).map(t => t.content);
    expect(contents).toEqual(["条目 B"]);
  });

  it("M14a：连续运行两次幂等", () => {
    const { home, agentDir } = makeAgentHome();
    writeJsonPins(agentDir, [{ id: "pin_a", content: "条目 A" }]);
    migratePinnedMemoryToTenets(home);
    migratePinnedMemoryToTenets(home);
    expect(activeTenets(agentDir)).toHaveLength(1);
  });

  it("M14b：prepare 后源文件被改动 → conflict，不覆盖、不归档，且不自动重试", () => {
    const { agentDir } = makeAgentHome();
    const jsonFile = writeJsonPins(agentDir, [{ id: "pin_a", content: "条目 A" }]);

    expect(() => migrateAgentPinnedTenets(agentDir, "hana", {
      at: (checkpoint) => {
        if (checkpoint === "recheck:before") {
          fs.appendFileSync(jsonFile, " ", "utf-8");
        }
      },
    })).toThrowError(/boom|conflict/i);

    const receipt = receiptOf(agentDir);
    expect(receipt.state).toBe("conflict");
    expect(fs.existsSync(path.join(agentDir, "pinned-memory.json"))).toBe(true);
    expect(fs.existsSync(tenetsFilePath(agentDir))).toBe(false);

    // conflict 是粘性的：不自动重试覆盖
    migrateAgentPinnedTenets(agentDir, "hana");
    expect(receiptOf(agentDir).state).toBe("conflict");
    expect(fs.existsSync(path.join(agentDir, "pinned-memory.json"))).toBe(true);
  });
});

describe("M20：迁移结果对设置页与 system prompt 可见", () => {
  it("只注入 active；多行完整可见；pending/rejected 不注入", () => {
    const { home, agentDir } = makeAgentHome();
    fs.writeFileSync(tenetsFilePath(agentDir), JSON.stringify({
      schemaVersion: 1,
      tenets: [
        { id: "t-p", content: "待审提案", priority: "medium", status: "pending", source: "model_proposed", sessionId: null, createdAt: "2026-08-01T00:00:00.000Z", decidedAt: null },
      ],
    }, null, 2), "utf-8");
    const multiline = "第一行\n第二行";
    writeJsonPins(agentDir, [{ id: "pin_m", content: multiline }]);

    migratePinnedMemoryToTenets(home);

    // 设置页读取语义（listTenets 全量、active 过滤）
    expect(listTenets(agentDir).some(t => t.content === multiline && t.status === "active")).toBe(true);
    // system prompt 注入：多行按缩进续行完整可见，pending 不出现
    const section = buildTenetsPromptSection(agentDir, true) || "";
    expect(section).toContain("- 第一行\n  第二行");
    expect(section).not.toContain("待审提案");
  });
});

describe("遗留 .migrated 与既有归档共存", () => {
  it("归档目标已存在时改用带源哈希的后缀，不覆盖既有 .migrated", () => {
    const { home, agentDir } = makeAgentHome();
    // 旧版迁移留下的归档
    fs.writeFileSync(path.join(agentDir, "pinned-memory.json.migrated"), JSON.stringify({ version: 1, items: [{ id: "pin_old", content: "旧归档内容" }] }), "utf-8");
    const before = fs.readFileSync(path.join(agentDir, "pinned-memory.json.migrated"), "utf-8");
    // 新的源（例如用户手工放回）
    writeJsonPins(agentDir, [{ id: "pin_new", content: "新条目" }]);

    migratePinnedMemoryToTenets(home);

    expect(activeTenets(agentDir).map(t => t.content)).toEqual(["新条目"]);
    expect(fs.readFileSync(path.join(agentDir, "pinned-memory.json.migrated"), "utf-8")).toBe(before);
    const siblings = fs.readdirSync(agentDir).filter(f => f.startsWith("pinned-memory.json.migrated"));
    expect(siblings.length).toBeGreaterThan(1);
  });
});
