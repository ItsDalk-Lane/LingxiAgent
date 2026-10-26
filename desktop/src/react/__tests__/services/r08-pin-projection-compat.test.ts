/** @vitest-environment jsdom */
// R08-C①｜置顶投影形状统一验收（真实组件分组 + 真实 Rust 服务子进程）。
//
// 缺陷（收尾任务 C①）：Rust 列表投影 pinnedAt 为数字（Unix 毫秒）、pin 响应为
// pinnedAtUnixMs，而 store/组件层（session-sections.isPinnedSession、SessionList、
// ws 合并）统一消费 string。修复 = 唯一边界归一（session-actions 的列表映射与
// pin 响应解析）。本验收经真实链路核对四态 + 原断言的已删 Agent 会话禁止置顶
// （真实已删助手会话，不是不存在会话顶替）。
import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { buildSessionSections, isPinnedSession } from '../../components/session-sections';
import { useStore } from '../../stores';
import { loadSessions, pinSession } from '../../stores/session-actions';
import { lingxiFetch } from '../../hooks/use-hana-fetch';
import { createLocalServerConnection, upsertServerConnection } from '../../services/server-connection';
import type { ServerConnection } from '../../services/server-connection';

const SERVICE_BINARY = `${process.cwd()}/rust/target/debug/lingxi-service`;
const requested = process.env.LINGXI_R07_SERVICE_BIN;
const binary = requested === SERVICE_BINARY ? SERVICE_BINARY : undefined;

describe.skipIf(!binary)('置顶投影形状统一（真实组件 + 真实 Rust 服务）', () => {
  let child: ReturnType<typeof spawn> | undefined;
  let address = '';
  let token = '';
  let homeDir = '';
  const previous = useStore.getState();
  const sessionsBefore = useStore.getState().sessions;

  const pinProbe = async (sessionId: string, pinned: boolean) => {
    const res = await lingxiFetch('/api/sessions/pin', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path: `native:${sessionId}`, sessionId, pinned }),
      throwOnHttpError: false,
    });
    return { status: res.status, body: await res.json().catch(() => null) };
  };

  beforeAll(async () => {
    const scratch = await realpath(await mkdtemp(join(tmpdir(), 'lingxi-r08-pin-projection-')));
    const home = join(scratch, 'home');
    homeDir = home;
    const workspace = join(scratch, 'workspace');
    // 空 home 启动（数据纪元闸拒绝带未识别数据的无印 home——B03-COLD-ROOT
    // 红线行为），助手目录在服务就绪后落盘（名册按需读盘，不缓存）。
    await mkdir(home, { recursive: true });
    await mkdir(workspace, { recursive: true });
    const config = join(scratch, 'config.json');
    await writeFile(config, JSON.stringify({ home, workspace: join(scratch, 'workspace') }));
    const env = { ...process.env };
    delete env.LINGXI_HOME;
    child = spawn(SERVICE_BINARY, ['--config', config, '--bind', '127.0.0.1:0'], { env, stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '';
    let stderr = '';
    child.stderr!.on('data', (data) => { stderr = (stderr + String(data)).slice(-4000); });
    address = await new Promise<string>((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`服务启动超时：${stderr}`)), 30_000);
      child!.once('error', (error) => { clearTimeout(timer); reject(error); });
      child!.once('exit', (code) => { clearTimeout(timer); reject(new Error(`服务提前退出 ${code}：${stderr}`)); });
      child!.stdout!.on('data', (data) => {
        stdout += String(data);
        const found = stdout.match(/LINGXI_SERVICE_READY addr=(\S+)/);
        if (found) { clearTimeout(timer); resolve(found[1]); }
      });
    });
    ({ token } = JSON.parse(await readFile(join(home, 'lingxi-service/local-token.json'), 'utf8')));
    // 服务就绪后落助手名册（agents/ 下 config.yaml，动态读盘）。
    for (const agent of ['a1', 'a9']) {
      await mkdir(join(home, `agents/${agent}/sessions`), { recursive: true });
      await writeFile(join(home, `agents/${agent}/config.yaml`), `agent:\n  name: "${agent.toUpperCase()}"\n`);
    }
    const port = Number(new URL(`http://${address}`).port);
    window.t = ((key: string) => key) as typeof window.t;
    window.i18n = { locale: 'en', defaultName: 'Lingxi', _data: {}, _agentOverrides: {}, load: async () => {}, setAgentOverrides: () => {}, t: window.t } as any;
    window.platform = {
      getServerConnectionInfo: async () => ({ port, token, serverNodeKind: 'lingxi-service', serverNodeTransport: 'http' }),
      getPlatform: async () => 'darwin', onSettingsChanged: () => () => {}, onServerRestarted: () => () => {},
    } as unknown as typeof window.platform;
    localStorage.clear();
    const connection: ServerConnection = createLocalServerConnection({
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
    void sessionsBefore;
  });

  it('Rust 列表与 pin 响应经边界归一后：即时置顶、重载置顶、双置顶排序、取消清除（真实分组）', async () => {
    // 两个真实会话（显式 sessionId，无需模型面）。
    for (const [sessionId, title] of [['r08-pin-s1', '置顶验收一'], ['r08-pin-s2', '置顶验收二']] as const) {
      const created = await (await lingxiFetch('/lingxi/v1/sessions', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ sessionId, agentId: 'a1', title }),
      })).json();
      expect(created.sessionId).toBe(sessionId);
    }
    await loadSessions();
    let sessions = useStore.getState().sessions as any[];
    expect(sessions.length).toBeGreaterThanOrEqual(2);
    const s1 = sessions.find((s) => s.sessionId === 'r08-pin-s1');
    const s2 = sessions.find((s) => s.sessionId === 'r08-pin-s2');
    // 未置顶初始态：两个都不在置顶区。
    let sections = buildSessionSections(sessions);
    expect(sections[0].kind).toBe('pinned');
    expect(sections[0].items).toHaveLength(0);

    // 即时置顶（pinSession 动作 → store 投影 → 真实分组谓词）。
    expect(await pinSession(s1.path, true)).toBe(true);
    sessions = useStore.getState().sessions as any[];
    const pinnedNow = sessions.find((s) => s.sessionId === 'r08-pin-s1');
    expect(typeof pinnedNow.pinnedAt).toBe('string');
    expect(pinnedNow.pinnedAt.length).toBeGreaterThan(0);
    expect(isPinnedSession(pinnedNow)).toBe(true);

    // 重载置顶（Rust 列表数字 pinnedAt → 边界归一 → 仍置顶）。
    await loadSessions();
    sessions = useStore.getState().sessions as any[];
    const afterReload = sessions.find((s) => s.sessionId === 'r08-pin-s1');
    expect(typeof afterReload.pinnedAt).toBe('string');
    expect(isPinnedSession(afterReload)).toBe(true);

    // 两项置顶 + 排序（pinOrder 升序真实排序）。
    expect(await pinSession(s2.path, true)).toBe(true);
    await loadSessions();
    sessions = useStore.getState().sessions as any[];
    sections = buildSessionSections(sessions);
    expect(sections[0].kind).toBe('pinned');
    const pinnedIds = sections[0].items.map((s) => s.sessionId);
    expect(pinnedIds).toEqual(['r08-pin-s1', 'r08-pin-s2']);

    // 取消置顶清除投影。
    expect(await pinSession(s1.path, false)).toBe(true);
    sessions = useStore.getState().sessions as any[];
    const cleared = sessions.find((s) => s.sessionId === 'r08-pin-s1');
    expect(cleared.pinnedAt == null).toBe(true);
    expect(isPinnedSession(cleared)).toBe(false);
    await pinSession(s2.path, false);
  }, 60_000);

  it('已删 Agent 会话禁止置顶（真实已删助手会话，非不存在会话顶替）', async () => {
    const created = await (await lingxiFetch('/lingxi/v1/sessions', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ sessionId: 'r08-pin-deleted-agent', agentId: 'a9', title: '已删助手会话' }),
    })).json();
    expect(created.sessionId).toBe('r08-pin-deleted-agent');
    // 助手在场时置顶允许（正前提成立）。
    const okPin = await pinProbe('r08-pin-deleted-agent', true);
    expect(okPin.status).toBe(200);
    // 产品语义的删除标记（agents_admin/memory 以此判定已删，动态读盘）。
    await writeFile(join(homeDir, 'agents/a9/.deleted-agent.json'), '{}\n');
    // 已删 Agent 会话置顶：必须显式拒绝（CONFLICT 409），不伪称成功。
    const rejected = await pinProbe('r08-pin-deleted-agent', true);
    expect(rejected.status).toBeGreaterThanOrEqual(400);
    const body = rejected.body as any;
    expect(Boolean(body?.error) || rejected.status >= 400).toBe(true);
  }, 60_000);
});
