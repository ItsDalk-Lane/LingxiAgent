import fs from "fs";
import os from "os";
import path from "path";
import YAML from "js-yaml";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { ProviderRegistry } from "../core/provider-registry.ts";

let tmpHome;

beforeEach(() => {
  tmpHome = fs.mkdtempSync(path.join(os.tmpdir(), "hana-media-bindings-"));
});

afterEach(() => {
  fs.rmSync(tmpHome, { recursive: true, force: true });
});

function bindings(registry: ProviderRegistry, providerId: string) {
  return registry.getMediaCapabilityBindings(providerId);
}

describe("ProviderRegistry media capability bindings", () => {
  it("maps agnes to both imageGeneration and videoGeneration with its own runtime id", () => {
    const registry = new ProviderRegistry(tmpHome);
    registry.reload();

    expect(bindings(registry, "agnes")).toEqual([
      { capability: "imageGeneration", runtimeProviderId: "agnes" },
      { capability: "videoGeneration", runtimeProviderId: "agnes" },
    ]);
  });

  it("maps minimax imageGeneration to itself without a credential lane", () => {
    const registry = new ProviderRegistry(tmpHome);
    registry.reload();

    expect(bindings(registry, "minimax")).toContainEqual({
      capability: "imageGeneration",
      runtimeProviderId: "minimax",
    });
    expect(bindings(registry, "minimax")).not.toContainEqual(
      expect.objectContaining({ capability: "videoGeneration" }),
    );
  });

  it("maps minimax-token-plan imageGeneration to runtime minimax through its credential lane", () => {
    const registry = new ProviderRegistry(tmpHome);
    registry.reload();

    expect(bindings(registry, "minimax-token-plan")).toEqual([
      {
        capability: "imageGeneration",
        runtimeProviderId: "minimax",
        credentialLaneId: "minimax-token-plan",
      },
    ]);
    // 不通过字符串裁剪猜测：minimax-token-plan 自己不是 runtime provider。
    expect(bindings(registry, "minimax-token-plan")).not.toContainEqual(
      expect.objectContaining({ runtimeProviderId: "minimax-token-plan" }),
    );
  });

  it("maps volcengine-speech speechRecognition to itself", () => {
    const registry = new ProviderRegistry(tmpHome);
    registry.reload();

    expect(bindings(registry, "volcengine-speech")).toEqual([
      { capability: "speechRecognition", runtimeProviderId: "volcengine-speech" },
    ]);
  });

  it("does not let volcengine inherit volcengine-speech by name proximity", () => {
    const registry = new ProviderRegistry(tmpHome);
    registry.reload();

    const volcengineCapabilities = bindings(registry, "volcengine").map((b) => b.capability);
    expect(volcengineCapabilities).toContain("imageGeneration");
    expect(volcengineCapabilities).not.toContain("speechRecognition");
    expect(bindings(registry, "volcengine")).not.toContainEqual(
      expect.objectContaining({ runtimeProviderId: "volcengine-speech" }),
    );
  });

  it("maps a custom media provider id exactly to itself", () => {
    fs.writeFileSync(path.join(tmpHome, "added-models.yaml"), YAML.dump({
      providers: {
        "my-proxy": {
          api_key: "proxy-key",
          base_url: "https://proxy.example.com/v1",
          api: "openai-completions",
          models: [{ id: "flux-1.1-pro", type: "image", name: "FLUX 1.1 Pro" }],
        },
      },
    }), "utf-8");

    const registry = new ProviderRegistry(tmpHome);
    registry.reload();

    expect(bindings(registry, "my-proxy")).toEqual([
      { capability: "imageGeneration", runtimeProviderId: "my-proxy" },
    ]);
  });

  it("returns [] for providers without any media capability", () => {
    const registry = new ProviderRegistry(tmpHome);
    registry.reload();

    expect(bindings(registry, "groq")).toEqual([]);
    expect(bindings(registry, "unknown-provider")).toEqual([]);
  });

  it("does not silently collapse a credential provider bound to multiple runtime providers", () => {
    const registry = new ProviderRegistry(tmpHome);
    registry.registerProviderContribution({
      id: "multi-lane",
      displayName: "Multi Lane",
      authType: "api-key",
      _pluginId: "multi-lane",
      capabilities: {
        chat: { projection: "none" },
        media: {
          imageGeneration: {
            credentialLanes: [
              { id: "lane-a", providerId: "lane-a", label: "Lane A" },
              { id: "lane-b", providerId: "lane-b", label: "Lane B" },
            ],
            models: [{ id: "img", protocolId: "openai-images" }],
          },
        },
      },
    });
    registry.reload();

    expect(bindings(registry, "lane-a")).toEqual([
      { capability: "imageGeneration", runtimeProviderId: "multi-lane", credentialLaneId: "lane-a" },
    ]);
    expect(bindings(registry, "lane-b")).toEqual([
      { capability: "imageGeneration", runtimeProviderId: "multi-lane", credentialLaneId: "lane-b" },
    ]);
  });
});
