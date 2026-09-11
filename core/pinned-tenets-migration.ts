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
 *   not_started → prepared → committing → target_committed → sources_archived → completed
 *                      ↘ failed（可重试）/ conflict（粘性，不自动重试）
 * prepared 尚未尝试提交；committing 必须先于目标写入持久化，不能凭原目标字节重放。
 * 提交前复查源与目标未改变；目标原子写入；逐项映射只记哈希与 id。
 * completed 收据永不驱动重新导入（用户之后删除的条目不复活）。
 */

import fs from "fs";
import path from "path";
import { createHash, randomUUID } from "node:crypto";
import { createModuleLogger } from "../lib/debug-log.ts";
import { atomicWriteSync } from "../shared/safe-fs.ts";
import { backupDirForReceipt, parseReceiptBackupDir, receiptBackupDirLocalPath } from "./pinned-tenets-backup-dir.ts";
import {
  planLegacyPinnedImport,
  legacyContentKey,
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
const MIGRATION_VERSION = 4;
const BACKUP_DIR = "pinned-migration-backup";

export interface MigrationReceipt {
  version: number;
  operationId?: string;
  kind: "migration" | "recovery";
  agentId: string;
  state: "prepared" | "committing" | "target_committed" | "sources_archived" | "completed" | "failed" | "conflict";
  sources: Array<{ file: string; sha256: string; mtimeMs: number }>;
  authority: { file: string; reason: string } | null;
  target: { existed: boolean; sha256: string | null };
  resultSha256: string | null;
  plan: Array<Omit<LegacyPinImportEntry, "normalizedContent"> & { exemption?: string; source?: string; sourceEntryKey?: string }>;
  counts: {
    sourceItems: number;
    added: number;
    duplicateActive: number;
    addedOverHistory: number;
    duplicateInBatch: number;
  };
  archived: Array<{ from: string; to: string; sourceHash?: string; state?: "planned" | "done" }>;
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

const HASH = /^[0-9a-f]{64}$/;
const safeName = (value: unknown): value is string => typeof value === "string" && !!value
  && value !== "." && value !== ".." && !/[\\/\0]/.test(value);
const sourceName = (value: unknown): value is string => safeName(value)
  && /^(pinned-memory\.json|pinned\.md)(\.migrated(?:-[a-zA-Z0-9-]+)?)?$/.test(value);

/** 缺失返回 null；读取失败和非法收据必须阻止自动写入。 */
export function readPinnedTenetsMigrationReceipt(agentDir: string): MigrationReceipt | null {
  let text: string;
  try { text = fs.readFileSync(receiptPath(agentDir), "utf8"); }
  catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw migrationError("MIGRATION_RECEIPT_UNREADABLE", "cannot read migration receipt");
  }
  let raw: unknown;
  try { raw = JSON.parse(text); } catch { throw migrationError("MIGRATION_RECEIPT_INVALID", "invalid migration receipt JSON"); }
  if (!isMigrationReceipt(raw, path.basename(agentDir))) {
    throw migrationError("MIGRATION_RECEIPT_INVALID", "invalid migration receipt identity, schema or plan");
  }
  return raw;
}

export function isMigrationReceipt(value: unknown, agentId: string): value is MigrationReceipt {
  if (!value || typeof value !== "object") return false;
  const r = value as MigrationReceipt;
  if (![2, 3, 4].includes(r.version) || r.agentId !== agentId || !["migration", "recovery"].includes(r.kind)
    || !["prepared", "committing", "target_committed", "sources_archived", "completed", "failed", "conflict"].includes(r.state)
    || !Array.isArray(r.sources) || !Array.isArray(r.plan) || !Array.isArray(r.archived)
    || !r.target || typeof r.target.existed !== "boolean" || !r.counts
    || typeof r.createdAt !== "string" || typeof r.updatedAt !== "string") return false;
  const legacyRecovery = r.version === 2 && r.kind === "recovery";
  if (r.target.sha256 !== null && !HASH.test(r.target.sha256)) return false;
  if (r.target.existed && !r.target.sha256 && !legacyRecovery) return false;
  if (!r.target.existed && r.target.sha256 !== null) return false;
  if (r.resultSha256 !== null && !HASH.test(r.resultSha256)) return false;
  if (!["failed", "conflict"].includes(r.state) && !r.resultSha256 && !legacyRecovery) return false;
  if (r.version >= 3 && !safeName(r.operationId)) return false;
  if (r.version < 4 && r.state === "committing") return false;
  if (r.sources.some(x => !x || !sourceName(x.file) || !HASH.test(x.sha256) || !Number.isFinite(x.mtimeMs))
    || new Set(r.sources.map(x => x.file)).size !== r.sources.length) return false;
  if (r.authority && (!sourceName(r.authority.file) || !r.sources.some(x => x.file === r.authority!.file))) return false;
  if (!r.authority && !["failed", "conflict"].includes(r.state)) return false;
  if (r.counts.sourceItems !== r.plan.length || !Object.values(r.counts).every(n => Number.isInteger(n) && n >= 0)) return false;
  const expected = new Map<string, string>();
  for (const [i, entry] of r.plan.entries()) {
    if (!entry || entry.order !== i || typeof entry.tenetId !== "string" || !entry.tenetId
      || !/^sha256:[0-9a-f]{64}$/.test(entry.contentHash)
      || !["added", "added_over_pending_history", "added_over_rejected_history", "duplicate_active", "duplicate_in_batch"].includes(entry.outcome)
      || typeof entry.createdAt !== "string" || !Number.isFinite(Date.parse(entry.createdAt))
      || typeof entry.decidedAt !== "string" || !Number.isFinite(Date.parse(entry.decidedAt))
      || !(entry.legacyId === null || typeof entry.legacyId === "string")) return false;
    if (expected.has(entry.tenetId) && expected.get(entry.tenetId) !== entry.contentHash) return false;
    expected.set(entry.tenetId, entry.contentHash);
  }
  // backupDir 存储合同（C02）：规范 POSIX 形式；旧 Windows 反斜杠形式只读兼容。
  if (r.backupDir !== null && parseReceiptBackupDir(r.backupDir) === null) return false;
  if (new Set(r.archived.map(x => x.from)).size !== r.archived.length) return false;
  return r.archived.every(x => x && r.sources.some(src => src.file === x.from) && sourceName(x.to)
    && x.to.startsWith(x.from + ".migrated")
    && (r.version === 2 || (x.sourceHash === r.sources.find(src => src.file === x.from)?.sha256 && ["planned", "done"].includes(x.state!))));
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
    return parsePinnedJsonItems(rawText, base);
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

function parsePinnedJsonItems(rawText: string, base: string): LegacyPinImportItem[] {
    let raw: any;
    try {
      raw = JSON.parse(rawText);
    } catch {
      throw migrationError("MIGRATION_SOURCE_UNREADABLE", `${base} is not valid JSON`);
    }
    if (!raw || typeof raw !== "object" || raw.version !== 1 || !Array.isArray(raw.items)) {
      throw migrationError("MIGRATION_SOURCE_UNREADABLE", `${base} has an unsupported or damaged shape`);
    }
    return raw.items.map((item: any) => {
      const content = String(item?.content ?? "").replace(/\r\n?/g, "\n").trim();
      // 旧运行时 serializeItems 对任一空内容项都会失败；坏项不能被过滤成删除语义。
      if (!content) throw migrationError("MIGRATION_SOURCE_UNREADABLE", `${base} contains an empty or invalid pinned item`);
      return {
        legacyId: typeof item?.id === "string" && item.id ? item.id : null,
        content,
        createdAt: typeof item?.createdAt === "string" ? item.createdAt : null,
      };
    });
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
  return `${base}.migrated-${sourceSha256}`;
}

// ── 主流程 ──────────────────────────────────────────────────────────────────

function failWithReceipt(agentDir: string, receipt: MigrationReceipt, code: string, message: string): void {
  if (fs.existsSync(receiptPath(agentDir))) {
    const directory = path.join(agentDir, "memory", BACKUP_DIR);
    fs.mkdirSync(directory, { recursive: true, mode: 0o700 });
    writePinnedBackup(receiptPath(agentDir), path.join(directory, `${sha256File(receiptPath(agentDir))}-receipt.json`));
  }
  receipt.state = code === "MIGRATION_VERIFY_FAILED" ? "conflict" : "failed";
  if (receipt.sources.length === 0) receipt.authority = null;
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
    operationId: randomUUID(),
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

/** 仅 pins 事务使用：独占创建并回读；同名备份必须逐字节摘要相同。 */
export function writePinnedBackup(sourcePath: string, destination: string): void {
  const expected = sha256File(sourcePath);
  try { fs.copyFileSync(sourcePath, destination, fs.constants.COPYFILE_EXCL); }
  catch (error) { if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error; }
  if (sha256File(destination) !== expected) throw migrationError("MIGRATION_BACKUP_FAILED", "backup digest mismatch");
}

function writeBackups(agentDir: string, receipt: MigrationReceipt, hooks?: MigrationFaultHooks): string {
  // 收据持久化用平台无关表示（C02）：不把本机 path.join 的结果写进收据。
  const relative = backupDirForReceipt(receipt.operationId!);
  const directory = receiptBackupDirLocalPath(agentDir, relative);
  fs.mkdirSync(directory, { recursive: true, mode: 0o700 });
  const files = receipt.sources.map(source => ({ source: path.join(agentDir, source.file), name: source.file }));
  if (receipt.target.existed) files.push({ source: tenetsFilePath(agentDir), name: "tenets.json" });
  if (fs.existsSync(receiptPath(agentDir))) files.push({ source: receiptPath(agentDir), name: "receipt.json" });
  for (const file of files) {
    hooks?.at?.(`backup:before:${file.name}`);
    writePinnedBackup(file.source, path.join(directory, `${sha256File(file.source)}-${file.name}`));
    hooks?.at?.(`backup:after:${file.name}`);
  }
  return relative;
}

/** 所有映射均证明 active、正文摘要、唯一 ID；新增项另证来源和原创建时间。 */
export function verifyPinnedTarget(targetPath: string, resultSha256: string, plan: MigrationReceipt["plan"]): { ok: boolean; reason: string; mismatches: string[] } {
  try {
    const data = readTenetsFileStrict(targetPath);
    const expected = new Map<string, string>();
    const mismatches: string[] = [];
    for (const entry of plan) {
      if (expected.has(entry.tenetId) && expected.get(entry.tenetId) !== entry.contentHash) mismatches.push(entry.tenetId);
      expected.set(entry.tenetId, entry.contentHash);
      const matches = data.tenets.filter(t => t.id === entry.tenetId);
      const target = matches[0];
      if (matches.length !== 1 || target.status !== "active"
        || `sha256:${sha256Text(legacyContentKey(target.content))}` !== entry.contentHash
        || (entry.outcome.startsWith("added") && (target.source !== "user_direct" || target.createdAt !== entry.createdAt))) mismatches.push(entry.tenetId);
    }
    // 空计划的证据只能是经过源计划验证的精确结果字节。
    if (plan.length === 0 && sha256File(targetPath) !== resultSha256) return { ok: false, reason: "empty_plan_target_changed", mismatches: [] };
    return { ok: mismatches.length === 0, reason: mismatches.length ? "planned_entries_changed" : "verified", mismatches };
  } catch { return { ok: false, reason: "invalid_target", mismatches: [] }; }
}

/** pins 迁移/批准恢复共享的局部写入顺序；不向其他存储推广。 */
export function executePinnedTargetTransaction(options: {
  targetPath: string;
  finalBytes: string;
  resultSha256: string;
  plan: MigrationReceipt["plan"];
  prepare: () => void;
  recheck: () => void;
  committing: () => void;
  committed: () => void;
  hooks?: MigrationFaultHooks;
}): void {
  readTenetsFileStrict(options.targetPath);
  options.prepare();
  options.hooks?.at?.("receipt:prepared");
  options.hooks?.at?.("recheck:before");
  options.recheck();
  options.hooks?.at?.("commit:before");
  // 先保存提交意图：之后即使目标被用户改回原字节，也不能据此重新导入。
  options.committing();
  options.hooks?.at?.("commit:intent:after");
  fs.mkdirSync(path.dirname(options.targetPath), { recursive: true });
  atomicWriteSync(options.targetPath, options.finalBytes);
  options.hooks?.at?.("commit:after");
  if (!verifyPinnedTarget(options.targetPath, options.resultSha256, options.plan).ok) {
    throw migrationError("MIGRATION_CONFLICT", "target verification failed after commit");
  }
  options.committed();
}

function verifyTargetCommitted(targetPath: string, resultSha256: string, plan: MigrationReceipt["plan"]): boolean {
  return verifyPinnedTarget(targetPath, resultSha256, plan).ok;
}

function archiveSources(agentDir: string, receipt: MigrationReceipt, hooks?: MigrationFaultHooks): void {
  for (const source of receipt.sources) {
    const entry = receipt.archived.find(a => a.from === source.file);
    if (!entry) throw migrationError("MIGRATION_CONFLICT", "archive destination not planned");
    const original = path.join(agentDir, source.file);
    const destination = path.join(agentDir, entry.to);
    const sourceExists = fs.existsSync(original);
    const destinationExists = fs.existsSync(destination);
    if ((sourceExists && sha256File(original) !== source.sha256)
      || (destinationExists && sha256File(destination) !== source.sha256)
      || (!sourceExists && !destinationExists)) {
      receipt.state = "conflict";
      receipt.error = { code: "MIGRATION_CONFLICT", message: `archive identity conflict: ${source.file}` };
      writeReceipt(agentDir, receipt);
      throw migrationError("MIGRATION_CONFLICT", receipt.error.message);
    }
    if (entry.state === "done" && !sourceExists) continue;
    hooks?.at?.(`archive:before:${source.file}`);
    if (!destinationExists) {
      // COPYFILE_EXCL 提供不覆盖保证；中断时源仍保留，重启按摘要恢复。
      fs.copyFileSync(original, destination, fs.constants.COPYFILE_EXCL);
      hooks?.at?.(`archive:copied:${source.file}`);
    }
    if (sha256File(destination) !== source.sha256) throw migrationError("MIGRATION_CONFLICT", "archive verification failed");
    if (sourceExists) {
      if (sha256File(original) !== source.sha256) throw migrationError("MIGRATION_CONFLICT", "archive source changed");
      fs.unlinkSync(original);
    }
    hooks?.at?.(`archive:after:${source.file}`);
    entry.state = "done";
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
  if (existing?.kind === "recovery") return; // 旧全局恢复收据不授权启动迁移。
  if (existing?.version === 2 && existing.state !== "failed") {
    if (!upgradeLegacyReceipt(agentDir, existing)) return;
  }
  if (existing && existing.state !== "failed") {
    resumeMigration(agentDir, existing, hooks);
    return;
  }
  runFreshMigration(agentDir, agentId, hooks, existing ?? undefined);
}

function upgradeLegacyReceipt(agentDir: string, receipt: MigrationReceipt): boolean {
  const conflict = () => {
    const directory = path.join(agentDir, "memory", BACKUP_DIR);
    fs.mkdirSync(directory, { recursive: true, mode: 0o700 });
    writePinnedBackup(receiptPath(agentDir), path.join(directory, `${sha256File(receiptPath(agentDir))}-receipt.json`));
    receipt.state = "conflict";
    receipt.error = { code: "MIGRATION_CONFLICT", message: "legacy receipt lacks unique source/plan evidence" };
    writeReceipt(agentDir, receipt);
    return false;
  };
  const paths = new Map<string, string>();
  const archived: MigrationReceipt["archived"] = [];
  for (const source of receipt.sources) {
    const original = path.join(agentDir, source.file);
    if (fs.existsSync(original)) {
      if (sha256File(original) !== source.sha256) return conflict();
      paths.set(source.file, original);
      archived.push({ from: source.file, to: archiveTargetName(agentDir, source.file, source.sha256), sourceHash: source.sha256, state: "planned" });
    } else {
      const candidates = fs.readdirSync(agentDir).filter(name => sourceName(name) && name.startsWith(source.file + ".migrated")
        && fs.statSync(path.join(agentDir, name)).isFile() && sha256File(path.join(agentDir, name)) === source.sha256);
      if (candidates.length !== 1) return conflict();
      paths.set(source.file, path.join(agentDir, candidates[0]));
      archived.push({ from: source.file, to: candidates[0], sourceHash: source.sha256, state: "done" });
    }
  }
  const authorityPath = receipt.authority && paths.get(receipt.authority.file);
  if (!authorityPath) return conflict();
  const items = readLegacyPinnedSource(authorityPath);
  if (items.length !== receipt.plan.length || items.some((item, i) => `sha256:${sha256Text(legacyContentKey(item.content))}` !== receipt.plan[i].contentHash)) return conflict();
  receipt.operationId = randomUUID();
  // 升级前保留旧收据、当前目标和所有来源的原始字节，不追认旧 completed。
  // 收据持久化用平台无关表示（C02）。
  const relative = backupDirForReceipt(receipt.operationId);
  const directory = receiptBackupDirLocalPath(agentDir, relative);
  fs.mkdirSync(directory, { recursive: true, mode: 0o700 });
  for (const [name, file] of [...paths.entries(), ["receipt.json", receiptPath(agentDir)] as const,
    ...(fs.existsSync(tenetsFilePath(agentDir)) ? [["tenets.json", tenetsFilePath(agentDir)] as const] : [])]) {
    writePinnedBackup(file, path.join(directory, `${sha256File(file)}-${name}`));
  }
  receipt.version = MIGRATION_VERSION;
  // 旧 prepared 可能已提交过；升级格式不产生“从未尝试提交”的新证据。
  if (receipt.state === "prepared") receipt.state = "committing";
  receipt.archived = archived; receipt.backupDir = relative;
  writeReceipt(agentDir, receipt);
  return true;
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

  executePinnedTargetTransaction({
    targetPath: prepared.targetPath, finalBytes, resultSha256: receipt.resultSha256!, plan: receipt.plan, hooks,
    prepare: () => {
      try { receipt.backupDir = writeBackups(agentDir, receipt, hooks); }
      catch { throw migrationError("MIGRATION_BACKUP_FAILED", "backup failed; original receipt and target preserved"); }
      receipt.archived = receipt.sources.map(source => ({ from: source.file, to: archiveTargetName(agentDir, source.file, source.sha256), sourceHash: source.sha256, state: "planned" }));
      receipt.state = "prepared";
      writeReceipt(agentDir, receipt);
    },
    recheck: () => assertUnchanged(agentDir, receipt, prepared.targetPath),
    committing: () => { receipt.state = "committing"; writeReceipt(agentDir, receipt); },
    committed: () => { receipt.state = "target_committed"; writeReceipt(agentDir, receipt); },
  });

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

/** 收据不能自报计划完整；用完整摘要定位的权威来源证明每一项。 */
function assertReceiptSourcePlan(agentDir: string, receipt: MigrationReceipt): void {
  const source = receipt.sources.find(item => item.file === receipt.authority?.file);
  if (!source) throw migrationError("MIGRATION_CONFLICT", "receipt source plan lacks authority");
  const candidates = [path.join(agentDir, source.file),
    ...receipt.archived.filter(item => item.from === source.file).map(item => path.join(agentDir, item.to)),
    ...(receipt.backupDir ? [path.join(receiptBackupDirLocalPath(agentDir, receipt.backupDir), `${source.sha256}-${source.file}`)] : [])];
  const file = candidates.find(candidate => fs.existsSync(candidate) && fs.statSync(candidate).isFile() && sha256File(candidate) === source.sha256);
  if (!file) throw migrationError("MIGRATION_CONFLICT", "receipt source plan cannot be verified");
  // 备份名包含摘要，解析仍沿原源的固定格式，不按备份文件名猜协议。
  const raw = fs.readFileSync(file, "utf8");
  const items = source.file.startsWith(PINNED_JSON)
    ? parsePinnedJsonItems(raw, source.file)
    : parsePinnedMarkdownItems(raw).map(content => ({ content, legacyId: null }));
  if (items.length !== receipt.plan.length || items.some((item, index) => {
    const entry = receipt.plan[index];
    const sourceTime = "createdAt" in item && typeof item.createdAt === "string" && Number.isFinite(Date.parse(item.createdAt))
      ? new Date(item.createdAt).toISOString() : null;
    return entry.contentHash !== `sha256:${sha256Text(legacyContentKey(item.content))}` || entry.legacyId !== item.legacyId
      || (entry.outcome.startsWith("added") && sourceTime !== null && entry.createdAt !== sourceTime);
  })) throw migrationError("MIGRATION_CONFLICT", "receipt source plan is incomplete or inconsistent");
}

function resumeMigration(agentDir: string, receipt: MigrationReceipt, hooks?: MigrationFaultHooks): void {
  // 旧收据的 Windows 反斜杠 backupDir：读取时兼容校验，这里在内存中转成规范
  // 形式——合法续写的收据持久化为统一 POSIX 路径，不为了换分隔符重跑迁移、
  // 也不移动既有备份（本地访问走 receiptBackupDirLocalPath 的兼容解析）。
  if (receipt.backupDir !== null) receipt.backupDir = parseReceiptBackupDir(receipt.backupDir);
  assertReceiptSourcePlan(agentDir, receipt);


  const targetPath = tenetsFilePath(agentDir);
  const targetExists = fs.existsSync(targetPath);
  const targetSha256 = targetExists ? sha256File(targetPath) : null;

  const commitUncertain = receipt.state === "committing" || (receipt.version < 4 && receipt.state === "prepared");
  if (commitUncertain || receipt.state === "target_committed" || receipt.state === "sources_archived") {
    // 已提交或曾尝试提交：只接受完整目标证明。原字节可能是用户删除后的结果。
    if (!targetExists || !verifyTargetCommitted(targetPath, receipt.resultSha256!, receipt.plan)) {
      receipt.state = "conflict";
      receipt.error = { code: "MIGRATION_CONFLICT", message: commitUncertain
        ? "commit outcome is unverified; automatic replay is not authorized"
        : "committed target no longer contains the planned entries" };
      writeReceipt(agentDir, receipt);
      return;
    }
    if (commitUncertain) {
      receipt.state = "target_committed";
      writeReceipt(agentDir, receipt);
    }
    archiveSources(agentDir, receipt, hooks);
    finalizeReceipt(agentDir, receipt, hooks);
    return;
  }

  // 仅 v4 prepared 证明尚未尝试提交；源必须全部在且字节一致，才能重新推导计划。
  for (const source of receipt.sources) {
    const filePath = path.join(agentDir, source.file);
    if (!fs.existsSync(filePath) || sha256File(filePath) !== source.sha256) {
      receipt.state = "conflict";
      receipt.error = { code: "MIGRATION_CONFLICT", message: `source ${source.file} changed or disappeared after prepare` };
      writeReceipt(agentDir, receipt);
      return;
    }
  }

  if (targetExists && targetSha256 === receipt.resultSha256 && verifyTargetCommitted(targetPath, receipt.resultSha256!, receipt.plan)) {
    // 当前目标已完整满足计划（例如仅有重复项）：只补收据与归档。
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

  executePinnedTargetTransaction({
    targetPath, finalBytes, resultSha256: receipt.resultSha256!, plan: receipt.plan, hooks,
    prepare: () => { /* prepared 与备份已在上次调用持久化。 */ },
    recheck: () => assertUnchanged(agentDir, receipt, targetPath),
    committing: () => { receipt.state = "committing"; writeReceipt(agentDir, receipt); },
    committed: () => {
      receipt.state = "target_committed";
      receipt.plan = receiptPlanEntries(entries);
      receipt.counts = planCounts(entries);
      writeReceipt(agentDir, receipt);
    },
  });

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
        const agentDir = path.join(agentsDir, entry.name);
        migrateAgentPinnedTenets(agentDir, entry.name);
        const receipt = readPinnedTenetsMigrationReceipt(agentDir);
        if (receipt?.kind === "migration" && ["failed", "conflict"].includes(receipt.state)) {
          log.warn(`[${entry.name}] pinned→tenets migration incomplete: ${receipt.state} (${receipt.error?.code ?? "MIGRATION_UNFINISHED"})`);
        }
      } catch (err: any) {
        log.warn(`[${entry.name}] migration failed: ${err?.message || err}`);
      }
    }
  } catch (err: any) {
    log.warn(`pinned→tenets migration scan failed: ${err?.message || err}`);
  }
}
