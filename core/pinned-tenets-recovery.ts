/** 旧 pins 归档的只读预览及明确批准恢复；不挂启动路径。 */
import fs from "node:fs";
import path from "node:path";
import { createHash, randomUUID } from "node:crypto";
import {
  PINNED_TENETS_MIGRATION_RECEIPT, readLegacyPinnedSource, readPinnedTenetsMigrationReceipt,
  sha256File, writePinnedBackup, verifyPinnedTarget, executePinnedTargetTransaction, isMigrationReceipt,
  type MigrationReceipt, type MigrationFaultHooks,
} from "./pinned-tenets-migration.ts";
import {
  dedupKey, legacyContentKey, planLegacyPinnedImport, readTenetsFileStrict, serializeTenetsFile, tenetsFilePath,
  type LegacyPinImportItem,
} from "../lib/memory/tenets.ts";
import { atomicWriteSync } from "../shared/safe-fs.ts";
import { backupDirForReceipt, parseReceiptBackupDir, receiptBackupDirLocalPath } from "./pinned-tenets-backup-dir.ts";

const ARCHIVED_SOURCE_RE = /^(pinned-memory\.json|pinned\.md)\.migrated(?:-[a-zA-Z0-9-]+)?$/;
const SAFE_ID = /^[a-zA-Z0-9][a-zA-Z0-9_-]*$/;
const HASH = /^[a-f0-9]{64}$/;
const OPERATIONS = "pinned-recovery-operations";
const digest = (value: string) => createHash("sha256").update(value).digest("hex");
const contentHash = (value: string) => `sha256:${digest(legacyContentKey(value))}`;

export interface RecoveryApproval {
  schemaVersion: number;
  operationId: string;
  agentId: string;
  sources: Array<{ file: string; sha256: string }>;
  /** null 表示批准时目标不存在；绝不按当前文件偷偷更新批准。 */
  observedTargetHash: string | null;
  decisions: Array<{ source: string; sourceEntryKey: string; contentHash: string; action: "restore" | "skip" }>;
}
export interface RecoveryCandidate {
  source: string;
  sourceEntryKey: string;
  legacyId: string | null;
  contentHash: string;
  /** 只供本地预览，不进入日志或收据。 */
  preview: string;
  classification: "present" | "inactive" | "similar" | "missing" | "previously_restored_now_missing";
  matchedTenetId?: string | null;
  legacyRestored?: boolean;
}
export interface RecoveryAgentReport {
  agentId: string;
  receiptState: string | null;
  diagnostic?: string;
  archivedSources: Array<{ file: string; sha256: string; itemCount: number }>;
  candidates: RecoveryCandidate[];
  recoverableCount: number;
  approvalTemplate: RecoveryApproval;
}
export interface RecoveryReport { home: string; agents: RecoveryAgentReport[] }
export interface RecoverySummary { restored: number; entries: MigrationReceipt["plan"] }
interface RecoveryOperation extends MigrationReceipt {
  approvalDigest: string;
  approval: RecoveryApproval;
  archiveStatus: "not_applicable";
  summary: RecoverySummary;
}
function fail(message: string): never { throw new Error(message); }
function currentTargetHash(dir: string): string | null {
  try { return sha256File(tenetsFilePath(dir)); }
  catch (error) { if ((error as NodeJS.ErrnoException).code === "ENOENT") return null; throw error; }
}
function sourceEntries(dir: string, source: { file: string; sha256: string }) {
  const items = readLegacyPinnedSource(path.join(dir, source.file));
  const counts = new Map<string, number>();
  for (const item of items) if (item.legacyId) counts.set(item.legacyId, (counts.get(item.legacyId) ?? 0) + 1);
  return items.map((item, ordinal) => ({ item, key: item.legacyId && counts.get(item.legacyId) === 1 ? item.legacyId : `${source.sha256}:${ordinal}` }));
}
function validatedApproval(input: unknown): RecoveryApproval {
  if (!input || typeof input !== "object") return fail("invalid approval");
  const a = input as RecoveryApproval;
  if (a.schemaVersion !== 1 || typeof a.operationId !== "string" || !SAFE_ID.test(a.operationId)
    || typeof a.agentId !== "string" || !SAFE_ID.test(a.agentId)
    || !(a.observedTargetHash === null || typeof a.observedTargetHash === "string" && HASH.test(a.observedTargetHash))
    || !Array.isArray(a.sources) || !a.sources.length || !Array.isArray(a.decisions) || !a.decisions.length) return fail("invalid approval schema/operation/snapshot");
  const sources = new Set<string>();
  for (const s of a.sources) {
    if (!s || typeof s.file !== "string" || !ARCHIVED_SOURCE_RE.test(s.file) || typeof s.sha256 !== "string" || !HASH.test(s.sha256) || sources.has(s.file)) return fail("invalid approval source");
    sources.add(s.file);
  }
  const decisions = new Set<string>();
  for (const d of a.decisions) {
    if (!d || !sources.has(d.source) || typeof d.sourceEntryKey !== "string" || !d.sourceEntryKey
      || typeof d.contentHash !== "string" || !/^sha256:[a-f0-9]{64}$/.test(d.contentHash) || !["restore", "skip"].includes(d.action)) return fail("invalid approval decision");
    const key = JSON.stringify([d.source, d.sourceEntryKey]);
    if (decisions.has(key)) return fail("duplicate or contradictory approval decision");
    decisions.add(key);
  }
  // 固定字段与排序使对象键序无关，决策变化则摘要必变。
  return { schemaVersion: 1, operationId: a.operationId, agentId: a.agentId, observedTargetHash: a.observedTargetHash,
    sources: a.sources.map(s => ({ file: s.file, sha256: s.sha256 })).sort((x,y) => x.file.localeCompare(y.file)),
    decisions: a.decisions.map(d => ({ source: d.source, sourceEntryKey: d.sourceEntryKey, contentHash: d.contentHash, action: d.action }))
      .sort((x,y) => x.source.localeCompare(y.source) || x.sourceEntryKey.localeCompare(y.sourceEntryKey)) };
}
function operationPath(dir: string, operationId: string): string { return path.join(dir, "memory", OPERATIONS, `${operationId}.json`); }
function readOperation(dir: string, operationId: string): RecoveryOperation | null {
  let raw: unknown;
  try { raw = JSON.parse(fs.readFileSync(operationPath(dir, operationId), "utf8")); }
  catch (error) { if ((error as NodeJS.ErrnoException).code === "ENOENT") return null; throw new Error("invalid or unreadable recovery operation receipt", { cause: error }); }
  if (!isMigrationReceipt(raw, path.basename(dir))) return fail("invalid recovery operation receipt");
  const r = raw as RecoveryOperation;
  if (![3, 4].includes(r.version) || r.kind !== "recovery" || r.operationId !== operationId || r.archiveStatus !== "not_applicable"
    || r.approvalDigest !== digest(JSON.stringify(validatedApproval(r.approval)))
    || !r.summary || !Number.isInteger(r.summary.restored) || r.summary.restored < 0 || JSON.stringify(r.summary.entries) !== JSON.stringify(r.plan)) return fail("invalid recovery operation proof");
  const decisions = r.approval.decisions.filter(d => d.action === "restore");
  if (decisions.length !== r.plan.length || r.plan.some((entry, index) => {
    const decision = decisions[index];
    return entry.source !== decision.source || entry.sourceEntryKey !== decision.sourceEntryKey || entry.contentHash !== decision.contentHash;
  }) || r.summary.restored !== r.plan.filter(p=>p.outcome.startsWith("added")).length) return fail("recovery receipt plan does not cover approval");
  return r;
}
function writeOperation(dir: string, receipt: RecoveryOperation) {
  receipt.updatedAt = new Date().toISOString();
  fs.mkdirSync(path.dirname(operationPath(dir, receipt.operationId!)), { recursive: true });
  atomicWriteSync(operationPath(dir, receipt.operationId!), JSON.stringify(receipt, null, 2) + "\n");
}
function completedOperations(dir: string): RecoveryOperation[] {
  const directory = path.join(dir, "memory", OPERATIONS);
  if (!fs.existsSync(directory)) return [];
  return fs.readdirSync(directory).filter(name => name.endsWith('.json')).map(name => {
    const id = name.slice(0, -5); if (!SAFE_ID.test(id)) return fail("invalid recovery operation filename");
    return readOperation(dir, id)!;
  }).filter(r => r.state === "completed");
}

/** dry-run 只读；completed 仅关闭其批准范围，不能隐藏整个 agent。 */
export function scanPinnedTenetsRecovery(home: string): RecoveryReport {
  const report: RecoveryReport = { home, agents: [] };
  const agentsDir = path.join(home, "agents");
  if (!fs.existsSync(agentsDir)) return report;
  for (const entry of fs.readdirSync(agentsDir, { withFileTypes: true })) {
    if (!entry.isDirectory() || !SAFE_ID.test(entry.name)) continue;
    const dir = path.join(agentsDir, entry.name);
    const legacy = readPinnedTenetsMigrationReceipt(dir);
    const archived = fs.readdirSync(dir).filter(name => ARCHIVED_SOURCE_RE.test(name)).sort();
    if (!archived.length) continue;
    const data = readTenetsFileStrict(tenetsFilePath(dir));
    const operations = completedOperations(dir);
    const template: RecoveryApproval = { schemaVersion: 1, operationId: randomUUID(), agentId: entry.name, sources: [], observedTargetHash: currentTargetHash(dir), decisions: [] };
    const agent: RecoveryAgentReport = { agentId: entry.name, receiptState: legacy?.state ?? null, archivedSources: [], candidates: [], recoverableCount: 0, approvalTemplate: template };
    if (legacy?.version === 2 && legacy.state === "completed") agent.diagnostic = "LEGACY_COMPLETION_UNVERIFIED";
    for (const file of archived) {
      const source = { file, sha256: sha256File(path.join(dir,file)) };
      const items = sourceEntries(dir,source);
      agent.archivedSources.push({ ...source, itemCount: items.length }); template.sources.push(source);
      for (const {item,key} of items) {
        const h = contentHash(item.content);
        const exact = data.tenets.find(t => t.status === "active" && legacyContentKey(t.content) === legacyContentKey(item.content))
          ?? data.tenets.find(t => legacyContentKey(t.content) === legacyContentKey(item.content));
        const wasRestored = operations.some(op => op.approval.sources.some(s=>s.file===file && s.sha256===source.sha256)
          && op.approval.decisions.some(d=>d.source===file && d.sourceEntryKey===key && d.contentHash===h && d.action==='restore'));
        const legacyRestored = legacy?.state === "completed" && legacy.sources.some(s=>s.sha256===source.sha256)
          && legacy.plan.some(p=>p.legacyId===item.legacyId && p.contentHash===h);
        let classification: RecoveryCandidate["classification"] = exact?.status === "active" ? "present" : exact ? "inactive" : data.tenets.some(t=>dedupKey(t.content)===dedupKey(item.content)) ? "similar" : "missing";
        if (classification !== "present" && (wasRestored || legacyRestored)) classification = "previously_restored_now_missing";
        agent.candidates.push({ source:file, sourceEntryKey:key, legacyId:item.legacyId, contentHash:h, preview:item.content.replace(/\s+/g,' ').slice(0,120), classification, matchedTenetId:exact?.id ?? null, legacyRestored:!!legacyRestored });
        template.decisions.push({source:file,sourceEntryKey:key,contentHash:h,action:"skip"});
      }
    }
    agent.recoverableCount = agent.candidates.filter(c=>c.classification==='missing'||c.classification==='inactive').length;
    report.agents.push(agent);
  }
  return report;
}

/** 只供持有 home 写入所有权的内部调用者或合成独占目录测试使用。CLI 尚无可复用所有权，拒绝 apply。 */
export function applyPinnedTenetsRecovery(home: string, input: unknown, hooks?: MigrationFaultHooks): RecoverySummary {
  const approval = validatedApproval(input);
  const dir = path.join(home, "agents", approval.agentId);
  if (!fs.statSync(dir).isDirectory()) return fail("agent not found");
  const approvalDigest = digest(JSON.stringify(approval));
  let receipt = readOperation(dir, approval.operationId);
  if (receipt) {
    // 旧操作收据的 Windows 反斜杠 backupDir：续写时在内存中转规范形式（C02）。
    if (receipt.backupDir !== null) receipt.backupDir = parseReceiptBackupDir(receipt.backupDir);
    if (receipt.approvalDigest !== approvalDigest) return fail("approval changed for existing operationId");
    if (receipt.state === 'completed') return receipt.summary;
    if (receipt.state === 'conflict' || receipt.state === 'failed') return fail("recovery operation conflict; new reviewed approval required");
  }
  const restore: LegacyPinImportItem[]=[];
  for (const source of approval.sources) {
    if (sha256File(path.join(dir, source.file)) !== source.sha256) return fail("stale_approval: source changed");
    const items = sourceEntries(dir,source);
    for (const decision of approval.decisions.filter(d=>d.source===source.file)) {
      const item = items.find(i=>i.key===decision.sourceEntryKey)?.item;
      if (!item || contentHash(item.content)!==decision.contentHash) return fail("stale_approval: source entry mismatch");
      if (decision.action === 'restore') restore.push(item);
    }
  }
  if (!restore.length) return fail("no restore decisions supplied");
  const targetPath = tenetsFilePath(dir);
  const targetHash = currentTargetHash(dir);
  if (receipt && (receipt.state !== 'prepared' || receipt.version < 4)) {
    // 已尝试提交或旧 prepared 都不能重执行；用户删回原字节仍然是后续修改。
    if (targetHash === null || !verifyPinnedTarget(targetPath, receipt.resultSha256!, receipt.plan).ok) {
      receipt.state = 'conflict';
      receipt.error = { code: 'MIGRATION_CONFLICT', message: 'recovery commit outcome is unverified; automatic replay is not authorized' };
      writeOperation(dir, receipt);
      return fail('recovery conflict: committed target changed or commit outcome is unverified');
    }
    receipt.state = 'completed'; receipt.completedAt = new Date().toISOString(); writeOperation(dir, receipt); return receipt.summary;
  }
  if (receipt && targetHash !== approval.observedTargetHash) {
    if (!verifyPinnedTarget(targetPath,receipt.resultSha256!,receipt.plan).ok) {
      receipt.state = 'conflict';
      receipt.error = { code: 'MIGRATION_CONFLICT', message: 'recovery target changed after preparation' };
      writeOperation(dir, receipt);
      return fail("recovery conflict: target changed after preparation");
    }
    receipt.state='completed'; receipt.completedAt=new Date().toISOString(); writeOperation(dir,receipt); return receipt.summary;
  }
  if (targetHash !== approval.observedTargetHash) return fail("stale_approval: target changed");
  const original = readTenetsFileStrict(targetPath);
  const {entries,finalTenets} = planLegacyPinnedImport(original,restore,receipt?.plan);
  const finalBytes = serializeTenetsFile({schemaVersion:original.schemaVersion,tenets:finalTenets});
  const resultSha256 = digest(finalBytes);
  if (receipt && receipt.resultSha256!==resultSha256) return fail("recovery conflict: prepared plan diverged");
  if (!receipt) {
    const approvedEntries = approval.decisions.filter(d=>d.action === "restore");
    const plan = entries.map(({normalizedContent:_body,...entry}, index)=>({...entry,exemption:'legacy_migration',source:approvedEntries[index].source,sourceEntryKey:approvedEntries[index].sourceEntryKey}));
    const now=new Date().toISOString();
    receipt={version:4,kind:'recovery',operationId:approval.operationId,agentId:approval.agentId,state:'prepared',
      sources:approval.sources.map(s=>({...s,mtimeMs:fs.statSync(path.join(dir,s.file)).mtimeMs})),authority:{file:approval.sources[0].file,reason:'explicit_recovery_approval'},
      target:{existed:targetHash!==null,sha256:targetHash},resultSha256,plan,
      counts:{sourceItems:plan.length,added:plan.filter(p=>p.outcome==='added').length,duplicateActive:plan.filter(p=>p.outcome==='duplicate_active').length,addedOverHistory:plan.filter(p=>p.outcome.startsWith('added_over')).length,duplicateInBatch:plan.filter(p=>p.outcome==='duplicate_in_batch').length},
      archived:[],archiveStatus:'not_applicable',backupDir:backupDirForReceipt(approval.operationId),error:null,createdAt:now,updatedAt:now,completedAt:null,
      approval,approvalDigest,summary:{restored:plan.filter(p=>p.outcome.startsWith('added')).length,entries:plan}};
  }
  const operation=receipt;
  executePinnedTargetTransaction({targetPath,finalBytes,resultSha256,plan:operation.plan,hooks,
    prepare:()=>{
      // 旧操作收据可能带 Windows 反斜杠 backupDir：本地访问走兼容解析（C02）。
      const backupDir=receiptBackupDirLocalPath(dir,operation.backupDir);fs.mkdirSync(backupDir,{recursive:true,mode:0o700});
      const files=approval.sources.map(s=>({file:path.join(dir,s.file),name:s.file}));
      if(targetHash!==null)files.push({file:targetPath,name:'tenets.json'});
      for(const name of [path.join('memory',PINNED_TENETS_MIGRATION_RECEIPT),path.join('memory',OPERATIONS,approval.operationId+'.json')]) {
        const file=path.join(dir,name); if(fs.existsSync(file))files.push({file,name:path.basename(file)});
      }
      for(const f of files){hooks?.at?.(`backup:before:${f.name}`);writePinnedBackup(f.file,path.join(backupDir,`${sha256File(f.file)}-${f.name}`));hooks?.at?.(`backup:after:${f.name}`);}
      writeOperation(dir,operation);
    },
    recheck:()=>{if(currentTargetHash(dir)!==approval.observedTargetHash||approval.sources.some(s=>sha256File(path.join(dir,s.file))!==s.sha256))return fail('stale_approval: preparation snapshot changed');},
    committing:()=>{operation.state='committing';writeOperation(dir,operation);},
    committed:()=>{operation.state='target_committed';writeOperation(dir,operation);},
  });
  hooks?.at?.('completed:before');operation.state='completed';operation.completedAt=new Date().toISOString();writeOperation(dir,operation);
  return operation.summary;
}
