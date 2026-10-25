#!/usr/bin/env node
/**
 * R08-T03 批次 E / 验收 A06：两客户端共连同一 Rust 测试服务（remote 模式）。
 *
 * 场景（任务书 R08-T03 A06 + Lead batch-E 裁决 (a) 回环退路）：
 *   harness 直启 lingxi-service（--bind 127.0.0.1:0，默认回环模式，合成 HOME），
 *   两个旧 Electron 客户端 A/B 各自独立合成 HOME，均走 REMOTE 分支
 *   （LINGXI_DESKTOP_RUST_REMOTE_URL=http://127.0.0.1:<port> +
 *   LINGXI_DESKTOP_RUST_REMOTE_TOKEN_FILE=0600 私钥文件，回环明文豁免
 *   ALLOW_CLEARTEXT）。核心验收语义全部硬断言：
 *
 *   1. remote 分支结构性零接触：A/B HOME 下永不出现 lingxi-service/instance.json；
 *      进程树（harness 子树）恰好 1 个 rust-kernel（=harness 服务 pid）；
 *      node-kernel=0（S1/S2/S3 三点硬断言，Lead 提醒③）。
 *   2. remote 分支确证（Lead batch-E 新增硬断言）：两端 GSCI
 *      host==='127.0.0.1'（local-owned/local-reused 恒为 null）+
 *      serverNodeKind==='lingxi-service' + port===夹具端口。若客户端悄悄落回
 *      local 分支，本断言即红，绝不降级为记录。
 *   3. 双客户端共连：任务腿从客户端 A 渲染进程内发起（CDP 在 A 页面上下文执行
 *      与真实客户端相同的 wsTicket + wire 协议流程，连接真实源自 A 进程），
 *      leg-a1 在双端在线时完成。
 *   4. 退 B 后 A 继续工作：Cmd+Q 定向退 B（AXRaise+frontmost by unix id，再经 CDP
 *      核实 B 渲染进程 document.hasFocus() 且 A 不持焦点后才发键），B 进程收敛、A 未被误伤；
 *      服务 pid/instanceId 不变、health 200；A 端 leg-a2 仍完成。
 *   5. 退 A 后服务仍存活（对客户端退出零接触），harness 负责回收服务并记录退出码。
 *
 * 显式偏差（Lead 裁决 (a) 已复核）：传输层为回环而非真实 LAN。真 LAN e2e 双重
 * 阻断 = 环境层 macOS ALF 对 adhoc 签名 debug 二进制静默丢包（auth-probe-04
 * 绿证：0.0.0.0 绑定下回环 200/LAN 超时、python3/nc 对照 200）+ 产品层
 * PPG-3（设备凭证无客户端呈现通道：32hex 正则拒 "hana_dev_" 形状；签发面
 * POST /lingxi/v1/devices/credentials 挂 local_only）。PPG-3 登记
 * T04/T05/T08 MUST-ADJUDICATE，与 PPG-1/2 并列。
 *
 * 纪律：任何断言失败 → verdict=fail + 非零退出，绝不降级为「记录但不断言」。
 * 键盘驱动走 AppleScript，当前仅 darwin 实装；其他平台显式 FAIL（不虚报）。
 * 用法：
 *   node scripts/rust-tauri/r08_t03_a06_drill.mjs --out <evidence-subdir>
 */
import { spawn, spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const __dirname = path.dirname(fileURLToPath(import.meta.url));

const ROOT = path.resolve(__dirname, '..', '..');
const TREE_RECORDER = path.join(__dirname, 'r08_t03_process_tree.mjs');
const STUB_PROVIDER = path.join(__dirname, 'r08_t03_stub_provider.mjs');
const RUST_BIN = path.join(ROOT, 'rust', 'target', 'debug', 'lingxi-service');

// 远端形态标题带「（远端）」后缀（boot.ts:186，键 = boot.host 非空）——
// 标题本身就是 remote 分支的第二重 oracle。
const EXPECT_TITLE = 'Lingxi · Rust 预览（远端）';
const CHAT_MARKER = 'A06-CHAT-TASK';

const failures = [];
const journalEntries = [];
let crashOutDir = null;
let crashHomes = [];
const quitMechanisms = {};

function parseArgs(argv) {
  const args = { out: null };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === '--out') { args.out = path.resolve(argv[i + 1]); i += 1; }
  }
  if (!args.out) throw new Error('usage: r08_t03_a06_drill.mjs --out <dir>');
  return args;
}

function journal(step, data) {
  const entry = { at: new Date().toISOString(), step, data };
  journalEntries.push(entry);
  console.log(`[journal] ${step} ${JSON.stringify(data)}`);
}

function fail(step, message) {
  failures.push({ step, message });
  console.error(`[FAIL] ${step}: ${message}`);
}

function assert(step, condition, message) {
  if (condition) { console.log(`[PASS] ${step}: ${message}`); return; }
  fail(step, `断言失败 — ${message}`);
}

function sleep(ms) { return new Promise((resolve) => { setTimeout(resolve, ms); }); }

async function waitFor(step, predicate, timeoutMs, intervalMs = 500) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    try {
      const value = await predicate();
      if (value) return value;
    } catch { /* 谓词内部错误按未就绪处理，超时时统一显式 FAIL */ }
    if (Date.now() > deadline) throw new Error(`${step}: timeout after ${timeoutMs}ms`);
    await sleep(intervalMs);
  }
}

function pidAlive(pid) {
  try { process.kill(pid, 0); return true; } catch { return false; }
}

function spawnLogged(command, argv, options, logFile) {
  const out = fs.openSync(logFile, 'a');
  const child = spawn(command, argv, { ...options, stdio: ['ignore', out, out] });
  fs.closeSync(out);
  return child;
}

// ── 构建产物新鲜度（Lead 提醒①）：与 A05 同款门禁 ──
function ensureFreshBuild(step, outDir) {
  const stamp = {
    rendererMtimeMs: null, mainNewer: [], preloadNewer: [], srcNewer: [],
  };
  const rendererDir = path.join(ROOT, 'desktop', 'dist-renderer');
  const indexHtml = path.join(rendererDir, 'index.html');
  if (!fs.existsSync(indexHtml)) {
    fail(step, `dist-renderer/index.html 缺失 — 先构建渲染层再开演练`);
    return stamp;
  }
  stamp.rendererMtimeMs = fs.statSync(indexHtml).mtimeMs;
  const newer = (file) => {
    try { return fs.statSync(file).mtimeMs > stamp.rendererMtimeMs; } catch { return false; }
  };
  if (newer(path.join(ROOT, 'desktop', 'main.cjs'))) stamp.mainNewer.push('desktop/main.cjs');
  if (newer(path.join(ROOT, 'desktop', 'preload.cjs'))) stamp.preloadNewer.push('desktop/preload.cjs');
  const walk = (dir, bucket, budget) => {
    for (const name of fs.readdirSync(dir)) {
      if (budget.count <= 0) return;
      const p = path.join(dir, name);
      let st = null;
      try { st = fs.statSync(p); } catch { continue; }
      if (st.isDirectory()) { walk(p, bucket, budget); continue; }
      budget.count -= 1;
      if (st.mtimeMs > stamp.rendererMtimeMs) bucket.push(path.relative(ROOT, p));
    }
  };
  walk(path.join(ROOT, 'desktop', 'src'), stamp.srcNewer, { count: 4000 });
  fs.writeFileSync(path.join(outDir, 'build-freshness.json'), JSON.stringify(stamp, null, 2) + '\n');
  assert(step, stamp.mainNewer.length === 0 && stamp.preloadNewer.length === 0 && stamp.srcNewer.length === 0,
    `构建产物新鲜（main/preload/src 均无新于 dist-renderer 的改动；实测 main=${stamp.mainNewer.length} preload=${stamp.preloadNewer.length} src=${stamp.srcNewer.length}）`);
  if (!fs.existsSync(RUST_BIN)) fail(step, `rust binary missing: ${RUST_BIN}`);
  return stamp;
}

// ── 替身供应商（与 A05 同桩；A06 仅用 CHAT 腿，零工具接触）──
async function startStubProvider(outDir) {
  const child = spawnLogged(process.execPath, [
    STUB_PROVIDER, '--port', '0',
    '--journal', path.join(outDir, 'stub-journal.jsonl'),
    '--read-path', 'a06-drill/unused.txt',
    '--final-delay-ms', '800',
  ], { cwd: ROOT }, path.join(outDir, 'stub-stdout.log'));
  const port = await waitFor('stub-ready', () => {
    let text = '';
    try { text = fs.readFileSync(path.join(outDir, 'stub-stdout.log'), 'utf8'); } catch { return null; }
    const match = text.match(/A05_STUB_READY (\d+)/);
    return match ? Number(match[1]) : null;
  }, 20000, 200);
  return { child, port };
}

/** 服务 HOME 第一刀：只放 runtime 目录（epoch 闸白名单），服务自行盖章。 */
function seedRuntimeOnly(home, stubPort) {
  fs.mkdirSync(path.join(home, 'lingxi-service'), { recursive: true });
  fs.writeFileSync(path.join(home, 'lingxi-service', 'models.json'), JSON.stringify({
    providers: {
      remote: {
        protocol: 'openai-completions',
        endpoint: `http://127.0.0.1:${stubPort}/v1`,
        auth: { kind: 'apiKey', apiKey: 'R08_SYNTHETIC_ONLY' },
      },
    },
    models: {
      chat: {
        provider: 'remote', model: 'chat',
        capabilities: { tools: true },
        compat: { contextWindow: 200000 },
      },
      title: {
        provider: 'remote', model: 'title',
        capabilities: { tools: false },
        compat: { contextWindow: 200000 },
      },
    },
  }, null, 2) + '\n');
}

/** 盖章用一次性启动（与 A05 同款：服务对 runtime-only HOME 走自盖章路径）。 */
async function stampSyntheticHome(home, outDir) {
  const env = { ...process.env };
  delete env.ELECTRON_RUN_AS_NODE;
  const child = spawnLogged(RUST_BIN, ['--home', home], { cwd: ROOT, env },
    path.join(outDir, 'stamp-boot.log'));
  const stampPath = path.join(home, 'data-epoch.json');
  try {
    await waitFor('stamp', () => fs.existsSync(stampPath), 45000, 200);
  } finally {
    child.kill('SIGTERM');
    await new Promise((resolve) => {
      child.once('exit', resolve);
      setTimeout(() => { child.kill('SIGKILL'); resolve(); }, 15000).unref();
    });
  }
  const stamp = JSON.parse(fs.readFileSync(stampPath, 'utf8'));
  journal('stamp', { stamp });
  assert('stamp', stamp.schemaVersion === 2,
    `服务自盖章 data-epoch.json schemaVersion===2（实测 ${stamp.schemaVersion}）`);
}

/** 服务 HOME 第二刀：agent/元神内容（A06 无读腿种子、无桌面偏好）。 */
function seedServiceHomeRest(home) {
  fs.mkdirSync(path.join(home, 'agents', 'lingxi'), { recursive: true });
  fs.writeFileSync(path.join(home, 'agents', 'lingxi', 'config.yaml'),
    'locale: zh-CN\nagent:\n  name: R08-A06\n  yuan: lingxi\nmemory:\n  enabled: false\ncompaction:\n  enabled: false\n');
  fs.mkdirSync(path.join(home, 'product', 'yuan'), { recursive: true });
  fs.writeFileSync(path.join(home, 'product', 'yuan', 'lingxi.md'),
    '你叫{{agentName}}，与用户一起完成任务。\n');
}

/** 客户端 HOME：remote 分支不跑服务、无 epoch 闸；只需跳过 onboarding。 */
function seedClientHome(home) {
  fs.mkdirSync(path.join(home, 'user'), { recursive: true });
  fs.writeFileSync(path.join(home, 'user', 'preferences.json'),
    JSON.stringify({ setupComplete: true }) + '\n');
}

function readRuntimeRecords(home) {
  const runtime = path.join(home, 'lingxi-service');
  const instance = JSON.parse(fs.readFileSync(path.join(runtime, 'instance.json'), 'utf8'));
  const credential = JSON.parse(fs.readFileSync(path.join(runtime, 'local-token.json'), 'utf8'));
  const url = new URL(`${instance.transport || 'http'}://${instance.bindAddr}`);
  return {
    pid: instance.pid,
    instanceId: instance.instanceId,
    port: Number(url.port),
    baseUrl: url.origin,
    token: credential.token,
  };
}

function httpJson(step, method, url, token, body) {
  return new Promise((resolve, reject) => {
    const req = http.request(url, {
      method,
      headers: {
        Authorization: `Bearer ${token}`,
        'Content-Type': 'application/json',
      },
    }, (res) => {
      const chunks = [];
      res.on('data', (c) => chunks.push(c));
      res.on('end', () => {
        const text = Buffer.concat(chunks).toString('utf8');
        let value = null;
        try { value = JSON.parse(text); } catch { /* 保留原文进证据 */ }
        resolve({ status: res.statusCode, headers: res.headers, value, text });
      });
    });
    req.on('error', reject);
    req.setTimeout(30000, () => { req.destroy(new Error(`${step}: http timeout`)); });
    if (body !== undefined) req.write(JSON.stringify(body));
    req.end();
  });
}

function snapshotTree(step, outDir, label, rootPid) {
  const out = path.join(outDir, `tree-${label}.json`);
  const argv = [TREE_RECORDER, '--out', out, '--label', label];
  if (rootPid) argv.push('--root-pid', String(rootPid));
  const result = spawnSync(process.execPath, argv, { cwd: ROOT, encoding: 'utf8' });
  if (result.status !== 0) {
    fail(step, `tree recorder exit ${result.status}: ${result.stderr || result.stdout}`);
    return null;
  }
  return JSON.parse(fs.readFileSync(out, 'utf8'));
}

/**
 * Lead 提醒③：node-kernel=0 是硬断言。A06 断言域 = harness 进程子树
 * （同机队友可能持有无关 rust/node 进程，全局计数只记录、不断言；
 * 语义 = 本演练链自身不得引入 node 内核）。
 * subtree 无 counts 字段（recorder 只给 descendants），此处自行归类计数；
 * electron-ui 主进程 = 无 --type= 参数者（Helper 均带 --type=）。
 */
function assertSubtreeTree(step, tree, { nodeKernel = 0, rustKernel = null, electronMains = null }) {
  if (!tree || !tree.subtree) { fail(step, 'tree snapshot/subtree missing'); return; }
  const desc = tree.subtree.descendants || [];
  const counts = { 'rust-kernel': 0, 'node-kernel': 0 };
  let mains = 0;
  for (const r of desc) {
    if (r.class === 'rust-kernel') counts['rust-kernel'] += 1;
    if (r.class === 'node-kernel') counts['node-kernel'] += 1;
    if (r.class === 'electron-ui' && !(r.args || '').includes('--type=')) mains += 1;
  }
  assert(step, counts['node-kernel'] === nodeKernel,
    `harness 子树 node-kernel count ${counts['node-kernel']} !== ${nodeKernel}（硬断言）`);
  if (rustKernel !== null) {
    assert(step, counts['rust-kernel'] === rustKernel,
      `harness 子树 rust-kernel count ${counts['rust-kernel']} !== ${rustKernel}`);
  }
  if (electronMains !== null) {
    assert(step, mains === electronMains,
      `harness 子树 Electron 主进程 count ${mains} !== ${electronMains}`);
  }
}

function cdpListTargets(port) {
  return new Promise((resolve, reject) => {
    const req = http.request({ host: '127.0.0.1', port, path: '/json/list', method: 'GET' }, (res) => {
      const chunks = [];
      res.on('data', (c) => chunks.push(c));
      res.on('end', () => {
        try { resolve(JSON.parse(Buffer.concat(chunks).toString('utf8'))); }
        catch (err) { reject(err); }
      });
    });
    req.on('error', reject);
    req.setTimeout(10000, () => req.destroy(new Error('cdp list timeout')));
    req.end();
  });
}

async function cdpEvaluateClient(client, expression, awaitPromise = true, timeoutMs = 20000) {
  // 缓存的 webSocketDebuggerUrl 在渲染目标回收后连接拒绝（t09-a06-standalone
  // 03/04 红实证：connInfoA2 抛 ECONNREFUSED 跳过后续全部断言）。重解析一次
  // 目标再试；进程已死则 findMainPageTarget 超时响亮失败——语义不被掩盖。
  try {
    return await cdpEvaluate(client.page.webSocketDebuggerUrl, expression, awaitPromise, timeoutMs);
  } catch (error) {
    if (!/ECONNREFUSED|EPIPE|closed before|not opened/i.test(String(error && error.message || error))) throw error;
    const page = await findMainPageTarget(client.cdpPort);
    if (!page) throw error;
    client.page = page;
    return cdpEvaluate(page.webSocketDebuggerUrl, expression, awaitPromise, timeoutMs);
  }
}

async function cdpEvaluate(wsUrl, expression, awaitPromise = true, timeoutMs = 20000) {
  // 作用域纪律（drill-03/04/05 教训）：promise 回调局部变量不得遮蔽外层 sock。
  const sock = await new Promise((resolve, reject) => {
    const WebSocketImpl = require('ws');
    const inner = new WebSocketImpl(wsUrl);
    inner.once('open', () => resolve(inner));
    inner.once('error', reject);
  });
  let seq = 0;
  const send = (method, params) => new Promise((resolve, reject) => {
    seq += 1;
    const id = seq;
    const timer = setTimeout(() => reject(new Error(`cdp ${method} timeout`)), timeoutMs);
    const onMessage = (data) => {
      const msg = JSON.parse(data.toString('utf8'));
      if (msg.id === id) {
        clearTimeout(timer);
        sock.off('message', onMessage);
        if (msg.error) reject(new Error(`cdp ${method}: ${JSON.stringify(msg.error)}`));
        else resolve(msg.result);
      }
    };
    sock.on('message', onMessage);
    sock.send(JSON.stringify({ id, method, params }));
  });
  try {
    await send('Runtime.enable', {});
    const result = await send('Runtime.evaluate', {
      expression, awaitPromise, returnByValue: true,
    });
    if (result.exceptionDetails) {
      throw new Error(`renderer threw: ${JSON.stringify(result.exceptionDetails).slice(0, 500)}`);
    }
    return result.result ? result.result.value : undefined;
  } finally {
    sock.close();
  }
}

async function findMainPageTarget(cdpPort) {
  return waitFor('cdp-main-page', async () => {
    let targets = [];
    try { targets = await cdpListTargets(cdpPort); } catch { return null; }
    const page = targets.find((t) => t.type === 'page' && /index\.html/.test(t.url || ''));
    return page || null;
  }, 90000, 1000);
}

function osascript(script) {
  const result = spawnSync('osascript', ['-e', script], { encoding: 'utf8' });
  return { status: result.status, stdout: (result.stdout || '').trim(), stderr: (result.stderr || '').trim() };
}

/**
 * 定向 Cmd+Q：双 Electron 实例同名且同 bundle。System Events 的
 * `set frontmost of p` / `frontmost of p` 按 bundle 而非 unix id 回答——
 * gate14 红实证：对 B 设前台、frontmost 回 true，Cmd+Q 却退了 A
 * （quit-b-keystroke-1 → electron-A-exit）。此前键击被 TCC 拦截时走
 * SIGTERM 兜底把这一缺陷盖住了。
 *
 * 键击前以 CDP 在目标渲染进程读 document.hasFocus()（按 pid 的真实键盘焦点，
 * 与 bundle 无关），并确认其余存活客户端 hasFocus()!==true；核实不到绝不盲发
 * Cmd+Q（盲发会退掉任意前台应用），直接走已披露的 SIGTERM 兜底。
 */
async function quitClientByPid(step, target, others = []) {
  const { pid, electron: electronChild } = target;
  const raiseScript = `
tell application "System Events"
  set p to first process whose unix id is ${pid}
  try
    perform action "AXRaise" of (first window of p)
  end try
  set frontmost of p to true
end tell`;
  const keystrokeScript = 'tell application "System Events" to keystroke "q" using {command down}';
  const readFocus = async (client) => {
    if (!pidAlive(client.pid)) return 'exited';
    try { return await cdpEvaluateClient(client, 'document.hasFocus()', false, 5000); } catch { return 'cdp-error'; }
  };
  const focusState = async () => {
    const state = { target: await readFocus(target), others: {} };
    for (const client of others) state.others[client.name] = await readFocus(client);
    return state;
  };
  const focusVerified = (state) => state.target === true
    && Object.values(state.others).every((value) => value !== true);
  let mechanism = 'keyboard';
  let exited = false;
  for (let attempt = 1; attempt <= 2 && pidAlive(pid); attempt += 1) {
    journal(`${step}-raise-${attempt}`, osascript(raiseScript));
    let state = await focusState();
    const deadline = Date.now() + 8000;
    while (!focusVerified(state) && Date.now() < deadline) {
      await sleep(250);
      state = await focusState();
    }
    journal(`${step}-focus-${attempt}`, { verified: focusVerified(state), state });
    if (!focusVerified(state)) continue;
    journal(`${step}-keystroke-${attempt}`, osascript(keystrokeScript));
    try { await waitFor(`${step}-exit-${attempt}`, () => !pidAlive(pid), 20000); exited = true; } catch { /* 下一轮/兜底 */ }
  }
  if (pidAlive(pid)) {
    mechanism = 'sigterm-graceful';
    journal(`${step}-sigterm`, { pid, reason: 'keyboard-path-exhausted-or-focus-unverified' });
    try { electronChild.kill('SIGTERM'); } catch { /* 已退出 */ }
    await waitFor(`${step}-sigterm-exit`, () => !pidAlive(pid), 30000);
    exited = true;
  }
  const collateral = others.filter((client) => !pidAlive(client.pid)).map((client) => client.name);
  return { mechanism, exited, collateral };
}

/**
 * 任务腿 = 在指定客户端渲染进程内执行与真实客户端相同的连接链：
 * GSCI 取连接信息 → POST /lingxi/v1/ws-ticket（Bearer）→ WS ?wsTicket= →
 * wire handshake → prompt → 等 assistant_run_end + status idle。
 * 连接真实源自该客户端进程（其网络栈/Origin 路径），非 harness 代发。
 */
function buildTaskExpression(sessionId, leg) {
  return `(async () => {
  const SESSION_ID = ${JSON.stringify(sessionId)};
  const LEG = ${JSON.stringify(leg)};
  const MARKER = ${JSON.stringify(CHAT_MARKER)};
  const EXPECT_FINAL = 'A06-CHAT-FINAL ' + LEG;
  const info = await window.hana.getServerConnectionInfo();
  if (!info || !info.port || !info.token) throw new Error('GSCI missing port/token');
  const host = info.host || '127.0.0.1';
  const base = 'http://' + host + ':' + info.port;
  const headers = { Authorization: 'Bearer ' + info.token, 'Content-Type': 'application/json' };
  const created = await fetch(base + '/lingxi/v1/sessions', {
    method: 'POST', headers,
    body: JSON.stringify({ sessionId: SESSION_ID, agentId: 'lingxi', title: SESSION_ID }),
  });
  if (created.status !== 201) throw new Error('session create status ' + created.status);
  const ticketRes = await fetch(base + '/lingxi/v1/ws-ticket', { method: 'POST', headers });
  if (!ticketRes.ok) throw new Error('ws-ticket status ' + ticketRes.status);
  const ticketBody = await ticketRes.json();
  const ticket = ticketBody && ticketBody.ticket;
  if (typeof ticket !== 'string' || !ticket) throw new Error('ws-ticket missing');
  return await new Promise((resolve, reject) => {
    const frames = [];
    const ws = new WebSocket('ws://' + host + ':' + info.port + '/lingxi/v1/ws?wsTicket=' + encodeURIComponent(ticket));
    const timer = setTimeout(() => {
      try { ws.close(); } catch {}
      reject(new Error('wire task timeout; frames=' + frames.length));
    }, 100000);
    const done = (fn, value) => { clearTimeout(timer); try { ws.close(); } catch {}; fn(value); };
    ws.onopen = () => {
      ws.send(JSON.stringify({
        protocol: 'lingxi.wire', clientKind: 'old-electron', clientVersion: '0',
        protocolMin: 1, protocolMax: 1,
      }));
    };
    ws.onerror = () => done(reject, new Error('ws error; frames=' + frames.length));
    ws.onclose = (ev) => {
      if (!ws.__a06Done) done(reject, new Error('ws close before run end code=' + ev.code + '; frames=' + frames.length));
    };
    let prompted = false;
    let runEnded = null;
    let idle = false;
    ws.onmessage = (ev) => {
      let frame = null;
      try { frame = JSON.parse(ev.data); } catch { return; }
      frames.push(frame);
      if (!prompted && frame && frame.protocol === 'lingxi.wire') {
        prompted = true;
        ws.send(JSON.stringify({ type: 'prompt', sessionId: SESSION_ID, text: '请回复标记。（标记 ' + MARKER + ' ' + LEG + '）' }));
      }
      // wire 帧可跨会话广播：仅认本会话（或无 sessionId 的连接级帧）。
      const mine = !frame.sessionId || frame.sessionId === SESSION_ID;
      if (mine && frame.type === 'assistant_run_end') runEnded = frame;
      if (mine && frame.type === 'status' && frame.isStreaming === false) idle = true;
      if (runEnded && idle) {
        ws.__a06Done = true;
        const blob = JSON.stringify(frames);
        done(resolve, {
          sessionId: SESSION_ID,
          frameCount: frames.length,
          // 成功 run_end 无 reason 字段（A05 drill-08 实测：失败才带
          // reason:"failed.provider_error"）；成功 = 帧存在且 reason 为空。
          runEnded: true,
          runEndReason: runEnded.reason ?? null,
          sawFinal: blob.includes(EXPECT_FINAL),
          sawMarker: blob.includes(MARKER + ' ' + LEG),
        });
      }
    };
  });
})()`;
}

async function main() {
  const args = parseArgs(process.argv);
  fs.mkdirSync(args.out, { recursive: true });
  const outDir = args.out;
  crashOutDir = outDir;

  if (process.platform !== 'darwin') {
    fail('platform', 'A06 键盘驱动退出链当前仅 darwin 实装；其他平台需扩展后复跑');
    return finish(outDir, null, null);
  }

  // ── 步骤 0：构建产物新鲜度（Lead 提醒①）──
  ensureFreshBuild('freshness', outDir);

  // ── 步骤 1：替身供应商 + 服务合成 HOME（runtime-only → 自盖章 → 补内容）──
  const svcHome = fs.realpathSync(
    fs.mkdtempSync(path.join(os.tmpdir(), 'lingxi-r08-t03-a06-svc-')),
  );
  crashHomes.push(svcHome);
  journal('svc-home', { svcHome });
  const stub = await startStubProvider(outDir);
  journal('stub', { pid: stub.child.pid, port: stub.port });
  seedRuntimeOnly(svcHome, stub.port);
  await stampSyntheticHome(svcHome, outDir);
  seedServiceHomeRest(svcHome);

  // ── 步骤 2：harness 直启服务（默认回环模式；显式偏差：非真 LAN，见文件头）──
  const svcEnv = { ...process.env };
  delete svcEnv.ELECTRON_RUN_AS_NODE;
  const service = spawnLogged(RUST_BIN, ['--bind', '127.0.0.1:0', '--home', svcHome],
    { cwd: ROOT, env: svcEnv }, path.join(outDir, 'service.log'));
  journal('service-spawn', { pid: service.pid });
  service.on('exit', (code, signal) => journal('service-exit', { code, signal }));
  let serviceReaped = false;

  // 两个客户端 + 句柄容器（finally 收尸用）
  const clients = {};
  try {
    await waitFor('rust-instance', () => {
      try {
        fs.accessSync(path.join(svcHome, 'lingxi-service', 'instance.json'));
        fs.accessSync(path.join(svcHome, 'lingxi-service', 'local-token.json'));
        return true;
      } catch { return false; }
    }, 90000);
    const conn = readRuntimeRecords(svcHome);
    journal('rust-service', { pid: conn.pid, port: conn.port, baseUrl: conn.baseUrl });
    assert('rust-service', pidAlive(conn.pid), `rust service pid ${conn.pid} alive`);

    // ── 步骤 3：客户端 A/B 合成 HOME + remote 令牌文件（0600 私钥文件）──
    const electronBin = require('electron');
    const cdpBase = 9223 + (process.pid % 500);
    for (const [name, cdpPort] of [['A', cdpBase], ['B', cdpBase + 500]]) {
      const home = fs.realpathSync(
        fs.mkdtempSync(path.join(os.tmpdir(), `lingxi-r08-t03-a06-cli${name}-`)),
      );
      crashHomes.push(home);
      seedClientHome(home);
      const tokenFile = path.join(home, 'remote-token.json');
      fs.writeFileSync(tokenFile, JSON.stringify({ token: conn.token }) + '\n', { mode: 0o600 });
      const env = {
        ...process.env,
        LINGXI_HOME: home,
        LINGXI_DESKTOP_SERVER_RUNTIME: 'rust',
        LINGXI_DESKTOP_RUST_REMOTE_URL: `http://127.0.0.1:${conn.port}`,
        LINGXI_DESKTOP_RUST_REMOTE_TOKEN_FILE: tokenFile,
      };
      delete env.ELECTRON_RUN_AS_NODE;
      const electron = spawnLogged(electronBin, ['.', `--remote-debugging-port=${cdpPort}`],
        { cwd: ROOT, env }, path.join(outDir, `electron-${name}.log`));
      journal(`electron-${name}`, { pid: electron.pid, cdpPort, home });
      electron.on('exit', (code, signal) => journal(`electron-${name}-exit`, { code, signal }));
      clients[name] = { name, home, cdpPort, electron, pid: electron.pid, page: null };

      // 等窗口 + 标题 + GSCI（Lead batch-E 硬断言：remote 分支确证）
      const page = await findMainPageTarget(cdpPort);
      clients[name].page = page;
      let lastTitle = null;
      let lastCdpError = null;
      let title = null;
      try {
        title = await waitFor(`title-${name}`, async () => {
          try {
            const value = await cdpEvaluate(page.webSocketDebuggerUrl, 'document.title', false);
            lastTitle = value;
            return value === EXPECT_TITLE ? value : null;
          } catch (error) { lastCdpError = String(error && error.message || error); return null; }
        }, 90000, 2000);
      } catch { /* 落入统一 fail，保留现场 */ }
      if (title !== EXPECT_TITLE) {
        journal(`title-${name}-red`, { lastTitle, lastCdpError, targetUrl: page.url });
        fail(`title-${name}`, `document.title === ${EXPECT_TITLE}（实测 ${JSON.stringify(lastTitle)}，cdpError=${lastCdpError}）`);
      }
      const connInfo = await cdpEvaluate(
        page.webSocketDebuggerUrl, 'window.hana.getServerConnectionInfo()', true);
      journal(`conn-info-${name}`, { connInfo: connInfo ? { ...connInfo, token: '<redacted>' } : connInfo });
      // remote 分支硬断言（Lead batch-E ②）：local-owned/local-reused 恒 host=null；
      // remote 分支才带显式 host。host==='127.0.0.1' + 夹具端口 + lingxi-service
      // 三者同真 ⟺ remote 分支，悄悄落回 local 即红，绝不降级为记录。
      assert(`remote-mode-${name}`,
        connInfo && connInfo.host === '127.0.0.1'
          && connInfo.port === conn.port
          && connInfo.serverNodeKind === 'lingxi-service'
          && connInfo.serverNodeTransport === 'http',
        `客户端 ${name} GSCI 确证 remote 分支（host/port/kind/transport；实测 ${JSON.stringify(connInfo && { ...connInfo, token: '<redacted>' })}）`);
      // 零接触结构性断言：remote 分支永不读本机实例文件。
      assert(`zero-touch-instance-${name}`,
        !fs.existsSync(path.join(home, 'lingxi-service', 'instance.json')),
        `客户端 ${name} HOME 下无 lingxi-service/instance.json（remote 零接触）`);
    }

    // ── 步骤 4：S1 进程树（双端在线、任务前）──
    const s1 = snapshotTree('tree-s1', outDir, 'S1-boot', process.pid);
    assertSubtreeTree('tree-s1', s1, { nodeKernel: 0, rustKernel: 1, electronMains: 2 });
    if (s1 && s1.subtree) {
      const rustDesc = s1.subtree.descendants.find((r) => r.class === 'rust-kernel');
      assert('tree-s1-rust-pid', rustDesc && rustDesc.pid === conn.pid,
        `子树内唯一 rust-kernel = harness 服务 pid ${conn.pid}（实测 ${rustDesc && rustDesc.pid}）`);
    }

    // ── 步骤 5：任务腿 a1 —— 从客户端 A 渲染进程内发起，双端在线时完成 ──
    const leg1 = await cdpEvaluateClient(
      clients.A,
      buildTaskExpression('a06-clientA-leg1', 'leg-a1'),
      true, 120000);
    journal('leg-a1', leg1);
    fs.writeFileSync(path.join(outDir, 'task-leg-a1.json'), JSON.stringify(leg1, null, 2) + '\n');
    assert('leg-a1-run-end', leg1 && leg1.runEnded === true && leg1.runEndReason === null,
      `leg-a1 assistant_run_end 到达且 reason 为空（成功语义；实测 ${JSON.stringify(leg1 && { runEnded: leg1.runEnded, reason: leg1.runEndReason })}）`);
    assert('leg-a1-final', leg1 && leg1.sawFinal === true,
      'leg-a1 帧含 A06-CHAT-FINAL leg-a1（任务真实完成）');
    const hist1 = await httpJson('history-leg-a1', 'GET',
      `${conn.baseUrl}/lingxi/v1/sessions/a06-clientA-leg1/history?all=1`, conn.token);
    fs.writeFileSync(path.join(outDir, 'history-leg-a1.json'),
      JSON.stringify({ headers: hist1.headers, value: hist1.value }, null, 2) + '\n');
    assert('history-leg-a1', hist1.status === 200
      && JSON.stringify(hist1.value).includes('A06-CHAT-FINAL leg-a1'),
      `leg-a1 服务侧历史含终结回答（status=${hist1.status}）`);

    // ── 步骤 6：定向退 B → B 收敛、A 未被误伤、服务 pid/instanceId 连续、A 健康 ──
    const qb = await quitClientByPid('quit-b', clients.B, [clients.A]);
    quitMechanisms.B = qb.mechanism;
    assert('quit-b-exit', !pidAlive(clients.B.pid),
      `客户端 B 主进程退出（mechanism=${qb.mechanism}）`);
    assert('client-a-alive-after-b', pidAlive(clients.A.pid) && qb.collateral.length === 0,
      `退 B 未误伤 A（A pid ${clients.A.pid} 存活；collateral=${JSON.stringify(qb.collateral)}）`);
    if (!pidAlive(clients.A.pid)) {
      throw new Error('client-a-collateral-exit: 定向退 B 时客户端 A 一并退出（Cmd+Q 错投），后续 A 侧断言无法成立');
    }
    assert('service-alive-after-b', pidAlive(conn.pid),
      `退 B 后服务 pid ${conn.pid} 存活（对客户端退出零接触）`);
    const connAfterB = readRuntimeRecords(svcHome);
    assert('service-continuity', connAfterB.pid === conn.pid && connAfterB.instanceId === conn.instanceId,
      `服务实例连续（pid/instanceId 不变；实测 pid=${connAfterB.pid}）`);
    const healthB = await httpJson('health-after-b', 'GET',
      `${conn.baseUrl}/lingxi/v1/health`, conn.token);
    assert('health-after-b', healthB.status === 200,
      `退 B 后 /lingxi/v1/health → 200（实测 ${healthB.status}）`);
    const connInfoA2 = await cdpEvaluateClient(
      clients.A, 'window.hana.getServerConnectionInfo()', true);
    assert('client-a-still-remote', connInfoA2
      && connInfoA2.host === '127.0.0.1' && connInfoA2.port === conn.port,
      `退 B 后客户端 A 连接面不变（remote；实测 host=${connInfoA2 && connInfoA2.host} port=${connInfoA2 && connInfoA2.port}）`);

    // ── 步骤 7：S2 进程树（B 已退、A 在线）──
    const s2 = snapshotTree('tree-s2', outDir, 'S2-after-B-quit', process.pid);
    assertSubtreeTree('tree-s2', s2, { nodeKernel: 0, rustKernel: 1, electronMains: 1 });

    // ── 步骤 8：任务腿 a2 —— 退 B 后 A 继续完成第二个任务 ──
    const leg2 = await cdpEvaluateClient(
      clients.A,
      buildTaskExpression('a06-clientA-leg2', 'leg-a2'),
      true, 120000);
    journal('leg-a2', leg2);
    fs.writeFileSync(path.join(outDir, 'task-leg-a2.json'), JSON.stringify(leg2, null, 2) + '\n');
    assert('leg-a2-run-end', leg2 && leg2.runEnded === true && leg2.runEndReason === null,
      `leg-a2 assistant_run_end 到达且 reason 为空（成功语义；实测 ${JSON.stringify(leg2 && { runEnded: leg2.runEnded, reason: leg2.runEndReason })}）`);
    assert('leg-a2-final', leg2 && leg2.sawFinal === true,
      'leg-a2 帧含 A06-CHAT-FINAL leg-a2（退 B 后任务仍真实完成）');
    const hist2 = await httpJson('history-leg-a2', 'GET',
      `${conn.baseUrl}/lingxi/v1/sessions/a06-clientA-leg2/history?all=1`, conn.token);
    fs.writeFileSync(path.join(outDir, 'history-leg-a2.json'),
      JSON.stringify({ headers: hist2.headers, value: hist2.value }, null, 2) + '\n');
    assert('history-leg-a2', hist2.status === 200
      && JSON.stringify(hist2.value).includes('A06-CHAT-FINAL leg-a2'),
      `leg-a2 服务侧历史含终结回答（status=${hist2.status}）`);

    // ── 步骤 9：退 A → 全客户端收敛、服务仍存活（零接触终态）──
    const qa = await quitClientByPid('quit-a', clients.A, []);
    quitMechanisms.A = qa.mechanism;
    assert('quit-a-exit', !pidAlive(clients.A.pid),
      `客户端 A 主进程退出（mechanism=${qa.mechanism}）`);
    assert('service-alive-after-all', pidAlive(conn.pid),
      `双端全退后服务 pid ${conn.pid} 仍存活（零接触终态硬断言）`);
    const healthEnd = await httpJson('health-final', 'GET',
      `${conn.baseUrl}/lingxi/v1/health`, conn.token);
    assert('health-final', healthEnd.status === 200,
      `终态 /lingxi/v1/health → 200（实测 ${healthEnd.status}）`);

    // ── 步骤 10：S3 进程树（双端全退；服务在、harness 待回收）──
    const s3 = snapshotTree('tree-s3', outDir, 'S3-all-quit', process.pid);
    assertSubtreeTree('tree-s3', s3, { nodeKernel: 0, rustKernel: 1, electronMains: 0 });

    // ── 步骤 11：桩 journal 核对（双腿 chat 请求各一次，零工具调用）──
    let stubJournal = [];
    try {
      stubJournal = fs.readFileSync(path.join(outDir, 'stub-journal.jsonl'), 'utf8')
        .trim().split('\n').filter(Boolean).map((l) => JSON.parse(l));
    } catch { /* journal 缺失走下面断言显式红 */ }
    fs.writeFileSync(path.join(outDir, 'stub-journal-pretty.json'),
      JSON.stringify(stubJournal, null, 2) + '\n');
    const chatReqs = stubJournal.filter((e) => e.model === 'chat' && e.request);
    const a06Reqs = chatReqs.filter((e) => JSON.stringify(e.request).includes(CHAT_MARKER));
    assert('stub-chat-requests', a06Reqs.length === 2,
      `桩记录 A06 chat 请求 === 2（双腿各一；实测 ${a06Reqs.length}）`);
    // act 取值 = 'final' | 'error' | 工具名（stub_provider.mjs:247）；工具接触 = 其余
    const toolCalls = stubJournal.filter((e) => e.act && e.act !== 'final' && e.act !== 'error');
    assert('stub-zero-tool', toolCalls.length === 0,
      `A06 双腿零工具接触（不撞 PPG 面；实测工具调用 ${toolCalls.length}）`);
  } finally {
    // ── 回收：服务必须仍存活至此（零接触），由 harness 负责 SIGTERM 收敛 ──
    if (pidAlive(service.pid)) {
      journal('reap-service', { pid: service.pid, aliveAtReap: true });
      service.kill('SIGTERM');
      const exited = await new Promise((resolve) => {
        const timer = setTimeout(() => resolve(false), 30000);
        service.once('exit', (code, signal) => { clearTimeout(timer); resolve({ code, signal }); });
      });
      journal('reap-service-result', { exited });
      assert('reap-service', Boolean(exited) || !pidAlive(service.pid),
        `harness 回收服务（SIGTERM 收敛；实测 ${JSON.stringify(exited)}）`);
      if (!exited) service.kill('SIGKILL');
      serviceReaped = true;
    } else {
      journal('reap-service', { pid: service.pid, aliveAtReap: false });
    }
    stub.child.kill('SIGTERM');
    await cleanupRunProcesses(clients, svcHome, outDir);
    console.error('[a06-dbg] finally tail reached');
  }

  console.error('[a06-dbg] after-finally reached');
  const r = finish(outDir, svcHome, serviceReaped);
  console.error('[a06-dbg] main body resolved');
  return r;
}

/** 主进程渲染诊断落盘在 HOME 下，必须随证据收走。 */
function collectLaunchDiagnostics(homes, outDir) {
  for (const home of homes.filter(Boolean)) {
    const src = path.join(home, 'diagnostics', 'desktop-launch');
    const tag = path.basename(home);
    for (const name of ['renderer.log', 'launch.log', 'launch-marker.json']) {
      try {
        fs.copyFileSync(path.join(src, name), path.join(outDir, `desktop-launch-${tag}-${name}`));
      } catch { /* 可能尚未产生 */ }
    }
  }
}

/** 崩溃/超时路径也必须收尸：按合成 HOME 精确匹配，每次运行唯一，不误伤。 */
async function cleanupRunProcesses(clients, svcHome, outDir) {
  const homes = [svcHome];
  for (const c of Object.values(clients || {})) {
    homes.push(c.home);
    try { c.electron.kill('SIGTERM'); } catch { /* 已退出 */ }
  }
  collectLaunchDiagnostics(homes, outDir);
  await sleep(3000);
  for (const home of homes) spawnSync('pkill', ['-TERM', '-f', home]);
  await sleep(3000);
  for (const home of homes) spawnSync('pkill', ['-KILL', '-f', home]);
  journal('cleanup', { homes });
}

let verdictFinalized = false;
function finish(outDir, svcHome, serviceReaped) {
  console.error('[a06-dbg] finish entered');
  verdictFinalized = true;
  const verdict = {
    schema: 'r08-t03-a06-verdict/v1',
    scenario: 'R08-T03-A06 双客户端 remote 共连同一 Rust 服务：退 B 后 A 继续任务，服务零接触',
    verdict: failures.length === 0 ? 'two-client-remote-pass__zero-touch-quit-verified' : 'fail',
    deviation: {
      id: 'A06-loopback-transport',
      ruling: 'Lead batch-E 裁决 (a) 已复核：回环-remote 退路，核心验收语义全部硬断言不变',
      detail: '传输层为 127.0.0.1 回环而非真实 LAN。真 LAN e2e 双重阻断：环境层 macOS ALF 对 adhoc 签名 debug 二进制静默丢包（t03-a06-auth-probe-04 绿证）；产品层 PPG-3（设备凭证无客户端呈现通道 + 签发面 local_only）。',
    },
    ppg3: {
      id: 'PPG-3',
      title: '远端桌面 LAN 认证通道缺失：设备凭证无客户端呈现通道，签发面 local_only',
      chain: 'transport.rs:201-206 非回环 → ConnectionKind::Lan'
        + ' → auth.rs:2558-2566 loopback token 在非 Local 连接恒 401 loopback_token_requires_local_transport'
        + ' → rust-local-service.cjs readRemoteRustConnection 仅接受 ^[0-9a-f]{32}$ 形状'
        + ' → 设备凭证 "hana_dev_"+base64url(32)（auth.rs:81/:2019）被该正则拒绝'
        + ' → 签发面 POST /lingxi/v1/devices/credentials（lib.rs:9037）挂 local_only，LAN 绑定下不可达',
      envLayer: 'macOS ALF 对 adhoc/linker 签名 debug 二进制静默丢弃入站包（socketfilterfw --getappblocked 报 permitted 仍丢）；python3/nc 对照同口 200 = 非服务缺陷',
      status: 'open — 登记 T04/T05/T08 MUST-ADJUDICATE，与 PPG-1/2 并列',
      evidence: ['t03-a06-auth-probe-04/output.log', 'verdict.json'],
    },
    quitMechanisms,
    quitMechanismNote: 'keyboard=AXRaise+set frontmost 后，经 CDP 核实目标渲染进程 document.hasFocus()===true'
      + ' 且其余存活客户端 !==true 再发 Cmd+Q（真实用户路径；System Events 的 frontmost 对同 bundle 双实例按 bundle 回答，'
      + 'gate14 红实证 Cmd+Q 错投退了 A，故不再以其为发键依据）；'
      + 'sigterm-graceful=焦点核实不到或键盘路径穷尽后 SIGTERM 兜底（Electron 捕获走正常退出链，exit 0）。'
      + '绝不盲发 Cmd+Q；替代显式记录，不静默。',
    failures,
    serviceHome: svcHome || null,
    serviceReapedByHarness: Boolean(serviceReaped),
    journal: journalEntries,
  };
  fs.writeFileSync(path.join(outDir, 'verdict.json'), JSON.stringify(verdict, null, 2) + '\n');
  console.log(`[r08-t03-a06] verdict=${verdict.verdict} failures=${failures.length} home=${svcHome} out=${outDir}`);
  if (failures.length) {
    for (const f of failures) console.log(`  FAIL[${f.step}] ${f.message}`);
  }
  process.exitCode = failures.length === 0 ? 0 : 1;
}

main().catch((err) => {
  // 崩溃路径也必须落盘 journal（drill-03/04 教训）。
  try {
    if (crashOutDir) {
      collectLaunchDiagnostics(crashHomes, crashOutDir);
      fs.writeFileSync(path.join(crashOutDir, 'crash-verdict.json'), JSON.stringify({
        schema: 'r08-t03-a06-crash/v1',
        error: String(err && err.stack || err),
        homes: crashHomes,
        journal: journalEntries,
      }, null, 2) + '\n');
    }
  } catch { /* 崩溃路径落盘失败不再叠加 */ }
  for (const home of crashHomes) {
    spawnSync('pkill', ['-TERM', '-f', home]);
  }
  if (verdictFinalized) {
    // finish() 已定案退出码：迟到异步错误（收尾后残余 CDP/HTTP 连接拒绝）
    // 只登记不翻转判定——判定以断言为准，不以收尾竞态为准。
    console.error(`[r08-t03-a06] post-verdict async error (ignored for exit code): ${err && err.message || err}`);
    return;
  }
  console.error(`[r08-t03-a06] crash: ${err && err.stack || err}`);
  process.exitCode = 2;
});
