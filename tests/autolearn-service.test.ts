/**
 * autolearn-service 闸门与结算链测试。
 *
 * 覆盖：闸门顺序（aborted/read_only/开关/阈值/防抖）、summarize 槽未配置跳过、
 * 提炼结果解析（skip/围栏 JSON/超限）、同名冲突跳过、guard 审查失败丢弃、
 * 建议卡确认→落盘+通知链、超时/拒绝不落盘、确认瞬间冲突不覆盖。
 * callText 用 usageContext.source.subsystem 区分提炼（autolearn）与审查（guard）。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";

vi.mock("../lib/i18n.js", () => ({
  getLocale: () => "zh-CN",
  t: (key: string, values?: Record<string, unknown>) => {
    if (!values) return key;
    const suffix = Object.entries(values).map(([k, v]) => `${k}=${v}`).join(",");
    return `${key}:${suffix}`;
  },
}));

vi.mock("../core/llm-client.js", () => ({
  callText: vi.fn(),
}));

import { callText } from "../core/llm-client.ts";
import {
  AUTOLEARN_CONFIRM_TIMEOUT_MS,
  AUTOLEARN_DAILY_CAP,
  AUTOLEARN_MIN_TOOL_CALLS,
  AUTOLEARN_SESSION_COOLDOWN_MS,
  buildAutolearnDistillPrompt,
  createAutolearnService,
  parseAutolearnDistillResult,
} from "../lib/autolearn/autolearn-service.ts";

const LESSON_JSON = JSON.stringify({
  name: "retry-empty-reply",
  description: "空回复先重试一次再放弃",
  lesson: "遇到空回复先换措辞重试一次。\n\n原样重发通常还是空。",
});

function trace(n: number) {
  return Array.from({ length: n }, (_, i) => ({ name: `tool_${i}`, ok: true, head: `out ${i}` }));
}

function makeTurn(overrides: any = {}) {
  return {
    sessionPath: "/tmp/sess-1.jsonl",
    toolCalls: AUTOLEARN_MIN_TOOL_CALLS,
    trace: trace(AUTOLEARN_MIN_TOOL_CALLS),
    aborted: false,
    permissionMode: "standard",
    ...overrides,
  };
}

function makeDeferred() {
  let resolve: any, reject: any;
  const promise = new Promise((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

/** 结算链是「await promise → 异步落盘 → 事件」的微任务+fs 混合链，多冲几拍再断言。 */
async function flushSettle() {
  for (let i = 0; i < 6; i++) {
    await Promise.resolve();
    await new Promise((r) => setImmediate(r));
  }
}

describe("autolearn-service", () => {
  const roots: string[] = [];

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    while (roots.length) {
      const r = roots.pop()!;
      fs.rmSync(r, { recursive: true, force: true });
    }
  });

  function makeService(opts: any = {}) {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-autolearn-"));
    roots.push(root);
    const skillsDir = path.join(root, "skills");
    const agentDir = path.join(root, "agent");
    fs.mkdirSync(agentDir, { recursive: true });

    const deferred = makeDeferred();
    const confirmStore = { create: vi.fn(() => ({ confirmId: "confirm-1", promise: deferred.promise })) };
    const emitEvent = vi.fn();
    const notifyInstalled = vi.fn();
    const log = { info: vi.fn(), warn: vi.fn() };
    let clock = opts.now ?? 1_000_000;
    const service = createAutolearnService({
      isEnabled: () => opts.enabled !== false,
      resolveAuxModel: opts.resolveAuxModel ?? (async (slot: string) => ({
        model: `${slot}-model`, apiKey: "k", baseUrl: "https://example.test", api: "openai",
      })),
      getUserSkillsDir: () => skillsDir,
      getAgentDirForSession: () => agentDir,
      confirmStore,
      emitEvent,
      notifyInstalled,
      log,
      now: () => clock,
    });
    return {
      service, skillsDir, agentDir, confirmStore, emitEvent, notifyInstalled, log, deferred,
      advance: (ms: number) => { clock += ms; },
    };
  }

  /** 提炼回 LESSON_JSON、审查回 safe。 */
  function mockLlmPass() {
    (callText as any).mockImplementation((args: any) => {
      const subsystem = args?.usageContext?.source?.subsystem;
      if (subsystem === "guard") return Promise.resolve("safe");
      return Promise.resolve(LESSON_JSON);
    });
  }

  it("闸门：aborted / 计划模式 / 关开关 / 调用数不足 都不触发提炼", async () => {
    const { service, confirmStore } = makeService();
    mockLlmPass();
    await service.observeTurn(makeTurn({ aborted: true }));
    await service.observeTurn(makeTurn({ permissionMode: "read_only", sessionPath: "/tmp/sess-ro.jsonl" }));
    await service.observeTurn(makeTurn({ toolCalls: AUTOLEARN_MIN_TOOL_CALLS - 1, sessionPath: "/tmp/sess-few.jsonl" }));
    expect(callText).not.toHaveBeenCalled();
    expect(confirmStore.create).not.toHaveBeenCalled();

    const off = makeService({ enabled: false });
    await off.service.observeTurn(makeTurn());
    expect(callText).not.toHaveBeenCalled();
    expect(off.confirmStore.create).not.toHaveBeenCalled();
  });

  it("summarize 槽未配置：跳过，不发卡", async () => {
    const { service, confirmStore, emitEvent } = makeService({ resolveAuxModel: async () => null });
    await service.observeTurn(makeTurn());
    expect(callText).not.toHaveBeenCalled();
    expect(confirmStore.create).not.toHaveBeenCalled();
    expect(emitEvent).not.toHaveBeenCalled();
  });

  it("模型说 skip / 输出不是 JSON：不发卡", async () => {
    const { service, confirmStore } = makeService();
    (callText as any).mockResolvedValue('{"skip":true}');
    await service.observeTurn(makeTurn());
    (callText as any).mockResolvedValue("我不确定，这轮没什么可学的");
    await service.observeTurn(makeTurn({ sessionPath: "/tmp/sess-2.jsonl" }));
    expect(confirmStore.create).not.toHaveBeenCalled();
  });

  it("同名技能已存在：跳过，不覆盖", async () => {
    const { service, skillsDir, confirmStore } = makeService();
    mockLlmPass();
    fs.mkdirSync(path.join(skillsDir, "retry-empty-reply"), { recursive: true });
    await service.observeTurn(makeTurn());
    expect(confirmStore.create).not.toHaveBeenCalled();
  });

  it("guard 审查不过：丢弃建议，只记日志", async () => {
    const { service, confirmStore, emitEvent, log } = makeService();
    (callText as any).mockImplementation((args: any) => {
      const subsystem = args?.usageContext?.source?.subsystem;
      if (subsystem === "guard") return Promise.resolve("suspicious: 越权指令");
      return Promise.resolve(LESSON_JSON);
    });
    await service.observeTurn(makeTurn());
    expect(confirmStore.create).not.toHaveBeenCalled();
    expect(emitEvent).not.toHaveBeenCalled();
    expect(log.info).toHaveBeenCalledWith(expect.stringContaining("dropped by safety review"));
  });

  it("全链通过：发卡 → 用户确认 → 落盘 + 启用回调 + 通知", async () => {
    const { service, skillsDir, agentDir, confirmStore, emitEvent, notifyInstalled, deferred } = makeService();
    mockLlmPass();
    await service.observeTurn(makeTurn());

    expect(confirmStore.create).toHaveBeenCalledWith(
      "autolearn_lesson",
      expect.objectContaining({ name: "retry-empty-reply" }),
      "/tmp/sess-1.jsonl",
      AUTOLEARN_CONFIRM_TIMEOUT_MS,
    );
    const suggestion = emitEvent.mock.calls.find((c: any[]) => c[0]?.type === "autolearn_suggestion");
    expect(suggestion).toBeTruthy();
    expect(suggestion[0].confirmId).toBe("confirm-1");
    expect(suggestion[1]).toBe("/tmp/sess-1.jsonl");
    // 确认前不落盘
    expect(fs.existsSync(path.join(skillsDir, "retry-empty-reply"))).toBe(false);

    deferred.resolve({ action: "confirmed" });
    await flushSettle();

    const skillFile = path.join(skillsDir, "retry-empty-reply", "SKILL.md");
    expect(fs.existsSync(skillFile)).toBe(true);
    expect(fs.readFileSync(skillFile, "utf8")).toContain("空回复先换措辞重试一次");
    expect(fs.existsSync(path.join(agentDir, "experience.md"))).toBe(true);
    expect(notifyInstalled).toHaveBeenCalledWith("retry-empty-reply", "/tmp/sess-1.jsonl");
    const notice = emitEvent.mock.calls.find((c: any[]) => c[0]?.type === "notification");
    expect(notice?.[0]?.title).toContain("autolearn.installed.title");
  });

  it("超时与忽略：不落盘、不通知", async () => {
    for (const action of ["timeout", "rejected"]) {
      const { service, skillsDir, confirmStore, deferred } = makeService();
      mockLlmPass();
      await service.observeTurn(makeTurn());
      expect(confirmStore.create).toHaveBeenCalledTimes(1);
      deferred.resolve({ action });
      await flushSettle();
      expect(fs.existsSync(path.join(skillsDir, "retry-empty-reply"))).toBe(false);
    }
  });

  it("防抖：同会话冷却期内不重复发，冷却过后可以再发", async () => {
    const { service, confirmStore, advance } = makeService();
    mockLlmPass();
    await service.observeTurn(makeTurn());
    await service.observeTurn(makeTurn());
    expect(confirmStore.create).toHaveBeenCalledTimes(1);
    advance(AUTOLEARN_SESSION_COOLDOWN_MS + 1);
    await service.observeTurn(makeTurn());
    expect(confirmStore.create).toHaveBeenCalledTimes(2);
  });

  it("防抖：每日上限用尽后全停；提炼失败不占配额", async () => {
    const { service, confirmStore, advance } = makeService();
    mockLlmPass();
    for (let i = 0; i < AUTOLEARN_DAILY_CAP; i++) {
      await service.observeTurn(makeTurn({ sessionPath: `/tmp/sess-cap-${i}.jsonl` }));
    }
    expect(confirmStore.create).toHaveBeenCalledTimes(AUTOLEARN_DAILY_CAP);
    await service.observeTurn(makeTurn({ sessionPath: "/tmp/sess-cap-overflow.jsonl" }));
    expect(confirmStore.create).toHaveBeenCalledTimes(AUTOLEARN_DAILY_CAP);

    // 提炼返回 skip 的回合不消耗配额：新一轮换个会话仍能发卡
    const { service: svc2, confirmStore: cs2 } = makeService();
    (callText as any).mockResolvedValue('{"skip":true}');
    await svc2.observeTurn(makeTurn());
    expect(cs2.create).not.toHaveBeenCalled();
    expect((svc2._debugState() as any).dailyCount).toBe(0);
  });

  it("确认瞬间同名技能已存在：不覆盖", async () => {
    const { service, skillsDir, deferred, notifyInstalled } = makeService();
    mockLlmPass();
    await service.observeTurn(makeTurn());
    // 建议存活期间用户手动装了同名技能
    fs.mkdirSync(path.join(skillsDir, "retry-empty-reply"), { recursive: true });
    deferred.resolve({ action: "confirmed" });
    await flushSettle();
    expect(fs.existsSync(path.join(skillsDir, "retry-empty-reply", "SKILL.md"))).toBe(false);
    expect(notifyInstalled).not.toHaveBeenCalled();
  });
});

describe("parseAutolearnDistillResult", () => {
  it("接受裸 JSON 与 ```json 围栏", () => {
    const bare = parseAutolearnDistillResult(LESSON_JSON);
    expect(bare?.name).toBe("retry-empty-reply");
    const fenced = parseAutolearnDistillResult(`\`\`\`json\n${LESSON_JSON}\n\`\`\``);
    expect(fenced?.lesson).toContain("重试");
  });

  it("skip / 缺字段 / 非对象 / 超限 都归一为 null", () => {
    expect(parseAutolearnDistillResult('{"skip":true}')).toBeNull();
    expect(parseAutolearnDistillResult('{"name":"x"}')).toBeNull();
    expect(parseAutolearnDistillResult("[1,2]")).toBeNull();
    expect(parseAutolearnDistillResult("")).toBeNull();
    expect(parseAutolearnDistillResult(null)).toBeNull();
    expect(parseAutolearnDistillResult(JSON.stringify({
      name: "x", description: "d", lesson: "l".repeat(5000),
    }))).toBeNull();
  });
});

describe("buildAutolearnDistillPrompt", () => {
  it("轨迹进提示词且条目数与单条长度有上限", () => {
    const prompt = buildAutolearnDistillPrompt(trace(60));
    expect(prompt).toContain("tool_0");
    expect(prompt).toContain("tool_39");
    expect(prompt).not.toContain("tool_40");
    const long = buildAutolearnDistillPrompt([{ name: "t", ok: false, head: "x".repeat(999) }]);
    expect(long.length).toBeLessThan(3000);
  });
});
