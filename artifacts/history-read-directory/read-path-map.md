# read-path-map.md — 历史读取路径全链路地图（A02）

- 仓库：`ItsDalk-Lane/LingxiAgent`，分支 `fix/pending-sep10`，HEAD `1d42b7405c76292f617291e3a01cd2f3ef5efd04`（2026-09-10 记录）。
- 定位方式：全部按「文件路径:函数名」定位，行号为当前 HEAD 的参考坐标（函数为权威锚点）。
- 覆盖：普通分页 / all=1 / reconciliation=1 / 延迟 content / find 五条读取路径 + 公共底座、sessionFiles registry 链、四个投影模块的主循环位置、前端链。
- 本文件只做事实记录，不改任何生产代码。

---

## 0. 公共读取底座（五条路径共用）

### 0.1 消息加载主入口

`core/message-utils.ts: loadSessionHistoryMessages(engine, explicitPath, options)`（普通分页 / all / content / find 使用；reconciliation 不用）

```
loadSessionHistoryMessages
├─ options.readSideEffects === false → loadSessionHistoryEvidence(...)（见 §3）
├─ looksLikePiSessionFile(sessionPath)                    [core/message-utils.ts]
│    异步 fs.open + 读前 512 字节，解析首行 JSON，要求 type==="session" 且有 string id
├─ repairOversizedSessionEntriesInFile(sessionPath)       [core/session-jsonl-file.ts]  ← 隐式修复 + 同步全文件读 + 可能整文件重写
├─ engine.openSessionManagerAtCurrentBranch(sessionPath, dirname(sessionPath))
│    ├─ SessionManager.open(path, sessionDir)             [lib/pi-sdk → @earendil-works/pi-coding-agent/dist/core/session-manager.js]
│    │    ├─ readSessionHeader(path)（有界扫描；超限 → loadEntriesFromFile 全量同步读）
│    │    └─ 构造函数内 loadEntriesFromFile(sessionFile)   ← 同步全文件读（openSync/readSync 循环；坏行静默跳过）
│    └─ SessionCoordinator.applySessionBranchHead(sessionPath, manager)   [core/session-coordinator.ts]
│         └─ applyStoredSessionBranchHead({store, sessionId, sessionManager})   [core/session-branch-head.ts]
│              ├─ manifestStore.getBranchHead(sessionId)   [core/session-manifest/store.ts，better-sqlite3，同步]
│              ├─ projectCurrentSessionBranchEntries(managerEntries(manager), {branchHead})   ← 完整分支校验（见 0.2）
│              ├─ applyLeafToManager(manager, selectedLeafId)（manager.branch(leaf) / resetLeaf）
│              └─ head 不匹配时 manifestStore.setBranchHead(...)  ← 读路径上的 manifest（SQLite）写入（隐式副作用）
├─ manager.getBranch()                                    [SDK，内存树回溯 leaf→root，无 I/O]
├─ projectBranchHistory(branch, sessionId)                [core/message-utils.ts]
│    ├─ collectDesktopInputCorrelations(entries, sessionId)   [core/desktop-input-correlation.ts]
│    ├─ entries.map(historyMessageFromEntry).filter(Boolean)  [core/message-utils.ts]
│    │    └─ toolResult 命中 isKnownLegacyLingxiToolFailure → 投影层 isError:true   [shared/tool-outcome.ts] ← 内存修复
│    └─ .map(projectSessionMessageForDisplay)             [core/session-reminders.ts]（剥离 reminder 展示块，纯内存）
└─ 任一步骤抛错 / 非 Pi 文件 → 兼容回退：
     fs.readFile(sessionPath, "utf-8")（异步全文件读）
     → 逐行 JSON.parse，坏行跳过（无分支校验！）
     → historyMessageFromEntry → projectSessionMessageForDisplay
     → 仍失败返回 []
```

关键事实（普通加载路径）：

| 事实 | 位置 |
| --- | --- |
| 一次请求最多 3 次全文件读：repair 同步读 + SDK header 扫描（有界）+ SDK 全量同步装载 | `core/session-jsonl-file.ts: repairOversizedSessionEntriesInFile`、`session-manager.js: SessionManager.open/loadEntriesFromFile` |
| 投影失败的兜底是「无分支校验的线性全读」，会包含被抛弃分支的记录 | `core/message-utils.ts: loadSessionHistoryMessages` fallback 块 |
| sessionId 仅当 `manifest.currentLocator.path === sessionPath` 时传入 `projectBranchHistory`，否则传 `''`（correlation 不收集） | `core/message-utils.ts` 调用点 |
| 旧工具失败在加载层统一补 isError（只改投影副本，不改文件） | `core/message-utils.ts: historyMessageFromEntry` → `shared/tool-outcome.ts: isKnownLegacyLingxiToolFailure` |

### 0.2 完整分支校验的唯一实现

`lib/session-jsonl.ts: projectCurrentSessionBranchEntries(entries, opts)`（被 `readCurrentSessionBranch` 包一层同步全文件读）

- `buildValidatedEntryIndex`（同文件）：
  - 全部条目无 id → 合成 `legacy-line-N` 线性链，标记 `legacySyntheticIds=true`（只读兼容投影）；
  - id 混合（部分有部分无）→ 抛 `SessionBranchError("session_branch_invalid_id")`；
  - id 重复 → 抛 `session_branch_duplicate_id`；
  - parentId 悬空 → 抛 `session_branch_dangling_parent`；
  - DFS 三色标记 → 抛 `session_branch_cycle`；
  - JSON 解析失败 → `parseFullSessionEntries` 抛 `session_branch_read_failed` / `session_branch_invalid_json`（严格，不跳坏行）。
- 头部解析：持久化 `branchHead` 存在时按 `observedTailLeafId` / `persistedLeafId` 判定 `persisted_head` vs `append_recovery`；无 head 时 `legacy_tail`（物理尾）。
- `lineageToRoot` → `computeSessionLineageMetadata`（sha256 前缀哈希）。
- 上游调用者：
  - `core/session-branch-head.ts: applyStoredSessionBranchHead`（冷开 manager，普通路径 §1/§2/§4/§5 每次都走）；
  - `core/session-branch-head.ts: readManifestSessionBranch`（经 `SessionCoordinator.getSessionBranchProjection`，`persistRecovery=true` 时也会 `setBranchHead` 写 manifest —— memory-ticker / diary 等读取方使用）；
  - `core/message-utils.ts: loadSessionHistoryEvidence`（reconciliation，§3）。

### 0.3 修订点（revision）与列表投影缓存

- `core/session-list-projection-cache.ts: sessionFileRevision(stat)` = `` `${stat.size}:${stat.mtimeMs}` `` —— 唯一修订格式，三处共用：本缓存失效判据、`/api/sessions` 列表投影 `revision`、`/api/sessions/messages` 响应 `revision`。
- `core/session-list-projection-cache.ts: SessionListProjectionCache.list(sessionDir)`：`readFileLikePaths` 列目录 → 逐文件 `fs.stat` → 签名命中返回克隆缓存；未命中 `buildSessionProjection`（异步**全文件读** + 逐行解析，坏行跳过）。位于**列表路径**：`core/session-coordinator.ts: listSessions`（engine `listSessions` → GET `/api/sessions`、`/sessions/search`）。
- 路由侧修订点：`server/routes/sessions.ts: readSessionFileRevision(sessionPath)` = 异步 `fs.stat` + `sessionFileRevision`；stat 失败返回 `null`（显式「未知」，不缓存、不 stamp）。messages（:1494）与 find（:1168）都在**读内容之前**取 revision（防把没读到的写入标成已同步，issue #1610 反向竞态）。

### 0.4 SessionCoordinator 在读取链中的位置

`core/session-coordinator.ts`：

| 函数 | 在读取链中的角色 |
| --- | --- |
| `openSessionManagerAtCurrentBranch` | messages/all/content/find 的每次加载都冷开 manager；内部 `applySessionBranchHead` |
| `applySessionBranchHead` | `_ensureBranchManifestForPath` + `applyStoredSessionBranchHead`（可能写 manifest） |
| `setSessionBranchHead` / `_syncSessionBranchHead(Quiet)` | 写侧；读取路径不直接调用 |
| `getSessionBranchProjection` → `readManifestSessionBranch` | 面向 memory-ticker / diary 等分支读取方；`persistRecovery` 默认写回 |
| `listSessions` | 列表/搜索路径，聚合 `SessionListProjectionCache.list` + titles + session-meta.json（`_readMetaCached`） |
| `readSessionPromptContextByPath` | `/sessions/prompt-snapshot` 用，不在五条路径内 |

---

## 1. 普通分页：GET /api/sessions/messages

路由入口：`server/routes/sessions.ts: createSessionsRoute → route.get("/sessions/messages", ...)`。
鉴权：`createRequestContext`（server/http/boundary.ts）→ `authorizeSessionRoute(requestContext, "sessions.read", …)`；`?sessionId=` 先经 `engine.getSessionManifest` 解析为 `manifest.currentLocator.path`，`?path=` 走 `isValidSessionPath`（`core/message-utils.ts`，防路径穿越）。

```
route.get("/sessions/messages")
├─ readDesktopInputRunSnapshot(engine, …)            [core/desktop-session-submit.ts]（仅 reconciling 时；§3）
├─ readSessionFileRevision(resolvedSessionPath)      [sessions.ts] ← 读内容前取修订点
├─ loadSessionHistoryMessages / loadSessionHistoryEvidence   ← §0.1 / §3（全文件读取 + 分支装载）
├─ 预扫描（全部纯内存，基于投影后 sourceMessages）：
│  ├─ originBySourceIndex        ← annotateOriginMessages（core/message-utils.ts）zip 回原始下标（:1504）
│  ├─ presentationBySourceIndex  ← MESSAGE_PRESENTATION_RECORD_TYPE → 其后第一条 user（:1525）
│  ├─ agentReviewBySourceIndex   ← AGENT_REVIEW_RECORD_TYPE(status=completed) → user（:1540）
│  ├─ runBoundsBySourceIndex     ← Run 边界预扫描（:1563；与主循环 displayIdx/latestTurnInputEntryId 语义逐字同源）
│  ├─ sessionCollabDecisions     ← collectSessionCollabDecisions（core/message-utils.ts）（:1629）
│  ├─ toolOutcomesByCallId       ← collectToolOutcomesByCallId（shared/tool-outcome.ts）（:1630）
│  ├─ modelCallReferenceBySourceIndex ← collectModelCallReferencesBySourceIndex（core/message-utils.ts）（:1631）
│  ├─ toolResultSourceIndexByCallId   ← 本地循环（:1633）
│  ├─ sourceIndexByEntryId / displayIndexByEntryId（:1642-1654）
│  └─ turnInputConsumption* / turnInputByAssistantEntryId ← parseTurnInputConsumptionRecord（lib/turn-input-presentation.ts）（:1655）
├─ resolveHistoryPageBounds(sourceMessages, {beforeId, limit, forceAll})   [sessions.ts]
│    total = isDisplayableHistoryMessage 计数；before = display 序号边界；startIdx=max(0,end-limit)；
│    hasMore = startIdx>0；limit clamp 50/200；forceAll → [0,total)（§2）
├─ 主循环 for sourceIndex in sourceMessages（:1788）：
│  ├─ user：更新 latestTurnInputEntryId/Visible（isHiddenTurnInputMessage，lib/turn-input-presentation.ts）；
│  │        页内 → extractTextContent → filterUnreferencedInlineImages → shouldDeferHistoryContent?
│  │        → createHistoryDeferredContent("inline_image")（server/history-deferred-content.ts）；
│  │        sanitizeVisibleContent（stripSessionReminderBlocks + isBridgeSessionPath→sanitizeBridgeVisibleText）；
│  │        并 origin / agentReview / presentation / clientMessageId(§7 desktop-input-correlation)（:1790-1840）
│  ├─ assistant：turnInput 回填（consumption 记录优先）、extractTextContent(stripThink)、
│  │        extractPersistedAssistantSemanticSegments（shared/assistant-semantic-segments.ts）；
│  │        toolUses → toolOutcomesByCallId + toolResultSourceIndexByCallId → soleRawToolResultText
│  │        → shouldDeferHistoryContent → createHistoryDeferredContent("tool_output"/"skill_content")；
│  │        附 turnStartIndex/turnEndIndex（Run 边界）、modelCallRef、turnStatus、thinking（:1841-1950）
│  ├─ toolResult：extractBlocks（server/block-extractors.ts）→ overlaySessionCollabDecision（core/message-utils.ts）
│  │        → deferHeavyHistoryBlock（sessions.ts：screenshot/artifact → createHistoryDeferredContent）（:1951-1964）
│  └─ custom：turn input / loop 指针更新；extractBlocks + deferHeavyHistoryBlock；
│           parseHistoryDeferredResult → recordMediaGenerationResult / recordTurnInputConsumptionInterlude /
│           recordTurnInputPresentationInterlude（parseTurnInputPresentationRecord）/ recordDeferredInterlude
│           （deferredStore.query + engine.subagentRuns.query 外部状态）/ recordLoopInterlude（:1965-2011）
├─ 主循环后外部状态补齐：
│  ├─ deferredStore.listBySession(sessionPath) 终态任务回灌（:2014）
│  ├─ resolveMediaGenerationBlocks（server/block-extractors.ts）（:2022）
│  ├─ 页切片：blocks 过滤 afterIndex ∈ [startIdx,endIdx)，afterIndex -= startIdx；forceAll 直通（:2029）
│  ├─ subagent 块终态：deferredResults.query + subagentRuns.query + createSubagentMetaCache
│  │        （engine.getSessionExecutorMetadata 或 readSubagentSessionMetaSync —— 同步读 session-meta.json）
│  │        + createSubagentSummaryCache → loadLatestAssistantSummaryFromSessionFile
│  │        （core/message-utils.ts，≤256KiB 有界尾读）（:2042-2118）
│  ├─ workflow 块终态：subagentRuns/deferredResults 回填 finishedAt/summary（:2123）
│  ├─ patchSessionFileLifecycleBlocks（registry 查询，§6）（:2144）
│  ├─ listSessionRegistryFiles → sessionFiles 字段（§6）（:2145）
│  ├─ extractLatestTodos(sourceMessages)（lib/tools/todo-compat.ts）（:2149）
│  ├─ engine.activityHub.rebroadcastSession（!reconciling && beforeId==null 时，WS 广播副作用）（:2154）
│  └─ reconciling：readDesktopInputRunSnapshot after + reconciliation 摘要（§3）（:2158）
└─ 响应：{ messages, blocks: slicedBlocks, todos, hasMore, nextBefore: String(startIdx), sessionFiles, revision, reconciliation? }（:2167）
```

关键事实：

| 项 | 位置/结论 |
| --- | --- |
| 全文件读取 | `loadSessionHistoryMessages` 链（§0.1）：repair 同步全读 + SDK 同步全量装载；回退路径异步全读 |
| 同步 I/O | repair（读+可能写）、SDK open、manifest SQLite 读、`readSubagentSessionMetaSync`、registry sidecar 首次水化 |
| 完整分支校验 | `applyStoredSessionBranchHead → projectCurrentSessionBranchEntries`（每次冷开都校验）；**回退全读路径无校验** |
| 隐式修复 | `repairOversizedSessionEntriesInFile`（可重写会话文件 + 备份 `.repair.json`）；`setBranchHead`（manifest 写回）；投影层 legacy isError 修补 |
| 分页语义 | `before` 是服务端 display 序号；`nextBefore = String(startIdx)`；空页仍返回 hasMore/nextBefore（前端据此推进） |
| 块坐标 | block.afterIndex = 挂靠消息的 display 序号；响应里已重映射为页内偏移（forceAll 除外） |

---

## 2. all=1 模式（强制全量）

与 §1 完全同一条路由与主循环，仅分页参数不同：

- 入口参数：`c.req.query("all") === "1"` → `forceAll=true`（`server/routes/sessions.ts`，`resolveHistoryPageBounds` 调用点 :1608-1609）。
- `resolveHistoryPageBounds`：`forceAll → { total, startIdx: 0, endIdx: total, hasMore: false }` —— 主循环仍全量遍历 sourceMessages，只是窗口为整段；`hasMore=false`、`nextBefore=null`。
- 块切片：`slicedBlocks = resolvedBlocks` 直通（:2029，不重映射 afterIndex）。
- 读取成本与 §1 相同（每次仍 repair + SDK 全量装载）；**没有**单独的免 hydrate 通道。
- 已知生产调用方：
  - `desktop/src/react/settings/tabs/observability/trace-detail/TraceDetailOverlay.tsx`（轨迹详情全量恢复，:61 拼 `&all=1`）；
  - 测试：`tests/history-pagination-run-continuity.test.ts`、`tests/session-find-route.test.ts`（作为分页等价性参照）。
- 语义固定：任务书 E 阶段前不新增通道、不改 all=1 行为（TASKBOOK I12）。

---

## 3. reconciliation=1 严格对账模式

路由入口同 §1；`reconciling = c.req.query('reconciliation') === '1'`（:1488）。

```
route.get("/sessions/messages?reconciliation=1")
├─ readDesktopInputRunSnapshot(engine, reconciliationSessionId, resolvedSessionPath)   [core/desktop-session-submit.ts]
│    beforeRun = {revision, status: running|reconciled_idle|unknown}（纯内存：isSessionStreaming + pending 提交表）
├─ readSessionFileRevision(...)（同 §1）
├─ loadSessionHistoryEvidence(engine, sessionPath, sessionId)   [core/message-utils.ts]  ← 严格路径，不走 §0.1
│  ├─ 身份链校验（任一失败 → unavailable(diagnostic)，messages=[]、complete=false）：
│  │    sessionId 可得 + engine.getSessionBranchHead 存在
│  │    → getSessionManifest：manifest.sessionId / currentLocator.path / getSessionIdForPath 三方一致
│  │    → getSessionBranchHead(sessionId)（core/session-manifest/store.ts，SQLite 同步读）
│  │    → branchHead.sessionId === sessionId
│  ├─ fs.readFile(sessionPath, 'utf8')（异步全文件读；不 repair、不猜分支）
│  ├─ 单一 type:"session" 头 + string id + version===3（文件头是 SDK UUID，与业务 sessionId 独立）
│  ├─ 运行时实例校验：engine.getSessionByPath(...).sessionManager 的 file/id 与当前路径、文件头一致
│  ├─ 读后再校验：manifest locator 未移动 + getSessionBranchHead 的 leafId/observedTailLeafId 未变（TOCTOU 双读）
│  ├─ projectCurrentSessionBranchEntries(entries, {branchHead, filePath})   ← 完整分支校验（§0.2）
│  │    抛 SessionBranchError → unavailable('branch_read_unverified')；legacySyntheticIds → 'legacy_branch_unverified'
│  ├─ lineage → byId 取回 entry → projectBranchHistory(branch, sessionId)（含 desktop-input-correlation、legacy isError 修补）
│  └─ 返回 { messages, complete: true, diagnostic: null }
├─ 主循环与各预扫描同 §1（输入是 evidence.messages；注意对账页 message 数为 0 时页面输出空）
├─ readDesktopInputRunSnapshot afterRun（:2158）
└─ reconciliation 载荷（:2159-2166）：
     { sessionId, sessionPath, snapshotId: randomUUID(), runRevision: afterRun.revision, complete: evidence.complete,
       runStatus: 前 Revision 不等或 !complete → 'unknown'；两侧均 running → 'running'；
       两侧均 reconciled_idle → 'reconciled_idle'；否则 'unknown', diagnostic? }
```

| 项 | 结论 |
| --- | --- |
| 全文件读取 | `loadSessionHistoryEvidence` 异步全读，一次；**无** repair / SDK open / 兼容回退 |
| 同步 I/O | manifest/branchHead SQLite 读（两次，读前读后各一）；`readDesktopInputRunSnapshot` 纯内存 |
| 完整分支校验 | `projectCurrentSessionBranchEntries` + `legacySyntheticIds` 拒绝 + 头部/身份双重校验 —— 五条路径中最严 |
| 隐式修复 | 无文件/manifest 写入（刻意不修复；任何不确定都降级为 `complete:false` + diagnostic） |
| 消费方 | `desktop/src/react/stores/session-actions.ts: fetchSessionHistoryPage`（连接捕获式只读对账，2MiB 上限，:151-184） |

---

## 4. 延迟内容（deferred content）

描述符生成：`server/history-deferred-content.ts`

- `shouldDeferHistoryContent(value)`：字符串长度 > `HISTORY_INLINE_CONTENT_LIMIT`（8 KiB）。
- `createHistoryDeferredContent(sourceMessages, sourceIndex, kind, ordinal, content, {preview})`：
  - locator = `{version: 1, sourceIndex, entryId, kind, ordinal}` → `encodeLocator` = base64url(JSON)；
  - kind ∈ `assistant_segment | tool_output | skill_content | screenshot | artifact | inline_image`；
  - descriptor = `{id, kind, size, preview?(240 字符), available: true}`；
  - 生成点全部在 §1 主循环：user inline image（:1804）、assistant reasoning segment（:1860）、tool_output/skill_content（:1885/:1896）、`deferHeavyHistoryBlock` 的 screenshot/artifact（sessions.ts:266）。
- 展开端点：`server/routes/sessions.ts: route.get("/sessions/content/:contentId", ...)`（:1392）
  1. `?sessionId=` → manifest locator，或 `?path=`；`isValidSessionPath` 校验；`authorizeSessionRoute("sessions.read")`。
  2. `loadSessionHistoryMessages(engine, resolvedSessionPath)` —— **完整重跑 §0.1 链（含 repair + SDK 全量装载）**，无任何页缓存/修订点短路。
  3. `resolveHistoryDeferredContent(sourceMessages, contentId)`（server/history-deferred-content.ts）：
     - `decodeLocator` 校验 version/kind/序号 → `sourceMessages[sourceIndex]`；
     - `entryId` 与 locator.entryId 不一致 → null（文件在读取窗口内被重写/分支切换时失效，安全失败）；
     - 按 kind 取内容：segment → `extractPersistedAssistantSemanticSegments[ordinal]`；tool_output/skill_content → `rawTextResult`（唯一 text 块）；inline_image → `extractTextContent`+`filterUnreferencedInlineImages[ordinal]`；screenshot/artifact → `extractBlocks(...)[ordinal]`。
  4. 命中返回 `{id, kind, content, mimeType?}`；未命中 404 `Historical content not found`。

| 项 | 结论 |
| --- | --- |
| 全文件读取 | 每次展开一次完整 §0.1 链（前端对展开结果有会话内缓存，见 §8） |
| 同步 I/O | 同 §1（repair/SDK/manifest） |
| 完整分支校验 | 同 §1（`applyStoredSessionBranchHead`）；entryId 绑定提供二次失效保护 |
| 隐式修复 | 同 §1（含 repair 重写可能性 —— 展开一张旧图也可能触发文件修复） |
| locator 稳定性 | sourceIndex+entryId 双重绑定；条目在物理文件中的序漂移由 entryId 兜底，重写后 descriptor 失效为 404 |

---

## 5. find：GET /api/sessions/find

路由：`server/routes/sessions.ts: route.get("/sessions/find", ...)`（:1134）。定位语义要求显式 `path`（无焦点回退）；`sessionId` 同样先经 manifest 解析。

```
route.get("/sessions/find")
├─ authorizeSessionRoute("sessions.read")
├─ query 长度上限 512（SESSION_SEARCH_QUERY_MAX_LENGTH）
├─ readSessionFileRevision(queryPath)（读前取修订点，:1168）
├─ findEntriesCache.get(queryPath)  [sessions.ts 模块级 Map，FIND_ENTRIES_CACHE_MAX=8，插入序淘汰]
│    revision 命中 → 直接用缓存 entries；revision 为 null（stat 失败）→ 不读不写缓存
│    未命中 →
│      loadSessionHistoryMessages(engine, queryPath)      ← 完整 §0.1 链
│      sanitize（stripSessionReminderBlocks + isBridgeSessionPath→sanitizeBridgeVisibleText）
│      collectFindableHistoryEntries(sourceMessages, sanitize)   [sessions.ts:322，导出供测试]
│        · displayIdx 语义与 messages 主循环逐字对齐（user/assistant + isDisplayableHistoryMessage）
│        · user：extractTextContent → 过滤 isHiddenTurnInputMessage → 剥 legacy steer 前缀/<t> 标签
│        · assistant：stripThink 后文本
│      写缓存 {revision, entries}
└─ findInSessionMessages(entries, query)   [lib/search/session-find.ts]
     normalizeSessionSearchText / tokenizeSessionSearchQuery（lib/search/session-search-tokenizer.ts）
     exact（包含完整 query，1000+ 分）与 token 命中（80+ 分）；MAX_MATCHES=500 截断标记 truncated
     → { query, revision, total, bestIndex, tokens, matches, truncated }（bestIndex 可能不在 matches 内）
```

| 项 | 结论 |
| --- | --- |
| 全文件读取 | 首次/修订点变化后一次 §0.1 全链；缓存命中为 0 I/O（revision 驱动失效，stat 除外） |
| 同步 I/O | 同 §1（首查）；缓存命中仅剩 `fs.stat`（异步） |
| 完整分支校验 | 同 §1；**缓存的 entries 是分支投影后的当前分支文本** |
| 隐式修复 | 同 §1（repair 链在首查时可达）；缓存本身是纯内存 |
| 序号契约 | `entry.index` = messages 路由的 display 序号（`id`），前端据此跳转（一致性由 tests/session-find-route.test.ts 固定） |
| 前端消费 | `desktop/src/react/stores/chat-find-actions.ts`（:24 拼 `/api/sessions/find?path&q`） |

---

## 6. sessionFiles 字段与 registry 调用链

调用点：`server/routes/sessions.ts: listSessionRegistryFiles(engine, resolvedSessionPath, sourceMessages)`（:2145，messages 路由主循环后；`activeReferences = sourceMessages` 即本次投影后的全部消息）。

```
listSessionRegistryFiles(engine, sessionPath, sourceMessages)
├─ engine.listSessionFiles(sessionPath, { references: sourceMessages })   [core/engine.ts]
│    └─ SessionFileRegistry.listReachable(sessionPath, references)        [lib/session-files/session-file-registry.ts]
│         ├─ list(sessionPath)：_idsBySession → _byId（内存索引）
│         ├─ collectSessionFileReferenceIdentities(references)（:983）
│         │    递归遍历消息对象 + 字符串文本：
│         │    · 对象：fileId / explicit type 下的 id / filePath / realPath / path
│         │    · 文本：SESSION_FILE_MARKER_RE（[hana-file …] JSON 标记）、ATTACHED_MEDIA_MARKER_RE（[attached_image: …]）
│         └─ sessionFileIsReachable(file, identities)：file.id/fileId/filePath/realPath/legacy* 命中即保留
├─ engine.serializeSessionFile 或 lib/session-files/session-file-response.ts: serializeSessionFile
└─ 过滤 null → sessionFiles 数组
```

- registry 数据源：进程内 `SessionFileRegistry`（engine `_sessionFiles`），按会话惰性水化自 sidecar `<session>.files.json`（`_hydrateSession` → `_readSidecar`：`existsSync` + `readFileSync`，首次触碰时一次；坏 schema 抛错）。
- `?references` 传投影消息而非分支 entries：`engine.listActiveSessionFiles`（engine.ts，用 `sessionManager.getBranch()` 或冷开）走的是同一 `listReachable`，但那是其余调用方（agent 侧）；messages 路由刻意用**投影后消息**做可达性来源。
- 前端入库：`session-actions.ts: loadMessages/loadMoreMessages` → `setSessionRegistryFiles` / hydrate flight 重放（issue #2188）→ `history-builder.ts: buildSessionFileLookup`（:234）把 registry 记录匹配回 user 消息内的文件标记/附件。

`patchSessionFileLifecycleBlocks`（sessions.ts:3185，messages 路由 :2144）：对 page 内 `file/artifact/skill/screenshot` 块用 `engine.getSessionFile(fileId)` / `getSessionFileByPath(filePath)`（registry 内存查询，可能触发首次 sidecar 水化）补 lifecycle 字段（status/missingAt/size/version…，`sessionFileLifecycleFields`）；screenshot 可经 `browserScreenshotPath`（lib/session-files/browser-screenshot-file.ts）反查并转 `file` 块。

---

## 7. 四个投影模块在主循环（及加载链）中的调用环节

| 模块 | 环节 | 具体位置 |
| --- | --- | --- |
| `core/desktop-input-correlation.ts`（`collectDesktopInputCorrelations`） | **加载时**（非主循环）：`core/message-utils.ts: projectBranchHistory` 对 branch entries 建 sourceEntryId→{clientMessageId, snapshotVersion, sourceEntryId} 映射（同客户端身份唯一性 → `acceptanceDiagnostic:'ambiguous'`）；主循环仅在 user 消息输出字段时消费（sessions.ts:1824 `clientMessageId/sourceEntryId/snapshotVersion`）。sessionId 为 `''`（manifest 不匹配）时空结果 | §0.1 + sessions.ts:1824 |
| `lib/turn-input-presentation.ts` | ① 预扫描：`parseTurnInputConsumptionRecord`（:1655-1670）建 `turnInputByAssistantEntryId` 等索引；② 主循环 user 分支：`isHiddenTurnInputMessage`（:1793）；③ assistant 分支：consumption 优先回填 turnInputEntryId/turnInputVisible（:1845-1849）；④ custom 分支：`isCustomTurnInputHistoryMessage`（:1966）、`recordTurnInputConsumptionInterlude`（:1986）、`recordTurnInputPresentationInterlude`→`parseTurnInputPresentationRecord`（:1989）；⑤ Run 边界预扫描把 custom turn input / LOOP_TURN 作为 Run 起点（:1580）。另在 `core/message-utils.ts: historyMessageFromEntry` 中消费/展示 custom 记录映射 | 见左 |
| `lib/tools/todo-compat.ts` | 主循环**之后**：`extractLatestTodos(sourceMessages)`（sessions.ts:2149）—— 对投影后的当前分支消息从后向前线性扫描（toolResult `TODO_TOOL_NAMES` + `TODO_STATE_CUSTOM_TYPE`），坏快照跳过；注释所称 branch-aware 由「sourceMessages 已是分支投影」间接成立；**兼容回退路径（无分支校验全读）下会扫到被抛弃分支**。独立分支感知实现 `extractLatestTodosFromEntries`（走 SDK `buildSessionContext`）仅用于 `core/session-turn-actions.ts:370` 等，不在本路由 | sessions.ts:2149 |
| `shared/tool-outcome.ts` | ① 加载时：`historyMessageFromEntry` → `isKnownLegacyLingxiToolFailure` 补 isError（内存投影修复）；② 预扫描：`collectToolOutcomesByCallId(sourceMessages)`（sessions.ts:1630，先建 assistant toolCall 上下文表再配对 toolResult）→ 主循环 assistant 分支消费（:1876 `projectedToolUses`），与 `toolResultSourceIndexByCallId`（:1878）一起做 tool_output/skill_content 延迟展开与 startedAt/endedAt | sessions.ts:1630/:1876 |

---

## 8. 前端链

### 8.1 session-actions（desktop/src/react/stores/session-actions.ts）

| 函数 | 行为 |
| --- | --- |
| `sessionMessagesUrl`（:186） | 拼 `/api/sessions/messages?path=&sessionId=(&before=)`；sessionId 来自 store 的 manifest 映射 |
| `fetchSessionHistoryPage`（:151) | 对账专用：捕获连接 URL+auth，`reconciliation=1`（+可选 before），2MiB 流式上限，返回 `{messages, hasMore, oldestId?, reconciliation?}` |
| `loadMessages`（:399) | 全量 hydrate：记录 message/todos live version 与 `_loadMessagesVersion`（stale 丢弃）→ `beginSessionFilesFlight`（#2188）→ fetch → 修订点 stamp（`initSession` 第 4 参）→ `setSessionRegistryFiles`（flight 重放）→ `buildItemsFromHistory(data, {openTailRun: stream buffer 有内容})` → `initSession(path, items, hasMore, revision, historyNextCursor(data))` → stream buffer 在途 assistant 追加。任一 live version 变化则整份快照丢弃（todos 与 messages 同生共死） |
| `loadMoreMessages`（:551) | `before = session.nextBefore ?? session.oldestId`（服务端游标优先，F1）→ fetch → `setSessionRegistryFiles` → `buildItemsFromHistory` → 游标缺失诊断终止（:574）→ `prependItems` |
| revision 补拉（:587 起） | 对比 chatSessions 缓存 revision vs 列表投影 revision（issue #1610）；per-session in-flight 去重 `_revisionReconcileInFlight`；流式进行中不补拉 |

`historyNextCursor`（:389）：取服务端 `nextBefore`（原始 display 序号），不从前端归并项 id 反推（F1 修复的核心不变量）。

### 8.2 chat-slice（desktop/src/react/stores/chat-slice.ts）

- `initSession`（:134）：整表替换 items + `hasMore` + `nextBefore` + `revision`；`oldestId` 仅作旧消费者兼容的显示项身份（:145）。
- `prependItems`（:168）：调 `mergePrependedHistoryItems`（history-run-merge）幂等合并，更新 `nextBefore/oldestId/hasMore`；空页也推进游标（T11b）。
- reset/清空路径（:204, :219, :491）负责 revision/游标清位。

### 8.3 history-builder（desktop/src/react/utils/history-builder.ts）

- `buildItemsFromHistory`（:628）：blocks 按 `afterIndex` 分组（`normalizeBlocks` :149）→ user 附件解析（`parseUserAttachments` + `buildSessionFileLookup` :234 匹配 sessionFiles）→ assistant **Run 分组**：`sameHistoryRun`（:604，优先 `turnStartIndex/turnEndIndex` 相等，旧服务端回退 `turnInputEntryId`+segments）→ `coversRunHead/ownsRunTail` 判定 → 截断 Run 携带 `runFacts{runKey: "start:end", records}`（:799）→ `projectHistoryRunRecords`（:499）/`projectHistoryRunFromFacts`（:588）投影 → `sourceOrderedItems`（:448）按 sourceIndex/afterIndex 归位。
- `openTailRun` 选项：流式进行中最新 Run 不派生终态（T06）。

### 8.4 history-run-merge（desktop/src/react/utils/history-run-merge.ts）

- `mergePrependedHistoryItems`（:63）：按显示身份去重 → 识别 incoming 末尾带 runFacts 的片段 ↔ existing 首个同 `runKey` 项 → `mergeRunFacts`（:27，displayId 并集、元数据随新头部）→ `projectHistoryRunFromFacts` 重投影替换（位置与 id 不变）；头部到达（min displayId === turnStartIndex）后丢弃 runFacts。

### 8.5 延迟内容前端

- `desktop/src/react/hooks/use-deferred-history-content.ts`：`loadDeferredHistoryContent`（:36）GET `/api/sessions/content/:id?path=`，模块级 `resolvedContent/pendingContent` Map 按 `sessionPath\0id` 缓存去重；`asDeferredHistoryContent`（:21）校验 descriptor 形状。
- 消费组件：`ToolGroupBlock.tsx`（:294、:356 展开时请求）、`ThinkingBlock.tsx`（:22 打开时）、`UserMessage.tsx`（:569）、`AssistantMessage.tsx`（:866、:925）。

---

## 9. 汇总

### 9.1 全文件读取点汇总

| # | 位置（文件:函数） | 触发路径 | 方式 | 备注 |
| --- | --- | --- | --- | --- |
| 1 | `core/session-jsonl-file.ts: repairOversizedSessionEntriesInFile` | messages/all/content/find（`loadSessionHistoryMessages` 正常链） | **同步** readFileSync | 每次请求都全读一次，即使无需修复 |
| 2 | `@earendil-works/pi-coding-agent …/session-manager.js: SessionManager.open → loadEntriesFromFile` | 同上 | **同步** openSync/readSync | 构造函数全量装载；header 有界扫描超限时也是全量 |
| 3 | `core/message-utils.ts: loadSessionHistoryMessages` 回退块 | 正常链投影失败/非 Pi 文件 | 异步 fs.readFile | 无分支校验的线性全读 |
| 4 | `core/message-utils.ts: loadSessionHistoryEvidence` | reconciliation=1 | 异步 fs.readFile | 严格读，无修复无回退 |
| 5 | `core/session-list-projection-cache.ts: buildSessionProjection`（经 `SessionListProjectionCache.list`） | 列表/搜索（GET /api/sessions、/sessions/search） | 异步 fs.readFile | 仅签名变化时；坏行跳过 |
| 6 | `core/message-utils.ts: loadLatestAssistantSummaryFromSessionFile`（`readSessionTailUtf8`） | messages 路由 subagent summary 补齐 | 异步尾读 ≤256KiB | 文件小于阈值时等价全读 |
| 7 | `server/history-deferred-content.ts` 消费端：`/sessions/content/:contentId` → `loadSessionHistoryMessages` | 每次 deferred 展开 | 同 #1+#2 | 每次展开整链重读，无缓存 |

（`lib/session-jsonl.ts: readCurrentSessionBranch/readSessionMessages` 的同步全读在 memory-ticker / diary / hub 等读取方，不在本五条 HTTP 路径内，属相邻链路。）

### 9.2 同步 I/O 汇总（五条读取路径内可达）

| 位置 | 操作 | 路径 |
| --- | --- | --- |
| `core/session-jsonl-file.ts: repairOversizedSessionEntriesInFile` | readFileSync；条件性 copyFileSync（`.repair.json`）+ writeFileSync 整文件重写 | messages/all/content/find |
| SDK `SessionManager.open`/`loadEntriesFromFile`/`readSessionHeader` | 同步文件读 | messages/all/content/find |
| `core/session-manifest/store.ts`（better-sqlite3，`getBranchHead`/`getBySessionId`/`setBranchHead`） | 同步 SQLite 读写 | 全部五条（身份解析/branchHead）；messages 冷开可能写 |
| `lib/subagent-executor-metadata.ts: readSubagentSessionMetaSync` | readFileSync（session-meta.json；每请求经 `createSubagentMetaCache` 去重） | messages/all（subagent 块补齐） |
| `lib/session-files/session-file-registry.ts: _readSidecar/_hydrateSession` | existsSync + readFileSync（首次触碰） | messages/all（`patchSessionFileLifecycleBlocks`、`listSessionFiles`） |
| `core/session-coordinator.ts: listSessions → _readMetaCached/_loadSessionTitlesFor` | 会话 meta/titles 读取 | 列表/搜索相邻链路 |

异步 I/O（对照）：`looksLikePiSessionFile`（512B 头读）、`readSessionFileRevision`（stat）、`loadSessionHistoryMessages` 回退、`loadSessionHistoryEvidence`、投影缓存 list/stat、`loadLatestAssistantSummaryFromSessionFile`。

### 9.3 隐式修复 / 副作用点汇总

| 类型 | 位置 | 说明 |
| --- | --- | --- |
| **文件重写** | `core/session-jsonl-file.ts: repairOversizedSessionEntriesInFile`（由 `loadSessionHistoryMessages` 触发） | 历史**读**请求可重写会话 JSONL：剥离超限行内媒体/投影超限条目并回写，原文件备份为 `<session>.jsonl.repair.json`；坏行被静默丢弃（`skipped` 计数）。all=1/content/find 同样可达 |
| **manifest 写回** | `core/session-branch-head.ts: applyStoredSessionBranchHead` → `setBranchHead`（messages/all/content/find 冷开时）；`readManifestSessionBranch`（persistRecovery，相邻链路） | 读时观察到 append_recovery / tail 前进 / legacy backfill 即把 recommendedHead 写入 SQLite manifest —— GET 请求带写副作用 |
| 投影层修复（只读） | `core/message-utils.ts: historyMessageFromEntry`（legacy 工具失败补 isError，`shared/tool-outcome.ts`）；`buildValidatedEntryIndex` legacy 合成 id（只读兼容投影） | 不落盘 |
| 读容错（吞错） | SDK `loadEntriesFromFile` 跳坏行；`loadSessionHistoryMessages` 回退跳坏行；投影缓存 `buildSessionProjection` 跳坏行 | 与严格路径（`parseFullSessionEntries` 抛错 / evidence 拒绝）形成对比；回退路径还会把抛弃分支计入展示与 todos 扫描 |
| WS 广播副作用 | `server/routes/sessions.ts` :2154 `engine.activityHub.rebroadcastSession`（首屏 hydrate、非对账时） | 读请求触发外部事件广播 |
| 内存缓存写入 | find `findEntriesCache`；`SessionListProjectionCache._dirs`；deferred 前端 `resolvedContent` | 修订点/签名驱动失效；revision=null 不缓存 |
| 响应对象原位改写 | `patchSessionFileLifecycleBlocks`（Object.assign 到 block）、subagent/workflow 终态回填（:2042-2142） | 只改响应副本，无持久化 |

### 9.4 对后续步骤（A03+）最要紧的结论

1. **读取热路径每次请求至少两次全文件同步读**（repair + SDK 装载），且 repair 在读路径上具备整文件重写能力——安全快路径设计必须先决定 repair 的去留/条件化。
2. 完整分支校验只在 `projectCurrentSessionBranchEntries` 一处实现，普通路径经由 `applyStoredSessionBranchHead` 每次冷开都跑一遍；而**兼容回退完全绕过它**（无校验 + 含抛弃分支），这是「正常路径 fallback 次数为 0」断言（A03）要钉住的分叉点。
3. 延迟 content 展开没有缓存通道，每次点击都重跑整链，是分页优化后最容易退化的次级端点。
4. 分页/Run 缝合的服务端事实源是 `runBoundsBySourceIndex`（预扫描）与主循环 `latestTurnInputEntryId` 指针，两处必须同步修改（源码注释已声明）；前端 `history-run-merge` 只信任 `runKey=turnStartIndex:turnEndIndex`。
