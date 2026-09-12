/**
 * 历史读取 —— 编排入口（阶段 B / B06，计划 §1.11/§4）
 *
 * readSessionHistoryPage 是普通分页的目录快路径 + 快照/回退链：
 *   captureReadContext → probe（命中）→ resolveHistoryPage → readHistoryRecords
 *   → projectHistoryPage（热页稀疏视图+目录事实+页首 seed）→ hydrateExternalState
 *   → 读后复核（stat+head+locator 仍等于捕获值）→ 返回捕获 revision（I07）。
 * 失败链：目录尝试 1 → invalidate(reason)+重建 → 尝试 2 → legacy 全量
 * （loadSessionHistoryMessages + 同一 projector 全量模式，记 fallbackReason）→
 * 真实 I/O 错误沿既有 500 路径（不返回旧页、不伪造空历史，P11）。
 * B 阶段失效策略（§6）：任何无法证明稳定的变化 → snapshot_changed/重建；追加分类留给 C。
 * 三种模式（全量 / 冷构建 / 热命中）共用同一 projector 与同一事实实现（I04）；
 * 日志只含内部会话标识、阶段、reason、版本摘要与计数，无正文无工具输出。
 */
import { loadSessionHistoryMessages, historyMessageFromEntry } from "../../core/message-utils.ts";
import { sessionFileMutationEpoch } from "../../core/session-file-mutation-epoch.ts";
import { projectSessionMessageForDisplay } from "../../core/session-reminders.ts";
import { collectToolOutcomesByCallId } from "../../shared/tool-outcome.ts";
import { resolveDeferredReceiverName } from "../deferred-result-interlude.ts";
import { createModuleLogger } from "../../lib/debug-log.ts";
import { extractLatestTodoSnapshot } from "../../lib/tools/todo-compat.ts";
import { captureReadContext } from "./read-context.ts";
import { HistoryDirectoryCache, historyDirectoryCacheKey } from "./cache.ts";
import { buildHistoryDirectoryIncremental, rebuildBranchView } from "./incremental.ts";
import { readHistoryRecords, collectLocations } from "./window-reader.ts";
import {
  resolveHistoryPage,
  resolveHistoryPageBounds,
} from "./page.ts";
import {
  identityRecordView,
  projectHistoryPage,
  type HistoryRecordView,
} from "./project-page.ts";
import { buildHistoryDirectory } from "./directory.ts";
import { scanHistoryFile } from "./scan.ts";
import { buildProjectionContextFromMessages, type ProjectionContext } from "./projection-context.ts";
import { hydrateExternalState } from "./hydrate.ts";
import type { HistoryBranchIdentity } from "./protocol.ts";
import {
  HISTORY_PROTOCOL_PAGE_LIMIT,
  type HistoryOverview,
  type HistoryOverviewUnavailableReason,
} from "./protocol.ts";
import type { HistoryDirectory, HistoryReadContext, HistoryReadHook, InvalidationReason } from "./types.ts";

export { INVALIDATION_REASONS } from "./types.ts";

const log = createModuleLogger("sessions/history-read");

export interface RoutePageResult {
  messages: any[];
  blocks: any[];
  todos: any[] | null;
  todoPanel?: Record<string, unknown> | null;
  hasMore: boolean;
  nextBefore: string | null;
  sessionFiles: any[];
}

export interface ReadSessionHistoryPageInput {
  engine: any;
  cache: HistoryDirectoryCache;
  sessionPath: string;
  sessionId: string | null;
  studioId?: string | null;
  beforeId: number | null;
  limit: number;
  forceAll: boolean;
  sanitizeVisibleContent: (value: string) => string;
  /** DI/测试用：关闭目录快路径（all=1 之外的特殊场景），不新增公开 query 参数。 */
  disableCache?: boolean;
  /** DI/测试用：扫描/定点读取的读取注入点（故障演练）。 */
  readFile?: HistoryReadHook;
}

export type ReadSessionHistoryPageOutcome =
  | { mode: "directory"; result: RoutePageResult; revision: string; fallbackReason: null; buildCount: number; rebuilds: number; branchIdentity: HistoryBranchIdentity | null }
  | { mode: "full"; result: RoutePageResult; revision: string | null; fallbackReason: InvalidationReason | null; buildCount: number; rebuilds: number; branchIdentity: null }
  | { mode: "error"; error: unknown };

function zeroSeed() {
  return { displayIdx: 0, latestTurnInputEntryId: null, latestTurnInputVisible: true, assistantOrdinalInTurn: 0 };
}

function mapRecordView(records: Map<number, any>, length: number): HistoryRecordView {
  return {
    length,
    get: (sourceIndex: number) => records.get(sourceIndex),
  };
}

/**
 * 全量模式（无缓存）：loadSessionHistoryMessages 的产物走同一 projector
 * （buildProjectionContextFromMessages + projectHistoryPage + hydrateExternalState）。
 * reconciliation 与 legacy 回退共用；all=1 亦经此（无窗口性能断言，B07）。
 */
export async function projectFullHistoryPage(
  engine: any,
  input: {
    sessionPath: string | null;
    sourceMessages: any[];
    beforeId: number | null;
    limit: number;
    forceAll: boolean;
    sanitizeVisibleContent: (value: string) => string;
  },
): Promise<RoutePageResult> {
  const context = buildProjectionContextFromMessages(input.sourceMessages);
  const pageBounds = resolveHistoryPageBounds(input.sourceMessages, {
    beforeId: input.beforeId,
    limit: input.limit,
    forceAll: input.forceAll,
  });
  const toolOutcomesByCallId = collectToolOutcomesByCallId(input.sourceMessages);
  const receiverName = resolveDeferredReceiverName(engine, input.sessionPath);
  const projected = projectHistoryPage({
    records: identityRecordView(input.sourceMessages),
    context,
    bounds: pageBounds,
    seed: zeroSeed(),
    iterate: input.sourceMessages.map((message, sourceIndex) => ({ sourceIndex, message })),
    sanitizeVisibleContent: input.sanitizeVisibleContent,
    toolOutcomesByCallId,
    engine,
    receiverName,
  });
  const { slicedBlocks, sessionFiles, todos, todoPanel } = await hydrateExternalState({
    engine,
    sessionPath: input.sessionPath,
    pageBounds,
    forceAll: input.forceAll,
    activeReferences: input.sourceMessages,
    todoSnapshot: context.todoSnapshot,
    blocks: projected.blocks,
    mediaGenerationResults: projected.mediaGenerationResults,
    standaloneMediaGenerationResults: projected.standaloneMediaGenerationResults,
    recordMediaGenerationResult: projected.recordMediaGenerationResult,
    recordDeferredInterlude: projected.recordDeferredInterlude,
  });
  return {
    messages: projected.messages,
    blocks: slicedBlocks,
    todos,
    todoPanel,
    hasMore: pageBounds.hasMore,
    nextBefore: pageBounds.hasMore ? String(pageBounds.startIdx) : null,
    sessionFiles,
  };
}

/**
 * 热页上下文物化：目录事实 + 本页读取到的稀疏记录 → ProjectionContext 形状
 * （同形状不同来源，I04）。注释/决策数据来自被读取的注释记录本身（指针在目录），
 * 不重放任何 pending 规则；Run 边界/ordinal 从目录记录级事实直取。
 * entryId → sourceIndex/displayIndex 经 byEntryId 的物理下标换算（头部恒为物理 0）。
 */
function materializeProjectionContext(
  directory: HistoryDirectory,
  records: Map<number, any>,
  iterateIndexes: number[],
  anchorMapsCache: WeakMap<HistoryDirectory, Map<number, { sourceIndex: number; afterIndex: number | null; deliveryId: string | null }>>,
): ProjectionContext {
  const physicalOffset = directory.file.header ? 1 : 0;
  const sourceIndexByEntryId = {
    get: (entryId: string) => {
      if (entryId == null) return undefined;
      const physicalIndex = directory.file.byEntryId.get(entryId);
      return physicalIndex != null ? physicalIndex - physicalOffset : undefined;
    },
  };
  const displayIndexByEntryId = {
    get: (entryId: string) => {
      const sourceIndex = sourceIndexByEntryId.get(entryId);
      const fact = sourceIndex != null && sourceIndex >= 0 ? directory.records.at(sourceIndex) : null;
      return fact ? fact.displayIndex ?? undefined : undefined;
    },
  };
  const toolResultSourceIndexByCallId = {
    get: (callId: string) => directory.assoc.toolResultByCallId.get(callId)?.sourceIndex,
  };

  const originBySourceIndex = new Map();
  const presentationBySourceIndex = new Map();
  const agentReviewBySourceIndex = new Map();
  const runBoundsBySourceIndex = new Map();
  const runOrdinalBySourceIndex = new Map();
  for (const sourceIndex of iterateIndexes) {
    const fact = directory.records.at(sourceIndex);
    if (!fact) continue;
    if (fact.runOrdinal != null) {
      // C03：Run 边界以 Run 小表为准（增量延续开放 Run 后，旧记录事实的 end 已过期）
      const bounds = directory.assoc.runBoundsByOrdinal.get(fact.runOrdinal);
      if (bounds) runBoundsBySourceIndex.set(sourceIndex, { start: bounds.start, end: bounds.end });
    } else if (fact.turnStartIndex != null) {
      runBoundsBySourceIndex.set(sourceIndex, { start: fact.turnStartIndex, end: fact.turnEndIndex });
    }
    if (fact.runOrdinal != null) runOrdinalBySourceIndex.set(sourceIndex, fact.runOrdinal);
    const originPtr = directory.assoc.originBySourceIndex.get(sourceIndex);
    if (originPtr) {
      const data = records.get(originPtr.originRecordSourceIndex)?.data;
      if (data?.origin != null) {
        originBySourceIndex.set(sourceIndex, {
          origin: data.origin,
          ...(typeof data.displayText === "string" ? { displayText: data.displayText } : {}),
        });
      }
    }
    const presentationPtr = directory.assoc.presentationBySourceIndex.get(sourceIndex);
    if (presentationPtr) {
      const data = records.get(presentationPtr.presentationRecordSourceIndex)?.data;
      if (data != null) presentationBySourceIndex.set(sourceIndex, data);
    }
    const reviewPtr = directory.assoc.agentReviewBySourceIndex.get(sourceIndex);
    if (reviewPtr) {
      const data = records.get(reviewPtr.reviewRecordSourceIndex)?.data;
      if (data?.status === "completed") agentReviewBySourceIndex.set(sourceIndex, data);
    }
  }

  // deferred-result 锚点 Map 按 directory 生命周期 memoize（值是同一 finalize 判定）。
  let deferredInterludeAnchors = anchorMapsCache?.get(directory);
  if (!deferredInterludeAnchors) {
    deferredInterludeAnchors = new Map();
    for (const anchor of directory.assoc.deferredInterludeAnchors) {
      if (anchor.anchorAfterIndex == null) continue;
      deferredInterludeAnchors.set(anchor.sourceIndex, {
        sourceIndex: anchor.sourceIndex,
        afterIndex: anchor.anchorAfterIndex,
        deliveryId: anchor.deliveryId,
      });
    }
    anchorMapsCache?.set(directory, deferredInterludeAnchors);
  }

  return {
    originBySourceIndex,
    presentationBySourceIndex,
    agentReviewBySourceIndex,
    originRecordSourceIndexByUserSourceIndex: directory.assoc.originBySourceIndex as any,
    presentationRecordSourceIndexByUserSourceIndex: directory.assoc.presentationBySourceIndex as any,
    agentReviewRecordSourceIndexByUserSourceIndex: new Map(
      [...directory.assoc.agentReviewBySourceIndex].map(([k, v]) => [k, v.reviewRecordSourceIndex]),
    ) as any,
    runBoundsBySourceIndex,
    runOrdinalBySourceIndex,
    collabDecisionsBySuggestionId: directory.assoc.collabDecisionBySuggestionId as any,
    modelCallReferenceBySourceIndex: directory.assoc.modelCallRefBySourceIndex as any,
    toolResultSourceIndexByCallId: toolResultSourceIndexByCallId as any,
    sourceIndexByEntryId: sourceIndexByEntryId as any,
    displayIndexByEntryId: displayIndexByEntryId as any,
    turnInputByAssistantEntryId: directory.assoc.turnInputByAssistantEntryId,
    turnInputConsumptionDeliveryIds: new Set(directory.assoc.consumptionDeliveryIds),
    turnInputConsumptionEntryIds: new Set(directory.assoc.consumptionEntryIds),
    mediaResultRecords: [...directory.assoc.mediaResultRecords],
    deferredInterludeAnchors,
    todoSnapshot: null,
    todoSnapshotSourceIndex: directory.assoc.todoSnapshot?.sourceIndex ?? null,
    recordFacts: [],
    displayTotal: directory.session.displayTotal,
    finalTurnInputEntryId: directory.session.tailPointerEntryId ?? null,
    finalTurnInputVisible: directory.session.tailPointerVisible ?? true,
    finalAssistantOrdinal: directory.session.tailAssistantOrdinal ?? 0,
    finalRunOrdinal: directory.session.tailRunOrdinal ?? 0,
    runBoundsByOrdinal: directory.assoc.runBoundsByOrdinal,
    consumptionByAssistantEntryId: directory.assoc.consumptionByAssistantEntryId,
  };
}

/** 读后一致性（I06/I07）：stat + head + locator 仍等于捕获（或预期写回）值才允许发布。 */
function contextsConsistent(
  before: HistoryReadContext,
  after: HistoryReadContext,
  expectedHeadRow?: { leafId: string | null; observedTailLeafId: string | null } | null,
): boolean {
  const dbg = process.env.HISTORY_READ_DEBUG === "1";
  const dbgFail = (why: string) => { if (dbg) console.error(`[dbg] inconsistent: ${why}`); return false; };
  if (before.publicRevision !== after.publicRevision) return dbgFail(`revision ${before.publicRevision} vs ${after.publicRevision}`);
  const a = before.fileIdentity;
  const b = after.fileIdentity;
  if (a.size !== b.size || a.mtimeMs !== b.mtimeMs) return dbgFail(`identity size/mtime ${a.size}/${a.mtimeMs} vs ${b.size}/${b.mtimeMs}`);
  if (a.dev != null && b.dev != null && a.dev !== b.dev) return dbgFail("dev");
  if (a.ino != null && b.ino != null && a.ino !== b.ino) return dbgFail("ino");
  if (a.ctimeMs != null && b.ctimeMs != null && a.ctimeMs !== b.ctimeMs) return dbgFail(`ctime ${a.ctimeMs} vs ${b.ctimeMs}`);
  // C03：head 恢复写回（legacy backfill / append recovery / observe tail）是构建的
  // 确定性产物——读后 head 与「预期写回行」一致即视为稳定（而非误判 snapshot_changed）。
  if (expectedHeadRow) {
    if (!after.branchHeadRowExists) return dbgFail("expected head row but none");
    if ((after.branchHeadRow?.leafId ?? null) !== (expectedHeadRow.leafId ?? null)) return dbgFail(`expected leaf ${expectedHeadRow.leafId} vs ${after.branchHeadRow?.leafId}`);
    if ((after.branchHeadRow?.observedTailLeafId ?? null) !== (expectedHeadRow.observedTailLeafId ?? null)) return dbgFail(`expected tail ${expectedHeadRow.observedTailLeafId} vs ${after.branchHeadRow?.observedTailLeafId}`);
  } else {
    if (before.branchHeadRowExists !== after.branchHeadRowExists) return false;
    if ((before.branchHeadRow?.leafId ?? null) !== (after.branchHeadRow?.leafId ?? null)) return false;
    if ((before.branchHeadRow?.observedTailLeafId ?? null) !== (after.branchHeadRow?.observedTailLeafId ?? null)) return false;
  }
  if ((before.locatorPath ?? null) !== (after.locatorPath ?? null)) return false;
  return true;
}

interface DirectoryAttempt {
  kind: "ok" | "fail" | "unavailable";
  reason?: InvalidationReason;
  message?: string;
  result?: RoutePageResult;
  revision?: string | null;
  buildCount?: number;
  fromCache?: boolean;
  /** E02：本次目录的分支选择身份（条件 GET 标签作用域；full/失败路径为 null）。 */
  branchIdentity?: HistoryBranchIdentity | null;
}

async function tryDirectoryOnce(
  engine: any,
  cache: HistoryDirectoryCache,
  input: ReadSessionHistoryPageInput,
  anchorMapsCache: WeakMap<HistoryDirectory, Map<number, { sourceIndex: number; afterIndex: number | null; deliveryId: string | null }>>,
): Promise<DirectoryAttempt> {
  const ctx = await captureReadContext(engine, {
    sessionPath: input.sessionPath,
    sessionId: input.sessionId,
    studioId: input.studioId,
  });
  if (!ctx) return { kind: "unavailable", reason: "revision_unknown" };
  const key = historyDirectoryCacheKey(ctx);
  let directory: HistoryDirectory | null = null;
  let materialized: ProjectionContext | null = null;
  let buildCount = 0;
  let expectedHeadAfter: { leafId: string | null; observedTailLeafId: string | null } | null | undefined;

  if (!input.disableCache) {
    const hit = cache.get(key);
    if (hit) {
      const verdict = cache.probe(key, ctx);
      if (verdict === "valid") {
        directory = hit.directory;
      } else if (verdict === "append_candidate") {
        // C03 可信追加：增量续读 + 受影响关系更新（old base 段共享，I06）。
        // 失败（结构非法/尾记录变化/混合形态）→ invalidate → 全量重建（严格层）。
        const started = process.hrtime.bigint();
        const incremental = await buildHistoryDirectoryIncremental({
          old: hit.directory,
          sessionPath: input.sessionPath,
          ctx,
          readFile: input.readFile,
        });
        const durationMs = Number(process.hrtime.bigint() - started) / 1e6;
        if (incremental.kind === "ok") {
          const lease = await cache.beginBuild(key);
          if (lease) {
            incremental.directory.session.mutationEpochAtBuild = sessionFileMutationEpoch(ctx.sessionPath);
            const accepted = cache.publish(key, lease, incremental.directory, {
              released: () => {},
              locatorPath: ctx.locatorPath,
            });
            if (accepted) {
              directory = incremental.directory;
              expectedHeadAfter = null; // 增量不写回 head；post-read 对 ctx 行复核
              cache.noteIncrementalUpdate(durationMs, incremental.metrics);
              log.info(
                `directory incremental update session=${ctx.sessionId ?? "path"} ` +
                `appendedBytes=${incremental.metrics.appendedBytes} rereadBytes=${incremental.metrics.rereadBytes} ` +
                `appendedRecords=${incremental.metrics.appendedRecords} affectedRelations=${incremental.metrics.affectedRelations} ` +
                `durationMs=${durationMs.toFixed(2)}`,
              );
            }
          } else {
            cache.invalidate(key, "snapshot_changed");
          }
        } else {
          log.warn(
            `directory incremental failed session=${ctx.sessionId ?? "path"} reason=${incremental.reason}` +
            `${incremental.detail ? ` detail=${incremental.detail}` : ""} → full rebuild`,
          );
          cache.invalidate(key, "snapshot_changed");
        }
      } else if (verdict === "branch_view_stale") {
        // C01：文件不变、分支选择改变 → 保留物理索引，O(N) 元数据重建分支视图
        const rebuilt = rebuildBranchView(hit.directory, ctx);
        if (rebuilt.kind === "ok") {
          directory = rebuilt.directory;
          cache.noteBranchViewRebuild();
        } else {
          log.warn(`branch view rebuild failed session=${ctx.sessionId ?? "path"} → full rebuild`);
          cache.invalidate(key, rebuilt.reason ?? "snapshot_changed");
        }
      }
    }
  }

  if (!directory && !input.disableCache) {
    const lease = await cache.beginBuild(key);
    if (!lease) return { kind: "unavailable", reason: "budget_exceeded" };
    buildCount += 1;
    try {
      const scan = await scanHistoryFile(input.sessionPath, {
        capturedLength: ctx.fileIdentity.size,
        readFile: input.readFile,
      });
      const built = await buildHistoryDirectory(scan, ctx);
      if (!built.directory) {
        const refusal = built as { reason: InvalidationReason; detail?: string };
        cache.failBuild(key, lease);
        cache.invalidate(key, refusal.reason);
        log.warn(
          `directory build refused session=${ctx.sessionId ?? "path"} reason=${refusal.reason}` +
            (refusal.detail ? ` detail=${refusal.detail}` : ""),
        );
        return { kind: "unavailable", reason: refusal.reason };
      }
      directory = built.directory;
      materialized = built.context;
      expectedHeadAfter = built.headWriteBack;
      // C02：记录发布时点的变更世代（probe 第 6 步据此区分可信追加与重写）。
      built.directory.session.mutationEpochAtBuild = sessionFileMutationEpoch(ctx.sessionPath);
      const accepted = cache.publish(key, lease, built.directory, {
        released: built.released,
        locatorPath: ctx.locatorPath,
      });
      if (!accepted) {
        // 并发竞态：他人已发布更新版本。本请求按本次尝试失败处理（不使用旧页）。
        directory = null;
      }
    } catch (error) {
      cache.failBuild(key, lease);
      throw error;
    }
  }

  if (!directory) {
    return { kind: "fail", reason: "snapshot_changed", buildCount };
  }

  const page = resolveHistoryPage(directory, {
    beforeId: input.beforeId,
    limit: input.limit,
    forceAll: false,
  });
  const locations = collectLocations(page.iterateRecordIndexes, directory, page.dependencyLocations);
  const read = await readHistoryRecords(input.sessionPath, ctx.fileIdentity, locations, {
    readFile: input.readFile,
  });
  if (!read.ok) {
    const failure = read as { ok: false; reason: InvalidationReason; message?: string };
    return { kind: "fail", reason: failure.reason, message: failure.message, buildCount };
  }

  // 逐记录投影（与 projectBranchHistory 完全同链：historyMessageFromEntry →
  // correlation 合并 → projectSessionMessageForDisplay）。热页读取到的是原始条目，
  // 必须经过与全量路径相同的投影链，projector 的输入才同形状（I04）。
  const projectedRecords = new Map<number, any>();
  for (const [sourceIndex, entry] of read.records) {
    const message = historyMessageFromEntry(entry);
    if (!message) continue;
    projectedRecords.set(sourceIndex, projectSessionMessageForDisplay({
      ...message,
      ...(message.role === "user" ? (directory.assoc.correlationByUserEntryId.get(message.id) || {}) : {}),
    }));
  }

  // 读后复核（I06/I07）：捕获边界内的读取才算数；之后发生的变化归下一次请求。
  const after = await captureReadContext(engine, {
    sessionPath: input.sessionPath,
    sessionId: input.sessionId,
    studioId: input.studioId,
  });
  if (!after || !contextsConsistent(ctx, after, expectedHeadAfter ?? null)) {
    return { kind: "fail", reason: "snapshot_changed", buildCount };
  }

  const receiverName = resolveDeferredReceiverName(engine, input.sessionPath);
  const context = materialized
    ?? materializeProjectionContext(directory, projectedRecords, page.iterateRecordIndexes, anchorMapsCache);
  const toolOutcomesByCallId = collectToolOutcomesByCallId([...projectedRecords.values()]);
  const projected = projectHistoryPage({
    records: mapRecordView(projectedRecords, directory.records.length),
    context,
    bounds: page.bounds,
    seed: {
      displayIdx: page.headState.displayIdx,
      latestTurnInputEntryId: page.headState.latestTurnInputEntryId,
      latestTurnInputVisible: page.headState.latestTurnInputVisible,
      assistantOrdinalInTurn: page.headState.assistantOrdinalInTurn,
    },
    iterate: page.iterateRecordIndexes
      .map((sourceIndex) => ({ sourceIndex, message: projectedRecords.get(sourceIndex) }))
      .filter((entry) => entry.message != null),
    sanitizeVisibleContent: input.sanitizeVisibleContent,
    toolOutcomesByCallId,
    engine,
    receiverName,
  });

  // todos：目录指针指向的快照记录已随依赖读取，经同一纯函数迁移（不重扫）。
  let todoSnapshot: ReturnType<typeof extractLatestTodoSnapshot> = null;
  if (directory.assoc.todoSnapshot) {
    const record = projectedRecords.get(directory.assoc.todoSnapshot.sourceIndex);
    if (record) todoSnapshot = extractLatestTodoSnapshot([record]);
  }

  const { slicedBlocks, sessionFiles, todos, todoPanel } = await hydrateExternalState({
    engine,
    sessionPath: input.sessionPath,
    pageBounds: page.bounds,
    forceAll: false,
    sessionFileReferenceIdentities: directory.session.activeFileReferenceIdentities,
    todoSnapshot,
    blocks: projected.blocks,
    mediaGenerationResults: projected.mediaGenerationResults,
    standaloneMediaGenerationResults: projected.standaloneMediaGenerationResults,
    recordMediaGenerationResult: projected.recordMediaGenerationResult,
    recordDeferredInterlude: projected.recordDeferredInterlude,
  });

  return {
    kind: "ok",
    result: {
      messages: projected.messages,
      blocks: slicedBlocks,
      todos,
      todoPanel,
      hasMore: page.bounds.hasMore,
      nextBefore: page.bounds.hasMore ? String(page.bounds.startIdx) : null,
      sessionFiles,
    },
    revision: ctx.publicRevision ?? "",
    buildCount,
    fromCache: materialized == null,
    branchIdentity: {
      selectedLeafId: directory.branch.selectedLeafId ?? null,
      physicalTailLeafId: directory.branch.physicalTailLeafId ?? null,
      headResolution: directory.branch.headResolution ?? null,
    },
  };
}

export async function readSessionHistoryPage(  input: ReadSessionHistoryPageInput,
): Promise<ReadSessionHistoryPageOutcome> {
  const { engine, cache } = input;
  const anchorMapsCache: WeakMap<HistoryDirectory, Map<number, { sourceIndex: number; afterIndex: number | null; deliveryId: string | null }>> = new WeakMap();
  let buildCount = 0;
  let rebuilds = 0;
  let fallbackReason: InvalidationReason | null = null;

  try {
    // all=1：同源全量模式（B07，无窗口性能断言、不进目录）。
    if (!input.forceAll && !input.disableCache) {
      const attempt1 = await tryDirectoryOnce(engine, cache, input, anchorMapsCache);
      if (attempt1.kind === "ok") {
        return {
          mode: "directory",
          result: attempt1.result!,
          revision: attempt1.revision ?? "",
          fallbackReason: null,
          buildCount: attempt1.buildCount ?? 0,
          rebuilds: 0,
          branchIdentity: attempt1.branchIdentity ?? null,
        };
      }
      buildCount += attempt1.buildCount ?? 0;
      if (attempt1.kind === "fail") {
        // 尝试 1 失败 → 失效 + 重建（尝试 2）；仍失败 → legacy 全量（§B06 最多 2 次）。
        const key = historyDirectoryCacheKey({
          sessionId: input.sessionId,
          sessionPath: input.sessionPath,
          studioId: input.studioId,
        });
        fallbackReason = attempt1.reason ?? "snapshot_changed";
        cache.invalidate(key, fallbackReason);
        log.warn(
          `directory attempt1 failed session=${input.sessionId ?? "path"} reason=${fallbackReason}` +
            `${attempt1.message ? ` detail=${attempt1.message}` : ""}`,
        );
        rebuilds += 1;
        const attempt2 = await tryDirectoryOnce(engine, cache, input, anchorMapsCache);
        if (attempt2.kind === "ok") {
          return {
            mode: "directory",
            result: attempt2.result!,
            revision: attempt2.revision ?? "",
            fallbackReason: null,
            buildCount: buildCount + (attempt2.buildCount ?? 0),
            rebuilds: 1,
            branchIdentity: attempt2.branchIdentity ?? null,
          };
        }
        buildCount += attempt2.buildCount ?? 0;
        fallbackReason = attempt2.reason ?? fallbackReason ?? "snapshot_changed";
        if (attempt2.kind === "unavailable") {
          // 尝试 2 判定目录不适用（如重建后仍 budget/legacy）→ 直接 legacy，不再重试。
          log.warn(
            `directory attempt2 unavailable session=${input.sessionId ?? "path"} reason=${fallbackReason} → legacy full`,
          );
          const sourceMessages = await loadSessionHistoryMessages(engine, input.sessionPath);
          const result = await projectFullHistoryPage(engine, {
            sessionPath: input.sessionPath,
            sourceMessages,
            beforeId: input.beforeId,
            limit: input.limit,
            forceAll: input.forceAll,
            sanitizeVisibleContent: input.sanitizeVisibleContent,
          });
          return { mode: "full", result, revision: null, fallbackReason, buildCount, rebuilds, branchIdentity: null };
        }
        log.warn(
          `directory attempt2 failed session=${input.sessionId ?? "path"} reason=${fallbackReason} → legacy full`,
        );
      } else {
        // 不可用（revision_unknown / budget_exceeded / legacy_fallback / 目录不适用）：
        // 确定性原因，不重试，直接 legacy 全量。
        fallbackReason = attempt1.reason ?? "legacy_fallback";
        log.warn(
          `directory unavailable session=${input.sessionId ?? "path"} reason=${fallbackReason} → legacy full`,
        );
      }
    }

    // legacy 全量（不使用任何旧目录；自带既有 repair/SDK 语义与 revision 纪律）。
    const sourceMessages = await loadSessionHistoryMessages(engine, input.sessionPath);
    const result = await projectFullHistoryPage(engine, {
      sessionPath: input.sessionPath,
      sourceMessages,
      beforeId: input.beforeId,
      limit: input.limit,
      forceAll: input.forceAll,
      sanitizeVisibleContent: input.sanitizeVisibleContent,
    });
    return { mode: "full", result, revision: null, fallbackReason, buildCount, rebuilds, branchIdentity: null };
  } catch (error) {
    // 真实 I/O/逻辑错误：交回路由既有 500 路径（不返回旧页、不伪造空历史，P11）。
    log.error(`history read failed session=${input.sessionId ?? "path"}: ${error?.message || error}`);
    return { mode: "error", error };
  }
}

// ── E04：复用目录的会话概览（不做页面投影/hydrate；冷热边界见 E04.3） ─────────

export type HistoryOverviewResult =
  | { kind: "ok"; overview: HistoryOverview }
  | { kind: "unavailable"; reason: HistoryOverviewUnavailableReason };

/** 目录构建拒绝原因 → 概览不可用原因（E04.1 三 reason，逐一可测）。 */
export function mapOverviewUnavailableReason(reason: InvalidationReason): HistoryOverviewUnavailableReason {
  if (reason === "revision_unknown") return "revision_unknown";
  if (reason === "legacy_fallback") return "unsupported_history";
  return "directory_unavailable";
}

export interface ReadSessionHistoryOverviewInput {
  engine: any;
  cache: HistoryDirectoryCache;
  sessionPath: string | null;
  sessionId: string | null;
  studioId?: string | null;
  disableCache?: boolean;
}

function overviewUnavailable(
  reason: HistoryOverviewUnavailableReason,
): HistoryOverviewResult {
  return { kind: "unavailable", reason };
}

/**
 * E04.1/E04.3：复用消息端点同一目录（同一 probe/增量/重建/single-flight 状态机），
 * 命中有效目录时只读固定规模聚合——全文件读取 0、JSONL 解析 0、无页面正文 hydrate、
 * 不遍历全部 Run/taskId。冷目录借用 B 同一构建 single-flight，不建第二份目录。
 */
export async function readSessionHistoryOverview(
  input: ReadSessionHistoryOverviewInput,
): Promise<HistoryOverviewResult> {
  const { engine, cache } = input;
  const ctx = await captureReadContext(engine, {
    sessionPath: input.sessionPath,
    sessionId: input.sessionId,
    studioId: input.studioId ?? null,
  });
  if (!ctx) return overviewUnavailable("revision_unknown"); // stat 失败/身份不可靠（P06/I08）

  const key = historyDirectoryCacheKey(ctx);
  let directory: HistoryDirectory | null = null;

  if (!input.disableCache) {
    const hit = cache.get(key);
    if (hit) {
      const verdict = cache.probe(key, ctx);
      if (verdict === "valid") {
        directory = hit.directory;
      } else if (verdict === "append_candidate") {
        // 与页面路径同一增量构建器（C03）：计数随增量维护，不重建全目录。
        const incremental = await buildHistoryDirectoryIncremental({
          old: hit.directory,
          sessionPath: input.sessionPath,
          ctx,
        });
        if (incremental.kind === "ok") {
          const lease = await cache.beginBuild(key);
          if (lease) {
            incremental.directory.session.mutationEpochAtBuild = sessionFileMutationEpoch(ctx.sessionPath);
            if (cache.publish(key, lease, incremental.directory, { released: () => {}, locatorPath: ctx.locatorPath })) {
              directory = incremental.directory;
            }
          } else {
            cache.invalidate(key, "snapshot_changed");
          }
        } else {
          log.warn(
            `overview incremental failed session=${input.sessionId ?? "path"} reason=${incremental.reason} → rebuild`,
          );
          cache.invalidate(key, "snapshot_changed");
        }
      } else if (verdict === "branch_view_stale") {
        const rebuilt = rebuildBranchView(hit.directory, ctx);
        if (rebuilt.kind === "ok") {
          directory = rebuilt.directory; // 概览按新分支前缀重算，不返回旧分支统计
          cache.noteBranchViewRebuild();
        } else {
          cache.invalidate(key, rebuilt.reason ?? "snapshot_changed");
        }
      } else {
        // 失效原因（probe 已 invalidate）→ 走下方全量重建一次（O(N) 允许）。
      }
    }
  }

  if (!directory) {
    const lease = await cache.beginBuild(key);
    if (!lease) return overviewUnavailable("directory_unavailable"); // 超预算 no-cache（B04）
    try {
      const scan = await scanHistoryFile(input.sessionPath, { capturedLength: ctx.fileIdentity.size });
      const built = await buildHistoryDirectory(scan, ctx);
      if (!built.directory) {
        const refusal = built as { reason: InvalidationReason };
        cache.failBuild(key, lease);
        cache.invalidate(key, refusal.reason);
        return overviewUnavailable(mapOverviewUnavailableReason(refusal.reason));
      }
      built.directory.session.mutationEpochAtBuild = sessionFileMutationEpoch(ctx.sessionPath);
      cache.publish(key, lease, built.directory, {
        released: built.released,
        locatorPath: ctx.locatorPath,
      });
      // 发布被并发拒绝时本请求仍可使用刚构建的目录（未发布不等于不可用）。
      directory = built.directory;
    } catch (error) {
      cache.failBuild(key, lease);
      log.error(`overview build failed session=${input.sessionId ?? "path"}: ${error?.message || error}`);
      return overviewUnavailable("directory_unavailable");
    }
  }

  // revision=null 不提供精确计数（E04.3）。
  const revision = directory.session.publicRevision ?? null;
  if (!revision) return overviewUnavailable("revision_unknown");

  const displayRecords = directory.session.displayTotal;
  const sourceRecords = directory.records.length;
  const counts = directory.overview.taskCategoryCounts;
  const overview: HistoryOverview = {
    schemaVersion: 1,
    available: true,
    sessionId: input.sessionId,
    revision,
    counts: {
      displayRecords,
      sourceRecords,
      runsWithAssistant: directory.overview.runsWithAssistant,
      referencedTasks:
        counts.subagent + counts.workflow + counts.media + counts.other,
    },
    runSizeDistribution: { ...directory.overview.runBuckets },
    taskDistribution: { ...counts },
    pagination: {
      legacyDefaultLimit: 50,
      recommendedLimit: HISTORY_PROTOCOL_PAGE_LIMIT,
      maxLimit: 200,
      estimatedPagesAtRecommendedLimit:
        displayRecords > 0 ? Math.ceil(displayRecords / HISTORY_PROTOCOL_PAGE_LIMIT) : 0,
    },
  };
  return { kind: "ok", overview };
}
