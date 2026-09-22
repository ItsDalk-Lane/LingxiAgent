/**
 * run-code-tool.ts — 常驻代码执行内核（阶段三·13）。
 *
 * 每种语言一个持久 REPL 会话（node-pty 经 terminal-session-manager，随
 * 会话归属/清理/stale 恢复走既有机制），变量与 import 跨调用存活。
 * 代码经 base64 单行注入（python: exec(base64) / node: eval(Buffer)），
 * 回显只有那一短行、好剥；写前记 transcript seq，读增量直到输出静默
 * （静默窗 + 总上限）——未静默即如实说明仍在执行。崩溃/失效（exited/
 * stale）如实报告并提供 restart 重建；缺语言（env-deps 探测）不注册，
 * 报错并指路环境依赖页。输出保头保尾截断复用 exec 的 truncateHeadTail。
 */
import { execFile } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { promisify } from "node:util";
import { Type, StringEnum } from "../pi-sdk/index.ts";
import { truncateHeadTail } from "../exec-command/runner.ts";
import { detectEnvDeps } from "../env-deps/detect.ts";

const execFileAsync = promisify(execFile);

export const RUN_CODE_SILENCE_MS = 800;
export const RUN_CODE_MAX_WAIT_MS = 30_000;
export const RUN_CODE_MAX_OUTPUT_BYTES = 50_000;

interface LanguageSpec {
  id: string;
  label: string;
  /** PTY 起动命令（REPL）。 */
  command: string;
  /** 起动时附加环境变量。 */
  extraEnv?: Record<string, string>;
  /** 注入到 REPL 的语句文本。 */
  wrap: (code: string) => string;
  /** 注入行的回显前缀特征（用于剥回显）；fileLoad 模式剥 .load 行。 */
  echoNeedle: string | null;
  /** node 模式：写临时文件 + .load 注入。 */
  fileLoad?: boolean;
}

/**
 * 注入策略（真机验证的取舍）：
 * - python：PYTHON_BASIC_REPL=1 回到无语法高亮的老 REPL（新 REPL 逐字符
 *   彩色回显会把 transcript 刷成转义噪声），代码走 exec(base64) 单行注入
 *   ——作用域=REPL 顶层 __main__，变量跨调用存活（实测）。
 * - node：REPL 里 direct eval 的作用域是 eval 自己，const/let 定义不进
 *   REPL 持久 context（实测 fib 丢失）；改写临时文件 + REPL 命令 .load，
 *   在 REPL 顶层作用域执行，const/函数定义跨调用存活。
 */
const LANGUAGES: Record<string, LanguageSpec> = {
  python3: {
    id: "python3",
    label: "Python 3",
    command: "python3 -i",
    extraEnv: { PYTHON_BASIC_REPL: "1" },
    wrap: (code) => `exec(__import__("base64").b64decode("${Buffer.from(code, "utf8").toString("base64")}"))`,
    echoNeedle: "exec(__import__",
  },
  node: {
    id: "node",
    label: "Node.js",
    command: "node",
    extraEnv: {},
    wrap: (code) => code,
    echoNeedle: null,
    fileLoad: true,
  },
};

export interface RunCodeToolDeps {
  manager: any; // TerminalSessionManager
  getSessionPath: () => string | null;
  getAgentId: () => string | null;
  getCwd: () => string;
  /** 缺省走 detectEnvDeps 探测；测试可注入。 */
  getAvailableLanguages?: () => Promise<string[]>;
  now?: () => number;
}

async function defaultAvailableLanguages(): Promise<string[]> {
  try {
    const report = await detectEnvDeps({});
    const installed = (id: string) => report.deps?.some?.((d: any) => d?.id === id && d?.status === "installed") === true;
    return ["python3", "node"].filter((id) => installed(id));
  } catch {
    return [];
  }
}

function stripEcho(text: string, spec: LanguageSpec, sentCode: string, rawCode = ""): string {
  let out = text;
  if (spec.echoNeedle) {
    out = out.split("\n").filter((line) => !line.includes(spec.echoNeedle)).join("\n");
  } else if (spec.fileLoad) {
    // node .load 会回显文件每一行 + REPL 对末表达式回显 undefined：两者都剥
    const codeLines = new Set(rawCode.split("\n").map((l) => l.trim()).filter(Boolean));
    out = out
      .split("\n")
      .filter((line) => {
        const t = line.trim();
        if (/^\.load\s/.test(t)) return false;
        if (t === "undefined") return false;
        if (codeLines.has(t)) return false;
        return true;
      })
      .join("\n");
  } else {
    // 逐字回显剥除：输出若以发送文本为前缀则剥掉该前缀
    if (sentCode && out.startsWith(sentCode)) out = out.slice(sentCode.length);
  }
  return out;
}

/** ANSI 转义序列（颜色/光标/括号粘贴模式）整体剥除。ESC/BEL 本身就是要匹配的目标。 */
export function stripAnsi(text: string): string {
  return text
    // eslint-disable-next-line no-control-regex
    .replace(/\x1b\[[0-9;?]*[a-zA-Z]/g, "")
    // eslint-disable-next-line no-control-regex
    .replace(/\x1b\][^\x07]*(\x07|\x1b\\)/g, "");
}

function stripPromptNoise(text: string): string {
  return stripAnsi(text)
    .replace(/^((>>>|\.\.\.|>)\s*)+/gm, "")
    .replace(/\r/g, "");
}

async function waitForQuiet(manager: any, sessionPath: string, terminalId: string, timeoutMs: number): Promise<void> {
  const startedAt = Date.now();
  let lastLen = -1;
  let lastChangeAt = startedAt;
  for (;;) {
    await new Promise((r) => setTimeout(r, 120));
    const tail = manager.readTail({ sessionPath, terminalId }) as any;
    const len = String(tail?.output ?? tail?.text ?? "").length;
    if (len !== lastLen) { lastLen = len; lastChangeAt = Date.now(); }
    if (Date.now() - lastChangeAt >= RUN_CODE_SILENCE_MS || Date.now() - startedAt >= timeoutMs) return;
  }
}

export function createRunCodeTool(deps: RunCodeToolDeps) {
  const now = deps.now || (() => Date.now());
  /** (sessionPath, languageId) → terminalId（会话内每语言一个内核）。 */
  const kernels = new Map<string, string>();
  const availabilityCache: { at: number; languages: string[] } | null = null;

  async function availableLanguages(): Promise<string[]> {
    if (typeof deps.getAvailableLanguages === "function") return deps.getAvailableLanguages();
    return defaultAvailableLanguages();
  }

  function kernelKey(sessionPath: string, languageId: string): string {
    return `${sessionPath}::${languageId}`;
  }

  function kernelEntry(sessionPath: string, languageId: string): any | null {
    const listResult = deps.manager.list(sessionPath) as any;
    const terminals = Array.isArray(listResult) ? listResult : (listResult?.terminals || []);
    // 身份兜底两路：map 缓存 → 按 label 扫（工具实例可能被重建，map 丢了
    // 不能重复 start 出第二个 REPL 让旧进程泄漏）。
    const label = `run_code:${languageId}`;
    const terminalId = kernels.get(kernelKey(sessionPath, languageId));
    // map 命中：返回任意状态（死会话由 run 分支如实报告）；
    // label 兜底：只认 running（否则 restart 后会捞到旧尸体、永远建不回）。
    const entry = terminalId
      ? terminals.find((e: any) => e.terminalId === terminalId) || null
      : terminals.find((e: any) => e.label === label && e.status === "running") || null;
    if (!entry) return null;
    kernels.set(kernelKey(sessionPath, languageId), entry.terminalId);
    return { entry, terminalId: entry.terminalId };
  }

  async function ensureKernel(sessionPath: string, spec: LanguageSpec): Promise<{ terminalId: string; seq: number } | { error: string }> {
    const found = kernelEntry(sessionPath, spec.id);
    if (found) {
      if (found.entry.status === "running") {
        return { terminalId: found.terminalId, seq: found.entry.seq ?? 0 };
      }
      // 崩溃/退出/失效：如实告知由上层决定 restart；这里只报告。
      return { error: `kernel ${spec.id} is ${found.entry.status} (exit ${found.entry.exitCode ?? "?"}) — call action=restart to rebuild it (state is lost)` };
    }
    // 起内核：cwd=会话工作目录；label 标语言
    const started = await deps.manager.start({
      toolCallId: `run_code:${spec.id}`,
      sessionPath,
      agentId: deps.getAgentId?.() || "",
      cwd: deps.getCwd(),
      command: spec.command,
      label: `run_code:${spec.id}`,
      cols: 200,
      rows: 50,
      ...(spec.extraEnv && Object.keys(spec.extraEnv).length ? { env: { ...process.env, ...spec.extraEnv } } : {}),
    });
    const terminalId = started?.terminalId;
    if (!terminalId) return { error: `failed to start ${spec.id} kernel` };
    kernels.set(kernelKey(sessionPath, spec.id), terminalId);
    // 就绪=输出静默（banner 全部落进 sinceSeq 之前，不再泄入首次结果）
    await waitForQuiet(deps.manager, sessionPath, terminalId, 3000);
    const entry = kernelEntry(sessionPath, spec.id);
    return { terminalId, seq: entry?.entry?.seq ?? 0 };
  }

  return {
    name: "run_code",
    description: "Persistent code kernel per language (python3 / node): variables, imports and function definitions survive across calls in the same session — great for iterative data work, prototyping, and debugging with state. Send code with action=run; output returns once the kernel goes quiet. If it is still busy when the wait cap hits you get told so — for CPU-long jobs prefer exec_command with wait_mode=auto. action=restart rebuilds the kernel (state lost); action=status shows kernel state. Languages missing on this machine report an install hint (see the Env Dependencies settings page).",
    parameters: Type.Object({
      action: Type.Optional(StringEnum(["run", "restart", "status"], { description: "run: execute code in the persistent kernel (default). restart: kill and rebuild the kernel (state lost). status: kernel state" })),
      language: Type.String({ description: "python3 or node" }),
      code: Type.String({ description: "Source to execute (for action=run). Use print()/console.log to surface results." }),
    }),
    sessionPermission: {
      resolveInvocation: (input: any = {}) => {
        if (input?.action === "status") {
          return { action: "status", kind: "read", capability: "run_code.status" };
        }
        return { action: input?.action === "restart" ? "restart" : "run", kind: "routine", capability: `run_code.${input?.action === "restart" ? "restart" : "run"}` };
      },
    },
    async execute(_toolCallId: string, params: any = {}, ..._rest: any[]) {
      void availabilityCache;
      const action = params?.action === "restart" || params?.action === "status" ? params.action : "run";
      const sessionPath = deps.getSessionPath?.() || null;
      if (!sessionPath) {
        return { isError: true, content: [{ type: "text", text: "no active session — run_code kernels are session-scoped" }] };
      }
      const languageId = typeof params?.language === "string" ? params.language.trim() : "";
      const spec = LANGUAGES[languageId];
      if (!spec) {
        return { isError: true, content: [{ type: "text", text: `language must be one of: ${Object.keys(LANGUAGES).join(", ")} (got "${languageId}")` }] };
      }
      const available = await availableLanguages();
      if (!available.includes(languageId)) {
        return {
          isError: true,
          content: [{
            type: "text",
            text: `${spec.label} is not installed on this machine — the ${languageId} kernel is not registered. Install it (e.g. brew install python3 / nodejs) and check the Env Dependencies settings page.`,
          }],
          details: { errorCode: "RUN_CODE_LANGUAGE_MISSING", language: languageId, available },
        };
      }

      if (action === "status") {
        const found = kernelEntry(sessionPath, languageId);
        if (!found) {
          return { content: [{ type: "text", text: `${spec.label} kernel: not started yet (first run starts it)` }], details: { started: false } };
        }
        return {
          content: [{ type: "text", text: `${spec.label} kernel: ${found.entry.status}${found.entry.exitCode != null ? ` (exit ${found.entry.exitCode})` : ""}` }],
          details: { started: true, status: found.entry.status, exitCode: found.entry.exitCode ?? null },
        };
      }

      if (action === "restart") {
        const found = kernelEntry(sessionPath, languageId);
        if (found) {
          try { deps.manager.close({ sessionPath, terminalId: found.terminalId }); } catch { /* 尽力 */ }
          kernels.delete(kernelKey(sessionPath, languageId));
        }
        const rebuilt = await ensureKernel(sessionPath, spec);
        if ("error" in rebuilt) {
          return { isError: true, content: [{ type: "text", text: rebuilt.error }] };
        }
        return { content: [{ type: "text", text: `${spec.label} kernel rebuilt (fresh state)` }], details: { restarted: true } };
      }

      // ── run ──
      const code = typeof params?.code === "string" ? params.code : "";
      if (!code.trim()) {
        return { content: [{ type: "text", text: "code is required for action=run" }] };
      }
      const kernel = await ensureKernel(sessionPath, spec);
      if ("error" in kernel) {
        return { isError: true, content: [{ type: "text", text: kernel.error }], details: { errorCode: "RUN_CODE_KERNEL_DEAD" } };
      }

      // 写入前记 seq；等待增量静默
      const before = kernelEntry(sessionPath, languageId);
      const sinceSeq = before?.entry?.seq ?? kernel.seq;
      let injection = spec.wrap(code);
      let scratchPath: string | null = null;
      if (spec.fileLoad) {
        // node：写临时文件 + .load（REPL 顶层作用域，const/函数存活）
        const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-run-code-"));
        scratchPath = path.join(dir, `run-${Date.now()}.js`);
        fs.writeFileSync(scratchPath, code, "utf8");
        injection = `.load ${scratchPath}`;
      }
      deps.manager.write({ sessionPath, terminalId: kernel.terminalId, chars: `${injection}\n` });

      const startedAt = now();
      let lastLen = -1;
      let lastChangeAt = startedAt;
      let output = "";
      for (;;) {
        await new Promise((r) => setTimeout(r, 120));
        const tail = deps.manager.readTail({ sessionPath, terminalId: kernel.terminalId, sinceSeq }) as any;
        output = String(tail?.output ?? tail?.text ?? "");
        if (output.length !== lastLen) {
          lastLen = output.length;
          lastChangeAt = now();
        }
        const entryNow = kernelEntry(sessionPath, languageId);
        const dead = !entryNow || entryNow.entry.status !== "running";
        const quiet = now() - lastChangeAt >= RUN_CODE_SILENCE_MS;
        const timeout = now() - startedAt >= RUN_CODE_MAX_WAIT_MS;
        if (dead) {
          const exitCode = entryNow?.entry?.exitCode ?? "?";
          return {
            isError: true,
            content: [{
              type: "text",
              text: [
                `${spec.label} kernel exited (exit ${exitCode}) while running your code. Partial output:`,
                "─".repeat(40),
                truncateHeadTail(stripPromptNoise(stripEcho(stripAnsi(output), spec, injection, code)), { maxBytes: RUN_CODE_MAX_OUTPUT_BYTES }).content,
                "─".repeat(40),
                "The kernel is dead — call action=restart to rebuild it (state is lost).",
              ].join("\n"),
            }],
            details: { errorCode: "RUN_CODE_KERNEL_EXITED", exitCode: exitCode },
          };
        }
        if (quiet || timeout) {
          const cleaned = stripPromptNoise(stripEcho(stripAnsi(output), spec, injection, code));
          const truncated = truncateHeadTail(cleaned, { maxBytes: RUN_CODE_MAX_OUTPUT_BYTES }) as any;
          const text = typeof truncated === "string" ? truncated : String(truncated?.content ?? cleaned);
          const stillBusy = timeout && !quiet;
          if (scratchPath) { try { fs.rmSync(path.dirname(scratchPath), { recursive: true, force: true }); } catch { /* 尽力 */ } }
          return {
            content: [{
              type: "text",
              text: stillBusy
                ? [
                    "kernel is still executing after the 30s wait cap — this call returned without its result.",
                    "Partial output so far:",
                    "─".repeat(40),
                    text,
                    "─".repeat(40),
                    "For long CPU-bound jobs use exec_command with wait_mode=auto instead; or split the work.",
                  ].join("\n")
                : (text.trim() || "(no output — use print()/console.log to surface values)"),
            }],
            details: { language: languageId, stillBusy, bytes: text.length },
          };
        }
      }
    },
  };
}
