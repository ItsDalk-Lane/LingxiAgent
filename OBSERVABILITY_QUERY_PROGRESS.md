# OBSERVABILITY_QUERY_PROGRESS.md — Phase 8 进度（Unified Query & Control Plane）

> 本文件是 Phase 8 的跨会话断点文件：新会话先读本文件 +
> MODEL_OBSERVABILITY_QUERY_AUDIT.md + OBSERVABILITY_STORAGE_PROGRESS.md，再动代码。
> 基线：`feature/model-call-observability`；Phase 7 功能树 `bfde47bc`（seal `b1c19a18`）。

## Current phase

**Phase 8 完成。** 开发顺序按任务书 §一百二十九执行：
Step 0（基线 b1c19a18）→ 1-2（MODEL_OBSERVABILITY_QUERY_AUDIT.md 十二问 +
MC-01～10 correlation 审计：9/10 FULL、MC-03 NONE）→ 3-5（query-types 契约 +
schema v2 + v1→v2 migration）→ 6-7（accounting projection + bounded backfill +
read-only opening）→ 8-14（call list/keyset/trace list/aggregate/trace detail/
call detail/payload exact/health）→ 15-17（preferences + startup loading +
runtime reconfigure）→ 18-20（HTTP routes + route-security + streaming export）
→ 21-25（7 测试文件 53 用例 + 既有 store-schema/security 扩展）→ 26-31
（scanner 站点登记 + fingerprint compatible repin sha256:b0712be2… +
typecheck ×3 + eslint 0 error + lint:boundary + targeted + full npm test）→
32-33（文档 + seal）。

## Schema version

`MODEL_OBSERVABILITY_SCHEMA_VERSION = 2`。v2 新增：`model_call_usage` 表
（accounting projection，PK=model_call_id 幂等 upsert）+
`idx_model_call_usage_status` + `idx_model_calls_conversation`。v1→v2 单事务
migration（只新增，v1 行不动）；migration 失败 → rollback + disabled handle
（migration_failed）。read side `supportedReadVersions=[1,2]`：v1 库不迁移可读，
accounting availability=projection_unavailable。

## Accounting projection status

完成。链路：Provider → Usage Ledger（truth）→ model_call_usage（read-optimized
projection）。live ingestion 复用 ledger append → `llm_usage` 事件
（engine constructor 在 usageLedger 创建后 `initializeAccounting` wire，
不改任何模型调用点）；bounded ledger（≤5000 条）best-effort backfill 只做一次
（meta key `usageLedgerBackfillCompletedAt`，报告标 `bounded_usage_ledger`，
不声称完整历史）；只存 numeric+status+identity，**error.message/name 不入库**；
无 modelCallId 不投影（MC-03 NONE 如实保留）；retention 随 trace 删除 +
maintenance 清 orphan usage 行；Usage Ledger 自身 5000 retention 不变。

## Query filter coverage

Call filter（§十八全量）：since/until（inclusive/exclusive）、traceId/
parentCallId/callId、provider/modelId/api/subsystem(category≡alias)/operation/
surface/trigger/callPurpose/terminalStatus(含 incomplete 伪值)/attributionKind/
sessionId/sessionPath/conversationId/conversationType/agentId/childAgentId/
childSessionId/taskId/inputShape/provenancePrecision/payloadAvailability
(present/unknown 由 payload row 存在性派生)/interruptedByRestart/hasPayload。
字段内 OR（≤32 值）、字段间 AND；unknown field/invalid enum/invalid date →
显式 400。维度全部闭集映射 SQL 列、值全绑定（注入测试锁定）。

## Pagination

Call：keyset `started_at DESC, call_id DESC`，NULL started_at 稳定最后
（跨 NULL 边界的 keyset 条件专门修正：NULL 区域不受非空域 call_id 上界约束）。
Trace：独立 keyset `last_seen_at DESC, trace_id DESC`。cursor opaque
（base64url JSON {v,kind,fp,s,c/t}），fingerprint=normalized query + origin
的 sha256 前 16 hex——filter 改变 → invalid_cursor（400），损坏/伪造/过长/
换 filter 复用全部 fail-safe。default limit 50 / max 200。

## GroupBy coverage

date（bucket=day + utcOffsetMinutes 显式，server timezone 不入局；SQL
`strftime('%Y-%m-%d', started_at, printf('%+d minutes', ?))`）/ provider /
model（provider+modelId 复合 key）/ category（≡subsystem）/ operation /
callPurpose / status / attributionKind / session / conversation / agent / task /
inputShape / provenancePrecision；多级 ≤3 维。指标（§四十一全集）：callCount/
traceCount/ok/error/aborted/incomplete/attemptCount/durationObserved/Total/
Average（julianday 算术）/usageCovered/usageMissing/input/output/reasoning/
cacheRead/cacheWrite/totalTokens/costTotal/cacheHitCount/cacheObservedCount。
聚合全部 SQLite 内完成（LEFT JOIN model_call_usage）；percentile 未做（§四十二）。

## Trace detail / payload retrieval

Trace detail：roots（parent=null 或 orphanParent 显式标记）/ edges /
orphanEdges / graphIntegrity（functional-graph 三色迭代染色检测环——含无根
纯环，不递归不 crash）/ usage aggregate / payload completeness summary /
drop counters。Call detail：trace summary + parent/child refs + attempts
（attempt ≠ provider request：MC-06 codex 401 = 1 call + 2 attempts + 2
provider_request ordinals + 2 provider_response + 1 semantic_request/response，
测试锁定）+ payload metadata（无正文）。Payload exact retrieval：只按
record id；OPAQUE/UNAVAILABLE → contentAvailable=false +
contentState=opaque_or_unavailable（不冒充空对象）；JSON 损坏 → corrupt
（不 500、不返回 raw string）。FTS（prompt/response/reasoning/blob）全部禁做。

## Settings / control plane

- preference：`preferences.json` `model_observability` namespace
  {enabled, persistTraceMetadata, persistPayloads, persistBlobs, retention:
  {traceDays,payloadDays,blobDays}}。canonical normalizer 单一来源
  （lib/llm/model-observability-preferences.ts）；PreferencesManager 落盘
  **原始意图**（raw merge，未表达字段不落盘），语义归一在读取侧——避免
  disabled 派生 false 被固化（关掉再打开不丢 §六十一 默认）。
- 默认：enabled=false；开启后 trace=true、payload=false、blob=false
  （payload/blob 必须额外显式 opt-in）。
- startup：engine constructor 在 PreferencesManager 之后 install（audit
  补充决策 1）；显式 CompositionRoot option（enabled=true）> 用户 preference。
  重启后 preference 自动生效（PreferencesManager 重读测试锁定）。
- runtime：engine.setModelObservabilitySettings（normalize → persist →
  close 旧 handle（5s bounded）→ install 新 → invalidate query reader →
  返回 desired+effective+queryHealth）；disable 只停新记录、绝不删
  observability.sqlite/blobs；desired ≠ effective（schema_newer 等
  reasonCode 显式）。engine.getModelObservabilitySettings 返回
  cryptographicallyEncryptedAtRest=false（§六十二）。
- §三十八：persistTraceMetadata=true && persistPayloads=false 时 call end
  标 payload_availability='not_captured'（仅 NULL 时）；v1 历史 NULL 不回填。
- 不回填 Prompt（§一百一十六）：开启 payload persistence 后旧 call 不从
  session history/memory 重建（测试锁定）。

## Query health

ModelObservabilityQueryHealth（query side）+ engine.getModelObservabilityHealth
（recording+query 合并）：queryStatus ready/absent/unavailable/degraded、
schemaVersion、accountingProjectionAvailable、oldest/newestCallAt、
call/trace/payloadRecord/usageProjection counts、三队列 queue counts、
drop/writeFailure/maintenance counters（持久化到 observability_meta，
disabled 期从 meta 恢复）、interruptedByRestartCalls、atRestEncryption=false。
Query 不 flush writer（§五十）；absent ≠ 500（§九十三 No-Store UX）。

## Export

ModelObservabilityExporter（lib/llm/model-observability-export.ts）：
独立 `MODEL_OBSERVABILITY_EXPORT_SCHEMA_VERSION=1`（与 user_version 分离）；
默认 metadata-only；includePayloads=true 只导 sanitized store（无 includeRaw）；
blob 只导 descriptor/metadata（无 bytes）；JSONL streaming（async generator
按 keyset 页迭代）；maxCalls 默认 50k/上限 100k，超限 → 预检 413 limit_error；
manifest 首行含 exportSchemaVersion/exportedAt/includePayloads/
storageSchemaVersion/totalCalls/backfillSource/dataCompleteness。

## Security

| Endpoint | 敏感度 | 权限 |
| --- | --- | --- |
| GET health / settings | 无正文统计/配置 | STUDIO_OWNER（显式登记 route-security.ts） |
| PUT settings | 开启永久记录 | LOCAL_ONLY |
| POST query/calls / traces / aggregate | call/trace metadata | STUDIO_OWNER |
| GET calls/:id / traces/:id / calls/:id/payloads | metadata（无正文） | STUDIO_OWNER |
| GET payloads/:recordId | Prompt/Response 正文 | LOCAL_ONLY |
| POST export | 可能含正文 | LOCAL_ONLY |

未认证全拒；前缀内未登记 verb fail closed（DELETE settings → LOCAL_ONLY deny）。
HTTP surface：server/routes/model-observability.ts（open-root 挂载，与 usage
route 分离）；复杂 query POST JSON body + normalizeModelObservabilityQuery*
严格 400；absent → 404 not_initialized（非 500）。

## Tests

新增 7 文件 53 用例：schema-v2 6（fresh v2/v1→v2 保真/rollback/unknown
higher/read-only v1/absent 不建文件）/ accounting-projection 9（含 live
ingestion + retention）/ query 19（filters/AND-OR/分页/同时间戳/NULL started_at/
cursor tamper/注入/group-by/coverage/trace graph/call detail/opaque/corrupt/
时区 bucket/EXPLAIN QUERY PLAN/10k perf）/ settings 7（preference 默认/
PreferencesManager 持久化/dynamic enable/disable/payload opt-in/生命周期/
absent）/ export 6 / http-route-security 扩展 3；store-schema 更新到 v2 期望。
旧 Usage API/UI 零改动（/api/usage/llm、UsageTab 原样）。

## Guards

typecheck ×3 绿；eslint 0 error；lint:boundary 绿（manifest 收录 7 新模块）；
scan-persistent-stores 61 stores（model-observability-db siteRules +
read-database 只读站点）；fingerprint compatible repin
（sha256:b0712be2cd46a74b426092262d0060a2c28a45fc98a73939d7c3c525eced9025）；
cli-runtime-closure 重算。

## Known query gaps

1. percentile（p50/p95/p99）未做（§四十二：本轮 count/sum/avg 足够）。
2. Trace list 的 filter 维度=call 级 join（语义=「trace 内至少一条 call 命中」），
   不做 trace 自身时间窗口 filter（first_seen/last_seen 区间）。
3. Blob raw retrieval / blob HTTP route 未做（Phase 9 配 UI preview 设计）。
4. usageAvailability 无 `unknown` 档（v2 下投影行存在性与 ledger entry 可判
   present/not_correlated；v1 → projection_unavailable）。
5. conversationType/surface/trigger/callPurpose 等维度 filter 走顺序扫
   （无 index；retention 有界行集上可接受，audit Q3 决策）。
6. 多进程并发写未做应用级仲裁（Phase 7 已知约束不变）。

## Seal

功能提交后 VERIFIED_SOURCE_SHA 按仓库既有机制推进到本轮验证树（单独 audit commit）。
