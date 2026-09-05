/**
 * git-graph — 提交历史的泳道布局（纯函数）
 *
 * 输入按新→旧排序的 {hash, parents}，输出每行的绘制要素：
 *   nodeLane    本行提交节点所在泳道（x 位置）
 *   activeLanes 本行下方仍延续的泳道索引（画竖线）
 *   mergeLanes  从节点引出曲线连到的泳道（合并/会合）
 *   laneCount   截至本行的泳道总数（图形宽度）
 *
 * 泳道用「槽池」管理：结束的泳道置空不前移（x 位置稳定，避免视觉跳动），
 * 后续新泳道优先复用空槽。线性历史=单泳道竖线+节点；分叉=新泳道+曲线；
 * 合并=节点曲线连入第一父所在泳道，本泳道释放。
 */

export interface GraphInputCommit {
  hash: string;
  parents: string[];
}

export interface GraphLaneRow {
  nodeLane: number;
  activeLanes: number[];
  mergeLanes: number[];
  laneCount: number;
}

export function computeGraphRows(commits: GraphInputCommit[]): GraphLaneRow[] {
  // 槽池：lanes[i] = 该泳道正在跟踪的父哈希（null = 空槽）
  const lanes: (string | null)[] = [];
  const rows: GraphLaneRow[] = [];

  const takeSlot = (hash: string): number => {
    const free = lanes.indexOf(null);
    if (free === -1) {
      lanes.push(hash);
      return lanes.length - 1;
    }
    lanes[free] = hash;
    return free;
  };

  for (const commit of commits) {
    let nodeLane = lanes.indexOf(commit.hash);
    if (nodeLane === -1) nodeLane = takeSlot(commit.hash);

    const [first, ...rest] = commit.parents;
    const mergeLanes: number[] = [];

    if (!first) {
      // 根提交：泳道终止（限流截断处 parents 也可能为空，同样处理）
      lanes[nodeLane] = null;
    } else {
      const firstLane = lanes.indexOf(first);
      if (firstLane === -1) {
        lanes[nodeLane] = first;
      } else if (firstLane !== nodeLane) {
        // 会合点：第一父已被其他泳道跟踪 → 本泳道释放，节点曲线连过去
        lanes[nodeLane] = null;
        mergeLanes.push(firstLane);
      }
      for (const parent of rest) {
        let lane = lanes.indexOf(parent);
        if (lane === -1) lane = takeSlot(parent);
        if (lane !== nodeLane) mergeLanes.push(lane);
      }
    }

    const activeLanes: number[] = [];
    lanes.forEach((tracked, index) => {
      if (tracked != null) activeLanes.push(index);
    });
    rows.push({ nodeLane, activeLanes, mergeLanes, laneCount: lanes.length });
  }

  return rows;
}

/** 全图需要的泳道总数（宽度上限；实际渲染再封顶防超宽仓库） */
export function graphLaneCount(rows: GraphLaneRow[]): number {
  return rows.reduce((max, row) => Math.max(max, row.laneCount), 0);
}
