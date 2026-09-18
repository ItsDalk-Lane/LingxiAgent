/**
 * ast-edit-tool.ts — 结构化重写（阶段二·7），两段式。
 *
 * action=preview（默认）：跑 sg 的 diff 输出（不动文件），保头保尾截断；
 * action=apply：先复扫匹配文件集 → 逐文件过「读后变更」新鲜度守卫（与
 * edit 同一张 tracker 表）→ sg --update-all 原地重写 → 逐文件 observeMutation
 * + fileChange 日记（与 write/edit 同一 recordFileOperation 通道）。
 * 应用与预览之间文件可能被用户改过：新鲜度守卫拦 stale，未登记读过的
 * 文件沿 edit 同语义放行（守卫防陈旧，不做首次读强制）。
 *
 * 权限：preview=read（计划模式可用）；apply=write（只读档被拒）。
 */
import { execFile } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { promisify } from "node:util";
import { Type, StringEnum } from "../pi-sdk/index.ts";
import { ensureAstGrepBinary } from "./ast-grep-binary.ts";
import { parseAstGrepJsonMatches } from "./ast-grep-tool.ts";
import { suggestSimilarPaths, formatDidYouMean } from "./path-suggestions.ts";
import type { FileFreshnessTracker } from "./file-freshness.ts";

const execFileAsync = promisify(execFile);

const RUN_TIMEOUT_MS = 60_000;
const PREVIEW_HEAD_LINES = 60;
const PREVIEW_TAIL_LINES = 20;

const MISSING_BINARY_HINT = [
  "ast-grep (sg) is not available and could not be downloaded automatically.",
  "Install it manually to use ast_grep / ast_edit (brew install ast-grep / winget install ast-grep / cargo install ast-grep).",
].join("\n");

function truncateDiffHeadTail(diff: string): string {
  const lines = diff.split("\n");
  if (lines.length <= PREVIEW_HEAD_LINES + PREVIEW_TAIL_LINES + 2) return diff;
  const head = lines.slice(0, PREVIEW_HEAD_LINES);
  const tail = lines.slice(-PREVIEW_TAIL_LINES);
  return [...head, `... (${lines.length - PREVIEW_HEAD_LINES - PREVIEW_TAIL_LINES} lines omitted)`, ...tail].join("\n");
}

function resolveWithin(pathValue: string, cwd: string): string {
  return path.isAbsolute(pathValue) ? pathValue : path.resolve(cwd, pathValue);
}

async function runSg(sgPath: string, args: string[], timeoutMs: number, cwd: string): Promise<{ stdout: string; stderr: string; timedOut: boolean; exitCode: number | null }> {
  return new Promise((resolve) => {
    try {
      // cwd 必传：sg run 的作用域是进程工作目录（= 本工具的沙盒根）。
      execFile(sgPath, args, { timeout: timeoutMs, maxBuffer: 16 * 1024 * 1024, cwd }, (err: any, stdout: any, stderr: any) => {
        if (err && err.killed) {
          resolve({ stdout: String(stdout || ""), stderr: String(stderr || ""), timedOut: true, exitCode: null });
          return;
        }
        resolve({
          stdout: String(stdout || ""),
          stderr: String(stderr || ""),
          timedOut: false,
          exitCode: typeof err?.code === "number" ? err.code : err ? 1 : 0,
        });
      });
    } catch (err) {
      resolve({ stdout: "", stderr: String((err as any)?.message || err), timedOut: false, exitCode: 1 });
    }
  });
}

export interface AstEditToolDeps {
  managedBinDir?: string | null;
  offline?: boolean;
  log?: { warn?: (msg: string) => void };
  tracker?: FileFreshnessTracker | null;
  getSessionPath?: () => string | null;
  recordFileOperation?: (entry: any) => any;
  /**
   * resourceOps.withFileChangeCapture —— 与 write/edit 同一条文件历史快照通道。
   * kind 沿既有联合（"write" | "edit"）：ast_edit 属 edit 档（同源文件改写）。
   */
  withFileChangeCapture?: (kind: "write" | "edit", execute: () => Promise<any>) => Promise<any>;
}

export function createAstEditTool(cwd: string, deps: AstEditToolDeps = {}) {
  return {
    name: "ast_edit",
    description: "Structural rewrite with ast-grep two-phase semantics: action=preview (default) shows the diff without touching files; action=apply rewrites in place. Metavariables must be NAMED to be spliced into the rewrite: pattern 'console.log($$$ARGS)' + rewrite 'logger.debug($$$ARGS)' produces logger.debug('hi'); an anonymous $$$ in the rewrite side is emitted literally, so never use it there. Always preview first, then apply. Apply refuses files modified since your last read (re-read, then retry). Scope with globs or explicit files.",
    parameters: Type.Object({
      action: Type.Optional(StringEnum(["preview", "apply"], { description: "preview: diff only (default). apply: rewrite files in place after freshness checks" })),
      pattern: Type.String({ description: "ast-grep pattern to match nodes to rewrite" }),
      rewrite: Type.String({ description: "Replacement text; may reference pattern metavariables like $VAR / $$$MULTI" }),
      language: Type.String({ description: "Language id (ts, tsx, js, py, go, rs, ...)" }),
      globs: Type.Optional(Type.Array(Type.String(), { description: "Optional glob filters narrowing the rewrite scope" })),
      files: Type.Optional(Type.Array(Type.String(), { description: "Optional explicit file paths (relative to cwd) to rewrite; recommended after previewing" })),
    }),
    sessionPermission: {
      resolveInvocation: (input: any = {}) => {
        if (input?.action === "apply") {
          return {
            action: "apply",
            kind: "routine",
            capability: "ast_edit.apply",
          };
        }
        return {
          action: "preview",
          kind: "read",
          capability: "ast_edit.preview",
        };
      },
    },
    async execute(_toolCallId: string, params: any = {}, ..._rest: any[]) {
      const action = params?.action === "apply" ? "apply" : "preview";
      if (typeof params?.pattern !== "string" || !params.pattern.trim()
        || typeof params?.rewrite !== "string"
        || typeof params?.language !== "string" || !params.language.trim()) {
        return { content: [{ type: "text", text: "pattern, rewrite and language are required" }] };
      }
      const sg = await ensureAstGrepBinary({ managedBinDir: deps.managedBinDir, offline: deps.offline, log: deps.log });
      if (!sg) {
        return { isError: true, content: [{ type: "text", text: MISSING_BINARY_HINT }] };
      }

      // ── 匹配文件集（预览与应用共用；也是新鲜度守卫的核对范围）──
      const matchRun = await runSg(sg.path, buildArgs(params, ["--json=compact"]), RUN_TIMEOUT_MS, cwd);
      if (matchRun.timedOut) {
        return {
          isError: true,
          content: [{ type: "text", text: `ast-grep match scan timed out after ${RUN_TIMEOUT_MS / 1000}s. Narrow the scope with globs or files, then retry.` }],
          details: { errorCode: "AST_EDIT_TIMEOUT", phase: "match" },
        };
      }
      if (matchRun.exitCode !== 0) {
        return {
          isError: true,
          content: [{ type: "text", text: `ast-grep failed: ${(matchRun.stderr || "unknown error").split("\n")[0].slice(0, 400)}` }],
          details: { errorCode: "AST_GREP_FAILED", phase: "match", stderr: matchRun.stderr.slice(0, 1000) },
        };
      }
      const matches = parseAstGrepJsonMatches(matchRun.stdout);
      const matchFiles = [...new Set(matches.map((m) => m.file))];
      if (!matchFiles.length) {
        return {
          content: [{ type: "text", text: "no matches — nothing to rewrite" }],
          details: { action, matchCount: 0 },
        };
      }

      // ── 显式 files 参数的路径存在性 + Did-you-mean ──
      const hints: string[] = [];
      if (Array.isArray(params.files)) {
        for (const raw of params.files) {
          if (typeof raw !== "string" || !raw.trim()) continue;
          const abs = resolveWithin(raw.trim(), cwd);
          if (!fs.existsSync(abs)) {
            const candidates = await suggestSimilarPaths(abs);
            hints.push(`file not found: ${raw}${candidates.length ? `\n${formatDidYouMean(candidates, path.dirname(abs))}` : ""}`);
          }
        }
      }

      if (action === "preview") {
        const diffRun = await runSg(sg.path, buildArgs(params, ["--rewrite", String(params.rewrite)]), RUN_TIMEOUT_MS, cwd);
        const diffText = diffRun.timedOut
          ? `diff scan timed out after ${RUN_TIMEOUT_MS / 1000}s; the match list below is still valid`
          : (diffRun.stdout.trim() || "(ast-grep printed no diff — rewrite may be a no-op)");
        const text = [
          `preview (${matchFiles.length} file(s), ${matches.length} match(es)) — no files were modified`,
          "─".repeat(60),
          truncateDiffHeadTail(diffText),
          "─".repeat(60),
          `files that would change:\n${matchFiles.map((f) => `- ${f}`).join("\n")}`,
          hints.length ? `\n${hints.join("\n")}` : "",
          "",
          "call again with action=apply to perform this rewrite",
        ].filter(Boolean).join("\n");
        return {
          content: [{ type: "text", text }],
          details: { action: "preview", matchCount: matches.length, matchFiles, truncatedDiff: diffRun.timedOut !== false },
        };
      }

      // ── apply：新鲜度守卫（与 edit 同一张表）──
      const staleFiles: string[] = [];
      const sessionPath = deps.getSessionPath?.() || null;
      for (const rel of matchFiles) {
        const abs = resolveWithin(rel, cwd);
        if (!fs.existsSync(abs)) continue;
        const staleness = await deps.tracker?.checkFreshBeforeMutation(sessionPath, abs);
        if (staleness?.stale) staleFiles.push(rel);
      }
      if (staleFiles.length) {
        return {
          isError: true,
          content: [{
            type: "text",
            text: [
              "Files changed since they were last read (by the user or another process). Re-read them, then retry:",
              ...staleFiles.map((f) => `- ${f}`),
            ].join("\n"),
          }],
          details: { errorCode: "FILE_STALE_SINCE_READ", staleFiles },
        };
      }

      // ── apply：原地重写（包在文件历史快照通道里，与 write/edit 同源）──
      // 改前指纹（mtimeMs）：apply 后按真实变化复验，报告只认真被改过的文件，
      // 不把 sg 的匹配集当改动集转述（防作用域错位时谎报 0 改动）。
      const mtimeBefore = new Map<string, number>();
      for (const rel of matchFiles) {
        const abs = resolveWithin(rel, cwd);
        try { mtimeBefore.set(rel, fs.statSync(abs).mtimeMs); } catch { /* 不存在即不会被动 */ }
      }
      let applyRun: { stdout: string; stderr: string; timedOut: boolean; exitCode: number | null };
      const perform = async () => runSg(sg.path, buildArgs(params, ["--rewrite", String(params.rewrite), "--update-all"]), RUN_TIMEOUT_MS, cwd);
      applyRun = deps.withFileChangeCapture
        ? await deps.withFileChangeCapture("edit", perform as any) as any
        : await perform();
      if (applyRun.timedOut) {
        return {
          isError: true,
          content: [{ type: "text", text: `ast-grep apply timed out after ${RUN_TIMEOUT_MS / 1000}s — files may be partially rewritten; re-read before any further edit.` }],
          details: { errorCode: "AST_EDIT_TIMEOUT", phase: "apply" },
        };
      }
      if (applyRun.exitCode !== 0) {
        return {
          isError: true,
          content: [{ type: "text", text: `ast-grep apply failed: ${(applyRun.stderr || "unknown error").split("\n")[0].slice(0, 400)}` }],
          details: { errorCode: "AST_GREP_FAILED", phase: "apply", stderr: applyRun.stderr.slice(0, 1000) },
        };
      }

      // ── 成功：按 mtime 复验真实改动 → 登记突变 + fileChange 日记（尽力，不阻塞结果）──
      const changedFiles: string[] = [];
      for (const rel of matchFiles) {
        const abs = resolveWithin(rel, cwd);
        if (!fs.existsSync(abs)) continue;
        const before = mtimeBefore.get(rel);
        const after = fs.statSync(abs).mtimeMs;
        if (before === undefined || after === before) continue; // 没真被改，不谎报
        changedFiles.push(rel);
        try {
          await deps.tracker?.observeMutation(sessionPath, abs);
        } catch { /* 登记失败不吞重写事实 */ }
        if (sessionPath && typeof deps.recordFileOperation === "function") {
          try {
            deps.recordFileOperation({
              sessionPath,
              filePath: abs,
              label: path.basename(abs),
              origin: "agent_ast_edit",
              operation: "modified",
            });
          } catch { /* 日记失败如实由 details 反映 */ }
        }
      }

      const summary = [
        `applied rewrite to ${changedFiles.length} file(s) (${matches.length} match(es) scanned, verified by mtime)`,
        ...changedFiles.map((f) => `- ${f}`),
        hints.length ? `\n${hints.join("\n")}` : "",
      ].filter(Boolean).join("\n");
      return {
        content: [{ type: "text", text: summary }],
        details: { action: "apply", matchCount: matches.length, changedFiles },
      };
    },
  };
}

function buildArgs(params: any, extra: string[]): string[] {
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
