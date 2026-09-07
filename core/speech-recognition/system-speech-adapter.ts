/**
 * system-speech-adapter.ts — macOS 系统语音识别适配器（本地、免费、离线）
 *
 * 通过 LingxiSpeechHelper（Swift，SFSpeechRecognizer）把已录制的 WAV 文件
 * 转成文字。helper 路径解析与 computer-use helper 同一候选链：环境变量覆盖
 * → 打包 Resources → dev 的 dist-speech/。
 *
 * 授权模型：helper 绝不自己弹权限（裸进程弹不了，实测 SIGABRT）；打包形态下
 * TCC 把语音识别授权归属到宿主 Lingxi.app，未授权时 helper 显式失败，本适配器
 * 原样透出错误码（不静默降级）。
 */

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { createModuleLogger } from "../../lib/debug-log.ts";

const log = createModuleLogger("system-speech-asr");

const HELPER_ENV_OVERRIDE = "LINGXI_SPEECH_HELPER_EXEC";
const HELPER_RELATIVE_RESOURCES = ["speech", "macos", "lingxi-speech-helper"];

function resolveHelperCommand(): string {
  const override = process.env[HELPER_ENV_OVERRIDE];
  if (override && fs.existsSync(override)) return override;

  // 打包形态：LINGXI.app/Contents/Resources/speech/macos/lingxi-speech-helper
  // process.execPath = .../Contents/MacOS/Lingxi
  try {
    const execDir = path.dirname(process.execPath);
    const candidate = path.join(execDir, "..", "Resources", ...HELPER_RELATIVE_RESOURCES);
    if (fs.existsSync(candidate)) return candidate;
  } catch {
    // execPath 解析失败继续走 dev 候选
  }

  // dev 形态：仓库根 dist-speech/mac-<arch>/
  const arch = process.arch === "x64" ? "mac-x64" : "mac-arm64";
  const devCandidate = path.join(process.cwd(), "dist-speech", arch, "lingxi-speech-helper");
  if (fs.existsSync(devCandidate)) return devCandidate;
  const altDevCandidate = path.join(process.cwd(), "..", "..", "dist-speech", arch, "lingxi-speech-helper");
  if (fs.existsSync(altDevCandidate)) return altDevCandidate;

  throw new Error("lingxi-speech-helper binary not found; run `npm run build:speech-helper` (dev) or reinstall the app (packaged)");
}

export const systemSpeechRecognitionAdapter = {
  id: "system-speech",
  name: "System Speech Recognition",
  protocolId: "system-speech-recognition",
  types: ["speechRecognition"],

  async available(): Promise<boolean> {
    if (process.platform !== "darwin") return false;
    try {
      resolveHelperCommand();
      return true;
    } catch {
      return false;
    }
  },

  async transcribe(input: any) {
    if (process.platform !== "darwin") {
      throw new Error("system speech recognition is only available on macOS");
    }
    const helper = resolveHelperCommand();
    const audioPath = input.file?.realPath || input.file?.filePath;
    if (!audioPath) throw new Error("system speech recognition requires a recorded audio file");
    if (!fs.existsSync(audioPath)) throw new Error(`audio file missing: ${audioPath}`);

    const args = ["transcribe", "--input", audioPath, "--timeout", "120"];
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

    let stdout: string;
    try {
      stdout = execFileSync(helper, args, {
        encoding: "utf-8",
        timeout: 150_000,
        maxBuffer: 4 * 1024 * 1024,
      });
    } catch (err: any) {
      const stderr = String(err?.stderr || err?.message || "");
      // helper 的授权/可用性错误是结构化 fail-closed 消息，原样透出。
      log.warn(`transcribe failed: ${stderr.slice(0, 300)}`);
      if (stderr.includes("permission denied")) {
        throw new Error("SYSTEM_SPEECH_PERMISSION_DENIED: " + stderr.trim());
      }
      if (stderr.includes("permission not yet granted")) {
        throw new Error("SYSTEM_SPEECH_PERMISSION_REQUIRED: 请先在系统弹窗或「系统设置 → 隐私与安全性 → 语音识别」中授权灵犀");
      }
      throw new Error(`system speech recognition failed: ${stderr.trim().slice(0, 300) || "unknown error"}`);
    }

    let parsed: any;
    try {
      parsed = JSON.parse(stdout.trim().split("\n").pop() || "{}");
    } catch {
      throw new Error("system speech recognition returned an unparsable result");
    }
    if (!parsed?.ok) throw new Error("system speech recognition returned no result");
    return {
      text: String(parsed.text || "").trim(),
      ...(input.language ? { language: input.language } : {}),
      ...(parsed.durationMs ? { durationMs: Number(parsed.durationMs) } : {}),
    };
  },
};
