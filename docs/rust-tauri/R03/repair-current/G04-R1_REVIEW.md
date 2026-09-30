# R03 修复轮 G04-R1 独立对抗性审查（F05：去重条目在真正受理前固化，失败重试返回不存在的运行）

- 审查代理：REVIEWER-REPAIR-R03-G04-R1（一次性独立对抗性 Reviewer，未参与 G04 候选的实现或修复）。
- 日期：2026-09-30。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。
- 候选：HEAD `198e0da1e` + 未提交工作树（`dedup.rs`/`sessions.rs`/`background.rs`/`runs.rs`/`subagents.rs`/`lingxi-service/src/lib.rs`/`run_store.rs` + 新测试 `admission_dedup_consistency.rs`/`admission_dedup_adversarial.rs`）。
- 工具链：一律 `~/.cargo/bin/cargo`（1.98.1），全部 `--locked`；`rust/Cargo.lock` sha1 `3b659f41eb262eaf42efc2f91ec93f9989d42934` 与 HEAD 相同（独立复核）。
- 证据根：`artifacts/rust-tauri/R03/repair-current/G04-R1/`（`logs/`、`probe/`）。
- 审查依据：派单 `docs/rust-tauri/R03/dispatches/R03-REPAIR-G04-R1_REVIEW_DISPATCH.md`；修复清单 F05 节与验收清单 JSON 5 case（`Lingxi_R03_修复验收清单_2026-09-30.json`）；R03 任务书 T04/T07 + A08/A12/A14；02 目标契约 §3/§8；真实 diff 与调用链；执行者三层报告（G04-E01）。

## VERDICT: PASS

## 候选摘要

受理生命周期从「admit 即永久绑定」改为两阶段绑定状态机：`Pending`（预留，重复见显式可重试 `AdmissionInFlight`）→ `Committed`（`record_run_started` 提交成功即承诺点，`runs.rs` 在 Ok 后立即 `commit_durable()`，此后任何失败路径不可撤销）＋ `Unverified` 兜底（owner 无裁决退出：future 丢弃/后台驱动失败/核验读失败，绑定保留、绝不删除，由下一次同 key 提交在真实存储上懒解决：行存在→提升+replay/冲突；可证缺席→撤销+全新受理；读失败→显式 Storage 错误）。已知未派发失败（后台注册表 cap / `spawn_detached` 拒绝，含两者之间窗口）同步撤销预留且仍是 loud 拒绝。跨重启同 key 以既有持久锚点 `run_lineage.cause_id="request:{id}"`（V4 列，无新迁移）显式拒绝并指名既有运行（`RequestIdBoundToEarlierRun`，409）。Replay 仅对 `Committed`（⇒ durable 行存在，全仓无 `DELETE FROM runs`）。

## 逐 C-ID 结果

| C-ID | 正常自查核对 | 对抗性变体 | 独立复测（命令 → 退出码） | 证据 |
|---|---|---|---|---|
| C01 后台 spawn 拒绝后无幽灵 Replay | 首次 `BackgroundRegistryFull{cap:4}` loud；runs 0、外部 0；重试真实启动且 run id 有 durable 行；会话恰 1 运行 | `adv_c01`：容量检查与 spawn 之间窗口（受监督容量占满→spawn 侧拒绝）；撤销后同 key 改内容全新受理、无关 key 不受影响 | `cargo test -p lingxi-service --locked --test admission_dedup_consistency r03_f05_c01` → **0**（1 passed）；`--test admission_dedup_adversarial adv_c01` → **0** | `logs/indep-c01.log`、`logs/indep-adv_c01.log`、`logs/c01-c05-consistency.log`、`logs/adv-c01-c05.log` |
| C02 运行起始持久化失败补偿 | `FailingStartPort` 委托真实库仅在提交前注入 → 首次 `Storage(Io)`、runs 0、外部 0；恢复后同 key 全新受理，最终恰 1 有效执行、外部 1。队列满（`QueueFull`）/IO 失败同属起始 Err 类走同一补偿链 | `adv_c02`：窗口内丢响应（Drop→Unverified）+ 取消交错（诚实 NotFound）；同 key 重试在真实库懒解决→撤销→全新受理（`run_id ≠` 消失的预留 id）→结算→replay | `… r03_f05_c02` → **0**；`… adv_c02` → **0**；单元腿 `--lib sessions::` → **0**（10 passed，含 `start_failure_under_request_id_re_admits_the_same_key_fresh`） | 同上 + `logs/lib-sessions-units.log` |
| C03 并发同 key 和异内容 | `ParkingStartPort` 停在预留/提交之间：同内容并发 = `AdmissionInFlight`（不返回 run id）；异内容 = `DuplicateRequestConflict`（任意相位）；释放后真实受理、结算后幂等 replay 同一运行 | `adv_c03`：跨会话（beta 同主体）自己命名空间全新受理；同会话异主体受 per-session busy gate 串行化（T02 冻结语义，如实观测）；结算后 device 主体同 key 全新受理（run_id 互异） | `… r03_f05_c03` → **0**；`… adv_c03` → **0**；单元 `same_key_duplicate_in_the_start_window_is_in_flight_not_a_ghost`（current_thread 确定性交错）含于 `--lib sessions::` → **0** | 同上 |
| C04 副作用之后响应失败不删绑定 | 外部 +1 后两态丢响应（A：journal started 已提交、future abort；B：receipt 已提交、响应丢弃）：同 key 重试均 replay 既有真实运行、外部计数不增、runs 不增。该 case 在旧代码也绿（红线钉，防"遇错撤销"修过头）——独立红基线复核确认（旧码 1 passed=C04） | `adv_c04`：外部动作**之前**丢响应（run 行已 durable、工具停 0 许可闸门）：replay 真实 dangling-active 行（可查询），`cancel_run_for` 诚实 `DanglingActive`；单元钉 `committed_binding_is_never_released_or_unmarked`（release/mark/drop 对 Committed 全 no-op） | `… r03_f05_c04` → **0**；`… adv_c04` → **0**；`--lib dedup::` → **0**（10 passed） | 同上 + `logs/lib-dedup-units.log` |
| C05 重启重试的安全契约 | 进程 A 完成（外部 +1、completed）→ 同数据根进程 B 全新 bootstrap：同 key = 显式拒绝或既有运行 replay（不盲重做，runs/外部不增）；新 key 正常受理；同 key 改内容同样不静默重做。书面契约冻结在 `dedup.rs` 模块文档 + `sessions.rs` 错误文档 + `RunDatabase::find_run_id_by_request`；不声称 exactly-once | `adv_c05`：未知副作用形态（外部 +1 已发生、journal started、run active 时进程死亡）：恢复扫描诚实收束 `interrupted_needs_attention`（`RecoverySettlement::Written`），同 key = `RequestIdBoundToEarlierRun` 指名该 run，新 key 真实结算 | `… r03_f05_c05` → **0**；`… adv_c05` → **0** | 同上 |

## 独立复核记录（真实命令 + 退出码）

全部命令在 `/Users/study_superior/Desktop/Code/LingxiAgent/rust` 下以 `~/.cargo/bin/cargo` 执行，日志在 `artifacts/rust-tauri/R03/repair-current/G04-R1/logs/`：

1. `cargo test --workspace --locked` → exit **0**：**69 suites / 689 passed / 0 failed**（期望 69/689/0，逐项吻合）。`logs/workspace-test.log`
2. `cargo fmt --all -- --check` → exit **0**（零 diff）。`logs/fmt-check.log`
3. `cargo clippy --workspace --all-targets --locked -- -D warnings` → exit **0**（零告警）。`logs/clippy.log`
4. `cargo run --manifest-path rust/Cargo.toml -p xtask --locked -- check-contracts` → exit **0**：56 generated files 无 diff、API_COMPAT_MATRIX **626 entries 零漂移**。`logs/check-contracts.log`
5. 同上 `check-boundaries` → exit **0**。`logs/check-boundaries.log`
6. G01–G03 回归逐套（均 exit **0**）：`cancel_link_inheritance` 7/0、`subagent_closeout` 8/0、`cancel_terminal_race` 13/0、`cancellation_tree` 8/0、`tool_receipt_unknown` 6/0；相邻面 `request_dedup`（A08）4/0、`background_disconnect_recovery`（A12）3/0、`exit_race_rejections`（A14）**2/0**（见 finding F-1：执行者报告误记 3/0）。`logs/regress-*.log`
7. **红基线（隔离 git worktree `/tmp/r03-g04-redcheck` @ `198e0da1e`，仅拷入新测试）**：`cargo test -p lingxi-service --locked --test admission_dedup_consistency -- --test-threads=1` → exit **101**：**1 passed（C04 钉）/ 4 failed**，失败逐条对应审查 source_facts——C01/C02 重试 `Ok(replayed=true)` 返回 `run_000001a0dcba6c00_000001` 而 runs 无该行（幽灵 Replay 最小反例原样复现）、C03 窗口内半提交假 replay、C05 重启盲重做。`logs/red-baseline-consistency.log`
8. **红→绿翻转（同一 worktree 拷入候选 7 文件 + 对抗套件）**：两套件 → exit **0**（5+5 全绿）——翻绿只依赖候选源码本身。`logs/worktree-candidate-green.log`。worktree 用后已 remove，真实工作区零改动（`git status` 与接手时一致）。

## 绑定状态机审查（派单 §2.2）

- **转换一致性**：`Committed` 为终态——`CommitDurable` 幂等（已是 Committed 则 no-op）、`ReleaseNotStarted`/`MarkUnverified` 对 Committed no-op、`resolve_unverified_absent` 拒绝 Committed、`resolve_unverified_present` 仅作用于 Unverified；`CommitDurable` 可从 Pending/Unverified 进入（后者仅在显式 `mark_unverified` 后同句柄存活的罕见交错可达，行存在才允许提交，语义诚实）。所有 op 带 run_id 身份栅栏（`a_stale_binding_cannot_touch_a_replaced_entry` 单元钉）。
- **删除路径枚举**（`dedup.rs` 全部 `guard.remove` 仅 2 处）：`ReleaseNotStarted`（非 Committed 才删）与 `resolve_unverified_absent`（Unverified+run_id 匹配才删）。调用点定性：前台/后台补偿 `release_not_started` 仅在 `drive_run` 终态 Err 且 `load_run=Ok(None)` 后；派发侧 retractor 仅在 spawn 被拒（future 从未被 poll，无任何写）；懒解决撤销仅在 `load_run=Ok(None)` 后。**无无条件删 key 路径**。
- **TOCTOU 关闭**：`load_run`/`find_run_id_by_request` 与全部写共用单 worker FIFO `DbQueue`（`run_store.rs` 46 处 `queue.submit`）——被丢弃 drive 已入队的 start 写必然先于其后入队的读提交，故"可证缺席"在单写者 FIFO 下成立（外部并发写者由 WAL 写锁排除）。
- **Replay 只返回可核实受理**：Committed 仅可由三类 durable 事实进入（drive 承诺点 / `load_run=Some` 提升×2），全仓无 `DELETE FROM runs`（独立 grep，仅 key_events 截断与测试迁移有 DELETE）⇒ Committed ⇒ 行存在为结构不变量。
- **并发不暴露半提交**：admission 闭包在注册表锁内执行（busy gate/分配均为同步快速路径，`allocate_run_id` 纯原子，无锁序倒置）；窗口内重复得 InFlight；admission 失败不记录；cap 满为 loud `DedupRegistryFull`。

## 新语义收紧与契约一致性（派单 §2.3）

- `AdmissionInFlight`（409 retryable）与 `RequestIdBoundToEarlierRun`（409，文案指名既有 run 并给出新 id 指引）均为修复红线 1/2/5 的直接推论。A08（同 key 异内容冲突，三态均冲突）`request_dedup` 4/0 绿；A12（durable start 后断连重连 replay）`background_disconnect_recovery` 3/0 绿——InFlight 仅存在于 durable start 前的窗口（此刻尚无任务可"继续"），未破坏 A12；A14（退出窗口已受理 replay 仍应答）`exit_race_rejections` 2/0 绿（Replay 决策不经过 intake 闭包）。HTTP 映射穷尽（match 全变体），`check-contracts` 626 零漂移复核 exit 0。
- 隔离与摘要：key 仍为 (owner_kind, owner_subject, session, request_id)；digest 覆盖完整规范化输入（CRLF 折叠），2000 字符投影仅影响 `recorded_input` 展示串（F06/G05 范围，未动）。四条 forbidden_shortcuts 均未违反（拒绝仍 loud、不伪造 run 行、不删已承诺绑定、不缩短摘要）。

## 执行者披露专项（派单 §2.4）——已升级为动态编排验证

执行者披露：后台任务**内部**起始写失败（spawn 成功后 detached 任务里 `record_run_started` IO 错误）无端口接缝（驱动直接持有 `Arc<RunDatabase>`，为后台断连语义有意为之）。本人不满足于结构论证，在隔离 /tmp worktree 编排了**无测试端口**的真实注入：外部 rusqlite 连接（独立线程持有）对真实库 `BEGIN EXCLUSIVE` 占住 WAL 写锁 → detached 任务的 start 写在 worker busy_timeout（5s）后 SQLITE_BUSY 失败（发生在任何 durable 事实之前）。结果：

- **候选**：exit **0**（5.43s）——任务尾补偿在真实库 `load_run=Ok(None)` → 撤销预留；同 key 重试全新受理（`!replayed`、新 run id）；最终恰 1 真实执行；结算后同 key replay 同一运行。
- **旧码（198e0da1e 源码还原后同 probe）**：exit **101**（5.38s）——重试返回 `replayed=true` 的幽灵（该 run 无 durable 行），即披露所指缺陷原样复现。

即：披露缺口不仅是真实缺陷（旧码红），候选补偿在真实链路上动态有效（绿）。probe 源码存 `probe/g04r1_reviewer_probe.rs`（仅存在于 /tmp worktree 与本证据目录，未进真实工作区）。执行者的三项覆盖（相同补偿代码形状 + Drop 单元钉 + adv_c02 真实库懒解决）+ 本编排，判定**充分**。

## finding 清单

- **F-1（LOW，证据引用不准确，不影响候选）**：`G04-E01_ADVERSARIAL_SELFCHECK.md` L66 记 `exit_race_rejections 3/0`，该套件实际 2 个测试（本人复跑 2/0 绿）；`G04-E01_REPORT.md` §5 将 `exit_race_rejections`/`execute_concurrency`/`r03_t08_acceptance_matrix`/`recovery_startup_scan`/`run_lifecycle`/`service_persistence` 归入「逐套复跑绿（normal-selfcheck/adjacent-suites.log）」，但该日志实含 9 套件、不含上述 6 项（它们由 69 套件 workspace 全量覆盖，执行者与本人的 workspace 日志均绿）——断言实质成立，引用出处不精确。
- **F-2（INFO，性能观察，非缺陷）**：`find_run_id_by_request` 在每个显式 id 且注册表无绑定的提交上执行一次 runs⋈run_lineage 查询；`cause_id` 无专用索引（经 `idx_runs_session` 按会话行收窄后过滤）。当前规模可接受；若提交量增长可考虑 `(origin, cause_id)` 索引。
- **F-3（INFO，行为注记，非缺陷）**：`record_run_started` 与 `record_run_lineage` 之间崩溃会留下"有 run 行、无锚点"的窗口——此后同 key 重启重试按无锚点全新受理。安全依据：一切外部动作都在 lineage 写之后，该窗口内无已确认/未知外部副作用；悬挂行由恢复扫描诚实收束为 interrupted_needs_attention。与 C05 契约意图一致，如实注记。

## 误判反证 / 需标 STALE

- 无：原审查 4 条 source_facts 全部在隔离红基线独立复现（幽灵 Replay 同 run id），执行者无反证声明亦获本人复核支持。
- 无 STALE 项。

## 审查范围声明

只审 G04（F05，C01–C05）。已读：派单全文、修复清单 F05 节、验收清单 JSON 5 case（given/when/then/adversarial_variation/evidence_required 逐条比对）、R03 任务书 T04/T05/T07 与 A08/A12/A13/A14 定义、02 目标契约 §3/§8、候选全部 7 文件 diff 与调用链（sessions.rs 前后台受理链、dedup.rs 状态机、background.rs 派发、runs.rs 承诺点、run_store.rs 锚点查询、lib.rs HTTP 映射）、两套新测试全文、执行者三份报告与其证据日志抽查、总控账本 diff（仅为派单前已存在的 G04-E01 记账，不在审查范围、未改动）。独立执行：门禁 5 项、逐 C-ID 10 次单测、单元 2 组、回归 8 套件、红基线 worktree（红→绿翻转）、披露专项编排 probe（候选绿/旧码红）。未修改任何产品/测试/配置/门禁/账本/执行者证据；未 commit/push；复测产物仅写 `artifacts/rust-tauri/R03/repair-current/G04-R1/` 与本报告。环境限制：无真实供应商/网络外发，替身仅产生外部响应与受控副作用；本地结果不代替其他平台或真实供应商验证。
