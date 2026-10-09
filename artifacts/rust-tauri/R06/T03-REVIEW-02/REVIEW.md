# R06-T03 REVIEW-02 — 第 2 轮一次性独立对抗性审查

- TASK_ID: R06-T03（会话树、重试、fork 与回退）
- 审查轮次: REVIEW-02（未参加实现、REVIEW-01、修复；独立取证）
- TASK_BASE_SHA: `4920c59e38ddf69c8b846fb1aea896fca70d939d`（分支 `codex/rust-tauri-migration`，工作区未提交候选）
- 审查日期: 2026-10-09
- 工具链纪律: 全程 `/Users/study_superior/.cargo/bin/cargo`（rustup shim → 1.98.1）；未使用任何其他 cargo；所有命令退出码真实记录于下文；测试筛选均非空。

## 〇、候选完整性（digest 复核——亲跑四轮，含一次 IDE 噪声归因）

```
$ bash scripts/rust-tauri/r06_candidate_digest.sh   # 第 1 轮：审查开始前
19f82d330edd4b3b51539742e43dcd7fd74b676af4074135fed9b67a3e9ac8e3   EXIT=0
$ bash scripts/rust-tauri/r06_candidate_digest.sh   # 第 2 轮：静态审查主体完成后
19f82d330edd4b3b51539742e43dcd7fd74b676af4074135fed9b67a3e9ac8e3   EXIT=0
$ bash scripts/rust-tauri/r06_candidate_digest.sh   # 第 3 轮：报告定稿前
1f60d1ee4cb83195396931fba38a175bd12e1fd59786272e689a01ccdf26508c   EXIT=0   ← 不一致，启动归因
$ bash /tmp/r2_digest_novs.sh                        # 第 4 轮：原脚本逐字同口径、仅追加排除 .vscode/
19f82d330edd4b3b51539742e43dcd7fd74b676af4074135fed9b67a3e9ac8e3   EXIT=0   ← 回到声称值
```

第 3 轮不一致的归因（完整证据链）：`.vscode/settings.json`（36 字节，内容
`{"git.ignoreLimitWarning": true}`）于 17:47 由本机 IDE 在本审查会话期间生成
（非候选内容、非本代理动作；会话初始 git status 快照中无此条目）。该文件落在
脚本口径内（未跟踪文件、不在排除前缀）。第 4 轮用与原脚本逐字相同的管道、仅追加
`grep -v '^\.vscode/'` 一行，结果精确回到声称值——两轮之间的唯一差异即该 IDE 文件，
**候选本体（全部 T03 源码/文档/测试）与执行者 digest 时刻逐字节一致，无候选漂移**。
（变体脚本在 /tmp，未触碰仓库；本审查的全部产物位于脚本排除前缀
`artifacts/rust-tauri/R06/` 之内，`rust/target/` 为 gitignore。）

## 一、任务完整性（Goal / Steps / Deliverables / R06-A05 / R06-A06 / 74 叶）

任务书（`Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R06_上下文、会话语义、记忆与知识库.md` T03 节）逐条对照：

| 任务书要求 | 核验结果 |
|---|---|
| Goal「保留现役可见功能而非只保留消息列表」 | 成立。fork/retry/rewind/具名检查点/管理面（rename/pin/pin-order/archive 三 childMode/restore/archived-delete/cleanup/search/memory）+ 分支投影均有真实 HTTP→service→adapters→kernel 链（见第二节）。 |
| Step 1 迁移实际入口 | 成立。组合根 `lib.rs:1345-1347` 无条件接线 `DbUserMessageRecorder`（非测试缝）；`runs.rs:1105-1114` USER-origin run 输入在 run 行持久提交后落消息树，失败响亮（DriveError，绝不假装已落）。 |
| Step 2 稳定 ID 不扩权 | 成立。fork 消息 ID 原样保留（`fork_copies_shared_history_with_stable_ids_*` 实测）；`fork_does_not_widen_authorizations` 实测权限/授权目录逐字段不扩。 |
| Step 3 重试区分新 Run 与 attempt、不覆盖已审计旧结果 | 成立。D6 两段式：retry 只重置并返回回合输入，客户端重发 execute 起**新 run**；`retry_resets_branch_and_new_run_never_overwrites_old` 实测旧 run 保持 completed。 |
| Step 4 回退检查 checkpoint、真实文件版本、冲突拒绝、外部副作用不假装回滚 | 成立（REPAIR-R1 后语义 D7）：逐文件三档（restored/conflicted/skipped/failed），冲突保留用户文件，收据恒带 `externalEffects:"not_rolled_back"`。 |
| Deliverable: SessionService | `lingxi-service/src/session_tree.rs`（885 行）+ `session_admin.rs`，真实编排非空壳。 |
| Deliverable: 树/分支索引 | schema v8：sessions +11 列、session_branch_heads、checkpoints（含 turn_input_message_id 锚点列）、checkpoint_file_versions、file_checkpoints（死表激活）、messages 重建 PK (session_id,message_id) + 可空 run_id 保 FK。 |
| Deliverable: 回退检查 | `restore_checkpoint_files`（session_tree.rs:227-306）逐文件 sha256 + 见证集判定 + 写回复读校验。 |
| R06-A05（fork 不串写） | REQUIRED→成立：adapters + service 双层实测 fork 后双侧写入互不可见、共享历史 ID 稳定、深度第三层 409。 |
| R06-A06（rewind 不覆盖外部修改） | REQUIRED→成立：外部修改/被删 → conflicted 保留原文件；见证版本 → restored 真实字节；偏好未开 → 403。service 生产链实测。 |
| 74 叶归类 | 16 implemented / 21 share / 37 deferred。逐叶对照见第六节；发现 2 处锚点过时（R2-F-02），未发现虚报整叶。 |

## 二、生产路径真实性（全链亲验）

HTTP → `auth.rs classify_route`（全部新端点 chat scope，逐匹配臂亲验 607-634 行；未知形状落 LocalOnly fail-closed）→ 路由（`lib.rs`，deny_unknown_fields 请求体）→ 闸（`gate_session_tree_write` 归属+生命周期 active→409 session_not_active；`gate_session_owned` 仅归属用于管理面与记忆面——restore/archived-delete 本就以归档态为对象，闸形正确；pin-order 对列表逐会话鉴权，无 IDOR）→ supervisor busy 租约 409 → service → adapters（`with_write_txn` 真实 SQLite 事务，foreign_keys=ON 于 queue.rs:592）→ kernel 纯逻辑。

关键路径逐条亲验：

1. **retry fileRollback 全契约**（R1 F-02）：路由层 `lib.rs:3172-3184` 非法值→400 invalid_file_rollback、缺省/none→None、workspace→Workspace，绝不静默降级；service `retry_turn`（session_tree.rs:422-535）busy→409 在先、workspace+偏好关→403、恢复先于重置（对照现役 session-turn-actions.ts:451-460 已亲读原文）、无锚点检查点→no_checkpoint 报告且不阻塞重置（对照现役 restoreTurn no_checkpoint）。与现役 `sessions.ts:1586-1596` 原文逐条比对一致（本轮重新亲读：400 code invalid_file_rollback、403 code file_rollback_disabled、默认 none）。
2. **rewind 内容级回滚三档**（R1 F-05 裁决语义 D7）：`restore_checkpoint_files`——当前==目标→skipped；当前∈见证集（该会话全部检查点记录过的 (path,sha) 集合）→restored（写回存档字节+复读校验，写坏/读坏如实入 failed 不谎称 restored）；其余（含被删）→conflicted 保留用户文件；内容行缺席（历史检查点）→conflicted 不覆盖。冲突不阻塞分支回移，收据四档+external_effects 恒 not_rolled_back。preview 同口径只读、不发布事件。
3. **file_checkpoints 真实读写**（R1 F-07）：V8 激活 PK (checkpoint_id,file_path)+FK→checkpoints；create_checkpoint 双侧 canonicalize+授权目录闸 403+读字节（utf-8/base64 形态标记，BLOB 恒存原始字节）随 upsert **同一写事务**落库；恢复从该表读字节写回。
4. **rewind 目标语义无 off-by-one**：现役 `session-turn-actions.ts:180-188` 注释原文「checkpoint.target 记录存档时的最新用户输入；rewind 把会话截断到该轮输入的信封之前」——候选复用 `resolve_retry_reset_point`（kernel:162-192，newHead=回合输入消息的 parent）与现役逐字对齐。
5. **fork**：root→boundary（含）复制、ID 稳定、seq 重排、parent 重链、深度上限 2（kernel check_fork_depth + 事务内重查）、撞库预检 Conflict("session_exists:")→409、副本 run_id=NULL（D9 有意偏离，其余字段全保真——`fork_preserves_message_fields_verbatim` 逐字段断言）。
6. **检查点**：latest 覆盖/具名 409 checkpoint_conflict/上限 200 裁最老非 latest；裁减与覆盖均先清两套子行（FK 序）；delete_checkpoint 先子后父。
7. **管理面**：archive 三 childMode（F-08 修复：仅真实翻转计数，`UPDATE ... AND lifecycle='active'` 影响行数>0 才计）；restore WrongLifecycle 三态响亮；archived-delete 14 步 FK 序 purge（代码顺序正确：file_checkpoints 528 行先于 checkpoints 530 行）；cleanup 严格 `< cutoff` + skippedBusy 显式上报；search 两阶段均 lifecycle='active' + LIKE ESCAPE；主列表 list_sessions `WHERE lifecycle='active'`（F-03，run_store.rs:416 亲验）；归档列表单独走 lifecycle='archived'（session_admin.rs:589）。
8. **final 消息 parent 链接**：run_store.rs final 消息 parent=分支头（fallback max seq）+同事务推进头，分支投影不断链。
9. **事件契约**：reset 标记与头移动同事务；key_event 提交后 publish_committed。

无平行构造者：全部能力经唯一 HTTP 生产链；`record_file_version` 保留为幂等补记入口（仅 2 个测试调用，生产零调用，报告 §14.3 如实声明保留——但见 R2-F-03 的文档不一致）。

## 三、对抗性验证

### 3.1 R1 findings 逐项复现修复真实性（九项 + 一观察）

| R1 项 | 修复核验（本轮亲验） |
|---|---|
| F-01 HIGH latest 覆盖×文件版本 500+残留 | 修复真实。upsert 单事务原子（Overwrite 先删两套子行再 UPDATE 再插新行；裁减先清子行再删行）。亲跑 `latest_overwrite_with_file_paths_replaces_versions_and_contents_atomically`、`checkpoint_eviction_with_file_versions_is_fk_safe`、`checkpoint_latest_overwrite_with_file_paths_replaces_atomically`（HTTP 级）全绿。 |
| F-02 retry fileRollback 缺席 | 修复真实（第二节 1）。亲跑 `retry_file_rollback_contract_matches_incumbent` 绿：400/403/workspace 真实恢复+报告/no_checkpoint 不阻塞全链路。 |
| F-03 主列表不过滤归档 | 修复真实（run_store.rs:416；search 两阶段 session_admin.rs:703/736）。service 管理面套件含归档后主列表不含、restore 后复现的 HTTP 级对偶断言，亲跑绿。 |
| F-04 fork 字段失真 | 修复真实。fork INSERT...SELECT 全字段保真（model_call_id/committed_at_unix_ms/entry_type 原样），唯一偏离 run_id=NULL（D9 申报，理由成立：保 FK 下源会话可永久删除；现役按会话分文件存储无此耦合）。`fork_preserves_message_fields_verbatim` 亲跑绿（副本 run_id 为空、其余逐等）。 |
| F-05 rewind 须内容级回滚（裁决） | 修复真实（第二节 2/3）。`rewind_restores_content_with_per_file_verdicts_and_branch_rewinds` 亲跑绿：外部改→conflicted+保留+分支回移；被删→conflicted 不重建；见证版本→restored 真实字节；未改→skipped；preview 同口径。 |
| F-06 丢 run_id FK | 修复真实。V8_SQL `run_id TEXT REFERENCES runs(run_id)` 可空（D10）；`ghost_run_id_is_rejected_by_foreign_key` 亲跑绿；用户消息 NULL 经 `user:{run_id}` 消息 id 保持归属追溯。 |
| F-07 file_checkpoints 死表 | 修复真实（第二节 3）。 |
| F-08 计数虚增 | 修复真实。`archive_children_counts_only_actual_flips` 亲跑绿（混合子树计数=真实翻转数）。 |
| F-09 RED 退出码空缺 | 修复真实。`02_red_tests.txt` 已回填并如实标注「推断 101，非当次实测」——不冒充历史实测，声明方式合规。 |
| OBS 撞库 500 | 修复真实。fork/create 两入口同修（事务内预检 Conflict→409 session_exists）；`duplicate_session_id_is_loud_*` adapters+service 双层亲跑绿。 |

修复者自查追加项（delete_archived_session FK 序 500→200、fork 副本 run_id 活引用→D9）亦亲验为真：管理面套件（含 fork 后永久删除源会话→200、archived delete→200）亲跑绿。

### 3.2 v7→v8 迁移保真重测（修复后新 SQL，亲自复跑）

R1 的 probe2 针对修复前 V8_SQL；修复改了 V8_SQL（恢复 FK），故在本目录新建 `probe2-v8resql/`（源码原样复制自 T03-REVIEW-01/probe2，仅文件头注释标明复跑；未改动 T03-REVIEW-01 任何文件）亲跑：

```
MIGRATED_VERSION=8
MESSAGE_ROWS=2
ROW=m_old_1|sess_old|run_old|user|{"text":"hello"}|NULL|111|1|NULL|NULL|message
ROW=m_old_2|sess_old|run_old|assistant|{"text":"world"}|mc-9|222|2|NULL|NULL|message
LIFECYCLE_DEFAULT=active FOLDERS_DEFAULT=[]
REBUILT_INDEXES=2
PROBE2_OK
PROBE2_EXIT=0
```

messages 两行逐字节保真（run_id=run_old 在新 FK 下存活、model_call_id=mc-9 保留、parent/branch NULL、entry_type=message 默认值正确）、sessions 新列默认值正确、索引重建。另核 `git diff base -- migrations.rs`：v1-v7 SQL 零改动（append-only 纪律成立），v8 为纯新增。

### 3.3 修复引入的新风险审查

1. **写回路径安全**：写回目标是检查点落库时的 canonical 路径；恢复判定先读当前内容比对见证集，conflicted 绝不写——外部修改（含符号链接替换后内容未见证的情形）不会被覆盖。授权目录闸在检查点创建侧；恢复侧的安全性由「只写见证过的内容」承载。边界情形（本地用户自己把检查点文件换成指向他处的符号链接且目标内容恰被见证）要求调用方本机已有写权限，无权限放大——记录为 OBS-R2-02。
2. **竞争**：DB 侧单写者队列串行；但 fork/retry/rewind 之间无会话级互斥——现役有 `session-operation-lock.ts`（fork/retry/rewind 任一并发第二者响亮 busy），候选只有 run 在飞 busy 闸。定级 R2-F-01（MEDIUM），详见 findings。
3. **冲突不阻塞语义**：与现役 restoreTurn「逐文件失败不阻塞」同构；收据如实。成立。
4. **no_checkpoint 与现役一致性**：现役 restoreTurn 无检查点返回 `{ok:false, reason:"no_checkpoint"}` 不阻塞提交；候选同（`FileRestoreReport::no_checkpoint()`，分支照常重置）。成立。
5. **D7 对任务书符合性**：任务书 Step 4「冲突时拒绝覆盖」「不假装回滚外部副作用」——D7 的 conflicted 档保留用户文件（拒绝覆盖）+ externalEffects 恒 not_rolled_back（不假装）。符合且较 D1 整批 409 更贴近现役 restoreTurn 形状。成立。
6. **201 裁减 FK**：裁减分支先删两套子行再删检查点行；`checkpoint_eviction_with_file_versions_is_fk_safe` 亲跑绿。
7. **四种删除路径**（upsert Overwrite / 裁减 / delete_checkpoint / delete_archived_session）全部「子→父」亲验代码与测试。
8. **防测试假绿**：service 13 测试为真实 TCP+HTTP/1.1+设备凭证+scripted provider 生产链（非 mock 路由）；adapters 18 测试走真实 SQLite 文件库；kernel 9 纯函数。三套件筛选非空（kernel 9 passed/87 filtered、adapters 18/0、service 13/0），退出码全 0。

### 3.4 常规对抗面

- 路径越界：`path_within_folders` 组件级判定+拒 `..`（`/work` 不吞 `/work2`）；canonicalize 双侧防符号链接逃逸；`checkpoint_file_paths_outside_authorized_folders_are_403` 亲跑绿。
- 跨主体：`cross_principal_tree_access_is_403` 亲跑绿。
- 认证分类：新端点全 chat scope；未知形状 fail-closed LocalOnly。
- 请求体：deny_unknown_fields+显式枚举解析，非法值响亮 400，无静默降级。
- fork 深度：第三层 409 session_fork_depth_limit（kernel+adapters+service 三层防御，亲跑绿）。

## 四、R05 及更早回归

1. **全量 workspace 复跑（亲自，完整输出）**：

```
$ /Users/study_superior/.cargo/bin/cargo test --locked --workspace
test result 行数（套件数）= 124
PASSED_SUM = 1652（精确求和：grep "^test result: ok. N passed" 逐行取 N）
FAILED 套件 = 0；含非零 failed 的结果行 = 0
WORKSPACE_FULL_EXIT = 0
```

与声称（124 套件 / 1652 通过 / 0 失败）逐项一致。另有一次较早的同等复跑（tail 截断日志）亦 EXIT=0。两次独立全量运行均零失败，无 flaky 迹象。

2. **R05 七关键套件抽查（亲自，筛选非空）**：

| 套件 | 结果 | EXIT |
|---|---|---|
| r05_t01_model_plane | 24 passed / 0 failed | 0 |
| r05_t02_credentials | 38 passed / 0 failed | 0 |
| r05_t03_protocol_adapters | 12 passed / 0 failed | 0 |
| r05_t07_rr1_usage_ledger | 15 passed / 0 failed | 0 |
| run_lifecycle | 12 passed / 0 failed | 0 |
| event_subscription | 12 passed / 0 failed | 0 |
| session_serialization | 5 passed / 0 failed | 0 |

3. **messages 表改动对 R03/R05 的影响**：v8 重建 INSERT...SELECT 保全列；run_store 既有读取路径（runs.rs 分支投影对齐为纯增量字段 modelCallId/committedAtUnixMs）；上述 run_lifecycle/event_subscription/session_serialization 全绿即实证。

## 五、防虚假完成（逐叶核验）

16 个 implemented 叶逐叶对照生产链+测试（锚点已逐一亲验）：archived-delete、cleanup、restore、rewind/preview、checkpoints GET、checkpoints/delete、rewind、archive×2 childMode、memory GET/PATCH、pin、pin-order、rename、search、turns/retry——全部有真实生产链与正负测试（本报告二/三节），未发现「声称整叶实则空壳」。

但发现 2 处叶图锚点过时（REPAIR-R1 语义变更后未同步叶图）：见 R2-F-02。属文档准确性问题，不构成整叶虚报（实现与测试真实存在且为修复后语义）。

21 share / 37 deferred 抽查无升级伪装：fork 叶因 WS 广播子句归 share（申报一致）；归档列表叶因「截短首消息」展示形状归 share；authorized-folders 四叶归 share（仅创建期校验+不扩权实测，增删端点如实后续）。t03_summary.deviation_note 如实申报 16/21/37 对预期 14/18/42 的偏差方向。

## 六、Findings（固定格式）

### R2-F-01

- FINDING_ID: R2-F-01
- SEVERITY: MEDIUM
- REQUIREMENT_ID: R06-T03 Goal（保留现役可见功能/语义）；现役锚点 `core/session-operation-lock.ts:22-38`
- FILE_AND_LINE: `rust/crates/lingxi-service/src/session_tree.rs:422-435`（retry_turn 仅查 busy）、`:550-556`（rewind 同形）、`rust/crates/lingxi-adapters/src/storage/session_tree.rs:346-359`（fork 链读取在写事务之外）；对照现役 `core/session-turn-actions.ts:201`（rewind 取锁）、`:379`（retry 取锁）、`core/session-coordinator.ts:3491`（fork 取锁）
- OBSERVED: 候选对同会话的 fork/retry/rewind 之间**无任何互斥**——唯一并发防护是 supervisor 的 run 在飞 busy 闸（`is_busy` 只反映 run）。同会话两个并发 rewind（无 run 在飞）双双通过闸：文件恢复并发交错（`std::fs::write` 非原子），分支头由写队列串行、后到者赢，收据可与最终落盘内容不一致；retry×retry 产生两个共享同 parent 的新回合（投影只跟随其中一链，另一回合的 run 产物成离枝孤儿）；fork×rewind 交错时 fork 复制的是读时刻的链快照（读在写事务外，TOCTOU），该快照在 rewind 落地后已非当前分支（内容仍是合法历史，有界）；rewind×execute 准入交错（窗口=文件恢复时长）可致 run 的 user 消息落在旧头（离枝）而 final 消息落在新头（投影上 user 输入「消失」）。
- EXPECTED: 现役 `acquireSessionOperation` 使同会话 fork/retry/rewind 任一并发第二者**响亮 busy 拒绝**（session-operation-lock.ts:26 `throw sessionOperationBusyError`）——树变更操作两两互斥是现役可见契约。
- REPRODUCTION: 同会话无 run 在飞时并发 `POST /sessions/{id}/rewind`（两个不同检查点、restoreFiles=true、偏好已开）×2：两请求均 200，文件落盘可能为两检查点字节混合，分支头=后到者；对照现役第二请求必得 session_busy。
- ROOT_CAUSE: 候选把「会话忙」仅建模为 run 在飞租约，未建模现役的会话级操作互斥锁。
- SAME_ROOT_CAUSE_PATHS: retry_turn / rewind_to_checkpoint / preview 外的全部树写面（fork）共用同一缺失；execute 准入与 rewind/retry 的交错窗口同根因。
- IMPACT: 有界——外部修改不会被误覆盖（见证集判定在竞争下仍成立：conflicted 从不写），无用户数据灭失；但可出现混合恢复结果、离枝孤儿回合、收据与终态不一致、fork 复制过期链。单用户桌面场景触发概率低，但 HTTP API 与 R08 客户端均可真实触发；属现役可见契约的行为分歧。
- REQUIRED_FIX: 为 SessionService 加会话级操作互斥（如 in-flight 操作集合或 per-session async mutex），fork/retry/rewind 任一并发第二者响亮 409（reason 对齐现役 session_busy 语义）；或在差异台账显式申报该分歧并附影响分析。
- REGRESSION_TESTS: 并发 rewind×rewind（一者 409）、并发 fork×retry（一者 409）、rewind×execute 准入交错（rewind 409 或 execute 排队）——至少覆盖前两者。

### R2-F-02

- FINDING_ID: R2-F-02
- SEVERITY: LOW
- REQUIREMENT_ID: R06-T03 叶图准确性（防虚假完成面的申报真实性）
- FILE_AND_LINE: `docs/rust-tauri/R06/R06_LEAF_MAP.json` 叶 `F-D02-SEMANTIC_EFFECT-SEMANTIC-EFFECT-CHECKPOINTS-CHECKPOINTS-ID-RESTO-3562B5`（rewind implemented 叶）锚点「A06强化：sha256逐文件比对，分歧含文件被删→409全拒、分支不动。测试：rewind_refuses_external_modification_*」；叶 `F-D02-ROUTE_BEHAVIOR-...-ROLLBACK-PREVIE-477DA3`（preview implemented 叶）锚点引「rewind_refuses_*」
- OBSERVED: rewind 叶锚点仍写修复前 D1 语义（整批 409、分支不动），且引用的测试名 `rewind_refuses_external_modification_*` 已随 REPAIR-R1 重写为 `rewind_restores_content_with_per_file_verdicts_and_branch_rewinds`（旧名在当前代码库零命中，本轮 grep 实证）；preview 叶锚点同引旧测试名。
- EXPECTED: 叶图作为审计面文件应与修复后语义（D7 逐文件三档、冲突不阻塞分支回移）和现存测试名一致。
- REPRODUCTION: `grep -c "rewind_refuses_external" rust/crates/lingxi-service/tests/r06_t03_session_tree.rs` → 0；叶图上述两叶锚点仍引该名。
- ROOT_CAUSE: REPAIR-R1 更新了叶 #55（retry fileRollback）但漏同步 rewind/preview 两叶的锚点。
- SAME_ROOT_CAUSE_PATHS: 本轮扫描全部 74 叶锚点，仅此 2 叶命中过时模式；share 叶 `F-D02-TOOL-TOOL-REWIND-E1625F` 的「外部改动保留A06/忙409」表述在 D7 下仍成立，不在此列。
- IMPACT: 实现与测试真实且为修复后语义（不构成整叶虚报）；但叶图作为审计索引误导后续审查者对 rewind 语义与回归锚点的定位。
- REQUIRED_FIX: 同步两叶锚点至 D7 语义与现存测试名。
- REGRESSION_TESTS: 无（文档修正）；以 grep 旧测试名零命中为验收。

### R2-F-03

- FINDING_ID: R2-F-03
- SEVERITY: LOW
- REQUIREMENT_ID: R06-T03 证据一致性（修复矩阵 vs 报告 vs 代码）
- FILE_AND_LINE: `artifacts/rust-tauri/R06/T03/09_repair_r1_fix_matrix.md` F-01 行「独立 `record_file_version` 入口退役（调用方归零，随改动移除）」；对照 `docs/rust-tauri/R06/R06-T03_REPORT.md` §14.3「`record_file_version` 保留但降级为幂等补记入口」；代码 `rust/crates/lingxi-adapters/src/storage/session_tree.rs:844`
- OBSERVED: 代码中 `record_file_version` 保留（pub，幂等 ON CONFLICT 版），仅 2 个测试调用（adapters 测试 :455/:763）、生产零调用。修复矩阵称「随改动移除」，报告 §14.3 称「保留」——矩阵与代码/报告矛盾。
- EXPECTED: 证据文件对同一下线的处置表述一致且与代码一致。
- REPRODUCTION: `grep -rn "record_file_version" rust/crates` → 定义 1 处+测试调用 2 处，无生产调用。
- ROOT_CAUSE: 修复矩阵定稿时按「移除」落笔，实际实现选择了「保留幂等版」，矩阵未回填。
- SAME_ROOT_CAUSE_PATHS: 报告 §14.3 与代码一致；仅矩阵 F-01 行失实。
- IMPACT: 纯文档失实，无行为影响；保留的 pub 函数有测试调用不属死代码，报告 §14.3 的声明真实。
- REQUIRED_FIX: 修正矩阵 F-01 行表述为「保留为幂等补记入口，生产零调用」。
- REGRESSION_TESTS: 无。

### R2-F-04

- FINDING_ID: R2-F-04
- SEVERITY: LOW
- REQUIREMENT_ID: R06-T03 代码注释准确性（安全关键顺序的文档）
- FILE_AND_LINE: `rust/crates/lingxi-adapters/src/storage/session_admin.rs:470-484`（delete_archived_session 头注 FK 依赖序）
- OBSERVED: 头注列「2. checkpoints ← sessions；3. file_checkpoints（session_id 无 FK，按会话清）」——把 file_checkpoints 描述为无 FK 且排在 checkpoints 之后。修复后 schema 该表有 FK→checkpoints，代码删除顺序正确（:528 file_checkpoints 先于 :530 checkpoints，且 :526-527 内联注释正确），仅头注过时。
- EXPECTED: 安全关键的 FK 顺序头注与代码一致。
- REPRODUCTION: 读 :470-484 对照 :521-530。
- ROOT_CAUSE: F-07 扩展②修复改了顺序与内联注释，未回头更新头注。
- SAME_ROOT_CAUSE_PATHS: 同文件 :19 行注「（foreign_keys=ON，queue.rs:592），顺序在本文件 delete_archived_session」表述正确；仅头注编号列表失实。
- IMPACT: 代码正确；误导后续维护者按头注添加新表时排错顺序（FK 失败 500 的再引入风险）。
- REQUIRED_FIX: 更新头注（file_checkpoints 移入「checks 点之前」并标注 FK）。
- REGRESSION_TESTS: 无（现有管理面套件已锁行为）。

## 七、Observations（不阻断，如实记录）

- OBS-R2-01: 恢复写回用 `std::fs::write` 非原子（无 temp+rename）。崩溃中写可留部分文件，复读校验会如实报 failed（不谎称 restored）；现役 workspace-snapshots 同为盲写非原子，属对等。建议后续硬化。
- OBS-R2-02: 见证集语义边界——外部修改一旦被后续检查点见证即成为可写回对象（D7 申报语义；该当前态经更新检查点可恢复，无不可逆灭失）。另：恢复目标为创建期 canonical 路径，本机用户自行符号链接替换的情形不构成权限放大（调用方本已持有写权限）。
- OBS-R2-03: retry 请求体 deny_unknown_fields 且字段名 `targetMessageId` 与现役 `target` 不同；现役额外字段（clientMessageId/snapshotVersion/replacementText/displayMessage/uiContext）在候选经 D6 两段式由客户端重发 execute 承载，其中「按版本与可选替换文字」子句叶 4DD3F7 如实归 share。 loud 400 不静默，R08 客户端适配时对齐。
- OBS-R2-04: v8 迁移在 foreign_keys=ON 下重建 messages——若历史库存在孤儿 run_id 会迁移响亮失败（库打不开）。实际上不可能：Rust 侧 store 自 v1 起 FK 恒 ON 且 run_id NOT NULL，现役 TS 数据不走此库。记录为边界认知。
- OBS-R2-05: rewind 收据的 restored 档以「写回+复读校验通过」为凭，竞争交错下可能描述的是自身写回瞬间的状态（见 R2-F-01 影响面）。

## 八、裁决

**VERDICT: PASS（带 1 项 MEDIUM + 3 项 LOW 后续项）**

依据：
- 候选 digest 两次亲跑一致，无漂移。
- 全部 REQUIRED 验收（R06-A05、R06-A06）经生产链实测成立。
- R1 全部 9 项 finding + 1 观察项修复真实（代码级核验 + 新增回归测试本轮亲自复跑全绿：kernel 9/9、adapters 18/18、service 13/13，筛选非空，退出码全 0）。
- v7→v8 迁移保真用修复后新 SQL 亲自重测通过（PROBE2_EXIT=0）。
- workspace 全量回归亲自复跑 124 套件 / 1652 通过 / 0 失败，EXIT=0，与声称逐项一致；R05 七关键套件抽查全绿。
- 74 叶逐叶核验无整叶虚报。

后续项（不阻断本轮验收，但须在 R06 收口前处置）：
- R2-F-01（MEDIUM）：会话级操作互斥缺失——修复或在差异台账显式申报，二选一。
- R2-F-02 / R2-F-03 / R2-F-04（LOW×3）：叶图两叶锚点同步、修复矩阵 record_file_version 表述修正、delete_archived_session 头注 FK 序更新。
