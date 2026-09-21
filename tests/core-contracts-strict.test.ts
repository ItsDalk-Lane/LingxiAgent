/**
 * P01-T04 strict 核心契约门禁的自测与负例验证（场景 P01-A05 / A06）。
 *
 * 证明三件事：
 *   1. 真实核心契约文件在 strict + noUncheckedIndexedAccess +
 *      exactOptionalPropertyTypes 下零诊断（正例 = 删除负例后的通过状态）。
 *   2. 注入"访问可能为 null 的值"负例（A05，tests/fixtures/
 *      core-contracts-negative/a05-null-negative.ts）使检查失败，错误码为
 *      空值检查类（TS2531 等）——不存在 any 吞掉错误。
 *   3. 注入"把 mt_ trace 当 mc_ call 使用"负例（A06，同目录
 *      a06-identity-misuse.ts）被品牌类型阻止（TS2322/TS2345 且指向
 *      ModelCallId）——负例通过 extraFiles 真实加入程序，不是未引用文件。
 *
 * 负例 fixture 被 tsconfig.test.json 显式排除（exclude "tests/fixtures/**"），
 * 永不进入常规工程；只能经本测试显式注入 strict 门禁。
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { runCoreContractsTypeCheck } from "../scripts/check-core-contracts-strict.mjs";

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const negativeFixtureDir = path.join(repositoryRoot, "tests", "fixtures", "core-contracts-negative");

describe("core contracts strict gate (P01-T04)", () => {
  it("real core contract files pass strict with noUncheckedIndexedAccess and exactOptionalPropertyTypes", () => {
    const report = runCoreContractsTypeCheck();

    expect(report.checkedFiles).toEqual(expect.arrayContaining([
      "shared/identity-brands.ts",
      "shared/hana-runtime-paths.ts",
      "shared/model-ref.ts",
      "shared/errors.ts",
      "lib/llm/model-call-identity.ts",
      "cli/local-server.ts",
    ]));
    expect(report.diagnostics).toEqual([]);
  });

  it("A05: injecting the null-unsafe negative fixture fails the check with a null-check error code", () => {
    const report = runCoreContractsTypeCheck({
      extraFiles: [path.join(negativeFixtureDir, "a05-null-negative.ts")],
    });

    const nullErrors = report.diagnostics.filter(
      (d) => d.file.endsWith("a05-null-negative.ts") && (d.code === "TS2531" || d.code === "TS18047" || d.code === "TS2349"),
    );
    expect(nullErrors.length).toBeGreaterThanOrEqual(1);
    // 负例自身之外不允许有新错误（真实代码仍然干净）。
    expect(report.diagnostics.filter((d) => !d.file.endsWith("a05-null-negative.ts"))).toEqual([]);
  });

  it("A06: injecting the identity-misuse negative fixture is blocked by the brand type", () => {
    const report = runCoreContractsTypeCheck({
      extraFiles: [path.join(negativeFixtureDir, "a06-identity-misuse.ts")],
    });

    const brandErrors = report.diagnostics.filter(
      (d) => d.file.endsWith("a06-identity-misuse.ts") && (d.code === "TS2322" || d.code === "TS2345"),
    );
    expect(brandErrors.length).toBeGreaterThanOrEqual(1);
    expect(brandErrors[0].message).toContain("ModelCallId");
    expect(report.diagnostics.filter((d) => !d.file.endsWith("a06-identity-misuse.ts"))).toEqual([]);
  });

  it("negative fixtures stay out of the regular test project (tsconfig.test.json excludes them)", () => {
    const config = JSON.parse(fs.readFileSync(path.join(repositoryRoot, "tsconfig.test.json"), "utf8"));
    expect(config.exclude).toContain("tests/fixtures/**");
  });

  it("a non-existent extraFile fails loudly instead of being silently skipped (acceptance fix)", () => {
    // 验收反例：extraFiles 指向不存在的文件时 createProgram 会静默跳过，
    // "注入负例必须失败"的测试会假绿。修复后必须显式抛错。
    expect(() => runCoreContractsTypeCheck({
      extraFiles: [path.join(negativeFixtureDir, "does-not-exist.ts")],
    })).toThrow(/does not exist/);
  });
});
