# R03-T03 报告｜取消树和子任务监督（EXECUTOR-R03-T03-E01）

- 状态：**READY_FOR_REVIEW**（执行者口径；独立复核归属总控另派）
- TASK_ID：R03-T03（ACCEPTANCE_IDS：R03-A05、R03-A06）
- TASK_BASE_SHA：`d075ec8a62bdd9e8346c450b9a4b62668554d9cd`（分支 `codex/rust-tauri-migration`，无 commit/push；工作树候选留给总控冻结）
- 执行时间：2026-09-29（UTC 摘要时间见 candidate-summary.txt）
- 环境：macOS darwin 27.0.0 arm64；rustup 锁定工具链 **1.98.1**（全部命令经 `~/.cargo/bin` rustup 代理 + `--locked`；证据链统一 `CARGO_TARGET_DIR=/tmp/r03-t03-target`）；SQLite=rusqlite 0.40.2 bundled；无网络外发、无真实供应商；npm/桌面栈零触碰；**tokio test-util feature 未引入**（T02 §8 环境红线遵守；并发/取消测试用单线程 runtime + 门控替身 + 有界真实超时（50/500ms）的确定性方案）
- 交付物三件：**取消树**（cancel.rs）、**TaskSupervisor**（task_supervisor.rs）、**清理期限策略**（CancelPolicy 锚定式预算 + drain 期限 + 未清理项报告）；另交付 waiting_approval 最小审批等待面（approval.rs，R04 网关前的测试接口边界）

---

## 1. 冻结语义溯源（先读原实现，再映射——不发明交互）

90 号来源 S19（STATE_TRANSITIONS.md）+ P02/CANCELLATION_MAP.md + Node 生产栈实读（文件:行均为本仓当前源码）：

| 冻结语义 | Node 生产栈证据 | Rust R03-T03 映射 |
|---|---|---|
| **取消传播到所属工作，不只改界面状态** | `core/session-coordinator.ts:5477-5498` `abortSession` 三分支（pre-prompt abort / 流中 force-release / 非流中 no-op false）；`_cleanupAbortedSessionSidecars`(:5424-5475) 逐步 fan-out 工具执行/子任务/子代理/审批 pending/终端/浏览器 | `CancelScope` 树（cancel.rs）：run 根 scope → child model call/approval wait/tool call/child run；`cancel()` 沿树向下急切传播（子不反向取消父——模型调用结束≠任务结束，T01 身份层的延续）；取消入口 `SessionStore::cancel_run_for`（归属检查后的服务面） |
| **取消是控制面标记，终态仍由唯一 finalize 落** | STATE_TRANSITIONS R6：abort 标记 isAborted，终态归 R4 agent_settled；R4 exactly-once | 四阶段：fire→`Requested`；driver 观测→`Cleaning`（durable running/waiting_approval→cancelling leg）+ 有界子任务回收；全部确认→`ConfirmedTerminated`；预算到期→`StopUnconfirmed`（报告未清理项，不假称安静）；终态经 T01 **唯一 finalize 路径**（`RunFinish::Cancelled`，from=Cancelling 两阶段） |
| **审批等待可取消；晚到 approve 不复活** | `lib/confirm-store.ts:126` `abortBySession`→clearTimeout+resolve({action:"aborted"})→wrapper 返回 toolError（执行 0 次）；晚到 resolve false；5min 默认超时 | 最小 `ApprovalGate`（approval.rs，服务层接口；生产默认 None=零行为变化）：driver 在工具执行前问询；决策 Rejected/Aborted=记录工具失败、**零执行**；取消把 gate 请求 future drop（替身 PendingGuard 清 pending——晚到 decide 返回 false，A05 实测）；审批等待持工具 permit（对应 incumbent 执行登记覆盖审批期） |
| **取消后不得启动新工具/模型调用** | CANCELLATION_MAP §2（agent loop 停、审批失效、SessionExecutionRegistry abort） | `tokio::select! { biased; cancelled, work }` ——取消与完成同时就绪时**取消优先**；循环顶每轮先查 root scope；每个 acquire/model-call/approval/tool-call await 均与取消竞速 |
| **父取消不误杀无关后台** | taskRegistry.abortByParentSession 仅按父会话；T8 状态机 | `spawn_linked`（挂 run 树）vs `spawn_detached`（有 owner/句柄/退出结果但**不挂任何 scope**）——A06 实测后台 ticker 在父取消后完成全部 20 tick |
| **清理期限：到达截止报告未清理项，不假称安静** | CANCELLATION_MAP §4（各期限值；强杀即时+如实呈现）；shutdown.rs 的锚定预算先例 | `CancelPolicy{cleanup_grace_ms 默认 5000, supervised_task_cap 512}`（退化值拒启）；`CancelBudget` 锚定在**请求时刻**（镜像 ShutdownBudget）；`drain_run` 到期 abort+报告 unconfirmed；`RunFinish::Cancelled` detail 与 `CancelPhase::StopUnconfirmed` 双记录 |

## 2. 实现与调用链（真实接线）

### 取消树（`rust/crates/lingxi-service/src/cancel.rs`，新）

- `CancelScope`：`Arc<ScopeInner>`（label/kind/cancelled/notify/parent/children-Weak）；`run_root(run_id)` 铸根；`child(label, kind)` 挂树（Weak 登记，锁内取还防注册丢失）；`cancel(reason)` 首次获胜、原因不被二次覆盖、沿树递归 `notify_waiters`；`cancelled()` future（flag+Notify 环回检查）；`live_descendant_labels()`/`path()` 监督观测。
- `CancelPhase` 四阶段 + `Abandoned`（driver 未经 finalize 消失的诚实态）；`RunCancelEntry`（根 scope + phase）；`CancelRegistry`：register/deregister（**撤销时保留有界 recent verdict**（cap 1024），结算后的四阶段判定可查询——「取消状态可查询」面）；`fire()`（Active→Requested，AlreadyCancelling/NotLive 可诊断）。
- `CancelPolicy` + `CancelBudget`（清理期限策略；`validate_resource_deps` 扩 1 项响亮拒启）。

### TaskSupervisor（`rust/crates/lingxi-service/src/task_supervisor.rs`，新）

- `spawn_linked(run_id, scope, label, fut)`：子任务包装 `select! { biased; scope.cancelled() => Aborted, fut => Completed }`——树触发时子 future 在 await 点被 **drop**（真实取消原语）；退出由 wrapper 即时记录进 entry。
- `spawn_detached(label, fut)`：同监督（owner=None、句柄、退出结果）但不挂 scope——run 取消永不触碰。
- `ChildHandle::wait()`：取值或监督退出（Aborted/Panicked/Failed）；panic 在任务边界被 `JoinError` 包住、payload 转字符串返回——**监督返回**，请求任务永不因子 panic 崩溃（实测）。
- `drain_run(run_id, budget)`：锚定预算内有界 join；到期 `abort()`（最后手段真实原语）+ 报告 unconfirmed（tracing warn + CleanupReport）；确认者退出结果交付 owner 并出册。
- 注册表有界（cap，满时先收割已结束未领条目，仍满响亮 `SpawnRejected`）——无无界记账。

### 驱动链接线（runs.rs 改造；生产链路）

```
HTTP POST /lingxi/v1/sessions/{id}/execute                     (lib.rs execute_session，未动)
  → SessionStore::execute_for → gate.try_begin_run → allocate_run_id    (T02，未动)
  → RunSupervisor::drive_run                                        (runs.rs)
      registry.register(run_id) → 根 scope + RegistrationGuard（RAII 全退出路径）
      record_run_started（T01 事务，未动）
      循环：root.is_cancelled 检查 + steering drain + quota acquire（select 竞速取消）
        模型调用 = spawn_linked 子任务（scope child model_call:{id}）→ wait 竞速取消
          子 panic/aborted → Failed{model_call_child_*} 响亮（监督返回）
        工具请求：tool permit（竞速取消）→ persist tool_call_started
          [approval gate 配置时] running→waiting_approval（durable 事务）
            → spawn_linked(approval:{id}) 等决策（竞速取消）
            → Approved：waiting_approval→running，执行；Rejected/Aborted：回 running、
              记录 Failed 工具事件、零执行（saw_tool_failure）
          工具执行 = spawn_linked 子任务（scope child tool_call:{id}）→ wait 竞速取消
      正常终 → finalize_settlement(from=live_status)（T01 唯一路径，未动）→ guard.disarm
      取消路径（六处竞速点之一触发）→ settle_cancellation：
        ① phase=Cleaning；② durable running/waiting_approval→cancelling（新事务）
        ③ 树已停子任务（观测退出）∪ drain_run（锚定预算）→ Confirmed / StopUnconfirmed
        ④ finalize_settlement(from=Cancelling, Cancelled)（唯一 finalize，cancelled.requested）
      permit 随帧 drop → 配额 RAII 归还（T02 路径复用）
  → cancel 入口：SessionStore::cancel_run_for（load_run → 会话归属 → 终态判定 →
      supervisor.cancel_run → registry.fire → 树触发 + phase=Requested）
```

### 存储面（kernel ports.rs + adapters run_store.rs）

- `StoragePort::record_run_state_change(ctx, from, to, reason, now)`（新）：**非终态相位事务**——kernel 转换表裁决、终态目标拒绝（属 commit_run_outcome）、stored≠from 的陈旧视图响亮 Conflict、单事务 status+`run_state_changed` 事件+last_event_seq。run_store 真实现 + 两处测试 FakePort 补齐；kernel 层新增门测试（合法腿/终态拒绝/非法腿）。

### 服务面（lib.rs/sessions.rs）

- `ServiceDeps` +`cancel_policy`/`approval_gate`（Default=生产默认：5000ms/512 + None——**无 gate 时零行为变化**，全部 R01/T02 既有测试原样绿）；`validate_resource_deps` 扩 cancel_policy 校验；RunSupervisor::new 增两参（启动响亮）。
- `SessionStore::cancel_run_for`（新服务面）+ `CancelRunOutcome{Accepted/AlreadyTerminal/DanglingActive}`（重启形 driverless 活行 → 如实报告 + 指向 R03-T07，不伪造终态）；`RunSupervisor::{cancel_run, cancel_phase, run_scope, task_supervisor, cancel_registry}` 公开观测/监督面。

## 3. 逐验收：预期 vs 实测

### R03-A05 取消覆盖等待态 — **PASS（参数化取消测试，本机隔离环境，确定性替身）**

- 前置：`r03_a05_cancel_exits_each_wait_state_and_returns_quotas`（cancellation_tree.rs）单测试参数化驱动三等待态，每态全新真实服务（真实组合根/真实 SQLite/真实内核状态机/真实配额/真实取消树）：
  - **queue_wait**：beta 停在工具 I/O 持唯一全局 tool 槽；alpha 到达工具准入**排队等待**（`waiting(Tool)==1`）。
  - **approval_wait**：替身工具请求审批 → durable status=`waiting_approval`（DB 直查）+ gate pending=1 + 工具 permit 持有（incumbent 语义：审批期在执行登记内）。
  - **stream_read**：provider 替身门控停在网络流读取中。
- 操作：每态经真实归属检查面 `cancel_run_for` 发送**一次**取消。
- 实测（全部断言绿）：
  - **三类等待均退出且结算 cancelled**：execute 任务返回、durable status=`cancelled`、terminal_reason=`cancelled.requested`；四阶段 verdict=`confirmed_terminated`（stream 含 model_call、approval 含 approval: 确认项；queue 无活子=诚实空）。
  - **配额归还**：queue 态 `waiting(Tool)==0`（T02 WaitGuard 路径）且无关持有者仍持槽（=1，取消未误伤）、释放后归零；approval 态 permit 归还 `in_use(Tool)==0`；stream 态 model/tool 全零。
  - **不额外调用工具**：三态目标 run 的工具替身执行数=0（工具 arrivals 通道断言；审批态的 `tool_call_started` 是等待所覆盖调用的事实，非执行）。stream 态脚本 turn2 本会请求工具——取消后未发生。
  - **durable 两阶段事件序**（逐态断言）：queue=`sc,mc×2,sc,sc`；approval=`sc,mc×2,tcs,sc,sc,sc`（含 running→waiting_approval→cancelling）；stream=`sc,sc,sc`。
  - **晚到审批拒绝**：取消后 gate pending 清零、`decide(Approved)` 返回 false（incumbent 晚到 resolve false 语义）。
- 证据：`a05-a06-cancellation-tree.log` 的 `R03_A05_TRACE ×9`（三态×3 阶段）+ `r03-t03-evidence.json`（9 组机器断言：三态 + queue 姊妹 + A06 + 边界四组）。

### R03-A06 独立任务不被误杀 — **PASS（监督关系和活动断言）**

- 前置（同跑）：父 run（其监督模型调用，停在流读取）+ **演示性 child run**（测试经 `task_supervisor().spawn_linked(parent_scope.child(ChildRun))` 挂父树，T06 前的取消树 child-run 链路）+ **无关后台**（`spawn_detached` ticker，20 tick）。
- 监督关系（取消前断言）：`live_children_of(parent)`=恰 2（`model_call:{run}-mc0001` + `child_run:demo`）；后台 entry `run_id=None`、`exit=None`（活跃）。
- 操作：`cancel_run_for(parent)`。
- 实测：父结算 `cancelled`/`cancelled.requested`；verdict=`confirmed_terminated` 且 confirmed=2（含 child_run:demo、**不含 background**）；child_run 经树停止（`TaskExit::Aborted`）；`live_children_of(parent)` 清空；**后台在取消时刻仍 Running（tick>0）并在此后完成全部 20 tick**（活动断言）；配额归零。
- 证据：`R03_A06_TRACE 1..3` + evidence `r03_a06`（survivedCancel=true、ticksAtCancel、ticksTotal=20）。

### 任务书「怎么做」1–5 对照

1. 每 run CancellationToken 等价（根 scope）+ child 继承（model/approval/tool/child-run 子 scope）；独立后台仅显式依赖才连带（detached 不挂树）✔
2. 四阶段区分（Requested/Cleaning/ConfirmedTerminated/StopUnconfirmed）+ 期限后报告未清理项（`noncooperating_child_is_reported_unconfirmed_not_fake_quiet`：永不 yield 的工具子——50ms 预算到期 → verdict=`stop_unconfirmed`、registry 保留无退出条目、run 仍结算 cancelled——不假称安静）✔
3. 取消审批/排队/网络流三等待（A05）；受管工作退出后回收资源（RAII permit/lease）并写最终状态（唯一 finalize，cancelled 终态）✔
4. child panic/error/aborted 监督返回（panic 在任务边界被收容并作为 `Panicked` 退出返回；`panicking_tool_child_is_supervised_and_the_run_settles_loudly`：请求任务不崩溃、run 结算 `completed.no_final.tool_partial_failure`、事件序含失败工具事实）；全部异步任务 owner/句柄/退出结果（spawn 必经 supervisor，注册表有界）✔
5. 不配合取消（上）、清理超时（同）、重启后可解释状态（`cancel_surface_is_diagnosable_...`：driverless 活行 → `DanglingActive`+指向 R03-T07，durable 行保持 running 不伪造）；已发生外部动作不承诺撤销（StopUnconfirmed 的 RunFinish detail 与日志明示）✔

## 4. 修改文件清单

修改（5）：`rust/crates/lingxi-kernel/src/ports.rs`（StoragePort 新方法 + FakePort + 门测试）、`lingxi-adapters/src/storage/run_store.rs`（record_run_state_change 事务）、`lingxi-service/src/{lib.rs,runs.rs,sessions.rs}`。
新增（4）：`lingxi-service/src/{cancel.rs,task_supervisor.rs,approval.rs}`（生产代码）、`lingxi-service/tests/cancellation_tree.rs`（永久测试，8 测试）。
`rust/Cargo.lock` 零变化（`90111c4b…`=R02_HANDOFF）；`lingxi-service/Cargo.toml` 零变化（无新依赖——panic 收容经 JoinError 实现，未引入 futures-util）。逐文件 SHA-256 见 candidate-summary.txt。

### 既有测试夹具改动（逐处+理由；无断言删除/放宽/改永真，无 skipped）

1. kernel ports.rs 测试 FakePort 与 sessions.rs 测试 FakePort：补 `record_run_state_change`（kernel 转换表门 + 终态拒绝的最小实现——trait 满足编译；事务性行为由真实 adapter 测试覆盖）。
2. **无其他夹具改动**——run_lifecycle/session_serialization/execute_concurrency/event_subscription/service_persistence/run_finalize_property/storage_transactions 断言原文未动，全部原样通过（模型/工具调用改经监督子任务后事件序不变）。

## 5. 验证命令与退出码（全部经 rustup 1.98.1 + `--locked`；verify-stage R03 未注册，未运行未伪造）

| 命令 | 退出码 | 结果摘要 |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | 无 diff |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 0 error/warning |
| `cargo test --workspace --locked` | 0 | **520 passed / 0 failed / 49 suites ok**（T02 为 499/0；+21 = cancellation_tree 8 + cancel:: 6 + task_supervisor 6 + kernel 门测试 1；含 R02 全量回归） |
| `cargo run -p xtask -- check-contracts` | 0 | API_COMPAT_MATRIX 626 entries 零漂移 |
| `cargo run -p xtask -- check-boundaries` | 0 | DEP-07/08/D5 全 PASS |

定向过滤器（全部命中>0，filters.log）：cancellation_tree 8、cancel:: 6、task_supervisor 6、sessions:: 8、runs:: 4、run_lifecycle 12、session_serialization 5、execute_concurrency 3、event_subscription 12、service_persistence 2、lingxi-kernel lib 23、storage_transactions 7、run_finalize_property 2。

确定性方案：A05/A06/审批/panic/诊断面用单线程 current_thread runtime + 通道锚点 + 门控替身；非配合子测试用 multi_thread(2)（阻塞子占独立 worker）+ 50ms 真实预算；`wait_until` 辅助为 1ms 轮询 + 500ms 硬上限（有界真实等待，非 test-util）。

## 6. 证据位置

`artifacts/rust-tauri/R03/T03-E01/`：`commands.log`（命令+退出码）、`workspace-test.log`（520/0 原始输出）、`clippy.log`、`fmt-check.log`、`check-contracts.log`、`check-boundaries.log`、`filters.log`（13 组定向过滤命中）、`a05-a06-cancellation-tree.log`（R03_A05/A06_TRACE 全程轨迹 + 8 passed，--test-threads=1 证据重跑）、`r03-t03-evidence.json`（9 组机器断言：a05 三态/queue 姊妹、a06、审批双腿、panic 监督、非配合子、取消面诊断）、`kernel-tests.log`、`adapters-tests.log`、`candidate-summary.txt`（基线/HEAD+逐文件 sha256+Cargo.lock 零变化）。复跑后无残留进程、无 `/tmp/lingxi-r03t03-*` 遗留。

## 7. 测试替身与最小接口边界

- 允许侧：`GatedProvider`/`GatedTool`（Parked/Immediate/Panic/BlockNeverYield）/`ManualGate` 仅在测试选定时刻产生外部响应或自身失败形态，经 `ServiceDeps` 注入**真实**组合根——真实 execute/cancel 入口、真实 SessionSupervisor/QuotaManager/RunSupervisor/取消树/TaskSupervisor、真实内核状态机与唯一 finalize、真实 RunDatabase 单写者事务（含新相位事务）、真实事件发布。替身不写状态、不落库、不 finalize、不参与身份 mint。
- `ApprovalGate` 是**服务层最小审批等待接口**（R04 完整网关前）：生产默认 None（无 gate 时无 run 进入 waiting_approval——零行为变化，由 R01/T02 全部既有测试原样绿证明）；替身只决定审批，PendingGuard 落 drop-清-pending 的 incumbent 语义。演示性 child run 由测试经公开监督 API 挂父树（T06 前的取消树链路，非子代理产品面）。
- 取消原语侧：子任务取消=scope 触发后 wrapper select 在 await 点 drop 子 future（与断连 drop 请求 future 同一真实原语）；abort 仅作 drain 到期最后手段；无任何替身模拟的「取消」。

## 8. 候选输入摘要

基线 `d075ec8a6…`（=HEAD，无 commit）；修改 5 + 新增 4 路径，逐文件 SHA-256 与说明见 `candidate-summary.txt`；`rust/Cargo.lock` 零变化（`90111c4b…`）。要点 digest：run_store `46389e1d…`、kernel ports `8c5b05da…`、service lib `1c0858da…`、runs.rs `7bff6a28…`、sessions.rs `bd3023f3…`、cancel.rs（新）`942f5790…`、task_supervisor.rs（新）`e5d3ec22…`、approval.rs（新）`17ed1c4d…`、cancellation_tree.rs（新）`5b3a7686…`（以 candidate-summary.txt 为准）。

## 9. 未验证项 / 边界（如实）

- **取消的传输入口**：`cancel_run_for` 为服务层 API（沿 T02 steer 的先例——WS/HTTP 传输入口归 R06/R07，当前 Rust WS 仅事件订阅，不发明新传输路由）；A05/A06 经服务面+真实归属检查驱动。R03-SUP-03 的 stream/run 粒度语义由本 Task 服务层证明，终端绑定归 R07。
- 迟到结果栅栏（attempt1 取消后投递 → audit-only stale）= **R03-T04**；本 Task 终态后事件仍走 T01 的响亮拒绝。
- 子代理运行关系（child run/thread 映射、权限继承、后台解耦）= **R03-T06**；本 Task 的 child run 是取消树监督链路的演示性挂接。
- 重启后非终态行的**恢复分类/推进** = **R03-T07**；本 Task 仅定义并如实报告 DanglingActive/Abandoned（durable 行保持 running）。
- 收据/InvocationJournal = T05；R04 将以完整审批策略网关替换最小 gate（同注入位）。
- 真实供应商/网络流（R05）：stream_read 用门控替身模拟网络读取等待；tokio test-util 未引入（环境红线）。
- 无跨平台（本机 arm64 macOS）；npm/桌面栈未触碰。

## 10. 独立复核重点建议

1. 取消树方向性：父 cancel 递归到子、子结束不取消父（cancel.rs `cancel_recursive`/单测）；`biased` select 的取消优先（取消与完成同时就绪时不启动新工作）。
2. 唯一 finalize 纪律：取消结算确实经 `finalize_settlement(from=Cancelling)`（runs.rs `settle_cancellation` 尾部）；`record_run_state_change` 的 stored≠from Conflict 与终态目标拒绝（run_store）。
3. RegistrationGuard 全退出路径：正常/取消/错误/abort 四路径的 disarm 差异（未 disarm → 树触发 + Abandoned verdict + durable 行如实）。
4. A05 断言口径：审批态的 `tool_call_started` 事件 vs 工具**执行**数（通道断言）——两者语义不同且报告如实区分。
5. TaskSupervisor 记忆边界：确认者由 drain/wait 出册、未确认者留在有界注册表（cap 压力收割）；`drain_run` 预算锚定在 fire 时刻。
6. 无 gate 时的零行为变化：R01/T02 既有套件计数逐项对照（run_lifecycle 12、session_serialization 5、execute_concurrency 3、event_subscription 12、service_persistence 2 等）。
