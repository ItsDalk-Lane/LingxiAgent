# Lingxi 模型调用可观测性审计

> 审计对象：`ItsDalk-Lane/LingxiAgent` 当前检出版本 `bf3c80b5`（`main`）
> 审计日期：2026-08-21
> 审计方式：只读静态追踪；未修改生产代码、UI、Schema 或测试
> 结论口径：只把最终能向模型、生成服务或语音识别端点发出请求的可达路径算作 Model Call；模型目录、任务状态轮询、凭证授权等控制面请求不算。

# 1. Executive Summary

## 1.1 总体判断

当前 Lingxi **不能**对任意一次模型调用可靠取得并关联完整的：

```text
Identity + Attribution + Prompt/Input + Provider Request
+ Provider Response + Usage + Relationships
```

现状更像一本“费用流水账”，外加散落在会话、媒体任务和业务文件里的结果记录：能知道不少调用消耗了多少，但无法用同一个稳定身份把“为什么调用、实际发了什么、收到什么、重试了几次、属于哪个用户任务”串成一条完整时间线。

按本报告识别出的 **9 种独立调用架构路径**计算：

| 能力 | 结果 | 明确分母与口径 |
|---|---|---|
| 模型调用事实进入 Usage Ledger | `YES 7 / PARTIAL 1 / NO 1` | 9 条路径；`PARTIAL` 是 Pi 普通会话事后补账，`NO` 是 Pi 原生压缩摘要 |
| 完整 Semantic Request 可事后查询 | `LOW` | 0/9 能以 requestId 查询完整语义请求；会话、媒体任务等只保存分散片段 |
| 完整 Provider Request 可事后查询 | `NONE` | 0/9；请求体只在发送前的局部变量、Hook 或外部 CLI 中短暂存在 |
| 完整最终 Semantic Response | `PARTIAL` | 多数业务会保存正文、摘要、文件或转写，但没有统一 requestId 关联；Pi 会话 JSONL 最完整 |
| Provider 原始 Response 可事后查询 | `NONE` | 0/9；HTTP 正文、SDK stream、CLI stdout 都没有作为关联记录保留 |
| Provider usage 值 | `PARTIAL` | 文本 Provider 返回 usage 时可记；多数媒体/语音只有 `usage_missing` |
| 一次任务的完整 Trace | `NO` | 没有稳定 `traceId/rootRequestId/parentRequestId/attemptId`；只有部分 session/task/child 字段 |

这些数字是**架构路径覆盖率**，不是线上调用量占比；仓库没有足够运行数据计算实际流量占比。

## 1.2 最高优先级事实

1. **Pi 普通会话的 Ledger ID 不是网络请求 ID。** 桌面、Bridge、Phone、Subagent 在收到最终 `message_end` 后才调用 `ledger.record()`；请求前不存在该 ID，耗时接近零，也无法覆盖 SDK 内部重试。证据：`core/session-coordinator.ts:558-593`、`core/bridge-session-manager.ts:260-315`、`hub/agent-executor.ts:104-147`。
2. **Pi 原生 compaction 是真实未入 Ledger 的模型旁路。** `pi-compatible` 模式或 cache-preserving 回退会让 Pi 的 `completeSummarization()` 直接经 `streamFn` 请求模型；usage 只进入 compaction entry。证据：`lib/extensions/compaction-guard-ext.ts:237-248,529-550`；依赖 `@earendil-works/pi-coding-agent@0.84.1/dist/core/compaction/compaction.js:440-487`。
3. **Ledger 还混入了不是 Model Call 的记录。** 媒体轮询、非 Anthropic 的 `/models` 探测、外部 CLI 凭证授权都经过 accounting，导致“Ledger entry 数”不等于“模型调用数”。证据：`core/media/poller.ts:331-408`、`lib/llm/provider-client.ts:289-312`、`hub/index.ts:744-776`。
4. **Provider Request/Response 目前均无统一留存。** 自有文本路径在 `fetch` 前有最终 body，Pi 有 `before_provider_request`，但都只短暂存在；Pi 的 `after_provider_response` 只有状态和响应头，没有正文。
5. **Prompt provenance 普遍在业务层被压平。** 后续即使捕获 Provider payload，也不能可靠区分人格、用户档案、记忆、工具结果、系统规则、输出格式要求分别来自哪里。
6. **Usage Ledger 不适合直接扩成 Prompt/Response Store。** 它固定只保留 5000 条、每次追加重写整个 JSON、无游标、无迁移框架，且明确不接收 Key、Header、Prompt 或媒体内容。证据：`lib/llm/usage-ledger.ts:12-13,33-40,129-170`、`lib/llm/model-request-accounting.ts:1-6`。

## 1.3 审计结论一句话

未来需要的是 **一个统一的 ModelCallObserver 协议，加上 Pi、自有文本 HTTP、媒体、语音、Provider 探测五类接点**；其中媒体 HTTP 适配器和外部 CLI 还要分别处理“准确 Provider payload”和“协议不可见”的差异。只在 Usage Ledger 或 `callText()` 加一次埋点，无法覆盖全仓。

# 2. First-Principles Findings

## 2.1 统一术语与判定规则

- **Model Call**：真正向外部或本地模型端点发出的生成、推理或识别请求。一次网络重试是一个 attempt；创建异步媒体任务是 Model Call，后续查询状态不是。
- **Request Identity**：逻辑调用 ID、attempt ID 和 Provider request ID 三层身份。Ledger 的 `requestId` 只有在网络前生成且能贯穿错误/响应时，才可视为模型请求身份。
- **Trace**：同一个用户任务或后台任务触发的多次模型调用之间的 root、parent、task 关系。
- **Semantic Request**：业务已经组装好的系统提示、消息历史、记忆、工具、媒体、任务说明和格式要求。
- **Provider Request**：完成协议兼容和序列化、剥离认证秘密后真正发送的 payload。
- **Semantic Response**：解析后的文本、思考、工具调用、结构化结果、媒体、结束原因和 usage。
- **Provider Response**：原始 HTTP/SDK/CLI 响应。

## 2.2 双向集合闭合

反向出口扫描覆盖：仓库内全部生产 `fetch/fetchImpl`、Pi SDK `streamSimple/completeSimple`、媒体 Adapter、语音 Adapter、`execFile`、SDK create/request、SSE/WebSocket 候选。逐项排除桥接平台、网页搜索、MCP、OAuth、插件下载、模型目录和普通站内网络。

正向业务扫描覆盖：Chat、辅助文本、Memory、Summary、Approval、Vision、Compaction、Subagent、Media、Speech、Provider 测试、Slash Command、后台任务、Skill、Plugin Host 能力。

闭合结果：

```text
业务侧确认的真实 Model Call
  = MC-01 ... MC-09 的上层调用集合

模型网络出口侧确认的真实生成/识别请求
  = MC-01 ... MC-09 的底层出口集合

未匹配的生产网络候选
  = 非模型控制面、普通外部服务，或运行时外部插件 UNKNOWN
```

已知仓库内生产代码没有发现第 10 条未入矩阵的 AI 网络出口。外部安装插件和外部 CLI 内部网络属于静态边界外，明确列入 Unknowns。

## 2.3 三层边界不能混为一层

以滚动记忆摘要为例：

```text
SessionSummaryManager.rollingSummary()       业务调用点
  → callText()                               自有文本调用边界
    → normalizeProviderPayload()             Provider 兼容边界
      → fetch()                              网络发送点
```

以主聊天为例：

```text
Desktop submit / session.prompt()            业务调用点
  → Pi AgentSession / ModelRuntime            SDK 调用边界
    → before_provider_request                 Lingxi 最终 payload 兼容边界
      → Pi provider adapter / SDK client      网络发送点
```

以图片生成为例：

```text
media_generate-image                         业务调用点
  → UniversalMediaManager / submit-image     媒体调度边界
    → adapter.submit()                        协议边界
      → fetch() 或 dreamina CLI               网络/进程出口
```

## 2.4 Usage 事实不等于调用事实

`withModelRequestAccounting()` 只接收归属、模型和 metadata，并明确不接收 Key、Header、Prompt 或媒体。它能回答“这个包装动作成功/失败以及有没有 usage”，不能回答实际请求和响应。相反，Pi 原生 compaction 产生了真实调用和 usage，却不进 Ledger；媒体 poll 不产生模型调用，却进 Ledger。见 `lib/llm/model-request-accounting.ts:1-49`。

## 2.5 可观测性的两个根本断点

- **请求侧断点**：业务层把有来源的信息拼成字符串；Provider 层只能看到扁平结果，无法反推来源。
- **关系侧断点**：现有 session/task/child 字段是局部业务身份，没有覆盖所有路径的 root/parent/attempt 身份，无法把跨子系统调用拼成一棵树。

# 3. Model Call Inventory

## 3.1 独立调用架构路径

| ID | 架构路径 | 真实业务覆盖 | 底层出口 | 是否活跃 |
|---|---|---|---|---|
| MC-01 | Pi 长驻/隔离 AgentSession 流式调用 | Desktop Chat、Bridge、Phone、Subagent、Automation/Workflow child | Pi `streamSimple` → Provider SDK/HTTP | 是 |
| MC-02 | Pi cache-preserving AgentRun 流式调用 | 自动/手动/中途 compaction、fresh compact、deleted-agent continuation、格式/工具恢复 | Pi `runAgentLoop` → 注入的 `streamFn` | 是 |
| MC-03 | Pi 原生 compaction summarizer | `pi-compatible` 模式、cache-preserving 失败后的允许回退 | Pi `completeSummarization` → `streamFn/completeSimple` | 是，且旁路 Ledger |
| MC-04 | 自有 `callText()` 文本 HTTP | Title、Summary、Memory、Dream、Approval、Vision、Diary、Skill、Plugin sample、Health 等 | 自有 `fetch` | 是 |
| MC-05 | Provider Anthropic 连通性探测 | Provider 设置页连接测试，仅 Anthropic 分支 | 直接 `POST /v1/messages` | 是；其它协议分支不是 Model Call |
| MC-06 | 图片生成 HTTP Adapter | Volcengine、OpenAI、Codex、MiniMax、DashScope、Gemini、Agnes | Adapter 内 `fetch` | 是 |
| MC-07 | 图片/视频生成外部 CLI Adapter | Jimeng/Dreamina | `execFile(dreamina, ...)`；CLI 内网络不透明 | 是 |
| MC-08 | 视频生成 HTTP Adapter | Agnes video task submit | Adapter 内 `POST /videos` | 是 |
| MC-09 | 语音识别 HTTP Adapter | OpenAI、MiMo、DashScope、Volcengine BigASR | Adapter 内 `fetchImpl` | 是 |

## 3.2 指定业务类别覆盖

| 类别 | 结论 | 路径/证据 |
|---|---|---|
| Chat / 主会话模型 | ACTIVE | MC-01；`core/session-coordinator.ts:2137-2153,4881-4915` |
| Title | ACTIVE | MC-04；`core/llm-utils.ts:220-275`，调用 `callText` |
| Summary | ACTIVE | MC-04；活动摘要 `core/llm-utils.ts:323-467`、频道摘要 `hub/channel-router.ts:1040-1115`、接管摘要 `core/slash-commands/rc-summary.ts:41-134` |
| Session Summary | ACTIVE | MC-04；滚动摘要及修复 `lib/memory/session-summary.ts:726-760,770-965` |
| Memory | ACTIVE | MC-04；编译压缩 `lib/memory/compile.ts:1054-1091` |
| Memory Reflection | NOT_APPLICABLE | `lib/memory/memory-reflection-runner.ts:79-170` 有实现，但全仓无生产 caller |
| Deep Memory | ACTIVE | MC-04；`lib/memory/deep-memory.ts:256-302` |
| Dream / Memory Dream | ACTIVE | MC-04；atomize/dedupe/optimize/compose/verify，`lib/memory/dream/model-runner.ts:56-91,140-379` |
| Approval | ACTIVE | MC-04；`lib/approval-gateway.ts:620-669` |
| Vision / Auxiliary Vision | ACTIVE | MC-04 独立请求，`core/vision-bridge.ts:872-967`；主 Chat 的原生多模态仍属于 MC-01 |
| Compaction / Session Compaction | ACTIVE | MC-02 与 MC-03；`lib/extensions/compaction-guard-ext.ts:237-550` |
| Fresh Compact | ACTIVE | 刷新 runtime/prompt 后仍进入 MC-02/MC-03；`core/engine.ts:2168-2188`、`core/bridge-session-manager.ts:1821-1825` |
| Subagent | ACTIVE | MC-01；`core/session-coordinator.ts:7963-8187` |
| Agent Executor | PARTIAL | Phone executor ACTIVE：`hub/agent-executor.ts:393-654`；普通 `runAgentSession()` 无生产 caller，故不另计 |
| Workflow / child agent | ACTIVE | MC-01；隔离 automation/subagent 同一创建与 prompt 链，`core/session-coordinator.ts:7778-8187` |
| Image Generation | ACTIVE | MC-06/MC-07；`plugins/media/tools/generate-image.ts:96` → `core/media/submit-image.ts:25-145` |
| Video Generation | ACTIVE | MC-07/MC-08；`plugins/media/tools/generate-video.ts:84` → `core/media/universal-media-manager.ts:711-825` |
| Speech Recognition | ACTIVE | MC-09；`core/speech-recognition-service.ts:278-378` |
| 媒体任务轮询 | NOT_APPLICABLE | `adapter.query()` 只查已经提交的任务状态，不触发生成；`core/media/poller.ts:331-475` |
| Health Check | ACTIVE | MC-04；`server/routes/models.ts:217-260` 发送真实 health prompt |
| 模型连通性 / Provider 测试 | MIXED | MC-05 仅 Anthropic POST 是 Model Call；其它 GET `/models` 是 NOT_APPLICABLE，`lib/llm/provider-client.ts:268-313` |
| Slash Command 内部调用 | ACTIVE | `/rc-summary` 为 MC-04；`/compact`、`/fresh-compact` 为 MC-02/03，`core/slash-commands/bridge-commands.ts:160-231` |
| 后台定时任务 | ACTIVE | 活动摘要、Memory、Dream、Diary 等均为 MC-04 |
| Session Snapshot / Side Task | NOT_APPLICABLE | `lib/llm/session-snapshot-side-task-runner.ts:35-115` 有实现，但其唯一上层 Memory Reflection 当前无生产 caller |
| Agent Appearance Summary | ACTIVE | MC-04，含头像视觉输入；`lib/agent-appearance-summary.ts:273-330` |
| Diary | ACTIVE | MC-04；`lib/diary/diary-writer.ts:715-741` |
| Skill 相关模型调用 | ACTIVE | 安装安全审查 `lib/tools/install-skill.ts:115-155`、名称翻译 `core/llm-utils.ts:281-314`，均 MC-04 |
| Plugin / Runtime 扩展触发 | ACTIVE/UNKNOWN | Host `model:sample-text` 明确走 MC-04，`packages/plugin-runtime/src/index.ts:1321`、`server/index.ts:794-831`；外部插件自行 `ctx.fetch` 的目标静态不可知 |
| CLI 模型调用 | ACTIVE | Jimeng/Dreamina 为 MC-07 |

## 3.3 主调用链图

### Chat

```text
Desktop / Bridge / Phone / Child Agent 入口
  → 组装系统提示、消息、工具、媒体
  → createAgentSession()
  → session.prompt()
  → Pi ModelRuntime.streamSimple()
  → before_provider_request（Lingxi Provider 兼容）
  → Pi Provider Adapter / SDK / HTTP
  → stream delta → Pi assembled assistant message
  → session JSONL / 业务事件
  → message_end 后补写 Usage Ledger
```

### Utility / Auxiliary

```text
业务任务
  → 选择辅助模型
  → 业务 Prompt Builder
  → callText()
  → Provider payload 构造
  → normalizeProviderPayload()
  → usageLedger.start()
  → fetch()
  → raw text / Codex SSE 聚合
  → Provider parser
  → usageLedger.finish()/recordError()
  → text 业务结果
```

### Media

```text
media_generate-image / media_generate-video
  → UniversalMediaManager
  → 参数解析 + TaskStore
  → withModelRequestAccounting()
  → adapter.submit()
     → HTTP fetch（MC-06/08）
     或 dreamina CLI（MC-07）
  → Provider task/file result
  → TaskStore
  → adapter.query() 轮询（控制面，不是 Model Call）
  → 最终文件 / deferred result
```

### Speech

```text
voice SessionFile
  → SpeechRecognitionService
  → 模型/协议/凭证选择
  → withModelRequestAccounting()
  → adapter.transcribe()
  → multipart 或 base64 HTTP request
  → raw JSON parser
  → text/language/duration
  → SessionFile transcription
```

### Subagent / Workflow

```text
Parent session / scheduled automation
  → isolated child session
  → child system prompt + tools + task instruction
  → createAgentSession() → session.prompt()
  → MC-01 Provider 链
  → child session JSONL
  → message_end 后补写带 parent/child/task 字段的 Ledger entry
```

# 4. Network Egress Inventory

| 出口 | 协议/调用方式 | Provider 范围 | 上层模块 | Accounting | Credential resolver | Request policy / compat |
|---|---|---|---|---|---|---|
| Pi Provider Adapter / SDK | `streamSimple`，SDK HTTP/SSE | Pi 运行时支持的 OpenAI Chat/Responses、Anthropic、Google 等；实际模型由 Registry 决定 | MC-01/02/03 | MC-01 PARTIAL；MC-02 YES；MC-03 NO | Pi `ModelRuntime` + auth facade | `core/engine.ts:2491-2528` 的 `before_provider_request` 统一兼容 |
| `core/llm-client.ts:575-579` | HTTP POST；Codex Responses 按 SSE 聚合 | Anthropic Messages、OpenAI Chat、OpenAI Responses、Codex Responses | MC-04 | YES | caller 先解析，`callText` 接收 key/headers | `normalizeProviderPayload()`，`core/llm-client.ts:538-557` |
| `lib/llm/provider-client.ts:297-310` | Anthropic POST 或其它 GET | Provider 设置连接测试 | MC-05 / 控制面 | YES，但混合语义 | `credentialBoundary.consume()` | 独立 probe 构造，无文本统一 compat |
| `core/media-adapters/volcengine.ts:212` | HTTP POST | Volcengine image | MC-06 | YES（外层） | `provider:credentials` | Adapter 自己序列化 |
| `core/media-adapters/openai.ts:209` | HTTP POST images | OpenAI-compatible image | MC-06 | YES | `provider:credentials` | Adapter 自己序列化 |
| `core/media-adapters/openai-codex.ts:238` | Responses POST + SSE；401 可刷新后重发 | OpenAI Codex image | MC-06 | YES；发生 401 refresh 时，一次 Ledger 会包含两次 network attempt | OAuth/Provider bus | Adapter 自己序列化 |
| `core/media-adapters/minimax.ts:108` | HTTP POST | MiniMax image | MC-06 | YES | Provider bus | Adapter 自己序列化 |
| `core/media-adapters/dashscope.ts:261` | HTTP POST async task | DashScope Wan image | MC-06 | YES | Provider bus | Adapter 自己序列化；`:296` GET 为 poll |
| `core/media-adapters/gemini.ts:231` | `generateContent` POST | Gemini image | MC-06 | YES | Provider bus | Adapter 自己序列化；`:168` 为资产下载，不是 Model Call |
| `core/media-adapters/agnes.ts:255` | image generations POST | Agnes image | MC-06 | YES | Provider bus | Adapter 自己序列化 |
| `core/media-adapters/agnes.ts:330` | video task POST | Agnes video | MC-08 | YES | Provider bus | Adapter 自己序列化；`:363/:373` GET 为 poll |
| `plugins/jimeng-cli/adapters/dreamina.ts:111` | `execFile` | Jimeng image/video | MC-07 | YES（外层）+ 凭证授权噪声 | 外部 CLI 登录边界 | CLI 内部 payload/response 对 Lingxi OPAQUE |
| `core/speech-recognition/adapters.ts:20,44,79,110` | multipart 或 JSON/base64 POST | OpenAI、MiMo、DashScope、Volcengine ASR | MC-09 | YES | Speech service fresh resolver | 各 Adapter 自己序列化 |

Pi 依赖源码核验使用当前 lockfile 对应的 `@earendil-works/pi-ai` / `pi-coding-agent` 0.84.1。其 OpenAI Adapter 在 `dist/api/openai-completions.js:102-145` 调用 SDK，`dist/utils/provider-retry.js:75-93` 执行内部 retry；Lingxi 仓库内没有这层网络正文的拥有权。

# 5. Full Observability Matrix

评级：Identity 使用 `FULL/PARTIAL/NONE`；Usage 使用 `YES/PARTIAL/NO`；Prompt provenance 使用 `STRUCTURED_PROVENANCE/FLATTENED/OPAQUE`。这里的“捕获点”表示数据**当前短暂存在且适合未来接 Hook 的位置**，不表示当前已经保存。

| ID | 调用类别 | 业务入口 | Prompt/Input Builder | 调用封装 | Provider/Adapter | 网络出口 | requestId | Trace/Task | Session | source/operation | Prompt provenance | Semantic Request 捕获点 | Provider Request 捕获点 | Semantic Response 捕获点 | Provider Response 捕获点 | Usage Ledger | Error 可观测 | Retry 关联 | 当前缺口 | 建议 Observer 层 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| MC-01 | Chat / Bridge / Phone / Child Agent | `session.prompt()`；`core/session-coordinator.ts:4881-4915,7963-8187`；`core/bridge-session-manager.ts:1246-1393`；`hub/agent-executor.ts:511-654` | Agent system prompt + snapshot + history + tools + native media | Pi `createAgentSession` / `ModelRuntime.streamSimple` | Pi Provider Adapter | SDK/HTTP/SSE | **NONE**：Ledger ID 在 `message_end` 后生成；Provider ID 未采集 | 普通 Chat 无 task/root；Subagent 局部有 parent/child/task | Desktop 有 sessionId/path；Bridge/Phone 有 conversation + path；Subagent 最完整 | `session/reply`、`phone/reply`、`subagent/run`、`automation/run` | **FLATTENED**；snapshot 只保留部分 section | `streamFn(model, context, options)` 前；已含 messages/tools/system | `before_provider_request` 兼容完成后的 payload | Pi 最终 assistant message：text/reasoning/toolCalls/stopReason/usage | SDK stream 内；现有 `after_provider_response` 只有 status/headers | **PARTIAL**：事后、有 usage 才记；错误依赖最终消息 | 最终 provider error 可进 message；崩溃/无最终消息不可见 | Pi 自动 retry 折叠成一条，无 attempt/root | 请求前身份、payload、raw response、attempt、全局 trace 缺失 | Pi stream wrapper + Provider Hook |
| MC-02 | Cache-preserving / fresh / cold compaction | `session_before_compact`；`core/session-compactor.ts:1622,1720-1880`；deleted continuation `core/session-coordinator.ts:4291-4370` | 完整历史、system、tools、compaction instruction；可含 recovery | `runCachePreservingCompactionAgentRun` → `runAgentLoop` | Pi streamFn | SDK/HTTP/SSE | **PARTIAL**：每个 provider turn 网络前 Ledger ID；无 Provider/attempt/root ID | 有 session；recovery metadata，无逻辑 compaction root | 有 sessionId/path/agent；deleted continuation 另含来源路径文字 | `compaction/compact`、`fresh_compact`、`deleted_agent_continue` | **FLATTENED/PARTIAL**：request context 结构化，system 和 instruction 内来源已压平 | `runCachePreservingCompactionAgentRun()` 的 `context/options`，`lib/llm/cache-preserving-compaction-agent-run.ts:342-408` | 复用 MC-01 `onPayload` 后的最终 payload | `message_end` final Pi message；runner 可拿完整 normalized response | Pi Adapter stream 内 | **YES**：每个 AgentRun provider turn start/finish/error | Runner 捕获异常和最终 error/abort | 工具/格式 recovery 有 metadata；Provider SDK retry仍折叠；没有 root | 不能把多个 recovery 与同一逻辑 compaction 稳定关联 | Pi stream wrapper + compaction logical-call metadata |
| MC-03 | Pi 原生 compaction | Pi `session.compact/_runAutoCompaction` | Pi 把 conversation 序列化进 `<conversation>` + summary instruction | Pi `completeSummarization()` | Pi streamFn / completeSimple | SDK/HTTP/SSE | **NONE**：无 Ledger ID、无 Provider/attempt ID | session compaction event 可定位会话，无 request tree | 有 session runtime；usage 随 compaction entry | Ledger 无 source；session event 有 reason | **FLATTENED**：会话序列化成单一用户文本 | Pi `completeSummarization(model, context, options,...)` | 仍经过 MC-01 `before_provider_request` | `generateSummaryWithUsage()` 的 response/summary/usage | Pi Adapter stream 内 | **NO**：usage 只写 compaction entry | `compaction_end` 有失败/abort；Ledger 不可见 | Pi summarization retry 内部，未暴露 attempt | 一类真实调用在 Ledger 完全不可见 | Pi stream wrapper；必须覆盖原生 summarizer |
| MC-04 | Utility / Auxiliary 文本 | Title/Memory/Approval/Vision/Diary/Skill/Plugin/Health 等 | 各业务 builder，最终为 `systemPrompt + messages` | `core/llm-client.ts:401-708` | 自有四协议兼容层 | `fetch` | **PARTIAL**：网络前 Ledger ID；无 Provider/attempt/root | 取决于 caller；多数只有 agent 或 session，Approval 为 unknown | 部分有 session；后台 Memory/Dream 多数只有 agent | 多数有 subsystem/operation；Approval 丢失 | **FLATTENED**；视觉 message/media 有结构，但 provenance 无 source | `callText()` 入口的语义参数，兼容前 | `normalizeProviderPayload()` 返回后、`fetch` 前 `payload/headers` | `callText` parser 的 text/reasoning/stopReason；返回值通常只剩 text/usage | `res.text()` 或 Codex 聚合 stream 在 `llm-client.ts:585-607` | **YES**：start/finish/error/abort；timeout 归为 error | AppError/Abort 写 Ledger；raw error context不入 Ledger | 长度/格式 repair 各有新 ID但无 root；无内建网络 retry | Prompt/response 未保存；provider request ID 仅错误上下文；attribution 不齐 | `callText` lifecycle wrapper |
| MC-05 | Anthropic Provider Probe | Provider 设置连接测试 | 固定 `.`，`max_tokens:1` | `probeProvider()` + accounting | Anthropic Messages | `POST /v1/messages` | **PARTIAL**：网络前 Ledger ID；无 Provider/attempt/root | 无 task/trace | 通常无 session；归属 provider management | `provider-management/connectivity-probe` | **FLATTENED**，但输入简单且固定 | `probeProvider` 构造的 body | `lib/llm/provider-client.ts:296-306` fetch 前 | 只保留 `{ok,status,error}` | response body 仅错误时短暂读取 | **YES**，但同一函数还把 GET `/models` 记入 Ledger | HTTP status/error 可见，成功正文不可见 | 无 retry 关联 | ModelCall 与 catalog GET 共用统计标签，无法区分 | Probe adapter Hook 或并入通用 HTTP observer |
| MC-06 | 图片 HTTP 生成 | `media_generate-image` | prompt + references + ratio/resolution/quality/seed/Provider params | `submitImageGeneration` → `runSubmitInBackground` → `adapter.submit` | 7 个 HTTP image adapters | Adapter `fetch` | **PARTIAL**：Ledger ID + 本地 taskId 在网络前；无统一 Provider/attempt ID | taskId/batchId/session 可局部关联；无 rootRequest | sessionId/path 通常存在；response delivery 可无 session | `media/submit` | 输入对象结构化但**无来源 provenance** | `params` + TaskStore task 创建处 | 每个 Adapter 序列化完成、fetch 前 | Adapter derived `{taskId/files/...}` + TaskStore | 每个 Adapter 内 raw JSON/SSE，未保存 | **YES**；通常 `usage_missing` | 外层捕获异常，TaskStore 保存 failReason | Codex 401 refresh retry 藏在一个 Ledger ID；其它 task retry无统一 attempt | 精确 payload/raw response/usage/provider request ID 缺失 | Media observer + 每个 HTTP Adapter request/response Hook |
| MC-07 | Jimeng 图片/视频 CLI | 媒体工具同上 | prompt、图片、视频参数转成 CLI args | `dreamina` adapter | 外部 Dreamina CLI | `execFile`，网络在进程外 | **PARTIAL**：外层 Ledger ID；Provider request ID不可见 | 本地 taskId/session；无 Provider attempt/root | sessionId/path | `media/submit`；另有 external credential authorization 噪声 | CLI args 有结构，CLI 内 request **OPAQUE** | Adapter submit params / CLI args | Lingxi 最深只能在 `execFile` 前捕获 command+args | CLI stdout 解析后的 task/files/status | CLI stdout/stderr；真正 Provider raw response **OPAQUE** | **YES** 外层；授权动作又误记一条非模型 entry | exit/error/stdout parser 可见 | CLI 内 retry未知，无法关联 | 无法捕获真正 wire payload/response；双重/噪声 accounting | Media observer + ExternalBoundary adapter；声明 captureLevel=opaque |
| MC-08 | 视频 HTTP 生成 | `media_generate-video` | prompt + image + duration/resolution/fps/Provider params | `UniversalMediaManager.submitVideo` → `adapter.submit` | Agnes video | `POST /videos` | **PARTIAL**：Ledger ID 网络前；Provider taskId 响应后才出现 | 提交前 metadata 无 taskId；响应后 TaskStore 有 taskId/session | sessionId/path | `media/submit` | 输入对象结构化但无 source provenance | `params` | Agnes Adapter `fetch` 前 | `{taskId/files}` + TaskStore final asset | raw JSON 只在 Adapter 内 | **YES**，通常 usage_missing | 外层错误可记；提交失败时无 TaskStore task | polling retry不是模型 retry；提交 retry无 root | 提交失败无法用 taskId关联；payload/raw/usage缺失 | Video submit observer + Agnes Adapter Hook |
| MC-09 | Speech Recognition | voice SessionFile | audio binary/base64/data URL + model + language | `SpeechRecognitionService` → `adapter.transcribe` | OpenAI/MiMo/DashScope/Volcengine | `fetchImpl` | **PARTIAL**：Ledger ID 网络前；Volcengine UUID生成但丢失 | 可用 fileId/session，但 Ledger metadata不含 fileId；无 root | sessionId/path | `speech-recognition/transcribe` | 输入对象结构化但 audio source provenance不入 Ledger | `_transcribeWithAccounting` 的 file/target/language | 每个 Adapter fetch 前 | text/language/duration，保存到 SessionFile | `parseJsonResponse()` 内 raw text | **YES**，通常 usage_missing | Service 保存失败状态，Ledger 保存 error | 无统一 retry/attempt；Volcengine request UUID未关联 | fileId、providerRequestId、segments/timestamps、raw response缺失 | Speech observer + Adapter HTTP Hook |

## 5.1 七项能力总表

| 路径 | Identity | Attribution | Prompt/Input | Provider Request | Provider Response | Usage | Relationships |
|---|---|---|---|---|---|---|---|
| MC-01 | NONE | PARTIAL | PARTIAL | transient only | NONE | PARTIAL | PARTIAL，仅 subagent 局部完整 |
| MC-02 | PARTIAL | PARTIAL | PARTIAL | transient only | NONE | YES | PARTIAL |
| MC-03 | NONE | PARTIAL | PARTIAL | transient only | NONE | NO Ledger / compaction entry 有 usage | NONE |
| MC-04 | PARTIAL | PARTIAL | PARTIAL | transient only | NONE | YES | PARTIAL/多数 NONE |
| MC-05 | PARTIAL | PARTIAL | 固定简单输入 | transient only | NONE | YES | NONE |
| MC-06 | PARTIAL | PARTIAL | PARTIAL | transient only | NONE | YES fact / usage_missing | PARTIAL task/session |
| MC-07 | PARTIAL | PARTIAL | PARTIAL/OPAQUE | OPAQUE | OPAQUE | YES fact / usage_missing | PARTIAL task/session |
| MC-08 | PARTIAL | PARTIAL | PARTIAL | transient only | NONE | YES fact / usage_missing | PARTIAL after response |
| MC-09 | PARTIAL | PARTIAL | PARTIAL | transient only | NONE | YES fact / usage_missing | PARTIAL session only |

## 5.2 Usage Ledger 结算位置

| 路径 | `start` | `finish` | `recordError` | 特殊终态 |
|---|---|---|---|---|
| MC-01 | 不在请求前；成功走 `ledger.record()` 内部即时 start，`core/session-coordinator.ts:585-591`；错误在最终 message 后显式 start，`:572-580` | `record()` 内即时 finish | 最终 error message 后调用 | success 无 usage 时完全跳过；abort/崩溃未产生 final message 时无记录 |
| MC-02 | 每次 isolated streamFn 前，`lib/llm/cache-preserving-compaction-agent-run.ts:399-408` | assistant `message_end`，`:354-375` | stream throw、error/abort final 或 runner fail，`:399-430` | 每个工具/格式 recovery 是新账本记录 |
| MC-03 | 无 | 无 | 无 | usage 仅由 Pi 写入 compaction entry |
| MC-04 | `fetch` 前，`core/llm-client.ts:560-564` | parse 完成，`:689-694` | catch，`:697-705` | abort有独立 aborted；timeout仍归 error；Provider无 usage为 usage_missing |
| MC-05 | `withModelRequestAccounting()` 在 probe 前，`lib/llm/provider-client.ts:289-312` | wrapper `lib/llm/model-request-accounting.ts:34-37` | wrapper `:39-48` | 同 wrapper 也包住非模型 GET `/models` |
| MC-06/07 | `adapter.submit` 前，`core/media/image-task-runner.ts:399-420` | wrapper `lib/llm/model-request-accounting.ts:34-37` | wrapper `:39-48` | 通常没有 usage；CLI凭证授权另产生噪声 entry |
| MC-08 | `adapter.submit` 前，`core/media/universal-media-manager.ts:749-765` | wrapper `lib/llm/model-request-accounting.ts:34-37` | wrapper `:39-48` | Provider taskId 在 finish 前结果中存在，但没写进 Ledger metadata |
| MC-09 | `adapter.transcribe` 前，`core/speech-recognition-service.ts:341-378` | wrapper `lib/llm/model-request-accounting.ts:34-37` | wrapper `:39-48` | 通常 usage_missing；fileId不在 metadata |

`usageLedger.start()` 的 pending 只在内存中；success/error/abort 都只有结算后才持久化。Provider 不返回 usage 时 `finish()` 仍会落一条 `usage_missing`，见 `lib/llm/usage-ledger.ts:67-90`。

# 6. Prompt Construction & Provenance Matrix

| 路径/业务 | Prompt/Input 从哪里形成 | 转换链 | 当前 provenance | 最适合的 Semantic Request 捕获点 | 无法从 Provider payload 恢复的信息 |
|---|---|---|---|---|---|
| 主 Chat | 平台说明、环境、用户档案、人格、记忆、工具/技能、会话历史、工具结果、附件 | `Agent.buildSystemPrompt()` → ResourceLoader append/skills/agents files → Pi context transform/compaction → Provider serializer/compat | **FLATTENED**。`core/agent.ts:1268-1413` 虽按 section 拼接，返回一个字符串；`session-meta.promptSnapshot` 只分开少数来源 | Pi `streamFn(model, context, options)` 调用前，同时附上业务侧 section descriptors | 哪段来自人格、用户档案、记忆、规则；某条内容来自工具结果还是用户；注入时间/版本 |
| Subagent/Automation | 目标 Agent 轻量 system、任务说明、parent context、工具 | isolated ResourceLoader → AgentSession context → Pi serializer | **FLATTENED**；parent/child metadata 比普通 Chat 好，但内容来源未保留 | isolated session 创建完、第一次 `session.prompt` 前 | task instruction 与 inherited context 的来源边界 |
| Compaction cache-preserving | live messages、system、tools、压缩 instruction、recovery | normalize provider context → AgentRun context → Pi serializer | **PARTIAL**：消息/工具结构存在，instruction 和 system 内部扁平；metadata 有 cache strategy/hash | `runCachePreservingCompactionAgentRun()` 构造 `context/options` 后 | system 内 section；summary instruction 各规则来源；历史消息原业务来源 |
| Compaction native | Pi 把完整对话转为文本，放进 `<conversation>`，再加 previous summary/instruction | `convertToLlm` → `serializeConversation` → 单一 user text → serializer | **FLATTENED** | Pi `completeSummarization()` 接收 context 时；更好位置是序列化前 | 结构化消息与工具来源一旦序列化后不可可靠恢复 |
| Utility `callText` | 各业务的 system + messages；Memory/Dream/Approval 等经模板拼接 | business builder → `callText` system-message merge → API-specific body → `normalizeProviderPayload` | **FLATTENED**。`lib/llm/prompt-layout.ts:14-39` 仅保留 system/user 与 hash；metadata 又未被 `callText` 落账 | `callText()` 入口参数，需 caller 同时传 prompt sections；Provider payload 捕获在 `normalizeProviderPayload` 后 | 模板版本、记忆来源、审批输入字段、摘要素材来源、格式规则来源 |
| Auxiliary Vision | 文件/图片引用、视觉消息、分析指令、JSON 格式要求 | `vision-bridge` 构造 multimodal messages → `callText` → Provider payload | 图片 block 有结构，来源/用途仍 **FLATTENED** | `core/vision-bridge.ts:917-967` 调用 `callText` 前 | 文件为何进入本次请求、是否用户附件/工具产物、原 session entry |
| Image/Video | prompt、reference image、model、ratio、resolution、quality、seed、duration/fps 等 | normalize input → parameter resolver → adapter-specific payload | 参数 **STRUCTURED**，但 provenance 只是值，没有 source/entry/tool-call 关系 | TaskStore add 前的 resolved input/params | prompt 来自用户原文、Agent 改写还是 Plugin；参考图来源与权限 |
| Speech | voice SessionFile、model、language；Adapter 转 multipart/base64/data URL | SessionFile → service input → protocol body | 文件对象结构化，Ledger 不含 fileId，来源关系 **PARTIAL** | `_transcribeWithAccounting()` 调用前 | 哪条用户消息/哪个附件触发，二进制来源和保留策略 |
| Provider Probe | 固定点号与模型 | probe body → fetch | 固定、可视为结构化常量 | `probeProvider()` fetch 前 | 无复杂 provenance 问题 |

### 6.1 Prompt 发生语义丢失的明确位置

1. `core/agent.ts:1268-1413`：有 section 的构造意图，但返回值是单一字符串；标题不是机器可用 provenance。
2. `core/session-prompt-snapshot.ts:32-80`：保留 `systemPrompt`、`appendSystemPrompt[]`、skills、agents files，但 `systemPrompt` 本身已扁平，且这是会话创建快照，不是逐请求输入。
3. `core/llm-client.ts:439-452`：把 `systemPrompt` 和 system messages 合并；再转成各 Provider body。
4. Pi 原生压缩依赖 `compaction.js:455-483`：把消息序列化进一个文本块。
5. `lib/llm/prompt-layout.ts:14-39` 的 hash/template metadata 被放进 `usageContext.metadata`；`normalizeUsageContext()` 只保留 `source/attribution`，而 `callText()` 启动 Ledger 时未把它传成顶层 metadata。因此这些 provenance 辅助字段实际没有落盘。

### 6.2 当前“保存完整 Prompt”的准确回答

- `session-meta.promptSnapshot` 保存会话创建时的系统能力快照和可选最终 system prompt；**不是本次 Model Call 的完整 Semantic Request**。
- session JSONL 保存 Conversation/工具消息；与 snapshot 组合可以近似重建某一时刻的业务上下文，但缺少请求时精确分支、变换结果、Provider 参数和 requestId；**不是最终 Provider payload**。
- Media `tasks.json` 保存 prompt 和 resolved params；这是**媒体业务请求状态**，不是 Provider payload。
- Memory/Diary 等保存的是业务结果或摘要，不是发送请求本身。
- 因此：当前没有任何可用 requestId 查询的“完整逐调用 Prompt”。

# 7. Response Capture Matrix

| 路径 | Provider raw response 存在位置 | normalized/semantic response | 当前持久化 | Streaming 特点 | 缺口 |
|---|---|---|---|---|---|
| MC-01 | Pi Adapter/SDK stream 内；Lingxi `after_provider_response` 最多 status+headers | Pi assistant message：text、thinking/reasoning、toolCall、stopReason、usage、error | session JSONL/事件保存统一消息；Ledger 只有 usage/error摘要且无 message id | delta/chunk 在 session event，最终 assembled message 在 `message_end` | raw body/chunk协议记录、Provider request id、Ledger-message 关联 |
| MC-02 | 同 MC-01 | AgentRun 的最终 assistant message、tool recovery result、summary | compaction entry 保存 summary/details；Ledger 保存 usage/cache metadata | 不需要保存所有 chunk；final message 在 runner 可可靠取得 | 多个 provider turn 与一个 compaction root 的关联 |
| MC-03 | 同 MC-01 | `generateSummaryWithUsage()` 返回 text+usage | compaction entry 保存 summary/usage | Pi 内 stream 聚合 | Ledger 完全缺失；raw response缺失 |
| MC-04 | `res.text()`；Codex SSE 在 `readCodexResponsesStream()` 聚合，`core/llm-client.ts:274-336,585-607` | parser 内可得 text、reasoning、stopReason、usage；公开返回通常只有 text/usage | 业务各自保存 text；Ledger 保存 usage/error | Codex stream 被聚合，不保留每个 SSE chunk | normalized response字段被裁剪；raw/provider id不存 |
| MC-05 | HTTP response；成功正文没有解析保存，错误可读短文本 | `{ok,status,error}` | Ledger + route result | 非流式 | 成功 Provider response、request id、usage均无 |
| MC-06 | Adapter 内 `json()/text()/SSE` | files、provider task id、revised prompt 等由各 Adapter 各自裁剪 | TaskStore 保存 task/files/failReason；并非所有派生字段都保留 | Codex image 为 SSE，其它多为 JSON | 原始响应、统一 media metadata、Provider request id、usage |
| MC-07 | CLI stdout/stderr；真正 HTTP response 在外部进程 | parser 得到 task/status/files | TaskStore 保存派生结果 | CLI 行为不可见 | Provider raw response永远无法由 Lingxi 层保证获得 |
| MC-08 | Agnes JSON | taskId/files/status | TaskStore | submit非流式，后续 poll | submit response原文、usage、失败时 task关联 |
| MC-09 | `parseJsonResponse()` 读取完整文本，`core/speech-recognition/adapters.ts:185-203` | text、可选 language/duration | SessionFile transcription | 非流式 | segments/timestamps、raw response、Provider id、Ledger-file link |

完整最终语义响应最可靠的现有位置：Pi 为最终 `message_end` assistant message；`callText` 为 Provider-specific parser 结束、返回值被裁剪之前；Media/Speech 为 Adapter parse 完成、业务结果写入 Store/SessionFile 之前。协议级调试的原始 stream/body 必须分别在 Pi Adapter/SDK、`callText` 的 response reader、各媒体/语音 Adapter 或 CLI 边界捕获。

# 8. Identity / Attribution / Trace Matrix

## 8.1 Identity

| 路径 | requestId 何时生成 | Response/Error 能否取到 | Retry 行为 | 评级 |
|---|---|---|---|---|
| MC-01 | Provider 完成后的 `ledger.record()` 内生成 | 只能在事后 Ledger entry；不能回到请求/stream | SDK attempts 折叠；无 logical root | NONE |
| MC-02 | 每个 AgentRun provider turn 调 streamFn 前 | runner success/error/abort 可结算 | recovery 另起 ID，Provider retry折叠；无 root | PARTIAL |
| MC-03 | 不生成 Ledger requestId | compaction entry 无请求身份 | Pi summarization retry不可关联 | NONE |
| MC-04 | `fetch` 前 `usageLedger.start()` | success/error/abort 可结算；进程崩溃 pending 丢失 | repair 另起 ID，无 root/attempt；无 Provider ID | PARTIAL |
| MC-05 | fetch 前 | HTTP error/success可结算 | 无 retry identity | PARTIAL |
| MC-06 | adapter submit 前，且 image 本地 taskId 已存在 | 外层 error/result可结算 | Codex 401 两次 attempt共用一个 Ledger ID | PARTIAL |
| MC-07 | exec 前 | 进程 error/result可结算 | CLI 内完全未知 | PARTIAL |
| MC-08 | adapter submit 前；Provider taskId 响应后产生 | success/error可结算，但失败无 taskId | 无 attempt/root | PARTIAL |
| MC-09 | adapter 前 | success/error可结算 | Volcengine 自建 `X-Api-Request-Id` 未返回；无 attempt/root | PARTIAL |

所有路径都没有稳定 `attemptId`；所有成功路径都没有统一采集 `providerRequestId`。`core/llm-client.ts:127-138` 只在错误上下文尝试提取 Provider `request_id`，它不会进入 Ledger。

## 8.2 Attribution 实际填充

| 场景 | 实际稳定字段 | Schema 支持但调用方不稳定/未填 | 明确缺失 |
|---|---|---|---|
| Desktop Chat | sessionId、sessionPath、agentId、subsystem/operation/surface/trigger | conversationId、taskId | trace/root/parentRequest/attempt |
| Bridge/Phone | agentId、conversationId/type、sessionPath | sessionId 在部分路径不填 | task/root/request parent |
| Subagent | parent sessionId/path、childAgentId、childSessionId/path、taskId、threadId | conversationId | 跨系统 traceId、parentRequestId |
| Automation | agentId、automation kind、source | parent session/task 常为空 | root/parent/attempt |
| `callText` utility | subsystem/operation 多数完整；部分有 agent/session | task、conversation、trigger 取决于 caller | 通用 trace/root/parentRequest |
| Approval | 无：字符串 `usageContext` 被归一为全 unknown | 类型本可接结构对象 | 全部 attribution |
| Memory/Dream/Diary | agentId + memory subsystem/operation | session/conversation/task 多数不填 | 原始触发 turn/root |
| Media | sessionId/path，image taskId 通常在 metadata | video submit前无 taskId；agent/conversation通常不填 | root/parent request/toolCall id |
| Speech | sessionId/path | fileId 未进 Ledger；agent/conversation不填 | root/parent/provider request |
| Provider probe | provider/model + operation | 用户/设置 session不填 | task/trace |

`usageContext` 的 schema 本身允许任意 attribution 字段，但 `normalizeUsageContext()` 只保证四个 source 字段和 attribution kind；类型存在不代表 caller 已填。Approval 的字符串实参是可复现的 `schema_supported_but_not_populated`：`lib/approval-gateway.ts:652-664` → `lib/llm/usage-context.ts:11-31`。

## 8.3 当前 Schema 复用判断

### 可以直接复用

- Ledger `requestId` 作为**账本记录 ID**，但不能直接宣称是网络 attempt ID。
- `sessionId/sessionPath`、`agentId`。
- `conversationId/conversationType`（Phone/Bridge）。
- `childAgentId/childSessionId/childSessionPath/taskId/threadId`（Subagent 局部）。
- `subsystem/operation/surface/trigger`。
- `model.provider/modelId/api`。
- 顶层 `metadata` 容器；仅适合非敏感、小体积结构数据。

### Schema 存在但调用方填充不完整

- `sessionId`：Phone/Bridge 和部分 utility 只传 path。
- `conversationId/type`：Desktop、Media、Speech、Memory 多数不传。
- `taskId`：主要只有 Subagent 和媒体 query；视频 submit、Speech、普通 Chat 无。
- `agentId`：Media/Speech/provider probe 常缺。
- `source/attribution`：Approval 为 unknown；plugin utility 依调用方式而定。
- `metadata`：cache-preserving compaction、media 有；utility prompt-layout metadata 因传递层级错误没有落盘。
- `callPurpose`：存在于 `callText` 请求策略，但未进入 Ledger 的稳定字段，也未覆盖所有 caller。

### 当前完全缺失

- `traceId`、`rootRequestId`、`parentRequestId`。
- 明确区分逻辑调用与网络重试的 `attemptId`。
- `providerRequestId` 的统一字段。
- `promptCategory`、`promptSections` 与 section source/version。
- `requestRef`、`responseRef`、`captureLevel`。
- Ledger entry 与 session message entry、tool call、fileId 的稳定引用。

## 8.4 Task 时间线能否重建

结论：**NO**。

可以按 sessionPath 大致排列普通 Chat；可以用 Subagent 的 parent/child/task 字段重建某一段父子关系；也可以用媒体 taskId 追踪媒体生命周期。但 Approval 会掉成 unknown，Memory/Dream/Diary 没有原始 turn，Vision/Media/Speech 没有 parentRequestId，Pi retry 没 attempt，原生 compaction甚至不进 Ledger。因此无法可靠重建：

```text
某个用户输入
→ Chat
→ Vision / Approval / Subagent / Media
→ 后续 Chat
→ Summary / Memory / Compaction
```

# 9. Bypass Inventory

这里的“旁路”只表示不经过主 Pi Chat 链或不经过预期统一 accounting，不自动等于 Bug。媒体、语音有独立协议是合理的；问题在于它们没有遵守同一份可观测性契约。

| 调用点 | 为什么是旁路 | 绕过的统一层 | Usage Ledger | Request/Response 能否捕获 | 是否建议未来统一 |
|---|---|---|---|---|---|
| `core/llm-client.ts:401-708` | Utility 为避免部分 Provider 流式延迟，直接自有 HTTP | Pi AgentSession / Pi Provider Adapter | YES | exact payload/raw body只在函数局部 transient | 统一 Observer 协议；保留独立 HTTP 实现 |
| Pi 原生 compaction | Pi hook 返回 fallback 后，由依赖自身 summarizer 发请求 | cache-preserving AgentRun accounting | **NO** | Provider payload仍经 Pi hook；最终 summary/usage在 compaction entry；raw无 | 必须纳入 Pi lifecycle observer |
| Media HTTP adapters | 生成协议不是 messages→text | Pi/callText compat、统一 response parser | YES，通常 usage_missing | 只在各 Adapter transient | 统一协议 + Adapter hooks，不强行套文本 client |
| Speech adapters | multipart/base64/ASR 协议独立 | Pi/callText | YES，通常 usage_missing | 各 Adapter transient；final text存 SessionFile | 统一协议 + Speech hooks |
| Anthropic probe direct POST | 固定最小请求，不经 `callText` | 文本 compat 和统一 parser | YES | success raw response不保留 | 可并入通用 probe observer；不能和 `/models` 混记 |
| Jimeng/Dreamina CLI | 网络请求在外部可执行文件内 | Credential resolver后的 Lingxi HTTP 层、Provider compat | YES 外层；另有授权噪声 | command/stdout可见，真实 wire OPAQUE | 统一 ExternalBoundary 事件；不虚构完整 wire capture |
| OpenAI Codex image 401 refresh | Adapter 内刷新 credential 后再次 `fetch` | attempt accounting | 一个 Ledger ID 包两次 attempt | 两次 payload局部可见，无 attempt ID | Adapter 内补 attempt hook |
| Pi SDK provider retry | SDK 对 408/409/429/5xx/网络错误重试 | Lingxi request identity | MC-01最终一条；MC-02每个 Agent turn一条，但仍折叠 SDK retry | SDK 内可见，Lingxi没有 attempt callback | Pi transport/adapter 层需要 attempt 事件 |
| Plugin `model:sample-text` | Plugin Host 触发 utility model | Pi Chat | YES，经 MC-04 | 与 callText相同 | 已有 host choke point，可直接纳入 MC-04 observer |
| 外部插件 `ctx.fetch`/自带 SDK | 安装插件可以请求任意地址，仓库无法知道目标是否 AI | 全部模型调用层 | UNKNOWN | 取决于插件；Host 只能看到通用 fetch | 需要插件声明/权限/observer contract；静态审计不能断言调用 |
| 通用 Shell / MCP 工具执行 | Agent 能执行用户允许的命令或调用外部 MCP；目标进程/服务可以自行使用 AI | 全部模型调用层 | UNKNOWN；只记录普通 tool call | Lingxi 只能看到命令/MCP 请求和结果，不拥有其内部模型 wire | 明确 Observatory 的 host-managed 边界；外部系统只能用声明式 external span 接入 |

## 9.1 不是 Model Call、但当前被记入 Ledger 的控制面

| 控制面动作 | 证据 | 为什么不算 Model Call | 当前影响 |
|---|---|---|---|
| 媒体任务查询 | `core/media/poller.ts:331-408` | 查询已提交任务的状态和资产，不触发生成 | 每次 poll 产生 `media/query`、通常 `usage_missing`，夸大请求数 |
| 非 Anthropic Provider 探测 | `lib/llm/provider-client.ts:289-312` | 只 GET 模型目录 | 与 Anthropic 真正最小生成探测共用 accounting 语义 |
| 外部 CLI 凭证授权 | `hub/index.ts:744-776` | 只签发外部凭证使用许可 | Jimeng 模型调用外额外出现一条 `external-cli` Ledger entry |
| Provider 模型发现/Ollama show | `server/routes/providers.ts:369-437,543-650` | 获取 catalog/模型详情 | 不在 Model Call 矩阵；部分路径不记、probe路径混记 |

# 10. Query Capability Gap

## 10.1 Backend 与 Frontend 对照

后端 `/api/usage/llm` 公开以下组合 filter：`since`、`until`、`attributionKind`、`sessionId`、`sessionPath`、`childSessionId`、`childSessionPath`、`agentId`、`subsystem`、`operation`、`modelId`、`provider`、`status`。证据：`server/routes/usage.ts:6-38`。

前端 action 只接受 `limit/since/until`。证据：`desktop/src/react/settings/tabs/providers/usage-ledger-actions.ts:61-99`。

| 能力 | Backend | Frontend action/UI | Gap |
|---|---|---|---|
| 时间范围 | since + until | 已暴露，用于 daily | 无 |
| 条数 | 最大 2000；日期窗口不传 limit 时读全部保留记录 | 已暴露；recent 固定 500 | recent 图表只看尾部 500 |
| sessionId/path | 支持 | 未暴露 | P3 |
| childSessionId/path | 支持 | 未暴露 | P3 |
| agentId | 支持 | 未暴露 | P3 |
| attribution kind | 支持 | 未暴露 | P3 |
| subsystem/operation | 支持 | 未暴露 | P3 |
| model/provider | 支持 | UI仅在拿回数据后本地分组，不作为查询 filter | P3 |
| status | 支持 | 未暴露 | P3 |
| conversationId/type | 不支持 | 不支持 | P2/P3 Schema/query gap |
| taskId/threadId/pluginId | 不支持 | 不支持 | P2/P3 Schema/query gap |
| trace/root/parent/attempt | Schema不存在 | 不支持 | P2，不只是 UI gap |
| 分页/drill-down | `nextCursor:null` | 无 | P3；不能稳定浏览超过当前窗口的明细 |

## 10.2 四个现有视图的真实数据来源

`UsageLedgerSection` 同时请求两批同一 API 数据：最近 500 条，以及当前 week/month/year 的日期窗口。`overall/category/model` 从最近 500 条在前端聚合；`daily` 从日期窗口批次聚合。证据：`desktop/src/react/settings/tabs/providers/UsageLedgerSection.tsx:26-57,74-80`。

因此：

- 四个视图使用同一 Endpoint 和同一 Entry Schema；不是四套独立统计 API。
- `overall/category/model` 与 `daily` **不是同一批 entries**。
- category/model 只能本地分组，不能点组后向后端组合 filter 做完整 drill-down。
- 后端已有的大多数过滤能力没有从前端 action 暴露。

# 11. Storage Constraints

## 11.1 当前实现事实

| 约束 | 当前事实 | 代码证据 |
|---|---|---|
| 最大条数 | 默认 5000，启动加载也只取尾部 maxEntries | `lib/llm/usage-ledger.ts:12,15-23,147-155` |
| 持久化 | 单文件 JSON，Engine 路径为 `usage-ledger.json` | `core/engine.ts:748-749`、`lib/llm/usage-ledger.ts:163-170` |
| 写入策略 | 每完成/失败一条，先 append/shift，再原子重写整个 JSON | `lib/llm/usage-ledger.ts:33-41,163-170` |
| pending | 只在进程内 `Map`；启动不恢复 | `lib/llm/usage-ledger.ts:23-25,46-64` |
| Schema version | 文件 `version=1`，entry `schemaVersion=1` | `lib/llm/usage-ledger.ts:13,77,103,168,184` |
| migration | 无按版本分派的 migration；加载时只 normalize 当前字段 | `lib/llm/usage-ledger.ts:147-160,176-197` |
| 查询 | 全量内存 filter，再从尾部 slice | `lib/llm/usage-ledger.ts:129-136,240-260` |
| 分页 | 固定 `nextCursor:null` | `lib/llm/usage-ledger.ts:133-136` |
| API 默认 limit | 没日期范围时 500；单次显式 limit 最多 2000；日期窗口无 limit 时读保留集合全部 | `server/routes/usage.ts:26-38` |
| 日期行为 | `endedAt < since` 和 `startedAt > until` 排除，ISO 字符串比较 | `lib/llm/usage-ledger.ts:240-243` |
| Error | 只保存 name/message；不保存结构化 Provider status/request id | `lib/llm/usage-ledger.ts:216-221` |
| Usage | 保存 normalized usage + raw 字段名列表，不保存 raw usage payload | `lib/llm/usage-ledger.ts:72-89,231-234` |

## 11.2 为什么适合 accounting

- 记录小、结构简单、固定上限，原子重写容易理解和恢复。
- normalized token/cost 足够做 overall/daily/category/model 聚合。
- source/attribution/model 能做有限过滤。
- accounting 失败不阻塞模型调用，符合计费旁路的容错需求。

## 11.3 为什么不适合完整 Prompt/Response

1. Prompt、tool result、图片 base64、音频和 Provider response 体积远超小型 usage entry；每次追加全文件重写会产生明显写放大。
2. 5000 条 FIFO 会删除审计证据，且没有引用计数、保留策略、压缩或归档。
3. 没有 pending crash recovery，无法证明进程崩溃前已经发出的请求。
4. 没有游标或索引；查询是内存全扫，无法承担 trace、全文或多维检索。
5. 没有 Blob Store、加密、访问控制、分级保留、删除审计或 redaction pipeline。
6. Schema v1 没有正式 migration dispatcher，直接塞大字段会让旧读写路径和 UI 一起承担风险。
7. `lib/llm/model-request-accounting.ts:1-6` 的安全边界本就明确拒绝 Key/Header/Prompt/Media；不应把这个限制无意间拆掉。

结论：Usage Ledger 可以继续作为 ModelCall 事件的 accounting 投影或索引消费者，但不应直接变成完整 Trace Store/Blob Store。

# 12. Privacy / Redaction Risks

## 12.1 分级规则

- `SAFE_TO_CAPTURE_AS_IS`：非敏感结构身份、时间、状态、模型标识、数值 usage；前提是 metadata 没有偷偷塞内容。
- `REQUIRES_REDACTION`：私人消息、记忆、联系人、工具结果、本地路径、设备信息、系统提示、Provider error 正文、签名 URL。
- `REQUIRES_EXTERNAL_BLOB_STORE`：图片、音频、视频、base64/binary 和大型 stream；observer 只留受控引用与摘要。
- `MUST_NEVER_CAPTURE`：API Key、Authorization、Cookie、OAuth access/refresh token、Provider secret、临时签名 credential。

## 12.2 按路径风险

| 路径 | 主要敏感内容 | 捕获分类 | 依据/说明 |
|---|---|---|---|
| MC-01 Chat | 用户私聊、Memory、联系人/频道、tool results、系统路径、工具 schema、附件 | `REQUIRES_REDACTION`；附件另 `REQUIRES_EXTERNAL_BLOB_STORE` | 完整 context 能包含用户和设备的几乎全部工作上下文 |
| MC-02/03 Compaction | 大段历史、工具输出、文件路径、旧 summary | `REQUIRES_REDACTION`；内嵌媒体应外置 | compaction 正是高密度历史汇总请求 |
| MC-04 Utility | Memory/Dream/Diary、Approval 的动作参数、Vision base64、Skill 内容、Plugin 输入 | `REQUIRES_REDACTION`；视觉 blob 外置 | 不同 operation 需要不同字段白名单，不能统一“全存” |
| MC-05 Probe | body 本身固定简单；headers 含 secret | body 可安全，headers 中认证内容 `MUST_NEVER_CAPTURE` | 只保留 header 名或安全 allowlist |
| MC-06/08 Media HTTP | prompt、参考图、本地路径、base64、最终签名 URL、用户资产 | 文本/路径 `REQUIRES_REDACTION`；二进制/大 base64 `REQUIRES_EXTERNAL_BLOB_STORE` | TaskStore 当前已保存 prompt/path，未来 observer 不能无条件复制 |
| MC-07 CLI | command args 可含 prompt/path；环境/登录 token；stdout 可含 URL | args/stdout `REQUIRES_REDACTION`；credential/env `MUST_NEVER_CAPTURE` | 不应抓整个 environment 或进程命令快照 |
| MC-09 Speech | 原始声音、base64、语言、转写私人内容、文件路径 | audio `REQUIRES_EXTERNAL_BLOB_STORE`；转写/路径 `REQUIRES_REDACTION` | Volcengine body 还把 API key 放入 user.uid，必须剥离 |

## 12.3 最危险的捕获位置

- `callText` 和 Provider Adapter 的 `headers` 同时含 Authorization/API key；捕获 Provider Request 时必须先做 header allowlist，而不是事后正则清洗。
- Speech 的 Volcengine payload 使用 API key 作为 `user.uid`（`core/speech-recognition/adapters.ts:101-130`），只删 Header 仍会泄密；必须有 body 字段级规则。
- 图片/音频 data URL 和 base64 会嵌在部分协议的 JSON body；“JSON 可存”不代表安全。
- Tool Result 能包含环境变量、文件内容、联系人或远端返回；只按 role 判断不能完成脱敏。
- Provider error body存在回显请求片段的风险；Error capture 也必须走同一 redaction pipeline。

# 13. Minimal Observer Boundary

## 13.1 最少接点集合

| Boundary | 覆盖调用 | 无法覆盖 | 可捕获数据与时机 | 需要上层补充 metadata |
|---|---|---|---|---|
| A. Pi Stream Lifecycle | MC-01、MC-02、MC-03 | 自有 HTTP、媒体、语音、probe | streamFn 调用前拿 Semantic Request；`before_provider_request` 后拿最终 body；final assistant message拿 Semantic Response；transport attempt 处拿 retry | trace/root/parent、callPurpose、promptSections、session/tool/message refs |
| B. `callText` Lifecycle | MC-04 | Pi、媒体、语音、probe | 入口拿 semantic input；compat 后/fetch 前拿 Provider Request；parser 后拿 semantic/raw response；catch拿 error | caller 的 prompt provenance、task/trace、operation；Provider request id extractor |
| C. Media Submit Contract + Adapter Hooks | MC-06、MC-07、MC-08 | Speech/文本/probe | Manager 层拿 semantic params/task/session；HTTP Adapter fetch 前后拿 wire；CLI 只拿 command/stdout并标 OPAQUE | parent toolCall/root、reference provenance、blob refs、captureLevel |
| D. Speech Contract + Adapter Hooks | MC-09 | 其它 | service 层拿 file/model/language；Adapter fetch 前后拿协议请求/响应；final transcription | fileId、parent turn/toolCall、blob ref、redaction policy |
| E. Provider Probe | MC-05 | 其它 | probe body、status、error；同时把 `/models` 标为 control-plane | invocation surface/user、probe kind、providerRequestId |

## 13.2 一个 Observer 还是多个

结论：**统一 Observer 协议 + 多 Adapter Hook**。

一个协议可以统一事件语义：logical call start、attempt start、provider request prepared、provider response received、semantic response completed、error/abort、usage settled，以及安全引用。一个物理 Hook 不够：Pi 网络在依赖 SDK 内，`callText` 是自有 HTTP，媒体和语音由各 Adapter 序列化，Dreamina 甚至在外部进程内。

## 13.3 最少埋点的理论覆盖

- 只加 **A + B 两个文本边界**：覆盖 4/9 架构路径（MC-01/02/03/04），并覆盖绝大多数文本业务类别；不能覆盖 probe、图片、视频、CLI、Speech。
- 再加 **C 媒体合同**：可观察调用事实/semantic input/result 达到 7/9（再覆盖 MC-06/07/08），但 HTTP wire 仍需 Adapter hooks，CLI wire仍 OPAQUE。
- 加 **D**：达到 8/9。
- 加 **E**：达到已知路径调用事实 9/9。
- 要达到 **9/9 的准确 Provider Request/Response**，除 A/B/D/E 外，还需每个媒体 HTTP Adapter 的发送/解析 Hook；MC-07 外部 CLI 只能诚实标记 OPAQUE，不能宣称 9/9 raw wire。

## 13.4 必须在业务层补 provenance 的调用

以下信息无法靠 Provider 层自动恢复：

1. Chat/Agent system prompt 中平台、人格、用户档案、Memory、规则、技能、团队资料的 section source/version。
2. Subagent 的任务说明、父会话摘录和子会话自有指令的区别。
3. Memory/Dream/Diary/Session Summary 中旧摘要、增量对话、已有记忆、格式规则分别来自哪里。
4. Approval 中哪个待批准动作、哪些字段被清洗、为什么触发 reviewer。
5. Vision/Media 中参考图来自用户附件、工具产物还是历史资产，以及原 entry/file/toolCall。
6. Speech 中 audio 对应哪个 turn/file，是否用户直接输入或桥接下载。
7. Compaction 中历史消息、previous summary、自定义 focus、恢复指令的分别来源。
8. Plugin sample 中 pluginId、host tool invocation 和插件自定义 prompt section。

# 14. Gap Ranking

## P0

| Gap | 影响 | 证据 |
|---|---|---|
| Pi 原生 compaction 不进 Usage Ledger | 一类真实模型调用在统一账本里完全不知道发生过 | `lib/extensions/compaction-guard-ext.ts:237-248,529-550` + Pi `dist/core/compaction/compaction.js:440-487` |
| 运行时外部插件、Shell、MCP 可在 Host 模型边界外自行调用 AI | 一旦外部代码这样做，现有矩阵/账本完全不可见；这是静态边界 UNKNOWN，不把它冒充已发现漏洞 | `core/plugin-context.ts:539`、`lib/sandbox/exec-helper.ts:49`、`core/mcp/clients/stdio-client.ts:59` |

## P1

| Gap | 影响 |
|---|---|
| 9/9 路径都没有可事后查询的完整 Provider Request/Response | 已知发生调用，也无法重放或审计 wire 事实 |
| MC-01 普通 Pi 会话事后生成 Ledger requestId | ID 不能贯穿请求、错误、超时和 retry |
| Approval usageContext 变 unknown | 调用虽被统计，但不知道为何发生、属于谁 |
| Prompt provenance 在 Agent、Utility、Compaction 业务层压平 | 未来从 payload 永远无法可靠恢复语义来源 |
| Media/Speech 绝大多数只有 usage_missing | 知道调用发生，但不知道真实 token/cost/provider usage |
| Dreamina CLI wire OPAQUE | 无法获得真正 Provider 请求和原始响应 |

## P2

| Gap | 影响 |
|---|---|
| 无 traceId/rootRequestId/parentRequestId/attemptId | 无法重建一个用户任务的完整调用树和 retry |
| Ledger 与 session message/toolCall/fileId 无引用 | 业务请求/响应虽在别处，也无法稳定 join |
| 控制面动作进入 Ledger | 请求数和 usage_missing 统计被轮询/目录/授权污染 |
| Utility prompt layout metadata 未落账 | template/hash/cache provenance 看似存在，实际查询不到 |
| 视频 submit 失败前无 taskId 关联；ASR fileId不进 Ledger | 失败链难以回到业务实体 |
| pending 不持久化 | 进程崩溃时无法证明已发未结算调用 |

## P3

| Gap | 影响 |
|---|---|
| UI action 不公开 10 个以上后端 filter | 已有 session/subsystem/model/status 查询能力无法使用 |
| recent 固定 500、无游标、无 drill-down | 组合分析和历史审计受限 |
| Frontend entry 类型遗漏 metadata、childSessionId 等 | 即使后端返回，前端也不能安全使用完整结构 |

# 15. Unknowns

1. **外部安装插件**：仓库只能确认 Host `model:sample-text` 走 MC-04，也能确认插件有通用 fetch 能力；无法静态确认用户实际安装插件是否调用第三方模型。状态：`UNKNOWN`。
2. **外部 Dreamina CLI 内部**：无法从 Lingxi 源码确认其 HTTP endpoint、重试、Provider request id、raw response 和 usage。状态：`OPAQUE/UNKNOWN`。
3. **所有 Pi Provider Adapter 的逐协议 raw 行为**：已按 lockfile 对应 0.84.1 核验公共 Hook、OpenAI Adapter和 retry choke point；没有把每个第三方 SDK 的内部响应对象都视为 Lingxi 可获得。未逐协议证明的 raw body字段保持 `UNKNOWN`。
4. **运行配置决定可达分支**：静态代码证明 native compaction、各媒体/语音 Provider 等可达，但无法从仓库证明某个用户当前启用了哪一个或历史调用次数。
5. **Provider 自身是否返回 usage/request id**：各供应商和模型会变化；代码未统一抽取时，本报告不假设字段存在。
6. **本地模型端点**：如果 Registry 指向 localhost/Ollama 等，它仍按 Model Call 计；是否出机器不影响可观测性定义。
7. **通用 Shell 与 MCP 的下游行为**：Lingxi 可以启动任意获准命令和外部 MCP server，但无法从仓库静态判断它们是否继续调用模型。它们不计入 9 条“已证实的 Host-managed 架构路径”；若未来要求覆盖，外部进程必须主动上报 external span。

# 16. Final Answers

### Q1. 全仓一共有多少种独立的模型调用路径？

**9 种已由仓库源码证实的 Host-managed 路径**，按共享的业务构造、调用封装和网络发送架构统计：Pi AgentSession、Pi cache-preserving AgentRun、Pi native compaction、`callText`、Anthropic probe、图片 HTTP、媒体 CLI、视频 HTTP、Speech HTTP。Provider 数量、业务文件数量和 retry attempt 不另加路径。外部插件/Shell/MCP 的下游模型行为不可静态枚举，单列 UNKNOWN，不伪造为一个确定数量。

### Q2. 哪些路径进入 Usage Ledger？哪些没有？

- `YES`：MC-02、MC-04、MC-05、MC-06、MC-07、MC-08、MC-09。
- `PARTIAL`：MC-01，只有收到最终 assistant `message_end` 后事后写入；成功无 usage 时不记，内部 attempts 不可见。
- `NO`：MC-03 Pi 原生 compaction；usage 只进入 compaction entry。
- 另外 Ledger 混有媒体 poll、GET `/models` 和 CLI 凭证授权等非 Model Call，不能直接用 entry 数作模型请求数。

### Q3. 哪些调用能稳定关联 session、conversation、task、agent？

- session + agent 最稳定：Desktop Chat、cache-preserving compaction、部分 utility。
- conversation + agent + sessionPath：Bridge/Phone；部分缺 sessionId。
- parent session + child session + child agent + task：Subagent，是当前最完整的局部关系。
- media：session + 本地 taskId；视频提交失败前无 taskId。
- speech：session；Ledger 不含 fileId。
- Memory/Dream/Diary：多为 agent，无原始 session/task。
- Approval：当前全部 unknown。
- 没有一条路径拥有全局稳定 trace/root/parentRequest/attempt。

### Q4. 当前有没有任何地方已经保存完整 Prompt？

没有保存“逐 Model Call 的完整 Prompt”。现有三类易混淆数据：

- `session-meta.promptSnapshot`：业务 system/capability 快照，不含逐请求 history/tool result/final wire。
- session JSONL：Conversation 与工具消息，不是请求时精确 Provider payload。
- media `tasks.json`：业务 prompt + params，不是 Adapter 序列化后的 Provider payload。

### Q5. 当前有没有任何地方可以获得真正发送给 Provider 的完整请求体？

**运行瞬间可以，事后不可以。** `callText` 在 compat 后/fetch 前有最终 body；Pi `before_provider_request` 有序列化 payload；媒体/语音各 Adapter fetch 前有 body。它们没有统一保存、requestId关联或 redaction。Dreamina CLI 的真正请求体在 Lingxi 层不可获得。

### Q6. 当前有没有任何地方可以获得 Provider 原始响应？

**运行瞬间局部可以，事后统一不可以。** `callText`、媒体、语音 Adapter 读取原始正文；Pi raw stream 在依赖 Adapter/SDK 内，Lingxi现有 response Hook只有 status/headers；CLI 只见 stdout。0/9 路径能按 requestId 查询 Provider raw response。

### Q7. 当前能否通过已有数据重建某个用户任务的所有模型调用、顺序和父子关系？

**NO。** session/task 能拼出局部片段，但 Approval、Memory、native compaction、SDK retries 和跨子系统 parent 关系断裂；没有 root/parent/attempt 标识。

### Q8. 哪几个位置最适合作为未来统一 Observer Hook？

五类：Pi Stream Lifecycle、`callText` Lifecycle、Media Submit + Adapter、Speech Service + Adapter、Provider Probe。它们共用一个 Observer 协议；Provider payload必须在认证剥离后、网络发送前捕获，semantic response必须在 parser完成、业务裁剪前捕获。

### Q9. 如果只增加最少量埋点，理论上能覆盖多少调用？

- 两个文本接点 A+B 覆盖 4/9 架构路径及绝大多数文本业务。
- 加媒体合同、Speech、Probe 后，五类接点可覆盖已知调用事实 9/9。
- 要准确覆盖 Provider wire，还必须给媒体/语音 HTTP Adapter 加发送/解析 Hook；外部 CLI只能标 OPAQUE。

### Q10. 哪些调用必须修改业务层才能提供 Prompt provenance？

主 Chat/Agent prompt、Subagent task context、Memory/Dream/Diary/Session Summary、Approval、Vision/Media reference、Speech file、Compaction source sections、Plugin sample 全部需要业务层提供 section/source/version/ref。Provider payload最多恢复“最终发了什么”，不能恢复“每一段为什么出现、来自哪里”。

# 17. Recommended Next Verification

下一阶段只建议验证，不在本报告设计完整实施方案：

1. 用一个可控假 Provider 分别跑 MC-01 至 MC-09，记录真实请求次数，特别验证 Pi SDK retry、Codex image 401 refresh 和 native compaction。
2. 对 Desktop → Vision → Approval → Subagent → Media → Summary/Memory 的组合场景做一次运行时时间线采样，量化现有 join 失败点。
3. 在不保存内容的前提下，先验证五类 Boundary 能否稳定发出 lifecycle 事件，并确认 error/abort/timeout/crash 的终态。
4. 对每个 Provider 协议建立 header/body redaction fixture，重点覆盖 Volcengine ASR body 内 credential、base64、tool result 和签名 URL。
5. 用真实外部 Dreamina CLI 只验证可见边界：command、exit、stdout、task id；明确记录无法进入的 wire 区域。
6. 用独立运行数据核对 Ledger 中非模型控制面条目的比例，再决定查询口径；不要从静态代码伪造线上百分比。
7. 对 Pi 0.84.1 的全部启用 Provider Adapter 做一次 Hook contract 测试，确认 `before_provider_request` 与 final semantic response 的字段稳定性。

---

## 审计自检声明

报告完成前已重新反查：`fetch(`、`fetchImpl(`、`usageLedger`、`withModelRequestAccounting`、`callText`、`stream`、`provider`、`model`、`prompt`、`systemPrompt`、`messages`、`image`、`video`、`speech`、`vision`、`approval`、`summary`、`memory`、`compaction`、`agent`、SDK create/request、`spawn/exec/execFile`。所有确认能发出生成/识别请求的 Host-managed 生产路径均已回填 MC-01 至 MC-09；控制面和普通外部网络已单独解释。通用插件网络、Shell 与 MCP 下游被明确标为 UNKNOWN；除这些运行时边界外，未发现一个已知 AI 网络请求仍游离在矩阵之外。
