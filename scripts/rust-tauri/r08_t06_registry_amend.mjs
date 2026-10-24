#!/usr/bin/env node
// R08-T06 裁决②重证驱动：provider-state 增补 pattern `models-store.json`
// （classes.rs）后，夹具锚不受影响的确定性双跑重证。
//
// 两条腿（各双跑）：
//   腿1 v3 基座夹具（fixture_gen --seed 20261023，32 文件）：
//     - 双生成 rootHash 必须 == 80a8d2c2…feaa（v3 封存锚，t05-fixture-v3-01
//       同口径）——「锚 80a8d2c2 保持」的直接断言；
//     - 夹具无 models-store.json → 新 pattern 零命中 → 分类输出定义性不变；
//     - 双跑全量 legacy-import：outcome 计数/源根哈希全等 + 收据归一化深对拍。
//   腿2 v2 大规模源 /tmp/t04i-src-a08b（433 文件，T04 B4/T05 P3 封存链所用
//     435 批夹具，含全 69 类广度）：
//     - 无 models-store.json（零命中核对）；
//     - 双跑全量导入必须复现封存值 435/435 done + issues 5206 + Rust 侧
//       source_root_sha256 前缀 40f42dc1（v2 锚）——增补对既有分类面零扰动的
//       最强回归证据。
//
// 收据归一化规则与 r08_t05_refactor_parity.mjs 同源（run_id ms / 绝对路径 /
// 时间戳键）。用法：
//   node scripts/rust-tauri/r08_t06_registry_amend.mjs \
//     --bin <lingxi-service> --scratch <dir> [--bulk-source <dir>]
// 证据走 r08_run.py 包裹；scratch 用 /private/tmp 下全新目录。
import { execFileSync } from 'node:child_process';
import { readdirSync, readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';

const args = process.argv.slice(2);
const argValue = (flag) => {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : null;
};
const bin = argValue('--bin') ?? 'rust/target/debug/lingxi-service';
const scratch = argValue('--scratch');
const bulkSource = argValue('--bulk-source') ?? '/tmp/t04i-src-a08b';
if (!scratch) {
  console.error('usage: r08_t06_registry_amend.mjs --bin <lingxi-service> --scratch <dir> [--bulk-source <dir>]');
  process.exit(2);
}

const ANCHOR_V3 = '80a8d2c2640e2b2974b299a10f36679bc19daf0911e51f82f0c43399cb32feaa';
const SEALED_BULK = { batchesTotal: 435, batchesDone: 435, issues: 5206, anchorPrefix: '40f42dc1' };

let failures = 0;
const check = (name, ok, detail = '') => {
  console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ` :: ${detail}` : ''}`);
  if (!ok) failures += 1;
};

rmSync(scratch, { recursive: true, force: true });
execFileSync('mkdir', ['-p', scratch]);

function walkRel(root) {
  const out = [];
  const walk = (dir) => {
    for (const ent of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, ent.name);
      if (ent.isDirectory()) walk(p);
      else if (ent.isFile()) out.push(p.slice(root.length + 1));
    }
  };
  walk(root);
  return out.sort();
}

function runImport(source, target) {
  const stdout = execFileSync(bin, ['legacy-import', '--source', source, '--target', target], {
    encoding: 'utf8',
    maxBuffer: 1 << 26,
  });
  const jsonLines = stdout.trimEnd().split('\n').filter((l) => l.startsWith('{'));
  const outcome = JSON.parse(jsonLines[jsonLines.length - 1]);
  return { outcome, stdout };
}

const normalizeReceipt = (obj, targetRoot) => {
  if (obj === null || typeof obj !== 'object') {
    if (typeof obj === 'string') return obj.replaceAll(targetRoot, '<TARGET>');
    return obj;
  }
  if (Array.isArray(obj)) return obj.map((v) => normalizeReceipt(v, targetRoot));
  const out = {};
  for (const [k, v] of Object.entries(obj)) {
    if (/^(runId|run_id|.*(At|Ms|Timestamp))$/.test(k) && (typeof v === 'number' || typeof v === 'string')) {
      out[k] = /run/i.test(k) && typeof v === 'string' ? v.replace(/:\d+$/, ':<MS>') : '<TS>';
    } else {
      out[k] = normalizeReceipt(v, targetRoot);
    }
  }
  return out;
};

const receiptDeepEqual = (targetA, targetB) => {
  const ra = normalizeReceipt(JSON.parse(readFileSync(join(targetA, 'legacy-import/migration-receipt.json'), 'utf8')), targetA);
  const rb = normalizeReceipt(JSON.parse(readFileSync(join(targetB, 'legacy-import/migration-receipt.json'), 'utf8')), targetB);
  const ras = JSON.stringify(ra, Object.keys(ra).sort());
  const rbs = JSON.stringify(rb, Object.keys(rb).sort());
  if (ras !== rbs) {
    for (const k of new Set([...Object.keys(ra), ...Object.keys(rb)])) {
      if (JSON.stringify(ra[k]) !== JSON.stringify(rb[k])) console.log(`  receipt key diff: ${k}`);
    }
  }
  return ras === rbs;
};

// ── 腿1：v3 基座夹具 ─────────────────────────────────────────────
const rootHashes = [];
for (const leg of ['a', 'b']) {
  const stdout = execFileSync('node', [
    'scripts/rust-tauri/r08_t04_fixture_gen.mjs',
    '--seed', '20261023',
    '--out', join(scratch, `src-${leg}`),
  ], { encoding: 'utf8' });
  rootHashes.push(JSON.parse(stdout.trim().split('\n').filter((l) => l.startsWith('{')).pop()).rootHash);
}
check('v3 anchor 80a8d2c2…feaa preserved (gen-a)', rootHashes[0] === ANCHOR_V3, rootHashes[0]);
check('v3 anchor 80a8d2c2…feaa preserved (gen-b)', rootHashes[1] === ANCHOR_V3, rootHashes[1]);
const relA = walkRel(join(scratch, 'src-a'));
const relB = walkRel(join(scratch, 'src-b'));
check('v3 base fixture file sets identical', JSON.stringify(relA) === JSON.stringify(relB), `${relA.length} files`);
check('v3 base fixture contains no models-store.json (zero-hit)', !relA.includes('models-store.json'));

const baseRuns = [];
for (const leg of ['a', 'b']) {
  const target = join(scratch, `target-base-${leg}`);
  const { outcome } = runImport(join(scratch, `src-${leg}`), target);
  baseRuns.push({ outcome, target });
}
const [ba, bb] = baseRuns;
for (const [leg, r] of [['a', ba], ['b', bb]]) {
  check(`base run-${leg} status complete`, r.outcome.status === 'complete', r.outcome.status);
}
check('base runs outcome counts identical',
  ba.outcome.batches_total === bb.outcome.batches_total &&
  ba.outcome.batches_done === bb.outcome.batches_done &&
  ba.outcome.issues === bb.outcome.issues,
  `${ba.outcome.batches_total}/${ba.outcome.batches_done}/${ba.outcome.issues} vs ${bb.outcome.batches_total}/${bb.outcome.batches_done}/${bb.outcome.issues}`);
check('base runs source_root_sha256 identical', ba.outcome.source_root_sha256 === bb.outcome.source_root_sha256, ba.outcome.source_root_sha256);
check('base receipts normalized deep-equal', receiptDeepEqual(ba.target, bb.target));

// ── 腿2：v2 大规模源（435 批全分类广度）─────────────────────────
const bulkRel = walkRel(bulkSource);
check('bulk source contains no models-store.json (zero-hit)', !bulkRel.includes('models-store.json'), `${bulkRel.length} files`);
const bulkRuns = [];
for (const leg of ['a', 'b']) {
  const target = join(scratch, `target-bulk-${leg}`);
  const { outcome } = runImport(bulkSource, target);
  bulkRuns.push({ outcome, target });
}
const [ka, kb] = bulkRuns;
for (const [leg, r] of [['a', ka], ['b', kb]]) {
  check(`bulk run-${leg} sealed counts 435/435 + issues 5206`,
    r.outcome.batches_total === SEALED_BULK.batchesTotal &&
    r.outcome.batches_done === SEALED_BULK.batchesDone &&
    r.outcome.issues === SEALED_BULK.issues,
    `${r.outcome.batches_total}/${r.outcome.batches_done}/${r.outcome.issues}`);
  check(`bulk run-${leg} v2 anchor 40f42dc1 preserved`, r.outcome.source_root_sha256.startsWith(SEALED_BULK.anchorPrefix), r.outcome.source_root_sha256);
}
check('bulk runs source_root_sha256 identical', ka.outcome.source_root_sha256 === kb.outcome.source_root_sha256);
check('bulk receipts normalized deep-equal', receiptDeepEqual(ka.target, kb.target));

console.log(
  `REGISTRY_AMEND_RESULT v3Anchor=${rootHashes[0].slice(0, 16)} baseCounts=${ba.outcome.batches_total}/${ba.outcome.batches_done}/${ba.outcome.issues} bulkCounts=${ka.outcome.batches_total}/${ka.outcome.batches_done}/${ka.outcome.issues} bulkAnchor=${ka.outcome.source_root_sha256.slice(0, 8)} doubleRunIdentical=true zeroHit=true`,
);
console.log(failures === 0 ? 'REGISTRY AMEND OK' : `REGISTRY AMEND FAILURES: ${failures}`);
process.exit(failures === 0 ? 0 : 1);
