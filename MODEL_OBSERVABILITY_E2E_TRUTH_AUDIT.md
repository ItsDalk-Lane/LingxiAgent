# Model Observatory — Phase 10 End-to-End Truth Audit（测试设计 + 审计总纲）

> 基线：`feature/model-call-observability` @ `d0b65509`（PHASE10_START_SHA；Phase 9
> verified tree `61779cbd`）。本文档**先于验证编码**写成，是 Phase 10 全部 E2E /
> Adversarial / Hardening / Acceptance 测试的设计契约（任务书 §三：先写审计，再跑验证）。
> 当前代码是唯一事实源；行号以 Phase 10 开始时工作树为准。

本轮证明的目标（不是新增功能）：

```text
Provider 实际收到什么 = Lingxi 认为发了什么 = Observer 记录 = Payload Capture
= SQLite = Query = HTTP = UI = Export
```

任何一层缺失都被表达为真实 missing-state，绝不以 0/{} /[]/null/“正常”掩盖。

---

## 1. Production Model Egress Matrix（Step 1 重扫结论，2026-08-22）

重扫口径：`streamSimple` / `completeSimple` / `completeSummarization` /
`generateSummary` / `runAgentLoop` / `streamFn` / `callText` / `fetch(` /
`fetchImpl` / `adapter.submit` / `adapter.transcribe` / `execFile` / `spawn`
/ Provider SDK completion/message 方法，覆盖 lib / core / hub / plugins /
server / packages / cli 生产代码。

### 1.1 结论：Host-managed 生产可达路径 = 10（MC-01～MC-10，无 MC-11+）

| ID | 路径 | 出口 | Observer 边界（既有） | Phase 10 变化 |
| --- | --- | --- | --- | --- |
| MC-01 | Pi AgentSession 流式（Desktop/Bridge/Phone/Subagent） | Pi Provider Adapter/SDK | model-call-stream-observer | 无 |
| MC-02 | cache-preserving AgentRun compaction | Pi streamFn | runner isolatedStreamFn | 无 |
| MC-03 | Pi native compaction summarizer | Pi completeSummarization→streamFn | isCompacting 分支 | 无 |
| MC-04 | callText 自有四协议 HTTP | core/llm-client.ts fetch | llm-client 统一 observer | 无 |
| MC-05 | Anthropic POST probe | provider-client observedProviderFetch | probe observer | 无 |
| MC-06 | 图片 HTTP ×7 adapter | adapter fetch（observed） | image-task-runner | 无 |
| MC-07 | Dreamina/Jimeng CLI | execFile | image-task-runner + external_process | 无 |
| MC-08 | 视频 HTTP（agnes submit） | adapter fetch（observed） | universal-media-manager | 无 |
| MC-09 | Speech ×4 adapter | fetchImpl（observed） | speech-recognition-service | 无 |
| MC-10 | Pi direct summary（diary 临时摘要） | completeSimple（经 facade） | observed-pi-direct-summary | 无 |

### 1.2 非 Model Call 出口（重扫复核，全部保持控制面语义）

- `fetch` 直接站点：`lib/llm/provider-client.ts:307`（GET /models 目录）、
  `core/media-adapters/common.ts:47` / `gemini.ts:169` / `agnes.ts:112,417,427` /
  `dashscope.ts:317`（资产下载 / poll）、`lib/bridge/*`（微信/桥接平台）、
  `lib/pi-sdk/search-tools.ts` + `lib/tools/web-fetch.ts` + `web-search.ts`
  （网页搜索/抓取）、`server/cli.ts` + `server/routes/bridge.ts` +
  `server/routes/providers.ts`（目录/发现/桥接控制面）。
- `execFile`/`spawn`：`plugins/jimeng-cli/lib/dreamina-capabilities.ts`（版本/
  能力发现）、`server/routes/media.ts:364`（openWithSystem 本地打开）、
  sandbox / shell / terminal / MCP stdio / computer-use / office html-to-pdf /
  desk（通用进程执行边界，静态 UNKNOWN 外部域，不是 Lingxi AI egress）。

### 1.3 LATENT / NOT_CURRENTLY_REACHABLE（不编号，不制造生产行为）

| 项 | 事实 | 处置 |
| --- | --- | --- |
| `lib/llm/session-snapshot-side-task-runner.ts:94` completeSimple | 唯一上层 memory-reflection-runner 仍无生产 caller | 沿用 Phase 3.5 结论 |
| **`core/media/local-cli-wrapper.ts` runLocalCliMedia**（本轮重扫登记） | 通用「本地 CLI 媒体生成」包装（可执行任意 CLI 产图/产视频）；全仓零 importer（provider-registry 只 import 同目录 media-runtime-contract 类型），无生产路径 | LATENT。若未来接线必须先走 MC-07 式 external_process observer；写入本文档防遗漏 |
| **`core/plugin-context.ts` 插件 `network.fetch` 能力**（Phase 11 重扫登记） | 插件运行时网络出口不经 observedProviderFetch 包装；当前 bundled 插件（beautify/media/office/jimeng-cli）无模型调用使用，属潜在第三方架构开口而非现存旁路 | LATENT/ARCHITECTURAL。若未来允许第三方插件直连模型 API，必须先定义观测边界（接入 observer 或显式声明为受信域外）；已同步登记于 Release Acceptance V3 §C/§J

### 1.4 修正声明

```text
Phase 3.5 结论：10 Host-managed production-reachable paths
Phase 10 重扫：仍为 10；新增登记 1 个 LATENT（local-cli-wrapper，非本轮引入，
Phase 11 重扫：仍为 10；新登记 1 个插件网络能力开口（plugin-context network.fetch，LATENT/ARCHITECTURAL）
基线 d5275e56 之前已存在）
```

---

## 2. Scenario Matrix

每个 Scenario 走真实 ingress（§一百三十六），由 `tests/helpers/
model-observability-scenario-harness.ts` 编排；Truth Receipt 结构（§十）在测试内
构建、不持久化到用户目录。

| # | Scenario | 真实 Ingress | Provider Witness 期望 | 核心断言 |
| --- | --- | --- | --- | --- |
| S1 | MC-01 simple chat | session.prompt（coordinator 或最小 createAgentSession 链） | 1 POST（SSE） | 1 trace / 1 call / origin=user_turn / parent=null / 四层 payload / usage / store rows / query / UI row / export |
| S2 | MC-01 multi-turn tool loop | 同上（工具回流后第二轮） | ≥2 POST | C2.parent=C1（callId 非 toolCallId）；conversation_history/tool_result/current_user_input 分类 |
| S3 | parallel tools | C1 双 toolCall，两工具内各一次辅助模型调用 | 3 POST | C2.parent=C1 且 C3.parent=C1（完成顺序无关） |
| S4 | subagent | spawn_subagent → child session.prompt | 2 POST | same traceId、C2.parent=C1、childAgentId/childSessionId 落 store/query/trace tree |
| S5 | MC-02 compaction run / recovery | compaction runner（含 recovery/repair turn） | N POST | turn 链 parent 链；task_instruction/format_constraint/tool_result provenance |
| S6 | MC-03 native compaction | pi-compatible compaction 分支 | 1 POST | partial/opaque provenance；provider wire unavailable 不显示空 Provider Request |
| S7 | MC-04 四协议 | callText × {anthropic, openai-chat, openai-responses, codex} | 4 POST | Witness body ≡ provider_request capture ≡ provider mapping（三方一致） |
| S8 | utility matrix | title/summary/memory/dream/approval/vision/diary-final/appearance/health 代表 | 每 caller ≥1 POST | 每 Prompt Construction 机制 ≥1 场景（§四十四） |
| S9 | approval format repair | approval gateway 二调 | 2 POST | repair 是独立真实 call/attempt；format_constraint 只在 repair 输入 |
| S10 | vision multimodal | vision-bridge | 1 POST | task_instruction/media_reference 真实；图片 bytes 不泄漏为文本 |
| S11 | MC-05 probe | probeProvider anthropic 分支 | 1 POST /v1/messages | POST 进 Observatory；GET /models 0 record |
| S12 | MC-06 normal | submitImage（真 runner） | 1 POST | 四层 + usage_missing 语义 |
| S13 | MC-06 codex 401 refresh | codex adapter 401→refresh→重发 | 2 POST | 1 trace/1 callId/2 attemptId/2 provider_request（ordinal 1,2）/2 provider_response/1+1 semantic；UI 1 Call |
| S14 | credential refresh 非 Model Call | codex refresh 链 | refresh 请求不计 POST 生成 | 0 额外 call |
| S15 | MC-07 CLI | dreamina fake executable | 0 HTTP（进程） | OPAQUE/external_process；argv/stdout 毒丸不进 payload |
| S16 | MC-08 video submit+poll | submitVideo + poll | 1 POST submit | submit 是 call；poll 0 record |
| S17 | MC-09 speech ×2 协议 | speech service（openai-like + volcengine） | 2 POST | audio externalize、language_hint、body.user.uid 脱敏、transcription 语义响应 |
| S18 | MC-10 diary full | /diary 任务（N 临时摘要 + 终稿） | N+1 POST | same trace；parent 语义按实际因果（不伪造树） |
| S19 | failure matrix | callText/Pi × {400,401,429,500,timeout,reset,abort,invalid JSON,malformed SSE} | 对应 status | 错误 Call 状态；safe error；abort≠error |
| S20 | concurrency A/B | 两 session 并行 | 2 POST | trace/parent/payload/usage 不串 |
| S21 | same-trace parallel media | C1 内并行 vision+image+approval | 4 POST | 三 child parent=C1，callId/attempt 独立 |
| S22 | ALS leak | setTimeout/setImmediate/detached/fire-and-forget 后台入口 | — | 不继承前一个用户 trace；detached force-new root |
| S23 | recording modes | OFF / metadata / payload / payload+blob 同 scenario | body 逐字节相同 | 业务返回一致；witness body 一致（§一百零二） |
| S24 | crash/restart | logical_call_start 后模拟 crash | 1 POST | terminal_status NULL + interrupted_by_restart；UI Incomplete ≠ Error |
| S25 | retention | old/new trace+payload→maintenance | — | payload 可先过期；UI expired≠unknown≠not_captured |
| S26 | blob GC | payload 过期→ref 消失→GC | — | 仅 0-ref blob 删除；共享 ref 保留 |
| S27 | queue overflow | tiny queue + 高并发 | N POST | 模型业务正常；drop counter 增加；UI 不假装完整 |
| S28 | disk write failure | readonly dir / closed DB | N POST | 业务不受影响；health degraded/disabled |
| S29 | query truth | 固定 Scenario Dataset + 独立 expected table | — | 14 filter 维度逐项；groupBy 全指标；cursor 遍历 exact set |
| S30 | DST | America/Los_Angeles 跨 DST 窗口 calls | — | 历史 date bucket 正确（failing test 先行；修复后非 DST 反向回归） |
| S31 | security matrix | route harness：local owner/remote owner/anonymous | — | 正文/blob/export/settings PUT 权限；verb fail-closed |
| S32 | poison matrix | 各协议携毒凭证 | witness 可见 secret | Observer/DB/WAL/SHM/Query/HTTP/Export/DOM 零命中 |
| S33 | redaction non-interference | 同 scenario capture ON/OFF | body 逐字节相同 | Redaction 只改 capture copy |
| S34 | UI vertical slices | 真实 temp SQLite→Query→Hono route→renderer client→React | — | Call row/Inspector/Trace/OPAQUE/UNAVAILABLE/corrupt |
| S35 | export truth | 10k/50k + filter + includePayloads | — | filter exact；identity 同 query；opaque 保留；无 raw；cancel 无泄漏 |
| S36 | blob perf/security | 1/16/64MB blob HEAD/GET | — | HEAD 只 stat；GET 不造成不可接受 stall（若证阻塞→streaming 修复+反向回归） |
| S37 | perf boundaries | 100/1000 calls × 4 recording modes | — | 无数量级回归 |
| S38 | UI large payloads | 1MB payload / 100~1000 call trace / 高基数 group | — | 不冻结/不栈溢出（实测需要才改） |
| S39 | observatory zero-call | 打开/查询/inspect/export/settings/blob 全操作 | 0 POST | 无递归污染 |
| S40 | multi-writer topology | server 进程唯一 writer 事实 | — | 架构不变量锁定 + busy 第二 writer 模拟 |

### 2.1 Utility Representative Caller Matrix（S8 细目）

不同 Prompt Construction 机制（§四十四）至少各一场景：

| 机制 | 代表 caller | 已迁移 provenance |
| --- | --- | --- |
| 显式 caller 段级 provenance（memory 域模板） | memory compile / rolling summary | exact |
| 普通 system+messages（title/summary） | title 生成 | structural fallback |
| 独立 format repair 二调 | approval repair | format_constraint exact |
| multimodal content | vision / appearance | media_reference exact |
| 常量指令 | health check | task_instruction exact |
| diary 终稿多段拼接 | diary final | exact |

---

## 3. Truth Oracle

### 3.1 Fake Provider Witness（独立事实源）

`tests/helpers/model-observability-scenario-harness.ts` 内建：

- 本地 `node:http` server，绑定 `127.0.0.1:0`（随机端口）。
- 记录每个请求：method、path、headers（含凭证毒丸）、raw body bytes、
  parsed JSON、request ordinal、response bytes、时延。
- 按 scenario 脚本返回确定性 fixture：
  - OpenAI Chat Completions SSE（`data: {...chunk}` + `[DONE]`）
  - OpenAI Responses / Codex Responses SSE（`response.output_text.delta` 等）
  - Anthropic Messages SSE（`content_block_delta` 等）
  - JSON 非流式（媒体/speech/probe）
  - 401（带 WWW-Authenticate）→ 刷新后 200（codex 双 attempt 硬场景）
  - 429/500/invalid JSON/malformed SSE/慢响应（timeout）/connection reset
    （`res.destroy()`）/ never-respond（abort）。
- **独立性纪律**：Witness 只用 node:http 原语；绝不 import observedProviderFetch /
  payload capture / observer 任何实现；expected 值只来自 scenario 构造事实与
  Witness 日志（§六/§七/§二十六）。
- 只存在测试代码，不接入生产 Observer（§六）。

### 3.2 Layer Chain（每 Scenario 的 Truth Receipt 字段）

```text
scenarioId / runtimeIngress / expectedCalls / providerWitness / observerEvents /
durableRows / queryResult / httpResult / uiResult / exportResult / violations[]
```

Receipt 在测试进程内存/temp dir；报告只落 `secretLeakDetected=false` 类结论，
毒丸值绝不复制进 Markdown（§十一）。

---

## 4. Cross-Layer Invariants（每个 Scenario 的机器校验）

1. **Call Identity**：callId Observer→Store→Query→API→UI→Export 完全一致；
   UI 绝不重新生成 id。
2. **Trace Identity**：traceId 从 ingress 到全部因果子调用符合 Phase 4 语义；
   同任务共享、独立任务不串。
3. **Parent**：parentCallId 只表示直接 causative upstream Model Call；绝不使用
   上一条调用/同 session 最近/同 trace 最近/toolCallId/requestId。
4. **Attempt Identity**：attemptId 是 transport/logical attempt 身份，绝不当
   Logical Call ID。
5. **Provider Request Ordinal**：同一 Logical Call 内 Provider Request 顺序；
   绝不等价 attempt ordinal。
6. **Payload Kind**：semantic_request / provider_request / provider_response /
   semantic_response 四层；任何 projection 不得折叠成 request/response。
7. **Visibility**：FULL/PARTIAL/METADATA_ONLY/OPAQUE/UNAVAILABLE 全链一致，
   下游绝不升级（OPAQUE 在 Store/Query/API/UI/Export 全部保持）。
8. **Fidelity**：runtime_exact/parsed_equivalent/stream_aggregate/normalized/
   metadata_only/external_process/opaque 保持；normalized 不得自称 raw。
9. **Sanitization**：sanitized payload 除 redaction/externalization/truncation
   显式改变的字段外保持真实结构；不得为安全把请求删成 `{}` 仍标 FULL。
10. **Semantic Request Truth**：stored semantic_request 与业务 boundary 对象
    脱敏后一致（独立捕获 boundary object 对比，§二十一）。
11. **Provider Request Truth**：Witness body ≡ captured provider_request
    （允许差异仅限 redaction/externalization/normalization fidelity 声明）。
12. **Provider Response Truth**：FULL/parsed 对比 fixture 实际返回；STREAM_
    AGGREGATE 验证 aggregate 正确且不宣称保存完整 SSE 序列。
13. **Semantic Response Truth**：UI 显示的 text/reasoning/toolCalls/
    structuredOutput/transcription/media/finishReason 来自实际 semantic_response，
    不从 Provider Response 猜。
14. **Provenance Truth**：每个 exact section 的 locator 在 sanitized
    semantic_request 上 resolve 出 expected segment；expected 来自 scenario
    构造事实，禁止文本搜索当 Oracle（§二十六）；provider mapping 的 semantic
    ordinal→provider locator 实际落到 Witness 收到的对应字段；redaction 改变
    长度后 remap 正确或诚实降级 structural，绝无 silent wrong span。
15. **Accounting Truth**：fixture usage → Ledger → model_call_usage → aggregate
    → Call UI 一致（input/output/reasoning/cache read/write/total/cost）。
16. **Observatory Zero-Call**：S39 全部 Observatory 操作 0 新 Model Call
    （witness 请求计数 + observer 事件双验证）。

---

## 5. Security Invariants

| # | 不变量 | 验证层 |
| --- | --- | --- |
| SEC-1 | Redaction 不改变真实 Provider Request（ON/OFF witness body 逐字节相同） | S23/S33 |
| SEC-2 | Observability ON/OFF 业务返回值一致 | S23 |
| SEC-3 | Credential 毒丸（Bearer/x-api-key/Cookie/Set-Cookie/OAuth/private key/signed URL query/Volcengine user.uid/provider token fields）不进 Observer 事件 JSON / payload sink / SQLite / WAL / SHM / Query JSON / HTTP JSON / Export JSONL / renderer DOM | S32 |
| SEC-4 | Fake Provider Witness **应该**看到测试 secret（否则无法证明 redaction 只改 copy）——不扫描 witness | §一百 |
| SEC-5 | Payload 正文 / blob / export / settings PUT = LOCAL_ONLY；remote STUDIO_OWNER 403；anonymous 401/403 | S31 |
| SEC-6 | Route verb hardening：未登记 verb（DELETE settings/PUT payload/POST blob/GET export 变体）fail closed | S31 |
| SEC-7 | Blob path traversal（`../`、`%2e%2e`、slash、backslash、超长 id）不突破 exact blobId | S31 |
| SEC-8 | SSRF：payload descriptor URL/blob preview/provenance 不让 server fetch 用户 URL（localhost trap server 验证零请求） | S31 |
| SEC-9 | 本地文件读取：blob preview 只读 registered `mb_*`（DB 行 + 重算 contained path） | S31 |
| SEC-10 | XSS：provider response/prompt 中 `<script>`/`<img onerror>`/svg/`javascript:` 只以纯文本渲染，无 DOM execution | S34 |
| SEC-11 | Error API 不泄正文：500+secret body → metadata endpoint 错误响应无 secret | S19/S31 |
| SEC-12 | 测试过程 console/server log 不打印 raw prompt/credential/provider body | 全部 |

---

## 6. Missing-State Semantics（禁止合并表）

| State | Runtime 语义 | 禁止的合并 | 验证 |
| --- | --- | --- | --- |
| unknown | 代码无法知道（如 observability_meta 读取失败） | 变 0/变正常 | S24/S27 专项审计 catch-return |
| not_captured | capture 层未捕获或 persistPayloads=false | 与 expired/dropped 混淆 | S23 metadata-only |
| not_correlated | usage 无 modelCallId 关联（MC-03） | 猜 correlation | S6 |
| unavailable | 结构性不可见（MC-02/03/10 wire、google resp） | 变 `{}`/变 empty | S5/S6/S18 |
| opaque | 外部进程边界（MC-07） | 升级 FULL | S15 |
| expired | retention 删除 | 变 unknown/not_captured | S25 |
| dropped | queue overflow | 隐藏/假装完整 | S27 |
| corrupt | JSON.parse 失败 | crash/500 | S34 |
| incomplete | terminal_status NULL（crash 未终态） | 变 Error | S24 |
| interrupted_by_restart | persistence inference | 变 Error | S24 |
| empty vs unavailable | 真实 `{}` FULL empty object | 与 UNAVAILABLE 互变 | S29 专项 |
| cost null | cost 未产生 | 变 0 | S29/S34 全链 |
| usage_missing | call ok 但无 usage | call 变 error | S12 |
| `{}` empty object / `[]` | 真实空集合 | 变 unavailable | S29 专项反向 |

专项（§三十二）：审计所有 `catch { return 0 }` / `return []` / `return null`
是否改变事实语义——unknown 不能被证明成 0。

---

## 7. Concurrency Matrix

| 场景 | 验证 | Scenario |
| --- | --- | --- |
| 两独立 Chat Session 并行 | traceId/parentCallId/payload/usage 不串 | S20 |
| 同 trace 并行媒体（vision+image+approval） | 三 child parent=C1、callId/attempt 独立 | S21 |
| 并行工具（双 toolCall 内模型调用） | 双双 parent=C1，不互为 parent，完成顺序无关 | S3 |
| ALS 泄漏（setTimeout/setImmediate/detached Promise/EventBus/cron/background memory/media poller/export/retention maintenance） | 不继承前用户 trace；detached force-new root | S22 |
| 多进程 writer 拓扑 | 生产拓扑 = server 单 writer（Desktop spawn server、CLI 无 engine）；写进 Architecture Invariant + runtime guard；模拟第二 writer → SQLite busy 不导致 Model Call 失败（observability 允许 degraded） | S40 |

---

## 8. Crash/Restart Matrix

| 场景 | 期望 | Scenario |
| --- | --- | --- |
| logical_call_start+attempt_start+provider_request 已 flush、无 logical_call_end，重启 | terminal_status=NULL、interrupted_by_restart=1；Query=incomplete；UI=Incomplete/Restart interruption（绝不 Error） | S24 |
| Provider Request 已 durable、Semantic Response 未落 | 重启后 payload pipeline 只显示真实存在部分；不补空 Semantic Response card | S24 |
| crash 丢最后未 flush batch | 诚实丢失（drop 计数/meta）；不伪造 | S24 |
| graceful shutdown | engine dispose 5s bounded flush | 既有 Phase 7 测试回归 |

---

## 9. Retention Matrix

| 场景 | 期望 | Scenario |
| --- | --- | --- |
| old trace/payload + new trace/payload → maintenance | payload 先过期、trace 保留；call 标 expired（≠unknown/not_captured） | S25 |
| payload 过期→blob ref 消失→GC | 仅 0-live-ref blob 删除；共享 ref 保留 | S26 |
| disabled recording | Provider call 正常执行、无新 durable record、历史 DB 仍可查 | S23 |
| payload enable 不回填历史 | Call A（metadata-only）无正文、Call B 有；禁止从 session history 重建 | S23 |
| blob enable 不回填 | 只有未来 eligible binary 进 blob store | S23 |
| recording disabled 时打开 Observatory | 历史可见；且不创建 observability.sqlite | S39 |

---

## 10. Performance Boundaries

宽松 stress（不制定虚假 SLA；关注数量级回归/stall/OOM）：

| 项 | 方法 | 阈值语义 |
| --- | --- | --- |
| Latency overhead | 100/1000 deterministic local calls × {off, metadata, payload} | 无数量级劣化，需调查 |
| Blob GET | 1/16/64MB；观察 event-loop stall/峰值内存；HEAD 只 stat 不读 bytes | 64MB readFileSync 明显阻塞 → P1 → streaming 修复（保持 exact blobId/LOCAL_ONLY/no-store/nosniff/safe MIME/no path exposure；修复后 HEAD/GET/traversal/missing 反向回归） |
| Export | 真实 10k/50k metadata records；server/renderer/IPC chunk 有界 | memory bounded |
| Export cancel | generator stop / reader cancel / partial 删除 / exportId 清理 / webContents destroy | 无 fd/Map/partial leak |
| UI 1MB payload | Inspector 打开 | 不长期冻结 |
| UI 大 trace | 100/1000 calls | 无递归栈溢出/无限循环 |
| 高基数 group | session/task 维度 | 不同时渲染几万 bucket（backend 无 limit 则先测，真实可 freeze 才做最小保护） |
| Facet cache | filter 变化/新 provider model/refresh | 不长期显示过期 facet |

---

## 11. UI Truth Matrix（vertical slices；数据必须来自真实 Query Service）

| Slice | 链路 | 断言 |
| --- | --- | --- |
| Filter→API | UI 选 Provider A+memory+error+Last7d | 实际 HTTP body ≡ expected normalized filter |
| Metrics | 100 calls（page 50） | UI 显示 aggregate=100，非 page 重算 |
| Call Row | SQLite row vs Query DTO vs HTTP JSON vs DOM | Model/Status/Tokens/Attempts/PayloadAvailability 全一致 |
| Inspector | 真实 temp SQLite→Query→Hono route→renderer client→组件（Electron 连接最小 test adapter） | 与 queryCallDetail 一致；非 mock props |
| Trace Tree | C1├C2└C3 真实 store | DOM 体现 parent/child；orphan→Missing parent；cycle 截断+degraded |
| OPAQUE payload | 真实 opaque row 全链 | 显示 Opaque，不渲染 `{}` |
| UNAVAILABLE payload | 真实 unavailable row 全链 | 诚实 unavailable |
| Corrupt payload | 手工损坏 payload_json（测试专用直写 DB） | contentState=corrupt、显示不可读、不 crash |
| XSS | 毒丸内容全链 | 纯文本，无执行 |
| Locale | zh/en/ja 切换 | filter 仍发 wire enum/raw ID；export schema 稳定 wire values |
| utcOffsetMinutes 符号 | 东八区 +480、美西夏令时 -420 | 不反 |
| Unknown 状态 | 需要显示未知处 | 不填 0/None/Empty；允许 Unknown/Degraded 状态 |

---

## 12. Export Truth Matrix

| 项 | 断言 |
| --- | --- |
| Filter exact | Provider=A+memory → 零 Provider B/非 memory |
| Identity | bundle callId/traceId/attempts/usage/payload metadata ≡ Query Service |
| includePayloads=false | 正文零出现（默认 metadata-only） |
| includePayloads=true | 只有 sanitized payload；永远无 raw secret |
| OPAQUE | visibility=opaque 原样保留，不变 `payload={}` |
| Blob | 只 descriptor/blobId，绝无 bytes |
| Cancel/destroy | 见 §10 |
| 413 | 超 maxCalls 上限显式错误，不静默截断 |

---

## 13. Known Gaps（A/B/C 三类，禁止把 bug 写成 future）

**A（无法观察，但诚实表达）**：
1. MC-07 CLI 内部 wire（external_process/OPAQUE）。
2. MC-02/03/10 provider wire（pi 0.84.1 summarizer options 无 onPayload）。
3. Pi google/mistral-conversations provider_response（adapter 不调 onResponse）。
4. Pi provider request 的 headers/endpoint（hook 不暴露）。
5. MC-03 usage correlation（除非 SDK 出现可靠 hook；保持 not_correlated，
   见 §15 专项）。
6. Pi transport retry attempt（logical_boundary，需 pi-ai 上游 onAttempt）。

**B（当前可修，但未修）**：_首轮为空；Phase 10 发现的 P0/P1 不允许留在这里_。

**C（产品未来功能，非 bug）**：FTS/reasoning search/global search/saved
filters/dashboard builder/alert rules/cloud sync/plugins——持续禁止。

---

## 14. Severity Classification & Repair Protocol

| 级 | 定义（任务书 §一百二十八） |
| --- | --- |
| P0 | 安全泄漏；错误身份/因果关系；raw secret 持久化；远程越权读正文；Observability 改变真实模型请求；一次 Call 被重复/错绑 |
| P1 | 统计/日期/usage 明显错误；缺失事实显示成真实 0；crash/retention 数据语义错误；生产明显卡顿；错误 payload visibility；生产 model egress 漏 Observer |
| P2 | 局部性能；UI misleading wording；低频 filter/query edge |
| P3 | 视觉/文案/polish |

**修复范围**：P0/P1 必修；局部低风险 P2 可修；P3 记录。

**修复协议（§一百三十一/§一百三十二）**：
1. 先写最小 failing test 证明问题存在；
2. 再改生产代码（禁止 `if (process.env.TEST)` 污染；测试 seam 只用依赖注入/
   fake provider/temp HOME/test observer）；
3. 报告记录 before/root cause/fix/after；
4. 每个修复做反向回归（修 DST → 非 DST 不破坏；修 blob streaming → HEAD/GET/
   LOCAL_ONLY/MIME/missing/traversal 仍正确）。

---

## 15. 专项：MC-03 Usage Correlation 重新追踪

Phase 10 重新追踪 `completeSummarization` → usage 是否在 runtime 某处可见 →
是否存在准确 callId association point。**只有存在确切因果关联才允许修复**；
禁止按时间/模型/session/调用顺序猜。若 SDK（pi 0.84.1）无可靠 hook：
保持 `not_correlated`，在 Release Acceptance 明确 **KNOWN CAPABILITY GAP**。

## 16. 专项：DST / Date Bucket

当前 date group = 显式 `{bucket:"day", utcOffsetMinutes}`（SQL
`strftime('%Y-%m-%d', started_at, printf('%+d minutes', ?))`）。固定 offset
对**历史**跨 DST 窗口可能分错日期。协议：先写 failing test（America/
Los_Angeles 跨 DST start/end、接近当地午夜 calls）；证明错误才做最小扩展
（优先 IANA timeZone），修复后非 DST timezone 反向回归。不凭想象改。

## 17. Schema 纪律

Phase 10 不随意升级 SQLite schema。只有 P0/P1 修复必须改变持久化模型才允许 v3
（单事务 migration + 保真 + read compatibility + rollback test + fingerprint +
store registry 全套）；runtime/query/UI 可解决就不动 schema。

## 18. Gate 清单（完成条件）

typecheck ×3 / eslint（0 新增 error）/ lint:boundary / persistence scanner +
fingerprint guard + closure / i18n parity + locale coverage / targeted
observability tests / full npm test / build:server / build:server:open /
build:client / package smoke（环境允许时）。Cross-platform 未执行的平台
如实写 NOT EXECUTED。全部通过后按既有机制推进 VERIFIED_SOURCE_SHA（功能
commit → 完整验证 → audit/seal commit）。
