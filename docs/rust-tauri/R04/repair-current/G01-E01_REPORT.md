# G01 工作单报告 — R04-RR1-F01（容量预留与派发失败回滚）

- 工作单：G01 → R04-RR1-F01（进程已启动后才检查登记容量，拒绝路径失去进程归属）
- 执行：**E01 中断 → E02 续做完成**。E01 完成基线红复现与产品修复主体后中断；E02（本代理，2026-10-01）核实 E01 改动、修复测试侧缺陷、补齐两层自查与门禁、落盘证据。
- 基线：`773d5a6968b87650cd135cc5684707f81d1acda0`（分支 `codex/rust-tauri-migration`）。修复为工作区未提交改动（按总控纪律，不 commit/push）。
- 状态：**READY_FOR_REVIEW**（不自行宣布独立 PASS；三层检查中的独立审查待 Reviewer）。

## 1. 红 → 绿记录

- 红复现（E01，证据未改动）：`artifacts/rust-tauri/R04/RR1-G01-E01/repro/red_repro_baseline_773d5a6.txt` — 基线 773d5a6 上
  `cargo test -p lingxi-service --test r04_t05_registry_capacity` exit 101，
  `repro_rr1_f01_registry_full_refusal_must_not_execute_the_command` 红：被拒 one-shot 的哨兵摘要已变化（`7e8f4d…` ≠ `678bf0…`），证明登记满的拒绝路径确实派发并执行了命令。
- 绿（E02，同一测试）：`logs/e02_per_cid_repro.txt` exit 0（单独运行）；全套件顺序 1 次 + 并行 4 次全绿（`logs/e02_fullfile_*.txt`）。
- 红测试本身保持不变（仅 E02 修复了它依赖的公共 harness 缺陷与格式，见 §3），断言未降级。

## 2. E01 产品改动审核结论（E02 逐行核实）

结论：**结构正确，予以保留；未发现需要推翻重写的结构性错误。** E02 仅补充一处 re-export 与测试侧修复。

核实要点（对应修复要求 1–4）：

1. **原子预留（要求 1）成立**：`reserve_live_slot`（procsupervisor.rs:946）在单个 state 互斥锁临界区内完成 cap 检查 + `live_count += 1`，先于任何 OS 资源创建（one-shot：在 `command.spawn()` 之前；PTY：在 `open_pty_pair()` 之前）。非锁外预查（TOCTOU），两并发 spawn 不可能同取最后一个名额（C02 屏障腿实测）。
2. **唯一所有者/RAII 恰好一次（要求 2）成立**：名额由 `LiveSlot` guard 独占；`commit(record)` 消费 guard（armed=false）并把释放责任移交给 record 的 settle 路径；`Drop` 仅在 armed 时归还。全仓 `live_count` 变更点仅三处（reserve +1 / armed drop −1 / settle 首次 −1），互斥由 armed 与 `settled` 标志保证。单元测试覆盖：commit 后 drop 不再释放、重复/迟到 settle 不重复归还、ghost settle 不虚增容量、8 轮 reserve/release 无漂移（`logs/e02_unit_live_slot_admission.txt`）。
3. **两条 spawn 路径的失败回滚全覆盖**：
   - one-shot：spawn Io 失败 → guard drop；组验证失败（派发后）→ `kill(pid)` + `child.wait().await` 真回收 + guard drop；管道随 Child drop 关闭。
   - PTY：PtyPairOpen 预检失败（零派发）→ guard drop；open_pty_pair / dup_cloexec 失败 → OwnedFd 自有 drop + guard drop；组验证失败（派发后）→ kill + wait + **显式 `drop(master_fd)`**（不依赖"关 PTY 恰好发信号"）+ guard drop。
   - Future 丢弃路径：reserve→commit 之间唯一 await 点在回滚路径的 `child.wait().await`；若调用方在该点丢弃，kill 已同步发出、guard Drop 归还名额；子进程回收由 tokio 进程驱动的 orphan queue 承接（E02 在样本中实际观察到 `GlobalOrphanQueue::reap_orphans` 在 park 时回收）。此极端边界属 F02（reaper/观察链）家族，如实记为残留观察项，不在 G01 内扩大修改。
4. **派发事实保留（要求 3）**：`note_dispatched_rollback` 把"已派发、已 SIGKILL、已回收"写进错误原文（错误里携带 pid）；不以 kill_on_drop 顶替责任链（kill_on_drop 仍为 false）。
   - E02 审核备注（保留现状）：该函数 `other =>` 分支把未来新增的派发后失败变体折叠为 `Invalid` 以保证全匹配——是有意的完备性选择，注释已声明。
5. **词汇区分（要求 4）**：`EXEC_PROCESS_REGISTRY_FULL`（supervisor 进程容量，ErrorCode::BudgetExceeded）与 `EXEC_SPAWN_FAILED`（启动失败）与网关 `PreparedRegistryFull` 三者分离；测试同时断言"是前者、不是后两者"。网关 prepared cap 在测试中固定 1024（远高于 live_cap），保证触发的是 supervisor 容量。
6. **验证注入钩子的边界**：`SpawnFaultPoint` / `arm_spawn_fault_for_verification` 仅进程内 Rust 代码可调用，全仓无产品调用点（grep 证实：仅 procsupervisor.rs 定义 + 本测试文件使用）；不可经模型/工具输入到达。组验证 fault 仅在真实 `getpgid == pid` 检查通过后触发，演练的是真实失败分支的同一回滚代码。

## 3. E02 对 E01 遗留现场的处理（测试侧缺陷修复）

E01 中断时测试文件已含 9 个用例（超出工作单描述的"仅 1 个红用例"），但存在四类缺陷，E02 逐一修复（`git diff` 可查）：

1. **`serial()` 互斥锁中毒砖死**（真缺陷，曾致全套件挂起）：`try_lock` 的 `Err`（含 `Poisoned`）被当作"忙"无限 sleep——任一测试持锁 panic 后，其余测试永久自旋。修复：`Poisoned` 走 `into_inner()` 恢复（与库内 `lock_or_poison` 同策略）。挂起现场样本（c02c panic 中毒 → c02a 在 serial() 永久等待）已定位并存证。
2. **C02 屏障腿的 PTY 异步回执竞争**（真缺陷）：PTY 竞争者成功即返回 Running，helper 的派发计数行异步落盘；原同步断言 `dispatch_count == 1` 读到 0 而 panic（实测 c02c 稳定红）。修复：PTY 腿改用 `park` 形 helper（exec `/bin/sleep 300`，终端保持 running，"status: running" 轮询确定性成立），派发回执改为有界 `wait_until`（10s）等待后断言恰为 1——非 sleep 碰绿，也非同步读竞争。
3. **9 个编译警告**（clippy -D warnings 会挂）：8 处 `unused_mut`、1 个死常量 `EXEC_TARGET`，已清。
4. **fmt 漂移**：E01 未跑 fmt；`cargo fmt --all` 应用（仅触新测试文件），`--check` 复验干净。另按仓库既有先例（backup_faults.rs 等）为 9 个测试加 `#[allow(clippy::await_holding_lock)]`（serial guard 跨 await 持有是本文件的既定设计，current_thread 运行时无 Send 边界）。

产品代码 E02 仅补一处：`lib.rs` 将 `EXEC_PROCESS_REGISTRY_FULL` 加入 re-export（与 `EXEC_SPAWN_FAILED` 对称；测试经由 `lingxi_service::exectools` 直接路径引用，不依赖该项，属 API 一致性补全）。

## 4. 逐 C-ID 自查

### C01 登记已满时零派发 — 通过（普通+对抗）

- 前提：live_cap=1；第一受控进程（/bin/sleep 300）经真实工具链占用唯一名额；第二命令为可写哨兵+计数 helper（`park` 形）。
- 动作：经真实网关（prepare → execute_prepared）→ 真实 `run_exec_command` → 真实 supervisor 发起第二个 one-shot。
- 预期/实际：明确容量拒绝——`EXEC_PROCESS_REGISTRY_FULL` + "registry is full"，且**不是** `EXEC_SPAWN_FAILED`、**不是**网关 prepared 词汇（三重否定断言）；1s 观察窗内哨兵 sha256 全程不变 + 内容仍为 BASELINE；**OS 派发计数 0**（对抗要求：不只断言 Err）；fd 表数目不变；spill 目录零新文件；真实注册表仍恰为第一进程且存活；无关哨兵存活；末尾经真实有界链终止第一进程且无孤儿。
- 证据：`logs/e02_per_cid_c01.txt` exit 0。

### C02 并发争最后名额 — 通过（普通+对抗）

- 前提：live_cap=2，一个名额被占用，剩余 1；两个竞争者经 `tokio::sync::Barrier` 同时到达（固定交错，非随机重跑）。
- 动作/变体：one-shot×one-shot、PTY×PTY、one-shot×PTY 混合，共 3 条腿。
- 预期/实际：每腿恰一个 Success、恰一个 `EXEC_PROCESS_REGISTRY_FULL` 拒绝；**真实 OS 派发计数恰 +1**（胜者的回执，有界等待后断言；败者零派发）；胜者为一-shot 时 Exited 0、为 PTY 时真实 write_stdin 轮询 "status: running"（合法对照：PTY 正常工作）；轮换无泄漏——胜者回收后连续 3 轮 refill（PTY）均可启动并正常关闭，live_handles 从不超 cap；无胜者孤儿。
- 对抗补充：全套件并行 4 次 + 顺序 1 次（时间与交错自然变化下仍全绿）；屏障交错为构造性固定，不靠随机重跑挑绿。
- 证据：`logs/e02_per_cid_c02_{oneshot,pty,mixed}.txt`、`logs/e02_fullfile_*.txt`（exit 0）。

### C03 启动后异常补偿 — 通过（普通+对抗）

- 前提：可控验证注入钩子（仅测试可 arm）在责任链边界制造失败；第一进程持续占用名额。
- 动作/变体：
  - **组身份验证失败（派发后）· one-shot**：3 轮注入。每轮错误原文含 `EXEC_SPAWN_FAILED` + "process-group ownership" + "dispatch DID happen"（派发事实保留）；从错误解析出的 pid **真实不存活**（kill+reap 在错误返回前完成，非仅返回 Err）；注册表仍只含第一进程；fd 数前后相等（回滚管道全关）。
  - **组身份验证失败（派发后）· PTY**：master/slave fd 由回滚显式关闭（fd 数前后相等），子进程回收、派发事实保留。
  - **PTY 初始化失败（派发前）**：PtyPairOpen 注入——零派发（错误明说 "zero dispatch"）、零 fd 残留、live_cap=1 下名额恰好归还一次（紧接的合法 one-shot 对照 Exit 0）。
- 恰好一次的容量算术证明（对抗：二次失败注入/重复补偿）：注入 3 轮后，live_cap=2 下仍"恰可再入一个、再下一个被拒"——零释放会拒绝 refill，双释放会放行 over，均与实测不符。
- 证据：`logs/e02_per_cid_c03_{oneshot_groupverify,pty_groupverify,pty_open}.txt` exit 0。

### C04 失败后的同进程再使用 — 通过（普通+对抗）

- 前提：不重启服务（同一 supervisor + 网关），live_cap=1，连续 4 轮（> live_cap）occupy → refuse → release。
- 预期/实际：每轮拒绝为 `EXEC_PROCESS_REGISTRY_FULL` 且全轮 **OS 派发计数恒 0**、哨兵摘要不变（零执行的持久证明）；每轮释放后名额立即可用（下一轮 occupy 成功）；**重复取消**：每轮对已结算记录连发 3 次 terminate 均为 AlreadyTerminal 无害空操作（无杂散 killpg、无 live_count 误减，由下一轮成功 occupy 反证）；合法对照：4 轮后普通 one-shot（echo，Exit 0）与 PTY（启动、write_stdin 轮询 running、正常关闭）仍可用；**迟到回调**：对已终止 PTY 句柄迟到 write_stdin 轮询如实报告 terminated、迟到 terminate 为 AlreadyTerminal，且之后仍能再跑一轮完整 occupy/release；无关哨兵全程存活；无孤儿（逐 pid 检验）。
- 证据：`logs/e02_per_cid_c04.txt` exit 0。

## 5. 源码变化与影响范围

- `rust/crates/lingxi-service/src/procsupervisor.rs`（E01，+332 行）：模块文档新增 admission-before-dispatch 节；`register` 拆分为 `reserve_live_slot` + `LiveSlot::commit`；`LiveSlot` RAII guard（armed/Drop/commit）；`SpawnFaultPoint` 验证注入枚举 + `arm_spawn_fault_for_verification`/`take_spawn_fault`；`note_dispatched_rollback`；两条 spawn 路径的派发后回滚（kill+reap+显式关 master+派发事实）；3 个新单元测试（live slot 记账）。
- `rust/crates/lingxi-service/src/exectools.rs`（E01，+19 行）：`EXEC_PROCESS_REGISTRY_FULL` 常量（注释申明与网关 prepared cap 的区分）；`run_exec_command` 对 `SpawnFailure::RegistryFull` 的专属映射（BudgetExceeded）——该 spawn 调用点同时覆盖 one-shot 与 tty 路径，两类的拒绝词汇一致。
- `rust/crates/lingxi-service/src/lib.rs`（E02）：re-export 补 `EXEC_PROCESS_REGISTRY_FULL`。
- 新测试 `rust/crates/lingxi-service/tests/r04_t05_registry_capacity.rs`（1267 行，9 用例 + 真实链 harness）。
- 行为影响面：登记满时从"已派发后失败"变为"零派发拒绝"，错误码/文案区分；其余 spawn 语义（setsid、reaper、终止链、PTY 寿命）不变。未改任何旧测试；旧断言 `EXEC_SPAWN_FAILED`（缺失二进制/坏 cwd 的 Io 失败）不受影响（workspace 全量绿含该套件）。

## 6. 同族路径检查

- `ProcessSupervisor::spawn` 的全部消费者：仅 `exectools.rs run_exec_command`（one-shot 与 PTY 共用）——RegistryFull 映射单点覆盖两路。
- 其他 spawn 点不经过 live 注册表、无"先派发后查容量"同型缺陷：`workerrpc.rs`（worker 子进程，自有 WorkerChild 有界回收与并发上限）、`mcpbridge.rs`（stdio transport 子进程，自有生命周期）、`lingxi-browser-spike`（spike）、`xtask/verify.rs`（构建期验证）、queue.rs/ws.rs（线程/任务 spawn，非 OS 进程）。
- settle/terminate/shutdown_all/Drop 与新记账的交互：`settle` 恰好一次已由 `settled` 标志保证；`shutdown_all` 只遍历有 record 的进程（预留未 commit 的名额无 record，由 guard 自还）；supervisor Drop 的同步 kill 兜底不变。
- 旧 R04-A09/A10 及对抗套件（未改动）在全量 no-fail-fast 运行中 14/14 通过。

## 7. 门禁命令与退出码（全部真实执行，证据在 logs/）

| # | 命令 | 退出码 | 证据 |
|---|------|--------|------|
| 1 | `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 首次 1（漂移仅在新测试文件）→ 应用 `cargo fmt --all` 后复验 **0** | `e02_gate_fmt.txt` |
| 2 | `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | **0** | `e02_gate_clippy.txt` |
| 3 | `cargo test --manifest-path rust/Cargo.toml --workspace --locked` | 首跑 **101**（A09 空 pid 文件竞态，见 §7.1，孤立复跑通过）；随后 as-written 复跑 **0**（83 个测试二进制全绿，含 A09/A10）；`--no-fail-fast` 全量 **0** 亦存证 | `e02_gate_workspace_tests{,_plain,_nofailfast}.txt` |
| 4 | `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-contracts` | **0** | `e02_gate_check_contracts.txt` |
| 5 | `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries` | **0** | `e02_gate_check_boundaries.txt` |
| 6 | 逐 C-ID 单独运行（repro + C01 + C02×3 + C03×3 + C04 + live_slot 单元） | 各 **0** | `e02_per_cid_*.txt`、`e02_unit_live_slot_admission.txt` |

cargo 一律 `~/.cargo/bin/cargo`（rustup 锁定 1.98.1），带 `--manifest-path rust/Cargo.toml --locked`；Cargo.lock 未改动。

### 7.1 门禁 3 的既有环境项（非本修复引入，已归因）

`r04_t05_process_tools`（E01 与 E02 均未触碰的既有套件）在本机存在两条时敏腿：

- **A09 空 pid 文件**：`wait_until(pid_file.exists())` 可落在 shell 重定向建文件与 printf 写入之间（读得 0 pid）。全量并行负载下出现过 1 次；孤立运行恒绿（0.09s）。基线代码固有竞态，与本修复无关。
- **A10 PTY ^C 后 exit 观察不到**：在**干净基线 773d5a6 worktree**（无任何 G01 改动）上同样确定性复现（孤立 3/3 红，修复树上孤立 5/5 红）；E02 用最小 supervisor 级探针定位：spawn+`exit 7` 直通路径在基线**通过**（0.56s，Exited{7}），复现需要 "^C 打断前台作业"序列——打断后 bash 处于 `Ss` 存活但不消费后续写入（本 macOS 构建的 readline 在 SIGINT 后丢弃 type-ahead 的时序效应；机器高负载时反而通过，故两次全量工作区运行均绿、A09/A10 均 ok）。该腿属 PTY 打断/观察语义 = **F02/G02 工作单范围**，G01 不越界修复，仅完整归因存证：`e02_baseline_773d5a6_*.txt`、`e02_baseline_773d5a6_supervisor_probe_intr_exit.txt`。
- 已知环境项（r00_management_leaves LAN 自连腿防火墙拦截）本轮未出现（no-fail-fast 全量绿）。

## 8. 未验证项（如实）

- **独立审查（三层检查第三层）**：未执行——属 Reviewer 职责，本代理最多 READY_FOR_REVIEW。
- **verify-stage R04**：未运行——按总控分工属 G05（新增场景接入正式 Gate 生产者时统一执行）；本报告门禁按工作单给定五条+逐 C-ID 执行。
- **非 Unix 平台**：`SpawnFailure::UnsupportedPlatform` 路径为 cfg(not(unix)) 编译分支，本机（macOS arm64）未机器验证；Windows 维持阶段登记的平台递延。
- **调用方在回滚 `child.wait().await` 处丢弃 Future 的极端边界**：kill 已同步发出、名额由 guard 归还，子进程回收依赖 tokio orphan queue（§2.3）；动态构造该丢弃点的专项测试未做，归入 F02 观察链家族一并处理更合适。
- **A10 基线失败腿**：非 G01 范围（§7.1），留待 G02；本机空载孤立运行时该腿时敏失败，全量（as-written 与 no-fail-fast 两形式）工作区运行均绿。

## 9. 现场与清理

- 基线归因 worktree `/tmp/r04_g01_baseline`（含一次性探针 `tests/zz_probe_a10.rs`）已完成取证使命，报告落盘后移除（`git worktree remove --force`），不残留于仓库。
- 总控账本（`docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`、`docs/rust-tauri/R04/repair-current/R04_RR1_FIX_ISSUES.json`）未由本代理修改（前者的工作区差异为总控在 G01 派单前的既有改动）。
- 无 commit/push；未启动 R05；未 pkill 系统进程；测试目录均为唯一命名隔离目录并随 teardown 清理。
