# R06 收口独立复审（Reviewer #4 / RV4）

- 复审对象：冻结候选 `844a10b2b`（`fix(rust-tauri): R06 closeout candidate — backend gap closure, AR replays, integration fixes`）
- 复审范围：R06-AR-F09（嵌入批处理）/ F10（嵌入身份，上轮正式 FAIL）/ F11（取消期限归属）/ F13（知识提交）
- 复审身份：全新独立 Reviewer，不延续任何前序执行者/评审者结论；worktree `/tmp/lingxi-rv4`，复核期间 `git status` 全净（无源码漂移）。
- 判据：义务全满足 PASS；缺口 FAIL；仅外部条件 BLOCKED。

## 一、亲跑结果（合成环境 /tmp/rv4-env，--locked，--test-threads=1）

| 套件 | 结果 | 对应 |
|---|---|---|
| r06_ar_w09_embedding_batches | 16/16 PASS | F09 + F11 面 |
| r06_ar_w10_embedding_identity | 9/9 PASS（含 C01–C04 全绿） | F10（R1 FAIL 四条重放） |
| r06_ar_w11_embedding_lifecycle | 11/11 PASS | F11（含 FIX-1/2/3 面） |
| r06_ar_f13_submission | 2/2 PASS | F13 |
| r06_t07_knowledge_tools | 6/6 PASS | F13/T07 工具面 |
| r06_original_knowledge_proofs | 12/12 PASS | 原 TS 语义 proofs |

合计 56/56 全绿，与 AR_HANDOFF D3b 声称的 w09 16/16、w11 11/11、f13 2/2、t07 6/6、w10 9/9 一致。我方运行自产回执已落 /tmp/rv4-env/{w09,w10,w11}/，其中 `w09/c02-quota-cancel.json`（ledger []、wire []）、`w11/c03-quota-true.json`（0 行、0 线路、Cancelled）、`w11/c03-quota-false.json`（1 行 unknown、transport_attempts 0）与断言语义逐项吻合。

## 二、逐义务源码抽查（对照 AR_ISSUE_MATRIX remaining_scope）

### F09 批处理（4/4 满足）
1. 首批探测消费迭代器：`knowledge.rs` 摄入循环为 `for batch in missing.chunks(EMBED_BATCH)`（missing=计划块−检查点），全文件无 peek/peekable；每个计划块在同一成功构建中只提交一次。c01 边界金样（1/64/65/128/129）实测通过。
2. 统一统计：stats 含 requestCountSemantics=`confirmed_or_possibly_sent_excluding_known_not_sent`、transportAttemptCount、confirmed/knownNotSent/unknownSent/pending 全谱计数，未见以删计数或调 expected 掩盖。
3. 缺口续嵌：检查点差集只补缺口（c02_failed / c02_sigkill 两测试）；不兼容身份另起新 variant 不混用（c04 changed-model/endpoint/protocol-parameter 各自全部重嵌且旧付费字节保留）。
4. ready 快速路径：`run_embedding_phase` 用已实测 variant 维度做身份学习（零维度=未发送配置，不可冒认 ready 命中），复用判定纯本地（get_ready_variant_metadata + block fingerprint），c04 断言 `stub.capture()` 零增量（"ready reuse must not pay for a dimension probe"）。

### F10 身份（4/4 满足，R1 四条 C01–C04 重放绿）
1. 同源身份：`embedding_operation.rs::embed_owned_observed` 只 resolve 一次 route，同一快照用于 build_embedding（真实 dispatch）、parse、journal 回执与返回的 `ServedEmbeddingOutcome.route`——不存在"事后贴标签"。
2. 每批核验：每批后 `identity != served` → fail_build + conflict「embedding identity changed; preserve old paid results and rebuild a distinct variant」。
3. 同维度隔离：身份 key 含 `lingxi.embedding-identity.v2` 全量描述符（provider/model/protocol/dimensions+parameter_fingerprint），同名不同指纹不冒认（c02 same-dimension 重载不污染首 variant）。
4. 全链：查询侧 served 身份+维度隔离（c03）、失败构建改身份重启不复用旧空间（c04）、**C04 恢复缺口关闭**：`crash_running_job_recovers_distinct_identity_and_preserves_paid_vectors` 以 SIGKILL 杀飞行中进程→model-b 重启→冗余手动 reingest 诚实 409→崩溃 RUNNING 作业由 worker 真实恢复：旧 A 付费行逐字节保留、新 B 行全量独立、wire 记 2 次旧 A 尝试（含 unknown）+全部 B 批次，与 W09 `c02_sigkill_running_recovers_only_gaps` 互证。R1 FAIL 的「queued-only claimant 不构成恢复」缺口已有运行时证据关闭。

### F11 取消期限与归属（4/4 满足）
1. 上下文传播：`EmbeddingCallRuntime{ownership(principal/session/run/attempt/parent_tool/cause_ref/deadline_unix_ms), cancel, request_key}` 贯通 embed_owned/embed_owned_observed；模型参数无自报身份入口。
2. 两类归属均非 None：摄入 job = 服务铸造稳定身份（cause_ref=`knowledge-job:{studio}:{jobId}`、attempt=`job:{id}#a{n}`）+ 持久化绝对期限（ensure_job_active 每批检查「deadline exhausted; paid checkpoints preserved」）；独立查询/宿主入口 = 服务铸造 cause + 300s 绝对期限 + background_root；运行内检索经 `search_in_scope_owned` 继承调用方 runtime（cancel.rs 提供 run_root/child 取消树）。
3. 迟到栅栏：配额/凭证等待取消响亮（W11 c03）、deadline 不续期（c03 network deadline is not renewed）、父取消单次结算（c03 parent cancel settles once）、native FIFO 保留 claim。
4. 真实归属：成功/失败/超时/取消-unknown 均落账不记 0；预发送已知未发送取消不虚构行（见 FIX-2 裁定）。

### F13 知识提交（4/4 满足）
原缺陷是「方法级测试构造、真实 execute 提交未闭环」。现行 `r06_ar_f13_submission` 走**真实认证 POST /lingxi/v1/sessions/{id}/execute + knowledge 选择**：
1. 最小完整提交语义：服务侧按轮冻结 scope+资料版本、绑定主体/会话/Run（run_binding.attempt 随重试推进），实际模型请求 system 段注入服务铸造范围句柄（「本轮知识资料范围：」在真实 wire 上断言）。
2. 兼容扩展：execute body 的 `knowledge{notebookIds,sourceIds}` 扩展字段，无界面重写依赖。
3. 每次执行校验：旧 scope 工具调用 → 真实 Failed 回执 + 下一模型请求收到 KNOWLEDGE_SCOPE_VIOLATION；非法选择（源真实存在但不在所选笔记本）→ 发模型请求**之前**拒绝、run Failed、上一 active scope 不被破坏；未选资料不声明检索工具。
4. 规范化记录与恢复：invocation journal 留痕（成功/拒绝均真实回执）；500 重试只推 attempt、失败窗口内资料被移出笔记本+新增源后仍冻结原 sources（content_snapshot_id/parse_artifact_id 逐字节相等）。

## 三、FIX-2 裁定复核（W11 c03 断言改为空台账）——独立意见：裁定正确（AGREE）

改动本身（git diff 6f5c4b01d→844a10b2b）：W11 c03 原断言要求取消与期限两支都恰好 1 行 `transport_attempts=0, usage_state=unknown`；冻结候选把「取消支」改为 `usage` 空（"pre-send cancel must not fabricate a usage row"），期限支保留原 1 行 unknown 断言。实现为 `record_embedding_receipt` 的窄守卫（embedding_operation.rs:614-618）：`cancelled && transportAttempts==0 && knownNotSent==true` 跳过费用行。

我方独立查证：
1. **W09 冻结证据**（副本内 `artifacts/rust-tauri/R06/AR/W09-E01/safe/commands/complete-family-09/c02-quota-cancel.json`，2026-10-11，早于本次改动）：配额等待取消作业 `pendingCallIntentCount:1, requestCount:0`，`wire:[]`，**`ledger:[]`** —— W09 冻结态即不写费用行。配套 `presend-construction-11/c01-presend-build.json` 显示构造失败（failed 非 cancelled）仍写 `transport_attempts:0, usage_state:unknown` 行，与现行窄守卫逐字一致。
2. **TS 原语义**（修复真实性锚点，非 Rust 自证）：
   - `core/model-operation-client.ts`（BaseModelOperationClient）：`resolveOperationFresh`（凭证/路由解析）在 `withModelRequestAccounting` **之前**；`usageLedger.start()`（lib/llm/usage-ledger.ts）只在 `run()`（真实 fetch）前一刻调用。等待期取消 → start 从未发生 → **无台账行**。
   - `lib/knowledge/ingestion-service.ts:907-914`：provider gate（§十六限流等待）`embeddingGate.run(gateKey, fn, abort.signal)` 包裹 `embedTextsForModel`，gate 排队取消同样到不了 client。
   - 上线路径中止：AbortError → `recordError(status="aborted")` → 写诚实 unknown 行。
   - 即 TS 语义 = 「预发送取消不写行；上过线中止诚实 unknown」，与 Rust 现行为完全同构。
3. **结论**：W11 c03 原断言（预发送取消也写 unknown 行）既不符 W09 冻结证据也不符 TS 原语义，属 Rust 侧自造的更严（且语义上错）的合同；按有冻结证据方统一是**纠错而非放松**——取消支仍断言响亮退出、零线路请求，期限支的 unknown 行断言原样保留。事实不丢失：预发送取消仍留 embedding-calls journal（save_journal 先于记账）与作业统计（pendingCallIntentCount/knownNotSentAttemptCount）；且 unknown 类专用于「可能已付费、结局未确认」并驱动 unconfirmed_paid 恢复守卫（FIX-1 同域），把确证未发送的取消灌入该类会虚假阻断同 attempt 重试。裁定维持。

## 四、F09/F11 三修复核

- FIX-1（跨 attempt 恢复守卫）：embedding_operation.rs:1149-1209 —— `unconfirmed_paid && !prior_attempt_closed` 拒绝自动重发；`prior_attempt_closed`（旧 attempt 已被 claim 互斥+join 关闭）时旧 unknown 先结算归档，新 attempt 以全新 call identity 补缺口；同 attempt 内仍拒绝。与 handoff 描述一致，W09 c02_sigkill/W10 crash-running 实测支撑。
- FIX-2：见上节，AGREE。
- FIX-3（pending 统计保留）：knowledge.rs:2558-2567 —— 父取消落在飞行中时 `pendingRequestCount` 不本地折入 unknown（仅本进程得到确定性结算才收敛），交接管 attempt 以 `unknownRequestsFromPreviousAttempt` 归类。实现与声称一致。

## 五、残留观察（不构成本四条 FAIL）

1. AR_ISSUE_MATRIX.json 四条条目的 status 字段仍停在修复前快照（如 F10=`INDEPENDENT_REVIEW01_FAIL...`、F13=`OPEN`、F09 self_check=`EXECUTOR_SELF_CHECK_RECORDED...`），与冻结候选实态不符——属台账滞后，非代码缺口；收口提交时应同步刷新。
2. F13「重启后只读恢复」由持久 turn_scope 行 + journal 回读覆盖（f13 两测试 + proofs），未见显式跨进程重启恢复专测；现有 SIGKILL 恢复证据集中在嵌入面（F09/F10）。记录为可选加固，不属 F13 remaining_scope 强制项的缺口（其要求「进入规范化记录、恢复准确」已由持久化+journal 断言满足）。
3. 遗留两相邻红按 handoff 归属他处（cancel_link_inheritance→F14、original_knowledge_proofs 预算→已修为 max_model_turns=64 且我方 12/12 复验绿），不在本四条范围。

## 六、总裁定

| 条目 | 判定 |
|---|---|
| R06-AR-F09 | **PASS** |
| R06-AR-F10（含 R1 C01–C04 重放与恢复缺口） | **PASS** |
| R06-AR-F11（含 FIX-1/2/3） | **PASS** |
| R06-AR-F13 | **PASS** |
| FIX-2 跨套件裁定 | **AGREE（维持 W09 冻结语义）** |

四条义务在冻结候选 844a10b2b 上均有亲跑绿测 + 源码同构证据；未发现虚构修复、静默降级或以改断言换绿（唯一断言改动经独立查证为纠错）。本复审仅覆盖上述四条及其指定面，不构成 R06 阶段门禁、R07 或其他 AR 条目的通过声明。
