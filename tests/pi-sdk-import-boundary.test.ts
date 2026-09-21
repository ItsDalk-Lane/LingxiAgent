import fs from "fs";
import path from "path";
import { describe, expect, it } from "vitest";

const ROOT = path.resolve(import.meta.dirname, "..");
// P01-T02：扫描面与生产源码对齐——原 walk 只收 .js/.mjs/.cjs，而 core/server/lib/hub
// 的生产实现几乎全是 .ts（.ts 盲区此前仅由 postinstall 的 patch-pi-sdk.cjs 兜住）。
// 三个 pi 包 + typebox 的直接 import 在任何生产层（含 cli/shared/plugins/desktop）
// 都必须收敛到 lib/pi-sdk 适配面内。
const SCAN_DIRS = ["core", "server", "lib", "hub", "cli", "shared", "plugins", "desktop"];
const ADAPTER_DIR = path.join(ROOT, "lib", "pi-sdk");

function walk(dir, files = []) {
  if (!fs.existsSync(dir)) return files;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "node_modules") continue;
      walk(full, files);
    } else if (/\.(js|mjs|cjs|ts|tsx)$/.test(entry.name)) {
      files.push(full);
    }
  }
  return files;
}

function findDirectImports(modulePattern) {
  const leaks = [];
  for (const dir of SCAN_DIRS) {
    for (const file of walk(path.join(ROOT, dir))) {
      if (file.startsWith(ADAPTER_DIR)) continue;
      const content = fs.readFileSync(file, "utf8");
      if (modulePattern.test(content)) {
        leaks.push(path.relative(ROOT, file));
      }
    }
  }
  return leaks;
}

describe("Pi SDK import boundary", () => {
  it("keeps production Pi SDK imports inside lib/pi-sdk", () => {
    // pi-ai / pi-coding-agent / pi-agent-core 三个包一视同仁——任一直接 import
    // 都必须只出现在 lib/pi-sdk 边界内（与 scripts/patch-pi-sdk.cjs 的 verifier
    // regex 保持同形）。此前 regex 漏列 pi-agent-core；当前虽无生产泄漏，但补齐
    // 以闭合静态覆盖。
    const pattern = /(?:from\s+["']@(?:mariozechner|earendil-works)\/(?:pi-ai|pi-coding-agent|pi-agent-core)|import\s*\(\s*["']@(?:mariozechner|earendil-works)\/(?:pi-ai|pi-coding-agent|pi-agent-core)|require\s*\(\s*["']@(?:mariozechner|earendil-works)\/(?:pi-ai|pi-coding-agent|pi-agent-core)|^[\t ]*import[\t ]+["']@(?:mariozechner|earendil-works)\/(?:pi-ai|pi-coding-agent|pi-agent-core))/m;
    expect(findDirectImports(pattern)).toEqual([]);
  });

  it("the import-boundary pattern statically covers pi-agent-core", () => {
    // 回归：防止 regex 再次遗漏 pi-agent-core（任一 import 形式都应命中）。
    const pattern = /(?:from\s+["']@(?:mariozechner|earendil-works)\/(?:pi-ai|pi-coding-agent|pi-agent-core)|import\s*\(\s*["']@(?:mariozechner|earendil-works)\/(?:pi-ai|pi-coding-agent|pi-agent-core)|require\s*\(\s*["']@(?:mariozechner|earendil-works)\/(?:pi-ai|pi-coding-agent|pi-agent-core)|^[\t ]*import[\t ]+["']@(?:mariozechner|earendil-works)\/(?:pi-ai|pi-coding-agent|pi-agent-core))/m;
    const samples = [
      'import { runAgentLoop } from "@earendil-works/pi-agent-core";',
      'import("@earendil-works/pi-agent-core")',
      'require("@earendil-works/pi-agent-core")',
      'import "@earendil-works/pi-agent-core/subpath";',
    ];
    for (const src of samples) {
      expect(pattern.test(src)).toBe(true);
    }
    // 注释/字符串里的引用不应被误判为真实 import（pattern 是语法锚定的）。
    expect(pattern.test('// see @earendil-works/pi-agent-core docs')).toBe(false);
    expect(pattern.test('// import "@earendil-works/pi-agent-core"')).toBe(false);
  });

  it("keeps production typebox imports inside lib/pi-sdk", () => {
    // P01-T02 扩面后发现：lib/tools/invocation/schema-validator.ts 消费
    // `typebox/value` 的 Value 校验器。typebox 是根级直接依赖（package.json），
    // 该文件是网关参数 schema 的运行时校验基础设施（tests/tool-schema-validator
    // 全覆盖），不是 Pi SDK 旁路——登记为唯一精确例外。schema 构造面（根模块
    // `typebox` 的 Type）仍必须经 lib/pi-sdk 适配面 re-export。
    const pattern = /(?:from\s+["']typebox["']|import\s*\(\s*["']typebox["']|require\s*\(\s*["']typebox["'])/;
    const typeboxValuePattern = /(?:from\s+["']typebox\/value["']|import\s*\(\s*["']typebox\/value["']|require\s*\(\s*["']typebox\/value["'])/;
    const rootSpecifiers = findDirectImports(pattern)
      .filter((file) => file !== "lib/tools/invocation/schema-validator.ts");
    expect(rootSpecifiers).toEqual([]);
    expect(findDirectImports(typeboxValuePattern))
      .toEqual(["lib/tools/invocation/schema-validator.ts"]);
  });
});
