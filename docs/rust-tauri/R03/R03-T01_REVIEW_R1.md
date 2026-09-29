# R03-T01 独立验收报告（REVIEWER-R03-T01-R01，第 1 轮）

- VERDICT: **PASS**
- TASK_ID: R03-T01（实现运行与尝试状态机）；ACCEPTANCE_IDS: R03-A01、R03-A02
- TASK_BASE_SHA: `526f7770f1eff6be289b8c34faeccc1b95e181fd`（分支 codex/rust-tauri-migration）
- 候选: 基线 HEAD + 未提交工作树（修改 9 + 新增 3 路径）
- 审查者: REVIEWER-R03-T01-R01（一次性独立验收代理；未参与本 Task 的实现/修复）
- 审查时间: 2026-09-29；环境: macOS darwin 27.0.0 arm64；rustup 锁定工具链 1.98.1（`~/.cargo/bin/cargo` 代理 + `--locked`；专用 `CARGO_TARGET_DIR=/tmp/r03-t01-review-target`，未用 Homebrew cargo）
- 复测产物: `artifacts/rust-tauri/R03/T01-R01-review/`（a01/a02/kernel/workspace/fmt/clippy/check-contracts/check-boundaries/r02-subset-* 日志 + r03-a02-finalize-property.json）

## 1. 候选绑定核对（复测前 + 复测后各一次）

- tracked-diff sha256 前 16 位：`90e0e442353606f7`（= 派单绑定值）；复测结束后复算仍为 `90e0e442353606f7`，HEAD 仍为 `526f7770f`，status 集不变（9 修改 + 3 新增 rust 路径 + docs/artifacts 未跟踪目录）。候选在本轮验收期间未被改动。
- 13 个文件（9 改 + 3 新 + Cargo.lock）逐文件 SHA-256 与执行者 `artifacts/rust-tauri/R03/T01-E01/candidate-summary.txt` 及派单摘要（runs.rs `fdf92e8a…`、run_lifecycle.rs `3e240396…`、run_finalize_property.rs `1561cb77…`）全部一致；`rust/Cargo.lock` 零变化（`90111c4b…` = R02_HANDOFF dependency_locks）。
- 复跑无残留：无 lingxi 测试进程存活，`/tmp/lingxi-r03*` 临时目录 0 个（测试自清理生效）。

## 2. 真实接线追查（源码证据，非枚举/演示）

从真实入口到存储写者/事件发布的完整链路逐文件核实：

```
HTTP POST /lingxi/v1/sessions/{id}/execute   (lingxi-service/src/lib.rs:2831 → execute_session:2025)
  → SessionStore::execute_for(port, events, state.runs(), principal, …)   (sessions.rs；supervisor 实参注入)
      会话归属/NotFound/Forbidden 判定保持 R02 原语义；run_id 由 backend 原子分配器铸造
  → RunSupervisor::drive_run   (lingxi-service/src/runs.rs，新；无第二写终态者)
      ① 内核 RunStateMachine::transition(Queued→Running) 活链预判
      ② port.record_run_started：单事务（run 行 + attempt 行 + queued→running 事件）
         → events.publish_committed 严格在 commit Ok 之后
      ③ 无 Provider（生产默认，R05 前）：CompletedWithoutFinal{NoProviderConfigured}
         有 Provider：turn 循环 —— ModelCallId={run}-mc%04d / ToolCallId={run}-tc%04d 由驱动方 mint；
         Final/ToolRequests/Continue/Empty/Failed(retryable→同 run 新 attempt record_attempt_started)
         每个事实经 port.record_run_events 单事务落 key_events
      ④ finalize_settlement（公开唯一 finalize 路径）→ RunOutcome → port.commit_run_outcome
         单事务内经内核 RunStateMachine::finalize 裁决 → publish_committed（commit 后）
```

R02 的「立即成功」内联执行路径已被删除（diff 证实），同一 Run 的终态只有 supervisor 单一 finalize 一个 owner。存储写者仍是 R02 的同一 RunDatabase 单写者事务边界（`with_write_txn` + 有界 DbQueue），无第二套存储。

## 3. Steps / Deliverables 逐项核对（任务书 R03-T01）

| 步骤 | 结论 | 证据 |
|---|---|---|
| 1 状态与转换表 + 外部协议兼容映射 | 满足 | 内核转换表保持 R01 版并逐分支对照契约 §4（queued→running↔waiting_approval；活跃态→cancelling→cancelled 两阶段；running→completed/failed/interrupted_needs_attention；终态拒绝一切）；`RunStatus::from_legacy_wire_name` 的词表锚点经本审查者独立核实真实存在（server/ws-protocol.ts:16 `completed\|failed\|aborted`、server/block-extractors.ts:290 `success`）；未知名返回 None（响亮）。新 wire_name 不变（协议测试锁定）。 |
| 2 RunId 固定 / attempt 递增 / ModelCallId、ToolCallId 独立 | 满足 | kernel `attempt_id/model_call_id/tool_call_id`；`retryable_provider_failure_reopens_attempt_on_the_same_run`：1 run 行、run_attempts 两行、attempt_count=2、mc1 挂 #a1 / mc2 挂 #a2（provider 观测断言）；provider 重连不另建用户任务。 |
| 3 单一 finalize / 幂等 / 冲突诊断 | 满足 | 内核 `RunStateMachine::finalize`（决策核心）+ `commit_run_outcome` 事务内再裁决（写锁下）；幂等从 R02 的「仅 status 相等」收紧为全结算相等（status+reason+final message；基线代码证实旧实现确实会静默吞同 status 异 payload）；R02 公开不变量（identical replay 返回全量 durable 事件、conflict detail 含 already terminal）等价保持（storage_transactions `finalize_is_idempotent_and_conflicts_are_diagnosed` 复跑绿）。 |
| 4 明确 outcome / 不编造答案 | 满足 | `RunFinish` 封闭词表：EmptyReply/ProcessOnly/ToolPartialFailure/NoProviderConfigured（completed.no_final.*）、TurnBudgetExceeded/ToolExecutorUnavailable/ProviderFailed（failed.*）、Cancelled、InterruptedNeedsAttention；final message 仅存在于 CompletedWithFinal；无 final 时 messages 行数 0 被显式断言（empty_reply/process_only/tool_partial_failure/no_provider 四测试）。 |
| 交付 RunStateMachine / 转换表 / 运行结果契约 | 满足 | 均在 lingxi-kernel（无平行状态机）；service 侧 runs.rs 是驱动者不是第二状态机。 |

## 4. 验收场景复测（本审查者真实重跑，全部经 rustup 1.98.1 + --locked）

### R03-A01 多模型调用只有一个任务终态 — PASS（复现）

- `cargo test --locked -p lingxi-service --test run_lifecycle`：**12 passed / 0 failed，exit 0**。
- 正主场景 `r03_a01_three_model_calls_produce_exactly_one_task_terminal`：替身按 工具→继续→最终 三次模型调用（ScriptedProvider/ToolDouble 仅产生外部响应，实现 `TurnProviderPort`/`ToolExecutorPort`，无任何状态书写面）。断言：runs 1 行 completed + `completed.with_final` + attempt_count=1；model_call_started/completed 各 3；tool_call_started/completed 各 1；call id 恰为 `{run}-mc0001..0003` 全挂 `#a1`、tool id `{run}-tc0001`；**durable 事件全序 11 条逐类型断言**，终态迁移事件恰 1 条且钉在 `vec![events.len()-2]`（final_message_committed 居末）——中间模型结束被结构性排除出任务终态；live hub 订阅收满 11 帧。
- 数据库断言直查 runs/key_events/messages（`query_one_text`），非替身自报。

### R03-A02 重复终态不重复结算 — PASS（复现，计数逐位一致）

- `cargo test --locked -p lingxi-adapters --test run_finalize_property`：**2 passed / 0 failed，exit 0**。
- 本审查者以 `R03_A02_PROPERTY_EVIDENCE` 重新生成机器计数，与执行者证据**逐字段一致**：`runsSettled=200, firstCommits=200, idempotentReplays=350, diagnosedConflicts=1450, unexpectedResults=0`，同 seed `0x5EED_0000_C0FF_EE01`。属性断言：每轮恰一次 `newly_committed=true`；replay 必须等于 winner 否则 panic；conflict detail 必含 run id 与 already terminal；持久态（status/terminal_reason/final message 行 0..1 及内容）恒等于先到者；`run_state_changed` 恒 2 条；`total_replays>0 && total_conflicts>0`（防空集假绿）。真实 RunDatabase，非内存桩。
- 乱序非法到达（running 直跳 cancelled）→ InvalidRequest，run 仍恰结算一次（第二测试）；纯核半场 `finalize_property_first_settlement_wins_and_never_flips`（500 轮）在 kernel 22/22 中。

### 门禁与全量（退出码实录）

| 命令 | 退出码 | 结果 |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | 无 diff |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 0 error/warning |
| `cargo test --workspace --locked` | 0 | **482 passed / 0 failed / 47 suites**（= 执行者声称值） |
| `cargo run --locked -p xtask -- check-contracts` | 0 | API_COMPAT_MATRIX 626 entries 零漂移 |
| `cargo run --locked -p xtask -- check-boundaries` | 0 | DEP-07/08/D5 PASS |

定向过滤器命中数全部 >0：run_lifecycle 12、run_finalize_property 2、kernel lib 22、R02 子集 sessions 8（180 filtered）/execute_concurrency 3/service_persistence 2/event_subscription 12/storage_transactions 7/run_id_allocator 5/migration_idempotency 2/event_store_reads 3/backup_restore 11/disk_full_fault 2 —— 与执行者报告的命中数全部一致，无「0 命中算通过」。

## 5. 测试替身与质量审计

- 替身边界合规（R03_SCOPE_MATRIX test_double_boundary）：`ScriptedProvider`/`ToolDouble` 只实现 TurnProviderPort/ToolExecutorPort（外部响应源），被注入**真实** `ServiceState::bootstrap_with_deps` 组合根；身份 mint（ModelCallId/ToolCallId）在驱动方；替身无状态书写、不落库、不 finalize。待测核心（状态机/finalize/存储事务/事件发布）全部真实。
- 无 mock 掉核心、无空集合断言（属性测试强制 replays>0/conflicts>0）、无 #[ignore]/skipped（各输出 0 ignored）、无断言改永真、无替身直写终态。
- sessions.rs 内部测试 FakePort 补齐 StoragePort 两个新方法为空实现——仅使 trait 满足编译，该 FakePort 原本就不参与生命周期断言（公开不变量断言全部经真实 EventService+RunDatabase），不构成保护降低。

## 6. R02 回归保护核对（6+3 处夹具改动逐处判断）

- sessions.rs 内部测试 6 处：仅插入 `&supervisor()`（`RunSupervisor::without_provider()`）实参，drive 走真实生命周期；owner 全可见/跨主体 Forbidden 零副作用/commit 失败=Storage 错误零 outcome/64 并发独立 run id 四组断言原文未动，复跑 8/8 绿。
- execute_concurrency.rs / service_persistence.rs / event_subscription.rs 各 1 处：仅插入 `state.runs()` 实参（diff 各 +1 行）；「每 run 恰 2 key events（64/128）」「重启后 completed+2 事件读回」断言原文保持，复跑 3/3、2/2、12/12 绿。无 Provider 路径刻意保持每 run 恰 start+terminal 两事件（`no_provider_configuration_is_explicit_and_r02_compatible` 显式锁住 total==2）。
- 全工作区 482/0（含 R02 全量）无未解释新增失败。

## 7. 范围边界（T02+ 未提前实现 = 正确）

未发现越界：无会话队列/串行化（T02）、无取消树/TaskSupervisor/CancellationToken（T03；cancelled/interrupted 仅作为可构造 outcome 走同一 finalize 契约，属 T01 第 3/4 步交付物而非提前实现）、无迟到结果 audit-only stale 记录与 requestId 去重（T04；本轮为身份底线：终态后迟到事件/未开 attempt 响亮 Conflict）、无 InvocationJournal/收据（T05）、无恢复协调器（T07）、无 R03 stage map（T08）。`rust/crates/xtask/src/stage_maps/` 仅 R02.json —— verify-stage R03 未注册且未伪造运行，符合预期。无生产入口切换、无 npm/桌面栈触碰。

## 8. 执行者报告核对结论

报告中全部可验证声称（测试命中数、退出码、482/0/47、626 entries、属性计数、文件摘要、Cargo.lock 零变化、词表锚点）经本审查者独立复算/复跑**全部属实**，无虚报；「未验证项」一节如实（waiting_approval 无运行链触发路径、T02+ 未做、verify-stage R03 未注册）。无 NOT_REPRODUCED / NOT_A_DEFECT 反证条目。

## 9. 建议性观察（非验收缺陷，不阻塞；与真实验收缺陷分开）

- O1（可观测性小缺口，建议 R04/R05 顺手补）：`drive_run` 中 `ToolRequests` 且 requests 为空 vec 时直接 break `Failed{empty_tool_request_list}`，该 turn 的 model_call 事件对未落库（其他 turn 种类均落）。outcome 响亮且可诊断（failed.provider_error + terminal_reason），不违反 T01 契约；同族路径仅此一处（其余分支均在 break 前 persist 或天然无需）。
- O2（测试化妆断言）：属性测试 `total_unexpected` 为常量 0、从不累加——真实保护是 `Err(other) => panic!`（unexpected 即失败），不构成假绿；建议后续把该计数接真实分支或删除以免误读。
- O3（时间戳粒度）：一次 drive 内全部事件共用单一 `now_ms`（请求时刻）。T01 确定性测试下合理；R05 真实 provider 到来时建议按 turn 记录各自时间。
- O4（如实边界，已在执行者报告 §8 声明）：waiting_approval 仅状态机表级验证，运行链触发需 R04 审批网关——属后续阶段义务，非本 Task 缺陷。

## 10. 结论

R03-T01 当前到期义务（Steps 1–4、三项交付物、A01、A02）均有真实有效证据；真实接线成立（HTTP 入口 → RunSupervisor → 内核状态机/finalize → run_store 单事务终态+事件 → commit 后发布）；R02 回归无保护降低；无越界实现；无未关闭的验收阻塞缺陷。

**VERDICT: PASS**
