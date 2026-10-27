# R03-T01 报告｜实现运行与尝试状态机（EXECUTOR-R03-T01-E01）

- 状态：**READY_FOR_REVIEW**（执行者口径；独立复核归属总控另派）
- TASK_ID：R03-T01（ACCEPTANCE_IDS：R03-A01、R03-A02）
- TASK_BASE_SHA：`526f7770f1eff6be289b8c34faeccc1b95e181fd`（分支 `codex/rust-tauri-migration`，无 commit/push；工作树候选留给总控冻结）
- 执行时间：2026-09-29（UTC 摘要时间见 candidate-summary.txt）
- 环境：macOS darwin 27.0.0 arm64；rustup 锁定工具链 **1.98.1**（rust-toolchain.toml，经 `~/.cargo/bin` rustup 代理调用；证据链全部使用 `CARGO_TARGET_DIR=/tmp/r03-t01-target-198`，未用 Homebrew cargo 冒充）；SQLite=rusqlite 0.40.2 bundled；无网络外发，无真实供应商

---

## 1. 实现与调用链（真实接线，非枚举/纯函数演示）

一次用户任务（Run）与一次模型请求（ModelCall）彻底分离。生产入口链：

```
HTTP POST /lingxi/v1/sessions/{id}/execute            (lib.rs execute_session)
  → SessionStore::execute_for(port, events, supervisor, principal, …)   (sessions.rs)
      会话归属/NotFound/Forbidden 判定（R02 原语义不变）
      run_id = backend.allocate_run_id(now)          ← RunId 任务创建时固定（R02 原子分配器）
      → RunSupervisor::drive_run(...)                (runs.rs, 新)
          ① RunStateMachine::transition(Queued→Running)    ← 内核状态机在活链上先判
          ② port.record_run_started(ctx#a1)               ← 单事务：run 行+attempt 行+queued→running 事件
             events.publish_committed(严格在 commit Ok 之后)
          ③ 无 Provider 配置（生产默认，R05 前无真实供应商）：
               finish = CompletedWithoutFinal{NoProviderConfigured}   ← 显式无内容完成，绝不编造回复
             有 Provider（测试注入确定性替身 / R05 真实适配器，同一 port）：
               loop（turn 预算内）：
                 ModelCallId = {run}-mc{nnnn}（独立身份，内核 mint）
                 替身 next_turn → ProviderTurn：
                   Final       → persist 事件对 → CompletedWithFinal（最终消息同事务落库）
                   ToolRequests → persist 事件对 → ToolCallId={run}-tc{nnnn} → ToolExecutorPort
                                  （R04 前为测试替身）→ tool_call_started/completed 事件 → 继续循环
                   Continue    → persist 事件对 → 继续循环（模型调用结束 ≠ 任务结束）
                   Empty       → persist 事件对 → CompletedWithoutFinal{empty_reply|process_only}
                   Failed(retryable) → persist 事件对 → 同一 run 开新 attempt（record_attempt_started，
                                  attempt {run}#a2…，attempt_count+1，不新建用户任务）或 Failed
                   turn 超预算 → Failed{turn_budget_exceeded}（响亮，不假装完成）
          ④ finalize_settlement（唯一 finalize 路径）：
               RunFinish（内核运行结果契约）→ RunOutcome{status, reason, key_events, final_message}
               → port.commit_run_outcome（单事务；事务内经内核 RunStateMachine::finalize 裁决）
               → events.publish_committed（commit 之后）
```

### 内核（lingxi-kernel/src/lib.rs，扩展现有 RunStateMachine，无平行状态机）

- `RunStateMachine::transition`：转换表保持任务书 §4 原样（queued→running↔waiting_approval；任意活跃态→cancelling→cancelled；running→completed/failed/interrupted_needs_attention；终态拒绝一切再转换）。新增穷举测试含 waiting_approval 路径与「running 不可直跳 cancelled（必须经 cancelling 两阶段）」。
- `RunStateMachine::finalize(current, stored, requested) -> FinalizeVerdict | FinalizeRejection`（新）：**单一 finalize 决策核心**。纯 transition 拒绝一切终态再转换；finalize 只识别「完全相同」（status+reason+final message 全等）的重复提交并返回 `IdempotentReplay`，其余终态再提交一律 `Conflict{可诊断 detail}`；首提交必须终态且转换合法；stored 自身与 run 行矛盾 → `CorruptSettlement`。
- `FinalizeSettlement`（新）：结算事实三元组；「完全相同」的机器定义。
- `RunFinish` + `NoFinalCause{EmptyReply,ProcessOnly,ToolPartialFailure,NoProviderConfigured}` + `FailureCause{ProviderFailed,TurnBudgetExceeded,ToolExecutorUnavailable}` + `Cancelled{detail}` + `InterruptedNeedsAttention{detail}`（新）：**运行结果契约**。`terminal_reason()` 产出稳定词表（`completed.with_final` / `completed.no_final.empty_reply` / `failed.turn_budget_exceeded` / `cancelled.requested` / …），落 `runs.terminal_reason`。final message 仅存在于 `CompletedWithFinal`——任何变体都不编造最终答案。
- 身份层 helper：`attempt_id(run,n)={run}#a{n}`（重试递增、同 run）、`model_call_id(run,n)={run}-mc{nnnn}`、`tool_call_id(run,n)={run}-tc{nnnn}`（独立产生，绝不复用 run/attempt 身份）。

### 端口（lingxi-kernel/src/ports.rs）

- 新 `TurnProviderPort`（dyn 兼容，boxed future）+ `ProviderTurn{Final,ToolRequests,Continue,Empty,Failed{retryable}}` + `ProviderDescriptor`：R05 Provider port 交接面。替换 R01 未使用的 `ModelPort` 占位 stub（避免两层重复 provider 面）。
- 新 `ToolExecutorPort`（dyn 兼容）复用现有 `ToolOutcome{Success,Failed,Cancelled,Unknown}`：R04 ToolExecutor 最小交接面。替换未使用的 `ToolPort` stub。
- `StoragePort` 新两方法（RPITIT，与既有方法同风格）：
  - `record_run_events(ctx, events, now)`：运行中事件（模型/工具调用事实）单事务落 key_events；校验 run 存在、owner 三元组一致、**run 仍活跃**（终态后迟到事件响亮 Conflict；audit-only stale 路径按派单留 R03-T04）、**attempt 确已开过**（身份底线：结果不能挂到从未开始的 attempt；完整 generation 栅栏属 T04）。
  - `record_attempt_started(ctx, now)`：同一 run 开新 attempt（run_attempts 行+attempt_count+1，状态保持 running）；终态 run 拒绝再开 attempt（已结束任务的再次执行=新 Run 关系）；attempt 重复即 Conflict。

### 存储（lingxi-adapters/src/storage/run_store.rs，复用同一事务边界，无第二套存储）

- `commit_run_outcome` 终态分支改为经内核 `RunStateMachine::finalize` 裁决，且幂等比较从「仅 status 相等」**收紧为全结算相等**（加载 stored 的 terminal_reason + `{run}-final` 消息行内容与请求逐一比对）：同 status 不同 reason/不同最终消息=Conflict（R02 旧实现会把同 status 异 payload 静默当 replay——本 Task 修正；R02 既有测试的「identical replay/conflict 诊断」公开不变量全部保持绿）。
- 实现 `record_run_events` / `record_attempt_started`（含上述全部校验），`last_event_seq` 同事务推进，事件仅经 Ok 返回值交付发布。
- **无 schema 迁移**：模型/工具调用事实落既有 `key_events`（wire 已知事件词表 `model_call_started/completed`、`tool_call_started/completed`，payload 含 model_call_id/provider/model/operation），A01 的数据库断言即建立在其上；usage/trace 投影表（R05 trace/usage）未提前建。

### 协议（lingxi-protocol/src/lib.rs）

- `RunStatus::from_legacy_wire_name`（新）：外部协议兼容映射，锚定 R00/R03 实读的现役词表（`server/ws-protocol.ts` assistant_run_end `completed|failed|aborted`、`server/block-extractors.ts` `success|failed|aborted`）→ Completed/Failed/Cancelled；未知名返回 None（响亮，不猜）。新 wire_name 不变（测试锁定）；旧客户端传输面适配归 R08。
- contracts/generated 零漂移（check-contracts exit 0：56 生成文件 + API_COMPAT_MATRIX 626 entries 全部匹配再生成）。

### 服务（lingxi-service）

- 新模块 `runs.rs`：`RunSupervisor`（RunSupervisor+运行事务的 owner，对应目标契约 §3 权威表）+ `RunDriveLimits{max_model_turns≤256,max_attempts≤8}`（启动期校验，退化值响亮拒启）+ `finalize_settlement` 公开（T03 取消、T07 恢复走同一路径）。No-final 原因优先级：tool_partial_failure > process_only > empty_reply（`finish_no_final`）。工具结果 → `ToolResultWire` 一一映射（Unknown 保持 Unknown）。
- `ServiceDeps` 新增 `turn_provider/ tool_executor/ run_limits`（生产默认 None/None/默认限额——**无 Provider 时行为显式**：`completed.no_final.no_provider_configured`，不用假回复冒充真实模型）；`ServiceState.runs()` 注入；`execute_session` handler 传 `state.runs()`。
- `SessionStore::execute_for` 签名增加 `supervisor` 参数：R02 的「立即成功」内联执行入口被**替换**为真实生命周期——同一 Run 的终态只有一个 owner（supervisor 的单一 finalize），不存在旧/新两条分别写终态的路径。

## 2. 修改文件清单

修改（9，全部逐 hash 见 artifacts/…/candidate-summary.txt）：`rust/crates/lingxi-kernel/src/{lib.rs,ports.rs}`、`lingxi-protocol/src/lib.rs`、`lingxi-adapters/src/storage/run_store.rs`、`lingxi-service/src/{lib.rs,sessions.rs}`、`lingxi-service/tests/{event_subscription.rs,execute_concurrency.rs,service_persistence.rs}`。
新增（3）：`lingxi-service/src/runs.rs`（生产代码）、`lingxi-service/tests/run_lifecycle.rs`、`lingxi-adapters/tests/run_finalize_property.rs`（永久测试）。
Cargo.lock 零变化（sha256 `90111c4b…` 与 R02_HANDOFF dependency_locks 相同；无新第三方依赖）。

### 既有测试夹具改动（逐处+理由；独立 Reviewer 复核未降保护）

1. `sessions.rs` 内部测试（6 处 `execute_for` 调用）：插入 `&supervisor()`（`RunSupervisor::without_provider()`）实参——公开不变量未动：owner 全可见/跨主体 Forbidden 且零副作用/commit 失败=Storage 错误且零 outcome/64 并发 64 独立 run id 全部原样通过。FakePort 补齐 StoragePort 两个新方法的空实现（与真实语义无关的内存桩）。
2. `execute_concurrency.rs`（1 处）、`service_persistence.rs`（1 处）、`event_subscription.rs`（1 处 `execute` 助手）：同样插入 `state.runs()` 实参。三套测试断言原文未改——包括 execute_concurrency 的「每 run 恰 2 key events（128/64）」与 service_persistence 的「重启后 completed+2 事件」读回（无 Provider 路径刻意保持每 run 恰 start+terminal 两个事件，见 run_lifecycle.rs 的 `no_provider_configuration_is_explicit_and_r02_compatible` 显式锁住该不变量）。
3. 无任何断言被删除/放宽/改永真；无 skipped。

## 3. 逐验收：预期 vs 实测

### R03-A01 多模型调用只有一个任务终态 — **PASS（本机隔离环境，确定性替身）**

- 前置：替身按 工具→继续→最终回复 三次模型调用（`ScriptedProvider`：turn1 `ToolRequests[read]` → turn2 `Continue` → turn3 `Final`；`ToolDouble` 返回 Success）。
- 预期：一个 Run、三条 ModelCall、唯一任务终态；中间模型结束不算任务结束。
- 实测（`run_lifecycle.rs::r03_a01_three_model_calls_produce_exactly_one_task_terminal`，12/12 全绿）：
  - 数据库断言：runs 恰 1 行 status=completed、terminal_reason=`completed.with_final`、attempt_count=1；`model_call_started`=3、`model_call_completed`=3、`tool_call_started/completed`=1/1（key_events 直查）；model call id 恰为 `{run}-mc0001..0003`，全部挂 attempt `{run}#a1`；tool call id=`{run}-tc0001`（驱动方 mint）；`{run}-final` 消息行存在且含最终文本。
  - 事件断言：durable 事件全序恰为 `run_state_changed(queued→running), mc1×2, tool×2, mc2×2, mc3×2, run_state_changed(running→completed), final_message_committed`（11 条）；**终态迁移事件恰 1 条**且位于最终消息事件之前、所有模型调用事件之后——turn1/turn2 的结束只是过程事实。live hub 订阅（执行前建立）收到全部 11 帧（commit 后发布）。
- 证据：`artifacts/rust-tauri/R03/T01-E01/a01-run-lifecycle.log`（exit 0，12 passed）。

### R03-A02 重复终态不重复结算 — **PASS（属性测试+结果计数）**

- 前置：相同 settled 消息重复 + 冲突消息乱序到达（5 种结算：completed+final A/B、completed 无 final、failed、interrupted；固定 xorshift seed `0x5eed0000c0ffee01`，每 run 10 个随机到达序，200 轮，真实 RunDatabase）。
- 预期：只持久一次终态/结算；冲突可诊断。
- 实测（`run_finalize_property.rs`，2/2 全绿）：
  - **计数**：`runs_settled=200, first_commits=200, idempotent_replays=350, diagnosed_conflicts=1450, unexpected=0`——每轮恰一次 `newly_committed=true`，其余到达非 replay 即 Conflict；持久态（status/terminal_reason/final message 行 0..1 及内容）恒等于先到者且不再翻转；`run_state_changed` 恒为 2 条（创建+唯一终态）。
  - 冲突 detail 均含 run id 与「already terminal」句（机器可诊断）；同 status 异 reason/异 final message 亦 Conflict（本 Task 收紧点）。
  - 纯核半场：kernel `finalize_property_first_settlement_wins_and_never_flips`（500 轮随机序）与服务级 `duplicate_finalize_replays_and_conflicting_finalize_is_diagnosed`（经公开 `finalize_settlement` 同一路径：重复→幂等且 key_events 计数不变；冲突→`Storage(Conflict)` 且终态不翻转）。
  - 乱序非法到达（running 直跳 cancelled）→ InvalidRequest 拒绝，run 仍恰好结算一次（`out_of_order_illegal_settlement_is_rejected_and_run_still_settles_once`）。
- 证据：`a02-finalize-property.log` + `r03-a02-finalize-property.json`（schema `lingxi.r03-a02-finalize-property.v1`，机器可读计数）+ `kernel-tests.log`。

### 补充规格点实测（任务书「怎么做」1–4）

- 状态+转换表：kernel 22/22 绿（穷举转换、终态拒绝、finalize 三态、身份层）。
- attempt 重试递增/不另建任务：`retryable_provider_failure_reopens_attempt_on_the_same_run`——1 run、2 attempts（run_attempts 两行、attempt_count=2）、mc1 挂 #a1 / mc2 挂 #a2、单一终态；`retry_budget_exhaustion…`（attempt 预算=2 时响亮 failed.provider_error）；`non_retryable_failure…`（不重试，attempt_count=1）。
- 唯一 finalize/终态不复活：A02 全组 + `terminal_run_rejects_late_attempts_and_events_loudly`（终态 run 拒绝新 attempt 与迟到事件；未开过的 attempt 在活跃 run 上也拒绝）。
- 明确 outcome：empty_reply / process_only / tool_partial_failure / no_provider_configured / turn_budget_exceeded / tool_executor_unavailable / provider_error / cancelled / interrupted_needs_attention 各有专属 reason 词表与测试；无 final 时永不落空消息（messages 行数=0 断言）。

## 4. 验证命令与退出码（全部经 rustup 1.98.1 + `--locked`；`verify-stage R03` 未注册，未运行未伪造）

| 命令 | 退出码 | 结果摘要 |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | 无 diff |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 0 error/warning |
| `cargo test --workspace --locked` | 0 | **482 passed / 0 failed / 47 suites ok**（R02 全量回归含在内） |
| `cargo run --locked -p xtask -- check-contracts` | 0 | 56 生成文件+API_COMPAT_MATRIX 626 entries 零漂移 |
| `cargo run --locked -p xtask -- check-boundaries` | 0 | DEP-07/08/D5 全 PASS |

定向过滤器与命中数（0 命中不算通过）：`run_lifecycle` 12、`run_finalize_property` 2、`lingxi-kernel`（lib）22、R02 受影响子集——sessions 8（180 filtered）、execute_concurrency 3、service_persistence 2、event_subscription 12、storage_transactions 7、run_id_allocator 5、migration_idempotency 2、event_store_reads 3、backup_restore 11、disk_full_fault 2。全部 exit 0。

## 5. 证据位置与摘要

`artifacts/rust-tauri/R03/T01-E01/`：`commands.log`（命令+退出码）、`workspace-test.log`（全量 482/0 原始输出）、`clippy.log`、`check-contracts.log`、`check-boundaries.log`、`a01-run-lifecycle.log`、`a02-finalize-property.log`、`r03-a02-finalize-property.json`、`kernel-tests.log`、`r02-subset-*.log`（10 份）、`candidate-summary.txt`（基线 SHA+HEAD+逐文件 sha256）。工作树仅含本 Task 改动+总控派单文件（docs/rust-tauri/R03/ 未跟踪目录），无用户既有修改被混入。

## 6. 测试替身边界

- 允许侧：`ScriptedProvider`/`ToolDouble` 仅产生外部响应（`ProviderTurn`/`ToolOutcome`），经 `ServiceDeps` 注入**真实** service 组合根——真实路由下的 execute 入口、真实 `RunSupervisor`、真实内核状态机/finalize、真实 RunDatabase 单写者事务、真实 EventService 发布。替身不写任何状态、不落库、不 finalize、不参与身份 mint（ModelCallId/ToolCallId 由驱动方产生）。
- 生产侧：`turn_provider/tool_executor` 生产默认 None（无 Provider 时显式 `completed.no_final.no_provider_configured`，不以假回复冒充真实模型）；替身定义仅存在于测试文件，不进生产默认。

## 7. 候选输入摘要

基线 `526f7770f…`（=HEAD，无 commit）；修改 9 + 新增 3 路径，逐文件 SHA-256 与说明见 `candidate-summary.txt`；`rust/Cargo.lock` 零变化（`90111c4b…`）。要点 digest：kernel lib `202a0a61…`、kernel ports `457d0954…`、protocol `7dc6fe93…`、run_store `dd7bf84e…`、service lib `50c980c5…`、sessions `abc2e7dd…`、runs.rs（新）`fdf92e8a…`、run_lifecycle.rs（新）`3e240396…`、run_finalize_property.rs（新）`1561cb77…`。

## 8. 未验证项 / 边界（如实）

- 会话队列/串行化（T02）、取消树与 waiting_approval 实际流转（T03，本轮仅转换表+finalize 契约可构造 cancelled/interrupted）、迟到结果 audit-only stale 记录与 request 去重（T04；本轮为身份底线：迟到事件/未开 attempt 响亮拒绝）、InvocationJournal/副作用收据（T05）、恢复协调器（T07）。均为后续 Task 范围，未提前实现也未宣称。
- `waiting_approval` 在运行链上无触发路径（R04 审批网关前无审批语义），仅状态机表级验证。
- 无真实供应商、无跨平台（本机 arm64 macOS）；本 Task 不涉及 npm/桌面栈（未触碰）。
- verify-stage R03 未注册（R03-T08 职责），本报告不声称其结果。

## 9. 独立复核重点建议

1. `commit_run_outcome` 收紧后的幂等/冲突半场：stored 结算加载（terminal_reason+`{run}-final` 反序列化比对）与内核 `finalize` 裁决的一致性；R02 旧绿（storage_transactions `finalize_is_idempotent_and_conflicts_are_diagnosed`）是否等价保持。
2. A01 事件全序断言是否真的钉住「中间模型结束≠任务结束」（terminal 位置断言 `vec![events.len()-2]`）。
3. 替身边界：`ScriptedProvider`/`ToolDouble` 是否有任何状态书写面（应无）。
4. 无 Provider 生产默认：`no_provider_configuration_is_explicit_and_r02_compatible` 与 execute_concurrency 的 2-events-per-run 不变量的耦合。
5. 夹具改动 3 处（§2 清单）是否降低任何 R02 公开保护（应全部原文断言）。
