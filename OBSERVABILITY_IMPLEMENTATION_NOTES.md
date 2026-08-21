# Model Call Observer — 实现报告（Phase 1 契约 + Phase 2 文本运行时 + Phase 2.5 安全收口 + Phase 3 全路径）

基线：`feature/model-call-observability`（第一轮 main @ e62bb535 之上）。Pi 三件套 0.84.1。
本文档以当前最终代码为准；Phase 1/2 implemented = 第一轮（MC-01～04），Phase 2.5 + Phase 3 implemented = 本轮。

## Current Observer Contract

`lib/llm/model-call-observer.ts`：9 个生命周期事件（名称即契约，第一轮冻结未变）：

```
logical_call_start → attempt_start → provider_request_prepared
→ provider_response_received → semantic_response_completed → logical_call_end
（失败：attempt_error / logical_call_error；中止：logical_call_aborted）
```

公共字段：eventType/timestamp/callId/attemptId/traceId/parentCallId/
providerRequestId/model{provider,modelId,api}/source{subsystem,operation,surface,trigger}/
attribution（与 usage attribution 同形状）/status/details/error{name,message,code}。

投递纪律：所有事件经 `safeEmitModelCallEvent`——observer 抛错/缺失/序列化失败
就地吞掉，绝不影响模型调用（自动化测试锁定：成功/失败双路径）。生产默认
observer 为 noop（`NOOP_MODEL_CALL_OBSERVER`），测试经 `setModelCallObserver`
注入 `TestModelCallObserver`（含 `eventsForCall/attemptsForCall/
assertNoSensitiveContent/assertLifecycle` 辅助断言）。

Recorder 状态机（`lib/llm/model-call-recorder.ts`，§十二）：
- callId 创建即存在（Provider 请求之前）；`beginAttempt` 可重复调用（同 call
  多 attempt）；`endLogicalCall` 恰好一次；
- **logical_call_end 之后一切生命周期方法为 silent no-op**（不 throw、不补
  假事件）——晚到事件被状态机丢弃（Safety D 测试锁定）；
- `attemptErrored` getter 跟踪当前 attempt 是否已投递 attempt_error，避免
  业务层 catch 重复投递。

## Identity Contract

`lib/llm/model-call-identity.ts`：三层身份不变——

- `callId`（`mc_`）：logical model call 稳定身份，**请求/外部执行之前**生成；
  success/error/abort 同一 callId；业务级重发（repair/recovery/retry-image）
  铸新 callId。
- `attemptId`（`ma_`）：每个真实/可观察 attempt 唯一。Codex image 401
  credential refresh = 同 callId 两个 attemptId（自动化测试锁定）。
- `traceId`（`mt_`）/`parentCallId`：caller 显式提供才传，缺省 null，不猜。
- `providerRequestId`：响应头 allowlist（6 个 id 头）+ **string only/trim/
  ≤128 chars**（超长整体丢弃为 null——恶意 Provider 经 x-request-id 塞内容
  无法进入事件；Safety C 测试锁定）。Ledger 侧错误 body 的 request_id 回落
  路径同样过 sanitize。

## Metadata Safety Contract

`sanitizeModelCallDetails`（`lib/llm/model-call-observer.ts`）——Recorder 的
emit 是唯一出口，**所有集成点无法绕过**（机器可执行，不再靠注释）：

1. **键 denylist**：归一化（lowercase + 去非字母数字）后整键匹配 §十全表
   （prompt/systemPrompt/messages/message/content/text/body/rawBody/
   rawResponse/responseBody/responseText/reasoning/toolResult/toolSchema/
   headers/authorization/cookie/apiKey/accessToken/refreshToken/credential/
   secret/token/base64/audio/video/image/imageData/stdout/stderr/commandArgs/
   args/environment/detail/error/transcription/payload/request/response/
   filename/filepath/signedUrl…）。命中即丢弃（fail closed）。
   `hasText`/`messageCount` 等布尔/计数键与被禁的 `text`/`message` 是不同
   归一键，不受影响（整键匹配，无子串误伤）。
2. **值形状 gate**：string（≤256 截断）/finite number/boolean/null 放行；
   嵌套 plain object（深度 ≤2、键 ≤32）/array（≤32 项）递归同规则；
   function/symbol/bigint/超深结构/剥空的嵌套对象一律丢弃。
3. typed builders（`summarizeProviderRequestPayload`/
   `summarizeAssistantMessage`/各 adapter requestDetails）继续作为第一层：
   只构造结构性 metadata；runtime gate 是最终防线，不是脱敏管道。

## Error Safety Contract

`normalizeModelCallError` 重定义为**安全错误事实**：

```
{ name（截断）, code（内部错误码，如 LLM_RATE_LIMITED）, message（仅 safe 标记） }
```

- Safe-message contract：唯一合法入口 `markModelCallSafeMessage(err, text)`
  （`Symbol.for("lingxi.modelCallSafeMessage")`）。只有仓库内部固定文案
  被标记（callText 的 invalid-JSON、empty-after-thinking）。
- **Provider 返回的不可信文本（error.message fallback rawText、error.detail、
  JSON error body、CLI stdout、stream errorMessage）一律 message=null**。
  第一轮的泄漏链（provider body → AppError.message → observer event.error.
  message）在唯一出口被切断；业务层错误信息对调用方保持原样（测试断言
  设置页 probe error 仍含 provider message、transcription 失败态不变）。
- `logical_call_error`/`attempt_error` 的 details 继续携带 errorKind
  （abort/timeout/network/http_error/provider_or_network/adapter_error/
  external_process）+ httpStatus 数值。

## Attempt Visibility Contract

`MODEL_CALL_ATTEMPT_VISIBILITY = exact | logical_boundary | external_process_boundary`
（§五十三，禁止自由字符串）：

- `exact`：Lingxi 亲见网络边界——MC-04/05/06/08/09 的每个 fetch。
- `logical_boundary`：MC-01/02/03 的一个 streamFn 调用折叠一次 attempt；
  pi-ai `retryProviderRequest` 内部 transport retry 不可见（0.84.1 实证，
  `tests/model-call-pi-retry-visibility.test.ts` 直驱真实实现锁定）。
- `external_process_boundary`：MC-07 Dreamina CLI 的 execFile 边界。

## Provider Wire Visibility Contract

`MODEL_CALL_PROVIDER_WIRE_VISIBILITY = request_response | response_only | opaque`
（§五十四）。HTTP attempt helper 发 `request_response`；CLI 发 `opaque`。
「没捕获」（MC-03 summarizer 不触发 payload hook）不写该字段——与「理论上
不可捕获」（opaque）不是同一种缺失，不混用。

## MC-01～MC-09 Coverage Matrix（§六十五）

| Path | Logical Call | Pre-request callId | Attempt visibility | Request boundary | Response boundary | Semantic response | Ledger correlation | Provider wire visibility | Control-plane clean |
| ---- | ------------ | ------------------ | ------------------ | ---------------- | ----------------- | ----------------- | ------------------ | ------------------------ | ------------------- |
| MC-01 Pi AgentSession（Chat/Bridge/Phone/Subagent） | FULL | FULL | PARTIAL | FULL | FULL | FULL | NONE | FULL | FULL |
| MC-02 Pi cache-preserving AgentRun | FULL | FULL | PARTIAL | FULL | FULL | FULL | FULL | FULL | FULL |
| MC-03 Pi native compaction summarizer | FULL | FULL | PARTIAL | NONE | NONE | FULL | NONE | NONE | FULL |
| MC-04 callText() utility | FULL | FULL | FULL | FULL | FULL | FULL | FULL | FULL | FULL |
| MC-05 Anthropic generation probe | FULL | FULL | FULL | FULL | FULL | FULL | FULL | FULL | FULL |
| MC-06 图片 HTTP adapters（7 个） | FULL | FULL | FULL | FULL | FULL | FULL | FULL | FULL | FULL |
| MC-07 Dreamina/Jimeng CLI | FULL | FULL | OPAQUE | NONE | NONE | FULL | FULL | OPAQUE | FULL |
| MC-08 视频 HTTP（Agnes video） | FULL | FULL | FULL | FULL | FULL | FULL | FULL | FULL | FULL |
| MC-09 Speech Recognition（4 个 adapter） | FULL | FULL | FULL | FULL | FULL | FULL | FULL | FULL | FULL |

口径说明：

- **Pre-request callId**：所有 9 条路径的 callId 都在网络请求/外部进程执行
  之前生成（矩阵全 FULL；第一轮已达成 MC-01~04，本轮补 05~09）。
- **MC-01/03 Ledger correlation NONE**：message_end 事后 record / compaction
  entry 不携带 callId——第一轮明确留给「Observer → Accounting Projection」
  阶段，本轮任务书 §50 只要求 MC-05～09 关联（全 FULL）。
- **MC-07 Request/Response boundary NONE + wire OPAQUE**：CLI 内部 HTTP 对
  Lingxi 理论上不可捕获——诚实缺失，不伪造 provider_request_prepared/
  provider_response_received（§八十六）。
- **MC-03 Request/Response boundary NONE**：0.84.1 summarizer options 不含
  onPayload/onResponse，事件缺失即真相（第一轮实证）。
- **Attempt visibility PARTIAL（MC-01/02/03）**= logical_boundary；FULL =
  exact；MC-07 = OPAQUE（external_process_boundary）。
- **Control-plane clean FULL**：probe GET /models、media poll、credential
  authorization 已全部出 ledger 且 0 observer 事件（§六十四测试锁定）。

## 接入位置（本轮 MC-05～MC-09）

统一 helper：`lib/llm/model-call-integration.ts`（唯一 integration layer，
复用同一 Recorder/Observer，无第二套事实系统）：

- `observedProviderFetch(carrier, fetchFn, {requestDetails})`——HTTP attempt：
  attempt_start(exact/request_response) → provider_request_prepared →
  provider_response_received(status+allowlist id) → !2xx 自动 attempt_error
  （http_error）→ fetch throw attempt_error（abort/timeout/network）+ rethrow。
  **可重复调用**（同 call 多 attempt）。
- `observedExternalProcessRun(carrier, runFn, {details})`——CLI attempt：
  external_process_boundary + opaque，不伪造 wire 事件。
- `beginObservedModelCall({model, usageContext/source/attribution, details})`
  ——业务边界 bootstrap：铸 callId + logical_call_start。
- `failObservedModelCall(recorder, err, {errorKind})`——logical_call_error +
  end(error)；attempt 级错误已在失败点投递，不重复。

| 路径 | logical call 边界 | attempt 边界 |
|---|---|---|
| MC-05 | `lib/llm/provider-client.ts` probeProvider（仅 anthropic-messages 分支） | 同文件 observedProviderFetch |
| MC-06 | `core/media/image-task-runner.ts` runSubmitInBackground | 7 个 adapter 的 submit fetch：volcengine/openai/openai-codex(×2)/minimax/dashscope/gemini/agnes-image |
| MC-07 | 同 MC-06（dreamina adapter 经同一 runSubmitInBackground） | `plugins/jimeng-cli/adapters/dreamina.ts` observedExternalProcessRun |
| MC-08 | `core/media/universal-media-manager.ts` submitVideo | `core/media-adapters/agnes.ts` agnesVideoAdapter.submit fetch |
| MC-09 | `core/speech-recognition-service.ts` _transcribeWithAccounting | `core/speech-recognition/adapters.ts` 4 个 adapter 的 fetchImpl |

Recorder 经业务边界显式注入（`ctx.modelCall` / `input.modelCall`）——
「用了 accounting wrapper」不自动推断「是模型调用」（§十五/§十六）；无
recorder 的调用点是纯 passthrough。

## Usage Ledger Correlation

- MC-05～09 每个真实 logical call 的 ledger entry 都带
  `metadata.modelCallId === observer.callId`（probe/media-image/media-video/
  speech 四处写入点，自动化测试逐路径断言）。
- 无双计（§七十七）：observer 不写 usage record；每条 generation 恰好 1 条
  ledger entry（Codex 401 refresh 的 2 次 HTTP attempt 共享同一条 entry——
  这正是 attempt/call 两层身份分离的意义）；usage_missing 保留
  （真实模型调用 + Provider 无 usage 仍是 1 条 record）。

## Control Plane Exclusions（§七十三）

ModelCallObserver 明确不接收控制面事件；以下动作保留行为/诊断但不进
observer、不进模型用量 ledger（`tests/model-call-control-plane.test.ts` +
`tests/model-call-probe-observer.test.ts` + `tests/media-poller.test.ts` +
`tests/jimeng-cli-runtime-integration.test.ts` 锁定）：

- **媒体 poll/query**（`core/media/poller.ts`：withModelRequestAccounting
  移除；原先每次 poll 产生一条 media/query usage_missing 污染统计）。
- **非生成 probe GET /models**（`lib/llm/provider-client.ts`：accounting
  只保留 Anthropic generation 分支）。
- **外部 CLI credential authorization**（`hub/index.ts`
  provider:authorize-external-credential-use：withModelRequestAccounting
  移除；许可签发/拒绝逻辑与诊断不变）。
- credential resolve/refresh（bus handler 本就不记账）、asset download、
  local file save、Gemini 参考图下载（remoteImageToInlinePart 不经 attempt
  helper）均不是 Model Call。
- codex 401 的 credential refresh 本身：0 logical call（callIds 长度 1 锁定）。

## OpenAI Codex 401 refresh 实际事件序列（§二十六验收）

```
logical_call_start
attempt_start A / provider_request_prepared A / provider_response_received A (401)
attempt_error A (http_error 401)
[credential refresh —— 控制面，0 事件]
attempt_start B / provider_request_prepared B / provider_response_received B (200)
semantic_response_completed / logical_call_end(ok)
```

same callId、distinct attemptId、A 错 B 成、end=ok——
`tests/model-call-media-observer.test.ts` 精确断言此序列。

## Retry Reality（第一轮结论保持）

pi-ai transport retry（408/409/429/5xx）循环内部无 hook → MC-01/02/03
仍是 logical_boundary 折叠；pi-coding-agent 语义 retry 重新调用 streamFn =
新 logical call（诚实建模）；MC-02 tool/format recovery 每次新 callId；
MC-04 无内部 retry（exact）。

## Safety（毒丸测试矩阵）

- Safety A：provider error body（message/detail/raw/HTML 三形态毒丸
  `TOP_SECRET_PROVIDER_RESPONSE_8F91C2`）不进任何事件序列化。
- Safety B：details 毒丸键（prompt/authorization/rawBody/apiKey/… +
  raw_body/RESPONSE-TEXT 变体）被 gate 剥离；值形状 gate 剥 function/
  超深/超长。
- Safety C：providerRequestId 超长丢弃。
- Safety D：end 后 no-op。
- Safety E：observer throw 业务正常。
- 媒体：prompt/参考图 URL/apiKey/CLI args/stdout/本地路径毒丸不进事件；
  语音：音频字节/base64/转写正文/apiKey（含 Volcengine body 内 user.uid
  credential——前提用断言复现）不进事件；允许 fileId/audioFormat/
  languageSpecified/inputSizeBucket（只读 size，不复制 Buffer、不 hash）。
- 性能（§七十）：无 payload stringify、无深拷贝、无媒体哈希、无同步持久
  化；结构摘要只遍历顶层数组计数。

## Tests（第一轮 42 + 本轮新增）

本轮新增 5 个测试文件（37 用例）+ 更新 4 个既有文件锁定新契约：

- `tests/model-call-safety-gate.test.ts`（8）：Safety A–E + 多 attempt 状态。
- `tests/model-call-probe-observer.test.ts`（6）：MC-05 双分支。
- `tests/model-call-media-observer.test.ts`（15）：7 adapter coverage registry
  （经真实 runSubmitInBackground 业务链）+ Codex 401 硬验收 + Dreamina
  opaque + Agnes video + poll 0 事件。
- `tests/model-call-speech-observer.test.ts`（6）：4 adapter coverage（经真实
  SpeechRecognitionService）+ 500 错误态 + Volcengine credential-in-body。
- `tests/model-call-control-plane.test.ts`（2）：poll / credential auth
  0-event + 0-ledger。
- 更新：`model-call-observer.test.ts`（错误契约）、`model-call-pi-stream-
  observer.test.ts`（provider 流错误正文不进事件）、`media-poller.test.ts`
  （query 0 ledger）、`jimeng-cli-runtime-integration.test.ts`（许可 0 ledger）。

## 门禁（仓库既有机制，按规程处理）

- `export-manifest.json`：收录 `lib/llm/model-call-integration.ts`（同组先例）。
- `build/open-boundary-baseline.json` + `build/cli-runtime-closure.json`：
  compute-cli-closure.mjs 复核后无新 debt（新边全部因 manifest 收录消失）。
- `build/persistence-schema-fingerprint.json`：compatible review repin
  （Observer runtime only，无 store 形状/格式变更；guarded 模块 agnes/
  universal-media-manager/hub 只有观测接线）。
- post-verification seal：本轮完成后按既有机制推进 VERIFIED_SOURCE_SHA
  （单独 audit commit，见下）。

## Remaining Gaps（本轮不做，§八十七）

- 全局 Trace propagation 尚未实现（traceId/parentCallId 契约就绪，caller
  不提供即 null；不根据 sessionId/taskId/时间猜）。
- Prompt provenance 尚未实现。
- Request/Response capture 尚未实现（fetch 前位置已被 lifecycle 命中并
  验证，但本轮零 body 保存）。
- Redaction Pipeline 尚未实现（metadata gate 是 fail-closed 门，不是脱敏
  管道）。
- Trace/Payload/Blob Store 尚未实现；Query/Export/UI 尚未实现。
- Pi SDK transport retries 仍只有 logical_boundary（事实仍如此；需 pi-ai
  上游 onAttempt 或 fetch 层埋点）。
- Dreamina provider wire 仍 OPAQUE（结构性不可见）。
- MC-01 message_end ledger record / MC-03 compaction entry 未携带 callId
  （Observer → Accounting Projection 阶段）。
- diary-writer `generateSummary` 直发（不走 agent.streamFunction/callText）
  与 session-snapshot-side-task-runner 的 completeSimple 回落（无生产
  caller）——范围外已知旁路，未接 observer。
- MC-01 CLI surface 落 desktop 的既有偏差（第一轮已记录）。

## Next Phase

Trace propagation（全局 traceId 根系 + parentCallId 自动建立）→ Prompt
provenance → Request/Response capture + Redaction Contract → Payload/Blob
Store → Query Service → Export → UI。
