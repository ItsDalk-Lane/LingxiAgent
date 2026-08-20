# BLOCKED

影响正确性、需要待裁决/上游修复的事项。处理方式：记录 → 跳过该子功能 → 继续其他任务。

## 2026-08-20 v0.447.4 上游同步

（本轮暂无）

---

## 2026-08-16 供应商模型统一化 + 联网/结构化输出任务

### 1. Moonshot native web search wire contract 无可靠证据

Moonshot 当前仓库 provider adapter（`lib/providers/moonshot.ts`，openai-completions）、
vendored Pi SDK、既有 fixture/test 中均无 `builtin_function` / `web_search` wire contract 证据，
本次执行环境也未核验官方协议文档。`resolveNativeWebSearchContract` 对 moonshot 返回
unsupported（fail closed），不猜参数。

### 2. Anthropic Messages 原生联网：Pi SDK 解析层无法处理 server-tool 生命周期

pi-ai 0.84.1 `anthropic-messages.js` 的 `content_block_start` 只处理
`text | thinking | redacted_thinking | tool_use`；`server_tool_use` / `web_search_tool_result`
块被静默丢弃；`pause_turn` stop reason 被映射为普通 stop，continuation
（原样提交 assistant content + 携带相同 server tool 继续）无法完成。
在 parser/lifecycle 解决前，Anthropic native web = unsupported（fail closed），
不做 lossy response stripping。

### 3. OpenAI Responses 原生联网：`web_search_call` 响应 item 被 Pi SDK 丢弃

请求侧注入 `tools: [{ type: "web_search" }]` 可行，但 pi-ai 0.84.1
`openai-responses-shared.js` 的 `createSlot()` 只为
`reasoning | message | function_call | custom_tool_call` 建 slot，
`web_search_call` item 不进入解析结果；且 Lingxi 走 `store: false`，
回放要求保留完整 item 列表。在 SDK parser 支持前，OpenAI Responses
native web = unsupported（fail closed）。

### 4. 智谱 GLM 的结构化输出（response_format JSON mode）无可靠证据

智谱官方文档确有联网搜索 tools 契约（已实现 web-search contract），但
`response_format: { type: "json_object" }` 在智谱 GLM 上的结构化输出契约
本次未取得可靠的官方/fixture 证据，因此 `resolveStructuredOutputContract`
对智谱（zhipu/zhipu-coding）的 openai-completions 返回 unsupported（fail closed），
不套 OpenAI 参数。其余 openai-completions 协议的 json_object 属协议标准 JSON mode，
按 model.api 声明支持（不依赖 hostname 猜能力）；用户显式开启后若 endpoint 拒绝
JSON mode，请求层显式透传 provider 错误，不静默退回普通文本。

### 5. openai-responses 非官方 endpoint 的结构化输出

`text: { format: { type: "json_object" } }` 的 Responses wire shape 依赖官方实现；
第三方 Responses 兼容网关未经 fixture 验证，`resolveStructuredOutputContract`
对非 OpenAI 官方 endpoint 的 openai-responses 保持 unsupported（fail closed）。

（原「待裁决」项——DailyBars 原生 title 是否彻底删除——已于 2026-08-13 经用户拍板解决：
title 从 `.usage-day-label` 移除，`UsageLedgerSection.test.tsx` 的探针从 title 换成 aria-label。）
