# G03-E01 普通自查（F04 逐 C-ID）

执行代理自检口径。全部命令在 `/Users/study_superior/Desktop/Code/LingxiAgent/rust` 下以 `~/.cargo/bin/cargo … --locked` 执行。证据根：`artifacts/rust-tauri/R03/repair-current/G03-E01/`。

## R03-FIX-F04-C01｜副作用后 panic 保留 Unknown — PASS

- **用例**：`tool_receipt_unknown.rs::f04_c01_panic_after_side_effect_journals_unknown_not_confirmed_failure`。
- **前置**：受控外部计数器（文件持久 `requests.log`，`executed=true` 行计数）初始 0。
- **操作**：Tool 替身对计数器 +1（durable 落盘后才返回）后 panic；经真实 Supervisor（真实 ServiceState + SQLite 存储链 + 真实 journal 写序）执行，provider 第二回合 Final 使 run 正常收口；读 journal 与恢复分类。
- **观测**（`normal-selfcheck/evidence.json#f04_c01_panic_after`）：
  - 外部计数 = **1**（副作用确实发生）；
  - journal phase = **unknown**，receipt outcome = **unknown**、`dispatched=true`（执行确已派发）、dedup_id 无（不臆造外部回执）；
  - receipt detail 含 panic payload 与 "the external outcome is unobserved"（错误来源可诊断；error 日志同时带 `exit=panicked`）；
  - 幂等键 `{run}-tc0001` 保留在 journal 条目；
  - 恢复分类 `Unknown`，保守能力下决策 `needs_attention`（非 ConfirmedFailed/ConfirmedSettled）；
  - run 终态 `completed/with_final`——**run 已结束不掩盖调用结果不明确**（红线 4 当前进程面）；`tool_call_completed` 事件 payload `"status":"unknown"`（契约 §5 词汇）。
- **红侧**：同一用例在未修复代码 6 红之一（`got Failed`，`adversarial-selfcheck/red-baseline-tool_receipt_unknown-final.log`）。
- **命令**：`cargo test -p lingxi-service --locked --test tool_receipt_unknown`（exit 0；单测过滤 `f04_c01_panic_after`）。

## R03-FIX-F04-C02｜派发后无结果各类异常一致 — PASS

- **用例**：`tool_receipt_unknown.rs::f04_c02_dispatched_no_result_variants_classify_consistently_unknown` + 单元 `runs::tests::every_unobserved_tool_exit_variant_journals_unknown_diagnosably`。
- **前置**：两个已派发等待的调用，外部是否完成不可确定。
- **操作/观测**：
  - 集成：同一 run 内 `lost.after`（先外部成功再丢响应：计数 1、响应丢失）与 `lost.before`（外部未动作：计数 0）——**两态 journal 均 unknown**、`dispatched=true`、无 dedup_id；外部总执行恰 1 次（`evidence.json#f04_c02_classification_table` 分类表：target / phase / outcome / dispatched / detail）。
  - 单元（`normal-selfcheck/unit-taskexit-variants.log`）：`unobserved_tool_exit_reason` 对 `Panicked/Aborted/Failed/Completed` 四个真实 TaskExit 变体逐一产出含 "unobserved" 与各自异常类标记的 reason——中止、通道丢失、内部异常不因异常种类漏分类（该分支是唯一 `Err(task_exit)` 入口，不按变体分岔）。
- **红侧**：集成用例旧码红（entry 0 `got Failed`）。
- **命令**：同上（exit 0）；`cargo test -p lingxi-service --locked --lib runs::tests::every_unobserved_tool_exit_variant`（exit 0，1 passed）。

## R03-FIX-F04-C03｜可信负面事实不丢失 — PASS

- **用例**：`tool_receipt_unknown.rs::f04_c03_trusted_negatives_survive_alongside_the_unknown`。
- **前置**：一例授权前拒绝（真实审批门拒绝 `reject.me`）；一例外部明确失败回执（`fail.external` 真实执行后返回结构化失败）；一例无结果 panic（`panic.fault`）。
- **操作/观测**（`evidence.json#f04_c03_three_classes`）：
  - tc0001 拒绝：phase failed、`dispatched=false`、detail "not dispatched: …"——**零派发由外部系统佐证**（`request_count=2`，reject.me 从未到达 executor；拒绝来自 wiring 的审批边界，不是适配器谎报）；
  - tc0002 真实外部失败：phase failed、`dispatched=true`、detail `upstream_unavailable…`，恢复决策 `ConfirmedSettled{Failed}`——**修复没有把真实失败扫进 Unknown**；
  - tc0003 panic：phase unknown、`dispatched=true`、决策 `NeedsAttention`——与前两者可区分。
  - 三类事实（已知未派发 / 可信外部失败 / 运行器失败无外部结果）在同一 journal 中并存且互不混淆（红线 1）。
- **红侧**：旧码 tc0003 红（`got Failed`）；tc0001/tc0002 两断言旧码即绿（钉，防止过度修正）。
- **命令**：同上（exit 0）。

## R03-FIX-F04-C04｜恢复及重复恢复不重做 Unknown — PASS

- **用例**：`tool_receipt_unknown.rs::f04_c04_recovery_and_repeat_recovery_do_not_redo_unknown`。
- **前置**：留下已加 1 但本地 Unknown 的调用（panic-after 回执 unknown 已提交；provider 第二回合停泊；abort drive 留下 dangling-active run 行）。
- **操作**：重启（生产形态 bootstrap）恢复扫描，随后重复恢复两次（直接 journal pass + 第三次 bootstrap 扫描）。
- **观测**（`evidence.json#f04_c04_repeat_recovery`）：
  - 扫描 1：`scanned=1`，类别 **interrupted_needs_attention**（unknown 副作用主导；user_reason 含 "UNKNOWN outcomes"），`unknown_verdicts_persisted=0`（in-process 回执已标 unknown，扫描不二次写）；外部计数仍 **1**；
  - 扫描 2（直接 pass）：`unknown_verdict_persisted=false`、class Unknown、NeedsAttention——幂等重放；
  - 扫描 3（再 bootstrap）：`scanned=0`（终态不复活），外部计数仍 1；journal phase 最终 unknown、可见。
- **红侧**：旧码类别红（`recoverable_wait`——ConfirmedSettled 丢失不确定性，恰为审查影响陈述）。
- **命令**：同上（exit 0）。

## 回归总表

| 项 | 结果 |
|---|---|
| `cargo test --workspace --locked` | 67 suites / 673 passed / 0 failed（底线 66/666/0） |
| `cargo fmt --all -- --check` | 零 diff |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 零告警 |
| G01 套件（cancel_link_inheritance 7 / subagent_closeout 8 / cancellation_tree 8） | 全绿 |
| G02 套件（cancel_terminal_race 13） | 全绿 |
| 相邻套件（invocation_journal 3 / late_result_fence 5 / recovery_startup_scan 7 / recovery_crash_points 2 / subagent_permission_inheritance 4 / subagent_lifecycle 3 / r03_t08_acceptance_matrix 17） | 全绿（`normal-selfcheck/adjacent-suites.log`） |
| `xtask check-contracts` / `check-boundaries` | exit 0（626 entries 零漂移） |
| `Cargo.lock` | sha1 `3b659f41…` 不变 |
