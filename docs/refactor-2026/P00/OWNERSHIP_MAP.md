# OWNERSHIP_MAP — 核心事实身份与单一权威（P00-T03）

版本：1.0｜证据基线：HEAD `92c6646c5`。逐项原始证据见 `artifacts/refactor-2026/P00/logs/agent-ownership-raw.md`。
状态图例：✅复用验证（已有正确权威+测试）｜⚠️风险待验证｜🔴已证实缺口。

| 事实 | 当前权威（写入者） | 生成/格式 | 只读消费者 | 状态 | 检测方式 |
|---|---|---|---|---|---|
| 业务 sessionId | SessionManifestStore → `session-manifest.db`（`core/session-manifest/store.ts:181`，DDL :220；engine.ts:2039 打开） | `sess_{ts36}_{rand20}`（`core/session-manifest/id.ts:5-9`；唯一性 store.ts:1112） | engine.getSessionIdForPath、resolver、trace-store:476 | ✅（tests/session-*） | manifest 查询+路由回归 |
| SDK session UUID / entry id | Pi SDK SessionManager → `agents/{id}/sessions/{sdkUuid}.jsonl` | UUID（session-manager.js:650）；entry id=UUID前8（:23） | 历史重投影、branch head | ⚠️双身份有意分层，但**JSONL 文件名用 SDK UUID**，是 sessionPath↔sessionId 换算复杂度根源；曾发生误用致 trace 复用零命中（session-coordinator.ts:5000 注释） | session-coordinator 回归 |
| 用户 turn / message id | Pi SDK SessionManager append（_appendEntry :770） | entry.id+parentId | history-read 投影、run 终态（chat.ts:916 persistedTurnEntryIds） | ✅ | history-run-outcome-edges 等测试 |
| 业务 taskId | TaskRegistry（`lib/task-registry.ts:114` register；_persist :499 → `.ephemeral/plugin-tasks.json`，engine.ts:866 构造） | **调用方自铸**：subagent-`lib/tools/subagent-tool.ts:55`、workflow-`workflow-tool.ts:207`、rewind-`rewind-tool.ts:132`、speech-`core/media-adapters/speech.ts:86`、image-task-runner.ts:15 裸 ts36 | engine 投影、loop 守恒（task-registry.ts:327）、task-bus-handlers | 🔴**无统一铸造厂，5 种格式并存**；媒体任务另在 `core/media/task-store.ts`（plugin-data/image-gen/tasks.json）双记账 | 主责 P02（统一 taskId 铸造与注册语义） |
| Assistant Run（runId） | chat 路由内存态（`server/routes/chat.ts:868` beginAssistantRun；**唯一 finalize** :910 finishAssistantRun exactly-once；插话重铸 :1015） | crypto.randomUUID()，不持久化 | 前端 run 生命周期、usage 记账（chat.ts:919-947） | ✅（协议 ws-protocol.ts:14-17：一次用户输入→agent_settled） | chat 路由测试 |
| model callId `mc_` / attemptId `ma_` | model-call-recorder（`lib/llm/model-call-recorder.ts:107/:239`）→ observability.sqlite | 唯一铸造厂 `lib/llm/model-call-identity.ts:24-39`（前缀+ts36+seq36+rand6） | 观测读库、usage | ✅（schema v7，tests/model-call-*） | 观测测试 |
| traceId `mt_` | ModelTraceScope（`lib/llm/model-trace-scope.ts`）+ 复用查找 `model-observability-trace-store.ts:476` | `mt_` 同铸造厂；**规则**：user_turn 会话粒度复用（runWithModelTraceRoot :156-182，接线 session-coordinator.ts:5032；engine.ts:700 resolveSessionReusableTraceId）；后台强制新根（runWithNewModelTrace :117，scheduler.ts:234）；未知因果=null（resolveModelTraceContext :244-271） | 观测、usage | ✅（tests/model-trace-scope 21例、model-observability-session-trace-reuse） | 既有测试+P00-A07 |
| 同名竞争：trace 一词三义 | `mt_`（模型轨迹）/ `AppError.traceId`（`shared/errors.ts:57` 错误信封）/ `knowledge_trace`（ws-protocol.ts:50 UI 过程行） | — | — | ⚠️语义不同非缺陷，但重构中易混淆，登记命名风险 | 文档约束（P04/P05 引用） |
| toolCallId | Provider 分配（pi-ai；Google 系由 pi-ai 铸）→ session JSONL；网关透传（gateway :27/:374） | 外部 id，Lingxi 不重铸 | gateway、trace refs、tool_start/end 事件 | ✅ | tool-invocation-gateway 测试 |
| SessionFile `sf_` / Resource `res_` | SessionFileRegistry（`lib/session-files/session-file-registry.ts:868-874`）→ sidecar `{sessionPath}.files.json` + 缓存目录 | sf_+sha256[:16]；res_sf_ 前缀映射（resource-envelope.ts:7） | 卡片、media、导出 | ✅ | resource-io 测试(16) |
| streamId `s_` / seq | chat 路由内存 ring buffer（`server/session-stream-store.ts`；chat.ts:633/:882） | `s_{ts36}_{rand6}`；seq 递增；上限 5000 事件/8MB/256KB | resume、stale_stream 判定 | ✅（tests/session-stream-store 9例） | 既有测试 |
| DATA_EPOCH | `shared/data-epoch.cjs`（stamp data-epoch.json/journal）+ `core/data-epoch-coordinator.ts`（启动闸 server/index.ts:300-324） | 迁移登记表**当前为空**（data-epoch-migrations.ts:40） | 启动闸、checkpoint/restore | ✅机制在；⚠️尚无实际 epoch 边缘被行使过 | persistence-schema-tripwire |
| 持久化全景 | `shared/persistence/store-registry.ts`（~70 store，含 pathPatterns/epochPolicy/siteRules） | — | 指纹扫描 scripts/scan-persistent-stores.mjs | ✅（tripwire 15例+CI guard） | build/persistence-schema-fingerprint.json |
| 凭证（OAuth） | Pi SDK AuthStorage+FileAuthStorageBackend → `{LINGXI_HOME}/auth.json`（core/model-manager.ts:190-192） | — | ModelRuntime credentials | ✅（oauth-* 测试） | 凭证路由回归 |
| 凭证（API key） | ProviderCatalogStore v2 → `provider-catalog.json`（core/provider-catalog.ts:15；迁移抢救 provider-auth-migration.ts:130） | — | providers 路由、模型解析 | ✅ | provider-compat 测试 |
| 设备/本地用户凭证 | device-registry（devices.json/device-credentials.json，密文经 shared/secret-fs） | — | web-auth、mobile | ✅ | server/device 测试 |
| LINGXI_HOME 解析 | `shared/hana-runtime-paths.cjs:13-16` resolveLingxiHome（读取点 server/index.ts:252、desktop/main.cjs:168） | 默认 `~/.lingxi`；dev 由 launch.js 覆盖 `~/.lingxi-dev` | 全部 store | 🔴**同职责双实现**：`cli/local-server.ts:5-15` resolveCliLingxiHome 独立复刻 | 主责 P01（统一到 shared） |

## 观测分组口径（S11 语义，P00-A07）

- 「单一 run」= 一次用户输入到 agent_settled（runId，内存）；「观测分组」= mt_ 轨迹。
- 用户会话多轮：同会话后续 turn 复用既有 mt_（session 粒度），**traceId 恒不等于 sessionId**。
- 后台任务（automation/phone_message/bridge_message）：每次入口强制新根。
- parentCallId：只认运行时因果链（stream observer 推进 lastCallId / 工具边界快照），无事实→null，不按时间猜。
- TaskRegistry 不是前台运行总管：同 taskId 合法重注册须区分 attempt/generation（当前无该概念——见 taskId 🔴 行，归 P02）。

## 结论

18 类事实中 13 类已有单一权威+测试（✅复用验证）；3 个 🔴 已证实缺口（taskId 无统一铸造厂、媒体任务双记账、LINGXI_HOME 双实现）全部登记到 REFACTOR_BACKLOG 并给出唯一主责阶段；2 个 ⚠️ 命名/分层风险登记为后续阶段的引用约束。未发现第二套 Agent 循环/权限网关/凭证存储/消息语义解析器。
