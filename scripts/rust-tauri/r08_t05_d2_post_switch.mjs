#!/usr/bin/env node
// R08-T05 D2 常量切换后端到端驱动（设计 §9.2/§10/§15；P8 批次）。
// 用法：node scripts/rust-tauri/r08_t05_d2_post_switch.mjs --bin <lingxi-service> --work <dir> --leg <name>
// 腿（每腿独立 scratch + 定种子 v3 夹具）：
//   full            全程跑通：migrate 无 --stop-after-phase → committed；裁决书双写；
//                   目标根 stamp epoch 2；旧根零写；指针有效。
//   resume-half     pointer_switch 干净停（e2e-05 已证上半）→ 同参续跑 → committed；
//                   journal ok 链单调无缺相；裁决书落盘。
//   kill-selfcheck  A09 selfcheck 断点腿（切换后批次）：相位前缘延迟钩子扩窗，
//                   轮询 journal 见 selfcheck 入场后 kill -9 → 同参续跑 → committed；
//                   无 refused、无双写者残留、旧根零写。
//   double-run      committed 后重跑 → 幂等 exit 0 + outcome=committed + target 不变。
// 公共断言：旧根逐字节零写（红线）；目标 stamp {minimumReaderEpoch:2,
//   committedDataEpoch:2}（R08_CUTOVER）；裁决书 schemaVersion=1 + cutoverId 回绑 +
//   strategy=separated-roots + sharedWrites=forbidden + enforcement×3 + 副本逐字节相等。
import { spawn, spawnSync } from 'node:child_process';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  statSync,
} from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';

const args = process.argv.slice(2);
function argValue(flag) {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : null;
}
const bin = argValue('--bin');
const workRoot = argValue('--work');
const leg = argValue('--leg');
if (!bin || !workRoot || !leg) {
  console.error('usage: r08_t05_d2_post_switch.mjs --bin <lingxi-service> --work <dir> --leg <name>');
  process.exit(2);
}
if (!['full', 'resume-half', 'kill-selfcheck', 'double-run'].includes(leg)) {
  console.error(`unknown leg: ${leg}`);
  process.exit(2);
}

mkdirSync(workRoot, { recursive: true });
const scratch = mkdtempSync(join(workRoot, `d2-${leg}-`));
const source = join(scratch, 'legacy-home');
const target = join(scratch, 'new-home');
const workDir = join(scratch, 'work');
const backupDir = join(scratch, 'backups');
const pointer = join(scratch, 'active-data-root.json');
const journalPath = join(workDir, 'journal.json');
const failures = [];
function check(name, ok, detail = '') {
  console.log(`${ok ? 'ok' : 'FAIL'} ${name}${ok ? '' : ` :: ${detail}`}`);
  if (!ok) failures.push(name);
}

// ── 树 hash（旧根零写红线） ─────────────────────────────────────────────
function hashTree(root) {
  const out = {};
  const walk = (dir, rel) => {
    for (const name of readdirSync(dir).sort()) {
      const abs = join(dir, name);
      const r = rel ? `${rel}/${name}` : name;
      const st = statSync(abs);
      if (st.isDirectory()) walk(abs, r);
      else if (st.isFile()) {
        out[r] = createHash('sha256').update(readFileSync(abs)).digest('hex');
      } else if (st.isSymbolicLink()) {
        out[r] = `symlink:${readFileSync(abs, 'utf8')}`;
      }
    }
  };
  walk(root, '');
  return out;
}
function treesEqual(a, b) {
  const ka = Object.keys(a);
  const kb = Object.keys(b);
  return ka.length === kb.length && ka.every((k) => a[k] === b[k]);
}

// ── v3 夹具（T04 生成器，定种子） ───────────────────────────────────────
mkdirSync(source, { recursive: true });
const gen = spawnSync(
  process.execPath,
  ['scripts/rust-tauri/r08_t04_fixture_gen.mjs', '--seed', '20261023', '--out', source],
  { encoding: 'utf8' },
);
if (gen.status !== 0) {
  console.error(`fixture gen failed: ${gen.stderr}`);
  process.exit(2);
}
const sourceBaseline = hashTree(source);

function migrateArgv(extra) {
  return [
    'migrate',
    '--source', source,
    '--target', target,
    '--work-dir', workDir,
    '--backup-dir', backupDir,
    '--pointer', pointer,
    ...extra,
  ];
}
function runMigrate(extra, env = {}) {
  return spawnSync(bin, migrateArgv(extra), {
    encoding: 'utf8',
    env: { ...process.env, ...env },
    timeout: 900000,
  });
}
function readJournal() {
  return JSON.parse(readFileSync(journalPath, 'utf8'));
}
const ALL_PHASES = [
  'preflight',
  'stop_write_probe',
  'quiesce_snapshot',
  'backup',
  'import',
  'import_verify',
  'epoch_adopt',
  'pointer_switch',
  'selfcheck',
  'committed',
];

// ── 终态公共断言 ─────────────────────────────────────────────────────────
function assertCommitted(label, run) {
  check(`${label}: exit 0`, run.status === 0, `status=${run.status} stderr=${run.stderr?.trim()}`);
  const line = (run.stdout ?? '').split('\n').find((l) => l.startsWith('CUTOVER_RESULT '));
  check(`${label}: CUTOVER_RESULT present`, Boolean(line), run.stdout?.trim());
  check(
    `${label}: outcome=committed phase=committed`,
    Boolean(line && line.includes('outcome=committed') && line.includes('phase=committed')),
    line,
  );
  const journal = readJournal();
  check(`${label}: journal.phase == committed`, journal.phase === 'committed', journal.phase);
  // 设计形状（T05_DESIGN §journal content + journal.rs advance 语义）：history 记录的是
  // 「离开」某相的事件；终态 committed 不被离开（advance out of terminal 被拒），故
  // history = 前 9 相 ok 依序 + journal.phase == committed（上条断言），无 committed 条目。
  // （红证据 t05-d2-02-full：原「ten phases」预期系本驱动预期错误，非编排器缺口。）
  const okPhases = (journal.history ?? []).filter((h) => h.outcome === 'ok').map((h) => h.phase);
  check(
    `${label}: nine phases ok in order + committed terminal state`,
    ALL_PHASES.slice(0, 9).every((p, i) => okPhases[i] === p) && okPhases.length === 9,
    JSON.stringify(okPhases),
  );
  check(
    `${label}: no refused entry`,
    !(journal.history ?? []).some((h) => h.outcome === 'refused') && journal.phase !== 'refused',
  );
  // 指针
  const ptr = existsSync(pointer) ? JSON.parse(readFileSync(pointer, 'utf8')) : null;
  check(`${label}: pointer valid`, Boolean(ptr) && ptr.activeDataRoot === realpathSync(target));
  // stamp：R08_CUTOVER 纪元 2
  const stampPath = join(target, 'data-epoch.json');
  const stamp = existsSync(stampPath) ? JSON.parse(readFileSync(stampPath, 'utf8')) : null;
  check(
    `${label}: target stamp epoch 2 (R08_CUTOVER)`,
    Boolean(stamp) && stamp.minimumReaderEpoch === 2 && stamp.committedDataEpoch === 2,
    JSON.stringify(stamp),
  );
  // 裁决书双写 + 内容
  const rulingWork = join(workDir, 'single-writer-ruling.json');
  const rulingCopy = join(target, 'legacy-import', 'single-writer-ruling.json');
  check(`${label}: ruling in work-dir`, existsSync(rulingWork));
  check(`${label}: ruling copy in target legacy-import/`, existsSync(rulingCopy));
  if (existsSync(rulingWork) && existsSync(rulingCopy)) {
    check(
      `${label}: ruling copy byte-equal`,
      readFileSync(rulingWork).equals(readFileSync(rulingCopy)),
    );
    const ruling = JSON.parse(readFileSync(rulingWork, 'utf8'));
    check(
      `${label}: ruling fields (schema/cutoverId/strategy/sharedWrites/enforcement×3)`,
      ruling.schemaVersion === 1
        && ruling.cutoverId === journal.cutoverId
        && ruling.strategy === 'separated-roots'
        && ruling.sharedWrites === 'forbidden'
        && Array.isArray(ruling.enforcement)
        && ruling.enforcement.length === 3,
      JSON.stringify(ruling).slice(0, 300),
    );
  }
  // 旧根零写红线
  check(
    `${label}: legacy root byte-equal (zero-mutation red line)`,
    treesEqual(sourceBaseline, hashTree(source)),
  );
}

function finish() {
  if (failures.length) {
    console.log(`SCRATCH_KEPT ${scratch}`);
    console.log(`D2_RESULT {"verdict":"FAIL","leg":"${leg}","failures":${JSON.stringify(failures)}}`);
    process.exit(1);
  }
  rmSync(scratch, { recursive: true, force: true });
  console.log(`D2_RESULT {"verdict":"PASS","leg":"${leg}"}`);
  process.exit(0);
}

// ── 腿 ──────────────────────────────────────────────────────────────────
if (leg === 'full') {
  const run = runMigrate([]);
  assertCommitted('full', run);
  finish();
}

if (leg === 'resume-half') {
  const first = runMigrate(['--stop-after-phase', 'pointer_switch']);
  check('stop-half: exit 0', first.status === 0, `status=${first.status}`);
  const j1 = existsSync(journalPath) ? readJournal() : null;
  check('stop-half: journal.phase == selfcheck', Boolean(j1) && j1.phase === 'selfcheck', j1?.phase);
  check(
    'stop-half: ruling absent before committed (§10)',
    !existsSync(join(workDir, 'single-writer-ruling.json')),
  );
  // §113：续跑必须显式 --resume <cutoverId>（不带时 journal 已存在响亮拒绝——防双跑）。
  const second = runMigrate(['--resume', j1.cutoverId]);
  assertCommitted('resume-half', second);
  // 续跑不得重复已 ok 的相位
  const j2 = readJournal();
  const okPhases = (j2.history ?? []).filter((h) => h.outcome === 'ok').map((h) => h.phase);
  const dupes = okPhases.filter((p, i) => okPhases.indexOf(p) !== i);
  check('resume-half: no duplicated ok phase', dupes.length === 0, JSON.stringify(okPhases));
  finish();
}

if (leg === 'kill-selfcheck') {
  const DELAY_MS = 2000;
  const child = spawn(bin, migrateArgv([]), {
    env: { ...process.env, LINGXI_CUTOVER_DELAY_BEFORE_PHASE_MS: String(DELAY_MS) },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let stdout = '';
  let stderr = '';
  child.stdout.on('data', (d) => (stdout += d));
  child.stderr.on('data', (d) => (stderr += d));
  // 轮询 journal 至 selfcheck 入场（in-progress 条目出现即相内）
  const deadline = Date.now() + 300000;
  let sawSelfcheck = false;
  while (Date.now() < deadline) {
    if (existsSync(journalPath)) {
      try {
        const j = readJournal();
        if ((j.history ?? []).some((h) => h.phase === 'selfcheck')) {
          sawSelfcheck = true;
          break;
        }
      } catch {
        // 半写 journal → 下轮重读
      }
    }
    await new Promise((r) => setTimeout(r, 50));
  }
  check('kill-selfcheck: selfcheck phase entered within deadline', sawSelfcheck);
  await new Promise((r) => setTimeout(r, 300)); // 落相内而非相前缘
  child.kill('SIGKILL');
  await new Promise((r) => setTimeout(r, 500));
  check('kill-selfcheck: no leftover process', child.exitCode !== 0);
  // 续跑（无延迟钩子；§113 显式 --resume 门）
  const jk = readJournal();
  const second = runMigrate(['--resume', jk.cutoverId]);
  assertCommitted('kill-selfcheck-resume', second);
  finish();
}

if (leg === 'double-run') {
  const first = runMigrate([]);
  assertCommitted('double-run:first', first);
  if (failures.length) finish();
  const targetAfterFirst = hashTree(target);
  const j1 = readJournal();
  // §113：不带 --resume 的重复执行必须响亮拒绝（防双跑守卫）
  const guarded = runMigrate([]);
  check(
    'double-run: rerun without --resume loudly refused (§113)',
    guarded.status !== 0
      && (guarded.stderr ?? '').includes('double-run guard')
      && (guarded.stderr ?? '').includes(j1.cutoverId),
    `status=${guarded.status} stderr=${guarded.stderr?.trim()}`,
  );
  // §114：带 --resume 且 phase=committed → 幂等空跑 exit 0 outcome=already-committed
  const second = runMigrate(['--resume', j1.cutoverId]);
  check(
    'double-run: --resume rerun exit 0 (already-committed idempotent, §114)',
    second.status === 0,
    `status=${second.status} stderr=${second.stderr?.trim()}`,
  );
  const line = (second.stdout ?? '').split('\n').find((l) => l.startsWith('CUTOVER_RESULT '));
  check(
    'double-run: --resume rerun outcome=already-committed',
    Boolean(line && line.includes('outcome=already-committed')),
    line,
  );
  check('double-run: target tree unchanged after reruns', treesEqual(targetAfterFirst, hashTree(target)));
  finish();
}
