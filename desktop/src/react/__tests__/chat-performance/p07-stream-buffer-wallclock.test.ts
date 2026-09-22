/**
 * P07-T04｜流式 UI 更新 wall-clock 性能画像（真实计时器，A04/A06/A07 场景锚点）。
 *
 * 与既有 chat-performance-baseline.test.ts（fake timers、确定性发布次数）互补：
 * 本文件用真实计时器测量：
 *   1. canonical 流（assistant_segment_*，服务端真实事件形状）在大体量 delta 下
 *      的缓冲吞吐（handle 调用/s）与主线程响应度（10ms 探针最大漂移）；
 *   2. 合并窗口内有界刷新：stream_flush 次数受 30fps 上限约束（上限断言而非精确值）；
 *   3. A06 形状：合并窗口内 content_block（文件到达）强制边界发布——最终 delta
 *      与附件同帧不丢；随后 assistant_run_end 收口 sealed；
 *   4. 流式期间不预解析 Markdown（markdown_parse = 0）。
 *
 * 输出 P07_UI_SAMPLE 一行 JSON（无正文副本，只有计数与毫秒）供阶段报告归档。
 * 阈值为宽松回归锚点（数量级守卫），不是微秒 SLA。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { streamBufferManager } from '../../hooks/use-stream-buffer';
import { useStore } from '../../stores';
import {
  observeChatPerformance,
  type ChatPerformanceEvent,
} from '../../utils/chat-performance';

const PATH = '/p07/chat-stream-wallclock.jsonl';

function currentAssistantText(): string {
  const session = useStore.getState().chatSessions[PATH];
  const assistant = session?.items?.find((item) => item.type === 'message' && item.data.role === 'assistant');
  if (!assistant || assistant.type !== 'message') return '';
  const blocks = assistant.data.blocks || [];
  return blocks
    .filter((block) => block.type === 'text')
    .map((block) => (block.type === 'text' ? block.source || '' : ''))
    .join('');
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function count(events: ChatPerformanceEvent[], name: string): number {
  return events.filter((event) => event.name === name).length;
}

/** 主线程响应度探针：10ms 周期定时器的最大漂移（benchmark-terminal-ui 同款口径）。 */
function startResponsivenessProbe() {
  const state = { maxDriftMs: 0 };
  let expectedAt = performance.now() + 10;
  const timer = setInterval(() => {
    const now = performance.now();
    state.maxDriftMs = Math.max(state.maxDriftMs, now - expectedAt);
    expectedAt = now + 10;
  }, 10);
  return { state, stop: () => clearInterval(timer) };
}

describe('P07-T04 stream buffer wall-clock profile', () => {
  beforeEach(() => {
    streamBufferManager.clearAll();
    useStore.setState({
      currentSessionId: null,
      currentSessionPath: null,
      sessions: [],
      sessionLocatorsById: {},
    } as never);
    useStore.getState().clearSession(PATH);
    useStore.getState().initSession(PATH, [{
      type: 'message',
      data: { id: 'user-p07', role: 'user', text: 'P07 流式压力输入' },
    }], false);
  });

  afterEach(() => {
    streamBufferManager.clearAll();
  });

  it('canonical segment 流 50,000 delta：吞吐、有界刷新与响应度（真实计时器）', async () => {
    vi.useRealTimers();
    const DELTAS = 50_000;
    const SEGMENT = 'seg-p07-text';
    const deltaText = '灵犀界面性能样本段，mixed EN text。';

    const events: ChatPerformanceEvent[] = [];
    const stop = observeChatPerformance((event) => events.push(event));
    const probe = startResponsivenessProbe();

    streamBufferManager.handle({ type: 'assistant_run_start', sessionPath: PATH, runId: 'run-p07', streamId: 's_p07' });
    streamBufferManager.handle({
      type: 'assistant_segment_start', sessionPath: PATH, segmentId: SEGMENT, kind: 'text', semanticPhase: 'answer',
    });
    const t0 = performance.now();
    for (let i = 0; i < DELTAS; i++) {
      streamBufferManager.handle({
        type: 'assistant_segment_delta', sessionPath: PATH, segmentId: SEGMENT,
        delta: deltaText, seq: i + 1, semanticPhase: 'answer',
      });
    }
    const burstMs = performance.now() - t0;
    streamBufferManager.handle({
      type: 'assistant_segment_end', sessionPath: PATH, segmentId: SEGMENT, semanticPhase: 'answer',
    });
    streamBufferManager.handle({
      type: 'assistant_run_end', sessionPath: PATH, runId: 'run-p07', streamId: 's_p07',
    });
    // 等待最后一帧定时器发布落地（合并窗口 ≤ 1 帧 + 余量）。
    await sleep(200);
    probe.stop();
    stop();

    const text = currentAssistantText();
    const flushes = count(events, 'stream_flush');
    const sample = {
      deltas: DELTAS,
      burst_wall_ms: +burstMs.toFixed(1),
      handle_per_sec: Math.round(DELTAS / (burstMs / 1000)),
      stream_flush_count: flushes,
      markdown_parse_count: count(events, 'markdown_parse'),
      structural_updates: count(events, 'structural_message_update'),
      probe_max_drift_ms: +probe.state.maxDriftMs.toFixed(1),
    };
    console.log(`P07_UI_SAMPLE ${JSON.stringify(sample)}`);

    // 内容完整：末帧不丢（收口前全部 delta 都进入最终 text source）。
    expect(text).toHaveLength(DELTAS * deltaText.length);
    // 流式期间不预解析 Markdown。
    expect(sample.markdown_parse_count).toBe(0);
    // 有界刷新：一次同步突发 + 收口，发布次数远小于 delta 数（30fps 合并生效）。
    expect(flushes).toBeGreaterThan(0);
    expect(flushes).toBeLessThan(Math.ceil(DELTAS / 100));
    // 宽松数量级回归锚点（非 SLA）：50k delta 缓冲 < 5s；主线程单次阻塞 < 1s。
    expect(burstMs).toBeLessThan(5_000);
    expect(probe.state.maxDriftMs).toBeLessThan(1_000);
  }, 30_000);

  it('合并窗口内 content_block 与终局事件不丢（A06 形状，真实计时器）', async () => {
    vi.useRealTimers();
    const SEGMENT = 'seg-p07-a06';
    const deltaText = '合并窗口附件场景正文。';
    const events: ChatPerformanceEvent[] = [];
    const stop = observeChatPerformance((event) => events.push(event));

    streamBufferManager.handle({ type: 'assistant_run_start', sessionPath: PATH, runId: 'run-a06', streamId: 's_a06' });
    streamBufferManager.handle({
      type: 'assistant_segment_start', sessionPath: PATH, segmentId: SEGMENT, kind: 'text', semanticPhase: 'answer',
    });
    // 尚未过合并窗口（同步连发）时：最后一批 delta + 文件块 + 收口紧随其后。
    for (let i = 0; i < 500; i++) {
      streamBufferManager.handle({
        type: 'assistant_segment_delta', sessionPath: PATH, segmentId: SEGMENT,
        delta: deltaText, seq: i + 1, semanticPhase: 'answer',
      });
    }
    streamBufferManager.handle({
      type: 'content_block', sessionPath: PATH,
      block: { type: 'file', path: '/tmp/p07-a06-artifact.md', name: 'p07-a06-artifact.md' } as never,
    });
    streamBufferManager.handle({
      type: 'assistant_segment_delta', sessionPath: PATH, segmentId: SEGMENT,
      delta: 'TAIL_AFTER_FILE', seq: 501, semanticPhase: 'answer',
    });
    streamBufferManager.handle({
      type: 'assistant_segment_end', sessionPath: PATH, segmentId: SEGMENT, semanticPhase: 'answer',
    });
    streamBufferManager.handle({
      type: 'assistant_run_end', sessionPath: PATH, runId: 'run-a06', streamId: 's_a06',
    });
    await sleep(200);
    stop();

    const session = useStore.getState().chatSessions[PATH];
    const assistant = session?.items?.find((item) => item.type === 'message' && item.data.role === 'assistant');
    const blocks = assistant?.type === 'message' ? assistant.data.blocks || [] : [];
    const text = blocks
      .filter((block) => block.type === 'text')
      .map((block) => (block.type === 'text' ? block.source || '' : ''))
      .join('');
    // 最终 delta 与文件块都保留：附件后正文继续累积，无内容丢失。
    expect(text.startsWith(deltaText.repeat(500))).toBe(true);
    expect(text.endsWith('TAIL_AFTER_FILE')).toBe(true);
    expect(blocks.some((block) => block.type === 'file')).toBe(true);
    expect(count(events, 'markdown_parse')).toBe(0);
  });
});
