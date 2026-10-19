import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

const routing = vi.hoisted(() => ({ nodeKind: undefined as string | undefined }));

// Mock useStore before importing lingxiFetch
vi.mock('../../stores', () => ({
  useStore: {
    getState: () => ({
      serverPort: '3210',
      serverToken: 'test-token-123',
      activeServerConnection: {
        kind: 'local',
        serverNodeKind: routing.nodeKind,
        serverId: 'local',
        studioId: 'local',
        label: 'Local Hana',
        baseUrl: 'http://127.0.0.1:3210',
        wsUrl: 'ws://127.0.0.1:3210',
        token: 'test-token-123',
        authState: 'paired',
        trustState: 'local',
        credentialKind: 'loopback_token',
        capabilities: ['chat', 'resources', 'tools'],
      },
    }),
  },
}));

import { lingxiUrl, lingxiFetch } from '../../hooks/use-hana-fetch';

describe('lingxiUrl', () => {
  it('构建带 token 的 URL', () => {
    const url = lingxiUrl('/api/health');
    expect(url).toBe('http://127.0.0.1:3210/api/health?token=test-token-123');
  });

  it('路径已有 query param 时用 & 连接', () => {
    const url = lingxiUrl('/api/sessions?limit=10');
    expect(url).toBe('http://127.0.0.1:3210/api/sessions?limit=10&token=test-token-123');
  });
});

describe('lingxiFetch', () => {
  const mockFetch = vi.fn();

  beforeEach(() => {
    mockFetch.mockReset();
    vi.stubGlobal('fetch', mockFetch);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('发送带 Authorization header 的请求', async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: async () => ({ status: 'ok' }),
    });

    await lingxiFetch('/api/health');

    expect(mockFetch).toHaveBeenCalledOnce();
    const [url, opts] = mockFetch.mock.calls[0];
    expect(url).toBe('http://127.0.0.1:3210/api/health');
    expect(opts.headers.Authorization).toBe('Bearer test-token-123');
  });

  it('非 2xx 状态码抛出错误', async () => {
    mockFetch.mockResolvedValueOnce({
      ok: false,
      status: 404,
      statusText: 'Not Found',
    });

    await expect(lingxiFetch('/api/missing')).rejects.toThrow('404');
  });

  it('允许调用方显式读取非 2xx 响应体', async () => {
    const response = {
      ok: false,
      status: 400,
      statusText: 'Bad Request',
      json: async () => ({ error: '日记材料准备失败: simulated summary failure' }),
    };
    mockFetch.mockResolvedValueOnce(response);

    const res = await lingxiFetch('/api/diary/write', { throwOnHttpError: false });

    expect(res).toBe(response);
    expect(mockFetch.mock.calls[0][1]).not.toHaveProperty('throwOnHttpError');
  });

  it('传递自定义 method 和 headers', async () => {
    mockFetch.mockResolvedValueOnce({ ok: true });

    await lingxiFetch('/api/test', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
    });

    const call = mockFetch.mock.calls[0];
    const url = call[0];
    const opts = call[1];
    expect(url).toBe('http://127.0.0.1:3210/api/test');
    // headers 被合并，Authorization 被注入
    expect(opts.headers).toHaveProperty('Authorization', 'Bearer test-token-123');
  });

  it('传递 AbortSignal 用于超时', async () => {
    mockFetch.mockResolvedValueOnce({ ok: true });

    await lingxiFetch('/api/test');

    const [, opts] = mockFetch.mock.calls[0];
    expect(opts.signal).toBeInstanceOf(AbortSignal);
  });
});


describe('Rust 频道与协作正式路由', () => {
  const fetchMock = vi.fn();
  beforeEach(() => {
    routing.nodeKind = 'lingxi-service';
    fetchMock.mockReset().mockResolvedValue({ ok: true });
    vi.stubGlobal('fetch', fetchMock);
  });
  afterEach(() => { routing.nodeKind = undefined; vi.unstubAllGlobals(); });

  it.each([
    '/api/channels', '/api/channels?limit=10', '/api/channels/toggle',
    '/api/channels/ch%2Fone', '/api/channels/ch1/messages', '/api/channels/ch1/read',
    '/api/channels/ch1/members', '/api/channels/ch1/members/agent%20one',
    '/api/dm', '/api/dm/bob?agentId=alice',
    '/api/conversations/dm%3Abob/agent-phone-settings?agentId=alice',
    '/api/conversations/ch1/agent-phone-tool-mode', '/api/conversations/ch1/agent-activities',
    '/api/conversations/dm%3Abob/export?agentId=alice',
    '/api/session-collab/apply', '/api/session-collab/reject',
    '/api/desk/beautify/status?agentId=alice', '/api/desk/beautify/cover',
    '/api/desk/beautify/cover/apply', '/api/desk/beautify/cover/preset/apply',
    '/api/media/generate', '/api/media/image/generate', '/api/media/speech/generate',
    '/api/media/tasks?sessionId=native%3Aabc', '/api/media/tasks/task-1',
    '/api/media/tasks/batch/batch-1', '/api/media/tasks/task-1/retry',
    '/api/media/generated/open/image-abc.png', '/api/media/generated/image-abc.png',
    '/api/media/image/providers', '/api/media/video/config', '/api/media/speech/models',
    '/api/media/speech/models/model-1', '/api/speech-recognition/providers',
    '/api/speech-recognition/config', '/api/speech-recognition/models',
    '/api/speech-recognition/models/whisper-1',
  ])('旧页面%s实际请求Rust路径并保留参数', async path => {
    await lingxiFetch(path, { method: 'POST', body: JSON.stringify({ example: '原参数' }) });
    expect(fetchMock).toHaveBeenCalledWith(`http://127.0.0.1:3210/lingxi/v1${path.slice(4)}`, expect.objectContaining({
      method: 'POST', body: JSON.stringify({ example: '原参数' }),
      headers: expect.objectContaining({ Authorization: 'Bearer test-token-123' }),
    }));
    expect(lingxiUrl(path)).toContain(`/lingxi/v1${path.slice(4)}`);
  });

  it('不为尚未实现的相似路由猜测Rust入口', async () => {
    await expect(lingxiFetch('/api/conversations/ch1/activities')).rejects.toThrow(/no migrated endpoint/);
    await expect(lingxiFetch('/api/session-collab/create')).rejects.toThrow(/no migrated endpoint/);
    // 前缀相近但不是媒体/语音识别正式面的路径不得被前缀规则误吞。
    await expect(lingxiFetch('/api/mediax')).rejects.toThrow(/no migrated endpoint/);
    await expect(lingxiFetch('/api/speech-recognitionx/config')).rejects.toThrow(/no migrated endpoint/);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('Node连接保持现役路径', async () => {
    routing.nodeKind = undefined;
    await lingxiFetch('/api/channels/ch1/messages');
    expect(fetchMock.mock.calls[0][0]).toBe('http://127.0.0.1:3210/api/channels/ch1/messages');
  });
});
