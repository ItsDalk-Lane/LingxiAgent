#!/usr/bin/env node
// R08-T06 D3｜恢复式再前滚演习（T06_DESIGN §11）：
//   1-5. 与 D1 同型准备：fixture → migrate → 正式服务新增（会话/附件/设置/三哨兵）
//        → 停服 → rollback committed（产出增量包 + 旧根就绪 + 指针→旧根）。
//   6. reforward --recover-new-root --delta-package <包> --target 全新恢复根
//      （模态 B：legacy import → delta import → 二次应用零增长断言 → epoch
//      adopt → 指针 → selfcheck）→ committed。
//   7. 断言：收据 secondImportZeroGrowth=true（J4）；恢复根哨兵三行身份/收据
//      与生成期逐字节同（A12）；会话/消息计数与附件 sha256 同（A11）；替身
//      计数每腿恒=1（外部零重发）；原新根零触碰（J6 双侧保留）。
// 用法：node scripts/rust-tauri/r08_t06_d3_drill.mjs --bin <lingxi-service> --work <dir>
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, readdirSync, realpathSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs';
import http from 'node:http';
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
  console.error('usage: r08_t06_d3_drill.mjs --bin <lingxi-service> --work <dir>');
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
const children = [];
const standinStatus = { alive: false, exitCode: null, signal: null };
let standinErr = '';
// gate14 红迹：末尾 /journal 读取 `TypeError: fetch failed` 只打了 stack，
// cause 与替身退出状态都没留下——根因无法判定。abort 时一并输出。
const abort = (err) => {
  for (const child of children) {
    try { child.kill('SIGKILL'); } catch { /* already gone */ }
  }
  console.error(`D3_ABORT ${err?.stack || err}`);
  if (err?.cause) console.error(`D3_ABORT_CAUSE ${err.cause?.stack || err.cause}`);
  console.error(`D3_ABORT_STANDIN ${JSON.stringify(standinStatus)} stderrTail=${JSON.stringify(standinErr.slice(-600))}`);
  process.exit(1);
};
process.on('uncaughtException', abort);
process.on('unhandledRejection', abort);

const source = join(work, 'old-root');
const target = join(work, 'new-root');
const workDir = join(work, 'rb-work');
const backupDir = join(work, 'rb-backup');
const deltaOut = join(work, 'delta-out');
const pointer = join(work, 'active-data-root.json');
const standinScratch = join(work, 'standin');

// ── 1. fixture + migrate ──
const gen = spawnSync(process.execPath, [join(REPO, 'scripts/rust-tauri/r08_t04_fixture_gen.mjs'), '--seed', '20261023', '--out', source], { encoding: 'utf8' });
check('fixture gen exit 0', gen.status === 0, gen.stderr?.slice(0, 120));
const migrate = spawnSync(bin, ['migrate', '--source', source, '--target', target, '--work-dir', join(work, 'm-work'), '--backup-dir', join(work, 'm-backup'), '--pointer', pointer], { encoding: 'utf8', timeout: 900000 });
check('migrate committed', migrate.status === 0 && /outcome=committed/.test(migrate.stdout || ''), (migrate.stderr || '').slice(-250));

// ── 2. 替身 + 服务 ──
const standin = spawn(process.execPath, [join(REPO, 'scripts/rust-tauri/r08_t06_standin.mjs'), '--scratch', standinScratch], { stdio: ['ignore', 'pipe', 'pipe'] });
children.push(standin);
standinStatus.alive = true;
standin.stderr.on('data', (d) => (standinErr += d));
standin.on('exit', (code, signal) => {
  Object.assign(standinStatus, { alive: false, exitCode: code, signal });
  console.log(`STANDIN_EXITED code=${code} signal=${signal} at=${new Date().toISOString()}`);
});
await until(async () => standinErr.includes('STANDIN_READY'), 'standin ready');
const workerPath = join(standinScratch, 't06-standin-worker.cjs');
const configPath = join(work, 'service-config.json');
writeFileSync(configPath, `${JSON.stringify({
  home: target,
  providers: {
    't06-standin': {
      protocol: 'openai-completions',
      endpoint: `http://127.0.0.1:${STANDIN_PORT}/v1`,
      auth: { kind: 'apiKey', apiKey: 't06-synthetic-key' },
      models: [{ id: 't06-standin-model', capabilities: { tools: true, imageInput: false } }],
    },
  },
  models: { chat: { provider: 't06-standin', model: 't06-standin-model', capabilities: { tools: true, imageInput: false } } },
}, null, 2)}\n`);
const spawnService = (homeDir) => {
  const svc = spawn(bin, ['--config', configPath, '--bind', '127.0.0.1:0'], {
    cwd: REPO,
    env: { ...process.env, LINGXI_BRIDGE_NODE: process.execPath, LINGXI_BRIDGE_WORKER: workerPath, LINGXI_PRODUCT_DIR: join(REPO, 'lib') },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let text = '';
  svc.stdout.on('data', (d) => (text += d));
  svc.stderr.on('data', (d) => (text += d));
  svc.__text = () => text;
  return svc;
};
let service = spawnService(target);
children.push(service);
const readyLine = await until(async () => (service.__text().match(/LINGXI_SERVICE_READY addr=(\S+) /) || [])[1], 'service ready');
const base = `http://${readyLine}`;
const tokenJson = JSON.parse(readFileSync(join(target, 'lingxi-service/local-token.json'), 'utf8'));
const apiToken = tokenJson.token ?? tokenJson.localToken ?? tokenJson;
async function api(method, path, body) {
  const res = await fetch(`${base}${path}`, {
    method,
    headers: { authorization: `Bearer ${apiToken}`, ...(body ? { 'content-type': 'application/json' } : {}) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  let value = text;
  try { value = JSON.parse(text); } catch { /* keep text */ }
  return { status: res.status, value };
}

// ── 3. 正式入口新增 + 哨兵 ──
check('agents create', (await api('POST', '/lingxi/v1/agents', { name: 'T06-D3', id: 't06d3', yuan: 'lingxi' })).status < 300);
const created = await api('POST', '/lingxi/v1/sessions/new', { agentId: 't06d3', title: 'T06-D3 新增会话' });
check('sessions/new', created.status < 300, `${created.status}`);
const sessionId = created.value.sessionId ?? created.value.session_id ?? created.value.id;
const attachPath = join(work, 't06-attach-payload.txt');
writeFileSync(attachPath, 'T06-D3 附件载荷：确定性字节内容 9876543210\n');
check('attachments/local', (await api('POST', `/lingxi/v1/sessions/${sessionId}/attachments/local`, { path: attachPath, label: 't06-d3-attachment', mode: 'snapshot' })).status < 300);
check('preferences/appearance', (await api('PUT', '/lingxi/v1/preferences/appearance', { theme: 't06-d3-edit' })).status < 300);
for (const step of [
  ['POST', `/lingxi/v1/bridge/config?agentId=t06d3`, { platform: 'telegram', enabled: false, credentials: { token: 't06-synthetic-token' } }],
  ['POST', `/lingxi/v1/bridge/owner?agentId=t06d3`, { platform: 'telegram', userId: 'sender-t06' }],
  ['POST', `/lingxi/v1/bridge/config?agentId=t06d3`, { platform: 'telegram', enabled: true }],
]) {
  check(`bridge ${step[0]} ${step[1].includes('owner') ? 'owner' : 'config'}`, (await api(step[0], step[1], step[2])).status < 300);
}

const runsDbOf = (root) => join(root, 'lingxi-service/data/runs.db');
const q = (root, sql) => {
  const db = new DatabaseSync(runsDbOf(root), { readOnly: true });
  try { return db.prepare(sql).all(); } finally { db.close(); }
};
const outboxRowsOf = (root) => q(root, "SELECT delivery_id, conversation_id, status, platform_receipt_json, error FROM bridge_outbox_v1 ORDER BY delivery_id");
await until(async () => {
  const rows = outboxRowsOf(target);
  return rows.length >= 3 && rows.some((r) => r.status === 'confirmed') && rows.some((r) => r.status === 'unknown') && rows.some((r) => r.status === 'started');
}, 'sentinel rows settled');
const sentinels = outboxRowsOf(target);
check('sentinels exactly 3 (no queued)', sentinels.length === 3 && sentinels.every((r) => r.status !== 'queued'), JSON.stringify(sentinels.map((r) => [r.conversation_id, r.status])));
const s1 = sentinels.find((r) => r.conversation_id === 'conv-t06-s1');
const snapCounts = {
  sessions: q(target, 'SELECT COUNT(*) AS n FROM sessions')[0].n,
  messages: q(target, 'SELECT COUNT(*) AS n FROM messages')[0].n,
};
function attachmentShas(root) {
  const out = {};
  const filesDir = join(root, 'session-files');
  const walk = (dir, rel) => {
    if (!existsSync(dir)) return;
    for (const name of readdirSync(dir).sort()) {
      const abs = join(dir, name);
      const r = rel ? `${rel}/${name}` : name;
      if (statSync(abs).isDirectory()) walk(abs, r);
      else out[r] = sha256(abs);
    }
  };
  walk(filesDir, '');
  return out;
}
const snapAttachments = attachmentShas(target);

// ── 3b. R08 收尾 B 域前置：切后可变更新 + 附件身份 + 新助手持久化文件 ──
// B③：旧根迁入的 legacy 会话在新根上改名（可变元数据），回滚包必须携带该更新。
const legacySessionRow = q(target, `SELECT session_id, title FROM sessions WHERE session_id <> '${String(sessionId).replace(/'/g, "''")}' ORDER BY created_at_unix_ms, session_id LIMIT 1`)[0];
check('B3 legacy session present on new root', Boolean(legacySessionRow?.session_id), JSON.stringify(legacySessionRow));
const B3_TITLE = 'B3-RENAMED 旧会话切后改名';
const renamed = await api('POST', '/lingxi/v1/sessions/rename', { sessionId: legacySessionRow.session_id, title: B3_TITLE });
check('B3 rename legacy session on new root', renamed.status < 300, `${renamed.status} ${JSON.stringify(renamed.value).slice(0, 160)}`);
check('B3 renamed title persisted on new root', q(target, `SELECT title FROM sessions WHERE session_id='${legacySessionRow.session_id}'`)[0]?.title === B3_TITLE);
// B①：新增附件的正式身份（fileId）与原字节 sha（恢复后按原 fileId/sessionId 打开比对）。
const attachBytesSha = createHash('sha256').update(readFileSync(attachPath)).digest('hex');
const filesList = await api('GET', `/lingxi/v1/sessions/${sessionId}/files`);
check('B1 files list on new root', filesList.status < 300, `${filesList.status}`);
const attachRow = (Array.isArray(filesList.value?.files) ? filesList.value.files : Array.isArray(filesList.value) ? filesList.value : []).find((f) => f.label === 't06-d3-attachment' || f.filename === 't06-attach-payload.txt');
check('B1 attachment row on new root (managed_cache)', attachRow?.fileId && attachRow?.storageKind === 'managed_cache', JSON.stringify(attachRow).slice(0, 240));
const attachFileId = attachRow?.fileId;
// B②：切后新助手的持久化文件在新根真实存在（agents/t06d3/config.yaml）。
check('B2 new agent config.yaml exists on new root', existsSync(join(target, 'agents/t06d3/config.yaml')));
// 替身日志读取不走全局 fetch 连接池：每次新建连接（agent:false），排除 keep-alive
// 复用已被服务端关闭的空闲套接字这一竞态；错误保留 code/cause 原样抛出。
function standinGetJson(pathname) {
  return new Promise((resolve, reject) => {
    const req = http.get({ host: '127.0.0.1', port: STANDIN_PORT, path: pathname, agent: false }, (res) => {
      let raw = '';
      res.setEncoding('utf8');
      res.on('data', (d) => (raw += d));
      res.on('end', () => {
        if (res.statusCode !== 200) return reject(new Error(`standin ${pathname} → ${res.statusCode}: ${raw.slice(0, 200)}`));
        try { resolve(JSON.parse(raw)); } catch (e) { reject(new Error(`standin ${pathname} bad json: ${e.message}`, { cause: e })); }
      });
    });
    req.on('error', (e) => reject(new Error(`standin ${pathname} request failed: ${e.code || e.message}`, { cause: e })));
  });
}
async function standinLegs() {
  let payload;
  try {
    payload = await standinGetJson('/journal');
  } catch (err) {
    // 替身进程仍在才允许一次重试（瞬时套接字错误）；进程已退则直接响亮失败。
    if (!standinStatus.alive) {
      throw new Error(`standin journal unreachable and standin process already exited: ${JSON.stringify(standinStatus)}`, { cause: err });
    }
    console.log(`STANDIN_FETCH_RETRY reason=${err.message} standin=${JSON.stringify(standinStatus)}`);
    await sleep(500);
    payload = await standinGetJson('/journal');
  }
  const { entries } = payload;
  const legs = { s1: 0, s2: 0, s3: 0, other: 0 };
  for (const e of entries) {
    if (e.kind !== 'platform-send') continue;
    const leg = ['s1', 's2', 's3'].find((s) => String(e.conversationId).endsWith(`-${s}`));
    if (leg) legs[leg] += 1;
    else legs.other += 1;
  }
  return legs;
}
const legsAfterGen = await standinLegs();
check('external sends once per leg (gen)', legsAfterGen.s1 === 1 && legsAfterGen.s2 === 1 && legsAfterGen.s3 === 1 && legsAfterGen.other === 0, JSON.stringify(legsAfterGen));

// ── 4. 停服 + rollback ──
service.kill('SIGTERM');
await until(async () => service.exitCode !== null, 'service stop');
const rollback = spawnSync(bin, ['rollback', '--pointer', pointer, '--old-root', source, '--new-root', target, '--work-dir', workDir, '--backup-dir', backupDir, '--delta-out', deltaOut, '--old-stack-launcher', join(REPO, 'scripts/launch.js'), '--old-stack-cwd', REPO], { encoding: 'utf8', timeout: 900000 });
check('rollback committed', rollback.status === 0 && /outcome=committed/.test(rollback.stdout || ''), (rollback.stderr || '').slice(-300));
const rollbackId = (rollback.stdout || '').match(/rollbackId=(\S+) /)?.[1];
const pkgDir = join(deltaOut, rollbackId);
check('delta package dir exists', existsSync(join(pkgDir, 'manifest.json')), pkgDir);

// ── 5. reforward 模态 B：恢复式再前滚到全新根 ──
const recoveredRoot = join(work, 'recovered-root');
// R08 收尾 B①：先隔离原新根（改名移走）再恢复——恢复根内任何仍指向原新根
// 的附件路径都无法再被解析，路径与身份脱钩会在正式接口读取时响亮暴露。
const isolatedTarget = `${target}.isolated`;
renameSync(target, isolatedTarget);
check('original new root isolated before reforward', !existsSync(target) && existsSync(join(isolatedTarget, 'data-epoch.json')));
// reforward 与 rollback 共用 work-dir：预检只读消费同目录 rollback-journal.json
// 的 committed 事实（偏差③：journal 按动词分离文件、同目录共存）。
const reforward = spawnSync(bin, ['reforward', '--pointer', pointer, '--old-root', source, '--new-root', target, '--work-dir', workDir, '--recover-new-root', '--backup-dir', join(work, 'rf-backup'), '--delta-package', pkgDir, '--target', recoveredRoot], { encoding: 'utf8', timeout: 900000 });
check('reforward-B exit 0', reforward.status === 0, (reforward.stderr || '').slice(-400));
check('reforward-B committed', /outcome=committed/.test(reforward.stdout || ''), (reforward.stdout || '').slice(-300));
const ptrFinal = JSON.parse(readFileSync(pointer, 'utf8'));
check('pointer → recovered root', ptrFinal.activeDataRoot.endsWith('recovered-root'), JSON.stringify(ptrFinal));

// J4：secondImportZeroGrowth 落收据。
const rfReceiptPath = join(workDir, 'reforward-receipt.json');
const rfReceipt = existsSync(rfReceiptPath) ? JSON.parse(readFileSync(rfReceiptPath, 'utf8')) : null;
check('reforward receipt present', Boolean(rfReceipt), rfReceiptPath);
const zeroGrowth = JSON.stringify(rfReceipt ?? {}).includes('"secondImportZeroGrowth":true');
check('J4 secondImportZeroGrowth=true in receipt', zeroGrowth, JSON.stringify(rfReceipt).slice(0, 300));

// A11：恢复根数据同型。
const recCounts = {
  sessions: q(recoveredRoot, 'SELECT COUNT(*) AS n FROM sessions')[0].n,
  messages: q(recoveredRoot, 'SELECT COUNT(*) AS n FROM messages')[0].n,
};
check('A11 session/message counts equal (recovered root)', JSON.stringify(recCounts) === JSON.stringify(snapCounts), `${JSON.stringify(snapCounts)} → ${JSON.stringify(recCounts)}`);
// A11 断言取子集方向：原新根的每个附件必须在恢复根字节相同（新数据可恢复）。
// 恢复根可合法多于原新根——legacy 全量重导入会带回原新根已按过期 GC 的
// legacy 会话文件（fixture 时间戳 2025 已过期，服务 GC 是产品合法行为；
// t06-d3-drill-07 现场实证）。多余件计数披露，不作静默差。
const recAttachments = attachmentShas(recoveredRoot);
const missing = Object.entries(snapAttachments).filter(([k, v]) => recAttachments[k] !== v);
const extra = Object.keys(recAttachments).filter((k) => !(k in snapAttachments));
check('A11 attachments: every original-new-root file byte-identical in recovered root (subset)', missing.length === 0, JSON.stringify(missing));
console.log(`A11_REIMPORTED_LEGACY_FILES count=${extra.length} ${JSON.stringify(extra)}`);
check('A11 settings edit recovered', readFileSync(join(recoveredRoot, 'user/preferences.json'), 'utf8').includes('t06-d3-edit'));

// A12：恢复根哨兵三行身份/收据逐字节同（S3 受控落定 unknown 除外——语义字段
// 不变即恒成立：confirmed 不重发、unknown 不拾取、started 不盲重试）。
const recSentinels = outboxRowsOf(recoveredRoot);
const r1 = recSentinels.find((r) => r.conversation_id === 'conv-t06-s1');
const r2 = recSentinels.find((r) => r.conversation_id === 'conv-t06-s2');
const r3 = recSentinels.find((r) => r.conversation_id === 'conv-t06-s3');
check('A12 S1 confirmed + receipt byte-identical (recovered)', r1?.status === 'confirmed' && r1?.platform_receipt_json === s1?.platform_receipt_json, JSON.stringify([s1?.platform_receipt_json, r1?.platform_receipt_json]));
check('A12 S2 unknown unchanged (recovered)', r2?.status === 'unknown' && r2?.error?.includes('platform_returned_no_message_id'), JSON.stringify(r2));
check('A12 S3 settled unknown controlled (recovered)', r3?.status === 'unknown' && (/do not resend/.test(r3?.error || '') || /worker stopped before a confirmed receipt/.test(r3?.error || '') || /platform_returned_no_message_id/.test(r3?.error || '')), JSON.stringify(r3));

// ── 5b. R08 收尾 B 域：恢复根上经正式接口验收（原新根已隔离） ──
const recoveredConfigPath = join(work, 'service-config.recovered.json');
writeFileSync(recoveredConfigPath, readFileSync(configPath, 'utf8').replace(JSON.stringify(target), JSON.stringify(recoveredRoot)));
const recoveredService = spawn(bin, ['--config', recoveredConfigPath, '--bind', '127.0.0.1:0'], {
  cwd: REPO,
  env: { ...process.env, LINGXI_BRIDGE_NODE: process.execPath, LINGXI_BRIDGE_WORKER: workerPath, LINGXI_PRODUCT_DIR: join(REPO, 'lib') },
  stdio: ['ignore', 'pipe', 'pipe'],
});
children.push(recoveredService);
let recoveredText = '';
recoveredService.stdout.on('data', (d) => (recoveredText += d));
recoveredService.stderr.on('data', (d) => (recoveredText += d));
const recoveredAddr = await until(async () => (recoveredText.match(/LINGXI_SERVICE_READY addr=(\S+) /) || [])[1], 'recovered service ready');
const recoveredTokenJson = JSON.parse(readFileSync(join(recoveredRoot, 'lingxi-service/local-token.json'), 'utf8'));
const recoveredToken = recoveredTokenJson.token ?? recoveredTokenJson.localToken ?? recoveredTokenJson;
async function rapi(method, path, body) {
  const res = await fetch(`http://${recoveredAddr}${path}`, {
    method,
    headers: { authorization: `Bearer ${recoveredToken}`, ...(body ? { 'content-type': 'application/json' } : {}) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  let value = text;
  try { value = JSON.parse(text); } catch { /* keep text */ }
  return { status: res.status, value };
}
// B①：按原 sessionId/fileId 经正式接口打开附件；行路径必须落在恢复根（不指向
// 已隔离的原新根），且接口给出的路径处字节与原字节 sha 一致。
const recFile = await rapi('GET', `/lingxi/v1/sessions/${sessionId}/files/${attachFileId}`);
check('B1 attachment opens by original fileId/sessionId on recovered root', recFile.status === 200 && Boolean(recFile.value?.file), `${recFile.status} ${JSON.stringify(recFile.value).slice(0, 200)}`);
const recRealPath = recFile.value?.file?.realPath ?? '';
const recFilePath = recFile.value?.file?.filePath ?? '';
const recoveredRootReal = realpathSync(recoveredRoot);
check('B1 attachment paths rebased into recovered root (not the isolated origin)', [recRealPath, recFilePath].every((p) => typeof p === 'string' && p.length > 0 && (p.startsWith(recoveredRootReal) || p.startsWith(recoveredRoot)) && !p.startsWith(isolatedTarget) && !p.startsWith(target)), JSON.stringify({ recRealPath, recFilePath, recoveredRoot }));
check('B1 attachment bytes at interface-reported path identical to original', existsSync(recRealPath) && createHash('sha256').update(readFileSync(recRealPath)).digest('hex') === attachBytesSha, recRealPath);
// B②：切后新助手在恢复根上配置齐全、归属正确、可切换、可继续新建会话。
check('B2 new agent config.yaml recovered from package (origin isolated)', existsSync(join(recoveredRoot, 'agents/t06d3/config.yaml')));
const recAgents = await rapi('GET', '/lingxi/v1/agents');
const recAgentList = Array.isArray(recAgents.value?.agents) ? recAgents.value.agents : Array.isArray(recAgents.value) ? recAgents.value : [];
const recAgent = recAgentList.find((a) => a.id === 't06d3');
check('B2 new agent listed with its name on recovered root', recAgents.status === 200 && recAgent?.name === 'T06-D3', JSON.stringify(recAgentList.map((a) => [a.id, a.name])).slice(0, 200));
const recAgentConfig = await rapi('GET', '/lingxi/v1/agents/t06d3/config');
check('B2 new agent config readable on recovered root', recAgentConfig.status === 200, `${recAgentConfig.status}`);
const recSessionRow = await rapi('GET', `/lingxi/v1/sessions/find?sessionId=${encodeURIComponent(sessionId)}`);
check('B2 new session ownership preserved (agentId=t06d3)', recSessionRow.status === 200 && recSessionRow.value?.agentId === 't06d3', JSON.stringify(recSessionRow.value).slice(0, 160));
const recSwitch = await rapi('POST', '/lingxi/v1/agents/switch', { id: 't06d3' });
check('B2 switch to new agent on recovered root', recSwitch.status < 300, `${recSwitch.status} ${JSON.stringify(recSwitch.value).slice(0, 160)}`);
const recContinue = await rapi('POST', '/lingxi/v1/sessions/new', { agentId: 't06d3', title: 'B2 继续使用新助手' });
check('B2 continue using new agent (new session) on recovered root', recContinue.status < 300, `${recContinue.status} ${JSON.stringify(recContinue.value).slice(0, 160)}`);
// B③：旧会话切后改名经回滚→恢复后标题保留（不退回旧标题）。
const recLegacy = await rapi('GET', `/lingxi/v1/sessions/find?sessionId=${encodeURIComponent(legacySessionRow.session_id)}`);
check('B3 legacy session renamed title preserved after rollback→reforward', recLegacy.status === 200 && recLegacy.value?.title === B3_TITLE, JSON.stringify({ got: recLegacy.value?.title, want: B3_TITLE }));
// B③ 幂等：J4 二次回灌零增长已含 sessionRowsUpdated=0（收据字段逐项核对）。
const rfReceiptText = JSON.stringify(rfReceipt ?? {});
const secondTotalsAt = rfReceiptText.indexOf('secondApplyTotals');
check('B3 second apply reports sessions sessionRowsUpdated=0 (idempotent)', secondTotalsAt >= 0 && /"sessions":\{[^}]*"sessionRowsUpdated":0/.test(rfReceiptText.slice(secondTotalsAt)), rfReceiptText.slice(Math.max(0, secondTotalsAt), Math.max(0, secondTotalsAt) + 400));
recoveredService.kill('SIGTERM');
await until(async () => recoveredService.exitCode !== null, 'recovered service stop', 15000).catch(() => recoveredService.kill('SIGKILL'));

// 替身计数冻结 + 原新根零触碰（J6 双侧保留）。
const legsFinal = await standinLegs();
check('external sends frozen (counter 恒=1/腿 across rollback+reforward-B)', JSON.stringify(legsFinal) === JSON.stringify(legsAfterGen), `${JSON.stringify(legsAfterGen)} → ${JSON.stringify(legsFinal)}`);
check('original new root preserved (epoch stamp intact, isolated copy)', existsSync(join(isolatedTarget, 'data-epoch.json')));

standin.kill('SIGTERM');
await until(async () => standin.exitCode !== null, 'standin stop', 15000).catch(() => standin.kill('SIGKILL'));

console.log(`D3_RESULT failures=${failures} legs=${JSON.stringify(legsFinal)} counts=${JSON.stringify(recCounts)}`);
console.log(failures === 0 ? 'D3 DRILL OK' : `D3 DRILL FAILURES: ${failures}`);
process.exit(failures === 0 ? 0 : 1);
