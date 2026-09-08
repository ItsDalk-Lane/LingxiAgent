/**
 * F9 反例测试：active 配额按来源分离（model_proposed 20 / user_direct 200）、
 * pending/rejected 不占 active 配额、isTenetError 指定码精确匹配、
 * pin_memory 写入失败不假报「已添加」。
 *
 * 对应任务书 M15–M19。
 */
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";

vi.mock("../lib/i18n.js", () => ({
  getLocale: () => "zh-CN",
  t: (key: string, vars?: Record<string, string>) => {
    const dict: Record<string, string> = {
      "error.pinnedAlreadyExists": "已经存在该条目",
      "error.pinnedAdded": `已添加 ${vars?.content ?? ""}`,
      "error.pinnedEmpty": "empty",
      "error.pinnedNotFound": `未找到 ${vars?.keyword ?? ""}`,
      "error.pinnedRemoved": "已删除",
    };
    return dict[key] ?? key;
  },
}));

import {
  addTenetDirect,
  addTenetProposal,
  decideTenet,
  activeTenets,
  pendingTenets,
  buildTenetsPromptSection,
  countActiveTenetsBySource,
  isTenetError,
  tenetsFilePath,
  TENET_ERRORS,
  MAX_ACTIVE_TENETS,
  MAX_ACTIVE_USER_TENETS,
  MAX_PENDING_TENETS,
  MAX_TENET_CONTENT_CHARS,
} from "../lib/memory/tenets.ts";
import { createPinnedMemoryTools } from "../lib/tools/pinned-memory.ts";

let agentDir: string;

beforeEach(() => {
  agentDir = fs.mkdtempSync(path.join(os.tmpdir(), "tenets-quota-"));
});

afterEach(() => {
  fs.rmSync(agentDir, { recursive: true, force: true });
});

type SeedOptions = {
  status?: "pending" | "active" | "rejected";
  source?: "model_proposed" | "user_direct";
};

function seedTenets(count: number, opts: SeedOptions = {}, prefix = "条目"): void {
  const tenets = Array.from({ length: count }, (_, i) => ({
    id: `${opts.status ?? "active"}-${opts.source ?? "user_direct"}-${prefix}-${i}`,
    content: `${prefix} ${i}`,
    priority: "medium",
    status: opts.status ?? "active",
    source: opts.source ?? "user_direct",
    sessionId: null,
    createdAt: new Date(Date.UTC(2026, 0, 1, 0, 0, i)).toISOString(),
    decidedAt: null,
  }));
  fs.mkdirSync(path.dirname(tenetsFilePath(agentDir)), { recursive: true });
  fs.writeFileSync(tenetsFilePath(agentDir), JSON.stringify({ schemaVersion: 1, tenets }, null, 2), "utf-8");
}

describe("countActiveTenetsBySource", () => {
  it("只统计 active，按来源分列；pending/rejected 不计入", () => {
    const data = [
      { status: "active", source: "model_proposed" },
      { status: "active", source: "model_proposed" },
      { status: "active", source: "user_direct" },
      { status: "pending", source: "model_proposed" },
      { status: "rejected", source: "model_proposed" },
      { status: "rejected", source: "user_direct" },
    ].map((t, i) => ({
      id: `x-${i}`, content: `c-${i}`, priority: "medium", ...t,
      sessionId: null, createdAt: "2026-01-01T00:00:00.000Z", decidedAt: null,
    }));
    expect(countActiveTenetsBySource(data as any)).toEqual({ modelProposed: 2, userDirect: 1 });
    expect(countActiveTenetsBySource([])).toEqual({ modelProposed: 0, userDirect: 0 });
  });
});

describe("M15/M16：两来源配额互不占用", () => {
  it("M15：已有 20 条 active/user_direct 时，首个 model_proposed 审批成功", () => {
    seedTenets(MAX_ACTIVE_TENETS, { status: "active", source: "user_direct" }, "直钉");
    const proposal = addTenetProposal(agentDir, { content: "首条模型提案" });
    const decided = decideTenet(agentDir, proposal.tenet.id, true);
    expect(decided.status).toBe("active");
    const counts = countActiveTenetsBySource(activeTenets(agentDir));
    expect(counts).toEqual({ modelProposed: 1, userDirect: MAX_ACTIVE_TENETS });
  });

  it("M16：已有 20 条 active/model_proposed 时，user_direct 直钉仍成功", () => {
    seedTenets(MAX_ACTIVE_TENETS, { status: "active", source: "model_proposed" }, "原则");
    const result = addTenetDirect(agentDir, { content: "用户直钉" });
    expect(result.duplicate).toBe(false);
    const counts = countActiveTenetsBySource(activeTenets(agentDir));
    expect(counts).toEqual({ modelProposed: MAX_ACTIVE_TENETS, userDirect: 1 });
  });
});

describe("M17：各自配额到顶时明确拒绝，不影响另一来源", () => {
  it("第 21 条 model_proposed 审批被拒；user_direct 直钉不受影响", () => {
    seedTenets(MAX_ACTIVE_TENETS, { status: "active", source: "model_proposed" }, "原则");
    const proposal = addTenetProposal(agentDir, { content: "第 21 条" });
    expect(() => decideTenet(agentDir, proposal.tenet.id, true))
      .toThrowError(expect.objectContaining({ code: TENET_ERRORS.LIMIT_REACHED }));
    expect(addTenetDirect(agentDir, { content: "直钉仍可" }).duplicate).toBe(false);
  });

  it("第 201 条 user_direct 新增被拒；model_proposed 审批不受影响", () => {
    seedTenets(MAX_ACTIVE_USER_TENETS, { status: "active", source: "user_direct" }, "直钉");
    expect(() => addTenetDirect(agentDir, { content: "第 201 条" }))
      .toThrowError(expect.objectContaining({ code: TENET_ERRORS.LIMIT_REACHED }));
    const proposal = addTenetProposal(agentDir, { content: "模型提案" });
    expect(decideTenet(agentDir, proposal.tenet.id, true).status).toBe("active");
  });

  it("历史超量条目保留不删；只阻止该来源继续新增", () => {
    // 旧迁移/旧版本可能留下超过当前上限的 active/model_proposed
    seedTenets(MAX_ACTIVE_TENETS + 3, { status: "active", source: "model_proposed" }, "遗留");
    expect(activeTenets(agentDir)).toHaveLength(MAX_ACTIVE_TENETS + 3);
    expect(buildTenetsPromptSection(agentDir, true)).toContain("遗留 22");
    const proposal = addTenetProposal(agentDir, { content: "新提案" });
    expect(() => decideTenet(agentDir, proposal.tenet.id, true))
      .toThrowError(expect.objectContaining({ code: TENET_ERRORS.LIMIT_REACHED }));
    // 已有内容不删
    expect(activeTenets(agentDir)).toHaveLength(MAX_ACTIVE_TENETS + 3);
  });
});

describe("M18：pending/rejected 不占 active 配额；pending 上限独立", () => {
  it("大量 pending/rejected 不影响两个 active 配额", () => {
    seedTenets(MAX_PENDING_TENETS, { status: "pending", source: "model_proposed" }, "待审");
    expect(addTenetDirect(agentDir, { content: "直钉" }).duplicate).toBe(false);
    expect(() => addTenetProposal(agentDir, { content: "第 31 条提案" }))
      .toThrowError(expect.objectContaining({ code: TENET_ERRORS.PENDING_FULL }));
    expect(countActiveTenetsBySource(activeTenets(agentDir))).toEqual({ modelProposed: 0, userDirect: 1 });
  });
});

describe("直钉遇 pending/rejected 同内容：必须产生 active 结果且保留历史", () => {
  it("与 pending 重复：新增 active/user_direct，pending 历史不动", () => {
    const proposal = addTenetProposal(agentDir, { content: "要简洁" });
    const direct = addTenetDirect(agentDir, { content: "要简洁" });
    expect(direct.duplicate).toBe(false);
    expect(direct.tenet.status).toBe("active");
    expect(direct.tenet.source).toBe("user_direct");
    const pending = pendingTenets(agentDir);
    expect(pending).toHaveLength(1);
    expect(pending[0].id).toBe(proposal.tenet.id);
  });

  it("与 rejected 重复：新增 active/user_direct，rejected 事实不篡改", () => {
    const proposal = addTenetProposal(agentDir, { content: "要简洁" });
    decideTenet(agentDir, proposal.tenet.id, false);
    const direct = addTenetDirect(agentDir, { content: "要简洁" });
    expect(direct.duplicate).toBe(false);
    const rejected = JSON.parse(fs.readFileSync(tenetsFilePath(agentDir), "utf-8")).tenets
      .filter((t: any) => t.status === "rejected");
    expect(rejected).toHaveLength(1);
    expect(rejected[0].id).toBe(proposal.tenet.id);
  });

  it("与 active 重复：仍按既有语义返回 duplicate，不新增", () => {
    addTenetDirect(agentDir, { content: "要简洁" });
    const again = addTenetDirect(agentDir, { content: "要简洁。" });
    expect(again.duplicate).toBe(true);
    expect(activeTenets(agentDir)).toHaveLength(1);
  });
});

describe("M19：isTenetError 指定码精确匹配；pin_memory 不假报成功", () => {
  it("传 code 时只精确匹配该码；不传才表示任意 TenetError", () => {
    const invalid: any = new Error("bad");
    invalid.code = TENET_ERRORS.INVALID;
    const limit: any = new Error("full");
    limit.code = TENET_ERRORS.LIMIT_REACHED;

    expect(isTenetError(invalid, TENET_ERRORS.INVALID)).toBe(true);
    expect(isTenetError(invalid, TENET_ERRORS.LIMIT_REACHED)).toBe(false);
    expect(isTenetError(limit, TENET_ERRORS.LIMIT_REACHED)).toBe(true);
    expect(isTenetError(limit, TENET_ERRORS.INVALID)).toBe(false);

    expect(isTenetError(invalid)).toBe(true);
    expect(isTenetError(limit)).toBe(true);
    expect(isTenetError(new Error("plain"))).toBe(false);
    expect(isTenetError({ code: "SOME_OTHER" })).toBe(false);
    expect(isTenetError(null)).toBe(false);
  });

  it("pin_memory 内容非法（INVALID）时返回失败结果并保留错误码，不冒充「已添加」", async () => {
    const [pinTool] = createPinnedMemoryTools(agentDir);
    const result: any = await pinTool.execute("call-1", { content: "x".repeat(MAX_TENET_CONTENT_CHARS + 1) });
    expect(result.isError).toBe(true);
    expect(result.details.errorCode).toBe(TENET_ERRORS.INVALID);
    expect(result.content[0].text).not.toContain("已添加");
    // 没写入任何内容
    expect(activeTenets(agentDir)).toHaveLength(0);
  });

  it("pin_memory 配额满（LIMIT_REACHED）时返回失败结果并保留错误码", async () => {
    seedTenets(MAX_ACTIVE_USER_TENETS, { status: "active", source: "user_direct" }, "直钉");
    const [pinTool] = createPinnedMemoryTools(agentDir);
    const result: any = await pinTool.execute("call-2", { content: "放不下了" });
    expect(result.isError).toBe(true);
    expect(result.details.errorCode).toBe(TENET_ERRORS.LIMIT_REACHED);
    expect(result.content[0].text).not.toContain("已添加");
    expect(activeTenets(agentDir)).toHaveLength(MAX_ACTIVE_USER_TENETS);
  });

  it("pin_memory 正常路径不受影响", async () => {
    const [pinTool] = createPinnedMemoryTools(agentDir);
    const ok: any = await pinTool.execute("call-3", { content: "对花生过敏" });
    expect(ok.isError).toBeUndefined();
    expect(ok.details.item.content).toBe("对花生过敏");
    const dup: any = await pinTool.execute("call-4", { content: "对花生过敏" });
    expect(dup.isError).toBeUndefined();
    expect(dup.content[0].text).toContain("已经存在");
  });
});
