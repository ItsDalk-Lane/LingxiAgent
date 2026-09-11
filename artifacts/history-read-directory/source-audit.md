# source-audit.md — SRC01–SRC16 证据逐条复核（任务书「证据索引与复核位置」）

- 仓库：`ItsDalk-Lane/LingxiAgent`，分支 `fix/pending-sep10`，实际 HEAD `1d42b7405c76292f617291e3a01cd2f3ef5efd04`（与任务书固定基线一致，2026-09-10 复核）。
- 工作区状态：`git status` 仅两项非本文件变化——①已跟踪文件 `tests/history-pagination-run-continuity.test.ts` 被修改（A03 夹具修正，未提交）；②未跟踪新增 `tests/history-pagination-invalid-fixture.test.ts` 与 `artifacts/history-read-directory/`。**其余全部 SRC 涉及的生产文件与 HEAD 完全一致**，行号即 HEAD 坐标（函数名为权威锚点）。
- 复核方法：生产代码逐文件打开核对函数与行范围；SRC01 同时核对 HEAD 版本（`git show HEAD:`）与工作区修正后版本；SRC11/SRC13/SRC16 按任务书要求只核本地 Node 版本与依赖内实际可用的 API 形态（`node -e` 实测 + node_modules 内类型/源码取证），未联网。
- 结论口径：确认 / 部分确认 / 未复现 / 已变化 / 被否定。

## 汇总

| 编号 | 结论 | 一句证据 |
|---|---|---|
| SRC01 | 确认（HEAD）＋已修正（A03，工作区未提交） | HEAD :93 `jsonlLine(parent, parent, …toolResult)` 复用 assistant id 且 parentId 自引用属实；A03 已改为合法链 u1→a1→r1→…，maxPages 改为按 display 推导 |
| SRC02 | 确认 | `core/message-utils.ts` 四函数逐一定位：118 / 161 / 169 / 203，行为与声称一致 |
| SRC03 | 确认 | 路由 `sessions.ts:1466`、`resolveHistoryPageBounds:362`、四组预扫描 :1504-1670、主循环 :1788-2012 |
| SRC04 | 确认 | `session-jsonl-file.ts`：repair:244、writeSessionEntriesFile:297、flushSessionManagerSnapshot:313 |
| SRC05 | 确认 | `sessionFileRevision` :19-21 返回 `` `${stat.size}:${stat.mtimeMs}` ``；修订点在读内容前取（sessions.ts:1495，注释 :1492-1494） |
| SRC06 | 确认 | `projectCurrentSessionBranchEntries` :193-266（persisted_head / append_recovery / legacy_tail）；`session-branch-head.ts` apply/persist/sync/readManifest 四入口齐全 |
| SRC07 | 确认 | deferred 回灌 :2014、subagent :2042、workflow :2120、sessionFiles :2145、todos :2149、rebroadcast :2154、reconciliation 载荷 :2158-2166、响应 :2167 |
| SRC08 | 确认 | locator = `{version, sourceIndex, entryId, kind, ordinal}`（history-deferred-content.ts:28-32）；创建 :80、展开端点 sessions.ts:1392 |
| SRC09 | 确认 | engines `>=24.12.0 <25`；Pi 三包精确锁 0.84.1；vitest ^4.1.10；typecheck 三配置；test 六项 exclude |
| SRC10 | 确认 | `.gitignore:16` `AGENTS.md`；`git check-ignore` 命中；本地文件存在（5619 字节） |
| SRC11 | 确认（本地形态） | Node v24.16.0；FileHandle.read 带 position 实测 bytesRead=实际字节、越界 0、close 后 EBADF |
| SRC12 | 确认 | guard 从指纹 JSON 派生受护集合（160 个文件）；兼容 repin 命令与任务书 D03 引用逐字一致 |
| SRC13 | 确认（本地形态） | hono 4.13.1 `c.body(null, status, headers)` 可表达 304 无正文；自带 etag 中间件；运行时强制 304 null-body |
| SRC14 | 确认 | `throwOnHttpError` 默认 true，非 2xx 抛错（use-hana-fetch.ts:39/:55-57），签名与任务书一致 |
| SRC15 | 确认 | 四函数 + 全部护栏（_switchVersion / _loadMessagesVersion / live version / revision 补拉）逐一定位 |
| SRC16 | 部分确认 | 本地 API 形态一致（RequestCache 含 no-store；Node fetch 无 HTTP 缓存存储、304 null-body 强制）；「浏览器实际行为」本地不可验证，任务书自身留待 F |

计数：**确认 15 条**（其中 SRC01 附状态变化说明，SRC11/SRC13 为本地 API 形态确认），**部分确认 1 条**（SRC16）。无未复现、无被否定、无与任务书相悖的出入。

---

## 逐条复核

### SRC01 — 长单轮测试夹具（重复 ID / 自引用 parent / 简化 buildApp / maxPages=200 / 旧耗时记录）

**任务书声称**：`tests/history-pagination-run-continuity.test.ts` 的 `writeLongRunSession` 存在 toolResult 与 assistant 重复 ID、toolResult 自引用 parentId；`buildApp` 为简化实现；翻页驱动器硬编码 `maxPages=200`；`artifacts/history-pagination-run-continuity/evidence/fix-verification.json` 记录旧耗时（140 规模 3 页约 7,075ms、1,000 规模 21 页约 52,992ms）。任务书并要求先修正夹具再测量。

**实际位置**：
- HEAD 版本（`git show HEAD:tests/…`）：`writeLongRunSession` 在 :66-103，:93 `jsonlLine(parent, parent, {role:"toolResult", …})` —— toolResult 的 id 与 parentId 均复用 assistant id（重复 ID + 自引用），**声称属实**；`buildApp` HEAD:105（两参简化桩）；`driveLoadUntilExhausted` HEAD:129 默认 `maxPages = 200`、:137 超限抛「分页失控」。
- 证据文件 `artifacts/history-pagination-run-continuity/evidence/fix-verification.json` 实存：before（旧游标规则）140 规模 92 页 / 237,966ms；after 140 规模 3 页 / 7,075ms、1,000 规模 21 页 / 52,992ms —— 与任务书 :34 引用的 7,075ms / 52,992ms 逐字一致。

**A03 修正后现状（工作区，未提交；见 fixture-audit.json 与 PROGRESS.md）**：
- `writeLongRunSession` 现为 :81-122：1 个 `{type:"session",version:3,id:<UUID>}` 文件头（SDK UUID，:57 常量）+ 1 user + N assistant + (N-1) toolResult，物理行 2N+1、display N+1；每条 toolResult 独立 id `r<i>`、parentId 指向对应 assistant，链 u1→a1→r1→a2(parent=r1)→…→aN(parent=r<N-1>)；支持带/不带尾换行变体。业务 sessionId 由真实 `SessionManifestStore`（SQLite）按生产格式生成，与 SDK 文件头身份两套并存。
- `buildApp` 扩展为 :165（agentsDir, sessionPath, sessionManifest, branchTracker），走 `engine.openSessionManagerAtCurrentBranch` 真实分支入口并计数（A03 取证：3 次请求 openThrows=0、getBranchThrows=0、fallbackReads=0）。
- `driveLoadUntilExhausted` 现为 :220-249：`maxPages = pagesForDisplay(displayCount, 50) + 2`（:226-228），不再硬编码 200；死循环保护保留并由 T12c（:515）验证超限显式报错。新增 T12 规模定义（:489，1k=21 页、10k=201 页）、T12b 尾换行变体（:523）、A03 夹具审计契约 describe（:591，`A03_FIXTURE_AUDIT_OUT` 可复跑再生成，:684）。
- 另有未跟踪新文件 `tests/history-pagination-invalid-fixture.test.ts`（8 测试：重复 ID/自引用/三环/缺父的严格层拒绝码、SRC01 形状兼容降级记录、SDK RangeError 取证、坏尾行修复行为）。
- 修正前后运行记录见 fixture-audit.json `testRuns`：修正前 5/5 绿但实际走兼容 raw-read fallback（SDK 对自引用形状 `getBranch()` 抛 RangeError 被 catch）；修正后 10/10 绿（~2.0s，分支路径、fallback 0）；合跑 18/18。

**结论**：确认（HEAD 原状与声称完全一致）＋已修正（A03 修正落在工作区、未提交；修正后夹具合法、驱动器按 display 推导上限、性能样本首次真正经过生产分支读取路径）。

### SRC02 — `core/message-utils.ts` 四函数

**任务书声称**（:32 及索引）：普通历史路径经 `loadSessionHistoryMessages()` 读取，含文件修复、SessionManager 打开、分支恢复与投影；另有投影/严格对账入口。

**实际位置与证据**：
- `loadSessionHistoryMessages(engine, explicitPath, options)` **:118-159**：`readSideEffects===false` 转 evidence（:121）；`looksLikePiSessionFile` → `repairOversizedSessionEntriesInFile(sessionPath)`（:125，隐式修复）→ `engine.openSessionManagerAtCurrentBranch`（:126-128）→ `getBranch()`（:129）→ `projectBranchHistory`（:132，sessionId 仅当 manifest locator 匹配才传入）；任一步抛错被 :134-136 吞掉后落入 :138-156 的逐行 raw-read 兼容回退（无分支校验、坏行跳过），最终失败返回 []（:158）。与声称的「修复检查、SessionManager 打开、当前分支恢复和消息投影」一致。
- `projectBranchHistory` **:161-166**：`collectDesktopInputCorrelations` + `historyMessageFromEntry` + `projectSessionMessageForDisplay`。
- `loadSessionHistoryEvidence(engine, sessionPath, requestedSessionId?)` **:169-201**：严格对账读取——身份链五重校验、`fs.readFile` 全读、单头 version===3、运行时实例校验、读后 branchHead 双快照对比（:190-194）→ `projectCurrentSessionBranchEntries`；**不修复文件/manifest**（:168 注释与实现一致）。
- `historyMessageFromEntry` **:203 起**：legacy 工具失败补 `isError:true`（:205-209，经 `isKnownLegacyLingxiToolFailure`），entry.id/timestamp 拷到 message（:210-211, :222-223）。

**结论**：确认。

### SRC03 — `/sessions/messages` 路由、分页边界、预扫描与主循环

**任务书声称**（:32 及索引）：路由随后还进行多组 origin、presentation、review、Run 边界、工具结局、输入消费等处理；分页窗口之外仍存在重复的全历史工作。

**实际位置与证据**（`server/routes/sessions.ts`，全长 3,2xx 行）：
- `route.get("/sessions/messages")` **:1466-2167+**；`reconciling = query('reconciliation')==='1'`（:1488）。
- 修订点竞态纪律：注释 :1492-1494 + `readSessionFileRevision` :1495，先于 :1497 的 `loadSessionHistoryEvidence` / `loadSessionHistoryMessages`（见 SRC05）。
- 预扫描（全部基于投影后 sourceMessages、纯内存）：origin :1504-1523（`annotateOriginMessages` zip 回原始下标）；presentation :1525-1539（`MESSAGE_PRESENTATION_RECORD_TYPE`）；agentReview :1540-1554（仅 `status==="completed"` 下发，:1550）；Run 边界 :1555-1595（displayCounter 与主循环同源，:1563 `runBoundsBySourceIndex`，注释明示两侧必须同步修改）；consumption/entryId 索引 :1619-1670（`turnInputConsumptionDeliveryIds/EntryIds` :1619-1620、`sourceIndexByEntryId/displayIndexByEntryId` :1622-1623、`parseTurnInputConsumptionRecord` :1657）。另有 `collectSessionCollabDecisions` :1629、`collectToolOutcomesByCallId` :1630、`collectModelCallReferencesBySourceIndex` :1631。
- `resolveHistoryPageBounds(sourceMessages, {beforeId, limit, forceAll})` **:362-375**（display 序号窗口切分；`isDisplayableHistoryMessage` :294）。
- 主展示循环 **:1788-2012**（`for (let sourceIndex…)`），页切片 :2029、`nextBefore = String(startIdx)` :2038。

**结论**：确认。预扫描 + 主循环 + 全文件读取（SRC02 链）即「窗口外重复全历史工作」的实体。

### SRC04 — `core/session-jsonl-file.ts` 修复/重写原语

**任务书声称**（:36 及索引）：会话文件并非绝对 append-only：修复与快照刷新存在整文件重写路径，不能仅凭 size 增大认定追加。

**实际位置与证据**：
- `repairOversizedSessionEntriesInFile(sessionPath, opts)` **:244-295**：读全文 → 逐行解析 → 投影超大行/剔除坏行 → 备份 `.repair.json` → 整文件重写（经 :297）。
- `writeSessionEntriesFile(sessionPath, entries)` **:297-311**：应用层唯一 JSONL 直写出口。
- `flushSessionManagerSnapshot(sessionManager, opts)` **:313-324**：把 SDK 内存前缀物化（`manager._rewriteFile()` + 置 flushed），配套 `schedulePreAssistantSessionManagerFlush` :326（每回合 microtask 触发）。
- 同文件还有内存版 `repairOversizedSessionEntries` :87、`projectOversizedSessionEntry` :61（细节与 writer-invalidation-map §1/§3-D 一致）。

**结论**：确认——三条整文件重写路径（读前修复、快照 flush、回合后投影）全部实存。

### SRC05 — `sessionFileRevision` 与读取前修订点

**任务书声称**（:38 及索引）：公开 `revision` 当前是 `` `${stat.size}:${stat.mtimeMs}` ``；文件 stat 签名不能当作整个响应和全部分支语义的证明。

**实际位置与证据**：
- `core/session-list-projection-cache.ts:19-21`：`sessionFileRevision(stat) { return `${stat.size}:${stat.mtimeMs}`; }` —— 逐字符一致；该签名同时用于列表投影缓存失效（:56、:127）与路由响应 revision。
- `server/routes/sessions.ts:1492-1494` 注释明确「修订点必须在读取内容之前取……不会偏新（把没读到的写入标成已同步……issue #1610 的反方向竞态）」，:1495 实际执行；messages 与 find（:1168 附近）同纪律。
- 分支侧另有 `leafId` / `observedTailLeafId` 语义（SRC06），响应还含 store 状态（SRC07）——「stat 签名 ≠ 全响应证明」成立。

**结论**：确认。

### SRC06 — 分支投影与持久化 head / append recovery

**任务书声称**（:38 及索引）：分支解析另有 `leafId`、`observedTailLeafId`、物理尾部等语义。

**实际位置与证据**：
- `lib/session-jsonl.ts`：`projectCurrentSessionBranchEntries(entries, opts)` **:193-266**。已读实现核对：`buildValidatedEntryIndex`（:92-156）负责 id 缺失合成（`legacySyntheticIds` :95-97）与重复/悬空/环等 `SessionBranchError`（class :7）；无 head → `legacy_tail`（:210 默认物理尾）；有 head 时 `persistedLeafId` 缺失即抛 `session_branch_head_missing`（:202-206）；物理尾是持久 head 的后代且非延续被抛弃尾 → `append_recovery`，否则 `persisted_head`（:211-227）；返回 `recommendedHead{leafId, observedTailLeafId, reason}`（:259-263）。`readCurrentSessionBranch` :188 为同步全读包装。
- `core/session-branch-head.ts`：`applyStoredSessionBranchHead` **:79-118**（不匹配/尾前进时 `manifestStore.setBranchHead` :108，reason `append_recovery` :110-111 —— 读路径隐式写 manifest）；`persistExplicitSessionBranchHead` :120-151、`syncSessionBranchHeadAfterAppend` :153-171（写侧权威化）；`readManifestSessionBranch` :173-199（persistRecovery 默认写回 :185-188）；`getPhysicalSessionTailLeafId` :48。

**结论**：确认。

### SRC07 — 主循环后状态补齐、sessionFiles、todos、rebroadcast、reconciliation

**任务书声称**（:849 及索引）：消息响应包含文件之外的状态；严格对账的随机 snapshotId 不在 E1 适用范围。

**实际位置与证据**（均在 `server/routes/sessions.ts`）：
- deferred 终态回灌 :2014-2021（`deferredStore.listBySession` :2015）；`resolveMediaGenerationBlocks` :2022-2026；页切片 :2029-2033。
- subagent 块终态 :2042-2118（`engine.deferredResults.query` + `engine.subagentRuns.query` :2044-2048、session-meta 缓存、子会话尾读摘要）；workflow 终态 :2119-2143（runStore 回填 finishedAt）。
- `patchSessionFileLifecycleBlocks` 调用 :2144（定义 :3185）；`sessionFiles = listSessionRegistryFiles(...)` :2145（定义 :3217，入参 `activeReferences = sourceMessages`）；`todos = extractLatestTodos(sourceMessages)` :2149。
- `engine.activityHub?.rebroadcastSession?.(...)` :2153-2156，条件 `!reconciling && beforeId == null`（首屏非翻页）。
- reconciliation 载荷 :2158-2166：`snapshotId: randomUUID()`（:2160，随机）、runRevision/runStatus/complete/diagnostic；最终响应 :2167 `{ messages, blocks, todos, hasMore, nextBefore, sessionFiles, revision, reconciliation? }`。

**结论**：确认——「随机 snapshotId 存在但不属于 E1 可稳定化字段」的前提属实。

### SRC08 — 延迟内容 locator 与描述符

**任务书声称**（:68 及索引）：允许给描述符生成函数加内部 locator 适配，不改对外格式/种类/阈值/解析语义；locator 含 sourceIndex/entryId/kind/ordinal。

**实际位置与证据**（`server/history-deferred-content.ts`）：
- `HISTORY_INLINE_CONTENT_LIMIT = 8 * 1024` :7；`shouldDeferHistoryContent` :76-78（>8KiB）。
- locator 形状 :28-32：`{ version, sourceIndex, entryId: string|null, kind, ordinal }`；`encodeLocator` :47（base64url JSON）、`decodeLocator` :51-74（校验 version/kind/序号/entryId 类型）。
- `createHistoryDeferredContent(sourceMessages, sourceIndex, kind, ordinal, content, {preview})` :80-103（descriptor `{id, kind, size, preview?, available}`）；`resolveHistoryDeferredContent` :105 起——`currentEntryId !== locator.entryId` 即返回 null（:116，fail-closed）；展开端点 `route.get("/sessions/content/:contentId")` 在 sessions.ts **:1392**（每次整链重读，无缓存）。

**结论**：确认。六种 kind 与解析语义与任务书描述一致；窗口化适配只需替换 sourceIndex 的来源即可保持对外格式不变。

### SRC09 — `package.json` 四项

**任务书声称**（:180、:705 及索引）：Node `>=24.12.0 <25`；固定 Pi 版本；Vitest 依赖；typecheck 已串联三份配置；test 脚本排除项。

**实际位置与证据**：
- `engines.node` = **`">=24.12.0 <25"`**（:26-28）；本机 Node v24.16.0 满足。
- Pi SDK 固定版本：`@earendil-works/pi-agent-core` / `pi-ai` / `pi-coding-agent` 均为精确锁定 **`0.84.1`**（无 caret，:80-82）。
- Vitest：devDependencies `vitest: ^4.1.10`（:152）。
- `typecheck`（:39）= `tsc --noEmit && tsc --noEmit -p tsconfig.node.json && tsc --noEmit -p tsconfig.test.json` —— 三份配置即 **根 `tsconfig.json`（默认）+ `tsconfig.node.json` + `tsconfig.test.json`**。
- `test`（:62）= `vitest run` 带 **六个 `--exclude`**：`**/.claude/**`、`**/.cache/**`、`**/dist/**`、`**/dist-server/**`、`**/dist-computer-use/**`、`**/dist-sandbox/**`（`test:watch` :73 同）。

**结论**：确认（与任务书 :180、:705 的表述一致）。

### SRC10 — `.gitignore` 的 AGENTS.md 规则

**任务书声称**（:161 及索引）：根 `AGENTS.md` 在远端未跟踪不代表本地不存在；本地缺失时明确记录，不编造内容。

**实际位置与证据**：`.gitignore:16` 为 `AGENTS.md`；`git check-ignore -v AGENTS.md` → `.gitignore:16:AGENTS.md	AGENTS.md`（确认被忽略、不入库）；本地文件实存（5,619 字节，2026-09-06），内容与会话注入的项目规则一致。

**结论**：确认。

### SRC11 — Node 24 File system API（外部规范 → 本地形态复核）

**任务书声称**（:391 及索引）：使用明确 position 的读取；检查实际 `bytesRead` 并循环补齐；异常 EOF 重新校验；所有分支关闭句柄；实现按实际锁定运行时 API 核实。

**本地实际形态（node -e 实测，Node v24.16.0，@types/node 24.13.2）**：
- `filehandle.read(buffer, offset, length, position)` 返回 `{ bytesRead, buffer }`；`bytesRead` 为**实际**读取字节数：满读 16、边界短读按实际、position 越过 EOF 时 `bytesRead === 0`（不抛错）——「把未填充 Buffer 当有效 JSON」的风险真实存在，循环补齐纪律必要。
- 句柄生命周期：`close()` 后再 read 抛 `EBADF`——所有分支必须显式 close。
- 运行时在 engines 范围内（`>=24.12.0 <25`），API 形态与任务书实现约束吻合。

**结论**：确认（规范文本本身未联网复核，按指示以本地运行时 API 形态为准）。

### SRC12 — 持久化 schema 指纹 guard 与 repin

**任务书声称**（:719-722、D03 及索引）：脚本支持兼容重钉形式；受护集合决定 B/E 改哪些文件要 repin。

**实际位置与证据**（`scripts/check-persistence-schema-fingerprint.mjs`，171 行）：
- 指纹路径常量 `FINGERPRINT_PATH = "build/persistence-schema-fingerprint.json"`（:37）；`guardedFiles()`（:54-69）从指纹 JSON 的 **`siteMappings[].sourceFile` + `schemas[].module` + `schemas[].extensions[].module` + `schemas[].protocolModules[].module`** 派生受护集合——**实析出 160 个源文件**（全清单见附录 A）。
- 与完整 tripwire 的关系：权威断言是 `tests/persistence-schema-tripwire.test.ts` 的 `assertCommittedPersistenceSchemaFingerprint`（:3-7、:139，在 `npm test` 内运行）；本脚本是快速 diff guard，**有意超集**——纯注释/空白编辑 hash 不变、tripwire 不触发，但 guard 仍要求 repin（:14-18、:165-167）。
- 兼容 repin 命令（:152-156）＝任务书 D03 引用形式逐字一致：`node scripts/generate-persistence-schema-fingerprint.mjs --classification compatible --compatibility-reason "<…>"`；breaking 形态（:157-163）要求 bump DATA_EPOCH（`--classification breaking --source-data-epoch --target-data-epoch --affected-store --checkpoint-policy --restore-policy`）。生成脚本实存且实际接受这些旗标（`generate-persistence-schema-fingerprint.mjs:818/:825-834`）。
- 本任务相关的受护文件（B/E 触及即须同 diff repin）包括：`server/routes/sessions.ts`、`server/index.ts`、`core/session-jsonl-file.ts`、`core/session-manifest/store.ts`、`core/session-manifest/checkpoint.ts`、`core/session-coordinator.ts`、`core/engine.ts`、`lib/session-files/session-file-registry.ts`、`lib/deferred-result-store.ts`、`lib/subagent-run-store.ts`、`lib/subagent-executor-metadata.ts`、`lib/loop/loop-store.ts`、`core/slash-commands/session-ops.ts`、`lib/tools/dm-tool.ts` 等（均见附录 A）。

**结论**：确认。

### SRC13 — RFC 9110 ETag / 条件请求 / 304（外部规范 → 本地形态复核）

**任务书声称**（:866、:876 及索引）：采用 `W/"hrp1-<digest>"` 弱 ETag；304 必须无 JSON 正文；协议头语义纳入标签版本；阶段 E 使用，授权来自本任务而非标准文档。

**本地实际形态**：
- 服务框架 hono **4.13.1**：`c.body` 的重载签名支持 `(data: null, status: StatusCode, headers: HeaderRecord)`（`node_modules/hono/dist/types/context.d.ts:96`）——「304 无正文 + 携带 ETag/Vary 头」可直接表达；`c.json` 无法用于带正文 304（类型层就区分 `ContentfulStatusCode`）。
- hono 自带 `etag` 中间件（`node_modules/hono/dist/middleware/etag/`，digest 实现同目录）可作 ETag/If-None-Match 处理的仓库内先例。
- 运行时 null-body 语义实测（Node 24.16 内置 undici）：`new Response(null, {status: 304})` 合法且 `body === null`；`new Response("x", {status: 304})` 抛 `TypeError: Invalid response status code 304` —— 304 带正文在运行时层即被拒绝，与任务书 :876 约束一致。
- RFC 9110 文本本身未联网复核（按指示）；「授权来自本任务」是任务书内部授权声明，非源码可证伪项，如实记录。

**结论**：确认（本地 API 形态完全支持任务书的 304/ETag 实施约束）。

### SRC14 — `lingxiFetch` 默认抛错与 `throwOnHttpError`

**任务书声称**（:892 及索引）：`lingxiFetch` 默认对非 2xx 抛错；已有 `throwOnHttpError` 选项；304 接线不得改坏所有调用者。

**实际位置与证据**（`desktop/src/react/hooks/use-hana-fetch.ts`，全文 62 行）：
- 签名 **:26-29**：`lingxiFetch(path: string, opts: RequestInit & { timeout?: number; throwOnHttpError?: boolean } = {}): Promise<Response>`。
- `throwOnHttpError` **默认 `true`**（:39 解构默认值）；`:55-57` 在 `throwOnHttpError && !res.ok` 时抛 `new Error(\`lingxiFetch ${path}: ${res.status} ${res.statusText}\`)` ——「默认对非 2xx 抛错」属实；传 `false` 即返回原始 Response 供调用方自行处理状态码。
- 其余形态：默认 30s 超时（:8/:37/:43，AbortController）；caller signal 桥接（:44-47）；连接鉴权 `appendConnectionAuth`（:34）；URL 经 `buildConnectionUrl`（:50，不带 token query；带 token 的是 `lingxiUrl` :13-19）。
- E 接线只需在历史协议调用点传 `throwOnHttpError: false` 并先判 304，不触碰默认路径 → 「不得改坏所有调用者」具备可行性。

**结论**：确认。

### SRC15 — `session-actions.ts` 函数与现有护栏

**任务书声称**（:890 及索引）：`sessionMessagesUrl`、`fetchSessionHistoryPage`、`loadMessages/loadMoreMessages`；连接/切换/流式版本等护栏；区分「校验当前已加载表示」与「重建/恢复/补全」，后者无条件请求。

**实际位置与证据**（`desktop/src/react/stores/session-actions.ts`，全文 1,894 行）：
- `sessionMessagesUrl(path, extra)` **:186-196**：`path` 必带；`sessionId` 来自 store manifest 映射（有才带，:190-191）；`extra` 附加（如 `{before}`）。
- `fetchSessionHistoryPage(connection, sessionRef, {before, signal})` **:151-184**：只读对账专用——`reconciliation=1`（:156）；认证/URL 取自**捕获的连接**（:158-159，原生 fetch，不经 lingxiFetch）；`!response.ok` 抛 `history_http_<status>`（:161）；2MiB 上限（content-length 预检 :163 + 流式累计 :172 + 全量兜底 :179）；结构校验 :182。
- `loadMessages(forPath?)` **:399-499**：护栏链——message live version 快照（:402）、todos live version 快照（:405-406）、`bumpLoadMessagesVersion` 每会话版本（:409）+ stale 丢弃（:417-423）、SessionFile flight（issue #2188，begin :413 / consume+resetSeen 决定快照与 upsert 重放 :424-441）、live version 早退（:442-458）、修订点 stamp `revision`（:468）→ `initSession(path, items, hasMore, revision, historyNextCursor(data))`（:472-478，hasMore 以服务端为准）、流式快照 `openTailRun`（:465-466）+ 在途 assistant 追加（:486-492）、失败路径也消费 flight（:497）。
- `loadMoreMessages(forPath?)` **:551-584**：`loadingMore` 守卫（:555）；`before = session.nextBefore ?? session.oldestId`（:561，服务端游标优先）；游标缺失诊断终止（:570-576）；`prependItems`（:579）。
- 连接/切换护栏：模块级 `_switchVersion`（:34，bump :60，判定 `isCurrentSwitch` :66-68），检查点遍布 switch/reload 路径（:846、:951、:999、:1048、:1072、:1135）；修订点对比补拉（issue #1610）:586-635，per-session in-flight 去重 `_revisionReconcileInFlight`（:592）；流式进行中不补拉/缓存命中跳过 hydrate 时校验修订点（:946-951、:987-999 注释）。

**结论**：确认。任务书 :890 要求区分的两类语义在代码中已有对应护栏实体，条件请求只可挂在「校验当前已加载表示」一侧。

### SRC16 — RFC 9111 / WHATWG Fetch 缓存语义（外部规范 → 本地形态复核）

**任务书声称**（:894、:1078 及索引）：显式传 `cache:'no-store'`，不依赖浏览器 HTTP cache 自动保存正文或把网络 304 合成 200；不设置 public/s-maxage 共享缓存；真实浏览器行为必须在 F 验证。

**本地实际形态**：
- WHATWG `RequestCache` 类型含 `"no-store"`（TypeScript 5.9.3 `lib.dom.d.ts:39391`：`"default" | "force-cache" | "no-cache" | "no-store" | "only-if-cached" | "reload"`）——renderer（Electron 42.8.1，Chromium 网络栈）具备任务书要求的 cache mode 形态。
- Node 侧（Node 24.16.0 内置 undici 7.25.0；依赖另精确钉 undici 7.29.0）：`fetch(url, {cache:'no-store'})` 接受并正常完成（实测）；`'only-if-cached'` 强制要求 `mode:'same-origin'`（`node_modules/undici/lib/web/fetch/request.js:341-345`）；**undici fetch 没有任何 HTTP 缓存存储实现**（`lib/web/fetch/` 内无 cache.put/HTTPCache 等）——Node 端不存在「自动保存正文」的问题，与服务端/测试环境前提一致。
- null-body status（204/205/304，WHATWG Fetch 语义）在运行时被强制（实测 Response 构造器，见 SRC13）。
- 「私有/no-store 响应头对浏览器共享缓存与 CORS 的实际效果」属浏览器网络栈行为，本地（无浏览器运行验证）不可证——任务书自身也规定真实浏览器行为留待 F 阶段验证。

**结论**：**部分确认**——本地可核的 API 形态全部与任务书前提一致且支持其设计（显式 no-store、应用层自管 ETag 元数据）；「浏览器实际行为」部分本地不可验证，按任务书安排属于 F 阶段。

---

## 附录 A：SRC12 受护源码全清单（`build/persistence-schema-fingerprint.json` 派生，160 个文件）

以下任一文件被改动而未在同一 diff 内 repin 指纹，`scripts/check-persistence-schema-fingerprint.mjs` 与 `tests/persistence-schema-tripwire.test.ts` 门禁即红：

core/agent-manager.ts, core/agent.ts, core/agents-md-migration.ts, core/bridge-session-manager.ts, core/channel-manager.ts, core/computer-use/providers/windows-uia-provider.ts, core/credential-backup-retention.ts, core/data-epoch-checkpoint-provider.ts, core/data-epoch-coordinator.ts, core/data-epoch-migrations.ts, core/data-epoch-restore.ts, core/device-registry.ts, core/engine.ts, core/execution-lease-registry.ts, core/first-run.ts, core/grant-registry.ts, core/input-drafts-store.ts, core/local-provider-plugin-store.ts, core/local-user-account.ts, core/mcp/manager.ts, core/media-adapters/agnes.ts, core/media-adapters/speech.ts, core/media/download.ts, core/media/local-cli-wrapper.ts, core/media/task-store.ts, core/media/universal-media-manager.ts, core/model-sync.ts, core/mount-aware-file-service.ts, core/pinned-tenets-migration.ts, core/pinned-tenets-recovery.ts, core/plugin-config.ts, core/preferences-manager.ts, core/provider-auth-migration.ts, core/provider-catalog.ts, core/resource-ticket-service.ts, core/security-audit-log.ts, core/server-identity.ts, core/server-network-config.ts, core/session-coordinator.ts, core/session-jsonl-file.ts, core/session-manifest/checkpoint.ts, core/session-manifest/db-files.ts, core/session-manifest/store.ts, core/session-project-catalog-store.ts, core/slash-commands/session-ops.ts, core/studio-cron-service.ts, core/studio-mounts.ts, core/vision-bridge.ts, core/web-session-store.ts, desktop/auto-updater.cjs, desktop/bootstrap.cjs, desktop/file-text-io.cjs, desktop/main.cjs, desktop/src/office-pdf-helper.cjs, desktop/src/shared/artifact-gc.cjs, desktop/src/shared/artifact-repair.cjs, desktop/src/shared/desktop-launch-diagnostics.cjs, desktop/src/shared/gpu-startup-policy.cjs, desktop/src/shared/launch-integrity.cjs, desktop/src/shared/win32-install-acl-heal.cjs, hub/agent-executor.ts, hub/channel-router.ts, hub/index.ts, lib/agent-appearance-summary.ts, lib/bridge/wechat-adapter.ts, lib/browser/browser-manager.ts, lib/channels/channel-store.ts, lib/character-cards/service.ts, lib/checkpoint-store.ts, lib/compat/checks/config-yaml.ts, lib/compat/checks/dirs.ts, lib/compat/checks/facts-db.ts, lib/conversations/agent-phone-projection.ts, lib/conversations/agent-phone-runtime.ts, lib/debug-log.ts, lib/deferred-result-store.ts, lib/desk/activity-store.ts, lib/desk/cron-store.ts, lib/desk/desk-manager.ts, lib/desk/heartbeat.ts, lib/diary/diary-writer.ts, lib/exec-command/runner.ts, lib/extract-zip.ts, lib/file-history/file-history-service.ts, lib/file-history/history-store.ts, lib/file-ref/resource-io.ts, lib/knowledge/ann-index-store.ts, lib/knowledge/knowledge-index-store.ts, lib/knowledge/knowledge-manager.ts, lib/knowledge/knowledge-store.ts, lib/knowledge/usearch-vector-backend.ts, lib/knowledge/vector-index-adapter.ts, lib/knowledge/vector-search-backend-factory.ts, lib/llm/model-observability-blob-store.ts, lib/llm/model-observability-persistence.ts, lib/llm/model-observability-read-database.ts, lib/llm/model-observability-schema.ts, lib/llm/model-observability-testing.ts, lib/llm/usage-ledger.ts, lib/loop/loop-store.ts, lib/memory/cache-snapshot-observation.ts, lib/memory/compile.ts, lib/memory/compiled-memory-snapshot.ts, lib/memory/compiled-memory-state.ts, lib/memory/config-loader.ts, lib/memory/dream/revision-store.ts, lib/memory/dream/state-store.ts, lib/memory/fact-store.ts, lib/memory/memory-ticker.ts, lib/memory/navigation.ts, lib/memory/session-summary.ts, lib/memory/tenets.ts, lib/pi-sdk/search-tools.ts, lib/resource-io/providers/local-fs-provider.ts, lib/resource-io/providers/url-provider.ts, lib/sandbox/read-office-media.ts, lib/sandbox/script.ts, lib/sandbox/win32-exec.ts, lib/sandbox/win32-runtime-cache.ts, lib/session-files/bridge-inbound-files.ts, lib/session-files/browser-screenshot-file.ts, lib/session-files/session-file-registry.ts, lib/skill-bundles/package-service.ts, lib/skill-bundles/store.ts, lib/skills/skill-name-translation-cache.ts, lib/skills/skill-package-installer.ts, lib/skills/skill-removal.ts, lib/subagent-executor-metadata.ts, lib/subagent-run-store.ts, lib/subagent-thread-store.ts, lib/task-registry.ts, lib/terminal/terminal-session-manager.ts, lib/tools/dm-tool.ts, lib/tools/experience.ts, lib/tools/workflow-tool.ts, lib/user-profile-store.ts, lib/workflow-activity-store.ts, lib/workflow/journal.ts, lib/zip-writer.ts, plugins/beautify/lib/markdown-cover-service.ts, plugins/jimeng-cli/adapters/dreamina.ts, plugins/office/lib/html-to-pdf.ts, server/index.ts, server/routes/agents.ts, server/routes/avatar.ts, server/routes/character-cards.ts, server/routes/config.ts, server/routes/desk.ts, server/routes/providers.ts, server/routes/sessions.ts, server/routes/upload.ts, server/utils/uploaded-skill-package.ts, shared/artifact-core/activation.cjs, shared/artifact-core/ota-core.cjs, shared/artifact-core/pointer-store.cjs, shared/artifact-core/ustar.cjs, shared/data-epoch.cjs, shared/default-workspace.ts, shared/safe-fs.ts, shared/secret-fs.ts

（指纹 JSON 顶层键：`dataEpoch, exemptions, generatedBy, inventoryReceipt, payloadFingerprint, registry, review, schemas, siteMappings, sourceDigest, version`；当前 dataEpoch = 1。）

## 附录 B：复核限制与如实记录

1. SRC01 的 A03 修正**尚未提交**：本审计的「确认（HEAD）」指任务书声称的旧夹具问题在 HEAD 属实；「已修正」状态仅存在于工作区（modified 测试文件 + untracked 新测试），是否进入候选提交由后续授权流程决定。
2. SRC11/SRC13/SRC16 的规范文本（Node 24 官方文档、RFC 9110/9111、WHATWG Fetch）未联网比对，按任务指示仅核本地运行时与依赖内 API 形态；SRC16 的浏览器实际行为按任务书自身约定留待 F 阶段验证。
3. 所有行号为 HEAD（或对 SRC01 为工作区修正版）的复核时点坐标，函数名为权威锚点；后续 HEAD 前进后行号可能漂移。
4. 复核过程中未修改任何生产文件；唯一写入为本文件。
