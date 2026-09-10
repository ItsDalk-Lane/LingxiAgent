/**
 * 历史读取 —— 页边界与页窗口解析（阶段 B / B01 页边界 + B05 窗口解析）
 *
 * resolveHistoryPageBounds 从 /sessions/messages 路由原样移入；语义逐字保持：
 * before 是 display 序号边界（返回区间 [max(0,end-limit), end)），0 是合法边界
 * （= 已翻到会话开头，返回空页并终结分页），负数/NaN 视为未指定返回最新页，
 * all=1 强制全量。sessions.ts 对本函数做 re-export，find 路由与测试的 import 不变。
 *
 * resolveHistoryPage（计划 §1.5）在目录事实上解析页窗口：
 *  - total 取目录 sessionFacts.displayTotal（构建时算好），不重扫；
 *  - 窗口记录取自升序 displayableSourceIndexes——displayIndex 恰为该数组中的位置，
 *    因此 [startIdx,endIdx) 即数组下标区间（O(K)，与二分等价）；
 *  - 块锚点按 afterIndex 升序数组做二分定位（O(log N + K)），禁止全目录 .filter；
 *  - dependencyLocations 汇总页外依赖（todos 指针、窗口 interlude 锚点记录、末页
 *    media 结果、窗口 user 命中的 origin/presentation/review 注释记录），去重后按
 *    byteOffset 排序；跨页 toolResult 结局与页内 media 块的 taskId 依赖在窗口记录
 *    读取后经 locateToolResultRecords / locateMediaResultRecords 以 O(命中数) 补定位
 *    （§5 #3/#5）；
 *  - headState 页首种子直接取首条窗口记录的 before 状态（不从根重放，易碎点 1/2/7）。
 */
import { isDisplayableHistoryMessage } from "./projection-context.ts";
import type { HistoryDirectory, HistoryRecordFact, InvalidationReason } from "./types.ts";

export type { InvalidationReason };

/** 窗口读取请求：记录定位 + 目录身份（读取后校验，X05）。 */
export interface HistoryRecordLocation {
  sourceIndex: number;
  entryId: string | null;
  /** raw entry type；null = 不做 type 校验（custom 角色可能来自 custom/custom_message 两种 raw type）。 */
  type: string | null;
  byteOffset: number;
  byteLength: number;
  /** 该行超限（读取后需内存套用 projectOversizedSessionEntry，与 scan 同形状）。 */
  oversized?: boolean;
}

export function resolveHistoryPageBounds(sourceMessages, { beforeId, limit, forceAll }) {
  let total = 0;
  for (const message of sourceMessages) {
    if (isDisplayableHistoryMessage(message)) total += 1;
  }
  if (forceAll) return { total, startIdx: 0, endIdx: total, hasMore: false };
  const endIdx = (beforeId != null && Number.isFinite(beforeId) && beforeId >= 0)
    ? Math.min(beforeId, total)
    : total;
  const startIdx = Math.max(0, endIdx - limit);
  return { total, startIdx, endIdx, hasMore: startIdx > 0 };
}

export interface HistoryPageSeed {
  sourceIndex: number;
  displayIdx: number;
  latestTurnInputEntryId: string | null;
  latestTurnInputVisible: boolean;
  assistantOrdinalInTurn: number;
}

export interface ResolvedHistoryPage {
  bounds: { total: number; startIdx: number; endIdx: number; hasMore: boolean };
  /** 窗口内记录的 sourceIndex 升序。 */
  windowRecordIndexes: number[];
  /** afterIndex ∈ 窗口的块锚点记录（toolResult / display!==false custom / interlude 命中）。 */
  blockAnchorIndexes: number[];
  /** 页外依赖记录定位（去重、按 byteOffset 升序）。 */
  dependencyLocations: HistoryRecordLocation[];
  /** 热页迭代闭包（窗口∪块锚点∪页外依赖，升序）——projectHistoryPage 的 iterate。 */
  iterateRecordIndexes: number[];
  headState: HistoryPageSeed;
}

/** 升序数组中第一个 >= value 的下标（value 传 Number.MAX_SAFE_INTEGER 可排除尾部 null 项）。 */
function lowerBound(arr: { length: number; at(index: number): { afterIndex: number | null } }, value: number): number {
  let lo = 0;
  let hi = arr.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    const item = arr.at(mid);
    const key = item == null || item.afterIndex == null ? Number.MAX_SAFE_INTEGER : item.afterIndex;
    if (key < value) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/** 泛化二分：升序序列中第一个 key(item) >= value 的下标（数组/RecordList 均可）。 */
function lowerBoundBy<T>(arr: { length: number; at(index: number): T }, value: number, key: (item: T) => number): number {
  let lo = 0;
  let hi = arr.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (key(arr.at(mid) as T) < value) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

function locationForRecord(directory: HistoryDirectory, sourceIndex: number): HistoryRecordLocation | null {
  const record = directory.records.at(sourceIndex);
  if (!record?.entryId) return null;
  const physicalIndex = directory.file.byEntryId.get(record.entryId);
  if (physicalIndex == null) return null;
  // raw type 校验：user/assistant/toolResult 的原始条目 type 恒为 "message"；
  // custom 角色可能来自 "custom" 或 "custom_message"，不下发 type（窗口读取只强校验 entryId）。
  const type = record.role === "user" || record.role === "assistant" || record.role === "toolResult"
    ? "message"
    : null;
  return {
    sourceIndex,
    entryId: record.entryId,
    type,
    byteOffset: directory.file.byteOffsets.at(physicalIndex),
    byteLength: directory.file.byteLengths.at(physicalIndex),
  };
}

export function resolveHistoryPage(
  directory: HistoryDirectory,
  params: { beforeId: number | null; limit: number; forceAll: boolean },
): ResolvedHistoryPage {
  // 页边界语义与 resolveHistoryPageBounds 逐字同源；total 来自目录会话事实。
  const total = directory.session.displayTotal;
  let startIdx: number;
  let endIdx: number;
  let hasMore: boolean;
  if (params.forceAll) {
    startIdx = 0;
    endIdx = total;
    hasMore = false;
  } else {
    endIdx = (params.beforeId != null && Number.isFinite(params.beforeId) && params.beforeId >= 0)
      ? Math.min(params.beforeId, total)
      : total;
    startIdx = Math.max(0, endIdx - params.limit);
    hasMore = startIdx > 0;
  }

  const windowRecordIndexes = directory.displayableSourceIndexes.slice(startIdx, endIdx);

  // 块锚点二分定位：afterIndex ∈ [startIdx, endIdx)（afterIndex 为 null 的尾部锚点
  // 以 MAX_SAFE_INTEGER 参与比较，天然落在区间外）。
  const anchors = directory.blockAnchorByAfterIndex;
  const anchorLo = lowerBound(anchors, startIdx);
  const anchorHi = lowerBound(anchors, endIdx);
  const blockAnchorIndexes = anchors.slice(anchorLo, anchorHi).map((a) => a.sourceIndex);

  const inPage = new Set<number>([...windowRecordIndexes, ...blockAnchorIndexes]);
  const dependencyLocations: HistoryRecordLocation[] = [];
  const seenDependency = new Set<number>();
  const dependenciesBySourceIndex = new Map<number, HistoryRecordLocation>();
  const addDependency = (sourceIndex: number) => {
    if (inPage.has(sourceIndex) || seenDependency.has(sourceIndex)) return;
    const loc = locationForRecord(directory, sourceIndex);
    if (!loc) return;
    seenDependency.add(sourceIndex);
    dependenciesBySourceIndex.set(sourceIndex, loc);
  };

  // ① todos 指针（§5 #1）：最新合法快照记录通常远离尾部窗口。
  if (directory.assoc.todoSnapshot) addDependency(directory.assoc.todoSnapshot.sourceIndex);
  // ② 窗口命中的 deferred-result interlude 锚点（§5 #7）：锚点 afterIndex 落在窗口内、
  //    而该 deferred-result 记录自身可能在页外——补定位之。
  for (const anchor of directory.assoc.deferredInterludeAnchors) {
    if (anchor.anchorAfterIndex == null) continue;
    if (anchor.anchorAfterIndex >= startIdx && anchor.anchorAfterIndex < endIdx) {
      addDependency(anchor.sourceIndex);
    }
  }
  // ③ media 结果记录（§5 #3）：窗口 media 块按 taskId 引用的结果记录可能在页外；
  //    B 阶段保守纳入全部结果记录（媒体记录稀少，宁可多读不可漏结果），末页
  //    standalone 注入亦由此覆盖。
  for (const media of directory.assoc.mediaResultRecords) addDependency(media.sourceIndex);
  // ④ 窗口 user 命中的 origin/presentation/review 注释记录（§5 #9）：记录可能在页首之前。
  for (const sourceIndex of windowRecordIndexes) {
    const origin = directory.assoc.originBySourceIndex.get(sourceIndex);
    if (origin) addDependency(origin.originRecordSourceIndex);
    const presentation = directory.assoc.presentationBySourceIndex.get(sourceIndex);
    if (presentation) addDependency(presentation.presentationRecordSourceIndex);
    const review = directory.assoc.agentReviewBySourceIndex.get(sourceIndex);
    if (review) addDependency(review.reviewRecordSourceIndex);
  }
  // ⑤ 窗口 span 内的全部记录（易碎点 1/2）：不可见 assistant 推大 ordinal、
  //    display:false custom（consumption/loop/deferred）在窗口内产出 interlude——
  //    指针状态与块闭包都依赖它们。displayIndexBefore 沿记录单调非递减 → 二分取
  //    displayIndexBefore ∈ [startIdx, endIdx]（O(log N)）。
  const records = directory.records;
  const spanLo = lowerBoundBy(records, startIdx, (fact: HistoryRecordFact) => fact.displayIndexBefore);
  const spanHi = lowerBoundBy(records, endIdx + 1, (fact: HistoryRecordFact) => fact.displayIndexBefore);
  // ⑥ 跨页 toolResult 结局（§5 #5）与 consumption interlude 锚点（§5 #7）：以窗口
  //    assistant 为锚——目录记录级 toolCallIds / assoc.consumptionByAssistantEntryId
  //    （构建期纯关联）定位页外记录。
  for (let sourceIndex = spanLo; sourceIndex < spanHi; sourceIndex += 1) {
    const record = records.at(sourceIndex);
    for (const callId of record?.toolCallIds ?? []) {
      const hit = directory.assoc.toolResultByCallId.get(callId);
      if (hit) addDependency(hit.sourceIndex);
    }
    if (record?.role === "assistant" && record.entryId) {
      for (const consumptionIndex of directory.assoc.consumptionByAssistantEntryId.get(record.entryId) ?? []) {
        addDependency(consumptionIndex);
      }
    }
  }
  for (const loc of dependenciesBySourceIndex.values()) dependencyLocations.push(loc);
  dependencyLocations.sort((a, b) => a.byteOffset - b.byteOffset);

  // 热页迭代闭包 = 窗口记录 ∪ 窗口 span 记录 ∪ 块锚点 ∪ 页外依赖（升序）：
  // span 记录维护指针状态（ordinal）并承载窗口内 interlude；页外依赖或者自身产出
  // 块（锚点/interlude），或者为产出块提供事实（结局/媒体/注释/快照）。
  const iterateIndexes = new Set<number>();
  for (let i = spanLo; i < spanHi; i += 1) iterateIndexes.add(i);
  for (const sourceIndex of windowRecordIndexes) iterateIndexes.add(sourceIndex);
  for (const sourceIndex of blockAnchorIndexes) iterateIndexes.add(sourceIndex);
  for (const sourceIndex of dependenciesBySourceIndex.keys()) iterateIndexes.add(sourceIndex);
  const iterateRecordIndexes = [...iterateIndexes].sort((a, b) => a - b);

  // 页首种子：直接取首条窗口记录的 before 状态（目录记录级事实），不从根重放。
  const firstIndex = windowRecordIndexes[0];
  let headState: HistoryPageSeed;
  if (firstIndex != null) {
    const fact = directory.records.at(firstIndex)!;
    headState = {
      sourceIndex: firstIndex,
      displayIdx: fact.displayIndexBefore,
      latestTurnInputEntryId: fact.turnInputEntryIdBefore,
      latestTurnInputVisible: fact.turnInputVisibleBefore,
      assistantOrdinalInTurn: fact.assistantOrdinalBefore,
    };
  } else if (directory.records.length > 0) {
    const last = directory.records.at(directory.records.length - 1)!;
    headState = {
      sourceIndex: directory.records.length,
      displayIdx: total,
      latestTurnInputEntryId: last.turnInputEntryIdBefore,
      latestTurnInputVisible: last.turnInputVisibleBefore,
      assistantOrdinalInTurn: last.assistantOrdinalBefore,
    };
  } else {
    headState = {
      sourceIndex: 0,
      displayIdx: 0,
      latestTurnInputEntryId: null,
      latestTurnInputVisible: true,
      assistantOrdinalInTurn: 0,
    };
  }

  return {
    bounds: { total, startIdx, endIdx, hasMore },
    windowRecordIndexes,
    blockAnchorIndexes,
    dependencyLocations,
    iterateRecordIndexes,
    headState,
  };
}

/**
 * §5 #5：窗口记录读取后，按窗口 assistant 实际引用的 toolUse id 补定位跨页
 * toolResult 记录（O(命中数)；结局在 hydrate 侧用现有 projectToolResultOutcome 现算，
 * 目录无对应 toolResult 时与旧全量 map miss 同样落 {status:"unknown",success:false}）。
 */
export function locateToolResultRecords(
  directory: HistoryDirectory,
  toolCallIds: Iterable<string>,
): HistoryRecordLocation[] {
  const out: HistoryRecordLocation[] = [];
  for (const callId of toolCallIds) {
    const hit = directory.assoc.toolResultByCallId.get(callId);
    if (!hit) continue; // 目录无对应 toolResult：与旧全量 map miss 同语义
    const loc = locationForRecord(directory, hit.sourceIndex);
    if (loc) out.push(loc);
  }
  out.sort((a, b) => a.byteOffset - b.byteOffset);
  return out;
}

/** §5 #3：按窗口 media 块引用的 taskId 补定位媒体结果记录。 */
export function locateMediaResultRecords(
  directory: HistoryDirectory,
  taskIds: Iterable<string>,
): HistoryRecordLocation[] {
  const byTask = new Map(directory.assoc.mediaResultRecords.map((m) => [m.taskId, m.sourceIndex]));
  const out: HistoryRecordLocation[] = [];
  for (const taskId of taskIds) {
    const sourceIndex = byTask.get(taskId);
    if (sourceIndex == null) continue;
    const loc = locationForRecord(directory, sourceIndex);
    if (loc) out.push(loc);
  }
  out.sort((a, b) => a.byteOffset - b.byteOffset);
  return out;
}
