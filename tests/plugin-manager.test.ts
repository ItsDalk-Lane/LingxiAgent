import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import { PluginManager } from "../core/plugin-manager.ts";

const tmpHome = path.join(os.tmpdir(), "hana-pm-test-" + Date.now());
const pluginsDir = path.join(tmpHome, "plugins");
const dataDir = path.join(tmpHome, "plugin-data");

beforeEach(() => {
  fs.mkdirSync(pluginsDir, { recursive: true });
  fs.mkdirSync(dataDir, { recursive: true });
});
afterEach(() => {
  fs.rmSync(tmpHome, { recursive: true, force: true });
});

async function makeBus() {
  const { EventBus } = await import("../hub/event-bus.ts");
  return new EventBus();
}

function writePluginToolFixture(filePath: string, source: string) {
  const explicitSource = source.includes("sessionPermission")
    ? source
    : `${source}\nexport const sessionPermission = { readOnly: true };\n`;
  fs.writeFileSync(filePath, explicitSource);
}

describe("scan", () => {
  it("discovers plugin from directory with manifest.json", async () => {
    const dir = path.join(pluginsDir, "my-plugin");
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, "manifest.json"), JSON.stringify({
      id: "my-plugin", name: "My Plugin", version: "1.0.0",
    }));
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    const plugins = pm.scan();
    expect(plugins).toHaveLength(1);
    expect(plugins[0].id).toBe("my-plugin");
    expect(plugins[0].name).toBe("My Plugin");
  });

  it("infers id from directory name when no manifest", async () => {
    const dir = path.join(pluginsDir, "simple-tool");
    fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
    writePluginToolFixture(path.join(dir, "tools", "hello.js"),
      'export const name = "hello";\nexport const description = "test";\nexport const parameters = {};\nexport async function execute() { return "hi"; }\n');
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    const plugins = pm.scan();
    expect(plugins).toHaveLength(1);
    expect(plugins[0].id).toBe("simple-tool");
  });

  it("detects contribution types from subdirectories", async () => {
    const dir = path.join(pluginsDir, "multi");
    fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
    fs.mkdirSync(path.join(dir, "skills"), { recursive: true });
    writePluginToolFixture(path.join(dir, "tools", "t.js"), "export const name='t';");
    fs.writeFileSync(path.join(dir, "skills", "s.md"), "---\nname: s\n---\n# S");
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    const plugins = pm.scan();
    expect(plugins[0].contributions).toContain("tools");
    expect(plugins[0].contributions).toContain("skills");
  });

  it("skips hidden directories and non-directories", async () => {
    fs.mkdirSync(path.join(pluginsDir, ".hidden"), { recursive: true });
    fs.writeFileSync(path.join(pluginsDir, "README.md"), "hi");
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    expect(pm.scan()).toHaveLength(0);
  });

  it("invalid manifest.json logs error and skips plugin", async () => {
    const dir = path.join(pluginsDir, "bad");
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, "manifest.json"), "NOT JSON");
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    expect(pm.scan()).toHaveLength(0);
  });

  it("marks manually copied OpenClaw plugin directories as incompatible", async () => {
    const dir = path.join(pluginsDir, "openclaw-voice");
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, "openclaw.plugin.json"), JSON.stringify({
      id: "openclaw-voice",
      name: "OpenClaw Voice",
      configSchema: { type: "object", additionalProperties: false },
    }));
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);

    pm.scan();
    await pm.loadAll();
    const entry = pm.getPlugin("openclaw-voice");

    expect(entry.status).toBe("incompatible");
    expect(entry.error).toMatch(/OpenClaw plugin/i);
    expect(pm.getDiagnostics()).toEqual(expect.arrayContaining([
      expect.objectContaining({
        id: "openclaw-voice",
        status: "incompatible",
        error: expect.stringMatching(/OpenClaw plugin/i),
      }),
    ]));
  });
});

describe("loadAll", () => {
  it("loads real bundled media, jimeng-cli, beautify, and office plugin contributions", async () => {
    const bus = await makeBus();
    for (const type of [
      "provider:register-runtime-media-capability-source",
      "provider:unregister-runtime-media-capability-source",
      "media-gen:register-adapter",
      "media-gen:unregister-adapter",
    ]) {
      bus.handle(type, async () => ({ ok: true }));
    }
    const pm = new PluginManager({
      pluginsDirs: [path.resolve("plugins")],
      dataDir,
      bus,
      runtimeContext: {
        serverId: "server_builtin_smoke",
        serverNodeId: "node_builtin_smoke",
        userId: "user_builtin_smoke",
        studioId: "studio_builtin_smoke",
        connectionKind: "local",
        credentialKind: "loopback_token",
      },
    } as any);

    pm.scan();
    try {
      await pm.loadAll();

      const diagnosticsById = new Map(pm.getDiagnostics().map((entry) => [entry.id, entry]));
      // MCP is a core module owned by the engine, not a plugin: the plugin host
      // must not discover it at all.
      expect(diagnosticsById.has("mcp")).toBe(false);
      expect(pm.routeRegistry.has("mcp")).toBe(false);
      for (const id of ["media", "jimeng-cli", "beautify", "office"]) {
        expect(diagnosticsById.get(id)).toMatchObject({
          id,
          source: "builtin",
          hidden: true,
          status: "loaded",
          error: null,
        });
      }

      const toolNames = pm.getAllTools().map((tool) => tool.name);
      expect(toolNames).toEqual(expect.arrayContaining([
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
      ]));
      // 内置插件的指南走工具，不贡献 skill 目录：它们的安装路径带版本号，
      // 冻结进会话快照后会在下次服务端更新时失效。
      expect(pm.getSkillPaths()).toHaveLength(0);
      expect(pm.getConfigSchema("beautify")?.properties).toHaveProperty("coverResolution");
    } finally {
      for (const id of ["media", "jimeng-cli", "beautify", "office"]) {
        await pm.unloadPlugin(id, { source: "builtin" });
      }
    }
  });

  it("loads plugin with index.js and calls onload", async () => {
    const dir = path.join(pluginsDir, "stateful");
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, "index.js"), `
      export default class TestPlugin {
        async onload() { this.loaded = true; }
      }
    `);
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();
    const entry = pm.getPlugin("stateful");
    expect(entry.status).toBe("loaded");
    expect(entry.instance.loaded).toBe(true);
  });

  it("passes runtime scope into lifecycle and tool contexts", async () => {
    const runtimeContext = {
      serverId: "server_plugin",
      serverNodeId: "node_plugin",
      userId: "user_plugin",
      studioId: "studio_plugin",
      connectionKind: "local",
      credentialKind: "loopback_token",
      platformAccountId: null,
      officialServiceKind: null,
      executionBoundary: {
        schemaVersion: 1,
        boundaryId: "execb_node_plugin_studio_plugin",
        kind: "local_process",
        serverNodeId: "node_plugin",
        studioId: "studio_plugin",
      },
    };
    const dir = path.join(pluginsDir, "scope-plugin");
    fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
    fs.writeFileSync(path.join(dir, "manifest.json"), JSON.stringify({
      id: "scope-plugin",
      trust: "full-access",
      activationEvents: ["onStartup"],
    }));
    fs.writeFileSync(path.join(dir, "index.js"), `
      export default class ScopePlugin {
        async onload() {
          globalThis.__hanaScopePluginLifecycle = {
            serverId: this.ctx.serverId,
            serverNodeId: this.ctx.serverNodeId,
            userId: this.ctx.userId,
            studioId: this.ctx.studioId,
            connectionKind: this.ctx.connectionKind,
            credentialKind: this.ctx.credentialKind,
          };
        }
      }
    `);
    writePluginToolFixture(path.join(dir, "tools", "scope.js"), `
      export const name = "scope";
      export const description = "Return scope";
      export const parameters = {};
      export async function execute(_input, ctx) {
        return JSON.stringify({
          serverId: ctx.serverId,
          serverNodeId: ctx.serverNodeId,
          userId: ctx.userId,
          studioId: ctx.studioId,
          sessionPath: ctx.sessionPath,
        });
      }
    `);
    const pm = new PluginManager({
      pluginsDir,
      dataDir,
      bus: await makeBus(),
      runtimeContext,
    } as any);
    pm.scan();
    await pm.loadAll();

    expect(globalThis.__hanaScopePluginLifecycle).toEqual({
      serverId: "server_plugin",
      serverNodeId: "node_plugin",
      userId: "user_plugin",
      studioId: "studio_plugin",
      connectionKind: "local",
      credentialKind: "loopback_token",
    });
    const tool = pm.getAllTools()[0];
    const result = await tool.execute("call-1", {}, {
      sessionManager: { getSessionFile: () => "/sessions/plugin-scope.jsonl" },
    });
    expect(JSON.parse(result.content[0].text)).toEqual({
      serverId: "server_plugin",
      serverNodeId: "node_plugin",
      userId: "user_plugin",
      studioId: "studio_plugin",
      sessionPath: "/sessions/plugin-scope.jsonl",
    });
    delete globalThis.__hanaScopePluginLifecycle;
  });

  it("prefers explicit runtime sessionPath over focus fallback for plugin tools", async () => {
    const dir = path.join(pluginsDir, "session-path-plugin");
    fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
    writePluginToolFixture(path.join(dir, "tools", "scope.js"), `
      export const name = "scope";
      export const description = "Return session path";
      export const parameters = {};
      export async function execute(_input, ctx) {
        return ctx.sessionPath || "";
      }
    `);
    const pm = new PluginManager({
      pluginsDir,
      dataDir,
      bus: await makeBus(),
      getSessionPath: () => "/sessions/focus.jsonl",
    } as any);
    pm.scan();
    await pm.loadAll();

    const tool = pm.getAllTools()[0];
    const result = await tool.execute("call-1", {}, {
      sessionPath: "/sessions/bridge-owner.jsonl",
    });

    expect(result.content[0].text).toBe("/sessions/bridge-owner.jsonl");
  });

  it("uses the Pi SDK fifth argument session ctx for static plugin tools", async () => {
    const dir = path.join(pluginsDir, "pi-context-plugin");
    fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
    writePluginToolFixture(path.join(dir, "tools", "scope.js"), `
      export const name = "scope";
      export const description = "Return session path";
      export const parameters = {};
      export async function execute(_input, ctx) {
        return ctx.sessionPath || "";
      }
    `);
    const pm = new PluginManager({
      pluginsDir,
      dataDir,
      bus: await makeBus(),
    } as any);
    pm.scan();
    await pm.loadAll();

    const tool = pm.getAllTools()[0];
    const result = await tool.execute("call-1", {}, new AbortController().signal, vi.fn(), {
      sessionManager: { getSessionFile: () => "/sessions/pi-context.jsonl" },
    });

    expect(result.content[0].text).toBe("/sessions/pi-context.jsonl");
  });

  it("provides register() on instance and cleans up on unload", async () => {
    const dir = path.join(pluginsDir, "reg-test");
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, "index.js"), `
      export default class RegPlugin {
        async onload() {
          this.register(() => { globalThis.__regTestCleanup = true; });
        }
      }
    `);
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();
    await pm.unloadPlugin("reg-test");
    expect(globalThis.__regTestCleanup).toBe(true);
    delete globalThis.__regTestCleanup;
  });

  it("failed onload marks plugin as failed, does not block others", async () => {
    const bad = path.join(pluginsDir, "bad-plugin");
    fs.mkdirSync(bad, { recursive: true });
    fs.writeFileSync(path.join(bad, "index.js"), `
      export default class Bad { async onload() { throw new Error("boom"); } }
    `);
    const good = path.join(pluginsDir, "good-plugin");
    fs.mkdirSync(path.join(good, "tools"), { recursive: true });
    writePluginToolFixture(path.join(good, "tools", "t.js"), "export const name='t';");
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();
    expect(pm.getPlugin("bad-plugin").status).toBe("failed");
    expect(pm.getPlugin("good-plugin").status).toBe("loaded");
  });

  it("timed out onload marks plugin as failed, does not block startup", async () => {
    const stuck = path.join(pluginsDir, "stuck-plugin");
    fs.mkdirSync(stuck, { recursive: true });
    fs.writeFileSync(path.join(stuck, "index.js"), `
      export default class Stuck { async onload() { await new Promise(() => {}); } }
    `);
    const good = path.join(pluginsDir, "after-stuck");
    fs.mkdirSync(good, { recursive: true });
    const pm = new PluginManager({
      pluginsDir,
      dataDir,
      bus: await makeBus(),
      lifecycleTimeoutMs: 20,
    } as any);
    pm.scan();

    const result = await Promise.race([
      pm.loadAll().then(() => "loaded"),
      new Promise((resolve) => setTimeout(() => resolve("hung"), 100)),
    ]);

    expect(result).toBe("loaded");
    expect(pm.getPlugin("stuck-plugin").status).toBe("failed");
    expect(pm.getPlugin("stuck-plugin").error).toMatch(/timed out/i);
    expect(pm.getPlugin("after-stuck").status).toBe("loaded");
  });

  it("plugin without index.js loads as static (no lifecycle)", async () => {
    const dir = path.join(pluginsDir, "static-only");
    fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
    writePluginToolFixture(path.join(dir, "tools", "t.js"), "export const name='t';");
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();
    expect(pm.getPlugin("static-only").status).toBe("loaded");
    expect(pm.getPlugin("static-only").instance).toBeNull();
  });

  it("keeps lifecycle inactive until matching tool activation event", async () => {
    const dir = path.join(pluginsDir, "lazy-tool");
    fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
    fs.writeFileSync(path.join(dir, "manifest.json"), JSON.stringify({
      id: "lazy-tool",
      name: "Lazy Tool",
      version: "1.0.0",
      activationEvents: ["onToolCall:run"],
    }));
    fs.writeFileSync(path.join(dir, "index.js"), `
      export default class LazyTool {
        async onload() { globalThis.__lazyToolActivated = true; }
      }
    `);
    writePluginToolFixture(path.join(dir, "tools", "run.js"), `
      export const name = "run";
      export const description = "Run lazy tool";
      export const parameters = {};
      export async function execute() { return "ok"; }
    `);
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();

    expect(pm.getPlugin("lazy-tool").activationState).toBe("inactive");
    expect(globalThis.__lazyToolActivated).toBeUndefined();

    await pm.getAllTools()[0].execute("call", {}, {});

    expect(globalThis.__lazyToolActivated).toBe(true);
    expect(pm.getPlugin("lazy-tool").activationState).toBe("activated");
    delete globalThis.__lazyToolActivated;
  });

  it("defaults old lifecycle plugins to onStartup activation", async () => {
    const dir = path.join(pluginsDir, "legacy-lifecycle");
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, "index.js"), `
      export default class LegacyLifecycle {
        async onload() { globalThis.__legacyLifecycleActivated = true; }
      }
    `);
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();

    expect(pm.getPlugin("legacy-lifecycle").activationEvents).toEqual(["onStartup"]);
    expect(pm.getPlugin("legacy-lifecycle").activationState).toBe("activated");
    expect(globalThis.__legacyLifecycleActivated).toBe(true);
    delete globalThis.__legacyLifecycleActivated;
  });

  it("stores ctx on entry after loading", async () => {
    const dir = path.join(pluginsDir, "ctx-test");
    fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
    writePluginToolFixture(path.join(dir, "tools", "t.js"),
      'export const name = "t";\nexport const description = "test";\nexport const parameters = {};\nexport async function execute() { return "ok"; }\n');
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();
    const entry = pm.getPlugin("ctx-test");
    expect(entry).toBeTruthy();
    expect(entry.ctx).toBeTruthy();
    expect(entry.ctx.pluginId).toBeTruthy();
    expect(entry.ctx.bus).toBeTruthy();
    expect(entry.ctx.config).toBeTruthy();
    expect(entry.ctx.log).toBeTruthy();
  });
});

describe("tool loading", () => {
  it("loads tools from tools/ directory with namespace prefix", async () => {
    const dir = path.join(pluginsDir, "search-plugin");
    fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
    writePluginToolFixture(path.join(dir, "tools", "web-search.js"), `
      export const name = "web-search";
      export const description = "Search the web";
      export const parameters = { type: "object", properties: { query: { type: "string" } } };
      export async function execute(input) { return "results for " + input.query; }
    `);
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();
    const tools = pm.getAllTools();
    expect(tools).toHaveLength(1);
    expect(tools[0].name).toBe("search-plugin_web-search");
    expect(tools[0].description).toBe("Search the web");
  });

  it("copies static plugin tool runtime availability checks", async () => {
    const dir = path.join(pluginsDir, "gated-plugin");
    fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
    writePluginToolFixture(path.join(dir, "tools", "paint.js"), `
      export const name = "paint";
      export const description = "Paint";
      export const parameters = {};
      export function isEnabledForAgentConfig(config) {
        return config?.tools?.disabled?.includes("beautify") !== true;
      }
      export async function execute() { return "ok"; }
    `);
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();
    const tool = pm.getAllTools()[0];
    expect(tool.name).toBe("gated-plugin_paint");
    expect(tool.isEnabledForAgentConfig({ tools: { disabled: [] } })).toBe(true);
    expect(tool.isEnabledForAgentConfig({ tools: { disabled: ["beautify"] } })).toBe(false);
  });

  it("preserves the explicit static tool metadata required by canonical target assembly", async () => {
    const dir = path.join(pluginsDir, "metadata-plugin");
    fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
    writePluginToolFixture(path.join(dir, "tools", "inspect.js"), `
      export const name = "inspect";
      export const label = "Inspect safely";
      export const description = "Inspect a resource";
      export const parameters = {
        type: "object",
        properties: { resourceId: { type: "string" } },
        required: ["resourceId"],
      };
      export const deferrable = false;
      export const pinned = true;
      export const sessionPermission = { readOnly: true };
      export const arbitraryHostField = "must-not-cross-the-adapter";
      export async function execute() { return "ok"; }
    `);
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();

    const tool = pm.getAllTools()[0];
    expect(tool).toMatchObject({
      name: "metadata-plugin_inspect",
      label: "Inspect safely",
      description: "Inspect a resource",
      deferrable: false,
      pinned: true,
      _pluginId: "metadata-plugin",
      parameters: {
        type: "object",
        required: ["resourceId"],
      },
      _toolTargetIdentity: {
        origin: "plugin",
        sourceId: "metadata-plugin",
      },
    });
    expect(tool.sessionPermission?.resolveInvocation).toBeTypeOf("function");
    expect(tool).not.toHaveProperty("arbitraryHostField");
  });

  it("invokes static plugin tools through the unified tool adapter", async () => {
    const dir = path.join(pluginsDir, "static-invoke");
    fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
    writePluginToolFixture(path.join(dir, "tools", "echo.js"), `
      export const name = "echo";
      export const description = "Echo text";
      export const parameters = {};
      export async function execute(input, ctx) {
        return (ctx.sessionPath || "") + ":" + input.text;
      }
    `);
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();

    const tool = pm.getPluginTool("static-invoke", "echo");
    const result = await pm.executePluginTool(tool, {
      toolCallId: "call-static",
      input: { text: "hello" },
      runtimeCtx: { sessionPath: "/sessions/static.jsonl" },
    });

    expect(result.content[0].text).toBe("/sessions/static.jsonl:hello");
  });

  it("normalizes static plugin tool sessionPermission metadata", async () => {
    const dir = path.join(pluginsDir, "static-permission");
    fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
    writePluginToolFixture(path.join(dir, "tools", "status.js"), `
      export const name = "status";
      export const description = "Read plugin status";
      export const parameters = {};
      export const sessionPermission = { readOnly: true };
      export async function execute() {
        return "ok";
      }
    `);
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();

    const tool = pm.getPluginTool("static-permission", "status");

    expect(tool.sessionPermission.resolveInvocation({})).toEqual({
      action: "read",
      kind: "read",
      capability: "status.read",
    });
  });

  it("finds plugin tools when the action id contains underscores", async () => {
    const dir = path.join(pluginsDir, "underscore-plugin");
    fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
    writePluginToolFixture(path.join(dir, "tools", "create-note.js"), `
      export const name = "create_note";
      export const description = "Create note";
      export const parameters = {};
      export async function execute(input) {
        return input.title;
      }
    `);
    writePluginToolFixture(path.join(dir, "tools", "archive.js"), `
      export const name = "underscore-plugin_archive";
      export const description = "Archive note";
      export const parameters = {};
      export async function execute(input) {
        return input.title;
      }
    `);
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();

    expect(pm.getPluginTool("underscore-plugin", "create_note")?.name)
      .toBe("underscore-plugin_create_note");
    expect(pm.getPluginTool("underscore-plugin", "underscore-plugin_create_note")?.name)
      .toBe("underscore-plugin_create_note");
    expect(pm.getPluginTool("underscore-plugin", "underscore-plugin_archive")?.name)
      .toBe("underscore-plugin_underscore-plugin_archive");
  });

  it("exposes a session file registration helper to plugin tools", async () => {
    const registerSessionFile = vi.fn(({ sessionPath, filePath, label, origin, storageKind }) => ({
      id: "sf_plugin_output",
      sessionPath,
      filePath,
      label,
      origin,
      storageKind,
    }));
    const dir = path.join(pluginsDir, "file-plugin");
    fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
    writePluginToolFixture(path.join(dir, "tools", "stage.js"), `
      export const name = "stage";
      export const description = "Stage plugin output";
      export const parameters = {};
      export async function execute(input, ctx) {
        const file = ctx.registerSessionFile({
          sessionPath: ctx.sessionPath,
          filePath: "/tmp/plugin-output.png",
          label: "plugin-output.png",
          origin: "plugin_output",
        });
        return { content: [{ type: "text", text: file.fileId || file.id }] };
      }
    `);
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus(), registerSessionFile } as any);
    pm.scan();
    await pm.loadAll();

    const tool = pm.getAllTools()[0];
    const result = await tool.execute("call-1", {}, {
      sessionManager: { getSessionFile: () => "/sessions/plugin.jsonl" },
    });

    expect(registerSessionFile).toHaveBeenCalledWith({
      sessionPath: "/sessions/plugin.jsonl",
      filePath: "/tmp/plugin-output.png",
      label: "plugin-output.png",
      origin: "plugin_output",
      storageKind: "plugin_data",
    });
    expect(result.content[0].text).toBe("sf_plugin_output");
  });

  it("exposes stageFile so plugin tools return SessionFile media items without hand-writing protocol", async () => {
    const registerSessionFile = vi.fn(({ sessionPath, filePath, label, origin, storageKind }) => ({
      id: "sf_plugin_stage",
      sessionPath,
      filePath,
      label,
      origin,
      storageKind,
      mime: "image/png",
      size: 12,
      kind: "image",
    }));
    const dir = path.join(pluginsDir, "stage-file-plugin");
    fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
    writePluginToolFixture(path.join(dir, "tools", "stage.js"), `
      export const name = "stage";
      export const description = "Stage plugin output";
      export const parameters = {};
      export async function execute(input, ctx) {
        const staged = ctx.stageFile({
          sessionPath: ctx.sessionPath,
          filePath: "/tmp/plugin-output.png",
          label: "plugin-output.png",
          origin: "external",
          storageKind: "external",
        });
        return {
          content: [{ type: "text", text: "done" }],
          details: { media: { items: [staged.mediaItem] } },
        };
      }
    `);
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus(), registerSessionFile } as any);
    pm.scan();
    await pm.loadAll();

    const tool = pm.getAllTools()[0];
    const result = await tool.execute("call-1", {}, {
      sessionManager: { getSessionFile: () => "/sessions/plugin.jsonl" },
    });

    expect(registerSessionFile).toHaveBeenCalledWith({
      sessionPath: "/sessions/plugin.jsonl",
      filePath: "/tmp/plugin-output.png",
      label: "plugin-output.png",
      origin: "plugin_output",
      storageKind: "plugin_data",
    });
    expect(result.details.media.items).toEqual([{
      type: "session_file",
      fileId: "sf_plugin_stage",
      sessionPath: "/sessions/plugin.jsonl",
      filePath: "/tmp/plugin-output.png",
      label: "plugin-output.png",
      mime: "image/png",
      size: 12,
      kind: "image",
    }]);
  });

  it("passes ResourceIO-backed resources into plugin tool context", async () => {
    const resourceIO = {
      read: vi.fn(async (ref) => ({
        resourceKey: "mount:docs:note.md",
        resource: ref,
        content: Buffer.from("mounted note"),
      })),
    };
    const dir = path.join(pluginsDir, "resource-tool-plugin");
    fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
    fs.writeFileSync(path.join(dir, "manifest.json"), JSON.stringify({
      id: "resource-tool-plugin",
      capabilities: ["resource.read"],
    }));
    writePluginToolFixture(path.join(dir, "tools", "read-resource.js"), `
      export const name = "read_resource";
      export const description = "Read a resource";
      export const parameters = {};
      export async function execute(input, ctx) {
        const result = await ctx.resources.read(input.resource);
        return result.content.toString("utf-8");
      }
    `);
    const pm = new PluginManager({
      pluginsDir,
      dataDir,
      bus: await makeBus(),
      resourceIO,
    } as any);
    pm.scan();
    await pm.loadAll();

    const tool = pm.getAllTools()[0];
    const result = await tool.execute("call-1", {
      resource: { kind: "mount", mountId: "docs", path: "note.md" },
    }, {
      sessionPath: "/sessions/plugin.jsonl",
    });

    expect(result.content[0].text).toBe("mounted note");
    expect(resourceIO.read).toHaveBeenCalledWith(
      { kind: "mount", mountId: "docs", path: "note.md" },
      expect.objectContaining({
        source: "plugin",
        reason: "plugin:resource-tool-plugin:read",
        sessionPath: "/sessions/plugin.jsonl",
        principal: expect.objectContaining({
          kind: "plugin",
          pluginId: "resource-tool-plugin",
          sessionPath: "/sessions/plugin.jsonl",
        }),
      }),
    );
  });

  it("passes sessionId-first runtime context into plugin tools and staged files", async () => {
    const registerSessionFile = vi.fn(({ sessionId, sessionPath, sessionRef, filePath, label, origin, storageKind }) => ({
      id: "sf_plugin_identity",
      sessionId,
      sessionPath,
      sessionRef,
      filePath,
      label,
      origin,
      storageKind,
    }));
    const dir = path.join(pluginsDir, "stage-id-plugin");
    fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
    writePluginToolFixture(path.join(dir, "tools", "stage.js"), `
      export const name = "stage";
      export const description = "Stage plugin output";
      export const parameters = {};
      export async function execute(input, ctx) {
        const staged = ctx.stageFile({
          filePath: "/tmp/plugin-output.png",
          label: ctx.sessionId + ":" + ctx.sessionRef.sessionId,
        });
        return {
          content: [{ type: "text", text: "done" }],
          details: { media: { items: [staged.mediaItem] } },
        };
      }
    `);
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus(), registerSessionFile } as any);
    pm.scan();
    await pm.loadAll();

    const tool = pm.getAllTools()[0];
    const result = await tool.execute("call-1", {}, {
      sessionId: "sess_plugin_tool",
      sessionPath: "/sessions/plugin.jsonl",
      sessionRef: {
        sessionId: "sess_plugin_tool",
        sessionPath: "/sessions/plugin.jsonl",
        legacySessionPath: "/sessions/legacy.jsonl",
      },
      sessionManager: { getSessionFile: () => "/sessions/ignored.jsonl" },
    });

    expect(registerSessionFile).toHaveBeenCalledWith(expect.objectContaining({
      sessionId: "sess_plugin_tool",
      sessionPath: "/sessions/plugin.jsonl",
      sessionRef: {
        sessionId: "sess_plugin_tool",
        sessionPath: "/sessions/plugin.jsonl",
        legacySessionPath: "/sessions/legacy.jsonl",
      },
      label: "sess_plugin_tool:sess_plugin_tool",
    }));
    expect(result.details.media.items[0]).toMatchObject({
      type: "session_file",
      fileId: "sf_plugin_identity",
      sessionId: "sess_plugin_tool",
      sessionPath: "/sessions/plugin.jsonl",
    });
  });

  it("skips tool files with invalid exports", async () => {
    const dir = path.join(pluginsDir, "bad-tool");
    fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
    writePluginToolFixture(path.join(dir, "tools", "bad.js"), "export const x = 1;");
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();
    expect(pm.getAllTools()).toHaveLength(0);
  });
});

describe("skill paths", () => {
  /** 第一个扫描目录是 builtin，其余是 community（见 scan()）。 */
  function makeSkillPlugin(rootDir: string, pluginId: string) {
    const dir = path.join(rootDir, pluginId);
    fs.mkdirSync(path.join(dir, "skills", "my-skill"), { recursive: true });
    fs.writeFileSync(path.join(dir, "skills", "my-skill", "SKILL.md"),
      "---\nname: my-skill\ndescription: test\n---\n# My Skill");
    return dir;
  }

  // 内置插件随 server artifact 分发，目录名带版本号（artifacts/server/{version}-{platformArch}/）。
  // skill 的绝对路径会被冻结进 session 的 system prompt 快照，artifact 一换代就指向不存在的文件，
  // 模型照着死路径读盘失败。内置插件的指南必须走工具（见 beautify_get-html-style-guide），
  // 工具在运行时解析资源，路径永不进上下文。
  it("rejects skill directories contributed by builtin plugins", async () => {
    makeSkillPlugin(pluginsDir, "builtin-skill-plug");
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    const scanned = pm.scan();
    expect(scanned[0].source).toBe("builtin");
    await pm.loadAll();
    expect(pm.getSkillPaths()).toHaveLength(0);
  });
});

describe("command loading", () => {
  it("loads commands from commands/ directory", async () => {
    const dir = path.join(pluginsDir, "cmd-plug");
    fs.mkdirSync(path.join(dir, "commands"), { recursive: true });
    fs.writeFileSync(path.join(dir, "commands", "hello.js"), `
      export const name = "hello";
      export const description = "Say hello";
      export async function execute(args, ctx) { return "Hello " + args; }
    `);
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();
    const cmds = pm.getAllCommands();
    expect(cmds).toHaveLength(1);
    expect(cmds[0].name).toBe("cmd-plug.hello");
  });
});

describe("extensions", () => {
  it("loads extension factories from extensions/ directory (full-access)", async () => {
    const builtinDir = path.join(tmpHome, "builtin-ext");
    const dir = path.join(builtinDir, "ext-plug");
    fs.mkdirSync(path.join(dir, "extensions"), { recursive: true });
    fs.writeFileSync(path.join(dir, "extensions", "strip.js"), `
      export default function(pi) {
        pi.on("before_provider_request", (event) => {
          return event.payload;
        });
      }
    `);
    const pm = new PluginManager({
      pluginsDirs: [builtinDir],
      dataDir,
      bus: await makeBus(),
    } as any);
    pm.scan();
    await pm.loadAll();
    const factories = pm.getExtensionFactories();
    expect(factories).toHaveLength(1);
    expect(typeof factories[0]).toBe("function");
  });

  it("skips extension files that don't export a function", async () => {
    const builtinDir = path.join(tmpHome, "builtin-ext-bad");
    const dir = path.join(builtinDir, "bad-ext");
    fs.mkdirSync(path.join(dir, "extensions"), { recursive: true });
    fs.writeFileSync(path.join(dir, "extensions", "not-a-fn.js"), `
      export const value = 42;
    `);
    const pm = new PluginManager({
      pluginsDirs: [builtinDir],
      dataDir,
      bus: await makeBus(),
    } as any);
    pm.scan();
    await pm.loadAll();
    expect(pm.getExtensionFactories()).toHaveLength(0);
  });

  it("unloadPlugin removes extension factories for that plugin", async () => {
    const builtinDir = path.join(tmpHome, "builtin-ext-unload");
    const dir = path.join(builtinDir, "unload-ext");
    fs.mkdirSync(path.join(dir, "extensions"), { recursive: true });
    fs.writeFileSync(path.join(dir, "extensions", "e.js"), `
      export default function(pi) { pi.on("tool_call", () => {}); }
    `);
    const pm = new PluginManager({
      pluginsDirs: [builtinDir],
      dataDir,
      bus: await makeBus(),
    } as any);
    pm.scan();
    await pm.loadAll();
    expect(pm.getExtensionFactories()).toHaveLength(1);
    await pm.unloadPlugin("unload-ext");
    expect(pm.getExtensionFactories()).toHaveLength(0);
  });
});

describe("configuration", () => {
  it("reads configuration schema from manifest", async () => {
    const dir = path.join(pluginsDir, "config-plug");
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, "manifest.json"), JSON.stringify({
      id: "config-plug", name: "Config Plugin", version: "1.0.0",
      contributes: { configuration: { properties: {
        interval: { type: "number", default: 25, title: "Interval" },
        enabled: { type: "boolean", default: true, title: "Enabled" },
      }}}
    }));
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();
    const [plugin] = pm.listPlugins();
    expect(plugin.contributions).toContain("configuration");
    const schema = pm.getConfigSchema("config-plug");
    expect(schema.properties.interval.type).toBe("number");
    expect(schema.properties.enabled.default).toBe(true);
  });

  it("getAllConfigSchemas returns schemas for all plugins", async () => {
    const dir = path.join(pluginsDir, "cfg");
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, "manifest.json"), JSON.stringify({
      id: "cfg", name: "C", version: "0.1.0",
      contributes: { configuration: { properties: { x: { type: "string" } } } }
    }));
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();
    const all = pm.getAllConfigSchemas();
    expect(all).toHaveLength(1);
    expect(all[0].pluginId).toBe("cfg");
  });

  it("reads and writes redacted config through the manager", async () => {
    const dir = path.join(pluginsDir, "secret-cfg");
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, "manifest.json"), JSON.stringify({
      id: "secret-cfg", name: "Secret", version: "0.1.0",
      contributes: { configuration: { properties: {
        apiKey: { type: "string", sensitive: true },
        enabled: { type: "boolean", default: true },
      } } }
    }));
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();

    const saved = pm.setConfig("secret-cfg", { apiKey: "secret-value" });
    expect(saved.values.apiKey).toBe("********");
    expect(pm.getConfig("secret-cfg").values).toEqual({ enabled: true, apiKey: "********" });
    expect(pm.getPlugin("secret-cfg").ctx.config.get("apiKey")).toBe("secret-value");
  });

  it("forks per-session config for loaded and disabled plugins without sharing child writes", async () => {
    for (const [id, disabled] of [["loaded-cfg", false], ["disabled-cfg", true]] as const) {
      const dir = path.join(pluginsDir, id);
      fs.mkdirSync(dir, { recursive: true });
      fs.writeFileSync(path.join(dir, "manifest.json"), JSON.stringify({
        id,
        name: id,
        version: "0.1.0",
        contributes: { configuration: { properties: {
          sessionValue: { type: "object", scope: "per-session" },
        } } },
      }));
      const pluginDataDir = path.join(dataDir, id);
      fs.mkdirSync(pluginDataDir, { recursive: true });
      fs.writeFileSync(path.join(pluginDataDir, "config.json"), JSON.stringify({
        schemaVersion: 1,
        global: {},
        agents: {},
        sessions: { "sess-source": { sessionValue: { id, disabled } } },
      }));
    }
    const preferencesManager = {
      getDisabledPlugins: () => ["disabled-cfg"],
    };
    const pm = new PluginManager({
      pluginsDir,
      dataDir,
      bus: await makeBus(),
      preferencesManager,
    } as any);
    pm.scan();
    await pm.loadAll();

    expect(pm.forkSessionConfig({
      sourceSessionId: "sess-source",
      targetSessionId: "sess-child",
    })).toMatchObject({ copied: 2 });
    for (const id of ["loaded-cfg", "disabled-cfg"]) {
      const state = JSON.parse(fs.readFileSync(path.join(dataDir, id, "config.json"), "utf-8"));
      expect(state.sessions["sess-child"]).toEqual(state.sessions["sess-source"]);
    }

    pm.setConfig("loaded-cfg", { sessionValue: { id: "child-write" } }, {
      scope: "per-session",
      sessionId: "sess-child",
    });
    const loadedState = JSON.parse(fs.readFileSync(path.join(dataDir, "loaded-cfg", "config.json"), "utf-8"));
    expect(loadedState.sessions["sess-source"].sessionValue).toEqual({ id: "loaded-cfg", disabled: false });
    expect(pm.discardSessionConfig({ sessionId: "sess-child" })).toMatchObject({ discarded: 2 });
  });
});

describe("agent templates", () => {
  it("loads agent templates from agents/ directory", async () => {
    const dir = path.join(pluginsDir, "agent-plug");
    fs.mkdirSync(path.join(dir, "agents"), { recursive: true });
    fs.writeFileSync(path.join(dir, "agents", "translator.json"), JSON.stringify({
      name: "Translator", systemPrompt: "You are a translator.", defaultModel: "gpt-4o",
    }));
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();
    const templates = pm.getAgentTemplates();
    expect(templates).toHaveLength(1);
    expect(templates[0].name).toBe("Translator");
    expect(templates[0]._pluginId).toBe("agent-plug");
  });
});

describe("provider declarations", () => {
  it("loads provider plugin data from providers/ directory", async () => {
    const dir = path.join(pluginsDir, "prov-plug");
    fs.mkdirSync(path.join(dir, "providers"), { recursive: true });
    fs.writeFileSync(path.join(dir, "providers", "my-llm.js"), `
      export const id = "my-llm";
      export const displayName = "My LLM";
      export const authType = "api-key";
      export const defaultBaseUrl = "https://api.my-llm.com/v1";
      export const defaultApi = "openai-completions";
    `);
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();
    const providers = pm.getProviderPlugins();
    expect(providers).toHaveLength(1);
    expect(providers[0].id).toBe("my-llm");
  });
});

// ── 权限执行 ─────────────────────────────────────────────────────────────────

describe("permission enforcement", () => {
  it("builtin plugin ignores disabled list and always loads", async () => {
    const dir = path.join(pluginsDir, "builtin-always");
    fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
    writePluginToolFixture(path.join(dir, "tools", "t.js"), `
      export const name = "t";
      export const description = "test";
      export const parameters = {};
      export async function execute() { return "ok"; }
    `);

    const mockPrefs = {
      getDisabledPlugins: () => ["builtin-always"],
      getAllowFullAccessPlugins: () => false,
    };
    const pm = new PluginManager({
      pluginsDir,
      dataDir,
      bus: await makeBus(),
      preferencesManager: mockPrefs,
    } as any);
    pm.scan();
    await pm.loadAll();
    const entry = pm.getPlugin("builtin-always");
    expect(entry.status).toBe("loaded");
    expect(entry.source).toBe("builtin");
  });
});

// ── 动态工具注册 ──────────────────────────────────────────────────────────────

describe("addTool (dynamic registration)", () => {
  it("动态工具注册、重注册和移除都会推进插件工具代次", async () => {
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);

    expect(pm.getPluginToolGeneration("dynamic-generation")).toBe(0);
    const removeFirst = pm.addTool("dynamic-generation", {
      name: "search",
      description: "Search",
      sessionPermission: { readOnly: true },
      execute: async () => "first",
    });
    const firstGeneration = pm.getPluginToolGeneration("dynamic-generation");
    expect(firstGeneration).toBeGreaterThan(0);
    expect(pm.isPluginToolCurrentlyAvailable("dynamic-generation", "dynamic-generation_search")).toBe(true);

    const removeSecond = pm.addTool("dynamic-generation", {
      name: "lookup",
      description: "Lookup",
      sessionPermission: { readOnly: true },
      execute: async () => "second",
    });
    expect(pm.getPluginToolGeneration("dynamic-generation")).toBeGreaterThan(firstGeneration);

    const beforeRemoval = pm.getPluginToolGeneration("dynamic-generation");
    removeFirst();
    expect(pm.getPluginToolGeneration("dynamic-generation")).toBeGreaterThan(beforeRemoval);
    expect(pm.isPluginToolCurrentlyAvailable("dynamic-generation", "dynamic-generation_search")).toBe(false);
    removeSecond();
  });

  it("dynamically registered tool appears in getAllTools", async () => {
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    const remove = pm.addTool("mcp-bridge", {
      name: "search",
      description: "MCP search tool",
      sessionPermission: { readOnly: true },
      execute: async () => "result",
    });
    const tools = pm.getAllTools();
    expect(tools).toHaveLength(1);
    expect(tools[0].name).toBe("mcp-bridge_search");
    expect(tools[0]._dynamic).toBe(true);
    expect(tools[0]._dynamicInvocationStyle).toBe("sdk_tool");

    remove();
    expect(pm.getAllTools()).toHaveLength(0);
  });

  it("rejects dynamic plugin tools without an explicit permission contract", async () => {
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);

    expect(() => pm.addTool("mcp-bridge", {
      name: "missing-permission",
      description: "Missing permission",
      execute: async () => "result",
    })).toThrow(expect.objectContaining({ code: "PERMISSION_CONTRACT_MISSING" }));
    expect(pm.getAllTools()).toHaveLength(0);
  });

  it("normalizes dynamic plugin tool sessionPermission metadata", async () => {
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    const sessionPermission = {
      kind: "external_side_effect",
      describeSideEffect: vi.fn(() => ({
        kind: "external_api",
        summary: "Queries a remote MCP server.",
        ruleId: "mcp-remote-query",
      })),
    };

    const remove = pm.addTool("mcp-bridge", {
      name: "github_search",
      description: "MCP search tool",
      sessionPermission,
      execute: async () => "result",
    });

    const [tool] = pm.getAllTools();
    expect(tool.sessionPermission.resolveInvocation({ query: "hana" })).toMatchObject({
      action: "execute",
      kind: "review",
      capability: "github_search.execute",
      sideEffect: {
      kind: "external_api",
      ruleId: "mcp-remote-query",
      },
    });

    remove();
  });

  it("invokes dynamic plugin tools with the SDK input/context signature", async () => {
    const dir = path.join(pluginsDir, "dyn-invoke");
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, "manifest.json"), JSON.stringify({
      id: "dyn-invoke",
      name: "Dynamic Invoke",
      version: "1.0.0",
    }));
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();
    const entry = pm.getPlugin("dyn-invoke");
    const execute = vi.fn(async (input, ctx) => `${ctx.agentId}:${input.query}`);
    const remove = pm.addTool("dyn-invoke", {
      name: "search",
      description: "Dynamic search",
      sessionPermission: { readOnly: true },
      execute,
    }, { pluginKey: entry.pluginKey, source: entry.source });

    const tool = pm.getPluginTool("dyn-invoke", "search");
    const result = await pm.executePluginTool(tool, {
      toolCallId: "call-dynamic",
      input: { query: "notes" },
      runtimeCtx: { agentId: "agent-a" },
    });

    expect(execute).toHaveBeenCalledWith(
      { query: "notes" },
      { agentId: "agent-a" },
    );
    expect(result.content[0].text).toBe("agent-a:notes");
    remove();
  });

  it("keeps legacy Pi-signature dynamic tools callable through the unified adapter", async () => {
    const dir = path.join(pluginsDir, "mcp-bridge");
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, "manifest.json"), JSON.stringify({
      id: "mcp-bridge",
      name: "MCP Bridge",
      version: "1.0.0",
    }));
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();
    const entry = pm.getPlugin("mcp-bridge");
    const execute = vi.fn(async (_toolCallId, params, runtimeCtx) => (
      `${runtimeCtx.agentId}:${params.query}`
    ));
    const remove = pm.addTool("mcp-bridge", {
      name: "github_search",
      description: "Legacy MCP search",
      invocationStyle: "pi_tool",
      sessionPermission: { readOnly: true },
      execute,
    }, { pluginKey: entry.pluginKey, source: entry.source });

    const tool = pm.getPluginTool("mcp-bridge", "github_search");
    const result = await pm.executePluginTool(tool, {
      toolCallId: "call-pi",
      input: { query: "issues" },
      runtimeCtx: { agentId: "agent-a" },
    });

    expect(execute).toHaveBeenCalledWith(
      "call-pi",
      { query: "issues" },
      { agentId: "agent-a" },
    );
    expect(result.content[0].text).toBe("agent-a:issues");
    remove();
  });

  it("passes Pi SDK fifth-argument ctx to full Pi-signature dynamic tools", async () => {
    const dir = path.join(pluginsDir, "dynamic-pi-context");
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, "manifest.json"), JSON.stringify({
      id: "dynamic-pi-context",
      name: "Dynamic Pi Context",
      version: "1.0.0",
    }));
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();
    const entry = pm.getPlugin("dynamic-pi-context");
    const execute = vi.fn(async (_toolCallId, _params, _signal, _onUpdate, ctx) => (
      ctx.sessionPath || ""
    ));
    const remove = pm.addTool("dynamic-pi-context", {
      name: "session_scope",
      description: "Full Pi signature session scope",
      invocationStyle: "pi_tool",
      sessionPermission: { readOnly: true },
      execute,
    }, { pluginKey: entry.pluginKey, source: entry.source });

    const tool = pm.getPluginTool("dynamic-pi-context", "session_scope");
    const signal = new AbortController().signal;
    const onUpdate = vi.fn();
    const result = await tool.execute("call-pi", {}, signal, onUpdate, {
      sessionManager: { getSessionFile: () => "/sessions/dynamic-pi.jsonl" },
    });

    expect(execute).toHaveBeenCalledWith(
      "call-pi",
      {},
      signal,
      onUpdate,
      expect.objectContaining({
        sessionPath: "/sessions/dynamic-pi.jsonl",
      }),
    );
    expect(result.content[0].text).toBe("/sessions/dynamic-pi.jsonl");
    remove();
  });

  it("plugin can register tools via ctx.registerTool in onload", async () => {
    const dir = path.join(pluginsDir, "dyn-plug");
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, "index.js"), `
      export default class DynPlugin {
        async onload() {
          this.register(this.ctx.registerTool({
            name: "dynamic-tool",
            description: "Registered at runtime",
            sessionPermission: { readOnly: true },
            execute: async (input) => "dynamic " + input.x,
          }));
        }
      }
    `);
    const pm = new PluginManager({ pluginsDir, dataDir, bus: await makeBus() } as any);
    pm.scan();
    await pm.loadAll();

    const tools = pm.getAllTools();
    expect(tools.some(t => t.name === "dyn-plug_dynamic-tool")).toBe(true);

    // unload should clean up
    await pm.unloadPlugin("dyn-plug");
    expect(pm.getAllTools().some(t => t.name === "dyn-plug.dynamic-tool")).toBe(false);
  });
});

// ── Hot operations ──────────────────────────────────────────────────────────

function createMockPrefs( overrides: any = {}) {
  return {
    _data: {
      allow_full_access_plugins: false,
      disabled_plugins: [],
      ...overrides,
    },
    getAllowFullAccessPlugins() { return this._data.allow_full_access_plugins; },
    setAllowFullAccessPlugins(v) { this._data.allow_full_access_plugins = v; },
    getDisabledPlugins() { return this._data.disabled_plugins; },
    setDisabledPlugins(list) { this._data.disabled_plugins = list; },
  };
}

function writeToolRoutePlugin(root: any, id: any, { text, sourceName = text }: any = {}) {
  const dir = path.join(root, id);
  fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
  fs.mkdirSync(path.join(dir, "routes"), { recursive: true });
  fs.writeFileSync(path.join(dir, "manifest.json"), JSON.stringify({
    id,
    name: `${sourceName} Plugin`,
    version: "1.0.0",
    trust: "full-access",
  }));
  writePluginToolFixture(path.join(dir, "tools", "echo.js"), `
    export const name = "echo";
    export const description = "Echo source";
    export const parameters = {};
    export async function execute() { return ${JSON.stringify(text)}; }
  `);
  fs.writeFileSync(path.join(dir, "routes", "api.js"), `
    export function register(app) { app.get("/who", (c) => c.text(${JSON.stringify(text)})); }
  `);
  return dir;
}

function writeConfigPlugin(root, id, version = "1.0.0") {
  const dir = path.join(root, id);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, "manifest.json"), JSON.stringify({
    id,
    name: "Config Shadow",
    version,
    contributes: {
      configuration: {
        properties: {
          mode: { type: "string", default: "unset" },
        },
      },
    },
  }));
  return dir;
}

describe("plugin lifecycle unload", () => {
  it("does not produce model Reminder ledger entries for plugin lifecycle changes", async () => {
    const dir = path.join(pluginsDir, "ledger-lifecycle");
    fs.mkdirSync(path.join(dir, "tools"), { recursive: true });
    writePluginToolFixture(path.join(dir, "tools", "echo.js"), "export const name = 'echo';");
    const append = vi.fn();
    const pm = new PluginManager({
      pluginsDir,
      dataDir,
      bus: await makeBus(),
      envChangeLedger: { append },
    } as any);

    pm.scan();
    await pm.loadAll();
    await pm.unloadPlugin("ledger-lifecycle");

    expect(append).not.toHaveBeenCalled();
  });
});
