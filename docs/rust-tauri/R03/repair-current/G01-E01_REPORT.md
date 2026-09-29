# R03 修复轮 G01-E01 执行报告（F01＋F02）

- 执行代理：EXECUTOR-REPAIR-R03-G01-E01（一次性执行/修复代理；本报告为执行者口径，不含独立审查）。
- 日期：2026-09-30。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。
- 基线 `FIX_BASE_SHA=cd3fb19e651f763afc6c75cb3163064fb54ca3fe`；本轮无 commit/push（未获授权），候选 = 该提交 + 本报告所列未提交修复工作树。总控本轮产物（`R03_FIX_ISSUES.json`、`R03_REPORT.md`/`R03_HANDOFF.json` 的 REOPENED 标注）未改动。
- 工具链：`~/.cargo/bin/cargo`（rustup 锁定 1.98.1），全部 `--locked`；`rust/Cargo.lock` sha1 `3b659f41…` 与 HEAD 相同（零依赖变化）。
- 证据根：`artifacts/rust-tauri/R03/repair-current/G01-E01/`（`normal-selfcheck/`、`adversarial-selfcheck/`、`logs/`、`commands.json`）。
- 结论：**READY_FOR_REVIEW**（workspace 65 suites / 648 passed / 0 failed ≥ 基线 63/626/0；fmt、clippy `-D warnings` 零输出零告警；check-contracts / check-boundaries 额外回归 exit 0）。

## 1. 实现范围

F01（取消树链接与继承）＋F02（父取消后子运行收尾/线程/配额泄漏，含监督注册表同族路径），及其同根因路径。不涉及 F03–F07，不进入 R04。

## 2. 根因与同族路径清单（复核结论：审查属实，无反证）

### F01 根因
`cancel.rs` 的三种父子构造中只有 `CancelScope::child` 做父侧登记；`run_root_under` 只写子侧 `parent` 指针，不进父 `children` → `cancel_recursive` 永远到不了 linked run root。`is_cancelled` 只读本节点 AtomicBool，不沿父链查。`child`/`run_root_under` 在父已取消时仍从 `cancelled=false` 开始（无继承）。消费链真实存在：`CancelRegistry::register_linked`（`runs.rs` 的 `drive_run` parent_scope 分支）与 `subagents.rs` 的子代理超时（`drive_scope.cancel("subagent timeout")` 后 `drive.await`）。

同族路径：
- `run_root_under`（直连消费：`register_linked`）；
- `child`（消费：`drive_run` 的 model/tool/approval call scope、`spawn_child` 的 child_run scope）；
- 注册与取消遍历的竞态窗口（`cancel_recursive` 的 children 锁边界 vs 注册 push）；
- 注册相位：linked 注册在父已取消时仍以 `Active` 起步（相位与作用域事实不一致）。

### F02 根因
`subagents.rs::spawn_child` 在步骤 1 预留 `active_per_session`/`active_global` 并置 `thread.busy=true`，清理只在子 future 正常走完的 `note_child_finished`。`task_supervisor.rs::spawn_linked` 的 biased select 在树取消时**直接丢弃整个 child future**（含子 drive 的四相取消/收尾/收尾簿记），只记 `TaskExit::Aborted` → 普通父取消后：子 durable 行保持 active、busy 永不清、配额不归还；重复取消耗尽配额。`RegistrationGuard::drop` 只负责内存树取消+Abandoned 相位，durable 行留启动扫描（旧 T08 FINDING-1 即此）。

同族路径：
- `drain_run` 超时分支：JoinHandle 被移入 timeout future 后丢弃，abort 只是"请求"，条目永为 Running 无句柄（幽灵项）；
- `spawn_detached`（后台驱动）：wrapper 直接 `fut.await`，子 panic 穿透 wrapper，exit 不被记录，无人 wait 时 panic 诊断丢失，TaskSupervisor 条目永 Running → 容量被已死任务耗尽；
- `BackgroundDriveRegistry::live_ids`/容量检查：删除已完成条目时不消费其退出（第二张表泄漏）；
- `note_child_finished` 忽略 `child_run_id`（`let _ = child_run_id;`）：迟到旧完成回调可清新运行的 busy，重复回调可双减计数。

## 3. 修复设计（最小完整）

### F01（cancel.rs）
1. **统一构造入口 + 成对链接**：`child()` 与 `run_root_under()` 共用 `new_child_scope` → `link_under(parent, child)`——在父 `children` 锁内 push 子 Weak **并**在锁内复查父取消标志；父已取消则当场继承（复用父的首次 reason 与 cancelled_at）。`cancel_recursive` 的首写段改为"先填 reason/cancelled_at 并释放锁，再置标志"——观察到标志为 true 的读者必然读到完整事实（这也使 link_under 在持 children 锁时读父 reason 无 ABBA：写者从不持 reason 等待 children）。**无漏项协议**：注册与取消遍历以父 children 锁为线性化点——要么注册先入表（遍历可达），要么遍历已过/在途（锁内复查见标志 → 继承）。
2. **is_cancelled 沿祖先链查**（单调：取消不会撤销）。
3. **注册相位**：`register_scope` 在作用域已取消（继承）时以 `Requested{继承的首次 reason}` 起步，绝不出现 Active-under-cancelled-scope。
4. 语义保持：只向下传播（`a_child_scope_never_cancels_its_parent` 既有测试仍绿）、无关树不受影响、首次 reason/时刻不被覆盖（`cancel_recursive` 首写段以 reason 锁裁决首写者）。

### F02
1. **两种树取消形态**（task_supervisor.rs）：调用级子（model/tool/approval）维持"标志即丢弃"；**运行级子（`TaskKind::ChildRun`）改为有界协作窗口**——树取消后子 future 继续被 poll（走完自己的 durable cancelling→drain→单次 finalize→收尾簿记），窗口以作用域首次取消时刻 + 清理 grace（`RunSupervisor` 把 `CancelPolicy.cleanup_grace_ms` 传入 `TaskSupervisor::with_cooperative_grace`，与父 drain 同一锚点）为界；超窗才最后手段丢弃（Aborted，如实告警）。
2. **panic 收容**（PanicGuard）：wrapper 对每次 poll 做 catch_unwind，exit（含 `Panicked(payload)`）**就地记录**——无人 wait 也可查询、可诊断，fire-and-forget 不再丢 panic。linked 与 detached 两路统一。
3. **超时回收观察者**（drain_run）：JoinHandle 留在 drain 帧内（`&mut join`），超时分支 abort 后交给分离观察者 task 持有 join 并在实际退出时记录 exit——"请求中止"≠"观察退出"，条目不再永为 Running 无句柄；Exited 条目在注册压力时回收（容量可复用）。
4. **子运行必达收尾守卫**（subagents.rs `ChildCloseout`）：在 `spawn_child` 预留之后、spawn 之前创建（覆盖 future 未首次 poll 窗口），移入子 future 由正常收尾 `complete(finish)` 解除；其余一切结束（最后手段丢弃、panic、未 poll）经 `Drop` 走异常收尾（计数归还、busy 清、诚实 failed 状态）。以 `AtomicBool` done 标志保证恰好一次；派发拒绝窗口（NotBound/SpawnRefused）显式 rollback + disarm（保持既有"拒绝不留线程痕迹"语义）。
5. **note_child_finished 双层防误清**：(a) 有界 `accounted_children` 环（1024）使重复/迟到回调整体 no-op（恰好一次计数）；(b) 身份栅栏——仅当 `thread.child_run_id == child_run_id` 才清 busy/记状态，被超越的旧 run 回调只还计数不动新运行（环被淘汰后的第二层防御）。
6. **双注册表回收**（background.rs）：`live_ids` 与容量检查改为 `reclaim_finished`——每个已完成条目由分离 waiter `handle.wait()` 消费退出（记录+回收 TaskSupervisor 条目），观测到的 TaskExit（含 Panicked）进入有界 recent-exits 环可查询。两张表都不泄漏。

### 既有测试语义更新（如实登记）
- `r03_t08_acceptance_matrix.rs::leaf_parent_cancel_stops_child`：按 F02 修复后的正确语义改写——子 durable 行同进程落 `cancelled`、busy 清、配额归零；重启段改为"恢复扫描尊重已存在终态"（原用例把"普通取消需重启收口"钉进了预期，恰是总控清单 F02 的禁止项；其"真实崩溃恢复"语义由 recovery_startup_scan / recovery_crash_points 等套件继续覆盖，未删除）。
- `cancellation_tree.rs::r03_a06`：演示性 child_run 由 `pending()` 改为"观察树取消后自行收尾"（协作窗口形态——真实子代理 drive 今日的形态），断言从 `Aborted` 改为经自身尾部完成；监督/无关后台断言全部保留。

## 4. 改动文件

| 文件 | 改动 |
|---|---|
| `rust/crates/lingxi-service/src/cancel.rs` | link_under 无漏项协议、首写段重排、is_cancelled 沿父查、register_scope 继承相位、单元测试 |
| `rust/crates/lingxi-service/src/task_supervisor.rs` | 协作窗口（ChildRun）、PanicGuard panic 收容、drain 超时观察者、`with_cooperative_grace`、测试 |
| `rust/crates/lingxi-service/src/subagents.rs` | ChildCloseout 必达守卫、note_child_finished 恰好一次 + 身份栅栏 + accounted 环（该方法改为 pub 供审计/测试注入） |
| `rust/crates/lingxi-service/src/runs.rs` | TaskSupervisor 接入 cleanup grace（两处构造） |
| `rust/crates/lingxi-service/src/background.rs` | reclaim_finished 双表回收、recent_exits 诊断环、容量检查复用回收 |
| `rust/crates/lingxi-service/tests/cancel_link_inheritance.rs` | 新增（F01 C01–C05，7 测试） |
| `rust/crates/lingxi-service/tests/subagent_closeout.rs` | 新增（F02 C01–C05 + 迟到/重复回调栅栏，8 测试） |
| `rust/crates/lingxi-service/tests/cancellation_tree.rs` | a06 演示 child 改协作收尾形态；移除失效 import |
| `rust/crates/lingxi-service/tests/r03_t08_acceptance_matrix.rs` | 父取消用例改同进程收口语义 |

未改动：`docs/rust-tauri/R03/repair-current/R03_FIX_ISSUES.json`、`R03_REPORT.md`/`R03_HANDOFF.json`（总控标注）、`rust/Cargo.lock`、任何存储迁移/校验值、R02 资产。

## 5. 测试清单（新增 22 项）

- `cancel_link_inheritance.rs`：linked_run_roots…mixed_entries（C01）、nodes_created_after…inherit（C02）、registration_racing…misses_no_node（C03）、subagent_timeout ×3（provider/approval/quota 等待，C04）、isolation_and_first_reason…（C05）。
- `subagent_closeout.rs`：parent_cancel_closes_children_in_process_repeatedly_beyond_the_cap（C01，per_session_limit=2 下 3 轮取消+1 轮正常，无重启/无 startup_scan）、thread_registry_refusal…（C02 登记窗口）、parent_cancel_before_the_childs_first_provider_call…zero_calls（C02/F01-C02 服务面：model_call_started=0、工具 0 次）、child_provider_panic…thread_continues（C03）、drain_expired_child_is_finally_reaped…（C04）、panicking_background_exits_are_recorded…（C05）、finished_background_drives_reap_both_registries（C05 双表）、late_and_duplicate_completions_cannot_corrupt_the_closeout（C03 对抗：迟到旧回调/重复回调）。
- 源内单元：`cancel.rs` 4 项新测试；`task_supervisor.rs` 3 项新测试（协作窗口内自收尾、无视窗口超窗丢弃 Drop 探针、首 poll 前取消仍记录）。

## 6. 验证

- 红绿：先对未修复代码跑新增反例（RED：11 失败/3 通过，日志在 `adversarial-selfcheck/red-baseline-*.log`），修复后全绿（见 `normal-selfcheck/`）。
- workspace：`cargo test --workspace --locked` = **65 suites / 648 passed / 0 failed**（基线 63/626/0，+2 suites +22 tests，无删除无跳过）。
- `cargo fmt --all -- --check` 零 diff；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警。
- 额外：`xtask check-contracts`（626 entries 零漂移）、`check-boundaries` exit 0。
- 逐 C-ID 两层自查：见 `G01-E01_NORMAL_SELFCHECK.md`、`G01-E01_ADVERSARIAL_SELFCHECK.md`。

## 7. 边界与如实声明

- 无真实供应商/无网络外发/隔离 /tmp 合成数据根；Provider/Tool/ApprovalGate 替身仅产生外部响应或受控 panic（外部副作用面），被测的取消裁决、监督终态、受理与真实存储链未被 mock。
- `note_child_finished` 改为 pub 是为迟到/重复回调的对抗性注入（审计/测试面）；其行为仍由真实运行时裁决。
- F02-C02 的"分配 run id 失败"窗口无法在进程内注入真实存储故障（无故障注入缝）；该窗口的 rollback 与 NotBound/SpawnRefused 共用同一显式回滚代码路径（SpawnRefused 已实测），静态复核登记。
- C04 协作窗口与父 drain 同锚点，理论上存在微秒级边界竞态（子 teardown 恰好耗满 grace 才会触发）；健康 SQLite 下子收尾为毫秒级，测试与默认 5s grace 均远离边界。该边界行为是"超窗最后手段丢弃 + StopUnconfirmed 如实报告"，不是泄漏。
- 未执行 `verify-stage R03`（阶段图重登记属 G07）；A15 矩阵测试本体在 workspace 内全绿。
- 本轮无 BLOCKED 项；对审查清单无反证（复核确认 F01/F02 全部 source_facts 属实）。
