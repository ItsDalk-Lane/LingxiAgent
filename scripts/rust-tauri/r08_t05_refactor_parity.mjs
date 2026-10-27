#!/usr/bin/env node
// R08-T05（裁决点 P3 附加条件）：动词体提取的零行为变化对拍。
// 基线 = T04 Round B 全量导入（t04i-cli-import-08：435/435, issues 5206,
// source_root_sha256 v2 锚 40f42dc1…88c9e，已封存；v3 时代锚 80a8d2c2…feaa，见文末 §6 注）；候选 = 重构后二进制对同一源夹具的
// 全新导入。归一化规则（与 T04 assert 族同一约定）：
//   - run_id 形态 import:<srchash>:<ms> → 校验前缀后剥 ms；
//   - 一切 target 绝对路径 → <TARGET>；
//   - receipt 内时间戳字段（*At/*Ms/startedAt/completedAt/updatedAt）→ <TS>；
//   - run-bound 文件（runs.db*, migration-receipt.json）不做字节级对比，
//     改做内容断言（批次/issues/状态）+ 其余文件逐字节 sha256 对拍。
// 用法：node scripts/rust-tauri/r08_t05_refactor_parity.mjs <baselineTarget> <candidateTarget> <baselineOutcomeJsonLine> <candidateOutcomeJsonLine>
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync, statSync, existsSync } from 'node:fs';
import { join, relative } from 'node:path';
import { execFileSync } from 'node:child_process';

const [baselineTarget, candidateTarget, baselineOutcomeLine, candidateOutcomeLine] =
  process.argv.slice(2);
if (!baselineTarget || !candidateTarget || !baselineOutcomeLine || !candidateOutcomeLine) {
  console.error(
    'usage: r08_t05_refactor_parity.mjs <baselineTarget> <candidateTarget> <baselineOutcomeJsonLine> <candidateOutcomeJsonLine>',
  );
  process.exit(2);
}

let failures = 0;
const check = (name, ok, detail = '') => {
  console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ` :: ${detail}` : ''}`);
  if (!ok) failures += 1;
};

const sha256 = (buf) => createHash('sha256').update(buf).digest('hex');

function walkFiles(root) {
  const out = [];
  const walk = (dir) => {
    for (const ent of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, ent.name);
      if (ent.isDirectory()) walk(p);
      else if (ent.isFile()) out.push(relative(root, p));
    }
  };
  if (existsSync(root)) walk(root);
  return out.sort();
}

// run-bound：内容含 run_id/时间戳/receipt 路径，不做字节对拍
const RUN_BOUND = (rel) =>
  rel === 'data-epoch.json' || // updatedAt 时间戳；语义字段在 §5 对拍
  rel === 'lingxi-service/data/runs.db' ||
  rel.startsWith('lingxi-service/data/runs.db-') ||
  rel === 'legacy-import/migration-receipt.json' ||
  rel.startsWith('legacy-import/staging/');

function normalizeOutcome(raw, targetRoot) {
  const o = JSON.parse(raw);
  check('outcome.run_id shape', /^import:[0-9a-f]{8}:\d+$/.test(o.run_id), o.run_id);
  o.run_id = o.run_id.replace(/:\d+$/, ':<MS>');
  o.receipt_path = String(o.receipt_path).replaceAll(targetRoot, '<TARGET>');
  return o;
}

function normalizeReceipt(obj, targetRoot) {
  if (obj === null || typeof obj !== 'object') {
    if (typeof obj === 'string') return obj.replaceAll(targetRoot, '<TARGET>');
    return obj;
  }
  if (Array.isArray(obj)) return obj.map((v) => normalizeReceipt(v, targetRoot));
  const out = {};
  for (const [k, v] of Object.entries(obj)) {
    if (/^(runId|.*(At|Ms|Timestamp))$/.test(k) && (typeof v === 'number' || typeof v === 'string')) {
      out[k] = k === 'runId' && typeof v === 'string' ? v.replace(/:\d+$/, ':<MS>') : '<TS>';
    } else {
      out[k] = normalizeReceipt(v, targetRoot);
    }
  }
  return out;
}

// 1) outcome 行对拍
const bo = normalizeOutcome(baselineOutcomeLine, baselineTarget);
const co = normalizeOutcome(candidateOutcomeLine, candidateTarget);
check(
  'outcome normalized equal',
  JSON.stringify(bo) === JSON.stringify(co),
  JSON.stringify(co),
);

// 2) 目标树文件清单对拍。runs.db-shm/-wal 属连接生命周期产物：基线目标
//    在 import-08 后被 rerun-refuse 证据复跑重新打开过而残留 WAL，候选则
//    干净关闭——存在性差异非导入行为差异；库内容等价由 §5 的 dump 摘要证明。
const LIFECYCLE = (rel) =>
  rel === 'lingxi-service/data/runs.db-shm' || rel === 'lingxi-service/data/runs.db-wal';
const bf = walkFiles(baselineTarget).filter(
  (r) => !r.startsWith('legacy-import/staging/') && !LIFECYCLE(r),
);
const cf = walkFiles(candidateTarget).filter(
  (r) => !r.startsWith('legacy-import/staging/') && !LIFECYCLE(r),
);
const onlyB = bf.filter((r) => !cf.includes(r));
const onlyC = cf.filter((r) => !bf.includes(r));
check(
  'target file sets equal',
  onlyB.length === 0 && onlyC.length === 0,
  `onlyBaseline=${onlyB.slice(0, 5)} onlyCandidate=${onlyC.slice(0, 5)}`,
);

// 3) 非 run-bound 文件逐字节对拍；SUMMARY.md 走归一化对拍
//    （内含 target 绝对路径 / run_id / ISO 时间戳，归一化规则同 assert 族）
const normalizeText = (text, targetRoot) =>
  text
    .replaceAll(targetRoot, '<TARGET>')
    .replace(/import:[0-9a-f]{8}:\d+/g, 'import:<HASH>:<MS>')
    .replace(/\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?Z/g, '<TS>');
let digestMismatches = [];
for (const rel of cf) {
  if (RUN_BOUND(rel)) continue;
  const b = readFileSync(join(baselineTarget, rel));
  const c = readFileSync(join(candidateTarget, rel));
  if (rel === 'legacy-import/SUMMARY.md') {
    if (
      normalizeText(b.toString('utf8'), baselineTarget) !==
      normalizeText(c.toString('utf8'), candidateTarget)
    ) {
      digestMismatches.push(rel + ' (normalized)');
    }
    continue;
  }
  if (sha256(b) !== sha256(c)) digestMismatches.push(rel);
}
check(
  'non-run-bound files byte-identical',
  digestMismatches.length === 0,
  digestMismatches.slice(0, 8).join(','),
);

// 4) 收据归一化深对拍。唯一豁免键 phase：receipts.rs:159 的相位标签在
const br = normalizeReceipt(
  JSON.parse(readFileSync(join(baselineTarget, 'legacy-import/migration-receipt.json'), 'utf8')),
  baselineTarget,
);
const cr = normalizeReceipt(
  JSON.parse(readFileSync(join(candidateTarget, 'legacy-import/migration-receipt.json'), 'utf8')),
  candidateTarget,
);
//    Round B import-08 取证之后被有意修正（T04 收尾），候选应为新标签、
//    基线应为旧标签——两值各自钉死断言，其余键必须逐一相等。
const EXPECTED_PHASE_BASELINE =
  'R08-T04 B3 (slice A: sessionfiles convert; slice B: legacy trace + cron import; slice C: runtime-state registry; plugins/epoch/knowledge surfaces)';
const EXPECTED_PHASE_CANDIDATE =
  'R08-T04 (LegacyImporter: sessionfiles/trace/cron/runtime-state/plugins/epoch/knowledge; T05 adoption consumes this receipt)';
check(
  'receipt phase baseline = pre-fix label',
  br.phase === EXPECTED_PHASE_BASELINE,
  br.phase,
);
check(
  'receipt phase candidate = post-fix label',
  cr.phase === EXPECTED_PHASE_CANDIDATE,
  cr.phase,
);
const { phase: _bp, ...brRest } = br;
const { phase: _cp, ...crRest } = cr;
const brs = JSON.stringify(brRest, Object.keys(brRest).sort());
const crs = JSON.stringify(crRest, Object.keys(crRest).sort());
check('receipt normalized deep-equal (sans phase)', brs === crs, crs.slice(0, 400));
if (brs !== crs) {
  for (const k of new Set([...Object.keys(brRest), ...Object.keys(crRest)])) {
    if (JSON.stringify(brRest[k]) !== JSON.stringify(crRest[k])) {
      console.log(
        `  receipt key diff: ${k}\n    baseline=${JSON.stringify(brRest[k])?.slice(0, 300)}\n    candidate=${JSON.stringify(crRest[k])?.slice(0, 300)}`,
      );
    }
  }
}

// 5) runs.db 内容断言（批次 435 done / issues 5206 / 迁移 tip=28 / 运行态 completed）
//    + data-epoch.json stamp 语义字段对拍（updatedAt 归一化）
const q = (db, sql) =>
  execFileSync('sqlite3', [db, sql], { encoding: 'utf8' }).trim();
for (const [label, root] of [
  ['baseline', baselineTarget],
  ['candidate', candidateTarget],
]) {
  const db = join(root, 'lingxi-service/data/runs.db');
  const done = q(db, "SELECT COUNT(*) FROM import_batches WHERE state='done'");
  const total = q(db, 'SELECT COUNT(*) FROM import_batches');
  const issues = q(db, 'SELECT COUNT(*) FROM import_issues');
  const tip = q(db, 'SELECT MAX(version) FROM schema_migrations');
  const state = q(db, "SELECT state FROM import_runs");
  check(`${label} batches 435/435 done`, done === '435' && total === '435', `${done}/${total}`);
  check(`${label} issues 5206`, issues === '5206', issues);
  check(`${label} migrations tip 28`, tip === '28', tip);
  check(`${label} run state completed`, state === 'completed', state);
  const stamp = JSON.parse(readFileSync(join(root, 'data-epoch.json'), 'utf8'));
  check(
    `${label} stamp semantic fields`,
    stamp.epoch === 1 &&
      stamp.committedDataEpoch === 1 &&
      stamp.minimumReaderEpoch === 1 &&
      stamp.lastVersion === 'legacy-import' &&
      stamp.schemaVersion === 2,
    JSON.stringify(stamp),
  );
}

// 5b) runs.db 逻辑内容对拍：.dump 全文（WAL 由 sqlite 读取已提交态），
//     归一化：13 位 unix-ms → <MS>；target 绝对路径及其 macOS /private
//     规范形 → <TARGET>（session_files 等表落盘目标路径属 run-bound）。
const dumpDigest = (db, targetRoot) => {
  const dump = execFileSync('sqlite3', [db, '.dump'], { encoding: 'utf8', maxBuffer: 1 << 28 });
  const norm = dump
    .replaceAll(`/private${targetRoot}`, '<TARGET>')
    .replaceAll(targetRoot, '<TARGET>')
    .replace(/\d{13}/g, '<MS>')
    .split('\n')
    .sort()
    .join('\n');
  return sha256(Buffer.from(norm));
};
const bDump = dumpDigest(join(baselineTarget, 'lingxi-service/data/runs.db'), baselineTarget);
const cDump = dumpDigest(join(candidateTarget, 'lingxi-service/data/runs.db'), candidateTarget);
check('runs.db logical dump digest equal', bDump === cDump, `${bDump.slice(0, 16)} vs ${cDump.slice(0, 16)}`);

// 6) 源夹具零改动红线（bulk 源根哈希锚）
// 锚点演进：v2 锚 `40f42dc1…` 封存 t05-refactor-parity-01..04 证据链（v2 基座夹具）；
// v3 锚 `80a8d2c2…` 对应 2026-10-23 Lead 裁决的 v3 基座（knowledge.db 真 SQLite、
// mystery-unmapped.bin 移出基座）。两锚各自锚定各自时代，未来重跑须用 v3 基座。
const srcHash = co.source_root_sha256 || '';
check(
  'source root hash anchor v3 80a8d2c2',
  srcHash.startsWith('80a8d2c2'),
  srcHash,
);

console.log(failures === 0 ? 'PARITY OK' : `PARITY FAILURES: ${failures}`);
process.exit(failures === 0 ? 0 : 1);
