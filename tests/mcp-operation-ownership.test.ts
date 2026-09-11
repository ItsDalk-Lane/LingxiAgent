import { afterEach, describe, expect, it, vi } from 'vitest';
import { McpManager } from '../core/mcp/manager.ts';
import { McpHttpError } from '../core/mcp/clients/http-client.ts';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function fixture(lifecycle = 'keep-alive') {
  let config: any = { enabled: true, connectors: [{ id: 'local', name: 'Local', command: 'unused', lifecycle, idleTimeoutMinutes: 1, tools: [{ name: 'lookup' }] }] };
  const clients: any[] = [];
  const starts: ReturnType<typeof deferred<void>>[] = [];
  const runtime = new McpManager({ dataDir: '/unused-memory-fixture', log: { info() {}, warn() {}, error() {} } }, {
    configStore: { get: () => config, set: (_key: string, value: any) => { config = { ...config, ...value }; } },
    clientFactory: () => {
      const index = clients.length;
      const client: any = {
        running: false,
        start: vi.fn(async () => { if (starts[index]) await starts[index].promise; client.running = true; }),
        stop: vi.fn(async () => { client.running = false; }),
        listTools: vi.fn(async () => [{ name: 'lookup', annotations: { readOnlyHint: index === 0 } }]),
        callTool: vi.fn(async () => ({ content: [{ type: 'text', text: 'ok' }] })),
      };
      clients.push(client);
      return client;
    },
  });
  return { runtime, clients, starts };
}

afterEach(() => vi.useRealTimers());

describe('MCP 操作归属', () => {
  it('旧启动失败的异步清理不能覆盖后来失败实例的认证状态', async () => {
    const { runtime, clients, starts } = fixture();
    starts.push(deferred<void>(), deferred<void>());
    const cleanupEntered = deferred<void>();
    const finishCleanup = deferred<void>();
    const old = runtime.startConnector('local').catch(error => error);
    clients[0].stop.mockImplementationOnce(async () => {
      cleanupEntered.resolve();
      await finishCleanup.promise;
    });
    starts[0].reject(new Error('旧连接启动失败'));
    await cleanupEntered.promise;
    // A 已从 clients 删除，但仍在执行异步清理；用户开启独立的 B 尝试。
    await runtime.stopConnector('local');
    const next = runtime.startConnector('local').catch(error => error);
    starts[1].reject(new McpHttpError('新连接必须重新认证', { status: 401 }));
    await next;
    const before = runtime.getState().connectors[0].status;
    finishCleanup.resolve();
    await old;
    const after = runtime.getState().connectors[0].status;
    await runtime.dispose();
    expect(before).toBe('needs-auth');
    expect(after).toBe('needs-auth');
  });

  it('连接等待期间撤销该助手的工具使用权后不执行工具', async () => {
    const { runtime, starts, clients } = fixture();
    let enabled = true;
    runtime._bus = { request: async () => ({ config: {
      mcp: { connectors: { local: { enabled: true, tools: { lookup: enabled } } } },
    } }) };
    starts.push(deferred<void>());
    const pending = runtime.callTool('local', 'lookup', {}, { agentId: 'hana' }).then(() => null, error => error);
    enabled = false;
    starts[0].resolve();
    const error = await pending;
    await runtime.dispose();
    expect(error).toBeInstanceOf(Error);
    expect(clients[0].callTool).not.toHaveBeenCalled();
  });

  it('正在启动且允许使用的连接仍可等待本次连接完成', async () => {
    const { runtime, starts } = fixture();
    starts.push(deferred<void>());
    const pending = runtime.startConnector('local');
    const eligibility = runtime.evaluateToolEligibility('local', 'lookup', {
      mcp: { connectors: { local: { enabled: true, tools: { lookup: true } } } },
    });
    starts[0].resolve();
    await pending;
    await runtime.dispose();
    expect(eligibility.eligible).toBe(true);
  });

  it('手动启动与按需启动共享同一连接尝试', async () => {
    const { runtime, clients, starts } = fixture();
    starts.push(deferred<void>(), deferred<void>());
    const manual = runtime.startConnector('local');
    const call = runtime.callTool('local', 'lookup', {});
    const count = clients.length;
    starts.forEach(start => start.resolve());
    await Promise.all([manual, call]);
    await runtime.dispose();
    expect(count).toBe(1);
  });

  it('旧启动失败不能删除停机后建立的新连接', async () => {
    const { runtime, clients, starts } = fixture();
    starts.push(deferred<void>());
    const old = runtime.startConnector('local').catch(error => error);
    await runtime.stopConnector('local');
    await runtime.startConnector('local');
    starts[0].reject(new Error('旧启动失败'));
    await old;
    const owned = runtime.clients.get('local') === clients[1];
    const status = runtime.getState().connectors[0].status;
    await runtime.dispose();
    await clients[1].stop();
    expect(owned).toBe(true);
    expect(status).toBe('running');
  });

  it('旧目录刷新不能覆盖新连接目录及权限注解', async () => {
    const { runtime, clients } = fixture();
    await runtime.startConnector('local');
    const listing = deferred<any[]>();
    clients[0].listTools.mockImplementationOnce(() => listing.promise);
    const old = runtime.refreshTools('local').then(() => null, error => error);
    await runtime.stopConnector('local');
    await runtime.startConnector('local');
    listing.resolve([{ name: 'stale', annotations: { readOnlyHint: true } }]);
    const error = await old;
    const tools = runtime.getConfig().connectors[0].tools.map(tool => tool.name);
    const annotations = runtime.getRuntimeToolAnnotations('local', 'lookup');
    await runtime.dispose();
    expect(error).toBeInstanceOf(Error);
    expect(tools).toEqual(['lookup']);
    expect(annotations).toEqual({ readOnlyHint: false });
  });

  it('目录刷新等待期间不能空闲停泊', async () => {
    vi.useFakeTimers();
    const { runtime, clients } = fixture('lazy');
    await runtime.startConnector('local');
    const listing = deferred<any[]>();
    clients[0].listTools.mockImplementationOnce(() => listing.promise);
    const pending = runtime.refreshTools('local');
    await vi.advanceTimersByTimeAsync(61_000);
    const running = clients[0].running;
    listing.resolve([{ name: 'lookup' }]);
    await pending;
    await runtime.dispose();
    expect(running).toBe(true);
  });

  it('停用后的迟到目录结果不能恢复旧权限', async () => {
    const { runtime, clients } = fixture();
    await runtime.startConnector('local');
    const listing = deferred<any[]>();
    clients[0].listTools.mockImplementationOnce(() => listing.promise);
    const pending = runtime.refreshTools('local').then(() => null, error => error);
    await runtime.setConnectorEnabled('local', false);
    await runtime.stopConnector('local');
    listing.resolve([{ name: 'stale', annotations: { readOnlyHint: true } }]);
    const error = await pending;
    const annotation = runtime.getRuntimeToolAnnotations('local', 'stale');
    await runtime.dispose();
    expect(error).toBeInstanceOf(Error);
    expect(annotation).toBeUndefined();
  });
});
