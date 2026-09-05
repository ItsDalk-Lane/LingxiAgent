import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { expect, it } from "vitest";
import { repairUseArchNative, singleArchitectureUseArchGyp } from "../scripts/prepare-usearch-native.mjs";

it("只去掉主扩展的双架构覆盖，让主扩展和计算库按同一目标构建", () => {
  const source = fs.readFileSync(path.resolve("node_modules/usearch/binding.gyp"), "utf8");
  const patched = singleArchitectureUseArchGyp(source);
  expect(patched).toBe(source.replace('"OTHER_CFLAGS": ["-arch arm64", "-arch x86_64"],', "")
    .replace('"OTHER_LDFLAGS": ["-arch arm64", "-arch x86_64"],', ""));
  expect(patched).toContain('"USEARCH_USE_NUMKONG=1"');
  expect(patched).toContain('"NK_DYNAMIC_DISPATCH=1"');
  expect(patched).toContain('"numkong_lib"');
  expect(() => singleArchitectureUseArchGyp(patched)).toThrow(/build contract changed/);
});

it("缺少可选扩展显式回退，但正式打包仍必须失败", () => {
  const rootDir = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-native-missing-"));
  const logs: string[] = [];
  try {
    expect(repairUseArchNative({ rootDir, platform: "darwin", arch: "x64", log: message => logs.push(message) }))
      .toEqual({ status: "unavailable" });
    expect(logs.join("\n")).toContain("portable fallback");
    expect(() => repairUseArchNative({ rootDir, platform: "darwin", arch: "x64", required: true }))
      .toThrow(/native extension unavailable/);
    expect(repairUseArchNative({ rootDir, platform: "linux", arch: "x64", required: true }))
      .toEqual({ status: "not-needed" });
  } finally { fs.rmSync(rootDir, { recursive: true, force: true }); }
});
