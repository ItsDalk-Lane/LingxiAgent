/**
 * 用量/用时胶囊 · 聚合与格式化单测（任务2 验收 d/e 组 + 渲染资格）。
 *
 * 锁定 aggregateTurnUsage 的求和口径（Σ、null 不冒充 0）、缓存命中百分比
 * （含防 99.95→100 失真）、TPS = Σ输出 ÷ ΣdurationMs、多 call 轮次求合、
 * 完成 turn 的窗口资格（streaming/failed/aborted/缺时间戳 → 无胶囊）。
 */
import { describe, expect, it, vi } from 'vitest';
import type { ModelObservabilityCallListItem } from '../../../../../shared/model-observability-api-contract.ts';
import {
  aggregateTurnUsage,
  formatCacheHitPercent,
  formatExactTokens,
  formatRunDuration,
  formatTokensCompact,
  formatTokensPerSecond,
  turnUsageWindow,
  turnUsageWindowFromNeighbors,
} from '../../components/chat/turn-usage';
import type { AssistantTurnProjection, ChatListItem } from '../../stores/chat-types';

const T0 = Date.UTC(2026, 8, 5, 10, 0, 0, 0);

function callItem(overrides: {
  callId?: string;
  durationMs?: number | null;
  firstResponseAt?: string | null;
  provider?: string | null;
  modelId?: string | null;
  availability?: string;
  summary?: Record<string, number | null> | null;
}): ModelObservabilityCallListItem {
  const availability = overrides.availability ?? 'present';
  const durationMs = 'durationMs' in overrides ? overrides.durationMs : 1000;
  return {
    callId: overrides.callId ?? 'mc_x',
    traceId: 'mt_x',
    parentCallId: null,
    startedAt: new Date(T0).toISOString(),
    endedAt: new Date(T0 + (durationMs ?? 1000)).toISOString(),
    durationMs,
    firstResponseAt: 'firstResponseAt' in overrides ? overrides.firstResponseAt : new Date(T0 + 800).toISOString(),
    terminalStatus: 'ok',
    model: { provider: overrides.provider ?? 'openai', modelId: overrides.modelId ?? 'gpt-test', api: 'responses' },
    usage: {
      availability,
      status: availability === 'present' ? 'ok' : null,
      summary: availability === 'present'
        ? (overrides.summary ?? {
          inputTokens: 0,
          inputUncachedTokens: 0,
          outputTokens: 0,
          reasoningTokens: 0,
          cacheReadTokens: 0,
          cacheWriteTokens: 0,
          totalTokens: 0,
          costTotal: null,
        })
        : null,
    },
  } as unknown as ModelObservabilityCallListItem;
}

const WINDOW = { startedAt: T0, completedAt: T0 + 8000, runMs: 8000 };

describe('turn-usage 聚合（验收 d：求和与命中百分比）', () => {
  it('单 call 各桶求和正确，缓存命中 = 缓存读取 ÷ (总量 − 输出)', () => {
    const stats = aggregateTurnUsage([
      callItem({
        durationMs: 4000,
        summary: {
          inputTokens: 1000, inputUncachedTokens: 250, outputTokens: 100,
          reasoningTokens: 30, cacheReadTokens: 750, cacheWriteTokens: 0,
          totalTokens: 1100, costTotal: null,
        },
      })],
      WINDOW,
    );
    expect(stats).not.toBeNull();
    expect(stats!.uncachedInputTokens).toBe(250);
    expect(stats!.cacheReadTokens).toBe(750);
    expect(stats!.cacheWriteTokens).toBe(0);
    expect(stats!.outputTokens).toBe(100);
    expect(stats!.reasoningTokens).toBe(30);
    expect(stats!.totalTokens).toBe(1100);
    // 命中 = 750 / (1100 − 100) = 75%（1 位小数口径显示 75）
    expect(stats!.cacheHitPercent).toBe('75');
    // TPS = 100 输出 ÷ 4s = 25；展示用时优先取 window.runMs（实时收尾可信）
    expect(stats!.tokensPerSecond).toBe(25);
    expect(stats!.runMs).toBe(8000);
    // TTFT = 最早响应到达 − 轮开始 = 800ms
    expect(stats!.ttftMs).toBe(800);
  });

  it('null 字段不冒充 0：无缓存读取事实 → 命中行无数据；TPS 缺时长 → null', () => {
    const stats = aggregateTurnUsage([
      callItem({
        durationMs: null,
        firstResponseAt: null,
        summary: {
          inputTokens: 500, inputUncachedTokens: null, outputTokens: 40,
          reasoningTokens: null, cacheReadTokens: null, cacheWriteTokens: null,
          totalTokens: 540, costTotal: null,
        },
      })],
      WINDOW,
    );
    expect(stats).not.toBeNull();
    expect(stats!.uncachedInputTokens).toBeNull();
    expect(stats!.cacheReadTokens).toBeNull();
    expect(stats!.cacheWriteTokens).toBeNull();
    expect(stats!.reasoningTokens).toBeNull();
    expect(stats!.cacheHitPercent).toBeNull();
    expect(stats!.tokensPerSecond).toBeNull();
    expect(stats!.ttftMs).toBeNull();
    // 总量与输出有事实 → 胶囊锚点成立
    expect(stats!.totalTokens).toBe(540);
    expect(stats!.outputTokens).toBe(40);
  });

  it('多 call TTFT 取最早响应到达，无 firstResponseAt 事实 → null', () => {
    const stats = aggregateTurnUsage([
      callItem({ callId: 'mc_t2', firstResponseAt: new Date(T0 + 1500).toISOString() }),
      callItem({ callId: 'mc_t1', firstResponseAt: new Date(T0 + 400).toISOString() }),
    ], WINDOW);
    expect(stats!.ttftMs).toBe(400);
    expect(aggregateTurnUsage([
      callItem({ callId: 'mc_t3', firstResponseAt: null }),
    ], WINDOW)!.ttftMs).toBeNull();
  });

  it('无 usage 事实（老会话）→ 整体 null，两个胶囊都不渲染', () => {
    expect(aggregateTurnUsage([callItem({ availability: 'unknown', summary: null })], WINDOW)).toBeNull();
    expect(aggregateTurnUsage([callItem({ availability: 'corrupt', summary: null })], WINDOW)).toBeNull();
    expect(aggregateTurnUsage([], WINDOW)).toBeNull();
  });
});

describe('turn-usage 聚合（验收 e：多 call 轮次求和）', () => {
  it('三个 call 跨桶求和，null 值跳过、模型标签去重保序', () => {
    const stats = aggregateTurnUsage([
      callItem({
        callId: 'mc_1', durationMs: 1000, provider: 'openai', modelId: 'gpt-a',
        summary: {
          inputTokens: 100, inputUncachedTokens: 60, outputTokens: 20,
          reasoningTokens: 5, cacheReadTokens: 40, cacheWriteTokens: 10,
          totalTokens: 120, costTotal: null,
        },
      }),
      callItem({
        callId: 'mc_2', durationMs: 2000, provider: 'openai', modelId: 'gpt-a',
        summary: {
          inputTokens: 300, inputUncachedTokens: 40, outputTokens: 80,
          reasoningTokens: 15, cacheReadTokens: 260, cacheWriteTokens: null,
          totalTokens: 380, costTotal: null,
        },
      }),
      callItem({
        callId: 'mc_3', durationMs: 500, provider: 'anthropic', modelId: 'claude-b',
        summary: {
          inputTokens: null, inputUncachedTokens: 100, outputTokens: 50,
          reasoningTokens: null, cacheReadTokens: null, cacheWriteTokens: 25,
          totalTokens: 500, costTotal: null,
        },
      }),
    ], WINDOW);
    expect(stats).not.toBeNull();
    expect(stats!.uncachedInputTokens).toBe(200);        // 60 + 40 + 100
    expect(stats!.cacheReadTokens).toBe(300);            // 40 + 260 + (null)
    expect(stats!.cacheWriteTokens).toBe(35);            // 10 + (null) + 25
    expect(stats!.outputTokens).toBe(150);               // 20 + 80 + 50
    expect(stats!.reasoningTokens).toBe(20);             // 5 + 15 + (null)
    expect(stats!.totalTokens).toBe(1000);               // 120 + 380 + 500
    expect(stats!.tokensPerSecond).toBeCloseTo(150 / 3.5, 10); // Σ输出 ÷ ΣdurationMs
    expect(stats!.modelLabels).toEqual(['openai/gpt-a', 'anthropic/claude-b']);
    // 命中 = 300 / (1000 − 150)，1 位小数
    expect(stats!.cacheHitPercent).toBe(formatCacheHitPercent(300, 850, 1));
  });

  it('历史路径：window 无 runMs 时展示用时由账本 ended_at 推导（最后调用结束 − 轮开始）', () => {
    const stats = aggregateTurnUsage([
      callItem({ callId: 'mc_h1', durationMs: 1000, summary: { inputTokens: 10, outputTokens: 5, totalTokens: 15, inputUncachedTokens: 10, reasoningTokens: null, cacheReadTokens: null, cacheWriteTokens: null, costTotal: null } }),
      callItem({ callId: 'mc_h2', durationMs: 2000, summary: { inputTokens: 10, outputTokens: 5, totalTokens: 15, inputUncachedTokens: 10, reasoningTokens: null, cacheReadTokens: null, cacheWriteTokens: null, costTotal: null } }),
    ], { startedAt: T0, completedAt: T0 + 90_000 });
    expect(stats).not.toBeNull();
    // ended_at = started_at + durationMs：最后调用 T0+2000 结束 → runMs = 2000
    expect(stats!.runMs).toBe(2000);
  });

  it('历史路径且账本无 ended_at 事实 → runMs 为 null，用时胶囊不渲染 0', () => {
    const stats = aggregateTurnUsage([
      { ...callItem({ durationMs: 1000, summary: { inputTokens: 10, outputTokens: 5, totalTokens: 15, inputUncachedTokens: 10, reasoningTokens: null, cacheReadTokens: null, cacheWriteTokens: null, costTotal: null } }), endedAt: null },
    ] as unknown as ModelObservabilityCallListItem[], { startedAt: T0, completedAt: T0 + 90_000 });
    expect(stats).not.toBeNull();
    expect(stats!.runMs).toBeNull();
  });
});

describe('formatCacheHitPercent（防 99.95→100 失真，1:1 移植算法）', () => {
  it('部分命中按 1 位小数取整，永不把部分命中显示成 100', () => {
    expect(formatCacheHitPercent(9990, 10000, 1)).toBe('99.9');
    expect(formatCacheHitPercent(99999, 100000, 1)).not.toBe('100');
    expect(formatCacheHitPercent(450, 900, 1)).toBe('50');
    expect(formatCacheHitPercent(0, 900, 1)).toBe('0');
  });

  it('全命中为 100、无输入（分母 0）为 null、正边界四舍五入进位', () => {
    expect(formatCacheHitPercent(900, 900, 1)).toBe('100');
    expect(formatCacheHitPercent(0, 0, 1)).toBeNull();
    // 5.95% 处正边界：进位不截断（ties round up）
    expect(formatCacheHitPercent(595, 10000, 1)).toBe('6');
  });
});

describe('紧凑/精确/时长/吞吐格式化（胶囊与弹窗标签）', () => {
  it('紧凑 K/M：517 / 12.2K / 517K / 1.2M', () => {
    expect(formatTokensCompact(517)).toBe('517');
    expect(formatTokensCompact(12200)).toBe('12.2K');
    expect(formatTokensCompact(517000)).toBe('517K');
    expect(formatTokensCompact(1_200_000)).toBe('1.2M');
  });

  it('千分位精确值不四舍五入', () => {
    expect(formatExactTokens(1234567)).toBe('1,234,567');
    expect(formatExactTokens(517)).toBe('517');
  });

  it('时长整秒与分秒拼接、吞吐 <10 一位小数', () => {
    expect(formatRunDuration(65000)).toBe('1分05秒');
    expect(formatRunDuration(8000)).toBe('8秒');
    expect(formatRunDuration(-5)).toBe('0秒');
    expect(formatTokensPerSecond(46.2)).toBe('46');
    expect(formatTokensPerSecond(3.14)).toBe('3.1');
  });
});

describe('turnUsageWindow 渲染资格（验收 b 的资格面）', () => {
  function turn(status: AssistantTurnProjection['status'], extra: Partial<AssistantTurnProjection> = {}): AssistantTurnProjection {
    return {
      id: 'turn_1',
      inputMessageId: null,
      assistantMessageIds: ['m1'],
      processBlockIds: [],
      answerBlockIds: [],
      resultBlockIds: [],
      controlBlockIds: [],
      status,
      startedAt: T0,
      completedAt: T0 + 5000,
      ...extra,
    };
  }

  it('completed 且时间戳齐备 → 出窗口（runMs = completedAt−startedAt 随窗口带出）', () => {
    expect(turnUsageWindow(turn('completed'))).toEqual({ startedAt: T0, completedAt: T0 + 5000, runMs: 5000 });
    expect(turnUsageWindow(turn('completed', { outcome: 'completed_without_user_output' }))).not.toBeNull();
  });

  it('streaming/failed/aborted 或缺时间戳 → 无胶囊', () => {
    expect(turnUsageWindow(turn('streaming'))).toBeNull();
    expect(turnUsageWindow(turn('failed'))).toBeNull();
    expect(turnUsageWindow(turn('aborted'))).toBeNull();
    expect(turnUsageWindow(turn('completed', { startedAt: undefined }))).toBeNull();
    expect(turnUsageWindow(turn('completed', { completedAt: undefined }))).toBeNull();
    expect(turnUsageWindow(turn('completed', { startedAt: T0 + 9000 }))).toBeNull();
    expect(turnUsageWindow(undefined)).toBeNull();
  });
});

describe('turnUsageWindowFromNeighbors 邻居回退窗口（历史投影无时间戳时的兜底）', () => {
  const T_USER = T0 - 2000;
  const T_ASSISTANT = T0 + 6000;   // assistant entry 落盘=回复开始，远早于轮结束
  const T_NEXT_USER = T0 + 90_000;

  function items(variants: {
    assistantStatus?: AssistantTurnProjection['status'];
    withAssistantTimestamp?: boolean;
    interludeBetween?: boolean;
    dropUserTimestamp?: boolean;
    withNextUser?: boolean;
  } = {}): ChatListItem[] {
    const list: ChatListItem[] = [
      { type: 'message', data: { id: 'u1', role: 'user', text: 'hi', ...(variants.dropUserTimestamp ? {} : { timestamp: T_USER }) } },
    ];
    if (variants.interludeBetween) {
      // interlude 形状仅用于占位（回溯须跳过非 message 项），不做完整类型建模
      list.push({ type: 'interlude', id: 'int1', data: { type: 'interlude', label: 'x' } } as unknown as ChatListItem);
    }
    list.push({
      type: 'message',
      data: {
        id: 'a1',
        role: 'assistant',
        blocks: [],
        ...(variants.withAssistantTimestamp === false ? {} : { timestamp: T_ASSISTANT }),
        turnProjection: {
          id: 'turn_1',
          inputMessageId: null,
          assistantMessageIds: ['a1'],
          processBlockIds: [],
          answerBlockIds: [],
          resultBlockIds: [],
          controlBlockIds: [],
          status: variants.assistantStatus ?? 'completed',
        },
      },
    });
    if (variants.withNextUser) {
      list.push({ type: 'message', data: { id: 'u2', role: 'user', text: 'next', timestamp: T_NEXT_USER } });
    }
    return list;
  }

  it('有下一轮：窗口 = 上一条 user 时刻 ~ 下一轮 user 时刻−1（assistant entry 时刻只是回复开始，不作上界）', () => {
    expect(turnUsageWindowFromNeighbors(items({ withNextUser: true, interludeBetween: true }), 'a1'))
      .toEqual({ startedAt: T_USER, completedAt: T_NEXT_USER - 1 });
  });

  it('最后一轮（无后续 user 消息）：上界取当前时刻，与实时收尾口径一致', () => {
    vi.useFakeTimers();
    vi.setSystemTime(T_NEXT_USER + 5_000);
    try {
      expect(turnUsageWindowFromNeighbors(items(), 'a1'))
        .toEqual({ startedAt: T_USER, completedAt: T_NEXT_USER + 5_000 });
    } finally {
      vi.useRealTimers();
    }
  });

  it('状态门槛：streaming / failed / aborted 轮不出窗口', () => {
    expect(turnUsageWindowFromNeighbors(items({ assistantStatus: 'streaming' }), 'a1')).toBeNull();
    expect(turnUsageWindowFromNeighbors(items({ assistantStatus: 'failed' }), 'a1')).toBeNull();
    expect(turnUsageWindowFromNeighbors(items({ assistantStatus: 'aborted' }), 'a1')).toBeNull();
  });

  it('缺上一条 user 时间戳 / 下一轮 user 时刻早于本轮开始 → null，不猜', () => {
    expect(turnUsageWindowFromNeighbors(items({ dropUserTimestamp: true }), 'a1')).toBeNull();
    // 下一轮 user 时刻 ≤ 本轮开始：数据时序倒挂，拒绝出窗口
    const backwards: ChatListItem[] = [
      { type: 'message', data: { id: 'u1', role: 'user', text: 'hi', timestamp: T_USER } },
      { type: 'message', data: { id: 'a1', role: 'assistant', blocks: [], timestamp: T_ASSISTANT, turnProjection: items().at(-1)!.type === 'message' ? (items().at(-1) as { data: { turnProjection: AssistantTurnProjection } }).data.turnProjection : undefined } },
      { type: 'message', data: { id: 'u2', role: 'user', text: 'next', timestamp: T_USER } },
    ];
    expect(turnUsageWindowFromNeighbors(backwards, 'a1')).toBeNull();
  });

  it('找不到目标消息 → null', () => {
    expect(turnUsageWindowFromNeighbors(items(), 'missing')).toBeNull();
  });
});
