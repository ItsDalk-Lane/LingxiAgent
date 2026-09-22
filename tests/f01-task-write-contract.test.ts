import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { describe, it, expect, vi } from 'vitest';
import { TaskRegistry } from '../lib/task-registry.ts';
import { registerTaskRegistryBusHandlers } from '../server/task-bus-handlers.ts';

describe('F01 全写入口生命周期', () => {
  for (const terminal of ['completed','failed','aborted','canceled']) {
    it(`C01/C02/C03 ${terminal} 不被任何终态写入口改写`, () => {
      const reg = new TaskRegistry(); const abort = vi.fn();
      reg.registerHandler('x', { abort }); reg.register('t', { type:'x' });
      if (terminal === 'completed') reg.complete('t','first');
      if (terminal === 'failed') reg.fail('t','first');
      if (terminal === 'aborted') reg.abort('t');
      if (terminal === 'canceled') reg.cancel('t');
      const before = reg.query('t'); const calls = abort.mock.calls.length;
      reg.complete('t','late'); reg.fail('t','late');
      reg.update('t',{status:'running',result:'late',error:'late'});
      reg.abort('t'); reg.cancel('t');
      expect(reg.query('t')).toEqual(before); expect(abort).toHaveBeenCalledTimes(calls);
      reg.update('t', {meta:{label:'shown'}});
      expect(reg.query('t').meta.label).toBe('shown');
    });
  }
  it('C04 复用后旧、缺失、null 批次的全部写入口被拒绝', () => {
    const reg = new TaskRegistry(); reg.registerHandler('x',{abort:vi.fn()});
    reg.register('t',{type:'x'}); reg.complete('t'); reg.register('t',{type:'x'});
    for (const expectedAttempt of [1,undefined,null]) {
      const options = {expectedAttempt};
      reg.update('t',{status:'failed'},options); reg.complete('t','late',options); reg.fail('t','late',options);
      reg.abort('t','late',options); reg.cancel('t','late',options); reg.remove('t',options);
      expect(reg.query('t')).toMatchObject({status:'running',attempt:2});
    }
    for (const bad of [0,-1,1.5,NaN,'2']) {
      for (const method of ['update','complete','fail','abort','cancel']) {
        expect(() => Reflect.apply(reg[method],reg,['t',method==='update'?{}:'bad',{expectedAttempt:bad}])).toThrow(/positive integer/);
      }
    }
    expect(reg.complete('t','fresh',{expectedAttempt:2})).toMatchObject({result:'fresh'});
  });
  it('C07 总线明确内存接受与落盘失败，重启不伪称保存成功', () => {
    const dir=fs.mkdtempSync(path.join(os.tmpdir(),'f01-'));
    try {
      const blocked=path.join(dir,'blocked'); fs.writeFileSync(blocked,'file');
      const reg=new TaskRegistry({persistencePath:path.join(blocked,'tasks.json')});
      const handlers=new Map(); const bus={handle:(name,fn)=>handlers.set(name,fn)};
      registerTaskRegistryBusHandlers(bus,reg);
      const receipt=handlers.get('task:register')({taskId:'t',type:'x'});
      expect(receipt).toMatchObject({ok:true,persistence:{durable:false,status:'failed'}});
      expect(receipt.persistence.error).toBeTruthy();
      const done=handlers.get('task:complete')({taskId:'t',result:'external-action-already-done',expectedAttempt:receipt.task.attempt});
      expect(done).toMatchObject({ok:true,task:{status:'completed'},persistence:{durable:false,status:'failed'}});
      expect(new TaskRegistry({persistencePath:path.join(blocked,'tasks.json')}).query('t')).toBeNull();
    } finally {fs.rmSync(dir,{recursive:true,force:true});}
  });
});

import { EventBus } from '../hub/event-bus.ts';
import { registerBackgroundExec } from '../lib/exec-command/background.ts';
it('C05 真实总线拒绝无批次复用写入，真实后台生产回调捕获原批次', async () => {
  const reg=new TaskRegistry(); const bus=new EventBus(); registerTaskRegistryBusHandlers(bus,reg);
  const first=await bus.request('task:register',{taskId:'bus-task',type:'x'});
  await bus.request('task:complete',{taskId:'bus-task',expectedAttempt:first.task.attempt,result:'first'});
  const second=await bus.request('task:register',{taskId:'bus-task',type:'x'});
  for (const expectedAttempt of [undefined,null,1]) {
    expect(await bus.request('task:complete',{taskId:'bus-task',expectedAttempt,result:'old'})).toMatchObject({ok:false,task:null});
  }
  expect(await bus.request('task:complete',{taskId:'bus-task',expectedAttempt:second.task.attempt,result:'new'})).toMatchObject({ok:true,task:{result:'new'}});
  vi.useFakeTimers();
  try {
    const manager={list:()=>[{terminalId:'reused',status:'exited',exitCode:0}],readTail:()=>({output:'old'}),close:vi.fn()};
    const deferredStore={defer:vi.fn(),resolve:vi.fn()};
    registerBackgroundExec({manager:manager as never,deferredStore,taskRegistry:reg},{terminalId:'reused',sessionPath:'/synthetic',agentId:null,command:'synthetic'});
    reg.complete('reused','first'); const next=reg.register('reused',{type:'exec_command_background'});
    await vi.advanceTimersByTimeAsync(1000);
    expect(reg.query('reused')).toMatchObject({status:'running',attempt:2});
    expect(deferredStore.resolve).not.toHaveBeenCalled();
    registerBackgroundExec({manager:manager as never,deferredStore,taskRegistry:reg},{terminalId:'reused',sessionPath:'/synthetic',agentId:null,command:'synthetic'});
    await vi.advanceTimersByTimeAsync(1000);
    expect(reg.query('reused')).toMatchObject({status:'completed',attempt:next.attempt});
    expect(deferredStore.resolve).toHaveBeenCalledTimes(1);
  } finally {vi.clearAllTimers();vi.useRealTimers();}
});

it('C04 删除后重新登记保留批次，旧清理不会删除新执行', () => {
  const reg=new TaskRegistry(); reg.register('t',{type:'x'});
  reg.remove('t'); const next=reg.register('t',{type:'x'});
  expect(next.attempt).toBe(2); expect(reg.remove('t',{expectedAttempt:1})).toBe(false);
  expect(reg.remove('t')).toBe(false); expect(reg.query('t')).toMatchObject({attempt:2});
});

it('C04 清理墓碑有界，淘汰后仍拒绝旧批次与缺省回调', () => {
  const reg=new TaskRegistry(); reg.registerHandler('x',{abort:vi.fn()});
  for(let i=0;i<1025;i++) {const task=reg.register(`id-${i}`,{type:'x'});reg.remove(task.taskId,{expectedAttempt:task.attempt});}
  const fresh=reg.register('id-0',{type:'x'});expect(fresh.attempt).toBeGreaterThan(1);
  expect(reg.complete('id-0','old',{expectedAttempt:1})).toBeNull();
  expect(reg.complete('id-0','unknown')).toBeNull();
  expect(reg.complete('id-0','fresh',{expectedAttempt:fresh.attempt})).toMatchObject({result:'fresh'});
});
it('C06 后台任务跨会话并行，取消第一项使用它自己的父会话', () => {
  vi.useFakeTimers();
  try {
    const reg=new TaskRegistry();
    const manager={list:()=>[],readTail:()=>({output:''}),close:vi.fn()};
    const deps={manager:manager as never,taskRegistry:reg,deferredStore:{defer:vi.fn(),resolve:vi.fn()}};
    registerBackgroundExec(deps,{terminalId:'a',sessionPath:'/a',agentId:null,command:'a'});
    registerBackgroundExec(deps,{terminalId:'b',sessionPath:'/b',agentId:null,command:'b'});
    reg.abort('a','user',{expectedAttempt:1});
    expect(manager.close).toHaveBeenCalledExactlyOnceWith({sessionPath:'/a',terminalId:'a'});
    expect(reg.query('b')).toMatchObject({status:'running'});
  } finally {vi.clearAllTimers();vi.useRealTimers();}
});

import { TaskVisibilityAttempts } from '../lib/tasks/task-execution.ts';
it('C05 媒体交接只用宿主登记回执绑定批次，坏回执不退回缺省', async () => {
  const reg=new TaskRegistry(); const bus=new EventBus(); registerTaskRegistryBusHandlers(bus,reg);
  const bindings=new TaskVisibilityAttempts();
  const first=await bus.request('task:register',{taskId:'media',type:'x'});
  bindings.bindReceipt('media',1,first);
  reg.complete('media','first',{expectedAttempt:first.task.attempt});
  const second=await bus.request('task:register',{taskId:'media',type:'x'});
  bindings.bindReceipt('media',2,second);
  expect(await bindings.remove(bus,'media',1)).toMatchObject({ok:false});
  expect(reg.query('media')).toMatchObject({attempt:2,status:'running'});
  for(const bad of [null,{task:{attempt:'2',taskId:'media'}},{task:{attempt:2,taskId:'other'}}]) {
    bindings.bindReceipt('media',3,bad);
    expect(await bindings.remove(bus,'media',3)).toMatchObject({ok:false});
  }
  expect(await bindings.remove(bus,'media',2)).toMatchObject({ok:true});
  expect(reg.query('media')).toBeNull();
});
it('C07 媒体可见性清理失败保留错误并释放批次索引', async () => {
  const bindings=new TaskVisibilityAttempts();
  bindings.bind('media',1,2);
  const failedBus={request:vi.fn(async () => {throw new Error('task remove unavailable');})};
  await expect(bindings.remove(failedBus,'media',1)).rejects.toThrow('task remove unavailable');
  const laterBus={request:vi.fn(async () => ({ok:false}))};
  await bindings.remove(laterBus,'media',1);
  expect(laterBus.request).toHaveBeenCalledWith('task:remove',{taskId:'media',expectedAttempt:null});
});
