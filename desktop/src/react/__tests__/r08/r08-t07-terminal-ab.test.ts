/** @vitest-environment jsdom */
/**
 * R08-T07｜D13 终端会话子系统 A/B 套件（独立套件，drill 以 --suite-files 单跑）。
 *
 * 目标：给 R08 三叶拿到**双内核真实正路径**逐断言证据（替代 earlier recorded）：
 *   R00-T02-LA-CE69063550AA terminal snapshot（A0 送当前终端快照 / A1 身份不符拒绝、不泄露他人终端）
 *   R00-T02-LA-4C35E6AEC6F7 terminal tail（A0 按 terminalId/sinceSeq 返回后续输出 / A1 无权/不匹配拒绝）
 *   R00-T02-LA-CFD9F02BC6AA terminal close（A0 终止并返回 killed|already_stopped（真实进程终止）/ A1 rejected 不谎报）
 *
 * 双腿同一份客户端边界：真实 store 连接（session-actions 同款构造）+ 真实
 * `services/websocket`（服务层帧捕获）+ 真实 exec_command/write_stdin 工具回合
 * （R08-TOOL-PLAN 替身计划；PTY 由内核真实创建）。
 *
 * 观测：observe('leaf.terminal.positive') → cases{...}；测试文件只如实记录，
 * 平价由 drill 比较器判（双腿 cases 同键全 true 才 parity）。
 *
 * 口径披露：驱动层级 = UI 绑定层（WS 客户端 + store 连接）/ 工具面，非像素级 DOM。
 * 网络边界：内核地址必须 http://127.0.0.1:<port>（环回强校验，否则整套拒跑）。
 */
import { afterAll, beforeAll, describe, it } from 'vitest';
import { writeFileSync } from 'node:fs';
import { useStore } from '../../stores';
import { createLocalServerConnection, upsertServerConnection } from '../../services/server-connection';
import { lingxiFetch } from '../../hooks/use-hana-fetch';
import { loadSessions } from '../../stores/session-actions';

const loopbackOrigin = (raw: string | undefined): string | null => {
  if (!raw) return null;
  let url: URL;
  try { url = new URL(raw); } catch { return null; }
  if (url.protocol !== 'http:' || url.hostname !== '127.0.0.1' || !url.port) return null;
  if (url.pathname !== '/' || url.search || url.hash || url.username || url.password) return null;
  return `http://127.0.0.1:${url.port}`;
};
const origin = loopbackOrigin(process.env.LINGXI_R08_AB_BASE);
const token = process.env.LINGXI_R08_AB_TOKEN;
const kind = process.env.LINGXI_R08_AB_KIND;
const out = process.env.LINGXI_R08_AB_OUT;
const enabled = Boolean(origin && token && (kind === 'node' || kind === 'rust') && out);

type Observation = { ok: boolean; observed?: unknown; error?: string; ms: number };
const observations: Record<string, Observation> = {};
async function observe(task: string, fn: () => Promise<unknown>) {
  const started = Date.now();
  try {
    const observed = await fn();
    observations[task] = { ok: true, observed, ms: Date.now() - started };
  } catch (error) {
    observations[task] = { ok: false, error: error instanceof Error ? error.message : String(error), ms: Date.now() - started };
  }
}
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

describe.skipIf(!enabled)(`R08-T07 terminal A/B [${kind}]`, () => {
  beforeAll(async () => {
    const port = Number(new URL(origin!).port);
    window.t = ((key: string) => key) as typeof window.t;
    window.i18n = { locale: 'en', defaultName: 'Lingxi', _data: {}, _agentOverrides: {}, load: async () => {}, setAgentOverrides: () => {}, t: window.t } as any;
    window.platform = {
      getServerConnectionInfo: async () => ({ port, token, ...(kind === 'rust' ? { serverNodeKind: 'lingxi-service', serverNodeTransport: 'http' } : {}) }),
      getPlatform: async () => 'darwin', onSettingsChanged: () => () => {}, onServerRestarted: () => () => {},
    } as unknown as typeof window.platform;
    localStorage.clear();
    const connection = createLocalServerConnection({
      serverPort: port, serverToken: token,
      serverNodeKind: kind === 'rust' ? 'lingxi-service' : null,
      serverNodeTransport: 'http',
    });
    if (!connection) throw new Error('connection not constructible');
    useStore.setState({
      serverPort: String(port), serverToken: token,
      serverConnections: upsertServerConnection({}, connection),
      activeServerConnectionId: connection.connectionId,
      activeServerConnection: connection,
    } as any);
    // Rust 迁移根 agent 名册补偿（T06 手册：Register 不随迁；正式切换经正式入口
    // 补建同 id agent——与主演习同口径，只补 Rust 腿）。
    if (kind === 'rust') {
      const agents = await lingxiFetch('/lingxi/v1/agents', { method: 'GET', throwOnHttpError: false });
      const list = await agents.json().catch(() => null);
      const rows = Array.isArray(list?.agents) ? list.agents : Array.isArray(list) ? list : [];
      if (!rows.some((a: any) => a?.id === 'a1')) {
        await lingxiFetch('/lingxi/v1/agents', {
          method: 'POST', headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ name: 'A1', id: 'a1', yuan: 'lingxi' }), throwOnHttpError: false,
        });
      }
    }
  });

  afterAll(() => {
    if (out) writeFileSync(out, `${JSON.stringify({ kind, origin, observations }, null, 2)}\n`);
  });

  // ── 客户端边界助手（与主演习同构但独立：本套件不依赖主演习的任何状态） ──
  const jsonFetch = async (path: string, init?: { method?: string; body?: unknown }) => {
    const res = await lingxiFetch(path, {
      method: init?.method ?? 'GET',
      ...(init?.body !== undefined ? { headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(init.body) } : {}),
      throwOnHttpError: false,
    });
    const body = await res.json().catch(() => null);
    return { status: res.status, body };
  };
  const rustOrNode = (rustPath: string, nodePath: string) => (kind === 'rust' ? rustPath : nodePath);
  const sessionLocator = (s: { sessionId: string; path: string }) => (kind === 'rust' ? `native:${s.sessionId}` : s.path);
  type ToolSession = { sessionId: string; path: string };
  type ToolCall = { tool: string; args?: Record<string, unknown>; direct?: boolean };

  // ── 真实 WS 服务：jsdom Event 兼容 shim（主演习 run-10/11 实证的同一方案） ──
  const wsFrames: string[] = [];
  const waitUntil = async (fn: () => boolean, label: string, budgetMs = 30000) => {
    const deadline = Date.now() + budgetMs;
    while (Date.now() < deadline) {
      if (fn()) return true;
      await sleep(200);
    }
    throw new Error(`timeout: ${label}`);
  };
  const ensureWsConnected = async () => {
    const wsMod = await import('../../services/websocket');
    if (!((window as any).WebSocket as any)?.__t08TerminalShim) {
      const wsModuleSpecifier = 'ws';
      const { WebSocket: NodeWs } = (await import(wsModuleSpecifier)) as unknown as { WebSocket: any };
      class WebSocketShim {
        static readonly CONNECTING = 0; static readonly OPEN = 1; static readonly CLOSING = 2; static readonly CLOSED = 3;
        static __t08TerminalShim = true;
        readyState = 0;
        url: string;
        onopen: any = null; onclose: any = null; onmessage: any = null; onerror: any = null;
        #inner: any;
        constructor(url: string) {
          this.url = url;
          this.#inner = new NodeWs(url, { handshakeTimeout: 15000 });
          this.#inner.on('open', () => { this.readyState = 1; this.onopen?.(new window.Event('open')); });
          this.#inner.on('message', (data: any) => {
            const text = Buffer.isBuffer(data) ? data.toString('utf8') : Array.isArray(data) ? Buffer.concat(data).toString('utf8') : String(data);
            this.onmessage?.(new window.MessageEvent('message', { data: text }));
          });
          this.#inner.on('close', (code: number, reason: Buffer) => {
            this.readyState = 3;
            this.onclose?.(new window.CloseEvent('close', { code, reason: reason?.toString('utf8') ?? '', wasClean: true }));
          });
          this.#inner.on('error', () => { this.onerror?.(new window.Event('error')); });
        }
        get protocol() { return ''; }
        send(data: any) { this.#inner?.send(data); }
        close(code?: number, reason?: string) {
          this.readyState = 2;
          try { this.#inner?.close(code, reason); } catch { this.#inner?.terminate(); }
        }
      }
      (window as any).WebSocket = WebSocketShim;
    }
    const alive = () => {
      const ws = wsMod.getWebSocket() as any;
      return useStore.getState().wsState === 'connected' && Boolean(ws && ws.readyState === WebSocket.OPEN);
    };
    if (!alive()) wsMod.connectWebSocket();
    await waitUntil(alive, 'ws connected', 25000);
    const socket = wsMod.getWebSocket() as any;
    if (socket && !socket.__t08TerminalHooked) {
      socket.__t08TerminalHooked = true;
      const origMessage = socket.onmessage?.bind(socket);
      socket.onmessage = (ev: any) => { wsFrames.push(String(ev.data)); origMessage?.(ev); };
      const origSend = socket.send?.bind(socket);
      socket.send = (data: any) => { wsFrames.push(`>>> ${String(data)}`); return origSend(data); };
    }
  };
  const parseFrame = (raw: string): any => { try { return JSON.parse(raw); } catch { return null; } };
  const sendFrame = async (frame: Record<string, unknown>) => {
    await ensureWsConnected();
    const wsMod = await import('../../services/websocket');
    const ws = wsMod.getWebSocket() as any;
    if (!ws || ws.readyState !== WebSocket.OPEN) throw new Error('ws not open');
    const start = wsFrames.length;
    ws.send(JSON.stringify(frame));
    return start;
  };
  /** 等本窗口内首个满足谓词的服务端帧（不含 `>>> ` 客户端回显）。 */
  const waitServerFrame = async (start: number, predicate: (parsed: any, raw: string) => boolean, budgetMs = 12000) => {
    const deadline = Date.now() + budgetMs;
    while (Date.now() < deadline) {
      const frames = wsFrames.slice(start).filter((raw) => !raw.startsWith('>>> '));
      for (const raw of frames) {
        const parsed = parseFrame(raw);
        if (parsed && predicate(parsed, raw)) return { parsed, raw };
      }
      await sleep(150);
    }
    return null;
  };
  const serverFramesSince = (start: number) => wsFrames.slice(start).filter((raw) => !raw.startsWith('>>> ')).map((raw) => ({ raw, parsed: parseFrame(raw) }));

  // ── 工具回合：R08-TOOL-PLAN（替身供应商按计划逐步发 tool_calls） ──
  const b64url = (s: string) => Buffer.from(s, 'utf8').toString('base64url');
  const planText = (planToken: string, calls: ToolCall[]) => {
    // direct：常驻工具（read/write/edit/exec_command）直接按名调用；write_stdin
    // 在双内核都走同一按需目录协议（mcp_describe_tool → mcp_call）。
    const steps = calls.flatMap((c) => (c.direct ? [{ tool: c.tool, args: c.args ?? {} }] : [
      { tool: 'mcp_describe_tool', args: { name: c.tool } },
      { tool: 'mcp_call', args: { tool: c.tool, arguments: c.args ?? {} } },
    ]));
    return `R08-TOOL-PLAN ${planToken} ${b64url(JSON.stringify({ steps }))}`;
  };
  const toolHistoryText = async (s: ToolSession) => {
    const r = await jsonFetch(rustOrNode(
      `/lingxi/v1/sessions/${encodeURIComponent(s.sessionId)}/history?all=1`,
      `/api/sessions/messages?path=${encodeURIComponent(s.path)}&reconciliation=1`,
    ));
    return JSON.stringify(r.body ?? '');
  };
  const newToolSession = async (title: string, mode: 'operate' | 'ask' = 'operate'): Promise<ToolSession | null> => {
    const r = await jsonFetch(rustOrNode('/lingxi/v1/sessions/new-detached', '/api/sessions/new-detached'), { method: 'POST', body: { agentId: 'a1', title } });
    if (r.status >= 300 || typeof r.body?.sessionId !== 'string') return null;
    const sessionId = r.body.sessionId as string;
    let path = kind === 'rust' ? `native:${sessionId}` : (typeof r.body?.path === 'string' ? r.body.path : '');
    if (!path) {
      await loadSessions();
      path = (useStore.getState().sessions as any[]).find((x) => x.sessionId === sessionId)?.path ?? '';
    }
    if (!path) return null;
    const s = { sessionId, path };
    await jsonFetch('/api/session-permission-mode', { method: 'POST', body: { mode, sessionPath: sessionLocator(s) } });
    return s;
  };
  const awaitQuiet = async () => {
    for (let quiet = 0; quiet < 12; quiet += 1) {
      const n = wsFrames.length;
      await sleep(1000);
      if (!wsFrames.slice(n).some((f) => /"isStreaming":\s*true/.test(f))) break;
    }
  };
  const runToolPlan = async (s: ToolSession, planToken: string, calls: ToolCall[], budgetMs = 90000) => {
    await ensureWsConnected();
    const wsMod = await import('../../services/websocket');
    const ws = wsMod.getWebSocket() as any;
    if (!ws || ws.readyState !== WebSocket.OPEN) return { sent: false, final: false, historyText: '', start: wsFrames.length };
    const text = planText(planToken, calls);
    const start = wsFrames.length;
    ws.send(JSON.stringify({ type: 'prompt', text, sessionId: s.sessionId, sessionPath: s.path, displayMessage: { text } }));
    let final = false;
    let historyText = '';
    const deadline = Date.now() + budgetMs;
    while (Date.now() < deadline) {
      historyText = await toolHistoryText(s);
      if (historyText.includes(`R08-TOOL-FINAL ${planToken}`)) { final = true; break; }
      await sleep(700);
    }
    await awaitQuiet();
    return { sent: true, final, historyText, start };
  };
  /** 终结回显 "R08-TOOL-FINAL <token> steps=<n> last=<最后工具回包>"。 */
  const finalEcho = (historyText: string, planToken: string) => {
    const m = historyText.match(new RegExp(`R08-TOOL-FINAL ${planToken} steps=(\\d+)(?: R08-STEER-SEEN)? last=((?:[^"\\\\]|\\\\.)*)`));
    if (!m) return null;
    let last = m[2];
    try { last = JSON.parse(`"${m[2]}"`); } catch { /* 保留转义态 */ }
    return { steps: Number(m[1]), last };
  };

  it('leaf.terminal.positive: snapshot/tail/close 三帧真实正路径（双内核 PTY）', async () => {
    await observe('leaf.terminal.positive', async () => {
      const cases: Record<string, boolean> = {};
      const fsMod = await import('node:fs');
      const pathMod = await import('node:path');
      const workspace = process.env.LINGXI_R08_AB_WORKSPACE ?? '';
      if (!workspace) return { cases, skipped: 'workspace-not-configured' };
      const wsDir = fsMod.realpathSync(workspace);
      const s = await newToolSession('R08 terminal');
      const other = await newToolSession('R08 terminal-other');
      if (!s || !other) return { cases, skipped: 'session-create-failed' };
      const terminalFramesFor = (terminalIds: string[]) => (parsed: any) => {
        const text = JSON.stringify(parsed ?? '');
        return terminalIds.some((id) => text.includes(id));
      };
      // ── 起真实 PTY（工具回合直调 exec_command tty=true）──
      // T1：交互 bash（tail/write_stdin 正路径）；T2：心跳循环（close 真实终止证明）。
      const heartbeat = pathMod.join(wsDir, `r08-terminal-heartbeat-${kind}-${Date.now()}-${process.pid}.txt`);
      const heartbeatCmd = `while true; do printf 'beat\\n' >> ${JSON.stringify(heartbeat)}; sleep 0.2; done`;
      const r1 = await runToolPlan(s, 'term1', [
        { tool: 'exec_command', args: { cmd: 'bash', tty: true, workdir: wsDir }, direct: true },
        { tool: 'exec_command', args: { cmd: heartbeatCmd, tty: true, workdir: wsDir }, direct: true },
      ]);
      const history = r1.historyText;
      const terminalIds = [...new Set(history.match(/(?:proc:[0-9a-f]{32}|term_[a-z0-9]+_[0-9a-f]{4,})/g) ?? [])];
      const interactiveId = terminalIds[0] ?? null;
      const heartbeatId = terminalIds[1] ?? null;
      const infra = { final: r1.final, echo: finalEcho(history, 'term1'), terminalIds, workspace: wsDir };
      // 证据探针（不参与判定）：让「拒绝」的形态可审计，而不是只有布尔。
      const probes: Record<string, unknown> = {
        // 「终端管理器不可用」分支在 A/B 两腿均不可达（两内核都已接线管理器）：
        // 该分支由 Rust 单测 terminals::a_wire_without_a_supervisor_is_an_explicit_capability_error
        // 与旧栈 chat.ts 的 terminal_unavailable 分支覆盖；本 field 如实披露。
        managerUnavailableReachable: false,
      };
      if (!r1.final || !interactiveId || !heartbeatId) return { cases, skipped: 'pty-start-failed', infra };

      // ══ CE6906 A0：通过 terminalWsBridge 送当前终端快照 ══
      {
        const start = await sendFrame({ type: 'terminal_snapshot_request', sessionId: s.sessionId, sessionPath: sessionLocator(s) });
        const hit = await waitServerFrame(start, (p) => p?.type === 'terminal_snapshot');
        const snapshot = hit?.parsed ?? null;
        const terminals: any[] = Array.isArray(snapshot?.terminals) ? snapshot.terminals : [];
        const current = terminals.find((t) => t?.terminalId === interactiveId);
        cases['snapshot.current-terminal-listed'] = Boolean(
          snapshot
          && snapshot.sessionId === s.sessionId
          && snapshot.sessionPath === sessionLocator(s)
          && current
          && current.status === 'running'
          && Number.isFinite(Number(current.seq))
          && typeof current.cwd === 'string'
          && typeof current.command === 'string',
        );
        cases['snapshot.lists-both-session-terminals'] = terminals.some((t) => t?.terminalId === interactiveId)
          && terminals.some((t) => t?.terminalId === heartbeatId);
        probes.snapshot = {
          type: snapshot?.type ?? null,
          currentStatus: current?.status ?? null,
          terminalCount: terminals.length,
          currentKeys: current ? Object.keys(current).sort() : null,
        };
        // ══ CE6906 A1：会话身份不符时拒绝，不送他人终端内容 ══
        // ① sessionId 与 sessionPath 指向不同会话 → 显式 error 帧，窗口内无任何终端内容。
        const mismatchStart = await sendFrame({
          type: 'terminal_snapshot_request',
          sessionId: s.sessionId,
          sessionPath: sessionLocator(other),
        });
        const mismatch = await waitServerFrame(mismatchStart, (p) => p?.type === 'terminal_snapshot' || p?.type === 'error');
        const mismatchWindow = serverFramesSince(mismatchStart);
        cases['snapshot.identity-mismatch-rejected'] = Boolean(
          mismatch
          && mismatch.parsed?.type === 'error'
          && mismatchWindow.every(({ parsed }) => !parsed || parsed.type !== 'terminal_snapshot' || !terminalFramesFor(terminalIds)(parsed)),
        );
        (probes.snapshot as Record<string, unknown>).mismatch = {
          type: mismatch?.parsed?.type ?? null,
          code: mismatch?.parsed?.code ?? null,
          leakedTerminalFrames: mismatchWindow.filter(({ parsed }) => parsed?.type === 'terminal_snapshot' && terminalFramesFor(terminalIds)(parsed)).length,
        };
        // ② 另一会话（有权）请求自己的快照 → 快照内不得出现本会话终端内容。
        const foreignStart = await sendFrame({
          type: 'terminal_snapshot_request',
          sessionId: other.sessionId,
          sessionPath: sessionLocator(other),
        });
        const foreign = await waitServerFrame(foreignStart, (p) => p?.type === 'terminal_snapshot' || p?.type === 'error');
        const foreignWindow = serverFramesSince(foreignStart);
        const foreignLeak = foreignWindow.some(({ parsed }) => terminalFramesFor(terminalIds)(parsed));
        cases['snapshot.other-session-no-leak'] = Boolean(
          foreign
          && foreign.parsed?.type === 'terminal_snapshot'
          && Array.isArray(foreign.parsed.terminals)
          && foreign.parsed.terminals.every((t: any) => !terminalIds.includes(t?.terminalId))
          && !foreignLeak,
        );
        (probes.snapshot as Record<string, unknown>).foreign = {
          type: foreign?.parsed?.type ?? null,
          terminals: Array.isArray(foreign?.parsed?.terminals) ? foreign.parsed.terminals.length : null,
          leaked: foreignLeak,
        };
      }

      // ══ 4C35E6 A0：按 terminalId/sinceSeq 返回后续输出 ══
      const requestTail = async (sessionId: string, sessionPath: string, terminalId: string, sinceSeq?: number) => {
        const start = await sendFrame({
          type: 'terminal_tail_request',
          sessionId,
          sessionPath,
          terminalId,
          ...(sinceSeq !== undefined ? { sinceSeq } : {}),
        });
        return {
          start,
          hit: await waitServerFrame(start, (p) => p?.type === 'terminal_tail' || p?.type === 'error'),
        };
      };
      const tail0 = await requestTail(s.sessionId, sessionLocator(s), interactiveId);
      const tail0Frame = tail0.hit?.parsed ?? null;
      const last0 = Number(tail0Frame?.lastSeq);
      cases['tail.initial-window'] = Boolean(
        tail0Frame
        && tail0Frame.type === 'terminal_tail'
        && tail0Frame.terminalId === interactiveId
        && tail0Frame.sessionId === s.sessionId
        && tail0Frame.sessionPath === sessionLocator(s)
        && Array.isArray(tail0Frame.chunks)
        && Number.isFinite(last0)
        && last0 >= 0
        && typeof tail0Frame.truncated === 'boolean',
      );
      // 先有输出、再经真实 write_stdin 写入新内容 → 按 sinceSeq=lasSeq 的 tail 收到新输出。
      const r2 = await runToolPlan(s, 'term2', [
        { tool: 'write_stdin', args: { process_id: interactiveId, chars: 'echo R08-TERMINAL-TAIL-MARK\\n' } },
      ]);
      const tailNew = await (async () => {
        const deadline = Date.now() + 15000;
        let latest: any = null;
        while (Date.now() < deadline) {
          const attempt = await requestTail(Number.isFinite(last0) ? s.sessionId : s.sessionId, sessionLocator(s), interactiveId, Number.isFinite(last0) ? last0 : 0);
          latest = attempt.hit?.parsed ?? null;
          const text = (latest?.chunks ?? []).map((c: any) => String(c?.data ?? '')).join('');
          if (latest?.type === 'terminal_tail' && text.includes('R08-TERMINAL-TAIL-MARK')) return latest;
          await sleep(400);
        }
        return latest;
      })();
      const tailNewText = (tailNew?.chunks ?? []).map((c: any) => String(c?.data ?? '')).join('');
      cases['tail.new-output-after-since'] = Boolean(
        r2.final
        && tailNew?.type === 'terminal_tail'
        && tailNewText.includes('R08-TERMINAL-TAIL-MARK')
        && Number(tailNew.lastSeq) > last0,
      );
      probes.tail = {
        initial: {
          type: tail0Frame?.type ?? null,
          chunks: Array.isArray(tail0Frame?.chunks) ? tail0Frame.chunks.length : null,
          lastSeq: Number.isFinite(last0) ? last0 : null,
          truncated: tail0Frame?.truncated ?? null,
        },
        writeFinal: r2.final,
        next: {
          type: tailNew?.type ?? null,
          lastSeq: tailNew?.lastSeq ?? null,
          markerSeen: tailNewText.includes('R08-TERMINAL-TAIL-MARK'),
          truncated: tailNew?.truncated ?? null,
        },
      };

      // ══ 4C35E6 A1：无权会话或不匹配终端拒绝，不接续错误进程 ══
      {
        // ① 身份不符（sessionId S + sessionPath other）→ 拒绝且不回任何终端输出。
        const mismatchTail = await requestTail(s.sessionId, sessionLocator(other), interactiveId);
        const window = serverFramesSince(mismatchTail.start);
        cases['tail.identity-mismatch-rejected'] = Boolean(
          mismatchTail.hit
          && mismatchTail.hit.parsed?.type === 'error'
          && window.every(({ parsed }) => parsed?.type !== 'terminal_tail')
          && !window.some(({ parsed }) => (parsed?.chunks ?? []).some?.((c: any) => String(c?.data ?? '').includes('R08-TERMINAL-TAIL-MARK'))),
        );
        (probes.tail as Record<string, unknown>).mismatch = {
          type: mismatchTail.hit?.parsed?.type ?? null,
          code: mismatchTail.hit?.parsed?.code ?? null,
          terminalTailFrames: window.filter(({ parsed }) => parsed?.type === 'terminal_tail').length,
        };
        // ② 有权但用他人会话的 terminalId → 拒绝，不接续错误进程、不泄内容。
        const foreignTail = await requestTail(other.sessionId, sessionLocator(other), interactiveId);
        const foreignWindow = serverFramesSince(foreignTail.start);
        cases['tail.foreign-terminal-rejected'] = Boolean(
          foreignTail.hit
          && foreignTail.hit.parsed?.type === 'error'
          && foreignWindow.every(({ parsed }) => parsed?.type !== 'terminal_tail')
          && !foreignWindow.some(({ parsed }) => JSON.stringify(parsed ?? '').includes(interactiveId) && parsed?.type === 'terminal_tail'),
        );
        (probes.tail as Record<string, unknown>).foreign = {
          type: foreignTail.hit?.parsed?.type ?? null,
          code: foreignTail.hit?.parsed?.code ?? null,
          terminalTailFrames: foreignWindow.filter(({ parsed }) => parsed?.type === 'terminal_tail').length,
        };
      }

      // ══ CFD9F0 A0：终止指定 terminalId 并返回 killed|already_stopped（真实进程被终止） ══
      const requestClose = async (sessionId: string, sessionPath: string, terminalId: string, requestId: string) => {
        const start = await sendFrame({ type: 'terminal_close_request', sessionId, sessionPath, terminalId, requestId });
        return {
          start,
          hit: await waitServerFrame(start, (p) => (p?.type === 'terminal_close_result' && String(p?.terminalId ?? '') === terminalId) || p?.type === 'error'),
        };
      };
      // 心跳文件先确有输出（真实进程证据），再关闭。
      try { await waitUntil(() => fsMod.existsSync(heartbeat) && fsMod.statSync(heartbeat).size > 0, 'heartbeat output', 10000); } catch { /* 如实入 case */ }
      const heartbeatBefore = fsMod.existsSync(heartbeat) ? fsMod.statSync(heartbeat).size : 0;
      const close1 = await requestClose(s.sessionId, sessionLocator(s), heartbeatId, 'term-close-1');
      const close1Frame = close1.hit?.parsed ?? null;
      cases['close.reports-killed-or-already-stopped'] = Boolean(
        close1Frame
        && close1Frame.type === 'terminal_close_result'
        && close1Frame.requestId === 'term-close-1'
        && (close1Frame.status === 'killed' || close1Frame.status === 'already_stopped'),
      );
      // 真实进程被终止：关闭后心跳文件不再增长（先给 1.2s 收尾窗口，再取样两次）。
      await sleep(1200);
      const beatAfter1 = fsMod.existsSync(heartbeat) ? fsMod.statSync(heartbeat).size : heartbeatBefore;
      await sleep(1000);
      const beatAfter2 = fsMod.existsSync(heartbeat) ? fsMod.statSync(heartbeat).size : beatAfter1;
      cases['close.process-really-stopped'] = heartbeatBefore > 0 && beatAfter1 === beatAfter2 && beatAfter1 >= heartbeatBefore;
      probes.close = {
        requested: {
          status: close1Frame?.status ?? null,
          requestIdEcho: close1Frame?.requestId ?? null,
        },
        heartbeat: { before: heartbeatBefore, after1: beatAfter1, after2: beatAfter2 },
      };

      // ══ CFD9F0 A1：终端管理器不可用/ID 不匹配返回 rejected，不谎报已停止 ══
      {
        const unknownClose = await requestClose(s.sessionId, sessionLocator(s), 'proc:00000000000000000000000000000000', 'term-close-unknown');
        cases['close.unknown-id-rejected'] = Boolean(
          unknownClose.hit
          && unknownClose.hit.parsed?.type === 'terminal_close_result'
          && unknownClose.hit.parsed?.status === 'rejected'
          && typeof unknownClose.hit.parsed?.reason === 'string'
          && unknownClose.hit.parsed.reason.length > 0,
        );
        (probes.close as Record<string, unknown>).unknown = {
          type: unknownClose.hit?.parsed?.type ?? null,
          status: unknownClose.hit?.parsed?.status ?? null,
          reason: unknownClose.hit?.parsed?.reason ?? null,
        };
        // 他人会话的 terminalId（有权请求方但终端不属于它）→ rejected，且终端仍真实运行。
        const foreignClose = await requestClose(other.sessionId, sessionLocator(other), interactiveId, 'term-close-foreign');
        const snapshotStart = await sendFrame({ type: 'terminal_snapshot_request', sessionId: s.sessionId, sessionPath: sessionLocator(s) });
        const snapshotAfter = await waitServerFrame(snapshotStart, (p) => p?.type === 'terminal_snapshot');
        const stillRunning = (snapshotAfter?.parsed?.terminals ?? []).some((t: any) => t?.terminalId === interactiveId && t?.status === 'running');
        cases['close.foreign-terminal-rejected'] = Boolean(
          foreignClose.hit
          && foreignClose.hit.parsed?.type === 'terminal_close_result'
          && foreignClose.hit.parsed?.status === 'rejected'
          && foreignClose.hit.parsed.status !== 'killed'
          && foreignClose.hit.parsed.status !== 'already_stopped'
          && stillRunning,
        );
        (probes.close as Record<string, unknown>).foreign = {
          status: foreignClose.hit?.parsed?.status ?? null,
          reason: foreignClose.hit?.parsed?.reason ?? null,
          terminalStillRunning: stillRunning,
        };
        // 身份不符的 close → 显式拒绝（error 或 rejected），绝不谎报 killed/already_stopped。
        const mismatchStart = await sendFrame({
          type: 'terminal_close_request',
          sessionId: s.sessionId,
          sessionPath: sessionLocator(other),
          terminalId: interactiveId,
          requestId: 'term-close-mismatch',
        });
        const mismatch = await waitServerFrame(mismatchStart, (p) => p?.type === 'terminal_close_result' || p?.type === 'error');
        const mismatchParsed = mismatch?.parsed ?? null;
        cases['close.identity-mismatch-rejected'] = Boolean(
          mismatch
          && ((mismatchParsed.type === 'error') || (mismatchParsed.type === 'terminal_close_result' && mismatchParsed.status === 'rejected'))
          && mismatchParsed.status !== 'killed'
          && mismatchParsed.status !== 'already_stopped',
        );
        (probes.close as Record<string, unknown>).mismatch = {
          type: mismatchParsed.type ?? null,
          status: mismatchParsed.status ?? null,
          code: mismatchParsed.code ?? null,
        };
      }

      // ── 收尾：正常关闭交互终端（不留进程），并断言三帧之后连接仍 OPEN ──
      const closeInteractive = await requestClose(s.sessionId, sessionLocator(s), interactiveId, 'term-close-interactive');
      const wsMod = await import('../../services/websocket');
      const stillOpen = Boolean((wsMod.getWebSocket() as any) && (wsMod.getWebSocket() as any).readyState === WebSocket.OPEN);
      cases['transport.survives-frame-family'] = stillOpen;
      const closeInteractiveStatus = closeInteractive.hit?.parsed?.status ?? null;
      // 可观测差异（不进 cases）：旧栈对已 killed 的管理器条目会再报 "killed"，
      // Rust 报 already_stopped；A0 断言两者都合法，语义都如实。

      (probes.close as Record<string, unknown>).interactive = {
        status: closeInteractiveStatus,
        requestIdEcho: closeInteractive.hit?.parsed?.requestId ?? null,
      };
      return {
        cases,
        infra,
        probes,
        heartbeat,
        last0,
        closeInteractiveStatus,
        tailNewLastSeq: tailNew?.lastSeq ?? null,
        // 调试样本（不参与判定）
        framesSample: wsFrames.slice(-4).map((f) => f.slice(0, 160)),
      };
    });
  }, 300_000);
});
