# R03-T07 报告｜故障恢复与服务退出策略（EXECUTOR-R03-T07-E01）

- 状态：**READY_FOR_REVIEW**（执行者口径；独立复核归属总控另派）
- TASK_ID：R03-T07（ACCEPTANCE_IDS：R03-A13、R03-A14）
- TASK_BASE_SHA：`0c138456bbe791f21621f5a24eb47bc1948f7585`（分支 `codex/rust-tauri-migration`，无 commit/push；工作树候选留给总控冻结）
- 执行时间：2026-09-29（证据时间见 candidate-summary.txt / 各 log）
- 环境：macOS darwin 27.0.0 arm64；rustup 锁定工具链 **1.98.1**（全部命令经 `~/.cargo/bin` rustup 代理 + `--locked`；证据链统一 `CARGO_TARGET_DIR=/tmp/r03-t07-target`）；SQLite=rusqlite 0.40.2 bundled；无网络外发、无真实供应商；npm/桌面栈零触碰；**tokio test-util feature 未引入**（无新依赖，`rust/Cargo.lock` 零变化 = `90111c4b…` R02_HANDOFF 原值；确定性方案沿用 T02–T06 先例：门控替身 + 通道/文件锚点 + 1ms 轮询有界等待）
- 交付物三件：**RecoveryCoordinator**（`lingxi-service/src/recovery.rs`，bootstrap 启动扫描接入）、**恢复分类表**（`lingxi-kernel/src/recovery.rs` 四态分类 + 每类用户可见原因/后续动作，纯函数 7 单测锁定）、**崩溃点测试集**（`lingxi-service/tests/recovery_crash_points.rs`：真实子进程 kill -9 × 11 个关键持久化边界前/后 + 可重放 seed + 崩溃前后轨迹）

---

## 1. 冻结语义溯源（先读规格与现状，再映射——不发明交互）

阶段书 R03-T07 原文 4 步 + 总控细化（派单 §Task 规格/§总控细化）与现状对照：

| 冻结语义 | 规格出处 | Rust R03-T07 映射 |
|---|---|---|
| **启动扫描非终态 run，按 journal 区分可恢复等待/可重试只读/unknown 外部副作用** | 怎么做 1；总控细化「非终态 run 恢复分类表」 | `RunDatabase::list_active_runs`（新 SELECT：仅 queued/running/waiting_approval/cancelling，终态永不进清单——终态不复活在列举层就成立）；每 run 先跑 T05 `recover_run_invocations`（journal 四态 + unknown 判定持久化），再由 kernel `classify_run_recovery` 依据**最坏优先**阶梯（NeedsAttention > 同键恢复/外部核验 > 只读重执 > 无崩溃窗口条目）得出 run 级四类之一 |
| **对无法安全继续的任务写 interrupted/needs_attention，暴露原因及可执行后续动作；不虚构模型最终回复** | 怎么做 2 | 扫描对每个非终态 run 经**同一单一 finalize 事务**（`StoragePort::commit_run_outcome`，kernel `RunStateMachine::finalize` 在事务内裁决——与 `RunSupervisor::finalize_settlement` 同一条存储路径）写 `interrupted_needs_attention`；run 行 reason 保持稳定词表（`interrupted_needs_attention.recovery_unsafe`），**终态 `run_state_changed` 事件 reason 携带富解释**（类别 + journal 事实 + next actions）——事件流即用户可见呈现面；`final_message=None` 恒成立（无任何类别编造回复，messages 表断言为证）。`cancelling` 在途者完成其已请求的 `cancelled`（不把用户自己的取消改写成中断）；`queued/waiting_approval` 先走合法非终端腿回 `running`（kernel 机无直连终态边，中途腿 reason 标注 `recovery_scan:`，历史如实） |
| **退出先拒绝新提交，按任务类型取消/等待，再 flush/join；明确 timeout 与残留报告** | 怎么做 3；总控细化退出序 | ①`SubmissionIntake` 单向闸（bootstrap 构造、二进制信号 future **第一动作**关闭——先于任何 drain）；拒绝点在**共享 admission 链的 admission 闭包内**（前台 HTTP execute 与后台 spawn 面同链拒绝 `ShuttingDown`→HTTP 503 `shutting_down`/`service.shutting_down`、retryable；同 requestId+同内容的**幂等重放仍被回答**——那是已受理任务的查询不是新提交）②任务类型处置：前台内联 run 由 transport drain 等待（R02 既有）；后台 drive 在 join 前经 supervisor 取消树**请求取消**（drive 在自身 await 点观测、经自身唯一 finalize 落 `cancelled` 并提交关键记录）③有界 drain join（T06 钩子），到期未确认者如实报 residue（`ShutdownReport.background_unconfirmed`，durable 行保持 active 交由**下一进程的启动扫描**闭环——T06 递延件在此收口）④storage close flush → record cleanup（R02 链不动）；退出码表 0–6 语义零变化 |
| **可控崩溃点测试每个关键持久化边界，保留重放种子** | 怎么做 4；总控细化 A13 真实进程级强杀 | `recovery_crash_points.rs`：父测试 spawn 真实子进程（`current_exe` 同一测试二进制）+ 测试本地 `GatedPort`（StoragePort 装饰器：全部写经真实 RunDatabase，在指定边界前/后写 marker 后永久停泊）→ 父进程见 marker 后 `Child::kill()`（unix=SIGKILL，真实强杀：无清理/无 flush/无 drop）→ 全新生产形态 bootstrap 重启（扫描在内）→ 逐 case 断言诚实呈现。**11 个崩溃点**：run 状态迁移（record_run_started）前/后、journal intent 前/后、started 前/后、外部副作用后/回执前、receipt 后、终态提交前/后；每 case 落 `{case}.seed.json` + `{case}.pre.json`（强杀前库内事实，独立 rusqlite 句柄读 WAL 已提交数据）+ `{case}.post.json`（重启后全部事实） |
| **复用既有件、不另建第二恢复通道** | 总控细化首条 | journal 面=T05 `recover_run_invocations` 原样调用；run 终态=同一 `commit_run_outcome` 事务；分类核心=kernel 纯函数（T05 六决策的 run 级提升，未复制决策逻辑）；退出=R02 `graceful_shutdown` 链上**加相位**而非另建协调器；取消=supervisor 既有取消树。全链无第二调度器/存储/事件面 |
| **新存储走 V5+；V1–V4 指纹不动；登记递延 T08** | 总控细化 | 本 Task **零新迁移**（`list_active_runs` 为既有表 SELECT）；V1=`479b0321…`、V2=`64d7edfd…`、V3=`3bd5388f…`、V4=`371d4154…` 逐字节未动（r02-script-probe S4 dump 对账，userVersion=4）。V2/V3/V4 注册表与 S4 硬编码递延 T08（与 T04 R1-F1/T05/T06 同根因，见 §9） |

## 2. 实现与调用链（真实接线）

### kernel（lingxi-kernel）

- **`src/recovery.rs`（新，纯函数 + 7 单测）**：`RunRecoveryCategory`（四态，wire 名 `recoverable_wait`/`retryable_read_only`/`unknown_side_effect`/`interrupted_needs_attention`）；`RunRecoveryPlan{category, user_reason, next_actions, subject_journal_ids}`（确定性 reason：无时间戳、无编造完成措辞）；`classify_run_recovery(&[RecoveryDecision])`（最坏优先阶梯）；`principal_from_storage_facts(kind, subject)`（runs 行所有权键→Principal 重建；device 等非所有权细节不声称——存储校验只看 kind+subject；未知词表响亮 Corrupted）。模块文档即**恢复分类表**（四类×持久事实×R03 动作×用户可见原因，含不虚构回复/终态不复活两条诚实规则）。
- `src/lib.rs`：`pub mod recovery;`（一行）。

### adapters（lingxi-adapters）

- `run_store.rs`：`list_active_runs()`（新查询，经同一单写者队列；未知 status=Corrupted；无 attempt 行=Corrupted——`record_run_started` 恒开 #1，缺行即自相矛盾的事实，拒绝猜测）返回 `ActiveRunFacts{run_id, session_id, owner_kind, owner_subject, status, generation, current_attempt}`——重建写上下文所需的全部身份事实。
- `mod.rs`：导出 `ActiveRunFacts`。**零迁移、零表变更。**

### service（lingxi-service）

- **`src/recovery.rs`（新）**：`RecoveryCoordinator::run_startup_scan(storage, events, capabilities, now)`——列举→逐 run：T05 journal 面（unknown 判定持久化，幂等）→ kernel 分类 → 终态落库（自建 `RunOutcome`：行 reason=稳定词表、事件 reason=富解释；与 `finalize_settlement` 的差异仅在事件 reason 富化，同事务同 kernel 裁决，注释明示对齐）→ `events.publish_committed`。并发收束竞态：finalize 撞 Conflict 时复查 `load_run`，真终态→`AlreadyTerminal`（如实记录、不重写）；否则响亮传播。`RecoveryScanReport`（scanned + 逐 run 结果 + `category_counts()`）。
- **bootstrap 接入（lib.rs）**：`bootstrap_with_instance_identity` 在 supervisor 构造后运行扫描，失败=`ServiceStartupError::Storage`（exit 2，fail-closed，与 R02 完整性闸同立场）；报告存 `Arc<OnceLock<RecoveryScanReport>>`，`state.recovery_report()` 可查。`ServiceDeps` 新增 `recovery_capabilities: Arc<dyn RecoveryCapabilitySource>`（默认 `ConservativeCapabilities`；R04 注册表替换解析而非默认值）。
- **`src/shutdown.rs`**：`SubmissionIntake`（AtomicBool 单向闸 + 单测）；`graceful_shutdown` 新参 `&RunSupervisor`，WS drain 后新增**任务类型取消相位**（`background.live_ids()` 逐个 `cancel_run("service shutdown")`，随后既有有界 drain join）；`ShutdownReport` 新增 `background_cancel_requested` / `background_unconfirmed`（残留如实；exit code 映射不变）。
- **`src/sessions.rs`**：`SessionExecuteError::ShuttingDown`；admission 闭包首查 `intake.is_closed()`（在 busy 闸/run-id 分配/任何写入之前；重放不经闭包故仍被回答）；`SessionStore::with_concurrency_and_intake`（组合根注入；`with_concurrency` 委派保持测试构造零扰动）。
- **`src/main.rs`**：信号 future 第一动作 `intake.close()`（先于 `ws.request_close()`）；`graceful_shutdown` 传 `&runs`；exit 6 诊断补 residue 字段。
- **`lib.rs` 路由**：`ShuttingDown → EndpointError::shutting_down()`（503 + `shutting_down` + `service.shutting_down` + retryable=true）。

### 闭环（T06 递延件收口）

T06 退出钩子留下的 unconfirmed durable-active 行 → 下一进程 bootstrap 扫描解决（`exit_race_rejections.rs` 断言已收束路径；`startup_scan_*` 断言扫描路径）；T05 报告 §9「恢复协调未提前」与本 Task §1 表逐项对应兑现。

## 3. 逐验收：预期 vs 实测

### R03-A13 运行中重启诚实呈现 — **PASS（真实进程级 kill -9；证据=崩溃前后轨迹）**

`r03_a13_kill9_crash_matrix_honest_restart_presentation`（recovery_crash_points.rs；11 case 全绿，kill9-suite.log EXIT=0）：

- 每 case：spawn 真实子进程（同测试二进制 `--exact crash_child_boundary_harness`，真实组合根 + 真实 RunDatabase + 响应生产型替身 + GatedPort）→ 子进程在边界写 marker 后停泊 → 父进程 `Child::kill()`（SIGKILL）→ `wait()` 收割 → 全新生产形态 bootstrap 重启 → 断言。
- **崩溃前轨迹**（pre.json，独立 rusqlite 读 WAL 已提交数据）：run 行数/状态、journal phases、外部执行数、marker（boundary/side/pid）。**崩溃后轨迹**（post.json）：终态状态+reason、journal phases+receipt outcome、final_messages 计数、恢复事件 reason、外部执行数。
- 关键实测（a13-kill9/ 33 个轨迹文件）：
  - `started_after`/`external_after`/`receipt_before`：journal=started→重启后 **unknown**（receipt outcome=unknown）、类别 **interrupted_needs_attention**、事件 reason 含「blind retry」「verify the outcome」；`external_after` 外部计数强杀前=1、重启后**仍=1**（副作用后崩溃不重复执行）；
  - `run_started_after`/`intent_*`/`started_before`/`receipt_after`/`finalize_before`：**recoverable_wait → interrupted**（无崩溃窗口条目/纯 settled），final_messages=0（不虚构回复）；
  - `finalize_after`：run 已 completed（drive 自身 finalize 先落地）→ 重启扫描 scanned=0、状态保持 completed（**终态不复活**）；
  - `run_started_before`：无任何 durable run 行（admission 是内存态）→ 扫描无对象、run_count=0（空白=没有对象，不是隐藏）；
  - 全部 interrupted case：行 reason=`interrupted_needs_attention.recovery_unsafe`（稳定词表）+ 事件 reason 富解释（含 `service restarted`）。
- 重放 seed：`{case}.seed.json`（固定 session/input/requestId/target + 边界配置）随轨迹落证据目录。

### R03-A14 退出不接收新任务 — **PASS（退出窗口竞态测试，服务层 + 真实 HTTP 传输层双证）**

`shutdown_window_rejects_concurrent_submissions_and_settles_existing_tasks`（exit_race_rejections.rs）：

- 前置：后台提交（requestId `exit-1`）停泊于工具 I/O（已有任务）；`submission_intake().close()`（与二进制信号 future 同一单向闸、同一调用）。
- 并发提交：10 路新提交（双会话×前台/后台面×plain/显式 id）→ **全部 `ShuttingDown`**，run 总数不变（零分配零写入）；同 requestId 同内容**重放被回答**（replayed=true、原 run id）。
- 退出进行中再提交：drain 期间再 4 路 → 仍全拒。
- 已有任务按策略收束：`graceful_shutdown`（真实协调器）→ 取消请求落于该 run（`background_cancel_requested=[run]`）→ drive 经自身唯一 finalize 落 **cancelled**（`cancelled.requested`，库文件直查）→ drain 确认（unconfirmed 空，exit code 0）。
- 传输层：`http_execute_is_refused_with_503_while_shutting_down`——真实 `run()` 服务 + 真实 TCP POST：关闭前 200；关闭后 503 + `shutting_down` + `service.shutting_down` + retryable（三路并发 POST 全拒）。
- 证据：`r03_a14_exit_window`（r03-t07-evidence.json：refused_before=10、during=4、replay_answered、terminal=cancelled、exit_code=0）+ `R03_A14_TRACE`。

### 必须交付三件 — PASS

- **RecoveryCoordinator**：生产代码 `src/recovery.rs` + bootstrap 真实接线（每个进程启动即运行；601/0 全量内所有 bootstrap 均经此路径）。
- **恢复分类表**：kernel `recovery.rs` 模块文档表 + `RunRecoveryCategory` 词表 + 每类 reason/next_actions（7 单测锁定阶梯/词表/内容/不虚构）。
- **崩溃点测试集**：11 边界 × 前/后 + seed + 双轨迹（§3 A13）。

### 任务书「怎么做」1–4 对照

1. 启动扫描 + journal 三区分 ✔（四类分类表，最坏优先）
2. interrupted/needs_attention + 原因与后续动作 + 不虚构回复 ✔（稳定行 reason + 富事件 reason + final_messages=0 断言）
3. 退出先拒新提交→按类型取消/等待→flush/join→timeout/残留报告 ✔（§2 退出序）
4. 可控崩溃点 + 重放种子 ✔（11 case + seed/轨迹落盘）

## 4. 修改文件清单

修改（10 tracked + 1 doc）：`rust/crates/lingxi-kernel/src/lib.rs`、`lingxi-adapters/src/storage/{mod.rs,run_store.rs}`、`lingxi-service/src/{lib.rs,main.rs,sessions.rs,shutdown.rs}`、`lingxi-service/tests/{execute_concurrency.rs,invocation_journal.rs,shutdown_coordinator.rs}`、`docs/rust-tauri/R02/SERVICE_START_AND_SHUTDOWN.md`（停止链/重启恢复两段更新为 T07 语义——文档一致性，非新功能）。
新增（7 rust 路径 + 1 支持目录）：`lingxi-kernel/src/recovery.rs`（生产+7 单测）、`lingxi-service/src/recovery.rs`（生产）、`lingxi-adapters/tests/active_run_listing.rs`（2）、`lingxi-service/tests/{recovery_startup_scan.rs(7),exit_race_rejections.rs(2),recovery_crash_points.rs(矩阵+子进程 harness)}`、`lingxi-service/tests/recovery_support/harness.rs`（共享夹具；子目录=非 cargo 目标）。
`rust/Cargo.lock` 零变化（`90111c4b…`）；三个 crate Cargo.toml 零变化。逐文件 SHA-256 与 tracked-diff 绑定值见 candidate-summary.txt。

### 既有测试夹具改动（逐处+理由；无断言删除/放宽/改永真，无 skipped）

1. **T05 A09（invocation_journal.rs）**：重启断言从「dangling active=running」改为协调器语义——`recovery_report()` 断言（scanned=1、类别、unknown_verdicts_persisted=1）+ 状态 `interrupted_needs_attention` + 行 reason 稳定词表 + **无 final message** + 显式 `recover_run_invocations` 重跑断言 `unknown_verdict_persisted=false`（幂等重放）。「外部计数=1 不变」核心断言原样保留。T05 报告 §10.4 已预登此重验义务。
2. **R02-A07（execute_concurrency.rs）**：重启断言 `(1,1)`→`(1,2)`（恢复扫描为故障残留的 dangling run 多落一个终态事件）+ 新增重启后状态断言 `interrupted_needs_attention`（**强化**了 no-fake-success：故障后的 dangling 行不再永久假装 running）。「失败提交零事件/终态事件不发布」等核心断言原文未动。
3. **shutdown_coordinator.rs**：5 处 `graceful_shutdown` 调用补 `&RunSupervisor`（T06 加 `background` 参数之间一先例；断言原文未动，7/0 绿）。
4. A10/其余 R02/R03 回归链：断言原文零改动，全量 601/0 内通过。

## 5. 验证命令与退出码（全部经 rustup 1.98.1 + `--locked`；verify-stage R03 未注册，未运行未伪造）

| 命令 | 退出码 | 结果摘要 |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | 无 diff（开发中首查报 diff 后 `cargo fmt --all` 应用于候选；终查干净——gates.log） |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 0 error/warning（开发中修 useless_concat/doc 缩进/unit let 三类自产告警后全绿） |
| `cargo test --workspace --locked` | 0 | **601 passed / 0 failed / 62 suites**（T06=580/58；+21 = kernel recovery 7 + adapters listing 2 + scan 7 + kill9 2 + exit-race 2 + shutdown intake 1；含 R02 全量回归） |
| `cargo run -p xtask -- check-contracts --locked` | 0 | 56 生成文件 + API_COMPAT_MATRIX 626 entries 零漂移（新 API 均为 Rust 内部面，不进 wire 生成物） |
| `cargo run -p xtask -- check-boundaries --locked` | 0 | O1–O8/DEP-01…09/D5 全 PASS |
| `bash scripts/rust-tauri/r02_t04_storage_tx.sh <probe>`（非门禁探针） | 1 | S1–S3 PASS（含 S2 kill -9 存活语义——重启后 runCount=1，扫描对已终态行零扰动）；S4 仍死于硬编码 version==1（**T04 R1-F1 原样递延 T08**，非新破坏；dump 给出 V1–V4 指纹==编译内、userVersion=4 对账） |

定向过滤器（filters.log，全部命中>0）：recovery_startup_scan 7、recovery_crash_points 2、exit_race_rejections 2、**adapters**::active_run_listing 2（首跑误用 `-p lingxi-service` 0 命中，已用正确 crate 重跑入档——0 命中未计通过，filters.log 内有更正注记）、invocation_journal 3、shutdown_coordinator 7、execute_concurrency 3、kernel `recovery::` 7、kernel `invocation::` 10、service-lib `shutdown::` 5。清理证据：复跑后 `/tmp` 无 `lingxi-r03t07-*` 遗留、无残留测试/子进程、无遗留监听（residue 检查三项均 0）。

## 6. 证据位置

`artifacts/rust-tauri/R03/T07-E01/`：`commands.log`（六命令+退出码+verify-stage 未运行说明）、`gates.log`（fmt+clippy）、`workspace-test.log`（601/0 原始输出 + EXIT=0）、`xtask.log`（contracts+boundaries 全文）、`scan-suite.log`（7 测试 --nocapture + EXIT）、`kill9-suite.log`（矩阵 --nocapture + EXIT）、`exit-race-suite.log`（2 测试 --nocapture + EXIT）、`a13-kill9/`（11 case × {seed,pre,post}.json 共 33 文件——崩溃前后轨迹+重放种子）、`r03-t07-evidence.json`（机器断言：a13 矩阵 11 case 摘要 + a14 退出窗口）、`filters.log`（定向+回归计数）、`r02-script-probe.log` + `r02-script-probe/`（S1–S3 PASS、S4 已知失败 + 指纹对账 dump）、`candidate-summary.txt`（基线/HEAD/逐文件 sha256/Cargo.lock 零变化/迁移指纹/tracked-diff sha256）。

## 7. 测试替身与崩溃注入边界（如实声明）

- **GatedPort（注入式受控故障点）**：测试本地 StoragePort 装饰器——所有写真实落 RunDatabase（单写者队列、每写一事务），仅在指定边界前/后写 marker 并永久停泊（`std::future::pending`，任务级停泊不占 worker 线程）。它不拦截/改写任何数据，只决定**强杀落在哪两次持久化之间**。
- **真实强杀**：`std::process::Child::kill()`（unix=SIGKILL）。子进程=同一测试二进制（真实组合根；T05「生产二进制无 Provider 配置无法承载替身」的差距以此收口——派单明示形态）。崩溃前后库内事实由**独立 rusqlite 句柄**直读（WAL 已提交数据），重启侧经生产 bootstrap。
- **替身边界**：provider/tool 替身只产外部响应与停泊（T05 同界）；外部系统=隔离临时目录文件计数（执行先落盘后返回）。分类、终态、事件、闸门、取消树、drain 预算全部归真实代码。
- **确定性**：通道/文件锚点 + 1ms 轮询有界等待（20s 上限）；并发未被串行化（A14 十路并发真实竞争 admission 闭包）。
- 开发中修过的测试自身缺陷（如实）：exit-race 首版在 registry 就绪前读 run 计数（竞态误判）→ 改为行+drive 双就绪等待；kill9 首版 run_id 查询带多余绑定参数 → 改会话绑定查询。均修正后全绿（缺陷在被测链外的测试编排）。

## 8. 候选输入摘要

基线 `0c138456b…`（=HEAD，无 commit）；修改 10 tracked + 1 doc、新增 7 rust 路径 + 1 支持目录，逐文件 SHA-256 与 tracked-diff sha256（`84346db6…`）见 candidate-summary.txt；`rust/Cargo.lock` 零变化；迁移指纹 V1–V4 与已发布值逐字节一致（零新迁移）。

## 9. 未验证项 / 边界（如实）

- **V2/V3/V4（及本 Task 零新增的 V5）注册登记递延 T08**：`R02-T04_STORAGE_REGISTRY.json` 数组与 `r02_t04_storage_tx.sh` S4 硬编码 version==1 未同步——T04 R1-F1 同根因，按派单指示递延（探针在当前树实测 exit 1 死于同一断言，S4 dump 已含指纹对账事实供 T08 修复）。
- **恢复「重驱动」未实现（有意）**：`recoverable_wait`/`retryable_read_only`/`unknown_side_effect` 三类的自动续跑需要 provider/executor 在扫描期可用——R05 循环经同一 supervisor 接手；R03 扫描将决策数据（journal 分类、resume key、核验指引）落入可解释终态与事件，不假装续跑。T05 A10 的同键 resume 执行路径在生产 wiring 有 executor 前不会被扫描调用（保守默认下该类本来也进 attention）。
- **capability registry 未提前**：`recovery_capabilities` 由调用方注入（测试注入 read-only/key-honored 源已证类别分支）；R04 逐 target 真实分类接入前生产默认 CONSERVATIVE。
- **`queued` 状态无 durable 生产者**：R03 链上 queued 是 wire 词表瞬态（record_run_started 直接落 running），扫描的 queued 腿为机器完备性防御（经 port 层无法构造，未单测——如实登记）。
- **run 级别的 `AlreadyTerminal` 竞态分支**：设计用于非 bootstrap 场景（bootstrap 时无并发 drive）；当前无生产调用方在运行期再触发扫描，该分支由代码路径+load_run 复查保证，无专门集成测试（防御分支，如实登记）。
- **HTTP 503 关闭窗口**测试为单实例内关闭自身 intake（真实 run() 服务 + 真实 TCP）；未做跨进程信号→HTTP 的端到端（二进制接线为同一 `intake.close()` 调用，编译级同一函数；真实信号链归 R09/T08 组合验证）。
- **跨重启去重（SubmissionDedup 进程内存语义）**：T05 §9 递延项未在本 Task 展开（同 requestId 跨重启=全新校验提交，当前语义已如实；持久去重归 R06/R07 入口工作）。
- 无跨平台（本机 arm64 macOS；`Child::kill` 在 Windows=TerminateProcess，语义同构但未在本机验证）；npm/桌面栈未触碰；真实供应商/网络归 R05。

## 10. 独立复核重点建议

1. **A13 的强杀真实性**：recovery_crash_points.rs 父循环——marker 先于 park 写入（父只杀确已停泊在边界的进程）、`Child::kill()`+`wait()`、pre/post 轨迹与 a13-kill9/ 落盘文件逐 case 对照；`external_after` 的外部计数 1→1 是「副作用后崩溃不重复执行」的直接证据。
2. **终态路径唯一性**：service/src/recovery.rs `settle` 与 runs.rs `finalize_settlement` 并排读——同一 `commit_run_outcome` 事务、同一 kernel 裁决；差异仅在事件 reason 富化（行 reason 稳定词表不受影响，A09/矩阵断言锁定）。
3. **A14 拒绝点的位置**：sessions.rs admission 闭包内（busy 闸/分配/写入之前）——重放不经闭包仍被回答的语义在 exit-race 测试有专门断言；HTTP 503 三字段（status/reason/causeId/retryable）在传输层测试逐项断言。
4. **退出序时序**：main.rs 信号 future（close→ws broadcast）与 shutdown.rs 相位 2→3→4→5（含取消→join→residue 报告）对照派单「关提交入口→统一截止→取消/等待→提交关键记录→回收」。
5. **既有断言未弱化**：git diff 逐处核对 §4 三组夹具改动——A09 核心断言（外部计数不变/unknown 判定/NeedsAttention）原文保留；R02-A07 只收紧（新增 interrupted 断言）；shutdown 5 处为参数补齐。
6. **指纹与锁**：独立重算 V1–V4 fingerprint 与已发布值比对；`rust/Cargo.lock` == `90111c4b…`；三个 Cargo.toml 零改动。
7. **601/0 对照**：T06 580 + 21 逐项核对（§5）；R02 回归链（认证/单写者/事件/损坏库备份/关闭/真实重启链）全绿。
