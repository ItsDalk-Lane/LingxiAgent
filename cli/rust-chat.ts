import readline from "node:readline";
import { randomUUID } from "node:crypto";
import type { Readable, Writable } from "node:stream";
import WebSocket from "ws";
import { RustCliClient, safeRustTerminalText, type RustClientContext, type RustSession } from "./rust-service.ts";

export function selectRustSession(sessions: RustSession[], target?: unknown): RustSession | undefined {
  const value = String(target || "").trim();
  if (!value) return sessions[0];
  if (/^[1-9][0-9]*$/.test(value)) return sessions[Number(value) - 1];
  return sessions.find(session => session.sessionId === value || `native:${session.sessionId}` === value);
}
export function rustFrameMatches(sessionId: string, frame: any): boolean {
  return frame?.sessionId === sessionId && (frame.sessionPath === undefined || frame.sessionPath === `native:${sessionId}`);
}
function safeOutput(value: unknown): string {
  // 保留正文换行；任何远端控制序列都不能变成终端指令。
  return String(value ?? "").replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "").replace(/\x1b\][^\x07]*(?:\x07|\x1b\\)/g, "").replace(/[\x00-\x08\x0b-\x1f\x7f-\x9f]/g, "");
}
export async function printRustStatus(client: RustCliClient): Promise<number> {
  const health = await client.health();
  // 身份面失败不阻断健康/版本展示，也不虚构认证来源；只如实标注并判不完整。
  let identity: any = null;
  let identityFailed = false;
  try { identity = await client.identity(); } catch { identityFailed = true; }
  let context: RustClientContext | null = null;
  if (!identityFailed) {
    try { context = await client.context(); } catch { context = null; }
  }
  console.log("LingxiAgent Rust service");
  console.log(`  URL       ${safeRustTerminalText(client.baseUrl)}`);
  console.log(`  Version   ${safeRustTerminalText(health.serverVersion)}`);
  console.log(`  Studio    ${safeRustTerminalText(identity?.studioId || "unavailable")}`);
  console.log(`  Agent     ${safeRustTerminalText(context?.agentName || context?.currentAgentId || "not configured")}`);
  console.log(`  Model     ${context?.currentModel ? safeRustTerminalText(`${context.currentModel.provider}/${context.currentModel.id}`) : (context?.modelState || "unavailable")}`);
  console.log(`  Auth      ${identityFailed ? "unavailable (identity check failed)" : safeRustTerminalText(identity?.credentialKind)}`);
  if (identityFailed || !identity?.studioId || !context?.currentAgentId || context?.modelState !== "ready" || !context?.currentModel) {
    console.error("Rust status is incomplete: Agent or model is not available");
    return 1;
  }
  return 0;
}

interface ChatOptions {
  target?: string;
  session?: string;
  plain?: boolean;
  input?: Readable & { isTTY?: boolean };
  output?: Writable;
}
export async function startRustChat(client: RustCliClient, options: ChatOptions = {}): Promise<number> {
  await client.health();
  await client.identity();
  const sessions = await client.sessions();
  const requested = options.target || options.session;
  let session = selectRustSession(sessions, requested);
  if (requested && !session) throw new Error(`Session not found: ${requested}`);
  let context: RustClientContext;
  if (!session) {
    context = await client.context();
    if (!context.currentAgentId || context.modelState !== "ready") throw new Error("Configure an Agent and chat model before opening CLI chat");
    session = await client.newSession(context.currentAgentId);
  } else {
    const selected = await client.session(session.sessionId);
    if (selected.sessionId !== session.sessionId || typeof selected.agentId !== "string") throw new Error("Selected Rust session identity is invalid");
    session = { ...session, agentId: selected.agentId };
    // 继续既有会话时按该会话身份读上下文：全局上下文可能是另一个助手的焦点，
    // 用它显示会报出错误的助手/模型（T09 断点6）。
    context = await client.context({ sessionId: session.sessionId });
  }
  const input = options.input || process.stdin;
  const output = options.output || process.stdout;
  const terminal = input.isTTY === true && !options.plain;
  const write = (value: unknown) => output.write(safeOutput(value));
  let socket: WebSocket | null = null;
  let socketEpoch = 0;
  let ended = false;
  let ready = false;
  let eof = false;
  let exitCode = 0;
  let connecting = false;
  let attempts = 0;
  let stream: string | null = null;
  let cursor = 0;
  let streaming = false;
  let pending: { id: string; accepted: boolean } | null = null;
  let abortWhenKnown = false;
  let abortRequested: string | null = null;
  let resumePending = false;
  let settling = false;
  let commandBusy = false;
  const lines: string[] = [];
  const buffer = new Map<number, any>();
  const printed = new Set<string>();
  const printedTools = new Set<string>();
  const completedRuns = new Set<string>();
  const outputByRun = new Map<string, string>();
  const shownApprovals = new Set<string>();
  let frameChain = Promise.resolve();
  let pollBusy = false;
  let settleTimer: ReturnType<typeof setTimeout> | undefined;
  let settleDeadline = 0;
  let resumeTimer: ReturnType<typeof setTimeout> | undefined;
  let resolveDone!: (code: number) => void;
  const done = new Promise<number>(resolve => { resolveDone = resolve; });
  const rl = readline.createInterface({ input, output, terminal });
  // 输入回调先登记，网络握手期间输入也只排队，不提前投递。
  rl.on("line", line => { void incoming(line); });
  rl.on("close", () => { eof = true; maybeExit(); });
  const prompt = () => { if (terminal && !eof && ready && !streaming && !pending) write("you › "); };
  const finish = (code: number) => {
    if (ended) return;
    ended = true; ready = false; exitCode = Math.max(exitCode, code);
    clearInterval(approvalTimer);
    if (settleTimer) clearTimeout(settleTimer);
    if (resumeTimer) clearTimeout(resumeTimer);
    process.off("SIGINT", interrupt);
    process.off("SIGTERM", terminate);
    rl.removeListener("SIGINT", interrupt);
    input.removeListener("keypress", keypress);
    rl.close();
    if (socket) { socket.removeAllListeners(); socket.on("error", () => {}); socket.terminate(); }
    resolveDone(exitCode);
  };
  const fail = (error: unknown) => { write(`\n${client.redact(error instanceof Error ? error.message : error)}\n`); finish(1); };
  function maybeExit() {
    if (!eof || lines.length || !ready || pending || streaming || settling || commandBusy) return;
    // EOF 时若仍有未确认终态的任务，不能用 0 退出伪装成已结束；保持真实 pending 可见。
    finish(exitCode);
  }
  function send(frame: any) {
    if (!socket || socket.readyState !== WebSocket.OPEN) throw new Error("Rust wire is not ready; input was not resent");
    socket.send(JSON.stringify(frame));
  }
  function requestResume() {
    if (resumePending) return;
    resumePending = true;
    // 恢复响应自身没有服务端截止：给一个真实超时，EOF/断线才不会挂住（断点2）。
    if (resumeTimer) clearTimeout(resumeTimer);
    resumeTimer = setTimeout(() => { resumePending = false; maybeExit(); }, 15_000);
    send({ type: "resume_stream", sessionId: session!.sessionId, sessionPath: `native:${session!.sessionId}`, ...(stream ? { streamId: stream } : {}), sinceSeq: cursor, resumeToken: randomUUID() });
  }
  function requestAbort() {
    if (!streaming && !pending) { finish(0); return; }
    if (!stream) { abortWhenKnown = true; write("\nStop queued until this stream is known.\n"); return; }
    if (abortRequested === stream) return;
    send({ type: "abort", sessionId: session!.sessionId, sessionPath: `native:${session!.sessionId}`, streamId: stream, reason: "cli_user_abort" });
    abortRequested = stream; write("\nStop requested; waiting for the actual task to settle.\n");
  }
  function interrupt() { try { requestAbort(); } catch (error) { fail(error); } }
  function terminate() { write(`\nDetached from ${session!.sessionId}; the task outcome must be checked with continue.\n`); finish(143); }
  function keypress(_text: unknown, key: any) { if (key?.name === "escape" && (streaming || pending)) interrupt(); }
  process.on("SIGINT", interrupt); process.on("SIGTERM", terminate); rl.on("SIGINT", interrupt); input.on("keypress", keypress);

  function renderHistory(history: any) {
    for (const row of history.items) {
      if (!row || typeof row.kind !== "string") throw new Error("Rust canonical history item is invalid");
      const run = row.run?.state === "known" && typeof row.run.runId === "string" ? row.run.runId : null;
      const id = row.messageId || row.segmentId || row.eventId;
      if (row.kind === "user_message") {
        if (pending && row.legacyFields?.clientMessageId === pending.id) pending.accepted = true;
        if (typeof row.text !== "string") throw new Error("Rust user history text is invalid");
        if (id && !printed.has(id)) { printed.add(id); write(`\nyou: ${row.text}\n`); }
      }
      // 恢复/重连后补齐工具执行事实：live 已展示的按 toolCallId 去重（断点 7）。
      if (row.kind === "tool_call") {
        const toolId = typeof row.toolCallId === "string" ? row.toolCallId : null;
        if (toolId && !printedTools.has(toolId) && typeof row.target === "string" && typeof row.status === "string") {
          printedTools.add(toolId);
          write(`◇ ${safeRustTerminalText(row.target)} ${safeRustTerminalText(row.status)}\n`);
        }
      }
      if (row.kind === "final_message" && run && Array.isArray(row.content)) {
        const text = row.content.filter((block: any) => block.type === "text").map((block: any) => block.text).join("");
        if (!completedRuns.has(run)) {
          const live = outputByRun.get(run) || "";
          if (text.startsWith(live)) write(text.slice(live.length));
          else write(`\n[restored final reply]\n${text}`);
          if (text) write("\n");
          outputByRun.set(run, text); completedRuns.add(run);
          // resource_ref 只在首次呈现该 run 时打印，不能每次对账重复显示（断点 7）。
          for (const block of row.content) if (block.type === "resource_ref" && block.resource?.resourceId) write(`[resource ${safeRustTerminalText(block.resource.resourceId)}]\n`);
        }
      }
      // assistant_segment 与 final_message 同源同文，渲染它会与最终答复重复，跳过。
    }
  }
  async function settle() {
    if (settling || ended) return;
    settling = true;
    try {
      const selectedId = session!.sessionId;
      const epochAtRead = socketEpoch;
      const history = await client.history(selectedId);
      // 旧代次的迟到读回不得覆盖切换/重连后的会话（T09 断点5）。
      if (ended || selectedId !== session!.sessionId || epochAtRead !== socketEpoch) return;
      renderHistory(history);
      if (history.reconciliation.runStatus !== "reconciled_idle") {
        if (Date.now() >= settleDeadline) throw new Error(`Task cleanup is not confirmed; continue ${selectedId} to inspect it`);
        settleTimer = setTimeout(() => { settleTimer = undefined; settling = false; void settle().catch(fail); }, 100);
        return;
      }
      if (pending && !pending.accepted) throw new Error("Prompt acceptance is unknown after reconnect; it was not resent");
      pending = null; streaming = false; abortRequested = null; abortWhenKnown = false; settleDeadline = 0;
      settling = false;
      write("\n");
      // 终态后必须继续推动输入队列：第一轮结束不推，第二行会永久卡住（断点1）。
      prompt(); void pump();
    } finally {
      // 仅当没有排定的后续对账时才落回非 settling；有定时器时由定时器回调复位。
      if (!settleTimer && settling) settling = false;
      maybeExit();
    }
  }
  async function event(frame: any) {
    if (frame.type === "session_user_message" && pending && frame.clientMessageId === pending.id) pending.accepted = true;
    if (frame.streamId && frame.streamId !== stream) return;
    if (frame.type === "status" && frame.isStreaming === true) streaming = true;
    if (frame.type === "assistant_segment_delta" && frame.kind !== "reasoning" && frame.semanticPhase !== "reasoning" && typeof frame.delta === "string" && stream && !completedRuns.has(stream)) {
      write(frame.delta); outputByRun.set(stream, (outputByRun.get(stream) || "") + frame.delta);
    } else if (frame.type === "text_delta" && typeof frame.delta === "string" && stream && !completedRuns.has(stream)) {
      write(frame.delta); outputByRun.set(stream, (outputByRun.get(stream) || "") + frame.delta);
    } else if (frame.type === "tool_start") write(`\n◇ ${safeRustTerminalText(frame.name || "tool")}\n`);
    else if (frame.type === "tool_end") {
      write(`◇ ${safeRustTerminalText(frame.name || "tool")} ${safeRustTerminalText(frame.status || (frame.success ? "succeeded" : "failed"))}\n`);
      if (typeof frame.id === "string" && frame.id) printedTools.add(frame.id);
    }
    else if (frame.type === "error") { exitCode = 1; write(`\n${client.redact(frame.message || frame.error?.message || "Rust task failed")}\n`); }
    if (frame.type === "status" && frame.isStreaming === false) {
      settleDeadline ||= Date.now() + 30_000;
      void settle().catch(fail);
    }
    if (abortWhenKnown && stream && (streaming || pending)) requestAbort();
  }
  async function sequenced(frame: any) {
    if (typeof frame.streamId !== "string" || !Number.isSafeInteger(frame.seq) || frame.seq < 1) throw new Error("Rust stream frame lacks a valid sequence or stream identity");
    if (frame.streamId !== stream) {
      if (streaming && stream) return;
      stream = frame.streamId; cursor = 0; buffer.clear();
    }
    if (frame.seq <= cursor) return;
    buffer.set(frame.seq, frame);
    if (buffer.size > 4096) throw new Error("Rust stream gap exceeds bounded replay capacity");
    while (buffer.has(cursor + 1)) {
      const next = buffer.get(cursor + 1); buffer.delete(cursor + 1); cursor += 1;
      await event(next);
    }
    if (buffer.size && !resumePending) requestResume();
  }
  async function frame(frame: any) {
    if (ended) return;
    if (!frame || typeof frame !== "object") throw new Error("Invalid Rust wire frame");
    if (frame.code && !frame.sessionId && !frame.type) throw new Error(frame.message || "Rust wire rejected request");
    if (!rustFrameMatches(session!.sessionId, frame)) return;
    if (frame.type === "input_rejected") {
      if (!pending || frame.clientMessageId !== pending.id) return;
      pending = null; exitCode = 1;
      write(`\nInput not accepted: ${client.redact(frame.message || frame.code)}\n`); void pump(); maybeExit(); return;
    }
    if (frame.type === "abort_result") {
      if (frame.status === "rejected") { abortRequested = null; write(`\nStop rejected: ${client.redact(frame.reason)}\n`); }
      return;
    }
    if (frame.type === "stream_resume") {
      resumePending = false;
      if (resumeTimer) { clearTimeout(resumeTimer); resumeTimer = undefined; }
      if (!Array.isArray(frame.events) || typeof frame.isStreaming !== "boolean" || typeof frame.runtimeIsStreaming !== "boolean") throw new Error("Rust resume response is incomplete");
      if (!Number.isSafeInteger(frame.nextSeq) || frame.nextSeq < 1) throw new Error("Rust resume response lacks a valid nextSeq");
      for (const replay of frame.events) {
        const inner = replay?.event;
        if (!inner || !Number.isSafeInteger(replay.seq) || replay.seq < 1 || !Number.isSafeInteger(inner?.seq) || inner.seq !== replay.seq) {
          throw new Error("Rust resume replay frame sequence is invalid");
        }
      }
      if (frame.reset || frame.truncated) {
        renderHistory(await client.history(session!.sessionId));
        stream = typeof frame.streamId === "string" ? frame.streamId : null;
        cursor = frame.events.length ? frame.events[0].seq - 1 : Math.max(0, frame.nextSeq - 1); buffer.clear();
      } else if (typeof frame.streamId === "string" && frame.streamId) {
        // 无重放也不重置时，正在运行的任务身份以响应的 streamId 为准（断点3）。
        stream = frame.streamId;
      }
      for (const replay of frame.events) await sequenced(replay.event);
      ready = true; streaming = frame.isStreaming || frame.runtimeIsStreaming;
      if (!streaming && pending) { settleDeadline ||= Date.now() + 30_000; void settle().catch(fail); }
      else if (!streaming) { void pump(); maybeExit(); }
      prompt(); return;
    }
    if (Number.isSafeInteger(frame.seq)) await sequenced(frame);
    else if (frame.type === "error") throw new Error(frame.message || "Rust chat request rejected");
  }
  async function connect() {
    if (ended || connecting) return;
    connecting = true; ready = false;
    const epoch = ++socketEpoch;
    try {
      const next = await client.openWire(value => {
        if (epoch !== socketEpoch || ended) return;
        frameChain = frameChain.then(() => frame(value)).catch(fail);
      }, error => {
        if (epoch !== socketEpoch || ended) return;
        socket = null; connecting = false; ready = false;
        if (++attempts > 3) { fail(error); return; }
        write(`\n${client.redact(error.message)}; reconnecting to resume without resending input.\n`);
        void connect();
      });
      if (ended || epoch !== socketEpoch) { next.terminate(); return; }
      socket = next; connecting = false; attempts = 0; resumePending = false; requestResume();
    } catch (error) { connecting = false; fail(error); }
  }
  async function command(line: string) {
    const [name, ...parts] = line.slice(1).trim().split(/\s+/);
    if (["q", "quit", "exit"].includes(name)) { write(`\nDetached from ${session!.sessionId}.\n`); finish(0); return; }
    if (name === "help" || name === "h") { write("/sessions  /continue <index|id>  /new  /status  /stop  /approve <id>  /reject <id>  /quit\n"); return; }
    if (name === "stop") { requestAbort(); return; }
    if (name === "approve" || name === "reject") {
      const approvals = await client.approvals(session!.sessionId);
      const approval = parts[0] ? approvals.find(row => row.approvalId === parts[0]) : approvals.length === 1 ? approvals[0] : undefined;
      if (!approval || (stream && approval.runId !== stream)) throw new Error("No unique pending approval belongs to the current task");
      const result = await client.answerApproval(session!.sessionId, approval.approvalId, name === "approve" ? "confirmed" : "rejected");
      if (result.ok !== true) throw new Error("Rust approval was not confirmed");
      write(`Approval ${result.decision}: ${safeRustTerminalText(approval.approvalId)}\n`); return;
    }
    if (name === "status") { exitCode = Math.max(exitCode, await printRustStatus(client)); return; }
    if (name === "sessions") {
      for (const [index, row] of (await client.sessions()).slice(0,20).entries()) write(`${index+1}. ${safeRustTerminalText(row.title || row.sessionId)} · ${safeRustTerminalText(row.agentId)} · ${row.sessionId}\n`);
      return;
    }
    if (streaming || pending) throw new Error("Wait for the current reply or stop it before switching sessions");
    if (name === "continue" || name === "new") {
      const next = name === "new" ? await client.newSession((await client.context()).currentAgentId || "") : selectRustSession(await client.sessions(), parts.join(" "));
      if (!next) throw new Error(`Session not found: ${parts.join(" ")}`);
      const validated = await client.session(next.sessionId);
      session = { ...next, agentId: validated.agentId };
      stream = null; cursor = 0; pending = null; buffer.clear(); resumePending = false;
      if (resumeTimer) { clearTimeout(resumeTimer); resumeTimer = undefined; }
      renderHistory(await client.history(session.sessionId)); requestResume(); return;
    }
    throw new Error(`Unknown command: /${name}`);
  }
  async function incoming(line: string) {
    if (ended || !line.trim()) return;
    if (line.startsWith("/")) {
      commandBusy = true;
      try { await command(line); } catch (error) { write(`\n${client.redact(error instanceof Error ? error.message : error)}\n`); exitCode = 1; }
      finally { commandBusy = false; void pump(); maybeExit(); prompt(); }
    } else { lines.push(line); if (lines.length > 256) { fail(new Error("CLI input queue exceeds256 lines")); return; } void pump(); }
  }
  async function pump() {
    if (ended || !ready || connecting || settling || commandBusy || streaming || pending) return;
    const text = lines.shift();
    if (!text) { maybeExit(); return; }
    const id = randomUUID(); pending = { id, accepted: false };
    send({ type: "prompt", sessionId: session!.sessionId, sessionPath: `native:${session!.sessionId}`, agentId: session!.agentId, text, clientMessageId: id, snapshotVersion: 1, clientAttemptId: randomUUID() });
  }
  const approvalTimer = setInterval(() => {
    if (ended || !ready || (!pending && !streaming) || pollBusy) return;
    pollBusy = true;
    const target = session!.sessionId;
    void client.approvals(target).then(rows => {
      if (ended || session!.sessionId !== target) return;
      for (const approval of rows) {
        if (stream && approval.runId !== stream || shownApprovals.has(approval.approvalId)) continue;
        shownApprovals.add(approval.approvalId);
        write(`\nApproval required ${safeRustTerminalText(approval.approvalId)}: ${safeRustTerminalText(approval.target)} ${client.redact(approval.summary || "")}\nUse /approve <id> or /reject <id>.\n`);
        if (eof) { write("Input ended without an approval decision; task remains pending and was not approved.\n"); finish(2); }
      }
    }).catch(fail).finally(() => { pollBusy = false; });
  }, 500);
  try {
    renderHistory(await client.history(session.sessionId));
    write(`LingxiAgent · ${context.agentName || session.agentId}\nSession ${session.sessionId} · native:${session.sessionId}\n`);
    await connect();
  } catch (error) { fail(error); }
  return done;
}
