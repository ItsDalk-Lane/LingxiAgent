# T06 独立评审报告（回滚与迁移后新增数据保留）

- 评审人：R08-T06 独立评审代理（与实现者 t02-core 无交集，资格成立）
- 评审日期：2026-10-23
- 评审对象：T06 全部实现（工作区未提交，git HEAD 86840d1700397d3dd620a41f2b884df4cddd5cc2，
  分支 codex/rust-tauri-migration）——cutover_rollback.rs / rollback_delta.rs / classes.rs
  models-store.json 增补 / main.rs 双动词注册 / R02 注册表注记 / 6 个脚本 / runbook / 证据链
- 评审方式：设计文档（J1–J9、⑤a、裁决①②、附加三钉）逐条对读实现 + 七缺陷修复审查 +
  证据链抽查（八组最终绿证据 + 红证据签名）+ 独立复跑（rev-t06-* ×8，r08_run.py 包裹、
  目录全新不复用）+ 抽查深核 ×4
- 评审期间未修改任何被评审产品代码/脚本/设计文档（评审人不修问题）；未执行任何
  git checkout/restore/clean/stash/commit/push。

## 结论：PASS-with-MINORS

- **BLOCKER 0 项、MAJOR 0 项、MINOR 5 项**（N1 账面计数误差、N2 死代码卫兵、
  N3 旧根 WAL 副文件潜在冻结面〔移交 T07〕、N4 设计文本漂移、N5 计数字段命名复用）。
- 独立复跑 E1–E8 **全部绿**，且全部与实现者最终绿证据**同绑同一候选 diff**
  （candidateDiffSha256=d5962b507d16b8f78b0279b3fa804619b2ce26c5b3bdf441bedc6a828e9ae859，
  即最终绿证与最终代码绑定成立）。
- 四项实现者登记偏差全部**接受**（见 §5 偏差裁定表）。
- 家族 #5 环境红：本次 bin/adapters 套件**未出现**同签名红（E2 16/16、E3 177/177 全绿）；
  lingxi-service lib 套件未在本次评审复跑范围（任务书 E 清单不含）。

---

## 1. 评审范围与方法

| 层面 | 对象 | 方法 |
|---|---|---|
| 编排器 | `rust/crates/lingxi-service/src/cutover_rollback.rs`（2814 行） | 全文通读 vs T06_DESIGN §3–§9 + 裁决①⑤a/附加三钉 |
| 增量包 | `rust/crates/lingxi-adapters/src/storage/rollback_delta.rs`（1552 行） | 全文通读 vs 设计 §4（J3/J7）+ 七缺陷修复 |
| 分类表增补 | `classes.rs` provider-state `models-store.json` + 注册表注记 | 对读裁决②四条件 |
| 动词面 | `main.rs`（git diff HEAD 487 行） | 逐 hunk 核 T06 改动 = `mod cutover_rollback` + rollback/reforward 两动词臂（纯增量）；migrate 臂保持 T05 形态（J5，见 §3 注记） |
| 脚本 | standin / d1 / d2 / d3 / registry_amend / control_probe（共 1514 行） | 全文/头部通读 vs 演习矩阵与三钉 |
| 证据链 | t06-* 62 组（最终绿 8 组 + 红链抽查 4 组） | receipt sourceBindings/candidateDiffSha256 比对 + output.log 断言计数 + 红签名对账 |
| 独立复跑 | rev-t06-* ×8 | r08_run.py 包裹、全新目录、串行、演习前 pkill 孤儿 |
| 抽查深核 | D1/D2/D3 scratch 独立核验 ×4 | sqlite3/shasum/手工 JSON 解析（不经被评审代码） |

## 2. 设计符合性逐条核（任务 A）

| 条目 | 落实证据 | 裁决 |
|---|---|---|
| J1 准备相纪律 | `rb_old_root_prepare`：原字节先归档包内 `old-root-prepare/` → sha256 回读复验 → 写入 → `prepareWrites[]` 逐条（印章/过渡日志/leases/knowledge 四路归档复验链逐行在位；rev-t06-d1 复跑包内见 4 个 .orig + prepareWrites=5） | **落实** |
| J2 默认不回写 | rollback 全相零代码路径写旧根 settings/knowledge；二者仅进增量包（diff_files category=knowledge/preferences，旧根侧无 apply） | **落实** |
| J3 冻结身份集差集 | `derive_legacy_identity_set`（manifest+classify+converter 同构身份规则）+ `export_db_delta` 过滤 + `rb_export_verify` ①重导 identitySetSha256 比对；对账不符=响亮拒 | **落实** |
| J4 模态 A 默认 / B 显式 / secondImportZeroGrowth | `recover_new_root` 开关；A 五相零导入；B `rf_b_delta_verify` 二次应用断言 `zero_growth()` → 收据 `secondImportZeroGrowth:true`（rev-t06-d3 复跑收据实测 True，二次 applied=0/filesApplied=0/16 already） | **落实** |
| J5 migrate 本体零改动 | main.rs diff 逐 hunk：T06 仅 `mod cutover_rollback;` + rollback/reforward 两臂 + usage 行；migrate 臂/LINGXI 判定均为 T04/T05 已有 hunk；cutover_rollback.rs 自带 RollbackJournal，零 import `cutover::journal`；orchestrator 7 存量测试在 E2 16/16 内全绿。*（注：T04/T05 亦未提交，无法 git 级分离 T05→T06 增量，本核以 hunk 归属 + 模块隔离 + 存量测试绿三重证据判定）* | **落实** |
| J6 探针失败指针回指 | `rb_old_stack_probe` Err 分支 commit_pointer 回新根 + refused + repairAction；rev-t06-d2 leg-c 实测：pointer→new-root、refusal.phase=old_stack_probe、bothSidesPreserved=true、旧根原字节 4 件归档在包 | **落实** |
| J7 scaffold-seed 导出 oldUiReadable=false | `DeltaRow.old_ui_readable=Some(false)`（仅 sessions 表）+ 单测断言 | **落实** |
| J8 印章 v2{1,1,1} | `write_legacy_import_stamp` 复用 + 写后 read_stamp 复验；rev-t06-d1 旧根实测 `{"schemaVersion":2,"epoch":1,"minimumReaderEpoch":1,"committedDataEpoch":1,...}` | **落实** |
| ⑤a knowledge 合成表 | 判别式（documents 存在 / processing_artifacts 缺 content_snapshot_id）→ 单事务 RENAME 行数前后核对 + integrity_check + 「knowledge 视图为空」披露 + 幂等（改名后判别式不再命中）；两个单测钉住 | **落实** |
| 裁决① 印章四字段冻结+provenance 盖戳 | `epoch_stamp_clause`：FROZEN 四字段逐值 + 键集相等 + 仅 lastVersion/updatedAt 允许 + 其余拒绝；`epochStampEvidence` 证据列实测 `{frozenFields:{2,1,1,1}, provenanceStamped:[lastVersion,updatedAt], verdict:allowed}`（legacy-import→0.1.43）；通过腿+拒绝腿单测 | **落实** |
| 裁决② models-store.json | classes.rs provider-state pattern 增补（GENERATED 注释带保留条款+新单测）；R02 注册表 generated_by 注记（定性/写链/锚重证/原文件不动四要件在文）；migrations 指纹 28 条零动，一致性门复跑 PASS（rev-t06-consistency-01）；双跑重证绿（rev-t06-amend-01：v3 锚 80a8d2c2 保持、bulk 435/435/5206+40f42dc1、双跑全等、零命中） | **落实** |
| 附加钉① N4 双钉 | `rf_b_backup_old_root` 注入 `checkpoint_receipt["id"]==checkpoint_id` + `backup_manifest_id` 独立钉原始 manifest 字节；`rf_b_epoch_adopt` 复用 `EpochAdoptionEvidence`/`run_epoch_transition`（T05 评审过的校验链零绕行） | **落实** |
| 附加钉② 哨兵禁 queued | D1/D3 断言「sentinels exactly 3 (no queued)」；queued 续投语义进 runbook §7；rev-t06-d1/d3 复跑均 PASS | **落实** |
| 附加钉③ 正式服务入口 | D1/D3 四类全走 HTTP API（agents POST / sessions/new / attachments/local snapshot / preferences/appearance PUT），模型面经 --config 指替身、无直接 SQL 注入 | **落实** |
| J9（leases/a1 config） | 封闭五件套写入集 + `prepareFixSetClosure` 披露「集外新阻断停手上报」+ 生产合规零写幂等路径 | **落实** |

## 3. 安全与红线审查（任务 B）

**单写者/零触碰**：
- rollback 探针新根断言=全树 `source_root_sha256` 前后零变化（`new_root_unchanged`，无排除面）——**真覆盖旧根全树之外的新根全树**；旧根活跃写集走证据派生白名单 + 印章条款，禁 data-epoch-transition.json。
- D2 零触碰断言（腿 a/b）= `hashTree` 全树逐文件 sha 前后相等（脚本侧独立于产品代码）；rev-t06-d2 复跑腿 b 旧根印章仍 fixture `{epoch:7}`、无 old-root-prepare 件、journal refused@preflight——**零触碰复核成立**。
- D2b 空间拒在 preflight（任何旧根写入之前）；D2c 探针失败发生在 prepare 之后（设计允许：准备相是唯一写入点，原字节归档在包，双侧保留）。

**无静默降级——七缺陷修复逐条判定**：

| # | 修复 | 正确性 | 新静默面？ | 超授权？ |
|---|---|---|---|---|
| ① lineage 复用 | 重导复用首次 lineage.json 字节，确定性门聚焦导出器行为；注释+runbook 诚实披露（manifest 确定性以同 lineage 输入为条件） | 正确（lineage 是 provenance 输入非导出行为，复用合理且披露） | 无 | 否 |
| ② payload 路径 | `payload/files/<relpath>` 层级直读，修扁平名残留 | 正确（与 write_delta_package 存储形态一致） | 无 | 否 |
| ③ READ_ONLY→副本 staging（P6） | `stage_quiesced_copy` 复制 db+wal+shm 到 temp 开副本，封缄源零触碰；t06-d2-drill-01 红（seal broken）→绿链在案 | 正确 | 无（D2 复跑绿实证封缄不破） | 否 |
| ④ 空表跳过 | `rows.is_empty() → continue`（先于 pk 检查）；有行而目标缺表仍响亮拒 | 正确（空表无可回灌，subagent_* 惰性建表族） | 无 | 否 |
| ⑤ schema_migrations 簿记 | BOOKKEEPING_TABLES 补列（6 项）；目标自跑迁移的 applied_at 必异 | 正确 | 无 | 否 |
| ⑥ automation_* 族 | 发散行整体替换 + 计数（sessionRowsUpdated）；messages/runs/model_call_usage/bridge_* 维持同身份异内容响亮 Conflict | 正确（「导入零写入」结论被 D3 证伪；新根为切后权威；**计数显式披露非静默**） | 无（替换有计数、收据可审计） | 语义精化超设计原文，已披露并经 D3 红驱动（见偏差表④/N4） |
| ⑦ session_files 族 | 身份冲突且发散→恢复根行胜 + `pathScopedPreserved` 计数；新身份附件照常导入（字节经 files 面恢复） | 正确（整体替换会写回原新根绝对路径=活性错误；保留目标行=根作用域正确性优先） | 无（显式降级披露，rev-t06-d3 实测 pathScopedPreserved=3） | 同上 |

**fail-closed 总体**：全相位失败=refused 终态 + exit 2 + repairAction + 双侧保留；journal 腐化/verb 错配/schema 不符响亮拒；refused 不可 resume；无 catch-and-continue 路径（通读未见）。

## 4. 证据可信性（任务 D）

- 八组最终绿证据（t06-final-build/svcbin/adapters、t06-d1-drill-final-08、t06-d2-drill-03、t06-d3-drill-08、t06-registry-amend-proof-02、t06-registry-amend-consistency-01）candidateDiffSha256 **全部=d5962b50…e859 且与盘上 candidate.diff 实算一致**；本人复跑（rev-t06-build-01 等 8 组）新捕获 diff 亦=d5962b50…——**最终绿证与最终代码绑定成立**，sourceBindingsStable 全 true。
- 断言计数核对：build 0 警告；svcbin「16 passed」；adapters「177 passed」；D1 PASS=38/FAIL=0（S3_SETTLE_PATH_FINAL=graceful-shutdown-settle，legs 恒 1/腿）；D2 PASS=27/FAIL=0；D3 PASS=26/FAIL=0；amend-proof **PASS=16**（台账记 17/17——见 N1）；consistency 尾行 PASS。
- 红证据诚实性抽查：t06-d1-drill-04（`determinism check failed: manifest.json differs between exports`）、t06-d2-drill-01（`new root seal broken`，leg-c）、t06-d3-drill-04（`conflict in schema_migrations`）、t06-d3-drill-05（`conflict in session_files`）——签名与台账七缺陷逐条吻合，红→绿闭合链完整，未发现删改/粉饰。

## 5. 偏差裁定表（任务 C）

| 偏差 | 裁定 | 理由 |
|---|---|---|
| D2(c) 早退 launcher 桩替代「端口预占用」 | **接受** | bind:0 取号即放属 TOCTOU 不可确定性复现；早退桩同属「旧栈不服务」故障类，J6 exercised 面（readiness 失败→指针回指+refused+repairAction+双侧保留）完全相同；rev-t06-d2 leg-c 实测全中；台账+脚本头双披露 |
| S3 哨兵经优雅关停路径落 unknown（非 startup 校验路径） | **接受** | 两条落定路径皆受控皆零重发（bridge.rs 关停路径 vs startup validate）；drill 接受两径并打印实际路径（rev-t06-d1=graceful-shutdown-settle）；startup 校验路径语义由 r07 测试 `sent_without_receipt_becomes_unknown_on_restart_and_is_not_sent_again` 另行钉死；runbook §7 双径披露。*注：T06 演习自身未直接复跑 startup 校验路径，覆盖依赖该 r07 存量测试——移交注记* |
| A11 恢复根附件取子集方向 | **接受** | A11 本义=「新数据可恢复且不重复」=原新根现存附件在恢复根字节相同（drill 断言 missing=0，rev-t06-d3 复跑 0 缺失）；恢复根多出 3 件=legacy 全量重导入带回已 GC legacy 文件（`A11_REIMPORTED_LEGACY_FILES count=3`，全为 legacy-* 名）——服务 GC 是产品合法行为，非数据异常；如实披露计数而非静默差 |
| automation/session_files 回灌语义精化 | **接受**（附 N4） | D3 红驱动、正确性成立（不修则恢复式再前滚对任何跑过调度器的根必炸/写回错误路径）、计数显式披露、账本表牙齿保留；但 T06_DESIGN §6 原文未回写（N4），且属实现期设计精化，建议列入 T08 最终 diff 评审包披露清单 |

## 6. 独立复跑结果（任务 E，rev-t06-*，r08_run.py 包裹、目录全新、串行）

| # | 命令 | 证据目录 | exit | 关键行 |
|---|---|---|---|---|
| E1 | `cargo build --manifest-path rust/Cargo.toml -p lingxi-service` | rev-t06-build-01 | 0 | Finished dev profile；0 warning |
| E2 | `cargo test --manifest-path rust/Cargo.toml -p lingxi-service --bin lingxi-service` | rev-t06-svcbin-01 | 0 | `16 passed; 0 failed` |
| E3 | `cargo test --manifest-path rust/Cargo.toml -p lingxi-adapters --lib` | rev-t06-adapters-01 | 0 | `177 passed; 0 failed; 13 ignored` |
| E4 | `node scripts/rust-tauri/r08_t06_d1_drill.mjs --bin … --work /private/tmp/rev-t06-d1` | rev-t06-d1-drill-01 | 0 | PASS=38/FAIL=0；D1 DRILL OK；legs {1,1,1,0}；counts 12/835/10 |
| E5 | `node scripts/rust-tauri/r08_t06_d2_drill.mjs --bin … --scratch /private/tmp/rev-t06-d2` | rev-t06-d2-drill-01 | 0 | PASS=27/FAIL=0；RAM disk 自卸载（无残留） |
| E6 | `node scripts/rust-tauri/r08_t06_d3_drill.mjs --bin … --work /private/tmp/rev-t06-d3` | rev-t06-d3-drill-01 | 0 | PASS=26/FAIL=0；D3 DRILL OK；A11 多余件 count=3 |
| E7 | `node scripts/rust-tauri/r08_t06_registry_amend.mjs --bin … --scratch /private/tmp/rev-t06-amend` | rev-t06-amend-01 | 0 | 16/16；v3Anchor=80a8d2c2… bulk 435/435/5206 anchor=40f42dc1 doubleRunIdentical=true |
| E8 | `python3 scripts/rust-tauri/r02_registry_consistency.py` | rev-t06-consistency-01 | 0 | `PASS: migrations.rs MIGRATIONS == registry (28 entries …)` |

全部 sourceBindingsStable=true、candidateDiffSha256=d5962b50…（与实现者最终绿证同绑）。
每腿前置 pkill 孤儿；演习后无孤儿服务/替身进程、无 RAM disk 残留。
环境限制披露：lingxi-service lib 套件（家族 #5）未复跑（不在 E 清单）；bin/adapters 套件未见同签名红。

## 7. 抽查深核记录（任务 F，全部独立工具、不经被评审代码）

| 项 | 方法 | 结果 |
|---|---|---|
| F(i) 旧根印章四字段+盖戳 | 直接 cat D1 scratch 旧根 `data-epoch.json` + 解析 rollback-receipt `probe.epochStampEvidence`/`prepareWrites` | 四字段 `{2,1,1,1}` 逐值正确；lastVersion `legacy-import→0.1.43`+updatedAt 盖戳（provenanceStamped 两键）；prepareWrites=5 与封闭五件套一一对应；包内 4 个 .orig 归档在案 |
| F(ii) D2 零触碰 | 直接检查腿 b scratch：旧根印章原样 `{epoch:7, stampedAt:…}`、目录树无准备相产物（find old-root-prepare/*.orig 为空）、journal refused@preflight（insufficient space） | 旧根在 preflight 拒绝路径**零写入**复核成立；腿 a 包篡改拒（packageSha256 mismatch）+指针未动、腿 c J6 回指+归档在包，亦逐一直接核过 |
| F(iii) D3 恢复根收据 | 直接解析 reforward-receipt.json + reforward-journal facts | `secondImportZeroGrowth:true`；二次 totals 全表 applied=0、filesApplied=0、filesAlreadyPresent=16；首灌 `session_files.pathScopedPreserved=3`、`automation_jobs_v1.sessionRowsUpdated=2`（显式披露在案）；sentinelReceipts 含 confirmed/unknown 行 |
| F(iv) 包哈希真·sha256 | 逐条调用 `shasum -a 256` 对拍 files-manifest 全 16 条 payload 文件（sha+size）+ 独立用 Python 重算 manifest packageSha256 | 16/16 全等、packageSha256 独立重算一致——**真内容哈希口径成立**（与 manifest 域双哈希约定分轨未混用） |

## 8. 发现清单（分级）

**BLOCKER：0 项。MAJOR：0 项。**

**MINOR 5 项**（均无阻断；是否采纳由 Lead/实现者决定；建议只写不做）：

- **N1（台账计数）**：`docs/rust-tauri/R08/PROGRESS-t06.md` §4b 与主 PROGRESS 尾节均记
  t06-registry-amend-proof-02「17/17」，该驱动实际只有 16 项 check（输出 16 PASS；
  本人复跑 rev-t06-amend-01 亦 16/16）。证据本身绿，仅账面数字差一。建议订正为 16/16。
- **N2（死代码/无效卫兵）**：`rust/crates/lingxi-adapters/src/storage/rollback_delta.rs:898-901`
  `read_delta_package` 内 `if canonical_manifest_string(...) != canonical_manifest_string_from_value(...) { /* 注释 */ }`
  ——条件计算结果被丢弃的空 if 块。有效防篡改门是紧随的 packageSha256 重算（覆盖全部实体
  字段；未知字段注入可通过但行为惰性，无安全后果）。建议删除空块或改为响亮校验。
- **N3（旧根 WAL 副文件潜在冻结面，移交 T07）**：P6 副本纪律只落在新根 runs.db
  （`stage_quiesced_copy`）；旧根侧两处 READ_ONLY 直开未对称处理——
  `derive_legacy_identity_set`→`observability.rs:17`（回滚相 4/5）与
  `cutover_rollback.rs:1376` `detect_synthetic_knowledge_tables`（相 6）。WAL 模式库上
  READ_ONLY 连接仍会在旧根创建 -shm/-wal（同 t06-d2-drill-01 根因；该红实证了此行为）。
  knowledge 侧若发生在相 4/5 会被 export_verify 的 identitySetSha256 门**响亮**拦截
  （knowledge-{db,-wal,-shm} 属 Register 类进 L）；但 observability 侧（不进身份集）与
  相 6 prepare 之后的写入**无检测**，属「回滚相内旧根冻结（除准备相外零写入）」的潜在
  静默违例。夹具库非 WAL 故 D1–D3 全绿未触发；与 T04 converter 同一 open flags（先例同
  形）。**移交 T07：核对真实旧根 observability/knowledge 库 journal_mode；若存在 WAL，
  将 P6 副本纪律对称扩展到旧根读取，或在其 T07 对照清单加查点。**
- **N4（设计文本漂移）**：`docs/rust-tauri/R08/T06_DESIGN.md` §6 模态 B 原文仍是
  「身份主键冲突 = already_present 计数、不入数」，与实现/runbook 的精化语义
  （sessions/automation_* 发散整体替换计数；session_files 族恢复根行胜 pathScopedPreserved；
  账本表响亮拒）不一致。runbook（§6 恢复根语义）已如实记载，但设计文本未回写注记。
  建议设计补「实现期精化（D3 红驱动，2026-10-23）」段落或指回 runbook；并按 §4b 条件④
  精神将该语义精化列入 T08 最终 diff 评审包披露清单。
- **N5（计数字段命名复用）**：`ApplyStats.session_rows_updated` / 收据 `sessionRowsUpdated`
  字段被 automation_* 族整体替换单独复用（rev-t06-d3 收据 `automation_jobs_v1.sessionRowsUpdated=2`）。
  语义在 runbook 披露、计数非静默，仅字段名以 sessions 为名易误读。供后续治理项
  （字段更名会动收据 schema，不建议 R08 内改）。

**观察项（不计级）**：① 探针白名单按**类模式**泛化（非控制实测的精确路径集）——属 §7.2a
裁决记录在案的泛化面，钉死测试 `control_probe_write_set_pinned` 保证控制集覆盖，接受；
② reforward 收据 `sentinelReceiptsUnchanged:true` 为结构推导硬编码（依据=bridge_* 账本表
同身份异内容必响亮 Conflict + 二次零增长，D3 drill 另以 SQL 逐字节核对哨兵行），接受；
③ 台账引用 D2b 拒绝文案 need=271802182 与复跑 271802012 微差（树字节计数随运行时间戳
微变），非账面错误。

## 9. 对 T07/T08 的移交注记

1. **（随 N3）** 真实旧根 sqlite WAL 模式核对——observability/knowledge 若为 WAL，回滚相
   旧根冻结面存在无检测写入点；建议 P6 对称扩展或对照清单加查点。
2. **（随偏差②）** S3 startup 校验路径的演习覆盖依赖 r07 存量测试；T07 若做真实旧根演练
   可择机直接复现 startup 落定路径取证。
3. **（随 N4）** automation_*/session_files 回灌语义精化 + models-store.json 注册表修正
   + manifest 双哈希不修约定，均应在 T08 最终 diff 评审包集中披露（台账已有承诺项）。
4. T07 对照清单既有三条（{epoch:7}/leases 合成行/knowledge 合成表任一出现=数据异常停手）
   在 runbook §3 与 PROGRESS-t06 §4f.8 在案，维持。
5. D1/D3 drill 每次 fixture→migrate 全程跑（非复用 T05 产物），T07 若复用本套演习脚本，
   注意替身固定端口 47613 的互斥（并发跑两份 drill 会撞端口）。

## 10. 结论

T06 实现与 T06_DESIGN（J1–J9、⑤a、裁决①②、附加三钉）**一致**；单写者/零触碰、
无静默降级（七修复逐条核过，全部计数/响亮披露）、失败双侧保留+修复模式、A11/A12 验收链
（D1+D3 全绿 + 本人独立复跑 + 4 项独立工具深核）全部成立；最终绿证据与最终代码同绑。
**评审结论：PASS-with-MINORS**（5 项 MINOR 均为账面/整洁/文档级与一项条件性潜在隐患
〔N3 移交 T07〕，不阻塞 T06 关闭与 T07 启动）。

---

评审人：R08-T06 独立评审（rev-t06-*，8 组）｜2026-10-23
