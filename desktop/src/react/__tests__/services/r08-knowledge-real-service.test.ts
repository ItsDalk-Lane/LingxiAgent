/** @vitest-environment jsdom */
// R08-T01 §6.2：knowledge 域真实接线验收。通过真实 Rust 服务子进程验证
// knowledge-api 的现役函数在 lingxi-service 连接下走 /lingxi/v1/knowledge/*
// （旧 /api/knowledge/* 在 Rust 侧无路由，映射缺失时会以 404 暴露）。
import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { useStore } from '../../stores';
import type { ServerConnection } from '../../services/server-connection';
import {
  createKnowledgeNotebook,
  listKnowledgeIngestion,
  listKnowledgeNotebooks,
  renameKnowledgeNotebook,
} from '../../components/knowledge/knowledge-api';

// 执行程序固定为本仓库 debug 构建产物（cargo build -p lingxi-service 的
// 唯一路径），环境变量只作为“是否运行”的门槛：值必须精确等于该固定路径。
// 不接受任意来源的可执行程序；缺环境时显式跳过（与既有 R07 real-service
// 测试同口径，正式 runner 负责供齐环境并拒绝跳过）。
const SERVICE_BINARY = `${process.cwd()}/rust/target/debug/lingxi-service`;
const requested = process.env.LINGXI_R07_SERVICE_BIN;
const binary = requested === SERVICE_BINARY ? SERVICE_BINARY : undefined;

describe('知识库面板连接真实 Rust 子进程', () => {
  it.skipIf(!binary)('现役 knowledge-api 函数经 Rust 路由完成笔记本创建/改名/列举（无 LINGXI_R07_SERVICE_BIN 明确跳过）', async () => {
    const scratch = await realpath(await mkdtemp(join(tmpdir(), 'lingxi-r08-knowledge-ui-')));
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
        connectionId: 'r08-knowledge-actual', kind: 'local', serverId: 'local',
        serverNodeKind: 'lingxi-service', studioId: 'local', label: '知识库接线测试',
        baseUrl: `http://${address}`, wsUrl: `ws://${address}`, token,
        authState: 'paired', trustState: 'local', credentialKind: 'loopback_token', capabilities: [],
      };
      useStore.setState({ serverConnections: { [source.connectionId]: source }, activeServerConnectionId: source.connectionId, activeServerConnection: source });

      // 全新资料上先列空表，再走真实创建/改名/读回。
      expect(await listKnowledgeNotebooks()).toEqual([]);
      const created = await createKnowledgeNotebook('R08 接线验收');
      expect(created.name).toBe('R08 接线验收');
      expect(created.id).toBeTruthy();
      const renamed = await renameKnowledgeNotebook(created.id, 'R08 接线验收·改名');
      expect(renamed.name).toBe('R08 接线验收·改名');
      const listed = await listKnowledgeNotebooks();
      expect(listed.map(row => row.id)).toContain(created.id);
      expect(listed.find(row => row.id === created.id)?.name).toBe('R08 接线验收·改名');
      const ingestion = await listKnowledgeIngestion({});
      expect(Array.isArray(ingestion.jobs)).toBe(true);
      expect(ingestion.jobs).toEqual([]);
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
