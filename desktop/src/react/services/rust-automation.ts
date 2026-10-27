import { useStore } from '../stores';
import { browserConnectionScope } from '../stores/browser-slice';
import { parseRustBrowserWorkspaces } from './rust-browser';
import type { Activity, DesktopNotificationOptions } from '../types';
import type {
  AutomationDisplayReceipt,
  AutomationNotificationClaim,
  AutomationNotificationCommand,
  AutomationNotificationReceipt,
  AutomationNotificationStatus,
} from '../../../../contracts/generated/ts/lingxi-protocol';
import {
  appendConnectionAuth,
  buildConnectionUrl,
  resolveServerConnection,
  type ServerConnection,
} from './server-connection';
import { usePlatform } from '../hooks/use-platform';

const NOTIFICATIONS_PATH = '/lingxi/v1/automation/notifications';
const POLL_INTERVAL_MS = 5_000;
const REQUEST_TIMEOUT_MS = 30_000;

/** 保留现有页面参数，只为 Rust 自动化切换正式路由。 */
export function rustAutomationRoute(connection: { serverNodeKind?: string }, path: string): string {
  if (connection.serverNodeKind !== 'lingxi-service') return path;
  if (/^\/api\/config(?:\?|$)/.test(path)) return path.replace('/api/config', '/lingxi/v1/settings/config');
  if (/^\/api\/(?:models|preferences\/notifications|agents\/[^/?]+\/config)(?:\?|$)/.test(path)) return `/lingxi/v1${path.slice('/api'.length)}`;
  if (/^\/api\/desk\/beautify\/(?:status|cover(?:\/apply|\/preset\/apply)?)(?:\?|$)/.test(path)) return `/lingxi/v1${path.slice('/api'.length)}`;
  return /^\/api\/desk\/(?:cron(?:\/history)?|heartbeat|activities(?:\/summarize|\/[^/?]+\/(?:session|promote))?)(?:\?|$)/.test(path)
    ? `/lingxi/v1${path.slice('/api'.length)}` : path;
}

/** 本机令牌代表本机所有者；远端连接不能借用当前电脑展示通知。 */
export function canDeliverRustAutomationNotifications(connection: ServerConnection): boolean {
  if (connection.serverNodeKind !== 'lingxi-service'
    || connection.kind !== 'local'
    || connection.trustState !== 'local'
    || connection.credentialKind !== 'loopback_token'
    || !['paired', 'user'].includes(connection.authState)
    || !connection.token
    || (connection.userId !== undefined && connection.userId !== 'user_local')) return false;
  try {
    const url = new URL(connection.baseUrl);
    return ['http:', 'https:'].includes(url.protocol)
      && ['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname)
      && !url.username && !url.password;
  } catch {
    return false;
  }
}

interface NotificationDependencies {
  currentConnection: () => ServerConnection | null;
  subscribeConnection: (listener: () => void) => () => void;
  fetch: typeof globalThis.fetch;
  showNotification: (title: string, body: string, agentId: string | null, options: DesktopNotificationOptions) => unknown | Promise<unknown>;
  reportError: (message: string) => void;
  deliverEvent?: (type: string, payload: Record<string, unknown>, source: string, isCurrent: () => boolean) => void | Promise<void>;
  replaceActivities: (activities: Activity[]) => void;
}

function object(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown> : null;
}

const RECEIPT_FIELDS = new Set([
  'deliveryId', 'triggerId', 'runId', 'channel', 'recipient', 'payloadJson',
  'status', 'platformReceipt', 'error', 'createdAt', 'updatedAt',
]);
const NOTIFICATION_STATUSES: readonly AutomationNotificationStatus[] = ['queued', 'started', 'confirmed', 'unknown', 'failed'];

/** 生成类型约束写入端，实际收到的 JSON 仍须检查 closed 字段与基本值。 */
function isNotificationReceipt(value: unknown): value is AutomationNotificationReceipt {
  const row = object(value);
  return !!row && Object.keys(row).every(key => RECEIPT_FIELDS.has(key))
    && ['deliveryId', 'channel', 'recipient', 'payloadJson'].every(key => typeof row[key] === 'string')
    && ['triggerId', 'runId', 'error'].every(key => row[key] === undefined || row[key] === null || typeof row[key] === 'string')
    && typeof row.status === 'string' && NOTIFICATION_STATUSES.some(status => status === row.status)
    && typeof row.createdAt === 'number' && Number.isSafeInteger(row.createdAt)
    && typeof row.updatedAt === 'number' && Number.isSafeInteger(row.updatedAt);
}

function readActivity(value: unknown): Activity | null {
  const row = object(value);
  if (!row || typeof row.id !== 'string' || !row.id || typeof row.type !== 'string' || !row.type) return null;
  const instant = typeof row.startedAt === 'number' && Number.isSafeInteger(row.startedAt) ? new Date(row.startedAt) : null;
  const timestamp = typeof row.timestamp === 'string' ? row.timestamp
    : instant && Number.isFinite(instant.getTime()) ? instant.toISOString() : null;
  if (!timestamp) return null;
  return { ...row, id: row.id, type: row.type, timestamp,
    title: typeof row.title === 'string' ? row.title : typeof row.label === 'string' ? row.label : '后台任务' };
}

/** 递归定时避免并行领取；领取后的未知展示结果永远不自动重发。 */
export function startRustAutomationNotifications(
  source: ServerConnection,
  overrides: Partial<NotificationDependencies> = {},
): () => void {
  const show = overrides.showNotification
    ?? (typeof window !== 'undefined' && usePlatform()?.showNotification
      ? (title: string, body: string, agentId: string | null, options: DesktopNotificationOptions) => usePlatform()!.showNotification!(title, body, agentId, options)
      : null);
  const notificationsAllowed = canDeliverRustAutomationNotifications(source) && !!show;
  if (source.serverNodeKind !== 'lingxi-service' || (!notificationsAllowed && !overrides.deliverEvent)) return () => {};

  // 保存完整身份快照，不能在异步请求途中换用新连接的令牌或地址。
  const key = JSON.stringify(source);
  const connection: ServerConnection = JSON.parse(key);
  const deps: NotificationDependencies = {
    currentConnection: () => resolveServerConnection(useStore.getState()),
    subscribeConnection: listener => useStore.subscribe(listener),
    fetch: (input, init) => globalThis.fetch(input, init),
    showNotification: show ?? (() => false),
    reportError: message => console.warn('[rust-automation]', message),
    replaceActivities: activities => useStore.setState({ activities }),
    ...overrides,
  };
  let stopped = false;
  let polling = false;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let activeRequest: AbortController | null = null;
  let requestTimeout: ReturnType<typeof setTimeout> | null = null;
  let unsubscribe: (() => void) | null = null;
  const claimed = new Set<string>();
  let eventCursor = 0;
  let eventsInitialized = false;

  function stop(): void {
    if (stopped) return;
    stopped = true;
    if (timer !== null) clearTimeout(timer);
    timer = null;
    activeRequest?.abort();
    activeRequest = null;
    if (requestTimeout !== null) clearTimeout(requestTimeout);
    requestTimeout = null;
    unsubscribe?.();
    unsubscribe = null;
  }

  function ownsConnection(): boolean {
    if (stopped) return false;
    try {
      if (JSON.stringify(deps.currentConnection()) === key) return true;
    } catch { /* 连接无效时停止，不能拿旧身份继续领取。 */ }
    stop();
    return false;
  }

  async function request(body?: AutomationNotificationCommand, path = NOTIFICATIONS_PATH): Promise<Record<string, unknown>> {
    if (!ownsConnection()) throw new Error('连接已切换');
    const controller = new AbortController();
    activeRequest = controller;
    const timeout = setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);
    requestTimeout = timeout;
    try {
      const response = await deps.fetch(buildConnectionUrl(connection, path), {
        method: body ? 'POST' : 'GET',
        headers: appendConnectionAuth(connection, body ? { 'Content-Type': 'application/json' } : undefined),
        ...(body ? { body: JSON.stringify(body) } : {}),
        signal: controller.signal,
      });
      if (!ownsConnection()) throw new Error('连接已切换');
      if (!response.ok) throw new Error(`通知队列请求失败 (${response.status})`);
      const result: unknown = await response.json();
      if (!ownsConnection()) throw new Error('连接已切换');
      const value = object(result);
      if (!value) throw new Error('通知队列返回格式无效');
      return value;
    } finally {
      clearTimeout(timeout);
      if (requestTimeout === timeout) requestTimeout = null;
      if (activeRequest === controller) activeRequest = null;
    }
  }

  async function failed(deliveryId: string, error: string): Promise<void> {
    await request({ action: 'failed', deliveryId, error });
  }

  async function unknown(deliveryId: string, error: string): Promise<void> {
    await request({ action: 'unknown', deliveryId, error });
  }

  async function refreshActivities(): Promise<void> {
    const snapshot = await request(undefined, '/lingxi/v1/desk/activities');
    if (!Array.isArray(snapshot.activities)) throw new Error('后台活动列表格式无效');
    const activities = snapshot.activities.map(readActivity);
    if (activities.some(activity => activity === null)) throw new Error('后台活动凭据格式无效');
    if (ownsConnection()) deps.replaceActivities(activities as Activity[]);
  }

  async function refreshBrowserWorkspaces(): Promise<void> {
    if (!deps.deliverEvent) return;
    const snapshot = await request(undefined, '/lingxi/v1/browser/workspaces');
    const workspaces = parseRustBrowserWorkspaces(snapshot, { executionNode: connection.serverId });
    if (!ownsConnection()) return;
    const paths = new Set(workspaces.map(workspace => workspace.sessionPath));
    const scope = browserConnectionScope({ activeServerConnection: connection });
    // 只清理同一真实连接中已消失的已知工作区，未知归属与其他节点保持原样。
    const removed = Object.values(useStore.getState().browserBySession || {}).filter(workspace =>
      workspace.executionNode === connection.serverId && workspace.connectionScope === scope
      && workspace.running && typeof workspace.sessionPath === 'string'
      && workspace.sessionPath.startsWith('native:') && !paths.has(workspace.sessionPath));
    for (const workspace of workspaces) {
      if (!ownsConnection()) return;
      await deps.deliverEvent('browser_status', { ...workspace }, 'server', ownsConnection);
    }
    for (const workspace of removed) {
      if (!ownsConnection()) return;
      await deps.deliverEvent('browser_status', { sessionPath: workspace.sessionPath, executionNode: connection.serverId, running: false, url: null }, 'server', ownsConnection);
    }
  }

  async function pollAppEvents(): Promise<void> {
    if (!deps.deliverEvent || !ownsConnection()) return;
    const result = await request(undefined, `/lingxi/v1/app-events?since=${eventCursor}`);
    if (typeof result.stale !== 'boolean' || typeof result.latestSequence !== 'number'
      || !Number.isSafeInteger(result.latestSequence) || result.latestSequence < 0 || !Array.isArray(result.events)) {
      throw new Error('后台活动事件游标格式无效');
    }
    const latest = result.latestSequence;
    const events = result.events.map(item => {
      const event = object(item);
      if (!event || typeof event.seq !== 'number' || !Number.isSafeInteger(event.seq) || event.seq < 1 || event.seq > latest
        || typeof event.type !== 'string' || !event.type || !object(event.payload)
        || event.source !== 'server' || typeof event.atUnixMs !== 'number' || !Number.isSafeInteger(event.atUnixMs)) {
        throw new Error('后台活动事件格式无效');
      }
      return { seq: event.seq, type: event.type, payload: event.payload as Record<string, unknown>, source: event.source };
    }).sort((left, right) => left.seq - right.seq);
    if (result.stale && events.length > 0) throw new Error('过期游标不能伪装成完整事件列表');
    if (!eventsInitialized || result.stale || latest < eventCursor) {
      // 初次连接、服务重启与日志过期均从真实持久活动重建，不能拿空事件清空页面。
      await refreshActivities();
      await refreshBrowserWorkspaces();
      if (ownsConnection()) { eventCursor = latest; eventsInitialized = true; }
      return;
    }
    for (const event of events) {
      if (!ownsConnection()) return;
      if (event.seq <= eventCursor) continue;
      if (event.type === 'activity_update') {
        const activity = readActivity(event.payload.activity);
        if (!activity) throw new Error('后台活动更新格式无效');
        await deps.deliverEvent(event.type, { ...event.payload, activity }, event.source, ownsConnection);
      } else if (event.type === 'cron_job_done') {
        await deps.deliverEvent(event.type, event.payload, event.source, ownsConnection);
        await refreshActivities();
      } else if (['agent_activity', 'workflow_progress', 'goal_over_budget', 'jian_update', 'media_generation_update', 'voice_transcription_update', 'channel_new_message', 'channel_created', 'dm_new_message', 'conversation_agent_activity', 'session_collab_decision', 'browser_status', 'browser_update', 'bridge_status', 'bridge_message', 'bridge_rc_attached', 'bridge_rc_detached', 'rc_attached', 'rc_detached'].includes(event.type)) {
        await deps.deliverEvent(event.type, event.payload, event.source, ownsConnection);
      }
      if (!ownsConnection()) return;
      eventCursor = event.seq;
    }
    if (ownsConnection()) eventCursor = latest;
  }

  async function deliver(deliveryId: string): Promise<void> {
    if (claimed.has(deliveryId) || !ownsConnection()) return;
    const result = await request({ action: 'claim', deliveryId });
    const rawClaim = object(result.claim);
    if (!rawClaim || Object.keys(rawClaim).some(key => key !== 'reserved' && key !== 'receipt')
      || typeof rawClaim.reserved !== 'boolean') throw new Error('通知领取结果无效');
    if (!rawClaim.reserved || !ownsConnection()) return;
    claimed.add(deliveryId);
    if (!isNotificationReceipt(rawClaim.receipt)) {
      await failed(deliveryId, '通知领取凭据格式无效');
      return;
    }
    const claim: AutomationNotificationClaim = { reserved: rawClaim.reserved, receipt: rawClaim.receipt };
    const receipt = claim.receipt;
    if (receipt.deliveryId !== deliveryId || receipt.status !== 'started'
      || receipt.channel !== 'desktop' || receipt.recipient !== 'user_local') {
      await failed(deliveryId, '通知领取凭据不匹配');
      return;
    }
    let payload: Record<string, unknown> | null = null;
    try {
      if (typeof receipt.payloadJson === 'string') payload = object(JSON.parse(receipt.payloadJson));
    } catch { /* 领取后留下明确失败，不能展示格式不明的内容。 */ }
    if (!payload || typeof payload.title !== 'string' || !payload.title.trim()
      || typeof payload.body !== 'string'
      || typeof payload.sessionId !== 'string' || !payload.sessionId
      || typeof payload.runId !== 'string' || !payload.runId
      || (payload.agentId !== undefined && payload.agentId !== null
        && (typeof payload.agentId !== 'string' || !payload.agentId.trim()))
      || (payload.desktopFocusPolicy !== undefined && payload.desktopFocusPolicy !== 'always' && payload.desktopFocusPolicy !== 'when_unfocused')
      || receipt.runId !== payload.runId) {
      await failed(deliveryId, '通知内容或来源凭据无效');
      return;
    }
    if (!ownsConnection()) return;
    let shown: unknown;
    try {
      shown = await deps.showNotification(payload.title, payload.body, typeof payload.agentId === 'string' ? payload.agentId : null,
        { desktopFocusPolicy: payload.desktopFocusPolicy === 'when_unfocused' ? 'when_unfocused' : 'always' });
    } catch {
      if (ownsConnection()) await unknown(deliveryId, '桌面通知展示结果未确认');
      return;
    }
    if (!ownsConnection()) return;
    const displayResult = object(shown);
    if (displayResult?.reason === 'unconfirmed') {
      await unknown(deliveryId, '桌面系统未确认通知展示结果');
    } else if (displayResult?.shown === true) {
      const platformReceipt: AutomationDisplayReceipt = { deliveryId, channel: 'desktop', displayed: true };
      await request({
        action: 'ack', deliveryId, platformReceipt,
      });
    } else if (shown === false || displayResult?.shown === false) {
      await failed(deliveryId, '桌面系统确认通知未显示');
    } else {
      await unknown(deliveryId, '桌面通知未返回有效展示回执');
    }
  }

  async function poll(): Promise<void> {
    if (polling || !ownsConnection()) return;
    polling = true;
    try {
      if (notificationsAllowed) {
      const result = await request();
      if (!Array.isArray(result.deliveries)) throw new Error('通知队列列表无效');
      for (const item of result.deliveries) {
        if (!ownsConnection()) break;
        if (!isNotificationReceipt(item) || !item.deliveryId) {
          deps.reportError('通知队列包含无效凭据');
          continue;
        }
        const row = item;
        // Started、Unknown 等都不能重新展示，即使服务端误将其放入列表。
        if (row.status !== 'queued' || row.channel !== 'desktop' || row.recipient !== 'user_local') continue;
        try {
          await deliver(row.deliveryId);
        } catch {
          if (ownsConnection()) deps.reportError('通知领取或送达记录保存失败；已领取的通知不会重复展示');
        }
      }
      }
    } catch {
      if (ownsConnection()) deps.reportError('通知队列暂时不可用，将在下次连接检查时重试');
    } finally {
      if (ownsConnection() && deps.deliverEvent) {
        try { await pollAppEvents(); }
        catch { if (ownsConnection()) deps.reportError('后台活动事件读取失败，保留现有列表并在下次重试'); }
      }
      polling = false;
      if (ownsConnection()) timer = setTimeout(() => { timer = null; void poll(); }, POLL_INTERVAL_MS);
    }
  }

  unsubscribe = deps.subscribeConnection(() => { ownsConnection(); });
  if (ownsConnection()) void poll();
  return stop;
}
