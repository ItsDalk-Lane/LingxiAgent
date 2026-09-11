# 服务端阶段报告（A—D 检查点，D06）

- 生成：2026-09-10｜基线 HEAD `1d42b7405c76292f617291e3a01cd2f3ef5efd04` @ `fix/pending-sep10`
- 环境：Node v24.16.0 / macOS arm64 (Apple M3 Ultra) / 本地 APFS（同机同负载口径，OS page cache 未清空；A—D 服务端基准不含真实网络，D07 另测）
- 配套：`acceptance-matrix.json`（P/X 逐项状态）、`acceptance-report.md`（回归对照与门禁详情）、`patches/a-to-d-server-stage.patch`（相对 A01 快照的可复跑检查点，已验证可干净套用于纯净 HEAD 树）

## 一、A—D 实现摘要

- **A（基线）**：旧读取路径性能基线 + 7 份冻结参考输出（1k/10k first/middle/last/all，sha256Normalized）+ 差分 harness + 仪表（read/parse/identity/serialize/residual、fullFileReadCalls、jsonlParseCount、metadataVisitedCount、内存采样）。
- **B（目录快路径）**：同源抽取（B01）→ 只读扫描 + 目录构建（B02/03，`server/history-read/`：scan/directory/projection-context）→ 定点窗口读取与 `HistoryDirectoryCache`（B04/05：8 槽 LRU、64MiB 驻留、16MiB 单目录准入、single-flight、版本化发布/失效）→ 路由接线（B06/07，`/sessions/messages` 生产主路径启用；失败链 尝试1→invalidate+重建→尝试2→legacy 全量）→ B08 验收（§7.1 工作量硬断言全过；B 冷页优化：补齐逻辑上移 finalize、lineageHash:false）。
- **C（可信追加增量）**：C01 probe 判定序（valid/append_candidate/branch_view_stale/失效 reason）；C02 mutation-epoch 进程内通知（插桩全部受支持重写路径，SDK append 不通知）；C03 增量扫描（续读起点 pendingTail→lastUndelimitedRow→indexedThrough、尾重读核对、SegmentedList/MapOverlay 版本隔离、Run 边界按 Run 小表）；C04 受影响关系 overlay（toolResult/turnInput/consumption/collab/modelCall/origin/presentation/review/correlation/todo/文件引用）；C05 真实路由增量验收。
- **D（本阶段）**：定向回归、phase D 复测、压力边界样本、三配置 typecheck、持久化指纹 repin 门禁、全量回归对照、闭包/边界基线 repin、检查点冻结。

## 二、参考与差分证据

- 冻结参考：`reference-outputs/`（7 份）+ `reference-manifest.json`；防污染前后 sha256。
- 差分三模式（full/cold/hot）：**21/21 sha256Normalized 逐字节一致**（阶段 B、C、D 各复验一次，最近一次 2026-09-10 20:0x，exit 0）。
- 阶段 C 增量证据：`phase-c/summary-c.{json,md}`（追加批次计数、parsedRecords、命中率、开放 Run 离散度）。
- 阶段 D 复测证据：`verification/summary-d.{json,md}`。

## 三、P/X 结果

32/32 项「已修复并验证」（P01—P12、X01—X20），逐项证据见 `acceptance-matrix.json`；范围注记：P05 前端跨请求清空留待 E/F，P09 retired 未接线为 HEAD 既有有界边界。计数：已修复并验证 32、已实现但指定环境未验证 0、待验证或受阻 0、经证据否定 0。

## 四、全量回归对照（要点，详见 acceptance-report.md）

- 纯净 HEAD 基线：13544 passed / 7 failed（7 签名全部 HEAD 既有：round2×2、round3×1、packaged-desktop-cleanup 超时、audit-seal、ObservabilityDateLine×2）。
- 当前工作区（闭包/manifest repin 后）：13661 passed / **7 failed，签名与基线逐条一致（diff=空）**；任务引入失败 0。
- 门禁处置：cli 闭包基线在位再生成（+489 行图边=新模块）；13 个新模块登记 `export-manifest.json`；持久化指纹 compatible repin（guard 0 + tripwire 15/15）。

## 五、固定 50 页大小性能对照（同 seed 20260910 / 同夹具 / 同页大小 50）

| 指标 ms | A（旧路径） | B（目录快路径） | C（+增量） | D（复测） | A→D 变化 |
|---|---|---|---|---|---|
| 1k 冷首页 p50 | 9.27 | 9.82 | 13.59 | 11.19 | +21%（构建开销，阈值内） |
| 1k 热页 p50 | 7.72 | 0.70 | 0.84 | 0.98 | **−87%** |
| 10k 冷首页 p50 | 79.08 | 73.00 | 94.55 | 93.20 | +18%（阈值 123.84 内；B08 初测 141.5 未达标→优化在案） |
| 10k 热页 p50 | 75.53 | 0.56 | 0.65 | 0.67 | **−99.1%** |
| 1k 完整翻页 wall | 176.6 | 30 | 31.0 | 36.8 | **−79%** |
| 10k 完整翻页 wall | 15,341 | 209 | 239.3 | 254.7 | **−98.3%** |
| 热页 fullFileReadCalls | 2 | 0 | 0 | 0 | §7.1 达成 |
| 热页 jsonlParseCount（1k/10k 同值） | 4004/40004 | 103 | 103 | 103 | O(K) 达成 |
| 10k/1k 全翻比 | 86.9 | 7.0 | 7.7 | 6.9 | ≤15 达成 |

增量（C05 口径，D 未回归项目）：1k/10k 四批次增量命中 8/8；parsedRecords=101/99/150/51（两规模一致）；开放 Run 离散度 1.43×/1.37×。

压力边界样本（D01，真实路由，冷构建+3 热页）：big-tool-result（单条 200KiB 工具结果）冷 1.97ms、驻留 0.06MiB；many-customs 2.78/0.15；multi-run（200 Run）2.51/0.22；many-discarded（600 物理/300 弃支）2.45/0.18；large-output（每条 4KiB）4.31/0.10；热页解析均 50/51/21（O(窗口)）、零整文件读、零回退。

## 六、生产接线与未迁移清单

- 已接线：`/sessions/messages`（普通分页 all=0）→ `readSessionHistoryPage`（`server/history-read/index.ts`），运行时实例私有 `HistoryDirectoryCache`（engine.historyReadCache 可注入观测）。
- 未迁移（保留原因）：`all=1` 强制全量返回（一次性全量投影，无页间复用收益，沿用旧路径）；find 路由仅复用同源页边界 `resolveHistoryPageBounds`（re-export）；reconciliation/content(deferred) 解析保持按页稀疏读取、未引入目录正文驻留；Y 项与协议层（E）未开始。

## 七、检查点与红线核对（D04）

- 无他人改动被回退（A01 快照干净；当前 tracked 改动 12 文件 + 未跟踪 34 文件全部归属本任务，含 3 个 repin 类门禁产物与 manifest 登记）。
- 无第二套 projector/正文缓存/每页隐式 SessionManager.open（热页 branchOpenCalls=0、getBranchCalls=0 硬断言）；无 SDK patch（lib/pi-sdk 零 diff）；无提前 E 实现。
- 日志仅含会话 id/字节量/原因，无正文与敏感输出；[dbg] 行受 `HISTORY_READ_DEBUG=1` 门控；测试证据全部合成数据。
- 本报告不宣称 A—F 全部完成；E（协议层）与 F（端到端汇总）留待后续阶段。
