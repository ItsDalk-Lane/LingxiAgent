# NEXT_STAGE_HANDOFF — P08 输入（P07 → P08）

日期：2026-09-22｜P07 结果：见 P07_RESULT.json（PASS_WITH_BLOCKED_ITEMS：12/12 场景 PASS；**本阶段生产代码零改动**；BLOCKED 仅继承项——真供应商冒烟与真模型行为评测，均待凭证/费用授权）。

## 1. 已验收坐标与环境

- 工作区 START = END 候选 = `52dbb45f9`（P06 提交后；零生产 commit；工作区改动 = 2 个新测试文件 + docs/artifacts refactor-2026/P07/ 证据目录，全部未跟踪，等待编排层统一提交）
- 分支 `docs/knowledge-closeout-2026-09-21`（未切换）；Node v24.16.0 / npm 11.13.0 / darwin arm64；lockfile sha256 `a9735825…`（未变）
- 全量 npm test：**4 红 = F1 已知基线同一组**（逐名与 P05/P06 一致；14797 绿 / 1 expected fail / 15 skipped；较 P06 +5 绿 = 本阶段新增测试）；f1-f12 patch 重写后已还原（哈希复核通过）

## 2. P07 建立的资产（P08 消费面）

| 资产 | 对 P08 的用途 |
|---|---|
| docs/refactor-2026/P07/P07_PERFORMANCE_COMPARISON.md §7 | **P08 产物验收的性能工作负载清单**（startup 源码/产物双形态、history A/B、W3 两层、W4、soak——全部命令即重放、合成输入） |
| artifacts/refactor-2026/P07/tools/bench-*.mjs（8 个） | 上表对应工具；bench-startup-bundle 需先 build:server（注意：seed 签名步骤需 LINGXI_SIGN_KEY，属发布凭据） |
| tests/p07-startup-lazy-init.test.ts（3 例） | A03 锚点：startBridgeManager single-flight/error/once（改 server/index.ts 该函数须同步） |
| desktop/.../p07-stream-buffer-wallclock.test.ts（2 例） | A06/A07 锚点：流合并窗口终态不丢 + 50k delta wall-clock（宽松上界，防数量级回归） |
| docs/refactor-2026/P07/HOTSPOT_REPORT.md | H1 延后热点完整证据与最小修复方案（见 §4） |
| 12/12 ACCEPTANCE_MAP.json | 性能面验收坐标（P08 全产品回归时可直接引用） |

## 3. 本阶段门禁（P08 不得削弱）

1. P01–P06 全部门禁延续绿（本轮实测：typecheck×3 / lint（eslint . 全绿，含 P07 工具）/ build:renderer / core-contracts + 三边界检查 / 定向 15 文件 113 例 / 全量 npm test F1 未扩大）。
2. P05/P06 锁定面继续零触碰（本阶段实测零 diff）；P08 同样不得以性能/退出名义绕过。
3. 性能预承诺规则（BENCHMARK_PROTOCOL 10% 阈值、正确性零容忍、工作量等价断言）继续有效；bench-compare --selftest 为 A11 常备自证。

## 4. H1 延后热点（如 P08 决定处理，需显式解锁授权）

`server/session-stream-store.ts` `trimEvents` 的 `splice(0,1)` 头部移除在 ring 饱和后为 O(maxEvents)/append（实测 0.38µs→11.1µs@cap5000→40µs@cap20000；W3L1 n=20000 吞吐 141k→90k ev/s）。该文件是 P05 锁定的 streamId/seq 恢复协议承载面。最小修复方案（HOTSPOT_REPORT §7.1）：头偏移游标 + 周期性一次 splice 压实，保持精确保留窗口/seq 连续性/reset/truncated 判定不变 + 等价测试 + 复跑微基准。**授权前不改。**

## 5. 已执行验证与遗留

- 已绿：见 P07_REPORT §验证（含 5 条首败-重跑链全部留档）。
- BLOCKED 继承：P04-T07-2 真供应商冒烟、P06/P07 真模型行为评测（无凭证/费用授权）。
- F1 封印推进与 F3 四平台 CI 证据取回：归 P08 主责（PROGRESS.md 流程 + gh/proxy 环境修复）。
- 遗留登记（本阶段未合法触碰，保留原样）：P04 NEXT_STAGE_HANDOFF §2 措辞项；P06 RESULT/ACCEPTANCE_MAP fix_rounds 计数瑕疵（可观察事实：P06 command-log 6 条 P06-FIXR1-* 命令 vs RESULT verification 列 7 项）；R10-09 顺序依赖波动（P04 起已知）。
- 本阶段限制（如实）：soak 未含真实浏览器开合循环（无 GUI 授权）；W4-S2 6 样本/S3 单样本；Electron 窗口级启动未测（W1 终点=server-info.json）。

## 6. 必保留兼容（P08 不可改变）

- P02–P06 交接全部条目继续有效。
- 本阶段新增：性能工作负载口径（W1 终点定义、W3 分层、W4 三规格、soak 判定规则）不得在 P08 单方面改动（BENCHMARK_PROTOCOL 变更须重新基线）。

## 7. 当前数据版本

- 零 schema 变更、零迁移（P05 DATA_COMPATIBILITY §1 全表继续有效）；指纹守卫未触碰。

## 8. 工作区卫生提醒（继承+新增）

- 全量 npm test 后必须复核 `artifacts/f1-f12-repair/round2/patches/89bc0b64-to-r01-r10-source.patch` 未被重写（本轮已还原，哈希 25fb315f…）。
- 本轮未跟踪：docs/refactor-2026/P07/、artifacts/refactor-2026/P07/、tests/p07-startup-lazy-init.test.ts、desktop/src/react/__tests__/chat-performance/p07-stream-buffer-wallclock.test.ts；零 tracked 文件修改（git status 仅上述未跟踪项）。
- EVIDENCE_SHA256.txt 为证据链最终步：任何日志追加后须重新生成。

## 9. 下一阶段唯一允许修改范围

P08（旧路径退出、全产品回归与发布准备）：按 P08 任务书执行旧路径退出决策、四平台产物验收与发布准备；性能面复用 §2 工作负载做产物形态对照（注意 build:server 签名凭据依赖）；不得以性能理由触碰 §3 门禁与 §4 之外新增热点修改（H1 需显式授权）。
