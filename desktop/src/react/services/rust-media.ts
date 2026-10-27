/** 媒体卡只采用同一连接、同一会话的持久回执，文件沿正式资源读面打开。 */
import type { MediaJobReceipt } from '../../../../contracts/generated/ts/lingxi-protocol';
import type { ChatListItem, ContentBlock } from '../stores/chat-types';
import { useStore } from '../stores';
import { sessionScopedKey, sessionScopedValue } from '../stores/session-slice';
import { captureSessionConnection, ownsSessionConnection, sessionOwnerFetch, subscribeSessionConnection, type SessionConnectionOwner } from './session-connection-owner';
import { type ServerConnection } from './server-connection';

const phases = ['submitting', 'submitted', 'generating', 'downloading', 'completed', 'failed', 'cancelled', 'unknown'];
const fields = ['taskId','kind','provider','status','sourceRunId','executionRunId','revision','files','remoteOutcome','remoteCancel','localDeliveryStopped','message','errorCode','createdAtMs','updatedAtMs'];
const row = (value: unknown): Record<string, unknown> | null => value !== null && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : null;
const decimal = (value: unknown): value is string => typeof value === 'string' && /^(0|[1-9][0-9]{0,19})$/.test(value) && BigInt(value) <= 18446744073709551615n;
export function parseRustMediaReceipt(value: unknown): MediaJobReceipt {
  const job = row(value);
  if (!job || Object.keys(job).some(key => !fields.includes(key)) || fields.some(key => !(key in job))
    || !['taskId','provider','sourceRunId'].every(key => typeof job[key] === 'string' && !!job[key] && (job[key] as string).length <= 4096)
    || !['image','video','speech'].includes(String(job.kind)) || !phases.includes(String(job.status))
    || !['revision','createdAtMs','updatedAtMs'].every(key => decimal(job[key])) || job.revision === '0'
    || !['executionRunId','message','errorCode'].every(key => job[key] === null || typeof job[key] === 'string')
    || !['not_yet_confirmed','not_submitted','accepted','completed','failed','unknown'].includes(String(job.remoteOutcome))
    || ![null,'unsupported','confirmed','unknown'].includes(job.remoteCancel as string | null)
    || typeof job.localDeliveryStopped !== 'boolean' || !Array.isArray(job.files) || job.files.length > 64) throw new Error('媒体任务回执格式无效');
  const ids = new Set<string>();
  for (const value of job.files) {
    const file = row(value);
    if (!file || Object.keys(file).some(key => !['fileId','filename','mime','sizeBytes'].includes(key))
      || typeof file.fileId !== 'string' || !/^sf_[A-Za-z0-9][A-Za-z0-9_-]*$/.test(file.fileId)
      || typeof file.filename !== 'string' || !file.filename || file.filename.length > 4096
      || typeof file.mime !== 'string' || !file.mime || !decimal(file.sizeBytes) || ids.has(file.fileId)) throw new Error('媒体文件回执格式无效');
    ids.add(file.fileId);
  }
  if (job.status === 'completed' && job.files.length === 0) throw new Error('媒体任务完成但没有交付文件');
  return structuredClone(job) as unknown as MediaJobReceipt;
}

function newer(left: MediaJobReceipt | undefined, right: MediaJobReceipt): MediaJobReceipt {
  if (left && (['taskId', 'sourceRunId', 'kind', 'provider', 'createdAtMs'] as const).some(key => left[key] !== right[key])) {
    throw new Error('媒体任务来源发生冲突');
  }
  if (!left || BigInt(right.revision) > BigInt(left.revision)) return right;
  // 回执字段顺序不是版本事实；只比较协议字段及有序文件列表。
  const content = (job: MediaJobReceipt) => fields.map(key => key === 'files'
    ? job.files.map(file => [file.fileId, file.filename, file.mime, file.sizeBytes])
    : job[key as keyof MediaJobReceipt] ?? null);
  if (right.revision === left.revision && JSON.stringify(content(right)) !== JSON.stringify(content(left))) throw new Error('媒体任务相同版本内容冲突');
  return left;
}
function jobsFor(sessionId: string): Record<string, MediaJobReceipt> {
  const state = useStore.getState();
  return sessionScopedValue(state, state.chatSessions, `native:${sessionId}`)?.nativeMediaJobs ?? {};
}
export function rustMediaBlocks(job: MediaJobReceipt, studioId = 'studio_local'): ContentBlock[] {
  const blocks: ContentBlock[] = job.files.map(file => {
    const resourceId = `res_${file.fileId}`;
    const content = `/lingxi/v1/resources/${encodeURIComponent(resourceId)}/content`;
    const ext = file.filename.includes('.') ? file.filename.split('.').pop()!.toLowerCase() : '';
    return { type: 'file', fileId: file.fileId, filePath: `session-file:${file.fileId}`, label: file.filename, ext, mime: file.mime,
      kind: job.kind === 'speech' ? 'audio' : job.kind, storageKind: 'managed_cache', status: 'available', replacesTaskId: job.taskId,
      resource: { schemaVersion: 1, resourceId, name: `studios/${studioId}/resources/${resourceId}`, studioId,
        type: 'file', source: 'session_file', fileId: file.fileId, filename: file.filename, mime: file.mime,
        lifecycle: { status: 'available', missingAt: null }, storage: { provider: 'session_file', storageKind: 'managed_cache' },
        links: { self: content.slice(0, -'/content'.length), content } } };
  });
  if (job.status !== 'completed') blocks.push({ type: 'media_generation', taskId: job.taskId, kind: job.kind,
    status: job.status === 'cancelled' ? 'aborted' : ['failed','unknown'].includes(job.status) ? job.status : 'pending',
    nativeMedia: true, nativePhase: job.status, reason: job.message ?? undefined,
    remoteOutcome: job.remoteOutcome, remoteCancel: job.remoteCancel ?? undefined, localDeliveryStopped: job.localDeliveryStopped });
  return blocks;
}
export function rustMediaBlocksFromDetails(details: unknown, sessionId: string): ContentBlock[] {
  const generation = row(row(details)?.mediaGeneration);
  if (!generation || !Array.isArray(generation.tasks)) return [];
  const latest = jobsFor(sessionId);
  return generation.tasks.flatMap(value => { const job = parseRustMediaReceipt(value); return rustMediaBlocks(newer(job, latest[job.taskId] ?? job)); });
}
export function projectRustMediaItems(items: ChatListItem[], jobs: Record<string, MediaJobReceipt>, studioId?: string): ChatListItem[] {
  return items.map(item => {
    if (item.type !== 'message' || item.data.role !== 'assistant') return item;
    let blocks = item.data.blocks ?? [];
    let changed = false;
    for (const job of Object.values(jobs)) {
      const belongs = blocks.some(block => block.type === 'media_generation' ? block.taskId === job.taskId : block.type === 'file' && block.replacesTaskId === job.taskId)
        || item.data.id.endsWith(`:${job.sourceRunId}`);
      if (!belongs) continue;
      const old = blocks.findIndex(block => block.type === 'media_generation' ? block.taskId === job.taskId : block.type === 'file' && block.replacesTaskId === job.taskId);
      const next = blocks.filter(block => !(block.type === 'media_generation' ? block.taskId === job.taskId : block.type === 'file' && block.replacesTaskId === job.taskId));
      next.splice(old < 0 ? next.length : Math.min(old, next.length), 0, ...rustMediaBlocks(job, studioId));
      blocks = next; changed = true;
    }
    return changed ? { ...item, data: { ...item.data, blocks } } : item;
  });
}
export function applyRustMediaJobs(owner: SessionConnectionOwner, sessionId: string, values: unknown[]): void {
  if (!ownsSessionConnection(owner) || owner.connection.serverNodeKind !== 'lingxi-service') return;
  const incoming = values.map(parseRustMediaReceipt);
  useStore.setState(state => {
    if (!ownsSessionConnection(owner)) return {};
    const path = `native:${sessionId}`;
    const key = sessionScopedKey(state, path) ?? path;
    const session = sessionScopedValue(state, state.chatSessions, path);
    if (!session) return {};
    const jobs = { ...session.nativeMediaJobs };
    for (const job of incoming) jobs[job.taskId] = newer(jobs[job.taskId], job);
    return { chatSessions: { ...state.chatSessions, [key]: { ...session, nativeMediaJobs: jobs, items: projectRustMediaItems(session.items, jobs, owner.connection.studioId) } } };
  });
}
async function boundedJson(response: Response): Promise<unknown> {
  const reader = response.body?.getReader();
  if (!reader) throw new Error('媒体回执缺少响应正文');
  let bytes = 0; let text = ''; const decoder = new TextDecoder();
  try { while (true) { const part = await reader.read(); if (part.done) break;
    bytes += part.value.length; if (bytes > 4 * 1024 * 1024) throw new Error('媒体回执超过大小限制');
    text += decoder.decode(part.value, { stream: true });
  } text += decoder.decode(); return JSON.parse(text); }
  finally { await reader.cancel().catch(() => {}); reader.releaseLock(); }
}
/** 请求头和正文都归同一连接；不能在收到头部后就提前释放撤销和超时监听。 */
async function requestReceipt(owner: SessionConnectionOwner, path: string, options: RequestInit = {}): Promise<unknown> {
  const controller = new AbortController();
  const abort = () => controller.abort();
  const unsubscribe = subscribeSessionConnection(() => { if (!ownsSessionConnection(owner)) abort(); });
  const timer = setTimeout(abort, 30_000);
  if (options.signal?.aborted) abort();
  else options.signal?.addEventListener('abort', abort, { once: true });
  try {
    const response = await sessionOwnerFetch(owner, path, { ...options, signal: controller.signal });
    const value = await boundedJson(response);
    if (!ownsSessionConnection(owner)) throw new Error('连接已切换，已忽略旧媒体回执');
    return value;
  } finally {
    clearTimeout(timer); unsubscribe(); options.signal?.removeEventListener('abort', abort);
  }
}
export async function refreshRustMediaJobs(owner: SessionConnectionOwner, sessionId: string, signal?: AbortSignal): Promise<void> {
  if (!ownsSessionConnection(owner) || owner.connection.serverNodeKind !== 'lingxi-service') return;
  const body = row(await requestReceipt(owner, `/lingxi/v1/sessions/${encodeURIComponent(sessionId)}/media/jobs`, { signal }));
  if (!body || Object.keys(body).some(key => key !== 'jobs') || !Array.isArray(body.jobs) || body.jobs.length > 1024) throw new Error('媒体任务列表格式无效');
  applyRustMediaJobs(owner, sessionId, body.jobs);
}
export async function cancelRustMediaJob(sessionId: string, taskId: string): Promise<void> {
  const owner = captureSessionConnection();
  if (!owner || owner.connection.serverNodeKind !== 'lingxi-service') throw new Error('媒体任务连接不可用');
  const body = row(await requestReceipt(owner, `/lingxi/v1/sessions/${encodeURIComponent(sessionId)}/media/jobs/${encodeURIComponent(taskId)}/cancel`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' }));
  if (!body || Object.keys(body).some(key => key !== 'job')) throw new Error('媒体取消回执格式无效');
  applyRustMediaJobs(owner, sessionId, [body.job]);
}
export function startRustMediaPolling(connection: ServerConnection): () => void {
  const owner = captureSessionConnection();
  const identity = (value: ServerConnection) => JSON.stringify(value, (_key, part: unknown) => part && typeof part === 'object' && !Array.isArray(part)
    ? Object.fromEntries(Object.entries(part).sort(([a], [b]) => a.localeCompare(b))) : part);
  if (!owner || owner.connection.serverNodeKind !== 'lingxi-service' || identity(owner.connection) !== identity(connection)) return () => {};
  let stopped = false; let timer: ReturnType<typeof setTimeout> | null = null;
  const controller = new AbortController();
  const stop = () => { stopped = true; controller.abort(); if (timer) clearTimeout(timer); unsubscribe(); };
  const unsubscribe = useStore.subscribe(() => { if (!ownsSessionConnection(owner)) stop(); });
  const poll = async () => {
    if (stopped || !ownsSessionConnection(owner)) return;
    const sid = useStore.getState().currentSessionId;
    try { if (sid) await refreshRustMediaJobs(owner, sid, controller.signal); }
    catch (error) { if (!stopped && ownsSessionConnection(owner)) console.warn('[rust-media] 媒体任务读取失败，保留已知状态', error); }
    finally { if (!stopped && ownsSessionConnection(owner)) timer = setTimeout(() => { timer = null; void poll(); }, 5000); }
  };
  void poll(); return stop;
}
