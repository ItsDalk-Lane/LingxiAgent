import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { afterEach, describe, expect, it } from 'vitest';
import { applyPinnedTenetsRecovery, scanPinnedTenetsRecovery, type RecoveryApproval } from '../core/pinned-tenets-recovery.ts';
import { migrateAgentPinnedTenets } from '../core/pinned-tenets-migration.ts';
import { addTenetDirect, readTenetsFileStrict, tenetsFilePath } from '../lib/memory/tenets.ts';

const homes: string[] = [];
const hash = (bytes: string | Buffer) => createHash('sha256').update(bytes).digest('hex');
afterEach(() => { for (const home of homes.splice(0)) fs.rmSync(home, { recursive: true, force: true }); });
function setup(items = [{ id: 'a', content: 'first' }, { id: 'b', content: 'second' }]) {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'r02-faults-')); homes.push(home);
  const dir = path.join(home, 'agents', 'test-agent'); fs.mkdirSync(path.join(dir, 'memory'), { recursive: true });
  const source = 'pinned-memory.json.migrated';
  fs.writeFileSync(path.join(dir, source), JSON.stringify({ version: 1, items }));
  return { home, dir, source };
}
function approval(home: string): RecoveryApproval {
  const template = scanPinnedTenetsRecovery(home).agents[0].approvalTemplate;
  return { ...template, decisions: template.decisions.map(decision => ({ ...decision, action: 'restore' })) };
}
const operationPath = (dir: string, a: RecoveryApproval) => path.join(dir, 'memory', 'pinned-recovery-operations', `${a.operationId}.json`);
function interrupt(home: string, a: RecoveryApproval) {
  expect(() => applyPinnedTenetsRecovery(home, a, { at: point => {
    if (point === 'commit:after') throw new Error('synthetic commit interruption');
  } })).toThrow('synthetic commit interruption');
}

describe('R02 恢复事务、兼容与 CLI 边界', () => {
  it('R02-03 两归档源同内容只建一个 active，保留每个批准源条目映射', () => {
    const f = setup([{ id: 'shared', content: 'same content' }]);
    const second = 'pinned-memory.json.migrated-second';
    fs.writeFileSync(path.join(f.dir, second), JSON.stringify({ version: 1, items: [{ id: 'shared', content: 'same content' }] }));
    const a = approval(f.home);
    const sourceBytes = a.sources.map(source => fs.readFileSync(path.join(f.dir, source.file)));
    const summary = applyPinnedTenetsRecovery(f.home, a);
    expect(summary.restored).toBe(1);
    expect(summary.entries).toHaveLength(2);
    expect(new Set(summary.entries.map(entry => entry.tenetId)).size).toBe(1);
    expect(summary.entries.map(entry => entry.contentHash)).toEqual(['sha256:' + hash('same content'), 'sha256:' + hash('same content')]);
    const receipt = JSON.parse(fs.readFileSync(operationPath(f.dir, a), 'utf8'));
    expect(receipt.approval.decisions).toHaveLength(2);
    expect(new Set(receipt.approval.decisions.map((entry: { source: string }) => entry.source))).toEqual(new Set([f.source, second]));
    expect(receipt.plan).toHaveLength(2);
    expect(readTenetsFileStrict(tenetsFilePath(f.dir)).tenets.filter(entry => entry.status === 'active')).toHaveLength(1);
    a.sources.forEach((source, index) => expect(fs.readFileSync(path.join(f.dir, source.file))).toEqual(sourceBytes[index]));
  });

  it('R02-10 commit:after 崩溃后按同批准重试，ID 和目标字节稳定', () => {
    const f = setup(); const a = approval(f.home);
    interrupt(f.home, a);
    const before = fs.readFileSync(tenetsFilePath(f.dir));
    const committing = JSON.parse(fs.readFileSync(operationPath(f.dir, a), 'utf8'));
    expect(committing.state).toBe('committing');
    const result = applyPinnedTenetsRecovery(f.home, a);
    expect(result).toEqual(committing.summary);
    expect(fs.readFileSync(tenetsFilePath(f.dir))).toEqual(before);
    const done = JSON.parse(fs.readFileSync(operationPath(f.dir, a), 'utf8'));
    expect(done.state).toBe('completed');
    expect(done.plan.map((entry: { tenetId: string }) => entry.tenetId)).toEqual(committing.plan.map((entry: { tenetId: string }) => entry.tenetId));
    expect(readTenetsFileStrict(tenetsFilePath(f.dir)).tenets).toHaveLength(2);
  });

  it('R02-11 目标备份权限错误保持来源和目标原字节，无假 completed', () => {
    const f = setup(); addTenetDirect(f.dir, { content: 'prior user data' });
    const a = approval(f.home);
    const sourceBefore = fs.readFileSync(path.join(f.dir, f.source));
    const targetBefore = fs.readFileSync(tenetsFilePath(f.dir));
    expect(() => applyPinnedTenetsRecovery(f.home, a, { at: point => {
      if (point === 'backup:before:tenets.json') throw Object.assign(new Error('synthetic backup EACCES'), { code: 'EACCES' });
    } })).toThrow('synthetic backup EACCES');
    expect(fs.readFileSync(path.join(f.dir, f.source))).toEqual(sourceBefore);
    expect(fs.readFileSync(tenetsFilePath(f.dir))).toEqual(targetBefore);
    expect(fs.existsSync(operationPath(f.dir, a))).toBe(false);
    applyPinnedTenetsRecovery(f.home, a);
    expect(readTenetsFileStrict(tenetsFilePath(f.dir)).tenets).toHaveLength(3);
  });

  it('R02-12 v2 全局 recovery completed 仅标对应条目，其他候选可见且启动不重导', () => {
    const f = setup(); const a = approval(f.home);
    a.decisions[1].action = 'skip';
    applyPinnedTenetsRecovery(f.home, a);
    const operation = operationPath(f.dir, a);
    const legacy = JSON.parse(fs.readFileSync(operation, 'utf8'));
    legacy.version = 2;
    legacy.resultSha256 = null;
    legacy.target = { existed: true, sha256: null };
    const globalPath = path.join(f.dir, 'memory', 'pinned-tenets-migration.receipt.json');
    fs.writeFileSync(globalPath, JSON.stringify(legacy));
    // 模拟升级前只有全局收据的历史布局，删除仅本测试生成的新式收据。
    fs.unlinkSync(operation);
    fs.writeFileSync(tenetsFilePath(f.dir), JSON.stringify({ schemaVersion: 1, tenets: [] }));
    const pinnedOriginal = Buffer.from(JSON.stringify({ version: 1, items: [{ id: 'new', content: 'unhandled original pin' }] }));
    fs.writeFileSync(path.join(f.dir, 'pinned-memory.json'), pinnedOriginal);
    const receiptBefore = fs.readFileSync(globalPath);
    const targetBefore = fs.readFileSync(tenetsFilePath(f.dir));
    const report = scanPinnedTenetsRecovery(f.home).agents[0];
    expect(report.diagnostic).toBe('LEGACY_COMPLETION_UNVERIFIED');
    expect(report.candidates.find(entry => entry.legacyId === 'a')).toMatchObject({ classification: 'previously_restored_now_missing', legacyRestored: true });
    expect(report.candidates.find(entry => entry.legacyId === 'b')).toMatchObject({ classification: 'missing', legacyRestored: false });
    migrateAgentPinnedTenets(f.dir, 'test-agent');
    expect(fs.readFileSync(globalPath)).toEqual(receiptBefore);
    expect(fs.readFileSync(tenetsFilePath(f.dir))).toEqual(targetBefore);
    expect(fs.readFileSync(path.join(f.dir, 'pinned-memory.json'))).toEqual(pinnedOriginal);
  });

  it('R02-14 CLI apply 没有已验证 home 所有权时拒绝，dry-run 只读', () => {
    const f = setup(); const a = approval(f.home);
    const approvalFile = path.join(f.home, 'approval.json'); fs.writeFileSync(approvalFile, JSON.stringify(a));
    const script = path.resolve('scripts/pinned-tenets-recovery.mjs');
    const original = fs.readFileSync(path.join(f.dir, f.source));
    const dry = spawnSync(process.execPath, [script, '--home', f.home], { encoding: 'utf8', timeout: 15_000 });
    expect(dry.status, dry.stderr).toBe(0);
    expect(JSON.parse(dry.stdout).agents[0].candidates).toHaveLength(2);
    const apply = spawnSync(process.execPath, [script, '--home', f.home, '--apply', '--approval', approvalFile], { encoding: 'utf8', timeout: 15_000 });
    expect(apply.status).toBe(1);
    expect(apply.stderr).toContain('BLOCKED_HOME_OWNERSHIP');
    expect(fs.readFileSync(path.join(f.dir, f.source))).toEqual(original);
    expect(fs.readdirSync(path.join(f.dir, 'memory'))).toEqual([]);
  });

  it('R02-16 提交后出现后续用户新增，同批准重试保留新增而不整体恢复旧备份', () => {
    const f = setup(); const a = approval(f.home); interrupt(f.home, a);
    addTenetDirect(f.dir, { content: 'later user item' });
    const before = fs.readFileSync(tenetsFilePath(f.dir));
    applyPinnedTenetsRecovery(f.home, a);
    expect(fs.readFileSync(tenetsFilePath(f.dir))).toEqual(before);
    expect(readTenetsFileStrict(tenetsFilePath(f.dir)).tenets.map(entry => entry.content).sort()).toEqual(['first', 'later user item', 'second']);
  });

  it('R02 收据计划缺项不能缩小原批准范围后虚假 completed', () => {
    const f = setup(); const a = approval(f.home); interrupt(f.home, a);
    const file = operationPath(f.dir, a);
    const receipt = JSON.parse(fs.readFileSync(file, 'utf8'));
    receipt.plan = receipt.plan.slice(0, 1);
    receipt.summary.entries = receipt.plan;
    receipt.summary.restored = 1;
    receipt.counts = { sourceItems: 1, added: 1, duplicateActive: 0, addedOverHistory: 0, duplicateInBatch: 0 };
    fs.writeFileSync(file, JSON.stringify(receipt));
    const target = readTenetsFileStrict(tenetsFilePath(f.dir));
    target.tenets = target.tenets.filter(entry => entry.id === receipt.plan[0].tenetId);
    fs.writeFileSync(tenetsFilePath(f.dir), JSON.stringify(target));
    const before = fs.readFileSync(tenetsFilePath(f.dir));
    expect(() => applyPinnedTenetsRecovery(f.home, a)).toThrow(/receipt|plan|proof|conflict/i);
    expect(fs.readFileSync(tenetsFilePath(f.dir))).toEqual(before);
    expect(JSON.parse(fs.readFileSync(file, 'utf8')).state).not.toBe('completed');
  });
});
