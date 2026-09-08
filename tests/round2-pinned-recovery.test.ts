import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash, randomUUID } from 'node:crypto';
import { afterEach, describe, expect, it } from 'vitest';
import { applyPinnedTenetsRecovery, scanPinnedTenetsRecovery } from '../core/pinned-tenets-recovery.ts';
import { importLegacyPinnedItems, tenetsFilePath, readTenetsFileStrict, addTenetDirect, removeTenet } from '../lib/memory/tenets.ts';
import { migrateAgentPinnedTenets, readPinnedTenetsMigrationReceipt } from '../core/pinned-tenets-migration.ts';
const homes: string[]=[];
afterEach(()=>{for(const h of homes.splice(0)) fs.rmSync(h,{recursive:true,force:true});});
const hash=(s: string | Buffer)=>createHash('sha256').update(s).digest('hex');
function setup(){const home=fs.mkdtempSync(path.join(os.tmpdir(),'r02-'));homes.push(home);const dir=path.join(home,'agents','test-agent');fs.mkdirSync(path.join(dir,'memory'),{recursive:true});const source='pinned-memory.json.migrated';fs.writeFileSync(path.join(dir,source),JSON.stringify({version:1,items:[{id:'a',content:'first'},{id:'b',content:'second'}]}));return {home,dir,source};}
function approval(f:ReturnType<typeof setup>){return {schemaVersion:1,operationId:randomUUID(),agentId:'test-agent',source:f.source,sources:[{file:f.source,sha256:hash(fs.readFileSync(path.join(f.dir,f.source)))}],observedTargetHash:fs.existsSync(tenetsFilePath(f.dir))?hash(fs.readFileSync(tenetsFilePath(f.dir))):null,decisions:[{source:f.source,sourceEntryKey:'a',contentHash:'sha256:'+hash('first'),action:'restore' as const}]};}
describe('R02 批准快照、范围与幂等',()=>{
 it('X2-01 pending/active 混合旧库经首次迁移、恢复重试、删除与重启后无重复或复活',()=>{
  const f=setup();
  const active=addTenetDirect(f.dir,{content:'first'}).tenet;
  fs.writeFileSync(tenetsFilePath(f.dir),JSON.stringify({schemaVersion:1,tenets:[{...active,id:'pending',status:'pending'},active]}));
  fs.renameSync(path.join(f.dir,f.source),path.join(f.dir,'pinned-memory.json'));
  migrateAgentPinnedTenets(f.dir,'test-agent');
  const migration=readPinnedTenetsMigrationReceipt(f.dir)!;
  expect(migration.state).toBe('completed');
  expect(migration.plan.find(entry=>entry.legacyId==='a')?.tenetId).toBe(active.id);
  expect(readTenetsFileStrict(tenetsFilePath(f.dir)).tenets.filter(item=>item.content==='first')).toHaveLength(2);

  const recoverySource='pinned-memory.json.migrated-extra';
  fs.writeFileSync(path.join(f.dir,recoverySource),JSON.stringify({version:1,items:[{id:'c',content:'third'}]}));
  const report=scanPinnedTenetsRecovery(f.home).agents[0];
  const candidate=report.candidates.find(item=>item.legacyId==='c')!;
  const request={...report.approvalTemplate,decisions:report.approvalTemplate.decisions.map(item=>(
   item.source===candidate.source&&item.sourceEntryKey===candidate.sourceEntryKey?{...item,action:'restore' as const}:item
  ))};
  const first=applyPinnedTenetsRecovery(f.home,request);
  expect(applyPinnedTenetsRecovery(f.home,request)).toEqual(first);
  const restored=readTenetsFileStrict(tenetsFilePath(f.dir)).tenets.find(item=>item.content==='third')!;
  expect(removeTenet(f.dir,restored.id)).toBe(true);

  migrateAgentPinnedTenets(f.dir,'test-agent');
  expect(applyPinnedTenetsRecovery(f.home,request)).toEqual(first);
  const final=readTenetsFileStrict(tenetsFilePath(f.dir)).tenets;
  expect(final.filter(item=>item.content==='first')).toHaveLength(2);
  expect(final.some(item=>item.content==='third')).toBe(false);
  expect(readPinnedTenetsMigrationReceipt(f.dir)).toEqual(migration);
 });
 it('R02-01 E11 pending 在前 active 在后仍复用 active',()=>{const f=setup();const a=addTenetDirect(f.dir,{content:'first'}).tenet;fs.writeFileSync(tenetsFilePath(f.dir),JSON.stringify({schemaVersion:1,tenets:[{...a,id:'pending',status:'pending'},a]}));const r=importLegacyPinnedItems(f.dir,[{legacyId:'old',content:'first'}]);expect(r.entries[0].tenetId).toBe(a.id);expect(readTenetsFileStrict(tenetsFilePath(f.dir)).tenets).toHaveLength(2);});
 it('R02-02 rejected 历史在前三次批量导入只增加一个 active',()=>{const f=setup();const a=addTenetDirect(f.dir,{content:'first'}).tenet;fs.writeFileSync(tenetsFilePath(f.dir),JSON.stringify({schemaVersion:1,tenets:[{...a,status:'rejected'}]}));for(let i=0;i<3;i++)importLegacyPinnedItems(f.dir,[{legacyId:'old',content:'first'}]);expect(readTenetsFileStrict(tenetsFilePath(f.dir)).tenets.filter(t=>t.status==='active')).toHaveLength(1);});
 it('R02-04 只恢复批准部分，其他候选仍显示',()=>{const f=setup();applyPinnedTenetsRecovery(f.home,approval(f));const report=scanPinnedTenetsRecovery(f.home);expect(report.agents[0]?.candidates.find(c=>c.legacyId==='b')?.classification).toBe('missing');});
 it('R02-05 同一 operation 返回原 summary 不再写库',()=>{const f=setup();const a=approval(f);const first=applyPinnedTenetsRecovery(f.home,a);const before=fs.readFileSync(tenetsFilePath(f.dir));expect(applyPinnedTenetsRecovery(f.home,a)).toEqual(first);expect(fs.readFileSync(tenetsFilePath(f.dir))).toEqual(before);});
 it('R02-06 同一 operation 改批准决策拒绝',()=>{const f=setup();const a=approval(f);applyPinnedTenetsRecovery(f.home,a);const changed={...a,decisions:[...a.decisions,{source:f.source,sourceEntryKey:'b',contentHash:'sha256:'+hash('second'),action:'restore' as const}]};expect(()=>applyPinnedTenetsRecovery(f.home,changed)).toThrow(/approval|operation/i);});
 it('R02-07 完成后删除再用原单不能复活',()=>{const f=setup();const a=approval(f);applyPinnedTenetsRecovery(f.home,a);fs.writeFileSync(tenetsFilePath(f.dir),JSON.stringify({schemaVersion:1,tenets:[]}));applyPinnedTenetsRecovery(f.home,a);expect(readTenetsFileStrict(tenetsFilePath(f.dir)).tenets).toHaveLength(0);});
 it('R02-08 新批准操作才允许恢复用户删除内容',()=>{const f=setup();applyPinnedTenetsRecovery(f.home,approval(f));fs.writeFileSync(tenetsFilePath(f.dir),JSON.stringify({schemaVersion:1,tenets:[]}));applyPinnedTenetsRecovery(f.home,approval(f));expect(readTenetsFileStrict(tenetsFilePath(f.dir)).tenets).toHaveLength(1);});
 it.each(['source','target'])('R02-09 批准后 %s 改变拒绝写入',kind=>{const f=setup();const a=approval(f);if(kind==='source')fs.appendFileSync(path.join(f.dir,f.source),' ');else addTenetDirect(f.dir,{content:'later'});const before=fs.existsSync(tenetsFilePath(f.dir))?fs.readFileSync(tenetsFilePath(f.dir)):null;expect(()=>applyPinnedTenetsRecovery(f.home,a)).toThrow(/stale_approval|conflict/i);expect(fs.existsSync(tenetsFilePath(f.dir))?fs.readFileSync(tenetsFilePath(f.dir)):null).toEqual(before);});
 it('R02-13 收据按 operation 存放，不覆盖首次迁移位置',()=>{const f=setup();const a=approval(f);applyPinnedTenetsRecovery(f.home,a);expect(fs.existsSync(path.join(f.dir,'memory','pinned-recovery-operations',a.operationId+'.json'))).toBe(true);expect(fs.existsSync(path.join(f.dir,'memory','pinned-tenets-migration.receipt.json'))).toBe(false);expect(fs.existsSync(path.join(f.dir,f.source))).toBe(true);});
});
