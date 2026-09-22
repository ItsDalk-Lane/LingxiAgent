调查完成。以下为 12 项身份/事实的权威清单。

# 灵犀 Lingxi 核心事实身份：产生位置与写入者清单

## 1. session（用户会话 sessionId）

- **生成**：`/Users/study_superior/Desktop/Code/LingxiAgent/core/session-manifest/id.ts:5-9` — `generateSessionId()`，格式 `sess_${Date.now()36 补齐9位}_${randomBytes(10).hex(20字符)}`。唯一性铸造在 `core/session-manifest/store.ts:1112-1121`（`_generateUniqueSessionId`，10 次撞库重试），默认 idGenerator 注入在 store.ts:197。
- **写入者（权威）**：`SessionManifestStore`（`core/session-manifest/store.ts:181`）写 SQLite 表 `session_manifests`（DDL store.ts:220）+ `session_locator_history`（store.ts:254/425）+ `session_branch_heads`（store.ts:295/588）。DB 路径 `{LINGXI_HOME}/session-manifest.db`，由 `core/engine.ts:2038-2041` 打开，engine.ts:506 构造；`SESSION_MANIFEST_DB_USER_VERSION = 5`（store.ts:12）。业务侧创建入口是 `core/session-coordinator.ts:1775 createSession` → `_ensureBranchManifestForPath`（session-coordinator.ts:1849-1859）。
- **注意双身份**：Pi SDK 另有自己的 session 头 UUID：`node_modules/@earendil-works/pi-coding-agent/dist/core/session-manager.js:650`（`this.sessionId = options?.id ?? createSessionId()`，createSessionId=randomUUID 行28），且 **JSONL 文件名用的是 SDK UUID**（session-manager.js:668 `${fileTimestamp}_${sessionId}.jsonl`）。业务 `sess_` id 只活在 manifest DB；SDK UUID 仅作诊断 ref（session-coordinator.ts:5022-5029 明确区分）。
- **只读消费者**：engine `getSessionIdForPath`（engine.ts:495/696/813/833/868 多处注入）、`core/session-manifest/resolver.ts`、`lib/llm/model-observability-trace-store.ts:476`（trace 复用查找）、store-registry 各 identityContract 声明"sessionId 是持久身份，sessionPath 只是 locator"。
- **恢复**：JSONL 恢复走 `lib/session-jsonl.ts:188 readCurrentSessionBranch`（branch head 行 199-230）+ manifest locator 解析。

## 2. 用户消息 turn / message id

- **生成**：Session JSONL 的 entry id 由 Pi SDK 铸造：`session-manager.js:23` `generateId = randomUUID().slice(0,8)`（8 字符短 id），每条 user/assistant/toolCall/toolResult entry 都有 `id` + `parentId`（追加于 session-manager.js:770 `_appendEntry` 及 785-1105 各 append*）。Lingxi 侧不重新铸造。
- **写入者（权威）**：Pi SDK `SessionManager` 直接 appendFileSync 到 `agents/{agentId}/sessions/{sdkSessionId}.jsonl`（store-registry `session-jsonl` 条目，`shared/persistence/store-registry.ts:586-619`，ownerModule core/session-coordinator.ts）。Lingxi 修复/重写走 `core/session-jsonl-file.ts:298 writeSessionEntriesFile`（写前递增变更世代，见 `core/session-file-mutation-epoch.ts:39`）。
- **legacy**：无 id 旧文件在读取时合成 `legacy-line-N`（lib/session-jsonl.ts:96-102），混合 id/无 id 会抛错（103-108）。
- **turn 边界事实**：`assistant_run_end` 携带 `turnInputEntryId / assistantEntryId(s)`（server/routes/chat.ts:916 persistedTurnEntryIds；协议 server/ws-protocol.ts:16）。
- **只读消费者**：`server/history-read/`（历史重投影，含 modelCallRefBySourceIndex，types.ts:376）、branch head 校验、conversation-map、前端气泡归并。

## 3. 业务 task（lib/task-registry.ts）

- **taskId 生成**：TaskRegistry **不铸造** id——`register(taskId, ...)`（task-registry.ts:114）由调用方传入，仅 `assertText`（115）。各调用方自行铸造（风险点，格式不统一）：
  - subagent：`lib/tools/subagent-tool.ts:55-57` `subagent-${Date.now()}-${random6}`
  - workflow：`lib/tools/workflow-tool.ts:207` `workflow-${Date.now()}-${random6}`
  - rewind：`lib/tools/rewind-tool.ts:132` `rewind-${checkpointName}-${Date.now()}`
  - speech：`core/media-adapters/speech.ts:86/175/257/375` `speech-${ts36}-${random6}`
  - 媒体任务：`core/media/image-task-runner.ts:15-17` `createTaskId()` = 裸 `ts36+random4`（无前缀）
- **写入者**：`TaskRegistry._persist()`（task-registry.ts:499-511，atomicWriteSync）→ `{LINGXI_HOME}/.ephemeral/plugin-tasks.json`（engine.ts:866-869 构造）。运行时 handler 留内存（886-891 注册 subagent abort）。
- **状态机**：ACTIVE_STATUSES（task-registry.ts:16：pending/running/paused/blocked/recovering）、FINAL_STATUSES（:18：completed/failed/canceled/aborted）；重启恢复把 active 改 `recovering`（:483-486）。**TaskRegistry 本身没有 attempt/generation 概念**；最接近的"generation"是会话 JSONL 变更世代（core/session-file-mutation-epoch.ts:39-53，进程内内存计数器，非持久事实）。
- **只读消费者**：engine 任务列表投影（engine.ts:1161/1226）、loop 守恒检查 `hasActiveForParentSession`（task-registry.ts:327-331）、`server/task-bus-handlers.ts`、block_update 事件（ws-protocol.ts:53）。
- **双记账风险**：媒体任务同时存在于 `core/media/task-store.ts`（`plugin-data/image-gen/tasks.json`，task-store.ts:393）与 TaskRegistry/ActivityHub——两套状态两个文件。

## 4. run / attempt（引擎一次执行）

- **Assistant Run（权威 run）**：`runId = crypto.randomUUID()`，生成于 `server/routes/chat.ts:868`（`beginAssistantRun`）；插话切分时重铸（chat.ts:1015 `splitAssistantRunForStestedInput`）。状态机：`agent_start` → beginAssistantRun（chat.ts:862-897，触发点 1874）；**唯一 finalize 出口** `agent_settled` → `finishAssistantRun`（chat.ts:910-987，exactly-once，911 guard）。协议定义 `server/ws-protocol.ts:14-17`："一次用户输入到 agent_settled 的完整执行周期，多个 Pi Model Turn 复用同一 runId"。**仅内存，不持久化**。
- **Pi Model Turn**：`model_turn_start/end` 的 turnId 仅 diagnostics（ws-protocol.ts:18-20），绝不 finalize Run。
- **model attempt**：见第 5 节 `ma_`。
- **subagent run**：`lib/subagent-run-store.ts:215/219`（`subagent-fork-run-${randomUUID()}` / `workflow-fork-run-...`），持久化 `subagent-runs.json` / `subagent-threads.json`（registry `subagent-state` 条目 store-registry.ts:890-903）；threadId=`subagent-fork-${randomUUID()}`（lib/subagent-thread-store.ts:183）。
- **只读消费者**：desktop 前端 run 生命周期（use-stream-buffer、process-fold）、usage 记账（chat.ts:919-947）。

## 5. model call（lib/llm/）

- **生成（唯一铸造厂）**：`lib/llm/model-call-identity.ts` — 前缀常量 `mc`/`ma`/`mt`（24-26 行），`mint` 格式 `${prefix}_${now36}_${seq36}_${random6}`（36-39 行，进程内单调计数 + 随机段防跨进程碰撞）；导出 `mintModelCallId`(54)/`mintModelAttemptId`(58)/`mintModelTraceId`(62)。
- **callId**：`model-call-recorder.ts:107-109` 在 recorder 创建时（Provider 请求前）铸造或显式接管；另有两处直铸：`lib/pi-sdk/model-call-stream-observer.ts:207`、`lib/llm/cache-preserving-compaction-agent-run.ts:418`。
- **attemptId 语义**：`beginAttempt`（model-call-recorder.ts:236-247）每次网络 attempt 新铸 `ma_`；Provider/SDK retry 属同一 callId 下不同 attemptId（identity.ts 头注 7-10 行）。attempt 边界示例：`core/llm-client.ts:838-850`（callText 一次调用=一个 attempt，attemptVisibility exact）。
- **mt_ 轨迹规则（model-trace-scope.ts）**：
  - **会话内后续 turn 复用**：`runWithModelTraceRoot` 156-182 行（reuseTraceId 分支 169-180：显式 scope 进入，lastCallId=null 防跨轮伪造因果）；引擎接线 `core/engine.ts:700-701` `resolveSessionReusableTraceId` → `model-observability-trace-store.ts:476-486`（SQL：该 session 最近一次 origin 非空的 trace；474 行注明不得按 origin='user_turn' 过滤）；调用点 `core/session-coordinator.ts:5012-5036`（prompt 入口，5023 行用业务 sessionId 而非 SDK UUID，5000-5004 行记录了用错 id 导致"结构性零命中"的回归）。
  - **后台/定时独立根**：`runWithNewModelTrace` 117-135 行（强制覆盖外层 scope，§五十：30 分钟后的 timer 回调不得仍属 T1）；显式脱离 `runWithoutModelTrace` 138-140。
  - **未知因果 = null**：`resolveModelTraceContext` 244-271 行；singleton 分支 270 行 `parentCallId: null`（234 行"自动猜 parentCallId 不安全"）；工具边界冻结 `runToolExecutionWithModelTrace` 193-211（causalParentCallId=进入工具时 scope.lastCallId）。
- **写入者（持久化）**：观测事件 → `installModelObservabilityPersistence` → `model-observability-trace-store.ts`（upsertTrace:109 / touchTrace:119 / insertCall:126，model_attempts 行 155 DDL）写 `{LINGXI_HOME}/model-observability/observability.sqlite`（schema `model-observability-schema.ts:46`，SCHEMA_VERSION=7 行 30；崩溃重启 reconcile 450-456 行标 `interrupted_by_restart` 不改 terminal_status）。
- **只读消费者**：`lib/llm/model-observability-read-database.ts`（只读打开）、`server/routes/model-observability.ts:140/182`（queryCalls/queryTraceDetail）、`server/history-read/types.ts:376`。

## 6. tool call

- **id 来源**：toolCallId 是 **Provider 分配**的 tool call id（经 pi-ai → pi-agent-core agent.js:403-409）；Google 系无原生 id 时由 pi-ai 铸（`node_modules/@earendil-works/pi-ai/dist/api/google-generative-ai.js:139`）。Lingxi 不重铸。
- **记录位置（权威）**：session JSONL 的 toolCall/toolResult entry（Pi SessionManager append，见第 2 节）；工具名/参数归 lib/tools；执行边界 `core/tool-invocation-gateway.ts`（request.toolCallId 定义 27 行，透传 374/470）+ `lib/tools/invocation/prepared-invocation-context.ts:17/90`。
- **另一身份（勿混淆）**：ToolTargetIdentity（工具目录身份，非调用身份）`lib/tools/invocation/identity.ts:34-87`，`tool:first-party|plugin|mcp:{...}`。
- **观测侧**：toolCallId 只进 trace scope refs（model-trace-scope.ts:193-211，"不建 Tool Trace Store"）；流事件 tool_start/tool_end 带 id（ws-protocol.ts:32-33）。

## 7. 其他 trace 概念（除 mt_ 外）

- `knowledge_trace` WS 事件行（id 由知识注入链路铸造，UI 过程行堆）：ws-protocol.ts:50、server/routes/chat.ts:2012-2016。非持久身份。
- `AppError.traceId`：`shared/errors.ts:57`（`Math.random().toString(16).slice(2,10)` 随机 8 hex），HTTP 错误信封 server/index.ts:653、`server/http/route-errors.ts:9-56`（RouteError 透传）。错误追踪专用，与 mt_ 无关。
- mt_ 之外的持久 trace 权威只有 observability.sqlite 的 `traces` 表（schema:104）。同名不同义是重构风险点。

## 8. resource / SessionFile / MediaItem / 交付文件

- **SessionFile id**：`sf_<sha256[:16]>`（ownerKey + sourceKey 或文件系统身份），`lib/session-files/session-file-registry.ts:868-874`；ownerKey 规则 876-888（优先 sessionId）。sidecar `{sessionPath}.files.json` v1（8-15 行）；缓存目录 `{LINGXI_HOME}/session-files/{sha256-24}`（17-22）。所有者：`SessionFileRegistry`（50 行起）。
- **Resource id**：`res_sf_...` = 前缀 `res_` + sf_ id，`lib/resources/resource-envelope.ts:2,7-14`（RESOURCE_ENVELOPE_SCHEMA_VERSION=1）。只读映射，不新造身份。
- **MediaItem**：SessionFile 的投影（`core/plugin-context.ts:107-124 toMediaItem`；`lib/tools/media-details.ts collectMediaItems`）。
- **媒体生成交付文件**：`core/media/universal-media-manager.ts:366-367`（`plugin-data/image-gen/` 与 `generated/`，故意保留旧名）；任务状态 `plugin-data/image-gen/tasks.json`（core/media/task-store.ts:393）。
- **stage_files 交付**：`lib/tools/output-file-tool.ts`（142-145 行：优先用 fileId 找 SessionFile 真相源再复用交付语义）。
- **观测 blob**：blobId = `mb_<random>`（store-registry.ts:1158 identityContract；`model-observability-blob-store.ts`），文件名不带原名。

## 9. streamId / seq（server/session-stream-store.ts）

- **生成**：`createStreamId`（session-stream-store.ts:251-253）= `s_${Date.now().toString(36)}_${random6}`；`beginSessionStream` 48-60 行（可显式传入复用）。seq = `state.nextSeq++`（71 行）。
- **上限**：DEFAULT_MAX_EVENTS=5000（12 行）、DEFAULT_MAX_BYTES=8MB（13 行）、DEFAULT_MAX_EVENT_BYTES=256KB（14 行）；trimEvents 144-152；turn 结束 `finishSessionStream` 84-89 清空 ring buffer；resume 旧 stream → reset 全量重放（114-124）。
- **生命周期绑定**：一个用户 Run = 一个 streamId（server/routes/chat.ts:882-884）；状态对象在 chat.ts:633 `createSessionStreamState()` 创建，纯内存。
- **消费者**：resume_stream 路由、abort 的 stale_stream 判定（ws-protocol.ts:9/36-37）、桌面 stream buffer。

## 10. SQLite 持久化全景 + DATA_EPOCH + schema fingerprint

- **SQLite 库（5 个）**：
  1. `session-manifest.db` — core/session-manifest/store.ts（engine.ts:2039 打开）
  2. `model-observability/observability.sqlite` — lib/llm/model-observability-schema.ts:46
  3. `knowledge/knowledge.db` — lib/knowledge/knowledge-store.ts（user_version v19，registry 兼容性长注 store-registry.ts:1004）
  4. `agents/{agentId}/memory/facts.db` — lib/memory/fact-store.ts（registry `agent-facts-sqlite`，store-registry.ts:480-495）
  5. `file-history/{workspaceHash}/history.sqlite` — lib/file-history/history-store.ts（registry:551-570）
- **权威清单**：`shared/persistence/store-registry.ts`（PERSISTENT_STORES，~70 个 store 定义，行 122-1543+；含 pathPatterns/epochPolicy/siteRules）；类型 `store-registry-types.ts`；启动相位 `startup-phases.ts`。
- **DATA_EPOCH**：stamp/journal 读写 `shared/data-epoch.cjs`（stamp=`data-epoch.json` 19-21 行，journal=`data-epoch-transition.json` 23-25 行，schema 版本 7-8 行）；协调器 `core/data-epoch-coordinator.ts`（transitionId=crypto.randomUUID() 380 行；启动闸 server/index.ts:300-324）；**迁移登记表当前为空**：`core/data-epoch-migrations.ts:40` `DATA_EPOCH_MIGRATIONS = Object.freeze([])`（尚无任何 epoch 边缘）；checkpoint/restore：core/data-epoch-checkpoint-provider.ts、core/data-epoch-restore.ts。
- **persistence-schema-fingerprint**：产物 `build/persistence-schema-fingerprint.json`；生成 `scripts/generate-persistence-schema-fingerprint.mjs`（FINGERPRINT_PATH 15 行，经 `scripts/scan-persistent-stores.mjs` 扫描 store-registry + 各 owner 模块源码）；守卫 `scripts/check-persistence-schema-fingerprint.mjs:37`；tripwire 测试 `tests/persistence-schema-tripwire.test.ts:23`。

## 11. 凭证存储

- **OAuth**：`{LINGXI_HOME}/auth.json`，写者 = Pi SDK `AuthStorage.create` + `FileAuthStorageBackend`（core/model-manager.ts:190-192，同文件同锁；authJsonPath 235 行）；强制刷新 core/oauth-force-refresh.ts；auth.json 只保留 OAuth 条目（model-manager.ts:428 注释）。
- **API key（运行时真相源）**：Provider Catalog v2 `provider-catalog.json`（`core/provider-catalog.ts:15` PROVIDER_CATALOG_FILE，ProviderCatalogStore:101）；旧 key 从 auth.json/models.json 抢救迁移：`core/provider-auth-migration.ts:130+`（migrateLegacyApiKeyAuthToProviders，优先级 catalog > models.json > auth.json）；写入方还有 server/routes/providers.ts、core/model-sync.ts、备份保留 core/credential-backup-retention.ts（registry `provider-state` 条目 store-registry.ts:419-445）。
- **设备/本地用户凭证**：devices.json / device-credentials.json / local-user-auth.json（core/device-registry.ts；registry:263/277）；密文写经 `shared/secret-fs.ts writeSecretFileSync`。运行时 key 引用：shared/runtime-api-key-ref.ts。

## 12. LINGXI_HOME 数据目录解析

- **权威实现**：`shared/hana-runtime-paths.cjs:13-16` `resolveLingxiHome(input)`（默认 `~/.lingxi`，支持 `~` 展开 + path.resolve）；TS 再导出 `shared/hana-runtime-paths.ts:3-9`。
- **读取点**：`server/index.ts:251-253`（resolve 后回写 process.env；251 行注释即开发隔离约定 `LINGXI_HOME=~/.lingxi-dev`）、`desktop/main.cjs:168-169`、`server/bootstrap.ts:21`（日志）、`core/server-port-selection.ts`（同宅互斥）、各测试。
- **重复实现（风险）**：`cli/local-server.ts:5-15` `resolveCliLingxiHome` + 私有 `resolveHomePath`——与 shared/hana-runtime-paths.cjs 逻辑等价但独立维护。
- **生产/开发区别**：无代码级 dev 目录常量；区别纯靠环境变量（server/index.ts:251 注释），默认生产目录 `~/.lingxi`。桌面端Electron userData 也按 LINGXI_HOME 隔离（desktop/main.cjs:359）。

## 汇总表

| 事实种类 | 当前权威（写入者） | 文件:行 | 风险备注 |
|---|---|---|---|
| 业务 sessionId `sess_` | SessionManifestStore → session-manifest.db | core/session-manifest/id.ts:5-9; store.ts:220/1112; engine.ts:2039 | 与 SDK 文件头 UUID 双身份（session-manager.js:650,668），靠 manifest locator 桥接；trace 复用误用 SDK UUID 曾致零命中（session-coordinator.ts:5000） |
| SDK session UUID / entry id | Pi SDK SessionManager → JSONL | pi-coding-agent session-manager.js:23,650,668,770 | entry id 为 8 字符 randomUUID 前缀，碰撞域仅本文件 |
| taskId（TaskRegistry） | 各工具自铸 → .ephemeral/plugin-tasks.json | lib/task-registry.ts:114,499; engine.ts:866 | 无统一铸造厂：subagent-tool.ts:55 / workflow-tool.ts:207 / rewind-tool.ts:132 / speech.ts:86 / image-task-runner.ts:15 五种格式并存；媒体任务另在 core/media/task-store.ts 双记账 |
| runId（Assistant Run） | chat 路由内存态 | server/routes/chat.ts:868,910,1015; ws-protocol.ts:14 | 不持久化；与 streamId/turnId 三个 per-reply 身份并存 |
| model callId `mc_` / attemptId `ma_` | model-call-recorder → observability.sqlite | model-call-identity.ts:24-39,54-62; recorder.ts:107,239 | 唯一铸造厂正确；schema user_version=7 独立于 DATA_EPOCH |
| traceId `mt_` | ModelTraceScope + trace-store 复用查找 | model-trace-scope.ts:117-182,244-271; trace-store.ts:476; session-coordinator.ts:5032 | 同名竞争：AppError.traceId（errors.ts:57）、knowledge_trace 行（ws-protocol.ts:50）语义无关 |
| toolCallId | Provider 分配 → session JSONL + gateway 透传 | pi-ai google-generative-ai.js:139; tool-invocation-gateway.ts:27,374 | 与 ToolTargetIdentity（invocation/identity.ts）易混淆 |
| SessionFile `sf_` / Resource `res_` | SessionFileRegistry → sidecar + session-files/{hash} | session-file-registry.ts:868-874; resource-envelope.ts:7 | MediaItem 仅投影无独立身份；媒体生成文件在 plugin-data/image-gen 另立 |
| streamId `s_` / seq | chat 路由内存 ring buffer | session-stream-store.ts:12-14,48,71,251; chat.ts:633,882 | 上限 5000 事件/8MB/256KB 单事件；turn 结束清空 |
| DATA_EPOCH | data-epoch.cjs stamp/journal + coordinator | shared/data-epoch.cjs:19-25; data-epoch-coordinator.ts:380; migrations.ts:40 | 迁移表为空（无实际 epoch 边缘）；schema fingerprint 三脚本联动（generate/check/scan-persistent-stores + tripwire test） |
| 凭证（OAuth/API key） | auth.json(SDK AuthStorage) / provider-catalog.json(Catalog v2) | core/model-manager.ts:190-192; provider-catalog.ts:15; provider-auth-migration.ts:130 | 两处存储按类型分工，迁移抢救逻辑是关键脆弱点 |
| LINGXI_HOME | shared/hana-runtime-paths.cjs resolveLingxiHome | hana-runtime-paths.cjs:13-16; server/index.ts:252; desktop/main.cjs:168 | **同职责双实现**：cli/local-server.ts:5-15 resolveCliLingxiHome 独立复刻同一逻辑 |

**明确的同职责多实现（两处位置）**：
1. 数据目录解析：`shared/hana-runtime-paths.cjs:13` vs `cli/local-server.ts:6-15`。
2. 会话身份：业务 `sess_`（core/session-manifest/id.ts:5）vs SDK UUID（pi-coding-agent session-manager.js:650）——有意分层但文件名使用 SDK id，是所有"sessionPath↔sessionId"换算复杂度的根源。
3. 任务状态记账：`lib/task-registry.ts`（plugin-tasks.json）vs `core/media/task-store.ts`（image-gen/tasks.json）。
4. trace 一词三义：`mt_`（model-trace-scope.ts）、`AppError.traceId`（shared/errors.ts:57）、`knowledge_trace`（ws-protocol.ts:50）。