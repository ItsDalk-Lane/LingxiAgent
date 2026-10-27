# R08 T04—T06 存储准备用事实核对

日期：2026-10-21。基线：`86840d1700397d3dd620a41f2b884df4cddd5cc2`，分支 `codex/rust-tauri-migration`。
状态：**PREPARATION_ONLY / NOT_IMPLEMENTED**。本记录只读源码与原始任务书，未运行迁移、未启动服务、未访问真实用户数据、未构建；不能作为 A07—A12 通过证据。仅本报告是新增文件。

## 结论

现有能力足够复用若干底层读写、备份、恢复和局部导入组件，但还没有覆盖全数据类别的 LegacyImporter、分离根切换协调器或新增数据往返命令。原 R01 的回滚演练使用合成 `messages.sqlite`，不是实际 Rust 库，不能继承为 R08 通过。R02 的实际 Rust 库已有 V1—V26，回滚必须带上新业务收据和文件状态，不能只导出聊天正文。

固定责任为 T04（分类/导入/校验）、T05（全局停写/一致快照/采用/切换）、T06（新增数据归档/旧根恢复/再次前滚）；T08 对三者的汇总只消费实际证据。R07 是否准入由另一名独立审查者决定；以下准备不假定其已经通过。不得借此重开 R06。

## 已经存在的真实入口与限制

| 真实位置/入口 | 可复用事实 | 不能据此声称的能力 |
|---|---|---|
| `rust/crates/lingxi-adapters/src/storage/run_store.rs` 的 `RunDatabase::open`、`queue()`、`backup_to` | 真实 SQLite、单写队列、迁移收据校验及全库备份 | 只锁自己的数据库，不阻止旧版本写其它文件；备份一个 runs.db 不是多库共同静止点 |
| `storage/backup.rs` 的 `backup_database`、`restore_backup` | 实际 rusqlite Online Backup API；默认 checkpoint(TRUNCATE)；完整性、哈希、私有权限、未完成文件与正式产物分开 | 不能用活跃 `.db` 的单文件复制替代；单库成功不证明全数据目录备份完整 |
| `storage/migrations.rs` 的 `MIGRATIONS`、`verify_receipts`、`apply_all` | V1—V26 的连续收据及 SQL 指纹；新版本、丢回执、篡改拒绝 | 不得修改已应用 SQL 指纹来得到绿灯；新 R08 表只能追加版本及登记 |
| `storage/session_tree.rs` 的 `SeedSession`、`AppendMessage` | 可保留会话/消息 ID、parent、role、原始 content、entry_type；run_id 可空且有外键；现有 fork 保留消息 ID | `seed_session`/`append_message` 是 INSERT，不是全量导入器/幂等批次；缺 run 不能创建虚构 runs 行 |
| `storage/canonical_message_store.rs` 与 service `history.rs`/`model_history.rs` | 已有统一查询、导出、分支头、旧记录与原始内容投影边界 | R06 测试 `ex11_legacy_unknown_fields_raw_and_opaque_are_preserved_or_explicitly_refused` 直接经 `append_message` 造行，只证明读取，不证明从旧 JSONL 导入 |
| service `sessionfiles.rs::SessionFileService::import_legacy_sidecar`；`POST /lingxi/v1/sessions/{session_id}/files/import-legacy` | version=1 的文件 sidecar；原 `sf_` ID 和 legacy 别名，附件与引用写入正式表 | 目前报告只有 imported/skipped；缺字段、部分坏引用会 continue，不能满足逐附件坏因报告；旧绝对路径须明确重定位到副本，不能继续指向原件 |
| service `observability.rs` 的 `/lingxi/v1/observability/import` 与 `run_store::import_legacy_model_calls` | 只读旧观测库；同 call ID 幂等，冲突事实拒绝；保留 trace/parent_call 映射 | 只是观测域局部导入，不能称全量；需接入统一批次/源哈希/计数/问题报告 |
| service `automation_import.rs::import_current_studio` | 已有只读旧 cron 文件、源摘要、正式作业身份与导入记录；原历史归档而不造 Run | 完整 service 启动会执行该导入并启动后台组件；不得在尚未切换时开放实际调度/外发来做“导入自检” |
| service `legacy_plugins.rs` | 当前只识别旧插件遗留并显式警告 | 警告不是插件数据转换，也不等于插件功能或迁移完成 |
| service `epoch.rs::coordinate_data_epoch_startup` | 同格式印章/journal，损坏拒启；非空无章根拒启；读取并报告高位日志事实 | 无自动采用旧数据、无 R08 升级迁移注册；不能删除 journal 或人工改章绕过拒启 |
| service `main.rs:652` 与 protocol `ContractVersions::R00_BASELINE` | 目前 service 启动仍以固定 `data_epoch=1` 调用 epoch gate | 直接把副本盖 epoch=2 当前二进制也会拒启；必须实现真实、可审阅的版本/采用契约再盖章，不能只改 fixture 印章 |
| service `instance.rs::acquire`、`InstanceGuard` 与 `paths.rs::DataRootLayout` | canonical home、OS 单写锁与实例身份、认证握手定位 | 旧二进制不认识 Rust 锁；不能把 instance.json 存在/消失当成进程已停证据 |
| `core/data-epoch-coordinator.ts::coordinateDataEpochStartup` 与 `shared/data-epoch.cjs` | 原正式采用、过渡 journal、checkpoint 协议、印章写入的可复用契约 | Rust 采用入口尚缺；不能直接写一份 JSON 代替采用/转换/校验收据 |
| `core/data-epoch-checkpoint-provider.ts::createDataEpochCheckpointProvider` | 已注册旧存储按类型枚举、SQLite backup、文件摘要和整单 verify；不单独捕捉 WAL/SHM | 当前旧注册表本身不能覆盖 Rust 所有新增数据；需显式传完整登记或扩展受控 R08 清单，不能悄悄漏项 |
| `core/data-epoch-restore.ts::restoreDataEpochCheckpoint` | 有 journal、字节验证、保留被覆盖内容到 quarantine 的恢复机制 | `assertNoLiveServer` 只查旧 server-info/probe，不检查 Rust OS 锁；当前恢复到本根，不等于 R08 需要的独立旧根默认回滚 |
| `cli/entry.ts` 的 `data diagnose/checkpoints/restore`，`cli/data.ts` | 旧维护 CLI，绕开业务连接、沿真实 epoch 模块执行 | 目前没有 migrate/cutover/rollback/forward 全链命令；通用 data restore 不能冒充新增数据保护与跨版本恢复 |
| `lingxi-storage-inspect <DB> backup <DEST> [STEM]`、`restore-verify` | 可复用的真实库备份/恢复证据命令 | inspector 普通 `Connection::open` 会有 WAL/SHM 等副作用，不宜拿真实原件作纯只读快照证明；仅在合成静止副本使用 |

重要接线：service `lib.rs:1310` 将 `bind_legacy_session_root` 绑定到 `config.data_home`，归档/恢复/删除可能实际移动该根下的旧 JSONL。正式导入后必须只绑定新隔离根的受控材料，不能绑定只读来源根。该事实不要求删除现有生命周期功能。

## 69 类旧存储逐类责任

来源：R00 STORES 与 R01 DATA_COMPATIBILITY_MATRIX，各 69 项，ID 集合相同。策略原样沿用：35 preserve-format、13 regenerable、11 switch-to-new-authority、6 shell-local、4 epoch-mechanism。以下是准备责任，**没有一行代表已执行导入/回滚**。分类为可重建也须有“不复制、重建/暂不可用原因、计数”的收据，不能静默漏掉。shell-local 不迁入业务权威，但 T05 停旧壳、T06 独立恢复、T08 隔离证明必须覆盖。

| store_id | 原策略 | R08 逐类操作责任 |
|---|---|---|
| `agent-authored-records` | `preserve-format` | T04 原格式逐项枚举/计数/摘要/引用保留；T05 共同静止点快照（其中 SQLite 走 Online Backup）；T06 新编辑单独归档、默认恢复旧快照，再前滚验证。 |
| `agent-facts-sqlite` | `preserve-format` | T04 保留原 schema/ID；T05 独立 checkpoint+Online Backup，T06 两侧静止恢复与内容摘要。 |
| `agent-memory` | `preserve-format` | T04：原格式/迁移与梦境 journal/事实库全部保留；目录含 SQLite，T05 必须枚举实际库，不能整目录直接复制热数据库；T06 按默认旧快照恢复、新编辑单独保留。 |
| `agent-phone` | `preserve-format` | T04 原格式逐项枚举/计数/摘要/引用保留；T05 共同静止点快照（其中 SQLite 走 Online Backup）；T06 新编辑单独归档、默认恢复旧快照，再前滚验证。 |
| `agent-profile` | `preserve-format` | T04 原格式逐项枚举/计数/摘要/引用保留；T05 共同静止点快照（其中 SQLite 走 Online Backup）；T06 新编辑单独归档、默认恢复旧快照，再前滚验证。 |
| `channels` | `preserve-format` | T04 原格式逐项枚举/计数/摘要/引用保留；T05 共同静止点快照（其中 SQLite 走 Online Backup）；T06 新编辑单独归档、默认恢复旧快照，再前滚验证。 |
| `character-card-staging` | `regenerable` | T04 明确登记旧缓存不转权威、按现行行为重建；T05 原件保留不删；T06 不将缓存缺失冒充业务数据丢失或恢复成功。 |
| `conversation-map-layout` | `switch-to-new-authority` | T04：转换并逐节点对账，不丢父子/布局身份；不得借换内核重排界面。 |
| `cron-automation` | `preserve-format` | T04：原 cron 配置/历史保留，复用受控导入；T05 切换前禁止新调度生效；T06 不重放已触发任务/通知。 |
| `data-epoch-checkpoints` | `epoch-mechanism` | T04 原件/版本/收据归档；T05 经正式协调器与整单验证采用、最后盖章，journal不可绕过；T06 各根保留自己的元数据且不独立恢复印章。 |
| `data-epoch-restore-quarantine` | `epoch-mechanism` | T04 原件/版本/收据归档；T05 经正式协调器与整单验证采用、最后盖章，journal不可绕过；T06 各根保留自己的元数据且不独立恢复印章。 |
| `data-epoch-stamp` | `epoch-mechanism` | T04 原件/版本/收据归档；T05 经正式协调器与整单验证采用、最后盖章，journal不可绕过；T06 各根保留自己的元数据且不独立恢复印章。 |
| `data-epoch-transition-journal` | `epoch-mechanism` | T04 原件/版本/收据归档；T05 经正式协调器与整单验证采用、最后盖章，journal不可绕过；T06 各根保留自己的元数据且不独立恢复印章。 |
| `deferred-result-state` | `switch-to-new-authority` | T04：归档旧等待状态，明确未决而不自动重试/运行；T06 保留未决收据。 |
| `desk-activity` | `preserve-format` | T04 原格式逐项枚举/计数/摘要/引用保留；T05 共同静止点快照（其中 SQLite 走 Online Backup）；T06 新编辑单独归档、默认恢复旧快照，再前滚验证。 |
| `desk-cover-upload-staging` | `regenerable` | T04 明确登记旧缓存不转权威、按现行行为重建；T05 原件保留不删；T06 不将缓存缺失冒充业务数据丢失或恢复成功。 |
| `desktop-diagnostics` | `shell-local` | T04 不迁入业务库、不共享；T05 先停止旧Electron写者；T06 旧壳在独立根启动且新根不变；生产默认不动。 |
| `desktop-gpu-startup-state` | `shell-local` | T04 不迁入业务库、不共享；T05 先停止旧Electron写者；T06 旧壳在独立根启动且新根不变；生产默认不动。 |
| `desktop-update-channel` | `shell-local` | T04 不迁入业务库、不共享；T05 先停止旧Electron写者；T06 旧壳在独立根启动且新根不变；生产默认不动。 |
| `desktop-win32-install-acl-heal-state` | `shell-local` | T04 不迁入业务库、不共享；T05 先停止旧Electron写者；T06 旧壳在独立根启动且新根不变；生产默认不动。 |
| `desktop-window-version-state` | `shell-local` | T04 不迁入业务库、不共享；T05 先停止旧Electron写者；T06 旧壳在独立根启动且新根不变；生产默认不动。 |
| `device-access-registries` | `preserve-format` | T04：保留 devices/credentials/pairing身份与原权限，未决配对隔离；T05/T06 一致快照，不因导入重新批准设备。 |
| `ephemeral-scanner-scratch` | `regenerable` | T04 明确登记旧缓存不转权威、按现行行为重建；T05 原件保留不删；T06 不将缓存缺失冒充业务数据丢失或恢复成功。 |
| `execution-leases` | `switch-to-new-authority` | T04：旧执行租约按源身份归档/诊断，绝不把旧 prepared/approved 迁成新授权；T05 先证明原执行器停止。 |
| `file-history-sqlite` | `preserve-format` | T04：保留历史内容与文件摘要；T05 逐workspace库 backup；T06 防止恢复新内容覆盖旧工作区。 |
| `knowledge-database` | `preserve-format` | T04：原格式/文档与chunk/引用身份保留；T05 逐库 checkpoint+Online Backup，T06 验真实引用与新增文档可恢复。 |
| `knowledge-indexes` | `regenerable` | T04 明确登记旧缓存不转权威、按现行行为重建；T05 原件保留不删；T06 不将缓存缺失冒充业务数据丢失或恢复成功。 |
| `knowledge-parse-artifacts` | `preserve-format` | T04 原格式逐项枚举/计数/摘要/引用保留；T05 共同静止点快照（其中 SQLite 走 Online Backup）；T06 新编辑单独归档、默认恢复旧快照，再前滚验证。 |
| `knowledge-processing-artifacts` | `preserve-format` | T04 原格式逐项枚举/计数/摘要/引用保留；T05 共同静止点快照（其中 SQLite 走 Online Backup）；T06 新编辑单独归档、默认恢复旧快照，再前滚验证。 |
| `knowledge-source-snapshots` | `preserve-format` | T04 原格式逐项枚举/计数/摘要/引用保留；T05 共同静止点快照（其中 SQLite 走 Online Backup）；T06 新编辑单独归档、默认恢复旧快照，再前滚验证。 |
| `legacy-upload-cache` | `regenerable` | T04 明确登记旧缓存不转权威、按现行行为重建；T05 原件保留不删；T06 不将缓存缺失冒充业务数据丢失或恢复成功。 |
| `local-user-auth` | `preserve-format` | T04/T05：原格式、0600/私有目录完整性；T06 恢复身份不扩大权限；只用合成凭证。 |
| `loop-state` | `switch-to-new-authority` | T04：保留循环历史/计数/原因果；恢复不能重启已执行工具或重复通知；不猜测 active 状态。 |
| `managed-runtime-caches` | `regenerable` | T04 明确登记旧缓存不转权威、按现行行为重建；T05 原件保留不删；T06 不将缓存缺失冒充业务数据丢失或恢复成功。 |
| `mcp-config` | `preserve-format` | T04 原格式逐项枚举/计数/摘要/引用保留；T05 共同静止点快照（其中 SQLite 走 Online Backup）；T06 新编辑单独归档、默认恢复旧快照，再前滚验证。 |
| `model-observability-blobs` | `preserve-format` | T04：原格式逐blob保留并核旧trace引用，不把opaque/签名直接显示；T06 与真实库导出关联。 |
| `model-observability-db` | `preserve-format` | T04：复用旧库只读导入，call/trace/parent和日期保留；旧库原格式归档；T05 全库备份；T06 V24映射往返对账。 |
| `office-render-jobs` | `regenerable` | T04 明确登记旧缓存不转权威、按现行行为重建；T05 原件保留不删；T06 不将缓存缺失冒充业务数据丢失或恢复成功。 |
| `operational-checkpoints` | `preserve-format` | T04 原格式逐项枚举/计数/摘要/引用保留；T05 共同静止点快照（其中 SQLite 走 Online Backup）；T06 新编辑单独归档、默认恢复旧快照，再前滚验证。 |
| `plugin-runtime-data` | `preserve-format` | T04 原格式逐项枚举/计数/摘要/引用保留；T05 共同静止点快照（其中 SQLite 走 Online Backup）；T06 新编辑单独归档、默认恢复旧快照，再前滚验证。 |
| `plugin-task-registry` | `switch-to-new-authority` | T04：旧任务登记与结果映射/归档，未知外部效果保留 unknown；不通过导入重跑插件。 |
| `provider-state` | `preserve-format` | T04/T05：保持原格式与凭证隔离，合成凭证演练，哈希可记但日志/证据不输出秘密；T06 保存新增配置但默认旧快照。 |
| `runtime-diagnostics` | `preserve-format` | T04 原格式逐项枚举/计数/摘要/引用保留；T05 共同静止点快照（其中 SQLite 走 Online Backup）；T06 新编辑单独归档、默认恢复旧快照，再前滚验证。 |
| `security-audit-log` | `preserve-format` | T04 原格式逐项枚举/计数/摘要/引用保留；T05 共同静止点快照（其中 SQLite 走 Online Backup）；T06 新编辑单独归档、默认恢复旧快照，再前滚验证。 |
| `security-grants` | `preserve-format` | T04：只保留原记录与corrupt说明，不把历史授权转换成新批准；T05/T06 私有快照并验证权限不增长。 |
| `security-key-material` | `preserve-format` | T04/T05：按原格式私有保存、绝不输出秘密；T06 原授权/票据生命周期不复活。 |
| `server-network-config` | `preserve-format` | T04 原格式逐项枚举/计数/摘要/引用保留；T05 共同静止点快照（其中 SQLite 走 Online Backup）；T06 新编辑单独归档、默认恢复旧快照，再前滚验证。 |
| `server-node-identity` | `preserve-format` | T04 原格式逐项枚举/计数/摘要/引用保留；T05 共同静止点快照（其中 SQLite 走 Online Backup）；T06 新编辑单独归档、默认恢复旧快照，再前滚验证。 |
| `server-runtime-info` | `regenerable` | T04 不迁为权威或存活证明；T05 认证探测+实际进程/句柄确认停写；新启动重新生成自己的记录。 |
| `session-checkpoints` | `regenerable` | T04 按原 regenerable 分类登记重建；明确与新 Rust 正式 checkpoints 表不同；T06 新表及文件版本必须完整保留。 |
| `session-context-notes` | `regenerable` | T04 明确登记旧缓存不转权威、按现行行为重建；T05 原件保留不删；T06 不将缓存缺失冒充业务数据丢失或恢复成功。 |
| `session-drafts-and-projects` | `preserve-format` | T04 原格式逐项枚举/计数/摘要/引用保留；T05 共同静止点快照（其中 SQLite 走 Online Backup）；T06 新编辑单独归档、默认恢复旧快照，再前滚验证。 |
| `session-files` | `preserve-format` | T04：保留 sf_ ID/别名/引用，受控重定位文件，逐坏附件报告；T05 停写共同快照；T06 新附件与引用真实导入/导出再核。 |
| `session-goal` | `regenerable` | T04 明确登记旧缓存不转权威、按现行行为重建；T05 原件保留不删；T06 不将缓存缺失冒充业务数据丢失或恢复成功。 |
| `session-jsonl` | `switch-to-new-authority` | T04：逐目录读取 header/消息/标记，保留 session/message/parent/fork ID、原字节、未知/opaque/签名，缺 Run 明示 legacy_unknown；批次落真实规范化库；T05 全源冻结快照；T06 导出与再导入不造任务。 |
| `session-manifest-sqlite` | `switch-to-new-authority` | T04：只读 manifest 对账 ID/归属/标题/生命周期，冲突保留并阻止采用；T05 checkpoint+backup；不把旧 manifest 和新 sessions 同时当权威。 |
| `session-sidecars` | `switch-to-new-authority` | T04：会话元数据/标题/Bridge绑定/视觉笔记分别映射并原文归档，保留父子/权限来源；不能从文件名猜授权。 |
| `signed-artifacts` | `shell-local` | T04 不迁入业务库、不共享；T05 先停止旧Electron写者；T06 旧壳在独立根启动且新根不变；生产默认不动。 |
| `skill-state` | `preserve-format` | T04 原格式逐项枚举/计数/摘要/引用保留；T05 共同静止点快照（其中 SQLite 走 Online Backup）；T06 新编辑单独归档、默认恢复旧快照，再前滚验证。 |
| `skill-translation-cache` | `regenerable` | T04 明确登记旧缓存不转权威、按现行行为重建；T05 原件保留不删；T06 不将缓存缺失冒充业务数据丢失或恢复成功。 |
| `studio-mount-registry` | `preserve-format` | T04 原格式逐项枚举/计数/摘要/引用保留；T05 共同静止点快照（其中 SQLite 走 Online Backup）；T06 新编辑单独归档、默认恢复旧快照，再前滚验证。 |
| `subagent-state` | `switch-to-new-authority` | T04：保留父子/线程/fork谱系及权限衰减；不复活旧 active 子任务，不把历史状态变成授权。 |
| `terminal-session-state` | `switch-to-new-authority` | T04：旧终端 transcript 原文归档、进程状态标不可继续；禁止导入时恢复 PID 或重跑命令。 |
| `usage-ledger` | `preserve-format` | T04 原格式逐项枚举/计数/摘要/引用保留；T05 共同静止点快照（其中 SQLite 走 Online Backup）；T06 新编辑单独归档、默认恢复旧快照，再前滚验证。 |
| `user-preferences` | `preserve-format` | T04 原格式逐项枚举/计数/摘要/引用保留；T05 共同静止点快照（其中 SQLite 走 Online Backup）；T06 新编辑单独归档、默认恢复旧快照，再前滚验证。 |
| `user-studio-registries` | `preserve-format` | T04 原格式逐项枚举/计数/摘要/引用保留；T05 共同静止点快照（其中 SQLite 走 Online Backup）；T06 新编辑单独归档、默认恢复旧快照，再前滚验证。 |
| `web-session-registry` | `preserve-format` | T04：原格式及会话撤销/期限保留，不恢复过期授权；T05/T06 与身份同一静止点。 |
| `workflow-state` | `switch-to-new-authority` | T04：保留 journal/会话/活动引用，旧未决工作流归档并可解释；不把导入当调度触发。 |
| `workspace-snapshots` | `regenerable` | T04 按既定 regenerable 分类记录，不迁作新权威；T06 必须另保留真实新栈 file checkpoints/witness，不能以旧缓存分类删掉新增恢复数据。 |

## 新增 Rust 存储、V24—V26 与“69 类”以外的必要清单

69 是旧登记类别，不是允许忽略新存储的上限。R02 registry 的 `new_persistence_points` 当前只有 `rust-runs-db`；该项 migrations 列有 V1—V26，但 tables 字典未逐项列入 V24 的 `observability_legacy_trace` 和 V25 的 `bridge_loop_deliveries_v1`。因此全表验收须枚举实际 `sqlite_master` 并核 MIGRATIONS，不能只循环该 tables 字典。R08 必须另以源码逐点核实新目录、恢复文件与 runtime 身份的分类，不能把 `lingxi-service/**` 全部视为可丢弃缓存。

| 新增内容 | T04/T05/T06责任 |
|---|---|
| `lingxi-service/data/runs.db` 的所有实际表与 `schema_migrations` | 新增归档采用真实全库 Online Backup，校验 SQL 收据/foreign_key_check/integrity_check，所有表逐类行数与逻辑摘要；兼有文件的表再查真实文件；只导出 messages 不足以恢复事实。 |
| V24 `observability_legacy_trace` | 保留 model_call_id/trace_id/parent_call_id/imported_at，重导 call幂等且事实冲突拒绝；规范化导出与旧 trace 联查一致。 |
| V25 `bridge_loop_deliveries_v1` | 保留 delivery_id、冻结connector generation、session/run、queued/enqueued/failed及错误；再次前滚不能把已外发效果当新触发重做。 |
| V26 `bridge_outbox_v1` 新 `loop_delivery_id` 与来源约束 | 原外发收据/来源四选约束及唯一ID必须一起恢复；证明外部替身计数不增、未决效果仍unknown/需核对，不能仅恢复消息文本。 |
| 其它既有 runs.db 表：invocation_journal、model_call_usage、session_operations、run_lineage、session_files/aliases/refs、file_history/checkpoints、automation与Bridge/browser/media/beautify/phone等 | 同一全库备份天然保留实际表，但验收须逐类别核关键身份/引用/收据；导入逻辑不可把任何历史行变成可运行任务/批准。 |
| `file-write-witnesses/`、`file-recovery-records/` | 当前 file_safety.rs 与 service lib 真实保护的恢复状态，须进新增归档与版本映射；pending 不可删，不可在恢复时重新执行已写动作。 |
| `lingxi-service/agent-deletions/` | 当前 service lib 保护的恢复目录，须辨认pending/完成状态并保留；不当作普通tmp静默清掉。 |
| `lingxi-service/device-registry.journal.json` | auth.rs 的跨文件身份事务日志；和devices/credentials/pairing等共同静止捕捉，半完成状态不能独立丢掉或重放成批准。 |
| `lingxi-service/instance.lock`、instance/stale记录、local-token、logs/tmp | OS锁不能复制后宣称仍持有；短期token、实例记录按新进程重新建立；诊断原文保存在受控归档但秘密不进入交付证据；清单明确哪些不作为可恢复业务事实。 |
| 合成workspace中的实际新文件/附件 | 不能只备份home后声称真实文件任务可恢复。fixture显式声明受控workspace、逐文件摘要/资源引用/变更收据；禁止扫描或打包未经授权的真实工作区。 |

## 最小完整实现建议（仍为待实施）

1. 复用唯一维护入口/现有 epoch 和 storage 组件增加 R08 受控命令，不另起业务内核。迁移协调器有明确 source/target/backup/control/workspace 参数，必须绝对、canonical、互不嵌套且指向获准合成副本。拒绝真实默认根/符号链接混淆/目标已被别批次占用。控制journal位于不会随active切换消失的独立根，记录source指纹、版本、旧/新根、阶段、完整清单和收据hash。
2. 明确停止旧server/旧Electron/CLI写者、Rust与SDK worker；复用Rust OS锁，另核旧server握手、PID/进程树和打开文件，不用mtime猜测。旧程序不认识新锁，因此始终物理分离根；通过active指针打开的新根须有旧版实际理解的高位印章，旧程序直接用固定旧根时只触碰旧根。自行重启/未能确认停止一律拒绝该切换，不结束未经授权的他人进程。
3. 以同一停写窗口预检版本、权限、备份空间和目标；逐库checkpoint+Online Backup，普通文件快照（包括目录内SQLite识别）后生成整单、fsync与verify。冻结来源之后导入只读；checkpoint会改变SQLite物理布局，应在同一静止点记录物理/逻辑基线，不能混用迁移前热WAL与迁移后冷库字节声称“原文变了”。
4. 在隔离目标离线运行 LegacyImporter。按69原策略+新增Rust清单逐项回执，区分NOT_PRESENT/保留/转换/重建/隔离/错误，未知文件保留但未分类不能假装全量。JSONL按稳定session/entry ID建映射，原始bytes另归档；缺run边界明确legacy_unknown（目前源码未见该专用标记）；不造Run/批准。会话/消息/父子/fork/trace/附件ID不重编号。未知/opaque/签名不直接显示，后续模型安全回放需现有模型档案规则。
5. 采用正式追加迁移表/现有操作收据记录每批输入hash、行/引用计数、完成游标、问题和逻辑摘要，内容与确认游标同一事务。重跑同批同内容无变化；同主键异内容响亮冲突。坏尾行/坏附件各自记录原位置和原因，保留好行，不能“skipped>0但overall PASS”掩盖必需数据不可恢复。暂停在已确认批次边界，硬退出后从真实journal继续。
6. 导入后经正式Rust查询/导出对账数量、消息parent/fork谱系、file alias与文件hash、trace映射、未知档案和新存储schema。转换/收据/整体manifest验证完成后走正式采用与epoch盖章，再原子active替换并fsync父目录。当前Rust own_epoch=1的限制必须经明确定义的新栈版本契约解决，生产旧默认不变；严禁手改印章/删journal使服务放行。
7. selfcheck前拒绝业务提交/调度/外发；selfcheck覆盖真实身份、协议、数据epoch与采样引用，成功后开放新根写入。若启动失败保留旧根/新根/所有收据，通过同入口按阶段修复/退回，不自动删任何数据。客户端连接由已有显式预览选择消费active新根，不更改生产默认。
8. 在真实Rust服务经正式入口产生新会话、附件、设置编辑、外部替身操作收据；停止所有写者，完整备份新库/保留格式编辑/文件与恢复日志，另提供可核对的逻辑导出+版本/身份映射作为再导入包。默认仅把旧快照恢复到另一个独立旧根，不把新schema灌回旧版；新根和归档原样保留。
9. 启动固定旧构建验证旧历史可读/可写范围、新root完全不变，说明新会话和新增配置行为在旧UI不可用。再次前滚恢复可再导入包到独立新根，走相同正式校验/采用/指针入口；二次前滚再重复验证身份/计数/附件摘要不变、外发计数不增长。恢复文件存在并不等于恢复成功。

## 必须有的反向验证和验收绑定

| 组 | 反例 | 必须观察的失败与恢复 |
|---|---|---|
| 数据根 | 默认真实根、错误根、source=target、嵌套target、链接/junction逃逸、同名不同canonical根 | 命令非零；真实/源根不变，不迁移、不切指针；恢复正确副本后成功。 |
| 单写者 | 旧server/旧壳/Rust任一仍活跃、同根第二Rust、两个迁移进程 | 拒绝获得共同静止点；无同数据集并行写；固定旧构建向新根拒写或物理隔离、新根hash不变。 |
| 数据源 | JSONL半条坏尾/未知字段/opaque/签名/缺附件/坏checksum/重复ID不同内容/父引用缺失 | 好行保留，问题逐项明确；原文全留；缺run保持unknown；不能凭行数相同就通过引用检查。 |
| 批处理 | 批次提交前/提交后游标前/游标后kill；暂停/重跑；源文件在恢复时改变 | 已确认批次不重复；游标与内容一致；源变化拒绝复用旧回执，不能无条件INSERT OR IGNORE吞差异。 |
| 一致备份 | 热WAL、目录内多个SQLite、一个库checkpoint busy、integrity失败、文件拷贝中断 | 所有库及文件同静止点；无正式完整manifest即不可采用；不复制活跃.db单文件；保留失败和已做部分。 |
| 五断点 | 预检/备份/导入/切指针前后/启动各崩溃 | 同入口重跑确定唯一有效根；原件、两根和journal保留；未验证目标不开放写；无双写。 |
| 空间权限 | 预检不足、备份/导入/归档途中ENOSPC、无写权限、manifest fsync失败 | 非零、不能切换默认，原库完整；已切换阶段保持唯一有效根并进入可恢复修复状态。 |
| epoch | 高位/损坏印章、半完成journal、无采用收据、库版本太新/回执指纹篡改 | 正式服务拒启；不能清章/删journal/放宽原门禁；正确同入口恢复后才绿。 |
| 回滚新增 | 新会话+附件+配置+已确认外发+未决外发；归档损坏/不足空间/旧根启动失败 | 归档验证前不切回；失败保留两侧；新数据再次前滚可查询/导出；确认外发不重发，unknown不盲重试。 |
| 收据全量 | 漏任一69类别或新增Rust状态、V24映射缺失、V25/V26去重行丢失、空清单/空测试匹配 | 验收必须非零；测试总退出0不得盖漏项；补齐后实际重跑。 |

A07/A08 绑定 importer 真实入口及查询/导出，不以测试直接SQL造预期内容代替。A09/A10 必须跑五断点与空间失败；A11/A12 必须跑真实Rust新数据往返和外部替身计数器。固定旧构建、Rust二进制、React产物、fixture、版本/锁文件、命令/退出码/具名断言/日志hash绑定最后候选，独立验收者沿原要求复测。

## 本次读取与差异核验

本次仅源码核对，未运行 Cargo/桌面/迁移测试。读取原R08全书、R01 ADR-004与ROLLBACK_DESIGN、69类机器矩阵、R02注册及上述实际代码。源文件摘要如下，便于后续识别准备报告是否已过期：

| 输入 | SHA-256 |
|---|---|
| `docs/rust-tauri/R00/STORES.json` | `6067bdd3be7d0232c182d8f9c1beb147e1a5b59a3fa3ae137889f31a75d7b493` |
| `docs/rust-tauri/R01/DATA_COMPATIBILITY_MATRIX.json` | `458bb2da36e2f14f4100b985e6ceb5480fc1b5f947a441aced41077331acbbb1` |
| `docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json` | `7c23f68b441fb8905c6a0ecb89390d8e399d23b938e6dd4bdfdc58e1edfde4f3` |
| `rust/crates/lingxi-service/src/epoch.rs` | `9c29d9ee8924d7b514a9bc4b7f8d374d420287f25b1b36f5295619c0f882b855` |
| `rust/crates/lingxi-adapters/src/storage/backup.rs` | `bd2d4283a23619624fc10d333a0a60700c81e5020dbc6b248747722a81646e15` |
| `rust/crates/lingxi-adapters/src/storage/migrations.rs` | `9ecea67b5642a17db79848137217a3e173c4d6f9d0e68309fb03ed807e9be549` |
| `core/data-epoch-restore.ts` | `399d31b16c09240727c9936059d253cf27e34312d14e96c3ceb24cd3d666fdb0` |
