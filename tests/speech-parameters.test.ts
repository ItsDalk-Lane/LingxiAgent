/**
 * F11/P8.2：唯一语音参数解析（V01–V06）。
 *
 * 参数优先级固定：本次显式输入 > 该语音供应商默认参数（speech 域，按逻辑
 * provider 身份索引）> 当前语音协议既有默认值。模型身份只来自 execution
 * target；缺失/null/非法值/合法零值语义必须显式区分；云端适配路径不得读取
 * 图片 ctx.config。网络全部 stub，断言 URL/参数/凭证来源。
 */

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { UniversalMediaManager } from "../core/media/universal-media-manager.ts";
import { PreferencesManager } from "../core/preferences-manager.ts";
import {
  openaiSpeechAdapter,
  minimaxSpeechAdapter,
  dashscopeSpeechAdapter,
  buildSystemSpeechSayArgs,
} from "../core/media-adapters/speech.ts";
import { resolveSpeechParameters } from "../core/media/media-parameters.ts";
import { setModelCallObserver } from "../lib/llm/model-call-observer.ts";
import { setModelCallPayloadSink } from "../lib/llm/model-call-payload-capture.ts";
import { installTestPayloadSink } from "../lib/llm/model-call-payload-testing.ts";

const roots: string[] = [];
let payloadSink: ReturnType<typeof installTestPayloadSink>;

function makeRoot() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-speech-params-"));
  roots.push(root);
  return root;
}

function makePreferences(root: any, initial: any = null) {
  const userDir = path.join(root, "user");
  const agentsDir = path.join(root, "agents");
  fs.mkdirSync(userDir, { recursive: true });
  if (initial) {
    fs.writeFileSync(path.join(userDir, "preferences.json"), JSON.stringify(initial, null, 2));
  }
  return new PreferencesManager({ userDir, agentsDir });
}

function makeBus() {
  const handlers = new Map();
  return {
    handlers,
    handle: vi.fn((type: string, handler: any) => {
      handlers.set(type, handler);
      return () => handlers.delete(type);
    }),
    subscribe: vi.fn(() => () => {}),
    request: vi.fn(async (type: string) => {
      if (type === "provider:credentials") {
        return { apiKey: "sk-test", baseUrl: "https://api.example.com/v1" };
      }
      return {};
    }),
    emit: vi.fn(),
  };
}

function makeManager(root: string, preferences: any, providers: any[] = []) {
  return new UniversalMediaManager({
    lingxiHome: root,
    preferences,
    providerRegistry: {
      resolveMediaExecutionTarget: vi.fn((input: any) => Object.freeze({
        modelId: input.modelId,
        modality: input.modality,
        runtimeProviderId: input.runtimeProviderId,
        credentialProviderId: input.credentialLane?.providerId ?? input.runtimeProviderId,
        credentialLaneId: input.credentialLane?.id || null,
        credentialSource: "provider-registry",
        adapterId: input.adapterId || null,
      })),
      getMediaProviders: vi.fn((capability: string) => (
        capability === "speech_generation" ? providers : []
      )),
      resolveMediaModel: vi.fn(({ providerId, modelId }: any) => {
        const provider = providers.find((item: any) => item.providerId === providerId);
        const model = provider?.models?.find((m: any) => m.id === modelId)
          || provider?.availableModels?.find((m: any) => m.id === modelId);
        if (!model) throw new Error(`Media model "${providerId}/${modelId}" not found`);
        return {
          capability: "speech_generation",
          providerId,
          provider,
          model,
          credentialLane: model.credentialLaneId
            ? { id: model.credentialLaneId, providerId: "minimax" }
            : null,
        };
      }),
    },
    registerSessionFile: () => {},
  });
}

/** 在 generated 根目录里造一个真实产物文件，返回相对文件名。 */
function makeGeneratedFile(root: string, name: string): string {
  const dir = path.join(root, "plugin-data", "image-gen", "generated");
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, name), "audio-bytes");
  return name;
}

function speechProvider(providerId: string, models: any[]) {
  return { providerId, displayName: providerId, authType: "api-key", models, availableModels: models };
}

beforeEach(() => {
  payloadSink = installTestPayloadSink();
  setModelCallObserver({ handleModelCallEvent() {} });
  vi.stubGlobal("fetch", vi.fn(async () => new Response(new ArrayBuffer(8), { status: 200 })));
});

afterEach(() => {
  setModelCallObserver(null);
  setModelCallPayloadSink(null);
  vi.unstubAllGlobals();
  for (const root of roots.splice(0)) {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

describe("F11/P8.2 manager 级语音参数解析（speech 域、逻辑 provider 索引）", () => {
  it("V01: 同 provider 的图片默认值与语音默认值不同，TTS 只取语音域", async () => {
    const root = makeRoot();
    const preferences = makePreferences(root, {
      imageGeneration: {
        providerDefaults: { openai: { voice: "图片声线不存在", format: "png" } },
      },
      speechGeneration: {
        providerDefaults: { openai: { voice: "echo", speed: 1.5 } },
      },
    });
    const file = makeGeneratedFile(root, "v01.mp3");
    const submit = vi.fn(async () => ({ taskId: "t-v01", files: [file] }));
    const manager = makeManager(root, preferences, [speechProvider("openai", [
      { id: "tts-1", protocolId: "openai-audio-speech" },
    ])]);
    const bus = makeBus();
    manager.start(bus);
    manager.registerAdapter({ id: "openai-speech", protocolId: "openai-audio-speech", types: ["speech"], submit });

    await manager.submitSpeech({
      input: { prompt: "hi", provider: "openai", model: "tts-1", delivery: { mode: "response" } },
    });

    expect(submit).toHaveBeenCalledTimes(1);
    const [params] = submit.mock.calls[0] as unknown as [Record<string, any>, unknown];
    expect(Object.isFrozen(params)).toBe(true);
    // 语音域默认生效；图片域的 voice/format 不得混入
    expect(params.voice).toBe("echo");
    expect(params.speed).toBe(1.5);
    expect(params.format).toBe("mp3");
    manager.stop();
  });

  it("V02: 语音缺省、图片域里有 voice/format 时不回退图片域", async () => {
    const root = makeRoot();
    const preferences = makePreferences(root, {
      imageGeneration: { providerDefaults: { openai: { voice: "nope", format: "wav", speed: 3 } } },
      speechGeneration: {},
    });
    const file = makeGeneratedFile(root, "v02.mp3");
    const submit = vi.fn(async () => ({ taskId: "t-v02", files: [file] }));
    const manager = makeManager(root, preferences, [speechProvider("openai", [
      { id: "tts-1", protocolId: "openai-audio-speech" },
    ])]);
    const bus = makeBus();
    manager.start(bus);
    manager.registerAdapter({ id: "openai-speech", protocolId: "openai-audio-speech", types: ["speech"], submit });

    await manager.submitSpeech({
      input: { prompt: "hi", provider: "openai", model: "tts-1", delivery: { mode: "response" } },
    });

    const [params] = submit.mock.calls[0] as unknown as [Record<string, any>, unknown];
    expect(params.voice).toBe("alloy");
    expect(params.speed).toBe(1);
    expect(params.format).toBe("mp3");
    manager.stop();
  });

  it("V03: 本次显式参数覆盖语音默认，TaskStore.params 与语义观测一致", async () => {
    const root = makeRoot();
    const preferences = makePreferences(root, {
      speechGeneration: { providerDefaults: { openai: { voice: "echo", speed: 1.5 } } },
    });
    const file = makeGeneratedFile(root, "v03.mp3");
    const submit = vi.fn(async () => ({ taskId: "t-v03", files: [file] }));
    const manager = makeManager(root, preferences, [speechProvider("openai", [
      { id: "tts-1", protocolId: "openai-audio-speech" },
    ])]);
    const bus = makeBus();
    manager.start(bus);
    manager.registerAdapter({ id: "openai-speech", protocolId: "openai-audio-speech", types: ["speech"], submit });

    const result = await manager.submitSpeech({
      input: { prompt: "hi", provider: "openai", model: "tts-1", voice: "fable", speed: 0.5, delivery: { mode: "response" } },
    });

    const [params] = submit.mock.calls[0] as unknown as [Record<string, any>, unknown];
    expect(params.voice).toBe("fable");
    expect(params.speed).toBe(0.5);
    // TaskStore.params 反映实际使用参数；模型身份不被默认参数改写
    const task = manager.getTask(result.tasks[0].taskId);
    expect(task.params).toBe(params);
    expect(task.params.voice).toBe("fable");
    expect(task.params.speed).toBe(0.5);
    expect(task.params.modelId).toBe("tts-1");
    expect(task.modelId).toBe("tts-1");
    const semantic = payloadSink.records.find((record) => record.kind === "semantic_request")!;
    expect((semantic.payload as any).parameters).toMatchObject({
      modelId: "tts-1",
      voice: "fable",
      speed: 0.5,
      format: "mp3",
    });
    manager.stop();
  });

  it("R08-01/09/10: manager→真实 OpenAI adapter 只解析一次，wire/任务/观测同值且只发一次请求", async () => {
    const root = makeRoot();
    const preferences = makePreferences(root, {
      speechGeneration: { providerDefaults: { openai: { model: "wrong-model", speed: 1.5 } } },
    });
    const manager = makeManager(root, preferences, [speechProvider("openai", [
      { id: "tts-real", protocolId: "openai-audio-speech" },
    ])]);
    const bus = makeBus();
    manager.start(bus);
    manager.registerAdapter(openaiSpeechAdapter);

    const result = await manager.submitSpeech({
      input: {
        prompt: "一次解析",
        provider: "openai",
        model: "tts-real",
        voice: "echo",
        speed: 99,
        format: "bogus",
        delivery: { mode: "response" },
      },
    });

    expect(fetch).toHaveBeenCalledTimes(1);
    const wire = JSON.parse((fetch as any).mock.calls[0][1].body);
    expect(wire).toMatchObject({ model: "tts-real", voice: "echo", speed: 4, response_format: "mp3" });
    const task = manager.getTask(result.tasks[0].taskId);
    expect(task.params).toMatchObject({ modelId: "tts-real", voice: "echo", speed: 4, format: "mp3" });
    expect(Object.isFrozen(task.params)).toBe(true);
    const semantic = payloadSink.records.find((record) => record.kind === "semantic_request")!;
    expect((semantic.payload as any).parameters).toMatchObject({
      modelId: "tts-real",
      voice: "echo",
      speed: 4,
      format: "mp3",
    });
    manager.stop();

    await openaiSpeechAdapter.submit(
      { prompt: "一次解析", modelId: "tts-real", voice: "echo", speed: 99, format: "bogus" },
      {
        dataDir: makeRoot(),
        bus,
        mediaExecutionTarget: {
          protocolId: "openai-audio-speech",
          modelId: "tts-real",
          credentialProviderId: "openai",
        },
      },
    );
    expect(fetch).toHaveBeenCalledTimes(2);
    const directWire = JSON.parse((fetch as any).mock.calls[1][1].body);
    expect(directWire).toEqual(wire);
  });

  it("R08-03/08: manager→真实 MiniMax adapter 的限幅、格式、groupId 与凭证 lane 同源", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({
      base_resp: { status_code: 0 },
      data: { audio: "0011" },
    }), { status: 200 })));
    const root = makeRoot();
    const preferences = makePreferences(root, {
      speechGeneration: { providerDefaults: { "minimax-token-plan": { model: "wrong", format: "wav" } } },
    });
    const manager = makeManager(root, preferences, [speechProvider("minimax-token-plan", [{
      id: "speech-target",
      protocolId: "minimax-tts",
      groupId: "group-target",
      credentialLaneId: "main",
    }])]);
    const bus = makeBus();
    manager.start(bus);
    manager.registerAdapter(minimaxSpeechAdapter);

    const result = await manager.submitSpeech({
      input: {
        prompt: "限幅",
        provider: "minimax-token-plan",
        model: "speech-target",
        speed: -9,
        delivery: { mode: "response" },
      },
    });
    expect(fetch).toHaveBeenCalledTimes(1);
    const wire = JSON.parse((fetch as any).mock.calls[0][1].body);
    expect(wire).toMatchObject({
      model: "speech-target",
      voice_setting: { voice_id: "male-qn-qingse", speed: 0.5 },
      audio_setting: { format: "wav" },
    });
    const task = manager.getTask(result.tasks[0].taskId);
    expect(task.params).toMatchObject({
      modelId: "speech-target",
      groupId: "group-target",
      credentialProviderId: "minimax",
      credentialLaneId: "main",
      speed: 0.5,
      format: "wav",
    });
    const semantic = payloadSink.records.find((record) => record.kind === "semantic_request")!;
    expect((semantic.payload as any).parameters).toMatchObject({
      modelId: "speech-target",
      groupId: "group-target",
      speed: 0.5,
      format: "wav",
    });
    manager.stop();
  });

  it("R08-04: manager→真实 DashScope adapter 不把未发送的 speed/format 记录成 wire 参数", async () => {
    const audioUrl = "https://cdn.example.com/tts.wav";
    vi.stubGlobal("fetch", vi.fn(async (url: any) => String(url).includes("multimodal-generation")
      ? new Response(JSON.stringify({ output: { audio: { url: audioUrl } } }), { status: 200 })
      : new Response(new ArrayBuffer(4), { status: 200 })));
    const root = makeRoot();
    const preferences = makePreferences(root);
    const manager = makeManager(root, preferences, [speechProvider("dashscope", [{
      id: "qwen-tts-real",
      protocolId: "dashscope-qwen-tts",
    }])]);
    const bus = makeBus();
    manager.start(bus);
    manager.registerAdapter(dashscopeSpeechAdapter);

    const result = await manager.submitSpeech({
      input: {
        prompt: "协议字段",
        provider: "dashscope",
        model: "qwen-tts-real",
        voice: "Serena",
        speed: 2,
        format: "mp3",
        delivery: { mode: "response" },
      },
    });
    expect(fetch).toHaveBeenCalledTimes(2);
    const wire = JSON.parse((fetch as any).mock.calls[0][1].body);
    expect(wire).toEqual({ model: "qwen-tts-real", input: { text: "协议字段", voice: "Serena" } });
    const task = manager.getTask(result.tasks[0].taskId);
    expect(task.params).not.toHaveProperty("speed");
    expect(task.params).toMatchObject({ format: "wav", formatMode: "protocol_output_default" });
    const semantic = payloadSink.records.find((record) => record.kind === "semantic_request")!;
    expect((semantic.payload as any).parameters).not.toHaveProperty("speed");
    expect((semantic.payload as any).parameters).toMatchObject({ format: "wav", formatMode: "protocol_output_default" });
    manager.stop();
  });

  it("V03b: null 不被 Number(null) 隐式吞掉——speed null 走默认缺失语义", async () => {
    const root = makeRoot();
    const preferences = makePreferences(root, {
      speechGeneration: { providerDefaults: { openai: { speed: 1.5 } } },
    });
    const file = makeGeneratedFile(root, "v03b.mp3");
    const submit = vi.fn(async () => ({ taskId: "t-v03b", files: [file] }));
    const manager = makeManager(root, preferences, [speechProvider("openai", [
      { id: "tts-1", protocolId: "openai-audio-speech" },
    ])]);
    const bus = makeBus();
    manager.start(bus);
    manager.registerAdapter({ id: "openai-speech", protocolId: "openai-audio-speech", types: ["speech"], submit });

    await manager.submitSpeech({
      input: { prompt: "hi", provider: "openai", model: "tts-1", speed: null, delivery: { mode: "response" } },
    });

    const [params] = submit.mock.calls[0] as unknown as [Record<string, any>, unknown];
    // 显式 null = 没有本次取值 → 用语音默认；绝不能被 Number(null)=0 吞成 0
    expect(params.speed).toBe(1.5);
    manager.stop();
  });

  it("V04: 多凭证 lane 时配置归属（逻辑 provider）与凭证归属（credentialProviderId）不混淆", async () => {
    const root = makeRoot();
    const preferences = makePreferences(root, {
      speechGeneration: { providerDefaults: { "minimax-token-plan": { voice: "male-qn-qingse" } } },
    });
    const file = makeGeneratedFile(root, "v04.mp3");
    const submit = vi.fn(async () => ({ taskId: "t-v04", files: [file] }));
    const provider = speechProvider("minimax-token-plan", [{
      id: "speech-02-hd",
      protocolId: "minimax-t2a-v2",
      groupId: "grp-1",
      credentialLaneId: "main",
    }]);
    const manager = makeManager(root, preferences, [provider]);
    const bus = makeBus();
    manager.start(bus);
    manager.registerAdapter({ id: "minimax-speech", protocolId: "minimax-t2a-v2", types: ["speech"], submit });

    await manager.submitSpeech({
      input: { prompt: "hi", provider: "minimax-token-plan", model: "speech-02-hd", delivery: { mode: "response" } },
    });

    const [params] = submit.mock.calls[0] as unknown as [Record<string, any>, unknown];
    // 语音默认按逻辑 provider 身份（minimax-token-plan）命中
    expect(params.voice).toBe("male-qn-qingse");
    // 凭证归属仍来自执行目标解析（lane 的 credentialProviderId），不被默认参数改写
    expect(params.credentialProviderId).toBe("minimax");
    expect(params.credentialLaneId).toBe("main");
    manager.stop();
  });

  it("V06: 系统 say 路径只用它实际支持的参数，不被云端默认声线污染", async () => {
    const root = makeRoot();
    const preferences = makePreferences(root, {
      speechGeneration: { providerDefaults: { openai: { voice: "alloy", speed: 2 } } },
    });
    const file = makeGeneratedFile(root, "v06.m4a");
    const submit = vi.fn(async () => ({ taskId: "t-v06", files: [file] }));
    const manager = makeManager(root, preferences, [speechProvider("system-speech", [
      { id: "system-speech-tts", protocolId: "system-speech" },
    ])]);
    const bus = makeBus();
    manager.start(bus);
    manager.registerAdapter({ id: "system-speech-tts", protocolId: "system-speech", types: ["speech"], submit });

    await manager.submitSpeech({
      input: { prompt: "hi", provider: "system-speech", model: "system-speech-tts", delivery: { mode: "response" } },
    });

    const [params] = submit.mock.calls[0] as unknown as [Record<string, any>, unknown];
    // openai 的云端默认不得渗透到系统协议
    expect(params.voiceMode).toBe("system_default");
    expect(params.voice).toBeUndefined();
    expect(params.rateMode).toBe("system_default");
    expect(params.rateWpm).toBeUndefined();
    expect(params.format).toBe("m4a");
    expect(params.modelId).toBe("system-speech-tts");
    manager.stop();
  });
});

describe("R08 唯一协议解析器反例", () => {
  const target = (protocolId: string, modelId: string, extra: Record<string, unknown> = {}) => ({
    protocolId,
    modelId,
    ...extra,
  });

  it("R08-01/02: OpenAI 非法格式回落、有限速度限幅，缺失/null/空串/错误类型不会被 Number 隐式转换", () => {
    const fast = resolveSpeechParameters({
      protocolId: "openai-audio-speech",
      executionTarget: target("openai-audio-speech", "tts-real"),
      explicitInput: { voice: "echo", speed: 99, format: "bogus" },
      speechProviderDefaults: { model: "must-not-win", speed: 1.5 },
    } as any);
    expect(fast).toEqual(expect.objectContaining({
      protocolId: "openai-audio-speech",
      modelId: "tts-real",
      voice: "echo",
      speed: 4,
      format: "mp3",
    }));
    expect(fast).not.toHaveProperty("model", "must-not-win");

    for (const speed of [undefined, null, "", "2", {}, Number.NaN, Number.POSITIVE_INFINITY]) {
      const resolved = resolveSpeechParameters({
        protocolId: "openai-audio-speech",
        executionTarget: target("openai-audio-speech", "tts-real"),
        explicitInput: { speed },
        speechProviderDefaults: {},
      } as any);
      expect(resolved.speed).toBe(1);
    }
    expect(resolveSpeechParameters({
      protocolId: "openai-audio-speech",
      executionTarget: target("openai-audio-speech", "tts-real"),
      explicitInput: { speed: 0 },
    } as any).speed).toBe(0.25);
  });

  it("R08-03/07/08: MiniMax 只取 speech 默认，保留执行目标模型与模型条目的 groupId", () => {
    const resolved = resolveSpeechParameters({
      protocolId: "minimax-t2a-v2",
      executionTarget: target("minimax-t2a-v2", "speech-target", { groupId: "group-target" }),
      explicitInput: { speed: -9 },
      speechProviderDefaults: { model: "wrong-model", voice: "female-yujie", format: "wav" },
    } as any);
    expect(resolved).toEqual(expect.objectContaining({
      modelId: "speech-target",
      groupId: "group-target",
      voice: "female-yujie",
      speed: 0.5,
      format: "wav",
    }));
  });

  it("R08-04: DashScope 不把未发送的 speed/format 记成已应用 wire 参数", () => {
    const resolved = resolveSpeechParameters({
      protocolId: "dashscope-qwen-tts",
      executionTarget: target("dashscope-qwen-tts", "qwen-tts-real"),
      explicitInput: { voice: "Serena", speed: 2, format: "mp3" },
    } as any);
    expect(resolved).toEqual(expect.objectContaining({
      modelId: "qwen-tts-real",
      voice: "Serena",
      format: "wav",
      formatMode: "protocol_output_default",
    }));
    expect(resolved).not.toHaveProperty("speed");
  });

  it("R08-05/06: system 默认选择不伪造 voice，显式小数 speed 只记录实际整数 WPM", () => {
    const defaulted = resolveSpeechParameters({
      protocolId: "system-speech",
      executionTarget: target("system-speech", "system-speech-tts"),
      explicitInput: {},
    } as any);
    expect(defaulted).toEqual(expect.objectContaining({
      voiceMode: "system_default",
      rateMode: "system_default",
      format: "m4a",
    }));
    expect(defaulted).not.toHaveProperty("voice");
    expect(defaulted).not.toHaveProperty("rateWpm");
    expect(defaulted).not.toHaveProperty("speed");

    const explicit = resolveSpeechParameters({
      protocolId: "system-speech",
      executionTarget: target("system-speech", "system-speech-tts"),
      explicitInput: { voice: " Ting-Ting ", speed: 1.234 },
    } as any);
    expect(explicit).toEqual(expect.objectContaining({
      voiceMode: "explicit",
      voice: "Ting-Ting",
      rateMode: "explicit",
      rateWpm: 216,
      format: "m4a",
    }));
    expect(explicit).not.toHaveProperty("speed");
    expect(buildSystemSpeechSayArgs({ prompt: " 你好 ", ...defaulted }, "/tmp/default.m4a"))
      .toEqual(["-o", "/tmp/default.m4a", "你好"]);
    expect(buildSystemSpeechSayArgs({ prompt: " 你好 ", ...explicit }, "/tmp/explicit.m4a"))
      .toEqual(["-o", "/tmp/explicit.m4a", "-v", "Ting-Ting", "-r", "216", "你好"]);
  });
});

describe("F11/P8.2 云端适配路径不再读取图片 ctx.config（V05）", () => {
  function forbiddenCtx(adapterId: string, overrides: Record<string, unknown> = {}) {
    return {
      dataDir: makeRoot(),
      bus: makeBus(),
      generatedDir: "/tmp/generated",
      // 任何读取都是违规：F11 明令 speech 适配器不得再读图片配置
      config: {
        get() {
          throw new Error("speech adapter must not read image ctx.config");
        },
      },
      mediaExecutionTarget: { credentialProviderId: "openai", runtimeProviderId: "openai", adapterId },
      ...overrides,
    };
  }

  it("openai 路径：参数全部来自显式 params，不触 config；模型不被默认参数改写", async () => {
    const body = await openaiSpeechAdapter.submit(
      { prompt: "hi", modelId: "tts-1", voice: "fable", format: "flac", speed: 0.5, signal: null },
      forbiddenCtx("openai-speech"),
    );
    expect(body.files).toHaveLength(1);
    const call = (fetch as any).mock.calls.at(-1);
    expect(call[0]).toContain("/audio/speech");
    const payload = JSON.parse(call[1].body);
    expect(payload).toMatchObject({ model: "tts-1", voice: "fable", response_format: "flac", speed: 0.5 });
    expect(payload.input).toBe("hi");
  });

  it("minimax 路径：不触 config，speed 进入 voice_setting", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({
      base_resp: { status_code: 0 },
      data: { audio: "0011" },
    }), { status: 200 })));
    const result = await minimaxSpeechAdapter.submit(
      { prompt: "hi", modelId: "speech-02-hd", groupId: "grp-1", voice: "male-qn-qingse", format: "wav", speed: 1.25, signal: null },
      forbiddenCtx("minimax-speech"),
    );
    expect(result.files).toHaveLength(1);
    const call = (fetch as any).mock.calls.at(-1);
    expect(call[0]).toContain("/v1/t2a_v2?GroupId=grp-1");
    const payload = JSON.parse(call[1].body);
    expect(payload.model).toBe("speech-02-hd");
    expect(payload.voice_setting).toEqual({ voice_id: "male-qn-qingse", speed: 1.25 });
    expect(payload.audio_setting.format).toBe("wav");
  });

  it("dashscope 路径：不触 config，voice 显式传入", async () => {
    const audioUrl = "https://cdn.example.com/a.wav";
    let audioFetched = false;
    vi.stubGlobal("fetch", vi.fn(async (url: any) => {
      if (String(url).includes("multimodal-generation")) {
        return new Response(JSON.stringify({ output: { audio: { url: audioUrl } } }), { status: 200 });
      }
      audioFetched = true;
      return new Response(new ArrayBuffer(4), { status: 200 });
    }));
    await dashscopeSpeechAdapter.submit(
      { prompt: "hi", modelId: "qwen-tts-latest", voice: "Serena", signal: null },
      forbiddenCtx("dashscope-speech"),
    );
    expect(audioFetched).toBe(true);
    const call = (fetch as any).mock.calls[0];
    const payload = JSON.parse(call[1].body);
    expect(payload.model).toBe("qwen-tts-latest");
    expect(payload.input).toEqual({ text: "hi", voice: "Serena" });
  });
});
