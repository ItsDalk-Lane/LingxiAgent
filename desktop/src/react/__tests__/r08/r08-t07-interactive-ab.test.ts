/** @vitest-environment jsdom */
/**
 * R08-T07｜D01 交互两叶 A/B 专属套件（追加套件，供 r08_t07_ab_drill.mjs --suite-files 消费）。
 *
 * 覆盖叶子：
 *   - R00-T02-LA-94CB746D04B7 ask_user（5 断言）→ 观测任务 leaf.tool.ask-user
 *   - R00-T02-LA-B47CAC08F418 POST /chat/task/:taskId/abort（2 断言）→ leaf.chat.task-abort
 *
 * 口径沿用主套件（r08-t07-kernel-ab.test.ts）：驱动层级 = UI 绑定层
 * （真实 store / ws-message-handler / lingxiFetch / 正式 REST 路由），非像素级 DOM。
 * 双腿由 LINGXI_R08_AB_KIND 指定，观测写 LINGXI_R08_AB_OUT；比较器由驱动完成。
 *
 * 每个 observe(task) 返回 { cases, ...观测 }；双腿 cases 键相同且全 true 才 parity。
 */
import { afterAll, beforeAll, describe, it } from 'vitest';
import { readFileSync, writeFileSync } from 'node:fs';
import { useStore } from '../../stores';
import { createLocalServerConnection, upsertServerConnection } from '../../services/server-connection';
import { lingxiFetch } from '../../hooks/use-hana-fetch';
import { readAskUserForm } from '../../components/input/ask-user-schema';
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

describe.skipIf(!enabled)(`R08-T07 interactive A/B [${kind}]`, () => {
  beforeAll(async () => {
    const port = Number(new URL(origin!).port);
    window.t = ((key: string) => key) as typeof window.t;
    window.i18n = { locale: 'en', defaultName: 'Lingxi', _data: {}, _agentOverrides: {}, load: async () => {}, setAgentOverrides: () => {}, t: window.t } as any;
    window.platform = {
      getServerConnectionInfo: async () => ({ port, token, ...(kind === 'rust' ? { serverNodeKind: 'lingxi-service', serverNodeTransport: 'http' } : {}) }),
      getPlatform: async () => 'darwin', onSettingsChanged: () => () => {}, onServerRestarted: () => () => {},
    } as unknown as typeof window.platform;
    localStorage.clear();
    // 镜像 app-init.ts：同一连接构造原语 + 同一 store 写入形态。
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
    // 迁移根 agent 名册补偿（与主套件同口径；只在 Rust 腿名册缺 a1 时经正式 API 补建）。
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

  // ── 通用边界（与主套件同形） ──
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
  const sessionLocator = (s: any) => (kind === 'rust' ? `native:${s.sessionId}` : s.path);
  // 鉴权不足探针：只换活动连接的令牌（保全量 map，与主套件 error-path.unauthorized 同口径）。
  const withInvalidToken = async <T,>(fn: () => Promise<T>): Promise<T> => {
    const good = useStore.getState().activeServerConnection as any;
    const bad = { ...good, token: 'r08-interactive-invalid-token' };
    const prevMap = { ...(useStore.getState() as any).serverConnections };
    useStore.setState({ activeServerConnection: bad, serverConnections: { ...prevMap, [bad.connectionId]: bad } } as any);
    try { return await fn(); } finally { useStore.setState({ activeServerConnection: good, serverConnections: prevMap } as any); }
  };

  // ── 真实 websocket.ts 帧捕获（与主套件同形 shim/钩子） ──
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
    if (!((window as any).WebSocket as any)?.__r08InteractiveShim) {
      const wsModuleSpecifier = 'ws';
      const { WebSocket: NodeWs } = (await import(wsModuleSpecifier)) as unknown as { WebSocket: any };
      class WebSocketShim {
        static readonly CONNECTING = 0; static readonly OPEN = 1; static readonly CLOSING = 2; static readonly CLOSED = 3;
        static __r08InteractiveShim = true;
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
    if (socket && !socket.__r08InteractiveHooked) {
      socket.__r08InteractiveHooked = true;
      const origMessage = socket.onmessage?.bind(socket);
      socket.onmessage = (ev: any) => { wsFrames.push(String(ev.data)); origMessage?.(ev); };
      const origSend = socket.send?.bind(socket);
      socket.send = (data: any) => { wsFrames.push(`>>> ${String(data)}`); return origSend(data); };
    }
    return wsMod;
  };
  const liveFrames = (from: number) => wsFrames.slice(from).filter((f) => !f.startsWith('>>> '));

  // ── 会话/回合线束（R08-TOOL-PLAN 同主套件） ──
  type ToolSession = { sessionId: string; path: string };
  type ToolCall = { tool: string; args?: Record<string, unknown>; direct?: boolean };
  const b64url = (s: string) => Buffer.from(s, 'utf8').toString('base64url');
  const planText = (planToken: string, calls: ToolCall[]) => {
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
    let sessionPath = kind === 'rust' ? `native:${sessionId}` : (typeof r.body?.path === 'string' ? r.body.path : '');
    if (!sessionPath) {
      await loadSessions();
      sessionPath = (useStore.getState().sessions as any[]).find((x) => x.sessionId === sessionId)?.path ?? '';
    }
    if (!sessionPath) return null;
    const s = { sessionId, path: sessionPath };
    await jsonFetch('/api/session-permission-mode', { method: 'POST', body: { mode, sessionPath: sessionLocator(s) } });
    return s;
  };
  const sendPlan = async (s: ToolSession, planToken: string, calls: ToolCall[]) => {
    const wsMod = await ensureWsConnected();
    const ws = wsMod.getWebSocket();
    if (!ws || ws.readyState !== WebSocket.OPEN) return { sent: false, start: wsFrames.length };
    const text = planText(planToken, calls);
    const start = wsFrames.length;
    ws.send(JSON.stringify({ type: 'prompt', text, sessionId: s.sessionId, sessionPath: s.path, displayMessage: { text } }));
    return { sent: true, start };
  };
  const awaitFinal = async (s: ToolSession, planToken: string, budgetMs = 90000) => {
    const deadline = Date.now() + budgetMs;
    let text = '';
    while (Date.now() < deadline) {
      text = await toolHistoryText(s);
      if (text.includes(`R08-TOOL-FINAL ${planToken}`)) return { final: true, text };
      await sleep(700);
    }
    return { final: false, text };
  };
  // 终结回答 "R08-TOOL-FINAL <token> steps=<n> last=<最后工具回包>"（历史 JSON 转义态）。
  const finalEcho = (historyText: string, planToken: string) => {
    const m = historyText.match(new RegExp(`R08-TOOL-FINAL ${planToken} steps=(\\d+)(?: R08-STEER-SEEN)? last=((?:[^"\\\\]|\\\\.)*)`));
    if (!m) return null;
    let last = m[2];
    try { last = JSON.parse(`"${m[2]}"`); } catch { /* 保留转义态 */ }
    return { steps: Number(m[1]), last };
  };
  const sendAbortWs = async (s: ToolSession) => {
    const wsMod = await import('../../services/websocket');
    const ws = wsMod.getWebSocket();
    if (!ws || ws.readyState !== WebSocket.OPEN) return false;
    ws.send(JSON.stringify(kind === 'rust'
      ? { type: 'abort', sessionId: s.sessionId, sessionPath: `native:${s.sessionId}`, reason: 'user' }
      : { type: 'abort', sessionId: s.sessionId, sessionPath: s.path }));
    return true;
  };

  // ── ask_user 专用：卡片面 / 答案路由 / 工具回包事实 ──
  const askQuestions = [
    {
      key: 'mode', question: '选择处理方式', type: 'single',
      options: [
        { value: 'v-fast', label: '快速' },
        { value: 'v-safe', label: '稳妥', description: '更稳的默认项' },
      ],
      recommended: 'v-safe', required: true,
    },
    {
      key: 'steps', question: '选择处理步骤', type: 'multi',
      options: [{ value: 'v-s1', label: '步骤一' }, { value: 'v-s2', label: '步骤二' }],
      required: true,
    },
    { key: 'note', question: '补充说明', type: 'text', required: true },
    { key: 'extra', question: '可选补充', type: 'text', required: false },
  ];
  const pendingConfirmations = () => ((useStore.getState() as any).pendingSessionConfirmationsByPath ?? {}) as Record<string, any>;
  const pendingAskBlockById = (id: string) => Object.values(pendingConfirmations()).find((b) => b?.confirmId === id) ?? null;
  const interactionsState = async (s: ToolSession) => {
    const r = await jsonFetch(`/lingxi/v1/sessions/interactions?sessionId=${encodeURIComponent(s.sessionId)}`);
    const questions = Array.isArray(r.body?.questions) ? r.body.questions : [];
    return { status: r.status, questions };
  };
  const askPendingEmpty = async (s: ToolSession) => {
    if (kind === 'rust') return (await interactionsState(s)).questions.length === 0;
    return Object.values(pendingConfirmations()).filter((b) => b?.kind === 'ask_user').length === 0;
  };
  /**
   * 等待 ask_user 卡到达 UI 绑定面：
   *   - 旧栈：真实 websocket 帧 content_block{session_confirmation,kind:ask_user,surface:input}
   *     经真实 ws-message-handler 写入 store pendingSessionConfirmationsByPath；
   *   - Rust：InteractiveToolsPanel 轮询的 GET /sessions/interactions 待答面。
   */
  const awaitAskCard = async (s: ToolSession, framesStart: number, budgetMs = 30000) => {
    const deadline = Date.now() + budgetMs;
    while (Date.now() < deadline) {
      if (kind === 'rust') {
        const state = await interactionsState(s);
        const hit = state.questions.find((q: any) => Array.isArray(q?.questions) && q.questions.some((x: any) => x?.question === '选择处理方式'));
        if (hit) return { id: String(hit.id), questions: hit.questions as any[], surface: 'interactions' };
      } else {
        const frame = liveFrames(framesStart).reverse().find((f) => (
          f.includes('session_confirmation') && f.includes('ask_user')
          && (f.includes(s.sessionId) || f.includes(s.path))
        ));
        if (frame) {
          try {
            const msg = JSON.parse(frame);
            const block = msg.block ?? msg.request?.request ?? msg.request;
            const questions = block?.payload?.questions ?? block?.questions;
            if (Array.isArray(questions)) {
              return { id: String(block.confirmId ?? block.id ?? ''), questions, surface: 'ws', block };
            }
          } catch { /* 继续等 */ }
        }
      }
      await sleep(200);
    }
    return null;
  };
  const readCardForm = (questions: any[]) => {
    try { return readAskUserForm({ kind: 'ask_user', payload: { questions } } as any); } catch { return null; }
  };
  const answerAsk = (s: ToolSession, id: string, semantic: 'confirm' | 'dismiss', values?: Record<string, unknown>) => {
    if (kind === 'rust') {
      return jsonFetch('/lingxi/v1/sessions/interactions/answer', {
        method: 'POST',
        body: { sessionId: s.sessionId, id, action: semantic === 'confirm' ? 'confirmed' : 'dismissed', values: values ?? {} },
      });
    }
    return jsonFetch(`/api/confirm/${encodeURIComponent(id)}`, {
      method: 'POST',
      body: { action: semantic === 'confirm' ? 'confirmed' : 'rejected', value: values },
    });
  };
  /** 旧栈工具回包 details 只在会话转录原文里（历史投影按 UI 白名单裁剪，不含 answers）。 */
  const nodeTranscriptEntries = (s: ToolSession) => {
    try {
      return readFileSync(s.path, 'utf8').split('\n').filter(Boolean).map((line: string) => {
        try { return JSON.parse(line); } catch { return null; }
      }).filter(Boolean);
    } catch { return []; }
  };
  const contentTextOf = (content: any): string => {
    if (typeof content === 'string') return content;
    if (Array.isArray(content)) return content.map((c: any) => (c && typeof c.text === 'string' ? c.text : '')).join('\n');
    return '';
  };
  const nodeAskResult = (s: ToolSession) => {
    const entries = nodeTranscriptEntries(s);
    for (let i = entries.length - 1; i >= 0; i -= 1) {
      const entry = entries[i];
      const message = entry?.message ?? entry;
      if (!message || (message.role !== 'toolResult' && message.role !== 'tool_result')) continue;
      const details = message.details;
      if (details && typeof details === 'object' && typeof details.answered === 'boolean') {
        return { details, contentText: contentTextOf(message.content), toolName: message.toolName ?? null };
      }
    }
    return null;
  };
  const findAskPayload = (value: unknown): any | null => {
    if (!value || typeof value !== 'object') return null;
    if (Array.isArray(value)) {
      for (const item of value) { const hit = findAskPayload(item); if (hit) return hit; }
      return null;
    }
    const obj = value as Record<string, unknown>;
    if (Array.isArray(obj.answers) && typeof obj.answered === 'boolean') return obj;
    for (const item of Object.values(obj)) { const hit = findAskPayload(item); if (hit) return hit; }
    return null;
  };
  const rustAskResult = async (s: ToolSession) => {
    const r = await jsonFetch(`/lingxi/v1/sessions/${encodeURIComponent(s.sessionId)}/history?all=1`);
    const details = findAskPayload(r.body);
    return details ? { details } : null;
  };
  const askToolResult = async (s: ToolSession) => (kind === 'rust' ? rustAskResult(s) : nodeAskResult(s));
  const answerById = (answers: any[], key: string) => answers.find((a) => a?.key === key) ?? null;

  it('leaf.tool.ask-user: 卡片可见/确认映射/超时推荐/拒答与中止无答案/无效与缺省显式错误（5 断言）', async () => {
    await observe('leaf.tool.ask-user', async () => {
      const cases: Record<string, boolean> = {};
      const debug: Record<string, unknown> = {};
      await ensureWsConnected();
      const s = await newToolSession('R08 ask-user');
      if (!s) return { cases, skipped: 'session-create-failed' };

      // ── A0 卡片问题与选项正确可见 ──
      const sentCard = await sendPlan(s, 'askc1', [{ tool: 'ask_user', args: { questions: askQuestions } }]);
      const card = await awaitAskCard(s, sentCard.start, 40000);
      const form = card ? readCardForm(card.questions) : null;
      cases['a0.questions-and-options-visible'] = Boolean(form) && form!.unsupported.length === 0
        && form!.questions.length === 4
        && form!.questions[0].question === '选择处理方式' && form!.questions[0].type === 'single'
        && form!.questions[0].options.map((o) => o.label).join('|') === '快速|稳妥'
        && form!.questions[1].type === 'multi' && form!.questions[1].options.map((o) => o.label).join('|') === '步骤一|步骤二'
        && form!.questions[2].type === 'text' && form!.questions[2].options.length === 0;
      cases['a0.values-and-labels-visible'] = Boolean(form)
        && form!.questions[0].options.map((o) => o.value).join('|') === 'v-fast|v-safe'
        && form!.questions[0].options[1].description === '更稳的默认项';
      cases['a0.recommended-visible'] = Boolean(form) && form!.questions[0].recommended.join('|') === 'v-safe';
      const pendingSurface = card
        ? (kind === 'rust'
          ? (await interactionsState(s)).questions.some((q: any) => q.id === card.id)
          : Boolean(pendingAskBlockById(card.id)))
        : false;
      cases['a0.pending-question-exposed'] = Boolean(card?.id) && pendingSurface;
      debug.card = { id: card?.id ?? null, surface: card?.surface ?? null, questions: card?.questions?.length ?? 0, pendingSurface };

      if (!card) return { cases, debug, sentCard };
      // ── A1 提交值与 key/label 映射一致 ──
      const submitValues = { mode: 'v-fast', steps: ['v-s2', 'v-unknown', 'v-s2'], note: '  用户原文  ' };
      const answered = await answerAsk(s, card.id, 'confirm', submitValues);
      debug.answerStatus = answered.status;
      // 待答面清除（真实 binding：旧栈 store 由 confirmation_resolved 帧清除；Rust 由 answer 路由清 pending）。
      let pendingClearedAfterAnswer = false;
      try {
        const deadline = Date.now() + 15000;
        while (Date.now() < deadline) {
          if (await askPendingEmpty(s)) { pendingClearedAfterAnswer = true; break; }
          await sleep(200);
        }
      } catch { /* 如实 */ }
      const round = await awaitFinal(s, 'askc1');
      const result = await askToolResult(s);
      const details = result?.details ?? null;
      const answers: any[] = Array.isArray(details?.answers) ? details.answers : [];
      const modeAnswer = answerById(answers, 'mode');
      const stepsAnswer = answerById(answers, 'steps');
      const noteAnswer = answerById(answers, 'note');
      cases['a1.confirmed-answered-true'] = details?.answered === true && details?.timedOut === false;
      cases['a1.keys-mapped'] = answers.length === 4 && answers.map((a) => a.key).join('|') === 'mode|steps|note|extra'
        && modeAnswer?.question === '选择处理方式';
      cases['a1.values-mapped'] = JSON.stringify(modeAnswer?.values) === '["v-fast"]'
        && JSON.stringify(stepsAnswer?.values) === '["v-s2"]'
        && JSON.stringify(noteAnswer?.values) === '["用户原文"]';
      cases['a1.labels-mapped'] = JSON.stringify(modeAnswer?.labels) === '["快速"]'
        && JSON.stringify(stepsAnswer?.labels) === '["步骤二"]'
        && JSON.stringify(noteAnswer?.labels) === '["用户原文"]';
      cases['a1.unknown-value-dropped'] = JSON.stringify(stepsAnswer?.values) === '["v-s2"]'
        && !JSON.stringify(stepsAnswer).includes('v-unknown');
      cases['a1.optional-unanswered'] = answerById(answers, 'extra')?.unanswered === true
        && Array.isArray(answerById(answers, 'extra')?.values) && answerById(answers, 'extra')!.values.length === 0;
      cases['a1.no-auto-selected'] = answers.every((a) => a?.autoSelected === false);
      cases['a1.pending-cleared-after-answer'] = pendingClearedAfterAnswer;
      cases['a1.round-completes'] = round.final === true && Boolean(finalEcho(round.text, 'askc1'));
      debug.answerDetails = JSON.stringify(details ?? null).slice(0, 600);
      // 已结算/未知问题：正确路由显式拒（A4 错误面之一；不编造成功）。
      const settled = await answerAsk(s, card.id, 'confirm', submitValues);
      const ghost = kind === 'rust'
        ? await jsonFetch('/lingxi/v1/sessions/interactions/answer', { method: 'POST', body: { sessionId: s.sessionId, id: 'r08-no-such-question', action: 'confirmed', values: {} } })
        : await jsonFetch(`/api/confirm/${encodeURIComponent('r08-no-such-question')}`, { method: 'POST', body: { action: 'confirmed' } });
      cases['a4.settled-or-unknown-question-explicit-error'] = settled.status >= 400 && settled.status < 500
        && ghost.status >= 400 && ghost.status < 500;
      // 无会话面：旧栈 confirm 路由（无 pending 归属）与 Rust interactions 路由（未知 sessionId）都必须显式错误。
      const missing = kind === 'rust'
        ? await jsonFetch(`/lingxi/v1/sessions/interactions?sessionId=${encodeURIComponent('r08-missing-session')}`)
        : await jsonFetch(`/api/confirm/${encodeURIComponent('r08-missing-session-confirm')}`, { method: 'POST', body: { action: 'confirmed' } });
      cases['a4.missing-session-explicit-error'] = missing.status >= 400 && missing.status < 500;
      debug.settled = { status: settled.status, ghost: ghost.status, missing: missing.status };
      await askPendingEmpty(s);

      // ── A2 超时只自动填推荐项（timedOut/autoSelected；非推荐题不编造） ──
      const sTimeout = await newToolSession('R08 ask-timeout');
      if (sTimeout) {
        const sentTimeout = await sendPlan(sTimeout, 'askto1', [{ tool: 'ask_user', args: { questions: askQuestions, timeout_seconds: 30 } }]);
        const cardTimeout = await awaitAskCard(sTimeout, sentTimeout.start, 40000);
        const roundTimeout = cardTimeout ? await awaitFinal(sTimeout, 'askto1', 120000) : { final: false, text: '' };
        const resultTimeout = await askToolResult(sTimeout);
        const detailsTimeout = resultTimeout?.details ?? null;
        const answersTimeout: any[] = Array.isArray(detailsTimeout?.answers) ? detailsTimeout.answers : [];
        const modeTimeout = answerById(answersTimeout, 'mode');
        const stepsTimeout = answerById(answersTimeout, 'steps');
        cases['a2.timedout-flag'] = Boolean(cardTimeout) && detailsTimeout?.timedOut === true && detailsTimeout?.answered === true;
        cases['a2.recommended-auto-selected'] = JSON.stringify(modeTimeout?.values) === '["v-safe"]'
          && JSON.stringify(modeTimeout?.labels) === '["稳妥"]'
          && modeTimeout?.autoSelected === true;
        cases['a2.non-recommended-not-filled'] = stepsTimeout?.autoSelected === false
          && Array.isArray(stepsTimeout?.values) && stepsTimeout!.values.length === 0
          && answerById(answersTimeout, 'note')?.autoSelected === false
          && answerById(answersTimeout, 'extra')?.autoSelected === false;
        cases['a2.round-completes'] = roundTimeout.final === true;
        // 超时后待答面必须清除（旧栈 confirmation_resolved timeout 帧 / Rust 定时清理）。
        let pendingClearedAfterTimeout = false;
        const timeoutDeadline = Date.now() + 15000;
        while (Date.now() < timeoutDeadline) {
          if (await askPendingEmpty(sTimeout)) { pendingClearedAfterTimeout = true; break; }
          await sleep(300);
        }
        cases['a2.pending-cleared-after-timeout'] = pendingClearedAfterTimeout;
        debug.timeout = { card: Boolean(cardTimeout), details: JSON.stringify(detailsTimeout ?? null).slice(0, 500), final: roundTimeout.final };
      } else {
        debug.timeout = 'session-create-failed';
      }

      // ── A3 拒答无答案（dismissed/answers 空） + A4 不编造答案 ──
      const sDismiss = await newToolSession('R08 ask-dismiss');
      let dismissedDetails: any = null;
      if (sDismiss) {
        const sentDismiss = await sendPlan(sDismiss, 'askdi1', [{ tool: 'ask_user', args: { questions: askQuestions } }]);
        const cardDismiss = await awaitAskCard(sDismiss, sentDismiss.start, 40000);
        const answeredDismiss = cardDismiss ? await answerAsk(sDismiss, cardDismiss.id, 'dismiss') : { status: 0, body: null };
        const roundDismiss = cardDismiss ? await awaitFinal(sDismiss, 'askdi1', 60000) : { final: false, text: '' };
        const resultDismiss = cardDismiss ? await askToolResult(sDismiss) : null;
        dismissedDetails = resultDismiss?.details ?? null;
        const echoDismiss = roundDismiss.final ? finalEcho(roundDismiss.text, 'askdi1') : null;
        cases['a3.dismissed-no-answers'] = answeredDismiss.status === 200
          && dismissedDetails?.answered === false
          && dismissedDetails?.dismissed === true
          && Array.isArray(dismissedDetails?.answers) && dismissedDetails!.answers.length === 0;
        let pendingClearedAfterDismiss = false;
        const dismissDeadline = Date.now() + 15000;
        while (Date.now() < dismissDeadline) {
          if (await askPendingEmpty(sDismiss)) { pendingClearedAfterDismiss = true; break; }
          await sleep(200);
        }
        cases['a3.dismissed-pending-cleared'] = pendingClearedAfterDismiss;
        cases['a3.dismissed-round-completes'] = roundDismiss.final === true;
        // 拒答后模型可见终结回显不得出现任何被选答案（不编造回答）。
        cases['a4.dismissed-does-not-fabricate-answer'] = Boolean(echoDismiss)
          && !/(快速|稳妥|步骤一|步骤二)/.test(echoDismiss!.last ?? '');
        debug.dismiss = { answered: answeredDismiss.status, details: JSON.stringify(dismissedDetails ?? null).slice(0, 400), echo: echoDismiss?.last?.slice(0, 200) ?? null };
      } else {
        debug.dismiss = 'session-create-failed';
      }

      // ── A3 中止无答案（abort 待答问题；aborted/answers 空，不编造） ──
      const sAbort = await newToolSession('R08 ask-abort');
      if (sAbort) {
        const sentAbort = await sendPlan(sAbort, 'askab1', [{ tool: 'ask_user', args: { questions: askQuestions } }]);
        const cardAbort = await awaitAskCard(sAbort, sentAbort.start, 40000);
        const abortFrameStart = wsFrames.length;
        const abortSent = cardAbort ? await sendAbortWs(sAbort) : false;
        let abortSettled = false;
        const abortDeadline = Date.now() + 25000;
        while (Date.now() < abortDeadline) {
          if (await askPendingEmpty(sAbort)) { abortSettled = true; break; }
          await sleep(250);
        }
        // 旧栈中止路径经 confirmStore.abortBySession 结算问题；Rust 取消 run 时 drop 待答 future。
        // 两者都必须让待答问题离场，且不得产生任何 answered=true 的编造答案。
        const resultAbort = await askToolResult(sAbort);
        const abortDetails = resultAbort?.details ?? null;
        const abortFrames = liveFrames(abortFrameStart);
        const abortAcknowledged = abortFrames.some((f) => /"aborted":\s*true|abort_result|"isStreaming":\s*false/.test(f));
        cases['a3.aborted-no-answers'] = Boolean(cardAbort) && abortSent && abortSettled
          && (abortDetails === null || (abortDetails.answered === false
            && Array.isArray(abortDetails.answers) && abortDetails.answers.length === 0));
        cases['a3.abort-acknowledged'] = abortAcknowledged;
        cases['a4.aborted-does-not-fabricate-answer'] = abortDetails === null
          || (abortDetails.answered !== true && !JSON.stringify(abortDetails).includes('"values":["v-'));
        debug.abort = {
          card: Boolean(cardAbort), settled: abortSettled, ack: abortAcknowledged,
          details: JSON.stringify(abortDetails ?? null).slice(0, 300),
          framesTail: abortFrames.slice(-3).map((f) => f.slice(0, 180)),
        };
      } else {
        debug.abort = 'session-create-failed';
      }

      // ── A4 问题无效返回错误（真实工具回合，卡片不产生、不写答案） ──
      const sInvalid = await newToolSession('R08 ask-invalid');
      if (sInvalid) {
        const invalidQuestions = [{ key: 'dup', question: '第一问' }, { key: 'dup', question: '第二问' }];
        const sentInvalid = await sendPlan(sInvalid, 'askiv1', [{ tool: 'ask_user', args: { questions: invalidQuestions } }]);
        const roundInvalid = await awaitFinal(sInvalid, 'askiv1', 60000);
        const echoInvalid = roundInvalid.final ? finalEcho(roundInvalid.text, 'askiv1') : null;
        const invalidResult = await askToolResult(sInvalid);
        cases['a4.invalid-questions-explicit-error'] = roundInvalid.final === true && Boolean(echoInvalid)
          && /invalid|duplicate|key|questions|错误|无效|拒绝/i.test(echoInvalid!.last ?? '');
        cases['a4.invalid-questions-no-card'] = (await askPendingEmpty(sInvalid)) && invalidResult === null
          && !liveFrames(sentInvalid.start).some((f) => f.includes('"kind":"ask_user"'));
        debug.invalid = { echo: echoInvalid?.last?.slice(0, 300) ?? null, pending: await askPendingEmpty(sInvalid) };
      } else {
        debug.invalid = 'session-create-failed';
      }

      return { cases, debug, sessionIds: [s.sessionId, sTimeout?.sessionId, sDismiss?.sessionId, sAbort?.sessionId, sInvalid?.sessionId].filter(Boolean) };
    });
  }, 420_000);

  it('leaf.chat.task-abort: 终止本会话拥有的后台子代理任务 + 404/403/运行态/不扩权（2 断言）', async () => {
    await observe('leaf.chat.task-abort', async () => {
      const cases: Record<string, boolean> = {};
      const debug: Record<string, unknown> = {};
      await ensureWsConnected();
      const parent = await newToolSession('R08 task-abort parent');
      const other = await newToolSession('R08 task-abort other');
      if (!parent || !other) return { cases, skipped: 'session-create-failed' };
      // 子代理任务用「20 步 read 计划」拖住子 run（每步替身固定 350ms 延迟），
      // 给所有权/运行态断言一个确定窗口；子任务文本同时携带 R08-TOOL-PLAN 标记。
      const childToken = `r08child${Date.now().toString(36)}`;
      const childSteps: ToolCall[] = Array.from({ length: 20 }, () => ({ tool: 'read', args: { path: `r08-abort-child-${childToken}.txt` }, direct: true }));
      const childTask = planText(childToken, childSteps);
      // 旧栈真实挂载点 /api/task/:taskId/abort（server/composition/open-root.ts 把 chat
      // restRoute 挂在 /api；原断言文案写作 /chat/task/... 是路由模块名，不是前缀）。
      const abortUrl = (taskId: string) => rustOrNode(
        `/lingxi/v1/task/${encodeURIComponent(taskId)}/abort`,
        `/api/task/${encodeURIComponent(taskId)}/abort`,
      );
      const abortTask = (s: ToolSession, taskId: string, bodyOverride?: unknown) => jsonFetch(abortUrl(taskId), {
        method: 'POST',
        body: bodyOverride ?? { sessionId: s.sessionId },
      });
      const taskIdFrom = (text: string): string | null => {
        const m = text.match(/taskId\\?"\s*:\s*\\?"([A-Za-z0-9._:-]+)/)
          ?? text.match(/task-id:\s*([A-Za-z0-9._:-]+)/);
        return m?.[1] ?? null;
      };
      const launchChild = async (s: ToolSession, planToken: string) => {
        const sent = await sendPlan(s, planToken, [{ tool: 'subagent', args: { agent: 'a1', label: `r08-${planToken}`, task: childTask } }]);
        if (!sent.sent) return null;
        const deadline = Date.now() + 45000;
        while (Date.now() < deadline) {
          const taskId = taskIdFrom(await toolHistoryText(s));
          if (taskId) return taskId;
          await sleep(300);
        }
        return null;
      };

      // ── A0 成功：终止当前会话拥有的后台子代理任务 → aborted 或 already_stopped ──
      const ownedTask = await launchChild(parent, 'r08ab1');
      const owned = ownedTask ? await abortTask(parent, ownedTask) : { status: 0, body: null };
      const ownedStatus = String((owned.body as any)?.status ?? '');
      cases['a0.owned-task-aborted-or-already-stopped'] = ownedTask !== null
        && owned.status === 200 && (ownedStatus === 'aborted' || ownedStatus === 'already_stopped');
      const ownedKeys = owned.body && typeof owned.body === 'object' ? Object.keys(owned.body as any) : [];
      const ownedText = JSON.stringify(owned.body ?? {});
      cases['a0.owned-response-scoped'] = ownedKeys.length > 0
        && ownedKeys.every((k) => ['ok', 'status', 'reason', 'sessionId', 'sessionPath', 'taskId'].includes(k))
        && !/answers|messages|history|tasks|transcript|parentSessionId/i.test(ownedText);
      // 运行态决定第二次结果：已终结任务再次 abort 必须显式终态且不得再次伪报 aborted。
      // 双腿差异（如实入观测、不硬改产品）：Rust 保留子线程历史 → already_stopped(200)；
      // 旧栈 deferred 任务在 finally 中移出 registry → not_found(404)。原断言「找不到 404」
      // 与「already_stopped」两种都属合规终态，故判定面 = 显式终态（非 aborted）。
      const second = ownedTask ? await abortTask(parent, ownedTask) : { status: 0, body: null };
      const secondStatus = String((second.body as any)?.status ?? '');
      cases['a1.terminal-state-decides-result'] = ownedTask !== null
        && ((second.status === 200 && secondStatus === 'already_stopped')
          || (second.status === 404 && (second.body as any)?.reason === 'not_found'));
      // 未知 taskId（会话本身有效）→ 404。
      const unknown = await abortTask(parent, `r08-no-such-task-${Date.now().toString(36)}`);
      cases['a0.unknown-task-404'] = unknown.status === 404;
      // 缺 body 会话上下文 → 400（旧栈 resolveWsSessionContext 拒绝；Rust session locator 无效）。
      const missingContext = await abortTask(parent, `r08-no-such-task-${Date.now().toString(36)}`, {});
      cases['a1.missing-session-context-400'] = missingContext.status === 400;
      // 鉴权不足 → 401/403（不得越权停止）。
      const unauthorized = await withInvalidToken(() => abortTask(parent, `r08-no-such-task-${Date.now().toString(36)}`));
      cases['a1.unauthorized-rejected'] = unauthorized.status === 401 || unauthorized.status === 403;
      debug.owned = { task: ownedTask, status: owned.status, body: JSON.stringify(owned.body).slice(0, 300), second: second.status, secondBody: JSON.stringify(second.body).slice(0, 200) };
      debug.unknown = { status: unknown.status, body: JSON.stringify(unknown.body).slice(0, 200) };
      debug.missing = { status: missingContext.status, body: JSON.stringify(missingContext.body).slice(0, 200) };
      debug.unauthorized = unauthorized.status;

      // ── A1 所有权：非本会话拥有的 taskId → 403；不得泄漏/写入另一会话状态 ──
      const otherTask = await launchChild(other, 'r08ab2');
      const cross = otherTask ? await abortTask(parent, otherTask) : { status: 0, body: null };
      cases['a0.cross-session-403'] = otherTask !== null && cross.status === 403;
      const crossText = JSON.stringify(cross.body ?? '');
      cases['a1.cross-session-no-leak'] = otherTask !== null
        && !crossText.includes(other.sessionId)
        && !crossText.includes(other.path)
        && !/answers|messages|history|tasks|transcript/i.test(crossText);
      // 越权尝试不得真的停掉任务：随后由拥有者会话停止必须仍能返回 aborted。
      const ownerAbort = otherTask ? await abortTask(other, otherTask) : { status: 0, body: null };
      const ownerStatus = String((ownerAbort.body as any)?.status ?? '');
      cases['a1.cross-session-task-unaffected'] = otherTask !== null
        && ownerAbort.status === 200 && ownerStatus === 'aborted';
      debug.cross = { task: otherTask, status: cross.status, body: JSON.stringify(cross.body).slice(0, 300), ownerAbort: ownerAbort.status, ownerBody: JSON.stringify(ownerAbort.body).slice(0, 200) };

      // 收尾：等两个子计划在后台自然结束（子 run 独立于父 run；不等待则下一次边界不稳）。
      await sleep(1200);
      return { cases, debug, parent: parent.sessionId, other: other.sessionId };
    });
  }, 300_000);
});
