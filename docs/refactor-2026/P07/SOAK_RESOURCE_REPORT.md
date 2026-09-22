# SOAK_RESOURCE_REPORT — P07-T06 缓存、监听与长运行资源回收

日期：2026-09-22｜结论：**无可复现的无界增长；资源上限与失效行为可证**（生产代码零改动，UNCHANGED_VERIFIED + soak 基准新建）。

## 1. soak 基准（固定负载 12 批循环）

命令：`P07-T06-bench-soak`（`node --expose-gc bench-soak-resources.mjs --batches 12`，exit 0）。样本：`samples/soak-resources.json`。

每批驱动真实组件完整生命周期：

| 组件 | 每批负载 | 断言（全部通过） |
|---|---|---|
| session-stream-store（ring buffer） | 20 会话 ×（begin→7,000 事件 append→resume 校验→finish） | retained 恒 ≤5,000（cap）；饱和后 resume `truncated=true` 且 replay 窗口=保留窗口；**finishSessionStream 后 events=0、totalEventBytes=0**（turn 结束即清场） |
| TerminalSessionManager + terminal-ws-bridge（真实实现，fake backend 与 scripts/benchmark-terminal-ui.mjs 同构） | 12 终端 start/64KB 输出/exit/dispose | 磁盘（lingxiHome/agents）有界（1.45MB 稳态）；无句柄累积（见 §2 active_resources） |
| HistoryDirectoryCache（真实预算） | 12 会话目录构建/热路径二次访问/驻留文件外部追加→probe 判非 valid | sessions 恒 ≤8（cap）；residentBytes ≪64MB；**每批 hits=6（热路径命中）、rebuilds=12（12 会话>8 槽 LRU 颠簸：主循环每批全量重建）、外部追加后 probe 判非 valid 1/1（实际判定 `append_candidate`：目录保留、非 invalidate）**；全 12 批 invalidations={}（样本 cache_final_stats） |

## 2. 进程级稳态（GC 前轨迹为准；GC 后读数单列不作为判定依据）

| 指标 | 预热后 3–5 批均值 | 最后 3 批均值 | 增幅 |
|---|---|---|---|
| rss | 336,587KB | 333,168KB | **-1.0%** |
| heapUsed | 105,263KB | 85,167KB | -19.1%（稳态反而下降） |
| external | 12,108KB | 12,551KB | +3.7%（波动区） |
| 逐批单调递增 >5% | — | — | **否** |

`active_resources` 每批恒为 {CloseReq:1, PipeWrap:2}（无定时器/句柄泄漏累积）。**判定：NO_REPRODUCIBLE_UNBOUNDED_GROWTH。**

## 3. 各缓存/队列上限与淘汰后重建（任务书 T06.3 清单逐项）

| 项 | 上限 | 淘汰后行为 | 证据 |
|---|---|---|---|
| ring buffer（session-stream-store） | maxEvents 5,000 / maxBytes 8MB / 单事件 256KB（超限 compact/omit） | trim 丢弃最旧；turn 结束全清 | soak §1 + tests/session-stream-store.test.ts |
| 历史目录缓存 | 8 会话 / 64MB 驻留 / 16MB 单目录 / 2 并发构建 | 淘汰即 released()；no-cache 会话走无缓存只读（不截断历史） | soak §1 + tests/history-read-directory-memory.test.ts（released 全触发、stats 归零） |
| ws 会话状态表（chat 路由） | MAX_SESSION_STATES（LRU，仅淘汰非流式最久未访问） | 重建初始状态 | `server/routes/chat.ts:578-598`（代码核对） |
| 工具描述缓存 | **不存在独立模块**：常驻 4 工具 schema 由 canonical 装配单一权威产出（P06 锁定面，D 组分 2420 tok），无独立可膨胀缓存 | N/A 有据（grep 无同职责实现；PROMPT_BUDGET_REPORT 锁常驻面 ≤ 基线） |
| 图像/媒体缓存 | **不存在进程内图像缓存模块**：媒体任务落盘（task-store fork 清理），缩略图按 30s 定时刷新仅活跃 browser 会话 | N/A 有据（browser 缩略图定时器在无活跃会话时自停：`server/routes/chat.ts:738-774`） |
| 任务队列 | TaskRegistry 定时器 MAX_TIMER_DELAY 上限；媒体 poller MAX_CONSECUTIVE_ERRORS=5 自停 | 代码核对（`lib/task-registry.ts`、`core/media/poller.ts:26`） |

## 4. 同业务负载多批次等价

soak 每批负载恒定（20 会话/12 终端/12 目录），全部断言逐批通过=任务数与结果等价；未把"后台任务未完成时的低内存"当优化（每批同步等待收口后才采样）。

## 5. 已有测试面（全量回归内复跑）

tests/history-read-directory-memory.test.ts（释放/预算/8 会话轮换）、tests/agent-executor-teardown.test.ts、tests/session-stream-store.test.ts、scripts/benchmark-terminal-ui.mjs 场景（resourceDelta 口径）。复跑结果见 P07_REPORT §验证。

## 6. 回退

零生产代码改动；soak 工具与样本独立可回退。

## 7. P07-FIXR1 更正（2026-09-22，独立验收修复轮）

来源 = [P07_ACCEPTANCE_REVIEW.md](P07_ACCEPTANCE_REVIEW.md) §4 发现 F-A/F-C（低严重度，工具与叙述层面）。本节更正上文中两处不准确叙述，前轮测量数据本身有效。

- **F-A（临时目录泄漏，本报告未直接涉及但同源）**：soak 工具原版 `process.exit()` 位于 try 块内，跳过 finally 的 `rmSync`——每次运行（含成功）泄漏 ≈12MB 的 `hana-p07-soak-*` 临时目录。已改为 `process.exitCode` 赋值 + 自然退出（退出码语义不变）。
- **F-C（§1 组件 3 机制叙述 + 断言）**：原文「驻留文件追加→probe 失效」「probe 失效 1/1」「invalidations directory_invalid 生效」不准确。以验收 CE-2 实测与生产源码（`server/history-read/cache.ts` probe、`types.ts` HistoryProbeVerdict、`index.ts` tryDirectoryOnce）为准：
  - 外部追加（未走插桩写入，mutation epoch 不变）的 probe 判定为 **`append_candidate`**（目录保留、非 invalidate）；读路径仅在 `verdict === "valid"` 时直接命中，其余判定走增量/全量重建，不会返回追加前旧快照。验收 CE-2 实测四类变更（追加/同长度重写/截断/原子替换）判定分别为 `append_candidate`/`untrusted_mutation`/`snapshot_changed`/`file_identity_changed`，全部正确。
  - 每批 `rebuilds=12` 由 **12 会话 > 8 槽的 LRU 颠簸**驱动（主循环按 0→11 顺序访问，被淘汰的 LRU 头总是恰为下一个待访问的会话 → 每批主循环 0 命中、全量重建；热路径 6 命中来自批尾二次访问），与追加无关。
  - 全 12 批 `cache_final_stats.invalidations = {}`——原「invalidations directory_invalid 生效」与样本不符，废弃。
  - 工具断言同步修复：`verdict !== "fresh"` 恒真（判定集中不存在 "fresh"），已改为「追加后 probe 不得判 `valid`，否则计失败」（`probe_append_detected`，样本字段 `probe_verdict_after_append` 留档实际判定）。
- 复核（run-logged，exit code 见 command-log.jsonl `P07-FIXR1-*`）：`P07-FIXR1-tmpdir-before/after/final`（运行前后 `hana-p07-soak-*` 均为 0，泄漏已消除）、`P07-FIXR1-soak-minimal`（6 批，exit 0）与 `P07-FIXR1-soak-original`（12 批原始规格，exit 0；`components_ok=true`、`NO_REPRODUCIBLE_UNBOUNDED_GROWTH`、rss_growth 0.3% 与原样本一致、逐批 rebuilds/hits/evictions 逐位一致）——判定逻辑不受修复影响；`P07-FIXR1-negprobe-inject-run`（断言有效性负向注入：禁用追加后 probe 判 `valid` → 断言变红、exit 1，稳态判定不受影响）+ `P07-FIXR1-negprobe-verify`（exit 0）+ `P07-FIXR1-lint`（exit 0）。
