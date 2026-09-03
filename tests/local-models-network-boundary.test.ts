import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const root = process.cwd();
const inferenceFiles = [
  "lib/local-models/runtime-service.ts",
  "lib/local-models/instance-manager.ts",
  "lib/local-models/in-process-factory.ts",
  "lib/local-models/sidecar-factory.ts",
  "lib/local-models/sidecar-manager.ts",
  "lib/local-models/composite-factory.ts",
];

describe("local model inference network boundary", () => {
  it("keeps inference orchestration free of network clients", () => {
    for (const relative of inferenceFiles) {
      const source = fs.readFileSync(path.join(root, relative), "utf8");
      expect(source, relative).not.toMatch(/from\s+["'](?:node:)?(?:http|https|net|tls|dgram|undici|ws)["']/);
      expect(source, relative).not.toMatch(/\b(?:fetch|WebSocket)\s*\(/);
    }
  });

  it("uses authenticated stdio instead of opening a listening port", () => {
    const source = fs.readFileSync(path.join(root, "lib/local-models/sidecar-manager.ts"), "utf8");
    expect(source).toContain('stdio: ["pipe", "pipe", "pipe"]');
    expect(source).toContain("LINGXI_LOCAL_MODEL_TOKEN");
    expect(source).not.toMatch(/\.listen\s*\(/);
  });
});
