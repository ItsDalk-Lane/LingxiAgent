# R06-AR-W12-P01 一次性修复交接

状态：**READY_FOR_INDEPENDENT_REVIEW / READY_FOR_FRESH_REVIEWER**。本报告是修复者本地自检，不签独立PASS；原T07和整个R06均未验收。未commit/push。

原基线/主仓HEAD：`2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d`；分支`codex/rust-tauri-migration`。最终候选为原精确11文件，无新增产品或测试文件；集合SHA256：`332d136324fe20b0b70f9ed326c9d32217b3b51132057563ffa97185b1e0d154`（按排序path+NUL+sha256+LF计算）。

最终只读测试副本：`/Users/study_superior/.codex/worktrees/r06-w12-review2/LingxiAgent`，使用其独有`rust/target`；父控绑定见`../W12-CANDIDATE-02/binding.json`。四C、对抗、service消费者、原A13/A14、TS全家族及适用门禁均以format02候选亲跑。早期主仓及format01成绩不替代最终检查。

## 问题、原因与修复

正式输入为W12-R01的REPORT/results/evidence-manifest，W12-E01仅作线索；已读不可变规格全文及原R06-T07全部Steps、交付、A13/A14和R00到期叶。两个Review1失败先亲自复现，并有真实正常正对照。

| 失败 | 修前实际结果 | 修复与修后实际结果 | 证据 |
| --- | --- | --- | --- |
| W12-R01-F01 | 第二真实SQLite连接升级999后，旧Rust句柄set_variant_status仍Ok，building→failed | 状态写委托统一cache_write即时事务，事务内重查版本；SCHEMA_NEWER、building保持。实际摄入失败消费者拒绝改写并保存错误码 | red-status-01 / green-status-01 / rust-final-02 |
| W12-R01-F02 | 版本检查与usage更新间真实升级999，last_used_at仍2020→2040并返回正常hit | usage检查与UPDATE同一immediate写事务。升级先取得锁则SCHEMA_NEWER、时间不变；写事务先持锁则第二连接SQLITE_BUSY、schema仍3，合法正常刷新 | red-query-01 / green-query-01 / ts-final-02 |

Rust服务将全文replace、向量begin/upsert首批与后批/complete及provider失败收尾统一记录原错误和收尾错误。版本拒绝时不再尝试标failed；provider失败后真实fail_build遭遇版本拒绝时，两个原因均保留。首批64条实际向量已写后第二连接升级999，后批拒写仍保64条和building，不把付费成果清空。

TS检索只对真实SQLITE_BUSY/LOCKED/READONLY/IOERR/FULL/CANTOPEN（含扩展码）保留有日志的最佳努力时间刷新；SCHEMA_NEWER、损坏、约束及一般错误不会无差别吞成正常向量成功。真实锁、query_only、max_page_count触发FULL及触发器约束对照分别亲验。保留原retention/重开/ANN fallback，未取消功能。

探针钩子仅安排第二真实连接的操作时机，真实prepare/run/版本读取和事务仍执行，没有伪造SQL、结果或存储。外部embedding响应仅用loopback替身；知识服务worker、adapter、事实库、网关与引用组装真实执行。

旧open根因另有原base旧红/当前新绿：old-open-rust-red-01重执行原只读base探针，两缓存降版本并删哨兵；old-open-ts-red-01加载git show基线实际三库源码而失败；old-open-ts-green-01加载最终候选实际三库源码，拒绝999且字节/哨兵不变，reset保同inode。

## 四C逐项自检

| 检查 | 本地结果与范围 |
| --- | --- |
| R06-AR-F12-C01 | Rust两个缓存与TS全文/向量/ANN真实拒绝999；DB/WAL字节不变，崩溃SHM按易失索引核存在/长度；旧open根因旧红新绿仍成立。 本地自检通过；独立复核NOT_RUN。 |
| R06-AR-F12-C02 | 真实chmod、SQLite排他/写锁、独立child RLIMIT_FSIZE分类保原件。检索usage真实BUSY/READONLY/FULL允许带日志最佳努力，非I/O约束错误必须抛；解除失败后正常刷新，paid行保留。 本地自检通过；独立复核NOT_RUN。 |
| R06-AR-F12-C03 | 自有无sidecar、独占guard且确认损坏才隔离；unknown/链接/sidecar拒绝。恢复I/O失败隔离副本保留；TS显式reset/rebuild同事务同步副本后清派生表，事实/原文不删且不自动embedding。 本地自检通过；独立复核NOT_RUN。 |
| R06-AR-F12-C04 | 新增Rust status及replace断言覆盖所有公开缓存写；真实worker future index失败保building+错误码持久化；向量首64行后升级999，后批拒写保building和paid行；provider失败的cleanup拒写明确记录。TS升级发生在锁前抛SCHEMA_NEWER并保时间，锁后第二连接升级SQLITE_BUSY且version3正常更新；正常保留期/重开/原fallback回归均绿。 本地自检通过；独立复核NOT_RUN。 |

C01-C04均用合成资料及真实SQLite；Rust child的实际RLIMIT_FSIZE/权限/进程锁、live/crashed WAL、系统临时目录别名、symlink/hardlink、不可信损坏、自有损坏隔离和隔离后恢复失败等原场景保留。崩溃SHM为可重建索引，仅核存在/长度；不假称其易失字节应绝对稳定。

## 同族全扫与兼容矩阵

| 家族 | 全部写入口/附带写 | 处理与验证 |
| --- | --- | --- |
| Rust全文缓存 | open(initialization), ensure_chunk_index_variant, set_variant_status, replace_artifact_chunks, reset, remove_artifact | 即时cache_write事务内版本检查；初始化即时事务+future只读预检；set_variant_status补闸口，其余原闸口保留；rust-final-02 / green-status-01 |
| Rust向量缓存 | open(initialization), begin_variant_build, upsert_batch, complete_build, fail_build, remove_artifact | 每次cache_write即时事务内检查；存储方法原已保护；service所有对应失败收尾统一记录类别/cleanup拒写；rust-final-02 专属C04及两真实loopback消费者 |
| TS portable | buildOrReplaceArtifact, removeArtifact, removeVariant, removeArtifactModel, touchVectorVariants, setVariantStatus, beginVectorVariantBuild, upsertChunkVectorBatch, completeVectorVariantBuild, failVectorVariantBuild, search(last_used_at), rebuild, private migrations/addVariantLastUsedAt | 所有记录写事务immediate内assertWritableSchema；fail委托status；search隐藏刷新补即时门并精确区分合法I/O最佳努力、schema/损坏/约束错误；ts-final-02 / green-query-01 |
| TS全文 | writeVariantMetadata, ensureChunkIndexVariant, setChunkIndexVariantStatus, replaceArtifactChunks, upsertSourceDocument, upsertSections, removeArtifact, removeChunkIndexVariantsByArtifact, reset, schema/migration | 现有即时事务内版本门；reset先同步恢复副本；保留原修复；所有列出公开写与nested委托复核；ts-final-02 |
| TS ANN/catalog/factory | constructor(initialization), begin, markReady, markFailed | 即时事务检查；factory失败保原catalog并明确portable fallback；保留原安全fallback，未取消graph/retention功能；ts-final-02 |
| 真实消费者 | KnowledgeService.process_job全文replace失败, 向量begin/upsert两批/complete失败, provider错误时fail_build收尾, delete_source, knowledge-query-service INDEX_INVALID自愈/FTS显式降级, usearch touch失败→portable search | 生产写都委托受保护原语；Rust错误与收尾分别记录；TS query语义拒绝可明确降级非正常向量命中；；family-scan-final.json / rust-final-02 / ts-final-02 |
| 事实/生命周期范围边界 | Rust KnowledgeFactStore.purge_source文件remove错误, TS knowledge-store初始化及事实业务写, TS knowledge-manager显式purgeSource, ANN graph临时文件清理 | 没有缓存open错误无差别删除。事实库业务旧writer/EX08生命周期全行为不是两缓存修复关闭范围；EX08/F11保留，不扩写他人文件。；；SOURCE_REVIEWED_NOT_F12_CLOSED |

静态全家族实际候选hash与行号见family-scan-final.json；静态复核并不冒充未亲跑的事实库/生命周期完整验收。

## 最终命令与结果

实际工具：绝对`/Users/study_superior/.cargo/bin/cargo`，输出cargo 1.98.1；Node24.16.0、npm11.13.0；macOS arm64。Cargo使用--locked/-j8；每条命令独立合成LINGXI_HOME。Rust日志实际出现8192MiB/600s guard armed，父进程subprocess.run(timeout=600)，不detach/setsid。锁SHA见final-binding.json。

每次attempt日志唯一、不覆盖旧红。results.json的33条commands保存完整argv、cwd、临时home、开始/结束UTC、exit、count、日志SHA与候选/夹具绑定。主要最终检查：

| attempt | 起止UTC | exit | count/说明 | 日志SHA256 |
| --- | --- | --- | --- | --- |
| clippy-final-01 | 2026-10-11T06:27:46.268559+00:00 → 2026-10-11T06:28:15.309284+00:00 | 0 | all-targets -D warnings | `bfe5ecce7caf8df265754570cb775b550158596af38d9cecddcf895e4aaf4734` |
| diff-final-01 | 2026-10-11T06:29:09.993306+00:00 → 2026-10-11T06:29:10.015218+00:00 | 0 | owned产品diff --check | `95b3a5c3471d929a5fb696b6106be775411e1cad0c35162eda1d9eaf28637a60` |
| fmt-final-02 | 2026-10-11T06:24:30.016596+00:00 → 2026-10-11T06:24:31.580231+00:00 | 0 | workspace格式检查 | `75c205370396261b897d9e37b526036d972c9c5cc770e78d9d7bde0654ebeebb` |
| green-query-01 | 2026-10-11T06:26:45.795322+00:00 → 2026-10-11T06:26:45.923771+00:00 | 0 | 2对照 | `cadc74fe3826418c7e4676821685e5f0a75455ba888a8c23c74109a98bedbc50` |
| green-status-01 | 2026-10-11T06:26:20.540526+00:00 → 2026-10-11T06:26:20.886776+00:00 | 0 | 1对照 | `cdf1771f1757fefb53699c8ff78c25b2ce44b76e43ef43231116ba87c5ca8a13` |
| lint-final-01 | 2026-10-11T06:25:13.079363+00:00 → 2026-10-11T06:25:14.296478+00:00 | 0 | 0错误/39既存any warning | `bf66db39cce266ab156acc940089fe960e30f96167d3c7a0897a648186469824` |
| old-open-ts-green-01 | 2026-10-11T06:24:58.155669+00:00 → 2026-10-11T06:24:58.284729+00:00 | 0 | 1对照 | `326eeb7aab967608bf4164da173137bf6f032ef107f4ee3d320c4abe568accc6` |
| rust-final-02 | 2026-10-11T06:24:30.016596+00:00 → 2026-10-11T06:27:36.387254+00:00 | 0 | 5套51登记通过（9+19+6+11+6）；19含3环境child helper，不重复计子进程结果 | `cd335e530b0a6bb642f13f7ec03e5015dd93250586167868f311d942af0b75bb` |
| ts-final-02 | 2026-10-11T06:27:18.741003+00:00 → 2026-10-11T06:27:47.588032+00:00 | 0 | 18套118通过；原16套83保留+6新增=89，另摄入/生命周期29 | `c0207833e8d6a9c875a227fd02df876c1b92da1c249af61585a2aae570c967e0` |
| typecheck-final-02 | 2026-10-11T06:27:59.006277+00:00 → 2026-10-11T06:28:27.471814+00:00 | 0 | root/node/test三份类型检查 | `0a1c35aef4a4c903b7b0099dea7151f9f5cb8d7c238ffb1dc0b0219e8c086ae1` |

最终完整Rust命令：
```text
/Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -j8 -p lingxi-adapters --test knowledge_store_r06t07 --test r06_ar_f12_cache_preservation -p lingxi-service --test r06_ar_f12_cache_consumers --test r06_t07_knowledge --test r06_t07_knowledge_tools -- --nocapture
```

最终完整TS命令：
```text
npx vitest run tests/r06-ar-f12-vector-cache-preservation.test.ts tests/r06-ar-f12-fulltext-cache-preservation.test.ts tests/r06-ar-f12-ann-cache-preservation.test.ts tests/knowledge-index-metadata-migration.test.ts tests/knowledge-section-index.test.ts tests/knowledge-query-index-separation.test.ts tests/knowledge-query-config-alignment.test.ts tests/knowledge-vector-index.test.ts tests/knowledge-vector-backend-parity.test.ts tests/knowledge-vector-backend-fallback.test.ts tests/knowledge-vector-shutdown.test.ts tests/knowledge-ann-crash-recovery.test.ts tests/knowledge-ann-rebuild.test.ts tests/knowledge-index-metadata-backfill.test.ts tests/knowledge-index-variants.test.ts tests/knowledge-ann-index-store.test.ts tests/knowledge-ingestion.test.ts tests/knowledge-lifecycle.test.ts --maxWorkers=1
```

实际五个Cargo测试执行文件路径和SHA绑定在final-binding.json，交接前再核无变化见delivery-binding-check.json。hash捕获时点是完整执行后及交接前两次；不声称首次执行前有自动binary hash记录。真实运行路径由Cargo日志对应，isolated-pre-binding-02在运行前绑定11源文件。探针binary仅在临时目录执行，SHA留证；没有提交binary、整源、.git或buildcache。

fixture-manifest是**运行后**174个Rust最终夹具及对抗夹具的现存文件hash，非输入前hash；操作前后保护由已绑定测试内的真实断言证明。TS afterEach清理后的空夹具清单不冒充输入证据。

## 保留的红与诊断

- ts-new-01真实调度包装只读属性赋值错误，修包装后不改SQL/断言；日志保留
- rust-new-02/03新测试SQL误读列名及时间夹具碰撞，真实schema核对/独立AtomicUsize后保留64paid断言和原2消费者断言
- fmt-final-01错误edition2024，只有owned四Rust文件按工作区2021重排；旧freeze和binding保留
- ts-final-01完整18suite真实1生命周期4秒等待超时；未改该测试/源码/超时。单套16诊断绿后完整18suite118 final02绿，旧整套红保留。
- 最初main并行候选早期绿不作最终证据，format01隔离早期绿亦SUPERSEDED。
- registry-01实际F14失败9 migration vs7 registry/v8v9缺项，未修改

四C和本项修复全部最终绿，但旧红未被删除、改写成不适用或用单套绿替代。ts-final-01原18套是117通过/1生命周期超时；lifecycle-diagnose单套16通过只诊断，再以完整ts-final-02的18套118通过提供新证据。未改原生命周期源码、断言或4秒等待值。

原测试名称零删除，新增文件零；专属service原2+新增4，portable原9+新增6；adapter仍19登记，旧句柄例只增加status/replace保护断言。test-preservation.json记录旧名称与断言计数，计数只作辅助，不能替代实际语义检查。

## 原Task全部义务与本项份额

| 原义务 | 本轮份额与剩余 |
| --- | --- |
| T07-Step1 | 原adapter9/服务11/工具6、当前专属6真实消费者及TS家族118亲跑；导入/分块/embedding/索引/检索/读取/引用/删除/配置相关场景通过，完整组合与所有原拒绝分支未全重验。 F09/F10/F11/F13/EX08；RELATED_REGRESSION_PASS_NOT_WHOLE_T07_ACCEPTANCE |
| T07-Step2 | 原A13 scope/引用与新库research表不存在实测；portable/ANN本地检索及fallback回归通过。 真实聊天自动scope归F13；research旧残留归R08；RELATED_REGRESSION_PASS_NOT_WHOLE_T07_ACCEPTANCE |
| T07-Step3 | 原A14真实HTTP+loopback dim2→dim3→rebuild/hybrid恢复及模型设置变体回归通过；动态身份F10开放。 动态身份完整契约F10不由本项关闭；RELATED_REGRESSION_PASS_NOT_WHOLE_T07_ACCEPTANCE |
| T07-Step4 | 四C完整亲验；两个Review1漏写点及向量错误收尾同族补齐。fallback/持久化/重启/retention/cancel相关原回归通过，Rust完整F11取消归属/EX08仍开放。 F11取消/usage/迟到写；EX08生命周期与purge文件错误；其它原拒绝矩阵。；F12_SCOPE_SELF_CHECK_PASS_OTHER_T07_OBLIGATIONS_OPEN |
| T07-Step5 | 两项Review1失败及同族追加在本轮修复；F09-F11/F13/EX08/F14、真实迁移/目标OS明确保留，不恢复研究引擎、不改到期归属。 ；UNRESOLVED_SCOPE_RECORDED |
| DELIVERY-KnowledgeService | 限定删除原2消费者保留，新增全文正常/未来失败及向量未来拒写/收尾拒绝4真实worker，相关服务回归通过。 ；RELATED_REGRESSION_PASS_NOT_WHOLE_T07_ACCEPTANCE |
| DELIVERY-索引兼容矩阵 | 报告列open、保原件、受控恢复、长期旧句柄、全部缓存写方法、查询隐藏写与错误收尾矩阵；须全新独立复核。 ；F12_COMPATIBILITY_MATRIX_SELF_CHECK_PASS |
| DELIVERY-引用与迁移测试 | A13/A14及原TS迁移/variant/metadata相关回归通过；真实用户旧资料迁移NOT_RUN。 ；RELATED_REGRESSION_PASS_NOT_WHOLE_T07_ACCEPTANCE |
| A13 | 6原网关工具真实测试：scope允许来源、引用可HTTP定位真实原文、非可信来源不提权；真实聊天自动scope仍F13。 ；RELATED_REGRESSION_PASS_NOT_WHOLE_T07_ACCEPTANCE |
| A14 | 原服务A14亲跑通过（60.88s服务套件）；维度变化显式FTS降级，rebuild后恢复，未冒充动态身份F10验收。 ；RELATED_REGRESSION_PASS_NOT_WHOLE_T07_ACCEPTANCE |

A13/A14经原真实网关、知识service、loopback供应商路径复跑；不能因此关闭F10动态身份或F13真实聊天自动scope。原Service/索引兼容矩阵/引用与迁移三项交付仅明确本项已证份额，真实用户旧资料迁移未执行。

R00共24行（含research ledger条目、23条补充验收叶）逐项如下。完整feature_id、原classification/stage_ids/acceptance_ids及原断言均保留在results.json.r00_leaves。以下均为相关回归通过，**不称完整原叶通过**；每叶完整拒绝/错误组合未全重验。

| 原到期叶/条目 | 本轮实际关联场景 | 剩余 |
| --- | --- | --- |
| F-D08-C-RESEARCH-LEDGER | research_engine_tables_are_not_restored, research_engine_tables_are_absent_from_the_new_store | 只证新库不恢复研究表；R08旧research兼容残留仍按原范围。 |
| R00-T02-LA-01E1E3CF7FA8 | notebook_crud_settings_and_ingestion_summary | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |
| R00-T02-LA-3D99324D55C7 | file_import_security_rules, import_pasted_text_parses_chunks_and_ingests_with_stub_embeddings | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |
| R00-T02-LA-763C552CB7DE | notebook_crud_settings_and_ingestion_summary | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |
| R00-T02-LA-E84EEED44AC1 | notebook_crud_settings_and_ingestion_summary | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |
| R00-T02-LA-210C7965364E | import_pasted_text_parses_chunks_and_ingests_with_stub_embeddings | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |
| R00-T02-LA-B0386DC9DED9 | notebook_crud_settings_and_ingestion_summary | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |
| R00-T02-LA-66C538ED4733 | notebook_crud_settings_and_ingestion_summary | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |
| R00-T02-LA-BB1F5E0DE88E | notebook_crud_settings_and_ingestion_summary | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |
| R00-T02-LA-E2AF86A962D0 | citation_resolution_returns_block_snapshot_source_and_viewer, a13_citation_markdown_resolves_to_actual_text | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |
| R00-T02-LA-76E2F5AAF035 | notebook_crud_settings_and_ingestion_summary, ingestion_without_model_lands_pending_embedding_then_completes_after_reboot | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |
| R00-T02-LA-8A62470CC75D | reingest_membership_and_delete_semantics | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |
| R00-T02-LA-C12CD37BA4A3 | reingest_membership_and_delete_semantics | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |
| R00-T02-LA-DB95BAC2A204 | file_import_security_rules | 仅亲跑相关本地导入安全；refresh changed/unchanged完整叶本轮NOT_RUN。 |
| R00-T02-LA-F49D5F80D234 | settings_change_triggers_rebuild_and_variants_coexist, a14_switching_to_incompatible_embedding_dimension_refuses_vector_channel_until_rebuild | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |
| R00-T02-LA-251147E586B2 | import_pasted_text_parses_chunks_and_ingests_with_stub_embeddings | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |
| R00-T02-LA-6189780D7F52 | citation_resolution_returns_block_snapshot_source_and_viewer | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |
| R00-T02-LA-23AB8E0A2493 | snapshot_content_serves_bytes_with_security_headers | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |
| R00-T02-LA-9283371CDAD8 | deletion_of_a_keeps_b_and_removes_all_a_versions, deletion_cache_error_is_reported_and_source_original_remains, source_delete_is_blocked_while_turn_scope_freezes_it | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |
| R00-T02-LA-269958F82F8A | knowledge_read_pagination_and_grep_outline, scope_violations_are_loud_not_fallback | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |
| R00-T02-LA-1AA3F1311F8A | knowledge_manage_actions_and_gates | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |
| R00-T02-LA-5F5365322CFF | knowledge_read_pagination_and_grep_outline | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |
| R00-T02-LA-FBD4D4D29679 | a13_citation_markdown_resolves_to_actual_text, knowledge_read_pagination_and_grep_outline | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |
| R00-T02-LA-062844EDA014 | a13_search_within_scope_returns_only_allowed_sources, a14_switching_to_incompatible_embedding_dimension_refuses_vector_channel_until_rebuild | 完整原拒绝/错误矩阵未全重验；F09/F10/F11/F13/EX08保持开放，不能用F12关闭。 |

## 精确11文件冻结

| path | SHA256 |
| --- | --- |
| `rust/crates/lingxi-adapters/src/storage/knowledge.rs` | `a123a9e0b11e0a95f630c18f8e002c36889905364361eaa34e89bad22506db01` |
| `rust/crates/lingxi-service/src/knowledge.rs` | `3f98131c88c8c1c1945a87752f1702d2cfb75ffdf2454e0d88d4554edbd3c370` |
| `lib/knowledge/vector-index-adapter.ts` | `4c74ef0a579de04195ac1b7612b398f5dc8d558c17eac83051b6f55b488b8877` |
| `lib/knowledge/knowledge-index-store.ts` | `eca060010a31ffb6acac5e349fd6a8a72875c59db673bc797ce04e7ddb0862d6` |
| `lib/knowledge/ann-index-store.ts` | `d18aace5c86d100e37e2f5932eb184044daabc4dec503800ec43a37264848404` |
| `lib/knowledge/vector-search-backend-factory.ts` | `54aeefa97965fe574859085616cc74070405489cce5557c2f29abb406fb2d74d` |
| `rust/crates/lingxi-adapters/tests/r06_ar_f12_cache_preservation.rs` | `f9790bf638769199b40af3b540cb34d2fa897c28e91b4cec0af91f9b09fe4b9b` |
| `rust/crates/lingxi-service/tests/r06_ar_f12_cache_consumers.rs` | `bd4b3d7b711e20aac3e56e4ca87b632bb804e4f265eedd0bfd2be218ad0e17e0` |
| `tests/r06-ar-f12-vector-cache-preservation.test.ts` | `5eb1a93524ce4eabf542229f7c68e960b37b8062f908eaf29596721b8908b773` |
| `tests/r06-ar-f12-fulltext-cache-preservation.test.ts` | `9973a3161d4d4bf7cf7ed4f079fdd45b22b1616be69d7408c1054f2bebbd6f59` |
| `tests/r06-ar-f12-ann-cache-preservation.test.ts` | `9b0b9219934632ff24af69e3a759ea7c35feffad5747a2114a066983f23ac791` |

主仓与最终副本逐11 SHA相同；集合`332d136324fe20b0b70f9ed326c9d32217b3b51132057563ffa97185b1e0d154`。保护用户.vscode/settings.json SHA `0f72eb0a2549c95973c2aa8abe54c9aa44491fb9ca2a1c6e437404655d36b3d3`；未写W02/W06 owned、Cargo/lib.rs/migrations/根控制meta及他人测试，最终副本无W02/W06产品。前一format01候选与父控binding均另存，未覆盖原证据。

## 影响STALE、未验证与恢复条件

- W12-E01及W12-R01旧11候选结论不适用于当前hash
- format01四C/早期main成绩不作为当前最终通过
- 先前知识service consumers/A13/A14对应hash证据随此次变化过期，已在final02限定份额亲重验
- 后续F09/F10/F11/F13/EX08改相同写链将使当前相应C/消费者/引用回归STALE，需重跑

仍开放：F09批次/费用/进度与真正续嵌；F10动态embedding身份与同维度换模型；F11取消/归属/usage/迟到写；F13真实聊天自动scope；EX08删除生命周期、事实purge文件错误及跨库原子性；F14正式门禁/迁移登记；全部R00完整拒绝矩阵/refresh changed-unchanged等完整原叶。

未执行：全新独立ReviewerR02；Windows/Linux原生；真实供应商/付费/外发；真实用户数据与旧资料迁移；物理断电；全部R02-R05正式闭包；R06正式完整门禁。R02-R05调用的部分真实路径被回归使用，但全正式前序闭包未重验。registry-01实际F14红（migrations9/registry7，v8/v9缺项）保留交给其归属，未扩大本项改写来换绿。

所有外部替身和测试child由真实父进程等待或随测试运行结束；无隐藏后台。只合成资料、仅loopback，不使用真实用户资料/真实付费供应商/外发。后续需全新ReviewerR02核固定候选；若同族产品再变化，应按上述STALE重验，不复用本修复者独立审查。

**最终交接：READY_FOR_FRESH_REVIEWER。修复者至此结束，不提交/推送，不自签独立PASS。**
