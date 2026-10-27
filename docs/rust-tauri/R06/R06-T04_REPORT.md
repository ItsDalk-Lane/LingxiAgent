# R06-T04 统一消息历史、重连与导出 — 实施报告

- TASK_ID: R06-T04
- TASK_BASE_SHA: `2cb2b064061e902ea18faa0ec38499ce32436cb7`（分支 `codex/rust-tauri-migration`）
- 验收 ID: R06-A07（实时与重开语义一致）、R06-A08（长历史分页不重复全扫）
- 工具链: `/Users/study_superior/.cargo/bin/cargo`（rustup shim → cargo/rustc 1.98.1，仓库根 `rust-toolchain.toml` 锁定）；全程未使用 homebrew cargo 1.93.0（证据 `artifacts/rust-tauri/R06/T04/00_toolchain/toolchain.txt`）
- 候选 digest：由 `scripts/rust-tauri/r06_candidate_digest.sh` 在全部文件写完后计算，值见 handback，**本报告不内嵌**（报告自身在 digest 范围内）

## 一、已完成的 Steps（任务书步骤逐条核对）

1. 规范化文本阶段、MOOD、工具、附件、失败及终态存为权威消息结构；旧记录由单一入口规范化 → 完成。权威结构 = `lingxi_protocol::history::HistoryItem` 七态（UserMessage/AssistantSegment/ToolCall/FinalMessage/RunTerminal/ResetMarker/LegacyRaw）+ `RunAssociation` 三态诚实；旧记录统一走 `history_projection::parse_normalized_message`（不可规范化 → LegacyRaw 原文档案，未知顶层键剥离进 legacy_fields）。设计→生产映射表（Requirement→Production Entry→Owner→Implementation→正/负测试→证据）先于实现建立在 `01_design/design.md`。
2. 实时/历史分页/重连/导出共享同一语义投影，前端不独立解析模型结束或补造最终答案 → 完成。`project_history` 纯函数是唯一投影入口；四方式一致性由「同一投影函数 × 同一持久事实」结构性保证（a07 测试锁定）；RunTerminal 只在无 final 时出现（绝不编造 final）。桌面侧旧解析的退役属 R08 接线（本任务不动桌面代码，台账 D3）。
3. 保留 streamId/seq/窗口读取行为目标；长会话分页从索引/游标定位，不每页完整扫描 → 完成。`read_branch_page` 走 parent 主键回溯 + legacy 区 idx_messages_session 回退，PageStats 探针实测单页检查行数 ≤ 2×limit+8 与页位置无关（A08）。
4. 未知旧字段保留在受控 legacy 扩展或原始档案；无法确定的 Run 关联标 unknown 而不按时间猜 → 完成。legacy_fields / LegacyRaw / RunAssociation::unknown(reason)；fork 副本（run_id 置 NULL，T03 D9）一律 unknown（fork_copy_run_association_is_unknown）。
5. 工具卡展开状态依据稳定 ID；仅工具执行无最终回复的任务也可展开查看 → 完成（服务端份额）。tool_call_id `{run_id}-tcNNNN` 全程稳定（a07 断言实时==历史同一 id）；无 final 的 run 投影为 ToolCall + RunTerminal 组（tool_only_run_projects_terminal_honestly / terminal_honesty_with_and_without_final），客户端有稳定 id 可锚定展开态。展开态 UI 本身属 R08。

## 二、已交付的 Deliverables

- **CanonicalMessageStore**：`rust/crates/lingxi-adapters/src/storage/canonical_message_store.rs`（read_branch_page / read_full_branch / read_branch_events / read_lineage_children / read_existing_run_ids / read_existing_message_ids / read_branch_head_revision；PageStats I/O 探针）。
- **统一历史投影**：`rust/crates/lingxi-service/src/history_projection.rs`（`project_history` 纯函数 + `ProjectionInput`；协议线型在 `rust/crates/lingxi-protocol/src/history.rs`）。
- **分页/导出接口**：`GET /lingxi/v1/sessions/{id}/history` 与 `GET /lingxi/v1/sessions/{id}/export`（`rust/crates/lingxi-service/src/history.rs`；路由注册 `lib.rs`；auth 分类 `auth.rs` → Scope("chat")）。

## 三、实际生产调用链（无平行构造者）

```
GET /lingxi/v1/sessions/{id}/history|export
  → auth.rs classify_route（/history、/export 后缀，GET/HEAD → Scope("chat")，落 fallback LocalOnly 失败闭合）
  → history.rs 路由（严格查询解析：未知/重复键、畸形游标、limit 非正 → 400，无静默）
  → 归属闸（sessions.get_for：NotFound 404 / Forbidden 403，先于一切实体读取与 ETag 判定）
  → read_branch_head_revision（O(1) 头读）→ If-None-Match 命中且无游标 → 304
  → CanonicalMessageStore（read_branch_page / read_full_branch）
  → project_items（候选 run → read_existing_run_ids → read_lineage_children 闭包 → read_branch_events → read_existing_message_ids 跨页 user 锚探测）
  → project_history（纯投影）
  → HistoryPage / HistoryExport + ETag "r{headRevision}"
```

实时/重开/重连走 R02 既有 execute/subscribe 链（本任务零改动）；a07 证明其事件序列与本投影逐项一致。无第二条历史读旁路。

## 四、代码修改清单

修改（已跟踪，相对 base）：
- `rust/crates/lingxi-protocol/src/lib.rs` +1（`pub mod history;`）
- `rust/crates/lingxi-adapters/src/storage/mod.rs` +1（`pub mod canonical_message_store;`）
- `rust/crates/lingxi-service/src/lib.rs` +11（两个 `pub mod` + /history、/export 路由注册）
- `rust/crates/lingxi-service/src/auth.rs` +29（/history、/export 后缀分类规则 + route_policy_table 新增 4 断言）
- `docs/rust-tauri/R06/R06_LEAF_MAP.json`（T04 89 叶 t04_status/t04_anchor 标注；无测试引用该文件，已核实）
- `docs/rust-tauri/R06/R06_PROGRESS.json`（T04 开启，任务书指定候选一部分，未回退、未编辑）

新增（未跟踪）：
- `rust/crates/lingxi-protocol/src/history.rs`（294 行：HistoryItem 七态/RunAssociation/HistoryToolStatus/HistoryCursor/HistoryPage/HistoryExport + 2 单测）
- `rust/crates/lingxi-adapters/src/storage/canonical_message_store.rs`（467 行）
- `rust/crates/lingxi-adapters/tests/canonical_message_store.rs`（567 行，8 测试）
- `rust/crates/lingxi-service/src/history_projection.rs`（1063 行，含 5 单测）
- `rust/crates/lingxi-service/src/history.rs`（359 行）
- `rust/crates/lingxi-service/tests/r06_t04_history.rs`（1185 行，6 集成测试）
- 证据目录 `artifacts/rust-tauri/R06/T04/`（00–08）

## 五、现役语义逐条锚定（RC-2，行号亲验于 base 工作区）

- 消息页读取：`server/routes/sessions.ts:1415-1416`（`beforeId=Number(query)`、`limit=Math.min(Number(...)||50,200)`——NaN 被静默吞为 50）；`all=1` forceAll `:1418-1419`；reconciliation=1 严格证据路径 `:1421-1435`；条件请求把 beforeId 一并传入 `:1495-1510`。
- 窗口快路径：`server/history-read/page.ts:138`（`displayableSourceIndexes.slice(start,end)` 窗口切片 + 锚点二分，不回放全史）——候选侧对应物为 parent 主键回溯 + idx_messages_session 回退（A08 探针对照）。
- 客户端二次解析反模式：`desktop/src/react/utils/history-builder.ts:560-563`（对结构化 mood/thinking block 双重表示做剥离）——任务书步骤②禁止的形态，候选侧由服务端统一投影取代（桌面退役属 R08）。
- v8 迁移产物形态：`migrations.rs` V8_SQL 把 pre-T03 行 parent 置 NULL、entry_type 置 'message'——legacy 线性化回退的对照基准。
- 事件持久事实：`run_store.rs:1398-1416`（record_run_started 落 `{run}-start` key event）、`:1531-1549`（commit_run_outcome 落所供 key_events + fmc）；`session_tree.rs:861`（user 消息 `user:{run_id}`）、`:901-912`（reset 标记 entry_type）。

## 六、新增测试（21 个，全部通过；RC-3 现役对照式）

- protocol（2）：`history::tests::cursor_roundtrip_and_strict_decode`、`history::tests::seq_fields_are_decimal_strings_on_the_wire`。
- adapters（8）：`a08_consecutive_pages_cover_full_branch_without_loss_or_duplication`（1200 消息/limit 50/24 页不丢不重，对照现役默认页大小 sessions.ts:1416）、`a08_page_walk_examines_only_window_rows`（单页 ≤2×limit+8、累计显著低于全扫下界）、`legacy_parentless_history_walks_full_seq_order`、`mixed_linked_and_legacy_regions_walk_through`、`root_reset_marker_stops_the_walk`、`cursor_must_reference_a_message_of_this_session`、`branch_events_scoped_to_page_runs_and_ordered`、`lineage_children_resolve_transitively_without_cycles`。
- service 单测（5，history_projection）：`legacy_final_content_resplit_and_unknown_fields_kept`、`unparseable_row_becomes_legacy_raw`、`terminal_honesty_with_and_without_final`、`orphan_child_merges_into_parent_group`、`fork_copy_run_association_is_unknown`。
- service 集成（6，r06_t04_history.rs）：`a07_realtime_reopen_reconnect_export_agree`、`tool_only_run_projects_terminal_honestly`、`history_etag_conditional_get_and_limit_clamp`、`history_cursor_validation_and_owner_isolation`、`export_equals_paged_concat`、`forked_copy_marks_run_association_unknown`。
- auth：`route_policy_table` 增补 /history、/export 4 断言（GET→Scope("chat")、POST→LocalOnly、/history/extra→LocalOnly）。

## 七、现有回归测试（真实命令与退出码）

- 定向：`cargo test --locked -p lingxi-protocol history::`（2/2，EXIT=0）；`-p lingxi-adapters --test canonical_message_store`（8/8，EXIT=0）；`-p lingxi-service history_projection::`（5/5）、`--test r06_t04_history`（6/6）、`auth::`（19/19，EXIT=0）。证据 `05_targeted_tests/`。
- 全量：`cargo test --locked --workspace` → **1673 passed / 0 failed，EXIT=0**（证据 `06_workspace_regression/full_workspace.txt`）。T03/R02–R05 全套件保持绿色（T03 语义保护成立：fork 共享历史 id、分支头、reset 标记、rewind/retry 消息形态、export↔reload、thinking/MOOD 不进正文、tool-only 不伪装完成、工具关联重载后保持——其中与 T04 相交的项由本节 T04 测试与原 T03 套件双重覆盖）。
- fmt/clippy：`cargo fmt --all -- --check` 与 `cargo clippy --locked --workspace --all-targets -- -D warnings` 均有「失败→修复→复检 EXIT=0」两阶段真实记录（`03_fmt/fmt_check.txt`、`04_clippy/clippy.txt`）。
- RED：`02_red_tests/red_adapters.txt`、`red_service.txt` 真实 EXIT=101 失败记录（编译期 E0716 与运行期 6 failed）。

## 八、Acceptance 证据

### R06-A07：实时与重开语义一致
given MOOD/思考/过程/工具/最终回复俱全的任务（ToolTurn 3 delta 含原始 `<mood>happy</mood>`/`<think>内部推理A</think>` + Final 含 `<think>终稿思考</think>最终答案B<mood>proud</mood>`）；when 对比实时、重开、重连、导出；then：`a07_realtime_reopen_reconnect_export_agree` 断言 realtime==reconnect（eventId/seq 序列逐项相等）、history==export（逐项相等）、阶段及顺序相同、实时与历史 blob 均不含 mood/think 原始标签、reasoning 段携带「内部推理A」、final content == [reasoning 终稿思考, text 最终答案B]、逐段文本 == 实时 delta 拼接、tool id 实时==历史且以 `{run_id}-tc` 开头、run 组前恰一条 user 消息。UI 断言份额：服务端投影即 UI 唯一数据源（前端零解析，台账 D3）；桌面对接属 R08。

### R06-A08：长历史分页不重复全扫
given 冻结大型历史样本（1200 消息线性链）；when 连续读取全部页面（limit=50，24 页）；then：`a08_consecutive_pages_cover_full_branch_without_loss_or_duplication`（不丢不重、次序符合分页语义）、`a08_page_walk_examines_only_window_rows`（I/O 探针：单页检查行数 ≤108 与页位置无关、累计 < 全扫下界 n×24/2=14400 且 ≤ n+24×16 线性）。证据 `05_targeted_tests/protocol_and_adapters.txt`。

## 九、R00 原始断言对应结果（89 叶）

`docs/rust-tauri/R06/R06_LEAF_MAP.json` tasks["R06-T04"].leaves 全部 89 叶已标注 `t04_status`/`t04_anchor`：implemented 1（#65 SESSIONS-MESSAGES 阅读会话消息页）、share 7（#19 WS-IN-PROMPT、#20 WS-IN-RESUME-STREAM、#59 FORK、#62 LATEST-USER-MESSAGE、#79 TURNS-RETRY、#83 SLASH-COMMAND-RESET、#85 TOOL-REWIND）、deferred 81（桌面接线/WS 入口/压缩/工具语义/管理面/独立读面，逐叶注明归属域）。锚点均引用真实通过测试名。

## 十、记录在案差异

差异台账 `07_diff_ledger/diff_ledger.md`：D1 游标形态（数字 displayId → 不透明游标，NaN 静默吞掉改为 400）、D2 新增 /export 面、D3 MOOD/思考由客户端二次解析改为服务端统一投影、D4 legacy 线性化回退、D5 条件请求收窄（游标页恒 200、ETag 在归属闸后、O(1) 头读）；R1–R5 实现相对设计稿的细化；F1–F6 RED→GREEN 修复留痕（含 a08 页序断言自相矛盾的预编译修正、事件条数 3→4 误算、legacy id 期望、嵌套 runtime fixture、export 前插装配）。

另：**叶数口径差异**——R06_PROGRESS.json task_chain 记 r00_leaf_count=101，而 R06_LEAF_MAP.json T04 段实际 89 叶（设计稿 §6 已注明）；以 LEAF_MAP 实载为准，未改 PROGRESS。

## 十一、接口兼容结果（R02–R05 / T01–T03）

- 对 R02（events/ws）：零改动；a07 复用既有 subscribe/resume 链证明一致性。/events 面与 /history 面分工在 protocol history.rs 模块头注明。
- 对 R03（执行入口）：零改动；prompt/abort/compact 等入口面原样。
- 对 R05（规范化入口）：复用 DeltaNormalizer 规范化片段与 normalize_final_message 幂等重拆，无新解析路径。
- 对 T01–T03：零改动其文件；T03 套件在全量回归中全绿。fork 副本 run 关联 unknown 的投影行为与 T03 D9（fork 副本 run_id 置 NULL）一致。
- 重复属主检查：/history、/export 为新端点，与既有路由无重叠；历史读面唯一实现（无平行构造者）。

## 十二、未验证事项

- 桌面 UI 实际渲染与工具卡展开态交互（属 R08 接线；本任务仅交付服务端投影与稳定 id）。
- 真实供应商端到端流（本任务全部在本机隔离环境 + 确定性替身下验证）。
- 正式打包/其他平台未执行（仅本机 macOS arm64）。
- 取消/恢复相交场景：取消中的 run 的 RunTerminal(cancelled) 投影由线型与 tool_status_of 覆盖，但「取消进行到一半时并发翻页」的交错时序未专测（归属闸 + 快照读取使其无锁一致，记录在案）。
- 叶数口径（101 vs 89）见第十节，未动 PROGRESS。

## 十三、已知风险

- legacy 线性化回退按 committed seq 排序（不按时间猜）；若 pre-T03 库存在 seq 与提交序不一致的手工改库，次序以 seq 为准——这是唯一确定性来源，已记录。
- 跨页 user 锚探测为批量主键查询（每页一次），run 数极大页的查询代价随 run 数线性；A08 探针边界内。
- `limit>200` 钳制而非报错（镜像现役 `Math.min(...,200)` 语义）；严格派可在后续阶段收紧，台账 D1 已注明差异。

STATUS: READY_FOR_INDEPENDENT_REVIEW
