# MODEL_OBSERVABILITY_QUERY_AUDIT.md — Phase 8 Query & Control Plane 架构审计

> 编码前审计（任务书 §四）。基线：`feature/model-call-observability` @ `b1c19a18`
> （Phase 7 功能树 `bfde47bc`）。当前代码是唯一事实源；本文件回答任务书十二问，
> 并给出 Usage Correlation Matrix / Query Dimension Matrix / Security Matrix 的
> 编码前基线（最终矩阵在 OBSERVABILITY_IMPLEMENTATION_NOTES.md 维护）。

## Q1. 当前 SQLite schema 中哪些字段可以直接 Filter

`model_calls`（Phase 7 DDL，全部为独立列）：`call_id` / `trace_id` /
`parent_call_id` / `provider` / `model_id` / `api` / `subsystem` / `operation` /
`surface` / `trigger` / `attribution_kind` / `session_id` / `session_path` /
`conversation_id` / `conversation_type` / `agent_id` / `child_agent_id` /
`child_session_id` / `child_session_path` / `task_id` / `call_purpose` /
`started_at` / `ended_at` / `terminal_status` / `input_shape` /
`provenance_precision` / `persistence_completeness` / `interrupted_by_restart` /
`payload_availability`。

`traces`：`trace_id` / `origin` / `first_seen_at` / `last_seen_at`。

**不可 filter 的维度**（任务书 §十八要求里有、schema 无对应事实列）：
`hasPayload`（可由 payload_availability + EXISTS payload row 派生，见 §三十七
真相枚举）；`attemptCount` / `providerRequestCount`（列表项 summary，非 filter
列——本轮不提供这两个 filter）。`payloadAvailability` 的 NULL 语义必须保留为
`unknown`，不得折叠（§三十七）。

## Q2. 哪些字段已有 index

Phase 7 已建 18 个 index。与 query 直接相关：

| Index | 覆盖 filter/排序 |
| --- | --- |
| `idx_model_calls_started(started_at)` | since/until、默认排序 |
| `idx_model_calls_trace(trace_id)` | traceId、trace drill-down |
| `idx_model_calls_model(provider, model_id)` | provider、modelId、provider+model |
| `idx_model_calls_subsystem(subsystem, operation)` | category/subsystem、operation |
| `idx_model_calls_terminal(terminal_status)` | terminalStatus |
| `idx_model_calls_attribution_kind(attribution_kind)` | attributionKind |
| `idx_model_calls_session(session_id)` | sessionId |
| `idx_model_calls_agent(agent_id)` | agentId |
| `idx_model_calls_task(task_id)` | taskId |
| `idx_traces_last_seen(last_seen_at)` | trace 列表排序 |

payload/attempt/blob 侧：`idx_model_attempts_call`、`idx_payload_records_call`
(+call_kind / attempt / call_ordinal)、`idx_payload_blob_refs_blob`、
`idx_blob_objects_state/created`（detail / batch 读取用，Step 46 防 N+1 的基础）。

## Q3. 哪些用户 Filter 当前没有 index

无 index：`api`、`surface`、`trigger`、`conversation_id`、`conversation_type`、
`session_path`、`child_agent_id`、`child_session_id`、`call_purpose`、
`input_shape`、`provenance_precision`、`interrupted_by_restart`、
`parent_call_id`、`ended_at`。

决策（§四十八：不为所有可能 filter 建几十个 index）：v2 migration 只新增
**`idx_model_calls_conversation(conversation_id)`**（conversation 是产品核心
group-by 维度——bridge/channel 会话统计）；其余保持无 index：retention 有界
（默认 180d）行集上 SQLite 顺序扫配合已有窄 index 足够，且 EXPLAIN QUERY PLAN
验证（Step 47）只针对核心 fixture（date / provider+model / subsystem / session /
task / trace——全部命中既有 index）。实测不够再单独 migration。

## Q4. Usage Ledger 和 Observatory 如何通过 modelCallId 关联

- Usage Ledger entry：`entry.metadata.modelCallId`（`metadata.modelCallId` /
  `metadata.traceId` / `metadata.parentCallId` 三元组，各调用点显式写入）。
- Observatory：`model_calls.call_id`（observer `recorder.callId`，调用点与
  ledger metadata 写入点持有同一 recorder/identity 对象）。
- 关联写入点（`grep modelCallId`）：`core/session-coordinator.ts:585`、
  `core/bridge-session-manager.ts:343`（MC-01 WeakMap 补账）、
  `lib/llm/cache-preserving-compaction-agent-run.ts:419`（MC-02）、
  `core/llm-client.ts:775`（MC-04）、`lib/llm/provider-client.ts:348`（MC-05）、
  `core/media/image-task-runner.ts:520`（MC-06/07）、
  `core/media/universal-media-manager.ts:879`（MC-08）、
  `core/speech-recognition-service.ts:494`（MC-09）、
  `lib/llm/observed-pi-direct-summary.ts:153`（MC-10）。
- 无任何时间/modelId 顺序猜测关联（§十三：无 modelCallId 不投影）。

## Q5. MC-01～MC-10 当前哪些路径具有 Usage Ledger correlation

| MC Path | Observer 入口 | Durable Call | Ledger correlation | 依据 |
| --- | --- | --- | --- | --- |
| MC-01 Pi Chat | stream observer | ✅ | **FULL** | message_end 补账 WeakMap（session-coordinator + bridge-session-manager） |
| MC-02 AgentRun compaction | runner isolatedStreamFn | ✅ | **FULL** | runner ledger.start metadata |
| MC-03 native compaction | streamFn isCompacting 分支 | ✅ | **NONE** | ledger entry 不带 modelCallId（Phase 4 审计遗留 gap，本轮如实标注） |
| MC-04 callText | llm-client | ✅ | **FULL** | llm-client start metadata |
| MC-05 probe | provider-client | ✅ | **FULL** | probe ledger.start spread |
| MC-06 image HTTP | image-task-runner | ✅ | **FULL** | runSubmitInBackground ledger.start |
| MC-07 CLI (jimeng-cli) | image-task-runner（同 MC-06） | ✅ | **FULL** | 同一 ledger.start 位点 |
| MC-08 video HTTP | universal-media-manager | ✅ | **FULL** | video ledger.start spread |
| MC-09 speech | speech-recognition-service | ✅ | **FULL** | accounting ledger.start spread |
| MC-10 direct summary | observed-pi-direct-summary | ✅ | **FULL** | summary ledger.start metadata |

结论：9/10 FULL、1/10（MC-03）NONE。**不能假设 10/10**（§八）。

## Q6. Usage Ledger 5000 条 retention 对长期 Observatory analysis 的缺口

Ledger（`lib/llm/usage-ledger.ts`）：`DEFAULT_MAX_ENTRIES=5000`、整文件原子重写、
`list()` 内存过滤、`nextCursor` 恒 null。Observatory 默认 retention 180d。若 query
时才内存 join ledger：>5000 条之后**老 call 的 token/cost 永久消失**（call 在、
accounting 无）。因此 Phase 8 建 durable projection（`model_call_usage`）：
ledger append（`llm_usage` event）→ live ingestion；Ledger 仍是 accounting truth
source（§十二），projection 只是 read-optimized 副本，随 trace retention 删除（§十六）。

## Q7. persistence disabled 时是否还能读取历史 observability.sqlite

**当前不能**：`installModelObservabilityPersistence` disabled → 无任何 read
原语可达（trace/payload store 读原语只挂在 active handle 内）。Phase 8 建立
`openModelObservabilityReadDatabase()`：fileMustExist + readonly + query_only，
DB 不存在 → `queryStatus="absent"`（§九十三），绝不创建文件（§九十二）。

## Q8. server route security 对敏感 Payload 应采用什么权限

现状：`/api/usage/llm` GET=STUDIO_OWNER（route-security.ts:117）；`/api/*`
fallback=STUDIO_OWNER。Phase 8 决策（Security Matrix，§六十七～七十）：

| Endpoint | 敏感度 | 权限 |
| --- | --- | --- |
| `GET /api/model-observability/health` | 无正文统计 | STUDIO_OWNER（显式登记） |
| `GET /api/model-observability/settings` | 配置（无正文） | STUDIO_OWNER |
| `PUT /api/model-observability/settings` | 开启永久记录 | **LOCAL_ONLY** |
| `POST .../query/calls` / `query/traces` / `query/aggregate` | call/trace metadata | STUDIO_OWNER |
| `GET .../calls/:callId`、`.../traces/:traceId`、`.../calls/:callId/payloads` | metadata（无正文） | STUDIO_OWNER |
| `GET .../payloads/:payloadRecordId` | **Prompt/Response 正文** | **LOCAL_ONLY** |
| `POST .../export` | 可能含正文 | **LOCAL_ONLY** |

理由：metadata 与既有 Usage Ledger 同级（owner 可见）；正文/开启记录/导出是
更高敏感面，远程 principal（含远程 owner）默认不可。**全部显式登记进
route-security.ts 并测试**（§六十七：不吃 fallback）。

## Q9. PreferencesManager 如何持久化 observability policy

现状：`{userDir}/preferences.json`（= `{lingxiHome}/user/preferences.json`），
PreferencesManager 内存 cache + `savePreferences` 原子写；get/set 成对、无
observability key。Phase 8：新增 `model_observability` namespace（§五十二），
集中 canonical normalizer（§五十三）：`lib/llm/model-observability-preferences.ts`
的 `normalizeModelObservabilityPreferences()`——PreferencesManager、engine startup、
coordinator 共用一份，禁止多套 default。retention 用 days（用户语义），
转换为 policy 时才 ×86400000。

## Q10. Engine 如何安全地动态 install/uninstall persistence

现状：Phase 7 handle 已有 `close()`（flush+uninstall+close DB，幂等）与
`uninstall()`；engine dispose 5s bounded close。动态 reconfigure（§五十七）=
close 旧 handle → `installModelObservabilityPersistence` 新 policy → 重新 wire
accounting consumer → invalidate query reader。Query service 是独立 read-only
连接（WAL 并发读），不共享 writer handle，reconfigure 后 lazy reopen（§九十/九十一）。

## Q11. schema v1 是否必须升级到 v2

**必须**。v1 无 accounting projection 表；`model_call_usage` 是新 durable 表
（§十/十一），外加 `idx_model_calls_conversation` index。migration 显式、单事务、
失败自动 rollback（Phase 7 `migrateModelObservabilitySchema` 契约扩展 case 1→2）。
read service 同时支持 v1/v2（§七）：v1 下 accounting availability=
`projection_unavailable`，Trace/Call/Payload 照常可读，**读取绝不迁移**。

## Q12. Query Service 用独立 read-only connection 还是复用 writer

**独立 readonly WAL connection**（§九十）：

- writer（coordinator）事务边界单一，不可共享给 query 层（query 层 read-only
  纪律 §一百二十七；better-sqlite3 同连接上 read/write 混合并发无意义）。
- WAL 模式下 readonly reader 与 writer 并发安全（audit Q3，Phase 7 已定）。
- reader 契约：`fileMustExist` + `readonly` + `query_only=ON` + busy_timeout
  5000 + 不 CREATE/migrate/VACUUM/改 user_version（§六）；DB 不存在 → `absent`。
- 生命周期：query service 持稳定 facade；reconfigure/DB 重建 → invalidate →
  下次查询 lazy reopen（§九十一，测试锁定）。
- 例外：query 不 flush writer（§五十），读 committed durable state，health 用
  pending queue counts 表达「尚未 commit」。

## 补充决策（编码前锁定）

1. **Engine 构造序**（§五十六）：observability install 从 PreferencesManager
   创建之前**移到紧随其后**（两者都在一切模型调用之前；`_prefs` 之后 install
   即可读取已保存 policy）。优先级：CompositionRoot 显式 option（enabled=true）
   > 用户 preference。避免第四套 parser（B 方案否决）。
2. **live ingestion 挂点**（§十四）：engine constructor 在 `createUsageLedger`
   之后把 `llm_usage` consumer（`engine.subscribe`）接到 coordinator 的
   accounting queue——复用 ledger append → event 通道，不改任何模型调用点。
3. **backfill**（§十五）：首次 v2 启用（meta key 标记）时对当前 bounded ledger
   的 ≤5000 条带 modelCallId entry 做幂等 upsert；报告标注
   `backfillSource: "bounded_usage_ledger"`，不声称完整历史。
4. **category 语义**（§十九）：`category` ≡ `subsystem`（与旧 Usage UI
   `entry.source?.subsystem` 一致），另提供 `callPurpose` / `operation` 独立维度。
5. **日期 bucket**（§四十三）：`bucket="day"` + `utcOffsetMinutes`（SQL
   `strftime('%Y-%m-%d', started_at, printf('%+d minutes', ?))`），server
   timezone 不入局；since inclusive / until exclusive 全接口统一（§四十四）。
6. **not_captured 标记**（§三十八）：v2 coordinator 在 persistTraceMetadata=true
   且 persistPayloads=false 时，call end 后写 `payload_availability='not_captured'`
   （仅 NULL 时）；旧 v1 NULL 不回填。
7. **projection 内容边界**（§十一）：只存 numeric accounting + status + identity；
   **不存** ledger `error.message`（可能来自 Provider Error，违反 Observable
   Metadata Safe Contract）；`error.name` 也不存（model_calls.error_name 已有
   observer 侧事实）。
8. **旧 API/UI 零改动**（§八十三/八十四）：`/api/usage/llm`、UsageTab、
   UsageLedgerSection 本轮不动（除公共类型兼容性最小修复）。
