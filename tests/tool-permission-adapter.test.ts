import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { PluginManager } from "../core/plugin-manager.ts";
import { EventBus } from "../hub/event-bus.ts";
import {
  normalizeToolPermissionContract,
} from "../lib/tools/invocation/permission-adapter.ts";
import {
  ToolInvocationError,
  createPluginToolIdentity,
} from "../lib/tools/invocation/index.ts";

const temporaryDirectories: string[] = [];

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

function identity(pluginId: string, publicName: string) {
  const prefix = `${pluginId}_`;
  const localName = publicName.startsWith(prefix) ? publicName.slice(prefix.length) : publicName;
  return createPluginToolIdentity({
    pluginId,
    publicName,
    capabilityBase: localName,
  });
}

function expectInvocationError(call: () => unknown, code: ToolInvocationError["code"]) {
  try {
    call();
  } catch (error) {
    expect(error).toBeInstanceOf(ToolInvocationError);
    expect(error).toMatchObject({ code });
    return;
  }
  throw new Error(`expected ${code}`);
}

describe("工具权限方言适配器", () => {
  it("把 legacy readOnly 转换成规范化只读解析器", () => {
    const tool = {
      name: "office_status",
      _pluginId: "office",
      sessionPermission: { readOnly: true },
    };

    const contract = normalizeToolPermissionContract(tool, identity("office", tool.name));

    expect(contract.source).toBe("legacy");
    expect(contract.legacyRoutineAutoAllow).toBe(false);
    expect(contract.resolveInvocation({})).toEqual({
      action: "read",
      kind: "read",
      capability: "status.read",
    });
  });

  it("把 legacy side effect 和描述函数放进同一规范化描述", () => {
    const tool = {
      name: "media_generate-image",
      _pluginId: "media",
      sessionPermission: {
        kind: "external_side_effect",
        describeSideEffect: (input: Record<string, unknown>) => ({
          kind: "external_generation",
          summary: `Generate ${input.prompt}`,
          ruleId: "media-image-generation",
        }),
      },
    };

    const descriptor = normalizeToolPermissionContract(
      tool,
      identity("media", tool.name),
    ).resolveInvocation({ prompt: "cover" });

    expect(descriptor).toEqual({
      action: "execute",
      kind: "review",
      capability: "generate-image.execute",
      sideEffect: {
        kind: "external_generation",
        summary: "Generate cover",
        ruleId: "media-image-generation",
      },
    });
  });

  it("以新方言为主并接受与 legacy 一致的声明", () => {
    const tool = {
      name: "calendar_create",
      _pluginId: "calendar",
      sessionPermission: {
        kind: "review",
        describeSideEffect: () => ({ kind: "external_write", ruleId: "calendar-create" }),
        resolveInvocation: () => ({
          action: "create",
          kind: "review",
          capability: "create.create",
        }),
      },
    };

    const contract = normalizeToolPermissionContract(tool, identity("calendar", tool.name));

    expect(contract.source).toBe("resolver");
    expect(contract.legacyRoutineAutoAllow).toBe(false);
    expect(contract.resolveInvocation({})).toEqual({
      action: "create",
      kind: "review",
      capability: "create.create",
      sideEffect: { kind: "external_write", ruleId: "calendar-create" },
    });
  });

  it("让 legacy 与新方言产生相同的规范化调用语义", () => {
    const legacyTool = {
      name: "docs_status",
      _pluginId: "docs",
      sessionPermission: { readOnly: true },
    };
    const resolverTool = {
      name: "docs_status",
      _pluginId: "docs",
      sessionPermission: {
        resolveInvocation: () => ({
          action: "read",
          kind: "read",
          capability: "status.read",
        }),
      },
    };

    expect(normalizeToolPermissionContract(
      resolverTool,
      identity("docs", resolverTool.name),
    ).resolveInvocation({})).toEqual(normalizeToolPermissionContract(
      legacyTool,
      identity("docs", legacyTool.name),
    ).resolveInvocation({}));
  });

  it("对 capability 错配、双方言矛盾和缺失权限声明 fail-closed", () => {
    const mismatched = {
      name: "calendar_create",
      _pluginId: "calendar",
      sessionPermission: {
        resolveInvocation: () => ({
          action: "create",
          kind: "review",
          capability: "other.create",
        }),
      },
    };
    const conflicting = {
      name: "calendar_read",
      _pluginId: "calendar",
      sessionPermission: {
        readOnly: true,
        resolveInvocation: () => ({
          action: "write",
          kind: "review",
          capability: "read.write",
        }),
      },
    };

    expectInvocationError(
      () => normalizeToolPermissionContract(mismatched, identity("calendar", mismatched.name))
        .resolveInvocation({}),
      "CAPABILITY_MISMATCH",
    );
    expectInvocationError(
      () => normalizeToolPermissionContract(conflicting, identity("calendar", conflicting.name))
        .resolveInvocation({}),
      "PERMISSION_CONTRACT_CONFLICT",
    );
    expectInvocationError(
      () => normalizeToolPermissionContract(
        { name: "calendar_missing", _pluginId: "calendar" },
        identity("calendar", "calendar_missing"),
      ),
      "PERMISSION_CONTRACT_MISSING",
    );
  });

  it("拒绝 legacy 副作用描述中的宿主身份字段", () => {
    const tool = {
      name: "calendar_create",
      _pluginId: "calendar",
      sessionPermission: {
        kind: "external_side_effect",
        describeSideEffect: () => ({ kind: "external_write", sessionId: "forged" }),
      },
    };

    expectInvocationError(
      () => normalizeToolPermissionContract(tool, identity("calendar", tool.name))
        .resolveInvocation({}),
      "PERMISSION_CONTRACT_CONFLICT",
    );
  });

  it("真实 PluginManager 为 12 个 bundled 工具建立有效规范化契约", async () => {
    const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-permission-adapter-"));
    temporaryDirectories.push(dataDir);
    const bus = new EventBus();
    for (const type of [
      "provider:register-runtime-media-capability-source",
      "provider:unregister-runtime-media-capability-source",
      "media-gen:register-adapter",
      "media-gen:unregister-adapter",
    ]) {
      bus.handle(type, async () => ({ ok: true }));
    }
    const manager = new PluginManager({
      pluginsDirs: [path.resolve("plugins")],
      dataDir,
      bus,
      runtimeContext: {
        serverId: "permission-adapter-test",
        serverNodeId: "permission-adapter-node",
        userId: "permission-adapter-user",
        studioId: "permission-adapter-studio",
        connectionKind: "local",
        credentialKind: "loopback_token",
      },
    } as never);

    manager.scan();
    try {
      await manager.loadAll();
      const names = [
        "media_generate-image",
        "media_generate-video",
        "media_describe-options",
        "media_get-guide",
        "beautify_create-cover",
        "beautify_apply-cover-candidate",
        "beautify_get-cover-style-guide",
        "beautify_get-html-style-guide",
        "beautify_list-capabilities",
        "office_list-capabilities",
        "office_read-document",
        "office_html-to-pdf",
      ];
      const tools = manager.getAllTools().filter((tool) => names.includes(tool.name));
      const kinds = tools.map((tool) => tool._normalizedPermissionContract.resolveInvocation({}).kind);

      expect(tools.map((tool) => tool.name).sort()).toEqual([...names].sort());
      expect(kinds.filter((kind) => kind === "read")).toHaveLength(7);
      expect(kinds.filter((kind) => kind === "review")).toHaveLength(5);
    } finally {
      for (const id of ["media", "jimeng-cli", "beautify", "office"]) {
        await manager.unloadPlugin(id, { source: "builtin" });
      }
    }
  });
});
