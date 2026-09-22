/**
 * P01-T06 依赖纪律检查器自测（场景 P01-A03 / A04 / A12）。
 *
 * A03 导入旁路：fixture 直接/别名导入 SDK → 检查失败；合法 adapter 例外通过。
 * A04 核心反向依赖：fixture 导入 Electron/前端 store → 失败且指出文件。
 * A12 检查器失效保护：合法、非法、动态无法静态解析三个样本——非法必失败；
 *      计算式动态 import 提及 SDK 名不得自动通过（保守判违例）。
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  DEPENDENCY_BOUNDARY_ALLOWLISTS,
  scanDependencyBoundaries,
} from "../scripts/check-dependency-boundaries.mjs";

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function withFixtureRoot(build: (root: string) => void, run: (root: string) => ReturnType<typeof scanDependencyBoundaries>) {
  const fixtureRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hana-dep-boundary-"));
  try {
    build(fixtureRoot);
    return run(fixtureRoot);
  } finally {
    fs.rmSync(fixtureRoot, { recursive: true, force: true });
  }
}

describe("dependency boundary static gate (P01-T06)", () => {
  it("production source is clean across all scan roots", () => {
    const report = scanDependencyBoundaries({ rootDir: repositoryRoot });

    expect(report.scannedFiles).toBeGreaterThan(2000);
    expect(report.violations).toEqual([]);
  });

  it("A03: direct SDK import outside the adapter fails; the legal adapter sample passes", () => {
    const report = withFixtureRoot((root) => {
      fs.mkdirSync(path.join(root, "core"), { recursive: true });
      fs.mkdirSync(path.join(root, "lib", "pi-sdk"), { recursive: true });
      fs.writeFileSync(
        path.join(root, "core", "rogue-direct.ts"),
        'import { getModel } from "@earendil-works/pi-ai/compat";\n',
      );
      fs.writeFileSync(
        path.join(root, "core", "rogue-require.cjs"),
        'const sdk = require("@earendil-works/pi-coding-agent");\n',
      );
      fs.writeFileSync(
        path.join(root, "core", "rogue-dynamic.ts"),
        'export const load = () => import("@earendil-works/pi-agent-core");\n',
      );
      fs.writeFileSync(
        path.join(root, "core", "rogue-typebox.ts"),
        'import { Type } from "typebox";\n',
      );
      fs.writeFileSync(
        path.join(root, "core", "rogue-deep.ts"),
        'import { X } from "../../node_modules/@earendil-works/pi-coding-agent/dist/core/compaction/compaction.js";\n',
      );
      // 合法适配器样本：lib/pi-sdk 内直接导入 SDK 不判违例。
      fs.writeFileSync(
        path.join(root, "lib", "pi-sdk", "legit-adapter.ts"),
        'import { createAgentSession } from "@earendil-works/pi-coding-agent";\nexport { createAgentSession };\n',
      );
    }, (root) => scanDependencyBoundaries({ rootDir: root }));

    const rules = report.violations.map((v) => v.rule).sort();
    expect(rules).toEqual([
      "sdk-deep-path",
      "sdk-direct-import",
      "sdk-direct-import",
      "sdk-direct-import",
      "sdk-direct-import",
    ]);
    expect(report.violations.every((v) => v.file.startsWith("core/rogue-"))).toBe(true);
    // 合法适配器样本未被点名。
    expect(report.violations.some((v) => v.file.startsWith("lib/pi-sdk/"))).toBe(false);
  });

  it("A04: electron import inside core fails and the report names the importing file", () => {
    const report = withFixtureRoot((root) => {
      fs.mkdirSync(path.join(root, "core", "nested"), { recursive: true });
      fs.mkdirSync(path.join(root, "desktop"), { recursive: true });
      fs.writeFileSync(
        path.join(root, "core", "nested", "host-leak.ts"),
        'import { app } from "electron";\n',
      );
      // desktop 层导入 electron 是合法宿主行为。
      fs.writeFileSync(
        path.join(root, "desktop", "main.cjs"),
        'const { app } = require("electron");\n',
      );
    }, (root) => scanDependencyBoundaries({ rootDir: root }));

    expect(report.violations).toHaveLength(1);
    expect(report.violations[0]).toMatchObject({
      file: "core/nested/host-leak.ts",
      rule: "host-into-core",
    });
  });

  it("A12: computed dynamic import that mentions an SDK name is not auto-passed; the legal adapter's dynamic loads stay clean", () => {
    const report = withFixtureRoot((root) => {
      fs.mkdirSync(path.join(root, "core"), { recursive: true });
      fs.mkdirSync(path.join(root, "lib", "pi-sdk"), { recursive: true });
      fs.writeFileSync(
        path.join(root, "core", "alias-loader.ts"),
        [
          'const pkg = "@earendil-works/pi-ai";',
          'export const load = () => import(pkg);',
        ].join("\n"),
      );
      // 计算式动态 import 但不提及 SDK 名（通用 loader）——静态不判。
      fs.writeFileSync(
        path.join(root, "core", "generic-loader.ts"),
        [
          'export async function freshImport(p: string) { return import(p); }',
        ].join("\n"),
      );
      // 适配层内的计算式动态加载属其职责，不判。
      fs.writeFileSync(
        path.join(root, "lib", "pi-sdk", "loader.ts"),
        'export const dyn = (p: string) => import(p);\n',
      );
    }, (root) => scanDependencyBoundaries({ rootDir: root }));

    expect(report.violations).toHaveLength(1);
    expect(report.violations[0]).toMatchObject({
      file: "core/alias-loader.ts",
      rule: "sdk-dynamic-unproven",
    });
  });

  // ── 验收加固：拼接构造的包名/electron 不再自动放行（常量折叠 + 片段提及）──
  it("constant-folded concatenation of SDK or electron names with computed dynamic import is flagged", () => {
    const report = withFixtureRoot((root) => {
      fs.mkdirSync(path.join(root, "core"), { recursive: true });
      // SDK scope + 包名分两段拼接，文件内无完整包名字面量。
      fs.writeFileSync(
        path.join(root, "core", "sdk-concat.ts"),
        [
          'const pkg = "@earendil-works" + "/pi-ai";',
          'export const load = () => import(pkg);',
        ].join("\n"),
      );
      // electron 拼接构造 + 计算式动态 import（host 渗入变体）。
      fs.writeFileSync(
        path.join(root, "core", "electron-concat.ts"),
        [
          'const m = "elect" + "ron";',
          'export const load = () => import(m);',
        ].join("\n"),
      );
      // 验收修复轮（2026-09-22）：全字面量 substitution 的模板表达式与二元
      // "+" 拼接静态等价，同受折叠约束（此前 `` `elect${''}ron` `` 全放行）。
      fs.writeFileSync(
        path.join(root, "core", "electron-template.ts"),
        [
          'export const load = () => import(`elect${""}ron`);',
        ].join("\n"),
      );
      fs.writeFileSync(
        path.join(root, "core", "sdk-template.ts"),
        [
          'const pkg = `@earendil-works${"/pi-ai"}`;',
          'export const load = () => import(pkg);',
        ].join("\n"),
      );
      // 含变量 substitution 的模板 = 运行时拼接，维持放行（固有静态边界）。
      fs.writeFileSync(
        path.join(root, "core", "runtime-template.ts"),
        [
          'export const load = (s: string) => import(`elect${s}ron`);',
        ].join("\n"),
      );
      // 真计算式（运行时拼接）+ 文件内零敏感名 → 维持放行（固有静态边界）。
      fs.writeFileSync(
        path.join(root, "core", "runtime-loader.ts"),
        [
          'export const load = (suffix: string) => import("./mods/" + suffix);',
        ].join("\n"),
      );
    }, (root) => scanDependencyBoundaries({ rootDir: root }));

    const flagged = report.violations.map((v) => `${v.file}:${v.rule}`).sort();
    expect(flagged).toEqual([
      "core/electron-concat.ts:host-dynamic-unproven",
      "core/electron-template.ts:host-into-core",
      "core/sdk-concat.ts:sdk-dynamic-unproven",
      "core/sdk-template.ts:sdk-dynamic-unproven",
    ]);
  });

  it("adapter reverse dependency into core/ or server/ fails", () => {
    const report = withFixtureRoot((root) => {
      fs.mkdirSync(path.join(root, "lib", "pi-sdk"), { recursive: true });
      fs.writeFileSync(
        path.join(root, "lib", "pi-sdk", "rogue-upward.ts"),
        'import { engine } from "../../core/engine.ts";\n',
      );
    }, (root) => scanDependencyBoundaries({ rootDir: root }));

    expect(report.violations).toHaveLength(1);
    expect(report.violations[0]).toMatchObject({
      file: "lib/pi-sdk/rogue-upward.ts",
      rule: "adapter-reverse-dep",
    });
  });

  it("keeps the typebox/value exception exact and single-file", () => {
    expect(DEPENDENCY_BOUNDARY_ALLOWLISTS.sdkDirectImport).toContain("lib/tools/invocation/schema-validator.ts");
    expect(DEPENDENCY_BOUNDARY_ALLOWLISTS.sdkDirectImport.filter((p) => p.endsWith("schema-validator.ts"))).toHaveLength(1);
  });
});
