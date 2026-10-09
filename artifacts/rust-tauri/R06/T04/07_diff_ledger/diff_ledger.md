# R06-T04 差异台账（Design → Implementation → Incumbent）

Task Base: `2cb2b064061e902ea18faa0ec38499ce32436cb7`
本台账分三部分：D 项（对现役的行为差异，逐条锚定现役 file:line）、R 项（实现相对设计稿的细化）、F 项（RED→GREEN 过程中的修复记录）。所有现役行号均在本次任务中亲自复核。

## D 项：对现役（incumbent）的行为差异

### D1 游标形态：数字 displayId → 不透明游标
- 现役：`server/routes/sessions.ts:1415-1416` `beforeId = Number(query("before"))`、`limit = Math.min(Number(...)||50, 200)`——数字 displayId 游标，非法输入被 `Number()||50` 静默吞掉（NaN→50）。
- 候选：`HistoryCursor{message_id, seq}` 不透明 base64url 游标（protocol history.rs），严格解码失败 → 400；limit 非十进制/0/负数 → 400，>200 钳制到 200。
- 理由：任务书步骤③「长会话分页从索引/游标定位」+ 02 §4 禁止静默降级。现役 NaN 静默吞掉属于隐性降级。
- 锁定测试：`cursor_roundtrip_and_strict_decode`（protocol）、`cursor_must_reference_a_message_of_this_session`（adapters）、`history_cursor_validation_and_owner_isolation`、`history_etag_conditional_get_and_limit_clamp`（service）。

### D2 新增 /export 面
- 现役：无独立导出端点；全量读取靠 `all=1`（sessions.ts:1418-1419 `forceAll`）混在消息页里。
- 候选：`GET /lingxi/v1/sessions/{id}/export` 独立面，严格无查询参数（任何 query → 400），全局链序旧→新，与逐页前插装配逐项一致。
- 理由：R06-A07 要求导出与实时/重开/重连四方式语义一致，独立面使契约显式化。
- 锁定测试：`export_equals_paged_concat`、`a07_realtime_reopen_reconnect_export_agree`。

### D3 MOOD/思考不进正文：由客户端二次解析 → 服务端统一投影
- 现役：客户端 `desktop/src/react/utils/history-builder.ts:560-563` 对「结构化 mood/thinking block 双重表示」做二次剥离（`hasStructuredMood`/`hasStructuredThinking`）——客户端独立解析模型输出，正是任务书步骤②禁止的形态。
- 候选：MOOD/think 在入口处已被结构化（R05 D5 + DeltaNormalizer），持久化的 key_events 只含规范化片段；final 消息 content 经 `normalize_final_message` 幂等重拆。统一投影只读持久事实，不含任何 mood/think 原始标签；前端零解析。
- 锁定测试：`a07_realtime_reopen_reconnect_export_agree`（实时 blob 与历史 blob 均不含 mood/think；reasoning 段携带思考、正文段只有最终答案）。

### D4 legacy 线性化回退
- 现役：v8 迁移把 pre-T03 行的 parent 置 NULL（migrations.rs V8_SQL），读侧依赖目录/窗口快路径（`server/history-read/page.ts:138` `displayableSourceIndexes.slice(start,end)` 窗口切片）。
- 候选：链接区走 parent 主键回溯；parent NULL 的 legacy 区按 committed seq 线性化回退（不按时间猜），根重置标记终止回溯。
- 锁定测试：`legacy_parentless_history_walks_full_seq_order`、`mixed_linked_and_legacy_regions_walk_through`、`root_reset_marker_stops_the_walk`。

### D5 条件请求收窄
- 现役：ETag 对带 before 的分页也参与条件判定（sessions.ts:1495-1510 把 `beforeId` 一并传入 conditional）。
- 候选：304 仅用于无游标入口页（游标页是历史切片，恒 200）；ETag 判定在归属闸之后（不泄露存在性），且先于消息读取（O(1) 头 revision 读）。
- 锁定测试：`history_etag_conditional_get_and_limit_clamp`。

## R 项：实现相对设计稿（01_design/design.md）的细化

- R1 `read_child_runs` → `read_lineage_children`：设计稿提「子 run 解析」；实现为带 visited 集合的传递闭包（frontier 迭代），孙级子代理 run 也能并入最近页内锚定祖先组。测试：`lineage_children_resolve_transitively_without_cycles`。
- R2 工具卡线型：设计稿写 ToolCallItem 含 result 状态字符串；实现为 `HistoryToolStatus` 五态枚举（Started/Success/Failed/Cancelled/Unknown），Unknown 镜像 ToolResultStatus::Unknown（外部完成的副作用无本地回执绝不报成功），args_digest 只在 started 事件提供时携带（completed-only 不编造）。
- R3 页序语义细化：设计稿未钉死页间次序；实现定为「最新页先出、页内旧→新」，全量读取（read_full_branch/export）逐页前插装配 → 全局链序旧→新，与页大小无关。测试：`a08_consecutive_pages_cover_full_branch_without_loss_or_duplication`、`export_equals_paged_concat`。
- R4 跨页 user 锚探测：设计稿写「无需跨页存在性查询」；GREEN 阶段发现 final 锚若不在 user 锚所在页会导致 run 组重复出现，新增 `read_existing_message_ids` 批量主键探测（`user:{run}` 是否存在于分支任意页），保证每 run 的项在整段分页遍历中恰好出现一次。测试：`export_equals_paged_concat`（limit=2 多页场景与导出逐项一致）。
- R5 归属闸返回形态：clippy `result_large_err` 修复，`Result<(), Response>` → `Option<Response>`（语义不变）。

## F 项：RED→GREEN 修复记录（诚实留痕）

- F1 a08 页序断言自相矛盾（RED 前自检发现）：`collected == ids`（最旧页先出）与「before=m2 → [m1] 严格更老」在任何单游标设计下互斥；且最旧页先出需 O(N) 找根（违反 A08 I/O 上界）并破坏根重置语义。修为最新页先出 + `ids.chunks(50).rev().flatten()` 期望。
- F2 事件条数误算：`record_run_started` 会落 `{run}-start` 的 queued→running key event（run_store.rs:1398-1416），每 run 事件数 3→4、两 run 6→8，注释注明来源行。
- F3 legacy id 期望错误：`write_legacy_rows` 经真实提交路径落 `{run}-final`（即 `legacy_run_{i}-final`），非 `l{i}`；legacy/mixed 两测试期望与 parent 链修正。
- F4 嵌套 runtime panic：sync `seed()`（内部 block_on）被 async fixture 调用 → 双 runtime panic；加 `seed_async` 变体。
- F5 `export_equals_paged_concat` 失败：分页拼接（limit=2，最新页先出）≠ 导出（单页=全局升序），根因是装配次序依赖页大小。修为 read_full_branch 逐页前插 → 全局链序与页大小无关，客户端装配语义=前插（聊天上滑加载），不变量更强。
- F6 编译/静态检查修复：E0004 补 ToolResultStatus::Unknown 分支（新增 HistoryToolStatus::Unknown）、E0308 缺 `&mut`、clippy map_entry（entry API）、type_complexity（parse_message_body 返回值简化为 Result<Map, Value>）、unused variable/assignments 告警清零。
