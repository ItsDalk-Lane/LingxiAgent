/**
 * run_tools（PTC）测试：
 * - 绑定桥：直挂工具直达、目录工具转发 mcp_call、自身递归拒绝、未知名 fail-closed；
 * - 权限同权证据：子调用以合成 ID（<外层>:ptc:<n>）重走包装后工具的 execute，
 *   拒绝以 isError/ToolCallError 传播进程序且可 try/catch；
 * - 结果语义：子调用中间结果不进 content（只进 details.subcalls），
 *   content 只有 console 输出 + return 值（截断 50KB）；
 * - 运行时边界：沙箱无 require/process/fetch/动态 import、可擦除 TS 通过、
 *   非可擦除报错、超时/中止/输出超限/并发上限。
 */
import { describe, expect, it, vi } from "vitest";
import { createPtcBindingHolder, createPtcTool, RUN_TOOLS_TOOL_NAME } from "../ptc-tool.ts";
import { runPtcProgram } from "../../ptc/ptc-runtime.ts";

function textResult(text: string, extra: Record<string, unknown> = {}) {
  return { content: [{ type: "text", text }], ...extra };
}

function makeWrappedTool(name: string, impl?: (params: any) => any) {
  const tool = {
    name,
    // 五参签名与真实工具的 execute 对齐：测试要断言子调用 ID（1 号位）与 ctx（5 号位）透传。
    execute: vi.fn(async (_id: string, params: any, _signal?: any, _onUpdate?: any, _ctx?: any) =>
      (impl ? impl(params) : textResult(`${name}-ok`))),
  };
  return tool;
}

function makeTool(directTools: any[] = [], opts: { attach?: boolean } = {}) {
  const binding = createPtcBindingHolder();
  // execute 返回是 成功/失败 两个对象字面量的推断联合；测试侧按 any 消费 details。
  const tool: any = createPtcTool({ binding });
  if (opts.attach !== false) binding.attach({ tools: [...directTools, tool] });
  return { tool, binding };
}

async function run(tool: any, code: string, extraParams: Record<string, unknown> = {}, ctx: any = { sessionPath: "/tmp/s.jsonl" }) {
  return tool.execute("tc-outer", { code, description: "test program", ...extraParams }, undefined, undefined, ctx);
}

describe("run_tools 工具契约", () => {
  it("code 缺失时报错且不进沙箱", async () => {
    const { tool } = makeTool();
    const result = await tool.execute("tc", { description: "x" }, undefined, undefined, {});
    expect(result.isError).toBe(true);
    expect(result.content[0].text).toContain("code is required");
  });

  it("权限契约：execute 动作 + routine 档，能力名对齐工具名", () => {
    const { tool } = makeTool();
    expect(tool.name).toBe("run_tools");
    expect(tool.sessionPermission.resolveInvocation({})).toEqual({
      action: "execute",
      kind: "routine",
      capability: "run_tools.execute",
    });
  });

  it("绑定表未 attach 时如实报错", async () => {
    const { tool } = makeTool([], { attach: false });
    const result = await run(tool, "return 1");
    expect(result.isError).toBe(true);
    expect(result.details.errorCode).toBe("RUN_TOOLS_NOT_ATTACHED");
  });
});

describe("run_tools 绑定桥", () => {
  it("直挂工具子调用：合成 ID/参数/ctx 原样透传，文本结果解包成字符串", async () => {
    const grep = makeWrappedTool("grep", (p) => textResult(`found:${p.pattern}`));
    const { tool } = makeTool([grep]);
    const ctx = { sessionPath: "/tmp/s.jsonl", agentId: "a1" };
    const result = await tool.execute("tc-outer", {
      code: "const hits = await tools.grep({ pattern: 'foo' }); console.log('H=' + hits); return hits;",
      description: "grep once",
    }, undefined, undefined, ctx);

    expect(result.isError).toBeUndefined();
    expect(grep.execute).toHaveBeenCalledTimes(1);
    const [subId, params, _signal, _onUpdate, subCtx] = grep.execute.mock.calls[0];
    expect(subId).toBe("tc-outer:ptc:1");
    expect(params).toEqual({ pattern: "foo" });
    expect(subCtx).toBe(ctx);
    expect(result.content[0].text).toContain("H=found:foo");
    expect(result.details.subcalls).toEqual([
      expect.objectContaining({ seq: 1, name: "grep", ok: true }),
    ]);
  });

  it("目录工具转发包装后的 mcp_call，并携带 {tool, arguments}", async () => {
    const mcpCall = makeWrappedTool("mcp_call", (p) => textResult(`via-bridge:${p.tool}`));
    const { tool } = makeTool([mcpCall]);
    const result = await run(tool, "return await tools.web_search({ query: 'q' });");

    expect(mcpCall.execute).toHaveBeenCalledTimes(1);
    expect(mcpCall.execute.mock.calls[0][1]).toEqual({ tool: "web_search", arguments: { query: "q" } });
    expect(result.content[0].text).toContain("via-bridge:web_search");
    expect(result.details.subcalls[0]).toEqual(expect.objectContaining({ name: "web_search", ok: true }));
  });

  it("run_tools 不能调自己（防递归），错误可被程序捕获", async () => {
    const { tool } = makeTool();
    const result = await run(tool, "const r = await tools.run_tools({}).catch((e) => e.toolName + ':' + e.message); return r;");
    expect(result.isError).toBeUndefined();
    expect(result.content[0].text).toContain("run_tools:run_tools cannot call itself");
    expect(result.details.subcalls[0]).toEqual(expect.objectContaining({ name: "run_tools", ok: false }));
  });

  it("未知名且直挂面无该工具时走桥；桥也没有则 fail-closed", async () => {
    const { tool } = makeTool();
    const result = await run(tool, "return await tools.nope({}).catch((e) => 'caught:' + e.toolName);");
    expect(result.content[0].text).toContain("caught:nope");
  });

  it("工具 isError → 程序内抛 ToolCallError（带 toolName），可 try/catch 继续", async () => {
    const write = makeWrappedTool("write", () => textResult("permission denied by user", { isError: true }));
    const { tool } = makeTool([write]);
    const result = await run(tool, `
      try { await tools.write({ path: 'a' }); return "NOT-REACHED"; }
      catch (e) { return e.name + "/" + e.toolName + "/" + e.message; }
    `);
    expect(result.content[0].text).toContain("ToolCallError/write/permission denied by user");
    expect(result.details.subcalls[0]).toEqual(expect.objectContaining({ ok: false, error: expect.stringContaining("denied") }));
  });

  it("execute 抛异常同样落成 ToolCallError", async () => {
    const boom = makeWrappedTool("boom", () => { throw new Error("kaboom"); });
    const { tool } = makeTool([boom]);
    const result = await run(tool, "return await tools.boom({}).catch((e) => e.message);");
    expect(result.content[0].text).toContain("kaboom");
  });

  it("多工具顺序+并发混编：Promise.all 全部完成，subcalls 按序记录", async () => {
    const read = makeWrappedTool("read", (p) => textResult(`content-of-${p.path}`));
    const { tool } = makeTool([read]);
    const result = await run(tool, `
      const one = await tools.read({ path: 'a.txt' });
      const rest = await Promise.all(['b.txt', 'c.txt'].map((p) => tools.read({ path: p })));
      return [one, ...rest].join('|');
    `);
    expect(result.content[0].text).toContain("content-of-a.txt|content-of-b.txt|content-of-c.txt");
    expect(result.details.subcalls.map((s: any) => s.seq)).toEqual([1, 2, 3]);
    expect(read.execute.mock.calls.map((c: any[]) => c[0])).toEqual(["tc-outer:ptc:1", "tc-outer:ptc:2", "tc-outer:ptc:3"]);
  });
});

describe("run_tools 结果语义", () => {
  it("子调用中间结果不进 content，只有 console + return 进", async () => {
    const read = makeWrappedTool("read", () => textResult("SECRET-INTERMEDIATE"));
    const { tool } = makeTool([read]);
    const result = await run(tool, `
      const v = await tools.read({ path: 'x' });
      console.log('visible line');
      return 'summary-only';
    `);
    expect(result.content[0].text).toContain("visible line");
    expect(result.content[0].text).toContain("summary-only");
    expect(result.content[0].text).not.toContain("SECRET-INTERMEDIATE");
    // 中间结果只在 details 留痕（参数摘要/成败），供 UI 展开
    expect(result.details.subcalls).toHaveLength(1);
    expect(result.details.description).toBe("test program");
    expect(typeof result.details.durationMs).toBe("number");
  });

  it("return 对象渲染为 JSON；无输出无返回时如实说明", async () => {
    const { tool } = makeTool();
    const withValue = await run(tool, "return { a: 1, b: [2, 3] };");
    expect(withValue.content[0].text).toContain('"a": 1');
    const silent = await run(tool, "const x = 1;");
    expect(silent.content[0].text).toContain("without printing or returning");
  });

  it("正文超 50KB 截断（保头保尾）", async () => {
    const { tool } = makeTool();
    const result = await run(tool, "console.log('x'.repeat(120_000));");
    expect(result.content[0].text.length).toBeLessThan(60_000);
    expect(result.isError).toBeUndefined();
  });

  it("子调用记录携带完整实参与结果文本（子调用行展开用）", async () => {
    const read = makeWrappedTool("read", () => textResult("line-1\nline-2"));
    const { tool } = makeTool([read]);
    const result: any = await run(tool, `
      await tools.read({ path: 'notes.md', limit: 2 });
      return 'done';
    `);
    const [record] = result.details.subcalls;
    expect(record.ok).toBe(true);
    expect(record.args).toEqual({ path: "notes.md", limit: 2 });
    expect(record.output).toBe("line-1\nline-2");
    expect(record.outputTruncated).toBeUndefined();
    // 程序没把中间值 print/return 出来时，content 依旧不含它——记录材料只进 details。
    expect(result.content[0].text).not.toContain("line-1");
  });

  it("子调用输出超单条/总预算时截断并如实标记", async () => {
    const big = makeWrappedTool("grep", () => textResult("y".repeat(30_000)));
    const { tool } = makeTool([big]);
    const result: any = await run(tool, `
      for (let i = 0; i < 10; i++) { await tools.grep({ pattern: 'p' + i }); }
      return 'done';
    `);
    const records = result.details.subcalls;
    expect(records).toHaveLength(10);
    // 单条 8KB：每条都截断保头保尾；总预算 64KB：第八条起撞总量闸门，后面只留摘要。
    expect(records[0].outputTruncated).toBe(true);
    expect(records[0].output.length).toBeLessThan(20_000);
    const firstOmitted = records.find((record: any) => record.outputOmitted === true);
    expect(firstOmitted).toBeTruthy();
    expect(firstOmitted.output).toBeUndefined();
    expect(records[9].outputOmitted).toBe(true);
  });

  it("程序异常：isError + 异常消息 + 已捕获日志随附", async () => {
    const { tool } = makeTool();
    const result = await run(tool, "console.log('before-crash'); throw new Error('program bug');");
    expect(result.isError).toBe(true);
    expect(result.details.errorCode).toBe("RUN_TOOLS_EXCEPTION");
    expect(result.content[0].text).toContain("program bug");
    expect(result.content[0].text).toContain("before-crash");
  });
});

describe("ptc-runtime 沙箱边界", () => {
  const noCall = async () => ({ ok: true as const, value: null });

  it("无 require/process/fetch，动态 import 被拒", async () => {
    const outcome = await runPtcProgram({
      code: `
        console.log(typeof require, typeof process, typeof fetch);
        try { await import('node:fs'); console.log('import-allowed'); }
        catch (e) { console.log('import-blocked'); }
      `,
      call: noCall,
    });
    expect(outcome.ok).toBe(true);
    expect(outcome.logs[0]).toBe("undefined undefined undefined");
    expect(outcome.logs[1]).toBe("import-blocked");
  });

  it("可擦除 TS 通过，非可擦除语法报 exception", async () => {
    const okRun = await runPtcProgram({ code: "const n: number = 41; return n + 1;", call: noCall });
    expect(okRun.ok).toBe(true);
    expect((okRun as any).value).toBe(42);
    const bad = await runPtcProgram({ code: "enum A { B }", call: noCall });
    expect(bad.ok).toBe(false);
    expect((bad as any).kind).toBe("exception");
    expect((bad as any).message).toContain("erasable");
  });

  it("墙钟超时 → timeout，且如实带已捕获日志", async () => {
    const outcome = await runPtcProgram({
      code: "console.log('started'); while (true) {}",
      call: noCall,
      budgets: { wallMs: 500 },
    });
    expect(outcome.ok).toBe(false);
    expect((outcome as any).kind).toBe("timeout");
    expect((outcome as any).logs).toContain("started");
  }, 10_000);

  it("输出超限 → output-limit", async () => {
    const outcome = await runPtcProgram({
      code: "for (let i = 0; i < 5000; i++) console.log(i, 'y'.repeat(200));",
      call: noCall,
      budgets: { maxLogBytes: 4000 },
    });
    expect(outcome.ok).toBe(false);
    expect((outcome as any).kind).toBe("output-limit");
  }, 10_000);

  it("中止信号联动：abort → abort 类失败", async () => {
    const ac = new AbortController();
    const promise = runPtcProgram({ code: "await new Promise(() => {})", call: noCall, signal: ac.signal });
    setTimeout(() => ac.abort(), 100);
    const outcome = await promise;
    expect(outcome.ok).toBe(false);
    expect((outcome as any).kind).toBe("abort");
  }, 10_000);

  it("并发闸：在飞子调用不超过 maxConcurrent", async () => {
    let inFlight = 0;
    let maxSeen = 0;
    const outcome = await runPtcProgram({
      code: "await Promise.all(Array.from({ length: 12 }, (_, i) => tools.slow({ i })));",
      call: async () => {
        inFlight += 1;
        maxSeen = Math.max(maxSeen, inFlight);
        await new Promise((r) => setTimeout(r, 20));
        inFlight -= 1;
        return { ok: true as const, value: null };
      },
      budgets: { maxConcurrent: 3 },
    });
    expect(outcome.ok).toBe(true);
    expect(maxSeen).toBeLessThanOrEqual(3);
  }, 10_000);

  it("返回函数等不可克隆值 → invalid-output", async () => {
    const outcome = await runPtcProgram({ code: "return () => 1;", call: noCall });
    expect(outcome.ok).toBe(false);
    expect((outcome as any).kind).toBe("invalid-output");
  });
});
