# R03-T05 报告｜运行日志与副作用收据（EXECUTOR-R03-T05-E01）

- 状态：**READY_FOR_REVIEW**（执行者口径；独立复核归属总控另派）
- TASK_ID：R03-T05（ACCEPTANCE_IDS：R03-A09、R03-A10）
- TASK_BASE_SHA：`0bf067d9ec329108408f04b00f37f2635fc4a55d`（分支 `codex/rust-tauri-migration`，无 commit/push；工作树候选留给总控冻结）
- 执行时间：2026-09-29（证据时间见 candidate-summary.txt / 各 log）
- 环境：macOS darwin 27.0.0 arm64；rustup 锁定工具链 **1.98.1**（全部命令经 `~/.cargo/bin` rustup 代理 + `--locked`；证据链统一 `CARGO_TARGET_DIR=/tmp/r03-t05-target`）；SQLite=rusqlite 0.40.2 bundled；无网络外发、无真实供应商；npm/桌面栈零触碰；**tokio test-util feature 未引入**（确定性方案：单线程 current_thread runtime + 通道锚点 + 门控替身 + 1ms 轮询/500ms 硬上限的有界真实等待——T02–T04 同一先例）
- 交付物三件：**InvocationJournal**（kernel 收据契约 + V3 迁移表 + 单写者队列上的五个端口方法 + 驱动链真实写序）、**副作用恢复策略**（kernel 纯分类决策 `classify_invocation_recovery` 四态/六决策 + 服务面 `recover_run_invocations`）、**unknown 结果结构**（`ReceiptOutcome::Unknown` + `InvocationReceipt{dispatched,dedup_id,detail}` + 恢复态 `InvocationPhase::Unknown` 的可升级收束）

---

## 1. 冻结语义溯源（先读规格与现状，再映射——不发明交互）

阶段书 R03-T05 原文 4 步 + 总控细化（派单 §Task 规格/§总控细化）与现状对照：

| 冻结语义 | 规格出处 | Rust R03-T05 映射 |
|---|---|---|
| **prepared→authorized→started→succeeded/failed/unknown 收据，与参数摘要和目标代次（run/attempt/generation）绑定** | 阶段书怎么做 1；总控细化「绑定主体(owner)、run/attempt/generation、目标、参数摘要、幂等键」 | kernel `InvocationPhase` 六态词表（wire_name 稳定，parse 拒未知值）；`InvocationJournalEntry` 全量绑定 owner_kind/owner_subject/session/run/attempt/generation/target/args_digest/args_summary/idempotency_key + phase + receipt。V3 表 `invocation_journal` 一行一调用（journal_id=驱动方 mint 的 ToolCallId，杜绝 provider 伪造身份）；幂等键=调用身份本身（`{call_id}`，跨重启稳定），外部是否受证为 per-tool capability 而非本层声明 |
| **副作用执行前持久化开始意图；执行后持久化外部响应/可用去重标识；不声称跨外部系统原子事务** | 阶段书怎么做 2 | 驱动链写序（runs.rs 生产链）：`record_invocation_intent(prepared)`（配额准入后、一切事件之前）→ `persist tool_call_started`（既有）→ authorized（无审批门=运行层授权上下文直授；有门=Approved 后）→ `advance_invocation(started)`（**外部执行派发前最后一笔落盘**）→ 外部执行 → `record_invocation_receipt`（外部响应摘要/去重标识=content digest 或幂等键）→ `persist tool_call_completed`（既有）。意图写失败=响亮 DriveError::Storage，**绝不无收据外发**；回执写失败=响亮失败（外部已执行、本地收据缺失——恢复读 started 即 unknown）。全部注释与 port 契约明示「无跨系统原子事务」 |
| **恢复时把「已执行但回执未持久化」归为 unknown；按工具幂等性和外部查询能力决定核验、重试或人工确认** | 阶段书怎么做 3；总控细化四态分类 | kernel `invocation_recovery_class` 四态（未执行=prepared/authorized；确认完成=succeeded；确认失败=failed 含 dispatched 区分；unknown=started 无回执/已标 unknown）+ `classify_invocation_recovery(entry, capability)` 六决策：SafeToReexecute（未执行）／ConfirmedSettled（已收束）／UnknownReadOnlyReexecute（只读）／UnknownResumeWithIdempotencyKey（受证实幂等→同键恢复）／UnknownVerifyExternally（可核验→先核验）／NeedsAttention（其余：禁止盲重试，可解释 reason）。决策序 read_only > key > verify > attention 为契约（单测锁定）。服务面 `recover_run_invocations`（读 journal→分类→**持久化 unknown 判定**，幂等重跑不重写）；`ToolRecoveryCapability::CONSERVATIVE` 为唯一默认（未证实工具一律不自动恢复——红线对齐） |
| **只读或受证实幂等任务允许有界自动恢复；发送消息/付款/破坏写等未知结果禁止盲重试** | 阶段书怎么做 4 | 同上六决策：自动恢复仅两通道（read_only；honors_idempotency_key 且 journal 有键）；非幂等 unknown → NeedsAttention（reason 明示「blind retry could duplicate the side effect」）。A09 实测非幂等替身计数器恢复后不再增长；A10 实测同键恢复外部不重复执行 |
| **完整启动扫描恢复协调归 T07；本 Task 交付分类决策函数与收据数据面** | 总控细化 | `invocations.rs` 为纯决策+数据面（不重执行任何工具、不复活 run、无第二调度器）；T07 的 RecoveryCoordinator 与 R04 的 capability registry 是消费方（模块头明示）。capability 来源接口 `RecoveryCapabilitySource`（默认 ConservativeCapabilities） |

**A09/A10 kill-重启形态（总控细化原文）**：「journal 意图先落盘、外部已执行、回执未落盘 → 重启后计数不自动再增、收据 unknown」——本 Task 用派单允许的同进程 drop+reopen 形态真实复刻该持久化边界（§7 详述边界与理由）；「幂等键场景恢复后同一外部操作不重复、核验收束」——A10 全程实测。

## 2. 实现与调用链（真实接线）

### kernel（lingxi-kernel）

- **`src/invocation.rs`（新）**：`InvocationPhase`（ports 定义、此处消费）、`RecoveryClass`、`RecoveryDecision`（六决策 + `name()` 稳定词表 + `journal_id()`）、`ToolRecoveryCapability{read_only,honors_idempotency_key,externally_verifiable}` + `CONSERVATIVE`、`invocation_recovery_class`、`classify_invocation_recovery`。纯函数、无 I/O 无时钟——恢复决策是崩溃幸存事实的函数。10 个单测覆盖四态、决策阶梯（含 read_only 优先序、有键能力无 journal 键不可 resume、词表稳定）。
- **`src/ports.rs`**：收据负载类型 `InvocationIntent`/`ReceiptOutcome`/`InvocationReceipt`/`InvocationJournalEntry`；`StoragePort` 新五方法（契约见下）；FakePort 补最小实现（advance 目标门 + succeeded-须-dispatched 门——trait 层规则；耐久行为全部由真实 adapter 测试覆盖）。
- `src/lib.rs`：`pub mod invocation;`（一行）。

### StoragePort 新五方法（写序即契约）

```
record_invocation_intent(ctx, intent, now)     INSERT phase='prepared'；同 id 同绑定幂等 replay，冲突 Conflict
advance_invocation(ctx, id, Authorized|Started, now)  阶梯有序（prepared→authorized→started；
                                               同相重推进=no-op；越级/回退=InvalidRequest）
record_invocation_receipt(ctx, id, receipt, now)  关闭：succeeded 仅自 started/unknown（成功必须被
                                               「已派发」见证）；failed 自任意未关闭相（含审批拒绝
                                               dispatched=false）；closed 后仅完全同回执 replay，异则 Conflict；
                                               unknown 占位可被 VERIFIED 回执升级收束（A10 形态）
record_invocation_unknown(id, detail, now)     恢复面关闭：仅 started-无回执合法（幂等重跑）；
                                               条目自身绑定的身份即权威（不写新身份事实）
load_invocation_journal(run_id)                恢复分类读（rowid 序=开序）；行解析对未知词表/半写回执/
                                               负数列一律 Corrupted 响亮（不猜测）
```

### 存储层（lingxi-adapters）

- **Migration V3 `invocation_journal`**（追加式，fingerprint 机制原样）：18 列全绑定 + `idx_invocation_journal_run` + `idempotency_key` 部分唯一索引（机械防两调用共用一键）。FK→runs 有意保留（journal 由活链驱动写、记录的是真实 run 的事实——与 T04 audit 表「记录到达的主张」无 FK 的区别在两表注释中说明）。`sha256(V3_SQL)=3bd5388f090ab0ae0e137d91d59a8e48f82f409bbd0de657abf29b897a4252c1`；V1/V2 SQL 文本零改动（指纹与已发布值逐字节一致，见 §8）。
- `run_store.rs`：五方法真实现（全部经同一 `DbQueue` 单写者队列 + `with_write_txn`，每写一事务；owner 三元组校验与 runs 其他写一致；跨 owner 写=Conflict）。
- migrations.rs 内部测试与 migration_idempotency.rs 零改动（T04 已按 `supported_version()` 区间化，V3 自动纳入幂等/防篡改/拒降级面）。

### 驱动链接线（lingxi-service/src/runs.rs，生产链路）

```
HTTP POST /lingxi/v1/sessions/{id}/execute                    (lib.rs，未动)
  → SessionStore::execute_submission_for                      (sessions.rs，未动)
  → RunSupervisor::drive_run                                  (runs.rs)
      模型 turn → ToolRequests：逐调用（ToolCallId={run}-tc{nnnn}）
        工具配额准入（T02，未动）
        ① journal.record_invocation_intent(prepared)          ← 一切外部动作之前
        ② persist tool_call_started（T01 事件，未动）
        ③ 无审批门 → journal.advance(Authorized)
           有审批门（T03）：waiting_approval 腿（未动）
             Approved   → journal.advance(Authorized)
             Rejected/Aborted → journal.record_receipt(Failed,
                             dispatched=false, "not dispatched: …")
                             + 既有失败事件；零执行（执行 0 次语义保留）
        ④ journal.advance(Started)                            ← 外部执行派发前最后落盘
        ⑤ spawn_linked 工具子任务（外部执行；取消树/监督未动）
        ⑥ 结果（含 T04 fence 判定后）→ journal.record_receipt
             Success → Succeeded + dedup_id=content digest
             Failed  → Failed + error 摘要
             Cancelled → Unknown（停止等待≠外部未完成，不伪造失败）
             Unknown（fence/未观测）→ Unknown
        ⑦ persist tool_call_completed（T01 事件，未动）
      终态：唯一 finalize（T01，未动）
```

新增 `journal_receipt_of` 映射（单测锁定：Cancelled→Unknown 的诚实语义、Success 携 dedup、Failed 携摘要）。

### 恢复面（lingxi-service/src/invocations.rs，新；lib.rs 注册 `pub mod invocations`）

```
classify_run_invocations(port, run_id, caps)      → RunRecoveryReport（纯读）
recover_run_invocations(port, run_id, caps, now)  → 读 journal → 逐条 kernel 分类 →
                                                    对 started-无回执条目持久化 unknown 判定
                                                    （record_invocation_unknown；幂等重跑
                                                    unknown_verdict_persisted=false）
```

`RunRecoveryReport.decision_counts()` 六桶计数（证据输出用）。不重执行任何工具、不写 run 状态、不含任何定时/扫描逻辑——T07 消费的决策+数据面。

## 3. 逐验收：预期 vs 实测

### R03-A09 副作用后崩溃不重复执行 — **PASS（本机隔离环境，文件型外部计数替身；证据=外部计数器/请求日志 + 恢复库）**

`r03_a09_crash_after_side_effect_does_not_reexecute_and_receipt_is_unknown`（invocation_journal.rs）：

- 前置：非幂等替身外部系统（`notify.double`，独立临时目录文件日志——不认任何幂等键，消息发送类比）。run 的 turn1 请求该工具；替身**先执行外部操作并落盘请求日志**（计数=1、请求=1），信号测试后**永久停靠（响应不返回）**——外部已执行、驱动收不到结果、回执未落盘。
- kill 前见证：DB 直查 journal 恰 1 行 **phase=started**、**无回执**、target/幂等键/owner 绑定齐全（started 在外部派发前已提交——写序的活链证据）。
- kill：`JoinHandle::abort()` 在 await 点丢弃驱动 future（真实取消原语；RegistrationGuard 触发取消树、Abandoned 相位、durable run 行保持 running 不伪造终态）；bounded-wait 注销完成；`close()`（FIFO 排空——停靠点在工具 await，回执 job 从未入队）；drop state。
- 重启：全新 `ServiceState::bootstrap`（**生产形态，无任何替身配置**）同 data root。实测：**外部计数=1 不变**（重启不自动再执行——当前即诚实行为；A09 断言的就是「不再自动增加」）；run 行 status=running（dangling active，如实）。
- T05 恢复面：`recover_run_invocations`（ConservativeCapabilities）→ 唯一条目决策 **NeedsAttention**（非幂等 unknown 禁止盲重试，reason 含 target 与「blind retry could duplicate」）；unknown 判定**持久化**。
- 收据显示 unknown：DB 直查 phase=unknown、receipt_outcome=unknown；**外部计数仍=1、请求仍=1**。
- 证据：`R03_A09_TRACE`（a09-a10-journal.log）+ evidence `r03_a09_unknown_receipt`（机器断言 7 键）。

### R03-A10 可验证幂等恢复 — **PASS（幂等替身请求记录为证据）**

`r03_a10_idempotent_key_resume_does_not_duplicate_the_external_operation`：

- 前置：幂等替身外部系统（`ledger.double`，文件态 `state.json` 键→记录结果 + 请求日志每行记 key/executed/digest）。驱动派发调用（呈现键=journal 幂等键=`{run}-tc0001`）；外部**执行一次**并持久键→结果，随后响应中断（停靠）；kill+重启同 A09 形态。
- 恢复分类：capability 源按 target 判定 ledger.double **受证实幂等** → 决策 **UnknownResumeWithIdempotencyKey{key 与原键同一}**；unknown 判定先持久化（诚实分类）。
- 恢复执行（T07 将驱动的同一端口调用，本测试按决策执行）：以恢复上下文（journal 条目重建 owner/run/attempt/generation + LocalUser 主体匹配）经**同一 ToolExecutorPort** 同 call id 再调用 → 替身按键去重：**不重复执行**、返回**原记录结果**。
- 核验收束：dedup 响应 == state.json 中该键记录（实测相等）→ `record_invocation_receipt(Succeeded, dedup_id=key)` 关闭（unknown→succeeded 的 VERIFIED 升级路径，adapter 测试另有专门锁定）→ journal 终态 **succeeded + dedup_id**；**外部执行总数恒=1**（请求=2：原始+去重重发——请求日志即证据）。
- 证据：`R03_A10_TRACE` + evidence `r03_a10_idempotent_resume`（8 键：requests=2/executed=1/decision/final_phase=succeeded/verified_digest）。

### 收据生命周期活链（怎么做 1/2 的全形态）— PASS

`journal_lifecycle_progresses_and_closes_receipts_on_the_live_chain`：一次 run 三工具调用（成功/外部失败/审批拒绝）经真实链（含 T03 审批门 waiting_approval 腿）：

- 三条 journal 全量绑定断言（journal_id={run}-tc000N、run/attempt `#a1`/generation=1、owner local_user/user_local、target、args_digest、幂等键=call id）；
- tc0001 succeeded（dispatched=true，dedup_id=外部 digest）；tc0002 failed（dispatched=true，外部错误摘要）；tc0003 **failed+dispatched=false+detail「not dispatched」**（审批拒绝=零执行收据）；外部计数恰=2（被拒调用从未到达外部）；
- run 以真实 Final 收束 `completed.with_final`（单一 finalize 未动）。证据 `r03_t05_lifecycle`。

### 存储契约与分类半场（adapter/kernel 永久测试）

- `invocation_journal_store.rs`（lingxi-adapters，5 测试，真实 RunDatabase）：生命周期全绑定；幂等 replay/冲突响亮（同 id 异 intent=Conflict；同相重推进/同回执重关=no-op）；非法阶梯与回执形状全拒（advance 目标门、started→authorized 回退、prepared 之上的 Succeeded 拒绝、prepared 之上的 Failed-dispatched=false 合法）；四态分类随 durable phase + unknown 判定关闭（幂等重跑、越权目标拒绝）+ unknown→satisfied 升级 + 跨 owner 写 Conflict + 决策接缝（同键 resume）。
- kernel `invocation::tests`（10 测试）：四态、未执行安全重执、settled 无动作、非幂等 unknown→NeedsAttention、只读重执、同键 resume、有能无键不可 resume/可核验、read_only 优先序、CONSERVATIVE 全闭、决策词表稳定。
- `runs.rs` 单测：`journal_receipt_maps_outcomes_onto_the_durable_receipt`（Cancelled→Unknown 诚实语义等）；T04 `fence_verdict` 单测原样绿。

### 任务书「怎么做」1–4 对照

1. 六态收据 + 参数摘要 + 目标代次绑定 ✔（V3 表/entry 结构/活链断言）
2. 执行前意图/执行后回执持久化，不声称跨系统原子事务 ✔（驱动链写序①④⑥；契约注释明示）
3. started-无回执→unknown；按幂等性/可查询性决定核验/重试/人工确认 ✔（kernel 分类决策 + 服务面 + CONSERVATIVE 默认）
4. 只读/受证实幂等有界自动恢复；未知禁止盲重试 ✔（六决策两自动通道 + A09 实测不再增 + A10 实测不重复）

## 4. 修改文件清单

修改（7）：`rust/crates/lingxi-kernel/src/{lib.rs,ports.rs}`、`lingxi-adapters/src/storage/{migrations.rs,run_store.rs}`、`lingxi-service/src/{lib.rs,runs.rs,sessions.rs}`。
新增（4）：`lingxi-kernel/src/invocation.rs`（生产+10 单测）、`lingxi-service/src/invocations.rs`（生产）、`lingxi-adapters/tests/invocation_journal_store.rs`（永久测试 5）、`lingxi-service/tests/invocation_journal.rs`（永久测试 3）。
`rust/Cargo.lock` 零变化（`90111c4b…`=R02_HANDOFF）；三个 crate Cargo.toml 零变化（无新依赖）。逐文件 SHA-256 与 tracked-diff 绑定值见 candidate-summary.txt。**task_supervisor.rs 零触碰**（T03 R1-D1 按派单条件继续递延）。

### 既有测试夹具改动（逐处+理由；无断言删除/放宽/改永真，无 skipped）

1. kernel ports.rs 测试 FakePort 与 sessions.rs 测试 FakePort：补五方法最小实现（T03/T04 同先例——trait 满足编译；kernel 规则门各一处，耐久行为由真实 adapter 测试覆盖）。既有断言原文零改动。
2. **无其他夹具改动**——run_lifecycle/cancellation_tree/session_serialization/late_result_fence/request_dedup/execute_concurrency/event_subscription/service_persistence/run_finalize_property/storage_transactions/migration_idempotency 断言原文未动，全部原样绿（journal 写为纯增量事务，事件序不变）。

## 5. 验证命令与退出码（全部经 rustup 1.98.1 + `--locked`；verify-stage R03 未注册，未运行未伪造）

| 命令 | 退出码 | 结果摘要 |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | 无 diff（开发中首查报 diff 后 `cargo fmt --all` 应用于候选；终查干净——fmt.log 注明） |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 0 error/warning |
| `cargo test --workspace --locked` | 0 | **558 passed / 0 failed / 54 suites**（T04=539/52；+19 = kernel invocation 10 + adapters store 5 + service 3 + runs 单测 1；含 R02 全量回归） |
| `cargo run -p xtask -- check-contracts` | 0 | API_COMPAT_MATRIX 626 entries 零漂移（journal 不进 wire 生成物） |
| `cargo run -p xtask -- check-boundaries` | 0 | DEP-07/08/D5 全 PASS |
| `bash scripts/rust-tauri/r02_t04_storage_tx.sh <probe>`（非门禁探针） | 1 | S1–S3 PASS；S4 仍死于硬编码 version==1（**T04 F-1 原样**，非新破坏）；S4 dump 顺带给出 V3 盘上收据==编译内指纹、userVersion==3 的对账事实 |

定向过滤器（全部命中>0，filters.log）：invocation_journal 3、invocation_journal_store 5、kernel `invocation::` 10、name r03_a09=1、r03_a10=1、journal_receipt=1、fence_verdict=1；回归子集 run_lifecycle 12、cancellation_tree 8、session_serialization 5、late_result_fence 5、request_dedup 4、migration_idempotency 2、storage_transactions 7、run_finalize_property 2、lingxi-kernel lib 35（T04=25，+10）。证据跑统一 `--test-threads=1`（共享 evidence JSON 读改写需串行）。如实记录：三组初始过滤器串因模块路径子串不匹配 0 命中（`runs::journal_receipt` 等），已用正确串重跑并以命中结果入档——0 命中未计为通过。

## 6. 证据位置

`artifacts/rust-tauri/R03/T05-E01/`：`commands.log`（五命令+退出码）、`workspace-test.log`（558/0 原始输出 + EXIT=0）、`clippy.log`、`fmt.log`、`xtask.log`（contracts+boundaries）、`a09-a10-journal.log`（三测试 TRACE + 证据重跑 + EXIT）、`r03-t05-evidence.json`（机器断言 3 组：a09/a10/lifecycle）、`filters.log`（定向+回归计数）、`r02-script-probe.log`+`r02-script-probe/`（F-1 边界与 V3 盘上收据对账）、`candidate-summary.txt`（基线/HEAD/逐文件 sha256/Cargo 零变化/迁移指纹/tracked-diff 绑定）。复跑后无 `/tmp/lingxi-r03t05-*` 遗留、无残留进程。

## 7. 测试替身与崩溃注入边界（如实声明）

- **外部系统替身**：`ExternalDouble`——独立临时目录文件（`requests.log` 每请求一行 JSON：key/executed/digest；幂等型另有 `state.json` 键→结果）。执行**先落盘后返回**（外部效应在其「客户端」崩溃后幸存）。非幂等型不认键（每请求执行）；幂等型按键去重。无任何真实外发。
- **kill 形态**：派单明确允许的「同进程 drop+reopen」。具体为：工具替身执行完外部操作后停靠（响应永不返回）→ `JoinHandle::abort()` 在 await 点丢弃真实驱动 future（RegistrationGuard 触发取消树/Abandoned，durable run 行保持 running——与崩溃后幸存状态一致）→ `close()` 排空队列（停靠点在工具 await，回执 job 从未入队——**不是被跳过的提交**）→ drop ServiceState → 全新 bootstrap 同 data root。持久化边界为真：journal prepared/started 在外部派发**前**已各别事务提交；回执从未提交。未采用隔离子进程 kill -9（生产二进制无 Provider 配置，无法承载替身工具链；该差距如实披露——进程级强杀的全链形态归 T07 崩溃点测试集）。
- **A10 恢复执行**：同键再调用由测试按决策经同一 `ToolExecutorPort` 驱动（T07 的 RecoveryCoordinator 才是生产驱动方；被测对象是决策正确性+journal 收束+外部不重复，非 resume 调度器）。恢复上下文从 journal 条目重建（owner/run/attempt/generation）。
- **等待原语**：通道锚点（arrivals/fired）+ 1ms 轮询/500ms 硬上限的有界真实等待；无 sleep 计时掩盖竞态；门控释放仅在 teardown/恢复前（不影响被测窗口）。
- 开发中修过的三处测试自身缺陷（如实）：生命周期用例误持已弃接收者致子任务 panic、误等第 4 个不存在的 turn、A10 恢复调用复用停靠替身未先放行——均已修正后全绿（缺陷在被测链外，不影响结论）。

## 8. 候选输入摘要

基线 `0bf067d9e…`（=HEAD，无 commit）；修改 7 + 新增 4 路径，逐文件 SHA-256 见 candidate-summary.txt（要点：invocation.rs `a964426e…`、invocations.rs `48617d72…`、run_store `584eb125…`、migrations `6248469b…`、runs.rs `250257dc…`、ports.rs `9b541277fe…`、invocation_journal_store.rs `a2ad122a…`、invocation_journal.rs `11184c2e…`）；`rust/Cargo.lock` 零变化；迁移指纹 V1=`479b0321…`（=registry 已发布值逐字节）、V2=`64d7edfd…`（=T04 审查发布值）、**V3=`3bd5388f090ab0ae0e137d91d59a8e48f82f409bbd0de657abf29b897a4252c1`（新）**；tracked-diff sha256 前 16 `e78df59386ebe190`。

## 9. 未验证项 / 边界（如实）

- **V3 迁移登记递延 T08**：`R02-T04_STORAGE_REGISTRY.json` 的 migrations 数组与 `scripts/rust-tauri/r02_t04_storage_tx.sh` S4 硬编码 version==1 未随 V3 同步——与 T04 审查 R1-F1 同根因，按派单指示与本 Task 派单原文**一并递延 T08**（脚本在当前树实测仍死于同一断言，见 §5 探针；S4 dump 已含 V3 收据对账事实供 T08 修复用）。
- **恢复协调未提前**：启动扫描（非终态 run 遍历→按决策推进/落 interrupted_needs_attention）、跨重启去重（SubmissionDedup 进程内存语义）、`interrupted` 用户面 = R03-T07；本 Task 的 `recover_run_invocations` 是其将消费的决策+数据面，未接入任何启动路径。
- **capability registry 未提前**：`ToolRecoveryCapability` 由调用方供给（测试/LedgerCapabilities 形态）；R04 工具目录的逐 target 真实分类未实现，默认 CONSERVATIVE（未证实=不自动恢复）。
- **generation 语义暂虚**：与 T04 O-4 同源——RunContext.generation 恒 1，journal 如实记录该值；R04 工具目录代次接入后获得真实语义（绑定结构已就位并有断言）。
- **审批链上的 journal prepared 先于 waiting_approval 腿**：journal 与 run 相位腿是两个事实面（journal 记调用收据，run 记状态机相位），prepared 在 waiting_approval 进入前已落盘——拒绝路径以 dispatched=false 收束，无歧义；R04 完整审批网关接手时结构不变。
- **kill 形态差距**：同进程 drop+reopen（§7）；进程级 kill -9 的全链崩溃点测试集归 T07（阶段书 T07 怎么做 4）。
- 无跨平台（本机 arm64 macOS）；npm/桌面栈未触碰；真实供应商/网络归 R05（A09/A10 外部系统均为受控本地替身）。

## 10. 独立复核重点建议

1. **写序而非仅绿灯**：runs.rs 工具路径的 ①intent→④started 在 spawn_linked 之前、⑥receipt 在结果之后——A09 的「kill 前 phase=started 无回执」断言正是该写序的活链见证；复核驱动代码顺序而非只看测试。
2. **unknown 判定的幂等与可升级**：`record_invocation_unknown` 仅 started-无回执合法、重跑 no-op；unknown→Succeeded 的 VERIFIED 升级（A10 收束）与 closed 后冲突拒绝的边界（adapter 测试 `journal_recovery_classes…`/`journal_replays…`）。
3. **决策阶梯方向**：read_only 优先于 key 优先于 verify；有能力无 journal 键时**不可** resume（防「能力存在即自动恢复」的越权放宽）；CONSERVATIVE 全闭。
4. **A09「不再自动增加」的语义**：重启后无协调器（T07 未建）→ 无任何重发路径；T05 恢复面自身只分类+落 unknown 不执行。若复核期引入 T07，此断言需随协调器语义重验。
5. **V1/V2 指纹不动**：独立重算 sha256(V1_SQL)/sha256(V2_SQL) 与已发布值比对；V3 盘上收据==编译内（r02-script-probe.log S4 dump 已含，可复算）。
6. **558/0 对照**：T04 539 + 19 逐项核对（§5）；R02 回归链全绿；FakePort 两处为 trait 满足最小实现。
