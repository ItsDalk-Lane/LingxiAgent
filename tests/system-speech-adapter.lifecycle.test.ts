/**
 * A06–A12：system-speech 适配器的异步子进程生命周期。
 *
 * 用合成的假 helper（node 脚本）替代真实 Swift 二进制：覆盖正常返回、单次
 * 结算、超时/取消杀子进程、事件循环不阻塞、错误码分类与打包路径解析合同。
 */
import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  systemSpeechRecognitionAdapter,
  resolveSystemSpeechHelperPath,
  SYSTEM_SPEECH_ERROR_CODES,
} from "../core/speech-recognition/system-speech-adapter.ts";

let tmpDir: string;
let audioFile: string;
let fakeHelper: string;

const FAKE_HELPER_SOURCE = String.raw`#!/usr/bin/env node
// 假 helper：按 FAKE_HELPER_MODE 模拟各种行为。
const fs = require("node:fs");
const args = process.argv.slice(2);
const mode = process.env.FAKE_HELPER_MODE || "ok";
if (process.env.FAKE_HELPER_PID_FILE) {
  fs.writeFileSync(process.env.FAKE_HELPER_PID_FILE, String(process.pid));
}
function out(obj) { process.stdout.write(JSON.stringify(obj) + "\n"); }
function writeBytes(stream, value, done) {
  const bytes = Buffer.from(value, "utf8");
  let index = 0;
  const step = () => {
    if (index >= bytes.length) { done(); return; }
    stream.write(bytes.subarray(index, index + 1));
    index += 1;
    setTimeout(step, 1);
  };
  step();
}
switch (mode) {
  case "ok":
    out({ ok: true, protocol: 2, text: "你好世界", durationMs: 12 });
    process.exit(0);
    break;
  case "empty":
    out({ ok: true, protocol: 2, text: "", resultCode: "EMPTY_RESULT" });
    process.exit(0);
    break;
  case "ok-then-noise":
    // 结果之后又吐错误并以非零退出：调用方只能结算一次（以首个终局为准）。
    out({ ok: true, protocol: 2, text: "先到结果" });
    setTimeout(() => {
      process.stderr.write("late failure after result\n");
      process.exit(3);
    }, 60);
    break;
  case "sleep":
    setTimeout(() => { out({ ok: true, protocol: 2, text: "late" }); process.exit(0); }, 5000);
    break;
  case "ignore-term":
    process.on("SIGTERM", () => {});
    setTimeout(() => {}, 30000);
    break;
  case "bad-json":
    process.stdout.write("not a json line\n");
    process.exit(0);
    break;
  case "big-stdout":
    // write 回调里再 exit：process.exit 会截断尚在冲刷的大块写入。
    process.stdout.write("x".repeat(8 * 1024 * 1024), () => process.exit(0));
    break;
  case "exit-json-error":
    out({ ok: false, protocol: 2, code: "PERMISSION_DENIED", message: "denied by user" });
    process.exit(2);
    break;
  case "exit-stderr-legacy":
    process.stderr.write("lingxi-speech-helper error: speech recognition permission denied\n");
    process.exit(2);
    break;
  case "bytewise-utf8":
    writeBytes(process.stdout, JSON.stringify({ ok: true, protocol: 2, text: process.env.FAKE_HELPER_TEXT || "你好🙂é中英�" }) + "\n", () => process.exit(0));
    break;
  case "bytewise-no-newline":
    writeBytes(process.stdout, JSON.stringify({ ok: true, protocol: 2, text: "无换行结尾🙂" }), () => process.exit(0));
    break;
  case "non-string-text":
    out({ ok: true, protocol: 2, text: { forged: "success" } });
    process.exit(0);
    break;
  case "bytewise-stderr":
    writeBytes(process.stderr, "中文错误：识别器不可用", () => process.exit(2));
    break;
  default:
    process.stderr.write("unknown mode " + mode + "\n");
    process.exit(9);
}
`;

function makeInput(overrides: Record<string, unknown> = {}) {
  return {
    file: { filePath: audioFile, realPath: audioFile, mime: "audio/wav", size: 4 },
    provider: { id: "system-speech", baseUrl: null },
    model: { id: "system-speech-recognition", protocolId: "system-speech-recognition" },
    credentials: {},
    language: "zh-CN",
    ...overrides,
  };
}

function helperEnv(mode: string, extra: Record<string, string> = {}) {
  return { ...process.env, FAKE_HELPER_MODE: mode, ...extra };
}

// 等假 helper 真正起来（pid 落盘）再驱动取消/超时——node 启动有几十毫秒
// 方差，固定 sleep 会打中尚未执行脚本入口的进程，造成假阴性。
async function waitForPidFile(pidFile: string, deadlineMs = 5000): Promise<number> {
  const started = Date.now();
  for (;;) {
    try {
      const pid = Number(fs.readFileSync(pidFile, "utf-8"));
      if (Number.isInteger(pid) && pid > 0) return pid;
    } catch {
      // 尚未落盘
    }
    if (Date.now() - started > deadlineMs) {
      throw new Error(`helper pid file not written within ${deadlineMs}ms: ${pidFile}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

beforeEach(() => {
  tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-speech-adapter-"));
  audioFile = path.join(tmpDir, "voice.wav");
  fs.writeFileSync(audioFile, "RIFF");
  fakeHelper = path.join(tmpDir, "fake-helper.cjs");
  fs.writeFileSync(fakeHelper, FAKE_HELPER_SOURCE);
  fs.chmodSync(fakeHelper, 0o755);
  process.env.LINGXI_SPEECH_HELPER_EXEC = fakeHelper;
});

afterEach(() => {
  delete process.env.LINGXI_SPEECH_HELPER_EXEC;
  delete process.env.LINGXI_SPEECH_PROCESS_TIMEOUT_MS;
  delete process.env.LINGXI_SPEECH_KILL_GRACE_MS;
  delete process.env.LINGXI_DESKTOP_IS_PACKAGED;
  delete process.env.LINGXI_DESKTOP_RESOURCES_PATH;
  fs.rmSync(tmpDir, { recursive: true, force: true });
});

describe("system speech adapter lifecycle（A06–A12）", () => {
  it("R07-02/03/06：真实异步 helper 逐字节写 UTF-8，复杂正文无损且只结算一次", async () => {
    let settleCount = 0;
    const result = await systemSpeechRecognitionAdapter.transcribe(makeInput(), {
      platform: "darwin", env: helperEnv("bytewise-utf8"),
    }).then(value => {
      settleCount += 1;
      return value;
    });
    expect(result.text).toBe("你好🙂é中英�");
    expect(settleCount).toBe(1);
  });

  it("R07-04：真实 helper 的无末尾换行 JSON 在 close 边界完整解码", async () => {
    const result = await systemSpeechRecognitionAdapter.transcribe(makeInput(), {
      platform: "darwin", env: helperEnv("bytewise-no-newline"),
    });
    expect(result.text).toBe("无换行结尾🙂");
  });

  it("R07-05：结构化成功中的非字符串 text 明确 INVALID_OUTPUT", async () => {
    await expect(systemSpeechRecognitionAdapter.transcribe(makeInput(), {
      platform: "darwin", env: helperEnv("non-string-text"),
    })).rejects.toMatchObject({ code: SYSTEM_SPEECH_ERROR_CODES.INVALID_OUTPUT });
  });

  it("R07-08：中文 stderr 跨字节块仍保持可读且按失败结算", async () => {
    await expect(systemSpeechRecognitionAdapter.transcribe(makeInput(), {
      platform: "darwin", env: helperEnv("bytewise-stderr"),
    })).rejects.toMatchObject({
      code: SYSTEM_SPEECH_ERROR_CODES.PROCESS_FAILED,
      message: expect.stringContaining("中文错误：识别器不可用"),
    });
  });

  it("R07-07：两个真实并发 transcribe 的解码与缓冲互不污染", async () => {
    const [left, right] = await Promise.all([
      systemSpeechRecognitionAdapter.transcribe(makeInput(), {
        platform: "darwin", env: helperEnv("bytewise-utf8", { FAKE_HELPER_TEXT: "左🙂通道" }),
      }),
      systemSpeechRecognitionAdapter.transcribe(makeInput(), {
        platform: "darwin", env: helperEnv("bytewise-utf8", { FAKE_HELPER_TEXT: "右🌸通道" }),
      }),
    ]);
    expect(left.text).toBe("左🙂通道");
    expect(right.text).toBe("右🌸通道");
  });

  it("A06：helper 正常 final 返回——不等待超时，stdout 可解析", async () => {
    const started = Date.now();
    const result = await systemSpeechRecognitionAdapter.transcribe(makeInput(), {
      platform: "darwin", env: helperEnv("ok"),
    });
    expect(result.text).toBe("你好世界");
    expect(Date.now() - started).toBeLessThan(4000);
  });

  it("A06b：空文本是明确的 EMPTY_RESULT，不伪造文本", async () => {
    const result = await systemSpeechRecognitionAdapter.transcribe(makeInput(), {
      platform: "darwin", env: helperEnv("empty"),
    });
    expect(result.text).toBe("");
    expect(result.resultCode).toBe("EMPTY_RESULT");
  });

  it("A07：结果后晚到错误/非零退出——只结算一次", async () => {
    const result = await systemSpeechRecognitionAdapter.transcribe(makeInput(), {
      platform: "darwin", env: helperEnv("ok-then-noise"),
    });
    expect(result.text).toBe("先到结果");
    // 给晚到的 stderr/exit 留时间：不应有第二次结算（unhandled rejection 会让 vitest 失败）。
    await new Promise((resolve) => setTimeout(resolve, 300));
  });

  it("A08：超时与外部取消都终止子进程且不输出成功", async () => {
    const pidFile = path.join(tmpDir, "helper.pid");
    // 1200ms：给假 helper（新写入的脚本）的 node 冷启动留出 macOS 文件扫描的
    // 余量（实测脚本首次执行偶发数秒级扫描延迟）；进程超时上限断言（<4s）不变。
    process.env.LINGXI_SPEECH_PROCESS_TIMEOUT_MS = "1200";
    const started = Date.now();
    await expect(systemSpeechRecognitionAdapter.transcribe(makeInput(), {
      platform: "darwin", env: helperEnv("sleep", { FAKE_HELPER_PID_FILE: pidFile }),
    })).rejects.toMatchObject({ code: SYSTEM_SPEECH_ERROR_CODES.TIMEOUT });
    expect(Date.now() - started).toBeLessThan(4000);
    const pid = Number(fs.readFileSync(pidFile, "utf-8"));
    expect(() => process.kill(pid, 0)).toThrow();

    // 外部 AbortSignal 取消：CANCELLED，子进程同样被终止。
    const pidFile2 = path.join(tmpDir, "helper2.pid");
    const controller = new AbortController();
    const pending = systemSpeechRecognitionAdapter.transcribe(makeInput({ signal: controller.signal }), {
      platform: "darwin", env: helperEnv("sleep", { FAKE_HELPER_PID_FILE: pidFile2 }),
    });
    const pid2 = await waitForPidFile(pidFile2);
    controller.abort();
    await expect(pending).rejects.toMatchObject({ code: SYSTEM_SPEECH_ERROR_CODES.CANCELLED });
    expect(() => process.kill(pid2, 0)).toThrow();
  });

  it("A09：helper 慢速在途时事件循环不被阻塞（非 execFileSync）", async () => {
    const controller = new AbortController();
    const pending = systemSpeechRecognitionAdapter.transcribe(makeInput({ signal: controller.signal }), {
      platform: "darwin", env: helperEnv("sleep"),
    });
    // 在 helper 挂起期间，一个 60ms 的计时器必须按时触发。
    const timerStart = Date.now();
    await new Promise((resolve) => setTimeout(resolve, 60));
    const timerElapsed = Date.now() - timerStart;
    expect(timerElapsed).toBeLessThan(1000);
    // 取消在途转写，避免 5s 尾巴。
    controller.abort();
    await expect(pending).rejects.toMatchObject({ code: SYSTEM_SPEECH_ERROR_CODES.CANCELLED });
  });

  it("A10：子进程无视温和终止时，有界等待后强制结束同一进程", async () => {
    const pidFile = path.join(tmpDir, "helper.pid");
    process.env.LINGXI_SPEECH_KILL_GRACE_MS = "150";
    const controller = new AbortController();
    const started = Date.now();
    const pending = systemSpeechRecognitionAdapter.transcribe(makeInput({ signal: controller.signal }), {
      platform: "darwin", env: helperEnv("ignore-term", { FAKE_HELPER_PID_FILE: pidFile }),
    });
    const pid = await waitForPidFile(pidFile);
    controller.abort();
    await expect(pending).rejects.toMatchObject({ code: SYSTEM_SPEECH_ERROR_CODES.CANCELLED });
    // SIGTERM 宽限 + SIGKILL：有界，远小于无限等待。
    expect(Date.now() - started).toBeLessThan(5000);
    expect(() => process.kill(pid, 0)).toThrow();
  });

  it("A11：helper 缺失/坏 JSON/超大 stdout/结构化错误/旧式 stderr 各自独立错误码", async () => {
    // helper 不存在
    await expect(systemSpeechRecognitionAdapter.transcribe(makeInput(), {
      platform: "darwin", env: helperEnv("ok", { LINGXI_SPEECH_HELPER_EXEC: path.join(tmpDir, "missing-helper") }),
    })).rejects.toMatchObject({ code: SYSTEM_SPEECH_ERROR_CODES.HELPER_NOT_FOUND });

    // 坏 JSON：退出码 0 但输出不可解析
    await expect(systemSpeechRecognitionAdapter.transcribe(makeInput(), {
      platform: "darwin", env: helperEnv("bad-json"),
    })).rejects.toMatchObject({ code: SYSTEM_SPEECH_ERROR_CODES.INVALID_OUTPUT });

    // 超大 stdout（超过 maxBuffer）
    await expect(systemSpeechRecognitionAdapter.transcribe(makeInput(), {
      platform: "darwin", env: helperEnv("big-stdout"),
    })).rejects.toMatchObject({ code: SYSTEM_SPEECH_ERROR_CODES.PROCESS_FAILED });

    // 结构化错误（protocol 2）：精确错误码
    await expect(systemSpeechRecognitionAdapter.transcribe(makeInput(), {
      platform: "darwin", env: helperEnv("exit-json-error"),
    })).rejects.toMatchObject({ code: SYSTEM_SPEECH_ERROR_CODES.PERMISSION_DENIED });

    // 旧式 stderr 字符串：兼容层映射（非主协议）
    await expect(systemSpeechRecognitionAdapter.transcribe(makeInput(), {
      platform: "darwin", env: helperEnv("exit-stderr-legacy"),
    })).rejects.toMatchObject({ code: SYSTEM_SPEECH_ERROR_CODES.PERMISSION_DENIED });
  });

  it("A12：打包形态按桌面路径合同解析 Resources，不回退 dev 残留；缺失即安装损坏", () => {
    // 构造假打包形态：Resources/speech/macos/lingxi-speech-helper
    const appRoot = path.join(tmpDir, "Lingxi.app");
    const resources = path.join(appRoot, "Contents", "Resources");
    const helperPath = path.join(resources, "speech", "macos", "lingxi-speech-helper");
    fs.mkdirSync(path.dirname(helperPath), { recursive: true });
    fs.writeFileSync(helperPath, "#!/bin/sh\n");
    fs.chmodSync(helperPath, 0o755);

    // server 进程的 execPath 是 node（与 Electron 无关）：不得据此推导。
    const packagedEnv = {
      LINGXI_DESKTOP_IS_PACKAGED: "1",
      LINGXI_DESKTOP_RESOURCES_PATH: resources,
    };
    // 路径合同：解析结果统一 POSIX 斜杠形态（Windows 输入反斜杠也归一）。
    expect(resolveSystemSpeechHelperPath({ env: packagedEnv })).toBe(helperPath.replace(/\\/g, "/"));

    // 打包但 Resources 缺失 helper：明确 HELPER_NOT_FOUND（安装损坏），
    // 且不得回退到 cwd 下偶然残留的 dev helper。
    const emptyResources = path.join(tmpDir, "Other.app", "Contents", "Resources");
    fs.mkdirSync(emptyResources, { recursive: true });
    const devLeftoverRoot = path.join(tmpDir, "devroot");
    const devHelper = path.join(devLeftoverRoot, "dist-speech", `mac-${process.arch === "x64" ? "x64" : "arm64"}`, "lingxi-speech-helper");
    fs.mkdirSync(path.dirname(devHelper), { recursive: true });
    fs.writeFileSync(devHelper, "#!/bin/sh\n");
    expect(() => resolveSystemSpeechHelperPath({
      env: { LINGXI_DESKTOP_IS_PACKAGED: "1", LINGXI_DESKTOP_RESOURCES_PATH: emptyResources },
      cwd: devLeftoverRoot,
    })).toThrowError(expect.objectContaining({ code: SYSTEM_SPEECH_ERROR_CODES.HELPER_NOT_FOUND }));

    // 开发形态：才允许 dist-speech/mac-<arch>。
    const devResolved = resolveSystemSpeechHelperPath({ env: {}, cwd: devLeftoverRoot });
    expect(devResolved).toBe(devHelper.replace(/\\/g, "/"));

    // override 仅信任受控环境变量（启动环境注入），存在性校验。
    expect(resolveSystemSpeechHelperPath({
      env: { LINGXI_SPEECH_HELPER_EXEC: fakeHelper },
    })).toBe(fakeHelper.replace(/\\/g, "/"));
  });
});
