# TRACE_COMPAT_REPORT — 轨迹分组与真实因果（P04-T05）

日期：2026-09-22｜基线 HEAD `3286c96e5`。全部 UNCHANGED_VERIFIED；本阶段零生产改动、零语义变化（用户已采纳的观测分组不在改动面）。

## 1. 分组规则（保持不动）

| 规则 | 实现 | 测试 |
|---|---|---|
| user_turn 按会话复用 mt_ 轨迹：同会话后续 turn 经 reuseTraceId 并入同一 trace，多轮 call 累加 | session-coordinator.ts:5032/:5203（resolveSessionReusableTraceId）→ model-trace-scope.ts runWithModelTraceRoot | tests/model-observability-session-trace-reuse.test.ts（7 例：同会话两轮一行累加 / 不同会话不串 / ingress 形态并入 / singleton 不复用 / 失败轮仍命中） |
| traceId 恒不等于 sessionId（铁律） | mintModelTraceId 独立铸造；reuse 只复用已铸 mt_ id | tests/model-trace-scope.test.ts · “reuseTraceId”族；tests/model-observability-session-trace-identity.test.ts |
| bridge/phone/automation/plugin/media/speech/provider_probe/health_check 独立根 | bridge-session-manager.ts:1165（origin=bridge_message）、agent-executor.ts:418（phone_message）、scheduler.ts:234（automation）、server/index.ts:723/759（plugin）、models.ts 健康检查（health_check）、runWithNewModelTrace 强制覆盖 | trace-propagation 测试 7/10、model-call-probe-observer、model-call-diary-observer |
| call/attempt 独立身份，同一 trace 内表达多轮 | recorder：callId/attemptId 分铸；attemptVisibility 区分 logical_boundary/exact/external_process_boundary | model-call-observer/pi-stream-observer/pi-retry-visibility |

## 2. 真实因果（parentCallId 只由事实提供）

- **统一解析**：resolveModelTraceContext——explicit > TraceScope.lastCallId > session 注册 trace > singleton；无事实 → null，绝不按“上一轮最近 call/数组末项/完成先后”猜。
- **agent loop 顺序链**：noteAgentStreamCallStarted 推进 lastCallId（C2.parent=C1 由运行时链证明）。
- **并行工具分支**（A12）：runToolExecutionWithModelTrace 进入边界即快照 causalParentCallId=scope.lastCallId（冻结），视觉/审批/子调用各支不覆盖共享 lastCallId——双双 parent=触发 call，与返回顺序无关。
- **辅助调用不推进 lastCallId**：callText/媒体/语音/probe/direct summary 只读（数据依赖≠触发因果）。
- **跨进程/worker**：ALS 不跨进程；外部进程边界（Dreamina CLI）在工具执行 scope 内以 observedExternalProcessRun 记 attempt（opaque），无隐式继承假象。
- 证据：tests/model-call-trace-propagation.test.ts 11 组（singleton/顺序链/并行双 parent/跨 session 子代理/工具内媒体/语音/automation 双根/diary 三 call/并发会话不串线/detached background 不泄漏/毒丸不入事件）+ e2e S2/S3（真实栈 parent=c1 非 toolCallId）。

## 3. refs 安全边界

- refs ≤8 键、键 ≤64、值截断 ≤128 字符（sanitizeTraceRefs）；只放 sessionId/taskId/toolCallId/pluginId 等安全身份，禁 prompt/结果/密钥（§五十五）。
- trace 事件 details 经 sanitizeModelCallDetails；毒丸回归见 trace-propagation 测试 11（“毒丸不出现在任何 trace 事件里”）与 model-call-safety-gate。
- 真实 payload 沿 Phase 6 payload store（受控脱敏 + 权限机制），与 trace refs 分离。

## 4. 回归结论

既有 21+7+11 组测试全绿（P04-BASE-trace-stream：8 文件 80 例；e2e 含真实栈）；本阶段未改动任何 trace 语义，无需迁移。P05 消费规范化事件时按本表分组语义取数，不重新推断因果。
