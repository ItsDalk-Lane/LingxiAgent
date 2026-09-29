# R03 修复轮 G02-E01 执行报告（F03：取消接受与完成提交的统一竞争裁决）

- 执行代理：EXECUTOR-REPAIR-R03-G02-E01（一次性执行/修复代理；本报告为执行者口径，不含独立审查）。
- 日期：2026-09-30。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。
- 基线 `FIX_BASE_SHA=cd3fb19e651f763afc6c75cb3163064fb54ca3fe`；候选起点 `520bb75b9906120c78b0d10036577fd911a5a81c`（含已通过独立审查的 G01 修复——本轮**未回退、未破坏**：cancel_link_inheritance 7/0、subagent_closeout 8/0、cancellation_tree 8/0 复跑全绿）。本轮无 commit/push（未获授权）。总控账本（`R03_FIX_ISSUES.json`、`R03_FIX_COMMIT_RECEIPTS.json`）未改动。
- 工具链：`~/.cargo/bin/cargo`（rustup 锁定 1.98.1），全部 `--locked`；`rust/Cargo.lock` sha1 `3b659f41eb262eaf42efc2f91ec93f9989d42934` 与 HEAD 相同（零依赖变化）。
- 证据根：`artifacts/rust-tauri/R03/repair-current/G02-E01/`（`normal-selfcheck/`、`adversarial-selfcheck/`、`logs/`）。
- 结论：**READY_FOR_REVIEW**（workspace 66 suites / 666 passed / 0 failed ≥ 底线 65/648/0；fmt 零 diff；clippy `-D warnings` 零告警；check-contracts / check-boundaries 额外回归 exit 0）。

## 1. 实现范围

F03（取消接受与完成提交没有统一竞争裁决）及其同根因路径。不涉及 F04–F07，不进入 R04。

## 2. 根因复核（结论：审查属实，已用隔离 worktree 红基线实测复现，无反证）

`drive_run` 的终态提交链是「fence 检查（provider 返回瞬间）→ 若干异步存储写（persist_model_call / journal 推进 / 状态腿）→ 循环 break → 无条件 `finalize_settlement`」。`cancel_run_for` 是「读 durable 行（active）→ 内存 fire（scope 标志 + Requested 相位）→ 返回 Accepted」。两条链没有任何共同裁决点：

1. fence 在 Provider 返回后检查 root，但 `persist_model_call` 是异步存储；Final 分支在其后 break `CompletedWithFinal`，循环末尾无条件 finalize 提交成功 outcome——「fence 通过—模型事件存储—最终提交」窗口内 Accepted 的取消随后仍落 completed + final message（F03-C01 红基线实测复现）。
2. 失败终态同窗口落 `failed`（`cancel_accepted_before_the_terminal_claim_beats_the_failed_terminal_too` 红基线复现）；无 Provider 早期收口同窗口落 `completed.no_final.no_provider_configured`（`cancel_at_the_no_provider_early_close_settles_cancelled` 红基线复现）。
3. `CancelRegistry::fire` 非原子：读相位 → scope.cancel → 写 Requested。两个并发 fire 都可读到 Active → 双 `Fired` + 第二 reason 覆盖第一（相位 reason ≠ scope 首因）；fire 的写还可把驱动已推进的 `Cleaning` 倒退回 `Requested`（F03-C03，宽树 hammer 红基线复现 96×4 并发）。
4. 派发边界无「恢复后重检」：取消在存储/授权边界 await 期间被接受后，恢复执行仍推进 journal 到 `started`（为一个从未派发的调用写 started 意图——事实诚实性破坏，`cancel_at_the_intent_boundary...` 红基线以 Prepared 断言复现）、无 Provider 收口直接完成。外部调用本身在旧码多由 TaskSupervisor 的「spawn 时作用域已取消即丢弃」与派发后 select 兜住（started/intent 边界变体红在 journal 相位与终态，不在调用计数——如实登记）；子代理 delegation 派发在旧码是**无 select 保护的直接 await**（`launcher.dispatch`），是真实的「取消后新外部操作」窗口（G8 门现已覆盖，见 §3.4）。

**同族终态路径清单**（全部经循环末尾/早收口的同一 finalize，全部纳入裁决）：Final→CompletedWithFinal、空 Final/Empty→CompletedWithoutFinal、no-provider→CompletedWithoutFinal、ProviderFailed(不可重试)→Failed、QuotaExhausted(Model/Tool)→Failed、TurnBudgetExceeded→Failed、empty_tool_request_list→Failed、ToolExecutorUnavailable→Failed、model_call_child_* 监督失败→Failed。`InterruptedNeedsAttention` 仅由恢复面构造（recovery 走 `finalize_settlement` 公共路径、无活动 entry，不属驱动裁决面）。

## 3. 修复设计（最小完整）

### 3.1 统一线性化规则（冻结）

- **裁决状态**：`RunCancelEntry` 的相位（一个 Mutex）是取消请求与终态提交的**唯一串行化点**。
- **取消先赢**：fire 在持锁临界区内完成「读相位 → scope 树取消 → Active→Requested 写」。终态提交前，驱动以 `claim_terminal` 在同一把锁下原子取「终态权」：若已有取消（Requested/Cleaning/Confirmed/StopUnconfirmed/Abandoned，或**父树遍历已置 scope 标志而相位未反映**——对抗自查发现的缝，见 §5）→ 返回 `CancelledBy{首因}`，驱动把意图终态**改道四相取消结算**（durable cancelling 腿 → 有界 drain → cancelled 单次 finalize），不提交 completed/failed、不提交 final message。
- **完成先定**：claim 在无取消时记录新相位 `Settling{terminal}`（**不可撤销点**，在任何 finalize await 之前）。此后 fire → `FireOutcome::TooLate`（不谎报 Accepted、不承诺停止、**不**对已在结算的运行发射取消树）；会话面映射为新 `CancelRunOutcome::TooLate`。终态照常恰好一次提交；claim→提交之间的任意 await 不再产生窗口——**不是**「最后一个 await 前补一次 is_cancelled」，claim 与提交之间无需任何重查。
- **过晚的边界情形**：终态已 durable 而驱动刚好注销（fire 见 NotLive）时，`cancel_run_for` 重载 run 行：terminal → `AlreadyTerminal`（不再误报 DanglingActive）；仍是 active 才报 DanglingActive。

### 3.2 cancel.rs（+341 行，含 5 个新单元测试）

1. `CancelPhase::Settling { terminal }` 新相位（不可撤销点；`cancel_requested()` 对其为 false——正常结算的运行不进 verdict 记录，与 Active 同）。
2. `RunCancelEntry::fire`：整个决策一个临界区；Settling 先查（TooLate 早退，不碰树）；首因由 scope 树首写规则保护、**相位 reason 从 scope 读回**（两处永不分歧）；Active→Requested 的 CAS 在锁内（双 Fired、reason 覆盖、Cleaning 倒退三类交错按构造消灭——旧码的读→写间隙不再存在，无可泊车窗口）。
3. `RunCancelEntry::claim_terminal` + `TerminalAdjudication{Claimed, CancelledBy}`：Active（且 scope 未因父树取消置位）→ 写 Settling → Claimed；任意取消相位 → CancelledBy{首因}；Active 但 scope 已被父树遍历取消 → 相位补记 Requested 并 CancelledBy（G01 传播语义与 F03 裁决一致：**取消树下的运行永不完成**）；Settling 重复 claim → 保留首claim、error 级日志（驱动不变式违约，响亮）。
4. 锁序：fire/claim 嵌套 `phase → scope.*`，无任何 `scope.* → phase` 路径（驱动各腿顺序取锁）——无 ABBA。

### 3.3 runs.rs（+160 行）

1. `adjudicated_finalize`：所有非取消终态的**唯一**裁决+提交入口——claim（同步、原子）→ Claimed 走 `finalize_settlement`（原单事务路径不变）；CancelledBy 走 `settle_cancellation`（四相取消结算），返回的 `RunFinish::Cancelled` detail 追加 `superseded in-flight terminal: …`（被改道终态的可诊断审计注记，不是第二个终态）。
2. 接线：循环末尾终态、无 Provider 早收口两处改走 `adjudicated_finalize`；`settle_cancellation` 内部的 cancelled finalize 与恢复面的直接 `finalize_settlement` 公共路径不变（取消按定义已赢，无需再裁决；幂等重放语义原样保留）。
3. RegistrationGuard 语义不变：finalize 成功 → disarm；错误 → 树取消 + Abandoned（Settling 被如实覆盖为诚实终局）。

### 3.4 runs.rs 派发门（F03-C04，任务书 02 §5「执行前重检撤销/生命周期」）

`gate_cancel!()`（展开为与原 loop-top 检查相同的四相收口早退）置于 9 处：loop-top（原检查改写）、模型派发前（受理许可后）、tool_request 模型事件持久化后、每个工具循环迭代首、工具受理许可后（意图写前）、意图+started 事件写后（授权边界前）、审批询问前（waiting_approval 腿前）、delegation 派发前、`advance_invocation(Started)` 后（工具执行 spawn 前）。**定位声明**：这是「恢复后重检」门（驱动从边界 await 恢复、即将开始新外部操作或其前置写时重查树），**不是**终态竞态的修复（那是 §3.1 的原子 claim）；与 TaskSupervisor 的 spawn 时标志丢弃构成双保险，并把旧码 delegation 直接 await 派发的未保护窗口关闭。残留：门与 spawn 之间为无 await 同步段，理论上可被抢占交错——该窗口由 T05 journal 契约（started 无回执 → Unknown，绝不盲重试）持有，如实登记。

### 3.5 sessions.rs（+48 行）

`cancel_run_for`：TooLate → 新 `CancelRunOutcome::TooLate{run_id, detail}`（诚实：无取消承诺，指向 durable 终态）；NotLive → 重载 run 行区分 terminal（AlreadyTerminal）/ active（DanglingActive）。

## 4. 改动文件

| 文件 | 改动 |
|---|---|
| `rust/crates/lingxi-service/src/cancel.rs` | Settling 相位、原子 fire、claim_terminal/TerminalAdjudication、TooLate、5 个新单元测试（含 1000 轮 claim-vs-fire 真并发一致性、父树取消 claim 让位） |
| `rust/crates/lingxi-service/src/runs.rs` | adjudicated_finalize 统一裁决、两处接线、9 处派发门 |
| `rust/crates/lingxi-service/src/sessions.rs` | CancelRunOutcome::TooLate、NotLive 重载区分 |
| `rust/crates/lingxi-service/tests/cancel_terminal_race.rs` | 新增（13 集成测试；GatedStorage 为**真实 RunDatabase 的直通装饰器**，仅在选定真实持久化边界确定性泊车——不 mock 存储链、不 sleep） |

未改动：总控账本、`Cargo.lock`、存储迁移/校验值、G01 五文件、R02 资产。

## 5. 验证

- **红绿**：13 测试文件在隔离 worktree（HEAD=520bb75b9，未含修复）实测 **6 红 / 7 钉**（`adversarial-selfcheck/red-baseline-cancel_terminal_race-final.log`；首次红跑另存 `red-baseline-cancel_terminal_race.log`）。红项：C01 主反例（final 事件持久化中取消→旧码 completed+message）、C01-failed 变体（旧码 failed）、C02（旧码谎报 Accepted + 事后 DanglingActive）、C03（宽树并发双 Fired/首因覆盖）、C04-intent（旧码 journal Started 未派发）、C04-no-provider（旧码 completed）。修复后 13/0 全绿。
- **workspace**：`cargo test --workspace --locked` = **66 suites / 666 passed / 0 failed**（底线 65/648/0；+1 suite、+13 集成、+5 单元；无删除无跳过）。
- `cargo fmt --all -- --check` 零 diff；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`Cargo.lock` sha1 不变。
- 额外：`xtask check-contracts`（626 entries 零漂移）、`check-boundaries` exit 0；G01 套件与相邻套件逐套复跑绿（normal-selfcheck/adjacent-suites.log）。
- 逐 C-ID 两层自查：见 `G02-E01_NORMAL_SELFCHECK.md`、`G02-E01_ADVERSARIAL_SELFCHECK.md`。

## 6. 边界与如实声明

- 无真实供应商/无网络外发/隔离 /tmp 合成数据根；Provider/Tool/ApprovalGate 替身仅产生外部响应并计数（外部副作用面）；被测的取消裁决、相位机、finalize 事务、真实 SQLite 存储链未被 mock——GatedStorage 对全部 13 个 StoragePort 方法直通委托，仅在泊车点先信号后等待。
- C02 的「首查 active → 驱动注销 → 重载 terminal」NotLive-重载腿无法在进程内确定性编排（重载点夹在会话面的 backend 读取之间，无注缝）；该腿为 6 行源码级复核 + 编译覆盖，确定性可编排的两态（TooLate 在途、AlreadyTerminal 事后）均已实测。与 G01 报告同类披露一致。
- C04：旧码在外部调用计数上多由 spawn 时标志丢弃兜住（started/intent 边界红在 journal 相位与终态面）；本轮修复使该性质不再单独依赖监督器包裹层，并关闭 delegation 直接 await 派发窗口。delegation 变体未单独集成测试（需绑定完整子代理发射器，其族由 G01 subagent 套件覆盖，门为同一行宏）——静态复核登记。
- 首因/相位不变式对「旧码读→写间隙」的消灭是构造性的（单锁持），对抗自查无法再在该处泊车；hammer（96 entry × 4 caller × 48 子树）与 1000 轮 claim-vs-fire 真并发为绿侧稳定性证据。
- 本轮无 BLOCKED 项；对审查结论无反证（4 条 source_facts 全部复现）。
