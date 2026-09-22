# HOTSPOT_REPORT — P07-T01 重测与有限热点选择

日期：2026-09-22｜HEAD：`52dbb45f9`（工作区，未提交）｜环境：darwin 27.0.0 arm64 / Node v24.16.0 / npm 11.13.0 / lockfile sha256 `a9735825…`（与 P00 基线**完全同环境**，配对比较有效）。

## 1. 重测方法与命令

全部命令经 `artifacts/refactor-2026/P07/tools/run-logged.mjs` 记录（command-log.jsonl，CMD 前缀 `P07-T01-*` / `P07-T02-*` / `P07-T03-*` / `P07-T06-*`）。工作负载与 P00 BENCHMARK_PROTOCOL 冻结口径一致；P00 未执行的 W3/W4 实验受限项按协议指定方式（scenario harness 本地 HTTP 供应商替身）补全。

## 2. 重测结果（当前 HEAD vs P00 基线，同环境成对）

| 工作负载 | P00 基线（SHA 92c6646c） | P07 复测（HEAD 52dbb45f9） | bench-compare 判定 | 工作量等价断言 |
|---|---|---|---|---|
| W1 启动（spawn→server-info.json，×10） | median 1479ms / p95 1482ms | median **1301ms** / p95 1302ms，ready 10/10 | **IMPROVED -12.0%** | 同 workload digest（bench-compare 通过） |
| W2 历史 n=1000 冷首页（×3） | p50 11.25ms | p50 10.77ms | 数值可比（-4%） | fullFileReadCalls/请求=0 |
| W2 历史 n=1000 热页（20 样本） | p50 1.108ms / p95 1.357ms | p50 0.894ms / p95 1.208ms | -19%（同量级） | jsonlParse 103/页（恒定） |
| W2 历史 n=10000 冷首页（×3） | p50 84.1ms | p50 72.63ms | -14%（同量级） | 同上 |
| W2 历史 n=10000 热页（200 样本） | p50 0.824ms / p95 0.986ms | p50 0.710ms / p95 0.879ms | -14%（同量级） | 同上 |
| W2 完整翻页 n=10000（201 页） | serverTotal 244.7ms | 220.0ms | -10%（同量级） | 页数/唯一性断言通过 |
| W5 内存：就绪+3s 空闲 | 477,760KB | 475,024KB | 可比 | — |
| W5 内存：200 次 input-draft PUT 后 | 479,504KB（ok 200/200） | 482,544KB（ok 200/200） | 可比（+3MB，波动区） | 200/200 成功 |

结论：前六阶段改造后无任何关键指标退步；启动与历史读取在同环境复测中略有改善（属 P01–P06 累积效果+机器波动区间，**不作为本阶段优化宣称**——本阶段零生产代码改动）。

## 3. W3/W4 补全（P00 实验受限项，backlog P07-1）

### W3 持续流式输出吞吐（分层如实计量）

**Layer 1（server 路由层）**：真实 `createChatRoute` + 真实 `session-stream-store` append/trim + 真实 broadcast 序列化扇出（生产形状合成 engine 事件，接线方式与 tests/chat-route-switching.test.ts 相同）。预热 3 轮后每尺寸 30 样本：

| 事件数 | median 墙钟 | 吞吐 | 事件环 p99 | 结束语义 |
|---|---|---|---|---|
| 200 | 1.72ms | 116,513 ev/s | <0.001ms（无采样） | assistant_segment_end→model_turn_end→assistant_run_end |
| 2,000 | 14.36ms | 141,115 ev/s | <0.001ms | 同上 |
| 20,000 | 222.13ms | 90,124 ev/s | <0.001ms | 同上（超出 maxEvents=5000，真实 trim 路径触发） |

样本：`artifacts/refactor-2026/P07/samples/stream-throughput.json`。

**Layer 2（模型流式解析层）**：真实 Pi agent session（`createAgentSession`）+ 本地 HTTP 供应商替身（`tests/helpers/model-observability-scenario-harness.ts` 的 `startFakeProviderWitness`，真实随机端口 HTTP server）SSE 投递 N delta，预热 1 + 30 样本，输出逐字节等价断言全通过：

| delta 数 | prompt median | 解析吞吐 | 输出等价 |
|---|---|---|---|
| 200 | 3.0ms | 66,180 deltas/s | all-equal |
| 2,000 | 24.3ms | 85,791 deltas/s | all-equal |
| 20,000 | 199.4ms | 100,387 deltas/s | all-equal |

样本：`samples/stream-model-layer.json`。分层口径如实登记：Layer 2 不含 observability 持久化（其开销边界已有 tests/model-observability-e2e-concurrency-perf.test.ts S37 的 100 次 off/on ×5 数量级守卫）；Layer 1/2 之间的 session-coordinator 接线不在进程内重复。

### W4 取消响应时延

| 规格 | 场景 | abort→收口 | 正确性 |
|---|---|---|---|
| 小 | prompt 后 500ms 取消（witness 延迟 15s 响应，10 样本） | median **1.2ms**（p95 1.6ms） | witness 请求计数每轮 +1（取消后无重复外发） |
| 中 | 流中 5s 取消（sse-bytes 长流，6 样本） | median **1.6ms**（p95 4.1ms） | 同上 |
| 大 | 工具执行中取消（真实 spawnAndStream 子进程树+孙进程；独立进程组哨兵） | **5.2ms** 收口 | 孙进程退出、哨兵存活、execution 以错误收口（tests/p02-cancellation-edges.test.ts A06 同形状） |

路由层 `abort` 消息→`assistant_run_end{aborted}` 的语义等价由既有 assistant-run-lifecycle / chat-route-switching 测试族锁定（见 ACCEPTANCE_MAP）。

## 4. CPU 剖析（启动路径，V8 --cpu-prof，SIGTERM 优雅退出落盘）

样本：`samples/cpu-prof/child-startup.cpuprofile`（1.73s 采样窗）。按模块桶聚合（self time）：

| 桶 | 占比 | 含义 |
|---|---|---|
| node:internal（ESM compile/resolve、CJS wrapSafe 等） | **37.5%** | 源码形态模块加载/编译（dev 形态成本；产物 bundle 形态大幅消失，见 STARTUP_CRITICAL_PATH.md） |
| wasm | 11.9% | usearch/native 向量与 Pi SDK 内 wasm |
| (idle) | 10.2% | 等待异步 I/O |
| 文件系统 syscall（open/close/stat/readFileUtf8/realpath/lstat/read） | ~16% | 数据目录播种/探测/迁移读 |
| 应用代码 core+lib+shared 合计 | **<3%** | 业务初始化逻辑本身极轻 |

启动阶段计时（`samples/` 与 `logs/P07-T01-startup-phases-prof.out`，单样本+10 次 W1 批量互证）：spawn→首阶段日志 **1131ms**（模块加载+bind），ensureFirstRun 8ms，身份注册 1ms，engine 构造 22ms，**engine.init 全部业务阶段 65ms**（Pi SDK 23ms / agents 14ms / ResourceLoader 11ms / 模型发现 7ms / 其余 ~10ms），init 后→ready 38ms。

## 5. 热点隔离微基准：stream-store trimEvents

`samples/stream-store-trim-microbench.json`（真实 `appendSessionStreamEvent`，只读生产模块）：

| 场景 | 稳态每 append | 相对未饱和 |
|---|---|---|
| 未饱和（retained 4,000 < cap 5,000） | 0.377µs | 1× |
| 饱和 cap=5,000（默认） | 11.144µs | **29×** |
| 饱和 cap=20,000 | 40.014µs | **106×** |

根因：`server/session-stream-store.ts` `trimEvents` 用 `state.events.splice(0, 1)` 头部移除——数组头部 splice 为 O(n)，饱和后每次 append 都付出 O(maxEvents) 移位。与 W3 Layer 1 中 n=20000 吞吐回落（141k→90k ev/s）互相印证。

## 6. 有限热点清单（本阶段进入项，≤3）

| # | 热点 | 证据 | 决定 |
|---|---|---|---|
| H1 | stream-store trim O(maxEvents)/append | §5 微基准 + W3L1 n=20000 回落 | **登记+延后**：该文件是 P05 锁定的 streamId/seq 恢复协议承载面（P06 交接 §9 明令零触碰）；影响有界（仅 >5,000 事件的异常 turn；最差 40µs/append；20k 事件整轮 222ms，无用户可感卡顿）。最小修复方案已写入 §7 供解锁后执行。本阶段不改。 |
| H2 | 启动前 1.1s 模块加载（源码形态） | §4 CPU 剖析+阶段计时 | **验证结论，不改**：应用业务初始化仅 65ms；产物 bundle 形态 median 1072ms（-18%）已消除大部分编译成本；可延后模块（Bridge/env-deps/清理）已有 setImmediate 延后且被结构测试锁定（见 STARTUP_CRITICAL_PATH.md）。 |
| H3 | 无 | 全部测量面（历史热页 0.7–0.9ms、UI 缓冲 3.2M handle/s、取消 1.2–5.2ms、模型层 66k–100k deltas/s）均远低于任何用户可感阈值 | **无第三个安全热点**，不制造改造需求。 |

## 7. 后续建议（不进入本阶段）

1. **H1 修复方案（需 P05 锁定面解锁授权）**：`trimEvents` 改为头偏移游标（head index）+ 周期性一次 splice 压实，保持精确保留窗口（恰为 maxEvents）、seq 连续性与 reset/truncated 判定不变；配等价测试（同保留数/同 seq/同恢复判定）+ 复跑本微基准验证回到 ~0.4µs/append。
2. W3 Layer 1 的双事件发布（text_delta 原始 + assistant_segment_delta 规范化）是 P05 消息语义裁决面的设计行为（兼容并存），如未来退出兼容可减少约一半 ws 帧——属 P08 旧路径退出决策范畴。
3. 源码形态启动（dev 工作流）可考虑 Node compile cache（`NODE_COMPILE_CACHE`），属开发体验优化，不影响产品，未纳入。

## 8. 工具与样本索引

- 工具：`artifacts/refactor-2026/P07/tools/{bench-stream-throughput,bench-stream-model-layer,bench-stream-store-trim,bench-startup-phases,bench-startup-bundle,bench-soak-resources,bench-memory,run-logged}.mjs`（前 5 个为本阶段新建；bench-memory 为 P00 工具的 P07 输出路径副本，口径不变；run-logged 为 P00 工具的 stage_id=P07 副本）。
- 样本：`artifacts/refactor-2026/P07/samples/{startup-p07-b1,history-p07/,history-p07-phaseB/,memory-p07.json,stream-throughput.json,stream-model-layer.json,stream-store-trim-microbench.json,soak-resources.json,startup-bundle.json,startup-stdout.log,cpu-prof/}`。
