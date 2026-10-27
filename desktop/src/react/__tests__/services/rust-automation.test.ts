import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createServer } from 'node:http';
import type { ServerConnection } from '../../services/server-connection';
import type { Activity, DesktopNotificationOptions } from '../../types';

const store = vi.hoisted(() => ({ state: {} as Record<string, unknown> }));
vi.mock('../../stores', () => ({
  useStore: { getState: () => store.state, subscribe: () => () => {} },
}));

import { lingxiFetch, lingxiUrl } from '../../hooks/use-hana-fetch';
import {
  canDeliverRustAutomationNotifications,
  rustAutomationRoute,
  startRustAutomationNotifications,
} from '../../services/rust-automation';

function connection(patch: Partial<ServerConnection> = {}): ServerConnection {
  return {
    connectionId: 'local', kind: 'local', serverId: 'local', serverNodeId: 'rust-local',
    serverNodeKind: 'lingxi-service', studioId: 'local', userId: 'user_local',
    label: '本机', baseUrl: 'http://127.0.0.1:3210', wsUrl: 'ws://127.0.0.1:3210',
    token: 'owner-token', authState: 'paired', trustState: 'local',
    credentialKind: 'loopback_token', capabilities: ['chat'], ...patch,
  };
}

function receipt(id = 'delivery-1', patch: Record<string, unknown> = {}) {
  return {
    deliveryId: id, channel: 'desktop', recipient: 'user_local', status: 'queued', runId: 'run-1',
    triggerId: 'trigger-1', platformReceipt: null, error: null, createdAt: 1_800_000_000_000, updatedAt: 1_800_000_000_001,
    payloadJson: JSON.stringify({ title: '已完成', body: '整理完成', sessionId: 'session-1', runId: 'run-1' }),
    ...patch,
  };
}

function response(value: unknown): Response {
  return { ok: true, status: 200, json: async () => value } as Response;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}

const stops: Array<() => void> = [];

function harness(options: {
  connection?: ServerConnection;
  fetch?: ReturnType<typeof vi.fn>;
  show?: (title: string, body: string, agentId: string | null, options: DesktopNotificationOptions) => unknown;
  deliverEvent?: (type: string, payload: Record<string, unknown>, source: string, isCurrent: () => boolean) => void | Promise<void>;
} = {}) {
  let current: ServerConnection | null = options.connection ?? connection();
  const source = current;
  let listener: (() => void) | undefined;
  const unsubscribe = vi.fn(() => { listener = undefined; });
  const fetch = options.fetch ?? vi.fn(async (_url: unknown, init: RequestInit) => {
    const body = init.body ? JSON.parse(init.body as string) : null;
    if (!body) return response({ deliveries: [receipt()] });
    if (body.action === 'claim') return response({ claim: { reserved: true, receipt: receipt(body.deliveryId, { status: 'started' }) } });
    return response({ ok: true });
  });
  const show = options.show ?? vi.fn(async () => ({ shown: true }));
  const reportError = vi.fn();
  const replaceActivities = vi.fn();
  const stop = startRustAutomationNotifications(source, {
    currentConnection: () => current,
    subscribeConnection: next => { listener = next; return unsubscribe; },
    fetch: fetch as typeof globalThis.fetch,
    showNotification: show, reportError, deliverEvent: options.deliverEvent, replaceActivities,
  });
  stops.push(stop);
  return {
    fetch, show, stop, unsubscribe, reportError, replaceActivities,
    switchConnection(next: ServerConnection | null) { current = next; listener?.(); },
    actions: () => fetch.mock.calls.flatMap((call: unknown[]) => {
      const init = call[1] as RequestInit;
      return init.body ? [JSON.parse(init.body as string)] : [];
    }),
  };
}

beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => {
  stops.splice(0).forEach(stop => stop());
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

function activity(id: string, status = 'running'): Record<string, unknown> {
  return { id, type: 'cron', label: '合成计划', agentId: 'lingxi', agentName: '合成助手',
    startedAt: 1_800_000_000_000, finishedAt: status === 'done' ? 1_800_000_001_000 : null,
    status, sessionFile: 'lingxi-session:activity-1', sessionPath: 'lingxi-session:activity-1',
    sessionId: 'activity-1', runId: 'run-1', jobId: 'job-1', triggerId: id };
}

it('正式事件游标向页面递交工作流、预算和便笺事件，同一序号不重复消费', async () => {
  const events = ['agent_activity', 'workflow_progress', 'goal_over_budget', 'jian_update'].map((type, index) => ({
    seq: index + 1, type, source: 'server', atUnixMs: 1_800_000_000_000, payload: { sessionId: 's1' },
  }));
  let phase = 0;
  const deliverEvent = vi.fn();
  const fetchImpl = vi.fn(async (url: unknown) => {
    const path = new URL(String(url)).pathname;
    if (path.endsWith('/app-events')) return response(phase++ === 0
      ? { stale: false, latestSequence: 0, events: [] }
      : { stale: false, latestSequence: 4, events });
    if (path.endsWith('/browser/workspaces')) return response({ workspaces: [] });
    return response({ activities: [] });
  });
  harness({ connection: connection({ kind: 'custom_remote', trustState: 'tunnel', credentialKind: 'device_credential' }), fetch: fetchImpl, deliverEvent });
  await vi.advanceTimersByTimeAsync(0); await vi.advanceTimersByTimeAsync(5_000);
  expect(deliverEvent.mock.calls.map(call => call[0])).toEqual(events.map(event => event.type));
  await vi.advanceTimersByTimeAsync(5_000);
  expect(deliverEvent).toHaveBeenCalledTimes(4);
});

function appEvent(seq: number, type: string, payload: Record<string, unknown>) {
  return { seq, type, payload, source: 'server', atUnixMs: 1_800_000_000_000 + seq };
}

describe('Rust 通知设置入口', () => {
  it('保留原设置请求与查询参数，Node地址保持原样', () => {
    expect(rustAutomationRoute({ serverNodeKind: 'lingxi-service' }, '/api/preferences/notifications?view=all')).toBe('/lingxi/v1/preferences/notifications?view=all');
    expect(rustAutomationRoute({ serverNodeKind: 'node' }, '/api/preferences/notifications')).toBe('/api/preferences/notifications');
  });
});

describe('Rust 真实 app-events 活动消费', () => {
  it('Bridge/RC 消费同一正式事件游标；远端连接只读事件，不领取本机通知', async () => {
    let reads = 0;
    const deliverEvent = vi.fn();
    const types = ['bridge_status', 'bridge_message', 'bridge_rc_attached', 'bridge_rc_detached'];
    const fetch = vi.fn(async (url: string) => {
      if (url.includes('/browser/workspaces')) return response({ workspaces: [] });
      if (url.includes('/activities')) return response({ activities: [] });
      if (url.includes('/app-events')) return response(++reads === 1
        ? { stale: false, latestSequence: 0, events: [] }
        : { stale: false, latestSequence: 4, events: types.map((type, index) => appEvent(index + 1, type, { agentId: 'hana', sessionPath: 'native:session', sessionKey: 'key' })) });
      throw new Error('远端不得领取本机通知');
    });
    const h = harness({ connection: connection({ kind: 'custom_remote', trustState: 'tunnel', credentialKind: 'device_credential', baseUrl: 'https://native.example.test' }), fetch, deliverEvent });
    await vi.advanceTimersByTimeAsync(0); await vi.advanceTimersByTimeAsync(10_000);
    expect(deliverEvent.mock.calls.map(call => call[0])).toEqual(types);
    const owns = deliverEvent.mock.calls[0][3]; expect(owns()).toBe(true);
    h.switchConnection(connection({ token: 'changed' })); expect(owns()).toBe(false);
    expect(fetch.mock.calls.some(call => call[0].includes('/notifications'))).toBe(false);
  });

  it('通过真实本机HTTP读取事件与快照，不以伪事件直接写页面', async () => {
    // 此项依赖真实网络完成后的下一轮定时，不提前推进虚拟时间跨过未完成的I/O。
    vi.useRealTimers();
    const paths: string[] = [];
    let eventReads = 0;
    const initial = deferred<void>();
    const delivered = deferred<Record<string, unknown>>();
    const show = vi.fn();
    const server = createServer((request, result) => {
      paths.push(request.url ?? '');
      result.setHeader('Content-Type', 'application/json');
      if (request.headers.authorization !== 'Bearer owner-token') {
        result.statusCode = 401; result.end(JSON.stringify({ error: 'owner token required' })); return;
      }
      const path = new URL(request.url!, 'http://127.0.0.1').pathname;
      const body = path.endsWith('/browser/workspaces') ? { workspaces: [] } : path.endsWith('/notifications') ? { deliveries: [] }
        : path.endsWith('/activities') ? { activities: [] }
          : ++eventReads === 1 ? { stale: false, latestSequence: 0, events: [] }
            : { stale: false, latestSequence: 1, events: [appEvent(1, 'activity_update', { activity: activity('http-trigger', 'done') })] };
      result.end(JSON.stringify(body));
    });
    await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
    const address = server.address();
    if (!address || typeof address === 'string') throw new Error('test HTTP address unavailable');
    const source = connection({ baseUrl: `http://127.0.0.1:${address.port}`, wsUrl: `ws://127.0.0.1:${address.port}` });
    const stop = startRustAutomationNotifications(source, {
      currentConnection: () => source, subscribeConnection: () => () => {}, showNotification: show,
      replaceActivities: () => initial.resolve(), deliverEvent: (_type, payload) => delivered.resolve(payload),
    });
    stops.push(stop);
    try {
      await initial.promise;
      expect(await delivered.promise).toMatchObject({ activity: { id: 'http-trigger', status: 'done', runId: 'run-1', sessionId: 'activity-1' } });
      expect(paths).toContain('/lingxi/v1/desk/activities');
      expect(paths.filter(path => path === '/lingxi/v1/app-events?since=0')).toHaveLength(2);
      expect(show).not.toHaveBeenCalled();
    } finally {
      stop();
      server.closeAllConnections();
      await new Promise<void>(resolve => server.close(() => resolve()));
    }
  }, 10_000);

  it('使用正式 since 游标并去重，完成事件读取真实持久活动且不展示通知', async () => {
    let polls = 0;
    let snapshots = 0;
    const deliverEvent = vi.fn();
    const fetch = vi.fn(async (url: string) => {
      const path = new URL(url).pathname;
      if (path.endsWith('/browser/workspaces')) return response({ workspaces: [] });
      if (path.endsWith('/notifications')) return response({ deliveries: [] });
      if (path.endsWith('/activities')) return response({ activities: ++snapshots === 1 ? [] : [activity('trigger-1', 'done')] });
      if (path.endsWith('/app-events')) return response(++polls === 1
        ? { stale: false, latestSequence: 0, events: [] }
        : { stale: false, latestSequence: 2, events: [appEvent(1, 'activity_update', { activity: activity('trigger-1') }),
          appEvent(1, 'activity_update', { activity: activity('trigger-1') }), appEvent(2, 'cron_job_done', { jobId: 'job-1', triggerId: 'trigger-1', actorAgentId: 'lingxi', status: 'succeeded' })] });
      throw new Error('unexpected route');
    });
    const h = harness({ fetch, deliverEvent });
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(deliverEvent.mock.calls.map(call => call[0])).toEqual(['activity_update', 'cron_job_done']);
    expect(deliverEvent.mock.calls[0][1]).toMatchObject({ activity: { id: 'trigger-1', title: '合成计划', timestamp: new Date(1_800_000_000_000).toISOString(), startedAt: 1_800_000_000_000 } });
    expect(fetch.mock.calls.map(call => call[0]).filter(url => url.includes('/app-events'))).toEqual([
      'http://127.0.0.1:3210/lingxi/v1/app-events?since=0',
      'http://127.0.0.1:3210/lingxi/v1/app-events?since=0',
      'http://127.0.0.1:3210/lingxi/v1/app-events?since=2',
    ]);
    expect(h.replaceActivities).toHaveBeenCalledTimes(2);
    expect((h.replaceActivities.mock.calls[1][0] as Activity[])[0].status).toBe('done');
    expect(h.show).not.toHaveBeenCalled();
  });

  it('媒体生成与语音转写事件沿同一正式游标转发到页面动作', async () => {
    let polls = 0;
    const deliverEvent = vi.fn();
    const fetch = vi.fn(async (url: string) => {
      const path = new URL(url).pathname;
      if (path.endsWith('/browser/workspaces')) return response({ workspaces: [] });
      if (path.endsWith('/notifications')) return response({ deliveries: [] });
      if (path.endsWith('/activities')) return response({ activities: [] });
      if (path.endsWith('/app-events')) return response(++polls === 1
        ? { stale: false, latestSequence: 0, events: [] }
        : { stale: false, latestSequence: 2, events: [
          appEvent(1, 'media_generation_update', { sessionId: 'sess-1', mediaGeneration: { taskId: 'task-1' } }),
          appEvent(2, 'voice_transcription_update', { sessionId: 'sess-1', fileId: 'sf_abc', transcription: { status: 'ready', text: '装配线实录' } })] });
      throw new Error('unexpected route');
    });
    harness({ fetch, deliverEvent });
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(deliverEvent.mock.calls.map(call => call[0])).toEqual(['media_generation_update', 'voice_transcription_update']);
    expect(deliverEvent.mock.calls[1][1]).toMatchObject({ sessionId: 'sess-1', fileId: 'sf_abc', transcription: { status: 'ready', text: '装配线实录' } });
    expect(deliverEvent.mock.calls[0][3]()).toBe(true);
  });

  it('过期和服务重启都重读真实快照；坏快照不能清空现有活动或推进游标', async () => {
    let polls = 0;
    let snapshots = 0;
    const fetch = vi.fn(async (url: string) => {
      const path = new URL(url).pathname;
      if (path.endsWith('/browser/workspaces')) return response({ workspaces: [] });
      if (path.endsWith('/notifications')) return response({ deliveries: [] });
      if (path.endsWith('/activities')) return response(++snapshots === 2 ? { invalid: true } : { activities: [activity(`trigger-${snapshots}`)] });
      if (path.endsWith('/app-events')) {
        polls++;
        return response(polls === 1 ? { stale: false, latestSequence: 5, events: [] }
          : polls < 4 ? { stale: true, latestSequence: 2000, events: [] }
            : { stale: false, latestSequence: 1, events: [] });
      }
      throw new Error('unexpected route');
    });
    const h = harness({ fetch, deliverEvent: vi.fn() });
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(5_000);
    expect(h.replaceActivities).toHaveBeenCalledOnce();
    expect(h.reportError).toHaveBeenCalledOnce();
    await vi.advanceTimersByTimeAsync(10_000);
    const paths = fetch.mock.calls.map(call => call[0]).filter(url => url.includes('/app-events'));
    expect(paths.map(url => new URL(url).searchParams.get('since'))).toEqual(['0', '5', '5', '2000']);
    expect(h.replaceActivities).toHaveBeenCalledTimes(3);
    expect(h.show).not.toHaveBeenCalled();
  });

  it('快照请求中切换连接立即中止，返回旧结果也不写入新页面', async () => {
    const snapshot = deferred<Response>();
    const fetch = vi.fn(async (url: string) => {
      if (url.includes('/notifications')) return response({ deliveries: [] });
      if (url.includes('/app-events')) return response({ stale: false, latestSequence: 0, events: [] });
      return snapshot.promise;
    });
    const h = harness({ fetch, deliverEvent: vi.fn() });
    await vi.advanceTimersByTimeAsync(0);
    const signal = (fetch.mock.calls[2] as unknown[])[1] as RequestInit;
    h.switchConnection(connection({ studioId: 'other' }));
    expect(signal.signal?.aborted).toBe(true);
    snapshot.resolve(response({ activities: [activity('old')] }));
    await vi.advanceTimersByTimeAsync(60_000);
    expect(h.replaceActivities).not.toHaveBeenCalled();
    expect(fetch).toHaveBeenCalledTimes(3);
  });
});

describe('Rust 自动化页面路由', () => {
  it.each([
    '/api/desk/cron', '/api/desk/cron?agentId=a&view=all',
    '/api/desk/cron/history?jobId=j&limit=50', '/api/desk/heartbeat?agentId=a',
    '/api/models?agentId=a', '/api/agents/a/config', '/api/desk/activities?limit=20',
    '/api/desk/activities/activity-1/session', '/api/desk/activities/activity-1/promote', '/api/desk/activities/summarize',
    '/api/desk/beautify/status', '/api/desk/beautify/cover?agentId=a',
    '/api/desk/beautify/cover/apply', '/api/desk/beautify/cover/preset/apply',
  ])('仅切换正式路径且保留参数：%s', path => {
    expect(rustAutomationRoute(connection(), path)).toBe(`/lingxi/v1${path.slice(4)}`);
    expect(rustAutomationRoute(connection({ serverNodeKind: undefined }), path)).toBe(path);
  });

  it('全局设置使用已存在的 Rust 设置路由并保持 Node 路径', () => {
    expect(rustAutomationRoute(connection(), '/api/config?agentId=a')).toBe('/lingxi/v1/settings/config?agentId=a');
    expect(rustAutomationRoute(connection({ serverNodeKind: undefined }), '/api/config')).toBe('/api/config');
  });

  it('不改相似路径或未实现的动作', () => {
    for (const path of ['/api/desk/crontab', '/api/desk/cron/unknown', '/api/desk/heartbeat/unknown', '/api/models/unknown', '/api/desk/activities/activity-1/unknown', '/api/desk/beautify', '/api/desk/beautify/status/unknown', '/api/desk/beautify/cover/apply/unknown', '/api/desk/beautify/cover/preset', '/api/desk/beautify/cover/retry']) {
      expect(rustAutomationRoute(connection(), path)).toBe(path);
    }
  });

  it('现有 fetch 入口保留方法、请求体、参数和连接认证', async () => {
    store.state = { activeServerConnection: connection() };
    const fetch = vi.fn(async () => response({ ok: true }));
    vi.stubGlobal('fetch', fetch);
    const body = JSON.stringify({ action: 'toggle', jobId: 'job-1', enabled: false });
    await lingxiFetch('/api/desk/cron?agentId=a', { method: 'POST', body });
    expect(fetch).toHaveBeenCalledWith('http://127.0.0.1:3210/lingxi/v1/desk/cron?agentId=a', expect.objectContaining({
      method: 'POST', body, headers: { Authorization: 'Bearer owner-token' },
    }));
    expect(lingxiUrl('/api/desk/cron/history?jobId=j')).toBe('http://127.0.0.1:3210/lingxi/v1/desk/cron/history?jobId=j&token=owner-token');
  });
});

describe('Rust 桌面通知持久队列', () => {
  it('先取得独占领取凭据，再显示，并凭真实展示回执确认', async () => {
    const h = harness();
    await vi.advanceTimersByTimeAsync(0);
    expect(h.actions()).toEqual([
      { action: 'claim', deliveryId: 'delivery-1' },
      { action: 'ack', deliveryId: 'delivery-1', platformReceipt: { deliveryId: 'delivery-1', channel: 'desktop', displayed: true } },
    ]);
    expect(h.show).toHaveBeenCalledExactlyOnceWith('已完成', '整理完成', null, { desktopFocusPolicy: 'always' });
    expect(h.fetch.mock.calls[0][0]).toBe('http://127.0.0.1:3210/lingxi/v1/automation/notifications');
    expect(h.fetch.mock.calls[0][1].headers).toEqual({ Authorization: 'Bearer owner-token' });
    expect(h.reportError).not.toHaveBeenCalled();
  });

  it('等待真实桌面桥的返回值，桥未确认前不记送达', async () => {
    const source = connection();
    const display = deferred<unknown>();
    const show = vi.fn(() => display.promise);
    vi.stubGlobal('window', { hana: { showNotification: show } });
    const fetch = vi.fn()
      .mockResolvedValueOnce(response({ deliveries: [receipt()] }))
      .mockResolvedValueOnce(response({ claim: { reserved: true, receipt: receipt('delivery-1', { status: 'started' }) } }))
      .mockResolvedValueOnce(response({ ok: true }));
    stops.push(startRustAutomationNotifications(source, {
      currentConnection: () => source, subscribeConnection: () => () => {}, fetch,
    }));
    await vi.advanceTimersByTimeAsync(0);
    expect(show).toHaveBeenCalledExactlyOnceWith('已完成', '整理完成', null, { desktopFocusPolicy: 'always' });
    expect(fetch).toHaveBeenCalledTimes(2);
    display.resolve({ shown: true });
    await vi.advanceTimersByTimeAsync(0);
    expect(JSON.parse(fetch.mock.calls[2][1].body)).toMatchObject({ action: 'ack' });
  });

  it('没有桌面桥时不领取；本机启动尚未补 userId 时仍可验证所有者令牌', () => {
    vi.stubGlobal('window', {});
    const source = connection({ userId: undefined });
    expect(canDeliverRustAutomationNotifications(source)).toBe(true);
    const fetch = vi.fn();
    stops.push(startRustAutomationNotifications(source, {
      currentConnection: () => source, subscribeConnection: () => () => {}, fetch,
    }));
    expect(fetch).not.toHaveBeenCalled();
  });

  it('助手身份只取领取后冻结的 payload；缺少身份的旧记录传 null', async () => {
    store.state = { currentAgentId: 'focus-agent' };
    const fetch = vi.fn(async (_url: unknown, init: RequestInit) => {
      const body = init.body ? JSON.parse(init.body as string) : null;
      if (!body) return response({ deliveries: [receipt('agent-b'), receipt('old')] });
      if (body.action === 'claim') return response({ claim: { reserved: true, receipt: receipt(body.deliveryId, {
        status: 'started', payloadJson: JSON.stringify({ title: body.deliveryId, body: '真实来源', sessionId: 'session-1', runId: 'run-1', ...(body.deliveryId === 'agent-b' ? { agentId: 'agent-b' } : {}) }),
      }) } });
      return response({ ok: true });
    });
    const h = harness({ fetch });
    await vi.advanceTimersByTimeAsync(0);
    expect(h.show).toHaveBeenNthCalledWith(1, 'agent-b', '真实来源', 'agent-b', { desktopFocusPolicy: 'always' });
    expect(h.show).toHaveBeenNthCalledWith(2, 'old', '真实来源', null, { desktopFocusPolicy: 'always' });
    expect(h.actions().filter(item => item.action === 'ack')).toHaveLength(2);
  });

  it('仅未聚焦时提醒的设置传到桌面桥，窗口聚焦抑制后保存已知失败', async () => {
    const fetch = vi.fn()
      .mockResolvedValueOnce(response({ deliveries: [receipt()] }))
      .mockResolvedValueOnce(response({ claim: { reserved: true, receipt: receipt('delivery-1', { status: 'started', payloadJson: JSON.stringify({ title: '提醒', body: '完成', sessionId: 'session-1', runId: 'run-1', desktopFocusPolicy: 'when_unfocused' }) }) } }))
      .mockResolvedValueOnce(response({ ok: true }));
    const h = harness({ fetch, show: vi.fn(async () => ({ shown: false, reason: 'hana_focused' })) });
    await vi.advanceTimersByTimeAsync(0);
    expect(h.show).toHaveBeenCalledExactlyOnceWith('提醒', '完成', null, { desktopFocusPolicy: 'when_unfocused' });
    expect(h.actions().map(item => item.action)).toEqual(['claim', 'failed']);
    await vi.advanceTimersByTimeAsync(5_000);
    expect(h.show).toHaveBeenCalledOnce();
  });

  it('显式 always 正常展示，未知焦点策略不能静默改成总是提醒', async () => {
    const fetch = vi.fn(async (_url: unknown, init: RequestInit) => {
      const body = init.body ? JSON.parse(init.body as string) : null;
      if (!body) return response({ deliveries: [receipt('always'), receipt('invalid')] });
      if (body.action === 'claim') return response({ claim: { reserved: true, receipt: receipt(body.deliveryId, { status: 'started', payloadJson: JSON.stringify({ title: '提醒', body: '完成', sessionId: 'session-1', runId: 'run-1', desktopFocusPolicy: body.deliveryId === 'always' ? 'always' : 'unknown' }) }) } });
      return response({ ok: true });
    });
    const h = harness({ fetch });
    await vi.advanceTimersByTimeAsync(0);
    expect(h.show).toHaveBeenCalledExactlyOnceWith('提醒', '完成', null, { desktopFocusPolicy: 'always' });
    expect(h.actions().map(item => item.action)).toEqual(['claim', 'ack', 'claim', 'failed']);
  });

  it('领取失败或已开始的记录不能显示；使用领取后冻结的内容', async () => {
    const fetch = vi.fn(async (_url: unknown, init: RequestInit) => {
      const body = init.body ? JSON.parse(init.body as string) : null;
      if (!body) return response({ deliveries: [receipt('taken'), receipt('good'), receipt('started', { status: 'started' }), receipt('unknown', { status: 'unknown' })] });
      if (body.action === 'claim') return response({ claim: {
        reserved: body.deliveryId !== 'taken',
        receipt: receipt(body.deliveryId, { status: 'started', payloadJson: JSON.stringify({ title: '冻结标题', body: '冻结内容', sessionId: 'session-1', runId: 'run-1' }) }),
      } });
      return response({ ok: true });
    });
    const h = harness({ fetch });
    await vi.advanceTimersByTimeAsync(0);
    expect(h.show).toHaveBeenCalledExactlyOnceWith('冻结标题', '冻结内容', null, { desktopFocusPolicy: 'always' });
    expect(h.actions().map(item => item.deliveryId)).toEqual(['taken', 'good', 'good']);
  });

  it.each([
    [false, 'failed'], [undefined, 'unknown'], [{ shown: false }, 'failed'],
    [{ shown: 1 }, 'unknown'], [{ shown: false, reason: 'unconfirmed' }, 'unknown'],
  ])('按真实显示结果记录失败或未知：%s', async (result, action) => {
    const h = harness({ show: vi.fn(async () => result) });
    await vi.advanceTimersByTimeAsync(0);
    expect(h.actions().map(item => item.action)).toEqual(['claim', action]);
    expect(h.actions()[1]).not.toHaveProperty('platformReceipt');
  });

  it('展示抛错保留未知结果，不冒充未显示或送达', async () => {
    const h = harness({ show: vi.fn(async () => { throw new Error('IPC disconnected'); }) });
    await vi.advanceTimersByTimeAsync(0);
    expect(h.actions()).toEqual([{ action: 'claim', deliveryId: 'delivery-1' }, {
      action: 'unknown', deliveryId: 'delivery-1', error: '桌面通知展示结果未确认',
    }]);
  });

  it('来源凭据和通知内容异常时拒绝展示并保存失败', async () => {
    const fetch = vi.fn()
      .mockResolvedValueOnce(response({ deliveries: [receipt()] }))
      .mockResolvedValueOnce(response({ claim: { reserved: true, receipt: receipt('delivery-1', { status: 'started', runId: 'other-run' }) } }))
      .mockResolvedValueOnce(response({ ok: true }));
    const h = harness({ fetch });
    await vi.advanceTimersByTimeAsync(0);
    expect(h.show).not.toHaveBeenCalled();
    expect(h.actions().map(item => item.action)).toEqual(['claim', 'failed']);
  });

  it('拒绝未知 DTO 字段与无效时间，领取后格式损坏明确保存失败', async () => {
    const fetch = vi.fn()
      .mockResolvedValueOnce(response({ deliveries: [receipt('extra', { displayed: true }), receipt('bad-time', { createdAt: 'now' }), receipt()] }))
      .mockResolvedValueOnce(response({ claim: { reserved: true, receipt: receipt('delivery-1', { status: 'started', trusted: true }) } }))
      .mockResolvedValueOnce(response({ ok: true }));
    const h = harness({ fetch });
    await vi.advanceTimersByTimeAsync(0);
    expect(h.show).not.toHaveBeenCalled();
    expect(h.actions().map(item => item.action)).toEqual(['claim', 'failed']);
    expect(h.reportError).toHaveBeenCalledTimes(2);
  });

  it('确认请求失败后不重复展示，即使旧列表再次返回该记录', async () => {
    const fetch = vi.fn(async (_url: unknown, init: RequestInit) => {
      const body = init.body ? JSON.parse(init.body as string) : null;
      if (!body) return response({ deliveries: [receipt(), receipt()] });
      if (body.action === 'claim') return response({ claim: { reserved: true, receipt: receipt('delivery-1', { status: 'started' }) } });
      throw new Error('ack outcome unknown');
    });
    const h = harness({ fetch });
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(h.show).toHaveBeenCalledOnce();
    expect(h.actions().map(item => item.action)).toEqual(['claim', 'ack']);
    expect(h.reportError).toHaveBeenCalledOnce();
  });

  it('上次请求结束后才开始下一轮，停止时中止请求并移除监听', async () => {
    const first = deferred<Response>();
    const second = deferred<Response>();
    const fetch = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const h = harness({ fetch });
    await vi.advanceTimersByTimeAsync(20_000);
    expect(fetch).toHaveBeenCalledOnce();
    first.resolve(response({ deliveries: [] }));
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(5_000);
    expect(fetch).toHaveBeenCalledTimes(2);
    const signal = fetch.mock.calls[1][1].signal as AbortSignal;
    h.stop();
    expect(signal.aborted).toBe(true);
    expect(h.unsubscribe).toHaveBeenCalledOnce();
    second.resolve(response({ deliveries: [receipt()] }));
    await vi.advanceTimersByTimeAsync(60_000);
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(h.show).not.toHaveBeenCalled();
  });

  it('领取期间切换身份，旧结果即使返回也不能展示，切回不复活', async () => {
    const claim = deferred<Response>();
    const fetch = vi.fn().mockResolvedValueOnce(response({ deliveries: [receipt()] })).mockReturnValueOnce(claim.promise);
    const h = harness({ fetch });
    await vi.advanceTimersByTimeAsync(0);
    h.switchConnection(connection({ token: 'new-owner-token' }));
    expect((fetch.mock.calls[1][1].signal as AbortSignal).aborted).toBe(true);
    h.switchConnection(connection());
    claim.resolve(response({ claim: { reserved: true, receipt: receipt('delivery-1', { status: 'started' }) } }));
    await vi.advanceTimersByTimeAsync(60_000);
    expect(h.show).not.toHaveBeenCalled();
    expect(h.actions().map(item => item.action)).toEqual(['claim']);
  });

  it('展示期间切换连接后不拿旧结果确认送达', async () => {
    const display = deferred<unknown>();
    const h = harness({ show: vi.fn(() => display.promise) });
    await vi.advanceTimersByTimeAsync(0);
    expect(h.show).toHaveBeenCalledOnce();
    h.switchConnection(connection({ studioId: 'other-studio' }));
    display.resolve({ shown: true });
    await vi.advanceTimersByTimeAsync(60_000);
    expect(h.actions().map(item => item.action)).toEqual(['claim']);
  });

  it.each([
    { kind: 'lan' }, { kind: 'custom_remote' }, { trustState: 'tunnel' },
    { credentialKind: 'device_credential' }, { baseUrl: 'http://remote.example:3210' },
    { baseUrl: 'http://other@127.0.0.1:3210' },
    { userId: 'another-user' }, { token: null }, { authState: 'expired' }, { serverNodeKind: undefined },
  ] as Partial<ServerConnection>[])('仅允许本机 Rust 所有者：%s', patch => {
    const source = connection(patch);
    expect(canDeliverRustAutomationNotifications(source)).toBe(false);
    const h = harness({ connection: source });
    expect(h.fetch).not.toHaveBeenCalled();
    expect(h.show).not.toHaveBeenCalled();
  });
});
