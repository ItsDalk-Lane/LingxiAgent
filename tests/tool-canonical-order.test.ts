import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LingxiEngine } from "../core/engine.ts";

// Provider-visible tool schemas ride the cacheable request prefix. The build
// output assembles several independently ordered segments (agent snapshot,
// plugin, MCP, bridge), so buildTools must hand back a canonical order —
// otherwise a session resumed after a restart could present the same tool set
// in a different byte order and cold-start the provider prefix cache.
// Code-unit order (not localeCompare) keeps the hash machine-independent.

function permissionTool(name) {
  return {
    name,
    sessionPermission: {
      resolveInvocation: () => ({
        action: "execute",
        kind: "routine",
        capability: `${name}.execute`,
      }),
    },
    execute: vi.fn(),
  };
}

function buildFixtureEngine(tmpDir, agentDir) {
  const agent = {
    id: "focus",
    agentDir,
    config: {},
    tools: [],
  };
  const engine = Object.create(LingxiEngine.prototype);
  engine.lingxiHome = tmpDir;
  engine.getAgent = vi.fn(() => agent);
  engine.isChannelsEnabled = vi.fn(() => false);
  engine._pluginManager = null;
  engine._prefs = {
    getFileBackup: () => ({ enabled: false }),
    getBuiltinToolDeferEnabled: () => false,
  };
  engine._readPreferences = () => ({ sandbox: true });
  engine._confirmStore = null;
  engine._emitEvent = vi.fn();
  engine.getSessionPermissionMode = vi.fn(() => "operate");
  engine._agentMgr = { agent };
  return engine;
}

describe("buildTools canonical provider tool order", () => {
  let tmpDir;

  afterEach(() => {
    if (tmpDir) fs.rmSync(tmpDir, { recursive: true, force: true });
    tmpDir = null;
  });

  it("returns custom tools sorted by name regardless of registration order", () => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-tool-order-"));
    const agentDir = path.join(tmpDir, "agents", "focus");
    const engine = buildFixtureEngine(tmpDir, agentDir);

    const { customTools } = engine.buildTools(tmpDir, [
      permissionTool("web_fetch"),
      permissionTool("todo_write"),
      permissionTool("notify"),
    ], {
      agentDir,
      workspace: tmpDir,
      getPermissionMode: () => "operate",
    });

    expect(customTools.map((tool) => tool.name)).toEqual([
      "notify",
      "todo_write",
      "web_fetch",
    ]);
  });

  it("produces the same order from different registration orders", () => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-tool-order-"));
    const agentDir = path.join(tmpDir, "agents", "focus");
    const names = ["knowledge_search", "notify", "subagent", "stage_files"];

    const orders = [
      [...names],
      [...names].reverse(),
      ["stage_files", "knowledge_search", "subagent", "notify"],
    ];
    const seen = orders.map((registration) => {
      const engine = buildFixtureEngine(tmpDir, agentDir);
      const { customTools } = engine.buildTools(tmpDir, registration.map(permissionTool), {
        agentDir,
        workspace: tmpDir,
        getPermissionMode: () => "operate",
      });
      return customTools.map((tool) => tool.name);
    });

    expect(seen[0]).toEqual([...names].sort());
    expect(seen[1]).toEqual(seen[0]);
    expect(seen[2]).toEqual(seen[0]);
  });
});
