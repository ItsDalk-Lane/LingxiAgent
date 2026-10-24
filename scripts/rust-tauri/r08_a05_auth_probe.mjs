// R08 家族#9 根因探针：a05 读腿授权链——POST /lingxi/v1/sessions 带 authorizedFolders
// 是否真正落库（sessions.authorized_folders）+ runs.rs replace_session_roots 是否生效。
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { DatabaseSync } from 'node:sqlite';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const work = '/private/tmp/r08-a05-auth-probe';
const bin = path.join(REPO, 'rust/target/debug/lingxi-service');
fs.rmSync(work, { recursive: true, force: true });
const home = path.join(work, 'home');
fs.mkdirSync(home, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const env = { ...process.env, LINGXI_PRODUCT_DIR: path.join(REPO, 'lib') };
delete env.ELECTRON_RUN_AS_NODE;

async function bootOnce() {
  for (const f of fs.readdirSync(work).filter((x) => x.startsWith('svc-'))) fs.rmSync(path.join(work, f));
  const log = fs.openSync(path.join(work, `svc-${Date.now()}.log`), 'w');
  const svc = spawn(bin, ['--home', home, '--bind', '127.0.0.1:0'], { cwd: REPO, env, stdio: ['ignore', log, log] });
  fs.closeSync(log);
  let addr = null;
  for (let i = 0; i < 90 && !addr; i += 1) {
    const files = fs.readdirSync(work).filter((f) => f.startsWith('svc-')).sort();
    const text = files.length ? fs.readFileSync(path.join(work, files[files.length - 1]), 'utf8') : '';
    const m = text.match(/LINGXI_SERVICE_READY addr=(\S+) /);
    if (m) addr = m[1];
    await sleep(250);
  }
  return { svc, addr };
}

// 第一刀：空 home 自盖章（epoch 闸白名单只允许 runtime 目录）
const first = await bootOnce();
first.svc.kill('SIGTERM');
await sleep(1200);
try { first.svc.kill('SIGKILL'); } catch { /* gone */ }
if (!fs.existsSync(path.join(home, 'data-epoch.json'))) { console.log('PROBE_FAIL stamp boot'); process.exit(1); }

// 第二刀：盖章后播种读腿目标
fs.mkdirSync(path.join(home, 'a05-drill'), { recursive: true });
fs.writeFileSync(path.join(home, 'a05-drill/read-note.txt'), 'PROBE-READ-42\n');

// 第二次启动：正式探测
const second = await bootOnce();
const svc = second.svc;
const addr = second.addr;
if (!addr) { console.log('PROBE_FAIL service not ready'); svc.kill('SIGKILL'); process.exit(1); }
const token = (() => { const t = JSON.parse(fs.readFileSync(path.join(home, 'lingxi-service/local-token.json'), 'utf8')); return t.token ?? t.localToken ?? t; })();
const base = `http://${addr}`;

// 1) agent 补建（迁移无 roster 同因——fresh home 自建默认 agent? 探针显式建）
let r = await fetch(`${base}/lingxi/v1/agents`, { method: 'POST', headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' }, body: JSON.stringify({ name: 'probe', id: 'lingxi', yuan: 'lingxi' }) });
console.log('agents create:', r.status);

// 2) 会话创建带 authorizedFolders
r = await fetch(`${base}/lingxi/v1/sessions`, { method: 'POST', headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' }, body: JSON.stringify({ sessionId: 'a05-read-session', agentId: 'lingxi', title: 'probe', authorizedFolders: [path.join(home, 'a05-drill')] }) });
const created = await r.json().catch(() => null);
console.log('session create:', r.status, JSON.stringify(created)?.slice(0, 200));

// 3) DB 落库核对
await sleep(500);
svc.kill('SIGTERM');
await sleep(1200);
try { svc.kill('SIGKILL'); } catch { /* gone */ }
try {
  const db = new DatabaseSync(path.join(home, 'lingxi-service/data/runs.db'), { readOnly: true });
  const row = db.prepare("SELECT session_id, authorized_folders_json FROM sessions WHERE session_id='a05-read-session'").get();
  console.log('DB row:', JSON.stringify(row));
  db.close();
} catch (e) {
  console.log('DB read failed:', e.message);
}
console.log('A05_AUTH_PROBE_DONE');
