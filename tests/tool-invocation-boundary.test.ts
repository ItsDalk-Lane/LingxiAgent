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

  it("keeps the route-invariant architecture contract documented", () => {
    const documentPath = path.join(
      repositoryRoot,
      "docs",
      "architecture",
      "tool-invocation-path-invariance.md",
    );
    const document = fs.readFileSync(documentPath, "utf8");

    for (const requiredSection of [
      "## 不变量",
      "## 身份与名称",
      "## 生命周期时序",
      "## 统一调用路径",
      "## PreparedInvocation 绑定",
      "## Raw source adapter 边界",
      "## 错误码",
      "## 新工具接入清单",
      "## 禁止重新引入的反模式",
    ]) {
      expect(document).toContain(requiredSection);
    }
    for (const requiredFact of [
      "targetId",
      "capabilityBase",
      "direct",
      "deferred",
      "plugin-dev-chat",
      "plugin-dev-http",
      "TARGET_REVOKED",
      "scripts/check-tool-invocation-boundaries.mjs",
    ]) {
      expect(document).toContain(requiredFact);
    }
  });

  it("keeps repair reports and machine facts complete without self-referential SHAs", () => {
    const read = (filename: string) => fs.readFileSync(
      path.join(repositoryRoot, "docs", "archives", "tool-invocation", filename), "utf8",
    );
    const report = read("TOOL_INVOCATION_REPAIR_REPORT.md");
    const testReport = read("TOOL_INVOCATION_REPAIR_TEST_REPORT.md");
    const remaining = read("TOOL_INVOCATION_REPAIR_REMAINING.md");
    const facts = JSON.parse(read("TOOL_INVOCATION_REPAIR_FACTS.json"));

    expect(facts).toMatchObject({
      baselineSha: "60d910b84572c525a7c9c49216fb9206623bf7a4",
      branch: "fix/tool-contract-path-invariance-v0134",
      sourceCandidateSha: null,
      sealSha: null,
      commitCoordinatesRecordedIn: "PROGRESS.md and final execution report",
      rawExecutionBoundaryViolations: 0,
      findings: {
        V1: "fixed",
        V2: "fixed",
        V3: "fixed",
        V4a: "fixed",
        V4b: "fixed_by_generation_contract",
        V5a: "fixed",
        V5b: "fixed_with_local_developer_principal",
        V6: "fixed",
        V7: "fixed",
        V8: "fixed",
        V9: "fixed",
        V10: "fixed",
        V11: "fixed",
        V12: "fixed",
      },
    });
    expect(Array.isArray(facts.tests)).toBe(true);
    expect(Array.isArray(facts.builds)).toBe(true);
    for (const finding of ["V1", "V2", "V3", "V4a", "V4b", "V5a", "V5b", "V6", "V7", "V8", "V9", "V10", "V11", "V12"]) {
      expect(report).toContain(`| ${finding} |`);
    }
    expect(testReport).toContain("P12 最终验证");
    expect(testReport).toContain("/tmp/lingxi-tool-contract-");
    expect(remaining).toContain("源代码剩余事项：`none`");
  });
});
