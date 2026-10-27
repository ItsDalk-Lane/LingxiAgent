# R03-T07 独立验收报告（REVIEWER-R03-T07-R01，第 1 轮）

- VERDICT: **PASS**
- TASK_ID: R03-T07（故障恢复与服务退出策略；ACCEPTANCE_IDS：R03-A13、R03-A14）
- TASK_BASE_SHA：`0c138456bbe791f21621f5a24eb47bc1948f7585`（= HEAD，分支 `codex/rust-tauri-migration`）
- 候选：基线 + 未提交工作树（验收期间代码冻结，本审查零产品源码/测试/配置/阶段图改动；复测产物只写 `artifacts/rust-tauri/R03/T07-R01-review/`，构建走 `/tmp/r03-t07-review-target`）
- 审查时间：2026-09-29；环境：macOS darwin 27.0.0 arm64，rustup 锁定 1.98.1，`--locked`、离线
- 输入：派单 `dispatches/R03-T07_REVIEW_R01_DISPATCH.md`；阶段书原文（经 `R03-T07_DISPATCH_E01.md` §Task 规格 + §总控细化转录核对）；`R03_SCOPE_MATRIX.json` / `R03_TEST_MAP.json`；执行者报告 + T07-E01 证据；T01–T06 报告与审查（T05 §10.4 预登义务重点核）；`R02_HANDOFF.json` + `SERVICE_START_AND_SHUTDOWN.md` diff；`git diff 0c138456b -- rust/ docs/` 逐文件读毕（466+/37-，11 tracked + 7 新 rust 路径 + 1 支持目录 + 1 doc）。

## 1. 候选绑定核对

- `git diff 0c138456b -- rust/ docs/` sha256 前 16 位 = `0e0b2144c141977b`，与派单一致；**验收开始与结束各算一次，相同**（审查过程未扰动候选；HEAD 未动，tracked 修改恒为 11 文件）。
- 执行者 candidate-summary 自述 tracked-diff `84346db6…` = `git diff 0c138456b -- rust/`（不含 docs）口径，独立复算一致——两值不矛盾，只是范围口径不同。
- 派单的 status 集摘要 `07f138f77af444a8` 用标准配方（porcelain / `-uall` / 排序 / 仅未跟踪）均不可复现（得 `bf8fe65d…`/`75b566f7…` 等）——判定为派单工具口径差异，非篡改信号：**内容绑定已由更强证据闭合**（candidate-summary 声明的 17 个逐文件 sha256 全部独立重算匹配，0 错；文件集与派单逐项枚举一致，无多无少）。见观察 O-2。
- `rust/Cargo.lock` sha256 = `90111c4b…`（= R02_HANDOFF 原值，零变化）；三个 crate `Cargo.toml`、`rust-toolchain.toml` 零 diff；`migrations.rs` 零 diff；无 `test-util` 引用。

## 2. 核心链路核验（a：RecoveryCoordinator 真实接线）

- **接线位置**：`lingxi-service/src/lib.rs` `bootstrap_with_instance_identity` 在 supervisor/events 构造后、返回 `ServiceState` 前运行 `RecoveryCoordinator::run_startup_scan`；`bootstrap`/`bootstrap_with_deps`/`bootstrap_with_locked_instance` 全部经此链（main.rs 二进制入口用 locked-instance 变体）——生产每进程必经。
- **fail-closed**：扫描错误 → `ServiceStartupError::Storage` → main.rs bootstrap 错误分支 `ExitCode::from(2)`（源码核实）。
- **列举层**：`RunDatabase::list_active_runs`（新 SELECT，既有表零迁移）只取 `queued/running/waiting_approval/cancelling`——**终态在列举层就不可见**（不复活的第一道闸）；未知 status、负 generation、缺 attempt 行均响亮 `Corrupted`（adapters 测试 `a_run_without_attempt_rows_is_loud_corruption` 锁定）。
- **T05 journal 面原样复用**：`recover_run_invocations`（unknown 判定持久化、幂等）+ `RecoveryCapabilitySource`（默认 `ConservativeCapabilities`）——无第二恢复通道。
- **kernel 四态分类**：`lingxi-kernel/src/recovery.rs` `classify_run_recovery` 最坏优先阶梯（NeedsAttention > resume/verify > read-only > 无崩溃窗口条目），纯函数 7 单测锁定词表/阶梯/不虚构；`principal_from_storage_facts` 重建所有权键（未知词表响亮 Corrupted）。
- **单一 finalize**：service `recovery.rs` `settle` 与 `runs.rs` `finalize_settlement` 并排读毕——同一 `StoragePort::commit_run_outcome`（run_store.rs 内核 `RunStateMachine` finalize 裁决在写事务内、含 session/owner 所有权校验，我读实现核实）；差异仅在事件 reason 富化与 event_id（`{run_id}-recovery`），行 reason 保持稳定词表（`interrupted_needs_attention.recovery_unsafe` / `cancelled.requested`，kernel `terminal_reason()` 原值）。
- **cancelling→cancelled**（完成用户已请求的取消）；**queued/waiting_approval** 经合法非终端腿回 `running`（reason 标 `recovery_scan:`，`record_run_state_change` 同受机器边校验）再落终态；Conflict 竞态分支复查 `load_run`，真终态→`AlreadyTerminal` 不重写。
- **不虚构回复**：`RunOutcome.final_message=None` 恒成立；messages 表断言（A09/scan/kill9 三处）。

## 3. R03-A13（运行中重启诚实呈现）— 有效

- `recovery_crash_points.rs`：父测试 spawn 真实子进程（`current_exe --exact crash_child_boundary_harness`，真实组合根 + 真实 RunDatabase + 生产形态替身 + GatedPort），marker **先于** park 写入，父见 marker 后 `Child::kill()`（unix=SIGKILL，非 drop 模拟）+ `wait()` 收割 + `!exited.success()` 断言；重启经生产 `ServiceState::bootstrap`（扫描在内）。
- **GatedPort 边界诚实**：装饰器全部写委托真实 RunDatabase，只决定强杀落在哪两次持久化之间；每 case 的 pre-trace 用**独立 rusqlite 句柄**直读库文件逐项断言崩溃前事实（run 行数/状态/journal phases/外部计数），post-trace 断言重启后事实——即使时序有偏也会被断言抓住。
- **11 边界覆盖任务书要求的全部关键持久化边界**：run 状态迁移（record_run_started）前/后、journal intent 前/后、started 前/后、外部副作用后（含 receipt 前）、receipt 后、终态提交（commit_run_outcome）前/后。
- **本审查独立复跑**（`T07-R01-review/kill9-suite.log`）：2/0 绿，`R03_A13_TRACE cases=11 all_honest=true`；33 个轨迹文件落我方目录；机器证据 11 case 摘要与执行者证据**逐 case 全等**（0 diff）。关键不变量在我方证据中直接复核：`external_after`/`receipt_before` 外部计数强杀前=1、重启后仍=1（不重复执行）；`started_after` 类别=interrupted_needs_attention；`finalize_after` 重启后保持 completed、扫描 scanned=0（终态不复活，其 final_messages=1 是 drive 自己的真实回复）；全部 interrupted case final_messages=0、行 reason 稳定词表、事件 reason 含 `service restarted`+类别+后续动作；`run_started_before` 无 durable 行、扫描无对象（如实非隐藏）。

## 4. R03-A14（退出不接收新任务）— 有效

- **闸**：`SubmissionIntake` AtomicBool 单向（Release/Acquire，幂等 close，单测锁定）；main.rs 信号 future **第一动作** `intake.close()`（先于 ws broadcast 与一切 drain）。
- **拒绝点位置**：`sessions.rs` admission 闭包内首查 `intake.is_closed()`——在 busy 闸、run-id 分配、任何写入之前；前台 `execute_submission_for` 与后台 `execute_background_for` 共享同一 `admit_submission` 链（同链同拒）。
- **重放仍被回答**：dedup 注册表在闭包之前咨询（代码路径核实）——同 requestId+同内容 Replay 不进闭包，返回原 run id；同 requestId 改内容仍是 DuplicateRequestConflict（既有语义不降级）。
- **服务层竞态测试**（复跑 2/0 绿）：已有后台 run 停泊于工具 I/O；关闸后 10 路并发新提交（双会话×前台/后台×plain/显式 id）全 `ShuttingDown`、run 总数不变（零分配零写入）；退出进行中（drain 期间）再 4 路仍全拒——**闸关闭与提交并发的时序证明成立**（ racer 与 `graceful_shutdown` 真并发）。
- **已有任务按策略收束**：退出相位 2.5a 经 supervisor 取消树请求取消（`background_cancel_requested=[run]`）→ drive 自身唯一 finalize 落 `cancelled`（库文件直查 `cancelled.requested`）→ drain 确认（unconfirmed 空）、exit 0。
- **传输层**：真实 `run()` 服务 + 真实 TCP POST 三路并发：关闸前 200；关闸后 503 + `shutting_down` + `service.shutting_down` + `retryable:true` 逐项断言。
- 退出码映射零变化（`exit_code_precedence_is_deterministic` 既有测试仍在，5 处调用仅补 `&runs` 参数，断言原文未动）。

## 5. SERVICE_START_AND_SHUTDOWN.md diff 审读

- 停止链段更新与实现一致：提交入口关闭（第一动作、503 `shutting_down`、重放仍被回答）→ 停收 → WS 广播 → 按任务类型取消/等待（取消树 + 各自唯一 finalize + residue 如实 + durable 行保持 active 交下次扫描）→ flush → 关库 → 清记录 → 释放锁。
- 新增「重启恢复（R03-T07）」段与 recovery.rs 语义一致（含无法分类拒启 exit 2——与 fail-closed 接线核实相符）。
- **R02 退出码表（§4，0–6）逐字未动**；诊断手册 §5 未动；R02 已验收的相位顺序（transport→WS→storage→record）保持，仅插入新首步与任务取消细节——无历史语义篡改。

## 6. 既有测试改动核查（含 T05 §10.4 预登义务）

- **T05 A09（invocation_journal.rs）**：与 T05 报告 §10.4「若复核期引入 T07，此断言需随协调器语义重验」精确对应——外部计数=1 不变的核心断言原文保留；新增扫描报告断言（scanned=1、类别、unknown_verdicts_persisted=1）+ 终态 interrupted_needs_attention + 行 reason 稳定词表 + 无 final message；显式 pass 的 `unknown_verdict_persisted` 由 true 改 false 是**扫描已持久化后的幂等重放**语义（持久化事实断言移至扫描层，未丢失）。义务兑现。
- **R02-A07（execute_concurrency.rs）**：`(1,1)`→`(1,2)` + 新增重启后 `interrupted_needs_attention` 状态断言——协调器语义下的**收紧**（故障残留 dangling 行不再永久假 running），核心断言（失败提交零事件等）原文未动。
- **shutdown_coordinator.rs**：5 处仅补 `&RunSupervisor` 参数（`without_provider()`），断言零改动。
- 全部改动为收紧/字面补齐，无删除、无放宽、无改永真、无 skip。

## 7. 测试质量与环境关注

- 无 mock 待测核心：分类/终态/事件/闸/取消树/drain 全真实代码；替身只产外部响应与停泊（T05/T06 同界）；崩溃子进程=真实组合根。
- 无空集合断言、无忽略断言、无 `#[ignore]`。
- **残留检查（本审查复跑后实测）**：`/tmp` 无 `lingxi-r03t07-*` 遗留（0）、无残留测试/子进程、无遗留监听。崩溃子进程在测试内 kill+wait 收割；临时 home/external 目录逐 case 清理；测试家目录以 pid+nanos 隔离，无端口绑定（bootstrap 不绑监听，仅 `run()` 绑定且 HTTP 测试正常停机）。

## 8. 独立复跑结果（`artifacts/rust-tauri/R03/T07-R01-review/`）

| 命令（rustup 1.98.1 + --locked） | 退出码 | 结果 |
|---|---|---|
| `cargo test --workspace --locked` | 0 | **601 passed / 0 failed / 62 suites**（= T06 580 + 21 新增，与执行者报告一致） |
| 定向 `recovery_crash_points`（含 TRACE/EVIDENCE 捕获） | 0 | 2/0，11 case 全诚实，33 轨迹 |
| 定向 `recovery_startup_scan` / `exit_race_rejections` | 0 | 7/0、2/0 |
| 定向 `active_run_listing`（-p lingxi-adapters） | 0 | 2/0 |
| 定向 `shutdown_coordinator` / `invocation_journal` / `execute_concurrency` | 0 | 7/0、3/0、3/0 |
| 定向 kernel `recovery::` | 0 | 7/0 |
| `cargo fmt --all -- --check` | 0 | 无 diff |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 0 告警 |
| `cargo run -p xtask -- check-contracts --locked` | 0 | drift-free |
| `cargo run -p xtask -- check-boundaries --locked` | 0 | 全 PASS |

全部定向命中 > 0（执行者 filters.log 中 adapters 套件首跑 0 命中的更正注记属实且未计入通过）。verify-stage R03 未运行未伪造（T08 项，正确）。

## 9. 范围核对

- **T08 未提前**：无 stage_maps/R03.json 注册；`r02_t04_storage_tx.sh` 与 `R02-T04_STORAGE_REGISTRY.json` 零 diff（S4 仍死于 T04 R1-F1 硬编码 version==1——执行者探针日志证实其如实递延，S4 dump 附 V1–V4 指纹+userVersion=4 对账）。
- **R04+/R05 未提前**：恢复重驱动未实现（有意，登记 §9）；`recovery_capabilities` 为注入接口（默认 CONSERVATIVE），无注册表提前。
- **零新迁移**：V1–V4 指纹独立重算 = `479b0321`/`64d7edfd`/`3bd5388f`/`371d4154`（与已发布值及盘上收据一致）；`list_active_runs` 为既有表 SELECT。
- Cargo/lock/工具链零变化；无 tokio test-util；修改面仅 rust 7 源文件 + 3 测试文件 + 1 R02 操作文档。

## 10. 执行者报告勘误

未发现失实陈述。报告 §3–§7 的自述与我方复跑/源码核对全部相符（含 601/0、21 新增计数分解、kill9 与 exit-race 轨迹、filters 更正、§9 边界如实登记）。

## 11. 建议性观察（非阻塞，不构成 FAIL 条件）

- **O-1**（shutdown.rs 相位 2.5a）：`background_cancel_requested` 在 `cancel_run` 未成功 fire 时（FireOutcome 非成功）也推入 run_id，字段名轻微过称（fire 结果只进日志）。残留诚实性不受影响（`background_unconfirmed` 才是权威残留字段）；建议后续把未成功 fire 的条目区分标注。
- **O-2**（派单口径）：review 派单的 status 集摘要 `07f138f77af444a8` 无法用标准 `git status` 配方复现；本次以 tracked-diff sha256 + 17 个逐文件 sha256 + 文件集枚举完成内容绑定。建议派单工具注明其摘要配方，避免后续轮次误判。
- **O-3**（设计口径，已披露）：三个「可恢复」类别在 R03 均落 `interrupted_needs_attention` 终态（类别作为数据存于事件 reason 与扫描报告，重驱动归 R05）。这与「不假装续跑/不虚构进度」的诚实规则一致且报告 §9 如实登记；R05 接手时应消费这些已落盘的决策数据（resume key/核验指引），届时需防类别信息只活在事件文本里不可机读的退化。
- **O-4**（测试纵深）：`background_cancel_requested` 的取消请求落在 drive 的 await 点这一点由 exit-race 测试的 cancelled 终态间接证明；未单独构造「取消请求后 drive 仍在预算内完成收尾但 drain 超时」的 unconfirmed→下进程扫描闭环集成用例（`exit_race_rejections` 有断言路径、kill9 覆盖重启侧，但两者未在一个用例里串成单一叙事）。T06 遗留的该闭环逻辑上已被两半覆盖。
- **O-5**（已披露）：跨进程真实信号→HTTP 503 的端到端未测（二进制接线与测试为编译级同一 `intake.close()` 调用）；归 R09/T08 组合验证。

## 12. 结论

R03-T07 到期义务（阶段书怎么做 1–4、必须交付三件 RecoveryCoordinator/恢复分类表/崩溃点测试集、A13、A14、总控细化全部条目、T05 §10.4 预登重验义务、T06 unconfirmed 递延闭环）均有有效证据且真实接线成立；回归满足（601/0 + 四门禁全绿，全部由本审查独立复跑）；范围未越界（T08/R04/R05 未提前、零迁移、Cargo 零变化）；环境干净（无残留进程/监听//tmp）。未发现未关闭的验收阻塞缺陷。

**VERDICT: PASS**
