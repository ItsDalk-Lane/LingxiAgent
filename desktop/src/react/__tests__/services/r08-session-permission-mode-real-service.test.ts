/** @vitest-environment jsdom */
// R08-T01：session-permission-mode 域真实接线验收。通过真实 Rust 服务子进程
// 验证旧 /api/session-permission-mode 调用经 serviceRoute 映射命中
// /lingxi/v1/session-permission-mode（映射缺失时 Rust 侧无路由会以 404 暴露），
// 并沿真实 HTTP 面验证 GET/POST 各分支与 engine 侧 session-permission-mode.ts
// 的既有语义一致（默认域 / pendingNewSession / currentSessionOnly / sessionPath
// 会话级写 / 显式 409 / 400）。
import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { useStore } from '../../stores';
import type { ServerConnection } from '../../services/server-connection';
import { lingxiFetch } from '../../hooks/use-hana-fetch';

// 执行程序固定为本仓库 debug 构建产物（cargo build -p lingxi-service 的
// 唯一路径），环境变量只作为“是否运行”的门槛：值必须精确等于该固定路径。
// 不接受任意来源的可执行程序；缺环境时显式跳过（与既有 R07/R08 real-service
// 测试同口径，正式 runner 负责供齐环境并拒绝跳过）。
const SERVICE_BINARY = `${process.cwd()}/rust/target/debug/lingxi-service`;
const requested = process.env.LINGXI_R07_SERVICE_BIN;
const binary = requested === SERVICE_BINARY ? SERVICE_BINARY : undefined;

type JsonBody = Record<string, unknown> & { mode?: string; defaultMode?: string; accessMode?: string; ok?: boolean };

async function getState(): Promise<{ status: number; body: JsonBody }> {
  const res = await lingxiFetch('/api/session-permission-mode', { throwOnHttpError: false });
  return { status: res.status, body: (await res.json()) as JsonBody };
}

async function postMode(payload: Record<string, unknown>): Promise<{ status: number; body: JsonBody }> {
  const res = await lingxiFetch('/api/session-permission-mode', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
    throwOnHttpError: false,
  });
  return { status: res.status, body: (await res.json()) as JsonBody };
}

describe('session-permission-mode 连接真实 Rust 子进程', () => {
  it.skipIf(!binary)('旧 /api 调用经映射命中 Rust 路由并完成 GET/POST 全分支（无 LINGXI_R07_SERVICE_BIN 明确跳过）', async () => {
    const scratch = await realpath(await mkdtemp(join(tmpdir(), 'lingxi-r08-perm-ui-')));
    const home = join(scratch, 'home');
    const workspace = join(scratch, 'workspace');
    let child: ReturnType<typeof spawn> | undefined;
    const previous = useStore.getState();
    try {
      await Promise.all([mkdir(home), mkdir(workspace)]);
      const config = join(scratch, 'config.json');
      await writeFile(config, JSON.stringify({ home, workspace }));
      const env = { ...process.env };
      delete env.LINGXI_HOME;
      child = spawn(SERVICE_BINARY, ['--config', config, '--bind', '127.0.0.1:0'], { env, stdio: ['ignore', 'pipe', 'pipe'] });
      let stdout = '';
      let stderr = '';
      child.stderr!.on('data', data => { stderr = (stderr + String(data)).slice(-4000); });
      const address = await new Promise<string>((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error(`服务启动超时：${stderr}`)), 30_000);
        child!.once('error', error => { clearTimeout(timer); reject(error); });
        child!.once('exit', code => { clearTimeout(timer); reject(new Error(`服务提前退出 ${code}：${stderr}`)); });
        child!.stdout!.on('data', data => { stdout += String(data); const found = stdout.match(/LINGXI_SERVICE_READY addr=(\S+)/); if (found) { clearTimeout(timer); resolve(found[1]); } });
      });
      const { token } = JSON.parse(await readFile(join(home, 'lingxi-service/local-token.json'), 'utf8'));
      const source: ServerConnection = {
        connectionId: 'r08-permission-mode-actual', kind: 'local', serverId: 'local',
        serverNodeKind: 'lingxi-service', studioId: 'local', label: '权限模式接线测试',
        baseUrl: `http://${address}`, wsUrl: `ws://${address}`, token,
        authState: 'paired', trustState: 'local', credentialKind: 'loopback_token', capabilities: [],
      };
      useStore.setState({ serverConnections: { [source.connectionId]: source }, activeServerConnectionId: source.connectionId, activeServerConnection: source });

      // 初始状态：无焦点会话、无偏好默认 → auto/operate/auto。
      const initial = await getState();
      expect(initial.status).toBe(200);
      expect(initial.body).toMatchObject({ mode: 'auto', accessMode: 'operate', defaultMode: 'auto' });

      // 默认域：写偏好默认并读回（无会话时 mode 跟随默认）。
      const setAsk = await postMode({ mode: 'ask' });
      expect(setAsk.status).toBe(200);
      expect(setAsk.body).toMatchObject({ ok: true, defaultMode: 'ask' });
      const afterAsk = await getState();
      expect(afterAsk.body).toMatchObject({ mode: 'ask', defaultMode: 'ask' });

      // pendingNewSession：写默认（下一会话消费）。
      const pending = await postMode({ mode: 'operate', pendingNewSession: true });
      expect(pending.status).toBe(200);
      expect(pending.body).toMatchObject({ ok: true, defaultMode: 'operate' });

      // 非法 mode 显式 400。
      const invalid = await postMode({ mode: 'nonsense' });
      expect(invalid.status).toBe(400);

      // currentSessionOnly 无活动会话显式 409，不静默写默认。
      const noSession = await postMode({ mode: 'operate', currentSessionOnly: true });
      expect(noSession.status).toBe(409);
      expect(noSession.body.ok).toBe(false);

      // 未知会话 sessionPath 显式 409；会话级 auto 显式 409；旧 JSONL 路径不猜映射显式 409。
      const unknown = await postMode({ mode: 'read_only', sessionPath: 'native:missing-session' });
      expect(unknown.status).toBe(409);
      const sessionAuto = await postMode({ mode: 'auto', sessionPath: 'native:missing-session' });
      expect(sessionAuto.status).toBe(409);
      const legacy = await postMode({ mode: 'ask', sessionPath: '/legacy/s1.jsonl' });
      expect(legacy.status).toBe(409);
      const afterRejections = await getState();
      expect(afterRejections.body.defaultMode).toBe('operate');

      // 会话级成功路径：真实会话（显式 sessionId，无需模型面，与 Rust 套件同口径）。
      // 建会话/切焦点是 Rust 原生源路由（非本端口），用直连 fetch 做测试准备。
      const sessionId = 'r08-ui-perm-session';
      const setup = async (path: string, payload: Record<string, unknown>) => {
        const res = await fetch(`${source.baseUrl}${path}`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
          body: JSON.stringify(payload),
        });
        return { status: res.status, body: (await res.json()) as JsonBody };
      };
      const created = await setup('/lingxi/v1/sessions', { sessionId, agentId: 'a', title: 'R08 权限接线' });
      expect(created.status).toBe(201);

      const scoped = await postMode({ mode: 'read_only', sessionPath: `native:${sessionId}` });
      expect(scoped.status).toBe(200);
      expect(scoped.body).toMatchObject({ ok: true, mode: 'read_only', accessMode: 'read_only', defaultMode: 'operate' });

      // 聚焦该会话后 GET 反映会话级显式值；currentSessionOnly 命中焦点会话。
      const switched = await setup('/lingxi/v1/sessions/switch', { sessionId });
      expect(switched.status).toBe(200);
      const focused = await getState();
      expect(focused.body).toMatchObject({ mode: 'read_only', accessMode: 'read_only', defaultMode: 'operate' });

      const currentOnly = await postMode({ mode: 'ask', currentSessionOnly: true });
      expect(currentOnly.status).toBe(200);
      expect(currentOnly.body).toMatchObject({ ok: true, mode: 'ask' });
      const afterCurrentOnly = await getState();
      expect(afterCurrentOnly.body.mode).toBe('ask');

      // 默认域写不改焦点会话显式值（engine 同语义：会话级优先于默认）。
      const defaultAuto = await postMode({ mode: 'auto' });
      expect(defaultAuto.status).toBe(200);
      expect(defaultAuto.body.defaultMode).toBe('auto');
      const final = await getState();
      expect(final.body).toMatchObject({ mode: 'ask', defaultMode: 'auto' });
    } finally {
      useStore.setState({ serverConnections: previous.serverConnections, activeServerConnectionId: previous.activeServerConnectionId, activeServerConnection: previous.activeServerConnection });
      if (child) {
        child.kill('SIGTERM');
        await new Promise<void>(resolve => { child!.once('exit', () => resolve()); setTimeout(() => { child!.kill('SIGKILL'); resolve(); }, 5000); });
      }
      await rm(scratch, { recursive: true, force: true });
    }
  });
});
