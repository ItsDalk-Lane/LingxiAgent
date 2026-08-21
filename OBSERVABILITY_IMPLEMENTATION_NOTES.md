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
