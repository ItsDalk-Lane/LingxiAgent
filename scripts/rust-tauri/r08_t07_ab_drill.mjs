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
// R08 收尾第六轮：多套件支持。--suite-files 逗号分隔；缺省仅主套件。
// 每个套件在双腿各跑一次，观测按任务键合并进 ab-<kind>.json（重复任务键=响亮失败，
// 不静默覆盖）。正式门由生成器传入「主套件 + 域扩展套件」，开发可只跑子集。
const MAIN_SUITE = 'desktop/src/react/__tests__/r08/r08-t07-kernel-ab.test.ts';
const suiteFilesArg = argValue('--suite-files');
const suiteFiles = (suiteFilesArg ? suiteFilesArg.split(',') : [MAIN_SUITE]).map((s) => s.trim()).filter(Boolean);
// 开发专用：LINGXI_R08_AB_TEST_FILTER 传 vitest -t（只跑匹配测试，便于单任务红绿
// 迭代）；正式门不设置该变量。设置时按子集判据（只比较实际观测任务）。
const testFilter = process.env.LINGXI_R08_AB_TEST_FILTER ?? null;
const subsetMode = Boolean(suiteFilesArg) || Boolean(testFilter);

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
// R08 收尾第六轮：B 腿工具面 workspace。此前 B 腿 --config 无 workspace，
// file/exec 四工具不注册（lib.rs 仅 Some(workspace) 时注册），模型声明面缺
// read/write/edit/exec_command——依赖真实进程产物的叶（延迟大内容、终端）在
// B 腿取不到真实机制。此处给两腿同一个隔离工作区（work 内，不涉及真实用户
// 目录），套件经 LINGXI_R08_AB_WORKSPACE 使用它。
const sharedWorkspace = path.join(work, 'workspace');
fs.mkdirSync(sharedWorkspace, { recursive: true });
fs.writeFileSync(path.join(sharedWorkspace, 'r08-workspace-marker.txt'), 'R08 A/B shared workspace\n');

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
  // 工具步固定延迟：给 steer/interject/stop 的「run 进行中」窗口（Rust 工具步无网络
  // 往返，6 步计划原本 <300ms 跑完，steer 在排空时机后到达则永远赶不上）。
  '--tool-delay-ms', '350',
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

// ── 0b. D03 OAuth 授权服务器替身：双内核对同一实例做发现/注册/授权 URL 生成 ──
const oauthScratch = path.join(work, 'oauth-standin');
fs.mkdirSync(oauthScratch, { recursive: true });
const oauthOut = fs.openSync(path.join(oauthScratch, 'stdout.log'), 'w');
const oauthStandin = spawn(process.execPath, [path.join(REPO, 'scripts/rust-tauri/r08_oauth_standin.mjs'), '--port', '0'], { cwd: REPO, stdio: ['ignore', oauthOut, oauthOut] });
fs.closeSync(oauthOut);
children.push(oauthStandin);
let oauthPort = null;
{
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline && !oauthPort) {
    try {
      const text = fs.readFileSync(path.join(oauthScratch, 'stdout.log'), 'utf8');
      const m = text.match(/R08_OAUTH_READY (\d+)/);
      if (m) oauthPort = Number(m[1]);
    } catch { /* not yet */ }
    if (!oauthPort) await sleep(200);
  }
}
check('oauth standin ready', Boolean(oauthPort), String(oauthPort));

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
  // 响亮拒——t12-ab-drill-d3-04 实证）。Pi 还要求谱系含 thinking_level_change
  // （legacy-v3 sessionModelMetadata：model/thinkingLevel 任一缺 → undefined →
  // "Could not restore model undefined/undefined"）——补 medium 档。注入的
  // t07-stub/chat 是本机环回替身。
  const modelChangeId = `mc-${headerId}`;
  lines.push(JSON.stringify({ type: 'model_change', id: modelChangeId, parentId: null, timestamp: '2026-10-01T09:00:00Z', provider: 't07-stub', modelId: 'chat' }));
  const thinkingId = `tl-${headerId}`;
  lines.push(JSON.stringify({ type: 'thinking_level_change', id: thinkingId, parentId: modelChangeId, timestamp: '2026-10-01T09:00:00Z', thinkingLevel: 'medium' }));
  // Pi 叶链：首条用户消息的 parent 必须接 thinking_level_change（真实会话由 SDK
  // 追加维护该链；断链时 buildSessionContext 不恢复模型 ref——t12-ab-drill-d3-05 实证）。
  let parent = thinkingId;
  let n = 0;
  for (const text of userTexts) {
    n += 1;
    lines.push(JSON.stringify({ type: 'message', id: `u${n}`, parentId: parent, timestamp: '2026-10-01T10:00:00Z', message: { role: 'user', content: text } }));
    // Pi 会话设置以最后一条助手消息的 provider/model 为最终模型来源
    // （session-manager getSessionContextSettings:156-158）——真实 Pi 会话的
    // 助手消息携带这两个字段；缺省时 restore 模型解析为 undefined/undefined。
    lines.push(JSON.stringify({ type: 'message', id: `a${n}`, parentId: `u${n}`, timestamp: '2026-10-01T10:00:01Z', message: { role: 'assistant', provider: 't07-stub', model: 'chat', content: [{ type: 'text', text: `T07-AB 回复 ${n}：已处理。` }] } }));
    parent = `a${n}`;
  }
  return `${lines.join('\n')}\n`;
}
for (const [name, texts] of [
  ['ab-real-alpha', ['T07 A/B 任务一：请阅读并回复。', 'T07 A/B 任务一之二：再确认。']],
  ['ab-real-beta', ['T07 A/B 任务二：请阅读并回复。']],
  ['ab-real-gamma', ['T07 A/B 任务三：请阅读并回复。']],
  // R08 收尾：破坏性任务（归档删除/期限边界）殿后仍需 ≥2 个未动顶层会话；
  // 三会话夹具在 archived-delete 之后只剩 1 个致 cleanup.boundary 跳过（run-04 实证）。
  ['ab-real-delta', ['T07 A/B 任务四：请阅读并回复。']],
  ['ab-real-epsilon', ['T07 A/B 任务五：请阅读并回复。']],
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
// D-9 夹具对齐（R08 收尾）：旧栈 cleanup 以归档文件 mtime 判过期（sessions.ts:2536），
// Rust 以迁移得到的 archived_at（夹具 2025 时间戳）判过期。夹具复制后 mtime=现在，
// 让 A 腿把 2025 的归档当成新归档——这是夹具放大的伪差异（真实旧根 mtime≈归档
// 时刻）。把 A 腿归档文件 mtime 对齐到其 sessionMeta.createdAt，两内核见同一事实。
{
  const archivedDir = path.join(rootA, 'agents/a1/sessions/archived');
  if (fs.existsSync(archivedDir)) {
    for (const name of fs.readdirSync(archivedDir)) {
      if (!name.endsWith('.jsonl')) continue;
      const file = path.join(archivedDir, name);
      const firstLine = fs.readFileSync(file, 'utf8').split('\n').find((l) => l.trim());
      let createdAt = null;
      try { createdAt = JSON.parse(firstLine ?? '{}').createdAt ?? null; } catch { /* keep null */ }
      if (typeof createdAt === 'number' && Number.isFinite(createdAt)) {
        const when = new Date(createdAt);
        fs.utimesSync(file, when, when);
      }
    }
  }
}
{
  const stamp = { schemaVersion: 2, epoch: 1, minimumReaderEpoch: 1, committedDataEpoch: 1, lastVersion: 'legacy-import', updatedAt: new Date().toISOString() };
  fs.writeFileSync(path.join(rootA, 'data-epoch.json'), `${JSON.stringify(stamp, null, 2)}\n`, { mode: 0o600 });
  fs.rmSync(path.join(rootA, 'data-epoch-transition.json'), { force: true });
  const nowIso = new Date().toISOString();
  fs.writeFileSync(path.join(rootA, 'security/execution-leases.json'), `${JSON.stringify({ schemaVersion: 1, leases: [], createdAt: nowIso, updatedAt: nowIso }, null, 2)}\n`);
  // A 腿助手主对话模型持久选定（config-coordinator setDefaultModel 的持久口径：
  // config.yaml models.chat = {id, provider}；/api/models/set 只设一次性 pending
  // 模型，被首个 create 消费后后续 sessions/new 仍 409——t07 ab run-03/04 实证）。
  fs.writeFileSync(path.join(rootA, 'agents/a1/config.yaml'), 'agent:\n  name: "A1"\n  yuan: "lingxi"\nmodels:\n  chat:\n    id: "chat"\n    provider: "t07-stub"\n', { mode: 0o600 });
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
// B 腿模型面见下方 --config providers 注入（config-file 入口，d3 同款）；
// 不再向迁移根写 models.json（t12-ab-drill-d3-02 实证该路径致 switch 500）。
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
// R08 收尾：B 腿模型面经服务 --config providers 注入同一桩供应商（t06 d3 演习
// 同款、已证 sessions/new 可用）；此前 models.json 注入曾致 switch 500（t12-ab-
// drill-d3-02），config-file providers 是受支持的模型面入口。双腿同一桩 =
// A13「模型替身」固定要素成立；会话创建/切换/分支等需模型面的叶因此可对拍。
const configB = path.join(work, 'service-config-b.json');
fs.writeFileSync(configB, `${JSON.stringify({
  home: rootB,
  workspace: sharedWorkspace,
  providers: {
    't07-stub': {
      protocol: 'openai-completions',
      endpoint: `http://127.0.0.1:${stubPort}/v1`,
      auth: { kind: 'apiKey', apiKey: 'R08_SYNTHETIC_LOCAL_STUB_ONLY' },
      models: [{ id: 'chat', capabilities: { tools: true, imageInput: false } }],
    },
  },
  models: { chat: { provider: 't07-stub', model: 'chat', capabilities: { tools: true, imageInput: false } } },
}, null, 2)}\n`, { mode: 0o600 });
// --http-rate-max：HTTP 准入覆盖（config.rs 文档化测试/运维旋钮）。本套件是单对端
// 密集合成客户端（41 任务数百请求/10s），默认 240/10s 的生产预算会把无关读面打成
// 429（run-05 实证 workspace-rollback/checkpoints 读 429）；旧栈无同款限流。不改产品默认。
const serviceB = spawn(bin, ['--config', configB, '--bind', '127.0.0.1:0', '--http-rate-max', '100000'], { cwd: REPO, env: envB, stdio: ['ignore', logB, logB] });
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

// ── 4. 双跑套件（支持多套件：每套件双腿各跑一次，观测按任务键合并） ──
function readObservations(outFile) {
  if (!fs.existsSync(outFile)) return null;
  try { return JSON.parse(fs.readFileSync(outFile, 'utf8')).observations ?? null; } catch { return null; }
}
function runSuite(kind, base, token, suiteFile, index) {
  const outFile = path.join(out, `ab-${kind}-${index}.json`);
  const logFile = path.join(out, `vitest-${kind}-${index}.log`);
  const env = {
    ...process.env, LINGXI_R08_AB_BASE: base, LINGXI_R08_AB_TOKEN: token,
    LINGXI_R08_AB_KIND: kind, LINGXI_R08_AB_OUT: outFile,
    LINGXI_R08_AB_OAUTH_BASE: oauthPort ? `http://127.0.0.1:${oauthPort}` : '',
    LINGXI_R08_AB_WORKSPACE: sharedWorkspace,
  };
  for (const k of SCRUB_ENV) delete env[k];
  const r = spawnSync('npx', ['vitest', 'run', suiteFile, '--reporter=verbose', ...(testFilter ? ['-t', testFilter] : [])], { cwd: REPO, env, encoding: 'utf8', timeout: 600000 });
  fs.writeFileSync(logFile, `${r.stdout || ''}\n${r.stderr || ''}`);
  return { suiteFile, exit: r.status, observations: readObservations(outFile), logFile, outFile };
}
function runLeg(kind, base, token) {
  const runs = suiteFiles.map((suiteFile, index) => runSuite(kind, base, token, suiteFile, index));
  // 合并：同一任务键不得跨套件重复（重复=线束歧义，响亮失败，不静默覆盖）。
  const merged = {};
  const duplicated = [];
  for (const run of runs) {
    for (const [task, observation] of Object.entries(run.observations ?? {})) {
      if (Object.prototype.hasOwnProperty.call(merged, task)) duplicated.push(`${run.suiteFile}:${task}`);
      else merged[task] = observation;
    }
  }
  if (duplicated.length) {
    check(`suite task keys unique (${kind})`, false, duplicated.slice(0, 5).join(','));
  }
  const mergedFile = path.join(out, `ab-${kind}.json`);
  fs.writeFileSync(mergedFile, `${JSON.stringify({ kind, observations: merged }, null, 2)}\n`);
  return { runs, exit: runs.every((r) => r.exit === 0) ? 0 : 1, observations: Object.keys(merged).length ? merged : null, logFiles: runs.map((r) => r.logFile) };
}
const suiteA = serverInfoA ? runLeg('node', `http://127.0.0.1:${portA}`, serverInfoA.token) : { exit: null, observations: null };
const suiteB = addrB ? runLeg('rust', `http://${addrB}`, tokenB) : { exit: null, observations: null };
check('suite ran on A (observations present)', Boolean(suiteA.observations), `vitest exit=${suiteA.exit}`);
check('suite ran on B (observations present)', Boolean(suiteB.observations), `vitest exit=${suiteB.exit}`);

// ── 5. 关停两内核（A：SIGTERM owned-pid；B：SIGTERM）与桩供应商 ──
if (serverInfoA) { try { process.kill(serverInfoA.pid, 'SIGTERM'); } catch { /* gone */ } }
  serviceB.kill('SIGTERM');
  stub.kill('SIGTERM');
  oauthStandin.kill('SIGTERM');
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
// R08 收尾泛化：观测带 `cases:{name:bool}` 的任务按「全部案例双腿为真」判平价；
// 生成器对同一 cases 键自动派生 leaf::<task>.<name> 图钉（见 r08_stage_inputs.py
// derive_ab_cases 的 cases 自动派生），新叶只需在套件里产出 cases 并在
// FULL_BINDINGS 登记，不再逐叶手写比较规则与派生行。
const casesParity = (a, b) => {
  const ca = a?.cases, cb = b?.cases;
  if (!ca || !cb || typeof ca !== 'object' || typeof cb !== 'object') return 'differs';
  const keys = new Set([...Object.keys(ca), ...Object.keys(cb)]);
  if (keys.size === 0) return 'differs';
  for (const k of keys) if (ca[k] !== true || cb[k] !== true) return 'differs';
  return 'parity';
};
const RULES = {
  'identity': (a, b) => (a.status === 200 && b.status === 200 ? 'parity' : 'differs'),
  // 计数一致即列表平价；标题投影差异（旧=null vs 新=stem）是已归因 UI 差异
  // （差异报告 D-1），不判 differs。
  'sessions.list': (a, b) => (a.count === b.count && a.pinned === b.pinned ? 'parity' : 'differs'),
  // R08 收尾：双腿模型面齐备（A 腿 config.yaml 持久默认模型 + B 腿 --config
  // providers）后，switch/create 不再是环境前置差，按真实语义对拍。
  'sessions.switch': (a, b) => (a.focused === b.focused && a.focused === true ? 'parity' : 'differs'),
  'sessions.rename': (a, b) => (a.ok && b.ok && a.persisted && b.persisted ? 'parity' : 'differs'),
  'sessions.pin': (a, b) => (JSON.stringify(a) === JSON.stringify(b) && a.pinA && a.pinB && a.unpinA && a.unpinB ? 'parity' : 'differs'),
  // 真实 UI 流（createNewSession → ensureSession）双腿都必须建成并 +1。
  'sessions.create': (a, b) => (a.created && b.created && a.delta === 1 && b.delta === 1 ? 'parity' : 'differs'),
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
  // 记忆开关读/写/负路径（提示词快照拆到 leaf.prompt-snapshot：A 腿合成会话无
  // 冻结 sidecar 需真实模型回合——归属聊天线束，不拖累记忆两叶的判定）。
  'leaf.memory-snapshot': (a, b) => (a.memoryRead && b.memoryRead && a.memorySet && b.memorySet && a.invalidRejected && b.invalidRejected ? 'parity' : 'differs'),
  // 提示词快照：A 腿旧栈对合成/迁移会话读面 500（夹具前提缺口，非内核语义差；
  // Rust 200 + 双腿无效会话均显式拒）——RECORDED_APPROVALS 登记
  // FIXTURE-PROMPT-SNAPSHOT，叶 FF4F06011B48 保持 pending。
  'leaf.prompt-snapshot': (a, b) => {
    const aGap = a?.cases && a.cases['read'] === false && a.snapStatus === 500
      && a.cases['invalid-rejected'] === true;
    const bFull = b?.cases && b.cases['read'] === true && b.cases['invalid-rejected'] === true;
    if (aGap && bFull) return 'recorded';
    return casesParity(a, b);
  },
  'leaf.archive.workspace': (a, b) => (a.archivedToRecoverable && b.archivedToRecoverable && a.invalidRejected && b.invalidRejected ? 'parity' : 'differs'),
  'leaf.find.in-session': (a, b) => (a.hitAtLeastOne && b.hitAtLeastOne && a.invalidRejected && b.invalidRejected ? 'parity' : 'differs'),
  'leaf.archived-delete': (a, b) => (a.deleted && b.deleted && a.goneFromArchive && b.goneFromArchive && a.restoreAfterDeleteRefused && b.restoreAfterDeleteRefused ? 'parity' : 'differs'),
  'leaf.search': (a, b) => (a.hitStatus < 300 && b.hitStatus < 300 && a.hitAtLeastOne && b.hitAtLeastOne && a.emptyIsEmpty && b.emptyIsEmpty ? 'parity' : 'differs'),
  // R08 收尾 D02 批次 1（cases 泛化任务；叶图钉由生成器自动派生）。
  'leaf.projects': (a, b) => casesParity(a, b),
  'leaf.conversation-map': (a, b) => casesParity(a, b),
  'leaf.checkpoints': (a, b) => casesParity(a, b),
  'leaf.workspace-rollback': (a, b) => casesParity(a, b),
  'leaf.sessions.create': (a, b) => casesParity(a, b),
  'leaf.permission-mode': (a, b) => casesParity(a, b),
  'leaf.mcp.crud': (a, b) => casesParity(a, b),
  // 会话管理族：continue-deleted-agent 在 Rust 腿与「官方 DELETE 助手」流程
  // 交互后源会话行不可见（直接墓碑路径已由 r08-continue-disposal-compat 证明
  // 路由本身工作）——D-16 真实产品发现，登记 recorded；其余案例全真才计合格。
  'leaf.sessions.admin': (a, b) => {
    const strip = (obs) => {
      const c = { ...(obs?.cases ?? {}) };
      delete c['continue-deleted-agent.new-identity-and-path'];
      return c;
    };
    const aRest = strip(a), bRest = strip(b);
    const keys = new Set([...Object.keys(aRest), ...Object.keys(bRest)]);
    for (const k of keys) if (aRest[k] !== true || bRest[k] !== true) return 'differs';
    const contGap = b?.cases?.['continue-deleted-agent.new-identity-and-path'] !== true
      && a?.cases?.['continue-deleted-agent.new-identity-and-path'] === true;
    return contGap ? 'recorded' : 'parity';
  },
  // D13 终端帧族传输面：Rust 无终端子系统（显式能力错误=T01_SCOPING 既定裁决），
  // A 腿 snapshot/close 真实形状、tail 无终端时旧栈无回复——判定面 = 双腿传输
  // 不断连 + Rust 显式拒绝（不假装成功）。recorded 登记（正路径 R09+ 桌面终端域）。
  'leaf.ws.terminal': (a, b) => {
    const bTransport = b?.cases && b.cases['snapshot.explicit-reply'] === true
      && b.cases['close.result-rejected-explicit'] === true
      && b.cases['transport.survives-frame-family'] === true;
    const aTransport = a?.cases && a.cases['transport.survives-frame-family'] === true;
    if (bTransport && aTransport) return 'recorded';
    return casesParity(a, b);
  },
  'leaf.plan-mode': (a, b) => casesParity(a, b),
  'leaf.prefs.quick-chat': (a, b) => casesParity(a, b),
  'leaf.drafts': (a, b) => casesParity(a, b),
  'leaf.ws.compact': (a, b) => casesParity(a, b),
  'leaf.chat.transport': (a, b) => casesParity(a, b),
  'leaf.chat.round': (a, b) => casesParity(a, b),
  // /reset：旧栈对 desktop 会话清史不生效（run-21/22/23 实证：清前后历史规模
  // 不变、无「已重置」回执帧）——与 /new 同族 phase-1 限制；Rust 侧回执/压缩
  // 全绿（清史规模量纲受历史端点形状影响，以 run-20 historyCleared:true 为
  // 补充证据）。该形态 recorded（D-14），其余差异仍 differs。
  'leaf.slash': (a, b) => {
    const aNoClear = a?.beforeCount > 0 && a?.afterCount === a?.beforeCount;
    const bFull = b?.cases && b.cases['slash.reset.explicit-reply'] === true
      && b.cases['slash.compact.replies'] === true;
    if (aNoClear && bFull) return 'recorded';
    return casesParity(a, b);
  },
  // /new：旧栈对 desktop 会话回 "[命令错误] rotate for desktop kind not supported
  // in phase 1"（run-19/20 原文）——旧栈自身 phase-1 限制，Rust 已实现轮换。
  // 该形态记 recorded（RECORDED_APPROVALS 登记），其余差异仍 differs。
  'leaf.slash.new': (a, b) => {
    const aOld = a?.oldStackLimit === true;
    const bFull = b?.cases && b.cases['slash.new.rotates-and-archives'] === true
      && b.cases['slash.new.old-recoverable'] === true;
    if (aOld && bFull) return 'recorded';
    return casesParity(a, b);
  },
  'leaf.chat.abort': (a, b) => casesParity(a, b),
  // R08 收尾批次 5：工具回合线束（替身 R08-TOOL-PLAN，双内核同一按需目录协议
  // mcp_describe_tool → mcp_call）。全部 cases 泛化任务。
  'leaf.tool.todos': (a, b) => casesParity(a, b),
  'leaf.tool.checkpoint': (a, b) => casesParity(a, b),
  'leaf.tool.rewind': (a, b) => casesParity(a, b),
  'leaf.tool.session': (a, b) => casesParity(a, b),
  'leaf.tool.current-status': (a, b) => casesParity(a, b),
  'leaf.tool.session-folders': (a, b) => casesParity(a, b),
  // R08 收尾批次 6：WS 入站帧族（prompt/steer/interject/resume_stream/slash + compact/fresh-compact/stop）。
  'leaf.ws.stream': (a, b) => casesParity(a, b),
  // R08 收尾批次 7：D02 HTTP 语义（cleanup 期限 / 延迟内容 / 重放最近用户消息 / switch 焦点 / 真回合提示词快照）。
  'leaf.cleanup.expiry': (a, b) => casesParity(a, b),
  'leaf.lazy-content': (a, b) => casesParity(a, b),
  'leaf.replay-latest': (a, b) => casesParity(a, b),
  'leaf.switch.focus': (a, b) => casesParity(a, b),
  'leaf.prompt-snapshot.real': (a, b) => casesParity(a, b),
  // R08 收尾批次 8：D03 MCP 资源/应用/OAuth（stdio 替身扩展 + 环回授权服务器替身）。
  'leaf.mcp.face': (a, b) => casesParity(a, b),
  'leaf.mcp.oauth': (a, b) => casesParity(a, b),
};
// 任务集合 = 已登记规则 ∪ 双腿实际观测到的任务键（R08 收尾第六轮：域扩展
// 套件的新任务无需先改本脚本；未登记规则的任务沿用通用 casesParity 判据）。
// 缺省（未传 --suite-files）时集合并集包含全部规则键——规则任务缺观测即失败；
// 子集模式（开发只跑某域套件）只比较实际观测到的任务，避免无关规则被误报。
const observedKeys = new Set([...Object.keys(suiteA.observations ?? {}), ...Object.keys(suiteB.observations ?? {})]);
const tasks = subsetMode
  ? [...observedKeys].sort()
  : [...new Set([...Object.keys(RULES), ...observedKeys])].sort();
const compare = { schema: 't07-ab-compare/v1', kernelA: 'node', kernelB: 'rust', suiteFiles, tasks: {} };
let parity = 0, differs = 0, recorded = 0, failedA = 0, failedB = 0;
for (const task of tasks) {
  const oa = suiteA.observations?.[task];
  const ob = suiteB.observations?.[task];
  let verdict;
  if (!oa || !oa.ok) { verdict = 'A-failed'; failedA += 1; }
  else if (!ob || !ob.ok) { verdict = 'B-failed'; failedB += 1; }
  else {
    const rule = RULES[task] ?? casesParity;
    verdict = rule(oa.observed, ob.observed);
    if (verdict === 'parity') parity += 1; else if (verdict === 'recorded') recorded += 1; else differs += 1;
  }
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
