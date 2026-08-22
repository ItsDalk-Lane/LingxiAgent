# Model Call Payload Capture Boundary Audit（Phase 6 · Step 1）

> 基线：`feature/model-call-observability` @ `ea909c6e`（Phase 5 完成树）。
> 本文是 Phase 6（Sensitive Payload Capture + Redaction + Provider-Wire Provenance）
> 的边界事实源：对 MC-01～MC-10 的每条生产可达 Model Call，重新回答四个层级的
> 运行时对象在哪、credential 在哪、二进制在哪、哪些边界结构性不可见。
> 所有行号为当前工作树实测；Pi SDK 事实来自 node_modules 原厂 0.84.1 产物
> （`scripts/patch-pi-sdk.cjs` 现为只读验证，dist 未被改写）。

## 0. 四个层级的定义（本轮起冻结）

| 层级 | 定义 | Lingxi 侧事实来源 |
|---|---|---|
| Semantic Request | provider 序列化之前，业务语义层的输入（systemPrompt/messages/tools；prompt/references；audio/language） | 各 MC 路径的业务参数对象 |
| Provider Request | compat/协议转换完成后、真正网络发送前的请求（method/endpoint/headers/body） | 自有 fetch 路径 = 构造点局部变量；Pi 路径 = `before_provider_request` hook payload |
| Provider Response | Lingxi 在 transport 层真正可见的响应（raw JSON / parsed / SSE aggregate / status+headers / opaque） | 自有 fetch = `res.json()/res.text()` 消费点；Pi = `after_provider_response`（仅 status+headers） |
| Semantic Response | provider-specific parsing 完成后、业务消费前的模型语义输出（text/reasoning/toolCalls/finishReason/usage/media/taskId/transcription） | 各 MC 路径的 parser 输出点 |

## 1. Pi SDK 0.84.1 Provider Hook 能力实证（MC-01/02/03 判定依据）

### 1.1 `before_provider_request` = 最终 body 的活引用（runtime_exact）

- event 全部字段仅 `{ type, payload }`（`PCA/core/extensions/types.d.ts:503-507`；
  runner.js:785-792 构造，handler 返回值可替换 payload）。
- 触发点在 vendor SDK 序列化**之前**、`buildParams` compat 转换**之后**：
  anthropic-messages.js:363-373（`client.messages.create` 内部才 stringify）、
  openai-completions.js:130、openai-responses.js:100、google-generative-ai.js:41。
- 各 API 下 payload 形状（构造点实测）：
  - **anthropic-messages**：`{model, messages, max_tokens, stream:true, system:[{type:"text",text,cache_control?}]}`（OAuth 时 system 前置一条固定文案）＋条件 temperature/tools/thinking/tool_choice。
  - **openai-chat-completions**：`{model, messages, stream:true, …}`；systemPrompt 不是独立字段，在 `messages[0]`（role=system/developer，openai-completions.js:835-838）。
  - **openai-responses**：`{model, input, stream:true, …}`；**无 instructions 字段**，systemPrompt 进 `input[0]`（openai-responses-shared.js:95-102）。
  - **google-generative-ai**：`{model, contents, config}`；systemPrompt 进 `config.systemInstruction`（:289）；**payload 内嵌 `config.abortSignal`（AbortSignal 对象）**——capture 序列化必须按 unsupported 剔除，不得 JSON 化。
- 凭证**不在 payload**：anthropic `x-api-key`、openai `Authorization: Bearer`、google `x-goog-api-key` 全部由 vendor SDK 在 fetch 层拼装（node_modules client 源码实测）。
- event 上**没有** headers/endpoint/apiKey（间接信息在 ExtensionContext，不采用）。

### 1.2 `after_provider_response` = status+headers（metadata_only），且覆盖有缺口

- shape `{ type, status, headers }`（types.d.ts:517-522）；headers 经 `headersToRecord` 扁平化。
- 每个 HTTP response 完成时一次、SSE 消费之前（anthropic-messages.js:373-379）；
  retry 期间失败的中间 attempt 不触发（onResponse 在 retryProviderRequest 返回后）。
- **Google（generative-ai/vertex）与 Mistral-conversations adapter 从不调用 onResponse**
  → 这些协议下 provider_response 结构性缺事件（诚实缺失，不伪造）。
- **body 不可见** → provider_response fidelity = metadata_only。

### 1.3 retry 与 hook 的相对位置

- `retryProviderRequest`（provider-retry.js:75-93）在闭包内重试；`onPayload` 在闭包外、
  retry 循环之前调用 → **同一 logical attempt 内 before_provider_request 恰好一次**，
  transport retry 复用同一 params 不重复触发（四 adapter 一致）。
- vendor SDK 全部 `maxRetries: 0`。

### 1.4 compaction / direct summary 不走 hook 链（MC-03/MC-10 provider wire 不可见）

- `completeSummarization`（PCA/core/compaction/compaction.js:440-451）的
  `createSummarizationOptions`（:426-432）只含 `{maxTokens, signal, apiKey, headers, env}`
  ——**无 onPayload/onResponse/transformHeaders**；即便复用 `agent.streamFunction`，
  options 展开后 adapter 的 `options?.onPayload?.()` 为 undefined → hooks 不触发。
- 无 streamFn 的 fallback 直连 pi-ai `completeSimple`，同样无 hook。
- 判定：MC-03（native compaction）与 MC-10（diary direct summary）的 provider
  request/response = **unavailable（结构已知、内容不可见）**，不重建。

### 1.5 MC-02 的 hook 可见性由 options 决定

- MC-01（agent loop）：streamFn options 来自 loopConfig（含 onPayload 桥）→ hooks 触发。
- MC-02（cache-preserving runner）：`isolatedStreamFn` 以**自己的 options** 调
  streamFn（cache-preserving-compaction-agent-run.ts:508）→ 无 onPayload → hooks 不触发。
- 运行时可精确判定：`typeof options?.onPayload === "function"`。

## 2. MC 路径四层边界矩阵（构造点实测）

### MC-01 Pi Chat / MC-02 Compaction AgentRun / MC-03 Native Compaction

| 层 | 运行时对象 | 位置 | 备注 |
|---|---|---|---|
| Semantic Request | `context`（systemPrompt/messages/tools） | streamFn wrapper（lib/pi-sdk/model-call-stream-observer.ts:123） | 三路径唯一公共边界；MC-02 为冻结 providerContext |
| Provider Request | `event.payload`（最终 body） | before_provider_request hook | MC-01 runtime_exact；MC-02/03 unavailable（§1.4/1.5）；google payload 内 abortSignal 剔除 |
| Provider Response | `{status, headers}` | after_provider_response hook | metadata_only；google/mistral 缺事件 |
| Semantic Response | `inner.result()` assembled message | observeStreamTerminal | content blocks（text/thinking/toolCall）/stopReason/usage；aborted 可 partial |

### MC-04 callText（core/llm-client.ts）

| 层 | 运行时对象 | 位置 |
|---|---|---|
| Semantic Request | `{ systemPrompt: mergedSystem(\|codex 注入), messages: normalizedMessages }` | §1 merge 完成（:517-530）+ §1.5 provenance（:532-555） |
| Provider Request | `{ method:"POST", endpoint, headers, body }` | body 构造（:574-643）+ `normalizeProviderPayload`（:662）之后、`JSON.stringify`（:672）之前 |
| Provider Response | `data`（parsed JSON）/ codex `readCodexResponsesStream` aggregate | :716-737；error body 同样可读 |
| Semantic Response | `{text, reasoning, finishReason, usage}` | extract + `<think>` strip 之后、空回复校验之前（:759-813） |

- 凭证位置：anthropic `headers["x-api-key"]`（:578）；openai/codex `headers.Authorization`（:605/:620/:632）；codex 另有 `chatgpt-account-id`（非 secret，保留）。
- 协议映射（构造时产生，见 provider-request-provenance.ts）：
  anthropic → `body.system`（string）+ `body.messages[j]`（过滤重排）；
  openai-completions → `body.messages[0].content`（系统消息）+ `body.messages[i+1]`；
  openai-responses → `body.instructions` + `body.input[i]`；
  codex → `body.instructions`（空系统时注入 DEFAULT_CODEX_UTILITY_INSTRUCTIONS=adapter_injected）+ `body.input[i]`。
- `normalizeProviderPayload` 会做结构变换（stripEmptyTools/stripOrphanToolMessages/
  reasoning replay/provider 子模块，provider-compat.ts:353-393）→ mapping 构造后做
  locator 存在性验证（纯路径解析，非内容匹配），失配降级 structural。

### MC-05 Provider Probe（lib/llm/provider-client.ts）

- anthropic 分支（真实最小生成）：semantic request = 固定 `messages:[{role:"user",content:"."}]`（:346，Phase 6 起允许捕获值）；provider request = 内联 body（:343-347）+ headers（buildProviderAuthHeaders:109-144，含 x-api-key）；provider response = `buildProbeResult`（:374-393，成功不读 body → metadata_only；错误 body 已读 → parsed）；semantic response = `{ok, status}`。
- 其它协议 GET /models = CONTROL_PLANE（:306-309 plain fetch），0 record 不变。

### MC-06 Image（7 个 HTTP adapter，body 构造点）

| adapter | body 构造 | 鉴权 headers | 响应读取 |
|---|---|---|---|
| agnes-images | agnes.ts:248-254 | Authorization Bearer（:259） | res.json() :281 / err :274 |
| dashscope | dashscope.ts:244-250 | Authorization + X-DashScope-Async（:257-260） | :284 / :278 |
| gemini | gemini.ts:224-230 | **x-goog-api-key**（:236） | :260 / :251 |
| minimax | minimax.ts:89-107 | Authorization Bearer | :134 / :127 |
| openai-codex | openai-codex.ts:228-237 | Authorization + chatgpt-account-id + OpenAI-Beta + originator（:241-247） | 流式 readStreamingPayload :98-132 → aggregate；401 refresh 重试 :261-268（同 call 两 attempt） |
| openai（含 multipart） | openai.ts:140-188 + FormData :81-93 | Authorization（multipart 时无 Content-Type，由 fetch 生成 boundary） | :232 / :226 |
| volcengine | volcengine.ts:162-208 | Authorization Bearer | :237 / :231 |

- 二进制：参考图在 adapter 内转 data URL / inline_data / base64（gemini:176-187、volcengine:182-196、minimax:102-107、openai-codex localImageToDataUrl:31-41）；响应 b64_json → Buffer（common.ts:32-42 等）→ 全部 externalize。

### MC-07 Dreamina/Jimeng CLI（plugins/jimeng-cli/adapters/dreamina.ts）

- Provider wire 在外部进程内（execFile :111-120），argv（:593-656）与 stdout（parseDreaminaTaskOutput:157-175）**不是 provider wire** → provider request/response = **opaque/external_process**，只发显式 opaque record，绝不 capture argv/stdout 冒充 wire。
- Semantic Request = `params`（prompt/references/generation params）；Semantic Response = `{taskId: submitId}`。

### MC-08 Video

- HTTP video adapter 仅 agnes-videos（agnes.ts:314-379）：body :323-335，Authorization :342-345，taskId 提取 :376-377。
- jimeng-cli-videos 走 MC-07 opaque 分支。
- poll（query :381-421）与资产下载（downloadVideoUrl :111-121）= 控制面，0 record。

### MC-09 Speech（4 个 active adapter，core/speech-recognition/adapters.ts）

| adapter | protocolId | 请求形态 | 鉴权 | audio 形态 |
|---|---|---|---|---|
| openai | openai-audio-transcriptions | **FormData**（model/language/file Blob，:17-20） | Authorization Bearer（:24） | Blob |
| mimo | mimo-chat-completions-asr | JSON（messages=[audioChatMessage]，:58-64） | **`api-key` header**（:55，非 Authorization） | data URL 嵌 input_audio.data |
| dashscope | dashscope-qwen-asr-chat | JSON（+stream:false+asr_options，:100-105） | Authorization Bearer（:97） | data URL |
| volcengine-speech | volcengine-bigasr-transcription | JSON（:143-149） | **X-Api-Key header（:135）+ body.user.uid（:145）** | 裸 base64 嵌 audio.data |

- **Volcengine body credential 实锤**：`body.user.uid = apiKey`（adapters.ts:145）——
  全代码库唯一 body 内 credential；必须由 protocol-specific 结构化规则
  （`volcengine-bigasr-transcription` → path `["user","uid"]`）处理，不得依赖
  generic key denylist（§一百四十九）。
- 响应：`parseJsonResponse`（:216-224）= res.text() 一次性消费（错误 body 同样可读）；
  semantic response = `{text, language?, durationMs?}`（service 侧 speech-recognition-service.ts:495-501 现只记结构，Phase 6 捕获正文经 redactor）。

### MC-10 Pi Direct Summary（lib/pi-sdk/index.ts:236-273 → observed-pi-direct-summary.ts）

- Semantic Request = facade 参数三元组 `{messages, customInstructions, previousSummary}`（全部结构可见）。
- Provider wire = unavailable（§1.4：options 无 onPayload）。
- Semantic Response = `generateSummary()` 返回的 summary 字符串。

## 3. Credential 位置总表（Redaction 覆盖输入）

| 类别 | 出现位置 | 处理 |
|---|---|---|
| Authorization Bearer | callText/probe/agnes/dashscope/minimax/volcengine-image/openai-codex-image/openai-speech/dashscope-speech headers；用户文本 inline（Bearer …） | header 键 denylist + inline detector |
| x-api-key | callText anthropic（:578）、probe anthropic | header 键 denylist |
| api-key（mimo）/ X-Api-Key（volcengine） | speech headers | 归一化键 denylist（apikey / xapikey） |
| x-goog-api-key | gemini image、google chat（vendor SDK） | 归一化键 denylist（xgoogapikey） |
| **body.user.uid（volcengine ASR）** | speech body（adapters.ts:145） | protocol-specific path 规则（唯一 body credential） |
| chatgpt-account-id | codex callText/image headers | **非 secret**（account 标识），保留 |
| OAuth token（Codex） | Authorization Bearer 值 | 同 Bearer |
| Cookie/Set-Cookie | 响应头理论出现 | header 键 denylist |
| Signed URL secret | 媒体响应 URL query（X-Amz-*/X-Goog-*/signature/token…） | sanitizeCapturedUrl → external_reference descriptor |
| 本地绝对路径 | 媒体参考图（params.image）、speech file 路径、tool result 文本 | local_file_reference descriptor + inline 路径替换 |
| 私钥 PEM / JWT / sk- / ghp_ / AIza / AKIA | 用户文本、tool call arguments | high-confidence inline detector（保守 + 正反例测试） |
| FormData Blob / Buffer / base64 / data URL | openai speech file、image multipart、媒体 base64 | external_blob descriptor |

## 4. 控制面（必须保持 0 ModelCallPayloadRecord）

GET /models 探测（provider-client.ts:307）、媒体 poll（agnes.ts:389-401、dashscope.ts:304-306、
poller 调度）、资产下载（common.ts:44-58、agnes.ts:111-121、gemini.ts:168-174）、
credential resolve/refresh（各 adapter getCredentials、openai-codex.ts:266 force refresh）、
credential authorize（dreamina requireExternalCredentialPermit）、媒体能力刷新
（universal-media-manager.ts:1134-1231）、Dreamina query/checkAuth（:500-589）。
这些边界不创建 capture session（不挂 recorder），结构性保证为 0。

## 5. 本轮已知 Opaque / Unavailable（诚实缺失清单）

1. MC-07 CLI provider wire（opaque/external_process）。
2. MC-03/MC-10 provider request/response（unavailable：SDK options 无 hook）。
3. MC-02 provider request/response（unavailable：runner options 无 onPayload；运行时判定）。
4. Pi google/mistral-conversations 的 provider_response（unavailable：adapter 不调 onResponse）。
5. Pi provider request 的 headers/endpoint（hook 不暴露 → 只 capture body，metadata 诚实为 null）。
6. 二进制内容一律 external_blob descriptor（无 Blob Store，不写字节）。
7. openai speech 的 FormData（multipart）：字段值可捕获、Blob externalize。
