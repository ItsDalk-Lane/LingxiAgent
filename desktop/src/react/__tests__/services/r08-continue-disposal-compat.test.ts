/** @vitest-environment jsdom */
// R08-C②｜续接已删 Agent 会话 + 工作台处置：旧 /api 路径 → Rust 的双向适配验收。
//
// 缺陷：continue-deleted-agent 与 workspace-disposal 此前未进翻译表（生产翻译函数
// 对二者请求数为 0，旧 UI 接 Rust 时被连接门拒）；且 Rust 续接响应无 path 字段而
// UI（InputArea.tsx:2387 → continueDeletedAgentSession）要求 path 做切换。
// 修复 = 翻译表两条 + 动作层响应适配（path ?? native:<sessionId>）。
// 验收驱动 = 真实 store 动作（与按钮同一边界）→ 真实 Rust 子进程 → 结果 →
// 列表/会话切换状态；负路径核对返回状态与副作用一致。
import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, readFile, realpath, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { useStore } from '../../stores';
import {
  continueDeletedAgentSession,
  disposeWorkspaceSessions,
  listArchivedSessions,
  loadSessions,
} from '../../stores/session-actions';
import { lingxiFetch } from '../../hooks/use-hana-fetch';
import { createLocalServerConnection, upsertServerConnection } from '../../services/server-connection';

const SERVICE_BINARY = `${process.cwd()}/rust/target/debug/lingxi-service`;
const requested = process.env.LINGXI_R07_SERVICE_BIN;
const binary = requested === SERVICE_BINARY ? SERVICE_BINARY : undefined;

describe.skipIf(!binary)('续接已删 Agent 会话 / 工作台处置（真实动作 + 真实 Rust 服务）', () => {
  let child: ReturnType<typeof spawn> | undefined;
  let homeDir = '';
  let workspaceA = '';
  let workspaceB = '';
  const previous = useStore.getState();

  const createSession = async (sessionId: string, agentId: string, title: string) => {
    const res = await lingxiFetch('/lingxi/v1/sessions', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ sessionId, agentId, title }),
    });
    const created = await res.json();
    expect(created.sessionId).toBe(sessionId);
  };
  const setWorkspace = async (sessionId: string, cwd: string) => {
    const res = await lingxiFetch(`/lingxi/v1/sessions/${encodeURIComponent(sessionId)}/workspace`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ cwd }),
    });
    expect(res.status).toBe(200);
  };

  beforeAll(async () => {
    const scratch = await realpath(await mkdtemp(join(tmpdir(), 'lingxi-r08-continue-disposal-')));
    homeDir = join(scratch, 'home');
    workspaceA = join(scratch, 'ws-a');
    workspaceB = join(scratch, 'ws-b');
    // 空 home 启动（数据纪元闸拒绝带未识别数据的无印 home）；助手在就绪后落盘。
    await Promise.all([mkdir(homeDir, { recursive: true }), mkdir(workspaceA, { recursive: true }), mkdir(workspaceB, { recursive: true })]);
    const config = join(scratch, 'config.json');
    await writeFile(config, JSON.stringify({ home: homeDir, workspace: workspaceA }));
    const env = { ...process.env };
    delete env.LINGXI_HOME;
    child = spawn(SERVICE_BINARY, ['--config', config, '--bind', '127.0.0.1:0'], { env, stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '';
    let stderr = '';
    child.stderr!.on('data', (data) => { stderr = (stderr + String(data)).slice(-4000); });
    const address = await new Promise<string>((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`服务启动超时：${stderr}`)), 30_000);
      child!.once('error', (error) => { clearTimeout(timer); reject(error); });
      child!.once('exit', (code) => { clearTimeout(timer); reject(new Error(`服务提前退出 ${code}：${stderr}`)); });
      child!.stdout!.on('data', (data) => {
        stdout += String(data);
        const found = stdout.match(/LINGXI_SERVICE_READY addr=(\S+)/);
        if (found) { clearTimeout(timer); resolve(found[1]); }
      });
    });
    const { token } = JSON.parse(await readFile(join(homeDir, 'lingxi-service/local-token.json'), 'utf8'));
    for (const agent of ['a1', 'a9']) {
      await mkdir(join(homeDir, `agents/${agent}/sessions`), { recursive: true });
      await writeFile(join(homeDir, `agents/${agent}/config.yaml`), `agent:\n  name: "${agent.toUpperCase()}"\n`);
    }
    const port = Number(new URL(`http://${address}`).port);
    window.t = ((key: string) => key) as typeof window.t;
    window.i18n = { locale: 'en', defaultName: 'Lingxi', _data: {}, _agentOverrides: {}, load: async () => {}, setAgentOverrides: () => {}, t: window.t } as any;
    window.platform = {
      getServerConnectionInfo: async () => ({ port, token, serverNodeKind: 'lingxi-service', serverNodeTransport: 'http' }),
      getPlatform: async () => 'darwin', onSettingsChanged: () => () => {}, onServerRestarted: () => () => {},
    } as unknown as typeof window.platform;
    localStorage.clear();
    const connection = createLocalServerConnection({
      serverPort: port, serverToken: token,
      serverNodeKind: 'lingxi-service', serverNodeTransport: 'http',
    })!;
    useStore.setState({
      serverPort: String(port), serverToken: token,
      serverConnections: upsertServerConnection({}, connection),
      activeServerConnectionId: connection.connectionId,
      activeServerConnection: connection,
      sessions: [],
    } as any);
  }, 60_000);

  afterAll(async () => {
    useStore.setState(previous);
    if (child?.exitCode === null) {
      child.kill('SIGTERM');
      await new Promise((resolve) => { child!.once('exit', resolve); setTimeout(resolve, 3000); });
    }
  });

  it('续接已删 Agent 会话：真实动作 → Rust 新会话身份 → 列表刷新 → 切换到返回的 path', async () => {
    await createSession('r08-cont-src', 'a9', '待续接会话');
    await loadSessions();
    // 负路径先行：助手未删时续接必须显式失败（400 agent_not_deleted），且无副作用。
    const before = (useStore.getState().sessions as any[]).map((s) => s.sessionId).sort();
    expect(await continueDeletedAgentSession('native:r08-cont-src')).toBe(false);
    await loadSessions();
    const afterNegative = (useStore.getState().sessions as any[]).map((s) => s.sessionId).sort();
    expect(afterNegative).toEqual(before);

    // 真实删除标记后续接成功：Rust 回 sessionId（无 path），动作层适配为 native:<id>。
    await writeFile(join(homeDir, 'agents/a9/.deleted-agent.json'), '{}\n');
    expect(await continueDeletedAgentSession('native:r08-cont-src')).toBe(true);
    const state = useStore.getState() as any;
    expect(typeof state.currentSessionPath).toBe('string');
    expect(state.currentSessionPath.startsWith('native:')).toBe(true);
    expect(state.currentSessionPath).not.toBe('native:r08-cont-src');
    const continued = (state.sessions as any[]).find((s) => s.path === state.currentSessionPath);
    expect(continued).toBeTruthy();
    expect(continued.title).toContain('(续接)');
    expect(continued.agentId).not.toBe('a9');
  }, 60_000);

  it('工作台处置：按 cwd 身份归档匹配会话（matched/disposed 一致），其他工作台不受影响；缺身份显式拒且无副作用', async () => {
    await createSession('r08-disp-a1', 'a1', '工作台A一');
    await createSession('r08-disp-a2', 'a1', '工作台A二');
    await createSession('r08-disp-b1', 'a1', '工作台B一');
    await setWorkspace('r08-disp-a1', workspaceA);
    await setWorkspace('r08-disp-a2', workspaceA);
    await setWorkspace('r08-disp-b1', workspaceB);
    await loadSessions();
    const activeBefore = (useStore.getState().sessions as any[]).map((s) => s.sessionId);
    expect(activeBefore).toEqual(expect.arrayContaining(['r08-disp-a1', 'r08-disp-a2', 'r08-disp-b1']));

    // 负路径：缺工作台身份 → Rust 400 missing_workspace_identity → 动作返回 null，无会话被动。
    expect(await disposeWorkspaceSessions({}, 'archive')).toBeNull();
    await loadSessions();
    expect((useStore.getState().sessions as any[]).map((s) => s.sessionId)).toEqual(expect.arrayContaining(['r08-disp-a1', 'r08-disp-a2', 'r08-disp-b1']));

    // 正路径：按 cwd 归档工作台 A 的两个会话。
    const result = await disposeWorkspaceSessions({ cwd: workspaceA }, 'archive');
    expect(result).toEqual({ ok: true, action: 'archive', matched: 2, disposed: 2, skippedStreaming: 0 });
    const activeAfter = (useStore.getState().sessions as any[]).map((s) => s.sessionId);
    expect(activeAfter).not.toContain('r08-disp-a1');
    expect(activeAfter).not.toContain('r08-disp-a2');
    expect(activeAfter).toContain('r08-disp-b1');
    const archived = (await listArchivedSessions()) as any[];
    const archivedIds = archived.map((s) => s.sessionId);
    expect(archivedIds).toEqual(expect.arrayContaining(['r08-disp-a1', 'r08-disp-a2']));
    expect(archivedIds).not.toContain('r08-disp-b1');
  }, 60_000);
});
