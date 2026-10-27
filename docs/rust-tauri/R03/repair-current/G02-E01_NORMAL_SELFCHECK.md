# G02-E01 普通自查（逐 C-ID）— F03 取消/终态统一竞争裁决

命令前缀一律 `~/.cargo/bin/cargo`，`--manifest-path rust/Cargo.toml`，全部 `--locked`。
证据根：`artifacts/rust-tauri/R03/repair-current/G02-E01/normal-selfcheck/`。

## 测试面说明

- 集成套件 `lingxi-service --test cancel_terminal_race`（13 测试）走**真实**组合：真实 SQLite `RunDatabase`（`GatedStorage` 直通装饰、全部 13 个 StoragePort 方法委托，仅泊车点先信号后等待）、真实事件服务、真实内核状态机与单事务 finalize、真实会话面（`execute_submission_for` / `cancel_run_for`）、真实取消树。泊车点均在真实持久化边界（`record_run_events`（按 model_call_id 定位）/ `record_invocation_intent` / `advance_invocation(Started)` / `record_invocation_receipt` / `commit_run_outcome` / `record_run_lineage` / `record_run_state_change(→waiting_approval)`），先释放 arrival 许可再等 go 许可——无轮询、无 sleep。
- 单元测试（`--lib cancel`，19 项中 5 项为本轮新增）直接钉相位机。

## R03-FIX-F03-C01 取消先于最终提交取得胜利

- 普通断言路径：`cancel_accepted_while_final_events_persist_beats_the_completed_terminal`（Provider 已返 Final；fence 通过后泊在 `-mc0001` 模型事件持久化；`cancel_run_for` → **Accepted**；释放 → 终态 `cancelled` / `cancelled.requested`；`messages` 表无 final message 行（无 final_message_committed）；`run_state_changed` ≥3（queued→running→cancelling→cancelled 的 durable 取消腿存在）；驱动经单次 finalize 正常返回）。
- 终态形状覆盖：`cancel_accepted_before_the_terminal_claim_beats_the_failed_terminal_too`（Failed 终态同规则改道）；`cancel_accepted_before_the_terminal_claim_beats_every_terminal_shape`（intent 边界取消后：executor 0 次、provider 1 次（取消前那轮）、cancelled）；`cancel_at_the_no_provider_early_close_settles_cancelled`（无 Provider 早期收口：cancelled 而非 completed.no_final.no_provider_configured、无 final message）。
- 单元层：`claim_after_an_accepted_cancel_diverts_with_the_first_reason`（fire→claim→CancelledBy{首因}，相位不被改道倒退）；`claim_honors_a_parent_tree_cancellation_the_phase_has_not_seen`（父树遍历置 scope、相位 Active → claim 让位并补 Requested）。
- 状态：**PASS**。命令：`cargo test -p lingxi-service --test cancel_terminal_race --locked`（13/0，`cancel_terminal_race.log`）；`cargo test -p lingxi-service --lib cancel --locked`（19/0，`cancel-unit-tests.log`）。

## R03-FIX-F03-C02 完成已确定时取消正确反馈

- 不可撤销点（冻结定义）：`claim_terminal` 在相位 Mutex 内写入 `Settling` 的瞬间——先于 finalize 的第一个 await；此后取消 → `FireOutcome::TooLate` → 会话面 `CancelRunOutcome::TooLate`，终态照常恰好一次提交。
- 普通断言路径：`cancel_racing_the_finalize_transaction_is_too_late_not_accepted`（泊在 `commit_run_outcome` 委托前＝claim 已取、事务在途；取消响应 **非 Accepted**；释放 → completed.with_final + final message 在；`run_state_changed` 恰 2 条（无 cancelling 腿）；事后再取消 → AlreadyTerminal）；`cancel_after_the_committed_terminal_reports_already_terminal`（完全提交后取消 → AlreadyTerminal，终态不翻转、不重复结算）。
- 幂等重放不回归：`run_lifecycle.rs::duplicate_finalize_replays_and_conflicting_finalize_is_diagnosed`（12/0）经公共 `finalize_settlement` 原路径复跑绿。
- 状态：**PASS**。命令同上（`adjacent-suites.log`、`cancel_terminal_race.log`）。

## R03-FIX-F03-C03 并发重复取消不倒退

- 普通断言路径：`concurrent_duplicate_fires_keep_the_first_reason_and_a_single_fired`（96 entry × 4 caller × 48 宽子树、barrier 同步齐发：**恰好 96 个 Fired**；每 entry 相位 Requested 且 reason == scope 首因）；`a_fire_after_the_driver_reached_cleaning_never_regresses_the_phase`（Cleaning 后重复 fire → AlreadyCancelling、相位不回退）；单元 `fire_landing_after_the_cleaning_leg_writes_nothing_back`（同上，锁内无写）。
- 状态：**PASS**。命令同上。

## R03-FIX-F03-C04 取消先发生时不再启动新操作

- 普通断言路径：`cancel_at_the_intent_boundary_dispatches_no_new_external_call`（泊在意图写：取消 Accepted 后 executor **0** 次、provider 1 次；journal 恰 1 条、`Prepared`、无回执——不为未派发调用写 started）；`cancel_at_the_started_boundary_dispatches_no_new_external_call`（泊在 started 推进：executor 0 次、cancelled）；`cancel_at_the_authorization_boundary_never_opens_the_approval_ask`（泊在 waiting_approval 腿：审批询问 **0** 次、executor 0 次、cancelled）；`cancel_at_the_no_provider_early_close_settles_cancelled`（无 Provider 早期收口变体）；`cancel_between_tool_iterations_stops_the_second_dispatch`（两工具请求：第 1 个完成（1 次外部调用）后泊在回执边界取消 → 第 2 个永不派发，executor 恰 1）；`started_without_receipt_journals_unknown_receipt_facts`（对照：正常完成链 journal Succeeded+回执）。
- 状态：**PASS**。命令同上。

## 回归底座

- workspace：66 suites / 666 passed / 0 failed（`logs/workspace-test-final.log`）。
- fmt 零 diff；clippy `-D warnings` 零告警；`Cargo.lock` sha1 `3b659f41…` 不变。
- G01 语义保持：cancel_link_inheritance 7/0、subagent_closeout 8/0、cancellation_tree 8/0。
- 相邻套件：late_result_fence 5/0、run_lifecycle 12/0、request_dedup 4/0、exit_race_rejections 2/0、execute_concurrency 3/0、r03_t08_acceptance_matrix 17/0、invocation_journal 3/0、background_disconnect_recovery 3/0（`adjacent-suites.log`）。
- check-contracts（626 entries 零漂移）/ check-boundaries exit 0（`logs/`）。
