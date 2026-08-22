# Model Observatory — Release Acceptance（Phase 10 最终验收）

> 基线：`feature/model-call-observability`（Phase 10 完成树）。审计设计：
> MODEL_OBSERVABILITY_E2E_TRUTH_AUDIT.md；过程与 findings：
> OBSERVABILITY_VALIDATION_PROGRESS.md。
> 证据口径：AUTOMATED_LOCAL_PROVIDER（本地 deterministic Fake Provider Witness，
> 真实 HTTP，无公网/无付费 Provider/无真实凭证）。REAL_PROVIDER_MANUAL：未执行。

## 结论

**是否可以把 Model Observatory 当 production feature 使用？——可以（有界）。**

- 全链身份/因果/账本/安全不变量经独立 Provider Witness 纵向核实，无一 P0。
- 一处 P1（DST 历史 date bucket）以 failing-test-first 修复并反向回归（F-1）。
- 诚实缺失（SDK 结构性不可见）全部保持 BOUNDED：绝不升级为 FULL。
- P2/P3 已记录；F-2（文档过实）与 F-3（blob 路由零测试）已处置。

## 1. MC Matrix

| MC | Production Reachable | Observer | Trace | Semantic Req | Provider Req | Provider Resp | Semantic Resp | Usage | Provenance | Durable | UI | Result |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| MC-01 Pi Chat | ✅ | FULL | FULL（S1/S2 实测） | FULL | FULL（hook body，runtime_exact） | METADATA_ONLY（hook 无 body） | FULL | FULL（correlation 需 coordinator 补账） | partial（SDK 尾段 structural） | FULL | Ledger 行 ≡ DB（S34） | **PASS** |
| MC-02 AgentRun compaction | ✅ | FULL | FULL | FULL | UNAVAILABLE（options 无 onPayload） | UNAVAILABLE | FULL | FULL | partial | FULL | 诚实 unavailable | **BOUNDED** |
| MC-03 native compaction | ✅ | FULL | FULL | FULL（structural） | UNAVAILABLE | UNAVAILABLE | FULL | **not_correlated（见 §2）** | structural | FULL | 诚实 unavailable | **BOUNDED** |
| MC-04 callText 四协议 | ✅ | FULL | FULL | FULL | FULL（构造点） | FULL（parsed/stream_aggregate） | FULL | FULL | **exact（四协议 mapping）** | FULL | Ledger ≡ DB | **PASS** |
| MC-05 Anthropic probe | ✅ | FULL | FULL（force-new origin=provider_probe） | FULL（"." 值捕获） | FULL | METADATA_ONLY（成功不读 body） | FULL | FULL | exact | FULL | — | **PASS** |
| MC-06 Image ×7 | ✅ | FULL | FULL | FULL | FULL（codex 401 双 ordinal） | FULL | FULL | FULL（usage_missing≠error） | exact（media locator） | FULL | 1 Call 非 2 | **PASS** |
| MC-07 Dreamina CLI | ✅ | FULL | FULL | FULL | OPAQUE/external_process | OPAQUE | FULL（taskId） | FULL | partial（CLI wire opaque） | FULL | OPAQUE 不升级 | **BOUNDED** |
| MC-08 Video（agnes submit） | ✅ | FULL | FULL | FULL | FULL | FULL | FULL（taskId/deferred） | FULL | exact | FULL | poll 0 record | **PASS** |
| MC-09 Speech ×4 | ✅ | FULL | FULL | FULL（audio=local_file_reference） | FULL（Volcengine uid 协议脱敏） | FULL | FULL（transcription） | FULL | exact | FULL（audio Blob=externalized，见 F-2） | — | **PASS** |
| MC-10 diary direct summary | ✅ | FULL | FULL（diary trace 内同根） | FULL | UNAVAILABLE | UNAVAILABLE | FULL | FULL | exact | FULL | parent=null 不伪造 | **PASS** |

MC-11+：无（Step 1 重扫仍为 10 条；新增 LATENT 登记：core/media/local-cli-wrapper.ts runLocalCliMedia，零 importer）。

## 2. Usage Matrix

| MC | Usage Produced | modelCallId Exact Correlation | Durable Projection | UI Availability |
| --- | --- | --- | --- | --- |
| MC-01 | provider usage | FULL（message_end WeakMap 补账） | ✅ | present |
| MC-02 | provider usage | FULL（runner metadata） | ✅ | present |
| MC-03 | compaction entry 内 usage | **NONE —— KNOWN CAPABILITY GAP**（pi 0.84.1 completeSummarization options 无 onPayload/usage hook；无准确 callId association point；禁止按时间/模型/顺序猜） | ❌（not_correlated，不投影） | not_correlated（诚实 warning） |
| MC-04 | provider usage | FULL（llm-client metadata） | ✅（S7 实测 token 全链一致） | present |
| MC-05/06/07/08 | 通常 usage_missing | FULL | usage_missing ≠ error（S12 实测） | not_correlated |
| MC-09 | 通常 usage_missing | FULL | ✅ | not_correlated |
| MC-10 | provider usage | FULL | ✅ | present |

## 3. Payload Truth Matrix

| MC | Layer | Runtime Visibility | Stored Visibility | UI Visibility | Truth Match |
| --- | --- | --- | --- | --- | --- |
| MC-01 | 四层 | FULL / METADATA_ONLY(resp) | 同 runtime（不升级） | 同 store | ✅（S1：hook body≡witness body，redaction 差异仅本地路径替换+sanitizationStatus） |
| MC-02/03/10 | provider 层 | UNAVAILABLE | UNAVAILABLE（payload=null） | 诚实 unavailable | ✅（S18） |
| MC-04 | 四层 | FULL | FULL | FULL | ✅（S7 四协议 body≡capture≡mapping） |
| MC-05 | resp | METADATA_ONLY | METADATA_ONLY | — | ✅（S11） |
| MC-06 | 四层 | FULL×2 attempts | FULL（ordinal 1/2） | 1 Logical Call | ✅（S13 硬场景） |
| MC-07 | provider 层 | OPAQUE | OPAQUE | OPAQUE | ✅（S15，毒丸 argv/stdout 零泄漏） |
| MC-08/09 | 四层 | FULL | FULL | FULL | ✅（S16/S17；audio=externalized F-2） |

Binary：一律 descriptor（externalized/stored 按 runtime 是否 materialize Node Buffer）；
blob bytes 绝不进 payload/DB 正文（F-2 记录真实分布）。

## 4. Trace Truth Matrix

| 场景 | 结果 |
| --- | --- |
| simple chat | 1 trace/1 call/origin=user_turn/parent=null ✅（S1） |
| multi-turn tool loop | C2.parent=C1.callId（非 toolCallId）✅（S2） |
| parallel tools | 双双 parent=C1（Phase 4 测试 3 + ALS 快照机制） |
| subagent | same trace + parent 链（Phase 4 测试 4） |
| media child | 工具内 parent=C1 ✅（S16 video 同机制） |
| diary | same trace、parent=null 不伪造 ✅（S18） |
| background/detached | force-new root（Phase 4 ingress 矩阵 + 测试 10） |
| concurrent sessions | trace/parent/payload/usage 不串 ✅（S20 实测） |
| codex 401 | 1 trace/1 call/2 attempts ✅（S13） |
| crash | terminal NULL + interruptedByRestart ≠ Error ✅（S24） |

## 5. Security Matrix

| Data | Local Owner | Remote Studio Owner | Anonymous | Export | Disk |
| --- | --- | --- | --- | --- | --- |
| metadata（calls/traces/aggregate/health/settings 读） | ✅ | ✅（STUDIO_OWNER） | ❌ 403 | metadata-only 可 | 0600/0700 |
| payload 正文 | ✅ | ❌ 403 local_only_route | ❌ | LOCAL_ONLY + sanitized only | 0600 |
| blob bytes | ✅（exact mb_* id） | ❌ 403 | ❌ | 不含 bytes | 0600 |
| settings PUT / export POST | ✅ | ❌ 403 | ❌ | — | — |
| credentials | — 不进任何层（witness 可见=redaction 只改 copy） | — | — | 零出现 | DB/WAL/SHM 字节级零命中（S1/S7/S17/S18） |

Verb hardening：未登记 verb fail closed（DELETE settings 等，含 blobs POST——本轮补测 F-3）。
Traversal/SSRF/本地文件读取：blob exact id 闭集 + 重算 contained path + traversal 全 400（F-3）。
XSS：UI 纯文本渲染（JsonValueViewer 无 HTML execution；Phase 9 测试 + S34 vertical）。
Error API：毒丸错误正文不入事件/durable DTO（S19）。

## 6. Missing-State Matrix

| State | Runtime Meaning | Store | Query | UI | Export |
| --- | --- | --- | --- | --- | --- |
| unknown | 无法知道 | 保留（不假装） | 保留 | 诚实 | 保留 |
| not_captured | persistPayloads=false | 显式列值 | filter 维度 | badge | metadata-only |
| not_correlated | 无 modelCallId（MC-03） | 不投影 | usage.availability | warning | 保留 |
| unavailable | SDK 不可见（MC-02/03/10） | payload=null 行 | contentState | 「仅本机」/不可见 | visibility 保留 |
| opaque | 外部进程（MC-07） | opaque 行 | opaque | Opaque badge | opaque 保留（非 {}） |
| expired | retention 删除正文 | 列值 | filter | badge | 无正文 |
| dropped | queue overflow | 列值+计数 | filter | 计数可见 | 无正文 |
| corrupt | JSON.parse 失败 | 原行保留 | contentState=corrupt | 不 crash | 保留 metadata |
| incomplete/crash | terminal NULL | NULL+interrupted | incomplete 伪 filter | Incomplete≠Error | 保留 |
| empty vs unavailable | 真实 {} | 保留 | 区分 | 区分 | 保留 |
| cost null / usage_missing | 未产生 | NULL（不变 0） | null（不变 0） | 「—」 | null |

Unknown 不被证明成 0：catch{return 0/[]/null} 专项审计未发现观测计数路径以假 0
表达未知（drop/queue 计数显式持久化 observability_meta）。

## 7. Performance Matrix（宽松数量级口径，非 SLA）

| Scenario | 规模 | 模式 | 结果 |
| --- | --- | --- | --- |
| 100 deterministic local calls | OFF vs Payload-ON | wall time | 无数量级回归（×5 guard 内，实测远低于）✅ |
| Blob HEAD 64MB | stat-only | <1s | ✅（includeBytes=false 走 statSync） |
| Blob GET 64MB | readFileSync | 可读全量 | 同步读 ~数十 ms 级（本地 SSD）；未观察到不可接受 event-loop stall；保留 readFileSync（若未来实测阻塞再按 §一百零五 streaming 修复） |
| Query 80 calls 分页+filter+aggregate | keyset | <1s | ✅（S29） |
| Export metadata | 流式 JSONL | bounded | ✅（S35；10k/50k 级未逐级跑，既有 Phase 8 性能 guard 覆盖 <10s） |
| UI Ledger vertical | 真实链路渲染 | 即时 | ✅（S34） |

## 8. Known Gaps（A/B/C）

**A（无法观察但诚实表达）**：MC-07 CLI wire；MC-02/03/10 provider wire
（pi 0.84.1 options 无 onPayload）；google/mistral-conversations provider_response；
Pi provider request headers/endpoint；MC-03 usage correlation（KNOWN CAPABILITY
GAP）；Pi transport retry attempt 粒度（logical_boundary）；Blob=FormData web
Blob/base64 字符串只 externalized（F-2，descriptor 诚实）。

**B（可修未修）**：无（P0=0、P1=0；F-1 已修）。

**C（未来功能）**：FTS/reasoning search/global search/saved filters/dashboard/
alerts/cloud sync/plugins——持续禁止。

## 9. Gates（执行记录）

| Gate | 结果 |
| --- | --- |
| typecheck ×3（main/node/test） | ✅ 全绿 |
| eslint（新改文件） | ✅ 0 error（114 no-explicit-any warning 与既有测试风格一致） |
| lint:boundary | ✅（1 既有基线 debt） |
| persistence scanner | ✅ 61 stores/720 sites |
| persistence fingerprint | ✅ 未变（sha256:15591e09…） |
| compute-cli-closure | ✅ 重生成 |
| i18n parity + coverage | ✅ |
| Phase 1~9 观测测试回归 | ✅ 134+161+157 |
| Phase 10 新增 E2E/UI vertical | ✅ 32 用例（8 文件） |
| full npm test（seal/坐标修复后复跑） | ✅ 12084 passed / 0 failed / 7 skipped（1196 files，2026-08-22 复跑 exit=0） |
| build:server / build:server:open / build:client | ✅ 三者全绿（throwaway 签名 key：/tmp 生成、不入库、不提交） |
| package smoke | ✅ renderer bundle 含 observability client；server bundle 含 route（model-observability/calls、/export）+ query（model_call_usage）+ storage（observability.sqlite、interrupted_by_restart）标记；seed kit manifest+签名经 verify-seed-kit 对 throwaway keyset 验证 OK |
| Cross-platform（Windows/Linux） | **NOT EXECUTED**（仅本机 macOS arm64 全量；不伪造兼容性结论） |

## 10. 使用建议

- 需要 Prompt/Response 审计时：本机（LOCAL_OWNER）打开 Observatory；远程
  viewer 只有 metadata 权限（含远程 owner）——这是产品语义不是缺陷。
- 跨 DST 历史统计：使用 timeZone 分桶（新契约）；旧固定 offset 语义保留兼容。
- MC-03 的 token 消耗在 Usage Ledger 的 compaction entry 中可见，但不与
  Observatory call 关联——诊断时需分开看（Known Capability Gap）。
