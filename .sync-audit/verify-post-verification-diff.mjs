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
// 与 tests/post-verification-audit-seal.test.ts 的 AUDIT_ALLOWLIST 为同一门禁的
// 两份副本，必须保持一致（2026-08-23 曾发散：本文件缺 V3 验收文档等条目，
// 导致 main 上独立门禁红而 vitest 形态绿）。
const AUDIT_ALLOWLIST = [
  "PROGRESS.md",
  // Phase 6 payload capture 跨会话进度活文档（对应 PROGRESS.md 角色）。
  "PAYLOAD_CAPTURE_PROGRESS.md",
  // Phase 7 durable storage 跨会话进度活文档（对应 PROGRESS.md 角色）。
  "OBSERVABILITY_STORAGE_PROGRESS.md",
  "UPSTREAM_SYNC_AUDIT.md",
  "UPSTREAM_SYNC_MATRIX.md",
  // 模型调用可观测性审计报告（e62bb535 合入的纯审计材料）。
  "MODEL_CALL_OBSERVABILITY_AUDIT.md",
  // Phase 10 E2E truth audit / release acceptance / 跨会话进度（纯审计材料）。
  "MODEL_OBSERVABILITY_E2E_TRUTH_AUDIT.md",
  "MODEL_OBSERVABILITY_RELEASE_ACCEPTANCE.md",
  "OBSERVABILITY_VALIDATION_PROGRESS.md",
  // Phase 10.1/11 同族审计材料：V2/V3 验收、对抗性台账、修复进度与各 Phase
  // 审计/进度文档（与上方既有条目同类，均为纯 markdown 审计材料）。
  "MODEL_OBSERVATORY_RELEASE_ACCEPTANCE_V2.md",
  "MODEL_OBSERVATORY_RELEASE_ACCEPTANCE_V3.md",
  "MODEL_OBSERVATORY_ADVERSARIAL_REVIEW.md",
  "OBSERVABILITY_REMEDIATION_PROGRESS.md",
  "MODEL_CALL_CLOSURE_DELTA.md",
  "MODEL_CALL_PAYLOAD_CAPTURE_AUDIT.md",
  "MODEL_OBSERVABILITY_QUERY_AUDIT.md",
  "MODEL_OBSERVABILITY_STORAGE_AUDIT.md",
  "MODEL_OBSERVABILITY_UI_AUDIT.md",
  "SEMANTIC_INPUT_PROVENANCE_AUDIT.md",
  "OBSERVABILITY_PROGRESS.md",
  "OBSERVABILITY_QUERY_PROGRESS.md",
  "OBSERVABILITY_UI_PROGRESS.md",
  "OBSERVABILITY_IMPLEMENTATION_NOTES.md",
  "PROMPT_PROVENANCE_PROGRESS.md",
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
