# R06 收口独立复审（RV2）——冻结候选 844a10b2b 上 R06-AR-F02 / R06-AR-F05 修复真实性

- Reviewer：全新独立 R06 收口 Reviewer（RV2），空历史，不复用执行/前审角色。
- 冻结候选：`844a10b2b83a98bb909a9f83bb89f140021d5adc`（worktree `/tmp/lingxi-rv2`，detached HEAD）。
- 判据：义务全满足 PASS；缺口 FAIL；仅外部条件 BLOCKED。
- 结论：**F02 = PASS（macOS 实测；Windows 腿为已登记的显式降级，见 OBS-1）**；**F05 = PASS**（相邻 F06/F07 义务保持 OPEN，不属 F05 范围，见 OBS-2）。**总判定：PASS（含两条如实观察项，无 FAIL 缺口）。**

## 1. 环境与亲验方式

| 项 | 值 |
|---|---|
| 工具链 | `/Users/study_superior/.cargo/bin/cargo` 1.98.1，`--locked`，`CARGO_TARGET_DIR=/tmp/rv2-target` |
| 合成环境 | `LINGXI_HOME=/tmp/rv2-env`、`TMPDIR=/tmp/rv2-env`（f04 套件 harness 严格 unwrap `LINGXI_HOME`） |
| 平台 | darwin 27.0.0 arm64（macOS） |
| 审计基线 | AR_ISSUE_MATRIX.json `audited_base=2e1933d8…`；候选即当前主仓 HEAD（工作区 clean） |

亲跑结果（本人执行，非执行者自检转述）：

| 套件 | 结果 |
|---|---|
| `r06_ar_f02_file_restore` | 16 passed / 0 failed |
| `r06_ar_f05_cache` | 7 passed / 0 failed |
| `r06_ar_f04_lifecycle` | 15 passed / 0 failed（需 LINGXI_HOME；首次未设环境变量时 15/15 在 0.97s 内全红，定位为 harness 前置 `std::env::var("LINGXI_HOME").unwrap()` panic，设环境后全绿——环境前置，非代码缺陷） |
| `r06_t05_filehistory` | 8 passed / 0 failed |
| `r06_t03_session_tree`（F02 关联原套件，加测） | 14 passed / 0 failed |
| `cargo clippy -p lingxi-service --all-targets -- -D warnings` | clean（exit 0） |

两处 D1 段所述修复（commit 844a10b2b diff 亲核）：
1. **oversized 观测先拒后不淘汰**：`rust/crates/lingxi-service/src/context_compiler.rs` `record_call_observation` 中 `size > CONTEXT_OBSERVATION_CACHE_BYTES` 拒绝分支从淘汰循环**之后**移到**之前**。旧序会先按 8MiB 预算清空全部已提交观测再拒绝插入（拒绝却销毁数据）；新序拒绝时不触发任何淘汰。
2. **撤销工具 stale/disabled 拒绝消息区分**：`rust/crates/lingxi-service/src/toolgateway.rs` 新增 `refuse_with_current_callability`，接入 `admission_checks` 的 prepare `map_err`。StaleCatalog 拒绝时若该引用在**当前**代次解析为不可调用（disabled/revoked），模型面收据改为 `TargetNotCallable`（"…disabled and not callable: …; the pinned catalog view is stale (held generation …, registry at …)"）；仅描述/Schema 变化仍报可恢复的 "stale catalog"；卸载/改名保留 stale-catalog。内核侧 stale-first prepare 契约（R04-A02）不变，只改模型面收据选择。

## 2. R06-AR-F02（rewind/retry 文件恢复授权与原子防冲突）——PASS

remaining_scope 五条逐条核验（源码 + 亲跑测试双重锚点）：

**① 经共享 ResourceAccess 与当前授权逐文件重验（不信检查点时授权）——满足。**
`rust/crates/lingxi-service/src/session_tree.rs:248-334` `restore_checkpoint_files` 接收当前 `Principal` + `Option<Arc<ResourceAccess>>`，在 `store::with_current_file_authorization`（与授权变更共用 DB 单写队列，`lingxi-adapters/src/storage/session_tree.rs:1651`）内逐文件执行 `authorize()`：`current_file_access`（从**当前** `authorized_folders_json` 新建 `ResourceAccess`，Write 授权且 `s.path == path`——规范化目标同一性；授权根自身为链接即整体拒绝）AND `shared_file_access`（共享实例重验）。rewind/retry 两入口（`session_tree.rs:791/913`）共用该函数；HTTP 路由传认证 Principal（`lib.rs:3888` rewind_route）。测试锚：`ar_f02_c02_current_folder_revocation`（撤权后 conflicted、文件不动，实跑绿）。

**② 安全打开/身份版本比较/同目录临时写+原子替换/竞态入契约——满足（macOS/Linux 全量实现）。**
`file_safety.rs::Snapshot::read`：canonicalize≠path 即拒（路径组件换链接）、`DirFd` O_NOFOLLOW 逐组件打开（`filetools.rs:447-475`）、`open_attachment_nofollow`（openat+O_NOFOLLOW）、8MiB 上限、读后 FileIdentity（dev/ino/mtime/size/mode，句柄自带元数据不复 stat）+stamp 复核。`FileSafety::replace`：同目录临时写→权限复制→sync→**再次 authorize + same_directory + 当前 Snapshot 三重比对**（读到写竞态）→提交前持久 journal→`rename_within` + `exchange_within`（macOS `renameatx_np(RENAME_SWAP)` / Linux `renameat2(RENAME_EXCHANGE)` 原子交换）→移出对象核对（identity+bytes）不符则回换并如实 Unknown。确定性竞态测试（非 sleep）：`ar_f02_c03_deterministic_external_content_and_identity_change` 以生产 test hook 在 `before_replace`/`commit_boundary` 屏障处由**外部 python 进程**改内容或 `os.replace` 换身份，两种边界×两种替换全绿且 `conflict_restored` 收据保留被换出的 A 字节。

**③ 检查点持久化前完整验证内容与来源；恢复区分已达/确证/外部冲突/未确认/失败；不把“旧检查点见过同字节”当外部编辑不存在证据——满足。**
检查点侧（`session_tree.rs:1130-1195`）：双侧 canonicalize 授权根+文件、`path_within_folders`、`is_protected` 拒绝、`current_file_access`+`shared_file_access` 双闸、`Snapshot::read` 安全读，content+sha256+size 同事务落库。恢复侧五档：skipped（当前即目标）/restored/conflicted/failed（reason 标明）/Unknown（`write_may_have_occurred`）。覆盖判定靠**写入见证**（`witnessed`：dev/ino/ctime/size/sha256 全等才认定当前内容是本会话系统写产物），非“检查点见过字节”——`ar_f02_c01_normal_restore_and_external_same_old_bytes_protected` 第二段：外部 python 进程写回旧检查点字节 B，rewind 判 conflicted、外部 B 保留（实跑绿）。`ar_f02_checkpoint_content_corruption_is_rejected` 验存储侧损坏拒绝。

**④ 写失败/磁盘满/取消/崩溃保留原文件或如实报告，failed≠零写入——满足。**
`replace` 把交换后任何 Failed 升格为 Unknown（`WriteError` Display 即 `write_may_have_occurred`）；恢复对象（`.lingxi-recovery-*.original`）永不自动删除，journal 状态机 `prepared/completed_original_retained/conflict_restored/unknown_original_retained/failed_before_exchange_retained` 全持久。`arm_files` 在任何写之前持久置 `file_effects=1`（`session_operations.rs:158`），崩溃遗留 prepared+file_effects 行使后续 begin 拒绝 `session_file_operation_unconfirmed`——重启不盲目重做。测试：`ar_f02_c04_failure_stage_receipts_and_original_bytes`（temp_write/permission_copy/temp_sync/before_replace/after_replace 五阶段注入）、`ar_f02_c04_process_stop_and_restart_does_not_replay_unknown_write`（真实子进程 exit 73 于 commit_boundary/after_replace/multi_after_first，断言恢复记录与字节事实）、`rewind_recovery_records_survive_request_replay_and_service_restart`。

**⑤ 全族同一安全原语——满足。**
rewind/retry→`restore_checkpoint_files`→`FileSafety::replace`；`resourceio.rs:2280-2400` 生产写路径 existent→`Snapshot::read`+`safety.replace`、新建→`safety.create`，authorize 闭包逐次经 plane 当前授权；`filetools.rs:1539` 注释“R06-AR-F02 之后生产写路径一律走 FileSafety”并有 `ar_f02_native_filetools_write_then_restore_and_session_witness_binding` 证明 filetools 写→见证→恢复闭环；`filehistory.rs:708` restore_route→`resource_io().write`→同一原语（`ar_f02_family_expected_version_and_filehistory_real_routes` + `r06_t05_filehistory` 8/8 实跑绿）。附加反伪造：`ar_f02_source_state_and_recovery_objects_cannot_be_forged_via_file_api`、`ar_f02_source_directory_link_cannot_supply_a_forged_witness`、`ar_f02_c02_checkpoint_path_symlink_must_not_change_external_sentinel`（检查点路径换指向外部哨兵的符号链接、哨兵故意放已见证字节 B——哨兵不变，实跑绿）。

## 3. R06-AR-F05（上下文缓存身份/失效/回收）——PASS

remaining_scope 四条逐条核验：

**① 静态段/动态材料按现役契约区分并逐字段记录——满足。**
`artifacts/rust-tauri/R06/AR/W05-E01/MATERIALS_AND_LIFECYCLE.md`（冻结候选内）13 行逐字段台账：每类材料的真实读取源、实际写入入口、生效与回收口径（system 材料每 Run 冻结、live tools 每 call 重建、observations 持久、cache/lifecycle 128 条 8MiB FIFO）；如实标注仍 OPEN 的相邻义务（F06 完整 YAML、F07 禁用后历史编码、头像/vision 同步），不借缓存申报完成。行为锚：`c01_actual_calls_keep_system_frozen_and_tools_live_after_revocation`（同 Run `renderSha256` 相等、`toolCatalogGeneration`/`snapshotId` 变化、wire[0]==wire[1] system 消息；改 identity.md 后下一 Run 线上出现 TOOLS_PERSONA_NEW）。

**② 明确缓存身份（主体/Agent/Session/形态/材料与配置代次）或等价运行快照索引；当前 Run 快照不中途漂移——满足（采用等价运行快照索引）。**
不再缓存编译产物（`compile_snapshot` 每次新采集；`compiled_for_run` 注释“无身份复用”）；`runs.rs:2110-2155` 从冻结 `Arc<CompiledContext>` 观测 + 真实 sessionId/agentId/sessionOwnerUserId/runId/actor/toolCatalogGeneration/toolDeclarationsSha256 合成 `snapshotId`（观测整体 sha256），modelCallId/attempt 是发送坐标不改 snapshotId；Run 全程持同一 Arc（`runs.rs:1424` “此 Arc 与 system 材料在运行内不再漂移”）。采集侧 `ContextMaterialEpoch`（writers/revision 原子计数，`agent_memory.rs:38-50`）+ 双 gather `PartialEq` 对照（kernel `ContextCompileInput` derive PartialEq）+ 8 次有限重试后响亮失败。并发锚：`c04_actual_acquisition_publish_barrier_never_sends_mixed_material`（AcquisitionBarrier 包裹生产 FileContextMaterialSource，屏障处外部改材料，发布版本一致、线上=观测）。身份隔离锚：`c02_real_runtime_other_agent_child_has_its_own_persona`、`c02_actor_owner_and_reader_remain_distinct_on_the_same_session`（真实 TCP HTTP wire）。

**③ persona/memory/Skills 修改、撤销、会话删除与生命周期接入失效/回收；有界缓存+可观察淘汰——满足。**
材料不再跨 Run 缓存，配置修改天然下一 Run 生效（c01 双探针实证）；已提交调用观测缓存 128 条/8MiB FIFO，`cache_stats` 暴露 entries/bytes/capacity/byteCapacity/evictions/reclaimed/policy（HTTP 观测端点 `includeCacheStats` 可查）；`invalidate_session` 由 SessionAdmin archive/descendants/永久删除/cleanup 多点调用（`session_admin.rs:506-1044`），重启内存缓存清零。锚：`c03_observation_cache_is_bounded_and_evictions_are_observable`（含 oversized 先拒后不淘汰断言：`after.entries==before.entries` 且 evictions 不变——旧代码此断言必败）、`c03_session_archive_and_delete_reclaim_the_context_observation_cache`（真实归档/删除/重启）。

**④ 观测端点定位实际发出的 run/snapshot，不临时重编译；并发编译只发布一个确定版本——满足。**
HTTP `GET /sessions/{id}/context-observation`（`lib.rs:5526`）→ `run_store.rs:491` `model_context_observation`：只读 durable `model_context_snapshot` key_event，且要求同 run/attempt 存在 `model_call_started`，按可选 runId/modelCallId/snapshotId 定位；读侧重校验 schema/sessionId/runId/actor.kind/actor.subject/sessionOwnerUserId 并对去坐标视图重算 sha256 与 snapshotId 比对，不符即 Corrupted；无快照 404（“observation never builds one on demand”）。c01 探针断言按真实 callId 回读首call观测==首发观测、最新值不可替代。

## 4. 观察项（不构成 FAIL）

- **OBS-1（F02，环境受限+显式降级已登记）**：Windows 腿 `fsio` 为 std 回退：最终组件重解析点拒绝（junction honesty）保留，但**打开时符号链接交换竞态未闭合**，源码 `filetools.rs:757-761` 显式登记“registered form, real-machine verification deferred to the stage's Windows leg”。本机为 macOS，Unix 全量原语（O_NOFOLLOW walk/RENAME_SWAP/RENAME_EXCHANGE）实测通过。按“显式降级并标注”红线不判 FAIL；正式 Windows 验收仍欠真实 Windows 机证据。
- **OBS-2（F05 相邻义务，保持 OPEN 如实）**：F06（完整 Agent config YAML 语义）与 F07（工具禁用后的撤权历史 provider 编码）在矩阵与台账中均保持 OPEN，本候选未宣称修复；F05-C01 执行者旧claim亦标 `PASS_LIMITED` 附 `FAIL_OPEN_F07_AND_RELATED_F05`。该缺口属 F06/F07 范围，不回溯推翻 F05 四条 remaining_scope 的满足。
- **OBS-3（流程）**：AR_ISSUE_MATRIX.json 中 F02 的 `self_check.cases` 仍标 NOT_RUN、`independent_review` 停留在平台中止轮；当前候选以 844a10b2b 内 16 项实测测试承担全部四 C + 家族反例（本审亲跑全绿）。建议后续把本审结果回写矩阵，避免台账与实测状态脱节。

## 5. 证据文件

- 测试输出：见 §1 表（本人运行，worktree /tmp/lingxi-rv2）。
- 关键源码坐标（均相对 `/tmp/lingxi-rv2/`）：
  - `rust/crates/lingxi-service/src/session_tree.rs:248-387,1130-1195`（恢复编排/当前授权/检查点验证）
  - `rust/crates/lingxi-service/src/file_safety.rs`（全文件：Snapshot/witness/replace 安全原语）
  - `rust/crates/lingxi-service/src/filetools.rs:420-760`（DirFd/FileIdentity/fsio 平台原语）、`:1539-1600`（生产写路径接入）
  - `rust/crates/lingxi-service/src/resourceio.rs:2280-2400`、`rust/crates/lingxi-service/src/filehistory.rs:708-770`（家族复用）
  - `rust/crates/lingxi-service/src/context_compiler.rs:776-988`（观测缓存/epoch 双读/先拒后不淘汰/invalidate/cache_stats）
  - `rust/crates/lingxi-service/src/toolgateway.rs:985-1026,1045`（stale/disabled 收据区分及接线）
  - `rust/crates/lingxi-service/src/runs.rs:1424,2110-2160`（每 Run 冻结/快照发布）
  - `rust/crates/lingxi-adapters/src/storage/run_store.rs:491-545`（durable 观测读面）、`session_operations.rs:158`（arm_files 前置持久）、`session_tree.rs:1651`（授权写队列闭包）
  - `rust/crates/lingxi-service/tests/r06_ar_f02_file_restore.rs`、`r06_ar_f05_cache.rs`（探针本体）
  - `artifacts/rust-tauri/R06/AR/W05-E01/MATERIALS_AND_LIFECYCLE.md`（逐字段台账）

## 6. 最终判定

| 项 | 判定 |
|---|---|
| R06-AR-F02 五条 remaining_scope | 全部满足（macOS 亲验；Windows 腿 OBS-1 为已登记外部受限项） |
| R06-AR-F05 四条 remaining_scope | 全部满足 |
| F05 两修（oversized 先拒后不淘汰；stale/disabled 消息区分+两探针） | 真实落地，亲跑探针绿 |
| **总判定** | **PASS**（无 FAIL 缺口；OBS-1/2/3 如实登记） |
