#!/usr/bin/env node
// R08-T06 D2｜失败注入三腿（T06_DESIGN §11）：
//   (a) 篡改 delta 包 manifest → --resume 后 export_verify 拒 → 两侧保留；
//   (b) 空间不足（RAM disk 小卷：avail < 2×tree+256MB slack）→ preflight 拒 → 零触碰；
//   (c) 旧栈探针失败（早退 launcher 桩——设计例因「环回端口预占用」的探针
//       bind:0 取号属 TOCTOU 不可确定性复现；早退桩同属「旧栈不服务」故障
//       类，J6 语义（失败→指针回指新根+修复模式）的 exercised 面相同，偏离在
//       PROGRESS-t06 台账披露）→ 指针回指新根 + refused + repairAction。
//
// 每腿独立全新 scratch；全部经 r08_run.py 取证。用法：
//   node scripts/rust-tauri/r08_t06_d2_drill.mjs --bin <lingxi-service> --scratch <dir>
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const REPO = new URL('../../', import.meta.url).pathname.replace(/\/$/, '');
const args = process.argv.slice(2);
const argValue = (flag) => {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : null;
};
const bin = argValue('--bin') ?? join(REPO, 'rust/target/debug/lingxi-service');
const scratch = argValue('--scratch');
if (!scratch) {
  console.error('usage: r08_t06_d2_drill.mjs --bin <lingxi-service> --scratch <dir>');
  process.exit(2);
}
rmSync(scratch, { recursive: true, force: true });
mkdirSync(scratch, { recursive: true });

let failures = 0;
const check = (name, ok, detail = '') => {
  console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ` :: ${detail}` : ''}`);
  if (!ok) failures += 1;
};
const sha256 = (p) => createHash('sha256').update(readFileSync(p)).digest('hex');
function hashTree(root) {
  const out = {};
  const walk = (dir, rel) => {
    if (!existsSync(dir)) return;
    for (const name of readdirSync(dir).sort()) {
      const abs = join(dir, name);
      const r = rel ? `${rel}/${name}` : name;
      if (statSync(abs).isDirectory()) walk(abs, r);
      else out[r] = sha256(abs);
    }
  };
  walk(root, '');
  return out;
}
function prep(leg) {
  const work = join(scratch, leg);
  const source = join(work, 'old-root');
  const target = join(work, 'new-root');
  const workDir = join(work, 'rb-work');
  const backupDir = join(work, 'rb-backup');
  const deltaOut = join(work, 'delta-out');
  const pointer = join(work, 'active-data-root.json');
  const gen = spawnSync(process.execPath, [join(REPO, 'scripts/rust-tauri/r08_t04_fixture_gen.mjs'), '--seed', '20261023', '--out', source], { encoding: 'utf8' });
  check(`[${leg}] fixture gen`, gen.status === 0, gen.stderr?.slice(0, 120));
  const migrate = spawnSync(bin, ['migrate', '--source', source, '--target', target, '--work-dir', join(work, 'm-work'), '--backup-dir', join(work, 'm-backup'), '--pointer', pointer], { encoding: 'utf8', timeout: 900000 });
  check(`[${leg}] migrate committed`, migrate.status === 0 && /outcome=committed/.test(migrate.stdout || ''), (migrate.stderr || '').slice(-250));
  return { work, source, target, workDir, backupDir, deltaOut, pointer };
}
const runRollback = (p, extra = []) => spawnSync(bin, ['rollback', '--pointer', p.pointer, '--old-root', p.source, '--new-root', p.target, '--work-dir', p.workDir, '--backup-dir', p.backupDir, '--delta-out', p.deltaOut, '--old-stack-launcher', join(REPO, 'scripts/launch.js'), '--old-stack-cwd', REPO, ...extra], { encoding: 'utf8', timeout: 900000 });
const pointerRoot = (p) => JSON.parse(readFileSync(p.pointer, 'utf8')).activeDataRoot;

// ── 腿 a：篡改 delta manifest → export_verify 拒 ──
{
  const p = prep('leg-a');
  const oldTreeBefore = hashTree(p.source);
  const stopped = runRollback(p, ['--stop-after-phase', 'delta_export']);
  check('[leg-a] stop-after delta_export exit 0', stopped.status === 0, (stopped.stderr || '').slice(-250));
  const pkgDir = join(p.deltaOut, readdirSync(p.deltaOut)[0]);
  const manifestPath = join(pkgDir, 'manifest.json');
  check('[leg-a] package manifest exists', existsSync(manifestPath));
  // 篡改：files-manifest 哈希改一位 → read_delta_package ③ 包自校验拒。
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  manifest.files.sha256 = manifest.files.sha256.replace(/.$/, manifest.files.sha256.endsWith('0') ? '1' : '0');
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  const journalPath = join(p.workDir, 'rollback-journal.json');
  const rollbackId = JSON.parse(readFileSync(journalPath, 'utf8')).id;
  const resumed = runRollback(p, ['--resume', rollbackId]);
  check('[leg-a] resume refused (non-zero)', resumed.status !== 0, `status=${resumed.status}`);
  check('[leg-a] refusal names export_verify', /export_verify/.test(resumed.stderr || ''), (resumed.stderr || '').slice(-250));
  const journal = JSON.parse(readFileSync(journalPath, 'utf8'));
  check('[leg-a] journal terminal refused (status=refused at export_verify)', journal.status === 'refused' && journal.refusal?.phase === 'export_verify', `${journal.status}/${journal.refusal?.phase}`);
  check('[leg-a] pointer unchanged (→ new root)', pointerRoot(p).endsWith('new-root'), pointerRoot(p));
  check('[leg-a] old root untouched (zero-touch)', JSON.stringify(hashTree(p.source)) === JSON.stringify(oldTreeBefore));
  check('[leg-a] new root intact', existsSync(join(p.target, 'data-epoch.json')));
}

// ── 腿 b：空间不足（RAM disk 小卷）→ preflight 拒 ──
{
  const DEV = spawnSync('hdiutil', ['attach', '-nomount', 'ram://65536'], { encoding: 'utf8' }).stdout.trim();
  check('[leg-b] ram disk attached', /^\/dev\/disk\d+$/.test(DEV), DEV);
  try {
    const fmt = spawnSync('newfs_hfs', ['-v', 'T06D2TINY', DEV], { encoding: 'utf8' });
    const mnt = spawnSync('diskutil', ['mount', DEV], { encoding: 'utf8' });
    const tinyRoot = (mnt.stdout || '').trim().split('\n').pop?.() || '/Volumes/T06D2TINY';
    const mountPoint = existsSync('/Volumes/T06D2TINY') ? '/Volumes/T06D2TINY' : tinyRoot;
    check('[leg-b] tiny volume mounted at /Volumes/T06D2TINY', mountPoint === '/Volumes/T06D2TINY', `${mountPoint} fmt=${fmt.status} mnt=${mnt.status}`);
    const p = prep('leg-b');
    const oldTreeBefore = hashTree(p.source);
    const tinyBackup = join(mountPoint, 'rb-backup');
    mkdirSync(tinyBackup, { recursive: true });
    const refused = spawnSync(bin, ['rollback', '--pointer', p.pointer, '--old-root', p.source, '--new-root', p.target, '--work-dir', p.workDir, '--backup-dir', tinyBackup, '--delta-out', p.deltaOut, '--old-stack-launcher', join(REPO, 'scripts/launch.js'), '--old-stack-cwd', REPO], { encoding: 'utf8', timeout: 900000 });
    check('[leg-b] preflight refused (non-zero)', refused.status !== 0, `status=${refused.status}`);
    check('[leg-b] refusal names insufficient space', /insufficient space on backup volume/.test(refused.stderr || ''), (refused.stderr || '').slice(-250));
    check('[leg-b] pointer unchanged (→ new root)', pointerRoot(p).endsWith('new-root'), pointerRoot(p));
    check('[leg-b] old root untouched (zero-touch)', JSON.stringify(hashTree(p.source)) === JSON.stringify(oldTreeBefore));
    check('[leg-b] old stamp untouched (still fixture {epoch:7})', JSON.parse(readFileSync(join(p.source, 'data-epoch.json'), 'utf8')).epoch === 7);
  } finally {
    spawnSync('diskutil', ['unmount', 'force', '/Volumes/T06D2TINY'], { encoding: 'utf8' });
    if (/^\/dev\/disk\d+$/.test(DEV)) spawnSync('hdiutil', ['detach', DEV, '-force'], { encoding: 'utf8' });
  }
}

// ── 腿 c：旧栈探针失败（早退 launcher 桩）→ J6 指针回指新根 + 修复模式 ──
{
  const p = prep('leg-c');
  const failLauncher = join(p.work, 'fail-launcher.cjs');
  writeFileSync(failLauncher, '// D2(c) 注入桩：模拟旧栈不服务（设计例因端口预占用不可确定性复现；\n// 同为「旧栈未就绪」故障类，J6 语义 exercised 面相同）。\nprocess.exit(1);\n');
  const refused = spawnSync(bin, ['rollback', '--pointer', p.pointer, '--old-root', p.source, '--new-root', p.target, '--work-dir', p.workDir, '--backup-dir', p.backupDir, '--delta-out', p.deltaOut, '--old-stack-launcher', failLauncher, '--old-stack-cwd', REPO], { encoding: 'utf8', timeout: 900000 });
  check('[leg-c] rollback refused (non-zero)', refused.status !== 0, `status=${refused.status}`);
  check('[leg-c] refusal names old stack probe', /old stack probe failed/.test(refused.stderr || ''), (refused.stderr || '').slice(-300));
  check('[leg-c] J6: pointer restored to new root', pointerRoot(p).endsWith('new-root'), pointerRoot(p));
  const journal = JSON.parse(readFileSync(join(p.workDir, 'rollback-journal.json'), 'utf8'));
  check('[leg-c] journal terminal refused (status=refused at old_stack_probe)', journal.status === 'refused' && journal.refusal?.phase === 'old_stack_probe', `${journal.status}/${journal.refusal?.phase}`);
  const repair = journal.refusal?.repairAction;
  check('[leg-c] repairAction recorded (修复模式 + 双侧保留)', Boolean(repair?.action) && repair?.context?.bothSidesPreserved === true, JSON.stringify(repair)?.slice(0, 220));
  check('[leg-c] both sides preserved', existsSync(join(p.target, 'data-epoch.json')) && existsSync(join(p.source, 'data-epoch.json')));
}

console.log(failures === 0 ? 'D2 DRILL OK' : `D2 DRILL FAILURES: ${failures}`);
console.log(`D2_RESULT failures=${failures}`);
process.exit(failures === 0 ? 0 : 1);
