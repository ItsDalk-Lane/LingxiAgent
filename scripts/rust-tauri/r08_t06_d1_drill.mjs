#!/usr/bin/env node
// R08-T06 D1｜A11+A12 一体全绿往返演习（T06_DESIGN §11）。
//
// 腿序：
//   1. fixture v3 → migrate（T05 已证路径）→ 新根 + 指针（epoch 2 committed）。
//   2. 起确定性替身（r08_t06_standin.mjs：平台计数/收据 + openai 兼容模型端点）。
//   3. 起正式服务（--config 注入模型面指向替身；LINGXI_BRIDGE_WORKER 注入协议
//      替身 worker —— 生产注入缝 bridge_worker.rs discover）。
//   4. 正式入口新增（钉③）：新会话(sessions/new) + 附件(attachments/local
//      snapshot) + 设置编辑(preferences/appearance PUT)。
//   5. 桥接哨兵三行（钉②：confirmed/unknown/started，零 queued）：enable
//      telegram 连接器 → 替身 worker 发三条入站消息 → 真实投递链 →
//      S1 confirmed（替身计数=1）、S2 unknown、S3 started（回执扣留）。
//   6. 立即停服（S3 在 30s 回执超时窗内保持 started）。
//   7. rollback 九相（含 J9+⑤a 准备相、证据派生白名单探针、印章条款①）。
//   8. reforward 模态 A → 重启自检（S3 经 startup 校验落定 unknown，零重发）。
//   9. 终态断言：计数器每腿恒=1；S1 收据字节不变；计数/身份/附件 sha256 不变。
//
// 用法：node scripts/rust-tauri/r08_t06_d1_drill.mjs --bin <lingxi-service> --work <dir>
// 证据走 r08_run.py；work 用 /private/tmp 下全新目录；不触真实 ~/.lingxi。
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';

const REPO = new URL('../../', import.meta.url).pathname.replace(/\/$/, '');
const args = process.argv.slice(2);
const argValue = (flag) => {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : null;
};
const bin = argValue('--bin') ?? join(REPO, 'rust/target/debug/lingxi-service');
const work = argValue('--work');
if (!work) {
  console.error('usage: r08_t06_d1_drill.mjs --bin <lingxi-service> --work <dir>');
  process.exit(2);
}
const STANDIN_PORT = 47613;

let failures = 0;
const check = (name, ok, detail = '') => {
  console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ` :: ${detail}` : ''}`);
  if (!ok) failures += 1;
};
const sha256 = (p) => createHash('sha256').update(readFileSync(p)).digest('hex');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(fn, label, budgetMs = 120000) {
  const deadline = Date.now() + budgetMs;
  while (Date.now() < deadline) {
    const v = await fn();
    if (v) return v;
    await sleep(200);
  }
  throw new Error(`timeout: ${label}`);
}

rmSync(work, { recursive: true, force: true });
mkdirSync(work, { recursive: true });
// 孤儿防护：任何异常出口都回收自有子进程（服务持单写者锁，泄漏即阻断后续腿）。
const children = [];
process.on('uncaughtException', (err) => {
  for (const child of children) {
    try { child.kill('SIGKILL'); } catch { /* already gone */ }
  }
  console.error(`D1_ABORT ${err?.stack || err}`);
  process.exit(1);
});
const source = join(work, 'old-root');
const target = join(work, 'new-root');
const workDir = join(work, 'rollback-work');
const backupDir = join(work, 'rollback-backup');
const deltaOut = join(work, 'delta-out');
const pointer = join(work, 'active-data-root.json');
const standinScratch = join(work, 'standin');

// ── 1. fixture + migrate（T05 路径）──
const gen = spawnSync(process.execPath, [join(REPO, 'scripts/rust-tauri/r08_t04_fixture_gen.mjs'), '--seed', '20261023', '--out', source], { encoding: 'utf8' });
check('fixture gen exit 0', gen.status === 0, gen.stderr?.slice(0, 200));
const migrate = spawnSync(bin, ['migrate', '--source', source, '--target', target, '--work-dir', workDir, '--backup-dir', backupDir, '--pointer', pointer], { encoding: 'utf8', timeout: 900000 });
check('migrate exit 0', migrate.status === 0, (migrate.stderr || migrate.stdout || '').slice(-400));
check('migrate committed', /outcome=committed/.test(migrate.stdout || ''), (migrate.stdout || '').slice(-200));

// ── 2. 替身 ──
const standin = spawn(process.execPath, [join(REPO, 'scripts/rust-tauri/r08_t06_standin.mjs'), '--scratch', standinScratch], { stdio: ['ignore', 'pipe', 'pipe'] });
children.push(standin);
let standinErr = '';
standin.stderr.on('data', (d) => (standinErr += d));
await until(async () => standinErr.includes('STANDIN_READY'), 'standin ready');
const workerPath = join(standinScratch, 't06-standin-worker.cjs');
check('standin worker emitted', existsSync(workerPath));

// ── 3. 服务配置（模型面 → 替身）+ 起正式服务 ──
const configPath = join(work, 'service-config.json');
writeFileSync(configPath, `${JSON.stringify({
  home: target,
  providers: {
    't06-standin': {
      protocol: 'openai-completions',
      endpoint: `http://127.0.0.1:${STANDIN_PORT}/v1`,
      auth: { kind: 'apiKey', apiKey: 't06-synthetic-key' },
      // capabilities 必须声明在目录模型条目上（gateway.rs L400：显式
      // provider+model 请求时取 CatalogModel.capabilities；R05 RR1 F01 未声明
      // 即响亮拒）；路由绑定上同步声明（provider.models 为 None 时的合成路径）。
      models: [{ id: 't06-standin-model', capabilities: { tools: true, imageInput: false } }],
    },
  },
  models: { chat: { provider: 't06-standin', model: 't06-standin-model', capabilities: { tools: true, imageInput: false } } },
}, null, 2)}\n`);
function spawnService() {
  const svc = spawn(bin, ['--config', configPath, '--bind', '127.0.0.1:0'], {
    cwd: REPO,
    env: {
      ...process.env,
      LINGXI_BRIDGE_NODE: process.execPath,
      LINGXI_BRIDGE_WORKER: workerPath,
      LINGXI_PRODUCT_DIR: join(REPO, 'lib'),
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  // 就绪行在 stdout（main.rs L1156；isolation probe 同口径读 stdout）。
  let readyText = '';
  svc.stdout.on('data', (d) => (readyText += d));
  svc.stderr.on('data', (d) => (readyText += d));
  svc.__readyText = () => readyText;
  svc.__errText = () => readyText;
  return svc;
}
let service = spawnService();
children.push(service);
const readyLine = await until(async () => (service.__readyText().match(/LINGXI_SERVICE_READY addr=(\S+) /) || [])[1], 'service ready');
const base = `http://${readyLine}`;
const token = JSON.parse(readFileSync(join(target, 'lingxi-service/local-token.json'), 'utf8'));
const apiToken = token.token ?? token.localToken ?? token;
async function api(method, path, body, raw = false) {
  const res = await fetch(`${base}${path}`, {
    method,
    headers: { authorization: `Bearer ${apiToken}`, ...(body && !raw ? { 'content-type': 'application/json' } : {}) },
    body: body === undefined ? undefined : raw ? body : JSON.stringify(body),
  });
  const text = await res.text();
  let value = text;
  try { value = JSON.parse(text); } catch { /* keep text */ }
  return { status: res.status, value };
}

// ── 4. 正式入口新增（钉③）──
// 迁移新根按设计无 agents/（Register 类登记不转写，T06_DESIGN §2 F3）——
// 先经正式管理入口创建 agent（本身即新增数据的一部分）。
const agentCreated = await api('POST', '/lingxi/v1/agents', { name: 'T06-D1', id: 't06d1', yuan: 'lingxi' });
check('agents create 2xx', agentCreated.status >= 200 && agentCreated.status < 300, `${agentCreated.status} ${JSON.stringify(agentCreated.value).slice(0, 200)}`);
const created = await api('POST', '/lingxi/v1/sessions/new', { agentId: 't06d1', title: 'T06-D1 新增会话' });
check('sessions/new 2xx', created.status >= 200 && created.status < 300, `${created.status} ${JSON.stringify(created.value).slice(0, 200)}`);
const sessionId = created.value.sessionId ?? created.value.session_id ?? created.value.id;
check('sessionId present', Boolean(sessionId), JSON.stringify(created.value).slice(0, 200));

const attachPath = join(work, 't06-attach-payload.txt');
writeFileSync(attachPath, 'T06-D1 附件载荷：确定性字节内容 0123456789\n');
const attachRes = await api('POST', `/lingxi/v1/sessions/${sessionId}/attachments/local`, { path: attachPath, label: 't06-d1-attachment', mode: 'snapshot' });
check('attachments/local 2xx', attachRes.status >= 200 && attachRes.status < 300, `${attachRes.status} ${JSON.stringify(attachRes.value).slice(0, 250)}`);

const prefRes = await api('PUT', '/lingxi/v1/preferences/appearance', { theme: 't06-d1-edit' });
check('preferences/appearance PUT 2xx', prefRes.status >= 200 && prefRes.status < 300, `${prefRes.status} ${JSON.stringify(prefRes.value).slice(0, 200)}`);

// ── 5. 桥接哨兵（钉②：三行无 queued）──
const agent = 't06d1';
for (const step of [
  ['POST', `/lingxi/v1/bridge/config?agentId=${agent}`, { platform: 'telegram', enabled: false, credentials: { token: 't06-synthetic-token' } }],
  ['POST', `/lingxi/v1/bridge/owner?agentId=${agent}`, { platform: 'telegram', userId: 'sender-t06' }],
  ['POST', `/lingxi/v1/bridge/config?agentId=${agent}`, { platform: 'telegram', enabled: true }],
]) {
  const r = await api(step[0], step[1], step[2]);
  check(`bridge ${step[1].split('?')[0].split('/').pop()} 2xx`, r.status >= 200 && r.status < 300, `${r.status} ${JSON.stringify(r.value).slice(0, 200)}`);
}

const runsDb = join(target, 'lingxi-service/data/runs.db');
const q = (sql) => {
  const db = new DatabaseSync(runsDb, { readOnly: true });
  try {
    return db.prepare(sql).all();
  } finally {
    db.close();
  }
};
const outboxRows = () => q("SELECT delivery_id, conversation_id, status, platform_receipt_json, error FROM bridge_outbox_v1 ORDER BY delivery_id");
await until(async () => {
  const rows = outboxRows();
  return rows.length >= 3 && rows.filter((r) => r.status === 'confirmed').length >= 1 && rows.filter((r) => r.status === 'unknown').length >= 1 && rows.filter((r) => r.status === 'started').length >= 1;
}, 'sentinel rows settled (confirmed/unknown/started)');
let sentinels = outboxRows();
check('sentinel rows exactly 3 (no queued)', sentinels.length === 3 && sentinels.every((r) => r.status !== 'queued'), JSON.stringify(sentinels.map((r) => [r.conversation_id, r.status])));
const s1 = sentinels.find((r) => r.conversation_id === 'conv-t06-s1');
const s2 = sentinels.find((r) => r.conversation_id === 'conv-t06-s2');
const s3 = sentinels.find((r) => r.conversation_id === 'conv-t06-s3');
check('S1 confirmed with platform receipt', s1?.status === 'confirmed' && Boolean(s1?.platform_receipt_json), JSON.stringify(s1));
check('S2 unknown', s2?.status === 'unknown', JSON.stringify(s2));
check('S3 started', s3?.status === 'started', JSON.stringify(s3));

// 替身计数快照（生成期末）：每腿恰一次外发。
async function standinSnapshot() {
  const res = await fetch(`http://127.0.0.1:${STANDIN_PORT}/journal`);
  const { entries } = await res.json();
  const legs = { s1: 0, s2: 0, s3: 0, other: 0 };
  for (const e of entries) {
    if (e.kind !== 'platform-send') continue;
    if (String(e.conversationId).endsWith('-s1')) legs.s1 += 1;
    else if (String(e.conversationId).endsWith('-s2')) legs.s2 += 1;
    else if (String(e.conversationId).endsWith('-s3')) legs.s3 += 1;
    else legs.other += 1;
  }
  return legs;
}
const legsAfterGen = await standinSnapshot();
check('external sends exactly once per leg (gen phase)', legsAfterGen.s1 === 1 && legsAfterGen.s2 === 1 && legsAfterGen.s3 === 1 && legsAfterGen.other === 0, JSON.stringify(legsAfterGen));

// 新根状态快照（附件字节 + 计数 + 哨兵行身份）。
function snapshotNewRoot() {
  const attachments = {};
  const filesDir = join(target, 'session-files');
  const walk = (dir, rel) => {
    if (!existsSync(dir)) return;
    for (const name of readdirSync(dir).sort()) {
      const abs = join(dir, name);
      const r = rel ? `${rel}/${name}` : name;
      if (statSync(abs).isDirectory()) walk(abs, r);
      else attachments[r] = sha256(abs);
    }
  };
  walk(filesDir, '');
  const counts = {
    sessions: q('SELECT COUNT(*) AS n FROM sessions')[0].n,
    messages: q('SELECT COUNT(*) AS n FROM messages')[0].n,
    runs: q('SELECT COUNT(*) AS n FROM runs')[0].n,
  };
  const rows = outboxRows().map((r) => JSON.stringify(r));
  return { attachments, counts, outbox: rows };
}
const snapAfterGen = snapshotNewRoot();
const prefsAfterGen = readFileSync(join(target, 'user/preferences.json'), 'utf8');
check('settings edit landed', prefsAfterGen.includes('t06-d1-edit'), prefsAfterGen.slice(0, 120));

// ── 6. 停服（S3 回执扣留行的受控落定）──
// 事实（bridge.rs L507-529）：优雅关停时宿主把 inflight started 行落为
// unknown（error="worker stopped before a confirmed receipt"）；若行在关停前
// 仍 started（如 kill -9），则下次启动经 startup 校验落 unknown（error=
// "send result unknown after restart; do not resend"）。两条皆为受控落定、
// 皆零重发——A12 断言口径：S3 终态 unknown + 落定路径入证据 + 外部计数不变。
service.kill('SIGTERM');
await until(async () => service.exitCode !== null, 'service SIGTERM exit');
check('service clean exit 0', service.exitCode === 0, `exitCode=${service.exitCode}`);
const s3AtShutdown = outboxRows().find((r) => r.conversation_id === 'conv-t06-s3');
const s3SettlePath = s3AtShutdown?.status === 'started' ? 'pending-startup-validation'
  : /worker stopped before a confirmed receipt/.test(s3AtShutdown?.error || '') ? 'graceful-shutdown-settle'
  : `unexpected:${s3AtShutdown?.status}:${s3AtShutdown?.error}`;
check('S3 at shutdown: started-or-controlled-unknown', s3AtShutdown && (s3AtShutdown.status === 'started' || (s3AtShutdown.status === 'unknown' && /worker stopped before a confirmed receipt/.test(s3AtShutdown.error || ''))), JSON.stringify(s3AtShutdown));
console.log(`S3_SETTLE_PATH_AT_SHUTDOWN ${s3SettlePath}`);

// ── 7. rollback 九相 ──
const rollback = spawnSync(bin, [
  'rollback', '--pointer', pointer, '--old-root', source, '--new-root', target,
  '--work-dir', workDir, '--backup-dir', backupDir, '--delta-out', deltaOut,
  '--old-stack-launcher', join(REPO, 'scripts/launch.js'), '--old-stack-cwd', REPO,
], { encoding: 'utf8', timeout: 900000 });
check('rollback exit 0', rollback.status === 0, (rollback.stderr || rollback.stdout || '').slice(-600));
check('rollback committed', /outcome=committed/.test(rollback.stdout || ''), (rollback.stdout || '').slice(-300));
const ptrAfterRollback = JSON.parse(readFileSync(pointer, 'utf8'));
check('pointer → old root after rollback', ptrAfterRollback.activeDataRoot.endsWith('old-root'), JSON.stringify(ptrAfterRollback));
const oldStamp = JSON.parse(readFileSync(join(source, 'data-epoch.json'), 'utf8'));
check('old root stamp v2{1,1,1}', oldStamp.schemaVersion === 2 && oldStamp.epoch === 1 && oldStamp.minimumReaderEpoch === 1 && oldStamp.committedDataEpoch === 1, JSON.stringify(oldStamp));
const leases = JSON.parse(readFileSync(join(source, 'security/execution-leases.json'), 'utf8'));
check('old root leases empty registry', leases.schemaVersion === 1 && Array.isArray(leases.leases) && leases.leases.length === 0, JSON.stringify(leases));
check('old root a1 config.yaml created', existsSync(join(source, 'agents/a1/config.yaml')));
const kdb = new DatabaseSync(join(source, 'knowledge/knowledge.db'), { readOnly: true });
const synth = kdb.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'fixture_synthetic_%' ORDER BY name").all().map((r) => r.name);
// ⑤a 语义：RENAME 保序（行零丢失）→ 旧栈探针冷启动时其迁移链**自建生产空表**
// （createSchemaV16 IF NOT EXISTS——documents 生产全 schema 从不建；processing_
// artifacts 建为空表）。断言=合成表行保全 + 生产 processing_artifacts 为空。
const synthRows = synth.map((t) => [t, kdb.prepare(`SELECT COUNT(*) AS n FROM "${t}"`).get().n]);
const prodDocuments = kdb.prepare("SELECT COUNT(*) AS n FROM sqlite_master WHERE type='table' AND name='documents'").get().n;
const prodArtifactsCount = prodDocuments >= 0 && kdb.prepare("SELECT COUNT(*) AS n FROM sqlite_master WHERE type='table' AND name='processing_artifacts'").get().n
  ? kdb.prepare('SELECT COUNT(*) AS n FROM processing_artifacts').get().n
  : null;
kdb.close();
check('knowledge synthetic tables renamed with rows preserved', synth.length === 2 && synthRows.every(([, n]) => n > 0), JSON.stringify(synthRows));
check('knowledge prod documents absent; prod processing_artifacts empty (chain-built)', prodDocuments === 0 && (prodArtifactsCount === null || prodArtifactsCount === 0), `prodDocuments=${prodDocuments} prodArtifacts=${prodArtifactsCount}`);

// ── 8. reforward 模态 A ──
const reforward = spawnSync(bin, [
  'reforward', '--pointer', pointer, '--old-root', source, '--new-root', target, '--work-dir', workDir,
], { encoding: 'utf8', timeout: 900000 });
check('reforward-A exit 0', reforward.status === 0, (reforward.stderr || reforward.stdout || '').slice(-600));
check('reforward-A committed', /outcome=committed/.test(reforward.stdout || ''), (reforward.stdout || '').slice(-300));
const ptrAfterForward = JSON.parse(readFileSync(pointer, 'utf8'));
check('pointer → new root after reforward', ptrAfterForward.activeDataRoot.endsWith('new-root'), JSON.stringify(ptrAfterForward));

// 再起正式服务取终态（reforward 自检进程短启短停）；S3 若关停前仍 started，
// 此刻经 startup 校验落 unknown；否则保持关停路径已落定的 unknown。
service = spawnService();
children.push(service);
await until(async () => (service.__errText().match(/LINGXI_SERVICE_READY addr=(\S+) /) || [])[1], 'service restart after reforward');
await until(async () => {
  const row = outboxRows().find((r) => r.conversation_id === 'conv-t06-s3');
  return row && row.status === 'unknown' ? row : false;
}, 'S3 settled unknown (controlled path)', 90000);
const settled = outboxRows();
const s1b = settled.find((r) => r.conversation_id === 'conv-t06-s1');
const s2b = settled.find((r) => r.conversation_id === 'conv-t06-s2');
const s3b = settled.find((r) => r.conversation_id === 'conv-t06-s3');
check('S1 confirmed + receipt byte-identical after round trip', s1b?.status === 'confirmed' && s1b?.platform_receipt_json === s1?.platform_receipt_json, JSON.stringify([s1?.platform_receipt_json, s1b?.platform_receipt_json]));
check('S2 unchanged after round trip', s2b?.status === 'unknown' && s2b?.error === s2?.error, JSON.stringify([s2, s2b]));
const s3ControlledSettle = s3b?.status === 'unknown'
  && (/do not resend/.test(s3b?.error || '') || /worker stopped before a confirmed receipt/.test(s3b?.error || ''));
check('S3 settled unknown via controlled path (no resend)', s3ControlledSettle, JSON.stringify(s3b));
console.log(`S3_SETTLE_PATH_FINAL ${/do not resend/.test(s3b?.error || '') ? 'startup-validation' : 'graceful-shutdown-settle'}`);

// ── 9. 终态断言 ──
const legsFinal = await standinSnapshot();
check('external sends frozen across rollback+reforward (counter 恒=1/腿)', JSON.stringify(legsFinal) === JSON.stringify(legsAfterGen), `${JSON.stringify(legsAfterGen)} → ${JSON.stringify(legsFinal)}`);
const snapFinal = { attachments: snapFinalAttachments(), counts: null, outbox: null };
function snapFinalAttachments() {
  const attachments = {};
  const filesDir = join(target, 'session-files');
  const walk = (dir, rel) => {
    if (!existsSync(dir)) return;
    for (const name of readdirSync(dir).sort()) {
      const abs = join(dir, name);
      const r = rel ? `${rel}/${name}` : name;
      if (statSync(abs).isDirectory()) walk(abs, r);
      else attachments[r] = sha256(abs);
    }
  };
  walk(filesDir, '');
  return attachments;
}
check('new attachment bytes unchanged (A11 往返附件摘要)', JSON.stringify(snapFinal.attachments) === JSON.stringify(snapAfterGen.attachments), `${Object.keys(snapAfterGen.attachments).length} files`);
const countsFinal = {
  sessions: q('SELECT COUNT(*) AS n FROM sessions')[0].n,
  messages: q('SELECT COUNT(*) AS n FROM messages')[0].n,
  runs: q('SELECT COUNT(*) AS n FROM runs')[0].n,
};
check('session/message/run counts unchanged (A11 新增数据可恢复且不重复)', JSON.stringify(countsFinal) === JSON.stringify(snapAfterGen.counts), `${JSON.stringify(snapAfterGen.counts)} → ${JSON.stringify(countsFinal)}`);
const prefsFinal = readFileSync(join(target, 'user/preferences.json'), 'utf8');
check('settings edit preserved', prefsFinal.includes('t06-d1-edit'));

service.kill('SIGTERM');
await until(async () => service.exitCode !== null, 'final service stop');
standin.kill('SIGTERM');
await until(async () => standin.exitCode !== null, 'standin stop', 15000).catch(() => standin.kill('SIGKILL'));

console.log(`D1_RESULT failures=${failures} legs=${JSON.stringify(legsFinal)} counts=${JSON.stringify(countsFinal)}`);
console.log(failures === 0 ? 'D1 DRILL OK' : `D1 DRILL FAILURES: ${failures}`);
process.exit(failures === 0 ? 0 : 1);
