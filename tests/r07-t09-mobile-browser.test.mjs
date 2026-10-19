// R07-T09 窄视口手机端真实浏览器验收（无 mocks）：
//   真实 lingxi-service 二进制（托管 desktop/dist-renderer 的 /mobile）
//   + 真实 CLI 子进程造一条会话历史 + 真实配对手机凭证
//   + 真实 Chrome（headless=new，390 宽视口）经 CDP 驱动：
//   登录 → 会话列表 → 打开会话见 CLI 造的消息 → 刷新后 cookie 会话恢复、
//   消息不重发（stub 侧 chat 请求数与页面消息数双重不变）。
import { afterAll, beforeAll, describe, test } from 'vitest';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import WebSocket from 'ws';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const BINARY = process.env.LINGXI_SERVICE_BIN
  || path.join(process.env.CARGO_TARGET_DIR || path.join(REPO, 'rust', 'target'), 'debug', 'lingxi-service');
const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const RENDERER_DIST = path.join(REPO, 'desktop', 'dist-renderer');
const SHOTS_DIR = path.join(REPO, 'artifacts', 'rust-tauri', 'R07', 'T09', 'browser');
const SUFFIX = `${process.pid}-${Date.now().toString(36)}`;
const PROMPT = `T09-MOBILE ${SUFFIX} 请直接回答`;
const FINAL = `T09-MOBILE-FINAL-${SUFFIX}`;
const TITLE = `T09TITLE${SUFFIX}`;

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// 确定性模型替身：chat 模型直接回最终答复（无工具编排），辅助模型回标题文本。
function startModelStub() {
  const seen = { chat: 0, aux: 0, promptChats: 0 };
  const server = http.createServer((req, res) => {
    if (req.method !== 'POST' || !req.url.endsWith('/chat/completions')) {
      res.writeHead(404); res.end(); return;
    }
    const chunks = [];
    req.on('data', (chunk) => chunks.push(chunk));
    req.on('end', () => {
      const body = JSON.parse(Buffer.concat(chunks).toString('utf8'));
      const sse = (text) => {
        const payload = [
          `data: ${JSON.stringify({ id: 't09', choices: [{ index: 0, delta: { role: 'assistant', content: text }, finish_reason: null }] })}\n\n`,
          `data: ${JSON.stringify({ id: 't09', choices: [{ index: 0, delta: {}, finish_reason: 'stop' }] })}\n\n`,
          `data: ${JSON.stringify({ id: 't09', choices: [], usage: { prompt_tokens: 10, completion_tokens: 10 } })}\n\n`,
          'data: [DONE]\n\n',
        ].join('');
        res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Content-Length': Buffer.byteLength(payload), Connection: 'close' });
        res.end(payload);
      };
      if (body.model === 'chat') {
        seen.chat += 1;
        if (JSON.stringify(body.messages).includes('T09-MOBILE')) seen.promptChats += 1;
        return sse(FINAL);
      }
      seen.aux += 1;
      return sse(TITLE);
    });
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => resolve({ server, port: server.address().port, seen }));
  });
}

async function bootService(home, productDir, configPath) {
  const env = {
    ...process.env,
    LINGXI_HOME: home,
    LINGXI_PRODUCT_DIR: productDir,
    LINGXI_RENDERER_DIST: RENDERER_DIST,
  };
  for (const key of Object.keys(env)) if (/^(https?|all|no)_proxy$/i.test(key)) delete env[key];
  const child = spawn(BINARY, ['--config', configPath, '--bind', '127.0.0.1:0'], {
    env, stdio: ['ignore', 'pipe', 'pipe'],
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

// 真实 CLI 子进程（直跑 cli/entry.ts：launch.js 会覆写 LINGXI_HOME）。
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

// 最薄 CDP 客户端：method 调用 + evaluate 轮询，不引浏览器自动化框架。
class Cdp {
  constructor(ws) { this.ws = ws; this.seq = 0; this.pending = new Map(); }
  static async connect(port) {
    const targets = await new Promise((resolve, reject) => {
      http.get({ host: '127.0.0.1', port, path: '/json/list' }, (res) => {
        let data = '';
        res.on('data', (chunk) => { data += chunk; });
        res.on('end', () => resolve(JSON.parse(data)));
      }).on('error', reject);
    });
    const page = targets.find((t) => t.type === 'page');
    assert.ok(page, `CDP 无 page target: ${JSON.stringify(targets)}`);
    const ws = new WebSocket(page.webSocketDebuggerUrl, { perMessageDeflate: false });
    await new Promise((resolve, reject) => { ws.once('open', resolve); ws.once('error', reject); });
    const cdp = new Cdp(ws);
    ws.on('message', (data) => {
      const msg = JSON.parse(data.toString());
      if (msg.id && cdp.pending.has(msg.id)) {
        const { resolve, reject } = cdp.pending.get(msg.id);
        cdp.pending.delete(msg.id);
        if (msg.error) reject(new Error(`${msg.error.message}`)); else resolve(msg.result);
      }
    });
    return cdp;
  }
  send(method, params = {}) {
    const id = ++this.seq;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }
  async eval(expression) {
    const r = await this.send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
    if (r.exceptionDetails) throw new Error(`页面内脚本失败: ${JSON.stringify(r.exceptionDetails).slice(0, 500)}\n脚本: ${expression.slice(0, 300)}`);
    return r.result.value;
  }
  async until(expression, label, timeoutMs = 30_000) {
    const t0 = Date.now();
    for (;;) {
      if (await this.eval(expression)) return;
      if (Date.now() - t0 > timeoutMs) throw new Error(`等待超时: ${label}`);
      await wait(200);
    }
  }
}

async function bootChrome(profileDir) {
  const child = spawn(CHROME, [
    '--headless=new', '--remote-debugging-port=0', '--no-first-run', '--no-default-browser-check',
    '--window-size=390,844', '--hide-scrollbars', `--user-data-dir=${profileDir}`,
  ], { stdio: ['ignore', 'ignore', 'pipe'] });
  let stderr = '';
  child.stderr.on('data', (chunk) => { stderr += chunk; });
  const portFile = path.join(profileDir, 'DevToolsActivePort');
  for (let i = 0; i < 300; i += 1) {
    if (fs.existsSync(portFile)) {
      const port = Number(fs.readFileSync(portFile, 'utf8').split('\n')[0].trim());
      if (port > 0) return { child, port };
    }
    if (child.exitCode !== null) throw new Error(`Chrome 退出: ${stderr.slice(-1500)}`);
    await wait(100);
  }
  throw new Error(`Chrome DevToolsActivePort 超时: ${stderr.slice(-1500)}`);
}

describe('R07-T09 窄视口手机端真实浏览器验收', () => {
  const ctx = {};

  beforeAll(async () => {
    assert.ok(fs.existsSync(BINARY), `lingxi-service binary missing: ${BINARY}`);
    assert.ok(fs.existsSync(path.join(RENDERER_DIST, 'mobile.html')), `renderer 产物缺失: ${RENDERER_DIST}/mobile.html（先构建 renderer）`);
    assert.ok(fs.existsSync(CHROME), `Chrome 缺失: ${CHROME}`);
    ctx.base = fs.mkdtempSync(path.join(os.tmpdir(), 'lingxi-t09-'));
    ctx.home = path.join(ctx.base, 'home');
    ctx.product = path.join(ctx.base, 'product');
    for (const dir of [ctx.home, ctx.product]) fs.mkdirSync(dir, { recursive: true });
    fs.mkdirSync(path.join(ctx.product, 'yuan'), { recursive: true });
    fs.writeFileSync(path.join(ctx.product, 'yuan', 'lingxi.md'), '你叫{{agentName}}，与用户一起完成任务。');

    const stub = await startModelStub();
    ctx.stub = stub.server;
    ctx.stubSeen = stub.seen;

    const config = {
      home: ctx.home,
      providers: {
        remote: { protocol: 'openai-completions', endpoint: `http://127.0.0.1:${stub.port}/v1`, auth: { kind: 'apiKey', apiKey: 'T09_SYNTHETIC_ONLY' } },
      },
      models: {
        chat: { provider: 'remote', model: 'chat', capabilities: { tools: true, imageInput: true }, compat: { contextWindow: 200000, reasoning: true } },
        title: { provider: 'remote', model: 'second', capabilities: { tools: true }, compat: { contextWindow: 200000 } },
        summarize: { provider: 'remote', model: 'summary', capabilities: { tools: true }, compat: { contextWindow: 1000000 } },
      },
    };
    const configPath = path.join(ctx.base, 'service-config.json');
    fs.writeFileSync(configPath, JSON.stringify(config));

    const service = await bootService(ctx.home, ctx.product, configPath);
    ctx.service = service;
    const owner = request(service.url, service.token);
    for (const [method, route, body] of [
      ['POST', '/lingxi/v1/agents', { id: 't09agent', name: 'T09 验收助手', yuan: 'lingxi' }],
      ['PUT', '/lingxi/v1/agents/t09agent/config', { memory: { enabled: false } }],
      ['PUT', '/lingxi/v1/agents/primary', { id: 't09agent' }],
      ['POST', '/lingxi/v1/agents/switch', { id: 't09agent' }],
    ]) {
      const res = await owner(method, route, body);
      assert.ok(res.ok, `${method} ${route} -> ${res.status}: ${await res.text()}`);
    }

    // CLI 腿造一条真实历史（user_message + final_message）。
    const cli = await runCli(ctx.home);
    assert.equal(cli.code, 0, `CLI exit=${cli.code}\nstdout:\n${cli.stdout}\nstderr:\n${cli.stderr}\nserviceStderr(尾部):\n${service.stderr().slice(-2000)}`);
    assert.ok(cli.stdout.includes(FINAL), `CLI 未输出最终答复\nstdout:\n${cli.stdout}`);
    ctx.sessionId = (await (await owner('GET', '/lingxi/v1/sessions')).json())
      .sessions.find((row) => row.agentId === 't09agent').sessionId;

    // 手机配对：默认四项原 scope（chat/resources.read/files.read/files.write）。
    const created = await (await owner('POST', '/lingxi/v1/devices/pairing-sessions', {
      requestedDevice: { deviceKind: 'mobile', displayName: 'T09 Phone' }, ttlMs: 300000,
    })).json();
    const approved = await (await owner('POST', `/lingxi/v1/devices/pairing-sessions/${created.pairingSessionId}/approve`, {
      userCode: created.userCode,
    })).json();
    assert.ok(approved.secret, `配对批准失败: ${JSON.stringify(approved)}`);
    ctx.secret = approved.secret;

    ctx.chrome = await bootChrome(path.join(ctx.base, 'chrome-profile'));
    ctx.cdp = await Cdp.connect(ctx.chrome.port);
    await ctx.cdp.send('Page.enable');
    fs.mkdirSync(SHOTS_DIR, { recursive: true });
  }, 180_000);

  afterAll(async () => {
    if (ctx.cdp) ctx.cdp.ws.close();
    if (ctx.chrome && ctx.chrome.child.exitCode === null) ctx.chrome.child.kill('SIGKILL');
    if (ctx.service) await stopService(ctx.service.child);
    if (ctx.stub) await new Promise((resolve) => ctx.stub.close(resolve));
    if (ctx.base) fs.rmSync(ctx.base, { recursive: true, force: true });
  });

  const shot = async (name) => {
    const r = await ctx.cdp.send('Page.captureScreenshot', { format: 'png' });
    fs.writeFileSync(path.join(SHOTS_DIR, `${name}-${SUFFIX}.png`), Buffer.from(r.data, 'base64'));
  };

  test('手机端登录→列表→历史→刷新恢复且不重发', async () => {
    const cdp = ctx.cdp;
    await cdp.send('Page.navigate', { url: `${ctx.service.url}/mobile` });
    // 未认证 → 登录表单（device 模式的 secret 输入框）。
    await cdp.until(`!!document.querySelector('form.onboarding-step input.ob-input')`, '登录表单出现');
    assert.match(await cdp.eval(`location.pathname`), /^\/mobile/);

    // React 受控输入：原生 setter + input 事件，然后提交表单。
    await cdp.eval(`(() => {
      const input = document.querySelector('form.onboarding-step input.ob-input');
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
      setter.call(input, ${JSON.stringify(ctx.secret)});
      input.dispatchEvent(new Event('input', { bubbles: true }));
      return true;
    })()`);
    await cdp.eval(`document.querySelector('form.onboarding-step button[type="submit"]').click()`);
    await cdp.until(`!!document.querySelector('main.mobile-desktop-root')`, '登录后进入主界面');
    assert.equal(await cdp.eval(`window.matchMedia('(max-width: 860px)').matches`), true, '窄视口媒体查询未命中（窗口宽度未生效）');
    await shot('t09-mobile-ready');

    // 打开侧栏，展开「未归组」组（非活跃组默认折叠是既有纪律），
    // 点击 CLI 造的那条会话（native:<sessionId> 路径稳定可辨）。
    const openTargetSession = `(() => {
      const item = document.querySelector('[data-session-path="native:${ctx.sessionId}"]');
      if (item) { item.click(); return 'clicked'; }
      const header = document.querySelector('[data-project-id="ungrouped"] [aria-expanded="false"]');
      if (header) { header.click(); return 'expanded'; }
      return false;
    })()`;
    await cdp.eval(`document.getElementById('tbToggleLeft').click()`);
    try {
      await cdp.until(`(${openTargetSession} === 'clicked')`, '会话列表出现目标会话并点击');
    } catch (err) {
      const dbg = await cdp.eval(`(() => ({
        sidebarOpen: !document.querySelector('#sidebar.collapsed'),
        groups: [...document.querySelectorAll('[data-project-id]')].map((g) => ({ id: g.getAttribute('data-project-id'), collapsed: g.querySelector('[aria-expanded]')?.getAttribute('aria-expanded'), count: g.querySelectorAll('[data-session-path]').length })),
        sessionItems: [...document.querySelectorAll('[data-session-path]')].map((e) => ({ path: e.getAttribute('data-session-path'), text: e.innerText.slice(0, 120) })),
        sidebarText: (document.querySelector('.session-list') || {}).innerText || null,
      }))()`);
      const apiSessions = await (await request(ctx.service.url, ctx.secret)('GET', '/lingxi/v1/sessions')).json();
      throw new Error(`${err.message}\n页面侧: ${JSON.stringify(dbg, null, 2)}\nAPI 侧: ${JSON.stringify(apiSessions.sessions?.map((s) => ({ id: s.sessionId, title: s.title, firstMessage: (s.firstMessage || '').slice(0, 60) })), null, 2)}`);
    }

    // 消息可见：CLI 的 prompt 与模型最终答复都在手机端渲染。
    await cdp.until(`(() => {
      const text = document.querySelector('main.mobile-desktop-root').innerText;
      return document.querySelectorAll('[data-message-id]').length >= 2
        && text.includes(${JSON.stringify(PROMPT)}) && text.includes(${JSON.stringify(FINAL)});
    })()`, '消息列表渲染 prompt 与最终答复');
    const before = await cdp.eval(`(() => ({
      messages: document.querySelectorAll('[data-message-id]').length,
      chats: ${'0'},
    }))()`);
    before.chats = ctx.stubSeen.promptChats;
    await shot('t09-mobile-history');

    // 刷新：cookie 会话应直接恢复（不再出现登录表单），且不得重发 prompt。
    await cdp.send('Page.reload');
    await cdp.until(`!!document.querySelector('main.mobile-desktop-root')`, '刷新后直接进入主界面（cookie 恢复）');
    assert.equal(await cdp.eval(`!!document.querySelector('form.onboarding-step')`), false, '刷新后不应回到登录表单');
    await cdp.eval(`document.getElementById('tbToggleLeft').click()`);
    await cdp.until(`(${openTargetSession} === 'clicked')`, '刷新后会话列表恢复并重新打开目标会话');
    await cdp.until(`(() => document.querySelector('main.mobile-desktop-root').innerText.includes(${JSON.stringify(FINAL)}))()`, '刷新后历史消息恢复');
    await wait(2000); // 给潜在的重发一个暴露窗口
    const after = {
      messages: await cdp.eval(`document.querySelectorAll('[data-message-id]').length`),
      chats: ctx.stubSeen.promptChats,
    };
    assert.equal(after.messages, before.messages, `刷新后消息数变化（疑似重发/重复渲染）: ${before.messages} -> ${after.messages}`);
    assert.equal(after.chats, before.chats, `刷新后 stub 收到新的 chat 请求（prompt 被重发）: ${before.chats} -> ${after.chats}`);
    await shot('t09-mobile-reloaded');
  }, 120_000);
});
