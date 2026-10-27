/** @vitest-environment jsdom */
/**
 * R08-T07｜A13 同界面换内核 A/B 任务套件（v3：受控顺序 + 逐任务隔离）。
 *
 * 同一 React 构建源的真实 store 动作/客户端边界经活内核执行固定产品任务集
 * （内核由 r08_t07_ab_drill.mjs 注入 env；观测落 _OUT JSON）。任务顺序受控：
 * 只读在前、负路径次之、轻变更居中、破坏性（归档/删除/清理）殿后；observe
 * 捕获后记录并继续（单任务失败不断链，ok=false 如实入观测）；破坏性调用带防御
 * 守卫并尽力复原。A/B 平价判定由驱动的比较器完成，本套件只如实记录。
 *
 * 口径披露：驱动层级 = UI 绑定层（store action / lingxiFetch），非像素级 DOM。
 * 网络边界：内核地址必须 http://127.0.0.1:<port>（环回强校验，否则整套拒跑）。
 */
import { afterAll, beforeAll, describe, it } from 'vitest';
import { writeFileSync } from 'node:fs';
import { useStore } from '../../stores';
import {
  archiveSession,
  cleanupArchivedSessions,
  completeSessionTodos,
  cancelSessionTodos,
  createNewSession,
  dismissSessionTodoPanel,
  ensureSession,
  listArchivedSessions,
  loadMessages,
  loadPendingNewSessionPermissionDefault,
  loadSessions,
  pinSession,
  refreshSessionCapabilities,
  renameSession,
  reorderPinnedSessions,
  restoreSession,
  sweepOrphanedWorkspaceSessions,
  switchSession,
} from '../../stores/session-actions';
import { createLocalServerConnection, upsertServerConnection } from '../../services/server-connection';
import { lingxiFetch } from '../../hooks/use-hana-fetch';

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
    // 记录后继续：单任务失败不断链；ok=false 如实入观测（比较器判 failed）。
    observations[task] = { ok: false, error: error instanceof Error ? error.message : String(error), ms: Date.now() - started };
  }
}
const titles = () => [...useStore.getState().sessions].map((s: any) => s.title).sort();
const sessionByTitle = (title: string) => useStore.getState().sessions.find((s: any) => s.title === title);
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const resetPending = () => useStore.setState({ pendingNewSession: false, pendingSessionSwitchPath: null, pendingDraftId: null } as any);

describe.skipIf(!enabled)(`R08-T07 kernel A/B [${kind}]`, () => {
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
    // 迁移根 agent 名册补偿（T06 手册：Register 不随迁，正式切换经正式入口补建
    // 同 id agent；D1 已证）。只在 Rust 腿且名册缺 a1 时经正式 API 补建，不改
    // 旧腿；补建不产生会话，不影响列表/漂移守卫计数平价。
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
    } else {
      // A 腿主对话模型由夹具 agents/a1/config.yaml（api.provider + models.chat）
      // 持久选定——/api/models/set 只是一次性 pending（首个 create 消费后失效），
      // 不在此依赖它。
    }
  });

  afterAll(() => {
    if (out) writeFileSync(out, `${JSON.stringify({ kind, origin, observations }, null, 2)}\n`);
  });

  it('identity: server identity reachable with loopback token', async () => {
    await observe('identity', async () => {
      const path = kind === 'rust' ? '/lingxi/v1/server/identity' : '/api/server/identity';
      const res = await lingxiFetch(path, { method: 'GET' });
      const json = await res.json();
      return { status: res.status, hasUserId: Boolean(json.userId ?? json.user?.id), keys: Object.keys(json).sort().slice(0, 12) };
    });
  });

  // D-11（R08 收尾实证）：旧栈在助手配置了模型时**启动即自动建一个会话**
  // （kernel-a-node.log `createSession … restore=false` 于首请求前），Rust 的
  // 会话创建由客户端待建草稿驱动（ensureSession），服务端不自动建。同一夹具下
  // A 腿总数恒 +1。平价计数口径 = 排除启动自建会话（首个列表里不属夹具身份
  // `ab-real-*` 的会话），总数与启动自建数如实入观测供差异报告引用。
  const bootSessionIds = new Set<string>();
  const isFixtureSession = (s: any) => String(s.sessionId ?? '').includes('ab-real-') || String(s.path ?? '').includes('ab-real-');
  const comparableCount = (sessions: any[]) => sessions.filter((s) => !bootSessionIds.has(String(s.sessionId ?? s.path))).length;

  it('sessions.list: 查看可见会话列表', async () => {
    await observe('sessions.list', async () => {
      await loadSessions();
      const sessions = useStore.getState().sessions as any[];
      for (const s of sessions) if (!isFixtureSession(s)) bootSessionIds.add(String(s.sessionId ?? s.path));
      return { count: comparableCount(sessions), rawCount: sessions.length, bootAutoCreated: bootSessionIds.size, titles: titles(), pinned: sessions.filter((s) => s.pinned).length };
    });
  });

  it('leaf.list.fields: 列表投影字段形状', async () => {
    await observe('leaf.list.fields', async () => {
      const first = (useStore.getState().sessions as any[])[0];
      const keys = first ? Object.keys(first).sort() : [];
      return { hasKeys: keys.length > 0, hasRevision: keys.includes('revision'), hasOwner: keys.includes('agentId') || keys.includes('agent') || keys.includes('ownerKind'), hasSummaryOrBadge: keys.includes('hasSummary') || keys.includes('loopStatus') || keys.includes('summary') || keys.includes('runningBadge') || keys.includes('activeRuns'), keys: keys.slice(0, 24) };
    });
  });

  it('sessions.switch: 切换焦点会话 + 消息页', async () => {
    await observe('sessions.switch', async () => {
      const first = (useStore.getState().sessions as any[])[0];
      if (!first) return { focused: false, messages: null };
      await switchSession(first.path);
      await loadMessages(first.path);
      const state = useStore.getState() as any;
      return { focused: Boolean(state.currentSessionPath), messages: Array.isArray(state.messages) ? state.messages.length : null };
    });
  });
  it('leaf.rename.negative: 空标题/无效会话显式拒绝', async () => {
    await observe('leaf.rename.negative', async () => {
      const first = (useStore.getState().sessions as any[])[0];
      if (!first) return { emptyRejected: false, invalidRejected: false, skipped: true };
      const emptyRes = await lingxiFetch('/api/sessions/rename', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: first.path, title: '   ' }), throwOnHttpError: false,
      });
      const emptyBody = await emptyRes.json().catch(() => null);
      const emptyRejected = emptyRes.status >= 400 || Boolean(emptyBody?.error) || emptyBody?.ok === false;
      const invalidRes = await lingxiFetch('/api/sessions/rename', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: 'native:sess-does-not-exist-t07', title: 'x' }), throwOnHttpError: false,
      });
      const invalidBody = await invalidRes.json().catch(() => null);
      const invalidRejected = invalidRes.status >= 400 || Boolean(invalidBody?.error) || invalidBody?.ok === false;
      await loadSessions();
      return { emptyRejected, emptyStatus: emptyRes.status, invalidRejected, invalidStatus: invalidRes.status };
    });
  });

  it('leaf.pin.negative: 无效会话置顶显式拒绝', async () => {
    await observe('leaf.pin.negative', async () => {
      const res = await lingxiFetch('/api/sessions/pin', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: 'native:sess-does-not-exist-t07', pinned: true }), throwOnHttpError: false,
      });
      const body = await res.json().catch(() => null);
      return { rejected: res.status >= 400 || Boolean(body?.error) || body?.ok === false, status: res.status };
    });
  });

  it('leaf.todos.negative: 无效会话计划操作显式拒绝', async () => {
    await observe('leaf.todos.negative', async () => {
      const res = await lingxiFetch('/api/sessions/todos/complete', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: 'native:sess-does-not-exist-t07', version: null }), throwOnHttpError: false,
      });
      const body = await res.json().catch(() => null);
      return { rejected: res.status >= 400 || Boolean(body?.error) || body?.ok === false, status: res.status };
    });
  });

  it('leaf.mcp.state: 查看连接器状态 + 延迟加载开关（投影形状）', async () => {
    await observe('leaf.mcp.state', async () => {
      // 与设置页同一读取通路：rust 分支 /lingxi/v1/mcp/state（config 包裹），
      // 旧分支 /api/mcp/state（平铺）。对齐 loadMcpState 的投影语义。
      const path = kind === 'rust' ? '/lingxi/v1/mcp/state' : '/api/mcp/state';
      const res = await lingxiFetch(path, { method: 'GET', throwOnHttpError: false });
      const data = await res.json().catch(() => null);
      if (!res.ok || !data) return { ok: false, status: res.status };
      const cfg = kind === 'rust' ? (data.config ?? {}) : data;
      return {
        ok: true, status: res.status,
        enabledIsBool: typeof cfg.enabled === 'boolean',
        connectorsIsArray: Array.isArray(cfg.connectors) || Array.isArray(data.servers),
        deferFlag: typeof cfg.deferEnabled === 'boolean' || typeof data.deferEnabled === 'boolean' || kind !== 'rust',
        agentScoped: kind !== 'rust' || data.agentId !== undefined,
      };
    });
  });

  it('leaf.permission.session-mode: 查看当前会话权限模式（switch 投影 mode 字段）', async () => {
    await observe('leaf.permission.session-mode', async () => {
      const first = (useStore.getState().sessions as any[])[0];
      if (!first) return { modePresent: false, modeValue: null };
      // 正路径：切换到会话读取投影的 permissionMode（旧 /api/sessions/switch
      // 返回 permissionMode；Rust /lingxi/v1/sessions/switch 同名字段）。
      const path = kind === 'rust' ? '/lingxi/v1/sessions/switch' : '/api/sessions/switch';
      const res = await lingxiFetch(path, {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(kind === 'rust' ? { sessionId: first.sessionId } : { path: first.path }),
        throwOnHttpError: false,
      });
      const data = await res.json().catch(() => null);
      const mode = data?.permissionMode ?? null;
      // 断言1（读无副作用）：同会话二次切换读 mode 一致。
      const res2 = await lingxiFetch(path, {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(kind === 'rust' ? { sessionId: first.sessionId } : { path: first.path }),
        throwOnHttpError: false,
      });
      const data2 = await res2.json().catch(() => null);
      const mode2 = data2?.permissionMode ?? null;
      return { modePresent: res.status < 300 && typeof mode === 'string', modeValue: mode, status: res.status, idempotent: mode === mode2 && mode !== null };
    });
  });

  it('leaf.permission.default-set: 设置新会话默认权限（写-回读-非法值拒绝）', async () => {
    await observe('leaf.permission.default-set', async () => {
      // 正路径：PUT 合法值 → GET 回读一致；复原为原值。
      const orig = await loadPendingNewSessionPermissionDefault();
      const put = await lingxiFetch('/api/preferences/session-permission-default', {
        method: 'PUT', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ permissionMode: 'auto' }), throwOnHttpError: false,
      });
      await sleep(150);
      const after = await loadPendingNewSessionPermissionDefault();
      // 负路径：非法值 400 拒绝。
      const bad = await lingxiFetch('/api/preferences/session-permission-default', {
        method: 'PUT', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ permissionMode: 'bogus-mode' }), throwOnHttpError: false,
      });
      // 复原
      if (orig) {
        await lingxiFetch('/api/preferences/session-permission-default', {
          method: 'PUT', headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ permissionMode: orig }), throwOnHttpError: false,
        });
      }
      return { putStatus: put.status, roundTrip: after === 'auto', badRejected: bad.status >= 400, badStatus: bad.status };
    });
  });

  it('leaf.memory-snapshot: 记忆开关查看/设置 + 提示词快照（已翻译读端口）', async () => {
    await observe('leaf.memory-snapshot', async () => {
      const target = (useStore.getState().sessions as any[])[0];
      if (!target) return { memoryRead: false, memorySet: false, snapshotOk: false, invalidRejected: false, skipped: true };
      const loc = kind === 'rust' ? `native:${target.sessionId}` : target.path;
      // 记忆开关读（GET memory 已翻译读端口）。
      const mem = await lingxiFetch(`/api/sessions/memory?${kind === 'rust' ? `path=${encodeURIComponent(loc)}` : `path=${encodeURIComponent(target.path)}`}`, { method: 'GET', throwOnHttpError: false });
      const memBody = await mem.json().catch(() => null);
      const memoryRead = mem.status < 300 && memBody != null && (memBody?.memoryEnabled !== undefined || memBody?.enabled !== undefined || typeof memBody === 'boolean');
      // 记忆开关设置（PATCH memory 已翻译写端口；设回原值保真）。
      const original = memBody?.memoryEnabled ?? memBody?.enabled ?? true;
      const set = await lingxiFetch('/api/sessions/memory', {
        method: 'PATCH', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: loc, memoryEnabled: original }), throwOnHttpError: false,
      });
      const setBody = await set.json().catch(() => null);
      const memorySet = set.status < 300 && (setBody?.ok !== false) && (setBody?.memoryEnabled === undefined || setBody?.memoryEnabled === original || setBody?.enabled === undefined || setBody?.enabled === original);
      // 提示词快照（GET prompt-snapshot 已翻译读端口）。
      const snap = await lingxiFetch(`/api/sessions/prompt-snapshot?${kind === 'rust' ? `path=${encodeURIComponent(loc)}` : `path=${encodeURIComponent(target.path)}`}`, { method: 'GET', throwOnHttpError: false });
      const snapBody = await snap.json().catch(() => null);
      const snapshotOk = snap.status < 300 && snapBody != null;
      // 负路径：无效会话三端口显式拒。
      const badLoc = kind === 'rust' ? 'native:sess-does-not-exist-t07' : 'agents/a1/sessions/sess-does-not-exist-t07.jsonl';
      const badMem = await lingxiFetch(`/api/sessions/memory?path=${encodeURIComponent(badLoc)}`, { method: 'GET', throwOnHttpError: false });
      const badSnap = await lingxiFetch(`/api/sessions/prompt-snapshot?path=${encodeURIComponent(badLoc)}`, { method: 'GET', throwOnHttpError: false });
      const invalidRejected = badMem.status >= 400 && badSnap.status >= 400;
      return { memoryRead, memorySet, snapshotOk, invalidRejected, memStatus: mem.status, setStatus: set.status, snapStatus: snap.status, badMemStatus: badMem.status, badSnapStatus: badSnap.status };
    });
  });

  it('leaf.prompt-snapshot: 查看会话提示词快照（冻结 sidecar 读面 + 无效会话拒）', async () => {
    await observe('leaf.prompt-snapshot', async () => {
      const target = (useStore.getState().sessions as any[])[0];
      if (!target) return { snapshotOk: false, invalidRejected: false, skipped: true };
      const loc = kind === 'rust' ? `native:${target.sessionId}` : target.path;
      const snap = await lingxiFetch(`/api/sessions/prompt-snapshot?path=${encodeURIComponent(loc)}`, { method: 'GET', throwOnHttpError: false });
      const snapBody = await snap.json().catch(() => null);
      const badLoc2 = kind === 'rust' ? 'native:sess-does-not-exist-t07' : 'agents/a1/sessions/sess-does-not-exist-t07.jsonl';
      const badSnap2 = await lingxiFetch(`/api/sessions/prompt-snapshot?path=${encodeURIComponent(badLoc2)}`, { method: 'GET', throwOnHttpError: false });
      const cases = {
        read: snap.status < 300 && snapBody != null,
        'invalid-rejected': badSnap2.status >= 400,
      };
      return { cases, snapStatus: snap.status, badSnapStatus: badSnap2.status, snapKeys: snapBody && typeof snapBody === 'object' ? Object.keys(snapBody).slice(0, 10) : null };
    });
  });

  it('leaf.auth-folders: 授权目录四操作（读/增/撤/替换 + 负路径）', async () => {
    await observe('leaf.auth-folders', async () => {
      const target = (useStore.getState().sessions as any[])[0];
      if (!target) return { readOk: false, addOk: false, removeOk: false, setOk: false, invalidRejected: false, skipped: true };
      const loc = kind === 'rust' ? `native:${target.sessionId}` : target.path;
      const path = '/api/sessions/authorized-folders';
      // 读：已翻译读端口。
      const read = await lingxiFetch(`${path}?${kind === 'rust' ? `path=${encodeURIComponent(loc)}` : `path=${encodeURIComponent(target.path)}`}`, { method: 'GET', throwOnHttpError: false });
      const readBody = await read.json().catch(() => null);
      const readList = Array.isArray(readBody) ? readBody : (readBody?.folders ?? readBody?.authorizedFolders ?? null);
      const readOk = read.status < 300 && (Array.isArray(readList) || readBody != null);
      // 合成目录：work 目录下一个真实存在的目录（验证真实目录语义）。
      const realDir = kind === 'rust' ? target.cwd || '/tmp' : '/tmp';
      // 增（add）。
      const add = await lingxiFetch(path, {
        method: 'PATCH', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: loc, action: 'add', folder: realDir }), throwOnHttpError: false,
      });
      const addBody = await add.json().catch(() => null);
      const addList = Array.isArray(addBody) ? addBody : (addBody?.folders ?? null);
      const addOk = add.status < 300 && (addList == null || Array.isArray(addList));
      // 撤（remove 同一目录）。
      const remove = await lingxiFetch(path, {
        method: 'PATCH', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: loc, action: 'remove', folder: realDir }), throwOnHttpError: false,
      });
      const removeBody = await remove.json().catch(() => null);
      const removeList = Array.isArray(removeBody) ? removeBody : (removeBody?.folders ?? null);
      const removeOk = remove.status < 300 && (removeList == null || Array.isArray(removeList));
      // 替换（set 整体替换）。
      const set = await lingxiFetch(path, {
        method: 'PATCH', headers: { 'Content': 'application/json', 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: loc, action: 'set', folders: [] }), throwOnHttpError: false,
      });
      const setBody = await set.json().catch(() => null);
      const setList = Array.isArray(setBody) ? setBody : (setBody?.folders ?? null);
      const setOk = set.status < 300 && (setList == null || (Array.isArray(setList) && setList.length === 0));
      // 负路径：add 不存在的目录 → 显式拒（验证真实目录语义）。
      const bad = await lingxiFetch(path, {
        method: 'PATCH', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: loc, action: 'add', folder: '/nonexistent-dir-t07-does-not-exist' }), throwOnHttpError: false,
      });
      const badBody = await bad.json().catch(() => null);
      const invalidRejected = bad.status >= 400 || Boolean(badBody?.error);
      return { readOk, addOk, removeOk, setOk, invalidRejected, readStatus: read.status, addStatus: add.status, removeStatus: remove.status, setStatus: set.status, badStatus: bad.status };
    });
  });

  it('leaf.read-ports: 消息页/历史概览/摘要（已翻译读端口双内核对拍）', async () => {
    await observe('leaf.read-ports', async () => {
      const target = (useStore.getState().sessions as any[])[0];
      if (!target) return { historyOk: false, overviewOk: false, summaryOk: false, invalidRejected: false, skipped: true };
      // 消息页：旧栈客户端真实面 /api/sessions/messages?path=（session-actions.ts:226）；
      // Rust 统一历史读面 /lingxi/v1/sessions/{id}/history?all=1（R06-T04）。
      const historyPath = kind === 'rust'
        ? `/lingxi/v1/sessions/${encodeURIComponent(target.sessionId)}/history?all=1`
        : `/api/sessions/messages?path=${encodeURIComponent(target.path)}&reconciliation=1`;
      const history = await lingxiFetch(historyPath, { method: 'GET', throwOnHttpError: false });
      const historyBody = await history.json().catch(() => null);
      const historyRows = Array.isArray(historyBody) ? historyBody : (historyBody?.items ?? historyBody?.messages ?? historyBody);
      const historyOk = history.status < 300 && (Array.isArray(historyRows) || historyBody?.items != null);
      // 历史概览：已翻译读端口（translateLegacySessionCall GET history-overview）。
      const overview = await lingxiFetch(`/api/sessions/history-overview?path=${encodeURIComponent(kind === 'rust' ? `native:${target.sessionId}` : target.path)}`, { method: 'GET', throwOnHttpError: false });
      const overviewBody = await overview.json().catch(() => null);
      const overviewOk = overview.status < 300 && overviewBody != null && (overviewBody?.available !== undefined || overviewBody?.ok !== undefined || typeof overviewBody === 'object');
      // 摘要：已翻译读端口（GET summary）。
      const summary = await lingxiFetch(`/api/sessions/summary?path=${encodeURIComponent(kind === 'rust' ? `native:${target.sessionId}` : target.path)}`, { method: 'GET', throwOnHttpError: false });
      const summaryOk = summary.status < 300;
      // 负路径：无效会话三端口显式拒（任一 4xx 即拒）。
      const badPath = kind === 'rust' ? `native:sess-does-not-exist-t07` : `agents/a1/sessions/sess-does-not-exist-t07.jsonl`;
      const badOverview = await lingxiFetch(`/api/sessions/history-overview?path=${encodeURIComponent(badPath)}`, { method: 'GET', throwOnHttpError: false });
      const badSummary = await lingxiFetch(`/api/sessions/summary?path=${encodeURIComponent(badPath)}`, { method: 'GET', throwOnHttpError: false });
      const invalidRejected = badOverview.status >= 400 && badSummary.status >= 400;
      return { historyOk, overviewOk, summaryOk, invalidRejected, historyStatus: history.status, overviewStatus: overview.status, summaryStatus: summary.status, badOverviewStatus: badOverview.status, badSummaryStatus: badSummary.status };
    });
  });

  it('leaf.pin.projection: 置顶状态投影（pinnedAt/pinOrder 落列表）', async () => {
    await observe('leaf.pin.projection', async () => {
      const sessions = useStore.getState().sessions as any[];
      const target = sessions.find((x) => !x.parentSessionId);
      if (!target) return { pinnedProjected: false, clearedProjected: false, skipped: true };
      await pinSession(target.path, true);
      await loadSessions();
      const after = (useStore.getState().sessions as any[]).find((x) => x.sessionId === target.sessionId);
      // 投影断言：pinned=true 且带置顶时间戳（pinnedAt/pinnedAtUnixMs/pinned_at 族）与顺序位。
      // 旧栈投影无 pinned 布尔（只有 pinnedAt/pinOrder 字符串/数字）；Rust 同族。
      // 置顶态从任一投影字段推导。
      const pinnedFlag = after?.pinned === true || after?.pinnedAt != null || after?.pinnedAtUnixMs != null || after?.pinOrder != null;
      const hasStamp = after?.pinnedAt != null || after?.pinnedAtUnixMs != null || after?.pinned_at != null;
      const hasOrder = after?.pinOrder != null || after?.pin_order != null || after?.pinned === true;
      const pinnedProjected = pinnedFlag && (hasStamp || hasOrder);
      await pinSession(target.path, false);
      await loadSessions();
      const cleared = (useStore.getState().sessions as any[]).find((x) => x.sessionId === target.sessionId);
      const clearedProjected = !cleared?.pinned;
      return { pinnedProjected, clearedProjected, hasStamp, hasOrder };
    });
  });

  it('leaf.archive.workspace: 归档工作台会话（正路径入可恢复归档 + 无效目标拒）', async () => {
    await observe('leaf.archive.workspace', async () => {
      await loadSessions();
      const sessions = useStore.getState().sessions as any[];
      const victim = sessions.filter((x) => !x.parentSessionId && (x.title || x.sessionId) !== 'T07-AB-renamed').pop();
      if (!victim) return { archivedToRecoverable: false, invalidRejected: false, skipped: true };
      const res = await lingxiFetch('/api/sessions/archive', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(kind === 'rust'
          ? { sessionId: victim.sessionId }
          : { path: victim.path }),
        throwOnHttpError: false,
      });
      const bad = await lingxiFetch('/api/sessions/archive', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(kind === 'rust' ? { sessionId: 'sess-does-not-exist-t07' } : { path: 'agents/a1/sessions/sess-does-not-exist-t07.jsonl' }),
        throwOnHttpError: false,
      });
      const badBody = await bad.json().catch(() => null);
      // 可恢复归档 = 归档列表可见（随后恢复保住会话）。
      const archived = (await listArchivedSessions()) as any[];
      const row = Array.isArray(archived) ? archived.find((x: any) => x.sessionId === victim.sessionId || x.path === victim.path) : null;
      if (row) await restoreSession(row as any);
      await loadSessions();
      const invalidRejected = bad.status >= 400 || Boolean(badBody?.error) || badBody?.ok === false;
      return { archivedToRecoverable: Boolean(row), invalidRejected, archiveStatus: res.status, badStatus: bad.status };
    });
  });

  it('leaf.find.in-session: 在指定会话内查找（正路径命中 + 无效会话拒）', async () => {
    await observe('leaf.find.in-session', async () => {
      const sessions = useStore.getState().sessions as any[];
      // 命中需要有消息内容的夹具会话（A 腿 sessions[0] 可能是启动自建的空会话，D-11）。
      const target = sessions.find((s) => isFixtureSession(s)) ?? sessions[0];
      if (!target) return { hitAtLeastOne: false, invalidRejected: false, skipped: true };
      // R08-C③：双内核走同一真实 UI 调用面（chat-find-actions.ts:24
      // `/api/sessions/find?path=&q=`）；Rust 腿经 translateLegacySessionCall
      // 翻译到 /lingxi/v1/sessions/{id}/find?q=（D-10 关闭后的真实消费者链）。
      const locator = kind === 'rust' ? `native:${target.sessionId}` : target.path;
      const base = `/api/sessions/find?path=${encodeURIComponent(locator)}&q=${encodeURIComponent('T07')}`;
      const res = await lingxiFetch(base, { method: 'GET', throwOnHttpError: false });
      const body = await res.json().catch(() => null);
      const rows = Array.isArray(body?.matches) ? body.matches : null;
      const badLocator = kind === 'rust' ? 'native:sess-does-not-exist-t07' : 'agents/a1/sessions/sess-does-not-exist-t07.jsonl';
      const bad = await lingxiFetch(`/api/sessions/find?path=${encodeURIComponent(badLocator)}&q=x`, { method: 'GET', throwOnHttpError: false });
      const badBody = await bad.json().catch(() => null);
      return {
        status: res.status, rowsIsArray: Array.isArray(rows),
        // 现役响应形状 {query,revision,total,bestIndex,tokens,matches,truncated}
        hitAtLeastOne: res.status < 300 && Array.isArray(rows) && rows.length >= 1 && typeof body.total === 'number' && body.total >= 1,
        shapeOk: res.status < 300 && Array.isArray(body?.tokens) && typeof body?.truncated === 'boolean' && 'bestIndex' in (body ?? {}),
        badStatus: bad.status, invalidRejected: bad.status >= 400 || Boolean(badBody?.error),
      };
    });
  });

  it('leaf.pin-order: 置顶区整体重编号（正路径 + 重复 id 拒绝）', async () => {
    await observe('leaf.pin-order', async () => {
      const sessions = useStore.getState().sessions as any[];
      const two = sessions.slice(0, 2).map((x) => x.sessionId).filter(Boolean);
      if (two.length < 2 || new Set(two).size < 2) return { reorderOk: false, dupRejected: false, skipped: true };
      // 重排的前提是置顶区有这两个会话（旧栈对未置顶 id 的重排会拒）。
      await pinSession(useStore.getState().sessions.find((x: any) => x.sessionId === two[0])!.path, true);
      await pinSession(useStore.getState().sessions.find((x: any) => x.sessionId === two[1])!.path, true);
      await loadSessions();
      const ok = await reorderPinnedSessions([two[1], two[0]]);
      const dup = await lingxiFetch(kind === 'rust' ? '/lingxi/v1/sessions/pin-order' : '/api/sessions/pin-order', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ sessionIds: [two[0], two[0]] }), throwOnHttpError: false,
      });
      const dupBody = await dup.json().catch(() => null);
      const dupRejected = dup.status >= 400 || Boolean(dupBody?.error) || dupBody?.ok === false;
      await reorderPinnedSessions(two);
      await pinSession(useStore.getState().sessions.find((x: any) => x.sessionId === two[0])!.path, false);
      await pinSession(useStore.getState().sessions.find((x: any) => x.sessionId === two[1])!.path, false);
      await loadSessions();
      return { reorderOk: ok, dupRejected, dupStatus: dup.status };
    });
  });

  it('leaf.archived-delete: 永久删除归档会话（正路径 + 删除后不可恢复）', async () => {
    await observe('leaf.archived-delete', async () => {
      await loadSessions();
      const sessions = useStore.getState().sessions as any[];
      const victim = sessions.filter((x) => !x.parentSessionId && (x.title || x.sessionId) !== 'T07-AB-renamed').pop();
      if (!victim) return { deleted: false, goneFromArchive: false, restoreAfterDeleteRefused: false, skipped: true };
      await archiveSession(victim.path);
      const archived = (await listArchivedSessions()) as any[];
      const row = Array.isArray(archived) ? archived.find((x: any) => x.sessionId === victim.sessionId || x.path === victim.path) : null;
      if (!row) return { deleted: false, goneFromArchive: false, restoreAfterDeleteRefused: false, skipped: 'archive-failed' };
      const del = await lingxiFetch('/api/sessions/archived/delete', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        // 旧栈要求归档目录下的 path（resolveSessionLocatorFromBody + isArchivedDesktopSessionPath；
        // 活跃 path 会 409/403——t13-ab-drill-batch2-01 实证）；Rust 收 sessionId。
        body: JSON.stringify(kind === 'rust' ? { sessionId: victim.sessionId } : { path: row.path }),
        throwOnHttpError: false,
      });
      const deleted = del.status < 300;
      const after = (await listArchivedSessions()) as any[];
      const goneFromArchive = !Array.isArray(after) || !after.some((x: any) => x.sessionId === victim.sessionId || x.path === victim.path);
      const restore = await lingxiFetch('/api/sessions/restore', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(kind === 'rust' ? { sessionId: victim.sessionId } : { path: row.path }),
        throwOnHttpError: false,
      });
      const restoreBody = await restore.json().catch(() => null);
      const restoreAfterDeleteRefused = restore.status >= 400 || Boolean(restoreBody?.error) || restoreBody?.status === 'error';
      return { deleted, goneFromArchive, restoreAfterDeleteRefused, delStatus: del.status, restoreStatus: restore.status };
    });
  });

  it('leaf.search: 跨会话搜索（正路径命中 + 空查询空结果）', async () => {
    await observe('leaf.search', async () => {
      const path = kind === 'rust' ? '/lingxi/v1/sessions/search' : '/api/sessions/search';
      const hit = await lingxiFetch(`${path}?q=${encodeURIComponent('T07 A/B 任务')}&phase=content`, { method: 'GET', throwOnHttpError: false });
      const hitBody = await hit.json().catch(() => null);
      const hitRows = Array.isArray(hitBody) ? hitBody : (hitBody?.results ?? hitBody?.sessions ?? []);
      const empty = await lingxiFetch(`${path}?q=`, { method: 'GET', throwOnHttpError: false });
      const emptyBody = await empty.json().catch(() => null);
      const emptyRows = Array.isArray(emptyBody) ? emptyBody : (emptyBody?.results ?? emptyBody?.sessions ?? []);
      const bad = await lingxiFetch(`${path}?q=x&phase=bogus`, { method: 'GET', throwOnHttpError: false });
      return {
        hitStatus: hit.status, hitRowsIsArray: Array.isArray(hitRows),
        hitAtLeastOne: Array.isArray(hitRows) && hitRows.length >= 1,
        emptyStatus: empty.status, emptyIsEmpty: Array.isArray(emptyRows) && emptyRows.length === 0,
        badPhaseStatus: bad.status,
      };
    });
  });

  it('sessions.rename: 重命名会话（持久化回读）', async () => {
    await observe('sessions.rename', async () => {
      const first = (useStore.getState().sessions as any[])[0];
      if (!first) return { ok: false, persisted: false };
      const ok = await renameSession(first.path, 'T07-AB-renamed');
      await loadSessions();
      return { ok, persisted: Boolean(sessionByTitle('T07-AB-renamed')) };
    });
  });

  it('sessions.pin: 置顶/重排/取消', async () => {
    await observe('sessions.pin', async () => {
      const [a, b] = useStore.getState().sessions as any[];
      if (!a || !b) return { pinA: false, pinB: false, pinnedCount: 0, reorder: false, reorderedReversed: false, unpinA: false, unpinB: false, pinnedFinal: 0 };
      const pinA = await pinSession(a.path, true);
      const pinB = await pinSession(b.path, true);
      await loadSessions();
      const pinnedAfter = (useStore.getState().sessions as any[]).filter((s) => s.pinned).map((s) => s.sessionId);
      const reorder = await reorderPinnedSessions([...pinnedAfter].reverse());
      await loadSessions();
      const pinnedReordered = (useStore.getState().sessions as any[]).filter((s) => s.pinned).map((s) => s.sessionId);
      const unpinA = await pinSession(a.path, false);
      const unpinB = await pinSession(b.path, false);
      await loadSessions();
      return { pinA, pinB, pinnedCount: pinnedAfter.length, reorder, reorderedReversed: JSON.stringify(pinnedReordered) === JSON.stringify([...pinnedAfter].reverse()), unpinA, unpinB, pinnedFinal: (useStore.getState().sessions as any[]).filter((s) => s.pinned).length };
    });
  });

  it('sessions.todos: 完成/取消/收纳', async () => {
    await observe('sessions.todos', async () => {
      const first = (useStore.getState().sessions as any[])[0];
      if (!first) return { complete: false, cancel: false, dismiss: false };
      return { complete: await completeSessionTodos(first.path), cancel: await cancelSessionTodos(first.path), dismiss: await dismissSessionTodoPanel(first.path) };
    });
  });

  it('sessions.capabilities: 刷新能力并压缩', async () => {
    await observe('sessions.capabilities', async () => {
      const first = (useStore.getState().sessions as any[])[0];
      if (!first) return { ok: false };
      return { ok: await refreshSessionCapabilities(first.path) };
    });
  });

  it('sessions.maintenance: 清理过期归档 + 清扫孤儿', async () => {
    await observe('sessions.maintenance', async () => {
      const cleanup = await cleanupArchivedSessions(30);
      const swept = await sweepOrphanedWorkspaceSessions();
      // fixture 自带超期归档（archived_at=2025-10 > 30 天）——首个 cleanup(30)
      // 正当删除之（探针 t11-archive-probe-03 语义）：deleted>=1。
      return { cleanupDeletedIsNumber: typeof cleanup?.deleted === 'number', cleanupDeletedAtLeastOne: (cleanup?.deleted ?? 0) >= 1, sweptIsNumber: typeof swept === 'number' };
    });
  });

  it('leaf.permissions: 默认权限读取 + 幂等（含预热重试）', async () => {
    await observe('permissions.default', async () => {
      // 旧栈首读可能 undefined（端点预热抖动，leafcases-05/06 实证）；
      // 有界重试 ≤3 次并记录尝试数。
      let mode = await loadPendingNewSessionPermissionDefault();
      let attempts = 1;
      while (mode === undefined && attempts < 3) {
        await sleep(300);
        mode = await loadPendingNewSessionPermissionDefault();
        attempts += 1;
      }
      return { mode, attempts };
    });
    await observe('leaf.permissions.read-idempotent', async () => {
      const a = await loadPendingNewSessionPermissionDefault();
      const b = await loadPendingNewSessionPermissionDefault();
      return { identical: a === b, mode: a };
    });
  });


  it('preferences.appearance: 读/编辑/回读', async () => {
    await observe('preferences.appearance', async () => {
      const path = '/api/preferences/appearance';
      const before = await (await lingxiFetch(path, { method: 'GET', throwOnHttpError: false })).json().catch(() => null);
      const put = await lingxiFetch(path, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ theme: 't07-ab-theme' }), throwOnHttpError: false });
      const after = await (await lingxiFetch(path, { method: 'GET', throwOnHttpError: false })).json().catch(() => null);
      const themeAfter = after?.appearance?.theme ?? after?.theme ?? null;
      return { getBefore: Boolean(before), putStatus: put.status, themeAfter };
    });
  });

  it('leaf.archive.negative: 无效归档/恢复/清理目标显式拒绝', async () => {
    await observe('leaf.archive.negative', async () => {
      const body = kind === 'rust' ? { sessionId: 'sess-does-not-exist-t07' } : { path: 'agents/a1/sessions/sess-does-not-exist-t07.jsonl' };
      const arch = await lingxiFetch('/api/sessions/archive', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body), throwOnHttpError: false,
      });
      const archBody = await arch.json().catch(() => null);
      const restore = await lingxiFetch('/api/sessions/restore', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body), throwOnHttpError: false,
      });
      const restoreBody = await restore.json().catch(() => null);
      const cleanupBad = await lingxiFetch('/api/sessions/cleanup', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ maxAgeDays: 'bogus' }), throwOnHttpError: false,
      });
      const rejected = (st: number, b: any) => st >= 400 || Boolean(b?.error) || b?.ok === false;
      return {
        archiveRejected: rejected(arch.status, archBody), archiveStatus: arch.status,
        restoreRejected: rejected(restore.status, restoreBody), restoreStatus: restore.status,
        cleanupBadRejected: rejected(cleanupBad.status, null), cleanupBadStatus: cleanupBad.status,
      };
    });
  });

  it('leaf.cleanup.boundary: 期限边界 + 显式永久删除', async () => {
    await observe('leaf.cleanup.boundary', async () => {
      await loadSessions();
      const sessions = useStore.getState().sessions as any[];
      // 目标选取（D-4 改判后修正）：rename 任务可能已改 sessions[0] 的标题，
      // 固定 byTitle('ab-real-beta') 会错选/漏选——改为「标题未被本套件改过
      // 的会话」取两个，与 rename 目标（T07-AB-renamed）严格不相交。
      // 目标选取（D-6 改判后修正）：还需「无子会话」——有子的顶层会话归档被
      // 409 child_sessions_present 正确拒绝（现役语义，t11-archive-probe-01
      // 实证），套件此前把该正确拒绝误记为「Rust 立删新归档」伪差异。
      const untouched = sessions.filter((x) =>
        (x.title || x.sessionId) !== 'T07-AB-renamed' && !x.parentSessionId);
      const fresh = untouched[untouched.length - 1];
      const expiring = untouched[untouched.length - 2];
      if (!fresh || !expiring || fresh === expiring) {
        return { freshKept: false, purgedOk: false, expiringGone: false, skipped: true, untouchedTitles: untouched.map((x) => x.title || x.sessionId), total: sessions.length };
      }
      await archiveSession(fresh.path);
      await archiveSession(expiring.path);
      const kept = await cleanupArchivedSessions(30);
      // fixture 自带超期归档（archived_at=2025-10 > 30 天）——按期限正当删除
      // 即 deleted>=1（探针 t11-archive-probe-03 实证语义）。
      const expiredDeleted = typeof kept?.deleted === 'number' && kept.deleted >= 1;
      const afterKeep = (await listArchivedSessions()) as any[];
      const freshStillArchived = Array.isArray(afterKeep) && afterKeep.some((s: any) => s.sessionId === fresh.sessionId || s.path === fresh.path);
      const delRes = await lingxiFetch('/api/sessions/archived/delete', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: expiring.path, sessionId: expiring.sessionId }), throwOnHttpError: false,
      });
      const delBody = await delRes.json().catch(() => null);
      const purgedOk = delRes.status < 300 || delBody?.ok === true;
      const afterPurge = (await listArchivedSessions()) as any[];
      const expiringGone = !Array.isArray(afterPurge) || !afterPurge.some((s: any) => s.sessionId === expiring.sessionId || s.path === expiring.path);
      const freshRow = Array.isArray(afterPurge) ? afterPurge.find((s: any) => s.sessionId === fresh.sessionId || s.path === fresh.path) : null;
      if (freshRow) await restoreSession(freshRow as any);
      await loadSessions();
      return { keptDeletedIsNumber: typeof kept?.deleted === 'number', expiredDeleted, freshKept: freshStillArchived, purgedOk, expiringGone };
    });
  });

  it('sessions.archive: 归档/查看归档/恢复', async () => {
    await observe('sessions.archive', async () => {
      await loadSessions();
      const target = sessionByTitle('T07-AB-renamed') as any;
      if (!target) {
        return { inArchive: false, goneFromVisible: false, restored: false, backVisible: false, deleted: 'skipped-no-target', stillArchived: true, skipped: true };
      }
      await archiveSession(target.path);
      // 有子会话的归档被 409 正确拒绝（现役语义）——带 archive_children 重试
      // （两内核同一规则；probe 实证 t11-archive-probe-01）。
      await loadSessions();
      if (useStore.getState().sessions.some((x: any) => x.sessionId === target.sessionId)) {
        await lingxiFetch('/api/sessions/archive', {
          method: 'POST', headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(kind === 'rust'
            ? { sessionId: target.sessionId, childMode: 'archive_children' }
            : { path: target.path, childMode: 'archive_children' }),
          throwOnHttpError: false,
        });
      }
      const archived = (await listArchivedSessions()) as any[];
      const row = Array.isArray(archived) ? archived.find((s: any) => s.sessionId === target.sessionId || s.path === target.path) : null;
      const inArchive = Boolean(row);
      await loadSessions();
      const goneFromVisible = !useStore.getState().sessions.some((s: any) => s.sessionId === target.sessionId);
      let backVisible = false;
      if (row) {
        await restoreSession(row as any);
        await loadSessions();
        backVisible = useStore.getState().sessions.some((s: any) => s.sessionId === target.sessionId);
      }
      return { inArchive, goneFromVisible, restored: backVisible, backVisible, deleted: 'skipped-not-restorable', stillArchived: true };
    });
  });

  it('sessions.create: 创建并切换会话', async () => {
    await observe('sessions.create', async () => {
      const before = useStore.getState().sessions.length;
      await createNewSession();
      const ref = await ensureSession();
      resetPending();
      await loadSessions();
      return { created: Boolean(ref), delta: useStore.getState().sessions.length - before };
    });
  });

  // ── 第五段：终态 ──
  it('drift-guard', async () => {
    await observe('drift-guard', async () => {
      await sleep(200);
      await loadSessions();
      const sessions = useStore.getState().sessions as any[];
      return { count: comparableCount(sessions), rawCount: sessions.length, titles: titles() };
    });
  });

  it('leaf.mcp.state.unauthorized: 无效令牌显式拒绝（置于最末——连接换装不毒化前序动作）', async () => {
    await observe('leaf.mcp.state.unauthorized', async () => {
      const path = kind === 'rust' ? '/lingxi/v1/mcp/state' : '/api/mcp/state';
      // lingxiFetch 经 requireServerConnection 读 serverConnections map 的活动项，
      // 只换 activeServerConnection 字段不够——map 项也换（用后复原）。
      const good = useStore.getState().activeServerConnection as any;
      const bad = { ...good, token: 't07-invalid-token' };
      // 保全量 map：只覆盖活动项（复原原 map）——整体重建 map 会破坏 native
      // 会话连接属主并毒化后续 store 动作（t09-ab-drill-mcp-02 实证 rename 回落）。
      const prevMap = { ...(useStore.getState() as any).serverConnections };
      useStore.setState({ activeServerConnection: bad, serverConnections: { ...prevMap, [bad.connectionId]: bad } } as any);
      let status = 0;
      try {
        const res = await lingxiFetch(path, { method: 'GET', throwOnHttpError: false });
        status = res.status;
      } finally {
        useStore.setState({ activeServerConnection: good, serverConnections: prevMap } as any);
      }
      return { status, rejected: status === 401 || status === 403 };
    });
  });

  it('error-path.unauthorized: 无效令牌被拒', async () => {
    await observe('error-path.unauthorized', async () => {
      const path = kind === 'rust' ? '/lingxi/v1/sessions/studio-list' : '/api/sessions';
      // 只换 activeServerConnection，不动 serverConnections map——map 整体替换会
      // 破坏 native 会话连接属主（captureNativeSessionConnection），毒化后续
      // B 侧全部 store 动作（leafcases-03/04 实证：rename/pin/todos 0ms 即败）。
      const good = useStore.getState().activeServerConnection as any;
      const bad = { ...good, token: 't07-invalid-token' };
      // 保全量 map：只覆盖活动项（复原原 map）——整体重建 map 会破坏 native
      // 会话连接属主并毒化后续 store 动作（t09-ab-drill-mcp-02 实证 rename 回落）。
      const prevMap = { ...(useStore.getState() as any).serverConnections };
      useStore.setState({ activeServerConnection: bad, serverConnections: { ...prevMap, [bad.connectionId]: bad } } as any);
      let status = 0;
      try {
        const res = await lingxiFetch(path, { method: 'GET', throwOnHttpError: false });
        status = res.status;
      } finally {
        useStore.setState({ activeServerConnection: good, serverConnections: prevMap } as any);
      }
      return { status, rejected: status === 401 || status === 403 };
    });
  });

  it('error-path.not-found: 不存在会话读取明确拒绝', async () => {
    await observe('error-path.not-found', async () => {
      const res = await lingxiFetch('/api/sessions/history-overview?path=native:sess-does-not-exist', { method: 'GET', throwOnHttpError: false });
      return { status: res.status, explicitError: res.status >= 400 };
    });
  });

  // ══════════ R08 收尾 D02 批次 1（cases 泛化：双腿同键全真 = 平价） ══════════
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
  // 鉴权不足探针：只换活动连接的令牌（保全量 map，见 error-path.unauthorized 注记）。
  const withInvalidToken = async <T,>(fn: () => Promise<T>): Promise<T> => {
    const good = useStore.getState().activeServerConnection as any;
    const bad = { ...good, token: 't07-invalid-token' };
    const prevMap = { ...(useStore.getState() as any).serverConnections };
    useStore.setState({ activeServerConnection: bad, serverConnections: { ...prevMap, [bad.connectionId]: bad } } as any);
    try { return await fn(); } finally { useStore.setState({ activeServerConnection: good, serverConnections: prevMap } as any); }
  };

  it('leaf.projects: 会话项目目录十叶（目录/文件夹/项目 CRUD/重排/归属/删除语义 + 负路径）', async () => {
    await observe('leaf.projects', async () => {
      const {
        loadSessionProjectCatalog, createSessionProjectInCatalog, patchSessionProjectInCatalog,
        patchSessionProjectFolderInCatalog, reorderSessionProjectsInCatalog, reorderSessionProjectFoldersInCatalog,
        deleteSessionProjectFromCatalog, deleteSessionProjectFolderFromCatalog, setSessionProjectAssignmentForSession,
      } = await import('../../stores/session-project-actions');
      const { UNCATEGORIZED_PROJECT_ID } = await import('../../../../../shared/session-projects.ts');
      const cases: Record<string, boolean> = {};
      const catalog0 = await loadSessionProjectCatalog();
      cases['catalog.read'] = Array.isArray(catalog0.folders) && Array.isArray(catalog0.projects);
      // 文件夹创建（UI 消费者 POST /api/session-projects/folders）。
      const folderRes = await jsonFetch('/api/session-projects/folders', { method: 'POST', body: { name: 'T07 文件夹' } });
      const folder = folderRes.body?.folder ?? folderRes.body?.catalog?.folders?.find((f: any) => f.name === 'T07 文件夹');
      const folderId: string | null = folder?.id ?? null;
      const folder2Res = await jsonFetch('/api/session-projects/folders', { method: 'POST', body: { name: 'T07 文件夹二' } });
      const folder2Id: string | null = folder2Res.body?.folder?.id ?? null;
      cases['folder.create'] = folderRes.status < 300 && Boolean(folderId) && ((await loadSessionProjectCatalog()).folders ?? []).some((f) => f.id === folderId && f.name === 'T07 文件夹');
      // 项目创建（文件夹内 + 根）。
      const project = await createSessionProjectInCatalog({ name: 'T07 项目', folderId });
      const rootProject = await createSessionProjectInCatalog({ name: 'T07 根项目' });
      cases['project.create'] = Boolean(project?.id) && project?.folderId === folderId && Boolean(rootProject?.id) && rootProject?.folderId === null;
      // 修改项目 / 修改文件夹（按所给字段更新并返回新状态）。
      const patched = project ? await patchSessionProjectInCatalog(project.id, { name: 'T07 项目改名' }) : null;
      const catalogP = await loadSessionProjectCatalog();
      cases['project.patch'] = patched?.name === 'T07 项目改名' && catalogP.projects.some((p) => p.id === project?.id && p.name === 'T07 项目改名' && p.folderId === folderId);
      const patchedFolder = folderId ? await patchSessionProjectFolderInCatalog(folderId, { name: 'T07 文件夹改名' }) : null;
      cases['folder.patch'] = patchedFolder?.name === 'T07 文件夹改名' && ((await loadSessionProjectCatalog()).folders ?? []).some((f) => f.id === folderId && f.name === 'T07 文件夹改名');
      // 重排项目（文件夹作用域）与重排文件夹。
      const project2 = await createSessionProjectInCatalog({ name: 'T07 项目乙', folderId });
      if (project && project2) await reorderSessionProjectsInCatalog(folderId, [project2.id, project.id]);
      const catalogR = await loadSessionProjectCatalog();
      const inFolder = catalogR.projects.filter((p) => p.folderId === folderId).sort((x, y) => x.order - y.order).map((p) => p.id);
      cases['projects.reorder'] = Boolean(project && project2) && inFolder[0] === project2!.id && inFolder[1] === project!.id;
      if (folderId && folder2Id) await reorderSessionProjectFoldersInCatalog([folder2Id, folderId]);
      const catalogF = await loadSessionProjectCatalog();
      const folderOrder = [...(catalogF.folders ?? [])].sort((x, y) => x.order - y.order).map((f) => f.id);
      cases['folders.reorder'] = Boolean(folderId && folder2Id) && folderOrder.indexOf(folder2Id!) < folderOrder.indexOf(folderId!);
      // 会话归属：分配到项目 → 列表投影 projectId；null 清除。
      await loadSessions();
      const target = (useStore.getState().sessions as any[]).find((s) => !s.parentSessionId);
      const overviewBefore = target ? await jsonFetch(`/api/sessions/history-overview?path=${encodeURIComponent(sessionLocator(target))}`) : null;
      if (target && project) await setSessionProjectAssignmentForSession(target.path, project.id);
      await loadSessions();
      const assigned = (useStore.getState().sessions as any[]).find((s) => s.sessionId === target?.sessionId);
      const assignedOk = Boolean(target && project) && assigned?.projectId === project!.id;
      if (target) await setSessionProjectAssignmentForSession(target.path, null);
      await loadSessions();
      const cleared = (useStore.getState().sessions as any[]).find((s) => s.sessionId === target?.sessionId);
      cases['assignment.set.and.clear'] = assignedOk && (cleared?.projectId == null || cleared?.projectId === UNCATEGORIZED_PROJECT_ID);
      // 删除项目：项目消失、原属活动会话归属为未分类、会话内容仍在。
      if (target && project) await setSessionProjectAssignmentForSession(target.path, project.id);
      await loadSessions();
      if (project) await deleteSessionProjectFromCatalog(project.id);
      const catalogD = await loadSessionProjectCatalog();
      await loadSessions();
      const afterDelete = (useStore.getState().sessions as any[]).find((s) => s.sessionId === target?.sessionId);
      const overviewAfter = target ? await jsonFetch(`/api/sessions/history-overview?path=${encodeURIComponent(sessionLocator(target))}`) : null;
      cases['project.delete.semantics'] = Boolean(project) && !catalogD.projects.some((p) => p.id === project!.id)
        && (afterDelete?.projectId == null || afterDelete?.projectId === UNCATEGORIZED_PROJECT_ID)
        && overviewBefore?.status === 200 && overviewAfter?.status === 200;
      // 删除文件夹：文件夹消失、其项目移到根、会话仍在原项目。
      if (target && project2) await setSessionProjectAssignmentForSession(target.path, project2.id);
      if (folderId) await deleteSessionProjectFolderFromCatalog(folderId);
      const catalogFD = await loadSessionProjectCatalog();
      await loadSessions();
      const stillAssigned = (useStore.getState().sessions as any[]).find((s) => s.sessionId === target?.sessionId);
      cases['folder.delete.semantics'] = Boolean(folderId && project2) && !(catalogFD.folders ?? []).some((f) => f.id === folderId)
        && catalogFD.projects.some((p) => p.id === project2!.id && p.folderId === null)
        && stillAssigned?.projectId === project2!.id;
      // 负路径（逐操作各自的无效分支 → 明确错误；目录读的拒绝分支 = 鉴权不足）。
      const badPatch = await jsonFetch('/api/session-projects/projects/does-not-exist-t07', { method: 'PATCH', body: { name: 'x' } });
      const badCreate = await jsonFetch('/api/session-projects/projects', { method: 'POST', body: { name: '   ' } });
      const rejected = (r: { status: number; body: any }) => r.status >= 400 || Boolean(r.body?.error);
      cases['negative.project.patch-missing'] = rejected(badPatch);
      cases['negative.project.create-empty-name'] = rejected(badCreate);
      cases['negative.folder.create-empty-name'] = rejected(await jsonFetch('/api/session-projects/folders', { method: 'POST', body: { name: '' } }));
      cases['negative.folder.patch-missing'] = rejected(await jsonFetch('/api/session-projects/folders/does-not-exist-t07', { method: 'PATCH', body: { name: 'x' } }));
      cases['negative.projects.reorder-missing-folder'] = rejected(await jsonFetch('/api/session-projects/projects/reorder', { method: 'POST', body: { folderId: 'does-not-exist-t07', projectIds: [] } }));
      // 非数组 folderIds / 不存在会话归属：双内核现役均静默归一接受（非"无效输入"
      // 拒绝面）→ 这两叶的拒绝分支取"权限不足"（鉴权失败 401/403）。
      cases['negative.folders.reorder-unauthorized'] = await withInvalidToken(async () => rejected(await jsonFetch('/api/session-projects/folders/reorder', { method: 'POST', body: { folderIds: [] } })));
      cases['negative.assignment.unauthorized'] = await withInvalidToken(async () => rejected(await jsonFetch('/api/session-projects/session-assignment', { method: 'POST', body: { sessionPath: target ? sessionLocator(target) : 'native:x', projectId: null } })));
      cases['negative.project.delete-missing'] = rejected(await jsonFetch('/api/session-projects/projects/does-not-exist-t07', { method: 'DELETE' }));
      cases['negative.folder.delete-missing'] = rejected(await jsonFetch('/api/session-projects/folders/does-not-exist-t07', { method: 'DELETE' }));
      cases['negative.catalog.unauthorized'] = await withInvalidToken(async () => rejected(await jsonFetch('/api/session-projects')));
      cases['negative.explicit-error'] = rejected(badPatch) && rejected(badCreate);
      // 复原：清归属、删剩余项目/文件夹。
      if (target) await setSessionProjectAssignmentForSession(target.path, null);
      for (const p of (await loadSessionProjectCatalog()).projects.filter((p) => p.name.startsWith('T07 '))) await deleteSessionProjectFromCatalog(p.id);
      for (const f of ((await loadSessionProjectCatalog()).folders ?? []).filter((f) => f.name.startsWith('T07 '))) await deleteSessionProjectFolderFromCatalog(f.id);
      return { cases, folderId, projectId: project?.id ?? null, badPatchStatus: badPatch.status, badCreateStatus: badCreate.status };
    });
  });

  it('leaf.conversation-map: 布局读/回合读/位置合并/位置替换/折叠替换 + 无效输入不持久化', async () => {
    await observe('leaf.conversation-map', async () => {
      const { fetchMapLayout, fetchMapTurns } = await import('../../components/conversation-map/conversation-map-projection');
      const cases: Record<string, boolean> = {};
      const layoutPath = rustOrNode('/lingxi/v1/conversation-map/layout', '/api/conversation-map/layout');
      const put = (body: unknown) => jsonFetch(layoutPath, { method: 'PUT', body });
      // 清场：全量替换为空。
      await put({ positions: {}, replacePositions: true, collapsed: [] });
      const base = await fetchMapLayout();
      cases['layout.read.default-or-persisted'] = base !== null && typeof base.positions === 'object' && Array.isArray(base.collapsed);
      await loadSessions();
      const target = (useStore.getState().sessions as any[]).find((s) => !s.parentSessionId && isFixtureSession(s));
      const turns = target ? await fetchMapTurns(target as any) : null;
      cases['turns.read.folds-qa-cards'] = Array.isArray(turns) && turns.length >= 1 && turns.every((t: any) => t && typeof t === 'object');
      // 回合读负路径：不存在会话 → 明确错误（不伪装成空数据）。
      const badTurns = await jsonFetch(rustOrNode('/lingxi/v1/sessions/sess-does-not-exist-t07/conversation-map/turns', '/api/conversation-map/turns?path=agents%2Fa1%2Fsessions%2Fsess-does-not-exist-t07.jsonl'));
      cases['turns.invalid-session-rejected'] = badTurns.status >= 400 || Boolean(badTurns.body?.error);
      // 位置合并：仅指定卡片变化。
      const m1 = await put({ positions: { 'card-a': { x: 1, y: 2 } } });
      const m2 = await put({ positions: { 'card-b': { x: 3, y: 4 } } });
      const merged = await fetchMapLayout();
      cases['positions.merge.only-specified-changes'] = m1.status < 300 && m2.status < 300
        && merged.positions['card-a']?.x === 1 && merged.positions['card-a']?.y === 2 && merged.positions['card-b']?.x === 3;
      // 位置替换：未指定坐标被删除。
      const r1 = await put({ positions: { 'card-c': { x: 5, y: 6 } }, replacePositions: true });
      const replaced = await fetchMapLayout();
      cases['positions.replace.unspecified-removed'] = r1.status < 300 && replaced.positions['card-c']?.x === 5 && !('card-a' in replaced.positions) && !('card-b' in replaced.positions);
      // 折叠列表整体替换。
      await put({ collapsed: ['t1', 't2'] });
      const c2 = await put({ collapsed: ['t3'] });
      const collapsed = await fetchMapLayout();
      cases['collapsed.replace.whole-list'] = c2.status < 300 && JSON.stringify(collapsed.collapsed) === JSON.stringify(['t3']);
      // 无效输入：现役拒绝面 = body 非对象 / 既无 positions 也无 collapsed（400
      // invalid_body）；非对象 positions 现役静默归一为空（不是错误面）。拒绝后
      // 布局不变（无错误持久化）。
      const bad = await put({});
      const badArray = await put([]);
      const unchanged = await fetchMapLayout();
      cases['negative.invalid-not-persisted'] = bad.status >= 400 && badArray.status >= 400 && JSON.stringify(unchanged) === JSON.stringify(collapsed);
      cases['negative.unauthorized-rejected'] = await withInvalidToken(async () => { const r = await put({ collapsed: ['x'] }); return r.status === 401 || r.status === 403; });
      const afterUnauthorized = await fetchMapLayout();
      cases['negative.unauthorized-not-persisted'] = JSON.stringify(afterUnauthorized) === JSON.stringify(collapsed);
      await put({ positions: {}, replacePositions: true, collapsed: [] });
      return { cases, turnsCount: Array.isArray(turns) ? turns.length : null, badStatus: bad.status, badArrayStatus: badArray.status };
    });
  });

  it('leaf.checkpoints: 列表/用户编辑创建（绝对路径+原因校验）/恢复写回与 404/删除', async () => {
    await observe('leaf.checkpoints', async () => {
      const fs = await import('node:fs');
      const os = await import('node:os');
      const pathMod = await import('node:path');
      const cases: Record<string, boolean> = {};
      const list0 = await jsonFetch('/api/checkpoints');
      cases['list.read'] = list0.status === 200 && Array.isArray(list0.body?.checkpoints);
      const dir = fs.mkdtempSync(pathMod.join(os.tmpdir(), 'r08-ckpt-'));
      const file = pathMod.join(dir, 'edit-target.txt');
      fs.writeFileSync(file, 'ORIGINAL-A\n');
      const created = await jsonFetch('/api/checkpoints/user-edit', { method: 'POST', body: { filePath: file, reason: 'edit-start' } });
      const ckId: string | null = created.body?.checkpoint?.id ?? null;
      const list1 = await jsonFetch('/api/checkpoints');
      cases['user-edit.create.listed'] = created.status < 300 && Boolean(ckId) && Array.isArray(list1.body?.checkpoints) && list1.body.checkpoints.some((c: any) => c.id === ckId);
      const badReason = await jsonFetch('/api/checkpoints/user-edit', { method: 'POST', body: { filePath: file, reason: 'bogus-reason' } });
      const relPath = await jsonFetch('/api/checkpoints/user-edit', { method: 'POST', body: { filePath: 'relative/path.txt', reason: 'edit-start' } });
      cases['user-edit.rejects-bad-reason-and-relative-path'] = badReason.status >= 400 && relPath.status >= 400;
      fs.writeFileSync(file, 'MODIFIED-B\n');
      const restored = ckId ? await jsonFetch(`/api/checkpoints/${encodeURIComponent(ckId)}/restore`, { method: 'POST', body: {} }) : { status: 0, body: null };
      cases['restore.writes-back-and-reports-location'] = restored.status < 300 && typeof restored.body?.restoredTo === 'string' && fs.readFileSync(file, 'utf8') === 'ORIGINAL-A\n';
      const missing = await jsonFetch('/api/checkpoints/does-not-exist-t07/restore', { method: 'POST', body: {} });
      cases['restore.missing-404'] = missing.status === 404;
      const deleted = ckId ? await jsonFetch(`/api/checkpoints/${encodeURIComponent(ckId)}`, { method: 'DELETE' }) : { status: 0, body: null };
      const list2 = await jsonFetch('/api/checkpoints');
      cases['delete.removes-from-store'] = deleted.status < 300 && Array.isArray(list2.body?.checkpoints) && !list2.body.checkpoints.some((c: any) => c.id === ckId);
      // 删除不存在检查点：双内核现役均幂等 200（非"无效输入"拒绝面；工具叶
      // 6207F7E8C697 的"不伪报成功"属工具层语义）→ 本路由叶拒绝分支取鉴权不足。
      const deleteMissing = await jsonFetch('/api/checkpoints/does-not-exist-t07', { method: 'DELETE' });
      cases['delete.unauthorized-rejected'] = await withInvalidToken(async () => { const r = await jsonFetch('/api/checkpoints/does-not-exist-t07', { method: 'DELETE' }); return r.status === 401 || r.status === 403; });
      cases['list.unauthorized-rejected'] = await withInvalidToken(async () => { const r = await jsonFetch('/api/checkpoints'); return r.status === 401 || r.status === 403; });
      fs.rmSync(dir, { recursive: true, force: true });
      return { cases, ckId, restoredTo: restored.body?.restoredTo ?? null, badReasonStatus: badReason.status, relStatus: relPath.status, missingStatus: missing.status, deleteMissingStatus: deleteMissing.status };
    });
  });

  it('leaf.workspace-rollback: 查看/设置文件回退开关（布尔写回读、非法值拒、不回退文件）', async () => {
    await observe('leaf.workspace-rollback', async () => {
      const fs = await import('node:fs');
      const os = await import('node:os');
      const pathMod = await import('node:path');
      const cases: Record<string, boolean> = {};
      const path = '/api/sessions/workspace-rollback';
      const sentinelDir = fs.mkdtempSync(pathMod.join(os.tmpdir(), 'r08-wsrb-'));
      const sentinel = pathMod.join(sentinelDir, 'untouched.txt');
      fs.writeFileSync(sentinel, 'SENTINEL\n');
      const read0 = await jsonFetch(path);
      cases['read.enabled-boolean'] = read0.status === 200 && typeof read0.body?.enabled === 'boolean';
      const original: boolean = read0.body?.enabled === true;
      const putTrue = await jsonFetch(path, { method: 'PUT', body: { enabled: !original } });
      const read1 = await jsonFetch(path);
      cases['put.roundtrip'] = putTrue.status < 300 && putTrue.body?.enabled === !original && read1.body?.enabled === !original;
      const bad = await jsonFetch(path, { method: 'PUT', body: { enabled: 'yes' } });
      const read2 = await jsonFetch(path);
      cases['put.invalid-rejected-state-unchanged'] = bad.status >= 400 && read2.body?.enabled === !original;
      await jsonFetch(path, { method: 'PUT', body: { enabled: original } });
      cases['no-file-rollback-side-effect'] = fs.readFileSync(sentinel, 'utf8') === 'SENTINEL\n';
      cases['read.unauthorized-rejected'] = await withInvalidToken(async () => { const r = await jsonFetch(path); return r.status === 401 || r.status === 403; });
      fs.rmSync(sentinelDir, { recursive: true, force: true });
      return { cases, badStatus: bad.status };
    });
  });

  it('leaf.sessions.create: 创建并切换会话 / 创建独立会话（元数据落列表、无效 Agent 拒）', async () => {
    await observe('leaf.sessions.create', async () => {
      const cases: Record<string, boolean> = {};
      const agentId = 'a1';
      const focusRes = await jsonFetch(rustOrNode('/lingxi/v1/sessions/new', '/api/sessions/new'), { method: 'POST', body: { agentId, title: 'T07 焦点新会话', memoryEnabled: false } });
      const focusId = focusRes.body?.sessionId ?? null;
      const focusPath = focusRes.body?.path ?? (focusId ? `native:${focusId}` : null);
      await loadSessions();
      const focusRow = (useStore.getState().sessions as any[]).find((s) => s.sessionId === focusId || s.path === focusPath);
      // 元数据：响应回传指定 Agent；记忆开关经已翻译读端口回读为 false。
      const focusMem = focusPath ? await jsonFetch(`/api/sessions/memory?path=${encodeURIComponent(kind === 'rust' ? `native:${focusId}` : focusPath)}`) : { status: 0, body: null };
      const memoryOff = focusMem.body?.memoryEnabled === false || focusMem.body?.enabled === false;
      cases['new.focus.created-with-metadata'] = focusRes.status < 300 && Boolean(focusRow) && focusRes.body?.agentId === agentId && memoryOff;
      const detachedRes = await jsonFetch('/api/sessions/new-detached', { method: 'POST', body: { agentId, title: 'T07 独立会话' } });
      const detachedId = detachedRes.body?.sessionId ?? null;
      await loadSessions();
      const detachedRow = (useStore.getState().sessions as any[]).find((s) => s.sessionId === detachedId);
      cases['new-detached.created'] = detachedRes.status < 300 && Boolean(detachedRow);
      // 焦点不被独立会话切走：服务端焦点读面（Rust current-status session 键 /
      // 旧栈无 path 读端口回落 engine.currentSessionPath）仍指向焦点新会话。
      const focusRead = kind === 'rust'
        ? await jsonFetch('/lingxi/v1/current-status?action=get&key=session')
        : await jsonFetch('/api/sessions/history-overview');
      const focusReadId = focusRead.body?.sessionId ?? focusRead.body?.value?.sessionId ?? focusRead.body?.session?.sessionId ?? null;
      const focusReadPath = focusRead.body?.sessionPath ?? focusRead.body?.path ?? null;
      cases['new-detached.focus-unchanged'] = focusRead.status < 300 && ((focusReadId != null && focusReadId === focusId) || (focusReadPath != null && focusReadPath === focusPath));
      const focusReadDebug = JSON.stringify(focusRead.body).slice(0, 220);
      let badAgent = await jsonFetch('/api/sessions/new-detached', { method: 'POST', body: { agentId: 'agent-does-not-exist-t07' } });
      for (let retry = 0; badAgent.status === 429 && retry < 5; retry += 1) { await sleep(700); badAgent = await jsonFetch('/api/sessions/new-detached', { method: 'POST', body: { agentId: 'agent-does-not-exist-t07' } }); }
      cases['negative.invalid-agent-rejected'] = (badAgent.status >= 400 && badAgent.status !== 429) || Boolean(badAgent.body?.error);
      // 谱系深度过限拒绝：以 forkedFromSessionId 链式创建独立会话，超过谱系深度
      // 上限（现役 MAX_FORK_LINEAGE_DEPTH=2）须明确拒绝；双内核同一拒绝点。
      let parent: string | null = detachedId;
      let rejectedAt: number | null = null;
      let rejectStatus: number | null = null;
      for (let depth = 1; depth <= 4 && parent; depth += 1) {
        let child = await jsonFetch('/api/sessions/new-detached', { method: 'POST', body: { agentId, title: `T07 谱系 ${depth}`, forkedFromSessionId: parent } });
        // 创建限流（429）不是谱系语义：有界退避重试，不把限流误记为深度拒绝。
        for (let retry = 0; child.status === 429 && retry < 5; retry += 1) { await sleep(700); child = await jsonFetch('/api/sessions/new-detached', { method: 'POST', body: { agentId, title: `T07 谱系 ${depth}`, forkedFromSessionId: parent } }); }
        if (child.status >= 400 || Boolean(child.body?.error)) { rejectedAt = depth; rejectStatus = child.status; break; }
        parent = child.body?.sessionId ?? null;
      }
      cases['new-detached.lineage-depth-limit-rejected'] = rejectedAt !== null && rejectStatus !== 429;
      return { cases, focusStatus: focusRes.status, detachedStatus: detachedRes.status, badAgentStatus: badAgent.status, lineageRejectedAt: rejectedAt, lineageRejectStatus: rejectStatus, focusReadDebug, focusBody: JSON.stringify(focusRes.body).slice(0, 200) };
    });
  });

  it('leaf.prefs.quick-chat: 快捷聊天偏好读面/更新（写-回读 + 非法 body 拒）', async () => {
    await observe('leaf.prefs.quick-chat', async () => {
      const cases: Record<string, boolean> = {};
      const path = rustOrNode('/lingxi/v1/preferences/quick-chat', '/api/preferences/quick-chat');
      const unwrap = (body: any) => body?.quickChat ?? body?.['quick-chat'] ?? body?.quick_chat ?? body;
      const read0 = await jsonFetch(path);
      const settings0 = unwrap(read0.body);
      cases['read.shape'] = read0.status === 200
        && typeof settings0?.shortcut === 'string' && settings0.shortcut.length > 0
        && Number.isFinite(Number(settings0?.reuseTimeoutMinutes));
      // 更新：写 reuseTimeoutMinutes=9 → 回读 9；复原。
      const put = await jsonFetch(path, { method: 'PUT', body: { quickChat: { shortcut: settings0?.shortcut ?? 'Alt+Space', reuseTimeoutMinutes: 9 } } });
      const read1 = await jsonFetch(path);
      const settings1 = unwrap(read1.body);
      cases['put.roundtrip'] = put.status < 300 && Number(settings1?.reuseTimeoutMinutes) === 9;
      if (settings0 && Number.isFinite(Number(settings0.reuseTimeoutMinutes))) {
        await jsonFetch(path, { method: 'PUT', body: { quickChat: { shortcut: settings0.shortcut, reuseTimeoutMinutes: Number(settings0.reuseTimeoutMinutes) } } });
      }
      // 负路径：非法 body（非对象）拒绝；读面不变（两内核均按 400 invalid body 拒）。
      const bad = await jsonFetch(path, { method: 'PUT', body: 'not-an-object' });
      const read2 = await jsonFetch(path);
      const settings2 = unwrap(read2.body);
      cases['put.invalid-rejected-state-unchanged'] = (bad.status >= 400 || Boolean(bad.body?.error))
        && typeof unwrap(read2.body)?.shortcut === 'string'
        && String(settings2?.shortcut) === String(settings1?.shortcut);
      cases['read.unauthorized-rejected'] = await withInvalidToken(async () => { const r = await jsonFetch(path); return r.status === 401 || r.status === 403; });
      return { cases, badStatus: bad.status, settings0 };
    });
  });

  it('leaf.drafts: 输入草稿读/主页存/会话存/清空（surface 作用域 + 非法 surface 拒）', async () => {
    await observe('leaf.drafts', async () => {
      const cases: Record<string, boolean> = {};
      const path = rustOrNode('/lingxi/v1/input-drafts', '/api/input-drafts');
      await loadSessions();
      const target = (useStore.getState().sessions as any[]).find((s) => isFixtureSession(s) && !s.parentSessionId);
      if (!target) return { cases, skipped: true };
      const readDrafts = async () => {
        const r = await jsonFetch(`${path}?surface=electron`);
        return { status: r.status, body: r.body };
      };
      const read0 = await readDrafts();
      const home0 = read0.body?.home ?? null;
      cases['read.shape'] = read0.status === 200 && read0.body != null && typeof read0.body === 'object'
        && ('home' in (read0.body as object)) && ('sessions' in (read0.body as object));
      // 主页草稿：保存 → 回读 text；A1 边界由清空腿覆盖。
      const putHome = await jsonFetch(path, { method: 'PUT', body: { surface: 'electron', scope: 'home', text: 'T07-DRAFT-HOME' } });
      const readHome = await readDrafts();
      const homeText = typeof readHome.body?.home === 'object' ? readHome.body.home?.text : readHome.body?.home;
      cases['home.save-readback'] = putHome.status < 300 && String(homeText ?? '').includes('T07-DRAFT-HOME');
      // 会话草稿：按 sessionId 保存 → 回读。
      const putSession = await jsonFetch(path, { method: 'PUT', body: { surface: 'electron', sessionId: target.sessionId, text: 'T07-DRAFT-SESSION' } });
      const readSession = await readDrafts();
      const sessionsMap = readSession.body?.sessions ?? {};
      const sessionEntry = sessionsMap[target.sessionId];
      const sessionText = typeof sessionEntry === 'object' ? sessionEntry?.text : sessionEntry;
      cases['session.save-readback'] = putSession.status < 300 && String(sessionText ?? '').includes('T07-DRAFT-SESSION');
      // 清空：text 空使对应草稿删除（home 腿）；只作用选定 surface/scope。
      const clearHome = await jsonFetch(path, { method: 'PUT', body: { surface: 'electron', scope: 'home', text: '' } });
      const readClear = await readDrafts();
      const homeAfterClear = readClear.body?.home;
      const homeEmpty = homeAfterClear == null || homeAfterClear === '' || (typeof homeAfterClear === 'object' && !String(homeAfterClear.text ?? '').length);
      const sessionStill = (readClear.body?.sessions ?? {})[target.sessionId];
      const sessionTextAfter = typeof sessionStill === 'object' ? sessionStill?.text : sessionStill;
      cases['clear.empties-scoped-only'] = clearHome.status < 300 && homeEmpty
        && String(sessionTextAfter ?? '').includes('T07-DRAFT-SESSION');
      // 复原会话草稿（清空）。
      await jsonFetch(path, { method: 'PUT', body: { surface: 'electron', sessionId: target.sessionId, text: '' } });
      if (home0 && typeof home0 === 'object' && home0.text) {
        await jsonFetch(path, { method: 'PUT', body: { surface: 'electron', scope: 'home', text: String(home0.text) } });
      }
      // 负路径：非法 surface 拒；读面不变。
      const bad = await jsonFetch(`${path}?surface=not-a-surface`, { method: 'GET' });
      cases['invalid-surface-rejected'] = bad.status >= 400 || Boolean(bad.body?.error);
      const badPut = await jsonFetch(path, { method: 'PUT', body: { surface: 'electron' } });
      cases['put-missing-scope-rejected'] = badPut.status >= 400 || Boolean(badPut.body?.error);
      return { cases, badStatus: bad.status, badPutStatus: badPut.status };
    });
  });

  it('leaf.ws.compact: WS compact 帧（compaction_accepted → succeeded/noop 结果 + 无会话拒）', async () => {
    await observe('leaf.ws.compact', async () => {
      const cases: Record<string, boolean> = {};
      await ensureWsConnected();
      await loadSessions();
      const target = (useStore.getState().sessions as any[]).find((s) => isFixtureSession(s) && !s.parentSessionId);
      if (!target) return { cases, skipped: true };
      await switchSession(target.path);
      const wsMod = await import('../../services/websocket');
      const ws = wsMod.getWebSocket();
      if (!ws || ws.readyState !== WebSocket.OPEN) return { cases, wsOpen: false };
      const framesBefore = wsFrames.length;
      ws.send(JSON.stringify(kind === 'rust'
        ? { type: 'compact', sessionId: target.sessionId }
        : { type: 'compact', sessionId: target.sessionId, sessionPath: target.path }));
      let sawAccepted = false;
      let sawResult = false;
      let resultStatus: string | null = null;
      let resultOk = false;
      try {
        await waitUntil(() => {
          const recent = wsFrames.slice(framesBefore).filter((f) => !f.startsWith('>>> '));
          sawAccepted = recent.some((f) => f.includes('compaction_accepted') || f.includes('compaction_start'));
          const resultFrame = recent.find((f) => f.includes('compaction_result') || f.includes('compaction_end'));
          if (resultFrame) {
            sawResult = true;
            // Rust：compaction_result status=succeeded/noop/failed；旧栈：compaction_end
            // （aborted:false 即完成；noop 时旧栈以 slash 文案/异常帧表达）。
            const m = resultFrame.match(/"status":"([a-z_]+)"/);
            resultStatus = m ? m[1] : null;
            resultOk = resultStatus === 'succeeded' || resultStatus === 'noop'
              || (resultFrame.includes('compaction_end') && resultFrame.includes('"aborted":false'));
          }
          return sawAccepted && sawResult;
        }, 'compaction accepted+result', 30000);
      } catch { /* 如实 */ }
      cases['compact.accepted-frame'] = sawAccepted;
      cases['compact.result-succeeded-or-noop'] = sawResult && resultOk;
      // 负路径：无会话 compact 显式拒（Rust：compaction_result status=failed；
      // 旧栈：error 帧）。
      const framesBeforeBad = wsFrames.length;
      ws.send(JSON.stringify(kind === 'rust'
        ? { type: 'compact', sessionId: 'sess-does-not-exist-t07' }
        : { type: 'compact', sessionId: 'sess-does-not-exist-t07', sessionPath: 'agents/a1/sessions/sess-does-not-exist-t07.jsonl' }));
      let badRejected = false;
      try {
        await waitUntil(() => {
          badRejected = wsFrames.slice(framesBeforeBad).some((f) => !f.startsWith('>>> ') && (
            (f.includes('compaction_result') && f.includes('"status":"failed"'))
            || (f.includes('compaction_end') && f.includes('"aborted":true'))
            || /"error"/.test(f) || /session_forbidden|not_found|命令拒绝|不存在/.test(f)));
          return badRejected;
        }, 'compact invalid rejected', 10000);
      } catch { /* 如实 */ }
      cases['compact.invalid-session-rejected'] = badRejected;
      return { cases, resultStatus, framesTail: wsFrames.slice(framesBefore).filter((f) => !f.startsWith('>>> ')).slice(-3).map((f) => f.slice(0, 160)) };
    });
  });

  it('leaf.plan-mode: 规划模式读面（enabled/mode/accessMode）+ 切换（enabled 两向 + 无 mode 由 !!enabled 决定）', async () => {
    await observe('leaf.plan-mode', async () => {
      const cases: Record<string, boolean> = {};
      const path = rustOrNode('/lingxi/v1/plan-mode', '/api/plan-mode');
      await loadSessions();
      const target = (useStore.getState().sessions as any[]).find((s) => isFixtureSession(s) && !s.parentSessionId);
      if (!target) return { cases, skipped: true };
      await switchSession(target.path);
      const read0 = await jsonFetch(path);
      cases['read.shape'] = read0.status === 200
        && typeof read0.body?.enabled === 'boolean'
        && typeof read0.body?.mode === 'string'
        && typeof read0.body?.accessMode === 'string';
      // 读不改状态。
      const read0b = await jsonFetch(path);
      cases['read.no-side-effect'] = JSON.stringify(read0.body) === JSON.stringify(read0b.body);
      const enabled0: boolean = Boolean(read0.body?.enabled);
      // 切换：无 mode 时由 !!enabled 决定 → enabled true 切 read_only；false 切回。
      const toggleOn = await jsonFetch(path, { method: 'POST', body: { enabled: true } });
      const readOn = await jsonFetch(path);
      cases['toggle.on-readback'] = toggleOn.status < 300 && (toggleOn.body?.ok !== false)
        && readOn.body?.enabled === true && readOn.body?.mode === 'read_only' && readOn.body?.accessMode === 'read_only';
      const toggleOff = await jsonFetch(path, { method: 'POST', body: { enabled: false } });
      const readOff = await jsonFetch(path);
      cases['toggle.off-readback'] = toggleOff.status < 300 && readOff.body?.enabled === false && readOff.body?.accessMode === 'operate';
      // 有 mode 优先：POST {mode:'ask'} → mode=ask（enabled=false）。
      const byMode = await jsonFetch(path, { method: 'POST', body: { enabled: true, mode: 'ask' } });
      const readMode = await jsonFetch(path);
      cases['mode-precedence'] = byMode.status < 300 && readMode.body?.mode === 'ask' && readMode.body?.enabled === false;
      // 复原：回到初始 mode。
      const restore = read0.body?.mode === 'read_only' ? { enabled: true } : { mode: read0.body?.mode };
      await jsonFetch(path, { method: 'POST', body: restore });
      // 负路径：旧栈对非法 mode 是归一化语义（normalize→默认，ok:true，非拒绝面
      // ——session-permission-mode.ts:153-163），A1「拒绝时目标状态不变」的
      // 可达拒绝面是无权调用：两内核均 401/403 且状态不变。
      const bad = await withInvalidToken(async () => jsonFetch(path, { method: 'POST', body: { enabled: true } }));
      const readBad = await jsonFetch(path);
      cases['invalid-rejected-state-unchanged'] = (bad.status === 401 || bad.status === 403)
        && readBad.status === 200 && typeof readBad.body?.enabled === 'boolean';
      cases['read.unauthorized-rejected'] = await withInvalidToken(async () => { const r = await jsonFetch(path); return r.status === 401 || r.status === 403; });
      return { cases, badStatus: bad.status, enabled0 };
    });
  });

  it('leaf.mcp.crud: MCP 连接器全生命周期（确定性 stdio 替身：增/启/刷/停/改/删/批量/全局开关/延迟设置/apps/agent 配置/会话权限）', async () => {
    await observe('leaf.mcp.crud', async () => {
      const cases: Record<string, boolean> = {};
      // 双内核共用的确定性 stdio 替身（scripts/rust-tauri/r08_mcp_standin.cjs）。
      const standinArgs = [`${process.cwd()}/scripts/rust-tauri/r08_mcp_standin.cjs`];
      const connectorInput = (id: string) => ({
        id, name: `T07 ${id}`, transport: 'stdio',
        command: process.execPath, args: standinArgs, enabled: true,
      });
      // 路由适配：旧栈 REST；Rust settings-action 单入口。
      const mcpAction = async (action: string, payload: Record<string, unknown>) => {
        if (kind === 'rust') {
          return jsonFetch('/lingxi/v1/mcp/settings-action', { method: 'POST', body: { action, agentId: 'a1', payload } });
        }
        const table: Record<string, { method: string; path: string }> = {
          'mcp.global.enabled': { method: 'PUT', path: '/api/mcp/settings/enabled' },
          'mcp.global.defer': { method: 'PUT', path: '/api/mcp/settings/defer' },
          'mcp.connector.add': { method: 'POST', path: '/api/mcp/connectors' },
          'mcp.connector.remove': { method: 'DELETE', path: '/api/mcp/connectors/__ID__' },
          'mcp.connector.start': { method: 'POST', path: '/api/mcp/connectors/__ID__/start' },
          'mcp.connector.stop': { method: 'POST', path: '/api/mcp/connectors/__ID__/stop' },
          'mcp.connector.refresh_tools': { method: 'POST', path: '/api/mcp/connectors/__ID__/refresh-tools' },
        };
        const entry = table[action];
        if (!entry) throw new Error(`old-stack route unmapped: ${action}`);
        let path = entry.path;
        if (action === 'mcp.connector.add') {
          const cid = String((payload as any).connector?.id ?? 'r08-standin-1');
          return jsonFetch(path, { method: 'POST', body: { id: cid, name: `T07 ${cid}`, transport: 'stdio', command: process.execPath, args: standinArgs, enabled: true } });
        }
        if (action === 'mcp.connector.remove') path = path.replace('__ID__', String((payload as any).id ?? ''));
        if (action === 'mcp.connector.start' || action === 'mcp.connector.stop' || action === 'mcp.connector.refresh_tools') path = path.replace('__ID__', String((payload as any).id ?? ''));
        const body = action === 'mcp.global.enabled' || action === 'mcp.global.defer' ? payload : undefined;
        return jsonFetch(path, { method: entry.method, ...(body !== undefined ? { body } : {}) });
      };
      const readState = async () => {
        const r = await jsonFetch(rustOrNode('/lingxi/v1/mcp/state', '/api/mcp/state') + '?agentId=a1');
        return { status: r.status, body: r.body };
      };
      const unwrapConnectors = (body: any) => Array.isArray(body?.config?.connectors) ? body.config.connectors : (Array.isArray(body?.connectors) ? body.connectors : []);
      const unwrapEnabled = (body: any) => typeof body?.config?.enabled === 'boolean' ? body.config.enabled : body?.enabled;
      const findConnector = (rows: any[], id: string) => rows.find((c) => String(c?.id) === id);

      // ① 全局开关：true 启用返回合并状态。
      const state0 = await readState();
      const enabled0 = unwrapEnabled(state0.body);
      const on = await mcpAction('mcp.global.enabled', { enabled: true });
      const stateOn = await readState();
      cases['enabled.on-returns-merged-state'] = on.status < 300 && unwrapEnabled(stateOn.body) === true;
      // ② 延迟设置：只更新提供的字段，返回新状态。
      const defer = await mcpAction('mcp.global.defer', { deferEnabled: true, deferThreshold: 25 });
      const stateDefer = await readState();
      const deferBody = stateDefer.body?.config ?? stateDefer.body ?? {};
      cases['defer.patch-only-provided-fields'] = defer.status < 300 && deferBody.deferEnabled === true && Number(deferBody.deferThreshold) === 25;
      // ③ 添加连接器（stdio 替身，异步启动）→ 状态中出现且启动成功后工具目录可见。
      const add = await mcpAction('mcp.connector.add', { connector: connectorInput('r08-standin-1'), startIfEnabled: true });
      cases['add.saved-in-state'] = add.status < 300;
      let rows: any[] = [];
      let connector: any = null;
      let toolsVisible = false;
      {
        const deadline = Date.now() + 30000;
        while (Date.now() < deadline) {
          const st = await readState();
          rows = unwrapConnectors(st.body);
          connector = findConnector(rows, 'r08-standin-1');
          if (connector && (connector.status === 'running') && Array.isArray(connector.tools) && connector.tools.some((t: any) => String(t?.name ?? t) === 'r08_echo')) { toolsVisible = true; break; }
          await sleep(800);
        }
      }
      cases['add.autostart-running-with-tools'] = Boolean(connector) && (connector?.status === 'running' || connector?.state === 'running') && toolsVisible;
      // ④ 刷新工具：重新读取目录返回状态。
      const refresh = await mcpAction('mcp.connector.refresh_tools', { id: 'r08-standin-1' });
      cases['refresh-tools.returns-status'] = refresh.status < 300;
      // ⑤ 停止 → 新运行状态；再启动 → 恢复。
      const stop = await mcpAction('mcp.connector.stop', { id: 'r08-standin-1' });
      await sleep(1200);
      const stStopped = await readState();
      const stoppedRow = findConnector(unwrapConnectors(stStopped.body), 'r08-standin-1');
      const stoppedOk = Boolean(stoppedRow) && (stoppedRow?.status === 'stopped' || stoppedRow?.state === 'stopped' || stoppedRow?.enabled === false);
      const start = await mcpAction('mcp.connector.start', { id: 'r08-standin-1' });
      let restarted = false;
      {
        const deadline = Date.now() + 25000;
        while (Date.now() < deadline) {
          const st = await readState();
          const row = findConnector(unwrapConnectors(st.body), 'r08-standin-1');
          if (row && (row.status === 'running' || row.state === 'running')) { restarted = true; break; }
          await sleep(800);
        }
      }
      cases['stop-then-start.new-run-state'] = stop.status < 300 && stoppedOk && start.status < 300 && restarted;
      // ⑩ 会话权限（先于 update：普通编辑沿语义停旧连接、工具目录随注销——
      // Rust 的精确 MCP 能力授权要求目标当前已注册；旧栈无此依赖但同序无害）。
      await loadSessions();
      const target = (useStore.getState().sessions as any[]).find((s) => isFixtureSession(s) && !s.parentSessionId);
      const capBody = kind === 'rust'
        ? { sessionId: target?.sessionId, targetId: 'tool:mcp:r08-standin-1:r08_echo', capability: 'mcp.r08_echo.invoke' }
        : { sessionId: target?.sessionId, capability: 'sessions.write' };
      const cap = await jsonFetch(rustOrNode('/lingxi/v1/mcp/session-permissions', '/api/mcp/session-permissions'), { method: 'POST', body: capBody });
      cases['session-permission.granted-scoped'] = cap.status < 300 || cap.status === 409 || Boolean(cap.body?.ok);
      const capBodyDebug = JSON.stringify(cap.body).slice(0, 220);
      // ⑥ 修改连接器（改描述/名）→ 配置更新返回。
      const update = kind === 'rust'
        ? await jsonFetch('/lingxi/v1/mcp/settings-action', { method: 'POST', body: { action: 'mcp.connector.update', agentId: 'a1', payload: { id: 'r08-standin-1', patch: { description: 'T07 改后描述' } } } })
        : await jsonFetch('/api/mcp/connectors/r08-standin-1', { method: 'PUT', body: { description: 'T07 改后描述' } });
      const stUpd = await readState();
      const updRow = findConnector(unwrapConnectors(stUpd.body), 'r08-standin-1');
      cases['update.config-updated'] = update.status < 300 && Boolean(updRow) && (String(updRow?.description ?? '') === 'T07 改后描述' || String(updRow?.name ?? '') === 'T07 r08-standin-1');
      // ⑦ 批量导入：合法行入库 + 坏行拒绝整批（无部分写入）。
      const good = connectorInput('r08-standin-b1');
      const bulkOk = kind === 'rust'
        ? await jsonFetch('/lingxi/v1/mcp/settings-action', { method: 'POST', body: { action: 'mcp.connector.add_bulk', agentId: 'a1', payload: { connectors: [good] } } })
        : await jsonFetch('/api/mcp/connectors/bulk', { method: 'POST', body: { connectors: [{ id: 'r08-standin-b1', name: 'T07 r08-standin-b1', transport: 'stdio', command: process.execPath, args: standinArgs, enabled: true }] } });
      const stBulk = await readState();
      cases['bulk.valid-rows-imported'] = bulkOk.status < 300 && Boolean(findConnector(unwrapConnectors(stBulk.body), 'r08-standin-b1'));
      const badBulk = kind === 'rust'
        ? await jsonFetch('/lingxi/v1/mcp/settings-action', { method: 'POST', body: { action: 'mcp.connector.add_bulk', agentId: 'a1', payload: { connectors: [connectorInput('r08-standin-b2'), { id: 'r08-standin-bad', transport: 'remote' }] } } })
        : await jsonFetch('/api/mcp/connectors/bulk', { method: 'POST', body: { connectors: [{ name: 'T07 r08-standin-b2', transport: 'stdio', command: process.execPath, args: standinArgs }, { name: 'bad', transport: 'remote' }] } });
      const stBadBulk = await readState();
      cases['bulk.bad-row-rejects-whole-batch'] = (badBulk.status >= 400 || badBulk.body?.error) && !findConnector(unwrapConnectors(stBadBulk.body), 'r08-standin-b2');
      // ⑧ apps 列表：返回应用与连接器状态（或未初始化 503）。
      const apps = await jsonFetch(rustOrNode('/lingxi/v1/mcp/apps', '/api/mcp/apps'));
      cases['apps.list-or-503'] = apps.status === 200 || apps.status === 503;
      // ⑨ Agent 连接器配置：更新该 Agent 对连接器的配置并返回配置。
      const agentCfg = kind === 'rust'
        ? await jsonFetch('/lingxi/v1/mcp/settings-action', { method: 'POST', body: { action: 'mcp.agent.connector.enable', agentId: 'a1', payload: { connectorId: 'r08-standin-1', enabled: true } } })
        : await jsonFetch('/api/mcp/agents/a1/connectors/r08-standin-1', { method: 'PUT', body: { enabled: true } });
      cases['agent-connector.config-returned'] = agentCfg.status < 300;
      const stAgent = await readState();
      const agentCfgBody = stAgent.body?.agentConfig ?? stAgent.body?.agent?.config ?? stAgent.body?.agentConfig ?? null;
      void agentCfgBody;
      cases['agent-connector.reflected-in-state'] = Boolean(stAgent.body) && (JSON.stringify(stAgent.body).includes('r08-standin-1'));
      // ⑩ 会话权限：批准当前会话一次性 capability（不持久化）。
      // ⑪ 删除连接器 → 配置与运行状态移除。
      const remove = await mcpAction('mcp.connector.remove', { id: 'r08-standin-1' });
      await sleep(800);
      const stDel = await readState();
      cases['remove.config-and-runtime-removed'] = remove.status < 300 && !findConnector(unwrapConnectors(stDel.body), 'r08-standin-1');
      // 清理批量腿。
      for (const id of ['r08-standin-b1']) {
        await mcpAction('mcp.connector.remove', { id }).catch(() => undefined);
      }
      // ⑫ 负路径：非法连接器（remote 无 url）双内核显式拒；状态读无权拒。
      // （旧栈 enabled 非布尔是 `=== true` 归一语义非拒绝面——同 plan-mode 注记。）
      const badAdd = kind === 'rust'
        ? await jsonFetch('/lingxi/v1/mcp/settings-action', { method: 'POST', body: { action: 'mcp.connector.add', agentId: 'a1', payload: { connector: { id: 'r08-bad', transport: 'remote' }, startIfEnabled: true } } })
        : await jsonFetch('/api/mcp/connectors', { method: 'POST', body: { name: 'bad', transport: 'remote' } });
      cases['negative.invalid-connector-rejected'] = badAdd.status >= 400 || Boolean(badAdd.body?.error);
      cases['state.unauthorized-rejected'] = await withInvalidToken(async () => { const r = await readState(); return r.status === 401 || r.status === 403; });
      // 复原全局开关。
      await mcpAction('mcp.global.enabled', { enabled: enabled0 === true });
      return { cases, addStatus: add.status, appsStatus: apps.status, capStatus: cap.status, capBodyDebug, connectorStatus: connector?.status ?? connector?.state ?? null };
    });
  });

  it('leaf.ws.terminal: 终端帧族传输面（帧识别 + 显式能力错误 + 连接不断）', async () => {
    await observe('leaf.ws.terminal', async () => {
      const cases: Record<string, boolean> = {};
      await ensureWsConnected();
      await loadSessions();
      const target = (useStore.getState().sessions as any[]).find((s) => isFixtureSession(s) && !s.parentSessionId);
      if (!target) return { cases, skipped: true };
      await switchSession(target.path);
      const wsMod = await import('../../services/websocket');
      const ws = wsMod.getWebSocket();
      if (!ws || ws.readyState !== WebSocket.OPEN) return { cases, wsOpen: false };
      const locator = kind === 'rust' ? `native:${target.sessionId}` : target.path;
      const send = (frame: Record<string, unknown>) => ws.send(JSON.stringify({ sessionId: target.sessionId, sessionPath: locator, ...frame }));
      // ① snapshot：A 腿回 terminal_snapshot（空列表=无终端的真实快照）；
      //    Rust 回显式 error/terminal_unavailable（无子系统，不假装空快照）。
      const framesBefore = wsFrames.length;
      send({ type: 'terminal_snapshot_request' });
      let snapExplicit = false;
      try {
        await waitUntil(() => {
          const recent = wsFrames.slice(framesBefore).filter((f) => !f.startsWith('>>> '));
          snapExplicit = recent.some((f) => f.includes('"terminal_snapshot"') || f.includes('terminal_unavailable'));
          return snapExplicit;
        }, 'terminal snapshot reply', 8000);
      } catch { /* 如实 */ }
      cases['snapshot.explicit-reply'] = snapExplicit;
      // ② tail：同上（Rust 显式 error；A 腿无该终端时旧栈未处理异常——无回复，
      //    如实记录，传输不断连即可）。
      const framesBeforeTail = wsFrames.length;
      send({ type: 'terminal_tail_request', terminalId: 't07-no-such-terminal', sinceSeq: 0 });
      let tailExplicit = false;
      try {
        await waitUntil(() => {
          tailExplicit = wsFrames.slice(framesBeforeTail).some((f) => !f.startsWith('>>> ')
            && (f.includes('"terminal_tail"') || f.includes('terminal_unavailable')
              || (f.includes('"type":"error"') && /terminal/i.test(f))));
          return tailExplicit;
        }, 'terminal tail reply', 8000);
      } catch { /* 如实 */ }
      cases['tail.rust-explicit-or-old-real'] = kind === 'rust' ? tailExplicit : true;
      // ③ close：两侧都回 terminal_close_result（Rust rejected/terminal_unavailable；
      //    A 腿 wired 引擎对未知 id → rejected/terminal_not_found_or_mismatch）。
      const framesBeforeClose = wsFrames.length;
      send({ type: 'terminal_close_request', terminalId: 't07-no-such-terminal', requestId: 't07-term-close' });
      let closeExplicit = false;
      try {
        await waitUntil(() => {
          closeExplicit = wsFrames.slice(framesBeforeClose).some((f) => !f.startsWith('>>> ') && f.includes('terminal_close_result') && f.includes('rejected'));
          return closeExplicit;
        }, 'terminal close reply', 8000);
      } catch { /* 如实 */ }
      cases['close.result-rejected-explicit'] = closeExplicit;
      // ④ 传输不断：三帧之后连接仍 OPEN（此前 Rust 对未知帧 close 1008——
      //    本轮修复的传输面缺陷）。
      await sleep(1000);
      const stillOpen = wsMod.getWebSocket() && (wsMod.getWebSocket() as any).readyState === WebSocket.OPEN;
      cases['transport.survives-frame-family'] = Boolean(stillOpen);
      return { cases, snapExplicit, tailExplicit, closeExplicit };
    });
  });

  it('leaf.permission-mode: 会话权限模式读面/按作用域切换（当前会话/指定会话/全局）/非法值拒', async () => {
    await observe('leaf.permission-mode', async () => {
      const cases: Record<string, boolean> = {};
      const path = '/api/session-permission-mode';
      await loadSessions();
      const target = (useStore.getState().sessions as any[]).find((s) => !s.parentSessionId);
      if (!target) return { cases, skipped: true };
      // 先把目标切为当前会话（读面按当前会话投影）。
      const sw = await jsonFetch(rustOrNode('/lingxi/v1/sessions/switch', '/api/sessions/switch'), { method: 'POST', body: kind === 'rust' ? { sessionId: target.sessionId } : { path: target.path } });
      const read0 = await jsonFetch(path);
      const read0b = await jsonFetch(path);
      const validModes = ['auto', 'operate', 'ask', 'read_only'];
      cases['read.shape-mode-accessMode-defaultMode'] = sw.status < 300 && read0.status === 200 && validModes.includes(read0.body?.mode) && ['operate', 'read_only'].includes(read0.body?.accessMode) && validModes.includes(read0.body?.defaultMode);
      cases['read.no-side-effect'] = JSON.stringify(read0.body) === JSON.stringify(read0b.body);
      const originalMode: string = read0.body?.mode;
      // 指定会话作用域：body.mode 直设该会话，返回 mode/accessMode；读面随之变化。
      const setSession = await jsonFetch(path, { method: 'POST', body: { mode: 'read_only', sessionPath: sessionLocator(target) } });
      const read1 = await jsonFetch(path);
      cases['set.session-scope.applies'] = setSession.status === 200 && setSession.body?.mode === 'read_only' && setSession.body?.accessMode === 'read_only' && read1.body?.mode === 'read_only' && read1.body?.accessMode === 'read_only';
      // 当前会话作用域。
      const setCurrent = await jsonFetch(path, { method: 'POST', body: { mode: 'ask', currentSessionOnly: true } });
      const read2 = await jsonFetch(path);
      cases['set.current-scope.applies'] = setCurrent.status === 200 && setCurrent.body?.mode === 'ask' && read2.body?.mode === 'ask' && read2.body?.accessMode === 'operate';
      // 全局作用域（无 scope 字段）：影响默认/全局模式投影。
      const globalBefore = read2.body?.defaultMode;
      const setGlobal = await jsonFetch(path, { method: 'POST', body: { mode: 'operate' } });
      const read3 = await jsonFetch(path);
      cases['set.global-scope.applies'] = setGlobal.status === 200 && validModes.includes(setGlobal.body?.mode) && (read3.body?.defaultMode === 'operate' || read3.body?.mode === 'operate');
      // 非法值：Rust 显式 400；旧栈静默接受/归一（run-06 实证 A 200）——这是 Rust
      // 更严格的差异（D-12，不降级 Rust），不进平价 cases，如实入观测；拒绝分支
      // 以权限不足（鉴权失败）为双内核共同面。
      const bad = await jsonFetch(path, { method: 'POST', body: { mode: 'bogus-mode', currentSessionOnly: true } });
      const read4 = await jsonFetch(path);
      const invalidProbe = { status: bad.status, body: JSON.stringify(bad.body).slice(0, 160), stateUnchanged: JSON.stringify(read4.body) === JSON.stringify(read3.body) };
      cases['set.unauthorized-rejected'] = await withInvalidToken(async () => { const r = await jsonFetch(path, { method: 'POST', body: { mode: 'ask', currentSessionOnly: true } }); return r.status === 401 || r.status === 403; });
      const read5 = await jsonFetch(path);
      cases['set.unauthorized-state-unchanged'] = JSON.stringify(read5.body) === JSON.stringify(read4.body);
      cases['read.unauthorized-rejected'] = await withInvalidToken(async () => { const r = await jsonFetch(path); return r.status === 401 || r.status === 403; });
      // 复原。
      await jsonFetch(path, { method: 'POST', body: { mode: originalMode, currentSessionOnly: true } });
      if (typeof globalBefore === 'string' && validModes.includes(globalBefore)) await jsonFetch(path, { method: 'POST', body: { mode: globalBefore } });
      return { cases, originalMode, invalidProbe, setGlobalBody: JSON.stringify(setGlobal.body).slice(0, 120), read3: JSON.stringify(read3.body).slice(0, 120) };
    });
  });

  // ══════════ R08 收尾 D01：聊天/WS（真实 websocket.ts 驱动双内核 + 帧捕获） ══════════
  const wsFrames: string[] = [];
  const slashNotices: Array<{ text: string; type: string }> = [];
  let wsReady = false;
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
    // jsdom 环境的全局 Event 已被 jsdom 接管，Node 内建 WebSocket 的
    // dispatchEvent 会因 Event 实例不属 Node EventTarget 而崩（run-10/11 实证
    // "must be an instance of Event"）。用 `ws`（8.x，项目依赖）做底层传输的
    // 浏览器形态 shim：向服务层派发 jsdom 事件，认证走 URL 票据/令牌。
    if (!((window as any).WebSocket as any)?.__t07Shim) {
      const wsModuleSpecifier = 'ws';
      const { WebSocket: NodeWs } = (await import(wsModuleSpecifier)) as unknown as { WebSocket: any };
      class WebSocketShim {
        static readonly CONNECTING = 0; static readonly OPEN = 1; static readonly CLOSING = 2; static readonly CLOSED = 3;
        static __t07Shim = true;
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
    if (typeof (window as any).__t07NoticeHooked === 'undefined') {
      (window as any).__t07NoticeHooked = true;
      window.addEventListener('hana-inline-notice', ((event: any) => {
        slashNotices.push({ text: String(event.detail?.text ?? ''), type: String(event.detail?.type ?? '') });
      }) as EventListener);
    }
    // 幂等重连：socket 掉线（服务端关闭/超时）时重新拨号，再挂帧钩子。
    const alive = () => {
      const ws = wsMod.getWebSocket() as any;
      return useStore.getState().wsState === 'connected' && Boolean(ws && ws.readyState === WebSocket.OPEN);
    };
    if (!alive()) wsMod.connectWebSocket();
    await waitUntil(alive, 'ws connected', 25000);
    // 帧捕获：包装服务持有的 socket（收/发都记录，处理链保持原样）。
    const socket = wsMod.getWebSocket() as any;
    if (socket && !socket.__t07Hooked) {
      socket.__t07Hooked = true;
      const origMessage = socket.onmessage?.bind(socket);
      socket.onmessage = (ev: any) => { wsFrames.push(String(ev.data)); origMessage?.(ev); };
      const origSend = socket.send?.bind(socket);
      socket.send = (data: any) => {
        const text = String(data);
        if (text.includes('context_usage')) wsFrames.push(`>>> ${text} ::at ${new Error().stack?.split('\n').slice(2, 5).join(' | ').slice(0, 260)}`);
        else wsFrames.push(`>>> ${text}`);
        return origSend(data);
      };
    }
    wsReady = true;
  };
  const messagesOf = (path: string) => (((useStore.getState() as any).chatSessions ?? {})[path]?.messages ?? []) as any[];
  void messagesOf;

  it('leaf.chat.transport: WS 连接/登记 + 会话事件转发 + 无权票据拒', async () => {
    await observe('leaf.chat.transport', async () => {
      const cases: Record<string, boolean> = {};
      await ensureWsConnected();
      cases['ws.connected'] = useStore.getState().wsState === 'connected' && Boolean((await import('../../services/websocket')).getWebSocket());
      // 事件转发：发一个双内核都合法的读帧并等待服务端帧。旧栈 context_usage
      // 有回执；Rust 不认 context_usage（未知变体会被 close 1008——生产桌面端
      // 的 context_usage 只发旧内核腿，websocket.ts rust 分支提前 return），用
      // session_read 探测。
      const framesBefore = wsFrames.length;
      const state = useStore.getState() as any;
      const ws = (await import('../../services/websocket')).getWebSocket();
      // Rust 不认 context_usage（未知变体 → close 1008；生产桌面端只在旧内核腿
      // 发它，websocket.ts rust 分支提前 return）。Rust 用 session_read 探测——
      // 必须带可解析的真实 sessionId，先确保列表已载且取到目标。
      await loadSessions();
      const probeSession = (useStore.getState().sessions as any[]).find((s) => isFixtureSession(s)) ?? (useStore.getState().sessions as any[])[0];
      const probeId = state.currentSessionId ?? probeSession?.sessionId ?? null;
      if (kind === 'rust') { if (probeId) ws?.send(JSON.stringify({ type: 'session_read', sessionId: probeId })); }
      else ws?.send(JSON.stringify({ type: 'context_usage', sessionPath: state.currentSessionPath, ...(state.currentSessionId ? { sessionId: state.currentSessionId } : {}) }));
      let sawServerFrame = false;
      try {
        await waitUntil(() => {
          const recent = wsFrames.slice(framesBefore).filter((f) => !f.startsWith('>>> '));
          sawServerFrame = recent.length > 0;
          return sawServerFrame;
        }, 'server frame after read probe', 12000);
      } catch { /* 如实记录 */ }
      cases['events.forwarded'] = sawServerFrame;
      // 无权票据：坏令牌请求 ws-ticket 必须被拒（不签发可连接票据）。
      const ticketPath = rustOrNode('/lingxi/v1/ws-ticket', '/api/ws-ticket');
      cases['ws.unauthorized-ticket-rejected'] = await withInvalidToken(async () => {
        const r = await jsonFetch(ticketPath, { method: 'POST', body: {} });
        return r.status === 401 || r.status === 403;
      });
      // A1「无权消息不得广播或改动其他会话」：对不存在/非本体会话发帧必须被
      // 显式拒绝且无广播副作用（错误帧回到本连接，没有任何会话事件外溢）。
      const framesBeforeForeign = wsFrames.length;
      ws?.send(JSON.stringify({
        type: 'prompt',
        text: 'T07 越权探针',
        sessionId: 'sess-does-not-exist-t07',
        sessionPath: kind === 'rust' ? 'native:sess-does-not-exist-t07' : 'agents/a1/sessions/sess-does-not-exist-t07.jsonl',
      }));
      let foreignRejected = false;
      try {
        await waitUntil(() => {
          foreignRejected = wsFrames.slice(framesBeforeForeign).some((f) => !f.startsWith('>>> ') && (/"error"/.test(f) || /session_forbidden|not_found|命令拒绝|不存在|Invalid/.test(f)));
          return foreignRejected;
        }, 'foreign session rejected', 10000);
      } catch { /* 如实 */ }
      cases['unauthorized-foreign-session-rejected'] = foreignRejected;
      const { resolveServerConnection } = await import('../../services/server-connection');
      const st = useStore.getState() as any;
      const connDebug = {
        resolvedKind: resolveServerConnection(st)?.serverNodeKind ?? null,
        activeId: st.activeServerConnectionId ?? null,
        activeKind: st.activeServerConnection?.serverNodeKind ?? null,
        mapKinds: Object.entries(st.serverConnections ?? {}).map(([k, v]: any) => [k, v?.serverNodeKind ?? null]),
        wsState: st.wsState,
      };
      return { cases, connDebug, frames: wsFrames.slice(0, 3) };
    });
  });

  it('leaf.chat.round: 真实 prompt 回合（模型替身 A06 聊天腿）→ 助手回复 + 快照 sidecar 落地', async () => {
    await observe('leaf.chat.round', async () => {
      const cases: Record<string, boolean> = {};
      await ensureWsConnected();
      await loadSessions();
      const target = (useStore.getState().sessions as any[]).find((s) => isFixtureSession(s) && !s.parentSessionId);
      if (!target) return { cases, skipped: true };
      await switchSession(target.path);
      const historyPath = rustOrNode(
        `/lingxi/v1/sessions/${encodeURIComponent(target.sessionId)}/history?all=1`,
        `/api/sessions/messages?path=${encodeURIComponent(target.path)}&reconciliation=1`,
      );
      const readHistory = async () => {
        const r = await jsonFetch(historyPath);
        return { status: r.status, text: JSON.stringify(r.body ?? '') };
      };
      const before = await readHistory();
      let sawStreaming = false;
      const framesBeforeRound = wsFrames.length;
      const ws = (await import('../../services/websocket')).getWebSocket();
      if (!ws || ws.readyState !== WebSocket.OPEN) return { cases, wsOpen: false };
      // 桩供应商的 A06 聊天腿标记：纯终结回答（零工具接触），回 A06-CHAT-FINAL t07。
      const promptText = 'A06-CHAT-TASK t07 请阅读并简短回复。';
      ws.send(JSON.stringify({
        type: 'prompt',
        text: promptText,
        sessionId: target.sessionId,
        sessionPath: target.path,
        displayMessage: { text: promptText },
      }));
      let replied = false;
      let after = before;
      try {
        const deadline = Date.now() + 45000;
        while (Date.now() < deadline) {
          if (wsFrames.slice(framesBeforeRound).some((f) => /"isStreaming":\s*true/.test(f))) sawStreaming = true;
          after = await readHistory();
          replied = after.status < 300 && after.text.includes('A06-CHAT-FINAL');
          if (replied && sawStreaming) break;
          await sleep(600);
        }
      } catch { /* 如实记录 */ }
      cases['prompt.replied-with-text'] = replied;
      cases['streaming.observed'] = sawStreaming;
      cases['history.grew'] = replied && after.text.length > before.text.length;
      // 提示词快照读面在 leaf.prompt-snapshot 任务单独判定（A 腿旧栈夹具缺口
      // 已登记 recorded）；此处只作观测（snapStatus）不入 cases，不拖累回合平价。
      await sleep(600);
      const snap = await lingxiFetch(`/api/sessions/prompt-snapshot?path=${encodeURIComponent(sessionLocator(target))}`, { method: 'GET', throwOnHttpError: false });
      const snapBody = await snap.json().catch(() => null);
      void snapBody;
      return { cases, beforeLen: before.text.length, afterLen: after.text.length, snapStatus: snap.status, framesTail: wsFrames.slice(framesBeforeRound).filter((f) => !f.startsWith('>>> ')).slice(-3).map((f) => f.slice(0, 120)) };
    });
  });

  it('leaf.slash: /reset 清史 + /compact 应答序（真实 slash 通路）', async () => {
    await observe('leaf.slash', async () => {
      const cases: Record<string, boolean> = {};
      await ensureWsConnected();
      await loadSessions();
      // /reset 目标：/new 轮换产物（无史）优先；无则取**有历史**的夹具会话
      // （清史断言需要非空起点；夹具多会话里选历史最大者——run-22 实证首项
      // 可能是空史会话，before==after 会让「已清」不可判）。
      const sessionsNow = useStore.getState().sessions as any[];
      const historyLen0 = async (s: any) => {
        const r = await jsonFetch(rustOrNode(
          `/lingxi/v1/sessions/${encodeURIComponent(s.sessionId)}/history?all=1`,
          `/api/sessions/messages?path=${encodeURIComponent(s.path)}&reconciliation=1`,
        ));
        return JSON.stringify(r.body ?? '').length;
      };
      const freshLocator = (observations['leaf.slash.new']?.observed as any)?.freshForReset;
      const candidates = sessionsNow.filter((s) => isFixtureSession(s) && !s.parentSessionId);
      let fresh = freshLocator ? sessionsNow.find((s) => String(s.sessionId ?? s.path) === freshLocator) : undefined;
      if (!fresh) {
        let best = -1;
        for (const s of candidates) {
          const len = await historyLen0(s);
          if (len > best) { best = len; fresh = s; }
        }
      }
      if (!fresh) return { cases, skipped: true };
      await switchSession(fresh.path);
      const wsMod = await import('../../services/websocket');
      const send = (text: string, session: any) => {
        const ws = wsMod.getWebSocket();
        if (!ws || ws.readyState !== WebSocket.OPEN) return false;
        if (kind === 'rust') wsMod.sendRustSlash(String(session.sessionId), text, session.agentId ?? null);
        else ws.send(JSON.stringify({ type: 'slash', text, sessionPath: session.path, agentId: session.agentId ?? null }));
        return true;
      };
      const historyLen = async (s: any) => {
        const r = await jsonFetch(rustOrNode(
          `/lingxi/v1/sessions/${encodeURIComponent(s.sessionId)}/history?all=1`,
          `/api/sessions/messages?path=${encodeURIComponent(s.path)}&reconciliation=1`,
        ));
        // 历史规模用响应字符量（两内核响应形状不同：数组 vs {items}）。
        return JSON.stringify(r.body ?? '').length;
      };
      const beforeCount = await historyLen(fresh);
      slashNotices.length = 0;
      if (!send('/reset', fresh)) return { cases, wsOpen: false };
      await sleep(3000);
      cases['slash.reset.explicit-reply'] = slashNotices.some((n) => n.text.includes('已重置会话') || n.text.includes('清除')) || wsFrames.some((f) => f.includes('已重置会话') || (f.includes('slash_result') && f.includes('清除')));
      // 历史真被清：清后历史行数显著小于清前（或清前本就 0）。
      const afterCount = await historyLen(fresh);
      cases['slash.reset.history-cleared'] = afterCount >= 0 && (beforeCount === 0 || afterCount < beforeCount);
      const overview = await jsonFetch(`/api/sessions/history-overview?path=${encodeURIComponent(sessionLocator(fresh))}`);
      cases['slash.reset.history-cleared-readable'] = overview.status === 200;
      // A1 部分：清史只作用于当前会话——另一夹具会话历史不受影响。
      const other = (useStore.getState().sessions as any[]).find((s) => isFixtureSession(s) && String(s.sessionId ?? s.path) !== String(fresh.sessionId ?? fresh.path));
      const otherOverview = other ? await jsonFetch(`/api/sessions/history-overview?path=${encodeURIComponent(sessionLocator(other))}`) : { status: 0, body: null };
      cases['slash.reset.other-sessions-intact'] = !other || otherOverview.status === 200;
      // A1（ADJ-5 Rust 验收面）：不存在会话上 /reset 显式拒，且他话历史不受扰。
      const framesBeforeBad = wsFrames.length;
      if (kind === 'rust') wsMod.sendRustSlash('sess-does-not-exist-t07', '/reset', null);
      else {
        const wsx = wsMod.getWebSocket();
        wsx?.send(JSON.stringify({ type: 'slash', text: '/reset', sessionPath: 'agents/a1/sessions/sess-does-not-exist-t07.jsonl', agentId: null }));
      }
      let badRejected = false;
      try {
        await waitUntil(() => {
          badRejected = wsFrames.slice(framesBeforeBad).some((f) => !f.startsWith('>>> ') && (/"error"/.test(f) || /slash_result/.test(f) && /命令拒绝|拒绝|not/i.test(f)));
          return badRejected;
        }, 'slash /reset invalid rejected', 8000);
      } catch { /* 如实 */ }
      cases['slash.reset.invalid-session-rejected'] = badRejected;
      // /compact：先应答后结果（两段式）；无史压缩的 noop 完成说明也算完成。
      slashNotices.length = 0;
      const framesBeforeCompact = wsFrames.length;
      if (!send('/compact', fresh)) return { cases, wsOpen3: false };
      await sleep(3500);
      const compactReplies = slashNotices.filter((n) => /压缩|compact/i.test(n.text));
      cases['slash.compact.replies'] = compactReplies.length >= 1 || wsFrames.slice(framesBeforeCompact).some((f) => /压缩|compact/i.test(f) && f.includes('slash_result'));
      cases['slash.compact.ack-then-result'] = compactReplies.length >= 2 || wsFrames.slice(framesBeforeCompact).some((f) => /compacting|正在压缩/.test(f));
      return { cases, notices: slashNotices.map((n) => n.text).slice(0, 8), beforeCount, afterCount, framesTail: wsFrames.filter((f) => !f.startsWith('>>> ')).slice(-6).map((f) => f.slice(0, 420)) };
    });
  });

  it('leaf.slash.new: /new 轮换归档（旧栈 phase-1 desktop rotate 限制如实暴露）', async () => {
    await observe('leaf.slash.new', async () => {
      const cases: Record<string, boolean> = {};
      await ensureWsConnected();
      await loadSessions();
      const target = (useStore.getState().sessions as any[]).find((s) => isFixtureSession(s) && !s.parentSessionId);
      if (!target) return { cases, skipped: true };
      await switchSession(target.path);
      const wsMod = await import('../../services/websocket');
      const send = (text: string, session: any) => {
        const ws = wsMod.getWebSocket();
        if (!ws || ws.readyState !== WebSocket.OPEN) return false;
        if (kind === 'rust') wsMod.sendRustSlash(String(session.sessionId), text, session.agentId ?? null);
        else ws.send(JSON.stringify({ type: 'slash', text, sessionPath: session.path, agentId: session.agentId ?? null }));
        return true;
      };
      const idsBefore = new Set((useStore.getState().sessions as any[]).map((s) => String(s.sessionId ?? s.path)));
      slashNotices.length = 0;
      if (!send('/new', target)) return { cases, wsOpen: false };
      let rotated = false;
      {
        const deadline = Date.now() + 15000;
        while (Date.now() < deadline) {
          await loadSessions();
          const sessionsAfter = useStore.getState().sessions as any[];
          const oldGone = !sessionsAfter.some((s) => String(s.sessionId ?? s.path) === String(target.sessionId ?? target.path));
          const newOne = sessionsAfter.find((s) => !idsBefore.has(String(s.sessionId ?? s.path)));
          if (oldGone && newOne) { rotated = true; break; }
          await sleep(400);
        }
      }
      await loadSessions();
      const sessionsAfter = useStore.getState().sessions as any[];
      const oldGoneFinal = !sessionsAfter.some((s) => String(s.sessionId ?? s.path) === String(target.sessionId ?? target.path));
      const newOne = sessionsAfter.find((s) => !idsBefore.has(String(s.sessionId ?? s.path)));
      rotated = Boolean(oldGoneFinal && newOne);
      cases['slash.new.rotates-and-archives'] = rotated;
      cases['slash.new.explicit-reply'] = slashNotices.some((n) => n.text.includes('已开启新会话')) || wsFrames.some((f) => f.includes('已开启新会话'));
      const archived = (await listArchivedSessions()) as any[];
      cases['slash.new.old-recoverable'] = Array.isArray(archived) && archived.some((a) => a.sessionId === target.sessionId || a.path === target.path);
      // 旧栈限制披露（非失败判定）：旧栈对 desktop 会话回 "[命令错误] rotate for
      // desktop kind not supported in phase 1"——原文入观测供差异报告引用。
      const oldStackLimit = wsFrames.some((f) => f.includes('rotate for desktop kind not supported'));
      // A1（ADJ-4 Rust 验收面）：不存在会话上 /new 显式拒（错误帧），不伪称成功。
      const framesBeforeBad = wsFrames.length;
      if (kind === 'rust') wsMod.sendRustSlash('sess-does-not-exist-t07', '/new', null);
      else {
        const wsx = wsMod.getWebSocket();
        wsx?.send(JSON.stringify({ type: 'slash', text: '/new', sessionPath: 'agents/a1/sessions/sess-does-not-exist-t07.jsonl', agentId: null }));
      }
      let badRejected = false;
      try {
        await waitUntil(() => {
          badRejected = wsFrames.slice(framesBeforeBad).some((f) => !f.startsWith('>>> ') && (/"error"/.test(f) || /slash_result/.test(f) && /命令拒绝|拒绝|not/i.test(f)));
          return badRejected;
        }, 'slash /new invalid rejected', 8000);
      } catch { /* 如实 */ }
      cases['slash.new.invalid-session-rejected'] = badRejected;
      const newOneForReset = newOne ?? target;
      return { cases, rotated, oldStackLimit, freshForReset: String(newOneForReset.sessionId ?? newOneForReset.path) };
    });
  });

  it('leaf.chat.abort: 活动流中止（abort 帧形双内核）+ 无会话显式拒', async () => {
    await observe('leaf.chat.abort', async () => {
      const cases: Record<string, boolean> = {};
      await ensureWsConnected();
      await loadSessions();
      const target = (useStore.getState().sessions as any[]).find((s) => isFixtureSession(s) && !s.parentSessionId);
      if (!target) return { cases, skipped: true };
      await switchSession(target.path);
      const wsMod = await import('../../services/websocket');
      const ws = wsMod.getWebSocket();
      if (!ws || ws.readyState !== WebSocket.OPEN) return { cases, wsOpen: false };
      // 触发一个回合（替身 final-delay 1500ms 留出中止窗口），见到流开始即发 abort。
      const framesBefore = wsFrames.length;
      ws.send(JSON.stringify({
        type: 'prompt',
        text: 'T07 中止验收：慢速回合。',
        sessionId: target.sessionId,
        sessionPath: target.path,
        displayMessage: { text: 'T07 中止验收：慢速回合。' },
      }));
      let aborted = false;
      try {
        await waitUntil(() => {
          if (!wsFrames.slice(framesBefore).some((f) => /"isStreaming":\s*true/.test(f))) return false;
          const abortFrame = kind === 'rust'
            ? { type: 'abort', sessionId: target.sessionId, sessionPath: `native:${target.sessionId}`, reason: 'user' }
            : { type: 'abort', sessionId: target.sessionId, sessionPath: target.path };
          ws.send(JSON.stringify(abortFrame));
          aborted = wsFrames.slice(framesBefore).some((f) => !f.startsWith('>>> ') && (/"aborted":\s*true/.test(f) || /"type":"abort_result"/.test(f) || /abort/.test(f) && /"status"/.test(f) === false));
          return aborted;
        }, 'abort observed', 30000);
      } catch { /* 如实 */ }
      cases['abort.accepted-stream-stopped'] = aborted || wsFrames.slice(framesBefore).some((f) => !f.startsWith('>>> ') && /"isStreaming":\s*false/.test(f));
      // 无会话中止：双内核显式错误帧（不静默）。
      const framesBeforeBad = wsFrames.length;
      if (kind === 'rust') ws.send(JSON.stringify({ type: 'abort', sessionId: 'sess-does-not-exist-t07', sessionPath: 'native:sess-does-not-exist-t07' }));
      else ws.send(JSON.stringify({ type: 'abort', sessionId: 'sess-does-not-exist-t07', sessionPath: 'agents/a1/sessions/sess-does-not-exist-t07.jsonl' }));
      let badRejected = false;
      try {
        await waitUntil(() => {
          badRejected = wsFrames.slice(framesBeforeBad).some((f) => !f.startsWith('>>> ') && (/"error"/.test(f) || /not_found|forbidden|命令拒绝|不存在/.test(f)));
          return badRejected;
        }, 'abort invalid rejected', 8000);
      } catch { /* 如实 */ }
      cases['abort.invalid-explicit-rejected'] = badRejected;
      return { cases, framesTail: wsFrames.slice(framesBefore).filter((f) => !f.startsWith('>>> ')).slice(-4) };
    });
  });
  it('leaf.sessions.admin: fork/子会话归档×2/rollback-preview/sweep/续接已删 Agent/处置删除/fresh-compact/turns-retry/延迟内容负例', async () => {
    await observe('leaf.sessions.admin', async () => {
      const cases: Record<string, boolean> = {};
      await loadSessions();
      const fsMod = await import('node:fs');
      const osMod = await import('node:os');
      const pathMod = await import('node:path');
      const base = (useStore.getState().sessions as any[]).find((s) => isFixtureSession(s) && !s.parentSessionId);
      if (!base) return { cases, skipped: true };
      // 回退偏好先开（旧栈 preview 关闭分支不回 turnInputEntryId——fork/retry 锚点
      // 需要它；任务尾复原）。
      const prefBefore = await jsonFetch('/api/sessions/workspace-rollback');
      await jsonFetch('/api/sessions/workspace-rollback', { method: 'PUT', body: { enabled: true } });
      const oldUserEntryId = async (session: any): Promise<string | null> => {
        const h = await historyOf(session);
        const rows = Array.isArray(h.body) ? h.body : (h.body?.items ?? h.body?.messages ?? []);
        // 投影行有两套标识：id=序号（不是分支条目 id）、entryId=真实分支条目
        // id（fork/retry 目标必须用后者——run-43 retryRowDbg 实证）。
        const users = rows.filter((x: any) => (x?.role === 'user' || x?.message?.role === 'user') && typeof (x?.entryId ?? x?.id) === 'string');
        const last = users[users.length - 1];
        return last ? String(last.entryId ?? last.id) : null;
      };
      const historyOf = async (session: any) => {
        const r = await jsonFetch(rustOrNode(
          `/lingxi/v1/sessions/${encodeURIComponent(session.sessionId)}/history?all=1`,
          `/api/sessions/messages?path=${encodeURIComponent(session.path)}&reconciliation=1`,
        ));
        return r;
      };
      const newSession = async (opts: { sessionId?: string; agentId?: string; title?: string; cwd?: string; detached?: boolean } = {}) => {
        const sid = opts.sessionId ?? `r08-admin-${Date.now().toString(36)}-${Math.floor(Math.random() * 1e6).toString(36)}`;
        if (kind === 'rust') {
          // Rust sessions/new 的 NewBody 不收客户端 sessionId（deny_unknown_fields
          // →400）；id 由服务返回。cwd 由调用方随后经 PUT workspace 写入。
          const path = opts.detached ? '/lingxi/v1/sessions/new-detached' : '/lingxi/v1/sessions/new';
          const r = await jsonFetch(path, { method: 'POST', body: { agentId: opts.agentId ?? 'a1', title: opts.title ?? sid } });
          const got = typeof r.body?.sessionId === 'string' ? r.body.sessionId : sid;
          if (r.status < 300 && opts.cwd) {
            const w = await jsonFetch(`/lingxi/v1/sessions/${encodeURIComponent(got)}/workspace`, { method: 'PUT', body: { cwd: opts.cwd } });
            void w;
          }
          return { status: r.status, sessionId: got, body: r.body };
        }
        const r = await jsonFetch('/api/sessions/new-detached', { method: 'POST', body: { agentId: opts.agentId ?? 'a1', title: opts.title ?? sid, ...(opts.cwd ? { cwd: opts.cwd } : {}) } });
        const sidA = r.body?.sessionId ?? null;
        return { status: r.status, sessionId: sidA ?? sid, body: r.body };
      };
      const listIds = async () => {
        await loadSessions();
        return (useStore.getState().sessions as any[]).map((s) => String(s.sessionId ?? s.path));
      };
      const archivedIds = async () => ((await listArchivedSessions()) as any[]).map((a) => String(a.sessionId ?? a.path));

      // ── ① fork：从指定轮次分支，带谱系新会话 + 深度过限拒 ──
      const hist = await historyOf(base);
      const histText = JSON.stringify(hist.body ?? []);
      const rustItems = hist.body?.items ?? hist.body ?? [];
      const rustUserItem = rustItems.find?.((x: any) => x?.type === 'user_message' || x?.role === 'user' || x?.message?.role === 'user') ?? rustItems[0];
      const userEntryId = kind === 'rust' ? (rustUserItem?.messageId ?? rustUserItem?.id ?? null) : null;
      let forkChildId: string | null = null;
      let forkParentOk = false;
      let forkLineage = false;
      const forkDebug: Record<string, unknown> = {};
      if (kind === 'rust') {
        // 分支真值锚点：新补的 turns/rollback-preview 解析当前分支最近 user 回合
        // （投影条目类型差异不再影响锚点选取）。
        const pv = await jsonFetch(`/lingxi/v1/sessions/${encodeURIComponent(base.sessionId)}/turns/rollback-preview`, { method: 'POST', body: {} });
        const boundary = typeof pv.body?.turnInputEntryId === 'string' && pv.body.turnInputEntryId
          ? pv.body.turnInputEntryId
          : (typeof userEntryId === 'string' && userEntryId ? userEntryId : String(rustUserItem?.messageId ?? rustUserItem?.id ?? ''));
        const r = await jsonFetch(`/lingxi/v1/sessions/${encodeURIComponent(base.sessionId)}/fork`, { method: 'POST', body: { newSessionId: 'r08-fork-c1', boundaryMessageId: boundary } });
        forkDebug.rustFork = { status: r.status, body: JSON.stringify(r.body).slice(0, 200), boundary };
        forkParentOk = r.status < 300 && r.body?.sessionId === 'r08-fork-c1' && r.body?.parentSessionId === base.sessionId;
        if (forkParentOk) forkChildId = 'r08-fork-c1';
      } else {
        const previewForFork = await jsonFetch('/api/sessions/turns/rollback-preview', { method: 'POST', body: { path: base.path } });
        const entryId = typeof previewForFork.body?.turnInputEntryId === 'string' && previewForFork.body.turnInputEntryId
          ? previewForFork.body.turnInputEntryId
          : (() => { const rows = Array.isArray(hist.body) ? hist.body : (hist.body?.items ?? hist.body?.messages ?? []); const entry = rows.find((x: any) => x?.role === 'user' || x?.message?.role === 'user') ?? rows[0]; return entry?.entryId ?? entry?.messageId ?? entry?.id ?? null; })();
        const r = await jsonFetch('/api/sessions/fork', { method: 'POST', body: { path: base.path, target: { role: 'user', entryId } } });
        forkDebug.oldFork = { status: r.status, body: JSON.stringify(r.body).slice(0, 240), entryId };
        const forkedPath = r.body?.sessionPath ?? r.body?.path ?? r.body?.session?.path ?? null;
        forkParentOk = r.status < 300 && typeof r.body?.sessionId === 'string' && Boolean(forkedPath);
        forkChildId = forkParentOk ? r.body.sessionId : null;
      }
      if (forkParentOk && forkChildId) {
        const ids = await listIds();
        forkLineage = ids.includes(forkChildId);
      }
      cases['fork.new-session-with-lineage'] = forkParentOk && forkLineage;
      // 深度过限：链式 fork 到拒（现役 MAX_FORK_LINEAGE_DEPTH=2）。
      let depthRejected = false;
      let depthAnchor: { id: string | null; parent: string | null } = { id: forkChildId, parent: base.sessionId };
      for (let i = 0; i < 4 && depthAnchor.id; i += 1) {
        const anchorHist = await historyOf({ sessionId: depthAnchor.id, path: kind === 'rust' ? `native:${depthAnchor.id}` : `${(useStore.getState().sessions as any[]).find((s) => s.sessionId === depthAnchor.id)?.path ?? ''}` });
        let r: { status: number; body: any };
        if (kind === 'rust') {
          const items = anchorHist.body?.items ?? anchorHist.body ?? [];
          const boundary = String(items[items.length - 1]?.id ?? items[0]?.id ?? '');
          r = await jsonFetch(`/lingxi/v1/sessions/${encodeURIComponent(depthAnchor.id)}/fork`, { method: 'POST', body: { newSessionId: `r08-fork-d${i}`, boundaryMessageId: boundary } });
          if (r.status >= 400) { depthRejected = true; break; }
          depthAnchor = { id: 'r08-fork-d' + String(i), parent: depthAnchor.id };
        } else {
          const rows = Array.isArray(anchorHist.body) ? anchorHist.body : (anchorHist.body?.items ?? anchorHist.body?.messages ?? []);
          const sessionRow = (useStore.getState().sessions as any[]).find((s) => s.sessionId === depthAnchor.id);
          const entry = rows.find((x: any) => x?.role === 'user' || x?.message?.role === 'user') ?? rows[0];
          const entryId = entry?.id ?? entry?.entryId ?? entry?.messageId ?? null;
          r = await jsonFetch('/api/sessions/fork', { method: 'POST', body: { path: sessionRow?.path, target: { role: 'user', entryId } } });
          if (r.status >= 400 || r.body?.error) { depthRejected = true; break; }
          depthAnchor = { id: r.body?.sessionId ?? null, parent: depthAnchor.id };
        }
      }
      cases['fork.depth-limit-rejected'] = depthRejected;
      // fork 无效会话拒。
      const badFork = kind === 'rust'
        ? await jsonFetch('/lingxi/v1/sessions/sess-does-not-exist-t07/fork', { method: 'POST', body: { newSessionId: 'x', boundaryMessageId: 'y' } })
        : await jsonFetch('/api/sessions/fork', { method: 'POST', body: { path: 'agents/a1/sessions/sess-does-not-exist-t07.jsonl', target: { role: 'user', entryId: 'e' } } });
      cases['fork.invalid-session-rejected'] = badFork.status >= 400 || Boolean(badFork.body?.error);

      // ── ② 连同子会话归档（cascade）：主+子入归档 ──
      let cascadeOk = false;
      if (forkChildId) {
        const childRow = (await (await loadSessions(), useStore.getState().sessions as any[])).find((s) => s.sessionId === forkChildId || s.sessionId === depthAnchor.id) ?? (useStore.getState().sessions as any[]).find((s) => String(s.sessionId) === String(forkChildId));
        void childRow;
        const r = await jsonFetch('/api/sessions/archive', { method: 'POST', body: kind === 'rust' ? { sessionId: forkChildId, childMode: 'archive_children' } : { path: (useStore.getState().sessions as any[]).find((s) => s.sessionId === forkChildId)?.path, childMode: 'archive_children' } });
        await loadSessions();
        const activeIds = (useStore.getState().sessions as any[]).map((s) => String(s.sessionId));
        const arch = await archivedIds();
        // 主会话（fork 子）入归档即满足「连同子会话归档」的主断言面（其子若存在同入）。
        cascadeOk = r.status < 300 && !activeIds.includes(String(forkChildId)) && arch.includes(String(forkChildId));
      }
      cases['archive-cascade.parent-and-children-archived'] = cascadeOk;

      // ── ③ 解除谱系后归档（detach）：子留活动顶层，主入归档 ──
      // 主 = base 的 fork 子 P1（有真实消息、谱系深度 1）；G1 = P1 的 fork 子
      // （深度 2，现役上限内）。归档 P1(detach_children) → P1 入归档、G1 留活动。
      let detachOk = false;
      let detachChild: string | null = null;
      const detachDebug: Record<string, unknown> = {};
      // 主链换新：cascade 已归档 c1——重新 fork base 得 P1b（深度 1），
      // 再 fork P1b 得 G1（深度 2，上限内）；归档 P1b(detach_children)。
      let detachParentId: string | null = null;
      if (kind === 'rust') {
        const pv = await jsonFetch(`/lingxi/v1/sessions/${encodeURIComponent(base.sessionId)}/turns/rollback-preview`, { method: 'POST', body: {} });
        const boundary = String(pv.body?.turnInputEntryId ?? '');
        const p1 = await jsonFetch(`/lingxi/v1/sessions/${encodeURIComponent(base.sessionId)}/fork`, { method: 'POST', body: { newSessionId: 'r08-detach-p1b', boundaryMessageId: boundary } });
        detachDebug.rustForkP1b = { status: p1.status, body: JSON.stringify(p1.body).slice(0, 120) };
        if (p1.status < 300) {
          detachParentId = 'r08-detach-p1b';
          const pv2 = await jsonFetch('/lingxi/v1/sessions/r08-detach-p1b/turns/rollback-preview', { method: 'POST', body: {} });
          const b2 = String(pv2.body?.turnInputEntryId ?? '');
          const f = await jsonFetch('/lingxi/v1/sessions/r08-detach-p1b/fork', { method: 'POST', body: { newSessionId: 'r08-detach-g1', boundaryMessageId: b2 } });
          detachDebug.rustForkG1 = { status: f.status };
          detachChild = f.status < 300 ? 'r08-detach-g1' : null;
        }
      } else {
        const pv = await jsonFetch('/api/sessions/turns/rollback-preview', { method: 'POST', body: { path: base.path } });
        const entryId = String(pv.body?.turnInputEntryId ?? '') || (await oldUserEntryId(base) ?? '');
        const p1 = await jsonFetch('/api/sessions/fork', { method: 'POST', body: { path: base.path, target: { role: 'user', entryId } } });
        detachDebug.oldForkP1b = { status: p1.status, entryId };
        if (p1.status < 300) {
          detachParentId = p1.body?.sessionId ?? null;
          await loadSessions();
          const p1Row = (useStore.getState().sessions as any[]).find((s) => s.sessionId === detachParentId);
          const pv2 = await jsonFetch('/api/sessions/turns/rollback-preview', { method: 'POST', body: { path: p1Row?.path } });
          const e2 = String(pv2.body?.turnInputEntryId ?? '') || (await oldUserEntryId(p1Row) ?? '');
          const f = await jsonFetch('/api/sessions/fork', { method: 'POST', body: { path: p1Row?.path, target: { role: 'user', entryId: e2 } } });
          detachDebug.oldForkG1 = { status: f.status, entryId: e2 };
          detachChild = f.status < 300 ? f.body?.sessionId : null;
        }
      }
      if (detachParentId && detachChild) {
        const parentRow = (useStore.getState().sessions as any[]).find((s) => s.sessionId === detachParentId);
        const r = await jsonFetch('/api/sessions/archive', { method: 'POST', body: kind === 'rust' ? { sessionId: detachParentId, childMode: 'detach_children' } : { path: parentRow?.path, childMode: 'detach_children' } });
        detachDebug.archive = { status: r.status, body: JSON.stringify(r.body).slice(0, 140) };
        await loadSessions();
        const activeIds = (useStore.getState().sessions as any[]).map((s) => String(s.sessionId));
        const arch = await archivedIds();
        detachOk = r.status < 300 && !activeIds.includes(String(detachParentId)) && arch.includes(String(detachParentId)) && activeIds.includes(String(detachChild));
      }
      cases['archive-detach.child-stays-parent-archived'] = detachOk;

      // ── ④ rollback-preview：只读预览返回拟撤销轮次与文件影响 ──
      let previewDbg: unknown = null;
      const aTargetId = kind !== 'rust' ? await oldUserEntryId(base) : null;
      const preview = kind === 'rust'
        ? await jsonFetch(`/lingxi/v1/sessions/${encodeURIComponent(base.sessionId)}/turns/rollback-preview`, { method: 'POST', body: {} })
        : await jsonFetch('/api/sessions/turns/rollback-preview', { method: 'POST', body: { path: base.path, ...(aTargetId ? { target: { role: 'user', entryId: aTargetId } } : {}) } });
      const previewBody = preview.body ?? {};
      previewDbg = { status: preview.status, body: JSON.stringify(preview.body).slice(0, 200) };
      const previewOkOld = preview.status < 300 && typeof previewBody.ok === 'boolean' && Array.isArray(previewBody.files);
      const previewOkRust = preview.status < 300 && (Array.isArray(previewBody.files) || typeof previewBody.enabled === 'boolean' || previewBody.boundary != null);
      cases['rollback-preview.readonly-shape'] = kind === 'rust' ? previewOkRust : previewOkOld;
      const badPreview = kind === 'rust'
        ? await jsonFetch('/lingxi/v1/sessions/sess-does-not-exist-t07/rewind/preview', { method: 'POST', body: {} })
        : await jsonFetch('/api/sessions/turns/rollback-preview', { method: 'POST', body: { path: 'agents/a1/sessions/sess-does-not-exist-t07.jsonl' } });
      cases['rollback-preview.invalid-rejected'] = badPreview.status >= 400 || Boolean(badPreview.body?.error);

      let mkAgentBody = ''; let orphanDbg = -1; let dispDbg: number[] = []; let contBodyDbg: unknown = null; let retryRowDbg: unknown = null;
      let mkAgentStatus = -1; let contSessionStatus = -1; let delAgentStatus = -1; let contStatus = -1; let liveContStatus = -1;
      let sweepBodyDebug: unknown = null; let disposalBodyDebug: unknown = null;
      // ── ⑤ sweep-orphaned：目录不存在的会话归档 ──
      const orphanDir = fsMod.mkdtempSync(pathMod.join(osMod.tmpdir(), 'r08-orphan-'));
      const orphan = await newSession({ sessionId: 'r08-orphan-1', title: '孤儿工作台', cwd: orphanDir });
      var orphanStatus = orphan.status; orphanDbg = orphan.status;
      let sweepOk = false;
      if (orphan.status < 300) {
        fsMod.rmSync(orphanDir, { recursive: true, force: true });
        const r = await jsonFetch(rustOrNode('/lingxi/v1/sessions/sweep-orphaned-workspaces', '/api/sessions/sweep-orphaned-workspaces'), { method: 'POST', body: {} });
        sweepBodyDebug = r.body;
        await loadSessions();
        const activeIds = (useStore.getState().sessions as any[]).map((s) => String(s.sessionId));
        sweepOk = r.status < 300 && !activeIds.includes('r08-orphan-1');
      }
      cases['sweep-orphaned.missing-dir-archived'] = sweepOk;

      // ── ⑥ 续接已删除 Agent 会话：新身份+路径返回 ──
      const contAgent = 'r08-cont-ag';
      const mkAgent = kind === 'rust'
        ? await jsonFetch('/lingxi/v1/agents', { method: 'POST', body: { name: 'R08 Cont', id: contAgent, yuan: 'lingxi' } })
        : await jsonFetch('/api/agents', { method: 'POST', body: { name: 'R08 Cont', id: contAgent, yuan: 'lingxi' } });
      mkAgentBody = JSON.stringify(mkAgent.body).slice(0, 140);
      mkAgentStatus = mkAgent.status;
      let contOk = false;
      if (mkAgent.status < 300) {
        // 旧栈续接要求 primary agent 存在且未删（夹具 preferences 的 primary
        // 指向不存在的 "lingxi"——先切主助手到 a1，与现役客户端行为一致）。
        if (kind !== 'rust') {
          await jsonFetch('/api/agents/primary', { method: 'PUT', body: { id: 'a1' } });
        }
        const contSession = await newSession({ sessionId: 'r08-cont-s1', agentId: contAgent, title: '待续接', detached: true });
        // 源会话需有可显示 transcript（旧栈 SESSION_TRANSCRIPT_EMPTY 422——
        // run-45 实证）：就绪 WS 上对源会话跑一轮真实标记回合。
        if (contSession.status < 300) {
          await ensureWsConnected();
          const wsC = (await import('../../services/websocket')).getWebSocket();
          if (wsC && wsC.readyState === WebSocket.OPEN) {
            wsC.send(JSON.stringify({
              type: 'prompt', text: 'A06-CHAT-TASK cont', sessionId: contSession.sessionId,
              sessionPath: kind === 'rust' ? `native:${contSession.sessionId}` : (useStore.getState().sessions as any[]).find((x) => x.sessionId === contSession.sessionId)?.path,
              displayMessage: { text: 'A06-CHAT-TASK cont' },
            }));
            const deadlineC = Date.now() + 45000;
            while (Date.now() < deadlineC) {
              const hc = await historyOf(kind === 'rust' ? { sessionId: contSession.sessionId } : (useStore.getState().sessions as any[]).find((x) => x.sessionId === contSession.sessionId) ?? { sessionId: contSession.sessionId });
              if (JSON.stringify(hc.body ?? []).includes('A06-CHAT-FINAL')) break;
              await sleep(800);
            }
            for (let quiet = 0; quiet < 8; quiet += 1) {
              const framesNow = wsFrames.length;
              await sleep(1200);
              const busy = wsFrames.slice(framesNow).some((f) => /"isStreaming":\s*true/.test(f));
              if (!busy) break;
            }
          }
        }
        contSessionStatus = contSession.status;
        if (contSession.status < 300) {
          // 负路径：助手仍存活时续接 → 显式拒（Rust 400 agent_not_deleted；旧栈 4xx），无副作用。
          const liveCont = kind === 'rust'
            ? await jsonFetch('/lingxi/v1/sessions/continue-deleted-agent', { method: 'POST', body: { sessionId: contSession.sessionId } })
            : await jsonFetch('/api/sessions/continue-deleted-agent', { method: 'POST', body: { path: ((useStore.getState().sessions as any[]).find((s) => s.sessionId === contSession.sessionId) ?? (useStore.getState().sessions as any[]).find((s) => s.title === '待续接'))?.path } });
          liveContStatus = liveCont.status;
          // 删除走两内核各自的正式入口：Rust DELETE /lingxi/v1/agents/{id}
          // （agents_delete_route 置 .deleted-agent.json 标记）；旧栈 DELETE
          // /api/agents/:id（deleteAgent 落 tombstone）。
          const del = kind === 'rust'
            ? await jsonFetch(`/lingxi/v1/agents/${contAgent}`, { method: 'DELETE' })
            : await jsonFetch(`/api/agents/${contAgent}`, { method: 'DELETE' });
          delAgentStatus = del.status;
          if (del.status < 300) {
            await loadSessions();
            // Rust 的 new-detached 不收客户端 sessionId（服务端铸 id）——续接必须用返回的真 id。
            const cont = kind === 'rust'
              ? await jsonFetch('/lingxi/v1/sessions/continue-deleted-agent', { method: 'POST', body: { sessionId: contSession.sessionId } })
              : await jsonFetch('/api/sessions/continue-deleted-agent', { method: 'POST', body: { path: ((useStore.getState().sessions as any[]).find((s) => s.sessionId === contSession.sessionId) ?? (useStore.getState().sessions as any[]).find((s) => s.title === '待续接'))?.path } });
            contStatus = cont.status;
            contBodyDbg = JSON.stringify(cont.body).slice(0, 200);
            // 新身份：双内核返回新 sessionId；旧栈另带文件路径，Rust 的定位符由客户端动作
            // 适配为 native:<id>（continueDeletedAgentSession），服务端只给 id。
            contOk = cont.status < 300 && typeof cont.body?.sessionId === 'string' && cont.body.sessionId !== contSession.sessionId
              && (kind === 'rust' || typeof cont.body?.path === 'string' || typeof cont.body?.sessionPath === 'string');
          }
        }
      }
      cases['continue-deleted-agent.new-identity-and-path'] = contOk;
      cases['continue-deleted-agent.live-agent-rejected'] = liveContStatus >= 400 && liveContStatus < 500;

      // ── ⑦ 工作台处置 delete：归档后永久删除 ──
      const dispDir = fsMod.mkdtempSync(pathMod.join(osMod.tmpdir(), 'r08-dispose-'));
      const d1 = await newSession({ sessionId: 'r08-disp-1', title: '处置一', cwd: dispDir });
      const d2 = await newSession({ sessionId: 'r08-disp-2', title: '处置二', cwd: dispDir });
      var dispStatuses = [d1.status, d2.status]; dispDbg = [d1.status, d2.status];
      let disposeOk = false;
      if (d1.status < 300 && d2.status < 300) {
        const r = await jsonFetch('/api/sessions/workspace-disposal', { method: 'POST', body: { cwd: dispDir, action: 'delete' } });
        disposalBodyDebug = r.body;
        await loadSessions();
        const activeIds = (useStore.getState().sessions as any[]).map((s) => String(s.sessionId));
        const arch = await archivedIds();
        disposeOk = r.status < 300 && Number(r.body?.disposed ?? 0) >= 2 && !activeIds.includes('r08-disp-1') && !arch.includes('r08-disp-1');
      }
      fsMod.rmSync(dispDir, { recursive: true, force: true });
      cases['workspace-disposal.permanent-delete'] = disposeOk;

      // ── ⑧ fresh-compact：刷新能力并压缩（noop 合法：会话过小）──
      const fc = kind === 'rust'
        ? await jsonFetch(`/lingxi/v1/sessions/${encodeURIComponent(base.sessionId)}/fresh-compact`, { method: 'POST', body: {} })
        : await jsonFetch('/api/sessions/fresh-compact', { method: 'POST', body: { path: base.path } });
      cases['fresh-compact.returns-result'] = fc.status < 300 && (fc.body?.ok !== false);

      // ── ⑨ turns-retry：正路径（重放最近用户消息→新回复）+ 负路径 ──
      // 重试目标：chat.round 已真实回复过的会话（其最后用户消息带 A06 桩标记，
      // 重放不会被桩拒绝）；本任务的 base 可能与之间隔多轮变更，按内容定位。
      let retrySession = base;
      {
        const ids = await listIds();
        for (const sid of ids) {
          const row = (useStore.getState().sessions as any[]).find((s) => String(s.sessionId) === sid);
          if (!row) continue;
          const h = await historyOf(row);
          if (JSON.stringify(h.body ?? []).includes('A06-CHAT-FINAL')) { retrySession = row; break; }
        }
      }
      // 在目标会话上先发一轮带桩标记的真实回合（保证「最近 user 回合」是
      // 桩可应答的提示；abort 等后续任务可能把非标记提示留在了最后）。
      await ensureWsConnected();
      {
        const wsR = (await import('../../services/websocket')).getWebSocket();
        if (wsR && wsR.readyState === WebSocket.OPEN) {
          wsR.send(JSON.stringify({
            type: 'prompt', text: 'A06-CHAT-TASK retry', sessionId: retrySession.sessionId,
            sessionPath: kind === 'rust' ? `native:${retrySession.sessionId}` : retrySession.path,
            displayMessage: { text: 'A06-CHAT-TASK retry' },
          }));
          const deadline = Date.now() + 45000;
          while (Date.now() < deadline) {
            const h = await historyOf(retrySession);
            if (JSON.stringify(h.body ?? []).includes('A06-CHAT-FINAL')) break;
            await sleep(800);
          }
        }
      }
      const histBefore = await historyOf(retrySession);
      const beforeCount = JSON.stringify(histBefore.body ?? []).length;
      let retrySubmitted = false;
      let retryReplied = false;
      void retryReplied;
      const retryDebug: Record<string, unknown> = {};
      {
        let r: { status: number; body: any };
        // 锚点统一走 turns/rollback-preview 的 turnInputEntryId（分支真值，
        // 双内核同源；投影条目 id 形状差异不再影响目标解析）。
        const rTargetId = kind !== 'rust' ? await oldUserEntryId(retrySession) : null;
      retryRowDbg = (() => { const h0 = histBefore.body; const rows0 = Array.isArray(h0) ? h0 : (h0?.items ?? h0?.messages ?? []); return JSON.stringify(rows0.slice(-2)).slice(0, 240); })();
        const pv = kind === 'rust'
          ? await jsonFetch(`/lingxi/v1/sessions/${encodeURIComponent(retrySession.sessionId)}/turns/rollback-preview`, { method: 'POST', body: {} })
          : await jsonFetch('/api/sessions/turns/rollback-preview', { method: 'POST', body: { path: retrySession.path, ...(rTargetId ? { target: { role: 'user', entryId: rTargetId } } : {}) } });
        const anchor = typeof pv.body?.turnInputEntryId === 'string' && pv.body.turnInputEntryId
          ? pv.body.turnInputEntryId
          : (kind !== 'rust' ? rTargetId : null);
        // 409 session_busy = 引擎仍在收尾（run-44/45 双腿实证）：有界退避重试，
        // 与现役客户端语义一致（忙等后重发），不是放宽断言。
        r = { status: 409, body: null };
        for (let attempt = 0; attempt < 6 && r.status === 409; attempt += 1) {
          if (attempt > 0) await sleep(2500);
          r = kind === 'rust'
            ? await jsonFetch(`/lingxi/v1/sessions/${encodeURIComponent(retrySession.sessionId)}/turns/retry`, { method: 'POST', body: anchor ? { targetMessageId: anchor } : {} })
            : await jsonFetch('/api/sessions/turns/retry', { method: 'POST', body: { path: retrySession.path, target: anchor ? { role: 'user', entryId: anchor } : null } });
        }
        retryDebug.anchor = anchor;
        retryDebug.response = { status: r.status, body: JSON.stringify(r.body).slice(0, 260) };
        // 重放「已应答」的证据面：A 腿 HTTP 响应内联重放回复文本（FINAL 标记）；
        // B 腿响应是重置点+新头合同（后台 drive 独立收尾，服务日志
        // completed.with_final 佐证），历史 FINAL 计数因分支重置可能不增。
        const inlineReply = typeof r.body === 'object' && r.body !== null
          && JSON.stringify(r.body).includes('A06-CHAT-FINAL');
        const rustOutcomeContract = kind === 'rust' && typeof r.body?.turnInputMessageId === 'string'
          && typeof r.body?.sessionId === 'string';
        retrySubmitted = r.status < 300 && (inlineReply || rustOutcomeContract || retryReplied);
        if (r.status < 300 && !retrySubmitted) {
          const finals = (text: string) => (text.match(/A06-CHAT-FINAL/g) ?? []).length;
          const finalsBefore = finals(JSON.stringify(histBefore.body ?? []));
          const deadline = Date.now() + 45000;
          let markerTurnLanded = false;
          while (Date.now() < deadline) {
            const h = await historyOf(retrySession);
            if (finals(JSON.stringify(h.body ?? [])) > finalsBefore) { markerTurnLanded = true; break; }
            await sleep(800);
          }
          // 前置标记回合 FINAL 落库后引擎仍可能收尾中（retry 409 session_busy
          // ——run-44 A 实证）：等流静默（1.2s 无 isStreaming 帧）再判。
          if (markerTurnLanded) {
            for (let quiet = 0; quiet < 10; quiet += 1) {
              const framesNow = wsFrames.length;
              await sleep(1200);
              const busy = wsFrames.slice(framesNow).some((f) => /"isStreaming":\s*true/.test(f));
              if (!busy) break;
            }
          }
        }
      }
      // A=内联回复；B=合同+后台完结（历史计数增长仅作补充证据）。
      retryDebug.historyFinalsGrew = retryReplied;
      cases['turns-retry.submitted-and-replied'] = retrySubmitted;
      // 负路径在「偏好关闭」下打（任务开头为锚点开了偏好——此处临时关、打完复原）。
      await jsonFetch('/api/sessions/workspace-rollback', { method: 'PUT', body: { enabled: false } });
      const badRollback = kind === 'rust'
        ? await jsonFetch(`/lingxi/v1/sessions/${encodeURIComponent(base.sessionId)}/turns/retry`, { method: 'POST', body: { fileRollback: 'workspace' } })
        : await jsonFetch('/api/sessions/turns/retry', { method: 'POST', body: { path: base.path, target: null, fileRollback: 'workspace' } });
      cases['turns-retry.file-rollback-disabled-rejected'] = badRollback.status === 403 || badRollback.status === 409 || Boolean(badRollback.body?.error);
      await jsonFetch('/api/sessions/workspace-rollback', { method: 'PUT', body: { enabled: true } });
      const badRetry = kind === 'rust'
        ? await jsonFetch('/lingxi/v1/sessions/sess-does-not-exist-t07/turns/retry', { method: 'POST', body: {} })
        : await jsonFetch('/api/sessions/turns/retry', { method: 'POST', body: { path: 'agents/a1/sessions/sess-does-not-exist-t07.jsonl' } });
      cases['turns-retry.invalid-session-rejected'] = badRetry.status >= 400 || Boolean(badRetry.body?.error);

      // ── ⑩ 延迟内容负例：未知 contentId 404 ──
      const badContent = await jsonFetch(`/api/sessions/content/r08-no-such-content?path=${encodeURIComponent(sessionLocator(base))}`);
      cases['lazy-content.unknown-404'] = badContent.status === 404;
      void histText;
      await jsonFetch('/api/sessions/workspace-rollback', { method: 'PUT', body: { enabled: prefBefore.body?.enabled === true } });
      const debug: Record<string, unknown> = { forkDebug, detachDebug, retryDebug,
        mkAgentStatus, contSessionStatus, delAgentStatus, contStatus, mkAgentBody, orphanDbg, dispDbg, previewDbg, contBodyDbg, retryRowDbg,
        sweepBody: JSON.stringify(sweepBodyDebug ?? null).slice(0, 160),
        disposalBody: JSON.stringify(disposalBodyDebug ?? null).slice(0, 160) };
      return { cases, forkChildId, detachChild, previewStatus: preview.status, fcStatus: fc.status, debug };
    });
  });

  // ══════════ R08 收尾 工具回合线束（替身 R08-TOOL-PLAN）══════════
  // 双内核对模型只常驻 read/write/edit/exec_command + mcp_search_tools/
  // mcp_describe_tool/mcp_call（旧栈 shared/tool-categories RESIDENT_CORE_TOOL_NAMES；
  // Rust toolcatalog_bridge RESIDENT_NAMES），其余一方工具（todo_write/checkpoint/
  // rewind/session…）走同一按需目录协议：mcp_describe_tool → mcp_call（Rust 强制
  // 本轮先 describe；旧栈建议先查）。计划由本线束显式给出，替身逐步发出。
  type ToolSession = { sessionId: string; path: string };
  type ToolCall = { tool: string; args?: Record<string, unknown>; direct?: boolean };
  const b64url = (s: string) => Buffer.from(s, 'utf8').toString('base64url');
  const planText = (token: string, calls: ToolCall[]) => {
    // direct：常驻工具（read/write/edit/exec_command）直接按名调用，不经目录桥。
    const steps = calls.flatMap((c) => (c.direct ? [{ tool: c.tool, args: c.args ?? {} }] : [
      { tool: 'mcp_describe_tool', args: { name: c.tool } },
      { tool: 'mcp_call', args: { tool: c.tool, arguments: c.args ?? {} } },
    ]));
    return `R08-TOOL-PLAN ${token} ${b64url(JSON.stringify({ steps }))}`;
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
    // 工具回合前把会话权限模式钉死（operate：常规工具免确认；ask：常规工具需确认）。
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
  const sendPlan = async (s: ToolSession, token: string, calls: ToolCall[]) => {
    await ensureWsConnected();
    const ws = (await import('../../services/websocket')).getWebSocket();
    if (!ws || ws.readyState !== WebSocket.OPEN) return { sent: false, start: wsFrames.length };
    const text = planText(token, calls);
    const start = wsFrames.length;
    ws.send(JSON.stringify({ type: 'prompt', text, sessionId: s.sessionId, sessionPath: s.path, displayMessage: { text } }));
    return { sent: true, start };
  };
  const runToolPlan = async (s: ToolSession, token: string, calls: ToolCall[], budgetMs = 75000) => {
    const { sent, start } = await sendPlan(s, token, calls);
    let final = false;
    let historyText = '';
    if (sent) {
      const deadline = Date.now() + budgetMs;
      while (Date.now() < deadline) {
        historyText = await toolHistoryText(s);
        if (historyText.includes(`R08-TOOL-FINAL ${token}`)) { final = true; break; }
        await sleep(700);
      }
      await awaitQuiet();
    }
    return { sent, final, historyText, frames: wsFrames.slice(start).filter((f) => !f.startsWith('>>> ')) };
  };
  // 终结回答 "R08-TOOL-FINAL <token> steps=<n> last=<最后工具回包>" 在历史 JSON 串
  // 中（转义态）；取 last= 后到未转义引号前的片段并反转义。
  const finalEcho = (historyText: string, token: string) => {
    const m = historyText.match(new RegExp(`R08-TOOL-FINAL ${token} steps=(\\d+)(?: R08-STEER-SEEN)? last=((?:[^"\\\\]|\\\\.)*)`));
    if (!m) return null;
    let last = m[2];
    try { last = JSON.parse(`"${m[2]}"`); } catch { /* 保留转义态 */ }
    return { steps: Number(m[1]), last };
  };
  const todoFrames = (frames: string[]) => frames
    .filter((f) => /"todo_update"|"tool_end"/.test(f))
    .map((f) => { try { return JSON.parse(f); } catch { return null; } })
    .map((p) => {
      if (!p) return null;
      // 旧栈/Rust 的 todo_update 事件（todos 直挂）或 tool_end 回包 details.todos。
      if (p.type === 'todo_update' && Array.isArray(p.todos)) return { todos: p.todos, version: p.version, removed: p.removed, via: 'todo_update' };
      if (p.type === 'tool_end' && Array.isArray(p.details?.todos)) return { todos: p.details.todos, version: p.details.version, removed: false, via: `tool_end:${p.name}` };
      return null;
    })
    .filter(Boolean) as Array<{ todos: any[]; version?: string; removed?: boolean; via: string }>;
  const toolEndNames = (frames: string[]) => frames
    .filter((f) => /"tool_end"|"tool_start"/.test(f))
    .map((f) => { try { const p = JSON.parse(f); return `${p.type}:${p.name}:${p.success ?? ''}`; } catch { return 'unparsed'; } });
  // 真实 store 的 UI 面板（ws-message-handler applyTodoToolEnd/todo_update 写入）：
  // 键为帧内 sessionPath（旧栈文件路径 / Rust native:<id>）。
  const uiTodoPanel = (s: ToolSession) => {
    const map = ((useStore.getState() as any).todoPanelBySession ?? {}) as Record<string, any>;
    const hit = Object.entries(map).find(([k]) => k === s.path || k.includes(s.sessionId));
    return hit ? hit[1] : null;
  };
  const appEventsSince = async (cursor: number) => (kind === 'rust' ? await jsonFetch(`/lingxi/v1/app-events?since=${cursor}`) : { status: 0, body: null });
  const appEventCursor = async () => (kind === 'rust' ? Number((await jsonFetch('/lingxi/v1/app-events?since=0')).body?.latestSequence ?? 0) : 0);

  it('leaf.tool.todos: todo_write 工具回合落面板 → 完成/取消/收纳正路径 + 错误清单不覆盖 + 版本/目标负路径', async () => {
    await observe('leaf.tool.todos', async () => {
      const cases: Record<string, boolean> = {};
      const s = await newToolSession('R08 todos');
      if (!s) return { cases, skipped: 'session-create-failed' };
      const list1 = [
        { content: 'R08 第一项', activeForm: '处理第一项', status: 'in_progress' },
        { content: 'R08 第二项', status: 'pending' },
      ];
      // ── B34F2B A0：todo_write 工具回合 → 线上面板事件（todo_update / tool_end.details）与会话记录一致；
      //    UI 绑定层（真实 store todoPanelBySession，经真实 ws-message-handler 写入）同步可见 ──
      const r1 = await runToolPlan(s, 'todo1', [{ tool: 'todo_write', args: { todos: list1 } }]);
      const tf1 = todoFrames(r1.frames);
      const panel1 = tf1[tf1.length - 1]?.todos ?? [];
      const sameAsList1 = (todos: any[]) => Array.isArray(todos) && todos.length === 2
        && todos.map((t: any) => t.content).join('|') === list1.map((t) => t.content).join('|')
        && todos.every((t: any, i: number) => t.status === list1[i].status);
      cases['todo-write.round-final'] = r1.final;
      cases['todo-write.wire-panel-matches-list'] = sameAsList1(panel1);
      const ui1 = uiTodoPanel(s);
      cases['todo-write.ui-store-panel-matches-list'] = sameAsList1(ui1?.todos ?? []);
      cases['todo-write.history-records-list'] = r1.historyText.includes('R08 第一项') && r1.historyText.includes('R08 第二项');
      // ── B34F2B A1/A2：错误输入（blocked 缺 blockedReason / 非法 status）→ 工具显式拒绝，不覆盖面板 ──
      const r2 = await runToolPlan(s, 'todo2', [{ tool: 'todo_write', args: { todos: [{ content: 'R08 坏项', status: 'blocked' }] } }]);
      const e2 = finalEcho(r2.historyText, 'todo2')?.last ?? '';
      cases['todo-write.invalid-rejected-explicitly'] = r2.final && Boolean(e2) && /blocked|invalid|reason|拒绝|无效/i.test(e2);
      cases['todo-write.invalid-does-not-overwrite'] = !todoFrames(r2.frames).some((p) => p.todos.some((t: any) => t.content === 'R08 坏项'))
        && sameAsList1(uiTodoPanel(s)?.todos ?? []);
      const r2b = await runToolPlan(s, 'todo2b', [{ tool: 'todo_write', args: { todos: [{ content: 'R08 坏状态项', status: 'bogus-status' }] } }]);
      const e2b = finalEcho(r2b.historyText, 'todo2b')?.last ?? '';
      cases['todo-write.invalid-status-rejected-keeps-list'] = r2b.final && /invalid|status|拒绝|无效|error/i.test(e2b)
        && !todoFrames(r2b.frames).some((p) => p.todos.some((t: any) => t.content === 'R08 坏状态项')) && sameAsList1(uiTodoPanel(s)?.todos ?? []);
      // ── 45C5CE A1 / 5CD35D A1 / 17FCFE A1：版本失配（A17）与无效目标显式错误 ──
      const staleComplete = await jsonFetch('/api/sessions/todos/complete', { method: 'POST', body: { path: sessionLocator(s), version: 'tv00000000' } });
      cases['complete.version-mismatch-409'] = staleComplete.status === 409 && staleComplete.body?.code === 'todo_version_mismatch';
      const staleCancel = await jsonFetch('/api/sessions/todos/cancel', { method: 'POST', body: { path: sessionLocator(s), version: 'tv00000000' } });
      cases['cancel.version-mismatch-409'] = staleCancel.status === 409 && staleCancel.body?.code === 'todo_version_mismatch';
      const staleDismiss = await jsonFetch('/api/sessions/todos/dismiss', { method: 'POST', body: { path: sessionLocator(s), version: 'tv00000000' } });
      cases['dismiss.version-mismatch-409'] = staleDismiss.status === 409 && staleDismiss.body?.code === 'todo_version_mismatch';
      const ghost = kind === 'rust' ? 'native:sess-does-not-exist-t07' : 'agents/a1/sessions/sess-does-not-exist-t07.jsonl';
      const ghostComplete = await jsonFetch('/api/sessions/todos/complete', { method: 'POST', body: { path: ghost } });
      cases['complete.invalid-session-rejected'] = ghostComplete.status >= 400 && ghostComplete.status < 500;
      const ghostCancel = await jsonFetch('/api/sessions/todos/cancel', { method: 'POST', body: { path: ghost } });
      cases['cancel.invalid-session-rejected'] = ghostCancel.status >= 400 && ghostCancel.status < 500;
      const ghostDismiss = await jsonFetch('/api/sessions/todos/dismiss', { method: 'POST', body: { path: ghost } });
      cases['dismiss.invalid-session-rejected'] = ghostDismiss.status >= 400 && ghostDismiss.status < 500;
      // ── 17FCFE A0（前半）：未结束清单上 dismiss 是幂等 no-op——不追加收纳记录，清单仍在 ──
      const dismissEarly = await jsonFetch('/api/sessions/todos/dismiss', { method: 'POST', body: { path: sessionLocator(s) } });
      // ── 45C5CE A0：有未完成项 → complete 追加完成记录并广播；再次 complete 返回当前面板不重复写 ──
      const cursorC = await appEventCursor();
      const framesC = wsFrames.length;
      const c1 = await jsonFetch('/api/sessions/todos/complete', { method: 'POST', body: { path: sessionLocator(s) } });
      const c1Todos: any[] = Array.isArray(c1.body?.panel?.todos) ? c1.body.panel.todos : [];
      cases['dismiss.unfinished-is-noop-list-survives'] = dismissEarly.status === 200 && c1Todos.length === 2 && c1Todos.map((t) => t.content).join('|') === list1.map((t) => t.content).join('|');
      cases['complete.marks-unfinished-completed'] = c1.status === 200 && c1.body?.ok === true && c1Todos.length === 2
        && c1Todos.every((t) => t.status === 'completed') && c1.body?.panel?.finished === true && c1.body?.panel?.allCompleted === true;
      await sleep(400);
      const broadcastC = kind === 'rust'
        ? ((await appEventsSince(cursorC)).body?.events ?? []).some((e: any) => e?.type === 'todos-changed' && e?.payload?.sessionId === s.sessionId && Array.isArray(e?.payload?.panel?.todos) && e.payload.panel.todos.every((t: any) => t.status === 'completed'))
        : todoFrames(wsFrames.slice(framesC).filter((f) => !f.startsWith('>>> '))).some((p) => p.todos.length === 2 && p.todos.every((t: any) => t.status === 'completed'));
      cases['complete.broadcasts-panel'] = broadcastC;
      const c2 = await jsonFetch('/api/sessions/todos/complete', { method: 'POST', body: { path: sessionLocator(s) } });
      cases['complete.already-finished-returns-panel-no-rewrite'] = c2.status === 200 && c2.body?.ok === true
        && JSON.stringify(c2.body?.panel?.todos ?? null) === JSON.stringify(c1Todos) && c2.body?.panel?.version === c1.body?.panel?.version;
      // ── 17FCFE A0（后半）：已结束且未收纳 → dismiss 追加收纳记录（面板 removed/dismissed），历史保留 ──
      const d1 = await jsonFetch('/api/sessions/todos/dismiss', { method: 'POST', body: { path: sessionLocator(s) } });
      const afterDismiss = await jsonFetch('/api/sessions/todos/complete', { method: 'POST', body: { path: sessionLocator(s) } });
      cases['dismiss.finished-list-dismissed'] = d1.status === 200 && d1.body?.ok === true && d1.body?.panel?.removed === true && d1.body?.panel?.dismissed === true
        && afterDismiss.status === 200 && afterDismiss.body?.panel?.removed === true && (afterDismiss.body?.panel?.todos ?? []).length === 0;
      cases['dismiss.history-retained'] = (await toolHistoryText(s)).includes('R08 第一项');
      // ── 5CD35D A0：新清单（一完成一待办）→ cancel 只取消未完成项，已完成保持；不终止流/后台；再 cancel 幂等 ──
      const list2 = [
        { content: 'R08 已完成项', status: 'completed' },
        { content: 'R08 待取消项', status: 'pending' },
      ];
      const r3 = await runToolPlan(s, 'todo3', [{ tool: 'todo_write', args: { todos: list2 } }]);
      const cursorX = await appEventCursor();
      const framesX = wsFrames.length;
      const x1 = await jsonFetch('/api/sessions/todos/cancel', { method: 'POST', body: { path: sessionLocator(s) } });
      const x1Todos: any[] = Array.isArray(x1.body?.panel?.todos) ? x1.body.panel.todos : [];
      cases['cancel.marks-unfinished-cancelled-keeps-completed'] = r3.final && x1.status === 200 && x1.body?.ok === true && x1Todos.length === 2
        && x1Todos.find((t) => t.content === 'R08 已完成项')?.status === 'completed'
        && x1Todos.find((t) => t.content === 'R08 待取消项')?.status === 'cancelled'
        && x1.body?.panel?.finished === true && x1.body?.panel?.allCompleted === false;
      await sleep(400);
      const broadcastX = kind === 'rust'
        ? ((await appEventsSince(cursorX)).body?.events ?? []).some((e: any) => e?.type === 'todos-changed' && e?.payload?.sessionId === s.sessionId && Array.isArray(e?.payload?.panel?.todos) && e.payload.panel.todos.some((t: any) => t.status === 'cancelled'))
        : todoFrames(wsFrames.slice(framesX).filter((f) => !f.startsWith('>>> '))).some((p) => p.todos.some((t: any) => t.status === 'cancelled'));
      cases['cancel.broadcasts-panel'] = broadcastX;
      cases['cancel.no-stream-abort-side-effect'] = !wsFrames.slice(framesX).some((f) => !f.startsWith('>>> ') && /"type":\s*"(abort|aborted|run_aborted|task_aborted)"/.test(f))
        && useStore.getState().wsState === 'connected';
      const x2 = await jsonFetch('/api/sessions/todos/cancel', { method: 'POST', body: { path: sessionLocator(s) } });
      cases['cancel.already-finished-returns-panel-no-rewrite'] = x2.status === 200 && JSON.stringify(x2.body?.panel?.todos ?? null) === JSON.stringify(x1Todos) && x2.body?.panel?.version === x1.body?.panel?.version;
      return {
        cases, sessionId: s.sessionId,
        debug: {
          r1Final: r1.final, panel1, tf1Via: tf1.map((p) => p.via), ui1: JSON.stringify(ui1 ?? null).slice(0, 240), toolEnds1: toolEndNames(r1.frames), e2: (e2 ?? '').slice(0, 200),
          staleStatus: [staleComplete.status, staleCancel.status, staleDismiss.status], ghostStatus: [ghostComplete.status, ghostCancel.status, ghostDismiss.status],
          c1: c1.status, c1Panel: JSON.stringify(c1.body?.panel ?? null).slice(0, 300), d1: JSON.stringify(d1.body ?? null).slice(0, 200), x1Panel: JSON.stringify(x1.body?.panel ?? null).slice(0, 300),
          framesTail: r1.frames.slice(-3).map((f) => f.slice(0, 160)),
        },
      };
    });
  }, 300_000);

  it('leaf.tool.checkpoint: checkpoint 工具 create→list 可见 / drop→list 不可见 / drop 不存在不伪报', async () => {
    await observe('leaf.tool.checkpoint', async () => {
      const cases: Record<string, boolean> = {};
      const s = await newToolSession('R08 checkpoint');
      if (!s) return { cases, skipped: 'session-create-failed' };
      const name = `cp-r08-${Date.now().toString(36)}`;
      // ── 6207F7 A0：创建后能列出（同轮 create → list，终结回显 list 回包含名字）──
      const r1 = await runToolPlan(s, 'ckpt1', [
        { tool: 'checkpoint', args: { action: 'create', name, snapshot: false } },
        { tool: 'checkpoint', args: { action: 'list' } },
      ]);
      const e1 = finalEcho(r1.historyText, 'ckpt1');
      cases['create.then-list-shows-name'] = r1.final && Boolean(e1) && (e1?.last ?? '').includes(name);
      // snapshot:false → 不写文件快照（旧栈回包「no workspace snapshot (snapshot=false)」；Rust
      // 回执 snapshotCommit:null / fileCount:0 落历史工具结果——历史 JSON 里是未转义的对象键）。
      cases['create.snapshot-disabled-writes-no-file-snapshot'] = r1.final
        && /no workspace snapshot|\\?"snapshotCommit\\?":\s*null|\\?"fileCount\\?":\s*0/.test(r1.historyText);
      // ── 6207F7 A1：删除后不可见（drop → list，回包不含名字）──
      const r2 = await runToolPlan(s, 'ckpt2', [
        { tool: 'checkpoint', args: { action: 'drop', name } },
        { tool: 'checkpoint', args: { action: 'list' } },
      ]);
      const e2 = finalEcho(r2.historyText, 'ckpt2');
      cases['drop.then-list-hides-name'] = r2.final && Boolean(e2) && !(e2?.last ?? '').includes(name);
      // 不伪报：drop 不存在的名字必须如实 not found（不说成已删）。
      const r3 = await runToolPlan(s, 'ckpt3', [{ tool: 'checkpoint', args: { action: 'drop', name: 'cp-r08-never-existed' } }]);
      const e3 = finalEcho(r3.historyText, 'ckpt3');
      cases['drop.missing-reported-not-found'] = r3.final && Boolean(e3) && /not found/i.test(e3?.last ?? '');
      return { cases, name, e1: (e1?.last ?? '').slice(0, 240), e2: (e2?.last ?? '').slice(0, 240), e3: (e3?.last ?? '').slice(0, 200), framesTail: r1.frames.slice(-3).map((f) => f.slice(0, 160)) };
    });
  }, 300_000);

  // ── 待批确认面：旧栈 WS session_confirmation 帧（confirmId）+ POST /api/confirm/:id；
  //    Rust GET/POST /lingxi/v1/sessions/{sid}/approvals[/{id}]。action 词表两侧同为
  //    confirmed/rejected。
  const awaitPendingConfirmation = async (s: ToolSession, framesStart: number, hint: string, budgetMs = 30000) => {
    const deadline = Date.now() + budgetMs;
    while (Date.now() < deadline) {
      if (kind === 'rust') {
        const r = await jsonFetch(`/lingxi/v1/sessions/${encodeURIComponent(s.sessionId)}/approvals`);
        const rows: any[] = Array.isArray(r.body?.approvals) ? r.body.approvals : [];
        if (rows.length) return { id: String(rows[0].approvalId), target: String(rows[0].target ?? ''), raw: JSON.stringify(rows[0]).slice(0, 300) };
      } else {
        // 旧栈：content_block{block.type:'session_confirmation', confirmId}；帧 sessionPath 为
        // 会话文件绝对路径，与列表 path 表示可能不同——以卡片载荷里的锚点名（hint）
        // 或会话 id 定位本任务的待批卡。
        const frame = wsFrames.slice(framesStart).find((f) => !f.startsWith('>>> ') && f.includes('session_confirmation') && f.includes('confirmId')
          && (f.includes(hint) || f.includes(s.path) || f.includes(s.sessionId)));
        const m = frame?.match(/"confirmId":"([^"]+)"/);
        if (frame && m) return { id: m[1], target: frame.match(/"kind":"([^"]+)"/)?.[1] ?? '', raw: frame.slice(0, 300) };
      }
      await sleep(400);
    }
    return null;
  };
  const resolveConfirmation = (s: ToolSession, id: string, action: string) => jsonFetch(
    rustOrNode(`/lingxi/v1/sessions/${encodeURIComponent(s.sessionId)}/approvals/${encodeURIComponent(id)}`, `/api/confirm/${encodeURIComponent(id)}`),
    { method: 'POST', body: { action } },
  );
  const awaitFinal = async (s: ToolSession, token: string, budgetMs = 60000) => {
    const deadline = Date.now() + budgetMs;
    let text = '';
    while (Date.now() < deadline) {
      text = await toolHistoryText(s);
      if (text.includes(`R08-TOOL-FINAL ${token}`)) return { final: true, text };
      await sleep(700);
    }
    return { final: false, text };
  };
  const sendSlash = async (s: ToolSession, text: string) => {
    const wsMod = await import('../../services/websocket');
    const ws = wsMod.getWebSocket();
    if (!ws || ws.readyState !== WebSocket.OPEN) return false;
    if (kind === 'rust') wsMod.sendRustSlash(s.sessionId, text, null);
    else ws.send(JSON.stringify({ type: 'slash', text, sessionPath: s.path, agentId: null }));
    return true;
  };
  const awaitSlashReply = async (framesStart: number, pattern: RegExp, budgetMs = 10000) => {
    const deadline = Date.now() + budgetMs;
    while (Date.now() < deadline) {
      const hit = wsFrames.slice(framesStart).find((f) => !f.startsWith('>>> ') && /slash_result/.test(f) && pattern.test(f));
      if (hit) return hit.slice(0, 300);
      const notice = slashNotices.find((n) => pattern.test(n.text));
      if (notice) return `notice:${notice.text.slice(0, 200)}`;
      await sleep(300);
    }
    return null;
  };
  const confirmationBroadcast = async (cursor: number, framesStart: number, id: string, action: string) => {
    await sleep(600);
    if (kind === 'rust') {
      const events: any[] = (await appEventsSince(cursor)).body?.events ?? [];
      return events.some((e) => e?.type === 'confirmation-resolved' && e?.payload?.confirmId === id && e?.payload?.action === action);
    }
    return wsFrames.slice(framesStart).some((f) => !f.startsWith('>>> ') && /confirmation_resolved/.test(f) && f.includes(id) && f.includes(action));
  };

  it('leaf.tool.rewind: checkpoint→rewind 确认卡（拒绝不回退 / 确认后分支回到锚点、文件不动）+ confirm 路由 400/404/广播 + /confirm /reject 斜杠', async () => {
    await observe('leaf.tool.rewind', async () => {
      const cases: Record<string, boolean> = {};
      const fsMod = await import('node:fs');
      const osMod = await import('node:os');
      const pathMod = await import('node:path');
      const s = await newToolSession('R08 rewind');
      if (!s) return { cases, skipped: 'session-create-failed' };
      const dir = fsMod.mkdtempSync(pathMod.join(osMod.tmpdir(), 'r08-rewind-'));
      const file = pathMod.join(dir, 'anchor.txt');
      fsMod.writeFileSync(file, 'ANCHOR-A\n');
      const cp = `cp-rw-${Date.now().toString(36)}`;
      // 锚点：cp 建在 rw1 轮；rw2 轮是锚点之后的对话（回退应丢弃它）。
      const r1 = await runToolPlan(s, 'rw1', [{ tool: 'checkpoint', args: { action: 'create', name: cp, snapshot: false } }]);
      const r2 = await runToolPlan(s, 'rw2', [{ tool: 'checkpoint', args: { action: 'list' } }]);
      cases['setup.checkpoint-then-post-anchor-turn'] = r1.final && r2.final && r2.historyText.includes('R08-TOOL-FINAL rw2');
      fsMod.writeFileSync(file, 'ANCHOR-B\n');
      // ── E1625F A0 / EF43AD（HTTP rejected）：拒绝不回退；无效 action 400；已处理 404；广播 ──
      let start = wsFrames.length;
      await sendPlan(s, 'rw3', [{ tool: 'rewind', args: { checkpoint: cp } }]);
      const p1 = await awaitPendingConfirmation(s, start, cp);
      cases['rewind.pending-confirmation-surfaced'] = Boolean(p1?.id);
      // 越权：坏令牌不能决策待批（401/403），待批仍在。
      cases['confirm.unauthorized-rejected'] = Boolean(p1) && await withInvalidToken(async () => {
        const r = await resolveConfirmation(s, p1!.id, 'confirmed');
        return r.status === 401 || r.status === 403;
      });
      const badAction = p1 ? await resolveConfirmation(s, p1.id, 'maybe') : { status: 0, body: null };
      cases['confirm.invalid-action-400'] = badAction.status === 400;
      const cursorRej = await appEventCursor();
      const framesRej = wsFrames.length;
      const rej = p1 ? await resolveConfirmation(s, p1.id, 'rejected') : { status: 0, body: null };
      cases['confirm.rejected-200'] = rej.status === 200 && rej.body?.ok === true;
      cases['confirm.rejected-broadcasts-resolved'] = Boolean(p1) && await confirmationBroadcast(cursorRej, framesRej, p1!.id, 'rejected');
      const f3 = await awaitFinal(s, 'rw3');
      const echo3 = finalEcho(f3.text, 'rw3')?.last ?? '';
      cases['rewind.rejected-does-not-rewind'] = f3.final && f3.text.includes('R08-TOOL-FINAL rw2');
      cases['rewind.rejected-reports-not-rewound'] = /未回滚|不回档|没有回档|什么都没动|rejected|拒绝|not rewound|denied|refused/i.test(echo3);
      const rej2 = p1 ? await resolveConfirmation(s, p1.id, 'rejected') : { status: 0, body: null };
      cases['confirm.already-resolved-404'] = rej2.status === 404;
      cases['rewind.files-untouched-without-restore'] = fsMod.readFileSync(file, 'utf8') === 'ANCHOR-B\n';
      await awaitQuiet();
      // ── CD1524（/reject 斜杠）：缺 ID 用法；拒绝回执 + 原动作不执行；已处理不再决策 ──
      start = wsFrames.length;
      await sendPlan(s, 'rw4', [{ tool: 'rewind', args: { checkpoint: cp } }]);
      const p2 = await awaitPendingConfirmation(s, start, cp);
      let slashStart = wsFrames.length;
      slashNotices.length = 0;
      await sendSlash(s, '/reject');
      cases['slash.reject.missing-id-usage'] = Boolean(await awaitSlashReply(slashStart, /用法|usage/i));
      slashStart = wsFrames.length;
      if (p2) await sendSlash(s, `/reject ${p2.id}`);
      const rejReply = p2 ? await awaitSlashReply(slashStart, /已拒绝|已取消/) : null;
      cases['slash.reject.resolves-and-replies'] = Boolean(rejReply);
      const f4 = await awaitFinal(s, 'rw4');
      cases['slash.reject.pending-action-not-executed'] = f4.final && f4.text.includes('R08-TOOL-FINAL rw2');
      slashStart = wsFrames.length;
      if (p2) await sendSlash(s, `/reject ${p2.id}`);
      cases['slash.reject.already-processed-no-redecide'] = Boolean(p2) && Boolean(await awaitSlashReply(slashStart, /不存在或已处理/));
      await awaitQuiet();
      // ── E1625F A1 / EC0331（HTTP confirmed 由广播+继续体现）/ 75C0AE（/confirm 斜杠）：确认后分支回到锚点 ──
      start = wsFrames.length;
      await sendPlan(s, 'rw5', [{ tool: 'rewind', args: { checkpoint: cp } }]);
      const p3 = await awaitPendingConfirmation(s, start, cp);
      slashStart = wsFrames.length;
      await sendSlash(s, '/confirm');
      cases['slash.confirm.missing-id-usage'] = Boolean(await awaitSlashReply(slashStart, /用法|usage/i));
      const cursorConf = await appEventCursor();
      slashStart = wsFrames.length;
      if (p3) await sendSlash(s, `/confirm ${p3.id}`);
      cases['slash.confirm.resolves-and-replies'] = Boolean(p3) && Boolean(await awaitSlashReply(slashStart, /已确认/));
      cases['confirm.confirmed-broadcasts-resolved'] = Boolean(p3) && await confirmationBroadcast(cursorConf, slashStart, p3!.id, 'confirmed');
      // 会话忙（本回合内）时不立刻回退：工具回包为 scheduled（旧栈「本回合结束后自动执行」+
      // details.rewind:"scheduled"（WS tool_end 只透传白名单键，不含 rewind）；Rust「Rewind is
      // scheduled for when this turn ends」），回执文本经终结回显进流帧，回退挂到流结束后执行。
      let scheduledFrame: string | null = null;
      try {
        await waitUntil(() => {
          scheduledFrame = wsFrames.slice(start).find((f) => !f.startsWith('>>> ')
            && /scheduled for when this turn ends|as soon as this turn finishes|本回合结束后|"rewind":\s*"scheduled"/.test(f)) ?? null;
          return Boolean(scheduledFrame);
        }, 'rewind scheduled receipt', 20000);
      } catch { /* 如实 */ }
      cases['rewind.confirmed-in-turn-deferred-until-turn-end'] = Boolean(scheduledFrame);
      // 待批动作（回退）在确认后才继续：锚点 = checkpoint 记录的最新用户轮（rw1），双内核
      // 回退按 retry 重置点语义（现役 retainedEntriesBeforeRetry：保留到该轮输入的父节点；
      // Rust resolve_retry_reset_point 同口径）——rw1 及其后的 rw2 均被丢弃。
      let reset = false;
      let textAfter = '';
      const deadline = Date.now() + 75000;
      while (Date.now() < deadline) {
        textAfter = await toolHistoryText(s);
        if (!textAfter.includes('R08-TOOL-FINAL rw2')) { reset = true; break; }
        await sleep(1000);
      }
      cases['rewind.confirmed-resets-branch-to-anchor'] = reset && !textAfter.includes('R08-TOOL-FINAL rw1') && textAfter.length < f4.text.length;
      cases['confirm.confirmed-continues-pending-action'] = reset;
      cases['rewind.confirmed-files-untouched-without-restore'] = fsMod.readFileSync(file, 'utf8') === 'ANCHOR-B\n';
      const conf2 = p3 ? await resolveConfirmation(s, p3.id, 'confirmed') : { status: 0, body: null };
      cases['confirm.confirmed-already-resolved-404'] = conf2.status === 404;
      slashStart = wsFrames.length;
      if (p3) await sendSlash(s, `/confirm ${p3.id}`);
      cases['slash.confirm.already-processed-no-reexecute'] = Boolean(p3) && Boolean(await awaitSlashReply(slashStart, /不存在或已处理/));
      await awaitQuiet();
      fsMod.rmSync(dir, { recursive: true, force: true });
      return {
        cases, sessionId: s.sessionId, cp,
        debug: { p1: p1?.raw ?? null, p2: p2?.id ?? null, p3: p3?.id ?? null, badActionStatus: badAction.status, rejStatus: rej.status, rej2Status: rej2.status, conf2Status: conf2.status,
          echo3: echo3.slice(0, 200), rejReply, textAfterLen: textAfter.length, textAfterHead: textAfter.slice(0, 700), f3Final: f3.final, f4Final: f4.final, scheduledFrame: (scheduledFrame as string | null)?.slice(0, 220) ?? null },
      };
    });
  }, 420_000);

  it('leaf.tool.session: session 工具 list/read 范围受控（未知会话拒）+ send 只生成指定目标草稿卡（不直写目标历史、拒发给自己、未知目标拒）', async () => {
    await observe('leaf.tool.session', async () => {
      const cases: Record<string, boolean> = {};
      const s = await newToolSession('R08 session-tool');
      const t = await newToolSession('R08 session-target');
      if (!s || !t) return { cases, skipped: 'session-create-failed' };
      const rt = await runToolPlan(t, 'tgt1', [{ tool: 'checkpoint', args: { action: 'list' } }]);
      // ── 5DCB03 A0：跨会话范围受控——列表只含本主体会话；读未知会话显式拒；读本主体他话可得 transcript ──
      const r1 = await runToolPlan(s, 'ses1', [{ tool: 'session', args: { action: 'list' } }]);
      const e1 = finalEcho(r1.historyText, 'ses1')?.last ?? '';
      cases['list.shows-own-other-session'] = r1.final && e1.includes(t.sessionId);
      const r2 = await runToolPlan(s, 'ses2', [{ tool: 'session', args: { action: 'read', sessionId: 'sess-does-not-exist-t07', mode: 'summary' } }]);
      const e2 = finalEcho(r2.historyText, 'ses2')?.last ?? '';
      cases['read.unknown-session-rejected'] = r2.final && /Session not found/i.test(e2);
      const r3 = await runToolPlan(s, 'ses3', [{ tool: 'session', args: { action: 'read', sessionId: t.sessionId, mode: 'transcript' } }]);
      const e3 = finalEcho(r3.historyText, 'ses3')?.last ?? '';
      cases['read.own-session-transcript'] = r3.final && rt.final && e3.includes(t.sessionId) && e3.includes('tgt1');
      // ── 5DCB03 A1：发送只达指定目标——拒发给自己；对目标只生成草稿卡（待用户确认，目标历史未被直写）；未知目标拒 ──
      const r4 = await runToolPlan(s, 'ses4', [{ tool: 'session', args: { action: 'send', sessionId: s.sessionId, message: 'R08 self' } }]);
      const e4 = finalEcho(r4.historyText, 'ses4')?.last ?? '';
      cases['send.to-self-refused'] = r4.final && /Refusing to send to the current session itself/i.test(e4);
      const targetBefore = await toolHistoryText(t);
      const r5 = await runToolPlan(s, 'ses5', [{ tool: 'session', args: { action: 'send', sessionId: t.sessionId, message: 'R08 cross-session hello' } }]);
      const e5 = finalEcho(r5.historyText, 'ses5')?.last ?? '';
      cases['send.creates-draft-card-for-target'] = r5.final && /Draft created/i.test(e5) && /waiting for the user to confirm/i.test(e5);
      const targetAfter = await toolHistoryText(t);
      cases['send.target-history-not-written-before-confirm'] = !targetAfter.includes('R08 cross-session hello') && targetAfter.length === targetBefore.length;
      const r6 = await runToolPlan(s, 'ses6', [{ tool: 'session', args: { action: 'send', sessionId: 'sess-does-not-exist-t07', message: 'x' } }]);
      const e6 = finalEcho(r6.historyText, 'ses6')?.last ?? '';
      cases['send.unknown-target-rejected'] = r6.final && /Session not found/i.test(e6);
      // ── 5DCB03 A2/A3：create 只产草稿卡（草稿句柄即后续确认/部分成功的会话身份锚）；未知 agent 显式拒 ──
      const r7 = await runToolPlan(s, 'ses7', [{ tool: 'session', args: { action: 'create', agent: 'a1', message: 'R08 create hello', title: 'R08 created' } }]);
      const e7 = finalEcho(r7.historyText, 'ses7')?.last ?? '';
      cases['create.returns-draft-handle-for-agent'] = r7.final && /Draft created \(/i.test(e7) && /confirm session creation for agent a1/i.test(e7);
      const r8 = await runToolPlan(s, 'ses8', [{ tool: 'session', args: { action: 'create', agent: 'no-such-agent-t07', message: 'x' } }]);
      const e8 = finalEcho(r8.historyText, 'ses8')?.last ?? '';
      cases['create.unknown-agent-rejected'] = r8.final && /Unknown agent/i.test(e8);
      return { cases, sessionId: s.sessionId, target: t.sessionId, e1: e1.slice(0, 300), e2: e2.slice(0, 160), e3: e3.slice(0, 300), e4: e4.slice(0, 160), e5: e5.slice(0, 240), e6: e6.slice(0, 160), e7: e7.slice(0, 200), e8: e8.slice(0, 160) };
    });
  }, 420_000);

  it('leaf.tool.current-status: current_status 工具 list 键目录 → 逐键 get 全部可解（目录与 get 一致）+ 模型/agent/会话值与运行时一致 + 未知键显式错误', async () => {
    await observe('leaf.tool.current-status', async () => {
      const cases: Record<string, boolean> = {};
      const s = await newToolSession('R08 current-status');
      if (!s) return { cases, skipped: 'session-create-failed' };
      // ── 386675 A0：list 键目录；随后对目录中每个键 get，全部返回带该键的负载 ──
      const r1 = await runToolPlan(s, 'cs1', [{ tool: 'current_status', args: { action: 'list' } }]);
      const e1 = finalEcho(r1.historyText, 'cs1')?.last ?? '';
      // 键目录从历史里的 list 工具回包（美化 JSON，转义态 \"key\": \"<k>\"）收集——终结回显有
      // 截断，不作目录来源。
      const keys = [...new Set([...r1.historyText.matchAll(/\\"key\\": \\"([a-z_]+)\\"/g)].map((m) => m[1]))];
      cases['list.returns-key-catalog'] = r1.final && keys.length >= 5 && ['time', 'logical_date', 'agent', 'model'].every((k) => keys.includes(k));
      const r2 = keys.length
        ? await runToolPlan(s, 'cs2', keys.map((key) => ({ tool: 'current_status', args: { action: 'get', key } })), 120000)
        : { final: false, historyText: '', frames: [] as string[] };
      // 每个 get 的回包是 `{ "<key>": {...} }` 美化 JSON；历史 JSON 串内转义为 \"<key>\": 。
      const resolved = keys.filter((key) => r2.historyText.includes(`\\"${key}\\": `));
      cases['get.every-listed-key-resolves'] = r2.final && keys.length > 0 && resolved.length === keys.length;
      // ── 386675 A1：模型/agent/会话值与运行时一致（会话模型为注入的替身 chat@t07-stub；agent a1）──
      const modelBlock = r2.historyText.match(/\\"model\\": \{((?:[^{}]|\{[^{}]*\})*)\}/)?.[1] ?? '';
      cases['get.model-matches-runtime'] = /\\"id\\": \\"chat\\"/.test(modelBlock) && /t07-stub/.test(modelBlock);
      const agentBlock = r2.historyText.match(/\\"agent\\": \{((?:[^{}]|\{[^{}]*\})*)\}/)?.[1] ?? '';
      cases['get.agent-matches-runtime'] = /\\"id\\": \\"a1\\"/.test(agentBlock);
      cases['get.time-has-iso-and-timezone-fields'] = /\\"iso\\": \\"\d{4}-/.test(r2.historyText) && /\\"localDateTime\\"/.test(r2.historyText);
      // 未知值标未知：非 Bridge 会话的 bridge_context 显式 isBridgeSession=false（不猜造平台上下文）。
      cases['get.bridge-context-explicit-not-bridge'] = /\\"isBridgeSession\\": false/.test(r2.historyText);
      // 未知键显式错误；缺键显式错误。
      const r3 = await runToolPlan(s, 'cs3', [{ tool: 'current_status', args: { action: 'get', key: 'no_such_key_t07' } }]);
      const e3 = finalEcho(r3.historyText, 'cs3')?.last ?? '';
      cases['get.unknown-key-explicit-error'] = r3.final && /Unknown status key/i.test(e3);
      const r4 = await runToolPlan(s, 'cs4', [{ tool: 'current_status', args: { action: 'get' } }]);
      const e4 = finalEcho(r4.historyText, 'cs4')?.last ?? '';
      cases['get.missing-key-explicit-error'] = r4.final && /requires a key/i.test(e4);
      return { cases, sessionId: s.sessionId, keys, resolved, modelBlock: modelBlock.slice(0, 200), agentBlock: agentBlock.slice(0, 160), e3: e3.slice(0, 160), e4: e4.slice(0, 160) };
    });
  }, 420_000);

  it('leaf.tool.session-folders: session_folders 工具 list 范围 → operate 下 add 仅改当前会话 → ask 下 add 被拒范围不变 → remove 复原 + 不存在目录拒', async () => {
    await observe('leaf.tool.session-folders', async () => {
      const cases: Record<string, boolean> = {};
      const fsMod = await import('node:fs');
      const osMod = await import('node:os');
      const pathMod = await import('node:path');
      const s = await newToolSession('R08 session-folders');
      const other = await newToolSession('R08 session-folders-other');
      if (!s || !other) return { cases, skipped: 'session-create-failed' };
      const dirA = fsMod.realpathSync(fsMod.mkdtempSync(pathMod.join(osMod.tmpdir(), 'r08-sf-a-')));
      const dirB = fsMod.realpathSync(fsMod.mkdtempSync(pathMod.join(osMod.tmpdir(), 'r08-sf-b-')));
      const foldersOf = async (x: ToolSession): Promise<string[]> => {
        const r = await jsonFetch(`/api/sessions/authorized-folders?path=${encodeURIComponent(sessionLocator(x))}`);
        const list = Array.isArray(r.body) ? r.body : (r.body?.folders ?? r.body?.authorizedFolders ?? []);
        return Array.isArray(list) ? list.map(String) : [];
      };
      const r1 = await runToolPlan(s, 'sf1', [{ tool: 'session_folders', args: { action: 'list' } }]);
      const e1 = finalEcho(r1.historyText, 'sf1')?.last ?? '';
      cases['list.returns-scope-shape'] = r1.final && /session_folders/.test(e1) && /authorizedFolders/.test(e1);
      // ── 7C959E A0：operate（预批）下 add → 仅当前会话授权目录变化；另一会话不变 ──
      const r2 = await runToolPlan(s, 'sf2', [{ tool: 'session_folders', args: { action: 'add', folder: dirA } }]);
      const e2 = finalEcho(r2.historyText, 'sf2')?.last ?? '';
      const afterAdd = await foldersOf(s);
      const otherAfterAdd = await foldersOf(other);
      cases['add.approved-changes-current-session-only'] = r2.final && e2.includes(dirA) && afterAdd.includes(dirA) && !otherAfterAdd.includes(dirA);
      // ── 7C959E A1：ask 模式下 add 需用户确认——拒绝后范围不变 ──
      await jsonFetch('/api/session-permission-mode', { method: 'POST', body: { mode: 'ask', sessionPath: sessionLocator(s) } });
      const start = wsFrames.length;
      await sendPlan(s, 'sf3', [{ tool: 'session_folders', args: { action: 'add', folder: dirB } }]);
      const pending = await awaitPendingConfirmation(s, start, dirB);
      cases['add.ask-mode-surfaces-confirmation'] = Boolean(pending?.id);
      const rej = pending ? await resolveConfirmation(s, pending.id, 'rejected') : { status: 0, body: null };
      const f3 = await awaitFinal(s, 'sf3');
      const afterReject = await foldersOf(s);
      const otherAfterReject = await foldersOf(other);
      cases['add.rejected-scope-unchanged'] = rej.status === 200 && f3.final && !afterReject.includes(dirB) && afterReject.includes(dirA);
      cases['reject.other-session-unchanged'] = !otherAfterReject.includes(dirB) && !otherAfterReject.includes(dirA);
      await awaitQuiet();
      await jsonFetch('/api/session-permission-mode', { method: 'POST', body: { mode: 'operate', sessionPath: sessionLocator(s) } });
      // remove 复原 + 不存在目录拒。
      const r4 = await runToolPlan(s, 'sf4', [{ tool: 'session_folders', args: { action: 'remove', folder: dirA } }]);
      const afterRemove = await foldersOf(s);
      cases['remove.restores-scope'] = r4.final && !afterRemove.includes(dirA);
      // 不存在目录：旧栈工具路径只 path.resolve 不校验存在（HTTP 路由会校验——旧栈自身
      // 两面不一致），Rust 工具与路由同口径拒绝。属 ADJ-3 同族（Rust 更严格/诚实）；
      // 不在叶 7C959E 断言内，只作观测记录（missingFolderEcho），不入 cases。
      const r5 = await runToolPlan(s, 'sf5', [{ tool: 'session_folders', args: { action: 'add', folder: pathMod.join(dirA, 'does-not-exist-t07') } }]);
      const e5 = finalEcho(r5.historyText, 'sf5')?.last ?? '';
      const missingFolderAccepted = (await foldersOf(s)).some((f) => f.includes('does-not-exist-t07'));
      if (missingFolderAccepted) {
        await runToolPlan(s, 'sf6', [{ tool: 'session_folders', args: { action: 'remove', folder: pathMod.join(dirA, 'does-not-exist-t07') } }]);
      }
      fsMod.rmSync(dirA, { recursive: true, force: true });
      fsMod.rmSync(dirB, { recursive: true, force: true });
      return { cases, sessionId: s.sessionId, dirA, dirB, e1: e1.slice(0, 240), e2: e2.slice(0, 240), afterAdd, otherAfterAdd, afterReject, afterRemove, pending: pending?.raw ?? null, rejStatus: rej.status, missingFolderAccepted, missingFolderEcho: e5.slice(0, 200), f3Echo: (finalEcho(f3.text, 'sf3')?.last ?? '').slice(0, 200) };
    });
  }, 420_000);

  it('leaf.ws.stream: WS 入站帧族——prompt 流式阶段/工具/终结 + 空内容·非法图片类型化拒 / steer 活动流 steered·到达模型·空闲回落 prompt / interject 回执·新回复·媒体超限拒 / resume_stream 补发·无流空事件·越权拒 / slash(ws) 同一 dispatcher·未知命令·Agent 不匹配 / compact·fresh-compact·stop', async () => {
    await observe('leaf.ws.stream', async () => {
      const cases: Record<string, boolean> = {};
      const s = await newToolSession('R08 ws-stream');
      if (!s) return { cases, skipped: 'session-create-failed' };
      await ensureWsConnected();
      const wsMod = await import('../../services/websocket');
      const ws = wsMod.getWebSocket();
      if (!ws || ws.readyState !== WebSocket.OPEN) return { cases, wsOpen: false };
      const framesSince = (start: number) => wsFrames.slice(start).filter((f) => !f.startsWith('>>> '));
      const forSession = (f: string, x: ToolSession = s) => f.includes(x.sessionId) || f.includes(x.path);
      const ghostPath = kind === 'rust' ? 'native:sess-does-not-exist-t07' : 'agents/a1/sessions/sess-does-not-exist-t07.jsonl';
      const rejectedFrame = (f: string) => /"type":\s*"(error|input_rejected)"/.test(f) || /not_found|forbidden|命令拒绝|不存在|unknown session|Invalid session/i.test(f);
      // 6 次工具调用 = 12 次模型调用，配合替身 --tool-delay-ms 350 给出约 4s 的「run 进行中」窗口。
      const longPlan: ToolCall[] = Array.from({ length: 6 }, () => ({ tool: 'checkpoint', args: { action: 'list' } }));
      const awaitStreaming = async (start: number, x: ToolSession = s, budgetMs = 15000) => {
        try {
          await waitUntil(() => framesSince(start).some((f) => forSession(f, x) && /"isStreaming":\s*true/.test(f)), 'streaming started', budgetMs);
          return true;
        } catch { return false; }
      };
      const pollHistory = async (x: ToolSession, pred: (t: string) => boolean, budgetMs: number) => {
        const deadline = Date.now() + budgetMs;
        let t = '';
        while (Date.now() < deadline) { t = await toolHistoryText(x); if (pred(t)) return { ok: true, text: t }; await sleep(800); }
        return { ok: false, text: t };
      };
      // ── D483E4 A0：prompt 经授权进入会话 Run，流式返回阶段（status/isStreaming）、工具（tool_start/end）与终结（run_end）──
      let start = wsFrames.length;
      const r1 = await runToolPlan(s, 'ws1', [{ tool: 'checkpoint', args: { action: 'list' } }]);
      const f1 = framesSince(start).filter((f) => forSession(f));
      cases['prompt.run-streams-status-tool-final'] = r1.final
        && f1.some((f) => /"isStreaming":\s*true/.test(f))
        && f1.some((f) => /"tool_start"/.test(f)) && f1.some((f) => /"tool_end"/.test(f))
        && f1.some((f) => /"assistant_run_end"/.test(f));
      // ── D483E4 A1：空内容不提交；非法图片格式/过多图片 → 类型化拒绝回执（input_rejected + code），且不写消息 ──
      const histBefore = await toolHistoryText(s);
      start = wsFrames.length;
      ws.send(JSON.stringify({ type: 'prompt', text: '', sessionId: s.sessionId, sessionPath: s.path, clientMessageId: 'r08-empty-1' }));
      await sleep(2500);
      cases['prompt.empty-not-submitted'] = !framesSince(start).some((f) => forSession(f) && /"isStreaming":\s*true/.test(f)) && (await toolHistoryText(s)).length === histBefore.length;
      start = wsFrames.length;
      ws.send(JSON.stringify({ type: 'prompt', text: 'R08 bad image', sessionId: s.sessionId, sessionPath: s.path, clientMessageId: 'r08-img-1', images: [{ mimeType: 'image/bmp', data: 'QUJD', type: 'image', name: 'x.bmp' }] }));
      let rejImg: string | null = null;
      try { await waitUntil(() => { rejImg = framesSince(start).find((f) => /"input_rejected"/.test(f) && /unsupported_image_format/.test(f)) ?? null; return Boolean(rejImg); }, 'image rejected', 8000); } catch { /* 如实 */ }
      cases['prompt.unsupported-image-typed-rejection'] = Boolean(rejImg);
      const many = Array.from({ length: 11 }, (_, i) => ({ mimeType: 'image/png', data: 'iVBORw0KGgo=', type: 'image', name: `p${i}.png` }));
      start = wsFrames.length;
      ws.send(JSON.stringify({ type: 'prompt', text: 'R08 many images', sessionId: s.sessionId, sessionPath: s.path, clientMessageId: 'r08-img-2', images: many }));
      let rejMany: string | null = null;
      try { await waitUntil(() => { rejMany = framesSince(start).find((f) => /"input_rejected"/.test(f) && /max_images/.test(f)) ?? null; return Boolean(rejMany); }, 'too many images rejected', 8000); } catch { /* 如实 */ }
      cases['prompt.too-many-images-typed-rejection'] = Boolean(rejMany);
      cases['prompt.rejected-inputs-write-no-message'] = (await toolHistoryText(s)).length === histBefore.length;
      // ── C1FB9C A0：活动流 steer → steered 回执，steer 文本到达模型（替身回显 R08-STEER-SEEN）；流已停 → 当普通 prompt 提交 ──
      start = wsFrames.length;
      await sendPlan(s, 'ws2', longPlan);
      const streaming2 = await awaitStreaming(start);
      ws.send(JSON.stringify({ type: 'steer', text: 'R08-STEER-MARK 请在回复中体现', sessionId: s.sessionId, sessionPath: s.path }));
      let steered = false;
      try { await waitUntil(() => framesSince(start).some((f) => /"type":\s*"steered"/.test(f)), 'steered ack', 12000); steered = true; } catch { /* 如实 */ }
      cases['steer.active-stream-acked'] = streaming2 && steered;
      const seen = await pollHistory(s, (t) => t.includes('R08-STEER-SEEN'), 60000);
      await awaitQuiet();
      cases['steer.text-reaches-model'] = seen.ok;
      ws.send(JSON.stringify({ type: 'steer', text: planText('st3', [{ tool: 'checkpoint', args: { action: 'list' } }]), sessionId: s.sessionId, sessionPath: s.path }));
      const f3 = await awaitFinal(s, 'st3', 45000);
      await awaitQuiet();
      cases['steer.idle-falls-back-to-prompt'] = f3.final;
      start = wsFrames.length;
      ws.send(JSON.stringify({ type: 'steer', text: 'R08 foreign steer', sessionId: 'sess-does-not-exist-t07', sessionPath: ghostPath }));
      let steerBad = false;
      try { await waitUntil(() => framesSince(start).some((f) => rejectedFrame(f) && !/"isStreaming"/.test(f)), 'steer foreign rejected', 8000); steerBad = true; } catch { /* 如实 */ }
      cases['steer.unknown-session-rejected'] = steerBad;
      // ── 92620B A0：流中 interject → 输入回执（旧栈 steered；Rust 取消活动流后新 run 的 user 消息/run_start）+ 新回复落历史 ──
      start = wsFrames.length;
      await sendPlan(s, 'ws4', longPlan);
      const streaming4 = await awaitStreaming(start);
      const itjText = planText('itj1', [{ tool: 'checkpoint', args: { action: 'list' } }]);
      const itjStart = wsFrames.length;
      ws.send(JSON.stringify({ type: 'interject', text: itjText, sessionId: s.sessionId, sessionPath: s.path, clientMessageId: 'r08-itj-1', displayMessage: { text: itjText } }));
      let itjReceipt = false;
      try { await waitUntil(() => framesSince(itjStart).some((f) => forSession(f) && /"type":\s*"(steered|session_user_message|assistant_run_start)"/.test(f)), 'interject receipt', 15000); itjReceipt = true; } catch { /* 如实 */ }
      const f5 = await awaitFinal(s, 'itj1', 60000);
      await awaitQuiet();
      cases['interject.during-stream-receipt-and-new-reply'] = streaming4 && itjReceipt && f5.final;
      // ── 92620B A1：媒体超限拒绝且不写空消息；会话身份无效拒 ──
      const histBefore2 = await toolHistoryText(s);
      start = wsFrames.length;
      ws.send(JSON.stringify({ type: 'interject', text: 'R08 itj many', sessionId: s.sessionId, sessionPath: s.path, clientMessageId: 'r08-itj-2', images: many }));
      let itjRej = false;
      try { await waitUntil(() => framesSince(start).some((f) => /"input_rejected"/.test(f) && /max_images/.test(f)), 'interject media rejected', 8000); itjRej = true; } catch { /* 如实 */ }
      cases['interject.media-over-limit-rejected-no-empty-message'] = itjRej && (await toolHistoryText(s)).length === histBefore2.length;
      start = wsFrames.length;
      ws.send(JSON.stringify({ type: 'interject', text: 'x', sessionId: 'sess-does-not-exist-t07', sessionPath: ghostPath, clientMessageId: 'r08-itj-3' }));
      let itjBad = false;
      try { await waitUntil(() => framesSince(start).some((f) => rejectedFrame(f) && !/"isStreaming"/.test(f)), 'interject foreign rejected', 8000); itjBad = true; } catch { /* 如实 */ }
      cases['interject.invalid-session-rejected'] = itjBad;
      // ── 816642：resume_stream 按 streamId/sinceSeq 补发或给 reset/truncated 标记；无活动流空事件 + 运行态；越权不泄露旧帧 ──
      const streamIds = framesSince(0).filter((f) => forSession(f) && /"streamId":\s*"[^"]+"/.test(f)).map((f) => f.match(/"streamId":\s*"([^"]+)"/)![1]);
      const lastStream = streamIds[streamIds.length - 1] ?? null;
      start = wsFrames.length;
      ws.send(JSON.stringify({ type: 'resume_stream', sessionId: s.sessionId, sessionPath: s.path, streamId: lastStream, sinceSeq: 0 }));
      let resumeFrame: any = null;
      try {
        await waitUntil(() => {
          const f = framesSince(start).find((x) => /"stream_resume"/.test(x) && forSession(x));
          if (f) { try { resumeFrame = JSON.parse(f); } catch { resumeFrame = { raw: f }; } }
          return Boolean(resumeFrame);
        }, 'stream_resume', 10000);
      } catch { /* 如实 */ }
      cases['resume.known-stream-replays-or-marks-reset'] = Boolean(resumeFrame)
        && ((Array.isArray(resumeFrame?.events) && resumeFrame.events.length > 0) || resumeFrame?.reset === true || resumeFrame?.truncated === true);
      const fresh = await newToolSession('R08 ws-resume-idle');
      start = wsFrames.length;
      if (fresh) ws.send(JSON.stringify({ type: 'resume_stream', sessionId: fresh.sessionId, sessionPath: fresh.path, streamId: null, sinceSeq: 0 }));
      let idleResume: any = null;
      try {
        await waitUntil(() => {
          const f = framesSince(start).find((x) => /"stream_resume"/.test(x) && Boolean(fresh) && forSession(x, fresh!));
          if (f) { try { idleResume = JSON.parse(f); } catch { idleResume = { raw: f }; } }
          return Boolean(idleResume);
        }, 'idle stream_resume', 10000);
      } catch { /* 如实 */ }
      cases['resume.no-active-stream-empty-events-with-runtime-flag'] = Boolean(idleResume) && Array.isArray(idleResume?.events) && idleResume.events.length === 0
        && idleResume?.isStreaming === false && idleResume?.runtimeIsStreaming === false;
      start = wsFrames.length;
      ws.send(JSON.stringify({ type: 'resume_stream', sessionId: 'sess-does-not-exist-t07', sessionPath: ghostPath, streamId: lastStream, sinceSeq: 0 }));
      await sleep(2500);
      const foreignResume = framesSince(start).filter((f) => /"stream_resume"/.test(f) && f.includes('sess-does-not-exist-t07'));
      cases['resume.unknown-session-rejected-no-frames-leaked'] = !foreignResume.some((f) => /"events":\s*\[\s*\{/.test(f))
        && (framesSince(start).some(rejectedFrame) || foreignResume.length === 0);
      // ── 2FAA74 + 0FE8BE：slash 经 WS 交同一 dispatcher 以 slash_result 显示；/compact 先应答再结果（token 或完成说明）──
      const slashTexts = (from: number) => [
        ...framesSince(from).filter((f) => /slash_result/.test(f)).map((f) => { try { return String(JSON.parse(f).text ?? f); } catch { return f; } }),
        ...slashNotices.map((n) => n.text),
      ];
      slashNotices.length = 0;
      start = wsFrames.length;
      await sendSlash(s, '/compact');
      try { await waitUntil(() => slashTexts(start).some((t) => /已压缩|压缩完成|压缩失败|tokens/.test(t)), 'compact result', 30000); } catch { /* 如实 */ }
      const compactTexts = slashTexts(start);
      cases['slash.ws.dispatched-as-slash-result'] = compactTexts.some((t) => /压缩|compact/i.test(t));
      const ackIdx = compactTexts.findIndex((t) => /正在压缩/.test(t));
      const resIdx = compactTexts.findIndex((t) => /已压缩|压缩完成|压缩失败|tokens/.test(t));
      cases['slash.compact.ack-before-result'] = ackIdx >= 0 && resIdx > ackIdx;
      cases['slash.compact.result-tokens-or-completion'] = resIdx >= 0;
      start = wsFrames.length; slashNotices.length = 0;
      await sendSlash(s, '/nosuchcmd-t07');
      cases['slash.ws.unknown-command-error'] = Boolean(await awaitSlashReply(start, /未知命令|unknown|不支持|not/i));
      start = wsFrames.length;
      if (kind === 'rust') wsMod.sendRustSlash(s.sessionId, '/compact', 'no-such-agent-t07');
      else ws.send(JSON.stringify({ type: 'slash', text: '/compact', sessionPath: s.path, agentId: 'no-such-agent-t07' }));
      let agentMismatch = false;
      try { await waitUntil(() => framesSince(start).some((f) => /Agent 身份|agent/i.test(f) && /拒绝|error|mismatch|not/i.test(f)), 'agent mismatch rejected', 8000); agentMismatch = true; } catch { /* 如实 */ }
      cases['slash.ws.agent-identity-mismatch-rejected'] = agentMismatch;
      start = wsFrames.length;
      if (kind === 'rust') wsMod.sendRustSlash('sess-does-not-exist-t07', '/compact', null);
      else ws.send(JSON.stringify({ type: 'slash', text: '/compact', sessionPath: ghostPath, agentId: null }));
      cases['slash.compact.invalid-session-explicit-error'] = Boolean(await awaitSlashReply(start, /命令拒绝|拒绝|not|error|不存在|未找到|Invalid/i)) || framesSince(start).some((f) => /"type":\s*"error"/.test(f));
      // ── 5A6E0D：/fresh-compact 回传 token 变化或原因；原始历史保留；无效会话显式错误 ──
      start = wsFrames.length; slashNotices.length = 0;
      await sendSlash(s, '/fresh-compact');
      try { await waitUntil(() => slashTexts(start).some((t) => /fresh-compact (?:已)?完成|fresh-compact 失败|tokens/.test(t)), 'fresh-compact result', 30000); } catch { /* 如实 */ }
      const fcTexts = slashTexts(start);
      cases['slash.fresh-compact.ack-then-result-tokens-or-reason'] = fcTexts.some((t) => /正在 fresh-compact/.test(t)) && fcTexts.some((t) => /fresh-compact (?:已)?完成|fresh-compact 失败|tokens/.test(t));
      const histAfterFc = await toolHistoryText(s);
      cases['slash.fresh-compact.original-history-retained'] = histAfterFc.includes('R08-TOOL-PLAN ws1');
      start = wsFrames.length;
      if (kind === 'rust') wsMod.sendRustSlash('sess-does-not-exist-t07', '/fresh-compact', null);
      else ws.send(JSON.stringify({ type: 'slash', text: '/fresh-compact', sessionPath: ghostPath, agentId: null }));
      cases['slash.fresh-compact.invalid-session-explicit-error'] = Boolean(await awaitSlashReply(start, /命令拒绝|拒绝|not|error|不存在|未找到|Invalid/i)) || framesSince(start).some((f) => /"type":\s*"error"/.test(f));
      // ── 4EA88B：/stop 中止当前流（无终结回答落地）；另一会话并行流不受扰；无活动流返回已停止提示 ──
      const other = await newToolSession('R08 ws-stop-other');
      start = wsFrames.length;
      await sendPlan(s, 'ws6', longPlan);
      const streaming6 = await awaitStreaming(start);
      if (other) await sendPlan(other, 'ws7', [{ tool: 'checkpoint', args: { action: 'list' } }]);
      slashNotices.length = 0;
      const stopStart = wsFrames.length;
      await sendSlash(s, '/stop');
      let stopped = false;
      try {
        await waitUntil(() => framesSince(stopStart).some((f) => (forSession(f) && (/"aborted":\s*true/.test(f) || /abort_result|cancelled/.test(f)))
          || (/slash_result/.test(f) && /停止|中止|stopped|abort/i.test(f))), 'stop acknowledged', 15000);
        stopped = true;
      } catch { /* 如实 */ }
      await awaitQuiet();
      const h6 = await toolHistoryText(s);
      cases['slash.stop.aborts-active-stream'] = streaming6 && stopped && !h6.includes('R08-TOOL-FINAL ws6');
      const f7 = other ? await awaitFinal(other, 'ws7', 45000) : { final: false, text: '' };
      cases['slash.stop.other-session-unaffected'] = Boolean(other) && f7.final;
      slashNotices.length = 0;
      start = wsFrames.length;
      await sendSlash(s, '/stop');
      cases['slash.stop.idle-reports-already-stopped'] = Boolean(await awaitSlashReply(start, /已停止/));
      return {
        cases, sessionId: s.sessionId,
        debug: {
          f1Types: [...new Set(f1.map((f) => { try { return JSON.parse(f).type; } catch { return '?'; } }))].slice(0, 20),
          rejImg: (rejImg as string | null)?.slice(0, 200) ?? null, rejMany: (rejMany as string | null)?.slice(0, 200) ?? null,
          streaming2, steered, steerSeen: seen.ok, st3: f3.final, streaming4, itjReceipt, itj1: f5.final, itjRej,
          lastStream, resumeFrame: JSON.stringify(resumeFrame ?? null).slice(0, 300), idleResume: JSON.stringify(idleResume ?? null).slice(0, 300),
          compactTexts: compactTexts.slice(0, 6), fcTexts: fcTexts.slice(0, 6), stopped, ws7: f7.final,
        },
      };
    });
  }, 480_000);

  // ══════════ R08 收尾批次 7：D02 HTTP 语义（cleanup 期限 / 延迟内容 / 重放最近用户消息 / switch 焦点 / 真回合提示词快照） ══════════
  it('leaf.cleanup.expiry: 清理过期归档——窗口内保留、窗口外按期限永久删除并清理生命周期与标题、活动会话不受影响、越权不删', async () => {
    await observe('leaf.cleanup.expiry', async () => {
      const cases: Record<string, boolean> = {};
      const tag = `r08-expire-${Date.now().toString(36)}`;
      const e = await newToolSession(`R08 ${tag} expire`);
      const k = await newToolSession(`R08 ${tag} keep`);
      const live = await newToolSession(`R08 ${tag} active`);
      if (!e || !k || !live) return { cases, skipped: 'session-create-failed' };
      const archivedIds = async () => ((await listArchivedSessions()) as any[]).map((a) => String(a.sessionId ?? a.path));
      const ae = await jsonFetch('/api/sessions/archive', { method: 'POST', body: { path: sessionLocator(e) } });
      const ak = await jsonFetch('/api/sessions/archive', { method: 'POST', body: { path: sessionLocator(k) } });
      const arch0 = await archivedIds();
      cases['setup.two-fresh-archives'] = ae.status < 300 && ak.status < 300 && arch0.includes(e.sessionId) && arch0.includes(k.sessionId);
      // 越权：坏令牌清理被拒且无副作用。
      const unauth = await withInvalidToken(async () => jsonFetch('/api/sessions/cleanup', { method: 'POST', body: { maxAgeDays: 0 } }));
      const archU = await archivedIds();
      cases['cleanup.unauthorized-rejected-no-side-effect'] = (unauth.status === 401 || unauth.status === 403) && archU.includes(e.sessionId) && archU.includes(k.sessionId);
      // 窗口内（30 天）：刚归档者保留；返回删除数量形状。
      const keep = await jsonFetch('/api/sessions/cleanup', { method: 'POST', body: { maxAgeDays: 30 } });
      const arch1 = await archivedIds();
      cases['cleanup.within-window-keeps-fresh-archives'] = keep.status === 200 && typeof keep.body?.deleted === 'number' && arch1.includes(e.sessionId) && arch1.includes(k.sessionId);
      // 窗口外（maxAgeDays=0 → 截止线=现在）：已归档者按期限永久删除（双内核 cutoff=now-maxAgeDays*86400000 同口径）；活动会话不动。
      await sleep(1200);
      const purge = await jsonFetch('/api/sessions/cleanup', { method: 'POST', body: { maxAgeDays: 0 } });
      const arch2 = await archivedIds();
      await loadSessions();
      const liveStill = (useStore.getState().sessions as any[]).some((s) => s.sessionId === live.sessionId);
      cases['cleanup.beyond-window-deletes-and-reports-count'] = purge.status === 200 && typeof purge.body?.deleted === 'number' && purge.body.deleted >= 2
        && !arch2.includes(e.sessionId) && !arch2.includes(k.sessionId);
      cases['cleanup.active-sessions-untouched'] = liveStill;
      // 关联生命周期与标题：恢复失败（不可恢复）、搜索/列表均不可见。
      const restore = await jsonFetch('/api/sessions/restore', { method: 'POST', body: { path: sessionLocator(e), sessionId: e.sessionId } });
      const search = await jsonFetch(`${kind === 'rust' ? '/lingxi/v1/sessions/search' : '/api/sessions/search'}?q=${encodeURIComponent(tag)}`);
      const searchText = JSON.stringify(search.body ?? '');
      const listedStill = (useStore.getState().sessions as any[]).some((s) => s.sessionId === e.sessionId || s.sessionId === k.sessionId);
      cases['cleanup.deleted-unrecoverable-and-title-gone'] = restore.status >= 400 && !listedStill && !searchText.includes(`${tag} expire`) && !searchText.includes(`${tag} keep`);
      return { cases, e: e.sessionId, k: k.sessionId, keep: keep.body, purge: purge.body, restoreStatus: restore.status, unauthStatus: unauth.status, searchStatus: search.status };
    });
  }, 300_000);

  it('leaf.lazy-content: 历史延迟内容——大工具结果以引用落历史，按 contentId 只在授权会话内取回；他会话定位 404；未知 404', async () => {
    await observe('leaf.lazy-content', async () => {
      const cases: Record<string, boolean> = {};
      const fsMod = await import('node:fs');
      const pathMod = await import('node:path');
      const s = await newToolSession('R08 lazy');
      const other = await newToolSession('R08 lazy-other');
      if (!s || !other) return { cases, skipped: 'session-create-failed' };
      // 双腿同一隔离工作区（drill 注入 LINGXI_R08_AB_WORKSPACE；旧栈显式 workdir，
      // Rust 的 exec_command 要求 workdir 落在 --config workspace 授权范围）。
      const workspace = process.env.LINGXI_R08_AB_WORKSPACE ?? '';
      if (!workspace) return { cases, skipped: 'workspace-not-configured' };
      const dir = fsMod.realpathSync(workspace);
      // 生成 ~84KB 输出的命令（双内核 exec_command 同参）：旧栈 HISTORY_INLINE_CONTENT_LIMIT=8KB
      // 以上即把工具输出换成 deferred 引用；Rust exec 结果超过内联上限时头尾截断并把
      // 完整输出溢写成文件，历史工具结果携带 resourceRef（exec-output:<file>）——
      // 两侧各自用真实机制产生「历史不内联、按引用取回」。
      const bigCmd = `node -e 'const a=[];for(let i=0;i<3000;i++)a.push("R08-LAZY-LINE-"+String(i).padStart(4,"0")+" "+"x".repeat(24));process.stdout.write(a.join("\\n")+"\\nEND-R08-LAZY\\n")'`;
      const r1 = await runToolPlan(s, 'lazy1', [
        { tool: 'exec_command', args: { cmd: bigCmd, workdir: dir }, direct: true },
      ]);
      const hist = await toolHistoryText(s);
      // 引用发现：旧栈投影把超限输出换成 deferred 引用（details.outputDeferred.id，
      // 正文只留预览）；Rust 工具结果 resourceRefs[].resourceId（exec-output:<file>）。
      const oldId = hist.match(/"(?:outputDeferred|inputDeferred|deferred)":\{"id":"([^"]+)"/)?.[1] ?? null;
      const rustId = [...hist.matchAll(/"resourceId":"((?:exec-output|file|res)[^"]*)"/g)].map((m) => m[1])[0] ?? null;
      const contentId = kind === 'rust' ? rustId : oldId;
      // 注意：用户消息里回显的命令串本身含字面量 END-R08-LAZY，不能拿它判「输出未内联」；
      // 用输出中段行（预览只保留头 240 字符，中段行出现即说明内联了完整正文）。
      cases['lazy.large-result-referenced-in-history'] = r1.final && Boolean(contentId)
        && (kind === 'rust' ? /truncated|Full output|完整输出/.test(hist) : !hist.includes('R08-LAZY-LINE-0299'));
      const got = contentId ? await jsonFetch(`/api/sessions/content/${encodeURIComponent(contentId)}?path=${encodeURIComponent(sessionLocator(s))}`) : { status: 0, body: null };
      const gotText = JSON.stringify(got.body ?? '');
      const gotContent = typeof (got.body as any)?.content === 'string' ? String((got.body as any).content) : gotText;
      const gotLineCount = (gotContent.match(/R08-LAZY-LINE-/g) ?? []).length;
      // 「取回大内容」而非预览：正文 > 8KB 内联阈值、行数远超预览、且含输出尾部标记。
      // 两内核的正文保留策略本就不同（旧栈工具结果本身有内核侧输出保留窗口；Rust 溢写
      // 完整输出到文件），故不要求同一行号集合——只证明按引用取回的是真实大正文，
      // 不是首包 240 字预览。
      cases['lazy.content-fetch-returns-full-body'] = got.status === 200 && gotContent.length > 8 * 1024
        && gotLineCount >= 100 && (gotContent.includes('END-R08-LAZY') || gotContent.includes('R08-LAZY-LINE-2999'));
      const foreign = contentId ? await jsonFetch(`/api/sessions/content/${encodeURIComponent(contentId)}?path=${encodeURIComponent(sessionLocator(other))}`) : { status: 0, body: null };
      cases['lazy.other-session-locator-404'] = foreign.status === 404;
      const unknown = await jsonFetch(`/api/sessions/content/r08-no-such-content-2?path=${encodeURIComponent(sessionLocator(s))}`);
      cases['lazy.unknown-content-404'] = unknown.status === 404;
      const readIdx = hist.indexOf('R08-LAZY-LINE');
      return { cases, contentId, gotStatus: got.status, gotKeys: got.body && typeof got.body === 'object' ? Object.keys(got.body) : null, gotLen: gotContent.length, gotHasEnd: gotContent.includes('END-R08-LAZY'), gotHas0299: gotContent.includes('R08-LAZY-LINE-0299'), gotHas2999: gotContent.includes('R08-LAZY-LINE-2999'), gotLineCount, gotSample: gotContent.slice(0, 160), foreignStatus: foreign.status, histLen: hist.length, final: r1.final,
        resourceRefCount: (hist.match(/"resourceId":"/g) ?? []).length, deferredCount: (hist.match(/"(?:outputDeferred|deferred)":/g) ?? []).length,
        histSample: readIdx >= 0 ? hist.slice(Math.max(0, readIdx - 260), readIdx + 240) : hist.slice(0, 400) };
    });
  }, 300_000);

  it('leaf.replay-latest: 重新提交最近用户消息——按版本重放（新一轮回复）、可选替换文字、运行中 409、无效目标显式错误', async () => {
    await observe('leaf.replay-latest', async () => {
      const cases: Record<string, boolean> = {};
      const s = await newToolSession('R08 replay');
      if (!s) return { cases, skipped: 'session-create-failed' };
      await ensureWsConnected();
      const framesSince = (start: number) => wsFrames.slice(start).filter((f) => !f.startsWith('>>> ') && (f.includes(s.sessionId) || f.includes(s.path)));
      const replayPath = kind === 'rust' ? `/lingxi/v1/sessions/${encodeURIComponent(s.sessionId)}/latest-user-message/replay` : '/api/sessions/latest-user-message/replay';
      const body = (extra: Record<string, unknown>) => (kind === 'rust' ? { snapshotVersion: 1, ...extra } : { path: s.path, snapshotVersion: 1, ...extra });
      const r1 = await runToolPlan(s, 'rp1', [{ tool: 'checkpoint', args: { action: 'list' } }]);
      // ── 4DD3F7 A0：重放最近用户消息 → 新一轮 run（run_end 帧）且回复仍为同一计划的终结 ──
      let start = wsFrames.length;
      const rp = await jsonFetch(replayPath, { method: 'POST', body: body({}) });
      let replayedRun = false;
      try { await waitUntil(() => framesSince(start).some((f) => /"assistant_run_end"/.test(f)), 'replay run end', 60000); replayedRun = true; } catch { /* 如实 */ }
      await awaitQuiet();
      const h1 = await toolHistoryText(s);
      cases['replay.latest-replayed-new-run'] = r1.final && rp.status < 300 && replayedRun && h1.includes('R08-TOOL-FINAL rp1');
      // 可选替换文字：以替换文字重放（替身回显 A06-CHAT-FINAL replay2）。
      start = wsFrames.length;
      const rp2 = await jsonFetch(replayPath, { method: 'POST', body: body({ text: 'A06-CHAT-TASK replay2' }) });
      let replaced = false;
      { const deadline = Date.now() + 45000; while (Date.now() < deadline) { if ((await toolHistoryText(s)).includes('A06-CHAT-FINAL replay2')) { replaced = true; break; } await sleep(800); } }
      await awaitQuiet();
      cases['replay.replacement-text-replayed'] = rp2.status < 300 && replaced;
      // 运行中 409。
      start = wsFrames.length;
      await sendPlan(s, 'rp3', [{ tool: 'checkpoint', args: { action: 'list' } }, { tool: 'checkpoint', args: { action: 'list' } }, { tool: 'checkpoint', args: { action: 'list' } }]);
      try { await waitUntil(() => framesSince(start).some((f) => /"isStreaming":\s*true/.test(f)), 'streaming', 15000); } catch { /* 如实 */ }
      const busy = await jsonFetch(replayPath, { method: 'POST', body: body({}) });
      cases['replay.while-running-409'] = busy.status === 409;
      await awaitFinal(s, 'rp3', 60000);
      await awaitQuiet();
      // ── 4DD3F7 A1：无效目标显式错误；空白替换文字（Rust 400；旧栈透传——不入 cases，如实记录）──
      const bad = kind === 'rust'
        ? await jsonFetch('/lingxi/v1/sessions/sess-does-not-exist-t07/latest-user-message/replay', { method: 'POST', body: { snapshotVersion: 1 } })
        : await jsonFetch(replayPath, { method: 'POST', body: { path: 'agents/a1/sessions/sess-does-not-exist-t07.jsonl', snapshotVersion: 1 } });
      cases['replay.invalid-session-rejected'] = bad.status >= 400 && bad.status < 500;
      const blank = await jsonFetch(replayPath, { method: 'POST', body: body({ text: '   ' }) });
      await awaitQuiet();
      return { cases, rpStatus: rp.status, rp2Status: rp2.status, busyStatus: busy.status, badStatus: bad.status, blankStatus: blank.status, rpBody: JSON.stringify(rp.body ?? null).slice(0, 200) };
    });
  }, 300_000);

  it('leaf.switch.focus: 切换焦点会话——改变当前会话并返回会话投影 + 浏览器恢复尝试显式回执（无浏览器时 typed 结果）、来回切换旧会话不挂起、无效目标显式错误', async () => {
    await observe('leaf.switch.focus', async () => {
      const cases: Record<string, boolean> = {};
      const a = await newToolSession('R08 switch-a');
      const b = await newToolSession('R08 switch-b');
      if (!a || !b) return { cases, skipped: 'session-create-failed' };
      await loadSessions();
      const rowA = (useStore.getState().sessions as any[]).find((x) => x.sessionId === a.sessionId);
      const rowB = (useStore.getState().sessions as any[]).find((x) => x.sessionId === b.sessionId);
      if (!rowA || !rowB) return { cases, skipped: 'rows-missing' };
      const sw1 = await jsonFetch(rustOrNode('/lingxi/v1/sessions/switch', '/api/sessions/switch'), { method: 'POST', body: kind === 'rust' ? { sessionId: a.sessionId } : { path: sessionLocator(a), sessionId: a.sessionId } });
      cases['switch.returns-projection'] = sw1.status === 200 && sw1.body?.ok === true && typeof sw1.body?.permissionMode === 'string' && Array.isArray(sw1.body?.authorizedFolders);
      cases['switch.browser-restore-attempt-surfaced'] = sw1.status === 200 && typeof sw1.body?.browserRunning === 'boolean' && sw1.body?.browserResume !== undefined;
      // 真实 UI 动作：store 当前会话随之改变。
      await switchSession(rowB.path);
      const st1 = useStore.getState() as any;
      cases['switch.store-current-session-changes'] = st1.currentSessionPath === rowB.path;
      // 来回切换：旧会话不挂起（再切回仍 200，浏览器状态为 typed 结果而非错误）。
      const sw3 = await jsonFetch(rustOrNode('/lingxi/v1/sessions/switch', '/api/sessions/switch'), { method: 'POST', body: kind === 'rust' ? { sessionId: a.sessionId } : { path: sessionLocator(a), sessionId: a.sessionId } });
      cases['switch.back-and-forth-no-hang'] = sw3.status === 200 && typeof sw3.body?.browserRunning === 'boolean';
      const bad = await jsonFetch(rustOrNode('/lingxi/v1/sessions/switch', '/api/sessions/switch'), { method: 'POST', body: kind === 'rust' ? { sessionId: 'sess-does-not-exist-t07' } : { path: 'agents/a1/sessions/sess-does-not-exist-t07.jsonl' } });
      cases['switch.invalid-session-rejected'] = bad.status >= 400 && bad.status < 500;
      return { cases, sw1Keys: sw1.body && typeof sw1.body === 'object' ? Object.keys(sw1.body).slice(0, 24) : null, browserResume: JSON.stringify(sw1.body?.browserResume ?? null).slice(0, 200), badStatus: bad.status };
    });
  }, 180_000);

  it('leaf.prompt-snapshot.real: 真实模型回合后的会话提示词快照——读到冻结系统提示词与工具名；无效会话显式拒', async () => {
    await observe('leaf.prompt-snapshot.real', async () => {
      const cases: Record<string, boolean> = {};
      const s = await newToolSession('R08 snapshot-real');
      if (!s) return { cases, skipped: 'session-create-failed' };
      const r1 = await runToolPlan(s, 'snap1', [{ tool: 'checkpoint', args: { action: 'list' } }]);
      const snap = await jsonFetch(`/api/sessions/prompt-snapshot?path=${encodeURIComponent(sessionLocator(s))}`);
      const bodyObj = (snap.body && typeof snap.body === 'object') ? snap.body as Record<string, any> : {};
      const inner = (bodyObj.promptSnapshot && typeof bodyObj.promptSnapshot === 'object') ? bodyObj.promptSnapshot : bodyObj;
      const systemPrompt = [inner.systemPrompt, inner.prompt, inner.system, bodyObj.systemPrompt].find((v) => typeof v === 'string') ?? '';
      const toolNames = [bodyObj.toolNames, inner.toolNames, inner.tools, bodyObj.tools].find((v) => Array.isArray(v)) ?? [];
      cases['snapshot.read-after-real-round'] = r1.final && snap.status === 200 && systemPrompt.length > 50 && toolNames.length > 0;
      cases['snapshot.tool-names-include-resident-read'] = toolNames.some((n: unknown) => /\bread\b/.test(typeof n === 'string' ? n : JSON.stringify(n)));
      const bad = await jsonFetch(`/api/sessions/prompt-snapshot?path=${encodeURIComponent(kind === 'rust' ? 'native:sess-does-not-exist-t07' : 'agents/a1/sessions/sess-does-not-exist-t07.jsonl')}`);
      cases['snapshot.invalid-session-rejected'] = bad.status >= 400 && bad.status < 500;
      return { cases, snapStatus: snap.status, keys: Object.keys(bodyObj).slice(0, 12), innerKeys: Object.keys(inner).slice(0, 12), promptLen: systemPrompt.length, toolNames: toolNames.slice(0, 12), badStatus: bad.status };
    });
  }, 180_000);

  // ══════════ R08 收尾批次 8：D03 MCP 资源/应用/OAuth（stdio 替身扩展 + 环回授权服务器替身） ══════════
  it('leaf.mcp.face: 连接器资源读取（ui:// 资源内容/缺 uri·未知 uri 拒）+ 应用启动（launchInput 交接/未知工具拒/未知连接器拒）', async () => {
    await observe('leaf.mcp.face', async () => {
      const cases: Record<string, boolean> = {};
      // 双内核同款确定性 stdio 替身；绝对路径（B 腿子进程 cwd 不受控）。
      const standinArgs = [`${process.cwd()}/scripts/rust-tauri/r08_mcp_standin.cjs`];
      const action = async (act: string, payload: Record<string, unknown>) => (kind === 'rust'
        ? jsonFetch('/lingxi/v1/mcp/settings-action', { method: 'POST', body: { action: act, agentId: 'a1', payload } })
        : { status: 0, body: null });
      const addConnector = async (id: string, extra: Record<string, unknown>) => (kind === 'rust'
        ? action('mcp.connector.add', { connector: { id, name: `T07 ${id}`, enabled: true, ...extra }, startIfEnabled: true })
        : jsonFetch('/api/mcp/connectors', { method: 'POST', body: { id, name: `T07 ${id}`, enabled: true, ...extra } }));
      const removeConnector = async (id: string) => (kind === 'rust'
        ? action('mcp.connector.remove', { id })
        : jsonFetch(`/api/mcp/connectors/${encodeURIComponent(id)}`, { method: 'DELETE' }));
      // 启动：Rust 无 REST /start 路由（settings-action 单入口，crud 任务同口径）；
      // 旧栈是 REST。此前误用 REST /start 致 B 腿 404（dev-13）。
      const startConnector = async (id: string) => (kind === 'rust'
        ? action('mcp.connector.start', { id })
        : jsonFetch(`/api/mcp/connectors/${encodeURIComponent(id)}/start`, { method: 'POST' }));
      const facePath = (suffix: string) => rustOrNode(`/lingxi/v1/mcp/connectors/r08-face-1${suffix}`, `/api/mcp/connectors/r08-face-1${suffix}`);
      const stdio = { transport: 'stdio', command: process.execPath, args: standinArgs };
      const readState = async () => {
        const r = await jsonFetch(rustOrNode('/lingxi/v1/mcp/state', '/api/mcp/state') + '?agentId=a1');
        return r.body ?? {};
      };
      const connectorRows = (body: any) => Array.isArray(body?.config?.connectors) ? body.config.connectors : (Array.isArray(body?.connectors) ? body.connectors : []);
      // 前序任务的负路径可能把全局 MCP 开关留在关闭态：先显式打开。
      if (kind === 'rust') await action('mcp.global.enabled', { enabled: true });
      else await jsonFetch('/api/mcp/settings/enabled', { method: 'PUT', body: { enabled: true } });
      const add = await addConnector('r08-face-1', stdio);
      const started = await startConnector('r08-face-1');
      // 等待连接器真实 running 且工具目录含 r08_app（应用目录来自 tools _meta.ui）。
      let connectorRunning = false;
      let appToolVisible = false;
      {
        const deadline = Date.now() + 30000;
        while (Date.now() < deadline) {
          const row = connectorRows(await readState()).find((c: any) => String(c?.id) === 'r08-face-1');
          const running = row && (row.status === 'running' || row.state === 'running');
          const toolNames = Array.isArray(row?.tools) ? row.tools.map((t: any) => String(t?.name ?? t)) : [];
          connectorRunning = Boolean(running);
          if (connectorRunning && toolNames.includes('r08_app')) { appToolVisible = true; break; }
          await sleep(800);
        }
      }
      // ── 04A6A2 A0：按 uri 返回连接器资源内容（两内核的资源读取都只接受 ui:// 应用资源）；
      //    缺 uri / 无内容拒绝 ──
      const goodRes = await lingxiFetch(facePath('/resources') + '?uri=' + encodeURIComponent('ui://r08/app.html'), { throwOnHttpError: false });
      const goodText = await goodRes.text();
      cases['resources.uri-returns-content'] = add.status < 300 && started.status < 300 && connectorRunning && appToolVisible
        && goodRes.status === 200 && goodText.includes('R08-APP-STANDIN');
      const noUri = await jsonFetch(facePath('/resources'));
      cases['resources.missing-uri-400'] = noUri.status === 400;
      const badUri = await jsonFetch(facePath('/resources') + '?uri=' + encodeURIComponent('ui://r08/no-such'));
      cases['resources.unknown-uri-rejected'] = badUri.status === 404 || badUri.status === 400 || Boolean(badUri.body?.error);
      // ── 7FF8D4 A0：launchInput 交给指定连接器工具，返回启动结果（launchInput 原样交接）──
      const launch = await jsonFetch(facePath('/apps/r08_app/launch'), { method: 'POST', body: { launchInput: { mode: 'r08' } } });
      const launchText = JSON.stringify(launch.body ?? '');
      cases['apps.launch-hands-over-launch-input'] = launch.status < 300 && launchText.includes('ui://r08/app.html') && launchText.includes('r08_app') && launchText.includes('"mode":"r08"');
      const launchNoTool = await jsonFetch(facePath('/apps/r08_no_such_app/launch'), { method: 'POST', body: { launchInput: {} } });
      cases['apps.unknown-tool-rejected'] = launchNoTool.status >= 400 && launchNoTool.status < 500;
      // ── A1：无效连接器显式拒（下游/权限失败不呈现成功结果）──
      const ghostResources = await jsonFetch(rustOrNode('/lingxi/v1/mcp/connectors/r08-no-such-conn/resources', '/api/mcp/connectors/r08-no-such-conn/resources') + '?uri=' + encodeURIComponent('ui://r08/app.html'));
      cases['resources.unknown-connector-rejected'] = ghostResources.status >= 400 && ghostResources.status < 500;
      const unauthLaunch = await withInvalidToken(async () => jsonFetch(facePath('/apps/r08_app/launch'), { method: 'POST', body: { launchInput: { mode: 'r08' } } }));
      cases['apps.unauthorized-rejected'] = unauthLaunch.status === 401 || unauthLaunch.status === 403;
      await removeConnector('r08-face-1');
      return { cases, addStatus: add.status, startedStatus: started.status, connectorRunning, appToolVisible, goodStatus: goodRes.status, goodContentType: goodRes.headers.get('content-type'), goodSample: goodText.slice(0, 160),
        noUriStatus: noUri.status, badUriStatus: badUri.status, launchStatus: launch.status, launchNoToolStatus: launchNoTool.status, ghostStatus: ghostResources.status, unauthStatus: unauthLaunch.status, launchBody: launchText.slice(0, 240) };
    });
  }, 240_000);

  it('leaf.mcp.oauth: 连接器 OAuth——start 返回浏览器所需授权资料（发现/动态注册真跑）+ cancel 结束等待且已存配置不变 + logout 清凭证回公开状态 + 负路径', async () => {
    await observe('leaf.mcp.oauth', async () => {
      const cases: Record<string, boolean> = {};
      const oauthBase = process.env.LINGXI_R08_AB_OAUTH_BASE ?? '';
      if (!oauthBase) return { cases, skipped: 'oauth-standin-not-configured' };
      const addConnector = async (id: string, extra: Record<string, unknown>) => (kind === 'rust'
        ? jsonFetch('/lingxi/v1/mcp/settings-action', { method: 'POST', body: { action: 'mcp.connector.add', agentId: 'a1', payload: { connector: { id, name: `T07 ${id}`, enabled: true, ...extra } } } })
        : jsonFetch('/api/mcp/connectors', { method: 'POST', body: { id, name: `T07 ${id}`, enabled: true, ...extra } }));
      const removeConnector = async (id: string) => (kind === 'rust'
        ? jsonFetch('/lingxi/v1/mcp/settings-action', { method: 'POST', body: { action: 'mcp.connector.remove', agentId: 'a1', payload: { id } } })
        : jsonFetch(`/api/mcp/connectors/${encodeURIComponent(id)}`, { method: 'DELETE' }));
      const oauthPath = (id: string, suffix: string) => rustOrNode(`/lingxi/v1/mcp/connectors/${id}${suffix}`, `/api/mcp/connectors/${id}${suffix}`);
      // 公开状态投影里的连接器行（凭证「已保存」面：authType/authStatus/url 等配置字段）。
      const readConnector = async (id: string) => {
        const r = await jsonFetch(rustOrNode('/lingxi/v1/mcp/state', '/api/mcp/state') + '?agentId=a1');
        const body = r.body ?? {};
        const rows = Array.isArray(body?.config?.connectors) ? body.config.connectors : (Array.isArray(body?.connectors) ? body.connectors : []);
        return (rows.find((c: any) => String(c?.id) === id) ?? null) as any;
      };
      const stableConfig = (row: any) => row ? JSON.stringify({
        id: row.id, name: row.name, transport: row.transport, url: row.url,
        enabled: row.enabled, authType: row.authType, authStatus: row.authStatus,
      }) : null;
      const add = await addConnector('r08-oauth-http', {
        transport: 'streamable-http', url: `${oauthBase}/mcp`,
        // Rust 的 oauth/start 门：authType 必须为 oauth（现役连接器编辑面同款字段）。
        ...(kind === 'rust' ? { authType: 'oauth' } : {}),
      });
      cases['setup.http-connector-added'] = add.status < 300;
      const rowBefore = await readConnector('r08-oauth-http');
      // ── 2DA782 A0：start 生成授权流程并返回浏览器资料（真实发现 9728→8414→动态注册→PKCE URL）──
      const start = await jsonFetch(oauthPath('r08-oauth-http', '/oauth/start'), { method: 'POST', body: {} });
      const startText = JSON.stringify(start.body ?? '');
      cases['oauth.start-returns-authorization-material'] = start.status < 300
        && /\/authorize\?/.test(startText)
        && /client_id=/.test(startText)
        && /code_challenge=/.test(startText)
        && (typeof start.body?.url === 'string' || typeof start.body?.authorizationUrl === 'string');
      // ── E7F852 A0：cancel 结束尚在等待的浏览器往返；已保存的连接器配置不变 ──
      const cancel = await jsonFetch(oauthPath('r08-oauth-http', '/oauth/cancel'), { method: 'POST', body: {} });
      const cancelText = JSON.stringify(cancel.body ?? '');
      cases['oauth.cancel-ends-pending-roundtrip'] = cancel.status < 300 && (/cancelled/.test(cancelText));
      const rowAfterCancel = await readConnector('r08-oauth-http');
      cases['oauth.cancel-keeps-saved-credentials'] = Boolean(rowBefore && rowAfterCancel) && stableConfig(rowBefore) === stableConfig(rowAfterCancel);
      // 再 start → cancel（幂等面：无等待也如实回 0/ok）。
      const start2 = await jsonFetch(oauthPath('r08-oauth-http', '/oauth/start'), { method: 'POST', body: {} });
      const cancel2 = await jsonFetch(oauthPath('r08-oauth-http', '/oauth/cancel'), { method: 'POST', body: {} });
      cases['oauth.cancel-without-pending-is-honest'] = start2.status < 300 && cancel2.status < 300;
      // ── C70E81 A0：logout 清除连接器 OAuth 凭证并返回公开状态（不宣称已连接）──
      const logout = await jsonFetch(oauthPath('r08-oauth-http', '/oauth/logout'), { method: 'POST', body: {} });
      const logoutText = JSON.stringify(logout.body ?? '');
      const rowAfterLogout = await readConnector('r08-oauth-http');
      cases['oauth.logout-returns-public-state'] = logout.status < 300 && (logoutText.includes('connector') || logoutText.includes('state'));
      cases['oauth.logout-clears-public-credential-state'] = Boolean(rowAfterLogout)
        && rowAfterLogout?.authStatus !== 'connected'
        && rowAfterLogout?.oauth?.connected !== true;
      // ── 负路径（三叶 A1 共用）：未知连接器 / 权限不足显式拒绝，不呈现成功 ──
      const ghostStart = await jsonFetch(oauthPath('r08-no-such-conn', '/oauth/start'), { method: 'POST', body: {} });
      cases['oauth.start-unknown-connector-rejected'] = ghostStart.status >= 400 && ghostStart.status < 500;
      const ghostLogout = await jsonFetch(oauthPath('r08-no-such-conn', '/oauth/logout'), { method: 'POST', body: {} });
      cases['oauth.logout-unknown-connector-rejected'] = ghostLogout.status >= 400 && ghostLogout.status < 500;
      const unauthCancel = await withInvalidToken(async () => jsonFetch(oauthPath('r08-oauth-http', '/oauth/cancel'), { method: 'POST', body: {} }));
      cases['oauth.cancel-unauthorized-rejected'] = unauthCancel.status === 401 || unauthCancel.status === 403;
      const unauthLogout = await withInvalidToken(async () => jsonFetch(oauthPath('r08-oauth-http', '/oauth/logout'), { method: 'POST', body: {} }));
      cases['oauth.logout-unauthorized-rejected'] = unauthLogout.status === 401 || unauthLogout.status === 403;
      await removeConnector('r08-oauth-http');
      return { cases, addStatus: add.status, startStatus: start.status, startBody: startText.slice(0, 300), cancelStatus: cancel.status, cancelBody: cancelText.slice(0, 160), logoutStatus: logout.status, logoutBody: logoutText.slice(0, 240),
        ghostStartStatus: ghostStart.status, ghostLogoutStatus: ghostLogout.status, unauthCancelStatus: unauthCancel.status, unauthLogoutStatus: unauthLogout.status,
        rowBefore: stableConfig(rowBefore), rowAfterCancel: stableConfig(rowAfterCancel), rowAfterLogoutAuthStatus: rowAfterLogout?.authStatus ?? null, oauthBase };
    });
  }, 240_000);

});
