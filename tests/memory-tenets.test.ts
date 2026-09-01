import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";

vi.mock("../lib/i18n.js", () => ({
  getLocale: () => "zh-CN",
  t: (key: string, vars?: Record<string, string>) => {
    const dict: Record<string, string> = {
      "error.tenetProposeLabel": "提出用户原则",
      "error.tenetProposeDesc": "desc",
      "error.tenetProposeContentDesc": "content",
      "error.tenetProposePriorityDesc": "prio",
      "error.tenetProposePaused": "paused",
      "error.tenetProposeInvalid": "invalid",
      "error.tenetProposeTooLong": `too long ${vars?.max}`,
      "error.tenetProposeDuplicate": `dup ${vars?.status}`,
      "error.tenetProposeSubmitted": "submitted",
      "error.tenetProposePendingFull": "pending full",
      "error.tenetProposeError": `err ${vars?.msg}`,
    };
    return dict[key] ?? key;
  },
}));

import {
  addTenetProposal,
  addTenetDirect,
  decideTenet,
  removeTenet,
  listTenets,
  activeTenets,
  pendingTenets,
  buildTenetsPromptSection,
  MAX_ACTIVE_TENETS,
  MAX_PENDING_TENETS,
  MAX_TENET_CONTENT_CHARS,
  TENET_ERRORS,
  tenetsFilePath,
} from "../lib/memory/tenets.ts";
import { createTenetProposeTool } from "../lib/tools/tenet-propose-tool.ts";

describe("用户原则（tenets）存储", () => {
  let agentDir;

  beforeEach(() => {
    agentDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-tenets-"));
  });

  afterEach(() => {
    fs.rmSync(agentDir, { recursive: true, force: true });
  });

  it("提案 → pending；批准 → active；注入块只含 active 且 critical 在前", () => {
    const { tenet } = addTenetProposal(agentDir, { content: "改文件前先说明影响范围", priority: "high" });
    expect(tenet.status).toBe("pending");
    expect(pendingTenets(agentDir)).toHaveLength(1);
    expect(buildTenetsPromptSection(agentDir, true)).toBeNull(); // 只有 pending 时不注入

    decideTenet(agentDir, tenet.id, true);
    expect(activeTenets(agentDir)).toHaveLength(1);

    addTenetDirect(agentDir, { content: "删除文件必须二次确认", priority: "critical" });
    const section = buildTenetsPromptSection(agentDir, true) || "";
    expect(section).toContain("# 用户原则");
    expect(section.indexOf("[critical]")).toBeLessThan(section.indexOf("[high]"));
    expect(section).toContain("改文件前先说明影响范围");
    // 英文头
    expect(buildTenetsPromptSection(agentDir, false)).toContain("# User Principles");
  });

  it("拒绝 → rejected，不注入；重复审批显式报错", () => {
    const { tenet } = addTenetProposal(agentDir, { content: "A" });
    decideTenet(agentDir, tenet.id, false);
    expect(activeTenets(agentDir)).toHaveLength(0);
    expect(buildTenetsPromptSection(agentDir, true)).toBeNull();
    expect(() => decideTenet(agentDir, tenet.id, true)).toThrowError(/already rejected/);
  });

  it("归一化查重：空白/大小写/句尾标点差异视为同一条", () => {
    addTenetProposal(agentDir, { content: "改文件前先说明影响范围。" });
    const dup = addTenetProposal(agentDir, { content: "  改文件前先说明影响范围 " });
    expect(dup.duplicate).toBe(true);
    expect(pendingTenets(agentDir)).toHaveLength(1);
  });

  it("active 上限 20：满员后 direct/审批都显式报 TENET_LIMIT_REACHED", () => {
    for (let i = 0; i < MAX_ACTIVE_TENETS; i++) {
      addTenetDirect(agentDir, { content: `原则 ${i}` });
    }
    expect(() => addTenetDirect(agentDir, { content: "超限原则" }))
      .toThrowError(expect.objectContaining({ code: TENET_ERRORS.LIMIT_REACHED }));

    const proposal = addTenetProposal(agentDir, { content: "待审提案" });
    expect(() => decideTenet(agentDir, proposal.tenet.id, true))
      .toThrowError(expect.objectContaining({ code: TENET_ERRORS.LIMIT_REACHED }));
  });

  it("pending 上限 30：满员后新提案报 TENET_PENDING_FULL", () => {
    for (let i = 0; i < MAX_PENDING_TENETS; i++) {
      addTenetProposal(agentDir, { content: `提案 ${i}` });
    }
    expect(() => addTenetProposal(agentDir, { content: "再来一条" }))
      .toThrowError(expect.objectContaining({ code: TENET_ERRORS.PENDING_FULL }));
  });

  it("内容校验：空/超长报 TENET_INVALID", () => {
    expect(() => addTenetProposal(agentDir, { content: "   " }))
      .toThrowError(expect.objectContaining({ code: TENET_ERRORS.INVALID }));
    expect(() => addTenetDirect(agentDir, { content: "x".repeat(MAX_TENET_CONTENT_CHARS + 1) }))
      .toThrowError(expect.objectContaining({ code: TENET_ERRORS.INVALID }));
  });

  it("删除与不存在 id 的报错", () => {
    const { tenet } = addTenetDirect(agentDir, { content: "A" });
    expect(removeTenet(agentDir, tenet.id)).toBe(true);
    expect(removeTenet(agentDir, tenet.id)).toBe(false);
    expect(() => decideTenet(agentDir, "no-such-id", true))
      .toThrowError(expect.objectContaining({ code: TENET_ERRORS.NOT_FOUND }));
  });

  it("文件损坏/缺失时读侧降级为空列表", () => {
    fs.mkdirSync(path.dirname(tenetsFilePath(agentDir)), { recursive: true });
    fs.writeFileSync(tenetsFilePath(agentDir), "not-json{", "utf-8");
    expect(listTenets(agentDir)).toEqual([]);
  });
});

describe("tenet_propose 工具", () => {
  let agentDir;
  let tool;
  let proposed: any[];

  beforeEach(() => {
    agentDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-tenet-tool-"));
    proposed = [];
    tool = createTenetProposeTool(agentDir, {
      onProposed: (tenet) => proposed.push(tenet),
    });
  });

  afterEach(() => {
    fs.rmSync(agentDir, { recursive: true, force: true });
  });

  it("正常提案：写入 pending、回调触发、details 带 tenetId", async () => {
    const res = await tool.execute("t1", { content: "周报先用英文写", priority: "high" });
    expect(res.content[0].text).toContain("submitted");
    expect(res.details.status).toBe("pending");
    expect(res.details.priority).toBe("high");
    expect(proposed).toHaveLength(1);
    expect(pendingTenets(agentDir)).toHaveLength(1);
  });

  it("重复提案幂等返回 duplicate，不触发回调", async () => {
    await tool.execute("t1", { content: "周报先用英文写" });
    const res = await tool.execute("t2", { content: "周报先用英文写。" });
    expect(res.details.duplicate).toBe(true);
    expect(proposed).toHaveLength(1);
  });

  it("isEnabled=false 返回暂停提示且不写库", async () => {
    const disabledTool = createTenetProposeTool(agentDir, { isEnabled: () => false });
    const res = await disabledTool.execute("t1", { content: "A" });
    expect(res.content[0].text).toBe("paused");
    expect(pendingTenets(agentDir)).toHaveLength(0);
  });

  it("超长/空内容在工具层拦截", async () => {
    const empty = await tool.execute("t1", { content: "" });
    expect(empty.content[0].text).toBe("invalid");
    const long = await tool.execute("t2", { content: "x".repeat(MAX_TENET_CONTENT_CHARS + 1) });
    expect(long.content[0].text).toContain("too long");
  });
});
