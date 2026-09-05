import { describe, expect, it } from "vitest";
import {
  createFirstPartyToolIdentity,
  createMcpToolIdentity,
  createPluginToolIdentity,
} from "../lib/tools/invocation/index.ts";

describe("规范化工具目标身份", () => {
  it("为三类来源生成互不碰撞且稳定的 TargetId", () => {
    const firstParty = createFirstPartyToolIdentity({
      publicName: "shared/name",
      capabilityBase: "shared.read",
    });
    const plugin = createPluginToolIdentity({
      pluginId: "team/plugin",
      publicName: "shared/name",
      capabilityBase: "plugin.shared",
    });
    const mcp = createMcpToolIdentity({
      serverId: "team/plugin",
      remoteToolName: "shared/name",
      publicName: "shared/name",
      capabilityBase: "registered.remote.capability",
    });

    expect(firstParty.targetId).toBe("tool:first-party:shared%2Fname");
    expect(plugin.targetId).toBe("tool:plugin:team%2Fplugin:shared%2Fname");
    expect(mcp.targetId).toBe("tool:mcp:team%2Fplugin:shared%2Fname");
    expect(new Set([firstParty.targetId, plugin.targetId, mcp.targetId])).toHaveLength(3);
  });

  it("只在插件注册时剥离一次精确的 pluginId 前缀", () => {
    const prefixed = createPluginToolIdentity({
      pluginId: "media",
      publicName: "media_media_generate-image",
      capabilityBase: "media_media_generate-image",
    });
    const unprefixed = createPluginToolIdentity({
      pluginId: "media",
      publicName: "generate-image",
      capabilityBase: "generate-image",
    });

    expect(prefixed.localName).toBe("media_generate-image");
    expect(prefixed.targetId).toBe("tool:plugin:media:media_generate-image");
    expect(unprefixed.localName).toBe("generate-image");
  });

  it("保持显示名、远端名、授权能力与 TargetId 相互独立", () => {
    const identity = createMcpToolIdentity({
      serverId: "calendar prod",
      remoteToolName: "events/create?dry=false",
      publicName: "mcp_calendar_create",
      capabilityBase: "calendar.events.write",
    });

    expect(identity).toEqual({
      targetId: "tool:mcp:calendar%20prod:events%2Fcreate%3Fdry%3Dfalse",
      origin: "mcp",
      sourceId: "calendar prod",
      localName: "events/create?dry=false",
      publicName: "mcp_calendar_create",
      capabilityBase: "calendar.events.write",
    });
    expect(Object.isFrozen(identity)).toBe(true);
  });
});
