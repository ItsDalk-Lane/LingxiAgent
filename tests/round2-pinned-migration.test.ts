import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { migrateAgentPinnedTenets, readPinnedTenetsMigrationReceipt } from '../core/pinned-tenets-migration.ts';
import { addTenetDirect, tenetsFilePath, readTenetsFileStrict } from '../lib/memory/tenets.ts';
const homes: string[] = [];
afterEach(() => { for (const home of homes.splice(0)) fs.rmSync(home, { recursive: true, force: true }); });
function fixture(content = 'synthetic item') {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'r01-')); homes.push(home);
  const dir = path.join(home, 'agents', 'test-agent'); fs.mkdirSync(path.join(dir, 'memory'), { recursive: true });
  fs.writeFileSync(path.join(dir, 'pinned-memory.json'), JSON.stringify({ version: 1, items: content ? [{id:'old', content}] : [] }));
  return dir;
}
const receiptPath = (d: string) => path.join(d, 'memory', 'pinned-tenets-migration.receipt.json');
function stop(dir: string, checkpoint = 'archive:before:pinned-memory.json') {
  expect(() => migrateAgentPinnedTenets(dir, 'test-agent', { at: p => { if (p === checkpoint) throw new Error('synthetic crash'); } })).toThrow('synthetic crash');
}
function changeTarget(dir: string, fn: (data: ReturnType<typeof readTenetsFileStrict>) => void) {
  const data = readTenetsFileStrict(tenetsFilePath(dir)); fn(data); fs.writeFileSync(tenetsFilePath(dir), JSON.stringify(data));
}
describe('R01 真实迁移后置条件与归档恢复', () => {
  it('R01-01 E02 duplicate-only 目标被清空不得认证完成', () => {
    const d = fixture(); addTenetDirect(d, {content:'synthetic item'}); stop(d);
    changeTarget(d, t => { t.tenets = []; }); migrateAgentPinnedTenets(d, 'test-agent');
    expect(readPinnedTenetsMigrationReceipt(d)?.state).toBe('conflict'); expect(fs.existsSync(path.join(d,'pinned-memory.json'))).toBe(true);
  });
  it.each(['content','pending','rejected','source','createdAt'] as const)('R01-02/R01-03 保留 ID 但改变 %s 不得归档', field => {
    const d = fixture(); stop(d);
    changeTarget(d, t => { const item = t.tenets[0]; if (field === 'content') item.content = 'user edited'; else if (field === 'source') item.source = 'model_proposed'; else if (field === 'createdAt') item.createdAt = '2020-01-01T00:00:00.000Z'; else item.status = field; });
    migrateAgentPinnedTenets(d,'test-agent'); expect(readPinnedTenetsMigrationReceipt(d)?.state).toBe('conflict');
  });
  it('R01-04 同一个目标映射两种哈希拒绝继续', () => {
    const d=fixture(); stop(d); const r=JSON.parse(fs.readFileSync(receiptPath(d),'utf8'));
    r.plan.push({...r.plan[0],order:1,contentHash:'sha256:'+'0'.repeat(64)}); r.counts.sourceItems=2; fs.writeFileSync(receiptPath(d),JSON.stringify(r));
    expect(() => migrateAgentPinnedTenets(d,'test-agent')).toThrow(/receipt|plan|conflict/i); expect(fs.existsSync(path.join(d,'pinned-memory.json'))).toBe(true);
  });
  it('R01-05 后续新增不覆盖', () => { const d=fixture(); stop(d); addTenetDirect(d,{content:'later'}); migrateAgentPinnedTenets(d,'test-agent'); expect(readTenetsFileStrict(tenetsFilePath(d)).tenets).toHaveLength(2); expect(readPinnedTenetsMigrationReceipt(d)?.state).toBe('completed'); });
  it('R01-06 合法空源完成', () => { const d=fixture(''); migrateAgentPinnedTenets(d,'test-agent'); expect(readPinnedTenetsMigrationReceipt(d)?.state).toBe('completed'); });
  it('R01-07 E03 空计划 prepared 后目标改变不能 every=true', () => { const d=fixture(''); stop(d,'receipt:prepared'); addTenetDirect(d,{content:'later'}); migrateAgentPinnedTenets(d,'test-agent'); expect(readPinnedTenetsMigrationReceipt(d)?.state).toBe('conflict'); });
  it('R01-09 E04 归档移走后收据未写可恢复', () => {
    const d=fixture(); stop(d); const r=JSON.parse(fs.readFileSync(receiptPath(d),'utf8'));
    const to=r.archived[0]?.to ?? 'pinned-memory.json.migrated';
    fs.renameSync(path.join(d,'pinned-memory.json'),path.join(d,to));
    migrateAgentPinnedTenets(d,'test-agent'); expect(readPinnedTenetsMigrationReceipt(d)?.state).toBe('completed');
  });
  it('R01-11 规划目的地被占用时保留源和目的地', () => { const d=fixture(); stop(d); const r=JSON.parse(fs.readFileSync(receiptPath(d),'utf8')); const to=r.archived[0]?.to ?? 'pinned-memory.json.migrated'; fs.writeFileSync(path.join(d,to),'other data'); try { migrateAgentPinnedTenets(d,'test-agent'); } catch {} expect(fs.existsSync(path.join(d,'pinned-memory.json'))).toBe(true); expect(fs.readFileSync(path.join(d,to),'utf8')).toBe('other data'); });
  it.each(['{bad', JSON.stringify({version:99,state:'prepared'}),JSON.stringify({version:3,state:'mystery'})])('R01-13 非法收据不当 absent %s', raw => {const d=fixture(); fs.writeFileSync(receiptPath(d),raw); expect(() => migrateAgentPinnedTenets(d,'test-agent')).toThrow(); expect(fs.readFileSync(receiptPath(d),'utf8')).toBe(raw); expect(fs.existsSync(tenetsFilePath(d))).toBe(false); });
  it('R01-14 v2 completed 后删除不复活不改收据', () => {const d=fixture(); migrateAgentPinnedTenets(d,'test-agent'); const r=JSON.parse(fs.readFileSync(receiptPath(d),'utf8')); r.version=2; fs.writeFileSync(receiptPath(d),JSON.stringify(r)); const before=fs.readFileSync(receiptPath(d)); changeTarget(d,t=>{t.tenets=[];}); migrateAgentPinnedTenets(d,'test-agent'); expect(fs.readFileSync(receiptPath(d))).toEqual(before); expect(readTenetsFileStrict(tenetsFilePath(d)).tenets).toEqual([]); });
  it('R01-15 prepared 前目标原字节备份可恢复', () => { const d=fixture(); addTenetDirect(d,{content:'original'}); const before=fs.readFileSync(tenetsFilePath(d)); stop(d,'receipt:prepared'); const r=readPinnedTenetsMigrationReceipt(d)!; const files=fs.readdirSync(path.join(d,r.backupDir!),{recursive:true}); expect(files.some(f => {const p=path.join(d,r.backupDir!,String(f)); return fs.statSync(p).isFile() && fs.readFileSync(p).equals(before);})).toBe(true); });
  it('R01-严格读取不默默过滤损坏条目', () => {const d=fixture(); fs.writeFileSync(tenetsFilePath(d),JSON.stringify({schemaVersion:1,tenets:[null]})); expect(()=>readTenetsFileStrict(tenetsFilePath(d))).toThrow();});
});

describe('R01 补充计划和故障窗口', () => {
  it('R01-计划映射缺项不能被结果哈希捷径认证', () => { const d=fixture(); stop(d); const r=JSON.parse(fs.readFileSync(receiptPath(d),'utf8')); r.plan=[]; r.counts.sourceItems=0; fs.writeFileSync(receiptPath(d),JSON.stringify(r)); expect(()=>migrateAgentPinnedTenets(d,'test-agent')).toThrow(/plan|receipt|conflict/i); expect(fs.existsSync(path.join(d,'pinned-memory.json'))).toBe(true); });
  it.each(['backup:before:tenets.json','backup:after:tenets.json','receipt:prepared','commit:before','commit:after','archive:copied:pinned-memory.json','archive:after:pinned-memory.json','completed:before'])('R01-08 写边界 %s 故障保留数据且可恢复', point => {
    const d=fixture(); addTenetDirect(d,{content:'original'});
    try { migrateAgentPinnedTenets(d,'test-agent',{at:p=>{if(p===point) throw new Error('synthetic crash');}}); } catch {}
    expect(readPinnedTenetsMigrationReceipt(d)?.state).not.toBe('completed');
    migrateAgentPinnedTenets(d,'test-agent');
    expect(readPinnedTenetsMigrationReceipt(d)?.state).toBe('completed'); expect(readTenetsFileStrict(tenetsFilePath(d)).tenets.map(t=>t.content).sort()).toEqual(['original','synthetic item']);
  });
  it('R01-12 v2 唯一归档证据转换后恢复', () => {const d=fixture(); stop(d); const r=JSON.parse(fs.readFileSync(receiptPath(d),'utf8')); r.version=2; r.archived=[]; fs.renameSync(path.join(d,'pinned-memory.json'),path.join(d,'pinned-memory.json.migrated')); fs.writeFileSync(receiptPath(d),JSON.stringify(r)); migrateAgentPinnedTenets(d,'test-agent'); expect(readPinnedTenetsMigrationReceipt(d)?.state).toBe('completed'); });
  it('R01-12 v2 多个相同归档路径不猜选', () => {const d=fixture(); stop(d); const r=JSON.parse(fs.readFileSync(receiptPath(d),'utf8')); r.version=2; r.archived=[]; fs.renameSync(path.join(d,'pinned-memory.json'),path.join(d,'pinned-memory.json.migrated')); fs.copyFileSync(path.join(d,'pinned-memory.json.migrated'),path.join(d,'pinned-memory.json.migrated-12345678')); fs.writeFileSync(receiptPath(d),JSON.stringify(r)); migrateAgentPinnedTenets(d,'test-agent'); expect(readPinnedTenetsMigrationReceipt(d)?.state).toBe('conflict'); });
});
