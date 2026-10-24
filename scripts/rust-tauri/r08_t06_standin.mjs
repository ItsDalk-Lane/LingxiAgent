#!/usr/bin/env node
// R08-T06 §4e｜确定性外部平台替身（A12 哨兵载体）。
//
// 职责（T06_DESIGN §5，形态同 r00-t06-stub-provider 先例）：
// - 本地环回 HTTP「外部平台」：POST /platform/send 计数 + 签发确定性平台收据。
//   三条哨兵腿按会话 id 路由（全部经真实桥接投递链到达此处）：
//     conv-t06-s1 → 确认收据 {messageId:"t06-standin-<n>"}（计数器 +1）
//     conv-t06-s2 → 无 message_id（宿主侧落 unknown：confirmed 空 ids → Unknown，
//                   bridge.rs:409 语义，不伪造收据）
//     conv-t06-s3 → withhold（worker 不回执，行留 started，等服务重启经 startup
//                   校验落定——A12「S3 经校验落定」）
// - 计数器持久化（<scratch>/standin-state.json，每发送即写）：跨进程重启续值，
//   「恒=1」断言的审计基线；GET /counter 只读。
// - 确定性模型端点（POST /v1/chat/completions，OpenAI 兼容流式/非流式）：
//   桥接回信要走真实模型链（ConfigModelGateway），端点必须本地环回且输出确定。
// - 同时产出配套协议 worker（<scratch>/t06-standin-worker.cjs）：真实桥接宿主
//   经 LINGXI_BRIDGE_NODE/LINGXI_BRIDGE_WORKER 注入缝 spawn（bridge_worker.rs
//   discover 显式路径优先；spawn env_clear 后 worker 拿不到环境变量——端口必须
//   常量约定，见 STANDIN_PORT）。
//
// 协议事实锚（r07_bridge_flow.rs 既有先例）：init→emit ready；inbound message 帧
// 触发 run+回信；send 帧 → 本替身 HTTP → receipt 帧（confirmed+platformMessageIds
// / confirmed 空 ids → unknown）；stop → exit 0。同 messageId 重投由宿主去重。
//
// 用法：node scripts/rust-tauri/r08_t06_standin.mjs --scratch <dir>
// 输出：stderr `STANDIN_READY port=<port>`；关闭：POST /admin/shutdown。
// 全部数据合成；只监听 127.0.0.1；不触真实供应商与真实 ~/.lingxi。
import http from 'node:http';
import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

// ── 常量约定（worker 与 server 共享；不得改一处不改另一处）──
export const STANDIN_PORT = 47613;
export const STANDIN_HOST = '127.0.0.1';

const args = process.argv.slice(2);
const argValue = (flag) => {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : null;
};
const scratch = argValue('--scratch');
if (!scratch) {
  console.error('usage: r08_t06_standin.mjs --scratch <dir>');
  process.exit(2);
}
mkdirSync(scratch, { recursive: true });

const STATE_PATH = join(scratch, 'standin-state.json');
const JOURNAL_PATH = join(scratch, 'standin-journal.jsonl');
const WORKER_PATH = join(scratch, 't06-standin-worker.cjs');

// ── 持久计数器（跨重启续值）──
function loadState() {
  if (!existsSync(STATE_PATH)) return { platformSendCount: 0, receiptCounter: 0 };
  try {
    return JSON.parse(readFileSync(STATE_PATH, 'utf8'));
  } catch {
    return { platformSendCount: 0, receiptCounter: 0 };
  }
}
function saveState(state) {
  writeFileSync(STATE_PATH, `${JSON.stringify(state, null, 2)}\n`);
}
const state = loadState();
function journal(entry) {
  appendFileSync(JOURNAL_PATH, `${JSON.stringify({ at: new Date().toISOString(), ...entry })}\n`);
}

// ── 配套协议 worker（真实桥接宿主的子进程；见文件头协议锚）──
const WORKER_SOURCE = `\
// R08-T06 协议替身 worker（由 r08_t06_standin.mjs 产出；宿主 spawn 后 env_clear，
// 替身端口为编译期常量）。stdout 只输出协议 JSONL。
const http = require('node:http');
const readline = require('node:readline');
const STANDIN_PORT = ${STANDIN_PORT};
const STANDIN_HOST = '${STANDIN_HOST}';
let init = null;
const media = new Map();
const INBOUND = [
  { leg: 's1', conversationId: 'conv-t06-s1', text: 'T06 哨兵 S1：请确认送达。' },
  { leg: 's2', conversationId: 'conv-t06-s2', text: 'T06 哨兵 S2：结果未知腿。' },
  { leg: 's3', conversationId: 'conv-t06-s3', text: 'T06 哨兵 S3：挂起回执腿。' },
];
const rl = readline.createInterface({ input: process.stdin });
rl.on('line', (line) => {
  let command;
  try { command = JSON.parse(line); } catch { process.exit(1); return; }
  const emit = (body) => process.stdout.write(JSON.stringify({
    connectorId: init.connectorId, generation: init.generation, ...body,
  }) + '\\n');
  if (command.kind === 'init') {
    init = command;
    emit({ kind: 'ready' });
    for (const { conversationId, text } of INBOUND) {
      emit({
        kind: 'message',
        messageId: 't06-inbound-' + conversationId,
        conversationId,
        conversationKind: 'dm',
        senderId: 'sender-t06',
        text,
        attachments: [],
        replyContext: { isGroup: false, targetChatId: 'sender-t06', contextToken: 't06-synthetic-context' },
      });
    }
  } else if (command.kind === 'media_begin') {
    media.set(command.attachmentId, []);
  } else if (command.kind === 'media_chunk') {
    media.get(command.attachmentId).push(Buffer.from(command.data, 'base64'));
  } else if (command.kind === 'send') {
    const payload = JSON.stringify({
      conversationId: command.conversationId,
      deliveryId: command.deliveryId,
      text: command.text,
      attachments: (command.attachments || []).map((id) => Buffer.concat(media.get(id) || []).toString('base64')),
    });
    const req = http.request(
      { host: STANDIN_HOST, port: STANDIN_PORT, path: '/platform/send', method: 'POST',
        headers: { 'content-type': 'application/json', 'content-length': Buffer.byteLength(payload) } },
      (res) => {
        let raw = '';
        res.on('data', (d) => (raw += d));
        res.on('end', () => {
          let result;
          try { result = JSON.parse(raw); } catch { result = {}; }
          if (result.withhold) return; // S3：不回执，行留 started（startup 校验落定）
          if (result.messageId) {
            emit({ kind: 'receipt', deliveryId: command.deliveryId, status: 'confirmed',
              platformMessageIds: [result.messageId], error: null });
          } else {
            // S2：平台未回消息编号——真实平台行为，宿主落 unknown（不伪造收据）。
            emit({ kind: 'receipt', deliveryId: command.deliveryId, status: 'unknown',
              platformMessageIds: [], error: 'platform_returned_no_message_id' });
          }
        });
      },
    );
    req.on('error', () => {
      emit({ kind: 'receipt', deliveryId: command.deliveryId, status: 'unknown',
        platformMessageIds: [], error: 'standin_unreachable' });
    });
    req.end(payload);
  } else if (command.kind === 'stop') {
    process.exit(0);
  }
});
`;
writeFileSync(WORKER_PATH, WORKER_SOURCE);

// ── 确定性模型语料（同一输入 → 同一输出字节；禁随机源）──
function deterministicReply(messages) {
  const last = messages.filter((m) => m && m.role === 'user').pop();
  const echo = last && typeof last.content === 'string' ? last.content.slice(0, 80) : '';
  return `T06 确定性回信：已收到「${echo}」。本回复由本地替身模型生成，内容仅依赖输入。`;
}

const server = http.createServer((req, res) => {
  const url = new URL(req.url, `http://${STANDIN_HOST}:${STANDIN_PORT}`);
  const send = (code, body, headers = {}) => {
    res.writeHead(code, { 'content-type': 'application/json', ...headers });
    res.end(typeof body === 'string' ? body : JSON.stringify(body));
  };
  if (req.method === 'GET' && url.pathname === '/health') {
    return send(200, { ok: true, port: STANDIN_PORT });
  }
  if (req.method === 'GET' && url.pathname === '/counter') {
    return send(200, { platformSendCount: state.platformSendCount, receiptCounter: state.receiptCounter });
  }
  if (req.method === 'GET' && url.pathname === '/journal') {
    const lines = existsSync(JOURNAL_PATH) ? readFileSync(JOURNAL_PATH, 'utf8').trimEnd().split('\n').filter(Boolean) : [];
    return send(200, { entries: lines.map((l) => JSON.parse(l)) });
  }
  if (req.method === 'POST' && url.pathname === '/admin/shutdown') {
    send(200, { ok: true });
    setTimeout(() => server.close(() => process.exit(0)), 50);
    return;
  }
  if (req.method === 'POST' && url.pathname === '/platform/send') {
    let raw = '';
    req.on('data', (d) => (raw += d));
    req.on('end', () => {
      let body = {};
      try { body = JSON.parse(raw); } catch { /* 坏包按未知腿处理 */ }
      state.platformSendCount += 1;
      saveState(state);
      if (String(body.conversationId || '').endsWith('-s1')) {
        state.receiptCounter += 1;
        saveState(state);
        const messageId = `t06-standin-${state.receiptCounter}`;
        journal({ kind: 'platform-send', conversationId: body.conversationId, deliveryId: body.deliveryId, verdict: 'confirmed', messageId, count: state.platformSendCount });
        return send(200, { ok: true, messageId });
      }
      if (String(body.conversationId || '').endsWith('-s2')) {
        journal({ kind: 'platform-send', conversationId: body.conversationId, deliveryId: body.deliveryId, verdict: 'no-message-id', count: state.platformSendCount });
        return send(200, { ok: true, messageId: null });
      }
      if (String(body.conversationId || '').endsWith('-s3')) {
        journal({ kind: 'platform-send', conversationId: body.conversationId, deliveryId: body.deliveryId, verdict: 'withhold-receipt', count: state.platformSendCount });
        return send(200, { ok: true, withhold: true });
      }
      // 集外会话：替身只观察不确认（真实外部平台不会为未知会话发收据）。
      journal({ kind: 'platform-send', conversationId: body.conversationId, deliveryId: body.deliveryId, verdict: 'out-of-sentinel-scope', count: state.platformSendCount });
      send(200, { ok: true, messageId: null });
    });
    return;
  }
  if (req.method === 'POST' && url.pathname === '/v1/chat/completions') {
    let raw = '';
    req.on('data', (d) => (raw += d));
    req.on('end', () => {
      let body = {};
      try { body = JSON.parse(raw); } catch { return send(400, { error: { message: 'bad json' } }); }
      const text = deterministicReply(Array.isArray(body.messages) ? body.messages : []);
      journal({ kind: 'model-call', model: body.model, stream: Boolean(body.stream) });
      const id = 't06-standin-chatcmpl';
      if (body.stream) {
        res.writeHead(200, { 'content-type': 'text/event-stream' });
        const chunk = (delta) => `data: ${JSON.stringify({ id, object: 'chat.completion.chunk', choices: [{ index: 0, delta, finish_reason: null }] })}\n\n`;
        res.write(chunk({ role: 'assistant', content: '' }));
        for (const piece of text.match(/.{1,24}/g) || []) res.write(chunk({ content: piece }));
        res.write(`data: ${JSON.stringify({ id, object: 'chat.completion.chunk', choices: [{ index: 0, delta: {}, finish_reason: 'stop' }] })}\n\n`);
        res.end('data: [DONE]\n\n');
        return;
      }
      return send(200, {
        id,
        object: 'chat.completion',
        created: 0,
        model: body.model || 't06-standin',
        choices: [{ index: 0, message: { role: 'assistant', content: text }, finish_reason: 'stop' }],
        usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 },
      });
    });
    return;
  }
  send(404, { error: { message: `no route ${req.method} ${url.pathname}` } });
});

server.on('error', (err) => {
  console.error(`STANDIN_BIND_FAILED port=${STANDIN_PORT}: ${err.message}`);
  process.exit(1);
});
server.listen(STANDIN_PORT, STANDIN_HOST, () => {
  console.error(`STANDIN_READY port=${STANDIN_PORT} scratch=${scratch}`);
  console.error(`STANDIN_WORKER ${WORKER_PATH}`);
  console.error(`STANDIN_COUNTER_RESUMED count=${state.platformSendCount}`);
});
