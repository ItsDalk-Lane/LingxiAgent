import path from "node:path";
import { describe, expect, it } from "vitest";

import {
  assertNativeTarget,
  buildRuntimeEnvironment,
  resolvePackagedArtifactDir,
} from "../scripts/smoke-packaged-knowledge.mjs";

describe("包内 Knowledge 烟测边界", () => {
  it("按发布目录规则解析四个平台的服务器种子", () => {
    expect(resolvePackagedArtifactDir({ rootDir: "/repo", platform: "darwin", arch: "arm64" }))
      .toBe(path.join("/repo", "dist-server-artifact", "mac-arm64"));
    expect(resolvePackagedArtifactDir({ rootDir: "/repo", platform: "darwin", arch: "x64" }))
      .toBe(path.join("/repo", "dist-server-artifact", "mac-x64"));
    expect(resolvePackagedArtifactDir({ rootDir: "/repo", platform: "win32", arch: "x64" }))
      .toBe(path.join("/repo", "dist-server-artifact", "win-x64"));
    expect(resolvePackagedArtifactDir({ rootDir: "/repo", platform: "linux", arch: "x64" }))
      .toBe(path.join("/repo", "dist-server-artifact", "linux-x64"));
  });

  it("拒绝用当前宿主冒充另一平台或架构执行包内烟测", () => {
    expect(() => assertNativeTarget({
      platform: "win32",
      arch: "x64",
      actualPlatform: "darwin",
      actualArch: "arm64",
    })).toThrow(/native target mismatch/);
  });

  it("启动包内服务器前剥离构建签名和发布凭证", () => {
    expect(buildRuntimeEnvironment({
      PATH: "/bin",
      LINGXI_SIGN_KEY: "/tmp/private.pem",
      LINGXI_SIGN_KEYSET: "/tmp/keyset.json",
      CSC_LINK: "certificate",
      APPLE_ID_PASSWORD: "password",
      GH_TOKEN: "token",
    })).toEqual({ PATH: "/bin" });
  });
});
