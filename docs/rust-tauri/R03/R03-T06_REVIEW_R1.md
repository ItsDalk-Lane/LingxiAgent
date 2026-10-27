# R03-T06 独立验收报告（REVIEWER-R03-T06-R01，第 1 轮）

- VERDICT: **PASS**（附 1 项非阻塞 MINOR 报告文档缺陷 D-1，随下一轮触碰该报告时更正；另有 5 项建议性观察 O-1..O-5，不构成本 Task 验收义务缺口）
- TASK_ID: R03-T06（子代理、后台运行与权限继承）；ACCEPTANCE_IDS：R03-A11、R03-A12
- 审查者：REVIEWER-R03-T06-R01（全新一次性独立验收代理；未参与本 Task 的实现/修复/派单；验收期间未修改任何产品源码/测试/配置/阶段图，代码冻结）
- 候选：TASK_BASE_SHA `6465395cf8e84121ea55b1481afc5711a4d707e8`（分支 codex/rust-tauri-migration，=HEAD，无 commit/push）+ 未提交工作树（修改 18 + 新增 7 个 rust 路径）
- 审查时间：2026-09-29；环境：macOS darwin 27.0.0 arm64；rustup 锁定工具链 1.98.1（全部命令经 `~/.cargo/bin/cargo` + `--locked`；专用 `CARGO_TARGET_DIR=/tmp/r03-t06-review-target`；本机 Homebrew rust 未使用）
- 复测产物：`artifacts/rust-tauri/R03/T06-R01-review/`（gates-rerun.log / targeted-rerun.log / workspace-test-rerun.log / xtask-rerun.log，含退出码）

## 0. 独立验收执行方式

- 逐文件读 `git diff 6465395cf -- rust/` 全量（修改 18 + 新增 7，共 1210 插入/80 删除），并整读四个新增生产文件（kernel subagent.rs、service subagents.rs/background.rs、adapters run_lineage_store.rs）与五个新增测试 suite 全文；runs.rs/sessions.rs 关键区段按现行正文逐行核对（不止 diff 上下文）。
- 独立实读执行者报告 §1 全部 Node 语义点位（subagent-tool.ts / subagent-tool-policy.ts / session-permission-mode.ts / session-permission-wrapper.ts / experiments/registry.ts / STATE_TRANSITIONS.md）逐一比对，含 4 处自报映射偏差的原始证据。
- 独立重算 V1–V4 迁移 SQL 指纹、逐文件 SHA-256、Cargo.lock/Cargo.toml 哈希、对账 candidate-summary.txt 与 R02_HANDOFF 链值。
- 真实复跑：fmt/clippy/workspace 全量/xtask 两门禁 + 七组定向（A11/A12/生命周期/lineage 存储/kernel subagent/task_supervisor D1 回归/cancellation_tree），候选摘要复跑前后两次核对。
- 结论区分：源码推断 / 实际运行 / 受环境限制，逐条标注。本机 arm64 macOS 单平台；无真实供应商（A11/A12 为派单允许的确定性替身）。

## 1. 候选绑定核对（前后两次）

| 项 | 复跑前 | 复跑后 |
|---|---|---|
| tracked-diff sha256 前 16 | `27010f53cfb3dce8` = 派单绑定值 ✓ | `27010f53cfb3dce8`（代码冻结成立）✓ |
| status 集 sha256 前 16 | `f2b53dcb14640a08` ✓（剔除绑定后写入的审查派单文件本身后与绑定一致；本审查 .log 证据被仓库既有 `*.log` 忽略规则遮蔽——T04/T05 审查同一形态） | 本报告写入后 status 集自然再变（报告文件本身），rust/ 路径集与 tracked-diff 不变 |
| 逐文件 sha256 | 18 修改 + 7 新增全部与 `T06-E01/candidate-summary.txt` 逐字节一致（本审查独立 `shasum -a 256` 复算，抽查 11 个含全部新增路径 + 全部修改文件清单核对）✓ | — |
| Cargo 零变化 | `rust/Cargo.lock` = `90111c4b…`（candidate-summary 同值；git diff 为空）；root/三 crate Cargo.toml 与 lingxi-protocol/ git diff 为空 ✓ | — |
| tokio test-util | 全部 Cargo.toml 无 `test-util`（lingxi-service tokio features 仍 `macros,rt-multi-thread,net,signal,io-util,time,sync`）；无 `pause()`/`start_paused`；源码仅 2 处注释提及 ✓ | — |

## 2. 冻结语义溯源核对（报告 §1 的 Node 证据逐处独立实读）

本审查者独立打开全部声称点位，语义逐条属实，未发明交互：

1. **派发 fire-and-forget**：`lib/tools/subagent-tool.ts:286` execute → `store.defer(taskId,…)`（:382-396，`deliveryIntent:"trigger_parent_turn"` :389）→ `runPromise` 挂 then/catch 不 await（:552-560）——属实。
2. **并发/超时**：:228-229 `MAX_PER_SESSION=10`/`MAX_GLOBAL=20`；:27 `SUBAGENT_TIMEOUT_MS=30min`；:428-431+461 `timeoutTimer` 在 `executeForAgent` 真正开跑才起（排队不计入）——属实；衰减检查（:342-350）先于并发闸（:353-363），Rust dispatch 同序。
3. **可见性**：:257 task 参数描述「cannot see the current conversation history unless you include it here」；:482 `fileReadSessionPaths:[parentSessionPath]`——属实；R03 child input=task 原文（A11 测试以 input marker 区分父子证明），文件读取面=R04/R06 如实递延。
4. **两态档位 + #1614 衰减拒绝**：`subagent-tool-policy.ts:65-73` `resolvePermissionMode`（read→READ_ONLY；write→父只读 throw，否则继承；省略→继承）+ :53-63 `SubagentAccessDeniedError`（code `SUBAGENT_WRITE_DENIED_BY_PARENT_READ_ONLY` 与 message 全文）——与 kernel `resolve_subagent_access`/`SubagentAccessDenied` **逐字一致**；`resolveInheritedMode(null)`→OPERATE（:46-49）= kernel「父档缺失=OPERATE」。
5. **逐 target 边界**：`session-permission-mode.ts:375-431` `classifySessionPermission`——`context.isSubagent && SUBAGENT_BLOCKED_TOOLS`（:382-389，code `ACTION_BLOCKED_IN_SUBAGENT`、layer `subagent_blocklist`、message 全文）**优先于**一切 mode 判定；READ_ONLY 收尾 `blockedByReadOnly`（:410）+ isSubagent 分支（:221-228，layer `subagent_access`、message 全文含「a subagent's permission can never exceed its parent session」）——与 kernel `authorize_child_tool` 三段结构（blocklist→只读白名单外拒绝→放行）及 code/layer/message **逐字一致**。
6. **blocklist 全集镜像**：`SUBAGENT_BLOCKED_TOOLS`（:80-103）19 项与 kernel `SUBAGENT_BLOCKED_TARGETS` 逐项一致，+`subagent_reply`/`subagent_close`（自报偏差①，属实：Node 靠 `executeIsolated` 隔离子会话使线程不可达，R03 child 共享父会话行，结构性防递归必须显式——kernel 注释如实登记）。
7. **deny→toolError**：`session-permission-wrapper.ts:798-806`（errorCode/permissionMode/toolName/details 透传）——Rust 镜像为 Forbidden 结构化工具事件。
8. **实验开关默认值**：`lib/experiments/registry.ts:15` `subagent.proactive_delegation`、:127 `defaultValue:false`；策略默认 intercept（subagent-tool-policy.ts:37-39）——kernel `SubagentPolicy::default` 同值（9 单测锁定）。
9. **回复=续接同会话 OPEN 线程**：`subagent_reply`（:666-940）`validateDirectThreadForReply` 三校验 + 显式>线程档>继承再衰减（:716-724 与 Rust `reply_child` 同构）；**busy 线程 Node 走 `runSerialized` 排队续接（:862 `threadStore.runSerialized`）而 Rust 响亮 ThreadBusy 拒绝**——自报偏差②，属实且如实披露（递延 R07）。
10. **关闭=三校验+reason 入 summary**：`subagent_close`（:942-999）NotFound/NotDirect/NotInSession/NotOpen/Busy + `closeDirectThread(summary=reason)`——Rust `close` 同构（reason 记为线程 `closed: {reason}` 锚）。
11. **父取消传播**：`docs/refactor-2026/P02/STATE_TRANSITIONS.md:42` T8 `abortByParentSession`（该父下全部活跃→aborted，子不反向）——`CancelRegistry::register_linked` + `run_root_under` 语义一致（cancel.rs diff 核实：child run 根 scope 挂父 scope 之下，传播单向）。

**4 处自报映射偏差全部有据且如实**（①回复/关闭入禁用面、②busy=响亮拒绝、③线程登记进程内存、④权限档在 SessionSupervisor 登记处）；另有 1 处未自报的保真度差距见 O-1（ask 档继承，非验收阻塞，R04 绑定）。

## 3. 到期义务逐项复核

### 3.1 子代理运行适配（必须交付 ①；怎么做 1）

真实接线成立（源码级 + 实际运行）：

- **dispatch**（runs.rs 委派分支 → `SubagentLauncher::dispatch`→`spawn_child`）：衰减（`resolve_subagent_access`）→ 并发闸（per-session 10/global 20，锁内判定+递增）→ 线程登记有界（cap 1024，先逐出已关闭）→ `storage.allocate_run_id`（唯一合法 run id 源）→ `parent_scope.child(ChildRun)` + `spawn_linked(owner=parent_run_id)` + `drive_run(parent_scope=Some)` 内 `register_linked` → **丢弃 ChildHandle（fire-and-forget）**，父工具调用立即返回 `Success{content_digest=child run id}`，journal 收据 Succeeded+dispatched=true+dedup_id=child run id（`journal_receipt_of` 核实）。失败路径（allocate 失败/未绑定/spawn 拒绝/衰减/超限）全部回滚并发计数与线程登记——A11 escalation 伴测实测会话 run 数=1、线程登记空（零 child run）。
- **reply**：三校验（NotFound/NotInSession/NotOpen/ThreadBusy 稳定错误码）→ tier 重解析（显式>线程档>继承，再对**当前**父档衰减）→ 续接 child run；continuation lineage.parent_run_id=发起 reply 的父 run、cause=其 reply 工具调用（生命周期测试断言）。
- **close**：同三校验 → Closed，reason 入线程锚，零 run 创建。
- **child drive**：`timeout_at(开跑锚点)` 包裹；到期 `drive_scope.cancel("subagent timeout")` 后 `drive.await`——子走**自身**四相取消+唯一 finalize（不 drop future）；steering=None；quota lane `{session}::subagent::{thread}` + agent lane `subagent:{agent}`（隔离车道动机注释在位；用户路径仍传 session 本身，语义不变——run_lifecycle 等回归全绿佐证）。
- **结果回流**：`deliver_retained`（bounded；满载 error! 响亮 + durable 事件为权威记录）→ 忙会话当前 run 下一轮 drain / 闲会话保留给下一 run 首轮——A11 正主实测下一 run 首模型轮 input 含 `[subagent-result thread=…` 且 drain 后无残留。
- **SubagentLauncher trait object**：双向 Weak（supervisor 持 `Weak<dyn>`、runtime 持 `Weak<RunSupervisor>`）——组合根 bind_supervisor 后 `ServiceState` 持两端，无引用环；bootstrap 声明与 lib.rs diff 核实。

### 3.2 权限衰减规则（必须交付 ③；怎么做 2；R03-A11）

- **kernel 两态档位**：`SessionPermissionMode{Operate,Ask,ReadOnly}`（wire 词表稳定、未知 None 响亮）+ `is_read_only` 坍缩（镜像 `isReadOnlyPermissionMode`）；`resolve_subagent_access` 矩阵 9 单测锁定（含 #1614 拒绝码与出路文案逐字）。
- **驱动层生效点**（runs.rs 工具循环，T05 authorized 判定步）：journal intent(prepared) → `tool_call_started` 事件 → **`RunGrant::Subagent{tier}` 下每 target 先 `authorize_child_tool`**；Denied → **不 advance(Authorized)、不 started、不派发执行器**，receipt `Failed+dispatched=false+detail=拒绝原因`、`tool_call_completed` 事件携带 `Forbidden {code}[{layer}]: {message}`；`RunGrant::Full`（用户 run）T05 语义不变（同段源码核实）。
- **A11 断言「真实授权边界拒绝」成立**：`RecordingTool` 替身**会执行 write**（返回成功）——正主实测 `tool_rx.try_recv().is_err()`（替身零调用、零 arrivals）+ journal `dispatched=0` + `ACTION_BLOCKED_BY_READ_ONLY` 与「never exceed its parent session」原因保留 + 替身从未被咨询；**负控**（operate 档）同一 write target 放行且执行器**恰一次**（arrivals==[child_run|write]）——拒绝是授权档的裁决而非全面封锁；**换模型不扩权**伴测（model 覆盖 other-provider/bigger-model）child 仍只读、write 仍拒、零执行——授权判据仅 grant（绑定 child run），与 per-call executor/model 无关。

### 3.3 后台提交接口（必须交付 ②；怎么做 3；R03-A12）

- **同 admission 链**：`execute_submission_for`/`execute_background_for` 共享 `admit_submission`（归属→NotFound/Forbidden→busy gate→T04 requestId 去重，Replay/Conflict 语义原样；admission 闭包在 dedup 键锁内执行）——diff 逐行核实为纯抽取重构，前台路径传参不变。
- **spawn_detached 同一 supervisor**（owner=None）+ 会话 lease move 进 drive（所有退出路径释放）+ registry 有界（cap 1024，先收割已完成）——`BackgroundDriveRegistry` 为登记/退出钩子面，**无触发/排队/定时逻辑，非第二调度器**。
- **三断开差异**：①客户端断线=后台 drive 结构上无 caller 可达（A12 实测监督登记 `run_id=None` 的 `background_drive:{run_id}` + registry live_ids + durable running + 重连同 requestId 幂等 Replay 原 run id、run 数=1、停泊工具 arrivals 无第二条、改内容 DuplicateRequestConflict、释放后同一 drive completed.with_final、events_page 可查）；②桌面关窗=同客户端断线（桌面是独立服务的又一客户端，02 §1；Tauri 宿主生命周期=R09——定义级映射，如实）；③服务退出=`graceful_shutdown` Phase 2.5 drain（shutdown.rs + main.rs 接线）→ 剩余预算 join、到期 `ShutdownReport.background_drain_timed_out`（入 `any_timeout`→exit 6）+ `SHUTDOWN_TIMEOUT_MARKER` stderr——**如实 unconfirmed 不假安静**（exit-hook 伴测：30ms 预算→confirmed 空、unconfirmed=[run]，且报告**不取消** drive，释放后仍按原策略收束 completed）；完整退出策略=T07 未提前。
- **前台对照**：dropped foreground future → durable 诚实 running + `Abandoned` verdict 可查询（差异定义：断线≠取消是后台提交策略的属性）。

### 3.4 V4 run_lineage（怎么做 4）

- **四元身份持久化不可变**：`record_run_lineage` 单写者队列一事务内——run 须存在（InvalidRequest）+ owner/session 匹配（Conflict）+ **非终态**（「lineage 是创建事实，非死后注释」）；已存在则**完全相同=幂等 replay、不同=响亮 Conflict**；`load_run_lineage` 未知 origin=Corrupted——存储契约 2 测试对真实 RunDatabase 全断言（含拒写后原值完好）。
- **每 run 都记录**：drive_run 在 `record_run_started` 后立即 `record_run_lineage`（失败响亮 DriveError::Storage）；用户 run origin=user+cause=`request:{id}`，child 全四元（parent=父 run、source=父 ModelCallId、cause=父 ToolCallId）——A11 正主与生命周期测试逐一断言。
- **V1/V2/V3 指纹逐字节不动（独立重算）**：`479b0321…`/`64d7edfd…`/`3bd5388f…` 与已发布值完全一致；**V4=`371d415462b7e8d698693f50a7b9c9f493fbb4e653ee30e5d3e8930f77e161b5`（新）**= candidate-summary；migrations.rs 仅追加（diff 核实）；`supported_version`=last()=4 → migration_idempotency 防篡改/拒降级面自动纳入（2/2 在 580/0 内）。盘上对账：T06-E01/r02-script-probe.log S4 dump 显示真实迁移库 receipts==compiledIn 四指纹逐字节、userVersion=4。
- **V4 登记递延 T08**：`R02-T04_STORAGE_REGISTRY.json` 与 `r02_t04_storage_tx.sh` S4 硬编码 version==1 未同步——与 T04 R1-F1/T05 V3 **同根因**（该脚本在基线上即 exit 1，T05 审查有探针为证；本轮候选未引入新破坏，probe dump 已含 V4 对账事实供 T08 修复）。按派单指示一并递延 T08，非本 Task 义务。

### 3.5 T03 R1-D1 强制携带修复（task_supervisor.rs）

finding→根因→修改→验证四段映射齐全且真实：

- **finding**（T03 审查原文）：drain_run 到期分支 abort() 死代码（JoinHandle 被 take 后 move 进 timeout，到期再取必 None）。
- **修改**：timeout 前 `let abort_handle = join.abort_handle();`（task_supervisor.rs:421-427，附 R1-D1 注释）；到期分支 `abort_handle.abort()`（:452-460）——**真实 abort**；模块文档（「an ABORT is REQUESTED through an AbortHandle saved before the join — effective at the child's next yield point; reported unconfirmed」）、分支注释、tracing 文案（「abort requested through the saved AbortHandle; …no false quiet」）三处同步更正——四处「abort sent」失实声称全部消除（本审查全文 grep 核实无残留）。
- **验证**：新增回归 `drain_expiry_branch_really_aborts_a_yielding_child`（无树取消、drain 20ms 预算到期 → unconfirmed=1 **且** Drop 探针 500ms 有界等待内置位——修复前该子 30s 不 Drop，测试必红）；**重跑属实**：本审查复跑 `task_supervisor::` **7/0**（6+D1 回归 1）+ `--test cancellation_tree` **8/0** + workspace 全量 **580/0**（见 §5）。

### 3.6 怎么做 1–4 与三交付物对照

| 义务 | 结论 |
|---|---|
| 1 派发/回复/关闭→child run/thread + 可见性 + 实验默认值 | 满足（§2/§3.1；task-only 可见性、proactive_delegation=false、intercept 默认） |
| 2 父授权∩子工具范围；换模型/执行器不扩权 | 满足（§3.2；衰减 at dispatch + 逐 target at authorized 判定步；两伴测实测） |
| 3 同一 Supervisor + 连接寿命解耦 + 三断开差异 | 满足（§3.3；spawn_detached 同 supervisor；最小退出钩子接 graceful_shutdown） |
| 4 四元身份 + 不另建第二调度器 | 满足（§3.4；child/后台/未来 cron-Bridge 复用同一 drive_run 链 + origin 词表 + 后台登记面） |
| 交付①子代理运行适配 / ②后台提交接口 / ③权限衰减规则 | 三件齐备，均真实接线（§3.1/§3.3/§3.2） |

## 4. 测试有效性与卫生

- **无 mock 待测核心**：三个 service suite 全部经 `ServiceState::bootstrap_with_deps` 注入**真实组合根**（真实 SQLite 含 V4、真实事件/内核状态机/唯一 finalize/会话闸/配额/取消树/TaskSupervisor/subagent runtime 后绑定）；替身仅 Provider（脚本化轮次/门控停泊）与 Tool（RecordingTool 记录到达并返回成功 / ParkingTool 0 许可停泊）——**只产外部响应**，不写状态、不落库、不 finalize、不参与身份 mint。A11 的关键证明结构成立：替身**愿意执行 write**而从未被咨询（arrivals 通道断言）——拒绝出自运行层边界，非替身决定。
- **无空集合/永真/忽略断言**：五个新 suite 无 `#[ignore]`、无 should_panic 捕获；exit-hook 的 `confirmed==[]` 是「如实不确认」的语义断言（伴 unconfirmed=[run] 非空断言），非空集兜底。
- **确定性**：门控替身 + 通道锚点 + 1ms 轮询/5s 硬上限 `wait_until`（三 suite 同款）；无 sleep 掩盖竞态；父子/前后台真实并发（门控只定序观测点），未串行化。
- **A11 负控与升级尝试真实存在**：operate 档放行恰一次（§3.2）；read-only+显式 write 派发即拒零 child run；model 覆盖不扩权。
- **既有测试改动逐处判断（8 文件）**：6 处 `delegation: None` 字面补齐（cancellation_tree/late_result_fence/run_lifecycle/session_serialization 各 1、invocation_journal 2 处）+ shutdown_coordinator 5 处 `graceful_shutdown` 补传空 registry + kernel/sessions 两处测试 FakePort 补 lineage 两方法最小实现——均为 trait 满足/签名补参，**断言原文零改动、无删除/放宽/改永真、无 skipped**；既有 suites 计数逐一相同且在 580/0 内通过（对照 T03/T05 审查计数）。保护未降。（报告计数口径见 D-1。）

## 5. 实际复跑（全部经 rustup 1.98.1 + `--locked`；原始输出与退出码见 `T06-R01-review/`）

| 命令 | 退出码 | 结果 |
|---|---|---|
| `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 0 | 无 diff |
| `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | 0 | 0 error/warning |
| `cargo test --manifest-path rust/Cargo.toml --workspace --locked` | 0 | **580 passed / 0 failed / 0 ignored / 58 suites ok**（=执行者值；T05 558 + 22 = kernel subagent 9 + D1 回归 1 + A11 4 + A12 3 + 生命周期 3 + lineage 存储 2，逐项核对成立） |
| `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-contracts` | 0 | 56 生成文件 + API_COMPAT_MATRIX 626 entries 零漂移（lineage/authorization 不进 wire） |
| `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries` | 0 | DEP-07/08/D5 全 PASS |
| 定向：kernel `subagent::` / service `task_supervisor::` / `--test cancellation_tree` | 0 | **9 / 7 / 8** 全绿，命中>0 |
| 定向：`--test subagent_permission_inheritance` / `background_disconnect_recovery` / `subagent_lifecycle` | 0 | **4 / 3 / 3** 全绿 |
| 定向：adapters `--test run_lineage_store` | 0 | **2** 全绿 |

复跑后无 `/tmp/lingxi-r03t06-*` 遗留、无残留 lingxi 进程；审查期间 tracked-diff 摘要不变（代码冻结成立）。执行者证据 T06-E01/workspace-test.log 独立解析同为 58 suites/580/0。

## 6. 范围核对（不越界）

- **T07 未提前**：无启动扫描、无 RecoveryCoordinator、无逐任务 cancel/wait 完整退出策略（shutdown drain 为派单明示的最小退出钩子；durable active 行诚实保留归 T07 恢复扫描，多处注释/测试如实指向）。
- **R07 未提前**：无 cron/Bridge 实现；`execute_background_for` 为服务层 API（与 T02 `steer_for` 同先例，派单明示允许）；无新 HTTP/WS 路由（lib.rs diff 仅错误映射分支 + 注释；check-contracts 626 entries 零漂移佐证；auth 表零扰动）；后台终端接入/续接队列/持久线程登记如实递延 R06/R07。
- **无第二调度器**：child run 与后台 drive 均走同一 `RunSupervisor::drive_run` 单一链；SubagentRuntime/BackgroundDriveRegistry 为登记/回流/退出钩子面（无触发/定时/排队逻辑）。
- **Cargo 零变化、无 test-util**（§1）；npm/桌面栈零触碰（status 集仅 rust/ + docs/artifacts）。
- **generation 仍虚 1**（T04 O-4/T05 同源如实记录）。

## 7. 执行者报告准确性

逐项对账：候选摘要/逐文件 hash/Cargo 零变化/V1–V4 指纹、全部命令退出码与命中数、580/0=558+22、A11/A12/生命周期/lineage 证据内容（复跑复现）、D1 修复四段、F-1 递延边界、4 处映射偏差、未验证项清单（§9）——**与源码/复跑事实一致**。一处计数口径失实见 D-1。开发中三处测试自身缺陷的自述与最终测试形态自洽。

## 8. 缺陷（均非验收阻塞）与建议性观察

### D-1（MINOR，报告文档缺陷）：`delegation: None` 补齐处数报告为 7，实际 6

- **定位/重现**：报告 §4 第 1 条与 §5/§10 均称「7 处构造点补 `delegation: None`」；`git diff 6465395cf -- rust/ | grep -c '^+.*delegation: None'` = **6**（cancellation_tree 1、invocation_journal 2、late_result_fence 1、run_lifecycle 1、session_serialization 1）。
- **违反/后果**：仅报告计数口径失实（T03 D2 同类）；candidate-summary.txt 逐文件哈希与 tracked-diff 绑定值不受影响，无代码/测试影响，无需重跑。
- **根因**：推测终版前手数口径残留（7 或为把 shutdown 5 处或新 suite 内构造误计）。
- **修复**：报告再版时更正为 6 处（或给出第 7 处定位）。

### 建议性观察（非缺陷，不阻塞）

- **O-1（R04 绑定，映射保真度）**：父会话 **ask** 档下省略 access → Rust 坍缩为 Operate 档（kernel 单测显式锁定 `None+Ask→Operate`），child 的写类 target 放行；而现役栈继承 ask 后，写类工具在执行层经 `approvalPolicy:"deny_on_prompt"`（subagent 固定 `allowHumanApproval:false`）转为 `TOOL_APPROVAL_UNAVAILABLE` 结构化拒——`subagent-tool-policy.ts` 头注明确「不在策略层坍缩成 operate，由执行层 deny_on_prompt 拒」。Rust 的两态衰减本身与 `isReadOnlyPermissionMode`/`legacyAccessModeFromPermissionMode`（ask→operate）一致，A11 验收场景（只读父）完全正确；但 ask 档子的「审批不可用即拒」残余差距未被列入 4 处映射偏差。归属 R04 工具策略网关（approvalPolicy 面在 R03 尚未接线，T03 审批门生产默认 None）。建议随 R04 需求登记或补入偏差台账。
- **O-2**：`spawn_child` 拒绝路径的 `rollback_thread` 恢复 busy=false 但不回滚已覆写的 `thread.tier`（reply 失败后线程记录保留新 tier）。下次 reply 会重新对当前父档衰减，**无扩权可能**，仅状态一致性瑕疵。
- **O-3**：runs.rs `subagents` 字段注释称未绑定 launcher 时委派「loud tool failure」；实际 `subagent`/`subagent_reply` 未绑定 → `DriveError::Internal`（响亮 **run** 失败），仅 `subagent_close` 映射为工具失败。两者均响亮、无静默，注释对失败形状描述不准。
- **O-4**：委派**拒绝**路径（escalation/ThreadBusy/超限等）journal 走共享 `journal_receipt_of`，Failed 收据 `dispatched=true` 而实际零 child run 派发（detail 携带拒绝码可辨；逐 target 授权拒绝路径已正确 `dispatched=false`）。恢复分类为 ConfirmedSettled（不盲重试），无正确性影响；R04 网关接管时可一并收紧。
- **O-5**：A11 model-override 伴测尾部 `let _ = read_tool_request();` 为死构造（cosmetic）；child spawn 任务内 `note_child_finished` 之前的 panic 会泄漏并发计数（当前驱动链不 panic，理论边界）。

## 8b. 受环境限制

本机 arm64 macOS 单平台；无真实供应商/真实桌面宿主（A11/A12 用确定性替身与真实服务组合根，符合派单与 05 §1 替身边界）；跨平台/正式打包验证不在本 Task 范围。

## 9. 结论

R03-T06 到期义务（阶段书怎么做 1–4、三交付物子代理运行适配/后台提交接口/权限衰减规则、A11、A12、总控细化全部条目、T03 R1-D1 强制修复）均有有效证据且真实接线成立：派发/回复/关闭映射到同一 RunSupervisor 的 child run（父树链接 + 线程登记 + 衰减 + 30min 开跑锚点超时 + deliver_retained 回流）；权限衰减落在真实运行层授权边界（被拒调用零派发、替身零咨询、负控证明档位裁决、换模型不扩权）；后台与前台共享 admission 链并以 detached 监督任务解耦连接寿命（断线窗口持续执行、同 requestId 幂等 replay 零重执行、可查询、退出钩子如实 unconfirmed）；V4 lineage 四元身份持久化不可变且 V1/V2/V3 指纹逐字节不动（独立重算）；D1 修复真实（保存 AbortHandle + Drop 探针回归）。回归满足（workspace 580/0 = T05 558 + 22，四门禁 exit 0，全部由本审查独立复跑）；范围未越界（T07/R07/R04 未提前，无第二调度器，Cargo 零变化，无 test-util）；8 个既有测试文件改动均为字面补齐、保护未降。冻结语义映射经现役 Node 源码逐点位核实忠实，4 处自报偏差有据如实。发现 1 项非阻塞 MINOR 报告计数缺陷（D-1）与 5 项建议性观察（O-1 ask 档残余差距应随 R04 登记），无未关闭验收阻塞缺陷。

**VERDICT: PASS**
