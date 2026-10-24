#!/usr/bin/env node
/**
 * R08-T06 控制探针（Lead 裁决 2026-10-23，§7.2a）：实测旧栈冷启动写集，
 * 供 D1 探针写集白名单证据派生（白名单 = 控制实测 ∪ {准备相两件套}）。
 *
 * 腿序（全部确定性、响亮失败，零静默兜底）：
 *   1. r08_t04_fixture_gen.mjs --seed 生成 fixture v3 到 <scratch>/fixture；
 *   2. 整树复制到 <scratch>/control-root（未回滚副本）；
 *   3. 准备相同构处理 + J9/⑤a 封闭修复集（原字节归档 <scratch>/prepare-archive/，
 *      逐项 sha256 记录 prepare-fixset.json）：
 *      ① 印章重写为 v2 {schemaVersion:2, epoch:1, minimumReaderEpoch:1,
 *      committedDataEpoch:1, lastVersion:"legacy-import"}（镜像 Rust
 *      epoch::write_legacy_import_stamp 形状）；
 *      ② stale 过渡日志归档移除；
 *      ③ execution-leases.json → 生产空注册表形状（execution-lease-registry.ts:83-90）；
 *      ④ agents/a1/config.yaml 最小有效配置（lib/config.example.yaml L7-9 形状，0600）；
 *      ⑤ knowledge.db 合成表改名保序（单事务 RENAME + integrity_check，行零丢失）；
 *   4. 快照 A（全树真 sha256）；
 *   5. spawn `node scripts/launch.js server`（cwd=repo），env 净化
 *      （删 ELECTRON_RUN_AS_NODE/LINGXI_TOKEN/LINGXI_SERVER_OWNER/
 *      LINGXI_SERVER_OWNER_PID/LINGXI_ALLOW_DATA_DOWNGRADE），
 *      LINGXI_HOME=control-root，LINGXI_PORT=环回空闲口；
 *   6. 就绪闸（≤60s）：server-info.json 端口匹配 + GET /api/health===200；
 *   7. 停驻 --hold-ms（默认 1500）后按 server-info.json 的 pid 发 SIGTERM
 *      （owned-pid 卫生：杀前再验 /api/health 仍 200，证明所杀即所答；
 *      launch.js 不转发信号，直接杀 launcher 会孤儿 server）；
 *   8. 关停闸（≤15s）：/api/health 断连 + server-info.json 已移除 +
 *      launcher 退出；
 *   9. 快照 B → 写集差分 added/modified/removed；stderr 标记扫描；
 *  10. 报告 <scratch>/control-probe-report.json + CONTROL_PROBE_RESULT 行。
 */
import { spawn, execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const PREPARED_STAMP = {
  schemaVersion: 2,
  epoch: 1,
  minimumReaderEpoch: 1,
  committedDataEpoch: 1,
  lastVersion: 'legacy-import',
};
const MARKERS = [
  'LINGXI_DATA_EPOCH_BLOCKED',
  'LINGXI_DATA_EPOCH_TRANSITION_INCOMPLETE',
  'LINGXI_DATA_EPOCH_BASELINE_WARNING',
];
const SCRUB_ENV = [
  'ELECTRON_RUN_AS_NODE',
  'LINGXI_TOKEN',
  'LINGXI_SERVER_OWNER',
  'LINGXI_SERVER_OWNER_PID',
  'LINGXI_ALLOW_DATA_DOWNGRADE',
];

function parseArgs(argv) {
  const args = { scratch: null, seed: '20261023', holdMs: 1500 };
  for (let i = 2; i < argv.length; i += 1) {
    const key = argv[i];
    const value = argv[i + 1];
    if (key === '--scratch') { args.scratch = path.resolve(value); i += 1; }
    else if (key === '--seed') { args.seed = value; i += 1; }
    else if (key === '--hold-ms') { args.holdMs = Number(value); i += 1; }
    else throw new Error(`unknown argument: ${key}`);
  }
  if (!args.scratch) throw new Error('--scratch is required');
  if (!Number.isFinite(args.holdMs) || args.holdMs < 0 || args.holdMs > 30000) {
    throw new Error('--hold-ms must be in [0, 30000]');
  }
  return args;
}

function fail(stage, detail) {
  console.error(`CONTROL_PROBE_FAIL stage=${stage} detail=${detail}`);
  process.exit(2);
}

function sha256File(abs) {
  return createHash('sha256').update(fs.readFileSync(abs)).digest('hex');
}

function snapshot(root) {
  const out = new Map();
  (function walk(dir, rel) {
    for (const name of fs.readdirSync(dir).sort()) {
      const abs = path.join(dir, name);
      const r = rel ? `${rel}/${name}` : name;
      const st = fs.lstatSync(abs);
      if (st.isDirectory()) walk(abs, r);
      else if (st.isFile()) out.set(r, sha256File(abs));
      else throw new Error(`special file in snapshot: ${r}`);
    }
  })(root, '');
  return out;
}

function diffSnapshots(before, after) {
  const added = [];
  const removed = [];
  const modified = [];
  for (const [rel, sha] of after) {
    if (!before.has(rel)) added.push(rel);
    else if (before.get(rel) !== sha) modified.push(rel);
  }
  for (const rel of before.keys()) if (!after.has(rel)) removed.push(rel);
  return { added, removed, modified };
}

async function freePort() {
  const srv = net.createServer();
  await new Promise((resolve, reject) => {
    srv.once('error', reject);
    srv.listen(0, '127.0.0.1', resolve);
  });
  const { port } = srv.address();
  await new Promise((resolve) => srv.close(resolve));
  return port;
}

const sleep = (ms) => new Promise((resolve) => { setTimeout(resolve, ms); });

async function healthStatus(port, token) {
  try {
    const res = await fetch(`http://127.0.0.1:${port}/api/health`, {
      headers: token ? { Authorization: `Bearer ${token}` } : {},
      signal: AbortSignal.timeout(2000),
    });
    return res.status;
  } catch {
    return 0;
  }
}

async function main() {
  const args = parseArgs(process.argv);
  fs.mkdirSync(args.scratch, { recursive: true });
  const fixtureDir = path.join(args.scratch, 'fixture');
  const controlRoot = path.join(args.scratch, 'control-root');
  const archiveDir = path.join(args.scratch, 'prepare-archive');
  if (fs.existsSync(controlRoot)) fail('scratch', `control-root already exists: ${controlRoot}`);
  if (fs.existsSync(fixtureDir)) fail('scratch', `fixture already exists: ${fixtureDir}`);
  fs.mkdirSync(archiveDir, { recursive: true });

  // 1. fixture
  const genOut = execFileSync(process.execPath, [
    path.join(REPO, 'scripts/rust-tauri/r08_t04_fixture_gen.mjs'),
    '--seed', args.seed, '--out', fixtureDir,
  ], { encoding: 'utf8' });
  fs.writeFileSync(path.join(args.scratch, 'fixture-gen.json'), genOut);

  // 2. copy
  fs.cpSync(fixtureDir, controlRoot, { recursive: true });

  // 3. prepare-equivalent + J9 封闭修复集（归档原字节 → 逐项写入 → sha256 记录）：
  //    ① 印章重写为 v2 {1,1,1}（镜像 Rust epoch::write_legacy_import_stamp）；
  //    ② stale 过渡日志归档移除；
  //    ③ execution-leases.json → 生产空注册表形状（execution-lease-registry.ts:83-90）；
  //    ④ agents/a1/config.yaml 最小有效配置（形状引 lib/config.example.yaml L7-9
  //       + core/first-run.ts:181-199 播种链，0600）。
  const fixSet = [];
  const recordFix = (rel, action, originalSha, newSha) => {
    fixSet.push({ path: rel, action, originalSha256: originalSha, newSha256: newSha });
  };
  const stampPath = path.join(controlRoot, 'data-epoch.json');
  const journalPath = path.join(controlRoot, 'data-epoch-transition.json');
  const leasesPath = path.join(controlRoot, 'security/execution-leases.json');
  const a1ConfigPath = path.join(controlRoot, 'agents/a1/config.yaml');
  fs.writeFileSync(path.join(archiveDir, 'data-epoch.json.original'), fs.readFileSync(stampPath));
  fs.writeFileSync(path.join(archiveDir, 'data-epoch-transition.json.original'), fs.readFileSync(journalPath));
  fs.writeFileSync(path.join(archiveDir, 'execution-leases.json.original'), fs.readFileSync(leasesPath));
  recordFix('data-epoch.json', 'rewrite-stamp-v2', sha256File(stampPath), null);
  recordFix('data-epoch-transition.json', 'archive-remove', sha256File(journalPath), null);
  recordFix('security/execution-leases.json', 'rewrite-empty-registry', sha256File(leasesPath), null);
  recordFix('agents/a1/config.yaml', 'create-minimal', null, null);

  fs.writeFileSync(stampPath, `${JSON.stringify({ ...PREPARED_STAMP, updatedAt: new Date().toISOString() }, null, 2)}\n`, { mode: 0o600 });
  fs.unlinkSync(journalPath);
  const nowIso = new Date().toISOString();
  fs.writeFileSync(leasesPath, `${JSON.stringify({ schemaVersion: 1, leases: [], createdAt: nowIso, updatedAt: nowIso }, null, 2)}\n`);
  const a1Config = [
    '# R08-T06 J9 夹具同构修复（控制腿）：形状引生产模板 lib/config.example.yaml L7-9',
    '# + core/first-run.ts:181-199 seedDefaultAgent 播种链；仅 agent 块最小键。',
    'agent:',
    '  name: "A1"',
    '  yuan: "lingxi"',
    '',
  ].join('\n');
  fs.writeFileSync(a1ConfigPath, a1Config, { mode: 0o600 });
  fixSet[0].newSha256 = sha256File(stampPath);
  fixSet[2].newSha256 = sha256File(leasesPath);
  fixSet[3].newSha256 = sha256File(a1ConfigPath);

  // 3e. ⑤a（Lead 批准 2026-10-23）：knowledge.db 合成表改名保序——夹具表
  //     documents/processing_artifacts 与生产同名但列形为合成最小形，迁移链
  //     CREATE TABLE IF NOT EXISTS 会跳过建表后又引用缺失列（content_snapshot_id）
  //     而崩。RENAME 保序（行零丢失、字节仍在库内）→ 迁移链自建生产空表。
  //     纪律：原库字节先归档；RENAME 在单事务内；术后 PRAGMA integrity_check 入证据。
  const knowledgeDbPath = path.join(controlRoot, 'knowledge/knowledge.db');
  fs.writeFileSync(path.join(archiveDir, 'knowledge.db.original'), fs.readFileSync(knowledgeDbPath));
  recordFix('knowledge/knowledge.db', 'rename-synthetic-tables', sha256File(knowledgeDbPath), null);
  const sqlite = await import('better-sqlite3');
  const kdb = sqlite.default(knowledgeDbPath);
  const renamed = [];
  kdb.exec('BEGIN');
  try {
    for (const [from, to] of [
      ['documents', 'fixture_synthetic_documents'],
      ['processing_artifacts', 'fixture_synthetic_processing_artifacts'],
    ]) {
      const exists = kdb.prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?").get(from);
      if (exists) { kdb.exec(`ALTER TABLE "${from}" RENAME TO "${to}"`); renamed.push([from, to]); }
    }
    kdb.exec('COMMIT');
  } catch (err) {
    kdb.exec('ROLLBACK');
    kdb.close();
    fail('fixset-knowledge', `rename failed: ${err && err.message ? err.message : err}`);
  }
  const integrity = kdb.pragma('integrity_check', { simple: true });
  const preservedRows = Object.fromEntries(renamed.map(([, to]) => [
    to, kdb.prepare(`SELECT COUNT(*) AS n FROM "${to}"`).get().n,
  ]));
  kdb.close();
  if (integrity !== 'ok') fail('fixset-knowledge', `integrity_check=${integrity}`);
  recordFix('knowledge/knowledge.db', 'integrity_check', null, integrity);
  fixSet[4].newSha256 = sha256File(knowledgeDbPath);
  fs.writeFileSync(
    path.join(args.scratch, 'prepare-fixset.json'),
    `${JSON.stringify({
      schemaVersion: 't06-prepare-fixset/v2',
      items: fixSet,
      knowledgeRename: { renamed, preservedRows, integrityCheck: integrity },
      disclosure: 'knowledge 视图为空（合成表已改名保序，行零丢失）',
    }, null, 2)}\n`,
  );

  // 4. snapshot A
  const snapA = snapshot(controlRoot);
  fs.writeFileSync(
    path.join(args.scratch, 'snapshot-a.json'),
    `${JSON.stringify(Object.fromEntries([...snapA].sort()), null, 2)}\n`,
  );

  // 5. spawn old stack
  const port = await freePort();
  const stdoutPath = path.join(args.scratch, 'old-stack.stdout.log');
  const stderrPath = path.join(args.scratch, 'old-stack.stderr.log');
  const outFd = fs.openSync(stdoutPath, 'w');
  const errFd = fs.openSync(stderrPath, 'w');
  const env = { ...process.env, LINGXI_HOME: controlRoot, LINGXI_PORT: String(port) };
  for (const key of SCRUB_ENV) delete env[key];
  const child = spawn(process.execPath, [path.join(REPO, 'scripts/launch.js'), 'server'], {
    cwd: REPO, env, stdio: ['ignore', outFd, errFd],
  });
  fs.closeSync(outFd);
  fs.closeSync(errFd);
  let launcherExit = null;
  child.on('exit', (code, signal) => { launcherExit = { code, signal }; });

  const serverInfoPath = path.join(controlRoot, 'server-info.json');
  const readinessDeadline = Date.now() + 60000;
  let serverInfo = null;
  let ready = false;
  let lastHttp = 0;
  let stopped = false;
  try {
    while (Date.now() < readinessDeadline) {
      if (launcherExit) fail('readiness', `launcher exited early: ${JSON.stringify(launcherExit)}`);
      if (fs.existsSync(serverInfoPath)) {
        try {
          const parsed = JSON.parse(fs.readFileSync(serverInfoPath, 'utf8'));
          if (parsed.port === port && Number.isInteger(parsed.pid) && typeof parsed.token === 'string') {
            serverInfo = parsed;
            lastHttp = await healthStatus(port, parsed.token);
            if (lastHttp === 200) { ready = true; break; }
          }
        } catch { /* 半写状态，下一轮重读 */ }
      }
      await sleep(200);
    }
    if (!ready) fail('readiness', `not ready in 60s (lastHttp=${lastHttp}, serverInfo=${JSON.stringify(serverInfo)})`);

    // 6. settle window
    await sleep(args.holdMs);

    // 7. SIGTERM owned-pid（杀前再验所杀即所答；launch.js 不转发信号，
    //    杀 launcher 会孤儿 server —— 必须杀 server-info.json 的 pid）
    const preKill = await healthStatus(port, serverInfo.token);
    if (preKill !== 200) fail('pre-kill', `health=${preKill} before SIGTERM`);
    const serverPid = serverInfo.pid;
    process.kill(serverPid, 'SIGTERM');

    // 8. shutdown gate
    const shutdownDeadline = Date.now() + 15000;
    stopped = false;
    while (Date.now() < shutdownDeadline) {
      const alive = await healthStatus(port, serverInfo.token);
      if (alive === 0 && !fs.existsSync(serverInfoPath)) { stopped = true; break; }
      await sleep(200);
    }
    if (!stopped) fail('shutdown', 'server did not stop cleanly in 15s');
    const launcherDeadline = Date.now() + 5000;
    while (!launcherExit && Date.now() < launcherDeadline) await sleep(100);
    if (!launcherExit) {
      child.kill('SIGKILL');
      fail('shutdown', 'launcher did not exit after server exit');
    }
  } finally {
    // 孤儿防护：launch.js 不转发信号，先杀 owned-pid 再杀 launcher。
    if (serverInfo && Number.isInteger(serverInfo.pid)) {
      try { process.kill(serverInfo.pid, 'SIGTERM'); } catch { /* 已退出 */ }
      await sleep(500);
      try { process.kill(serverInfo.pid, 'SIGKILL'); } catch { /* 已退出 */ }
    }
    if (!launcherExit) child.kill('SIGKILL');
  }

  // 9. snapshot B + diff + markers
  const snapB = snapshot(controlRoot);
  fs.writeFileSync(
    path.join(args.scratch, 'snapshot-b.json'),
    `${JSON.stringify(Object.fromEntries([...snapB].sort()), null, 2)}\n`,
  );
  const writeSet = diffSnapshots(snapA, snapB);
  const stderrText = fs.readFileSync(stderrPath, 'utf8');
  const stderrLines = stderrText.split('\n');
  const markerHits = Object.fromEntries(MARKERS.map((m) => [
    m, stderrLines.filter((l) => l.includes(m)),
  ]));
  const warnLines = stderrLines.filter((l) => /warn|fail.?open|fallback|降级|epoch/i.test(l));

  // 10. report
  const report = {
    schemaVersion: 't06-control-probe/v1',
    seed: args.seed,
    port,
    serverPid: serverInfo.pid,
    holdMs: args.holdMs,
    readinessOk: true,
    shutdownClean: stopped,
    serverInfoUnlinked: !fs.existsSync(serverInfoPath),
    launcherExit,
    writeSet,
    stderrMarkers: markerHits,
    stderrWarnLines: warnLines,
    files: { stdout: stdoutPath, stderr: stderrPath },
  };
  fs.writeFileSync(
    path.join(args.scratch, 'control-probe-report.json'),
    `${JSON.stringify(report, null, 2)}\n`,
  );
  console.log(`CONTROL_PROBE_RESULT port=${port} pid=${serverInfo.pid} `
    + `added=${writeSet.added.length} modified=${writeSet.modified.length} `
    + `removed=${writeSet.removed.length} markersClean=${Object.values(markerHits).every((v) => v.length === 0)}`);
}

main().catch((err) => fail('top', err && err.stack ? err.stack : String(err)));
