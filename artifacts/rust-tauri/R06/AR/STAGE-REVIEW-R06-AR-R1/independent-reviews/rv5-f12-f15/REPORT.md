# R06 收口独立复核（RV5）——R06-AR-F12 与 R06-AR-F15

- Reviewer：RV5（全新独立实例，不复用执行者自检结论）
- 基线：`0987f72fb`（含 F15 justified-recovery 补丁）；工作树 `/tmp/lingxi-rv5`（`git worktree add` 自基线）
- 工具链：cargo 1.98.1 `--locked`，CARGO_TARGET_DIR=/tmp/rv5-target；合成环境 /tmp/rv5-env；Node v24.16.0（--experimental-strip-types）
- 结论：**F15 修复面 PASS（本审范围）**；**F12 核心保护语义 PASS，但其冻结受影响族内存在 1 个基线确定性红测试（RV5-F12-01，FAIL）**，F12 不能在修复该项前宣告完全关闭。

## 1. F12（错误分类/缓存保护）亲验

### 1.1 规定主套件
`cargo test -p lingxi-adapters --test r06_ar_f12_cache_preservation`，`LINGXI_HOME=/tmp/rv5-env/f12`：
**22 passed / 0 failed**（含 c01 未来 schema 双库哨兵、c02 chmod/排他锁/RLIMIT_FSIZE、c03 隔离区与 WAL、c04 并发/旧句柄/回滚族）。与执行者申报一致。

补充族内亲跑：
- `-p lingxi-service --test r06_ar_f12_cache_consumers`：**5 passed / 1 FAILED**（见 §1.4 发现）
- TS 三套（vitest）：vector 15 + fulltext 6 + ann 9 = **30/30**

### 1.2 知识面抽查（源码级）
- `rust .../storage/knowledge.rs:289-363`：`open_connection` 先只读探测 `check_user_version`，`KNOWLEDGE_SCHEMA_NEWER` 先于任何写；`storage_sqlite`（:268）按 BUSY/LOCKED→BUSY、PERMISSION/READONLY→PERMISSION、IOERR/FULL/CANTOPEN→IO、CORRUPT/NOTADB→CORRUPT 分类，未命中→INVALID——错误类型区分与 remaining_scope 第 1 条一致。
- `open_cache`（:531-635）：仅 `KNOWLEDGE_STORAGE_CORRUPT` 进入恢复路径；锁升级后重验；有 sidecar→BUSY 保留；无 `.owner`/身份漂移→保留；`quick_check` 只读确认后才隔离（rename 进 `.quarantine-*`，附 owner 副本与目录 fsync），不生成 embedding。与"重建仅限本程序、无活跃写者、确认损坏"一致。
- `cache_write`（:637-646）：`BEGIN IMMEDIATE` 事务内 `check_user_version`——写路径版本闸与写入同事务（W12-R01-F01/F02 修复的机制核心）。
- `complete_build`（:3229-3241）：schema 闸提前于 `checkpoint_entries`，"schema 过新被误分类 INDEX_INVALID"的当日修复在源码与测试中均落实。
- TS `ann-index-store.ts:16-50 openKnowledgeCacheDatabase`：只读守卫先行版本验证；非 WAL 库在 EXCLUSIVE 事务内验版本后才切 WAL——初始化侧写在版本闸之后（W12-R02 修复）。
- 全族 "open 失败→remove/reset" 扫描：rust 侧仅 2 处 `remove_file`（knowledge.rs:1541 与 service/knowledge.rs:1077，均为显式 source/artifact 删除语义）；TS 侧 rm 均为发布/临时文件清理或显式删除；`vector-search-backend-factory.ts` ANN 构造失败仅回退 portable 并记 `ANN_CATALOG_UNAVAILABLE`，不删除。未发现同族残留。

### 1.3 三个历史 blocker 亲跑定论（全部反转通过）
| Blocker | 亲跑方式 | 结果 |
|---|---|---|
| W12-R01-F01（旧句柄 set_variant_status 写 future 库） | 原始 counter-old-status.rs 重编译于基线 rlib，独立 LINGXI_HOME | **PASS**：`Err(KNOWLEDGE_SCHEMA_NEWER)`，before==after=="building"，version 保持 999 |
| W12-R01-F02（search 时间戳 check-then-write 竞态） | 新探针 counter-query-race-v2.mjs：在写事务内、版本断言刚通过后由第二真实连接尝试升级 | **PASS**：升级尝试得 `SQLITE_BUSY`（写锁持有，窗口原子闭合），version 保持 3，last_used_at 合法刷新；即使升级落入窗口，`KNOWLEDGE_SCHEMA_NEWER` 不在 best-effort 吞噬名单（vector-index-adapter.ts:1111-1115 只吞 SQLITE_*），会响亮抛出 |
| W12-R02-INITIALIZATION-SIDE-WRITE（TS 三库构造期 journal/WAL 先于版本闸） | 原始 probe-open-window-v2.mjs 移植到基线路径 | **PASS**：vector/fulltext/ann 三库全部 `KNOWLEDGE_SCHEMA_NEWER`、主文件字节逐 offset 不变（changedOffsets=[]）、journal 保持 delete、version 999、哨兵在 |

注：F02 复核用调度屏障落在 `assertWritableSchema` 之后（写事务内部），即原反例声称的精确竞态窗口；修复机制为"版本断言+UPDATE 同 IMMEDIATE 事务"，第二连接在窗口内无法落地升级。

### 1.4 发现 RV5-F12-01（FAIL）：`r06_ar_f12_cache_consumers` 基线确定性红
- 现象：`vector_ingestion_provider_failure_records_cleanup_version_rejection` 于基线 4/4 次运行同一断言失败（r06_ar_f12_cache_consumers.rs:365：job.error 须含 `SYNTHETIC_PROVIDER_ERROR`；同块还要求 `cache status unchanged`）。全套 5/6。
- 实测 job.error：`KNOWLEDGE_SCHEMA_NEWER: knowledge schema version 999 is newer than supported 1; variant failure status unavailable: KNOWLEDGE_SCHEMA_NEWER: ...`——**供应商错误主因被清除**。
- 根因（当前源码亲读）：`rust/crates/lingxi-service/src/knowledge.rs:2598-2601`，embed 失败路径 `self.vectors.fail_build(&build.variant_id)?;`——清理写被 future-schema 拒绝时，`?` 把清理错误**替换**为主错误返回，原始 provider 错误丢失。场景时序（测试 handler `fetch_add` 返回旧值）：call1=首批 embed 成功并写入 64 行；call2=升级 999+502→provider 错误→fail_build 拒写→错误被替换。与 `process_job` 尾部（:2334-2342）"append 而不替换"的组合哲学不一致。
- 定位：该文件自 bulk 提交 `6f5c4b01d` 后仅 `844a10b2b` 改动过；W12-P01 证据中该套件三次运行为 101/101/0（最后一次绿），其候选 service/knowledge.rs sha `3f98131c…` ≠ 基线 `408ee573…`。数据保护语义（字节/状态 building/paid 行保留/版本保持 999）在本场景实测完整——缺陷是**错误可追溯性/分类组合**，非删库或越权写。
- 可操作修复项：(1) `embed_planned_job` 错误路径（:2599 及同族 :2607/:2616/:2657/:2670/:2682/:2687 中"主错误已有、fail_build 仅清理"的分支）不得用 `?` 替换主错误；应保留 provider 错误码/文本并追加清理拒写说明（如 `"; variant failure status unavailable: {cleanup}; cache status unchanged"`），与 :2338 组合口径对齐；(2) 修复后 `--test r06_ar_f12_cache_consumers` 须全绿并留独立运行证据。

### 1.5 remaining_scope 逐条核对
1. 按错误类型区分（schema 过新/权限/锁/IO/可重建损坏）——源码+22 测试族证实，PASS（Rust 双库与 TS 三库）；服务层错误组合缺口见 RV5-F12-01。
2. 重建仅限本程序/无活跃写者/确认可重建——`open_cache` guard+owner+quick_check 闸证实，PASS。
3. 原文事实库与付费向量保护——`c03_owned_corruption_quarantines_only_one_cache_and_preserves_sources_and_vectors`、consumers 套件（通过的 5 项）证实，PASS。
4. 全族 open 失败→remove 排查+迁移/降级窗口旧版本拒写——独立 grep 族扫 + init-window 探针证实，PASS。
5. W12-R01-F01 全文状态写未来库拒写——亲跑 PASS（§1.3）。
6. W12-R01-F02 查询时间刷新同事务版本保护——亲跑 PASS（§1.3）。
（4 项 self_check C01-C04 的核心语义均有亲验证据；唯一红点在服务层错误组合，见上。）

## 2. F15（评测器真实性）亲验

### 2.1 正控
- `cargo test -p lingxi-service --test r06_t08_behavior_eval`，`LINGXI_PRODUCT_DIR=/tmp/lingxi-rv5/lib` + 独立 runtime-root：**13 passed / 0 failed**（BE-01…BE-10 + 3 个 usage 语义测试）。与执行者 battery.log 一致。
- 正向审计（metrics 全量模式，自产 records+facts）：`python3 scripts/rust-tauri/r06_evaluation_audit.py --records … --facts … --root /tmp/lingxi-rv5 --runtime-root /tmp/rv5-env/f15rt` → `exit 0`（"记录与真实证据一致；原调用预算通过；未决义务保留"）。破坏恢复状态后再跑一次仍 exit 0。

### 2.2 BE-04 数值 SQL 独立复算（直读生产 runs.db，不经评测器）
3 行 model_call_usage：succeeded 1 行（40/17，sess_be04a，run completed.with_final）；failed 2 行（30+30 / 11+11，sess_be04b/c，failed.provider_error）。合计 **100/39 = 40/17 + 60/22**；chat∧succeeded=1；三腿 run 终态与记录 `ledgerOutcomeSucceededRows=1`、`modelCallsObserved=3` 完全一致。执行者申报数字复现。

### 2.3 对抗面：3 个自造新变异（非 replay-results.json 既有用例），逐一被拒
| 变异 | 内容 | 审计结果 |
|---|---|---|
| M1 BE-03 自洽恢复件伪造 | 保留件字节篡改 + 磁盘恢复记录 expected.sha256/size + facts 清单摘要三方对齐（自洽伪造） | exit 1：`BE-03 恢复前后双哈希与已知语义内容不符`（已知原文语义层拒收） |
| M2 BE-04 数值漂移（最难层） | record tokens(100→101/41)、facts SQL 行、wire SSE usage 三方一致改为 41，仅真实生产 runs.db 仍为 40 | exit 1：`BE-04 原始生产库与捕获model_call_usage不符`（直读生产库层拒收） |
| M3 BE-01 全 LIVE 冒称 | comparisonMode=REAL_LIVE_PROVIDER_VERIFIED 且 liveProviderExecuted/incumbentEngineExecuted 同时翻 true（比既有用例更强的"标签+旗标齐改"） | exit 1：`BE-01 替身/旧引擎/LIVE口径矛盾` |

另：变异构建期间的两次"误伤型"拒收（跨 home 同号 run 的 SQL 漂移被 BE-10 wire 层拒收；facts/磁盘不一致被"实际文件与最终捕获不符"拒收）同样 exit 1——审计器多层防御均在真实工作。执行者 f15-repair/replay-results.json 的 24 用例（2 正控+22 负控）已读取核对，与本次独立重放互补。

### 2.4 F15 范围界定
本审覆盖 F15 修复候选的两份 owned 代码（r06_t08_behavior_eval.rs + r06_evaluation_audit.py）及其 justified-recovery 补丁的真实性/抗伪造面。W15-R01-F01（完整原范围未满足）等 11 项 findings 中关于**原始业务范围**的部分不在本修复面内、维持矩阵登记（R06_NOT_ACCEPTED）；本审结论仅为该修复候选在其申报范围内的独立验证，不构成阶段 PASS。

## 3. 环境限制与未执行项
- scope 模式负控（含"删候选绑定"）未在本轮独立重放：其五件套冻结输入绑定旧候选（bc2ce51fd 树），当前树哈希必然不符；该面以执行者 replay-results.json（own-candidate-bindings-deleted exit 1）为唯一证据，未亲跑。metrics 模式全部关键层（含最深的生产库比对）已亲证。
- Rust blame 运行（6f5c4b01d 处 consumers 套件）见附录；无论其结果如何，基线红的事实与修复项不变。
- 未执行：Windows/Linux 平台、正式打包、真实供应商 LIVE。

## 4. 总判定
- **F15（本修复面）：PASS** —— 正控 13/13、审计 exit 0、BE-04 数值独立复算吻合、3 个新变异全拒。
- **F12：核心保护语义 PASS（22+30 测试、3 blocker 反转、源码抽查、族扫），但受影响族 1 项确定性红（RV5-F12-01）→ 判 FAIL（可修复：错误组合保留主因），修复并全绿后方可关闭。
