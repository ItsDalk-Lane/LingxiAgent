# R03 对抗性修复派单｜G01-E01（执行代理）

派单时间：2026-09-30。派单人：R03 修复总控编排器。派单性质：一次性执行代理（EXECUTOR-REPAIR-R03-G01-E01）。

## 0. 你是谁、只做什么

你是一次性执行/修复代理，只处理 **G01 = F01＋F02** 及其同根因路径。基线 `FIX_BASE_SHA=cd3fb19e651f763afc6c75cb3163064fb54ca3fe`，当前候选 `CANDIDATE=cd3fb19e651f763afc6c75cb3163064fb54ca3fe`（本轮尚无修复提交，工作区仅有总控本轮产物：`docs/rust-tauri/R03/repair-current/R03_FIX_ISSUES.json`、`R03_REPORT.md`/`R03_HANDOFF.json` 的 REOPENED 标注——**不要改动或还原它们**）。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。

**你没有 commit/push 权限，不得执行 git commit/push/reset；不得自称独立验收 PASS。** 完成实现+普通自查+对抗性自查后返回 `READY_FOR_REVIEW`，或准确 `FAIL`/`BLOCKED` 并结束本次代理。

## 1. 必读（先读全文，不读摘要、不只搜 PASS）

1. `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/Lingxi_R03_对抗性审查_问题清单与修复总控提示词_2026-09-30.md`（F01/F02 两节全文 + 第三部分总控规程 + S01–S07 来源索引）
2. `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/Lingxi_R03_修复验收清单_2026-09-30.json`（issues F01/F02 与其全部 10 个 case 的 given/when/then/adversarial_variation/evidence_required）
3. `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R03_运行状态机、并发、取消与恢复.md`（原阶段任务书，A01–A16 语义不得破坏）
4. `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/01_通用执行约束.md`、`02_目标架构与强制契约.md` §4（运行状态机/取消四相）、`05_验收与性能协议.md`
5. 现行报告：`docs/rust-tauri/R03/R03_REPORT.md`、`R03_HANDOFF.json`、`R03_FINAL_STAGE_REVIEW_R1.md`（理解裁决2 为何被 F08 质疑）
6. 源码（当前 HEAD 实际代码，不能照抄行号）：`rust/crates/lingxi-service/src/cancel.rs`、`subagents.rs`、`task_supervisor.rs`、`runs.rs`、`background.rs`、`recovery.rs` 及其全部调用者与既有测试（`rust/crates/lingxi-service/tests/` 下 cancellation_tree、subagent_*、background_*、recovery_* 等）。

## 2. 问题与你的 10 个 C-ID

**F01（P1）取消树链接与继承不成立**——`run_root_under` 不向父登记、`is_cancelled` 不沿父查、`child` 不继承已取消父、`register_linked`/drive_run 的 parent_scope 分支真实消费该链。C-ID：
- R03-FIX-F01-C01 linked run root 必须收到父取消（三入口混用、不同深度取消）
- R03-FIX-F01-C02 父已取消后创建子节点（继承取消或拒绝派发；适配器调用计数 0）
- R03-FIX-F01-C03 注册与取消竞态不漏节点（锁边界停驻、取消遍历后加入窗口、多级节点）
- R03-FIX-F01-C04 真实子代理 timeout 收束（Provider 等待/审批等待/配额等待分别测）
- R03-FIX-F01-C05 隔离性与重复取消（无关根不受影响、子取消不向上、首次原因不被覆盖）

**F02（P1）父取消跳过子运行收尾，线程/配额/监督泄漏**——`spawn_child` 先加 active_per_session/active_global 且置 thread.busy，仅 drive 正常返回的 `note_child_finished` 清理；`spawn_linked` biased select 在树取消时丢弃整个 child future（只写 TaskExit::Aborted）跳过 drive 终态/收尾；`RegistrationGuard::drop` 留 active durable 行交启动扫描；同族窗口：`drain_run` 超时 abort 后条目可永为 Running 无句柄、`BackgroundDriveRegistry::live_ids` 可丢已完成句柄不消费 panic/abort 结果。C-ID：
- R03-FIX-F02-C01 同进程多次父取消后仍可使用（超上限次数取消后配额回基线、busy 清除、durable 合法终态、最后正常子任务成功；禁止重启或 startup_scan 帮普通取消过关）
- R03-FIX-F02-C02 未首次 poll 与派发拒绝的收尾（预留/分配 ID/登记线程/spawn 各窗口停止，无永久 busy 或不可解释 active 行）
- R03-FIX-F02-C03 异常结束也必达清理（Provider/Tool panic 或真实存储故障提前返回；迟到旧完成回调不得清新 child_run_id 的 busy）
- R03-FIX-F02-C04 超时中止最终可回收（drain 超时 abort 后任务实际退出时最终回收，不永留 Running/无句柄幽灵项）
- R03-FIX-F02-C05 后台 panic 丢句柄与无关任务隔离（两个注册表、TaskExit、资源增长、无关哨兵继续）

## 3. 修复红线（违反即 FAIL）

F01：
1. 必须修真正的 `register_linked`/`run_root_under` 消费链，不能只修另一个辅助 child 方法。
2. 父侧登记与子侧父引用必须成对建立；“查父取消—注册子—继承首次取消原因/时刻”与父取消遍历要在一个能证明无漏项的并发协议内（不能只在锁外补一次 is_cancelled）。
3. 父已取消时不得开始新的外部动作；新节点继承取消或明确拒绝创建/派发。
4. 保留只向下传播、不影响无关树、首次原因不覆盖的语义。

F02：
1. 区分“发送协作取消”与“最后手段强制丢弃”：先让 child drive 走其合法取消/收据/finalize 收尾，监督器再确认退出。
2. 普通取消必须在**健康服务的当前进程内**收尾：同时验证 durable 状态、busy、active 计数、监督登记。**禁止“重启以后才检查恢复了所以取消通过”；真正崩溃的启动恢复测试保留。**
3. 收尾必须覆盖正常、取消、panic、派发失败、future 未首次 poll、超时路径；用明确守卫或单一完成协调器，避免双减/误清新运行。
4. 数据库失败或真正无法停止时保存明确错误/待协调状态，不伪造 cancelled，也不强制写成功终态抹掉未知副作用。
5. 监督器超时 abort 后保留可恢复的完成观察，最终回收条目；“请求中止”≠“观察退出”。
6. 禁止：提高并发上限/定时重启掩盖泄漏；只清 busy 不归还计数或不处理 durable 行；仅改文档。

通用：不提前进入 R04；不覆盖用户修改；不改写需求；新增测试采用合法增量（不编辑旧迁移校验值）。

## 4. 若认为审查有误

对某 source_fact 有真实反证（当前源码调用链或可复现实验），记录证据链并交总控转全新 Reviewer 裁决 NOT_A_DEFECT；不得自行跳过或弱化，也不得盲修。

## 5. 环境与工具链（必须遵守）

- 一律用 `~/.cargo/bin/cargo`（rustup 代理，锁定 1.98.1）。PATH 中 `/opt/homebrew/bin/cargo` 是 1.93.0 且不读取 `rust-toolchain.toml`——禁止使用。
- 标准：`~/.cargo/bin/cargo fmt --manifest-path rust/Cargo.toml --all -- --check`；`~/.cargo/bin/cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings`；`~/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --workspace --locked`（基线 63 suites / 626 passed / 0 failed，不得倒退）。
- 隔离数据根（/tmp 合成根），不碰真实用户数据、无网络外发、无真实供应商。
- 证据根：`artifacts/rust-tauri/R03/repair-current/G01-E01/`（normal-selfcheck/、adversarial-selfcheck/、logs/ 分开；真实命令、退出码、日志落盘，不许旧文件遮盖）。
- 报告：`docs/rust-tauri/R03/repair-current/G01-E01_REPORT.md`（实现范围/根因/同族路径清单/改动文件/测试清单）+ 同目录 `G01-E01_NORMAL_SELFCHECK.md`、`G01-E01_ADVERSARIAL_SELFCHECK.md`（逐 C-ID：尝试的攻击窗口→观测→是否推翻→命令与退出码→证据路径）。
- **不要编辑** `docs/rust-tauri/R03/repair-current/R03_FIX_ISSUES.json`（总控账本）。

## 6. 第一层：普通逐项自查（每个 C-ID 真实执行）

真实生产/服务入口进入修改后代码；正常/拒绝/错误/取消/恢复行为符合规格；运行状态、DB 行、事件、资源计数、线程状态相互一致；测试真实运行、匹配非 0、退出码真实；相关 R02/R03 回归仍成立。先对原反例建立失败依据（可在旧提交隔离副本或以可信静态依据标记），再在修后代码跑对应回归；不能人为制造无关 bug 冒充复现。不得只跑旧测试；不得 mock 被测的 Supervisor/取消裁决/真实存储链（Provider/Tool/ApprovalGate 替身只产生外部响应或受控外部副作用）。

## 7. 第二层：对抗性自查（逐 C-ID 消费 adversarial_variation）

主动尝试推翻自己的修复。重点交错：取消发生在子节点登记前/后、工具派发前/返回后、持久化等待中；子代理正常结束/父取消/自身 timeout/panic/派发拒绝/未首次 poll/清理超时后实际退出；迟到旧完成回调与新 child_run_id；同 Run 并发取消、不同原因重复取消、状态与原因不倒退；两个注册表间的泄漏对照；同进程重复超上限后的可继续使用。优先 barrier、固定调度、受控故障点；禁止靠反复重跑碰到绿。输出 ADVERSARIAL_SELFCHECK 逐项记录；发现新失败必须修并重跑受影响两层。

## 8. 返回格式

完成后返回：结论（READY_FOR_REVIEW / FAIL / BLOCKED）、改动文件清单、逐 C-ID 状态表（normal/adversarial 两层各 PASS/FAIL+证据路径）、workspace 测试总数、遇到的问题与任何对审查结论的反证。然后结束。
