/**
 * trajectory-virtual-rows.ts — 轨迹记录到可测量虚拟台账行的纯投影。
 *
 * 移植自 dsh-desktop packages/client/ui-trajectory/src/client/trajectory-virtual-rows.ts
 * （MIT），原样移植。
 */

import type { TrajectoryCellProps } from './trajectory-record.ts';
import { trajectoryRecordId } from './trajectory-record.ts';

const CONTENT_ROW_HEIGHT = 30;
const COLLAPSED_SUMMARY_HEIGHT = 20;
const TERMINAL_BOUNDARY_HEIGHT = 9;

/** 虚拟行投影所需的最小记录形状。 */
export interface VirtualizableTrajectoryRecord {
  cell: TrajectoryCellProps;
  collapsedSummaryKind?: 'turn' | 'assistant';
}

/** 一个可测量虚拟行内保留的一条逻辑记录。 */
export interface TrajectoryVirtualRowEntry<T extends VirtualizableTrajectoryRecord> {
  logicalIndex: number;
  record: T;
}

/** 一个 virtualizer 条目；可携带零高度的请求边界记录。 */
export interface TrajectoryVirtualRow<T extends VirtualizableTrajectoryRecord> {
  entries: readonly TrajectoryVirtualRowEntry<T>[];
  height: number;
  key: string;
}

/**
 * 派生 React、virtualizer 与浏览器滚动契约共享的 DOM 安全行身份。
 */
export function trajectoryVirtualRecordKey(
  record: VirtualizableTrajectoryRecord,
): string {
  const identity = encodeURIComponent(trajectoryRecordId(record.cell));
  return record.collapsedSummaryKind === undefined
    ? identity
    : `${identity}\u0000summary\u0000${record.collapsedSummaryKind}`;
}

/**
 * 把仅含分隔的记录并入下一条内容行，virtualizer 永远不持有零高度条目。
 * 终端分隔保留 CSS 自有的下方标记间隙作为独立条目。
 */
export function groupTrajectoryVirtualRows<T extends VirtualizableTrajectoryRecord>(
  records: readonly T[],
): readonly TrajectoryVirtualRow<T>[] {
  const rows: TrajectoryVirtualRow<T>[] = [];
  let pending: TrajectoryVirtualRowEntry<T>[] = [];

  for (const [logicalIndex, record] of records.entries()) {
    const entry = { logicalIndex, record };
    if (record.cell.requestOnly === true) {
      pending.push(entry);
      continue;
    }
    const entries = [...pending, entry];
    pending = [];
    rows.push({
      entries,
      height: record.collapsedSummaryKind === undefined
        ? CONTENT_ROW_HEIGHT
        : COLLAPSED_SUMMARY_HEIGHT,
      key: trajectoryVirtualRecordKey(record),
    });
  }

  if (pending.length > 0) {
    rows.push({
      entries: pending,
      height: TERMINAL_BOUNDARY_HEIGHT,
      key: pending.map(candidate => trajectoryVirtualRecordKey(candidate.record)).join('|'),
    });
  }

  return rows;
}
