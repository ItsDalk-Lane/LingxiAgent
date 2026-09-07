import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { migratePinnedMemoryToTenets } from "../core/pinned-tenets-migration.ts";
import { activeTenets, listTenets } from "../lib/memory/tenets.ts";

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

describe("pinned → tenets 一次性迁移", () => {
  it("json 优先：pinned-memory.json 条目并入 tenets（active/user_direct），旧文件改名 .migrated", () => {
    const { home, agentDir } = makeAgentHome();
    fs.writeFileSync(path.join(agentDir, "pinned.md"), "- md 里也有一行\n", "utf-8");
    fs.writeFileSync(path.join(agentDir, "pinned-memory.json"), JSON.stringify({
      version: 1,
      items: [
        { id: "pin_a", content: "对花生过敏", createdAt: "2026-08-01T00:00:00.000Z" },
        { id: "pin_b", content: "生日是 3 月 5 日", createdAt: "2026-08-02T00:00:00.000Z" },
      ],
    }, null, 2), "utf-8");

    migratePinnedMemoryToTenets(home);

    const active = activeTenets(agentDir);
    expect(new Set(active.map(t => t.content))).toEqual(new Set(["对花生过敏", "生日是 3 月 5 日"]));
    expect(active.every(t => t.source === "user_direct" && t.status === "active")).toBe(true);
    // 旧文件改名留证，不删除
    expect(fs.existsSync(path.join(agentDir, "pinned.md"))).toBe(false);
    expect(fs.existsSync(path.join(agentDir, "pinned-memory.json"))).toBe(false);
    expect(fs.existsSync(path.join(agentDir, "pinned.md.migrated"))).toBe(true);
    expect(fs.existsSync(path.join(agentDir, "pinned-memory.json.migrated"))).toBe(true);
    // 再跑一次：源文件已不在，幂等
    migratePinnedMemoryToTenets(home);
    expect(activeTenets(agentDir)).toHaveLength(2);
  });

  it("json 缺失/为空时回退解析 pinned.md（含缩进续行）", () => {
    const { home, agentDir } = makeAgentHome();
    fs.writeFileSync(path.join(agentDir, "pinned.md"), "- 第一行\n  第二行续\n- 单行条目\n", "utf-8");

    migratePinnedMemoryToTenets(home);

    const contents = activeTenets(agentDir).map(t => t.content);
    expect(contents).toContain("第一行\n第二行续");
    expect(contents).toContain("单行条目");
  });

  it("与既有 tenets 内容重复的 pin 跳过，不产生重复条目", () => {
    const { home, agentDir } = makeAgentHome();
    fs.writeFileSync(path.join(agentDir, "memory", "tenets.json"), JSON.stringify({
      schemaVersion: 1,
      tenets: [{
        id: "t1", content: "回复要简短", priority: "medium", status: "active",
        source: "model_proposed", sessionId: null,
        createdAt: "2026-08-01T00:00:00.000Z", decidedAt: "2026-08-01T00:00:00.000Z",
      }],
    }), "utf-8");
    fs.writeFileSync(path.join(agentDir, "pinned-memory.json"), JSON.stringify({
      version: 1,
      items: [{ id: "pin_x", content: "  回复要简短。 " }],
    }), "utf-8");

    migratePinnedMemoryToTenets(home);

    expect(listTenets(agentDir)).toHaveLength(1);
    expect(listTenets(agentDir)[0].source).toBe("model_proposed");
    expect(fs.existsSync(path.join(agentDir, "pinned-memory.json.migrated"))).toBe(true);
  });

  it("没有旧 pins 文件的 agent 目录完全不动", () => {
    const { home, agentDir } = makeAgentHome();
    migratePinnedMemoryToTenets(home);
    expect(fs.existsSync(path.join(agentDir, "memory", "tenets.json"))).toBe(false);
  });
});
