# 阶段 C 实施设计（主线基于 B 实现现状撰写；实施时以 TASKBOOK C01—C05 为准）

前置事实：scan.ts 已支持 `startOffset` 续读；cache.probe 目前要求 size/mtime 全等，
任何追加 → file_identity_changed/snapshot_changed → 全量重建（B 阶段正确行为）。
index.ts 的失败链：尝试1 → invalidate+重建 → 尝试2 → legacy 全量。

## C01 变化分类（probe 扩展）

probe 返回值扩展为 `"valid" | "append_candidate" | "branch_view_stale" | InvalidationReason`。
判定序（directory.file / directory.branch / ctx / mutationEpoch）：

1. `ctx.publicRevision == null` → `revision_unknown`（不读不写缓存，I08）
2. dev/ino 不等 → `file_identity_changed`（同路径替换，X04；全量重建）
3. locator 不等 → `locator_changed`
4. size < directory.file.observedFileSize → `snapshot_changed`（缩短→全量重建）
5. size == observedFileSize：
   - mtime/ctime/revision 全等 → 查分支：head 三态字段全等 → `valid`；不等 → `branch_view_stale`（文件不变分支改变→保留物理索引、只重建分支视图）
   - mtime/ctime 不等（同长度重写/归档 utimes，X02）→ `untrusted_mutation`（无法区分→全量重建）
6. size > observedFileSize：
   - **mutationEpoch 未变**（自目录发布以来无重写/修复/生命周期通知）且 dev/ino 相同 → `append_candidate`（可信追加，走 C03 增量）
   - 否则 → `untrusted_mutation`（重写后变大或无法区分追加与重写，X03；全量重建）
7. 追加与分支变化同时发生 → 增量扫描更新物理索引后按 head 规则重建分支视图（弃分支追加不污染当前视图，X09）

## C02 可信依据（mutation epoch 内存通知）

新增窄模块（如 `core/session-file-mutation-epoch.ts` 或并入既有 engine 生命周期）：
进程内 Map<resolvedPath, { epoch: number, lastKind: string }>；`noteSessionFileMutation(path, kind)` 递增。
**只是缓存提示，不是持久化事实源；不改变写入次序/持久化形状/SDK 行为。**

插桩点（writer-invalidation-map §汇总表逐项核对，覆盖全部受支持重写路径）：
- repairOversizedSessionEntriesInFile / writeSessionEntriesFile / flushSessionManagerSnapshot
- open 时 v1→v3 迁移重写
- 孤儿 toolResult 删除、inline media 修复、fork 子文件改写
- 归档/恢复/删除（rename/unlink/utimes）、locator 重新绑定
- **时序：重写类必须在「旧偏移可能失效之前」递增（写前调），写失败不得回退 epoch**
- SDK appendFileSync 追加路径**不**递增（这正是可信追加）

目录发布时记录 `mutationEpochAtBuild`；probe 第 6 步比较当前 epoch。
**信任边界（必须写进报告）**：承诺范围 = 单进程写入模型 + 应用层通知覆盖的全部重写路径 +
dev/ino/ctime 捕获的替换；不承诺 = 任意外部进程 in-place 改写未读前缀（无统一权威通知，
此类变化无法与纯追加区分——已知边界，writer-map 记录 Lingxi 有同宅互斥闸但存在双内核
冷启动秒级竞态窗口）。不依赖 fs.watch；保留每次 stat/身份检查。

## C03 增量扫描与合并

续读起点 = `pendingTailOffset ?? lastUndelimitedRow?.offset ?? indexedThroughOffset`
（lastUndelimitedRow 重读以确认分隔符补齐/内容未变；不从旧 observedFileSize 盲续）。

`buildHistoryDirectoryIncremental(oldDirectory, scanDelta, ctx)`：
1. 新条目按物理序处理：id 唯一性对 byEntryId 校验（重复/自环/缺父 → 放弃增量，
   `directory_invalid` → 走全量重建+既有严格层，X01）
2. parentId 判定：延续当前 leaf → 追加进分支；挂到其他节点 → 其他分支（更新物理索引，
   视图按 head 规则决定，不自动计入当前 display 总数）；非法结构同上放弃增量
3. **版本隔离（I06）**：旧版本持有者看到完整旧快照。禁止原地修改共享数组/Map：
   - records：分段结构（如 1024/段）或 base+tail copy-on-write，追加只复制尾段
   - byEntryId / toolResultByCallId 等 Map：base+delta overlay（查 delta 落 base），
     delta 以新增条目为有界
   - Run 边界：**从记录级 fact.turnStartIndex/turnEndIndex 迁到 runOrdinal→{start,end} 的
     按 Run 小表**（该 Run 对象单独 copy-on-write）——超长开放 Run 逐条追加不得 O(Run 长度)
     改写全部记录；此改动需同步全量模式 context 与 materializeProjectionContext（I04 同一形状）
4. 新版本 bounds：observedFileSize/indexedThroughOffset/pendingTail/lastUndelimitedRow 按
   增量扫描结果更新；publicRevision=读前捕获值，发布前读后复核（I07）
5. 冷构建允许 O(N)，增量只允许 O(新增字节+新增条目+受影响关系)

## C04 受影响关系（仅处理新增/变化条目，逐项测试）

| 追加事实 | 增量更新 |
|---|---|
| 开放 Run 新增 assistant | 该 Run 表项 turnEndIndex（copy-on-write 单 Run 对象）；重读旧页得新结局 |
| 旧 toolCall 的结果 | toolResultByCallId delta；旧页重读结局刷新（X10） |
| 输入消费事件 | turnInputByAssistantEntryId + consumption 集合/反向索引（X11） |
| 协作决定 | collabDecisionBySuggestionId（X12） |
| correlation 冲突 | correlationByUserEntryId ambiguous 更新（X12） |
| origin/review/presentation/modelCall 前置事件 | 对应 bySourceIndex 指针（后继消息在新增条目内时） |
| 媒体最终记录/状态事件 | mediaResultRecords 追加 |
| todo 快照 | todoSnapshot 指针前移/坏快照跳过/清空规则（X13，规则留在 extractLatestTodoSnapshot） |
| 文件引用记录 | activeFileReferenceIdentities 增量并入 |

## C05 验收

- benchmark 扩展 --phase C：1k/10k 热目录上追加 1/10/100 条与一条大工具结果；
  记录新增字节/尾记录重读/验证字节/解析条数/旧元数据访问/受影响关系数/更新时间；
  两规模新增工作量一致（仅尾记录差异允许）
- 超长开放 Run 逐条追加：无 O(Run) 批量改写证据（计数）
- ≥1 用例经真实生产写入入口（真实 SessionManager append + 分支同步）+ 真实路由读取
- 增量命中率计数（禁止「每次都降级重建却宣称 C 完成」）
- 输出 artifacts/history-read-directory/phase-c/

## 回归门禁（每步后）
history-read 十测试文件 + P08 六套件 + 差分三模式 21/21 + typecheck。
P04：翻页中正常追加——新增可见事实正确、旧坐标稳定、重读旧页===当前完整投影。
