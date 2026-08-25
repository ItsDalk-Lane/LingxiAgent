import { describe, expect, it } from "vitest";

import { assertRunnerPlatform } from "../scripts/assert-runner-platform.mjs";

describe("runner 平台与架构硬校验", () => {
  it("接受当前真实宿主", () => {
    expect(assertRunnerPlatform({
      expectedPlatform: process.platform,
      expectedArch: process.arch,
      actualPlatform: process.platform,
      actualArch: process.arch,
    })).toEqual({ platform: process.platform, arch: process.arch });
  });

  it("拒绝矩阵标签与真实平台或架构不一致", () => {
    expect(() => assertRunnerPlatform({
      expectedPlatform: "linux",
      expectedArch: "x64",
      actualPlatform: "darwin",
      actualArch: "arm64",
    })).toThrow(/runner mismatch/);
    expect(() => assertRunnerPlatform({
      expectedPlatform: "darwin",
      expectedArch: "x64",
      actualPlatform: "darwin",
      actualArch: "arm64",
    })).toThrow(/runner mismatch/);
  });

  it("拒绝未声明的平台组合", () => {
    expect(() => assertRunnerPlatform({
      expectedPlatform: "freebsd",
      expectedArch: "x64",
      actualPlatform: "freebsd",
      actualArch: "x64",
    })).toThrow(/unsupported expected runner/);
  });
});
