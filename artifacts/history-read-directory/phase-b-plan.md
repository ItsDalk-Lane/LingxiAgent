# 阶段 B 实施计划（完整版，由 plan 子代理产出，实施者以此为准）

基线：HEAD `1d42b74…`，`fix/pending-sep10`。前置事实（A 阶段证据钉死）：旧热页每请求 2 次全文件同步读、`jsonlParseCount`=4004/40004、`fullHistoryProjectionCount`=1、`metadataVisitedCount`=N（1k）/10N（10k）；参考输出 7 份已冻结（normalization-rules R1–R5）；A03 合法夹具 sha256 已入 `fixture-audit.json`。

## 1. 模块划分与 B01 抽取清单

新目录 `server/history-read/`，全部为 server 内部模块，不新增公开 HTTP 面。仓库 TS 风格为宽松类型（`.ts` + 少量标注），签名按此写。

### 1.1 `server/history-read/types.ts` — 数据结构（见 §3）

仅导出 interface/type，无逻辑。

### 1.2 `server/history-read/read-context.ts` — `captureReadContext`

```ts
export async function captureReadContext(
  engine: any,
  input: { sessionPath: string; sessionId: string | null; runtimeId: string | null; studioId: string | null },
): Promise<HistoryReadContext | null>
```

- 抽取来源：`server/routes/sessions.ts: readSessionFileRevision`（387）的 stat+`sessionFileRevision` 组合，以及 `resolveSessionCacheLocator`（400）的 manifest/locator 解析。**不移动这两个函数**——route 保留原函数，`captureReadContext` 调用它们（`readSessionFileRevision` 改为从模块导出，route 内调用点不变）。
- 逻辑：①`fs.stat`（读内容**之前**，维持 :1491 的竞态纪律）→ `publicRevision`；stat 失败返回 `null`（I08：不读旧缓存不写新缓存，走既有无缓存路径）。②同一次 stat 取 `dev/ino/size/mtimeMs/ctimeMs` 构成 `internalIdentity`（macOS/Linux 有 dev/ino；Windows 降级为 size+mtime+ctime 并在 `identityFields` 里记录缺哪些字段）。③`engine.getSessionBranchHead(sessionId)` 取 head 行，**区分三种状态**：`{ exists: false }`（无行）/ `{ exists: true, leafId: null }`（显式空选择）/ 正常行（I05）。④`engine.getSessionManifest(sessionId)?.currentLocator?.path` 作 locator 绑定，并决定 correlation 是否启用（复用现规则：`manifest.currentLocator.path === sessionPath` 才传 sessionId，见 `core/message-utils.ts:132`）。
- 不打开 SessionManager、不全读 JSONL（§4 表要求）。

### 1.3 `server/history-read/scan.ts` — `scanHistoryFile`

```ts
export interface ScanResult {
  entries: AnyEntry[];              // 临时对象，build 结束即丢弃，不进目录
  physical: HistoryPhysicalEntry[]; // entryId/byteOffset/byteLength/separatorLength/type
  bounds: {
    observedFileSize: number;
    indexedThroughOffset: number;   // 已安全处理的记录边界
    pendingTailOffset: number | null;
    lastUndelimitedRow: { offset: number; length: number } | null;
  };
  header: { sdkSessionId: string; version: number } | null;
  error: ScanError | null;          // { code, physicalIndex } —— 损坏/重复 id 等
}
export async function scanHistoryFile(
  sessionPath: string,
  opts: { capturedLength: number; startOffset?: number; chunkBytes?: number; readFile?: ReadHook },
): Promise<ScanResult>
```

- 全新实现（B02），唯一新写的读取原语。规则：原始 Buffer 计偏移（禁用 JS 字符串长度）；分块读（默认 256 KiB）+ 跨块 UTF-8 半字符缓冲（`buf.lastIndexOf(0x0A)` 切行，整行字节再 decode）；`bytesRead` 循环补齐；CRLF 记入 `separatorLength`；无尾换行的完整末条记入 `lastUndelimitedRow`；截断 JSON/不完整 UTF-8 尾 → `pendingTailOffset`（不 parse、不进 entries，I09）。
- **不调用 `repairOversizedSessionEntriesInFile`**（B02 红线）。超限行（>1 MiB）在归一化时套用同一纯函数 `projectOversizedSessionEntry`（`core/session-jsonl-file.ts:61`，已导出）在内存投影，并把 `hanaRepair` 元数据按现有 repair 的产物形状附上——保证与“旧路径先 repair 再读”的投影输出逐字段一致（见 §8 风险 1 的验证）。
- 行级 JSON.parse 失败（已完成位置）→ `error.code = "corrupt_record"`，不建立目录（走 legacy 回退）。
- `readFile` hook 参数仅为测试注入短读/中断（X05/X07）。

### 1.4 `server/history-read/directory.ts` — `buildHistoryDirectory`

```ts
export async function buildHistoryDirectory(
  scan: ScanResult,
  ctx: HistoryReadContext,
): Promise<{ directory: HistoryDirectory; released: () => void } | { directory: null; reason: InvalidationReason }>
```

- 流程（单次全量元数据遍历，B03 允许）：
  1. `projectCurrentSessionBranchEntries(scan.entries, { branchHead, filePath })`（`lib/session-jsonl.ts:193`，**直接复用，不复制分支选择逻辑**）→ `lineage`/`selectedLeafId`/`physicalTailLeafId`/`headResolution`/`legacySyntheticIds`/`recommendedHead`。`SessionBranchError` → 返回 `{directory:null, reason}`（对应 X01 拒绝码）；`legacySyntheticIds=true` → `reason:"legacy_fallback"`（保守：无可证明分支不建目录）。
  2. `projection.recommendedHead` 与存储 head 不一致时，通过 `engine.setSessionBranchHead(sessionPath, recommendedHead)` 完成与 `applyStoredSessionBranchHead`（`core/session-branch-head.ts:107-116`）同语义的幂等写回（`append_recovery`/`observe_tail`/`legacy_backfill` reason 沿用现有枚举）——**恢复职责不取消，但不再为取令牌打开全量 SessionManager**（B07 要求，writer-map §5 #17 语义等价）。
  3. `projectBranchHistory(branchEntries, correlationSessionId)`（`core/message-utils.ts:161`，**需从 module-private 改为 export**）得到 `sourceMessages`，再过 `scanProjectionFacts`（§1.6）产出全部关联事实。
  4. 与 `scan.physical` 对齐：branch entryId → 物理位置（`byEntryId`）；组装坐标级/记录级/关联级/会话级结构（§3）。
  5. `measure()` 估算驻留字节；超 16 MiB → 返回 `{directory:null, reason:"budget_exceeded"}`（该会话标记 no-cache，不截断）。
  6. 释放：清空对 `scan.entries`/`sourceMessages` 的引用，返回 `released` 闭包供 cache 在淘汰/失败时调用（I11）。

### 1.5 `server/history-read/page.ts` — `resolveHistoryPage`

```ts
export function resolveHistoryPage(
  directory: HistoryDirectory,
  params: { beforeId: number | null; limit: number; forceAll: boolean },
): ResolvedHistoryPage
// ResolvedHistoryPage = {
//   bounds: { total, startIdx, endIdx, hasMore },      // 语义 = 现 resolveHistoryPageBounds
//   windowRecordIndexes: number[],                      // displayIndex ∈ [startIdx,endIdx) 的 sourceIndex 升序
//   blockAnchorIndexes: number[],                       // afterIndex ∈ 窗口的 toolResult/custom/interlude 记录（二分定位）
//   dependencyLocations: PhysicalLocation[],            // 工具结局/todos/媒体结果/interlude 锚点指向的页外记录（去重后）
//   headState: { turnInputEntryId, turnInputVisible, assistantOrdinal, displayIdx } // 页首种子
// }
```

- 页边界语义逐字来自现 `resolveHistoryPageBounds`（`sessions.ts:362`）：把该函数**原样移入**本模块并从 `sessions.ts` re-export（route 与 find 路由的 import 不变——见“调用点不变”清单）。`total` 直接取 `directory.sessionFacts.displayTotal`（构建时算好），不再每页计数（闭包项 #12）。
- 窗口记录用 directory 的 displayIndex 升序数组做 `lowerBound/upperBound` 二分（O(log N + K)）；block 锚点用 afterIndex 升序数组二分。禁止全目录 `.filter`（7.1 硬条件）。

### 1.6 `server/history-read/projection-context.ts` — 事实扫描（同源核心）

```ts
// 流式事实收集器：冷构建逐条喂；无缓存全量路径对整个数组一次喂。同一实现。
export function createProjectionFactScanner(opts: { sessionPath: string; bridgeSession: boolean }):
  ProjectionFactScanner
// scanner.visit(message, sourceIndex)：主循环前全部预扫描的一次性合并实现
// scanner.finalize(): ProjectionContext

export function buildProjectionContextFromMessages(sourceMessages, opts): ProjectionContext
// = for 循环 visit + finalize。无缓存全量路径的入口。

export interface ProjectionContext {
  originBySourceIndex: Map<number, { origin: string; displayText?: string }>;
  presentationBySourceIndex: Map<number, PresentationData>;
  agentReviewBySourceIndex: Map<number, ReviewData>;       // 仅 status==="completed"
  runBoundsBySourceIndex: Map<number, { start: number; end: number }>;
  collabDecisionsBySuggestionId: Map<string, DecisionData>;
  toolResultSourceIndexByCallId: Map<string, number>;
  sourceIndexByEntryId: Map<string, number>;
  displayIndexByEntryId: Map<string, number>;
  turnInputByAssistantEntryId: Map<string, string>;
  consumptionDeliveryIds: Set<string>;
  consumptionEntryIds: Set<string>;
  mediaResultRecords: Array<{ sourceIndex: number; taskId: string | null; success: boolean }>;
  deferredInterludeAnchors: Map<number /*anchorAfterIndex*/, { sourceIndex: number; deliveryId: string | null }>;
  todoSnapshotPointer: { sourceIndex: number } | null;
  recordFacts: HistoryRecordFact[];   // 每条记录的 before 状态（§3 记录级）
  displayTotal: number;
}
```

- 抽取来源（逐个，全部“移动循环体、调用点改 import”）：
  | 现位置（sessions.ts） | 移入 | 调用点如何保持不变 |
  |---|---|---|
  | origin zip 预扫描 :1504-1524（消费 `annotateOriginMessages`） | `visit()` origin 分支（同一 pending 指针规则，含“中间 agentReview/presentation 条目跳过不清指针”） | route 全量路径改调 `buildProjectionContextFromMessages`，输出同名 Map |
  | presentation :1525-1539、agentReview :1540-1554（含 completed 过滤） | `visit()` | 同上 |
  | Run 边界预扫描 :1563-1595 | `visit()`（displayCounter/runOrdinal 与主循环同源的判定不变） | 同上 |
  | `toolResultSourceIndexByCallId` 循环 :1632-1641 | `visit()` | 同上 |
  | `sourceIndexByEntryId/displayIndexByEntryId` :1642-1654 | `visit()` | 同上 |
  | turn-input consumption 解析 :1655-1670 | `visit()` | 同上 |
  | `collectSessionCollabDecisions` :1629 | 不移动：scanner 内直接调用现有 `core/message-utils.ts:328` 纯函数 | 不变 |
  | `collectModelCallReferencesBySourceIndex` :1631 | 不移动：直接调用现有 `core/message-utils.ts:266` | 不变 |
  | deferred-result 记录收集（media/interlude 锚点，:1983-1999 的记录侧） | `visit()` custom 分支记录 `mediaResultRecords`；`finalize()` 里对每个 deferred-result 记录执行 `nextImmediateDisplayableAssistantIndex` 等价判定（该函数移入本模块，route import 保持） | 同上 |
  | todos 反向扫描 :2149 | `finalize()` 调用现有 `extractLatestTodoSnapshot`（`lib/tools/todo-compat.ts:144`，不复制“最后者胜/坏快照跳过”规则），只记指针 | 同上 |
- 与旧代码的一个刻意差异：旧代码是 7 个独立 pass，scanner 合并为 1 个 pass + finalize 的 2 个补充 pass（todos 反扫、interlude 锚点前向）。这些 pass 相互独立（各自只读 sourceMessages），合并不改变任何 Map 内容；由 A04 差分保证。
- `recordFacts` 的 before 状态在 visit 时随手记（`displayIndexBefore`、`turnInputEntryIdBefore/VisibleBefore`、`assistantOrdinalBefore`），这就是冷构建与全量路径**同一实现**同时产出“目录事实”和“全量上下文”的机制（I04 的落点）。

### 1.7 `server/history-read/project-page.ts` — `projectHistoryPage`（主循环迁出）

```ts
export interface PageProjectorInput {
  records: HistoryRecordView;          // get(sourceIndex) → message | null（§2）
  context: ProjectionContext;          // 全量路径来自 buildProjectionContextFromMessages；热页来自目录事实（同形状）
  bounds: { total: number; startIdx: number; endIdx: number; hasMore: boolean };
  seed: { sourceIndex: number; displayIdx: number; latestTurnInputEntryId: string | null;
          latestTurnInputVisible: boolean; assistantOrdinalInTurn: number };  // 全量路径 seed={0,0,null,true,0}
  iterate: Array<{ sourceIndex: number; message: AnyMessage }>;  // 全量=全部记录；热页=窗口∪页内锚点记录
  sanitizeVisibleContent: (value: string) => string;
  hydrate: PageHydrateHooks;           // recordMediaGenerationResult / recordDeferredInterlude 等闭包注入
}
export function projectHistoryPage(input: PageProjectorInput):
  { messages: any[]; blocks: any[]; mediaGenerationResults: Map; standaloneMediaGenerationResults: any[] }
```

- 抽取来源：`sessions.ts:1788-2012` 主循环**整体**（user/assistant/toolResult/custom 四分支、assistantOrdinal 先加后判 displayable、toolResult 与 custom 的 `afterIndex = displayIdx - 1`、consumption 优先覆盖指针、loop 置 null、origin→agentReview→presentation 的 spread 次序 :1828-1838、`assistant:${ordinal}` 段 id、Run 边界挂接、legacy isError 已在读取层完成）。逐分支原样搬移，不改任何判定。
- 页内/页外守卫改为数据驱动：全量路径 `iterate` 含全部记录，`currentIndex` 窗口判断保留（与现状逐字节同行为）；热页路径 `iterate` 只含窗口记录与 block 锚点命中记录，守卫天然通过。
- `createHistoryDeferredContent` 适配（B05 locator 适配点）：`server/history-deferred-content.ts` 增加内部入口
  ```ts
  export function createHistoryDeferredContentFor(record: unknown, sourceIndex: number, kind, ordinal, content, opts?)
  ```
  原 `createHistoryDeferredContent(sourceMessages, sourceIndex, …)` 改为取 `sourceMessages[sourceIndex]` 后委托新入口——**对外签名与 locator 格式（version 1, sourceIndex+entryId+kind+ordinal）不变**，热页只传稀疏 view 里取出的 record。`resolveHistoryDeferredContent` 完全不动。
- `deferHeavyHistoryBlock`、`soleRawToolResultText`、`sanitizeVisibleContent` 构造器（`stripSessionReminderBlocks` + `isBridgeSessionPath`）从 route 移入本模块导出，route import 原名不变。

### 1.8 `server/history-read/window-reader.ts` — `readHistoryRecords`

```ts
export async function readHistoryRecords(
  sessionPath: string,
  identity: InternalFileIdentity,              // 打开后 fstat 必须匹配（X04/X05）
  locations: PhysicalLocation[],                // 已排序去重
  opts: { maxConcurrent: 4; mergeGapBytes: 0 /*默认只合并紧邻/重叠*/; readFile?: ReadHook },
): Promise<{ records: Map<number, AnyMessage> /* by sourceIndex */; logicalReadBytes: number } | ReadError>
```

- B05 规则：position read + `bytesRead` 循环补齐；异常 EOF → 重验身份，不把空 Buffer 当 JSON；每条解析后校验 `entryId/type` 与目录一致，不一致 → `invalidation: directory_invalid` 触发本会话失效（不拿错行投影）；所有分支关闭句柄。同一请求重复引用只解析一次（Map 去重）。间隙合并默认关闭，若启用需报 `coalescingExtraBytes`。

### 1.9 `server/history-read/hydrate.ts` — `hydrateExternalState`

```ts
export async function hydrateExternalState(
  page: ProjectedPage, engine: any, opts: { sessionPath; resolvedBounds; isLastPage; storeSnapshot? },
): Promise<void>  // 原地补齐 slicedBlocks / sessionFiles / todos 等（不重新扫描历史）
```

- 抽取来源：`sessions.ts:2014-2149` 的主循环后段——deferredStore.listBySession 回灌、`resolveMediaGenerationBlocks`、块切片（:2029-2033 原样）、subagent/workflow 终态回灌（:2042-2142 整体搬移，含 `createSubagentMetaCache/createSubagentSummaryCache`）、`patchSessionFileLifecycleBlocks`、`listSessionRegistryFiles`、todos。这些代码本就只依赖 `slicedBlocks`+engine store，搬到 hydrate 后 route 调用一次，行为不变。
- `listSessionRegistryFiles` 的引用来源改为 directory 的紧凑身份集合（见 §5 项 2）；无目录时传 `sourceMessages`（原样）。

### 1.10 `server/history-read/cache.ts` — `HistoryDirectoryCache`

```ts
export class HistoryDirectoryCache {
  constructor(opts: { maxSessions?: 8; maxResidentIndexBytes?: 64*MiB; maxSingleDirectoryBytes?: 16*MiB;
                      maxConcurrentBuilds?: 2 });
  get(key: string): { directory: HistoryDirectory } | null;           // 命中提升 LRU
  probe(key: string, ctx: HistoryReadContext): "valid" | InvalidationReason;
  beginBuild(key: string): Promise<BuildLease | null>;                // single-flight + 全局信号量(2)
  publish(key: string, lease: BuildLease, directory: HistoryDirectory): void;  // 版本校验后原子发布
  invalidate(key: string, reason: InvalidationReason): void;
  dispose(): void;
  stats(): { sessions: number; residentBytes: number; builds: number; hits: number; evictions: number };
}
```

### 1.11 `server/history-read/index.ts` — 编排入口（route 唯一调用面）

```ts
export async function readSessionHistoryPage(engine, cache, req: {
  sessionPath; sessionId; beforeId; limit; forceAll; sanitizeVisibleContent; disableCache?: boolean;
}): Promise<
  | { mode: "directory"; result: RoutePageResult; revision: string; fallbackReason: null; buildCount: number }
  | { mode: "full"; result: RoutePageResult; revision: string; fallbackReason: InvalidationReason | null }
  | { mode: "error"; error: unknown }>
```

`RoutePageResult` = 现 route 组装的全部字段（messages/blocks/todos/hasMore/nextBefore/sessionFiles）。

### 1.12 B01 抽取的“调用点不变”清单（汇总）

| 被抽取/移动 | 原调用点 | 保持不变的方式 |
|---|---|---|
| 主循环 :1788-2012 → `projectHistoryPage` | route 唯一 | route 改调 `readSessionHistoryPage`；无目录分支内部喂全量 records + 全量 context，输出字段逐一相同 |
| 7 组预扫描 → `createProjectionFactScanner` | route 唯一 | 同上，全量路径用 `buildProjectionContextFromMessages` |
| 主循环后段 :2014-2149 → `hydrateExternalState` | route 唯一 | 同上 |
| `resolveHistoryPageBounds` | route + （语义参照）find 注释 | 移入 `page.ts`，`sessions.ts` 顶部 `export { resolveHistoryPageBounds } from "../history-read/page.ts"` 式 re-export，find/测试 import 路径不变 |
| `deferHeavyHistoryBlock`/`soleRawToolResultText`/`sanitizeVisibleContent` 工厂 | route 内部 | 移入 `project-page.ts` 导出，route 改 import（无外部消费者） |
| `historyMessageFromEntry`/`projectBranchHistory`（message-utils 内 private） | `loadSessionHistoryMessages`/`loadSessionHistoryEvidence` | 仅加 `export`，函数体零改动 |
| `nextImmediateDisplayableAssistantIndex` | route 主循环 | 移入 `projection-context.ts`（finalize 用），导出 |
| `projectOversizedSessionEntry` | repair 链 | 已导出，scanner 直接 import，函数不动 |
| `collectSessionFileReferenceIdentities` | `SessionFileRegistry.listReachable` | 已导出（registry :983）；`listReachable` 增加可选 `referenceIdentities` 入参，`engine.listSessionFiles(path,{references})` 旧签名与全部既有调用方不变 |
| `createHistoryDeferredContent` | route/projector 多处 | 原签名保留并委托 `createHistoryDeferredContentFor`；两处函数体共用 |
| `sessionFileRevision`/`readSessionFileRevision` | route/find | 不动，`captureReadContext` 复用 |

## 2. 同源语义方案（I04）

**原则：三种模式只有一套 normalize/locate/project 代码，差异仅在“事实从哪来”与“记录怎么读”。**

```
冷目录构建:  scanHistoryFile(全读一次) → projectCurrentSessionBranchEntries（分支语义唯一实现）
             → projectBranchHistory → createProjectionFactScanner.visit/finalize → 压缩为目录 → 丢弃 entries/messages
热目录窗口:  目录事实（=同一 scanner 的冻结产物）+ resolveHistoryPage + readHistoryRecords（稀疏）
             → projectHistoryPage(records=sparseView, context=目录事实切片, seed=页首状态)
无缓存全量:  loadSessionHistoryMessages（原函数，含 repair/SDK 链，行为不变）
             → buildProjectionContextFromMessages（=同一 scanner）→ projectHistoryPage(records=全数组, seed=零起点)
```

- **直接复用、零改动的纯函数**（易碎语义全部留在原函数里，绝不重写）：`historyMessageFromEntry`（含 legacy tool failure isError 补写、custom_message/custom/loop-user-prompt 三类投影）、`projectSessionMessageForDisplay`（Reminder 剥离，读取层）、`isDisplayableHistoryMessage`、`annotateOriginMessages`、`collectModelCallReferencesBySourceIndex`、`collectSessionCollabDecisions`/`overlaySessionCollabDecision`、`collectDesktopInputCorrelations`、`collectToolOutcomesByCallId`/`projectToolResultOutcome`/`isKnownLegacyLingxiToolFailure`、`extractTextContent`/`filterUnreferencedInlineImages`/`contentHasThinkingBlock`、`extractPersistedAssistantSemanticSegments`、`isHiddenTurnInputMessage`/`isCustomTurnInputHistoryMessage`/`parseTurnInputConsumptionRecord`/`parseTurnInputPresentationRecord`、`extractLatestTodoSnapshot`/`migrateLegacyTodos`、`stripSessionReminderBlocks`/`sanitizeBridgeVisibleText`、`extractBlocks`/`resolveMediaGenerationBlocks`、`buildDeferredResultInterludeBlock`/`buildLoopInterludeBlock`、`projectCurrentSessionBranchEntries`、`resolveHistoryDeferredContent`。
- **轻抽取**：主循环、预扫描、页边界、后段 hydrate（§1.6/1.7/1.9，均为“搬移不改判定”）；`HistoryRecordView` 稀疏视图（`get(sourceIndex)`，全量路径用恒等数组适配器，热页用 Map 适配器）。
- **易碎点清单（实施与评审时逐一对照，任何一处“顺手优化”都算破坏 I03/I04）**：
  1. **隐藏 user 占 display 序号**：`isDisplayableHistoryMessage` 只看原始文本/内联图，`<hana-background-result>`/`<hana-deferred-tasks>` 是文本→占号；`stripSessionReminderBlocks` 剥离后变空的 user 仍占号（判定在剥离前）。页窗口切在服务端序号上。scanner 记录 visibility 必须用剥离前的 message。
  2. **不可见 assistant 推大 assistantOrdinal**：`assistantOrdinalInTurn += 1` 在 displayable 判定**之前**（:1842）。跨页 seed 如果只重放可见记录，同轮后续 `assistantSegments[].id`（`assistant:${ordinal}:…`）会错——这正是 before 状态必须进目录的原因。
  3. **Reminder 双层剥离**：读取层（`projectSessionMessageForDisplay`）+ 展示层（`sanitizeVisibleContent`）都执行；find 路由用原始文本判定。迁移时两层都要保留。
  4. **origin/presentation/agentReview 覆盖次序**：`displayText` 由 origin → agentReview → presentation 依次 spread（:1828-1832），后写覆盖前写；review 仅 `status==="completed"` 下发；origin 的 pending 指针跨过 review/presentation 条目不清零。覆盖次序错一条就是前端 displayName 错。
  5. **legacy tool failure**：`historyMessageFromEntry` 的 isError 内存补写发生在预扫描之前（投影层），因此 `collectToolOutcomesByCallId` 看到的是已补写的 isError——目录事实扫描必须基于同一投影产物，不能从未补写的原始 entry 直接收集结局。
  6. **consumption 覆盖指针**：assistant 的 `turnInputEntryId` 优先取 `turnInputByAssistantEntryId`，且绑定恒 `turnInputVisible:false`；隐藏轮 entryId=null 时仍要显式下发 `turnInputVisible:false`（:1939-1943 的双向语义）。
  7. **afterIndex 时机**：toolResult/custom 的锚点是 `displayIdx - 1`（进入记录时的计数减一），不是记录自身 displayIndex（它们没有）。热页按 `displayIndexBefore - 1` 预计算进目录，差异=1 的 off-by-one 是最容易踩的坑。
  8. **分支选择**：唯一实现是 `projectCurrentSessionBranchEntries`；head 行缺失（legacy_tail）与存在但 `leafId=null`（显式空选择）、`observedTailLeafId` 参与的 append_recovery 判定、`continuesDiscardedObservedTail` 排除，全部不得在目录层复制第二份（I05）。

## 3. 目录数据结构草案（不含正文/工具输出/完整 Map）

```ts
// —— 文件级 ——
interface HistoryDirectory {
  version: 1;
  key: HistoryDirectoryKey;
  file: HistoryFileIndex;
  branch: HistoryBranchIndex;
  records: HistoryRecordFact[];            // 按分支序（= sourceIndex 序）
  assoc: HistoryAssociationIndex;
  session: HistorySessionFacts;
  measuredBytes: number;                   // measure() 保守估算，含 string×2、Map/Set/数组条目成本
}
interface HistoryDirectoryKey {
  runtimeId: string; studioId: string;     // 缺省 "default"
  sessionId: string | null;                // 无业务 id 时为 null，走 path 命名空间
  normalizedPath: string;                  // path.resolve
  fileIdentity: { dev?: number; ino?: number; size: number; mtimeMs: number; ctimeMs?: number };
}
interface HistoryFileIndex {
  observedFileSize: number;
  indexedThroughOffset: number;
  pendingTailOffset: number | null;
  lastUndelimitedRow: { offset: number; length: number } | null;
  header: { sdkSessionId: string; version: number };
  byEntryId: Map<string, { physicalIndex: number; byteOffset: number; byteLength: number }>;
  physicalCount: number;
}
// —— 分支级 ——
interface HistoryBranchIndex {
  headRowExists: boolean;                  // manifest 无行 ≠ leafId:null（I05）
  persistedLeafId: string | null;
  observedTailLeafId: string | null;
  selectedLeafId: string | null;
  physicalTailLeafId: string | null;
  headResolution: "legacy_tail" | "persisted_head" | "append_recovery";
  lineageEntryIds: string[];               // root→leaf，仅 id
}
// —— 坐标级 ——
// records[] 隐含 sourceIndex=数组下标；另存：
//   displayableSourceIndexes: number[]（升序，供窗口二分）
//   blockAnchorByAfterIndex: { afterIndex: number; sourceIndex: number }[]（升序，供锚点二分）
// —— 记录级（每条，小字段）——
interface HistoryRecordFact {
  entryId: string | null;
  role: "user" | "assistant" | "toolResult" | "custom";
  customType?: string;
  visible: boolean;
  displayIndex: number | null;
  displayIndexBefore: number;              // afterIndex = displayIndexBefore-1
  turnInputEntryIdBefore: string | null;
  turnInputVisibleBefore: boolean;
  assistantOrdinalBefore: number;
  toolCallIds?: string[];
  toolName?: string; isError?: boolean;
  runOrdinal?: number; turnStartIndex?: number; turnEndIndex?: number;
  timestamp?: string;
}
// —— 关联级 ——
interface HistoryAssociationIndex {
  toolResultByCallId: Map<string, { sourceIndex: number }>;
  correlationByUserEntryId: Map<string, { clientMessageId?: string; snapshotVersion?: number;
                                          sourceEntryId?: string; acceptanceDiagnostic?: "ambiguous" }>;
  turnInputByAssistantEntryId: Map<string, string>;
  consumptionDeliveryIds: string[];        // 排序数组 + 二分（页请求水化为 Set）
  consumptionEntryIds: string[];
  collabDecisionBySuggestionId: Map<string, { status: string; resultSessionId?: string }>;
  modelCallRefBySourceIndex: Map<number, { modelCallId: string; traceId: string | null; parentCallId: string | null }>;
  originBySourceIndex: Map<number, { originRecordSourceIndex: number }>;
  presentationBySourceIndex: Map<number, { presentationRecordSourceIndex: number }>;
  agentReviewBySourceIndex: Map<number, { reviewRecordSourceIndex: number; completed: boolean }>;
  deferredInterludeAnchors: { anchorAfterIndex: number; sourceIndex: number; deliveryId: string | null }[];
  mediaResultRecords: { sourceIndex: number; taskId: string | null; success: boolean }[];
  todoSnapshot: { sourceIndex: number } | null;
}
// —— 会话级 ——
interface HistorySessionFacts {
  displayTotal: number;
  publicRevision: string;                  // 构建前 stat 的 `${size}:${mtimeMs}`（I07）
  fileIdentity: HistoryDirectoryKey["fileIdentity"];
  activeFileReferenceIdentities: SessionFileReferenceIdentities;
  originPresentationPayloadInline: boolean;
}
```

红线自查：没有 `content`/`details`/`output`/`base64`/block 对象/`collectToolOutcomesByCallId` 的完整 Map；没有 `sourceMessages` 数组；没有 parsed entries。

## 4. 读取序列（B06）嵌入现有路由

**route 侧改动点**（`server/routes/sessions.ts` GET /sessions/messages，:1466）：

```
:1468-1487  身份解析 + isValidSessionPath + authorizeSessionRoute   —— 原样，先于缓存（I01）
:1488-1490  reconciling / beforeRun snapshot                        —— 原样（reconciliation 不进快路径）
:1494       revision = await readSessionFileRevision(...)           —— 原样保留（竞态纪律不动）
:1496-2167  ↓ 替换为：
  if (reconciling) {
    evidence = await loadSessionHistoryEvidence(...)                —— 原样
    result = projectFullPage(evidence.messages, …)                  // 同一 projector，全量模式
  } else {
    outcome = await readSessionHistoryPage(engine, cache, { sessionPath: resolvedSessionPath,
      sessionId: querySessionId, beforeId, limit, forceAll, sanitizeVisibleContent })
    // readSessionHistoryPage 内部（index.ts）：
    //   ① captureReadContext（stat 在读内容前）
    //   ② revision null → 直接无缓存全量（I08，且不写缓存）
    //   ③ 非 forceAll 且命中 → probe（fstat 身份 + path stat + revision 相等 + head 行相等 + locator 相等）
    //      → resolveHistoryPage → readHistoryRecords → projectHistoryPage(热页) → hydrateExternalState
    //      → 复核（再 stat + head + locator）：全部仍等于捕获值 → 返回 {mode:"directory", revision=ctx.publicRevision}
    //   ④ 任一失效 → cache.invalidate(reason) → 尝试 2：重建目录（fresh ctx）重复 ③
    //   ⑤ 尝试 2 仍失败 / forceAll / 目录不适用（非 Pi、legacy、budget_exceeded、disableCache）
    //      → 无缓存全量：loadSessionHistoryMessages + buildProjectionContextFromMessages + projectHistoryPage
    //        + hydrateExternalState（revision 沿用 :1494 已取值 + 读后一致性校验沿用现状口径）
    //   ⑥ 全量也抛真实 I/O 错误 → 沿用现有 catch（:2168 c.json 500），不返回旧页、不伪造空历史
  }
:2154       rebroadcastSession（!reconciling && beforeId==null）     —— 移到目录重试成功之后、每请求恰一次
:2158-2167  afterRun / reconciliation 载荷 / c.json 组装             —— 原样
```

- **分支到快路径的条件**（全部满足才走目录）：`!reconciling && !forceAll && revision !== null && looksLikePiSessionFile && 目录版本有效`。`forceAll`（all=1）用同源**全量模式**（B07 允许），无窗口性能断言；`reconciling` 完全不碰目录（B07/严格证据等级 I12）。
- **fallback 链**（最多 2 次一致视图尝试 + 1 次重建，§B06）：

```
尝试1（目录）─失败(任一 reason)→ invalidate + 重建 ─→ 尝试2（目录）─失败→ legacy 全量（loadSessionHistoryMessages，
不使用任何旧目录，自带既有 repair/SDK 语义与 revision 纪律，记 fallbackReason）─失败→ 现有错误路径
```

- reason 枚举按 B06 十一项原样实现（`revision_unknown / file_identity_changed / branch_changed / locator_changed / untrusted_mutation / directory_invalid / short_read / tail_incomplete / budget_exceeded / legacy_fallback / snapshot_changed`），正常追加在 B 阶段表现为 `snapshot_changed` → 重建（可信增量留给 C）。日志含内部会话标识、阶段、reason、前后版本摘要、重建/回退次数；无正文无工具输出。
- 返回的 `revision`：目录路径 = `ctx.publicRevision`（构建前 stat，读取复核保证此后未变 → 不超前，I07）；重建/回退路径 = 其自身捕获值。
- `disableCache` 为 `readSessionHistoryPage` 的 DI 参数（测试用），不新增 query 参数；关闭时走“临时构建 + 同一 projector”。
- `/sessions/content/:contentId`、`/sessions/find`、`/sessions/search` 不迁移（B07）：find 继续用 `loadSessionHistoryMessages` + `collectFindableHistoryEntries`（该函数对共享纯函数的依赖经 Batch-1 回归保护）。

## 5. 页外依赖闭包逐项落点

| # | 依赖 | 目录定位 | 热页成本 |
|---|---|---|---|
| 1 | todos 尾部快照 | `assoc.todoSnapshot` 指针（构建时用 `extractLatestTodoSnapshot` 反扫一次） | 读 1 条记录 + 纯函数迁移；无反扫 |
| 2 | sessionFiles 全分支引用 | `session.activeFileReferenceIdentities`：构建时跑现有 `collectSessionFileReferenceIdentities`；热页把身份集合交给 `listReachable(path, {referenceIdentities})` | O(引用数+registry 大小) |
| 3 | media 终态 | `assoc.mediaResultRecords`（taskId→记录位置）；页内 media_generation 块→按 taskId 补读记录；末页→补读全部 success 记录复现 standalone 注入；`deferredStore.listBySession` 照旧 | 记录数 = 页内 taskId 数（+末页 success 数） |
| 4 | collab 状态 | `assoc.collabDecisionBySuggestionId`；`overlaySessionCollabDecision` 原函数逐块覆盖 | O(页内块数) |
| 5 | 跨页 toolCall 结局 | `assoc.toolResultByCallId` → `dependencyLocations`；结局用 `projectToolResultOutcome` 现算；目录无对应 toolResult → 与旧全量 map miss 同样落 `{status:"unknown",success:false}` | 读取数 = 页内 toolCall 数 |
| 6 | turnInput/Run 边界 | 记录级 before 状态 + `assoc.turnInputByAssistantEntryId`；`runBounds` 存记录级 | O(1)/记录 |
| 7 | interlude 消费/抑制 | `assoc.consumptionDeliveryIds/EntryIds`（请求内水化为 Set）+ `deferredInterludeAnchors`（anchorAfterIndex 升序，窗口二分） | O(K + 窗口锚点数) |
| 8 | correlation/歧义 | `assoc.correlationByUserEntryId`（构建时 `collectDesktopInputCorrelations` 全分支统计一次）；sessionId=='' 时为空 Map | O(1)/user |
| 9 | origin/presentation/review/modelCallRef | bySourceIndex 指针表；页首命中时补读对应 custom 记录 | O(命中数) |
| 10 | hasMore/nextBefore/total/afterIndex 重映射 | `session.displayTotal` + `resolveHistoryPage` 边界 + 切片逻辑（hydrate 内原样） | O(K) |
| 11 | deferred locator 解析 | 不优化（B07）；展开端点整链重读 + entryId 校验保持 fail-closed | — |
| 12 | revision/reconciliation | 捕获/复核点在 route 与 `captureReadContext`，两次 head 快照语义不变 | — |

外部 store（deferredResults/subagentRuns/registry sidecar/session-meta/agent registry/streaming 状态）全部视为可变外部输入，仅存在于 `hydrateExternalState`，不冻结进目录。

## 6. 缓存预算与失效

- **归属与键**：`HistoryDirectoryCache` 实例在 `createSessionsRoute` 内创建（route/runtime 实例私有，不设模块级单例）。稳定键 = `runtimeId \0 studioId \0 (sessionId ?? "path:"+resolve(path))`；目录条目内部再绑定 `normalizedPath + fileIdentity(dev/ino/size/mtime/ctime) + locator.path + branch head`——键定位槽位，身份决定有效性（I01、X18）。
- **槽位语义**：每会话一个槽保存“当前目录版本”；在途旧版本按引用计数计入 `residentBytes`（旧构建晚完成 → `publish` 版本校验失败丢弃，X16）。
- **预算**：`maxSessions=8`（LRU）；`maxResidentIndexBytes=64 MiB`；`maxSingleDirectoryBytes=16 MiB`（超限该会话永久走无缓存只读路径，不截断）；`maxConcurrentBuilds=2`（全局信号量 + per-key single-flight；失败从 in-flight 表移除可重试）；`maxConcurrentReadsPerPage=4`。`measuredBytes` 口径：string 按 UTF-8 字节×保守系数、Map/Set/数组按条目常数 + key 长度；**不用 JSON.stringify 长度冒充 heap**；单列构建期临时内存。
- **probe（命中校验，全部通过才可用）**：①路径 stat 的 dev/ino/size/mtime/ctime 与目录一致；②`sessionFileRevision(stat) === directory.session.publicRevision`；③head 行存在性/leafId/observedTailLeafId 相等；④`manifest.currentLocator.path` 未变。任一不过 → `invalidate(reason)`。
- **B 阶段失效策略**：任何无法证明为“同一稳定视图”的变化 → 重建（追加分类留给 C01）。probe 失败、read 阶段身份/EOF 异常、记录 entryId/type 不符、读后复核变化、head 变化、归档 rename/utimes（size 不变 mtime 变 → `snapshot_changed` 重建，宁可误重建不可混页）。`revision_unknown`（stat 失败）→ 不读不写缓存。淘汰/失效/`dispose` 必须释放目录内 Map、闭包与句柄（I11，X17）。

## 7. 测试计划（B08）

**差分主工具（A04 复用）**：新脚本 `scripts/diff-history-read-directory-phase-b.mjs`（或给参考采集脚本加 `--compare` 模式），复用 harness：每请求从同一原始夹具字节复制独立文件 + 全新空 manifest store + 全新 app；对 7 份冻结参考（n1000 first/middle/last/all、n10000 first/middle/last）以相同 R1–R5 归一化重放，比对 `sha256Normalized` + manifest 字段清单。三种模式各跑一遍：cache-off 全量、cache-on 冷、cache-on 热（第二遍同请求）。任何字段差异（含 sourceIndex/entryId/Run 边界/deferred id）都是失败。

**定向测试（新增文件，映射 X01—X20 中 B 阶段可覆盖项）**：

| 新测试文件 | 覆盖 | 要点 |
|---|---|---|
| `tests/history-read-directory-scanner.test.ts` | X06、X07、X01(扫描层) | 尾换行有/无、CRLF、中文/emoji、跨块 UTF-8 半字符、偏移正确性；注入短读补齐、提前 EOF 不读空 Buffer；坏尾行→pendingTailOffset |
| `tests/history-read-directory-build.test.ts` | X01、X08、X09、X13 | 重复 id/自环/缺父/环 → 拒绝码与现严格层一致；head 缺失 vs leafId=null vs append_recovery vs observedTail 续弃分支（真实 SessionManifestStore）；todos 指针规则；目录 facts === 全量上下文性质测试 |
| `tests/history-read-directory-cache.test.ts` | X15、X16、X17、X02、X03、X04、P09 | 同版本并发只 build 一次；乱序完成旧不覆新；淘汰/取消/异常/销毁无泄漏；同长度重写、变长重写、同 size+mtime 原子替换→失效 |
| `tests/history-read-window-reader.test.ts` | X05、X07(读取层) | 读取途中 truncate/替换/locator 迁移→不混合页面；entryId 校验失败→directory_invalid |
| `tests/history-read-route-fallback.test.ts` | X05、P06、P11 | 最多 2 次尝试+1 次重建次数断言（受控调度）；stat=null 不读不写缓存；真实 I/O 故障走既有错误路径；reason 枚举齐全 |
| `tests/history-read-directory-semantics.test.ts` | X10、X11、X12、X14、X19 | 跨页 toolResult 结局；interlude 抑制/去重次序；collab 后补、correlation ambiguous；store 变化反映；热页 deferred 凭证经 /sessions/content/:contentId 展开成功 |
| `tests/history-read-cache-auth.test.ts` | X18、P10 | 热缓存后拒绝授权；跨 sessionId/path、studio/runtime 无污染 |
| `tests/history-read-shared-functions.test.ts` | X20 | all/reconciliation/find 共用函数回归 |
| `tests/history-read-directory-memory.test.ts` | 7.4/P09 | 唯一大载荷标记夹具可达性扫描（目录无正文/Buffer/源消息数组/完整输出 Map）；淘汰后无引用残留；8 会话预算 |
| 扩展 benchmark 脚本 | 7.1/7.2 | `IMPLEMENTED_PHASES` 增加 B；断言热页 `fullFileReadCalls=0`、`fullHistoryProjectionCount=0`、`fallbackCount=0`、`jsonlParseCount === 去重记录数`；输出 phase-b/ |

**既有 P08 套件全绿**（每批次末跑）：`tests/history-pagination-run-continuity.test.ts`、`tests/history-pagination-invalid-fixture.test.ts`、`tests/history-run-outcome-edges.test.ts`、`tests/sessions-route.test.ts`、`tests/session-find-route.test.ts`、`tests/history-read-directory-counters.test.ts`、`desktop/src/react/__tests__/chat-semantics/turn-outcome-unification.test.ts`、`npm run typecheck`（三配置）。

## 8. 风险排序（Top 5 + 验证方法）

1. **主循环/预扫描抽取造成无缓存路径行为漂移**。验证：Batch 1 单独落地，前后用 7 份参考输出 sha256Normalized 逐字节差分 + P08 全套 + 手写混合小夹具精确期望；Batch 1 合入前跑“抽取前基线一轮”留对照。
2. **repair 缺席导致的差异**：超限行→scanner 内存套用 `projectOversizedSessionEntry`（含 `hanaRepair` 元数据形状）；坏行→目录构建失败走 legacy 回退（该路径仍 repair，与旧行为一致）。验证：构造超限行/坏行夹具，断言新路径响应===旧路径（repair 后）响应、新路径不产生 `.repair.json`、文件 sha256 不变（有意行为差异记录进 PROGRESS）。
3. **页首 seed 与隐藏记录语义**（易碎点 1/2/7）：只有窗口模式暴露，全量差分测不出来。验证：专造夹具（页首前有不可见 assistant、隐藏 user、loop-turn、跨页 Run），断言窗口投影 === 全量投影按 displayIndex 切片（同函数强不变量），人工核对 `assistantSegments[].id`/`turnInputVisible:false`/隐藏整页游标推进。
4. **分支/manifest 恢复职责的等价迁移**：目录路径不再冷开 SessionManager，`applyStoredSessionBranchHead` 的 setBranchHead 恢复写必须由 build 的 `projectCurrentSessionBranchEntries` + `engine.setSessionBranchHead` 幂等复现。验证：X08/X09 用真实 SQLite store 断言 head 行终态与旧路径逐字段一致；git grep 复核未触碰 reconciliation 无副作用约束。
5. **字节扫描器坐标错误**（CRLF/跨块 UTF-8/无尾换行末条/pendingTail）：验证：X06 性质测试（随机多字节内容 × 随机 chunk 边界 × 4 种行尾组合，逐条 `readHistoryRecords(offset)` === 整文件 parse）；X02–X04 由 probe 五元组兜底+专门用例（含同 size+mtime 原子替换）。

次级风险：`nextBefore`/空页语义窗口模式回归（T11 已覆盖）；`rebroadcastSession` 重试后重复广播（断言每请求恰一次）；目录预算估算口径与真实 heap 偏差（7.4 用可达性扫描实测校准）。

## 9. 实施批次（5 批，每批可独立验证、可中止）

**Batch 1 — 同源抽取（B01，纯重构零行为变更）**
内容：§1.6/1.7/1.9 的 scanner/projector/hydrate 抽取 + `resolveHistoryPageBounds`/工具函数移位导出 + `historyMessageFromEntry`/`projectBranchHistory` 加 export；route 改调全量模式（无缓存、无目录）；不新增 fs 语义。
验收：
```bash
npx vitest run tests/history-pagination-run-continuity.test.ts tests/history-pagination-invalid-fixture.test.ts tests/history-run-outcome-edges.test.ts tests/sessions-route.test.ts tests/session-find-route.test.ts tests/history-read-directory-counters.test.ts
node scripts/diff-history-read-directory-phase-b.mjs --mode full --sizes 1000,10000   # 7/7 sha256Normalized 一致
npm run typecheck
```

**Batch 2 — 只读扫描与目录构建（B02+B03）**
内容：`scan.ts`/`directory.ts`/`types.ts` + 超限行内存投影复用；不动 route。
验收：
```bash
npx vitest run tests/history-read-directory-scanner.test.ts tests/history-read-directory-build.test.ts
npm run typecheck
```

**Batch 3 — 定点读取与缓存（B04+B05）**
内容：`page.ts`/`window-reader.ts`/`cache.ts` + `createHistoryDeferredContentFor` 适配 + registry `referenceIdentities` 入参；不动 route。
验收：
```bash
npx vitest run tests/history-read-directory-cache.test.ts tests/history-read-window-reader.test.ts tests/history-read-directory-scanner.test.ts
npm run typecheck
```

**Batch 4 — 路由接线与快照/回退（B06+B07，生产启用点）**
内容：`read-context.ts`/`index.ts` + route 改造（§4）+ 日志/reason 枚举 + rebroadcast 恰一次。
验收：
```bash
npx vitest run tests/history-read-route-fallback.test.ts tests/history-read-directory-semantics.test.ts tests/history-read-cache-auth.test.ts tests/history-read-shared-functions.test.ts tests/history-read-directory-memory.test.ts
npx vitest run tests/history-pagination-run-continuity.test.ts tests/history-pagination-invalid-fixture.test.ts tests/history-run-outcome-edges.test.ts tests/sessions-route.test.ts tests/session-find-route.test.ts
node scripts/diff-history-read-directory-phase-b.mjs --mode full --mode cold --mode hot --sizes 1000,10000
npm run typecheck
```

**Batch 5 — 阶段验收（B08）**
内容：benchmark `--phase B`；phase-b 数据；7.1/7.2/7.4 断言核对；PROGRESS.md/acceptance 记录。
验收：
```bash
node scripts/benchmark-history-read-directory.mjs --phase B --sizes 1000,10000 --page-size 50 --seed 20260910 --output artifacts/history-read-directory/phase-b
npx vitest run tests/history-read-directory-memory.test.ts
npm test && npm run typecheck
```
B08 通过标准：稳定热页 `fullFileReadCalls=0`、`fullHistoryProjectionCount=0`、`fallbackCount=0`；热页 p50_10k ≤ 3×max(p50_1k, 5ms)；冷页 p50 ≤ 1.25×9.3ms+25ms（1k）/1.25×79.1ms+25ms（10k）；全翻 10k/1k ≤ 15；目录无正文驻留。

## 补充判断依据

- 分支选择一律经 `projectCurrentSessionBranchEntries`（`lib/session-jsonl.ts:193`），目录层不做第二份 lineage/hash 实现；B01 允许的“纯分支选择与 hash 分离”**本期不做**。
- `legacySyntheticIds` 文件与 `budget_exceeded` 会话显式走 `legacy_fallback`（旧链路原样）。
- 每批结束更新 `artifacts/history-read-directory/PROGRESS.md`；Batch 4 是生产行为变更点，落地前先把 Batch 1 的“抽取前参考差分基线”存档，任何 Batch 4 后的差分失败先回滚到 Batch 3 状态二分定位，而不是在缓存层修补投影差异（I04）。
