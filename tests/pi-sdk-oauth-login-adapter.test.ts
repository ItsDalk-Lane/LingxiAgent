import { describe, expect, it, vi } from "vitest";

import {
  AuthStorage,
  loginOAuthProvider,
  SdkAuthFacade,
  type OAuthLoginCallbacks,
} from "../lib/pi-sdk/index.ts";
// 0.83.0：OAuth 登录迁到 ModelRuntime.login；facade 需要一个 ModelRuntime。
import { ModelRuntime } from "@earendil-works/pi-coding-agent";

describe("Pi SDK OAuth login adapter", () => {
  it("satisfies the real 0.83.0 selector contract before browser I/O starts", async () => {
    const authStorage = AuthStorage.inMemory();
    // 组装 ModelRuntime（内置目录含 openai-codex）；allowModelNetwork:false 避免联网。
    const modelRuntime = await ModelRuntime.create({
      credentials: authStorage,
      allowModelNetwork: false,
    });
    const facade = new SdkAuthFacade({ authStorage, modelRuntime });
    const sentinel = new Error("__hana_stop_before_io__");
    const onAuth = vi.fn();
    const onDeviceCode = vi.fn();
    const callbacks: OAuthLoginCallbacks = {
      onAuth,
      onDeviceCode,
      onPrompt: async () => "",
      onSelect: async (prompt) => {
        // 0.83.0 openai-codex 先弹 select（browser / device_code），此时还没任何 I/O。
        expect(prompt.options.map(option => option.id)).toContain("browser");
        throw sentinel;
      },
      signal: new AbortController().signal,
    };

    await expect(loginOAuthProvider(facade, "openai-codex", callbacks))
      .rejects.toThrow("__hana_stop_before_io__");
    expect(onAuth).not.toHaveBeenCalled();
    expect(onDeviceCode).not.toHaveBeenCalled();
  });
});
