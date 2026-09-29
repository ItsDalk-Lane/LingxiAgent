# R03 对抗性修复派单｜G02-E01（执行代理）

派单时间：2026-09-30。派单人：R03 修复总控编排器。派单性质：一次性执行代理（EXECUTOR-REPAIR-R03-G02-E01）。

## 0. 你是谁、只做什么

你是一次性执行/修复代理，只处理 **G02 = F03**（取消接受与完成提交无统一竞争裁决）及其同根因路径。基线 `FIX_BASE_SHA=cd3fb19e651f763afc6c75cb3163064fb54ca3fe`，当前候选 `CANDIDATE=520bb75b9906120c78b0d10036577fd911a5a81c`（含已完成并通过独立审查的 G01 修复：取消树链接与同进程子收尾——**不得回退或破坏**）。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。

**你没有 commit/push 权限。** 完成实现+普通自查+对抗性自查后返回 `READY_FOR_REVIEW`，或准确 `FAIL`/`BLOCKED`。

工作区内已有未提交的总控账本更新（`R03_FIX_ISSUES.json`、`R03_FIX_COMMIT_RECEIPTS.json`）——**不要改动或还原**。

## 1. 必读

1. `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/Lingxi_R03_对抗性审查_问题清单与修复总控提示词_2026-09-30.md`（F03 节全文 + 总控规程）
2. `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/Lingxi_R03_修复验收清单_2026-09-30.json`（F03 的 4 个 case：R03-FIX-F03-C01..C04）
3. R03 任务书（A02/A05/A07/A15 语义）+ 02 目标契约 §4（取消四相、finalize 单事务、迟到事件仅审计）
4. 现行报告与 G01 产物：`docs/rust-tauri/R03/repair-current/`（G01-E01 报告、G01-R1 审查——理解已改变的取消树语义）
5. 源码：`rust/crates/lingxi-service/src/runs.rs`（drive_run/fence_verdict/settle_cancellation/finalize_settlement/journal_receipt_of）、`cancel.rs`（G01 后的 CancelRegistry fire/phase）、`sessions.rs`（cancel_run_for）、既有测试（run_lifecycle、cancellation_tree、late_result_fence、request_dedup 等）

## 2. 问题与你的 4 个 C-ID

**F03（P1）**：`fence_verdict` 在 Provider 返回后检查 root，但 `persist_model_call` 是异步存储操作；Final 分支在其后直接形成 CompletedWithFinal，循环末尾无条件 `finalize_settlement` 提交成功 outcome，未与取消请求共同裁决；`cancel_run_for` 读 active 行后触发内存取消即返回 Accepted。窗口：取消已被接受，驱动随后仍落 completed。`CancelRegistry::fire` 读阶段再写 Requested 也应检查并发首次原因/阶段倒退。

- R03-FIX-F03-C01 取消先于最终提交取得胜利（Provider 已返 Final、模型事件持久化 barrier 暂停时取消 Accepted → 释放后不得出现 completed/final_message_committed）
- R03-FIX-F03-C02 完成已确定时取消正确反馈（权威提交完成/不可撤销点之后取消 → 已终态/过晚，不反写旧终态、不重复结算）
- R03-FIX-F03-C03 并发重复取消不倒退（两 fire 与 phase 推交错；首次原因一致、phase 单调、终态不重复；barrier 固定窗口，不靠 sleep）
- R03-FIX-F03-C04 取消先发生时不再启动新操作（模型返回工具请求、驱动处于存储/授权边界时先接受取消 → 零新外部调用，仅取消/审计/收尾写入；覆盖无 Provider 早期收口与多个工具循环边界）

## 3. 修复红线

1. 设计同一 Run 上取消请求与终态提交的**统一线性化规则**：取消先赢则不得再提交 completed；完成先已确定则取消响应明确过晚/已终态（不谎报 Accepted 且承诺停止）。
2. 裁决放到真正权威运行所有者/事务边界或等价原子状态协议，**覆盖所有终态路径**（completed/failed/cancelled/interrupted），不只覆盖 Final 一条。
3. **禁止只在最后 await 前补一次 is_cancelled**（仍留检查→提交间窗口）；同样禁止把检查→提交间多次 await 的每个缝都手工补一遍当"原子"。
4. 重复取消保持首次原因与单调阶段；不得把 Cleaning/Confirmed 写回 Requested；取消清理失败按事实报告。
5. 不得规定取消永远覆盖已确认完成的任务（破坏既有终态）；测试不得永远先取消再调模型（必须覆盖最终持久化竞态）。
6. 不破坏 G01 语义与既有 A01–A16 断言（G01 套件 cancel_link_inheritance/subagent_closeout 必须保持绿）。

若认为审查有误：给出当前源码调用链或可复现反证，交总控转全新 Reviewer 裁决；不得自行弱化。

## 4. 环境与产出

- 一律 `~/.cargo/bin/cargo`（锁定 1.98.1；PATH 中 Homebrew cargo 1.93.0 禁用）；全部 `--locked`；隔离 /tmp 数据根；无网络外发。
- 证据：`artifacts/rust-tauri/R03/repair-current/G02-E01/`（normal-selfcheck/、adversarial-selfcheck/、logs/）；报告 `docs/rust-tauri/R03/repair-current/G02-E01_REPORT.md` + `G02-E01_NORMAL_SELFCHECK.md` + `G02-E01_ADVERSARIAL_SELFCHECK.md`（逐 C-ID：攻击窗口→观测→是否推翻→命令/退出码→证据）。
- 回归底线：workspace 测试 ≥ 65 suites/648 passed/0 failed（允许增加）；fmt 零 diff；clippy -D warnings 零告警；Cargo.lock 不变。
- 不得 mock 被测的取消裁决/Supervisor/finalize 事务/真实存储链（Provider/Tool 替身只产生外部响应；存储故障用真实持久化故障点注入）。
- 不编辑总控账本文件。

## 5. 返回格式

结论（READY_FOR_REVIEW / FAIL / BLOCKED）、改动文件清单、逐 C-ID 两层自查状态表（含证据路径）、workspace 测试统计、对审查结论的反证（如有）。
