# writer-invalidation-map.md — 会话 JSONL 写入/变化入口清单（A02）

任务：历史分页读取与协议层全链路优化 · 阶段 A02
基准 HEAD：`1d42b7405c76292f617291e3a01cd2f3ef5efd04`（工作区仅新增未跟踪的 `artifacts/history-read-directory/`，无源码改动）
定位方式：按实际函数定位（函数名 + 首次定义处），不依赖行号；行号仅为复核线索，对应 HEAD。

## 0. 方法与检索范围

检索词覆盖：`appendFile / appendFileSync`、`writeFile / writeFileSync / fsp.writeFile`、
`rename / renameSync`、`truncate`（含 `openSync(..., "w")` 的隐式截断）、`unlink / rm / rmSync`、
`copyFile / copyFileSync`、`utimes`、`_rewriteFile`、`writeSessionEntriesFile`、
`SessionManager.append* / branch / resetLeaf / createBranchedSession / setSessionFile / forkFrom`、
`setBranchHead / moveSessionLifecycle / updateLocatorLifecycle`、`fs.watch / chokidar / watchFile`。

范围：`core/ lib/ server/ hub/ shared/ cli/ desktop/*.cjs` 应用代码 + `@earendil-works/pi-*` SDK。
`lib/session-jsonl.ts`、`core/desktop-input-correlation.ts`、`server/history-deferred-content.ts`、
`core/session-list-projection-cache.ts`、`lib/tools/todo-compat.ts`、`shared/tool-outcome.ts`
经核实为纯读模块（无任何 fs 写调用），不产生写入口。

---

## 1. 写入模型基座：Pi SDK SessionManager 的原语

所有桌面/桥接会话 JSONL 的最终落盘都收敛到 SDK
`node_modules/@earendil-works/pi-coding-agent/dist/core/session-manager.js` 的 `SessionManager`。
应用层（core/）没有任何绕过它的 JSONL 直写（全仓唯一的 JSONL `writeFileSync` 是 Hana 自己的
`core/session-jsonl-file.ts` `writeSessionEntriesFile`，见 §3-D）。SDK 原语共 4 个：

| SDK 原语 | 函数 | fs 调用 | 旧前缀字节保证 |
|---|---|---|---|
| 追加 | `_persist(entry)`（`flushed=true` 分支，含 `_appendEntry` 全部调用者：`appendMessage`、`appendCustomEntry`、`appendCustomMessageEntry`、`appendCompaction`、`appendSessionInfo`、`appendLabelChange`、`appendModelChange`、`appendThinkingLevelChange`、`branchWithSummary`） | `appendFileSync(sessionFile, line)` | **保证**。在当前 EOF 追加一行；不触碰已写字节。前提：单写者。 |
| 首 flush | `_persist(entry)`（`hasAssistant && !flushed` 分支） | `openSync(sessionFile, "wx")` 逐行 `writeFileSync` | 新建文件（`wx` 已存在即抛 EEXIST），无旧前缀可言。注意：若外部已创建同名文件而 `flushed=false`，此路径抛错而非覆盖。 |
| 整文件重写 | `_rewriteFile()` | `openSync(sessionFile, "w")`（先截断为 0）+ 逐行 `writeFileSync` | **不保证**。truncate-then-write；内存 `fileEntries` 重新 `JSON.stringify`，旧前缀字节可变（修复投影、引用改写都会改内容）；崩溃窗口内文件可为空/半文件。无原子 rename。 |
| 打开时迁移 | `_setSessionFile`（`SessionManager.open` 构造路径） | 空文件(size 0) → `newSession()+_rewriteFile()` 写 header；`migrateToCurrentVersion` 检出 v1/v2 → `_rewriteFile()` | **不保证**（v1/v2 文件被重写升级为 v3；v3 文件 open 不写）。**这是一条读路径触发的写**。 |

`branch(leafId)` / `resetLeaf()` 只改内存 leaf 指针，不写文件；分支持久化在 Hana 的 manifest
store（§3-H）。`forkFrom`、`exportToJsonl` 在 SDK 内存在但 Lingxi 应用代码未调用（已 grep 核实），
不在可到达集合内。

**SDK 没有任何写入通知**：`session-manager.js` 无 EventEmitter、无 emit、无 append/rewrite 回调
（已核实 0 匹配）。写入发生与否对应用层不可观测，除非写点本身在 Hana 代码里。

---

## 2. 通知与监听现状（「应用层内存通知」判定依据）

1. **无会话文件监听**。全仓 watcher 只有四处，均不针对 `agents/*/sessions/`：
   - `core/skill-manager.ts`（chokidar，skillsDir）；
   - `lib/knowledge/source-file-watcher.ts`（fs.watch，知识库源目录）；
   - `lib/file-history/workspace-watcher.ts`（fs.watch recursive，**用户工作区根**，file-history 特性）；
   - `lib/resource-io/resource-watch-registry.ts`（fs.watch，按资源引用注册的 workspace 文件）。
2. **无文件变化失效总线**。grep `session_file_changed|invalidateHistory|file_changed` 仅命中
   stat 派生的 `sessionFileRevision`（`core/session-list-projection-cache.ts`：`${size}:${mtimeMs}`，
   `server/routes/sessions.ts` `readSessionFileRevision` 同源）。stat 失败返回 null = 「修订点未知」。
3. **进程内可观测信号（弱通知，不可直接当失效通知用）**：
   - `AgentSession.subscribe(event)`（`core/session-coordinator.ts` `createSession` 内注册）：
     `message_end`（区分 role）、`compaction_start` 等，只覆盖**活跃会话回合内**的 SDK 追加时机，
     不覆盖冷开 manager 的追加、修复重写、生命周期 rename/unlink；
   - compaction 的 `onCompacted` 回调（`core/session-compactor.ts` `appendCompactionResultToSession`）；
   - engine 事件总线 `_d.emitEvent`（UI 事件，如 `session_user_message`、`todo_update`、
     `session_branch_persistence_warning`），非文件事实通知。
4. **结论：当前所有写入口都没有可信的、可被缓存失效订阅的应用层通知**；「表」中该列全部为
   ✗，仅追加类可注明「有回合同步边界可挂钩」（阶段 C 加失效通知的候选位置）。

---

## 3. 入口清单

标注约定：通知列 = 是否存在可信应用层内存通知；跨进程列 = 该入口写入的内容是否可能被另一进程
产生/覆盖；旧前缀列 = 写入模型对已读前缀字节的影响（✅保证 / ✗破坏 / 新文件无旧前缀 / n/a 字节不变但路径与元数据变）。

### A. 正常生产追加（append-only，经 SDK `_persist` flushed 分支）

| 入口 | 写入类型 | 通知 | 跨进程 | 旧前缀 |
|---|---|---|---|---|
| `core/session-coordinator.ts` `_promptWithinTrace` → `session.prompt()` → SDK agent 循环 `appendMessage/appendCustomMessageEntry/appendCompaction`（助手/用户/toolResult/compaction/branch_summary 落盘） | 追加 | ✗（有 `session.subscribe` 的 `message_end` 可挂钩，回合内同步） | 单 kernel 闸内否则可能 | ✅ |
| `core/desktop-session-submit.ts` `recordDesktopInputCorrelationEntry` / `recordMessageOriginEntry` / `recordAgentReviewEntry` / `recordMessagePresentationEntry`（`appendCustomEntry`） | 追加（origin/review/presentation/输入 correlation custom 条目） | ✗ | 同上 | ✅ |
| `core/session-coordinator.ts` `recordCustomEntry`（live manager 分支）+ `engine.recordCustomEntry`（`core/engine.ts` 转发；`lib/deferred-result-coordinator.ts`、协作决定、媒体结果等消费） | 追加（deferred/collab/media custom 条目） | ✗ | 同上 | ✅ |
| `core/session-coordinator.ts` `recordCustomEntry` **cold 分支**：`openSessionManagerAtCurrentBranch` + `appendCustomEntry`（无 live manager 时） | 追加 + 冷开副作用（见 D/H） | ✗ | 同上 | ✅（追加本身）；冷开可触发迁移重写（✗） |
| `lib/llm/model-call-correlation.ts` `persistModelCallReferenceForMessage`（`message_end(role=assistant)` 时 `appendCustomEntry`，由 `createSession` 的 subscribe 注册） | 追加（modelCallReference） | ✗（就是 subscribe 回调内，天然有进程内时机） | 同上 | ✅ |
| `core/session-turn-actions.ts` `commitRetryBranch`（重试/分支重置：`engine.setSessionBranchHead` + `appendCustomEntry(SESSION_BRANCH_RESET_RECORD_TYPE)`） | 追加（分支重置标记）+ 分支头持久化（见 H） | ✗ | 同上 | ✅（JSONL 部分） |
| `core/session-compaction-runtime.ts`（`appendCustomMessageEntry`）、`lib/loop/loop-controller.ts`、`lib/loop/loop-messages.ts`、`lib/tools/channel-tool.ts`、`lib/tools/dm-tool.ts`、`server/routes/session-collab.ts`、`server/routes/sessions.ts` todos-complete 路由（`getWritableSessionManager` + `appendCustomMessageEntry` + `syncSessionBranchHead`） | 追加 | ✗ | 同上 | ✅ |
| `core/session-coordinator.ts` `continueDeletedAgentSession` 中 `manager.appendMessage(...)` 循环（新会话） | 追加（新文件） | ✗ | 同上 | 新文件 |

**注意（双写者隐患，仓库已留注释证据）**：`ensureSessionLoaded` 的 in-flight 去重注释
（`core/session-coordinator.ts`）明确记录过「两个 SessionManager 同时写同一 JSONL → 幽灵写入者
→ 孤儿分支」事故。`openSessionManagerAtCurrentBranch` / `getWritableSessionManager` 的冷开
manager 若与 live manager 并存且都写，即为进程内双写者。可信追加模型必须把「同一时刻同一文件
只有一个活跃写者」作为前提并给出判定。

### B. 首 flush / 预助手物化

| 入口 | 写入类型 | 通知 | 跨进程 | 旧前缀 |
|---|---|---|---|---|
| `core/session-jsonl-file.ts` `flushSessionManagerSnapshot` → `manager._rewriteFile()` + 置 `flushed=true` | 整文件重写（把 SDK 内存前缀物化，防 assistant 到达时重复前缀） | ✗ | 单 kernel 闸内否则可能 | ✗（truncate 重写；对象通常是不存在/同内容文件，但字节层面是重写） |
| 同上 `schedulePreAssistantSessionManagerFlush`（`createSession` subscribe 对每个非 assistant `message_end` 经 microtask 触发） | 首 flush（preAssistantOnly：仅尚无 assistant 时） | ✗ | 同上 | ✗/新文件 |
| `core/session-coordinator.ts` `createSession` 内 `flushSessionManagerSnapshot(session.sessionManager)`（会话创建/restore 完成时） | 首 flush | ✗ | 同上 | ✗/新文件 |
| SDK `_persist` 的 `wx` 首 flush（`hasAssistant && !flushed`） | 首 flush（新建） | ✗ | 同上 | 新文件（`wx` 已存在即抛错） |

`flushSessionManagerSnapshot` 的其他调用点：`rewriteForkedSessionDraftReferences`（1043）、
fork 流程（3672/3861/4023）、`_cloneForkedSubagentChildSession`（3047/3209/3272）——语义均为
「改写内存 entries 后强制落盘」，即重写，见 F。

### C. 回合后重写（每轮 prompt finally）

| 入口 | 写入类型 | 通知 | 跨进程 | 旧前缀 |
|---|---|---|---|---|
| `core/session-coordinator.ts` `_promptWithinTrace` finally → `pruneSessionInlineMediaHistory`（`core/session-inline-media-prune.ts` `pruneSessionManagerEntries`：就地改 `entry.message` 后 `_rewriteFile`） | 整文件重写（剥离 inline media） | ✗ | 单 kernel 闸内否则可能 | ✗ |
| 同 finally → `_projectOversizedSessionHistory`（`repairOversizedSessionEntries` 内存投影 + `_rewriteFile`） | 整文件重写（超大行投影） | ✗ | 同上 | ✗ |
| `core/bridge-session-manager.ts` `_executeExternalMessageWithinTrace` 内 `pruneSessionInlineMediaHistory` | 整文件重写 | ✗ | 同上 | ✗ |

### D. 读路径隐式修复（历史读取/恢复时触发，对分页缓存最危险的一组）

| 入口 | 写入类型 | 通知 | 跨进程 | 旧前缀 |
|---|---|---|---|---|
| `core/message-utils.ts` `loadSessionHistoryMessages`（`/sessions/messages` 普通路径直接调用）→ `repairOversizedSessionEntriesInFile`（`core/session-jsonl-file.ts`：读全文 → 逐行解析 → 投影超大行/剔除坏行 → `copyFileSync` 备份 `.repair.json` → `writeSessionEntriesFile` 整文件 `writeFileSync` 重写） | 修复 + 整文件重写 + 新建 `.repair.json` 旁车 | ✗（仅返回值 `repaired` 给调用方，`loadSessionHistoryMessages` 丢弃之） | 单 kernel 闸内否则可能 | ✗（坏行被删、超大行被投影改写；仅当存在坏/大行时触发） |
| 同入口 → `openSessionManagerAtCurrentBranch`（`core/session-coordinator.ts`）→ `SessionManager.open`：v1/v2 文件 `migrateToCurrentVersion` → `_rewriteFile()` | 迁移 + 整文件重写 | ✗ | 同上 | ✗（仅 v1/v2 触发；v3 不写） |
| 同入口 → `applySessionBranchHead`（`core/session-branch-head.ts` `applyStoredSessionBranchHead`）head 不匹配时 `manifestStore.setBranchHead`（含 `append_recovery` / `legacy_backfill` / `observe_tail`） | manifest 写（分支头 backfill），不动 JSONL | ✗ | manifest 为单 kernel SQLite | n/a（JSONL） |
| `core/session-coordinator.ts` `switchSession`（4798–4801）、reloadSession（6534–6535）、`_loadSessionForAttach`/`ensureSessionLoaded`（6618–6621）、`executeIsolated` resume 分支（8051–8053）在 `SessionManager.open` 前依次执行 `_repairOversizedSessionHistory`、`_repairOrphanToolHistory`、`_repairInlineMediaHistory` | 修复 + 整文件重写（条件触发） | ✗（仅 log.warn + 一次性 `session_unhealthy_warning` 事件） | 同上 | ✗ |
| `core/session-coordinator.ts` `_repairOrphanToolHistory` → `core/session-health.ts` `repairOrphanToolResultEntriesInFile`：`readSessionEntriesFile` → 删除孤儿 toolResult、重连 parentId → `writeSessionEntriesFile` 整文件重写 | 修复（**删除条目**）+ 整文件重写 | ✗ | 同上 | ✗（已读前缀条目可被删除/重连） |
| 同组 `_repairInlineMediaHistory` → `core/session-inline-media-prune.ts` `repairSessionInlineMediaEntriesInFile` → `writeSessionEntriesFile` | 修复 + 整文件重写 | ✗ | 同上 | ✗ |
| `core/bridge-session-manager.ts` `_repairInlineMediaHistory`（bridge restore 1216、bridge compact reopen 1768）、`repairOrphanToolResultEntriesInFile`（1206、1758 `compactSession`） | 修复 + 整文件重写（bridge 会话） | ✗ | 同上 | ✗ |
| `loadSessionHistoryMessages` 兼容回退分支（Pi 头校验失败后 `fs.readFile` 逐行投影） | 无写 | — | — | 只读 |

**对照**：`loadSessionHistoryEvidence`（`core/message-utils.ts`，reconciliation=1 严格路径）
**显式不修复、不写 manifest**（注释「不修复文件/manifest」），只读 + 双重 head 校验。阶段 C 的
可信追加不得让 reconciliation 获得副作用（与任务书 B07 一致）。

### E. 分支/locator 状态写（不动 JSONL 字节，但改变「哪个文件是事实源」「当前分支是什么」）

| 入口 | 写入类型 | 通知 | 跨进程 | 旧前缀 |
|---|---|---|---|---|
| `core/session-branch-head.ts` `applyStoredSessionBranchHead`（head 缺失/尾观测变化时 `setBranchHead`，`append_recovery`） | manifest 写（分支头恢复） | ✗ | manifest SQLite 单 kernel | n/a |
| `core/session-branch-head.ts` `persistExplicitSessionBranchHead` / `syncSessionBranchHeadAfterAppend`（coordinator `_syncSessionBranchHead(Quiet)`，回合 finally、todos 路由、fork/clone 等调用） | manifest 写（分支头权威化） | ✗ | 同上 | n/a |
| `core/session-branch-head.ts` `readManifestSessionBranch`（读路径，`persistRecovery` 默认 true 时写回 `append_recovery` head） | manifest 写（读触发） | ✗ | 同上 | n/a |
| `core/session-manifest/store.ts` `setBranchHead`(969)、`updateLocator`(700 附近)、`updateLocatorLifecycle`(718)、`moveSessionLifecycle`（coordinator/路由包装）、`setPinnedAt/setPinOrder/setCapabilitySnapshot/setExecutorMetadata` | locator 重新绑定 / 生命周期迁移（SQLite WAL 事务） | ✗（store 无事件） | 单 kernel（同宅互斥闸） | n/a |

### F. fork / 迁移类（写新文件；源文件字节不动）

| 入口 | 写入类型 | 通知 | 跨进程 | 旧前缀 |
|---|---|---|---|---|
| `core/session-coordinator.ts` `_forkSessionAtNodeUnlocked`：`sourceManager.createBranchedSession(boundaryEntry.id)`（SDK：`sourceManager` 被改绑到**子**新文件并 `_rewriteFile` 落盘）+ 草稿/媒体/subagent/workflow 引用改写（改内存 `fileEntries`）+ `flushSessionManagerSnapshot`（3672/3861/4023，子文件整文件重写）+ `appendCustomEntry(DEFERRED_RESULT_RECORD_TYPE)` | 新建子会话文件 + 子文件多次整文件重写 | ✗ | 单 kernel 闸内 | 源文件 ✅ 不动；子文件为新文件 |
| fork 失败清理：`fsp.rm(childSessionPath)`（4277）+ `updateLocatorLifecycle(...,"deleted")`（4266）+ `_discardForkedSubagentChildSession`/`_discardForkedWorkflowTaskState`（2786/3426 内 rm journal/child session） | 删除（子文件/子会话/journal） | ✗ | 同上 | 文件消失 |
| `_cloneForkedSubagentChildSession`（2948：`SessionManager.open` 子会话 + `appendCustomEntry` + `flushSessionManagerSnapshot` 3047/3209/3272；目标已存在时 `fsp.rm(targetSessionPath)` 3293；`_cloneForkedWorkflowTaskState` 3299 `copyFile` journal 3339、失败 `fsp.rm(journalPath)` 3453） | 克隆子会话：追加 + 重写 + 目标删除 + journal 复制 | ✗ | 同上 | ✗（对被重建的目标文件） |
| `continueDeletedAgentSession`（4283：createSession 新文件 → `appendMessage` 循环 → `manager._rewriteFile()` 4340/4363 强制落盘 → `writeSessionMeta`；失败 `discardSessionRuntime` + `fsp.rm(createdSessionPath)` 4378） | 迁移式重建（新文件）+ 失败删除 | ✗ | 同上 | 新文件 |

### G. 生命周期 rename / unlink / touch（内容字节不变，路径与元数据变）

| 入口 | 写入类型 | 通知 | 跨进程 | 旧前缀 |
|---|---|---|---|---|
| `server/routes/sessions.ts` `archiveActiveSessionCore`（625）：`closeSession` → manifest `moveSessionLifecycle(archived)` → `fs.rename(active → sessions/archived/)`(656) → sidecar 随迁 → `fs.utimes(dest)`(674，mtime=归档时刻)；失败回滚 rename + manifest | 归档（rename + mtime 触碰） | ✗（同进程路由同步可知；对缓存层无通知） | 单 kernel 闸内；外部进程无法经此入口但 rename 结果外部可见 | n/a（字节不变；**mtime 变化 ⇒ revision `${size}:${mtimeMs}` 变化而内容未变**） |
| `POST /sessions/restore`（3078）：`repairHeaderOnlyActiveRestoreTarget`（目标 header-only 残留时 `fs.unlink` 802）→ `fs.rename(archived → active)`(3116) → manifest move；失败回滚 rename(3129) | 恢复（rename + 可能删除残留目标） | ✗ | 同上 | n/a / 残留目标被删 |
| `permanentlyDeleteArchivedFile`（733）：`fs.rename(→ .deleting)`(739) → manifest `lifecycle=deleted` → `fs.unlink`(762)；失败回滚 | 删除（两阶段：staged rename → unlink） | ✗ | 同上 | 文件消失 |
| `POST /sessions/cleanup`（2863）：按 mtime 阈值遍历 archived 目录逐条 `permanentlyDeleteArchivedFile` | 批量删除 | ✗ | 同上 | 文件消失 |
| `core/session-coordinator.ts` `promoteActivitySession`（7856）：`fs.renameSync(activity → sessions/)`(7865) → `moveSessionLifecycle(active)`；manifest 失败回滚 rename(7881) | locator 迁移（activity 目录提升） | ✗ | 同上 | n/a |
| `executeIsolated` cleanup（7975/7990 `fs.unlinkSync` 临时/ephemeral 会话；`tombstoneFreshIsolatedManifest` 7955 `updateLocatorLifecycle(deleted)`） | 删除（临时隔离会话） | ✗ | 同上 | 文件消失 |
| `core/slash-commands/session-ops.ts` `_rotateBridge`（102 `fs.renameSync` bridge 会话轮转归档）与 `_deleteBridge`（120 `fs.unlinkSync`） | 分支重置归档 / 删除（bridge 会话） | ✗ | 同上 | n/a / 消失 |
| workspace-disposal / sweep-orphaned-workspaces 路由：复用 `archiveActiveSessionCore` / 永久删除 | 归档/删除 | ✗ | 同上 | 同上 |

### H. 邻接持久 store（非 JSONL，但与失效判定耦合，列出防混淆）

| 入口 | 对象 | 说明 |
|---|---|---|
| `core/session-coordinator.ts` `saveSessionTitle`(6859) / `clearSessionTitle`(7033) | `<sessionDir>/titles.json`（`fsp.writeFile` 6872/7060） | 列表标题；key 为活跃路径 |
| `writeSessionMeta` / `_doDeleteSessionMetaEntry`(7482) / `_quarantineOversizedSessionMeta`(7572 `fsp.rename`) / `_compactOversizedSessionMeta`(7599) / `_externalizeSessionMetaPayloads`(7667) / 7689 `fsp.writeFile` | `<sessionDir>/session-meta.json`（经 `_metaWriteQueue` 串行） | 权限模式、capability、continuation 元数据等 sidecar |
| `repairOversizedSessionEntriesInFile` 的 `copyFileSync(→ .repair.json)` | 修复备份旁车 | 首次修复时创建 |
| `core/bridge-session-manager.ts` bridge index（`writeIndex`） | `bridge/index.json` | bridge 会话映射 |
| `core/data-epoch-restore.ts:545` `appendFile` | epoch 恢复日志 | 非会话文件 |
| `lib/debug-log.ts`、`core/security-audit-log.ts`、`lib/workflow/journal.ts`、`lib/desk/cron-store.ts`、`lib/terminal/terminal-session-manager.ts`、`lib/channels/channel-store.ts`(频道 MD `appendFile`)、`server/index.ts:1152`、`server/routes/sessions.ts:2769`(switch-error.log) | 各自专用日志/store | 均非会话 JSONL（已逐一核实目标路径） |

---

## 4. 跨进程写入可能性

- **Lingxi 自身**：`server/index.ts`「同宅互斥闸」（257–286）：启动时用 token 认证探测
  `server-info.json`，同宅已有活内核则 `process.exit(1)`；desktop 客户端按 LINGXI_HOME 设置
  userData 单实例锁（`desktop/src/shared/single-instance-lock.cjs`）。**已文档化的残余竞态**：
  「两个内核同时冷启动、都还没写下 server-info.json 的秒级窗口不设防」；同端口后到者
  EADDRINUSE 兜底，但 fallback 端口候选存在（`BIND_FALLBACK_CANDIDATE_CODES`），窗口内双内核
  理论上可各占一端口。CLI `hana serve` / `hana chat` 经同一互斥闸（`cli/server-runner.ts`）。
  hub（channel/dm router）在同一 server 进程内，不构成额外进程。
- **非 Lingxi 进程**：编辑器、云同步（iCloud/Dropbox 等）、备份/杀毒、用户手工 `cp/mv` ——
  无任何防护，对 `agents/*/sessions/*.jsonl` 与 archived 目录同样可见。文件系统层面不存在
  「只有本进程能写」的保证。
- **判定**：所有入口的「跨进程可能」= **是**（对外部进程无防护；对 Lingxi 双内核有互斥闸 +
  秒级竞态窗口）。因此任何不验证文件身份（dev/ino 等）的增量策略都不能只依赖尾部采样
  （任务书 C02 的结论在本表中成立）。注意 archive 的 `utimes` 会让 mtime 单独变化；
  云同步回写可能同时改 size 与 mtime 甚至内容，stat 签名无法区分「追加」与「同长度重写」。

---

## 5. 汇总表（阶段 C「可信追加」适用范围依据）

| # | 入口（文件:函数） | 类型 | 应用层通知 | 跨进程可能 | 旧前缀保证 |
|---|---|---|---|---|---|
| 1 | SDK `SessionManager._persist` flushed 分支（经 `core/` 全部 `appendMessage/appendCustomEntry/appendCustomMessageEntry/appendCompaction/...` 调用点：§3-A 全表） | 追加 | ✗（回合内可用 `session.subscribe` 挂钩） | 是 | ✅ append-only |
| 2 | SDK `_persist` wx 首 flush | 首次 flush | ✗ | 是 | 新文件（已存在则抛错） |
| 3 | `core/session-jsonl-file.ts:flushSessionManagerSnapshot`（含 `schedulePreAssistantSessionManagerFlush`、`createSession` 2259、fork/clone 各调用点） | 首 flush / 整文件重写 | ✗ | 是 | ✗ |
| 4 | SDK `_rewriteFile`（回合后：`core/session-inline-media-prune.ts:pruneSessionManagerEntries`；`core/session-coordinator.ts:_projectOversizedSessionHistory`、`continueDeletedAgentSession` 4340/4363） | 修复 / 整文件重写 | ✗ | 是 | ✗ |
| 5 | `core/session-jsonl-file.ts:repairOversizedSessionEntriesInFile`（读路径：`core/message-utils.ts:loadSessionHistoryMessages`；恢复路径：`_repairOversizedSessionHistory` ×4 处） | 修复 + 重写 + `.repair.json` 备份 | ✗（返回值被读路径丢弃） | 是 | ✗ |
| 6 | `core/session-health.ts:repairOrphanToolResultEntriesInFile`（`_repairOrphanToolHistory` ×4 处；bridge 1206/1758） | 修复（删条目）+ 重写 | ✗ | 是 | ✗ |
| 7 | `core/session-inline-media-prune.ts:repairSessionInlineMediaEntriesInFile`（`_repairInlineMediaHistory` ×4 处 + bridge 570/1216/1768） | 修复 + 重写 | ✗ | 是 | ✗ |
| 8 | `SessionManager.open` 空 header 写 / v1→v3 `migrateToCurrentVersion` 重写（`_setSessionFile`；经 `openSessionManagerAtCurrentBranch`、restore、bridge open 等一切 open） | 迁移 / 首次 flush | ✗ | 是 | ✗（v3 稳定文件不触发） |
| 9 | `core/session-coordinator.ts:_forkSessionAtNodeUnlocked`（`createBranchedSession` + 引用改写 + 多次 `flushSessionManagerSnapshot`） | 迁移（新建子文件）+ 重写 | ✗ | 是 | 源文件 ✅；子文件新 |
| 10 | fork 失败清理（`fsp.rm` 4277/4378/3293/3453；`_discardForkedSubagentChildSession`） | 删除 | ✗ | 是 | 文件消失 |
| 11 | `server/routes/sessions.ts:archiveActiveSessionCore`（rename 656 + `utimes` 674 + sidecar 随迁） | 归档（rename + touch） | ✗ | 是 | n/a（字节不变，revision 变） |
| 12 | `POST /sessions/restore`（rename 3116；残留目标 `unlink` 802） | 恢复（rename）/ 删除残留 | ✗ | 是 | n/a / 消失 |
| 13 | `permanentlyDeleteArchivedFile`（rename→`.deleting` 739 + `unlink` 762；`/sessions/cleanup` 批量） | 删除（两阶段） | ✗ | 是 | 消失 |
| 14 | `promoteActivitySession`（`renameSync` 7865，回滚 7881） | locator 迁移（rename） | ✗ | 是 | n/a |
| 15 | `executeIsolated` cleanup（`unlinkSync` 7975/7990 + manifest 墓碑 7955） | 删除 | ✗ | 是 | 消失 |
| 16 | `core/slash-commands/session-ops.ts:_rotateBridge` / `_deleteBridge`（bridge rename 102 / unlink 120） | 分支重置归档 / 删除 | ✗ | 是 | n/a / 消失 |
| 17 | `core/session-branch-head.ts` `applyStoredSessionBranchHead` / `persistExplicitSessionBranchHead` / `syncSessionBranchHeadAfterAppend` / `readManifestSessionBranch` → `core/session-manifest/store.ts:setBranchHead`(969) | 分支头持久化（含 append_recovery backfill、读触发恢复） | ✗ | manifest 单 kernel | n/a（不改 JSONL） |
| 18 | `core/session-manifest/store.ts:updateLocator / updateLocatorLifecycle / moveSessionLifecycle` | locator 重新绑定 / 生命周期迁移 | ✗ | 同上 | n/a |
| 19 | `session-meta.json` 写/删/隔离/压缩（`core/session-coordinator.ts` 7489–7689）+ `titles.json`（6872/7060） | 邻接 store 重写 | ✗ | 是 | n/a（非 JSONL） |

### 阶段 C 结论（直接可用）

1. **旧前缀不变（可增量续读）的只有 #1 与 #2/#9 的新建文件情形**；#1 是唯一对已存在文件
   的 append-only 路径。启用可信增量的前提链：单 kernel（互斥闸成立但存在秒级竞态 → 保留
   dev/ino 身份校验）＋ 单活跃 SessionManager（需防御 `ensureSessionLoaded` 注释记录的幽灵
   写者形态）＋ 旧记录边界有效 ＋ 通知覆盖全部重写路径。
2. **当前没有任何入口具备可信应用层通知**。若 C02 要求「重写失效先于旧偏移失效可见」，
   需要在以下同步边界注入内存世代通知（均为 Hana 代码内、不改 SDK）：#3 `flushSessionManagerSnapshot`
   入口、#4 两个回合后重写 helper、#5/#6/#7 三个 repair helper（及其全部调用点）、#8 不可注入
   （SDK 内部，只能以「open 后 revision/内容核对」覆盖）、#9–#16 各生命周期入口（含
   `moveSessionLifecycle`/`setBranchHead` 的 manifest 变化）。
3. **读路径自身会写**（#5、#8、#17 的 `readManifestSessionBranch` 恢复写、`loadSessionHistoryMessages`
   的修复 + 冷开）。新目录构建器必须绕开这些副作用（任务书 B02 已规定），否则「只读扫描」的
   快照不变式（I06）不成立。严格 reconciliation 路径（`loadSessionHistoryEvidence`）已核实为
   纯读，是唯一现成的无副作用读取先例。
4. **revision 语义陷阱**：`revision = ${size}:${mtimeMs}`；归档 `utimes`、修复重写、
   云同步 mtime 扰动都能使 revision 变化而语义未变，或同长度重写使 size 不变。仅凭 stat 差异
   无法分类变化（C01 决策表必须依赖内部身份 + 通知，不能只看 stat）。

## 复核指引

- SDK 原语：`node_modules/@earendil-works/pi-coding-agent/dist/core/session-manager.js`
  （`_persist`/`_rewriteFile`/`_setSessionFile`/`migrateToCurrentVersion`/`createBranchedSession`）。
- 读路径写副作用：`core/message-utils.ts:loadSessionHistoryMessages`（修复 + 冷开）；
  对照纯读的 `loadSessionHistoryEvidence`。
- 恢复路径修复簇：`core/session-coordinator.ts` `switchSession` / reloadSession /
  `_loadSessionForAttach` / `executeIsolated` resume 四处 `SessionManager.open` 之前的三修复。
- 生命周期：`server/routes/sessions.ts` `archiveActiveSessionCore` / `permanentlyDeleteArchivedFile` /
  restore 路由 / cleanup 路由；`core/session-coordinator.ts:promoteActivitySession`。
- 互斥闸与残余竞态：`server/index.ts`（「同宅互斥闸」注释块）、
  `desktop/src/shared/single-instance-lock.cjs`。
- watcher 排除证据：`core/skill-manager.ts`、`lib/knowledge/source-file-watcher.ts`、
  `lib/file-history/workspace-watcher.ts`（监听工作区而非会话）、`lib/resource-io/resource-watch-registry.ts`。
