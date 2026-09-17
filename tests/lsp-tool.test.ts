/**
 * lsp 工具测试（阶段三·14）：mock LSP server（tests/fixtures/mock-lsp-server.mjs，
 * 真进程真帧协议）覆盖 initialize 握手、7 动作往返、行号 1↔0 基转换、
 * rename 验证后写回、prepareRename 拒绝、服务器缺失、权限契约。
 * 服务器命令注入为 `node mock-lsp-server.mjs`（typescript 语言位）。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { createLspTool, LSP_LANGUAGES, lspLanguageForFile, summarizeLocations } from "../lib/tools/lsp-tool.ts";

const mockServerPath = fileURLToPath(new URL("./fixtures/mock-lsp-server.mjs", import.meta.url));
const roots: string[] = [];
function freshRoot() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-lsp-"));
  roots.push(root);
  fs.writeFileSync(path.join(root, "a.ts"), "function greet(name) {\n  const msg = greet(name);\n  return msg;\n}\n");
  return root;
}
afterEach(() => { while (roots.length) fs.rmSync(roots.pop()!, { recursive: true, force: true }); });

function makeTool(root: string, opts: { available?: boolean; writes?: Map<string, string> } = {}) {
  const writes = opts.writes ?? new Map<string, string>();
  return {
    tool: createLspTool({
      cwd: root,
      getSessionPath: () => path.join(root, "s.jsonl"),
      probeServer: async () => opts.available !== false,
      readFile: async (p) => (writes.has(p) ? writes.get(p)! : fs.readFileSync(p, "utf8")),
      writeFile: async (p, content) => { writes.set(p, content); },
    }),
    writes,
  };
}

// 让 typescript 语言的 server 指向 mock（命令注入点）
const realTs = { ...LSP_LANGUAGES.typescript };
LSP_LANGUAGES.typescript = {
  ...realTs,
  serverCommand: process.execPath,
  serverArgs: [mockServerPath],
};

describe("lsp 工具（mock server 全动作）", () => {
  beforeEach(() => { vi.clearAllMocks(); });

  it("status：逐语言可用性", async () => {
    const root = freshRoot();
    const { tool } = makeTool(root);
    const r: any = await tool.execute("c1", { action: "status" });
    expect(r.content[0].text).toContain("typescript: ready");
  });

  it("definition：1-based 输入 → 0-based 协议 → 1-based 输出", async () => {
    const root = freshRoot();
    const { tool } = makeTool(root);
    const r: any = await tool.execute("c1", { action: "definition", language: "typescript", file: "a.ts", line: 2, column: 16 });
    expect(r.content[0].text).toContain("target.ts:5:1");
    expect(r.details.count).toBe(1);
  });

  it("references：多位置清单", async () => {
    const root = freshRoot();
    const { tool } = makeTool(root);
    const r: any = await tool.execute("c1", { action: "references", language: "typescript", file: "a.ts", line: 1, column: 10 });
    expect(r.content[0].text).toContain("a.ts:3:9");
    expect(r.content[0].text).toContain("b.ts:10:5");
  });

  it("hover：markdown 值摘录", async () => {
    const root = freshRoot();
    const { tool } = makeTool(root);
    const r: any = await tool.execute("c1", { action: "hover", language: "typescript", file: "a.ts", line: 1, column: 10 });
    expect(r.content[0].text).toContain("function greet");
  });

  it("symbols：含嵌套 children 的扁平轮廓", async () => {
    const root = freshRoot();
    const { tool } = makeTool(root);
    const r: any = await tool.execute("c1", { action: "symbols", language: "typescript", file: "a.ts" });
    expect(r.content[0].text).toContain("greet");
    expect(r.content[0].text).toContain("inner");
    expect(r.details.count).toBe(3);
  });

  it("diagnostics：didOpen 推送收集 + 分级展示", async () => {
    const root = freshRoot();
    const { tool } = makeTool(root);
    const r: any = await tool.execute("c1", { action: "diagnostics", language: "typescript", file: "a.ts" });
    expect(r.content[0].text).toContain("[error] a.ts:3");
    expect(r.content[0].text).toContain("[warning] a.ts:6");
    expect(r.details.count).toBe(2);
  });

  it("rename：prepareRename 验证 → 编辑计算 → 写回", async () => {
    const root = freshRoot();
    const { tool, writes } = makeTool(root);
    const r: any = await tool.execute("c1", { action: "rename", language: "typescript", file: "a.ts", line: 1, column: 10, new_name: "salute" });
    expect(r.details.applied).toBe(true);
    expect(r.content[0].text).toContain("salute");
    const changed = [...writes.keys()];
    expect(changed).toHaveLength(1);
    expect(writes.get(changed[0])).toContain("function salute(name)");
  });

  it("prepareRename 拒绝：如实拒绝不写", async () => {
    const root = freshRoot();
    const { tool, writes } = makeTool(root);
    process.env.MOCK_LSP_RENAME_REJECT = "1";
    try {
      const r: any = await tool.execute("c1", { action: "rename", language: "typescript", file: "a.ts", line: 1, column: 10, new_name: "x" });
      expect(r.isError).toBe(true);
      expect(r.content[0].text).toContain("cannot rename this symbol");
      expect(writes.size).toBe(0);
    } finally {
      delete process.env.MOCK_LSP_RENAME_REJECT;
    }
  });

  it("服务器缺失：LSP_SERVER_MISSING + 环境页指引", async () => {
    const root = freshRoot();
    const { tool } = makeTool(root, { available: false });
    const r: any = await tool.execute("c1", { action: "hover", language: "typescript", file: "a.ts", line: 1, column: 1 });
    expect(r.isError).toBe(true);
    expect(r.details.errorCode).toBe("LSP_SERVER_MISSING");
    expect(r.content[0].text).toContain("Env Dependencies");
  });

  it("权限契约：读类=read；rename=write", () => {
    const root = freshRoot();
    const { tool } = makeTool(root);
    expect(tool.sessionPermission.resolveInvocation({ action: "definition" }).kind).toBe("read");
    expect(tool.sessionPermission.resolveInvocation({}).kind).toBe("read");
    expect(tool.sessionPermission.resolveInvocation({ action: "rename" }).kind).toBe("write");
  });
});

describe("lsp 辅助", () => {
  it("文件扩展名路由语言", () => {
    expect(lspLanguageForFile("/x/a.tsx")?.languageId).toBe("typescript");
    expect(lspLanguageForFile("/x/m.py")?.languageId).toBe("python");
    expect(lspLanguageForFile("/x/m.rs")?.languageId).toBe("rust");
    expect(lspLanguageForFile("/x/m.txt")).toBeNull();
  });

  it("summarizeLocations 兼容 Location 与 LocationLink", () => {
    const out = summarizeLocations([
      { uri: "file:///a.ts", range: { start: { line: 0, character: 2 } } },
      { targetUri: "file:///b.ts", targetRange: { start: { line: 7, character: 0 } } },
    ]);
    expect(out).toEqual([
      { file: "/a.ts", line: 1, column: 3 },
      { file: "/b.ts", line: 8, column: 1 },
    ]);
  });
});
