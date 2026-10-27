# G02 工作单报告 — R04-RR1-F02（reaper 超时只丢 JoinHandle，且 OS 回收先于归属状态更新）

- 工作单：G02 → R04-RR1-F02 及其验收用例 C01–C04。
- 执行：E01（本代理，2026-10-01）。
- 基线：`1692d2314`（分支 `codex/rust-tauri-migration`，含已合入的 G01 修复）。修复为工作区未提交改动（按总控纪律，不 commit/push）。
- 状态：**READY_FOR_REVIEW**（不自行宣布独立 PASS；三层检查中的独立审查待 Reviewer）。
- 工具链：`~/.cargo/bin/cargo`（rustup 锁定 1.98.1），全部命令带 `--manifest-path rust/Cargo.toml --locked`；`rust/Cargo.lock` 未改动（`git status` 仅两处源码修改 + 新测试 + 新证据目录）。

## 1. 红 → 绿记录

### 1.1 核心 F02 反例（reaper 丢 JoinHandle）

- 红测试：`rust/crates/lingxi-service/tests/r04_rr1_f02_reaper_cleanup.rs::
  rr1_f02_repro_grandchild_pump_outlives_the_grace_and_mutates_the_settled_collector`
  ——只用基线已有的公共 API 写成（collector 快照 + fd 计数，均外部可观察，无需先改产品代码）。
  形态：直接子 `sh` 退出，后台孙进程持有 stdout/stderr 写端并持续慢写（1s/3s 后各写一行）；stdio grace=250ms。
- 红运行（基线 1692d2314，真实 cargo）：`artifacts/.../repro/red_repro_baseline_1692d23.txt`，exit **101**——
  **终态后 settled collector 发生变化（total_bytes 12 → 21，TICK_ONE 被仍在运行的脱管泵追加）**。这正是复审的事实 1：`timeout(grace, pump)` 超时只丢 JoinHandle，tokio 语义下任务脱管继续跑、继续持有读端、继续更新已结算采集器。
- 绿运行（修复后，同一测试）：`repro/green_repro_after_fix.txt`，exit 0（终态后 collector 逐字节冻结、fd 表回到基线）。
- 红测试演化说明（如实）：红证据落盘时的版本还断言"孙进程存活"；修复后观察到**写端持有者在读端关闭后死于自身 SIGPIPE**（向无读者管道写入的 OS 契约，与原栈行为一致），故该断言改为更强的事实断言——`verification_signal_log()` 为空（我们从未向其发任何信号）。红失败触发点是 collector 变化断言，与演化后版本一致。

### 1.2 同族补充反例（spawn 侧"OS 回收先于归属验证"）

- 归因（干净基线 worktree，12 次全套件循环，`logs/baseline_flake_attribution.txt`）：**4/12 失败**，均为
  `EXEC_SPAWN_FAILED: getpgid(pid) returned -1`——快速退出的子进程（`/usr/bin/env`/`/usr/bin/true`/`/bin/pwd`，~1–2ms 退出）在本进程任一运行时线程的 tokio SIGCHLD 处理里被机会性回收，早于 `verify_group` 的 getpgid；基线于是拒绝一个**已完整运行完**的合法子进程（同一根因也造成本会话早期看到的 `ask_session` 在 1485 行的偶发红）。这是 F02 标题族（OS 回收先于归属状态更新）的 spawn 侧实例，按工作单"修根因与同族路径"一并修复。
- 红证明（确定性）：`repro/red_fastexit_old_behavior_unit.txt`——在修复树上临时恢复旧的"getpgid==-1 即拒绝"分支，单测
  `verify_group_admits_a_reaped_child_of_ours_and_the_live_leader` exit **101**；恢复修复后 15/15 绿（`repro/red_fastexit_old_behavior.txt` 为集成腿同实验，轻载下未触发竞态，见 §6 未验证项）。
- 修复后稳定性：t05 全套件 20/20 绿（`logs/t05_stability_final.txt`；基线为 8/12）。

## 2. 修复内容（`rust/crates/lingxi-service/src/procsupervisor.rs`，+1143/−111 行）

对应修复要求 1–5：

1. **保留句柄、abort 并观察实际结束（要求 1、4）**：新 `drain_output_tasks()`——reaper 对每个泵/reader JoinHandle 先按共享 grace 截止时间等自然 EOF；到期则 `abort()` 并在第二个共享预算 `pump_abort_join`（`SupervisorLimits` 新字段，默认 500ms）内 **join 观察实际结束**。每次结束都审计处置：`output_task_finished` / `output_task_panicked` / `output_task_closed_after_abort` / `output_task_end_unconfirmed`（join 不可观察时如实记录"未确认"，绝不折算成已回收）。one-shot 双泵与 **PTY master reader**（原先同样脱管、其 dup 的 master fd 随任务无限存活）走同一链。
2. **事实分别建模（要求 2）**：`RecordInner`/`ProcessSnapshot` 新增 `child_reaped`（reaper `wait()` 解析的真实回收观察，在 grace **之前**发布）、`drained_stdio`（仅当全部流自然 EOF）、`reclaimed`（仅当全部任务结束被观察）、`pumps_alive`（观察缝）。`finalize_exit` 改收 `DrainFacts`，`reclaimed` 不再无条件置 true。
3. **组信号只在可证明归属时发送（要求 3）**：`mark_terminating`（改为方法）在记录锁内做发送时所有权门 `prove_group_ownership`：`child_reaped == false` **且** `getpgid(child) == pgid`（实时内核复核）才 `killpg`；否则审计 `group_signal_skipped`（含原因：已回收 / 内核身份不符），不发信号。reap 事实的发布与发送/跳过决策在同一锁下原子。`Drop` 兜底 killpg 走同一门。模块文档重写：**删除"记录非终态即可证明 PID 未 reap"的错误断言**，如实记载残余窗口（无 pidfd 平台上，reap 与标志发布之间数条指令内 pid 被回收并成为新会话组长——文档化而非掩饰）。
4. **真实观察 + 有界预算（要求 4）**：整个 reaper 尾部预算 = `stdio_grace + pump_abort_join`（两相共享截止时间），`output_drain_summary` 审计耗时与各处置计数；`reclaimed/drained` 全部来自观察。信号收据同理：新增穷尽式**信号调用日志** `verification_signal_log()`（`SignalRecord`：目标、信号、syscall 返回值、发送时归属证明方式），仅观察、不参与决策。
5. **后台后代寿命语义（要求 5）**：不动"正常 `cmd &` 不杀"策略——自然退出从不发组信号（日志为空即为证）；持端后代获得的就是有界 grace + 读端关闭；写端持有者其后死于自身 SIGPIPE（OS 契约），静默持有者存活（C01 双腿分别验证）。PTY 家族：孙进程在会话组长退出时死于内核 SIGHUP（终端语义，非我方），master 读端仍按有界预算收口。

**观察缝（交付的一部分）**：`owned_task_count()`（reaper/泵/reader/脱管尾任务计数，guard 增减）、`pumps_alive`（每记录）、`retained_record_count()/retained_record_ids()`、`verification_signal_log()`、`arm_pump_fault_for_verification(PumpFaultPoint::PanicNext)`（仅进程内 Rust 可用，产品无调用点——与 G01 的 SpawnFaultPoint 同一模式）。

**同族 spawn 侧修复**：`verify_group` 对 `getpgid == -1`（微秒龄子进程的 pid 已消失 ⇒ 已退出并被回收）改为**接纳**记录：reaper 持真实 `Child` 解析存储的退出状态；`child_reaped` 门从此关闭一切组信号，故即便 pid 已被回收复用也不可能经此记录发信号；存活且报告**不同** pgid 的 pid 仍然拒绝。模块文档同步。

## 3. 逐 C-ID 自查（前提/动作/预期/实际/命令/退出码）

统一：真实 `ProcessSupervisor` + 真实 OS 进程；哨兵 `/bin/sleep` 为本测试创建、按精确 pid 清理；测试目录唯一命名；文件级 `serial()` 互斥（G01 先例）保证 fd 计数断言在并行 gate 下确定。命令与退出码见 `logs/per_cid_runs_final.txt`。

### R04-RR1-F02-C01 孙进程持有输出端 — 通过（普通+对抗）

- 普通（工具面，`rr1_f02_c01_grandchild_holding_both_ends_freezes_the_result_and_closes_the_read_ends`，exit 0）：真实 `ProcessTools::run_exec_command`；静默持有者持两端。结果仅在泵真实关闭后产生；终态后 collector 两快照（间隔 400ms）逐字节相等；`pumps_alive==0`、`drained_stdio==false`（诚实）、`reclaimed==true`、审计 2×`output_task_closed_after_abort`；**fd 表回到 spawn 前基线**；孙进程存活（cmd & 语义）且信号日志为空。
- 对抗（`rr1_f02_c01_adversarial_slow_writer_cannot_mutate_the_settled_collector`，exit 0）：继承两端、0.6s/1.0s/1.0s 慢写 2s+；终态后连续探测 2s，total_bytes/window 纹丝不动；写者自身 SIGPIPE 死亡非我方（信号日志空）。
- PTY 家族腿（`rr1_f02_c01_pty_family_grandchild_holding_the_slave_closes_the_master`，exit 0）：孙进程持 pty slave；reader 的结束被观察（自然 EIO 或 grace 到期 abort，两种 OS 处置均如实断言一致）；master 全关（fd 基线）；终态后 transcript 冻结（两次 deliver 相等）；信号日志空。孙进程命运营销侧 SIGHUP（记录不虚报）。

### R04-RR1-F02-C02 已 reap 未排空窗口 — 通过（普通+对抗）

- 普通（`rr1_f02_c02_terminate_in_the_reaped_undrained_window_signals_nothing`，exit 0）：grace=1500ms；轮询至 `child_reaped==true && 非终态`（真实 wait 边界）后发 `terminate(Close)`。收据 `Terminated{fact: Code(0), reclaimed: true, drained_stdio: false}`——直子真实退出 + 管道状态诚实；**信号日志为空 + 审计 `group_signal_skipped`（已回收原因）+ 无 `killpg_sent`**；孙进程在 terminate 后仍存活（无组信号即 approved 语义）；哨兵存活；fd 回基线。
- 对抗（`rr1_f02_c02_adversarial_window_offsets_never_signal_a_stale_group`，exit 0）：三档 grace（200/700/1500ms）从窗口最紧处打入，允许命中或错过（AlreadyTerminal 侧同样安全）；每腿断言无任何组信号、孙进程与哨兵存活。pid 回绕无法在宿主合法制造——身份失效形态由发送时内核身份复核的单测覆盖（`group_signal_is_skipped_when_the_kernel_identity_disagrees`：pid 无人持有 ⇒ 拒绝）。
- 补充单测（lib，exit 0）：`group_signal_is_skipped_once_the_child_reap_is_published`（reap 发布后跳过且日志空）、上述身份不符腿。

### R04-RR1-F02-C03 双泵预算和延后观察 — 通过（普通+对抗）

- 普通（`rr1_f02_c03_double_stuck_pumps_complete_within_the_single_budget`，exit 0）：双泵不结束，grace=80ms、join=150ms。整链（grace 超时→abort→join 观察→settle→shutdown_all）耗时 **≤ 单一预算 grace+2×join+400ms 且 ≥ grace**；`output_drain_summary` 记录耗时；2×`output_task_closed_after_abort`；`owned_task_count` 归零（无不可查询任务）；fd 基线；终态后 collector 冻结；shutdown_all 空收据 <1s。
- 对抗 1（`rr1_f02_c03_adversarial_panicking_pump_is_observed_and_honest`，exit 0）：`PumpFaultPoint::PanicNext` 令 stdout 泵真 panic——处置 `output_task_panicked`、`drained_stdio==false`、`reclaimed==true`（panic 结束被 join 观察）、链不拖延、fd 基线。
- 对抗 2（`rr1_f02_c03_adversarial_exit_and_cancel_racing_never_signals_unprovably`，exit 0）：15 轮 `tokio::join!(wait_terminal, terminate)`，grace 30/250ms 交替（变换临界时间）；每轮收据要么 `Terminated{Code(0) 或 Signal(9)}`（取消先到 ⇒ 所有权已验证的 killpg ⇒ 真实观察的 SIGKILL 退出）要么 `AlreadyTerminal(Exited{0})`；**全部日志信号均有发送时归属证明**；哨兵存活全程。

### R04-RR1-F02-C04 压力下不遗留 — 通过（普通+对抗合一）

- （`rr1_f02_c04_stress_cycles_return_to_the_declared_steady_state`，exit 0）：ring_cap=3、live_cap=3、12 轮（>3× 环容量，终态淘汰与延后退出并发；每 4 轮交替**写端持有者**——SIGHUP/SIGPIPE 形态）。逐轮：`pumps_alive==0`（有界等待）、`reclaimed==true`、`live_handles()` 空、**fd 表=基线**、`owned_task_count` 归零。终态：`retained_record_count() ≤ 3`（声明稳态）、无任务、无 fd 残留、哨兵全程存活（无误杀）。自有孙进程按精确 pid 清理。

## 4. A10 腿复核（G01 §7.1 归因移交）

- 复现尝试：本机当前条件下孤立运行 `r04_a10_pty_input_reads_resize_interrupt_and_exit` **16/16 绿**（6 次于修复前树、10 次于最终树，`logs/per_cid_runs*.txt`）；全套件并行 20/20 绿（该腿含）。G01 E02 报告的"孤立 3/3–5/5 红"未能在本会话复现。
- 结论（如实）：无法复现 ⇒ 不虚构缺陷、不做无据修复。对该腿的源码级复核：`pty_write_all` 全量写入、read 轮询及时、中断（\x03）经线路规程达前台组——监督链无观察语义缺陷；归因维持 G01 E02 的边界描述（macOS readline 在 SIGINT 后丢弃 type-ahead 的子侧时序效应，负载相关）。本单对该腿的实质改进是间接的：spawn 侧 fast-exit 修复消除了同套件高负载偶发红，套件稳定性 8/12→20/20，使该腿回归证据更可信。若 Reviewer 环境复现，按 G01 报告边界另行处置。

## 5. 源码变化与影响范围

- `rust/crates/lingxi-service/src/procsupervisor.rs`（+1143/−111）：模块文档重写（删除错误所有权断言、新增 reaper 可观察性节）；`DEFAULT_PUMP_ABORT_JOIN`；`PumpFaultPoint`/`SignalRecord`/`SignalTarget`/`SIGNAL_LOG_CAP`；`SupervisorLimits.pump_abort_join`；`RecordInner{child_reaped,pumps_alive}` 与 `ProcessSnapshot{child_reaped,drained_stdio,reclaimed,pumps_alive}`；`OwnedTaskGuard`；监督者新字段与观察 API；`mark_terminating` 方法化 + 发送时所有权门；`Drop` 同门；`note_child_reaped`/`GroupOwnership`/`prove_group_ownership`/`PumpEnd`/`DrainFacts`/`audit_push`/`wall_now_ms`/`drain_output_tasks`/`finalize_exit`（重写）/`spawn_output_pump`/`spawn_pty_reader`；one-shot/PTY reaper 重写；spawn 回滚 kill 经 `send_child_kill` 记录；`verify_group` fast-exit 接纳。新增 6 个单元测试（原 9 + 新 6 = 15）。
- `rust/crates/lingxi-service/tests/r04_t05_process_tools.rs`（+50）：**仅加强** `adversarial_parent_exit_with_grandchild_holding_the_pipe_is_bounded`（原断言全保留：elapsed<1500ms、PARENT_DONE、exit 0、孙进程存活）——新增 pumps/reap/drain/审计/collector 冻结/信号日志断言。未删旧测试、未降任何断言。
- 新测试 `rust/crates/lingxi-service/tests/r04_rr1_f02_reaper_cleanup.rs`（12 用例：repro + C01×3 + C02×2 + C03×3 + C04 + fast-exit 回归；文件级 serial）。
- 行为影响面：自然退出的 one-shot/PTY 结果产生时点推迟至泵观察完成（增量 = grace + join，默认 ≤750ms，实测毫秒级）；`reclaimed/drained` 从"无条件 true"变为诚实值；已 reap 记录上的 terminate 不再发组信号（改由 reaper 结算，收据带真实退出事实）；spawn 对 fast-exit 子进程从偶发拒绝变为接纳。`exectools.rs` 消费面无需改动（`Terminated{reclaimed,drained}` 字段语义变诚实；F03 家族的 `Signal(9)` 兜底映射未动——属 G03）。
- G01 语义保持：`reserve_live_slot`/`LiveSlot`/`settle` 记账未改；其套件 9/9 绿（多轮复跑）。
- 过程记录（如实）：会话中一次误用 `git checkout` 还原了 procsupervisor.rs，全部产品改动按留档编辑序列重放并带断言校验，此后所有门禁与证据均基于重放后代码重新执行；最终 diff 完整、无中间态残留。

## 6. 同族路径检查

- one-shot 泵 ✓（重写）；PTY reader ✓（同链）；`shutdown_all`（经 terminate 门）✓；`terminate`/`terminate_detached`（尾任务入 `owned_tasks` 记账）✓；`Drop` 兜底 killpg（同门）✓；spawn 回滚 kill（经信号日志、pid 未回收时点）✓；`verify_group` fast-exit ✓（含确定性单测 + 基线归因）。其他 spawn 点不持本监督者管道，无同型问题（G01 §6 已核，未变）。
- 测试侧缺陷自纠（对抗自查中发现并修复，均为测试侧）：fd 断言需文件级串行且 `serial().await` 必须真正 await（未 await 的 async 调用不执行任何锁逻辑——已修复并复验）；PTY 孙进程存活断言被内核 SIGHUP 语义推翻（改为记录不虚报）；C03 竞走腿 137/0 双合法（断言改为"真实观察的退出"）；PTY 腿快照陈旧竞态（终态后重取）。

## 7. 门禁命令与退出码（全部真实执行，证据在 `artifacts/rust-tauri/R04/RR1-G02-E01/logs/`）

| # | 命令 | 退出码 | 证据 |
|---|------|--------|------|
| 1 | `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 首次 1（新测试文件漂移）→ 应用 fmt 后 **0** | `gate_fmt.txt` |
| 2 | `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | 首次 101（doc_lazy_continuation / if_same_then_else，均已修）→ **0** | `gate_clippy.txt` |
| 3 | `cargo test --manifest-path rust/Cargo.toml --workspace --locked` | **0**（84 个结果行全 ok，含 r00 LAN 腿） | `gate_workspace_tests.txt` |
| 4 | `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-contracts` | **0** | `gate_check_contracts.txt` |
| 5 | `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries` | **0** | `gate_check_boundaries.txt` |
| 6 | 逐 C-ID 单独运行（repro/C01×3/C02×2/C03×3/C04/fast-exit/单元/G01 套件/t05 全套/F02 全文件） | 各 **0** | `per_cid_runs_final.txt`、`per_cid_runs.txt` |

## 8. 未验证项（如实）

- **独立审查（三层检查第三层）**：未执行——属 Reviewer 职责，本代理最多 READY_FOR_REVIEW。
- **verify-stage R04**：未运行——按总控分工属 G05。
- **残余窗口**：无 pidfd 平台上，reap 与 `child_reaped` 发布间数条指令内 pid 回收并成为新会话组长的窗口无法在用户态闭合（模块文档已载）；C02 对抗以此边界用发送时复核替代强造 pid 回绕。
- **fast-exit 竞态的集成级红证明**：该竞态依赖跨线程 SIGCHLD 时序，轻载孤立运行无法确定性触发（`repro/red_fastexit_old_behavior.txt` 未红）；红证明由确定性单元测试承担（`repro/red_fastexit_old_behavior_unit.txt`，exit 101）+ 干净基线 4/12 全套件红（`baseline_flake_attribution.txt`）。
- **A10 时敏失败**：本会话 16/16 + 并行 20/20 未复现（§4）；非本单缺陷证据。
- **非 Unix 平台**：`cfg(not(unix))` 分支未机器验证（macOS arm64 本机）；Windows 维持阶段登记的平台递延。
