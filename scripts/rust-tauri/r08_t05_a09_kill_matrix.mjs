#!/usr/bin/env node
// R08-T05 A09 断点恢复矩阵驱动（设计 §11）。
// 用法：node scripts/rust-tauri/r08_t05_a09_kill_matrix.mjs --bin <lingxi-service> --work <dir> --scenario <name>
// 场景（同一 --work 目录跨调用共享 baseline.json；baseline 必须先跑）：
//   baseline                       干净跑 --stop-after-phase pointer_switch，落对拍基准
//   cleanstop:<phase>              相边界干净停 → --resume 完成 → 全断言
//   kill9:<phase>                  真实 kill -9 落相前缘（LINGXI_CUTOVER_DELAY_BEFORE_PHASE_MS
//                                  扩窗，编排器文档化测试钩子）→ --resume 完成 → 全断言
//   kill9:import:early|late        kill -9 落 convert 中段（baseline 校准 import 时长，
//                                  早 25% / 晚 75% 定时偏移）→ --resume → 全断言
//   control:stop-after-batch       直连 legacy-import --stop-after-batch 3 → --resume → 对拍
// 相位域（常量切换前）：preflight/backup/import/pointer_switch。
//   pointer_switch 干净停的 resume 半程与 selfcheck 断点 legs 归 D2 切换后批次
//   （epoch 门预切换拒绝已升 epoch=2 的目标根启动——门按设计工作）。
// 每轮断言（§11）：
//   ① --resume 续跑完成，journal history 链无跳相（首 ok 序单调，无 refused）；
//   ② 指针无半态（缺席或完整 JSON + activeDataRoot==realpath(target) + cutoverId 回绑；
//     指针目录无 .tmp-* 残渣）；
//   ③ 旧根 manifest 与切割前基线逐字节相等；
//   ④ 无双写者：kill 后 pgrep 无遗留进程；终态后重跑 legacy-import 探针 exit 2
//     （锁可获 + 门拒重跑）；
//   ⑤ 终态与 baseline 对拍：target 归一化 identity hash + runs.db 统计 +
//     issues 多重集 + adoption receipt（剥时间戳/runId/cutoverId）逐项相等。
import { spawn, spawnSync, execSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  readlinkSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { join } from 'node:path';

const args = process.argv.slice(2);
function argValue(flag) {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : null;
}
const bin = argValue('--bin');
const workRoot = argValue('--work');
const scenario = argValue('--scenario');
if (!bin || !workRoot || !scenario) {
  console.error(
    'usage: r08_t05_a09_kill_matrix.mjs --bin <lingxi-service> --work <dir> --scenario <name>',
  );
  process.exit(2);
}

const CUTOVER_PHASES = [
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
const TERMINAL = 'pointer_switch'; // D2 前缘：selfcheck/committed 切换后批次
const PRE_SWITCH_BPS = ['preflight', 'backup', 'import', 'pointer_switch'];
const DELAY_MS = 1200; // 相位前缘窗口（编排器测试钩子）
const KILL_LAND_MS = 600; // 窗口内落点

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
// 归一化 JSON：剥 run-bound 键（时间戳/runId/cutoverId/pid/时长），键序稳定，
// 字符串中的 scratch 绝对路径替换为 <SCRATCH>（receipt/sidecar/epoch 文书内嵌）。
function canon(value) {
  if (Array.isArray(value)) return value.map(canon);
  if (value && typeof value === 'object') {
    const out = {};
    for (const k of Object.keys(value).sort()) {
      // run-bound 键：时间戳/runId/cutoverId/pid/时长/清单绑定 hash
      // （manifest 内容含 createdAt → 其 hash 跨实例不可对拍；内容相等由
      //   identity hash + issues 多重集 + 旧根零写另行约束）
      if (/At$|^cutoverId$|^runId$|^pid$|^[Dd]uration|^generatedAtUnixMs$|^backupManifestId$|^stillpointManifestSha256$/.test(k)) continue;
      out[k] = canon(value[k]);
    }
    return out;
  }
  if (typeof value === 'string') {
    return value
      .split(scratch).join('<SCRATCH>')
      .replace(/cut-\d+-\d+/g, '<CUTOVER>')
      .replace(/import:[0-9a-f]+:\d+/g, '<RUN>');
  }
  return value;
}
const canonStr = (v) => JSON.stringify(canon(v));

const scratch = mkdtempSync(join(workRoot, `a09-${scenario.replaceAll(':', '_')}-`));
const source = join(scratch, 'legacy-home');
const target = join(scratch, 'new-home');
const workDir = join(scratch, 'work');
const backupDir = join(scratch, 'backups');
const pointer = join(scratch, 'active-data-root.json');
const journalPath = join(workDir, 'journal.json');

mkdirSync(source, { recursive: true });

// 夹具（T04 生成器，定种子，v3 基座）
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
    timeout: 600000,
  });
}
function readJournal() {
  return JSON.parse(readFileSync(journalPath, 'utf8'));
}
function sqlite(db, sql) {
  return execSync(`sqlite3 ${JSON.stringify(db)} ${JSON.stringify(sql)}`, { encoding: 'utf8' }).trim();
}

// ── 对拍快照 ─────────────────────────────────────────────────────────────
function targetSnapshot({ withEpoch = true, killRecovered = false, resumedRun = false } = {}) {
  const excluded = new Set([
    'lingxi-service/data/runs.db',
    'lingxi-service/data/runs.db-wal',
    'lingxi-service/data/runs.db-shm',
    'legacy-import/migration-receipt.json',
    'legacy-import/issues.jsonl',
    'legacy-import/SUMMARY.md',
    'legacy-import/adoption-receipt.json',
    'data-epoch.json',
    'data-epoch-transition.json',
  ]);
  const tree = hashTree(target);
  const identity = {};
  const sidecars = {};
  for (const [k, v] of Object.entries(tree)) {
    if (excluded.has(k)) continue;
    if (k.startsWith('legacy-import/staging/') && k.endsWith('.sidecar.json')) {
      // sidecar 内嵌绝对路径 + epoch-ms → 归一化语义对拍，不进字节 hash
      sidecars[k] = canonStr(JSON.parse(readFileSync(join(target, k), 'utf8')));
      continue;
    }
    identity[k] = v;
  }
  const db = join(target, 'lingxi-service/data/runs.db');
  const stats = {
    sessions: sqlite(db, 'SELECT COUNT(*) FROM sessions'),
    modelCallUsage: sqlite(db, 'SELECT COUNT(*) FROM model_call_usage'),
    cronJobs: sqlite(db, 'SELECT COUNT(*) FROM automation_jobs_v1 WHERE deleted_at IS NULL'),
    userVersion: sqlite(db, 'PRAGMA user_version'),
    batchesDone: sqlite(db, "SELECT COUNT(*) FROM import_batches WHERE state='done'"),
    batchesTotal: sqlite(db, 'SELECT COUNT(*) FROM import_batches'),
    issues: sqlite(db, 'SELECT COUNT(*) FROM import_issues'),
    runState: sqlite(db, 'SELECT state FROM import_runs'),
  };
  let issuesMultiset = sqlite(
    db,
    'SELECT severity, kind, COUNT(*) FROM import_issues GROUP BY 1,2 ORDER BY 1,2',
  );
  const receiptRaw = JSON.parse(
    readFileSync(join(target, 'legacy-import/migration-receipt.json'), 'utf8'),
  );
  if (killRecovered) {
    // 显式设计 delta（T04 中断恢复语义，非静默）：kill -9 后在途 run 被判
    // interrupted，续跑记 warning/run-recovered-from-kill ×1；receipt 记
    // resumed=true + resumedFromRunId。对拍前显式扣除该 delta。
    stats.issues = String(Number(stats.issues) - 1);
    issuesMultiset = issuesMultiset
      .split('\n')
      .filter((line) => line !== 'warning|run-recovered-from-kill|1')
      .join('\n');
    receiptRaw.issues.count -= 1;
  }
  if (killRecovered || resumedRun) {
    // 续跑记账键（任何 --resume 完成腿均出现；干净首跑恒 false/缺省）。
    receiptRaw.resumed = false;
    delete receiptRaw.resumedFromRunId;
  }
  const receipt = canonStr(receiptRaw);
  const snap = { identity, sidecars, stats, issuesMultiset, receipt };
  if (withEpoch) {
    snap.epochStamp = canonStr(JSON.parse(readFileSync(join(target, 'data-epoch.json'), 'utf8')));
    snap.epochTransition = canonStr(
      JSON.parse(readFileSync(join(target, 'data-epoch-transition.json'), 'utf8')),
    );
    snap.adoptionReceipt = canonStr(
      JSON.parse(readFileSync(join(target, 'legacy-import/adoption-receipt.json'), 'utf8')),
    );
  }
  return snap;
}

// ── §11 断言组 ───────────────────────────────────────────────────────────
function assertHistoryChain(label) {
  const j = readJournal();
  const okFirst = {};
  (j.history ?? []).forEach((h, i) => {
    if (h.outcome === 'ok' && !(h.phase in okFirst)) okFirst[h.phase] = i;
  });
  const expectedPrefix = CUTOVER_PHASES.slice(0, CUTOVER_PHASES.indexOf(TERMINAL) + 1);
  const seq = expectedPrefix.map((p) => okFirst[p]);
  check(
    `${label}: history ok 链覆盖 ${expectedPrefix[0]}..${TERMINAL} 且序单调（无跳相）`,
    seq.every((v) => v !== undefined) && seq.every((v, i) => i === 0 || v > seq[i - 1]),
    JSON.stringify({ okFirst, history: (j.history ?? []).map((h) => `${h.phase}:${h.outcome}`) }),
  );
  check(
    `${label}: history 无 refused 条目`,
    !(j.history ?? []).some((h) => h.outcome === 'refused'),
  );
  return j;
}
function assertPointer(label, cutoverId, { mustExist = true } = {}) {
  const tmpResidue = readdirSync(join(pointer, '..')).filter((n) => n.startsWith('.tmp-'));
  check(`${label}: 指针目录无 .tmp-* 残渣`, tmpResidue.length === 0, tmpResidue.join(','));
  if (!existsSync(pointer)) {
    check(`${label}: 指针${mustExist ? '存在' : '缺席（合法：switch 前无指针）'}`, !mustExist);
    return;
  }
  let p = null;
  try {
    p = JSON.parse(readFileSync(pointer, 'utf8'));
  } catch {
    p = null;
  }
  check(`${label}: 指针完整 JSON 且语义回绑`, !!p
    && p.activeDataRoot === realpathSync(target)
    && p.cutoverId === cutoverId, JSON.stringify(p));
}
function assertSourceIntact(label) {
  check(`${label}: 旧根逐字节零写`, treesEqual(sourceBaseline, hashTree(source)));
}
function assertNoDualWriter(label) {
  let orphans = '';
  try {
    orphans = execSync(`pgrep -fl ${JSON.stringify(bin)} || true`, { encoding: 'utf8' }).trim();
  } catch {
    orphans = '';
  }
  check(`${label}: 无遗留 ${bin} 进程`, orphans === '', orphans);
  const probe = spawnSync(bin, ['legacy-import', '--source', source, '--target', target], {
    encoding: 'utf8',
    timeout: 120000,
  });
  check(
    `${label}: 重跑探针 exit 2（目标锁可获 + 门拒重跑 = 无双写者）`,
    probe.status === 2 && (probe.stderr ?? '').includes('--resume'),
    `status=${probe.status} stderr=${(probe.stderr ?? '').trim().slice(0, 200)}`,
  );
}
function assertParity(label, baseline, { withEpoch = true, killRecovered = false } = {}) {
  const snap = targetSnapshot({ withEpoch, killRecovered });
  check(`${label}: identity hash 对拍`, canonStr(snap.identity) === canonStr(baseline.identity));
  check(`${label}: staging sidecar 归一化对拍`, canonStr(snap.sidecars) === canonStr(baseline.sidecars));
  check(`${label}: runs.db 统计对拍`, canonStr(snap.stats) === canonStr(baseline.stats),
    `${canonStr(snap.stats)} vs ${canonStr(baseline.stats)}`);
  check(`${label}: issues 多重集对拍`, snap.issuesMultiset === baseline.issuesMultiset);
  check(`${label}: migration receipt 归一化对拍`, snap.receipt === baseline.receipt);
  if (withEpoch) {
    check(`${label}: epoch stamp 归一化对拍`, snap.epochStamp === baseline.epochStamp);
    check(`${label}: epoch transition 归一化对拍`, snap.epochTransition === baseline.epochTransition);
    check(`${label}: adoption receipt 归一化对拍`, snap.adoptionReceipt === baseline.adoptionReceipt);
  }
}

function loadBaseline() {
  const p = join(workRoot, 'baseline.json');
  if (!existsSync(p)) {
    console.error('baseline.json missing — run --scenario baseline first in this --work dir');
    process.exit(2);
  }
  return JSON.parse(readFileSync(p, 'utf8'));
}

// resume 至终态（D2 前缘 = pointer_switch）
function resumeToTerminal(label) {
  const j = readJournal();
  const res = runMigrate(['--resume', j.cutoverId, '--stop-after-phase', TERMINAL]);
  check(
    `${label}: resume exit 0 且 stopped@${TERMINAL}`,
    res.status === 0
      && (res.stdout ?? '').includes(`outcome=stopped`)
      && (res.stdout ?? '').includes(`phase=selfcheck`),
    `status=${res.status} stdout=${(res.stdout ?? '').trim()} stderr=${(res.stderr ?? '').trim().slice(0, 300)}`,
  );
  return res;
}
function fullLegAssertions(label, baseline, { killRecovered = false } = {}) {
  const j = assertHistoryChain(label);
  assertPointer(label, j.cutoverId);
  assertSourceIntact(label);
  assertNoDualWriter(label);
  assertParity(label, baseline, { killRecovered });
}

// kill -9 落相前缘：等待 journal.phase==bp，窗口内 SIGKILL。
async function killAtPhase(bp) {
  const child = spawn(bin, migrateArgv(['--stop-after-phase', TERMINAL]), {
    env: { ...process.env, LINGXI_CUTOVER_DELAY_BEFORE_PHASE_MS: String(DELAY_MS) },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let stderr = '';
  child.stderr.on('data', (d) => (stderr += d));
  const deadline = Date.now() + 90000;
  let observed = null;
  while (Date.now() < deadline) {
    if (existsSync(journalPath)) {
      try {
        const j = readJournal();
        if (j.phase === bp) {
          observed = j;
          break;
        }
      } catch {
        // 原子写窗口内的半读——下一拍重试
      }
    }
    await new Promise((r) => setTimeout(r, 20));
  }
  if (!observed) {
    child.kill('SIGKILL');
    check(`kill9:${bp}: 观察窗内 journal.phase 到达 ${bp}`, false, `stderr=${stderr.slice(0, 300)}`);
    return null;
  }
  await new Promise((r) => setTimeout(r, KILL_LAND_MS));
  child.kill('SIGKILL');
  await new Promise((r) => child.once('exit', r));
  return observed;
}

// ── 场景 ────────────────────────────────────────────────────────────────
const [kind, a, b] = scenario.split(':');

if (kind === 'baseline') {
  const res = runMigrate(['--stop-after-phase', TERMINAL]);
  check('baseline: exit 0 stopped@pointer_switch', res.status === 0
    && (res.stdout ?? '').includes('outcome=stopped'), `status=${res.status}`);
  const j = readJournal();
  const importEntry = (j.history ?? []).find((h) => h.phase === 'import' && h.outcome === 'ok');
  const importDurationMs = importEntry
    ? Date.parse(importEntry.leftAt) - Date.parse(importEntry.enteredAt)
    : 0;
  const snap = targetSnapshot();
  snap.importDurationMs = importDurationMs;
  writeFileSync(join(workRoot, 'baseline.json'), JSON.stringify(snap, null, 2));
  check('baseline: import 时长校准 > 0', importDurationMs > 0, `${importDurationMs}ms`);
  console.log(`BASELINE importDurationMs=${importDurationMs} batches=${snap.stats.batchesDone}/${snap.stats.batchesTotal} issues=${snap.stats.issues}`);
} else if (kind === 'cleanstop') {
  const bp = a;
  if (!PRE_SWITCH_BPS.slice(0, 3).includes(bp)) {
    console.error(`pre-switch cleanstop 相位域 = preflight/backup/import（pointer_switch 干净停 resume 半程归 D2）: ${bp}`);
    process.exit(2);
  }
  const baseline = loadBaseline();
  const stop = runMigrate(['--stop-after-phase', bp]);
  const nextPhase = CUTOVER_PHASES[CUTOVER_PHASES.indexOf(bp) + 1];
  check(`cleanstop:${bp}: exit 0 outcome=stopped`, stop.status === 0
    && (stop.stdout ?? '').includes('outcome=stopped'), `status=${stop.status}`);
  const j0 = readJournal();
  check(`cleanstop:${bp}: journal 相位==${nextPhase}（下一待执行相）`, j0.phase === nextPhase, j0.phase);
  assertSourceIntact(`cleanstop:${bp}(停后)`);
  resumeToTerminal(`cleanstop:${bp}`);
  fullLegAssertions(`cleanstop:${bp}`, baseline);
} else if (kind === 'kill9' && (a !== 'import' || !b)) {
  const bp = a;
  if (!PRE_SWITCH_BPS.includes(bp)) {
    console.error(`pre-switch kill9 相位域: ${PRE_SWITCH_BPS.join('/')}: ${bp}`);
    process.exit(2);
  }
  const baseline = loadBaseline();
  const observed = await killAtPhase(bp);
  if (observed) {
    console.log(`JOURNAL_AT_KILL ${JSON.stringify(readJournal())}`);
    const jk = readJournal();
    check(`kill9:${bp}: 杀后 journal 相位==${bp}`, jk.phase === bp, jk.phase);
    check(`kill9:${bp}: 杀后无 refused 终态`, jk.phase !== 'refused');
    assertPointer(`kill9:${bp}(杀后)`, jk.cutoverId, { mustExist: false });
    assertSourceIntact(`kill9:${bp}(杀后)`);
    resumeToTerminal(`kill9:${bp}`);
    console.log(`JOURNAL_FINAL ${JSON.stringify(readJournal())}`);
    fullLegAssertions(`kill9:${bp}`, baseline);
  }
} else if (kind === 'kill9' && a === 'import' && b) {
  const timing = b; // early | late
  if (!['early', 'late'].includes(timing)) {
    console.error('kill9:import 需 :early|:late');
    process.exit(2);
  }
  const baseline = loadBaseline();
  const T = baseline.importDurationMs;
  const frac = timing === 'early' ? 0.25 : 0.75;
  const offset = DELAY_MS + Math.max(150, Math.round(T * frac));
  const child = spawn(bin, migrateArgv(['--stop-after-phase', TERMINAL]), {
    env: { ...process.env, LINGXI_CUTOVER_DELAY_BEFORE_PHASE_MS: String(DELAY_MS) },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  const deadline = Date.now() + 90000;
  let seen = false;
  while (Date.now() < deadline) {
    if (existsSync(journalPath)) {
      try {
        if (readJournal().phase === 'import') {
          seen = true;
          break;
        }
      } catch { /* 半读重试 */ }
    }
    await new Promise((r) => setTimeout(r, 20));
  }
  check(`kill9:import:${timing}: 观察到相=import`, seen);
  if (seen) {
    await new Promise((r) => setTimeout(r, offset));
    child.kill('SIGKILL');
    await new Promise((r) => child.once('exit', r));
    const jk = readJournal();
    console.log(`JOURNAL_AT_KILL ${JSON.stringify(jk)}`);
    check(`kill9:import:${timing}: 杀后相=import 且无书签`, jk.phase === 'import' && !jk.import,
      `phase=${jk.phase} import=${JSON.stringify(jk.import)}`);
    // 中段证据：runs.db 恰一 run 行 state=running，批次部分完成
    const db = join(target, 'lingxi-service/data/runs.db');
    check(`kill9:import:${timing}: runs.db 已建（导入已入中段）`, existsSync(db));
    if (existsSync(db)) {
      const runRow = sqlite(db, 'SELECT run_id || "|" || state FROM import_runs');
      const rows = runRow.split('\n').filter(Boolean);
      check(`kill9:import:${timing}: 恰一 run 行且 state=running`, rows.length === 1
        && rows[0].endsWith('|running'), runRow);
      const done = Number(sqlite(db, "SELECT COUNT(*) FROM import_batches WHERE state='done'"));
      const total = Number(baseline.stats.batchesTotal);
      const lo = timing === 'early' ? 0 : Math.floor(total * 0.3);
      const hi = timing === 'early' ? Math.ceil(total * 0.6) : total - 1;
      console.log(`MID_CONVERT_LANDING ${timing} done=${done}/${total}`);
      check(`kill9:import:${timing}: 批次部分完成 ${timing === 'early' ? '≤60%' : '≥30%'} 且 <total`,
        done >= 1 && done <= hi && done >= lo && done < total, `done=${done}/${total}`);
    }
    assertSourceIntact(`kill9:import:${timing}(杀后)`);
    resumeToTerminal(`kill9:import:${timing}`);
    console.log(`JOURNAL_FINAL ${JSON.stringify(readJournal())}`);
    fullLegAssertions(`kill9:import:${timing}`, baseline, { killRecovered: true });
  }
} else if (kind === 'control' && a === 'stop-after-batch') {
  const baseline = loadBaseline();
  const first = spawnSync(bin, ['legacy-import', '--source', source, '--target', target,
    '--stop-after-batch', '3'], { encoding: 'utf8', timeout: 600000 });
  check('control: 首跑 exit 0 且 interrupted', first.status === 0
    && (first.stdout ?? '').includes('"interrupted": true'),
    `status=${first.status} stdout=${(first.stdout ?? '').slice(0, 200)}`);
  let runId = null;
  try {
    const j = JSON.parse(first.stdout ?? '{}');
    runId = j.runId ?? j.run_id;
  } catch { /* 下方断言 */ }
  check('control: runId 可解析', typeof runId === 'string' && runId.length > 0, `${runId}`);
  if (runId) {
    const second = spawnSync(bin, ['legacy-import', '--source', source, '--target', target,
      '--resume', runId], { encoding: 'utf8', timeout: 600000 });
    check('control: 续跑 exit 0 且 complete', second.status === 0
      && (second.stdout ?? '').includes('"status":"complete"'),
      `status=${second.status} stdout=${(second.stdout ?? '').slice(0, 200)}`);
    // 对拍（无 epoch 文件：控制腿不经过 epoch_adopt）
    const snap = targetSnapshot({ withEpoch: false, resumedRun: true });
    check('control: identity hash 对拍', canonStr(snap.identity) === canonStr(baseline.identity));
    check('control: staging sidecar 归一化对拍', canonStr(snap.sidecars) === canonStr(baseline.sidecars));
    check('control: runs.db 统计对拍', canonStr(snap.stats) === canonStr(baseline.stats),
      `${canonStr(snap.stats)} vs ${canonStr(baseline.stats)}`);
    check('control: issues 多重集对拍', snap.issuesMultiset === baseline.issuesMultiset);
    check('control: migration receipt 归一化对拍', snap.receipt === baseline.receipt);
    assertSourceIntact('control');
  }
} else {
  console.error(`unknown scenario ${scenario}`);
  process.exit(2);
}

if (failures.length) {
  console.log(`SCRATCH_KEPT ${scratch}`);
  console.log(`A09_RESULT {"verdict":"FAIL","scenario":"${scenario}","failures":${JSON.stringify(failures)}}`);
  process.exit(1);
}
rmSync(scratch, { recursive: true, force: true });
console.log(`A09_RESULT {"verdict":"PASS","scenario":"${scenario}"}`);
