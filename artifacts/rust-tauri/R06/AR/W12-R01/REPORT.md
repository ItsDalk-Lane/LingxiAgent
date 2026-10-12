# R06-AR-W12 / F12 独立对抗性验收 R01

VERDICT: **FAIL**。

固定候选的原“future schema打开不删库”问题已真实改好，权限/锁/I/O失败、限定损坏恢复、源A删除保留源B也通过。但旧句柄仍有两条写入旁路：Rust全文状态更新遗漏版本闸口，TS向量检索的使用时间刷新存在检查到写入窗口。二者均用真实SQLite复现，阻断F12-C04与修复条目4。没有实现修复、修改产品或既有测试、Git写入、用户设置写入。

本Reviewer首次接本项，没有参与实现或此前审查；只使用原Task、完整规格、固定源码、实际diff/生产消费者和自己运行的结果。W12-E01报告作为线索，不能替代结论。结束本项后不复用。

## 固定候选与运行边界

基线/HEAD：`2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d`。候选11文件汇总SHA256：`b20c5613e62946201cc148223ac8dc76cf1beee318c0b0c4869203ca1ed71a16`。开始与结束逐文件、bytes、base SHA及Cargo.lock亲核一致，见 `binding-check.json` 与 `final-binding.json`。候选未包含W02/W06产品；Cargo.lock SHA256 `d4f8c2977adbde14dd42e670205f1054fa7ef1ddceb9f0b13c190c2bfa9d6fd4` 与基线相同。

最终Rust运行只用独立副本target，cargo1.98.1、--locked、-j8，8GiB/600s guard开启并有实际armed日志。早先shared-target启动的成绩保留为SUPERSEDED，不作最终依据。Darwin arm64，Node24.16.0。每个命令独立合成LINGXI_HOME；旧回归显式ServiceConfig或os.tmpdir也是合成目录，未触真实用户home。外部响应仅loopback embedding替身；真实存储、服务bootstrap/认证HTTP、工具网关、查询及消息/引用组装均未mock。5个实际test executable SHA绑定在final-binding；真实服务入口在这些测试进程内运行，不以共享service binary代替。

## 两项确证阻断（一次汇总）

### W12-R01-F01：Rust全文状态写入仍可改future库

违反：F12必须修复第4条旧版本拒写规则、F12-C04、原T07-Step4 schema兼容。位置：`rust/crates/lingxi-adapters/src/storage/knowledge.rs:2265-2272`；真实消费者：`rust/crates/lingxi-service/src/knowledge.rs:1816-1819`。

复现：生产KnowledgeIndexStore::open创建旧句柄及variant，另一真实SQLite连接将user_version改999，旧句柄set_variant_status("failed")。期待KNOWLEDGE_SCHEMA_NEWER且状态不变；实际返回Ok(())，状态building→failed，schema仍999。`counter-old-status.log`：exit101，1个契约断言失败，guard实际开启。这不是编译失败或0匹配。

根因：此公开写方法仍直接conn.execute；其它写方法已经用cache_write即时事务及同事务版本检查，唯独它未接入。原专属旧句柄测试也没有调用此方法。生产摄入replace_artifact_chunks因future版本失败后，错误收尾仍调用它，可以覆盖新格式的索引状态并影响后续可检索状态。

全部已找到同族：KnowledgeIndexStore::set_variant_status及ingest_job失败收尾。扫描其余Rust两缓存写路径未发现另一个同类漏门；事实库其它业务写入/EX08不是本次缓存旧writer关闭范围。

修复要求：状态写入采用同一cache_write事务内检查；加入这个旧句柄和真实摄入失败消费者检查，失败收尾不要把版本拒绝事实吞掉。必须重跑C01—C04、两缓存adapter家族、KnowledgeService摄入失败/删除消费者、A13/A14。

### W12-R01-F02：TS向量检索使用时间更新没有同事务版本保护

违反：F12必须修复第4条、F12-C04、原T07-Step4。位置：`lib/knowledge/vector-index-adapter.ts:1107-1122`。消费者：`knowledge-query-service.ts:1255,1263`普通/精确chunk检索，`usearch-vector-backend.ts:354`portable回退按variant查询。

复现：生产PortableVectorIndexAdapter.buildOrReplaceArtifact建立真实向量，search先通过真实assertWritableSchema。仅在实际prepare(UPDATE)前加确定性调度屏障，让另一真实SQLite连接升级schema999，再继续原prepare/run。没有mockSQL、返回行或存储，也没有改变候选文件。期待future记录不变；实际last_used_at从2020改2040，schema999，仍返回1条正常命中。`counter-query-race.log`：exit1，1个契约断言失败。

根因：刷新是隐藏写入；版本检查和UPDATE不在同一即时事务。其它公开写方法已改为同事务闸口，该刷新遗漏，catch还无差别吞版本不兼容。它影响所有portable搜索及ANN fallback的使用时间/保留期数据，不能以“只是查询”略过。

修复要求：刷新统一使用即时写事务并在其中复核版本；schema-newer单独处理，避免静默正常结果掩盖拒写/降级状态；加入升级窗口反例。必须重跑C01—C04、TS全文/向量/ANN及fallback/查询/配置/保留期家族、A14。

## 四C与真实运行

| 检查 | 独立结果 | 事实与证据 |
|---|---|---|
| C01 schema过新保留 | PASS（生产open范围） | Rust两缓存与TS全文/向量/ANN均拒绝schema999；live及崩溃WAL原件保留。DB/WAL字节不变，崩溃SHM按易失索引仅断言存在/长度；adapters-isolated.log、ts.log、old-new-green.log |
| C02 权限/锁/I/O | PASS | 真实chmod、SQLite排他/写锁、独立子进程RLIMIT_FSIZE，分类错误且保留原件；adapters-isolated.log、ts.log |
| C03 限定损坏恢复 | PASS | Rust自有静止损坏隔离且只重建目标缓存，事实/原文/另一缓存不变；未知归属、有sidecar拒绝；隔离后I/O失败仍有唯一原副本。TS显式reset/rebuild先同步一致备份/收据，不自动付费embedding |
| C04 并发/新旧版本 | FAIL | 原跨进程guard、持写锁、重启、多个旧写方法检查绿；独立补出的状态写入及查询刷新升级窗口红 |

亲跑：adapter原9 + 专属19注册通过（专属含3仅子进程环境启用的helper；16实际父反例，其child在父例中真正运行）；服务2新增删除消费者 + 原11真实HTTP流程 + 原6网关工具通过；TS16 suites/83 tests通过（两次13/73与3/10互不重复）。typecheck三套通过，隔离候选adapters/service --all-targets clippy -D warnings通过，cargo fmt --all --check通过，git diff --check通过。通过数量不能覆盖上述缺失入口。

所有命令/起止时间/home/exit/count/log SHA在results.json commands及各同名json；binary/候选绑定在final-binding.json。C-ID没有0匹配，guard没有关闭或触发后冒充PASS。

## 旧红确为行为契约失败

独立Rust：固定base的实际storage全文通过git show读入内存，原模块未改，仅放入old模块并接探针main；使用锁定依赖编译运行。两缓存生产open返回Ok，schema999降为1，sentinel表消失。old-rust-red-02.log exit101，实际两库契约红。初次探针漏getrandom extern导致编译失败，old-rust-compile.log保留并记PREPARATION_FAILURE；未产生程序的old-rust-red记NOT_RUN/test_count0，绝不计旧红。补齐真实依赖后编译exit0才取得上述行为红。

独立TS：registerHooks只从git show固定base加载实际vector/fulltext/ANN代码；无源码副本写进证据树。old-red.log exit1：vector删除付费哨兵并降版本3，fulltext/ANN拒绝前已改变future文件字节，reset更换inode。相同探针加载候选old-new-green.log exit0：三库future拒绝、字节/version/哨兵保持，reset inode保持。

实现者旧红日志也有真实运行及非零行为断言，与本轮一致；本结论以本轮旧红/候选绿为主。

## 原T07条款、交付物、A13/A14与R00到期叶

原T07全5 Steps、3交付物、A13/A14已逐项读取并列在results.json original_requirements。Step4因两处版本保护漏写FAIL；Step1完整组合未全重验，只能报本轮真实相关回归；Step2 scope/不恢复研究引擎、Step3维度变化、A13引用和A14不兼容模型均有本轮真实服务证据。KnowledgeService删除消费者本轮通过不等价完整T07验收；索引兼容矩阵因漏入口不能关闭。实际用户资料迁移未运行。

24个D08叶的原acceptance_assertions、亲跑关联测试、状态与剩余在results.json r00_leaves。均标RELATED_SCENARIOS_PASS_NOT_WHOLE_LEAF_ACCEPTANCE，不能用旧implemented/share锚点冒充完整叶PASS。refresh changed/unchanged叶本轮只跑了相关导入安全，完整刷新NOT_RUN；research LEDGER只证明新库不恢复研究表，旧残留兼容按原R08。

F09批次/费用/进度、F10动态embedding身份、F11取消/归属/迟到写、F13真实聊天scope、EX08删除生命周期与事实purge文件错误保持开放，不由F12关闭或改名延期。F14登记检查亲跑仍失败：migration9条、登记7条，v8/v9缺失（r02-registry.log exit1）；这是既有其它任务问题，不修改登记骗绿。

## 逐文件独立摘要

- `rust/crates/lingxi-adapters/src/storage/knowledge.rs`：增加分类错误、只读future预检、自有/静止损坏隔离、恢复锁及缓存写事务；状态写入漏闸口见F01。事实库只改open版本门及列出全部来源解析版本。 SHA256 `e93ce9607943b05b32bf10addfadb47e27abbcc44d2580df55e864d469964212`。
- `rust/crates/lingxi-service/src/knowledge.rs`：delete_source只清目标来源全部解析版本，传播缓存清理错误；不再全库reset。原文purge错误和跨库竞态归EX08/F11，本项不关闭。 SHA256 `3f12c23d4cc3142943a67acd86671b1d2cadaf51f657ca8fcf98f1e3eddd1944`。
- `lib/knowledge/vector-index-adapter.ts`：future只读预检；open错误保留；显式rebuild事务内同步完整副本后清向量；多写方法加版本门，search使用记录更新漏事务见F02。 SHA256 `d2902e8b6141a5232e2683c33f998cbeb8c1d596da4cba23709bbd8285f822e5`。
- `lib/knowledge/knowledge-index-store.ts`：open错误不删除；版本闸口覆盖写入口；reset同步备份后同库清派生表；SQL异常区分避免误触发损坏重建。 SHA256 `eca060010a31ffb6acac5e349fd6a8a72875c59db673bc797ce04e7ddb0862d6`。
- `lib/knowledge/ann-index-store.ts`：构造先只读检查future与文件类型；写方法即时事务验版本；不删除catalog。 SHA256 `d18aace5c86d100e37e2f5932eb184044daabc4dec503800ec43a37264848404`。
- `lib/knowledge/vector-search-backend-factory.ts`：去掉ANN构造失败后任意rm数据库/sidecar；保留并明示回退。 SHA256 `54aeefa97965fe574859085616cc74070405489cce5557c2f29abb406fb2d74d`。
- `rust/crates/lingxi-adapters/tests/r06_ar_f12_cache_preservation.rs`：19注册含3仅子进程环境启用helper，16实际父场景；future/WAL/chmod/锁/RLIMIT/损坏归属/重启/跨进程guard/旧句柄覆盖；漏set_variant_status与TS搜索刷新。 SHA256 `8ae031904f055eff2a444b5c693dfee8872780a350f1ef4141fac1fe11c0b42b`。
- `rust/crates/lingxi-service/tests/r06_ar_f12_cache_consumers.rs`：2真实删除消费者：A全部解析版本清理、B缓存与原文保留；future写错误返回且原文可读。 SHA256 `2b66d17412719eb907e35bb0698d91477c5b32b9097ddf0014840b654bac4c6f`。
- `tests/r06-ar-f12-vector-cache-preservation.test.ts`：9真实portable/SQLite检查，含future、活跃/崩溃WAL、备份失败、旧句柄；未包含search刷新升级窗口。 SHA256 `51f15be7b7f25ad8e74ad4ed056c3da1ad8273c216959f094d11c105bf3efd46`。
- `tests/r06-ar-f12-fulltext-cache-preservation.test.ts`：6真实全文SQLite检查，含live inode、同步副本、future旧句柄、未知损坏、锁/权限、崩溃WAL。 SHA256 `9973a3161d4d4bf7cf7ed4f079fdd45b22b1616be69d7408c1054f2bebbd6f59`。
- `tests/r06-ar-f12-ann-cache-preservation.test.ts`：4真实catalog/factory检查，含future/WAL/旧句柄/未知损坏显式回退。 SHA256 `9b0b9219934632ff24af69e3a759ea7c35feffad5747a2114a066983f23ac791`。

## 影响、限制与交接

本次没有修改R02—R05的核心状态机、凭证、tool gateway或run迁移。A13经过真实R04网关和R05模型链，但不等价全前序闭包。W02/W06与本候选隔离，不借本结果关闭它们。F01/F02修复将改变storage/query实际产品与测试，当前受影响证据必须STALE，并由全新执行者修复及全新Reviewer重新验收。

NOT_RUN：Windows/Linux原生、真实供应商/付费、真实资料迁移、物理断电、全R02—R05闭包、R06正式整体门禁、全部R00完整拒绝矩阵。没有真实外界阻塞，因此结论FAIL而非BLOCKED。所有运行/child已经结束；合成fixtures保留可复核，无隐藏孤儿进程。只新增指定证据中的探针/日志/摘要/报告，不复制源码仓库/.git/缓存，不触用户settings。

本项阻断集合为W12-R01-F01、W12-R01-F02。审查完成后结束本代理。
