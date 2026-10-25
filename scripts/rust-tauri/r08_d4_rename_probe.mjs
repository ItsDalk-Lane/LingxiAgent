// D-4 定位探针：Rust 迁移根上 rename 后 studio-list 标题是否回落。
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { DatabaseSync } from 'node:sqlite';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const work = '/private/tmp/r08-d4-probe';
const bin = path.join(REPO, 'rust/target/debug/lingxi-service');
fs.rmSync(work, { recursive: true, force: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// 迁移根：fixture → migrate
const fixture = path.join(work, 'fixture');
const gen = spawnSync(process.execPath, [path.join(REPO, 'scripts/rust-tauri/r08_t04_fixture_gen.mjs'), '--seed', '20261023', '--out', fixture], { encoding: 'utf8' });
if (gen.status !== 0) { console.log('fixture gen failed'); process.exit(1); }
const rootB = path.join(work, 'root-b');
const migrate = spawnSync(bin, ['migrate', '--source', fixture, '--target', rootB, '--work-dir', path.join(work, 'mw'), '--backup-dir', path.join(work, 'mb'), '--pointer', path.join(work, 'ptr.json')], { encoding: 'utf8' });
if (migrate.status !== 0) { console.log('migrate failed:', migrate.stderr?.slice(-200)); process.exit(1); }

// 起服务
const log = fs.openSync(path.join(work, 'svc.log'), 'w');
const env = { ...process.env, LINGXI_PRODUCT_DIR: path.join(REPO, 'lib') };
delete env.ELECTRON_RUN_AS_NODE;
const svc = spawn(bin, ['--home', rootB, '--bind', '127.0.0.1:0'], { cwd: REPO, env, stdio: ['ignore', log, log] });
fs.closeSync(log);
let addr = null;
for (let i = 0; i < 90 && !addr; i += 1) {
  const text = fs.existsSync(path.join(work, 'svc.log')) ? fs.readFileSync(path.join(work, 'svc.log'), 'utf8') : '';
  const m = text.match(/LINGXI_SERVICE_READY addr=(\S+) /);
  if (m) addr = m[1];
  await sleep(250);
}
if (!addr) { console.log('not ready'); svc.kill('SIGKILL'); process.exit(1); }
const token = (() => { const t = JSON.parse(fs.readFileSync(path.join(rootB, 'lingxi-service/local-token.json'), 'utf8')); return t.token ?? t.localToken ?? t; })();
const H = { authorization: `Bearer ${token}`, 'content-type': 'application/json' };
const base = `http://${addr}`;

// 1) 初始列表 → 取第一个会话
let r = await fetch(`${base}/lingxi/v1/sessions/studio-list`, { headers: H });
let list = await r.json();
const first = list.sessions[0];
console.log('before rename:', first.sessionId, '| title:', JSON.stringify(first.title), '| revision:', first.revision);

// 2) DB 直查（rename 前）
{
  const db = new DatabaseSync(path.join(rootB, 'lingxi-service/data/runs.db'), { readOnly: true });
  const row = db.prepare('SELECT session_id, title FROM sessions WHERE session_id=?').get(first.sessionId);
  console.log('DB before:', JSON.stringify(row));
  db.close();
}

// 3) rename
r = await fetch(`${base}/lingxi/v1/sessions/rename`, { method: 'POST', headers: H, body: JSON.stringify({ sessionId: first.sessionId, title: 'D4-PROBE-RENAMED' }) });
console.log('rename status:', r.status, JSON.stringify(await r.json().catch(() => null)));

// 4) DB 直查（rename 后）
{
  const db = new DatabaseSync(path.join(rootB, 'lingxi-service/data/runs.db'), { readOnly: true });
  const row = db.prepare('SELECT session_id, title FROM sessions WHERE session_id=?').get(first.sessionId);
  console.log('DB after:', JSON.stringify(row));
  db.close();
}

// 5) 重载列表（立即 / 500ms 后 / 2s 后）
for (const wait of [0, 500, 2000]) {
  await sleep(wait);
  r = await fetch(`${base}/lingxi/v1/sessions/studio-list`, { headers: H });
  list = await r.json();
  const row = list.sessions.find((s) => s.sessionId === first.sessionId);
  console.log(`list after ${wait}ms:`, row ? `title=${JSON.stringify(row.title)} revision=${row.revision}` : 'MISSING');
}

svc.kill('SIGTERM');
await sleep(1200);
try { svc.kill('SIGKILL'); } catch { /* gone */ }
console.log('D4_PROBE_DONE');
