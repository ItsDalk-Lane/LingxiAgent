# R07 收口反向验证证据（negative-verification-closeout-1，2026-10-20）

对应任务书第五节「增加隔离的反向验证：故意制造路由漏项、空/失败证据或
旧内核调用，证明对应检查变红；恢复后变绿」。三组红→绿均有可复核产物；
未篡改任何正式历史证据。

## 1. 路由漏项 → A23 矩阵变红（matrix-red/）

- 红：修复多行 `.route(` 扫描后首轮运行，`R07_BIDIRECTIONAL_MAP_RED_82_unmapped.json`
  是当时的输出——409 条路由中 82 条 lib.rs 主干路由无需求归因，
  `scripts/rust-tauri/r07_behavior_matrix.py` 以退出码 1 拒绝（新增的
  unmapped≠0 非零退出与「路由清单 < 300 即扫描器回归」自检同时生效）。
  旧单行扫描器只识别 141 条（复算：`\.route\("` 141 vs `\.route\(\s*"` 409），
  这就是审查确认的「246 条显式路由中 204 条不在 A23 清单」的机器根源。
- 绿：按 handler 实现模块补齐 23 组前缀归因后，409 路由 0 unmapped、
  退出码 0（同一脚本当前源码直接复跑可复现）。

## 2. 空/失败证据 → 场景锚点变红（anchor-red/）

`xtask_anchor_tests.txt`：127 项 xtask 单元测试全绿，其中三项是本轮新增
的锚点机制正反向测试（verify/runner_tests.rs）：

- `suite_test_anchor_hit_keeps_scenario_pass`：日志含 `test X ... ok` → 场景 PASS。
- `suite_test_anchor_miss_fails_even_when_command_exits_zero`（红）：套件退出 0
  但日志零匹配关键测试 → 场景 FAIL、observed=absent、overall FAIL。
- `suite_test_anchor_failed_test_line_is_not_a_pass`（红）：关键测试出现但
  `... FAILED` → 场景 FAIL、observed=not_ok。

24 个基础场景已在 R07.json 逐场景登记 39 条 `suiteTestAnchors`（stage_map
解析强制锚点 command ∈ 场景 commandRefs）。

## 3. 旧内核调用 → A24 检测变红（detector-red/）

`t12_closure_with_detector_selftest.log`：r07_t12_closure 套件 3/3。
其中 `r07_t12_kernel_detector_catches_a_real_node_process`（红侧自证）故意
spawn 一个真实 node 子进程，证明检测器能抓到（否则 A24 的「无旧内核」
断言只是空转）；`r07_t12_a24_closed_loop_without_legacy_kernel`（绿侧）
在六能力域闭环后用同一检测逻辑断言进程树无 node，且出站 HTTP 全部落在
供应商替身 `/v1/` 前缀内。

## 附注

A23 运行时全表探针（409 路由 TRACE 405 / 无凭证 GET 401）在
`r07_t12_closure` 套件内执行；本轮新扫描出的 3 条 token 承载授权路由
（preview/html、bridge/media）按 classify_route 的 Public 分支逐字镜像进
豁免清单——404 防存在性探测是这些面的既定语义，不是漏检。
