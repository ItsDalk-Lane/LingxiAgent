# G02-E01 对抗性自查（逐 C-ID）— F03 取消/终态统一竞争裁决

对抗方法：攻击窗口 → 观测 → 是否推翻 → 命令/退出码 → 证据。红基线在**隔离 git worktree**（`/tmp/lingxi-g02-red2`，HEAD=`520bb75b9`，即含 G01、未含本轮修复的候选）上对最终 13 测试文件实测。命令前缀一律 `~/.cargo/bin/cargo`、`--manifest-path rust/Cargo.toml`、`--locked`。证据根：`artifacts/rust-tauri/R03/repair-current/G02-E01/adversarial-selfcheck/`。

## R03-FIX-F03-C01 取消先于最终提交取得胜利

- **攻击窗口**：验收清单的三序控制——「模型事件前后、提交入队前、事务提交前」。
  - 模型事件**中**（主反例）：泊在 `record_run_events`（`-mc0001` 的 started+done 两条在**同一真实事务提交前**）→ `cancel_run_for` 取得 **Accepted**（真实响应，非替身）→ 释放。
  - 模型事件**前**：泊在 Provider 流读内（cancellation_tree a05/late_result_fence 族既有套件：biased select 于等待中即收口）——本轮不重复建。
  - 提交**入队前**（事件已落、claim 未取）：事件持久化与 claim 之间为无 await 同步段，无可泊车点；其可泊车代表 = 早期边界（intent/started/lineage/waiting_approval 五处泊车测试，均在事件后、终态提交前接受取消）。
  - 事务**提交前**（claim 已取、事务在途）：按冻结规则此时取消**不得**赢 → 归 C02 主测。
- **观测（红）**：旧码 6 红之首——释放后 run 行 `completed` + `messages` 表存在 final message 行（`final_message_committed` 事实），与 Accepted 的取消承诺直接矛盾；failed 终态与 no-provider 收口两个变体同样红。
- **是否推翻修复**：否。修复后三变体全部 `cancelled` / `cancelled.requested` / 无 final message / durable cancelling 腿在。裁决点是原子 `claim_terminal`（相位 Mutex），泊车点无论选在哪个边界、离提交多近，取消先到即改道——不依赖「最后一个 await 前补查」。
- **命令/退出码**：红 `cargo test -p lingxi-service --test cancel_terminal_race --locked`（worktree：**FAILED，6 failed / 7 passed**）；绿同命令（工作区：**ok 13/0**）。
- **证据**：`red-baseline-cancel_terminal_race-final.log`（worktree 红）；`normal-selfcheck/cancel_terminal_race.log`（绿）；`stability-5x-race-core.log`（C01 主测 ×5 + C02 主测 ×5 连跑全绿，泊车握手确定性、非碰运气）。

## R03-FIX-F03-C02 完成已确定时取消正确反馈

- **攻击窗口**：交错「取消响应送达」与「事务确认」——泊在 `commit_run_outcome` 委托前（claim 已写入 `Settling`、真实 SQLite 事务未提交）发起取消；以及完全提交+驱动注销后再取消（首查可能见 active 旧行 → fire 见 NotLive）。
- **观测（红）**：旧码在途取消返回 **Accepted{Requested}**（谎报：承诺停止但 completed 照落）；释放后 completed.with_final + final message 在；**事后**取消返回 **DanglingActive**（把已终态运行误报为 active-无驱动）。双重不诚实。
- **是否推翻修复**：否。修复后：在途 → `TooLate`（响应明示「结算已不可撤销、无取消无停止承诺」，不反写旧终态）；释放后终态恰一次提交（`run_state_changed` 恰 2 条，无 cancelling 腿）；事后 → 经 NotLive 重载返回 `AlreadyTerminal{Completed}`（不再误报 DanglingActive）。
- **残余披露**：「首查 active → 注销 → 重载 terminal」的重载腿无法在进程内确定性编排（重载点夹在会话面 backend 读取之间，无注缝）——6 行源码级复核 + 编译覆盖；可确定性编排的两态均已实测。这与 G01 对同类无注缝窗口的披露口径一致。
- **命令/退出码**：同上（红含 `cancel_racing_the_finalize_transaction_is_too_late_not_accepted` FAILED；绿 ok）。
- **证据**：`red-baseline-cancel_terminal_race-final.log`、`normal-selfcheck/cancel_terminal_race.log`、`stability-5x-race-core.log`。

## R03-FIX-F03-C03 并发重复取消不倒退

- **攻击窗口**：验收清单要求「barrier 固定读旧 phase/写 Requested 窗口」。旧码的窗口=fire 内部「读相位→scope 树遍历→写 Requested」的**非锁持段**。对抗处理分两层：
  1. **让窗口最大**：每 entry 预建 48 个子 scope，使旧码 fire 的读→写之间横跨整棵树的遍历加锁（微秒级窗口放大到可稳定命中）；4 个 OS 线程 `Barrier` 齐发、96 entry。
  2. **构造性消灭**：修复后该段整体处于相位 Mutex 一个临界区内——**窗口不存在**，对抗方无处泊车；因此绿侧证据=真并发 hammer 的不变式（恰好 1×Fired、相位 reason==scope 首因、无倒退）+ 1000 轮 claim-vs-fire 真并发一致性配对（`Claimed↔TooLate` 或 `CancelledBy{首因}↔Fired`，绝无双赢；败方 fire 不碰树）。
- **观测（红）**：旧码 hammer 出现 >1 Fired/entry 且相位 reason ≠ scope 首因（第二 reason 覆盖第一）。
- **是否推翻修复**：否。绿侧 96×4 hammer 与 1000 轮配对全绿；顺序化对手（fire→Cleaning→fire）不回写。
- **命令/退出码**：红同上（`concurrent_duplicate_fires_keep_the_first_reason_and_a_single_fired` FAILED）；绿 `cargo test -p lingxi-service --test cancel_terminal_race --locked`（ok）+ `cargo test -p lingxi-service --lib cancel --locked`（19/0，含 `claim_and_fire_serialize_consistently_under_real_concurrency`）。
- **证据**：`red-baseline-cancel_terminal_race-final.log`、`normal-selfcheck/cancel-unit-tests.log`、`stability-3x-suite.log`。

## R03-FIX-F03-C04 取消先发生时不再启动新操作

- **攻击窗口**：「模型返回工具请求、驱动处于存储或授权边界时先接受取消，再继续调用派发代码」。泊车点覆盖：意图写（存储边界）、started 推进（最后存储边界）、waiting_approval 腿（授权边界）、回执写（工具循环迭代间）、无 Provider 收口（早期收口边界）；外部调用计数 = Provider/Tool/ApprovalGate 替身的真实调用计数器。
- **观测（红）**：旧码两条红——(a) 意图边界释放后 journal 被推进到 **Started**（为从未派发的调用写 started 意图；恢复面将误分类 Unknown）且终态照走；(b) 无 Provider 收口直接 `completed`（取消承诺被完全无视）。**如实登记**：旧码的外部调用计数在 started/intent 边界由 TaskSupervisor「spawn 时作用域已取消即丢弃」兜住（executor 0 次），审批询问同因未 poll 而为 0——红在终态与 journal 事实面，不在调用计数面；且旧码 **delegation 派发是无 select 保护的直接 await**（真实「取消后新外部操作」窗口）。
- **是否推翻修复**：否。修复后五类边界全部：取消 Accepted → 释放 → 零新外部调用（executor 0 / approval asks 0 / provider 恒为取消前那轮）、journal 停在 `Prepared`（无 started-for-never-dispatched）、终态 cancelled、仅取消/审计/收尾写入。派发门为 9 处显式「恢复后重检」（任务书 02 §5 契约），与监督器 spawn 丢弃构成双保险；delegation 窗口由同一门关闭（该族行为由 G01 subagent 套件保持，未单独建集成测试——静态复核登记）。
- **残余披露**：门与 spawn 之间为无 await 同步段，可被抢占交错的理论窗口由 T05 journal 契约持有（started 无回执 → Unknown，不盲重试、不伪造失败）。
- **命令/退出码**：红同上（`cancel_at_the_intent_boundary…`、`cancel_at_the_no_provider_early_close…` FAILED）；绿同上（ok 13/0）。
- **证据**：`red-baseline-cancel_terminal_race-final.log`、`normal-selfcheck/cancel_terminal_race.log`、`stability-3x-suite.log`（套件 3× + `--test-threads=1` 全绿）。

## 汇总

| C-ID | 攻击面 | 红基线（worktree, 520bb75b9） | 绿（候选工作区） | 推翻? |
|---|---|---|---|---|
| C01 | 三序泊车（事件中/边界/提交前） | 4 红（completed+message、failed、intent-shape、no-provider） | 13/0 | 否 |
| C02 | 响应送达×事务确认交错 | 红（Accepted 谎报 + DanglingActive 误报） | TooLate + AlreadyTerminal，单次结算 | 否 |
| C03 | 宽树放大读→写窗口 + 真并发 | 红（双 Fired / 首因覆盖） | hammer + 1000 轮配对全绿（窗口构造性消灭） | 否 |
| C04 | 5 类存储/授权/收口边界泊车 | 2 红（journal Started 未派发、no-provider completed） | 零新外部调用、journal Prepared、cancelled | 否 |

红基线合计 **6 failed / 7 passed**（`red-baseline-cancel_terminal_race-final.log`；其中 7 个 passed 项为钉住既有正确行为的回归钉，非反例）。worktree 已清理（`git worktree remove --force` + prune）。
