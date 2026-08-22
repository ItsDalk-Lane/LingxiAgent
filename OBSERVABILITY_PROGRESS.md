# Model Call Observer — 进度（已完成）

第一轮（Phase 1 契约 + Phase 2 文本运行时）：基线 main @ e62bb535，Step 0–9 完成。
第二轮（Phase 2.5 安全收口 + Phase 3A MC-05～09 接入 + Phase 3B 控制面分离）：
基线 feature/model-call-observability @ 9dfde99a，全部完成。
第三轮（Phase 3.5 残余闭合 + Phase 4 Trace）：基线 @ e795b9ef，全部完成。

## 第三轮完成项

- Phase 3.5 残余闭合（修正 9 → 10 paths，MODEL_CALL_CLOSURE_DELTA.md）：
  - diary temporary summary（Pi generateSummary 直发 completeSimple）确认为
    生产可达独立旁路 → **MC-10**；facade re-export 包装为 Observed direct
    summary（lib/llm/observed-pi-direct-summary.ts），复用既有
    Recorder/Observer/Identity；diary 传 observerContext 获得归属 + ledger
    entry（metadata 三元组关联）。attemptVisibility=logical_boundary、零伪造
    wire 事件。真实 Pi 链测试（stub fetch 伪 SSE Provider）锁定。
  - session-snapshot-side-task-runner completeSimple：仍无生产 caller →
    LATENT / NOT_CURRENTLY_REACHABLE（不制造生产行为）。
- Phase 4 Trace Contract（lib/llm/model-trace-scope.ts，AsyncLocalStorage）：
  - ModelTraceScope（traceId/origin/refs/causalParentCallId/lastCallId）+
    统一身份解析 resolveModelTraceContext（explicit → scope → singleton；
    traceId 恒非空、parent 无事实即 null）。
  - Ingress 接线：desktop user_turn / bridge / phone / slash（inherit-or-mint）；
    automation cron、diary、memory daily+compile、dream、speech、probe、
    health check（force-new detach）；plugin/media（inherit-or-mint）；
    facade session.prompt 兜底 ingress（subagent 继承不 mint）。
  - 因果传播：stream observer 推进 lastCallId（loop 内 C2.parent=C1）；
    session-options 工具边界（全工具唯一收口）快照 causalParentCallId=产生
    toolCall 的那次调用 → 工具内 Vision/Approval/Media/Subagent/callText 自动
    继承（并行工具双双 parent=C1，子 scope 冻结快照互不覆盖）。
  - Ledger trace metadata：MC-04/05/06/07/08/09/10 + MC-02 spread 三元组；
    MC-01 message_end 补账经 WeakMap（model-call-correlation.ts）补齐
    correlation（无侵入，对象 GC 自动回收）。
  - TestModelCallObserver 扩展 Trace Explorer（eventsForTrace/callsForTrace/
    childrenOf/rootsForTrace/callIdentity/assertTraceGraphValid）。
- 测试：新增 3 文件 35 用例（trace-scope 17 / propagation 14 场景 / diary 4）
  + 更新 calltext 契约测试；第一/二轮 96 回归全绿。
- 验证：typecheck ×3 绿；eslint 0 error；lint:boundary 绿（manifest 收录
  model-trace-scope/model-call-correlation/observed-pi-direct-summary）；
  cli-runtime-closure 复核；persistence fingerprint compatible repin；
  full npm test 通过。
- §九十终验反扫答案：**NO**（无生产可达 Model Call 缺 Observer lifecycle；
  10/10 路径覆盖）。

## Seal

本轮功能提交后，VERIFIED_SOURCE_SHA 按仓库既有机制推进到新验证树
（单独 audit commit：verified-source-sha.txt + matrix 文件）。

## Phase 5 — Semantic Input Provenance（2026-08-21 第四轮）

- Semantic Input Provenance Contract（lib/llm/semantic-input-provenance.ts）：
  统一 `ModelSemanticInputProvenance`（schemaVersion/inputShape/sections）；
  section 只含 category（24 值闭集）/role（7 值）/source（9 类型 + 安全逻辑 id，
  绝对路径与 URL fail-closed 拒绝）/locator（5 根 + path + UTF-16 [start,end) span，
  span=null 为 identity-only）/precision（exact/structural/opaque）。
  renderer `renderProvenancedText` 与调用方 join 字节级一致（空段语义保持）；
  sanitize gate fail closed（非法段丢弃、exact 必须可定位）。
- Sidecar：recorder 持有 per-call provenance（随 call GC，无全局 Map）；完整 map
  经事件 non-enumerable symbol 引用传递（JSON.stringify/安全门不可见），
  Observer 事件只带 summary（inputShape/provenancePrecision/sectionCount/
  去重 categories/opaqueCount）。rollup：exact/partial/opaque（无百分比）。
- MC-01：stream observer 在 streamFn 边界构造——session 冻结快照 runtime 前缀
  验证（startsWith(真实冻结对象)，非模板重建）→ 快照 sections exact（平移 0）；
  SDK 尾段（append+project_context+skills+cwd）单段 structural + skills/
  agentsFile identity-only（basename）；messages 按 role 分类（toolResult→
  tool_result；turn 内最后 user → current_user_input，依据 prompt ingress ALS
  turn 标记 + loop 只追加 assistant/toolResult 的 runtime 不变量；无标记不猜）；
  tools 逐项 tool_definition。快照 provenance 持久化为 v1 附加可选字段
  systemPromptProvenance（无内容副本；旧快照恢复 → structural 诚实降级）。
- MC-02：runner 在 isolatedStreamFn 按实际 providerContext 构造（system 整段
  structural；strict → task_instruction / repair → format_constraint 可区分；
  recovery placeholder toolResult → tool_result；observer 对未覆盖尾段扩展分类）。
- MC-03：isCompacting → system=structural task_instruction（SDK 常量不随包出口，
  镜像为手工副本非同源数据结构，不伪造 exact）；messages[0]=structural task_input。
- MC-04：callText 归一化 boundary（merge 完成 + normalizedMessages）——caller
  显式 provenance 随 system merge 同步 remap（span 平移 + index 重排）；未提供 →
  structural fallback；codex 空系统注入 DEFAULT_CODEX_UTILITY_INSTRUCTIONS →
  adapter_injected 段（注入点标记，§八十二）。length-contract 二次 repair 重建
  provenance（追加 format_constraint 段）。
- Utility callers 显式迁移（~20 call site）：fact extraction（timeContext/
  prevSnapshot/currentSummary 三段）、compile×4、rolling summary+repair、dream
  全阶段+两类 repair、approval（含二调 format_constraint）、vision×2（指令段/
  user request 段/图片 media_reference）、diary 终稿（≥5 chunk）、appearance、
  title/translate/activity×2/agent-id/description、rc-summary、install-skill、
  health check。多段拼接处全部改为构造点分段渲染；四处（compile/_compactLLM、
  rolling repair、install-skill、dream）带「渲染必须与原串字节一致否则回退无
  provenance」零漂移防御。
- MC-05..09：probe（固定消息 task_instruction exact）、image/video（media_prompt/
  media_reference locator 指参数位不含值；jimeng-cli-* → external_cli_media 形状）、
  speech（audio_input/language_hint）。
- MC-10：generateSummary facade 在参数边界构造三元组 provenance（messages 段级 +
  customInstructions/previousSummary parameters 寻址）。
- 审计：SEMANTIC_INPUT_PROVENANCE_AUDIT.md（Step 1 交付，Prompt Construction
  Matrix + caller 迁移清单 + Known Gaps）。
- 等价与安全：Agent system prompt golden fixture（改造前生成，zh/en 字节一致）；
  renderer Unicode（中文/emoji/代理对/ZWJ）span 单测；毒丸测试（TOP_SECRET×4
  JSON 序列化不可见）；payload 不变测试（callText 传/不传 provenance body 一致）；
  snapshot roundtrip + persona V1→V2；Trace/safety/control-plane 回归全绿。
- 验证：typecheck ×3 0 error；eslint 0 error；lint:boundary 绿（manifest 收录
  semantic-input-provenance(-payload)）；cli-runtime-closure 重算；persistence
  fingerprint compatible repin；full npm test 11776 通过。

## Phase 6 — Sensitive Payload Capture + Redaction + Provider-Wire Provenance（2026-08-22 第五轮）

**状态：完成。** 交付：

- 契约五模块：`lib/llm/model-call-payload-types.ts`（record/kind/visibility/
  fidelity/sanitization/资源上限/ProviderRequestProvenance）、
  `model-call-payload-redaction.ts`（copy-on-capture redactor：credential 键、
  协议专项路径 volcengine→user.uid、inline secret 正反例、URL/本地路径/二进制
  externalization、span offset remap）、`model-call-payload-capture.ts`（sink
  注册表 + capture session + ordinal 计数）、`provider-request-provenance.ts`
  （callText 四协议 mapping，构造时产生 + post-compat 校验降级）、
  `model-call-payload-testing.ts`（Test sink）。
- 通道架构：Observer（metadata，契约冻结）‖ PayloadCapture（正文，先统一
  Redaction 后入 sink，detached only）；生产默认 sink=NOOP，session=null 快路径
  （spy 锁定 redactor 不运行）；sink throw 不影响业务（callText/Pi/media/speech
  测试锁定）。
- 全路径接入：MC-04 四层 + 四协议 mapping（anthropic system/messages 重排、
  openai messages[0] 平移、responses/codex instructions/input、codex 空系统
  adapter_injected）；MC-01 streamFn context + before/after_provider_request
  hook（payload=最终 body，runtime_exact；response=metadata_only）；MC-02/03/10
  provider wire 显式 unavailable（options 无 onPayload 运行时判定 / summarizer
  无 hook）；google response hook 缺失显式 unavailable；MC-05 probe（"." 允许
  捕获；GET /models 0 record）；MC-06 ×7 + MC-08 agnes wire 层（构造点 body +
  响应解析点）+ codex 401 双 ordinal；MC-07 CLI 显式 opaque（argv/stdout 不进
  sink）；MC-09 ×4（audio externalize、Volcengine body credential 协议专项）。
- Recorder/Scope/Integration 扩展：recorder.payloadCapture handle（attachPayload
  Chat）、scope.payloadCapture（ALS 共享给 hooks）、observedProviderFetch capture
  描述符 + captureProviderHttpResponse helper。
- 审计：MODEL_CALL_PAYLOAD_CAPTURE_AUDIT.md（Step 1：四层边界矩阵、credential
  位置总表、Pi hook 实证、控制面清单、opaque 清单）。
- 验证：typecheck ×3 0 error；eslint 0 error；lint:boundary 绿（manifest 收录
  5 个新共享模块）；既有观测测试 14 文件 131 用例全绿无回归；新增 7 文件
  103 用例全绿；full npm test 见下。

## Phase 7 — Durable Model Observatory Storage（2026-08-22 第六轮）

**状态：完成。** 交付：单 SQLite（user_version=1）逻辑分表 traces/model_calls/
model_attempts/payload_records/blob_objects/payload_blob_refs/observability_meta +
外置 blobs 树；bounded 异步 coordinator（handler 只 enqueue，trace/payload/blob
三队列独立容量，overflow 显式计数并持久化跨 restart）；crash reconciliation 只标
interrupted_by_restart 不伪造终态；retention 六维度 policy + 集中 safe fallback
（trace 180d/payload 30d/blob 30d，payload 可先过期）；privileged Blob
Externalizer contract（Buffer/TypedArray/ArrayBuffer 可存，Blob/base64 诚实
externalized；atomic 写 + ref-count GC + orphan/missing recovery）；engine/server
真实生产 wiring（默认 disabled，显式 policy 开启，dispose 5s bounded flush）。
毒丸落盘字节级扫描（DB+wal+shm）零命中。Store Registry ×2 登记 + fingerprint
introspector + compatible repin（sha256:f3d6c1f9…）。新增 6 测试文件 44 用例；
既有观测 302 用例回归全绿；full npm test 11925 通过。详见
OBSERVABILITY_STORAGE_PROGRESS.md / MODEL_OBSERVABILITY_STORAGE_AUDIT.md /
OBSERVABILITY_IMPLEMENTATION_NOTES.md Phase 7 节（含 Durable/Completeness/
Retention 三矩阵与 At-Rest NO 结论）。

## Phase 8 — Unified Observatory Query & Control Plane（2026-08-22 第七轮）

**状态：完成。** 把 Phase 1～7 的 Durable Fact Store 提升为统一、可分页、可分组、
可追溯的 Read Model，并让 observability recording preference 第一次成为真正的
持久化产品配置：

- **统一 Query Contract**（lib/llm/model-observability-query-types.ts）：同一份
  Filter Contract 驱动 Call 列表 / Trace 列表 / Aggregate / Drill-down / Export；
  category ≡ subsystem（与旧 Usage UI 一致）；字段内 OR（≤32 值）字段间 AND；
  unknown field / invalid enum / invalid date / invalid cursor 显式 400；维度/排序
  全部闭集映射 SQL、值全绑定（注入测试锁定）。
- **Keyset pagination**：call（started_at DESC, call_id DESC，NULL 稳定最后——
  跨 NULL 边界条件专门修正）/ trace（last_seen_at DESC, trace_id DESC）各自独立
  cursor；cursor opaque 且与 normalized query fingerprint 绑定（filter 改变 →
  400 invalid_cursor）；default 50 / max 200。
- **Aggregate Group By 全在 SQLite 内完成**：date（day + 显式 utcOffsetMinutes，
  server timezone 不入局）/provider/model/category/operation/callPurpose/status/
  attributionKind/session/conversation/agent/task/inputShape/provenancePrecision，
  多级 ≤3 维；指标含 token/cache/cost/usage coverage；无 usage 的 call 仍计入
  callCount（coverage 诚实分列）。
- **Durable Accounting Projection**（schema v2 `model_call_usage`）：Provider →
  Usage Ledger（truth）→ projection 单向；live ingestion 复用 llm_usage 事件；
  bounded ledger（≤5000）幂等 backfill（标注 bounded_usage_ledger，不声称完整
  历史）；error.message/name 不入库；无 modelCallId 不投影（MC-03 NONE 保留）；
  retention 随 trace 删除 + orphan 清理；Ledger 自身行为不变。
- **Read side**：openModelObservabilityReadDatabase（readonly + fileMustExist +
  query_only，绝不创建/迁移；absent ≠ 500）；supportedReadVersions=[1,2]，v1
  历史库照常可查（accounting 标 projection_unavailable）；稳定 facade +
  mtime/size 失效重开（reconfigure 不持已关 handle）。
- **Trace Explorer 后端**：roots/orphanParent 显式、orphanEdges、functional-graph
  染色 cycle 检测（含无根纯环 → degraded 不 crash）；call detail 保留 attempt ≠
  provider request（MC-06 codex 401 = 1 call/2 attempts/2 ordinals 测试锁定）；
  payload 正文只在 exact retrieval 返回，OPAQUE/UNAVAILABLE/METADATA_ONLY 不升级，
  JSON 损坏 → corrupt 不 500。
- **Settings/Control Plane**：preferences.json `model_observability` namespace
  （enabled/persistTraceMetadata/persistPayloads/persistBlobs/retention days）；
  canonical normalizer 单一来源；默认 disabled、payload/blob 额外 opt-in；
  落盘原始意图（读取侧归一）；startup 自动加载（engine install 移到
  PreferencesManager 之后，显式 CompositionRoot option 优先）；运行中
  enable/disable/reconfigure（close 旧 handle → install 新 → invalidate reader）；
  disable 不删历史数据；desired ≠ effective 显式（schema_newer 等）；
  atRestEncryption=false 诚实暴露；not_captured 运行时标记（v1 NULL 不回填）。
- **HTTP surface**（server/routes/model-observability.ts，与 usage route 分离）：
  health/settings GET+PUT/query calls+traces+aggregate(POST JSON)/detail/payload
  metadata/payload exact/export(NDJSON streaming)；route-security 显式登记——
  metadata=STUDIO_OWNER，payload 正文/settings PUT/export=LOCAL_ONLY，未认证全拒，
  前缀内未登记 verb fail closed。
- **Export Contract**：独立 EXPORT_SCHEMA_VERSION=1；默认 metadata-only；
  includePayloads 只导 sanitized store（无 includeRaw）；blob 无 bytes；JSONL
  streaming 按 keyset 页；maxCalls 50k/上限 100k 超限 413。
- 旧 /api/usage/llm、Usage Ledger、Usage UI 零改动。

审计 MODEL_OBSERVABILITY_QUERY_AUDIT.md（十二问 + MC correlation：9/10 FULL、
MC-03 NONE）；进度 OBSERVABILITY_QUERY_PROGRESS.md。schema v1→v2 显式单事务
migration（只新增，rollback 安全）。新增 7 测试文件 53 用例 + 既有
store-schema/route-security/composition-boundary 扩展；typecheck ×3 / eslint
0 error / lint:boundary 绿（manifest 收录 7 新模块）；scanner 站点登记
（read-database 只读打开）；fingerprint compatible repin（sha256:b0712be2…）。
Phase 9（Usage Observatory UI：Unified Filter Bar + Group By + Metrics Dashboard +
Call Ledger + Trace Explorer + Prompt/Response Inspector + Export UI）待开始。
