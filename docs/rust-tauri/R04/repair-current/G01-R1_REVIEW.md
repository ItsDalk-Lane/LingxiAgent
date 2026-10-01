# G01 独立审查报告 — R04-RR1-F01（登记容量先于派发 + 拒绝路径进程归属）

- Reviewer：**REVIEWER-R04-RR1-G01-R01**（全新代理，未参与 G01 执行/修复，与 E01/E02 无共享上下文）。
- 审查对象：基线 `773d5a6968b87650cd135cc5684707f81d1acda0`（分支 `codex/rust-tauri-migration`）→ 候选＝当前工作区未提交改动
  （`rust/crates/lingxi-service/src/procsupervisor.rs`、`exectools.rs`、`lib.rs` + 新测试 `tests/r04_t05_registry_capacity.rs`）。
- 审查日期：2026-10-01。工具链：`~/.cargo/bin/cargo` 1.98.1（rustup 锁定），全部命令带 `--manifest-path rust/Cargo.toml --locked`。
- 我的证据目录：`artifacts/rust-tauri/R04/RR1-G01-R1/logs/`（本报告所有指针均指向该目录）。
- 执行者报告（`G01-E01_REPORT.md`）仅作线索读取，未采信其任何日志作为我的复测结果；下述全部结果为我本人实际运行。

## 1. 独立复测：实际执行的命令与退出码

### 1.1 旧红（基线 /tmp 独立 worktree，`git worktree add --detach /tmp/r04_r1_g01_baseline 773d5a696`）

| # | 命令 | 结果 | 证据 |
|---|------|------|------|
| R1 | 把候选新测试文件复制进基线 worktree 后 `cargo test --locked -p lingxi-service --test r04_t05_registry_capacity` | **exit 101，编译红**：4 个 error（`SpawnFaultPoint` unresolved import ×1、`arm_spawn_fault_for_verification` no method ×3） | `logs/r01_baseline_fullfile_test.txt`（尾部 `exit=101`） |
| R2 | 我的**基线可编译隔离 repro**（从候选测试文件截取 harness + repro 用例原文 1–534 行，仅去掉基线不存在的 `SpawnFaultPoint` 导入；文件 `zz_r01_isolated_repro.rs`，运行后已随 worktree 删除）`cargo test --locked -p lingxi-service --test zz_r01_isolated_repro` | **exit 101，运行时红（反例本体）**：`assertion 'left == right' failed: R04-RR1-F01: the refused one-shot must not have executed`，哨兵 sha256 `7e8f4d94…` ≠ `678bf03b…` —— 被拒命令在基线上**真实执行并覆写了哨兵** | `logs/r01_baseline_isolated_repro.txt` |

说明：R1 是编译级红（新 API 在基线不存在），单独不足以证明缺陷；R2 是我在基线上独立得到的**运行时旧红**，repro 用例断言集与候选文件中同名用例逐字一致（提取自候选文件），证明的是同一反例。缺陷确证：基线 `register()`（基线 procsupervisor.rs:824）在 `command.spawn()` 之后才检查 `live_cap`，RegistryFull 返回路径上子进程已派发且无人所有（kill_on_drop(false)、无 reaper、无 ProcessOwnershipGuard）。

### 1.2 新绿（主树＝候选，逐 C-ID 独立进程运行）

| # | 用例（`cargo test --locked -p lingxi-service --test r04_t05_registry_capacity <name> -- --exact`） | 结果 | 证据 |
|---|------|------|------|
| G0 | 全套件（9 用例一次运行） | **exit 0，9 passed / 0 failed**（4.36s） | `logs/r01_candidate_fullfile_test.txt` |
| G1 | `repro_rr1_f01_registry_full_refusal_must_not_execute_the_command` | exit 0 | `logs/r01_candidate_per_cid_repro….txt` |
| G2 | `rr1_f01_c01_registry_full_refuses_with_zero_dispatch`（C01） | exit 0 | `logs/r01_candidate_per_cid_rr1_f01_c01….txt` |
| G3-G5 | `rr1_f01_c02_two_one_shots…` / `…two_ptys…` / `…one_shot_and_pty…`（C02 三腿，屏障固定交错） | 各 exit 0 | `logs/r01_candidate_per_cid_rr1_f01_c02_….txt` ×3 |
| G6-G8 | `rr1_f01_c03_one_shot_group_verify…` / `…pty_group_verify…` / `…pty_open_failure…`（C03 三变体） | 各 exit 0 | `logs/r01_candidate_per_cid_rr1_f01_c03_….txt` ×3 |
| G9 | `rr1_f01_c04_same_process_usable_after_repeated_refusals_and_releases`（C04） | exit 0 | `logs/r01_candidate_per_cid_rr1_f01_c04….txt` |

### 1.3 同族回归

| # | 命令 | 结果 | 证据 |
|---|------|------|------|
| F1 | `cargo test --locked -p lingxi-service --test r04_t05_process_tools`（候选，既有 14 用例，未改动） | **exit 0，14 passed**（含 r04_a09、r04_a10、grandchild、output storm、utf8、unresponsive 等对抗腿） | `logs/r01_candidate_r04_t05_process_tools.txt` |
| F2 | `cargo test --locked -p lingxi-service --lib procsupervisor`（候选，含 3 个新 live-slot 单测） | **exit 0，9 passed** | `logs/r01_candidate_procsupervisor_unit.txt` |

### 1.4 A09/A10 孤立复跑（干净基线 worktree；先移除我的两个测试副本恢复 pristine，`git status` 干净后运行）

| # | 命令 | 结果 | 证据 |
|---|------|------|------|
| A1-A3 | 基线 `r04_a09_managed_tree_is_killed… -- --exact` ×3 | **3/3 exit 0**（0.11s / 0.09s / 0.09s） | `logs/r01_baseline_a09_run{1,2,3}.txt` |
| A4-A6 | 基线 `r04_a10_pty_input_reads_resize_interrupt_and_exit -- --exact` ×3 | **3/3 exit 0**（各 0.58s） | `logs/r01_baseline_a10_run{1,2,3}.txt` |
| A7-A9 | 候选 A10 同法 ×3 | **3/3 exit 0**（各 0.58s） | `logs/r01_candidate_a10_run{1,2,3}.txt` |

### 1.5 门禁复跑（候选）

| # | 命令 | 结果 | 证据 |
|---|------|------|------|
| D1 | `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | **exit 0** | `logs/r01_gate_fmt.txt` |
| D2 | `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | **exit 0** | `logs/r01_gate_clippy.txt` |

### 1.6 现场清理（安全边界）

- /tmp worktree：`git worktree remove --force /tmp/r04_r1_g01_baseline`，`git worktree list` 仅剩主树，目录已不存在。
- 我自有测试进程：基线 panic 运行遗留 1 个孤儿 `/bin/sleep 300`（pid 89513，ppid=1，启动时间与我的运行吻合）已按精确 pid 清理并复核消失；首个受管进程已由基线 supervisor Drop 的 killpg 兜底回收（仅未登记哨兵成孤儿——恰与缺陷结构一致）。
- 我自有测试目录：我 panic 运行遗留的 `lingxi-r04rr1f01-root-89512-*` 已删；$TMPDIR 中其余 16 个 `lingxi-r04rr1f01-*` 目录属执行者早前进程（pid 非我），不属我的清理范围，仅记录。
- 最终 `ps` 核查：无 `sleep 300` / `dispatch-helper` / 相关测试进程残留。

## 2. 逐项核对结论

### 2.1 reserve_live_slot 是否真原子（修复要求 1）— 成立

`procsupervisor.rs:946-956`：单个 `state` 互斥锁临界区内完成 `live_count >= live_cap` 检查 + `+= 1`，非锁外预查（无 TOCTOU）。调用点先于一切 OS 资源创建：
- one-shot：`spawn_oneshot` 行 1020 reserve → 行 1044 `command.spawn()`（reserve 与 spawn 之间无任何 OS 资源创建）；
- PTY：`spawn_pty` 行 1194 reserve → 行 1206 `open_pty_pair()`（含 PtyPairOpen 注入位也晚于 reserve，且该注入零 OS 资源）。
全仓 `live_count` 变更点仅 3 处（951 reserve +1 / 889 armed Drop −1 / 977 settle 首次 −1），`records.insert` 仅 1 处（行 878，`LiveSlot::commit` 内）；旧 `register()` 已整体移除。无绕过 guard 的路径。

### 2.2 LiveSlot commit/Drop/settle 恰好一次（修复要求 2，所有交错）— 成立

- 释放互斥的两条路径结构性排他：armed Drop 释放的 guard 必然未 commit（commit 在同一把 state 锁内置 `armed=false` 并安装 record）；已 commit 的 record 由 settle 释放，而 settle 以 `records.get(id)` 命中为前提（未 commit 的名额无 record，settle 对其是 no-op）。
- settle 恰好一次：`record.inner.settled` 标志在锁内 check-and-set；重复 settle、迟到 settle、ghost id settle 均为 no-op（单测 `live_slot_commit_transfers_release_to_settle_exactly_once` 覆盖，且 C03/C04 用容量算术在真实链上复核：零释放会拒 refill、双释放会放行 over，实测都未发生）。
- commit 前 panic：`LiveSlot` 是普通 Drop guard，unwind 时释放名额（单测 `live_slot_drop_after_panic_style_abandon_still_releases` + 8 轮无漂移）。
- Future 丢弃：两条 spawn 路径 reserve→commit 的成功路径上**无任何 await 点**（tokio `Command::spawn` 同步、`verify_group` 同步、record 构造同步），丢弃只能发生在失败分支的 `child.wait().await`，此时 kill 已同步发出、guard Drop 仍归还名额（见 2.7 残留观察 O-2）。

### 2.3 派发后失败的回滚（修复要求 3）— 成立

`verify_group` 失败分支（one-shot 行 1055-1066；PTY 行 1256-1269）：`libc::kill(pid, SIGKILL)` + `child.wait().await`（真实 reap，非仅返回 Err）+（PTY）显式 `drop(master_fd)`（不依赖"关 PTY 恰好发信号"）；`note_dispatched_rollback` 把"已派发、已 SIGKILL、已回收 + pid"写进错误原文。`kill_on_drop` 保持 false，责任链未被 kill_on_drop 顶替。C03 两条 GroupVerify 腿在真实链上断言：错误含 `EXEC_SPAWN_FAILED` + `process-group ownership` + `dispatch DID happen`，从错误解析出的 pid 真实不存活（kill+reap 在错误返回前完成）、fd 表前后相等、名额恰好归还一次。

### 2.4 三重词汇区分（修复要求 4）— 成立

- `EXEC_PROCESS_REGISTRY_FULL`（exectools.rs:118，`ErrorCode::BudgetExceeded`，注释明确与网关 prepared cap 的区分）vs `EXEC_SPAWN_FAILED`（UpstreamUnavailable）vs 网关 `GatewayRefusal::PreparedRegistryFull`（toolgateway.rs:429 `gateway_prepared_registry_full` / 行 501 "prepared-invocation registry is at its cap"）。
- 调用侧映射落在 `exectools.rs:685-705` 的唯一 `supervisor.spawn` 调用点（one-shot 与 tty 共用，两路拒绝词汇一致）。
- C01 测试同时做正向与双负向断言（是 supervisor 容量词、不是 EXEC_SPAWN_FAILED、不是网关 prepared 词）；测试 harness 将网关 prepared cap 固定 1024（远高于 live_cap），保证触发的是 supervisor 容量。旧测试无任何用例依赖旧的"容量满 → EXEC_SPAWN_FAILED"合并行为（`terminate_after_natural_exit…spawn_failures_are_loud` 测的是缺二进制 Io 失败，14/14 通过；全仓其余 `RegistryFull` 命中属 quotas/sessions 等无关注册表）。

### 2.5 SpawnFaultPoint 注入钩子 — 无产品调用点，且不遮蔽真实检查

全仓 grep：`arm_spawn_fault_for_verification` 仅 procsupervisor.rs:917 定义 + 新测试文件 3 处调用，零产品调用点；仅进程内持有 supervisor 的 Rust 代码可 arm，不可经模型/工具输入到达。`GroupVerify` 注入位在真实 `getpgid(pid)==pid` 检查**通过之后**才触发（procsupervisor.rs:1161），不可能把真实失败伪装成通过；它演练的回滚代码与真实失败分支逐字相同。结论：这是可控故障点，不是 mock。

### 2.6 测试是否 mock 掉被验收对象 — 没有

新测试走真实链：真实 `ToolInvocationGateway`（prepare → execute_prepared，CallerSurface::UserRun）→ 真实 `register_process_tools` 执行器 → 真实 `ProcessSupervisor`（POSIX 进程组、killpg、有界清理）。无任何 Provider/工具替身。对抗证据面向真实 OS：派发计数（helper 落盘计数）、哨兵 sha256、`/dev/fd` fd 表计数、spill 目录文件数、`live_handles` 注册表快照、按 pid 的存活探针——满足"F01 检查实际命令是否根本未派发，而不是只查注册表没新行"。

### 2.7 执行者遗留主张复核（A09/A10 归因）

- A09：我在干净基线孤立 3/3 绿（0.09-0.11s），与执行者"孤立恒绿、全量并行负载下偶发空 pid 文件竞态"的归因一致；该竞态与 G01 改动无涉（G01 未触碰 A09 代码路径）。
- A10：执行者称基线孤立确定性红（3/3）；**我在同一基线孤立 3/3 绿，候选侧孤立 3/3 绿**——其红主张在我处未复现（环境/负载差异）。由于两侧在我处均无失败，不存在可归因于 G01 的回归；若其所述时敏现象在特定负载下存在，属 F02/G02（PTY 打断/观察链）范围，与 G01 的容量预留/回滚改动无代码交集（diff 未触碰 PTY 读写与打断路径）。该差异如实记录：执行者报告的 A10 红主张未经我证实，但其"非 G01 引入"的结论方向与我的零失败观测不矛盾。

### 2.8 workerrpc / mcpbridge 消费面 — 不受影响

两文件自带子进程生命周期（自有 Command 构造与并发上限），不经过 ProcessSupervisor live 注册表，不引用 `EXEC_SPAWN_FAILED`/`EXEC_PROCESS_REGISTRY_FULL`/`SpawnFailure`（grep 零命中）；`clippy --workspace --all-targets --locked -D warnings` exit 0 覆盖其编译。其余 spawn 点（sandbox.rs helper、xtask、spike bin）同理在 G01 范围外且未受词汇分离影响。

## 3. C01–C04 独立裁决

- **C01 登记已满时零派发 — 通过**：live_cap=1、第一受控进程运行中，第二个 one-shot 经真实工具链发起；拒绝为 `EXEC_PROCESS_REGISTRY_FULL`（三重区分断言）；1s 观察窗内哨兵 sha256 恒不变 + 内容仍 BASELINE + **OS 派发计数 0**（对抗要求：不只查 Err）+ fd 表不变 + spill 零新文件 + 注册表仍恰为第一进程且存活 + 无关哨兵存活。我逐 C-ID 独立运行 exit 0。
- **C02 并发争最后名额 — 通过**：live_cap=2 占 1 剩 1，`tokio::sync::Barrier` 固定交错（非随机重跑）；三腿（one-shot×2 / PTY×2 / 混合）各自恰 1 成功 + 1 `EXEC_PROCESS_REGISTRY_FULL` 拒绝；真实 OS 派发计数恰 +1（胜者回执，PTY 腿有界等待后断言）；胜者 PTY 时真实 write_stdin 轮询 running（合法对照）；轮换 3 轮 refill 无计数泄漏；live_handles 从不超 cap。三腿我均独立运行 exit 0。
- **C03 启动后异常补偿 — 通过**：组身份验证失败（派发后，one-shot 与 PTY 两变体，各 3 轮/1 轮注入）：kill+reap 在错误返回前完成（pid 真实不存活）、PTY master fd 显式关闭（fd 表前后相等）、派发事实含 pid 保留在错误原文、名额恰好归还（容量算术：refill 恰可 1 个、over 被拒）；PTY 初始化失败（派发前）：零派发、零 fd、live_cap=1 下名额立即可用且合法 one-shot 对照 Exit 0。对抗"回滚再次失败"＝重复注入多轮无漂移。全部我独立运行 exit 0。
- **C04 失败后的同进程再使用 — 通过**：同 supervisor 不重启，4 轮（> live_cap=1）occupy → refuse → release；每轮拒绝零派发（计数恒 0、哨兵不变）；重复取消（每轮 3 次 terminate 已结算记录）均 AlreadyTerminal 无害；迟到 write_stdin 轮询如实报 terminated；之后合法 one-shot（echo Exit 0）与 PTY（启动/轮询/关闭）仍可用，再跑一轮完整 occupy/release；无关哨兵全程存活；无孤儿。我独立运行 exit 0。

## 4. 发现的问题

**无阻塞问题、无同族未修复回归。** 以下为非阻塞观察（均不构成 FAIL 条件）：

- **O-1（info）旧红的层级说明**：候选完整测试文件在基线只能得到编译红（新 API 不存在），运行时旧红需隔离 repro（我已独立完成，见 §1.1-R2）。repro 用例本身按设计只用基线稳定词汇（其文件内注释已声明），这不是缺陷，但后续工作单若引用"完整文件在基线红"应区分编译红/运行红两层。
- **O-2（info）回滚 await 点的极端丢弃边界**：失败分支 `child.wait().await` 期间调用方丢弃 Future 时，kill 已同步发出、名额由 guard 归还，子进程最终 reap 依赖 tokio orphan queue。属 F02（reaper/观察链）家族残留，执行者已如实披露；不影响 G01 的 C01-C04 判定（派发事实与 kill 责任未丢失）。
- **O-3（info）执行者 A10 红主张未复现**：执行者报告称基线孤立 3/3 红、修复树孤立 5/5 红；我的孤立复跑基线 3/3 绿、候选 3/3 绿。其归因方向（非 G01 引入）与我两侧全绿的观测不矛盾，但其"确定性红"描述与我处环境不一致，记录为报告差异，不作为 G01 判定依据。若 G02/F02 工作单处理 A10 时序，应以可复现证据为准。
- **O-4（info）`note_dispatched_rollback` 的 `other =>` 分支**把假想的未来派发后失败变体折叠为 `Invalid` 以保证全匹配——有意的完备性选择（注释已声明），当前无实际变体命中。
- **O-5（info）执行者遗留临时目录**：$TMPDIR 存在其早前运行的 16 个 `lingxi-r04rr1f01-root-*` 目录（pid 非我），按边界不由我清理，仅登记。

## 5. 范围与未验证项（如实）

- verify-stage R04 / check-contracts / check-boundaries / workspace 全量：未在本审查复跑（属 G05 收口与总控门禁职责；我复跑的集合为工作单指定的最低集合 + fmt/clippy）。执行者报告的对应 exit 0 仅作线索，未采信为我的结果。
- `cfg(not(unix))` 分支：本机 macOS arm64 未机器验证，维持平台递延登记。
- 我未修改主树任何文件；主树中 `docs/rust-tauri/ORCHESTRATOR_PROGRESS.json` 的工作区差异经核对为总控层的重开记录（非执行性内容），非 G01 执行代理所写代码。

## 6. 结论

C01–C04 全部当前到期检查经我独立运行有效通过（真实网关 + 真实工具链 + 真实 supervisor，OS 级对抗证据）；旧红新绿双向独立复现（基线运行时哨兵反例 exit 101 → 候选 exit 0）；同族回归（r04_t05_process_tools 14/14、procsupervisor 单测 9/9、workerrpc/mcpbridge 编译与源码核对）无回归；fmt/clippy -D warnings 均为 0；无本组阻塞项；无未修复同族路径。

VERDICT: PASS
