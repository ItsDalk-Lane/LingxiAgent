import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, expect, vi } from 'vitest';
import { Hono } from 'hono';
import { ModelRuntime, DefaultResourceLoader, SessionManager, SettingsManager } from '@earendil-works/pi-coding-agent';
import { createAssistantMessageEventStream } from '@earendil-works/pi-ai';
import { createAgentSession } from '../../lib/pi-sdk/index.ts';
import { submitDesktopSessionMessage } from '../../core/desktop-session-submit.ts';
import { createSessionsRoute } from '../../server/routes/sessions.ts';
import { generateSessionId } from '../../core/session-manifest/id.ts';

const cleanup: Array<() => void | Promise<void>> = [];
afterEach(async () => { for (const fn of cleanup.splice(0).reverse()) await fn(); vi.restoreAllMocks(); });

export async function createDesktopInputHistoryFixture() {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'r04-sdk-history-'));
  cleanup.push(() => fs.rmSync(home, { recursive: true, force: true }));
  const sessions = path.join(home, 'agents', 'test-agent', 'sessions');
  fs.mkdirSync(sessions, { recursive: true });
  const runtime = await ModelRuntime.create({ authPath: path.join(home, 'auth.json'), modelsPath: null, refreshOnCreate: false });
  runtime.registerProvider('synthetic-r04', { api: 'openai-completions', baseUrl: 'http://127.0.0.1:1/v1', apiKey: 'synthetic-unused-key', authHeader: true } as any);
  const loader = new DefaultResourceLoader({ cwd: home, agentDir: home, noExtensions: true, noSkills: true, noPromptTemplates: true, noThemes: true, noContextFiles: true });
  await loader.reload();
  const manager = SessionManager.create(home, sessions);
  const model: any = { id: 'synthetic-r04-model', name: '合成模型', provider: 'synthetic-r04', api: 'openai-completions', baseUrl: 'http://127.0.0.1:1/v1', reasoning: false, input: ['text'], contextWindow: 32768, maxTokens: 128, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 } };
  // 只替换供应商流边界，真实 SDK prompt、事件顺序及 SessionManager append 全部执行。
  const stream = vi.spyOn(runtime, 'streamSimple').mockImplementation(() => {
    const result = createAssistantMessageEventStream();
    queueMicrotask(() => {
      result.push({ type: 'done', reason: 'stop', message: { role: 'assistant', content: [{ type: 'text', text: 'synthetic reply' }], api: 'openai-completions', provider: model.provider, model: model.id, usage: { input: 1, output: 1, cacheRead: 0, cacheWrite: 0, totalTokens: 2, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } }, stopReason: 'stop', timestamp: Date.now() } } as any);
      result.end();
    });
    return result;
  });
  const { session } = await createAgentSession({ cwd: home, model, modelRuntime: runtime, resourceLoader: loader, sessionManager: manager, settingsManager: SettingsManager.inMemory({ compaction: { enabled: false }, retry: { enabled: false } }), noTools: 'all' });
  cleanup.push(() => session.dispose());
  const sessionPath = manager.getSessionFile()!;
  // 生产环境的业务身份由 manifest 生成，与 SDK 文件头 UUID 独立。
  const sessionId = generateSessionId();
  const engine: any = {
    agentsDir: path.join(home, 'agents'), currentSessionPath: sessionPath,
    ensureSessionLoaded: async () => session,
    promptSession: async (_path: string, text: string, opts: any) => session.prompt(text, opts),
    getSessionByPath: (p: string) => p === sessionPath ? session : null,
    getSessionIdForPath: (p: string) => p === sessionPath ? sessionId : null,
    getSessionManifest: (id: string) => id === sessionId ? { sessionId, currentLocator: { path: sessionPath } } : null,
    getSessionBranchHead: () => ({ sessionId, leafId: manager.getLeafId(), observedTailLeafId: manager.getEntries().at(-1)?.id || null }),
    isSessionStreaming: () => session.isStreaming,
    agentIdFromSessionPath: () => 'test-agent', getAgent: () => ({ agentName: 'Synthetic' }),
    getSessionWorkspaceMount: () => null, emitEvent: vi.fn(),
    activityHub: { rebroadcastSession: vi.fn() },
  };
  const app = new Hono(); app.route('/api', createSessionsRoute(engine));
  const submit = (id: string, snapshotVersion = 1) => submitDesktopSessionMessage(engine, { sessionId, sessionPath, text: 'identical user text', clientMessageId: id, snapshotVersion } as any);
  const history = async () => {
    const response = await app.request(`/api/sessions/messages?sessionId=${encodeURIComponent(sessionId)}&reconciliation=1`);
    expect(response.status).toBe(200);
    return response.json() as Promise<any>;
  };
  return { home, manager, session, sessionId, sessionPath, engine, stream, app, submit, history };
}
