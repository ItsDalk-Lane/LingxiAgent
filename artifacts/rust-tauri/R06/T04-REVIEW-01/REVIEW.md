# R06-T04 独立对抗性审查报告（第 1 轮）

- 审查对象：R06-T04「统一消息历史、重连与导出」
- TASK_BASE_SHA：`2cb2b064061e902ea18faa0ec38499ce32436cb7`（分支 `codex/rust-tauri-migration`）
- 候选形态：工作区未提交形态（执行者报告 `docs/rust-tauri/R06/R06-T04_REPORT.md`，证据 `artifacts/rust-tauri/R06/T04/00-08`）
- 审查立场：独立对抗性审查。不采信执行者的完成声明；所有关键声称均由本审查亲自复核（读源码、跑命令、对现役锚点）。
- 工具链纪律：本审查全部 cargo 调用均经 `/Users/study_superior/.cargo/bin/cargo`（rustup shim → 1.98.1）；未使用 homebrew cargo；所有命令记录真实退出码；测试筛选均非空。

## 0. 候选完整性复核（digest）

亲跑 `bash scripts/rust-tauri/r06_candidate_digest.sh`：

```
CANDIDATE_DIGEST = ce0ae1adc3f68fbac4519d34156281dd8032096a5b5ca057a9171da97a66ec15
```

与执行者声称逐字一致，与证据 `08_candidate_digest/candidate_digest.txt` 一致。工作区 `.vscode/` 噪声未触发归因变体。**结论：候选无漂移，本报告审查的即是执行者交付的形态。**

---

## 一、任务完整性

### 1.1 任务书 Steps 逐条核对（任务书 R06 :166 T04 全节，已读原文）

| 步骤 | 原文要点 | 复核结论 |
|---|---|---|
| ① | 规范化文本阶段、MOOD、工具、附件、失败及终态存为权威消息结构；旧记录由单一入口规范化 | **成立**。权威结构 = `lingxi-protocol/src/history.rs` 七态 HistoryItem；旧记录唯一入口 `history_projection::parse_normalized_message`（顶层未知键剥入 legacy_fields；整体不可解析 → LegacyRaw 原文归档）。单测 `legacy_final_content_resplit_and_unknown_fields_kept`、`unparseable_row_becomes_legacy_raw` 锁定。 |
| ② | 实时/分页/重连/导出/客户端共享同一投影；前端不独立解析模型结束或补造最终答案 | **成立（服务端份额）**。`project_history` 是纯函数（无 I/O/时钟/随机，history_projection.rs:5-7 模块文档明示），四方式一致性由「同一函数 × 同一持久事实」结构性保证，`a07_realtime_reopen_reconnect_export_agree` 逐项锁定。桌面旧解析（history-builder.ts:560-563 二次剥离，现役锚点亲验）的退役属 R08，台账 D3 记录。 |
| ③ | 保留 streamId/seq/窗口读取行为目标；长会话分页从索引/游标定位，不每页完整扫描 | **成立**。`read_branch_page` 走 parent 主键回溯 + legacy 区 `idx_messages_session` 回退；seq 以 u64 十进制字符串上线（protocol history.rs 单测锁定）；A08 I/O 探针实测（见 §三.8）。 |
| ④ | 未知旧字段受控保留；Run 关联标 unknown 不按时间猜 | **成立**。legacy_fields/LegacyRaw + `RunAssociation` 三态：known 仅来自 run_id 列或本会话 id 约定且 run 实存；fork 副本（run_id NULL，T03 D9）一律 unknown（`fork_copy_run_association_is_unknown` + 集成测试 `forked_copy_marks_run_association_unknown` 双重锁定）。 |
| ⑤ | 工具卡展开状态依据稳定 ID；tool-only 任务可展开 | **成立（服务端份额）**。tool_call_id `{run_id}-tcNNNN` 实时==历史（a07 断言）；无 final 的 run 投影为 ToolCall+RunTerminal（`tool_only_run_projects_terminal_honestly`、`terminal_honesty_with_and_without_final`）。展开态 UI 属 R08。 |

### 1.2 Deliverables

三件交付物均真实存在且为生产路径（非平行构造）：`canonical_message_store.rs`（467 行）、`history_projection.rs`（1063 行）、`history.rs` 路由（359 行）+ protocol `history.rs`（294 行线型）。§二给出全链追踪。

### 1.3 A07 / A08（acceptance-catalog.json 原文已亲验，均 REQUIRED）

- **R06-A07**（实时与重开语义一致；then：阶段及顺序相同、正文不混入独立思考/MOOD；evidence：规范化序列 diff 及 UI 断言）：核心断言由 `a07_realtime_reopen_reconnect_export_agree` 完整覆盖——ToolTurn delta 含原始 `<mood>happy</mood>`/`<think>内部推理A</think>`、Final 含 `<think>终稿思考</think>最终答案B<mood>proud</mood>` 的样本下，realtime==reconnect（eventId/seq 逐项等）、history==export（逐项等）、双 blob 禁含 mood/think 原始标签与取值、reasoning 段携带思考、final content==[reasoning,text]、tool id 稳定。**UI 断言份额未执行**，执行者 §十二如实披露并归属 R08（桌面接线）；任务书 :395 明示 R06 仅向 R07/R08 交付消息投影，故该份额在 T04 阶段结构上不可执行。记录为观察项 O-5，不阻断。
- **R06-A08**（长历史分页不重复全扫；then：消息不丢重、读取字节/查询量符合索引设计；evidence：I/O 探针与分页结果）：由 `a08_consecutive_pages_cover_full_branch_without_loss_or_duplication`（1200 消息 / limit 50 / 24 页 / `chunks(50).rev().flatten()` 期望）与 `a08_page_walk_examines_only_window_rows`（单页 ≤2×limit+8 与页位置无关、累计 < 全扫下界 14400 且 ≤ n+24×16、首页末页成本差 ≤limit+8）覆盖。探针边界见观察项 O-3。

### 1.4 89 叶逐叶复核（重点：81 deferred 是否构成范围削减）

`R06_LEAF_MAP.json` tasks["R06-T04"].leaves 实载 89 叶：implemented 1（#65 SESSIONS-MESSAGES）、share 7（#19 WS-IN-PROMPT、#20 WS-IN-RESUME-STREAM、#59 FORK、#62 LATEST-USER-MESSAGE、#79 TURNS-RETRY、#83 SLASH-COMMAND-RESET、#85 TOOL-REWIND）、deferred 81。

- **implemented 叶 #65**：断言含 before/limit/all/reconciliation/消息/块/待办/修订/ETag 多子句。anchor 明示：待办子句归待办域独立叶（#76-78，deferred，叶间分工属 R00 既有设计）；reconciliation 子句以退役处理（见 F-02）；其余子句均有真实测试锚定。标注诚实（附范围说明），但台账遗漏见 F-02。
- **share 7 叶**：逐叶读 anchor，均为「本任务只承担投影/呈现份额，机制本体属既有面（R02 WS、T03 fork/retry/reset/rewind）」的如实切分，锚点测试真实存在（`root_reset_marker_stops_the_walk`、`forked_copy_marks_run_association_unknown`、a07 等）。名副其实。
- **deferred 81 叶**：逐叶分类去向 = 49 T03 管理域（归档/生命周期/工作区等，t04_anchor 逐叶注明「属 T03 管理域或独立读面」）+ 9 R08 桌面接线 + 7 R02-R03 WS/执行面 + 6 工具语义 + 5 独立读面 + 4 命令域 + 1 压缩域。关键词扫描（history/历史/分页/导出/游标/投影/重连）未见 T04 核心语义叶被偷运 deferred。**结论：81 deferred 均有去向依据，不构成范围削减。**
- 归档专项（审查清单明列「归档会话历史可读性」）：现役 `/sessions/messages` 读路径无 lifecycle 闸（sessions.ts:1383+，`assertManifestLifecycle` 仅闸 archive/delete/retry 等 mutation，:723-733、:2573），归档会话历史现役可读；候选侧 SessionRow（run_store.rs:47-55）尚无 lifecycle 字段，归档面整体属 T03 管理域/后续，读路径对所有会话一致——**未引入「归档后历史不可读」的回退**。无 finding。
- 叶数口径：PROGRESS task_chain 记 r00_leaf_count=101，LEAF_MAP 实载 89；设计稿 §6 已注明口径差异，执行者未改 PROGRESS（观察项 O-6）。

---

## 二、生产路径真实性（全链追踪）

从真实 HTTP 入口逐层追踪（全部行号亲读）：

```
GET /lingxi/v1/sessions/{id}/history|export
  lib.rs:5211-5219 路由注册 → auth.rs:648-659 classify_route（/history /export 后缀，
    GET/HEAD→Scope("chat")；:581 纯 GET /sessions/{id} 规则要求 rest 无 '/'，故新后缀
    不被旧规则吞掉；未知形状 /history/extra 等 fail-closed LocalOnly，route_policy_table
    :2560-2576 四条断言锁定）
  → history.rs:56 parse_history_query（严格解析：未知键/重复 before/limit/畸形游标/
    limit 非正 → 400；export :125-129 拒绝任何 query）
  → history.rs:60 gate_session → sessions.rs:610-622 get_for（归属闸：NotFound 404 /
    Forbidden 403 / Ok 放行；先于一切实体读取与 ETag 判定）
  → history.rs:65-72 ETag：read_branch_head_revision（canonical_message_store.rs:394，
    单条主键 SELECT，O(1) 头读）→ 仅 before.is_none() 且 If-None-Match 命中 → 304
  → canonical_message_store.rs:98 read_branch_page / :448 read_full_branch
  → history.rs:246 project_items（候选 run → read_existing_run_ids 实存校验 →
    read_lineage_children 传递闭包 → read_branch_events 按 run 索引 →
    read_existing_message_ids 跨页 user 锚批量主键探测）
  → history_projection.rs:50 project_history（纯函数）
  → HistoryPage / HistoryExport + ETag "r{headRevision}"
```

- **无第二套历史读取路径竞争**：T03 的 `/branch`（lib.rs:3987-4012 branch_history_route）是原始链投影面（调试/管理语义），与 /history 语义投影面分工不同，非竞争实现；桌面侧接线属 R08，今日 /history 是唯一统一历史读面。
- **ETag/304 仅无游标入口**：history.rs:70 `params.before.is_none() &&` 守卫；游标页恒 200（台账 D5 对照现役 :1495-1510 把 beforeId 一并传入条件判定的收窄差异）。
- **归属闸先于 ETag**：:60 闸 → :65-72 ETag，顺序正确，不向无权限者泄露存在性（跨主体拿不到 ETag 判定）。
- **export**：:130 闸 → :134-141 ETag（export 无游标概念，恒参与条件请求，与现役 all=1 语义兼容）。

---

## 三、对抗性验证（场景 D 为核心）

1. **五类内容四形态一致**：a07 样本同时含 MOOD（`<mood>happy</mood>`/`<mood>proud</mood>`）、think（`<think>内部推理A</think>`/`<think>终稿思考</think>`）、过程段（3 个 ToolTurn delta）、工具（EchoTool started/completed）、最终回复。实时/运行结束重开/重连补发/导出四形态逐项相等（测试内断言链亲读）。
2. **MOOD/think 原始标签不进正文 blob**：双 blob 禁含 `<mood`/`</mood>`/`<think`/`</think>` 及取值 happy/calm/proud；reasoning 段携带「内部推理A」；final content==[reasoning 终稿思考, text 最终答案B]（`normalize_final_message` 幂等重拆，与实时入口同一 scanner）。
3. **tool id 稳定**：实时==历史==`{run_id}-tcNNNN` 前缀断言。
4. **无 final 不伪造完成**：`tool_only_run_projects_terminal_honestly` + 单测 `terminal_honesty_with_and_without_final`（有 fmc 不另产 RunTerminal、无 fmc 有终态 → RunTerminal，规则在 history_projection.rs:15-17 模块文档公开）。
5. **取消形状**：RunTerminal(cancelled) 由线型与 `tool_status_of` 覆盖（HistoryToolStatus 五态含 Cancelled）；「取消进行中并发翻页」未专测，执行者 §十二如实披露（归属闸+快照读取无锁一致）。记录在案，非阻断。
6. **运行中投影**：进行中 run 的段/工具按 started 如实呈现（只有 started 不编造结局；只有 completed 不编造 target/digest——history_projection.rs bucket_events 亲读）。
7. **严格游标**：protocol `HistoryCursor::decode` 严格（base64url→UTF8→JSON→deny_unknown_fields）；store 侧本会话存在性+seq 吻合校验（canonical_message_store.rs:158-175，偏差 → InvalidRequest → 路由映射 400）。测试：`cursor_roundtrip_and_strict_decode`、`cursor_must_reference_a_message_of_this_session`、`history_cursor_validation_and_owner_isolation`（before=!!!/AAAA/空 → 400，跨会话游标拒绝）。篡改/伪造/跨会话三类均覆盖。
8. **分页中途变更**：游标基于 (message_id, seq) 的不可变 parent 链；翻页中途 head 追加/rewind（T03 rewind 不删旧历史，只挂新头）不破坏持有游标的页序；`head_revision` 随行返回供客户端察觉分支移动。手工推演成立。
9. **limit 边界**：0/-3/abc/重复 → 400；9999 → 钳制 200（`history_etag_conditional_get_and_limit_clamp`；钳制镜像现役 `Math.min(...,200)`，台账 D1 记录差异）；1/50/200 正常路径在 a08 与 a07 覆盖。
10. **1200 消息不丢不重**：a08 两测试（24 页 chunks(50).rev().flatten() 期望逐 id 相等；`write_legacy_rows` 经真实提交路径落库再拟态 v8 迁移形态，非内存伪造）。
11. **PageStats 方法学**：探针在 store 层逐查询计数（:119-120, :143-144, :151-152, :159-161, :231-232, :254-255 等），单页断言 ≤2×limit+8 且首页/末页成本差有界——方法学真实（计数点与 SQL 一一对应）。边界见 O-3。
12. **跨主体 403**：设备主体访问他主体会话 history+export → 403（`history_cursor_validation_and_owner_isolation`）；未知会话 404。
13. **归档可读**：见 §1.4 归档专项——现役可读、候选无归档状态可闸、无回退引入。
14. **fork 共同历史两侧一致**：fork 副本共享 message_id、run_id NULL → RunAssociation unknown（双测试锁定）；共同历史按同一投影函数呈现。
15. **reset 标记呈现**：ResetMarker 作为历史项投影；根重置标记终止 walk 防穿透（`root_reset_marker_stops_the_walk`；T03 `reset_branch_head` 恒把 head 指向新标记行，session_tree.rs:878-943 亲读复核——排除了「根重置后 legacy_tail 复活旧历史」的假设）。
16. **导出分支语义**：任务书原文未要求跨分支导出；导出 = 当前分支链全局链序（`read_full_branch` 逐页前插装配，与页大小无关），`export_equals_paged_concat`（limit=2 多页）锁定与分页装配逐项一致。符合任务书。
17. **v8 数据 parent 断裂/孤儿**：parent 指向缺失消息 → `StorageError::Corrupted` 响亮失败（canonical_message_store.rs:233-241），绝不静默跳过；legacy 区（parent 全 NULL）按 committed seq 线性化（不按时间猜），`legacy_parentless_history_walks_full_seq_order`、`mixed_linked_and_legacy_regions_walk_through` 锁定。
18. **锚定恰好一次**：页内 user 锚 > final 锚（分支无 user 锚才挂组）> lineage MergeInto 最近页内锚定祖先（visited 防环）> Elsewhere（有 fmc 自锚他页本页不产）——跨页 user 锚探测经 `read_existing_message_ids` 批量主键查询。手工推演跨页 user 锚/final 锚/子代理/Elsewhere 四情形，恰好一次不变量结构上成立。

---

## 四、独立回归复跑

（本节全部为本审查亲自执行；命令均经 `/Users/study_superior/.cargo/bin/cargo`，退出码真实记录。）

### 4.1 全量回归（独立复跑）

命令：`cd rust && /Users/study_superior/.cargo/bin/cargo test --locked --workspace`（2026-10-09 23:31 启动，墙钟约 21 分钟——其中 r05_t08_resources 等两个既有套件各约 300s，为 R05 既有耗时，非 T04 新增）。

- **EXIT=0**（任何失败都会使 cargo 退出码非零）→ 0 failed 独立确证。
- 测试总数独立核验：`cargo test --locked --workspace -- --list | grep -c ": test$"` → **1673**（LIST_EXIT=0；编译缓存热，list 不执行测试）。源码形态与执行者 digest 一致 ⇒ 测试集合相同。
- 合并结论：**1673 passed / 0 failed，与声称一致**。

### 4.2 执行者证据核验（静态）

- `06_workspace_regression/full_workspace.txt`：126 个 "test result: ok" 行，累计 passed=1673 / failed=0，EXIT=0——与声称一致。
- R05 全族套件（t01-t08 共 24 个测试二进制名）与 `r06_t03_session_tree`、`r06_t04_history` 均在证据中真实出现。
- `05_targeted_tests/`：protocol 2/2、adapters 8/8、history_projection 5/5、r06_t04_history 6/6、auth 19/19，均 EXIT=0。
- `02_red_tests/`：真实 RED 留痕（EXIT=101，6 failed + 编译期 E0716）。
- `03_fmt/`、`04_clippy/`：「失败（EXIT=1/101，真实 diff 与 3 处 clippy error）→ 修复 → 复检 EXIT=0」两阶段留痕真实。

### 4.3 定向套件独立复跑

全部 EXIT=0，筛选均非空：

| 命令 | 结果 |
| --- | --- |
| `cargo test --locked -p lingxi-protocol history::` | 2 passed / 0 failed（23 filtered out） |
| `cargo test --locked -p lingxi-adapters --test canonical_message_store` | 8 passed / 0 failed |
| `cargo test --locked -p lingxi-service --lib history_projection` | 5 passed / 0 failed（361 filtered out） |
| `cargo test --locked -p lingxi-service --test r06_t04_history` | 6 passed / 0 failed |
| `cargo test --locked -p lingxi-service --lib auth::` | 19 passed / 0 failed（347 filtered out；含 route_policy_table 的 /history /export 4 断言） |

T03 套件（`r06_t03_session_tree`：fork 共享历史 id、retry 不覆写、rewind 分支回退等）与 R05 全族 24 个套件在 4.1 的全量复跑中随 workspace 一并执行且 EXIT=0——T03 语义在 T04 投影下不失真由「T03 套件全绿 + T04 投影测试锁定 fork/reset/rewind 呈现」双重支撑。

---

## 五、防虚假完成核查

- **#65 真实生产链**：/history、/export 从 HTTP 入口到 SQLite 的全链已在 §二逐层核实，非平行构造；正负测试俱在（正：a07/a08/export_equals_paged_concat；负：游标验证/归属/ETag/limit 边界）。
- **share 7 叶名副其实**：逐叶 anchor 亲读，投影份额与机制归属切分如实（§1.4）。
- **测试对照现役真实语义（RC-3）**：现役锚点全部由本审查亲验一致——sessions.ts:1415-1416（beforeId=Number、limit=Math.min(Number||50,200)，NaN 静默吞）、:1418-1419（forceAll）、:1421-1435（reconciliation=1 严格证据路径）、:1495-1510（条件请求含 beforeId）、history-builder.ts:560-563（客户端二次剥离反模式）、page.ts:138（窗口快路径）、migrations.rs V8_SQL（pre-T03 行 parent NULL、entry_type='message'）、run_store.rs:1398-1416/:1531-1549（`{run}-start` 与 fmc 落库）、session_tree.rs:861/:901-912（user 消息 id 约定与 reset 标记）。测试样本（含原始 mood/think 标签的 delta 与 final）对照的是现役真实规范化语义，非自洽样本。
- **RED→GREEN 留痕**：F1-F6 修复记录（含 a08 页序断言自相矛盾的预编译修正、事件条数 3→4 误算、export 前插装配）与 02_red_tests 真实失败输出互证。

---

## 六、Findings

### F-01

- FINDING_ID: R06-T04-REVIEW01-F01
- SEVERITY: LOW
- REQUIREMENT_ID: 执行者报告 §三声称「严格查询解析：未知/重复键、畸形游标、limit 非正 → 400」；history.rs:170 模块注释「重复或未知键一律 400」
- FILE_AND_LINE: `rust/crates/lingxi-service/src/history.rs:206-213`（all 臂）
- OBSERVED_BEHAVIOR: `parse_history_query` 的 `all` 臂只校验取值非 "1" 时拒绝，无重复键检查：`?all=1&all=1` 静默通过（all=true）。before 臂（:179-183）与 limit 臂（:191-195）均有 `is_some()` 重复检查。
- EXPECTED_BEHAVIOR: 与声称一致——重复 `all` 键同样 400；或将声称收窄为「before/limit 重复键 400」。
- REPRODUCTION: 代码路径直读（all 臂无重复检查即置 true）；HTTP 形态 `GET /lingxi/v1/sessions/{id}/history?all=1&all=1` 将返回 200 而非 400。
- ROOT_CAUSE: all 臂漏写 `if all { return Err(...) }` 前置检查（before/limit 两臂同文件均有）。
- SAME_ROOT_CAUSE_PATHS: 已横向扫描同函数全部分支与 export 路由——before/limit 有重复检查、未知键拒绝、export 拒绝任何 query（:125-129）；仅此一处。
- IMPACT: 无安全/正确性影响（重复 all=1 与单个 all=1 语义相同；现役 forceAll 对重复键同样宽松）；属「声称精确性」问题。
- REQUIRED_FIX: 二选一：(a) all 臂补重复检查并加单测；(b) 修正报告 §三与模块注释的声称。
- REGRESSION_TESTS: `parse_history_query` 增加 `all=1&all=1` → 400 用例（若选 (a)）。

### F-02

- FINDING_ID: R06-T04-REVIEW01-F02
- SEVERITY: LOW
- REQUIREMENT_ID: 任务书步骤③（保留窗口读取行为目标）；02 §4 禁止静默降级之精神（差异须显式记录）
- FILE_AND_LINE: `artifacts/rust-tauri/R06/T04/07_diff_ledger/diff_ledger.md`（D1-D5 未覆盖）；`rust/crates/lingxi-service/src/history.rs:214-218`（unknown query → 400）
- OBSERVED_BEHAVIOR: 现役消息页公开查询参数 `reconciliation=1`（sessions.ts:1421-1435 严格证据路径，本审查亲验）在候选侧被 400（unknown query parameter）。该退役决策仅见于 R06_LEAF_MAP.json #65 anchor 一句话（「reconciliation 元数据由确定性投影取代，前端不再和解」），差异台账 D1-D5 未列条目。
- EXPECTED_BEHAVIOR: 对现役公开参数的退役应在差异台账单列一条（含理由），与 D1-D5 同等显著性。
- REPRODUCTION: 台账全文 grep `reconciliation` 无命中（本审查已核）；#65 anchor 有一句说明。
- ROOT_CAUSE: 台账编写时将该子句的退役视为 #65 anchor 内说明，未上升为 D 项。
- SAME_ROOT_CAUSE_PATHS: 已横向核对现役消息页其余公开参数（before/limit/all/ETag 头）在台账均有对应（D1/D2/D5）；reconciliation 是唯一遗漏。
- IMPACT: 文档完整性问题；无现役客户端受影响（桌面接线属 R08）；退役方向与任务书步骤②（前端不独立解析）一致，行为本身合理。
- REQUIRED_FIX: 差异台账补一条（如 D6）记录 reconciliation=1 退役及理由。
- REGRESSION_TESTS: 文档类，无需测试；可在 #65 anchor 与台账间加交叉引用。

---

## 七、观察项（非阻断，记录在案）

- O-1：`read_branch_page` 头部路径中 head_message_id 指向不存在消息行时静默返回空页（canonical_message_store.rs:148-156），与 parent 缺失的响亮 Corrupted（:233-241）处理不对称。仅数据库损坏可达，无正常路径触发。
- O-2：parent 链成环（仅篡改可达）时 `read_full_branch`（:455-466）页间循环依赖 has_more，环上恒 true，理论可无限循环；单页 walk 有 limit+1 界（:187-189），故单页不挂。正常写入路径 parent 只指向已存在的更早消息，环不可达。
- O-3：PageStats 探针覆盖 walk 读取（head/游标/逐行前驱），不含 project_items 的附加查询（run 实存校验、lineage 闭包、事件、跨页 user 锚探测——各为每页常数次批量索引查询，报告 §十三已部分披露跨页锚探测随 run 数线性）。A08 核心断言「不重复全扫」仍成立（探针证明 walk 为 O(页大小)），但「字节/查询量符合索引设计」的完整口径应注明探针边界。
- O-4：a07 测试对实时捕获应用「有 fmc 跳过终态项」归一化。该规则是投影的公开规则（history_projection.rs:15-17 模块文档明示，四方式同规则），非测试特例；实时侧事件流本身含终态事件而历史投影不另产终态项，测试在两侧对齐到投影语义后比较，方法合理。
- O-5：A07 evidence 的「UI 断言」份额未执行。执行者 §十二如实披露并归属 R08；任务书 :395 明示 R06 仅交付消息投影，T04 阶段结构上无可断 UI。该份额挂起至 R08，届时须核销。
- O-6：叶数口径 101（PROGRESS task_chain）vs 89（LEAF_MAP 实载）。设计稿 §6 已注明，执行者未改 PROGRESS；以实载 89 为准不影响逐叶复核结论。

---

## 八、裁决依据汇总

- 候选完整性：digest 亲跑复核逐字一致（§0）。
- 任务完整性：Steps ①-⑤ 逐条成立；三件 Deliverables 真实；A07/A08（均 REQUIRED）核心断言由独立复跑的测试覆盖；89 叶逐叶复核无范围削减（§一）。
- 生产路径真实性：/history 与 /export 全链逐层亲读，无平行构造者；归属闸先于 ETag；304 仅无游标入口；auth 分类 fail-closed（§二）。
- 对抗性验证：场景 D 全清单 18 项逐项核查通过，含 MOOD/think 剥离、final 重拆、tool id 稳定、无 final 不伪造、严格游标三负类、1200 消息不丢不重、v8 断裂响亮失败、锚定恰好一次手工推演（§三）。
- 独立回归：全量 EXIT=0 + 测试总数 1673 独立核验；五个定向套件全部复跑通过（§四）。
- 防虚假完成：implemented 叶 #65 有真实生产链与正负测试；share 7 叶名副其实；测试对照现役真实语义（现役锚点全部亲验一致）；RED→GREEN 留痕互证（§五）。
- Findings：2 个 LOW（F-01 all 重复键声称精确性；F-02 reconciliation 退役台账遗漏），均无安全/正确性影响，不阻断验收。观察项 6 条（O-1 至 O-6）记录在案。
- 未发现能够推翻「本 Task 已完成」声明的证据；未发现虚假完成、范围削减或静默降级。

VERDICT: PASS
