# R03 修复轮 G02-R1 独立对抗性审查（F03：取消与完成的统一竞争裁决）

- 审查代理：REVIEWER-REPAIR-R03-G02-R1（一次性独立对抗性 Reviewer，未参与 G02 候选的实现或修复）。
- 日期：2026-09-30。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。
- 候选：HEAD `520bb75b9` + 未提交工作树（`cancel.rs` +341 / `runs.rs` +160 / `sessions.rs` +48 / 新测试 `tests/cancel_terminal_race.rs`，另总控账本更新不在审查范围）。实际 `git status`/`git diff --stat` 与派单声明一致；`rust/Cargo.lock` 零改动。
- 工具链：一律 `~/.cargo/bin/cargo`（1.98.1），全部 `--locked`。复测产物根：`artifacts/rust-tauri/R03/repair-current/G02-R1/`。

## VERDICT: PASS

候选通过全部 4 个 C-ID 的独立核查：线性化规则（相位 Mutex 上的原子 `claim_terminal` / `fire`）覆盖所有非取消终态路径；不可撤销点（写 `Settling`）先于 finalize 首个 await；独立红基线复现旧码反例、候选全绿；G01 回归与全部门禁通过。findings 均为非阻断观察项（见下）。

## 1. 线性化规则审查（静态）

1. **终态路径全覆盖**：`runs.rs` 内 `finalize_settlement` 仅 2 个调用点——`adjudicated_finalize`（L1606，Claimed 路径）与 `settle_cancellation`（L1731，cancelled 终态，取消按构造已赢）。循环末尾全部 break 形状（CompletedWithFinal / finish_no_final 族 CompletedWithoutFinal / Failed 全家族：TurnBudgetExceeded、QuotaExhausted(Model/Tool)、ToolExecutorUnavailable、ProviderFailed 含 model_call_child_* 与 late_result_fenced、empty_tool_request_list）与无 Provider 早收口（L622-632）都先经 `adjudicated_finalize`。三个驱动入口（sessions.rs 前台 L567、background.rs L270、subagents.rs 子运行 L769）共用同一 `drive_run`，裁决面无旁路。恢复面（recovery.rs）无活动 entry，不属本裁决面。
2. **不可撤销点时序**：`claim_terminal` 在相位 Mutex 临界区内同步写 `Settling`，返回后紧接 `finalize_settlement(...).await`，两者之间无 await——「写 Settling」严格先于 finalize 首个 await（`commit_run_outcome`）。与"不能只在最后 await 前补一次 is_cancelled"的禁令相符：claim 与提交之间无需再重查。
3. **父树让位路径**：`claim_terminal` 在 Active 时检查 `scope.is_cancelled()`（父树遍历/subagent 超时的直接 scope cancel 均置标志）→ `CancelledBy{首因}` 并补 `Requested` 腿；`register_linked` 继承注册同样处理。逆序（父遍历落在 claim 之后）保留子运行已 claim 的终态——冻结规则的对称面，子运行自身的取消者仍获诚实 TooLate（见 finding-1，判 NOT_A_DEFECT）。
4. **禁断后果逐一核验**：
   - 取消先赢后 completed/final message：CancelledBy 改道 `settle_cancellation`，final message 仅在 `commit_run_outcome` 事务内随 `finish.final_message()` 写入（run_store.rs L1440-1451），Cancelled finish 不携带；会话面 drive 返回后无任何补充 message 写入。
   - 完成已定谎报 Accepted：`fire` 见 `Settling` 早退 `TooLate`（不碰树）；会话面映射新 `CancelRunOutcome::TooLate`；NotLive 分支重载 run 行区分 AlreadyTerminal/DanglingActive，不反写旧终态（响应值，无行写入）。
   - 相位回写 Requested：`fire` 仅在临界区内 Active→Requested；Cleaning/Confirmed 等返回 AlreadyCancelling 零写入；Settling 返回 TooLate 零写入。
   - 首因覆盖：相位 reason 从 scope 首写规则读回，两处永不分歧；重复 fire 恰一个 Fired。
   - 重复结算：原子 claim 单次；`settle_cancellation` 仅经 gate（已观察到取消）或 CancelledBy 改道进入，与 Claimed 互斥；存储层幂等重放语义保留。
   - `Settling` 非取消相位：`cancel_requested()` 为 false，正常结算注销不进 verdict 记录；finalize 出错时 RegistrationGuard 的 Abandoned 覆盖是诚实终局（终态未提交，durable 行保持 active）。
5. **锁序**：fire/claim 仅 `phase → scope.*` 嵌套；`cancel_recursive`/`link_under` 不触 entry.phase，无 `scope.* → phase` 路径，无 ABBA。
6. **G01 协作**：ChildCloseout 仅做配额/线程/快照记账（不 finalize、不 claim）；子运行走同一 `adjudicated_finalize`；G02 未引入双重 finalize 或协作取消窗口冲突。`gate_cancel!()` 恰 9 处（grep 计数核实）。

## 2. 逐 C-ID 结果

| C-ID | 正常自查核对 | 对抗性变体 | 独立复测命令与退出码 | 证据 |
|---|---|---|---|---|
| F03-C01 取消先赢 | 主反例（`-mc0001` 模型事件持久化泊车→Accepted→释放→cancelled、无 final message、cancelling 腿≥3）与 failed/intent 变体断言与规格一致 | 事件中泊车为确定性 Park 握手（无 sleep）；提交前归 C02；事件后边界（intent/started/lineage/waiting_approval）由 C04 族覆盖 | `cargo test -p lingxi-service --test cancel_terminal_race --locked cancel_accepted_while_final_events_persist` → **EXIT=0**（另 3 次稳定性复跑 EXIT=0）；变体 `cancel_accepted_before_the_terminal_claim`（2 tests）→ **EXIT=0**；红基线（worktree@520bb75b9+新测试）同测试 **FAILED：旧码落 `completed`** | `G02-R1/independent-reruns/C01-main-counterexample-rerun.log`、`C01-terminal-shape-variants-rerun.log`、`stability-C01-run{1,2,3}.log`、`G02-R1/red-baseline/baseline-520bb75b9-new-tests.log` |
| F03-C02 完成已定反馈 | `commit_run_outcome` 委托前泊车（claim 已取、事务在途）：非 Accepted、终态恰一次（run_state_changed 恰 2）、事后 AlreadyTerminal；完全提交后取消 AlreadyTerminal 不翻转 | 响应送达×事务确认交错由 Park 固定；「首查 active→注销→重载 terminal」腿无法进程内确定性编排（重载点无注缝）——6 行源码级复核成立：NotLive 分支 reload 后 terminal→AlreadyTerminal / active→DanglingActive | 全套件复跑含两测试：`cargo test -p lingxi-service --test cancel_terminal_race --locked` → **EXIT=0（13/0）**；红基线同测试 **FAILED：旧码谎报 `Accepted{Requested}`** | `G02-R1/independent-reruns/cancel-terminal-race-full-suite-rerun.log`、红基线 log 同上、执行者 `red-baseline-cancel_terminal_race-final.log` |
| F03-C03 并发不倒退 | 96 entry×4 caller×48 宽树 barrier 齐发：恰 96 Fired、相位 reason==scope 首因；Cleaning 后 fire 不回写（集成+单元双层） | 旧码读→写窗口横跨树遍历被宽树放大（红基线实测 **104≠96 Fired**，首因覆盖随断言暴露）；修复后窗口构造性消灭（单锁持），绿侧以真并发不变式+1000 轮 claim-vs-fire 配对（`Claimed↔TooLate` 或 `CancelledBy{首因}↔Fired`，败方不碰树） | `… --test cancel_terminal_race --locked concurrent_duplicate_fires` → **EXIT=0**；`… a_fire_after_the_driver_reached_cleaning` → **EXIT=0**；`… --lib --locked claim_and_fire_serialize…`（1000 轮真并发）→ **EXIT=0**（另 3 次稳定性 EXIT=0） | `G02-R1/independent-reruns/C03-hammer-rerun.log`、`C03-no-regression-rerun.log`、`C03-claim-vs-fire-1000-rounds-rerun.log`、`stability-claim-vs-fire-run{1,2,3}.log`、红基线 log |
| F03-C04 取消后无新外部操作 | intent/started/waiting_approval/receipt/no-provider 五类边界泊车：executor 0、approval asks 0、provider 恒取消前轮、journal 停 Prepared、终态 cancelled | 9 处 `gate_cancel!` 恢复后重检门（含模型许可后、每工具迭代首、delegation 派发前、started 推进后）；对照测试钉 journal Succeeded+回执 | 全套件复跑（含 5 个 C04 测试）→ **EXIT=0（13/0）**；红基线 `cancel_at_the_intent_boundary…` **FAILED：旧码 journal 推进 Started（left: Started / right: Prepared）**、no-provider **FAILED：旧码 completed** | 同全套件 log、红基线 log、静态 grep `gate_cancel!();`=9 |

单元层：`cargo test -p lingxi-service --lib --locked cancel` → **EXIT=0（19/0，含 5 个新测试）**（`G02-R1/independent-reruns/cancel-unit-tests-rerun.log`）。

## 3. 红基线真实性（独立）

`git worktree add /tmp/r03-g02-redcheck 520bb75b9` → 拷入候选新测试 `cancel_terminal_race.rs` → `~/.cargo/bin/cargo test -p lingxi-service --test cancel_terminal_race --locked`（worktree 内）→ **EXIT=101，7 passed / 6 failed**，与执行者 `red-baseline-cancel_terminal_race-final.log` 完全同构（同 6 个失败项；我的运行实测捕获 C03 双 Fired=104/96、C01 completed-after-Accepted、C02 Accepted-during-finalize、C04 journal Started）。用后 `git worktree remove --force` + `prune`，`git worktree list` 仅剩主工作区。旧码红、新码绿双向成立，测试非“永绿摆设”。

## 4. G01 回归

- `--test cancel_link_inheritance` → **EXIT=0（7/0）**
- `--test subagent_closeout` → **EXIT=0（8/0）**
- `--test cancellation_tree` → **EXIT=0（8/0）**

G01 语义（树链接/继承/子运行收尾）未被 G02 破坏；ChildCloseout 与 adjudicated_finalize 无双重 finalize 冲突（静态核实：closeout 仅记账）。

## 5. 门禁（真实退出码）

- `~/.cargo/bin/cargo test --workspace --locked` → **EXIT=0；66 suites ok；666 passed / 0 failed**（=期望 66/666/0；G01 后 65/648 + 1 suite + 18 tests）
- `~/.cargo/bin/cargo fmt --all -- --check` → **EXIT=0，零 diff（0 行输出）**
- `~/.cargo/bin/cargo clippy --workspace --all-targets --locked -- -D warnings` → **EXIT=0，零告警**
- `rust/Cargo.lock`：`git diff --stat -- rust/Cargo.lock` 零行，未变。

## 6. Findings（均非阻断）

1. **NOT_A_DEFECT（设计冻结点确认）**：父树遍历落在子运行 `Settling` claim 之后时，子运行终态保留（父取消 Accepted、子仍完成）。这是冻结线性化规则的对称面：claim 即不可撤销点，与 C02「完成已定→取消反馈过晚」一致；子运行自身的取消者获诚实 TooLate。全序自洽，无 C-ID then-clause 被违反。
2. **边缘观察（诚实性余量）**：`Settling` claim 后 finalize 事务失败（存储错误）时，TooLate 响应指引「查询 run 行」会得到 active 行（guard 记 Abandoned、树取消、durable 行诚实）。属存储错误边缘，不在 4 个 C-ID 的 then 范围内；执行者报告已按实披露相邻边缘。
3. **残留披露复核（接受）**：门与 spawn 间无 await 同步段的可抢占窗口由 T05 journal 契约持有（started 无回执→Unknown，不盲重试）；delegation 派发门有静态覆盖与 G01 subagent 族行为保持，未单独建集成测试——执行者已披露，G01 套件绿佐证。
4. C04 在旧码的外部调用计数面多由 TaskSupervisor spawn 丢弃兜住（红在 journal/终态面）——执行者如实登记，不构成反证。

## 7. 误判反证 / STALE

- 无误判反证：F03 四条 source_facts 全部在我的独立红基线中复现，修复后全部消除。
- 无需标 STALE：执行者三份报告的数字（红 6/7、绿 13/0、19/0 单元、66/666/0、fmt/clippy 零告警、Cargo.lock 不变）逐项与我的独立复测一致，证据文件齐全无缺。

## 8. 审查范围声明

仅审 G02（F03，R03-FIX-F03-C01..C04）。未修改任何产品/测试/配置/门禁/账本/执行者证据；未 commit/push。复测产物仅写入 `artifacts/rust-tauri/R03/repair-current/G02-R1/` 与本报告。F04-F08 及后续阶段独立功能缺失不计入本轮 FAIL。本地单机 macOS 结果，不替代其他平台/正式打包/真实供应商验证。
