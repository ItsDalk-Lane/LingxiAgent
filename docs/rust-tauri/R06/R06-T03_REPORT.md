# R06-T03 会话树、重试、fork 与回退 — 实施报告

- TASK_ID: R06-T03
- TASK_BASE_SHA: `4920c59e38ddf69c8b846fb1aea896fca70d939d`（分支 `codex/rust-tauri-migration`）
- 验收 ID: R06-A05（fork 不串写）、R06-A06（rewind 不覆盖外部修改）
- 工具链: `/Users/study_superior/.cargo/bin/cargo`（rustup shim → cargo/rustc 1.98.1，仓库根 `rust-toolchain.toml` 锁定）；全程未使用 homebrew cargo 1.93.0（证据 `artifacts/rust-tauri/R06/T03/00_toolchain.txt`）
- 候选 digest：由 `scripts/rust-tauri/r06_candidate_digest.sh` 在全部文件写完后计算，值见 handback，**本报告不内嵌**（报告自身在 digest 范围内）

## 一、已完成的 Steps（任务书步骤逐条核对）

1. 勘察现役端到端生产链（fork/retry/rewind/检查点/归档管理面）→ 完成，锚点见第五节与差异台账 `07_diff_ledger.md`（RC-1/RC-2）。
2. RED：先写现役对照式失败测试 → 完成（`02_red_tests.txt`）。
3. GREEN：schema v8 迁移 + kernel 纯逻辑 + adapters 存储 + service 路由/闸 → 全绿（`03_fmt.txt`/`04_clippy.txt`/`05_targeted_tests.txt`，fmt/clippy 均有失败→修复→复检两阶段真实记录）。
4. workspace 全量回归 → 1643 passed / 0 failed，exit 0（`06_workspace_regression.txt`）。

## 二、已交付的 Deliverables

- SessionService：`rust/crates/lingxi-service/src/session_tree.rs`（fork/retry/rewind/preview/检查点）+ `session_admin.rs`（创建/改名/置顶/归档/恢复/删除/清理/搜索/记忆开关）。
- 树/分支索引：schema v8（`r06_t03_session_tree` 迁移）——sessions +11 列（parent_session_id/fork_point_message_id/lineage_depth/lifecycle/pinned_at/pin_order/memory_enabled/authorized_folders/permission_mode/archived_at/last_activity）；新表 `session_branch_heads`、`checkpoints`、`checkpoint_file_versions`（仅存 sha256+size，无内容字节）、`file_checkpoints`；messages 重建主键 `(session_id, message_id)` + `parent_message_id/branch_id/entry_type`。
- 回退检查：rewind 前 sha256 逐文件比对，分歧（含文件被删）整批 409 全拒（A06，台账 D1）；`rewind/preview` 只读预检。

## 三、实际生产调用链（无平行构造者）

HTTP → `auth.rs classify_route`（chat scope）→ 路由（`lib.rs`）→ 闸（`gate_session_tree_write` 归属+生命周期 / `gate_session_owned` 仅归属）→ service（`session_tree.rs`/`session_admin.rs`）→ adapters（`storage/session_tree.rs`/`session_admin.rs` 真实 SQLite 事务）→ kernel 纯逻辑（重置点解析/检查点规划/路径闸）。
execute 走既有生产链：路由 → 生命周期闸 → supervisor busy 闸 → run；final 消息 parent 链接修复（`run_store.rs`）使分支投影链不断裂。无第二条旁路实现。

## 四、代码修改清单

修改（已跟踪，相对 base 的 diff 统计）：
- `rust/crates/lingxi-adapters/src/storage/migrations.rs` +110（schema v8 迁移）
- `rust/crates/lingxi-adapters/src/storage/mod.rs` +2（`pub mod session_admin`）
- `rust/crates/lingxi-adapters/src/storage/run_store.rs` +69/-（final 消息 parent 链接修复 + 消息写入 entry_type）
- `rust/crates/lingxi-kernel/Cargo.toml` +4（sha2/hex 依赖）
- `rust/crates/lingxi-kernel/src/lib.rs` +1（`pub mod session_tree`）
- `rust/crates/lingxi-service/src/auth.rs` +70（sessions 管理面路由分类，chat scope）
- `rust/crates/lingxi-service/src/lib.rs` +1117（路由、闸、错误映射、execute 生命周期闸）
- `rust/crates/lingxi-service/src/runs.rs` +54（分支投影读取对齐）
- `rust/Cargo.lock` +1
- `docs/rust-tauri/R06/R06_PROGRESS.json`（T02 收口/T03 开启，任务书指定候选一部分，未回退）

新增（未跟踪）：`lingxi-kernel/src/session_tree.rs`（389 行）、`lingxi-adapters/src/storage/session_tree.rs`（1013 行）、`lingxi-adapters/src/storage/session_admin.rs`（742 行）、`lingxi-service/src/session_tree.rs`（678 行）、`lingxi-service/src/session_admin.rs`（375 行）、测试 `lingxi-adapters/tests/session_tree_r06t03.rs`（1095 行/12 测试）、`lingxi-service/tests/r06_t03_session_tree.rs`（1103 行/10 测试）、证据目录 `artifacts/rust-tauri/R06/T03/`。

## 五、现役语义逐条锚定（RC-2，行号亲验于 base 工作区）

- fork：`core/session-coordinator.ts:3065` 调用 `node_modules/@earendil-works/pi-coding-agent/dist/core/session-manager.js:1113 createBranchedSession`（根→叶路径、entry ID 保留）；深度上限 `server/routes/sessions.ts:1738 MAX_FORK_LINEAGE_DEPTH=2`、fork 端点 `:1768`。
- retry：`server/routes/sessions.ts:1558`；目标解析 `core/session-turn-actions.ts:78-165`（user→该回合/assistant→前一 user/缺省→最近 user）；busy `:382-384`；`commitRetryBranch :454-571`。
- 检查点：`core/session-checkpoints.ts:14-16`（latest/200 上限）`:74-101`（覆盖/冲突）；回退偏好默认关 `core/preferences-manager.ts:322-324`；现役盲写回 `core/workspace-snapshots.ts:584-620`（台账 D1 对照）。
- 管理面：`server/routes/sessions.ts` rename:2493 / cleanup:2517（默认 90、严格 `< cutoff`）/ archived:2555 / archive:2569-2660（409 child_sessions_present 带 childCount、detach 摘直接子、archive_children 递归跳过流式并计数）/ restore:2813 / archived-delete:2882（仅归档）/ pin:1014 / pin-order:1052（整体重编号、重复 400、逐会话鉴权）/ search:857（512 上限、title/content 两阶段）/ find:922 / memory GET:1103 PATCH:1135。

## 六、新增测试（31 个，全部通过；RC-3 现役对照式）

- kernel `session_tree::tests` 9：重置点解析三形态+响亮失败、fork 深度/retained 区间、检查点覆盖/冲突/201 裁减、sha256 形状、组件级路径闸。
- adapters `session_tree_r06t03` 12：fork 复制+ID 稳定+独立写入+不扩权、reset 标记（含根重置 newHead=null）、检查点 201 窗口/删除 FK 序/latest 覆盖、管理面（改名/置顶/重排/记忆/归档三态/恢复/删除/搜索 LIKE 免疫）。
- service `r06_t03_session_tree` 10（真实 TCP+HTTP/1.1+设备凭证+scripted provider 生产链）：A05 fork 不串写、深度 409、busy 三操作 409、retry 重置+新 run 不覆盖旧 run、目标解析对照现役、A06 外部修改/删除全拒+分支不动+一致时 verified 回执、越界路径 403、检查点 CRUD 现役形状、管理面全链、跨主体 403。

## 七、现有回归测试（真实命令与退出码）

- `cargo fmt --all -- --check`：初检非 0（T03 新文件未格式化）→ `cargo fmt --all` → 复检 exit 0。
- `cargo clippy --locked --workspace --all-targets -- -D warnings`：初检 exit 101（2 个 result_large_err）→ 最小修复（两 gate 函数加 `#[allow]` 并注理由）→ 复检 exit 0 零警告。
- `cargo test --locked -p lingxi-kernel`：160/160 exit 0（lib 96 + T01 20 + T02 44）。
- `cargo test --locked -p lingxi-adapters`：全部套件 exit 0（lib 119 等，合计 287 passed）。
- `cargo test --locked -p lingxi-service`：全部套件 exit 0（lib 361 等，合计 986 passed）。
- `cargo test --locked --workspace`：124 套件、1643 passed / 0 failed、WORKSPACE_EXIT=0。
- 证据：`03_fmt.txt`/`04_clippy.txt`/`05_targeted_tests.txt`/`06_workspace_regression.txt`。

## 八、Acceptance 证据

### R06-A05：fork 不串写
`fork_copies_history_with_stable_ids_and_independent_writes`（adapters）+ `fork_copies_history_with_stable_ids_and_writes_stay_independent`（service 生产链）：fork 后源会话与分支会话各自 execute 写入，双侧分支投影互不含对方新消息；共享历史消息 ID 稳定；`lineageDepth=1`；深度第三层 409 `session_fork_depth_limit`。

### R06-A06：rewind 不覆盖外部修改
`rewind_detects_external_modification_and_refuses_to_overwrite`（adapters sha256 比对）+ `rewind_refuses_on_external_modification_and_preserves_branch`（service 生产链）：检查点后外部修改 → 409 `rewind_file_conflict`、分支头不变、用户文件原样保留；文件被删同样 409；内容一致 → 200 且 `verified` 回执 + `externalEffects:"not_rolled_back"` 如实标注；偏好未开 → 403 `file_rollback_disabled`；preview 只读判定 restore/conflict。

## 九、R00 原始断言对应结果（74 叶）

`docs/rust-tauri/R06/R06_LEAF_MAP.json` 的 T03 节已逐叶标注 `t03_status` + `t03_anchor`：**真实实现 16 / 合法份额 21 / 后续 37**。任务书预期分布 14/18/42，实际偏差方向为实做面更宽（检查点三叶与 rewind 预览达整叶实现），未为凑预期分布而降级任何一叶；偏差说明写入叶图 `t03_summary.deviation_note`。

## 十、记录在案差异

D1-D6 + 附带显式新增（skippedBusy/childCount/LIKE 免疫），逐条现役行号锚定见 `artifacts/rust-tauri/R06/T03/07_diff_ledger.md`。要点：D1 rewind 从盲写回强化为冲突全拒；D6 retry 两段式（重置+返回回合输入，客户端重发 execute）；D3 归档载体 JSONL 搬移→lifecycle 字段翻转。

## 十一、R05 接口兼容结果

既有路由形状、鉴权分类、run 生命周期全部保持：workspace 1643/1643 全绿（含 R05 及更早全部套件）；`auth.rs` 既有分类规则未改语义，仅新增 sessions 管理面条目（chat scope）；`runs.rs` 分支投影读取对齐不改既有字段形状。无破坏性变更。

## 十二、未验证事项

- 真实模型供应商端到端 fork/retry/rewind（本 Task 用 scripted TurnProviderPort；供应商集成属既有通道，未在本 Task 重复验证）。
- WS 广播（session-changed 等）未实现未验证（R07 范围，叶 #24/#34/#41 的广播子句如实归份额）。
- 桌面壳消费端（R08）未验证；authorized-folders 增删替换端点、new-detached、消息分页/ETag 后续。
- macOS 以外平台未执行（环境受限，如实申报）。
- 「已删 Agent 会话」相关子句：Rust 侧尚无 agent 删除概念，不可检验（叶 #38/#39/#43 注记）。

## 十三、已知风险

- retry 两段式（D6）依赖客户端重发 execute；若客户端不重发，分支停在新头（可恢复，非数据损坏），评审时需确认该交互形状获接受。
- cleanup 的 `skippedBusy` 为现役没有的新字段（显式上报非静默），需客户端容忍未知字段。
- 检查点文件版本仅存哈希不存内容（D1 设计前提）：恢复语义=验证未变更，不是内容回滚；与现役"盲写回"语义不同属刻意强化，评审需确认。
- messages 表重建为 v8 主键：迁移对既有库一次性重建，大库迁移耗时未实测（当前测试库规模小）。

---

## 十四、REPAIR-R1（2026-10-09；R1 独立评审 FAIL 后的同根因修复）

> 本章为追加记录：第一至十三章为初版候选原样保留（其中 §十 D1 的 rewind 语义与
> §十三「仅存哈希不存内容」风险行已被本章 D7 语义取代，原文不删、以本章为准）。
> 修复矩阵与逐项落点见 `artifacts/rust-tauri/R06/T03/09_repair_r1_fix_matrix.md`；
> 差异台账追加 D7–D10 见 `07_diff_ledger.md`。

### 14.1 评审结论与处置总览

R1 评审 VERDICT=FAIL，8 项 Finding + 1 观察项。全部修复并回归，无一项以
「条件不适用」结案：

| Finding | 级别 | 处置 |
|---|---|---|
| F-01 latest 覆盖×checkpoint_file_versions 崩 | HIGH | upsert 改单事务原子（子行随检查点替换）；HTTP 同文件二次记录 500→200 |
| F-02 retry fileRollback 子句未实现 | MEDIUM | 按现役 sessions.ts:1584-1607 全契约实现（400/403/workspace 真实恢复+报告） |
| F-03 主列表未过滤归档 | MEDIUM | list_sessions + `WHERE lifecycle='active'`；HTTP 级归档/恢复对偶断言 |
| F-04 fork 字段失真 | LOW | 复制全字段保真（model_call_id/committed_at_unix_ms 原样；run_id 例外见 D9） |
| F-05 rewind 须内容级回滚（裁决） | 裁决 | file_checkpoints 死表激活为内容存档；逐文件三档（restored/conflicted/skipped/failed）；冲突保留用户文件且分支照常回移；externalEffects 恒 not_rolled_back |
| F-06 messages 丢 run_id FK | LOW | v8 恢复 `run_id TEXT REFERENCES runs(run_id)`（可空）；用户消息 NULL，幽灵 run FK 响亮拒绝 |
| F-07 file_checkpoints 死表 | LOW | 随 F-05 激活（PK (checkpoint_id,file_path) + FK） |
| F-08 archived_children 计数虚高 | LOW | 仅真实翻转计数 |
| F-09 RED 证据退出码空缺 | LOW | 02_red_tests.txt 回填并标注（推断 101，非当次实测，如实声明） |
| OBS fork 撞库→500 | 观察项 | fork/创建撞库 → 响亮 409 session_exists（同类入口同修） |

### 14.2 修复中自查捕获的同根因扩展（评审未列出，一并修复）

1. **201 裁减×文件子行**：带文件版本的检查点触顶裁减时子行未清 → FK 失败
   （原子 upsert 重写内同修）。
2. **delete_archived_session FK 序**：file_checkpoints 激活 FK 后，purge 顺序
   「先父后子」必 500——由管理面套件实测捕获（archived delete 500→200），
   四个 checkpoints 删除入口全部复核为「子→父」。
3. **fork 副本 run_id 跨会话活引用**：副本持有源会话 runs 引用时，源会话永久
   删除被 FK 卡死。裁决：副本 run_id 置 NULL（其余字段保真；源 run 追溯经
   消息 id 承载），台账 D9 记为对「全字段保真」的唯一有意偏离。

### 14.3 退役清单（修复产生的孤立代码，随修复移除）

- `SessionTreeError::RewindConflict` 及 HTTP 409 映射臂（整批 409 语义废止）。
- kernel `FileRestoreVerdict` 两档升三档（`judge_file_restore` 签名随语义升级）。
- `record_file_version` 保留但降级为幂等补记入口（生产路径改走组合 upsert
  单事务；探针复跑仍可用）。

### 14.4 修复后验收（全部真实命令、真实退出码；证据在 `09_repair_r1_*`）

- 三套件逐名非空：kernel session_tree 9/9、adapters 18/18（含 6 新增）、
  service 13/13（重写 1 + 新增 3 + 管理面扩展），EXIT=0。
- `cargo fmt --all -- --check` EXIT=0；`cargo clippy --locked --workspace
  --all-targets -- -D warnings` EXIT=0；`cargo test --locked --workspace`
  EXIT=0：124 suites / 1652 passed / 0 failed（基线 124/1643，净增 9＝
  adapters+6、service+3；证据 `09_repair_r1_workspace_regression.txt`）。

### 14.5 语义面申报（相对初版候选的变化）

- rewind 收据形状：`verified[]` → `restored/conflicted/skipped/failed[]` +
  `externalEffects`（台账 D7）。
- retry 响应新增可选 `fileRollbackReport`（台账 D8；现役同名字段对齐）。
- 分支历史投影新增 `modelCallId`、`committedAtUnixMs`（纯增量）。
- fork 副本 `run_id=NULL`（台账 D9）。
- 检查点文件内容入库（`file_checkpoints` BLOB，本地库、授权目录闸内）——
  初版 §十三「仅存哈希」前提作废，内容面限于会话授权目录内文件。
