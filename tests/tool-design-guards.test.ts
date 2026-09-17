import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { createFileFreshnessTracker } from "../lib/sandbox/file-freshness.ts";
import {
  wrapEditToolWithErrorHints,
  wrapFileToolWithPathSuggestions,
  wrapMutationToolWithFreshness,
  wrapReadToolWithFreshness,
} from "../lib/sandbox/file-tool-guards.ts";
import { wrapGrepToolWithModes } from "../lib/sandbox/grep-pager.ts";
import { createExecCommandTools } from "../lib/exec-command/tool.ts";
import { createWebFetchTool } from "../lib/tools/web-fetch.ts";
import { createWebSearchTool } from "../lib/tools/web-search.ts";

let tmpDir: string;

beforeEach(() => {
  tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "tool-design-guards-"));
});

afterEach(() => {
  fs.rmSync(tmpDir, { recursive: true, force: true });
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

function textOf(result: any) {
  return (result?.content || []).map((item: any) => item?.text || "").join("\n");
}

function makeCtx(cwd: string) {
  return { sessionManager: { getCwd: () => cwd } };
}

// ── exec_command：默认超时 / 钳制 / 错误哲学 / 保头保尾 ──

describe("exec_command timeout and error philosophy", () => {
  it("defaults timeout to 120s and clamps to 600s; accepts string numbers", async () => {
    const seen: any[] = [];
    const bashTool = {
      execute: vi.fn(async (_id: string, params: any) => {
        seen.push(params.timeout);
        return { content: [{ type: "text", text: "ok" }] };
      }),
    };
    const [execCommand] = createExecCommandTools({ bashTool, getCwd: () => tmpDir, platform: "linux" });
    await execCommand.execute("a", { cmd: "echo hi" }, null, null, makeCtx(tmpDir));
    await execCommand.execute("b", { cmd: "echo hi", timeout: "90" }, null, null, makeCtx(tmpDir));
    await execCommand.execute("c", { cmd: "echo hi", timeout: 5000 }, null, null, makeCtx(tmpDir));
    expect(seen).toEqual([120, 90, 600]);
  });

  it("reports timeouts as normal output with a teaching footer, not isError", async () => {
    const bashTool = {
      execute: vi.fn(async () => {
        throw new Error("partial output\n\nCommand timed out after 120 seconds");
      }),
    };
    const [execCommand] = createExecCommandTools({ bashTool, getCwd: () => tmpDir, platform: "linux" });
    const result: any = await execCommand.execute("t", { cmd: "sleep forever" }, null, null, makeCtx(tmpDir));
    expect(result.isError).toBeUndefined();
    expect(result.content[0].text).toContain("Command timed out after 120 seconds (default timeout)");
    expect(result.content[0].text).toContain("For long-running work pass timeout=<seconds> (max 600)");
    expect(result.content[0].text).toContain("tty=true");
    expect(result.details.execCommand).toMatchObject({
      ok: false,
      errorCode: "EXEC_COMMAND_TIMEOUT",
      isError: false,
    });
  });

  it("keeps plain non-zero exits as normal output; aborts stay errors", async () => {
    const bashTool = {
      execute: vi.fn()
        .mockRejectedValueOnce(new Error("boom\n\nCommand exited with code 2"))
        .mockRejectedValueOnce(new Error("Command aborted")),
    };
    const [execCommand] = createExecCommandTools({ bashTool, getCwd: () => tmpDir, platform: "linux" });
    const failed: any = await execCommand.execute("f", { cmd: "false" }, null, null, makeCtx(tmpDir));
    expect(failed.isError).toBeUndefined();
    expect(failed.details.execCommand).toMatchObject({ ok: false, exitCode: 2, errorCode: "EXEC_COMMAND_EXIT_NONZERO" });
    const aborted: any = await execCommand.execute("a", { cmd: "yes" }, null, null, makeCtx(tmpDir));
    expect(aborted.isError).toBe(true);
  });

  it("truncates final output head+tail with an exact omission marker and spill path", async () => {
    const commandExec = vi.fn(async (_command: string, _cwd: string, opts: any = {}) => {
      for (let i = 1; i <= 2100; i++) {
        opts.onData(Buffer.from(`L${i}\n`, "utf-8"));
      }
      return { exitCode: 0 };
    });
    const [execCommand] = createExecCommandTools({
      bashTool: { execute: vi.fn() },
      commandExec,
      getCwd: () => tmpDir,
      platform: "win32",
    });
    const result: any = await execCommand.execute("big", { cmd: "type big.log", max_output_tokens: 6000 }, null, null, makeCtx(tmpDir));
    const text = textOf(result);
    expect(text).toContain("L1\n");
    expect(text).toContain("L2100");
    expect(text).toMatch(/\[\.\.\. \d+ lines \/ .+ omitted \.\.\.\]/);
    expect(text).toContain("Showing first ");
    expect(text).toContain("of 2101 lines. Full output: ");
    expect(result.details.truncation).toMatchObject({ truncated: true, truncatedBy: "head_tail" });
    expect(result.details.fullOutputPath).toBeTruthy();
    expect(fs.existsSync(result.details.fullOutputPath)).toBe(true);
  });
});

// ── 新鲜度：陈旧写拦截 + 紧邻重复读去重 ──

describe("file freshness guards", () => {
  it("blocks edit/write when the file changed since it was observed", async () => {
    const file = path.join(tmpDir, "note.txt");
    fs.writeFileSync(file, "v1");
    const tracker = createFileFreshnessTracker();
    const inner = { execute: vi.fn(async () => ({ content: [{ type: "text", text: "edited" }] })) };
    const tool = wrapMutationToolWithFreshness(inner, { tracker, getSessionPath: () => "s1", cwd: tmpDir });

    await tracker.observeRead("s1", file, JSON.stringify({ path: file, offset: null, limit: null }));
    fs.writeFileSync(file, "v2 changed externally");
    const result: any = await tool.execute("e1", { path: file }, null, null, makeCtx(tmpDir));
    expect(result.isError).toBe(true);
    expect(result.details.errorCode).toBe("FILE_STALE_SINCE_READ");
    expect(result.content[0].text).toContain("Re-read the file");
    expect(inner.execute).not.toHaveBeenCalled();
  });

  it("does not trip on its own writes and allows unobserved files", async () => {
    const file = path.join(tmpDir, "a.txt");
    fs.writeFileSync(file, "seed");
    const tracker = createFileFreshnessTracker();
    const inner = { execute: vi.fn(async () => ({ content: [{ type: "text", text: "done" }] })) };
    const tool = wrapMutationToolWithFreshness(inner, { tracker, getSessionPath: () => "s1", cwd: tmpDir });

    // 从未读过的文件直接改：放行（不做 read-before-edit 强制）。
    const first: any = await tool.execute("w1", { path: file }, null, null, makeCtx(tmpDir));
    expect(first.isError).toBeUndefined();
    // 自己刚写完接着改：不误报陈旧。
    const second: any = await tool.execute("w2", { path: file }, null, null, makeCtx(tmpDir));
    expect(second.isError).toBeUndefined();
    expect(inner.execute).toHaveBeenCalledTimes(2);
  });

  it("returns a stub for immediate identical re-reads and real content otherwise", async () => {
    const file = path.join(tmpDir, "r.txt");
    fs.writeFileSync(file, "stable content");
    const tracker = createFileFreshnessTracker();
    const inner = { execute: vi.fn(async () => ({ content: [{ type: "text", text: "stable content" }] })) };
    const tool = wrapReadToolWithFreshness(inner, { tracker, getSessionPath: () => "s1", cwd: tmpDir });

    const first: any = await tool.execute("r1", { path: file }, null, null, makeCtx(tmpDir));
    expect(first.content[0].text).toBe("stable content");
    const second: any = await tool.execute("r2", { path: file }, null, null, makeCtx(tmpDir));
    expect(second.details.duplicateRead).toBe(true);
    expect(second.content[0].text).toContain("Duplicate read");
    expect(second.content[0].text).toContain("different offset or limit");
    // 参数不同 → 正常重读。
    const third: any = await tool.execute("r3", { path: file, offset: 2 }, null, null, makeCtx(tmpDir));
    expect(third.details?.duplicateRead).toBeUndefined();
    expect(third.content[0].text).toBe("stable content");
  });
});

// ── 编辑报错定位：行号清单 / 最近匹配 / 易混字符 ──

describe("edit error hints", () => {
  function makeFile(name: string, content: string) {
    const file = path.join(tmpDir, name);
    fs.writeFileSync(file, content);
    return file;
  }

  it("lists occurrence line numbers for ambiguous matches", async () => {
    const file = makeFile("dup.txt", "alpha\nkeep\nalpha\nkeep\nkeep\nalpha\n");
    const inner = {
      execute: vi.fn(async () => ({
        isError: true,
        content: [{ type: "text", text: `Found 3 occurrences of the text in ${JSON.stringify(file)}. The text must be unique. Please provide more context to make it unique.` }],
      })),
    };
    const tool = wrapEditToolWithErrorHints(inner, { cwd: tmpDir });
    const result: any = await tool.execute("e", { path: file, edits: [{ oldText: "alpha", newText: "beta" }] }, null, null, makeCtx(tmpDir));
    expect(result.isError).toBe(true);
    expect(result.content[0].text).toContain("start at lines [1, 3, 6]");
    expect(result.content[0].text).toContain("Include more surrounding lines");
  });

  it("flags typographic variants when normalization would match", async () => {
    const file = makeFile("smart.txt", "intro\nit\u2019s a \u201cquoted\u201d name \u2014 here\noutro\n");
    const inner = {
      execute: vi.fn(async () => ({
        isError: true,
        content: [{ type: "text", text: `Could not find the exact text in ${JSON.stringify(file)}. The old text must match exactly including all whitespace and newlines.` }],
      })),
    };
    const tool = wrapEditToolWithErrorHints(inner, { cwd: tmpDir });
    const result: any = await tool.execute("e", {
      path: file,
      edits: [{ oldText: "it's a \"quoted\" name - here", newText: "x" }],
    }, null, null, makeCtx(tmpDir));
    expect(result.content[0].text).toContain("typographic variants");
    expect(result.content[0].text).toContain("line 2");
  });

  it("offers a nearest match line when nothing normalizes", async () => {
    const file = makeFile("near.txt", "const alphaValue = compute(1);\nconst other = 2;\n");
    const inner = {
      execute: vi.fn(async () => ({
        isError: true,
        content: [{ type: "text", text: `Could not find the exact text in ${JSON.stringify(file)}. The old text must match exactly including all whitespace and newlines.` }],
      })),
    };
    const tool = wrapEditToolWithErrorHints(inner, { cwd: tmpDir });
    const result: any = await tool.execute("e", {
      path: file,
      edits: [{ oldText: "const alphaValue = compute(2);", newText: "x" }],
    }, null, null, makeCtx(tmpDir));
    expect(result.content[0].text).toContain("Nearest match: line 1");
  });
});

// ── 路径建议 ──

describe("path suggestions", () => {
  it("appends did-you-mean candidates to read failures", async () => {
    const real = path.join(tmpDir, "config.json");
    fs.writeFileSync(real, "{}");
    const inner = {
      execute: vi.fn(async () => {
        throw Object.assign(new Error(`ENOENT: no such file or directory, open '${path.join(tmpDir, "confg.json")}'`), { code: "ENOENT" });
      }),
    };
    const tool = wrapFileToolWithPathSuggestions(inner, { cwd: tmpDir });
    await expect(tool.execute("r", { path: path.join(tmpDir, "confg.json") }, null, null, makeCtx(tmpDir)))
      .rejects
      .toThrow(/Did you mean "config\.json"/);
  });
});

// ── grep 输出模式 / 翻页 ──

function makeGrepInner(lines: string[], { matchLimitReached = false } = {}) {
  return {
    name: "grep",
    description: "inner",
    parameters: { properties: { pattern: {}, path: {}, glob: {}, ignoreCase: {}, literal: {}, context: {}, limit: {} } },
    // 模拟 pi grep：尊重 limit，达到上限时置 matchLimitReached。
    execute: vi.fn(async (_id: string, params: any = {}) => {
      const limit = Number.isFinite(Number(params.limit)) ? Number(params.limit) : 100;
      const capped = lines.length > limit;
      const shown = lines.slice(0, limit);
      return {
        content: [{ type: "text", text: shown.join("\n") }],
        details: {
          matchLimitReached: matchLimitReached || capped,
          truncation: capped ? { truncated: true } : undefined,
        },
      };
    }),
  };
}

describe("grep modes and pagination", () => {
  it("derives a newest-first file list for output_mode=files", async () => {
    const older = path.join(tmpDir, "older.ts");
    const newer = path.join(tmpDir, "newer.ts");
    fs.writeFileSync(older, "x");
    fs.writeFileSync(newer, "x");
    await new Promise((resolve) => setTimeout(resolve, 20));
    fs.utimesSync(older, new Date(), new Date(Date.now() - 60_000));
    const inner = makeGrepInner([
      `${older}:1: alpha`,
      `${newer}:2: alpha`,
    ]);
    const tool = wrapGrepToolWithModes(inner);
    const result: any = await tool.execute("g", { pattern: "alpha", output_mode: "files" }, null, null, makeCtx(tmpDir));
    const text = textOf(result);
    expect(text).toContain("Found 2 matching files (newest first)");
    const order = text.indexOf("newer.ts");
    expect(order).toBeGreaterThan(-1);
    expect(text.indexOf("older.ts")).toBeGreaterThan(order);
  });

  it("counts matches per file with an honest total for output_mode=count", async () => {
    const inner = makeGrepInner([
      `${tmpDir}/a.ts:1: x`,
      `${tmpDir}/a.ts:5: x`,
      `${tmpDir}/b.ts:9: x`,
    ]);
    const tool = wrapGrepToolWithModes(inner);
    const result: any = await tool.execute("g", { pattern: "x", output_mode: "count" }, null, null, makeCtx(tmpDir));
    const text = textOf(result);
    expect(text).toContain("Found 3 total matches across 2 files.");
    expect(text).toContain(`${path.join(tmpDir, "a.ts")}: 2`);
    expect(text).toContain(`${path.join(tmpDir, "b.ts")}: 1`);
  });

  it("pages content matches with offset and an actionable footer", async () => {
    const lines = Array.from({ length: 5 }, (_, i) => `${tmpDir}/f.ts:${i + 1}: hit`);
    const inner = makeGrepInner(lines);
    const tool = wrapGrepToolWithModes(inner);
    const result: any = await tool.execute("g", { pattern: "hit", limit: 2, offset: 2 }, null, null, makeCtx(tmpDir));
    expect(inner.execute).toHaveBeenCalledWith("g", expect.objectContaining({ limit: 4 }), null, null, expect.anything());
    const text = textOf(result);
    expect(text).toContain(`${tmpDir}/f.ts:3: hit`);
    expect(text).not.toContain(":1: hit");
    expect(text).toContain("Showing matches 3-4");
    expect(text).toContain("offset=4");
  });

  it("suggests similar paths when the search path is missing", async () => {
    fs.writeFileSync(path.join(tmpDir, "src"), "");
    const inner = makeGrepInner([]);
    inner.execute.mockImplementation(async () => {
      throw new Error(`Path not found: ${path.join(tmpDir, "srcs")}`);
    });
    const tool = wrapGrepToolWithModes(inner);
    await expect(tool.execute("g", { pattern: "x", path: path.join(tmpDir, "srcs") }, null, null, makeCtx(tmpDir)))
      .rejects
      .toThrow(/Did you mean "src"/);
  });
});

// ── web_search / web_fetch：诚实计数与错误姿势 ──

describe("web tool honesty and error posture", () => {
  it("shows an honest count and caps snippet length; empty results stay normal", async () => {
    const results = Array.from({ length: 15 }, (_, i) => ({
      title: `T${i}`,
      url: `https://example.com/${i}`,
      content: "x".repeat(600),
    }));
    vi.stubGlobal("fetch", vi.fn(async () => ({
      ok: true,
      status: 200,
      headers: new Map(),
      json: async () => ({ data: { results } }),
      text: async () => JSON.stringify({ data: { results } }),
    })));
    const tool: any = createWebSearchTool({});
    const result: any = await tool.execute("s", { query: "plain keywords" }, null, null, makeCtx(tmpDir));
    expect(result.isError).toBeUndefined();
    const text = textOf(result);
    expect(text).toContain("Showing 10 of 15 results. Increase maxResults to see more.");
    expect(text).toContain("x".repeat(500));
    expect(text).toContain("…");
    expect(text).not.toContain("x".repeat(501));
  });

  it("marks provider failures as errors with machine codes", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => ({ ok: false, status: 429, headers: new Map([["retry-after", "30"]]) })));
    const tool: any = createWebSearchTool({
      searchConfigResolver: () => ({ provider: "tavily", api_key: "key" }),
    });
    const result: any = await tool.execute("s", { query: "anything" }, null, null, makeCtx(tmpDir));
    expect(result.isError).toBe(true);
    expect(result.details.errorCode).toBe("WEB_SEARCH_RATE_LIMITED");
    expect(result.details.retryAfterMs).toBe(30_000);
  });

  it("spills truncated fetches to a file with a recovery hint; HTTP failures are errors", async () => {
    const payload = "y".repeat(30_000);
    vi.stubGlobal("fetch", vi.fn(async () => ({
      ok: true,
      status: 200,
      headers: { get: () => "text/plain" },
      text: async () => payload,
    })));
    const tool: any = createWebFetchTool();
    const result: any = await tool.execute("f", { url: "http://93.184.216.34/data.txt" }, null, null, makeCtx(tmpDir));
    expect(result.isError).toBeUndefined();
    const text = textOf(result);
    expect(text).toContain("[Truncated: showing first 12000 of 30000 characters.");
    const match = text.match(/Full content saved to (\S+) — read it with the read tool\./);
    expect(match).toBeTruthy();
    expect(fs.readFileSync(match![1], "utf-8")).toBe(payload);

    vi.stubGlobal("fetch", vi.fn(async () => ({
      ok: false,
      status: 404,
      statusText: "Not Found",
      headers: { get: () => "" },
      text: async () => "",
    })));
    const missing: any = await tool.execute("f2", { url: "http://93.184.216.34/missing.txt" }, null, null, makeCtx(tmpDir));
    expect(missing.isError).toBe(true);
    expect(missing.details.errorCode).toBe("WEB_FETCH_HTTP_404");
  });
});
