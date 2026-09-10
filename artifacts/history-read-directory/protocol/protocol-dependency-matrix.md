# 协议依赖矩阵（E01：字段/头部 ↔ 产生/消费代码一一映射）

原则：复用仓库既有接口/类型并记录一对一映射，不并存两套协议。本矩阵是 G3"语义与表示边界可解释"的实现级明细。

## 一、响应业务字段（普通成功页，全部进入 ETag 表示）

| 字段 | 产生位置（服务端） | 消费位置（客户端） | 备注 |
|---|---|---|---|
| `messages` | `server/history-read/project-page.ts` projectHistoryPage → index.ts result | `desktop/src/react/utils/history-builder.ts` buildItemsFromHistory | 条目含 id/sourceIndex/entryId/role/content |
| `blocks` | project-page.ts blocks + `server/history-read/hydrate.ts` slicedBlocks | history-builder.ts（interlude 块） | deferred/媒体块修补 |
| `todos` | hydrate.ts:416–420（todoSnapshot 经 `lib/tools/todo-compat.ts` applyTodoLifecycle） | session-actions.ts loadMessages（migrateLegacyTodos → setSessionTodosForPath） | 外部状态：随目录 todoSnapshot 指针 |
| `sessionFiles` | hydrate.ts（registry listReachable，`lib/session-files/session-file-registry.ts`） | setSessionRegistryFiles / upsertSessionRegistryFile | 外部状态：sidecar registry |
| `hasMore` | index.ts page.bounds.hasMore | initSession（hasMore 以服务端为准，T11b） | |
| `nextBefore` | index.ts page.bounds（含义不变：更早页游标） | historyNextCursor（session-actions.ts:382） | |
| `revision` | `server/routes/sessions.ts` readSessionFileRevision（读前捕获，I07 读后复核） | initSession 修订点 stamp / reconcile 对比 | 与 ETag 职责独立，不改格式 |
| deferred 凭证 | 消息 content 内嵌 content-id；展开经 `server/history-deferred-content.ts` resolveHistoryDeferredContent（凭 sourceIndex/entryId 语义） | deferred content 拉取链 | 条件快照覆盖全部字段，凭证字段变化→标签失配 |

## 二、协议头/请求条件 ↔ 实现位置

| 头部/条件 | 产生/判定代码 | 消费/作用 |
|---|---|---|
| `Lingxi-History-Protocol: 1` | `server/history-read/protocol.ts` HISTORY_PROTOCOL_VERSION / evaluateHistoryConditionalGet headers | E03 客户端能力探测（缺失→回退 50/200） |
| `Lingxi-History-Page-Limit: K` | protocol.ts `HISTORY_PROTOCOL_PAGE_LIMIT`（**单一配置点，当前 50**，E06 才改） | E03 客户端页大小建议 |
| `ETag: W/"hrp1-<sha256>"` | protocol.ts buildHistoryPageTag | E03 条件请求回显 |
| `If-None-Match`（弱比较/`*`/非法忽略） | protocol.ts evaluateIfNoneMatch | 服务端 304 判定 |
| `Cache-Control: private, no-store` | protocol.ts evaluateHistoryConditionalGet headers（200/304 同） | 禁缓存；不设 Vary（no-store 禁缓存，无需中介协商） |
| 304 无正文/无 Content-Length | `server/routes/sessions.ts` c.body(null, 304, headers) | RFC 9110 |

## 三、条件 GET 求值链（E02.1 八步 ↔ 代码）

| 步骤 | 代码位置 |
|---|---|
| 1 身份解析/授权/请求校验 | sessions.ts:1160 起（createRequestContext → authorizeSessionRoute sessions.read） |
| 2 B/C 读取+快照复核 | `server/history-read/index.ts` readSessionHistoryPage → tryDirectoryOnce（读后复核 I07，:451–458） |
| 3 projector+外部状态 | projectHistoryPage + hydrateExternalState（index.ts:464–502） |
| 4 lifecycle/rebroadcast（仅一次） | sessions.ts:1246–1248（beforeId==null 时 activityHub.rebroadcastSession，先于求值） |
| 5–6 序列化一次+作用域 ETag | sessions.ts:1254–1278 JSON.stringify 一次 → protocol.ts buildHistoryPageTag |
| 7–8 匹配→304 无正文 / 200 同字节串 | protocol.ts evaluateHistoryConditionalGet + sessions.ts c.body |

分支选择身份：`ReadSessionHistoryPageOutcome.branchIdentity`（index.ts:70–73；tryDirectoryOnce ok 返回自 directory.branch.selectedLeafId/physicalTailLeafId/headResolution；full/失败=null）——直接取自 B/C 目录，不重开 SessionManager、不遍历分支。

## 四、作用域身份输入 ↔ 来源

| 输入 | 来源 |
|---|---|
| principalId（授权主体） | `server/http/boundary.ts` createRequestContext → requestContext.principalId |
| serverNodeId / studioId | 同上（runtimeContext/authPrincipal） |
| sessionId | sessions.ts reconciliationSessionId（manifest 解析） |
| 规范 locator | path.resolve(resolvedSessionPath)（digest 输入为内部身份摘要来源；标签本身不可逆） |
| 进程盐 | protocol.ts 模块级 randomBytes(32)（不落盘；重启失配=正常 200） |

## 五、既有接口复用映射（不并存两套协议）

| 既有接口 | 协议中的角色 |
|---|---|
| `resolveHistoryPageBounds`（history-read/page.ts，经 sessions.ts re-export） | find 路由共用页边界；协议不改其语义 |
| `readSessionFileRevision` | revision 唯一来源；ETag 独立 digest，不修改 revision 语义 |
| `SessionDirectoryCache`（B04 probe/租约/发布） | 条件 GET 之前的表示新鲜度保证层；条件求值不绕过它 |
| `lib/debug-log.ts` createModuleLogger | ETag 日志（requestId/结果/截短摘要） |
