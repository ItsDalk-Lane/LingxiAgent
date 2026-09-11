/**
 * C02：迁移/恢复收据 backupDir 的平台无关存储合同。
 *
 * 写侧：新产生/合法续写的收据一律 "/" 分隔（backupDirForReceipt），
 * 不再把本机 path.join 的结果持久化。
 * 读侧：校验接受规范 POSIX 形式与旧 Windows 写入逻辑产生的反斜杠形式；
 * 续跑在内存中转规范形式，只读扫描不改写文件；实际文件访问经
 * receiptBackupDirLocalPath 用本机规则合成路径。
 *
 * 平台限制：本文件在 POSIX（macOS）上运行；「Windows 旧表示」通过把真实
 * 迁移/恢复产出的收据 backupDir 重写为反斜杠形式模拟——缺陷本身是数据格式
 * 级（写入侧把平台分隔符持久化），表示规则与续跑语义可由此完整证明；真实
 * Windows 客户端端到端写入留待该平台实测（不在本环境伪造）。
 */
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash, randomUUID } from 'node:crypto';
import { afterEach, describe, expect, it } from 'vitest';
import {
  migrateAgentPinnedTenets,
  readPinnedTenetsMigrationReceipt,
  type MigrationReceipt,
} from '../core/pinned-tenets-migration.ts';
import { applyPinnedTenetsRecovery } from '../core/pinned-tenets-recovery.ts';
import { parseReceiptBackupDir, receiptBackupDirLocalPath, backupDirForReceipt } from '../core/pinned-tenets-backup-dir.ts';
import { tenetsFilePath, readTenetsFileStrict } from '../lib/memory/tenets.ts';

const homes: string[] = [];
afterEach(() => { for (const home of homes.splice(0)) fs.rmSync(home, { recursive: true, force: true }); });
const sha = (s: string | Buffer) => createHash('sha256').update(s).digest('hex');
const POSIX_BACKUP = /^memory\/pinned-migration-backup(?:\/[a-zA-Z0-9_-]+)?$/;

function fixture(content = 'synthetic item') {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'r-c02-')); homes.push(home);
  const dir = path.join(home, 'agents', 'test-agent'); fs.mkdirSync(path.join(dir, 'memory'), { recursive: true });
  fs.writeFileSync(path.join(dir, 'pinned-memory.json'), JSON.stringify({ version: 1, items: content ? [{ id: 'old', content }] : [] }));
  return { home, dir };
}
const receiptPath = (d: string) => path.join(d, 'memory', 'pinned-tenets-migration.receipt.json');
const operationPath = (d: string, id: string) => path.join(d, 'memory', 'pinned-recovery-operations', `${id}.json`);

function readReceiptRaw(dir: string): MigrationReceipt {
  return JSON.parse(fs.readFileSync(receiptPath(dir), 'utf8')) as MigrationReceipt;
}
function writeReceiptRaw(dir: string, receipt: MigrationReceipt) {
  fs.writeFileSync(receiptPath(dir), JSON.stringify(receipt, null, 2) + '\n');
}
/** 模拟旧 Windows 写入逻辑：把收据 backupDir 换成 path.win32 风格反斜杠形式。 */
function windowsifyBackupDir<T extends { backupDir: string | null }>(receipt: T): T {
  expect(receipt.backupDir).toMatch(POSIX_BACKUP);
  receipt.backupDir = receipt.backupDir!.replace(/\//g, '\\');
  return receipt;
}
function crashAt(checkpoint: string) {
  return { at: (p: string) => { if (p === checkpoint) throw new Error('synthetic crash'); } };
}

describe('C02 写侧：新收据一律 POSIX 规范路径', () => {
  it('完整迁移写出的收据 backupDir 是 "/" 分隔且可原样读回（写→读往返）', () => {
    const { dir } = fixture();
    migrateAgentPinnedTenets(dir, 'test-agent');
    const raw = readReceiptRaw(dir);
    expect(raw.backupDir).toMatch(POSIX_BACKUP);
    expect(raw.state).toBe('completed');
    // 备份文件实际落在该规范路径表示的目录下（本机规则合成）。
    const backupDirPath = receiptBackupDirLocalPath(dir, raw.backupDir);
    expect(fs.existsSync(backupDirPath)).toBe(true);
    // 重新读取（严格校验路径）不抛错，且与磁盘内容一致。
    expect(readPinnedTenetsMigrationReceipt(dir)).toEqual(raw);
  });

  it('合法续写（Windows 旧收据恢复中断后）持久化为规范路径', () => {
    const { dir } = fixture();
    expect(() => migrateAgentPinnedTenets(dir, 'test-agent', crashAt('archive:before:pinned-memory.json'))).toThrow();
    const receipt = windowsifyBackupDir(readReceiptRaw(dir));
    writeReceiptRaw(dir, receipt);
    expect(receipt.backupDir).toContain('\\');
    migrateAgentPinnedTenets(dir, 'test-agent');
    const after = readReceiptRaw(dir);
    expect(after.state).toBe('completed');
    expect(after.backupDir).toMatch(POSIX_BACKUP);
    expect(readTenetsFileStrict(tenetsFilePath(dir)).tenets.some(t => t.content === 'synthetic item')).toBe(true);
  });

  it('恢复操作（recovery）新收据 backupDir 同样是规范路径', () => {
    const { home, dir } = fixture();
    fs.renameSync(path.join(dir, 'pinned-memory.json'), path.join(dir, 'pinned-memory.json.migrated'));
    const source = 'pinned-memory.json.migrated';
    const approval = {
      schemaVersion: 1, operationId: randomUUID(), agentId: 'test-agent',
      sources: [{ file: source, sha256: sha(fs.readFileSync(path.join(dir, source))) }],
      observedTargetHash: fs.existsSync(tenetsFilePath(dir)) ? sha(fs.readFileSync(tenetsFilePath(dir))) : null,
      decisions: [{ source, sourceEntryKey: 'old', contentHash: `sha256:${sha('synthetic item')}`, action: 'restore' as const }],
    };
    applyPinnedTenetsRecovery(home, approval);
    const op = JSON.parse(fs.readFileSync(operationPath(dir, approval.operationId), 'utf8'));
    expect(op.backupDir).toMatch(POSIX_BACKUP);
    expect(op.backupDir).toBe(backupDirForReceipt(approval.operationId));
  });
});

describe('C02 兼容读：v4 迁移收据可证明的续跑状态 × Windows 旧表示', () => {
  it.each([
    ['prepared', 'receipt:prepared'],
    ['committing', 'commit:after'],
    ['target_committed', 'archive:before:pinned-memory.json'],
    ['sources_archived', 'completed:before'],
  ] as const)('Windows 旧表示的 %s 收据可读、可续跑至 completed', (expectedState, checkpoint) => {
    const { dir } = fixture();
    expect(() => migrateAgentPinnedTenets(dir, 'test-agent', crashAt(checkpoint))).toThrow();
    const raw = readReceiptRaw(dir);
    expect(raw.state).toBe(expectedState);
    const receipt = windowsifyBackupDir(raw);
    writeReceiptRaw(dir, receipt);
    // 严格校验读取接受 Windows 旧表示。
    expect(readPinnedTenetsMigrationReceipt(dir)?.state).toBe(expectedState);
    migrateAgentPinnedTenets(dir, 'test-agent');
    const after = readReceiptRaw(dir);
    expect(after.state).toBe('completed');
    expect(after.backupDir).toMatch(POSIX_BACKUP);
    // 归档与目标内容完整。
    expect(fs.existsSync(path.join(dir, 'pinned-memory.json'))).toBe(false);
    expect(fs.existsSync(path.join(dir, 'pinned-memory.json.migrated'))).toBe(true);
    expect(readTenetsFileStrict(tenetsFilePath(dir)).tenets.some(t => t.content === 'synthetic item')).toBe(true);
  });

  it('completed 后删除记忆，再读 Windows 旧收据不复活、不重导', () => {
    const { dir } = fixture();
    migrateAgentPinnedTenets(dir, 'test-agent');
    const receipt = windowsifyBackupDir(readReceiptRaw(dir));
    fs.writeFileSync(tenetsFilePath(dir), JSON.stringify({ schemaVersion: 1, tenets: [] }));
    writeReceiptRaw(dir, receipt);
    const before = fs.readFileSync(receiptPath(dir));
    migrateAgentPinnedTenets(dir, 'test-agent');
    expect(fs.readFileSync(receiptPath(dir))).toEqual(before);
    expect(readTenetsFileStrict(tenetsFilePath(dir)).tenets).toHaveLength(0);
  });

  it('旧 v2 completed 收据（含 Windows backupDir）读取不抛错且不重导', () => {
    const { dir } = fixture();
    migrateAgentPinnedTenets(dir, 'test-agent');
    const raw = readReceiptRaw(dir);
    raw.version = 2;
    delete (raw as Partial<MigrationReceipt>).operationId;
    windowsifyBackupDir(raw);
    fs.writeFileSync(tenetsFilePath(dir), JSON.stringify({ schemaVersion: 1, tenets: [] }));
    writeReceiptRaw(dir, raw);
    migrateAgentPinnedTenets(dir, 'test-agent');
    expect(readTenetsFileStrict(tenetsFilePath(dir)).tenets).toHaveLength(0);
  });

  it('旧 v2 prepared（Windows backupDir）没有目标提交证据，升级保留原件且明确冲突', () => {
    const { dir } = fixture();
    expect(() => migrateAgentPinnedTenets(dir, 'test-agent', crashAt('receipt:prepared'))).toThrow();
    const raw = readReceiptRaw(dir);
    raw.version = 2;
    delete (raw as Partial<MigrationReceipt>).operationId;
    windowsifyBackupDir(raw);
    writeReceiptRaw(dir, raw);
    migrateAgentPinnedTenets(dir, 'test-agent');
    const after = readReceiptRaw(dir);
    expect(after.state).toBe('conflict');
    expect(after.version).toBe(4);
    expect(after.backupDir).toMatch(POSIX_BACKUP);
    expect(fs.existsSync(path.join(dir, 'pinned-memory.json'))).toBe(true);
    expect(fs.existsSync(path.join(dir, 'pinned-memory.json.migrated'))).toBe(false);
    expect(fs.existsSync(tenetsFilePath(dir))).toBe(false);
  });
});

describe('C02 兼容读：v4 恢复操作收据 × Windows 旧表示', () => {
  function setupRecovery() {
    const { home, dir } = fixture();
    fs.renameSync(path.join(dir, 'pinned-memory.json'), path.join(dir, 'pinned-memory.json.migrated'));
    const source = 'pinned-memory.json.migrated';
    const approval = {
      schemaVersion: 1, operationId: randomUUID(), agentId: 'test-agent',
      sources: [{ file: source, sha256: sha(fs.readFileSync(path.join(dir, source))) }],
      observedTargetHash: fs.existsSync(tenetsFilePath(dir)) ? sha(fs.readFileSync(tenetsFilePath(dir))) : null,
      decisions: [{ source, sourceEntryKey: 'old', contentHash: `sha256:${sha('synthetic item')}`, action: 'restore' as const }],
    };
    return { home, dir, approval, source };
  }

  it('target_committed 中断 + Windows 旧表示：续跑完成，批准摘要与 operation 身份不变', () => {
    const f = setupRecovery();
    // 第一轮在目标已提交、完成收据更新前崩溃。
    expect(() => applyPinnedTenetsRecovery(f.home, f.approval, crashAt('completed:before'))).toThrow();
    const before = JSON.parse(fs.readFileSync(operationPath(f.dir, f.approval.operationId), 'utf8'));
    const identityBefore = { operationId: before.operationId, approvalDigest: before.approvalDigest, approval: before.approval, plan: before.plan };
    windowsifyBackupDir(before);
    fs.writeFileSync(operationPath(f.dir, f.approval.operationId), JSON.stringify(before, null, 2) + '\n');
    const summary = applyPinnedTenetsRecovery(f.home, f.approval);
    expect(summary.restored).toBe(1);
    const after = JSON.parse(fs.readFileSync(operationPath(f.dir, f.approval.operationId), 'utf8'));
    expect(after.state).toBe('completed');
    expect(after.operationId).toBe(identityBefore.operationId);
    expect(after.approvalDigest).toBe(identityBefore.approvalDigest);
    expect(after.approval).toEqual(identityBefore.approval);
    expect(after.plan).toEqual(identityBefore.plan);
    expect(after.backupDir).toMatch(POSIX_BACKUP);
    expect(readTenetsFileStrict(tenetsFilePath(f.dir)).tenets.some(t => t.content === 'synthetic item')).toBe(true);
  });

  it('completed 恢复操作 + Windows 旧表示：幂等返回原 summary，不重导', () => {
    const f = setupRecovery();
    const first = applyPinnedTenetsRecovery(f.home, f.approval);
    const raw = JSON.parse(fs.readFileSync(operationPath(f.dir, f.approval.operationId), 'utf8'));
    windowsifyBackupDir(raw);
    fs.writeFileSync(operationPath(f.dir, f.approval.operationId), JSON.stringify(raw, null, 2) + '\n');
    // 用户此后删除了恢复出来的条目：旧收据不得复活。
    fs.writeFileSync(tenetsFilePath(f.dir), JSON.stringify({ schemaVersion: 1, tenets: [] }));
    expect(applyPinnedTenetsRecovery(f.home, f.approval)).toEqual(first);
    expect(readTenetsFileStrict(tenetsFilePath(f.dir)).tenets).toHaveLength(0);
  });
});

describe('C02 拒绝路径：结构先行验证，危险输入一律拒绝', () => {
  it.each([
    ['盘符', 'C:\\memory\\pinned-migration-backup'],
    ['UNC', '\\\\server\\share\\pinned-migration-backup'],
    ['绝对路径', '/memory/pinned-migration-backup'],
    ['路径逃逸(POSIX)', 'memory/pinned-migration-backup/../escape'],
    ['路径逃逸(Windows)', 'memory\\pinned-migration-backup\\..\\escape'],
    ['点段', 'memory/pinned-migration-backup/.'],
    ['双点段', 'memory/pinned-migration-backup/..'],
    ['多余层级', 'memory/pinned-migration-backup/a/b'],
    ['混合分隔符', 'memory/pinned-migration-backup\\abc'],
    ['混合分隔符2', 'memory\\pinned-migration-backup/abc'],
    ['NUL', 'memory/pinned-migration-backup/\0abc'],
    ['前缀伪造', 'memoryx/pinned-migration-backup'],
    ['尾部空白', 'memory/pinned-migration-backup '],
    ['空串', ''],
    ['非字符串', 42],
  ])('%s 被拒绝', (_name, value) => {
    expect(parseReceiptBackupDir(value)).toBeNull();
  });

  it.each([
    'memory/pinned-migration-backup',
    'memory/pinned-migration-backup/0e2d1f2a-1111-2222-3333-444455556666',
    'memory/pinned-migration-backup/restore_1',
    'memory\\pinned-migration-backup',
    'memory\\pinned-migration-backup\\0e2d1f2a-1111-2222-3333-444455556666',
    'memory\\pinned-migration-backup\\restore_1',
  ])('合法表示 %j 被接受并规范化', value => {
    const canonical = parseReceiptBackupDir(value);
    expect(canonical).toBe(String(value).replace(/\\/g, '/'));
  });

  it('危险 backupDir 让收据整体判无效（读取抛错），不落入续跑', () => {
    const { dir } = fixture();
    migrateAgentPinnedTenets(dir, 'test-agent');
    const raw = readReceiptRaw(dir);
    raw.state = 'target_committed';
    raw.backupDir = 'memory/pinned-migration-backup/../../escape';
    writeReceiptRaw(dir, raw);
    expect(() => readPinnedTenetsMigrationReceipt(dir)).toThrow(/invalid migration receipt/i);
    // 源文件保持归档态，没有被续跑移动或重导。
    expect(fs.existsSync(path.join(dir, 'pinned-memory.json'))).toBe(false);
  });

  it('receiptBackupDirLocalPath 拒绝非法输入（明确失败，不猜测路径）', () => {
    expect(() => receiptBackupDirLocalPath('/agents/x', 'C:\\evil')).toThrow(/invalid receipt backupDir/i);
    expect(receiptBackupDirLocalPath('/agents/x', null)).toBe(path.join('/agents/x', 'memory', 'pinned-migration-backup'));
    expect(receiptBackupDirLocalPath('/agents/x', 'memory\\pinned-migration-backup\\abc'))
      .toBe(path.join('/agents/x', 'memory', 'pinned-migration-backup', 'abc'));
  });
});
