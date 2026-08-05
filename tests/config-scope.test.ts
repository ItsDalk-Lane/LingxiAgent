/**
 * config-scope.js 单元测试
 *
 * 测试：splitByScope、injectGlobalFields
 */

import { describe, it, expect } from "vitest";
import { splitByScope, injectGlobalFields } from "../shared/config-scope.ts";

// ---------------------------------------------------------------------------
// splitByScope
// ---------------------------------------------------------------------------

describe("splitByScope", () => {
  it("extracts top-level global fields while keeping agent fields (models)", () => {
    const partial = { locale: "zh-CN", sandbox: false, sandbox_network: true, hardware_acceleration: false, models: ["gpt-4"] };
    const { global: g, agent }: any = splitByScope(partial);

    expect(g).toEqual(expect.arrayContaining([
      expect.objectContaining({ key: "locale", value: "zh-CN" }),
      expect.objectContaining({ key: "sandbox", value: false }),
      expect.objectContaining({ key: "sandbox_network", value: true }),
      expect.objectContaining({ key: "hardware_acceleration", value: false }),
    ]));
    expect(agent.models).toEqual(["gpt-4"]);
    expect(agent.locale).toBeUndefined();
    expect(agent.sandbox).toBeUndefined();
    expect(agent.sandbox_network).toBeUndefined();
    expect(agent.hardware_acceleration).toBeUndefined();
  });

  it("extracts nested global fields (capabilities.learn_skills) while keeping sibling nested fields", () => {
    const partial = {
      capabilities: { learn_skills: true, other_cap: "keep" },
    };
    const { global: g, agent }: any = splitByScope(partial);

    expect(g).toEqual(expect.arrayContaining([
      expect.objectContaining({ key: "capabilities.learn_skills", value: true }),
    ]));
    expect(agent.capabilities).toBeDefined();
    expect(agent.capabilities.other_cap).toBe("keep");
    expect(agent.capabilities.learn_skills).toBeUndefined();
  });

  it("removes empty parent after extracting the only nested global child", () => {
    const partial = { capabilities: { learn_skills: false } };
    const { global: g, agent }: any = splitByScope(partial);

    expect(g).toEqual(expect.arrayContaining([
      expect.objectContaining({ key: "capabilities.learn_skills", value: false }),
    ]));
    expect(agent.capabilities).toBeUndefined();
  });

  it("desk.home_folder is agent-scoped, heartbeat_interval also agent-scoped", () => {
    const partial = {
      desk: { home_folder: "/home/user", heartbeat_interval: 30 },
    };
    const { global: g, agent }: any = splitByScope(partial);

    // home_folder is now agent scope — NOT extracted as global
    expect(g).not.toEqual(expect.arrayContaining([
      expect.objectContaining({ key: "desk.home_folder" }),
    ]));
    expect(agent.desk.home_folder).toBe("/home/user");
    expect(agent.desk.heartbeat_interval).toBe(30);
  });

  it("extracts desk.heartbeat_master as global", () => {
    const partial = {
      desk: { heartbeat_master: false, heartbeat_interval: 20 },
    };
    const { global: g, agent }: any = splitByScope(partial);

    expect(g).toEqual(expect.arrayContaining([
      expect.objectContaining({ key: "desk.heartbeat_master", value: false }),
    ]));
    expect(agent.desk.heartbeat_interval).toBe(20);
    expect(agent.desk.heartbeat_master).toBeUndefined();
  });

  it("extracts bridge permission globals while keeping platform config", () => {
    const partial = {
      bridge: {
        permissionMode: "auto",
        readOnly: true,
        receiptEnabled: false,
        richStreamingEnabled: false,
        telegram: { token: "tg-token" },
      },
    };
    const { global: g, agent }: any = splitByScope(partial);

    expect(g).toEqual(expect.arrayContaining([
      expect.objectContaining({ key: "bridge.permissionMode", value: "auto" }),
      expect.objectContaining({ key: "bridge.readOnly", value: true }),
      expect.objectContaining({ key: "bridge.receiptEnabled", value: false }),
      expect.objectContaining({ key: "bridge.richStreamingEnabled", value: false }),
    ]));
    expect(agent.bridge.telegram).toEqual({ token: "tg-token" });
    expect(agent.bridge.permissionMode).toBeUndefined();
    expect(agent.bridge.readOnly).toBeUndefined();
    expect(agent.bridge.receiptEnabled).toBeUndefined();
    expect(agent.bridge.richStreamingEnabled).toBeUndefined();
  });

  it("extracts automation permission mode as a global work setting", () => {
    const partial = {
      automation: { permissionMode: "auto", localDraft: true },
      desk: { heartbeat_interval: 20 },
    };
    const { global: g, agent }: any = splitByScope(partial);

    expect(g).toEqual(expect.arrayContaining([
      expect.objectContaining({ key: "automation.permissionMode", value: "auto" }),
    ]));
    expect(agent.automation).toEqual({ localDraft: true });
    expect(agent.desk.heartbeat_interval).toBe(20);
  });

  it("returns empty global array when no global fields present", () => {
    const partial = { models: ["qwen-plus"], name: "Alice" };
    const { global: g, agent }: any = splitByScope(partial);

    expect(g).toHaveLength(0);
    expect(agent.models).toEqual(["qwen-plus"]);
    expect(agent.name).toBe("Alice");
  });

  it("keeps tools.disabled in agent scope", () => {
    const partial = {
      tools: { disabled: ["browser", "cron"] },
      locale: "zh-CN",
    };
    const { global: g, agent }: any = splitByScope(partial);

    expect(g).toEqual(expect.arrayContaining([
      expect.objectContaining({ key: "locale", value: "zh-CN" }),
    ]));
    expect(g).not.toEqual(expect.arrayContaining([
      expect.objectContaining({ key: "tools.disabled" }),
    ]));
    expect(agent.tools).toEqual({ disabled: ["browser", "cron"] });
  });

  it("returns empty agent when only global fields present", () => {
    const partial = { locale: "en", sandbox: true, update_channel: "beta", keep_awake: true };
    const { global: g, agent } = splitByScope(partial);

    expect(g.length).toBeGreaterThan(0);
    expect(g).toEqual(expect.arrayContaining([
      expect.objectContaining({ key: "keep_awake", value: true }),
    ]));
    expect(Object.keys(agent)).toHaveLength(0);
  });

  it("extracts network_proxy as a top-level global field", () => {
    const partial = {
      network_proxy: { mode: "manual", httpProxy: "http://127.0.0.1:7890" },
      models: { chat: { id: "gpt-4.1", provider: "openai" } },
    };
    const { global: g, agent }: any = splitByScope(partial);

    expect(g).toEqual(expect.arrayContaining([
      expect.objectContaining({ key: "network_proxy", value: partial.network_proxy }),
    ]));
    expect(agent.network_proxy).toBeUndefined();
    expect(agent.models).toEqual(partial.models);
  });

  it("handles empty partial", () => {
    const { global: g, agent } = splitByScope({});

    expect(g).toHaveLength(0);
    expect(agent).toEqual({});
  });

  it("does not mutate original partial nested objects", () => {
    const caps = { learn_skills: true, other: "x" };
    const partial = { capabilities: caps };
    splitByScope(partial);

    // The original nested object must remain untouched
    expect(caps.learn_skills).toBe(true);
    expect(caps.other).toBe("x");
  });
});

// ---------------------------------------------------------------------------
// injectGlobalFields
// ---------------------------------------------------------------------------

describe("injectGlobalFields", () => {
  it("injects all global fields from engine getters", () => {
    const engine = {
      getLocale: () => "ja",
      getTimezone: () => "Asia/Tokyo",
      getSandbox: () => false,
      getSandboxNetwork: () => true,
      getHardwareAcceleration: () => false,
      getUpdateChannel: () => "beta",
      getThinkingLevel: () => "high",
      getLearnSkills: () => true,
      getHeartbeatMaster: () => true,
      getBridgePermissionMode: () => "auto",
      getBridgeReadOnly: () => true,
      getBridgeReceiptEnabled: () => false,
      getBridgeRichStreamingEnabled: () => false,
      getAutomationPermissionMode: () => "auto",
      getNetworkProxy: () => ({ mode: "direct" }),
      getKeepAwake: () => true,
    };
    const config: any = {};
    injectGlobalFields(config, engine);

    expect(config.locale).toBe("ja");
    expect(config.timezone).toBe("Asia/Tokyo");
    expect(config.sandbox).toBe(false);
    expect(config.sandbox_network).toBe(true);
    expect(config.hardware_acceleration).toBe(false);
    expect(config.update_channel).toBe("beta");
    expect(config.thinking_level).toBe("high");
    expect(config.capabilities?.learn_skills).toBe(true);
    expect(config.desk?.heartbeat_master).toBe(true);
    expect(config.bridge?.permissionMode).toBe("auto");
    expect(config.bridge?.readOnly).toBe(true);
    expect(config.bridge?.receiptEnabled).toBe(false);
    expect(config.bridge?.richStreamingEnabled).toBe(false);
    expect(config.automation?.permissionMode).toBe("auto");
    expect(config.network_proxy).toEqual({ mode: "direct" });
    expect(config.keep_awake).toBe(true);
  });

  it("skips getters that don't exist on engine (doesn't throw)", () => {
    // Engine only implements a subset of getters
    const engine = {
      getLocale: () => "en",
    };
    const config: any = {};
    expect(() => injectGlobalFields(config, engine)).not.toThrow();
    expect(config.locale).toBe("en");
    // Fields whose getters are absent should not appear
    expect(config.sandbox).toBeUndefined();
  });

  it("creates nested parent (capabilities, desk) if not present", () => {
    const engine = {
      getLearnSkills: () => false,
      getHeartbeatMaster: () => false,
      getBridgePermissionMode: () => "read_only",
      getBridgeReadOnly: () => false,
      getBridgeReceiptEnabled: () => true,
      getBridgeRichStreamingEnabled: () => true,
    };
    const config: any = {};
    injectGlobalFields(config, engine);

    expect(config.capabilities).toBeDefined();
    expect(config.capabilities.learn_skills).toBe(false);
    expect(config.desk).toBeDefined();
    expect(config.desk.heartbeat_master).toBe(false);
    expect(config.bridge).toBeDefined();
    expect(config.bridge.permissionMode).toBe("read_only");
    expect(config.bridge.readOnly).toBe(false);
    expect(config.bridge.receiptEnabled).toBe(true);
    expect(config.bridge.richStreamingEnabled).toBe(true);
  });
});
