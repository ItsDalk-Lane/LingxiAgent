#!/usr/bin/env node
/**
 * 机器校验：post-verification diff guard（独立可执行，供 CI / 人工单独运行）。
 *
 * 读取 .sync-audit/verified-source-sha.txt，执行：
 *   git diff --name-only VERIFIED_SOURCE_SHA..HEAD
 * 并断言所有改动仅落在审计 allowlist 内。出现任何生产代码、测试逻辑、
 * 构建 runtime logic 或 runtime generated artifact，退出非零（exit 1）。
 *
 * 用法：
 *   node .sync-audit/verify-post-verification-diff.mjs
 *
 * 与 tests/post-verification-audit-seal.test.ts 为同一门禁的两个形态：
 * 一个纳入 vitest 套件，一个可单独跑（不受测试基建影响）。
 */
import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const VERIFIED_SOURCE_SHA_FILE = path.join(ROOT, ".sync-audit", "verified-source-sha.txt");

// 审计文件 allowlist：新增/改名审计脚本时须同步维护。
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

function fail(msg) {
  console.error(`✗ ${msg}`);
  process.exit(1);
}

if (!fs.existsSync(VERIFIED_SOURCE_SHA_FILE)) {
  fail("缺少 .sync-audit/verified-source-sha.txt");
}

const sha = fs.readFileSync(VERIFIED_SOURCE_SHA_FILE, "utf-8").trim();
if (!/^[0-9a-f]{40}$/.test(sha)) {
  fail(`VERIFIED_SOURCE_SHA 非 40 位十六进制: ${sha}`);
}

let changed;
try {
  const out = execSync(`git diff --name-only ${sha}..HEAD`, { cwd: ROOT, encoding: "utf-8" });
  changed = out.split("\n").map((s) => s.trim()).filter(Boolean);
} catch (err) {
  fail(`git diff ${sha}..HEAD 失败: ${err.message}`);
}

const allowlisted = new Set(AUDIT_ALLOWLIST);
const violators = changed.filter((f) => !allowlisted.has(f));
if (violators.length > 0) {
  fail(
    `VERIFIED_SOURCE_SHA 之后出现非审计文件改动（禁止修改生产代码/测试逻辑/runtime artifacts）:\n` +
      violators.map((f) => `  - ${f}`).join("\n"),
  );
}

console.log(`✓ post-verification diff guard OK: ${sha}..HEAD 仅 ${changed.length} 个审计文件变化`);
