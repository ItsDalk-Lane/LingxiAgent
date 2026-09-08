import fs from 'node:fs';
import path from 'node:path';
import { describe, expect, it, vi } from 'vitest';
import { DESKTOP_INPUT_CORRELATION_TYPE } from '../core/desktop-input-correlation.ts';
import { createDesktopInputHistoryFixture as fixture } from './helpers/desktop-input-history-fixture.ts';

describe('R04 真实 SDK 桌面提交到历史关联', () => {
  it('R04-15 相同正文不同 client ID 经真实 append 和历史路由仍分别关联', async () => {
    const f = await fixture(); await f.submit('input-a', 2); await f.submit('input-b', 3);
    const bytes = fs.readFileSync(f.sessionPath);
    const response = await f.history();
    const users = response.messages.filter((message: any) => message.role === 'user');
    expect(users.map((message: any) => [message.clientMessageId, message.snapshotVersion])).toEqual([['input-a', 2], ['input-b', 3]]);
    const entries = f.manager.getBranch().filter(entry => entry.type === 'message' && entry.message.role === 'user');
    expect(users.map((message: any) => message.sourceEntryId)).toEqual(entries.map(entry => entry.id));
    expect(response.reconciliation).toMatchObject({ sessionId: f.sessionId, sessionPath: f.sessionPath, complete: true, runStatus: 'reconciled_idle' });
    expect(f.stream).toHaveBeenCalledTimes(2);
    expect(f.engine.activityHub.rebroadcastSession).not.toHaveBeenCalled();
    expect(fs.readFileSync(f.sessionPath)).toEqual(bytes);
  });

  it('R04-08 分支读取失败保持不完整，不能 fallback 全 JSONL 认证', async () => {
    const f = await fixture(); await f.submit('input-a');
    f.engine.getSessionBranchHead = () => ({ sessionId: f.sessionId, leafId: 'missing-entry', observedTailLeafId: f.manager.getLeafId() });
    const before = fs.readFileSync(f.sessionPath);
    const response = await f.history();
    expect(response.reconciliation).toMatchObject({ complete: false, runStatus: 'unknown' });
    expect(response.messages.some((message: any) => message.clientMessageId)).toBe(false);
    expect(fs.readFileSync(f.sessionPath)).toEqual(before);
  });

  it('R04 缺失权威运行读取能力不能宣称 idle', async () => {
    const f = await fixture(); await f.submit('input-a'); delete f.engine.isSessionStreaming;
    const response = await f.history();
    expect(response.reconciliation).toMatchObject({ complete: true, runStatus: 'unknown' });
  });

  it('R04-07 孤立关联不能认证一个不存在的 user entry', async () => {
    const f = await fixture(); await f.session.prompt('unassociated user');
    f.manager.appendCustomEntry(DESKTOP_INPUT_CORRELATION_TYPE, { schemaVersion: 1, sessionId: f.sessionId, sourceEntryId: 'missing-entry', clientMessageId: 'orphan', snapshotVersion: 1 });
    const response = await f.history();
    expect(response.reconciliation.complete).toBe(true);
    expect(response.messages.some((message: any) => message.clientMessageId)).toBe(false);
  });

  it('R04-08 当前分支不含原 user 或关联时不泄漏废弃分支证据', async () => {
    const f = await fixture(); await f.submit('branch-a');
    const priorLeaf = f.manager.getLeafId()!;
    await f.submit('branch-b');
    f.manager.branch(priorLeaf);
    const response = await f.history();
    expect(response.messages.filter((message: any) => message.clientMessageId).map((message: any) => message.clientMessageId)).toEqual(['branch-a']);
  });

  it('R04-08 fork 新 session 身份不能消费旧 session client ID', async () => {
    const f = await fixture(); await f.submit('original-client');
    const forkId = 'fork-session-identity';
    const entries = fs.readFileSync(f.sessionPath, 'utf8').trim().split('\n').map(line => JSON.parse(line));
    entries[0].id = forkId;
    const forkPath = path.join(path.dirname(f.sessionPath), 'fork-session-identity.jsonl');
    fs.writeFileSync(forkPath, entries.map(entry => JSON.stringify(entry)).join('\n') + '\n');
    f.engine.getSessionManifest = (id: string) => id === forkId ? { sessionId: forkId, currentLocator: { path: forkPath } } : null;
    f.engine.getSessionBranchHead = () => ({ sessionId: forkId, leafId: entries.at(-1).id, observedTailLeafId: entries.at(-1).id });
    const response = await f.app.request(`/api/sessions/messages?sessionId=${forkId}&reconciliation=1`);
    expect(response.status).toBe(200);
    const body: any = await response.json();
    expect(body.reconciliation.complete).toBe(true);
    expect(body.messages.some((message: any) => message.clientMessageId)).toBe(false);
  });

  it('R04-11 相同关联重复幂等，client ID 指向另一个版本的 entry 时明确 ambiguous', async () => {
    const f = await fixture(); await f.submit('repeated-client', 1);
    const record: any = f.manager.getBranch().find(entry => entry.type === 'custom' && entry.customType === DESKTOP_INPUT_CORRELATION_TYPE);
    f.manager.appendCustomEntry(DESKTOP_INPUT_CORRELATION_TYPE, record.data);
    let response = await f.history();
    expect(response.messages.filter((message: any) => message.clientMessageId)).toHaveLength(1);
    await f.submit('repeated-client', 2);
    response = await f.history();
    const users = response.messages.filter((message: any) => message.role === 'user');
    expect(users).toHaveLength(2);
    expect(users.every((message: any) => !message.clientMessageId && message.acceptanceDiagnostic === 'ambiguous')).toBe(true);
  });

  it('R04-16 关联存储失败且诊断广播也失败，真实 user 和运行继续且只提交一次', async () => {
    const f = await fixture();
    const append = f.manager.appendCustomEntry.bind(f.manager);
    vi.spyOn(f.manager, 'appendCustomEntry').mockImplementation((type, data) => {
      if (type === DESKTOP_INPUT_CORRELATION_TYPE) throw Object.assign(new Error('synthetic correlation disk failure'), { code: 'EACCES' });
      return append(type, data);
    });
    f.engine.emitEvent.mockImplementation((event: any) => {
      if (event.type === 'session_input_correlation_unavailable') throw new Error('synthetic diagnostic transport failure');
    });
    await f.submit('no-correlation');
    expect(f.stream).toHaveBeenCalledTimes(1);
    const response = await f.history();
    expect(response.messages.filter((message: any) => message.role === 'user')).toHaveLength(1);
    expect(response.messages.some((message: any) => message.clientMessageId)).toBe(false);
    expect(response.messages.some((message: any) => message.role === 'assistant')).toBe(true);
  });

  it('R04-10 读取过程中完整运行一次，前后均idle也因revision变化保持unknown', async () => {
    const f = await fixture(); await f.submit('before-read');
    const original = fs.promises.readFile.bind(fs.promises);
    let injected = false;
    vi.spyOn(fs.promises, 'readFile').mockImplementation((async (...args: any[]) => {
      if (args[0] === f.sessionPath && !injected) {
        injected = true;
        await f.session.prompt('a complete intervening run');
      }
      return (original as any)(...args);
    }) as any);
    const response = await f.history();
    expect(injected).toBe(true);
    expect(f.session.isStreaming).toBe(false);
    expect(response.reconciliation.runStatus).toBe('unknown');
    expect(f.stream).toHaveBeenCalledTimes(2);
  });

  it('R04-03 历史关联已经落盘但供应商流仍在等待，确认接收不能宣称idle', async () => {
    const f = await fixture(); await f.submit('prior');
    const entered = Promise.withResolvers<void>();
    const released = Promise.withResolvers<void>();
    const original = f.stream.getMockImplementation()!;
    f.stream.mockImplementationOnce((...args: any[]) => {
      entered.resolve();
      return released.promise.then(() => (original as any)(...args)) as any;
    });
    const submitting = f.submit('still-running');
    await entered.promise;
    try {
      const response = await f.history();
      expect(response.messages.some((message: any) => message.clientMessageId === 'still-running')).toBe(true);
      expect(response.reconciliation.runStatus).toBe('running');
    } finally { released.resolve(); await submitting; }
  });

  it.each(['duplicate-id', 'old-schema', 'invalid-json'])('R04-08 %s 不能通过修复/宽松读取认证，文件字节保持不变', async kind => {
    const f = await fixture(); await f.submit('input-a');
    const lines = fs.readFileSync(f.sessionPath, 'utf8').trim().split('\n');
    if (kind === 'duplicate-id') lines.push(lines.at(-1)!);
    else if (kind === 'invalid-json') lines.push('{bad json');
    else { const header = JSON.parse(lines[0]); header.version = 2; lines[0] = JSON.stringify(header); }
    fs.writeFileSync(f.sessionPath, lines.join('\n') + '\n');
    const before = fs.readFileSync(f.sessionPath);
    const response = await f.history();
    expect(response.reconciliation.complete).toBe(false);
    expect(response.messages.some((message: any) => message.clientMessageId)).toBe(false);
    expect(fs.readFileSync(f.sessionPath)).toEqual(before);
  });
});
