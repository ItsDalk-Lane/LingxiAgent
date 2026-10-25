// R08 ASK 缺省态探针：全新无偏好根上双内核「新会话默认权限」读数。
// 断言（叶 25C4FF66FEE5 断言0 的缺省分支 + 8D3CEB6133E1 断言语境）：
// 无偏好时 GET default permission → "ask"（旧栈 SESSION_PERMISSION_MODES.ASK；
// Rust 对齐）。A 腿 = 旧栈全新 home（first-run 后），B 腿 = Rust 全新 home。
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const bin = path.join(REPO, 'rust/target/debug/lingxi-service');
const work = '/private/tmp/r08-ask-default-probe';
fs.rmSync(work, { recursive: true, force: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function freePort() {
  const srv = net.createServer();
  await new Promise((res, rej) => { srv.once('error', rej); srv.listen(0, '127.0.0.1', res); });
  const { port } = srv.address();
  await new Promise((res) => srv.close(res));
  return port;
}
async function oldStackDefault() {
  const home = path.join(work, 'home-node');
  fs.mkdirSync(home, { recursive: true });
  const port = await freePort();
  const log = fs.openSync(path.join(work, 'old.log'), 'w');
  const env = { ...process.env, LINGXI_HOME: home, LINGXI_PORT: String(port) };
  for (const k of ['ELECTRON_RUN_AS_NODE', 'LINGXI_TOKEN', 'LINGXI_SERVER_OWNER', 'LINGXI_SERVER_OWNER_PID']) delete env[k];
  const child = spawn(process.execPath, [path.join(REPO, 'scripts/launch.js'), 'server'], { cwd: REPO, env, stdio: ['ignore', log, log] });
  fs.closeSync(log);
  const infoPath = path.join(home, 'server-info.json');
  let info = null;
  const deadline = Date.now() + 90000;
  while (Date.now() < deadline) {
    if (fs.existsSync(infoPath)) {
      try {
        const p = JSON.parse(fs.readFileSync(infoPath, 'utf8'));
        if (p.port === port && p.token) {
          try {
            const r = await fetch(`http://127.0.0.1:${port}/api/health`, { headers: { Authorization: `Bearer ${p.token}` }, signal: AbortSignal.timeout(2000) });
            if (r.status === 200) { info = p; break; }
          } catch { /* not yet */ }
        }
      } catch { /* half-written */ }
    }
    await sleep(250);
  }
  let result = 'old-stack-not-ready';
  if (info) {
    const r = await fetch(`http://127.0.0.1:${port}/api/preferences/session-permission-default`, { headers: { Authorization: `Bearer ${info.token}` } });
    const data = await r.json().catch(() => null);
    result = `status=${r.status} mode=${JSON.stringify(data?.permissionMode)}`;
    try { process.kill(info.pid, 'SIGTERM'); } catch { /* gone */ }
  }
  await sleep(1500);
  try { process.kill(child.pid, 'SIGKILL'); } catch { /* gone */ }
  child.kill('SIGKILL');
  return result;
}
async function rustDefault() {
  const home = path.join(work, 'home-rust');
  fs.mkdirSync(home, { recursive: true });
  const log = fs.openSync(path.join(work, 'rust.log'), 'w');
  const env = { ...process.env, LINGXI_PRODUCT_DIR: path.join(REPO, 'lib') };
  delete env.ELECTRON_RUN_AS_NODE;
  const svc = spawn(bin, ['--home', home, '--bind', '127.0.0.1:0'], { cwd: REPO, env, stdio: ['ignore', log, log] });
  fs.closeSync(log);
  let addr = null;
  for (let i = 0; i < 90 && !addr; i += 1) {
    const text = fs.existsSync(path.join(work, 'rust.log')) ? fs.readFileSync(path.join(work, 'rust.log'), 'utf8') : '';
    const m = text.match(/LINGXI_SERVICE_READY addr=(\S+) /);
    if (m) addr = m[1];
    await sleep(250);
  }
  let result = 'rust-not-ready';
  if (addr) {
    const token = (() => { const t = JSON.parse(fs.readFileSync(path.join(home, 'lingxi-service/local-token.json'), 'utf8')); return t.token ?? t.localToken ?? t; })();
    const r = await fetch(`http://${addr}/lingxi/v1/preferences/session-permission-default`, { headers: { authorization: `Bearer ${token}` } });
    const data = await r.json().catch(() => null);
    result = `status=${r.status} mode=${JSON.stringify(data?.permissionMode)}`;
  }
  svc.kill('SIGTERM');
  await sleep(1200);
  try { svc.kill('SIGKILL'); } catch { /* gone */ }
  return result;
}
const a = await oldStackDefault();
const b = await rustDefault();
console.log(`ASK_DEFAULT node=[${a}] rust=[${b}]`);
console.log(a.includes('mode="ask"') && b.includes('mode="ask"') ? 'ASK_DEFAULT_OK' : 'ASK_DEFAULT_DIVERGES');
process.exit(a.includes('mode="ask"') && b.includes('mode="ask"') ? 0 : 1);
