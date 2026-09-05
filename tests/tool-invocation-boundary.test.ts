import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  EXACT_BOUNDARY_ALLOWLISTS,
  scanToolInvocationBoundaries,
} from "../scripts/check-tool-invocation-boundaries.mjs";

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

describe("tool invocation static boundaries", () => {
  it("uses exact-file allowlists for every raw execution boundary", () => {
    expect(EXACT_BOUNDARY_ALLOWLISTS).toEqual({
      mcpCallTool: [
        "core/mcp/clients/http-client.ts",
        "core/mcp/manager.ts",
      ],
      pluginExecuteTool: ["core/plugin-dev-service.ts"],
      canonicalTargetExecutor: ["core/tool-invocation-gateway.ts"],
    });
  });

  it("finds no raw executor or deferred-object-map bypass in production source", () => {
    const report = scanToolInvocationBoundaries({ rootDir: repositoryRoot });

    expect(report.scannedFiles).toBeGreaterThan(0);
    expect(report.violations).toEqual([]);
  });

  it("reports every protected boundary when a synthetic source bypasses it", () => {
    const fixtureRoot = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-tool-boundary-"));
    try {
      fs.mkdirSync(path.join(fixtureRoot, "core"), { recursive: true });
      fs.writeFileSync(
        path.join(fixtureRoot, "core", "rogue.ts"),
        "mcp.callTool('search', {}); pluginManager.executePluginTool(tool); target.executeCanonical();\n",
      );
      fs.writeFileSync(
        path.join(fixtureRoot, "core", "engine.ts"),
        "const builtinToolsByName = new Map();\n",
      );
      fs.writeFileSync(
        path.join(fixtureRoot, "core", "tool-catalog-bridge.ts"),
        "builtinCall();\n",
      );

      const report = scanToolInvocationBoundaries({ rootDir: fixtureRoot });

      expect(report.violations.map((violation) => violation.rule).sort()).toEqual([
        "bridge-raw-adapter",
        "canonical-executor-bypass",
        "engine-deferred-raw-map",
        "mcp-raw-execution",
        "plugin-raw-execution",
      ]);
    } finally {
      fs.rmSync(fixtureRoot, { recursive: true, force: true });
    }
  });
});
