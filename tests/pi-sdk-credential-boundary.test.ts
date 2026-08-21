import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AuthStorage, createModelRuntime } from "../lib/pi-sdk/index.ts";

const tempDirs: string[] = [];

afterEach(() => {
  vi.unstubAllEnvs();
  for (const dir of tempDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

function writeModelsJson(providers: Record<string, unknown>) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-sdk-credential-boundary-"));
  tempDirs.push(dir);
  const modelsPath = path.join(dir, "models.json");
  fs.writeFileSync(modelsPath, `${JSON.stringify({ providers }, null, 2)}\n`, "utf-8");
  return modelsPath;
}

describe("Pi SDK credential boundary", () => {
  it("rejects ambient provider credentials even when the provider is explicit in models.json", async () => {
    vi.stubEnv("OPENAI_API_KEY", "sk-test-ambient-must-not-run");
    const runtime = await createModelRuntime(
      AuthStorage.inMemory(),
      undefined,
      writeModelsJson({ openai: {} }),
    );
    const model = runtime.getModels("openai")[0];

    expect(model).toBeTruthy();
    await expect(runtime.getAuth(model)).rejects.toMatchObject({
      code: "LINGXI_AMBIENT_CREDENTIAL_FORBIDDEN",
      providerId: "openai",
    });
    expect((await runtime.getAvailable()).some(candidate => candidate.provider === "openai")).toBe(false);
  });

  it("keeps a request credential already resolved by the Lingxi boundary", async () => {
    vi.stubEnv("OPENAI_API_KEY", "sk-test-ambient-must-not-win");
    const runtime = await createModelRuntime(
      AuthStorage.inMemory(),
      undefined,
      writeModelsJson({ openai: {} }),
    );
    const model = runtime.getModels("openai")[0];

    const resolution = await runtime.getAuth(model, { apiKey: "sk-test-explicit-boundary" });

    expect(resolution?.auth.apiKey).toBe("sk-test-explicit-boundary");
    expect(resolution?.auth.apiKey).not.toBe("sk-test-ambient-must-not-win");
  });
});
