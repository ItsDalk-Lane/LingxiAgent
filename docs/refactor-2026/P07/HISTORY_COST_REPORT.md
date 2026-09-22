# HISTORY_COST_REPORT — P07-T03 历史读取与重复投影

日期：2026-09-22｜结论：**UNCHANGED_VERIFIED（读取/投影零改动）+ 冷热分离与工作量计量复核**。

## 1. 生产链路与缓存权威

- 路由：`server/routes/sessions.ts:316` — `engine.historyReadCache instanceof HistoryDirectoryCache ? 复用 : new HistoryDirectoryCache()`（实例私有，无模块级单例）。
- 缓存（`server/history-read/cache.ts`）：每会话一槽（键 = runtimeId\0studioId\0sessionId|path），预算 **maxSessions=8 / residentBytes=64MB / 单目录 16MB / 并发构建 2**；probe 四条件（fstat 五元组/revision/head 三态/locator）任一不过即 invalidate；构建信号量 + per-key single-flight；publish 版本校验。
- 分页投影：page 游标（before/limit）+ Run 结局逻辑为 **P05 锁定面**，本阶段零触碰（P06 交接 §9）。

## 2. 读取/parse/投影计数（同 fixture seed=20260910、page-size=50，与 P00 成对）

### 2.1 phase A（真实 v3 路由，未注入观测 cache）

| 指标 | n=1000 | n=10000 | P00 对照 |
|---|---|---|---|
| 冷首页 serverTotal p50（×3） | 10.77ms | 72.63ms | 11.25 / 84.1ms（可比） |
| 热页 serverTotal p50/p95 | 0.894/1.208ms（20 样本） | 0.710/0.879ms（200 样本） | 1.108/0.824ms（可比） |
| **fullFileReadCalls/请求（热页）** | **0**（p50=p95=max=0） | **0** | 同（工作量硬断言保持） |
| **jsonlParseCount/页** | 103（p50=p95） | 103（p50=p95） | 同 103 —— **O(K+|Dpage|)，与总会话规模无关** |
| 完整翻页（21/201 页）serverTotal | 30.7ms | 220.0ms | 35.1 / 244.7ms |

### 2.2 phase B（注入可观测目录 cache，同一生产实现）

`samples/history-p07-phaseB/summary-b.json`：热页 p50 0.9125ms（n=1000）/ 0.715ms（n=10000），fullFileReadCalls=0、jsonlParse 103/页、页数与输出唯一性断言通过。

**结论：热分页成本严格受请求窗口约束（每页恒定 ~103 条 metadata 解析 + 0 次全文件读）；重复投影未随规模放大。**

## 3. 冷热分离与首建索引单列

- **冷首页（首建目录索引）单列报告**：n=1000 ≈ 10.8ms、n=10000 ≈ 72.6ms（线性于文件规模，一次性）。
- **热页**：0.7–0.9ms（目录命中，无全文件读）。
- 冷热比 ≈ 12–100×：目录缓存的收益结构如实呈现，不隐瞒冷成本（A04 场景：两个副本分开报告、内容等价——翻页页数/唯一性断言在两 phase 均通过）。

## 4. 缓存失效（A05 场景）

- 追加（P07-FIXR1 更正，原「判非 fresh → invalidate → 下一批重建」不准确）：soak（SOAK_RESOURCE_REPORT §组件 3）每批对驻留会话文件**外部**追加一行 → `cache.probe` 实际判定 `append_candidate`（外部追加未走插桩写入、mutation epoch 不变 → 目录保留、非 invalidate；读路径仅在 `verdict === "valid"` 直接命中，其余走增量/全量重建，不返回旧快照）；soak 每批的目录重建由 12 会话>8 槽 LRU 颠簸驱动（12/12 批 rebuilds=12、invalidations={}）。四类变更判定的独立实测见 P07_ACCEPTANCE_REVIEW.md CE-2（追加/同长度重写/截断/原子替换 → `append_candidate`/`untrusted_mutation`/`snapshot_changed`/`file_identity_changed` 全部正确）。
- 截断/原子替换：probe 的 fstat 五元组（size/mtimeMs/dev/ino/ctimeMs）+ publicRevision 覆盖（同路径不同 inode/大小必失配）；既有测试锁定：tests/history-read-directory-cache.test.ts（probe/invalidate/版本校验 X16）、tests/history-read-directory-counters.test.ts、tests/history-read-directory-semantics.test.ts（分支/资源范围不串）。
- mtime 单一假设不存在：fileIdentity 为五元组（见 `cache.ts` probe 四条件）。

## 5. 重复投影与内存

- 热页 heapUsedDelta p50 ≈ 1.1MB/请求（phase A 仪表），驻留由 64MB 预算约束；soak 12 批实测 cache residentBytes 稳定在 ~0.5MB 量级、sessions 恒 ≤8、evictions 136 次（淘汰后正确重建：每批 12 会话工作集 > 8 槽 → 重建 12 次/批且命中 6 次/批）。
- 大载荷无正文驻留（可达对象图审计）：tests/history-read-directory-memory.test.ts（既有，全绿复跑见 P07_REPORT 验证节）。

## 6. 改动与回退

零生产代码改动（P05 锁定面零触碰）。新增仅基准样本与命令日志（`P07-T01-bench-history`、`P07-T03-bench-history-phaseB`）。回退 = 删除样本目录，无代码回退项。
