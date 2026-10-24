# R08-T06 进度台账｜回滚与迁移后新增数据保留

- 任务：task-8（R08-T06 回滚演练，design-first）
- owner：t02-core
- 起始：2026-10-23

## 状态：实现完成 READY_FOR_REVIEW（2026-10-23 续接会话：D1/D2/D3 演习矩阵全绿 + runbook 交付；待独立评审，评审人不得为实现者）

## Lead 批复（2026-10-23，逐点裁决，全部采纳建议 + 附加条件）

- **J1 允许**：条件=原字节归档必须先于写入完成并复验 sha256，写入逐条入 journal；准备相在收据 prepareWrites[] 逐项列名。
- **J2 默认不回写**：映射表 mergeBackDefault=false 钉死；回写永远是显式人工决策。
- **J3 冻结身份集差集 + already_present 交叉校验**，对账不符=refused 零静默。
- **J4 模态 A 默认**；B 仅 --recover-new-root 显式；B 二次导入零增长必须落收据 secondImportZeroGrowth 字段。
- **J5 新动词 rollback/reforward**，migrate 本体与 T05 语义零改动。
- **J6 指针回指新根** + 修复模式；双侧保留。
- **J7 导出 scaffold-seed**（oldUiReadable=false）。
- **J8 印章目标 v2 {1,1,1} Node 同构**。

**附加三钉**：
1. 评审 MINOR N4 转我：epoch_adopt 证据注入 id 双钉不变量，T06 复用 epoch 模块时保持。
2. A12 哨兵禁用 queued 行；queued 续投语义进 runbook 明示。
3. D1 新根真实服务新增必须走**正式服务入口**（非直接 SQL 注入），覆盖会话+附件+设置编辑三类。

## 流水

### 2026-10-23 设计期

1. **取证完成**（全部源码/实测锚定）：
   - 任务书 R08-T06 §4 五条 + A11/A12 原文提取（逐字）。
   - ADR-004 闸边界 + R01 ROLLBACK_DESIGN 全读。
   - 印章事实：fixture 旧根 legacy v1 `{epoch:7}`（r08_t04_fixture_gen.mjs:311-312，epoch 域 out-of-scope 合成值）+ 现役旧栈 `DATA_EPOCH=1`（shared/contract-versions.json）→ 旧栈对旧根原样**结构性拒启** → 回滚必须含受控旧根准备相（裁决 J1/J8）。
   - 新根实测布局（t05-d2 scratch）：runs.db 在 `lingxi-service/data/runs.db`；settings 在 `user/preferences.json`（userprefs.rs:4）；knowledge/ 保格式库；session-files/；legacy-import/ 收据群；lingxi-service/ 运行时再生面。
   - V27 簿记表（import_runs/import_batches/import_issues，migrations.rs:1087）+ T04 行级 already_present（convert_sessions.rs:243）→ 增量边界判据 = 旧根冻结身份集差集。
   - A12 代码事实：list_pending_outbox 仅拾 queued（bridge.rs:1020）；claim queued→started（L1035）；settle started→confirmed/unknown/failed（L1068）；started 走 validate（L1040）；连接器重配置 bump generation（L340）；V25 deliveries "delivery_id 幂等保证不重复外发"。∴ confirmed 永不重发、unknown 永不拾取，结构性成立。
   - 复用缝：pointer.rs（read_pointer L101/commit_pointer L124/原子 rename）、backup.rs（run_backup L293/backup-manifest.json L25）、selfcheck 模板（orchestrator L825）、statfs/quiesce。
   - 旧栈启动形态：`node scripts/launch.js server` + `LINGXI_PORT`（server/index.ts:352），就绪 = server-info.json + 环回应答。
2. **设计交付**：`docs/rust-tauri/R08/T06_DESIGN.md`（8 开放点逐一作答 §3–§10；裁决点 J1–J8 含建议；演习矩阵 D1/D2/D3；命令面 rollback/reforward 新动词、migrate 零改动；收据 schema t06-rollback-receipt/v1 + t06-reforward-receipt/v1）。
3. 已报 Lead 审批；**批准前不动实现**。

## 实现计划（2026-10-23 代码研学后锁定）

### 模块布局

1. **`rust/crates/lingxi-adapters/src/storage/rollback_delta.rs`**（新，~700 行+单测）——增量导出/导入纯存储逻辑，DB 代码留在 storage crate：
   - `LegacyIdentitySet`：sessions=BTreeSet(jsonl stem)、messages=BTreeSet((session_id,message_id) 含 fallback `legacy-{s}-{line}` 与标记 `legacy-unknown-block-{s}-{line}`)、runs=BTreeSet(runBoundary runId)、model_call_ids=BTreeSet（旧根 model-observability-db 经 `observability::read_legacy_observability_rows` 导出）、files=BTreeMap(relpath→sha256)（session-files+knowledge+user/preferences.json 等 register 类）。
   - `derive_legacy_identity_set(old_root)`：`manifest::build_manifest` + `classes::classify`（T04 同一分类表）+ `session_jsonl::parse_session_jsonl` + converter 同一身份规则；unmapped-shape jsonl 与 converter 同样跳过并记录（不猜）。
   - `export_db_delta(new_runs_db, &L)`：枚举用户表，剔除簿记表（import_runs/import_batches/import_issues/import_run_extensions/legacy_runtime_state_registry）与 sqlite 内部表；身份=主键列值数组（PRAGMA table_info 通用）；legacy 归属过滤：sessions/messages/runs/model_call_usage/observability_legacy_trace 按 L 对应集合过滤，其余表 L=∅ 全行为增量。
   - `apply_db_delta(target_db, tables)`：单事务 PRAGMA foreign_keys=OFF → 逐行 INSERT/幂等判定 → `PRAGMA foreign_key_check` 违规响亮回滚。表级语义：messages/runs/model_call_usage/bridge 等=同身份同内容 already_present / 不同内容 conflict 响亮（不可改账）；sessions=发散行 INSERT OR REPLACE 计 session_row_updated；默认 INSERT-only+conflict。二次应用必须 applied=0（零增长证明）。
   - 鲁棒性论证：L 只从旧根真实内容推导（不过近似），漏推导身份 → 仅多进 payload 且 B 模态 already_present 交叉通过，无数据丢失方向。
2. **`rust/crates/lingxi-service/src/cutover_rollback.rs`**（新，~900 行+单测）——rollback/reforward 编排器；**自带回滚专用 journal**（rollback-journal.json，~150 行，同原子写纪律；不动 journal.rs 一个字节=J5）。
   - rollback 9 相：preflight（旧根存在+印章可读+指针→新根+新根印章 epoch2 committed+空间 required_bytes+slack+新根无活写者 probe_rust_instance_lock）→ stop_new_writer → backup_new_root（复用 backup::run_backup，id=rollback id）→ delta_export（rollback_delta 导出 + 包 manifest.json/lineage.json，lineage.sourceCutoverId 取指针 cutover_id）→ export_verify（重导 L+重盘 N 确定性复证 identitySetSha256）→ old_root_prepare（J1：归档原字节入包 old-root-prepare/ → sha256 复验 → 逐条 journal → 写 v2{1,1,1} 印章=复用 `epoch::write_legacy_import_stamp`（write_stamp 同一原子链）→ 归档并移除 stale transition journal）→ pointer_switch（commit_pointer 原子 rename）→ old_stack_probe（新探针，下述）→ committed。
   - old_stack_probe：绑定 127.0.0.1:0 取闲口 → `node <launcher> server`（--old-stack-launcher/--old-stack-cwd 显式传）+ LINGXI_HOME=旧根 + LINGXI_PORT；断言 server-info.json 出现且端口/pid 匹配 + 环回 GET /api/server/identity 200 + stderr 无 LINGXI_DATA_EPOCH_BLOCKED/无 fail-open 警告标记；SIGTERM 退出后：新根树 sha256 零变化复验 + 旧根写集=前后 manifest diff 逐项必须 classify 为 Regenerable 或 server-info.json（T04 分类表即白名单权威，无 ad-hoc 清单；data-epoch.json 必须在白名单外=印章字节不变断言）。**J6：探针失败 → 指针提交回新根（last-known-good）+ refused + 收据 repairAction**。
   - resume 语义镜像 migrate：无 --resume+journal→拒 exit 2；--resume+committed→exit 0 already-committed；--resume+refused→终态；相内断→续跑。
   - reforward：模态 A（默认 fast-repoint）：新根树 hash == 回滚收据 seal hash（变了=响亮拒）+ 旧根印章仍 {1,1} 族 + commit_pointer→新根 + selfcheck（expectations=probe 新根 db 实况计数，验活服务-盘面一致性，非同义反复）+ 收据 mode=fast-repoint。模态 B（--recover-new-root）：preflight → backup 旧根（reforward 自有 checkpoint_id+backup_manifest_id）→ legacy_import（复用 legacy_import_runner::run_import + ImportArgs{source:旧根,target:新恢复根}，instance_lock_held=true）→ delta_import（apply_db_delta）→ delta_verify（二次应用断言 applied=0 → secondImportZeroGrowth=true 落收据 J4；哨兵行 identity+sha 集合比对）→ epoch_adopt（复用 EpochAdoptionEvidence 链：cutover_id=reforwardId=transitionId、checkpoint_receipt.id==checkpoint_id——**附加钉① N4 双钉由同一 validate() 强制，零绕行**）→ pointer_switch → selfcheck → committed。
   - B 目标根默认 work_dir/recovered-root；原新根全程零触碰（J6 双侧保留）。
3. **`main.rs`**：`rollback`/`reforward` 两动词，镜像 migrate 解析；migrate 本体零改动（J5）。
4. **standin**：`scripts/rust-tauri/r08_t06_standin.mjs`（r00-stub 形态：确定性环回 HTTP 计数 + 平台收据签发）。
5. **演习**：D1 全程（新根走**正式服务入口**新增 会话+附件+设置编辑 三类——附加钉③；哨兵 confirmed/unknown/started 无 queued——附加钉②）；D2 故障注入 3 腿（manifest 篡改/空间不足/旧探针失败=预占端口）；D3 恢复 reforward B + secondImportZeroGrowth。
6. **runbook**：`docs/rust-tauri/R08/T06_ROLLBACK_RUNBOOK.md`（明示 queued 续投语义——附加钉②）。

### 代码研学锚定（本日完成）

- runs 身份=runBoundary runId，`insert_legacy_run` INSERT 列 1:1，同 id 异主=响亮拒（convert_sessions.rs:134-178,283）。
- sidecars 只改 sessions 元数据（标题/meta），不产生新身份行；session-manifest-sqlite 只读校验（convert_sidecars.rs:187-279）。
- legacy observability 导入链：`run_store.rs:3600` INSERT OR IGNORE model_call_usage（model_call_id 身份，异内容 Conflict 不可改账）+ observability_legacy_trace（L3661）。∴ L 须含 model_call_ids（旧根 model-observability-db 推导，observability::read_legacy_observability_rows 为 pub）。
- automation_jobs_v1 导入零写入 → L=∅。
- `manifest::build_manifest`/`source_root_sha256`（manifest.rs:73/136）= 树盘点与 canonical hash 原语，双根通用。
- `plan::build_plan`（plan.rs:30）= classify 分派 + batch_key 形态；session-files 目录级 convert 批次指纹规则（L70-93）。
- journal.rs 结构（L32-171）+ orchestrator import 相幂等放行/在途发现（L605-672）+ run_backup 六参签名（L573-580）+ stop_write_probe 双探针用法（L518-541）——全部复用面确认。

### 2026-10-23 实现期①——rollback_delta 完成

1. **`rollback_delta.rs` 建成**（~1470 行含 7 单测，全绿；`storage/mod.rs` 注册 `pub mod rollback_delta`）：
   - `LegacyIdentitySet`/`derive_legacy_identity_set`（manifest+classify+converter 同规则身份推导）、`export_db_delta`（剔除簿记表+sqlite 内部表；legacy 归属过滤；sessions 行 oldUiReadable=false=J7）、`write_delta_package`/`read_delta_package`（manifest.json 零时间戳全确定性；行级 rowSha256+文件 sha+packageSha256 三层复验）、`apply_db_delta`（单事务 FK OFF→逐行 INSERT/already_present/UPDATE(sessions 计 session_rows_updated)/conflict 响亮→fk_check 违规回滚）、`apply_files_delta`、`PackageManifest`。
   - 7 单测：身份推导同 converter 规则（含 fallback/marker/确定性）、unmapped jsonl 跳过不猜、导出过滤+簿记剔除+oldUiReadable=false+L=∅ 全行导出、包往返+篡改检测（payload 字节翻转→sha mismatch 响）、幂等回灌+二次零增长+消息发散 conflict 响、FK 违规整事务回滚且孤行不入、session-files 同径异 sha 拒。
   - `cargo test -p lingxi-adapters --lib`：**176 passed / 0 failed**（T04/T05 存量测试全绿，零回归）。
2. **重大发现（已报 Lead）：T04 manifest.rs `file_sha256` 双重哈希**——`sha256_hex(&hasher.finalize())` 对已完成摘要再哈希一次，manifest 条目"sha256"= `sha256(sha256_raw(content))`，非文件真实 sha256。T04/T05 全程同口径自洽故从未炸响；**该约定已锁进 T05 封存对拍锚**（r08_t05_refactor_parity.mjs:231 的 `80a8d2c2…feaa` 比对的是 Rust `source_root_sha256` 输出）→ **不得"修复"**（修复=改变锚值=破坏封存证据链，J5）。JS 夹具生成器 rootHash（r08_t04_fixture_gen.mjs:333-335）为真 sha256 但格式不同（无 size 列），本就不同轨、从未直接对拍。
3. **T06 处置（约定显式隔离）**：包层一律用**真·内容 sha256**（`shasum -a 256` 可对拍）：`FileDeltaEntry.sha256`=读文件实算；`diff_files` 差集判定仍走 manifest 约定（新旧同口径自洽），产出条目另取真哈希并复验 size 防扫描-读取间改写；`write_delta_package` 拷贝后复验字节哈希=记录值（拷贝完整性，响亮拒）；`LegacyIdentitySet` 文件哈希字段注释明示"仅内部比对，不写出包、不作外部证据"。已写进代码注释+将进 runbook。

### 2026-10-23 实现期②——cutover_rollback 编排器编译全绿

1. **`cutover_rollback.rs` 建成**（bin crate，~2175 行含 6 单测）：journal 自有原子写纪律（schema t06-rollback-journal/v1）、rollback 9 相 + reforward 模态 A 5 相/模态 B 9 相状态机、resume 五则、per-phase repairAction 表、old_root_prepare J1 归档-复验-逐条 journal 链、old_stack_probe（环回裸 TCP HTTP + SIGTERM 关停 + 关停后新根零变化/旧根写集白名单/stderr 标记三断言 + J6 指针回指）、模态 B epoch_adopt 复用 EpochAdoptionEvidence（N4 双钉注释在位）。
2. **main.rs**：`mod cutover_rollback;` + rollback/reforward 两动词（镜像 migrate 解析面，migrate 本体零改动 = J5）。
3. **构建与测试**：`cargo build -p lingxi-service` 零警告；`cargo test -p lingxi-service --bin lingxi-service` **13/13 绿**（6 新 + 7 orchestrator 存量）；`cargo test -p lingxi-adapters --lib` 176/176 绿。Cargo.lock **零新增 `[[package]]`**（chrono 依赖边为 R08-T04 B3 存量）。
4. **三处实现期对设计的偏差（全部显式记录，非静默降级）**：
   - ① `backup_new_root` **不复用 `backup::run_backup`**：run_backup 走 T04 旧根分类表，新根路径全 Unclassified 会响亮拒；实现=本地 `copy_tree_full`（符号链接/特殊文件响亮拒）+ `stillpoint::scan_pass` 源根哈希比对 + 自写 backup-manifest（schema t06-newroot-backup/v1）。run_backup 复用仅保留在模态 B `backup_old_root`（旧根、T04 分类表原生域）。
   - ② 探针 HTTP 端点由设计文本 `/api/server/identity` 改为**实测既有端点 `/api/health`**（server/index.ts L389 启动期 503 → L1027 就绪 200，免鉴权）；设计不变量"环回 HTTP 应答 200"语义不变。
   - ③ journal 文件**按动词分离**：`rollback-journal.json` / `reforward-journal.json`（设计原文单文件名表述的精化；reforward preflight 只读消费 rollback journal committed 事实，verb 字段交叉校验）。
5. **代码事实补记**：`logs/{timestamp}.log` 分类为 runtime-diagnostics=**Register 而非 Regenerable**（classes.rs:198）；写集白名单（server-info.json ∪ Regenerable）不覆盖日志文件——若演习中旧栈冷启动向旧根写日志文件，探针写集断言将按设计响亮拒（此时属设计假设与现实冲突的真实红色，须上报裁决，不扩白名单）。D1 演习观察项。
6. **环境红登记（非本任务引入，不修）**：lingxi-service lib 3 测试在本会话环境失败，统一 panic=`Storage(InvalidRequest { detail: "MCP 原始只读目录不可用" })`（background_tasks owned_media / skills_github_tests real_driver / sessiontools concurrent_atomic_receipts）——harness MCP 只读目录不可用于 cargo test 进程的环境限制；本任务改动全部在 bin crate，lib 未触碰。

### 2026-10-23 Lead 验收② + 裁决（实现期②后）

1. **里程碑②验收**：三处偏差全部追认（backup_new_root 独立实现 / /api/health 实测端点 / journal 动词分离），理由成立、语义不变、台账在录。
2. **观察项③裁决（先发制人，不等 D1 红）**：探针写集白名单改为**证据派生**——D1 前先跑控制探针（旧栈在 fixture 旧根未回滚副本上冷启动，证据 `t06-control-probe-*`）实测冷启动写集；D1 白名单 = 控制实测 ∪ {准备相两件套}。控制集之外出现写入=真红。口径分离：回滚相内冻结、探针相活跃、活跃写集以控制实测为准。已入 T06_DESIGN §7.2a。
3. 家族 #5 环境红（lib 3 红）定性不变，不修。
4. 下一步序：控制探针先行 → standin → D1/D2/D3。

### 2026-10-23 控制探针首轮红 + J9 裁决（夹具同构修复集）

- **t06-control-probe-01（红，保留）**：旧栈冷启动被夹具合成噪声阻断——① `security/execution-leases.json` 夹具写 `{version:1, leases:[{id,holder}]}`，现役 `core/execution-lease-registry.ts:94` 要求 `schemaVersion:1` 且 `:105`+`core/remote-execution-boundary.ts:49` 对每条 lease 强校验全字段（夹具行非语义有效 lease，T04 注册类覆盖样本）→ startServer 抛错 exit 1；② 夹具 `agents/a1` 无 config.yaml → first-run 判目录无效（保留不载入）并新建默认助手 lingxi，会掏空 §7.2 断言⑤取证价值。
- **J9（Lead 批准，封闭修复集两项，J1 纪律全适用）**：
  1. `execution-leases.json` 改写为生产空注册表形状 `{schemaVersion:1, leases:[], createdAt, updatedAt}`（逐字段引 `createEmptyExecutionLeaseRegistry`，execution-lease-registry.ts:83-90；原字节先归档包内 + sha256 复验 + journal + prepareWrites[] 列名）；
  2. `agents/a1/config.yaml` 补最小有效配置，形状引生产 first-run 模板：`lib/config.example.yaml` L7-9（agent.name/yuan 块）+ `core/first-run.ts:181-199` seedDefaultAgent 播种链（模板复制→desk/memory 合并→writeSecretFileSync 0600）；最小键 `agent.name`/`agent.yuan`，0600。
  3. 封闭集之外出现新阻断 → 停手报 Lead，不得自行扩集；拒绝 leases-only 备选肢（a1 不可读掏空断言⑤）。
- 下一步：控制探针修复版取证（t06-control-probe-02），转绿后 prepare 相实装 J9 修复集，再开 D1。

### 2026-10-23 控制探针取证（02 红→05 绿，J9+⑤a 修复集生效）

- **02 红（保留）**：leases/a1 修复生效（registries 过、first-run 不再建 lingxi），但夹具 `knowledge/knowledge.db` 合成表（`documents`/`processing_artifacts` 最小列形）撞 `knowledge-store.ts:1781` createSchemaV16（IF NOT EXISTS 跳过建表后引用缺失列 `content_snapshot_id`）→ 构造期崩。**第三例同族夹具噪声**（{epoch:7}/leases/knowledge 表，真实旧根三者皆无）。
- **⑤a（Lead 批准，四钉）**：knowledge.db 合成表单事务 RENAME 保序（行零丢失）→ 迁移链自建生产空表；原库字节 J1 先归档；术后 `integrity_check=ok` 入证据；prepareWrites[]/readable-scope 披露「knowledge 视图为空」；三例噪声记入 T07 对照清单（T07 见任一=数据异常须停手）。
- **03 红（保留）**：server 已完整启动（server-info 写出）但探针 readiness 判 403——**纠正早前错误事实**：`/api/health` 就绪相**需鉴权**（`server/http/route-security.ts:58` AUTHENTICATED_ONLY；503 引导相才免鉴权，`server/index.ts:389`）。复用面 `cutover/probe.rs:142` 本就带 `Authorization: Bearer`（正确），自研 JS 探针与 `cutover_rollback.rs` 的 `http_get_status` 缺 token（缺陷，修）。
- **04 红（保留）**：`stopped` 作用域 bug（try 内声明、报告段引用）——修。
- **05 绿（`CONTROL_PROBE_RESULT port=63993 added=55 modified=4 removed=0 markersClean=true`，PROBE_EXIT=0）**：三 epoch 标记零出现（清洁启动），server-info 正常注销，launcher 随退。实测冷启动写集逐类核对（classes.rs）：54/55 added + 3/4 modified 全部命中 Register/Regenerable 注册类；**唯一未注册路径 `models-store.json`**（内容 `{}`，出处 = 三方 SDK `@earendil-works/pi-ai` models-store，生产冷启动同写）→ §7.4 链候选。
- **`data-epoch.json` modified 解码**：旧栈清洁启动把 `lastVersion:"legacy-import"` 盖为自身版本 `"0.1.43"` + 刷 `updatedAt`；四个 epoch 字段（schemaVersion/epoch/minimumReaderEpoch/committedDataEpoch=2/1/1/1）逐值冻结。生产忠实行为 → 设计「探测期间印章逐字节不变」条款与证据冲突，提请 Lead 裁决改「四字段冻结 + lastVersion/updatedAt 允许盖戳」。
- 另修 JS 探针孤儿防护（finally 先 SIGTERM owned-pid 再杀 launcher）；cutover_rollback.rs 待修：① http_get_status 加 token 鉴权；② L1161「免鉴权」错注；③ 终止纪律改 SIGTERM server_info.pid。

### 2026-10-23 §4b models-store.json 注册表增补（裁决②）落地

1. **条件①定性（审计漏网）**：STORAGE_READINESS.md 全文（69 类责任表 + 新增 Rust 存储清单 + 反例表）零处提及 models-store.json = 审计漏网，非已列漏实现。写链完整锚定：`core/model-manager.ts:234` `modelsJsonPath=<lingxiHome>/models.json` → `lib/pi-sdk/index.ts:354-360` `ModelRuntime.create({modelsPath})` → pi-coding-agent `model-runtime.js:80` `FileModelsStore(dirname(modelsPath)/models-store.json)` → 数据根顶层（与 models.json 同目录）；`desktop/main.cjs:2075` 显式删除 `PI_CODING_AGENT_DIR` 环境变量 → 路径唯一来源是 Lingxi 显式传入的 modelsPath，不受宿主 Pi 安装影响。**事实更正**：写出者是 `@earendil-works/pi-coding-agent` 的 `FileModelsStore`（pi-ai 仅导出 ModelsStore 接口/内存实现）；台账早前「pi-ai 运行时存储」归因系 SDK 家族层面正确、具体包名修正如上。
2. **实施**：classes.rs `provider-state`（Register）patterns 增补 `"models-store.json"`（紧邻 models.json）；GENERATED 注释追加例外增补段（重跑提取必须保留该 pattern）；新单测 `models_store_json_amendment_classifies_provider_state`。类数保持 69（`table_has_69_classes_and_unique_ids` 不受影响）。
3. **条件②**：R02-T04_STORAGE_REGISTRY.json `generated_by` 追加 R08-T06 注记（先例风格：T04 收口同款），明确定性/写链/锚重证/原文件不动；migrations 指纹区 28 条零改动，`r02_registry_consistency.py` 复跑 **PASS**（t06-registry-amend-consistency-01）。
4. **验证链**：build 绿（t06-registry-amend-build-01）；adapters lib **177/177**（176+1 新测试，t06-registry-amend-lib-01）；**条件③双跑重证绿**（t06-registry-amend-proof-02，16/16；原台账误记 17/17，独立评审 N1 更正）：v3 基座双生成 rootHash 全等 `80a8d2c2…feaa`（封存锚保持）、双夹具 models-store.json 零命中、基座双导入 outcome/收据归一化全等（35/35/5）、v2 大规模源（/tmp/t04i-src-a08b，433 文件 435 批）双导入精确复现封存值 435/435 done + issues 5206 + Rust 锚前缀 40f42dc1——增补对既有分类面（435 批广度）零扰动的最强回归证据。
5. **红证据保留**：t06-registry-amend-proof-01 红（8 断言失败）——根因=本驱动首版口径错位（把 v2 大规模源的封存计数 435/5206 误用于 v3 基座 32 文件夹具，且 outcome.status 实为 `complete` 而 `completed` 是 import_runs 表 state 字段），非增补引入；v2/v3 两基座关系与两种锚口径（JS rootHash vs Rust source_root_sha256）已在修正版驱动中分层断言。
6. 条件④：「T05 评审后注册表修正」已写入 R02 注册表注记与本台账；最终 diff 评审包披露义务挂 T08 交付清单（不遗忘项）。

### 2026-10-23 §4c 证据派生白名单 + §4d J9/⑤a 实装（cutover_rollback.rs）

1. **§4c 白名单重设计**（原 `PROBE_ALLOWED_EXACT ∪ Regenerable` → 证据派生）：
   - `PROBE_ALLOWED_CLASSES`（20 类）= 控制实测写集（t06-control-probe-05：added=55/modified=4/removed=0）按 classes.rs 归类的 **18 类**（含 Convert 类 `session-manifest-sqlite`——实测确认旧栈冷启动写自己的会话清单库，白名单不能只按 Register/Regenerable 收）∪ 准备相件所属 2 类（`execution-leases`/`agent-profile`，防御性，§7.2a「∪ 准备相件」）。
   - 泛化面 = 类模式本身（logs/{timestamp}.log→runtime-diagnostics、knowledge/indexes/**、skills/{skillName}、file-history 显式 wal/shm 模式）+ **{db}-shm/-wal 兄弟泛化**（strip 后缀对主库类判别——兄弟文件属主库存储身份）。
   - 集外类/未分类路径写入 = forbidden 真红（§7.2a：控制集之外出现写入=回滚事务泄漏，停手上报）。
   - **裁决①条款**：`epoch_stamp_clause`——data-epoch.json 四字段（schemaVersion/epoch/minimumReaderEpoch/committedDataEpoch）逐值冻结 + 仅 lastVersion/updatedAt 盖戳允许 + 字段集变化（增/删）拒绝；探针报告新增 `epoch_stamp_evidence` 证据列（前后六字段逐值 + provenanceStamped 判定）。data-epoch-transition.json 永远禁止。
   - 单测：`write_set_classification_whitelist` 重写（通过腿+拒绝腿）；**`control_probe_write_set_pinned`**——控制实测 58 路径（55 added+3 modified 非印章）逐字面钉进测试，白名单收紧到容不下控制写集时在此响亮红（+印章通过腿实测字节 legacy-import→0.1.43）。
2. **§4d prepare 相扩展**（J1 纪律全适用：原字节先归档包内 + sha256 复验 + 逐条 journal + prepareWrites[]）：
   - **J9①**：execution-leases.json 非合规（非四键 {schemaVersion:1,leases:[],createdAt,updatedAt} 空注册表形状）→ 归档+重写生产空注册表；合规 → verify-only 零写（resume 幂等）。
   - **J9②**：扫描 agents/{id}/ 缺 config.yaml 者 → 补最小配置（agent.name=agentId 大写/agent.yuan，0600，形状引 lib/config.example.yaml L7-9 + first-run.ts:181-199；夹具 a1→"A1" 与控制探针验证字节同构）；已有 → 零写。
   - **⑤a**：knowledge.db 合成表判别式（`documents` 表存在即合成——生产全 schema 无此表；`processing_artifacts` 缺 content_snapshot_id 列即合成——生产 v16 该列 NOT NULL）→ 原库字节归档+复验 → 单事务 RENAME fixture_synthetic_*（行数前后核对零丢失）→ PRAGMA integrity_check=ok → 披露「knowledge 视图为空」。生产形状/已改名 → verify-only 零写（幂等）。
   - `prepareFixSetClosure` 落相位结果：封闭五件套清单 + 「集外新阻断停手上报」规则。
   - 单测：`knowledge_synthetic_detection_rename_idempotent`（判别+改名+行零丢失+幂等）、`knowledge_production_shape_not_flagged`（生产形状不误伤）。
3. **验证链**：t06-whitelist-build-01 红（BTreeSet difference 迭代类型，2 错）→ 02 绿 + svc-bin 14/14；t06-prepare-build-01 红（move 语义 2 错 + Iterator::next 形态 1 错）→ 02（1 警告）→ 03 绿 **0 警告 + 16/16**。红证据全保留。
4. **替身对接面研学结论（§4e 依据）**：桥接投递真实链 = outbox(queued)→claim(started)→pipe Send→SDK worker→Receipt{confirmed+platformMessageIds / confirmed 空 ids→unknown / unknown}；**注入缝 = `LINGXI_BRIDGE_NODE`+`LINGXI_BRIDGE_WORKER`**（bridge_worker.rs discover 显式路径优先，spawn env_clear 仅传 PATH/LANG 族——worker 无法经 env 拿参数，端口须常量约定）；**r07_bridge_flow.rs 已有完整协议替身先例**（Node 单行 worker：init→ready→inbound message→send→receipt/suppress-receipts；started 行在服务重启时经 startup 校验落 unknown，r07 测试 `sent_without_receipt_becomes_unknown_on_restart_and_is_not_sent_again` 钉死该语义）；宿主 30s 回执超时会先行落 unknown（需在窗口内停服才走 startup 校验路径——证据里按 error 文案区分两条落定路径）。服务起停面：`lingxi-service --home <root>` → stderr `LINGXI_SERVICE_READY addr=127.0.0.1:<port>`；token 在 `<root>/lingxi-service/local-token.json`（auth.rs:76）；模型链 ConfigModelGateway 支持经根内 provider 配置指 base_url（本地确定性端点可行）。

### 2026-10-23 §4e 替身 + §4f 演习矩阵 D1/D2/D3 全绿 + §4g runbook

1. **§4e 确定性替身**（`scripts/rust-tauri/r08_t06_standin.mjs`，smoke 绿 t06-standin-smoke-01）：
   - 环回 HTTP 平台（**固定端口 47613 常量约定**——bridge_worker.rs spawn env_clear 后 worker 拿不到 env，端口必须编译期/脚本期常量）：POST /platform/send 按会话路由三腿（s1→确认收据+计数；s2→无 message_id→宿主落 unknown 不伪造；s3→withhold 回执扣留）+ GET /counter /journal /health + /admin/shutdown；**计数器持久化**（standin-state.json 每发送即写，跨进程续值）。
   - 内置 OpenAI 兼容确定性模型端点（/v1/chat/completions 流式/非流式）——桥接回信走真实模型链（ConfigModelGateway）。
   - 产出配套协议 worker（t06-standin-worker.cjs）：LINGXI_BRIDGE_NODE/WORKER 注入缝（r07_bridge_flow.rs 先例形态）；init→ready→三入站消息→send→POST 替身→receipt（confirmed+platformMessageIds / unknown / withhold）。
2. **§4f D1 全绿**（t06-d1-drill-final-08，**38/38 PASS，最终二进制绑定复跑**）：fixture→migrate→正式服务（--config 模型面指向替身）→**钉③四类正式入口**（agents 创建〔迁移新根按设计无 agents/，Register 不转写——F3〕+sessions/new+attachments/local snapshot+preferences/appearance PUT）→**钉②三哨兵零 queued**（S1 confirmed 替身收据 t06-standin-1 / S2 unknown / S3 started→优雅关停受控落定）→外部计数每腿恰 1 次且全程冻结→rollback 九相 committed（J9+⑤a 全生效：印章 v2{1,1,1}+lastVersion 0.1.43 盖戳〔裁决①〕、leases 空注册表、a1 config、合成表改名 8+8 行保全、生产空表链建）→证据派生白名单探针绿→reforward A→S1 收据字节往返不变/计数/附件 sha256 不变。
3. **§4f D2 全绿**（t06-d2-drill-03，27/27）：(a) 篡改包 manifest→--resume 后 export_verify 拒（packageSha256 mismatch）+ status=refused + 双侧保留 + 旧根零触碰；(b) macOS RAM disk 32MB 小卷（hdiutil+newfs_hfs）→preflight 拒 `insufficient space on backup volume: need 271802182, available 32751616`+零触碰（印章仍 {epoch:7}）；(c) 早退 launcher 桩（设计例因「端口预占用」bind:0 取号属 TOCTOU 不可确定性复现；早退桩同属「旧栈不服务」故障类，J6 exercised 面相同——**偏离在案**）→old_stack_probe 拒+**J6 指针回指新根**+journal.refusal.repairAction{bothSidesPreserved:true}。
4. **§4f D3 全绿**（t06-d3-drill-08，26/26）：reforward --recover-new-root 到全新恢复根 committed；**J4 secondImportZeroGrowth=true 落收据**；A11 会话/消息计数逐值相等（12/835）+ 原新根附件在恢复根字节相同（子集方向；恢复根多 3 件=legacy 全量重导入带回原新根已按过期 GC 的 legacy 会话文件——服务 GC 是产品合法行为，**非数据异常，如实披露计数**）；A12 三哨兵身份/收据逐字节同（S3 受控落定）；替身计数恒=1/腿；原新根零触碰。
5. **七个真缺陷发现与修复**（红→绿链全保留）：
   - ① **export_verify 确定性门被 lineage 时间戳击穿**（t06-d1-drill-04）：manifest.json 内嵌 lineageSha256（折入 packageSha256），lineage.json 含 createdAt——重导各自取 now → 恒红。修：重导**复用首次导出的 lineage.json 字节**（lineage 是 provenance 输入非导出器行为）；rollback_delta 注释同步改诚实（manifest 确定性以「同 lineage 输入」为条件）。
   - ② **export_verify payload 比对路径扁平名残留**（t06-d1-drill-05）：`replace('/',"__")` 残留 vs 包实际层级存储。修：直接 `payload/files/<relpath>`。
   - ③ **export_db_delta READ_ONLY 连接污染封缄源**（t06-d2-drill-01）：sqlite READ_ONLY 对 WAL 库仍创建 -shm/-wal → 新根封缄破裂。修：**P6 纪律落地**——stage_quiesced_copy 复制 db+wal+shm 到临时目录开副本（副本上正常恢复语义，wal 内已提交事务可见），封缄源零触碰。
   - ④ **apply_db_delta 空表 arity 校验误拒**（t06-d3-drill-02）：包侧零行表 identity_len 回退 read 占位 pk_columns=0 ≠ 目标 1。修：空表跳过（先于 pk 检查）。
   - ⑤ **BOOKKEEPING_TABLES 漏列 schema_migrations**（t06-d3-drill-04）：目标自跑迁移产生自己的收据行（applied_at 必异）。修：补列（6 项）。
   - ⑥ **automation_* 族回灌语义缺失**（t06-d3-drill-02/03）：T04 期「automation_jobs_v1 导入零写入→L=∅」结论**证伪**——legacy 导入会建 legacy-job-* 行、新根服务调度器又合法改写 next_run_at/updated_at → 同身份异内容响亮拒，恢复式再前滚对任何跑过调度器的根必炸。修（设计精化，D3 红驱动）：automation_* 族与 sessions 同款可变运行态语义——发散整体替换并计数（新根为切后权威；messages/runs/model_call_usage/bridge_* 账本表维持不可改账）。
   - ⑦ **session_files 族根作用域路径冲突**（t06-d3-drill-05）：包行携带原新根绝对路径+生命周期态，恢复导入已用本根路径重建 → 整体替换=写回错误路径（活性错误）。修（裁决）：身份冲突且发散→**恢复根行胜**，计数 `pathScopedPreserved` 显式披露（非静默）；新身份附件照常导入。
   - 另两处驱动侧红（非产品）：readiness 行在 stdout 被误弃（drill-01/02）；⑤a 断言口径（drill-06：旧栈探针后迁移链自建生产空表=⑤a 设计意图本身）。
6. **§4g runbook**：`docs/rust-tauri/R08/T06_ROLLBACK_RUNBOOK.md`（哈希口径披露〔manifest 双哈希内部约定+真内容 sha256 分轨〕/前置检查/命令序列/逐相期望/准备相封闭五件套/探针白名单与印章条款/失败修复流/再前滚两模态+恢复根语义/外部收据闸+**queued 续投语义**/收据位置/旧 UI 可读范围映射）。
7. **验证基线（最终候选）**：build 0 警告（t06-final-build-01）；svc-bin **16/16**（t06-final-svcbin-01）；adapters **177/177**（t06-final-adapters-01）；D1 final 38/38（t06-d1-drill-final-08）；D2 27/27；D3 26/26。红证据目录：t06-d1-drill-01..06、t06-d2-drill-01..02、t06-d3-drill-01..07、t06-whitelist-build-01、t06-prepare-build-01/02、t06-exportverify-fix-*、t06-payload-path-fix-build-01、t06-export-staging-build-01、t06-arity-fix-*、t06-automation-semantics-*、t06-bookkeeping-fix-*、t06-pathsoped-fix-*、t06-emptytable-fix-*（根因全部在案）。
8. **T07 对照清单移交**（不变）：真实旧根见 {epoch:7}/leases 合成行/knowledge 合成表任一=数据异常停手；P9 钉子（首启会话计数==导入收据计数）；A11 恢复根附件子集方向语义。

## 候选问题登记

（空——设计期无新增候选）

## 红线自查

- 合成 HOME / /private/tmp scratch；真实 ~/.lingxi 未触碰；禁改文件未触碰；无 commit/push；旧根零改动红线在本任务内由设计显式升级为"唯一事务性写入点=旧根准备相"（J1 已获 Lead 批准：原字节归档先于写入+sha256 复验+逐条 journal+收据 prepareWrites[]）。

### 2026-10-23 T06 独立评审 PASS → T06 CLOSED

- **独立评审 PASS-with-MINORS**（BLOCKER 0 / MAJOR 0 / MINOR 5；评审人=独立子代理，非实现者；证据 rev-t06-* 8 组全新目录全部独立复跑绿：build 0 警告 / svc-bin 16/16 / adapters 177/177 / D1 38/38 / D2 27/27 / D3 26/26 / amend 双跑 16/16 / 一致性门 PASS）。报告 `T06_REVIEW.md`；PROGRESS-review.md 已附摘要。
- 评审核实：J1–J9/⑤a/①②/附加三钉逐条真实落实；七修复判定正确无新静默面；四偏差全部接受；最终绿证与最终代码同绑（candidateDiffSha256 全等 d5962b50…e859）。
- MINOR 处置：**N1 已修**（台账 17/17→16/16 更正）；**N4 本节回写**——T06_DESIGN §6 模态 B 原文「主键冲突=already_present 计数、不入数」经 D3 红驱动精化为：sessions 与 automation_* 族（可变运行态）发散整体替换计数；session_files 族恢复根行胜计 pathScopedPreserved；空表跳过；schema_migrations 簿记排除——现行权威以 T06_ROLLBACK_RUNBOOK §6 与 rollback_delta.rs 实现为准；**N2**（read_delta_package 空 if 死块）与 **N3**（旧根侧 derive_legacy_identity_set→observability、⑤a 判别 knowledge.db 两处 READ_ONLY 直开未套 P6 副本纪律——WAL 模式真实旧根会静默建 -shm/-wal；评审定性移交 T07）登记为 T07 开工首批处置项；**N5**（sessionRowsUpdated 字段名单用）不改（避免收据 schema 漂移）。
- **T06 CLOSED（评审通过）**。task-8 终态。
- **评审移交 N2/N3 落地（2026-10-23，T07 开工首批）**：N2 删 `read_delta_package` 空 if 死块（连带移除孤儿 `canonical_manifest_string_from_value`）；N3 对称套 P6——`derive_legacy_identity_set` 的 observability 读与 ⑤a `detect_synthetic_knowledge_tables` 判别均改为 `stage_quiesced_copy` 副本只读（`stage_quiesced_copy` 改 pub 供 service 侧复用），旧根冻结零触碰对 WAL 模式真实旧根亦成立。重绑证据：build 0 警告（t06-n2n3-build-01）、svc-bin 16/16、adapters 177/177、D1 38/38（t06-n2n3-d1-01）、D2 27/27（t06-n2n3-d2-01）、D3 26/26（t06-n2n3-d3-01）。
