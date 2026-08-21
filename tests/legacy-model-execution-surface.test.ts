import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const root = path.resolve(import.meta.dirname, "..");

describe("legacy model execution surfaces", () => {
  it("does not ship the retired synchronous ExecutionRouter", () => {
    expect(fs.existsSync(path.join(root, "core", "execution-router.ts"))).toBe(false);
    const manifest = JSON.parse(fs.readFileSync(path.join(root, "export-manifest.json"), "utf-8"));
    expect(manifest.paths).not.toContain("core/execution-router.ts");
  });

  it("does not inject unused model credential resolvers into Agent", () => {
    const agentSource = fs.readFileSync(path.join(root, "core", "agent.ts"), "utf-8");
    const managerSource = fs.readFileSync(path.join(root, "core", "agent-manager.ts"), "utf-8");

    expect(agentSource).not.toContain("_resolveModel");
    expect(agentSource).not.toContain("resolveModelFresh");
    expect(managerSource).not.toContain("resolveModelWithCredentials(bareId)");
    expect(managerSource).not.toContain("resolveModelWithCredentialsFresh(bareId)");
  });
});
