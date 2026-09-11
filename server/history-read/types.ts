/**
 * 历史读取 —— 目录数据结构（阶段 B / B02+B03）
 *
 * 目录只是可丢弃、可重建的派生索引，不是第二份会话事实源（TASKBOOK §1）。
 * 红线自查：这里没有任何正文 / 工具参数与输出 / thinking 全文 / base64 /
 * parsed block / collectToolOutcomesByCallId 完整 Map / sourceMessages 数组 /
 * parsed entries——只有小型定位与语义事实；长文本一律存原记录引用
 * （*RecordSourceIndex 指针），按页临时读取（B03、I11）。
 */

/** B06 失效/回退原因枚举（正常追加在 B 阶段表现为 snapshot_changed → 重建）。 */
export type InvalidationReason =
  | "revision_unknown"
  | "file_identity_changed"
  | "branch_changed"
  | "locator_changed"
  | "untrusted_mutation"
  | "directory_invalid"
  | "short_read"
  | "tail_incomplete"
  | "budget_exceeded"
  | "legacy_fallback"
  | "snapshot_changed";

/** 11 项枚举的权威清单（测试/日志/诊断共用，防止枚举缺项）。 */
export const INVALIDATION_REASONS: InvalidationReason[] = [
  "revision_unknown",
  "file_identity_changed",
  "branch_changed",
  "locator_changed",
  "untrusted_mutation",
  "directory_invalid",
  "short_read",
  "tail_incomplete",
  "budget_exceeded",
  "legacy_fallback",
  "snapshot_changed",
];

/**
 * C01 变化分类的 probe 判定（在 InvalidationReason 之外新增两个非失效判定）：
 *  - "append_candidate"：身份不变 + size 增长 + 变更世代未变 → 可信追加（C03 增量续读）；
 *  - "branch_view_stale"：文件不变（或可信追加同时）head 三态字段变化 → 保留物理索引，
 *    按现有 head 规则重建分支视图。
 * 两者都不是失效原因：probe 返回它们时目录保留、不 invalidate。
 */
export type HistoryProbeVerdict = "valid" | "append_candidate" | "branch_view_stale" | InvalidationReason;

/** 原始字节定位（永远属于原文件；不把投影后长度当文件坐标，B02）。 */
export interface PhysicalLocation {
  byteOffset: number;
  byteLength: number;
}

/** 内部文件校验信息（B04 probe 五元组的基础；Windows 缺 dev/ino 时降级并记录）。 */
export interface HistoryReadFileIdentity {
  dev?: number;
  ino?: number;
  size: number;
  mtimeMs: number;
  ctimeMs?: number;
}

/** 持久化 head 行的目录内投影（I05：行存在性与 leafId=null 分别记录）。 */
export interface HistoryBranchHeadRow {
  leafId: string | null;
  observedTailLeafId?: string | null;
  reason?: string | null;
}

/**
 * 读取上下文（结构定义先行；captureReadContext 在 B04/Batch 4 落地并填充）。
 * 业务身份与授权先于缓存（I01）：本结构只承载已授权请求解析出的事实。
 */
export interface HistoryReadContext {
  sessionPath: string;
  sessionId: string | null;
  runtimeId?: string | null;
  studioId?: string | null;
  /** manifest.currentLocator?.path；与 sessionPath 相等时 correlation 启用（同 loadSessionHistoryMessages 规则）。 */
  locatorPath: string | null;
  /** 读取内容前捕获的公开 revision `${size}:${mtimeMs}`；null = 修订未知（I07/I08，不建目录）。 */
  publicRevision: string | null;
  fileIdentity: HistoryReadFileIdentity;
  /** head 行三态（I05）：rowExists=false 无行（legacy）；row.leafId=null 显式空选择；否则正常行。 */
  branchHeadRowExists: boolean;
  branchHeadRow: HistoryBranchHeadRow | null;
  /**
   * head 幂等写回（目录恢复职责，语义镜像 core/session-branch-head.ts
   * readManifestSessionBranch 的 persistRecovery：仅当 !headMatches 时写，
   * reason ∈ append_recovery / branch_read_observe_tail / branch_read_legacy_backfill）。
   * 不需要存活 SessionManager；Batch 4 由 captureReadContext 用 manifest store 接线。
   */
  persistBranchHead?: (state: { leafId: string | null; observedTailLeafId: string | null; reason: string }) => unknown;
}

// ── 扫描（§1.3）──

/**
 * 定位读取注入点（测试用；生产用默认 fs 位置读）。镜像 FileHandle.read 语义：
 * 返回实际 bytesRead（可能小于 length）；0 = position 处 EOF。
 */
export type HistoryReadHook = (
  buffer: Buffer,
  offset: number,
  length: number,
  position: number,
) => Promise<number>;

/** 物理条目：字节坐标 + 记录身份。entries[i] 与 physical[i] 一一对应。 */
export interface HistoryPhysicalEntry {
  physicalIndex: number;
  entryId: string | null;
  byteOffset: number;
  /** 不含行分隔符（\n 或 \r\n）的字节长度；projectOversizedSessionEntry 投影不改变该值。 */
  byteLength: number;
  /** 1 = \n，2 = \r\n；末条无换行 = 0。 */
  separatorLength: number;
  type: string | null;
  /** 该行超过 maxLineBytes，已在内存套用 projectOversizedSessionEntry（hanaRepair 元数据）。 */
  oversized: boolean;
}

export interface HistoryScanBounds {
  /** 本次捕获的文件长度（读前 stat.size）。 */
  capturedLength: number;
  /** 本次实际观察到的字节终点（可能小于 capturedLength：捕获后文件变短属 B06 校验范围）。 */
  observedFileSize: number;
  /** 已安全处理的记录边界（pendingTail 存在时 = 其起点）。 */
  indexedThroughOffset: number;
  /** 未完成尾记录起点（截断 JSON / 不完整 UTF-8 尾）；没有则为 null（I09）。 */
  pendingTailOffset: number | null;
  /** 合法但无末尾换行的末条定位；没有则为 null。 */
  lastUndelimitedRow: { offset: number; length: number } | null;
}

export interface HistoryScanError {
  /** 已完成位置行级 JSON.parse 失败：不建目录，走既有 legacy 链路（含 repair 语义）。 */
  code: "corrupt_record";
  /** 该行将占据的物理下标（= 当时 entries.length）。 */
  physicalIndex: number;
  message?: string;
}

export interface HistoryScanResult {
  /** 临时对象：逐行 JSON.parse（超限行内存投影）产物，含 session 头条目；build 结束即丢弃，不进目录。 */
  entries: any[];
  physical: HistoryPhysicalEntry[];
  bounds: HistoryScanBounds;
  header: { sdkSessionId: string; version: number } | null;
  error: HistoryScanError | null;
}

// ── 记录级（§3；scanner 产出 before 状态子集，build 补齐关联/Run 字段）──

export interface HistoryRecordFact {
  sourceIndex: number;
  entryId: string | null;
  role: string | null;
  customType?: string;
  /**
   * E04：本记录由既有权威解析器（parseHistoryDeferredResult）识别出的任务引用。
   * 仅含 taskId 的记录携带（稀疏字段）；type 为该记录自身的任务元数据类型
   * （subagent/workflow/image-generation/video-generation/…），类别映射在概览聚合层。
   */
  deferredTaskRef?: { taskId: string; type: string };
  /** isDisplayableHistoryMessage(message)（Reminder 剥离前的原始消息判定，易碎点 1）。 */
  visible: boolean;
  /** 该记录自身的 display 序号；仅可见 user/assistant 有，其余为 null。 */
  displayIndex: number | null;
  /** 进入该记录时主循环的 displayIdx（toolResult/custom 锚点 afterIndex = 该值 - 1，易碎点 7）。 */
  displayIndexBefore: number;
  turnInputEntryIdBefore: string | null;
  turnInputVisibleBefore: boolean;
  /** 不可见 assistant 也已推大（先加后判，易碎点 2）。 */
  assistantOrdinalBefore: number;
  toolCallIds?: string[];
  toolName?: string;
  isError?: boolean;
  runOrdinal?: number;
  turnStartIndex?: number;
  turnEndIndex?: number;
  timestamp?: string;
}

// ── 目录（§3）──

export interface HistoryDirectoryKey {
  runtimeId: string;
  studioId: string;
  /** 无业务 id 时为 null，走 path 命名空间（B04）。 */
  sessionId: string | null;
  normalizedPath: string;
  fileIdentity: HistoryReadFileIdentity;
}

export interface HistoryFileIndex {
  observedFileSize: number;
  indexedThroughOffset: number;
  pendingTailOffset: number | null;
  lastUndelimitedRow: { offset: number; length: number } | null;
  header: { sdkSessionId: string; version: number } | null;
  /** entryId → 物理下标（分支投影成功后 id 唯一）；字节位置按物理下标查 byteOffsets/byteLengths。 */
  byEntryId: Map<string, number>;
  /**
   * 按物理下标平铺的字节位置（C05 紧凑化：不再逐条建位置对象，20k 物理条目省
   * ~2.2MB 估算驻留；SegmentedList base 段跨增量版本共享，追加段 O(新增)）。
   */
  byteOffsets: SegmentedList<number>;
  byteLengths: SegmentedList<number>;
  physicalCount: number;
}

/**
 * 分段追加列表（C03 版本隔离）：base 段与旧版本共享只读，追加段有界。
 * 提供 at/length/slice/迭代/map/find —— 数组面兼容既有消费者。
 */
export class SegmentedList<T> {
  segments: T[][];
  offsets: number[];

  constructor(segments: T[][]) {
    this.segments = segments.filter((seg) => seg.length > 0);
    this.offsets = [];
    let offset = 0;
    for (const seg of this.segments) {
      this.offsets.push(offset);
      offset += seg.length;
    }
  }

  get length(): number {
    return this.offsets.length
      ? this.offsets[this.offsets.length - 1] + this.segments[this.segments.length - 1].length
      : 0;
  }

  at(index: number): T | undefined {
    if (index < 0 || index >= this.length) return undefined;
    let lo = 0;
    let hi = this.offsets.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (this.offsets[mid] <= index) lo = mid;
      else hi = mid - 1;
    }
    return this.segments[lo][index - this.offsets[lo]];
  }

  slice(start?: number, end?: number): T[] {
    const lo = start ?? 0;
    const hi = end ?? this.length;
    const out: T[] = [];
    for (let i = Math.max(0, lo); i < Math.min(hi, this.length); i += 1) out.push(this.at(i)!);
    return out;
  }

  find(predicate: (item: T, index: number) => unknown): T | undefined {
    let i = 0;
    for (const item of this) {
      if (predicate(item, i)) return item;
      i += 1;
    }
    return undefined;
  }

  map<U>(fn: (item: T, index: number) => U): U[] {
    const out: U[] = [];
    let i = 0;
    for (const item of this) out.push(fn(item, i++));
    return out;
  }

  *[Symbol.iterator](): Iterator<T> {
    for (const seg of this.segments) yield* seg;
  }
}

/** C03 记录列表别名。 */
export const HistoryRecordList = SegmentedList;
export type HistoryRecordList = SegmentedList<HistoryRecordFact>;

/**
 * Map 追加 overlay（C03）：base 只读共享，delta 有界（仅新增/变化键）。
 * 扩展 Map —— 类型上与 Map<K,V> 完全兼容，消费方（page/index/hydrate）无需改动。
 */
export class MapOverlay<K, V> extends Map<K, V> {
  base: Map<K, V>;
  delta: Map<K, V> = new Map();

  constructor(base: Map<K, V>) {
    super();
    this.base = base;
  }

  override get size(): number {
    let visible = 0;
    for (const key of this.base.keys()) if (!this.delta.has(key)) visible += 1;
    return visible + this.delta.size;
  }

  override get(key: K): V | undefined {
    return this.delta.has(key) ? this.delta.get(key) : this.base.get(key);
  }

  override has(key: K): boolean {
    return this.delta.has(key) || this.base.has(key);
  }

  override set(key: K, value: V): this {
    this.delta.set(key, value);
    return this;
  }

  override delete(key: K): boolean {
    return this.delta.delete(key);
  }

  override clear(): void {
    this.delta.clear();
  }

  override forEach(callback: (value: V, key: K, map: Map<K, V>) => void, thisArg?: unknown): void {
    for (const [key, value] of this) callback.call(thisArg, value, key, this);
  }

  override *keys(): MapIterator<K> {
    const shadowed = new Set(this.delta.keys());
    for (const key of this.base.keys()) if (!shadowed.has(key)) yield key;
    yield* this.delta.keys();
  }

  override *values(): MapIterator<V> {
    for (const [_, value] of this) yield value;
  }

  override *entries(): MapIterator<[K, V]> {
    for (const key of this.keys()) yield [key, this.get(key) as V];
  }

  override [Symbol.iterator](): MapIterator<[K, V]> {
    return this.entries();
  }
}

export interface HistoryBranchIndex {
  headRowExists: boolean;
  persistedLeafId: string | null;
  observedTailLeafId: string | null;
  selectedLeafId: string | null;
  physicalTailLeafId: string | null;
  headResolution: "legacy_tail" | "persisted_head" | "append_recovery";
  /** root→leaf，仅 id（lineageHash 留在投影层按需计算，不复制完整 hash 逻辑）。 */
  lineageEntryIds: SegmentedList<string>;
}

export interface HistoryAssociationIndex {
  toolResultByCallId: Map<string, { sourceIndex: number }>;
  correlationByUserEntryId: Map<string, {
    clientMessageId?: string;
    snapshotVersion?: number;
    sourceEntryId?: string;
    acceptanceDiagnostic?: "ambiguous";
  }>;
  turnInputByAssistantEntryId: Map<string, string>;
  /**
   * turn-input consumption 记录按其 assistant entryId 的反向索引（构建期纯关联，
   * 不复制锚点公式——锚点数值仍由投影器现有代码现算）。热页据此把「锚在窗口内
   * assistant、记录自身在页外」的 consumption interlude 记录纳入迭代/读取闭包。
   */
  consumptionByAssistantEntryId: Map<string, number[]>;
  /** 排序列表 + 二分（页请求水化为 Set，§5 #7）。 */
  consumptionDeliveryIds: SegmentedList<string>;
  consumptionEntryIds: SegmentedList<string>;
  collabDecisionBySuggestionId: Map<string, { status?: string; resultSessionId?: string }>;
  modelCallRefBySourceIndex: Map<number, { modelCallId: string; traceId: string | null; parentCallId: string | null }>;
  /** 指针表：页首命中时补读对应 custom 记录（§5 #9），不驻留 origin/displayText 正文。 */
  originBySourceIndex: Map<number, { originRecordSourceIndex: number }>;
  presentationBySourceIndex: Map<number, { presentationRecordSourceIndex: number }>;
  agentReviewBySourceIndex: Map<number, { reviewRecordSourceIndex: number; completed: boolean }>;
  /** anchorAfterIndex 升序；afterIndex null = 会话尾部无后继 assistant。 */
  deferredInterludeAnchors: SegmentedList<{ anchorAfterIndex: number | null; sourceIndex: number; deliveryId: string | null }>;
  mediaResultRecords: SegmentedList<{ sourceIndex: number; taskId: string; success: boolean }>;
  /** 当前分支「最新合法快照」指针（extractLatestTodoSnapshot 选中的那条记录；§5 #1）。 */
  todoSnapshot: { sourceIndex: number } | null;
  /**
   * Run 边界小表（C03：runOrdinal → {start,end}）。记录级只存 runOrdinal；
   * 追加延续开放 Run 时 copy-on-write 单个表项（O(1)/追加，不 O(Run) 改写）。
   */
  runBoundsByOrdinal: Map<number, { start: number; end: number }>;
  /** clientMessageId → 引用过的 sourceEntryId 集合（C04 X12：跨追加 ambiguous 判定）。 */
  clientMessageIdEntries: Map<string, Set<string>>;
}

export interface HistorySessionFacts {
  displayTotal: number;
  /** 构建前 stat 的 `${size}:${mtimeMs}`（I07）；null = 修订未知（不发布目录）。 */
  publicRevision: string | null;
  fileIdentity: HistoryReadFileIdentity;
  /**
   * 目录发布时捕获的进程内变更世代（C02）：此后任何已插桩重写/修复/生命周期
   * 写入都会递增该路径 epoch；probe 比较不等即 `untrusted_mutation` 全量重建。
   * 追加路径不递增，故「身份不变 + size 增长 + epoch 未变」= 可信追加候选。
   */
  mutationEpochAtBuild?: number;
  /**
   * 尾部指针/计数终态（C03 增量 seed）：全量构建/上次增量完成时的指针状态
   * （scanner 终值），增量扫描据此延续指针机，不重放旧记录。
   */
  tailPointerEntryId?: string | null;
  tailPointerVisible?: boolean;
  tailAssistantOrdinal?: number;
  tailRunOrdinal?: number;
  /**
   * 当前分支活跃文件引用身份集合（§5 #2，供 registry listReachable 的
   * referenceIdentities 入参）。Batch 2 未接线：collectSessionFileReferenceIdentities
   * 尚未从 registry 导出（Batch 3 随 listReachable 入参一并开放），此处恒为 null，
   * 热页在接线前继续走原全量引用路径。
   */
  activeFileReferenceIdentities: Set<string> | null;
  originPresentationPayloadInline?: boolean;
}

/** E04：任务类别（映射自任务自身元数据 type；无可信类别归 other）。 */
export type HistoryTaskCategory = "subagent" | "workflow" | "media" | "other";

/**
 * E04 会话概览计数（分支作用域；目录/增量构建器维护）。
 * runSizeByOrdinal/taskCategories 为 copy-on-write overlay（增量 delta 有界）；
 * 计数器为普通数字（新版本直接派生）。热路径概览只读计数器，不遍历 Run/taskId。
 */
export interface HistoryDirectoryOverview {
  /** Run ordinal → 该 Run 内 displayable assistant 数（仅 ≥1 的 Run 有条目）。 */
  runSizeByOrdinal: Map<number, number>;
  /** 三桶计数，合计 = runsWithAssistant。 */
  runBuckets: { oneTo50: number; from51To200: number; over200: number };
  runsWithAssistant: number;
  /** taskId → 类别（分支序首次分类胜出；不同 taskId 去重）。 */
  taskCategories: Map<string, HistoryTaskCategory>;
  taskCategoryCounts: { subagent: number; workflow: number; media: number; other: number };
}

export interface HistoryDirectory {
  version: 1;
  key: HistoryDirectoryKey;
  file: HistoryFileIndex;
  branch: HistoryBranchIndex;
  /** 按分支序（= sourceIndex 序）的记录级事实（C03 分段容器，base 段跨版本共享）。 */
  records: SegmentedList<HistoryRecordFact>;
  /** 升序，供窗口二分（O(log N + K)，7.1 禁止每页全量 filter）。 */
  displayableSourceIndexes: SegmentedList<number>;
  /** afterIndex 升序的块锚点（toolResult / display!==false 的 custom）。 */
  blockAnchorByAfterIndex: SegmentedList<{ afterIndex: number; sourceIndex: number }>;
  assoc: HistoryAssociationIndex;
  session: HistorySessionFacts;
  /** E04 会话概览计数（分支作用域，目录/增量构建器维护；热路径 O(1) 读取）。 */
  overview: HistoryDirectoryOverview;
  /** measure() 保守估算（string×系数、Map/数组条目成本）；非 JSON.stringify 口径。 */
  measuredBytes: number;
}
