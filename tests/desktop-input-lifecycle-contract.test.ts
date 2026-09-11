import { describe, expect, it, vi } from 'vitest';
import {
  abortPendingDesktopSubmission,
  isDesktopInputRejectedBeforeAcceptance,
  submitDesktopSessionInterjection,
  submitDesktopSessionMessageWithReceipt,
  MESSAGE_ORIGIN_RECORD_TYPE,
  MESSAGE_PRESENTATION_RECORD_TYPE,
} from '../core/desktop-session-submit.ts';
import { DESKTOP_INPUT_CORRELATION_TYPE } from '../core/desktop-input-correlation.ts';
import { SessionCoordinator } from '../core/session-coordinator.ts';
import { deliverAgentMessage } from '../lib/session-collab/delivery.ts';
import { createDesktopInputHistoryFixture } from './helpers/desktop-input-history-fixture.ts';

describe('输入移交、落盘与关联证据', () => {
  it.each(['关联写入失败', '业务身份变化'])('真实 SDK：%s后提交末端失败不得宣称输入未接受', async (failure) => {
    const f = await createDesktopInputHistoryFixture();
    if (failure === '关联写入失败') {
      const append = f.manager.appendCustomEntry.bind(f.manager);
      vi.spyOn(f.manager, 'appendCustomEntry').mockImplementation((type, data) => {
        if (type === DESKTOP_INPUT_CORRELATION_TYPE) throw new Error('synthetic correlation write failure');
        return append(type, data);
      });
    }
    const prompt = f.engine.promptSession;
    f.engine.promptSession = async (...args: any[]) => {
      if (failure === '业务身份变化') f.engine.getSessionIdForPath = () => 'another-business-session';
      await prompt(...args);
      throw new Error('synthetic post-input failure');
    };

    const caught = await f.submit('unproven-correlation').catch(error => error);
    expect(caught).toBeInstanceOf(Error);
    expect(f.manager.getBranch().filter(entry => entry.type === 'message' && entry.message.role === 'user')).toHaveLength(1);
    expect(f.engine.emitEvent.mock.calls.some(([event]: any[]) => event.message?.sourceEntryId)).toBe(false);
    expect(isDesktopInputRejectedBeforeAcceptance(caught)).toBe(false);
    expect(f.stream).toHaveBeenCalledOnce();
  });

  it('真实 SDK：明确的模型预检拒绝仍证明没有移交或写入输入', async () => {
    const f = await createDesktopInputHistoryFixture();
    vi.spyOn(f.session, 'model', 'get').mockReturnValue(undefined);
    const caught = await f.submit('preflight-rejected').catch(error => error);
    expect(caught).toBeInstanceOf(Error);
    expect(isDesktopInputRejectedBeforeAcceptance(caught)).toBe(true);
    expect(f.manager.getBranch().filter(entry => entry.type === 'message' && entry.message.role === 'user')).toHaveLength(0);
    expect(f.stream).not.toHaveBeenCalled();
  });

  it('插入回退普通发送后，外层不能把内层已移交的失败重新标成拒绝', async () => {
    const f = await createDesktopInputHistoryFixture();
    f.engine.steerSession = () => false;
    const prompt = f.engine.promptSession;
    f.engine.promptSession = async (...args: any[]) => {
      await prompt(...args);
      throw new Error('synthetic post-input failure');
    };
    const caught = await submitDesktopSessionInterjection(f.engine, {
      sessionId: f.sessionId, sessionPath: f.sessionPath, text: '空闲时插入', clientMessageId: 'interject-fallback',
    }).catch(error => error);
    expect(caught).toBeInstanceOf(Error);
    expect(f.manager.getBranch().filter(entry => entry.type === 'message' && entry.message.role === 'user')).toHaveLength(1);
    expect(isDesktopInputRejectedBeforeAcceptance(caught)).toBe(false);
  });

  it('提交期间运行实例更换，另一实例已经执行的输入仍必须保持结果未知', async () => {
    const f = await createDesktopInputHistoryFixture();
    const replacement = await createDesktopInputHistoryFixture();
    f.engine.promptSession = async (_path: string, text: string) => {
      f.engine.getSessionByPath = () => replacement.session;
      await replacement.session.prompt(text);
      throw new Error('synthetic replacement runtime failure');
    };
    const caught = await f.submit('runtime-replaced').catch(error => error);
    expect(caught).toBeInstanceOf(Error);
    expect(replacement.manager.getBranch().filter(entry => entry.type === 'message' && entry.message.role === 'user')).toHaveLength(1);
    expect(f.engine.emitEvent.mock.calls.some(([event]: any[]) => event.message?.sourceEntryId)).toBe(false);
    expect(isDesktopInputRejectedBeforeAcceptance(caught)).toBe(false);
  });

  it('SDK 预检期间停止，真正移交前再次核对取消并拒绝接受收据', async () => {
    const f = await createDesktopInputHistoryFixture();
    const preflightEntered = Promise.withResolvers<void>();
    const preflightFinished = Promise.withResolvers<void>();
    f.engine.preflightSessionInput = () => {};
    f.engine.promptSession = async (_path: string, text: string, opts: any, hooks: any) => {
      preflightEntered.resolve();
      await preflightFinished.promise;
      hooks.afterCachePreflight();
      hooks.afterInputAccepted?.();
      await f.session.prompt(text, opts);
    };
    const opts = { sessionId: f.sessionId, sessionPath: f.sessionPath, text: '准备期间取消', clientMessageId: 'cancel-preflight' };
    const receipt = submitDesktopSessionMessageWithReceipt(f.engine, opts);
    const outcome = Promise.allSettled([receipt.accepted, receipt.completion]);
    await preflightEntered.promise;
    expect(abortPendingDesktopSubmission(f.engine, opts)).toBe(true);
    preflightFinished.resolve();
    for (const result of await outcome) {
      expect(result.status).toBe('rejected');
      if (result.status === 'rejected') expect(result.reason).toMatchObject({ code: 'input_cancelled_before_acceptance' });
    }
    expect(f.manager.getBranch().filter(entry => entry.type === 'message' && entry.message.role === 'user')).toHaveLength(0);
    expect(f.stream).not.toHaveBeenCalled();
  });

  it('跨会话投递收到移交后的 session_busy 错误时不得换路重复发送', async () => {
    const f = await createDesktopInputHistoryFixture();
    const prompt = f.engine.promptSession;
    const failure = new Error('session_busy');
    f.engine.promptSession = async (...args: any[]) => { await prompt(...args); throw failure; };
    f.engine.steerSession = vi.fn(() => false);
    const caught = await deliverAgentMessage(f.engine, {
      targetSessionId: f.sessionId, message: '只交付一次', from: { agentId: 'synthetic-agent', agentName: 'Synthetic' },
    }).catch(error => error);
    expect(caught).toBe(failure);
    expect(isDesktopInputRejectedBeforeAcceptance(caught)).toBe(false);
    expect(f.manager.getBranch().filter(entry => entry.type === 'message' && entry.message.role === 'user')).toHaveLength(1);
    expect(f.stream).toHaveBeenCalledOnce();
  });

  it.each(['接受收据', '跨会话投递'])('加载期间取消不能让%s报告接受成功', async (consumer) => {
    const f = await createDesktopInputHistoryFixture();
    const loaded = Promise.withResolvers<void>();
    f.engine.ensureSessionLoaded = async () => { await loaded.promise; return f.session; };
    const opts = { sessionId: f.sessionId, sessionPath: f.sessionPath, text: '取消前保留原文', clientMessageId: 'cancel-before-load' };
    const receipt = consumer === '接受收据' ? submitDesktopSessionMessageWithReceipt(f.engine, opts) : null;
    const outcome = receipt
      ? Promise.allSettled([receipt.accepted, receipt.completion])
      : Promise.allSettled([deliverAgentMessage(f.engine, {
        targetSessionId: f.sessionId, message: opts.text, from: { agentId: 'synthetic-agent', agentName: 'Synthetic' },
      })]);
    expect(abortPendingDesktopSubmission(f.engine, opts)).toBe(true);
    loaded.resolve();
    for (const result of await outcome) {
      expect(result.status).toBe('rejected');
      if (result.status === 'rejected') {
        expect(result.reason).toMatchObject({ name: 'AbortError', code: 'input_cancelled_before_acceptance' });
        expect(isDesktopInputRejectedBeforeAcceptance(result.reason)).toBe(true);
      }
    }
    expect(f.manager.getBranch().filter(entry => entry.type === 'message' && entry.message.role === 'user')).toHaveLength(0);
    expect(f.stream).not.toHaveBeenCalled();
  });
});

describe('SDK 插入异步结果', () => {
  it('真实 SDK 提前消费插入队列时，来源和展示记录仍绑定这条输入而非下一条', async () => {
    const f = await createDesktopInputHistoryFixture();
    const start = Promise.withResolvers<void>();
    const finishFirst = Promise.withResolvers<void>();
    const consumed = Promise.withResolvers<void>();
    const stream = f.stream.getMockImplementation()!;
    f.stream.mockImplementationOnce((...args: any[]) => {
      start.resolve();
      return finishFirst.promise.then(() => (stream as any)(...args)) as any;
    });
    const initial = f.submit('initial-input');
    await start.promise;
    const body = '这一条插入的原文';
    const unsubscribe = f.session.subscribe(event => {
      if (event.type === 'message_end' && event.message.role === 'user'
        && JSON.stringify(event.message.content).includes(body)) consumed.resolve();
    });
    f.engine.steerSession = async (_path: string, text: string) => {
      await f.session.steer(text);
      finishFirst.resolve();
      // 模拟 SDK 已排入队列，但调用方收到 Promise 完成前输入已被实际消费。
      await consumed.promise;
      return true;
    };
    try {
      await submitDesktopSessionInterjection(f.engine, {
        sessionId: f.sessionId, sessionPath: f.sessionPath, text: body, clientMessageId: 'early-consumed-interject',
        displayMessage: { text: '插入展示原文', source: 'agent_session', origin: { kind: 'agent', agentId: 'synthetic-other' } },
      });
      await initial;
      const entries = f.manager.getBranch();
      const userIndex = entries.findIndex(entry => entry.type === 'message' && entry.message.role === 'user'
        && JSON.stringify(entry.message.content).includes(body));
      expect(userIndex).toBeGreaterThan(0);
      for (const type of [MESSAGE_ORIGIN_RECORD_TYPE, MESSAGE_PRESENTATION_RECORD_TYPE]) {
        const indexes = entries.flatMap((entry, index) => entry.type === 'custom' && entry.customType === type ? [index] : []);
        expect(indexes).toHaveLength(1);
        expect(indexes[0]).toBeLessThan(userIndex);
      }
      await f.submit('after-interject');
      const history = await f.history();
      const users = history.messages.filter((message: any) => message.role === 'user');
      expect(users).toHaveLength(3);
      expect(users[1]).toMatchObject({
        clientMessageId: 'early-consumed-interject', displayText: '插入展示原文',
        origin: { kind: 'agent', agentId: 'synthetic-other' },
      });
      expect(users[2].origin).toBeUndefined();
      expect(users[2].displayText).toBeUndefined();
    } finally { unsubscribe(); finishFirst.resolve(); await initial; }
  });

  it('真实协调器等待 SDK 插入完成；异步失败不得提前返回成功', async () => {
    const enqueue = Promise.withResolvers<void>();
    // 给旧实现未消费的 Promise 安装观察者，让反例准确失败而非制造未处理异常。
    void enqueue.promise.catch(() => {});
    const session = { isStreaming: true, steer: vi.fn(() => enqueue.promise) };
    const coordinator = {
      _getSessionEntryByPath: () => ({ session }),
      preflightSessionInput: vi.fn(),
    };
    const result = SessionCoordinator.prototype.steerSession.call(coordinator, '/synthetic/input.jsonl', '插入原文');
    const settled = Promise.resolve(result).then(value => ({ value }), error => ({ error }));
    expect(result).toBeInstanceOf(Promise);
    const failure = new Error('synthetic async SDK refusal');
    enqueue.reject(failure);
    expect(await settled).toEqual({ error: failure });
  });

  it('真实提交入口等待插入结果，失败不消费提醒、不投影成功消息', async () => {
    const f = await createDesktopInputHistoryFixture();
    const enqueue = Promise.withResolvers<boolean>();
    void enqueue.promise.catch(() => {});
    f.engine.isSessionStreaming = () => true;
    f.engine.steerSession = () => enqueue.promise;
    f.engine.renderSessionReminderBlock = () => ({ block: 'synthetic reminder', receipt: ['r1'] });
    f.engine.consumeRenderedSessionReminderBlock = vi.fn();
    const completion = submitDesktopSessionInterjection(f.engine, {
      sessionId: f.sessionId, sessionPath: f.sessionPath, text: '插入原文', clientMessageId: 'async-interject',
    });
    let completed = false;
    const outcome = completion.then(value => { completed = true; return { value }; }, error => { completed = true; return { error }; });
    await new Promise(resolve => setImmediate(resolve));
    expect(completed).toBe(false);
    expect(f.engine.consumeRenderedSessionReminderBlock).not.toHaveBeenCalled();
    expect(f.engine.emitEvent.mock.calls.some(([event]: any[]) => event.type === 'session_user_message')).toBe(false);
    const failure = new Error('synthetic async insertion failure');
    enqueue.reject(failure);
    expect(await outcome).toEqual({ error: failure });
    expect(f.engine.consumeRenderedSessionReminderBlock).not.toHaveBeenCalled();
  });
});
