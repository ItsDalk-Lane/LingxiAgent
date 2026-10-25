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

  it('sessions.list: 查看可见会话列表', async () => {
    await observe('sessions.list', async () => {
      await loadSessions();
      const sessions = useStore.getState().sessions as any[];
      return { count: sessions.length, titles: titles(), pinned: sessions.filter((s) => s.pinned).length };
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
      const target = sessions[0];
      if (!target) return { hitAtLeastOne: false, invalidRejected: false, skipped: true };
      const base = kind === 'rust' ? `/lingxi/v1/sessions/find?sessionId=${encodeURIComponent(target.sessionId)}&q=${encodeURIComponent('T07')}` : `/api/sessions/find?path=${encodeURIComponent(target.path)}&q=${encodeURIComponent('T07')}`;
      const res = await lingxiFetch(base, { method: 'GET', throwOnHttpError: false });
      const body = await res.json().catch(() => null);
      const rows = Array.isArray(body) ? body : (body?.results ?? body?.entries ?? body?.matches ?? []);
      const badPath = kind === 'rust' ? '/lingxi/v1/sessions/find?sessionId=sess-does-not-exist-t07&q=x' : '/api/sessions/find?path=agents/a1/sessions/sess-does-not-exist-t07.jsonl&q=x';
      const bad = await lingxiFetch(badPath, { method: 'GET', throwOnHttpError: false });
      const badBody = await bad.json().catch(() => null);
      return {
        status: res.status, rowsIsArray: Array.isArray(rows),
        hitAtLeastOne: res.status < 300 && (Array.isArray(rows) ? rows.length >= 1 : Boolean(body)),
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
        return { freshKept: false, purgedOk: false, expiringGone: false, skipped: true };
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
      return { count: useStore.getState().sessions.length, titles: titles() };
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
});
