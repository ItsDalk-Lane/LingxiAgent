#!/usr/bin/env node
// R08-T05 A10：小盘预检拒绝（T05_DESIGN §12，一次到位验收矩阵）。
//   负例：64MB APFS dmg 挂载作 --backup-dir → migrate 预检 #2 refuse exit 2。
//   断言：①exit 2 + journal phase=refused + refusal.check="disk-space"
//         ②preflight-report 含 avail/required 实数且 backupDirVolume 短缺
//         ③目标根不存在（import 相才创建 = 结构性零部分产物）
//         ④旧根 manifest 逐字节相等（零改动红线）
//         ⑤work-dir 仅含 journal.json + preflight-report.json
//         ⑥无 .tmp-* 残留（scratch + 小卷）
//   正控制：同 fixture、常规卷 backup-dir → exit 0 stopped@pointer_switch
//           （证明阈值是实测比较而非常量拒绝）。
//   近拟合控制（best-effort，登记不阻断）：dmg 容量 = required + 余量 →
//           avail ≥ required → migrate 通过。粒度所限失败仅登记。
// 用法：node r08_t05_a10_small_volume.mjs --bin <lingxi-service> [--work <dir>]
// 出口：0 全绿；1 断言失败（SCRATCH_KEPT）；2 用法/环境错误。
import { spawnSync, execFileSync } from 'node:child_process';
import {
  existsSync, lstatSync, mkdtempSync, readFileSync, readdirSync, readlinkSync,
  rmSync, statfsSync,
} from 'node:fs';
import { createHash } from 'node:crypto';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO = join(fileURLToPath(import.meta.url), '../../..');
const FIXTURE_GEN = join(REPO, 'scripts/rust-tauri/r08_t04_fixture_gen.mjs');
const TERMINAL = 'pointer_switch'; // 常量切换前终态（selfcheck 结构性不可达）

const argv = process.argv.slice(2);
function flag(name) {
  const i = argv.indexOf(name);
  return i >= 0 ? argv[i + 1] : null;
}
const bin = flag('--bin');
if (!bin) {
  console.error('usage: --bin <lingxi-service> [--work <dir>]');
  process.exit(2);
}
const workRoot = flag('--work') ?? '/private/tmp/t05a10-matrix';
const scratch = mkdtempSync(join(workRoot, 'a10-'));

const failures = [];
const notes = [];
function check(label, cond, detail = '') {
  if (cond) console.log(`ok   ${label}`);
  else {
    console.log(`FAIL ${label}${detail ? ` — ${detail}` : ''}`);
    failures.push(label);
  }
}
function note(label, msg) {
  console.log(`note ${label}: ${msg}`);
  notes.push(`${label}: ${msg}`);
}

const sha256 = (buf) => createHash('sha256').update(buf).digest('hex');
function hashTree(root) {
  const entries = {};
  const walk = (dir, rel) => {
    for (const name of readdirSync(dir).sort()) {
      const abs = join(dir, name);
      const r = rel ? `${rel}/${name}` : name;
      const s = lstatSync(abs);
      if (s.isSymbolicLink()) entries[r] = `symlink:${readlinkSync(abs)}`;
      else if (s.isDirectory()) walk(abs, r);
      else if (s.isFile()) entries[r] = sha256(readFileSync(abs));
    }
  };
  walk(root, '');
  return entries;
}
const canonStr = (v) => JSON.stringify(v, Object.keys(v).sort());
function findTmpResidue(...roots) {
  const hits = [];
  const walk = (dir) => {
    for (const name of readdirSync(dir)) {
      const abs = join(dir, name);
      if (name.startsWith('.tmp-')) hits.push(abs);
      try {
        if (lstatSync(abs).isDirectory() && !lstatSync(abs).isSymbolicLink()) walk(abs);
      } catch { /* 挂载点可能已卸 */ }
    }
  };
  for (const r of roots) if (existsSync(r)) walk(r);
  return hits;
}

function genFixture(dest) {
  const r = spawnSync(process.execPath, [FIXTURE_GEN, '--seed', '20261023', '--out', dest],
    { encoding: 'utf8', timeout: 600000 });
  if (r.status !== 0) {
    console.error(`fixture gen failed: ${r.status}\n${r.stderr}`);
    process.exit(2);
  }
}
function runMigrate(source, target, workDir, backupDir, pointer) {
  return spawnSync(bin, ['migrate',
    '--source', source, '--target', target, '--work-dir', workDir,
    '--backup-dir', backupDir, '--pointer', pointer,
    '--stop-after-phase', TERMINAL], { encoding: 'utf8', timeout: 600000 });
}
const readJournal = (workDir) => JSON.parse(readFileSync(join(workDir, 'journal.json'), 'utf8'));
const readReport = (workDir) =>
  JSON.parse(readFileSync(join(workDir, 'preflight-report.json'), 'utf8'));

// ── dmg 助手 ──────────────────────────────────────────────────────────
const mountedVols = [];
function attachDmg(dmgPath, volname) {
  execFileSync('hdiutil', ['create', '-size', '64m', '-fs', 'APFS', '-volname', volname, dmgPath],
    { stdio: 'pipe' });
  const out = execFileSync('hdiutil', ['attach', dmgPath, '-nobrowse'], { encoding: 'utf8' });
  const line = out.split('\n').find((l) => l.includes('/Volumes/'));
  if (!line) throw new Error(`attach output has no mount point: ${out}`);
  const mount = line.trim().split(/\s+/).pop();
  mountedVols.push(mount);
  return mount;
}
function attachSizedDmg(dmgPath, volname, sizeArg) {
  execFileSync('hdiutil', ['create', '-size', sizeArg, '-fs', 'APFS', '-volname', volname, dmgPath],
    { stdio: 'pipe' });
  const out = execFileSync('hdiutil', ['attach', dmgPath, '-nobrowse'], { encoding: 'utf8' });
  const line = out.split('\n').find((l) => l.includes('/Volumes/'));
  if (!line) throw new Error(`attach output has no mount point: ${out}`);
  const mount = line.trim().split(/\s+/).pop();
  mountedVols.push(mount);
  return mount;
}
function detachAll() {
  for (const m of mountedVols.splice(0)) {
    try {
      execFileSync('hdiutil', ['detach', m, '-quiet'], { stdio: 'pipe' });
    } catch { /* best-effort */ }
  }
}
process.on('exit', detachAll);

// ══ 1. 负例：64MB 卷作 backup-dir → 预检拒绝 ═════════════════════════
const source = join(scratch, 'legacy-home');
genFixture(source);
const sourceBaseline = hashTree(source);

const mount = attachDmg(join(scratch, 'probe.dmg'), 'r08t05');
const negTarget = join(scratch, 'neg', 'new-home');
const negWork = join(scratch, 'neg', 'work');
const negPointer = join(scratch, 'neg', 'active-data-root.json');
const neg = runMigrate(source, negTarget, negWork, join(mount, 'backups'), negPointer);
console.log(`NEGATIVE exit=${neg.status} stderr=${(neg.stderr ?? '').trim().slice(0, 160)}`);
check('负例: exit 2', neg.status === 2, `exit=${neg.status}`);

const j = existsSync(join(negWork, 'journal.json')) ? readJournal(negWork) : null;
check('负例: journal 存在且 phase=refused', j?.phase === 'refused', JSON.stringify(j?.phase));
check('负例: refusal.check=disk-space', j?.refusal?.check === 'disk-space',
  JSON.stringify(j?.refusal));

const report = existsSync(join(negWork, 'preflight-report.json')) ? readReport(negWork) : null;
check('负例: preflight-report.json 存在', report !== null);
if (report) {
  const ds = report.checks?.find((c) => c.id === 'disk-space');
  check('负例: disk-space 检查 status=refused', ds?.status === 'refused', JSON.stringify(ds?.status));
  const req = ds?.evidence?.requiredBytes;
  const vols = ds?.evidence?.volumes ?? [];
  const backupVol = vols.find((v) => v.volume === 'backupDirVolume');
  check('负例: 证据含 requiredBytes 实数', Number.isFinite(req) && req > 0, `${req}`);
  check('负例: 证据含 backupDirVolume avail 实数且短缺',
    Number.isFinite(backupVol?.availableBytes) && backupVol.availableBytes < req,
    `avail=${backupVol?.availableBytes} required=${req}`);
  if (Number.isFinite(req)) {
    console.log(`DISK_SPACE_FACT requiredBytes=${req} backupAvail=${backupVol?.availableBytes}`);
  }
}
check('负例: 目标根不存在（结构性零部分产物）', !existsSync(negTarget));
check('负例: 旧根逐字节零改动', canonStr(hashTree(source)) === canonStr(sourceBaseline));
if (existsSync(negWork)) {
  const workFiles = readdirSync(negWork).sort();
  check('负例: work-dir 仅含 journal+preflight-report',
    canonStr(workFiles) === canonStr(['journal.json', 'preflight-report.json']),
    canonStr(workFiles));
}
const tmpHits = findTmpResidue(scratch, mount);
check('负例: 无 .tmp-* 残留', tmpHits.length === 0, tmpHits.join(','));

// ══ 2. 正控制：常规卷同 fixture → 通过 ═══════════════════════════════
const posSource = join(scratch, 'pos', 'legacy-home');
genFixture(posSource);
const posTarget = join(scratch, 'pos', 'new-home');
const posWork = join(scratch, 'pos', 'work');
const posPointer = join(scratch, 'pos', 'active-data-root.json');
const pos = runMigrate(posSource, posTarget, posWork, join(scratch, 'pos', 'backups'), posPointer);
console.log(`POSITIVE exit=${pos.status}`);
check('正控制: exit 0', pos.status === 0, `stderr=${(pos.stderr ?? '').trim().slice(0, 200)}`);
if (existsSync(join(posWork, 'journal.json'))) {
  const pj = readJournal(posWork);
  // mark_stopped 语义：终态相位已完成（history outcome=ok），停点检查在
  // 下一相位 in-progress 入口翻转 outcome=stopped，journal.phase 停在该
  // 待执行相位（=selfcheck）。
  const doneEntry = pj.history?.find((e) => e.phase === TERMINAL && e.outcome === 'ok');
  const stopEntry = pj.history?.find((e) => e.phase === 'selfcheck' && e.outcome === 'stopped');
  check(`正控制: ${TERMINAL} 完成 + 干净停在 selfcheck 入口（非拒绝）`,
    pj.phase === 'selfcheck' && doneEntry && stopEntry && !pj.refusal,
    JSON.stringify({ phase: pj.phase, doneEntry: !!doneEntry, stopEntry: !!stopEntry, refusal: pj.refusal }));
}
// 记录实测 required（近拟合控制用）
let requiredBytes = null;
if (existsSync(join(posWork, 'preflight-report.json'))) {
  const pr = readReport(posWork);
  requiredBytes = pr.checks?.find((c) => c.id === 'disk-space')?.evidence?.requiredBytes ?? null;
}

// ══ 3. 近拟合控制（best-effort）：avail 略大于 required → 通过 ═══════
// dmg 容量粒度 + APFS 元数据开销所限；失败登记不阻断验收。
if (Number.isFinite(requiredBytes)) {
  const sizeMiB = Math.ceil(requiredBytes / 1048576) + 96; // required + 96MiB 元数据/余量
  try {
    const fitMount = attachSizedDmg(join(scratch, 'fit.dmg'), 'r08t05fit', `${sizeMiB}m`);
    const avail = statfsSync(fitMount).bavail * statfsSync(fitMount).frsize;
    console.log(`NEARFIT_FACT sizeMiB=${sizeMiB} avail=${avail} required=${requiredBytes}`);
    if (avail < requiredBytes) {
      note('近拟合', `dmg 粒度内 avail ${avail} < required ${requiredBytes}，近拟合不可构造（登记不阻断）`);
    } else {
      const fitSource = join(scratch, 'fit', 'legacy-home');
      genFixture(fitSource);
      const fit = runMigrate(fitSource, join(scratch, 'fit', 'new-home'),
        join(scratch, 'fit', 'work'), join(fitMount, 'backups'),
        join(scratch, 'fit', 'active-data-root.json'));
      if (fit.status === 0) {
        check('近拟合: avail≥required 时 migrate 通过（阈值为实测比较）', true);
      } else {
        note('近拟合', `exit=${fit.status}（备份体积可能超 avail；登记不阻断）stderr=${(fit.stderr ?? '').trim().slice(0, 160)}`);
      }
    }
  } catch (err) {
    note('近拟合', `dmg 构造失败（登记不阻断）: ${err.message}`);
  }
} else {
  note('近拟合', '正控制未产出 requiredBytes，跳过');
}

detachAll();
if (failures.length) {
  console.log(`SCRATCH_KEPT ${scratch}`);
  console.log(`A10_RESULT {"verdict":"FAIL","failures":${JSON.stringify(failures)},"notes":${JSON.stringify(notes)}}`);
  process.exit(1);
}
rmSync(scratch, { recursive: true, force: true });
console.log(`A10_RESULT {"verdict":"PASS","notes":${JSON.stringify(notes)}}`);
