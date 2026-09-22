# REQUEST_COMPAT_MATRIX — 最小公共请求与供应商差异（P04-T03）

日期：2026-09-22｜基线 HEAD `3286c96e5`。全部为 UNCHANGED_VERIFIED 的事实登记（本阶段零生产改动）；差异适配各有既有契约测试。

## 1. 公共字段（所有 text/chat 请求共享）

| 字段 | 装配位置 | 说明 |
|---|---|---|
| 模型身份 | model-manager / AuxiliaryModelResolver / ModelOperationResolver | provider+id 联合键；协议 api 显式声明优先 |
| 操作与用途 | callPurpose（callText）/ operation（operation client）/ source.subsystem | 只表达请求目的，不承载模型事实 |
| 输入模态 | convertContentForApi（llm-client.ts:447）+ provider-media-serializer | text/image 块按协议投影；媒体不强塞文本（vision 走图像块或独立媒体协议） |
| 推理配置 | temperature（可选）、thinkingLevel（chat：model-manager.resolveThinkingLevel） | 未传不写入请求体，用 provider 默认 |
| 输出预算 | outputPolicy provider-default/bounded + resolveOutputBudgetFact | 显式 maxTokens 才有 cap；预算来源物化为 attempt 事实 |
| 工具 schema | Pi 会话 customTools→sessionOpts（chat 链） | 工具调用不在 callText/operation 请求面 |
| 超时/取消 | callText AbortSignal.timeout+signal；operation client combinedSignal；chat abortSession | 见 STREAM_RETRY_POLICY |
| 元数据 | provider_request_prepared 结构摘要（messageCount/toolCount/hasSystemPrompt/hasMedia/inputByteEstimate） | 安全门 sanitize，绝不含正文 |

## 2. 协议 family 差异与适配点

### 2.1 callText（core/llm-client.ts，SUPPORTED_BUFFERED_APIS 白名单）

| 协议 | 端点 | 认证头 | 特殊处理 | 契约测试 |
|---|---|---|---|---|
| openai-completions | base+/chat/completions | Authorization: Bearer | 默认路径；quirks 经 provider-compat | tests/llm-client-provider-compat.test.ts、tests/provider-compat/*（16 文件） |
| anthropic-messages | base+/v1/messages | x-api-key + anthropic-version | system/messages 分离；role 过滤 index map | 同上 + e2e harness anthropicMessagesJson |
| openai-responses | base+/responses | Bearer | instructions+input | harness openaiResponsesJson |
| openai-codex-responses | chatgpt.com/backend-api/codex/responses | Bearer + chatgpt-account-id | accountId 从 model headers/token claim 解析；空系统注入固定 instruction（provenance adapter_injected）；**读 SSE 聚合**（readCodexResponsesStream，TextDecoder stream:true） | tests/provider-compat/codex-responses.test.ts |
| google-generative-ai | /models/{id}:generateContent | x-goog-api-key | systemInstruction/contents/parts；usageMetadata 归一 | tests/provider-compat/* + harness |

未列协议 → 显式错误 `No Hana buffered adapter is registered`（未知能力不虚报支持，T03-2）。

### 2.2 操作协议（core/model-operation-client.ts operationDialect，shared/model-operations.ts 冻结清单）

openai-embeddings（默认回退）/ ollama-embed（URL 剥 /v1 补 /api/embed、num_ctx）/ gemini-embed（batchEmbedContents、x-goog-api-key）/ voyage-embeddings（input_type）/ minimax-embeddings（GroupId query 必填、texts/type、base_resp 错误显式抛）/ cohere-rerank（+siliconflow，DashScope compatible-mode 改写）/ voyage-rerank / dashscope-rerank（gte/qwen3-vl 原生嵌套端点 vs qwen3-rerank 兼容端点双分流）。测试：tests/model-operation-client.test.ts、tests/model-operation-protocols.test.ts、tests/model-operation-resolver.test.ts（registry 集成族）。

### 2.3 独立协议（保留，不强统一）

- 媒体：core/media-adapters/*（Codex 图像、即梦等）——独立提交/轮询协议。
- 语音：speech adapters 独立协议（protocolId）。
- 响应归一化：各 operationDialect.normalizeResponse 把厂商形状折成 openai/cohere 形状后走同一校验强度（数量/index/向量维度/分数合法性）。

## 3. 最终 payload 与观测同源（T03-3）

- **callText**：normalizeProviderPayload 之后的同一 `body` 对象（stringify 前）既被 `fetch` 发送（serializedBody）也被 `payloadCapture.captureProviderRequest`（§llm-client.ts:848-855）与 `summarizeCallTextRequest`（结构摘要）消费；凭证头在 capture 之前经统一 Redactor 替换（受控脱敏另做，不重造第二份"看起来一样"的对象）。
- **operation client**：`capture: { method, url, headers, body: dialect.body }` 即真实 fetch 参数（model-operation-client.ts:563）。
- **chat（Pi）**：before_provider_request hook 消费 SDK 真实 payload（e2e S1 断言 hook body ≡ witness body）；MC-02/03 无 onPayload hook 时显式 unavailable，不从语义层重建（诚实缺失）。
- 证据：tests/model-call-payload-calltext/pi/capture/redaction.test.ts、e2e S1/S3（witness 对照）。

## 4. 代理/TLS（T03-4）

全局出站代理：`lib/net/outbound-proxy.ts`（undici ProxyAgent/Socks5，server/index.ts 装配 setGlobalDispatcher），模型 fetch（callText/operation/media/speech）与 MCP/bridge 共用同一全局策略。全仓无 `rejectUnauthorized:false`、无 `NODE_TLS_REJECT_UNAUTHORIZED` 放行（grep 核实）；本阶段未为统一 SDK 开启任何不安全证书或代理绕过。

## 5. 供应商特性保全检查

- quirks（enable_thinking 等）：modelObj.quirks 优先，opts.quirks 仅缺省回退（已标注废弃）。
- provider-compat 16 文件覆盖 agnes/codex/deepseek×2/kimi/longcat/mimo/ollama/qwen/volcengine/zhipu/media-markers/openai-input-audio/openai-video-url/reasoning-replay/tool-pairing——chat 与 utility 共享同一 normalizeProviderPayload（mode 区分）。
- 用户显式配置不支持的协议/操作：model_not_found / protocol_missing / No buffered adapter / MiniMax GroupId 缺失显式报错，均不静默降级。
