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

  it('旧确认链在 Rust 连接下显式拒绝，不猜测入口', async () => {
    // T01 裁决（见 docs/rust-tauri/R08/T01_SCOPING.md §4(b)1 更正）：旧
    // confirmStore 流在 Rust 侧无生产者（Rust 事件/历史不产生 confirmation
    // 块）；工具批准走 approvals 面板、ask-user 走 interactions、跨会话投递
    // 走 session-collab apply/reject、定时建议走 apply_suggestion。旧路径如
    // 被触达必须显式能力错误，不静默降级、不建第二确认裁决。
    await expect(lingxiFetch('/api/confirm/c-1', { method: 'POST', body: JSON.stringify({ action: 'confirmed' }) })).rejects.toThrow(/no migrated endpoint/);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('Node连接保持现役路径', async () => {
    routing.nodeKind = undefined;
    await lingxiFetch('/api/channels/ch1/messages');
    expect(fetchMock.mock.calls[0][0]).toBe('http://127.0.0.1:3210/api/channels/ch1/messages');
  });
});

describe('Rust 知识库正式路由', () => {
  const fetchMock = vi.fn();
  beforeEach(() => {
    routing.nodeKind = 'lingxi-service';
    fetchMock.mockReset().mockResolvedValue({ ok: true });
    vi.stubGlobal('fetch', fetchMock);
  });
  afterEach(() => { routing.nodeKind = undefined; vi.unstubAllGlobals(); });

  // 路径集合来自 knowledge.rs knowledge_routes() 与 knowledge-api.ts 的逐条同构面。
  it.each([
    '/api/knowledge/notebooks', '/api/knowledge/notebooks?includeArchived=1',
    '/api/knowledge/notebooks/nb-1', '/api/knowledge/notebooks/nb-1/settings',
    '/api/knowledge/notebooks/nb-1/import-directory',
    '/api/knowledge/notebooks/nb-1/sources',
    '/api/knowledge/notebooks/nb-1/sources/src-1',
    '/api/knowledge/notebooks/nb-1/sources/src-1/refresh',
    '/api/knowledge/notebooks/nb-1/sources/src-1/reingest',
    '/api/knowledge/ingestion', '/api/knowledge/parse-artifacts/pa-1/blocks',
    '/api/knowledge/parse-artifacts/pa-1/chunks',
    '/api/knowledge/snapshots/snap-1/content',
    '/api/knowledge/citations/cit-1',
  ])('旧页面%s实际请求Rust路径并保留参数', async path => {
    await lingxiFetch(path, { method: 'GET' });
    expect(fetchMock).toHaveBeenCalledWith(`http://127.0.0.1:3210/lingxi/v1${path.slice(4)}`, expect.objectContaining({
      method: 'GET',
      headers: expect.objectContaining({ Authorization: 'Bearer test-token-123' }),
    }));
    expect(lingxiUrl(path)).toContain(`/lingxi/v1${path.slice(4)}`);
  });

  it('不误吞前缀相近路径', async () => {
    await expect(lingxiFetch('/api/knowledgex')).rejects.toThrow(/no migrated endpoint/);
    await expect(lingxiFetch('/api/knowledgebase/notebooks')).rejects.toThrow(/no migrated endpoint/);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('Node连接保持现役路径', async () => {
    routing.nodeKind = undefined;
    await lingxiFetch('/api/knowledge/notebooks');
    expect(fetchMock.mock.calls[0][0]).toBe('http://127.0.0.1:3210/api/knowledge/notebooks');
  });
});

describe('Rust 文件历史正式路由', () => {
  const fetchMock = vi.fn();
  beforeEach(() => {
    routing.nodeKind = 'lingxi-service';
    fetchMock.mockReset().mockResolvedValue({ ok: true });
    vi.stubGlobal('fetch', fetchMock);
  });
  afterEach(() => { routing.nodeKind = undefined; vi.unstubAllGlobals(); });

  // 四条与 Rust filehistory 路由（lib.rs file-history 面）逐条同构。
  it.each([
    '/api/file-history/files?agentId=a',
    '/api/file-history/versions?agentId=a&relPath=notes.md',
    '/api/file-history/snapshot?agentId=a&id=snap-1',
    '/api/file-history/restore',
  ])('旧页面%s实际请求Rust路径并保留参数', async path => {
    await lingxiFetch(path, { method: path.endsWith('/restore') ? 'POST' : 'GET' });
    expect(fetchMock).toHaveBeenCalledWith(`http://127.0.0.1:3210/lingxi/v1${path.slice(4)}`, expect.objectContaining({
      headers: expect.objectContaining({ Authorization: 'Bearer test-token-123' }),
    }));
    expect(lingxiUrl(path)).toContain(`/lingxi/v1${path.slice(4)}`);
  });

  it('不误吞前缀相近路径', async () => {
    await expect(lingxiFetch('/api/file-historyx/files')).rejects.toThrow(/no migrated endpoint/);
    expect(fetchMock).not.toHaveBeenCalled();
  });
});

describe('Rust 会话项目正式路由', () => {
  const fetchMock = vi.fn();
  beforeEach(() => {
    routing.nodeKind = 'lingxi-service';
    fetchMock.mockReset().mockResolvedValue({ ok: true });
    vi.stubGlobal('fetch', fetchMock);
  });
  afterEach(() => { routing.nodeKind = undefined; vi.unstubAllGlobals(); });

  // 与 Rust projects 路由（lib.rs session-projects 面）同构的现役调用；
  // session-assignment 为 R08-T01 补齐的旧契约端口（真实服务测试见
  // r06_session_project_catalog::session_assignment_assigns_clears_and_rejects_unknown）。
  it.each([
    '/api/session-projects',
    '/api/session-projects/projects',
    '/api/session-projects/projects/reorder',
    '/api/session-projects/projects/pj-1',
    '/api/session-projects/folders/fd-1',
    '/api/session-projects/folders/reorder',
    '/api/session-projects/session-assignment',
  ])('旧页面%s实际请求Rust路径并保留参数', async path => {
    await lingxiFetch(path, { method: 'POST' });
    expect(fetchMock).toHaveBeenCalledWith(`http://127.0.0.1:3210/lingxi/v1${path.slice(4)}`, expect.objectContaining({
      method: 'POST',
      headers: expect.objectContaining({ Authorization: 'Bearer test-token-123' }),
    }));
    expect(lingxiUrl(path)).toContain(`/lingxi/v1${path.slice(4)}`);
  });

  it('不误吞前缀相近路径', async () => {
    await expect(lingxiFetch('/api/session-projectsx')).rejects.toThrow(/no migrated endpoint/);
    expect(fetchMock).not.toHaveBeenCalled();
  });
});

describe('Rust 逐项补接的直连缺口（desk 技能/预览/检查点/权限默认/会话检索/思维档）', () => {
  const fetchMock = vi.fn();
  beforeEach(() => {
    routing.nodeKind = 'lingxi-service';
    fetchMock.mockReset().mockResolvedValue({ ok: true });
    vi.stubGlobal('fetch', fetchMock);
  });
  afterEach(() => { routing.nodeKind = undefined; vi.unstubAllGlobals(); });

  it.each([
    '/api/desk/skills?agentId=a',
    '/api/desk/install-skill',
    '/api/desk/delete-skill',
    '/api/preview/html',
    '/api/checkpoints/user-edit',
    '/api/preferences/session-permission-default',
    '/api/sessions/search?q=x',
    '/api/sessions/find?text=y',
    '/api/sessions/new-detached',
    '/api/session-thinking-level',
    '/api/session-permission-mode',
    '/api/preferences/appearance',
    '/api/preferences/workspace-ui-state?workspace=w&surface=electron',
    '/api/system/env-deps?agentId=a',
    '/api/system/env-deps/refresh?agentId=a',
  ])('旧页面%s实际请求Rust同构路径', async path => {
    await lingxiFetch(path, { method: 'GET' });
    expect(fetchMock).toHaveBeenCalledWith(`http://127.0.0.1:3210/lingxi/v1${path.slice(4)}`, expect.objectContaining({
      headers: expect.objectContaining({ Authorization: 'Bearer test-token-123' }),
    }));
    expect(lingxiUrl(path)).toContain(`/lingxi/v1${path.slice(4)}`);
  });

  it('不误吞会话域其余旧路径（messages/summary 等形状不同，待逐项改写）', async () => {
    await expect(lingxiFetch('/api/sessions/messages')).rejects.toThrow(/no migrated endpoint/);
    await expect(lingxiFetch('/api/sessions/searchx')).rejects.toThrow(/no migrated endpoint/);
    await expect(lingxiFetch('/api/session-permission-modex')).rejects.toThrow(/no migrated endpoint/);
    await expect(lingxiFetch('/api/preferences/appearancex')).rejects.toThrow(/no migrated endpoint/);
    await expect(lingxiFetch('/api/preferences/workspace-ui-statex')).rejects.toThrow(/no migrated endpoint/);
    await expect(lingxiFetch('/api/system/env-depsx')).rejects.toThrow(/no migrated endpoint/);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  // R08-T01 env-deps：现役消费者 EnvDepsTab.tsx 的
  // GET /api/system/env-deps?agentId=... 与 POST /api/system/env-deps/refresh
  // ?agentId=... 原样转发 Rust；协议转换仅路径前缀，query 逐字节保留。
  it('旧页面 POST /api/system/env-deps/refresh 实际请求Rust路径且参数保留', async () => {
    await lingxiFetch('/api/system/env-deps/refresh?agentId=alpha', { method: 'POST' });
    expect(fetchMock).toHaveBeenCalledWith(
      'http://127.0.0.1:3210/lingxi/v1/system/env-deps/refresh?agentId=alpha',
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ Authorization: 'Bearer test-token-123' }),
      }),
    );
  });

  it('env-deps 在 Node 连接保持现役路径', async () => {
    routing.nodeKind = undefined;
    await lingxiFetch('/api/system/env-deps?agentId=alpha');
    expect(fetchMock.mock.calls[0][0]).toBe('http://127.0.0.1:3210/api/system/env-deps?agentId=alpha');
  });

  // R08-T01 appearance：现役消费者 appearance-sync.ts 的 PUT（body 即偏好
  // 对象本身，无 wrapper）原样转发 Rust；GET 亦走同一映射（见上表）。
  it('旧页面 PUT /api/preferences/appearance 实际请求Rust路径且body不变', async () => {
    const body = { theme: 'hana-ink', serif: false, paperTexture: true };
    await lingxiFetch('/api/preferences/appearance', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    expect(fetchMock).toHaveBeenCalledWith('http://127.0.0.1:3210/lingxi/v1/preferences/appearance', expect.objectContaining({
      method: 'PUT',
      body: JSON.stringify(body),
      headers: expect.objectContaining({ Authorization: 'Bearer test-token-123' }),
    }));
  });

  // R08-T01 workspace-ui-state：现役消费者 workspace-ui-state-sync.ts 的
  // GET（workspace+surface query）与 PUT（body 即 {workspace,surface,state}）
  // 原样转发 Rust；协议转换仅路径前缀，query 与 body 逐字节保留。
  it('旧页面 GET/PUT /api/preferences/workspace-ui-state 实际请求Rust路径且body不变', async () => {
    const query = `workspace=${encodeURIComponent('/repo/a')}&surface=pwa`;
    await lingxiFetch(`/api/preferences/workspace-ui-state?${query}`, { method: 'GET' });
    expect(fetchMock).toHaveBeenCalledWith(
      `http://127.0.0.1:3210/lingxi/v1/preferences/workspace-ui-state?${query}`,
      expect.objectContaining({
        method: 'GET',
        headers: expect.objectContaining({ Authorization: 'Bearer test-token-123' }),
      }),
    );
    const body = { workspace: '/repo/a', surface: 'pwa', state: { previewOpen: true, openTabs: ['t1'] } };
    await lingxiFetch('/api/preferences/workspace-ui-state', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    expect(fetchMock).toHaveBeenCalledWith('http://127.0.0.1:3210/lingxi/v1/preferences/workspace-ui-state', expect.objectContaining({
      method: 'PUT',
      body: JSON.stringify(body),
      headers: expect.objectContaining({ Authorization: 'Bearer test-token-123' }),
    }));
  });

  // R08-T01 session-permission-mode：现役 POST 消费者（PlanModeButton /
  // SessionConfirmationPrompt / composer-permission-mode）四分支契约
  // currentSessionOnly / pendingNewSession / sessionPath / 默认均按原样
  // 转发到 Rust 组合状态端口，body 逐字节保留（协议转换仅路径前缀）。
  it.each([
    { mode: 'operate', currentSessionOnly: true },
    { mode: 'ask', pendingNewSession: true },
    { mode: 'read_only', sessionPath: 'native:s-1' },
    { mode: 'auto' },
  ])('旧页面 POST /api/session-permission-mode 分支%#实际请求Rust路径且body不变', async body => {
    await lingxiFetch('/api/session-permission-mode', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    expect(fetchMock).toHaveBeenCalledWith('http://127.0.0.1:3210/lingxi/v1/session-permission-mode', expect.objectContaining({
      method: 'POST',
      body: JSON.stringify(body),
      headers: expect.objectContaining({ Authorization: 'Bearer test-token-123' }),
    }));
  });
});

describe('Rust 会话域旧 query 寻址的协议翻译', () => {
  const fetchMock = vi.fn();
  beforeEach(() => {
    routing.nodeKind = 'lingxi-service';
    fetchMock.mockReset().mockResolvedValue({ ok: true });
    vi.stubGlobal('fetch', fetchMock);
  });
  afterEach(() => { routing.nodeKind = undefined; vi.unstubAllGlobals(); });

  it('读端口按 native 定位改写为 /sessions/{id} 子路由并保留其余参数', async () => {
    await lingxiFetch(`/api/sessions/summary?path=${encodeURIComponent('native:s-1')}&fresh=1`);
    expect(fetchMock.mock.calls[0][0]).toBe('http://127.0.0.1:3210/lingxi/v1/sessions/s-1/summary?fresh=1');
    await lingxiFetch(`/api/sessions/history-overview?path=${encodeURIComponent('native:s-1')}`);
    expect(fetchMock.mock.calls[1][0]).toBe('http://127.0.0.1:3210/lingxi/v1/sessions/s-1/history-overview');
    await lingxiFetch(`/api/sessions/content/${encodeURIComponent('rc-1')}?path=${encodeURIComponent('native:s-1')}`);
    expect(fetchMock.mock.calls[2][0]).toBe('http://127.0.0.1:3210/lingxi/v1/sessions/s-1/content/rc-1');
    await lingxiFetch(`/api/sessions/prompt-snapshot?sessionId=s-2`);
    expect(fetchMock.mock.calls[3][0]).toBe('http://127.0.0.1:3210/lingxi/v1/sessions/s-2/prompt-snapshot');
  });

  it('写端口剥离旧 body.path 后走 Rust 子路由', async () => {
    await lingxiFetch('/api/sessions/memory', {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path: 'native:s-1', memoryEnabled: false }),
    });
    expect(fetchMock.mock.calls[0][0]).toBe('http://127.0.0.1:3210/lingxi/v1/sessions/s-1/memory');
    expect(fetchMock.mock.calls[0][1].body).toBe(JSON.stringify({ memoryEnabled: false }));
    await lingxiFetch('/api/sessions/authorized-folders', {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path: 'native:s-1', action: 'add', folder: '/tmp/x' }),
    });
    expect(fetchMock.mock.calls[1][0]).toBe('http://127.0.0.1:3210/lingxi/v1/sessions/s-1/authorized-folders');
    expect(fetchMock.mock.calls[1][1].body).toBe(JSON.stringify({ action: 'add', folder: '/tmp/x' }));
  });

  it('旧 JSONL 路径不猜映射，触达即显式拒绝', async () => {
    await expect(lingxiFetch(`/api/sessions/summary?path=${encodeURIComponent('/Users/x/sessions/a.json')}`))
      .rejects.toThrow(/no migrated endpoint/);
    await expect(lingxiFetch('/api/sessions/memory', {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path: '/Users/x/sessions/a.json', memoryEnabled: true }),
    })).rejects.toThrow(/no migrated endpoint/);
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
