#!/usr/bin/env node
// R08-T03 进程树记录器（A05/A06 证据用）
//
// 采集全量进程表（pid/ppid/comm/args），按 Lingxi 演练语义分类：
//   rust-kernel   lingxi-service 二进制（Rust 服务内核）
//   node-kernel   旧 Node 内核（dev: server/bootstrap.ts；打包: hana-server / bootstrap.js / bundle/index.js）
//   electron-ui   Electron 壳及其 Helper（渲染/GPU 等纯 UI 进程）
//   other         其余系统进程
//
// 用法：
//   node scripts/rust-tauri/r08_t03_process_tree.mjs --out <file.json> [--label S1] [--root-pid <pid>]
//
// --root-pid 给定时额外输出该进程的祖先链与全部后代（subtree），用于
// 「退出超时残留 PID」与「Electron-UI-only vs 旧 Node 内核仍在执行」的精确对照。
// 平台：darwin/linux 走 ps；win32 走 PowerShell CIM（演练仅在 POSIX 执行，win32 属平台限定格）。

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const SCHEMA = 'r08-t03-process-tree/v1';

function parseArgs(argv) {
  const opts = { out: null, label: null, rootPid: null, selfTest: false };
  for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i];
    if (a === '--out') opts.out = argv[++i];
    else if (a === '--label') opts.label = argv[++i];
    else if (a === '--root-pid') opts.rootPid = Number(argv[++i]);
    else if (a === '--self-test') opts.selfTest = true;
    else throw new Error(`unknown argument: ${a}`);
  }
  if (!opts.selfTest && !opts.out) throw new Error('missing --out <file.json>');
  if (opts.rootPid !== null && (!Number.isInteger(opts.rootPid) || opts.rootPid <= 0)) {
    throw new Error('--root-pid must be a positive integer');
  }
  return opts;
}

function collectPosix() {
  const out = execFileSync('ps', ['-axo', 'pid=,ppid=,comm=,args='], { encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 });
  const rows = [];
  for (const line of out.split('\n')) {
    const m = line.match(/^\s*(\d+)\s+(\d+)\s+(\S+)\s*(.*)$/);
    if (!m) continue;
    rows.push({ pid: Number(m[1]), ppid: Number(m[2]), comm: m[3], args: m[4] || '' });
  }
  return rows;
}

function collectWin32() {
  // PowerShell CIM；CommandLine 可能为 null（系统进程），归一化为空串。
  const script = 'Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name,CommandLine | ConvertTo-Json -Compress';
  const out = execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  const parsed = JSON.parse(out || '[]');
  const list = Array.isArray(parsed) ? parsed : [parsed];
  return list.map((p) => ({
    pid: Number(p.ProcessId),
    ppid: Number(p.ParentProcessId ?? 0),
    comm: String(p.Name || ''),
    args: String(p.CommandLine || ''),
  }));
}

const NODE_KERNEL_PATTERNS = [
  /server[\\/]bootstrap\.ts/,   // dev：node server/bootstrap.ts
  /hana-server/,                // 打包 POSIX wrapper / win32 hana-server.exe
  /[\\/]bootstrap\.js/,         // 打包 win32 显式 bootstrap.js
  /[\\/]bundle[\\/]index\.js/,  // 打包 server 入口（LINGXI_SERVER_ENTRY 兜底）
];
// macOS ps comm 截断 16 字节（实测 '/Users/study_sup'），Electron/rust 全变
// other（drill-06 红证据）。Electron 与 rust 一律按 args（完整命令行）判：
// 主进程 …/Electron.app/Contents/MacOS/Electron，Helper 路径含
// 'Electron Helper[ (Renderer|GPU…)].app/Contents/MacOS/Electron Helper'；
// 打包发行版主二进制已改名，不会误命中其它正式 Electron 应用。
const ELECTRON_UI_PATTERN = /[\\/]MacOS[\\/]Electron( Helper( \((Renderer|GPU|Plugin)\))?)?(\s|$)|[\\/]electron(.exe)?(\s|$)/i;
const RUST_KERNEL_PATTERN = /(^|\s)\S*[\\/]lingxi-service(\s|$)/;

function classify(row) {
  const base = path.basename(row.comm).toLowerCase();
  if (base === 'lingxi-service' || base === 'lingxi-service.exe') return 'rust-kernel';
  if (RUST_KERNEL_PATTERN.test(row.args)) return 'rust-kernel';
  if (NODE_KERNEL_PATTERNS.some((re) => re.test(row.args) || re.test(row.comm))) return 'node-kernel';
  // comm 快路径必须带词边界：^electron 裸前缀会把 Electronical 误伤进 electron-ui
  //（--self-test 合成样本逮获）；截断形态（'/Users/study_sup'）反正落到 args 主判。
  if (ELECTRON_UI_PATTERN.test(row.args) || /^electron(\.exe)?(\s|$|\()/i.test(row.comm)) return 'electron-ui';
  return 'other';
}

function buildSubtree(rows, rootPid) {
  const byPid = new Map(rows.map((r) => [r.pid, r]));
  const children = new Map();
  for (const r of rows) {
    if (!children.has(r.ppid)) children.set(r.ppid, []);
    children.get(r.ppid).push(r.pid);
  }
  const ancestors = [];
  let cur = byPid.get(rootPid) || null;
  const seen = new Set();
  while (cur && !seen.has(cur.pid)) {
    seen.add(cur.pid);
    ancestors.push(cur);
    cur = byPid.get(cur.ppid) || null;
  }
  const descendants = [];
  const queue = [...(children.get(rootPid) || [])];
  const seenDesc = new Set();
  while (queue.length) {
    const pid = queue.shift();
    if (seenDesc.has(pid)) continue;
    seenDesc.add(pid);
    const row = byPid.get(pid);
    if (row) descendants.push(row);
    for (const c of children.get(pid) || []) queue.push(c);
  }
  return { rootPid, ancestors, descendants };
}

// --self-test：合成样本断言（不采真实进程表）。样本覆盖 batch-C 形态 +
// drill-06 教训（macOS comm 截断 16 字节后只能靠 args 分类）。任一不符即非零退出。
function runSelfTest() {
  const cases = [
    // [row, expected, note]
    [{ comm: 'lingxi-service', args: 'rust/target/debug/lingxi-service --home /x' }, 'rust-kernel', 'comm 精确命中'],
    [{ comm: '/Users/study_sup', args: '/repo/rust/target/debug/lingxi-service --home /private/tmp/home' }, 'rust-kernel', 'comm 截断 → args 兜底（drill-06）'],
    [{ comm: '/Users/study_sup', args: '/x/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron .' }, 'electron-ui', 'Electron 主进程 comm 截断（drill-06）'],
    [{ comm: '/Users/study_sup', args: '/x/Electron.app/Contents/MacOS/Electron Helper (Renderer).app/Contents/MacOS/Electron Helper (Renderer) --type=renderer' }, 'electron-ui', 'Helper (Renderer)'],
    [{ comm: '/Users/study_sup', args: '/x/Electron.app/Contents/MacOS/Electron Helper (GPU).app/Contents/MacOS/Electron Helper (GPU) --type=gpu-process' }, 'electron-ui', 'Helper (GPU)'],
    [{ comm: '/Users/study_sup', args: '/x/Electron.app/Contents/MacOS/Electron Helper.app/Contents/MacOS/Electron Helper --type=utility' }, 'electron-ui', 'Helper 无后缀'],
    [{ comm: 'node', args: 'node server/bootstrap.ts' }, 'node-kernel', 'dev node 内核'],
    [{ comm: 'hana-server', args: '/opt/hana/hana-server --port 8511' }, 'node-kernel', '打包 POSIX wrapper'],
    [{ comm: 'node', args: 'node /x/bootstrap.js' }, 'node-kernel', '打包 bootstrap.js'],
    [{ comm: 'node', args: 'node /x/bundle/index.js' }, 'node-kernel', '打包 bundle/index.js'],
    [{ comm: 'Electronical', args: '/x/Electronical --flag' }, 'other', 'Electronical 不误伤'],
    [{ comm: 'node', args: 'node /x/scripts/tool.mjs' }, 'other', '普通 node 脚本'],
    [{ comm: 'cat', args: 'cat /tmp/lingxi-service.log' }, 'other', '路径含 lingxi-service 但非可执行 token'],
    [{ comm: 'zsh', args: '-zsh' }, 'other', '无关进程'],
  ];
  const failures = [];
  for (const [row, expected, note] of cases) {
    const actual = classify(row);
    if (actual !== expected) failures.push({ row, expected, actual, note });
  }
  if (failures.length) {
    console.error(`[r08-t03-tree] self-test FAILED: ${failures.length}/${cases.length}`);
    for (const f of failures) console.error(JSON.stringify(f));
    process.exit(1);
  }
  console.log(`[r08-t03-tree] self-test OK: ${cases.length}/${cases.length} 合成样本分类正确`);
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.selfTest) { runSelfTest(); return; }
  const rows = (process.platform === 'win32' ? collectWin32() : collectPosix())
    .map((r) => ({ ...r, class: classify(r) }));
  rows.sort((a, b) => a.pid - b.pid);
  const counts = { 'rust-kernel': 0, 'node-kernel': 0, 'electron-ui': 0, other: 0 };
  for (const r of rows) counts[r.class] += 1;
  const payload = {
    schema: SCHEMA,
    capturedAt: new Date().toISOString(),
    platform: process.platform,
    label: opts.label,
    counts,
    rows,
  };
  if (opts.rootPid !== null) payload.subtree = buildSubtree(rows, opts.rootPid);
  fs.mkdirSync(path.dirname(path.resolve(opts.out)), { recursive: true });
  fs.writeFileSync(opts.out, `${JSON.stringify(payload, null, 2)}\n`);
  const summary = `[r08-t03-tree] ${opts.label || 'snapshot'} @ ${payload.capturedAt}: ` +
    `rust-kernel=${counts['rust-kernel']} node-kernel=${counts['node-kernel']} ` +
    `electron-ui=${counts['electron-ui']} other=${counts.other} → ${opts.out}`;
  console.log(summary);
}

main();
