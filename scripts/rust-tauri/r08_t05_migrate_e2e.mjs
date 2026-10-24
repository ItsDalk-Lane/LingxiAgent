#!/usr/bin/env node
// R08-T05 Phase E 端到端：migrate 动词编排至 pointer_switch（含双跑守卫/续跑守卫）。
// 断言（设计 §3/§9.2/§10）：
//   1. --stop-after-phase pointer_switch → exit 0，CUTOVER_RESULT outcome=stopped；
//   2. journal.json 相位 == "selfcheck"（pointer_switch 已推进），preflight..pointer_switch
//      八相 history 全部 outcome=ok 且顺序单调；import 书签 completed；adopt 书签 committed；
//   3. 指针文件 rename-原子提交：activeDataRoot == 目标根真实路径，cutoverId 回绑 journal；
//   4. 旧根零写红线：迁移前后源根逐字节一致（全树 sha256 清单比对）；
//   5. 目标库已导入：runs.db 存在且 user_version == 28（T04 锚点 tip）；
//   6. 备份根存在且 backup-manifest.json 落盘；工作目录无 .tmp-* 原子写残渣；
//   7. 裁决书仅在 committed 相写入 → 停在 pointer_switch 时必须缺席；
//   8. 双跑守卫：同 work-dir 不带 --resume 再跑 → exit 2；--resume 错 id → exit 2。
// 用法：node scripts/rust-tauri/r08_t05_migrate_e2e.mjs --bin <lingxi-service> --work <scratch> [--variant happy|unmapped]
//   --variant happy（默认）：v3 基座夹具，期望编排推进至 pointer_switch；
//   --variant unmapped：派生负路径变体（显式追加 mystery-unmapped.bin），期望 backup 相
//     fail-closed refuse exit 2 且 preflight-report/现场不劣化（v3 时代负路径重证）。
import { spawnSync } from 'node:child_process';
import {
  createHash,
} from 'node:crypto';
import {
  existsSync,
  lstatSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  readlinkSync,
  realpathSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';

const args = process.argv.slice(2);
function argValue(flag) {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : null;
}
const bin = argValue('--bin');
const workRoot = argValue('--work');
if (!bin || !workRoot) {
  console.error('usage: r08_t05_migrate_e2e.mjs --bin <lingxi-service> --work <scratch-dir>');
  process.exit(2);
}

const failures = [];
function check(name, condition, detail = '') {
  if (condition) {
    console.log(`ok   ${name}`);
  } else {
    failures.push(`${name}${detail ? ` — ${detail}` : ''}`);
    console.log(`FAIL ${name}${detail ? ` — ${detail}` : ''}`);
  }
}

function sha256(buf) {
  return createHash('sha256').update(buf).digest('hex');
}
// 全树清单：relpath -> sha256（符号链接记录目标串，不追随）。
function hashTree(root) {
  const entries = {};
  const walk = (dir, rel) => {
    for (const name of readdirSync(dir).sort()) {
      const abs = join(dir, name);
      const r = rel ? `${rel}/${name}` : name;
      const s = lstatSync(abs);
      if (s.isSymbolicLink()) {
        entries[r] = `symlink:${readlinkSync(abs)}`;
      } else if (s.isDirectory()) {
        walk(abs, r);
      } else if (s.isFile()) {
        entries[r] = sha256(readFileSync(abs));
      }
    }
  };
  walk(root, '');
  return entries;
}
function treesEqual(a, b) {
  const ka = Object.keys(a);
  const kb = Object.keys(b);
  if (ka.length !== kb.length) return false;
  return ka.every((k) => a[k] === b[k]);
}

const scratch = mkdtempSync(join(workRoot, 't05-e2e-'));
const source = join(scratch, 'legacy-home');
const target = join(scratch, 'new-home');
const workDir = join(scratch, 'work');
const backupDir = join(scratch, 'backups');
const pointer = join(scratch, 'active-data-root.json');

// 1) 夹具（T04 生成器，定种子，禁 Date.now）
const gen = spawnSync(
  process.execPath,
  ['scripts/rust-tauri/r08_t04_fixture_gen.mjs', '--seed', '20261023', '--out', source],
  { encoding: 'utf8' },
);
check('fixture-gen exit 0', gen.status === 0, gen.stderr?.trim());

// 1b) v3 夹具基座（Lead 裁决 2026-10-23）：生成器已写真实 SQLite knowledge.db，
// mystery-unmapped.bin 移出基座。负路径变体由 --variant unmapped 显式派生。
const variant = argValue('--variant') ?? 'happy';
if (!['happy', 'unmapped'].includes(variant)) {
  console.error(`unknown --variant ${variant} (happy|unmapped)`);
  process.exit(2);
}
if (variant === 'unmapped') {
  // 派生负路径变体：显式写入注册表外文件（与 v2 时代 mystery-unmapped.bin 同位同名）。
  writeFileSync(join(source, 'mystery-unmapped.bin'), Buffer.from('unmapped-v3-variant-20261023'));
  console.log('ok   variant=unmapped: mystery-unmapped.bin derived onto base fixture');
}

const beforeTree = hashTree(source);

// 2) migrate 编排至 pointer_switch 后干净停相
function runMigrate(extra) {
  return spawnSync(
    bin,
    [
      'migrate',
      '--source', source,
      '--target', target,
      '--work-dir', workDir,
      '--backup-dir', backupDir,
      '--pointer', pointer,
      ...extra,
    ],
    { encoding: 'utf8' },
  );
}
const first = runMigrate(['--stop-after-phase', 'pointer_switch']);

// 2u) 负路径变体（--variant unmapped）：backup 相 fail-closed refuse（v3 时代重证；
// v2 时代证据 t05-migrate-e2e-02 已封存）。断言拒绝签名 + 现场不劣化 + 旧根零写。
if (variant === 'unmapped') {
  const stderr = first.stderr ?? '';
  check('unmapped variant: migrate exit 2 (refuse)', first.status === 2, `status=${first.status} stderr=${stderr.trim()}`);
  check(
    'unmapped variant: refusal names backup + unclassified file',
    stderr.includes('refused [backup]')
      && stderr.includes('unclassified source file outside the 69-store registry')
      && stderr.includes('mystery-unmapped.bin'),
    stderr.trim(),
  );
  const ujPath = join(workDir, 'journal.json');
  check('unmapped variant: journal exists', existsSync(ujPath));
  const uj = existsSync(ujPath) ? JSON.parse(readFileSync(ujPath, 'utf8')) : null;
  if (uj) {
    check('unmapped variant: journal phase == refused', uj.phase === 'refused', `phase=${uj.phase}`);
    const okPhases = (uj.history ?? []).filter((h) => h.outcome === 'ok').map((h) => h.phase);
    check(
      'unmapped variant: preflight/stop_write_probe/quiesce_snapshot ok before refuse',
      ['preflight', 'stop_write_probe', 'quiesce_snapshot'].every((p, i) => okPhases[i] === p),
      JSON.stringify(okPhases),
    );
  }
  check('unmapped variant: target never created', !existsSync(target));
  check('unmapped variant: pointer never written', !existsSync(pointer));
  check(
    'unmapped variant: legacy root byte-equal (zero-mutation red line)',
    treesEqual(beforeTree, hashTree(source)),
  );
  finish();
}

function finish() {
  if (failures.length) {
    // 失败现场保留：journal/preflight-report/指针等取证物留在 scratch，路径打在 stdout。
    console.log(`SCRATCH_KEPT ${scratch}`);
    console.log(`E2E_RESULT {"verdict":"FAIL","variant":"${variant}","failures":${JSON.stringify(failures)}}`);
    process.exit(1);
  }
  rmSync(scratch, { recursive: true, force: true });
  console.log(`E2E_RESULT {"verdict":"PASS","variant":"${variant}"}`);
  process.exit(0);
}

check('migrate exit 0', first.status === 0, `status=${first.status} stderr=${first.stderr?.trim()}`);
const resultLine = (first.stdout ?? '').split('\n').find((l) => l.startsWith('CUTOVER_RESULT '));
check('CUTOVER_RESULT line present', Boolean(resultLine), first.stdout?.trim());
check(
  'outcome=stopped at boundary (phase=selfcheck, detail names pointer_switch)',
  Boolean(
    resultLine
      && resultLine.includes('outcome=stopped')
      && resultLine.includes('phase=selfcheck')
      && resultLine.includes('after phase pointer_switch'),
  ),
  resultLine,
);

// 3) journal 相位与历史
const journalPath = join(workDir, 'journal.json');
check('journal.json exists', existsSync(journalPath));
const journal = existsSync(journalPath) ? JSON.parse(readFileSync(journalPath, 'utf8')) : null;
const phasesThrough = [
  'preflight',
  'stop_write_probe',
  'quiesce_snapshot',
  'backup',
  'import',
  'import_verify',
  'epoch_adopt',
  'pointer_switch',
];
if (journal) {
  check('journal.phase == selfcheck', journal.phase === 'selfcheck', `phase=${journal.phase}`);
  const history = journal.history ?? [];
  const okPhases = history.filter((h) => h.outcome === 'ok').map((h) => h.phase);
  check(
    'eight phases ok in order',
    phasesThrough.every((p, i) => okPhases[i] === p),
    JSON.stringify(okPhases),
  );
  check('import bookmark completed', journal.import?.status === 'completed', JSON.stringify(journal.import));
  check('adopt bookmark committed', journal.adopt?.phase === 'committed', JSON.stringify(journal.adopt));
}

// 4) 指针读回
check('pointer file exists', existsSync(pointer));
const ptr = existsSync(pointer) ? JSON.parse(readFileSync(pointer, 'utf8')) : null;
if (ptr) {
  check(
    'pointer activeDataRoot == realpath(target)',
    ptr.activeDataRoot === realpathSync(target),
    `activeDataRoot=${ptr.activeDataRoot} realpath=${realpathSync(target)}`,
  );
  check('pointer cutoverId matches journal', Boolean(journal) && ptr.cutoverId === journal.cutoverId);
  check('pointer schemaVersion == 1', ptr.schemaVersion === 1);
}

// 5) 旧根零写红线
const afterTree = hashTree(source);
check(
  'legacy root byte-equal (zero-mutation red line)',
  treesEqual(beforeTree, afterTree),
  `before=${Object.keys(beforeTree).length} after=${Object.keys(afterTree).length}`,
);

// 6) 目标库导入核验
const runsDb = join(target, 'lingxi-service', 'data', 'runs.db');
check('target runs.db exists', existsSync(runsDb));
if (existsSync(runsDb)) {
  const db = new DatabaseSync(runsDb, { readOnly: true });
  const row = db.prepare('PRAGMA user_version').get();
  check('user_version == 28', Number(row.user_version) === 28, `user_version=${row.user_version}`);
  db.close();
}

// 7) 备份与原子写残渣
check('backup-manifest.json exists', existsSync(join(backupDir, 'backup-manifest.json'))
  || readdirSync(backupDir).some((n) => existsSync(join(backupDir, n, 'backup-manifest.json'))));
function noTmpResidue(root) {
  if (!existsSync(root)) return true;
  const stack = [root];
  while (stack.length) {
    const dir = stack.pop();
    for (const name of readdirSync(dir)) {
      if (name.startsWith('.tmp-')) return false;
      const abs = join(dir, name);
      if (statSync(abs).isDirectory()) stack.push(abs);
    }
  }
  return true;
}
check('no .tmp-* residue in work-dir', noTmpResidue(workDir));
check('no .tmp-* residue in backup-dir', noTmpResidue(backupDir));

// 8) 裁决书缺席（仅 committed 相写入）
check(
  'single-writer-ruling.json absent before committed',
  !existsSync(join(workDir, 'single-writer-ruling.json')),
);

// 9) 双跑守卫 / 续跑错 id
const doubleRun = runMigrate([]);
check(
  'double-run guard exit 2',
  doubleRun.status === 2
    && (doubleRun.stderr ?? '').includes('already exists')
    && (doubleRun.stderr ?? '').includes('pass --resume'),
  `status=${doubleRun.status} stderr=${doubleRun.stderr?.trim()}`,
);
const wrongResume = runMigrate(['--resume', 'cut-0000000000000-0']);
check(
  'resume wrong id exit 2',
  wrongResume.status === 2 && (wrongResume.stderr ?? '').includes('does not match'),
  `status=${wrongResume.status} stderr=${wrongResume.stderr?.trim()}`,
);
// 守卫失败后现场不劣化：journal 相位不变
const journalAfter = JSON.parse(readFileSync(journalPath, 'utf8'));
check('journal phase unchanged after guards', journalAfter.phase === 'selfcheck', journalAfter.phase);

finish();
