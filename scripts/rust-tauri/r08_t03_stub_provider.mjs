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
 *   - 含 A06-CHAT-TASK <leg>：纯聊天腿，终结回答回显 leg。
 *   - 含 R08-TOOL-PLAN <token> <base64url{steps:[{tool,args}]}>：工具回合腿，
 *     按编排器给定计划逐步发 tool_calls（第 k 步在本轮已有 k 条 tool 结果时
 *     发出），耗尽后终结回答 "R08-TOOL-FINAL <token> steps=<n> last=<回包>"。
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
// 工具步响应的服务端固定延迟（--tool-delay-ms，默认 0）：给 WS 流面演练
// （steer/interject/stop）留出「run 进行中」的确定窗口；夹具确定性参数。
const TOOL_STEP_DELAY_DEFAULT_MS = 0;
// A06：纯聊天腿（零文件工具接触）。提示形状 "A06-CHAT-TASK <leg-token>"，
// 终结回答回显 leg-token，编排器据此区分 A 的第一次/第二次任务。
const CHAT_MARKER = 'A06-CHAT-TASK';
// R08 收尾工具回合腿：提示形状 "R08-TOOL-PLAN <token> <base64url(JSON)>"，
// JSON = {steps:[{tool,args}, ...]}。第 k 步在「最后一条 user 之后已有 k 条
// role:"tool" 结果」时发出（双内核都把同轮工具结果作为 role:"tool" 回传，
// 跨轮历史不计入）；步骤耗尽 → 终结回答 "R08-TOOL-FINAL <token> steps=<n>
// last=<最后一条工具回包原文>"。所有步骤由编排器显式给出（含双内核共同的
// mcp_describe_tool → mcp_call 按需目录协议），替身不做任何推断。
const TOOL_PLAN_MARKER = 'R08-TOOL-PLAN';
const TOOL_PLAN_FINAL = 'R08-TOOL-FINAL';
/** 曾在某次模型调用的 user 提交里看到 R08-STEER-MARK 的计划 token（进程内，演练级）。 */
const steerSeenTokens = new Set();
const WRITE_PATH = 'a05-drill/write-note.txt';
const WRITE_CONTENT = 'A05-WRITE-v1\n';
const FINAL_ANSWER = 'A05-FINAL-ANSWER';
const TITLE_ANSWER = 'A05 演练会话';

function parseArgs(argv) {
  const args = { port: 0, journal: null, finalDelayMs: 1500, readPath: null, toolDelayMs: TOOL_STEP_DELAY_DEFAULT_MS };
  for (let i = 2; i < argv.length; i += 1) {
    const key = argv[i];
    const value = argv[i + 1];
    if (key === '--port') { args.port = Number(value); i += 1; }
    else if (key === '--journal') { args.journal = path.resolve(value); i += 1; }
    else if (key === '--final-delay-ms') { args.finalDelayMs = Number(value); i += 1; }
    else if (key === '--tool-delay-ms') { args.toolDelayMs = Number(value); i += 1; }
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
  if (!Number.isFinite(args.toolDelayMs) || args.toolDelayMs < 0 || args.toolDelayMs > 10000) {
    throw new Error('--tool-delay-ms must be in [0, 10000]');
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

function textOf(m) {
  if (!m) return '';
  if (typeof m.content === 'string') return m.content;
  if (Array.isArray(m.content)) {
    return m.content.map((c) => (c && typeof c.text === 'string' ? c.text : '')).join('\n');
  }
  return '';
}

function lastUserText(messages) {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const m = messages[i];
    if (m && m.role === 'user') return textOf(m);
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

/** 最后一条 user 之后的 role:"tool" 结果条数（本轮已完成的计划步数）。 */
function toolResultsSinceLastUser(messages) {
  let lastUser = -1;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    if (messages[i] && messages[i].role === 'user') { lastUser = i; break; }
  }
  let n = 0;
  for (let i = lastUser + 1; i < messages.length; i += 1) {
    if (messages[i] && messages[i].role === 'tool') n += 1;
  }
  return n;
}

/**
 * 解析 R08-TOOL-PLAN 标记：返回 {token, steps} 或 {error}。
 * base64url 载荷只含 [A-Za-z0-9_-]，不受内核对 user 文本的包裹/追加影响。
 */
function parseToolPlan(userText) {
  const m = userText.match(/R08-TOOL-PLAN\s+([A-Za-z0-9._-]+)\s+([A-Za-z0-9_-]+)/);
  if (!m) return { error: 'R08-TOOL-PLAN marker malformed (expect: token base64url)' };
  let plan;
  try {
    plan = JSON.parse(Buffer.from(m[2], 'base64url').toString('utf8'));
  } catch (error) {
    return { error: `R08-TOOL-PLAN payload is not base64url JSON: ${error.message}` };
  }
  const steps = Array.isArray(plan?.steps) ? plan.steps : null;
  if (!steps || steps.length === 0 || steps.length > 32) {
    return { error: 'R08-TOOL-PLAN steps must be a non-empty array (<=32)' };
  }
  for (const [i, step] of steps.entries()) {
    if (!step || typeof step.tool !== 'string' || !/^[A-Za-z0-9_-]{1,64}$/.test(step.tool)) {
      return { error: `R08-TOOL-PLAN step[${i}].tool must be a wire-safe tool name` };
    }
    if (step.args !== undefined && (typeof step.args !== 'object' || step.args === null || Array.isArray(step.args))) {
      return { error: `R08-TOOL-PLAN step[${i}].args must be an object` };
    }
  }
  return { token: m[1], steps };
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
  // R08 工具回合腿：按编排器给定的计划逐步发工具调用，步骤耗尽回终结。
  if (userText.includes(TOOL_PLAN_MARKER)) {
    const plan = parseToolPlan(userText);
    if (plan.error) return { kind: 'error', status: 400, reason: `stub: ${plan.error}` };
    // steer 到达记录：Rust 只在排空 steering 信箱的那一次模型调用里把文本追加到
    // 同一条 user 提交（后续调用不再携带），按计划 token 记住「本计划曾看到 steer」，
    // 终结时回显；旧栈把 steer 作为新 user 消息插入，走下方 CONT 分支直接可见。
    if (messages.some((m) => m && m.role === 'user' && textOf(m).includes('R08-STEER-MARK'))) {
      steerSeenTokens.add(plan.token);
    }
    const done = toolResultsSinceLastUser(messages);
    if (done < plan.steps.length) {
      const step = plan.steps[done];
      return {
        kind: 'tool',
        ident: `r08-plan-${plan.token}-${done}`,
        tool: step.tool,
        args: step.args ?? {},
      };
    }
    const last = lastToolContent(messages);
    // 回显截断在延迟内容阈值之下，保证终结回答在双内核历史投影中内联可见。
    const steerSeen = steerSeenTokens.has(plan.token) ? ' R08-STEER-SEEN' : '';
    return {
      kind: 'final',
      ident: null,
      content: `${TOOL_PLAN_FINAL} ${plan.token} steps=${plan.steps.length}${steerSeen} last=${last.slice(0, 1200)}`,
    };
  }
  // 工具回合会话的内核自发续轮（如延迟回退结果回送后的 trigger_parent_turn /
  // 延迟结果插话 / Pi steer 插入的 user 消息）：最后一条 user 无标记但会话早先含
  // R08-TOOL-PLAN → 给出显式终结回答（journal 记 act=final，内容带 CONT 标识），
  // 不算静默兜底。
  if (messages.some((m) => m && m.role === 'user' && textOf(m).includes(TOOL_PLAN_MARKER))) {
    const steerSeen = userText.includes('R08-STEER-MARK') ? ' R08-STEER-SEEN' : '';
    return { kind: 'final', ident: null, content: `R08-TOOL-CONT${steerSeen} ${userText.slice(0, 200).replace(/\s+/g, ' ')}` };
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
  return { kind: 'error', status: 400, reason: 'stub: prompt missing A05-READ-TASK / A05-WRITE-TASK / A06-CHAT-TASK / R08-TOOL-PLAN marker' };
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
        schema: 'r08-t03-stub-provider/v3',
        readMarker: READ_MARKER,
        writeMarker: WRITE_MARKER,
        chatMarker: CHAT_MARKER,
        toolPlanMarker: TOOL_PLAN_MARKER,
        toolPlanFinal: TOOL_PLAN_FINAL,
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
      } else if (act.kind === 'tool' && args.toolDelayMs > 0) {
        setTimeout(respond, args.toolDelayMs);
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
