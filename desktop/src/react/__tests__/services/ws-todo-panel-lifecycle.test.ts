// @vitest-environment jsdom

/**
 * 清单改版 阶段1 回归测试：固定现状行为 + 复现已知缺陷。
 *
 * 计划书（PLAN.md 阶段 1）要求先用测试证明要改的行为，明确哪些是现状、
 * 哪些是缺陷：
 *
 * 【缺陷复现 — 当前实现下失败】
 * - D1: 实时 tool_end 失败（success:false）时，失败结果被转换为空清单，
 *       最后一份有效清单被静默清空（对应验收 A13）。
 * - D2: 实时 tool_end 缺少 details.todos（数据损坏）时同样被转换为空清单
 *       （对应验收 A13）。
 *
 * 【必须保留的现状行为 — 当前实现下通过】
 * - K1: 成功的 tool_end 正常更新目标会话清单并 bump live version。
 * - K2: 明确提交空清单（todos: []）作为明确清空处理，与失败严格区分（A14）。
 * - K3: 其他会话的 tool_end 不影响当前会话清单（A15 会话隔离）。
 * - K4: 全部 completed 的旧格式清单按旧生命周期移除（旧记录兼容，A18）。
 *
 * 阶段 2 落地新数据契约后，D1/D2 改用新面板状态断言并转绿；
 * K 系列在改版后必须保持通过。
 */
import { cleanup } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { useStore } from '../../stores';
import { handleServerMessage } from '../../services/ws-message-handler';

const SESSION_A = '/session/todo-a.jsonl';
const SESSION_B = '/session/todo-b.jsonl';

const OLD_TODOS = [
  { content: '保留的任务', activeForm: '正在保留的任务', status: 'in_progress' as const },
  { content: '待办任务', activeForm: '正在待办任务', status: 'pending' as const },
];

function seedSession(path: string, todos = OLD_TODOS) {
  // 不设置 sessionId 映射，sessionScopedKey 退化为 path，与断言读取一致。
  useStore.setState({ currentSessionPath: path, currentSessionId: null } as never);
  useStore.getState().setSessionTodoPanel(path, {
    todos,
    version: null,
    finished: false,
    allCompleted: false,
    dismissed: false,
    updateFailed: false,
  });
}

function todoToolEnd(overrides: Record<string, unknown>) {
  handleServerMessage({
    type: 'tool_end',
    id: 'tc-todo-1',
    name: 'todo_write',
    sessionPath: SESSION_A,
    ...overrides,
  });
}

describe('清单改版阶段1：实时清单更新回归', () => {
  beforeEach(() => {
    seedSession(SESSION_A);
    seedSession(SESSION_B, [{ content: 'B 会话任务', activeForm: '正在做 B', status: 'pending' as const }]);
    useStore.setState({ currentSessionPath: SESSION_A } as never);
  });

  afterEach(() => cleanup());

  it('K1: 成功的 tool_end 更新目标会话清单', () => {
    const next = [{ content: '新任务', activeForm: '正在做新任务', status: 'in_progress' as const }];
    todoToolEnd({ success: true, details: { todos: next } });

    expect(useStore.getState().todosBySession[SESSION_A]).toEqual(next);
    expect(useStore.getState().todosLiveVersionBySession[SESSION_A]).toBeGreaterThan(0);
  });

  it('K2: 明确提交空清单 = 明确清空，不与失败混淆', () => {
    todoToolEnd({ success: true, details: { todos: [] } });

    expect(useStore.getState().todosBySession[SESSION_A]).toEqual([]);
  });

  it('K3: 其他会话的 tool_end 不影响当前会话清单（会话隔离）', () => {
    handleServerMessage({
      type: 'tool_end',
      id: 'tc-other',
      name: 'todo_write',
      sessionPath: '/session/elsewhere.jsonl',
      success: true,
      details: { todos: [{ content: '别人的任务', activeForm: '正在做', status: 'pending' }] },
    });

    expect(useStore.getState().todosBySession[SESSION_A]).toEqual(OLD_TODOS);
  });

  it('K4: 旧格式（无版本标识）全部 completed 仍按旧生命周期移除', () => {
    todoToolEnd({
      success: true,
      details: {
        todos: [
          { content: '做完的事', activeForm: '正在做完的事', status: 'completed' },
        ],
      },
    });

    expect(useStore.getState().todosBySession[SESSION_A]).toEqual([]);
  });

  it('D1（缺陷已修复）: tool_end 失败时保留最后有效清单并标记更新失败', () => {
    todoToolEnd({ success: false, error: 'validation failed', details: {} });

    expect(useStore.getState().todosBySession[SESSION_A]).toEqual(OLD_TODOS);
    const panel = useStore.getState().todoPanelBySession[SESSION_A];
    expect(panel?.todos).toEqual(OLD_TODOS);
    expect(panel?.updateFailed).toBe(true);
  });

  it('D2（缺陷已修复）: tool_end 缺少 details.todos 时保留最后有效清单', () => {
    todoToolEnd({ success: true, details: {} });

    expect(useStore.getState().todosBySession[SESSION_A]).toEqual(OLD_TODOS);
    expect(useStore.getState().todoPanelBySession[SESSION_A]?.updateFailed).toBe(true);
  });

  it('D2b（缺陷已修复）: tool_end details 完全缺失时保留最后有效清单', () => {
    todoToolEnd({ success: true });

    expect(useStore.getState().todosBySession[SESSION_A]).toEqual(OLD_TODOS);
    expect(useStore.getState().todoPanelBySession[SESSION_A]?.updateFailed).toBe(true);
  });

  it('失败后的下一次成功更新清除失败标记', () => {
    todoToolEnd({ success: false, error: 'boom' });
    expect(useStore.getState().todoPanelBySession[SESSION_A]?.updateFailed).toBe(true);

    const next = [{ content: '新任务', activeForm: '正在做新任务', status: 'in_progress' as const }];
    todoToolEnd({ success: true, details: { todoVersion: 2, todos: next } });
    const panel = useStore.getState().todoPanelBySession[SESSION_A];
    expect(panel?.todos).toEqual(next);
    expect(panel?.updateFailed).toBe(false);
    expect(panel?.version).toMatch(/^tv[0-9a-f]{8}$/);
  });

  it('v2 全部完成的 tool_end 保留收尾摘要（不按旧语义消失）', () => {
    const done = [
      { content: '做完的事', activeForm: '正在做完的事', status: 'completed' as const },
      { content: '也做完', activeForm: '正在也做完', status: 'completed' as const },
    ];
    todoToolEnd({ success: true, details: { todoVersion: 2, todos: done } });

    const panel = useStore.getState().todoPanelBySession[SESSION_A];
    expect(panel?.todos).toEqual(done);
    expect(panel?.finished).toBe(true);
    expect(panel?.allCompleted).toBe(true);
  });

  it('v2 受阻清单保持活动并携带原因', () => {
    const blocked = [
      { content: '做完', activeForm: '正在做完', status: 'completed' as const },
      { content: '卡住', activeForm: '正在卡住', status: 'blocked' as const, blockedReason: '缺少凭据' },
    ];
    todoToolEnd({ success: true, details: { todoVersion: 2, todos: blocked } });

    const panel = useStore.getState().todoPanelBySession[SESSION_A];
    expect(panel?.finished).toBe(false);
    expect(panel?.todos[1]).toMatchObject({ status: 'blocked', blockedReason: '缺少凭据' });
  });

  it('todo_update 事件应用服务端权威面板快照', () => {
    const todos = [
      { content: 'done', activeForm: 'doing done', status: 'completed' as const },
      { content: 'rest', activeForm: 'doing rest', status: 'cancelled' as const },
    ];
    handleServerMessage({
      type: 'todo_update',
      sessionPath: SESSION_A,
      todos,
      version: 'tv12345678',
      finished: true,
      allCompleted: false,
    });

    const panel = useStore.getState().todoPanelBySession[SESSION_A];
    expect(panel).toMatchObject({
      todos, version: 'tv12345678', finished: true, allCompleted: false, updateFailed: false,
    });
  });

  it('todo_update 收纳事件隐藏当前面板', () => {
    handleServerMessage({
      type: 'todo_update',
      sessionPath: SESSION_A,
      removed: true,
      dismissed: true,
      todos: [],
    });

    expect(useStore.getState().todoPanelBySession[SESSION_A]).toBeUndefined();
    expect(useStore.getState().todosBySession[SESSION_A]).toEqual([]);
  });

  it('todoPanel 按会话隔离：B 会话更新不影响 A 会话面板', () => {
    handleServerMessage({
      type: 'tool_end',
      id: 'tc-b',
      name: 'todo_write',
      sessionPath: SESSION_B,
      success: true,
      details: {
        todoVersion: 2,
        todos: [{ content: 'B 新任务', activeForm: '正在做 B 新任务', status: 'completed' }],
      },
    });

    expect(useStore.getState().todoPanelBySession[SESSION_A]?.todos).toEqual(OLD_TODOS);
    expect(useStore.getState().todoPanelBySession[SESSION_B]?.finished).toBe(true);
  });
});
