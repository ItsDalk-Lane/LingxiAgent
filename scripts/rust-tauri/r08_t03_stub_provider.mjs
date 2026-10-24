#!/usr/bin/env node
/**
 * R08-T03 批次 D（A05）确定性供应商替身（Lead 裁决版：读腿 + 写腿）。
 *
 * 与 r00-t06-stub-provider.mjs 不同：本替身按 OpenAI tool_calls 形状驱动，
 * 帧形状逐字节镜像 rust/crates/lingxi-service/tests/r08_t01_a01_a02_ws.rs
 * 的测试夹具（act → tool_calls delta → finish_reason:"tool_calls" → [DONE]；
 * 终结幕 → content delta → finish_reason:"stop" → usage → [DONE]）。
 *
 * 幕序裁决键 = 最后一条 role:"user" 消息里的任务标记（Lead 裁决，替代旧的
 * role:"tool" 条数法）：
 *   - 含 A05-READ-TASK：读腿。无 tool 结果 → 调已注册的 `read` 工具读
 *     --read-path 指定的相对路径；有 tool 结果 → 终结回答**原样回显**工具
 *     回包内容（端到端真实往返证据，编排器据回显内容断言）。
 *   - 含 A05-WRITE-TASK：写腿（PPG-1 红证据腿）。无条件调**未注册**的
 *     `write` 工具 → Rust 侧 target_of_wire_name 拒绝（"provider requested
 *     unknown tool"）→ run 落定 failed.provider_error。编排器有界轮询服务
 *     日志取证，不得挂 untilFrameType。
 *   - 无标记：400 显式拒绝（替身同样禁止静默兜底）。
 * 仅 `chat` 模型走工具链脚本；title 等辅助模型拿最小终结帧。
 * 非流式请求（stream!==true）返回一次性 JSON，形状与 SSE 幕一致。
 *
 * 终结幕首字节前固定延迟 --final-delay-ms（默认 1500），给演练编排器
 * 留出「run 进行中」的进程树快照窗口；该延迟是夹具确定性参数，不是
 * 对生产行为的模拟降级。
 *
 * 只绑 127.0.0.1；所有请求体落 --journal JSONL 供计数与工具面核对
 * （journal 用同步 writeSync 落盘，编排器可随时安全读）。
 * 就绪时向 stdout 打一行 `A05_STUB_READY <port>`。
 */
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';

const READ_MARKER = 'A05-READ-TASK';
const WRITE_MARKER = 'A05-WRITE-TASK';
// A06：纯聊天腿（零文件工具接触）。提示形状 "A06-CHAT-TASK <leg-token>"，
// 终结回答回显 leg-token，编排器据此区分 A 的第一次/第二次任务。
const CHAT_MARKER = 'A06-CHAT-TASK';
const WRITE_PATH = 'a05-drill/write-note.txt';
const WRITE_CONTENT = 'A05-WRITE-v1\n';
const FINAL_ANSWER = 'A05-FINAL-ANSWER';
const TITLE_ANSWER = 'A05 演练会话';

function parseArgs(argv) {
  const args = { port: 0, journal: null, finalDelayMs: 1500, readPath: null };
  for (let i = 2; i < argv.length; i += 1) {
    const key = argv[i];
    const value = argv[i + 1];
    if (key === '--port') { args.port = Number(value); i += 1; }
    else if (key === '--journal') { args.journal = path.resolve(value); i += 1; }
    else if (key === '--final-delay-ms') { args.finalDelayMs = Number(value); i += 1; }
    else if (key === '--read-path') { args.readPath = value; i += 1; }
    else throw new Error(`unknown argument: ${key}`);
  }
  if (!Number.isInteger(args.port) || args.port < 0 || args.port > 65535) {
    throw new Error('--port must be an integer in [0, 65535]');
  }
  if (!args.journal) throw new Error('--journal is required');
  if (!args.readPath) throw new Error('--read-path is required（读腿种子文件的 data_home 相对路径）');
  if (!Number.isFinite(args.finalDelayMs) || args.finalDelayMs < 0 || args.finalDelayMs > 60000) {
    throw new Error('--final-delay-ms must be in [0, 60000]');
  }
  return args;
}

function sseFrame(res, payload) {
  res.write(`data: ${JSON.stringify(payload)}\n\n`);
}

function sendSse(res, frames) {
  res.writeHead(200, {
    'Content-Type': 'text/event-stream',
    'Cache-Control': 'no-cache',
    Connection: 'close',
  });
  for (const frame of frames) sseFrame(res, frame);
  res.write('data: [DONE]\n\n');
  res.end();
}

function toolActFrames(ident, tool, args) {
  return [
    {
      id: 'r08-t03-tool',
      choices: [{
        index: 0,
        delta: {
          role: 'assistant',
          tool_calls: [{
            index: 0,
            id: ident,
            type: 'function',
            function: { name: tool, arguments: JSON.stringify(args) },
          }],
        },
        finish_reason: null,
      }],
    },
    { id: 'r08-t03-tool', choices: [{ index: 0, delta: {}, finish_reason: 'tool_calls' }] },
  ];
}

function finalFrames(content) {
  return [
    {
      id: 'r08-t03-final',
      choices: [{
        index: 0,
        delta: { role: 'assistant', content },
        finish_reason: null,
      }],
    },
    {
      id: 'r08-t03-final',
      choices: [{ index: 0, delta: {}, finish_reason: 'stop' }],
      usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
    },
  ];
}

function nonStreamBody(frames) {
  // 把 SSE 幕序折叠成一次性 chat.completion：tool 幕保留 tool_calls，
  // 终结幕保留 content；usage 只在终结幕出现。
  const first = frames[0].choices[0];
  const message = { role: 'assistant' };
  if (first.delta.tool_calls) message.tool_calls = first.delta.tool_calls;
  if (typeof first.delta.content === 'string') message.content = first.delta.content;
  const finish = frames[frames.length - 1].choices[0].finish_reason;
  const body = {
    id: frames[0].id,
    object: 'chat.completion',
    choices: [{ index: 0, message, finish_reason: finish }],
  };
  const usage = frames[frames.length - 1].usage;
  if (usage) body.usage = usage;
  return body;
}

function lastUserText(messages) {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const m = messages[i];
    if (m && m.role === 'user') {
      if (typeof m.content === 'string') return m.content;
      if (Array.isArray(m.content)) {
        return m.content.map((c) => (c && typeof c.text === 'string' ? c.text : '')).join('\n');
      }
    }
  }
  return '';
}

function lastToolContent(messages) {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const m = messages[i];
    if (m && m.role === 'tool') {
      return typeof m.content === 'string' ? m.content : JSON.stringify(m.content);
    }
  }
  return '';
}

function decideAct(model, messages, readPath) {
  if (model !== 'chat') return { kind: 'final', ident: null, content: TITLE_ANSWER };
  const userText = lastUserText(messages);
  const toolCount = messages.filter((m) => m && m.role === 'tool').length;
  // A06 聊天腿：纯终结回答，回显 leg-token；零工具接触（不撞 PPG 面）。
  if (userText.includes(CHAT_MARKER)) {
    const m = userText.match(/A06-CHAT-TASK\s+([A-Za-z0-9._-]+)/);
    const leg = m ? m[1] : 'unknown-leg';
    return { kind: 'final', ident: null, content: `A06-CHAT-FINAL ${leg}` };
  }
  // 写腿：无条件请求未注册的 write —— PPG-1 红证据，Rust 侧 provider_error。
  if (userText.includes(WRITE_MARKER)) {
    return {
      kind: 'tool',
      ident: 'a05-call-write',
      tool: 'write',
      args: { path: WRITE_PATH, content: WRITE_CONTENT },
    };
  }
  // 读腿：先调已注册的 read，拿到工具回包后终结回答原样回显其内容。
  if (userText.includes(READ_MARKER)) {
    if (toolCount === 0) {
      return { kind: 'tool', ident: 'a05-call-read', tool: 'read', args: { path: readPath } };
    }
    return { kind: 'final', ident: null, content: `${FINAL_ANSWER} readback: ${lastToolContent(messages)}` };
  }
  return { kind: 'error', status: 400, reason: 'stub: prompt missing A05-READ-TASK / A05-WRITE-TASK marker' };
}

function main() {
  const args = parseArgs(process.argv);
  fs.mkdirSync(path.dirname(args.journal), { recursive: true });
  // 同步写（openSync 'ax' 独占 + writeSync）：编排器在写腿落定后立即读
  // journal 做工具面硬断言，异步流有 flush 竞态；'ax' 拒复用旧路径。
  const journalFd = fs.openSync(args.journal, 'ax');
  const jwrite = (entry) => fs.writeSync(journalFd, `${JSON.stringify(entry)}\n`);
  let seq = 0;

  const server = http.createServer((req, res) => {
    const url = new URL(req.url, 'http://127.0.0.1');
    if (req.method === 'GET' && url.pathname === '/info') {
      res.writeHead(200, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({
        schema: 'r08-t03-stub-provider/v2',
        readMarker: READ_MARKER,
        writeMarker: WRITE_MARKER,
        readPath: args.readPath,
        writePath: WRITE_PATH,
        finalAnswer: FINAL_ANSWER,
        requests: seq,
      }));
      return;
    }
    if (req.method !== 'POST' || url.pathname !== '/v1/chat/completions') {
      res.writeHead(404, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ error: 'unknown route' }));
      return;
    }
    const chunks = [];
    req.on('data', (chunk) => chunks.push(chunk));
    // 写腿会被 Rust 侧中途断开（provider_error），请求流 aborted 不视为替身故障。
    req.on('aborted', () => {
      jwrite({ at: new Date().toISOString(), event: 'request-aborted', seq });
    });
    req.on('end', () => {
      let body;
      try {
        body = JSON.parse(Buffer.concat(chunks).toString('utf8'));
      } catch {
        res.writeHead(400, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ error: 'malformed JSON body' }));
        return;
      }
      seq += 1;
      const model = typeof body.model === 'string' ? body.model : '';
      const messages = Array.isArray(body.messages) ? body.messages : [];
      const toolCount = messages.filter((m) => m && m.role === 'tool').length;
      const act = decideAct(model, messages, args.readPath);
      jwrite({
        seq,
        at: new Date().toISOString(),
        model,
        toolMessages: toolCount,
        stream: body.stream === true,
        act: act.kind === 'tool' ? act.tool : act.kind,
        request: body,
      });

      if (act.kind === 'error') {
        res.writeHead(act.status, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ error: act.reason }));
        return;
      }
      const frames = act.kind === 'tool'
        ? toolActFrames(act.ident, act.tool, act.args)
        : finalFrames(act.content);
      const respond = () => {
        if (res.writableEnded || res.destroyed) return;
        if (body.stream === true) {
          sendSse(res, frames);
        } else {
          res.writeHead(200, { 'Content-Type': 'application/json' });
          res.end(JSON.stringify(nonStreamBody(frames)));
        }
      };
      if (act.kind === 'final' && model === 'chat' && args.finalDelayMs > 0) {
        setTimeout(respond, args.finalDelayMs);
      } else {
        respond();
      }
    });
  });

  server.listen(args.port, '127.0.0.1', () => {
    const actual = server.address().port;
    process.stdout.write(`A05_STUB_READY ${actual}\n`);
  });

  const shutdown = () => {
    server.close(() => {
      fs.closeSync(journalFd);
      process.exit(0);
    });
    // 兜底：连接未排空也不挂住编排器。
    setTimeout(() => process.exit(0), 3000).unref();
  };
  process.on('SIGTERM', shutdown);
  process.on('SIGINT', shutdown);
}

main();
