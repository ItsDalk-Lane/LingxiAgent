# R03-T02 报告｜会话串行化和全局限流（EXECUTOR-R03-T02-E01）

- 状态：**READY_FOR_REVIEW**（执行者口径；独立复核归属总控另派）
- TASK_ID：R03-T02（ACCEPTANCE_IDS：R03-A03、R03-A04）
- TASK_BASE_SHA：`8b2f2cd595625e17b4153af072efc8484b79d5b6`（分支 `codex/rust-tauri-migration`，无 commit/push；工作树候选留给总控冻结）
- 执行时间：2026-09-29（UTC 摘要时间见 candidate-summary.txt）
- 环境：macOS darwin 27.0.0 arm64；rustup 锁定工具链 **1.98.1**（全部命令经 `~/.cargo/bin` rustup 代理 + `--locked`；证据链统一 `CARGO_TARGET_DIR=/tmp/r03-t02-target`）；SQLite=rusqlite 0.40.2 bundled；无网络外发、无真实供应商；npm/桌面栈零触碰
- 交付物三件：`SessionSupervisor`（session_supervisor.rs）、并发/排队策略（冻结语义映射：busy 拒绝 + steering 通道 + 有界登记处）、配额管理器（quotas.rs）

---

## 1. 冻结语义溯源（先读原实现，再映射——不发明交互）

90 号来源 S04/S11/S12/S13/S19 与 Node 生产栈实读结论（文件:行均为本仓当前源码）：

| 冻结语义 | Node 生产栈证据 | Rust R03-T02 映射 |
|---|---|---|
| **普通新提交遇忙即拒**（不排队） | `core/desktop-session-submit.ts:455-460`（`pendingDesktopSessionSubmissions`/`isSessionStreaming` → `notAcceptedError("session_busy")`）；`server/routes/sessions.ts:1539`（409 `{"error":"session_busy"}`）；`server/routes/chat.ts:560`（`session_busy` 稳定码、`retryable: true`） | `SessionSupervisor::try_begin_run`：busy → `SessionExecuteError::Busy` → HTTP **409** conflict + `details.reason="session_busy"` + `retryable:true`（`EndpointError::session_busy`）。拒绝发生在 allocate run id **之前**（零 durable 副作用）。采纳的"排队"= 客户端可重试，服务端不排普通输入队列 |
| **steering/follow-up 不打断循环** | `core/session-coordinator.ts:5294-5301`（`steerSession`→`session.steer`，isStreaming 才接受）；`server/routes/chat.ts:2445-2461`（WS steer 消息；miss 降级 prompt）；P02 STATE_TRANSITIONS R5（steer 落盘切 Run 语义层，`runSplit=true`，真正终结仍归 agent_settled）；P02 CONCURRENCY_RULES §3 | `SessionStore::steer_for`（服务层 API，与 execute 同一归属检查）：busy → `SteerOutcome::Accepted` 入**有界** steering 收件箱；idle → `SteerOutcome::Miss`（调用方降级为普通提交）。RunSupervisor 在**每个 provider turn 前** drain 收件箱，steer 文本进入下一模型调用的 input（`…\n\n[steering]\n{文本}`）；循环不终止、run 不拆分。runSplit 历史投影= R06（本 Task 不伪造） |
| **打断（abort）** | CANCELLATION_MAP §1（pre-prompt 标记 / abortSession 三分支 / agent_settled 唯一终态） | 用户取消入口与取消树 = **R03-T03**（T01 报告 §8 已确认未做）。本 Task 用现有真实终止原语验证配额归还（见 §3 A04 边界） |
| **并发容量有界** | CONCURRENCY_RULES §4（subagent per-session+global limiter；超限显式拒绝或排队，无无界队列） | `QuotaManager`：模型/工具 × 全局/agent/session 三层，FIFO 有界等待队列 + 超时拒绝（`failed.quota_exhausted.*`） |

## 2. 实现与调用链（真实接线）

### SessionSupervisor（`rust/crates/lingxi-service/src/session_supervisor.rs`，新）

- **每会话一个明确 owner**：`try_begin_run(session_id) -> SessionLease`（同一时刻每 busy 会话恰一个 lease）；内部锁只在 O(1) 登记处操作时持有，**绝不跨模型/工具 I/O await**。`SessionLease` 为 RAII：正常结算、错误、超时、驱动 future 被取消，任何路径都释放会话。
- `SessionConcurrencyLimits{steering_inbox_capacity(默认 8), registry_cap(默认 1024)}`：启动期响亮校验（0 拒启，`validate_resource_deps` 扩展 7 个退化用例）；登记处硬上限 + 空闲槽驱逐（镜像 R02 RateLimiter registry-cap 先例），溢出 → `SessionRegistryFull` → 503。
- Steering 收件箱有界；**运行结束后残留的 steer 文本保留**给该会话下一次 run 首个模型调用消费（Node 语义：steered 用户消息是持久会话输入，绝不静默丢弃——`leftover_steering_survives_into_the_next_run` 钉住）。
- `SubmissionKind{NewTurn,Steering}` 类型面区分两类提交（任务书"怎么做"第 2 条）。

### 配额管理器（`rust/crates/lingxi-service/src/quotas.rs`，新）

- `QuotaLimits{model,tool: LayeredQuotaLimits{global=8,per_agent=4,per_session=1|2}, wait_queue_capacity=64, wait_timeout_ms=30_000, max_agent_lanes=256, max_session_lanes=4096}`——全部经 `ServiceDeps.quota_limits` 注入，退化值响亮拒启。
- `QuotaManager::acquire(resource, agent_id, session_id)`：**global → agent → session 固定顺序**三层获取（任何一层失败 RAII 释放已持有层；层容量≥1 保证无死锁）。等待者 FIFO 有界（`QueueFull` 拒绝=任务书"超限按协议拒绝或排队"的拒绝半边）；等待带超时（`TimedOut`）。
- **取消安全无泄漏**（granted 标志+队列成员只在 lane 锁内变更）：等待 future 被 drop/abort → `WaitGuard::drop` 归还队列位；**授权与取消竞态**（release 恰在 waiter drop 前送达）→ guard 检测 granted=true 归还槽位。单测 `cancelled_waiter_releases_its_queue_place_and_racing_grant` 专证该竞态零泄漏；超时分支同样先查 granted（拿到就持有返回，不泄漏）。
- `QuotaPermit` RAII：调用结束/错误/超时/取消（future drop）全额归还三层。

### 驱动链接线（修改）

```
HTTP POST /lingxi/v1/sessions/{id}/execute                    (lib.rs execute_session)
  → SessionStore::execute_for                                  (sessions.rs)
      归属/NotFound/Forbidden（R02 原语义不变，先于一切副作用）
      → gate.try_begin_run                                     ← busy → 409 session_busy（retryable，零副作用）
      → allocate_run_id（拒绝路径不达此步）
      → RunSupervisor::drive_run(…, agent_id, steering=lease.inbox())   (runs.rs)
          每 turn 前：drain steering → acquire(Model: global→agent→session)
            配额失败 → break Failed{QuotaExhausted} → 单一 finalize
          每 tool call：acquire(Tool) → persist started → tools.execute（permit 只罩 I/O）
            → drop permit → persist completed
      → finalize_settlement（T01 唯一 finalize 路径，未动）
  lease Drop → 会话空闲（所有退出路径）
```

- kernel（`lingxi-kernel/src/lib.rs`）：`QuotaResource{Model,Tool}` + `FailureCause::QuotaExhausted{resource}` + `reason_segment()`；terminal_reason 新词表 `failed.quota_exhausted.model|tool`（不与 provider 失败混同；穷举测试扩 2 例）。RunStatus wire 零变化。
- `RunSupervisor::new` 增 `QuotaManager` 参数（启动校验两项）；`without_provider()` 默认有界配额。`ServiceDeps` 新增 `quota_limits`/`session_concurrency`（Default=生产默认；Debug 面）。
- steer 面说明：steer 的传输入口在 Node 是 WS 消息（`msg.type==="steer"`），Rust 栈 WS 目前仅事件订阅——**本 Task 不发明新传输路由**，`steer_for` 为服务层 API（R06 会话语义/R07 终端接入时挂 WS 面）；HTTP execute 面对 `SteeringInboxFull` 的映射已备好（closed match）。

## 3. 逐验收：预期 vs 实测

### R03-A03 同会话顺序可重复 — **PASS（本机隔离环境，确定性替身，可控调度）**

- 前置：同一会话（sess_local_alpha）同时提交两个任务。替身 `GatedProvider`（按会话分发脚本，测试用 0-permit 信号量逐 turn 门控；单线程 current_thread runtime，零 sleep）。
- 实测（`session_serialization.rs::r03_a03_same_session_serializes_and_steers_per_frozen_semantics`）：
  - **按冻结语义**：提交1 占据会话（turn1 到达即忙）；提交2（普通）→ `SessionExecuteError::Busy`，**零 durable 副作用**（runs 恰 1 行、key_events 恰 7 条，被拒者未 allocate run id）；同刻 steering 提交 → **Accepted**（不打断循环）。
  - **可控调度推进**：释放 turn1 门 → 驱动 drain steering → turn2 的 provider 观测 input 含 `[steering] focus on the config file`（steer 到达**下一**模型调用——Node 语义）；释放 turn2 → 单一终态 `completed.with_final`。
  - **消息不交叉写入**：durable key_events 按 seq 全序恰为 `run_state_changed, mc×2(turn1), mc×2(turn2), run_state_changed(终态), final_message_committed`——胜出 run 的写入连续、无第二 run 穿插（不存在第二 run）；结算后该会话下一次普通提交被接受。
  - 调度轨迹证据：`artifacts/rust-tauri/R03/T02-E01/a03-a04-session-serialization.log` 的 `R03_A03_TRACE 1..5` + `r03-t02-evidence.json`（机器可读：acceptedRuns=1/busyRejections=1/steering.reachedTurn=2/durableEventsOfWinner 全序）。
- 附加：`leftover_steering_survives_into_the_next_run`（迟到 steer 不丢、进入下一 run 首 turn input）、`busy_rejection_writes_nothing_and_steering_is_distinct`（store 层：Busy 零副作用 + steer Accepted/Miss/跨主体 Forbidden）、sessions.rs 内部 64 并发（current_thread 确定性：恰 1 接受 + 63 Busy + 顺序 8 连发 id 不坍缩）。

### R03-A04 跨会话不被全局锁阻塞 — **PASS（并发及配额断言；取消边界如实声明）**

- 前置：会话A（alpha）工具暂停（`GatedTool` 在 0-permit 信号量上挂起，持有唯一 global tool permit），会话B（beta）纯文本。注入配额 tool.global=1、model.global=2。
- 实测（`r03_a04_cross_session_parallel_and_quota_release_on_cancel`）：
  - **B 正常结束且 A 仍挂起**：B 全链完成（`completed.with_final`）时 `in_use(Tool)==1`（A 仍持有）、`in_use(Model)==0`——模型/工具 I/O 不在任何全局锁内等待（A04 的核心断言）。
  - **取消 A 后配额最终释放**：终止原语= **tokio `JoinHandle::abort()`**（在 await 点 drop 驱动 future——今日真实存在的取消原语，任何被断开的 HTTP 连接同样触发）。abort 后：`in_use(Tool)==0`、`in_use(Model)==0`、alpha busy slot 释放。
  - **诚实边界（派单要求明示差异）**：A 的 run 行**保持 `running`、无 terminal_reason**——用户取消入口/取消树/`cancelled` 终态落地属 **R03-T03**（T01 报告 §8 确认未建，本 Task 不伪造取消终态）。abort 走的正是 T03 取消将复用的 RAII 释放路径。
  - 姊妹证明（无取消的错误路径）：`failed_tool_path_settles_and_returns_quotas`——工具替身报错 → run 收束 `completed.no_final.tool_partial_failure` → tool/model 配额全归零、会话释放。
  - **配额耗尽响亮收束**：`quota_exhaustion_fails_loudly_and_releases_on_settle`——A 持唯一 tool 槽时 B 的工具 run 有界等待 500ms 超时 → `failed.quota_exhausted.tool`（单一 finalize、不悬挂不假成功）；释放 A 后槽归还、A 正常完成。
  - 配额管理器单测 6 例：FIFO 释放、超时无幽灵等待者/无泄漏槽、**有界等待队列满即拒**、**取消+授权竞态归还**、per-agent 层独立、退化配置拒启。
- 证据：同 log 的 `R03_A04_TRACE 1..3` + evidence JSON（afterCancel 计数、aRunRowHonest.status="running"）。

### 任务书"怎么做"1–4 对照

1. 每会话一个 owner（SessionLease），I/O 不在锁内等待（gate 锁 O(1) 不跨 await；permit 语义=许可而非互斥锁，A04 证明跨会话并行）✔
2. 排队/追问/打断语义保留并区分（§1 表；不发明交互，steer 传输面不提前造）✔
3. 全局/agent/session 模型+工具配额；有界队列；超限按协议拒绝（409/QueueFull/超时）或排队（FIFO 有界等待）✔
4. 带超时的等待可取消（WaitGuard/cancel-safety）；取消/失败/超时归还配额（RAII；A04 三路径实测）✔

## 4. 修改文件清单

修改（6）：`rust/crates/lingxi-kernel/src/lib.rs`、`lingxi-service/src/{lib.rs,runs.rs,sessions.rs}`、`lingxi-service/tests/{event_subscription.rs,execute_concurrency.rs}`。
新增（3）：`lingxi-service/src/{quotas.rs,session_supervisor.rs}`（生产代码）、`lingxi-service/tests/session_serialization.rs`（永久测试）。
`rust/Cargo.lock` 零变化（`90111c4b…`=R02_HANDOFF）；`lingxi-service/Cargo.toml` 零变化（曾试加 tokio test-util dev-dep，因环境干扰移除——见 §8；最终候选无依赖变化）。逐文件 SHA-256 见 `candidate-summary.txt`。

### 既有测试夹具改动（逐处+理由；无断言删除/放宽/改永真，无 skipped）

1. **`sessions.rs::concurrent_executes_never_collapse_into_one_run_id`（重写为 `concurrent_executes_serialize_one_accepted_and_no_run_id_collapse`）**：R02 语境"64 并发同会话全部接受"在 R03-T02 冻结语义下不再成立（第二个普通提交=409）。新形态：64 并发 → **恰 1 接受 + 63 Busy**（被拒零副作用）+ 8 次顺序快发 → id 全不坍缩。**F01 保护保留**：所有被接受提交 id 唯一（原子分配器不坍缩）。
2. **同文件 FakePort::record_run_started 加一个 `yield_now().await`**：R02 期替身全同步（busy 窗口零宽，串行化不可观测）；补一个 await 点模拟真实 RunDatabase 必有的存储往返。真实后端并发矩阵在 execute_concurrency.rs 不受影响。
3. **`execute_concurrency.rs` 两个并发测试重写**（同因由，语义变更如实映射）：in-process 64 并发 → 1 接受+63 Busy（current_thread 确定性）+ 64 顺序全接受 distinct + 重启再 64 顺序 distinct 无重叠（F01+reseed 保护原文保留）；HTTP 64 并发 → 每个 200 带 distinct runId、其余 409 `session_busy`+`retryable:true`（传输到达序决定精确切分，**确定性 1+63 归服务层测试**——如实标注）+ durable 事实与接受数严格一致 + 8 顺序全接受。`http_running_storage_fault…` 未动。
4. **`event_subscription.rs::a09_snapshot…` 双 writer 改写两会话**（w0→alpha、w1→beta，alpha 流边界 k=0..=4）：R02-A09 的保护对象（真实并发提交下快照+订阅无间隙）不变；原"两 writer 同会话"是被冻结语义改变的用法。其余 11 测试未动。
5. `run_lifecycle.rs`/`service_persistence.rs` **零改动**（execute_for 外部签名未变）。

## 5. 验证命令与退出码（全部经 rustup 1.98.1 + `--locked`；verify-stage R03 未注册，未运行未伪造）

| 命令 | 退出码 | 结果 |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | 无 diff |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 0 error/warning |
| `cargo test --workspace --locked` | 0 | **499 passed / 0 failed / 48 suites ok**（含 R02 全量回归；T01 为 482/0） |
| `cargo run -p xtask -- check-contracts` | 0 | 56 生成文件 + API_COMPAT_MATRIX 626 entries 零漂移 |
| `cargo run -p xtask -- check-boundaries` | 0 | DEP-07/08/D5 PASS |

定向过滤器（全部命中>0，见 filters.log）：quotas 6、session_supervisor 5、session_serialization 5、sessions:: 8、execute_concurrency 3、run_lifecycle 12、event_subscription 12、service_persistence 2、lingxi-kernel lib 22、run_finalize_property 2、storage_transactions 7、run_id_allocator 5、event_store_reads 3、backup_restore 11。

并发测试不以串行化掩盖竞态：A03/A04/串行化组用单线程 runtime + 门控替身（确定性调度），A04 中 B 与挂起的 A **真并行推进**；配额单测直接驱动 FIFO/超时/取消竞态。

## 6. 证据位置

`artifacts/rust-tauri/R03/T02-E01/`：`commands.log`（命令+退出码+过滤器清单）、`workspace-test.log`（499/0 原始输出）、`clippy.log`、`fmt-check.log`、`check-contracts.log`、`check-boundaries.log`、`filters.log`（14 组定向过滤命中数）、`a03-a04-session-serialization.log`（R03_A03/A04_TRACE 调度轨迹 + 5 passed）、`r03-t02-evidence.json`（schema 无版本化键：a03/a04/error_path/quota_exhaustion/leftover_steering 五组机器断言）、`candidate-summary.txt`（基线/HEAD + 逐文件 sha256 + Cargo.lock 零变化）。

## 7. 测试替身边界

- 允许侧：`GatedProvider`/`GatedTool` 仅在测试选定时刻产生外部响应（`ProviderTurn`/`ToolOutcome`），经 `ServiceDeps` 注入**真实**组合根——真实 execute 入口、真实 SessionSupervisor/QuotaManager/RunSupervisor、真实内核状态机/finalize、真实 RunDatabase 单写者事务、真实事件发布。替身不写状态、不落库、不 finalize、不参与身份 mint。
- 取消原语侧：tokio `abort()` 是运行时真实取消（await 点 drop 整个驱动 future 栈），非替身模拟；A 的存储状态如实留在 `running`。
- 生产侧：无 Provider 时行为不变（`completed.no_final.no_provider_configured`，2 events/run 不变量保持——execute_concurrency/持久化断言原文仍绿）。

## 8. 未验证项 / 边界（如实）

- **真实用户取消传播归 R03-T03**：A04 的"取消A"用 tokio abort（现存唯一真实终止原语），未伪造 `cancelled` 终态；A 的 run 行停留 `running`（中断恢复分类归 T07）。
- steer 的 **WS 传输入口**与 **runSplit 历史投影**（durable 用户消息）= R06/R07；本 Task 交付服务层通道与区分类型。steering 文本在 R03 无 durable 用户消息行（模型 input 为消费面）——已在模块文档与本报告声明，不宣称已持久化。
- waiting_approval、取消树、迟到栅栏、收据、恢复协调器 = T03/T04/T05/T07，未提前实现。
- **环境观察（重要，供后续任务避坑）**：给测试二进制新增 tokio `test-util` feature（改变二进制内容）曾使 `r00_management_leaves` 的 LAN 自连用例 100% 停摆（连接建立、写成功、0 字节回读——本机 utun8=198.18.0.1 代理 TUN + en0 路由 reject 标志的环境拦截，即 R02-RUNNER-ENV-INTERFERENCE 类）；对照实验：基线 3/3 过、基线+仅该 feature 1/1 挂。已改用无 feature 的等价确定性方案（事件驱动门控 + 两个有界真实超时 100/500ms），最终候选全量 499/0。**后续任务如需改 tokio feature 或重编译该测试二进制，预期会再现此环境拦截，非代码缺陷。**
- 无真实供应商、无跨平台（本机 arm64 macOS）。

## 9. 独立复核重点建议

1. 冻结语义映射忠实性：`session_busy` 409+retryable+零副作用、steer Accepted/Miss/有界/残留、无服务端普通输入队列（对照 Node 源码点位 §1 表）。
2. 配额取消安全：`WaitGuard` 对"授权竞态"的归还（quotas.rs Drop 实现 + 竞态专测）；三层获取顺序与部分失败释放。
3. A04 断言是否伪造取消（应无：run 行 `running`、无 terminal_reason；abort 为运行时原语）。
4. 夹具改动 §4 四处是否降低 R02 公开保护（F01 id 唯一性、A09 无间隙、busy 零副作用均应原文级保留）。
5. `event_subscription.rs` A09 改两会话后，k 边界矩阵与权威对照是否仍覆盖原竞态窗口。
