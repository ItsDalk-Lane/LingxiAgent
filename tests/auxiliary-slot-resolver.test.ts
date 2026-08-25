/**
 * Auxiliary Slot Resolver — contract tests.
 *
 * 证明 Slot 之间真的相互隔离。
 * Case A–I 对应任务书第十八节。
 */
import { describe, it, expect, vi } from "vitest";
import { AuxiliaryModelResolver } from "../core/auxiliary-model-resolver.ts";
import {
  AUXILIARY_SLOTS,
  AUXILIARY_SLOT_IDS,
  isAuxiliarySlot,
} from "../core/auxiliary-slots.ts";

// ── Test harness ──────────────────────────────────────────────────────

interface FakeModel {
  id: string;
  provider: string;
  api?: string;
  input?: string[];
  headers?: Record<string, string>;
  baseUrl?: string;
}

function makeSentinel(id: string, provider = "testprov"): FakeModel {
  return { id, provider, api: "openai", input: ["text"] };
}

function makeImageSentinel(id: string, provider = "testprov"): FakeModel {
  return { id, provider, api: "openai", input: ["text", "image"] };
}

interface HarnessConfig {
  slotRefs: Partial<Record<string, any>>;
  chatModel: FakeModel | null;
  models: FakeModel[];
  freshCredentials?: Record<string, any>;
  cachedCredentials?: Record<string, any>;
}

function makeResolver(config: HarnessConfig) {
  const models = config.models;
  const resolveModel = (ref: any): FakeModel | null => {
    if (!ref) return null;
    const id = typeof ref === "object" ? ref.id : ref;
    const provider = typeof ref === "object" ? ref.provider : undefined;
    return (
      models.find(
        (m) => m.id === id && (!provider || m.provider === provider),
      ) || null
    );
  };

  const getSlotModelRef = (slot: string) => config.slotRefs[slot] || null;
  const getChatModel = () => config.chatModel;

  const freshCallLog: string[] = [];
  const resolveProviderCredentialsFresh = async (provider: string) => {
    freshCallLog.push(provider);
    return (
      config.freshCredentials?.[provider] || {
        api: "openai",
        apiKey: `key-${provider}`,
        baseUrl: `https://${provider}.example.com/v1`,
        credentialSource: "provider-catalog",
      }
    );
  };
  const getProviderCredentials = (provider: string) =>
    config.cachedCredentials?.[provider] || {
      api: "openai",
      apiKey: `key-${provider}`,
      baseUrl: `https://${provider}.example.com/v1`,
      credentialSource: "provider-catalog",
    };

  const resolver = new AuxiliaryModelResolver({
    resolveModel,
    getChatModel,
    getSlotModelRef,
    resolveProviderCredentialsFresh,
    getProviderCredentials,
  });
  return { resolver, freshCallLog, resolveModel, getProviderCredentials };
}

// ── Tests ─────────────────────────────────────────────────────────────

describe("AuxiliarySlot descriptors", () => {
  it("defines exactly 7 canonical slots", () => {
    expect(AUXILIARY_SLOT_IDS).toEqual([
      "title",
      "summarize",
      "memory",
      "knowledge",
      "vision",
      "approval",
      "guard",
    ]);
  });

  it("each slot has preference key, fallback, capability", () => {
    for (const id of AUXILIARY_SLOT_IDS) {
      const d = AUXILIARY_SLOTS[id];
      expect(d.id).toBe(id);
      expect(d.preferenceKey).toMatch(/_model$/);
      expect(["chat", "image_capable_chat", "none"]).toContain(d.fallback);
      expect(["text", "image"]).toContain(d.capability);
    }
  });

  it("title/summarize/memory/knowledge fallback to chat", () => {
    expect(AUXILIARY_SLOTS.title.fallback).toBe("chat");
    expect(AUXILIARY_SLOTS.summarize.fallback).toBe("chat");
    expect(AUXILIARY_SLOTS.memory.fallback).toBe("chat");
    expect(AUXILIARY_SLOTS.knowledge.fallback).toBe("chat");
  });

  it("vision fallback to image_capable_chat", () => {
    expect(AUXILIARY_SLOTS.vision.fallback).toBe("image_capable_chat");
    expect(AUXILIARY_SLOTS.vision.capability).toBe("image");
  });

  it("approval/guard fallback to none", () => {
    expect(AUXILIARY_SLOTS.approval.fallback).toBe("none");
    expect(AUXILIARY_SLOTS.guard.fallback).toBe("none");
  });

  it("isAuxiliarySlot type-guards", () => {
    expect(isAuxiliarySlot("title")).toBe(true);
    expect(isAuxiliarySlot("utility")).toBe(false);
    expect(isAuxiliarySlot("approval")).toBe(true);
    expect(isAuxiliarySlot("random")).toBe(false);
  });
});

// Case A: 六 Slot 使用六个不同模型，每个入口只收到自己的 sentinel
describe("Case A: six slots resolve to six different sentinels", () => {
  it("each slot returns only its own configured model", async () => {
    const models = [
      makeSentinel("title-sentinel", "provider"),
      makeSentinel("summary-sentinel", "provider"),
      makeSentinel("memory-sentinel", "provider"),
      makeImageSentinel("vision-sentinel", "provider"),
      makeSentinel("approval-sentinel", "provider"),
      makeSentinel("guard-sentinel", "provider"),
    ];
    const { resolver } = makeResolver({
      slotRefs: {
        title: { id: "title-sentinel", provider: "provider" },
        summarize: { id: "summary-sentinel", provider: "provider" },
        memory: { id: "memory-sentinel", provider: "provider" },
        vision: { id: "vision-sentinel", provider: "provider" },
        approval: { id: "approval-sentinel", provider: "provider" },
        guard: { id: "guard-sentinel", provider: "provider" },
      },
      chatModel: makeSentinel("chat-sentinel", "provider"),
      models,
    });

    const titleExec = await resolver.resolveAuxiliaryModelFresh("title");
    const summarizeExec = await resolver.resolveAuxiliaryModelFresh("summarize");
    const memoryExec = await resolver.resolveAuxiliaryModelFresh("memory");
    const visionExec = await resolver.resolveAuxiliaryModelFresh("vision");
    const approvalExec = await resolver.resolveAuxiliaryModelFresh("approval");
    const guardExec = await resolver.resolveAuxiliaryModelFresh("guard");

    expect(titleExec?.model?.id).toBe("title-sentinel");
    expect(summarizeExec?.model?.id).toBe("summary-sentinel");
    expect(memoryExec?.model?.id).toBe("memory-sentinel");
    expect(visionExec?.model?.id).toBe("vision-sentinel");
    expect(approvalExec?.model?.id).toBe("approval-sentinel");
    expect(guardExec?.model?.id).toBe("guard-sentinel");
  });
});

// Case B: 修改一个 Slot 不影响其它 Slot
describe("Case B: modifying one slot does not affect others", () => {
  it("changing approval A→B leaves all other slots unchanged", async () => {
    const modelsA = [
      makeSentinel("approval-a", "provider"),
      makeSentinel("title-x", "provider"),
      makeSentinel("sum-x", "provider"),
      makeSentinel("mem-x", "provider"),
      makeImageSentinel("vis-x", "provider"),
      makeSentinel("guard-x", "provider"),
      makeSentinel("chat-x", "provider"),
    ];
    const slotRefsBase = {
      title: { id: "title-x", provider: "provider" },
      summarize: { id: "sum-x", provider: "provider" },
      memory: { id: "mem-x", provider: "provider" },
      vision: { id: "vis-x", provider: "provider" },
      approval: { id: "approval-a", provider: "provider" },
      guard: { id: "guard-x", provider: "provider" },
    };
    const { resolver: resolverA } = makeResolver({
      slotRefs: slotRefsBase,
      chatModel: makeSentinel("chat-x", "provider"),
      models: modelsA,
    });
    const titleBefore = (await resolverA.resolveAuxiliaryModelFresh("title"))?.model?.id;
    const sumBefore = (await resolverA.resolveAuxiliaryModelFresh("summarize"))?.model?.id;
    const memBefore = (await resolverA.resolveAuxiliaryModelFresh("memory"))?.model?.id;
    const visBefore = (await resolverA.resolveAuxiliaryModelFresh("vision"))?.model?.id;
    const guardBefore = (await resolverA.resolveAuxiliaryModelFresh("guard"))?.model?.id;

    // Now change approval to B
    const modelsB = [...modelsA, makeSentinel("approval-b", "provider")];
    const { resolver: resolverB } = makeResolver({
      slotRefs: { ...slotRefsBase, approval: { id: "approval-b", provider: "provider" } },
      chatModel: makeSentinel("chat-x", "provider"),
      models: modelsB,
    });

    const approvalB = (await resolverB.resolveAuxiliaryModelFresh("approval"))?.model?.id;
    expect(approvalB).toBe("approval-b");

    // All other slots unchanged
    expect((await resolverB.resolveAuxiliaryModelFresh("title"))?.model?.id).toBe(titleBefore);
    expect((await resolverB.resolveAuxiliaryModelFresh("summarize"))?.model?.id).toBe(sumBefore);
    expect((await resolverB.resolveAuxiliaryModelFresh("memory"))?.model?.id).toBe(memBefore);
    expect((await resolverB.resolveAuxiliaryModelFresh("vision"))?.model?.id).toBe(visBefore);
    expect((await resolverB.resolveAuxiliaryModelFresh("guard"))?.model?.id).toBe(guardBefore);
  });
});

// Case C: 未配置普通 Slot → fallback chat
describe("Case C: unconfigured normal slot falls back to chat", () => {
  it("summarize=null resolves to chat-sentinel", async () => {
    const { resolver } = makeResolver({
      slotRefs: {},
      chatModel: makeSentinel("chat-sentinel", "provider"),
      models: [makeSentinel("chat-sentinel", "provider")],
    });
    const result = await resolver.resolveAuxiliaryModelFresh("summarize");
    expect(result?.model?.id).toBe("chat-sentinel");
  });

  it("title=null falls back to chat", async () => {
    const { resolver } = makeResolver({
      slotRefs: {},
      chatModel: makeSentinel("chat-sentinel", "provider"),
      models: [makeSentinel("chat-sentinel", "provider")],
    });
    const result = await resolver.resolveAuxiliaryModelFresh("title");
    expect(result?.model?.id).toBe("chat-sentinel");
  });
});

// Case D: 配置错误不得 fallback
describe("Case D: configured-but-unresolvable slot does not fall back", () => {
  it("summarize=invalid/model throws, chat not consulted", async () => {
    const { resolver, freshCallLog } = makeResolver({
      slotRefs: {
        summarize: { id: "non-existent-model", provider: "provider" },
      },
      chatModel: makeSentinel("valid-chat", "provider"),
      models: [makeSentinel("valid-chat", "provider")], // does NOT include non-existent-model
    });
    await expect(resolver.resolveAuxiliaryModelFresh("summarize")).rejects.toThrow();
    // chat provider should never have been consulted for credentials
    expect(freshCallLog).not.toContain("provider");
  });
});

// Case E: Approval 未配置 → returns null (no chat fallback)
describe("Case E: approval unconfigured returns null, chat never called", () => {
  it("approval=null → null result", async () => {
    const { resolver, freshCallLog } = makeResolver({
      slotRefs: {},
      chatModel: makeSentinel("valid-chat", "provider"),
      models: [makeSentinel("valid-chat", "provider")],
    });
    const result = await resolver.resolveAuxiliaryModelFresh("approval");
    expect(result).toBeNull();
    expect(freshCallLog.length).toBe(0);
  });
});

// Case F: Approval 配错 → throws (no chat fallback)
describe("Case F: approval misconfigured throws, chat not called", () => {
  it("approval=invalid/model → throws", async () => {
    const { resolver, freshCallLog } = makeResolver({
      slotRefs: {
        approval: { id: "invalid-model", provider: "provider" },
      },
      chatModel: makeSentinel("valid-chat", "provider"),
      models: [makeSentinel("valid-chat", "provider")],
    });
    await expect(resolver.resolveAuxiliaryModelFresh("approval")).rejects.toThrow();
    expect(freshCallLog.length).toBe(0);
  });
});

// Case G: Guard 未配置 → null (no fallback to chat or approval)
describe("Case G: guard unconfigured returns null", () => {
  it("guard=null → null, neither chat nor approval consulted", async () => {
    const { resolver, freshCallLog } = makeResolver({
      slotRefs: {},
      chatModel: makeSentinel("valid-chat", "provider"),
      models: [makeSentinel("valid-chat", "provider")],
    });
    const result = await resolver.resolveAuxiliaryModelFresh("guard");
    expect(result).toBeNull();
    expect(freshCallLog.length).toBe(0);
  });
});

// Case H: Vision capability
describe("Case H: vision capability enforcement", () => {
  it("setting a non-image model to vision slot is rejected at resolve time", async () => {
    const { resolver } = makeResolver({
      slotRefs: {
        vision: { id: "text-only-model", provider: "provider" },
      },
      chatModel: makeSentinel("chat-x", "provider"),
      models: [makeSentinel("text-only-model", "provider"), makeSentinel("chat-x", "provider")],
    });
    await expect(resolver.resolveAuxiliaryModelFresh("vision")).rejects.toThrow();
  });

  it("unconfigured vision only falls back if chat supports image", async () => {
    const { resolver: resolverNoImage } = makeResolver({
      slotRefs: {},
      chatModel: makeSentinel("chat-text-only", "provider"),
      models: [makeSentinel("chat-text-only", "provider")],
    });
    const result = await resolverNoImage.resolveAuxiliaryModelFresh("vision");
    expect(result).toBeNull();

    const { resolver: resolverWithImage } = makeResolver({
      slotRefs: {},
      chatModel: makeImageSentinel("chat-image", "provider"),
      models: [makeImageSentinel("chat-image", "provider")],
    });
    const result2 = await resolverWithImage.resolveAuxiliaryModelFresh("vision");
    expect(result2?.model?.id).toBe("chat-image");
  });
});

// Case I: Fresh credential (simplified — full integration in fresh-credential-routing tests)
describe("Case I: fresh credential resolves from provider", () => {
  it("Slot A→ProviderA, Slot B→ProviderB resolve independently", async () => {
    const { resolver, freshCallLog } = makeResolver({
      slotRefs: {
        summarize: { id: "sum-model", provider: "provA" },
        memory: { id: "mem-model", provider: "provB" },
      },
      chatModel: makeSentinel("chat-x", "provA"),
      models: [
        makeSentinel("sum-model", "provA"),
        makeSentinel("mem-model", "provB"),
        makeSentinel("chat-x", "provA"),
      ],
    });

    await resolver.resolveAuxiliaryModelFresh("summarize");
    await resolver.resolveAuxiliaryModelFresh("memory");
    expect(freshCallLog).toContain("provA");
    expect(freshCallLog).toContain("provB");
  });
});

describe("Knowledge 请求边界刷新", () => {
  it("每次请求都重新读取 Provider 凭证，不复用旧密钥", async () => {
    const config: HarnessConfig = {
      slotRefs: {
        knowledge: { id: "knowledge-model", provider: "provA" },
      },
      chatModel: null,
      models: [makeSentinel("knowledge-model", "provA")],
      freshCredentials: {
        provA: {
          api: "openai",
          apiKey: "first-key",
          baseUrl: "https://provA.example.com/v1",
        },
      },
    };
    const { resolver, freshCallLog } = makeResolver(config);

    expect((await resolver.resolveAuxiliaryModelFresh("knowledge"))?.apiKey).toBe("first-key");
    config.freshCredentials!.provA.apiKey = "rotated-key";
    expect((await resolver.resolveAuxiliaryModelFresh("knowledge"))?.apiKey).toBe("rotated-key");
    expect(freshCallLog).toEqual(["provA", "provA"]);
  });

  it("下一次请求使用最新的 Knowledge 模型选择", async () => {
    const config: HarnessConfig = {
      slotRefs: {
        knowledge: { id: "knowledge-a", provider: "provA" },
      },
      chatModel: null,
      models: [
        makeSentinel("knowledge-a", "provA"),
        makeSentinel("knowledge-b", "provB"),
      ],
    };
    const { resolver, freshCallLog } = makeResolver(config);

    expect((await resolver.resolveAuxiliaryModelFresh("knowledge"))?.model?.id).toBe("knowledge-a");
    config.slotRefs.knowledge = { id: "knowledge-b", provider: "provB" };
    expect((await resolver.resolveAuxiliaryModelFresh("knowledge"))?.model?.id).toBe("knowledge-b");
    expect(freshCallLog).toEqual(["provA", "provB"]);
  });
});

// Extra: resolveAuxiliaryExecution returns full object
describe("resolveAuxiliaryExecution", () => {
  it("returns full execution object with model/provider/apiKey/baseUrl", async () => {
    const { resolver } = makeResolver({
      slotRefs: {
        vision: { id: "vis-model", provider: "provider" },
      },
      chatModel: null,
      models: [makeImageSentinel("vis-model", "provider")],
    });
    const exec = await resolver.resolveAuxiliaryExecution("vision");
    expect(exec).not.toBeNull();
    expect(exec?.model?.id).toBe("vis-model");
    expect(exec?.provider).toBe("provider");
    expect(exec?.api).toBe("openai");
    expect(exec?.apiKey).toBeTruthy();
    expect(exec?.baseUrl).toBeTruthy();
  });
});
