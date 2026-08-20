/**
 * §47 真实数据 migration smoke（一次性验收脚本，非仓内测试）
 * 用一份"旧版用户数据"走新版启动路径，验证：
 *   persona migrated / old file preserved safely / memory intact /
 *   automation intact / providers intact / sessions intact
 */
import fs from "fs";
import os from "os";
import path from "path";
import { buildFailedPersonaRenameIndex, migrateAgentPersonaFileNames } from "../core/agents-md-migration.ts";
import { ensureFirstRun } from "../core/first-run.ts";
import { resolvePersonaSource } from "../core/persona-source.ts";

const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-migration-smoke-"));
const lingxiHome = path.join(root, "home");
const productDir = path.resolve("lib");
const agentsDir = path.join(lingxiHome, "agents");
const agentDir = path.join(agentsDir, "my-agent");

// ── 造旧版数据：v0.444.1 时代布局 ─────────────────────────────
fs.mkdirSync(path.join(agentDir, "memory", "daily"), { recursive: true });
fs.mkdirSync(path.join(agentDir, "desk"), { recursive: true });
fs.mkdirSync(path.join(agentDir, "sessions"), { recursive: true });
fs.mkdirSync(path.join(lingxiHome, "user"), { recursive: true });
fs.mkdirSync(path.join(lingxiHome, "providers"), { recursive: true });

fs.writeFileSync(path.join(agentDir, "config.yaml"), [
  "agent:", "  name: 旧助手", "  yuan: lingxi", "user:", "  name: 老用户",
  "locale: zh", "memory:", "  enabled: true", "",
].join("\n"));
// 旧人格文件（ ishiki 时代 ）
fs.writeFileSync(path.join(agentDir, "ishiki.md"), "旧版人格内容：温和、爱喝茶。\n", "utf-8");
fs.writeFileSync(path.join(agentDir, "public-ishiki.md"), "旧版对外人格：对访客简短有礼。\n", "utf-8");
// 记忆/自动化/会话/供应商存量数据
fs.writeFileSync(path.join(agentDir, "memory", "memory.md"), "MEMORY_BEACON_42\n", "utf-8");
fs.writeFileSync(path.join(agentDir, "memory", "facts.md"), "FACTS_BEACON_42\n", "utf-8");
fs.writeFileSync(path.join(agentDir, "memory", "daily", "2026-08-19.md"), "DAILY_BEACON_42\n", "utf-8");
const cronJobs = [{ id: "job-1", label: "喝茶提醒", schedule: "0 9 * * *", enabled: true }];
fs.writeFileSync(path.join(agentDir, "desk", "cron-jobs.json"), JSON.stringify(cronJobs, null, 2), "utf-8");
fs.writeFileSync(path.join(agentDir, "sessions", "session-1.jsonl"), '{"type":"meta"}\n', "utf-8");
fs.writeFileSync(path.join(lingxiHome, "providers", "ollama.json"), JSON.stringify({ apiKey: "PROVIDER_BEACON" }), "utf-8");
fs.writeFileSync(path.join(lingxiHome, "user", "user.md"), "USER_BEACON\n", "utf-8");

// ── 新版启动路径：first-run + 人格迁移（engine 启动序列同序）──
const firstRunReport = ensureFirstRun(lingxiHome, productDir);
const migration = migrateAgentPersonaFileNames({ agentsDir, log: () => {} });

const checks = [];
const ok = (name, cond) => checks.push([name, !!cond]);

// persona migrated
ok("ishiki.md → AGENTS.md", fs.readFileSync(path.join(agentDir, "AGENTS.md"), "utf-8").includes("温和、爱喝茶"));
ok("public-ishiki.md → AGENTS.public.md", fs.readFileSync(path.join(agentDir, "AGENTS.public.md"), "utf-8").includes("对访客简短有礼"));
ok("旧 ishiki.md 已改名（原位不存在）", !fs.existsSync(path.join(agentDir, "ishiki.md")));
ok("旧 public-ishiki.md 已改名", !fs.existsSync(path.join(agentDir, "public-ishiki.md")));
ok("migration report 记录 renamed×2", migration.renamed.length === 2 && migration.failed.length === 0);
// intact
ok("memory.md 未动", fs.readFileSync(path.join(agentDir, "memory", "memory.md"), "utf-8").includes("MEMORY_BEACON_42"));
ok("facts.md 未动", fs.readFileSync(path.join(agentDir, "memory", "facts.md"), "utf-8").includes("FACTS_BEACON_42"));
ok("daily 记忆未动", fs.readFileSync(path.join(agentDir, "memory", "daily", "2026-08-19.md"), "utf-8").includes("DAILY_BEACON_42"));
ok("cron-jobs.json 未动", JSON.parse(fs.readFileSync(path.join(agentDir, "desk", "cron-jobs.json"), "utf-8"))[0].id === "job-1");
ok("sessions 未动", fs.existsSync(path.join(agentDir, "sessions", "session-1.jsonl")));
ok("provider 配置未动", fs.readFileSync(path.join(lingxiHome, "providers", "ollama.json"), "utf-8").includes("PROVIDER_BEACON"));
ok("user.md 未动", fs.readFileSync(path.join(lingxiHome, "user", "user.md"), "utf-8").includes("USER_BEACON"));
ok("first-run 未误报 invalid agent", firstRunReport.invalidAgentDirs.length === 0);

// ── 旧数据从备份回来（新旧并存）─────────────────────────────
fs.writeFileSync(path.join(agentDir, "ishiki.md"), "从备份恢复回来的旧人格。\n", "utf-8");
const second = migrateAgentPersonaFileNames({ agentsDir, log: () => {} });
ok("新旧并存：AGENTS.md 不被覆盖", fs.readFileSync(path.join(agentDir, "AGENTS.md"), "utf-8").includes("温和、爱喝茶"));
ok("新旧并存：旧文件安全保留为 .pre-agents-rename.bak",
  fs.existsSync(path.join(agentDir, "ishiki.md.pre-agents-rename.bak"))
  && fs.readFileSync(path.join(agentDir, "ishiki.md.pre-agents-rename.bak"), "utf-8").includes("备份恢复"));
ok("新旧并存：无 failed", second.failed.length === 0 && second.superseded.length === 1);

// ── 改名失败 → 运行时人格语义（migration-degraded fallback）─────────────
// 语义 smoke：失败不能只保证"文件没删、应用不阻塞"，必须保证用户自定义人格
// 在本次运行中仍然是实际生效的 persona。fs.renameSync 在 CJS interop 下是可
// 写的 plain property，这里直接替换来确定性制造 EACCES（与 vitest spy 同构）。
const lockedDir = path.join(agentsDir, "locked-agent");
fs.mkdirSync(lockedDir, { recursive: true });
fs.writeFileSync(path.join(lockedDir, "ishiki.md"), "锁定人格：改名失败也要生效。\n", "utf-8");
fs.writeFileSync(path.join(lockedDir, "public-ishiki.md"), "锁定对外人格。\n", "utf-8");

const realRenameSync = fs.renameSync;
fs.renameSync = () => {
  const err = new Error("permission denied");
  err.code = "EACCES";
  throw err;
};
const lockedMigration = migrateAgentPersonaFileNames({ agentsDir, log: () => {} });
fs.renameSync = realRenameSync;

ok("改名失败被结构化记录（ishiki + public-ishiki）",
  lockedMigration.failed.length === 2 && lockedMigration.failedDetails.length === 2);
ok("改名失败：旧文件原地保留未删",
  fs.readFileSync(path.join(lockedDir, "ishiki.md"), "utf-8").includes("改名失败也要生效"));

const failedIndex = buildFailedPersonaRenameIndex(lockedMigration.failedDetails);
const degraded = resolvePersonaSource({
  agentDir: lockedDir,
  productDir,
  yuanType: "lingxi",
  locale: "zh",
  kind: "agents",
  migrationFallback: { legacyFilePath: path.join(lockedDir, failedIndex.get("locked-agent").get("AGENTS.md")) },
});
ok("改名失败：effective persona 仍是用户自定义内容", degraded.content.includes("改名失败也要生效"));
ok("改名失败：fromTemplate=false（不是模板回落）", degraded.fromTemplate === false);

const noRecord = resolvePersonaSource({
  agentDir: lockedDir, productDir, yuanType: "lingxi", locale: "zh", kind: "agents",
});
ok("无失败记录：旧文件不读（无永久双读协议）",
  noRecord.fromTemplate === true && !noRecord.content.includes("改名失败也要生效"));

// 下次启动改名成功：degraded fallback 自动消失，人格内容不丢
const healed = migrateAgentPersonaFileNames({ agentsDir, log: () => {} });
ok("下次启动改名成功：locked-agent 完成迁移",
  healed.renamed.includes("locked-agent/AGENTS.md") && healed.failed.length === 0);
const healedPersona = resolvePersonaSource({
  agentDir: lockedDir, productDir, yuanType: "lingxi", locale: "zh", kind: "agents",
});
ok("改名成功后：人格内容从 AGENTS.md 读出且不丢",
  healedPersona.fromTemplate === false && healedPersona.content.includes("改名失败也要生效"));

fs.rmSync(root, { recursive: true, force: true });

let failed = 0;
for (const [name, pass] of checks) {
  console.log(`${pass ? "✓" : "✗ FAIL"}  ${name}`);
  if (!pass) failed += 1;
}
console.log(`\n${checks.length - failed}/${checks.length} checks passed`);
process.exit(failed ? 1 : 0);
