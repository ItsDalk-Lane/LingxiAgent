/**
 * pinned-tenets-migration.ts — 一次性迁移：旧「置顶记忆」文件并入 tenets 库。
 *
 * 背景：置顶记忆（pinned.md + pinned-memory.json）与用户原则（tenets.json）
 * 已合并为单一「置顶与原则」库（tenets 为底座）。本迁移把每个 agent 目录下
 * 尚未迁移的 pins 数据无损并入 tenets.json，然后把旧文件改名归档留证。
 *
 * 与旧版运行时的权威来源规则一致（父基线 pinned-memory-store.ts）：
 * Markdown mtime > JSON mtime + 1ms 时以 Markdown 为准，否则以 JSON 为准；
 * JSON 缺失才以 Markdown 为准。合法的空 items 是删除语义，不拿另一份复活。
 * 权威来源损坏/不可读 = 明确失败，不静默按空库迁移、不凭猜测回退另一份。
 *
 * 可靠性（收据状态机，收据不含正文，正文备份在本地受控目录）：
 *   not_started → prepared → target_committed → sources_archived → completed
 *                      ↘ failed（可重试）/ conflict（粘性，不自动重试）
 * 提交前复查源与目标未改变；目标原子写入；逐项映射只记哈希与 id。
 * completed 收据永不驱动重新导入（用户之后删除的条目不复活）。
 */

import fs from "fs";
import path from "path";
import { createHash } from "node:crypto";
import { createModuleLogger } from "../lib/debug-log.ts";
import { atomicWriteSync } from "../shared/safe-fs.ts";
import {
  planLegacyPinnedImport,
  readTenetsFileStrict,
  serializeTenetsFile,
  tenetsFilePath,
  isTenetError,
  TENET_ERRORS,
  type LegacyPinImportItem,
  type LegacyPinImportEntry,
  type TenetsFile,
} from "../lib/memory/tenets.ts";

const log = createModuleLogger("pins-migration");

export const PINNED_MD = "pinned.md";
export const PINNED_JSON = "pinned-memory.json";
export const PINNED_TENETS_MIGRATION_RECEIPT = "pinned-tenets-migration.receipt.json";
const MIGRATION_VERSION = 2;
const BACKUP_DIR = "pinned-migration-backup";

export interface MigrationReceipt {
  version: number;
  kind: "migration" | "recovery";
  agentId: string;
  state: "prepared" | "target_committed" | "sources_archived" | "completed" | "failed" | "conflict";
  sources: Array<{ file: string; sha256: string; mtimeMs: number }>;
  authority: { file: string; reason: string } | null;
  target: { existed: boolean; sha256: string | null };
  resultSha256: string | null;
  plan: Array<Omit<LegacyPinImportEntry, "normalizedContent"> & { exemption?: string }>;
  counts: {
    sourceItems: number;
    added: number;
    duplicateActive: number;
    addedOverHistory: number;
    duplicateInBatch: number;
  };
  archived: Array<{ from: string; to: string }>;
  backupDir: string | null;
  error: { code: string; message: string } | null;
  createdAt: string;
  updatedAt: string;
  completedAt: string | null;
}

export interface MigrationFaultHooks {
  /** 测试用故障注入点；生产调用不传。抛错即模拟崩溃。 */
  at?: (checkpoint: string) => void;
}

// ── 通用小工具 ──────────────────────────────────────────────────────────────

function sha256Text(text: string): string {
  return createHash("sha256").update(text, "utf-8").digest("hex");
}

export function sha256File(filePath: string): string {
  return createHash("sha256").update(fs.readFileSync(filePath)).digest("hex");
}

function receiptPath(agentDir: string): string {
  return path.join(agentDir, "memory", PINNED_TENETS_MIGRATION_RECEIPT);
}

export function readPinnedTenetsMigrationReceipt(agentDir: string): MigrationReceipt | null {
  try {
    const raw = JSON.parse(fs.readFileSync(receiptPath(agentDir), "utf-8"));
    if (!raw || typeof raw !== "object" || typeof raw.state !== "string") return null;
    return raw as MigrationReceipt;
  } catch {
    return null;
  }
}

function writeReceipt(agentDir: string, receipt: MigrationReceipt): MigrationReceipt {
  receipt.updatedAt = new Date().toISOString();
  fs.mkdirSync(path.dirname(receiptPath(agentDir)), { recursive: true });
  atomicWriteSync(receiptPath(agentDir), JSON.stringify(receipt, null, 2) + "\n");
  return receipt;
}

function migrationError(code: string, message: string): Error & { code: string } {
  const err: any = new Error(message);
  err.code = code;
  return err;
}

// ── 旧 pins 解析 ────────────────────────────────────────────────────────────

export function parsePinnedMarkdownItems(content: string): string[] {
  const text = String(content ?? "").replace(/\r\n/g, "\n").replace(/\r/g, "\n");
  const lines = text.endsWith("\n") ? text.slice(0, -1).split("\n") : text.split("\n");
  const rawItems: string[] = [];
  let current: string | null = null;

  for (const line of lines) {
    const bullet = line.match(/^-\s(.*)$/);
    if (bullet) {
      if (current !== null) rawItems.push(current);
      current = bullet[1];
      continue;
    }
    if (current === null) {
      if (line.trim()) current = line;
      continue;
    }
    current += `\n${line.replace(/^ {2}/, "")}`;
  }
  if (current !== null) rawItems.push(current);
  return rawItems.map((item) => item.trim()).filter(Boolean);
}

/**
 * 按文件名解析旧 pins 源（pinned-memory.json / pinned.md，含 .migrated 变体）。
 * 解析失败抛错——权威来源损坏必须明确失败，不能静默当空库。
 */
export function readLegacyPinnedSource(filePath: string): LegacyPinImportItem[] {
  const base = path.basename(filePath);
  const rawText = fs.readFileSync(filePath, "utf-8");
  if (base.startsWith(PINNED_JSON)) {
    let raw: any;
    try {
      raw = JSON.parse(rawText);
    } catch {
      throw migrationError("MIGRATION_SOURCE_UNREADABLE", `${base} is not valid JSON`);
    }
    if (!raw || typeof raw !== "object" || raw.version !== 1 || !Array.isArray(raw.items)) {
      throw migrationError("MIGRATION_SOURCE_UNREADABLE", `${base} has an unsupported or damaged shape`);
    }
    return raw.items
      .map((item: any) => ({
        legacyId: typeof item?.id === "string" && item.id ? item.id : null,
        content: String(item?.content ?? "").replace(/\r\n?/g, "\n").trim(),
        createdAt: typeof item?.createdAt === "string" ? item.createdAt : null,
      }))
      .filter((item: LegacyPinImportItem) => item.content);
  }
  if (base.startsWith(PINNED_MD)) {
    return parsePinnedMarkdownItems(rawText).map((content) => ({
      legacyId: null,
      content,
      createdAt: null,
    }));
  }
  throw migrationError("MIGRATION_SOURCE_UNREADABLE", `${base} is not a recognized pinned source`);
}

// ── 权威来源选择（沿用旧版运行时 mtime 规则） ───────────────────────────────

type AuthorityChoice = { file: string; reason: string };

function chooseAuthority(agentDir: string): AuthorityChoice | null {
  const jsonPath = path.join(agentDir, PINNED_JSON);
  const mdPath = path.join(agentDir, PINNED_MD);
  const jsonStat = fs.existsSync(jsonPath) ? fs.statSync(jsonPath) : null;
  const mdStat = fs.existsSync(mdPath) ? fs.statSync(mdPath) : null;
  if (!jsonStat && !mdStat) return null;
  if (jsonStat && !mdStat) return { file: PINNED_JSON, reason: "json_only" };
  if (!jsonStat && mdStat) return { file: PINNED_MD, reason: "markdown_only" };
  if (mdStat!.mtimeMs > jsonStat!.mtimeMs + 1) {
    return { file: PINNED_MD, reason: "markdown_newer_mtime" };
  }
  return { file: PINNED_JSON, reason: "json_preferred" };
}

// ── 收据计划条目的序列化（剥离正文，只留哈希与 id） ─────────────────────────

function receiptPlanEntries(entries: readonly LegacyPinImportEntry[]): MigrationReceipt["plan"] {
  return entries.map(({ normalizedContent: _content, ...entry }) => ({
    ...entry,
    exemption: "legacy_migration",
  }));
}

function planCounts(entries: readonly LegacyPinImportEntry[]): MigrationReceipt["counts"] {
  return {
    sourceItems: entries.length,
    added: entries.filter((e) => e.outcome === "added").length,
    duplicateActive: entries.filter((e) => e.outcome === "duplicate_active").length,
    addedOverHistory: entries.filter((e) => e.outcome === "added_over_pending_history" || e.outcome === "added_over_rejected_history").length,
    duplicateInBatch: entries.filter((e) => e.outcome === "duplicate_in_batch").length,
  };
}

// ── 归档（不覆盖既有 .migrated） ────────────────────────────────────────────

function archiveTargetName(agentDir: string, base: string, sourceSha256: string): string {
  const plain = path.join(agentDir, `${base}.migrated`);
  if (!fs.existsSync(plain)) return `${base}.migrated`;
  return `${base}.migrated-${sourceSha256.slice(0, 8)}`;
}

// ── 主流程 ──────────────────────────────────────────────────────────────────

function failWithReceipt(agentDir: string, receipt: MigrationReceipt, code: string, message: string): void {
  receipt.state = "failed";
  receipt.error = { code, message };
  try {
    writeReceipt(agentDir, receipt);
  } catch (err: any) {
    log.warn(`[${receipt.agentId}] failed to persist migration receipt: ${err?.message || err}`);
  }
}

function newReceipt(agentId: string): MigrationReceipt {
  return {
    version: MIGRATION_VERSION,
    kind: "migration",
    agentId,
    state: "prepared",
    sources: [],
    authority: null,
    target: { existed: false, sha256: null },
    resultSha256: null,
    plan: [],
    counts: { sourceItems: 0, added: 0, duplicateActive: 0, addedOverHistory: 0, duplicateInBatch: 0 },
    archived: [],
    backupDir: null,
    error: null,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
    completedAt: null,
  };
}

/** 读取并校验全部来源与目标；任何损坏都抛带码错误（由调用方记 failed 收据）。 */
function readSourcesAndTarget(agentDir: string, authority: AuthorityChoice) {
  const sources: MigrationReceipt["sources"] = [];
  for (const base of [PINNED_JSON, PINNED_MD]) {
    const filePath = path.join(agentDir, base);
    if (!fs.existsSync(filePath)) continue;
    const stat = fs.statSync(filePath);
    sources.push({ file: base, sha256: sha256File(filePath), mtimeMs: stat.mtimeMs });
  }
  // 权威来源必须可解析（损坏即抛 MIGRATION_SOURCE_UNREADABLE）
  const items = readLegacyPinnedSource(path.join(agentDir, authority.file));
  // 目标严格读取（损坏/未来 schema/EACCES 即抛 TENET_STORE_*）
  const targetPath = tenetsFilePath(agentDir);
  const targetExisted = fs.existsSync(targetPath);
  let targetSha256: string | null = null;
  let targetData: TenetsFile;
  try {
    targetSha256 = targetExisted ? sha256File(targetPath) : null;
    targetData = readTenetsFileStrict(targetPath);
  } catch (err: any) {
    if (isTenetError(err)) throw err;
    throw migrationError(TENET_ERRORS.STORE_READ_FAILED, `cannot read target tenets store: ${err?.code || err?.message || err}`);
  }
  return { sources, items, targetPath, targetExisted, targetSha256, targetData };
}

function writeBackups(agentDir: string, sources: MigrationReceipt["sources"]): string {
  const backupDir = path.join(agentDir, "memory", BACKUP_DIR);
  fs.mkdirSync(backupDir, { recursive: true });
  for (const source of sources) {
    const name = `${source.sha256.slice(0, 8)}-${source.file}`;
    const dest = path.join(backupDir, name);
    if (!fs.existsSync(dest)) {
      fs.copyFileSync(path.join(agentDir, source.file), dest);
    }
  }
  return path.join("memory", BACKUP_DIR);
}

function verifyTargetCommitted(targetPath: string, resultSha256: string, plan: MigrationReceipt["plan"]): boolean {
  let raw: string;
  try {
    raw = fs.readFileSync(targetPath, "utf-8");
  } catch {
    return false;
  }
  if (sha256Text(raw) === resultSha256) return true;
  // 目标在提交后又有新写入：计划内条目仍在即视为已提交（绝不重写覆盖新数据）
  let data: TenetsFile;
  try {
    data = readTenetsFileStrict(targetPath);
  } catch {
    return false;
  }
  const plannedAdds = plan.filter((p) => p.outcome === "added" || p.outcome === "added_over_pending_history" || p.outcome === "added_over_rejected_history");
  return plannedAdds.every((p) => data.tenets.some((t) => t.id === p.tenetId));
}

function archiveSources(agentDir: string, receipt: MigrationReceipt, hooks?: MigrationFaultHooks): void {
  for (const source of receipt.sources) {
    if (receipt.archived.some((a) => a.from === source.file)) continue;
    const filePath = path.join(agentDir, source.file);
    if (!fs.existsSync(filePath)) {
      throw migrationError(
        "MIGRATION_CONFLICT",
        `migration conflict: source ${source.file} disappeared before archiving (agent ${receipt.agentId})`,
      );
    }
    hooks?.at?.(`archive:before:${source.file}`);
    const to = archiveTargetName(agentDir, source.file, source.sha256);
    fs.renameSync(filePath, path.join(agentDir, to));
    receipt.archived.push({ from: source.file, to });
    receipt.state = receipt.state === "prepared" ? "target_committed" : receipt.state;
    writeReceipt(agentDir, receipt);
  }
}

function finalizeReceipt(agentDir: string, receipt: MigrationReceipt, hooks?: MigrationFaultHooks): void {
  if (receipt.state !== "sources_archived") {
    receipt.state = "sources_archived";
    writeReceipt(agentDir, receipt);
  }
  hooks?.at?.("completed:before");
  receipt.state = "completed";
  receipt.error = null;
  receipt.completedAt = new Date().toISOString();
  writeReceipt(agentDir, receipt);
}

/**
 * 单个 agent 的迁移入口（引擎扫描与测试故障注入共用）。
 * 收据 completed/conflict 直接返回；failed 从头重试；中间态断点续跑。
 */
export function migrateAgentPinnedTenets(agentDir: string, agentId: string, hooks?: MigrationFaultHooks): void {
  const existing = readPinnedTenetsMigrationReceipt(agentDir);
  if (existing && (existing.state === "completed" || existing.state === "conflict")) return;
  if (existing && existing.state !== "failed") {
    resumeMigration(agentDir, existing, hooks);
    return;
  }
  runFreshMigration(agentDir, agentId, hooks, existing ?? undefined);
}

function runFreshMigration(agentDir: string, agentId: string, hooks: MigrationFaultHooks | undefined, prior?: MigrationReceipt): void {
  const authority = chooseAuthority(agentDir);
  if (!authority) {
    // 无源文件：完全不动（不写收据、不建目录）
    if (prior) writeReceipt(agentDir, { ...prior, state: "failed", error: { code: "MIGRATION_SOURCE_UNREADABLE", message: "source files disappeared before retry" } });
    return;
  }

  const receipt = newReceipt(agentId);
  receipt.authority = authority;

  let prepared: ReturnType<typeof readSourcesAndTarget>;
  try {
    prepared = readSourcesAndTarget(agentDir, authority);
  } catch (err: any) {
    const code = isTenetError(err) ? err.code : (err?.code || "MIGRATION_SOURCE_UNREADABLE");
    failWithReceipt(agentDir, receipt, code, err?.message || String(err));
    return;
  }
  receipt.sources = prepared.sources;
  receipt.target = { existed: prepared.targetExisted, sha256: prepared.targetSha256 };

  const { entries, finalTenets } = planLegacyPinnedImport(prepared.targetData, prepared.items);
  const finalBytes = serializeTenetsFile({ schemaVersion: prepared.targetData.schemaVersion, tenets: finalTenets });
  receipt.resultSha256 = sha256Text(finalBytes);
  receipt.plan = receiptPlanEntries(entries);
  receipt.counts = planCounts(entries);

  try {
    receipt.backupDir = writeBackups(agentDir, receipt.sources);
  } catch (err: any) {
    failWithReceipt(agentDir, receipt, "MIGRATION_BACKUP_FAILED", err?.message || String(err));
    return;
  }

  receipt.state = "prepared";
  writeReceipt(agentDir, receipt);
  hooks?.at?.("receipt:prepared");

  // 提交前复查：源与目标必须保持 prepare 时的字节
  hooks?.at?.("recheck:before");
  assertUnchanged(agentDir, receipt, prepared.targetPath);

  // 原子提交目标
  hooks?.at?.("commit:before");
  fs.mkdirSync(path.dirname(prepared.targetPath), { recursive: true });
  atomicWriteSync(prepared.targetPath, finalBytes);
  hooks?.at?.("commit:after");

  if (!verifyTargetCommitted(prepared.targetPath, receipt.resultSha256!, receipt.plan)) {
    failWithReceipt(agentDir, receipt, "MIGRATION_VERIFY_FAILED", "target verification failed after commit");
    return;
  }
  receipt.state = "target_committed";
  writeReceipt(agentDir, receipt);

  archiveSources(agentDir, receipt, hooks);
  finalizeReceipt(agentDir, receipt, hooks);
  log.log(`[${agentId}] pinned→tenets migration: ${receipt.counts.added} added, ${receipt.counts.duplicateActive} duplicates, ${receipt.counts.sourceItems} source items`);
}

function assertUnchanged(agentDir: string, receipt: MigrationReceipt, targetPath: string): void {
  for (const source of receipt.sources) {
    const filePath = path.join(agentDir, source.file);
    if (!fs.existsSync(filePath) || sha256File(filePath) !== source.sha256) {
      receipt.state = "conflict";
      receipt.error = { code: "MIGRATION_CONFLICT", message: `source ${source.file} changed during preparation` };
      writeReceipt(agentDir, receipt);
      throw migrationError("MIGRATION_CONFLICT", `migration conflict: source ${source.file} changed during preparation (agent ${receipt.agentId})`);
    }
  }
  const targetExists = fs.existsSync(targetPath);
  const targetSha256 = targetExists ? sha256File(targetPath) : null;
  if (targetExists !== receipt.target.existed || targetSha256 !== receipt.target.sha256) {
    receipt.state = "conflict";
    receipt.error = { code: "MIGRATION_CONFLICT", message: "target tenets store changed during preparation" };
    writeReceipt(agentDir, receipt);
    throw migrationError("MIGRATION_CONFLICT", `migration conflict: target tenets store changed during preparation (agent ${receipt.agentId})`);
  }
}

function resumeMigration(agentDir: string, receipt: MigrationReceipt, hooks?: MigrationFaultHooks): void {
  if (receipt.state === "sources_archived") {
    finalizeReceipt(agentDir, receipt, hooks);
    return;
  }

  const targetPath = tenetsFilePath(agentDir);
  const targetExists = fs.existsSync(targetPath);
  const targetSha256 = targetExists ? sha256File(targetPath) : null;

  if (receipt.state === "target_committed") {
    // 目标已提交：只校验计划条目仍在（允许后续用户写入），然后只补归档。
    if (!targetExists || !verifyTargetCommitted(targetPath, receipt.resultSha256!, receipt.plan)) {
      receipt.state = "conflict";
      receipt.error = { code: "MIGRATION_CONFLICT", message: "committed target no longer contains the planned entries" };
      writeReceipt(agentDir, receipt);
      return;
    }
    archiveSources(agentDir, receipt, hooks);
    finalizeReceipt(agentDir, receipt, hooks);
    return;
  }

  // state === "prepared"：目标可能已提交（崩溃在收据更新前），也可能未提交。
  // 源必须全部在且字节一致——计划内容要从源重新推导。
  for (const source of receipt.sources) {
    const filePath = path.join(agentDir, source.file);
    if (!fs.existsSync(filePath) || sha256File(filePath) !== source.sha256) {
      receipt.state = "conflict";
      receipt.error = { code: "MIGRATION_CONFLICT", message: `source ${source.file} changed or disappeared after prepare` };
      writeReceipt(agentDir, receipt);
      return;
    }
  }

  if (targetExists && targetSha256 === receipt.resultSha256) {
    // 崩溃在 commit 之后、收据更新之前：目标已是计划终态，只补收据与归档。
    receipt.state = "target_committed";
    writeReceipt(agentDir, receipt);
    archiveSources(agentDir, receipt, hooks);
    finalizeReceipt(agentDir, receipt, hooks);
    return;
  }

  const matchesOriginalTarget = targetExists === receipt.target.existed && targetSha256 === receipt.target.sha256;
  if (!matchesOriginalTarget) {
    // 目标在 prepare 后被改过：计划条目若已齐全则视为已提交，否则冲突。
    if (targetExists && verifyTargetCommitted(targetPath, receipt.resultSha256!, receipt.plan)) {
      receipt.state = "target_committed";
      writeReceipt(agentDir, receipt);
      archiveSources(agentDir, receipt, hooks);
      finalizeReceipt(agentDir, receipt, hooks);
      return;
    }
    receipt.state = "conflict";
    receipt.error = { code: "MIGRATION_CONFLICT", message: "target tenets store changed after prepare" };
    writeReceipt(agentDir, receipt);
    return;
  }

  // 未提交：用收据中的计划 id/时间戳重放，保证终态字节与 resultSha256 一致。
  let targetData: TenetsFile;
  let items: LegacyPinImportItem[];
  try {
    targetData = readTenetsFileStrict(targetPath);
    items = readLegacyPinnedSource(path.join(agentDir, receipt.authority!.file));
  } catch (err: any) {
    const code = isTenetError(err) ? err.code : (err?.code || "MIGRATION_SOURCE_UNREADABLE");
    failWithReceipt(agentDir, receipt, code, err?.message || String(err));
    return;
  }
  const overrides = receipt.plan.map((p) => ({ tenetId: p.tenetId, createdAt: p.createdAt, decidedAt: p.decidedAt }));
  const { entries, finalTenets } = planLegacyPinnedImport(targetData, items, overrides);
  const finalBytes = serializeTenetsFile({ schemaVersion: targetData.schemaVersion, tenets: finalTenets });
  if (sha256Text(finalBytes) !== receipt.resultSha256) {
    receipt.state = "conflict";
    receipt.error = { code: "MIGRATION_CONFLICT", message: "recomputed plan diverges from the prepared receipt" };
    writeReceipt(agentDir, receipt);
    return;
  }

  hooks?.at?.("commit:before");
  fs.mkdirSync(path.dirname(targetPath), { recursive: true });
  atomicWriteSync(targetPath, finalBytes);
  hooks?.at?.("commit:after");

  if (!verifyTargetCommitted(targetPath, receipt.resultSha256!, receipt.plan)) {
    failWithReceipt(agentDir, receipt, "MIGRATION_VERIFY_FAILED", "target verification failed after commit");
    return;
  }
  receipt.state = "target_committed";
  receipt.plan = receiptPlanEntries(entries);
  receipt.counts = planCounts(entries);
  writeReceipt(agentDir, receipt);

  archiveSources(agentDir, receipt, hooks);
  finalizeReceipt(agentDir, receipt, hooks);
}

/** 启动时调用：扫全部 agent 目录执行一次性迁移。失败只记日志与收据，不阻塞启动。 */
export function migratePinnedMemoryToTenets(lingxiHome: string): void {
  try {
    const agentsDir = path.join(lingxiHome, "agents");
    if (!fs.existsSync(agentsDir)) return;
    for (const entry of fs.readdirSync(agentsDir, { withFileTypes: true })) {
      if (!entry.isDirectory() || entry.name.startsWith(".")) continue;
      try {
        migrateAgentPinnedTenets(path.join(agentsDir, entry.name), entry.name);
      } catch (err: any) {
        log.warn(`[${entry.name}] migration failed: ${err?.message || err}`);
      }
    }
  } catch (err: any) {
    log.warn(`pinned→tenets migration scan failed: ${err?.message || err}`);
  }
}
