import { PassThrough } from "stream";
import { EventEmitter } from "events";
import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { spawn, spawnSync } = vi.hoisted(() => ({
  spawn: vi.fn(),
  spawnSync: vi.fn(),
}));

vi.mock("child_process", () => ({
  spawn,
  spawnSync,
}));

vi.mock("node:child_process", () => ({
  spawn,
  spawnSync,
}));

vi.mock("@earendil-works/pi-coding-agent", () => ({
  createAgentSession: vi.fn(),
  ModelRegistry: class {},
  SessionManager: class {},
  SettingsManager: class {},
  createReadTool: vi.fn(),
  createWriteTool: vi.fn(),
  createEditTool: vi.fn(),
  createBashTool: vi.fn(),
  createGrepTool: vi.fn(() => ({
    name: "grep",
    execute: vi.fn(async () => ({ content: [{ type: "text", text: "sdk passthrough" }] })),
  })),
  createFindTool: vi.fn(() => ({
    name: "find",
    execute: vi.fn(async () => ({ content: [{ type: "text", text: "sdk passthrough" }] })),
  })),
  createLsTool: vi.fn(),
  createGrepToolDefinition: vi.fn(() => ({
    name: "grep",
    label: "grep",
    description: "grep",
    parameters: {},
    execute: vi.fn(),
  })),
  createFindToolDefinition: vi.fn((_cwd, options: any = {}) => ({
    name: "find",
    label: "find",
    description: "find",
    parameters: {},
    execute: async (_toolCallId, { pattern, limit }, signal) => {
      const results = await options.operations.glob(pattern, process.cwd(), {
        ignore: [],
        limit: limit ?? 1000,
      });
      if (signal?.aborted) throw new Error("Operation aborted");
      return { content: [{ type: "text", text: results.join("\n") }] };
    },
  })),
  DefaultResourceLoader: class {},
  formatSkillsForPrompt: vi.fn(),
  getLastAssistantUsage: vi.fn(),
  AuthStorage: class {},
  estimateTokens: vi.fn(),
  findCutPoint: vi.fn(),
  generateSummary: vi.fn(),
  serializeConversation: vi.fn(),
  shouldCompact: vi.fn(),
  parseSessionEntries: vi.fn(),
  buildSessionContext: vi.fn(),
  resizeImage: vi.fn(),
  formatDimensionNote: vi.fn(),
  convertToLlm: vi.fn(),
  DEFAULT_MAX_BYTES: 50 * 1024,
  formatSize: (bytes) => `${(bytes / 1024).toFixed(1)}KB`,
  truncateHead: (content) => ({
    content,
    truncated: false,
    maxBytes: 50 * 1024,
  }),
  truncateLine: (line, maxChars = 500) => (
    line.length <= maxChars
      ? { text: line, wasTruncated: false }
      : { text: `${line.slice(0, maxChars)}... [truncated]`, wasTruncated: true }
  ),
}));

function createChildProcess({ stdout = "", stderr = "", code = 0 }: any = {}) {
  const child: any = new EventEmitter();
  child.stdout = new PassThrough();
  child.stderr = new PassThrough();
  child.killed = false;
  child.kill = vi.fn(() => {
    child.killed = true;
  });

  setImmediate(() => {
    if (stdout) child.stdout.write(stdout);
    child.stdout.end();
    if (stderr) child.stderr.write(stderr);
    child.stderr.end();
    child.emit("close", code);
  });

  return child;
}

describe("Hana Pi SDK search tools", () => {
  let tempRoot: string | null = null;

  function managedPaths() {
    if (!tempRoot) tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hana-search-tools-"));
    return {
      managedBinDir: path.join(tempRoot, "runtime", "pi-sdk", "bin"),
    };
  }

  beforeEach(() => {
    vi.clearAllMocks();
    spawnSync.mockReturnValue({ status: 0, stdout: "tool version\n", stderr: "" });
  });

  afterEach(() => {
    if (tempRoot) fs.rmSync(tempRoot, { recursive: true, force: true });
    tempRoot = null;
  });

  it("runs grep ripgrep with hidden Windows console windows", async () => {
    const { createGrepTool } = await import("../lib/pi-sdk/index.ts");
    const cwd = process.cwd();
    const match = {
      type: "match",
      data: {
        path: { text: `${cwd}/package.json` },
        line_number: 1,
        lines: { text: "{\n" },
      },
    };
    spawn.mockReturnValue(createChildProcess({ stdout: `${JSON.stringify(match)}\n` }));

    const tool = (createGrepTool as any)(cwd, {
      ...managedPaths(),
      operations: {
        isDirectory: () => true,
        readFile: () => "",
      },
    });

    await (tool as any).execute("call-1", { pattern: "name", path: "." });

    expect(spawn).toHaveBeenCalledWith(
      "rg",
      expect.any(Array),
      expect.objectContaining({
        stdio: ["ignore", "pipe", "pipe"],
        windowsHide: true,
      }),
    );
  });

  it("runs find fd with hidden Windows console windows", async () => {
    const { createFindTool } = await import("../lib/pi-sdk/index.ts");
    const cwd = process.cwd();
    spawn.mockReturnValue(createChildProcess({ stdout: `${cwd}/package.json\n` }));

    const tool = (createFindTool as any)(cwd, managedPaths());

    await (tool as any).execute("call-2", { pattern: "package.json", path: "." });

    expect(spawn).toHaveBeenCalledWith(
      "fd",
      expect.any(Array),
      expect.objectContaining({
        stdio: ["ignore", "pipe", "pipe"],
        windowsHide: true,
      }),
    );
  });

  it("requires an explicit absolute managed binary directory", async () => {
    const { createGrepTool, createFindTool } = await import("../lib/pi-sdk/index.ts");

    expect(() => (createGrepTool as any)(process.cwd(), {})).toThrow(
      "managedBinDir must be an absolute path",
    );
    expect(() => (createFindTool as any)(process.cwd(), { managedBinDir: "relative/bin" })).toThrow(
      "managedBinDir must be an absolute path",
    );
  });

  it("uses the managed binary from Hana's runtime directory when present", async () => {
    const { createGrepTool } = await import("../lib/pi-sdk/index.ts");
    const cwd = process.cwd();
    const paths = managedPaths();
    const binaryName = process.platform === "win32" ? "rg.exe" : "rg";
    const managedPath = path.join(paths.managedBinDir, binaryName);
    fs.mkdirSync(paths.managedBinDir, { recursive: true });
    fs.writeFileSync(managedPath, "managed-rg", "utf-8");

    spawn.mockReturnValue(createChildProcess());
    const tool = (createGrepTool as any)(cwd, {
      ...paths,
      operations: {
        isDirectory: () => true,
        readFile: () => "",
      },
    });

    await (tool as any).execute("call-prefer-managed", { pattern: "missing", path: "." });

    expect(spawn).toHaveBeenCalledWith(managedPath, expect.any(Array), expect.any(Object));
    expect(fs.readFileSync(managedPath, "utf-8")).toBe("managed-rg");
  });

  it("records grouped grep results with actual line numbers and context rows", async () => {
    const { createGrepTool } = await import("../lib/pi-sdk/index.ts");
    const cwd = process.cwd();
    const searchRoot = path.join(cwd, "nested");
    const events = [
      { type: "match", data: { path: { text: path.join(searchRoot, "a.txt") }, line_number: 2, lines: { text: "match\n" } } },
      { type: "match", data: { path: { text: path.join(searchRoot, "b.txt") }, line_number: 1, lines: { text: "another match\n" } } },
    ];
    spawn.mockReturnValue(createChildProcess({ stdout: events.map(event => JSON.stringify(event)).join("\n") + "\n" }));
    const tool = createGrepTool(cwd, {
      ...managedPaths(),
      operations: {
        isDirectory: () => true,
        readFile: filePath => filePath.endsWith("a.txt") ? "before\nmatch\nafter" : "another match\ntail",
      },
    });
    const result: any = await (tool as any).execute("grouped", { pattern: "match", path: "nested", context: 1 });
    expect(result.details.search).toEqual({
      kind: "grep", basePath: searchRoot, matchCount: 2, fileCount: 2, truncated: false,
      files: [
        { path: "a.txt", matches: [{ line: 1, text: "before", context: true }, { line: 2, text: "match" }, { line: 3, text: "after", context: true }] },
        { path: "b.txt", matches: [{ line: 1, text: "another match" }, { line: 2, text: "tail", context: true }] },
      ],
    });
  });

  it("records the parent directory as basePath when grep searches a single file", async () => {
    const { createGrepTool } = await import("../lib/pi-sdk/index.ts");
    const cwd = process.cwd();
    const filePath = path.join(cwd, "nested", "single.txt");
    const event = { type: "match", data: { path: { text: filePath }, line_number: 4, lines: { text: "match\n" } } };
    spawn.mockReturnValue(createChildProcess({ stdout: `${JSON.stringify(event)}\n` }));
    const tool = createGrepTool(cwd, {
      ...managedPaths(), operations: { isDirectory: () => false, readFile: () => "" },
    });
    const result: any = await (tool as any).execute("single-file", { pattern: "match", path: "nested/single.txt" });
    expect(result.details.search).toMatchObject({
      basePath: path.dirname(filePath), files: [{ path: "single.txt", matches: [{ line: 4, text: "match" }] }],
    });
  });

  it("bounds search metadata while retaining result counts and marking truncation", async () => {
    const { createGrepTool } = await import("../lib/pi-sdk/index.ts");
    const cwd = process.cwd();
    const events = Array.from({ length: 250 }, (_, index) => ({
      type: "match", data: { path: { text: path.join(cwd, "many.txt") }, line_number: index + 1, lines: { text: "x".repeat(490) + "\n" } },
    }));
    spawn.mockReturnValue(createChildProcess({ stdout: events.map(event => JSON.stringify(event)).join("\n") + "\n" }));
    const tool = createGrepTool(cwd, {
      ...managedPaths(), operations: { isDirectory: () => true, readFile: () => "" },
    });
    const result: any = await (tool as any).execute("bounded", { pattern: "x", limit: 300 });
    expect(result.details.search).toMatchObject({ matchCount: 250, fileCount: 1, truncated: true });
    expect(result.details.search.files[0].matches.length).toBeLessThan(250);
    expect(Buffer.byteLength(JSON.stringify(result.details.search))).toBeLessThan(60 * 1024);
  });

  it("records find paths and empty grep results without parsing the display notice", async () => {
    const { createFindTool, createGrepTool } = await import("../lib/pi-sdk/index.ts");
    const cwd = process.cwd();
    const searchRoot = path.join(cwd, "project");
    spawn.mockReturnValueOnce(createChildProcess({ stdout: `${searchRoot}/a.txt\n${searchRoot}/nested/b.txt\n` }));
    const find = createFindTool(cwd, managedPaths());
    const found: any = await (find as any).execute("find-details", { pattern: "*.txt", path: "project", limit: 2 });
    expect(found.details.search).toEqual({
      kind: "find", basePath: searchRoot, files: [{ path: "a.txt" }, { path: "nested/b.txt" }], fileCount: 2, truncated: true,
    });
    spawn.mockReturnValueOnce(createChildProcess({ code: 1 }));
    const grep = createGrepTool(cwd, {
      ...managedPaths(), operations: { isDirectory: () => true, readFile: () => "" },
    });
    const empty: any = await (grep as any).execute("empty", { pattern: "none" });
    expect(empty.details.search).toEqual({ kind: "grep", basePath: cwd, files: [], matchCount: 0, fileCount: 0, truncated: false });
  });
});
