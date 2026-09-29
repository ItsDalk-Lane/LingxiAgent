# R03-T05 独立验收报告（REVIEWER-R03-T05-R01，第 1 轮）

- VERDICT: **PASS**（无本 Task 验收阻塞缺陷；V3 迁移登记递延 R03-T08 = T04 F-1 同根因既定递延，非新破坏；另有 5 项建议性观察 O-1..O-5，不构成义务缺口）
- TASK_ID: R03-T05（运行日志与副作用收据；ACCEPTANCE_IDS R03-A09 / R03-A10）
- 审查者：REVIEWER-R03-T05-R01（全新一次性验收代理；未参与本 Task 的实现/修复/派单）
- 候选：TASK_BASE_SHA `0bf067d9ec329108408f04b00f37f2635fc4a55d`（分支 codex/rust-tauri-migration，=HEAD，无 commit）+ 未提交工作树
- 审查时间：2026-09-29；环境 macOS darwin 27.0.0 arm64；rustup 锁定 cargo/rustc 1.98.1（与执行者一致）；复测产物目录 `artifacts/rust-tauri/R03/T05-R01-review/`（审查期间未修改任何产品源码/测试/配置/阶段图）

## 0. 独立验收执行方式

- 逐文件读 `git diff 0bf067d9e -- rust/` 全量（修改 7 + 新增 4，共 1140 插入/8 删除），并读四个新增文件全文与 runs.rs 现行关键区段（写序逐行核到行号，不止 diff 上下文）。
- 独立重算 V1/V2/V3 迁移 SQL 指纹、逐文件 SHA-256、Cargo 锁/TOML 哈希，对账 R02-T04_STORAGE_REGISTRY.json 与 R02_HANDOFF.json。
- 真实复跑全部四门禁 + 定向三件（kernel/adapters/service）+ A09/A10/生命周期三测试 --nocapture 独立复现 TRACE + workspace 全量 558/0 + verify-stage R03 硬拒探针 + r02_t04_storage_tx.sh 探针（复现 T04 F-1 边界）。
- 结论区分：源码推断 / 实际运行 / 受环境限制，逐条标注。本机 arm64 macOS 单平台；无真实外部系统（A09/A10 为派单允许的受控本地文件替身）。

## 1. 候选绑定核对（前后两次）

| 项 | 结果 |
|---|---|
| tracked-diff sha256 前 16（复跑前） | `e78df59386ebe190` = 派单绑定值 ✓ |
| tracked-diff sha256 前 16（复跑后） | `e78df59386ebe190`（审查期间代码冻结成立）✓ |
| status 集 sha256 前 16 | `34423e3b4fa70cf6` ✓（剔除绑定后写入的审查派单文件后与绑定一致；复测产物 .log 被仓库既有 `*.log` 忽略规则遮蔽，不进入 status 集——与 T04 审查同一形态） |
| 逐文件 sha256（11 项） | 修改 7 + 新增 4 全部与 `T05-E01/candidate-summary.txt` 逐字节一致 ✓（本审查独立计算） |
| Cargo 零变化 | `rust/Cargo.lock` `90111c4b…` = R02_HANDOFF 发布值 ✓；root + 三 crate Cargo.toml 哈希与 candidate-summary「untouched contracts」逐字节一致 ✓ |

## 2. 到期义务逐项复核（阶段书怎么做 1–4 + 三交付物 + A09 + A10 + 总控细化）

### 2.1 六态收据与全量绑定（怎么做 1）——真实接线成立（源码逐行核实 + 实际运行）

- **词表**：kernel `InvocationPhase` 六态 `prepared/authorized/started/succeeded/failed/unknown`（`wire_name` 稳定、`parse` 拒未知值），`is_closed` 刻意不含 unknown（可被 VERIFIED 回执升级）。
- **绑定**：V3 表 `invocation_journal` 18 列全绑定（journal_id=驱动方 mint 的 ToolCallId、session/run/attempt/generation、owner_kind/owner_subject、target、args_digest/args_summary、idempotency_key、phase、回执四列、双时间戳）；`InvocationJournalEntry` 读视图同构。journal_id 复用驱动方 ToolCallId——provider 无法伪造调用身份（身份仍由驱动方 mint，与 T04 栅栏同源）。
- **幂等键**＝调用身份本身（`{call_id}`，跨重启稳定）；「外部是否受证该键」是 per-tool capability（`ToolRecoveryCapability`），本层不声称——契约切分正确。`idempotency_key` 部分唯一索引机械防两调用共用一键（V3 DDL）。
- **实测**：`journal_lifecycle…` 三调用全量绑定断言（journal_id/attempt `{run}#a1`/generation=1/owner/target/digest/键）全绿；我的独立复跑 TRACE `entries=3 phases=succeeded,failed,failed(rejected) external_executions=2` 与执行者证据逐字一致。

### 2.2 写序即契约（怎么做 2）——源码级确认，非测试内假装

驱动链（`rust/crates/lingxi-service/src/runs.rs`）逐行核实：

1. `record_invocation_intent(prepared)`（:809）——配额准入后、**一切事件之前**；写失败 `map_err(DriveError::Storage)?` 响亮失败，**绝不无收据外发**；
2. `persist tool_call_started`（:822，T01 事件未动）；
3. 无审批门→`advance(Authorized)`（:831）；有门→Approved 后 `advance(Authorized)`（:924）；Rejected/Aborted→`record_receipt(Failed, dispatched=false, "not dispatched: …")`（:954）后走既有失败事件（零执行语义保留，外部计数实测不含被拒调用）；
4. `advance(Started)`（:988）——**在 `spawn_linked` 工具子任务（:1002）之前**，即外部派发前最后一笔独立事务落盘；
5. 外部执行（select 等待 :1013-1061）→ T04 fence 判定（:1067）→ `record_invocation_receipt`（:1119）——**在 `persist tool_call_completed` 流事件（:1127）之前**；
6. 全部 journal 写走 `StoragePort` 五方法，每方法一 job 一 `with_write_txn`（BEGIN IMMEDIATE+COMMIT）——intent/started/receipt 各自独立事务。

「不声称跨系统原子事务」：ports.rs 契约注释、V3 表注释、`journal_receipt_of` 注释三处明示。A09 测试在 kill **前**用 DB 直查见证 `phase=started、receipt.is_none()`——写序的活链见证而非仅绿灯（该断言若写序不实必红）。

### 2.3 恢复分类四态 + 六决策（怎么做 3/4）

kernel `invocation.rs`（纯函数，无 I/O 无时钟）：

- `invocation_recovery_class`：prepared/authorized→未执行；started 无回执→**Unknown**；succeeded/failed→确认完成/失败；unknown→Unknown（先前诚实判定）。
- `classify_invocation_recovery` 六决策：SafeToReexecute（未执行，有界自动重执安全）／ConfirmedSettled（已收束，dedup 随行）／UnknownReadOnlyReexecute（只读）／UnknownResumeWithIdempotencyKey（受证实幂等+journal 有键→**同键**恢复）／UnknownVerifyExternally（可核验→先核验）／NeedsAttention（其余，reason 含 target 与「blind retry could duplicate the side effect」）。
- **决策序 read_only > key > verify > attention 为契约**（单测 `read_only_takes_precedence…` 锁定）；**有能力无 journal 键不可 resume**（`honored_capability_without_a_journaled_key_cannot_resume`——防「能力存在即自动恢复」越权放宽）；`CONSERVATIVE` 三项全闭为唯一默认（未证实=不自动恢复）。**非幂等 unknown 禁止盲重试**由 NeedsAttention 唯一收口，A09 实测（计数恢复后不增）。
- 服务面 `invocations.rs`：`classify_run_invocations`（纯读）+ `recover_run_invocations`（读 journal→逐条分类→仅对 started-无回执持久化 unknown 判定；重跑幂等 `unknown_verdict_persisted=false`）。不重执行任何工具、不写 run 状态、无定时/扫描——**未接入任何启动路径**（全仓 grep：仅测试调用）＝ T07 RecoveryCoordinator 的决策+数据面，未提前实现 T07。

### 2.4 Migration V3 纯增量、V1/V2 逐字节不动（独立重算）

- diff 确认 migrations.rs 仅追加 `V3_NAME/V3_SQL` 常量与 MIGRATIONS 第三条目；无既有条目改动。
- 独立重算（python 对源文件提取 SQL 文本后 sha256）：
  - V1 `479b0321494269fca85d9f973b01a8f9d1aa57dc08fc3cf8ddbafeace461bb8` = R02-T04_STORAGE_REGISTRY 已发布值**逐字节一致** ✓
  - V2 `64d7edfdab74e13e623a9d8d5f2381dad4803f545fa126d8eda8cc02c3dc889c` = T04 审查发布值 ✓
  - V3 `3bd5388f090ab0ae0e137d91d59a8e48f82f409bbd0de657abf29b897a4252c1`（新）= candidate-summary ✓
- 盘上对账：我复跑 `r02_t04_storage_tx.sh` 探针，S4 dump 显示真实迁移库 receipts==compiledIn 三指纹逐字节、userVersion==3（探针日志 `T05-R01-review/r02-script-probe-rerun.log`）。
- `supported_version()` 取 last()=3，migration_idempotency/防篡改/拒降级面自动纳入 V3（T04 已区间化），2/2 绿。
- **V3 登记递延 T08**：`R02-T04_STORAGE_REGISTRY.json` migrations 数组仍只有 V1、r02 脚本 S4 仍硬编码 `version==1`——与 T04 审查 R1-F1 **同根因**（该脚本在 T04/V2 基线上就 exit 1，T04 审查自有探针为证；本候选 V3 未引入新破坏，我的探针复现 S1–S3 PASS、S4 同一断言失败 exit 1）。按派单指示一并递延 T08，非本 Task 义务。

### 2.5 unknown 结果结构

- **Cancelled→Unknown 诚实映射**：`journal_receipt_of`（runs.rs）——「停止等待≠外部未完成，不伪造失败」，detail `executor stopped waiting (cancelled); external outcome unobserved`；单测 `journal_receipt_maps_outcomes_onto_the_durable_receipt` 四分支锁定；fence 判为 stale 的工具结果同样落 Unknown（T04 词汇表一致，不伪成功不盲重试）。
- **unknown 可被 VERIFIED 回执升级**：存储规则 `succeeded` 仅自 `started`/`unknown` 合法——unknown 占位（recovery 判定或未观测结果）可被核实回执收束。adapter 测试 `journal_recovery_classes…` 实测 unknown→succeeded 收束 + 分类升为 ConfirmedCompleted；A10 活链实测同一形态。
- `record_invocation_unknown` 恢复面关闭：仅 started-无回执合法（幂等重跑）；intent-only/closed 一律 InvalidRequest 响亮；条目自身绑定身份即权威（不写新身份事实）。三向误用拒绝实测。

### 2.6 R03-A09 副作用后崩溃不重复执行 — **PASS**

- 复核 `r03_a09_crash_after_side_effect_does_not_reexecute_and_receipt_is_unknown` 全文并独立复跑（TRACE 逐字复现：`external_executions=1 journal=started→unknown decision=needs_attention run_row=running`）。
- 链路：非幂等文件替身（notify.double，独立临时目录 requests.log，**执行先落盘后返回**——外部效应在客户端死亡后幸存）执行 1 次后永久停靠 → kill 前直查 journal 恰 1 行 started 无回执（写序活链见证）→ `JoinHandle::abort()` 真实取消原语（RegistrationGuard 触发取消树、bounded-wait 见证注销、durable run 行保持 running 不伪造终态）→ `close()` 排空 FIFO（停靠点在工具 await，回执 job 从未入队——不是被跳过的提交）→ drop → 全新 `ServiceState::bootstrap`（生产形态、无替身配置）同 data root。
- 断言：重启后**外部计数=1 不变**（无协调器即无重发路径，当前即诚实行为；断言的是「不再自动增加」）→ 恢复面 NeedsAttention（非幂等 unknown 禁盲重试）+ unknown 判定持久化 → DB 直查 phase=unknown、receipt_outcome=unknown、计数仍=1、请求仍=1。证据=外部计数器（requests.log）+恢复库（journal 行），符合任务书 A09 证据要求。
- **kill 形态判断（派单要求明示）**：派单总控细化原文明确允许「同进程 drop+reopen 库模拟崩溃边界」且硬性要求两点均满足——①真实持久化边界（journal intent/started 先于外部执行各自事务落盘、回执后落盘：源码级+kill 前直查见证）；②外部计数器为受控本地替身独立文件（成立）。进程级 kill -9 的全链崩溃点测试集在阶段书属 T07（怎么做 4「使用可控崩溃点测试每个关键持久化边界」+ A13 前置「强制结束进程」），非本 Task 义务；且进程级持久化存活语义在 R02-T04 S2（kill -9 真实服务进程，本轮探针复现 PASS）已有覆盖。执行者如实披露差距并归 T07。**结论：当前证据等级足以 PASS A09**；附带条件——T07 引入 RecoveryCoordinator 后「重启不自动再执行」断言须随协调器语义重验（执行者已自行标注，见 O-1）。

### 2.7 R03-A10 可验证幂等恢复 — **PASS**

- 复核 `r03_a10_idempotent_key_resume_does_not_duplicate_the_external_operation` 全文并独立复跑（TRACE 复现：`key={run}-tc0001 requests=2 executed=1 decision=unknown_resume_with_idempotency_key phase=succeeded`）。
- 幂等替身（ledger.double）state.json 键→结果 + 请求日志每行记 key/executed/digest；键=journal 幂等键=调用身份（跨重启稳定）。响应前中断（停靠+abort+close+drop，同 A09 形态）→ 重启 → LedgerCapabilities 判定受证实幂等 → 决策 **UnknownResumeWithIdempotencyKey{key=原键同一}**（断言 `resume_key == key`）→ unknown 判定先持久化 → 以 journal 条目重建恢复上下文（owner/run/attempt/generation + LocalUser 主体）经**同一 ToolExecutorPort** 同 call id 再调用 → 替身按键去重：**请求=2、执行=1**、返回原记录结果 → 核验 dedup 响应==state.json 记录（实测相等）→ `record_receipt(Succeeded, dedup_id=key)` 经 unknown→succeeded VERIFIED 升级收束 → journal 终态 succeeded+dedup_id、外部执行恒=1。
- 恢复执行由测试按决策驱动（T07 生产驱动方），被测对象=决策正确性+journal 收束+外部不重复——与派单「本 Task 交付分类决策与收据数据面」一致，披露如实。

### 2.8 必须交付三件

| 交付物 | 落点 | 判定 |
|---|---|---|
| InvocationJournal | V3 表 + StoragePort 五方法（真实 adapter 单写者队列一写一事务）+ 驱动链接线 + FakePort 两处 trait 满足 | ✓ 真实接线（存储契约 5 测试对真实 RunDatabase） |
| 副作用恢复策略 | kernel `classify_invocation_recovery` 四态/六决策 + 服务面 classify/recover | ✓（10 kernel 单测 + adapter 接缝 + A09/A10 实测） |
| unknown 结果结构 | `ReceiptOutcome::Unknown` + `InvocationReceipt{dispatched,dedup_id,detail}` + `InvocationPhase::Unknown` 可升级 | ✓（含 Cancelled 诚实映射） |

## 3. 测试有效性与卫生

- **无 mock 待测核心**：kernel 测试纯函数；adapter `invocation_journal_store.rs` 5 测试全部打真实 RunDatabase（迁移 V3+单写者队列+事务）；service `invocation_journal.rs` 3 测试走真实 ServiceState（真实存储/事件/kernel 状态机/单一 finalize/真实驱动写序）。替身仅为 Provider/Tool 外部协议与文件型外部系统——任务书测试替身边界允许项。
- **无空集合/永真/忽略断言**：grep 证实新文件无 `#[ignore]`、无 is_empty 兜底、无恒真断言；`load_invocation_journal` 的 FakePort 空实现仅 trait 满足（耐久行为由真实 adapter 测试覆盖），且 kernel FakePort 反而加了两道规则门（advance 目标门、succeeded-须-dispatched 门）。
- **过滤器命中**：执行者自报三组初始过滤器串 0 命中已纠正、未计为通过——复核属实且最终命中数与我的复跑一致：kernel invocation 10、adapters store 5、service journal 3、r03_a09 1、r03_a10 1、runs `journal_receipt` 1、`fence_verdict` 1，全部 >0。
- **既有测试改动逐处判断**：仅两处 FakePort 补五方法（kernel ports.rs 测试、sessions.rs 测试）+ runs.rs 新增 1 单测——无断言删除/放宽/改永真，无 skipped；T04 `fence_verdict`、T01–T03 既有 suites 原样绿（workspace 558/0 内含）。

## 4. 实际复跑（全部经 rustup 1.98.1 + `--locked`；原始输出与退出码见 `T05-R01-review/commands-rerun.log`）

| 命令 | 退出码 | 结果 |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | 无 diff |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 0 error/warning |
| `cargo test --workspace --locked` | 0 | **558 passed / 0 failed / 0 ignored / 54 suites**（=执行者值；T04=539，+19 = kernel invocation 10 + adapters store 5 + service journal 3 + runs 单测 1） |
| `cargo run -p xtask --locked -- check-contracts` | 0 | API_COMPAT_MATRIX 626 entries 零漂移 |
| `cargo run -p xtask --locked -- check-boundaries` | 0 | DEP-07/08/D5 全 PASS |
| 定向：kernel `invocation` / adapters `invocation_journal_store` / service `invocation_journal`（--test-threads=1）/ `r03_a09` / `r03_a10` | 0 | 10 / 5 / 3 / 1 / 1 全绿，命中>0 |
| 三测试 --nocapture 独立 TRACE 复现 | 0 | A09/A10/lifecycle 三 TRACE 与执行者证据逐字一致（a09-a10-rerun-full.log） |
| `cargo run -p xtask -- verify-stage R03 --evidence <probe>`（探针） | **2** | 硬拒 `unknown stage "R03"`（未注册未伪造 ✓；R03.json 归 T08/R03-SUP-01） |
| `bash scripts/rust-tauri/r02_t04_storage_tx.sh <probe>`（非门禁探针） | **1** | S1–S3 PASS（含真实 kill -9 崩溃恢复 S2）；S4 死于硬编码 version==1（T04 F-1 原样）；S4 dump 提供 V3 盘上收据==编译内指纹对账 |

复跑后无 `/tmp/lingxi-r03t05-*` 遗留（探针目录已清理）、无残留进程；审查期间 tracked-diff 摘要不变（代码冻结成立）。

## 5. 范围核对（不越界）

- **T06 未提前**：无 subagent/后台/权限继承内容。
- **T07 未提前**：无 RecoveryCoordinator/启动扫描/退出策略；`recover_run_invocations` 无任何生产调用点（全仓 grep 仅测试）；跨重启去重、`interrupted` 用户面如实递延。
- **R04 未提前**：无 capability registry/工具网关；`RecoveryCapabilitySource` 为接口 + CONSERVATIVE 默认（调用方供给），模块头明示 R04 消费。
- **generation 真实语义未提前**：恒 1 如实记录（T04 O-4 同源，绑定结构与断言已就位，R04 接入）。
- **task_supervisor.rs 零触碰**（diff 为空）→ T03 R1-D1 按派单条件继续递延，前提成立。
- **Cargo 零变化**（§1）；**无 tokio test-util**（grep 仅命中一条注释）；npm/桌面栈零触碰（status 集仅 rust/+docs/artifacts）。
- **V3 登记递延 T08** 与 T04 F-1 同根因（§2.4，基线上即 exit 1 的既有失败），非新破坏。

## 6. 执行者报告准确性

逐项对账：候选摘要/逐文件 hash/Cargo 零变化/迁移指纹三值、558/0=539+19、全部命令退出码与命中数、三组过滤器 0 命中的如实披露、A09/A10/lifecycle 证据 JSON 内容、F-1 递延与 r02 脚本边界、未验证项清单（§9：T07 协调、capability registry、generation、审批链两事实面、kill 形态差距、单平台）——**与源码/复跑事实一致，未发现虚报或报告盲区**（T04 轮的 registry 盲区本轮已主动披露）。开发中三处测试自身缺陷的自述与最终测试形态自洽。

## 7. 建议性观察（非缺陷，不影响本 Task 验收）

- **O-1（T07 绑定）**：A09「重启不自动再执行」当前由「无协调器即无重发路径」+恢复面只分类不执行保证；T07 引入 RecoveryCoordinator 后该不变式须随协调器语义重新断言（执行者已自标）。进程级 kill 的 journal 边界崩溃点测试归 T07 崩溃点测试集（阶段书 T07 怎么做 4）。
- **O-2（T08 绑定）**：V3 未登记 registry/r02 脚本（=T04 F-1 同根因递延）；T08 修复时须同步登记 V2+V3 两版本并重跑 `r02_t04_storage_tx.sh`（S4 断言应改为编译内等价）+ `verify-stage R02` 链。
- **O-3**：`record_invocation_receipt` 允许自 intent-only 相位记 Unknown/Failed(dispatched=true)——当前驱动链不可达（unknown 回执仅产生于派发后），属宽容 API 面；R04 网关接管时可选收紧。
- **O-4**：`record_invocation_unknown` 固定写 `dispatched=1`——正确性依赖「started 先于派发」写序契约（本 Task 已成立并锁测）；若未来驱动形态变化需同步重验。
- **O-5**：`journal_entry_from_row` 将 dispatched 非零一律读为 true（仅 0 为 false）——与写入侧 bool 映射一致，负值场景仅在外部篡改时出现且仍非猜测值；cosmetic。

## 8. 结论

R03-T05 到期义务（阶段书怎么做 1–4、三交付物 InvocationJournal/副作用恢复策略/unknown 结果结构、A09、A10、总控细化全部条目）均有有效证据且真实接线成立（端口契约→存储 V3 单写者事务→驱动链写序→恢复分类决策，四层贯通；写序为源码级确认并获 kill 前直查与独立复跑双重见证）；回归满足（workspace 558/0 = T04 539 + 19，R02 全量链绿，四门禁 exit 0，全部由本审查者独立复跑并逐键复现）；无越界实现（T06/T07/R04 未提前，task_supervisor 零触碰故 T03 R1-D1 继续递延，Cargo 零变化，无 test-util）；V3 登记递延 T08 为 T04 F-1 既定同根因递延且脚本失败在基线上即存在，非本 Task 新破坏。无未关闭验收阻塞缺陷。**允许 PASS。**
