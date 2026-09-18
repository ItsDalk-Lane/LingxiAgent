/**
 * ast-grep-tool.ts — 结构化模式搜索（阶段二·7）。
 *
 * 用托管/PATH 上的 sg 二进制做 AST 级模式搜索（如「所有名为 X 的函数」），
 * 输出三模式 + 翻页（照 grep-pager 的语义：content / files / count、
 * offset 分页、footer 诚实报告「可能还有」）。只读工具；二进制缺失时
 * 如实报错并指路环境依赖页，不静默降级。
 *
 * sg CLI：`sg run --pattern P --lang L --json=compact`（默认尊重
 * .gitignore）。JSON 条目含 file / range.start.line / text / language。
 */
import { execFile } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { promisify } from "node:util";
import { Type, StringEnum } from "../pi-sdk/index.ts";
import { ensureAstGrepBinary } from "./ast-grep-binary.ts";

const execFileAsync = promisify(execFile);

const MAX_SCAN_MATCHES = 1000;
const MAX_FILES_LISTED = 100;
const DEFAULT_CONTENT_LIMIT = 100;
const RUN_TIMEOUT_MS = 30_000;

const MISSING_BINARY_HINT = [
  "ast-grep (sg) is not available and could not be downloaded automatically.",
  "Install it manually to use ast_grep / ast_edit:",
  "- macOS: brew install ast-grep",
  "- Windows: winget install ast-grep",
  "- any: cargo install ast-grep",
  "The status also shows up on the settings → 环境依赖 (Env Dependencies) page.",
].join("\n");

export interface AstGrepMatch {
  file: string;
  startLine: number;
  endLine: number;
  text: string;
  language: string;
}

/** 解析 sg --json=compact 输出；形状不符的条目跳过（不猜）。 */
export function parseAstGrepJsonMatches(raw: string): AstGrepMatch[] {
  let parsed: any;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return [];
  }
  if (!Array.isArray(parsed)) return [];
  const matches: AstGrepMatch[] = [];
  for (const entry of parsed) {
    if (!entry || typeof entry !== "object") continue;
    const file = typeof entry.file === "string" ? entry.file : "";
    const startLine = Number(entry?.range?.start?.line);
    const endLine = Number(entry?.range?.end?.line);
    if (!file || !Number.isInteger(startLine) || !Number.isInteger(endLine)) continue;
    matches.push({
      file,
      startLine,
      endLine,
      text: typeof entry.text === "string" ? entry.text : "",
      language: typeof entry.language === "string" ? entry.language : "",
    });
  }
  return matches;
}

async function runSg(sgPath: string, args: string[], timeoutMs: number, cwd: string): Promise<{ stdout: string; timedOut: boolean; stderr: string }> {
  return new Promise((resolve) => {
    let child: any;
    try {
      // cwd 必传：sg run 搜的是进程工作目录，漏传就会搜到调用方进程的仓库根。
      child = execFile(sgPath, args, { timeout: timeoutMs, maxBuffer: 16 * 1024 * 1024, cwd }, (err: any, stdout: any, stderr: any) => {
        if (err && err.killed) {
          resolve({ stdout: String(stdout || ""), timedOut: true, stderr: String(stderr || "") });
          return;
        }
        if (err) {
          resolve({ stdout: "", timedOut: false, stderr: String(stderr || err.message) });
          return;
        }
        resolve({ stdout: String(stdout || ""), timedOut: false, stderr: String(stderr || "") });
      });
      void child;
    } catch (err) {
      resolve({ stdout: "", timedOut: false, stderr: String((err as any)?.message || err) });
    }
  });
}

function buildRunArgs(params: any, extra: string[]): string[] {
  const args = ["run", "--pattern", String(params.pattern), "--lang", String(params.language)];
  if (Array.isArray(params.globs)) {
    for (const glob of params.globs) {
      if (typeof glob === "string" && glob.trim()) args.push("--globs", glob.trim());
    }
  }
  if (Array.isArray(params.files)) {
    for (const file of params.files) {
      if (typeof file === "string" && file.trim()) args.push(file.trim());
    }
  }
  args.push(...extra);
  return args;
}

export function formatAstGrepContentPage(matches: AstGrepMatch[], offset: number, limit: number): { text: string; total: number } {
  const lines = matches.map((m) => `${m.file}:${m.startLine}: ${m.text.replace(/\s+/g, " ").slice(0, 200)}`);
  const page = lines.slice(offset, offset + limit);
  const parts: string[] = page;
  if (offset + limit < lines.length) {
    parts.push("", `... ${lines.length - offset - limit} more matches — use offset=${offset + limit} to see the next page`);
  }
  return { text: parts.join("\n"), total: lines.length };
}

export function formatAstGrepFilesPage(matches: AstGrepMatch[], cwd: string): string {
  const files = new Map<string, number>();
  for (const m of matches) files.set(m.file, (files.get(m.file) || 0) + 1);
  const rows = [...files.entries()]
    .map(([file, count]) => {
      let mtime = 0;
      try { mtime = fs.statSync(path.join(cwd, file)).mtimeMs; } catch { /* 外部/已删文件排最后 */ }
      return { file, count, mtime };
    })
    .sort((a, b) => b.mtime - a.mtime)
    .slice(0, MAX_FILES_LISTED)
    .map(({ file, count }) => `${file} (${count})`);
  return rows.join("\n");
}

export function formatAstGrepCountPage(matches: AstGrepMatch[], truncated: boolean): string {
  const counts = new Map<string, number>();
  for (const m of matches) counts.set(m.file, (counts.get(m.file) || 0) + 1);
  const rows = [...counts.entries()].sort((a, b) => b[1] - a[1]).map(([file, count]) => `${file}: ${count}`);
  const total = matches.length;
  const header = truncated
    ? `total: ${total}+ (scan cap ${MAX_SCAN_MATCHES} reached — count is a lower bound)`
    : `total: ${total}`;
  return [header, ...rows].join("\n");
}

/**
 * @param {string} cwd  搜索根目录
 * @param {object} options
 * @param {string} options.managedBinDir  托管二进制目录（{lingxiHome}/runtime/pi-sdk/bin）
 * @param {boolean} [options.offline]  PI_OFFLINE 等离线标记：跳过下载
 */
export function createAstGrepTool(cwd: string, options: { managedBinDir?: string | null; offline?: boolean; log?: { warn?: (msg: string) => void } } = {}) {
  return {
    name: "ast_grep",
    description: "Search code by AST pattern (structural search) with ast-grep syntax, e.g. pattern '$F($$$ARGS)' with language 'js' finds all function calls named F. Use for queries where text grep is too loose: all usages of a function, all functions of a shape, catch blocks with a given body. Results are paginated with offset; output_mode picks matches / file list / per-file counts.",
    parameters: Type.Object({
      pattern: Type.String({ description: "ast-grep pattern, e.g. 'function $NAME($$$) { $$$ }' or '$OBJ.$METHOD($$$)'" }),
      language: Type.String({ description: "Language id for parsing, e.g. ts, tsx, js, py, go, rs, java, c, cpp, cs, ruby, swift, kotlin, scala, html, css" }),
      globs: Type.Optional(Type.Array(Type.String(), { description: "Optional glob filters, e.g. ['src/**/*.ts']" })),
      files: Type.Optional(Type.Array(Type.String(), { description: "Optional explicit file paths (relative to cwd) to search instead of the whole tree" })),
      output_mode: Type.Optional(StringEnum(["content", "files", "count"], { description: "content: file:line: snippet pages (default); files: matched files with counts, newest first; count: per-file match counts" })),
      offset: Type.Optional(Type.Number({ description: "0-based page offset for content mode (default 0)" })),
      limit: Type.Optional(Type.Number({ description: "Page size for content mode (default 100)" })),
    }),
    sessionPermission: {
      resolveInvocation: (input: any = {}) => ({
        action: "search",
        kind: "read",
        capability: "ast_grep.search",
      }),
    },
    async execute(_toolCallId: string, params: any = {}, ..._rest: any[]) {
      if (typeof params?.pattern !== "string" || !params.pattern.trim()
        || typeof params?.language !== "string" || !params.language.trim()) {
        return { content: [{ type: "text", text: "pattern and language are required" }] };
      }
      const sg = await ensureAstGrepBinary({ managedBinDir: options.managedBinDir, offline: options.offline, log: options.log });
      if (!sg) {
        return { isError: true, content: [{ type: "text", text: MISSING_BINARY_HINT }] };
      }

      const mode = params.output_mode === "files" || params.output_mode === "count" ? params.output_mode : "content";
      const offset = Number.isInteger(params.offset) && (params.offset as number) >= 0 ? params.offset as number : 0;
      const limit = Number.isInteger(params.limit) && (params.limit as number) > 0
        ? Math.min(params.limit as number, MAX_SCAN_MATCHES)
        : DEFAULT_CONTENT_LIMIT;

      const { stdout, timedOut, stderr } = await runSg(sg.path, buildRunArgs(params, ["--json=compact"]), RUN_TIMEOUT_MS, cwd);
      if (timedOut) {
        return {
          isError: true,
          content: [{ type: "text", text: `ast-grep timed out after ${RUN_TIMEOUT_MS / 1000}s (large tree?). Narrow the scope with globs or files, then retry.` }],
          details: { errorCode: "AST_GREP_TIMEOUT" },
        };
      }
      const matches = parseAstGrepJsonMatches(stdout);
      if (!matches.length) {
        if (stderr.trim()) {
          return { content: [{ type: "text", text: `no matches (ast-grep note: ${stderr.trim().split("\n")[0].slice(0, 300)})` }], details: { matchCount: 0 } };
        }
        return { content: [{ type: "text", text: "no matches" }], details: { matchCount: 0 } };
      }
      const truncated = matches.length >= MAX_SCAN_MATCHES;

      if (mode === "files") {
        return {
          content: [{ type: "text", text: formatAstGrepFilesPage(matches, cwd) }],
          details: { matchCount: matches.length, fileCount: new Set(matches.map((m) => m.file)).size, outputMode: mode },
        };
      }
      if (mode === "count") {
        return {
          content: [{ type: "text", text: formatAstGrepCountPage(matches, truncated) }],
          details: { matchCount: matches.length, truncated, outputMode: mode },
        };
      }
      const page = formatAstGrepContentPage(matches, offset, limit);
      return {
        content: [{ type: "text", text: page.text }],
        details: { matchCount: matches.length, pageReturned: Math.min(limit, Math.max(0, matches.length - offset)), offset, limit, truncated, outputMode: mode },
      };
    },
  };
}
