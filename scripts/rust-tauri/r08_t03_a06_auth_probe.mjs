#!/usr/bin/env node
/**
 * R08-T03 批次 E 前置探针：A06 设计口径「--bind <LAN-IP> --network-mode lan」下，
 * 桌面远端模式的认证通道与当前机器的 LAN 可达性。
 *
 * 背景（源码链，探测前已核）：
 *   - 服务侧：LAN 模式下同机经 LAN IP 接入 → ConnectionKind::Lan
 *     （transport.rs:201-206：host 头非 loopback 且 peer 非 loopback → Lan）。
 *   - auth.rs:2558-2566：loopback token 在非 Local 连接上恒拒绝
 *     （loopback_token_requires_local_transport）。
 *   - 桌面侧：readRemoteRustConnection（rust-local-service.cjs:369-372）只接受
 *     {"token":"<32 位小写 hex>"} 形状 = loopback token；设备凭证 secret 为
 *     "hana_dev_" + base64url(32)（auth.rs:81,2019），被该正则拒绝 →
 *     桌面远端模式没有任何呈现设备凭证的通道。
 *
 * 探针面（自适应，全部落在证据目录）：
 *   P0a 服务绑 0.0.0.0 + lan 模式，经 127.0.0.1 GET /health → 预期 200
 *       （服务本体健康、回环路径正常）。
 *   P0b 同一服务同一端口经本机 LAN IP GET /health → 记录可达性。
 *       本机实测：macOS Application Firewall 对 adhoc 签名 debug 二进制静默丢
 *       弃入站（对照：Apple 签名 nc 与下载签名 python3 同路径 200）。
 *   P0c python3 对照监听器绑同一 LAN IP → 预期 200（证明网络路径本身可用，
 *       拦截是按二进制生效）。
 *   P1/P2 仅当 P0b 可达时执行：经 LAN IP 的 identity 鉴权面实证
 *       （无凭证 → 401 missing_credential；loopback token → 401
 *       loopback_token_requires_local_transport）。不可达时记 env-blocked，
 *       产品层结论由上述源码链承载。
 *   P3  桌面 token 文件正则对 hana_dev_ 样本的判定（rust-local-service.cjs:370
 *       复算；设备凭证无客户端通道的静态事实）。
 *
 * 退出码：已执行面与预期不符 → 1；P1/P2 因 P0b 不可达而跳过不算失败。
 */

import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const RUST_BIN = path.join(REPO, 'rust', 'target', 'debug', 'lingxi-service');

function parseArgs(argv) {
  const args = { out: null };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === '--out') { args.out = argv[i + 1]; i += 1; }
    else throw new Error(`unknown arg: ${argv[i]}`);
  }
  if (!args.out) throw new Error('--out <evidence-dir> is required');
  return args;
}

function lanAddress() {
  for (const iface of ['en0', 'en1']) {
    try {
      const out = execFileSync('ipconfig', ['getifaddr', iface], { encoding: 'utf8' }).trim();
      if (/^\d+\.\d+\.\d+\.\d+$/.test(out)) return out;
    } catch { /* 接口无地址，试下一个 */ }
  }
  throw new Error('no LAN IPv4 address on en0/en1');
}

function sleep(ms) { return new Promise((resolve) => setTimeout(resolve, ms)); }

async function waitReady(proc, timeoutMs) {
  let buf = '';
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`READY timeout; stdout tail: ${buf.slice(-400)}`)), timeoutMs);
    proc.stdout.on('data', (chunk) => {
      buf += chunk.toString('utf8');
      const m = buf.match(/LINGXI_SERVICE_READY addr=(\S+) home=(\S+) source=(\S+)/);
      if (m) { clearTimeout(timer); resolve({ addr: m[1], home: m[2], source: m[3] }); }
    });
    proc.on('exit', (code) => { clearTimeout(timer); reject(new Error(`service exited ${code} before READY`)); });
  });
}

/** 返回 {status, body}；status=0 表示连接层失败/超时（message 进 body）。 */
async function probe(url, token, timeoutMs = 4000) {
  const headers = token ? { Authorization: `Bearer ${token}` } : {};
  try {
    const res = await fetch(url, { headers, signal: AbortSignal.timeout(timeoutMs), redirect: 'error' });
    return { status: res.status, body: (await res.text()).slice(0, 500) };
  } catch (err) {
    return { status: 0, body: `fetch error: ${err.message}` };
  }
}

/** P0c 对照：python3 内联 HTTP 监听器（下载签名二进制，ALF 自动放行类）。 */
function pythonControl(lan, port) {
  const code = [
    'import http.server,socketserver',
    'class H(http.server.BaseHTTPRequestHandler):',
    '    def do_GET(self):',
    '        self.send_response(200); self.end_headers(); self.wfile.write(b"ok")',
    '    def log_message(self,*a): pass',
    `socketserver.TCPServer(("${lan}",${port}),H).serve_forever()`,
  ].join('\n');
  return spawn('python3', ['-c', code], { stdio: ['ignore', 'ignore', 'pipe'] });
}

async function killTree(pid) {
  try { process.kill(pid, 'SIGTERM'); } catch { return; }
  const deadline = Date.now() + 10000;
  while (Date.now() < deadline) {
    try { process.kill(pid, 0); } catch { return; }
    await sleep(100);
  }
  try { process.kill(pid, 'SIGKILL'); } catch { /* 已退 */ }
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  fs.mkdirSync(args.out, { recursive: true });
  const report = { schema: 'r08-t03-a06-auth-probe/v2', checks: [], probes: {} };
  const check = (id, pass, detail) => {
    report.checks.push({ id, pass, detail });
    console.log(`${pass ? 'PASS' : 'FAIL'} ${id} :: ${detail}`);
  };

  const lan = lanAddress();
  const homeRaw = fs.mkdtempSync(path.join(os.tmpdir(), 'lingxi-r08-t03-a06-probe-'));
  const home = fs.realpathSync(homeRaw); // 非规范化 home 会撞 ELOOP（drill-03 已证）
  report.env = { lan, home, rustBin: RUST_BIN };

  // 最小种子：运行时目录 + models.json（闭合 schema：仅 providers/models/network；
  // 探针不触模型，endpoint 指到不可达回环即可）。
  fs.mkdirSync(path.join(home, 'lingxi-service'), { recursive: true });
  fs.writeFileSync(path.join(home, 'lingxi-service', 'models.json'), JSON.stringify({
    providers: {
      remote: {
        protocol: 'openai-completions',
        endpoint: 'http://127.0.0.1:1/v1',
        auth: { kind: 'apiKey', apiKey: 'R08_SYNTHETIC_ONLY' },
      },
    },
    models: {
      chat: { provider: 'remote', model: 'chat', capabilities: { tools: true } },
      title: { provider: 'remote', model: 'title', capabilities: { tools: false } },
    },
  }, null, 2) + '\n');

  const service = spawn(RUST_BIN, ['--bind', '0.0.0.0:0', '--network-mode', 'lan', '--home', home],
    { stdio: ['ignore', 'pipe', 'pipe'] });
  let stderrTail = '';
  service.stderr.on('data', (c) => { stderrTail = (stderrTail + c.toString('utf8')).slice(-2000); });
  report.servicePid = service.pid;
  let py = null;

  try {
    const ready = await waitReady(service, 20000);
    report.ready = ready;
    const port = Number(ready.addr.split(':').pop());
    const token = JSON.parse(
      fs.readFileSync(path.join(home, 'lingxi-service', 'local-token.json'), 'utf8')).token;
    check('token-shape', /^[0-9a-f]{32}$/.test(token),
      `local-token.json token matches desktop regex shape (len=${token.length})`);

    // P0a：回环路径健康。
    const p0a = await probe(`http://127.0.0.1:${port}/lingxi/v1/health`, null);
    report.probes.p0a_loopback_health = p0a;
    check('p0a-loopback-health-200', p0a.status === 200, `GET health via 127.0.0.1 -> ${p0a.status}`);

    // P0b：同机经 LAN IP 的可达性（本机预期被 ALF 静默丢弃 → status 0）。
    const p0b = await probe(`http://${lan}:${port}/lingxi/v1/health`, null);
    report.probes.p0b_lan_health = p0b;
    const lanReachable = p0b.status === 200;
    report.lanReachableForDebugBinary = lanReachable;
    console.log(`INFO p0b lan health via ${lan} -> status=${p0b.status} (${p0b.body.slice(0, 80)})`);

    // P0c：python3 对照（证明网络路径本身可用，拦截按二进制生效）。
    const pyPort = port + 1;
    py = pythonControl(lan, pyPort);
    await sleep(1200);
    const p0c = await probe(`http://${lan}:${pyPort}/`, null);
    report.probes.p0c_python_control = p0c;
    check('p0c-python-control-lan-200', p0c.status === 200,
      `python3 listener via ${lan}:${pyPort} -> ${p0c.status}`);

    if (lanReachable) {
      // P1/P2：LAN 可达时实证认证拒绝面。
      const base = `http://${lan}:${port}`;
      const p1 = await probe(`${base}/lingxi/v1/server/identity`, null);
      report.probes.p1_no_credential = p1;
      check('p1-missing-credential', p1.status === 401 && /missing_credential/.test(p1.body),
        `GET identity no-auth via LAN -> ${p1.status} ${p1.body.slice(0, 120)}`);
      const p2 = await probe(`${base}/lingxi/v1/server/identity`, token);
      report.probes.p2_loopback_token = p2;
      check('p2-loopback-token-refused-on-lan',
        p2.status === 401 && /loopback_token_requires_local_transport/.test(p2.body),
        `GET identity Bearer loopback-token via LAN -> ${p2.status} ${p2.body.slice(0, 160)}`);
    } else {
      report.probes.p1_p2_skipped = 'env-blocked: LAN path to debug binary filtered (see p0b/p0c); '
        + 'auth denial asserted via source chain transport.rs:201-206 + auth.rs:2558-2566';
      console.log('INFO p1/p2 skipped: LAN path env-blocked for adhoc-signed debug binary');
    }

    // P3：桌面 token 文件正则对设备凭证样本的判定（rust-local-service.cjs:370 复算）。
    const deviceSecretSample = `hana_dev_${'A'.repeat(43)}`; // base64url(32) 形状样本
    const desktopRegex = /^[0-9a-f]{32}$/;
    check('p3-desktop-regex-rejects-device-secret', !desktopRegex.test(deviceSecretSample),
      `desktop token regex rejects device secret sample "${deviceSecretSample.slice(0, 16)}..."`);
  } finally {
    if (py) await killTree(py.pid);
    await killTree(service.pid);
    fs.rmSync(homeRaw, { recursive: true, force: true });
  }
  if (stderrTail) report.stderrTail = stderrTail;

  const failed = report.checks.filter((c) => !c.pass);
  report.verdict = failed.length === 0
    ? (report.lanReachableForDebugBinary ? 'lan-reachable-auth-chain-probed' : 'env-blocked-lan-source-chain-stands')
    : 'probe-unexpected';
  fs.writeFileSync(path.join(args.out, 'auth-probe.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`verdict=${report.verdict} checks=${report.checks.length} failed=${failed.length}`);
  process.exit(failed.length === 0 ? 0 : 1);
}

main().catch((err) => { console.error(err.stack || String(err)); process.exit(2); });
