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
      externalCredentialBoundary: {
        id: "dreamina-cli-login",
        kind: "external-cli",
      },
    });
    expect(registry.resolveChatProvider("jimeng-cli")).toMatchObject({
      providerId: "jimeng-cli",
      projection: "none",
    });
    expect(registry.getMediaModels("jimeng-cli", "image_generation")).toEqual([]);
    expect(registry.getMediaModels("jimeng-cli", "video_generation")).toEqual([]);
  });

  it("只给所属插件签发无秘密的外部凭证许可", () => {
    const registry = new ProviderRegistry("/tmp/hana-jimeng-provider-permit-test");
    registry.registerProviderContribution({
      ...provider,
      _pluginId: "jimeng-cli",
    });

    expect(registry.authorizeExternalCredentialUse("jimeng-cli", {
      boundaryId: "dreamina-cli-login",
      operation: "submit",
      pluginId: "jimeng-cli",
    })).toEqual({
      providerId: "jimeng-cli",
      boundaryId: "dreamina-cli-login",
      kind: "external-cli",
      operation: "submit",
      credentialSource: "external",
    });
    expect(() => registry.authorizeExternalCredentialUse("jimeng-cli", {
      boundaryId: "dreamina-cli-login",
      operation: "submit",
      pluginId: "other-plugin",
    })).toThrow(/cannot authorize/i);
    expect(() => registry.authorizeExternalCredentialUse("jimeng-cli", {
      boundaryId: "wrong-boundary",
      operation: "submit",
      pluginId: "jimeng-cli",
    })).toThrow(/boundary/i);
  });
});
