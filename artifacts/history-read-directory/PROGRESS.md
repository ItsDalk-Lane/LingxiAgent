# PROGRESS — 历史分页读取与协议层全链路优化

任务书: `artifacts/history-read-directory/TASKBOOK.md`（2.0 版，A→F）
基线 HEAD: `1d42b7405c76292f617291e3a01cd2f3ef5efd04` @ `fix/pending-sep10`

## 阶段状态总览

| 阶段 | 状态 | 备注 |
|---|---|---|
| A | 进行中 | A01–A05 完成（A 阶段闸门条件已具备，见下方 A05 日志） |
| B | 进行中 | Batch 1–4 完成并验证（普通分页已启用目录缓存）；B08 验收：§7.1 工作量硬断言全过、热页/全翻阈值大幅达标，**冷页 10k 阈值未达标**（142.2ms > 123.8ms，已归因在案）——B08 未全部达标，如实保留未达标项 |
| C | 已完成（已修复并验证） | C01–C05 完成：可信追加增量（两规模 8/8 增量命中、parsedRecords 与规模无关）、受影响关系增量更新、开放 Run 无 O(Run) 证据；详见 C 完成日志 |
| D | 已完成（已修复并验证） | D01—D06 完成：定向 355/355、phase D 阈值 4/4、压力边界 5/5、typecheck 三配置、指纹 repin+guard+tripwire、全量失败签名与 HEAD 基线完全一致；D07 留待下一轮 |
| E | 未开始 | 已获授权（D 闸门成立后直接实施） |
| F | 未开始 | |

## 日志

### 2026-09-10 — A01 冻结启动状态（完成）
- 核对分支/HEAD：`fix/pending-sep10` @ `1d42b740…`，与任务书固定基线一致；工作区干净，无他人未提交修改需保护。
- 环境：Node v24.16.0（满足 >=24.12 <25）、npm 11.13.0、macOS arm64（Apple M3 Ultra，96 GiB，本地 APFS）。
- 已读根 AGENTS.md（系统注入）；`artifacts/history-read-directory/` 此前不存在，本次新建。
- 产出：`environment.json`、`initial-status.txt`、`protected-changes.json`、`acceptance-budget.json`、`TASKBOOK.md`、`PROGRESS.md`、`BLOCKED.md`。
- 下一步：A02 读取/依赖/写入边界三清单。

### 2026-09-10 — A03 修正长 Run 夹具并验证生产读取路径（完成）
- 根因确认：`writeLongRunSession` 中 `jsonlLine(parent, parent, toolResult)` 使 toolResult 复用 assistant id 且 parentId 自引用。取证：对该形状直接调用 `SessionManager.open().getBranch()` 抛 `RangeError: Invalid array length`（SDK 沿自引用 parentId 无限回溯），`loadSessionHistoryMessages` catch 后静默落入兼容 raw-read fallback——修正前 5/5 绿断言实际从未经过生产分支读取路径（旧 92s 耗时主要来自每请求该溢出回溯，修正后 ~2s）。
- 夹具修正：每条 toolResult 独立 id `r<i>`、parentId 指向对应 assistant，链 `u1→a1→r1→a2(parent=r1)→…→aN(parent=r<N-1>)`；规模定义固定 1 文件头 + 1 user + N assistant + (N-1) toolResult，行数 `2N+1`、display `N+1`；页大小 50 时 1k=21 页、10k=201 页（T12 固定）。SDK 文件头改为 UUID，业务 sessionId 由真实 `SessionManifestStore`（SQLite）按生产格式生成（`sess_…`），两套身份不同且各自合法（T01 断言）。
- 驱动器：硬编码 `maxPages=200` 改为 `ceil(displayCount/50)+2` 诊断余量；死循环保护保留并由 T12c 验证超限显式报错；10k 合法第 201 页不会误报失控。
- 分支入口证明：engine `openSessionManagerAtCurrentBranch` 包真实 SessionManager（既有真实测试 helper 形态，行为不变仅计数），T01/T12b 断言每请求走分支读取且零异常 → 正常路径 fallback 次数 0。
- 新增 `tests/history-pagination-invalid-fixture.test.ts`（8 测试，非法夹具不混入性能样本）：重复 ID/自引用/三环/缺父按现有生产严格层（`lib/session-jsonl.ts`）真实行为断言 `session_branch_duplicate_id`/`session_branch_cycle`/`session_branch_dangling_parent`，未改任何生产代码；SRC01 形状经真实路由为静默兼容降级（200 返回 fallback 投影，如实记录）；坏尾行（截断 JSON）单独构造：严格层 `session_branch_invalid_json`，生产读取先 oversized-repair 落盘修复（`.repair.json` 备份、剔除坏行）后走分支路径零异常、fallback 0；SDK `getBranch` 对自引用/环 RangeError 崩溃单独取证。
- 带/不带尾换行合法变体参数化覆盖（T12b，N=60 → 2 页，链形状与分支语义对两种文件尾一致）。
- 运行记录（均实际执行，exit code 为准）：修正前定向测试 5/5 绿（exit 0，92.41s，fallback 路径）；修正后 10/10 绿（exit 0，~2.0s，分支路径、fallback 0）；两文件合跑 18/18（exit 0，8.45s）；`npm run typecheck` 三份 tsconfig 通过（exit 0）。
- 未暴露真实生产语义缺陷：合法夹具走分支路径后，原语义断言（3 页/141 唯一记录/单逻辑 Run/零伪无回复/all=1 逐块等价/游标契约/幂等）全部保持成立。BLOCKED 无新增。
- 产出：`artifacts/history-read-directory/fixture-audit.json`（seed、构造方式、字节数与 sha256、各规模数量与页数、分支身份、执行路径计数、修正前后运行记录；`A03_FIXTURE_AUDIT_OUT` 环境变量可复跑再生成机器可验证部分）。

### 2026-09-10 — A04 建立可信参考结果（完成）
- 参考输出：`reference-outputs/` 下 7 份归一化响应（1k：first/middle/last/all=1；10k：first/middle/last）+ `reference-manifest.json`（每份 sha256Normalized、关键字段清单、防污染前后 sha256、仪表计数、内存采样）+ `reference-summary.md`。
- 端到端分页拼接断言（每次采集随跑）：1k 21 页/1001 条、10k 201 页/10001 条，页间重叠 0、与 all=1 逐条等价（`walkEvidence`）；手写小夹具精确期望仍由 A03 测试（T01/T04/T11/T11b/T03/T12 系列）与新增计数器自测 C8（夹具构建器与 fixture-audit.json 同 sha256）保留。
- 防污染：每请求从同一原始夹具字节复制独立文件（全新临时目录 + 全新空 SQLite manifest store + 全新 Hono app），记录前后 sha256；全部请求 `fileUnchanged=true`、无 `.repair.json`（合法夹具无坏行/超限行，repair 只读不重写——与 A03 结论一致）。
- 归一化：`reference-outputs/normalization-rules.md` **先于采集落盘**（脚本启动校验其存在），规则 R1 revision mtime、R2 临时路径、R3 UUID 防御（实测 0 命中）、R4 时钟断言（仅允许夹具固定 09:00:00/10:00:00）、R5 字节稳定序列化。
- 运行记录：`node scripts/collect-history-read-directory-reference.mjs --sizes 1000,10000 --seed 20260910 --out …` exit 0；7/7 请求 200、fullFileReadCalls=2/请求、jsonlParseCount 4004（1k）/40004（10k）。

### 2026-09-10 — A05 性能基线（完成，A 阶段闸门条件具备）
- 新增独立入口 `scripts/benchmark-history-read-directory.mjs`（真实 CLI：--phase/--sizes/--page-size/--seed/--output/--cold-runs/--walks；未知参数 exit 2；未实现 phase（D/F）exit 3；不使用 --passWithNoTests；SIGINT/SIGTERM 保留已完成样本并标 completed=false 退出 130）。配套 helper 在 `scripts/lib/`：计数器（history-read-counters.mjs）、模块重定向钩子（history-read-instrumentation.mjs + instr-*.mjs）、A03 字节级一致夹具/harness（history-read-fixture.mjs）。
- 仪表实现（覆盖 sync+async 真实读取与解析入口）：fs 包装（readFileSync/readFile/promises.readFile/openSync+readSync/closeSync/promises.open+FileHandle.read/createReadStream/写复制）、JSON.parse/stringify 包装、module.registerHooks 解析重定向（Pi SDK 与仓库具名 fs 导入 → 委托模块；三个全数组扫描/入口模块 → 计数包装）。**实测发现并解决两个坑**：①Node 内置模块 ESM 具名导出为启动快照，仅改 CJS 导出对象拦不住 SDK 的 `import { readSync } from "fs"`，必须解析期重定向；②请求仪表初值不得克隆全局累计桶（曾致模块装载读取计入请求）。两个问题都以显式报错/修复收场，未留静默降级。
- 计数器自测 `tests/history-read-directory-counters.test.ts`（8/8 绿）：整文件读两种形态（read-api、SDK 式 openSync+readSync 顺序 episode）、定位短读/EOF 分别计数、JSONL 行解析计数、请求作用域隔离、卸载恢复、包装模块 scanCalls、夹具 sha256 一致性。
- 基线数字（Apple M3 Ultra / Node v24.16.0 / APFS，Hono 内存进程内口径；OS page cache 未清空）：

  | 规模 | 冷首页 p50（3 次） | 热页 p50 / p95（样本） | 完整翻页 | fullFileReadCalls/请求 | jsonlParseCount/请求 | sessionFileReadBytes/请求 |
  |---|---|---|---|---|---|---|
  | 1k | 9.3ms | 7.7ms / 10.5ms（20） | 21 页，wall 177ms | 2 | 4004 | 890,874B |
  | 10k | 79.1ms | 75.5ms / 83.0ms（200） | 201 页，wall 15,341ms | 2 | 40004 | 9,044,880B |

  完整翻页唯一性：页间重叠 0、相对 all=1 缺页 0。冷≈热（旧路径每请求重复全文件读+全量投影，无目录可命中），10k 完整翻页 ≈ 15.3s 是 B 阶段对照基线。
- 口径说明（已写入 summary-a.json `timingModel`）：互斥分项 identityMs+readMs+parseMs+serializeMs 可加，serverTotalMs 独立实测，差值=residualMs（10k 热页 residual p50 ≈ 41.6ms，为 20k 条投影/预扫描主循环，符合 A02 预测）；jsonlParseMs ⊂ parseMs；fullHistoryProjectionCount=getBranch 调用数（1:1 代理口径）；metadataVisitedCount=5 个被包装全数组扫描入口条目访问数（下界）；directoryMs/hydrateMs/externalStateMs 阶段 A 恒 0（如实记录，非归因）。
- 运行记录：`node scripts/benchmark-history-read-directory.mjs --phase A --sizes 1000,10000 --page-size 50 --seed 20260910 --output artifacts/history-read-directory/baseline` exit 0，completed=true、failures=0；产出 `baseline/`（summary-a.json、baseline-summary.md、requests-a-n1000.jsonl、requests-a-n10000.jsonl）。
- 验证：`npm run typecheck` 三份配置 exit 0；`npx vitest run tests/history-read-directory-counters.test.ts tests/history-pagination-run-continuity.test.ts tests/history-pagination-invalid-fixture.test.ts` 26/26 绿。生产代码零改动（git 状态仅 scripts/tests/artifacts 新增与 A03 既有测试修改）。
- A 阶段闸门核对：合法夹具结构验证 ✓（A03）；正常生产路径执行证据 ✓（branchOpen=1/请求、fallback=0）；1k/10k 冷/热分项与完整翻页基线 ✓；参考输出已保存 ✓。**A 阶段完成条件具备，可进入阶段 B 生产改造。**

### 2026-09-10 — B01 Batch 1：同源抽取（完成，纯重构零行为变更）

按 `phase-b-plan.md` §1.6/§1.7/§1.9/§4/§9 Batch 1 执行；不建目录、不建缓存、不加扫描器（fs 语义零新增）。

- 改动文件清单：
  - 新建 `server/history-read/projection-context.ts`：`createProjectionFactScanner`（7 组预扫描合并为单 visit pass + finalize 两个补充 pass：todos 反扫、deferred-result 锚点前向）+ `buildProjectionContextFromMessages`（无缓存全量入口）+ `isDisplayableHistoryMessage`/`nextImmediateDisplayableAssistantIndex`/`parseHistoryDeferredResult`/`historyDeferredDeliveryId`/`isMediaGenerationDeferredResult` 移入导出；`recordFacts` 逐条记录 before 状态（displayIndexBefore / turnInputEntryIdBefore / turnInputVisibleBefore / assistantOrdinalBefore），作为 B03 目录事实与全量上下文的同源产出机制（I04）。`collectSessionCollabDecisions` / `collectModelCallReferencesBySourceIndex` / `extractLatestTodoSnapshot` / `annotateOriginMessages` 直接调用现有纯函数，判定规则零复制。
  - 新建 `server/history-read/project-page.ts`：`projectHistoryPage`（sessions.ts 主循环四分支整体迁出：afterIndex=displayIdx-1、origin→agentReview→presentation spread 次序、assistantOrdinal 先加后判、consumption 覆盖指针、loop 置 null、iterate/bounds/seed 参数化——全量 seed=零起点）；`deferHeavyHistoryBlock`/`soleRawToolResultText`/`createSanitizeVisibleContent`/`isBridgeSessionPath`/`taskFromSubagentRun`/`mergeSubagentTaskMetadata` 移入导出；record* 闭包随投影器创建并返回，主循环与后段回灌共享同一实例（blocks/去重集合不分裂）。
  - 新建 `server/history-read/hydrate.ts`：`hydrateExternalState`（sessions.ts:2014-2149 后段迁出：deferred 回灌、resolveMediaGenerationBlocks、块切片、subagent/workflow 终态回灌含 meta/summary 缓存（engine 参数化）、patchSessionFileLifecycleBlocks、listSessionRegistryFiles（registry 引用仍传原 sourceMessages）、todos——由 context.todoSnapshot 按 extractLatestTodos 输出契约派生，快照只扫一次）；`isTerminalDeferredTask`/`sessionFileLifecycleFields` 等随之移入。
  - 新建 `server/history-read/page.ts`：`resolveHistoryPageBounds` 原样移入，sessions.ts 顶部 re-export 保持 import 兼容。
  - 修改 `server/routes/sessions.ts`：messages 路由主循环区替换为 全量模式调用链（loadSessionHistoryMessages 原样 → buildProjectionContextFromMessages → projectHistoryPage(records=全数组恒等, seed=零起点) → hydrateExternalState）；rebroadcast/afterRun/reconciliation/c.json 原位保留；移除全部已迁出的模块级/闭包 helper（sessions.ts 净 -1083 行）。
  - 修改 `core/message-utils.ts`：仅给 `historyMessageFromEntry`/`projectBranchHistory` 加 export（diff 复核：函数体零改动）。
  - 新建 `scripts/diff-history-read-directory-phase-b.mjs`：复用 A04 harness（每请求独立夹具副本 + 全新 manifest store + 全新 app），对 7 份冻结参考按相同 R1–R5 归一化比对 sha256Normalized + 字段清单（messages 数、首末条 id/sourceIndex/role、blocks 数、todos、hasMore、nextBefore、sessionFiles 数、revisionSize、deferred 描述符数）；未知参数 exit 2、清单缺失 exit 3。
- 差分结果（`node scripts/diff-history-read-directory-phase-b.mjs --mode full --sizes 1000,10000`）：**7/7 sha256Normalized 逐字节一致**（n1000 first/middle(before=501)/last(before=50)/all + n10000 first/middle/last），字段清单零差异，每请求 `fileUnchanged=true`，exit 0。抽取前基线对照（改造前代码重放同 7 请求）先跑并存档：`phase-b/pre-batch1-baseline.json`（7/7 PASS）；改造后报告 `phase-b/post-batch1-diff.json`。
- 测试退出码（均实际执行）：`npx vitest run tests/history-pagination-run-continuity.test.ts tests/history-pagination-invalid-fixture.test.ts tests/history-run-outcome-edges.test.ts tests/sessions-route.test.ts tests/session-find-route.test.ts tests/history-read-directory-counters.test.ts desktop/src/react/__tests__/chat-semantics/turn-outcome-unification.test.ts` → 7 文件 151 测试全绿，exit 0；附加回归 `tests/session-route-errors.test.ts` + `tests/sessions-archived-route.test.ts`（同样 import sessions.ts）40 测试 exit 0；`npm run typecheck`（三配置）exit 0。
- 遇到的问题：无阻塞。实施过程中一次 Edit 误替换（主循环删除块的 new_string 写错）当即发现并整块修正，修正后经差分与全套测试验证；typecheck 报出的 3 个接线错误（project-page 漏 import isMediaGenerationDeferredResult、todoSnapshot 类型并集、c.json `messages` 需改 `projected.messages`）均已修复。
- 下一步：Batch 2（B02+B03 只读扫描与目录构建：scan.ts/directory.ts/types.ts），不动 route。

### 2026-09-10 — B02+B03 Batch 2：只读扫描与目录构建（完成，route 未动）

按 `phase-b-plan.md` §1.3/§1.4/§3/§7/§8 风险 2/5、TASKBOOK B02/B03 与 I05/I09/I11 执行；不动 server/routes/sessions.ts 与其他生产文件。

- 改动文件清单：
  - 新建 `server/history-read/types.ts`：§3 全套 interface（HistoryDirectory/Key/FileIndex/BranchIndex/RecordFact/AssociationIndex/SessionFacts、PhysicalLocation、HistoryReadContext、InvalidationReason 11 项枚举、HistoryScanResult/HistoryPhysicalEntry/HistoryReadHook）。红线自查：无正文/工具输出/base64/parsed block/完整 Map/sourceMessages/entries——只有定位与语义事实 + *RecordSourceIndex 指针。
  - 新建 `server/history-read/scan.ts`：`scanHistoryFile`（Buffer 偏移、256KiB 分块、行级字节累积不截断多字节 UTF-8、CRLF separatorLength=2、bytesRead 循环补齐、无尾换行完整末条→lastUndelimitedRow 纳入、截断 JSON/不完整 UTF-8 尾→pendingTailOffset 不 parse 不进条目、已完成位置 JSON.parse 失败→error.code="corrupt_record"（安全边界止于该行起点）、超限行内存套用现有 projectOversizedSessionEntry（hanaRepair 形状与 repair 产物逐字段一致，零写文件、零 .repair.json）、startOffset 续读、readFile hook 注入、所有分支关闭句柄）。不调用 repairOversizedSessionEntriesInFile。
  - 新建 `server/history-read/directory.ts`：`buildHistoryDirectory`——分支选择唯一实现 projectCurrentSessionBranchEntries（SessionBranchError→{directory:null, reason:"directory_invalid", detail:err.code}，拒绝码与严格层逐字一致；legacySyntheticIds→legacy_fallback；revision null→revision_unknown）；head 幂等写回镜像 readManifestSessionBranch 的 persistRecovery 规则（append_recovery/branch_read_observe_tail/branch_read_legacy_backfill），经 ctx.persistBranchHead 窄回调直写 manifest store（engine.setSessionBranchHead 需存活 SessionManager，不适用冷构建，Batch 4 接线）；projectBranchHistory + buildProjectionContextFromMessages 压缩为目录事实（records 补齐 toolCallIds/toolName/isError/runOrdinal/turnStart/turnEnd/timestamp；displayableSourceIndexes/blockAnchorByAfterIndex 升序；assoc 13 项关联指针表含 origin/presentation/review 记录指针与 todoSnapshot{sourceIndex}）；measure() 保守估算（string 字节×2、Map/数组条目常数、WeakSet 防环）>16MiB→budget_exceeded；released() 清空 scan.entries/sourceMessages/branchEntries/byId/context（I11）。
  - 微调 `server/history-read/projection-context.ts`（Batch 1 模块，仅增量）：HistoryRecordFact 类型改由 types.ts 定义并 re-export；visit 记录 origin/presentation/review 注释记录自身的 sourceIndex 指针表；finalize 增 todoSnapshotSourceIndex（从尾单记录探针调用 extractLatestTodoSnapshot，deep-equal 定位所选记录，不复制合法性规则）与 runOrdinalBySourceIndex 导出。Batch 1 全量路径行为零变化（差分回归证实）。
  - 新建 `tests/history-read-directory-scanner.test.ts`（14 测试）、`tests/history-read-directory-build.test.ts`（22 测试）。
- 关键语义判定：①「真实无尾换行末条」与「截断尾」以 JSON.parse 成败区分（I09：合法完整记录纳入 + 记录 lastUndelimitedRow；截断/不完整 UTF-8→pendingTailOffset 从其起点续读）；②corrupt_record 的 indexedThroughOffset 止于坏行起点（该行未安全处理）；③head 写回与旧路径（readManifestSessionBranch persistRecovery=true）终态 leafId/observedTailLeafId/reason 逐字段一致（updatedAt/sessionId 属 store 时钟/身份，两库不可比）；④activeFileReferenceIdentities 本批次显式置 null（registry 的 collectSessionFileReferenceIdentities 未导出，Batch 3 随 listReachable referenceIdentities 入参一并接线，热页接线前走原全量引用路径）。
- 测试数与退出码（均实际执行）：`npx vitest run tests/history-read-directory-scanner.test.ts tests/history-read-directory-build.test.ts` → 36/36 绿，exit 0（覆盖 X06 尾换行有/无×LF/CRLF×中文/emoji×随机 chunk 边界性质测试、X07 短读补齐/提前 EOF 不读空 Buffer、坏尾行→pendingTail 续读补齐、corrupt_record、超限行 hanaRepair 与 repair 产物 deep-equal 且文件字节不变；X01 六种拒绝码与严格层一致；X08/X09 五种 head 场景真实 SQLite 双库对拍终态一致；X13 todos 三态；I04 性质测试 build 上下文与 buildProjectionContextFromMessages 全字段 deep-equal；measure/预算）。回归：`npx vitest run tests/history-pagination-run-continuity.test.ts tests/history-pagination-invalid-fixture.test.ts tests/history-run-outcome-edges.test.ts tests/sessions-route.test.ts tests/session-find-route.test.ts` → 138/138 绿，exit 0；`node scripts/diff-history-read-directory-phase-b.mjs --mode full --sizes 1000,10000` → 7/7 sha256Normalized 逐字节一致，exit 0（Batch 1 基线不回归）；`npm run typecheck` 三配置 exit 0。
- 遇到的问题：无阻塞。首轮 10 个测试失败全部为测试侧预期错误（origin/review 注释记录须位于其注释的 user 之前、TODO_STATE custom_message 缺 removed:false 被「最后者胜」判为清空、跨库 head 行比较含 store 时钟字段），生产模块侧修复仅 1 处（corrupt_record 安全边界止于坏行起点）；均已修复并全绿。
- 下一步：Batch 3（B04+B05：page.ts resolveHistoryPage 扩展、window-reader.ts、cache.ts、createHistoryDeferredContentFor 适配、registry referenceIdentities 入参），不动 route。

### 2026-09-10 — B04+B05 Batch 3：定点读取与缓存（完成，route 未动）

按 `phase-b-plan.md` §1.5/§1.8/§1.10/§6/§7、TASKBOOK B03/B04/B05 与 I02/I08/I11 执行。

- 改动文件清单：
  - 扩展 `server/history-read/page.ts`：`resolveHistoryPage(directory, params)`——页边界语义与 resolveHistoryPageBounds 逐字同源、total 取目录 displayTotal；窗口记录 = displayableSourceIndexes 升序数组下标区间（displayIndex≡位置，O(K)）；块锚点按 afterIndex 二分（lowerBound，null 项以 MAX_SAFE_INTEGER 排尾天然排除）；dependencyLocations 汇总页外依赖（todos 指针、窗口命中的 deferred-result interlude 锚点记录、末页 media success 记录、窗口 user 命中的 origin/presentation/review 注释记录），去重按 byteOffset 排序；headState 直接取首条窗口记录 before 状态（不从根重放）；`locateToolResultRecords`/`locateMediaResultRecords` 供窗口读取后按 toolUse id/taskId 以 O(命中数) 补定位跨页依赖（§5 #3/#5）。
  - 新建 `server/history-read/window-reader.ts`：`readHistoryRecords`——打开句柄先 fstat 五元组校验（dev/ino/size/mtime/ctime 任一不符→file_identity_changed，X02/X03/X04）；位置排序去重、区域合并默认只含行分隔符级「紧邻」（mergeGapBytes 为分隔符外额外间隙，coalescingExtraBytes 如实报告）；bytesRead 循环补齐；异常 EOF 先重验身份再报 short_read（不读空 Buffer）；每条解析后 entryId 强校验 + type 校验（custom 角色不下发 raw type，custom/custom_message 两种 raw 均合法）；不符→directory_invalid fail-closed（X05）；超限行内存套用 projectOversizedSessionEntry（与 parseSessionLine 同规则）；区域并发受 maxConcurrent（默认 4）限制；所有分支关闭句柄；readFile hook 供测试注入。附 `collectLocations` 便捷封装。
  - 新建 `server/history-read/cache.ts`：`HistoryDirectoryCache`（实例私有）+ `historyDirectoryCacheKey`——8 槽 LRU（命中提升）、64MiB 驻留（含 retired 在途旧版本，I11）、16MiB 单目录准入（publish 拒收 + budget_exceeded→该会话 no-cache）、并发构建 2（全局信号量+per-key single-flight，同 key 并发 miss 共享同一 lease）、publish 版本校验（invalidate/begin 推进 generation；stale lease 拒绝且立即 released，X16）、probe 四条件（fstat 五元组/revision/head 三态语义字段/locator）任一不过即 invalidate(reason)、invalidate/dispose/release 调用 released() 并清 Map/在途表。
  - 修改 `server/history-deferred-content.ts`：新增 `createHistoryDeferredContentFor(record, sourceIndex, …)` 内部入口；原 `createHistoryDeferredContent` 取 `sourceMessages[sourceIndex]` 后委托——locator 编码/描述符格式/阈值/解析语义不变；`resolveHistoryDeferredContent` 未动。
  - 修改 `lib/session-files/session-file-registry.ts`：`listReachable` 增加可选第三参 `{ referenceIdentities }`（目录紧凑身份集合，传入时跳过深扫）；未传走原深扫路径，既有调用方（core/engine.ts:1581 等）签名与行为不变（registry 套件 27/27 回归）。
  - 新建 `tests/history-read-window-reader.test.ts`（11 测试）、`tests/history-read-directory-cache.test.ts`（14 测试）。
- 测试数与退出码（均实际执行）：四个 history-read 文件合跑 61/61 绿 exit 0（window-reader：定位读取/短读补齐/EOF 先重验身份/截断/同长度重写/同 size+mtime 原子替换/entryId-type fail-closed/合并额外字节/并发峰值≤上限；cache：single-flight 共享 lease、stale publish 拒绝+租约不可变、failBuild 移除在途、probe 四条件逐项、LRU 淘汰与命中提升、字节预算、retired 计入驻留与 release 归还、dispose 全释放、信号量受控调度第三构建等待放行）。回归：P08 五套件 138/138 绿 exit 0；`session-file-registry.test.ts` 27/27 exit 0；Batch 1 差分 7/7 sha256Normalized 逐字节一致 exit 0（deferred-content 委托与 registry 改动行为中性）；`npm run typecheck` 三配置 exit 0。
- 关键语义判定：①JSONL 行分隔符（\n/\r\n）属文件结构而非「间隙」——mergeGapBytes=0 时紧邻记录（间隔≤分隔符）合并，超出部分才计入额外字节并报告；②custom 角色记录的 raw type 有 custom/custom_message 两种，窗口读取的 type 校验只对已知 raw type（message）启用，entryId 恒强校验；③invalidate 会连带 released retired 旧版本（版本推进后旧目录不得存活）；④engine.setSessionBranchHead 需存活 SessionManager，不作为目录恢复写回原语（Batch 2 已定，persistBranchHead 窄回调由 Batch 4 接 manifest store）。
- 遇到的问题：无阻塞。首轮 8 个测试失败中 2 个为生产侧缺陷（stats().sessions 误返回字节数、budget_exceeded 对缺失槽位不生效致 no-cache 失效），其余为测试预期偏差（locatorPath 绑定缺失、retired spy 挂错版本、窗口 sourceIndex 与 raw 下标差一个头部、短读计数基准），均已修复并全绿。
- 下一步：Batch 4（B06+B07：read-context.ts、index.ts 编排入口、route 接线与快照/回退、日志/reason 枚举、rebroadcast 恰一次——生产行为变更点，落地前以 Batch 1 差分基线回归护航）。

### 2026-09-10 — B06+B07 Batch 4：路由接线与快照/回退（完成，生产主路径启用目录缓存）

按 `phase-b-plan.md` §1.2/§1.11/§4/§6/§7/§8 风险 1/3/4、TASKBOOK B06/B07 与 I01/I03/I04/I06/I07/I08/I12 执行。**普通 `/api/sessions/messages` 分页生产主路径自此走目录快路径**；all=1/reconciliation/find/content 语义未触碰。

- 改动文件清单：
  - 新建 `server/history-read/read-context.ts`：`captureReadContext`——stat 先于读内容（I07/I08，失败→null 不读不写缓存）；同一 stat 取 dev/ino/size/mtimeMs/ctimeMs 内部身份（缺字段记录 identityFields）；分支头三态（引擎无 head 能力→supported:false，目录与 probe 双侧一致跳过，不伪造）；locator 绑定；persistBranchHead 窄回调（探测 engine._sessionManifestStore，无则不写回——恢复职责退回旧打开边界，不伪造）。不打开 SessionManager、不全读 JSONL。
  - 新建 `server/history-read/index.ts`：`readSessionHistoryPage` 编排 + `projectFullHistoryPage`（全量模式，reconciliation/legacy 共用）+ `materializeProjectionContext`（目录事实+稀疏记录→ProjectionContext 同形状：注释数据来自被读取注释记录、Run/ordinal 从记录级事实直取、entryId→sourceIndex 经 byEntryId 物理下标换算、deferred 锚点 Map 按目录生命周期 memoize）；热页链 = probe→resolveHistoryPage→readHistoryRecords→逐记录投影（historyMessageFromEntry→correlation 合并→projectSessionMessageForDisplay，与 projectBranchHistory 同链）→projectHistoryPage（稀疏视图+页首 seed）→hydrateExternalState→读后复核（stat+head+locator 一致才发布，I06/I07）；失败链：尝试 1→invalidate+重建→尝试 2→legacy 全量（记 fallbackReason）→error 交回 500（P11）；disableCache/readFile 为 DI 参数；日志仅内部标识/reason/版本摘要/计数。
  - 修改 `server/routes/sessions.ts`（§4 接线）：身份解析/授权原样先行（I01）；revision 预取原样保留（响应 revision 字节不变）；reconciling 完全不进目录（走 projectFullHistoryPage，I12）；普通分页走 readSessionHistoryPage；`historyDirectoryCache` 实例在 createSessionsRoute 内创建（route 私有；engine.historyReadCache 可注入观测）；rebroadcast 移到目录重试成功后恰一次；响应键序保持 messages/blocks/todos/hasMore/nextBefore/sessionFiles/revision。
  - 微调 `directory.ts`（构建期元数据）：assistant 记录存 toolUse ids（extractTextContent 一次解析，热页 §5 #5 跨页结局闭包）；assoc.consumptionByAssistantEntryId 反向索引（consumption interlude 页外记录定位，锚点数值仍由投影器现算）；session.activeFileReferenceIdentities 接线（registry 收集器已导出，§5 #2 落地）。
  - 微调 `page.ts`：resolveHistoryPage 依赖闭包扩展——窗口 span 记录（displayIndexBefore∈[start,end]，含不可见 assistant 与 display:false custom，二分 O(log N)，修复热页 ordinal 漂移真缺陷）+ 窗口 assistant 的 toolCallIds/consumption 锚点依赖 + media 结果记录保守全纳；输出 iterateRecordIndexes（热页 iterate）。
  - 微调 `project-page.ts`：PageProjectorInput.sourceMessages → records 视图（identityRecordView/Map 视图，I02 不重编号）；deferred 凭证走 createHistoryDeferredContentFor（locator 不变）；deferred-result afterIndex 改用 context.deferredInterludeAnchors（同一 finalize 判定，冷热同路径）；hydrate.ts 输入改 activeReferences + sessionFileReferenceIdentities（热页经 registry.listReachable referenceIdentities 出 sessionFiles，§5 #2）。
  - 修改 `lib/session-files/session-file-registry.ts`：仅给 collectSessionFileReferenceIdentities 加 export（行为零变化）。
  - 修改 `tests/sessions-route.test.ts`：message-utils mock 工厂改为 importOriginal 展开底座（新管线消费 projectBranchHistory/historyMessageFromEntry 等真实实现）；`tests/history-pagination-run-continuity.test.ts`：分支入口证明断言更新为 B06 契约（分页请求零 SessionManager 打开，0 即零回退证明；A03 审计 executionPath 描述同步）。
  - 新建 5 个测试文件（fallback 6 / semantics 9 / cache-auth 3 / shared-functions 3 / memory 3）。
- 差分三模式（`node scripts/diff-history-read-directory-phase-b.mjs --mode full --mode cold --mode hot --sizes 1000,10000`）：**21/21（7 参考 × 3 模式）sha256Normalized 逐字节一致**，exit 0；hot 模式同环境两遍（冷构建+热命中）均与冻结参考全等；10k 目录经估算校准（属性名共享不再逐次计）后 <16MiB，10k 热命中为真目录路径（此前 budget_exceeded→legacy 的日志已消失）；报告存档 `phase-b/post-batch4-diff.json`。
- 测试退出码（均实际执行）：Batch 4 五文件 24/24 绿 exit 0；P08 六套件 143/143 绿 exit 0；registry 套件 27/27 exit 0；`npm run typecheck` 三配置 exit 0。
- 关键语义判定：①热页读取原始条目必须经与 projectBranchHistory 相同的逐记录投影链（首轮集成测试抓到「messages 为空」即此缺失）；②窗口 span 内不可见 assistant/ display:false custom 必须进热页迭代闭包（不变量测试抓到 assistantSegments ordinal 漂移真缺陷，二分修复）；③manifest store 恢复写回经 persistBranchHead 窄回调（readManifestSessionBranch 语义），engine.setSessionBranchHead 因需存活 SessionManager 不采用；④registry 收集器 export 属必要最小改动（热页 sessionFiles 正确性依赖，超出本批允许清单 1 行，特此记录）。
- 遇到的问题：无阻塞。集成调试抓到并修复 2 个真缺陷（热页缺投影链→空消息、span 记录缺位→ordinal 漂移）与 1 个估算口径偏差（16MiB 预算误拒 10k 目录）；测试侧修正 6 处预期/夹具错误。
- 下一步：Batch 5（B08 阶段验收：benchmark --phase B、phase-b 性能数据、7.1/7.2/7.4 断言核对、PROGRESS/acceptance 记录）。

### 2026-09-10 — B08 Batch 5：阶段 B 验收（完成；冷页 10k 阈值未达标，如实记录）

执行：`node scripts/benchmark-history-read-directory.mjs --phase B --sizes 1000,10000 --page-size 50 --seed 20260910 --output artifacts/history-read-directory/phase-b` exit 0（completed=true、failures=0）；`npx vitest run tests/history-read-directory-memory.test.ts` 3/3 绿 exit 0。脚本扩展：IMPLEMENTED_PHASES 增加 B；每环境注入独立可观测 cache 实例；按 cache stats 增量分类 cold-build / hot-hit / legacy-fallback；§7.1 硬断言与 §7.2 阈值判定内建（baseline/summary-a.json 对照）。环境：Apple M3 Ultra / 28 CPU / Node v24.16.0 / APFS；**OS page cache 未清空（状态未知，如实注明）**；大基准独占运行，未与其他高负载测试并行。

- 工作量硬断言（§7.1，全部通过）：热命中页（1k 20 样本 / 10k 200 样本，覆盖首/中/末）fullFileReadCalls=0、fullHistoryProjectionCount=0、零回退（无分支入口调用）；jsonlParseCount p50=103（1k 与 10k 完全相同，≈2×页大小+依赖，O(K) 证实）；metadataVisitedCount p50=103（1k/10k 同量级，O(log N+K+|Dpage|) 证实——物化上下文/锚点/结局均为 O(命中数) 定位，无全目录 filter）；窗口逻辑读量 = 记录行字节（span 103 条）+ 校验字节，无合并间隙（coalescingExtraBytes=0）。
- 冷构建（每规模 3 次全新缓存实例）：1k cold jsonlParse=2101（整文件一次扫描+解析，含全部记录边界）；10k cold jsonlParse=20101；冷构建后 cache 驻留 residentBytes：1k≈1.63MB / 10k≈16.44MB（<16MiB 准入线，接近上限如实注明；更大会话将按 §6 no-cache 降级）。
- §7.2 延迟阈值逐项判定（对照 A 基线 baseline/summary-a.json）：

  | 阈值 | A 基线 | B 实测 | 限值 | 判定 |
  |---|---|---|---|---|
  | 冷页 p50 1k ≤ 1.25×9.27+25ms | 9.27ms | 19.38ms | 36.58ms | **通过** |
  | 冷页 p50 10k ≤ 1.25×79.08+25ms | 79.08ms | 141.49ms（两轮 142.2/141.5，复现稳定） | 123.84ms | **未达标** |
  | 热页 p50 10k ≤ 3×max(0.71ms, 5ms) | 75.5ms | 0.65ms | 15ms | **通过**（较 A 热页 75.5ms 提升约 116×） |
  | 全翻 total_10k/total_1k ≤ 15 | A: 15,341ms/177ms | 274.09ms / 37.3ms（ratio 7.3） | 505.41ms | **通过**（10k 全翻 201 页 300ms wall，较 A 15,341ms 提升约 51×） |

  冷页 10k 未达标归因：B 冷构建在 A 的读取+投影之外新增目录构建（scan 4.5MB 逐行字节切分 + projectCurrentSessionBranchEntries 校验/lineage hash + 记录/关联压缩 + measure 驻留估算），10k 上 residual 44.4→150.5ms（+106ms），parse 反而 29.0→15.4ms（单次扫描 vs A 的 repair+SDK 双读）。属一次性构建成本（按会话版本摊销），非缺陷：热页/全翻收益（53–130×）远超冷页支出；阈值本身冻结不得放宽，如实保留未达标。1k 冷页达标（19.4ms，构建开销 ≈10ms）。
- 完整翻页唯一性：1k 21 页/10k 201 页，页间 overlap=0、相对 all=1 missing=0、记录数=display（Walk 校验内建）。
- 内存（§7.4/P09）：`tests/history-read-directory-memory.test.ts` 3/3 绿（唯一大载荷夹具可达性扫描：目录可达对象图零 PAYLOAD 标记字符串、零 Buffer/TypedArray；released() 后 transient 清空；8 槽共存 + 轮换淘汰 + released 触发）。benchmark 每请求内存采样见 requests-b-n*.jsonl；目录驻留估算 vs heap 对照口径见 memory 测试与 cache stats（估算 ≪ 实际 heap 增量，证明无正文驻留）。
- 产出：`phase-b/summary-b.json`、`phase-b/phase-b-summary.md`、`phase-b/requests-b-n1000.jsonl`、`phase-b/requests-b-n10000.jsonl`、`phase-b/post-batch4-diff.json`（三模式差分 21/21）。
- 结论（四态口径）：阶段 B 的 B01–B07 生产接线与 §7.1 工作量、§7.2 热页/全翻阈值、§7.4 内存 = **已修复并验证**；§7.2 冷页 10k 阈值 = **未达标（已测量、已归因，阈值冻结不放宽）**——是否投入构建成本优化属后续决策，不阻塞 C 阶段增量工作的独立推进（C 的增量扫描与本项冷构建成本正交）。
- 下一步：阶段 C（可信追加与受影响关系增量更新）或按用户指示处理冷页阈值缺口。

### 状态分类约定（D05）
已修复并验证 / 已实现但指定环境未验证 / 待验证或受阻 / 经证据否定

### 2026-09-10 — B 冷页优化修复（中断后由主线完成）
- 背景：B08 初测 10k 冷首页 p50 141.5ms > 阈值 123.8ms；优化子代理被 429 中断，遗留 lib/session-jsonl.ts 的 lineageHash:false 选项与半成品改动，且 2 个 I04 性质测试失败。
- 根因：directory.ts 在 build 返回前就地改写 context.recordFacts（补 toolCallIds/runOrdinal/turnStartIndex/turnEndIndex/timestamp/toolName/isError），导致 build 产出的 ProjectionContext 与 buildProjectionContextFromMessages 不再全字段等价。
- 修复（主线执行）：补齐逻辑上移至 projection-context.ts finalize()，两条路径共用同一事实形状（I04）；directory.ts 删除就地改写循环与 isToolCallBlock import；保留 lineageHash:false（目录只消费 lineage id 列表，默认调用方行为不变）。
- 复测：I04 性质测试 22/22；history-read 十文件 + P08 套件合计 231/231 exit 0；差分 full/cold/hot 21/21 逐字节一致；typecheck 三配置 exit 0。
- 最终 phase B 数字：1k 冷 9.8ms / 热 0.7ms / 全翻 21 页 30ms；10k 冷 73.0ms / 热 0.6ms / 全翻 201 页 209ms。§7.2 四条阈值全部 PASS（10k 冷 73.0 ≤ 123.84；热 0.56 ≤ 15；全翻比 185.94 ≤ 400.43；1k 冷 9.82 ≤ 36.58）。§7.1 硬条件全过（热页 fullFileReadCalls=0、fullHistoryProjectionCount=0、fallback=0、jsonlParseCount=103 与规模无关）。
- profile 证据留存于 logs/prof-cold.mjs、logs/prof-out/。
- **阶段 B 状态：已修复并验证**（B01—B08 全部完成，生产主路径已启用目录缓存）。
- 下一步：阶段 C（可信追加与受影响关系增量更新）。


### 2026-09-10 — 阶段 C 启动：C01 变化分类 + C02 变更世代（主线实施）
- 新增 `core/session-file-mutation-epoch.ts`：进程内 per-path 单调 epoch 注册表（仅缓存提示，非持久化事实源）。插桩（写前递增、失败不回退）：`writeSessionEntriesFile`、`flushSessionManagerSnapshot`（_rewriteFile 前）、`pruneSessionManagerEntries`（就地改写+_rewriteFile 前）、sessions.ts 归档/恢复/永久删除 rename（源+目标）。SDK appendFileSync 追加路径不递增。迁移重写仅影响 v1/v2 文件（永无 v3 目录），设计覆盖。
- `cache.probe` 扩展为 C01 判定序：`valid` / `append_candidate`（身份不变+size 增长+epoch 未变，目录保留）/ `branch_view_stale`（文件不变 head 变，目录保留）/ 失效 reason（缩短→snapshot_changed、同长度 mtime 变→untrusted_mutation、ino/dev 变→file_identity_changed、locator→locator_changed、epoch 已变的变长→untrusted_mutation）。
- `index.ts`：发布前记录 `mutationEpochAtBuild`；append_candidate/branch_view_stale 在 C03 增量接线前按 B 行为显式失效+全量重建（安全回退，语义不变）。
- 测试更新（C01 决策表逐项）：X02 同长度重写→untrusted_mutation；X03 拆分为纯追加→append_candidate（目录保留）与带通知变长重写→untrusted_mutation；head 变化→branch_view_stale（目录保留）。
- 验证：history-read 十文件 + P08 套件 231/231 exit 0；差分三模式 21/21 逐字节一致；typecheck 三配置 exit 0。
- 设计文档：phase-c-plan.md（C03 增量版本隔离：分段 records、base+delta Map overlay、Run 边界迁移到按 Run 小表）。
- 下一步：C03 增量扫描与尾记录合并、C04 受影响关系更新、C05 增量性能验证（等待子代理配额恢复后实施）。

### 2026-09-10 — 阶段 C 完成：C03 增量扫描 + C04 受影响关系 + C05 增量性能验证（子代理实施，主线复核现场后收尾）

**实现主体**（`server/history-read/incremental.ts` 新建，index/types/cache/projection-context/directory 接线）：

- C01 决策表逐行落地：`append_candidate`→`buildHistoryDirectoryIncremental`（beginBuild 租约+publish；失败→invalidate→全量重建；log.info/warn 逐路径记录）；`branch_view_stale`→`rebuildBranchView`（仅支持 rewind 到已构建 lineage 内节点，其余 fail 全量重建）；`expectedHeadAfter = built.headWriteBack`（增量不写回 head，post-read 复核）。cache 新增 noteIncrementalUpdate/NoteIncrementalFailure/noteBranchViewRebuild/lastPublished 与三个命中率计数。
- C03 续读起点 `pendingTailOffset ?? lastUndelimitedRow.offset ?? indexedThroughOffset`（lastUndelimitedRow 重读核对 id/字节长度，不盲续）；delta 结构校验（duplicate/dangling/cycle→放弃增量走严格层全量）；head 规则复用判定（parentOf 对新条目走 newParent、旧条目走 lineage 索引前驱；混合形态→fail snapshot_changed）；版本隔离 I06：records/displayable/blockAnchor/lineage/deferred/media/consumption 走 SegmentedList overlayAppend（base 段跨版本共享），byEntryId 等 13 张关联 Map 走 MapOverlay（base 引用 + 有界 delta），Run 边界迁到 `runBoundsByOrdinal` 按 Run 小表（copy-on-write 单 Run 对象，开放 Run 逐条追加 O(1)），全量/增量/materializeProjectionContext 同形状（I04）。
- C04 受影响关系仅处理新增条目：toolResult/turnInput/consumption/collab/modelCall/origin/presentation/agentReview/correlation（跨追加 ambiguous）/todo 指针（向后扫）/文件引用集合并入；affectedRelations 计数入 metrics。
- 增量 measuredBytes = 旧估算 + 8192 + measure(delta)（overlay 只计 delta，O(新增)）；全量统一走 measureDirectoryBytes。

**本轮三个根因修复**（先取证后修，均有实证）：

1. C01 遗留回归（16 套件中唯一失败测试）：`directory.ts` lineageEntryIds 传入 flat 数组被 SegmentedList 当多 segment（length=各 id 字符串长度之和，n=30 夹具 161≠60）→ 重新包一层 `[...]`。该缺陷曾被修复，工作区复核时丢失（文件未跟踪、无 git 保护），已恢复并全绿。
2. 10k 增量退化（append-10/100/big-tool-result 全量重建）：诊断脚本实证 10k 基线 measuredBytes=16,766,472（15.99MiB），距 16MiB 准入线仅 10,744B——append-1 后 16,775,634 惊险过线；append-10 增量构建成功但 publish 被 `measuredBytes > maxSingleDirectoryBytes` 静默拒绝（index.ts 该分支无日志无计数）→ 落全量重建；append-100 全量也超预算 → budget_exceeded → noCache 锁存。**合规修复（不动 16MiB 验收预算）**：`HistoryFileIndex.byEntryId` 值由逐条 `{physicalIndex,byteOffset,byteLength}` 对象（20k×~112B 估算驻留）改为 `Map<string,number>` + `byteOffsets/byteLengths` SegmentedList（base 段共享、追加段 O(新增)），净省 ~1.76MB；10k 基线降至 15,007,096（14.31MiB），余量 1.69MiB > C05 四批次累计增量成本 ~95KB。附带删除两处死代码 estimateDirectoryBytes（估算口径单一化）。
3. C03 off-by-one 真缺陷（新回归测试锁定）：tail 重读条目已在旧物理索引内，但新条目物理下标公式多加 `+ tailRereadCount` → lastUndelimitedRow 场景下新条 sourceIndex 错位 1（materializeProjectionContext 指针换算取错事实）。修正为 `oldPhysicalCount + i`，新增 P04 等价回归测试（物理下标连续 + 热页与全量 JSON 全等；对旧代码该测试红）。

**C05 数字（真实路由，热目录追加，seed 20260910）**：

| 规模 | 批次 | appendedBytes | parsedRecords | 增量命中 | updateServerTotalMs |
|---|---|---|---|---|---|
| 1k | append-1 | 142 | 101 | ✓ | 2.51ms |
| 1k | append-10 | 1,385 | 99 | ✓ | 1.78ms |
| 1k | append-100 | 14,135 | 150 | ✓ | 3.41ms |
| 1k | big-tool-result | 204,929 | 51 | ✓ | 2.87ms |
| 10k | append-1 | 143 | 101 | ✓ | 1.62ms |
| 10k | append-10 | 1,385 | 99 | ✓ | 2.79ms |
| 10k | append-100 | 14,135 | 150 | ✓ | 18.75ms |
| 10k | big-tool-result | 204,929 | 51 | ✓ | 3.32ms |

- **两规模 parsedRecords 完全一致**（101/99/150/51）——新增工作量由增量决定、与历史规模无关，C05 核心验收达成；命中率 8/8，失败 0，无一次降级重建。
- 开放 Run（500 条）逐条追加 20 次：离散度 max/min = 1.43×(1k)/1.37×(10k)，判定线 ≤8×，无 O(Run) 批量改写证据。
- 10k 基线 14.31MiB 距 16MiB 准入线余量 1.69MiB；更大或持续追加会话超线时按 B04 no-cache 只读降级（设计内，不截断历史）。
- summary-c.md 生成器 phase A 模板残留已修：task/timingModel/工作量表头/口径备注按 phase 区分；cacheStateNote/appendUpdateCount 数据层扩展到 phase C（原恒 no_directory/0）；md 增补 C05 数字表。

**已知边界（如实记录）**：retired 旧版本由 RETIRED_PER_SLOT_LIMIT=2 挤出/invalidate 释放，`cache.release()`（在途请求完成归还）全仓无调用方——追加中会话瞬时驻留最高 3×目录估算（有界、计入 residentBytes，I11 口径），属 B04 既有行为，不在本任务改动。信任边界（C02）：承诺范围=单进程写入+mutation-epoch 插桩的全部重写路径+dev/ino 捕获的替换；不承诺=外部进程 in-place 改写未读前缀（无权威通知，与纯追加不可区分）。

**验证退出码**：16 套件（15 文件 231 测试，含新增回归）231/231 exit 0；差分三模式 21/21 逐字节一致 exit 0；typecheck 三配置 exit 0；phase C 基准全部校验通过 exit 0（产物 `phase-c/summary-c.{json,md}`）。

**阶段 C 状态：已修复并验证**（C01—C05 完成；P12 生产快路径证据=真实 fs 追加+生产等价分支同步+真实路由读取，非测试桩）。

### 2026-09-10 — 阶段 D 完成：D01—D06 回归、门禁与检查点（D07 留待下一轮）

- **D01 定向与复测**：三段顺序（新增单元 73 → P/X 集成 147 → 前置语义集+desktop turn-outcome 135）全绿 exit 0；`--phase D` 基准（benchmark 补 D 支持：DIRECTORY_PHASES 门 + D 压力 scenarios 段）：1k 冷 11.2/热 0.98/全翻 21 页 36.8ms，10k 冷 93.2/热 0.67/全翻 201 页 254.7ms，§7.2 四阈值全 PASS（vs A 基线）；压力边界 5/5（big-tool-result/many-customs/multi-run/many-discarded/large-output：热页解析 50/51/21、零整文件读、零回退、目录驻留 0.06–0.22MiB）。首跑 scenarios 因 `fixtureBytesByN[0]` 抛错（fatal 如实记录于 summary），修 measuredRequest 增 meta 覆盖后成功。
- **D02**：typecheck 三配置串联一次执行 exit 0（24s）；指纹检查脚本 repin 前红/repin 后绿（见 D03）；全量对照=APFS clone（cp -Rc → /tmp/lingxi-baseline-1d42b740，restore 至 HEAD+删除 19 项任务未跟踪清单，porcelain=0）跑基线全量，再跑当前全量：基线 13544 passed/7 failed；当前第 1 轮 11 failed（+4=cli-closure-census×2、open-boundary-lint×2，已提交闭包/边界基线不含任务新源，非行为缺陷）；按仓库 repin 约定处置（writeCliRuntimeClosure/writeOpenBoundaryBaseline 在位再生成 + export-manifest.json 登记 13 个新模块，先例 server/history-deferred-content.ts）后 38/38 绿；第 2 轮全量 **7 failed 与基线签名逐条一致（diff=空）**，任务引入失败 0。7 项均为 HEAD 既有：round2×2、round3×1、packaged-desktop-cleanup 90s 超时、audit-seal、ObservabilityDateLine×2（历史线索全部核实）。附带发现：每次全量 vitest 由 round2 证据测试在位再生成 f1-f12-repair round2 补丁文件（纯净 HEAD 副本同样出现 +158/-1），已 git restore 不计入任务 diff。
- **D03 持久化指纹**：guard 触及 3 受护源（session-jsonl-file/registry/sessions.ts），逐文件论证=非持久化读取/组织变化（写前世代通知字节不变；可选入参+字面量快路径语义等价；投影主循环同源迁移）；`--classification compatible` repin（理由与实际 diff 相符）→ guard exit 0 + tripwire 15/15；指纹 diff 仅 payloadFingerprint/review/3 个 sourceHash。无 breaking，未触碰 DATA_EPOCH。
- **D04 交付审查**：tracked 改动 12 文件 + 未跟踪 34 文件全部归属本任务（A01 快照干净，无他人改动被回退）；无第二套 projector、无正文缓存、无每页隐式 open（热页 branchOpen=0/getBranch=0 硬断言）、无 SDK patch（pi-sdk 零 diff）、无提前 E；日志无正文/敏感输出（[dbg] 受 env 门控）；未迁移清单（all=1/find 边界复用/reconciliation/content）与保留原因见 server-stage-report.md §六。
- **D05 状态判定**：P01—P12 + X01—X20 共 32 项全部「已修复并验证」（范围注记：P05 前端联动留待 E/F、P09 retired 未接线为 HEAD 既有有界边界）；见 acceptance-matrix.json 与 acceptance-report.md。
- **D06 检查点**：server-stage-report.md（A—D 摘要、差分证据、P/X、A/B/C/D 性能对照表：10k 热页 75.5→0.67ms −99.1%、全翻 15,341→254.7ms −98.3%）；patches/a-to-d-server-stage.patch（相对 A01 快照，47 文件差异，纯净 HEAD 副本 `git apply --check` 通过）；commands.jsonl 记录本阶段全部命令与退出码。未创建 git 提交/分支。
- **阶段 D 状态：已修复并验证**（D01—D06；D07 协议基线下一轮单独做；不宣称 A—F 全部完成）。

### 2026-09-10 — 阶段 D07 完成：真实传输基线与协议技术闸门（不实现 E 功能）

- 新建 `scripts/benchmark-history-protocol.mjs`：真实 HTTP loopback（@hono/node-server 挂 D01 同一 Hono app，counters 服务端 span 复用）+ 本地 TCP 代理确定性整形（rtt=每请求双向首字节各 rtt/2 合计一次往返、每请求新建连接；带宽=每方向 64KiB 桶 token bucket）；未知参数 exit 2、E/F 预留 exit 2、校验失败 exit 1。三口径分列（Hono 内存 span / HTTP loopback / 受控链路），不代表公网质量。
- 调试修复 2 处（均有实证）：代理对源 socket `close` 立即 `client.end()` 截断整形队列中的响应（客户端 hang up）→ 改为 srcClose 标记排空后优雅 end；首字节延迟起算点错误（连接建立时而非首块数据到达）+ `this.firstDelayMs` 未赋值（NaN 恒不等待）→ 延迟塌缩 50ms 变 25ms/完全不生效，修正后 rtt50 首屏 52.8ms、rtt150 首屏 158.7ms（≈150ms+服务端），模型与实现一致。
- 全矩阵（2 规模 × 7 链路格 × 4 行为）exit 0、failures=0、1582 请求全部 200：**a 首屏** 1 请求（10k rtt150/bw10：158.7ms、31.2KiB）；**b 同页重复校验（现状=重新完整 GET）** 1 请求、字节与首屏全同（156.0ms/31.2KiB，E1 收益点）；**c 完整翻页** 1k=21 页/10k=201 页逐格断言（10k rtt150/bw10：31.7s、6.06MiB）；**d 总量/任务分布=现状无端点**，requests=0 如实记录不虚构强制全翻。客户端单页 Node 可复现成本：JSON 解析 ≈0.09ms、状态应用骨架 ≈0.01ms；buildItems 完整链/React 提交渲染不可 Node 复现（逐请求 gaps 注明）。服务端冷构建 span 1k 21.2ms / 10k 121.3ms（含目录构建）。
- 产出：`protocol/baseline-d/`（requests-d.jsonl + summary-d.{json,md} 含口径说明：rtt 分配/桶模型/可测范围/三口径不混算）；`protocol-gate-check.md`：G1 成立（P/X 32/32、定向无新增失败、全量签名与 HEAD 基线一致）、G2 成立（四行为数据齐全、HTTP 真实请求、客户端复现边界注明）、G3 成立（响应依赖 messages/blocks/todos/sessionFiles/hasMore/nextBefore/revision+deferred 凭证、重载 all/reconciliation/find 排除、客户端失效入口 8 项清单、权限边界保持）；授权状态=已获得，本任务内继续。
- 未实现任何 E 功能（无 ETag/概览/页大小头/协议头）；大基准独占运行。
- **D07 状态：已修复并验证**（基线与闸门文档交付；E01—E08 待 G1/G2/G3 成立后按 TASKBOOK 顺序实施）。

### 2026-09-10 — 阶段 E 批次 E-a 完成：E01 合同冻结 + E02 服务端条件 GET + E02.4 验证（E03/E04/E06 未实施）

- **E01 合同冻结**（`artifacts/history-read-directory/protocol/`）：`protocol-contract.md`（E01 最小合同 9 行全量 + 13 项状态转换确定行为——含同长度重写恰等表示允许 304、鉴权失效不泄露标签、重启换盐失配、旧服务器回退等非理想路径）；`protocol-dependency-matrix.md`（响应字段/协议头/八步流程/作用域输入 ↔ 产生消费代码一一映射，复用 resolveHistoryPageBounds/readSessionFileRevision/B04 cache 等既有接口记录在案）；`protocol-acceptance-budget.json`（E06.2 预算冻结：语义失败 0、首屏 p95 ≤1.20×D+30ms、客户端单页 ≤max(1.5×对照,75ms)、内存 ≤max(2×对照,16MiB)、概览热 JSON ≤4KiB 且 0 JSONL 读、校验元数据 ≤256KiB、K>50 全翻请求数必须更低；含 D07 基线锚点 protocol/baseline-d/）。
- **E02 服务端条件 GET**：新模块 `server/history-read/protocol.ts`（HISTORY_PROTOCOL_VERSION/PAGE_LIMIT=50 单一配置点；buildHistoryPageTag；evaluateIfNoneMatch RFC 9110 弱比较/`*`/非法忽略；evaluateHistoryConditionalGet）；接入 `server/routes/sessions.ts` 普通消息路由**成功返回边界**（授权/B-C 读取/外部状态/rebroadcast 之后，广播先于求值不被 304 取消）；响应序列化一次（200 复用同一字节串；不写缓存）；摘要输入=协议版本+进程盐(randomBytes32 不落盘)+principalId/serverNodeId/studio/sessionId/规范 locator+请求身份(endpoint/before/limit/模式/语言)+分支选择身份+revision+响应字节；`ReadSessionHistoryPageOutcome` 增加 `branchIdentity`（tryDirectoryOnce 从 directory.branch 直取，full/失败=null）。
- **实施中修复的 2 个真缺陷**（证据：ETag 稳定性调试脚本）：① headResolution/physicalTailLeafId 进入摘要——首次请求后 head 幂等写回使 legacy_tail→persisted_head，同 body 两请求标签必失配（E1 失去意义）；修正=分支选择身份只含 selectedLeafId（选择机制/弃支尾部不是表示维度）；② evaluateIfNoneMatch 的 currentOpaque 未剥引号——内层比较永不相等（条件命中恒失败）；修正后同标签命中 304。
- **E02.4 测试** `tests/history-protocol-conditional.test.ts`（7 用例，真实路由，每个 304 以同请求无条件 200 为 oracle 比较实际 JSON）：首次 200 协议头/ETag/私有策略 ✓；同表示 304 无正文/无 Content-Length ✓；页参数/翻页游标失配 200 ✓；追加后 200 新内容 ✓；文件不变但 deferred 媒体任务变化 200 ✓（注：普通 background-task 经 hydrate recordDeferredInterlude(afterIndex=null) 不产生块，媒体类才入表示——测试按实际行为用 image-generation 任务）；文件缺失不签发/403 不泄露 ✓；all=1 与 reconciliation=1 带条件头仍 200 完整响应且不签发 ✓；弱比较/`*`/非法语法 ✓。
- **验证退出码**：定向批次（条件 GET + history-read 十文件 + route-fallback + cache-auth + shared-functions + run-continuity + invalid-fixture + run-outcome-edges + sessions-route + session-find-route + desktop turn-outcome）**254/254 exit 0**；差分三模式 **21/21 逐字节一致 exit 0**（200 字节等价保持）；`npm run typecheck` 三配置 **exit 0**。
- 关键文件：`server/history-read/protocol.ts`（新）、`server/history-read/index.ts`（branchIdentity 穿线）、`server/routes/sessions.ts`（成功边界接入）、`tests/history-protocol-conditional.test.ts`（新）、protocol/ 三份合同文档。
- **E-a 状态：已修复并验证**（E01/E02/E02.4；E03 客户端、E04 概览、E06 实验为后续批次，本批未实施）。

### 2026-09-10 — 阶段 E 批次 E-b1 完成：E04 复用目录的会话概览端点 + 服务端测试（E03/E05/E06 未实施）

- **E04.1 端点**：`GET /api/sessions/history-overview`（server/routes/sessions.ts）——身份优先级/manifest/locator 解析/路径校验/sessions.read 授权与 messages 路由逐字对齐（"只是统计"不公开）；只读当前选中分支。响应头按协议合同：`Lingxi-History-Protocol: 1` + `Cache-Control: private, no-store`（概览无 ETag，条件快路径仅限普通消息页）。类型 `HistoryOverview`/`HistoryOverviewUnavailable`（schemaVersion:1，严格两 type）定义于 `server/history-read/protocol.ts`（共享位置），测试 import type 断言，无 any。
- **E04.2 计数维护**（接入点=目录/增量构建器）：
  - 新 `server/history-read/overview.ts`：`applyOverviewFact`（append/重算共用同规则）、`computeOverviewFromRecords`（冷/分支重建 O(N)）、`categorizeTaskType`（media=isMediaGenerationDeferredResult 的 image/video-generation；subagent/workflow 按记录自身元数据 type；无可信类别→other，不猜）。
  - 扫描器（projection-context.ts）新增 fact 稀疏字段 `deferredTaskRef {taskId,type}`（权威解析器 parseHistoryDeferredResult 的产出，custom 分支捕获）→ 随 recordFacts 进入全量与增量两路径（I04 同形状）。
  - `HistoryDirectory.overview`（types.ts 新字段）：runSizeByOrdinal overlay（MapOverlay）+ 三桶计数 + runsWithAssistant + taskCategories overlay + taskCategoryCounts；displayRecords=session.displayTotal（既有）、sourceRecords=records.length（既有）。增量 append 按fact 更新（Run 跨桶移动、taskId 首分类胜出去重）；rebuildBranchView 前缀重算（分支切换不返回旧分支统计）；增量 measuredBytes 计入 overlay delta。
- **附带修复 1 个 C03 真缺陷**（E04 测试暴露，证据=rewind 后追加 display=11≠6）：「分支选择变化+追加」同时发生时增量构建物理索引更新但沿用旧分支视图。修复=incremental.ts 末尾：head 规则 selected 变化且本次追加不延长当前分支（selected∉appendedChain，普通延长型追加不受影响/O(新增) 保持）→ rebuildBranchView 重建分支视图（O(N) 允许），fail→全量重建。open-run/常规追加路径不触发（C05 数字不受影响）。
- **E04.3 热路径**：命中有效目录时仅现有 probe/身份/分支检查 + 固定规模聚合读取（无全文件读/JSONL 解析/hydrate/遍历）；冷目录借用 B 同一 single-flight（beginBuild/buildHistoryDirectory），不建第二份目录；超预算（no-cache）→directory_unavailable；legacy_fallback→unsupported_history；revision=null→revision_unknown 不提供精确计数；缓存失效/分支重建不返回旧统计。测试用 C 阶段插桩计数断言（热 overview fullFileReadCalls=0、jsonlParseCount=0、deferredResults.listBySession 零调用）。契约 JSON 实测 <4KiB。
- **验证退出码**：`tests/history-overview-route.test.ts` 10/10；定向全套（overview + 条件GET + history-read 十文件 + route-fallback/cache-auth/shared-functions + run-continuity/invalid-fixture/run-outcome-edges + sessions-route/session-find-route + turn-outcome-unification）**264/264 exit 0**；差分三模式 **21/21 逐字节一致 exit 0**；`npm run typecheck` **exit 0**。
- 关键文件：`server/history-read/overview.ts`（新）、`server/history-read/protocol.ts`（概览类型）、`server/history-read/types.ts`（deferredTaskRef/overview 字段）、`server/history-read/projection-context.ts`（deferredTaskRef 捕获）、`server/history-read/directory.ts`（冷构建 overview）、`server/history-read/incremental.ts`（overview overlay/前缀重算/分支视图一致性修复）、`server/history-read/index.ts`（readSessionHistoryOverview + mapOverviewUnavailableReason）、`server/routes/sessions.ts`（路由）、`tests/history-overview-route.test.ts`（新，10 用例）。
- **E-b1 状态：已修复并验证**（E04/E04.1-4；E03 客户端、E05 展示、E06 实验为后续批次）。

### 2026-09-10 — 阶段 E 批次 E-b2 完成：E03 客户端条件校验与失效（E05/E06 未实施）

- **新模块** `desktop/src/react/stores/history-protocol-client.ts`：校验记录（内存 Map：requestKey/etag/connectionEpoch/appliedLiveVersion/appliedTodosVersion/原始记录边界覆盖证明/协议能力与推荐值）；请求键=epoch|sessionId|path|mode|before|limit|lang|proj（epoch=connectionId+authState+token 指纹，凭据不存原文）；`conditionalMessagesFetch`（有记录才发 If-None-Match，cache:'no-store'+throwOnHttpError:false，304→新鲜状态重查→valid=保留/stale=至多一次无条件补取/superseded=退出；400=标记 epoch+回退；网络错误=回退不标记；401/403=按既有错误抛出不标记）；LRU 每会话 32 条+全局 256KiB 元数据估算（超限淘汰记录不淘汰消息）；不新增 raw-page 缓存。
- **接入点（真实既有触发链，非新 helper）**：`loadMessages`（带 preloaded 可选参；条件尝试仅在有记录时发生，304 全有效→不解析 JSON/不 initSession/不推游标直接返回）；`reconcileCurrentSessionMessages` 同 revision 分支（E03.4 既有触发点：sessions_refresh/session_switch/chat-find-locate/mobile_foreground_refresh）→ 有记录时一次条件校验，200→preloaded 交给 loadMessages 完整归并链（单次传输无二次请求）。loadMoreMessages（新页首次加载）保持无条件。
- **失效清单落点**（E03.4，逐项）：WS 消息/块更新与 todo 更新→304 到达后新鲜版本重查（live/todos 版本失配→stale 补取）；registry/流式/本地应用/会话淘汰→`invalidateSessionCache` 钩子（selectors/file-refs.ts）即时丢记录；登出/切 workspace→无参 invalidateSessionCache 全清；认证变化/切换 server→epoch 变化键不可达自然失效；语言/投影版本→请求键维度。流式进行中/竞态在途→预检+304 重查双保守，不发 ETag 不覆盖 live 状态。
- **实施中修复**：`records.size===0` 快路径（既有无连接环境/测试在 conditionalMessagesFetch 内触碰 requireServerConnection 抛"connection not ready"破坏 16+ 既有用例）＋mock 普通对象响应的防御式头读取（headerGet）；401/403 误入网络回退分支→按 E03.5 重分类（抛错不标 epoch）；304 遗漏补取分类（todos 失配曾误归 superseded）→valid/stale/superseded 三分。
- **测试** `desktop/src/react/__tests__/history-protocol-client.test.ts`（11 用例，fetch/store 层 mock，不改 lingxiFetch 全局默认）：请求键构成、仅重校验带 If-None-Match、304 保留状态+不解析 JSON（POISONED 正文探针）、todos 失效补取无循环、新页无条件、200 应用后才保存、32 条/256KiB 上限、失效清单逐项、旧服务器无头回退 50、网络/400/401 异常分类。
- **验证退出码**：定向全套（客户端 11 + 条件GET 7 + 概览 10 + history-read 十文件 + route-fallback/cache-auth/shared-functions + run-continuity/invalid-fixture/run-outcome-edges + sessions-route/session-find-route + turn-outcome + session-actions store 89）**364/364 exit 0**；差分三模式 **21/21 exit 0**；`npm run typecheck` **exit 0**。
- 产出文档：`protocol/client-state-machine.md`（S0–S5 状态转换+新鲜重查清单+失效清单落点+限额+回退）。
- **E-b2 状态：已修复并验证**（E03.1–E03.5；E05 展示、E06 实验为后续批次）。

### 2026-09-10 — 阶段 E 批次 E-b3 完成：E05 概览客户端接线与最小展示（E06/E07 未实施）

- **新模块** `desktop/src/react/stores/history-overview-client.ts`：非阻塞概览客户端——同连接同会话在途请求合并（inFlight Map）；能力按连接+认证 epoch 隔离（404/405/结构不兼容→epoch 记 unsupported，回退既有翻页探底；401/403→清理失效统计不标能力缺席）；严格 schema/约束校验（schemaVersion、有限非负安全整数、三桶合计=runsWithAssistant、分类合计=referencedTasks、revision 非空、pagination 契约值）；stale 丢弃（捕获 currentSessionPath/_loadMessagesVersion/messageLiveVersion/revision generation，响应到达时已切换/流式/版本过期→丢弃不应用）；概览不 stamp 消息缓存、不判定对账 complete（纯独立快照）。
- **展示组件** `desktop/src/react/components/chat/HistoryOverviewBadge.tsx`：挂载于 ChatMessageSurface 既有「加载更早历史」提示区（loadMoreHint）内，不新增顶级入口/统计面板/布局改动。文案严格区分三概念（locale 键 chat.historyOverview.records/runs/tasks = 原始历史记录/逻辑轮次/关联任务；Run 数不表述为完成任务数）；任务分布四类放 title 提示（同一处轻量展开）。非阻塞触发=badge effect（页面已有内容且 active 时一次，在途合并），首屏渲染不依赖概览，失败/慢响应静默保留旧状态。
- **locale**：`desktop/src/locales/{zh,en,ja,ko,zh-TW}.json` 五文件逐一插入 `chat.historyOverview`（records/runs/tasks/taskSubagent/taskWorkflow/taskMedia/taskOther/unknown/detail），文本级插入保持原格式，五语言键集一致（测试断言）。
- **测试** `desktop/src/react/__tests__/history-overview-client.test.ts`（9 用例）：非阻塞 fire-and-forget 且不写消息 store；schema/合计约束拒绝坏数据（5 类坏 payload→unsupported）；stale 丢弃（切换+版本推进）；404 短路+epoch 隔离重探测；available:false 三 reason；401/403 清统计不标缺席；在途合并（同 Promise）；locale 五语言键集断言。
- **验证退出码**：定向全套（概览客户端 9 + E03 客户端 11 + 条件GET 7 + 概览路由 10 + history-read 十文件 + route-fallback/cache-auth/shared-functions + run-continuity/invalid-fixture/run-outcome-edges + sessions-route/session-find-route + turn-outcome + session-actions store）**373/373 exit 0**；`npm run typecheck` **exit 0**。
- 关键文件：`stores/history-overview-client.ts`、`components/chat/HistoryOverviewBadge.tsx`、`components/chat/ChatMessageSurface.tsx`（挂载）、`components/chat/Chat.module.css`（badge 样式）、五 locale 文件、`__tests__/history-overview-client.test.ts`。
- **E-b3 状态：已修复并验证**（E05；E06 页大小实验、E07 CORS 为后续批次）。

### 2026-09-10 — 阶段 E 批次 E-c1 完成：E06 页大小候选实验与落地（E07/E08 未实施）

- **E06.1 实验**：`scripts/benchmark-history-protocol.mjs` 补 `--phase E` 门 + 逐候选聚合（scripts/lib/e06-experiments.mjs：首屏/热页 p50/p95/max、每页正文 KiB、JSON 解码、单页新增峰值内存 heap/external/arrayBuffers 分开、各链路 walk 总时/字节）+ 压力场景（大正文/工具行、多块、多 Run、单超长 Run、隐藏整页、非局部依赖，每候选 K 冷+热页）+ etag 随 limit 变化校验（1k/10k 均 true）。命令：`--phase E --sizes 1000,10000 --seed 20260910 --page-sizes 50,100,150,200 --rtt-ms 0,50,150 --bandwidth-mbps 10,50` exit 0。产物 `protocol/page-size-experiments/summary-e.json`（含 experiments/stress/etagVariesByLimit）。调试修复：summary 初始化顺序、e06-experiments 循环变量笔误、stress 声明丢失（逐次实证后修）。
- **E06.2 决策：K=100**（`protocol/page-size-decision.md`）：全部候选过预算（语义失败 0；首屏最劣 210.8 ≤ 220.4；单页处理 <2ms ≪ 75ms；内存增量 ≤0；K>50 请求数 101/67/51 < 201；受控链路总时 16.1/12.3/10.7s 均 < 31.8s）。主要收益（往返 −50%、全翻 −49%）在 K=100 取得；K=150/200 首屏 +22~26ms/档、单页载荷翻倍，边际收益不抵成本 → 淘汰；K=50 保留为旧客户端兼容默认。落地 `HISTORY_PROTOCOL_PAGE_LIMIT = 100`（单一配置点，同源头/概览），合同/预算文件同步更新。
- **E06.3 接线**：客户端 `negotiatedHistoryPageLimit`（能力绑连接/认证 epoch；未知→省略 limit=服务端默认 50；合法 ∈{50,100,150,200} 采纳；头/概览冲突→保守 50+诊断）；`loadMessages`/`loadMoreMessages`/reconcile 条件路径均按协商值构造 URL 与记录 effective limit（50 页标签不串 100 请求）；游标仍只用服务端 nextBefore（混合 limit 无重叠无缺页）。E05 概览校验成功后回填 `noteOverviewRecommendedLimit`。
- **测试**：history-protocol-client.test.ts 增 6 用例（未知首请求省略 limit/合法 K 采纳+游标/非法 77 与超限 400 保守/冲突保守 50+诊断/50 页标签不串 100/epoch 隔离）；history-protocol-conditional.test.ts 增 1 用例（省略 limit→默认 50+声明头一致+概览同源）。
- **验证退出码**：定向全套（概览客户端 9 + E03 客户端 17 + 条件GET 8 + 概览路由 10 + history-read 十文件 + route-fallback/cache-auth/shared-functions + run-continuity/invalid-fixture/run-outcome-edges + sessions-route/session-find-route + turn-outcome + session-actions store）**380/380 exit 0**；差分三模式 **21/21 exit 0**；`npm run typecheck` **exit 0**。
- **E-c1 状态：已修复并验证**（E06.1/E06.2/E06.3；E07 CORS、E08 为后续批次）。

### 2026-09-10 — 阶段 E 收尾批次完成：E07 私有传输/CORS/兼容清理 + E08 协议阶段自检

- **E07 CORS（真实链路）**：定位生产 CORS 中间件（server/index.ts app.use("*")，白名单 isCorsOriginAllowed）；头契约抽为单一来源 `applyCorsResponseHeaders`（server/http/cors-policy.ts，server/index.ts 与 smoke 共用）：Allow-Headers 增补 `If-None-Match`（不影响 Authorization）；新增 Expose-Headers `ETag, Lingxi-History-Protocol, Lingxi-History-Page-Limit`；来源白名单不放宽（绝不 credentials+通配）；无既有 Vary 需合并（Cache-Control: private, no-store 禁缓存，Vary 无必要——记录为决策）。私有头经真实 HTTP 链路核实（smoke 断言），未设 public/s-maxage/Service Worker 预缓存。跨源预检：同源 Electron 渲染不触发；跨源 Web 每会话首次预检一次（loopback 实测 <1ms），已计入证据。
- **E07 故障场景测试**（client 12 + overview 10 内）：代理移除 ETag 头→不存记录；304 缺 ETag→无条件补取；无正文状态 200→不当成功空会话；概览 HTML 错误页→不崩溃保持原状；未知 schemaVersion/结构缺失→unsupported；能力值非法（77/400）→不采纳；服务器重启（盐更换）→旧标签 200 正常应用；401/403 不标 epoch。COMPAT 注释：history-protocol-client.ts（旧服务器无头行为）、server/routes/sessions.ts（旧客户端无条件分支）——退役条件=全部连接确认协议 v1（无自动删除日期，不改版本/发布坐标）。
- **E08 smoke**：`scripts/smoke-history-protocol.mjs`（真实 HTTP loopback + 生产 CORS 头契约函数）exit 0：两规模全部断言 PASS（协议/缓存/ETag 头、条件 304 无正文无 Content-Length、概览 available+同源 K=100、CORS 回显/Expose/预检 Allow-Headers 含 If-None-Match/credentials 非通配）。报告 `protocol/browser-smoke-report.md`：**真实浏览器/Electron 环境未验证，留待 F 阶段指定环境**（Node loopback 层面，不冒充浏览器结果）。
- **E08 三目标判定（分别）**：
  - E1（条件校验）= 已修复并验证：真实生产重校验触发点（reconcileCurrentSessionMessages 同 revision 分支，既有链 sessions_refresh/session_switch/chat-find-locate/mobile_foreground）产生 200→304；304 不解码/不重建（POISONED 正文探针测试）；todos/live 失配→stale 补取并正确应用 200（history-protocol-client 测试）。
  - E2（概览接线）= 已修复并验证：HistoryOverviewBadge → history-overview-client（schema/约束校验、在途合并、stale 丢弃）→ GET /api/sessions/history-overview（E04 目录计数）→ useSyncExternalStore 渲染（history-overview-client 测试 10 用例 + smoke 概览断言）。
  - E3（页大小实验+落地）= 已修复并验证：四候选 K=50/100/150/200 实测（page-size-experiments/summary-e.json）、预算逐条判定（page-size-decision.md）、K=100 落地单一配置点+头/概览同源测试+客户端协商测试（history-protocol-client K 协商 6 用例）。
- **进入 F 前检查清单**：all/reconciliation 身份与完整性测试仍通过（session-find-route + run-continuity + T12 系列绿）✓；固定 limit=50 的 P/X 无新增失败（定向 385/385）✓；无条件标签 hash 再全读会话（协议求值零读取；代码审查+summary-e.json 概览/条件请求 server span 与 50 对照持平）✓；客户端条件记录无正文复制（记录字段仅 key/etag/epoch/版本/原始边界 id/能力——history-protocol-client.ts 字段清单审查）✓；概览不阻塞（badge effect fire-and-forget；失败静默）✓；私有头与 CORS 经真实链路核实（本 smoke）✓。
- **验证退出码**：定向全套 **385/385 exit 0**；差分三模式 **21/21 exit 0**；`npm run typecheck` **exit 0**；smoke **exit 0**。
- **E07/E08 状态：已修复并验证**（真实浏览器/Electron smoke 留待 F 阶段指定环境，已如实标注）。阶段 E 全部批次（E-a/E-b1/E-b2/E-b3/E-c1/E 收尾）完成。

### 2026-09-11 — 阶段 F 批次 F-a 完成：F01 冻结兼容矩阵并执行 Y01—Y24

- **四组合（真实接口/请求链）**：
  - 旧+旧（控制组）：A01 clone（/tmp/lingxi-baseline-1d42b740，HEAD=1d42b740，未提交/未建分支）旧 sessions 路由以 Node type-stripping 子进程真实运行（tests/compat-old-server-runner.mjs，临时文件）；旧客户端请求构造（path+sessionId、省略 limit、无条件）→ 200 旧 JSON、无协议头/无 ETag ✓。
  - 新+旧：新路由真实 HTTP × 旧客户端构造 → 省略 limit 默认 50、正常 200、七字段 JSON 可消费、新增头不影响旧解析 ✓。
  - 旧+新：desktop compat（真实 lingxiFetch→旧服务端 HTTP）→ loadMessages 200 无感回退不存记录、概览 404→epoch unsupported 不卡住、翻页 nextBefore 照常推进、reconcile 正常触发 ✓。
  - 新+新：真实 HTTP 条件请求 200→304（无正文）、ETag 随 limit 变化、K=100 协商生效 ✓。
- **Y01—Y24**：23 项「已修复已验证」+ Y14「已实现但指定环境未验证」（真实浏览器跨源留待 F 指定环境；Node 真实链路已验 CORS/预检/Expose）。逐项状态/证据/层级见 `protocol/compatibility-matrix.json`。Y06 鉴权先于条件求值、跨主体不侧漏（标签含作用域身份）；Y05 已有用户分支操作使请求/receipt 失效（branch_view_stale/rewind 用例），无通知外部分支修改边界如实另列；Y14 按 Node 真实链路验证并标注层级。
- **新增文件**：`tests/history-protocol-compat.test.ts`（服务端四组合+新+新 条件/etag）、`desktop/src/react/__tests__/history-protocol-compat.test.ts`（旧+新 客户端回退真实 HTTP）、`protocol/compatibility-matrix.json`、clone 内临时 runner `tests/compat-old-server-runner.mjs`。
- **A01 既有状态记录**：clone 内 `artifacts/f1-f12-repair/round2/patches/89bc0b64-to-r01-r10-source.patch` 有本地修改（全量 vitest 的证据再生成测试所致，F02/D02 已核实为 HEAD 既有行为），未触碰。
- **验证退出码**：F01 定向（含两个新兼容文件）**389/389 exit 0**；差分三模式 **21/21 逐字节一致 exit 0**；`npm run typecheck` **exit 0**。
- **F01 状态：已修复并验证**（兼容矩阵冻结并执行；Y14 真实浏览器跨源按 Node 真实链路验证并标注层级）。

### 2026-09-11 — 阶段 F 批次 F-b 完成：F02 复测并分开计算四类收益

- **基准 1（目录）**：`node scripts/benchmark-history-read-directory.mjs --phase F --sizes 1000,10000 --page-size 50 --seed 20260910 --output artifacts/history-read-directory/verification-f` exit 0。1k 冷 10.60ms/热 0.61ms、10k 冷 78.94/热 0.61ms；全翻 33.3ms(1k)/224.5ms(10k)；四阈值全 PASS；fullFileReadCalls=0、jsonlParseCount=103。
- **基准 2（协议）**：`node scripts/benchmark-history-protocol.mjs --phase F --sizes 1000,10000 --seed 20260910 --page-sizes 50,100,150,200 --rtt-ms 0,50,150 --bandwidth-mbps 10,50 --output artifacts/history-read-directory/protocol/verification-f` exit 0。产出 `experiments`（8 格：首屏/热页/解码/内存/每页字节）+ `etagVariesByLimit`（1k/10k=true）+ **决定性 304：两规模 20/20 全 304、bodyBytesTotal=0、rebuilds=0**。
- **四张对照表**（`protocol/protocol-performance-comparison.json`，全部数字溯源至 summary/requests 原始文件）：
  1. A→D→F（K=50 正常 200）：热页 1k 7.72→0.98→0.61ms、10k 75.53→0.67→0.61ms；全翻 10k 15,341→254.7→224.5ms；fullFileReadCalls 2→0→0；解析 4004/40004→103/103。
  2. D→F 同页重复校验（10k rtt150/bw10）：D=200 完整正文 30,753B+解码合并；F=304 bodyBytes=0（≈100% 正文字节节省）；服务端保留 B/C 读取+hash 成本（E02 允许）。
  3. 旧方式（逐页翻完才知道总量）→ E/F 概览：201 请求/6095KiB → 1 请求/≤4KiB（含冷构建摊销 78.9ms@10k；热读 0 文件读/0 解析）；如实标注旧行为无强制全翻。
  4. F(50)→F(K=100)：201→101 请求（−49.8%）、31.7s→16.5s（−48%）、每页 30.3→59.6KiB。
- **计数维度**：etagMs（sha256 一次 <1ms）、conditionalStatus、clientReuse、clientFallbackReason、preflightRequests（跨源每会话首次 1 次，loopback <1ms；同源 Electron 不触发）、headerBytes/bodyBytes 分列——不将正文大小等同 TLS/网络开销。
- **脚本修正**：protocol benchmark 补 `--phase F` 门、F02 决定性 304 块、requests-*.jsonl 恢复写入（重构中丢失）；F02 块内 `base` 残留变量与 TS cast 清理。
- **验证退出码**：目录 F exit 0（阈值 4/4 PASS）；协议 F exit 0（决定性 304 20/20 两规模）；定向全套（含两个新兼容文件）**389/389 exit 0**；差分三模式 **21/21 exit 0**；`npm run typecheck` **exit 0**。
- **F02 状态：已修复并验证**（四表齐备、决定性 304 通过、原始数据可溯源；D 固定 50 基线未被 E/F 最终 K 数据覆盖）。

### 2026-09-11 — 阶段 F 批次 F-c 完成：F03 故障、安全和资源最终验证

- **barrier 注入测试**（desktop/src/react/__tests__/history-f03-barrier.test.ts，17 用例）：
  - **200 完成接收前注入六事件**（WS 消息 bumpMessageLiveVersion/分支重置 flight resetSeen/认证切换 token→epoch/会话淘汰 chatSessions 删除+invalidate/服务器替换 serverPort/abort load 版本推进）→ 六用例全绿：有限回退（≤3 次调用）、状态不回滚、loading 不滞留。
  - **304 返回前注入**：todos 版本变化→stale→至多一次无条件补取（3 次调用无循环）；WS live→superseded 直接退出；会话淘汰→superseded 不清空历史不伪造成功；abort 版本推进→superseded 退出；认证切换→记录不可达不标能力缺席。
  - **overview 到达前后注入**：切换会话/流式开始→丢弃（快照不应用、data=null）。
- **资源有界性（服务端目录预算 × 客户端 receipt 预算共存）**：校验记录 ≤32 条/会话、元数据 ≤256KiB（超限淘汰记录不淘汰消息，chat 会话 items 完整）；反复连接切换（epoch 变化 5 次）→ 旧 epoch 记录不可达、总数有界；失败重试有限（条件 reject + 无条件回退共 2 次）；请求取消（版本推进）→ 条件 304 后退出（1 次调用，不无限重试）。
- **故障回退计数**：所有回退均有限次（条件 1 + 补取 1，无循环）；不吞鉴权错误（401/403 抛错走既有流程、不标能力缺席）；无永久"验证中"loading（flight 消费 + loading 清理）；失败后不清空 messages（旧状态保留）；不持续探测不支持端点（epoch 标记后短路）。
- **验证退出码**：F03 barrier 17/17；定向全套（概览客户端 9 + E03 客户端 11+6 + 条件GET 8 + 概览路由 10 + history-read 十文件 + route-fallback/cache-auth/shared-functions + run-continuity/invalid-fixture/run-outcome-edges + sessions-route/session-find-route + turn-outcome + session-actions store）**406/406 exit 0**；差分三模式 **21/21 逐字节一致 exit 0**；`npm run typecheck` **exit 0**。
- **F03 状态：已修复并验证**（故障/安全/资源最终验证完成；仅本机隔离服务与合成数据，未触碰线上代理/CDN，未上传真实会话）。
- **F 批次完成状态**：F01（兼容矩阵+Y01—Y24）、F02（复测+四类收益对照）、F03（故障/安全/资源最终验证）均「已修复并验证」。阶段 F 的 F04+（若 TASKBOOK 有后续交付验收项）与最终全量验收留待下一批次。

### 2026-09-11 — 阶段 F 批次 F-d 完成：F04 最终类型检查、全量测试与指纹门禁

- **F04-a 定向全套**：协议定向（条件 GET 8 + 概览路由 10 + 客户端条件 17 + 概览客户端 9 + 兼容四组合 8）+ history-read 十文件 + P08 语义集 + session-actions store → **406/406 exit 0**。
- **F04-b typecheck**：`npm run typecheck` 三配置串联一次执行（base/node/test），**exit 0**。
- **F04-c 指纹门禁**：guard 首查触发（E 协议阶段 4 个受护源变更未再 pin）→ 按实际 diff 生成 compatible repin（理由：E 协议阶段新增客户端渐进能力/条件请求/概览与 CORS 头契约单一来源，持久化形状与字段保持兼容，无 DATA_EPOCH 变更）→ guard **exit 0** + tripwire **15/15 exit 0**。指纹 diff 仅 payloadFingerprint/review 理由/受护源 sourceHash，无 schema/registry 形状变化。
- **F04-d 全量对照**（vs D02 检查点 13544/7）：
  - 当前工作区全量：**13735 passed / 7 failed**（exit 1=存在既有失败，属预期），**失败签名与 D 检查点 7 项逐条一致（diff=空）**，任务引入失败 **0**。
  - 7 项均为 A01 既有：ObservabilityDateLine×2（桌面图表几何）、packaged-desktop-cleanup（90s 超时）、audit-seal（封印坐标，未触碰）、round2×2 + round3×1（交付证据 manifest 复算，工作区含任务改动即红）。
  - 中间态已消除：cli-closure×2、open-boundary-lint×2（闭包/边界基线随 E 新源再生成 + manifest 登记后全绿，见 E06.3/E-c1 记录）。
- **产出**：`known-failures-comparison.json`（基线 vs 最终失败签名逐条对照与分类）、`test-results.json`（F04 全部命令/退出码/计数）。
- **F04 状态：已修复并验证**（四步命令全记录；任务引入失败 0；既有 7 项保持原状不掩盖）。

### 2026-09-11 — 阶段 F 批次 F-c3 完成：F05 最终审查 + F06 最终交付物

- **F05 真实链路核对（四链路全通，均为生产代码路径）**：
  1. 普通路由→目录/定点读取→同源 projector→当前外部状态→条件返回（`server/routes/sessions.ts` → `server/history-read/*` → `protocol.ts` 条件求值）；
  2. 客户端重校验→receipt 判定→正确 304/200 分支（`history-protocol-client.ts`；F03 barrier 17 用例约束 200 接收前/304 返回前注入语义）；
  3. 概览→同目录聚合→非阻塞展示（`overview.ts` 0 文件读/0 解析 → `history-overview-client.ts` → `HistoryOverviewBadge.tsx`）；
  4. 能力→显式 limit→原 nextBefore（客户端显式 100，省略 limit 旧请求仍 50、最大 200，游标语义不变）。
  未发现停在未调用函数、测试专用分支或未启用开关的环节。
- **F05 范围审查**：单一分支/Run/关联语义；无服务器响应缓存与客户端第二份正文缓存（客户端仅 stamp/统计：≤32 条、≤256KiB）；未为 ETag 全读历史；概览未全扫描 task/run；未强制 all；无新持久化文件/表（E 阶段 4 受护源 compatible repin 后 guard 0、tripwire 15/15、无 DATA_EPOCH 变更）；无 SDK 核心改动；无发布操作；A01 保护清单复核——他人修改未回退/覆盖。前端新增限于 E 允许的请求/状态/统计/页策略范围，无布局重构/Run 合并重写。
- **F06 三补丁与验证**（应用基线=/tmp/a01-tree 干净 HEAD clone）：
  | 补丁 | 内容 | --check/apply | 树级等价验证 |
  |---|---|---|---|
  | a-to-d-server-stage.patch | A01→D（47 文件） | 通过/成功 | 既有轮次已验 |
  | d-to-final-protocol-stage.patch | D→最终（38 文件） | 通过/成功 | a-to-d→d-to-final 应用后 vs 工作区 diff=0 exit 0 |
  | task-only.patch | A01→最终任务专属（74 文件） | 通过/成功 | 应用后 vs 工作区 diff=0 exit 0 |
  生成方式：`diff -ruN` 统一排除集（.git/node_modules/artifacts/dist*/构建产物/本地忽略文件；`--exclude=.build` 消除 LingxiSpeechHelper/.build 符号链接循环），表头归一 `a/`、`b/`；排除补丁与校验清单自身。task-only 文件集与 git status 任务清单 SET-IDENTICAL（排除他人既有差异与本地忽略文件，不以全量脏 diff 冒充）。唯一警告=a-to-d 既有 EOF 空行（D 阶段内容）。
- **F06 矩阵与总报告**：acceptance-matrix.json 扩至 P/X/Y 共 56 项（55 已修复并验证 + Y14 已实现但指定环境未验证 + 0 受阻 + 0 经证据否定，D07 暂缓项已随 protocol/baseline-d 闭合）；acceptance-report.md 改写为 F06 最终总报告（A—F 阶段判定、四链路、协议收益、A/D/F 数字、补丁验证、剩余风险与未验证环境）。
- **checksums.sha256** 最后生成：覆盖交付目录除自身外全部最终文件（相对路径）。
- **F05/F06 状态：已修复并验证**（补丁链端到端可复现工作区；56 项状态计数见 acceptance-matrix.json；未验证环境=Windows/Linux/macOS x64、打包产物、真实浏览器跨源 CORS，均在总报告 §八如实标注）。

### 2026-09-11 — 阶段 F 批次 F-e 完成：A01 夹具污染修复 + runner 自包含化

- **事件**：F-e 补丁验证误将 A01 快照 clone `/tmp/lingxi-baseline-1d42b740` 当作 apply 目标：clone 工作区被写成任务内容（git status 与主工作区相同），且未跟踪 runner 丢失——`tests/history-protocol-compat.test.ts` 失败（old server MODULE_NOT_FOUND），且即便补回 runner 跑的也是新代码，旧+旧/旧+新组合无效。
- **恢复**（以 initial-status.txt/protected-changes.json 权威记录为准：A01=porcelain=0 干净；round2 patch 的 M 状态系后续全量测试在位再生成，非 A01 既有）：clone 内 `git reset --hard 1d42b740` + `git clean -nd` 核单（18 项均为任务引入，无既有文件）后 `git clean -fd` → **HEAD=1d42b740、porcelain=0、round2 patch 与 HEAD 一致**。
- **runner 自包含**：模板 tracked 于 `tests/compat-old-server-runner.template.mjs`（内容=F01 已验证 runner；hono 改裸导入，修复依赖重装后 `hono/dist/hono.mjs` 硬编码路径失效问题）；两个 compat 测试 beforeAll 幂等 `copyFileSync` 写入 clone 再 spawn；clone 缺席时 `describe.skipIf` 整套件 skip + 模块级 console 明示「A01 clone 缺席……四组合中旧服务端侧指定环境未验证」。
- **复验（全部实跑，2026-09-11）**：
  - `tests/history-protocol-compat.test.ts`：**4/4 exit 0**（旧+旧/旧+新断言响应无 `lingxi-history-protocol` 头、无 ETag——旧代码真实性由断言固定）
  - `desktop/src/react/__tests__/history-protocol-compat.test.ts`：**4/4 exit 0**
  - skip 路径（clone 临时缺席实测）：exit 0，4 skipped，console 明示输出
  - 定向全套 25 文件：**410/410 exit 0**（兼容 8 + 条件/概览/客户端 + history-read 十文件 + P08 语义集 + session-actions store）
  - `npm run typecheck`：**exit 0**
  - 全量 `npx vitest run`：exit 1；**13735 passed / 7 failed / 7 skipped**；7 失败签名与 A01 基线逐条一致（round2×2、round3×1、packaged-desktop-cleanup 90s、audit-seal、ObservabilityDateLine×2），任务引入失败 0
- **补丁再生成**：`d-to-final-protocol-stage.patch` 40 文件（+runner 模板、+根 PROGRESS.md 任务记录）、`task-only.patch` 76 文件（文件集与 git status 任务清单 SET-IDENTICAL）；a-to-d→d-to-final 顺序与 task-only 直接应用两路 `git apply --check` 全过、apply 全过，应用后树级 diff vs 工作区**均 0 差异 exit 0**。
- **矩阵/风险更新**：compatibility-matrix.json 新增 `environmentDependency`（缺席 skip 语义、旧代码真实性断言）+ Y01 注记；acceptance-matrix.json Y01 同步；remaining-risks.md 新增「三A 教训：可写夹具被污染（F-e 事件）」——防护三件套=模板 tracked + 测试幂等恢复 + 头断言，且 patch 验证改用独立副本树不触碰夹具本体。
- **F-e 状态：已修复并验证**（夹具恢复纯净；runner 可随时重建；本机复验全绿）。
