#!/usr/bin/env node
/**
 * R08-T03 批次 D / 验收 A05：旧 Electron 壳 + Rust standalone 全链演练。
 *
 * 场景（任务书 R08-T03 A05）：旧 Node 内核关闭（LINGXI_DESKTOP_SERVER_RUNTIME=rust）
 * + 预览开启，Rust 独立服务承载，无 Node/Pi 兜底。按 Lead 裁决拆两腿：
 *
 *   读腿（端到端绿证）：合成 HOME 服务数据根种子文件 → wire 协议驱动已注册的
 *   `read` 工具读回，终结回答回显种子内容；三点进程树 S1/S2/S3、node-kernel=0、
 *   CDP 标题断言全部照旧硬断言。
 *
 *   写腿（PPG-1 红证据，预期 BLOCKED）：rust 预览启动路径不传 --config →
 *   workspace_root=None → write/edit 不注册（模型工具面无写能力）。桩仍按
 *   任务书原语义请求 write → 服务拒（provider requested unknown tool）→
 *   run 落定 failed.provider_error。硬断言：桩 journal seq-1 工具面显式不含
 *   write/edit、服务日志出现 outcome=failed.provider_error、无伪终结帧、
 *   目标文件未落盘。该腿不挂 untilFrameType（provider_error 不发 run-end）。
 *
 * 另实测冻结矩阵行 1（关主窗口 hide）与行 3（完全退出 Cmd+Q → owned
 * SIGTERM 收敛）。键盘驱动走 AppleScript keystroke，当前仅 darwin 实装；
 * 其他平台显式 FAIL（不虚报）。
 *
 * 纪律：任何断言失败 → verdict=fail + 非零退出，绝不降级为「记录但不断言」。
 * 两腿齐绿（读腿真绿 + 写腿按预期 BLOCKED 且证据齐全）→ 退出码 0，
 * verdict 字符串为 read-pass__write-blocked-PPG-1。
 * 用法：
 *   node scripts/rust-tauri/r08_t03_a05_drill.mjs --out <evidence-subdir>
 */
import { spawn, spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import WebSocket from 'ws';

const require = createRequire(import.meta.url);
const __dirname = path.dirname(fileURLToPath(import.meta.url));

const ROOT = path.resolve(__dirname, '..', '..');
const TREE_RECORDER = path.join(__dirname, 'r08_t03_process_tree.mjs');
const STUB_PROVIDER = path.join(__dirname, 'r08_t03_stub_provider.mjs');

const EXPECT_TITLE = 'Lingxi · Rust 预览';
// 读腿：种子文件在服务数据根（data_home = 合成 HOME）下的相对路径。
const READ_NOTE_REL = 'a05-drill/read-note.txt';
const READ_NOTE_CONTENT = 'A05-READ-NOTE-v1\n秘密标记：LINGXI-A05-READ-7351\n';
const READ_SECRET = 'LINGXI-A05-READ-7351';
const READ_MARKER = 'A05-READ-TASK';
const READ_PROMPT = `请读取文件 ${READ_NOTE_REL} 并原样复述其内容。（标记 ${READ_MARKER}）`;
// 写腿：PPG-1 红证据。目标文件预期永不落盘。
const WRITE_NOTE_REL = 'a05-drill/write-note.txt';
const WRITE_MARKER = 'A05-WRITE-TASK';
const WRITE_PROMPT = `请创建文件 ${WRITE_NOTE_REL} 并写入一行内容。（标记 ${WRITE_MARKER}）`;
const FINAL_ANSWER = 'A05-FINAL-ANSWER';

const failures = [];
const journalEntries = [];
let crashOutDir = null;
let crashHome = null;

function parseArgs(argv) {
  const args = { out: null };
  for (let i = 2; i < argv.length; i += 1) {
    if (argv[i] === '--out') { args.out = path.resolve(argv[i + 1]); i += 1; }
    else throw new Error(`unknown argument: ${argv[i]}`);
  }
  if (!args.out) throw new Error('--out is required');
  return args;
}

function journal(step, data) {
  journalEntries.push({ at: new Date().toISOString(), step, ...data });
}

function fail(step, message) {
  failures.push({ step, message });
  journal(step, { fail: message });
}

function assert(step, condition, message) {
  if (!condition) fail(step, message);
  else journal(step, { ok: message });
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitFor(step, predicate, timeoutMs, intervalMs = 500) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const value = await predicate();
    if (value) return value;
    if (Date.now() > deadline) {
      throw new Error(`${step}: timeout after ${timeoutMs}ms`);
    }
    await sleep(intervalMs);
  }
}

function pidAlive(pid) {
  try { process.kill(pid, 0); return true; } catch (err) { return err && err.code === 'EPERM'; }
}

function newestMtimeMs(dir, filter) {
  let newest = 0;
  const stack = [dir];
  while (stack.length) {
    const current = stack.pop();
    let entries;
    try { entries = fs.readdirSync(current, { withFileTypes: true }); } catch { continue; }
    for (const entry of entries) {
      const full = path.join(current, entry.name);
      if (entry.isDirectory()) stack.push(full);
      else if (entry.isFile() && filter(full)) {
        const mtime = fs.statSync(full).mtimeMs;
        if (mtime > newest) newest = mtime;
      }
    }
  }
  return newest;
}

/** Lead 提醒①：起窗前先验 dist-renderer/preload 产物新鲜度，陈旧先重建留痕。 */
function ensureFreshBuild(step, outDir) {
  const checks = [
    {
      name: 'dist-renderer',
      artifactDir: path.join(ROOT, 'desktop', 'dist-renderer'),
      sourceDirs: [path.join(ROOT, 'desktop', 'src', 'react'), path.join(ROOT, 'desktop', 'src', 'shared')],
      sourceFilter: (f) => /\.(ts|tsx|css|html)$/.test(f),
      build: ['npm', ['run', 'build:renderer']],
    },
    {
      name: 'preload-bundle',
      artifactDir: null,
      artifactFile: path.join(ROOT, 'desktop', 'preload.bundle.cjs'),
      sourceDirs: null,
      sourceFiles: [path.join(ROOT, 'desktop', 'preload.cjs')],
      build: ['npm', ['run', 'build:preload']],
    },
  ];
  const report = [];
  for (const check of checks) {
    const artifactNewest = check.artifactFile
      ? (fs.existsSync(check.artifactFile) ? fs.statSync(check.artifactFile).mtimeMs : 0)
      : newestMtimeMs(check.artifactDir, () => true);
    let sourceNewest = 0;
    if (check.sourceDirs) {
      for (const dir of check.sourceDirs) {
        sourceNewest = Math.max(sourceNewest, newestMtimeMs(dir, check.sourceFilter));
      }
    } else {
      for (const file of check.sourceFiles) {
        if (fs.existsSync(file)) sourceNewest = Math.max(sourceNewest, fs.statSync(file).mtimeMs);
      }
    }
    const stale = !(artifactNewest > 0) || sourceNewest > artifactNewest;
    const entry = { name: check.name, artifactNewest, sourceNewest, stale, rebuilt: false };
    if (stale) {
      const [cmd, argv] = check.build;
      const result = spawnSync(cmd, argv, { cwd: ROOT, encoding: 'utf8' });
      entry.rebuilt = true;
      entry.buildExit = result.status;
      fs.writeFileSync(path.join(outDir, `build-${check.name}.log`),
        `${result.stdout || ''}\n${result.stderr || ''}`);
      if (result.status !== 0) {
        fail(step, `${check.name} rebuild failed (exit ${result.status}); see build-${check.name}.log`);
      }
    }
    report.push(entry);
    journal(step, entry);
  }
  return report;
}

function spawnLogged(command, argv, options, logFile) {
  const fd = fs.openSync(logFile, 'a');
  const child = spawn(command, argv, { ...options, stdio: ['ignore', fd, fd] });
  return child;
}

async function startStubProvider(outDir) {
  const child = spawnLogged(process.execPath, [
    STUB_PROVIDER, '--port', '0',
    '--journal', path.join(outDir, 'stub-journal.jsonl'),
    '--read-path', READ_NOTE_REL,
    '--final-delay-ms', '1500',
  ], { cwd: ROOT }, path.join(outDir, 'stub-stdout.log'));
  // READY 行从日志文件回读（stdio 已重定向），简单轮询。
  const port = await waitFor('stub-ready', () => {
    let text = '';
    try { text = fs.readFileSync(path.join(outDir, 'stub-stdout.log'), 'utf8'); } catch { return null; }
    const match = text.match(/A05_STUB_READY (\d+)/);
    return match ? Number(match[1]) : null;
  }, 20000, 200);
  return { child, port };
}

/** 第一刀播种：只放 runtime 目录（epoch 闸白名单），让服务自行盖章。 */
function seedRuntimeOnly(home, stubPort) {
  // 模型面：standalone models.json（runtime 目录整文件即模型面文档）。
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

/**
 * data-epoch 闸：HOME 内出现 agents/ 等非白名单条目即拒绝启动
 * （unstamped-home-with-data，exit 2）。 truthful 顺序 = 先只放 runtime
 * 目录让服务走 ProvablyNew 自盖章路径，停掉后再播种其余内容。
 * 不手写 stamp（不模拟内部状态文件格式）。
 */
async function stampSyntheticHome(home, outDir) {
  const rustBin = path.join(ROOT, 'rust', 'target', 'debug', 'lingxi-service');
  if (!fs.existsSync(rustBin)) {
    fail('stamp', `rust binary missing: ${rustBin}`);
    return;
  }
  const env = { ...process.env };
  delete env.ELECTRON_RUN_AS_NODE;
  const child = spawnLogged(rustBin, ['--home', home], { cwd: ROOT, env },
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

/** 第二刀播种：盖章完成后放 agent/偏好/元神内容。 */
function seedSyntheticHomeRest(home) {
  fs.mkdirSync(path.join(home, 'agents', 'lingxi'), { recursive: true });
  fs.writeFileSync(path.join(home, 'agents', 'lingxi', 'config.yaml'),
    'locale: zh-CN\nagent:\n  name: R08-A05\n  yuan: lingxi\nmemory:\n  enabled: false\ncompaction:\n  enabled: false\n');
  fs.mkdirSync(path.join(home, 'product', 'yuan'), { recursive: true });
  fs.writeFileSync(path.join(home, 'product', 'yuan', 'lingxi.md'),
    '你叫{{agentName}}，与用户一起完成任务。\n');
  // 跳过 onboarding：setupComplete 门禁只看这个文件（main.cjs isSetupComplete）。
  fs.mkdirSync(path.join(home, 'user'), { recursive: true });
  fs.writeFileSync(path.join(home, 'user', 'preferences.json'),
    JSON.stringify({ setupComplete: true }) + '\n');
  // 读腿种子：read 工具的根 = data_home（合成 HOME 本身），放相对路径目标。
  fs.mkdirSync(path.join(home, 'a05-drill'), { recursive: true });
  fs.writeFileSync(path.join(home, READ_NOTE_REL), READ_NOTE_CONTENT);
  journal('seed-read-note', { path: READ_NOTE_REL, content: READ_NOTE_CONTENT });
}

/** 最新一份 rust 服务日志全文（写腿 provider_error 取证源）。 */
function readLatestServiceLog(home) {
  const dir = path.join(home, 'lingxi-service', 'logs');
  let entries = [];
  try { entries = fs.readdirSync(dir); } catch { return ''; }
  const logs = entries.filter((n) => /^service-.*\.log$/.test(n)).sort();
  if (!logs.length) return '';
  try { return fs.readFileSync(path.join(dir, logs[logs.length - 1]), 'utf8'); } catch { return ''; }
}

/** 桩 journal（同步 writeSync 落盘，读取无 flush 竞态）。 */
function readStubJournal(outDir) {
  const file = path.join(outDir, 'stub-journal.jsonl');
  let text = '';
  try { text = fs.readFileSync(file, 'utf8'); } catch { return []; }
  return text.split('\n').filter(Boolean).map((line) => {
    try { return JSON.parse(line); } catch { return { parseError: line.slice(0, 120) }; }
  });
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

function wsClient(url, token) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url, { headers: { Authorization: `Bearer ${token}` } });
    ws.once('open', () => resolve(ws));
    ws.once('error', reject);
  });
}

function nextFrame(ws, trace, timeoutMs = 15000) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      ws.off('message', onMessage);
      reject(new Error('ws frame timeout'));
    }, timeoutMs);
    function onMessage(data) {
      clearTimeout(timer);
      // 常驻记录器（ws.on('message')）负责落 trace，这里只解析返回。
      const value = JSON.parse(data.toString('utf8'));
      resolve(value);
    }
    ws.once('message', onMessage);
  });
}

async function untilFrameType(ws, trace, type, timeoutMs = 150000) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const frame = await nextFrame(ws, trace, Math.max(1000, deadline - Date.now()));
    if (frame && frame.type === type) return frame;
    if (Date.now() > deadline) throw new Error(`missing frame type ${type}`);
  }
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

/** Lead 提醒③：node-kernel=0 是硬断言，记录但不断言即红。 */
function assertTree(step, tree, { nodeKernel = 0, rustKernel = null, electronUi = null }) {
  if (!tree) { fail(step, 'tree snapshot missing'); return; }
  assert(step, tree.counts['node-kernel'] === nodeKernel,
    `node-kernel count ${tree.counts['node-kernel']} !== ${nodeKernel}（硬断言）`);
  if (rustKernel !== null) {
    assert(step, tree.counts['rust-kernel'] === rustKernel,
      `rust-kernel count ${tree.counts['rust-kernel']} !== ${rustKernel}`);
  }
  if (electronUi !== null) {
    assert(step, tree.counts['electron-ui'] >= electronUi,
      `electron-ui count ${tree.counts['electron-ui']} < ${electronUi}`);
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

async function cdpEvaluate(wsUrl, expression, awaitPromise = true) {
  // 注意作用域：promise 回调里的局部变量不得遮蔽外层 sock
  // （drill-03/04/05 的 title 90s 超时根因 = 内层 sock 遮蔽导致外层
  //  ReferenceError，每次轮询都被 catch 吞成 null）。
  const sock = await new Promise((resolve, reject) => {
    const inner = new WebSocket(wsUrl);
    inner.once('open', () => resolve(inner));
    inner.once('error', reject);
  });
  let seq = 0;
  const send = (method, params) => new Promise((resolve, reject) => {
    seq += 1;
    const id = seq;
    const timer = setTimeout(() => reject(new Error(`cdp ${method} timeout`)), 20000);
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
 * 聚焦到指定 pid 的进程（a06 quitClientByPid 同款已证形态：AXRaise + set
 * frontmost + 轮询确认）。`tell application "Electron" to activate` 对开发版
 * Electron 不达前台（frontmost 恒为宿主终端 → keystroke 送错目标——
 * t16-a05-unlock-check-01 journal 实证 row1 实测 visible + row3 超时；TCC
 * 本身已授权，keystroke exit 0）。返回 frontmost 是否就位。
 */
function focusProcessByPid(pid) {
  const script = `
tell application "System Events"
  set p to first process whose unix id is ${pid}
  try
    perform action "AXRaise" of (first window of p)
  end try
  set frontmost of p to true
  set n to 0
  repeat until (frontmost of p) or n > 16
    delay 0.25
    set n to n + 1
  end repeat
  if not (frontmost of p) then error "frontmost-not-achieved"
  return name of p
end tell`;
  return osascript(script);
}

let freezeHideMechanism = 'keyboard';
let freezeQuitMechanism = 'keyboard';

// R08_A05_MANUAL=1 手动辅助模式（2026-10-25 用户路径①）：宿主策略丢弃合成
// AppleScript 焦点/按键（六查在案），但真实用户点击+键盘永远有效。此模式下
// row1/row3 跳过 osascript 合成，改为终端实时提示 + 宽窗等待用户亲自按键，
// 机制如实记 verdict（manual-user）——比合成按键更真实的用户路径证据。
const MANUAL_MODE = process.env.R08_A05_MANUAL === '1';
function manualPrompt(line) {
  if (!MANUAL_MODE) return;
  process.stdout.write(`\n[手动辅助] ${line}\n`);
}
/** 手动等待谓词成立（宽窗 180s，每 2s 心跳提示剩余秒数）。 */
async function manualWait(label, predicate, budgetMs = 180000) {
  if (!MANUAL_MODE) return false;
  manualPrompt(`请在 ${Math.round(budgetMs / 1000)} 秒内完成：${label}`);
  const deadline = Date.now() + budgetMs;
  while (Date.now() < deadline) {
    try { if (await predicate()) return true; } catch { /* 未就绪 */ }
    const remain = Math.round((deadline - Date.now()) / 1000);
    if (remain % 10 === 0) process.stdout.write(`  …等待用户操作（剩 ${remain}s）\n`);
    await sleep(2000);
  }
  return false;
}

async function main() {
  const args = parseArgs(process.argv);
  fs.mkdirSync(args.out, { recursive: true });
  const outDir = args.out;
  crashOutDir = outDir;

  if (process.platform !== 'darwin') {
    fail('platform', 'A05 键盘驱动退出链当前仅 darwin 实装；其他平台需扩展后复跑');
    return finish(outDir, null);
  }

  // ── 步骤 0：构建产物新鲜度（Lead 提醒①）──
  ensureFreshBuild('freshness', outDir);

  // ── 步骤 1：合成 HOME + 替身供应商 + epoch 盖章 ──
  // macOS $TMPDIR 经 /var→/private/var 符号链接；rust fsio DirFd::open 逐段
  // O_NOFOLLOW 走 canonical 家目录，非 canonical 路径必 ELOOP → bootstrap
  // "MCP 原始只读目录不可用" exit 2（红证据 drill-02）。家目录必须 realpath。
  const home = fs.realpathSync(
    fs.mkdtempSync(path.join(os.tmpdir(), 'lingxi-r08-t03-a05-')),
  );
  journal('home', { home });
  crashHome = home;
  const stub = await startStubProvider(outDir);
  journal('stub', { pid: stub.child.pid, port: stub.port });
  seedRuntimeOnly(home, stub.port);
  await stampSyntheticHome(home, outDir);
  seedSyntheticHomeRest(home);

  // ── 步骤 2：起 Electron（rust runtime，旧 Node 关闭）──
  // 会话 shell 可能导出 ELECTRON_RUN_AS_NODE=1（宿主链污染）：不清掉则
  // Electron 以纯 Node 跑 main.cjs、零输出静默 exit 1（红证据 drill-01）。
  const electronBin = require('electron');
  const cdpPort = 9223 + (process.pid % 1000);
  const electronEnv = {
    ...process.env,
    LINGXI_HOME: home,
    LINGXI_DESKTOP_SERVER_RUNTIME: 'rust',
  };
  delete electronEnv.ELECTRON_RUN_AS_NODE;
  const electron = spawnLogged(electronBin, [
    '.', `--remote-debugging-port=${cdpPort}`,
  ], {
    cwd: ROOT,
    env: electronEnv,
  }, path.join(outDir, 'electron.log'));
  journal('electron', { pid: electron.pid, cdpPort });
  electron.on('exit', (code, signal) => journal('electron-exit', { code, signal }));

  try {
    // ── 步骤 3：等 Rust 服务就绪（instance.json + token）──
    await waitFor('rust-instance', () => {
      try {
        fs.accessSync(path.join(home, 'lingxi-service', 'instance.json'));
        fs.accessSync(path.join(home, 'lingxi-service', 'local-token.json'));
        return true;
      } catch { return false; }
    }, 90000);
    const conn = readRuntimeRecords(home);
    journal('rust-service', { pid: conn.pid, port: conn.port, baseUrl: conn.baseUrl });
    assert('rust-service', pidAlive(conn.pid), `rust service pid ${conn.pid} alive`);

    // ── 步骤 4：CDP 标题与连接断言 ──
    const page = await findMainPageTarget(cdpPort);
    journal('cdp-target', { url: page.url });
    let lastTitle = null;
    let lastCdpError = null;
    let title = null;
    try {
      title = await waitFor('title', async () => {
        try {
          const value = await cdpEvaluate(page.webSocketDebuggerUrl, 'document.title', false);
          lastTitle = value;
          return value === EXPECT_TITLE ? value : null;
        } catch (error) { lastCdpError = String(error && error.message || error); return null; }
      }, 90000, 2000);
    } catch { /* 落入下面统一 fail，保留现场继续收集后续独立证据 */ }
    if (title !== EXPECT_TITLE) {
      journal('title-red', { lastTitle, lastCdpError, targetUrl: page.url });
      fail('title', `document.title === ${EXPECT_TITLE}（实测 ${JSON.stringify(lastTitle)}，cdpError=${lastCdpError}）`);
    }
    const connInfo = await cdpEvaluate(
      page.webSocketDebuggerUrl,
      'window.hana.getServerConnectionInfo()',
      true,
    );
    journal('conn-info', { connInfo: connInfo ? { ...connInfo, token: '<redacted>' } : connInfo });
    // GSCI 契约（main.cjs:5648）：local-owned 模式 host 按设计为 null（= 127.0.0.1 回环），
    // 远端模式才是显式地址；字段名是 serverNodeKind（drill-06 红证据：误用 kind 字段）。
    const connHost = connInfo && (connInfo.host === null ? '127.0.0.1' : connInfo.host);
    assert('conn-info', connInfo && connHost === '127.0.0.1' && connInfo.port === conn.port,
      `renderer connection host/port === 127.0.0.1:${conn.port}（local host:null 即回环；实测 host=${connInfo && JSON.stringify(connInfo.host)} port=${connInfo && connInfo.port}）`);
    assert('conn-info-kind', connInfo && connInfo.serverNodeKind === 'lingxi-service',
      `renderer connection serverNodeKind === lingxi-service（实测 ${connInfo && connInfo.serverNodeKind}）`);

    // ── 步骤 5：S1 进程树（启动后、任务前）──
    const s1 = snapshotTree('tree-s1', outDir, 'S1-boot', electron.pid);
    assertTree('tree-s1', s1, { nodeKernel: 0, rustKernel: 1, electronUi: 1 });
    if (s1 && s1.subtree) {
      const rustDesc = s1.subtree.descendants.find((r) => r.class === 'rust-kernel');
      assert('tree-s1-owned', rustDesc && rustDesc.pid === conn.pid,
        `rust kernel pid ${conn.pid} 在 Electron 子树内（local-owned 证据）`);
    }

    // ── 步骤 6：读腿 —— wire 协议驱动已注册 read 工具，端到端真实读回 ──
    const trace = [];
    const writeTrace = () => {
      // trace 是执行轨迹核心证据：成功与崩溃路径都必须落盘
      //（drill-06 教训：崩溃时 trace 丢失，被迫从服务日志反推根因）。
      fs.writeFileSync(path.join(outDir, 'trace.jsonl'),
        trace.map((row) => JSON.stringify(row)).join('\n') + '\n');
    };
    const readSession = 'a05-read-session';
    // 会话级授权目录 = 产品正规孔道（CreateSessionBody.authorizedFolders，
    // lib.rs:5371-5387）：无 workspace 时 read 走零根 ResourceAccess
    // （lib.rs:1498-1508 注释"零根策略不授予任何目录"），唯一授权来源是每轮
    // replace_session_roots 从会话行 authorized_folders 刷新（runs.rs:2948-2952）。
    // 种子目录开机前已存在，validate_authorized_folder 要求现存目录 → 合法 201。
    const readScopeDir = path.join(home, 'a05-drill');
    const created = await httpJson('session-create', 'POST',
      `${conn.baseUrl}/lingxi/v1/sessions`, conn.token,
      { sessionId: readSession, agentId: 'lingxi', title: readSession, authorizedFolders: [readScopeDir] });
    assert('session-create', created.status === 201, `POST /lingxi/v1/sessions → 201（实测 ${created.status}）`);

    const ws = await wsClient(`${conn.baseUrl.replace('http', 'ws')}/lingxi/v1/ws`, conn.token);
    // 常驻记录器：写腿期间无主动 nextFrame 监听，帧也必须进 trace（PPG-1 反证）。
    ws.on('message', (data) => {
      try { trace.push({ at: new Date().toISOString(), frame: JSON.parse(data.toString('utf8')) }); } catch { /* 非 JSON 帧忽略 */ }
    });
    try {
      ws.send(JSON.stringify({
        protocol: 'lingxi.wire', clientKind: 'old-electron', clientVersion: '0',
        protocolMin: 1, protocolMax: 1,
      }));
      const hello = await nextFrame(ws, trace);
      assert('handshake', hello && hello.protocol === 'lingxi.wire', 'WS handshake ack protocol=lingxi.wire');

      ws.send(JSON.stringify({ type: 'prompt', sessionId: readSession, text: READ_PROMPT }));
      // 终结幕首字节延迟 1500ms（夹具参数）：此刻 run 进行中，取 S2。
      const s2 = snapshotTree('tree-s2', outDir, 'S2-inflight', electron.pid);
      assertTree('tree-s2', s2, { nodeKernel: 0, rustKernel: 1 });

      await untilFrameType(ws, trace, 'assistant_run_end');
      const idle = await untilFrameType(ws, trace, 'status');
      assert('run-idle', idle.isStreaming === false, '读腿 run 结束后 status.isStreaming === false');
    } finally {
      writeTrace();
    }

    // ── 步骤 7：读腿轨迹断言（历史 + 回显内容）──
    const history = await httpJson('history', 'GET',
      `${conn.baseUrl}/lingxi/v1/sessions/${readSession}/history?all=1`, conn.token);
    fs.writeFileSync(path.join(outDir, 'history.json'),
      JSON.stringify({ headers: history.headers, value: history.value }, null, 2) + '\n');
    assert('history', history.status === 200, `history → 200（实测 ${history.status}）`);
    const items = (history.value && history.value.items) || [];
    const toolItems = items.filter((v) => v.kind === 'tool_call');
    const toolTargets = toolItems.map((v) => v.target);
    assert('history-tools',
      toolTargets.join(',') === 'tool:first-party:read',
      `工具序 === [read]（实测 ${toolTargets.join(',') || '<none>'}）`);
    assert('history-tools-status',
      toolItems.length === 1 && toolItems.every((v) => v.status === 'success'),
      'read 工具调用 success');
    const historyText = JSON.stringify(history.value);
    assert('history-final', historyText.includes(FINAL_ANSWER), '终结回答含 A05-FINAL-ANSWER');
    assert('history-readback', historyText.includes(READ_SECRET),
      '终结回答回显种子内容 LINGXI-A05-READ-7351（端到端真实往返证据）');
    assert('history-prompt', historyText.includes(READ_MARKER), '历史含读腿 prompt 标记');
    const seedBack = fs.readFileSync(path.join(home, READ_NOTE_REL), 'utf8');
    assert('seed-note-intact', seedBack === READ_NOTE_CONTENT, '种子文件内容未被改动');

    // ── 步骤 8：写腿 = PPG-1 红证据（预期 BLOCKED，不挂 untilFrameType）──
    // rust 预览启动路径不传 --config → workspace 缺位 → write/edit 不注册。
    // 桩按任务书原语义请求 write → 服务拒（provider requested unknown tool）→
    // run 落定 failed.provider_error。drill-07 帧级实测修正 drill-06 记录错误：
    // 服务会发终结帧（assistant_run_end reason=failed.provider_error + error 帧
    // run_failed），只是无任何 assistant_segment 伪回答。
    const traceBeforeWrite = trace.length;
    const writeSession = 'a05-write-session';
    const createdW = await httpJson('write-session-create', 'POST',
      `${conn.baseUrl}/lingxi/v1/sessions`, conn.token,
      { sessionId: writeSession, agentId: 'lingxi', title: writeSession });
    assert('write-session-create', createdW.status === 201,
      `写腿会话 POST → 201（实测 ${createdW.status}）`);
    ws.send(JSON.stringify({ type: 'prompt', sessionId: writeSession, text: WRITE_PROMPT }));
    // 有界轮询服务日志取证；拿不到 = 显式红，绝不挂帧等待。
    let providerErrorLine = null;
    try {
      providerErrorLine = await waitFor('write-provider-error', () => {
        const text = readLatestServiceLog(home);
        const line = text.split('\n').find((l) => l.includes('outcome=failed.provider_error'));
        return line || null;
      }, 20000, 500);
    } catch { providerErrorLine = null; }
    journal('write-provider-error', { line: providerErrorLine });
    assert('write-leg-provider-error', Boolean(providerErrorLine),
      '服务日志出现 outcome=failed.provider_error（PPG-1 写腿红证据）');
    // 帧级反证（drill-07 trace.jsonl 帧 16-18）：写腿唯一 assistant_run_end
    // 且 reason=failed.provider_error，伴随 error 帧 run_failed，全程无
    // assistant_segment 伪终结回答。帧先于日志行落定，500ms 缓冲足够。
    await sleep(500);
    writeTrace();
    const writeFrames = trace.slice(traceBeforeWrite)
      .map((row) => row.frame)
      .filter((f) => f && f.sessionId === writeSession);
    const writeRunEnds = writeFrames.filter((f) => f.type === 'assistant_run_end');
    assert('write-leg-run-end-failed',
      writeRunEnds.length === 1 && writeRunEnds[0].reason === 'failed.provider_error',
      `写腿唯一 run_end 且 reason=failed.provider_error（实测 ${writeRunEnds.map((f) => f.reason).join(',') || '<none>'}）`);
    const writeErrorFrames = writeFrames.filter((f) => f.type === 'error');
    assert('write-leg-error-frame',
      writeErrorFrames.some((f) => f.code === 'run_failed' && f.message === 'failed.provider_error'),
      `写腿 error 帧 run_failed/failed.provider_error（实测 error 帧 ${writeErrorFrames.length}）`);
    const writeSegments = writeFrames.filter((f) => f.type && f.type.startsWith('assistant_segment'));
    assert('write-leg-no-final-segment', writeSegments.length === 0,
      `写腿无 assistant_segment 伪终结回答（实测 ${writeSegments.length}）`);
    // 反证 2：目标文件不得落盘。
    const writeTarget = path.join(home, WRITE_NOTE_REL);
    assert('write-leg-no-file', !fs.existsSync(writeTarget),
      `${WRITE_NOTE_REL} 未落盘（写工具不存在，写腿按预期 BLOCKED）`);
    ws.close();

    // ── 步骤 8b：工具面硬断言（PPG-1 正式证据，14 工具清单落证据）──
    const stubJournal = readStubJournal(outDir);
    fs.writeFileSync(path.join(outDir, 'stub-journal-pretty.json'),
      JSON.stringify(stubJournal, null, 2) + '\n');
    const chatReqs = stubJournal.filter((e) => e.model === 'chat' && e.request);
    assert('stub-chat-requests', chatReqs.length >= 3,
      `桩记录 chat 请求 ≥3（读腿×2 + 写腿×1；实测 ${chatReqs.length}）`);
    const firstChat = chatReqs[0] || null;
    const advertised = firstChat && Array.isArray(firstChat.request.tools)
      ? firstChat.request.tools.map((t) => t && t.function && t.function.name).filter(Boolean)
      : [];
    fs.writeFileSync(path.join(outDir, 'surface-tools.json'),
      JSON.stringify({ advertised }, null, 2) + '\n');
    journal('surface-tools', { advertised });
    assert('surface-nonempty', advertised.length > 0, 'seq-1 请求带工具面声明');
    assert('surface-has-read', advertised.includes('read'), '工具面含已注册 read（读腿前提）');
    assert('surface-no-write-edit', !advertised.includes('write') && !advertised.includes('edit'),
      `工具面显式不含 write/edit（PPG-1 硬断言；实测面：${advertised.join(',')}）`);
    const writeAttempts = stubJournal.filter((e) => e.act === 'write');
    assert('write-attempt-recorded', writeAttempts.length === 1,
      `桩记录恰好一次 write 请求（证明写调用到达供应商边界后被 Rust 拒绝；实测 ${writeAttempts.length}）`);
    const readActs = stubJournal.filter((e) => e.act === 'read');
    assert('read-act-recorded', readActs.length === 1,
      `桩记录恰好一次 read 调用（实测 ${readActs.length}）`);

    // ── 步骤 9：冻结矩阵行 1（关主窗口 → hide，服务存活）──
    let row1Mechanism = 'keyboard';
    if (MANUAL_MODE) {
      row1Mechanism = 'manual-user';
      journal('row1-manual-begin', {});
      const hidden = await manualWait(
        '点击屏幕上的 Lingxi/Electron 窗口使其最前，然后按 Cmd+W（隐藏窗口）',
        async () => {
          try {
            const v = await cdpEvaluate(page.webSocketDebuggerUrl, 'document.visibilityState', false);
            return v === 'hidden';
          } catch { return false; }
        });
      journal('row1-manual-result', { hidden });
      await sleep(500);
    } else {
    let r1 = focusProcessByPid(electron.pid);
    journal('row1-focus', r1);
    await sleep(800);
    if (r1.status === 0) {
      r1 = osascript('tell application "System Events" to keystroke "w" using {command down}');
      journal('row1-close', r1);
    } else {
      // 前台竞态兜底（a06 已定族；无需前台）：AppleEvent set visible false——
      // 与 Cmd+W 同产物（app 级隐藏，窗口保留非销毁）。机制入 verdict 非静默。
      row1Mechanism = 'visibility-appleevent-fallback';
      r1 = osascript(`tell application "System Events" to set visible of (first process whose unix id is ${electron.pid}) to false`);
      journal('row1-close-fallback', { mechanism: row1Mechanism, result: r1 });
    }
    }
    await sleep(2500);
    let pagesAfterClose = [];
    try { pagesAfterClose = await cdpListTargets(cdpPort); } catch { pagesAfterClose = []; }
    // hide ≠ destroy（main.cjs:3252 close→hide）：webContents 存活，CDP 页面
    // 目标仍在；hide 实证 = document.visibilityState 转 'hidden'。
    const mainTarget = pagesAfterClose.find((t) => t.type === 'page' && /index\.html/.test(t.url || ''));
    assert('row1-page-kept', Boolean(mainTarget),
      'hide 后主窗口 CDP 页面目标仍在（hide 非 destroy）');
    let visibility = '<no-target>';
    if (mainTarget) {
      try {
        visibility = await cdpEvaluate(mainTarget.webSocketDebuggerUrl, 'document.visibilityState');
      } catch (err) { visibility = `<evaluate-error ${err.message}>`; }
    }
    assert('row1-hide', visibility === 'hidden',
      `Cmd+W 后 visibilityState=hidden（hide 实测；实测 ${visibility}）`);
    assert('row1-service-alive', pidAlive(conn.pid), 'hide 后 rust 服务仍存活');
    // /lingxi/v1/version 在 rust 路由表不存在（drill-07 实测 404）——
    // 活性探针改用真实路由 /lingxi/v1/health（lib.rs:8066）。
    const healthAfterHide = await httpJson('row1-health', 'GET',
      `${conn.baseUrl}/lingxi/v1/health`, conn.token);
    assert('row1-health', healthAfterHide.status === 200,
      `hide 后 /lingxi/v1/health → 200（实测 ${healthAfterHide.status}）`);

    // ── 步骤 10：冻结矩阵行 3（Cmd+Q 完全退出 → owned SIGTERM 收敛）──
    let row3Mechanism = 'keyboard';
    let r3;
    if (MANUAL_MODE) {
      row3Mechanism = 'manual-user';
      journal('row3-manual-begin', {});
      const exited = await manualWait(
        '再点击一次 Lingxi/Electron 窗口，然后按 Cmd+Q（完全退出应用）',
        () => !pidAlive(electron.pid));
      journal('row3-manual-result', { exited });
      r3 = { status: exited ? 0 : 1, stdout: '', stderr: exited ? '' : 'manual-timeout' };
    } else {
    const r3focus = focusProcessByPid(electron.pid);
    journal('row3-focus', r3focus);
    await sleep(800);
    if (r3focus.status === 0) {
      r3 = osascript('tell application "System Events" to keystroke "q" using {command down}');
      journal('row3-quit', r3);
    } else {
      // 前台竞态兜底（a06 quitClientByPid 同款 Lead 接受形态）：owned SIGTERM——
      // Electron 捕获后走正常退出链（before-quit 收尾），exit code 0 已证。
      row3Mechanism = 'sigterm-graceful';
      try { electron.kill('SIGTERM'); } catch { /* 已退出 */ }
      journal('row3-quit-fallback', { mechanism: row3Mechanism });
      r3 = { status: 0, stdout: '', stderr: '' };
    }
    }
    freezeQuitMechanism = row3Mechanism;
    freezeHideMechanism = row1Mechanism;
    if (!MANUAL_MODE) {
      await waitFor('electron-exit', () => !pidAlive(electron.pid), 45000);
    }
    assert('row3-electron-exit', !pidAlive(electron.pid), 'Electron 主进程退出');
    await waitFor('rust-exit', () => !pidAlive(conn.pid), 45000);
    assert('row3-rust-exit', !pidAlive(conn.pid), `owned rust pid ${conn.pid} 收敛退出`);

    // ── 步骤 11：S3 进程树（退出后）──
    const s3 = snapshotTree('tree-s3', outDir, 'S3-quit', null);
    assertTree('tree-s3', s3, { nodeKernel: 0, rustKernel: 0 });
    if (s3) {
      const lingering = s3.rows.filter((r) => r.args && r.args.includes(home));
      assert('tree-s3-lingering', lingering.length === 0,
        `退出后无携带合成 HOME 路径的残留进程（实测 ${lingering.length}）`);
    }
  } finally {
    stub.child.kill('SIGTERM');
    await cleanupRunProcesses(electron, home, outDir);
  }

  return finish(outDir, home);
}

/** 主进程渲染诊断（console-message 等）落盘在 HOME 下，必须随证据收走。 */
function collectLaunchDiagnostics(home, outDir) {
  if (!home) return;
  const src = path.join(home, 'diagnostics', 'desktop-launch');
  for (const name of ['renderer.log', 'launch.log', 'launch-marker.json']) {
    try {
      fs.copyFileSync(path.join(src, name), path.join(outDir, `desktop-launch-${name}`));
    } catch { /* 可能尚未产生 */ }
  }
}

/**
 * 崩溃/超时路径也必须收尸：drill-03 的 title 等待超时就留下了整棵
 * Electron + rust 子树。按合成 HOME 精确匹配，home 每次运行唯一，不误伤。
 */
async function cleanupRunProcesses(electron, home, outDir) {
  collectLaunchDiagnostics(home, outDir);
  try { electron.kill('SIGTERM'); } catch { /* 已退出 */ }
  await sleep(3000);
  spawnSync('pkill', ['-TERM', '-f', home]);
  await sleep(3000);
  spawnSync('pkill', ['-KILL', '-f', home]);
  journal('cleanup', { electronPid: electron.pid, home });
}

function finish(outDir, home) {
  const READ_STEPS = /^(session-create|handshake|tree-s2|run-idle|history|seed-note-intact)/;
  const WRITE_STEPS = /^(write-|surface-|stub-|read-act-recorded)/;
  const readFails = failures.filter((f) => READ_STEPS.test(f.step));
  const writeFails = failures.filter((f) => WRITE_STEPS.test(f.step));
  const verdict = {
    // 冻结矩阵行 1/3 的实际退出/隐藏机制（键盘 vs 兜底）——a06 同族前台竞态披露。
    manualMode: MANUAL_MODE,
    hideMechanism: freezeHideMechanism,
    quitMechanism: freezeQuitMechanism,
    schema: 'r08-t03-a05-verdict/v2',
    scenario: 'R08-T03-A05 旧Electron壳 + Rust standalone：读腿端到端绿 + 写腿 BLOCKED(PPG-1)',
    verdict: failures.length === 0 ? 'read-pass__write-blocked-PPG-1' : 'fail',
    legs: {
      read: {
        expect: 'pass',
        result: readFails.length === 0 ? 'pass' : 'fail',
        failures: readFails,
      },
      write: {
        expect: 'blocked-PPG-1',
        result: writeFails.length === 0 ? 'blocked-as-expected' : 'deviation',
        failures: writeFails,
      },
    },
    ppg1: {
      id: 'PPG-1',
      title: '预览启动路径无 workspace：核心写工具（write/edit）不注册',
      chain: 'desktop/main.cjs:1437 仅传 --home（无 --config）'
        + ' → rust main.rs:455 workspace_root 唯一来源 = --config.workspace'
        + ' → lib.rs:1666 核心写工具仅 workspace_access=Some 分支注册'
        + ' → 模型工具面（14 项）无 write/edit'
        + ' → 写任务 provider requested unknown tool → failed.provider_error',
      paradigm: '旧 Node = /api/fs/* 任意路径 HTTP 路由（T01 残账 DIRECT 6）；Rust = workspace 约束的工具注册，范式不同',
      status: 'open — 桌面预览 workspace 策略（是否桌面生成 --config 指向默认书桌）属产品语义决策，登记 T04/T05 待裁决，不在 T03 现场发明',
      evidence: ['surface-tools.json', 'stub-journal.jsonl', 'trace.jsonl', 'history.json'],
    },
    failures,
    syntheticHome: home,
    journal: journalEntries,
  };
  fs.writeFileSync(path.join(outDir, 'verdict.json'), JSON.stringify(verdict, null, 2) + '\n');
  console.log(`[r08-t03-a05] verdict=${verdict.verdict} failures=${failures.length} home=${home} out=${outDir}`);
  if (failures.length) {
    for (const f of failures) console.log(`  FAIL[${f.step}] ${f.message}`);
  }
  process.exitCode = failures.length === 0 ? 0 : 1;
}

main().catch((err) => {
  // 崩溃路径也必须落盘 journal（drill-03/04 的 title 超时现场一度全靠日志拼装）。
  try {
    if (crashOutDir) {
      collectLaunchDiagnostics(crashHome, crashOutDir);
      fs.writeFileSync(path.join(crashOutDir, 'crash-verdict.json'), JSON.stringify({
        schema: 'r08-t03-a05-crash/v1',
        error: String(err && err.stack || err),
        syntheticHome: crashHome,
        journal: journalEntries,
      }, null, 2) + '\n');
    }
  } catch { /* 崩溃现场尽力而为 */ }
  console.error(`[r08-t03-a05] orchestrator crashed: ${err.stack || err}`);
  process.exitCode = 2;
});
