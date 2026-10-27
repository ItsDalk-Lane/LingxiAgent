# G01-E01 对抗性自查（第二层）

执行代理：EXECUTOR-REPAIR-R03-G01-E01。候选：`cd3fb19e6` + 未提交修复工作树。
方法：先以新增反例在**未修复代码**上取得 RED（失败依据，`adversarial-selfcheck/red-baseline-*.log`），修复后对每个 adversarial_variation 定向复跑（barrier/固定调度/受控故障点；不靠重跑碰绿）。证据根：`artifacts/rust-tauri/R03/repair-current/G01-E01/adversarial-selfcheck/`。

## F01-C01｜混用三入口、不同深度取消

- 攻击窗口：`run_root_under`（register_linked 消费链）/`register_linked`/`child` 混合建树；在根取消与中间深度取消两种位置开火。
- 观测（修前）：根取消后 `child_entry` 未取消（red log :450）；修后：全部节点取消、waiter 250ms 内醒来、中间深度取消只向下（父不取消）。
- 是否推翻：否。命令 `cargo test --test cancel_link_inheritance`（7/0，exit 0）＋源内单元 `linked_run_root_receives_the_parent_cancellation`、`a_child_scope_never_cancels_its_parent`。
- 证据：normal-selfcheck/test-cancel_link_inheritance.log；red-baseline-cancel_link_inheritance.log。

## F01-C02｜取消与创建的两种顺序（barrier 前后）

- 攻击窗口：(a) 先取消后创建（unit：child/run_root_under/register_linked 三入口）；(b) 服务面——父 turn2 持唯一 model 许可泊车、子首模型调用停在**受理队列**时父被取消（注册先、取消后，且子尚未产生任何外部动作）。
- 观测：(a) 修前无继承（red :498）→ 修后继承标志/首因/时刻 + 相位 Requested；(b) 修前子**完成**（取消未达，provider 被调）→ 修后子 cancelled、`model_call_started`=0、工具 0 次、队列位归还、父泊车不受子取消影响（不上行）。
- 是否推翻：否。命令 `cargo test --test cancel_link_inheritance`（7/0）＋ `--test subagent_closeout parent_cancel_before -- --nocapture`（exit 0）。
- 证据：adv-preprovider.log；red-baseline-subagent_closeout.log。

## F01-C03｜锁边界竞态、遍历后加入、多级节点

- 攻击窗口：std::sync::Barrier 同步 8 个注册线程与取消者同时开火；每个注册者再建孙节点（多级）；断言对**任意交错**成立。
- 观测：修前"every node registered … is cancelled" 失败（red :583，遍历后加入漏取消）；修后 16/16 节点全取消、root 首因保留、事后加入节点继承。
- 是否推翻：否。命令 `cargo test --test cancel_link_inheritance registration_racing -- --nocapture`（exit 0）。
- 证据：adv-race.log；red-baseline-cancel_link_inheritance.log。
- 并发取消线性化：三线程不同 reason 同时 fire（barrier）→ root reason ∈ 三者之一且事后冻结（isolation_and_first_reason… 内断言）。

## F01-C04｜三类等待分别收敛（Provider/审批/配额）

- 攻击窗口：子自身 timeout（250–400ms）在 (a) provider 泊流、(b) 审批 pending、(c) model 受理队列（父持许可泊车、子 KID 剧本永不可达）三种停泊点到达；门/许可**不释放**，唯一出口是 timeout 的作用域取消经链接树抵达内层 drive。
- 观测：修前三个变体全部"timed out … to become cancelled"（red :394 ×3——drive 永悬）；修后子 durable=cancelled、线程释放、counts (0,0)、(b) 审批 pending 清零+工具 0 次、(c) 队列位归还且父仍持许可（子超时不上行）、(c) 子 provider 0 次。
- 是否推翻：否。命令 `cargo test --test cancel_link_inheritance`（7/0）。
- 证据：normal-selfcheck/test-cancel_link_inheritance.log；red-baseline-cancel_link_inheritance.log。

## F01-C05｜隔离与重复取消

- 攻击窗口：两无关根；对根 A 连发 3 个不同 reason（顺序）+ 3 线程并发不同 reason（barrier）+ 事后覆盖尝试；根 A 之子取消。
- 观测：根 B 全程 Active 未取消；子取消不上行；A 的 reason 线性化后冻结。旧码即正确（RED 阶段 ok）——本项为不回归钉住，非新修复。
- 是否推翻：否。命令同上。
- 证据：normal-selfcheck/test-cancel_link_inheritance.log。

## F02-C01｜同进程重复超上限后可继续使用（禁止重启/startup_scan）

- 攻击窗口：per_session_limit=2，连续 **3** 轮"派发泊车子→取消父"（超过上限次数），每轮同时核 durable 终态/busy/active_counts/监督登记；随后第 4 轮派发正常子。测试无任何第二次 bootstrap（结构性禁止重启与 startup_scan）。
- 观测：修前第 1 轮即 "timed out … round 1: the child settles cancelled in-process"（red :340，durable 行永 active）；修后 3 轮全收口（cancelled + busy false + last_run_status=cancelled + (0,0) + live_children 空），第 4 轮 parent/child 双 completed。
- 是否推翻：否。命令 `cargo test --test subagent_closeout`（8/0，exit 0）。
- 证据：normal-selfcheck/test-subagent_closeout.log；red-baseline-subagent_closeout.log。

## F02-C02｜预留/登记线程/派发/未首 poll 各窗口

- 攻击窗口：(a) 线程登记满（thread_registry_cap=1，唯一 open 线程泊车）→ 第二派发拒绝；(b) 派发成功但子停在 model 受理队列（未到 provider）时父取消；(c) 监督级：current_thread 下 spawn 后**同步**取消（wrapper 未首 poll，polled 标志证明）。
- 观测：(a) 拒绝后 threads_of(拒绝会话) 空、counts (0,0)、lineage 仅 1 条（拒绝零创建）、结构化 tool failure 到模型、首线程不受扰；(b) 见 F01-C02(b)；(c) exit=Aborted 记录、future 从未被 poll、entry 由 wait 回收。(a) 旧码即正确（RED ok，钉住）；(b)(c) 修前失败。
- 是否推翻：否。命令 `--test subagent_closeout`（8/0）＋ lib `tree_firing_before_the_first_poll_still_records_the_abort`（adv-prepoll.log exit 0）。
- 证据：adv-prepoll.log；red-baseline-subagent_closeout.log。派发拒绝（监督 cap 满 → SpawnRefused）旧路径由 F02-C05-panic 场景反向证明（修前 cap 被 Running 幽灵耗尽 → 拒绝；修后可 spawn）。
- 未覆盖（如实）：run-id 分配的存储故障窗口无进程内注缝（与 NotBound/SpawnRefused 同一回滚代码路径，静态复核）。

## F02-C03｜异常结束必达清理＋迟到旧回调

- 攻击窗口：子 provider 在受控位置 panic（panic 穿透 model-call 子 → 修后由 PanicGuard 就地收容）；随后同线程 reply 续跑；再注入 (1) 迟到的**被超越** run 完成回调、(2) 已结算 run 的重复回调（`note_child_finished` 为 pub 审计面，注入即真实簿记代码）。
- 观测：panic 后子 failed 终态、线程释放、counts (0,0)、续跑 child completed；注入 (1)：当前运行 busy 不被清、counts 保持 (1,1)；注入 (2)：恰好一次，counts 保持 (1,1)、busy 不动；随后真实完成照常收口 (0,0)。修前：树取消路径泄漏（C01 RED）；迟到回调无栅栏为静态依据（旧 `let _ = child_run_id;`）。
- 是否推翻：否。命令 `--test subagent_closeout late_and_duplicate -- --nocapture`（adv-fence.log exit 0）。
- 证据：adv-fence.log。

## F02-C04｜超时中止最终可回收（请求≠观察）

- 攻击窗口：子任务 250ms 自有限让出，drain 预算 30ms（超预算后**有限**让出，不用无限忙等）；drain 超时 → abort 请求 → 观察实际退出 → 同 cap 再 spawn。
- 观测：修前 exit 永不记录（"timed out … exit is finally recorded"，red :324——Running 无句柄幽灵）；修后 unconfirmed 报告如实（exit=None），实际退出后 exit_of=Aborted（分离观察者记录），容量压力回收后再 spawn 成功且正常完成。
- 是否推翻：否。命令 `--test subagent_closeout drain_expired…`（suite 8/0）。
- 证据：normal-selfcheck/test-subagent_closeout.log；red-baseline-subagent_closeout.log。协作窗口对抗（超窗最后手段）另见 adv-window.log（Drop 探针证明确实丢弃）与窗口内自收尾测试。

## F02-C05｜后台 panic 丢句柄与两注册表隔离

- 攻击窗口：真实后台监督任务 panic（fire-and-forget 丢句柄形态）×3 + 无关哨兵持续运行；服务面 3 轮真实后台 run 完成后查两张表；重复足量次。
- 观测：修前第 3 次 panic spawn 即 `SpawnRejected{cap:3}`（panic 条目永 Running，容量被尸体耗尽——red :976）且完成驱动在 TaskSupervisor 的条目不回收（red :324）；修后每次 panic 的 `Panicked(payload)` 就地可查询（无需 waiter）、哨兵存活并完成、压力回收后新 spawn 成功、无 Running 幽灵；服务面每轮 live_ids 收缩且 `background_drive:{id}` 监督条目消失（双表同收）。
- 是否推翻：否。命令 `--test subagent_closeout`（8/0）。
- 证据：normal-selfcheck/test-subagent_closeout.log；red-baseline-subagent_closeout.log。

## 交错清单核对（总控 §7 要求逐项）

- 取消在子节点登记前/后：F01-C02/C03（barrier 两序 + 锁边界竞态）。
- 工具派发前：F02-C02(b)；工具返回后/持久化等待中的迟到结果：F03/G02 范围（本单不涉及；本单的"迟到完成回调"以 F02-C03 注入覆盖簿记面）。
- 子代理正常结束（lifecycle 3/0）、父取消（C01）、自身 timeout（F01-C04）、panic（C03）、派发拒绝（C02a）、未首 poll（C02c）、清理超时后实际退出（C04）。
- 同 Run 并发取消/不同原因重复取消/相位不倒退：F01-C05 + `registry_fire_tracks_the_first_reason_and_is_idempotent`（既有）+ register_linked 继承相位单测（Repeated 起步，fire 幂等）。
- 迟到旧完成回调 vs 新 child_run_id：F02-C03 注入（身份栅栏 + accounted 环）。
- 同进程重复超上限后可继续使用：F02-C01。
- 两个注册表对照：F02-C05。

## 结论

两轮（RED→GREEN）均真实执行、退出码落盘；未发现修复被推翻的情形；发现并修复了自查过程中的全部新失败（详见执行报告 §6）。残余边界（协作窗口与父 drain 的同锚点微秒级竞态、run-id 分配故障窗口无注缝）已在执行报告 §7 如实登记，不构成普通取消泄漏路径。
