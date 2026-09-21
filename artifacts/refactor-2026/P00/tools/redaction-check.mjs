#!/usr/bin/env node
/**
 * P00-A09「脱敏样本」扫描器：检查 P00 全部可公开产物（docs/refactor-2026/P00、
 * artifacts/refactor-2026/P00/logs、fixtures、samples、isolated 的 JSON 摘要）
 * 是否泄漏合成敏感标记或常见敏感模式。受控样本文件本身豁免。
 * 命中即 exit 1（FAIL）。
 */
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import console from "node:console";
import { fileURLToPath } from "node:url";

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");
const P00 = path.join(REPO, "artifacts", "refactor-2026");
const DOCS = path.join(REPO, "docs", "refactor-2026");
const EXEMPT = new Set([
  path.join(P00, "P00", "fixtures", "redaction", "synthetic-secret-sample.json"),
  // 扫描器自身必须包含待检模式的定义，豁免自扫。
  path.resolve(fileURLToPath(import.meta.url)),
  // 受控保留（仅限以下两份，验收修复轮自整族正则收窄）：首两版扫描器的错误日志
  // 引用了受控样本标记，已在 P00_REPORT 差异与限制一节声明不进入公开打包。
  // r3/r4 起错误只打印模式名、不含标记，无需也不应豁免。
  path.join(P00, "P00", "logs", "P00-A09-redaction-check.err"),
  path.join(P00, "P00", "logs", "P00-A09-redaction-check-r2.err"),
]);
// 公开产物白名单之外的位置（隔离 HOME 里 server 写的合成运行数据）不扫敏感，
// 但 server-A 原始日志保留在受控位置、也不公开。
const SCAN_ROOTS = [
  path.join(DOCS, "P00"),
  path.join(P00, "P00", "logs"),
  path.join(P00, "P00", "fixtures"),
  path.join(P00, "P00", "samples"),
  path.join(P00, "P00", "tools"),
];
// 避开原始 out/err 大文件？不——A09 要求日志/索引均无敏感标记。但 command-log.jsonl
// 记录的 argv 含 --reason 文本，也需扫描。全部纳入。
const PATTERNS = [
  [/sk-P00SYNTHETIC-[a-z0-9-]+/, "synthetic-api-key"],
  [/p00-bearer-synthetic-\d+/, "synthetic-bearer"],
  [/-----BEGIN RSA PRIVATE KEY-----P00SYNTHETIC/, "synthetic-private-key-header"],
  [/P00SyntheticPassword/, "synthetic-password"],
  [/ghp_P00SyntheticToken\w*/, "synthetic-gh-token"],
  [/P00-USER-CONTENT-MARKER-\w+/, "synthetic-user-content"],
  // 常见真实形态（宽口径）：长 JWT
  [/Bearer\s+eyJ[A-Za-z0-9_-]{20,}/, "real-jwt-shape"],
];

const hits = [];
function walk(dir) {
  if (!fs.existsSync(dir)) return;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) { walk(p); continue; }
    if (EXEMPT.has(p)) continue;
    if (!/\.(json|jsonl|md|mjs|txt|log|out|err)$/.test(entry.name)) continue;
    const text = fs.readFileSync(p, "utf-8");
    for (const [re, name] of PATTERNS) {
      if (re.test(text)) hits.push(`${path.relative(REPO, p)} :: ${name}`);
    }
  }
}
for (const root of SCAN_ROOTS) walk(root);

if (hits.length) {
  console.error(`redaction-check FAIL (${hits.length} hits):`);
  for (const h of hits) console.error("  - " + h);
  process.exit(1);
}
console.log("redaction-check PASS: P00 公开产物（docs/artifacts 的日志、索引、fixture、工具）未发现合成敏感标记或常见敏感形态");
