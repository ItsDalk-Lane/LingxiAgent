// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { createServer, type ServerResponse } from 'node:http';
import { createHash } from 'node:crypto';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { MediaJobReceipt } from '../../../../../contracts/generated/ts/lingxi-protocol';
import { AssistantMessage } from '../../components/chat/AssistantMessage';
import { useStore } from '../../stores';
import { loadMessages } from '../../stores/session-actions';
import { sessionScopedValue } from '../../stores/session-slice';
import { captureSessionConnection, ownsSessionConnection } from '../../services/session-connection-owner';
import { cancelRustMediaJob, refreshRustMediaJobs as refreshFromService, startRustMediaPolling } from '../../services/rust-media';
import { resolveFileRefUrl } from '../../services/resource-url';
import { handleAppEvent } from '../../services/app-event-actions';
import { lingxiFetch } from '../../hooks/use-hana-fetch';
import { streamBufferManager } from '../../hooks/use-stream-buffer';
import type { ServerConnection } from '../../services/server-connection';

// 只替换外部 HTTP 响应；状态、历史、卡片渲染和文件打开均用正式实现。
function receipt(patch: Partial<MediaJobReceipt> = {}): MediaJobReceipt {
  return { taskId: 'media-task', kind: 'image', provider: 'fixture-provider', status: 'submitted', sourceRunId: 'run-media',
    executionRunId: null, revision: '9007199254740992', files: [], remoteOutcome: 'accepted', remoteCancel: null,
    localDeliveryStopped: false, message: null, errorCode: null, createdAtMs: '1800000000000', updatedAtMs: '1800000000001', ...patch };
}
// 完整1×1灰度透明PNG；本机已核IHDR、各chunk CRC及可解压的像素行。
const pixels = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=', 'base64');
const file = { fileId: 'sf_media_fixture', filename: '完成图.png', mime: 'image/png', sizeBytes: String(pixels.length) };
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(done => { resolve = done; }); return { resolve, promise }; }
const closers: Array<() => Promise<void>> = [];
const stops: Array<() => void> = [];
async function refreshRustMediaJobs(...args: Parameters<typeof refreshFromService>): Promise<void> {
  await act(async () => { await refreshFromService(...args); });
}

async function fixture() {
  let current: unknown = receipt();
  let cancelResult: MediaJobReceipt = receipt({ status: 'cancelled', revision: '9007199254740993', localDeliveryStopped: true, remoteCancel: 'unsupported' });
  let hold: 'jobs' | 'cancel' | null = null;
  let cancelError: string | null = null;
  let pending: ServerResponse | null = null;
  const started = deferred<void>(); const closed = deferred<void>();
  const calls: Array<{ path: string; method: string; body: string; auth: string | undefined }> = [];
  const initial = receipt();
  const server = createServer(async (request, response) => {
    const url = new URL(request.url!, 'http://127.0.0.1');
    let body = ''; for await (const part of request) body += part;
    calls.push({ path: request.url!, method: request.method!, body, auth: request.headers.authorization });
    if (request.headers.authorization !== 'Bearer media-owner' && url.searchParams.get('token') !== 'media-owner') {
      response.writeHead(401, { 'Content-Type': 'application/json' }); response.end('{"message":"authentication required"}'); return;
    }
    response.setHeader('Content-Type', 'application/json');
    if (url.pathname === '/lingxi/v1/sessions/s/history') {
      response.setHeader('ETag', 'history-media');
      response.end(JSON.stringify({ schemaVersion: 'lingxi.canonical-history.v1', sessionId: 's', revision: 'history-media', page: { hasMore: false, nextBefore: null }, items: [
        { kind: 'tool_call', toolCallId: 'tool-media', firstSeq: '1', run: { state: 'known', runId: 'run-media' }, target: 'tool:first-party:generate_image', status: 'success', result: { details: { mediaGeneration: { tasks: [initial] } } } },
        { kind: 'run_terminal', runId: 'run-media', status: 'completed', eventSeq: '2' },
      ] })); return;
    }
    if (url.pathname === '/lingxi/v1/events-fixture') { response.end(JSON.stringify({ type: 'media_generation_update', payload: { sessionId: 's', mediaGeneration: current } })); return; }
    if (url.pathname === '/lingxi/v1/stream-fixture') {
      const identity = { sessionPath: 'native:s', sessionId: 's', runId: 'run-media', streamId: 'stream-media' };
      response.end(JSON.stringify([
        { ...identity, type: 'assistant_run_start' },
        { ...identity, type: 'tool_start', id: 'live-tool-media', name: 'generate_image', args: { prompt: '本地图片检查' } },
        { ...identity, type: 'tool_end', id: 'live-tool-media', name: 'generate_image', success: true, details: { mediaGeneration: { tasks: [initial] } } },
      ])); return;
    }
    if (url.pathname === '/lingxi/v1/resources/res_sf_media_fixture/content') {
      response.setHeader('Content-Type', 'image/png'); response.end(pixels); return;
    }
    const isJobs = url.pathname === '/lingxi/v1/sessions/s/media/jobs';
    const isCancel = url.pathname === '/lingxi/v1/sessions/s/media/jobs/media-task/cancel';
    if (isCancel && cancelError) { response.statusCode = 403; response.end(JSON.stringify({ message: cancelError })); return; }
    if (isCancel) current = cancelResult;
    if (isJobs || isCancel) {
      if (hold === (isJobs ? 'jobs' : 'cancel')) {
        response.writeHead(200); response.write(isJobs ? '{"jobs":[' : '{"job":'); response.flushHeaders(); pending = response;
        response.once('close', () => closed.resolve()); started.resolve(); return;
      }
      response.end(JSON.stringify(isJobs ? { jobs: [current] } : { job: current })); return;
    }
    if (url.pathname.startsWith('/lingxi/v1/desk/beautify/')) { response.end(JSON.stringify({ ok: true, path: url.pathname, query: url.search, method: request.method, body })); return; }
    response.statusCode = 404; response.end('{"error":"unexpected fixture endpoint"}');
  });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const address = server.address(); if (!address || typeof address === 'string') throw Error('本地服务未启动');
  const connection: ServerConnection = { connectionId: 'media-a', kind: 'local', serverId: 'media-node', serverNodeKind: 'lingxi-service',
    studioId: 'studio', label: '媒体检查', baseUrl: `http://127.0.0.1:${address.port}`, wsUrl: `ws://127.0.0.1:${address.port}`,
    token: 'media-owner', authState: 'paired', trustState: 'local', credentialKind: 'loopback_token', capabilities: ['chat'] };
  closers.push(async () => { pending?.destroy(); server.closeAllConnections(); await new Promise<void>(resolve => server.close(() => resolve())); });
  return { connection, calls, started, closed, set: (value: unknown) => { current = value; }, cancel: (value: MediaJobReceipt) => { cancelResult = value; },
    refuseCancel: (message: string) => { cancelError = message; },
    hold: (kind: 'jobs' | 'cancel') => { hold = kind; }, release: () => { hold = null; pending?.end('}'); } };
}
function select(connection: ServerConnection) {
  useStore.setState({ serverConnections: { [connection.connectionId]: connection }, activeServerConnectionId: connection.connectionId, activeServerConnection: connection } as never);
  captureSessionConnection();
  useStore.setState({ currentSessionId: 's', currentSessionPath: 'native:s', sessions: [{ sessionId: 's', path: 'native:s' }], sessionLocatorsById: { s: { path: 'native:s' } } } as never);
}
function session() { const state = useStore.getState(); return sessionScopedValue(state, state.chatSessions, 'native:s')!; }
function Conversation({ readOnly = false }: { readOnly?: boolean } = {}) {
  const items = useStore(state => sessionScopedValue(state, state.chatSessions, 'native:s')?.items ?? []);
  return <>{items.map(item => item.type === 'message' && item.data.role === 'assistant' ? <AssistantMessage key={item.data.id} sessionPath="native:s" message={item.data}
    agentDisplay={{ id: 'a', displayName: '助手', avatarUrl: null, fallbackAvatar: null, yuan: 'lingxi', isUser: false }} readOnly={readOnly} isStreaming={false} isSelected={false} showAvatar={false} /> : null)}</>;
}
beforeEach(() => {
  streamBufferManager.clearAll();
  window.t = ((key: string) => key) as typeof window.t;
  useStore.setState({ serverPort: null, serverToken: null, serverConnections: {}, activeServerConnectionId: null, activeServerConnection: null,
    currentSessionId: null, currentSessionPath: null, chatSessions: {}, sessions: [], sessionLocatorsById: {}, sessionRegistryFilesByPath: {}, agents: [], agentName: '助手', agentYuan: 'lingxi',
    mediaViewer: null, streamingSessions: [], selectedMessageIdsBySession: {}, _loadMessagesVersion: {}, _messageLiveVersion: {} } as never);
});
afterEach(async () => { stops.splice(0).forEach(stop => stop()); cleanup(); streamBufferManager.clearAll(); for (const close of closers.splice(0)) await close(); });

describe('媒体真实 HTTP → 历史/状态 → 卡片和文件打开', () => {
  it('真实流式快照仍在时，完成回执立即替换卡片且下一段正文不能倒退它', async () => {
    const f = await fixture(); select(f.connection); useStore.getState().initSession('native:s', [], false);
    const response = await fetch(`${f.connection.baseUrl}/lingxi/v1/stream-fixture`, { headers: { Authorization: 'Bearer media-owner' } });
    for (const event of await response.json()) streamBufferManager.handle(event);
    render(<Conversation />); expect(screen.getByText('chat.media.phase.submitted')).toBeInTheDocument();
    f.set(receipt({ status: 'completed', revision: '9007199254740993', files: [file], remoteOutcome: 'completed' }));
    await refreshRustMediaJobs(captureSessionConnection()!, 's');
    expect(await screen.findByAltText('完成图.png')).toBeInTheDocument();
    expect(screen.queryByText('chat.media.phase.submitted')).not.toBeInTheDocument();
    await act(async () => {
      streamBufferManager.handle({ type: 'text_delta', sessionPath: 'native:s', sessionId: 's', runId: 'run-media', streamId: 'stream-media', delta: '继续说明已交付文件' });
      streamBufferManager.handle({ type: 'tool_start', sessionPath: 'native:s', sessionId: 's', runId: 'run-media', streamId: 'stream-media', id: 'following-read', name: 'read', args: { path: '后续说明' } });
    });
    expect(screen.getAllByAltText('完成图.png')).toHaveLength(1);
    expect(screen.queryByText('chat.media.phase.submitted')).not.toBeInTheDocument();
  });
  it('同一任务经历提交、生成、下载后替换为可打开文件，重载旧历史和旧回执不倒退', async () => {
    const f = await fixture(); select(f.connection); await loadMessages('native:s'); render(<Conversation />);
    expect(screen.getByText('chat.media.phase.submitted')).toBeInTheDocument();
    const owner = captureSessionConnection()!;
    for (const [status, revision] of [['generating','9007199254740993'], ['downloading','9007199254740994']] as const) {
      f.set(receipt({ status, revision, remoteOutcome: status === 'downloading' ? 'completed' : 'accepted' }));
      await refreshRustMediaJobs(owner, 's'); expect(screen.getByText(`chat.media.phase.${status}`)).toBeInTheDocument();
    }
    const complete = receipt({ status: 'completed', revision: '9007199254740995', files: [file], remoteOutcome: 'completed' });
    f.set(complete); await refreshRustMediaJobs(owner, 's');
    fireEvent.click(await screen.findByAltText('完成图.png'));
    expect(useStore.getState().mediaViewer?.files).toHaveLength(1);
    const selected = useStore.getState().mediaViewer!.files[0];
    const url = resolveFileRefUrl(selected, { connection: f.connection }).url;
    expect(url).toContain('/lingxi/v1/resources/res_sf_media_fixture/content');
    const binary = await fetch(url); expect(binary.status).toBe(200); expect(binary.headers.get('content-type')).toBe('image/png');
    const actualPixels = Buffer.from(await binary.arrayBuffer()); expect(actualPixels).toEqual(pixels); expect(actualPixels.length).toBe(68);
    expect(createHash('sha256').update(actualPixels).digest('hex')).toBe('431ced6916a2a21a156e38701afe55bbd7f88969fbbfc56d7fe099d47f265460');
    expect(screen.queryByText('chat.media.phase.downloading')).not.toBeInTheDocument();
    f.set(receipt()); await loadMessages('native:s');
    expect(session().nativeMediaJobs?.['media-task'].revision).toBe('9007199254740995');
    expect(screen.getAllByAltText('完成图.png')).toHaveLength(1);
    expect(f.calls.filter(call => call.path.includes('/media/jobs')).every(call => call.auth === 'Bearer media-owner')).toBe(true);
  });

  it('相同版本仅字段排列不同可以重放，版本 BigInt 不丢精度，真实来源变更整批拒绝', async () => {
    const f = await fixture(); select(f.connection); await loadMessages('native:s'); const owner = captureSessionConnection()!;
    const value = receipt({ revision: '9007199254740993', status: 'completed', files: [file], remoteOutcome: 'completed' });
    f.set(value); await refreshRustMediaJobs(owner, 's');
    f.set(Object.fromEntries(Object.entries({ ...value, files: [Object.fromEntries(Object.entries(file).reverse())] }).reverse()));
    await refreshRustMediaJobs(owner, 's');
    expect(session().nativeMediaJobs?.['media-task'].revision).toBe('9007199254740993');
    for (const patch of [{ sourceRunId: 'foreign-run' }, { provider: 'other-provider' }, { kind: 'video' }, { createdAtMs: '1800000000010' }]) {
      f.set({ ...value, revision: '9007199254740994', ...patch });
      await expect(refreshRustMediaJobs(owner, 's')).rejects.toThrow('来源发生冲突');
      expect(session().nativeMediaJobs?.['media-task']).toEqual(value);
    }
    f.set({ ...value, message: '相同版本不同内容' });
    await expect(refreshRustMediaJobs(owner, 's')).rejects.toThrow('相同版本内容冲突');
  });

  it('真实取消按钮只停止等待，保留已交付文件并区分远端不支持取消和未知', async () => {
    const f = await fixture(); select(f.connection);
    f.set(receipt({ status: 'downloading', files: [file], remoteOutcome: 'completed' })); await loadMessages('native:s'); render(<Conversation />);
    f.cancel(receipt({ status: 'cancelled', revision: '9007199254740993', files: [file], remoteOutcome: 'completed', remoteCancel: 'unsupported', localDeliveryStopped: true }));
    fireEvent.click(screen.getByRole('button', { name: 'chat.media.stopWaiting' }));
    await screen.findByText('chat.media.remoteCancel.unsupported');
    expect(screen.getByText('chat.media.phase.cancelled')).toBeInTheDocument();
    expect(screen.getByText('chat.media.localDeliveryStopped')).toBeInTheDocument();
    expect(screen.getByAltText('完成图.png')).toBeInTheDocument();
    expect(screen.queryByText('common.regenerate')).not.toBeInTheDocument();
    const posted = f.calls.filter(call => call.method === 'POST'); expect(posted).toHaveLength(1); expect(posted[0].body).toBe('{}');
    f.set(receipt({ status: 'unknown', revision: '9007199254740994', files: [file], remoteOutcome: 'unknown', remoteCancel: 'unknown', localDeliveryStopped: true }));
    const owner = captureSessionConnection()!;
    const response = await fetch(`${f.connection.baseUrl}/lingxi/v1/events-fixture`, { headers: { Authorization: 'Bearer media-owner' } });
    const event = await response.json(); await act(async () => { await handleAppEvent(event.type, event.payload, { isCurrent: () => ownsSessionConnection(owner) }); });
    expect(screen.getByText('chat.media.remoteOutcome.unknown')).toBeInTheDocument();
    expect(screen.getByText('chat.media.remoteCancel.unknown')).toBeInTheDocument();
    expect(screen.getAllByAltText('完成图.png')).toHaveLength(1);
    expect(screen.queryByRole('button', { name: 'chat.media.stopWaiting' })).not.toBeInTheDocument();
  });

  it('远端明确失败与本地交付失败均保留真实事实，不标成未知或远端生成失败', async () => {
    const f = await fixture(); select(f.connection); await loadMessages('native:s'); render(<Conversation />);
    f.set(receipt({ status: 'failed', revision: '9007199254740993', remoteOutcome: 'failed' as MediaJobReceipt['remoteOutcome'], errorCode: 'PROVIDER_FAILED', message: '供应商明确失败' }));
    await refreshRustMediaJobs(captureSessionConnection()!, 's');
    expect(screen.getByText('chat.media.phase.failed')).toBeInTheDocument(); expect(screen.getByText('chat.media.remoteOutcome.failed')).toBeInTheDocument();
    expect(screen.queryByText('chat.media.phase.unknown')).not.toBeInTheDocument();
    f.set(receipt({ status: 'failed', revision: '9007199254740994', remoteOutcome: 'completed', files: [file], message: '后续下载失败' }));
    await refreshRustMediaJobs(captureSessionConnection()!, 's');
    expect(screen.getByText('chat.media.taskFailed')).toBeInTheDocument(); expect(screen.getByText('chat.media.remoteOutcome.completed')).toBeInTheDocument();
    expect(screen.getByAltText('完成图.png')).toBeInTheDocument();
  });

  it('取消被拒绝时保留生成中事实与可见原因，只读卡片不产生取消请求', async () => {
    const f = await fixture(); select(f.connection); await loadMessages('native:s'); f.refuseCancel('该任务当前没有撤销权限'); render(<Conversation />);
    fireEvent.click(screen.getByRole('button', { name: 'chat.media.stopWaiting' }));
    await screen.findByText('该任务当前没有撤销权限');
    expect(screen.getByText('chat.media.phase.submitted')).toBeInTheDocument();
    expect(session().nativeMediaJobs?.['media-task'].status).toBe('submitted');
    expect(f.calls.filter(call => call.method === 'POST')).toHaveLength(1);
    cleanup(); render(<Conversation readOnly />);
    expect(screen.queryByRole('button', { name: 'chat.media.stopWaiting' })).not.toBeInTheDocument();
    expect(f.calls.filter(call => call.method === 'POST')).toHaveLength(1);
  });

  it('损坏或超限回执拒绝更新，保留实际已知任务', async () => {
    const f = await fixture(); select(f.connection); await loadMessages('native:s'); const owner = captureSessionConnection()!;
    const known = structuredClone(session().nativeMediaJobs);
    for (const value of [
      { ...receipt(), revision: 9007199254740993 }, { ...receipt(), revision: '18446744073709551616' },
      { ...receipt(), status: 'completed', files: [] }, { ...receipt(), files: [file, file] },
      { ...receipt(), files: [{ ...file, sizeBytes: 8 }] }, { ...receipt(), trustedOwner: 'forged' },
    ]) {
      f.set(value); await expect(refreshRustMediaJobs(owner, 's')).rejects.toThrow(); expect(session().nativeMediaJobs).toEqual(known);
    }
    f.set({ ...receipt(), message: 'x'.repeat(4 * 1024 * 1024) });
    await expect(refreshRustMediaJobs(owner, 's')).rejects.toThrow('超过大小限制');
    expect(session().nativeMediaJobs).toEqual(known);
  });

  it('连接失效时中断已收到响应头的正文流，不接收旧结果或继续轮询', async () => {
    const f = await fixture(); select(f.connection); useStore.getState().initSession('native:s', [], false);
    f.hold('jobs'); const reordered = Object.fromEntries(Object.entries(f.connection).reverse()) as unknown as ServerConnection;
    const stop = startRustMediaPolling(reordered); stops.push(stop); await f.started.promise;
    select({ ...f.connection, connectionId: 'media-b', serverId: 'other-node' });
    await f.closed.promise;
    expect(session()?.nativeMediaJobs).toBeUndefined();
    expect(f.calls.filter(call => call.path.includes('/media/jobs'))).toHaveLength(1);
  });

  it('取消响应晚到时离开再返回同连接也不复活旧结果，重连读取同任务不再提交', async () => {
    const f = await fixture(); select(f.connection); await loadMessages('native:s'); f.hold('cancel');
    const cancellation = cancelRustMediaJob('s', 'media-task'); const rejected = expect(cancellation).rejects.toThrow();
    await f.started.promise; select({ ...f.connection, connectionId: 'media-b' }); select(f.connection);
    await rejected; await f.closed.promise; f.release();
    await loadMessages('native:s');
    expect(session().nativeMediaJobs?.['media-task'].status).toBe('cancelled');
    expect(f.calls.filter(call => call.method === 'POST')).toHaveLength(1);
    expect(Object.keys(session().nativeMediaJobs ?? {})).toEqual(['media-task']);
  });

  it('美化四个现役入口沿正式HTTP映射，保留方法、正文、查询及认证', async () => {
    const f = await fixture(); select(f.connection);
    for (const [path, method] of [['status','GET'],['cover','POST'],['cover/apply','POST'],['cover/preset/apply','POST']] as const) {
      const response = await lingxiFetch(`/api/desk/beautify/${path}?agentId=a`, { method, ...(method === 'POST' ? { body: '{"sessionId":"s","prompt":"封面"}' } : {}) });
      const result = await response.json(); expect(result.path).toBe(`/lingxi/v1/desk/beautify/${path}`); expect(result.query).toBe('?agentId=a'); expect(result.method).toBe(method);
      if (method === 'POST') expect(result.body).toBe('{"sessionId":"s","prompt":"封面"}');
    }
    expect(f.calls.every(call => call.auth === 'Bearer media-owner')).toBe(true);
  });
});
