# R05-T07 — 用量、trace 及持久化语义（MODEL_USAGE_SEMANTICS）

版本：2026-10-03。本文是 R05-T07 的实现语义文档：usage 的归一口径、trace 因果、
持久化 schema 与写序契约。机器可读的公式表在
`rust/crates/lingxi-adapters/src/models/usage.rs` 的 `USAGE_MAPPINGS`（测试
`r05_t07_usage_families.rs` 逐字段断言，本文与其同源，改一处必须同时改另一处）。

## 1. 一个 ModelCall 一条台账

每次真实模型请求（物理 HTTP 请求）都在 `model_call_usage` 台账中可追溯：

- 主对话（chat plane）：run driver 在每个 model call 完成（Final /
  ToolRequests / Continue / Empty / Failed 全部五类终局）时写一行；
- worker 回调：`GatewayWorkerModel` 在把答案交还 worker 之前经
  `LedgerWorkerCallbackTrace` 写一行（写失败=回调显式失败，不静默丢账）；
- 操作面（embedding / rerank / image / video / speech / transcribe）：
  `OperationService` 在每次 dispatch 得到响应（或失败）后写一行
  （`origin=operation`，无 session/run 归属——属内部记账，owner 范围查询看不到）。

行的身份列：`session_id` / `run_id` / `attempt` / `model_call_id`（宿主铸造）
+ `purpose`（`chat` / `auxiliary.{slot}` / 操作名）+ `origin`（`user` /
`subagent` / `worker-callback` / `operation` …，沿用 run lineage 词表）+
`parent_run_id` / `cause_ref`。父子关系只来自 driver 的 lineage 事实，
绝不按时间相近推断（C02）。

## 2. usage 归一：供应商字段 → 统一字段公式（C07）

| 协议族 | input | output | cache_read | cache_write | reasoning | cache 计入口径 | reasoning 计入口径 |
|---|---|---|---|---|---|---|---|
| openai-completions | `usage.prompt_tokens` | `usage.completion_tokens` | `usage.prompt_tokens_details.cached_tokens` | — | `usage.completion_tokens_details.reasoning_tokens` | **含于** input | **含于** output |
| openai-responses | `usage.input_tokens` | `usage.output_tokens` | `usage.input_tokens_details.cached_tokens` | — | `usage.output_tokens_details.reasoning_tokens` | **含于** input | **含于** output |
| openai-codex-responses | 同 responses | 同 responses | 同 responses | — | 同 responses | **含于** input | **含于** output |
| anthropic-messages | `usage.input_tokens` | `usage.output_tokens` | `usage.cache_read_input_tokens` | `usage.cache_creation_input_tokens` | —（无字段，不造） | **不含于** input（独立计费类目） | — |
| google-generative-ai | `usageMetadata.promptTokenCount` | `candidatesTokenCount + thoughtsTokenCount`（RR1 F23 归一） | `usageMetadata.cachedContentTokenCount` | — | `usageMetadata.thoughtsTokenCount` | **含于** input | **wire 上分字段；归一后含于** output |

聚合规则：**"含于"的分量子集已计入统一总量，任何聚合都不得再相加**——总量
即聚合值；anthropic 的 cache 两类是不含于 input 的独立类目（有效计费输入 =
input + cache_read + cache_write，消费方按需相加，统一字段保持分离以免盲目
重复计入）。组件字段缺失即 `None`（"未报告"），不是 0。

**Google 口径（RR1 F23 修订）**：Google 在 wire 上把候选输出
（`candidatesTokenCount`）与思考（`thoughtsTokenCount`）**分字段**报告，
`totalTokenCount = prompt + candidates + thoughts`（官方 thinking 文档：输出
成本同时计入 output 与 thoughts）。解码头把统一 `output_tokens` 归一为
`candidates + thoughts`（总生成输出），因此：
- 消费方计费输出 = `output_tokens`（与 OpenAI 家族同一约定，绝不漏掉 thoughts
  半边，也绝不把 thoughts 再加一次）；
- `reasoning_tokens` 保留 thoughts 分量（组件事实不丢）；
- 流式运行快照的回写（accumulator→缓冲体重放）按 `output - reasoning` 拆回
  候选字段，重解码恰好还原 fold（不二次相加）。

操作面（embedding/rerank 等）按 incumbent normalize 后的 JSON 取数：
`input_tokens`（缺则 `prompt_tokens`）/ `output_tokens`；只有 `total_tokens`
时两半都保持未知（总量绝不靠猜拆分）。RR1 F22 修订：normalize 只做**形状选
择**，绝不做数值转换——供应商原始值（null/字符串/浮点/容器）原样进入严格
解码器，非法即整条 `invalid`（null 视为缺失，"null 必须保持缺失"）；
`total_tokens` 只在两半都是真非负整数时才合成（缺失半边绝不按 0 参与合成）。
非法诊断只点名字段路径/类型/原因并**不回显 payload**（provider 可能回显凭证
材料；类型+长度即足够定位）。MiniMax 的 `total_tokens` 同样原样透传（不经
f64 转换，整数精度无损）。

## 3. 流式聚合模式（C04）

| 协议族 | 缓冲模式 | 流式模式 | 语义 |
|---|---|---|---|
| openai-completions / responses / codex | FinalSnapshot | FinalSnapshot | 终止帧的 usage 就是本请求最终值：相同重发=幂等 no-op，不同第二份=响亮冲突（不合并） |
| anthropic-messages | FinalSnapshot | RunningTotal | `message_start.usage` 是输入半边（含 cache 类目），`message_delta.usage.output_tokens` 是**累计**运行总量：增长→替换（绝不求和），相同→no-op，回退→响亮冲突 |
| google-generative-ai | FinalSnapshot | RunningTotal | 各 chunk 的 `usageMetadata` 是累计快照：按字段单调替换，回退=响亮冲突 |

`UsageFolder`（`lingxi-kernel/src/usage.rs`）实现上述两条规则；累计快照永不
求和、重复完成事件不重复记账、同一响应重传不双计（C03/C04）。

## 4. 未知/部分/估计/零/非法（C05/C06）

- `reported`：两半总量都来自供应商；
- `partial`：只到半边（如流在 `message_start` 后中断）——已知半边保留，
  缺失半边 `None` 并按名列出（`missing_fields`），**绝不写成 0**；
- `estimated`：宿主推算值（当前链路没有产生它的调用点；字段与 basis 预留）；
- `unknown`：没有可用 usage 事实（响应无 usage / 请求失败 / 流中断无可信数）；
- `invalid`：usage 对象到了但数字违反契约——负数、非整数（含浮点与科学计数
  溢出到 f64 的表示）、字符串。**处理：响亮记账不响亮失败**——turn 照常
  交付，行记 `invalid` + 违规细节（点名字段与性质），任何数字都不被信任、
  不截断、不饱和、不归零。

供应商报告的 0 是 0；缺失是 `None`。wire 事件（`model_call_completed.usage`）
只在两半总量都已知时投影数字，其余一律 null——UI 不会把未知读成 0。

## 5. 物理请求与逻辑调用（C03）

- `transport_attempts`（RR1 F38 起**可空**，migration v7 表重建）：一个
  逻辑 model call 实际发出的物理请求数。
  **0 = not-sent**（RR1 F21：路由解析/逐模型能力/凭证解析在派遣前拒绝、
  operation 排队在总预算内超时——什么都没离开进程，绝不虚构 1 次）；
  ≥1 = 已交给传输层；401-refresh 重试（`GatewayedProvider` 对持证路由的
  单次协调刷新）计入同一行（`transport_attempts=2`），usage 取成功那次
  ——两个可能计费的请求不会并成一条不可见记录；
  **NULL = 尝试数未知**（RR1 F38：call 的 future 在结算前被 drop——run
  取消竞态掐断在途 SSE、worker invocation deadline 到期/drop 在途回调——
  离开进程的物理请求数无法观测；绝不写 0 冒充 not-sent、绝不写 1 冒充
  已发一次）；
- **失败调用不漏账**（RR1 F21）：worker 回调的失败路径（500、pre-send
  拒绝）同样先写台账行再返回拒绝（usage=unknown、真实 attempts、resolved
  identity 随行）；**解析失败不丢 usage**（RR1 F38 同路径修复）：流式
  回合缓冲体重建后的 parse 失败（如无工具调用收到工具响应、非法批次）在
  五族一律把该流**已观测到的 usage 事实**（严格解码：合法=Known/Partial、
  违规=Invalid、无=Unknown）附到 Failed 结果上——回合照常响亮失败，计费
  事实不随 parse 错误消失；
- **取消不抹账**（RR1 F38，回应原审计 F-WU02）：
  - run driver 在 model call 流中被取消（select 竞态臂）：`settle_cancellation`
    之前先为该 call 落一行（outcome=`cancelled`、usage unknown、attempts
    NULL、dispatch-moment 身份）；取消后 fence 拒绝的迟到回合（Stale+
    cancelled）：行携带回合**真实观测到的** usage/attempts/身份，outcome=
    `cancelled`。两处都**只写台账行、不写 `model_call_completed` 事件**——
    A09/C16 的『取消的 call 绝不以伪造 completed 事件收尾』纪律保持不变，
    run 自身的 `cancelled` 终态收束事件流；
  - worker 回调在途中被 invocation deadline 掐断：RPC 层（`workerrpc`）
    经 `WorkerModelPort::abandoned` 落行（invocation/cb_id/parent_tool_call
    在作用域；路由未解析按 workermodel 既有 `unreported` 惯例）；run 取消
    drop 整个 execute future 时，回调的 RAII 弃置守卫把行**脱离到运行时**
    尽力落账（失败响亮记日志，绝不静默消失）；
- run 级重试（retryable 失败→新 attempt）：每次重试是新的 model call
  （`mc0001` / `mc0002`…），各自一行；失败行 usage=unknown（不是 0）；
- 已知边界（登记）：进程在 model call 中途被 kill（崩溃窗口）时，该 call
  有 `model_call_started` 事件但没有完成事件，也没有 usage 行——**只有
  崩溃窗口**如此（冻结 HEAD 的原登记即此一条）；正常取消（cancel-before/
  after-send）自 RR1 F38 起都有行。崩溃窗口的意图先行记账属后续阶段的
  InvocationJournal 同型改造（见 PROGRESS_LEDGER followup）。

## 6. 费用（C08）

本阶段没有任何价格来源（配置无价格表、无币种/版本口径），因此：
- token 事实照常记录；
- `cost_basis` 恒为 NULL（=unknown）；台账不含任何金额/价格列
  （`c08` 测试断言 schema 无 price/cost/amount 列）；
- 未来接入计价时在 `cost_basis` 命名来源，绝不硬编码市场价格。

## 7. 持久化（C09/C10）

### 7.1 Schema（migration v5 + v6 + v7，`model_call_usage`）

```
model_call_usage(model_call_id PRIMARY KEY, session_id, run_id, attempt,
  purpose, origin, parent_run_id, cause_ref, parent_tool_call_id,
  provider, model, protocol, usage_state, input_tokens, output_tokens,
  cache_read_tokens, cache_write_tokens, reasoning_tokens, missing_fields,
  estimate_basis, invalid_detail, transport_attempts, outcome,
  started_at_unix_ms, settled_at_unix_ms, emitted_tool_calls, cost_basis,
  recorded_at_unix_ms)
```

- 刻意无外键：记账是审计型事实（worker/操作面行没有自己的 run 行；
  与 stale_result_audit 同立场）；
- token 列可空：NULL=未报告（不是 0）；SQLite INTEGER 是 i64——超出
  i64 范围的 u64 事实被响亮拒绝（`InvalidRequest`），绝不截断；
- **RR1 F21 追加列（migration v6，ALTER 追加）**：
  - `outcome`（`succeeded|failed|cancelled|unknown`）——调用结算状态，
    与 usage 事实分离（500 响应可能带完整 usage；成功调用可能 unknown）；
  - `started_at_unix_ms` / `settled_at_unix_ms`——宿主观测的开始/结算
    时刻（耗时=差值；NULL=未观测，如崩溃窗口行）；
  - `parent_tool_call_id`——child（worker 回调）行的**真实父工具
    JOIN 键**（driver 铸造的 ToolCallId，经真实 worker RPC 传递，绝不用
    随机 RPC request id 冒充）；
  - `emitted_tool_calls`——父 model 行（chat 工具回合）列出其批次发出
    的 tool call id；child→parent_tool→parent MODEL 的两跳 JOIN 全部在
    台账内闭合（绝不按时间相近推断）；
  - 旧版本写入的行 `outcome` 默认 `'unknown'`（诚实状态）；
- **RR1 F38 追加（migration v7，表重建）**：`transport_attempts` 改为
  NULLABLE——NULL=drop 后尝试数未知（见 §5），旧行原值保留、不重写不
  丢弃，索引同构重建（`r05_t07_persistence.rs::
  migration_v6_to_v7_makes_attempts_nullable_and_keeps_rows`）；
- 迁移：v4 数据库（隔离副本）开库即升 v5/v6/v7，旧行不动
  （`r05_t07_persistence.rs::migration_v4_to_v5_…`、
  `…::migration_v6_to_v7_…`）；
- 模型配置（config 文件 / gateway 重载 / 服务重启换新 plane）不重写、
  不删除既有账行——记账 append-only；同一 `model_call_id` 重写不同内容
  = `StorageError::Conflict`（`identical_replay_is_idempotent_…`）。

### 7.2 查询与授权隔离（C09）

`StoragePort::query_model_call_usage(ModelUsageQuery)`：
- `owner_user_id` 范围经 sessions 的 owner JOIN 过滤——越权主体的行
  **不返回**（不是过滤后正文）；
- `session_id` / `run_id` 进一步收窄；行按写入顺序返回；
- **RR1 F21 筛选面**（任务书"按日期/类别/模型/会话筛选"）：
  `purpose`（类别精确匹配）、`model`（模型精确匹配）、
  `recorded_from/to_unix_ms`（`recorded_at_unix_ms` 闭区间日期窗口）
  可与任意 scope 形状组合；返回行携带 `outcome`/`started_at`/
  `settled_at`/`parent_tool_call_id`/`emitted_tool_calls` 全部新列；
- 操作面调用上下文（RR1 F21）：`OperationService::embed/rerank` 接受
  可选 `OperationCallContext`（session/run/attempt/cause_ref）——带上下
  文的行是 session 级记账（可 JOIN 到 run），无上下文保持合法独立根
  （内部记账，owner 范围查询看不到）；
- 无 session 的操作面行只对内部未限定查询可见。

导出/正文红线：台账只含数字与身份字段，无内容块、无 opaque、无凭证；
`r05_t07_usage_trace.rs::c09` 用整个 runs.db 字节扫描断言 API key 材料
从未进入数据库（密钥只存在于凭据库文件）。现役观测 UI 不在本阶段重做
（任务书明示）；查询/导出所需的字段与脱敏规则以本台账+既有 redaction 为准。

### 7.3 写序（C10）

**先提交，再发布**：usage 行的 `record_model_call_usage` 提交成功之后才
`record_run_events(model_call_completed)` 并 `publish_committed`。任一 DB
写失败：
- 不发布完成事件（订阅者看不到"成功"）；
- run 以 `DriveError::Storage` 响亮失败；
- 重试写入幂等（同 id 同内容=重放 no-op）。

worker 回调同规则：台账行写失败 → 回调以
`the usage ledger refused the callback's accounting row` 显式拒绝，
worker 不收到成功答案。

## 8. 与 wire 契约的关系

`lingxi_protocol::UsageRecord`（`{inputTokens, outputTokens}`）保持冻结：
它是 `model_call_completed` 事件的投影，只在两半已知时携带数字。更丰富的
事实（组件/口径/未知性/非法标记）在 kernel `usage` 模块与台账中，二者由
`ParsedChat::usage()` / `ModelCallUsage::wire_record()` 单向推导——投影永不
独立断言。

## 9. 测试映射（C01–C10）

| 检查点 | 测试 |
|---|---|
| C01 | `r05_t07_usage_trace.rs::c01_multi_run_session_trace_stays_continuous_and_per_call_queryable` |
| C02 | `…::c02_background_subagent_and_worker_rows_carry_true_parentage` |
| C03 | `…::c03_refresh_retry_is_two_physical_requests_in_one_honest_row`、`…::c03_retryable_failure_then_success_are_two_rows_and_unknown_is_not_zero` |
| C04 | `r05_t07_usage_families.rs` 的 c04_*（anthropic 运行总量替换/回退响亮、gemini 运行快照、openai 终值幂等/冲突）+ kernel `usage::tests` |
| C05 | `…::c05_unknown_partial_and_reported_states_stay_distinct_everywhere`（台账三态 + wire null） |
| C06 | `…::c06_illegal_usage_numbers_mark_the_row_invalid_and_never_distort` + families 的 c06 |
| C07 | families 的 c07_*（公式表 + 各族夹具） |
| C08 | `…::c08_cost_stays_unknown_without_a_price_basis` |
| C09 | `…::c09_owner_scoped_queries_isolate_and_secrets_never_enter_the_db` + `r05_t07_persistence.rs` 迁移/幂等/换配置不覆写 |
| C10 | `…::c10_a_ledger_write_failure_publishes_no_completion_and_the_retry_is_idempotent` |
| RR1 F38 | `r05_t07_rr1_usage_ledger.rs::rr1_f38_driver_cancel_of_in_flight_stream_leads_a_cancelled_row_across_restart`（driver 取消腿+重启）、`…::rr1_f38_worker_deadline_expiry_of_in_flight_callback_leads_a_cancelled_row`（worker deadline 腿+重启）、`…::rr1_f38_run_cancel_drop_of_in_flight_callback_leads_a_cancelled_row`（run 取消 drop 腿：RAII 守卫脱离落账+重启）、`r05_t07_persistence.rs::migration_v6_to_v7_makes_attempts_nullable_and_keeps_rows`（v7 可空迁移）、`…::rr1_f21_unexpected_tool_response_failure_keeps_the_reported_usage` / `…::rr1_f21_partial_usage_survives_a_failed_aux_settlement`（F21 自检补腿） |

## 10. 登记的边界

- **崩溃窗口**（进程在 model call 中途被 kill：started 事件无 completed
  事件、无 usage 行）——这是本文件冻结 HEAD 时登记的唯一无行边界，RR1
  F38 后依旧如此（意图先行记账属后续阶段 InvocationJournal 同型改造）；
- **正常取消窗口自 RR1 F38 起都有行**（订正记录：此前 R1 轮曾把『driver
  取消窗口无行』写成与崩溃窗口同界的既有登记——不实，冻结 HEAD §10 只
  登记过崩溃窗口；F38 修复轮已把该窗口关闭，见 §5）：driver 取消竞态/
  fence 迟到回合写 outcome=`cancelled` 行（attempts NULL 或真实值），worker
  deadline 到期/drop 在途回调经 `abandoned` 落行；**不写** completed 事件
  （A09/C16 纪律：取消的 call 不以伪造 completed 事件收尾）；
- run-cancel drop 整个 worker execute future 时的弃置行是**脱离到运行时
  的尽力落账**（RAII 守卫 + spawn；写失败响亮记日志）——进程死亡本身仍属
  崩溃窗口；
- worker/operation 面的取消前/排队超时拒绝**有**行（attempts=0，§5）；
- 操作面计费按"每次 dispatch 一行"，媒体任务的状态轮询（query）不记账
  （无 usage 对象、按 incumbent 口径不属计费请求）；媒体（image/video/
  speech/transcribe）行带 `outcome`，但 `started/settled` 为 NULL（其
  R07 业务面落地时补齐观测，登记于此不虚标）；
- `estimated` 状态当前无生产调用点（字段与序列化已就位并被 round-trip 测试
  覆盖）；
- 正式组合根的 worker 工具注册（模型回合直接调用 worker 工具）归 F24；
  本包的父子 JOIN 经真实 worker 子进程链 + 真实 run driver 证明
  （`r05_t07_rr1_usage_ledger.rs::rr1_f21_worker_parent_join_…`）；
- LIVE 供应商实测未获授权（RR-BLK-CREDENTIALS，最迟 R10）——本阶段全部
  证据来自离线/受控替身链路。
