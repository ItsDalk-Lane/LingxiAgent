/**
 * F7/P9 X05：录音/转写进行时，另一会话的流式回答不受影响。
 *
 * system-speech 适配器是异步 spawn（F6）：识别子进程在途时事件循环不被阻塞，
 * chat 路由对其他会话的事件照常投影。用假 helper（sleep 模式）拖住在途识别，
 * 同窗口内驱动另一会话的真实 createChatRoute 事件链并断言先完成。
 */

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { systemSpeechRecognitionAdapter } from "../core/speech-recognition/system-speech-adapter.ts";
import { createChatRoute } from "../server/routes/chat.ts";

const SLEEP_HELPER = String.raw`#!/usr/bin/env node
const fs = require("node:fs");
if (process.env.FAKE_HELPER_PID_FILE) {
  fs.writeFileSync(process.env.FAKE_HELPER_PID_FILE, String(process.pid));
}
setTimeout(() => {
  process.stdout.write(JSON.stringify({ ok: true, protocol: 2, text: "迟到结果" }) + "\n");
  process.exit(0);
}, 400);
`;

let tmpDir: string;

function fakeAudioFile(): string {
  const filePath = path.join(tmpDir, "voice.wav");
  fs.writeFileSync(filePath, "RIFF");
  return filePath;
}

describe("X05: 转写在途不阻塞其他会话的流式回答", () => {
  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-x05-"));
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  it("识别子进程睡眠期间，另一会话的 mood/answer 事件链先完成", async () => {
    const helperPath = path.join(tmpDir, "fake-helper.cjs");
    fs.writeFileSync(helperPath, SLEEP_HELPER);
    fs.chmodSync(helperPath, 0o755);

    // 在途识别：sleep 模式假 helper（400ms）
    const transcribePromise = systemSpeechRecognitionAdapter.transcribe({
      file: { filePath: fakeAudioFile(), realPath: fakeAudioFile() },
      provider: { id: "system-speech" },
      model: { id: "system-speech" },
      credentials: {},
    }, {
      env: {
        ...process.env,
        LINGXI_SPEECH_HELPER_EXEC: helperPath,
        FAKE_HELPER_PID_FILE: path.join(tmpDir, "helper.pid"),
      },
      platform: "darwin",
    } as never);

    // 同窗口内：另一会话的真实 chat 路由事件链
    let createHandlers;
    let subscriber;
    const upgradeWebSocket = vi.fn((factory) => {
      createHandlers = factory;
      return () => new Response(null);
    });
    const hub = {
      subscribe: vi.fn((fn) => { subscriber = fn; }),
      send: vi.fn(async () => {}),
      eventBus: { emit: vi.fn() },
    };
    const engine = {
      agentName: "Ming",
      abortAllStreaming: vi.fn(async () => {}),
      getSessionByPath: vi.fn(() => ({ entries: [] })),
      isSessionStreaming: vi.fn(() => false),
      isSessionSwitching: vi.fn(() => false),
      steerSession: vi.fn(() => false),
      slashDispatcher: null,
    };
    createChatRoute(engine, hub, { upgradeWebSocket });
    const handlers = createHandlers({});
    const ws = { readyState: 1, send: vi.fn() };
    handlers.onOpen({}, ws);

    const otherPath = "/tmp/x05-other.jsonl";
    const msg = {
      role: "assistant",
      api: "anthropic-messages",
      content: [{ type: "text", text: "<mood>并行回答</mood>正文" }],
    };
    const startedAt = Date.now();
    subscriber?.({ type: "agent_start" }, otherPath);
    subscriber?.({ type: "turn_start" }, otherPath);
    subscriber?.({ type: "message_start", message: msg }, otherPath);
    subscriber?.({
      type: "message_update", message: msg,
      assistantMessageEvent: { type: "text_delta", contentIndex: 0, delta: "<mood>并行回答</mood>正文", partial: msg },
    }, otherPath);
    subscriber?.({
      type: "message_update", message: msg,
      assistantMessageEvent: { type: "text_end", contentIndex: 0, content: "<mood>并行回答</mood>正文", partial: msg },
    }, otherPath);
    subscriber?.({ type: "message_end", message: msg }, otherPath);
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, otherPath);
    subscriber?.({ type: "agent_settled" }, otherPath);

    // 事件链在识别完成之前就完成投递（事件循环未被 helper 阻塞）
    const payloadTypes = ws.send.mock.calls.map(([raw]) => JSON.parse(String(raw)).type);
    expect(payloadTypes).toContain("mood_start");
    expect(payloadTypes).toContain("text_delta");
    expect(Date.now() - startedAt).toBeLessThan(400);

    const result = await transcribePromise;
    expect(result.text).toBe("迟到结果");
  });
});
