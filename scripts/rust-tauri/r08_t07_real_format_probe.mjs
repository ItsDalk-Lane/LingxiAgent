#!/usr/bin/env node
/**
 * R08-T07 F-T07-1 决定性探针：真实旧栈会话格式（Pi SDK）在两内核的可见性。
 *
 *   A 腿（旧栈，应绿）：夹具旧根副本换成 1 个真实格式会话
 *     （首行 {type:"session",version,id,cwd,timestamp} + {type:"message",
 *      id,parentId,timestamp,message:{role,content}} 条目，lib/session-jsonl.ts
 *      生产读取格式/tests 先例），冷启动旧栈 → /api/sessions 必须列出该会话，
 *      会话内容可读（消息数>0）。
 *   B 腿（T04 legacy-import，红证据）：同一根跑 lingxi-service legacy-import
 *     → 新根 sessions 表应有该会话但 messages 为 0 条（真实条目全部
 *     is_message=false —— 顶层无 role、entryType 缺省，convert_sessions.rs:331）。
 *
 * 输出 REAL_FORMAT_PROBE_RESULT 行；A 绿 B 红即缺口钉死（如实记录，不修产品）。
 */
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { DatabaseSync } from 'node:sqlite';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const args = process.argv.slice(2);
const argValue = (f) => { const i = args.indexOf(f); return i >= 0 ? args[i + 1] : null; };
const bin = argValue('--bin') ?? path.join(REPO, 'rust/target/debug/lingxi-service');
const work = argValue('--work');
if (!work) { console.error('usage: r08_t07_real_format_probe.mjs --bin <lingxi-service> --work <dir>'); process.exit(2); }
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
fs.rmSync(work, { recursive: true, force: true });
fs.mkdirSync(work, { recursive: true });
let failures = 0;
const check = (name, ok, detail = '') => { console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ` :: ${detail}` : ''}`); if (!ok) failures += 1; };
const children = [];
process.on('uncaughtException', (err) => { for (const c of children) { try { c.kill('SIGKILL'); } catch { /* gone */ } } console.error(`PROBE_ABORT ${err?.stack || err}`); process.exit(1); });

// ── 数据根：夹具 v3 副本 → 换成 1 个真实格式会话（其余夹具会话移除）──
const rootA = path.join(work, 'old-root');
{
  const gen = spawnSync(process.execPath, [path.join(REPO, 'scripts/rust-tauri/r08_t04_fixture_gen.mjs'), '--seed', '20261023', '--out', path.join(work, 'fixture')], { encoding: 'utf8' });
  check('fixture gen', gen.status === 0);
  fs.cpSync(path.join(work, 'fixture'), rootA, { recursive: true });
  const sessionsDir = path.join(rootA, 'agents/a1/sessions');
  for (const name of fs.readdirSync(sessionsDir)) {
    if (name.endsWith('.jsonl')) fs.rmSync(path.join(sessionsDir, name));
  }
  // 准备集（同 A/B drill）：旧栈可冷启动。
  const stamp = { schemaVersion: 2, epoch: 1, minimumReaderEpoch: 1, committedDataEpoch: 1, lastVersion: 'legacy-import', updatedAt: new Date().toISOString() };
  fs.writeFileSync(path.join(rootA, 'data-epoch.json'), `${JSON.stringify(stamp, null, 2)}\n`, { mode: 0o600 });
  fs.rmSync(path.join(rootA, 'data-epoch-transition.json'), { force: true });
  const nowIso = new Date().toISOString();
  fs.writeFileSync(path.join(rootA, 'security/execution-leases.json'), `${JSON.stringify({ schemaVersion: 1, leases: [], createdAt: nowIso, updatedAt: nowIso }, null, 2)}\n`);
  fs.mkdirSync(path.join(rootA, 'agents/a1'), { recursive: true });
  if (!fs.existsSync(path.join(rootA, 'agents/a1/config.yaml'))) {
    fs.writeFileSync(path.join(rootA, 'agents/a1/config.yaml'), 'agent:\n  name: "A1"\n  yuan: "lingxi"\n', { mode: 0o600 });
  }
  // 真实格式会话（Pi SDK：tests/history-pagination-run-continuity.test.ts L87-122 同形状）。
  const lines = [
    JSON.stringify({ type: 'session', version: 3, id: '7e5f1a3c-9d24-4b8e-a6c1-2f4b8d9e0a31', cwd: '/tmp', timestamp: '2026-10-01T09:00:00Z' }),
    JSON.stringify({ type: 'message', id: 'u1', parentId: null, timestamp: '2026-10-01T10:00:00Z', message: { role: 'user', content: 'T07 真实格式探针：请阅读并回复。' } }),
    JSON.stringify({ type: 'message', id: 'a1', parentId: 'u1', timestamp: '2026-10-01T10:00:01Z', message: { role: 'assistant', content: [{ type: 'text', text: 'T07 真实格式回复：已收到。' }] } }),
    JSON.stringify({ type: 'message', id: 'u2', parentId: 'a1', timestamp: '2026-10-01T10:01:00Z', message: { role: 'user', content: '第二条用户消息。' } }),
  ];
  fs.writeFileSync(path.join(sessionsDir, 'real-format-probe.jsonl'), `${lines.join('\n')}\n`);
  // 合成 manifest 移除（真实根的 manifest 由旧栈自建；避免合成行隐藏会话）。
  fs.rmSync(path.join(rootA, 'session-manifest.db'), { force: true });
  fs.rmSync(path.join(rootA, 'session-manifest.db-wal'), { force: true });
  fs.rmSync(path.join(rootA, 'session-manifest.db-shm'), { force: true });
  // ⑤a：knowledge.db 合成表改名（同 A/B drill 准备集，否则旧栈 createSchemaV16 崩）。
  const sqlite = await import('better-sqlite3');
  const kdb = sqlite.default(path.join(rootA, 'knowledge/knowledge.db'));
  kdb.exec('BEGIN');
  for (const [from, to] of [['documents', 'fixture_synthetic_documents'], ['processing_artifacts', 'fixture_synthetic_processing_artifacts']]) {
    if (kdb.prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?").get(from)) kdb.exec(`ALTER TABLE "${from}" RENAME TO "${to}"`);
  }
  kdb.exec('COMMIT');
  kdb.close();
}

async function freePort() {
  const srv = net.createServer();
  await new Promise((res, rej) => { srv.once('error', rej); srv.listen(0, '127.0.0.1', res); });
  const { port } = srv.address();
  await new Promise((res) => srv.close(res));
  return port;
}

// ── A 腿：旧栈冷启动 → 列表 + 内容 ──
const port = await freePort();
const log = fs.openSync(path.join(work, 'old-stack.log'), 'w');
const env = { ...process.env, LINGXI_HOME: rootA, LINGXI_PORT: String(port) };
for (const k of ['ELECTRON_RUN_AS_NODE', 'LINGXI_TOKEN', 'LINGXI_SERVER_OWNER', 'LINGXI_SERVER_OWNER_PID', 'LINGXI_ALLOW_DATA_DOWNGRADE']) delete env[k];
const launcher = spawn(process.execPath, [path.join(REPO, 'scripts/launch.js'), 'server'], { cwd: REPO, env, stdio: ['ignore', log, log] });
fs.closeSync(log);
children.push(launcher);
let serverInfo = null;
{
  const infoPath = path.join(rootA, 'server-info.json');
  const deadline = Date.now() + 90000;
  while (Date.now() < deadline) {
    if (fs.existsSync(infoPath)) {
      try {
        const p = JSON.parse(fs.readFileSync(infoPath, 'utf8'));
        if (p.port === port && Number.isInteger(p.pid) && typeof p.token === 'string') {
          try {
            const res = await fetch(`http://127.0.0.1:${port}/api/health`, { headers: { Authorization: `Bearer ${p.token}` }, signal: AbortSignal.timeout(2000) });
            if (res.status === 200) { serverInfo = p; break; }
          } catch { /* not yet */ }
        }
      } catch { /* half-written */ }
    }
    await sleep(250);
  }
}
check('A: old stack ready', Boolean(serverInfo));
let listed = [];
let contentInfo = null;
if (serverInfo) {
  const res = await fetch(`http://127.0.0.1:${port}/api/sessions`, { headers: { Authorization: `Bearer ${serverInfo.token}` } });
  listed = await res.json();
  check('A: real-format session LISTED by old stack', listed.length === 1, JSON.stringify(listed.map((s) => [s.title, s.path?.slice(-40)])));
  const sess = listed[0];
  if (sess) {
    // 内容读取：summary 投影（消息计数面）。
    const sumRes = await fetch(`http://127.0.0.1:${port}/api/sessions/summary?path=${encodeURIComponent(sess.path)}`, { headers: { Authorization: `Bearer ${serverInfo.token}` } });
    const summary = await sumRes.json().catch(() => null);
    contentInfo = { summaryStatus: sumRes.status, messageCount: summary?.messageCount ?? summary?.message_count ?? null, keys: summary ? Object.keys(summary).slice(0, 8) : null };
    check('A: real-format session content readable', sumRes.status === 200, JSON.stringify(contentInfo));
  }
  try { process.kill(serverInfo.pid, 'SIGTERM'); } catch { /* gone */ }
  await sleep(1500);
  try { process.kill(serverInfo.pid, 'SIGKILL'); } catch { /* gone */ }
  launcher.kill('SIGKILL');
}

// ── B 腿：同一根 legacy-import（T04 链）→ 计数对账 ──
const target = path.join(work, 'new-root');
const imported = spawnSync(bin, ['legacy-import', '--source', rootA, '--target', target], { encoding: 'utf8', timeout: 900000 });
check('B: legacy-import exit 0', imported.status === 0, (imported.stderr || '').slice(-300));
let bCounts = null;
if (imported.status === 0) {
  const db = new DatabaseSync(path.join(target, 'lingxi-service/data/runs.db'), { readOnly: true });
  const sessions = db.prepare('SELECT COUNT(*) AS n FROM sessions').get().n;
  const messages = db.prepare('SELECT COUNT(*) AS n FROM messages').get().n;
  const probeRows = db.prepare("SELECT entry_type, role FROM messages WHERE session_id='real-format-probe'").all();
  const conversational = probeRows.filter((r) => r.entry_type === 'message' && (r.role === 'user' || r.role === 'assistant')).length;
  const unknownBlocks = probeRows.filter((r) => r.entry_type === 'legacy-unknown-block').length;
  const issues = db.prepare('SELECT COUNT(*) AS n FROM import_issues').get().n;
  db.close();
  bCounts = { sessions, messages, probeRows: probeRows.length, conversational, unknownBlocks, issues };
  // 决定性断言（红证据，预期 FAIL）：真实格式的 3 条对话消息须以 user/assistant
  // 消息身份迁入；若全部落 legacy-unknown-block（原字节隔离、非对话）即缺口。
  check('B: real-format entries imported as user/assistant messages (EXPECTED RED if F-T07-1 holds)', conversational >= 3, JSON.stringify(bCounts));
}
console.log(`REAL_FORMAT_PROBE_RESULT listedByOldStack=${listed.length} oldStackContent=${JSON.stringify(contentInfo)} importedConversational=${bCounts?.conversational ?? 'n/a'} importedUnknownBlocks=${bCounts?.unknownBlocks ?? 'n/a'} failures=${failures}`);
console.log(failures === 0 ? 'REAL FORMAT PROBE: BOTH KERNELS FAITHFUL' : 'REAL FORMAT PROBE: DIVERGENCE CONFIRMED (F-T07-1)');
process.exit(failures === 0 ? 0 : 1);
