/**
 * Post-verification audit seal — diff guard（机器校验）。
 *
 * 审计模型：
 *
 *   VERIFIED_SOURCE_SHA（被最终 typecheck/lint/测试/构建/打包验证的源码树）
 *        ↓
 *   只允许 audit-only seal（纯审计材料变更）
 *        ↓
 *   当前 branch HEAD（由 Git ref 自身标识，不在 commit 内容中自引用）
 *
 * 本测试读取 .sync-audit/verified-source-sha.txt，执行
 *   git diff --name-only VERIFIED_SOURCE_SHA..HEAD
 * 并断言改动的文件全部落在审计 allowlist 内。若出现任何生产代码、测试逻辑、
 * 构建 runtime logic 或 runtime generated artifacts，则门禁失败（exit 1）。
 *
 * 约束：branch HEAD 可能比 VERIFIED_SOURCE_SHA 多出审计收口提交；但绝不允许
 * 在 VERIFIED_SOURCE_SHA 之后偷偷修改业务实现却继续声称验证有效。
 */
import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const VERIFIED_SOURCE_SHA_FILE = path.join(ROOT, ".sync-audit", "verified-source-sha.txt");

// 审计文件 allowlist：VERIFIED_SOURCE_SHA 之后只允许这些文件变化。
// 若为支持本 guard 本身新增/改名脚本，须同步加入此列表。
const AUDIT_ALLOWLIST = [
  "PROGRESS.md",
  "UPSTREAM_SYNC_AUDIT.md",
  "UPSTREAM_SYNC_MATRIX.md",
  ".sync-audit/upstream-sync-matrix.json",
  ".sync-audit/verified-source-sha.txt",
  ".sync-audit/build-sync-matrix.mjs",
  ".sync-audit/verify-post-verification-diff.mjs",
  "tests/upstream-sync-matrix.test.ts",
  "tests/post-verification-audit-seal.test.ts",
];

function verifiedSourceSha(): string {
  return fs.readFileSync(VERIFIED_SOURCE_SHA_FILE, "utf-8").trim();
}

function diffNamesSinceVerified(): string[] {
  const sha = verifiedSourceSha();
  const out = execSync(
    `git diff --name-only ${sha}..HEAD`,
    { cwd: ROOT, encoding: "utf-8" },
  );
  return out.split("\n").map((s) => s.trim()).filter(Boolean);
}

describe("post-verification audit seal (diff guard)", () => {
  it("VERIFIED_SOURCE_SHA is a valid 40-hex commit reachable in this repo", () => {
    const sha = verifiedSourceSha();
    expect(sha).toMatch(/^[0-9a-f]{40}$/);
    // 若坐标不可达，下方 git diff 直接失败；这里显式给出可读错误。
    expect(() => execSync(`git rev-parse --verify ${sha}^{commit}`, { cwd: ROOT }))
      .not.toThrow();
  });

  it("changes since VERIFIED_SOURCE_SHA are audit-only (allowlist enforced)", () => {
    const changed = diffNamesSinceVerified();
    const allowlisted = new Set(AUDIT_ALLOWLIST);
    const violators = changed.filter((f) => !allowlisted.has(f));
    expect(
      violators,
      `VERIFIED_SOURCE_SHA 之后出现了非审计文件改动（禁止修改生产代码/测试逻辑/runtime artifacts）:\n` +
        violators.join("\n"),
    ).toEqual([]);
  });

  it("the audit-seal test and generator are both present for the guard to be self-consistent", () => {
    const generator = path.join(ROOT, ".sync-audit", "verify-post-verification-diff.mjs");
    expect(fs.existsSync(generator), "verify-post-verification-diff.mjs 缺失").toBe(true);
    const thisTest = path.join(ROOT, "tests", "post-verification-audit-seal.test.ts");
    expect(fs.existsSync(thisTest)).toBe(true);
  });
});
