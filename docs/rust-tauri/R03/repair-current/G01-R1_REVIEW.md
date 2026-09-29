# R03 修复轮 G01-R1 独立对抗性审查（F01＋F02）

- 审查代理：REVIEWER-REPAIR-R03-G01-R1（一次性独立对抗性 Reviewer，未参与 G01 候选的实现或修复）。
- 日期：2026-09-30。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。
- 候选：HEAD `cd3fb19e651f763afc6c75cb3163064fb54ca3fe` + 未提交修复工作树（产品 5 文件：cancel.rs / task_supervisor.rs / subagents.rs / runs.rs / background.rs；测试 4 文件：cancellation_tree.rs、r03_t08_acceptance_matrix.rs 修改 + cancel_link_inheritance.rs、subagent_closeout.rs 新增）。审查期间候选冻结，未改动。
- 工具链：一律 `~/.cargo/bin/cargo`（cargo 1.98.1），全部 `--locked`；`rust/Cargo.lock` sha1 `3b659f41eb262eaf42efc2f91ec93f9989d42934` 实测与执行者声明一致（零依赖变化）。
- 本审查复测产物根：`artifacts/rust-tauri/R03/repair-current/G01-R1/`。

## VERDICT: PASS

## 1. 候选摘要（实测 diff --stat）

```
 rust/crates/lingxi-service/src/background.rs       | 105 ++++-
 rust/crates/lingxi-service/src/cancel.rs           | 241 ++++++++++--
 rust/crates/lingxi-service/src/runs.rs             |  13 +-
 rust/crates/lingxi-service/src/subagents.rs        | 173 ++++++++-
 rust/crates/lingxi-service/src/task_supervisor.rs  | 423 ++++++++++++++++++---
 rust/crates/lingxi-service/tests/cancellation_tree.rs      |  21 +-
 rust/crates/lingxi-service/tests/r03_t08_acceptance_matrix.rs |  66 ++--
 9 files changed, 912 insertions(+), 140 deletions(-)   （另有 2 个新增测试文件，未跟踪）
```

修复设计与实际 diff 一致：F01＝`cancel.rs` 统一构造入口 `new_child_scope`→`link_under`（父 children 锁为线性化点的无漏项协议＋锁内继承首因/首时刻）、`is_cancelled` 沿祖先链、`register_scope` 对已取消作用域以 `Requested{继承 reason}` 起步；F02＝ChildRun 协作取消窗口（锚定作用域首次取消时刻＋cleanup grace）、PanicGuard 逐 poll 收容 panic 并就地记录 exit、drain 超时分离观察者（请求中止≠观察退出）、`ChildCloseout` 必达收尾守卫（done 标志恰好一次＋Drop 异常收尾）、`note_child_finished` accounted 环（1024）＋身份栅栏、`BackgroundDriveRegistry::reclaim_finished` 双表回收＋recent_exits 环（256）。

## 2. 逐 C-ID 独立核查结果

独立复测命令均为真实执行并落盘于 G01-R1/；退出码为实测。

| C-ID | 正常自查核对 | 对抗性变体核对 | 独立复测命令与退出码 | 证据路径 |
|---|---|---|---|---|
| F01-C01 | 混用 run_root_under/register_linked/child 三入口建三级树，父取消达全部节点、waiter 250ms 有界醒、中间深度取消只向下不上行——与源码 `link_under` 协议一致（父 children 锁双侧线性化、锁内复查标志、继承首因/首时刻；锁序 parent.children→child.* 无 ABBA） | 测试本体即对抗变体（三入口混合＋两种取消深度） | `~/.cargo/bin/cargo test --manifest-path rust/Cargo.toml -p lingxi-service --test cancel_link_inheritance --locked` → **exit 0，7/0** | G01-R1/indep-cancel_link_inheritance.log；红：red-verify-cancel_link_inheritance.log |
| F01-C02 | 单元：三入口在父已取消后创建均继承（标志/首因/父首时刻），register_linked 相位=Requested；服务面：子停在 model 受理队列时父取消→子 cancelled、model_call_started=0、工具 0 次、队列位归还、父泊车许可不受影响（不上行） | 先取消后创建 / 先注册后取消（泊车队列序）两序均覆盖 | 同上（套件含两项）→ **exit 0** | 同上；服务面在 indep-subagent_closeout.log |
| F01-C03 | Barrier 同步 8 注册者（各建孙节点，多级）与取消者同时开火；任意交错下全部归属节点取消、root 首因保留、事后加入节点继承 | 定向复跑：`cargo test … --test cancel_link_inheritance registration_racing -- --nocapture --test-threads=1` → **exit 0**（1 passed） | 命令见左 → **exit 0** | G01-R1/indep-F01-C03-race.log；红：red-verify…log（:583 FAILED） |
| F01-C04 | provider 泊流 / 审批 pending / 配额队列三变体，门与许可均不释放，唯一出口是子自身 timeout 的作用域取消经链接树抵达内层 drive：子 durable=cancelled、线程释放、counts (0,0)、审批 pending 清零＋工具 0 次、(c) 队列位归还且父仍持许可、子 provider 0 次 | 三变体即对抗面（分别停泊三类等待） | 套件内三测试 → **exit 0** | G01-R1/indep-cancel_link_inheritance.log；红：三变体在旧码 :394 FAILED |
| F01-C05 | 两无关根＋重复 3 因＋并发 3 因（barrier）＋事后覆盖尝试：根 B 全程 Active、子取消不上行、首因线性化后冻结。旧码本项即正确（红跑中 ok）——执行者如实登记为"不回归钉住"，非虚报新修复 | 并发不同 reason 线性化（barrier）在测试内 | 套件内 → **exit 0** | 同上（红跑中本项 ok，与执行者声明一致） |
| F02-C01 | per_session_limit=2 下 3 轮"派发泊车子→真实 cancel_run_for 取消父"（超过上限次数），每轮同时断言四收口：durable=cancelled、busy 清＋last_run_status=cancelled、active_counts (0,0)、live_children 空；第 4 轮正常子双 completed。**红线专项**：测试结构性无重启（每测试函数仅一次 bootstrap_with_deps）、无 startup_scan/RecoveryCoordinator 引用（grep 实证）、取消经真实会话面 `cancel_run_for` | 超上限重复（3>2）＋最终正常派发即对抗面 | `cargo test … -p lingxi-service --test subagent_closeout --locked` → **exit 0，8/0** | G01-R1/indep-subagent_closeout.log；红：red-verify-subagent_closeout-C01.log（旧码 exit 101，"the child settles cancelled in-process" 永不达成） |
| F02-C02 | (a) thread_registry_cap=1 拒绝：拒绝会话零线程/零计数/lineage 仅 1 条、结构化 tool failure 到模型、首线程不受扰；(b) 派发成功但子停受理队列时父取消（0 次外部调用）；(c) 监督级首 poll 前取消仍记录 Aborted（lib 单测，workspace 覆盖）。run-id 分配存储故障窗口无进程内注缝——执行者如实披露；源码复核：`allocate_run_id` 失败→rollback_caps，此时线程记录尚未创建（步骤 3 在步骤 2 之后），零线程残留，回滚路径与 NotBound/SpawnRefused 同一代码路径 | 预留/登记线程/派发/未首 poll 各窗口 | 套件 → **exit 0**；红：thread_registry_refusal 旧码即 ok（钉住，如实登记），(b) 旧码 FAILED | G01-R1/indep-subagent_closeout.log |
| F02-C03 | 子 provider 受控 panic→failed 终态、线程释放、(0,0)、同线程 reply 续跑 completed；迟到被超越回调不清新运行 busy、重复回调 no-op（accounted 环＋身份栅栏），随后真实完成照常收口 | 定向注入（pub `note_child_finished`，注入即真实簿记代码）：`cargo test … --test subagent_closeout late_and_duplicate_completions -- --nocapture` → **exit 0**（1 passed） | 套件＋定向 → **exit 0** | G01-R1/indep-F02-C03-fence.log；旧码迟到回调无栅栏静态属实（旧 `let _ = child_run_id;`，本次 diff 中移除） |
| F02-C04 | 子 250ms 有限让出＞drain 预算 30ms：先报 unconfirmed（exit=None，请求≠观察），实际退出后 exit_of=Aborted（分离观察者记录），同 cap 再 spawn 成功且正常完成。无无限忙等（受控有限窗口） | 观察者分离设计源码复核：timeout 轮询 `&mut join`，超时分支 abort 后 join 移交分离 observer，退出落地即记录，压力回收 | 套件内 → **exit 0** | G01-R1/indep-subagent_closeout.log；红：旧码 :324 FAILED（Running 无句柄幽灵） |
| F02-C05 | 3 次真实 panic 全部 `Panicked(payload)` 就地可查询（无 waiter）、哨兵存活并完成、压力回收后新 spawn 成功、无 Running 幽灵；服务面 3 轮后台完成→live_ids 收缩且 `background_drive:{id}` 监督条目消失（双表同收）；无关任务隔离由 a06 套件维持（8/0 独立复跑 exit 0） | 重复 3 次＋双注册表对照＋哨兵 | 套件内两项 → **exit 0** | G01-R1/indep-subagent_closeout.log、indep-cancellation_tree.log；红：旧码 :976 SpawnRejected{cap:3}＋:324 |

生产入口接线独立核验：`drive_run` parent_scope 分支→`register_linked`→`run_root_under`→`link_under`（真实消费链）；`spawn_child`→`spawn_linked`(ChildRun→协作窗口)＋`ChildCloseout`；`spawn_background_drive`→`spawn_detached`(PanicGuard)＋`reclaim_finished`——均与 diff/源码一致，非仅辅助方法自测。

## 3. F02 红线专项结论

普通父取消在数据库健康、服务不重启、同进程内四者同时收口：**满足**。证据：F02-C01 测试每轮同时断言 DB 终态（cancelled）＋thread.busy=false＋active_counts (0,0)＋TaskSupervisor live_children 空；结构性禁止重启（每测试仅一次 bootstrap，grep 无 startup_scan/RecoveryCoordinator 辅助）；我的旧代码 worktree 复跑证明修复前该反例真实失败（泄漏存在）。真正进程崩溃恢复链保留且绿：`recovery_startup_scan` 7/0、`recovery_crash_points` 2/0（均独立复跑 exit 0，日志在 G01-R1/）。

## 4. 测试语义修改审查（r03_t08_acceptance_matrix / cancellation_tree）

**结论：更正旧错误预期，未削弱 A06/A15 保护。**

- `leaf_parent_cancel_stops_child`：旧预期"普通取消后子 durable 行留 active，由重启 startup scan 收口为 interrupted_needs_attention"正是总控清单 F02/F08 明文禁止的预期（"把普通取消必须重启写进新的正确测试预期"）。改写后：同进程 durable=cancelled＋busy 清＋lanes (0,0)（全部为**增强**断言）；监督断言（live_children empty）、无 final message 断言、父 cancelled 断言全部保留；重启段保留但改为断言"恢复扫描尊重已存在终态"（终态不复活）。真实崩溃恢复语义未被删除——由 recovery_startup_scan / recovery_crash_points 继续覆盖（独立复跑绿）。
- `cancellation_tree.rs::r03_a06`：演示 child_run 由 `pending()`（断言被强制丢弃为 Aborted）改为观察树取消后自行收尾——与 F02 修复要求的"先协作收尾、后最后手段"一致；无关后台存活、监督确认、父取消断言全部保留。"最后手段丢弃"本身仍被新 lib 单测 `run_level_child_ignoring_the_window_is_dropped_at_expiry`（Drop 探针）钉住，`noncooperating_child_is_reported_unconfirmed_not_fake_quiet` 等既有不合作报告语义在套件内仍绿（独立复跑 8/0）。

## 5. 修复未制造的负面效应核查（逐项）

- 双重 finalize：runs.rs 的 drive/finalize 逻辑未改（仅接入 grace）；子在窗口内走自身单次 finalize，超窗丢弃则不 finalize（durable 行如实保持非终态交恢复分类）。未发现双重。
- 重复计数释放：`ChildCloseout` done 标志（恰好一次）＋`accounted_children` 环＋拒绝路径显式 disarm；对抗注入实测无双减。
- 跨 child_run_id 误清 busy：身份栅栏（`thread.child_run_id == child_run_id` 才清）；被超越回调注入实测不清新运行。
- 幽灵 Running/无句柄条目：drain 观察者＋PanicGuard 就地记录＋压力回收＋双表 reclaim；C04/C05 断言无 exit=None 残留。
- 取消先赢后仍 completed：durable 终态裁决仍属 drive 自身 fence（F03/G02 范围，本轮未动）；wrapper 层面取消与完成同时就绪时 biased 先看取消、窗口内已完成的子按其真实返回交付——与"先让 child drive 走合法收尾"要求一致，非倒退。
- 无关树连带取消：只向下传播（`a_child_scope_never_cancels_its_parent` 等既有测试绿）；C04 配额变体实证子超时不释放父的许可。
- 首次取消原因被覆盖：首写段 reason 锁裁决＋先填后置标志的顺序；C05 并发三因线性化实测冻结。
- `note_child_finished` 改 pub：生产调用者未增加（grep 全仓：仅 `ChildCloseout::complete`/`Drop` 与两个测试注入点）；该对象不经 HTTP/IPC 暴露，属内部运行时的审计/注入面扩大，风险低、理由成立（见 §7 OBS-2）。

## 6. 红基线真实性（独立 worktree 复现）

方法：`git worktree add /tmp/r03-g01-redcheck cd3fb19e6`（隔离副本），拷入**当前**新测试文件复跑，用后 `git worktree remove --force` 清理，未触碰当前工作树。

1. `cancel_link_inheritance.rs` 原样拷入旧代码：`cargo test -p lingxi-service --test cancel_link_inheritance --locked` → **exit 101，1 passed / 6 FAILED**（C01/C02/C03/C04×3 红，C05 ok）——与执行者 `red-baseline-cancel_link_inheritance.log` 的失败集合逐项一致。日志：G01-R1/red-verify-cancel_link_inheritance.log。
2. `subagent_closeout.rs` 拷入旧代码，仅删除 `late_and_duplicate_completions_cannot_corrupt_the_closeout` 一项（该测试依赖修复后才 pub 的 `note_child_finished`，旧私有 API 下无法编译；其余 7 项字节不变——此机械裁剪为本审查唯一测试副本改动，仅存在于 /tmp 副本并已在日志中体现）：`cargo test … --test subagent_closeout parent_cancel_closes --locked` → **exit 101**，"the child settles cancelled in-process" 永不达成（wait_status 超时）——普通父取消泄漏在旧代码上真实存在。日志：G01-R1/red-verify-subagent_closeout-C01.log。
3. 执行者红日志 `red-baseline-subagent_closeout.log` 跑的是 7 项版本（2 passed/5 failed）——与上述 pub-API 依赖解释自洽，非隐瞒。

## 7. 门禁复跑（真实退出码）

| 命令 | 退出码 | 结果 |
|---|---|---|
| `~/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --workspace --locked` | **0** | 65 suites / 648 passed / 0 failed（=期望值；基线 63/626/0，+2 suites +22 tests，无删除无跳过） |
| `~/.cargo/bin/cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | **0** | 零 diff |
| `~/.cargo/bin/cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | **0** | 零告警 |

日志：G01-R1/gate-workspace-test.log、gate-fmt-check.log、gate-clippy-check.log。

## 8. finding 清单

无阻塞 finding。非阻塞观察（OBS，均不构成本轮 FAIL 依据）：

- **OBS-1**（已由执行者如实披露）：协作窗口与父 drain 同锚点，子收尾恰耗满 grace 才会触发超窗最后手段丢弃（durable 行如实非终态＋StopUnconfirmed＋恢复分类兜底），是协议允许的最后手段而非泄漏；健康 SQLite 下毫秒级收尾远离边界。
- **OBS-2**：`note_child_finished` 改 pub 扩大了 crate 内可审计面。生产无新调用者、不经外部协议暴露，本轮接受；后续治理轮可考虑 `#[doc(hidden)]` 或测试门控（非必需）。
- **OBS-3**：F02-C02 的"run id 分配存储故障"窗口无进程内注入缝，仅静态复核（回滚路径共享且线程记录晚于该窗口创建，零残留）。如实登记，不虚报已实测。

## 9. 误判反证

无。执行者报告、两层自查与我的独立复测全部一致，未发现需要反证的声明。

## 10. 需标 STALE 的旧证据

无。G01-E01 全部证据（normal/adversarial/logs/commands.json）与本审查独立复测一致，无需标 STALE。

## 11. 审查范围声明

本审查未修改任何产品源码、测试、配置、门禁、账本（R03_FIX_ISSUES.json）与 G01-E01 执行者证据；未 git commit/push；复测产物仅写入 `artifacts/rust-tauri/R03/repair-current/G01-R1/` 与本报告。红基线验证在 /tmp 隔离 worktree 完成，已清理（`git worktree list` 仅剩主工作区）。
