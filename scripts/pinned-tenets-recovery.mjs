#!/usr/bin/env node
/**
 * pinned-tenets-recovery.mjs — 旧「置顶记忆」.migrated 归档的恢复工具。
 *
 * 默认 dry-run（只读扫描，不改任何文件）：
 *   node scripts/pinned-tenets-recovery.mjs [--home <LINGXI_HOME>]
 *
 * 真实恢复（必须显式批准；只恢复清单里 action=restore 的条目）：
 *   node scripts/pinned-tenets-recovery.mjs --home <LINGXI_HOME> --apply --approval <approval.json>
 *
 * approval.json 格式：
 *   {
 *     "agentId": "hana",
 *     "source": "pinned-memory.json.migrated",
 *     "decisions": [{ "contentHash": "sha256:<hex>", "action": "restore" | "skip" }]
 *   }
 * contentHash 从 dry-run 报告的 candidates 中复制。
 *
 * 边界：归档源文件永不删除；收据写在 agentDir/memory/ 下；不重置系统权限；
 * 不操作 --home 以外的目录。macOS 授权状态与本工具无关。
 */
import path from "node:path";
import fs from "node:fs";
import os from "node:os";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

function parseArgs(argv) {
  const args = { home: path.join(os.homedir(), ".lingxi"), apply: false, approval: null };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--home") args.home = path.resolve(argv[++i]);
    else if (arg === "--apply") args.apply = true;
    else if (arg === "--approval") args.approval = path.resolve(argv[++i]);
    else if (arg === "--help" || arg === "-h") {
      console.log(fs.readFileSync(fileURLToPath(import.meta.url), "utf-8").split("\n").slice(0, 24).join("\n"));
      process.exit(0);
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
  return args;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const { scanPinnedTenetsRecovery, applyPinnedTenetsRecovery } = await import(
    path.join(__dirname, "..", "core", "pinned-tenets-recovery.ts")
  );

  if (!args.apply) {
    const report = scanPinnedTenetsRecovery(args.home);
    console.log(JSON.stringify(report, null, 2));
    if (report.agents.length > 0) {
      console.error(
        `\ndry-run only. To restore, write an approval JSON (agentId/source/decisions by contentHash) ` +
        `and re-run with --apply --approval <file>.`,
      );
    }
    return;
  }

  if (!args.approval) {
    throw new Error("--apply requires --approval <file>; refusing to restore without an explicit approval manifest");
  }
  const approval = JSON.parse(fs.readFileSync(args.approval, "utf-8"));
  const result = applyPinnedTenetsRecovery(args.home, approval);
  console.log(JSON.stringify({ ok: true, restored: result.restored }, null, 2));
}

main().catch((err) => {
  console.error(`pinned-tenets-recovery failed: ${err?.message || err}`);
  process.exit(1);
});
