// R07-T09 A17/A18 跨入口验收（真实链路，无 mocks）：
//   真实 lingxi-service 二进制 + 真实 CLI 子进程（chat --plain 读 stdin）
//   + 确定性 loopback 模型替身（OpenAI SSE，按轮次编排 mcp_call 驱动真实
//   write / stage_files 工具）+ 真实配对设备凭证。
// A17：CLI 发任务 → Run 真实写文件并登记 sf_/res_ → 手机凭证（chat /
//   resources.read / files.read / files.write 四项原 scope）在 bootstrap /
//   sessions / history / files / resources 链上见到同一 sessionId / runId /
//   messageId / resourceId，并同字节下载内容。
// A18：缺 resources.read 的设备凭证与已撤销凭证对已知 resourceId 的
//   GET / HEAD / 带票据访问全部拒绝；票据不豁免鉴权中间件（R02 冻结纪律）。
import { afterAll, beforeAll, describe, test } from 'vitest';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const BINARY = process.env.LINGXI_SERVICE_BIN
  || path.join(process.env.CARGO_TARGET_DIR || path.join(REPO, 'rust', 'target'), 'debug', 'lingxi-service');
const SUFFIX = `${process.pid}-${Date.now().toString(36)}`;
const FILE_NAME = 'a17-xentry-output.txt';
const PROMPT = `A17-XENTRY ${SUFFIX} 请写入并交付文件`;
const PAYLOAD = `A17 同字节核验 ✓ ${SUFFIX}\n第二行 multibyte ✓\n`;
const PAYLOAD_SHA256 = crypto.createHash('sha256').update(PAYLOAD, 'utf8').digest('hex');
const FINAL = `A17-XENTRY-FINAL-${SUFFIX}`;

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// 确定性模型替身：chat 模型按会话中 tool 消息数推进 write → stage_files →
// 最终答复；辅助模型（title/summarize）只回纯文本，绝不编排工具。
function startModelStub(targetFile) {
  const seen = [];
  const server = http.createServer((req, res) => {
    if (req.method !== 'POST' || !req.url.endsWith('/chat/completions')) {
      seen.push({ url: req.url, method: req.method, rejected: true });
      res.writeHead(404); res.end(); return;
    }
    const chunks = [];
    req.on('data', (chunk) => chunks.push(chunk));
    req.on('end', () => {
      const body = JSON.parse(Buffer.concat(chunks).toString('utf8'));
      seen.push({ model: body.model, stream: body.stream, messages: (body.messages || []).length,
        tools: (body.messages || []).filter((m) => m.role === 'tool').map((m) => String(m.content).slice(0, 300)) });
      const sse = (frames) => {
        const payload = frames.map((frame) => `data: ${JSON.stringify(frame)}\n\n`).join('') + 'data: [DONE]\n\n';
        res.writeHead(200, {
          'Content-Type': 'text/event-stream',
          'Content-Length': Buffer.byteLength(payload),
          Connection: 'close',
        });
        res.end(payload);
      };
      const finalText = (text) => sse([
        { id: 'a17', choices: [{ index: 0, delta: { role: 'assistant', content: text }, finish_reason: null }] },
        { id: 'a17', choices: [{ index: 0, delta: {}, finish_reason: 'stop' }] },
        { id: 'a17', choices: [], usage: { prompt_tokens: 10, completion_tokens: 10 } },
      ]);
      const toolCall = (id, tool, args) => sse([
        { id: 'a17', choices: [{ index: 0, delta: { role: 'assistant', tool_calls: [
          { index: 0, id, type: 'function', function: { name: 'mcp_call', arguments: JSON.stringify({ tool, arguments: args }) } },
        ] }, finish_reason: null }] },
        { id: 'a17', choices: [{ index: 0, delta: {}, finish_reason: 'tool_calls' }] },
      ]);
      const describe = (id, name) => sse([
        { id: 'a17', choices: [{ index: 0, delta: { role: 'assistant', tool_calls: [
          { index: 0, id, type: 'function', function: { name: 'mcp_describe_tool', arguments: JSON.stringify({ name }) } },
        ] }, finish_reason: null }] },
        { id: 'a17', choices: [{ index: 0, delta: {}, finish_reason: 'tool_calls' }] },
      ]);
      if (body.model !== 'chat') return finalText(`A17-AUX-${SUFFIX}`);
      if (!JSON.stringify(body.messages).includes('A17-XENTRY')) return finalText('A17-PASS-THROUGH');
      // 目录纪律：mcp_call 前必须本轮先 mcp_describe_tool（否则 invalid_message）。
      const toolCount = body.messages.filter((message) => message.role === 'tool').length;
      if (toolCount === 0) return describe('a17-d-write', 'write');
      if (toolCount === 1) return toolCall('a17-write', 'write', { path: targetFile, content: PAYLOAD });
      if (toolCount === 2) return describe('a17-d-stage', 'stage_files');
      if (toolCount === 3) return toolCall('a17-stage', 'stage_files', { filepaths: [targetFile] });
      return finalText(FINAL);
    });
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => resolve({ server, port: server.address().port, seen }));
  });
}

async function bootService(home, productDir, configPath) {
  // 摘掉宿主机代理变量：验收链全是 loopback，不应经任何代理（也避免
  // 宿主代理软件的可用性影响确定性）。
  const env = { ...process.env, LINGXI_HOME: home, LINGXI_PRODUCT_DIR: productDir, RUST_LOG: (i=>i||'debug')(process.env.A17_RUST_LOG) };
  for (const key of Object.keys(env)) if (/^(https?|all|no)_proxy$/i.test(key)) delete env[key];
  const child = spawn(BINARY, ['--config', configPath, '--bind', '127.0.0.1:0'], {
    env,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let stdout = ''; let stderr = '';
  child.stdout.on('data', (chunk) => { stdout += chunk; });
  child.stderr.on('data', (chunk) => { stderr += chunk; });
  for (let i = 0; i < 600; i += 1) {
    const match = stdout.match(/LINGXI_SERVICE_READY addr=(\S+)/);
    if (match) {
      const token = JSON.parse(fs.readFileSync(path.join(home, 'lingxi-service', 'local-token.json'), 'utf8')).token;
      return { child, url: `http://${match[1]}`, token, stderr: () => stderr };
    }
    if (child.exitCode !== null) throw new Error(`service exited during boot: ${stderr}`);
    await wait(50);
  }
  throw new Error(`service startup timeout: ${stderr}`);
}

async function stopService(child) {
  if (child.exitCode === null) child.kill('SIGTERM');
  for (let i = 0; i < 200 && child.exitCode === null; i += 1) await wait(50);
  if (child.exitCode === null) child.kill('SIGKILL');
}

// 真实 CLI 子进程：chat --plain 从 stdin 读一行 prompt，EOF 后等待任务终态退出。
// 直跑 cli/entry.ts：scripts/launch.js 的 applyDevEnvironment 会把 LINGXI_HOME
// 覆写为 ~/.lingxi-dev，无法指向本测试的隔离 home。
async function runCli(home) {
  const child = spawn(process.execPath, ['cli/entry.ts', 'chat', '--plain'], {
    cwd: REPO,
    env: { ...process.env, LINGXI_HOME: home },
    stdio: ['pipe', 'pipe', 'pipe'],
  });
  let stdout = ''; let stderr = '';
  child.stdout.on('data', (chunk) => { stdout += chunk; });
  child.stderr.on('data', (chunk) => { stderr += chunk; });
  child.stdin.write(`${PROMPT}\n`);
  child.stdin.end();
  const deadline = Date.now() + 120_000;
  while (child.exitCode === null) {
    if (Date.now() > deadline) { child.kill('SIGKILL'); throw new Error(`CLI timeout.\nstdout:\n${stdout}\nstderr:\n${stderr}`); }
    await wait(100);
  }
  return { code: child.exitCode, stdout, stderr };
}

const request = (base, token) => (method, route, body) => fetch(base + route, {
  method,
  headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
  body: body === undefined ? undefined : JSON.stringify(body),
});

describe('R07-T09 A17/A18 跨入口真实验收', () => {
  const ctx = {};

  beforeAll(async () => {
    assert.ok(fs.existsSync(BINARY), `lingxi-service binary missing: ${BINARY}（先构建或设 LINGXI_SERVICE_BIN）`);
    ctx.base = fs.mkdtempSync(path.join(os.tmpdir(), 'lingxi-a17-'));
    ctx.home = path.join(ctx.base, 'home');
    ctx.workspace = path.join(ctx.base, 'workspace');
    ctx.product = path.join(ctx.base, 'product');
    for (const dir of [ctx.home, ctx.workspace, ctx.product]) fs.mkdirSync(dir, { recursive: true });
    // 上下文编译的人格模板：缺失时运行会在发起供应商调用前失败（实测确诊）。
    fs.mkdirSync(path.join(ctx.product, 'yuan'), { recursive: true });
    fs.writeFileSync(path.join(ctx.product, 'yuan', 'lingxi.md'), '你叫{{agentName}}，与用户一起完成任务。');
    ctx.targetFile = path.join(fs.realpathSync(ctx.workspace), FILE_NAME);

    const stub = await startModelStub(ctx.targetFile);
    ctx.stub = stub.server;
    ctx.stubSeen = stub.seen;

    const config = {
      home: ctx.home,
      workspace: fs.realpathSync(ctx.workspace),
      providers: { remote: { protocol: 'openai-completions', endpoint: `http://127.0.0.1:${stub.port}/v1`, auth: { kind: 'apiKey', apiKey: 'A17_SYNTHETIC_ONLY' } } },
      models: {
        chat: { provider: 'remote', model: 'chat', capabilities: { tools: true, imageInput: true }, compat: { contextWindow: 200000, reasoning: true } },
        title: { provider: 'remote', model: 'second', capabilities: { tools: true, imageInput: true }, compat: { contextWindow: 200000, reasoning: true } },
        summarize: { provider: 'remote', model: 'summary', capabilities: { tools: true }, compat: { contextWindow: 1000000 } },
      },
    };
    const configPath = path.join(ctx.base, 'service-config.json');
    fs.writeFileSync(configPath, JSON.stringify(config));

    const service = await bootService(ctx.home, ctx.product, configPath);
    ctx.service = service;
    const owner = request(service.url, service.token);
    // 真实 API 建助手并设为当前（同 settings-owner-orchestrator 金样）。
    for (const [method, route, body] of [
      ['POST', '/lingxi/v1/agents', { id: 'a17agent', name: 'A17 验收助手', yuan: 'lingxi' }],
      ['PUT', '/lingxi/v1/agents/a17agent/config', { memory: { enabled: false } }],
      ['PUT', '/lingxi/v1/agents/primary', { id: 'a17agent' }],
      ['POST', '/lingxi/v1/agents/switch', { id: 'a17agent' }],
    ]) {
      const res = await owner(method, route, body);
      assert.ok(res.ok, `${method} ${route} -> ${res.status}: ${await res.text()}`);
    }

    // CLI 腿：真实子进程发 prompt，驱动真实 Run 与真实工具。
    const cli = await runCli(ctx.home);
    if (cli.code !== 0) {
      const dump = await (await request(service.url, service.token)('GET', '/lingxi/v1/sessions')).json().catch(() => null);
      let hist = null;
      try {
        const sid = dump.sessions[0].sessionId;
        hist = await (await request(service.url, service.token)('GET', `/lingxi/v1/sessions/${sid}/history?all=1&reconciliation=1`)).json();
      } catch (e) { hist = String(e); }
      console.error('A17-DEBUG history:', JSON.stringify(hist).slice(0, 4000));
    }
    assert.equal(cli.code, 0, `CLI exit=${cli.code}\nstdout:\n${cli.stdout}\nstderr:\n${cli.stderr}\nstubSeen:\n${JSON.stringify(ctx.stubSeen)}\nserviceStderr(尾部):\n${ctx.service.stderr().slice(-3000)}`);
    assert.ok(cli.stdout.includes(FINAL), `CLI 未输出模型最终答复\nstdout:\n${cli.stdout}`);
    assert.ok(cli.stdout.includes('◇'), `CLI 未展示工具执行\nstdout:\n${cli.stdout}`);
    if (!fs.existsSync(ctx.targetFile)) {
      const found = [];
      const walk = (dir) => { for (const e of fs.readdirSync(dir, { withFileTypes: true })) { const p = path.join(dir, e.name); if (e.isDirectory()) walk(p); else found.push(p); } };
      walk(ctx.base);
      const auditPath = path.join(ctx.home, 'logs', 'security-audit.jsonl');
      const audit = fs.existsSync(auditPath) ? fs.readFileSync(auditPath, 'utf8') : '(no audit)';
      const witnessDir = path.join(ctx.home, 'file-write-witnesses');
      const witnesses = fs.existsSync(witnessDir) ? fs.readdirSync(witnessDir).map((f) => fs.readFileSync(path.join(witnessDir, f), 'utf8')).join('\n') : '(none)';
      throw new Error(`write 落盘缺失: ${ctx.targetFile}\nbase 下全部文件:\n${found.join('\n')}\nstubSeen: ${JSON.stringify(ctx.stubSeen)}\n== security-audit ==\n${audit.slice(-3000)}\n== witnesses ==\n${witnesses.slice(0, 2000)}`);
    }
    assert.equal(fs.readFileSync(ctx.targetFile, 'utf8'), PAYLOAD, 'write 工具落盘字节不符');

    // 会话与资源身份（服务种子自带两个合成会话，按助手身份选 CLI 创建的那个）。
    const sessions = await (await owner('GET', '/lingxi/v1/sessions')).json();
    const mine = sessions.sessions.filter((row) => row.agentId === 'a17agent');
    assert.equal(mine.length, 1, JSON.stringify(sessions));
    ctx.sessionId = mine[0].sessionId;
    ctx.runId = mine[0].lastRuns?.[0]?.runId;
    assert.ok(ctx.runId, '会话缺 runId');
    const files = await (await owner('GET', `/lingxi/v1/sessions/${ctx.sessionId}/files`)).json();
    const registered = files.files.find((file) => file.filename === FILE_NAME);
    assert.ok(registered, `stage_files 未登记会话文件: ${JSON.stringify(files)}`);
    ctx.fileId = registered.fileId;
    ctx.resourceId = `res_${ctx.fileId}`;
    const ticketRes = await owner('POST', `/lingxi/v1/resources/${ctx.resourceId}/ticket`, {});
    assert.ok(ticketRes.ok, `owner 签票据失败: ${ticketRes.status}`);
    ctx.ticket = (await ticketRes.json()).ticket;

    // 手机配对：默认四项原 scope（chat/resources.read/files.read/files.write）。
    const pair = async (scopes) => {
      const created = await (await owner('POST', '/lingxi/v1/devices/pairing-sessions', {
        requestedDevice: { deviceKind: 'mobile', displayName: 'A17 Phone' }, ttlMs: 300000,
      })).json();
      const approved = await (await owner('POST', `/lingxi/v1/devices/pairing-sessions/${created.pairingSessionId}/approve`, {
        userCode: created.userCode, ...(scopes ? { scopes } : {}),
      })).json();
      assert.ok(approved.secret, `配对批准失败: ${JSON.stringify(approved)}`);
      return approved;
    };
    ctx.phone = await pair(null);
    ctx.reduced = await pair(['chat', 'files.read', 'files.write']);
  }, 180_000);

  afterAll(async () => {
    if (ctx.service) await stopService(ctx.service.child);
    if (ctx.stub) await new Promise((resolve) => ctx.stub.close(resolve));
    if (ctx.base) fs.rmSync(ctx.base, { recursive: true, force: true });
  });

  test('A17：手机四 scope 凭证见同一身份并同字节下载', async () => {
    const phone = request(ctx.service.url, ctx.phone.secret);

    const bootstrap = await phone('GET', '/lingxi/v1/mobile/bootstrap');
    if (bootstrap.status !== 200) throw new Error(`bootstrap ${bootstrap.status}: ${await bootstrap.text()}`);
    assert.equal((await bootstrap.json()).currentAgentId, 'a17agent');

    const sessions = await (await phone('GET', '/lingxi/v1/sessions')).json();
    const phoneRow = sessions.sessions.find((row) => row.agentId === 'a17agent');
    assert.ok(phoneRow, JSON.stringify(sessions));
    assert.equal(phoneRow.sessionId, ctx.sessionId);
    // runId 在列表与历史两侧一致（跨面同一运行）。
    assert.equal(phoneRow.lastRuns?.[0]?.runId, ctx.runId);

    const historyRes = await phone('GET', `/lingxi/v1/sessions/${ctx.sessionId}/history?all=1&reconciliation=1`);
    if (historyRes.status !== 200) throw new Error(`history ${historyRes.status}: ${await historyRes.text()}`);
    const history = await historyRes.json();
    assert.equal(history.sessionId, ctx.sessionId);
    const user = history.items.find((item) => item.kind === 'user_message');
    assert.ok(user && typeof user.messageId === 'string' && user.messageId, 'history 缺用户 messageId');
    assert.ok(user.text.includes(PROMPT), 'history 用户消息与 CLI 输入不符');
    const final = history.items.find((item) => item.kind === 'final_message');
    assert.ok(final && final.run && final.run.state === 'known' && typeof final.run.runId === 'string' && final.run.runId, 'history 缺 runId');
    assert.ok(JSON.stringify(final.content).includes(FINAL), 'history 最终答复与 CLI 输出不符');
    // write 工具结果携带 file: 引用与 sha256 digest，与盘上字节一致。
    const historyText = JSON.stringify(history.items);
    assert.ok(historyText.includes(`file:${ctx.targetFile}`), 'history 缺 write 产物引用');
    assert.ok(historyText.includes(PAYLOAD_SHA256), 'history 缺 write 产物 digest');

    const files = await (await phone('GET', `/lingxi/v1/sessions/${ctx.sessionId}/files`)).json();
    assert.equal(files.files.find((file) => file.filename === FILE_NAME).fileId, ctx.fileId);

    const envelope = await phone('GET', `/lingxi/v1/resources/${ctx.resourceId}`);
    if (envelope.status !== 200) throw new Error(`envelope ${envelope.status}: ${await envelope.text()}`);
    assert.equal((await envelope.json()).resourceId, ctx.resourceId);

    const content = await phone('GET', `/lingxi/v1/resources/${ctx.resourceId}/content`);
    if (content.status !== 200) throw new Error(`content ${content.status}: ${await content.text()}`);
    const bytes = Buffer.from(await content.arrayBuffer());
    assert.equal(bytes.toString('utf8'), PAYLOAD, '手机下载字节与写入不符');
    assert.equal(crypto.createHash('sha256').update(bytes).digest('hex'), PAYLOAD_SHA256);
  });

  test('A18：缺 resources.read 与已撤销凭证对已知 resourceId 全拒（票据不豁免）', async () => {
    const owner = request(ctx.service.url, ctx.service.token);
    const reduced = request(ctx.service.url, ctx.reduced.secret);
    const content = `/lingxi/v1/resources/${ctx.resourceId}/content`;

    // 对照：缺 scope 凭证本身有效（chat 可读），仅资源面被拒。
    assert.equal((await reduced('GET', '/lingxi/v1/sessions')).status, 200);
    assert.equal((await reduced('GET', content)).status, 403);
    assert.equal((await reduced('HEAD', content)).status, 403);
    assert.equal((await reduced('GET', `${content}?ticket=${encodeURIComponent(ctx.ticket)}`)).status, 403);

    // 撤销手机凭证后：GET/HEAD/带票据均 401，连 chat 面也一起失效。
    const revoke = await owner('POST', `/lingxi/v1/devices/credentials/${ctx.phone.credential.credentialId}/revoke`, {});
    assert.ok(revoke.ok, `撤销失败: ${revoke.status} ${await revoke.text()}`);
    const phone = request(ctx.service.url, ctx.phone.secret);
    assert.equal((await phone('GET', content)).status, 401);
    assert.equal((await phone('HEAD', content)).status, 401);
    assert.equal((await phone('GET', `${content}?ticket=${encodeURIComponent(ctx.ticket)}`)).status, 401);
    assert.equal((await phone('GET', '/lingxi/v1/sessions')).status, 401);
    // 票据本身仍有效：owner 带它下载仍 200（拒绝来自凭证状态，不是资源损坏）。
    assert.equal((await owner('GET', `${content}?ticket=${encodeURIComponent(ctx.ticket)}`)).status, 200);
  }, 60_000);
});
