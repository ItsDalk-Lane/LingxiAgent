# P07_PERFORMANCE_COMPARISON — 配对基准与功能总回归

日期：2026-09-22｜比较方法：P00 BENCHMARK_PROTOCOL 冻结口径（同机/同 Node/同 lockfile/同输入，预承诺 10% 阈值，正确性零容忍先行）。

**前置声明：本阶段生产代码零改动。** 全部比较是「当前 HEAD（P01–P06 累积后）vs P00 基线（重构前 92c6646c）」的同环境复测对照，以及本阶段新建的 W3/W4/soak 首轮测量（无"前版本"可配对——P00 时点这些项实验受限未执行）。因此本报告**不包含任何由 P07 代码改动带来的优化宣称**；所有 IMPROVED 判定归因于 P01–P06 已验收改动+同机波动，且均通过工作量等价断言。

## 1. 环境与等价性

| 项 | P00 基线 | P07 复测 |
|---|---|---|
| Node / OS / arch | v24.16.0 / darwin 27.0.0 / arm64 | **完全相同** |
| lockfile sha256 | `a9735825…` | **相同（未变）** |
| 输入 | W1 空 HOME+固定入口；W2 seed=20260910 v3 夹具（sha256 逐字节核对一致）；W5 合成 PUT | 相同（fixture audit 交叉核对通过） |
| 功能开关 | 默认 | 默认（无任何开关变化） |

比较有效性由 bench-compare workload digest + 各基准内置工作量硬断言保证（fullFileReadCalls=0、jsonlParse 恒定、输出等价 all-equal、witness 请求计数）。

## 2. 配对结果汇总（全量样本见 samples/，判定见 §5 命令表）

| # | 指标 | P00 | P07 | Δ | 预承诺判定 |
|---|---|---|---|---|---|
| 1 | W1 启动 median（×10） | 1479ms | 1301ms | -12.0% | **IMPROVED**（正确性：ready 10/10、同 workload digest；typecheck/lint/全量测试绿） |
| 2 | W1 启动 p95 | 1482ms | 1302ms | -12.1% | 同上 |
| 3 | W2 冷首页 n=1000（×3） | 11.25ms | 10.77ms | -4.3% | COMPARABLE |
| 4 | W2 冷首页 n=10000（×3） | 84.1ms | 72.6ms | -13.7% | 同量级改善（冷启动样本 n=3，标注高分位稳定性有限） |
| 5 | W2 热页 n=1000 p50/p95 | 1.108/1.357ms | 0.894/1.208ms | -19%/-11% | 同量级改善 |
| 6 | W2 热页 n=10000 p50/p95 | 0.824/0.986ms | 0.710/0.879ms | -14%/-11% | 同量级改善 |
| 7 | W2 完整翻页 n=10000 | 244.7ms | 220.0ms | -10.1% | COMPARABLE 边界 |
| 8 | W5 空闲 RSS | 477,760KB | 475,024KB | -0.6% | COMPARABLE |
| 9 | W5 200 PUT 后 RSS | 479,504KB | 482,544KB | +0.6% | COMPARABLE |

**退步项：无。**（第 9 项 +3MB 在单样本内存波动区内，非持续劣化；soak 12 批轨迹 rss +0.3%/-1.0% 两次运行互证无单调增长。）

## 3. 本阶段新建测量（无历史基线可配对，如实登记为首轮）

| 指标 | 结果 | 样本量 |
|---|---|---|
| W3 L1 路由层吞吐 | 116k/138k/89k ev/s（200/2k/20k） | 每尺寸 30（预热 3） |
| W3 L2 模型解析吞吐 | 66k/86k/100k deltas/s，输出 all-equal | 每尺寸 30（预热 1） |
| W4 取消 S1/S2/S3 | 1.2 / 1.6 / 5.2ms | 10 / 6 / 1（S3 为单次进程树实测，正确性断言全过） |
| UI 50k delta | 15.6ms 突发、3.2M handle/s、flush=2、探针 8.7ms | 1（测试内，断言含宽松上界锚点） |
| soak 12 批 | rss +0.3%、heap -2.8%（第二次运行）；-1.0%/-19.1%（第一次） | 2 次全量 12 批 |
| bundle 形态启动 | 1072ms / 1041ms（两次 ×10） | 20 |
| trim 微基准 | 0.38µs（未饱和）→ 11.1µs（cap 5k）→ 40µs（cap 20k） | 每 scenario 单轮 20k append |

## 4. 语义/权限/预算等价检查（先于性能比较，任务书 T07.2）

- 工具面/权限：P03–P06 门禁全部复跑绿（core-contracts、tool-invocation-boundaries、dependency-boundaries、lint-open-boundary）。
- 消息语义/流恢复/历史投影/资源授权（P05 锁定面）：零触碰（git diff 为空于这些文件）；定向套件（session-stream-store、history-read-directory-×3、assistant-run-lifecycle 族所在全量测试）绿。
- canonical 装配/常驻文案/校验反馈（P06 锁定面）：零触碰；p06 4 文件定向复跑绿；预算账本（A/B/C ≤ 基线，D 2420 tok）未变。
- W3L2 输出逐字节等价、W2 页数/唯一性断言、soak 任务量等价断言全部通过。
- 取消零副作用：W4 三规格 witness 外发计数零增量、子进程树退出、哨兵存活。

## 5. 命令与证据索引（全部 exit 0 除注明）

| command_id | 内容 | 结果 |
|---|---|---|
| P07-T00-env | 环境取证 | 0 |
| P07-T01-bench-startup | W1 复测 ×10 | 0（IMPROVED -12%） |
| P07-T01-bench-history | W2 phase A 复测 | 0 |
| P07-T03-bench-history-phaseB | W2 phase B 目录路径 | 0 |
| P07-T01-bench-memory / -r2 | W5 复测（首次 Bearer token 不匹配 401 → 修复重跑，双留档） | 1 → 0 |
| P07-T01-bench-w3-layer1 / -r2 | W3 L1 | 0 / 0 |
| P07-T01-bench-w3l2-w4 / -r2 / -r3 | W3 L2 + W4（r2 因 node:abort-controller 内建不存在失败留档；r3 绿） | 0 / 1 / 0 |
| P07-T01-bench-trim-micro / -r2 | trim 微基准 | 0 / 0 |
| P07-T01-startup-phases-prof / -r2 / -r3 | 启动阶段计时+CPU 剖析（r3：修复 --out-dir 缺省解析 bug 后复跑，profile 落位 samples/cpu-prof/；阶段计时三次互证 1266–1437ms） | 0 / 0 / 0 |
| P07-T02-build-server | server bundle 重建（seed 签名步骤无凭据 exit 1，bundle/index.js 已产出可用于计时——按设计拒绝未签名 seed） | 1（部分成功，如实登记） |
| P07-T02-bench-startup-bundle / -r2 | bundle 启动 ×10 | 0 / 0 |
| P07-T02-lazy-init / P07-T07-lazy-init-rerun | A03 行为测试 | 0 / 0 |
| P07-T04-ui-wallclock | UI wall-clock 测试 | 0 |
| P07-T06-bench-soak / -r2 | soak 12 批 | 0 / 0 |
| P07-T07-typecheck / -r2 | tsc ×3（首次 1 处测试类型错 → 修复重跑） | 1 → 0 |
| P07-T07-lint / -r2 | eslint .（首次 P07 工具 18 err → 修复重跑） | 1 → 0 |
| P07-T07-build-renderer | vite build renderer | 0 |
| P07-T07-bench-compare-selftest | 假优化拒绝自证 | 0 |
| P07-T07-directed-suites | 15 文件 113 例定向回归 | 0 |
| P07-T07-core-contracts | 四项边界门禁 | 0 |
| P07-T07-full-test | 全量 npm test | 见 P07_REPORT §验证（F1 核对+patch 还原） |

## 6. 场景判定（任务书 §6，对应 ACCEPTANCE_MAP.json）

- **A11 安全不能提速牺牲**：本阶段不存在"临时关闭校验的假优化分支"——零生产改动使该分支不可达（N/A 有据：无任何代码分支）；防范机制已验证：bench-compare --selftest 判假优化 INVALID（少处理数据禁比），且所有性能样本先过工作量等价断言。若未来出现该分支，自测证明会被拒。
- **A12 性能退步**：§2 汇总包含全部指标（含 +0.6% 的 W5 活动内存，如实列出）；无超过 10% 阈值的可复现退步，无需回退。

## 7. 交付 P08 的性能工作负载

P08 直接可复用（全部合成输入、固定种子、命令即重放）：
1. `bench-startup.mjs`（源码形态）/ `bench-startup-bundle.mjs`（产物形态，需先 build:server+签名凭据）；
2. `benchmark-history-read-directory.mjs --phase A/B`；
3. `bench-stream-throughput.mjs`、`bench-stream-model-layer.mjs`（W3/W4）；
4. `bench-soak-resources.mjs`（长运行资源）；
5. `npm run benchmark:terminal-ui`（终端面，既有）。
