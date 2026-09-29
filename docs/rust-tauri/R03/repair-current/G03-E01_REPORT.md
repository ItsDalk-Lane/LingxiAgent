# R03 修复轮 G03-E01 执行报告（F04：工具已派发但 panic/无回执被错误归类为确认失败）

- 执行代理：EXECUTOR-REPAIR-R03-G03-E01（一次性执行/修复代理；本报告为执行者口径，不含独立审查）。
- 日期：2026-09-30。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。
- 基线 `FIX_BASE_SHA=cd3fb19e651f763afc6c75cb3163064fb54ca3fe`；候选起点 `ccb09fde679b4cd96e2ba2f5f51e53bce564710f`（含已通过独立审查的 G01 取消树/子收尾与 G02 统一终态裁决——本轮**未回退、未破坏**：cancel_link_inheritance 7/0、subagent_closeout 8/0、cancel_terminal_race 13/0、cancellation_tree 8/0 逐套复跑全绿）。本轮无 commit/push（未获授权）。总控账本（`R03_FIX_ISSUES.json`、`R03_FIX_COMMIT_RECEIPTS.json`）未改动（其未提交变更为派单前已存在）。
- 工具链：`~/.cargo/bin/cargo`（rustup 锁定 1.98.1），全部 `--locked`；`rust/Cargo.lock` sha1 `3b659f41eb262eaf42efc2f91ec93f9989d42934` 与 HEAD 相同（零依赖变化）。
- 证据根：`artifacts/rust-tauri/R03/repair-current/G03-E01/`（`normal-selfcheck/`、`adversarial-selfcheck/`、`logs/`）。
- 结论：**READY_FOR_REVIEW**（workspace 67 suites / 673 passed / 0 failed ≥ 底线 66/666/0；fmt 零 diff；clippy `-D warnings` 零告警；check-contracts（626 entries 零漂移）/ check-boundaries 额外回归 exit 0）。

## 1. 实现范围

F04（工具已派发但异常无回执被错误归类为确认失败）及其同根因路径（receipt 事实类混淆的同一族：delegation 拒绝回执的 `dispatched` 事实修正）。不涉及 F05–F07，不进入 R04。

## 2. 根因复核（结论：审查属实，已用隔离 worktree 红基线实测复现，无反证）

调用链复核与审查清单一致：

1. `runs.rs` drive_run 的工具执行 select 中，`tool_child.wait()` 返回 `Err(task_exit)`（G01 后的真实变体：`Panicked(payload)` / `Aborted` / `Failed(detail)`）时，驱动**伪造** `ToolOutcome::Failed { Internal "tool child {name} without an outcome" }` ——这是运行器层事实，不是外部回执。
2. `journal_receipt_of(Failed)` 把它写成 `ReceiptOutcome::Failed` 且 `dispatched=true`（写序本身正确：started 在派发前、回执在派发后；错在回执的类别）。
3. `lingxi-kernel/src/invocation.rs` 的 `invocation_recovery_class` 把 phase `Failed` 归 `ConfirmedFailed`，`classify_invocation_recovery` 返回 `ConfirmedSettled`；run 级 `classify_run_recovery` 把只有 ConfirmedSettled 决策的 run 归 `recoverable_wait`。**实际已发生的写入/发送在本地被记成"确认失败"，恢复面失去不确定性信息。**
4. `Cancelled` 分支（executor 停止等待）与 fence 拒收分支早已是 Unknown——panic 分支与之不一致，正是审查所指。

**红基线实测**（隔离 git worktree，HEAD=ccb09fde6，未含修复）：新增 6 个反例测试 `rust/crates/lingxi-service/tests/tool_receipt_unknown.rs` 全红（6 failed / 0 passed，`adversarial-selfcheck/red-baseline-tool_receipt_unknown-final.log`）。代表性失败信息：`a dispatched-no-receipt panic must journal unknown, got Failed`（核心分类错误）、`the unknown side effect governs the run: recoverable_wait`（恢复类别丢失不确定性）、idem 控制例拿不到 resume-with-key（拿到 ConfirmedSettled）。

### 同族路径清单（receipt 写点全量枚举，逐一定性）

| 写点（runs.rs） | 修复前事实 | 定性 | 处置 |
|---|---|---|---|
| 授权边界拒绝（~L1024） | Failed + `dispatched=false`，executor 零调用 | 已知未派发（类 1），正确 | 不变（C03 钉） |
| 审批拒绝/中止（~L1186） | Failed + `dispatched=false`，零执行 | 已知未派发（类 1），正确 | 不变（C03 钉） |
| delegation 结果（~L1315） | 拒绝也经 `journal_receipt_of` → Failed + `dispatched=true` | **事实错误**：每个 `SubagentDispatchError` 均零子 run 创建（已知未派发），却记成已派发 | **修正**：拒绝回执显式 `dispatched=false`（同族事实类修正，红基线独立复现 `red-baseline-delegation-dispatched.log`：旧码该列="1"） |
| 工具执行回执（~L1483） | executor 返回的 Failed → Failed + `dispatched=true` | 可信外部明确失败（类 2），正确 | 不变（C03 钉；修复不得把真实失败扫进 Unknown） |
| **工具 child Err(task_exit)（~L1410）** | 伪造 Failed + `dispatched=true` → ConfirmedFailed/ConfirmedSettled | **F04 本体**：运行器失败无外部结果（类 3）被冒充类 2 | **修复**：Unknown |

范围外且已核对的相邻面：模型 child `Err(task_exit)` → run 级 `failed.provider_failed(model_call_child_*)`（run 终态诚实陈述"该次 provider 调用无结果"，模型调用不写 invocation journal，无回执分类面——不在 F04 范围，语义保持）；审批 child `Err(task_exit)` → `ApprovalDecision::Aborted` → Failed+`dispatched=false`（零执行事实为真，类 1 正确，不变）；`record_invocation_unknown` 存储面（Started+无回执才允许）与 `recover_run_invocations`（仅 Started+无回执持久化 verdict、幂等）无需改动。

## 3. 修复设计（最小完整）

### 3.1 核心（runs.rs，F04 本体）

`tool_child.wait()` 的 `Err(task_exit)` 分支不再构造 `ToolOutcome::Failed`，改为 `ToolOutcome::Unknown { reason }`，reason 由新助手 `unobserved_tool_exit_reason(&TaskExit)` 生成——**消费真实 TaskExit 变体而非字符串猜测**（红线 6）：

- `Panicked(payload)` → "tool executor panicked after dispatch (panic: {payload}); the external outcome is unobserved"（panic payload 落入 receipt detail + error 日志，错误来源可诊断）；
- `Aborted` → 监督中止（future 在 await 点被丢弃），外部结果未观察；
- `Failed(detail)` → 监督通道丢失（{detail}），外部结果未观察；
- `Completed` → "完成却未交付结果"的内部异常（防御性，理论不可达）。

后续链路按既有正确语义自动成立：`journal_receipt_of(Unknown)` → `ReceiptOutcome::Unknown`+`dispatched=true`（执行确已派发）；`tool_result_wire(Unknown)` → 模型看到 `unknown` 状态（契约 §5）；`saw_tool_failure` 计入（run 以 tool-partial-failure 词汇结算或带 final 完成）；恢复分类 Unknown → 按能力（保守=NeedsAttention / 幂等=ResumeWithKey / 只读=Reexecute / 可核验=VerifyExternally）。幂等键信息保留（intent 里的 `idempotency_key` 不动）。**禁止未知副作用自动重做**由该分类链既有语义持有（A09/A10 已验证），本修复只是把 panic 情形接回同一语义。

### 3.2 同族事实修正（runs.rs delegation 分支）

delegation 拒绝回执显式构造为 `Failed + dispatched=false + "not dispatched: …"`（成功仍走 `journal_receipt_of`，`dispatched=true`）。红线 1 的三类事实（已知未派发 / 可信外部明确失败 / 运行器失败无外部结果）在 journal 中保持可区分。分类不变（Failed 相位 → ConfirmedFailed/ConfirmedSettled——拒绝是可信负面，不重试正确）。

### 3.3 不做的事（对照"不能这样修好"）

- 不是"所有错误都变 Failed/Unknown"：executor 返回的真实 Failed 与授权/审批拒绝的 dispatched=false 原样保留（C03 三个钉断言）。
- 不为测试重复执行真实外部发送：核验/重试只经恢复决策（原 key），外部计数器断言总量不增。
- 不改 kernel 分类（`invocation_recovery_class` 对 Failed=ConfirmedFailed 的设计本就正确，错在 service 层伪造 Failed）；不改存储迁移/校验值；不动 G01/G02 语义。

## 4. 改动文件

| 文件 | 改动 |
|---|---|
| `rust/crates/lingxi-service/src/runs.rs` | F04 本体：Err(task_exit)→Unknown + `unobserved_tool_exit_reason`（4 变体）；delegation 拒绝回执 `dispatched=false`；`journal_receipt_of` 文档边界更新；新单元测试 `every_unobserved_tool_exit_variant_journals_unknown_diagnosably` |
| `rust/crates/lingxi-service/tests/tool_receipt_unknown.rs` | 新增（6 集成测试：C01×2 / C02 / C03 / C04×2，真实 ServiceState+SQLite 存储链+真实 Supervisor/journal/恢复分类；Tool 替身只产生文件持久的外部计数器/请求日志/幂等去重态与受控 panic 故障点） |
| `rust/crates/lingxi-service/tests/subagent_permission_inheritance.rs` | 既有 readonly_parent 用例追加 delegation 拒绝回执 `dispatched=0` 断言（钉 3.2 的事实修正；红基线独立复现） |

未改动：总控账本、`Cargo.lock`、kernel/adapters 源码、存储迁移/校验值、G01/G02 五文件、R02 资产。

## 5. 验证

- **红绿**：`tool_receipt_unknown` 6 测试在隔离 worktree（HEAD=ccb09fde6）实测 **0 passed / 6 failed**；修复后 **6 passed / 0 failed**（3 次复跑稳定，`stability-3x-suite.log`）。delegation `dispatched` 断言旧码红（"1"≠"0"）新码绿。
- **workspace**：`cargo test --workspace --locked` = **67 suites / 673 passed / 0 failed**（底线 66/666/0；+1 suite、+6 集成、+1 单元、+1 既有用例断言；无删除无跳过）。
- `cargo fmt --all -- --check` 零 diff；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`Cargo.lock` sha1 不变。
- 额外：`xtask check-contracts`（626 entries 零漂移）、`check-boundaries` exit 0；G01/G02 套件与相邻套件（invocation_journal / late_result_fence / recovery_* / subagent_* / r03_t08_acceptance_matrix）逐套复跑绿（`normal-selfcheck/adjacent-suites.log`）。
- 逐 C-ID 两层自查：见 `G03-E01_NORMAL_SELFCHECK.md`、`G03-E01_ADVERSARIAL_SELFCHECK.md`。

## 6. 边界与如实声明

- 无真实供应商/无网络外发/隔离 /tmp 合成数据根；Provider/ApprovalGate/Tool 替身只产生外部响应、审批决定与受控外部副作用（独立持久计数器文件/请求日志），被测的 Supervisor 裁决、journal 写序、恢复分类与真实 SQLite 存储链未被 mock。受控 panic 由真实 PanicGuard 收容（G01 行为，未 mock 掉）。
- C02 的 `TaskExit::Aborted`（强制中止）与 `TaskExit::Failed`（监督通道丢失）无法在进程内经真实链确定性编排到该分支（树取消因 biased select 与 cancel_recursive 自根向下的置序恒先走 `root.cancelled()` 收束腿——G02 设计使然；wrapper 级 abort 仅发生在 drive 之外的 drain/显式 abort）。二者以单元级变体枚举覆盖（`every_unobserved_tool_exit_variant_journals_unknown_diagnosably`，4 变体逐一断言 Unknown 语义+可诊断标记）+ 分支结构统一性（单一 `Err(task_exit)` 入口不按变体分岔）作为证据；端到端实测覆盖了 `Panicked` 在外部确认前后两态。此披露与 G01/G02 报告同类。
- C04 控制例的"重启后核验"以恢复决策 + 原 key 重放呈现（R03 无 re-drive loop，A10 同形态）；`UnknownResumeWithIdempotencyKey` 的 key 与 journal 原 key 逐字相等已断言，二次核验仍单次执行。
- 本轮无 BLOCKED 项；对审查结论无反证（4 条 source_facts 全部红基线复现）。
