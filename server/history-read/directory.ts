/**
 * 历史读取 —— 目录构建（阶段 B / B03）
 *
 * 冷构建单次全量元数据遍历：分支选择唯一实现 projectCurrentSessionBranchEntries
 * （本层不做第二份 lineage/hash 判定，I05）；head 恢复职责镜像
 * core/session-branch-head.ts readManifestSessionBranch 的 persistRecovery 规则经
 * ctx.persistBranchHead 幂等写回（不为此打开全量 SessionManager）；消息投影复用
 * Batch 1 的 projectBranchHistory + buildProjectionContextFromMessages（同一 scanner，
 * I04），再把上下文压缩为小型定位/语义事实。
 *
 * 红线（B03）：目录不驻留正文、工具参数/输出、thinking 全文、完整 review/presentation
 * 对象、parsed block、媒体内容、sourceMessages 数组或 parsed entries——只有指针表
 * （*RecordSourceIndex）与小型关联事实。
 */
import path from "path";
import { projectCurrentSessionBranchEntries, SessionBranchError } from "../../lib/session-jsonl.ts";
import { projectBranchHistory } from "../../core/message-utils.ts";
import { collectDesktopInputCorrelations } from "../../core/desktop-input-correlation.ts";
import { TURN_INPUT_CONSUMPTION_EVENT_TYPE, parseTurnInputConsumptionRecord } from "../../lib/turn-input-presentation.ts";
import { collectSessionFileReferenceIdentities } from "../../lib/session-files/session-file-registry.ts";
import { buildProjectionContextFromMessages, type ProjectionContext } from "./projection-context.ts";
import { SegmentedList } from "./types.ts";
import { computeOverviewFromRecords } from "./overview.ts";
import { DESKTOP_INPUT_CORRELATION_TYPE } from "../../core/desktop-input-correlation.ts";
import type {
  HistoryAssociationIndex,
  HistoryBranchIndex,
  HistoryDirectory,
  HistoryDirectoryKey,
  HistoryFileIndex,
  HistoryReadContext,
  HistoryRecordFact,
  HistoryScanResult,
  HistorySessionFacts,
  InvalidationReason,
} from "./types.ts";

export const HISTORY_DIRECTORY_VERSION = 1;
/** 单目录准入上限；超限该会话走无缓存只读路径，不截断历史（§6）。 */
export const MAX_SINGLE_DIRECTORY_BYTES = 16 * 1024 * 1024;

export interface BuildHistoryDirectoryOptions {
  /** DI/测试用预算覆盖；默认 MAX_SINGLE_DIRECTORY_BYTES。 */
  maxDirectoryBytes?: number;
}

export type BuildHistoryDirectoryResult =
  | {
      directory: HistoryDirectory;
      context: ProjectionContext;
      released: () => void;
      /** 本次构建执行的 head 恢复写回（若有）：读后复核的期望 after 状态（C03）。 */
      headWriteBack: { leafId: string | null; observedTailLeafId: string | null; reason: string } | null;
    }
  | { directory: null; reason: InvalidationReason; detail?: string; context?: undefined; released?: undefined; headWriteBack?: undefined };

/** 与 core/session-branch-head.ts headMatches 同语义（该函数未导出；两处必须同步改）。 */
function headMatches(head: any, recommendation: { leafId: string | null; observedTailLeafId: string | null }): boolean {
  return !!head
    && (head.leafId ?? null) === (recommendation.leafId ?? null)
    && (head.observedTailLeafId ?? null) === (recommendation.observedTailLeafId ?? null);
}

/**
 * 保守驻留估算：string 值按字节×2（保守覆盖 UTF-16）；Map/Set/数组按条目常数 +
 * 元素递归；对象按 64B 头 + 8B/字段槽 + 值递归——属性名字符串不逐次计入（V8 对
 * 同形对象共享/intern 属性名，逐次计会数倍高估，已按实际驻留校准，§6）。
 * 不是 JSON.stringify 长度，也不冒充 heap 精确值。
 */
export function measureDirectoryBytes(value: any, seen: WeakSet<object> = new WeakSet()): number {
  if (value == null) return 0;
  const type = typeof value;
  if (type === "string") return 16 + Buffer.byteLength(value, "utf8") * 2;
  if (type === "number" || type === "boolean" || type === "bigint") return 8;
  if (type !== "object" && type !== "function") return 0;
  if (seen.has(value)) return 0;
  seen.add(value);
  let total = 64;
  if (value instanceof Map) {
    total += 128 + value.size * 32;
    for (const [k, v] of value) total += measureDirectoryBytes(k, seen) + measureDirectoryBytes(v, seen);
  } else if (value instanceof Set) {
    total += 128 + value.size * 32;
    for (const v of value) total += measureDirectoryBytes(v, seen);
  } else if (Array.isArray(value)) {
    total += value.length * 8;
    for (const v of value) total += measureDirectoryBytes(v, seen);
  } else if (ArrayBuffer.isView(value)) {
    total += (value as any).byteLength ?? 0;
  } else {
    // for-in 计字段槽：避免每对象 keys/values 数组分配（20k 记录规模下的 GC 热点）
    let fields = 0;
    for (const key in value) {
      fields += 1;
      total += measureDirectoryBytes(value[key], seen);
    }
    total += fields * 8;
  }
  return total;
}

/**
 * B08 冷构建优化说明（历史）：records/byEntryId/lineage 曾有专用估算公式，
 * 现全量与增量统一走 measureDirectoryBytes（口径单一，§6 校准）。
 */

export async function buildHistoryDirectory(
  scan: HistoryScanResult,
  ctx: HistoryReadContext,
  opts: BuildHistoryDirectoryOptions = {},
): Promise<BuildHistoryDirectoryResult> {
  const maxDirectoryBytes = opts.maxDirectoryBytes ?? MAX_SINGLE_DIRECTORY_BYTES;

  // I08：revision 未知 → 不建目录、不写缓存。
  if (ctx.publicRevision == null) {
    return { directory: null, reason: "revision_unknown" };
  }
  // I08 之外的真实读取完整性（X05/X07）：捕获了 capturedLength 字节却只观察到更少
  // 字节 = 读取途中异常 EOF——不建立目录（不发布不完整视图，P11 不伪造空历史）。
  if (scan.bounds.observedFileSize < scan.bounds.capturedLength) {
    return { directory: null, reason: "short_read" };
  }
  if (scan.error) {
    return { directory: null, reason: "directory_invalid", detail: scan.error.code };
  }

  // 分支选择唯一实现（I05）：branchHead 行存在性与 leafId=null 语义由 ctx 区分，
  // projectCurrentSessionBranchEntries 内部区分 legacy_tail / persisted_head / append_recovery。
  const branchHeadRow = ctx.branchHeadRowExists ? (ctx.branchHeadRow ?? { leafId: null }) : null;
  let projection: any;
  try {
    projection = projectCurrentSessionBranchEntries(scan.entries, {
      branchHead: branchHeadRow,
      filePath: ctx.sessionPath,
      // 目录只消费 lineage 的 id 列表（hash 不进目录，分支身份以 head 行三态为准）：
      // 跳过逐条 JSON.stringify + SHA-256 链（B08 冷构建优化，默认调用方行为不变）。
      lineageHash: false,
    });
  } catch (err) {
    if (err instanceof SessionBranchError) {
      // 拒绝码与严格层一致（X01）：duplicate_id / cycle / dangling_parent / invalid_id /
      // invalid_json / head_missing 原样透传，目录一律不建立。
      return { directory: null, reason: "directory_invalid", detail: err.code };
    }
    throw err;
  }
  if (projection.legacySyntheticIds) {
    // 无可证明分支（id-less legacy）：不建目录，保守走 legacy_fallback。
    return { directory: null, reason: "legacy_fallback" };
  }

  // head 幂等写回（恢复职责不取消，但不为取令牌打开全量 SessionManager）。
  // reason 沿用 readManifestSessionBranch 的现有枚举：
  //   append_recovery / branch_read_observe_tail / branch_read_legacy_backfill。
  let headWriteBack: { leafId: string | null; observedTailLeafId: string | null; reason: string } | null = null;
  if (ctx.persistBranchHead && !headMatches(branchHeadRow, projection.recommendedHead)) {
    headWriteBack = {
      leafId: projection.recommendedHead.leafId ?? null,
      observedTailLeafId: projection.recommendedHead.observedTailLeafId ?? null,
      reason: projection.headResolution === "append_recovery"
        ? "append_recovery"
        : branchHeadRow
          ? "branch_read_observe_tail"
          : "branch_read_legacy_backfill",
    };
    ctx.persistBranchHead(headWriteBack);
  }

  // 分支条目（root→leaf 原始 entry）：与 loadSessionHistoryEvidence 的消费方式一致，
  // lineage 只携带 id，原条目经 byId 取回。
  const byId = new Map(scan.entries.filter((entry) => entry?.id != null).map((entry) => [entry.id, entry]));
  const branchEntries = projection.lineage.map((lineageEntry: any) => byId.get(lineageEntry.id));

  // correlation 启用规则与 loadSessionHistoryMessages 同源：locator 绑定当前路径才传业务 sessionId。
  const correlationSessionId = ctx.locatorPath != null && ctx.locatorPath === ctx.sessionPath
    ? ctx.sessionId
    : "";
  const sourceMessages = projectBranchHistory(branchEntries, correlationSessionId);
  const context = buildProjectionContextFromMessages(sourceMessages);

  // ── 压缩为目录事实 ──
  // 记录级关联/Run 字段已由 scanner finalize 统一补齐（与无缓存全量路径同一事实
  // 形状，I04）；directory 以分段容器接管 recordFacts 数组（C03 增量追加共享 base 段，
  // released() 清 context 引用键，数组本体由容器持有）。
  const records = new SegmentedList([context.recordFacts]);
  // E04：概览计数冷构建一次重算（O(N)，仅冷路径允许）。
  const overview = computeOverviewFromRecords(context.recordFacts);

  const displayableFlat: number[] = [];
  const blockAnchorFlat: Array<{ afterIndex: number; sourceIndex: number }> = [];
  for (let sourceIndex = 0; sourceIndex < records.length; sourceIndex += 1) {
    const fact = records.at(sourceIndex)!;
    if (fact.displayIndex != null) displayableFlat.push(fact.sourceIndex);
    const isBlockAnchor = fact.role === "toolResult"
      || (fact.role === "custom" && sourceMessages[sourceIndex]?.display !== false);
    if (!isBlockAnchor) continue;
    const afterIndex = fact.displayIndexBefore - 1;
    if (afterIndex >= 0) blockAnchorFlat.push({ afterIndex, sourceIndex });
  }

  const toolResultByCallId = new Map<string, { sourceIndex: number }>();
  for (const [callId, sourceIndex] of context.toolResultSourceIndexByCallId) {
    toolResultByCallId.set(callId, { sourceIndex });
  }

  const correlationByUserEntryId = new Map();
  for (const [entryId, value] of collectDesktopInputCorrelations(branchEntries, correlationSessionId)) {
    correlationByUserEntryId.set(entryId, {
      ...(typeof value?.clientMessageId === "string" ? { clientMessageId: value.clientMessageId } : {}),
      ...(Number.isSafeInteger(value?.snapshotVersion) ? { snapshotVersion: value.snapshotVersion } : {}),
      ...(typeof value?.sourceEntryId === "string" ? { sourceEntryId: value.sourceEntryId } : {}),
      ...(value?.acceptanceDiagnostic === "ambiguous" ? { acceptanceDiagnostic: "ambiguous" } : {}),
    });
  }

  // C03：Run 边界小表（runOrdinal → {start,end}），增量追加 copy-on-write 单表项。
  const runBoundsByOrdinal = new Map<number, { start: number; end: number }>();
  for (const fact of context.recordFacts) {
    if (fact.runOrdinal != null && fact.turnStartIndex != null && !runBoundsByOrdinal.has(fact.runOrdinal)) {
      runBoundsByOrdinal.set(fact.runOrdinal, { start: fact.turnStartIndex, end: fact.turnEndIndex ?? fact.turnStartIndex });
    }
  }
  const recordFacts = context.recordFacts;

  const collabDecisionBySuggestionId = new Map<string, { status?: string; resultSessionId?: string }>();
  for (const [suggestionId, decision] of context.collabDecisionsBySuggestionId) {
    collabDecisionBySuggestionId.set(suggestionId, {
      ...(typeof decision?.status === "string" ? { status: decision.status } : {}),
      ...(typeof decision?.resultSessionId === "string" ? { resultSessionId: decision.resultSessionId } : {}),
    });
  }

  // turn-input consumption 记录的反向索引（纯关联）：consumption interlude 的块锚
  // 指向其 assistant（锚点数值由投影器现算），热页解析窗口闭包需要
  // 「assistant entryId → 页外 consumption 记录」这层定位。
  const consumptionByAssistantEntryId = new Map<string, number[]>();
  for (let sourceIndex = 0; sourceIndex < sourceMessages.length; sourceIndex += 1) {
    const message = sourceMessages[sourceIndex];
    if (message?.role !== "custom" || message.customType !== TURN_INPUT_CONSUMPTION_EVENT_TYPE) continue;
    const parsed = parseTurnInputConsumptionRecord(message.data);
    const assistantEntryId = typeof parsed?.assistant?.entryId === "string" && parsed.assistant.entryId.trim()
      ? parsed.assistant.entryId.trim()
      : null;
    if (!assistantEntryId) continue;
    const list = consumptionByAssistantEntryId.get(assistantEntryId) || [];
    list.push(sourceIndex);
    consumptionByAssistantEntryId.set(assistantEntryId, list);
  }

  const originBySourceIndex = new Map<number, { originRecordSourceIndex: number }>();
  for (const [sourceIndex, recordSourceIndex] of context.originRecordSourceIndexByUserSourceIndex) {
    originBySourceIndex.set(sourceIndex, { originRecordSourceIndex: recordSourceIndex });
  }
  const presentationBySourceIndex = new Map<number, { presentationRecordSourceIndex: number }>();
  for (const [sourceIndex, recordSourceIndex] of context.presentationRecordSourceIndexByUserSourceIndex) {
    presentationBySourceIndex.set(sourceIndex, { presentationRecordSourceIndex: recordSourceIndex });
  }
  const agentReviewBySourceIndex = new Map<number, { reviewRecordSourceIndex: number; completed: boolean }>();
  for (const [sourceIndex, recordSourceIndex] of context.agentReviewRecordSourceIndexByUserSourceIndex) {
    agentReviewBySourceIndex.set(sourceIndex, { reviewRecordSourceIndex: recordSourceIndex, completed: true });
  }

  const deferredFlat = [...context.deferredInterludeAnchors.values()].map((anchor) => ({
    anchorAfterIndex: anchor.afterIndex,
    sourceIndex: anchor.sourceIndex,
    deliveryId: anchor.deliveryId,
  })).sort((a, b) => {
    if (a.anchorAfterIndex == null && b.anchorAfterIndex == null) return a.sourceIndex - b.sourceIndex;
    if (a.anchorAfterIndex == null) return 1;
    if (b.anchorAfterIndex == null) return -1;
    return (a.anchorAfterIndex - b.anchorAfterIndex) || (a.sourceIndex - b.sourceIndex);
  });
  const deferredInterludeAnchors = new SegmentedList([deferredFlat]);
  const mediaResultRecords = new SegmentedList([context.mediaResultRecords]);

  const assoc: HistoryAssociationIndex = {
    toolResultByCallId,
    correlationByUserEntryId,
    turnInputByAssistantEntryId: context.turnInputByAssistantEntryId,
    consumptionDeliveryIds: new SegmentedList([[...context.turnInputConsumptionDeliveryIds].sort()]),
    consumptionEntryIds: new SegmentedList([[...context.turnInputConsumptionEntryIds].sort()]),
    // C04 X12 跨追加 ambiguous 依据：clientMessageId → 已引用 sourceEntryId 集合
    clientMessageIdEntries: (() => {
      const index = new Map<string, Set<string>>();
      for (const message of sourceMessages) {
        if (message?.role !== "custom" || message.customType !== DESKTOP_INPUT_CORRELATION_TYPE) continue;
        const data = message.data;
        if (data?.schemaVersion !== 1 || typeof data?.clientMessageId !== "string" || !data.clientMessageId) continue;
        const set = index.get(data.clientMessageId) || new Set<string>();
        set.add(typeof data.sourceEntryId === "string" ? data.sourceEntryId : "");
        index.set(data.clientMessageId, set);
      }
      return index;
    })(),
    runBoundsByOrdinal,
    collabDecisionBySuggestionId,
    modelCallRefBySourceIndex: context.modelCallReferenceBySourceIndex,
    originBySourceIndex,
    presentationBySourceIndex,
    agentReviewBySourceIndex,
    consumptionByAssistantEntryId,
    deferredInterludeAnchors,
    mediaResultRecords,
    todoSnapshot: context.todoSnapshotSourceIndex != null
      ? { sourceIndex: context.todoSnapshotSourceIndex }
      : null,
  };

  const file: HistoryFileIndex = {
    observedFileSize: scan.bounds.observedFileSize,
    indexedThroughOffset: scan.bounds.indexedThroughOffset,
    pendingTailOffset: scan.bounds.pendingTailOffset,
    lastUndelimitedRow: scan.bounds.lastUndelimitedRow,
    header: scan.header,
    byEntryId: (() => {
      const map = new Map<string, number>();
      for (const p of scan.physical) {
        if (p.entryId != null) map.set(p.entryId, p.physicalIndex);
      }
      return map;
    })(),
    byteOffsets: new SegmentedList([Array.from(scan.physical, (p) => p.byteOffset)]),
    byteLengths: new SegmentedList([Array.from(scan.physical, (p) => p.byteLength)]),
    physicalCount: scan.physical.length,
  };

  const branch: HistoryBranchIndex = {
    headRowExists: ctx.branchHeadRowExists,
    persistedLeafId: branchHeadRow ? (branchHeadRow.leafId ?? null) : null,
    observedTailLeafId: branchHeadRow?.observedTailLeafId ?? null,
    selectedLeafId: projection.selectedLeafId ?? null,
    physicalTailLeafId: projection.physicalTailLeafId ?? null,
    headResolution: projection.headResolution,
    lineageEntryIds: new SegmentedList([projection.lineage.map((entry: any) => entry.id)]),
  };

  const session: HistorySessionFacts = {
    displayTotal: context.displayTotal,
    publicRevision: ctx.publicRevision,
    fileIdentity: ctx.fileIdentity,
    // 当前分支全历史文件引用身份（§5 #2）：构建期跑现有收集器一次，热页经
    // registry.listReachable 的 referenceIdentities 入参使用（不再每页深扫正文）。
    activeFileReferenceIdentities: collectSessionFileReferenceIdentities(sourceMessages) as Set<string>,
    // C03 增量 seed：扫描终态指针/计数
    tailPointerEntryId: context.finalTurnInputEntryId,
    tailPointerVisible: context.finalTurnInputVisible,
    tailAssistantOrdinal: context.finalAssistantOrdinal,
    tailRunOrdinal: context.finalRunOrdinal,
  };

  const key: HistoryDirectoryKey = {
    runtimeId: ctx.runtimeId || "default",
    studioId: ctx.studioId || "default",
    sessionId: ctx.sessionId,
    normalizedPath: path.resolve(ctx.sessionPath),
    fileIdentity: ctx.fileIdentity,
  };

  const displayableSourceIndexes2 = new SegmentedList([displayableFlat]);
  const blockAnchorByAfterIndex2 = new SegmentedList([blockAnchorFlat]);

  const directory: HistoryDirectory = {
    version: HISTORY_DIRECTORY_VERSION,
    key,
    file,
    branch,
    records,
    displayableSourceIndexes: displayableSourceIndexes2,
    blockAnchorByAfterIndex: blockAnchorByAfterIndex2,
    assoc,
    session,
    overview,
    measuredBytes: 0,
  };
  // B08：records/byEntryId/lineage 为已知扁平结构走专用公式（同口径、保守、免
  // 全图递归与 Set），assoc/锚点/序号数组仍走通用遍历。
  directory.measuredBytes = measureDirectoryBytes(directory);
  if (directory.measuredBytes > maxDirectoryBytes) {
    // 超预算：该会话走无缓存只读路径，不截断历史（§6）。
    return { directory: null, reason: "budget_exceeded" };
  }

  // I11：released 清空构建期临时引用（scan.entries / sourceMessages / branchEntries /
  // byId / context）。目录本身不持有任何正文或 entries 引用。
  let releasedFlag = false;
  const released = () => {
    if (releasedFlag) return;
    releasedFlag = true;
    scan.entries.length = 0;
    sourceMessages.length = 0;
    branchEntries.length = 0;
    byId.clear();
    if (context) Object.keys(context).forEach((k) => delete (context as any)[k]);
  };

  return { directory, context, released, headWriteBack };
}
