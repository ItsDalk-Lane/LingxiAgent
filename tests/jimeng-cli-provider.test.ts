import { describe, expect, it } from "vitest";
import { ProviderRegistry } from "../core/provider-registry.ts";
import * as provider from "../plugins/jimeng-cli/providers/jimeng-cli.ts";

describe("Jimeng CLI provider contribution", () => {
  it("declares provider identity without a stale static media catalog", () => {
    const registry = new ProviderRegistry("/tmp/hana-jimeng-provider-test");
    registry.registerProviderContribution({
      ...provider,
      _pluginId: "jimeng-cli",
    });

    expect(registry.get("jimeng-cli")).toMatchObject({
      id: "jimeng-cli",
      displayName: "即梦 CLI",
      authType: "none",
      source: { kind: "plugin", pluginId: "jimeng-cli" },
    });
    expect(registry.resolveChatProvider("jimeng-cli")).toMatchObject({
      providerId: "jimeng-cli",
      projection: "none",
    });
    expect(registry.getMediaModels("jimeng-cli", "image_generation")).toEqual([]);
    expect(registry.getMediaModels("jimeng-cli", "video_generation")).toEqual([]);
  });
});
