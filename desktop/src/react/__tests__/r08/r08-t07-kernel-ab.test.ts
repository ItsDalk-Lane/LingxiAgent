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
      return { cleanupDeletedIsNumber: typeof cleanup?.deleted === 'number', sweptIsNumber: typeof swept === 'number' };
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

  it('leaf.cleanup.boundary: 期限边界 + 显式永久删除', async () => {
    await observe('leaf.cleanup.boundary', async () => {
      await loadSessions();
      const sessions = useStore.getState().sessions as any[];
      const byTitle = (t: string) => sessions.find((x) => (x.title || x.sessionId) === t || x.sessionId === t || x.path?.endsWith(`${t}.jsonl`));
      const fresh = byTitle('ab-real-beta') ?? sessions[sessions.length - 1];
      const expiring = byTitle('ab-real-gamma') ?? sessions[sessions.length - 2];
      if (!fresh || !expiring || fresh === expiring) {
        return { freshKept: false, purgedOk: false, expiringGone: false, skipped: true };
      }
      await archiveSession(fresh.path);
      await archiveSession(expiring.path);
      const kept = await cleanupArchivedSessions(30);
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
      return { keptDeletedIsNumber: typeof kept?.deleted === 'number', freshKept: freshStillArchived, purgedOk, expiringGone };
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
