# R06-T04 设计 — 统一消息历史、重连与导出

- TASK_BASE_SHA: `2cb2b064061e902ea18faa0ec38499ce32436cb7`
- 验收: R06-A07（实时/重开/重连/导出语义一致）、R06-A08（长历史分页不重复全扫）
- 交付物: CanonicalMessageStore；统一历史投影；分页/导出接口

## 1. 勘察结论（RC-1 端到端现役链）

### 1.1 现役（Node）四个读取方式的真实载体

| 方式 | 现役实现 | 锚点 |
|---|---|---|
| 实时 | core/events.ts ReservedTagScanner → WS 广播 assistant 事件 | `core/events.ts:20` |
| 重开（历史分页） | `/sessions/messages`（before/limit≤200/all=1/ETag 304）→ history-read 目录快路径 | `server/routes/sessions.ts:1383-1513` |
| 重连 | WS resume_stream(streamId/sinceSeq) → turn 环缓冲 5000 事件，reset/truncated 标记 | `server/session-stream-store.ts` |
| 导出 | **现役无会话导出端点**（observability/channels 的 export 属其他域）；T04 的"导出"是本任务新建的统一投影全量读出 | 全库检索无会话级 export |

现役的 MOOD/思考清洗在**桌面客户端各自进行**（`desktop/src/react/utils/message-parser.ts:51`、
`assistant-block-builder.ts:34`、`format.ts:112`、`history-segment-sanitizer.ts:28`）——正是
任务书 02 §3 禁止的"各客户端各自清洗MOOD/思考"。候选侧把解析权收回服务端
（R05 已冻结：同一 scanner，mood 块在入口即结构化剥离，不进入事件流——R05 D5）。

### 1.2 候选侧已持久化事实面（T04 的输入）

- `messages`（v8，PK(session_id,message_id)）：user 消息（`user:{run_id}`，run_id NULL，
  仅 RunOrigin::User 铸造，runs.rs:1108-1115）、final 消息（`{run_id}-final`，content_json 为
  已规范化 NormalizedMessage，与 run 终态同一事务，run_store.rs:1546-1640）、reset 标记
  （entry_type='hana-session-branch-reset'，content {reason,to,sourceEntryId}，
  session_tree.rs:878-943）。索引 idx_messages_session(session_id,seq)、
  idx_messages_parent(session_id,parent_message_id)。
- `key_events`（UNIQUE(stream_id,seq)，stream_id=session_id）：12 种已知事件含
  assistant_segment_start/delta/end（segment_id=`assistant:{turn}:{reasoning|text}:default`，
  phase=reasoning/commentary/final_answer/unresolved）、tool_call_started/completed
  （ToolCallDescriptor/ToolResultWire，tool_call_id=`{run_id}-tcNNNN` 全局稳定）、
  run_state_changed、final_message_committed。索引 idx_key_events_run(run_id)、
  idx_key_events_session(session_id,seq)。**生产无自动 purge**（purge_events_before 仅
  管理面 API），事件可被视为完整。
- `run_lineage`（parent_run_id 索引）：子代理 run 与父 run 同会话同事件流
  （subagents.rs:484,775-792 共用 drive_run），子 run 无 user 消息、有 final
  （织入父会话分支链）。关联子 run 的唯一确定性来源是 lineage，**不按时间猜**。
- `session_branch_heads`：头指针 + revision（每次推进 +1）——天然 ETag 材料。

### 1.3 现状缺口（T04 要填的）

1. `/branch` 只投影 user/final/reset 三类消息行——**工具卡与过程段不在历史投影里**，
   且无分页（整链返回）。
2. `branch_id_chain` 对 parent 全 NULL 的 legacy 会话只能走到尾消息一行——v8 迁移
   把 pre-T03 行全部置 parent_message_id=NULL，需要 LegacyReader 规范化（任务书步骤①）。
3. 无导出接口；无统一投影函数；A07 要求的四方式一致性没有承载体。

## 2. 验收映射（Requirement → Production Entry → Owner → Implementation → +Test → −Test → Evidence）

| Req | Production Entry | Owner | Implementation | Positive Test | Negative Test | Evidence |
|---|---|---|---|---|---|---|
| A07 四方式一致 | GET /history、GET /export、WS subscribe/resume、execute 实时流 | service `history.rs` + `history_projection.rs` | 同一投影函数消费同批持久事实 | `r06_t04_history.rs::a07_realtime_reopen_reconnect_export_agree` | 正文混入 mood/think → 断言失败（fixture 内置 `<mood>`/`<think>` 原文） | 05_targeted_tests |
| A08 索引分页 | GET /history?before&limit | adapters `canonical_message_store.rs` | 游标走 PK/索引单步回溯，页成本 O(页大小) | `canonical_message_store.rs::a08_consecutive_pages_lossless` + `a08_page_cost_independent_of_history_length` | 游标伪造/跨会话 → 400/403 | 05 + I/O 探针输出 |
| 步骤① 权威结构 | messages+key_events（不建新表） | adapters | 工具/终态经事件 join 进投影 | 工具卡含稳定 toolCallId/结果/终态 | 无 final 的 run 不编造 final（RunTerminal 如实） | 05 |
| 步骤② 共享投影 | 四方式共用 `project_history` | service | 实时捕获事件 → 同一投影函数 | 四方式规范化序列 diff 为空 | 前端无需解析（服务端产物即最终语义） | 05 |
| 步骤③ streamId/seq/窗口 | 保留 /events + WS resume 不动 | service(既有) | 新 /history 不替代既有事件面 | 既有 event_subscription 回归全绿 | — | 06 |
| 步骤④ legacy/unknown | LegacyReader（内容重切+seq 线性回退+run 关联三态） | service projection + adapters walk | 未知顶层字段入 legacyFields；不可解析行 → legacy_raw 原文；run 关联 unknown | fork 副本 run 关联 unknown；legacy mood 剥离 | 不按时间猜关联（无时间字段参与判定） | 05 |
| 步骤⑤ 工具卡稳定ID | tool_call_id=`{run_id}-tcNNNN` 全程不变 | projection | 实时/历史/导出同一 id | 工具卡 id 跨方式相等断言 | 仅工具无回复的 run 可展开（有 ToolCall+RunTerminal 项） | 05 |

## 3. 组件设计

### 3.1 协议层 `lingxi-protocol/src/history.rs`（新）

线型（serde，`deny_unknown_fields` 仅用于请求侧；投影产物宽松读取由 service 控制）：

- `HistoryItem`（`#[serde(tag="kind", rename_all="snake_case")]`）：
  - `UserMessage { message_id, seq, committed_at_unix_ms, text, run: RunAssociation, legacy_fields }`
  - `AssistantSegment { segment_id, run, phase, segment_kind, text, complete, first_seq }`
  - `ToolCallItem { tool_call_id, run, target, args_summary, args_digest, status(ToolResultStatus|Started), result: Option<ToolResultWire>, first_seq }`
  - `FinalMessage { message_id, seq, run, model_call_id, content: Vec<ContentBlock>, committed_at_unix_ms, legacy_fields }`
  - `RunTerminal { run_id, status, terminal_reason, event_seq }`
  - `ResetMarker { message_id, seq, reason, to, source_entry_id }`
  - `LegacyRaw { message_id, seq, raw: serde_json::Value, reason }`（步骤④原始档案）
- `RunAssociation { state: known|unknown, run_id?, reason? }`——**只有** run_id 列、
  本会话 id 约定（`user:`/`-final`）且 run 实存三种确定性来源给 known；其余 unknown。
- `HistoryPage { schema_version, session_id, head_revision, items, page{limit,has_more,next_before} }`
- `HistoryExport { schema_version, session_id, head_revision, items }`（items 与分页全集一致）
- `HistoryCursor { message_id, seq }`：canonical JSON → base64url(no pad)，严格 decode
  （镜像 SubscribeCursor 机制，events.rs:205-226）。
- schema 版本常量 `lingxi.canonical-history.v1`。

### 3.2 存储层 `lingxi-adapters/src/storage/canonical_message_store.rs`（新，CanonicalMessageStore）

- `read_branch_page(db, session_id, before: Option<HistoryCursor>, limit) -> BranchPage`：
  - 无 before：读 session_branch_heads 头（无头行 → legacy_tail：seq 最大 entry_type='message'）。
  - 有 before：游标消息必须属于本会话（PK 查找，查无 → InvalidRequest 响亮 400）。
  - 回溯步进：`predecessor(m) = m.parent_message_id` 非空 → PK 查找；
    为空且存在更早 message 行（legacy 区域）→ idx_messages_session 上
    `seq < m.seq` 的最大者（**LegacyReader 链回退**：pre-T03 线性写序以 committed seq 为准，
    不是时间猜测）；皆无 → 到头。
  - 每步 1 次索引查找，取够 limit 即停（O(页大小)，不扫全史）。
  - 返回 messages（链序）+ head_revision + next_before（本页最老消息）+ has_more +
    `PageStats { rows_examined, queries }`（I/O 探针，不上线）。
- `read_branch_events(db, session_id, run_ids) -> Vec<StoredEventRow>`：
  一次 `WHERE session_id=? AND run_id IN (...) ORDER BY seq`（idx_key_events_session/run）。
- `read_child_runs(db, run_ids) -> Vec<(parent_run_id, child_run_id)>`：run_lineage 反查，
  迭代至闭包（深度有界，循环检测）；子 run 无事件时静默为空集（正常：fork 副本/标记）。
- `read_full_branch` = 循环 read_branch_page 到底（导出与 A08 全量遍历共用同一路径，
  证明分页遍历不丢不重）。

### 3.3 投影 `lingxi-service/src/history_projection.rs`（新，纯函数，唯一入口）

`project_history(messages, events) -> Vec<HistoryItem>`：

1. 事件分桶：assistant_segment_* 按 segment_id 聚合成段（start→deltas→end 周期；
   同 id 多周期 = 多个段项，按 first event seq 排序）；tool_call_started/completed 按
   tool_call_id 配对（只有 started → status 保持 started 且 result=None，**不编造结局**；
   只有 completed → 仍产出，started_seq=None）；run_state_changed 终态 → RunTerminal
   （completed 且本 run 有 final → 不产出，FinalMessage 即完成展示——四方式同规则）。
2. run 分组成序：run r 的项按事件 seq 升序。
3. 锚定（页内消息集合判定，无需跨页存在性查询）：
   user 锚（`user:{r}` 在页内）> final 锚（`{r}-final` 在页内）> lineage 并入父 run 组
   （按事件 seq 插入，项带各自 run 关联）。每个 run 的项在全分支分页遍历中**恰好出现一次**。
4. 骨架合并：链消息依序产出 UserMessage/FinalMessage/ResetMarker；user 锚 run 的项紧随
   user 消息后；final 锚 run 的项紧随其 FinalMessage 前。
5. 内容规范化（步骤①④单一入口）：
   - final/user content_json：先按 Value 解析；顶层未知键 → legacy_fields 保留；
     整体不可解析 → LegacyRaw（原文归档）；NormalizedMessage 的 Text 块统一再过
     `split_reserved_tag_segments`（think→Reasoning、mood→drop、围栏/转义字面保持——
     对已规范化内容幂等，对 R03 时代 raw 行完成剥离；与实时同一 scanner）。
   - user 消息体 `{"text": ...}` 解析失败 → LegacyRaw。

### 3.4 服务层 `lingxi-service/src/history.rs`（新）+ 路由/鉴权

- `GET /lingxi/v1/sessions/{id}/history?before&limit&all`：
  - 归属闸同 branch_history_route（get_for：404/403/200）。
  - limit 默认 50、>200 钳 200（现役 sessions.ts:1415-1419）；解析失败/重复参数 400；
    all=1 全量（现役 all=1 语义，内部仍走同一分页器逐页收集）。
  - ETag `"r{head_revision}"`；If-None-Match 匹配且无 before → 304（现役
    evaluateHistoryConditionalGet 语义锚点 sessions.ts:1468-1510）。
- `GET /lingxi/v1/sessions/{id}/export`：全量投影（同一分页器遍历），
  body = HistoryExport。
- auth.rs：两个新后缀 GET/HEAD → Scope("chat")（与 /branch 同口径）。
- 前端职责收敛：投影产物即最终展示语义，客户端不再自行解析模型结束/补造 final
  （02 §3 + 步骤②）；本任务不动桌面代码（R08 接线范围），以 HTTP 级断言证明语义。

### 3.5 重连一致性（A07 第三方式）

WS resume（stream_resume_slice）与 events_page 读取的是**同一张 key_events 表**——
重连补发的事件经同一投影函数得到的段/工具/终态序列与重开必然一致。测试用事实验证：
运行中断线→带 cursor 重连→补发事件投影 == 重开同区间投影。

## 4. T03 语义保护清单（不得回退）

- fork 共享历史消息 id 稳定、分支头/reset 标记形状不变：投影只读，不改任何写路径。
- rewind/retry 的消息形态：reset 标记按 ResetMarker 原样呈现（reason/to/sourceEntryId）。
- 导出↔重放一致：导出产物 items 与分页全集逐项相等（测试断言）。
- 场景 D：正文不混 think/MOOD——投影对 legacy 内容重切 + 实时入口已剥离，双断言。
- 仅工具执行无最终回复的 run：ToolCall 项 + RunTerminal(completed.no_final.*)，可展开。
- 工具关联重载后保留：tool_call_id 持久于事件负载，重开投影逐 id 复现。

## 5. 差异预告（07_diff_ledger 将逐条锚定）

- D1：`before` 由现役"显示序号"改为不透明游标（messageId+seq）——候选分支链在
  rewind 后序号不稳定，消息 id 才是稳定身份；任务书步骤③要求的是保留"窗口读取
  行为目标"，不是序号载体。
- D2：导出端点为新建（现役无会话导出），语义=统一投影全量。
- D3：MOOD 在四方式中一致地不出现（R05 D5 冻结：mood 结构化出事件流）——
  A07 的"正文不混入 MOOD"在所有方式下同构成立。
- D4：legacy 链回退以 committed seq 线性化 pre-T03 区域（确定性存储事实，非时间猜测）。

## 6. 叶图口径

R06_LEAF_MAP.json T04 节实际 **89 叶**；R06_PROGRESS.json task_chain 的
`r00_leaf_count: 101` 与 r00_leaf_map 注记（T04:89 / T06:101）不一致——按叶图实际
89 叶标注，差异写入报告"未验证事项/差异"。
