# R06-T03 差异台账（Difference Ledger）

勘察口径：RC-1（端到端生产链勘察）、RC-2（逐条锚定现役文件:行号，锚点亲验于 TASK_BASE_SHA 4920c59e3 工作区）、RC-3（现役对照式测试）。
现役 = TypeScript 生产链 `server/` + `core/` + `pi-coding-agent/`；本实现 = `rust/crates/{lingxi-kernel,lingxi-adapters,lingxi-service}`。

## D1 — rewind 文件恢复：盲写回 → sha256 比对 + 冲突全拒（A06 强化）

- 现役锚点：`core/workspace-snapshots.ts:584-620`（restore 按快照逐文件盲写回，无外部修改检测）；`core/session-checkpoints.ts:14-16,74-101`（检查点记录与恢复入口）。
- 现役行为：恢复时直接覆盖工作区文件。若用户在检查点之后手工改了文件，恢复会**静默覆盖用户修改**。
- 本实现：`checkpoint_file_versions` 只存 `sha256 + size_bytes`（**不存内容字节**，无内容外泄面）。rewind 前逐文件重算 sha256 比对；任何分歧——包括文件在检查点之后被**删除**——整批 409 `rewind_file_conflict`，分支头不动、用户文件不丢。全部一致时恢复为"验证未变更"的 no-op（语义：分支回退已生效，文件本就处于检查点状态）。
- 测试锚点：`rewind_refuses_on_external_modification_and_preserves_branch`（外部修改→409、分支不变、文件保留；文件被删→409；内容一致→200 且 `verified` 回执）。
- 偏离方向：**强化**（现役缺陷修复）。receipt 显式携带 `externalEffects: "not_rolled_back"`，不假装外部世界被回滚。

## D2 — fork 历史复制：JSONL 条目复制 → 关系行同事务复制（消息 ID 稳定保持一致）

- 现役锚点：`node_modules/@earendil-works/pi-coding-agent/dist/core/session-manager.js:1113`（`createBranchedSession(leafId)`：新会话文件只含根到指定叶的路径，entry ID 原样保留；调用侧 `core/session-coordinator.ts:3065`）。
- 本实现：`fork_session` 单事务内 `INSERT INTO messages ... SELECT`（根→边界含边界），`message_id` 原样保留；`session_branch_heads` 各自独立起头。
- 一致性：消息 ID 稳定语义与现役一致（fork 后两侧同一历史消息同 ID）。
- 差异仅在载体形状：文件复制 → 关系行复制；A05「fork 不串写」由 `fork_copies_shared_history_with_stable_ids_and_independent_writes`（fork 后双侧各自写入互不可见）实测。

## D3 — 归档/恢复载体：JSONL 文件搬移 → lifecycle 字段翻转

- 现役锚点：`server/routes/sessions.ts:2569-2660`（archive：归档目录搬移 + childMode 三态）；`:2813`（restore：移回活跃目录）；`:2882`（archived/delete）；`:2517`（cleanup，maxAgeDays 默认 90，严格 `mtime < cutoff`）。
- 本实现：`sessions.lifecycle`（active/archived）+ `archived_at_unix_ms` 字段翻转；永久删除为 14 步 FK 序purge（checkpoint_file_versions→…→sessions）；cleanup 严格 `archived_at < cutoff`（与现役严格小于一致，测试用 50ms 真实时钟差验证边界）。
- 语义等价：「可恢复归档」「仅归档态可永久删除」「恢复后可再归档」逐一保持；「文件及 sidecar 移回目录」的观察面差异属 D3 载体差异（R08 桌面侧不直接读文件目录，走 REST）。

## D4 — 检查点/rewind 暴露面：引擎内工具面 → REST 资源面

- 现役锚点：`core/session-checkpoints.ts:74-101`（latest 覆盖 / 具名冲突 / 200 上限）；`core/workspace-snapshots.ts`（快照写回）。现役由引擎内 checkpoint tracker 触发，无独立 REST 资源。
- 本实现：`POST/GET /lingxi/v1/sessions/{id}/checkpoints`、`POST .../checkpoints/delete`、`POST .../rewind`、`POST .../rewind/preview`（预览为 T03 新增的只读端点，现役 UI 的"回退预览"语义由此承载）。
- 保持一致：latest 覆盖、具名 409 `checkpoint_conflict`、200 上限（第 201 条裁最老非 latest）逐条实测（`checkpoint_crud_matches_incumbent_shape`、`checkpoint_window_evicts_oldest_non_latest_at_201`）。

## D5 — rewind 无服务端确认卡：REST 直执 + 预览端点承载确认语义

- 现役锚点：`core/session-turn-actions.ts:382-384`（busy 闸）与 UI 侧确认交互；`core/preferences-manager.ts:322-324`（文件回退偏好默认关）。
- 本实现：无服务端确认卡状态机；客户端应先调 `rewind/preview`（只读、返回逐文件 restore/conflict 判定）再决定是否执行 rewind。偏好闸保持：`restore_files=true` 且偏好未开启 → 403 `file_rollback_disabled`。
- 偏离方向：交互形状差异（确认责任移到调用方），安全语义不降级（偏好闸 + 冲突全拒在位）。

## D6 — retry 两段式：端点内"重置+重跑"→「重置并返回回合输入」+ 客户端重发 execute

- 现役锚点：`core/session-turn-actions.ts:78-165`（目标解析：user→该回合 / assistant→前一 user / 缺省→最近 user）；`:454-571`（`commitRetryBranch` 重置分支）；`server/routes/sessions.ts:1558`（retry 端点内完成重置与重跑）。
- 本实现：`POST .../turns/retry` 只做强一致部分——busy 闸、归属+生命周期闸、服务端解析重置点（**不接受客户端给的 newHead**，RC-1 修复：重置点只能由服务端从当前分支解析）、写 `reset:*` 标记、返回 `turnInputMessageId + turnInputContentJson + newHead`；客户端拿回合输入重发 `execute` 产生新 run。
- 理由：传输预算内 HTTP execute 阻塞至 run 终态，端点内串行"重置+重跑"会把重置事务与整段模型调用绑进同一请求生命周期；两段式使重置成为独立可证实的强一致步骤。新 run 绝不覆盖旧 run（`retry_resets_branch_and_new_run_never_overwrites_old` 实测旧 run 保持 completed、新 run 另起）。
- 根回合重试：重置目标为「根 user 回合」时 `newHead=null`（对照现役 `retryBranchParentId = null`），分支投影只剩 reset 标记（`branch_reset_to_null_head_leaves_only_the_marker` 实测）。

## 附带显式新增（非语义偏离，如实申报）

- cleanup 响应新增 `skippedBusy` 字段：现役按文件 mtime 删除、无"忙会话"概念；本实现保护在跑 run 的存储不被抽走，忙会话显式跳过并计数上报（不静默）。
- 409 `child_sessions_present` 响应携带 `childCount` 数值（现役同款细节字段）。
- 搜索 LIKE 通配符注入免疫：服务层转义 `% _ \`（`ESCAPE '\'`），`admin_search_title_content_owner_and_like_escape` 实测。

---

# REPAIR-R1 追加条目（2026-10-09；R1 评审 FAIL 后的同根因修复）

## D7 — rewind 语义修订：整批 409 → 内容级回滚 + 逐文件三档（**取代 D1 的拒绝式语义**）

- 裁决来源：R1 评审 FINDING-05（管理者裁决）——rewind 必须是**内容级回滚**：检查点真实存内容，无冲突文件真实写回；禁止「任何检查点后变化都永远 409」状态。
- 现役锚点：`core/workspace-snapshots.ts:584-620`（`_restoreChange` 盲写回快照字节）、`restoreTurn`（无检查点 → `{ok:false, reason:"no_checkpoint"}`，逐文件失败不阻塞回合提交）；`core/session-turn-actions.ts:189-344`（rewind = 分支头回移 + reset 标记，文件恢复与分支重置解耦）。
- 本实现（REPAIR-R1 后）：`file_checkpoints` 从死表激活为内容存档（`PRIMARY KEY (checkpoint_id, file_path)`，`REFERENCES checkpoints`，BLOB 原始字节 + utf-8/base64 形态标记）。rewind 逐文件三档判定：当前==检查点目标 → `skipped`；当前内容被系统见证过（该文件在本会话任一检查点记录过的哈希）→ `restored`（真实写回存档字节并复读校验）；系统从未见过的版本或文件被删 → `conflicted`（**该文件用户修改原样保留，绝不覆盖**）。冲突不阻塞分支回移（与现役 restoreTurn 的「逐文件失败不阻塞」同构），收据逐文件如实列 `restored/conflicted/skipped/failed` + `externalEffects: "not_rolled_back"`。
- 与 D1 的关系：D1 的「不存内容字节 + 任何分歧整批 409」被本条取代（D1 文本保留作历史记录）。D1 的保护目标——绝不覆盖用户外部修改——在逐文件 `conflicted` 档中原样保留并加强（精确到单文件而非整批）。
- 测试锚点：`rewind_restores_content_with_per_file_verdicts_and_branch_rewinds`（外部修改→conflicted+保留+分支回移；删除→conflicted 不重建；见证版本→restored 真实字节；未改→skipped；预览同口径只读）。

## D8 — retry 的 fileRollback 子句：未实现 → 按现役契约实现（R1 FINDING-02）

- 现役锚点：`server/routes/sessions.ts:1584-1597`（非法值→400 `invalid_file_rollback`；workspace+偏好未开→403 `file_rollback_disabled`）；`core/session-turn-actions.ts:451-460`（`performWorkspaceFileRollback` 先于 `commitRetryBranch`，逐文件失败不阻塞提交，报告随 `session_branch_reset` 事件携带）。
- 本实现：路由层解析 `fileRollback`（缺省/none/workspace，其余 400，绝不静默降级成 none）；workspace 时以「回合输入消息对应的最新检查点」为锚点（`checkpoints.turn_input_message_id`，建检查点时回填）做与 rewind 同一套内容级恢复；无锚点检查点 → `fileRollbackReport.ok=false, reason="no_checkpoint"` 且分支照常重置。报告随响应与 `session_branch_reset` 事件字段返回。
- 测试锚点：`retry_file_rollback_contract_matches_incumbent`（400/403/no_checkpoint/restored+conflicted 全链路）。

## D9 — fork 副本的 run_id：置 NULL（对「全字段保真」的唯一有意偏离）

- 现役锚点：`pi-coding-agent/dist/core/session-manager.js:1113`（`createBranchedSession` 浅复制全部条目字段——但现役按会话分 JSONL 文件存储，条目天然只活在所属会话文件内，无跨会话引用概念）。
- 本实现偏离：fork 复制的消息行 `run_id` 置 `NULL`（其余字段——消息 ID、role、content、model_call_id、committed_at_unix_ms、entry_type——原样保真，FINDING-04 修复保持）。
- 理由（关系模型硬性约束）：`messages.run_id REFERENCES runs(run_id)`（D10）下，副本若持有源会话 runs 行引用，源会话永久删除时 runs 行被副本引用而 FK 拒绝——现役无此耦合。源 run 的可追溯性由消息 ID（`user:{run_id}` / `{run_id}-final`）原样保留承载。实测锚点：`management_surface_*`（fork 后永久删除源会话 200）；`fork_preserves_message_fields_verbatim`（副本 run_id 为空、其余字段逐相等）。

## D10 — messages.run_id：可空但保 REFERENCES（R1 FINDING-06）

- 背景：v1 schema `run_id TEXT NOT NULL REFERENCES runs(run_id)`；T03 初版 v8 重建时丢失 FK。REPAIR-R1 恢复为 `run_id TEXT REFERENCES runs(run_id)`（**可空**）。
- 可空语义：用户输入消息不归属任何 run 行（run_id=NULL）；归属追溯经消息 ID `user:{run_id}`（裸 `WHERE run_id=?` 证据查询不会把用户输入误读为 run 的最终消息）。assistant/结果消息 run_id 必填照旧。幽灵 run_id（不存在的 run）被 FK 响亮拒绝（`ghost_run_id_is_rejected_by_foreign_key` 实测）。

## REPAIR-R1 附带显式新增（非语义偏离，如实申报）

- 会话 id 撞库（fork `newSessionId` / 管理面创建）→ 响亮 409 `session_exists`（对照现役 `server/routes/sessions.ts:587` 的 active 路径冲突 409 语义）；修复前落 Internal 500（R1 观察项）。
- 分支历史投影（GET branch）响应新增 `modelCallId`、`committedAtUnixMs` 两字段（FINDING-04 保真修复的观察面；纯增量，客户端不读不受影响）。
- rewind 收据形状：`verified: string[]` → `restored/conflicted/skipped/failed: string[]` + `externalEffects`（D7 配套；T03 候选内部迭代，现役无对应 HTTP 形状可比）。
- 检查点锚点列 `checkpoints.turn_input_message_id`（retry fileRollback 锚点；现役快照按 turnInputEntryId 取回合，`session-turn-actions.ts:451-460` 同语义）。
