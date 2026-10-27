#!/usr/bin/env node
// R08-T04 B4 收口断言：A08 双轮（--stop-after-batch 断点续跑 / kill -9 复核恢复）
// + 条件③ fail-closed 复证 + 收据终审（resumedFromRunId 链 / 源 manifest 逐字节）。
// 用法: node r08_t04_b4_assert.mjs <sourceDir> <targetDir> <baselineTargetDir|-> <mode> [expectedRootHash]
//   mode: stopped | resumed | killed
// 退出码 0 = 全过；任何断言失败响亮退出 1。
import { createRequire } from 'node:module';
import { readFileSync, existsSync } from 'node:fs';
import path from 'node:path';

const require = createRequire(import.meta.url);
const [source, target, baseline, mode, expectedRootHash] = process.argv.slice(2);
if (!source || !target || !baseline || !mode) {
  console.error('usage: r08_t04_b4_assert.mjs <sourceDir> <targetDir> <baselineTargetDir|-> <stopped|resumed|killed> [expectedRootHash]');
  process.exit(2);
}

let pass = 0;
const fails = [];
function ok(name, cond, detail = '') {
  if (cond) { pass++; console.log(`PASS ${name}`); }
  else { fails.push(name); console.log(`FAIL ${name} ${detail}`); }
}

const { DatabaseSync } = require('node:sqlite');
const db = new DatabaseSync(path.join(target, 'lingxi-service/data/runs.db'), { readOnly: true });
const one = (sql, ...args) => db.prepare(sql).get(...args);
const all = (sql, ...args) => db.prepare(sql).all(...args);

// ── 1. run 态 ─────────────────────────────────────────────────
const runs = all('SELECT run_id, state FROM import_runs');
ok('exactly one import run', runs.length === 1, JSON.stringify(runs));
const run = runs[0] || {};
const wantState = mode === 'stopped' ? 'interrupted' : 'completed';
ok(`run.state=${wantState}`, run.state === wantState, run.state);
const runId = run.run_id;

// ── 2. 批次账 ─────────────────────────────────────────────────
const bstates = all('SELECT state, COUNT(*) AS n FROM import_batches GROUP BY state');
const bmap = Object.fromEntries(bstates.map((r) => [r.state, r.n]));
const total = bstates.reduce((s, r) => s + r.n, 0);
if (mode === 'stopped') {
  ok('stopped: some done, some planned', (bmap.done ?? 0) > 0 && (bmap.planned ?? 0) > 0, JSON.stringify(bmap));
} else {
  ok('all batches terminal (done+skipped)', (bmap.done ?? 0) + (bmap.skipped ?? 0) === total && (bmap.planned ?? 0) === 0 && (bmap.in_progress ?? 0) === 0,
    JSON.stringify(bmap));
}

// ── 3. 收据终审 ───────────────────────────────────────────────
const receipt = JSON.parse(readFileSync(path.join(target, 'legacy-import/migration-receipt.json'), 'utf8'));
ok('receipt.runId matches run row', receipt.runId === runId, `${receipt.runId} vs ${runId}`);
if (mode === 'stopped') {
  ok('stopped: receipt.status=partial', receipt.status === 'partial', receipt.status);
  ok('stopped: no resumedFromRunId (first run)', !('resumedFromRunId' in receipt), JSON.stringify(Object.keys(receipt)));
  ok('stopped: resumed=false', receipt.resumed === false);
} else {
  ok('receipt.status=complete', receipt.status === 'complete', receipt.status);
  ok('receipt.pendingConvert=0', (receipt.batches?.pendingConvert ?? 0) === 0);
  ok('resumedFromRunId chain present', receipt.resumedFromRunId === runId, JSON.stringify(receipt.resumedFromRunId));
  ok('receipt.resumed=true', receipt.resumed === true);
}
ok('receipt.sourceUnchanged=true', receipt.sourceUnchanged === true);
if (expectedRootHash) {
  // importer 自身 manifest 算法的根哈希：跨轮（停止/续跑/kill 恢复）必须稳定。
  // v2 夹具锚定（40395742…）由生成侧证据 t04i-fixture-a08-02 承担。
  ok('receipt.sourceRootSha256 stable (importer algorithm)', receipt.sourceRootSha256 === expectedRootHash, receipt.sourceRootSha256);
}

// ── 4. 源 manifest 逐字节一致（哈希不变）─────────────────────────
const mfBefore = path.join(target, 'legacy-import/source-manifest.before.json');
const mfAfter = path.join(target, 'legacy-import/source-manifest.after.json');
ok('source manifest pair exists', existsSync(mfBefore) && existsSync(mfAfter));
if (existsSync(mfBefore) && existsSync(mfAfter)) {
  const a = readFileSync(mfBefore);
  const b = readFileSync(mfAfter);
  ok('source manifest before/after byte-equal', Buffer.compare(a, b) === 0, `before=${a.length}B after=${b.length}B`);
  const parsed = JSON.parse(b.toString('utf8'));
  ok('manifest covers fixture source files', Array.isArray(parsed) ? parsed.length > 0 : Object.keys(parsed).length > 0);
}

// ── 5. issues 清单：kill 复核必须留痕，非 kill 轮不得有 ─────────
const issuesText = readFileSync(path.join(target, 'legacy-import/issues.jsonl'), 'utf8');
const killIssues = issuesText.split('\n').filter((l) => l.includes('run-recovered-from-kill')).length;
if (mode === 'killed') {
  ok('kill recovery issue recorded exactly once', killIssues === 1, `count=${killIssues}`);
} else {
  ok('no kill-recovery issue in clean stop/resume round', killIssues === 0, `count=${killIssues}`);
}
const conflictIssues = issuesText.split('\n').filter((l) => l.includes('conflict') && l.includes('"severity":"error"')).length;
ok('no conflict errors (no duplicate/merge attempts)', conflictIssues === 0, `count=${conflictIssues}`);

// ── 6. 与全量基线逐表行级一致（无重复主键 / 无重复导入）──────────
if (baseline !== '-') {
  const bdb = new DatabaseSync(path.join(baseline, 'lingxi-service/data/runs.db'), { readOnly: true });
  // 全表归入归一化比较：导入时间列（committed/last_activity/pinned/created 于
  // legacy_unknown 会话均为导入时刻）天然 run-bound。detail_json 是批次执行诊断，
  // in_progress 批次重跑会改写 alreadyPresent 计数（设计允许的记账差异，幂等以数据表为准）。
  const tables = ['sessions', 'messages', 'session_files', 'session_file_refs',
    'model_call_usage', 'observability_legacy_trace', 'automation_jobs_v1',
    'legacy_runtime_state_registry', 'import_batches'];
  const stripCol = (c) => c === 'run_id' || c === 'detail_json' || c === 'legacy_file_paths_json'
    || c === 'archived_at' || /_unix_ms$/.test(c) || /_at_ms$/.test(c);
  const realTarget = (t) => { try { return require('node:fs').realpathSync(t); } catch { return t; } };
  const dump = (d, t, selfTarget) => {
    const cols = d.prepare(`SELECT name FROM pragma_table_info('${t}')`).all().map((r) => r.name)
      .filter((c) => !stripCol(c));
    let s = JSON.stringify(d.prepare(`SELECT ${cols.join(',')} FROM ${t} ORDER BY rowid`).all());
    if (selfTarget) {
      s = s.split(selfTarget).join('<TARGET>').split(realTarget(selfTarget)).join('<TARGET>');
    }
    return s;
  };
  for (const t of tables) {
    ok(`table ${t} content-identical to baseline (run/ts/path normalized)`,
      dump(db, t, path.resolve(target)) === dump(bdb, t, path.resolve(baseline)), t);
  }
  const bc = bdb.prepare('SELECT COUNT(*) AS n FROM import_batches').get().n;
  ok(`import_batches total=${bc} matches baseline`, total === bc, `${total} vs ${bc}`);
  bdb.close();

  // issues 多重集比较（seq 天然 run-bound）：剔除 kill 复核条目后必须与基线全等——
  // 证明 resume 未重复插入任何诊断（in_progress 批次重跑幂等）。
  const parseIssues = (p) => readFileSync(p, 'utf8').split('\n').filter(Boolean)
    .map((l) => { const o = JSON.parse(l); delete o.seq; delete o.runId; return JSON.stringify(o); });
  const baseIssues = parseIssues(path.join(baseline, 'legacy-import/issues.jsonl'));
  const selfIssues = parseIssues(path.join(target, 'legacy-import/issues.jsonl'))
    .filter((l) => !l.includes('run-recovered-from-kill'));
  const multiset = (arr) => { const m = new Map(); for (const x of arr) m.set(x, (m.get(x) ?? 0) + 1); return m; };
  const bm = multiset(baseIssues), sm = multiset(selfIssues);
  let issuesEqual = bm.size === sm.size;
  if (issuesEqual) for (const [k, n] of bm) if (sm.get(k) !== n) { issuesEqual = false; break; }
  ok('issues multiset identical to baseline (kill-recovery entry aside)', issuesEqual,
    `baseline=${baseIssues.length} self=${selfIssues.length}`);
}

// ── 7. 主键无重复（防御性独立断言）─────────────────────────────
const pkCols = all("SELECT name FROM pragma_table_info('messages') WHERE pk > 0 ORDER BY pk").map((r) => r.name);
if (pkCols.length > 0) {
  const dup = one(`SELECT COUNT(*) AS n FROM (SELECT ${pkCols.join(',')} FROM messages GROUP BY ${pkCols.join(',')} HAVING COUNT(*) > 1)`);
  ok('messages PK unique (no duplicates)', dup?.n === 0, JSON.stringify(dup));
}

console.log(`\nB4-ASSERT[${mode}] ${fails.length === 0 ? 'ALL PASS' : 'FAILURES'}: ${pass} pass, ${fails.length} fail`);
process.exit(fails.length === 0 ? 0 : 1);
