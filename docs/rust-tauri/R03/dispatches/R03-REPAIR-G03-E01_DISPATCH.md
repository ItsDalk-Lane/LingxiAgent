# R03 对抗性修复派单｜G03-E01（执行代理）

派单时间：2026-09-30。派单人：R03 修复总控编排器。派单性质：一次性执行代理（EXECUTOR-REPAIR-R03-G03-E01）。

## 0. 你是谁、只做什么

你是一次性执行/修复代理，只处理 **G03 = F04**（工具已派发但异常无回执时被错误归类为确认失败）及其同根因路径。基线 `FIX_BASE_SHA=cd3fb19e651f763afc6c75cb3163064fb54ca3fe`，当前候选 `CANDIDATE=ccb09fde679b4cd96e2ba2f5f51e53bce564710f`（含已通过独立审查的 G01 取消树/子收尾与 G02 统一终态裁决——**不得回退或破坏**）。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。

**你没有 commit/push 权限。** 工作区内总控账本文件（`R03_FIX_ISSUES.json`、`R03_FIX_COMMIT_RECEIPTS.json`）为未提交更新——不要改动或还原。

## 1. 必读

1. `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/Lingxi_R03_对抗性审查_问题清单与修复总控提示词_2026-09-30.md`（F04 节全文 + 总控规程）
2. `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/Lingxi_R03_修复验收清单_2026-09-30.json`（F04 的 4 个 case：R03-FIX-F04-C01..C04）
3. R03 任务书 T05 节（A09/A10：副作用后崩溃不重复执行、可验证幂等恢复）+ 02 目标契约 §5（ToolResult 区分 success/failed/cancelled/unknown）+ §8（started 无回执→unknown）
4. 现行 `docs/rust-tauri/R03/repair-current/` G01/G02 报告（理解当前取消/终态语义）
5. 源码：`rust/crates/lingxi-service/src/runs.rs`（tool_child.wait Err→ToolOutcome::Failed 的分支；journal_receipt_of）、`rust/crates/lingxi-kernel/src/invocation.rs`（invocation_recovery_class/classify_invocation_recovery）、`rust/crates/lingxi-service/src/invocations.rs`（recover_run_invocations/ConservativeCapabilities）、`rust/crates/lingxi-service/src/recovery.rs`（RecoveryCoordinator 启动扫描）、`task_supervisor.rs`（TaskExit 语义，G01 后已变）及既有测试（invocation_journal、recovery_*、late_result_fence 等）

## 2. 问题与你的 4 个 C-ID

**F04（P1）**：工具已派发后 `tool_child.wait` 返回 `Err(task_exit)` 时 drive_run 构造 `ToolOutcome::Failed`（非 Unknown）；`journal_receipt_of(Failed)` 写 `ReceiptOutcome::Failed` 且 `dispatched=true`；`invocation_recovery_class` 把 Failed 归 `ConfirmedFailed`，`classify_invocation_recovery` 据此返回 `ConfirmedSettled`。panic/中止/监督通道丢失只能证明没拿到可靠结果，不能证明外部操作失败（Cancelled 分支已用 Unknown，panic 分支不一致）。

- R03-FIX-F04-C01 副作用后 panic 保留 Unknown（受控外部计数器初始 0，Tool 加 1 后 panic，经真实 Supervisor 执行并读 journal：外部计数 1、收据 Unknown、错误来源可诊断；panic 分别在外部确认前后触发均不得臆测回执）
- R03-FIX-F04-C02 派发后无结果各类异常一致（中止、通道丢失、超时 → 保留 Unknown 不漏分类；先外部成功再丢响应 vs 外部未动作结果未知两态）
- R03-FIX-F04-C03 可信负面事实不丢失（授权前拒绝 dispatched=false；外部明确失败回执保留可信 failed；两者区别于无结果 panic；拒绝须来自真实授权边界，不得适配器谎报）
- R03-FIX-F04-C04 恢复及重复恢复不重做 Unknown（留下已加 1 但本地 Unknown 的调用，恢复扫描两次：无新外部执行、Unknown/待核验可见、恢复幂等；另设可信幂等控制例，安全核验只用原 key）

## 3. 修复红线

1. 区分三类事实：已知未派发、可信外部明确失败、运行器/适配器失败且无外部结果。
2. 派发后无可信回执的 panic、通道丢失、强制中止、等待超时 → 保持或转换为 **Unknown**；内部运行错误单独记录（不冒充外部结果）。
3. 保留外部幂等键/回执/查询所需信息；**禁止未知副作用自动重做**；已知拒绝与明确失败仍正确归类。
4. 核对当前进程与启动恢复两面对 Unknown 的呈现；Run 终态已结束也不能掩盖尚不明确的调用结果。
5. 禁止：所有错误都变 Failed；所有拒绝一律 Unknown（丢掉已知未派发事实）；为完成测试重复执行真实外部发送。
6. 与 G01/G02 语义协同：TaskExit 语义在 G01 后已改（PanicGuard 就地记录等）——分类须消费真实 TaskExit 变体而非字符串猜测；不得破坏 G01/G02 套件。

若认为审查有误：给当前源码调用链或可复现反证，交总控转全新 Reviewer。

## 4. 环境与产出

- 一律 `~/.cargo/bin/cargo`（锁定 1.98.1；PATH 中 Homebrew cargo 1.93.0 禁用）；全部 `--locked`；隔离 /tmp 数据根；无网络外发。
- 证据：`artifacts/rust-tauri/R03/repair-current/G03-E01/`（normal-selfcheck/、adversarial-selfcheck/、logs/）；报告 `docs/rust-tauri/R03/repair-current/G03-E01_REPORT.md` + `G03-E01_NORMAL_SELFCHECK.md` + `G03-E01_ADVERSARIAL_SELFCHECK.md`（逐 C-ID：攻击窗口→观测→是否推翻→命令/退出码→证据）。
- 回归底线：workspace ≥ 66 suites/666 passed/0 failed（允许增加）；fmt 零 diff；clippy -D warnings 零告警；Cargo.lock 不变；G01/G02 套件保持绿。
- 不得 mock 被测的 Supervisor/journal 写序/恢复分类/真实存储链（Tool 替身只产生受控外部副作用——如独立持久计数器文件）。

## 5. 返回格式

结论（READY_FOR_REVIEW / FAIL / BLOCKED）、改动文件清单、逐 C-ID 两层自查状态表（含证据路径）、workspace 测试统计、对审查结论的反证（如有）。
