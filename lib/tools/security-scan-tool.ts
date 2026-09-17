/**
 * security-scan-tool.ts — 代码安全扫描（阶段二·9）。
 *
 * 基线纯 JS 永远可用（密钥/敏感信息规则源自日志脱敏与 PII guard 的正则）；
 * 检测到 semgrep/gitleaks 自动并入，缺失如实缺席（环境依赖页可见）。
 * 两种模式：changes=扫当前工作区改动（git status/diff 取清单，非 git 或
 * 无改动则如实说明并建议 path）；path=扫指定目录/文件。范围限死在
 * cwd + 授权文件夹内（每个文件硬校验）。结果分级汇总只带 file:line +
 * ruleId，不带命中正文；计数摘要写安全审计账（同样不进原文）。
 * 权限：read（计划模式也能扫——只读检查无害且有益）。
 */
import { execFile } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { promisify } from "node:util";
import { Type, StringEnum } from "../pi-sdk/index.ts";
import {
  scanFilesForSecrets,
  sortFindings,
  countBySeverity,
  type ScannedFileInput,
} from "../security/baseline-scanner.ts";
import type { SecretFinding } from "../security/secret-rules.ts";
import {
  queryExternalScannerAvailability,
  runGitleaks,
  runSemgrep,
} from "../security/external-scanners.ts";
import { appendSecurityAuditEvent } from "../../core/security-audit-log.ts";

const execFileAsync = promisify(execFile);

const GIT_TIMEOUT_MS = 15_000;
const DEFAULT_MAX_FILES = 400;
const HARD_MAX_FILES = 2000;
const DISPLAY_CAP = 50;
/** path 模式跳过的目录/扩展名（.git 本体、依赖、构建产物、二进制常见后缀）。 */
const SKIP_DIRS = new Set([".git", "node_modules", "dist", "dist-sandbox", "build", "out", ".venv", "venv", "__pycache__", ".next", "target"]);
const SKIP_EXTS = new Set([".png", ".jpg", ".jpeg", ".gif", ".webp", ".ico", ".pdf", ".zip", ".gz", ".tgz", ".tar", ".mp4", ".mp3", ".wav", ".woff", ".woff2", ".ttf", ".eot", ".dylib", ".so", ".dll", ".exe", ".bin", ".wasm"]);

export interface SecurityScanToolDeps {
  cwd: string;
  getAuthorizedFolders?: () => string[];
  /** lingxiHome；审计账写入（空则跳过审计，结果照常）。 */
  getLingxiHome?: () => string | null;
  /** 审计写入接口（注入以便测试）；缺省用 appendSecurityAuditEvent。 */
  appendAudit?: (event: Record<string, any>) => void;
}

function isInsideRoots(absPath: string, roots: string[]): boolean {
  return roots.some((root) => {
    const rel = path.relative(path.resolve(root), absPath);
    return rel === "" || (!rel.startsWith("..") && !path.isAbsolute(rel));
  });
}

async function listChangedFiles(cwd: string): Promise<{ files: string[]; reason?: string }> {
  try {
    const { stdout } = await execFileAsync("git", ["status", "--porcelain", "-z", "--untracked-files=all"], {
      cwd, timeout: GIT_TIMEOUT_MS, maxBuffer: 8 * 1024 * 1024,
    });
    const out: string[] = [];
    for (const entry of stdout.split("\0")) {
      if (!entry) continue;
      // porcelain -z: "XY path"（重命名 "XY new\0old" 只取 new）
      const filePath = entry.slice(3).trim();
      if (filePath) out.push(filePath);
    }
    return { files: out };
  } catch (err: any) {
    return { files: [], reason: err?.message?.includes("not a git repository")
      ? "not_a_git_repo"
      : "git_unavailable" };
  }
}

function walkFiles(rootDir: string, maxFiles: number): string[] {
  const out: string[] = [];
  const queue = [rootDir];
  while (queue.length && out.length < maxFiles) {
    const dir = queue.shift()!;
    let entries: fs.Dirent[] = [];
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch { continue; }
    for (const entry of entries) {
      if (out.length >= maxFiles) break;
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        if (SKIP_DIRS.has(entry.name) || entry.name.startsWith(".")) continue;
        queue.push(full);
      } else if (entry.isFile()) {
        if (SKIP_EXTS.has(path.extname(entry.name).toLowerCase())) continue;
        out.push(full);
      }
    }
  }
  return out;
}

export function createSecurityScanTool(deps: SecurityScanToolDeps) {
  const cwd = path.resolve(deps.cwd);
  const roots = () => {
    const extra = (typeof deps.getAuthorizedFolders === "function" ? deps.getAuthorizedFolders() : [])
      .filter((f) => typeof f === "string" && f.trim());
    return [cwd, ...extra.map((f) => path.resolve(f))];
  };

  return {
    name: "security_scan",
    description: "Scan code for leaked secrets and sensitive data before shipping. Baseline JS scanner always runs (provider API keys, private keys, secret assignments, URL tokens, PII); semgrep/gitleaks join automatically when installed (see the Env Dependencies settings page). mode=changes scans the working tree's git changes (default), mode=path scans a given directory/file. Scope is limited to the workspace and authorized folders. Findings are file:line + rule id only — never secret contents. After code changes that touch auth, config, or keys, run a scan proactively.",
    parameters: Type.Object({
      mode: StringEnum(["changes", "path"], { description: "changes: scan git working-tree changes (default). path: scan a directory or file" }),
      path: Type.String({ description: "Target directory or file for mode=path (relative to cwd)" }),
      max_files: Type.Number({ description: `File cap for mode=path (default ${DEFAULT_MAX_FILES}, max ${HARD_MAX_FILES})` }),
    }),
    sessionPermission: {
      resolveInvocation: (_input: any = {}) => ({
        action: "scan",
        kind: "read",
        capability: "security_scan.read",
      }),
    },
    async execute(_toolCallId: string, params: any = {}, ..._rest: any[]) {
      const mode = params?.mode === "path" ? "path" : "changes";
      const allowedRoots = roots();

      // ── 目标文件清单（相对路径）──
      let relFiles: string[] = [];
      let changesNote: string | null = null;
      if (mode === "changes") {
        const { files, reason } = await listChangedFiles(cwd);
        relFiles = files;
        if (reason === "not_a_git_repo") {
          changesNote = "working tree is not a git repository — no change list available; use mode=path to scan a directory instead";
        } else if (reason) {
          changesNote = `git unavailable (${reason}) — use mode=path to scan a directory instead`;
        } else if (!files.length) {
          return {
            content: [{ type: "text", text: "no working-tree changes to scan" }],
            details: { mode, filesScanned: 0, counts: { critical: 0, warning: 0, info: 0 } },
          };
        }
      } else {
        const target = typeof params?.path === "string" && params.path.trim()
          ? path.resolve(cwd, params.path.trim())
          : cwd;
        if (!isInsideRoots(target, allowedRoots)) {
          return {
            isError: true,
            content: [{ type: "text", text: `path "${params.path}" is outside the workspace and authorized folders` }],
            details: { errorCode: "SECURITY_SCAN_OUT_OF_SCOPE" },
          };
        }
        const maxFiles = Math.min(
          Number.isInteger(params?.max_files) && (params.max_files as number) > 0 ? params.max_files as number : DEFAULT_MAX_FILES,
          HARD_MAX_FILES,
        );
        if (fs.existsSync(target) && fs.statSync(target).isFile()) {
          relFiles = [path.relative(cwd, target)];
        } else {
          relFiles = walkFiles(target, maxFiles).map((abs) => path.relative(cwd, abs));
          if (relFiles.length >= maxFiles) {
            changesNote = `file cap ${maxFiles} reached — raise max_files or narrow the path`;
          }
        }
      }

      // ── 范围硬校验 + 读文件 ──
      const allowedRootsSet = allowedRoots;
      const inputs: ScannedFileInput[] = [];
      let outOfScope = 0;
      for (const rel of relFiles) {
        const abs = path.resolve(cwd, rel);
        if (!isInsideRoots(abs, allowedRootsSet)) { outOfScope += 1; continue; }
        try {
          const stat = fs.statSync(abs);
          if (!stat.isFile()) continue;
          if (stat.size > 4 * 1024 * 1024) continue; // 大文件让基线层如实记 oversized（>2MB 会跳过），>4MB 直接不读
          inputs.push({ path: rel, content: fs.readFileSync(abs, "utf8") });
        } catch { /* 读不了的文件如实少扫，不阻塞整体 */ }
      }

      // ── 基线（永远跑）+ 环境增强 ──
      const baseline = scanFilesForSecrets(inputs);
      const availability = await queryExternalScannerAvailability();
      const externalFindings: SecretFinding[] = [];
      const engines = ["baseline"];
      if (availability.semgrep) {
        const found = await runSemgrep(cwd, inputs.slice(0, DEFAULT_MAX_FILES).map((f) => f.path));
        if (found.length || true) engines.push("semgrep");
        externalFindings.push(...found);
      }
      if (availability.gitleaks) {
        const found = await runGitleaks(cwd);
        engines.push("gitleaks");
        externalFindings.push(...found);
      }

      // ── 合并去重（不同引擎同 file:line:rule 保留一条；同 file:line 不同规则都留）──
      const seen = new Set<string>();
      const findings = sortFindings([...baseline.findings, ...externalFindings].filter((f) => {
        const key = `${f.file}:${f.line}:${f.ruleId}`;
        if (seen.has(key)) return false;
        seen.add(key);
        return true;
      }));
      const counts = countBySeverity(findings);

      // ── 审计账：只写计数与引擎清单，不进命中正文 ──
      try {
        const append = deps.appendAudit;
        if (append) {
          append({ action: "security_scan", result: "success", metadata: { mode, engines, filesScanned: baseline.filesScanned, outOfScope, ...counts } });
        } else if (typeof deps.getLingxiHome === "function") {
          const home = deps.getLingxiHome();
          if (home) {
            appendSecurityAuditEvent(home, {
              action: "security_scan",
              result: "success",
              metadata: { mode, engines, filesScanned: baseline.filesScanned, outOfScope, ...counts },
            });
          }
        }
      } catch { /* 审计失败不阻塞扫描结果 */ }

      // ── 结果文本（只带 file:line + ruleId）──
      const lines: string[] = [
        `security scan (${engines.join(" + ")}) — ${counts.critical} critical, ${counts.warning} warning, ${counts.info} info`,
        ...(baseline.filesSkipped ? [`(${baseline.filesSkipped} file(s) skipped: ${baseline.skipped.binary} binary, ${baseline.skipped.oversized} oversized)`] : []),
        ...(outOfScope ? [`(${outOfScope} file(s) outside scope ignored)`] : []),
        ...(changesNote ? [`note: ${changesNote}`] : []),
        ...(!availability.semgrep && !availability.gitleaks
          ? ["semgrep/gitleaks not installed — baseline only (install hints on the Env Dependencies settings page)"]
          : []),
      ];
      if (findings.length) {
        lines.push("", "findings:");
        for (const f of findings.slice(0, DISPLAY_CAP)) {
          lines.push(`  [${f.severity}] ${f.file}:${f.line} — ${f.ruleId}`);
        }
        if (findings.length > DISPLAY_CAP) {
          lines.push(`  ... ${findings.length - DISPLAY_CAP} more (narrow the scan scope to review)`);
        }
      } else {
        lines.push("", "no findings");
      }
      return {
        content: [{ type: "text", text: lines.join("\n") }],
        details: {
          mode,
          engines,
          filesScanned: baseline.filesScanned,
          filesSkipped: baseline.filesSkipped,
          outOfScope,
          counts,
          findingCount: findings.length,
          findings: findings.slice(0, DISPLAY_CAP),
        },
      };
    },
  };
}
