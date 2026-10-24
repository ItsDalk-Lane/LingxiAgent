#!/usr/bin/env node
// 通过现有开发入口启动真实 Electron，核对主进程、服务子进程与单实例空间。
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import { spawn, execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const output = path.resolve(process.argv[2] || '');
assert(process.argv[2], '需要全新证据目录');
fs.mkdirSync(output, { recursive: false });
const sandbox = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'lingxi-r08-isolation-')));
const report = { schemaVersion: 1, status: 'IN_PROGRESS', sourceSha: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: repo, encoding: 'utf8' }).trim(), sandbox, startedAt: new Date().toISOString(), tests: [], processes: [] };
const children = [];
const sockets = [];
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
const negativeWrongRoot = process.argv.includes('--expect-wrong-root');
report.negativeControl = negativeWrongRoot ? '错误的期望数据根必须被拒绝' : null;
const digest = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const executable = path.join(repo, 'rust/target/debug/lingxi-service');
report.binarySha256 = digest(executable);
report.inputs = Object.fromEntries(['scripts/launch.js', 'scripts/dev-env.js', 'desktop/bootstrap.cjs', 'desktop/main.cjs', 'desktop/src/shared/single-instance-lock.cjs', 'rust/Cargo.lock'].map(name => [name, digest(path.join(repo, name))]));

async function until(check, description, timeout = 45000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const result = await check();
    if (result) return result;
    await wait(50);
  }
  throw Error('超时：' + description);
}

function processTree(rootPids) {
  const rows = execFileSync('ps', ['-axo', 'pid,ppid,command'], { encoding: 'utf8' }).split('\n').flatMap(line => {
    const match = /^\s*(\d+)\s+(\d+)\s+(.+)$/.exec(line);
    return match ? [{ pid: Number(match[1]), parentPid: Number(match[2]), command: match[3] }] : [];
  });
  const owned = new Set(rootPids);
  let changed = true;
  while (changed) {
    changed = false;
    for (const row of rows) if (owned.has(row.parentPid) && !owned.has(row.pid)) { owned.add(row.pid); changed = true; }
  }
  return rows.filter(row => owned.has(row.pid));
}

function alive(pid) {
  try { process.kill(pid, 0); return true; }
  catch (error) { if (error.code === 'ESRCH') return false; throw error; }
}

async function boot(label, home) {
  fs.mkdirSync(home, { recursive: true });
  const stdout = fs.openSync(path.join(output, label + '.stdout.log'), 'wx');
  const stderr = fs.openSync(path.join(output, label + '.stderr.log'), 'wx');
  const env = { ...process.env, LINGXI_HOME: home, LINGXI_DESKTOP_SERVER_RUNTIME: 'rust', LINGXI_SERVICE_BIN: executable, LINGXI_PRODUCT_DIR: path.join(repo, 'lib') };
  delete env.ELECTRON_RUN_AS_NODE;
  // 仅开启环回调试端口；连接后只读取实际主进程状态和请求正常退出。
  const child = spawn(process.execPath, ['scripts/launch.js', 'electron-dev', '--inspect=127.0.0.1:0'], { cwd: repo, env, stdio: ['ignore', stdout, stderr] });
  fs.closeSync(stdout); fs.closeSync(stderr);
  children.push(child);
  const record = { label, launcherPid: child.pid, requestedHome: home, exitCode: null };
  child.on('exit', code => { record.exitCode = code; });
  report.processes.push(record);
  return { child, record, label, home };
}

async function initializeHome(label, home) {
  fs.mkdirSync(home, { recursive: true });
  const out = fs.openSync(path.join(output, label + '-initialize.stdout.log'), 'wx');
  const err = fs.openSync(path.join(output, label + '-initialize.stderr.log'), 'wx');
  const child = spawn(executable, ['--home', home], { cwd: repo, env: { ...process.env, LINGXI_HOME: home, LINGXI_PRODUCT_DIR: path.join(repo, 'lib') }, stdio: ['ignore', out, err] });
  fs.closeSync(out); fs.closeSync(err);
  children.push(child);
  await until(() => {
    if (child.exitCode !== null) throw Error('副本正规初始化失败，见 ' + label + '-initialize 日志');
    return fs.readFileSync(path.join(output, label + '-initialize.stdout.log'), 'utf8').includes('LINGXI_SERVICE_READY');
  }, '真实服务采用空白合成根');
  child.kill('SIGTERM');
  await until(() => child.exitCode !== null, '初始化服务停止');
  assert.equal(child.exitCode, 0);
  report.initializations ??= [];
  report.initializations.push({ home, pid: child.pid, exitCode: child.exitCode, stampSha256: digest(path.join(home, 'data-epoch.json')) });
}

async function connect(instance) {
  const markerPath = path.join(instance.home, 'diagnostics/desktop-launch/launch-marker.json');
  const marker = await until(() => {
    if (!fs.existsSync(markerPath)) return false;
    const marker = JSON.parse(fs.readFileSync(markerPath, 'utf8'));
    if (marker.payload.status.endsWith('failed')) throw Error('桌面启动失败：' + marker.payload.status);
    return marker.payload.status === 'main-loaded' && marker;
  }, '桌面主入口载入');
  instance.record.mainPid = marker.payload.pid;
  const endpoint = await until(() => {
    const text = fs.readFileSync(path.join(output, instance.label + '.stderr.log'), 'utf8');
    const endpoint = /Debugger listening on (ws:\/\/127\.0\.0\.1:\d+\/\S+)/.exec(text)?.[1];
    if (instance.child.exitCode !== null) throw Error('桌面提前退出，见 ' + instance.label + ' 日志');
    return endpoint;
  }, '主进程调试口');
  const socket = new WebSocket(endpoint);
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  sockets.push(socket);
  let id = 0;
  const pending = new Map();
  socket.onmessage = event => { const reply = JSON.parse(event.data); const pair = pending.get(reply.id); if (pair) { pending.delete(reply.id); reply.error ? pair.reject(Error(JSON.stringify(reply.error))) : pair.resolve(reply.result); } };
  const command = (method, params) => new Promise((resolve, reject) => { const key = ++id; pending.set(key, { resolve, reject }); socket.send(JSON.stringify({ id: key, method, params })); });
  const evaluate = async expression => {
    const result = await command('Runtime.evaluate', { expression, returnByValue: true });
    if (result.exceptionDetails) throw Error(JSON.stringify(result.exceptionDetails));
    return result.result.value;
  };
  instance.evaluate = evaluate;
  instance.socket = socket;
  const state = await until(async () => {
    const state = await evaluate(`(()=>{const e=process.mainModule.require('electron');return e.app.isReady()?{pid:process.pid,home:process.env.LINGXI_HOME,nodeBin:process.env.LINGXI_DEV_NODE_BIN,userData:e.app.getPath('userData'),sessionData:e.app.getPath('sessionData'),electron:process.versions.electron,windows:e.BrowserWindow.getAllWindows().map(w=>({id:w.id,url:w.webContents.getURL()}))}:null})()`);
    return state;
  }, 'Electron 就绪');
  instance.record.actual = state;
  assert.equal(fs.realpathSync(state.home), fs.realpathSync(instance.home));
  assert.equal(state.userData, path.join(fs.realpathSync(instance.home), '.electron-user-data'));
  assert.equal(state.sessionData, state.userData);
  assert.equal(state.nodeBin, process.execPath);
  const runtime = path.join(instance.home, 'lingxi-service/instance.json');
  const info = await until(() => fs.existsSync(runtime) && JSON.parse(fs.readFileSync(runtime)), '真实 Rust 子进程');
  assert(Number.isInteger(info.pid));
  instance.record.servicePid = info.pid;
  assert.equal(fs.realpathSync(info.homePath), fs.realpathSync(instance.home));
  process.kill(info.pid, 0);
  // 不将本轮令牌写入交付日志。
  instance.record.instance = { pid: info.pid, home: info.homePath, address: info.bindAddr };
  if (negativeWrongRoot) assert.equal(fs.realpathSync(state.home), path.join(sandbox, 'wrong-expected-root'), '数据根与期望副本不一致');
  return state;
}

try {
  const homeA = path.join(sandbox, 'a', 'copy');
  const homeB = path.join(sandbox, 'b', 'copy');
  // 夹具先由真实服务正式采用，绝不伪造印章或删过渡日志。
  await initializeHome('a', homeA);
  await initializeHome('b', homeB);
  const a = await boot('a', homeA);
  const aState = await connect(a);
  const b = await boot('b', homeB);
  const bState = await connect(b);
  assert.notEqual(aState.userData, bState.userData);
  assert.notEqual(a.record.servicePid, b.record.servicePid);
  report.tests.push({ name: '不同路径同名副本独立运行、真实数据根和子进程正确', status: 'PASS' });
  const duplicate = await boot('duplicate', a.home);
  await until(() => duplicate.child.exitCode !== null, '同根重复客户端退出');
  assert.equal(duplicate.child.exitCode, 0);
  assert.equal(JSON.parse(fs.readFileSync(path.join(a.home, 'lingxi-service/instance.json'))).pid, a.record.servicePid);
  process.kill(aState.pid, 0); process.kill(bState.pid, 0);
  fs.writeFileSync(path.join(output, 'process-tree.txt'), execFileSync('ps', ['-axo', 'pid,ppid,command'], { encoding: 'utf8' }).split('\n').filter(line => [sandbox, repo + '/node_modules/electron', executable].some(text => line.includes(text))).join('\n') + '\n');
  report.tests.push({ name: '同根重复启动未开启第二服务，其他副本仍存活', status: 'PASS' });
  const alias = path.join(sandbox, 'alias');
  fs.symlinkSync(a.home, alias, 'junction');
  const aliasClient = await boot('alias-duplicate', alias);
  await until(() => aliasClient.child.exitCode !== null, '同一真实根路径别名客户端退出');
  assert.equal(aliasClient.child.exitCode, 0);
  assert.equal(JSON.parse(fs.readFileSync(path.join(a.home, 'lingxi-service/instance.json'))).pid, a.record.servicePid);
  report.tests.push({ name: '真实 Electron 路径别名与原根共用单实例锁', status: 'PASS' });
  report.ownedProcessTree = processTree([a.record.actual.pid, b.record.actual.pid, a.record.servicePid, b.record.servicePid]);
  for (const instance of [a, b]) {
    await instance.evaluate(`process.mainModule.require('electron').app.quit(); true`);
    instance.socket.close();
    await until(() => instance.child.exitCode !== null, '正常退出桌面', 30000);
    assert.equal(instance.child.exitCode, 0);
    await until(() => { try { process.kill(instance.record.servicePid, 0); return false; } catch (error) { if (error.code === 'ESRCH') return true; throw error; } }, '自有 Rust 服务退出', 30000);
  }
  await until(() => report.ownedProcessTree.every(row => !alive(row.pid)), '全部 Electron/Rust 后代退出', 30000);
  report.residue = report.ownedProcessTree.filter(row => alive(row.pid));
  report.tests.push({ name: '正常退出清理各自服务、无残留写者', status: 'PASS' });
  report.status = 'PASS';
} catch (error) {
  report.status = 'FAIL'; report.error = String(error.stack); process.exitCode = 1;
} finally {
  for (const socket of sockets) socket.close();
  // 失败现场保留，清理仅限本次明确记录的进程。
  for (const instance of children) if (instance.exitCode === null) instance.kill('SIGTERM');
  for (const record of report.processes) for (const pid of [record.mainPid, record.actual?.pid, record.servicePid].filter(Boolean)) {
    try { process.kill(pid, 'SIGTERM'); } catch (error) { if (error.code !== 'ESRCH') report.cleanupError = String(error); }
  }
  await wait(500);
  report.forcedCleanup = [];
  for (const record of report.processes) for (const pid of new Set([record.mainPid, record.actual?.pid, record.servicePid].filter(Boolean))) {
    try { process.kill(pid, 0); process.kill(pid, 'SIGKILL'); report.forcedCleanup.push(pid); }
    catch (error) { if (error.code !== 'ESRCH') report.cleanupError = String(error); }
  }
  if (report.forcedCleanup.length && report.status === 'PASS') {
    report.status = 'FAIL'; report.error = '正常退出后仍需强制清理进程'; process.exitCode = 1;
  }
  report.finishedAt = new Date().toISOString();
  fs.writeFileSync(path.join(output, 'result.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify({ status: report.status, tests: report.tests, evidence: output, error: report.error }));
}
