/**
 * run_code 常驻内核测试：语言可用性门、注入包裹与回显剥离、静默返回、
 * 仍在执行如实说明、内核死亡报告与 restart 重建、label 身份兜底、权限契约。
 * manager 用带 transcript 累加的轻量假件模拟。
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createRunCodeTool } from "../run-code-tool.ts";

function makeManager() {
  // terminals: terminalId → {entry, transcript}
  const terminals = new Map<string, { entry: any; transcript: string; seq: number }>();
  let seqCounter = 1;
  return {
    terminals,
    start: vi.fn(async ({ sessionPath, command, label }: any) => {
      const terminalId = `t-${seqCounter++}`;
      terminals.set(terminalId, { entry: { terminalId, sessionPath, command, label, status: "running", exitCode: null, seq: 0 }, transcript: "", seq: 0 });
      return { terminalId };
    }),
    list: vi.fn((sessionPath: string) => ({
      sessionPath,
      terminals: [...terminals.values()].map((t) => ({ ...t.entry, seq: t.seq })).filter((e) => e.sessionPath === sessionPath),
    })),
    write: vi.fn(({ terminalId, chars }: any) => {
      const t = terminals.get(terminalId);
      if (!t) throw new Error("no such terminal");
      // PTY 回显：注入行原样回显，随后是结果行
      t.transcript += chars.split("\n")[0] + "\n";
      t.seq += 1;
    }),
    feed: (terminalId: string, text: string) => {
      const t = terminals.get(terminalId);
      if (!t) return;
      t.transcript += text;
      t.seq += 1;
    },
    readTail: vi.fn(({ terminalId }: any) => {
      const t = terminals.get(terminalId);
      return { output: t ? t.transcript : "" };
    }),
    close: vi.fn(({ terminalId }: any) => {
      const t = terminals.get(terminalId);
      if (t) t.entry.status = "exited";
    }),
    kill: (terminalId: string, exitCode = 1) => {
      const t = terminals.get(terminalId);
      if (t) { t.entry.status = "exited"; t.entry.exitCode = exitCode; }
    },
  };
}

function makeTool(manager: any, languages: string[] = ["python3", "node"]) {
  return createRunCodeTool({
    manager,
    getSessionPath: () => "/tmp/s.jsonl",
    getAgentId: () => "a1",
    getCwd: () => "/tmp",
    getAvailableLanguages: async () => languages,
  });
}

describe("run_code 工具", () => {
  beforeEach(() => { vi.clearAllMocks(); });

  it("变量跨调用存活（同内核连续两次 run）+ 回显剥除 + 结果输出", async () => {
    const manager = makeManager();
    const tool = makeTool(manager);
    // 让假件在 write 后异步补结果
    manager.write.mockImplementation(({ terminalId, chars }: any) => {
      const t = manager.terminals.get(terminalId);
      t.transcript += chars.split("\n")[0] + "\n";
      t.seq += 1;
      setTimeout(() => manager.feed(terminalId, "40\n"), 60);
    });
    const r1: any = await tool.execute("c1", { language: "python3", code: "x = 40" });
    expect(r1.details.language).toBe("python3");
    expect(r1.content[0].text).not.toContain("exec(__import__"); // 回显已剥
    manager.write.mockImplementation(({ terminalId, chars }: any) => {
      const t = manager.terminals.get(terminalId);
      t.transcript += chars.split("\n")[0] + "\n";
      t.seq += 1;
      setTimeout(() => manager.feed(terminalId, ">>> 40\n40\n"), 60);
    });
    const r2: any = await tool.execute("c2", { language: "python3", code: "print(x)" });
    expect(r2.content[0].text).toContain("40");
    // 两次共用同一内核
    expect(manager.start).toHaveBeenCalledTimes(1);
  });

  it("缺语言：RUN_CODE_LANGUAGE_MISSING + 安装指引", async () => {
    const manager = makeManager();
    const tool = makeTool(manager, ["node"]);
    const r: any = await tool.execute("c1", { language: "python3", code: "1" });
    expect(r.isError).toBe(true);
    expect(r.details.errorCode).toBe("RUN_CODE_LANGUAGE_MISSING");
    expect(r.content[0].text).toContain("Env Dependencies");
  });

  it("内核死亡：如实报告 exit 与部分输出，指路 restart", async () => {
    const manager = makeManager();
    const tool = makeTool(manager);
    const r1: any = await tool.execute("c1", { language: "node", code: "1+1" });
    expect(r1.isError).toBeUndefined();
    const terminalId = manager.start.mock.calls[0][0].terminalId ?? manager.terminals.keys().next().value;
    manager.kill(terminalId, 3);
    const r2: any = await tool.execute("c2", { language: "node", code: "2+2" });
    expect(r2.isError).toBe(true);
    expect(r2.details.errorCode).toBe("RUN_CODE_KERNEL_DEAD");
    expect(r2.content[0].text).toContain("restart");
  });

  it("run 中途死亡：RUN_CODE_KERNEL_EXITED + 部分输出", async () => {
    const manager = makeManager();
    const tool = makeTool(manager);
    await tool.execute("c1", { language: "node", code: "1" });
    const terminalId = [...manager.terminals.keys()][0];
    const pending = tool.execute("c2", { language: "node", code: "while(true){}" }).then((r: any) => r);
    await new Promise((r) => setTimeout(r, 150));
    manager.feed(terminalId, "partial output\n");
    manager.kill(terminalId, 137);
    const r: any = await pending;
    expect(r.isError).toBe(true);
    expect(r.details.errorCode).toBe("RUN_CODE_KERNEL_EXITED");
    expect(r.content[0].text).toContain("partial output");
  });

  it("restart 重建 + status 查询", async () => {
    const manager = makeManager();
    const tool = makeTool(manager);
    await tool.execute("c1", { language: "python3", code: "1" });
    const restarted: any = await tool.execute("c2", { language: "python3", action: "restart" });
    expect(restarted.content[0].text).toContain("rebuilt");
    const status: any = await tool.execute("c3", { language: "python3", action: "status" });
    expect(status.content[0].text).toContain("running");
  });

  it("label 身份兜底：工具实例重建后不重复 start（按 label 找回内核）", async () => {
    const manager = makeManager();
    const tool1 = makeTool(manager);
    await tool1.execute("c1", { language: "python3", code: "1" });
    const tool2 = makeTool(manager); // 新实例（map 空）
    await tool2.execute("c2", { language: "python3", code: "2" });
    expect(manager.start).toHaveBeenCalledTimes(1);
  });

  it("未知语言直接拒绝", async () => {
    const tool = makeTool(makeManager());
    const r: any = await tool.execute("c1", { language: "ruby", code: "1" });
    expect(r.isError).toBe(true);
    expect(r.content[0].text).toContain("python3");
  });

  it("权限契约：status=read；run/restart=write", () => {
    const tool = makeTool(makeManager());
    expect(tool.sessionPermission.resolveInvocation({ action: "status" }).kind).toBe("read");
    expect(tool.sessionPermission.resolveInvocation({}).kind).toBe("routine");
    expect(tool.sessionPermission.resolveInvocation({ action: "restart" }).kind).toBe("routine");
  });
});
