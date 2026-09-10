/**
 * 历史读取 —— 可信增量构建与分支视图重建（阶段 C / C03+C04）
 *
 * C01 判定给出 append_candidate（身份/locator 不变、size 增长、变更世代未变）时，
 * 从旧目录的记录边界续读增量并合成新版本目录：
 *  - 续读起点 = pendingTailOffset ?? lastUndelimitedRow?.offset ?? indexedThroughOffset
 *    （不从旧 observedFileSize 盲续；lastUndelimitedRow 重读核对 id/字节长度）；
 *  - 新条目 id 唯一性 / parentId 存在性 / 自环与环检查；非法结构 → 放弃增量，
 *    index.ts 据此失效并走全量严格层（X01）；
 *  - 分支归属复用现有 head 规则（append_recovery / persisted_head / legacy_tail，
 *    含 continuesDiscardedObservedTail）；弃分支追加只更新物理索引、不进当前视图；
 *  - 版本隔离（I06）：base+tail 分段记录与 Map/数组 overlay，旧版本持有者看到
 *    完整旧快照；Run 边界在 runOrdinal→{start,end} 小表上 copy-on-write 单表项
 *    （超长开放 Run 逐条追加 O(1)，不 O(Run) 改写）；
 *  - C04 受影响关系只处理新增条目：toolResult 结局、consumption/interlude、
 *    collab/correlation、todo 快照指针、文件引用集合、媒体记录、Run 边界延续；
 *  - 每次增量 O(新增字节 + 新增条目 + 受影响关系)，不重解析无关旧正文；
 *  - 混合形态（同一批增量里既有当前分支延续又有弃分支条目）指针机无法无歧义
 *    延续 → 保守放弃增量走全量重建。
 *
 * branch_view_stale（文件不变、head 选择改变且目标在已构建 lineage 内）→
 * rebuildBranchView：O(N) 元数据重建（TASKBOOK 允许），零文件读取、物理索引不变。
 */
import { historyMessageFromEntry, annotateOriginMessages } from "../../core/message-utils.ts";
import { projectSessionMessageForDisplay } from "../../core/session-reminders.ts";
import { DESKTOP_INPUT_CORRELATION_TYPE } from "../../core/desktop-input-correlation.ts";
import { TODO_TOOL_NAMES, TODO_STATE_CUSTOM_TYPE } from "../../lib/tools/todo-constants.ts";
import { extractLatestTodoSnapshot } from "../../lib/tools/todo-compat.ts";
import { collectSessionFileReferenceIdentities } from "../../lib/session-files/session-file-registry.ts";
import { scanHistoryFile } from "./scan.ts";
import { applyOverviewFact, computeOverviewFromRecords } from "./overview.ts";
import {
  MapOverlay,
  SegmentedList,
} from "./types.ts";
import type {
  HistoryDirectory,
  HistoryDirectoryOverview,
  HistoryReadContext,
  HistoryReadHook,
  HistoryRecordFact,
  HistoryTaskCategory,
  InvalidationReason,
} from "./types.ts";
import { measureDirectoryBytes } from "./directory.ts";
import { createProjectionFactScanner } from "./projection-context.ts";

export interface IncrementalMetrics {
  appendedBytes: number;
  rereadBytes: number;
  validatedBytes: number;
  parsedRecords: number;
  appendedEntries: number;
  appendedRecords: number;
  affectedRelations: number;
  durationMs: number;
}

export type IncrementalOutcome =
  | { kind: "ok"; directory: HistoryDirectory; metrics: IncrementalMetrics }
  | { kind: "fail"; reason: InvalidationReason; detail?: string };

function isDescendantOf(
  candidateLeafId: string | null,
  ancestorLeafId: string | null,
  parentOf: (id: string) => string | null,
): boolean {
  if (candidateLeafId == null) return ancestorLeafId == null;
  if (ancestorLeafId == null) return true;
  let current = candidateLeafId;
  let guard = 0;
  while (current != null && guard < 1_000_000) {
    if (current === ancestorLeafId) return true;
    current = parentOf(current) ?? null;
    guard += 1;
  }
  return false;
}

/** C03 续读起点：待完成尾记录 → 合法无尾换行末条 → 已索引边界（不盲续）。 */
export function computeResumeOffset(old: HistoryDirectory): number {
  return old.file.pendingTailOffset
    ?? old.file.lastUndelimitedRow?.offset
    ?? old.file.indexedThroughOffset;
}

/** C03/C05：数组追加（base 段与旧版本共享只读，追加段有界）。 */
function overlayAppend<T>(prev: SegmentedList<T>, tail: T[]): SegmentedList<T> {
  const segments = Array.isArray(prev)
    ? [prev as unknown as T[]]
    : prev instanceof SegmentedList
      ? [...prev.segments]
      : (() => { throw new TypeError(`overlayAppend: bad prev (${(prev as any)?.constructor?.name ?? typeof prev}, segments=${typeof (prev as any)?.segments})`); })();
  return tail.length ? new SegmentedList<T>([...segments, tail]) : prev;
}

export async function buildHistoryDirectoryIncremental(opts: {
  old: HistoryDirectory;
  sessionPath: string;
  ctx: HistoryReadContext;
  readFile?: HistoryReadHook;
}): Promise<IncrementalOutcome> {
  const started = process.hrtime.bigint();
  const { old: oldDir, ctx, sessionPath, readFile } = opts;
  const newSize = ctx.fileIdentity.size;
  const resumeOffset = computeResumeOffset(oldDir);
  const appendedBytes = Math.max(0, newSize - resumeOffset);
  const rereadBytes = Math.max(0, resumeOffset - oldDir.file.indexedThroughOffset);

  // ── 增量扫描（从记录边界续读）──
  const scan = await scanHistoryFile(sessionPath, {
    capturedLength: newSize,
    startOffset: resumeOffset,
    readFile,
  });
  if (scan.error) return { kind: "fail", reason: "directory_invalid", detail: scan.error.code };
  if (scan.bounds.observedFileSize < newSize) return { kind: "fail", reason: "short_read" };

  const deltaEntries = scan.entries;

  // lastUndelimitedRow 重读核对：首条 delta 记录必须是旧末条本体（id/字节长度一致）。
  // 确认后该条目跳过一切“新增”处理（它已在旧物理索引/旧记录中，I09/I10）。
  const tailRereadCount = oldDir.file.lastUndelimitedRow != null ? 1 : 0;
  if (tailRereadCount === 1 && deltaEntries.length > 0) {
    const first = deltaEntries[0];
    const lastFact = oldDir.records.at(oldDir.records.length - 1);
    const firstId = typeof first?.id === "string" ? first.id : null;
    const firstLen = scan.physical[0]?.byteLength ?? -1;
    const oldId = lastFact?.entryId ?? null;
    const oldLen = oldDir.file.lastUndelimitedRow.length;
    if (firstId == null || firstId !== oldId || firstLen !== oldLen) {
      return { kind: "fail", reason: "snapshot_changed", detail: "tail_record_changed" };
    }
  }
  const newEntries = deltaEntries.slice(tailRereadCount);
  const newPhysical = scan.physical.slice(tailRereadCount);

  // ── 结构校验（X01 增量版）──
  // 父指针直接取自追加条目的原始字段（追加自带 parentId）；旧文件成员判定用
  // old.file.byEntryId（保留的物理索引）与 old.branch.lineageEntryIds（root→leaf 序）。
  const deltaIds = new Set<string>();
  const oldLineage = oldDir.branch.lineageEntryIds;
  const oldLineageHas = (id: string | null): boolean => {
    if (id == null) return false;
    for (let i = 0; i < oldLineage.length; i += 1) if (oldLineage.at(i) === id) return true;
    return false;
  };
  const newParent = new Map<string, string | null>();
  for (const entry of newEntries) {
    const id = entry?.id;
    if (typeof id !== "string" || !id) continue;
    if (deltaIds.has(id) || oldDir.file.byEntryId.has(id)) {
      return { kind: "fail", reason: "directory_invalid", detail: "session_branch_duplicate_id" };
    }
    deltaIds.add(id);
  }
  function byIdHas(id: string): boolean {
    return deltaIds.has(id) || oldDir.file.byEntryId.has(id);
  }
  for (const entry of newEntries) {
    const id = entry?.id;
    if (typeof id !== "string" || !id) continue;
    const parentId = typeof entry?.parentId === "string" && entry.parentId ? entry.parentId : null;
    if (parentId === id) {
      return { kind: "fail", reason: "directory_invalid", detail: "session_branch_cycle" };
    }
    if (parentId != null && !byIdHas(parentId)) {
      return { kind: "fail", reason: "directory_invalid", detail: "session_branch_dangling_parent" };
    }
    newParent.set(id, parentId);
  }
  for (const entry of newEntries) {
    const id = entry?.id;
    if (typeof id !== "string" || !id) continue;
    let current = newParent.get(id) ?? null;
    let guard = 0;
    while (current != null && deltaIds.has(current) && guard <= deltaIds.size) {
      current = newParent.get(current) ?? null;
      guard += 1;
    }
    if (current != null && deltaIds.has(current)) {
      return { kind: "fail", reason: "directory_invalid", detail: "session_branch_cycle" };
    }
  }

  // ── head 规则（与 projectCurrentSessionBranchEntries 同一判定）──
  const lineageIndexOf = (id: string | null): number => {
    if (id == null) return -1;
    for (let i = 0; i < oldLineage.length; i += 1) if (oldLineage.at(i) === id) return i;
    return -1;
  };
  // 追加条目的父链经 newParent；一旦回溯落入旧 lineage，用索引比较判定祖先关系
  const parentOf = (id: string): string | null => {
    if (deltaIds.has(id)) return newParent.get(id) ?? null;
    const idx = lineageIndexOf(id);
    return idx > 0 ? oldLineage.at(idx - 1)! : null;
  };
  const physicalTailLeafId = (() => {
    for (let i = newEntries.length - 1; i >= 0; i -= 1) {
      const id = newEntries[i]?.id;
      if (typeof id === "string" && id) return id;
    }
    return oldDir.branch.physicalTailLeafId;
  })();
  let selectedLeafId: string | null;
  let headResolution: "legacy_tail" | "persisted_head" | "append_recovery";
  if (!ctx.branchHeadRowExists) {
    selectedLeafId = physicalTailLeafId;
    headResolution = "legacy_tail";
  } else {
    const persistedLeafId = ctx.branchHeadRow?.leafId ?? null;
    const observedTailLeafId = ctx.branchHeadRow?.observedTailLeafId ?? null;
    if (persistedLeafId != null && !byIdHas(persistedLeafId)) {
      return { kind: "fail", reason: "directory_invalid", detail: "session_branch_head_missing" };
    }
    const physicalTailChanged = physicalTailLeafId !== observedTailLeafId;
    const continuesDiscardedObservedTail = observedTailLeafId != null
      && persistedLeafId !== observedTailLeafId
      && isDescendantOf(physicalTailLeafId, observedTailLeafId, parentOf);
    if (
      physicalTailChanged
      && isDescendantOf(physicalTailLeafId, persistedLeafId, parentOf)
      && !continuesDiscardedObservedTail
    ) {
      selectedLeafId = physicalTailLeafId;
      headResolution = "append_recovery";
    } else {
      selectedLeafId = persistedLeafId;
      headResolution = "persisted_head";
    }
  }

  // 追加链：selected → 回溯到旧 lineage 成员为止（进入当前视图的新条目，root→leaf）
  const appendedChain: string[] = [];
  {
    let current = selectedLeafId;
    let guard = 0;
    while (current != null && !oldLineageHas(current) && guard < 1_000_000) {
      appendedChain.push(current);
      current = parentOf(current) ?? null;
      guard += 1;
    }
    if (current != null && !oldLineageHas(current) && current !== oldDir.branch.selectedLeafId) {
      // selected 落在旧 lineage 的非叶节点（rewind/切换）→ 全量重建（保守，不混页）
      return { kind: "fail", reason: "snapshot_changed", detail: "branch_switch" };
    }
  }
  const inBranchIds = new Set(appendedChain);

  // 投影追加条目（与 projectBranchHistory 同链）
  const appendedProjected: Array<{ id: string; message: any } | null> = newEntries.map((entry) => {
    const message = historyMessageFromEntry(entry);
    if (!message) return null;
    const id = typeof entry?.id === "string" && entry.id ? entry.id : "";
    return { id, message: projectSessionMessageForDisplay(message) };
  });
  const projectedInBranch = appendedProjected.filter((p): p is { id: string; message: any } => p != null && inBranchIds.has(p.id));
  const allSideOnly = projectedInBranch.length === 0;
  if (!allSideOnly && projectedInBranch.length !== appendedProjected.filter((p) => p != null).length) {
    // 混合形态：指针机无法无歧义延续（保守放弃，走全量重建）
    return { kind: "fail", reason: "snapshot_changed", detail: "mixed_branch_append" };
  }

  // scanner：seed = 旧目录尾部终态，扫描追加投影消息（指针机同一实现，I04）
  const appendedMessages = appendedProjected
    .filter((p): p is { id: string; message: any } => p != null)
    .map((p) => p.message);
  const scanner = createProjectionFactScanner({
    sourceMessages: appendedMessages,
    annotatedOriginMessages: annotateOriginMessages(appendedMessages),
    seed: {
      displayCounter: oldDir.session.displayTotal,
      latestTurnInputEntryId: oldDir.session.tailPointerEntryId ?? null,
      latestTurnInputVisible: oldDir.session.tailPointerVisible ?? true,
      assistantOrdinalInTurn: oldDir.session.tailAssistantOrdinal ?? 0,
      runOrdinal: oldDir.session.tailRunOrdinal ?? 0,
    },
  });
  for (let i = 0; i < appendedMessages.length; i += 1) scanner.visit(appendedMessages[i], i);
  const fresh = scanner.finalize();

  // local（appendedMessages 下标）→ 全局 record sourceIndex（仅 in-branch 投影）
  const runBoundsByOrdinal = new MapOverlay(oldDir.assoc.runBoundsByOrdinal);
  const localToGlobal = new Map<number, number>();
  {
    let next = oldDir.records.length;
    for (let i = 0; i < appendedProjected.length; i += 1) {
      const projected = appendedProjected[i];
      if (projected == null || !inBranchIds.has(projected.id)) continue;
      localToGlobal.set(i, next);
      next += 1;
    }
  }

  // ── C03/C04：base+tail overlay 组装（I06 不改写旧版本）──
  const appendedRecordFacts: HistoryRecordFact[] = [];
  const newAnchors: Array<{ afterIndex: number; sourceIndex: number }> = [];
  const newDeferredAnchors: Array<{ anchorAfterIndex: number | null; sourceIndex: number; deliveryId: string | null }> = [];
  const newMedia: Array<{ sourceIndex: number; taskId: string; success: boolean }> = [];
  const newDisplayables: number[] = [];
  let affectedRelations = 0;
  let displayAdded = 0;

  const physicalByEntryId = new MapOverlay<string, number>(oldDir.file.byEntryId);
  // E04 概览计数：copy-on-write overlay（delta 有界），仅分支内追加记录参与更新。
  const overview: HistoryDirectoryOverview = {
    runSizeByOrdinal: new MapOverlay(oldDir.overview.runSizeByOrdinal),
    runBuckets: { ...oldDir.overview.runBuckets },
    runsWithAssistant: oldDir.overview.runsWithAssistant,
    taskCategories: new MapOverlay(oldDir.overview.taskCategories),
    taskCategoryCounts: { ...oldDir.overview.taskCategoryCounts },
  };
  // 字节位置按物理下标平铺：base 段与旧版本共享（SegmentedList），追加段 = 新物理
  // 条目（重读末条不占新槽：它已在旧物理索引内，C03 off-by-one 修复）。
  const tailOffsets: number[] = [];
  const tailLengths: number[] = [];
  for (let i = 0; i < newEntries.length; i += 1) {
    const entry = newEntries[i];
    const id = typeof entry?.id === "string" && entry.id ? entry.id : null;
    const physical = newPhysical[i];
    if (id) physicalByEntryId.set(id, oldDir.file.physicalCount + i);
    tailOffsets.push(physical.byteOffset);
    tailLengths.push(physical.byteLength);
  }

  if (!allSideOnly) {
    for (let local = 0; local < appendedMessages.length; local += 1) {
      const global = localToGlobal.get(local);
      if (global == null) continue;
      const fact = fresh.recordFacts[local];
      if (!fact) continue;
      fact.sourceIndex = global;
      if (fact.displayIndex != null) {
        newDisplayables.push(global);
        displayAdded += 1;
      }
      // Run 边界小表 copy-on-write：延续开放 Run 恢复旧 start、写入新 end（O(1)）。
      // 同步修正 fact 字段（I04：记录级事实与 Run 小表一致）。
      if (fact.runOrdinal != null) {
        const prev = oldDir.assoc.runBoundsByOrdinal.get(fact.runOrdinal);
        const start = prev?.start ?? fact.turnStartIndex ?? 0;
        const end = fact.turnEndIndex ?? fact.turnStartIndex ?? start;
        fact.turnStartIndex = start;
        fact.turnEndIndex = end;
        runBoundsByOrdinal.set(fact.runOrdinal, { start, end });
        affectedRelations += 1;
      }
      if (fact.role === "toolResult" || (fact.role === "custom" && appendedMessages[local].display !== false)) {
        const afterIndex = fact.displayIndexBefore - 1;
        if (afterIndex >= 0) newAnchors.push({ afterIndex, sourceIndex: global });
      }
      appendedRecordFacts.push(fact);
      applyOverviewFact(overview, fact);
    }
    for (const [local, anchor] of fresh.deferredInterludeAnchors) {
      const global = localToGlobal.get(local);
      if (global == null) continue;
      newDeferredAnchors.push({ anchorAfterIndex: anchor.afterIndex, sourceIndex: global, deliveryId: anchor.deliveryId });
      affectedRelations += 1;
    }
    for (const media of fresh.mediaResultRecords) {
      const global = localToGlobal.get(media.sourceIndex);
      if (global == null) continue;
      newMedia.push({ sourceIndex: global, taskId: media.taskId, success: media.success });
      affectedRelations += 1;
    }
  }

  // ── C04 关联 overlay（仅新增条目；O(受影响关系)）──
  const toolResultByCallId = new MapOverlay(oldDir.assoc.toolResultByCallId);
  const turnInputByAssistantEntryId = new MapOverlay(oldDir.assoc.turnInputByAssistantEntryId);
  const consumptionByAssistantEntryId = new MapOverlay(oldDir.assoc.consumptionByAssistantEntryId);
  const collabDecisionBySuggestionId = new MapOverlay(oldDir.assoc.collabDecisionBySuggestionId);
  const modelCallRefBySourceIndex = new MapOverlay(oldDir.assoc.modelCallRefBySourceIndex);
  const originBySourceIndex = new MapOverlay(oldDir.assoc.originBySourceIndex);
  const presentationBySourceIndex = new MapOverlay(oldDir.assoc.presentationBySourceIndex);
  const agentReviewBySourceIndex = new MapOverlay(oldDir.assoc.agentReviewBySourceIndex);
  const correlationByUserEntryId = new MapOverlay(oldDir.assoc.correlationByUserEntryId);
  const clientMessageIdEntries = new MapOverlay(oldDir.assoc.clientMessageIdEntries);

  if (!allSideOnly) {
    for (const [callId, local] of fresh.toolResultSourceIndexByCallId) {
      const global = localToGlobal.get(local);
      if (global == null) continue;
      toolResultByCallId.set(callId, { sourceIndex: global });
      affectedRelations += 1;
    }
    for (const [assistantEntryId, inputEntryId] of fresh.turnInputByAssistantEntryId) {
      turnInputByAssistantEntryId.set(assistantEntryId, inputEntryId);
      affectedRelations += 1;
    }
    for (const [assistantEntryId, locals] of fresh.consumptionByAssistantEntryId) {
      const globals = locals
        .map((local) => localToGlobal.get(local))
        .filter((v): v is number => v != null);
      if (!globals.length) continue;
      consumptionByAssistantEntryId.set(assistantEntryId, globals);
      affectedRelations += 1;
    }
    for (const [suggestionId, decision] of fresh.collabDecisionsBySuggestionId) {
      collabDecisionBySuggestionId.set(suggestionId, {
        ...(typeof decision?.status === "string" ? { status: decision.status } : {}),
        ...(typeof decision?.resultSessionId === "string" ? { resultSessionId: decision.resultSessionId } : {}),
      });
      affectedRelations += 1;
    }
    for (const [local, ref] of fresh.modelCallReferenceBySourceIndex) {
      const global = localToGlobal.get(local);
      if (global == null) continue;
      modelCallRefBySourceIndex.set(global, ref);
      affectedRelations += 1;
    }
    for (const [userLocal, recordLocal] of fresh.originRecordSourceIndexByUserSourceIndex) {
      const gUser = localToGlobal.get(userLocal);
      const gRecord = localToGlobal.get(recordLocal);
      if (gUser == null || gRecord == null) continue;
      originBySourceIndex.set(gUser, { originRecordSourceIndex: gRecord });
      affectedRelations += 1;
    }
    for (const [userLocal, recordLocal] of fresh.presentationRecordSourceIndexByUserSourceIndex) {
      const gUser = localToGlobal.get(userLocal);
      const gRecord = localToGlobal.get(recordLocal);
      if (gUser == null || gRecord == null) continue;
      presentationBySourceIndex.set(gUser, { presentationRecordSourceIndex: gRecord });
      affectedRelations += 1;
    }
    for (const [userLocal, recordLocal] of fresh.agentReviewRecordSourceIndexByUserSourceIndex) {
      const gUser = localToGlobal.get(userLocal);
      const gRecord = localToGlobal.get(recordLocal);
      if (gUser == null || gRecord == null) continue;
      agentReviewBySourceIndex.set(gUser, { reviewRecordSourceIndex: gRecord, completed: true });
      affectedRelations += 1;
    }
    // 跨追加 correlation：新 correlation 记录引用旧 user → ambiguous 更新（X12）
    for (const entry of newEntries) {
      if (entry?.role !== "custom" || entry.customType !== DESKTOP_INPUT_CORRELATION_TYPE) continue;
      const data = entry.data as { schemaVersion?: number; sourceEntryId?: unknown; clientMessageId?: unknown } | null | undefined;
      if (data?.schemaVersion !== 1) continue;
      const clientMessageId = typeof data.clientMessageId === "string" && data.clientMessageId ? data.clientMessageId : null;
      const sourceEntryId = typeof data.sourceEntryId === "string" && data.sourceEntryId ? data.sourceEntryId : null;
      if (!clientMessageId || !sourceEntryId) continue;
      if (appendedProjected.some((p) => p?.id === sourceEntryId)) continue; // 新 user 已随链处理
      // 旧条目（含旧 user）仍按 entryId 注入 ambiguous；投影合并仅消费 user 记录
      correlationByUserEntryId.set(sourceEntryId, { acceptanceDiagnostic: "ambiguous" });
      affectedRelations += 1;
      const set = clientMessageIdEntries.get(clientMessageId) ?? new Set<string>();
      set.add(sourceEntryId);
      clientMessageIdEntries.set(clientMessageId, set);
    }
  }

  // todo 快照指针（X13）：仅新增记录参与判定；新无合法快照 → 指针保持
  let todoSnapshot = oldDir.assoc.todoSnapshot;
  if (!allSideOnly) {
    for (let local = appendedMessages.length - 1; local >= 0; local -= 1) {
      const global = localToGlobal.get(local);
      if (global == null) continue;
      const message = appendedMessages[local];
      const isTodoCandidate = message.role === "toolResult"
        ? TODO_TOOL_NAMES.includes(message.toolName)
        : message.role === "custom" && message.customType === TODO_STATE_CUSTOM_TYPE;
      if (!isTodoCandidate) continue;
      const snapshot = extractLatestTodoSnapshot([message]);
      if (snapshot) {
        todoSnapshot = { sourceIndex: global };
        affectedRelations += 1;
        break;
      }
    }
  }

  // 文件引用集合（C04）：旧集合 copy-on-write + 追加记录并入
  const activeFileReferenceIdentities = new Set(oldDir.session.activeFileReferenceIdentities ?? []);
  if (!allSideOnly) {
    for (const identity of collectSessionFileReferenceIdentities(appendedMessages) as Set<string>) {
      if (!activeFileReferenceIdentities.has(identity)) {
        activeFileReferenceIdentities.add(identity);
        affectedRelations += 1;
      }
    }
  }

  // ── 新版本目录（base+tail overlay）──
  const displayTotal = oldDir.session.displayTotal + displayAdded;
  const tailLineage = allSideOnly ? [] : appendedChain;
  const directory: HistoryDirectory = {
    version: oldDir.version,
    key: { ...oldDir.key, fileIdentity: ctx.fileIdentity },
    file: {
      observedFileSize: newSize,
      indexedThroughOffset: scan.bounds.indexedThroughOffset,
      pendingTailOffset: scan.bounds.pendingTailOffset,
      lastUndelimitedRow: scan.bounds.lastUndelimitedRow,
      header: oldDir.file.header,
      byEntryId: physicalByEntryId,
      byteOffsets: overlayAppend(oldDir.file.byteOffsets, tailOffsets),
      byteLengths: overlayAppend(oldDir.file.byteLengths, tailLengths),
      physicalCount: oldDir.file.physicalCount + newEntries.length,
    },
    branch: {
      headRowExists: ctx.branchHeadRowExists,
      persistedLeafId: ctx.branchHeadRowExists ? (ctx.branchHeadRow?.leafId ?? null) : null,
      observedTailLeafId: ctx.branchHeadRowExists ? (ctx.branchHeadRow?.observedTailLeafId ?? null) : null,
      selectedLeafId,
      physicalTailLeafId,
      headResolution,
      lineageEntryIds: overlayAppend(oldDir.branch.lineageEntryIds, tailLineage),
    },
    records: overlayAppend(oldDir.records, appendedRecordFacts),
    displayableSourceIndexes: overlayAppend(oldDir.displayableSourceIndexes, newDisplayables),
    blockAnchorByAfterIndex: overlayAppend(oldDir.blockAnchorByAfterIndex, newAnchors),
    assoc: {
      toolResultByCallId,
      correlationByUserEntryId,
      turnInputByAssistantEntryId,
      consumptionByAssistantEntryId,
      consumptionDeliveryIds: overlayAppend(oldDir.assoc.consumptionDeliveryIds, []),
      consumptionEntryIds: overlayAppend(oldDir.assoc.consumptionEntryIds, []),
      collabDecisionBySuggestionId,
      modelCallRefBySourceIndex,
      originBySourceIndex,
      presentationBySourceIndex,
      agentReviewBySourceIndex,
      deferredInterludeAnchors: overlayAppend(oldDir.assoc.deferredInterludeAnchors, newDeferredAnchors),
      mediaResultRecords: overlayAppend(oldDir.assoc.mediaResultRecords, newMedia),
      todoSnapshot,
      runBoundsByOrdinal,
      clientMessageIdEntries,
    },
    overview,
    session: {
      displayTotal,
      publicRevision: ctx.publicRevision,
      fileIdentity: ctx.fileIdentity,
      mutationEpochAtBuild: oldDir.session.mutationEpochAtBuild,
      activeFileReferenceIdentities,
      tailPointerEntryId: fresh.finalTurnInputEntryId,
      tailPointerVisible: fresh.finalTurnInputVisible,
      tailAssistantOrdinal: fresh.finalAssistantOrdinal,
      tailRunOrdinal: fresh.finalRunOrdinal,
    },
    measuredBytes: 0,
  };
  // C05：增量 measuredBytes = 旧估算 + 追加段估算（单调可加、保守、O(新增)）；
  // overlay 只计量 delta（base 引用不重复计入）。
  directory.measuredBytes = oldDir.measuredBytes + 8192 + measureDirectoryBytes({
    appendedRecordFacts,
    newAnchors,
    newDeferredAnchors,
    newMedia,
    newDisplayables,
    affectedRelations,
    tailLineage,
    physicalByEntryId: physicalByEntryId.delta,
    byteTailOffsets: tailOffsets,
    byteTailLengths: tailLengths,
    overviewRunSizes: (overview.runSizeByOrdinal as MapOverlay<number, number>).delta,
    overviewTaskCategories: (overview.taskCategories as MapOverlay<string, HistoryTaskCategory>).delta,
  });

  // C01 §7：追加与分支选择变化同时发生（head 规则选出的分支 ≠ 旧视图分支，且本次
  // 追加不延长当前分支）→ 物理索引更新后按 head 规则重建分支视图，不静默沿用旧
  // 视图（X09/E04：视图必须与 selected 一致；rewind 前缀重建 O(N) 允许）。
  const appendExtendsCurrentBranch = selectedLeafId != null && appendedChain.includes(selectedLeafId);
  let finalDirectory = directory;
  if (ctx.branchHeadRowExists && !appendExtendsCurrentBranch && selectedLeafId !== oldDir.branch.selectedLeafId) {
    const rebuilt = rebuildBranchView(directory, ctx);
    if (rebuilt.kind !== "ok") {
      return { kind: "fail", reason: rebuilt.reason ?? "snapshot_changed", detail: rebuilt.detail };
    }
    finalDirectory = rebuilt.directory;
  }

  return {
    kind: "ok",
    directory: finalDirectory,
    metrics: {
      appendedBytes,
      rereadBytes,
      validatedBytes: rereadBytes + appendedBytes,
      parsedRecords: newEntries.length,
      appendedEntries: newEntries.length,
      appendedRecords: appendedRecordFacts.length,
      affectedRelations,
      durationMs: Number(process.hrtime.bigint() - started) / 1e6,
    },
  };

  function allInBranchAppend(): boolean {
    return !allSideOnly && projectedInBranch.length === appendedProjected.filter((p) => p != null).length;
  }
}

/**
 * C01 branch_view_stale：文件不变、head 选择改变 → 保留物理索引与记录事实，
 * 仅重建分支视图（O(N) 元数据，TASKBOOK 允许）。支持「rewind 到已构建 lineage
 * 内节点」：前缀展示序/Run 边界不变，直接截取。其他形态（切换到非 lineage 内
 * 未构建节点）→ fail snapshot_changed 走全量重建（保守，不混页）。
 */
function lineageIndexOf(list: SegmentedList<string>, id: string): number {
  for (let i = 0; i < list.length; i += 1) if (list.at(i) === id) return i;
  return -1;
}

export function rebuildBranchView(
  old: HistoryDirectory,
  ctx: HistoryReadContext,
): { kind: "ok"; directory: HistoryDirectory } | { kind: "fail"; reason: InvalidationReason; detail?: string } {
  if (!ctx.branchHeadRowExists) return { kind: "fail", reason: "snapshot_changed", detail: "no_head_row" };
  const selected = ctx.branchHeadRow?.leafId ?? null;
  if (selected == null || lineageIndexOf(old.branch.lineageEntryIds, selected) === -1) {
    return { kind: "fail", reason: "snapshot_changed", detail: "selected_not_built" };
  }
  // 前缀校验：selected 必须是旧 lineage 的前缀成员（rewind），否则 → 全量重建
  const oldLineage = old.branch.lineageEntryIds;
  const cutIndex = (() => {
    for (let i = 0; i < oldLineage.length; i += 1) if (oldLineage.at(i) === selected) return i;
    return -1;
  })();
  if (cutIndex === -1) {
    return { kind: "fail", reason: "snapshot_changed", detail: "selected_not_in_built_lineage" };
  }

  const prefixIds = new Set<string>();
  for (let i = 0; i <= cutIndex; i += 1) prefixIds.add(oldLineage.at(i)!);

  const keptDisplayable: number[] = [];
  const keptAnchors: Array<{ afterIndex: number; sourceIndex: number }> = [];
  const keptDeferred: Array<{ anchorAfterIndex: number | null; sourceIndex: number; deliveryId: string | null }> = [];
  const keptMedia: Array<{ sourceIndex: number; taskId: string; success: boolean }> = [];
  let keptRecords = 0;
  let displayTotal = 0;
  for (let sourceIndex = 0; sourceIndex < old.records.length; sourceIndex += 1) {
    const fact = old.records.at(sourceIndex)!;
    const kept = fact.entryId != null && prefixIds.has(fact.entryId);
    if (kept) {
      keptRecords += 1;
      if (fact.displayIndex != null) {
        keptDisplayable.push(sourceIndex);
        displayTotal = fact.displayIndex + 1;
      }
      if (fact.role === "toolResult" || (fact.role === "custom" && fact.displayIndexBefore - 1 < displayTotal && fact.displayIndexBefore - 1 >= 0)) {
        const afterIndex = fact.displayIndexBefore - 1;
        if (afterIndex >= 0 && afterIndex < displayTotal) {
          keptAnchors.push({ afterIndex, sourceIndex });
        }
      }
    }
  }
  for (const anchor of old.assoc.deferredInterludeAnchors) {
    if (anchor.anchorAfterIndex != null && anchor.anchorAfterIndex < displayTotal) keptDeferred.push(anchor);
  }
  for (const media of old.assoc.mediaResultRecords) {
    if (media.sourceIndex < keptRecords) keptMedia.push(media);
  }

  const directory: HistoryDirectory = {
    version: old.version,
    key: old.key,
    file: old.file,
    branch: {
      headRowExists: true,
      persistedLeafId: selected,
      observedTailLeafId: ctx.branchHeadRow?.observedTailLeafId ?? null,
      selectedLeafId: selected,
      physicalTailLeafId: old.branch.physicalTailLeafId,
      headResolution: "persisted_head",
      lineageEntryIds: new SegmentedList([oldLineage.slice(0, cutIndex + 1)]),
    },
    records: new SegmentedList([old.records.slice(0, keptRecords)]),
    displayableSourceIndexes: new SegmentedList([keptDisplayable]),
    blockAnchorByAfterIndex: new SegmentedList([keptAnchors]),
    assoc: {
      ...old.assoc,
      deferredInterludeAnchors: new SegmentedList([keptDeferred]),
      mediaResultRecords: new SegmentedList([keptMedia]),
    },
    session: { ...old.session, displayTotal },
    // E04：分支 rewind = 分支重建 → 概览计数按保留前缀重算（O(N) 允许），
    // 不返回旧分支统计（E04.3）。
    overview: computeOverviewFromRecords(old.records.slice(0, keptRecords)),
    measuredBytes: old.measuredBytes,
  };
  return { kind: "ok", directory };
}
