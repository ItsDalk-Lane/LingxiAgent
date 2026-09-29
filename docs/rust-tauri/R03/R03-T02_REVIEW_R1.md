# R03-T02 独立验收报告（REVIEWER-R03-T02-R01，第 1 轮）

- VERDICT: **PASS**
- TASK_ID: R03-T02（会话串行化和全局限流）；ACCEPTANCE_IDS：R03-A03、R03-A04
- TASK_BASE_SHA：`8b2f2cd595625e17b4153af072efc8484b79d5b6`（分支 codex/rust-tauri-migration）
- 候选：基线 HEAD + 未提交工作树（修改 6 + 新增 3 rust 路径）
- 审查者：REVIEWER-R03-T02-R01（一次性独立验收代理；未参与本 Task 的实现/修复；验收期间代码冻结，未修改任何产品源码/测试/配置/阶段图）
- 审查时间：2026-09-29；环境：macOS darwin 27.0.0 arm64；rustup 锁定工具链 1.98.1（`~/.cargo/bin/cargo` 代理 + `--locked`；专用 `CARGO_TARGET_DIR=/tmp/r03-t02-review-target`；PATH 中的 Homebrew cargo 1.93.0 未使用）
- 复测产物：`artifacts/rust-tauri/R03/T02-R01-review/`（a03-a04 定向 + evidence 复算、filters、workspace-test、fmt/clippy/check-contracts/check-boundaries 日志）

## 1. 候选绑定核对（复测前 + 复测后各一次）

- tracked-diff sha256 前 16 位：`7af03d804b5c1736`（= 派单绑定值）。复测结束后复算仍为 `7af03d804b5c1736`，HEAD 仍为 `8b2f2cd59`；status 集摘要（排除派单文件与本轮验收产物目录）复测前后均为 `3405658c52284b4a`（= 派单绑定值）——候选在本轮验收期间未被改动，rust/ 路径集完全一致。
- 逐文件 SHA-256：6 修改 + 3 新增共 9 个文件与执行者 `artifacts/rust-tauri/R03/T02-E01/candidate-summary.txt` 全部一致（本审查者独立 shasum 复算）。
- `rust/Cargo.lock` = `90111c4b…`（= R02_HANDOFF dependency_locks，零变化）；`rust/Cargo.toml` 与 `lingxi-service/Cargo.toml` git diff 为空。**tokio `test-util` feature 未进入最终候选**：lingxi-service tokio features 仍为 `macros,rt-multi-thread,net,signal,io-util,time,sync`，全 rust/ 源码无 `test-util`/`pause()`/tokio 时间控制 API（`ManualClock::advance` 为 R02 既有注入面，非 tokio test-util）。执行者报告 §8 的环境发现（曾试加该 feature 导致 r00_management_leaves LAN 自连停摆、最终移除）与最终候选状态一致。

## 2. 真实接线追查（源码证据）

从真实入口到存储写者/事件发布/配额归还的完整链路逐文件核实（非枚举/演示）：

```
HTTP POST /lingxi/v1/sessions/{id}/execute        (lib.rs execute_session → state.runs())
  → SessionStore::execute_for                      (sessions.rs:347)
      会话归属/NotFound/Forbidden（R02 原语义，先于一切副作用）
    → gate.try_begin_run(session_id)               (session_supervisor.rs:259)
        busy → SessionExecuteError::Busy → 409 session_busy + retryable:true
               （lib.rs EndpointError::session_busy:1086；拒绝发生在 allocate_run_id 之前，
                 零 durable 副作用——A03/execute_concurrency 断言 runs/key_events 计数证实）
        RegistryFull → 503
    → allocate_run_id（仅胜出者）
    → RunSupervisor::drive_run(…, agent_id, steering=lease.inbox())   (runs.rs)
        每 turn 前：SteeringInbox::drain_joined → turn_input = "{input}\n\n[steering]\n{text}"
        每 turn：acquire(Model: global→agent→session) → provider.next_turn → drop permit
        每 tool call：acquire(Tool) → persist started → tools.execute → drop permit → persist completed
        配额失败 → break Failed{QuotaExhausted{resource}} → 唯一 finalize 路径（T01 未动）
    → lease Drop（任何退出路径：正常/错误/超时/future 取消）→ 会话空闲；残留 steering 保留
```

关键正确性判断（源码级）：

- **单一 owner**：`try_begin_run` 在 slots 锁内检查/置 busy 并返回 `SessionLease`（RAII）；Drop 释放 busy、无残留 steering 时移除槽位。每 busy 会话恰一个 lease。
- **锁不跨 I/O**：`SessionSupervisor`/`SteeringInbox`/`QuotaManager` 全部内部锁为 O(1) 同步操作，均在 await 前释放（逐处核对 `acquire_one_lane`/`try_begin_run`/`steering_submit`/`drain_joined`，无 MutexGuard 跨 await）；模型/工具 I/O 期间只持有 RAII admission permit（许可而非互斥锁），A04 证明其他会话全链推进。
- **等待队列有界、超时可拒**：`waiters.len() >= waiter_cap → QueueFull`（响亮拒绝）；`tokio::time::timeout(wait_timeout_ms)` → `TimedOut` → run `failed.quota_exhausted.{model,tool}`（kernel `reason_segment`，单一 finalize）。
- **三层获取顺序固定** global→agent→session、每层容量 ≥1（0 拒启）：统一顺序 + RAII 释放 + 有界等待，无循环等待死锁面。
- **取消安全零泄漏**（quotas.rs `WaitGuard`）：granted 标志与队列成员只在 lane 锁内变更；等待 future 被 drop → 未授权则出队、已授权（release 竞态送达）则归还槽位并队首移交；超时分支先查 granted（竞态拿到即持有，不泄漏）。专测 `cancelled_waiter_releases_its_queue_place_and_racing_grant` 逐段驱动该竞态。

## 3. Steps / Deliverables 逐项核对（任务书 R03-T02）

| 步骤/交付 | 结论 | 证据 |
|---|---|---|
| 1 每会话一个明确 owner；I/O 不在全局锁中等待 | 满足 | `SessionLease` RAII（§2 链路）；A04：A 挂起在工具 I/O 时 B `completed.with_final`、`in_use(Tool)==1`、`in_use(Model)==0` |
| 2 保留已采纳排队/追问/打断语义；区分普通提交与 steering | 满足 | §4 冻结语义逐点位核实（Node 源码 5 处 + P02 文档）；`SubmissionKind{NewTurn,Steering}`、`SteerOutcome::Accepted/Miss`、busy→409 retryable 零副作用；steer 不打断循环（turn2 input 含 `[steering]`）、残留 steer 跨 run 保留（`leftover_steering_survives_into_the_next_run`） |
| 3 全局/agent/session 模型+工具配额；有界队列；超限按协议拒绝或排队 | 满足 | `QuotaManager`（model/tool × global/agent/session；FIFO 有界等待 = 排队半边，QueueFull/TimedOut = 拒绝半边）；lane registry 上限 + 空闲驱逐；退化配置 6 用例拒启（lib.rs `validate_resource_deps`）；HTTP 面 409/503/409(inbox) 全映射 |
| 4 带超时等待可取消；取消/失败归还配额无泄漏 | 满足 | WaitGuard 语义（§2）；A04 abort 后 tool/model in_use 全 0 + lease 释放；错误路径（tool 失败）与耗尽路径（超时 failed.quota_exhausted.tool）均归还（evidence JSON 复算一致） |
| 交付 SessionSupervisor / 并发排队策略 / 配额管理器 | 满足 | `session_supervisor.rs`（476 行）、冻结语义映射（§1 表 + 模块文档）、`quotas.rs`（850 行） |

## 4. 冻结语义溯源核对（报告 §1 的 Node 证据逐处验证）

本审查者独立读取声称的全部 文件:行，语义逐条一致：

1. `core/desktop-session-submit.ts:455-460`：`pendingDesktopSessionSubmissions.has(...)` / `engine.isSessionStreaming(...)` → `notAcceptedError("session_busy")` — 属实（当前源码行 454-460）。
2. `server/routes/sessions.ts:1539`：`return c.json({ error: "session_busy" }, 409)` — 属实。
3. `server/routes/chat.ts:559-561`：`err?.message === "session_busy"` → `{ code: "session_busy", retryable: true }` — 属实（报告引 560）。
4. `core/session-coordinator.ts:5294-5301`：`steerSession` — `if (!entry?.session.isStreaming) return false;` → `entry.session.steer(text)` — 属实（isStreaming 才接受，miss 返回 false）。
5. `server/routes/chat.ts:2445-2461`：WS `msg.type === "steer"`；miss 时 `msg.type = "prompt"` 降级 — 属实。

交叉验证 P02 冻结文档：`docs/refactor-2026/P02/CONCURRENCY_RULES.md` §3「用户输入→Pi 会话：session_busy 门禁 + steer 语义（不打断循环）」、§4「超限行为均为显式拒绝或排队（limiter），无无界队列」；`STATE_TRANSITIONS.md` R5「steer 落盘 runSplit=true，只切 Run 语义层，真正终结仍归 R4」。执行者对「已采纳排队 = 客户端可重试、服务端无普通输入队列」的读法与现役源码一致，无发明交互；runSplit（durable 用户消息投影）如实声明为 R06、WS steer 传输入口声明为 R06/R07（当前 Rust WS 仅事件订阅，未新造传输路由）——见 §8-O2。

## 5. 验收场景复测（本审查者真实重跑，全部经 rustup 1.98.1 + --locked）

### R03-A03 同会话顺序可重复 — PASS（复现）

- `cargo test -p lingxi-service --test session_serialization`：**5 passed / 0 failed，exit 0**（两遍：默认 + `--nocapture` + `R03_T02_EVIDENCE`）。
- 正主场景 `r03_a03_same_session_serializes_and_steers_per_frozen_semantics`：门控替身（0-permit 信号量 + arrivals 通道，current_thread runtime，零 sleep）下——提交1 占据会话；普通提交2 → `Busy` 且 **runs 恰 1 行、key_events 恰 7 条**（被拒者未 allocate run id）；steering → Accepted；释放 turn1 门后 **turn2 provider 观测 input 含 `[steering] focus on the config file`**（steer 到达下一模型调用，循环不终止）；终态唯一 `completed.with_final`，durable 事件全序恰 7 条且为胜出 run 连续写入；结算后下一次普通提交被接受。调度轨迹 `R03_A03_TRACE 1..5` + `R03_A04_TRACE 1..3` 共 8 行真实落盘。
- **机器证据逐字节复现**：本审查者以独立 evidence 文件重跑，`r03-t02-evidence-review.json` 与执行者 `T02-E01/r03-t02-evidence.json` **完全一致（diff 为空）**——含确定性 run id，复现性充分。
- 附加：`busy_rejection_writes_nothing_and_steering_is_distinct`（Busy 零副作用 + Accepted/Miss/跨主体 Forbidden）、sessions.rs 内部 64 并发（恰 1 接受 + 63 Busy）+ 8 连发 id 不坍缩（复跑 8/8 绿）。

### R03-A04 跨会话不被全局锁阻塞 — PASS（复现；取消边界如实）

- `r03_a04_cross_session_parallel_and_quota_release_on_cancel`：注入 tool.global=1/model.global=2；会话A 挂起在 GatedTool 的 0-permit 信号量（持唯一 global tool permit + alpha lease）时，会话B 纯文本全链完成（`completed.with_final`），此刻 `in_use(Tool)==1`（A 仍持）、`in_use(Model)==0` ——**跨会话不被任何全局锁阻塞的核心断言成立**（A 的 I/O 等待只阻塞 A 自己）。
- **取消A → 配额最终释放**：`a.abort()` 后 `in_use(Tool)==0`、`in_use(Model)==0`、alpha lease 释放；A 的 run 行**如实停留 `running`、无 terminal_reason**（不伪造 cancelled 终态）。
- 姊妹路径全绿：`failed_tool_path_settles_and_returns_quotas`（工具失败 → `completed.no_final.tool_partial_failure`，配额归零）；`quota_exhaustion_fails_loudly_and_releases_on_settle`（真实有界 500ms 超时 → `failed.quota_exhausted.tool` 单一 finalize，释放后槽归还、A 正常完成）。

**A04 abort 边界裁定（派单重点复核项）**：A04 字面要求为「B 正常结束，A 的配额最终释放」，二者均有真实运行时断言；其并不要求 cancelled 终态落地——取消树/用户取消入口属 R03-T03（任务书 §4 T03；已核实当前无任何取消路由，T01 报告 §8 亦确认未建）。tokio `abort()` 是本阶段**真实存在的**终止原语（await 点 drop 整个驱动 future 栈），且走的正是 T03 取消将复用的 RAII 释放路径（SessionLease Drop / QuotaPermit Drop / WaitGuard Drop 同一条链）；执行者在模块文档、报告 §3/§7/§8 与 evidence JSON `cancellationPrimitive` 字段三处明示与 T03 的差异。**该边界声明如实，不构成 A04 断言缺口**；「run 行停留 running」的中断恢复分类是 T07 已登记义务（见 §8-O3）。

### 门禁与全量（退出码实录，本审查者复跑）

| 命令 | 退出码 | 结果 |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | 无 diff |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 0 error/warning |
| `cargo test --workspace --locked` | 0 | **499 passed / 0 failed / 0 ignored**（42 测试二进制 + 6 doc-tests；= 执行者声称值） |
| `cargo run -p xtask -- check-contracts` | 0 | API_COMPAT_MATRIX 626 entries 零漂移 |
| `cargo run -p xtask -- check-boundaries` | 0 | DEP-07/08/D5 PASS |

定向过滤器命中数全部 >0 且与执行者 filters.log 一致：quotas 6、session_supervisor 5、session_serialization 5、sessions:: 8、execute_concurrency 3、run_lifecycle 12、event_subscription 12。逐 suite 对照 T01 基线（482/0）与本候选（499/0）：**+17 = quotas 6 + session_supervisor 5 + session_serialization 5 + sessions 内部 +1（新 `busy_rejection_writes_nothing_and_steering_is_distinct`；`concurrent_executes…` 原位重写）**，其余全部 suite 数量逐一相同——无任何测试被删除或静默移除。复跑无残留进程、无遗留 `/tmp/lingxi-r03t02-*` 目录。

## 6. 测试替身与质量审计

- 替身边界合规（R03_SCOPE_MATRIX test_double_boundary）：`GatedProvider`/`GatedTool` 仅实现 `TurnProviderPort`/`ToolExecutorPort`（在测试选定时刻产生外部响应），经 `ServiceState::bootstrap_with_deps` **真实组合根**注入——真实 execute 入口、真实 SessionSupervisor/QuotaManager/RunSupervisor、真实内核状态机与唯一 finalize、真实 RunDatabase 单写者事务、真实事件发布；替身无状态书写面、不落库、不 finalize、不参与身份 mint；验收断言直查 SQLite（`query_one_text`），非替身自报。
- 无 mock 掉待测核心、无空集合断言（burst 断言精确 1+63；evidence 断言非空计数）、无 `#[ignore]`（0 ignored）、无断言改永真、无 skipped。
- **串行化未掩盖业务竞态**：A03/A04 用单线程 runtime + 门控替身实现**可控调度**（真并发交织：A 真实挂起在 I/O 等待、B 真实推进完成），非缩减并发；配额单测直接驱动 FIFO/超时/取消-授权竞态；in-process 64 并发 burst 的确定性 1+63 依赖存储队列往返顺序（每个提交的 lookup 排在胜出者写入之前），非 `serial_test` 类串行化；HTTP burst 明确接受非确定切分（accepted+busy=64、durable 事实与接受数严格一致、顺序 8 全接受），把精确 1+63 定数交给服务层确定性测试——如实标注（§8-O4）。
- 测试启动的临时 home/进程全部自清理（复跑后核实无残留）。

## 7. R02/R01 回归保护核对（4 处夹具改动逐处判断）

1. **sessions.rs 内部并发测试（重写）**：R02 语境「64 并发全接受」与 T02 冻结语义（busy 即拒）直接矛盾，语义变更为任务书本身要求。F01 公开不变量保留并加强：胜出 run + 8 连发同毫秒提交 → 9 个唯一 id（HashSet 尺寸断言）+ run_count=9；被拒者零副作用以 runs/key_events 计数断言。**保护未降**。
2. **FakePort::record_run_started 加一个 `yield_now().await`**：R02 期替身全同步使 busy 窗口零宽、串行化不可观测；补一个 await 点模拟真实存储往返。真实后端并发矩阵（execute_concurrency.rs）不受影响，`http_running_storage_fault…` 未动。合理。
3. **execute_concurrency.rs 两测试重写**：F01 + restart reseed 保护原文级保留（in-process：1+63 → 65 唯一 id → 重启再 64 唯一且与首轮零重叠；HTTP：每个 200 均唯一 runId、409 均含 `session_busy`+`"retryable":true`、durable 事实 = accepted×2 事件、顺序 8 全接受）。「每 run 恰 2 key events」不变量保持（130/129 计数换算正确）。
4. **event_subscription.rs A09 双 writer 改两会话**（w0→alpha、w1→beta）：保护对象（真实并发提交下快照+订阅无间隙）不变；alpha 流 4 事件、k=0..=4 覆盖该流**全部**边界（原矩阵对 8 事件覆盖 0..=8，同构全覆盖）；提交与订阅者的真实竞态窗口保留；原「两 writer 同会话」恰为冻结门禁现在拒绝的用法。**保护未降**。
5. run_lifecycle.rs / service_persistence.rs 零改动（git diff 证实）。

R02 全量回归在全工作区 499/0 内通过；R02 各 suite（auth_matrix 24、storage_transactions 7、event_store_reads 3、backup_restore 11、instance_lifecycle 3、shutdown_coordinator 7、r00 两叶、spike 组等）计数与 T01 基线逐一相同。

## 8. 执行者报告核对结论与建议性观察

**报告可验证声称全部属实**：候选摘要与逐文件 hash、Cargo.lock/Cargo.toml 零变化、tokio test-util 未进入最终候选、5 处 Node 语义点位、全部命令退出码与命中数、499/0、626 entries、evidence 内容（逐字节复现）、A04 run 行 `running` 的如实呈现。**无虚报**；无 NOT_REPRODUCED / NOT_A_DEFECT 反证条目。

一处**报告措辞级小误差**（非缺陷）：报告 §2 称 `validate_resource_deps`「扩展 7 个退化用例」，实际新增 **6** 个（quota 4 + session 2，lib.rs 逐条清点）；全部退化旋钮（各层容量/wait_queue_capacity/wait_timeout_ms 上下界/steering_inbox_capacity/registry_cap）确有拒启覆盖，义务本身满足。

建议性观察（非验收缺陷，不阻塞；与真实验收缺陷分开）：

- **O1（agent-lane 注册表驱逐的窄 TOCTOU，建议 R04/R05 顺手加固）**：`quotas.rs::lane_from` 在注册表达上限驱逐空闲 lane 时，另一任务可能已从 `lane_from` 拿到该 lane 的 Arc 但尚未 enqueue（此刻 in_use==0、waiters 空，可被误逐）——同 key 可能短暂存在两个 lane 对象，极端情况下 per-agent 上限被临时放宽（global 层不受影响，且每 lane 仍有界等待）。触发需 256 个并发 agent 键达上限 + 同 key 精确交错；session 层被 busy gate（同会话串行）结构性排除。与 R02 RateLimiter registry-cap 先例同构。建议：驱逐时跳过「最近创建」的 lane 或在 lane_from 返回前预占一个槽。
- **O2（已声明的递延，登记勿丢）**：steer 的 WS 传输入口与 runSplit durable 投影 = R06/R07；steering 文本当前仅内存通道 + 模型 input 消费面（无 durable 用户消息行）。已在模块文档/报告 §8 如实声明，属后续阶段义务。
- **O3（abort 后的 run 行 `running`）**：诚实行为；中断/恢复分类归 T07 启动扫描（非终态 run → interrupted/needs_attention）。busy gate 已随 lease 释放（会话可接受新提交），durable `running` 行无内存 owner——T07 落地前这是已知中间态，报告已声明。
- **O4（HTTP burst 非确定切分）**：测试如实标注传输到达序决定精确切分、确定性 1+63 归服务层；durable 一致性断言已闭合保护。保持现状即可。
- **O5（环境观察）**：tokio feature 变更会触发本机 TUN 代理对 r00_management_leaves LAN 自连的拦截（执行者对照实验：基线 3/3 过、仅加 feature 1/1 挂）；最终候选未引入该变更，本轮全量 499/0 复跑亦通过。该发现对后续任务有真实避坑价值。

## 9. 范围边界（T03+ 未提前实现 = 正确）

未发现越界：无取消树/CancellationToken/TaskSupervisor/清理期限（T03）、无迟到栅栏/requestId 去重（T04）、无 InvocationJournal/副作用收据（T05）、无后台/子代理接口（T06）、无恢复协调器/退出策略（T07）、无 R03 stage map（`rust/crates/xtask/src/stage_maps/` 仅 R02.json；verify-stage R03 未注册、未运行、未伪造）。diff 中无相关关键词；npm/桌面栈零触碰；生产默认入口未变。

## 10. 结论

R03-T02 当前到期义务（怎么做 1–4、三项交付物 SessionSupervisor/并发排队策略/配额管理器、A03、A04）均有真实有效证据；真实接线成立（HTTP execute → SessionSupervisor lease → RunSupervisor drive → 三层配额 → T01 唯一 finalize，锁不跨 I/O、队列有界、等待可取消、RAII 归还）；冻结语义映射经现役 Node 源码逐点位核实忠实；R02/R01 回归满足且 4 处夹具改动未降低公开保护（F01 id 唯一性、A09 无间隙、busy 零副作用均原文级保留）；无越界实现；无未关闭的验收阻塞缺陷。

**VERDICT: PASS**
