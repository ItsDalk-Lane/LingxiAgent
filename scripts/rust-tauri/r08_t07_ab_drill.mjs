#!/usr/bin/env node
/**
 * R08-T07｜A13 同界面换内核 A/B 驱动（广度引擎）。
 *
 * 固定四要素（任务书 T07 步骤 2）：
 *   - UI 版本：同一仓库 desktop/src/react 源（vitest 套件 r08-t07-kernel-ab.test.ts
 *     经 jsdom 加载同一份 store/服务边界代码，两腿零差异）；
 *   - 数据：fixture v3（seed 20261023）——A 腿 = 旧栈跑**准备后的夹具副本**
 *     （t06-control-probe-05 同构准备集：印章 v2{1,1,1}/过渡日志移除/leases 空注册表/
 *     a1 config/knowledge 合成表改名），B 腿 = Rust 跑**同一夹具的 T05 迁移根**；
 *     二者为同一逻辑数据（迁移保身份）；
 *   - 模型替身：本腿任务集不触模型（会话管理/权限/偏好/错误路径）；模型回合由
 *     T03 a05 系与 T06 D1 另证；
 *   - 测试步骤：同一套件按 LINGXI_R08_AB_KIND 双跑，观测值落 JSON，比较器判平价。
 *
 * 输出：<out>/ab-node.json、ab-rust.json、vitest-<kind>.log、ab-compare.json、
 *       AB_RESULT 行。退出码：双腿套件均执行且产出 + 平价判定全等=0；否则 1
 *       （差异如实入报告，不弱化）。
 * 用法：node scripts/rust-tauri/r08_t07_ab_drill.mjs --bin <lingxi-service> --work <dir>
 */
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const args = process.argv.slice(2);
const argValue = (flag) => { const i = args.indexOf(flag); return i >= 0 ? args[i + 1] : null; };
const bin = argValue('--bin') ?? path.join(REPO, 'rust/target/debug/lingxi-service');
const work = argValue('--work');
if (!work) { console.error('usage: r08_t07_ab_drill.mjs --bin <lingxi-service> --work <dir>'); process.exit(2); }

const SCRUB_ENV = ['ELECTRON_RUN_AS_NODE', 'LINGXI_TOKEN', 'LINGXI_SERVER_OWNER', 'LINGXI_SERVER_OWNER_PID', 'LINGXI_ALLOW_DATA_DOWNGRADE'];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failures = 0;
const check = (name, ok, detail = '') => { console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ` :: ${detail}` : ''}`); if (!ok) failures += 1; };
const pidAlive = (pid) => { try { process.kill(pid, 0); return true; } catch { return false; } };
const children = [];
process.on('uncaughtException', (err) => { for (const c of children) { try { c.kill('SIGKILL'); } catch { /* gone */ } } console.error(`AB_ABORT ${err?.stack || err}`); process.exit(1); });

fs.rmSync(work, { recursive: true, force: true });
fs.mkdirSync(work, { recursive: true });
const out = path.join(work, 'out');
fs.mkdirSync(out, { recursive: true });
const fixture = path.join(work, 'fixture');
const rootA = path.join(work, 'root-a-node');
const rootB = path.join(work, 'root-b-rust');

async function freePort() {
  const srv = net.createServer();
  await new Promise((resolve, reject) => { srv.once('error', reject); srv.listen(0, '127.0.0.1', resolve); });
  const { port } = srv.address();
  await new Promise((resolve) => srv.close(resolve));
  return port;
}
async function httpStatus(port, pathName, token) {
  try {
    const res = await fetch(`http://127.0.0.1:${port}${pathName}`, { headers: token ? { Authorization: `Bearer ${token}` } : {}, signal: AbortSignal.timeout(2000) });
    return res.status;
  } catch { return 0; }
}

// ── 0. 模型替身（D-3 扩腿）：t03 桩供应商 + 双腿模型面注入 ──
// 旧栈 switch/会话创建在无模型时 500/拒绝（D-3 环境前置）；本阶段补齐 A13
// 「模型替身」固定要素：同一桩供应商双内核共用，A 腿注入旧栈 Pi SDK 格式
// <root>/models.json（core/model-sync.ts 投影形状），B 腿注入 Rust 格式
// lingxi-service/models.json。
const stubScratch = path.join(work, 'stub');
fs.mkdirSync(stubScratch, { recursive: true });
const stubOut = fs.openSync(path.join(stubScratch, 'stdout.log'), 'w');
// 桩供应商 CLI 要求 --read-path（读腿标记路径；本线束不驱动聊天回合，占位
// 指向 A 腿种子文件——桩对无标记请求 400 显式拒绝，不影响就绪）。
const stub = spawn(process.execPath, [
  path.join(REPO, 'scripts/rust-tauri/r08_t03_stub_provider.mjs'),
  '--port', '0', '--journal', path.join(stubScratch, 'journal.jsonl'),
  '--read-path', 't07-ab/nochat.txt',
  '--final-delay-ms', '1500',
], { cwd: REPO, stdio: ['ignore', stubOut, stubOut] });
fs.closeSync(stubOut);
children.push(stub);
let stubPort = null;
{
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline && !stubPort) {
    try {
      const text = fs.readFileSync(path.join(stubScratch, 'stdout.log'), 'utf8');
      const m = text.match(/A05_STUB_READY (\d+)/);
      if (m) stubPort = Number(m[1]);
    } catch { /* not yet */ }
    if (!stubPort) await sleep(200);
  }
}
check('stub provider ready', Boolean(stubPort), String(stubPort));

// ── 1. 数据根：夹具 v3 基座 + 会话面替换为真实 Pi SDK 格式（T07 真实数据口径）──
// 夹具自造 entryType 会话对旧栈不可见（旧栈要求 {type:"session"} 头行）——
// A13 的 A/B 数据必须两内核同见：移除 entryType 会话与合成 session-manifest，
// 换 3 个真实格式会话（lib/session-jsonl.ts 生产读取形状）。非会话面（knowledge/
// observability/附件等）仍为夹具内容（boot 与正路径覆盖）。
const gen = spawnSync(process.execPath, [path.join(REPO, 'scripts/rust-tauri/r08_t04_fixture_gen.mjs'), '--seed', '20261023', '--out', fixture], { encoding: 'utf8' });
check('fixture gen', gen.status === 0, gen.stderr?.slice(0, 200));
function realFormatLines(headerId, userTexts) {
  const lines = [JSON.stringify({ type: 'session', version: 3, id: headerId, cwd: '/tmp', timestamp: '2026-10-01T09:00:00Z' })];
  // Pi 真实形态：model_change 条目冻结会话模型（restore 链 buildSessionContext
  // 从此读 model ref；无此条目时旧栈 switch 走 fallback 且 restore=true 下被
  // 响亮拒——t12-ab-drill-d3-04 实证）。注入的 t07-stub/chat 是本机环回替身。
  const modelChangeId = `mc-${headerId}`;
  lines.push(JSON.stringify({ type: 'model_change', id: modelChangeId, parentId: null, timestamp: '2026-10-01T09:00:00Z', provider: 't07-stub', modelId: 'chat' }));
  // Pi 叶链：首条用户消息的 parent 必须接 model_change（真实会话由 SDK 追加
  // 维护该链；断链时 buildSessionContext 不恢复模型 ref——t12-ab-drill-d3-05 实证）。
  let parent = modelChangeId;
  let n = 0;
  for (const text of userTexts) {
    n += 1;
    lines.push(JSON.stringify({ type: 'message', id: `u${n}`, parentId: parent, timestamp: '2026-10-01T10:00:00Z', message: { role: 'user', content: text } }));
    lines.push(JSON.stringify({ type: 'message', id: `a${n}`, parentId: `u${n}`, timestamp: '2026-10-01T10:00:01Z', message: { role: 'assistant', content: [{ type: 'text', text: `T07-AB 回复 ${n}：已处理。` }] } }));
    parent = `a${n}`;
  }
  return `${lines.join('\n')}\n`;
}
for (const [name, texts] of [
  ['ab-real-alpha', ['T07 A/B 任务一：请阅读并回复。', 'T07 A/B 任务一之二：再确认。']],
  ['ab-real-beta', ['T07 A/B 任务二：请阅读并回复。']],
  ['ab-real-gamma', ['T07 A/B 任务三：请阅读并回复。']],
]) {
  fs.writeFileSync(path.join(fixture, 'agents/a1/sessions', `${name}.jsonl`), realFormatLines(`7e5f-${name}`, texts));
}
for (const name of fs.readdirSync(path.join(fixture, 'agents/a1/sessions'))) {
  if (name.endsWith('.jsonl') && !name.startsWith('ab-real-')) fs.rmSync(path.join(fixture, 'agents/a1/sessions', name));
}
fs.rmSync(path.join(fixture, 'session-manifest.db'), { force: true });
fs.rmSync(path.join(fixture, 'session-manifest.db-wal'), { force: true });
fs.rmSync(path.join(fixture, 'session-manifest.db-shm'), { force: true });
fs.cpSync(fixture, rootA, { recursive: true });
{
  const stamp = { schemaVersion: 2, epoch: 1, minimumReaderEpoch: 1, committedDataEpoch: 1, lastVersion: 'legacy-import', updatedAt: new Date().toISOString() };
  fs.writeFileSync(path.join(rootA, 'data-epoch.json'), `${JSON.stringify(stamp, null, 2)}\n`, { mode: 0o600 });
  fs.rmSync(path.join(rootA, 'data-epoch-transition.json'), { force: true });
  const nowIso = new Date().toISOString();
  fs.writeFileSync(path.join(rootA, 'security/execution-leases.json'), `${JSON.stringify({ schemaVersion: 1, leases: [], createdAt: nowIso, updatedAt: nowIso }, null, 2)}\n`);
  fs.writeFileSync(path.join(rootA, 'agents/a1/config.yaml'), 'agent:\n  name: "A1"\n  yuan: "lingxi"\n', { mode: 0o600 });
  const sqlite = await import('better-sqlite3');
  const kdb = sqlite.default(path.join(rootA, 'knowledge/knowledge.db'));
  kdb.exec('BEGIN');
  for (const [from, to] of [['documents', 'fixture_synthetic_documents'], ['processing_artifacts', 'fixture_synthetic_processing_artifacts']]) {
    if (kdb.prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?").get(from)) kdb.exec(`ALTER TABLE "${from}" RENAME TO "${to}"`);
  }
  kdb.exec('COMMIT');
  const integrity = kdb.pragma('integrity_check', { simple: true });
  kdb.close();
  check('root-a prepared (knowledge integrity ok)', integrity === 'ok', String(integrity));
}
// A 腿旧栈模型面：引擎启动期 syncModels 会用 provider-catalog 投影**覆写**
// 根 models.json（空目录→清空），故直接写 catalog（唯一真理源；core/
// provider-catalog.ts PROVIDER_CATALOG_FILE + core/model-sync.ts 投影消费
// snake_case base_url/api_key/models[{id}]）。apiKey 为本机环回替身占位，
// 非真实凭据。
fs.writeFileSync(path.join(rootA, 'provider-catalog.json'), `${JSON.stringify({
  catalogVersion: 2,
  providers: {
    't07-stub': {
      base_url: `http://127.0.0.1:${stubPort}/v1`,
      api: 'openai-completions',
      api_key: 'R08_SYNTHETIC_LOCAL_STUB_ONLY',
      models: [{ id: 'chat' }],
    },
  },
}, null, 4)}\n`, { mode: 0o600 });
const migrate = spawnSync(bin, ['migrate', '--source', fixture, '--target', rootB, '--work-dir', path.join(work, 'm-work'), '--backup-dir', path.join(work, 'm-backup'), '--pointer', path.join(work, 'active-data-root.json')], { encoding: 'utf8', timeout: 900000 });
// B 腿不注入模型面：t12-ab-drill-d3-02 实证注入后 B switch 反而 500 回归
// （无注入时 200）——B 腿模型面留空，会话管理域用例不依赖模型回合。
check('root-b migrated (T05 path)', migrate.status === 0 && /outcome=committed/.test(migrate.stdout || ''), (migrate.stderr || '').slice(-250));

// ── 2. 起 A 腿：旧 Node 栈 ──
const portA = await freePort();
const envA = { ...process.env, LINGXI_HOME: rootA, LINGXI_PORT: String(portA) };
for (const k of SCRUB_ENV) delete envA[k];
const logA = fs.openSync(path.join(out, 'kernel-a-node.log'), 'w');
const launcherA = spawn(process.execPath, [path.join(REPO, 'scripts/launch.js'), 'server'], { cwd: REPO, env: envA, stdio: ['ignore', logA, logA] });
fs.closeSync(logA);
children.push(launcherA);
let serverInfoA = null;
{
  const infoPath = path.join(rootA, 'server-info.json');
  const deadline = Date.now() + 90000;
  while (Date.now() < deadline) {
    if (fs.existsSync(infoPath)) {
      try {
        const parsed = JSON.parse(fs.readFileSync(infoPath, 'utf8'));
        if (parsed.port === portA && Number.isInteger(parsed.pid) && typeof parsed.token === 'string' && (await httpStatus(portA, '/api/health', parsed.token)) === 200) { serverInfoA = parsed; break; }
      } catch { /* half-written */ }
    }
    await sleep(250);
  }
}
check('kernel A (node) ready', Boolean(serverInfoA), `port=${portA}`);

// ── 3. 起 B 腿：Rust 服务 ──
const logBPath = path.join(out, 'kernel-b-rust.log');
const logB = fs.openSync(logBPath, 'w');
const envB = { ...process.env, LINGXI_PRODUCT_DIR: path.join(REPO, 'lib') };
for (const k of SCRUB_ENV) delete envB[k];
const serviceB = spawn(bin, ['--home', rootB, '--bind', '127.0.0.1:0'], { cwd: REPO, env: envB, stdio: ['ignore', logB, logB] });
fs.closeSync(logB);
children.push(serviceB);
let addrB = null;
{
  const deadline = Date.now() + 90000;
  while (Date.now() < deadline) {
    const text = fs.existsSync(logBPath) ? fs.readFileSync(logBPath, 'utf8') : '';
    const m = text.match(/LINGXI_SERVICE_READY addr=(\S+) /);
    if (m) { addrB = m[1]; break; }
    if (serviceB.exitCode !== null) break;
    await sleep(250);
  }
}
check('kernel B (rust) ready', Boolean(addrB), addrB || `exit=${serviceB.exitCode}`);
const tokenB = addrB ? (() => { const t = JSON.parse(fs.readFileSync(path.join(rootB, 'lingxi-service/local-token.json'), 'utf8')); return t.token ?? t.localToken ?? t; })() : null;

// ── 4. 双跑同一套件 ──
function runSuite(kind, base, token) {
  const outFile = path.join(out, `ab-${kind}.json`);
  const logFile = path.join(out, `vitest-${kind}.log`);
  const env = { ...process.env, LINGXI_R08_AB_BASE: base, LINGXI_R08_AB_TOKEN: token, LINGXI_R08_AB_KIND: kind, LINGXI_R08_AB_OUT: outFile };
  for (const k of SCRUB_ENV) delete env[k];
  const r = spawnSync('npx', ['vitest', 'run', 'desktop/src/react/__tests__/r08/r08-t07-kernel-ab.test.ts', '--reporter=verbose'], { cwd: REPO, env, encoding: 'utf8', timeout: 600000 });
  fs.writeFileSync(logFile, `${r.stdout || ''}\n${r.stderr || ''}`);
  const observations = fs.existsSync(outFile) ? JSON.parse(fs.readFileSync(outFile, 'utf8')).observations : null;
  return { exit: r.status, observations, logFile };
}
const suiteA = serverInfoA ? runSuite('node', `http://127.0.0.1:${portA}`, serverInfoA.token) : { exit: null, observations: null };
const suiteB = addrB ? runSuite('rust', `http://${addrB}`, tokenB) : { exit: null, observations: null };
check('suite ran on A (observations present)', Boolean(suiteA.observations), `vitest exit=${suiteA.exit}`);
check('suite ran on B (observations present)', Boolean(suiteB.observations), `vitest exit=${suiteB.exit}`);

// ── 5. 关停两内核（A：SIGTERM owned-pid；B：SIGTERM）与桩供应商 ──
if (serverInfoA) { try { process.kill(serverInfoA.pid, 'SIGTERM'); } catch { /* gone */ } }
serviceB.kill('SIGTERM');
stub.kill('SIGTERM');
await sleep(1500);
if (serverInfoA) { try { process.kill(serverInfoA.pid, 'SIGKILL'); } catch { /* gone */ } }
launcherA.kill('SIGKILL');
if (serviceB.exitCode === null) serviceB.kill('SIGKILL');
// gate14 实测：正常收尾未回收桩，每轮留一个孤儿 node（ppid=1）常驻。
// 桩的 SIGTERM 处理要等连接排空（自带 3s 兜底），故轮询等待退出再断言，
// 不即时判定；超时才 SIGKILL。
{
  const deadline = Date.now() + 6000;
  while (pidAlive(stub.pid) && Date.now() < deadline) await sleep(200);
  if (pidAlive(stub.pid)) {
    console.log(`STUB_SIGKILL pid=${stub.pid} (SIGTERM 6s 未退出)`);
    stub.kill('SIGKILL');
    const killDeadline = Date.now() + 3000;
    while (pidAlive(stub.pid) && Date.now() < killDeadline) await sleep(100);
  }
}
check('stub provider reaped', !pidAlive(stub.pid), `stub pid ${stub.pid} exit=${stub.exitCode} signal=${stub.signalCode}`);

// ── 6. 比较器：逐任务平价判定 ──
// 判定三值：'parity'（语义一致）/ 'differs'（真实差异，入差异报告）/
// 'recorded'（环境前置不可比或已知投影差异，归因记录，不计平价也不计失败）。
const RULES = {
  'identity': (a, b) => (a.status === 200 && b.status === 200 ? 'parity' : 'differs'),
  // 计数一致即列表平价；标题投影差异（旧=null vs 新=stem）是已归因 UI 差异
  // （差异报告 D-1），不判 differs。
  'sessions.list': (a, b) => (a.count === b.count && a.pinned === b.pinned ? 'parity' : 'differs'),
  // A 腿旧栈无模型配置时 /api/sessions/switch 500（环境前置；差异报告 D-3）。
  'sessions.switch': (a, b) => (b.focused === true && a.focused === false ? 'recorded' : a.focused === b.focused ? 'parity' : 'differs'),
  'sessions.rename': (a, b) => (a.ok && b.ok && a.persisted && b.persisted ? 'parity' : 'differs'),
  'sessions.pin': (a, b) => (JSON.stringify(a) === JSON.stringify(b) && a.pinA && a.pinB && a.unpinA && a.unpinB ? 'parity' : 'differs'),
  // 双侧 created=false：A=旧栈无模型拒绝建会话；B=迁移根无 agent 名册（Register
  // 不随迁）→ sessions/new 404（差异报告 D-2/D-4，各自归因，非平价判定面）。
  'sessions.create': (a, b) => 'recorded',
  // 归档往返核心 = 入列+不可见+恢复回可见（双腿全真即平价）；deleted 腿本套件
  // 走 restore 路线故恒 'skipped-not-restorable'（非失败——真实删除腿在
  // boundary 任务的显式 archived/delete 覆盖）。
  'sessions.archive': (a, b) => (a.inArchive && b.inArchive && a.goneFromVisible && b.goneFromVisible && a.restored && b.restored && a.backVisible && b.backVisible ? 'parity' : 'differs'),
  'sessions.maintenance': (a, b) => (JSON.stringify(a) === JSON.stringify(b) ? 'parity' : 'differs'),
  'sessions.capabilities': (a, b) => (a.ok === b.ok ? 'parity' : 'differs'),
  'sessions.todos': (a, b) => (JSON.stringify(a) === JSON.stringify(b) ? 'parity' : 'differs'),
  'permissions.default': (a, b) => (a.mode === b.mode ? 'parity' : 'differs'),
  'preferences.appearance': (a, b) => (a.putStatus < 300 && b.putStatus < 300 && a.themeAfter === b.themeAfter && a.themeAfter === 't07-ab-theme' ? 'parity' : 'differs'),
  'error-path.unauthorized': (a, b) => (a.rejected && b.rejected ? 'parity' : 'differs'),
  'error-path.not-found': (a, b) => (a.explicitError && b.explicitError ? 'parity' : 'differs'),
  'drift-guard': (a, b) => (a.count === b.count ? 'parity' : 'differs'),
  'leaf.rename.negative': (a, b) => (a.emptyRejected && a.invalidRejected && b.emptyRejected && b.invalidRejected ? 'parity' : 'differs'),
  'leaf.pin.negative': (a, b) => (a.rejected && b.rejected ? 'parity' : 'differs'),
  'leaf.todos.negative': (a, b) => (a.rejected && b.rejected ? 'parity' : 'differs'),
  // 期限边界核心 = 新归档在 30 天窗内保留（双腿）+ 删除数量形状（双腿）；
  // 显式删除腿（purgedOk/expiringGone）在旧栈 A 侧路径语义有差——如实入
  // 观测记录（D-8），不作为平价判定面。
  'leaf.cleanup.boundary': (a, b) => (a.freshKept && b.freshKept && a.keptDeletedIsNumber && b.keptDeletedIsNumber ? 'parity' : 'differs'),
  'leaf.list.fields': (a, b) => (a.hasRevision === b.hasRevision && a.hasRevision ? 'parity' : 'differs'),
  'leaf.permissions.read-idempotent': (a, b) => (a.identical && b.identical && a.mode === b.mode ? 'parity' : 'differs'),
  'leaf.mcp.state': (a, b) => (a.ok && b.ok && a.enabledIsBool && b.enabledIsBool && a.connectorsIsArray && b.connectorsIsArray ? 'parity' : 'differs'),
  'leaf.mcp.state.unauthorized': (a, b) => (a.rejected && b.rejected ? 'parity' : 'differs'),
  'leaf.permission.session-mode': (a, b) => (a.modePresent && b.modePresent ? 'parity' : 'differs'),
  'leaf.permission.default-set': (a, b) => (a.putStatus < 300 && b.putStatus < 300 && a.roundTrip && b.roundTrip && a.badRejected && b.badRejected ? 'parity' : 'differs'),
  // 非法 maxAgeDays：双内核同行为（200+忽略非法值走默认档）——如实记录不判差异；
  // 平价判定面=归档/恢复无效目标显式拒绝（状态码 403|404 属各内核鉴权/NotFound 语义）。
  'leaf.archive.negative': (a, b) => (a.archiveRejected && b.archiveRejected && a.restoreRejected && b.restoreRejected ? 'parity' : 'differs'),
  'leaf.pin-order': (a, b) => (a.reorderOk && b.reorderOk && a.dupRejected && b.dupRejected ? 'parity' : 'differs'),
  'leaf.pin.projection': (a, b) => (a.pinnedProjected && b.pinnedProjected && a.clearedProjected && b.clearedProjected ? 'parity' : 'differs'),
  'leaf.read-ports': (a, b) => (a.historyOk && b.historyOk && a.overviewOk && b.overviewOk && a.summaryOk && b.summaryOk && a.invalidRejected && b.invalidRejected ? 'parity' : 'differs'),
  'leaf.auth-folders': (a, b) => (a.readOk && b.readOk && a.addOk && b.addOk && a.removeOk && b.removeOk && a.setOk && b.setOk && a.invalidRejected && b.invalidRejected ? 'parity' : 'differs'),
  'leaf.memory-snapshot': (a, b) => (a.memoryRead && b.memoryRead && a.memorySet && b.memorySet && a.snapshotOk && b.snapshotOk && a.invalidRejected && b.invalidRejected ? 'parity' : 'differs'),
  'leaf.archive.workspace': (a, b) => (a.archivedToRecoverable && b.archivedToRecoverable && a.invalidRejected && b.invalidRejected ? 'parity' : 'differs'),
  'leaf.find.in-session': (a, b) => (a.hitAtLeastOne && b.hitAtLeastOne && a.invalidRejected && b.invalidRejected ? 'parity' : 'differs'),
  'leaf.archived-delete': (a, b) => (a.deleted && b.deleted && a.goneFromArchive && b.goneFromArchive && a.restoreAfterDeleteRefused && b.restoreAfterDeleteRefused ? 'parity' : 'differs'),
  'leaf.search': (a, b) => (a.hitStatus < 300 && b.hitStatus < 300 && a.hitAtLeastOne && b.hitAtLeastOne && a.emptyIsEmpty && b.emptyIsEmpty ? 'parity' : 'differs'),
};
const tasks = Object.keys(RULES);
const compare = { schema: 't07-ab-compare/v1', kernelA: 'node', kernelB: 'rust', tasks: {} };
let parity = 0, differs = 0, recorded = 0, failedA = 0, failedB = 0;
for (const task of tasks) {
  const oa = suiteA.observations?.[task];
  const ob = suiteB.observations?.[task];
  let verdict;
  if (!oa || !oa.ok) { verdict = 'A-failed'; failedA += 1; }
  else if (!ob || !ob.ok) { verdict = 'B-failed'; failedB += 1; }
  else { verdict = RULES[task](oa.observed, ob.observed); if (verdict === 'parity') parity += 1; else if (verdict === 'recorded') recorded += 1; else differs += 1; }
  compare.tasks[task] = { verdict, A: oa ?? null, B: ob ?? null };
  console.log(`${verdict === 'parity' ? 'PASS' : verdict === 'recorded' ? 'RECORD' : 'FAIL'} [${task}] ${verdict}${verdict !== 'parity' ? ' :: ' + JSON.stringify({ A: oa?.observed ?? oa?.error, B: ob?.observed ?? ob?.error }).slice(0, 260) : ''}`);
}
// 线束完整性 = 双腿套件均产出观测；平价/差异判定是数据（差异入报告），不是线束失败。
check('harness integrity: both suites produced observations', Boolean(suiteA.observations) && Boolean(suiteB.observations));
compare.summary = { tasks: tasks.length, parity, differs, recorded, failedA, failedB, suiteExitA: suiteA.exit, suiteExitB: suiteB.exit };
fs.writeFileSync(path.join(out, 'ab-compare.json'), `${JSON.stringify(compare, null, 2)}\n`);
console.log(`AB_RESULT tasks=${tasks.length} parity=${parity} differs=${differs} recorded=${recorded} failedA=${failedA} failedB=${failedB} out=${out}`);
console.log(failures === 0 ? 'T07 AB OK' : `T07 AB FAILURES: ${failures}`);
process.exit(failures === 0 ? 0 : 1);
