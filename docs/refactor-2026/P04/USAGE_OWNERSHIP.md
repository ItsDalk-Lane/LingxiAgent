# USAGE_OWNERSHIP — 用量记录与实际 attempt 对应（P04-T06）

日期：2026-09-22｜基线 HEAD `3286c96e5`。全部 UNCHANGED_VERIFIED；本阶段零生产改动。
P04-FIXR1（2026-09-22，独立验收问题 #1）：§1 写入点枚举按源码复核补全（文档修订，仍零生产改动），见文末修订记录。

## 1. 计量观察点（生产写入面全枚举）

| # | 观察点 | 写入 | 去重身份 |
|---|---|---|---|
| 1 | callText（core/llm-client.ts:856-1099） | usageLedger.start→finish/recordError（一次请求恰一条；pending map 删除后才可 finish，重复 finish 返回 null） | requestId + metadata.modelCallId |
| 2 | 操作协议（model-operation-client.ts withModelRequestAccounting） | 同上 | 同上 |
| 3 | 媒体/语音（universal-media-manager / image-task-runner / speech-recognition-service） | 同上 | 同上 |
| 4 | chat message_end 补账（recordAssistantUsage：session-coordinator.ts:588、bridge-session-manager.ts:345、hub/agent-executor） | ledger.record/recordError（usage 在 assembled message 上） | metadata.modelCallId（WeakMap：model-call-correlation.ts） |
| 5 | MC-10 日记直连摘要（lib/llm/observed-pi-direct-summary.ts:158-204 observePiDirectSummary；usageLedger 由 pi-sdk facade generateSummary 第 14 参 observerContext 显式注入；传 streamFn 的调用不经此路径——观测与补账走 chat 链，防双计） | usageLedger.start→finish/recordError（一个 generateSummary 边界恰一条；accounting 失败不影响观测/业务） | requestId + metadata.modelCallId（=recorder.callId） |
| 6 | MC-02 缓存保留压缩 run（lib/llm/cache-preserving-compaction-agent-run.ts:423 isolatedStreamFn 边界，finish/recordError 见同文件 :381/:384/:518/:563；生产入口 core/session-compactor.ts:1878） | usageLedger.start→finish/recordError（每次真实 logical call 恰一条；业务级 recovery/repair 是新 call 新键） | requestId + metadata.modelCallId（=mintModelCallId()） |
| 7 | connectivity-probe 生成探测（lib/llm/provider-client.ts:344 probeProvider，仅 anthropic-messages 分支经 withModelRequestAccounting；其余协议为 GET /models CONTROL_PLANE 零写入；入口 server/routes/providers.ts:825 设置页连通性测试，操作覆盖记载在 OPERATION_COVERAGE_MATRIX provider-probe family） | withModelRequestAccounting 内 usageLedger.start→finish/recordError | requestId + metadata.modelCallId（observedModelCallLedgerMetadata(recorder)） |
| 8 | 观测库投影（model-observability accounting projection） | llm_usage 事件 → 投影行 | **modelCallId 幂等 upsert**（A13 核心去重） |

写入面枚举口径（P04-FIXR1 全量复核，2026-09-22）：对生产代码 grep ledger 四写方法（start/finish/record/recordError）的全部直调点 + `withModelRequestAccounting`（lib/llm/model-request-accounting.ts，唯一共享写入 helper）的全部生产调用方，逐一核对到上表 7 处写入边界（#8 为消费投影非写入）。初版「无其他 usage 写入点（仅上述边界调用）」的 grep 声称枚举不全（漏 #5/#6/#7 三处），已废止；本轮源码逐处核实三处均以 metadata.modelCallId 键控台账，与投影 A13 幂等去重衔接，无重复计费路径。已知潜在写入点：lib/llm/session-snapshot-side-task-runner.ts:83 存在同构 start→finish/recordError 直写，但其唯一上游 lib/memory/memory-reflection-runner.ts runMemoryReflection 当前无生产调用方（grep 仅命中定义与注释引用），生产不可达、不计入本表；未来接线时须补入。UI 重放/重载不产生新写入（投影消费事件流，读取路径只读）。

## 2. 状态与诚实性（A14）

- 成功：status=ok + usage 数值；失败：error；取消：aborted；**供应商未给 usage：status=usage_missing，usage=null**——数值列保持 NULL，绝不被写成 0（“Number(null)===0 陷阱”测试锁定）。
- 估算与真实计费不混标：normalizeLlmUsage 仅消费 provider 实际返回字段（input/output/cacheRead/cacheWrite/totalTokens + 厂商形状归一），cost 由模型条目 costRates 计算；无精确 tokenizer 的地方按既有机制标字节代理（estimate-text-tokens），不冒充精确 token。
- MC-03 native summarizer 不经 exact 关联：usageCorrelation="not_correlated" 作为运行时事实持久化，不猜。
- 证据：tests/llm-usage-ledger.test.ts（“records usage_missing…”、“无事实字段…”在投影测试）、tests/model-observability-accounting-projection.test.ts（10 例）。

## 3. 重复事件与去重（A13）

- 台账（usage 页/导出）来自投影：**同一 modelCallId 重复进入 → 一行**（幂等 upsert，§十四）；bounded backfill 幂等 + meta 标记不声称完整历史。
- 台账读取/界面重载不新增：投影按事件流消费，读路径（query/export 路由）零写入。
- 证据：tests/model-observability-accounting-projection.test.ts · “同一 modelCallId 重复进入 → 一行”、“bounded ledger backfill：幂等…”、“live ingestion…”、“retention：trace 删除时 usage projection 随之删除”。

## 4. 查询/筛选/导出契约（保持）

- ledger list 过滤：since/until/status/attributionKind/sessionId/sessionPath/childSession(Id|Path)/agentId/subsystem/operation/modelId/provider/limit。
- 观测查询：model-observability-query（trace/call/payload/detail）；导出：model-observability-export。
- 证据：tests/llm-usage-ledger.test.ts（“filters by sessionId…”、“bounds entries and filters…”、“date window…”、childSessionPath）、tests/model-observability-query.test.ts、tests/model-observability-export.test.ts。

## 5. 缓存 token 与媒体计费

- cacheRead/cacheWrite 只取供应商实际字段（usage-observer 归一 + cacheSupport），无字段则缺省，不凭文本 token 推算。
- 媒体计费仅使用 adapter 实际可得 usage（外部进程边界 opaque 时如实无 usage）；价格来源=模型条目 cost（costRates 随记录持久化 rawUsageShape 供版本追溯）。

## 6. 台账合计口径

usage 页合计来自投影真实记录；未请求（无 attempt）、重复展示（重载）、空 usage（usage_missing）均不产生虚假已知消费。P05 历史投影消费同一事实源，不二次写入。

## 7. 修订记录

- **P04-FIXR1（2026-09-22）**：来源 = 独立验收 [P04_ACCEPTANCE_REVIEW.md](P04_ACCEPTANCE_REVIEW.md) 问题 #1（§1「无其他 usage 写入点」枚举不全）。修复 = §1 表新增 #5 MC-10 直连摘要、#7 connectivity-probe 生成探测两处（验收指出），复核中另发现并补入 #6 MC-02 缓存保留压缩 run（core/session-compactor 生产可达，同为漏列）；废止初版不完备的 grep 声称并改写枚举口径；登记 session-snapshot-side-task-runner 潜在（当前不可达）写入点。三处键控均为 metadata.modelCallId，与 §3 投影幂等去重一致。零生产代码改动；命令与 exit code 见 artifacts/refactor-2026/P04/logs/P04-FIXR1-*。
