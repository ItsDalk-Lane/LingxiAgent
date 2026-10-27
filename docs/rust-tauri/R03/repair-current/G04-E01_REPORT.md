# R03 修复轮 G04-E01 执行报告（F05：去重条目在真正受理前固化，失败重试返回不存在的运行）

- 执行代理：EXECUTOR-REPAIR-R03-G04-E01（一次性执行/修复代理；本报告为执行者口径，不含独立审查）。
- 日期：2026-09-30。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。
- 基线 `FIX_BASE_SHA=cd3fb19e651f763afc6c75cb3163064fb54ca3fe`；候选起点 `198e0da1e`（含已通过独立审查的 G01 取消树/子收尾、G02 统一终态裁决、G03 工具回执 Unknown——本轮**未回退、未破坏**：cancel_link_inheritance 7/0、subagent_closeout 8/0、cancel_terminal_race 13/0、cancellation_tree 8/0、tool_receipt_unknown 6/0 逐套复跑全绿）。本轮无 commit/push（未获授权）。总控账本（`R03_FIX_ISSUES.json`、`R03_FIX_COMMIT_RECEIPTS.json`）未改动（其未提交变更为派单前已存在）。
- 工具链：`~/.cargo/bin/cargo`（rustup 锁定 1.98.1），全部 `--locked`；`rust/Cargo.lock` sha1 `3b659f41eb262eaf42efc2f91ec93f9989d42934` 与 HEAD 相同（零依赖变化）。
- 证据根：`artifacts/rust-tauri/R03/repair-current/G04-E01/`（`normal-selfcheck/`、`adversarial-selfcheck/`、`logs/`）。
- 结论：**READY_FOR_REVIEW**（workspace 69 suites / 689 passed / 0 failed ≥ 底线 67/673/0；fmt 零 diff；clippy `-D warnings` 零告警；check-contracts（626 entries 零漂移）/ check-boundaries exit 0）。

## 1. 实现范围

F05（受理/去重/持久化/后台派发一致性）及其同根因路径（受理生命周期全部写点：前台补偿、后台拒绝补偿、后台任务失败补偿、Replay 事实核验、跨重启同 key 契约）。不涉及 F06（2000 字符截断——`recorded_input` 原样保留）与 F07，不进入 R04。

## 2. 根因复核（结论：审查属实，已用隔离 worktree 红基线实测复现，无反证）

调用链复核与审查清单一致：

1. `sessions.rs` `admit_submission` 的闭包只取得 session lease 与 run id，`dedup.rs` `SubmissionDedup::admit` 在闭包成功后**立即永久**插入 key→run 绑定——此刻既无 durable run 行、也无任何派发事实。
2. 前台随后才在 `drive_run` 内 `record_run_started`；后台随后才 `spawn_background_drive`（注册表 cap 检查 → `spawn_detached`）。两处的失败（存储 IO、注册表/受监督容量）都发生在"绑定已固化、事实不存在"之后，且无回滚。
3. 失败后重试同 key：`admit` 直接命中 Replay 分支，`admit_submission` 不经任何核验返回 `ExecuteAccepted(replayed=true)`——幽灵 Run（`load_run` 不存在）。同进程即可复现。
4. 重启后内存去重表消失，同 key 被静默当作全新提交执行——已有事实（前一生的 durable 运行、journal 未知副作用）被忽略后盲重做。

**红基线实测**（隔离 git worktree，HEAD=198e0da1e，未含修复）：5 个用例 **1 passed（C04 钉）/ 4 failed**（`adversarial-selfcheck/red-baseline-admission_dedup_consistency.log`）：C01 幽灵 replay（`run_000001a0dcba6c00_000001` 无行）、C02 起始失败后同 key 幽灵 replay、C03 窗口内半提交假 replay、C05 重启盲重做。审查的 4 条 source_facts 全部红基线复现。

### 同族路径清单（受理生命周期写点全量枚举，逐一定性）

| 写点 | 修复前事实 | 定性 | 处置 |
|---|---|---|---|
| admission 闭包失败（intake 关闭/busy/注册表满/分配失败） | 不记录（既有正确语义） | 已知未开始，正确 | 不变（dedup 单元钉 `rejected_admission_records_nothing_and_cap_is_loud`） |
| admit 成功即永久绑定 | 预留即承诺 | **F05 本体** | 改为 Pending 预留 + verdict 句柄 |
| 前台 `record_run_started` 失败 | 绑定残留 → 幽灵 replay | **F05 本体** | 真实库核验后补偿（提升/撤销/Unverified） |
| 后台注册表 cap 拒绝 / `spawn_detached` 拒绝（两者之间的窗口） | 绑定残留 → 幽灵 replay | **F05 本体（C01）** | 已知未派发 → 同步撤销预留（loud 拒绝保留） |
| 后台任务内起始写失败 | 绑定残留 Pending 永久滞留 | 同族 | 任务尾核验 + Drop→Unverified 回退 |
| 请求 future 被丢弃（前台断连形态） | 绑定停留于"永久"态（当时无区分） | 同族 | Drop→Unverified；下次同 key 在真实库懒解决 |
| durable start 之后的任何失败（lineage 写失败、finalize 失败、响应丢失） | 绑定保留（正确方向）但与失败类无区分 | 语义需钉 | 承诺点后**永不可撤销**（单元钉 + C04 两态钉） |
| Replay 分支 | 不核验 Run 存在 | **F05 本体** | 仅 Committed（⇒ durable 行存在）可 Replay；结构不变量（见 §5） |
| 重启后同 key | 静默全新执行 | **F05 本体（C05）** | 持久 lineage 锚点（`cause_id="request:{id}"`）→ 显式拒绝指名既有运行 |

## 3. 修复设计（最小完整）

### 3.1 受理生命周期五事实（红线 1 的落地，冻结在 `dedup.rs` 模块文档）

| 状态 | 语义 | 同 key 重复提交所见 |
|---|---|---|
| `Pending` | 预留：活跃提交处于受理与 durable-start 裁决之间 | 显式可重试 `AdmissionInFlight`（绝不暴露半提交假结果） |
| `Committed` | 持久受理：`record_run_started` 已提交——**承诺点** | `Replay`（行存在，真实受理） |
| `Unverified` | 结果不确定：owner 未交付裁决即退出（future 丢弃/后台任务失败/核验读失败） | `Unverified` 决策 → 调用方在真实库上解决（提升+replay/冲突、可证缺席→撤销+重新受理、读失败→显式 Storage 错误） |

外加两类"已知未开始拒绝"：admission 闭包失败（不记录）与派发拒绝（同步撤销预留）。**requestId 对外承诺稳定 Run 身份的时刻 = durable run-start 提交成功**（`runs.rs` 在 `record_run_started` Ok 后立即 `commit_durable()`，代码注释冻结该定义）。

### 3.2 一致顺序与补偿（红线 2）

顺序：容量预留（lease/注册表）→ 请求绑定（Pending）→ 持久受理（run 行）→ 派发（spawn）。补偿按失败点严格分类：

- **已知未派发**（后台注册表 cap / `spawn_detached` 拒绝——包括两者之间的注入窗口）：`release_not_started()` 同步撤销——无任务、无行、无外部动作，重试重新受理；拒绝仍是 loud 错误（不是成功响应）。
- **已知未开始**（前台/后台 `record_run_started` 失败）：对真实库 `load_run` 核验——`None` ⇒ 撤销（run 行不存在证明无任何外部动作：一切外部动作都在 durable start 之后）；`Some` ⇒ 提升为 Committed（行是事实，错误如实返回，重试 replay 真实运行）；读失败 ⇒ `mark_unverified`（保留绑定）。
- **未知执行**（请求 future 丢弃、后台任务 panic、核验不可进行）：`AdmissionBinding::Drop` 回退统一标 Unverified——**绝不删除**；下一次同 key 提交在真实存储上懒解决（`admit_submission` 的有界 4 轮循环：提升→replay/冲突；可证缺席→撤销→重新受理；读失败→Storage 错误）。
- **承诺点之后**：任何错误路径对 Committed 绑定的 release/mark 全部 no-op（`transition` 的状态守卫 + run_id 身份栅栏；单元钉 `committed_binding_is_never_released_or_unmarked`、`a_stale_binding_cannot_touch_a_replaced_entry`）。

### 3.3 Replay 只返回可核实的真实受理（红线 3）

Committed 只能由 durable 事实进入（drive 提交钩子 / `load_run=Some` 的提升）；run 行无 DELETE 路径 ⇒ Committed ⇒ 行存在是结构不变量。窗口内的并发重复拿 InFlight（不返回 run id）；消失的预留 id 不会被复活（adv_c02 断言 `run_id ≠ parked_run_id`）。

### 3.4 隔离、冲突与完整摘要（红线 4）

key 仍为 (owner_kind, owner_subject, session, request_id)；digest 仍覆盖完整规范化输入（CRLF 折叠不变）；同 key 异内容在 Pending/Unverified/Committed 三态都显式冲突（A08 既有断言保持绿）。窗口期间跨会话隔离成立；同会话两主体仍受 per-session busy gate 串行化（R03-T02 冻结语义，如实观测）。

### 3.5 重启同 key 契约（红线 5）

不新建任何分布式机制：持久锚点就是每条 user run 既有的 `run_lineage.cause_id = "request:{id}"`（V4 既有列，无新迁移）。本进程注册表无绑定时查 `RunDatabase::find_run_id_by_request`（runs⋈run_lineage，origin='user'，按 owner/session/cause 精确限定）；命中 ⇒ 显式 `RequestIdBoundToEarlierRun{request_id, run_id}`（可查询该运行的 durable 结局，或换新 id 重发）——绝不静默重做；不命中 ⇒ 正常全新受理。前一生的未知副作用仍由既有 R03-T07 恢复扫描诚实收束（adv_c05 验证 interrupted_needs_attention）。

### 3.6 不做的事（对照"不能这样修好"）

- 不是"任何错误都删 dedup 项"：删除仅限两类可证明未开始/未派发事实；未知一律保留（Drop 回退 + 单元钉）。
- 不伪造 run 行让 Replay 看似存在：消失的预留直接撤销重新受理。
- 后台派发拒绝仍返回错误（`BackgroundRegistryFull`），不是成功响应。
- 不缩短输入摘要（`recorded_input` 的 2000 字符投影原样保留——那是 G05/F06 的范围）。
- 不动 G01–G03 语义（五套件逐套复跑绿）；不破坏 A08（request_dedup 4/0 绿）。

## 4. 改动文件

| 文件 | 改动 |
|---|---|
| `rust/crates/lingxi-service/src/dedup.rs` | 两阶段绑定状态机（Pending/Committed/Unverified + 决策 InFlight/Unverified、Fresh 附 verdict 句柄）；`AdmissionBinding`（commit_durable/release_not_started/mark_unverified/is_committed + Drop 回退）与 `AdmissionRetractor`（派发侧撤销句柄）；`resolve_unverified_present/absent`；模块文档冻结五事实与承诺点；单元测试 6→10（新增 4 个生命周期用例，2 个既有用例按新语义改写） |
| `rust/crates/lingxi-service/src/sessions.rs` | `dedup` 改 `Arc`；`admit_submission` 增端口参数 + 跨重启持久锚点检查 + Unverified 懒解决有界循环；`execute_submission_for` 前台补偿；`execute_background_for` 绑定句柄移交后台任务；新错误变体 `AdmissionInFlight`、`RequestIdBoundToEarlierRun`；`SessionBackend::find_run_id_by_request`（trait/erased/RunDatabase/MemoryBackend）；FakePort 增 `fail_start` 与真实 `load_run`；新增 2 个单元用例（C02/C03 腿） |
| `rust/crates/lingxi-service/src/background.rs` | `spawn_background_drive` 接收绑定句柄：两处派发拒绝同步撤销（含容量检查与 spawn 之间的窗口）；任务内 drive 提交 + 失败尾核验（真实库 load_run → 提升/撤销/Unverified） |
| `rust/crates/lingxi-service/src/runs.rs` | `drive_run` 增 `admission: Option<&AdmissionBinding>`；`record_run_started` Ok 后即 `commit_durable()`（承诺点，注释冻结定义） |
| `rust/crates/lingxi-service/src/subagents.rs` | 子运行 drive_run 传 `None`（无 requestId，无绑定面） |
| `rust/crates/lingxi-adapters/src/storage/run_store.rs` | `find_run_id_by_request`：runs⋈run_lineage 持久锚点查询（owner/session/origin/cause 精确限定，最新优先） |
| `rust/crates/lingxi-service/src/lib.rs` | 导出 `AdmissionBinding`；HTTP 面 `AdmissionInFlight`/`RequestIdBoundToEarlierRun` 映射 + `EndpointError::admission_in_flight`（409 retryable）/`request_id_bound_to_earlier_run`（409，文案含既有 run id 与新 id 指引） |
| `rust/crates/lingxi-service/tests/admission_dedup_consistency.rs` | 新增（5 集成测试 = C01..C05 基线场景；红基线用例即此文件在旧代码 worktree 运行） |
| `rust/crates/lingxi-service/tests/admission_dedup_adversarial.rs` | 新增（5 对抗变体：spawn 窗口注入/窗口内丢响应+取消交错/隔离对照/外部动作前丢响应/未知副作用重启） |

未改动：总控账本、`Cargo.lock`、kernel 源码、存储迁移/校验值、G01–G03 五文件、R02 资产。

## 5. 验证

- **红绿**：`admission_dedup_consistency` 在隔离 worktree（HEAD=198e0da1e）实测 **1 passed / 4 failed**；修复后 **5 passed / 0 failed**（3 次复跑稳定，`normal-selfcheck/stability-3x-suites.log`）。对抗套件 5/0。
- **workspace**：`cargo test --workspace --locked` = **69 suites / 689 passed / 0 failed**（底线 67/673/0；+2 套件、+10 集成、+6 单元，无删除无跳过）。
- `cargo fmt --all -- --check` 零 diff；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`Cargo.lock` sha1 不变。
- 额外门禁：`xtask check-contracts`（626 entries 零漂移）exit 0、`xtask check-boundaries` exit 0；G01–G03 套件与相邻面（request_dedup / background_disconnect_recovery / exit_race_rejections / execute_concurrency / r03_t08_acceptance_matrix / subagent_closeout / recovery_startup_scan / run_lifecycle / service_persistence）逐套复跑绿（`normal-selfcheck/adjacent-suites.log`）。
- 逐 C-ID 两层自查：见 `G04-E01_NORMAL_SELFCHECK.md`、`G04-E01_ADVERSARIAL_SELFCHECK.md`。

## 6. 边界与如实声明

- 无真实供应商/无网络外发/隔离 /tmp 合成数据根；Provider/Tool 替身只产生外部响应与受控外部副作用（独立持久计数文件）。存储装饰器（FailingStartPort/ParkingStartPort）把全部写委托真实 SQLite，仅在审查点名的边界注入受控故障（recovery_crash_points 的 GatedPort 既有模式）；被测的受理去重、Supervisor、真实存储链未被 mock。
- **环境限制**：后台任务**内部**的起始写失败（spawn 成功后 detached 任务里 `record_run_started` IO 错误）无法经真实链确定性注入（后台驱动直接使用 `Arc<RunDatabase>`，无端口接缝——后台断连语义的设计使然）。以三项证据覆盖：与前台 C02 逐字相同的补偿代码形状（同一组 verdict 调用）、Drop→Unverified 回退的单元级钉、Unverified 懒解决在真实存储上的端到端验证（adv_c02）。此披露与 G01/G02/G03 报告同类。
- 语义变化如实登记（均为红线要求的收紧）：(a) 受理窗口内的同 key 并发从"立即假 replay"改为显式可重试 InFlight；(b) 跨重启同 key 从"静默全新执行"改为显式拒绝指名既有运行（新 id、无 id 路径不变）。A12（durable start 后重连 replay）与 A14（退出窗口 replay 仍应答）语义均保持（对应套件绿）。
- 本轮无 BLOCKED 项；对审查结论无反证（4 条 source_facts 全部红基线复现）。
