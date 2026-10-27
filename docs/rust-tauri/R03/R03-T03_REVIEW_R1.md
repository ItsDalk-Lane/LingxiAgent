# R03-T03 独立验收报告（REVIEWER-R03-T03-R01，第 1 轮）

- VERDICT: **PASS**（附 2 项非阻塞 MINOR 缺陷 D1/D2，须在下一轮触碰该文件时修正；见 §10）
- TASK_ID: R03-T03（取消树和子任务监督）；ACCEPTANCE_IDS：R03-A05、R03-A06
- TASK_BASE_SHA：`d075ec8a62bdd9e8346c450b9a4b62668554d9cd`（分支 codex/rust-tauri-migration）
- 候选：基线 HEAD + 未提交工作树（修改 5 + 新增 4 个 rust 路径）
- 审查者：REVIEWER-R03-T03-R01（一次性独立验收代理；未参与本 Task 的实现/修复；验收期间未修改任何产品源码/测试/配置/阶段图，代码冻结）
- 审查时间：2026-09-29；环境：macOS darwin 27.0.0 arm64；rustup 锁定工具链 1.98.1（`~/.cargo/bin/cargo` 代理 + `--locked`；专用 `CARGO_TARGET_DIR=/tmp/r03-t03-review-target`；PATH 中 Homebrew cargo/rustc 1.93.0 未使用）
- 复测产物：`artifacts/rust-tauri/R03/T03-R01-review/`（定向/全量/门禁日志 + 独立 evidence 复算 JSON）

## 1. 候选绑定核对（复测前 + 复测后各一次）

- tracked-diff sha256 前 16 位：`17002eba62e995b2`（= 派单绑定值）。全部复测结束后复算仍为 `17002eba62e995b2`，HEAD 仍为 `d075ec8a6`；status 集摘要（排除派单文件与本轮验收产物目录）复测前后均为 `4246f782a4f4e321`（= 派单绑定值）——候选在本轮验收期间未被改动，rust/ 路径集完全一致。
- 逐文件 SHA-256：本审查者独立 `shasum -a 256` 复算 9 个文件（5 修改 + 4 新增），与执行者 `artifacts/rust-tauri/R03/T03-E01/candidate-summary.txt` **全部一致**；`rust/Cargo.lock` = `90111c4b…`（= R02_HANDOFF dependency_locks，零变化）；`rust/Cargo.toml`、`lingxi-service/Cargo.toml`、`lingxi-kernel/src/lib.rs`、`lingxi-protocol/` git diff 为空。
- **tokio `test-util` feature 未进入候选**：全部 Cargo.toml 无 `test-util`（lingxi-service tokio features 仍为 `macros,rt-multi-thread,net,signal,io-util,time,sync`）；rust/ 源码无 `pause()`/tokio 时间控制 API（确定性来自门控替身 + 1ms 轮询 `wait_until`（500ms 硬上限）+ 真实有界超时 50/500ms）。
- 注：执行者报告 §8 正文列出的 3 个要点 digest 与最终候选不符（见 D2）；报告自声明「以 candidate-summary.txt 为准」，而 candidate-summary 与实际文件全部一致，候选完整性不受影响。

## 2. 真实接线追查（源码级，非枚举）

从取消入口到唯一 finalize 的完整链路逐文件核实：

```
SessionStore::cancel_run_for（sessions.rs 新）                ← 服务面取消入口
  → port.load_run → 会话归属检查（can_access；跨主体 Forbidden，实测）
  → 终态判定：terminal → AlreadyTerminal（incumbent no-op false 语义）
  → supervisor.cancel_run → CancelRegistry::fire              ← Phase 1 Requested
      → entry.scope.cancel(reason) → cancel_recursive 沿树向下急切传播
        （首次获胜；原因不覆盖；children 锁内取还防注册丢失；Notify 唤醒全部等待者）
  → run 自己的 driver（runs.rs drive_run 六处竞速点观测）：
      循环顶 root.is_cancelled / acquire_or_break(select 竞速) /
      model_child.wait() / gate_child.wait() / tool_child.wait() 各 select!{biased; cancelled, work}
      —— 取消与完成同时就绪时取消优先；scope 已取消时 spawn_linked 的 wrapper
         biased-select 在首次 poll 即 Aborted，子 future 不被 poll（不执行新工具）
  → settle_cancellation（runs.rs 新）                          ← Phase 2 Cleaning
      ① durable record_run_state_change(live_status→cancelling)（新相位事务）
      ② CancelBudget 锚定 scope.cancelled_at()（请求时刻，镜像 ShutdownBudget shutdown.rs:71）
      ③ tree_confirmed（已观测退出）∪ drain_run（锚定预算内 join）
         → 全确认 ConfirmedTerminated / 到期 StopUnconfirmed（报告 unconfirmed，不假称安静）
      ④ finalize_settlement(from=Cancelling, RunFinish::Cancelled)   ← 唯一 finalize（T01，未动）
      permit 随帧 drop → 配额 RAII 归还（T02 WaitGuard/QuotaPermit 路径复用）
  RegistrationGuard（RAII，全退出路径）：finalize 后 disarm；未 disarm（错误/drop/panic）
      → 树触发 + phase=Abandoned + durable 行保持 active（实测：tokio abort 驱动 future）
```

关键正确性判断（源码级逐条）：

- **两阶段 durable 是结构强制的**：kernel `RunStateMachine::transition`（lib.rs:139-150，本次未改）中 `Running` 的合法目标**不含** `Cancelled`——`Cancelling → Cancelled` 是唯一入径；`finalize_settlement` 未改（diff 证实），仍经 `commit_run_outcome` 单事务提交。`record_run_state_change`（run_store.rs 新）在写锁内单事务完成：归属校验（ctx_facts 对 session/owner）、终态目标拒绝（属 commit_run_outcome）、stored 终态拒绝、stored≠from 陈旧视图响亮 Conflict、kernel 转换表裁决、status+`run_state_changed` 事件+last_event_seq 同事务。cancellation_tree.rs `state_change_transaction_commits_legal_legs_and_diagnoses_bad_ones` 在真实 adapter 上断言合法腿/陈旧 from/终态目标/两阶段+cancelling→cancelled 全链。
- **监督返回无 fire-and-forget**：生产 run 工作仅经 `task_supervisor.rs:286/326` 的 `tokio::spawn`（wrapper）；每个 entry 有 owner（run_id/None）、可恢复句柄、退出结果；panic 在任务边界被 JoinError 包住、payload 转字符串返回（实测 `panicking_tool_child…`：请求任务不崩溃、run 结算 `completed.no_final.tool_partial_failure`、事件序含失败工具事实）；模型子 panic → `Failed{model_call_child_panicked}` 响亮。
- **取消优先与零执行**：`spawn_linked` wrapper 的 `select!{biased; cancelled, fut}` 使「取消后不得启动新工具/模型调用」成立——A05 审批态、stream 态 turn2 的工具脚本均未执行（tool arrivals 通道断言 =0）。
- **Abandoned 真实可达**：`cancel_surface_is_diagnosable…` 用 `alpha.abort()`（传输断连同款 drop 原语）驱动 future 消失 → guard Drop 树触发 + Abandoned verdict + durable `running`（无伪造终态）+ 后续 `cancel_run_for` → `DanglingActive`（detail 指名 R03-T07）。生产链路下 execute_for 内联 await drive_run（sessions.rs:427），HTTP handler future 断连即触发同一 guard 路径。
- **父取消不反向、无关不连带**：`cancel_recursive` 只下行；`a_child_scope_never_cancels_its_parent` 单测 + A06（detached ticker 在父取消后完成全部 20 tick，`run_id=None` 不入任何 drain）。

## 3. Steps / Deliverables 逐项核对（任务书 R03-T03）

| 步骤/交付 | 结论 | 证据 |
|---|---|---|
| 1 每 run CancellationToken 等价 + child 继承；独立后台仅显式依赖连带 | 满足 | `CancelScope::run_root` 铸根；model_call/approval/tool_call/child-run 全部 `root.child(...)` 挂树；`spawn_detached` 不挂任何 scope（A06 ticker 实测存活） |
| 2 区别请求取消/清理完成/无法确认停止；到期报告未清理项 | 满足 | `CancelPhase` 四阶段 + `Abandoned`；`noncooperating_child_is_reported_unconfirmed_not_fake_quiet`（永不 yield 工具子 + 50ms 真实预算 → verdict=StopUnconfirmed、registry 保留无退出条目、run 仍结算 cancelled） |
| 3 取消审批/排队/网络流三等待；退出后回收资源写最终状态 | 满足 | A05 参数化三态全绿（下 §5）；permit RAII 归还；唯一 finalize `cancelled.requested` |
| 4 child panic/error/aborted 监督返回；禁止 fire-and-forget | 满足 | `TaskExit{Completed,Failed,Aborted,Panicked}`；`ChildHandle::wait`/`drain_run` 双取回路径；panic 实测收容；注册表有界（cap 512，满时先收割已结束条目，仍满响亮 SpawnRejected） |
| 交付 取消树 / TaskSupervisor / 清理期限策略 | 满足 | cancel.rs（686 行）、task_supervisor.rs（782 行）、`CancelPolicy{cleanup_grace_ms=5000, supervised_task_cap=512}`（退化值拒启，`validate_resource_deps` 扩 1 项）+ `CancelBudget` 请求时刻锚定 + `drain_run` 期限 |

## 4. 冻结语义溯源核对（报告 §1 的 Node 证据逐处独立验证）

本审查者独立读取全部声称点位，语义逐条属实：

1. `core/session-coordinator.ts:5477-5499` `abortSession` 三分支（pre-prompt abort / 流中 force-release / 非流中 no-op false）——属实。
2. `_cleanupAbortedSessionSidecars`（:5424-5475）逐步 fan-out：工具执行 → taskRegistry.abortByParentSession → subagentRuns/Threads → deferredResults → confirmStore.abortBySession → 终端 → 浏览器，每步 warn 不阻断——属实。
3. `lib/confirm-store.ts:126` `abortBySession`：clearTimeout + 删 pending + `resolve({action:"aborted"})`——属实；`DEFAULT_TIMEOUT = 5min`（:11）——属实。
4. `docs/refactor-2026/P02/STATE_TRANSITIONS.md` R6：「abort（控制面）标记 isAborted；终态仍由 R4 落」——与四阶段/唯一 finalize 映射一致；T8「abortByParentSession 仅按父会话，终态跳过」——与 linked/detached 边界一致。
5. `docs/refactor-2026/P02/CANCELLATION_MAP.md` §2「取消后不得启动新模型调用/新工具」（biased select + 循环顶检查）、§4 清理期限表（强杀即时+如实呈现）、§1「AbortSignal 不跨进程当 JSON 传」（树为进程内，意图经服务面进入）——均与实现一致。

交叉：R02 `shutdown.rs:71` `ShutdownBudget`（信号时刻锚定）确为 `CancelBudget` 镜像先例。执行者未发明交互；`DanglingActive`/`Abandoned` 的恢复分类显式留给 R03-T07（非本 Task 义务）。

## 5. 验收场景复测（本审查者真实重跑，rustup 1.98.1 + --locked）

### R03-A05 取消覆盖等待态 — PASS（复现）

`cargo test -p lingxi-service --test cancellation_tree`：**8 passed / 0 failed / 0 ignored，exit 0**（两遍：默认 + `--nocapture` evidence 重跑）。正主 `r03_a05_cancel_exits_each_wait_state_and_returns_quotas` 单测试参数化驱动三态、每态全新真实服务（真实组合根/SQLite/内核状态机/配额/取消树）：

- **queue_wait**：beta 持唯一 global tool 槽（durable 停在工具 I/O）；alpha 工具准入排队（`waiting(Tool)==1` 断言）。
- **approval_wait**：durable status=`waiting_approval`（DB 直查）+ gate pending=1 + tool permit 持有=1（审批期在执行登记内的 incumbent 语义）。
- **stream_read**：provider 替身门控停在网络流读取（turn2 脚本会请求工具——取消后未发生）。
- 每态经真实归属检查面 `cancel_run_for` 发送一次取消。断言（全绿）：execute 任务返回、status=`cancelled`、terminal_reason=`cancelled.requested`、verdict=`confirmed_terminated`（queue 态 confirmed 为诚实空——该 run 从未进入工具调用）；配额归还（queue：waiting=0 且 beta 仍持=1、释放后归零；approval/stream：model+tool 全零）；目标 run 工具执行数=0（arrivals 通道断言）；durable 两阶段事件序逐态精确断言（queue=`sc,mc×2,sc,sc`；approval=`sc,mc×2,tcs,sc,sc,sc` 含 running→waiting_approval→cancelling；stream=`sc,sc,sc`）；晚到审批：pending 清零、`decide(Approved)` 返回 false。姊妹 `queue_wait_cancel_leaves_the_unrelated_holder_untouched` 独立复现双会话矩阵。

**A05「不额外调用工具」口径裁定**：审批态的 `tool_call_started` 是等待所覆盖调用的事实记录（T01 既有事件语义），工具**执行**以替身 arrivals 通道断言为 0——执行与事件两语义区分如实，通过条件成立。

### R03-A06 独立任务不被误杀 — PASS（复现）

`r03_a06_cancel_parent_spares_unrelated_background`：父 run（监督模型调用，停流读取）+ 演示性 child run（经公开监督 API `spawn_linked(parent_scope.child(ChildRun))` 挂父树；真实子代理派发面=R03-T06，报告/模块文档如实声明为 T06 前链路）+ 无关后台（`spawn_detached` ticker×20）。取消前监督关系断言：`live_children_of(parent)`=恰 2（model_call + child_run:demo）、后台 entry `run_id=None`/`exit=None`/tick≥1（活动证明）。取消父后：父结算 `cancelled`/`cancelled.requested`；child 经树停止（`TaskExit::Aborted`）；live children 清空；verdict=ConfirmedTerminated confirmed=2（含 child_run:demo、**不含 background**）；**后台在取消时刻仍 Running 并完成全部 20 tick**（`ticksAtCancel>0`、`ticksTotal==20`）；配额归零。

### 边界与四阶段诚实态 — PASS（复现）

`noncooperating_child_is_reported_unconfirmed_not_fake_quiet`（multi_thread(2) + 50ms 真实预算）：**「无法确认停止」真实可达**——永不 yield 的工具子到期 → verdict=`stop_unconfirmed`、unconfirmed=[tool_call:*]、registry 保留无退出条目、run 仍结算 `cancelled`、`RunFinish::Cancelled` detail 与 warn 明示「外部已发生动作不承诺撤销」。`cancel_surface_is_diagnosable…`：NotFound / AlreadyTerminal(completed) / 跨主体 Forbidden / Aborted→DanglingActive（durable `running` + 指名 R03-T07）四诊断面全断言。`approval_wait_round_trips…`：Approved 执行+完成、Rejected 零执行+`completed.no_final.tool_partial_failure`（晚到拒绝语义）。

### 机器证据独立复算

本审查者以独立 `R03_T03_EVIDENCE` 重跑生成 `r03-t03-evidence-review.json`：与执行者 `T03-E01/r03-t03-evidence.json` 9 组键完全一致，内容归一化后**仅 `r03_a06.ticksAtCancel` 1↔2 差异**（取消时刻已完成的 tick 数，时序活性探针，非断言值——断言为 `>0` 与 `==20`），其余逐字节一致（含 run id）——复现性充分。

### 门禁与全量（退出码实录，本审查者复跑）

| 命令 | 退出码 | 结果 |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | 无 diff |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 0 error/warning |
| `cargo test --workspace --locked` | 0 | **520 passed / 0 failed / 0 ignored / 49 suites ok**（= 执行者声称值；T02 为 499/0，+21 = cancellation_tree 8 + cancel:: 6 + task_supervisor:: 6 + kernel 门测试 1） |
| `cargo run -p xtask -- check-contracts` | 0 | API_COMPAT_MATRIX 626 entries 零漂移 |
| `cargo run -p xtask -- check-boundaries` | 0 | DEP-07/08/D5 PASS |

定向过滤器命中数全部 >0 且与执行者 filters.log 一致：cancellation_tree 8、cancel:: 6、task_supervisor:: 6、sessions:: 8、runs:: 4、run_lifecycle 12、session_serialization 5、execute_concurrency 3、event_subscription 12、service_persistence 2、lingxi-kernel 23、storage_transactions 7、run_finalize_property 2。复跑后无残留进程、无 `/tmp/lingxi-r03t03-*` 遗留。

## 6. 测试替身与质量审计

- 替身边界合规（R03_SCOPE_MATRIX test_double_boundary）：`GatedProvider`/`GatedTool`/`ManualGate` 仅实现 `TurnProviderPort`/`ToolExecutorPort`/`ApprovalGate`（测试选定时刻产生外部响应或自身失败形态），经 `ServiceState::bootstrap_with_deps` 注入**真实组合根**——真实 execute/cancel 入口、真实 SessionSupervisor/QuotaManager/RunSupervisor/取消树/TaskSupervisor、真实内核状态机与唯一 finalize、真实 RunDatabase 单写者事务（含新相位事务）、真实事件发布；替身不写状态、不落库、不 finalize、不参与身份 mint；断言直查 SQLite。
- 无 mock 掉待测核心、无空集合断言（queue 态 confirmed 空断言是语义化的诚实空，附 deduced 理由）、无 `#[ignore]`/`should_panic` 捕获改永真、无 skipped（全工作区 0 ignored）。
- **并发未被串行化掩盖**：A05/A06 用单线程 runtime + 门控替身实现可控调度（run 真实挂起在 I/O/审批/准入等待、无关持有者真实推进完成——真实交织）；queue 态是真实跨会话竞争（beta 持槽、alpha 排队）；非配合子测试 multi_thread(2)（阻塞子占独立 worker）+ 50ms 真实预算；无 serial_test 类锁。
- 生产代码无测试后门：`ServiceDeps.approval_gate` 默认 None；无 cfg(test) 分支进入生产路径。

## 7. R01/R02 回归保护核对（夹具改动逐处）

diff 文件清单证明本 Task 仅改动 5+4 个 rust 路径。既有测试夹具改动恰 2 处：

1. **kernel ports.rs 测试 FakePort**：补 `record_run_state_change` 最小实现（kernel 转换表门 + 终态拒绝）——trait 满足编译；新增门测试 `non_terminal_state_change_surface_is_kernel_gated`（5 合法腿 + 3 终态拒绝 + 2 非法腿）。事务性行为由真实 adapter 测试覆盖（cancellation_tree）。**保护未降**。
2. **sessions.rs 测试 FakePort**：同型补齐，无断言变化。**保护未降**。

其余全部 suite（run_lifecycle 12 / session_serialization 5 / execute_concurrency 3 / event_subscription 12 / service_persistence 2 / storage_transactions 7 / run_finalize_property 2 / kernel 23(+1) / sessions 8）原文未动、计数逐一相同，且在 520/0 全量内通过——模型/工具改经监督子任务后事件序不变（事件序精确断言复现）。无 gate 时零行为变化由 R01/T02 全量原样绿证明。

## 8. 范围边界（T04+ 未提前实现 = 正确）

- **T04 迟到栅栏**：无 attempt/generation 栅栏、无 requestId 去重、无 audit-only stale 记账；终态后取消 → AlreadyTerminal，迟到审批 → gate 双 false（T01 身份底线处理）。未越界。
- **T06 子代理**：无派发/回复/关闭面、无权限继承；child run 仅为取消树监督链路的演示性挂接（公开 API、测试侧），报告/文档如实声明。
- **T07 恢复**：仅定义并如实报告 `DanglingActive`/`Abandoned`，durable 行保持 active 不伪造；无启动扫描、无推进。
- **T05 收据**：无 InvocationJournal。**R04**：approval.rs 仅最小等待接口（trait + 决策类型 + drop-清-pending 契约文档），生产默认 None；无策略网关。
- `record_run_state_change` 遵守既有事务边界：终态仍归 `commit_run_outcome`（本表面拒绝终态目标）；`finalize_settlement`/`commit_run_outcome` 零改动；冲突响亮（Conflict/InvalidRequest）。
- 传输面：未新增 HTTP/WS 路由（lib.rs diff 仅模块注册/导出/注入面；check-contracts 626 零漂移佐证）——取消传输入口沿 T02 steer 先例递延 R06/R07，R03-SUP-03 服务层语义已证。npm/桌面栈零触碰。

## 9. 执行者报告核对结论

可验证声称**基本全部属实**：候选摘要与逐文件 hash、Cargo.lock/Cargo.toml 零变化、tokio test-util 未引入、5 处 Node 语义点位（§4 逐一核实）、全部命令退出码与命中数、520/0、626 entries、A05/A06/边界四组证据内容（独立复算一致）、DanglingActive/Abandoned 呈现、无 gate 零行为变化的 suite 计数对照。两处失实/瑕疵见 D1/D2（反证如下）。

## 10. 缺陷（均非验收阻塞）与建议性观察

### D1（MINOR，须修正）：`drain_run` 到期分支的 abort 是死代码，「到期 abort()」声称不实

- **定位/重现**：`rust/crates/lingxi-service/src/task_supervisor.rs:405-409` 先 `entry.handle…take()` 取走 JoinHandle；`:420` 将其 move 进 `tokio::time::timeout(remaining, join)`；到期 `Err(_elapsed)` 分支（`:443-469`）中 `:447-453` 再取 `entry.handle…as_ref()`——此时必为 `None`（handle 已被 timeout 消耗、随超时 future drop 而 detach），`handle.abort()` **不可达**。
- **违反契约/后果**：① 模块文档（:22-24「at expiry the rest are ABORTED (the last-resort real primitive)」）、分支注释（:444-446）、`tracing::warn!`（:460 明文「abort sent; no false quiet」）与执行者报告 §1/§2（「到期 `abort()`（最后手段真实原语）+ 报告 unconfirmed」）四处声称一个不会发生的 abort——日志行与文档对清理动作的描述失实；② 功能后果：**无**——任何会在 await 点 yield 的子任务在树触发瞬间已被 wrapper 的 biased select drop（A05 各态即时退出证实）；能活到 drain 到期的只有不让出控制权的子任务，而 `abort()` 对其同样要到下一个 yield 点才生效。验收义务本身（到期报告未清理项、不假称安静、run 仍结算）全部真实且有测试。
- **根因**：句柄所有权滑移——JoinHandle 被 timeout future 消耗后才尝试 abort。
- **同族路径**：无（`ChildHandle::abort()` 路径句柄完好，可用；生产链路无其他直接 abort 调用点）。
- **最小修复方向**：进入 timeout 前保存 `join.abort_handle()`，到期分支 abort 该 AbortHandle；同步修正注释/日志/文档与报告表述。
- **必须重跑**（修复后）：`cargo test -p lingxi-service --lib task_supervisor:: --locked` + `--test cancellation_tree` + `cargo test --workspace --locked`。

### D2（MINOR，报告文档缺陷）：执行者报告 §8 三个要点 digest 陈旧

报告 §8 列 runs.rs `7bff6a28…`（实际/摘要 `3b521304…`）、cancel.rs `942f5790…`（实际 `d682ba49…`）、cancellation_tree.rs `5b3a7686…`（实际 `7210a099…`）；candidate-summary.txt（报告自声明以其为准）与实际全部一致，候选绑定（tracked-diff `17002eba62e995b2`）不受影响。推测为终版前文件迭代残留。修复：报告再版时以 candidate-summary.txt 为准更正；无需重跑测试。

### 建性观察（非缺陷，不阻塞）

- **O1**：finalize 提交与 `guard.disarm()` 之间的微秒窗口内到达的取消会返回 `Fired/Accepted` 而运行实际已终态（verdict 停留 Requested，durable 行终态、无状态损坏）。与 incumbent「abort 落在 settle 后」同形竞态，durable 事实仍一致；可不处理。
- **O2**：run 注册后、`record_run_started` 提交前到达的取消因行不存在返回 NotFound（无 durable 对象可取消）；incumbent 的 pre-prompt abort 分支属传输面（R06/R07）。
- **O3**：Phase `Cleaning` 为瞬时中间态，测试经终态 verdict 间接覆盖（未单独断言观测到 cleaning 相位快照）；四阶段区分义务由模型+终态断言满足，如后续需要可加观测断言。
- **O4**：`settle_cancellation` 沿用 drive 开始的注入时钟 `now_ms` 写 `updated_at`（确定性时钟语义）；如需相位时间戳精确可后续取当前注入时刻。
- **O5**：TaskSupervisor cap 为进程级（512，全部并发 run 共享）；满载时响亮 SpawnRejected（run 失败而非静默）。R05/R07 真实负载接入时建议复核该值与驱逐行为。

## 11. 结论

R03-T03 到期义务（怎么做 1–4、三项交付物取消树/TaskSupervisor/清理期限策略、A05、A06）均有真实有效证据：真实接线成立（服务面取消入口 → 归属检查 → CancelScope 树急切传播 → run driver 六竞速点观测 → durable 两相位事务（running/waiting_approval→cancelling）→ 锚定预算 drain → ConfirmedTerminated/StopUnconfirmed → 经 T01 唯一 finalize 落 `cancelled`）；四阶段与 Abandoned 诚实态全部真实可达且有测试；「无法确认停止」经永不 yield 子任务实证（不假称安静）；冻结语义映射经现役 Node 源码逐点位核实忠实；R01/R02 回归满足（520/0，全部 suite 计数逐一相同，2 处夹具补齐未降低保护）；无越界实现（T04/T05/T06/T07/R04 未提前）；本审查者独立复跑全部定向与门禁通过、evidence 复算一致。发现 2 项非阻塞 MINOR 缺陷（D1 到期 abort 死代码与相关失实声称、D2 报告陈旧 digest），不触及任何验收断言义务，随下一轮触碰该文件时修正并按 D1 列明重跑。

**VERDICT: PASS**
