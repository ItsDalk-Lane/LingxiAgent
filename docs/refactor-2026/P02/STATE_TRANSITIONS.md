# STATE_TRANSITIONS — 单次执行的状态迁移与所有者（P02-T02）

版本：1.0｜证据基线：HEAD `c3cd52859` + 本阶段改动。
结论先行：**runId 与后台 task 的终态裁决在本仓库已经是集中式的，本阶段零状态机重构**（UNCHANGED_VERIFIED），新增的是 TaskRegistry attempt 栅栏（见 IDENTITY_CONTRACT §3）与本文档固化的状态/事件表 + 补齐的幂等测试（A02/A04）。

## 1. 状态所有者（谁裁决什么）

| 执行域 | 终态所有者（唯一） | 其他角色 |
|---|---|---|
| 前台 Assistant Run（runId） | chat 路由 `finishAssistantRun`（chat.ts:910，exactly-once：`assistantRunActive`/`assistantRunSettled` 双守卫）；唯一正常触发 = `agent_settled` 事件；管理性兜底 = `finishStreamingState`（chat.ts:1236，仅在 run active 且 settled 未到时） | 前端只消费 `assistant_run_end` 投影与历史重投影（runOrdinal），不回写终态 |
| 后台业务 task（taskId+attempt） | TaskRegistry `complete/fail/cancel/abort`（first-write-wins + attempt 栅栏，本阶段新增） | 总线 `task:complete/fail/update` 透传 expectedAttempt；媒体域另有 TaskStore/DeferredResultStore 各自栅栏（IDENTITY_CONTRACT §4.3） |
| 媒体生成执行 | core/media/task-store.ts `settleTask`（expectedAttempt 栅栏 + deliveryState） | poller 投递后 `task:remove` 清 TaskRegistry 可见性镜像 |
| 延迟交付 | DeferredResultStore（`checkAttempt` + durable flushSync 回执） | deferred-result-bus-handlers |
| SDK 会话/模型循环 | Pi（AgentLoop 内部重试/工具轮次） | 产品层不再驱动第二套循环（P01 BOUNDARIES §3.1 红线） |

## 2. Assistant Run 状态/事件表（runId 域）

| # | 事件 | 迁移 | 守卫/备注 | 测试 |
|---|---|---|---|---|
| R1 | `agent_start`（Pi） | ∅ → active（新 runId/stream 复用） | 幂等：Run 已 active 不重建；retry 的第二个 agent_start 同 Run | assistant-run-lifecycle #1/#2 |
| R2 | Model Turn `turn_start/turn_end` | active 内推进 modelTurnOrdinal | 严禁 finalize/reset Run 级状态 | assistant-run-lifecycle #1 |
| R3 | `agent_end`（willRetry 任意） | 无迁移（只记录） | 严禁 finalize | assistant-run-lifecycle #1/#2 |
| R4 | `agent_settled` | active → settled{completed \| failed \| aborted} | **唯一正常 finalize**，exactly-once；runStatus = isAborted ? aborted : (hasError ? failed : completed)；空回复检测在此处且仅在此处 | assistant-run-lifecycle #1/#3；**P02 补：A02 重复 settled 幂等（run_end+usage 恰一次）、A04 迟到事件不逆转终态**（tests/p02-run-finalize-edges.test.ts） |
| R5 | steer 落盘（user 消息 commit） | active → completed(runSplit=true) → active（新 runId，同 streamId） | 只切 Run 语义层；真正终结仍归 R4 | assistant-run-lifecycle #4 |
| R6 | `abort`（WS）→ hub.abort/engine.abortSession | （控制面）标记 isAborted；终态仍由 R4 或 R7 落 | abortSession 单入口三分支（pre-prompt/force-release/无操作） | chat.ts:2367 分支 + session-coordinator-isolated-abort |
| R7 | 断线宽限到期 / turn stall watchdog / 兜底 | active → settled{aborted \| completed} | `finishStreamingState` 仅在 settled 未到时生效；exactly-once 同 R4 | finishStreamingState 代码路径；watchdog chat.ts:1254 |
| R8 | settled 后任意迟到事件（delta/tool_start/重复 settled） | 无迁移 | `assistantRunSettled` 守卫直接返回；**终态不可被旧消息逆转** | **A04**（tests/p02-run-finalize-edges.test.ts） |

## 3. TaskRegistry 状态/事件表（task 域）

状态集：`pending/running/paused/blocked/recovering`（活跃）→ `completed/failed/canceled/aborted`（终态）；非清单状态在 normalizeStatus 抛错（fail-closed）。

| # | 事件 | 迁移 | 守卫/备注 | 测试 |
|---|---|---|---|---|
| T1 | `register`（新） | ∅ → running（attempt=1） | handler 缺失只 warn（无 abort 支持任务显式登记） | task-registry.test.ts #2 |
| T2 | `register`（活跃中重复） | running → running | 幂等，attempt 不变 | **P02：task-identity-attempts #2** |
| T3 | `register`（终态后，合法复用） | final → running（attempt+1，终态/进度清零） | 承接下一次执行；不禁止重注册 | **P02：task-identity-attempts #3** |
| T4 | `update`（带/不带 expectedAttempt） | 任意 → patch | attempt 栅栏：stale 返回 null 不落盘 | **P02：task-identity-attempts #4** |
| T5 | `complete`/`fail` | 活跃 → completed/failed | attempt 栅栏 + **first-write-wins**（终态后不可改写；重复完成无双写） | **P02：task-identity-attempts #5/#6** |
| T6 | `abort` | 活跃 → aborted（handler.abort 派发） | already_aborted/not_found/no_handler 显式区分 | task-registry.test.ts #3-#6 |
| T7 | `cancel` | aborted → canceled（内部改名） | 委托 abort；不经 T5 守卫 | task-registry.test.ts #8 |
| T8 | `abortByParentSession` | 该父下全部活跃 → aborted | 终态跳过（skippedFinal 计数） | task-registry.test.ts #6/#7 |
| T9 | 进程重启 `_loadPersisted` | 活跃 → recovering | attempt 保留；缺 attempt 旧记录按 1 | task-registry.test.ts #10；**P02：#7/#8** |

## 4. 三维度区分（不被压成 success 布尔）

1. **执行结果**（runStatus/task.status）≠ **交付结果**（历史投影 outcome：completed_with_answer / completed_without_user_output / failed / 活跃无终态）≠ **清理结果**（teardown warn/AggregateError，见 CANCELLATION_MAP §4）。三者载体与测试各自独立：runStatus 在 chat 路由；outcome 在 history-builder（tests/history-run-outcome-edges.test.ts T07/T10/T12/T06/T14）；清理结果在 agent-executor（tests/agent-executor-teardown.test.ts + P02 A12）。
2. `completed` 但无用户交付 → 历史侧恰一个 `missing_final_answer`（不隐藏过程块）；`failed` 带部分正文 → 两者共存；cancel requested 但外部执行未证停止 → media pending 的 "generation interrupted during submission; provider acceptance is unknown and generation was not retried"（poller.ts:175-180，A09 依据）。

## 5. 迟到回调/重复结束/重试旧回调的 generation 检查汇总

| 域 | 机制 | 测试 |
|---|---|---|
| runId | assistantRunSettled exactly-once（R4/R8） | A02/A04 + assistant-run-lifecycle |
| task 域 | expectedAttempt 栅栏 + first-write-wins（T3-T5） | task-identity-attempts |
| 媒体 TaskStore | settleTask expectedAttempt → stale 拒绝 | media 既有测试（task-store） |
| 延迟交付 | deferred bus checkAttempt（mediaAttempt 冲突拒绝） | deferred-result-bus-handlers 测试 |
| 工具网关 | prepared invocation lifecycleGeneration 复核（gateway invoke） | tool-invocation-gateway 17 例（P01 A08） |

## 6. 任务完成检查对照

- 状态/事件表每条有测试：§2/§3 右列逐条对应真实测试文件。
- 终态唯一但不会误终结合法 pause/continue：`paused` 属活跃态（T9 恢复为 recovering，T8 会终止它——与既有产品规则一致：会话中止清下属任务）；runId 域无 pause 概念（Pi 续跑由 agent_settled 之前的多 Model Turn 表达，R2 不 finalize）。
