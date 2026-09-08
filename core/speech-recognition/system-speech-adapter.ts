/**
 * system-speech-adapter.ts — macOS 系统语音识别适配器（本地、免费、离线）
 *
 * 通过 LingxiSpeechHelper（Swift，SFSpeechRecognizer）把已录制的 WAV 文件
 * 转成文字。helper 路径解析遵循桌面路径合同（与 computer-use helper 同源）：
 * 受控环境变量覆盖 → 打包 Resources（形状校验）→ 仅 dev 的 dist-speech/。
 * 绝不从 server 进程的 process.execPath 推导（那是 node，与 Electron 无关）。
 *
 * 进程模型（F6）：
 * - 异步 spawn，无 shell，不阻塞事件循环；
 * - 取消（AbortSignal）/超时都有界终止同一个子进程：SIGTERM → 宽限 → SIGKILL；
 * - 单次结算：首个终局（结构化结果/结构化错误/退出/超时/取消）决定结果，
 *   晚到事件一律丢弃；取消与超时绝不记为成功；
 * - helper stdout 协议 2（单行 JSON，含 code 字段）为主协议；旧式 stderr
 *   字符串映射仅作兼容层。
 *
 * 授权模型：helper 绝不自己弹权限（裸进程弹不了，实测 SIGABRT）；打包形态下
 * TCC 把语音识别授权归属到宿主 Lingxi.app，宿主经 desktop/speech-permissions.cjs
 * 在录音前完成检查与请求。未授权时 helper 显式失败，本适配器原样透出错误码
 * （不静默降级）。
 */

import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { createModuleLogger } from "../../lib/debug-log.ts";

const log = createModuleLogger("system-speech-asr");

export const SYSTEM_SPEECH_ERROR_CODES = {
  HELPER_NOT_FOUND: "SYSTEM_SPEECH_HELPER_NOT_FOUND",
  PERMISSION_REQUIRED: "SYSTEM_SPEECH_PERMISSION_REQUIRED",
  PERMISSION_DENIED: "SYSTEM_SPEECH_PERMISSION_DENIED",
  RESTRICTED: "SYSTEM_SPEECH_RESTRICTED",
  RECOGNIZER_UNAVAILABLE: "SYSTEM_SPEECH_RECOGNIZER_UNAVAILABLE",
  TIMEOUT: "SYSTEM_SPEECH_TIMEOUT",
  CANCELLED: "SYSTEM_SPEECH_CANCELLED",
  INVALID_OUTPUT: "SYSTEM_SPEECH_INVALID_OUTPUT",
  PROCESS_FAILED: "SYSTEM_SPEECH_PROCESS_FAILED",
} as const;

type SystemSpeechErrorCode =
  (typeof SYSTEM_SPEECH_ERROR_CODES)[keyof typeof SYSTEM_SPEECH_ERROR_CODES];

class SystemSpeechError extends Error {
  code: SystemSpeechErrorCode;
  constructor(code: SystemSpeechErrorCode, message: string) {
    super(message);
    this.name = "SystemSpeechError";
    this.code = code;
  }
}

function speechError(code: SystemSpeechErrorCode, message: string): SystemSpeechError {
  return new SystemSpeechError(code, message);
}

// 集中可调参数：识别上限传给 helper（--timeout 秒），进程上限/终止宽限由
// 本适配器强制执行；均可经受控环境变量覆盖（测试与验收环境注入）。
const RECOGNITION_TIMEOUT_SECONDS = 120;
const DEFAULT_PROCESS_TIMEOUT_MS = 150_000;
const DEFAULT_KILL_GRACE_MS = 1_500;
const MAX_STDOUT_BYTES = 4 * 1024 * 1024;

const HELPER_ENV_OVERRIDE = "LINGXI_SPEECH_HELPER_EXEC";
const PROCESS_TIMEOUT_ENV = "LINGXI_SPEECH_PROCESS_TIMEOUT_MS";
const KILL_GRACE_ENV = "LINGXI_SPEECH_KILL_GRACE_MS";
const HELPER_RELATIVE_RESOURCES = ["speech", "macos", "lingxi-speech-helper"];

function positiveIntFromEnv(env: any, name: string, fallback: number): number {
  const raw = Number(env?.[name]);
  return Number.isFinite(raw) && raw > 0 ? Math.floor(raw) : fallback;
}

// ---------- helper 路径解析（桌面路径合同，参照 macos-cua-provider 的既有形状） ----------

function normalizedAbsolutePosixPath(value: any): string | null {
  const candidate = typeof value === "string" ? value.trim() : "";
  if (!candidate || candidate.includes("\0") || !path.posix.isAbsolute(candidate)) return null;
  return path.posix.normalize(candidate);
}

function macAppResourcesRoot(value: any): string | null {
  const resourcesRoot = normalizedAbsolutePosixPath(value);
  if (!resourcesRoot || path.posix.basename(resourcesRoot) !== "Resources") return null;
  const contentsRoot = path.posix.dirname(resourcesRoot);
  if (path.posix.basename(contentsRoot) !== "Contents") return null;
  const appRoot = path.posix.dirname(contentsRoot);
  if (!path.posix.basename(appRoot).endsWith(".app")) return null;
  return resourcesRoot;
}

function resourcesRootFromPackagedAppPath(value: any): string | null {
  const appPath = normalizedAbsolutePosixPath(value);
  if (!appPath || path.posix.basename(appPath) !== "app.asar") return null;
  return macAppResourcesRoot(path.posix.dirname(appPath));
}

function resourcesRootFromPackagedExecPath(value: any): string | null {
  const execPath = normalizedAbsolutePosixPath(value);
  if (!execPath) return null;
  const macosRoot = path.posix.dirname(execPath);
  if (path.posix.basename(macosRoot) !== "MacOS") return null;
  const contentsRoot = path.posix.dirname(macosRoot);
  if (path.posix.basename(contentsRoot) !== "Contents") return null;
  const appRoot = path.posix.dirname(contentsRoot);
  if (!path.posix.basename(appRoot).endsWith(".app")) return null;
  return path.posix.join(contentsRoot, "Resources");
}

function pushUniquePosixPath(out: string[], value: any) {
  if (!value) return;
  const normalized = path.posix.normalize(String(value));
  if (!out.includes(normalized)) out.push(normalized);
}

function helperCandidate(resourcesRoot: string): string {
  return path.posix.join(resourcesRoot, ...HELPER_RELATIVE_RESOURCES);
}

/**
 * 解析 lingxi-speech-helper 的绝对路径。
 * 顺序：LINGXI_SPEECH_HELPER_EXEC（受控注入，校验存在性）→ 打包形态
 * （LINGXI_DESKTOP_IS_PACKAGED=1：RESOURCES_PATH 形状校验优先，然后 APP_PATH /
 * EXEC_PATH 推导；缺失即安装损坏，明确 HELPER_NOT_FOUND，不回退 dev 残留）
 * → dev 形态（dist-speech/mac-<arch>，仅非打包）。
 */
export function resolveSystemSpeechHelperPath({
  env = process.env,
  cwd = process.cwd(),
  arch = process.arch,
  hanaRoot = null,
}: any = {}): string {
  const override = normalizedAbsolutePosixPath(env?.[HELPER_ENV_OVERRIDE]);
  if (override) {
    if (fs.existsSync(override)) return override;
    throw speechError(
      SYSTEM_SPEECH_ERROR_CODES.HELPER_NOT_FOUND,
      `${HELPER_ENV_OVERRIDE} 指向的 helper 不存在: ${override}`,
    );
  }

  if (env?.LINGXI_DESKTOP_IS_PACKAGED === "1") {
    const roots: string[] = [];
    pushUniquePosixPath(roots, macAppResourcesRoot(env?.LINGXI_DESKTOP_RESOURCES_PATH));
    pushUniquePosixPath(roots, resourcesRootFromPackagedAppPath(env?.LINGXI_DESKTOP_APP_PATH));
    pushUniquePosixPath(roots, resourcesRootFromPackagedExecPath(env?.LINGXI_DESKTOP_EXEC_PATH));
    for (const root of roots) {
      const candidate = helperCandidate(root);
      if (fs.existsSync(candidate)) return candidate;
    }
    // 打包形态缺失 helper = 安装损坏：显式失败，绝不回退 dev 目录里偶然残留的构建。
    throw speechError(
      SYSTEM_SPEECH_ERROR_CODES.HELPER_NOT_FOUND,
      "安装损坏：应用包内缺少 lingxi-speech-helper（Resources/speech/macos/）。请重新安装灵犀。",
    );
  }

  const archDir = arch === "x64" ? "mac-x64" : "mac-arm64";
  const devRoots: string[] = [];
  pushUniquePosixPath(devRoots, normalizedAbsolutePosixPath(hanaRoot));
  pushUniquePosixPath(devRoots, normalizedAbsolutePosixPath(cwd));
  for (const root of devRoots) {
    const candidate = path.posix.join(root, "dist-speech", archDir, "lingxi-speech-helper");
    if (fs.existsSync(candidate)) return candidate;
  }
  throw speechError(
    SYSTEM_SPEECH_ERROR_CODES.HELPER_NOT_FOUND,
    "lingxi-speech-helper 未构建：请先运行 npm run build:speech-helper（dev）或重新安装应用（打包）。",
  );
}

// ---------- helper stdout 协议（protocol 2）与旧式 stderr 兼容层 ----------

const KNOWN_STRUCTURED_CODES = new Set<string>([
  SYSTEM_SPEECH_ERROR_CODES.PERMISSION_REQUIRED,
  SYSTEM_SPEECH_ERROR_CODES.PERMISSION_DENIED,
  SYSTEM_SPEECH_ERROR_CODES.RESTRICTED,
  SYSTEM_SPEECH_ERROR_CODES.RECOGNIZER_UNAVAILABLE,
  SYSTEM_SPEECH_ERROR_CODES.TIMEOUT,
  SYSTEM_SPEECH_ERROR_CODES.CANCELLED,
]);

const DEFAULT_ERROR_MESSAGES: Record<string, string> = {
  [SYSTEM_SPEECH_ERROR_CODES.PERMISSION_REQUIRED]:
    "请先在系统弹窗或「系统设置 → 隐私与安全性 → 语音识别」中授权灵犀",
  [SYSTEM_SPEECH_ERROR_CODES.PERMISSION_DENIED]:
    "语音识别权限被拒绝：请在「系统设置 → 隐私与安全性 → 语音识别」中允许灵犀",
  [SYSTEM_SPEECH_ERROR_CODES.RESTRICTED]: "语音识别受设备管理策略限制，无法使用",
  [SYSTEM_SPEECH_ERROR_CODES.RECOGNIZER_UNAVAILABLE]:
    "系统语音识别不可用（该语言可能不受支持，或识别服务暂不可用）",
  [SYSTEM_SPEECH_ERROR_CODES.TIMEOUT]: "系统语音识别超时",
  [SYSTEM_SPEECH_ERROR_CODES.CANCELLED]: "系统语音识别已取消",
  [SYSTEM_SPEECH_ERROR_CODES.INVALID_OUTPUT]: "系统语音识别返回了无法解析的结果",
  [SYSTEM_SPEECH_ERROR_CODES.PROCESS_FAILED]: "系统语音识别进程失败",
};

function mapStructuredErrorCode(code: any): SystemSpeechErrorCode {
  const normalized = String(code || "").trim().toUpperCase();
  const withPrefix = normalized.startsWith("SYSTEM_SPEECH_")
    ? normalized
    : `SYSTEM_SPEECH_${normalized}`;
  if (KNOWN_STRUCTURED_CODES.has(withPrefix)) {
    return withPrefix as SystemSpeechErrorCode;
  }
  return SYSTEM_SPEECH_ERROR_CODES.PROCESS_FAILED;
}

function parseProtocolLine(line: string): any | null {
  const trimmed = line.trim();
  if (!trimmed.startsWith("{")) return null;
  try {
    const parsed = JSON.parse(trimmed);
    if (parsed && typeof parsed === "object" && typeof parsed.ok === "boolean") return parsed;
    return null;
  } catch {
    return null;
  }
}

function lastNonEmptyLine(text: string): string {
  const lines = text.split("\n");
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    if (lines[index].trim()) return lines[index];
  }
  return "";
}

/** 旧式 stderr 字符串映射（兼容层；主协议是 stdout 的结构化 JSON）。 */
function errorFromLegacyStderr(stderr: string): SystemSpeechError {
  const text = String(stderr || "");
  const lowered = text.toLowerCase();
  let code: SystemSpeechErrorCode = SYSTEM_SPEECH_ERROR_CODES.PROCESS_FAILED;
  if (lowered.includes("permission denied")) {
    code = SYSTEM_SPEECH_ERROR_CODES.PERMISSION_DENIED;
  } else if (lowered.includes("permission not yet granted")) {
    code = SYSTEM_SPEECH_ERROR_CODES.PERMISSION_REQUIRED;
  } else if (lowered.includes("restricted")) {
    code = SYSTEM_SPEECH_ERROR_CODES.RESTRICTED;
  } else if (lowered.includes("no speech recognizer available") || lowered.includes("recognizer unavailable")) {
    code = SYSTEM_SPEECH_ERROR_CODES.RECOGNIZER_UNAVAILABLE;
  } else if (lowered.includes("timed out")) {
    code = SYSTEM_SPEECH_ERROR_CODES.TIMEOUT;
  }
  const detail = text.trim().slice(0, 300);
  return speechError(code, detail || DEFAULT_ERROR_MESSAGES[code]);
}

function errorFromStructured(payload: any): SystemSpeechError {
  const code = mapStructuredErrorCode(payload?.code);
  const message = typeof payload?.message === "string" && payload.message.trim()
    ? payload.message.trim().slice(0, 300)
    : DEFAULT_ERROR_MESSAGES[code];
  return speechError(code, message);
}

// ---------- 异步子进程生命周期 ----------

export interface SystemSpeechTranscription {
  text: string;
  resultCode?: string;
  language?: string;
  durationMs?: number;
}

type Settlement =
  | { kind: "result"; value: SystemSpeechTranscription }
  | { kind: "error"; error: SystemSpeechError };

export const systemSpeechRecognitionAdapter = {
  id: "system-speech",
  name: "System Speech Recognition",
  protocolId: "system-speech-recognition",
  types: ["speechRecognition"],

  async available(): Promise<boolean> {
    if (process.platform !== "darwin") return false;
    try {
      resolveSystemSpeechHelperPath();
      return true;
    } catch {
      return false;
    }
  },

  async transcribe(input: any, runtime: any = {}) {
    // platform 可注入（测试用合成 helper 跨平台验证进程生命周期）；
    // 生产链路不传，默认 process.platform，非 macOS 硬拒。
    const platform = runtime?.platform ?? process.platform;
    if (platform !== "darwin") {
      throw new Error("system speech recognition is only available on macOS");
    }
    const env = runtime?.env ?? process.env;
    const helper = resolveSystemSpeechHelperPath({ env });
    const audioPath = input.file?.realPath || input.file?.filePath;
    if (!audioPath) throw new Error("system speech recognition requires a recorded audio file");
    if (!fs.existsSync(audioPath)) throw new Error(`audio file missing: ${audioPath}`);

    const signal: AbortSignal | undefined = input.signal;
    if (signal?.aborted) {
      throw speechError(SYSTEM_SPEECH_ERROR_CODES.CANCELLED, DEFAULT_ERROR_MESSAGES[SYSTEM_SPEECH_ERROR_CODES.CANCELLED]);
    }

    const processTimeoutMs = positiveIntFromEnv(env, PROCESS_TIMEOUT_ENV, DEFAULT_PROCESS_TIMEOUT_MS);
    const killGraceMs = positiveIntFromEnv(env, KILL_GRACE_ENV, DEFAULT_KILL_GRACE_MS);

    const args = ["transcribe", "--input", audioPath, "--timeout", String(RECOGNITION_TIMEOUT_SECONDS)];
    if (input.language) args.push("--locale", String(input.language));

    // 外部进程 wire 不透明（同 external_cli_media 语义）：向观测层显式标注，
    // 不伪装出 provider HTTP 请求/响应捕获。
    input.modelCall?.payloadCapture?.noteProviderWireUnavailable?.("provider_request", {
      reason: "external-process-opaque",
      visibility: "opaque",
      fidelity: "external_process",
    });
    input.modelCall?.payloadCapture?.noteProviderWireUnavailable?.("provider_response", {
      reason: "external-process-opaque",
      visibility: "opaque",
      fidelity: "external_process",
    });

    return await new Promise<SystemSpeechTranscription>((resolve, reject) => {
      let settlement: Settlement | null = null;
      let promiseSettled = false;
      let child: ReturnType<typeof spawn> | null = null;
      let childClosed = false;
      let stdoutBytes = 0;
      let stdoutBuffer = "";
      let stderrBuffer = "";
      let pendingLine = "";
      let timeoutTimer: NodeJS.Timeout | null = null;
      let killTimer: NodeJS.Timeout | null = null;

      const resultFromPayload = (parsed: any) => {
        const text = String(parsed?.text ?? "").trim();
        return {
          text,
          ...(typeof parsed?.resultCode === "string" && parsed.resultCode
            ? { resultCode: parsed.resultCode }
            : (text ? {} : { resultCode: "EMPTY_RESULT" })),
          ...(input.language ? { language: input.language } : {}),
          ...(parsed?.durationMs ? { durationMs: Number(parsed.durationMs) } : {}),
        };
      };

      const settlePromise = () => {
        if (!settlement || promiseSettled) return;
        promiseSettled = true;
        if (timeoutTimer) { clearTimeout(timeoutTimer); timeoutTimer = null; }
        if (killTimer) { clearTimeout(killTimer); killTimer = null; }
        if (signal) signal.removeEventListener("abort", onAbort);
        if (settlement.kind === "result") {
          resolve(settlement.value);
        } else {
          reject(settlement.error);
        }
      };

      const finish = (outcome: Settlement) => {
        if (settlement) return;
        settlement = outcome;
        // 终局已定：进程级截止不再适用（错误终局的有界终止由 kill 宽限接管）。
        if (timeoutTimer) { clearTimeout(timeoutTimer); timeoutTimer = null; }
        if (outcome.kind === "result") {
          // 结构化终局已到：立即结算给调用方；子进程由后台回收链有界清理。
          settlePromise();
          reapAfterSettlement();
          return;
        }
        if (!child || childClosed) {
          settlePromise();
          return;
        }
        // 错误终局：先有界终止同一个子进程，确认退出后再结算（不留孤儿窗口）。
        terminateChild();
      };

      const reapAfterSettlement = () => {
        if (!child || childClosed) return;
        killTimer = setTimeout(() => {
          if (childClosed || !child) return;
          try { child.kill("SIGTERM"); } catch { /* 已退出 */ }
          killTimer = setTimeout(() => {
            if (childClosed || !child) return;
            try { child.kill("SIGKILL"); } catch { /* 已退出 */ }
          }, killGraceMs);
        }, killGraceMs);
      };

      const terminateChild = () => {
        if (!child || childClosed) return;
        try { child.kill("SIGTERM"); } catch { /* 已退出 */ }
        killTimer = setTimeout(() => {
          if (childClosed || !child) return;
          try { child.kill("SIGKILL"); } catch { /* 已退出 */ }
        }, killGraceMs);
      };

      // 用 close 而非 exit 做终局推导：close 保证 stdio 数据已全部交付，
      // 避免「exit 先到、stdout 尾包后到」的解析竞态。
      const onChildClose = (code: number | null, exitSignal: string | null) => {
        if (childClosed) return;
        childClosed = true;
        if (killTimer) { clearTimeout(killTimer); killTimer = null; }
        if (!settlement) {
          const lastLine = lastNonEmptyLine(stdoutBuffer);
          const parsed = parseProtocolLine(lastLine);
          if (parsed?.ok === true) {
            settlement = { kind: "result", value: resultFromPayload(parsed) };
          } else if (parsed?.ok === false) {
            settlement = { kind: "error", error: errorFromStructured(parsed) };
          } else if (code === 0) {
            settlement = {
              kind: "error",
              error: speechError(
                SYSTEM_SPEECH_ERROR_CODES.INVALID_OUTPUT,
                `helper 退出码 0 但输出无法解析: ${lastLine.trim().slice(0, 200) || "(empty stdout)"}`,
              ),
            };
          } else {
            settlement = {
              kind: "error",
              error: errorFromLegacyStderr(
                stderrBuffer || `helper exited with code ${code ?? "null"}${exitSignal ? ` signal ${exitSignal}` : ""}`,
              ),
            };
          }
        }
        settlePromise();
      };

      const settleFromStdoutLine = (line: string) => {
        if (settlement) return;
        const parsed = parseProtocolLine(line);
        if (!parsed) return;
        if (parsed.ok === true) {
          finish({ kind: "result", value: resultFromPayload(parsed) });
        } else {
          finish({ kind: "error", error: errorFromStructured(parsed) });
        }
      };

      const onStdout = (chunk: Buffer) => {
        if (settlement) return;
        stdoutBytes += chunk.length;
        if (stdoutBytes > MAX_STDOUT_BYTES) {
          finish({
            kind: "error",
            error: speechError(
              SYSTEM_SPEECH_ERROR_CODES.PROCESS_FAILED,
              `helper stdout 超过 ${MAX_STDOUT_BYTES} 字节上限`,
            ),
          });
          return;
        }
        const text = chunk.toString("utf-8");
        stdoutBuffer += text;
        pendingLine += text;
        let newlineIndex = pendingLine.indexOf("\n");
        while (newlineIndex >= 0) {
          const line = pendingLine.slice(0, newlineIndex);
          pendingLine = pendingLine.slice(newlineIndex + 1);
          settleFromStdoutLine(line);
          if (settlement) return;
          newlineIndex = pendingLine.indexOf("\n");
        }
      };

      const onStderr = (chunk: Buffer) => {
        if (stderrBuffer.length < 64 * 1024) {
          stderrBuffer += chunk.toString("utf-8");
        }
      };

      const onAbort = () => {
        finish({
          kind: "error",
          error: speechError(
            SYSTEM_SPEECH_ERROR_CODES.CANCELLED,
            DEFAULT_ERROR_MESSAGES[SYSTEM_SPEECH_ERROR_CODES.CANCELLED],
          ),
        });
      };

      try {
        child = spawn(helper, args, {
          env,
          stdio: ["ignore", "pipe", "pipe"],
        });
      } catch (err: any) {
        finish({
          kind: "error",
          error: speechError(
            SYSTEM_SPEECH_ERROR_CODES.PROCESS_FAILED,
            `helper 启动失败: ${err?.message || err}`,
          ),
        });
        return;
      }

      child.on("error", (err: any) => {
        // spawn 失败（如 helper 在存在性校验后被删除）：子进程从未运行，
        // 标记关闭以便 finish 立即结算（failed spawn 不保证派发 close）。
        childClosed = true;
        const notFound = err?.code === "ENOENT";
        finish({
          kind: "error",
          error: speechError(
            notFound ? SYSTEM_SPEECH_ERROR_CODES.HELPER_NOT_FOUND : SYSTEM_SPEECH_ERROR_CODES.PROCESS_FAILED,
            `helper 启动失败: ${err?.message || err}`,
          ),
        });
      });
      child.on("close", onChildClose);
      child.stdout?.on("data", onStdout);
      child.stderr?.on("data", onStderr);

      if (signal) signal.addEventListener("abort", onAbort, { once: true });

      timeoutTimer = setTimeout(() => {
        finish({
          kind: "error",
          error: speechError(
            SYSTEM_SPEECH_ERROR_CODES.TIMEOUT,
            `系统语音识别超过进程上限 ${Math.round(processTimeoutMs / 1000)}s`,
          ),
        });
      }, processTimeoutMs);
    }).catch((err) => {
      if (err instanceof SystemSpeechError) {
        log.warn(`transcribe failed: [${err.code}] ${err.message.slice(0, 300)}`);
      }
      throw err;
    });
  },
};
