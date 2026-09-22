# CONCURRENCY_RULES — 并发与父子任务边界（P02-T04）

版本：1.0｜证据基线：HEAD `c3cd52859` + 本阶段改动。
结论先行：并发隔离的承载已是**按身份键控的登记处 + AsyncLocalStorage**，不存在参与异步执行的全局"最近一次调用"变量；本阶段零并发机制重构（UNCHANGED_VERIFIED），补齐跨会话/父子边界测试（A07/A08，tests/p02-concurrency-edges.test.ts）。

## 1. 可变"当前值"审计（currentSession/currentAgent/currentTrace）

| 可变值 | 位置 | 是否参与异步执行 | 隔离方式 |
|---|---|---|---|
| `coordinator._session` / `_currentSessionPath` | session-coordinator | **否**（只表达桌面焦点会话；切换/abort 时同步刷新 :5952） | 一切执行态按 path/sessionId 存于 `_sessions`/`_prePromptAbortControllers`/`_hibernatedSessionMeta` 等 Map，不用"当前值"找执行 |
| `ss`（chat 会话流状态） | chat 路由 `sessionState` Map | 是（流式） | 按 sessionPath 键控；MAX_SESSION_STATES=100 上限 |
| trace 上下文 | `ModelTraceScope`（AsyncLocalStorage） | 是 | ALS 异步链传播；禁 global currentTraceId；`runWithNewModelTrace` 强制新根防 timer 泄漏（§五十） |
| 工具会话绑定 | `captureSessionBinding` → prepared invocation sessionId | 是（工具执行期） | 每次 execute 快照绑定，不读全局当前会话 |
| phone abort handler | `registerAgentPhoneAbortHandler` | 是 | 按 agent+conversation+sessionPath 注销式登记，teardown 反注册 |

**没有**裸模块级 `currentX` 参与异步裁决；"A 取消/切换不会修改 B 的状态、凭证、工具结果或轨迹"由 A07 三组测试逐面验证（工具 signal / 待审确认 / 后台任务）。

## 2. 父子任务规则（按登记，不按时间/名称猜）

| 规则 | 实现 | 测试 |
|---|---|---|
| 附属任务随父会话终止 | TaskRegistry `abortByParentSession`（parentSessionId 优先、path 移动后可追；终态 skip） | A08-1/A08-3；task-registry.test.ts #6/#7 |
| 独立后台任务不受无关会话取消影响 | 无 parentSession* 登记 → `matchesParentSession` 不匹配 | A08-2 |
| 子代理/媒体/rewind 的父登记 | 注册时显式携带 parentSessionId/Path（subagent-tool:431、media submit、rewind） | 各自工具测试 |
| 父子模型调用因果 | 仅运行时链（scope.lastCallId / 工具边界快照）；无事实 → null | model-trace-scope 21 例 |
| 后台执行 detach | `runWithNewModelTrace`（scheduler/automation 强制新根） | model-trace-scope 泄漏防线用例 |

## 3. 同一会话写入顺序

- 用户输入→Pi 会话：`session_busy` 门禁（hub 路由表）+ steer 语义（不打断循环，切 Run 语义层）。
- 会话 JSONL：SDK SessionManager 单写入者；产品侧同步用 `openSessionManagerAtCurrentBranch` + `_syncSessionBranchHeadQuiet`。
- 并行文件修改：文件工具走 session-files 登记处/资源锁路径（P03 工具域详审）；本阶段不引入新 `Promise.all` 等价并发安全假设。

## 4. 并发容量与等待行为（沿用现有上限，无新增无限队列）

| 维度 | 上限 | 位置 |
|---|---|---|
| 缓存会话数 | 20（LRU 驱逐） | session-coordinator MAX_CACHED_SESSIONS:742 |
| workflow 并发 | `maxConcurrent` + AGENT_TOTAL_BACKSTOP=1000（嵌套共享父 limiter，限一层） | workflow-tool:49-51/280-285 |
| subagent 并发 | per-session + global（createSubagentTool 闭包） | subagent-tool:351 |
| chat 会话流状态 | 100 | chat.ts MAX_SESSION_STATES |
| 流事件 ring buffer | 5000 条 / 8MB / 单条 256KB | session-stream-store |

超限行为均为显式拒绝或排队（limiter），无每任务一个完整运行时，无无界队列。

## 5. 任务完成检查对照

A 取消或切换不修改 B 的状态、凭证、工具结果或轨迹：A07 三个维度（SessionExecutionRegistry signal、ConfirmStore 待审、TaskRegistry 任务）+ 既有 model-trace-scope 并发隔离用例。
