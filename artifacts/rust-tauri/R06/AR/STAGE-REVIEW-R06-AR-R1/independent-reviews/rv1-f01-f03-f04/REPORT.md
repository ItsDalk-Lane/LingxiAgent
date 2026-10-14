# R06 收口独立审查报告（R1）——冻结候选 844a10b2b 三项 AR 发现修复复核

- 审查者：全新独立 Reviewer（无既往历史，不复用执行者角色）
- 审查对象：`844a10b2b83a98bb909a9f83bb89f140021d5adc`（"fix(rust-tauri): R06 closeout candidate — backend gap closure, AR replays, integration fixes"）
- 副本：`git worktree add /tmp/lingxi-rv1 844a10b2b`，副本内 HEAD 与目标提交逐位一致，`git status --porcelain` 为空（冻结完整）。
- 工具链：`/Users/study_superior/.cargo/bin/cargo`（cargo 1.98.1 / rustc 1.98.1，仓库锁约定），`CARGO_TARGET_DIR=/tmp/rv1-target` 独立 target。
- 判据：全部当前必需义务（AR_ISSUE_MATRIX 各 finding 的 remaining_scope 逐条 + 对应锚点测试亲自跑绿）有效满足才 PASS。

## 亲跑锚点（合成环境 /tmp/rv1-env：LINGXI_HOME=/tmp/rv1-env/home，TMPDIR=/tmp/rv1-env/tmp，LINGXI_W03_EVIDENCE=/tmp/rv1-env/w03-evidence）

命令：`cargo test --manifest-path /tmp/lingxi-rv1/rust/Cargo.toml --locked -p lingxi-service --test r06_ar_f01_history --test r06_ar_f03_operations --test r06_ar_f04_lifecycle --test r06_original_session_proofs -- --test-threads=4`

| 套件 | 结果 |
| --- | --- |
| r06_ar_f01_history | **8 passed / 0 failed / 0 ignored**（c01–c04、families、uncertain、child 裁剪、archive/reset 竞态） |
| r06_ar_f03_operations | **21 passed / 0 failed / 0 ignored** |
| r06_ar_f04_lifecycle | **15 passed / 0 failed / 0 ignored** |
| r06_original_session_proofs | **18 passed / 0 failed / 0 ignored** |

合计 62/62 全绿，exit 0。W03 证据目录实际落盘（`/tmp/rv1-env/w03-evidence/` 含 21 项 JSON+SQL 快照），证明 F03 套件在真实环境变量下执行而非静默跳过 save()。f01 的 c01–c04 走 `tests/fixtures/r06_ar_f01_probe.py` 真实二进制（`CARGO_BIN_EXE_lingxi-service`）+ 认证 HTTP + 线上请求捕获 + 直查 SQLite，非替身。f04 走真实 TestServer 正式路由。

与执行者重放报告（AR_HANDOFF 2026-10-14 R1 段：f01 8/8、f03 21/21、f04 15/15、proofs 18/18）逐项数字一致；本报告判定基于本人亲跑与源码核对，不以执行者自检为结论。

## R06-AR-F01 —— 判定 PASS

remaining_scope 五条逐一对照（源码均在 /tmp/lingxi-rv1）：

1. **统一运行上下文历史读取入口 / 输入落库时点防重复** — `runs.rs` drive_run：`model_history.load(principal, agent_id, session_id, run_id)`（model_history.rs:138-205）按主体/Agent/Session 经 `gate()`（归属+Agent 校验）读取；`list_branch_messages`（adapters session_tree.rs:889）沿 `branch_id_chain` 只读当前有效分支 root→head。用户输入仅在 `RunOrigin::User` 且 `record_run_started` 持久提交后经 `record_user_message` 落库（runs.rs:1347-1381，失败即真实 Failed 终态）；恢复侧 `branch_run_inputs` 集合 + `ExchangeItem::CurrentSubmission` 单次标记防止重复加入（runs.rs:1548-1563）。
2. **协议角色/工具配对/调用身份/opaque 签名与来源绑定** — `validate_pairs`（model_history.rs:1210）强制工具请求/结果完整配对（缺失即 `incomplete_tool_exchange` 拒绝）；`projected_tool_item`（:1220）从 F08 投影恢复，绝不读 raw 卡为模型输入；opaque 块必须有 origin（provider/model）并对照 `model_call_usage` 真实账目核验（:536-565，protocol 不符即拒）；助手/工具宿主身份绑定源 Run 前缀（:572-582）；损坏档案响亮拒绝且原件保留（`legacy_tool_archive_unavailable` 等，亲跑 c03 及 deep-fork/cross-session/forged-argument 控制组覆盖）。
3. **fork/retry/rewind 有效分支 / 跨供应商 opaque 不重放** — `load_at_boundary` 边界截断 + fork 谱系 owner/agent 一致性核验（:160-205）；分支变更在 record 侧以 branch head 比对拦截（"branch head changed before archive commit"、"current Run branch changed by reset/non-child writer"，:795-875）；跨供应商：`TurnOrigin::authorizes(provider, model)` 精确匹配才允许携带 opaque（kernel model_exchange.rs:287-306），anthropic/google/openai_responses 渲染侧强制（各 254-278 行），无 origin 的同族 opaque 降级为普通 user 消息不静默转发；c03 实测换供应商在 wire 前显式失败。
4. **压缩/摘要/保留后缀接入同一恢复路径；原件保留；重启只读** — Compacted/HardTruncated 均 `archive_projection` 后 `record(..., replace=true)`（runs.rs:1894-1938），原始权威记录 append-only 保留；摘要失败且跨 Run 已达预算 → 真实 Failed（`restored_history_context_budget*`），不超窗发送；重启加载仅 `recover_abandoned`（恢复未知收据终态，model_history.rs:85-136，带 digest 绑定校验）+ 只读 load，无模型/工具重执行；c04 实测 10 轮真实摘要触发、重启后摘要+保留后缀恢复、原件逐字节不变。
5. **foreground/background/child 调用链；子代理只继承授权裁剪材料** — `RunGrant::Subagent` 不加载父 model_history，仅 `subagents.load_history`（runs.rs:1543-1559）继承编译器按权限裁剪的材料；`f01_child_inherits_authorized_material_only` 实测共享用户材料可继承、父会话内容与其它 Agent 私有材料零出现在 wire；background 受理以 durable RunStart 为准（runs.rs:1320-1339 started_notice 在真实提交后确认，对应 W01-E01 义务项，实测 `unknown_file_effect_survives_storage_failure_restart_and_replay` 等绿）。

矩阵 rounds 状态：round 1 为平台中断（INTERRUPTED_PLATFORM_BLOCKED），无已完成的独立判决——本轮为首个完成判决。

## R06-AR-F03 —— 判定 PASS

remaining_scope 四条逐一对照：

1. **统一会话变更准入** — `SessionAdmission`（kernel session_admission.rs）为唯一闸；fork/retry/rewind/checkpoint 创建与删除（service session_tree.rs:499/1107/1279）、execute（runs.rs:1158-1241：同库 gate `try_acquire` 或经 `retain_authorized_run_permit` 继承已发许可并复核 `session_admission_changed_or_unconfirmed`）、archive/delete/create（session_admin.rs:218 acquire 同一 gate）。亲跑 `same_database_constructor_and_instance_contract`、`public_direct_drive_obeys_actual_same_database_admission`、`public_borrowed_submission_rejects_mixed_real_database_before_effects` 绿。
2. **先得权后读态、持锁至收据；非全局网络锁；多会话锁序** — gate 注释与实现明确「只在登记/释放时持互斥锁，绝不跨网络等待」；多 SID 经 BTreeSet 排序一次性原子取得（无部分占用→无循环等待）；`shared_session_admission`（adapters run_store.rs:141-160）按 canonical SQLite 路径 Weak 注册表共享，独立 home 互不拖住；操作任务持有 lease 至收据落库（session_tree.rs:560「客户端取消等待不能让已入队写入失去保护」，fork/retry 均 tokio::spawn 持 permit）；准入后重读会话行并复核归属（session_tree.rs:507-520）。亲跑 `slow_model_does_not_block_independent_session_or_metadata`、`authenticated_barrier_rewind_execute_and_cancelled_waiter`、`direct_driver_independent_database_reentry_and_release` 绿。
3. **lifecycle/branch revision/权限快照复检；重复请求幂等或冲突** — `load_session_admission` 返回 owner/agent/lifecycle/permission_mode/未知文件效果并在 drive 侧复核；fork 事务内重查深度闸与 `session_operations::verify` 版本期望（adapters session_tree.rs:658-663）；分支头 revision 递增 + model_history record 侧 head 比对拦截「检查后变更」；requestId 经 `session_operations` 日志（`Begin::Fresh` vs 重放冲突）。亲跑 `latest_permission_and_lifecycle_are_rechecked_before_commit`、`completed_fork_replay_conflict_and_recovered_event_are_consistent`、`retired_seed_identity_stays_deleted_and_no_journal_reuse_is_preserved` 绿。
4. **关键状态与事件同事务可恢复一致；不靠重建任务补事件** — v10 迁移新增 `session_operations` 日志表（migrations.rs:568-584）；fork 行插入 + `insert_key_event` + `session_operations::complete` 收据在同一写事务（adapters session_tree.rs:695-877）；亲跑 `positive_and_fork_event_atomicity`（事件写失败回滚状态）、`unknown_file_effect_survives_storage_failure_restart_and_replay`（未知文件效果跨重启/重放不自动重做）绿。

执行者 R1 段所称「f03 两红为测试 harness macOS 路径缺口、生产零改动」与源码事实一致：f03/f04 harness 现均对 home/workspace root `canonicalize`（f03:305-311/443、f04:396-398），本人在 macOS（/tmp→/private/tmp 别名环境）亲跑 21/21 复现通过。

## R06-AR-F04 —— 判定 PASS

remaining_scope 五条逐一对照：

1. **真实活动时间 + 并发/回退规则与清理调度一致** — `record_run_started` 事务内 `touch_on_connection`（adapters run_store.rs:1628；session_files.rs:1413-1420 `MAX(COALESCE(...),now)` 防时间回退）；冷度判定统一读 `sessions.last_activity_unix_ms`（session_files.rs:957-997，差异台账 D3）；亲跑 `f04_c02_real_activity_cleanup_and_clock_rollback`（可控时钟）绿。
2. **正式 fork 链继承文件与旧 ID 别名；别名唯一性与主体范围** — fork 事务内收集保留段消息 `reference_identities` → `fork_files_on_connection` 复制 managed payload、新 id 按目标 ownerKey 派生、旧 id/旧路径累积 legacy 别名（adapters session_tree.rs:654-725；service sessionfiles.rs:1202-1221 对照现役 forkSessionFiles）；`resolve_identity` 只认本会话属主行/别名（sessionfiles.rs:1347-1374），无全局别名放大；多次 fork 撞名 `-{n}-` 唯一化。亲跑 `f04_c01_formal_fork_retains_attachment_in_siblings`（真实 HTTP fork 后兄弟分支旧 sf_ id 可读）绿。
3. **附件/工具产物/最终消息/历史引用规范化登记与引用闭包** — 工具完成事件与最终消息写入同事务 `sync_message_refs`（run_store.rs:1107-1122、1871）；rewind/reset 链同样同步（adapters session_tree.rs:264/385/851）；`collect_reference_identities` 结构化递归+标记扫描、解析不到的进 broken 点名不静默。亲跑 `f04_c03_reference_commit_cleanup_window_is_atomic`、`f04_existing_paths_preserve_authority_and_other_session_payload` 绿。
4. **清理/删除/文件回退接入真实事实；区分权威文件与可重建缓存** — `cleanup_cold_payloads` 仅清 `managed_cache` 且非 expired，保护所有有效引用、活跃运行与实际读线程（payload lease），托管根与 canonical 路径守卫，外部文件即使位于缓存目录也无删除权；会话删除后清扫成功才移除托管身份。亲跑含真实阳性删除样本：`f04_c02`（未引用冷缓存真删、被引用交付保留）、`f04_permanent_delete_removes_owned_cache_and_retains_other_branch`、`f04_cancelled_read_retains_physical_lease_until_exit`、`f04_expired_owned_blob_recovers_identity_atomically`、`f04_c04_legacy_import_interrupt_missing_and_restart` 绿。
5. **NoopChangeLog 不再掏空文件历史** — 生产组合根（service lib.rs:1330-1341）`register_core_file_tools` 注入 `filehistory::FileProductChangeLog::new(file_history, session_files)`；NoopChangeLog 仅存类型供旧测试接线（filetools.rs:387），正式链 write/edit 修改回调接 SessionFileService（与 AR_HANDOFF「正式文件工具组合根已接」一致）。

矩阵记录：F04 的 repair/self_check/independent_review 在旧矩阵中为 NOT_RUN（W04 执行者传输中断遗留）；本判定基于冻结候选上的当前源码与亲跑，覆盖其全部 remaining_scope，历史 NOT_RUN 字段不构成当前缺口。

## 附注

- 已知背景核实：`r00_management_leaves` 的环境限制红（LAN 接口被防火墙阻断）与本三项无关，未纳入判定。
- 矩阵内各 finding 的 executor 自检证据路径（`/Users/study_superior/.codex/worktrees/...` 及 artifacts W01-E01/W03-E02）不在本副本内，本审查未依赖、也未复用其结论；判定完全基于本人独立亲跑 + 冻结候选源码核对。
- 保留副本与产物：worktree `/tmp/lingxi-rv1`、target `/tmp/rv1-target`、合成环境 `/tmp/rv1-env`、审查产物 `/tmp/rv1-review`。
