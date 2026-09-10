import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  migrateAgentPinnedTenets, migratePinnedMemoryToTenets, readPinnedTenetsMigrationReceipt,
} from '../core/pinned-tenets-migration.ts';
import { applyPinnedTenetsRecovery, scanPinnedTenetsRecovery } from '../core/pinned-tenets-recovery.ts';
import { addTenetDirect, readTenetsFileStrict, removeTenet, tenetsFilePath } from '../lib/memory/tenets.ts';

const homes: string[] = [];
afterEach(() => {
  vi.restoreAllMocks();
  for (const home of homes.splice(0)) fs.rmSync(home, { recursive: true, force: true });
});

function fixture(archived = false, items: unknown[] = [{ id: 'legacy', content: '需要迁移的内容' }]) {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'lingxi-pinned-contract-'));
  homes.push(home);
  const dir = path.join(home, 'agents', 'test-agent');
  fs.mkdirSync(dir, { recursive: true });
  addTenetDirect(dir, { content: '原有用户内容' });
  const source = path.join(dir, `pinned-memory.json${archived ? '.migrated' : ''}`);
  fs.writeFileSync(source, JSON.stringify({ version: 1, items }));
  const target = tenetsFilePath(dir);
  const receipt = path.join(dir, 'memory', 'pinned-tenets-migration.receipt.json');
  return { home, dir, source, target, receipt };
}

function crashAt(checkpoint: string) {
  return { at(point: string) { if (point === checkpoint) throw new Error(`中断：${checkpoint}`); } };
}

function recoveryApproval(home: string) {
  const approval = scanPinnedTenetsRecovery(home).agents[0].approvalTemplate;
  approval.decisions.forEach(decision => { decision.action = 'restore'; });
  return approval;
}

function deleteImported(dir: string) {
  const imported = readTenetsFileStrict(tenetsFilePath(dir)).tenets.find(item => item.content === '需要迁移的内容');
  expect(imported).toBeDefined();
  removeTenet(dir, imported!.id);
}

describe('迁移提交证据与用户后续删除', () => {
  it('启动写提交收据失败后继续运行；用户删回原字节，下一次启动不能重新导入', () => {
    const f = fixture();
    const original = fs.readFileSync(f.target);
    const rename = fs.renameSync;
    let injected = false;
    const fault = vi.spyOn(fs, 'renameSync').mockImplementation((from, to) => {
      if (!injected && String(to) === f.receipt
        && JSON.parse(fs.readFileSync(from, 'utf8')).state === 'target_committed') {
        injected = true;
        throw Object.assign(new Error('合成收据写入失败'), { code: 'EIO' });
      }
      return rename(from, to);
    });
    expect(() => migratePinnedMemoryToTenets(f.home)).not.toThrow();
    fault.mockRestore();
    expect(injected).toBe(true);
    deleteImported(f.dir);
    expect(fs.readFileSync(f.target)).toEqual(original);
    migratePinnedMemoryToTenets(f.home);
    expect(fs.readFileSync(f.target)).toEqual(original);
    expect(readPinnedTenetsMigrationReceipt(f.dir)?.state).toBe('conflict');
    expect(fs.existsSync(f.source)).toBe(true);
  });

  it('新 prepared 在尝试提交之前中断，可在原快照上继续', () => {
    const f = fixture();
    expect(() => migrateAgentPinnedTenets(f.dir, 'test-agent', crashAt('commit:before'))).toThrow('中断');
    expect(readPinnedTenetsMigrationReceipt(f.dir)).toMatchObject({ version: 4, state: 'prepared' });
    migrateAgentPinnedTenets(f.dir, 'test-agent');
    expect(readPinnedTenetsMigrationReceipt(f.dir)?.state).toBe('completed');
    expect(readTenetsFileStrict(f.target).tenets).toHaveLength(2);
  });

  it('提交意图已落盘而目标未写时中断，原字节不足以授权自动重放', () => {
    const f = fixture();
    const original = fs.readFileSync(f.target);
    expect(() => migrateAgentPinnedTenets(f.dir, 'test-agent', crashAt('commit:intent:after'))).toThrow('中断');
    expect(readPinnedTenetsMigrationReceipt(f.dir)).toMatchObject({ version: 4, state: 'committing' });
    migrateAgentPinnedTenets(f.dir, 'test-agent');
    expect(readPinnedTenetsMigrationReceipt(f.dir)?.state).toBe('conflict');
    expect(fs.readFileSync(f.target)).toEqual(original);
    expect(fs.existsSync(f.source)).toBe(true);
  });

  it('真实子进程在提交意图落盘后被硬终止，重启不猜测提交结果', () => {
    const f = fixture();
    const original = fs.readFileSync(f.target);
    const moduleUrl = pathToFileURL(path.resolve('core/pinned-tenets-migration.ts')).href;
    const script = `import { migrateAgentPinnedTenets } from ${JSON.stringify(moduleUrl)};
      migrateAgentPinnedTenets(${JSON.stringify(f.dir)}, 'test-agent', { at(point) {
        if (point === 'commit:intent:after') process.kill(process.pid, 'SIGKILL');
      }});`;
    const child = spawnSync(process.execPath, ['--input-type=module', '--eval', script], { encoding: 'utf8', timeout: 15_000 });
    expect(child.error).toBeUndefined();
    if (process.platform === 'win32') expect(child.status, child.stderr).not.toBe(0);
    else expect(child.signal, child.stderr).toBe('SIGKILL');
    expect(readPinnedTenetsMigrationReceipt(f.dir)?.state).toBe('committing');
    migratePinnedMemoryToTenets(f.home);
    expect(readPinnedTenetsMigrationReceipt(f.dir)?.state).toBe('conflict');
    expect(fs.readFileSync(f.target)).toEqual(original);
    expect(fs.existsSync(f.source)).toBe(true);
  });

  it('提交后收据未更新且用户新增其他内容，只补结算并保留新增', () => {
    const f = fixture();
    expect(() => migrateAgentPinnedTenets(f.dir, 'test-agent', crashAt('commit:after'))).toThrow('中断');
    addTenetDirect(f.dir, { content: '之后新增' });
    const before = fs.readFileSync(f.target);
    migrateAgentPinnedTenets(f.dir, 'test-agent');
    expect(fs.readFileSync(f.target)).toEqual(before);
    expect(readPinnedTenetsMigrationReceipt(f.dir)?.state).toBe('completed');
  });

  it.each([2, 3])('旧 v%s prepared 只有原快照，保留资料并明确冲突', version => {
    const f = fixture();
    const original = fs.readFileSync(f.target);
    expect(() => migrateAgentPinnedTenets(f.dir, 'test-agent', crashAt('receipt:prepared'))).toThrow('中断');
    const receipt = JSON.parse(fs.readFileSync(f.receipt, 'utf8'));
    receipt.version = version;
    fs.writeFileSync(f.receipt, JSON.stringify(receipt));
    migrateAgentPinnedTenets(f.dir, 'test-agent');
    expect(readPinnedTenetsMigrationReceipt(f.dir)?.state).toBe('conflict');
    expect(fs.readFileSync(f.target)).toEqual(original);
    expect(fs.existsSync(f.source)).toBe(true);
  });

  it.each([2, 3])('旧 v%s prepared 的完整目标已经存在，可以只补结算', version => {
    const f = fixture();
    expect(() => migrateAgentPinnedTenets(f.dir, 'test-agent', crashAt('commit:after'))).toThrow('中断');
    const receipt = JSON.parse(fs.readFileSync(f.receipt, 'utf8'));
    receipt.version = version;
    receipt.state = 'prepared';
    fs.writeFileSync(f.receipt, JSON.stringify(receipt));
    const before = fs.readFileSync(f.target);
    migrateAgentPinnedTenets(f.dir, 'test-agent');
    expect(readPinnedTenetsMigrationReceipt(f.dir)?.state).toBe('completed');
    expect(fs.readFileSync(f.target)).toEqual(before);
  });
});

describe('恢复不因目标回到旧字节而重执行', () => {
  it('新 prepared 恢复操作尚未尝试提交时，可沿原批准继续且计划身份不变', () => {
    const f = fixture(true);
    const approval = recoveryApproval(f.home);
    expect(() => applyPinnedTenetsRecovery(f.home, approval, crashAt('receipt:prepared'))).toThrow('中断');
    const operation = path.join(f.dir, 'memory', 'pinned-recovery-operations', `${approval.operationId}.json`);
    const prepared = JSON.parse(fs.readFileSync(operation, 'utf8'));
    expect(prepared).toMatchObject({ version: 4, state: 'prepared' });
    expect(applyPinnedTenetsRecovery(f.home, approval)).toEqual(prepared.summary);
    expect(readTenetsFileStrict(f.target).tenets).toHaveLength(2);
    expect(JSON.parse(fs.readFileSync(operation, 'utf8')).state).toBe('completed');
  });

  it.each(['commit:after', 'completed:before'])('%s 中断后用户删除，再用同一操作不能复活', checkpoint => {
    const f = fixture(true);
    const original = fs.readFileSync(f.target);
    const approval = recoveryApproval(f.home);
    expect(() => applyPinnedTenetsRecovery(f.home, approval, crashAt(checkpoint))).toThrow('中断');
    deleteImported(f.dir);
    expect(fs.readFileSync(f.target)).toEqual(original);
    expect(() => applyPinnedTenetsRecovery(f.home, approval)).toThrow(/conflict/i);
    expect(fs.readFileSync(f.target)).toEqual(original);
    const receipt = JSON.parse(fs.readFileSync(path.join(f.dir, 'memory', 'pinned-recovery-operations', `${approval.operationId}.json`), 'utf8'));
    expect(receipt.state).toBe('conflict');
  });

  it.each(['删除其中一项', '修改恢复正文'])('恢复已提交后%s，同一操作不得重执行或假完成', change => {
    const f = fixture(true, [{ id: 'first', content: '需要迁移的内容' }, { id: 'second', content: '另一项' }]);
    const approval = recoveryApproval(f.home);
    expect(() => applyPinnedTenetsRecovery(f.home, approval, crashAt('completed:before'))).toThrow('中断');
    if (change === '删除其中一项') deleteImported(f.dir);
    else {
      const target = readTenetsFileStrict(f.target);
      target.tenets.find(item => item.content === '需要迁移的内容')!.content = '用户修改后的正文';
      fs.writeFileSync(f.target, JSON.stringify(target));
    }
    const before = fs.readFileSync(f.target);
    expect(() => applyPinnedTenetsRecovery(f.home, approval)).toThrow(/conflict/i);
    expect(fs.readFileSync(f.target)).toEqual(before);
    expect(() => applyPinnedTenetsRecovery(f.home, approval)).toThrow(/conflict/i);
    expect(fs.readFileSync(f.target)).toEqual(before);
  });

  it('恢复提交意图落盘后目标未写，不将旧目标冒充为未尝试提交', () => {
    const f = fixture(true);
    const original = fs.readFileSync(f.target);
    const approval = recoveryApproval(f.home);
    expect(() => applyPinnedTenetsRecovery(f.home, approval, crashAt('commit:intent:after'))).toThrow('中断');
    expect(() => applyPinnedTenetsRecovery(f.home, approval)).toThrow(/conflict/i);
    expect(fs.readFileSync(f.target)).toEqual(original);
  });

  it('旧 v3 prepared 恢复操作缺少完整目标证明时，不重放批准', () => {
    const f = fixture(true);
    const original = fs.readFileSync(f.target);
    const approval = recoveryApproval(f.home);
    expect(() => applyPinnedTenetsRecovery(f.home, approval, crashAt('receipt:prepared'))).toThrow('中断');
    const operation = path.join(f.dir, 'memory', 'pinned-recovery-operations', `${approval.operationId}.json`);
    const receipt = JSON.parse(fs.readFileSync(operation, 'utf8'));
    receipt.version = 3;
    fs.writeFileSync(operation, JSON.stringify(receipt));
    expect(() => applyPinnedTenetsRecovery(f.home, approval)).toThrow(/conflict/i);
    expect(fs.readFileSync(f.target)).toEqual(original);
    expect(JSON.parse(fs.readFileSync(operation, 'utf8')).state).toBe('conflict');
  });

  it('旧 v3 prepared 的恢复目标已完整时，只补收据而不重写目标', () => {
    const f = fixture(true);
    const approval = recoveryApproval(f.home);
    expect(() => applyPinnedTenetsRecovery(f.home, approval, crashAt('commit:after'))).toThrow('中断');
    const operation = path.join(f.dir, 'memory', 'pinned-recovery-operations', `${approval.operationId}.json`);
    const receipt = JSON.parse(fs.readFileSync(operation, 'utf8'));
    receipt.version = 3;
    receipt.state = 'prepared';
    fs.writeFileSync(operation, JSON.stringify(receipt));
    const before = fs.readFileSync(f.target);
    expect(applyPinnedTenetsRecovery(f.home, approval)).toEqual(receipt.summary);
    expect(fs.readFileSync(f.target)).toEqual(before);
    expect(JSON.parse(fs.readFileSync(operation, 'utf8')).state).toBe('completed');
  });

  it('含下划线的操作身份经过批准、写入与再次读取保持一致', () => {
    const f = fixture(true);
    const approval = recoveryApproval(f.home);
    approval.operationId = 'restore_1';
    const first = applyPinnedTenetsRecovery(f.home, approval);
    expect(first.restored).toBe(1);
    expect(applyPinnedTenetsRecovery(f.home, approval)).toEqual(first);
    const receipt = JSON.parse(fs.readFileSync(path.join(f.dir, 'memory', 'pinned-recovery-operations', 'restore_1.json'), 'utf8'));
    expect(receipt.backupDir).toBe('memory/pinned-migration-backup/restore_1');
  });
});

describe('损坏来源不能变成合法空计划', () => {
  it.each([
    [null],
    [{ id: 'bad' }],
    [{ id: 'good', content: '应保留的有效内容' }, { id: 'bad', content: ' \r\n ' }],
  ])('来源包含坏项时整体失败，原件和目标不改：%j', (...items) => {
    const f = fixture(false, items);
    const source = fs.readFileSync(f.source);
    const target = fs.readFileSync(f.target);
    migrateAgentPinnedTenets(f.dir, 'test-agent');
    expect(readPinnedTenetsMigrationReceipt(f.dir)).toMatchObject({ state: 'failed', error: { code: 'MIGRATION_SOURCE_UNREADABLE' } });
    expect(fs.readFileSync(f.source)).toEqual(source);
    expect(fs.readFileSync(f.target)).toEqual(target);
    expect(fs.existsSync(`${f.source}.migrated`)).toBe(false);
  });

  it('恢复扫描拒绝坏项，而非给出空候选并掩盖损坏', () => {
    const f = fixture(true, [null]);
    const before = fs.readFileSync(f.source);
    expect(() => scanPinnedTenetsRecovery(f.home)).toThrow();
    expect(fs.readFileSync(f.source)).toEqual(before);
  });

  it('保留旧运行时允许的非空内容归一化，合法空数组仍可完成', () => {
    const numeric = fixture(false, [{ id: 'old', content: 42 }]);
    migrateAgentPinnedTenets(numeric.dir, 'test-agent');
    expect(readTenetsFileStrict(numeric.target).tenets.some(item => item.content === '42')).toBe(true);
    const empty = fixture(false, []);
    migrateAgentPinnedTenets(empty.dir, 'test-agent');
    expect(readPinnedTenetsMigrationReceipt(empty.dir)).toMatchObject({ state: 'completed', counts: { sourceItems: 0 } });
  });
});
