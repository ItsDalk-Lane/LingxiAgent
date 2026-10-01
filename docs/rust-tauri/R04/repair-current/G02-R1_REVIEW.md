# G02 独立复审报告 — R04-RR1-F02（reaper 超时只丢 JoinHandle，且 OS 回收先于归属状态更新）

- Reviewer：**REVIEWER-R04-RR1-G02-R01**（全新代理，未参与 G02 执行/修复，无共享上下文）。
- 审查对象：基线 `1692d2314`（`codex/rust-tauri-migration`，含 G01 修复）→ 候选 = 当前工作区未提交改动
  （`rust/crates/lingxi-service/src/procsupervisor.rs` +1143/−111、`tests/r04_t05_process_tools.rs` +50、
  新测试 `tests/r04_rr1_f02_reaper_cleanup.rs`；另有执行者台账 `docs/rust-tauri/R04/repair-current/R04_RR1_FIX_ISSUES.json`
  状态行更新，非产品代码）。工作目录 `/Users/study_superior/Desktop/Code/LingxiAgent`。
- 工具链：`~/.cargo/bin/cargo 1.98.1`（rustc 1.98.1），全部命令带 `--manifest-path rust/Cargo.toml --locked`；`rust/Cargo.lock` 未动。
- 纪律：主树产品/测试源码零修改；复测输出只写 `artifacts/rust-tauri/R04/RR1-G02-R1/`；基线与候选对照复跑在
  `/tmp/r04-g02-baseline`、`/tmp/r04-g02-candidate` 两个独立 git worktree（**已用后清理**，含自有测试进程，清理后核实无残留）。
  执行者报告 `G02-E01_REPORT.md` 只作线索，未复制其任何日志。
- 结论：**VERDICT: PASS**（依据见 §2–§5；问题清单 §6 全部为非阻塞观察项）。

## 1. 独立复测：实际命令与退出码

退出码说明：`cargo test` 失败=101、成功=0；下表"结果行"为原始输出中的 `test result:` 行（证据文件存
`artifacts/rust-tauri/R04/RR1-G02-R1/logs/`，文件名在括号内）。

| # | 命令（两侧同命令/同测试） | 结果 | 证据 |
|---|---|---|---|
| 1 | 基线 worktree（`/tmp/r04-g02-baseline`，checkout 1692d2314）：`cargo test -p lingxi-service --test rr1_g02_r01_red_baseline`（**我自己写的反例**，只用基线公共 API：spawn/wait_terminal/output_snapshot/stdio_grace + fd 计数） | **FAILED（exit 101）**：`I1 VIOLATED: the settled collector changed after the terminal phase (total_bytes Some((7, 14)))` | `red_reviewer_own_baseline.txt` |
| 1b | 同上反例的 fd 半边变体（fd 断言前移） | **FAILED**：`I2 VIOLATED: pipe read ends still open after settle (12 vs 10)`（DBG：fds_before=10 fds_after=12） | `red_reviewer_fd_half_baseline.txt` |
| 2 | 候选对照树（`/tmp/r04-g02-candidate` = 基线 + 复制的候选三文件）：同一反例 | **ok. 1 passed**（2.7s，I1+I2 均过） | `green_reviewer_own_candidate.txt` |
| 3 | 主树（候选）：`cargo test -p lingxi-service --test r04_rr1_f02_reaper_cleanup`（全套） | **ok. 11 passed; 0 failed**（13.05s） | `t05_suite_main.txt` 同目录随跑（全套输出在终端留档；逐测试行见 logs） |
| 4 | 主树：C01 三腿 + C02 两腿逐个 `-- --exact` 独立运行 | 各 **ok. 1 passed**（0.72s/0.73s/2.32s/1.52s/2.48s） | `per_cid_c01_c02.txt` |
| 5 | 主树：`cargo test -p lingxi-service --test r04_t05_process_tools` | **ok. 14 passed; 0 failed**（含 A09、A10、加强版 parent-exit） | `t05_suite_main.txt` |
| 6 | 主树：`cargo test -p lingxi-service --test r04_t05_registry_capacity`（G01 套件） | **ok. 9 passed; 0 failed** | `g01_registry_suite_main.txt` |
| 7 | 主树：`cargo test -p lingxi-service --lib`（procsupervisor 内嵌单测在内） | **ok. 296 passed; 0 failed**（含 6 个 F02 新单测） | `lib_unit_tests_main.txt` |
| 8 | 主树：`cargo clippy --workspace --all-targets --locked -- -D warnings` | **0**（无警告输出） | `gate_clippy.txt` |
| 9 | 主树：`cargo fmt --all -- --check` | **0** | `gate_fmt.txt` |
| 10 | 候选对照树：**我的 C02 变体**（grace=900ms、终止原因=Shutdown，与执行者测试不同参数）+ **我的合法对照**（正常运行树取消） | **ok. 2 passed**：变体=信号日志空+`group_signal_skipped` 审计+无 `killpg_sent`+收据 `Terminated{Code(0), reclaimed=true, drained_stdio=false}`+持端孙进程与哨兵存活；对照=**恰好 1 条** `Group(pgid)` 信号、`ownership="verified_at_send"`、`syscall_result=0`、观察退出=Signal(9)/137、树上 3 个真实 pid 全死、哨兵存活 | `reviewer_variants.txt` |
| 11 | 基线 worktree：t05 全套 12 次循环（假拒归因尝试） | **12/12 pass，0 失败**（我的轻载环境未复现执行者的 4/12 假拒） | `baseline_flake_attribution_reviewer.txt` |
| 12 | 基线 worktree：我写的 200 连发 fast-exit 压测（×2，其中一次加 8 路自建 CPU 负载，负载进程测后 kill） | 均 **ok**（0 假拒） | `fastexit_burst_baseline.txt`、`fastexit_burst_baseline_loaded.txt` |
| 13 | 候选对照树：**revert 实验**——临时恢复旧"`getpgid==-1` 即拒绝"分支后跑单测 `verify_group_admits_a_reaped_child_of_ours_and_the_live_leader` | **FAILED（exit 101）**：`a reaped-and-gone pid of our just-spawned child admits the record (the baseline refused it with a false EXEC_SPAWN_FAILED)`；恢复候选文件后同测试 **ok**，且文件与主树逐字节一致（diff 核实） | `red_fastexit_unit_reviewer.txt` + 终端输出 |
| 14 | 候选对照树：t05 全套 20 次稳定性循环 | **20/20 pass** | `candidate_stability_reviewer.txt` |
| 15 | 主树：A10（`r04_a10_pty_input_reads_resize_interrupt_and_exit`）孤立运行 10 次 | **10/10 pass** | `a10_isolated_reviewer.txt` |

（`cargo run -p xtask -- verify-stage R04` 按 §七/总控分工属 G05，本单未运行；`check-contracts`/`check-boundaries` 为执行者门禁，非本 Reviewer 最低集合，未复跑。）

## 2. 旧红新绿（独立、自己写的反例）

反例形态（我独立编写，非执行者版本的复制）：直接子 `/bin/sh -c` 退出，后台孙进程持两端并在 ~1s/~2s 慢写；stdio grace=250ms。两条外部可观察不变量：

- **I1**：终态后 settled collector 逐字节冻结——基线**红**（total_bytes 7→14，`LATE_A\n` 被脱管泵追加：`timeout(grace, pump)` 超时把 JoinHandle 丢给 tokio 语义的 detach，泵继续持有读端并更新已结算采集器）。这正是总控 §Tokio 行为依据所指的行为。
- **I2**：fd 表回到 spawn 前基线——基线**红**（10→12，两条读端仍被脱管泵持有）。

同一测试在候选（含候选文件的对照树）**全绿**。红/绿两侧命令一致（--locked），退出码与关键输出已存证（§1 #1/#1b/#2）。
结论：**F02 核心"reaper 只丢 JoinHandle"缺陷在基线真实存在，候选真实关闭，红→绿成立。**

## 3. 逐 C-ID 独立结论

### R04-RR1-F02-C01 孙进程持有输出端 — **通过**
- 执行者三腿（工具面 one-shot / PTY 家族 / 慢写对抗）在我主树独立运行全绿（§1 #4）。
- 我的独立反例从外部复证了同一证据链（collector 冻结 + fd 回落），且 fd 证据在基线上确有判别力（I2 红）。
- `pumps_alive==0`、`reclaimed=true`（abort 后 join 观察）、`drained_stdio=false`（诚实）、2×`output_task_closed_after_abort`、
  信号日志空（自然退出永不发信号）、持端孙进程按 `cmd &` 语义存活、PTY 腿 master 全关+transcript 冻结——测试断言与实现一致。
- "不以返回耗时短代替回收证据"：C01 断言的是观察事实（collector/audit/fd/信号日志），非耗时。

### R04-RR1-F02-C02 已 reap 未排空窗口 — **通过**
- 执行者两腿独立运行绿；**我的变体**（不同 grace=900ms、终止原因=Shutdown）同样绿：真实 wait 边界（轮询至
  `child_reaped==true && 非终态`）内 terminate → **信号日志空 + 审计 `group_signal_skipped`（reap 原因）+ 无 `killpg_sent`**；
  收据 `Terminated{fact: Code(0), reclaimed: true, drained_stdio: false}`——直子真实退出 + 管道状态如实，无虚构 137。
- 持端孙进程与哨兵存活（无失效归属信号到达任何进程）；fd 回基线。
- 身份失效形态（pid 回绕）无法在宿主合法制造——由发送时内核身份复核单测覆盖
  （`group_signal_is_skipped_when_the_kernel_identity_disagrees`，lib 运行绿；dummy pid 无人持有 ⇒ 拒绝）。

### R04-RR1-F02-C03 双泵预算和延后观察 — **通过**
- 三腿（双泵卡死/泵 panic/退出-取消竞走 15 轮）全套绿。
- 代码级核对：`drain_output_tasks` 的 `join_deadline = grace_deadline + pump_abort_join` 是**两相共享截止时间**
  （第一条流的 abort-join 消耗的时间会压缩第二条流的余量），整个尾部 ≤ `stdio_grace + pump_abort_join`，无多段期限。
  `PumpEnd` 四变体（finished/panicked/aborted-observed/end-unconfirmed）穷尽且各有审计 kind；
  `reclaimed = drain.all_ends_observed`、`drained_stdio` 仅来自自然 EOF——基线的无条件 `inner.reclaimed = true` 已删
  （基线 1741 行 vs 候选 2369 行，git show 核对）。`output_drain_summary` 记录耗时与各处置计数。
- 泵 panic 腿：`PumpFaultPoint::PanicNext` 只决定 panic **何时**发生，drain/join 走的是产品真实路径（非 mock）。

### R04-RR1-F02-C04 压力下不遗留 — **通过**
- 12 轮（ring_cap=3、live_cap=3，>3× 环容量；每 4 轮交替静默/写端持有者，终态淘汰与延后退出并发）逐轮：
  `pumps_alive==0`、`reclaimed==true`、`live_handles()` 空、fd 表=基线；终态 `retained_record_count() ≤ 3`、
  `owned_task_count()==0`、哨兵全程存活；自有孙进程按精确 pid 清理。全套绿。

## 4. 重点对抗项

### 4.1 合法对照（A09 真实清理链不退化）— **通过**
- t05 `r04_a09_managed_tree_is_killed_and_the_unrelated_sentinel_survives` 绿（killpg_sent 审计 + 受管树 3 pid 全死 + 哨兵存活 + reclaimed 收据）。
- 我的自有对照更直接：取消一棵**仍在运行、未 reap、可证明归属**的树 → `verification_signal_log()` **恰好 1 条**
  `Group(pgid)`、`ownership="verified_at_send"`、`syscall_result=0`（killpg 真实成功）、观察退出 **Signal(9)**（真实 wait 观察，
  非虚构）、组内孙进程真死、哨兵存活。**归属门没有过度触发：该杀的仍被真实 killpg 杀掉。**

### 4.2 `verify_group` 的 `getpgid==-1` 接纳（行为变化，重点审）— **安全且方向正确；量化假拒频率未能在本机复现**
- **-1 的语义**：POSIX/Linux/macOS 的 `getpgid(2)` 唯一记载错误是 `ESRCH`（不存在 EPERM 变体；getpgid 不需要权限）。
  因此 -1 ⇒ 该 pid 已不在进程表（连僵尸都不是——僵尸仍报告其 pgid）。**活进程（包括无权限的活进程）不会产生 -1**，
  "把权限失败的活进程也接纳"不成立。存活但报告**不同** pgid 的 pid 仍走 mismatch 拒绝分支（不变）。
- **接纳后的终止链安全**：该记录的任何组信号需过 `prove_group_ownership`（记录锁内）：`child_reaped==false` **且**
  实时 `getpgid(pid)==pgid`。对已死的 fast-exit 子进程：reaper 的 `wait()` 立即解析并发布 `child_reaped`（grace 之前）；
  发布前的窗口内实时 getpgid 对已死 pid 返回 -1 ⇒ mismatch ⇒ 跳过。**已死进程无信号需求，路径封闭。**
- **残余窗口**：reap 与发布之间数条指令内 pid 被回收并成为新会话组长的窗口在无 pidfd 平台（macOS 无 pidfd）不可闭合，
  模块文档如实记载；该窗口在**基线的 `pgid==pid` 第一分支同样存在**（基线也接纳"getpgid 恰好等于 pid"的回收后身份），非本次引入的回归。
- **假拒的独立验证**：执行者称干净基线 12 循环 4 次 `EXEC_SPAWN_FAILED: getpgid(pid) returned -1`。我的轻载环境：
  基线 t05 全套 12 次循环 + 200 连发 fast-exit ×2（其中一次 8 路 CPU 负载）**全部绿，未复现**——该竞态需跨运行时线程的
  SIGCHLD 机会性回收恰好落在 spawn 与 getpgid 之间的微秒窗，频率强依赖机器/负载，**执行者的 4/12 量化数字未获本机证实**。
  但行为差异本身我用确定性方法独立证明：在我的候选对照树临时恢复旧"-1 即拒绝"分支 → 单测
  `verify_group_admits_a_reaped_child_of_ours_and_the_live_leader` **红**（真实子进程、我方 wait 回收后 getpgid=-1 被旧逻辑拒绝）；
  恢复候选 → 绿。修复方向（接纳"我方已完整运行完的子进程"）正确，且候选 t05 全套 20/20（我的循环）。

### 4.3 观察缝零产品调用点 — **通过**
`owned_task_count / pumps_alive / verification_signal_log / arm_pump_fault_for_verification / retained_record_count /
retained_record_ids / arm_spawn_fault_for_verification`：grep 全仓，除 `procsupervisor.rs` 自身定义外仅两个测试文件引用，
**产品代码零调用点**。

### 4.4 `child_reaped` 发布时序与锁原子性 — **通过**
- one-shot 与 PTY reaper 均为：`child.wait().await` 解析 → `note_child_reaped`（记录锁内置位+审计）→ grace drain。
  发布在 grace **之前**，整个排空窗口被归属门覆盖。
- `mark_terminating`（方法化）在**同一记录锁内**完成 phase 置位 + `prove_group_ownership`（child_reaped 检查 + 实时 getpgid）
  + killpg 发送（经 `send_group_kill` 记入穷尽式信号日志）或 `group_signal_skipped` 审计——reap 事实发布与发送/跳过决策原子。
- `Drop` 兜底走同一门（仅 Proven 才发）；锁序 state→inner→signal_log 全仓一致，无逆序死锁路径。

### 4.5 t05 加强"只加强未削弱" — **通过**
逐行对照 git diff：`adversarial_parent_exit_with_grandchild_holding_the_pipe_is_bounded` 原断言全部保留
（elapsed<1500ms @941、PARENT_DONE @946、exit 0 @947-950、孙进程存活 @959-962），+50 行为纯新增断言
（reap/reclaimed/drained 诚实性、2×abort-观察审计、collector 冻结、信号日志空）。未删任何旧测试、未降任何门禁。

### 4.6 测试是否 mock 掉被验收对象 — **否**
全部用真实 `ProcessSupervisor` + 真实 `/bin/sh` 子进程 + 真实管道/PTY + 真实 OS 信号；哨兵/孙进程按精确 pid 清理；
目录唯一命名。`PumpFaultPoint`/`SpawnFaultPoint` 只决定故障**何时**触发，所锻炼的 drain/回滚代码是产品路径。

## 5. A10 三方观测（如实并列，不强行调和）

| 观测方 | 条件 | 结果 |
|---|---|---|
| G01 执行者（E02 报告，线索） | 孤立运行 | 3/3–5/5 红（macOS readline 在 SIGINT 后丢弃 type-ahead 的时序效应） |
| G02 执行者（E01 报告，线索） | 孤立 16 次 + 并行 20 次 | 16/16 + 20/20 绿 |
| **本 Reviewer（独立）** | 主树孤立 10 次 | **10/10 绿** |

本机观测与 G02 执行者一致、与 G01 执行者矛盾；该腿与本 F02 修复无直接耦合（PTY 交互语义未改），维持"当前条件未复现"的如实记录，不下缺陷结论，也不做无据修复。

## 6. 问题清单（全部非阻塞，无 FAIL 项）

| ID | 严重度 | 定位 | 违反要求/后果 | 根因 | 同族 | 修复要求（若采纳） | 重跑集合 |
|---|---|---|---|---|---|---|---|
| G02-R1-OBS-01 | Low | `procsupervisor.rs` `Drop for ProcessSupervisor`（约 2045–2070 行） | 非违反硬性要求：Drop 路径在归属不可证明时**静默跳过**，未像 `mark_terminating` 那样留 `group_signal_skipped` 审计。后果：Drop 期跳过无审计痕迹（安全性不受影响——信号日志对"实际发送"仍是穷尽的，未发生不可证明发送） | Drop 循环只处理 Proven 分支 | 无（仅 Drop） | 可选：Drop 跳过时同样 push `group_signal_skipped` 审计以对齐 | `r04_rr1_f02_reaper_cleanup` + t05 全套 |
| G02-R1-OBS-02 | Low | `note_child_reaped`/`drain_output_tasks`/`finalize_exit` 的 `wall_now_ms()` | 审计时间戳用墙钟 SystemTime 而非注入 ServiceClock，测试时钟不可控；NTP 跳变下审计行顺序可能呈现倒挂（纯外观） | 新增辅助沿用墙钟 | 无 | 可选：统一走 now_ms | lib + F02 全套 |
| G02-R1-OBS-03 | Info | 执行者报告 §3/§5 | 报告称新测试文件"12 用例"，实际 **11** 个（repro+fastexit+C01×3+C02×2+C03×3+C04）。报告算术笔误，无代码影响 | — | — | 无需修复（以测试实际为准） | — |
| G02-R1-OBS-04 | Info | `group_has_members`（reaper 在子进程已 reap 后探测） | `killpg(pgid,0)` 在 pgid 已回收时可能对无关组返回 0，审计行 `leftover_group_after_exit` 可能失真；仅审计文本、从不参与决策；signal-0 探针不入信号日志属设计（日志只记真实信号） | 探针时点在 reap 之后 | 无 | 可选：审计文本标注"best-effort" | F02 全套 |
| G02-R1-OBS-05 | Info | `r04_rr1_f02_reaper_cleanup.rs` C02 对抗腿 | "无组信号"断言写成 `s.target == SignalTarget::Child(0)`（不可能的 target）——功能正确但费解 | — | — | 可选：改为 `!matches!(s.target, SignalTarget::Group(_))` | 该文件 |

未复现项（如实）：基线 fast-exit 假拒的**动态频率**（执行者 4/12）在本机轻载下未能触发（12 循环 + 400 连发 + 8 路负载全绿）；
行为差异已由我的确定性 revert 红绿实验独立证实（§4.2）。

## 7. 残余窗口（文档化，非缺陷关闭缺口）

无 pidfd 平台上，内核 reap 与 `child_reaped` 发布间数条指令内 pid 回收并成为新会话组长的窗口不可在用户态闭合；
候选将其如实写入模块文档并以发送时实时复核兜底，C02 对抗在此边界内验证。该窗口基线同样存在（且基线还额外错杀
fast-exit 合法子进程），候选严格优于基线。

## 8. 裁决依据汇总

- C01–C04 全部独立有效通过（我的运行 + 我的自有变体/反例，非仅复述执行者）。
- 旧红新绿成立（我自己的基线 API 反例：I1 collector 变异 + I2 fd 泄漏，候选双修复）。
- 合法对照不退化：正常运行树取消仍发出恰好一次归属验证的真实 killpg 且孙进程真死、哨兵存活。
- 同族回归零失败：t05 14/14（且候选 20/20 稳定）、G01 9/9、lib 296/296、clippy -D warnings 0、fmt 0。
- 观察缝零产品调用点；测试无 mock 顶替；t05 加强纯增量。
- 问题清单全部为 Low/Info 观察项，无本组阻塞、无未修复同族回归。

VERDICT: PASS
