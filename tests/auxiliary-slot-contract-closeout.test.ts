/**
 * Auxiliary Slot — 收口修复 Consumer-Level Contract Tests.
 *
 * 这组测试证明「即使 Resolver 设计正确，Consumer 也不能重新破坏契约」。
 * 对应任务书 Task 5.1 / 12 / 13 / 14 / 15 的 negative validation。
 *
 * 重点断言：配置错误不 fallback、unknown slot 被拒绝、canonical 单一真理源一致。
 */
import { describe, it, expect } from "vitest";
import {
  AUXILIARY_SLOTS,
  AUXILIARY_SLOT_IDS,
  AUXILIARY_SLOT_PREF_ENTRIES,
  AuxiliaryConfigurationError,
  isAuxiliaryConfigError,
  isAuxiliarySlot,
} from "../core/auxiliary-slots.ts";
import { AuxiliaryModelResolver } from "../core/auxiliary-model-resolver.ts";
import {
  AUXILIARY_MODEL_PREF_KEYS,
  normalizeSharedModelsPatch,
} from "../core/config-coordinator.ts";
import { AUXILIARY_SLOT_IDS as SHARED_SLOT_IDS } from "../shared/auxiliary-slot-ids.ts";

// ── Resolver harness（复用 auxiliary-slot-resolver.test.ts 的形态） ────

interface FakeModel {
  id: string;
  provider: string;
  api?: string;
  input?: string[];
}

function makeResolver(config: {
  slotRefs?: Record<string, any>;
  chatModel?: FakeModel | null;
  models?: FakeModel[];
  freshCredentials?: Record<string, any>;
}) {
  const models = config.models || [];
  const resolveModel = (ref: any): FakeModel | null => {
    if (!ref) return null;
    const id = typeof ref === "object" ? ref.id : ref;
    const provider = typeof ref === "object" ? ref.provider : undefined;
    return models.find((m) => m.id === id && (!provider || m.provider === provider)) || null;
  };
  const chatCalls: string[] = [];
  const resolver = new AuxiliaryModelResolver({
    resolveModel,
    getChatModel: (agentId?: string | null) => {
      chatCalls.push(String(agentId || "default"));
      return config.chatModel ?? null;
    },
    getSlotModelRef: (slot: string) => config.slotRefs?.[slot] || null,
    resolveProviderCredentialsFresh: async (provider: string) =>
      config.freshCredentials?.[provider] || {
        api: "openai",
        apiKey: `key-${provider}`,
        baseUrl: `https://${provider}.example.com/v1`,
      },
    getProviderCredentials: (provider: string) =>
      config.freshCredentials?.[provider] || {
        api: "openai",
        apiKey: `key-${provider}`,
        baseUrl: `https://${provider}.example.com/v1`,
      },
  });
  return { resolver, chatCalls };
}

// ── Task 5.1 / C6：Preferences patch 必须拒绝 unknown slot ──────────

describe("Task 5.1 / C6: unknown slot rejection", () => {
  it("models.summarzie（拼写错误）→ throw（400）", () => {
    expect(() =>
      normalizeSharedModelsPatch({ summarzie: { provider: "ollama", id: "qwen3" } }),
    ).toThrow(/unknown shared model field "summarzie"/);
  });

  it("models.random_future_slot → throw", () => {
    expect(() =>
      normalizeSharedModelsPatch({ random_future_slot: { provider: "x", id: "y" } }),
    ).toThrow(/unknown shared model field "random_future_slot"/);
  });

  it("models.summarize = null → 正确 clear（不 throw）", () => {
    const result = normalizeSharedModelsPatch({ summarize: null });
    expect(result.summarize).toBeNull();
  });

  it("models.summarize omitted → 原值不变（结果不含 summarize）", () => {
    const result = normalizeSharedModelsPatch({ vision_enabled: true });
    expect(result).not.toHaveProperty("summarize");
    expect(result.vision_enabled).toBe(true);
  });

  it("models.summarize = valid ModelRef → set", () => {
    const result = normalizeSharedModelsPatch({ summarize: { provider: "ollama", id: "qwen3" } });
    expect(result.summarize).toEqual({ provider: "ollama", id: "qwen3" });
  });

  it("禁止为兼容而允许 utility / utility_large", () => {
    expect(() =>
      normalizeSharedModelsPatch({ utility: { provider: "x", id: "y" } }),
    ).toThrow(/unknown shared model field "utility"/);
    expect(() =>
      normalizeSharedModelsPatch({ utility_large: { provider: "x", id: "y" } }),
    ).toThrow(/unknown shared model field "utility_large"/);
  });

  it("vision_enabled 仍是允许的非 Slot 字段", () => {
    const result = normalizeSharedModelsPatch({ vision_enabled: false });
    expect(result.vision_enabled).toBe(false);
  });
});

// ── Task 12 C7 / Task 4：Canonical slot drift protection ─────────────

describe("Task C7 / Task 4: canonical slot single source of truth", () => {
  it("ConfigCoordinator 的 AUXILIARY_MODEL_PREF_KEYS 从 canonical descriptor 派生（不是第二份手写映射）", () => {
    // AUXILIARY_MODEL_PREF_KEYS 必须与 canonical AUXILIARY_SLOT_PREF_ENTRIES 完全一致。
    expect(AUXILIARY_MODEL_PREF_KEYS).toEqual(AUXILIARY_SLOT_PREF_ENTRIES);
  });

  it("canonical AUXILIARY_SLOT_IDS 与 shared 单一真理源一致", () => {
    expect([...AUXILIARY_SLOT_IDS]).toEqual([...SHARED_SLOT_IDS]);
  });

  it("每个 canonical Slot 的 preferenceKey 与 descriptor 一致（不是手写第二份）", () => {
    for (const [field, prefKey] of AUXILIARY_MODEL_PREF_KEYS) {
      expect(AUXILIARY_SLOTS[field].preferenceKey).toBe(prefKey);
    }
  });

  it("canonical Slot 数量恰好为 6（title/summarize/memory/vision/approval/guard）", () => {
    expect([...AUXILIARY_SLOT_IDS].sort()).toEqual(
      ["approval", "guard", "memory", "summarize", "title", "vision"],
    );
  });

  it("isAuxiliarySlot 对未知字段返回 false", () => {
    expect(isAuxiliarySlot("title")).toBe(true);
    expect(isAuxiliarySlot("summarzie")).toBe(false);
    expect(isAuxiliarySlot("utility")).toBe(false);
    expect(isAuxiliarySlot("utility_large")).toBe(false);
  });
});

// ── Task 6.3：结构化配置错误类型 ────────────────────────────────────

describe("Task 6.3: structured AuxiliaryConfigurationError", () => {
  it("isAuxiliaryConfigError 识别结构化错误（不靠 i18n 文本匹配）", () => {
    const err = new AuxiliaryConfigurationError("boom", "model_not_found", "summarize");
    expect(isAuxiliaryConfigError(err)).toBe(true);
    expect(err.code).toBe("AUXILIARY_CONFIG_ERROR");
    expect(err.slot).toBe("summarize");
    expect(err.reason).toBe("model_not_found");
  });

  it("isAuxiliaryConfigError 对普通 Error 返回 false（运行时失败不混淆为配置错误）", () => {
    expect(isAuxiliaryConfigError(new Error("timeout"))).toBe(false);
    expect(isAuxiliaryConfigError(new Error("HTTP 500"))).toBe(false);
    expect(isAuxiliaryConfigError(null)).toBe(false);
    expect(isAuxiliaryConfigError(undefined)).toBe(false);
  });

  it("isAuxiliaryConfigError 识别 duck-typed code（跨进程边界）", () => {
    const plainErr: any = { code: "AUXILIARY_CONFIG_ERROR", message: "x" };
    expect(isAuxiliaryConfigError(plainErr)).toBe(true);
  });
});

// ── Task 13：Approval / Guard fallback policy ────────────────────────

describe("Task 13: approval / guard fallback policy (canonical descriptor)", () => {
  it("approval fallback=none（未配置不 fallback chat）", () => {
    expect(AUXILIARY_SLOTS.approval.fallback).toBe("none");
  });

  it("guard fallback=none（未配置不 fallback chat 也不 fallback approval）", () => {
    expect(AUXILIARY_SLOTS.guard.fallback).toBe("none");
  });

  it("title/summarize/memory fallback=chat", () => {
    expect(AUXILIARY_SLOTS.title.fallback).toBe("chat");
    expect(AUXILIARY_SLOTS.summarize.fallback).toBe("chat");
    expect(AUXILIARY_SLOTS.memory.fallback).toBe("chat");
  });

  it("vision fallback=image_capable_chat（只 fallback 到支持图片的 chat）", () => {
    expect(AUXILIARY_SLOTS.vision.fallback).toBe("image_capable_chat");
  });
});

// ── Task 14：Vision capability ───────────────────────────────────────

describe("Task 14: vision capability descriptor", () => {
  it("vision slot capability=image（必须图片输入）", () => {
    expect(AUXILIARY_SLOTS.vision.capability).toBe("image");
  });

  it("其它 Slot capability=text", () => {
    for (const id of AUXILIARY_SLOT_IDS) {
      if (id === "vision") continue;
      expect(AUXILIARY_SLOTS[id].capability).toBe("text");
    }
  });
});

// ── Task 12 C2：generateAgentId config-error visibility ─────────────
// _generateAgentId 调用 resolveAuxiliaryModelFresh("title")；title 显式配置错误时
// resolver 抛结构化错误，消费方据此 emit diagnostic（不静默吞掉）。

describe("Task C2: title config-error 抛结构化错误（generateAgentId 可观测）", () => {
  it("title 显式配置错误 → resolveAuxiliaryModelFresh 抛 AuxiliaryConfigurationError", async () => {
    const { resolver } = makeResolver({
      slotRefs: { title: { provider: "ollama", id: "non-existent" } },
      models: [], // 模型不存在
      chatModel: { id: "chat-sentinel", provider: "openai", api: "openai" },
    });
    await expect(resolver.resolveAuxiliaryModelFresh("title")).rejects.toThrow();
    await expect(resolver.resolveAuxiliaryModelFresh("title")).rejects.toBeInstanceOf(AuxiliaryConfigurationError);
  });

  it("title 未配置 → resolver fallback chat（不抛错）", async () => {
    const { resolver } = makeResolver({
      slotRefs: {},
      chatModel: { id: "chat-sentinel", provider: "openai", api: "openai" },
    });
    const resolved = await resolver.resolveAuxiliaryModelFresh("title");
    expect(resolved).not.toBeNull();
    expect(resolved.model.id).toBe("chat-sentinel");
  });
});

// ── Task 12 C3：Memory independent from chat ─────────────────────────
// memory_model 有效但 chat=null 时，MemoryTicker 仍能拿到 memory 模型。

describe("Task C3: memory valid + chat null → MemoryTicker 仍可工作", () => {
  it("memory 显式有效，chat=null → resolve memory 得到 memory sentinel，不查 chat", async () => {
    const { resolver, chatCalls } = makeResolver({
      slotRefs: { memory: { provider: "ollama", id: "memory-sentinel" } },
      models: [{ id: "memory-sentinel", provider: "ollama", api: "openai" }],
      chatModel: null,
    });
    const execution = await resolver.resolveAuxiliaryExecution("memory");
    expect(execution).not.toBeNull();
    expect(execution.model.id).toBe("memory-sentinel");
    // memory 已显式配置，不应查 chat。
    expect(chatCalls).toHaveLength(0);
  });
});

// ── Task 12 C4：Memory explicit failure no fallback ──────────────────
// memory 显式配置错误 + chat 有效 → 不得 fallback chat。

describe("Task C4: memory invalid + chat valid → 不 fallback chat", () => {
  it("memory 显式配置错误 → resolver 抛 AuxiliaryConfigurationError，不查 chat", async () => {
    const { resolver, chatCalls } = makeResolver({
      slotRefs: { memory: { provider: "ollama", id: "non-existent" } },
      models: [{ id: "chat-sentinel", provider: "openai", api: "openai" }],
      chatModel: { id: "chat-sentinel", provider: "openai", api: "openai" },
    });
    await expect(resolver.resolveAuxiliaryExecution("memory")).rejects.toBeInstanceOf(AuxiliaryConfigurationError);
    // 配置错误不得 fallback chat。
    expect(chatCalls).toHaveLength(0);
  });
});

// ── Task 12 C5：Diary slot routing（memory slot） ────────────────────
// writeDiary 现在走 resolveAuxiliaryExecution("memory")，由 memory slot 自行 fallback。

describe("Task C5: diary 走 memory slot（不再 chat-first）", () => {
  it("memory 显式有效 + chat 也有效 → resolve memory 得到 memory sentinel（不是 chat）", async () => {
    const { resolver } = makeResolver({
      slotRefs: { memory: { provider: "ollama", id: "sentinel-memory" } },
      models: [
        { id: "sentinel-memory", provider: "ollama", api: "openai" },
        { id: "sentinel-chat", provider: "openai", api: "openai" },
      ],
      chatModel: { id: "sentinel-chat", provider: "openai", api: "openai" },
    });
    const execution = await resolver.resolveAuxiliaryExecution("memory");
    expect(execution.model.id).toBe("sentinel-memory");
  });

  it("memory 未配置 → resolver fallback chat（diary 仍可用）", async () => {
    const { resolver } = makeResolver({
      slotRefs: {},
      models: [{ id: "sentinel-chat", provider: "openai", api: "openai" }],
      chatModel: { id: "sentinel-chat", provider: "openai", api: "openai" },
    });
    const execution = await resolver.resolveAuxiliaryExecution("memory");
    expect(execution.model.id).toBe("sentinel-chat");
  });
});

// ── Task 25：Cross-agent chat fallback ───────────────────────────────
// resolver fallback 到 chat 时必须用目标 agent 的 chat，不是 focused agent 的。

describe("Task 25: cross-agent chat fallback", () => {
  it("resolve memory for agent B → fallback 用 agent B 的 chat", async () => {
    const chatA = { id: "chat-A", provider: "openai", api: "openai" };
    const chatB = { id: "chat-B", provider: "openai", api: "openai" };
    const chatByAgent: Record<string, FakeModel> = { "agent-A": chatA, "agent-B": chatB };
    const models = [chatA, chatB];
    const resolver = new AuxiliaryModelResolver({
      resolveModel: (ref: any) => {
        const id = typeof ref === "object" ? ref.id : ref;
        return models.find((m) => m.id === id) || null;
      },
      // getChatModel 接收 agentId，返回对应 agent 的 chat（模拟 per-agent chat）。
      getChatModel: (agentId?: string | null) => chatByAgent[String(agentId || "default")] || null,
      getSlotModelRef: () => null, // memory 未配置 → fallback chat
      resolveProviderCredentialsFresh: async () => ({
        api: "openai", apiKey: "k", baseUrl: "https://x.example.com/v1",
      }),
      getProviderCredentials: () => ({
        api: "openai", apiKey: "k", baseUrl: "https://x.example.com/v1",
      }),
    });
    const execution = await resolver.resolveAuxiliaryExecution("memory", { agentId: "agent-B" });
    expect(execution.model.id).toBe("chat-B");
    // 不是 focused agent 的 chat-A。
    expect(execution.model.id).not.toBe("chat-A");
  });
});
