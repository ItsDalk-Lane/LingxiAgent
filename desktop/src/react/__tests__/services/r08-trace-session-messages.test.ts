import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

const routing = vi.hoisted(() => ({ nodeKind: undefined as string | undefined }));

// Mock useStore before importing the service module
vi.mock('../../stores', () => ({
  useStore: {
    subscribe: () => () => {},
    getState: () => ({
      serverPort: '3210',
      serverToken: 'test-token-123',
      activeServerConnection: {
        kind: 'local',
        serverNodeKind: routing.nodeKind,
        serverId: 'local',
        studioId: 'local',
        label: 'Local Hana',
        baseUrl: 'http://127.0.0.1:3210',
        wsUrl: 'ws://127.0.0.1:3210',
        token: 'test-token-123',
        authState: 'paired',
        trustState: 'local',
        credentialKind: 'loopback_token',
        capabilities: ['chat'],
      },
    }),
  },
}));

import {
  projectRustHistoryForTrace,
  readTraceSessionMessages,
} from '../../services/rust-chat-history';
import { parseSessionMessages } from '../../settings/tabs/observability/trace-detail/trace-conversation-model';

const run = { state: 'known', runId: 'run-1' };

describe('projectRustHistoryForTrace（canonical → 旧 /api/sessions/messages 最小形状）', () => {
  it('user_message 透传 legacyFields 的 displayText/images，时间戳取 committedAtUnixMs', () => {
    const out = projectRustHistoryForTrace([{
      kind: 'user_message', messageId: 'u1', seq: '1', committedAtUnixMs: 111,
      text: '真实文本', run,
      legacyFields: { displayText: '显示文本', images: [{ data: 'aW1n', mimeType: 'image/png' }], clientMessageId: 'c1' },
    }]);
    expect(out).toEqual([{
      role: 'user', entryId: 'u1', content: '真实文本', timestamp: 111,
      displayText: '显示文本', images: [{ data: 'aW1n', mimeType: 'image/png' }],
    }]);
  });

  it('完整 turn：final 正文/推理块拼接、modelCallRef 只填 modelCallId、工具与终态归位', () => {
    const out = projectRustHistoryForTrace([
      { kind: 'assistant_segment', segmentId: 's1', run, phase: 'reasoning', segmentKind: 'reasoning', text: '思考', complete: true, firstSeq: '2' },
      {
        kind: 'tool_call', toolCallId: 'run-1-tc1', run, target: 'read', argsSummary: '{"path":"a.ts"}',
        status: 'success', firstSeq: '3',
        result: { status: 'success', details: { output: '文件内容' }, content: [], resourceRefs: [], truncated: false },
      },
      {
        kind: 'tool_call', toolCallId: 'run-1-tc2', run, target: 'exec',
        status: 'failed', firstSeq: '4',
        result: { status: 'failed', error: { code: 'io', message: '命令失败', retryable: false }, content: [], resourceRefs: [], truncated: false },
      },
      {
        kind: 'final_message', messageId: 'run-1-final', seq: '5', run, modelCallId: 'run-1:3',
        content: [{ type: 'text', text: '最终回复' }, { type: 'reasoning', text: '最终推理' }],
        committedAtUnixMs: 222,
      },
      { kind: 'run_terminal', runId: 'run-1', status: 'completed', eventSeq: '6' },
    ]);
    expect(out).toHaveLength(1);
    expect(out[0]).toEqual({
      role: 'assistant', entryId: 'run-1-final', content: '最终回复', timestamp: 222,
      thinking: '最终推理',
      modelCallRef: { modelCallId: 'run-1:3', traceId: null, parentCallId: null },
      turnStatus: 'completed',
      toolCalls: [
        {
          id: 'run-1-tc1', name: 'read', status: 'success', success: true,
          args: '{"path":"a.ts"}', details: { output: '文件内容' },
        },
        { id: 'run-1-tc2', name: 'exec', status: 'failed', success: false, error: '命令失败' },
      ],
    });
    // 与生产解析器的真实兼容性证明（不是形状复述）。
    const parsed = parseSessionMessages(out);
    expect(parsed).not.toBeNull();
    expect(parsed![0].role).toBe('assistant');
    expect(parsed![0].content).toBe('最终回复');
    expect(parsed![0].modelCallRef?.modelCallId).toBe('run-1:3');
    expect(parsed![0].toolCalls).toHaveLength(2);
  });

  it('无 final 的取消 run：正文/推理来自段、turnStatus 映射 aborted、时间戳诚实置 null', () => {
    const out = projectRustHistoryForTrace([
      { kind: 'assistant_segment', segmentId: 's1', run, phase: 'reasoning', segmentKind: 'reasoning', text: '部分推理', complete: false, firstSeq: '2' },
      { kind: 'assistant_segment', segmentId: 's2', run, phase: 'final_answer', segmentKind: 'text', text: '半截回复', complete: false, firstSeq: '3' },
      { kind: 'run_terminal', runId: 'run-1', status: 'cancelled', eventSeq: '4' },
    ]);
    expect(out).toEqual([{
      role: 'assistant', entryId: 'run-1-partial', content: '半截回复', timestamp: null,
      thinking: '部分推理', turnStatus: 'aborted',
    }]);
    const parsed = parseSessionMessages(out);
    expect(parsed![0].turnStatus).toBe('aborted');
    expect(parsed![0].timestampMs).toBeNull();
  });

  it('interrupted_needs_attention 如实标为 failed，绝不显示为正常', () => {
    const out = projectRustHistoryForTrace([
      { kind: 'assistant_segment', segmentId: 's1', run, phase: 'unresolved', segmentKind: 'text', text: 'x', complete: false, firstSeq: '2' },
      { kind: 'run_terminal', runId: 'run-1', status: 'interrupted_needs_attention', eventSeq: '3' },
    ]);
    expect(out[0].turnStatus).toBe('failed');
  });

  it('多 turn 与 user 消息保持时间线顺序', () => {
    const out = projectRustHistoryForTrace([
      { kind: 'user_message', messageId: 'u1', seq: '1', committedAtUnixMs: 1, text: '一', run },
      { kind: 'final_message', messageId: 'run-1-final', seq: '2', run, content: [{ type: 'text', text: '答一' }], committedAtUnixMs: 2 },
      { kind: 'user_message', messageId: 'u2', seq: '3', committedAtUnixMs: 3, text: '二', run },
      { kind: 'final_message', messageId: 'run-2-final', seq: '4', run: { state: 'known', runId: 'run-2' }, content: [{ type: 'text', text: '答二' }], committedAtUnixMs: 4 },
    ]);
    expect(out.map(m => [m.role, m.entryId])).toEqual([
      ['user', 'u1'], ['assistant', 'run-1-final'], ['user', 'u2'], ['assistant', 'run-2-final'],
    ]);
  });

  it('reset_marker / legacy_raw 跳过；非数组输入响亮失败', () => {
    const out = projectRustHistoryForTrace([
      { kind: 'reset_marker', messageId: 'r1', seq: '2', reason: 'retry' },
      { kind: 'legacy_raw', messageId: 'old', seq: '3', raw: { kept: true }, reason: 'unparseable' },
    ]);
    expect(out).toEqual([]);
    expect(() => projectRustHistoryForTrace(null as unknown as unknown[])).toThrow('历史输入无效');
  });
});

describe('readTraceSessionMessages（Node/Rust 双分支）', () => {
  const mockFetch = vi.fn();

  beforeEach(() => {
    mockFetch.mockReset();
    vi.stubGlobal('fetch', mockFetch);
  });

  afterEach(() => {
    vi.restoreAllMocks();
    routing.nodeKind = undefined;
  });

  it('Node 连接：沿旧 /api/sessions/messages URL 原样返回 messages', async () => {
    routing.nodeKind = 'node-server';
    mockFetch.mockResolvedValueOnce(new Response(
      JSON.stringify({ messages: [{ role: 'user', entryId: 'u1', content: 'hi', timestamp: 1 }] }),
      { status: 200, headers: { 'content-type': 'application/json' } },
    ));
    const out = await readTraceSessionMessages('sess-1');
    expect(mockFetch).toHaveBeenCalledTimes(1);
    expect(String(mockFetch.mock.calls[0][0])).toContain('/api/sessions/messages?sessionId=sess-1&all=1');
    expect(out).toEqual([{ role: 'user', entryId: 'u1', content: 'hi', timestamp: 1 }]);
  });

  it('Rust 连接：读 canonical 历史并投影为旧最小形状', async () => {
    routing.nodeKind = 'lingxi-service';
    mockFetch.mockResolvedValueOnce(new Response(JSON.stringify({
      schemaVersion: 'lingxi.canonical-history.v1',
      sessionId: 'sess-1',
      revision: 'rev-1',
      items: [
        { kind: 'user_message', messageId: 'u1', seq: '1', committedAtUnixMs: 1, text: '问', run },
        { kind: 'final_message', messageId: 'run-1-final', seq: '2', run, content: [{ type: 'text', text: '答' }], committedAtUnixMs: 2 },
      ],
      page: { hasMore: false, nextBefore: null },
    }), { status: 200, headers: { 'content-type': 'application/json', 'etag': 'rev-1' } }));
    const out = await readTraceSessionMessages('sess-1');
    expect(String(mockFetch.mock.calls[0][0])).toContain('/lingxi/v1/sessions/sess-1/history');
    expect(out).toEqual([
      { role: 'user', entryId: 'u1', content: '问', timestamp: 1 },
      { role: 'assistant', entryId: 'run-1-final', content: '答', timestamp: 2 },
    ]);
  });

  it('Node 分支非 2xx 响亮失败（调用方既有语义：catch → calls-only 降级）', async () => {
    routing.nodeKind = 'node-server';
    mockFetch.mockResolvedValueOnce(new Response('nope', { status: 500 }));
    await expect(readTraceSessionMessages('sess-1')).rejects.toThrow('500');
  });
});
