# G01-E01 普通逐项自查（第一层）

执行代理：EXECUTOR-REPAIR-R03-G01-E01。候选：`cd3fb19e6` + 未提交修复工作树。工具链 rustup 1.98.1，全部 `--locked`。
证据根：`artifacts/rust-tauri/R03/repair-current/G01-E01/`（normal-selfcheck/ 逐套件日志，logs/ 全量与标准命令）。
每项先列**修复前失败依据**（RED，同一工作树修复编辑之前运行，日志 `adversarial-selfcheck/red-baseline-*.log`），再列修复后真实回归。

| C-ID | 修复前依据（RED） | 修后命令与结果 | 退出码 | 证据 |
|---|---|---|---|---|
| F01-C01 | `linked_run_roots_receive_parent_cancellation_across_mixed_entries` FAILED（cancel_link_inheritance.rs:450 "cancelling the parent must reach child_entry"——run_root_under/register_linked 未登记父侧） | `cargo test -p lingxi-service --test cancel_link_inheritance` → 7/0 | 0 | normal-selfcheck/test-cancel_link_inheritance.log；red-baseline-cancel_link_inheritance.log |
| F01-C02 | `nodes_created_after_parent_cancellation_inherit_it` FAILED（:498 "a child of an already-cancelled parent starts cancelled"）＋服务面 `parent_cancel_before_the_childs_first_provider_call…` RED（旧码子运行完成而非取消、provider 计数>0） | 同上（单元 7/0 含继承相位=Requested）＋ `--test subagent_closeout` → 8/0（model_call_started=0、工具到达 0 次断言通过） | 0 | 同上两日志；red-baseline-subagent_closeout.log |
| F01-C03 | `registration_racing_cancellation_misses_no_node` FAILED（:583——并发注册漏取消） | 同上（barrier 同步 8 注册者+取消者、多级节点，全部节点取消、root 首因保留） | 0 | adv-race.log（修复后定向复跑）＋ red-baseline |
| F01-C04 | 三个 timeout 变体 FAILED（:394 "the child settles cancelled through its timeout"——provider/approval/quota 等待全部不收敛） | 同上（三变体各自断言子 durable=cancelled、线程释放、计数 (0,0)、审批 pending 清零、队列位归还、KID provider 0 次） | 0 | normal-selfcheck/test-cancel_link_inheritance.log |
| F01-C05 | （旧码本项语义已正确：isolation_and_first_reason… RED 阶段即 ok——如实登记为"修复未改变既有正确行为"，作为不回归钉住） | 同上（两无关根、子取消不上行、并发三因线性化后冻结） | 0 | 同上 |
| F02-C01 | `parent_cancel_closes_children_in_process_repeatedly_beyond_the_cap` FAILED（:340 "round 1: the child settles cancelled in-process"——durable 行永 active） | `cargo test -p lingxi-service --test subagent_closeout` → 8/0：per_session_limit=2 下 3 轮取消（每轮 durable cancelled + busy 清 + counts (0,0) + 监督无幽灵）＋第 4 轮正常子成功；测试全程无重启、无 startup_scan（结构性：未第二次 bootstrap） | 0 | normal-selfcheck/test-subagent_closeout.log |
| F02-C02 | `parent_cancel_before_the_childs_first_provider_call…` FAILED（:340）；线程登记窗口拒绝路径旧码本就无残留（thread_registry_refusal… RED 即 ok，钉住） | 同上 8/0（登记窗口拒绝零残留＋零 lineage；派发前取消：子 cancelled、model_call_started=0、busy 清、counts (0,0)；监督级"首 poll 前取消仍记录"单测 tree_firing_before… ok） | 0 | 同上＋ lib 229/0（normal-selfcheck/test-lib-units.log） |
| F02-C03 | 旧码 provider panic 值路径本就走 drive 正常返回（child_provider_panic… RED 即 ok，钉住）；旧码真实缺陷在树取消丢弃路径（F02-C01 RED 已证）与迟到回调无栅栏（静态依据：旧 `note_child_finished` `let _ = child_run_id;`） | 同上 8/0（panic 后线程释放、failed 终态、同线程 reply 续跑成功；迟到/重复回调对抗项见对抗文档） | 0 | 同上 |
| F02-C04 | `drain_expired_child_is_finally_reaped…` FAILED（:324 "the expired child's exit is finally recorded"——幽灵 Running 无句柄） | 同上（超预算子实际退出后 exit=Aborted 记录、同 cap 再 spawn 成功）；监督级 drain_expiry… 旧测试仍绿 | 0 | 同上＋ normal-selfcheck/test-lib-units.log |
| F02-C05 | `panicking_background_exits_are_recorded…` FAILED（SpawnRejected{cap:3}——panic 条目永 Running 耗尽容量）＋ `finished_background_drives_reap_both_registries` FAILED（:324 "the supervised drive entry is reaped"——第二表泄漏） | 同上 8/0（3 次 panic 全部就地记录 Panicked(payload) 可查询、哨兵全程存活、容量可复用；3 轮后台完成→两张表都回收）；无关后台任务隔离由 a06（cancellation_tree 8/0）维持 | 0 | 同上＋ test-cancellation_tree.log |

## 生产入口接线核验（逐项）

- F01：`drive_run` parent_scope 分支 → `register_linked` → `run_root_under` → `link_under`（真实消费链，非仅辅助方法）；`spawn_child` 的 child_scope 与 drive 的 call_scope 走 `child()` → 同一 `link_under`。
- F02：`spawn_child`（dispatch/reply 公用）→ `spawn_linked`（ChildRun → 协作窗口）＋ `ChildCloseout`；后台 `spawn_background_drive` → `spawn_detached`（PanicGuard）＋ `BackgroundDriveRegistry::reclaim_finished`。
- 状态/DB/事件/计数/线程一致性：C01 每轮断言四者同时收口；C04/C05 断言注册表状态与容量。

## 受影响回归

- workspace 全量 65/648/0（含 cancellation_tree、subagent_lifecycle、background_disconnect_recovery、recovery_startup_scan、exit_race_rejections、r03_t08_acceptance_matrix、R02 期全部套件）——无倒退。
- fmt / clippy(-D warnings) / check-contracts(626 entries) / check-boundaries 全部 exit 0（logs/）。

## 未尽事项（如实）

- F02-C02 的"分配 run id 存储故障"窗口无进程内故障注入缝：与 NotBound/SpawnRefused 共用显式回滚路径（SpawnRefused 已实测），静态复核。
- 旧 A15 用例中"重启扫描把父取消后的子行收口为 interrupted_needs_attention"的预期已被本修复替代（同进程收口）；改写后的重启段断言"终态不被复活"。真实崩溃恢复链（recovery_startup_scan 7/0、recovery_crash_points 在 workspace 内）未动。
