# R06-T03 独立对抗性审查报告（REVIEW-01，第 1 轮）

- REVIEW_ID: R06-T03-REVIEW-01
- 审查对象: 工作区未提交候选（base `4920c59e38ddf69c8b846fb1aea896fca70d939d` + 工作区改动）
- 审查代理未参加实现与修复；全部结论由本代理亲自复核/复跑得出。
- 审查产物目录: `artifacts/rust-tauri/R06/T03-REVIEW-01/`（本文件 + `probe/` + `probe2/` 两个活体探针工程）

## 0. 基础事实复核（全部亲自执行）

| 项 | 声称 | 复核结果 | 证据 |
|---|---|---|---|
| CANDIDATE_DIGEST | `c65faae9…bf9f60` | **一致**（`bash scripts/rust-tauri/r06_candidate_digest.sh` 实跑输出逐字符相同，exit 0） | 本代理终端实跑 |
| 工具链 | cargo/rustc 1.98.1（rustup shim） | 一致（`cargo 1.98.1 (797e8a9bc)` / `rustc 1.98.1 (48a229cea)`） | 同上 |
| workspace 全量 | 124 套件 / 1643 passed / 0 failed | **一致**（本代理后台完整复跑 `cargo test --locked --workspace`，124 个 `test result:` 行，awk 求和 PASSED=1643 FAILED=0，末尾 `WORKSPACE_EXIT=0`，任务退出码 0） | bkwah2126.output |
| R05 关键套件真实在跑 | — | 单独特定 `--test` 复跑（防合并 run 无逐测试输出）：`r05_t01_model_plane` 24/24、`r05_t02_credentials` 38/38、`r05_t03_protocol_adapters` 12/12、`r05_t07_rr1_usage_ledger` 15/15、`run_lifecycle` 12/12、`event_subscription` 10/10、`session_serialization` 12/12，全部 0 failed、exit 0。ModelGateway/凭证/协议族/Usage 台账/Run 状态机/事件订阅均有真实测试体在执行 | 本代理终端实跑 |
| T03 新增套件 | service 10 + adapters 12 + kernel 9 | 复跑 `r06_t03_session_tree`（service）10/10 逐测试名确认在列（fork 深度 409、busy 三操作 409、A05、A06、跨主体 403 等），exit 0 | 本代理终端实跑 |
| v8 迁移数据保真 | messages 重建不丢数据 | **成立**（probe2：v7 库 2 消息逐字段保留含 `model_call_id='mc-9'`，parent/branch 默认 NULL、entry_type='message'、lifecycle 默认 active、索引重建 2 个，`PROBE2_OK` exit 0）。大库耗时未实测（执行者已申报） | `probe2/run_output.txt` |
| RED→GREEN | 02_red_tests.txt | 证据内编译错误 E0433 可见，但 `=== exit_code:` 字段为空（退出码未记录），见 FINDING-09 | artifacts T03/02_red_tests.txt |

**结论：候选未漂移；全量回归与抽查真实有效；迁移保真成立。以下问题独立成立，不因此打折。**

## 1. FINDINGS

---

### FINDING-01：latest 覆盖后 checkpoint_file_versions 组合断裂（PK 冲突 500 + 旧文件版本残留污染 rewind 核验面）

- SEVERITY: **HIGH**
- REQUIREMENT_ID: R06-T03 Deliverable「回退检查」/ 差异台账 D4（latest 覆盖保持现役形状）/ R06-A06 间接（rewind 核验面被污染）
- FILE_AND_LINE:
  - `rust/crates/lingxi-adapters/src/storage/session_tree.rs:578-586`（`Overwrite` 分支只 `UPDATE checkpoints`，**不删旧 `checkpoint_file_versions`**）
  - 对照 `:1002-1005`（`delete_checkpoint` 正确先删文件版本再删检查点行——同一文件内两套标准）
  - `rust/crates/lingxi-service/src/session_tree.rs:575-600`（`upsert_checkpoint` 与逐文件 `record_file_version` **分属两次独立写事务，非原子**；注释自称「latest 覆盖 = 版本行随行替换」与实际不符）
- OBSERVED_BEHAVIOR（probe1 实证，`probe/run_output.txt` exit 0）:
  - 场景 A：latest 覆盖后对同一文件 `record_file_version` → `StorageError::Internal { detail: "UNIQUE constraint failed: checkpoint_file_versions.checkpoint_id, checkpoint_file_versions.file_path" }`。生产链上即 `POST /sessions/{id}/checkpoints` 第二次 latest 携带与上次相同的文件 → **HTTP 500 `storage.internal`**（`EndpointError::storage` 把 Internal 映射为 500，lib.rs:2232 一带）。
  - 场景 B：latest 覆盖携带不同文件集合 → 旧文件版本行残留：`list_file_versions` 返回 `["/tmp/f1", "/tmp/f2"]`，f1 属于已被覆盖的旧目标状态。rewind 时会对「当前 latest」核验一个早已不属于它的文件，正常用户也永远拿到 409 `rewind_file_conflict`（核验面污染）。
- EXPECTED_BEHAVIOR: 现役语义「latest 覆盖」= 该检查点整体指向新状态（`core/session-checkpoints.ts:14-16,74-101`）；覆盖后旧文件版本不得残留；同名文件可正常重新记录；upsert 与版本替换应同事务原子完成。
- REPRODUCTION: `cd artifacts/rust-tauri/R06/T03-REVIEW-01/probe && cargo run`（源码与输出已归档）。生产复现路径：`POST checkpoints {name:"latest", filePaths:["f1"]}` → `POST checkpoints {name:"latest", filePaths:["f1"]}`（第二次 500）；或第二次带 `["f2"]`（200 但 f1 残留，随后 rewind 409）。
- ROOT_CAUSE: (1) `upsert_checkpoint` 的 Overwrite 分支缺 `DELETE FROM checkpoint_file_versions WHERE checkpoint_id=?`；(2) service 把 upsert 与版本记录拆成两次队列提交，无跨调用事务，也无失败回滚（第一次 record 失败后检查点行已指向新 target，处于半更新态）。
- SAME_ROOT_CAUSE_PATHS: rewind 的核验读取 `list_file_versions`（同一残留面）；`cleanup`/`delete_archived_session` 的 FK 序删除不受影响（它们整棵删）。凡依赖「checkpoint_id 的版本集合 = 该检查点真实文件集」的读取方全被污染。
- IMPACT: 「latest 覆盖」是执行者自宣整叶实现（叶图 checkpoint 三叶 + D4 声称与现役一致）的核心动作；携带文件版本的 latest 在第二次使用时**确定性 500**——功能可见面真实损坏，不是边角。
- REQUIRED_FIX: Overwrite 分支同事务内删除旧文件版本行；service 改为单次提交（upsert+版本记录进同一 `with_write_txn`），或 adapters 提供组合原语；补充 latest+filePaths 重复覆盖的回归测试。
- REGRESSION_TESTS: 现有全部测试网均未覆盖「latest 覆盖 × filePaths」组合——service `checkpoint_crud_matches_incumbent_shape` 与 adapters `named_checkpoint_latest_overwrites` 的 latest 覆盖**都不带文件版本**，带文件版本的用例又都不是 latest。需新增：latest 覆盖带相同文件（200）、带不同文件（旧版本不残留）、覆盖后 rewind 按新集合核验。

---

### FINDING-02：叶 #55（SESSIONS-TURNS-RETRY）fileRollback 子句未实现，「真实实现」归类夸大

- SEVERITY: MEDIUM
- REQUIREMENT_ID: R06-T03 Goal「保留现役可见功能而非只保留消息列表」/ 任务书步骤 3「沿用现役产品语义」
- FILE_AND_LINE:
  - 现役：`server/routes/sessions.ts:1584-1607`（retry 接受 `fileRollback`，`"workspace"` 需偏好开启否则 403）+ `core/session-turn-actions.ts:454-571`（fileRollbackReport）
  - 候选：`rust/crates/lingxi-service/src/lib.rs` `RetryRequestBody`（fork 端点旁，`deny_unknown_fields`，仅 `targetMessageId`——传 `fileRollback` 直接 400）
  - 叶图 `docs/rust-tauri/R06/R06_LEAF_MAP.json` 叶 #55 `t03_anchor` 声称「fileRollback偏好闸403」（与实际形状不符：不是闸 403，是字段被拒 400）
- OBSERVED_BEHAVIOR: 现役 retry 的文件回退交互（可选 workspace 回退 + 偏好闸 403）在候选中整体缺席；客户端按现役契约发送 `fileRollback:"workspace"` 会收到 400 请求错误而非 403 偏好闸。
- EXPECTED_BEHAVIOR: 整叶实现应保留该子句或将其显式归入份额/后续并申报差异台账。
- REPRODUCTION: `POST /lingxi/v1/sessions/{id}/turns/retry {"targetMessageId":"m","fileRollback":"workspace"}` → 400（deny_unknown_fields）。
- ROOT_CAUSE: D6 两段式改造时把 retry 载荷裁剪为仅 targetMessageId，fileRollback 子句未随台账申报（D1-D6 及附带新增均未提及）。
- SAME_ROOT_CAUSE_PATHS: rewind 端点侧的 `restoreFiles`+偏好闸（403 `file_rollback_disabled`）已做——说明执行者知道该语义，retry 侧属遗漏而非刻意设计。
- IMPACT: 叶归类失真（implemented→应降 share/deferred 并申报）；现役客户端契约不兼容点未入台账。
- REQUIRED_FIX: 要么实现 retry 的 fileRollback 子句（含偏好闸 403），要么叶 #55 降为 share 并在差异台账申报「retry 文件回退后续阶段」。
- REGRESSION_TESTS: 新增 retry 携带 fileRollback 的正反测试（偏好关→403；开→按 rewind 同款冲突核验）。

---

### FINDING-03：GET /sessions 主列表不过滤归档会话

- SEVERITY: MEDIUM
- REQUIREMENT_ID: R06-T03 任务书步骤 1「按R00叶子功能迁移…归档…实际入口」/ 叶 #26/#27「移入可恢复归档」的可见面
- FILE_AND_LINE:
  - `rust/crates/lingxi-adapters/src/storage/run_store.rs:407-414`：`SELECT … FROM sessions ORDER BY created_at_unix_ms DESC`——**无 `WHERE lifecycle`**
  - 路由链：lib.rs:5119-5120 `GET /lingxi/v1/sessions` → list_sessions(lib.rs:2986) → `sessions.list_for` → 上述 SQL
  - 对照：同库 `session_admin.rs:554` `list_archived_sessions` 按归档过滤（归档列表面完整）；现役归档即从活跃列表消失（文件搬移出目录）
- OBSERVED_BEHAVIOR: 归档后会话仍出现在主列表；主列表与「已归档列表」不再是互补两面，归档的可见语义（从日常工作面消失）未保持。
- EXPECTED_BEHAVIOR: 主列表只含 active（或至少提供等价过滤）；叶 #26/#27 若坚持 implemented，应含此可见面。
- REPRODUCTION: 创建会话 → `POST sessions/{id}/archive` → `GET /lingxi/v1/sessions` 仍含该会话。现有测试 `management_surface_…`（r06_t03_session_tree.rs:876）只查 `/sessions/archived` 可见（:992），**从未断言主列表不含已归档**——测试盲区。
- ROOT_CAUSE: list_sessions 是 R03 既有函数，v8 加列后未补 lifecycle 过滤；T03 管理面测试只验证归档列表正向可见，漏掉主列表负向断言。
- SAME_ROOT_CAUSE_PATHS: 任何直接读 `sessions` 表全量列举的消费面（search 若无 lifecycle 过滤同样会把归档会话搜出——`session_admin.rs` search 未按 lifecycle 过滤，需同查；`find` 同理）。
- IMPACT: 归档功能对用户可见面不完整（「归档了还在列表里」）；与现役可观察行为不一致且未申报差异。
- REQUIRED_FIX: 主列表 SQL 增加 lifecycle 过滤（或路由参数化）；search/find 明确归档包含策略并与现役对齐；台账申报。
- REGRESSION_TESTS: 归档后主列表不含、restore 后复现；search 对归档会话的行为断言。

---

### FINDING-04：fork 复制的消息行字段失真且未申报（model_call_id 置 NULL、committed_at_unix_ms 改写为 fork 时刻）

- SEVERITY: LOW（观察级；但属台账漏报）
- REQUIREMENT_ID: R06-T03 步骤 2「保持 session/parent/branch 和 message 稳定 ID」/ D2 声称「仅 parent 重链」
- FILE_AND_LINE: `rust/crates/lingxi-adapters/src/storage/session_tree.rs` fork 复制段：`SELECT message_id, parent_message_id, run_id, role, content_json`（**不取 model_call_id / committed_at_unix_ms**），INSERT 时 model_call_id 写 NULL、committed_at_unix_ms 写 `req.now_unix_ms`（fork 时刻）。对照现役 `session-manager.js:1113 createBranchedSession`：`{...entry, parentId}` 完整浅拷贝（全部字段保留）。
- OBSERVED_BEHAVIOR: fork 后共享历史消息的模型调用溯源字段丢失、提交时间被改写；D2 台账声称差异「仅在载体形状」「仅 parent 重链」不准确。
- EXPECTED_BEHAVIOR: 要么逐字段保留（与现役一致），要么在台账显式申报字段级差异及影响面。
- REPRODUCTION: fork 后直查 `messages`：`SELECT model_call_id, committed_at_unix_ms FROM messages WHERE session_id=<new>` 与源会话同行对比。
- ROOT_CAUSE: 复制 SELECT 列清单裁剪 + 时间戳复用请求时钟。
- SAME_ROOT_CAUSE_PATHS: Usage 台账/按 model_call_id 的审计联查若跨分支引用共享历史消息，会在分叉侧断链。
- IMPACT: 审计/溯源面弱化；「ID 稳定」成立但「记录保真」不成立；违反 RC-2 差异申报完整性。
- REQUIRED_FIX: 复制时保留全部原始字段（仅 parent/branch 按分叉重链）。
- REGRESSION_TESTS: fork 后逐字段等值断言（含 model_call_id、committed_at_unix_ms）。

---

### FINDING-05：rewind 只验证不恢复——「文件恢复」可见功能实质缺席（D1 解释需管理者裁决）

- SEVERITY: MEDIUM（语义裁决项，非代码 bug）
- REQUIREMENT_ID: R06-T03 步骤 4 原文「回退检查checkpoint及真实文件版本，冲突拒绝」/ R06-A06
- FILE_AND_LINE: schema `checkpoint_file_versions`（仅存 `sha256+size_bytes`，无内容字节，migrations.rs v8）；service rewind 一致分支返回 `verified` 回执 + `externalEffects:"not_rolled_back"`；对照现役 `core/workspace-snapshots.ts:584-620` 真实内容写回。
- OBSERVED_BEHAVIOR: 任何「检查点后文件发生变化」的场景——**包括模型/工具自己在后续回合改了文件**——rewind 一律 409，且系统内不存内容字节，永远无法执行恢复。全部一致时 rewind 是 no-op。即：rewind 的文件维度只能回答「没动过」，不能执行「回退」。
- EXPECTED_BEHAVIOR（字面/现役）: rewind 应在无冲突时把文件恢复到检查点状态；有冲突才拒绝。任务书字面「回退检查…真实文件版本，冲突拒绝」可被读作「检查」也可被读作「检查并恢复」；A06 通过条件「检测冲突并保护用户修改；不能伪称全部撤销」字面上被满足。
- REPRODUCTION: checkpoint → 正常会话继续（模型改文件）→ rewind → 409。
- ROOT_CAUSE: 设计决策（D1 强化）+ 存储形态（不存内容字节）共同使「恢复」在结构上不可能。
- SAME_ROOT_CAUSE_PATHS: `file_checkpoints` 表（v8 新建）在全代码库**无任何读写**（仅删除会话时清理）——若本意是内容承载表则未完成接线；`rewind/preview` 的 restore 判定对「模型改过文件」场景同样永远给 conflict。
- IMPACT: 现役 rewind 的核心用户价值（回退文件）在候选中不存在；D1 以「强化」申报，但强化的是冲突检测，**削除的是恢复能力**——二者应分别裁决。
- REQUIRED_FIX（若管理者不接受纯验证语义）: 存内容字节（或接 file_checkpoints）并实现一致时写回 + 冲突拒绝的双分支；更新 D1 表述。
- REGRESSION_TESTS: 一致时文件内容确实回到检查点态；模型改文件后 rewind 的期望行为按裁决落定。
- REVIEW 立场: A06 字面不挂；此项作为**需求解释分歧**上报，不计入 FAIL 的独立理由，但管理者必须在验收时明确裁断。

---

### FINDING-06：v8 messages 重建移除 run_id 外键约束（NOT NULL REFERENCES → TEXT 可空）

- SEVERITY: LOW
- REQUIREMENT_ID: 历史兼容 / R03 消息写入完整性
- FILE_AND_LINE: `rust/crates/lingxi-adapters/src/storage/migrations.rs:457`（v8 表 `run_id TEXT,`）对照 `:177`（v7 及之前 `run_id TEXT NOT NULL REFERENCES runs(run_id)`）
- OBSERVED_BEHAVIOR: v8 后消息行的 run 归属约束消失（可空、无外键）；reset 标记行（`reset:*`）确实无 run_id 故需要可空，但外键可同时保留（REFERENCES 允许 NULL）。
- EXPECTED_BEHAVIOR: `run_id TEXT REFERENCES runs(run_id)`——可空但保留参照完整性。
- ROOT_CAUSE: 重建时为容纳 reset 标记去掉了整条约束。
- IMPACT: 孤儿 run_id/拼写错误不再被库层拦截；删除 run 的 FK 序失去库层兜底（当前靠应用层 14 步删除序）。
- REQUIRED_FIX: 下一 schema 版本补回 REFERENCES。
- REGRESSION_TESTS: 插入指向不存在 run 的消息应失败。

---

### FINDING-07：file_checkpoints 表为死表（零读写）

- SEVERITY: LOW
- FILE_AND_LINE: migrations.rs v8 建表；全代码库 grep 仅删除会话的清理路径触及。
- OBSERVED_BEHAVIOR: 新建表无任何读写代码，属未接线结构。
- IMPACT: 无行为影响；与 FINDING-05 合看加剧「内容恢复曾规划但未完成」的观感；若确属 R08 预留应在叶图/台账点名。
- REQUIRED_FIX: 申报归属（R08 预留）或移除。
- REGRESSION_TESTS: —

---

### FINDING-08：archive_children 分支对已归档后代计数虚增

- SEVERITY: LOW
- FILE_AND_LINE: `rust/crates/lingxi-adapters/src/storage/session_admin.rs:354-391`：递归分支带 `AND lifecycle='active'` 过滤（只归档活跃的），但 `outcome.archived_children += 1` 对**每个被遍历到的后代**无条件递增。
- OBSERVED_BEHAVIOR: 子树中已归档的后代不会被重复归档，但仍计入 `archivedChildren` 响应数。
- EXPECTED_BEHAVIOR: 计数 = 本操作实际翻转的数量（或字段改名/口径申报）。
- IMPACT: 响应字段数值夸大；现役 anchor（sessions.ts:2569-2660）语义为「归档了的直接/递归后代数」。
- REQUIRED_FIX: 仅在真实翻转时计数。
- REGRESSION_TESTS: 混合子树（部分已归档）的计数断言。

---

### FINDING-09：RED 证据退出码字段空缺

- SEVERITY: LOW
- FILE_AND_LINE: `artifacts/rust-tauri/R06/T03/02_red_tests.txt` 的 `=== exit_code:` 行为空。
- OBSERVED_BEHAVIOR: 编译错误（E0433）构成 RED 事实可接受，但任务纪律要求记录真实退出码；字段空缺。
- REQUIRED_FIX: 补录命令与退出码。
- REGRESSION_TESTS: —

---

### 观察项（不计 finding）：fork new_session_id 撞库映射 500

`fork_session` 对撞库的 `UNIQUE` 约束经 `map_rusqlite`（migrations.rs:806）落入 `StorageError::Internal` → HTTP 500；现役同类为响亮 409/400。触发面小（客户端自带新 id 且撞库），记为后续收口项，不单独成 finding。

search limit 无上限钳制：**与现役一致**（现役 `Number(c.req.query("limit"))` 原样透传，sessions.ts:875），不构成偏离，撤销此前怀疑。

## 2. 生产路径真实性（第二部分）

逐环亲验，链完整：HTTP 路由（lib.rs:5134 等 route 注册）→ auth.rs classify_route（新增 sessions 管理面/树端点全部归 chat scope，fail-closed LocalOnly 兜底）→ `gate_session_tree_write`（归属+生命周期 409/403）/ `gate_session_owned` → SessionService（session_tree.rs/session_admin.rs）→ adapters 单写者队列 + `with_write_txn` 真实 SQLite 事务 → kernel 纯逻辑（深度闸/重置点解析/检查点规划/路径闸/sha256）。execute 生命周期闸（归档 409 session_not_active）在位；final 消息 parent=分支头且同事务推进（run_store.rs）。未发现平行构造者或测试旁路。**FINDING-01/03 属于链上真实缺陷而非断链。**

## 3. 对抗性场景结论（第三部分）

已实测成立：深度闸 409（含事务内重查）、busy 真实租约 409（fork/retry/rewind 三操作）、跨主体 403（6 端点）、A05 双侧写入隔离、retry 新 run 不覆盖旧 run、A06 外部修改/删除全拒+verified 回执、越界路径 403、v7→v8 保真（probe2）、attempt 重试不重复落盘 user 消息、LIKE 注入免疫。
已实测**不成立**：latest 覆盖×文件版本（FINDING-01）、归档主列表可见性（FINDING-03）、fork 字段保真（FINDING-04）、retry fileRollback（FINDING-02）。
执行者自认四风险复核：(a) retry 两段式——形状真实可工作，但 fileRollback 裁剪未申报（FINDING-02）；(b) skippedBusy——申报属实，观察接受；(c) 仅存哈希——即 FINDING-05，需裁决；(d) v8 迁移——保真实测成立，大库耗时仍未实测（维持申报）。

## 4. R05 及更早回归（第四部分）

见第 0 节：全量 124/1643/0 复跑一致；R05 七个关键套件单独复跑非空且全绿；T03 套件 10 测试逐名在列。messages 主键重建未破坏既有写入路径（session_serialization/run_lifecycle 绿）。

## 5. 防虚假完成（第五部分）

16 个「真实实现」叶中，checkpoint 三叶的实际成色受 FINDING-01 直接打击（latest 覆盖是 D4 声称与现役一致的核心动作，带文件版本即 500）；叶 #55 应降级（FINDING-02）；叶 #26/#27 可见面不完整（FINDING-03）。其余 implemented 叶抽查均有真实生产链+正负测试，未发现以内部机制测试冒充用户语义的新增案例。

## 6. 裁决

**VERDICT: FAIL**

阻断项：FINDING-01（HIGH，latest 覆盖×文件版本确定性 500 + rewind 核验面污染——自宣 implemented 的核心动作真实损坏）、FINDING-02 与 FINDING-03（MEDIUM，叶归类夸大/可见面不完整）。FINDING-05 为需管理者裁决的需求解释分歧。修复 FINDING-01/02/03 并补对应回归测试后可申请第 2 轮复审；LOW 组（04/06/07/08/09）可随修复一并收口。
