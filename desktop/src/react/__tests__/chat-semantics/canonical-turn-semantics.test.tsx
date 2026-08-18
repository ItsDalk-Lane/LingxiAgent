/**
 * 聊天语义收口回归测试（任务书阶段1：先失败后修复）
 *
 * 测试 C：真实 missing_final_answer 必须走完整 streamBufferManager 管线。
 * 测试 D：Process Fold 打开后 Thinking/MOOD/Tool/Skill 内部卡片必须可以继续展开。
 * ID 矩阵：turn_end 持久化绑定前后 canonical blockId 不变。
 */

// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { streamBufferManager } from '../../hooks/use-stream-buffer';
import { snapshotStreamBuffer } from '../../stores/stream-invalidator';
import { useStore } from '../../stores';
import { readLiveAssistantMessage } from '../../stores/live-turn-store';
import { ChatTranscript } from '../../components/chat/ChatTranscript';
import { resolveAssistantTurnOutcome } from '../../utils/turn-outcome';
import type { ChatListItem, ChatMessage, ContentBlock } from '../../stores/chat-types';

const PATH = '/test/canonical-turn.jsonl';
const SESSION_ID = 'sess_canonical_turn';

function userItem(id: string, text: string): ChatListItem {
  return { type: 'message', data: { id, role: 'user', text } };
}

function getItems(): ChatListItem[] {
  return useStore.getState().chatSessions[PATH]?.items ?? [];
}

function getAssistantMessage(): ChatMessage | null {
  const item = getItems().find((entry) => entry.type === 'message' && entry.data.role === 'assistant');
  return item?.type === 'message' ? item.data : null;
}

function assistantBlocks(): ContentBlock[] {
  const message = getAssistantMessage();
  if (!message) return [];
  const live = readLiveAssistantMessage(PATH, message.id);
  return live ? [...live.blocks] : (message.blocks || []);
}

const t = (key: string, vars?: Record<string, string | number>): string => {
  const table: Record<string, string> = {
    'processFold.summary': '✨ {name}忙活了一阵子',
    'processFold.tools': '{n} 个工具',
    'processFold.thinking': '{n} 次思考',
    'processFold.unsuccessful': '{n} 次尝试未成功',
    'chat.turnStatus.missingFinalAnswer': '未生成最终回复',
    'thinking.done': '思考完成',
    'thinking.active': '思考中',
    'toolGroup.skill.completed': '已运行技能 {name}',
    'toolGroup.skill.running': '正在运行技能 {name}',
    'toolGroup.skill.promptUnavailable': '（无可用参数）',
  };
  return (table[key] || key).replace(/\{(\w+)\}/g, (_, name) => String(vars?.[name] ?? ''));
};

function resetSession(): void {
  streamBufferManager.clearAll();
  useStore.setState({
    currentSessionId: null,
    currentSessionPath: null,
    sessions: [],
    sessionLocatorsById: {},
    streamingSessions: [],
    selectedIdsBySession: {},
    terminalsBySession: {},
    agents: [],
    agentName: '小花',
    agentYuan: 'lingxi',
  } as never);
  useStore.getState().clearSession(PATH);
  useStore.getState().initSession(PATH, [userItem('u1', 'hi')], false);
}

/** 服务端 canonical 事件序列（协议解析已在前移后的 normalizer 完成）。 */
function canonicalTurn(script: Array<Record<string, unknown>>): void {
  for (const event of script) streamBufferManager.handle({ sessionPath: PATH, ...event });
}

describe('streamBufferManager canonical 收口', () => {
  beforeEach(resetSession);

  it('测试A客户端面：mood canonical 流 + legacy mood 事件不再双写 mood block', () => {
    canonicalTurn([
      { type: 'mood_start', moodOrdinal: 0 },
      { type: 'mood_text', moodOrdinal: 0, delta: 'Vibe: 专注' },
      { type: 'mood_end', moodOrdinal: 0 },
      { type: 'assistant_segment_start', segmentId: 'assistant:1:text:0', kind: 'text', semanticPhase: 'final_answer' },
      { type: 'assistant_segment_delta', segmentId: 'assistant:1:text:0', delta: '最终答复', semanticPhase: 'final_answer' },
      { type: 'assistant_segment_end', segmentId: 'assistant:1:text:0', semanticPhase: 'final_answer' },
      // 旧服务器兼容期会同时发 legacy mood 事件；canonical 模式下不能产生第二个 mood
      { type: 'mood_start' },
      { type: 'mood_text', delta: 'Vibe: 专注' },
      { type: 'mood_end' },
      { type: 'text_delta', delta: '最终答复' },
      { type: 'turn_end', assistantEntryId: 'entry-assistant-1' },
    ]);

    const blocks = assistantBlocks();
    expect(blocks.filter((block) => block.type === 'mood')).toHaveLength(1);
    const textBlocks = blocks.filter((block) => block.type === 'text');
    expect(textBlocks).toHaveLength(1);
    expect((textBlocks[0] as { source?: string }).source).toBe('最终答复');
  });

  it('legacy 服务器（无 canonical 事件）仍完整走旧管线产出正文与 mood', () => {
    canonicalTurn([
      { type: 'mood_start' },
      { type: 'mood_text', delta: 'Vibe: 专注' },
      { type: 'mood_end' },
      { type: 'text_delta', delta: '旧服务器答复' },
      { type: 'turn_end', assistantEntryId: 'entry-assistant-legacy' },
    ]);

    const blocks = assistantBlocks();
    expect(blocks.filter((block) => block.type === 'mood')).toHaveLength(1);
    const text = blocks.find((block) => block.type === 'text');
    expect((text as { source?: string }).source).toBe('旧服务器答复');
  });
});

describe('resolveAssistantTurnOutcome 矩阵', () => {
  function outcomeFor(blocks: ContentBlock[], status: 'streaming' | 'completed' | 'failed' | 'aborted' = 'completed') {
    return resolveAssistantTurnOutcome({ blocks, status });
  }

  it('final_answer -> completed_with_answer', () => {
    expect(outcomeFor([
      { type: 'text', source: '答复', semanticPhase: 'final_answer' },
    ]).outcome).toBe('completed_with_answer');
  });

  it('commentary + final_answer -> completed_with_answer', () => {
    expect(outcomeFor([
      { type: 'text', source: '过程', semanticPhase: 'commentary' },
      { type: 'text', source: '答复', semanticPhase: 'final_answer' },
    ]).outcome).toBe('completed_with_answer');
  });

  it('thinking + final_answer / tool + final_answer -> completed_with_answer', () => {
    expect(outcomeFor([
      { type: 'thinking', content: '思考', sealed: true },
      { type: 'text', source: '答复', semanticPhase: 'final_answer' },
    ]).outcome).toBe('completed_with_answer');
    expect(outcomeFor([
      { type: 'tool_group', tools: [{ name: 'read', done: true, success: true }], collapsed: false },
      { type: 'text', source: '答复', semanticPhase: 'final_answer' },
    ]).outcome).toBe('completed_with_answer');
  });

  it('process only / commentary only -> completed_without_user_output', () => {
    expect(outcomeFor([
      { type: 'thinking', content: '思考', sealed: true },
      { type: 'tool_group', tools: [{ name: 'read', done: true, success: true }], collapsed: false },
    ]).outcome).toBe('completed_without_user_output');
    expect(outcomeFor([
      { type: 'text', source: '只完成了内部检查', semanticPhase: 'commentary' },
    ]).outcome).toBe('completed_without_user_output');
  });

  it('file result only / media result only -> completed_with_result', () => {
    expect(outcomeFor([
      { type: 'file', filePath: '/tmp/a.png', label: 'a.png', ext: 'png' },
    ]).outcome).toBe('completed_with_result');
    expect(outcomeFor([
      { type: 'media_generation', taskId: 't1', kind: 'image', status: 'pending' },
    ]).outcome).toBe('completed_with_result');
  });

  it('pending confirmation -> completed_with_control', () => {
    expect(outcomeFor([
      { type: 'session_confirmation', confirmId: 'c1', kind: 'approval', surface: 'message', status: 'pending', title: '需要确认' },
    ]).outcome).toBe('completed_with_control');
  });

  it('failed / aborted 无论有无部分文字', () => {
    expect(outcomeFor([{ type: 'text', source: '部分', semanticPhase: 'final_answer' }], 'failed').outcome).toBe('failed');
    expect(outcomeFor([{ type: 'text', source: '部分', semanticPhase: 'final_answer' }], 'aborted').outcome).toBe('aborted');
  });

  it('streaming 恒为 streaming', () => {
    expect(outcomeFor([], 'streaming').outcome).toBe('streaming');
  });

  it('commentary 不会被自然语言升级为 answer', () => {
    expect(outcomeFor([
      { type: 'text', source: '任务完成了，结果如下：一切正常。', semanticPhase: 'commentary' },
    ]).outcome).toBe('completed_without_user_output');
  });
});

describe('Process Fold 全链路交互（测试D）', () => {
  beforeEach(() => {
    resetSession();
    window.t = t as typeof window.t;
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', {
      configurable: true,
      value: vi.fn(),
    });
    window.requestAnimationFrame = ((callback: FrameRequestCallback) => {
      callback(0);
      return 1;
    }) as typeof window.requestAnimationFrame;
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  function skillToolGroup(): ContentBlock {
    return {
      type: 'tool_group',
      tools: [{
        id: 'call-skill',
        name: 'read',
        args: { path: '/skills/leader/SKILL.md' },
        done: true,
        success: true,
        status: 'succeeded',
        details: { skillInvocation: { content: '# Skill: leader' } },
      }],
      collapsed: false,
    };
  }

  it('正常 final_answer：折叠 + 正文共存，内部卡片仍可点击', async () => {
    canonicalTurn([
      { type: 'assistant_segment_start', segmentId: 'assistant:1:reasoning:default', kind: 'reasoning', semanticPhase: 'reasoning' },
      { type: 'assistant_segment_delta', segmentId: 'assistant:1:reasoning:default', delta: '推理', semanticPhase: 'reasoning' },
      { type: 'assistant_segment_end', segmentId: 'assistant:1:reasoning:default', semanticPhase: 'reasoning' },
      { type: 'mood_start', moodOrdinal: 0 },
      { type: 'mood_text', moodOrdinal: 0, delta: 'Vibe: 好' },
      { type: 'mood_end', moodOrdinal: 0 },
      { type: 'tool_start', id: 'call-read', name: 'read', args: { path: '/tmp/a.md' } },
      { type: 'tool_end', id: 'call-read', name: 'read', success: true, status: 'succeeded' },
      { type: 'assistant_segment_start', segmentId: 'assistant:2:text:0', kind: 'text', semanticPhase: 'final_answer' },
      { type: 'assistant_segment_delta', segmentId: 'assistant:2:text:0', delta: '最终答复正文', semanticPhase: 'final_answer' },
      { type: 'assistant_segment_end', segmentId: 'assistant:2:text:0', semanticPhase: 'final_answer' },
      {
        type: 'turn_end',
        turnInputEntryId: 'entry-user-1',
        userEntryId: 'entry-user-1',
        assistantEntryId: 'entry-assistant-2',
      },
    ]);

    render(
      <ChatTranscript
        items={getItems()}
        sessionPath={PATH}
        enableProcessFold
      />,
    );

    expect(screen.queryByText('未生成最终回复')).not.toBeInTheDocument();
    expect(screen.getByText('最终答复正文')).toBeInTheDocument();
    const summary = screen.getByRole('button', { name: /小花忙活了一阵子/ });
    fireEvent.click(summary);
    await waitFor(() => expect(summary).toHaveAttribute('aria-expanded', 'true'));

    const moodSummary = screen.getByText(/MOOD|心绪/);
    fireEvent.click(moodSummary);
    await waitFor(() => expect(screen.getByText(/Vibe: 好/)).toBeInTheDocument());
  });

  it('延迟结果：turn sealed 后 file 替换 media_generation，missing_final_answer 被移除且 outcome 升级', () => {
    canonicalTurn([
      { type: 'content_block', block: { type: 'media_generation', taskId: 'task-late', kind: 'image', status: 'pending', prompt: 'x' } },
      { type: 'turn_end', assistantEntryId: 'entry-assistant-late' },
    ]);
    expect(assistantBlocks().some((block) => block.type === 'turn_status'
      && block.status === 'missing_final_answer')).toBe(true);

    streamBufferManager.handle({
      type: 'content_block',
      sessionPath: PATH,
      block: {
        type: 'file',
        replacesTaskId: 'task-late',
        fileId: 'sf-late',
        filePath: '/tmp/late.png',
        label: 'late.png',
        ext: 'png',
      },
    });

    const blocks = assistantBlocks();
    expect(blocks.some((block) => block.type === 'turn_status' && block.status === 'missing_final_answer')).toBe(false);
    expect(blocks.some((block) => block.type === 'file')).toBe(true);
    const message = getAssistantMessage();
    expect(message?.turnProjection?.outcome ?? undefined).toBeDefined();
    expect(message?.turnProjection?.outcome).toBe('completed_with_result');
  });
});
