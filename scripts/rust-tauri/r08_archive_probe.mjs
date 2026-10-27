// D-4 定位探针：Rust 迁移根上 rename 后 studio-list 标题是否回落。
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { DatabaseSync } from 'node:sqlite';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const work = '/private/tmp/r08-archive-probe3';
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

// 1) 初始列表
let r = await fetch(`${base}/lingxi/v1/sessions/studio-list`, { headers: H });
let list = await r.json();
const target = list.sessions[list.sessions.length - 1];
console.log('target:', target.sessionId);

// 2) 归档
r = await fetch(`${base}/lingxi/v1/sessions/archive`, { method: 'POST', headers: H, body: JSON.stringify({ sessionId: target.sessionId, childMode: 'archive_children' }) });
console.log('archive status:', r.status, JSON.stringify(await r.json().catch(() => null)));

// 3) 归档列表
r = await fetch(`${base}/lingxi/v1/sessions/archived`, { headers: H });
const arch = await r.json();
console.log('archived list keys:', Object.keys(arch), '| count:', (arch.sessions ?? arch).length);
console.log('contains target:', JSON.stringify(arch).includes(target.sessionId));

// 4) cleanup(30) 边界
r = await fetch(`${base}/lingxi/v1/sessions/cleanup`, { method: 'POST', headers: H, body: JSON.stringify({ maxAgeDays: 30 }) });
console.log('cleanup(30):', r.status, JSON.stringify(await r.json().catch(() => null)));

// 5) 再看归档列表
r = await fetch(`${base}/lingxi/v1/sessions/archived`, { headers: H });
const arch2 = await r.json();
console.log('after cleanup contains target:', JSON.stringify(arch2).includes(target.sessionId), '| count:', (arch2.sessions ?? arch2).length);

svc.kill('SIGTERM');
await sleep(1200);
try { svc.kill('SIGKILL'); } catch { /* gone */ }
console.log('ARCHIVE_PROBE_DONE');
