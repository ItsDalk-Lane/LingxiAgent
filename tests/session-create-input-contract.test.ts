import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Hono } from 'hono';
import { serve } from '@hono/node-server';
import type { ServerType } from '@hono/node-server';
import { mkdtempSync, writeFileSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { ensureGrantRegistry } from '../core/grant-registry.ts';
import { createSessionsRoute } from '../server/routes/sessions.ts';

// 真实 HTTP → 真实路由/授权/输入解析；只替换会产生会话及模型副作用的引擎边界。
describe('F02 会话创建真实 HTTP 输入契约 C09-C13', () => {
  let server: ServerType;
  let base: string;
  let home: string;
  let grantBefore: string;
  let engine: ReturnType<typeof makeEngine>;
  let principal: { kind: string; principalId: string; studioId: string; scopes: string[]; connectionKind: string };
  function makeEngine() {
    const created = { sessionPath: path.join(home, 'new.jsonl'), sessionId: 'synthetic-new', agentId: 'hana' };
    const modelCall = vi.fn();
    const create = vi.fn(async (..._args: unknown[]) => {
      writeFileSync(created.sessionPath, 'synthetic session');
      modelCall();
      return created;
    });
    return {
      lingxiHome: home, config: {}, cwd: home, currentAgentId: 'hana', agentName: 'Hana',
      createSession: create, createSessionForAgent: vi.fn(create), createDetachedSession: vi.fn(create),
      modelCall, persistSessionMeta: vi.fn(), updateConfig: vi.fn(async () => {}),
      setSessionProjectAssignment: vi.fn(async () => {}), setSessionForkedFrom: vi.fn(async () => {}),
      getAgent: () => ({ agentName: 'Hana' }), getThinkingLevel: () => 'medium',
    };
  }
  beforeEach(async () => {
    home = mkdtempSync(path.join(tmpdir(), 'lingxi-f02-'));
    writeFileSync(path.join(home, 'sentinel'), 'unchanged');
    ensureGrantRegistry(home);
    grantBefore = readFileSync(path.join(home, 'security', 'grants.json'), 'utf8');
    engine = makeEngine();
    principal = { kind: 'account_user', principalId: 'synthetic-writer', studioId: 'synthetic-studio', scopes: ['sessions.write'], connectionKind: 'custom_remote' };
    const app = new Hono<{ Variables: { authPrincipal: typeof principal } }>();
    app.use('*', async (c, next) => { c.set('authPrincipal', principal); await next(); });
    app.route('/api', createSessionsRoute(engine));
    server = serve({ fetch: app.fetch, hostname: '127.0.0.1', port: 0 });
    await new Promise<void>((resolve) => server.on('listening', resolve));
    const address = server.address();
    if (!address || typeof address === 'string') throw new Error('missing HTTP port');
    base = `http://127.0.0.1:${address.port}/api`;
  });
  afterEach(async () => {
    await new Promise<void>((resolve, reject) => server.close(err => err ? reject(err) : resolve()));
    rmSync(home, { recursive: true, force: true });
  });
  async function post(endpoint: string, body: string) {
    return fetch(`${base}/sessions/${endpoint}`, { method: 'POST', headers: { 'content-type': 'application/json' }, body });
  }
  function noEffects() {
    expect(engine.createSession).not.toHaveBeenCalled();
    expect(engine.createSessionForAgent).not.toHaveBeenCalled();
    expect(engine.createDetachedSession).not.toHaveBeenCalled();
    expect(engine.persistSessionMeta).not.toHaveBeenCalled();
    expect(engine.updateConfig).not.toHaveBeenCalled();
    expect(engine.setSessionProjectAssignment).not.toHaveBeenCalled();
    expect(engine.setSessionForkedFrom).not.toHaveBeenCalled();
    expect(engine.modelCall).not.toHaveBeenCalled();
    expect(readdirSync(home)).toEqual(['security', 'sentinel']);
    expect(readFileSync(path.join(home, 'security', 'grants.json'), 'utf8')).toBe(grantBefore);
    expect(readFileSync(path.join(home, 'sentinel'), 'utf8')).toBe('unchanged');
  }
  for (const endpoint of ['new', 'new-detached']) {
    for (const [body, code] of [['{broken', 'invalid_json'], ['null', 'invalid_body'], ['[]', 'invalid_body'], ['42', 'invalid_body'], ['"string"', 'invalid_body'], ['true', 'invalid_body']]) {
      it(`C09/C11 ${endpoint} 拒绝 ${body} 并无写入/模型副作用`, async () => {
        const res = await post(endpoint, body!);
        expect(res.status).toBe(400);
        expect(await res.json()).toMatchObject({ code });
        noEffects();
      });
    }
    const invalidFields = {
      cwd: 42, workspaceMountId: {}, workspaceFolders: 'folder', memoryEnabled: 'yes',
      agentId: [], thinkingLevel: {}, projectId: 42,
      ...(endpoint === 'new' ? { currentAgentId: false } : { permissionMode: [], forkedFromSessionId: 42, recordWorkspaceHistory: 'yes' }),
    };
    for (const [field, value] of Object.entries(invalidFields)) {
      it(`C10/C11 ${endpoint} 拒绝错误 ${field}`, async () => {
        const res = await post(endpoint, JSON.stringify({ [field]: value }));
        expect(res.status).toBe(400);
        expect(await res.json()).toMatchObject({ code: 'invalid_field_type' });
        noEffects();
      });
    }
    it(`C10/C11 ${endpoint} 不静默过滤错误目录元素`, async () => {
      const res = await post(endpoint, JSON.stringify({ workspaceFolders: [home, 42] }));
      expect(res.status).toBe(400);
      expect(await res.json()).toMatchObject({ code: 'invalid_field_type' });
      noEffects();
    });
    for (const body of ['', '{}', '{"cwd":null,"workspaceMountId":null,"workspaceFolders":null,"memoryEnabled":null,"agentId":null,"thinkingLevel":null,"projectId":null,"permissionMode":null}']) {
      it(`C12 ${endpoint} 保留合法缺省 ${body}`, async () => {
        const res = await post(endpoint, body);
        expect(res.status).toBe(200);
        expect(engine.modelCall).toHaveBeenCalledTimes(1);
        expect(engine.persistSessionMeta).toHaveBeenCalledTimes(1);
      });
    }
    it(`C12 ${endpoint} 保留目录/记忆/agent/思考及未知字段语义`, async () => {
      const res = await post(endpoint, JSON.stringify({ cwd: home, workspaceFolders: [home, ' '], memoryEnabled: false, agentId: 'other', thinkingLevel: 'high', permissionMode: 'read_only', unknownExtension: { value: 1 } }));
      expect(res.status).toBe(200);
      if (endpoint === 'new') {
        expect(engine.createSessionForAgent).toHaveBeenCalledWith('other', home, false, undefined, expect.objectContaining({ workspaceFolders: [home], thinkingLevel: 'high' }));
      } else {
        expect(engine.createDetachedSession).toHaveBeenCalledWith(expect.objectContaining({ cwd: home, memoryEnabled: false, agentId: 'other', workspaceFolders: [home], permissionMode: 'read_only', thinkingLevel: 'high' }));
      }
    });
    it(`C13 ${endpoint} 只读主体不能用默认输入创建`, async () => {
      principal.scopes = ['sessions.read'];
      const res = await post(endpoint, '');
      expect(res.status).toBe(403);
      noEffects();
    });
    it(`C13 ${endpoint} 错误主体无授权不创建`, async () => {
      principal.kind = 'device'; principal.scopes = [];
      const res = await post(endpoint, '{}');
      expect(res.status).toBe(403);
      noEffects();
    });
  }
});
