# ENTRY_EXECUTION_MATRIX — 已采纳执行入口 × 生命周期证据（P02-T06）

版本：1.0｜证据基线：HEAD `c3cd52859` + 本阶段改动。P00 ENTRYPOINT_MATRIX 的入口清单不变（收敛判定复用）；本表补每个入口的**正常 + 取消/异常**两类证据与生命周期终止点。

## 1. 入口 × 证据矩阵

| 入口 | 正常执行证据 | 取消/异常证据 | 生命周期终止点 |
|---|---|---|---|
| E-DESKTOP 桌面输入 | composition Part 4 纵向链（真实 server+WS+Pi+witness 模型+read 工具+历史回读） | assistant-run-lifecycle abort 用例；**P02：A04 迟到事件不逆转终态**（p02-run-finalize-edges）；WS abort 消息分支（chat.ts:2367 stale_stream/hub.abort/pendingSubmission 兜底） | agent_settled → finishAssistantRun（exactly-once）+ usage 一次 |
| E-CLI | cli-local-server 8 例（P01-A09/A11）+ composition Part 4（同一 /ws 链） | **cli-abort-contract**：stream 三元组齐才发 abort、陈旧终态忽略、跨会话流事件不采纳 | 同 E-DESKTOP |
| E-BRIDGE 外部消息 | bridge-manager → executeExternalMessage（bridge 会话测试族） | 会话中止 → `_cleanupAbortedSessionSidecars` 全量面（工具执行/任务/子代理/deferred/审批/终端/浏览器，CANCELLATION_MAP §1）；bridge 独立 trace 根不受前台取消影响（S11） | 同 E-DESKTOP（bridge session 的 agent_settled） |
| E-CRON 定时/心跳 | scheduler → executeIsolated（ephemeral 隔离 = P00 已验 UNCHANGED_VERIFIED） | **session-coordinator-isolated-abort**：开跑后中止停后续 turn、pre-run 窗口中止、session.abort 失败无未处理 rejection | executeIsolated 内部 teardown（临时 session tombstone+unlink） |
| E-CHANNEL 频道/DM | agent-executor phone 用例（快照/工具/usage/diagnostics） | **phone abort handler 登记/注销**用例；**P02-A12** 清理失败并入原失败（AggregateError） | teardownSessionResources（emit→unsub→dispose）+ abort 反注册 |
| E-WS 断线（窗口关闭/网络） | — | **P02：p02-entry-lifecycle-edges**——宽限到期 abortAllStreaming 恰一次；宽限内重连不中止（窗口关闭≠立即取消）；再次全断线重新进入兜底 | 宽限定时器一次性 + unref |
| E-SUBAGENT/WORKFLOW 子任务 | subagent/workflow 工具测试（defer 登记与结果回送） | REST `/task/:taskId/abort` + WS subagent_stop_request（ownership/stale 校验）；父会话中止 abortByParentSession（**A08**） | deferred resolve/fail → runStore/registry 收口 |
| E-MEDIA 媒体后台 | media 工具测试族（submit/poller/delivery） | **A05**（审批取消）；父会话中止 → handler.abort → poller.cancel；**A09/A11** 崩溃/持久化失败语义 | settleTask + durable 交接回执 + task:remove |

## 2. 权限主体差异（共享裁决，不共享权限）

- 各入口的授权主体继续走各自入口的 principal 解析（E-WS-AUTH → resolveHttpRequestPrincipal；phone 的 read_only 档 + host 预授权 routine 能力；isolated 的 deny_on_prompt）。共同服务（网关/registry）不抬升低权限入口：phone read_only 拦截用例（agent-executor-teardown「read-only temp sessions」+ session-permission-mode phone 用例）证明。
- 无新平行 TaskRegistry / Activity / deferred 模块（grep 复核：register 唯一实现 lib/task-registry.ts）。

## 3. 窗口关闭语义（按当前配置）

窗口关闭 = WS 断线 → **不自动等同用户取消**：进入 `LINGXI_WS_DISCONNECT_ABORT_GRACE_MS` 宽限（默认值见 chat.ts DEFAULT_DISCONNECT_ABORT_GRACE_MS，0=禁用），宽限内重连恢复流式；到期无客户端才 abortAllStreaming（p02-entry-lifecycle-edges 三例固化）。

## 4. 本阶段改动

零生产入口改动（全部 UNCHANGED_VERIFIED + 新增证据测试）；唯一生产级新增是 TaskRegistry attempt 栅栏与总线 attempt 透传（IDENTITY_CONTRACT §3），对全部入口统一生效。
