/**
 * trace-layout.ts — 轨迹轮次/分组模型与折叠助手。
 *
 * 输出契约（TrajectoryTurnModel/TrajectoryGroupModel）移植自 dsh-desktop
 * packages/client/ui-trajectory/src/client/layout.ts（MIT）；dsh 的
 * deriveTrajectoryLayout 消费的是其 runtime 事件流节点，灵犀的数据装配
 * （trace-conversation-model.ts）直接产出同形状模型，故此处只保留类型与
 * 可共享的折叠语义（分组描述、请求编号类型、usage 折叠）。
 */

import type { TrajectoryCellProps } from './trajectory-record.ts';
import { formatElapsedSeconds } from './trajectory-record.ts';

/** 轮次内的一个 Message 或 Step 分组。 */
export interface TrajectoryGroupModel {
  title: string;
  description?: string;
  cells: readonly TrajectoryCellProps[];
}

/** 一个 sticky 轮次，或轮次间的独立区段（Between turns）。 */
export interface TrajectoryTurnModel {
  turn: number | null;
  groups: readonly TrajectoryGroupModel[];
}

/** 不相交的 provider token 桶（单请求或会话前缀累计）。 */
export interface TrajectoryUsage {
  input?: number;
  cacheRead?: number;
  cacheWrite?: number;
  output?: number;
  reasoning?: number;
}

/** 检查器「请求」面板的请求身份（与 session 全局编号配对）。 */
export interface TrajectoryRequestNumberBase {
  seq?: number;
  group: string;
  number: number;
  status?: 'complete' | 'running' | 'error';
  startedAt?: number;
  completedAt?: number | null;
  error?: string;
  provider?: string;
  model?: string;
  usage?: TrajectoryUsage;
  cumulativeUsage?: TrajectoryUsage;
}

export type TrajectoryRequestNumber = TrajectoryRequestNumberBase & (
  | {
    purpose?: 'assistant';
    turn: number;
    step: number;
  }
  | {
    /** 轮次间隙的侧线调用（压缩/标题/知识滚动等）。 */
    purpose: 'side';
    turn: number | null;
    step: 0;
  }
);

/** 折叠时携带绝对时间与工具名的 cell（分组墙钟描述用）。 */
export interface LaidCellLike {
  cell: TrajectoryCellProps;
  absTime: number | null;
  toolName?: string;
}

/**
 * 分组描述：墙钟跨度 + 工具直方图（如 `1.5 s bash×6`）。
 * 移植自 dsh layout.ts groupDescription。
 */
export function describeTrajectoryGroup(laid: readonly LaidCellLike[]): string | undefined {
  const parts: string[] = [];
  const times: number[] = [];
  for (const l of laid) {
    if (l.absTime === null || !Number.isFinite(l.absTime)) continue;
    times.push(l.absTime);
    if (l.cell.kind === 'tool' && l.cell.timeSeconds !== null && Number.isFinite(l.cell.timeSeconds)) {
      times.push(l.absTime + l.cell.timeSeconds * 1000);
    }
  }
  if (times.length >= 2) {
    const span = formatElapsedSeconds((Math.max(...times) - Math.min(...times)) / 1000);
    if (span !== '—') parts.push(span);
  } else if (times.length === 1) {
    const own = laid.find(l => l.absTime === times[0])?.cell.timeSeconds;
    const span = own !== null && own !== undefined ? formatElapsedSeconds(own) : undefined;
    if (span !== undefined && span !== '—') parts.push(span);
  }
  const tools = new Map<string, number>();
  for (const l of laid) {
    if (l.toolName === undefined || l.cell.kind !== 'tool') continue;
    tools.set(l.toolName, (tools.get(l.toolName) ?? 0) + 1);
  }
  for (const [name, count] of tools) {
    parts.push(count > 1 ? `${name}×${count}` : name);
  }
  return parts.length === 0 ? undefined : parts.join(' ');
}

/** 两段 epoch 毫秒的自身耗时秒；任一不可用为 null。 */
export function durationSecondsBetween(later: number | null, earlier: number | null): number | null {
  if (earlier === null || later === null || !Number.isFinite(later) || !Number.isFinite(earlier)) {
    return null;
  }
  return Math.max(0, (later - earlier) / 1000);
}

/** 可用作绝对时间的 epoch 毫秒，否则 null。 */
export function finiteEpochMs(time: number | null | undefined): number | null {
  return typeof time === 'number' && Number.isFinite(time) ? time : null;
}
