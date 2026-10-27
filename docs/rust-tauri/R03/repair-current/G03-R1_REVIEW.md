# R03 修复轮 G03-R1 独立审查报告（F04：工具已派发但 panic/无回执被错误归类为确认失败）

- 审查代理：REVIEWER-REPAIR-R03-G03-R1（一次性独立对抗性 Reviewer，未参与 G03 候选的实现或修复）。
- 日期：2026-09-30。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。
- 候选：HEAD `ccb09fde6` + 未提交工作树（`rust/crates/lingxi-service/src/runs.rs`、新测试 `tests/tool_receipt_unknown.rs`、`tests/subagent_permission_inheritance.rs` +断言）。
- 工具链：`~/.cargo/bin/cargo` 1.98.1（rustup 锁定），全部 `--locked`；复测产物仅写 `artifacts/rust-tauri/R03/repair-current/G03-R1/`；未修改任何产品/测试/配置/门禁/账本/执行者证据；未 commit/push。

## VERDICT: PASS

## 候选摘要

F04 本体修复在 `runs.rs` 驱动的工具执行 select 中：`tool_child.wait()` 返回 `Err(task_exit)` 时不再伪造 `ToolOutcome::Failed`（Internal "tool child … without an outcome"），改为 `ToolOutcome::Unknown { reason }`，reason 由新助手 `unobserved_tool_exit_reason(&TaskExit)` 生成——穷尽匹配 G01 后的真实四变体（`Panicked(payload)` / `Aborted` / `Failed(detail)` / `Completed` 防御臂），各自保留异常类诊断标记并声明 "the external outcome is unobserved"。同族事实修正：delegation 拒绝回执显式 `Failed + dispatched=false + "not dispatched: …"`（成功仍走 `journal_receipt_of`，`dispatched=true`）。后续链路走既有正确语义：`journal_receipt_of(Unknown)` → `ReceiptOutcome::Unknown + dispatched=true`、无 dedup_id；`tool_result_wire` → `ToolResultStatus::Unknown`（02 契约 §5 词汇）；kernel 分类 `Unknown → NeedsAttention/ResumeWithKey/…`（kernel 未改动，错在 service 层伪造 Failed——设计定位正确）。测试面：新 `tool_receipt_unknown.rs` 6 集成测试（真实 `ServiceState::bootstrap_with_deps` + SQLite 存储链 + 真实 Supervisor/journal/恢复分类，替身只产生文件持久外部计数器/请求日志/幂等去重态与受控 panic 故障点）+ 单元 `every_unobserved_tool_exit_variant_journals_unknown_diagnosably` + subagent 套件 delegation `dispatched=0` 断言。

diff 范围核实：`git diff ccb09fde6` 仅 `runs.rs`（139 行）+ 两测试文件（+15 行断言 / 新文件）；`Cargo.lock` sha1 `3b659f41…` 与 HEAD 逐字节相同；kernel/adapters/存储迁移/G01-G02 产品文件零改动。

## 逐 C-ID 结果

| C-ID | 结论 | 独立复测命令（均在 `rust/` 下） | 退出码 | 证据 |
|---|---|---|---|---|
| R03-FIX-F04-C01 副作用后 panic 保留 Unknown | **PASS** | `~/.cargo/bin/cargo test -p lingxi-service --locked --test tool_receipt_unknown f04_c01` | 0（2 passed） | `G03-R1/review-c01-c04-evidence.json#f04_c01_panic_after / #f04_c01_adv_panic_before` |
| R03-FIX-F04-C02 派发后无结果各类异常一致 | **PASS** | 同上 `f04_c02`；`--lib every_unobserved_tool_exit_variant` | 0（1 passed）；0（1 passed） | `G03-R1/review-c02-c03-evidence.json#f04_c02_classification_table`；单元测试输出 |
| R03-FIX-F04-C03 可信负面事实不丢失 | **PASS** | 同上 `f04_c03` | 0（1 passed） | `G03-R1/review-c02-c03-evidence.json#f04_c03_three_classes` |
| R03-FIX-F04-C04 恢复及重复恢复不重做 Unknown | **PASS** | 同上 `f04_c04` | 0（2 passed） | `G03-R1/review-c01-c04-evidence.json#f04_c04_repeat_recovery / #f04_c04_adv_idem_control` |

### C01｜副作用后 panic 保留 Unknown

- 主例（PanicAfter）：外部计数 **1**（副作用确实发生，durable 落盘先于返回）；journal phase **unknown**、receipt `unknown/dispatched=true`、dedup_id 无（不臆造外部回执）；detail 含 panic payload 与 "the external outcome is unobserved"（错误来源可诊断，error 日志同词汇）；幂等键 `{run}-tc0001` 保留在 journal 条目（核验面可用）；恢复分类 `Unknown` → 保守决策 `needs_attention`（非 ConfirmedFailed/ConfirmedSettled）；run 终态 `completed/completed.with_final` 且 `tool_call_completed` 事件 payload `"status":"unknown"`——终态不掩盖不明确调用（红线 4 当前进程面，02 契约 §5）。
- 对抗变体（PanicBefore）：`fired` 通道证明外部从未被调用，计数 0，journal 仍 **unknown**——本地不因外部状态不同而改变类别（本地无法区分，Unknown 是唯一诚实陈述）。
- 红基线：主例在 ccb09fde6 旧码红，失败信息 `a dispatched-no-receipt panic must journal unknown, got Failed`（`G03-R1/red-baseline-tool_receipt_unknown.log`）——F04 source_facts 第 1/2/4 条的反例独立复现。

### C02｜派发后无结果各类异常一致 + 执行者披露专项裁定

- 集成：同一 run 内 `lost.after`（先外部成功再丢响应：计数 1）与 `lost.before`（外部未动作：计数 0）**两态 journal 均 unknown / dispatched=true / 无 dedup_id**，外部总执行恰 1 次（分类表见证据 JSON）。两态丢响应（规格 adversarial_variation）覆盖。
- 单元：`unobserved_tool_exit_reason` 对 `Panicked/Aborted/Failed/Completed` 四变体逐一产出含 "unobserved" 与各自异常类标记的 reason。
- **披露专项独立裁定：Aborted/Failed 无法集成编排的结构论证成立，单元枚举 + 单一入口已封闭漏点。** 我沿调用链逐点核实（非转述执行者）：
  1. drive 侧 select（runs.rs L1396-1451）`biased;` 先 poll `root.cancelled()`，再 poll `tool_child.wait()`；
  2. `cancel_recursive`（cancel.rs L306-351）自根向下：先置根 flag（Release）再递归子 scope——树取消发生时 `root.cancelled()` 必已 ready，biased 恒先命中收束腿（`settle_cancellation`），drive 的 select 拿不到 `Err(Aborted)`；
  3. wrapper 侧（task_supervisor.rs L344-359，CALL 级）同样 biased 且只在 tool scope cancel 时 send `Err(Aborted)`，而那一刻 root 已 cancel；
  4. `tool_scope` 是 `drive_run` 局部变量（L1375 创建，仅传入 L1387 `spawn_linked`），不逃逸函数——外部/测试无法单独 cancel 它而保持 root 未 cancel；产品内全部 cancel 入口都是从某 run 的 root 开始（runs.rs:223 RegistrationGuard、subagents.rs:798 子代理自身 drive_scope）；
  5. `TaskExit::Failed` 仅出现在 `wait()` 的 sender_gone+join-Ok 防御臂（L778）与 drain 的「join finished without an exit record」（L545/L587）——wrapper 所有退出路径都 send（oneshot），该分支理论不可达且不在 drive select 内；
  6. `unobserved_tool_exit_reason` 的 match 穷尽真实枚举、不按变体分岔 outcome 类（只分岔 reason 文案）；未来新增 TaskExit 变体将编译错（非穷尽）→ 结构封闭。
  结论：`Err(task_exit)` 在集成层唯一可确定性到达的变体是 `Panicked`（PanicGuard 收容后 in-band 送达，已双态实测）；单元枚举 + 单一入口覆盖是当前结构下的充分证据。R03 驱动内无工具等待超时（修复要求中「等待超时」无对应代码面，取消侧清理预算由 G02 持有）——非遗漏。

### C03｜可信负面事实不丢失（三态不互相污染）

- 独立复测分类表：tc0001 `reject.me` → `failed/dispatched=false`（detail "not dispatched: …"；零派发由外部系统佐证 `request_count=2`，reject.me 无请求行——拒绝来自 wiring 的审批门 `RejectOneTarget`，非适配器谎报）；tc0002 `fail.external` → `failed/dispatched=true`、detail `upstream_unavailable`、决策 `ConfirmedSettled{Failed}`——**修复没有把真实失败扫进 Unknown**；tc0003 `panic.fault` → `unknown/dispatched=true`、`NeedsAttention`。三类并存互不混淆（红线 1/5）。
- 源码核实：授权边界（runs.rs ~L1024）与审批拒绝（~L1186）的 `dispatched=false` 内联回执不在本轮 diff 中、原样保留；审批 child `Err(task_exit)` → `ApprovalDecision::Aborted` → Failed+`dispatched=false`（审批在派发前，零执行事实为真）不变；executor 返回的真实 Failed 仍经 `journal_receipt_of` 得 `dispatched=true`。禁止项「所有错误都变 Failed」「所有拒绝一律 Unknown」「为测试重复执行真实外部发送」均未发生。
- delegation 拒绝事实修正：`SubagentDispatchError` 全变体逐一核对（subagents.rs）——AccessDenied/SessionLimit/GlobalLimit/ThreadRegistryFull/ThreadNotFound/ThreadNotOpen/ThreadNotInSession/ThreadBusy/InvalidTarget 在 spawn 前拒绝；NotBound/Storage(allocate_run_id 失败)/SpawnRefused 均带回滚且发生在 `spawn_linked` 成功前——**每个 Err 路径零子 run 创建，dispatched=false 事实正确**。分类不变（Failed 相位 → ConfirmedSettled，可信负面不重试）。`subagent_permission_inheritance` 修复后 4/4 绿（含新 `dispatched=0` 断言），旧码该断言红（"1"≠"0"，`G03-R1/red-baseline-delegation-dispatched.log`）。

### C04｜恢复及重复恢复不重做 Unknown + 幂等控制例只用原 key

- 主例：panic-after 留下「计数 1 + 本地 unknown」dangling-active run → 三次恢复：扫描 1（生产形态 `ServiceState::bootstrap`，doubles 缺席）`scanned=1`、类别 `interrupted_needs_attention`（unknown 副作用主导；user_reason 含 "UNKNOWN outcomes"）、`unknown_verdicts_persisted=0`（in-process 回执已标 unknown，扫描不二次写）；恢复 2（直接 journal pass）`persisted=false`、`Unknown/NeedsAttention` 幂等重放；恢复 3（再 bootstrap）`scanned=0`（终态不复活）。三次后外部计数恒 **1**，journal 最终 phase unknown 可见。
- 恢复面源码核实：`recover_run_invocations` 仅对 `Started 且无回执` 的崩溃窗口条目写 unknown verdict（invocations.rs L163-179）——已标 unknown 的条目天然幂等；启动扫描经 `RecoveryCoordinator::run_startup_scan` → 单一 finalize 路径结算，不重驱动、不重执行（re-drive 属 R05，本轮 parked）。
- 控制例：`ledger.idem`（受证实幂等能力，去重行为被测试自身断言）→ 决策 `UnknownResumeWithIdempotencyKey`，**resume_key 与 journal 原 key 逐字相等**（`run_…-tc0001`，证据 JSON 双列对照）；核验经真实 executor 端口原 key 重放 → 请求 2 次、执行仍 1 次、digest 与外部记录一致；verified 收据（Succeeded+dedup=原 key）定案为 succeeded；**二次核验**同 key → 请求 3 次、执行仍 1 次。无新造 key、无重复外部执行（A10 形态）。

## 红基线（真实性独立复核）

隔离 `git worktree add /tmp/r03-g03-redcheck-r1 ccb09fde6` + 拷入新测试（用后已 remove，`git worktree list` 仅剩主工作区）：

- `cargo test -p lingxi-service --locked --test tool_receipt_unknown` → exit **101**，**0 passed / 6 failed**；核心失败 `a dispatched-no-receipt panic must journal unknown, got Failed`（`G03-R1/red-baseline-tool_receipt_unknown.log`）。
- `--test subagent_permission_inheritance` → exit **101**，3 passed / 1 failed（`a refused delegation journals dispatched=false` 断言红）（`G03-R1/red-baseline-delegation-dispatched.log`）。

与执行者 `red-baseline-*` 日志结论一致；修复前红、修复后绿的红绿对照成立。

## G01/G02 回归（复跑全绿）

| 套件 | 结果 |
|---|---|
| cancel_link_inheritance（G01/F01） | 7 passed / 0 failed |
| subagent_closeout（G01/F02） | 8 passed / 0 failed |
| cancellation_tree（G01） | 8 passed / 0 failed |
| cancel_terminal_race（G02/F03） | 13 passed / 0 failed |
| subagent_permission_inheritance（含新断言） | 4 passed / 0 failed |

runs.rs 改动未破坏 G02 裁决入口（`settle_cancellation`/`fence_verdict` 路径原样；`Err(task_exit)` 分支在其下游消费）。

## 门禁（真实退出码）

| 门禁 | 命令 | 退出码 | 结果 |
|---|---|---|---|
| workspace 测试 | `~/.cargo/bin/cargo test --workspace --locked` | 0 | **67 suites / 673 passed / 0 failed**（= 期望 67/673/0，≥ 底线 66/666/0；无删除无跳过） |
| fmt | `~/.cargo/bin/cargo fmt --all -- --check` | 0 | 零 diff |
| clippy | `~/.cargo/bin/cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 零告警 |

执行者侧 `G03-E01/logs/workspace-test-final.log` 复核：67 个 ok 块 / 673 passed / 0 failed，与声明一致。

## finding

1. **INFO（表述歧义，不构成 FAIL 依据）**：执行者报告称「总控账本未改动（其未提交变更为派单前已存在）」——账本 diff 中含 G03-E01 轮次登记（`AWAITING_INDEPENDENT_REVIEW`、67/673/0 摘要、报告路径），其内容依赖执行结果，只可能在执行完成后写入。最合理解读：总控在 R1 派单前统一登记（R1 派单亦声明「总控账本更新不在审查范围」），执行者的「未改动」指其执行期间未再动该文件、与 E01 派单「不要改动或还原」一致。无证据表明执行者伪造；如实记录供总控知悉。
2. **INFO（已披露的编排边界）**：C04 主例「重启」以进程内 `task.abort()` + 重新 `bootstrap` 呈现，无真实进程 kill（R03 无该基础设施，A09/A10 同形态）；C02 的 `Aborted/Failed` 以单元枚举覆盖（专项裁定见上，结构论证成立）。两者均属环境限制的如实披露，非遗漏。

## 误判反证

无。F04 的 4 条 source_facts 全部经红基线独立复现（旧码伪造 Failed → ConfirmedSettled、delegation 拒绝 dispatched="1"）；修复后三态事实清晰、无过度修正。

## 需标 STALE

无。

## 审查范围声明

仅审 G03（F04，R03-FIX-F04-C01..C04）及其同族 delegation 事实修正。未修改任何产品/测试/配置/门禁/账本/执行者证据；未 commit/push；复测产物仅写入 `artifacts/rust-tauri/R03/repair-current/G03-R1/` 与本报告。F05-F07 及后续阶段不计入本轮。本地单机 macOS 结果（隔离 /tmp 数据根、合成替身、无网络外发），不替代其他平台、正式打包或真实供应商验证。
