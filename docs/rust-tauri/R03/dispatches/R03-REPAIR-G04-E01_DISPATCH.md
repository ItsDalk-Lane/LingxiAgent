# R03 对抗性修复派单｜G04-E01（执行代理）

派单时间：2026-09-30。派单人：R03 修复总控编排器。派单性质：一次性执行代理（EXECUTOR-REPAIR-R03-G04-E01）。

## 0. 你是谁、只做什么

你是一次性执行/修复代理，只处理 **G04 = F05**（去重条目在真正受理前固化，失败重试返回不存在的运行）及其同根因路径。基线 `FIX_BASE_SHA=cd3fb19e651f763afc6c75cb3163064fb54ca3fe`，当前候选 `CANDIDATE=198e0da1e6bd4c3a919982ffe46eb27800dcb78a`（含已通过独立审查的 G01/G02/G03 修复——**不得回退或破坏**）。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。

**你没有 commit/push 权限。** 总控账本文件为未提交更新——不要改动或还原。

## 1. 必读

1. `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/Lingxi_R03_对抗性审查_问题清单与修复总控提示词_2026-09-30.md`（F05 节全文 + 总控规程）
2. `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/Lingxi_R03_修复验收清单_2026-09-30.json`（F05 的 5 个 case：R03-FIX-F05-C01..C05）
3. R03 任务书 T04（A08 相同 requestId 不同内容拒绝；requestId 去重+校验请求摘要）+ T07 恢复契约 + 02 目标契约 §3/§8
4. 现行 `docs/rust-tauri/R03/repair-current/` G01–G03 报告（当前取消/终态/Unknown 语义）
5. 源码：`rust/crates/lingxi-service/src/sessions.rs`（admit_submission/execute_submission_for/execute_background_for/cancel_run_for）、`dedup.rs`（SubmissionDedup::admit/normalized_request_digest_hex/DedupDecision）、`background.rs`（spawn_background_drive/BackgroundDriveRegistry）、`runs.rs`（record_run_started/RegistrationGuard）、`recovery.rs`（启动扫描契约）及既有测试（request_dedup、background_*、recovery_*）

## 2. 问题与你的 5 个 C-ID

**F05（P1）**：`admit_submission` 的闭包只取得 session lease 和 run ID，`SubmissionDedup::admit` 随即永久记 key→run；前台随后才 `record_run_started`、后台随后才 `spawn_background_drive`，两处均可在没有运行记录/没有执行前失败；失败后没有与受理状态匹配的回滚/确认；Replay 分支直接回 `ExecuteAccepted(replayed=true)` 不核实该 Run 存在。同进程即可复现（去重表为进程内存）。

- R03-FIX-F05-C01 后台 spawn 拒绝后无幽灵 Replay（占满真实 TaskSupervisor 容量提交带 key 的后台任务→失败→释放容量→同 key 重试：首次明确未受理；重试真实启动或明确恢复既有有效记录，不能返回不存在 Run；容量检查与 spawn 之间也注入失败）
- R03-FIX-F05-C02 运行起始持久化失败补偿（真实存储起始事务提交前注入故障→前台带 key 提交失败→恢复存储后同 key 重试：不残留假受理、无外部动作时可安全重新受理、最终只一个有效执行；覆盖队列满、事务 IO 失败、取消在受理窗口）
- R03-FIX-F05-C03 并发同 key 和异内容（同主体会话两个同 key 请求在预留/提交之间交错：同内容重试只有一个有效受理；不同内容明确冲突；不暴露半提交假结果；跨主体/会话相同 key 作隔离对照）
- R03-FIX-F05-C04 副作用之后响应失败不删绑定（任务外部动作已发生、随后响应/查询失败→同 key 重试：不因遇错清理 dedup 而重复执行；返回已有结果或明确待核验；分别在 journal 提交前/后丢响应两态）
- R03-FIX-F05-C05 重启重试的安全契约（任务有确认或未知副作用、客户端仍持原 key→重启后同 key 提交：按明确契约恢复绑定/拒绝或要求确认，不能静默忽略已有事实后盲重做；同测新 key 正常受理与同 key 改内容；不得伪称 exactly-once 覆盖任意外部系统）

## 3. 修复红线

1. 把**预留、持久受理、实际启动、已知未开始拒绝、结果不确定**区分开；定义 requestId 何时对外承诺稳定 Run 身份。
2. 容量预留、请求绑定、持久化受理与派发采用可证明一致的顺序及补偿：已知未派发失败可安全撤销预留；**未知执行不得直接删除绑定**。
3. Replay 只能返回可核实的真实受理结果；不能返回没有任何持久/受管执行事实的 Run。
4. 保留主体/会话隔离、同 key 不同内容冲突（DuplicateRequestConflict）及完整输入摘要。
5. 重启后同 key 安全语义：沿用本阶段已有恢复契约明确的持久绑定或显式重新确认/拒绝方案，不能因内存丢失把已知/未知外部操作当全新请求重做；**不得借此新建完整分布式任务平台**。
6. 禁止：任何错误都无条件删除 dedup 项；失败重试时伪造 run 行让 Replay 看似存在；把后台派发拒绝改成成功响应；缩短摘要到 2000 字符迁就输入截断（那是 G05 的 F06）。
7. 不破坏 G01–G03 语义与套件；不破坏 A08（同 key 异内容冲突）既有断言。

若认为审查有误：给当前源码调用链或可复现反证，交总控转全新 Reviewer。

## 4. 环境与产出

- 一律 `~/.cargo/bin/cargo`（锁定 1.98.1；PATH 中 Homebrew cargo 1.93.0 禁用）；全部 `--locked`；隔离 /tmp 数据根；无网络外发。
- 证据：`artifacts/rust-tauri/R03/repair-current/G04-E01/`（normal-selfcheck/、adversarial-selfcheck/、logs/）；报告 `docs/rust-tauri/R03/repair-current/G04-E01_REPORT.md` + `G04-E01_NORMAL_SELFCHECK.md` + `G04-E01_ADVERSARIAL_SELFCHECK.md`（逐 C-ID：攻击窗口→观测→是否推翻→命令/退出码→证据）。
- 回归底线：workspace ≥ 67 suites/673 passed/0 failed（允许增加）；fmt 零 diff；clippy -D warnings 零告警；Cargo.lock 不变；G01–G03 套件保持绿。
- 不得 mock 被测的受理去重/Supervisor/真实存储链（Provider/Tool 替身只产生外部响应或受控外部副作用）。

## 5. 返回格式

结论（READY_FOR_REVIEW / FAIL / BLOCKED）、改动文件清单、逐 C-ID 两层自查状态表（含证据路径）、workspace 测试统计、对审查结论的反证（如有）。
