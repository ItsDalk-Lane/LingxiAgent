#!/usr/bin/env node
// R08-T04 A07/A08 夹具生成器（Lead 绑定条件②：同种子逐字节可复现）。
// 全部内容来自 mulberry32(种子) + 递增计数器；禁止 Date.now / Math.random /
// 环境相关字节。用法：
//   node scripts/rust-tauri/r08_t04_fixture_gen.mjs --seed 20261023 --out <dir>
// 自证（证据走 r08_run.py）：连跑两次到两个目录，diff -r 必须为空。
// 锚点演进（旧锚点证据不回溯改写，各自锚定各自时代）：
//   v2 = 20261023 种子基座（含 ASCII 占位 knowledge.db + mystery-unmapped.bin），
//        rootHash 前缀 40f42dc1…88c9e，封存 T04 B4/A08 + T05 P3 对拍证据链；
//   v3 = 2026-10-23 Lead 裁决：knowledge.db 改真实 SQLite（store-registry 声明
//        format="sqlite"）、mystery-unmapped.bin 移出基座（负路径变体由驱动脚本
//        派生）。v3 起 T05 A09/A10 及后续全部使用。
import { createHash } from 'node:crypto';
import { mkdirSync, writeFileSync, rmSync, existsSync, readdirSync, statSync, readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { DatabaseSync } from 'node:sqlite';

function mulberry32(seed) {
  let a = seed >>> 0;
  return function () {
    a |= 0; a = (a + 0x6D2B79F5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const args = process.argv.slice(2);
function argValue(flag) {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : null;
}
const seed = Number(argValue('--seed') ?? '20261023');
const out = argValue('--out');
if (!out) {
  console.error('usage: r08_t04_fixture_gen.mjs --seed <n> --out <dir>');
  process.exit(2);
}

const rand = mulberry32(seed);
let counter = 0;
const BASE_MS = 1_760_000_000_000; // 固定纪元，禁 Date.now。
function nextId(prefix) {
  counter += 1;
  return `${prefix}-${String(counter).padStart(6, '0')}-${Math.floor(rand() * 0xffffffff).toString(16).padStart(8, '0')}`;
}
function nextTs() {
  counter += 1;
  return BASE_MS + counter * 1000;
}
function wordStream(words) {
  const vocab = ['lumen', 'tide', 'quartz', 'anchor', 'signal', 'harbor', 'cedar', 'orbit', 'flint', 'ember'];
  const outWords = [];
  for (let i = 0; i < words; i += 1) outWords.push(vocab[Math.floor(rand() * vocab.length)]);
  return outWords.join(' ');
}
function write(rel, content) {
  const abs = join(out, rel);
  mkdirSync(dirname(abs), { recursive: true });
  writeFileSync(abs, content);
}
function writeJson(rel, value) {
  write(rel, `${JSON.stringify(value, null, 2)}\n`);
}
function sha256(buf) {
  return createHash('sha256').update(buf).digest('hex');
}

if (existsSync(out)) rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });

// ---- 1. fork 会话对（A07 关系还原）----
const parentSession = 'sess-parent-0001';
const childSession = 'sess-child-0002';
function jsonl(lines) {
  return `${lines.map((l) => JSON.stringify(l)).join('\n')}\n`;
}
{
  const parentLines = [
    { entryType: 'sessionMeta', id: nextId('meta'), sessionId: parentSession, agentId: 'a1', createdAt: nextTs(), title: 'parent' },
    { entryType: 'runBoundary', id: nextId('rb'), runId: 'run-parent-1', sessionId: parentSession, ts: nextTs(), kind: 'start' },
    { entryType: 'user', id: nextId('u'), sessionId: parentSession, runId: 'run-parent-1', ts: nextTs(), content: wordStream(12) },
    { entryType: 'assistant', id: nextId('as'), sessionId: parentSession, runId: 'run-parent-1', ts: nextTs(), content: wordStream(24) },
    { entryType: 'fork', id: nextId('fork'), sessionId: parentSession, ts: nextTs(), childSessionId: childSession, reason: 'user-branch' },
    { entryType: 'runBoundary', id: nextId('rb'), runId: 'run-parent-1', sessionId: parentSession, ts: nextTs(), kind: 'end' },
  ];
  write(`agents/a1/sessions/${parentSession}.jsonl`, jsonl(parentLines));
  const childLines = [
    { entryType: 'sessionMeta', id: nextId('meta'), sessionId: childSession, agentId: 'a1', createdAt: nextTs(), parentSessionId: parentSession, forkOf: parentSession },
    { entryType: 'runBoundary', id: nextId('rb'), runId: 'run-child-1', sessionId: childSession, ts: nextTs(), kind: 'start' },
    { entryType: 'user', id: nextId('u'), sessionId: childSession, runId: 'run-child-1', ts: nextTs(), content: wordStream(9) },
    { entryType: 'assistant', id: nextId('as'), sessionId: childSession, runId: 'run-child-1', ts: nextTs(), content: wordStream(18), customVendorField: { preserved: true, n: 42 } },
    { entryType: 'runBoundary', id: nextId('rb'), runId: 'run-child-1', sessionId: childSession, ts: nextTs(), kind: 'end' },
  ];
  write(`agents/a1/sessions/${childSession}.jsonl`, jsonl(childLines));
}

// ---- 2. legacy_unknown 会话（无 Run 边界 → run_id NULL + missing-run-boundary）----
{
  const sessionId = 'sess-unknown-0003';
  const lines = [
    { entryType: 'sessionMeta', id: nextId('meta'), sessionId, agentId: 'a1', createdAt: nextTs() },
    { entryType: 'user', id: nextId('u'), sessionId, ts: nextTs(), content: wordStream(7) },
    { entryType: 'assistant', id: nextId('as'), sessionId, ts: nextTs(), content: wordStream(15) },
  ];
  write(`agents/a1/sessions/${sessionId}.jsonl`, jsonl(lines));
}

// ---- 3. 未知块会话（未知 entryType → 隔离归档 + 标记消息；未知字段透传）----
{
  const sessionId = 'sess-blocks-0004';
  const lines = [
    { entryType: 'sessionMeta', id: nextId('meta'), sessionId, agentId: 'a1', createdAt: nextTs() },
    { entryType: 'runBoundary', id: nextId('rb'), runId: 'run-blocks-1', sessionId, ts: nextTs(), kind: 'start' },
    { entryType: 'user', id: nextId('u'), sessionId, runId: 'run-blocks-1', ts: nextTs(), content: wordStream(6) },
    { entryType: 'quantumEntangler', id: nextId('qb'), sessionId, runId: 'run-blocks-1', ts: nextTs(), payload: wordStream(10) },
    { entryType: 'assistant', id: nextId('as'), sessionId, runId: 'run-blocks-1', ts: nextTs(), content: wordStream(11), opaqueSignature: 'sig-blob-unparsed' },
    { entryType: 'holoProjection', id: nextId('hp'), sessionId, runId: 'run-blocks-1', ts: nextTs(), matrix: [1, 2, 3] },
    { entryType: 'runBoundary', id: nextId('rb'), runId: 'run-blocks-1', sessionId, ts: nextTs(), kind: 'end' },
  ];
  write(`agents/a1/sessions/${sessionId}.jsonl`, jsonl(lines));
}

// ---- 4. 损坏尾行会话（811 好行 + 截断尾；前缀导入 + 余量隔离）----
{
  const sessionId = 'sess-corrupt-0005';
  const lines = [
    { entryType: 'sessionMeta', id: nextId('meta'), sessionId, agentId: 'a1', createdAt: nextTs() },
    { entryType: 'runBoundary', id: nextId('rb'), runId: 'run-corrupt-1', sessionId, ts: nextTs(), kind: 'start' },
  ];
  for (let i = 0; i < 809; i += 1) {
    const type = i % 2 === 0 ? 'user' : 'assistant';
    lines.push({ entryType: type, id: nextId('e'), sessionId, runId: 'run-corrupt-1', ts: nextTs(), seq: i, content: wordStream(4) });
  }
  const good = jsonl(lines);
  const tailLine = JSON.stringify({ entryType: 'assistant', id: nextId('as'), sessionId, runId: 'run-corrupt-1', ts: nextTs(), content: wordStream(30) });
  const truncatedTail = tailLine.slice(0, Math.floor(tailLine.length / 2)); // 半行：无换行、非法 JSON
  write(`agents/a1/sessions/${sessionId}.jsonl`, good + truncatedTail);
}

// ---- 5. sidecar 三件套（meta/titles/files v1）----
writeJson('agents/a1/sessions/session-meta.json', {
  version: 1,
  sessions: {
    [parentSession]: { title: '父会话', archived: false },
    [childSession]: { title: '子会话（fork）', archived: false },
    'sess-unknown-0003': { title: '无边界会话', archived: false },
    'sess-blocks-0004': { title: '未知块会话', archived: false },
    'sess-corrupt-0005': { title: '损坏尾行会话', archived: false },
    'sess-archived-0006': { title: '归档会话', archived: true },
  },
});
writeJson('agents/a1/sessions/session-titles.json', {
  version: 1,
  titles: { [parentSession]: '父会话', [childSession]: '子会话（fork）', 'sess-archived-0006': '归档会话' },
});

// ---- 6. 附件载荷 + 描述符（session-files 类：载荷与 .json 描述符同目录；
// broken 项描述符声明全长而载荷截断 → B2 bad-attachment 检测面）----
{
  const good1 = Buffer.from(wordStream(40));
  const good2 = Buffer.from(JSON.stringify({ series: [3, 1, 4, 1, 5] }));
  const full = Buffer.from(wordStream(120));
  const truncated = full.subarray(0, Math.floor(full.length / 3));
  write('session-files/sfh-ab/notes.md', good1);
  write('session-files/sfh-cd/chart.json', good2);
  write('session-files/sfh-ef/big.bin', truncated);
  function descriptor(dir, fileId, sessionId, filename, payload, mime, declaredSize) {
    writeJson(`session-files/${dir}/${filename}.meta.json`, {
      version: 1,
      fileId,
      sessionId,
      filename,
      mime,
      size: declaredSize ?? payload.length,
      sha256: sha256(payload),
      status: 'available',
    });
  }
  descriptor('sfh-ab', 'file-good-1', parentSession, 'notes.md', good1, 'text/markdown');
  descriptor('sfh-cd', 'file-good-2', childSession, 'chart.json', good2, 'application/json');
  descriptor('sfh-ef', 'file-broken-1', parentSession, 'big.bin', truncated, 'application/octet-stream', full.length);
}

// ---- 7. 归档会话 ----
{
  const sessionId = 'sess-archived-0006';
  const lines = [
    { entryType: 'sessionMeta', id: nextId('meta'), sessionId, agentId: 'a1', createdAt: nextTs() },
    { entryType: 'runBoundary', id: nextId('rb'), runId: 'run-arch-1', sessionId, ts: nextTs(), kind: 'start' },
    { entryType: 'user', id: nextId('u'), sessionId, runId: 'run-arch-1', ts: nextTs(), content: wordStream(5) },
    { entryType: 'runBoundary', id: nextId('rb'), runId: 'run-arch-1', sessionId, ts: nextTs(), kind: 'end' },
  ];
  write(`agents/a1/sessions/archived/${sessionId}.jsonl`, jsonl(lines));
}

// ---- 8. 观测库 + 会话清单库（node:sqlite 确定性内容）----
// v2 夹具演进（台账「夹具版本演进」）：占位 calls 表替换为真实 v2+ 形状
// model_calls + model_call_usage（B3 切片B 读链可读），并新增 cron 源。
{
  mkdirSync(join(out, 'model-observability'), { recursive: true });
  const obs = new DatabaseSync(join(out, 'model-observability', 'observability.sqlite'));
  obs.exec(`CREATE TABLE model_calls (
      call_id TEXT PRIMARY KEY, trace_id TEXT, parent_call_id TEXT,
      provider TEXT, model_id TEXT, api TEXT, subsystem TEXT,
      call_purpose TEXT, session_id TEXT, started_at TEXT, ended_at TEXT,
      terminal_status TEXT);
    CREATE TABLE model_call_usage (
      model_call_id TEXT PRIMARY KEY, input_total_tokens INTEGER,
      output_total_tokens INTEGER, reasoning_tokens INTEGER,
      cache_read_tokens INTEGER, cache_write_tokens INTEGER);`);
  const iso = (n) => new Date(BASE_MS + n * 60_000).toISOString();
  // 确定性五行：trace 父子链 / 未知会话引用 / 无 usage 行 / 全 NULL usage 行 /
  // 混合 ok/error/NULL terminal_status（覆盖读链每个分支）。
  const calls = [
    ['call-legacy-p1', 'trace-legacy-1', null, 'legacy-provider-1', 'legacy-model-1', 'messages', 'chat', 'session-reply', 'sess-parent-0001', iso(1), iso(2), 'ok'],
    ['call-legacy-c1', 'trace-legacy-1', 'call-legacy-p1', 'legacy-provider-1', 'legacy-model-1', 'messages', 'tool', 'tool-call', 'sess-child-0002', iso(3), iso(4), 'error'],
    ['call-legacy-u1', null, null, 'legacy-provider-2', 'legacy-model-2', 'responses', 'cron', 'tick', null, iso(5), null, null],
    ['call-legacy-p2', 'trace-legacy-2', null, 'legacy-provider-1', 'legacy-model-1', 'messages', 'chat', 'session-reply', 'sess-unknown-0003', iso(6), iso(7), 'ok'],
    ['call-legacy-n1', 'trace-legacy-3', null, 'legacy-provider-2', 'legacy-model-2', 'responses', 'chat', 'session-reply', 'sess-parent-0001', iso(8), iso(9), 'aborted'],
  ];
  const insCall = obs.prepare(`INSERT INTO model_calls
    (call_id, trace_id, parent_call_id, provider, model_id, api, subsystem, call_purpose, session_id, started_at, ended_at, terminal_status)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`);
  for (const row of calls) insCall.run(...row);
  const insUsage = obs.prepare(`INSERT INTO model_call_usage
    (model_call_id, input_total_tokens, output_total_tokens, reasoning_tokens, cache_read_tokens, cache_write_tokens)
    VALUES (?, ?, ?, ?, ?, ?)`);
  // p1 全字段（含 cache/reasoning）；p2 部分 NULL；n1 全 NULL（走 usage=None 分支）；
  // c1 无 usage 行（LEFT JOIN NULL）。
  insUsage.run('call-legacy-p1', 100, 50, 3, 7, 11);
  insUsage.run('call-legacy-p2', 200, 80, null, null, null);
  insUsage.run('call-legacy-n1', null, null, null, null, null);
  obs.close();
  const manifest = new DatabaseSync(join(out, 'session-manifest.db'));
  manifest.exec('CREATE TABLE sessions (session_id TEXT PRIMARY KEY, agent_id TEXT, updated_at INTEGER)');
  for (const sid of [parentSession, childSession, 'sess-unknown-0003', 'sess-blocks-0004', 'sess-corrupt-0005', 'sess-archived-0006']) {
    manifest.prepare('INSERT INTO sessions (session_id, agent_id, updated_at) VALUES (?, ?, ?)').run(sid, 'a1', nextTs());
  }
  manifest.close();
}

// ---- 8b. cron 源（agents/a1/desk/cron-jobs.json；actor a1 在夹具内存在）----
{
  const isoCron = (n) => new Date(BASE_MS + n * 3_600_000).toISOString();
  writeJson('agents/a1/desk/cron-jobs.json', {
    jobs: [
      {
        id: 'legacy-job-1', schemaVersion: 3, actorAgentId: 'a1',
        type: 'every', schedule: 3_600_000, enabled: true,
        prompt: `legacy hourly reminder: ${wordStream(4)}`,
        executor: { kind: 'agent_session' },
        nextRunAt: isoCron(10), createdAt: isoCron(1), lastRunAt: isoCron(2),
      },
      {
        id: 'legacy-job-2', schemaVersion: 4, actorAgentId: 'a1',
        type: 'every', schedule: 7_200_000, enabled: true,
        executor: { kind: 'direct_action', action: 'notify', params: { title: 'legacy-notify', body: wordStream(6) } },
        nextRunAt: isoCron(11), createdAt: isoCron(3), lastRunAt: isoCron(4),
      },
      {
        id: 'legacy-job-3', schemaVersion: 4, actorAgentId: 'a1',
        type: 'every', schedule: 3_600_000, enabled: false,
        executor: { kind: 'plugin_action', pluginId: 'legacy-plug', actionId: 'ping', params: { n: 1 } },
        createdAt: isoCron(5), lastRunAt: isoCron(6),
      },
    ],
  });
}

// ---- 9. knowledge 域（register + regenerable 对照）----
// v3（Lead 裁决 2026-10-23）：knowledge.db 必须是真实 SQLite——store-registry.ts:989-997
// 声明 knowledge-database format="sqlite"，T05 preflight data-consistency 对副本做
// quick_check，假库会被如实 refuse（红证据 t05-migrate-e2e-01）。确定性：定序建表+定序
// 插入，无时间戳/随机字节，同 SQLite 版本下逐字节可复现（双跑自证覆盖）。
{
  mkdirSync(join(out, 'knowledge'), { recursive: true });
  const kb = new DatabaseSync(join(out, 'knowledge', 'knowledge.db'));
  kb.exec(`CREATE TABLE documents (
      doc_id TEXT PRIMARY KEY, title TEXT NOT NULL, source_path TEXT,
      created_at INTEGER NOT NULL);
    CREATE TABLE processing_artifacts (
      artifact_id TEXT PRIMARY KEY, doc_id TEXT NOT NULL,
      parser TEXT NOT NULL, locator_map TEXT NOT NULL);`);
  const insDoc = kb.prepare('INSERT INTO documents (doc_id, title, source_path, created_at) VALUES (?, ?, ?, ?)');
  const insArt = kb.prepare('INSERT INTO processing_artifacts (artifact_id, doc_id, parser, locator_map) VALUES (?, ?, ?, ?)');
  for (let i = 1; i <= 8; i += 1) {
    const docId = `doc-${String(i).padStart(4, '0')}`;
    insDoc.run(docId, `knowledge-doc-${i}-${wordStream(3)}`, `knowledge/sources/src-${i}.md`, BASE_MS + i * 1000);
    insArt.run(`art-${String(i).padStart(4, '0')}`, docId, 'markdown-v1', JSON.stringify({ extract: 'knowledge/artifacts/doc1/extract.json', ordinal: i }));
  }
  kb.close();
}
write('knowledge/indexes/fts/shard0.bin', Buffer.from(`regenerable-index-${seed}`));
writeJson('knowledge/artifacts/doc1/extract.json', { version: 1, text: wordStream(20) });

// ---- 10. 转换类运行态小文件（切片 A 之外的 convert 类）----
writeJson('conversation-map.json', { version: 1, layout: { [parentSession]: { x: 0, y: 0 }, [childSession]: { x: 1, y: 0 } } });
writeJson('.ephemeral/deferred-tasks.json', { version: 1, tasks: [{ id: nextId('dt'), state: 'pending' }] });
writeJson('.ephemeral/loop-state.json', { version: 1, cursor: 7 });
writeJson('.ephemeral/plugin-tasks.json', { version: 1, tasks: [] });
writeJson('.ephemeral/terminal-sessions/t1.json', { version: 1, shell: 'zsh', scrollback: wordStream(8) });
writeJson('subagent-runs.json', { version: 1, runs: [{ id: nextId('sub'), parent: 'run-parent-1' }] });
writeJson('workflow-activity.json', { version: 1, active: [] });
write('agents/a1/workflow-journals/run-w1.jsonl', jsonl([
  { type: 'step', id: nextId('w'), ts: nextTs(), note: wordStream(5) },
]));
writeJson('security/execution-leases.json', { version: 1, leases: [{ id: nextId('lease'), holder: 'run-parent-1' }] });

// ---- 11. epoch 域（out-of-scope，登记不迁移）----
writeJson('data-epoch.json', { epoch: 7, stampedAt: BASE_MS });
writeJson('data-epoch-transition.json', { from: 6, to: 7, at: BASE_MS });

// ---- 12. 插件运行态 ----
// v3（Lead 裁决 2026-10-23）：mystery-unmapped.bin 移出基座夹具——基座=干净 happy-path
//（全文件被 69-store 注册表分类）；unmapped 负路径变体由驱动脚本显式派生（追加
// `mystery-unmapped.bin` 后使用），杜绝「同一夹具两种命运」。负路径 fail-closed 契约
// 证据：v2 时代 t05-migrate-e2e-02；v3 时代由 r08_t05_migrate_e2e.mjs --variant unmapped 重证。
writeJson('plugin-data/old-plugin/state.json', { version: 1, counters: { runs: 3 } });

// 自证输出：文件清单 + 根哈希（逐字节判定交给 diff -r 双跑）。
const files = [];
function walk(dir, rel) {
  for (const name of readdirSync(dir).sort()) {
    const abs = join(dir, name);
    const r = rel ? `${rel}/${name}` : name;
    if (statSync(abs).isDirectory()) walk(abs, r);
    else files.push(r);
  }
}
walk(out, '');
files.sort();
const rootHash = createHash('sha256')
  .update(files.map((f) => `${sha256(readFileSync(join(out, f)))}  ${f}`).join('\n'))
  .digest('hex');
console.log(JSON.stringify({ seed, out, fileCount: files.length, rootHash }));
