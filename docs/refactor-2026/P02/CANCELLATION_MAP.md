# CANCELLATION_MAP — 取消传播与有界清理（P02-T03）

版本：1.0｜证据基线：HEAD `c3cd52859` + 本阶段改动。

## 1. 一次执行拥有的可取消资源清单（含归属与传播接口）

| 资源 | 所有者/登记处 | 取消传播接口 | 有界清理 |
|---|---|---|---|
| pre-prompt 窗口（检索/排队中的提交） | `_prePromptAbortControllers`（session-coordinator，按 path） | `abortSession` 分支一 `pending.abort()`（:5479-5493）；chat 侧 `abortPendingDesktopSubmission` 标记 | AbortController 原生；不补发 turn_end（见代码注释） |
| Pi 会话模型循环/流 | Pi AgentSession | `session.abort()`（abortSession→_forceReleaseStreamingSession :5974 / runAgentSession onAbort / phone abort handler） | dispose + `emitSessionShutdown`（teardownSessionResources） |
| 工具执行 | `SessionExecutionRegistry`（按 sessionId，engine.ts:742/4624 包装全部工具） | `abortToolExecutionsForSession`（abort cleanup 第一步 :5429）；执行内 signal = `AbortSignal.any(local, upstream)` | 条目随工具 promise settle 自动 release（runtime-only） |
| 等待审批 | `ConfirmStore`（pending map） | `confirmStore.abortBySession`（:5458）→ promise 以 `{action:"aborted"}` 收口 → wrapper 返回 toolError（执行 0 次）；**晚到 resolve 返回 false** | 每条 pending 有 5min 超时 timer，abort/resolve 均 clearTimeout |
| 后台业务 task | TaskRegistry handlers（按 type） | `taskRegistry.abortByParentSession`（:5438）；REST `/task/:taskId/abort` + WS `subagent_stop_request`（stale_stream/ownership 校验） | abort 状态落盘；终态任务 skippedFinal |
| 子代理 run/thread | subagent-run-store / subagent-thread-store | `abortByParentSession` / `removeBySession`（:5443/:5448） | 状态机内收口 |
| 延迟交付 | DeferredResultStore | `suppressBySession`（:5453；retry fences deliverySuppressed 在 chat 侧再挡一道 :2221） | durable store flush |
| 终端/浏览器 sidecar | terminalSessions / BrowserManager | `closeTerminalsForSession`（:5463）/ `closeBrowserForSession`（:5468） | 平台 PTY/浏览器生命周期 |
| turn stall watchdog | chat 路由 `turnStallTimer` | `clearTurnStallWatchdog`（finishAssistantRun/finishStreamingState） | `unref()` + 活动时间重排 |
| WS 断线宽限 | `disconnectAbortTimer`（chat.ts:447） | 全部 client 断开且宽限到期 → `abortAllStreaming`；新 client 连接取消定时 | 宽限 ms 可配（0=禁用），`unref()` |
| 计划任务 timer | TaskRegistry `_scheduleTimers` | `clearTimers`/`unschedule` | `unref()` + MAX_TIMER_DELAY 封顶 |
| 沙盒命令进程树 | exec-helper `spawnAndStream`（seatbelt/bwrap/win32-exec 共用） | `signal.abort`/timeout → `killTree`：**按 pgid**（posix：detached 子进程 + `process.kill(-pid)`；win32：`taskkill /F /T`）；不按进程名 | abort/timeout 均 reject（"aborted"/"timeout:N"）；stdio 收尾窗口 exitStdioGraceMs=100ms |

**AbortSignal 不跨进程当 JSON 传**：WS/REST 只传取消意图（abort 消息/taskId），signal 在 server 进程内由各登记处铸造（SessionExecutionRegistry 每执行一个 controller）。

## 2. 取消后的启动禁令（"取消后不得启动新模型调用/新工具"）

- 模型层：`session.abort()` 后 Pi AgentLoop 收口，agent_settled 是唯一终态来源；hub/subagent 侧 runAgentSession 每轮 `if (signal?.aborted) throw AbortError`（:340/:706）。
- 工具层：会话 abort → SessionExecutionRegistry abortBySession 使在途 signal aborted；agent loop 已停 → 不再有新 toolCall。
- 审批层：待审请求随取消失效（A05 测试：执行 0 次 + 晚到 approve false）。
- 已承诺外部操作：媒体 submit 中断落 `failed: "generation interrupted during submission; provider acceptance is unknown and generation was not retried"`（poller.ts:175-180）——未知不自动重试、如实显示。

## 3. 清理顺序契约（finally 顺序）

统一入口 `teardownSessionResources`（core/session-teardown.ts）：
1. `emitSessionShutdown`（SDK 扩展清理 setInterval/订阅）
2. Hanako 层 `unsub`
3. `session.dispose`

每步失败 warn 不阻断；**本阶段起返回 `{errors}`**，`runAgentSession` 将其并入 AggregateError（原失败在前、清理失败在后，A12）；其余调用方行为不变（返回值忽略=显式选择 warn-only，登记于 §5 账本）。

## 4. 清理期限

| 期限 | 值 | 来源 |
|---|---|---|
| turn stall abort | `turnStallAbortMs`（0=禁用） | 既有配置（chat 路由装配） |
| WS 断线宽限 | `disconnectAbortGraceMs`（0=禁用） | 既有配置 |
| 审批超时 | 5min（ConfirmStore DEFAULT_TIMEOUT） | 既有常量 |
| 进程树强杀 | 即时 SIGKILL（posix 组信号/win32 taskkill /F /T） | 既有沙盒能力：不声称"温和等待"也不无限等；signal 发送后由 close/exit 事件证实退出（A06 用进程存活探测验证） |
| stdio 收尾窗口 | 100ms（exitStdioGraceMs） | 既有常量 |

虚拟时钟验证：turn stall/disconnect 宽限的既有测试使用 fake timers（assistant-run-lifecycle 等）；真实子进程退出：A06（tests/p02-cancellation-edges.test.ts，真实 spawn + pid 存活探测 + 哨兵对照）。

## 5. 本阶段改动与遗留账本

- 改动：`teardownSessionResources` 返回收集的 errors（不改变抛错行为）；`runAgentSession` 并入 AggregateError（A12）。零新取消通道、零权限面变化。
- 账本（不改行为的显式登记，验收修复轮 2026-09-22 补全为全量 9 处调用方）：
  - `runAgentSession`（hub/agent-executor.ts:355）：清理失败并入 AggregateError（A12，唯一升级处）。
  - session-coordinator 5 处（:2233/:2254/`_teardownSessionEntry` :6035/:8418/isolated :8635）仍 warn-only——LRU 驱逐/常规关闭路径的清理失败只记日志（与会话已自然结束的场景匹配，升级为抛错会改变正常路径语义，非本阶段必需）。
  - bridge-session-manager 2 处（`executeExternalMessage` finally :1470、`compactSession` finally :1879）仍 warn-only——均为 finally 收尾，主结果已返回或异常已沿主路径上抛，清理失败不并入。
  - `runAgentPhoneSession`（hub/agent-executor.ts:721）仍 warn-only——电话会话捕获结果以返回值交付（无 AggregateError 通道），与 runAgentSession 的抛错型交付形态不同；如后续统一交付形态可复用 A12 模式。
  - `_forceReleaseStreamingSession` 的 unsub/abort/dispose 各自 try-warn（中止路径已在事件流上给用户终态）。
