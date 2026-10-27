#!/usr/bin/env node
// R08-T04 B3 收口断言：trace / cron / runtime-state / epoch / plugins /
// knowledge 六面扩展 + V28 登记 + 逐字节归档 + 收据形状。
// 用法: node r08_t04_b3_assert.mjs <sourceDir> <targetDir>
// 退出码 0 = 全过；任何断言失败响亮退出 1。
import { createRequire } from 'node:module';
import { readFileSync, existsSync, readdirSync, statSync } from 'node:fs';
import { createHash } from 'node:crypto';
import path from 'node:path';

const require = createRequire(import.meta.url);
const [source, target] = process.argv.slice(2);
if (!source || !target) {
  console.error('usage: r08_t04_b3_assert.mjs <sourceDir> <targetDir>');
  process.exit(2);
}

let pass = 0;
const fails = [];
function ok(name, cond, detail = '') {
  if (cond) { pass++; console.log(`PASS ${name}`); }
  else { fails.push(name); console.log(`FAIL ${name} ${detail}`); }
}
function sha256(file) {
  return createHash('sha256').update(readFileSync(file)).digest('hex');
}

// better-sqlite3 不在根依赖？用 node:sqlite（Node 24 内置，稳定 API）。
const { DatabaseSync } = require('node:sqlite');
const db = new DatabaseSync(path.join(target, 'lingxi-service/data/runs.db'), { readOnly: true });

const one = (sql, ...args) => db.prepare(sql).get(...args);
const all = (sql, ...args) => db.prepare(sql).all(...args);

// ── 1. 收据形状 ────────────────────────────────────────────────
const receipt = JSON.parse(readFileSync(path.join(target, 'legacy-import/migration-receipt.json'), 'utf8'));
ok('receipt.status=complete', receipt.status === 'complete', receipt.status);
ok('receipt.pendingConvert=0', (receipt.batches?.pendingConvert ?? receipt.pendingConvert) === 0);
const ext = receipt.extensions || {};
ok('receipt.extensions 6 keys', ['epoch','knowledgeDb','legacyCron','legacyPlugins','legacyTrace','runtimeState']
  .every((k) => k in ext), Object.keys(ext).join(','));

// ── 2. 迁移 tip=28 ────────────────────────────────────────────
const ver = one('PRAGMA user_version');
ok('migrations tip=28', (ver?.user_version ?? ver?.['user_version']) === 28,
  JSON.stringify(ver));

// ── 3. legacyTrace 面 ─────────────────────────────────────────
const traceExt = ext.legacyTrace;
ok('legacyTrace.importedCalls=5', traceExt?.importedCalls === 5, JSON.stringify(traceExt).slice(0, 120));
ok('legacyTrace.failedStores=false', traceExt?.failedStores === false);
const usageCount = one('SELECT COUNT(*) AS n FROM model_call_usage');
ok('model_call_usage rows=5', usageCount?.n === 5, JSON.stringify(usageCount));
// 父子调用关系可还原：observability_legacy_trace（R07-T10 权威链）c1.parent=p1
const rel = one(`SELECT parent_call_id AS p FROM observability_legacy_trace
                 WHERE model_call_id = 'call-legacy-c1'`);
ok('trace parent-child preserved (observability_legacy_trace)', rel?.p === 'call-legacy-p1', JSON.stringify(rel));
const traceRowCount = one('SELECT COUNT(*) AS n FROM observability_legacy_trace');
ok('observability_legacy_trace rows=4', traceRowCount?.n === 4, JSON.stringify(traceRowCount));

// ── 4. legacyCron 面 ──────────────────────────────────────────
const cronExt = ext.legacyCron;
ok('legacyCron.imported=3', cronExt?.imported === 3, JSON.stringify(cronExt).slice(0, 120));
ok('legacyCron.blocked=0', cronExt?.blocked === 0);
const jobCount = one('SELECT COUNT(*) AS n FROM automation_jobs_v1');
ok('automation_jobs_v1 rows=3', jobCount?.n === 3, JSON.stringify(jobCount));

// ── 5. runtimeState 面：V28 登记 + 逐字节归档 ──────────────────
const regRows = all('SELECT store_class, entry_key, source_relpath, source_sha256, archived_relpath FROM legacy_runtime_state_registry');
ok('registry rows=10 across 8 classes',
  regRows.length === 10 && new Set(regRows.map((r) => r.store_class)).size === 8,
  `rows=${regRows.length} classes=${new Set(regRows.map((r) => r.store_class)).size}`);
let archiveOk = true;
for (const r of regRows) {
  const srcFile = path.join(source, r.source_relpath);
  const arcFile = path.join(target, 'legacy-import/runtime-state-archive', r.source_relpath);
  if (!existsSync(arcFile)) { archiveOk = false; console.log(`  archive missing: ${r.source_relpath}`); continue; }
  const srcSha = sha256(srcFile);
  if (srcSha !== r.source_sha256) { archiveOk = false; console.log(`  sha mismatch: ${r.source_relpath}`); }
  if (sha256(arcFile) !== srcSha) { archiveOk = false; console.log(`  archive bytes differ: ${r.source_relpath}`); }
}
ok('registry sha matches source AND archive byte-equal (10/10)', archiveOk);
ok('runtimeState ext label', String(ext.runtimeState?.label ?? '').includes('待对应任务') ||
  JSON.stringify(ext.runtimeState).includes('已登记'), JSON.stringify(ext.runtimeState).slice(0, 160));

// ── 6. epoch 面 ───────────────────────────────────────────────
const stamp = JSON.parse(readFileSync(path.join(target, 'data-epoch.json'), 'utf8'));
ok('epoch stamp=1', stamp.epoch === 1 && stamp.minimumReaderEpoch === 1 && stamp.committedDataEpoch === 1,
  JSON.stringify(stamp));
ok('epoch ext written', ext.epoch?.stamp === 1 && ext.epoch?.written === true);

// ── 7. plugins / knowledge 面（如实记录）────────────────────────
ok('legacyPlugins ext present', Array.isArray(ext.legacyPlugins?.detected));
ok('knowledgeDb ext present', typeof ext.knowledgeDb?.present === 'boolean');

// ── 8. 源哈希与问题清单 ────────────────────────────────────────
ok('receipt.sourceUnchanged=true', receipt.sourceUnchanged === true || receipt.source_unchanged === true);

console.log(`\nB3-ASSERT ${fails.length === 0 ? 'ALL PASS' : 'FAILURES'}: ${pass} pass, ${fails.length} fail`);
process.exit(fails.length === 0 ? 0 : 1);
