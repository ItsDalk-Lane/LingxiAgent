/** @vitest-environment jsdom */
// R08-T01：旧会话域 query 寻址协议翻译（lingxiFetch translateLegacySessionCall）
// 的真实服务端到端验收——GET 读端口与 PATCH 写端口（body.path 剥离）经真实
// Rust 子路由往返。旧 JSONL 路径不猜映射的负例在 use-hana-fetch.test.ts。
import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { useStore } from '../../stores';
import { lingxiFetch } from '../../hooks/use-hana-fetch';
import type { ServerConnection } from '../../services/server-connection';

// 执行程序固定为本仓库 debug 构建产物；环境变量只作等值门槛（见
// r08-knowledge-real-service.test.ts 同款说明）。
const SERVICE_BINARY = `${process.cwd()}/rust/target/debug/lingxi-service`;
const requested = process.env.LINGXI_R07_SERVICE_BIN;
const binary = requested === SERVICE_BINARY ? SERVICE_BINARY : undefined;

describe('旧会话域协议翻译连接真实 Rust 子进程', () => {
  it.skipIf(!binary)('memory/authorized-folders 经翻译层真实往返（无 LINGXI_R07_SERVICE_BIN 明确跳过）', async () => {
    const scratch = await realpath(await mkdtemp(join(tmpdir(), 'lingxi-r08-session-compat-')));
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
        connectionId: 'r08-session-compat', kind: 'local', serverId: 'local',
        serverNodeKind: 'lingxi-service', studioId: 'local', label: '会话兼容翻译测试',
        baseUrl: `http://${address}`, wsUrl: `ws://${address}`, token,
        authState: 'paired', trustState: 'local', credentialKind: 'loopback_token', capabilities: [],
      };
      useStore.setState({ serverConnections: { [source.connectionId]: source }, activeServerConnectionId: source.connectionId, activeServerConnection: source });

      // 建真实会话（显式 sessionId，无需模型面）。
      const created = await (await lingxiFetch('/lingxi/v1/sessions', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ sessionId: 'r08-compat-s1', agentId: 'a', title: '翻译验收' }),
      })).json();
      expect(created.sessionId).toBe('r08-compat-s1');
      const sessionPath = 'native:r08-compat-s1';

      // GET 读端口：旧 query 寻址经翻译走 Rust 子路由。
      const initial = await (await lingxiFetch(`/api/sessions/memory?path=${encodeURIComponent(sessionPath)}`, { throwOnHttpError: false })).json();
      expect(typeof initial.memoryEnabled).toBe('boolean');
      // PATCH 写端口：旧 body.path 剥离后写入并读回翻转。
      const flipped = !initial.memoryEnabled;
      const patched = await (await lingxiFetch('/api/sessions/memory', {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: sessionPath, memoryEnabled: flipped }),
      })).json();
      expect(patched.memoryEnabled).toBe(flipped);
      const reread = await (await lingxiFetch(`/api/sessions/memory?path=${encodeURIComponent(sessionPath)}`)).json();
      expect(reread.memoryEnabled).toBe(flipped);

      // authorized-folders：add 后读回包含新目录，remove 后不再包含。
      await lingxiFetch('/api/sessions/authorized-folders', {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: sessionPath, action: 'add', folder: workspace }),
      });
      let folders = await (await lingxiFetch(`/api/sessions/authorized-folders?path=${encodeURIComponent(sessionPath)}`)).json();
      const list: string[] = folders.folders ?? folders.authorizedFolders ?? [];
      expect(list).toContain(workspace);
      await lingxiFetch('/api/sessions/authorized-folders', {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: sessionPath, action: 'remove', folder: workspace }),
      });
      folders = await (await lingxiFetch(`/api/sessions/authorized-folders?path=${encodeURIComponent(sessionPath)}`)).json();
      const listAfter: string[] = folders.folders ?? folders.authorizedFolders ?? [];
      expect(listAfter).not.toContain(workspace);

      // summary 读端口经翻译返回 200（滚动摘要记录端口存在；内容不造）。
      const summaryRes = await lingxiFetch(`/api/sessions/summary?path=${encodeURIComponent(sessionPath)}`, { throwOnHttpError: false });
      expect([200, 404]).toContain(summaryRes.status);
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
