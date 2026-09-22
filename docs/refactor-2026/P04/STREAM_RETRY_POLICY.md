# STREAM_RETRY_POLICY — 流式解析、重试与取消（P04-T04）

日期：2026-09-22｜基线 HEAD `3286c96e5`。全部为 UNCHANGED_VERIFIED 的事实登记；分片/UTF-8 边界/交错工具参数为本阶段新增真实协议栈测试（S3）。

## 1. 职责边界（谁解析供应商协议）

```text
pi-ai provider adapter（SDK 内部）        SSE/字节流解码、工具参数 delta 拼接、消息组装
  ↓ AssistantMessageEventStream
lib/pi-sdk/stream-guard.ts               空名 toolCall 拦截/文本恢复（不重复解协议）
lib/pi-sdk/model-call-stream-observer.ts 观测（call/attempt 生命周期；不解协议）
server assistant_event_normalizer        正文/推理/阶段元数据规范化呈现（A07）
```

消费端（前端/桥接/CLI）只见规范化事件，不各自解供应商协议（P05 消费同一语义）。

## 2. 分片与边界覆盖（T04-2）

| 场景 | 证据 |
|---|---|
| 工具 args 跨多 delta、多 call 交错、空 delta | tests/model-observability-e2e-chat.test.ts · “S3 P04-A06”（真实 Pi AgentSession + 本地 SSE witness：两个 toolCall 参数交错分片，各自重组为 {city,unit} 且仅在完成后执行） |
| UTF-8 多字节字符跨 TCP 字节边界 | 同 S3（witness 新增 sse-bytes 分片投递，强制在“北”的 3 字节序列中间切断；9 字节固定边界另切多处） |
| 重复/空结束、text_end 契约 | tests/assistant-event-normalizer.test.ts · “text_end 契约表”×3、“回合结束仍未拿到阶段…” |
| 晚到阶段元数据（phase 只能在块结束时确定） | 同上 · “阶段只能在块结束时确定时…”×2、“供应商直接给出阶段时…”（A07：先发未决过程段，不错误提前定最终） |
| 网络断开/畸形 JSON | callText parseFailed → markModelCallSafeMessage（llm-client.ts:932-937）；operation client invalid_provider_response（responseJson parse 失败捕获后显式抛） |
| 空名 toolCall/协议片段混入正文 | tests/pi-sdk-stream-guard.test.ts（7 例：拦截、恢复、不恢复协议 XML 片段） |
| 解析状态按 call/attempt 隔离 | model-call-recorder.ts 状态机（每 callId 独立 recorder；beginAttempt 新 attemptId；endLogicalCall 后 silent no-op） |

## 3. 重试策略（何时允许、何时禁止）

| 通道 | 内部重试 | 观测口径 | 证据 |
|---|---|---|---|
| Pi chat（pi-ai retryProviderRequest） | 408/409/429/5xx/网络错误自动重试（SDK 内部） | **折叠为 1 logical call + 1 attempt**（attemptVisibility=logical_boundary，不伪造多个 attemptId）；重试耗尽原错误抛出；400 等非 retryable 不重试 | tests/model-call-pi-retry-visibility.test.ts（429→429→success 真实 3 次网络调用、耗尽、400） |
| callText（utility/probe） | **无内部重试**：一次调用 = 一个真实网络 attempt（attemptVisibility=exact） | attempt_error 后 call 终态 error | tests/model-call-calltext-observer.test.ts |
| 操作协议（embedding/rerank） | 无自动重试；ModelOperationRequestError.retryable 仅作上层退避信号（job 级） | observedProviderFetch 每 fetch = 1 attempt | tests/model-operation-client.test.ts |
| 媒体 Codex 401 | 凭证强制旋转后**单次**重试（attempt 2 = 新 attemptId；refresh 是控制面不算新 call） | openai-codex.ts:283-295 | tests/model-call-media-observer.test.ts |
| MCP 传输（对照） | 超时单发不重放（P03-A12 语义延续） | — | tests/mcp-http-client.test.ts |

**禁止项（现状即合规）**：流已公开后失败不静默重发拼接——旧 attempt 独立记录（attempt_error + logical_call_end(error)），新 attempt/新 callId 不把旧输出接到新输出冒充连续流（MC-02 “tool recovery 产生第二个 logical call” 用例锁定）。带副作用工具之后的失败不自动重试同 turn（Pi SDK 语义保持，未在外层加重试器）。

## 4. 取消传播（T04-4）

- chat：UI abort → hub.abort → session-coordinator.abortSession（P02 链：pre-prompt AbortController / _forceReleaseStreamingSession → session.abort() → SDK 取消实际网络请求）。
- callText/operation/media/speech：调用方 signal 与 AbortSignal.timeout 合并（AbortSignal.any）传入 fetch——超时**真实取消**底层请求，不是 Promise.race 假超时留空跑计费（chat.ts:3036 注释“超时即取消 Pi SDK 连接，无空跑”）。
- 取消后的凭证刷新不复活请求：P04-A08（operation client：refresh 完成后 combinedSignal 已 aborted → fetch 立即拒绝、零发送）。
- 终态语义：AbortError → logical_call_aborted + end(aborted)；TimeoutError → errorKind=timeout（callText 侧业务错误 LLM_TIMEOUT 区分保留）；外部取消结果未知时以 aborted 终态如实记录，不补 success。
- 证据：tests/p02-cancellation-edges.test.ts、tests/session-coordinator-isolated-abort.test.ts、tests/model-call-pi-stream-observer.test.ts（abort 终态）、tests/model-operation-client.test.ts（“honors caller cancellation”+“P04-A08”）。

## 5. 本阶段改动

- 新增：tests/model-observability-e2e-chat.test.ts S3（A06 真实协议分片重组）。
- harness 扩展：tests/helpers/model-observability-scenario-harness.ts 新增 `sse-bytes` witness 脚本（字节分片 + 片间延迟投递），既有 7 类脚本零改动。
- 生产代码零改动。
