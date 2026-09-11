/**
 * E04：会话概览计数（TASKBOOK E04.2/E04.3）。
 *
 * 计数在 B/C 同一目录/增量构建器中维护：冷构建/分支重建 O(N) 重算；
 * 增量追加按 fact 增量更新（run 跨桶移动、taskId 去重、类别首分类胜出），
 * 热路径概览只读计数器（O(1)），不遍历全部 Run/taskId。
 *
 * 类别映射来自任务自身元数据 type（权威解析器 parseHistoryDeferredResult 的
 * 产出），不靠正文关键词或名称猜：
 *  - image-generation / video-generation → media（isMediaGenerationDeferredResult）
 *  - "subagent" → subagent；"workflow" → workflow；无可信类别 → other
 */
import type {
  HistoryDirectoryOverview,
  HistoryRecordFact,
  HistoryTaskCategory,
} from "./types.ts";

export function categorizeTaskType(type: string): HistoryTaskCategory {
  if (type === "image-generation" || type === "video-generation") return "media";
  if (type === "subagent") return "subagent";
  if (type === "workflow") return "workflow";
  return "other";
}

function bucketOf(count: number): keyof HistoryDirectoryOverview["runBuckets"] | null {
  if (count <= 0) return null;
  if (count <= 50) return "oneTo50";
  if (count <= 200) return "from51To200";
  return "over200";
}

/** 单条分支记录的概览增量（append/重算共用同一规则，I04 同形状）。 */
export function applyOverviewFact(ov: HistoryDirectoryOverview, fact: HistoryRecordFact): void {
  if (fact.role === "assistant" && fact.displayIndex != null && fact.runOrdinal != null) {
    const prev = ov.runSizeByOrdinal.get(fact.runOrdinal) ?? 0;
    const next = prev + 1;
    ov.runSizeByOrdinal.set(fact.runOrdinal, next);
    if (prev === 0) ov.runsWithAssistant += 1; // 仅有用户输入尾段不含 assistant，不计入
    const from = bucketOf(prev);
    const to = bucketOf(next);
    if (from !== to) {
      if (from) ov.runBuckets[from] -= 1;
      ov.runBuckets[to] += 1;
    }
  }
  const ref = fact.deferredTaskRef;
  if (ref?.taskId && !ov.taskCategories.has(ref.taskId)) {
    const category = categorizeTaskType(ref.type);
    ov.taskCategories.set(ref.taskId, category);
    ov.taskCategoryCounts[category] += 1;
  }
}

export function emptyOverview(): HistoryDirectoryOverview {
  return {
    runSizeByOrdinal: new Map(),
    runBuckets: { oneTo50: 0, from51To200: 0, over200: 0 },
    runsWithAssistant: 0,
    taskCategories: new Map(),
    taskCategoryCounts: { subagent: 0, workflow: 0, media: 0, other: 0 },
  };
}

/** 冷构建/分支重建：分支序逐条重算（O(N)，仅这些路径允许）。 */
export function computeOverviewFromRecords(records: Iterable<HistoryRecordFact>): HistoryDirectoryOverview {
  const ov = emptyOverview();
  for (const fact of records) applyOverviewFact(ov, fact);
  return ov;
}
