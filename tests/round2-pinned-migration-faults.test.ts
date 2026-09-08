import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { afterEach, describe, expect, it } from 'vitest';
import { migrateAgentPinnedTenets, readPinnedTenetsMigrationReceipt, sha256File, writePinnedBackup } from '../core/pinned-tenets-migration.ts';
import { addTenetDirect, readTenetsFileStrict, tenetsFilePath } from '../lib/memory/tenets.ts';

const homes: string[] = [];
afterEach(() => { for (const home of homes.splice(0)) fs.rmSync(home, { recursive: true, force: true }); });
const receiptPath = (dir: string) => path.join(dir, 'memory', 'pinned-tenets-migration.receipt.json');
function fixture() {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'r01-faults-'));
  homes.push(home);
  const dir = path.join(home, 'agents', 'test-agent');
  fs.mkdirSync(path.join(dir, 'memory'), { recursive: true });
  fs.writeFileSync(path.join(dir, 'pinned-memory.json'), JSON.stringify({ version: 1, items: [{ id: 'old', content: 'synthetic pin' }] }));
  return dir;
}

describe('R01 合成文件系统故障与真实硬终止', () => {
  it('R01-08 SIGKILL 在归档删除源后、收据写入前发生，重启仅补记完成', async () => {
    const dir = fixture();
    addTenetDirect(dir, { content: 'existing user item' });
    const sourceBytes = fs.readFileSync(path.join(dir, 'pinned-memory.json'));
    const moduleUrl = pathToFileURL(path.resolve('core/pinned-tenets-migration.ts')).href;
    // 子进程直接导入生产模块，信号终止不经过异常捕获或 finally。
    const script = `import { migrateAgentPinnedTenets } from ${JSON.stringify(moduleUrl)};
      migrateAgentPinnedTenets(${JSON.stringify(dir)}, 'test-agent', { at(point) {
        if (point === 'archive:after:pinned-memory.json') process.kill(process.pid, 'SIGKILL');
      }});`;
    const child = spawn(process.execPath, ['--input-type=module', '--eval', script], { stdio: ['ignore', 'pipe', 'pipe'] });
    let stderr = '';
    child.stderr.on('data', chunk => { stderr += chunk.toString(); });
    const termination = await new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((resolve, reject) => {
      child.once('error', reject);
      child.once('close', (code, signal) => resolve({ code, signal }));
    });
    // Windows 没有 POSIX 信号语义：process.kill(pid, 'SIGKILL') 由 Node 仿真为
    // 非零退出码（code=1, signal=null）；崩溃注入本身在两端都真实发生。
    if (process.platform === 'win32') {
      expect(termination.code, stderr).not.toBe(0);
      expect(termination.signal).toBeNull();
    } else {
      expect(termination, stderr).toEqual({ code: null, signal: 'SIGKILL' });
    }
    const interrupted = readPinnedTenetsMigrationReceipt(dir)!;
    expect(interrupted.state).toBe('target_committed');
    expect(interrupted.archived[0].state).toBe('planned');
    expect(fs.existsSync(path.join(dir, 'pinned-memory.json'))).toBe(false);
    const archivePath = path.join(dir, interrupted.archived[0].to);
    expect(fs.readFileSync(archivePath)).toEqual(sourceBytes);
    const committedBytes = fs.readFileSync(tenetsFilePath(dir));
    migrateAgentPinnedTenets(dir, 'test-agent');
    const completed = readPinnedTenetsMigrationReceipt(dir)!;
    expect(completed.state).toBe('completed');
    expect(completed.operationId).toBe(interrupted.operationId);
    expect(completed.archived[0].to).toBe(interrupted.archived[0].to);
    expect(fs.readFileSync(tenetsFilePath(dir))).toEqual(committedBytes);
    expect(fs.readFileSync(archivePath)).toEqual(sourceBytes);
    expect(readTenetsFileStrict(tenetsFilePath(dir)).tenets.map(item => item.content).sort()).toEqual(['existing user item', 'synthetic pin']);
  }, 15_000);

  it('R01-16 完整摘要命名的已有备份内容不符，拒绝复用且不覆盖', () => {
    const dir = fixture();
    const source = path.join(dir, 'pinned-memory.json');
    const original = fs.readFileSync(source);
    const backupDir = path.join(dir, 'memory', 'pinned-migration-backup', 'synthetic-operation');
    fs.mkdirSync(backupDir, { recursive: true });
    const destination = path.join(backupDir, `${sha256File(source)}-pinned-memory.json`);
    fs.writeFileSync(destination, 'different synthetic bytes');
    expect(() => writePinnedBackup(source, destination)).toThrow(/backup digest mismatch/i);
    expect(fs.readFileSync(destination, 'utf8')).toBe('different synthetic bytes');
    expect(fs.readFileSync(source)).toEqual(original);
  });

  it('R01-15 旧收据备份失败，不覆盖收据、目标或来源原字节', () => {
    const dir = fixture();
    addTenetDirect(dir, { content: 'existing user item' });
    expect(() => migrateAgentPinnedTenets(dir, 'test-agent', { at: point => {
      if (point === 'receipt:prepared') throw new Error('synthetic interruption');
    } })).toThrow('synthetic interruption');
    const oldReceipt = JSON.parse(fs.readFileSync(receiptPath(dir), 'utf8'));
    oldReceipt.state = 'failed';
    oldReceipt.error = { code: 'SYNTHETIC_PRIOR_FAILURE', message: 'unique prior evidence' };
    fs.writeFileSync(receiptPath(dir), JSON.stringify(oldReceipt, null, 4) + '\n');
    const paths = [receiptPath(dir), tenetsFilePath(dir), path.join(dir, 'pinned-memory.json')];
    const before = paths.map(file => fs.readFileSync(file));
    let injected = false;
    try {
      migrateAgentPinnedTenets(dir, 'test-agent', { at: point => {
        if (point === 'backup:before:receipt.json') {
          injected = true;
          throw Object.assign(new Error('synthetic receipt backup EACCES'), { code: 'EACCES' });
        }
      } });
    } catch (error) {
      expect(error).toMatchObject({ code: 'MIGRATION_BACKUP_FAILED' });
    }
    expect(injected).toBe(true);
    paths.forEach((file, index) => expect(fs.readFileSync(file)).toEqual(before[index]));
  });

  it('R01-13 收据读取错误（EACCES/EISDIR）不能当作不存在启动迁移', () => {
    const dir = fixture();
    const file = receiptPath(dir);
    const original = Buffer.from('{"synthetic":"unreadable receipt"}\n');
    fs.writeFileSync(file, original);
    if (process.platform === 'win32') {
      // Windows 权限模型不产生 POSIX 读限制（chmod 000 不挡读）：用「收据路径
      // 是目录」制造确定性的非 ENOENT 读取错误（EISDIR），同一不变量——读取
      // 错误必须按损坏收据显式失败，不得当作不存在启动迁移。
      fs.rmSync(file);
      fs.mkdirSync(file);
      try {
        expect(() => fs.readFileSync(file)).toThrow(expect.objectContaining({ code: 'EISDIR' }));
        expect(() => migrateAgentPinnedTenets(dir, 'test-agent')).toThrow(expect.objectContaining({ code: 'MIGRATION_RECEIPT_UNREADABLE' }));
        expect(fs.existsSync(tenetsFilePath(dir))).toBe(false);
        expect(fs.existsSync(path.join(dir, 'pinned-memory.json'))).toBe(true);
      } finally { fs.rmSync(file, { recursive: true }); }
    } else {
      fs.chmodSync(file, 0o000);
      try {
        // 明确证明夹具产生了 EACCES，权限模型不支持时让该验收失败而非假通过。
        expect(() => fs.readFileSync(file)).toThrow(expect.objectContaining({ code: 'EACCES' }));
        expect(() => migrateAgentPinnedTenets(dir, 'test-agent')).toThrow(expect.objectContaining({ code: 'MIGRATION_RECEIPT_UNREADABLE' }));
        expect(fs.existsSync(tenetsFilePath(dir))).toBe(false);
        expect(fs.existsSync(path.join(dir, 'pinned-memory.json'))).toBe(true);
      } finally { fs.chmodSync(file, 0o600); }
      expect(fs.readFileSync(file)).toEqual(original);
    }
  });

  it.each([
    ['schema string', { schemaVersion: 'future' }],
    ['schema negative', { schemaVersion: -1 }],
    ['schema array', { schemaVersion: [] }],
    ['status array', { status: ['active'] }],
    ['source array', { source: ['user_direct'] }],
  ] as Array<[string, Record<string, unknown>]>)('R01 严格读取拒绝非法 %s，不归一化后覆盖既有目标', (_name, change) => {
    const dir = fixture();
    addTenetDirect(dir, { content: 'existing user item' });
    const target = tenetsFilePath(dir);
    const data = JSON.parse(fs.readFileSync(target, 'utf8'));
    if ('schemaVersion' in change) data.schemaVersion = change.schemaVersion;
    else Object.assign(data.tenets[0], change);
    fs.writeFileSync(target, JSON.stringify(data));
    const before = fs.readFileSync(target);
    expect(() => readTenetsFileStrict(target)).toThrow();
    migrateAgentPinnedTenets(dir, 'test-agent');
    expect(fs.readFileSync(target)).toEqual(before);
    expect(fs.existsSync(path.join(dir, 'pinned-memory.json'))).toBe(true);
    expect(readPinnedTenetsMigrationReceipt(dir)?.state).not.toBe('completed');
  });
});
