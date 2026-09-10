# dependency-matrix.md — GET /api/sessions/messages 逐字段依赖矩阵（A02）

- 基准：HEAD `1d42b7405c76292f617291e3a01cd2f3ef5efd04`，分支工作区（只读分析，未改生产代码）。
- 主入口：`server/routes/sessions.ts:1466` `route.get("/sessions/messages")`；主展示循环 `sessions.ts:1788-2012`；响应组装 `sessions.ts:2167`。
- 阅读底座：`loadSessionHistoryMessages`（`core/message-utils.ts:118`）→ `repairOversizedSessionEntriesInFile`（隐式修复）→ `engine.openSessionManagerAtCurrentBranch`（`core/session-coordinator.ts:1324`，会触发 `applyStoredSessionBranchHead` 的隐式 `setBranchHead`）→ `SessionManager.getBranch()` → `projectBranchHistory`（`core/message-utils.ts:161`）。
- 坐标体系（全文通用）：
  - `sourceIndex`：`sourceMessages` 数组下标 = 当前分支 parent 链上的物理条目顺序（response 中 `messages[].sourceIndex`、`blocks[].sourceIndex`、deferred locator 内嵌）。
  - display 序号（`messages[].id = String(displayIdx)`）：仅 user/assistant 且 `isDisplayableHistoryMessage`（`sessions.ts:294`）为真时推进；分页窗口 `[startIdx, endIdx)` 按它切（`resolveHistoryPageBounds`，`sessions.ts:362`）。
  - `entryId`：JSONL entry.id（`historyMessageFromEntry`，`core/message-utils.ts:203` 拷到 message.id）。
  - `afterIndex`：块锚点的 display 序号；非 all 模式下重映射为页内偏移（`sessions.ts:2029-2033`），因此页内 `afterIndex === messages` 数组位置。
- 列含义：**页外依赖**=字段取值是否需要页面窗口 `[startIdx,endIdx)` 之外的记录/store；**追加影响**=文件尾部追加新记录后，旧页面重新拉取时该字段如何变化；**外部可变**=是否依赖 JSONL 之外的 store。

## 1. origin（跨会话协作来源）

| 响应字段 | 来源（文件:函数） | 原始坐标 | 关联键 | 页外依赖 | 追加影响 | 外部可变 |
|---|---|---|---|---|---|---|
| `messages[user].origin`、`displayText` | `core/message-utils.ts:annotateOriginMessages`(293) + 路由 zip 回映射 `sessions.ts:1504-1524` | `hana-message-origin` custom 条目（`customType="hana-message-origin"`，`core/desktop-session-submit.ts:61`，写入点 `recordMessageOriginEntry`:265）→ 注释其后第一条 user | sourceIndex（origin 条目与 user 条目） | 是：pending 指针从 origin 条目跨到下一条 user，二者都可能分属窗口两侧（含中间 agentReview/presentation 条目被跳过不清指针） | 尾部追加不影响旧 user 的 origin（指针只向后找最近 user，旧配对稳定）；仅当追加发生在「origin 之后、user 之前」的空隙才改变归属（正常写入序列不会） | 否（纯 JSONL） |

## 2. presentation（展示层投影：displayText/skills/sessionRefs/agentMentions/knowledgeRefs/knowledgeRetrieval/agentReviewRequest）

| 响应字段 | 来源 | 原始坐标 | 关联键 | 页外依赖 | 追加影响 | 外部可变 |
|---|---|---|---|---|---|---|
| `messages[user].displayText / skills / sessionRefs / agentMentions / knowledgeRefs / knowledgeRetrieval / agentReviewRequest` | 路由内联扫描 `sessions.ts:1525-1539`（`MESSAGE_PRESENTATION_RECORD_TYPE="hana-message-presentation"`）；写入点 `core/desktop-session-submit.ts:recordMessagePresentationEntry`(311) | presentation custom 条目 → 其后第一条 user | sourceIndex 对 | 是：同 origin，pending 指针可跨窗口边界 | 同 origin，旧页稳定 | 否 |
| `displayText` 三源覆盖顺序 | `sessions.ts:1828-1832` | origin → agentReview → presentation 依次 spread | — | 同上 | — | 否 |

## 3. review（agentReview，含「仅 completed 生效」规则）

| 响应字段 | 来源 | 原始坐标 | 关联键 | 页外依赖 | 追加影响 | 外部可变 |
|---|---|---|---|---|---|---|
| `messages[user].agentReview`、`displayText` | 路由内联扫描 `sessions.ts:1540-1554`；写入点 `recordAgentReviewEntry`（`core/desktop-session-submit.ts:288`） | `hana-agent-review-result` custom 条目 → 其后第一条 user；**仅 `data.status==="completed"` 才下发** | sourceIndex 对 | 是（同 origin 指针语义） | 同 origin | 否 |

## 4. 客户端输入 correlation（desktop-input-correlation）

| 响应字段 | 来源 | 原始坐标 | 关联键 | 页外依赖 | 追加影响 | 外部可变 |
|---|---|---|---|---|---|---|
| `messages[user].clientMessageId / sourceEntryId / snapshotVersion` | `core/desktop-input-correlation.ts:collectDesktopInputCorrelations`(4)，由 `projectBranchHistory`（`core/message-utils.ts:161-166`）合并进 user 消息；写入点 `core/desktop-session-submit.ts:withInputCorrelation`(102-135) `appendCustomEntry(DESKTOP_INPUT_CORRELATION_TYPE)` | `lingxi-desktop-input-correlation` custom 条目 → `data.sourceEntryId` 指向 user entry | `sourceEntryId`（user entryId）；有效性要求 `data.sessionId === 业务 sessionId` 且 sourceEntryId 在本分支 users 集合内 | **是（全局）**：`byClient` 跨整条分支统计「同一 clientMessageId 是否对应多个 user entry」，歧义判定无法在窗口内完成；correlation 条目物理上也可能在窗口外 | 尾部追加新 correlation/new user 不改变既有配对；但若追加造成同一 clientMessageId 第二次出现，**旧 user 的字段会从正常值变成 `acceptanceDiagnostic:'ambiguous'`**（旧页被追溯改写） | 否 |
| `messages[user].acceptanceDiagnostic:'ambiguous'` | `core/desktop-input-correlation.ts:19-26`：同一 sourceEntryId 出现 ≥2 个不同 `(clientMessageId,snapshotVersion)` 身份，或同一 clientMessageId 映射 ≥2 个 sourceEntryId → ambiguous | — | clientMessageId+snapshotVersion | 全局（同上） | 同上 | 否 |

注意：fallback 读取路径（非 Pi 文件/损坏文件，`loadSessionHistoryMessages` 的 raw 分支）**不做** correlation 合并，只做 reminder 投影——同一文件两条读路径字段集不同。

## 5. turn input（latestTurnInputEntryId / visible / assistantOrdinal 页首投影上下文）

| 响应字段 | 来源 | 原始坐标 | 关联键 | 页外依赖 | 追加影响 | 外部可变 |
|---|---|---|---|---|---|---|
| `messages[assistant].turnInputEntryId` | 主循环指针 `sessions.ts:1781,1790-1793,1966-1969`；消费覆盖 `turnInputByAssistantEntryId`（`sessions.ts:1655-1670` + `1845-1848`），解析 `lib/turn-input-presentation.ts:parseTurnInputConsumptionRecord`(134) | 指针=最近一个 turn 输入边界（user 消息 / `isCustomTurnInputHistoryMessage` 的 custom_message / `loop-turn` 置 null）；消费记录 `turn_input_consumption` custom 条目内 `input.entryId`→`assistant.entryId` | entryId（输入 entry、assistant entry）、deliveryId | **是**：①指针可指向任意早的输入（长 Run 跨页时窗口起点之前的 user）；②消费记录映射对全数组扫描构建 | 尾部追加新输入不影响旧 assistant 的绑定；追加新消费记录会覆盖 `turnInputEntryId` 并强制 `turnInputVisible:false`（旧页追溯改写） | 否 |
| `messages[assistant].turnInputVisible` | `sessions.ts:1786,1793,1849,1939-1943`；隐藏判定 `lib/turn-input-presentation.ts:isHiddenTurnInputMessage`(22)（`<hana-background-result`/`<hana-deferred-tasks>` 文本） | 初始 `true`（角色卡开场白）；隐藏 user/隐藏 custom 输入/loop 轮置 `false`；消费绑定恒 `false` | 同上 | 是（同上）；且「轮内尚无可见输入也必须显式下发 false」是跨记录状态 | 同上 | 否 |
| `assistantSegments[].id`（内嵌 assistantOrdinal） | `extractPersistedAssistantSemanticSegments(m.content, assistantOrdinalInTurn)`（`sessions.ts:1854-1857`；`shared/assistant-semantic-segments.ts:21`） | `assistantOrdinalInTurn`：user/custom 输入清零、**每条 assistant（含不可见）+1**（`sessions.ts:1791,1842,1967`） | Run 内序号 | 是：同一 Run 内此前所有 assistant（含被分页切走、不可见的）都参与计数 | 追加在尾部不影响旧 Run 的序号 | 否 |

## 6. modelCallReference（前置事件注释）

| 响应字段 | 来源 | 原始坐标 | 关联键 | 页外依赖 | 追加影响 | 外部可变 |
|---|---|---|---|---|---|---|
| `messages[assistant].modelCallRef`（modelCallId/traceId/parentCallId） | `core/message-utils.ts:collectModelCallReferencesBySourceIndex`(266)；写入点 `lib/llm/model-call-correlation.ts:26,76`（`hana-model-call-reference-v1`） | custom 条目 → **其后第一条 assistant**（user 清 pending；schemaVersion!==1 丢弃） | sourceIndex 对；modelCallId/parentCallId 为观测链键 | 是：reference 条目若紧贴页首 assistant 之前，则落在窗口外 | 旧 assistant 的注释稳定（相邻关系固定） | 否 |

## 7. Run 边界（turnStartIndex / turnEndIndex，history-run-merge 相关）

| 响应字段 | 来源 | 原始坐标 | 关联键 | 页外依赖 | 追加影响 | 外部可变 |
|---|---|---|---|---|---|---|
| `messages[assistant].turnStartIndex / turnEndIndex` | 预扫描 `sessions.ts:1555-1595`（displayCounter 与主循环同源）；输入边界=user 消息（含隐藏，只要 displayable 即占号）/ custom turn 输入 / `loop-turn` | 全局 display 序号区间 `[run 起点, run 尾 assistant]`；`runKey = "${turnStartIndex}:${turnEndIndex}"`（client 侧 `desktop/src/react/utils/history-builder.ts:801`） | runOrdinal；display 序号 | **是（最强制）**：Run 的输入记录与首条 assistant 常在窗口外；客户端缝合（`desktop/src/react/utils/history-run-merge.ts:mergePrependedHistoryItems`:63、`projectHistoryRunFromFacts`:588）要求拿到同 Run 全部记录才能派生终态（`missing_final_answer` 只能来自 Run 尾） | 尾部追加推进最新 Run 的 `turnEndIndex`，只影响未完成 Run 的旧页片段（重取后缝合结果变化）；已闭合 Run 的区间不变 | 否 |
| 客户端 `runFacts` / ownsRunTail / coversRunHead | `history-builder.ts:741-809` | — | displayId（=服务端 display 序号字符串） | 是：终态裁决权在 Run 尾记录，跨页 | 同上 | 否 |

## 8. assistant 主体（content/thinking/segments/turnStatus/timestamp）

| 响应字段 | 来源 | 原始坐标 | 关联键 | 页外依赖 | 追加影响 | 外部可变 |
|---|---|---|---|---|---|---|
| `content` | `extractTextContent(stripThink)`（`core/message-utils.ts:55`）→ `sanitizeVisibleContent`（`sessions.ts:1596-1601`：`stripSessionReminderBlocks` + bridge 会话加 `sanitizeBridgeVisibleText`） | 本条 assistant | — | 否 | 尾部追加不影响 | 否（纯投影） |
| `thinking`（超 8KiB 换 deferred 预览） | `sessions.ts:1944-1946` + `server/history-deferred-content.ts:shouldDeferHistoryContent`(76) | 本条 | deferred locator（sourceIndex+entryId） | deferred 解析需整文件重读（见 §16） | 否 | 否 |
| `assistantSegments`（reasoning 超限换 deferred） | `sessions.ts:1854-1868` | 本条；ordinal=段在 content 数组位置 | deferred locator | 同上 | 否 | 否 |
| `turnStatus`（failed/aborted，completed 不下发） | `sessions.ts:1869-1873`（`m.stopReason`） | 本条 | — | 否 | 否 | 否 |
| `timestamp`、`startedAt` | `sessions.ts:1848,1920,1948` | 本条 entry | — | 否 | 否 | 否 |
| `images`（user，未引用过滤+deferred） | `sessions.ts:1798-1813`；`filterUnreferencedInlineImages`（`core/message-utils.ts:105`，按 `[attached_image:]` marker 数扣减） | 本条 user | marker 计数在同条文本内 | 否 | 否 | 否 |

Reminder 语义：`stripSessionReminderBlocks`（`core/session-reminders.ts:119`）在 `projectSessionMessageForDisplay`(196)（读取层）与路由 `sanitizeVisibleContent`（展示层）双重执行；`[hana_reminder…]`/`[hana_reference]`/`[KnowledgeContext]` 整块剥离，未闭合块剥离到文末（fail-closed）。剥离只改用户可见投影，不改 JSONL、不影响 display 序号判定（`isDisplayableHistoryMessage` 看原始文本，剥离后可能变空的隐藏 user 依旧占号）。

## 9. toolCalls / toolResult 结局

| 响应字段 | 来源 | 原始坐标 | 关联键 | 页外依赖 | 追加影响 | 外部可变 |
|---|---|---|---|---|---|---|
| `messages[assistant].toolCalls[]` | `extractTextContent` 的 toolUses（`core/message-utils.ts:79-92`，processOrder=content 内位置） | 本条 assistant content 内 tool_use/toolCall 块 | `toolCallId`（block.id） | — | — | — |
| `toolCalls[].status/success/error/details` | `shared/tool-outcome.ts:collectToolOutcomesByCallId`(214)（`sessions.ts:1630,1875-1876`）；结局投影 `projectToolResultOutcome`(180)；旧格式失败升级 `isKnownLegacyLingxiToolFailure`(145)（读取层也在 `historyMessageFromEntry:204-209` 把 legacy 失败改写 `isError:true`） | toolResult 条目（物理上通常紧随 assistant，但配对按 **全数组** callId 索引 `sessions.ts:1632-1641`） | `toolCallId`；上下文来自 assistant content 的 tool_use args（`toolCallContextById`, tool-outcome.ts:196） | **是（前向）**：assistant 在页内、toolResult 在页外（正常相邻，但闭包设计必须允许 j>i） | toolResult 追加后，旧页 assistant 的工具卡从 `status:"unknown",success:false` 变为真实结局（追溯改写） | 否 |
| `toolCalls[].endedAt`、`details.output/outputDeferred`、`details.skillInvocation` | `sessions.ts:1877-1923`：endedAt=toolResult 条目 timestamp；超限 output/skill 内容换 deferred（`soleRawToolResultText` sessions.ts:260） | toolResult 条目 | toolCallId→sourceIndex | 是（同上） | 同上 | 否 |

## 10. collab 协作决定（草稿卡状态覆盖）

| 响应字段 | 来源 | 原始坐标 | 关联键 | 页外依赖 | 追加影响 | 外部可变 |
|---|---|---|---|---|---|---|
| `blocks[suggestion_card].status / resultSessionId` | `collectSessionCollabDecisions`（`core/message-utils.ts:328`）+ `overlaySessionCollabDecision`(343)（路由 `sessions.ts:1629,1957`）；记录写入 `lib/session-collab/decision-record.ts:3`（`hana-session-collab-decision`） | 决策 custom 条目（全数组扫描，按 `suggestionId` 建 map，后者覆盖前者）→ 覆盖 **任意更早** toolResult 产出的 suggestion_card 块 | `suggestionId` | **是（前向全局）**：决策记录几乎总在占位卡之后的页 | 用户确认后旧页草稿卡从 pending 变 confirmed/rejected（旧页追溯改写） | 否（历史面只用 JSONL 决策记录；实时面另有 SessionCollabDraftStore，不进本响应） |

## 11. media 媒体记录与最终状态

| 响应字段 | 来源 | 原始坐标 | 关联键 | 页外依赖 | 追加影响 | 外部可变 |
|---|---|---|---|---|---|---|
| `blocks[media_generation]`（pending 占位） | `server/block-extractors.ts:extractMediaGenerationBlocks`(261)（`media_generate-image/video/speech`、`mcp_call`） | toolResult details.mediaGeneration.tasks | `taskId`、`batchId` | — | — | — |
| media 终态（成功→file 块替换 / 失败→fallback） | `resolveMediaGenerationBlocks`（`block-extractors.ts:349`，路由 `sessions.ts:2022-2026`）；结果收集：①JSONL `hana-deferred-result` 记录与 `hana-background-result` custom_message（`parseHistoryDeferredResult`，`sessions.ts:3231`；`lib/deferred-result-notification.ts:97,113,127`）②**deferred store** 终态任务（`sessions.ts:2014-2021` `deferredStore.listBySession`，store=`lib/deferred-result-store.ts`，持久化 `~/.ephemeral/deferred-tasks.json`，接线 `server/index.ts:475`） | 结果记录/任务可在占位块之后任意远处（media 成功结果还带 `afterIndex=pageBounds.total-1` 的 standalone 注入） | `taskId` | **是（前向到分支尾）+外部 store**：占位块在页首、结果在尾部/内存 store 是常态 | 媒体完成/失败后，旧页占位块被替换为文件块或失败态（旧页追溯改写） | **是**：deferredResults store（可被另一进程/清理计时器改写） |
| 替换出的 `file` 块 lifecycle 字段 | `resultSessionFileBlocks`（block-extractors.ts:283）→ §14 的 registry patch | — | fileId/filePath | 是（registry） | registry 状态变化会改写 | **是** |

## 12. subagent / workflow 外部状态

| 响应字段 | 来源 | 原始坐标 | 关联键 | 页外依赖 | 追加影响 | 外部可变 |
|---|---|---|---|---|---|---|
| `blocks[subagent]` 基础字段 | `block-extractors.ts:subagent`(142)（toolResult details） | toolResult details.taskId 等 | `taskId` | — | — | — |
| `blocks[subagent].sessionId/streamKey/agentId/agentName/streamStatus/summary` | 修正段 `sessions.ts:2042-2118`：`engine.deferredResults.query` + `engine.subagentRuns.query`（`lib/subagent-run-store.ts`，持久化 `~/subagent-runs.json`，接线 `server/index.ts:662`）+ `resolveSubagentBlockSession`(421) + session-meta sidecar（`createSubagentMetaCache` sessions.ts:445 → `readSubagentSessionMetaSync`，`lib/subagent-executor-metadata.ts:62`，同步 IO）+ 子会话文件尾读摘要（`createSubagentSummaryCache` sessions.ts:560 → `loadLatestAssistantSummaryFromSessionFile`，`core/message-utils.ts:399`） | JSONL 占位块 + 外部 run/deferred 记录 + **子会话 JSONL** | taskId；子会话 sessionId/sessionPath | **是（外部 store + 另一文件）** | 子任务完成后旧页 `streamStatus:running→done/failed/aborted`、summary 回填（追溯改写）；deferred store 24h 清理后部分信息仅剩 run store | **是**：subagentRuns、deferredResults、子会话文件、agents 目录 session-meta、agent registry |
| `blocks[workflow].streamStatus/finishedAt/summary` | 修正段 `sessions.ts:2123-2142`（block_update patch 不落盘，重启后从 runStore 回填） | toolResult details.taskId | taskId | 是（同上） | 同上 | **是** |

## 13. interlude 家族（deferred 结果 / turn-input 消费与展示 / loop）

| 响应字段 | 来源 | 原始坐标 | 关联键 | 页外依赖 | 追加影响 | 外部可变 |
|---|---|---|---|---|---|---|
| `blocks[interlude variant=deferred_result]` | `recordDeferredInterlude`（`sessions.ts:1730-1765`）+ `buildDeferredResultInterludeBlock`（`server/deferred-result-interlude.ts:194`）；输入=`hana-background-result` custom_message（`parseHistoryDeferredResult`），receiver 名 `resolveDeferredReceiverName`(235)（agent registry） | 锚点=其后第一条 displayable assistant 的前一 display 位（`nextImmediateDisplayableAssistantIndex`，sessions.ts:349，前向扫描）；deliveryId=`details.deliveryId` 或 `history:${sourceIndex}`（`historyDeferredDeliveryId` sessions.ts:3241） | deliveryId、taskId、entryId | **是（前向锚点 + 全局去重集）**：`turnInputConsumptionDeliveryIds/EntryIds`、`deferredInterludeDeliveryIds` 在全数组构建——本页 interlude 可能被**页外更晚**的消费记录抑制 | 消费记录追加后旧页 interlude 消失（去重，追溯改写） | 部分（meta/result 回退查 deferredStore.query；receiver 名来自 agent registry） |
| `blocks[interlude]`（turn-input consumption/presentation 记录） | `recordTurnInputConsumptionInterlude`(1682)/`recordTurnInputPresentationInterlude`(1713)；解析 `lib/turn-input-presentation.ts:92,134` | consumption：锚点=assistantEntryId 的 display 位-1（经 `displayIndexByEntryId`/`sourceIndexByEntryId` 全局反查，sessions.ts:1642-1654）；presentation：锚点=当前 displayIdx-1 | entryId（input/assistant）、deliveryId | 是（entryId 反查表全局构建） | 追加不改变既有锚点（entryId 坐标稳定） | 否 |
| `blocks[interlude]`（loop kickoff/wakeup/notice） | `recordLoopInterlude`（`sessions.ts:1766-1779`）+ `buildLoopInterludeBlock`（`lib/loop/loop-messages.ts:90`）；`loop-turn` 同时把输入指针置 null（`sessions.ts:2000-2008`） | custom_message `loop-turn`/`loop-notice`，锚点=displayIdx-1 | — | 否（相邻） | 否 | 否 |

## 14. sessionFiles（会话文件引用提取与 registry 调用链）+ 文件块 lifecycle patch

| 响应字段 | 来源 | 原始坐标 | 关联键 | 页外依赖 | 追加影响 | 外部可变 |
|---|---|---|---|---|---|---|
| 顶层 `sessionFiles[]` | `listSessionRegistryFiles`（`sessions.ts:3217`）→ `engine.listSessionFiles(path,{references:sourceMessages})`（`core/engine.ts:1579`）→ `SessionFileRegistry.listReachable`（`lib/session-files/session-file-registry.ts:475`）→ `collectSessionFileReferenceIdentities`(983) 深扫 **整条分支 sourceMessages**：`[SessionFile]{…}` marker（文本正则）、`[attached_image/video/audio:…]` marker、fileId/filePath/realPath 对象字段 → 与 sidecar registry（`session-meta.json`，`sessionFileSidecarPath`:13 + managed cache）求交 → `serializeSessionFile`（`lib/session-files/session-file-response.ts`） | 引用可出现在任意 message 文本/附件 marker | fileId / filePath / realPath / legacyFileIds / legacyFilePaths（`sessionFileIsReachable`:1002） | **是（全分支）**：任何页外消息引用的文件都必须保留在列表里；窗口化读取会让右侧文件面板丢文件 | 新消息引用新文件 → 列表增长；文件被删/移动 → sidecar `status/missingAt` 更新，所有页面的该字段变化 | **是**：registry sidecar + 磁盘实际文件 + locator（manifest） |
| `blocks[file/artifact/skill/screenshot].fileId/filePath/label/status/missingAt/size/mime/kind/storageKind/presentation/version/resource` 等 lifecycle patch | `patchSessionFileLifecycleBlocks`（`sessions.ts:3185`）→ `engine.getSessionFile / getSessionFileByPath`（engine.ts:1576-1577）；screenshot base64 可反推 `browserScreenshotPath` 并降级为 file 块（`lib/session-files/browser-screenshot-file.ts`） | 块内 fileId/filePath | fileId/filePath；sessionPath（locator 选项） | 是（registry/sidecar） | registry 状态变化直接改写旧页块字段 | **是** |

## 15. todos（「当前分支最新合法快照」规则）

| 响应字段 | 来源 | 原始坐标 | 关联键 | 页外依赖 | 追加影响 | 外部可变 |
|---|---|---|---|---|---|---|
| 顶层 `todos[]`（或 null） | `extractLatestTodos(sourceMessages)`（`lib/tools/todo-compat.ts:169`；路由 `sessions.ts:2149`）；快照判定 `extractLatestTodoSnapshot`(144)：从 **数组末尾** 反向扫，`toolResult` 且 `toolName ∈ {todo, todo_write}` 或 custom `lingxi.todo_state`（`lib/tools/todo-constants.ts`）；坏快照（details.todos 非数组）跳过继续向前，空数组=合法「显式清空」；`removed`（全 completed 或 removed!==false）→ `[]`；旧格式 item 就地迁移（`migrateLegacyTodos`:67） | 全分支最后一条合法 todo 快照（sourceMessages 已是当前分支投影，故分支安全；独立入口 `extractLatestTodosFromEntries`:183 另走 `buildSessionContext` leaf-to-root） | 无显式关联键（靠位置「最后者胜」） | **是（几乎永远在最后一页）**：除最新页外，任何窗口都不含该快照 | 每次追加新 todo 快照，**所有页面**的 todos 同步更新（顶层字段不随页） | 否 |

## 16. deferred 重内容（deferred locator）

| 响应字段 | 来源 | 原始坐标 | 关联键 | 页外依赖 | 追加影响 | 外部可变 |
|---|---|---|---|---|---|---|
| `*.deferred{id,kind,size,preview?,available}`（inline_image / assistant_segment / tool_output / skill_content / screenshot / artifact） | `createHistoryDeferredContent`（`server/history-deferred-content.ts:80`，阈值 8KiB，preview 240 字符）；各挂点：`sessions.ts:266-292`(screenshot/artifact)、1800-1812(inline_image)、1859-1867(reasoning)、1884-1908(tool_output/skill_content) | locator=base64url `{version:1, sourceIndex, entryId, kind, ordinal}` | sourceIndex+entryId（双重定位） | 解析端 `GET /sessions/content/:contentId`（`sessions.ts:1391-1427`）整文件重读并校验 `entryId` 相等——**读窗口外内容**；分支重置/重写后 entryId 失配 → null（fail-closed） | 尾部追加不影响 locator；重写/修复（保序保 id）也不影响 | 否 |

## 17. revision / hasMore / nextBefore / reconciliation

| 响应字段 | 来源 | 原始坐标 | 关联键 | 页外依赖 | 追加影响 | 外部可变 |
|---|---|---|---|---|---|---|
| `revision` | `readSessionFileRevision`（`sessions.ts:387`）→ `sessionFileRevision`（`core/session-list-projection-cache.ts:19`）=`"${size}:${mtimeMs}"`；**在读取内容之前取**（sessions.ts:1491-1494 竞态纪律） | 文件 stat | sessionPath | 是（文件级身份，与页无关） | 每次追加必变 | 是（文件系统） |
| `hasMore` / `nextBefore` | `resolveHistoryPageBounds`（`sessions.ts:362-375`）；`nextBefore=String(startIdx)`（2038） | display 总数 total（全数组计数） | display 序号 | 是（total 需全量计数） | 追加 displayable 消息使 total 增大、改变最新页边界（旧页区间不变） | 否 |
| `reconciliation`（仅 `?reconciliation=1`） | `readDesktopInputRunSnapshot`（`core/desktop-session-submit.ts:89`）+ `loadSessionHistoryEvidence`（`core/message-utils.ts:169`，严格分支校验，两次 head 快照对比） | runtime 提交队列/streaming 状态 + branchHead(leafId,observedTailLeafId) | sessionId/sessionPath | 是（全文件 + 运行态） | 运行态随时变化 | **是**：engine 运行态、pendingDesktopSessionSubmissions、manifest store |
| 副作用：`activityHub.rebroadcastSession` | `sessions.ts:2154-2156`（仅首屏非翻页非对账） | — | sessionPath | — | — | 是（WS 广播） |

## 18. 显示序号语义专节（任务指定三规则，均在主循环定位）

1. **隐藏 user 占 display 序号**：`isDisplayableHistoryMessage`（sessions.ts:294）对 user 只看「有文本块或内联图」——`<hana-background-result>`/`<hana-deferred-tasks>` 系统注入是文本，**占号**；Run 预扫描（1574-1577）同样给它们推进 displayCounter 并开启新 Run。前端 `history-builder.ts:670` 再按正则过滤不渲染。→ 服务端 display 序号 ≠ 用户可见条数，页窗口切在「服务端序号」上。
2. **Reminder 清理后显示语义**：剥离发生在读取层（`projectSessionMessageForDisplay`）与展示层（`sanitizeVisibleContent`）两处；`isDisplayableHistoryMessage` 与 find 路由用原始文本判定，因此「剥离后为空」的 user 仍占号；assistant 可见性看 `hasAssistantSemanticTextContent`（包含 commentary 文本块，`sessions.ts:249`），与 `extractTextContent` 过滤 commentary 的口径不同。
3. **不可见 assistant 对 assistantOrdinal 的影响**：`assistantOrdinalInTurn += 1` 在 displayable 判定**之前**（sessions.ts:1842），不可见 assistant（空内容/无 toolUse/stopReason=completed）不占 display 序号、不进 Run 区间，但**推大会话轮内序号**，直接改变同轮后续 assistant 的 `assistantSegments[].id`（`assistant:${ordinal}:…`）。跨页闭包若只重放可见记录会得到不同 segment id。

## 19. 追加影响总表（尾部 append 视角）

- **不受尾部追加影响的旧页字段**（append-only + 坐标稳定）：`messages[].id/sourceIndex/entryId/content/origin/presentation/agentReview/modelCallRef/turnInputEntryId/turnStartIndex/turnEndIndex`（已闭合 Run）、`blocks[].afterIndex/sourceIndex`、deferred locator。
- **会被尾部追加追溯改写的旧页字段**：`toolCalls[].status/details`（toolResult 后到）、`blocks[suggestion_card].status`（collab 决策后到）、`blocks[media_generation]`（结果记录/store 后到）、`blocks[subagent/workflow].streamStatus/summary`（run store 后到）、interlude 的存在性（消费记录后到抑制）、`messages[user].clientMessageId→ambiguous`（重复 clientMessageId 后到）、顶层 `todos`、顶层 `sessionFiles`、`revision`、最新 Run 的 `turnEndIndex`。
- **非 append 写入（分支重置/重试/rewind、`repairOversizedSessionEntriesInFile` 全量重写、locator 重绑定、归档恢复删除）可整体改变 sourceIndex 与 display 序号**——所有坐标字段的失效源，属 writer-invalidation-map（B03 配套表）范围。

## 20. 页外依赖汇总（B03 依赖闭包的关键输入）

**A. 强制读窗口之外（不做就产出错误值，不是仅丢精度）：**

| # | 字段/块 | 方向 | 距离上界 | 依赖对象 |
|---|---|---|---|---|
| 1 | `todos` | 后向（向分支尾） | 到尾部 | 最后一条合法 todo 快照（坏快照跳过语义要求连续回扫） |
| 2 | 顶层 `sessionFiles` | 全分支 | 整条分支 | 所有消息的 marker/对象引用 + registry sidecar |
| 3 | `blocks[media_generation]` 终态 | 前向 | 到分支尾 | `hana-deferred-result`/`hana-background-result` 记录 + deferredResults store |
| 4 | `blocks[suggestion_card].status` | 前向 | 到分支尾 | collab 决策记录（suggestionId 索引） |
| 5 | `toolCalls[].status/details/endedAt` | 前向 | 相邻但闭包须含 j>i | toolResult 条目（toolCallId 配对） |
| 6 | `turnInputEntryId/turnInputVisible` | 后向 | 跨页（长 Run 的输入可在任意早处） | turn 输入边界记录 + `turn_input_consumption` 记录（assistantEntryId→inputEntryId 全局映射） |
| 7 | `turnStartIndex/turnEndIndex` + 客户端 Run 缝合 | 双向 | 同 Run 全部记录（可跨多页） | Run 边界扫描 + 同 Run 所有 assistant 记录 |
| 8 | interlude 去重/抑制 | 前向 | 到分支尾 | `turn_input_consumption` 的 deliveryId/entryId 集合（晚于本页的记录可抑制本页 interlude） |
| 9 | deferred interlude 锚点 | 前向 | 相邻 displayable assistant（可能在下一页） | `nextImmediateDisplayableAssistantIndex` |
| 10 | correlation（clientMessageId/snapshotVersion/ambiguous） | 全分支 | 整条分支 | correlation custom 条目（歧义检测需全局 byClient 统计） |
| 11 | `origin/presentation/agentReview/modelCallRef` | 后向近邻 | 通常相邻，可跨窗口边界 | 紧邻前置 custom 条目 |
| 12 | `hasMore/nextBefore`、`afterIndex` 页内重映射 | 全分支计数 | 整条分支 | displayable 总数 |
| 13 | deferred locator 解析（`/messages/:contentId`） | 随机访问 | 整文件 | sourceIndex+entryId 定位 |
| 14 | `revision`/`reconciliation` | 文件级/运行态 | — | stat、branchHead 两次快照、streaming 状态 |

**B. JSONL 之外的 store（快路径必须视为可变外部输入）：**

- `session-manifest.db`（`core/engine.ts:1956` SessionManifestStore）：manifest、currentLocator、branchHead；读取链 `openSessionManagerAtCurrentBranch` 会**隐式写**（append_recovery/observe_tail/legacy_backfill 时 `setBranchHead`，`core/session-branch-head.ts:79-118`）。
- `deferredResults`（`lib/deferred-result-store.ts`，`~/.ephemeral/deferred-tasks.json`，1s 防抖落盘 + 24h 清理计时器）：media 终态、subagent/interlude 元数据回退。
- `subagentRuns`（`lib/subagent-run-store.ts`，`~/subagent-runs.json`）：subagent/workflow 终态、childSessionId/Path。
- `SessionFileRegistry`（sidecar `session-meta.json` + managed cache）：sessionFiles 列表与所有文件块 lifecycle 字段。
- agents 目录 sidecar（subagent session-meta，同步 `readFileSync`）与**子会话 JSONL 尾读**（subagent summary）。
- agent registry（`resolveSessionOwnership/getAgent`）：interlude receiver 名、executor 身份。
- 运行态：`isSessionStreaming`、pendingDesktopSessionSubmissions（reconciliation）。

**C. 读取路径的隐式写/修复（闭包设计必须隔离）：**

- `repairOversizedSessionEntriesInFile`（`core/session-jsonl-file.ts:244`）：读前全量重写坏行/超限行，留 `.repair.json` 备份——改变文件字节与 revision。
- `applyStoredSessionBranchHead`（`core/session-branch-head.ts:79`）：读时可能回写 branchHead（manifest store）。
- 对照组：`loadSessionHistoryEvidence`（`core/message-utils.ts:169`）是「不修复、不回写、两次 head 校验」的严格读取，reconciliation 专用。

**D. 对 B03 的直接推论**：任何「只读页窗口 [startIdx,endIdx) 对应物理行区间」的快路径，对 #1-#4、#6-#8、#10、#12（A 表）与全部 B 类 store 都不完整；安全快路径的最小闭包 = 窗口行 + 分支尾扫描（todos/collab/media 结果/消费抑制集）+ 全局 displayable 计数 + correlation 全分支统计 + 外部 store 快照，或显式声明这些字段退化为「按需第二遍扫描/不保证新鲜」。
