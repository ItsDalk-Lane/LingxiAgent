import { describe, it, expect } from "vitest";
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const ROOT = path.resolve(__dirname, "..");

/**
 * 上游版本一致性契约。
 *
 * 关于页「上游版本」字段的真相源是 package.json 的 lingxi.upstreamVersion，经
 * vite.config.ts 的 define 在构建期注入到 AboutTab。这样同步上游代码时只需改
 * package.json 一处，关于页自动跟随。
 *
 * 这组测试钉死整条链路：任何一环被改坏（package.json 漏字段、vite define 指错、
 * AboutTab 退回硬编码），CI 立刻红，杜绝"同步了上游代码却忘了更新关于页版本号"。
 */
describe("upstream version consistency", () => {
  const pkg = JSON.parse(fs.readFileSync(path.join(ROOT, "package.json"), "utf-8"));
  const viteConfig = fs.readFileSync(path.join(ROOT, "vite.config.ts"), "utf-8");
  const aboutTab = fs.readFileSync(
    path.join(ROOT, "desktop/src/react/settings/tabs/AboutTab.tsx"),
    "utf-8",
  );

  it("package.json declares lingxi.upstreamVersion as a semver-like string", () => {
    const v = pkg?.lingxi?.upstreamVersion;
    expect(typeof v).toBe("string");
    expect(v).toMatch(/^\d+\.\d+\.\d+/);
  });

  it("vite.config.ts injects lingxi.upstreamVersion into the renderer via define", () => {
    // define 必须读取 package.json 的 lingxi.upstreamVersion（而非字面量），否则
    // 改 package.json 不会生效——那就失去了单一真相源的意义。
    expect(viteConfig).toContain("pkg.lingxi?.upstreamVersion");
    expect(viteConfig).toContain("LINGXI_UPSTREAM_VERSION");
  });

  it("AboutTab reads the injected value, not a hardcoded version literal", () => {
    // 必须从 import.meta.env 读，不能是硬编码的版本号字面量——否则又会回到
    // "改了 package.json 但关于页不更新"的老问题。
    expect(aboutTab).toContain("import.meta.env.LINGXI_UPSTREAM_VERSION");
    // 反向钉子：源码里不得出现旧的硬编码上游版本号（任何引号包裹的纯版本字面量）。
    // 只匹配看起来像上游版本号的赋值（UPSTREAM_VERSION = 'x.y.z'），正常注释不算。
    const hardcoded = aboutTab.match(/UPSTREAM_VERSION\s*=\s*['"]\d+\.\d+\.\d+['"]/);
    expect(hardcoded).toBeNull();
  });

  it("AboutTab renders the upstream version to the user (the field is wired through)", () => {
    // 确保展示位确实用了 UPSTREAM_VERSION 这个名字，没有半路换成别的常量。
    expect(aboutTab).toMatch(/v\{UPSTREAM_VERSION\}/);
  });
});
