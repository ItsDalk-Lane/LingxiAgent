# IDENTITY_CONTRACT — 运行身份清单与关联规则（P02-T01）

版本：1.0｜证据基线：HEAD `c3cd52859` + 本阶段改动（见 P02_RESULT.json 的 start/end SHA）。
输入：P00 OWNERSHIP_MAP / ENTRYPOINT_MATRIX、P01 BOUNDARIES §2-§3、NEXT_STAGE_HANDOFF §4、本阶段逐文件复核。

## 1. 身份清单（实际执行入口逐一核对）

| 身份 | 格式/铸造厂 | 语义（一次…） | 生命周期 | 持久化 | 本阶段变化 |
|---|---|---|---|---|---|
| sessionId `sess_` | SessionManifestStore（core/session-manifest/id.ts:5-9） | 会话（业务实体） | 跨进程长期 | manifest store | 无（P00 ✅） |
| SDK session UUID | Pi SessionManager（JSONL 文件名） | SDK 会话文件 | 跨进程 | JSONL | 无；恒 ≠ traceId ≠ sessionId 之外的任何身份（P01 BOUNDARIES §3.3） |
| runId（Assistant Run） | chat 路由 `crypto.randomUUID()`（chat.ts beginAssistantRun） | 一次用户输入 → agent_settled | 内存（单 server 进程） | 不持久化（历史按 user 消息边界重投影 runOrdinal） | 无（P00 ✅ exactly-once finalize） |
| streamId `s_…` | session-stream-store beginSessionStream | 一轮流式事件流（seq 单调） | 内存 ring buffer | 不持久化 | 无 |
| taskId `task_` | **lib/tasks/task-identity.ts（本阶段新统一铸造厂）** | 业务后台任务（可多次执行） | 跨进程（plugin-tasks.json） | TaskRegistry | **本阶段统一**：5 处调用方自铸收口（subagent/workflow/rewind/speech/media） |
| attempt（task 执行代次） | TaskRegistry（register 时维护） | taskId 的一次执行 | 随 task 持久化 | plugin-tasks.json | **本阶段新增**：迟到回调栅栏 |
| model callId `mc_` / attemptId `ma_` | lib/llm/model-call-identity.ts | 一次逻辑模型调用 / 一次网络 attempt | 跨进程 | observability.sqlite v7 | 无（P00 ✅） |
| traceId `mt_` | ModelTraceScope + mintModelTraceId | 观测分组（因果根） | 跨进程 | observability.sqlite | 无；user_turn 会话粒度复用、后台强制新根（S11 规则保留） |
| toolCallId | Provider 分配（pi-ai） | 一次工具调用 | 随 session JSONL | session JSONL | 无（外部 id 不重铸） |
| scheduleId | TaskRegistry.schedule（调用方给） | 计划任务 | 跨进程 | plugin-tasks.json | 无 |
| parentCallId | stream observer 运行时因果推进 | 上游模型调用关联 | — | observability.sqlite | 无；无事实 → null，不按时间猜（S11） |

## 2. run / task / turn / trace / call / attempt 的区分（不变量）

1. **run**（runId）= 一次用户输入到 agent_settled 的前台执行；**turn**（Pi Model Turn）= run 内一次模型轮次（modelTurnOrdinal 推进）；**task**（taskId）= 可复用的业务后台任务；**call/attempt**（mc_/ma_）= 模型调用域身份；**trace**（mt_）= 观测分组。五者不得互换（P01 BOUNDARIES §3.3 已固化 runId≠mt_≠SDK UUID）。
2. traceId 与运行 ID 保持分离：用户会话多轮复用 mt_（会话粒度）、后台任务独立新根（scheduler.ts:234 runWithNewModelTrace、agent-executor.ts:418 phone 根、bridge-session-manager bridge_message 根）。P02 未改动任何分组口径。
3. 单次执行身份 = taskId + attempt（后台任务域）或 runId（前台域）。终态裁决按单次执行身份互斥：同一 attempt 的终态 first-write-wins；不同 attempt 互不可见（栅栏）。

## 3. TaskRegistry attempt 语义（本阶段新增，A03）

- `register()`：新任务 attempt=1；活跃中重复 register 幂等（attempt 不变）；**终态后 register = 合法复用**，attempt+1 并清零终态/进度——承接下一次执行，不禁止重注册。
- `complete()/fail()/update()` 接受可选 `expectedAttempt`：执行方在 register 返回快照中捕获本次 attempt，终态回调时回传；不匹配（迟到回调）→ 返回 null、不落盘。不带 expectedAttempt 的旧调用路径语义不变（向后兼容）。
- 终态 first-write-wins：completed/failed 后再 complete/fail 不改写首次结果（防重复完成双写）；需要新执行走 register 复用。`cancel()` 的 aborted→canceled 改名是内部控制动作，不经该守卫。
- 总线透传：`task:register` 现返回含 attempt 的任务快照；`task:complete/fail/update` 接受 `expectedAttempt`。**当前消费状态（验收修复轮 2026-09-22 标注）：仓库内生产代码尚无 `task:complete/fail/update` 发送方（该总线面供运行时插件使用），即 expectedAttempt 栅栏"基础设施就绪、在库消费为 0"；P02 域内已接线的迟到回调防护实际生效于媒体域（settleTask expectedAttempt）与交付域（deferred checkAttempt）。P03+ 扩展 task 域调用方时必须按 NEXT_STAGE_HANDOFF §6 捕获 register 快照并回传 expectedAttempt。**
- 持久化：attempt 随 plugin-tasks.json 落盘；`_loadPersisted` 恢复后栅栏语义不变。
- 边界（显式排除，非遗漏）：`cancel()/abort()` 不受 expectedAttempt 栅栏约束——取消是操作者/系统对**当前态**的控制动作而非某次执行的迟到回调，作用于登记处的现行任务（仓库内无迟到 cancel 生产者；stop-task-tool/chat 路由的 abort 均为用户当下意图）。

## 4. 兼容规则（旧记录）

1. **旧格式 taskId 原值读**：历史记录中的 `subagent-…/workflow-…/rewind-…/speech-…/裸 ts36` 不迁移、不重铸；TaskRegistry 仍按非空字符串接纳（assertText），品牌守卫（`asTaskId`）只约束 P02 之后的新铸造路径，不用于拒绝旧数据。
2. **旧值缺失不可猜**：持久化记录缺 attempt 字段按 1 读（taskAttempt 归一化）；缺 parentCallId → null；不从时间邻近推测父子身份。
3. **媒体双记账的权威划分（P00 🔴 P02-2 的实际形态）**：媒体生成状态权威 = core/media/task-store.ts（自带 attempt/generation 栅栏：settleTask expectedAttempt、isCurrentAttempt）；交付权威 = DeferredResultStore（deferred-result-bus-handlers checkAttempt + durable flushSync 回执）；TaskRegistry 条目是**可见性镜像**（"runtime visibility only"，代码注释已声明），settle 后由 poller `task:remove` 清除（poller.ts:357），不承担终态裁决。三处 attempt 栅栏语义一致，无双终态写。
4. **新 mint 与旧 ID 无碰撞**：`task_` 前缀与全部旧格式前缀不重叠；同进程 seq 单调 + 跨进程随机段防碰撞（与 mc_/ma_/mt_ 同构）。
5. **会话 fork 克隆路径的自铸例外（验收修复轮 2026-09-22 登记）**：以下三处 fork 克隆 ID 铸造点**未**收口至 task-identity，属 P00 所有权图未枚举的既有路径（早于本阶段、randomUUID 保证唯一、无格式消费者、克隆时点不进 TaskRegistry 铸造面，TaskRegistry 按非空字符串原值接纳）：
   - `core/media/task-store.ts:222` `forkedMediaTaskId` → `media-fork-{uuid}`（forkSessionTasks 克隆任务 ID）
   - `lib/subagent-run-store.ts:214` `forkedRunTaskId` → `subagent-fork-run-{uuid}`（forkSessionRuns 克隆 run ID）
   - `lib/subagent-run-store.ts:218` `forkedWorkflowTaskId` → `workflow-fork-run-{uuid}`（同上，workflow 类）
   P02-1 的收口范围 = P00 所有权图枚举的 5 处 TaskRegistry-facing 自铸（已全部完成）；上述 fork 克隆铸造器是否统一归入 task-identity 属范围扩张决策，移交 P03+ 或后续阶段，不在本阶段静默扩大。另注：`core/media/universal-media-manager.ts` 的 batchId（媒体批分组身份，:898/:1264）、`show-card-tool` 的卡片 ID（`c_`）等为独立身份域，非 TaskRegistry taskId，不在收口范围。

## 5. 父子/来源关系登记

| 关系 | 载体 | 规则 |
|---|---|---|
| 后台任务 ↔ 父会话 | TaskRegistry parentSessionId/parentSessionPath/parentSessionRef | abortByParentSession 只终止登记了该父的附属任务；无父登记 = 独立后台任务，不受无关会话取消影响（A08） |
| 子代理 ↔ 父 | subagent-run-store register(parentSessionId/Path) + registry register | 同上；threadId = taskId |
| 媒体任务 ↔ 会话 | media task-store sessionId/sessionPath + deferred sessionRef | 同上 |
| 模型调用因果 | mt_ scope causalParentCallId / lastCallId | 仅运行时因果链（stream observer 推进/工具边界快照）；无事实 → null |
| 后台执行 detach | runWithNewModelTrace 强制新根 | 不继承已结束请求的可变上下文（S11 §五十） |

## 6. 任务完成检查对照

- 两次执行不会因相同 taskId 混淆：attempt 栅栏 + 测试（tests/task-identity-attempts.test.ts「旧 attempt 的迟到 complete/fail/update 被拒绝且不落盘」）。
- 无证据的父模型调用保持 null：resolveModelTraceContext singleton/无 scope → parentCallId=null（tests/model-trace-scope.test.ts 21 例既有覆盖，未改动）。
- 统一铸造厂接入真实调用方：subagent-tool（2 处）、workflow-tool、rewind-tool、speech×4、image-task-runner createTaskId——P00 所有权图枚举的生产 5 类自铸全部收口，P02 后新铸造的 TaskRegistry-facing taskId 均出自 task-identity（无第二 `task_` 格式铸造点）。会话 fork 克隆路径另有三处 store 域自铸克隆 ID（`media-fork-`/`subagent-fork-run-`/`workflow-fork-run-`，randomUUID），为显式登记例外，见 §4.5。
