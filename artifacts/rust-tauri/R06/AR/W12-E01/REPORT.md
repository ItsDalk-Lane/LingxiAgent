# R06-AR-W12 / F12 一次性执行报告

状态：`READY_FOR_INDEPENDENT_REVIEW`。本机合成环境 F12 两层自检通过，独立验收未执行；原 R06-T07 / 整个 R06 不据此宣布 PASS。

本次修复使旧程序、权限失败、锁和 I/O 故障无法借“缓存可重建”删除数据库；真损坏只在有归属、静止证据时隔离。删除一份资料只清它自己的所有解析版本，不会清其它来源的全文索引或付费向量。整个执行没有 Git 写操作、真实用户数据迁移、外发或付费调用。

## 候选与运行边界

基线与当前 HEAD 均为 `2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d`。用户 `.vscode/settings.json` 原件未修改。主工作区含总控、W02、W06 并行改动，最终产品是下面 **11 个 owned 文件**；独立 Reviewer 必须只复制这些文件到该基线。未修改 Cargo、lock、kernel、lib.rs、migrations 或原测试。W06 修改的 YAML 依赖不被此候选使用。

平台 Darwin arm64；cargo `/Users/study_superior/.cargo/bin/cargo` 1.98.1、`-j8`、共享 `rust/target`；Node 24.16.0、npm 11.13.0。资源守卫保留。每次测试命令新建合成 `LINGXI_HOME`；专属 fixture 强制读取该环境并在其内建 SQLite；系统临时目录别名测试及原回归的 os.tmpdir 也仅合成库。生产 SQLite/存储/服务被实际调用，未 mock 掉被测对象。服务 A13/A14 供应商仅本地固定 embedding 与 loopback 替身；没有真实供应商或 LIVE 证明。

候选汇总 SHA256（按 results.json candidate 顺序 `path + NUL + sha256 + LF`）：`b20c5613e62946201cc148223ac8dc76cf1beee318c0b0c4869203ca1ed71a16`。

| 文件 | SHA256 |
|---|---|
| `rust/crates/lingxi-adapters/src/storage/knowledge.rs` | `e93ce9607943b05b32bf10addfadb47e27abbcc44d2580df55e864d469964212` |
| `rust/crates/lingxi-service/src/knowledge.rs` | `3f12c23d4cc3142943a67acd86671b1d2cadaf51f657ca8fcf98f1e3eddd1944` |
| `lib/knowledge/vector-index-adapter.ts` | `d2902e8b6141a5232e2683c33f998cbeb8c1d596da4cba23709bbd8285f822e5` |
| `lib/knowledge/knowledge-index-store.ts` | `eca060010a31ffb6acac5e349fd6a8a72875c59db673bc797ce04e7ddb0862d6` |
| `lib/knowledge/ann-index-store.ts` | `d18aace5c86d100e37e2f5932eb184044daabc4dec503800ec43a37264848404` |
| `lib/knowledge/vector-search-backend-factory.ts` | `54aeefa97965fe574859085616cc74070405489cce5557c2f29abb406fb2d74d` |
| `rust/crates/lingxi-adapters/tests/r06_ar_f12_cache_preservation.rs` | `8ae031904f055eff2a444b5c693dfee8872780a350f1ef4141fac1fe11c0b42b` |
| `rust/crates/lingxi-service/tests/r06_ar_f12_cache_consumers.rs` | `2b66d17412719eb907e35bb0698d91477c5b32b9097ddf0014840b654bac4c6f` |
| `tests/r06-ar-f12-vector-cache-preservation.test.ts` | `51f15be7b7f25ad8e74ad4ed056c3da1ad8273c216959f094d11c105bf3efd46` |
| `tests/r06-ar-f12-fulltext-cache-preservation.test.ts` | `9973a3161d4d4bf7cf7ed4f079fdd45b22b1616be69d7408c1054f2bebbd6f59` |
| `tests/r06-ar-f12-ann-cache-preservation.test.ts` | `9b0b9219934632ff24af69e3a759ea7c35feffad5747a2114a066983f23ac791` |

## 正对照、旧红与根因全链

原Rust生产链 `bootstrap → KnowledgeService::open → KnowledgeIndexStore::open / KnowledgeVectorStore::open → try_open/check_user_version`：任何 Err 都 remove DB/WAL/SHM 后重新打开。基线正常打开/重开 `positive-old.log` 1/1 绿（exit 0），基线 schema 999 + 哨兵 `red-old.log` 行为失败（exit 101、1 个实际失败）：返回成功而非拒绝，旧红不是编译失败/零匹配。此二库循环在第一个全文库失败，不能声称原红同时跑完两库；新绿实际遍历两库。

现役TS `PortableVectorIndexAdapter.constructor → openWithRecovery → catch → deleteFiles → open` 任意错误删库；`ts-red-old.log` 正常 1 绿、future 哨兵 1 红（exit 1，构造竟不抛）。TS全文 `KnowledgeIndexStore.openWithRecovery` 原先确有 CORRUPT/NOTADB/quick_check 白名单，**不是任意错误都 unlink**；但版本检查前设置 WAL，确认损坏与 reset 仍裸删活跃 DB/WAL/SHM。`fts-red-old.log` 正常 1 绿、reset 保活跃 inode/恢复副本 1 红，实际 inode 被换。ANN `factory → AnnIndexStore.constructor → catch → rmSync(DB/WAL/SHM) → rebuild` 同族任意错误；`ann-red-old.log` 正常 1 绿、future schema 哨兵 1 红，原件字节被改变。四组真实旧红与对应同名新绿构成等效根因负向对照；未做虚假的编译红变异。

消费者 Rust `delete_source → index.reset` 原本吞错误并全清索引，vector remove 也吞错误；现在错误传播，按 source 的全部 snapshot/artifact 清理。`list_source_parse_artifacts` 的 prepare/query/collect 全部 `?` 传播；不以空列表掩盖读取失败。新增消费者测试验证删 A 两个版本、保留 B 的索引、真实向量命中与原文；缓存 version 改成 999 后删 A 返回拒绝、事实 source/原文与三条向量保持。

## 恢复及兼容矩阵

| 输入/窗口 | Rust两库 | 现役TS全文/portable/ANN |
|---|---|---|
| 正常新建、重开 | 正常建目录/缓存与 owner；通过 | 正常迁移与现有业务回归通过 |
| future schema、live/crashed future WAL | 只读探针拒绝；写事务再验版本，旧句柄也拒写 | 版本探针早于 WAL/迁移；旧句柄事务再验；ANN明确 portable fallback |
| 权限/锁/磁盘 I/O | 分类 permission/busy/io 响亮错误；保原件 | 保原件，原始 cause 可查；不会当损坏自动清库 |
| 明确损坏且 owned、无sidecar、无活跃适配器 | shared guard 升 exclusive，身份再验，readonly quickcheck，唯一隔离目录保原 DB 后创建空缓存；明确 reindex required、不重付费 | 没有足够的跨实例静止证明，构造明确拒绝保留；ANN显式fallback |
| 未知文件、symlink/hardlink、sidecar、活跃guard | 拒绝不隔离，不删任何 sidecar | 构造拒绝；不裸unlink；ANN保目录后fallback |
| 显式 reset/rebuild | 全文事务清派生表，同 inode | SQLite immediate 事务挡写者，serialize 保存包含提交 WAL 的完整 DB，fsync DB/receipt/Unix目录，再在同 live DB 清表；备份失败回滚不清 |

原文 facts、snapshots、artifacts 与其它 paid vectors不属于恢复目标。Rust隔离副本即使新库初始化遇到真实 I/O 失败仍保留；不宣称恢复了损坏的付费向量。TS副本 receipt 明示 `snapshot_retained_reindex_required / embeddingsRegenerated=false`。SHM 是SQLite易失读锁/索引，crashed-WAL只读探针可能更新读锁字节；最终断言 DB 和 WAL 持久字节相同、SHM仍存在且长度相同、schema/哨兵可读，不能声称 SHM所有字节不可变。

macOS普通启动回归曾真实失败：NOFOLLOW 遇到 `/var` 系统目录别名（W02 exchange-attempt1 5 个启动失败；本单 self-check-03 原9回归8失败）。已规范化可信数据库父目录，再用原文件名 + SQLite NOFOLLOW；没有移除链接保护，也没有仅改测试 home 掩盖。专属 raw system-temp alias 正对照和最终服务 bootstrap 回归均通过。

## 第一层逐项自检

| C-ID | 期待 | 实际及证据 | 状态 |
|---|---|---|---|
| R06-AR-F12-C01 | 两个缓存 future schema 哨兵；live/crashed WAL及旧句柄拒写 | 两库返回 KNOWLEDGE_SCHEMA_NEWER；DB/WAL不变，SHM存在且长度不变，哨兵可读；TS三库版本闸口。；`adapters-candidate-02.log` / `ts-family-final.log` | SELF_CHECK_PASS |
| R06-AR-F12-C02 | 真实chmod不可写目录、SQLite锁、受控I/O错误，不删除 | 真实 chmod/BEGIN EXCLUSIVE/RLIMIT_FSIZE；permission/busy/io分类，文件原件保留；TS备份失败不清表。；`adapters-candidate-02.log` / `ts-family-final.log` | SELF_CHECK_PASS |
| R06-AR-F12-C03 | 独立明确损坏缓存受控隔离，不伤事实原文及其它paid vector；失败留唯一副本 | Rust拥有归属+无sidecar+独占guard才隔离重建；损坏unknown/sidecar拒绝；重建I/O失败隔离原件仍在；TS自动物理恢复拒绝，显式reset/rebuild先WAL一致快照并fsync。；`adapters-candidate-02.log` / `ts-family-final.log` | SELF_CHECK_PASS |
| R06-AR-F12-C04 | 另一实例/旧版本不删除活跃DB，重启正常 | 真实子进程适配器持shared guard阻止隔离；writer事务阻止TS重建；旧句柄逐写事务验版本；normal reopen绿。；`adapters-candidate-02.log` / `ts-family-final.log` | SELF_CHECK_PASS |

| 原Step/交付/A-ID | 期待与本轮实际 | 状态 |
|---|---|---|
| Step1 | 实测原R00范围全部资料流程；原adapter9、service11、tools6相关回归重跑；新增source删除2实际消费者；并未重验全部旧客户端/R00拒绝分支，F09-11/EX仍开放。 | RELATED_REGRESSION_PASS_FULL_TASK_OPEN |
| Step2 | 本地向量与scope保留，不恢复研究引擎；A13、research tables absent、原向量/ANN回归；损坏fallback实际portable返回同paid向量命中。 | RELATED_REGRESSION_PASS_FULL_TASK_OPEN |
| Step3 | embedding身份、维度、版本不混查；A14维度不兼容与settings variants原回归绿；本单专治cache schema，F10身份reload根因未关闭。 | RELATED_REGRESSION_PASS_FULL_TASK_OPEN |
| Step4 | fallback/持久化/取消/重启/schema/original保护；本单C01-C04与真实open/delete消费者通过；未解决F11取消/usage及EX08 purge收据。 | F12_SELF_CHECK_PASS_OTHER_OBLIGATIONS_OPEN |
| Step5 | 冲突路径如实登记，未决阻断完整验收；F09-F11/F13/EX08/F14及原R00未覆盖拒绝枝保留，未称T07整体PASS。 | UNRESOLVED_RECORDED |
| R06-A13 | 允许范围结果与引用定位实际原文；原3个A13专属tools测试+citation/service/分页回归绿；真实聊天scope未证明，`service-regression-final.log` | LOCAL_RELATED_REGRESSION_PASS_F13_OPEN |
| R06-A14 | 不兼容模型/维度不伪造正常检索；原A14服务测试+adapter维度拒绝+settings variants绿；F10动态身份未关闭，`service-regression-final.log` | LOCAL_RELATED_REGRESSION_PASS_F10_OPEN |
| KnowledgeService | delete_source真实限定清本source全部artifact版本，错误传播；`service-regression-final.log` | RELATED_DELIVERABLE_UPDATED_NOT_WHOLE_T07_ACCEPTANCE |
| 索引兼容矩阵 | REPORT恢复/拒绝矩阵，schema读取与写事务闸口；`adapters-candidate-02.log; ts-family-final.log` | RELATED_DELIVERABLE_UPDATED_NOT_WHOLE_T07_ACCEPTANCE |
| 引用与迁移测试 | A13/A14回归实测；真实用户迁移NOT_RUN；`service-regression-final.log` | RELATED_DELIVERABLE_UPDATED_NOT_WHOLE_T07_ACCEPTANCE |

原 R00 的24叶（含 research 兼容残留）逐叶期待、实际测试名、证据和未闭环项完整列于 `results.json:r00_leaves`，期待直接来自现行 LEAF_MAP acceptance_assertions；没有沿用其旧 implemented 当本轮 PASS。下表是同一逐叶摘要，**均非完整叶验收**。

| 原R00叶尾标 | 实际证据 | 未闭环 |
|---|---|---|
| LEDGER | research_engine_tables_are_not_restored; research_engine_tables_are_absent_from_the_new_store | 兼容research残留，不恢复研究引擎；不等价真实旧库迁移验收 |
| 01E1E3 | notebook_crud_settings_and_ingestion_summary | EX08生命周期/清理全语义未闭环 |
| 3D9932 | file_import_security_rules; import_pasted_text_parses_chunks_and_ingests_with_stub_embeddings | 目录导入/解析器全矩阵未重验 |
| 763C55 | notebook_crud_settings_and_ingestion_summary | 原全部失败分支未逐一重验 |
| E84EEE | notebook_crud_settings_and_ingestion_summary | 原全部失败分支未逐一重验 |
| 210C79 | import_pasted_text_parses_chunks_and_ingests_with_stub_embeddings | F09/F11/导入类型全矩阵开放 |
| B0386D | notebook_crud_settings_and_ingestion_summary | 原全部失败分支未逐一重验 |
| 66C538 | notebook_crud_settings_and_ingestion_summary | 原全部失败分支未逐一重验 |
| BB1F5E | notebook_crud_settings_and_ingestion_summary | 不可达状态等EX语义未闭环 |
| E2AF86 | citation_resolution_returns_block_snapshot_source_and_viewer; a13_citation_markdown_resolves_to_actual_text | 真实聊天F13仍开放 |
| 76E2F5 | notebook_crud_settings_and_ingestion_summary; ingestion_without_model_lands_pending_embedding_then_completes_after_reboot | unreachableSources恒空；F09/F11进度/取消义务开放 |
| 8A6247 | reingest_membership_and_delete_semantics | F09/F11摄入恢复/并发未本单关闭 |
| C12CD3 | reingest_membership_and_delete_semantics | EX08生命周期完整拒绝矩阵未重验 |
| DB95BA | file_import_security_rules（仅相关导入安全） | refresh changed/unchanged完整要求本轮未独立运行 |
| F49D5F | settings_change_triggers_rebuild_and_variants_coexist; a14_switching_to_incompatible_embedding_dimension_refuses_vector_channel_until_rebuild | F10模型身份动态变化开放 |
| 251147 | import_pasted_text_parses_chunks_and_ingests_with_stub_embeddings | 全部缺失/身份拒绝分支未逐一重验 |
| 618978 | citation_resolution_returns_block_snapshot_source_and_viewer | 全部身份拒绝分支未逐一重验 |
| 23AB8E | snapshot_content_serves_bytes_with_security_headers | 真实数据迁移未运行 |
| 928337 | r06_ar_f12_cache_consumers 2/2; source_delete_is_blocked_while_turn_scope_freezes_it | EX08 facts.purge_source吞文件remove错误；F11迟到摄入/跨库原子性未本单关闭 |
| 269958 | knowledge_read_pagination_and_grep_outline; scope_violations_are_loud_not_fallback | 完整regex/分页/真实聊天矩阵未重新证明 |
| 1AA3F1 | knowledge_manage_actions_and_gates | 全action失败生命周期矩阵仍由EX08承担 |
| 5F5365 | knowledge_read_pagination_and_grep_outline | 空文档/全错误矩阵未逐一重验 |
| FBD4D4 | a13_citation_markdown_resolves_to_actual_text; knowledge_read_pagination_and_grep_outline | 同轮刷新冻结/恢复完整链未在本轮重验，F13开放 |
| 062844 | a13_search_within_scope_returns_only_allowed_sources; a14_switching_to_incompatible_embedding_dimension_refuses_vector_channel_until_rebuild | 真实聊天自动scope F13开放 |

## 第二层对抗性自检与同族扫描

实际反例包含：合法future schema 哨兵、live writer未提交事务、真实子进程退出留下WAL、生产适配器独立子进程持shared guard、长期旧句柄被外部升级、真实chmod撤写权限、SQLite排他锁、子进程RLIMIT_FSIZE/SIGXFSZ受控I/O、重建在rename后失败、链接及硬链接错误对象、损坏+sidecar、未知损坏身份、源A/B共享库、多解析版本删除、TS备份失败。没有以mock PermissionDenied或mock SQL返回代替实际文件/数据库故障；只在服务供应商边界使用合成embedding。

`family-whole-final.log` / `consumer-family-scan.log` 留原扫描。Rust全文与vector open、全文 reset、service delete消费者已修。TS全文 open/reset、portable open/rebuild与SQL异常分类、ANN构造/factory任意rm已修。query-service 的 INDEX_INVALID reset/rebuild仍委托上述安全事务原语；SQL锁/权限/I/O改为 RETRIEVAL_UNAVAILABLE，避免误触发清缓存。语义损坏/不一致仍可明确请求派生重建，其前置备份确保保留原件。ANN graph临时文件清理与命名variant失效只作用于派生 graph，不属于SQLite open失败重置或付费vector；保持原业务，相关crash/rebuild回归通过。facts KnowledgeStore 构造只关句柄抛错，没有open失败删库；其非缓存schema窗口不本单扩大。backup/agent_memory 的显式恢复/原子临时文件清理不是知识cache open错误同根因，不改W02/W06所有权。

EX08仍开放：Rust `facts.purge_source` 在事实事务提交后 `let _ = remove_file(snapshot)`，不能据本单关闭原生命周期/清理诚实收据。跨三个库的删除部分失败可能产生已删派生行但事实原文仍保留，返回错误而非虚假成功；并发摄入的delete-wins/迟到结果全链属于 F11/EX08。TS manager明确source purge不视作open错误缓存恢复，同样不借本单关闭全部生命周期。

## 最终执行与历史失败

- `adapters-candidate-02.log`：exit 0，原 adapter 9 + 专属 19（其中3个仅在父测试以环境标志启动的child helper；真实父反例16），无过滤。生产跨进程场景的child实际已运行。
- `service-regression-final.log`：exit 0，专属删除消费者2 + 原服务11 + 原工具6；A13/A14真实服务回归。
- `ts-family-final.log`：exit 0，16 suite / 83 tests；专属 vector9、全文6、ANN4，原相关回归64。没有修改旧测试，原suite钉数不变。
- `typecheck-final.log`（及typecheck-family.log）：exit 0，最终候选root/node/test三套tsc。
- `lint-family.log`：exit 0，0 errors / 39既有any warnings；没有关闭规则。
- `clippy-adapters-candidate.log`：exit 0、`-D warnings`。扩围 `clippy-candidate.log` exit101，当前共享service/session_tree.rs:494 的 retry_turn 8参数警告；不是W12路径，不修改/不算通过。
- `fmt-owned-candidate.log`：exit0；`diff-check-final.log`：exit0，无差异空白错误。`fmt-candidate.log`全仓exit1，并行 preview/sessionfiles 两处格式差异；不越界改。此前 `fmt-all-final.log`在当时共享候选绿，不能代替最新失败。
- `r02-registry-final.log`：exit1，迁移9条 vs登记7条、v8/v9未登记，F14开放；不得改注册表骗绿。

全部初版失败保留：self-check-01两例红揭示损坏头+WAL可被SQLite改变；加入只读头检验，self-check-02绿。self-check-03 alias正常启动红，canonical parent修复后04/05/final绿。ts-self-02 crashed SHM严格字节断言红，按SQLite易失锁语义精确验证DB/WAL不变与SHM存在/哨兵逻辑后03绿；未删除持久数据保护断言。fmt-first/fmt-all最初格式红修复后owned绿。adapters-candidate.log exit101是我末轮误写 test target名，**不算行为红**，正确 knowledge_store_r06t07 后candidate-02绿。lint-candidate的复合shell整体exit1来自随后格式命令，lint自身0error。r00-leaves-read初次null数组读取失败，不是产品结果；read-02正确读取24叶。所有日志SHA/退出码在results.json保留，扫描汇总null不是测试通过。

可复核命令（每行独立运行，新合成home，不安装新依赖）：

```sh
LINGXI_HOME=$(mktemp -d /tmp/lingxi-w12-review-XXXXXX) /Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -j8 -p lingxi-adapters --test knowledge_store_r06t07 --test r06_ar_f12_cache_preservation -- --nocapture
LINGXI_HOME=$(mktemp -d /tmp/lingxi-w12-review-XXXXXX) /Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -j8 -p lingxi-service --test r06_ar_f12_cache_consumers --test r06_t07_knowledge --test r06_t07_knowledge_tools -- --nocapture
LINGXI_HOME=$(mktemp -d /tmp/lingxi-w12-review-XXXXXX) npx vitest run tests/r06-ar-f12-*.test.ts tests/knowledge-index-*.test.ts tests/knowledge-section-index.test.ts tests/knowledge-query-index-separation.test.ts tests/knowledge-query-config-alignment.test.ts tests/knowledge-vector-*.test.ts tests/knowledge-ann-*.test.ts
npm run typecheck
```

## 影响、STALE与未验证

F09批次/断点/进度、F10动态模型身份、F11取消/usage/迟到写、F13真实聊天scope、EX08生命周期、F14总门禁/登记、F15最终证据重算都保留开放。旧T07评审把任意打开错误删库列LOW并允许仅补说明的结论已失效；本单升级为实测修复，但不替整个T07/旧叶图自签。总控报告或先前跨任务测例在产品hash变化后应重新绑定；独立Review只准采用本候选11文件。

未运行独立Review、Linux/Windows、真实供应商、真实资料迁移、物理断电、全R00拒绝矩阵、全部R02-R05前序套件与R06正式整体门禁。TS在无法证明确认损坏且静止时明确拒绝并保留；需要线下恢复，不自动继续付费生成。Unix目录同步已实现，Windows目录持久性没有本机证明。Rust协作guard不能约束绕开生产适配器且不遵守guard的任意外部工具；有SQLite sidecar时一律不物理隔离。此限制不把活跃已知写者当可删。

只交付 `READY_FOR_INDEPENDENT_REVIEW`；后续由全新Reviewer追加攻击和独立基线复制验证。本执行者结束本Task，不接下一个Task。
