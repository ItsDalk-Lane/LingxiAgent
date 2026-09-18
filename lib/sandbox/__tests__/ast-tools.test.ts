/**
 * ast_grep / ast_edit 工具行为测试。
 *
 * 二进制层（ensureAstGrepBinary）整体 mock 成固定 sg 路径 + 指定版本，
 * execFile mock 按 args 形状分派（--json=compact → 匹配 JSON；--update-all
 * → 重写成功）。覆盖：参数校验、缺二进制诚实报错、三模式+翻页 footer、
 * 超时、预览不动文件、apply 的新鲜度拦截/突变登记/fileChange 日记、
 * 权限契约（preview=read / apply=write）。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";

vi.mock("../ast-grep-binary.ts", () => ({
  ensureAstGrepBinary: vi.fn(async () => ({ path: "/fake/bin/sg", version: "0.39.4-test" })),
  resolveAstGrepBinary: vi.fn(() => "/fake/bin/sg"),
  astGrepManagedPath: (dir: string) => path.join(dir, "sg"),
  astGrepAssetName: vi.fn(() => "ast-grep-x86_64-apple-darwin.tar.gz"),
}));

const execFileMock = vi.hoisted(() => vi.fn());

vi.mock("node:child_process", () => ({
  execFile: execFileMock,
  spawnSync: vi.fn(() => ({ error: null, status: 0 })),
}));

import { execFile } from "node:child_process";
import { createAstGrepTool, parseAstGrepJsonMatches, formatAstGrepContentPage } from "../ast-grep-tool.ts";
import { createAstEditTool } from "../ast-edit-tool.ts";
import { createFileFreshnessTracker } from "../file-freshness.ts";

const MATCHES = [
  { file: "src/a.ts", range: { start: { line: 10 }, end: { line: 12 } }, text: "foo(1)", language: "ts" },
  { file: "src/a.ts", range: { start: { line: 40 }, end: { line: 40 } }, text: "foo(2)", language: "ts" },
  { file: "src/b.ts", range: { start: { line: 5 }, end: { line: 5 } }, text: "foo(3)", language: "ts" },
];

function okRun(stdout: string) {
  return (cmd: any, args: any, _opts: any, cb: any) => {
    void cmd; void args;
    cb(null, stdout, "");
  };
}

describe("ast_grep 工具", () => {
  beforeEach(() => {
    execFileMock.mockReset();
    execFileMock.mockImplementation(okRun(JSON.stringify(MATCHES)));
  });
  afterEach(() => { vi.clearAllMocks(); });

  const tool = createAstGrepTool("/repo", { managedBinDir: "/managed" });

  it("参数校验：缺 pattern/language 直接说明，不起子进程", async () => {
    const r = await tool.execute("c1", { language: "ts" });
    expect((r as any).content[0].text).toContain("required");
    expect(execFileMock).not.toHaveBeenCalled();
  });

  it("content 模式分页 + 诚实 footer", async () => {
    const r: any = await tool.execute("c1", { pattern: "foo($$$)", language: "ts", limit: 2 });
    expect(r.content[0].text).toContain("src/a.ts:10: foo(1)");
    expect(r.content[0].text).toContain("src/a.ts:40: foo(2)");
    expect(r.content[0].text).toContain("1 more matches — use offset=2");
    expect(r.details.matchCount).toBe(3);
  });

  it("files 模式：唯一文件 + 命中数", async () => {
    const r: any = await tool.execute("c1", { pattern: "foo($$$)", language: "ts", output_mode: "files" });
    expect(r.content[0].text).toContain("src/a.ts (2)");
    expect(r.content[0].text).toContain("src/b.ts (1)");
  });

  it("count 模式：总数 + 逐文件计数", async () => {
    const r: any = await tool.execute("c1", { pattern: "foo($$$)", language: "ts", output_mode: "count" });
    expect(r.content[0].text).toContain("total: 3");
    expect(r.content[0].text).toContain("src/a.ts: 2");
  });

  it("超时：AST_GREP_TIMEOUT 如实报错", async () => {
    execFileMock.mockImplementation((_c: any, _a: any, _o: any, cb: any) => {
      const err = new Error("killed") as any;
      err.killed = true;
      cb(err, "", "");
    });
    const r: any = await tool.execute("c1", { pattern: "p", language: "ts" });
    expect(r.isError).toBe(true);
    expect(r.details.errorCode).toBe("AST_GREP_TIMEOUT");
  });

  it("参数面：pattern/language 必填，globs/files 可选，翻页键齐全", () => {
    const keys = Object.keys((tool.parameters as any).properties);
    expect(keys).toContain("pattern");
    expect(keys).toContain("output_mode");
    expect(keys).toContain("offset");
  });

  it("权限契约：恒 read（计划模式可用）", () => {
    expect(tool.sessionPermission.resolveInvocation({})).toEqual({
      action: "search", kind: "read", capability: "ast_grep.search",
    });
  });
});

describe("ast_edit 工具", () => {
  const roots: string[] = [];
  beforeEach(() => {
    execFileMock.mockReset();
    execFileMock.mockImplementation(okRun(JSON.stringify(MATCHES)));
  });
  afterEach(() => {
    while (roots.length) fs.rmSync(roots.pop()!, { recursive: true, force: true });
  });

  function freshRoot() {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-ast-edit-"));
    roots.push(root);
    for (const f of ["src/a.ts", "src/b.ts"]) {
      fs.mkdirSync(path.join(root, path.dirname(f)), { recursive: true });
      fs.writeFileSync(path.join(root, f), "let x = foo(1);\n");
    }
    return root;
  }

  it("preview：不落 --update-all，正文声明未改文件", async () => {
    const root = freshRoot();
    execFileMock.mockImplementation((_c: any, args: any, _o: any, cb: any) => {
      if (args.includes("--update-all")) throw new Error("must not update in preview");
      if (args.includes("--json=compact")) return cb(null, JSON.stringify(MATCHES), "");
      return cb(null, "diff --git a/src/a.ts\n-foo(1)\n+bar(1)", "");
    });
    const tool = createAstEditTool(root, { managedBinDir: "/managed" });
    const r: any = await tool.execute("c1", { pattern: "foo($$$)", rewrite: "bar($$$)", language: "ts" });
    expect(r.content[0].text).toContain("no files were modified");
    expect(r.content[0].text).toContain("action=apply");
    expect(r.details.matchFiles).toEqual(["src/a.ts", "src/b.ts"]);
  });

  it("apply：过新鲜度守卫后重写并登记突变与日记", async () => {
    const root = freshRoot();
    const tracker = createFileFreshnessTracker();
    const recordFileOperation = vi.fn();
    const withFileChangeCapture = vi.fn(async (_kind: string, execute: any) => execute());
    execFileMock.mockImplementation((_c: any, args: any, opts: any, cb: any) => {
      if (args.includes("--json=compact")) return cb(null, JSON.stringify(MATCHES), "");
      // --update-all 分支模拟真实重写：改内容 + 显式推进 mtime（防同刻写粒度持平）
      for (const rel of ["src/a.ts", "src/b.ts"]) {
        const target = path.join(opts.cwd, rel);
        fs.writeFileSync(target, fs.readFileSync(target, "utf8").replace("foo", "bar"));
        const later = new Date(Date.now() + 5000);
        fs.utimesSync(target, later, later);
      }
      return cb(null, "done", "");
    });
    const tool = createAstEditTool(root, {
      managedBinDir: "/managed",
      tracker,
      getSessionPath: () => "/tmp/sess.jsonl",
      recordFileOperation,
      withFileChangeCapture,
    });
    const r: any = await tool.execute("c1", { action: "apply", pattern: "foo($$$)", rewrite: "bar($$$)", language: "ts" });
    expect(r.content[0].text).toContain("applied rewrite to 2 file(s) (3 match(es) scanned, verified by mtime)");
    expect(r.details.changedFiles).toEqual(["src/a.ts", "src/b.ts"]);
    expect(execFileMock.mock.calls.some((c: any[]) => c[1].includes("--update-all"))).toBe(true);
    expect(withFileChangeCapture).toHaveBeenCalled();
    expect(recordFileOperation).toHaveBeenCalledTimes(2);
    expect(recordFileOperation.mock.calls[0][0]).toMatchObject({ origin: "agent_ast_edit", operation: "modified" });
  });

  it("apply：读后被外部改动的文件被拒（FILE_STALE_SINCE_READ）", async () => {
    const root = freshRoot();
    // 登记「读」时 statFile 报真实指纹；之后（守卫复查时）对 a.ts 报漂移指纹，
    // 等价于「读过之后文件被外部改动」。
    const abs = path.join(root, "src/a.ts");
    const realStat = fs.statSync.bind(fs);
    let drifted = false;
    const statFile = vi.fn(async (p: string) => {
      const st = realStat(p);
      const size = p === abs && drifted ? st.size + 999 : st.size;
      return { isFile: () => true, mtimeMs: st.mtimeMs, size } as any;
    });
    const tracker = createFileFreshnessTracker({ statFile });
    await tracker.observeRead("/tmp/sess.jsonl", abs, "k1");
    drifted = true;
    const tool = createAstEditTool(root, {
      managedBinDir: "/managed",
      tracker,
      getSessionPath: () => "/tmp/sess.jsonl",
    });
    const r: any = await tool.execute("c1", { action: "apply", pattern: "foo($$$)", rewrite: "bar($$$)", language: "ts" });
    expect(r.isError).toBe(true);
    expect(r.details.errorCode).toBe("FILE_STALE_SINCE_READ");
    expect(r.details.staleFiles).toEqual(["src/a.ts"]);
    expect(execFileMock.mock.calls.some((c: any[]) => c[1].includes("--update-all"))).toBe(false);
  });

  it("apply：匹配扫描失败如实上抛 sg stderr", async () => {
    const root = freshRoot();
    execFileMock.mockImplementation((_c: any, args: any, _o: any, cb: any) => {
      if (args.includes("--json=compact")) {
        const err = new Error("fail") as any;
        err.code = 2;
        return cb(err, "", "pattern parse error");
      }
      return cb(null, "", "");
    });
    const tool = createAstEditTool(root, { managedBinDir: "/managed" });
    const r: any = await tool.execute("c1", { action: "apply", pattern: "(", rewrite: "x", language: "ts" });
    expect(r.isError).toBe(true);
    expect(r.content[0].text).toContain("pattern parse error");
  });

  it("权限契约：preview=read / apply=write", () => {
    const tool = createAstEditTool("/repo", { managedBinDir: "/managed" });
    expect(tool.sessionPermission.resolveInvocation({ action: "preview" }).kind).toBe("read");
    expect(tool.sessionPermission.resolveInvocation({ action: "apply" })).toEqual({
      action: "apply", kind: "routine", capability: "ast_edit.apply",
    });
    // 默认（无 action）按 preview 处置
    expect(tool.sessionPermission.resolveInvocation({}).kind).toBe("read");
  });
});

describe("parseAstGrepJsonMatches / formatAstGrepContentPage", () => {
  it("坏 JSON / 缺字段条目跳过，不猜", () => {
    expect(parseAstGrepJsonMatches("not json")).toEqual([]);
    expect(parseAstGrepJsonMatches('[{"file":"a.ts"}]')).toEqual([]);
    const good = parseAstGrepJsonMatches(JSON.stringify(MATCHES));
    expect(good).toHaveLength(3);
    expect(good[0]).toMatchObject({ file: "src/a.ts", startLine: 10, endLine: 12 });
  });

  it("末页无 footer，整页短文本原样", () => {
    const page = formatAstGrepContentPage([{ file: "a.ts", startLine: 1, endLine: 1, text: "x", language: "ts" }], 0, 10);
    expect(page.text).toBe("a.ts:1: x");
    expect(page.total).toBe(1);
  });
});
