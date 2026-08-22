# Model Call Observer — 实现报告（Phase 1 契约 + Phase 2 文本运行时 + Phase 2.5 安全收口 + Phase 3 全路径 + Phase 3.5 残余闭合 + Phase 4 Trace）

基线：`feature/model-call-observability`。Pi 三件套 0.84.1。
本文档以当前最终代码为准；Phase 1/2 = 第一轮，Phase 2.5/3 = 第二轮，
Phase 3.5 + Phase 4 = 本轮（trace 传播 + MC-10）。

## Residual Boundary Closure（Phase 3.5，本轮新增）

修正旧「9 paths」结论：重新全仓出口反扫发现 diary temporary summary 是
**生产可达的第 10 条独立架构路径**（MC-10）：

```text
POST /api/diary/write → engine.writeDiary → collectDiaryMaterialResult
→ generateTemporarySummary × N → generateDiaryCompactionSummary
→ Pi generateSummary()（未传 streamFn）→ completeSimple() → Provider
```

- 原状态：不经 streamFunction/callText/MC-05～09、不进 ledger、不可观测。
- 接入：`lib/pi-sdk/index.ts` 的 `generateSummary` re-export 包装为
  Observed direct summary（`lib/llm/observed-pi-direct-summary.ts`，
  复用 ModelCallRecorder/Observer/Identity，**无** DiaryObserver/SummaryObserver）。
  diary 传第 14 参 observerContext（usageContext + usageLedger）获得归属与账本。
- attemptVisibility=logical_boundary（transport retry 不可见，不伪造 exact）；
  无 provider_request_prepared/provider_response_received（summarizer options
  无 onPayload、不在 session 扩展链——事件缺失即真相，同 MC-03）。
- ledger：每临时摘要 1 条 entry，metadata.{modelCallId,traceId,parentCallId}。
- `session-snapshot-side-task-runner` completeSimple 回落：唯一上层
  memory-reflection-runner 仍无生产 caller → **LATENT / NOT_CURRENTLY_REACHABLE**
  （不制造生产行为；facade completeSimple 保持裸 re-export）。
- 全部事实记录在 `MODEL_CALL_CLOSURE_DELTA.md`（Audit Addendum，原始审计
  报告保持 bf3c80b5 历史快照不改写）。

## Trace Contract（Phase 4，本轮新增）

### 第一性原理定义

- `traceId` = 一次具有共同因果根源的完整任务执行。不是 sessionId/
  conversationId/taskId/callId；一个 session 可以有很多 trace，一个 trace
  可以跨多个 session（subagent）。禁止 `traceId = sessionId`。
- `parentCallId` = 直接造成当前 Model Call 的上游 Model Call。不是"最近的"、
  不按时间/session/数组顺序猜。无事实 → null（允许 null；不造假 parent）。
- Trace root 不一定是 Model Call（用户输入不是调用）——trace 可以只有
  traceId 而无 rootCallId，不造 UserRequestModelCall。
- 数据依赖 ≠ 触发因果（§四十八）：diary 素材 A 拼进 prompt B 不构成
  B.parent=A；本轮只表达直接执行因果。

### ModelTraceScope（`lib/llm/model-trace-scope.ts`）

与 ModelCallScope 分层：TraceScope 生命周期=整个任务；ModelCallScope=
单次 Provider 调用。实现=AsyncLocalStorage（并发隔离/异步传播/嵌套 child
task），禁止 global currentTraceId / module-level mutable parent。

```ts
{
  traceId, origin（有限枚举）, causalParentCallId（建立时快照）,
  refs（≤8 键 string 安全业务引用）,
  lastCallId（mutable：仅 agent-loop 流式调用推进）
}
```

API：`runWithModelTrace` / `runWithNewModelTrace`（force-new，detach 语义）/
`runWithoutModelTrace` / `runWithModelTraceRoot`（inherit-or-mint，顶层任务
入口用）/ `runToolExecutionWithModelTrace`（工具子 scope）/ `noteAgentStreamCallStarted`
/ `resolveModelTraceContext`（唯一身份解析入口）。

### 身份解析优先级（所有接点统一，§四十一/四十二/四十三）

```text
explicit caller context → current ModelTraceScope → (session 注册 trace) → singleton trace
```

singleton 兜底保证**所有生产 Model Call traceId != null**（独立
Health Check/Probe/后台任务形成单 call trace）；自动生成 traceId 安全、
自动猜 parentCallId 不安全（无事实 → null）。

### Trace Origin（closed enum，§六十八/六十九）

user_turn / bridge_message / phone_message / slash_command / automation /
background / plugin / media / speech / provider_probe / health_check /
diary / unknown。prompt 类别（memory/approval/vision/summary）继续由
subsystem/operation 表达，不进 origin。事件侧只落 `logical_call_start.details.traceOrigin`。

## Trace Root Semantics — Root Ingress Matrix

| Task ingress | New Trace | Inherit Trace | Parent source | Detach behavior |
| ------------ | --------- | ------------- | ------------- | --------------- |
| Desktop user turn（coordinator.prompt） | runWithModelTraceRoot origin=user_turn | 外层 scope 原样继承 | scope.lastCallId 链 | turn 内派生任务继承；cron/timer 类强制新根 |
| Bridge inbound（executeExternalMessage） | origin=bridge_message | 同上 | 同上 | 同上 |
| Phone inbound（runAgentPhoneSession） | origin=phone_message | 同上 | 同上 | 同上 |
| Slash command（dispatcher.tryDispatch） | origin=slash_command | bridge turn 内继承 | 同上 | 同上 |
| Automation/Cron（scheduler._executeCronJob） | **force-new** origin=automation | 不继承（每 run 新 trace） | null | 天然 detach |
| Diary（engine.writeDiary） | **force-new** origin=diary | 不继承 | null | 天然 detach |
| Memory daily/compile（memory-ticker） | **force-new** origin=background | 不继承（checkpoint 可能在 turn 链内触发，必须切断） | null | 天然 detach |
| Dream（startAutomaticIfEligible） | **force-new** origin=background | 不继承 | null | 天然 detach |
| Speech（transcribeAudio/transcribeVoiceAttachment） | **force-new** origin=speech | 不继承（REST/队列独立任务） | null | 天然 detach |
| Provider Probe（probeProvider anthropic 分支） | **force-new** origin=provider_probe | 不继承 | null | 天然 detach |
| Health Check（/models/health） | **force-new** origin=health_check | 不继承 | null | 天然 detach |
| Plugin（model:sample-text / utility:call-text） | runWithModelTraceRoot origin=plugin | chat 工具内触发 → 继承工具子 scope | 同上 | 后台触发铸新根 |
| Media（submitImage/submitVideo） | runWithModelTraceRoot origin=media | 工具内生成继承 Chat trace | 工具子 scope 快照 | 独立提交铸新根 |
| Subagent（executeIsolated → session.prompt） | facade trace ingress 兜底 inherit-or-mint | 工具 spawn → 继承（§三十五不 mint）；调度器已包 automation trace | 工具子 scope 快照 | — |
| Compaction（MC-02 runner / MC-03 native） | 无独立 ingress：mid-turn 继承 turn trace；slash /compact 走 slash ingress | 继承 | scope.lastCallId | — |

## ParentCall Semantics — Causal Edge Matrix（代码/测试已证明的边）

| Parent model call | transition | child model call | 证明 |
| ----------------- | ---------- | ---------------- | ---- |
| Chat C1（agent loop 流式） | 同一 runAgentLoop 工具结果回流后继续推理 | Chat C2（C2.parent=C1） | tests/model-call-trace-propagation.test.ts 测试2 |
| Chat C1 | parallel toolCall tc_a → 工具执行边界子 scope | Vision/工具内任意辅助 call（parent=C1） | 测试3（并行动作双双 parent=C1，绝不互为 parent） |
| Chat C1 | spawn_subagent 工具 → child session.prompt | Child Chat C2（parent=C1，跨 session 同 trace） | 测试4 |
| Chat C1 | media 工具 → bus → submitImage/submitVideo | Media submit call（parent=C1） | 测试5 |
| AgentRun turn N | recovery/repair 下一 turn | turn N+1（parent=N，经 scope.lastCallId 推进） | runner 统一解析 + 测试2 同机制 |
| diary task root | 直接触发的临时摘要×N + 终稿 | 全部 parent=null、same trace | 测试8 / diary 测试 |

无法证明直接因果的位置（数据依赖 / 后台扫描派生）保持 `parentCallId=null`
（TRACE_ONLY）：diary 临时摘要之间、turn 后 title/summary（若 fire-and-forget
脱离链则 singleton）、memory 后台各调用间。不按时间/session 猜。

工具边界实现：`session-options.ts wrapToolDefinitionExecutionOnce`（全部
base+custom 工具的唯一收口，execute 首参即 toolCallId）→ 进入时快照
`causalParentCallId = scope.lastCallId`。toolCallId（Tool Invocation
Identity）只进 scope refs 保留，不冒充 parentCallId（§三十三/三十四）。
并行安全：子 scope 冻结快照 + 每 ALS 链独立对象，无全局 lastCall。

## Concurrency / Detach Contract

- 并发 Session A/B：各自 root scope，trace 内不出现对方 callId（测试9）。
- 并行工具：双双 parent=C1（测试3）。
- detached background：T1 内创建的 delayed 任务执行时强制新 trace，不泄漏
  T1（测试10；memory-ticker/dream/scheduler/diary/speech/probe/health 均
  force-new 入口）。
- ALS 泄漏边界（§四十九）：进程内 bus 是 promise 链（工具→媒体提交可传播）；
  已知 fire-and-forget（queueVoiceTranscription）入口已 force-new。未接线的
  未知延迟回调最坏回落 singleton trace（诚实缺失，不串线到错误 trace 需要
  显式 force-new 入口遗漏才会发生——见 Known Trace Gaps）。

## MC-01～MC-10 Trace Coverage Matrix

| Path | Observer | traceId | Root creation | Trace inheritance | parentCallId | Concurrency safe | Ledger trace metadata |
| ---- | -------- | ------- | ------------- | ----------------- | ------------ | ---------------- | --------------------- |
| MC-01 Pi AgentSession | FULL | 恒非空 | prompt ingress（desktop/bridge/phone/subagent 兜底） | turn 内全调用+工具派生 | C2.parent=C1（loop 链）；工具内辅助 parent=C1 | ALS 隔离 | **FULL（本轮补）**：message_end 补账 metadata.{modelCallId,traceId,parentCallId}（WeakMap 关联） |
| MC-02 cache-preserving AgentRun | FULL | 恒非空 | mid-turn 继承 / slash、独立 compact 新根 | runner 统一解析 | recovery turn 链 parent 链 | 同上 | FULL（metadata 加 traceId/parentCallId） |
| MC-03 native compaction | FULL | 恒非空 | mid-turn 继承 | 同上 | scope.lastCallId | 同上 | NONE（compaction entry 不带——Remaining Gap） |
| MC-04 callText | FULL | 恒非空 | caller scope / singleton | 工具内辅助继承 | 工具内 parent=C1；独立 null | 同上 | FULL |
| MC-05 Anthropic probe | FULL | 恒非空 | force-new origin=provider_probe | 不继承 | null | n/a | FULL |
| MC-06 图片 HTTP | FULL | 恒非空 | 工具内继承 / 独立 origin=media | 同上 | 工具内 parent=C1 | 同上 | FULL |
| MC-07 Dreamina CLI | FULL | 恒非空 | 同 MC-06 | 同上 | 同上 | 同上 | FULL |
| MC-08 视频 HTTP | FULL | 恒非空 | 同 MC-06 | 同上 | 同上 | 同上 | FULL |
| MC-09 Speech | FULL | 恒非空 | force-new origin=speech | 不继承 | null | n/a | FULL |
| **MC-10 Pi direct summary（diary 临时摘要，本轮新增）** | FULL | 恒非空 | diary force-new origin=diary | diary 任务内全摘要+终稿同 trace | 全部 null（任务根直接触发） | 同上 | FULL |

## Usage Ledger（§六十二/六十三）

Observer 仍是 Trace Truth Source；Ledger 只是 Accounting Projection（不反向
推导 trace）。低风险增补（无 schema version 变化，metadata 容器自由字段）：

- MC-04：`metadata.{modelCallId,traceId,parentCallId}`（llm-client start）。
- MC-05～09：四处写入点 spread `observedModelCallLedgerMetadata(recorder)`。
- MC-02：runner ledger.start metadata 加 traceId/parentCallId。
- MC-10：observed direct summary 内 ledger start/finish/recordError。
- MC-01（原 correlation=NONE，本轮补齐）：assembled message ↔ 身份经
  `lib/llm/model-call-correlation.ts` WeakMap 关联（对象 GC 自动回收，无
  生命周期负担），三处 message_end 补账（session-coordinator /
  bridge-session-manager / agent-executor）读取注入 metadata。**无侵入**：
  不改 session message schema、不污染 assistant content、不给 Provider 发
  标记（§六十五）；对象被复制/未观测调用 → null，不猜（fail-open）。

## 测试（本轮新增 3 文件 35 用例 + 更新 1 文件）

- `tests/model-trace-scope.test.ts`（17）：解析优先级/singleton/并发隔离/
  detach/工具子 scope 快照与并行不串线/origin 枚举/refs 边界。
- `tests/model-call-trace-propagation.test.ts`（14）：任务书测试 1～11 场景
  （单 call、多 provider turn、并行工具、subagent 跨 session、media、speech、
  automation 两 run、diary 三调用同 trace、并发 session、detached
  background、毒丸 safety）+ MC-01 WeakMap 关联；全部场景收尾断言
  `assertTraceGraphValid()`（traceId 非空/parent≠self/parent 同 trace 或
  null/无环/生命周期身份稳定，§五十六～六十机器校验）。
- `tests/model-call-diary-observer.test.ts`（4）：**真实 Pi generateSummary
  链**（stub 全局 fetch 伪 SSE Provider）证明 MC-10 旁路闭合——生命周期、
  ledger 关联、错误终态、错误正文不泄漏、空消息短路。
- 更新 `tests/model-call-calltext-observer.test.ts`：锁定 Phase 4 新契约
  （traceId 恒非空 singleton + parent 不猜；metadata 三元组）。
- 第一/二轮全部回归（96 用例）通过。

## 门禁（按仓库既有规程处理）

- `export-manifest.json`：收录 model-trace-scope / model-call-correlation /
  observed-pi-direct-summary。
- `lint:boundary`：绿（新边全部因 manifest 收录消失；基线 debt 不变）。
- `compute-cli-closure`：复核重写（新增 3 源模块 + importers，无新 debt）。
- `persistence-schema-fingerprint`：compatible repin（观测接线 + additive
  metadata，无 store 形状/格式变更）。
- post-verification seal：完成后按既有机制推进 VERIFIED_SOURCE_SHA。

## Known Trace Gaps（本轮不做/诚实缺失）

- MC-03 compaction entry 不带 callId/traceId（Observer 可见、Ledger 侧
  accounting 留给 Accounting Projection 阶段）。
- Pi transport retry 仍 logical_boundary（需 pi-ai 上游 onAttempt）。
- Dreamina wire 仍 OPAQUE（结构性）。
- turn 后 fire-and-forget 派生（title/activity summary 若经 postMessage 事件
  总线等非 ALS 链触发）可能各自 singleton——真实任务边界模糊处保持诚实，
  不强行并 trace。
- 未经 ingress 接线的未来新入口（直接调 session.prompt/callText 的新代码）
  由 facade 兜底（session.prompt inherit-or-mint）与 singleton 兜底覆盖，
  origin=unknown。
- runAgentPhoneSession 的 runAgentSession（非 phone）仍无生产 caller——LATENT。

## Next Phase

Prompt Provenance（sections/source/version/ref）→ Request/Response capture +
Redaction Contract → Payload/Blob Store → Query Service → Export → UI。

---

# Phase 5 — Semantic Input Provenance（第四轮，2026-08-21）

> 审计入口：SEMANTIC_INPUT_PROVENANCE_AUDIT.md（Step 1 交付，含 Prompt
> Construction Matrix 与 caller 迁移清单）。本节记录契约语义与最终矩阵。

## Provenance Contract

统一 `ModelSemanticInputProvenance`（lib/llm/semantic-input-provenance.ts）：

```
{ schemaVersion: 1, inputShape, sections[] }
section = { category, role, precision, locator, source }
```

- **不包含任何输入内容**（§五/§三十七）：section 只有五个维度；sanitize gate
  fail closed（非法 category/locator/source 逐段丢弃，source.id 与 path 拒绝
  绝对路径/UNC/drive letter/URL）。
- **不新增内容 hash**（§二十四）：template identity 复用 prompt-layout 的
  cacheGroup/templateVersion（source.id/source.version），cachePrefixHash 行为
  不变。
- 生命周期 = model call lifecycle：recorder 持有（per-call sidecar，随 recorder
  GC），无全局 Map（§四十一/四十二）。完整 map 经事件 non-enumerable symbol
  （`MODEL_CALL_SEMANTIC_PROVENANCE`）随事件传递——JSON.stringify 与 Metadata
  Safety Gate 均不可见；Observer 事件只携带安全 summary（§三十九/七十六）：
  `inputShape / provenancePrecision / inputSectionCount / inputCategories(去重≤32)
  / opaqueSectionCount`。TestModelCallObserver 提供 `provenanceForCall` /
  `categoriesForCall`（§八十七，仅测试路径）。

## Category Taxonomy（24 值闭集，全部有真实使用点）

platform_instruction / persona / user_profile / memory_context /
skill_instruction / agents_file / session_instruction / agent_roster /
conversation_history / current_user_input / tool_definition / tool_result /
task_instruction / task_input / format_constraint / previous_summary /
compaction_summary / media_prompt / media_reference / audio_input /
language_hint / adapter_injected / sdk_internal / unknown。

Category 与 subsystem/operation/callPurpose 严格正交（§十九：callPurpose 表达
「为什么调」，category 表达「这段输入是什么来源」）。role（system/developer/
user/assistant/tool/input/parameter）是独立第二维度。

## Locator Contract

`{ root: systemPrompt|messages|tools|input|parameters, path?: (index|key)[], span? }`

- 文本根（systemPrompt）与文本内容（messages[i].content 字符串）用 UTF-16
  code unit 闭开区间 `[start, end)`，即 `text.slice(start, end) === sectionText`
  （§二十七；中文/emoji/代理对/ZWJ 有单测锁定）。
- messages/tools/parameters 用 index/key 寻址（span 可省略——index 寻址本身
  exact）；`span: null` = identity-only（知道存在与身份、无法定位），只允许
  structural/opaque。
- section 顺序 = Semantic Request 实际顺序（§一百一十一）；ordinal = 数组下标，
  未来持久化以 callId + ordinal 引用（§一百一十二）；单 call 上限 1024 段，
  超限尾段折叠并记录（§一百一十）。

## Precision Contract

- `exact`：来源已知 + 位置由运行时实际对象证明。禁止「重建 = exact」（§三十二）
  ——唯一例外是被描述对象与冻结快照/构造产物为同一数据结构且经 runtime 验证
  （MC-01 的 `startsWith(customPrompt)` 前缀验证对**真实冻结快照对象**执行，
  验证失败降级 structural）。
- `structural`：位置/范围已知但来源只能粗分类；identity-only 段（skills/
  agentsFiles/append、MC-02 system、MC-03 全部）属此档。
- `opaque`：SDK/Adapter 在该位置加入输入但 Lingxi 无法定位（本轮实际使用：
  MC-07 CLI 内部 wire 由 attempt 级 external_process_boundary 表达，无 section
  级 opaque 段）。
- Call 级 rollup：全部 exact → exact；有 exact 且有 structural/opaque →
  partial；全部 opaque → opaque。无百分比（§一百一十五）。

## MC-01～MC-10 Provenance Matrix（Step 14 最终矩阵）

| Path | Semantic Boundary | System Provenance | Messages | Tools | Media/Audio | Overall Precision | Snapshot-safe | Provider Payload Changed |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| MC-01 chat | streamFn wrapper（model-call-stream-observer.ts:117） | 快照前缀验证 exact（platform/persona/user_profile/memory_context/agent_roster/session_instruction）+ SDK 尾段 structural + skills/agentsFile/append identity-only | conversation_history / current_user_input（turn 证明）/ tool_result 全 exact | tool_definition exact（source=tool name） | N/A | **partial**（SDK 尾段 structural） | ✅（快照冻结 provenance；persona V1→V2 不漂移） | 否 |
| MC-02 compaction run | isolatedStreamFn（runner :430+） | session_instruction 整段 structural | live→conversation_history exact；instruction strict→task_instruction / repair→format_constraint；recovery toolResult→tool_result（observer 尾段扩展） | tool_definition exact（placeholder name） | N/A | **partial**（system structural） | N/A（runner 不做快照证明，诚实） | 否 |
| MC-03 native compaction | streamFn（isCompacting 分支） | task_instruction structural（SDK 镜像非同源，不伪造 exact） | task_input structural（serializeConversation 拍平不可拆） | 无 | N/A | **partial**（全部 structural，无 exact 伪造） | N/A | 否 |
| MC-04 callText | merge 完成 + normalizedMessages（llm-client.ts §1.5） | task_instruction（layout: template exact；fallback structural）；codex 空系统 → adapter_injected exact | task_input/task_instruction 段级（caller 显式）或 structural fallback；system merge 同步 remap | 无 | vision/appearance multimodal content 寻址 | 显式 caller **exact** / 未迁移 **partial** | N/A | 否（有字节一致测试） |
| MC-05 probe | 请求体构造（provider-client.ts） | 无 | task_instruction exact（固定占位消息） | 无 | N/A | **exact** | N/A | 否 |
| MC-06 image HTTP | submit params（image-task-runner.ts） | codex 固定 instructions；其余无 | N/A | codex image tool | media_prompt + media_reference（parameters 寻址，不含值） | **exact** | N/A | 否 |
| MC-07 CLI | argv 构造（dreamina.ts；provenance 在 runner 边界） | 无 | N/A | 无 | media_prompt/media_reference exact；CLI 内部 wire opaque（attempt 表达） | **partial**（结构性） | N/A | 否 |
| MC-08 video HTTP | submit params（universal-media-manager.ts） | 无 | N/A | 无 | media_prompt/media_reference；duration/resolution/fps 为 config 不进 provenance | **exact** | N/A | 否 |
| MC-09 speech | _transcribeWithAccounting | 无 | mimo/dashscope 包裹层 | 无 | audio_input + language_hint（input/parameters 寻址） | **exact** | N/A | 否 |
| MC-10 direct summary | generateSummary facade 参数边界 | （SDK 内部，不在本边界） | conversation_history/tool_result 段级 exact | 无 | customInstructions→task_instruction / previousSummary→previous_summary（parameters 寻址） | **exact** | N/A | 否 |

## Prompt Source Matrix（§一百一十七）

| Source Category | Producer | Render Point | Semantic Target | Exact Locator | Template/Source Identity | Known Gaps |
| --- | --- | --- | --- | --- | --- | --- |
| platform | agent.ts buildSystemPromptArtifact 各 runtime 块 | chunk 装配 | systemPrompt | ✅ span | runtime: platform.* | — |
| persona | yuan/identity/AGENTS.md 模板 + appearance | 同上 | systemPrompt | ✅ span | runtime: persona / agent.appearance | — |
| user_profile | user.md + resolveUserName | 同上 | systemPrompt | ✅ span | runtime: user.profile | — |
| memory | pinned.md/memory.md/规则模板 | 同上 + memory 域 utility | systemPrompt / userContent | ✅ span | memory: memory.pinned/longterm/time-context | rolling summary system 内嵌 persona/memory/roster 的 span 未拆（整段 template exact） |
| skills | Pi SDK formatSkillsForPrompt（快照 skillsResult） | SDK customPrompt 尾部 | systemPrompt | ❌ identity-only | skill: <names≤8> | SDK 拼装位置不可定位（等待 SDK extension） |
| agents_file | SDK agents files 注入 | SDK customPrompt 尾部 | systemPrompt | ❌ identity-only | runtime: <basename≤8> | 同上；绝对路径禁入 |
| history | sessionManager JSONL → convertToLlm | streamFn context.messages | messages[i] | ✅ index | — | — |
| user_input | session.prompt() 输入 | streamFn context.messages | messages[lastUser] | ✅ index（turn 证明） | — | agent.continue 等无 turn 标记场景诚实归 conversation_history |
| tools | agent.state.tools | streamFn context.tools | tools[i] | ✅ index | tool: <name>（不存 schema） | — |
| tool_results | agent loop toolResult | streamFn context.messages | messages[i] | ✅ index + toolName/toolCallId | — | — |
| task templates | memory/diary/approval/vision 等 prompt builder | 各 caller 构造点 | systemPrompt / messages | ✅ span/index | template: cacheGroup+templateVersion 或 template id | — |
| format constraints | repair/预算/格式指令 | 各 caller 构造点 | userContent 段 / 独立 message | ✅ | runtime/template id | — |
| summaries | previousSummary/prevDraft/草稿 | 各 caller 构造点 | userContent 段 / parameters | ✅ | memory: *.previous | — |
| media | image/video submit params | runner 边界 | parameters | ✅ key/index | runtime: media.prompt/reference | 值（URL/路径/base64）绝不入 provenance |
| speech | audio + language | service 边界 | input/parameters | ✅ key | runtime: speech.* | 音频字节/转写文本绝不入 provenance |

## Session Snapshot Semantics

- `SessionPromptSnapshot` v1 **不升级版本**：`systemPromptProvenance` 为附加可选
  字段（安全 metadata：category/locator/source/precision，无 sections[].content
  ——§十四禁止内容副本）。旧快照 normalize 后无该字段 → streamFn 侧整段
  structural session_instruction（§八十五诚实降级，不伪造 FULL）。
- 新建 session 经 `buildSystemPromptArtifact`（与旧 buildSystemPrompt 同一装配，
  golden 字节锁定）冻结 text+provenance；restore 优先用快照 provenance；
  `finalSystemPrompt` 覆盖路径下 customPrompt 前缀性质保持（前缀验证兜底）。
- 持久化指纹已按 compatible repin（无 store schema/payload 形状变化）。

## Known Opaque Sources（诚实清单）

1. Pi native summarizer system prompt（MC-03）：SDK 常量不随包出口，Lingxi 镜像
   为手工副本 → structural（runtime 等值不作 exact 依据）。
2. Pi native summarizer messages[0] 内部组成：serializeConversation 在 SDK 内，
   按标签解析最终字符串属禁止的反推 → structural。
3. SDK system prompt 尾部（append+project_context+skills+cwd 混合段）：单段
   structural + identity-only 子段；不猜 span。
4. MC-02 system prompt 整段：runner 取 session 最终 prompt，不做前缀证明。
5. Dreamina/Jimeng CLI 内部 wire：外部进程边界（attempt 级 opaque 表达）。
6. rolling summary system 内嵌 persona/userProfile/memory/roster：整段按 template
   identity exact，子 span 未拆（模板字面量拆分转录风险 > 收益，记录为 gap）。
7. provider-compat normalizeProviderPayload 的字段改名/搬移：Phase 6 Provider
   Request Capture 处理（唯一例外 codex 空系统注入已标 adapter_injected）。

## Prompt Equivalence Tests / Safety Tests

- 等价：agent golden（改造前 fixture，zh/en 字节一致 + span 首尾相接）；renderer
  单测（join 语义/空段/UTF-16）；compile/_compactLLM、rolling repair、
  install-skill、dream 四处运行时「渲染===原串」防御；callText 传/不传
  provenance wire body 逐字节一致；memory/dream/approval/vision/diary/appearance
  域既有测试（11776 全绿）锁定内容。
- 安全：毒丸（TOP_SECRET_PERSONA/MEMORY/USER/TOOL_RESULT + image/speech 场景）
  JSON.stringify 不可见；Metadata Safety Gate 回归；Observer/Trace/Control-plane
  既有测试全绿；事件 symbol 引用不参与序列化。

## Next Phase

Phase 6：Request/Response Capture + Redaction Contract + Sensitive Payload
Boundary——届时在既有 callId 下同时取得 Semantic Request + Provenance，由
Redaction Pipeline 决定内容保存策略；Provider Request 层（compat 变换后的 wire
payload）的 provenance 也在该轮处理。

---

# Phase 6 — Sensitive Payload Capture + Redaction + Provider-Wire Provenance（第五轮）

第一性原理目标：在运行时真实可见的位置取得每一次 Model Call 的
**Semantic Request / Provider Request / Provider Response / Semantic Response**
四层正文，经统一、机器可执行、不可绕过的 Redaction/Externalization 后送入独立
的 Sensitive Payload Channel——与 ModelCallObserver（safe metadata channel）完全
隔离的第二契约。无法观察的 provider wire 显式标记 unavailable/opaque，绝不重建
（§八十四/§一百零三）。

## Phase 6 Sensitive Payload Contract

- `lib/llm/model-call-payload-types.ts`：`ModelCallPayloadRecord`（schemaVersion/
  kind/identity/attemptId/providerRequestOrdinal/visibility/fidelity/sanitization/
  payload/provenance sidecar）。kind 闭集 = semantic_request|provider_request|
  provider_response|semantic_response。metadata 复用 Observer 既有 safe identity
  （model/source/attribution），不建第二套身份。
- 一个 logical call 的基数（§十八）：1 semantic_request、N provider_request、
  N provider_response、0..1 semantic_response；N 由 transport 边界决定。
  `providerRequestOrdinal` 由 capture session 单调分配（§十九）：codex image
  401 refresh = 同 call 两条 provider_request（ordinal 1/2、两个 attemptId）。
- 资源上限（§四十三，测试锁定）：maxDepth=24 / maxNodes=20000 / maxArrayItems=256 /
  maxObjectKeys=128 / maxStringChars=131072 / maxRecordChars=1000000。超限显式
  truncated/degraded，绝不静默 slice。

## Capture Channel Architecture（§九/§十一/§十四）

```
Model Call Runtime ─┬─ ModelCallObserver（SAFE METADATA，事件无正文——契约冻结）
                    └─ ModelCallPayloadCapture（SENSITIVE DATA）
                           ↓ 统一 Redaction（先脱敏后入 sink）
                         ModelCallPayloadSink（sanitized detached copy only）
```

- 生产默认 sink = `NOOP_MODEL_CALL_PAYLOAD_SINK`：不安装 sink 时
  `createModelCallPayloadCaptureSession` 返回 null，集成点以 null 短路——不深
  遍历、不脱敏、不复制（§四十五，测试用 spy 锁定 redactor 不运行）。
- session 只持身份 + ordinal 计数器 + sink 引用（§一百二十），不持 Prompt
  history；capture 后即释放原始引用。
- 本轮只实现 Noop + Test sink；未来 Payload Store 只能作为新 sink 接入（§十二）。
- sink throw/redaction error 全部就地吞掉（§十三，测试锁定 callText/Pi/media/
  speech 业务照常）。
- 关联通道：recorder 持 session handle（`recorder.payloadCapture`）；Pi 路径经
  ALS scope（`scope.payloadCapture`）共享给 provider hooks——hook 里的临时
  recorder 看不到原 recorder 实例，共享的是 session capability 引用（§一二三）。

## Redaction Contract（§二十三～§三十七/§三十八～§四十三）

`lib/llm/model-call-payload-redaction.ts`，copy-on-capture（原始对象绝不修改，
§十五，测试锁定 before/after 逐字节一致；§一百七十：secret 不进第二份副本——
遍历时直接构造安全副本，无 blind structuredClone）：

1. **credential 键**（任意嵌套，归一化整键匹配）：authorization/x-api-key/
   api-key/x-goog-api-key/cookie/set-cookie/access_token/refresh_token/
   client_secret/private_key/password/secret/signature… → 值替换
   `<redacted:credential>`，键名与安全 header（content-type/anthropic-version/
   chatgpt-account-id）保留（§二十六/§一百一十四）。
2. **协议专项 body credential 路径**（§二十五/§一百四十九）：
   `PROVIDER_BODY_CREDENTIAL_PATHS["volcengine-bigasr-transcription"] = ["user.uid"]`
   ——Volcengine ASR 的 body 内 credential 由结构化规则处理，不依赖 generic key
   名（专项硬验收测试锁定 reason=protocol-body-credential；generic `uid` 键不
   受影响）。
3. **高置信 inline secret**（§三十五/§三十六，正例+反例测试锁定）：PEM 私钥整块
   （含 BEGIN/END 行）、JWT、sk-/sk-ant-/ghp_/AIza/AKIA、Bearer/Basic token 部分
   （保留字面量）、`api_key=…`/`access_token: …` 等 kv 形态（值 ≥16 token 字符）。
   反例：UUID、file id、`token count`/`model=` 研究文本、sha256 hash、多语言
   普通文本全部存活（§一百二十九）。
4. **URL**（§二十八/§二十九）：query credential（key/token/signature/X-Amz-*/
   X-Goog-* 前缀）→ `{kind:"external_reference", scheme, host, path, redacted:true}`
   descriptor；普通 endpoint 原样保留。
5. **本地绝对路径**（§三十）：整串 → `{kind:"local_file_reference", basename}`；
   文本内嵌 `/Users/…`/`C:\Users\…` inline 替换 `<redacted:secret>`。
6. **二进制**（§三十一/§三十二）：Buffer/TypedArray/ArrayBuffer/Blob →
   `{kind:"external_blob", mediaType, byteLength, captureStatus:"externalized"}`；
   data URL / 已知媒体键（b64_json/image_base64/audio/data/result…） /
   ≥1024 字符 base64 采样（256 字符有界采样，§三十三）→ descriptor。不写字节、
   不 hash 大媒体。FormData → `{kind:"multipart_form_data", fields, files[]}`。
   AbortSignal（google payload 内嵌）按 unsupported 剔除。
7. **offset mapping**（§四十八～§五十）：`redactTextWithMap` 返回 replacements；
  semantic_request capture 用 `remapSpanAfterRedaction` 把 Phase 5 systemPrompt
  span 平移到脱敏后文本（无重叠 → 平移；重叠/截断越界 → span=null + precision
  降级 structural + action 记录），绝不保留错位位置。

Sanitization summary（§三十八～§四十二）：`{redacted, truncated, degraded,
actions[]}`——actions 是 `{path, action, reason}` 闭集（removed/replaced/
externalized/truncated/unsupported），**绝不携带原值**（§三十九）。visibility
（观测能力）与 sanitization（安全变换）正交（§四十一）：provider request 完整
可见但 Authorization 被替换 = visibility full + sanitization.redacted。

## Payload Fidelity Contract（§二十二）

runtime_exact（发送前 body 对象/invalid-JSON rawText）｜parsed_equivalent（业务
JSON.parse 结果）｜stream_aggregate（codex SSE aggregate/Pi assembled message）｜
normalized（semantic response 外壳）｜metadata_only（probe 成功不读 body、Pi
after_provider_response、codex 401 首响应）｜external_process（CLI opaque）｜
opaque（unavailable record）。严禁自称 raw。

## Semantic Request Capture（§四十六/§四十七）

boundary = Phase 5 provenance boundary：MC-01/02/03 = streamFn context
（systemPrompt/messages/tools，含 tool definition schema——正文级，§七十三）；
MC-04 = merge 后 mergedSystem(+codex 注入) + normalizedMessages；MC-05 = 固定
占位消息 "."（Phase 6 起允许捕获值，§八十六）；MC-06/08 = {prompt, image refs}；
MC-09 = {audio→local_file_reference, language}；MC-10 = 三元组全参。
capture 副本上的 systemPrompt locator 保持可解析（span remap 后 slice 验证，
测试锁定）；provenance 契约本身保持 content-free（§一百一十六）。

## Provider Request Capture（§六十四/§七十四）

- MC-04：`normalizeProviderPayload` 之后、`JSON.stringify` 之前的最终 body +
  真实 headers/endpoint（构造点局部变量，非重建）。
- MC-01：`before_provider_request` hook 的 `event.payload`——pi-ai 0.84.1 实证
  为 compat 转换后、序列化前的最终 body 活引用（runtime_exact，audit §1.1）；
  hook 不暴露 headers/endpoint（诚实 null）；凭证不在 payload（vendor SDK fetch
  层拼装；body 内意外出现的 credential 由 redactor 防御纵深替换）。
- MC-05/06/08/09：adapter 真实构造点 body（observedProviderFetch 的 capture
  描述符，§八十九——helper 只搬运，不反射 closure）。
- MC-02/03（options 无 onPayload，运行时判定）与 MC-10（summarizer options 无
  hook）→ 显式 unavailable record（§八十四/§一百零三），不从 semantic 重建。
- MC-07 CLI → 显式 opaque/external_process record；argv/stdout 绝不冒充 wire
  （§九十五，测试锁定 stdout 毒丸不可见）。

## Provider-Wire Provenance（§五十三～§六十一）

`lib/llm/provider-request-provenance.ts`：`ProviderRequestProvenance`
（semanticSectionOrdinal → providerLocator{path,span?} + transformation 闭集
10 值 + mappingPrecision exact/structural/opaque）。mapping 在 **transformation
发生时**由构造代码产生（§五十九）：callText 四协议的落点（body.system /
messages[0].content / instructions / input[i]）在 llm-client 构造分支内与 body
同源产生；`normalizeProviderPayload` 之后做 locator 存在性 + 长度校验（构造
产物自检，非内容搜索，§五十八），失配降级 structural（§一三八——structural
语义 section 不产生 exact mapping）。compat mutation 测试锁定降级行为。
Pi 路径 body 由 vendor SDK 构造，无法在 transformation 处产生 sidecar →
`providerRequestProvenance = null`（不为矩阵全绿重写 serializer，§六十一）。

## Provider Response Capture（§六十六～§七十一/§一百五十一/§一百五十二）

自有 fetch 路径在业务解析点捕获（复用已解析对象，§一百六十七；不 tee/clone
stream，§六十九）：success = parsed body；error body 同样捕获（先捕获后抛错）；
codex = stream_aggregate；probe 成功 = metadata_only（业务不读 body，诚实）。
Pi = after_provider_response 仅 status+headers（metadata_only）；google/
mistral-conversations adapter 不触发 onResponse（audit §1.2）→ 显式 unavailable。
network error 无 provider_response record（§一百一十二）；error body 中的普通
diagnostic 保留、credential 删除（§一百五十二，测试锁定）。

## Semantic Response Capture（§七十/§一百零五～§一百一十/§一百五十四）

统一外壳 `ModelSemanticResponse`（text/reasoning/toolCalls/structuredOutput/
media/transcription/finishReason/usage/completeness）。MC-04 = parser + thinking
strip 之后、业务校验之前；MC-01 = assembled message content blocks（toolCall
name/arguments/id 捕获，§一百零七；redacted_thinking 只留结构标记不解密，
§一百一十）；aborted/error 有已组装内容 → completeness=partial（§八十），完全
无输出不制造（§一百五十五）；MC-06/08 = task submission 语义（taskId/deferred/
fileCount，§一五六）；MC-09 = transcription 正文（§一百零一）。

## MC-01～MC-10 Capture Matrix（Step 20 最终矩阵）

|MC Path|Semantic Request|Provider Request|Provider Response|Semantic Response|Provider Fidelity|Binary Policy|Redaction|Provider Provenance|
|---|---|---|---|---|---|---|---|---|
|MC-01 Pi Chat|FULL|FULL（hook payload）|METADATA_ONLY（hook；google=UNAVAILABLE）|FULL|runtime_exact / metadata_only|externalize|统一 Redactor|null（SDK 构造，无 sidecar）|
|MC-02 AgentRun|FULL|UNAVAILABLE（options 无 onPayload）|UNAVAILABLE|FULL|opaque|externalize|统一 Redactor|null|
|MC-03 Native Compaction|FULL|UNAVAILABLE|UNAVAILABLE|FULL|opaque|externalize|统一 Redactor|null|
|MC-04 callText|FULL|FULL（构造点）|FULL（parsed/stream_aggregate/rawText）|FULL|runtime_exact / parsed_equivalent / stream_aggregate|externalize|统一 Redactor（含 mapping remap）|exact×4 协议（构造时产生 + post-compat 校验）|
|MC-05 Probe|FULL（"." 允许捕获）|FULL（构造点）|METADATA_ONLY（成功）/FULL（error body）|FULL（structuredOutput）|runtime_exact|externalize|统一 Redactor|null（固定形状）|
|MC-06 Image ×7|FULL|FULL（构造点）|FULL（parsed）|FULL（media submission）|runtime_exact / parsed_equivalent / stream_aggregate(codex)|reference+b64 externalize|统一 Redactor（含 FormData）|null|
|MC-07 Dreamina CLI|FULL|OPAQUE|OPAQUE|FULL（taskId）|external_process|argv 不捕获|统一 Redactor|null|
|MC-08 Video|FULL|FULL（agnes 构造点；CLI=OPAQUE）|FULL（parsed）|FULL（taskId/deferred）|runtime_exact / parsed_equivalent|reference externalize|统一 Redactor|null|
|MC-09 Speech ×4|FULL（audio descriptor）|FULL（构造点；含 body.user.uid 协议规则）|FULL（parsed）|FULL（transcription）|runtime_exact / parsed_equivalent|audio externalize|统一 Redactor（协议专项硬验收）|null|
|MC-10 Direct Summary|FULL（三元组）|UNAVAILABLE|UNAVAILABLE|FULL（summary text）|opaque|externalize|统一 Redactor|null|

## Known Opaque Payloads（§一六二）

1. MC-07 CLI provider wire（external_process）。2. MC-02/03/10 provider wire
（pi 0.84.1 summarizer options 无 onPayload——audit §1.4 实证）。3. google/
mistral-conversations 的 provider_response。4. Pi provider request 的 headers/
endpoint（hook 不暴露）。5. 全部二进制（无 Blob Store，本轮不写字节，§一六二）。

## Tests（Step 18-19/24，103 个新用例 + 131 个既有观测回归）

`tests/model-call-payload-redaction.test.ts`（49：毒丸正例/反例、credential 键、
Volcengine 协议专项、URL、本地路径、二进制/FormData/AbortSignal、原对象不可变、
资源上限、循环引用、span remap）；`-capture.test.ts`（9：noop 快路径、sink
throw、record 形状、ordinal、unavailable、provenance remap、毒丸断言）；
`-calltext.test.ts`（12：四协议四层、codex 注入、system merge mapping、
400/401/429/500 error body、invalid JSON、network/abort、wire 等价 + redactor
不运行、sink throw、inline secret）；`-pi.test.ts`（8：hook fidelity 锁定、
MC-02/03 unavailable、google、aborted partial、toolCall/redacted_thinking、
快路径）；`-media.test.ts`（13：7 adapter coverage、本地参考图、codex 401 双
ordinal、CLI opaque、agnes video、500、快路径）；`-speech.test.ts`（7：4 adapter
coverage、Volcengine 专项、500、快路径）；`-summary.test.ts`（5：probe 四层 +
401 + GET /models 0 record、diary 三元组 + unavailable + 网络失败）。

## Next Phase

- Payload Store / Blob Store / Query API / Export / Usage UI（全部明示本轮不做）。
- 未来 sink 接入：持久化 sink 只能实现 ModelCallPayloadSink，永远收 sanitized
  detached copy。
- Pi provider mapping sidecar：需上游在 buildParams 处暴露 mapping（或 Lingxi
  侧 serializer 包装修改），当前诚实为 null。

---

# Phase 7 — Durable Model Observatory Storage（第六轮，2026-08-22）

> 审计入口：MODEL_OBSERVABILITY_STORAGE_AUDIT.md（Step 1 十问交付 + 架构决策 +
> pragma 表 + At-Rest 结论）。断点文件：OBSERVABILITY_STORAGE_PROGRESS.md。

第一性原理目标：把 Phase 1～6 建立的模型调用事实安全投影为**跨进程重启可恢复的
Durable Model Observatory**——Storage 永远是 Observer/Capture 的消费者，而不是
模型执行依赖。

## Storage Architecture

```text
Model Runtime ─┬─ ModelCallObserver（SAFE METADATA，契约未动）
                └─ Payload Capture（统一 Redaction 后 sanitized copy）
                         │
              ModelObservabilityPersistenceCoordinator（handler 只 enqueue）
                         │  bounded trace/payload/blob queues + setImmediate batch
              {LINGXI_HOME}/model-observability/observability.sqlite（单 DB 逻辑分表）
                         └─ blobs/{shard}/{blobId}.bin（privileged externalizer 通道）
```

- 模块（lib/llm/）：model-observability-schema（DDL + user_version migration +
  disable-on-failure open）/ -trace-store（事件→SQL 投影）/ -payload-store（sanitized
  record 持久化 + blob descriptor 存储态归一）/ -blob-store（atomic 文件写 + GC +
  orphan/missing recovery）/ -retention（policy + maintenance）/ -persistence
  （coordinator + install/uninstall/composite wiring）/ -testing（测试 harness）。
- SQLite：WAL / synchronous=NORMAL / busy_timeout=5000 / secure_delete=ON /
  auto_vacuum=INCREMENTAL / foreign_keys=OFF（容忍 out-of-order + partial crash，
  关联完整性由 shell upsert 与读侧解释承担，§二十四）。

## SQLite Schema（v1，PRAGMA user_version 自管）

traces（trace_id PK/origin/first_seen/last_seen/call_count）｜model_calls（call_id PK
+ trace/parent + model 三元组 + source 四元组 + attribution 十列独立索引列 +
call_purpose + 生命周期时间戳 + terminal_status/error + provenance 摘要列 +
attribution/source/safe_details JSON + persistence_completeness +
interrupted_by_restart + payload_availability）｜model_attempts（attempt_id PK/call_id/
四时间戳/provider_request_id/http_status/attempt_visibility/provider_wire_visibility/
error）｜payload_records（自增 id PK + kind/attempt/ordinal/captured_at/visibility/
fidelity/sanitization 三布尔/正文与双 provenance JSON/字符数）｜blob_objects｜
payload_blob_refs｜observability_meta（drop 计数等 health 元数据持久化）。

## Trace Store（事件投影，§二十）

logical_call_start→call+trace upsert（origin 进 traces）；attempt_start→attempt 行
（attemptVisibility/providerWireVisibility 落列）；provider_request_prepared/
provider_response_received→attempt 时间戳/httpStatus/providerRequestId（attempt
shell 幂等，允许 NULL=事件缺失即真相，§二十一）；semantic_response_completed→
semantic_completed_at；attempt_error/logical_call_error/aborted→安全 error 事实；
logical_call_end→ended_at+terminal_status+persistence_completeness=complete。
不从 payload 反推 observer 事件（§二十二）；payload 先到→partial call shell
（started_at NULL，§二十三）。

## Payload Store

只实现 ModelCallPayloadSink（sanitized detached copy only，§四/十七）；无第二次
业务 redaction——只做 serialization safety/size hard limit（1M chars）/kind 闭集
校验（fail closed drop，§十八）；staged blob descriptor 在 commit 期归一
stored/store_failed（存储态记账；失败降级移除 blobId，绝不 dangling ref，§七十一）；
跨批 blob 以 metadata row 存在性验证 durable（isBlobDurable）；排序 = 自增 id。

## Blob Store（privileged contract，§六十/六十一）

- 通道：Redactor（describeBinary）在统一脱敏时咨询 `ModelCallBlobExternalizer`
  （进程级注册点，默认 null=Phase 6 externalized 行为，§六十二）；字节经
  externalizer bounded 复制进 blob queue，Payload 通道只拿 descriptor
  （blobId=mb_<random>，无内容 hash，§六十六；磁盘文件名=<blobId>.bin，§六十七）。
- eligible：仅 runtime 真实 materialized 的 Buffer/TypedArray/ArrayBuffer（§六十三）；
  绝不自动读本地文件/下载 URL/fetch signed URL（§六十四）。**Blob/base64/dataURL
  保持 externalized（PARTIAL，§七十四）**。
- 写入：随机 staging 文件（0600）→ rename 发布（§七十）；flush 顺序 = blob 文件
  先写 → 同一 SQLite transaction 提交 blob_objects + payload_records + refs
  （§七十二）；GC 只删 0-live-ref（§九十一）；orphan 文件 24h grace 回收（§九十二）；
  missing 标 state=missing 不 crash（§九十三）。

## Persistence Queue（coordinator，§三十四～四十一）

- handler 只 enqueue（零同步 fs/SQLite）；trace(4096)/payload(2048)/blob(256 个，
  64MB) 独立 bounded queue——1MB response 拖不死 trace metadata（§一百一十）。
- overflow：drop newest + 显式计数 + call 标 payload_availability='dropped'；
  计数持久化 observability_meta、跨 restart 恢复（§四十三）。
- flush：setImmediate coalesce + 2s interval（unref）；transaction throw → 整批
  rollback 后单次 retry → 再失败诚实 drop（§四十九）。
- crash 诚实语义（§四十四）：接受崩溃丢最后一个未 flush batch，不要求
  logical_call_start 落盘后才发 Provider 请求；graceful shutdown flush（engine
  .dispose 5s bounded timeout，§四十五）。

## Crash Semantics（§四十六/四十七）

Startup Reconciliation 只做 `interrupted_by_restart=1`（persistence inference），
terminal_status 保持 NULL——用户杀进程 ≠ Provider error，不伪造 logical_call_end。

## Retention Contract（§五十三～五十八）

policy 六维度；safe fallback 集中定义（trace 180d/payload 30d/blob 30d）；产品
默认值留 Phase 8（§五十四）。payload 可先于 trace 过期：整 trace 单位删除正文、
call 标 payload_availability='expired'，metadata 保留；blobMaxAge 只作用于 refless
blob（删仍被引用的 blob 会制造 dangling ref，违反 §七十一——其寿命由 payload
retention 决定）。maintenance = startup once + 1h unref timer + 显式
runMaintenance()，runWithoutModelTrace detach（§八十六/八十七），compact =
wal_checkpoint(TRUNCATE)+incremental_vacuum（§八十八）。

## At-Rest Protection（§七十五～七十七/一百三十七）

**Is observability content cryptographically encrypted at rest? NO.** 全仓无
keytar/safeStorage/libsecret（audit Q6 实证）→ 不实现伪加密（§七十六）。保护 =
private 目录 Unix 0700 + DB/WAL/SHM/blob 0600（目录先收紧，§七十八；Windows
依赖 profile 继承 ACL，不假装 POSIX 等价）+ payload persistence 显式 opt-in +
bounded retention。剩余威胁：同用户进程直读、备份/同步渠道明文复制、SSD
wear-leveling（删除语义 = logical deletion + secure_delete + blob unlink，不承诺
物理不可恢复，§九十）。

## Data Epoch Classification & Store Registry（§三十～三十二）

两个新 descriptor：`model-observability-db`（sqlite-runtime + fingerprint
introspector 开真实 store）、`model-observability-blobs`（tree）。均
epochPolicy=compatible、affectedByEpochMigration=false、checkpoint/restorePolicy
显式写明排除理由（结构性排除：migration 批次引用即 fail-closed）。scanner 61
stores 全绿；fingerprint compatible repin。

## Persistence Lifecycle（§七十九～八十五）

默认 policy disabled（不建文件，生产=Phase 6）；`new LingxiEngine({
modelObservability })` + `startServer` CompositionRoot 透传（engine_construct 安装，
早于一切模型调用；Phase 8 接 UI/settings，无隐藏 env 开关）；persistTraceMetadata/
persistPayloads/persistBlobs 独立开关（persistBlobs⊆persistPayloads）；composite
observer/sink 保持既有 test/debug sink 工作；close/uninstall 恢复先前注册对象。

## MC-01～MC-10 Durable Matrix（§一百三十四）

|MC Path|Trace Durable|Attempts|Semantic Req|Provider Req|Provider Resp|Semantic Resp|Provenance|Restart Safe|
|---|---|---|---|---|---|---|---|---|
|MC-01 Pi Chat|✅|logical_boundary（1）|FULL|FULL（hook body）|**METADATA_ONLY**（持久化不升级）|FULL|provider mapping **null** 保留|✅|
|MC-02 AgentRun|✅|logical_boundary|FULL|**UNAVAILABLE**|**UNAVAILABLE**|FULL|—|✅|
|MC-03 Native Compaction|✅|logical_boundary|FULL|**UNAVAILABLE**|**UNAVAILABLE**|FULL|—|✅|
|MC-04 callText|✅|exact + request_response|FULL|FULL|FULL|FULL|四协议 mapping exact 持久化|✅|
|MC-05 Probe|✅|exact|FULL（"." 值）|FULL|**METADATA_ONLY**|FULL（structuredOutput）|—|✅|
|MC-06 Image ×7（codex 401）|✅|exact ×2|FULL|FULL ×2（ordinal 1/2）|FULL ×2（401/200）|FULL|—|✅|
|MC-07 Dreamina CLI|✅|external_process_boundary + **opaque**|FULL|**OPAQUE**/external_process|**OPAQUE**|FULL|—|✅|
|MC-08 Video|✅|exact|FULL|FULL|FULL|FULL（taskId/deferred）|—|✅|
|MC-09 Speech ×4|✅|exact|FULL（audio→blob stored）|FULL（Volcengine uid 协议脱敏持久化）|FULL|FULL（transcription）|—|✅|
|MC-10 Direct Summary|✅|logical_boundary|FULL（三元组）|**UNAVAILABLE**|**UNAVAILABLE**|FULL|—|✅|

Persistence 未把任何 UNAVAILABLE/OPAQUE/METADATA_ONLY 升级为 FULL（durable-matrix
测试锁定）。

## Storage Completeness Matrix（§一百三十五）

|Fact|Runtime Source|Persistent Table/Store|Key|Missing-state Semantics|
|---|---|---|---|---|
|Trace|ModelTraceScope traceId|traces|trace_id|事件缺失即真相；无假 trace|
|Logical Call|logical_call_start|model_calls|call_id|start 事件丢→partial shell（started_at NULL）；payload 先到同|
|Attempt|attempt_start|model_attempts|attempt_id|request/response/error 时间戳 NULL=事件未发生（MC-03）|
|Semantic Request|capture session|payload_records kind=semantic_request|(call_id,id)|无行=not captured（capture 层未捕获或 persistPayloads=false）|
|Provider Request|capture/hook|payload_records kind=provider_request|(call_id,provider_request_ordinal)|unavailable/opaque 行保留原语义|
|Provider Response|capture/hook|payload_records kind=provider_response|同上|同上；network error 无行=真相|
|Semantic Response|capture session|payload_records kind=semantic_response|(call_id,id)|0..1 基数；无输出不制造|
|Semantic Provenance|Phase 5 sidecar|payload_records.semantic_input_provenance_json|(call_id,id)|span remap 后仍可定位（locator roundtrip 测试）|
|Provider Provenance|构造点 sidecar|payload_records.provider_request_provenance_json|(call_id,id)|Pi 路径 null 保留|
|Blob|externalizer|blob_objects + payload_blob_refs + blobs/*.bin|blob_id|state=missing（文件丢失不 crash）；store_failed descriptor 无 blobId|
|Usage correlation|ledger metadata.modelCallId|（不进本 store；Usage Ledger 独立）|modelCallId|两系统经 modelCallId 关联，互不替代|
|Drop/失败计数|coordinator health|observability_meta|key|Phase 8 可诚实告知「这一段观测有缺失」|

## Retention Matrix（§一百三十六）

|Data Class|Retention Policy|Deletion Unit|GC Dependency|Survives Payload Expiry|
|---|---|---|---|---|
|Trace|traceMaxAgeMs（fallback 180d）/maxTraceRows?|完整 trace|—|—（本体）|
|Call|随 trace|完整 trace 内|trace 删除|✅（metadata 属 trace）|
|Attempt|随 call|完整 trace 内|trace 删除|✅|
|Payload|payloadMaxAgeMs（fallback 30d）/maxPayloadBytes?|完整 trace 的全部 payload|删除后 call 标 expired|—（本体先删）|
|Blob|blobMaxAgeMs（仅 refless）/maxBlobBytes?|refless blob|payload 删除 → ref 消失 → GC|被引用 blob 随 payload 删除后 GC|
|Health metadata|不按年龄删除（observability_meta）|—|—|✅|

## Tests（Step 14-17）

新增 6 文件 44 用例（详见 OBSERVABILITY_STORAGE_PROGRESS.md）。关键硬测试：
毒丸落盘字节级扫描（DB+wal+shm Buffer.includes，§一百零一）；codex 401 双 ordinal
durable（§九十七）；opaque/unavailable 不升级（§九十八/九十九）；crash 不伪造终态
（§一百零六）；write failure/queue overflow/trace 优先/graceful flush（§一百零八～
一百一十一）；restart roundtrip/retention/blob 全套。

## Known Storage Gaps

见 OBSERVABILITY_STORAGE_PROGRESS.md（Blob PARTIAL、无加密、同用户进程威胁、
多进程长期双写未仲裁、同步 flush 无 worker、Phase 8 全未实现）。

## Next Phase

Phase 8：Unified Query Service + Filters + Group By + Drill Down + Trace Explorer
Backend + Payload Retrieval + Export Contract——建立在已稳定的 durable facts 上。

---

# Phase 8 — Unified Query & Control Plane（2026-08-22 第七轮）

> 审计：MODEL_OBSERVABILITY_QUERY_AUDIT.md（十二问 + Security/Dimension 矩阵）；
> 断点：OBSERVABILITY_QUERY_PROGRESS.md。本轮不实现最终 Usage UI（Phase 9）。

## Phase 8 Query Contract

统一模型 = Filters + Group By + Drill Down（§三）：同一 Filter Contract 驱动
Logical Call 列表（不分组）、Trace 列表、统计 Group By、Drill-down
（Group → Calls → Trace → Call → Attempt → Payload）、Export。**不是**四套
割裂的 overall/daily/category/model API。Query 唯一入口
`createModelObservabilityQueryService({ lingxiHome })`（lib/llm/model-observability-query.ts）；
route 只调它、export 复用同一 normalized query、UI 未来只调 API。
Query 层 read-only（§一百二十七）：不 DELETE/UPDATE/INSERT、不 flush writer、
不读 blob 文件、不调用模型 runtime（缺数据就是缺，不重建）。

## Filter Contract

- 时间：since inclusive / until exclusive（`started_at >= since AND < until`，
  全接口统一）；ISO-8601 严格校验。
- 身份等值：traceId / parentCallId / callId。
- 多值（字段内 OR ≤32 值，字段间 AND）：provider / modelId / api / subsystem /
  operation / surface / trigger / callPurpose / terminalStatus / attributionKind /
  sessionId / sessionPath / conversationId / conversationType / agentId /
  childAgentId / childSessionId / taskId / inputShape / provenancePrecision /
  payloadAvailability。
- **category ≡ subsystem**（§十九 alias；与旧 Usage UI `entry.source.subsystem`
  一致——不是 callPurpose）；callPurpose / operation 是独立维度。
- terminalStatus 支持 `incomplete` 伪值（terminal_status IS NULL——logical
  incomplete 是合法事实）；payloadAvailability 的 `present`/`unknown` 由 payload
  row 存在性派生（§三十七真相枚举：present/expired/dropped/not_captured/unknown，
  NULL 不折叠）；interruptedByRestart / hasPayload 布尔。
- normalize（normalizeModelObservabilityQuery / …TraceQuery / …AggregateQuery）：
  unknown field / invalid enum / oversized array / invalid date / invalid limit /
  invalid cursor → 显式 error code（route 层 400），绝不静默忽略。
- SQL 纪律（§二十一）：字段→列闭集映射（MULTI_FIELD_COLUMNS），维度/排序闭集，
  所有值绑定参数；注入测试（groupBy DROP TABLE / sort 注入 / value 含 SQL
  payload）锁定 DB 结构不受影响。

## Pagination Contract

- Call：`ORDER BY (started_at IS NULL) ASC, started_at DESC, call_id DESC`
  （NULL started_at 稳定最后，§一百）；keyset 条件分两态：cursor 在非空域
  （`started_at IS NULL OR (非空域比较)`——NULL 行整体在非空行之后，不受
  call_id 上界约束）vs 已在 NULL 域（`IS NULL AND call_id < ?`）。
- Trace：`ORDER BY last_seen_at DESC, trace_id DESC`，独立 cursor（不复用
  call cursor，§二十九）。
- Cursor：opaque base64url JSON `{v:1, kind, fp, s, c|t}`；fp = normalized
  filter（+origin）+sort 的 canonical JSON sha256 前 16 hex——filter 改变 →
  invalid_cursor；长度 ≤512；损坏/伪造 version/换 filter 复用全 fail-safe
  （400 语义，不 SQL error / OOM）。
- limit default 50 / max 200；`limit=all` 禁止（export 有独立机制）。
- 测试：100 calls×17 连续翻页无重复无遗漏、末页 cursor=null；40 同时间戳
  callId tie-break；NULL started_at 排最后且分页终止。

## Group By Contract

维度（≤3 级）：date / provider / model（provider+modelId 复合）/ category
（≡subsystem）/ operation / callPurpose / status / attributionKind / session /
conversation / agent / task / inputShape / provenancePrecision。
date bucket 显式 `{bucket:"day", utcOffsetMinutes}`（§四十三：同一 query 在
不同时区 server 结果一致；SQL `strftime('%Y-%m-%d', started_at,
printf('%+d minutes', ?))`；NULL started_at → date=null 组）。
指标（§四十一全集）：callCount/traceCount(DISTINCT trace_id)/ok/error/aborted/
incomplete/attemptCount(相关子查询按 idx_model_attempts_call)/
durationObservedCount/durationTotalMs/durationAverageMs（julianday 毫秒算术）/
usageCoveredCalls/usageMissingCalls（§二十四：与 terminalStatus 正交）/
inputTokens/outputTokens/reasoningTokens/cacheRead/cacheWrite/totalTokens/
costTotal/cacheHitCount/cacheObservedCount。全部 SQLite 聚合（LEFT JOIN
model_call_usage），UI 只接 aggregate result；无 usage 的 call 仍计入 callCount
（§一百零四 coverage 测试）。percentile 本轮不做（§四十二）。

## Accounting Projection（schema v2）

- 关系单向：Provider → **Usage Ledger（accounting truth source）** →
  model_call_usage（read-optimized durable projection）。Ledger 保留全部职责
  （5000 retention / 原子重写 / list 行为不变）。
- 表（§十一）：model_call_id PK / usage_request_id / started_at / ended_at /
  duration_ms / usage_status / input_total+uncached / output_total+reasoning /
  cache_read+write+miss / cache_hit / cache_created / cache_hit_ratio /
  total_tokens / cost_total / raw_usage_shape / created_at+updated_at。
  **不存 error.message / error.name**（Observable Metadata Safe Contract）。
- 关联（§十三）：只有 `entry.metadata.modelCallId` 存在才投影；不通过时间/
  modelId/session/顺序猜。幂等 upsert（同 modelCallId 重复 → 一行，latest wins）。
- Live ingestion（§十四）：engine 在 usageLedger 创建后
  `handle.initializeAccounting({listLedgerEntries, subscribeUsage})`——复用
  ledger append → `llm_usage` 事件（engine.subscribe），不改任何模型调用点；
  bounded usage queue（2048，溢出显式计数）随 coordinator flush 事务提交。
- Backfill（§十五）：首次 v2 启用对当前 bounded ledger（≤5000 条）best-effort
  幂等 backfill，meta key `usageLedgerBackfillCompletedAt` 只做一次；报告
  backfillSource=`bounded_usage_ledger`，**不声称完整历史**。
- Retention（§十六）：deleteTraces 随 trace 删 usage 行 + maintenance 清
  orphan usage（model_call_id 不在 model_calls）。

## Usage Correlation Matrix（Phase 8 实测）

| MC Path | Observer | Durable Call | Usage Ledger | modelCallId Correlation | Durable Usage Projection |
| --- | --- | --- | --- | --- | --- |
| MC-01 Pi Chat | ✅ | ✅ | ✅ | FULL（message_end WeakMap 补账：session-coordinator + bridge-session-manager） | ✅（有 correlation 即投影） |
| MC-02 AgentRun compaction | ✅ | ✅ | ✅ | FULL（runner ledger.start metadata） | ✅ |
| MC-03 native compaction | ✅ | ✅ | ✅ | **NONE（ledger entry 不带 modelCallId——Phase 4 遗留 gap，如实标注）** | ❌（不猜，not_correlated） |
| MC-04 callText | ✅ | ✅ | ✅ | FULL（llm-client start metadata） | ✅ |
| MC-05 probe | ✅ | ✅ | ✅ | FULL（provider-client spread） | ✅ |
| MC-06 image HTTP | ✅ | ✅ | ✅ | FULL（image-task-runner ledger.start） | ✅ |
| MC-07 CLI | ✅ | ✅ | ✅ | FULL（同 MC-06 位点） | ✅ |
| MC-08 video HTTP | ✅ | ✅ | ✅ | FULL（universal-media-manager） | ✅ |
| MC-09 speech | ✅ | ✅ | ✅ | FULL（speech-recognition-service） | ✅ |
| MC-10 direct summary | ✅ | ✅ | ✅ | FULL（observed-pi-direct-summary） | ✅ |

9/10 FULL；usageAvailability 枚举 present / not_correlated /
projection_unavailable（v1）；无 unknown/ledger_expired（不声称无法证明的事）。

## Trace Explorer Backend

- listTraces（§二十八）：filter 语义 = 「trace 内至少存在一条符合 filter 的
  call」（JOIN model_calls + GROUP BY trace），origin 是 trace 自身维度；
  keyset 分页。
- getTraceDetail（§三十/三十一）：roots（parent=NULL；parent 指向 trace 外/
  不存在 → orphanParent=true 显式标记，不偷偷变 root）/ edges（parent 存在）/
  orphanEdges（missingParentCallId）/ cycle 检测 = parent 指针 functional
  graph 三色迭代染色（O(n)、不递归、**无根纯环也能检出**）→ graphIntegrity=
  degraded（不 crash；针对损坏/旧版本/partial write/手工修改）；usage
  aggregate + payload completeness summary + drop/health context。
- getCallDetail（§三十二/三十三）：trace summary + parent/child refs + attempts
  + usage + payload record metadata（§三十四：metadata 与 body 分离，正文不
  inline）。**attempt ≠ provider request**（§三十三）：MC-06 codex 401 =
  1 call + 2 attempts + 2 provider_request（ordinal 1/2）+ 2 provider_response
  + 1 semantic_request + 1 semantic_response（测试锁定不折叠成 2 个 logical call）。

## Payload Retrieval

- getPayloadRecord(id)（§三十五）：只允许 exact identity retrieval；无 FTS/
  contains search（§一百二十二持续禁止）。返回 sanitized payload +
  semanticInputProvenance + providerRequestProvenance + sanitization +
  visibility/fidelity。
- Fail safe（§三十六）：JSON.parse 失败 → contentState=corrupt（不 500、不返回
  raw malformed string）；OPAQUE/UNAVAILABLE → contentAvailable=false +
  contentState=opaque_or_unavailable（payload=null，不冒充空对象——即使磁盘
  被手工塞值也不返回）。
- Query service 不读 blob 文件（§八十五）；blob raw route 不做（Phase 9）。

## Persistence Settings / Control Plane

- preference namespace `model_observability`（§五十二）：{enabled,
  persistTraceMetadata, persistPayloads, persistBlobs, retention:{traceDays,
  payloadDays, blobDays}}——days（用户语义），转 policy 才 ×DAY_MS。
- canonical normalizer 单一来源（§五十三）：lib/llm/model-observability-
  preferences.ts；PreferencesManager / engine startup / coordinator 共用。
  PreferencesManager 落盘**原始意图**（raw merge，未表达字段不落盘），语义
  归一在读取侧——disabled 派生 false 不固化，关掉再打开不丢开启默认。
- 安全默认（§六十一）：enabled=false；开启后 trace=true、payload=false、
  blob=false（额外显式 opt-in；persistBlobs ⊆ persistPayloads）。
- Startup（§五十四/五十六）：engine install 移到 PreferencesManager 创建之后
  （audit 决策：单一 parser，无第四套 fs.readFileSync 解析）；优先级 =
  CompositionRoot 显式 option（enabled=true）> 用户 preference；重启自动生效。
- Runtime（§五十七～六十）：engine.setModelObservabilitySettings = normalize →
  persist desired → close 旧 handle（5s bounded）→ install 新 → invalidate
  query reader → 返回 desired+effective+queryHealth；disable 只停新记录，
  **绝不删 observability.sqlite/blobs**；reconfigure 不泄漏旧 sink/observer
  （close uninstall 恢复先前注册对象）；query facade mtime/size 失效重开。
- desired ≠ effective（§五十九）：schema_newer 等 → effective.status=disabled
  + reasonCode，不返回假 success。Settings API 返回
  cryptographicallyEncryptedAtRest=false（§六十二：filesystem permissions ≠ 加密）。
- §三十八：persistTraceMetadata=true && persistPayloads=false 时 call end 写
  payload_availability='not_captured'（仅 NULL 时；v1 历史 NULL 不回填）。
- §一百一十六：开启 payload persistence 后**绝不**从 session history/memory/
  persona 重建过去 Prompt——历史没有 capture 就是没有（测试锁定）。

## Security Boundary（Phase 8 Security Matrix）

| Endpoint | Data Sensitivity | Required Principal/Scope |
| --- | --- | --- |
| GET /api/model-observability/health | 统计（无正文） | STUDIO_OWNER（显式登记） |
| GET /api/model-observability/settings | 配置（无正文） | STUDIO_OWNER |
| PUT /api/model-observability/settings | 开启永久 recording | **LOCAL_ONLY** |
| POST /api/model-observability/query/calls | call metadata | STUDIO_OWNER |
| POST /api/model-observability/query/traces | trace metadata | STUDIO_OWNER |
| POST /api/model-observability/query/aggregate | 聚合 | STUDIO_OWNER |
| GET /api/model-observability/calls/:callId | call metadata + attempts | STUDIO_OWNER |
| GET /api/model-observability/traces/:traceId | trace graph metadata | STUDIO_OWNER |
| GET /api/model-observability/calls/:callId/payloads | payload metadata（无正文） | STUDIO_OWNER |
| GET /api/model-observability/payloads/:recordId | **Prompt/Response 正文** | **LOCAL_ONLY** |
| POST /api/model-observability/export | 可能含正文 | **LOCAL_ONLY** |

依据（§六十七～七十）：metadata 与既有 /api/usage/llm（STUDIO_OWNER）同级；
正文/开启记录/导出是更高敏感面，远程 principal（含远程 owner）默认不可；
未认证全拒；前缀内未登记 verb fail closed（DELETE → LOCAL_ONLY deny）。
复杂 query 用 POST JSON body（多值 filter + groupBy + cursor + dateBucket；
read-only operation，§六十五）。absent store → 404 not_initialized（§九十三
No-Store UX，非 500 ENOENT）；query 不隐式创建 store（§九十二，测试锁定）。

## Export Contract

- 独立版本 `MODEL_OBSERVABILITY_EXPORT_SCHEMA_VERSION=1`（与 SQLite
  user_version 各自演化，§七十四）。
- 默认 metadata-only（includePayloads=false / includeBlobs 无此选项）；
  includePayloads=true 只导 **Sanitized Payload Store** 内容；**没有 includeRaw**
  （系统不存在 raw payload store，§七十六）；blob 只导 descriptor/metadata +
  blobIds，绝不 base64 bytes（§七十七）。
- 复用统一 Filter Contract（§七十八：NormalizedModelObservabilityQuery）。
- JSONL streaming（§七十九/一百一十九）：manifest 首行（exportSchemaVersion/
  exportedAt/includePayloads/storageSchemaVersion/totalCalls/backfillSource/
  dataCompleteness）+ 每 logical call 一行 bundle {call, trace summary,
  attempts, usage, payloads}；async generator 按 keyset 页迭代（200/页）。
- Bounded（§八十一）：maxCalls 默认 50k、上限 100k；预 count 超限 → 413
  limit_error（不 OOM、不静默截断）。毒丸纪律（§一百一十八）：只读 sanitized
  store；OPAQUE/UNAVAILABLE 原样保留。

## Query Performance

- EXPLAIN QUERY PLAN 验证（§四十七，测试锁定）：date（idx_model_calls_started）/
  trace（idx_model_calls_trace）/ provider+model（idx_model_calls_model）/
  subsystem+operation（idx_model_calls_subsystem）/ session（idx_model_calls_session）/
  agent（idx_model_calls_agent）/ task（idx_model_calls_task）/ status
  （idx_model_calls_terminal）/ conversation（idx_model_calls_conversation，v2 新增）
  全部走 index。
- 不为所有可能 filter 建 index（§四十八）：api/surface/trigger/conversation_type/
  session_path/call_purpose/input_shape/provenance_precision 等走顺序扫
  （retention 有界行集，audit Q3 决策）。
- 防 N+1（§四十六）：call page 50 = 1×calls + 1×attempts(IN) + 1×payload
  summary(IN) + 1×usage(IN)。
- 10k calls 宽松性能 guard（§一百二十一：page+filter+aggregate < 10s，不做
  严格 wall-clock）。

## Known Query Gaps

1. percentile（p50/p95/p99）未做（§四十二）。
2. trace 自身时间窗 filter（first_seen/last_seen 区间）未做（本轮 call 级 join 语义已覆盖产品需求）。
3. blob raw retrieval / blob HTTP route 未做（Phase 9 配 UI/access control 设计）。
4. usageAvailability 无 unknown 档（三态：present/not_correlated/
   projection_unavailable——代码事实能证明的极限）。
5. 部分维度 filter 无 index（顺序扫，见 Query Performance）。
6. prompt/response/reasoning/blob FTS 持续禁止（§一百二十二/一百二十三）。

## Next Phase

Phase 9：Usage Observatory UI——Usage 页面重构（Unified Filter Bar + Group By +
Metrics Dashboard + Call Ledger + Trace Explorer + Prompt/Response Inspector +
Export UI），全部消费 Phase 8 API；不改 Query Contract。
