# R03-T04 独立验收报告（REVIEWER-R03-T04-R01，第 1 轮）

- VERDICT: **PASS**（附 1 项非阻塞 MINOR 缺陷 F-1，随 R03-T08 阶段门禁前必须关闭；另有 5 项建议性观察 O-1..O-5，不构成本 Task 验收义务缺口）
- TASK_ID: R03-T04（attempt/stream 栅栏处理迟到结果；ACCEPTANCE_IDS R03-A07 / R03-A08）
- 审查者：REVIEWER-R03-T04-R01（全新一次性验收代理；未参与本 Task 的实现/修复/派单）
- 候选：TASK_BASE_SHA `9e319d64836d5a1656908a81e23ab5edf8370515`（分支 codex/rust-tauri-migration）+ 未提交工作树；无 commit/push
- 审查时间：2026-09-29；环境 macOS darwin 27.0.0 arm64；rustup 锁定 cargo 1.98.1 / rustc 1.98.1（与执行者一致）
- 复测产物目录：`artifacts/rust-tauri/R03/T04-R01-review/`（原始命令行+退出码+原始输出见 commands-rerun.log；审查期间未修改任何产品源码/测试/配置/阶段图）

## 0. 独立验收执行方式

- 逐文件读 `git diff 9e319d648 -- rust/` 全量（修改 10 + 新增 4，共 1058 插入/136 删除），并读四个新增文件全文与 run_store.rs/runs.rs/sessions.rs/dedup.rs 现行完整正文（不止 diff 上下文）。
- 真实复跑全部四门禁 + 定向三件 + 受影响 suites + workspace 全量（命令与退出码见 §5 与 commands-rerun.log）。
- 独立重算迁移 SQL 指纹并用 R02-T04 检查工具对真实迁移库做盘上收据对账（§3.4）。
- 结论区分：源码推断 / 实际运行 / 受环境限制，逐条标注。本机 arm64 macOS 单平台；无真实供应商（A07 用门控替身，规格允许）。

## 1. 候选绑定核对（前后两次）

| 项 | 结果 |
|---|---|
| tracked-diff sha256 前 16（复跑前） | `35111570638b82b9` = 派单绑定值 ✓ |
| tracked-diff sha256 前 16（复跑后） | `35111570638b82b9`（审查期间代码冻结成立）✓ |
| status 集 sha256 前 16 | `f8fb20cb446bbc15` ✓（需剔除绑定后写入的派单文件本身；其余未跟踪集与绑定一致） |
| 逐文件 sha256（15 项） | 修改 10 + 新增 4 + `rust/Cargo.lock` 全部与 `T04-E01/candidate-summary.txt` 逐字节一致 ✓ |

## 2. 到期义务逐项复核（阶段书怎么做 1–4 + A07 + A08 + 派单总控细化）

### 2.1 三层栅栏（怎么做 1/2）——真实接线成立（源码逐行核实 + 实际运行）

**端口层**（`rust/crates/lingxi-kernel/src/ports.rs`）：`ResultFence{run_id, attempt, generation}` 新载体；`TurnProviderPort::next_turn` 返回 `ProviderTurnResult{fence, turn}`、`ToolExecutorPort::execute` 返回 `ToolExecutionResult{fence, outcome}`（所有异步返回带三重身份）；`matches_ctx` 三项全等才 admit；`LateResultReason` 五词表稳定 + `StaleResultFact`（只记事件类型摘要，不记全文）。单测 2 个（`result_fence_admits_only_the_current_identity_triple` 覆盖旧/新 attempt、异 run、异 generation 四向拒绝；词表稳定测试），实际运行通过（kernel lib 25/0 = T03 23 + 2 ✓）。

**驱动层写前核对**（`rust/crates/lingxi-service/src/runs.rs`）：模型与工具两调用点均在 persist 之前 `fence_verdict(root, &result.fence, &ctx)`：
- 身份不符 → audit `fence_mismatch` + 活 run 响亮 `failed.provider_error`（code `late_result_fenced.*`，retryable=false）——集成测试实测 run 落 `failed`、messages=0、被栅栏结果的事件计数 0；
- 身份匹配但取消先到 → `CancelledBeforeWrite` → audit + `settle_cancellation`，完成内容永不落流（T03 O1 写侧窗口就此收口）；单测 `fence_verdict_requires_identity_and_no_cancellation` 三分支全覆盖（实际运行通过）；
- 工具侧栅栏 → 当前 call 记 **Unknown**（不伪成功：测试断言 `tool-content` 计数 0、unknown 事件恰 1 条；不盲重试：无重发路径，run 继续用真实 Final 完成）；
- audit 写失败为显式降级（`tracing::error` 标注，栅栏本身已生效）——符合红线"要么抛要么显式降级并标注"。

**存储层三腿拒写 + audit 独立提交**（`rust/crates/lingxi-adapters/src/storage/run_store.rs`）：`record_run_events` 在单写者队列 job 内：终态 run → 拒 + audit `run_terminal`（detail 保留 "already terminal"）；未开 attempt → 拒 + audit `attempt_never_opened`；已开但被超越的旧 attempt（`current_attempt_of` = `ORDER BY rowid DESC LIMIT 1`，单写者 ⇒ rowid=开序，run_attempts 无 DELETE，核实无乱序插入路径）→ 拒 + audit `attempt_stale`。**关键结构核实**：三条拒绝分支各自先以独立 `with_write_txn`（BEGIN IMMEDIATE + COMMIT）落 audit 行、**然后**才返回 `Err(Conflict)`——拒绝事务的 rollback 不可能吞掉 audit。该结构不是仅靠绿灯：A07 两测试在拿到 Conflict 后立即 SQL 直查 `stale_result_audit` 计数（若 audit 在被回滚事务内则恒 0，测试必红），我的复跑复现 audit 恰 1 行（重复投递 → 恰 2 行）。执行者自报"首轮把 audit 放进拒绝事务被实测抓出后修正"与最终结构及测试护栏自洽，修正真实。`record_stale_result`（新 port 方法）为独立单事务 audit-only 写，任何 run 状态下合法；kernel/sessions 的 FakePort 补的最小实现仅为 trait 满足，耐久行为全部由真实 adapter 测试覆盖（fence_mismatch 两测试的 audit 行正是经真实端口写入）。

**A07 双形态**（验收原文「attempt2 或下一Run已开始」两半都做）：
- 跨 attempt（`r03_a07_late_attempt1_result_vs_current_attempt_is_stale_only`）：attempt1 可重试失败 → attempt2 开启停流读取 → 带外投递 attempt1 的 `{run}-mc0009-done` → `Conflict`（"no longer the current attempt"）+ audit 恰 1 行（reason=attempt_stale, attempt={run}#a1, refused 含 model_call_completed）；投递前后 key_events/messages 计数不变；放行 attempt2 → `completed.with_final`、最终正文= attempt2 答案、全流 `payload_json LIKE '%mc0009%'` 计数 0——跨 attempt 断言三层合成（不落流/不成正文/不成成功状态）✓。
- 下一 Run（`r03_a07_late_result_after_cancel_and_next_run_pollutes_nothing`）：run1 取消结算（cancelled/cancelled.requested）→ run2 completed → 投递 run1/attempt1 结果 → `Conflict`（"already terminal"）+ audit 恰 1 行 run_terminal；run1 终态不翻转、messages=0；run2 事件/消息计数不变、run2 流 `mc0007` 计数 0、run2 正文不含 run1 内容——跨 Run 零污染 ✓。
- 写前核对活链两组（模型错标身份→响亮失败；工具外来 generation=99→Unknown+audit）见上。

**A08**（`rust/crates/lingxi-service/tests/request_dedup.rs` + `dedup.rs` + `sessions.rs` + `lib.rs`）：
- 同 id 改内容 → `DuplicateRequestConflict`（HTTP 面 409 `request_id_conflict`，不可重试），**runs 恒 1**（不复用旧执行、不新增执行）——测试以 DB 计数断言，实际运行复现；
- 幂等半边：同 id 同内容（CRLF→LF 归一后等价）→ 返回原 run_id + `replayed:true`、runs=1；随后同 id 改内容仍 conflict；
- key=(owner_kind, owner_subject, session_id, request_id) 绑定可信主体+会话（kernel storage 词表，非 payload 声明）；三主体三命名空间实测互不串用（同 user 异 kind、同 principal 异 session 各自独立接纳，owner 回放命中自己的绑定）；
- admission（busy gate try_begin_run + allocate_run_id 原子计数）为同步闭包，在 dedup 注册表锁内执行、全临界区无 await（首实现的 MutexGuard 穿 await 问题已以闭包式重构消除，结构核实属实；实际为全局单锁，串行化强于"key 锁"，见 O-2）；失败 admission 零残留（dedup 单测）；注册表 cap 4096 满载 503 响亮；
- 摘要 = CRLF→LF 后全量输入 sha256（复用 protocol 既有 `digest_arguments` canon，无截断投影——单测断言尾部空格差异不折叠）；
- 无 id 路径与 pre-T04 行为一致：`ExecuteSubmission::plain` 委托，同输入 3 次提交 = 3 run（实测）；`execute_for` 签名不变，既有调用者零改动。

### 2.2 重连只订阅（怎么做 3）

`reconnect_resubscribes_only_and_never_restarts_a_model`：结算后 subscribe(None) 取 cut（snapshot_seq>0），持 cursor 续订阅 `mode=resume`；断言 runs 计数不变、provider 到达通道空（`try_recv` err = 零新模型调用）、run 状态不变。实际运行通过，证据 `reconnect_read_only` 复现。游标续读复用 R02 EventService 既有能力，未建第二套。

### 2.3 状态属性测试（怎么做 4）

`late_result_fencing_property.rs`（真实 RunDatabase，每轮临时真库；固定 xorshift seed `0x5eed0000a07df00d` 写死为测试身份；60 轮；池内种子洗牌乱序交付）：
- 三形态：重复（同类交付二次 + 结算重复）、乱序（洗牌含结算居中）、延迟（结算后投递 / attempt2 后投 attempt1）；
- **每笔交付后**断言：终态永不翻转（非法 durable 序列即失败）；被拒交付 key_events 计数不动、messages 恒 0；stale 类拒绝恰 +1 行 audit、非 stale 类拒绝零 audit（settlement 冲突归 T01 域）；非法状态机请求（Running→Completed、Cancelling→Running）必拒；
- **totals 非空泛化断言**与我复跑的机器证据逐键一致：`current_ok=36, stale_refused=23, ghost_refused=26, terminal_refused=167, settle_first=60（=每轮恰一次首结算）, settle_replay=10, settle_conflict=110, illegal_rejected=120（=每轮 2 笔全拒）`——八类全部真实行使，无空集合、无永真断言、无 ignored（各 result 行 0 ignored）。我的复跑证据 JSON 与执行者 T04-E01 的 10 键**逐键相等**（含全部属性 totals）。

### 2.4 migration V2（版本化增量、不改已发布校验值）

- 源码核实：V2 为纯追加（MIGRATIONS 数组加 version 2 `stale_result_audit`：无 FK 有意为之——audit 记录"到达的主张"，可指向不存在的 run；含 run_id 索引）；V1_SQL 文本零改动。
- 独立重算：`sha256(V1_SQL) = 479b0321494269fca85d9f973b01a8f9d1aa57dc08fc3cf8fddbafeace461bb8` **与 R02-T04_STORAGE_REGISTRY.json 已发布值逐字节一致**；`sha256(V2_SQL) = 64d7edfdab74e13e623a9d8d5f2381dad4803f545fa126d8eda8cc02c3dc889c`。
- 盘上对账（实际运行）：`r02_t04_storage_tx.sh` 探针输出的真实迁移库 receipts 显示 V1 收据指纹 479b0321…（未动）、V2 收据指纹 64d7edfd…（= 编译内重算值）；`verify_receipts` 对每一版本做 连续性/名称/指纹 三重校验，V2 自动纳入防篡改面。
- migration_idempotency.rs 的版本断言从硬编码 "1" 改 `supported_version()`：3 轮重开版本不动、schema 指纹逐字节一致、行数稳定、V1 收据==编译内指纹、两形态篡改/未来版本全拒——**保护未降**（migrations.rs 内部测试 3 处同性质：幂等期望区间化、篡改 UPDATE 加 `WHERE version=1` 维持"单行篡改被检出"意图——两行收据下全表置 0 会先撞主键而非走防篡改检查、未来版本改 supported_version()+1，均等价适配）。

## 3. 发现

### F-1（MINOR，非阻塞；R03-T08 阶段门禁前必须关闭）：migration V2 未同步两处 R02-T04 迁移记录资产

- **定位/重现**：
  1. `docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json` 的 `new_persistence_points[0].migrations` 仍只有 version 1；R02_HANDOFF.json 接口面把该文件登记为 StoragePort 的迁移指纹权威（"migrations 版本化（R02-T04_STORAGE_REGISTRY.json 指纹）"）。V2（fingerprint `64d7edfdab74e13e623a9d8d5f2381dad4803f545fa126d8eda8cc02c3dc889c`）未登记。
  2. `scripts/rust-tauri/r02_t04_storage_tx.sh` S4 断言 `userVersion == supportedVersion == 1`（357 行），在当前 V2 树上实际运行 **exit 1**（本审查探针：`artifacts/rust-tauri/R03/T04-R01-review/r02-script-probe.log` + `r02-script-probe/`）。该脚本被 `rust/crates/xtask/src/stage_maps/R02.json` 的 `a07_a08_storage_tx` 命令登记，R03_TEST_MAP 的 stage_gate_T08 明确要求"R02 基础链回归…经 verify-stage R02 或等效定向命令重跑"——T08 阶段门禁时该链必红。
- **违反的义务**：不违反 R03-T04 任务书条款（"须走新迁移版本而非改 v1 SQL"已遵守；R02-T04 报告的登记规矩字面针对"新持久化点"，runs.db 非新点；本 Task 派单亦未列 R02 脚本/registry 为交付物）。属于派单预判的"registry 是否需要并已按规矩更新"项：**需要、未更新**。
- **后果**：指纹权威文件与实现漂移（R08 迁移演练消费者会读到缺 V2 的迁移清单）；R03-T08 阶段门禁的 R02 回归链一条命令确定失败。
- **根因**：执行者把 Rust 侧迁移测试做了 supported_version() 等价化，但漏掉了两处同样硬编码版本 1 的非 Rust 记录资产（registry JSON、R02 证据脚本）。
- **同族路径**：`scripts/rust-tauri/r02_t04_storage_tx.sh` 全部 S4 断言（version 与 receipts 数量两处）；registry `migrations[]` 数组。
- **必须重跑**：修 registry+脚本后重跑 `r02_t04_storage_tx.sh`（预期 S4 恢复绿且盘上双收据指纹断言改为编译内等价）；T08 落 R03.json 后重跑 `verify-stage R02`（或等效定向命令）整链 + `migration_idempotency` + workspace。
- **处置建议**：登记进 R03 阶段账本，归属 R03-T08（或总控授权的修复轮）；本 Task 不为使其变绿而回改。

### 建议性观察（不构成验收缺口）

- **O-1（dedup 绑定时序）**：绑定在 admission 成功时写入（早于 `drive_run`）；若 drive_run 之后以存储错误失败，该 id 已永久绑定，同 id 同内容重试会 replay 一个 durable 上可能不存在/未完成的 run id。处于已文档化的进程内存/跨重启归 T07 边界内（conflict 语义不受影响）；建议 T07 恢复语义落地时对 replay 返回的 run_id 做 durable 存在性核对。
- **O-2（锁粒度措辞）**：`SubmissionDedup::admit` 是全局单 Mutex（非 per-key），admission 闭包为全同步非阻塞（try 门+原子计数），争用有界且正确性强于报告/注释所述"key 锁"；仅措辞精度问题。
- **O-3（跨主体拒写不 audit）**：`record_run_events` 的 owner 三元组不匹配分支返回 Conflict 但不落 audit（代码注释：属边界违规探针，归安全审计面，非本 run 自身工作的迟到结果）。与派单三腿清单一致，为有意设计；记录备查。
- **O-4（generation 语义暂虚）**：RunContext.generation 当前恒 1，栅栏 generation 腿要等 R04 工具目录代次接入才有真实语义；实现已按三重全等落地并有异 generation 单测与集成测试（generation=99），执行者已如实披露。
- **O-5（audit 表无保留策略）**：durable audit 表按到达记账、无清理边界（执行者已披露，与 key_events 同性质诊断事实）；建议在资源上限旗标体系（R02-T07 面）后续考虑有界化。

## 4. 受影响既有测试改动逐处判定（保护未降）

| 文件 | 改动 | 判定 |
|---|---|---|
| cancellation_tree.rs / run_lifecycle.rs / session_serialization.rs | 各 2 处 impl 签名适配：`ctx_at_issue = ctx.clone()`（**发出时**捕获）后返回值包 `of_ctx(&ctx_at_issue, …)` | 回显请求时身份=诚实适配器默认行为；取消/重试不改 ctx 三元组，`fence_verdict` 的取消腿独立判定——行为不变；**断言原文零改动**（diff 逐行核实），复跑 8/12/5 全绿 |
| migration_idempotency.rs | 版本断言 "1"→`supported_version()` | 版本不得移动的语义保持，V1 收据指纹断言原样；复跑 2/0 绿 |
| migrations.rs 内部测试 3 处 | 幂等期望区间化 / 篡改 UPDATE 加 WHERE / 未来版本 +1 | 见 §2.4，防篡改/幂等/拒降级语义等价保持 |
| sessions.rs 内部 FakePort | 补 `record_stale_result` 最小实现 | 仅 trait 满足；真实行为由 adapter 层覆盖 |

无断言删除/放宽/改永真，无 skipped/ignored（全部 result 行 0 ignored），无 mock 待测核心（替身只产外部响应/错标身份；状态、审计、存储、finalize、取消树全为真实组件）。

## 5. 实际复跑结果（全部本机真实执行；命令与退出码原文见 commands-rerun.log）

| 命令 | 退出码 | 结果 |
|---|---|---|
| 定向三件（late_result_fence/request_dedup/late_result_fencing_property，--test-threads=1 + 独立证据文件） | 0 | 5/0 + 4/0 + 1/0；我的 10 键证据与执行者逐键相等 |
| name 过滤 r03_a07 / r03_a08 | 0 | 命中 4 / 1（>0 ✓） |
| 受影响+回归子集（run_lifecycle/cancellation_tree/session_serialization/execute_concurrency/event_subscription/service_persistence；migration_idempotency/storage_transactions/run_finalize_property；lingxi-kernel） | 0 | 12+8+5+3+12+2；2+7+2；25 —— 与执行者 §5 计数逐一相同 |
| `cargo test --workspace --locked` | 0 | **52 suites，539 passed / 0 failed**（=T03 520 + 新增 19：5+4+1+6+2+1，逐项核对成立；含 R02 全量回归绿） |
| `cargo fmt --all -- --check` | 0 | 无 diff |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 0 error/warning |
| `cargo run -p xtask -- check-contracts` | 0 | API_COMPAT_MATRIX 626 entries 零漂移；生成物 drift-free（`ExecuteRequest.requestId`/`ExecuteAccepted.replayed` 为 lingxi-service 侧加法，不在 lingxi-protocol 生成物面，wire 兼容为纯加式：serde default + 仅 true 序列化） |
| `cargo run -p xtask -- check-boundaries` | 0 | DEP-07/08/D5 全 PASS |
| `cargo run -p xtask -- verify-stage R03` | 硬拒 | `unknown stage "R03"`（未注册未伪造 ✓；R03.json 归 R03-T08/R03-SUP-01） |
| 迁移指纹重算 + 盘上收据对账 | — | V1=479b0321…（=已发布值）；V2=64d7edfd…（编译内=盘上收据一致） |
| `bash scripts/rust-tauri/r02_t04_storage_tx.sh`（探针） | **1** | S4 硬编码版本 1 断言失败（F-1 证据，非本 Task 门禁项） |

复跑后无 `/tmp/lingxi-r03t04-*` 遗留、无残留进程。

## 6. 范围核对（不越界）

- **T05+ 未提前**：grep `InvocationJournal`/收据状态机零命中；无恢复推进（dedup 明确进程内存、跨重启归 T07，模块头+报告如实声明）；无恢复策略/unknown 收据结构。
- **R04 未提前**：generation 腿按三重全等实现但未引入工具目录/registry 语义（O-4 如实披露）。
- **R06/R07 未倒灌**：取消传输入口/WS steer 路由未新增（A07 经服务面 `cancel_run_for` + 存储端口带外投递，沿 T02/T03 先例）。
- **task_supervisor.rs 零触碰**（diff 为空）→ T03 审查 R1-D1（drain 到期 abort 死代码）按派单条件**继续递延**，递延前提成立。
- **Cargo 零变化**：`rust/Cargo.toml`、`rust/Cargo.lock`（=90111c4b…R02_HANDOFF 值）、三 crate Cargo.toml diff 全空（摘要复用 protocol 既有 sha2/canon，无新依赖）。
- **无 tokio test-util**：grep 仅命中 late_result_fence.rs 注释"无 test-util 时间控制"；确定性方案为单线程 current_thread + 通道锚点 + 门控替身 + 1ms 轮询/500ms 硬上限有界真实等待。
- **npm/桌面栈/生产默认入口零触碰**（git status 全集核对）；Node/Electron 生产链无改动。

## 7. 执行者报告准确性抽查（反证情况）

- 候选摘要（15 文件 hash、Cargo.lock、双绑定值）：全部独立复算一致，无失实。
- 539/0 与 52 suites、+19 分解、各定向/回归计数、属性 totals 八类数值：全部独立复现一致。
- §10 自报复核重点（audit 不被回滚吞、"当前 attempt" rowid 判定、admit 闭包串行化、A07 泄漏断言口径、迁移测试适配非弱化）：逐项源码核实属实；"首轮实现被实测抓出并修正"（audit 回滚吞、MutexGuard 穿 await）不可重放历史，但最终结构与护栏测试自洽，无反证。
- 报告 §4/§6/§8/§9 的清单、证据路径、未验证项披露（跨重启去重、取消传输入口、generation 语义、CancelledBeforeWrite 竞态窗口、audit 增长、R1-D1 递延）与源码/复跑事实一致，无发现虚报。
- **报告未提且实际存在的点 = F-1**（registry/R02 脚本未随 V2 同步）——执行者报告对迁移登记只写了"fingerprint 机制原样"，未提 registry 更新义务，这是本轮唯一报告盲区。

## 8. 结论

R03-T04 到期义务（阶段书怎么做 1–4、三交付物：结果栅栏/请求去重/状态属性测试、A07 双形态、A08 幂等冲突、重连只订阅）均有有效证据且真实接线成立（端口载体 → 驱动写前核对 → 存储三腿拒写 + audit 独立事务提交，三层实测贯通）；回归满足（workspace 539/0 = T03 520 + 19，R02 全量链绿，四门禁全 exit 0，证据由本审查者独立复跑并逐键复现）；无越界实现（T05+/R04/R06+ 未提前，task_supervisor 零触碰故 R1-D1 继续递延，Cargo 零变化，无 test-util）。发现 1 项非阻塞 MINOR（F-1：V2 未同步 registry 与 R02-T04 脚本，脚本在当前树实测 exit 1），不影响本 Task 任何验收断言与当前门禁，但必须在 R03-T08 阶段门禁（R02 回归链重跑）前关闭并按 F-1 所列矩阵重跑。允许 PASS。

**VERDICT: PASS**（F-1 非阻塞，登记递延至 R03-T08/总控；O-1..O-5 建议性观察）。
