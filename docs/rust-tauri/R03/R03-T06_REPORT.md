# R03-T06 报告｜子代理、后台运行与权限继承（EXECUTOR-R03-T06-E01）

- 状态：**READY_FOR_REVIEW**（执行者口径；独立复核归属总控另派）
- TASK_ID：R03-T06（ACCEPTANCE_IDS：R03-A11、R03-A12）
- TASK_BASE_SHA：`6465395cf8e84121ea55b1481afc5711a4d707e8`（分支 `codex/rust-tauri-migration`，无 commit/push；工作树候选留给总控冻结）
- 执行时间：2026-09-29（证据时间见 candidate-summary.txt / 各 log）
- 环境：macOS darwin 27.0.0 arm64；rustup 锁定工具链 **1.98.1**（全部命令经 `~/.cargo/bin` rustup 代理 + `--locked`；证据链统一 `CARGO_TARGET_DIR=/tmp/r03-t06-target`）；SQLite=rusqlite 0.40.2 bundled；无网络外发、无真实供应商；npm/桌面栈零触碰；**tokio test-util feature 未引入**（无新依赖，`rust/Cargo.lock` 零变化 = `90111c4b…` R02_HANDOFF 原值；确定性方案：门控替身 + 通道锚点 + 1ms 轮询/5s 硬上限的有界真实等待——T02–T05 同一先例）
- 交付物三件：**子代理运行适配**（`lingxi-service/src/subagents.rs`：dispatch/reply/close 映射到同一 RunSupervisor 的 child run + 线程登记 + 派发期衰减 + 父树链接 + 结果回流）、**后台提交接口**（`SessionStore::execute_background_for` + `lingxi-service/src/background.rs`：同一 Supervisor 的 detached 驱动 + 有界登记 + 最小退出钩子）、**权限衰减规则**（`lingxi-kernel/src/subagent.rs`：两态档位、#1614 衰减拒绝、逐 target 授权边界、四元 lineage 身份）
- **强制携带修复 T03 R1-D1 已完成**（§7：finding→根因→修改→验证映射）

---

## 1. 冻结语义溯源（先读 Node 生产栈真实实现，再映射——不发明交互）

「现役 subagent」语义全部从 90 号来源顺藤实读本仓源码后映射；文件:行 为本仓当前源码：

| 冻结语义 | Node 生产栈证据（文件:行） | Rust R03-T06 映射 |
|---|---|---|
| **派发=fire-and-forget，立即返回 taskId/threadId** | `lib/tools/subagent-tool.ts:286-662`（execute：`store.defer(taskId,…)` + `deliveryIntent:"trigger_parent_turn"` :389；runPromise 挂 then/catch 不 await） | 驱动层 target=`subagent` 的 ToolRequests 分支（runs.rs）：`SubagentLauncher::dispatch` 分配 child run id、挂父取消树 `spawn_linked(parent_scope.child(ChildRun))` 后**丢弃 ChildHandle**（登记处可回收），父工具调用立即返回 Success{content_digest=child run id}；journal 收据 Succeeded+dispatched=true+dedup_id=child run id |
| **回复=续接同会话的 OPEN 线程** | `subagent-tool.ts:666-940`（subagent_reply：校验 direct/open/同父会话；`resumeSessionPath`） | `SubagentLauncher::reply`（subagents.rs）：thread 存在/同会话/open/非 busy 校验（NotFound/NotInSession/NotOpen/ThreadBusy 稳定错误码）→ tier 重解析（显式>线程档>继承，再对当前父档衰减）→ 新 child run，lineage.parent_run_id=发起 reply 的 run |
| **关闭=关闭同会话 OPEN 且非 busy 线程** | `subagent-tool.ts:942-999`（closeDirectThread，reason 入 summary） | `SubagentLauncher::close`：同三校验 → `ThreadStatus::Closed`，reason 记为线程 summary 锚（`closed: {reason}`） |
| **权限档两态 + 显式>继承** | `lib/tools/subagent-tool-policy.ts:65-73`（read→READ_ONLY；write→OPERATE 仅父非只读；省略→继承父档 normalize） | kernel `resolve_subagent_access(explicit, parent_mode)`（subagent.rs）：同矩阵；省略=继承并坍缩两态；父档缺失=OPERATE（Node `resolveInheritedMode(null)`） |
| **衰减拒绝 #1614：父只读+write 显式报错** | `subagent-tool-policy.ts:53-63`（`SubagentAccessDeniedError`，code `SUBAGENT_WRITE_DENIED_BY_PARENT_READ_ONLY`，不静默降级） | kernel `SubagentAccessDenied{code, message}`（code 逐字保留，message 含出路「switch…or re-dispatch with access:"read"」）→ 驱动层映射为 Forbidden 工具错误；A11 escalation 伴测实测：**零 child run 创建** |
| **真实拒绝边界=运行层逐工具判定（非替身决定）** | `core/session-permission-mode.ts:375-431`（classifySessionPermission，`isSubagent` 上下文 :382）→ `:221-236` blockedByReadOnly（layer `subagent_access`，文案含「a subagent's permission can never exceed its parent session」）→ `lib/tools/session-permission-wrapper.ts:764-799`（deny→toolError） | kernel `authorize_child_tool(tier, target)` + 驱动层在 **T05 journal authorized 判定步**应用（runs.rs 工具循环）：`RunGrant::Subagent` 下每个 target 先裁决；Denied → **不 advance Authorized、不 started、不派发执行器**，journal 收据 Failed+dispatched=false+detail=拒绝原因，tool_call_completed 事件携带结构化 Forbidden；`RunGrant::Full`（用户 run）保持 T05 原语义不变 |
| **子上下文固定禁用面** | `session-permission-mode.ts:80-105`（SUBAGENT_BLOCKED_TOOLS 全集：扇出/长期记忆写/自动化/对外副作用/ask_user/loop_control/knowledge_manage） | kernel `SUBAGENT_BLOCKED_TARGETS` 全集镜像 + **新增 `subagent_reply`/`subagent_close`**（结构性防递归：Node 靠隔离子会话使线程不可达，R03 child 共享父会话行，必须由边界显式拒绝——如实登记的映射偏差，见 §9） |
| **只读档白名单 + 未知 fail-closed** | `session-permission-mode.ts:362-368`（FILE_READ_ACTIONS 放行）+ READ_ONLY 默认 blockedByReadOnly | kernel `SUBAGENT_READ_ONLY_TARGETS`（read/grep/find/ls/glob/web_search/web_fetch/todo_read/current_status/knowledge_read/grep/outline）+ 只读档下不在白名单一律拒 |
| **消息可见性：子只看 task 文本；子可读父会话文件** | `subagent-tool.ts:257`（task 参数描述明示「cannot see the current conversation history unless you include it here」）+ `:482` `fileReadSessionPaths:[parentSessionPath]`（session-coordinator.ts:8286 落地） | child run 的 input=delegation.task **原文**（无父历史注入；测试替身按 input marker 区分父子证明）；文件读取面=R04/R06 工具，R03 如实递延；父子关系由 lineage 持久化承载 |
| **实验开关默认值** | `lib/experiments/registry.ts:15` `PROACTIVE_SUBAGENT_EXPERIMENT_ID="subagent.proactive_delegation"`，`defaultValue:false` | kernel `SubagentPolicy{proactive_delegation:false, tool_strategy:Intercept}`（`LINGXI_SUBAGENT_TOOL_STRATEGY` 默认甲 intercept 同 :36-38）；经 `ServiceDeps.subagent_policy` 注入，退化值响亮拒启 |
| **并发 10/会话、20 全局、30 分钟超时（开跑才计时）** | `subagent-tool.ts:228-229`（MAX_PER_SESSION/MAX_GLOBAL）、:27（30min）、:429-431/:461（timer 在 executeForAgent 开跑时起） | `SubagentPolicy{per_session_limit:10, global_limit:20, timeout_ms:30*60*1000}`：dispatch 时 per-session+global 计数闸（超限 SessionLimit/GlobalLimit 响亮）；child drive 包 `timeout_at(开跑锚点)`，到期 cancel child scope → child 走自身四相取消结算（不 drop future） |
| **父取消传播到子（abortByParentSession）** | `docs/refactor-2026/P02/STATE_TRANSITIONS.md:42`（T8：该父下全部活跃→aborted） | `CancelRegistry::register_linked(run_id, parent_scope)`（cancel.rs 新）：child run 的取消根挂父 scope 之下——父取消向下急切传播、子取消不反向。~~child 由自身 drive 观测并经唯一 finalize 落 cancelled（T03 机制不动）~~ **【更正 2026-09-29，STAGE-REPAIR-R03-G01-F01，依 R03-T08_REVIEW_R1 §7 FINDING-1 裁决】**原表述在父取消路径不成立：spawn_linked 包装器的 biased select 在树取消分支先就绪时**丢弃 child drive future**——监督层子任务立即停止（A06 成立：live_children_of(parent) 为空、child 永不 completed、无 final message），drive 不会经自身 finalize 落 cancelled；child run 的 durable 行保持 active，由下一进程启动扫描按 T07 两阶段闭环诚实收口为 interrupted_needs_attention（重启闭环已实测，T08 组合矩阵崩溃恢复案例）。child **自身超时路径**（上行 timeout_at 锚点）仍正常经自身四相取消落 cancelled。原句以删除线保留供追溯，产线修复归属见 R03_HANDOFF known_gaps（R04/R06 触碰或总控另派）。 |
| **结果回流父会话（trigger_parent_turn）** | `subagent-tool.ts:389`（deliveryIntent）+ DeferredResultStore | child 结束 → `SessionSupervisor::deliver_retained`（session_supervisor.rs 新）：忙会话的当前 run 下一模型轮 drain；闲会话**保留**给下一 run 首轮（T02 leftover 语义）；主动触发新父轮=R06/R07（R03 无自动提交循环，如实边界） |
| **子并发车道与父隔离（隔离会话语义）** | Node：child 跑在隔离 session（`prepareIsolatedSession`），不与父会话争流控 | drive_run 新 `quota_session_lane` 参数（runs.rs）：用户提交传 session 本身（语义不变）；child 传 `{session}::subagent::{thread}` + 独立 agent lane `subagent:{agent}`——父停在模型读时占住会话唯一 model 车道不会饿死 child（反之亦然）；总量另受 10/20 闸约束 |

**四元身份持久化（怎么做 4）**：kernel `RunLineage{parent_run_id, origin, source_message_id, cause_id}` + `RunOrigin{User,Subagent,Cron,Heartbeat,Bridge}`（wire 词表稳定，未知值响亮 Corrupted）；**每个 run 都记录**：用户 run origin=user+cause=request_id 锚（`request:{id}`），child 全四元（parent=父 run、source=父发出委派请求的 ModelCallId、cause=父的委派 ToolCallId）。存储 V4 `run_lineage` 表（§3）；Bridge/cron 在 R07 复用同一 origin 词表+后台登记面——**不另建第二调度器**（后台 child 与未来 cron 走同一 `RunSupervisor::drive_run` 单一链）。

## 2. 实现与调用链（真实接线）

### 驱动链（runs.rs 生产链改动点）

```
HTTP POST /lingxi/v1/sessions/{id}/execute                     (lib.rs，未动)
  → SessionStore::execute_submission_for / execute_background_for (sessions.rs)
      归属/NotFound/Forbidden → busy gate → requestId 去重（admit_submission 共享抽取）
      用户 run：DriveAuthorization{lineage: user_submission(request_id), grant: Full}
  → RunSupervisor::drive_run(port, events, principal, session, agent, run, input,
                              gen, now, steering, parent_scope, authorization, quota_lane)
      ① record_run_started（T01 未动）
      ② record_run_lineage（新：run 行创建后立即落四元身份）
      ③ 模型 turn → ToolRequests：逐调用
          journal intent(prepared)（T05 未动）→ tool_call_started（T01 未动）
          ④ R03-T06 授权边界：grant=Full → Allowed（T05 语义不变）
                         grant=Subagent{tier} → authorize_child_tool(tier, target)
             Denied → journal receipt Failed+dispatched=false+原因 → 工具事件 Forbidden → continue
          ⑤ 审批门（T03 未动，拒绝路径零执行语义保留）
          ⑥ 委派分支（request.delegation 携带）：
             subagent        → launcher.dispatch（衰减→并发闸→分配 child run→挂父树→spawn）
             subagent_reply  → launcher.reply（线程校验→tier 重解析→续接 child run）
             subagent_close  → launcher.close（校验→Closed，不建 run）
             其他 target 带委派载荷 → InvalidTarget 响亮
          ⑦ 其余 target：advance(Started) → spawn_linked 执行（T03/T04/T05 未动）
      终态：唯一 finalize（T01，未动）
```

### 子代理运行时（lingxi-service/src/subagents.rs，新）

- `SubagentRuntime{storage:Arc<RunDatabase>, events, sessions, policy, supervisor:OnceLock<Weak<RunSupervisor>>, state}`——组合根构造、bootstrap 后 `bind_supervisor(Arc::downgrade)`。
- **`SubagentLauncher` trait（trait object）**：RunSupervisor 持 `Weak<dyn SubagentLauncher>`、runtime 持 `Weak<RunSupervisor>`——双向互指若为具体类型会使 Send/Sync auto-trait 成环（编译器实测报错）；trait object 的声明式 Send+Sync 打断传播，无 unsafe impl。
- dispatch/reply/close 见 §1 映射表；失败路径（分配失败/未绑定/spawn 拒绝）全部回滚并发计数与线程登记（**零 child run 创建**）。
- child drive：`timeout_at(开跑锚点)` 包裹；`RunAuthorization=DriveAuthorization{lineage(四元), grant:Subagent{tier}}`；steering=None（子不可交互）；quota lane 隔离（§1 末行）。
- 结果回流：`deliver_retained`（bounded；满载=响亮 error log，run 自身 durable 事件为权威记录，线程快照可查询——不静默丢）。

### 后台提交接口（sessions.rs `execute_background_for` + src/background.rs，新）

```
execute_background_for(storage:Arc, events:Arc, supervisor:Arc, registry, principal, session, submission, now)
  = 与前台完全相同的 admit_submission（归属→busy→requestId 去重，Replay/Conflict 语义复用 T04）
  → background::spawn_background_drive(...)：
      supervisor.task_supervisor().spawn_detached("background_drive:{run_id}", drive)   ← owner=None
      会话 lease move 进 drive（任何退出路径释放）；authorization=user_submission(request_id)
      registry 有界登记（cap 1024，先收割已完成）
  → 立即返回 ExecuteAccepted
```

**三种断开差异（阶段书怎么做 3 的定义）**：
- **客户端断线**：后台 drive 为 detached 监督任务（无 owner）——断线无法触及；重连方以同 requestId 重提交 → T04 幂等 Replay（原 run id 返回，零重执行），run 状态/终态事件/events page 可查询（A12 实测）。前台 execute 仍是内联 await（断线=request future 消失→guard 触发树→durable 行诚实 active+Abandoned verdict——A12 对照组实测，恢复分类归 T07）。
- **桌面关窗**：桌面是独立服务进程的又一客户端（目标契约 02 §1「关闭一个客户端不等于取消所有任务」）——运行层语义与客户端断线相同（Tauri 宿主生命周期=R09，不在本 Task 造语义）。
- **服务退出**：`graceful_shutdown` 新增 background drain 相位（shutdown.rs，`ShutdownReport.background_drain_timed_out` 新字段→exit code 6）：剩余预算内 join 活跃后台 drive，到期**如实报 unconfirmed**（不假称安静；durable 行保持 active 归 T07 恢复扫描）。完整退出策略（逐任务 cancel/wait）=T07。

### 存储层（V4 增量迁移）

- `run_lineage` 表（FK→runs；`idx_run_lineage_parent` 部分索引）——`record_run_lineage`/`load_run_lineage`（StoragePort 新两方法，run_store.rs 经同一单写者队列每写一事务）：run 须存在+owner 匹配+**非终态**（lineage 是创建事实，非死后注释）；**不可变**——完全相同=幂等 replay，不同=响亮 Conflict（父系不可改写）；未知 origin 词表=Corrupted。
- **指纹**：V1=`479b0321…`、V2=`64d7edfd…`、V3=`3bd5388f…` 均与已发布值逐字节一致（零改动）；**V4=`371d415462b7e8d698693f50a7b9c9f493fbb4e653ee30e5d3e8930f77e161b5`（新）**。r02-script-probe S4 dump 佐证盘上 receipts==编译内（userVersion=4）。
- migration_idempotency 内部测试零改动（T04 已区间化 supported_version，V4 自动纳入幂等/防篡改/拒降级面）。

## 3. 逐验收：预期 vs 实测

### R03-A11 子代理不能升级权限 — **PASS（真实授权边界产生拒绝；证据=权限继承测试 4 项）**

`tests/subagent_permission_inheritance.rs`（真实 bootstrap 组合根 + 门控替身只产外部响应）：

- **正主 `r03_a11_readonly_parent_child_write_is_refused_by_the_real_boundary`**：父会话经归属检查面 `set_permission_mode_for` 切 read_only → 父 run turn1 请求 subagent（access 省略→继承只读档，衰减允许收缩）→ child run 创建（lineage 四元全断言：parent_run_id=父、origin=subagent、source=`{parent}-mc*`、cause=`{parent}-tc*`）→ child turn1 请求 **write** → **运行层授权边界拒绝**：
  - journal 收据：`dispatched=0`、detail 含 `ACTION_BLOCKED_BY_READ_ONLY` 与「never exceed its parent session」（**原因保留**）；
  - tool_call_completed 事件携带同码结构化 Forbidden（模型可见，镜像 Node toolError）；
  - **执行器替身零调用**（arrivals 通道断言——拒绝来自驱动层边界，替身从未被问询；替身本身会执行 write，证明不是替身决定）；
  - child 以 completed.with_final 收束（拒绝=记录的工具失败，不杀 run）；父 run completed。
  - **父子关系保留**：`run_lineage_for`（归属检查查询面）返回四元。
  - **结果回流**：child 结束→deliver_retained 落 retained steering（父已收束）→ 会话**下一 run** 首模型轮 input 断言含 `[subagent-result thread=…`（T02 leftover 语义承接 trigger_parent_turn 的 R03 保真）。
- **负控 `operate_parent_child_write_is_executed_once`**：同形状、父档默认 OPERATE → 同一 write target **放行**、执行器恰一次——拒绝是**授权档的裁决**而非全面封锁。
- **升级尝试 `readonly_parent_write_access_request_is_refused_at_dispatch`**：父只读+显式 access=write → **派发即拒**（SUBAGENT_WRITE_DENIED_BY_PARENT_READ_ONLY）：父工具事件/journal 携带该码，会话 run 数=1（**零 child run**），线程登记为空。
- **换模型不能扩权 `model_override_cannot_widen_the_readonly_grant`**：委派带 model 覆盖（other-provider/bigger-model）→ child 仍只读档，write 仍拒、执行器零调用。换执行器同理：授权判据仅 grant（绑定 child run），与 per-call executor/model 无关（执行器在拒绝路径根本不被调用）。

### R03-A12 断线不等于取消 — **PASS（证据=断连恢复测试 3 项）**

`tests/background_disconnect_recovery.rs`：

- **正主 `r03_a12_background_run_survives_disconnect_reconnect_replays_and_is_queryable`**：
  1. `execute_background_for`（requestId="reconn-1"）**立即返回**（run 驱动已 detached）；
  2. run 到达工具 I/O 停靠点后断言：监督登记中存在 `run_id=None` 的 `background_drive:{run_id}` 任务（**结构上无任何客户端/连接可达**）+ registry live_ids 含该 run + durable status=running（**断开窗口内按原策略持续执行**——此刻不存在任何 caller）；
  3. **重连**：同 requestId+同内容再提交 → `replayed=true`、原 run id 返回、会话 run 数=1、停泊工具 arrivals 无第二条（**不重复提交**）；同 id 改内容 → `DuplicateRequestConflict`（T04 护栏）；
  4. 释放停靠 → **同一 drive** 按原策略经唯一 finalize 收束 completed.with_final；
  5. **可查询**：durable 终态事件（run_state_changed completed.with_final）+ `events_page` 表面返回该 run 流（重连客户端的真实查询面）。
- **对照组 `foreground_drive_disappearing_is_abandoned_not_cancelled`**：前台提交的 request future 被丢弃（断连形态）→ durable 行诚实保持 running + 取消登记 `Abandoned` verdict 可查询——**差异定义**：断线≠取消是后台提交策略的属性，不是所有 run 的属性（前台断连仍诚实 abandoned，恢复=T07）。
- **退出钩子 `exit_hook_drain_reports_live_drives_as_unconfirmed`**：活跃后台 drive + 30ms drain 预算 → confirmed 空、unconfirmed=[run]（**不假称安静**）；报告**不取消** drive——释放后仍按原策略收束 completed。

### 子代理生命周期（必须交付「子代理运行适配」的完整面）— PASS

`tests/subagent_lifecycle.rs` 3 项：dispatch→reply→close 全链（三 run：父+两 child 同线程；continuation child 的 lineage.parent_run_id=发起 reply 的父 run、cause=其 reply 工具调用；close 后线程 Closed+reason 入 summary）；unknown thread 回复=结构化 not_found 拒绝（零 run 创建）；busy 线程回复=结构化 conflict 拒绝（并发计数/线程状态断言）。

### 存储契约 — PASS

`lingxi-adapters/tests/run_lineage_store.rs` 2 项：四元往返+不可变（相同 replay/改写 Conflict/拒写后原值完好）+ 守卫（无 run 行 InvalidRequest/终态拒记/未知 origin Corrupted）。

### 任务书「怎么做」1–4 对照

1. 派发/回复/关闭映射 child run/thread + 可见性规则 + 实验开关默认值 ✔（§1 映射表；task-only 可见性=child input 原文；proactive_delegation 默认 false）
2. 父授权∩子工具范围，换模型/执行器不扩权 ✔（衰减 at dispatch + 逐 target 边界 at authorized 判定步；两伴测）
3. 同一 Supervisor + 连接寿命解耦 + 三种断开差异 ✔（spawn_detached 同 supervisor；§2 差异定义 + 最小退出钩子接入 graceful_shutdown）
4. 四元身份保留 + 不另建第二调度器 ✔（V4 表+每 run 记录；Bridge/cron=R07 复用 origin 词表与同一 drive 链/后台登记面）

## 4. 修改文件清单

修改（18）：`rust/crates/lingxi-kernel/src/{lib.rs,ports.rs}`、`lingxi-adapters/src/storage/{migrations.rs,run_store.rs}`、`lingxi-service/src/{lib.rs,main.rs,runs.rs,cancel.rs,session_supervisor.rs,sessions.rs,shutdown.rs,task_supervisor.rs}`、`lingxi-service/tests/{cancellation_tree.rs,invocation_journal.rs,late_result_fence.rs,run_lifecycle.rs,session_serialization.rs,shutdown_coordinator.rs}`。
新增（7 rust 路径）：`lingxi-kernel/src/subagent.rs`（生产+9 单测）、`lingxi-service/src/{subagents.rs,background.rs}`（生产）、`lingxi-service/tests/{subagent_permission_inheritance.rs,background_disconnect_recovery.rs,subagent_lifecycle.rs}`（永久测试 4+3+3）、`lingxi-adapters/tests/run_lineage_store.rs`（永久测试 2）。
`rust/Cargo.lock` 零变化（=R02_HANDOFF `90111c4b…`）；三个 crate Cargo.toml 零变化（无新依赖）。逐文件 SHA-256 与 tracked-diff 绑定值见 candidate-summary.txt。

### 既有测试夹具改动（逐处+理由；无断言删除/放宽/改永真，无 skipped）

1. **`ToolRequest` 新增 `delegation: Option<DelegationRequest>` 字段**（kernel ports）→ 5 个既有测试文件的 7 处构造点补 `delegation: None`（trait 满足编译的字面补齐；既有断言原文零改动）。
2. **`graceful_shutdown` 新增 `background` 参数**（最小退出钩子接线）→ shutdown_coordinator.rs 5 处调用点补传空 registry（该夹具不驱动后台 run；drain 相位由 A12 退出钩子测试覆盖）。
3. kernel ports.rs / sessions.rs 两处测试 FakePort：补 `record_run_lineage`/`load_run_lineage` 最小实现（T03/T05 同先例——trait 满足编译；耐久行为由真实 adapter 测试 run_lineage_store.rs 覆盖）。
4. **无其他夹具改动**——run_lifecycle/cancellation_tree/session_serialization/late_result_fence/invocation_journal(×2)/execute_concurrency/event_subscription/request_dedup 等断言原文未动，全部在 580/0 全量内通过。

## 5. 验证命令与退出码（全部经 rustup 1.98.1 + `--locked`；verify-stage R03 未注册，未运行未伪造）

| 命令 | 退出码 | 结果摘要 |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | 无 diff（开发中首查报 diff 后 `cargo fmt --all` 应用于候选；终查干净——gates.log） |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 0 error/warning |
| `cargo test --workspace --locked` | 0 | **580 passed / 0 failed / 0 ignored / 58 suites ok**（T05=558/54；+22 = kernel subagent 9 + task_supervisor D1 回归 1 + A11 4 + A12 3 + 生命周期 3 + lineage 存储 2；含 R02 全量回归） |
| `cargo run -p xtask -- check-contracts` | 0 | 56 生成文件 + API_COMPAT_MATRIX 626 entries 零漂移（lineage/authorization 不进 wire 生成物） |
| `cargo run -p xtask -- check-boundaries` | 0 | DEP-07/08/D5 全 PASS |
| `bash scripts/rust-tauri/r02_t04_storage_tx.sh <probe>`（非门禁探针） | 1 | S1–S3 PASS；S4 仍死于硬编码 version==1（**T04 R1-F1 原样，非新破坏，递延 T08**）；S4 dump 给出 V4 盘上收据==编译内（`371d4154…`）、userVersion=4 的对账事实 |

定向过滤器（全部命中>0，filters.log）：subagent_permission_inheritance 4、background_disconnect_recovery 3、subagent_lifecycle 3、run_lineage_store 2、kernel `subagent::` 9、`task_supervisor::` 7；回归子集 run_lifecycle 12、session_serialization 5、execute_concurrency 3、event_subscription 12、request_dedup 4、late_result_fence 5、invocation_journal 3、invocation_journal_store 5、migration_idempotency 2、storage_transactions 7、shutdown_coordinator 7、lingxi-kernel lib 44（T05=35，+9）。复跑后无 `/tmp/lingxi-r03t06-*` 遗留、无残留测试进程。

## 6. 证据位置

`artifacts/rust-tauri/R03/T06-E01/`：`gates.log`（fmt+clippy）、`workspace-test.log`（580/0 原始输出）、`xtask.log`（contracts+boundaries）、`d1-fix-rerun.log`（D1 修复后 task_supervisor 7/0 + cancellation_tree 8/0）、`a11-a12-lifecycle.log`（五个新 suite --nocapture 原始输出 + EXIT）、`filters.log`（定向+回归计数）、`r02-script-probe.log`（S4 dump 含 V4 对账）、`candidate-summary.txt`（基线/HEAD/逐文件 sha256/Cargo.lock 零变化/迁移指纹/tracked-diff sha256）。

## 7. D1 修复映射（T03 审查 R1-D1 强制携带）

- **finding**：`task_supervisor.rs drain_run` 到期分支 abort() 死代码——JoinHandle 在 timeout 前被 `take()` move 进 `tokio::time::timeout(remaining, join)`，到期 `Err(_elapsed)` 分支再取 `entry.handle`（必为 `None`），`handle.abort()` 不可达；模块文档/分支注释/`tracing::warn!`（「abort sent; no false quiet」）与 T03 报告四处声称一个不会发生的 abort。
- **根因**：句柄所有权滑移——JoinHandle 被 timeout future 消耗后才尝试 abort（审查 R1-D1 判定一致）。
- **修改**：timeout 前 `let abort_handle = join.abort_handle();`（task_supervisor.rs drain_run，附 R1-D1 注释）；到期分支 `abort_handle.abort()`——**真实 abort**（经保存的 AbortHandle；在 wrapper 的下一 yield 点生效）；同步更正模块文档（「an ABORT is REQUESTED through an AbortHandle saved before the join — effective at the child's next yield point; reported unconfirmed」）、到期分支注释与 tracing 文案（「abort requested through the saved AbortHandle; it takes effect at the child's next yield point; no false quiet」）——请求≠已观测停止，报告仍为 unconfirmed（不假称安静语义保留）。
- **验证**：新增回归测试 `drain_expiry_branch_really_aborts_a_yielding_child`——无树取消、drain 自身 20ms 预算到期 → 断言 unconfirmed=1 **且**子 future 的 Drop 探针在 500ms 有界等待内置位（修复前 abort 不可达，future 30s 不 Drop，测试必失败）；重跑 `cargo test -p lingxi-service --lib task_supervisor::`（7/0，含新回归）+ `--test cancellation_tree`（8/0）+ workspace 全量（580/0）——d1-fix-rerun.log / workspace-test.log。

## 8. 测试替身与边界（如实声明）

- **替身**：ScriptedProvider（按 input marker 脚本化 turn、可选门控停泊、记录 served inputs）、RecordingTool（仅记录到达的 target 并返回成功——**它会执行 write**，用于证明拒绝非替身决定）、ParkingTool（0 许可信号量停泊）。全部经 `ServiceState::bootstrap_with_deps` 注入真实组合根；不写状态、不落库、不 finalize、不参与身份 mint。
- **断言面**：DB 直查（query_one_text）+ 真实服务面（set_permission_mode_for/run_lineage_for/events_page/cancel 面）+ 运行时登记（threads_of/active_counts/task registry）。
- **确定性**：门控 + 通道锚点 + 1ms 轮询/5s 硬上限有界等待；无 sleep 掩盖竞态；并发未被串行化（父子真实并发，门控只定序观测点）。
- **开发中修正的测试自身缺陷**（如实）：A11 正主初版在 execute 内联 await 停泊父轮（自锁）→ 改 next-run 回流形态；生命周期测试门控停泊先于线程 id 解析的次序错误 → 解析移入门后；journal/事件断言先于 child 收束的次序错误 → 前置收束等待。均修正后全绿（缺陷在被测链外的测试编排）。

## 9. 未验证项 / 边界（如实）

- **V4 注册登记递延 T08**：`R02-T04_STORAGE_REGISTRY.json` migrations 数组与 `r02_t04_storage_tx.sh` S4 硬编码 version==1 未随 V4 同步——与 T04 R1-F1/T05 V3 同根因，按派单指示**一并递延 T08**（探针在当前树实测仍死于同一断言，S4 dump 已含 V4 对账事实供 T08 修复）。
- **映射偏差（有据）**：① `subagent_reply`/`subagent_close` 加入子上下文禁用面——Node 靠隔离子会话使线程从子内不可达，R03 child 共享父会话行，结构性防递归必须显式（kernel 注释登记）；② busy 线程的 reply=响亮 ThreadBusy 拒绝——Node `runSerialized` 排队续接，后台续接队列随 R07 入口工作递延；③ 线程登记为进程内存（Node 持久 thread store）——durable 事实=run 行+lineage，持久线程续接=R06/R07；④ 父会话权限档存于 SessionSupervisor 登记处（Node 引擎内存同构），R06 持久化。
- **后台提交传输入口递延 R07**：`execute_background_for` 为服务层 API（与 T02 `steer_for` 同先例——不发明新 HTTP/WS 路由，auth 表/API 矩阵零扰动）；后台任务的终端接入（CLI/WS/定时）=R07。
- **不提前做的**：T07 启动扫描/恢复协调/完整退出策略（本 Task 只定义差异+最小退出钩子+unconfirmed 如实报告）；R04 完整工具策略网关（授权边界复用 T05 判定链的位置即其接手点）；R06 会话语义（子会话上下文/runSplit 投影）；R05 真实供应商。
- **generation 仍虚 1**（T04 O-4/T05 同源）：lineage/journal 如实记录该值。
- 无跨平台（本机 arm64 macOS）；npm/桌面栈未触碰。

## 10. 独立复核重点建议

1. **拒绝的出处**：runs.rs 工具循环 ④ 步——`RunGrant::Subagent` 分支先于 advance(Authorized)/审批/执行；被拒路径 journal `dispatched=0` 且替身 arrivals=0（A11 正主两断言）——替身从未被咨询。
2. **衰减矩阵与 #1614**：kernel `resolve_subagent_access`（显式 read/write/省略 × 三父档）+ 派发期升级拒绝零 child run（escalation 伴测 run 数=1）。
3. **quota lane 隔离的正确性**：drive_run `quota_session_lane` 参数——用户路径传 session（语义不变，run_lifecycle 等回归全绿佐证）；child 传隔离 lane（否则父停泊在模型读会饿死 child——该动机注释在 subagents.rs）。
4. **断连语义的差异面**：后台=detached+registry+replay；前台断连=诚实 abandoned（对照组）；退出钩子=unconfirmed 如实（不取消、不假安静）。
5. **D1 修复真实性**：task_supervisor.rs 的 `abort_handle` 保存点 + 回归测试的 Drop 探针（无树取消、纯 drain 预算到期路径）。
6. **V1/V2/V3 指纹不动**：独立重算 fingerprint_sql 与已发布值比对；V4 盘上==编译内（probe S4 dump）。
7. **580/0 对照**：T05 558 + 22 逐项核对（§5）；7 处 `delegation: None` 与 5 处 shutdown 夹具补参为字面补齐（diff 可逐处核对）。
