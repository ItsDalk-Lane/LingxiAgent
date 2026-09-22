// P05 独立验收反例（验收子代理构造，不属于执行者交付物）。
// 归属（P05-FIXR2 头注事实修正，口径同 FI 文件）：本文件匹配 vitest 默认 include
// （npm test 的 --exclude 不含 artifacts/），已并入默认测试集；开发期曾在隔离临时根
// /tmp/p05x-accept 迭代，终态为仓库内默认集成员。
// 可移植性（P05-FIXR2）：import 一律为相对本文件的仓库相对路径（../../../../ = 仓库根），
// 无机器绝对路径，提交后 CI/其他机器可直接解析。
// 三个反例方向：
//   C1 跨流迟到事件（消费端 live 路径）：旧流 segment 的迟到 canonical delta 不得污染新轮 Run 投影。
//   C2 ring 截断边界精确性 + 字节上限 + 大事件压缩可观察（真实 session-stream-store）。
//   C3 资源历史同名文件污染（v2 实时 toolCallId 引用链）：展开全文必须读持久记录而非当前磁盘；
//      且 locator 会话不匹配时必须拒绝（引用不是授权凭证）。
// @vitest-environment jsdom

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { describe, expect, it, vi } from 'vitest';

import { streamBufferManager } from "../../../../desktop/src/react/hooks/use-stream-buffer.ts";
import { useStore } from "../../../../desktop/src/react/stores/index.ts";
import { readLiveAssistantMessage } from "../../../../desktop/src/react/stores/live-turn-store.ts";

import {
  appendSessionStreamEvent,
  beginSessionStream,
  createSessionStreamState,
  resumeSessionStream,
} from "../../../../server/session-stream-store.ts";

import {
  createLiveToolContentDescriptor,
  resolveHistoryDeferredContent,
} from "../../../../server/history-deferred-content.ts";

// ── C1：跨流迟到事件（消费端 live 投影污染反例） ──────────────────────────────

const C1_SESSION = '/tmp/p05x-accept-c1-session.jsonl';

function resetC1Store() {
  streamBufferManager.clearAll();
  useStore.setState({
    currentSessionId: null,
    currentSessionPath: null,
    sessions: [],
    sessionLocatorsById: {},
    streamingSessions: [],
  } as never);
  useStore.getState().clearSession(C1_SESSION);
  useStore.getState().initSession(C1_SESSION, [
    { type: 'message', data: { id: 'u1', role: 'user', text: '第一问' } },
    { type: 'message', data: { id: 'u2', role: 'user', text: '第二问' } },
  ], false);
}

function feedC1(msgs: any[]) {
  for (const msg of msgs) streamBufferManager.handle(msg);
}

describe('C1 跨流迟到事件：旧流 segment 迟到 delta 不得污染新轮 Run', () => {
  // F03：原反例保留全部断言，恢复普通正向验收。
  it('Run A 结束后开启 Run B，旧流(segA/streamA)迟到 delta 不得出现在 Run B 的 answer', async () => {
    resetC1Store();

    feedC1([
      { type: 'assistant_run_start', sessionPath: C1_SESSION, runId: 'runA', streamId: 'streamA' },
      { type: 'assistant_segment_start', sessionPath: C1_SESSION, streamId: 'streamA', seq: 1, segmentId: 'segA', kind: 'text', semanticPhase: 'final_answer' },
      { type: 'assistant_segment_delta', sessionPath: C1_SESSION, streamId: 'streamA', seq: 2, segmentId: 'segA', semanticPhase: 'final_answer', delta: 'A轮正文' },
      { type: 'assistant_segment_end', sessionPath: C1_SESSION, streamId: 'streamA', seq: 3, segmentId: 'segA', semanticPhase: 'final_answer' },
      { type: 'assistant_run_end', sessionPath: C1_SESSION, runId: 'runA', streamId: 'streamA', status: 'completed' },
      // Run B：新流
      { type: 'assistant_run_start', sessionPath: C1_SESSION, runId: 'runB', streamId: 'streamB' },
      { type: 'assistant_segment_start', sessionPath: C1_SESSION, streamId: 'streamB', seq: 1, segmentId: 'segB', kind: 'text', semanticPhase: 'final_answer' },
      { type: 'assistant_segment_delta', sessionPath: C1_SESSION, streamId: 'streamB', seq: 2, segmentId: 'segB', semanticPhase: 'final_answer', delta: 'B轮正文' },
    ]);

    // 迟到事件：旧流 segA 的尾巴 delta（seq 高于 A 轮已应用 seq，模拟晚到的旧流事件）
    feedC1([
      { type: 'assistant_segment_delta', sessionPath: C1_SESSION, streamId: 'streamA', seq: 9, segmentId: 'segA', semanticPhase: 'final_answer', delta: '【迟到旧流尾巴】' },
      { type: 'assistant_run_end', sessionPath: C1_SESSION, runId: 'runB', streamId: 'streamB', status: 'completed' },
    ]);

    await vi.waitFor(() => {
      const state: any = useStore.getState();
      expect(state.chatSessions[C1_SESSION]?.items?.length).toBeGreaterThan(2);
    });

    const state: any = useStore.getState();
    const items = state.chatSessions[C1_SESSION]?.items ?? [];
    const serialized = JSON.stringify(items);

    // Run B 的正文可正常投影
    expect(serialized).toContain('B轮正文');
    // 迟到旧流尾巴不得进入任何 live 投影正文
    expect(serialized).not.toContain('【迟到旧流尾巴】');
  });
});

// ── C2：ring 截断边界 / 字节上限 / 大事件压缩（真实 store） ───────────────────

describe('C2 ring 边界与压缩可观察性（真实 session-stream-store）', () => {
  it('边界精确：sinceSeq=firstSeq-1 不算截断；firstSeq-2 算截断且补发从 firstSeq 起', () => {
    const state = createSessionStreamState({ maxEvents: 10 });
    beginSessionStream(state, 's_boundary');
    for (let i = 0; i < 25; i += 1) {
      appendSessionStreamEvent(state, { type: 'text_delta', delta: `d${i}` });
    }
    const firstSeq = state.events[0].seq;
    expect(firstSeq).toBe(16); // 26 事件、上限 10 → 存活 seq 16..25

    const atBoundary = resumeSessionStream(state, { streamId: 's_boundary', sinceSeq: firstSeq - 1 });
    expect(atBoundary.truncated).toBe(false);
    expect(atBoundary.sinceSeq).toBe(firstSeq - 1);
    expect(atBoundary.events.every((e: any) => e.seq > firstSeq - 1)).toBe(true);

    const belowBoundary = resumeSessionStream(state, { streamId: 's_boundary', sinceSeq: firstSeq - 2 });
    expect(belowBoundary.truncated).toBe(true);
    expect(belowBoundary.sinceSeq).toBe(firstSeq - 1);
    expect(belowBoundary.events[0].seq).toBe(firstSeq);
    expect(belowBoundary.nextSeq).toBe(state.nextSeq);
  });

  it('字节上限触发 trim：超 maxBytes 后旧事件被丢、resume 显式 truncated（不报完整）', () => {
    const state = createSessionStreamState({ maxEvents: 1000, maxBytes: 4 * 1024 });
    beginSessionStream(state, 's_bytes');
    for (let i = 0; i < 40; i += 1) {
      appendSessionStreamEvent(state, { type: 'text_delta', delta: 'x'.repeat(512) });
    }
    expect(state.events.length).toBeLessThan(40); // 确实触发了字节 trim
    expect(state.droppedEvents).toBeGreaterThan(0);
    const resumed = resumeSessionStream(state, { streamId: 's_bytes', sinceSeq: 0 });
    expect(resumed.truncated).toBe(true);
    expect(resumed.events.length).toBe(state.events.length);
    expect(resumed.nextSeq).toBe(41);
  });

  it('超大单事件压缩可观察：compacted 标记与 originalByteLength 存在，不把压缩结果当原始完整', () => {
    const state = createSessionStreamState({ maxEvents: 100, maxBytes: 8 * 1024 * 1024, maxEventBytes: 2 * 1024 });
    beginSessionStream(state, 's_compact');
    appendSessionStreamEvent(state, { type: 'content_block', block: { type: 'file', data: 'B'.repeat(64 * 1024) } });
    const resumed = resumeSessionStream(state, { streamId: 's_compact', sinceSeq: 0 });
    const stored = resumed.events[0].event as any;
    expect(stored.compacted).toBe(true);
    expect(Number(stored.originalByteLength)).toBeGreaterThan(60 * 1024);
    // 压缩后的正文不得伪装成原始完整内容
    expect(JSON.stringify(stored)).not.toContain('BBBBBBBB');
  });
});

// ── C3：v2 实时引用链的同名文件污染 + 引用非授权凭证 ─────────────────────────

describe('C3 资源历史（v2 实时 toolCallId 引用）：持久记录 vs 磁盘陷阱 + 引用非授权凭证', () => {
  it('展开全文读持久 toolResult 的 details（v2 合法 kind=tool_output），不受磁盘陷阱影响；会话不匹配拒绝', async () => {
    const diskFile = path.join(os.tmpdir(), `p05x-c3-${Date.now()}-${Math.random().toString(36).slice(2, 8)}.txt`);
    const delivered = 'D'.repeat(9000) + ':v2-delivered-end';
    fs.writeFileSync(diskFile, delivered);
    try {
      // 持久 toolResult 记录的 output 与磁盘同名文件内容分叉：记录为准
      fs.writeFileSync(diskFile, 'C'.repeat(9000) + ':v2-current-end');
      const sources = [
        {
          id: 'assistant-c3', role: 'assistant',
          content: [{ id: 'call-c3', type: 'toolCall', name: 'exec_command', input: { command: 'cat out.txt' } }],
        },
        {
          id: 'result-c3', role: 'toolResult', toolCallId: 'call-c3', toolName: 'exec_command',
          content: [{ type: 'text', text: delivered }],
        },
      ];

      const descriptor = createLiveToolContentDescriptor('/tmp/p05x-c3-session.jsonl', 'call-c3', 'tool_output', delivered.length);
      expect(descriptor).not.toBeNull();
      const resolved = resolveHistoryDeferredContent(sources, descriptor!.id, '/tmp/p05x-c3-session.jsonl');
      expect(resolved?.content).toBe(delivered);
      expect(resolved?.content).not.toContain(':v2-current-end');

      // 会话不匹配（locator 会话 ≠ 本次授权会话）：必须拒绝，引用不是授权凭证
      const wrongSession = resolveHistoryDeferredContent(sources, descriptor!.id, '/tmp/p05x-c3-OTHER-session.jsonl');
      expect(wrongSession).toBeNull();

      // toolCallId 不存在（伪造引用）：解析不出内容，不回退读磁盘
      const ghost = createLiveToolContentDescriptor('/tmp/p05x-c3-session.jsonl', 'call-ghost', 'tool_output', 10);
      expect(resolveHistoryDeferredContent(sources, ghost!.id, '/tmp/p05x-c3-session.jsonl')).toBeNull();
    } finally {
      fs.rmSync(diskFile, { force: true });
    }
  });
});
