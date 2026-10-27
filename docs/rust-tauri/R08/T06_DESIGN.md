# R08-T06 设计｜回滚与迁移后新增数据保留

- 状态：DRAFT 待 Lead 审批（design-first 闸：批准后才进入实现）
- 日期：2026-10-23
- 执行者：t02-core
- 输入锚点：
  - 任务书 `R08_旧Electron客户端接入与数据迁移演练.md` R08-T06 §4（做什么/怎么做 5 条/必须交付）+ R08-A11 + R08-A12 原文
  - task-8 Lead 8 个开放设计点（本文件 §3–§10 逐点作答）
  - `STORAGE_READINESS.md` L10 分工锚 / L42-110 逐类 T06 责任表 / L112-127 新 store 清单 / 建议 8（L138）/ 建议 9（L139）/ 反例 L153 / L156
  - `R01/ROLLBACK_DESIGN.md`（§1.5 协作式闸结论、§2 保格式编辑可读性、§4 检查单）与 `R01/ADR-004-storage-cutover.md`（§2 闸边界实测）
  - `COVERAGE_PREPARATION.md` L34 T06 行
- 设计红线（继承）：零静默降级；合成 HOME 演习；真实 `~/.lingxi` 永不触碰；任何失败两侧保留、进入修复模式、绝不自动删除"多余"目录（任务书④）；红证据保留

## 1. 任务书锚点（原文）

R08-T06 怎么做 5 条：
1. 演练覆盖切换后新增写入（新会话、设置变更、附件等），不是静态样例。
2. 回滚前停新版写者，把新增数据导出/快照成独立包，并记录版本和映射；绝不向旧程序写它无法理解的新 schema。
3. 旧版本数据根恢复后验证旧程序能启动；新增数据保留为可重新导入的独立包，并明示哪些项旧 UI 不可立即编辑。
4. 回滚失败保留新旧两侧并进入修复模式，绝不自动删除"多余"目录。
5. 再前滚时不重复导入同一批新增数据，也不重放外发操作收据。

必须交付：RollbackCoordinator 或等价的受控迁移命令；回滚操作手册；前滚/回滚收据。

- R08-A11：迁移后新增会话与附件 → 回滚旧版本再重新前滚；通过条件=新数据可恢复且不重复，旧版本期间可读范围明确；证据=往返数据与附件摘要。
- R08-A12：新版本已有确认的外部操作收据 → 回滚再前滚；通过条件=不重新发送工具动作，只恢复历史记录；证据=外部计数器和收据。

## 2. 基线事实（全部实测/源码锚定）

- **F1 结构性优势**：T05 源零改动 → 旧根本即 epoch=1 原样。回滚 = **指针回指原旧根 + 新增数据独立包导出**，不是 R01 演练的"B0 恢复到独立副本"形态（R01 §4 时间线 t1 B0 在 R08 被旧根原样替代）。此框架 Lead 已在 task-8 确认。
- **F2 印章事实（本设计最关键的实测发现）**：
  - 旧根 fixture 携带 legacy v1 印章 `{epoch:7}` + 过渡日志 `{from:6,to:7}`（`r08_t04_fixture_gen.mjs:311-312`，生成器明示 epoch 域 out-of-scope 登记不迁移）。
  - 现役旧栈 `DATA_EPOCH=1`（`shared/contract-versions.json`）。按 ADR-004 §2 实测闸边界，完好可读的高位印章（7>1）触发 `mustBlock` → **旧栈在旧根原样上拒启**（`LINGXI_DATA_EPOCH_BLOCKED reason=epoch-downgrade-blocked`）。
  - ∴ 回滚事务必须包含**受控旧根准备相**（§7），这是回滚对旧根的唯一事务性写入点。
  - 新根印章 v2 `{epoch:2, minimumReaderEpoch:2, committedDataEpoch:2}`（T05 epoch_adopt 落盘）→ 旧栈硬拒（epoch-downgrade-blocked）。旧栈绝无可能误开新根。
- **F3 新根实测布局**（t05-d2 scratch 盘点）：
  - `lingxi-service/data/runs.db`（Rust 权威库，V1–V28）
  - `session-files/`（附件/文件侧车）
  - `knowledge/`（保格式库 `knowledge*.db` + `.owner`/`.guard` 单写者文件）
  - `user/preferences.json`（设置，`userprefs.rs:4`：与 workspace-rollback 同源的合并写，保留未知字段）
  - `legacy-import/`（T04 收据群：migration-receipt.json / adoption-receipt.json / issues.jsonl / source-manifest.{before,after}.json / quarantine / staging）
  - `lingxi-service/` 运行时（instance.lock / local-token.json / logs / tmp / media-exec-spill / management.json / browser / devices*.json）
  - `logs/security-audit.jsonl`
- **F4 增量边界判据可得性**：V27 簿记表 `import_runs`/`import_batches`/`import_issues`（migrations.rs:1087）在新根 runs.db 内；T04 转换路径行级幂等（`convert_sessions.rs:243` 会话种子已存在→already-imported）；旧根冻结 → 可用 T04 同源分类器对旧根**重导出 legacy 身份集**。边界判据不依赖时钟、不依赖 T05 新增语义。
- **F5 A12 收据闸代码事实**（`storage/bridge.rs`）：
  - `list_pending_outbox`（L1020）**只拾 `status='queued'`**；`claim_outbox`（L1035）queued→started；`settle_outbox`（L1068）started→confirmed/unknown/failed。
  - `validate_started_outbox`（L1040）：started 行启动时走校验路径，非盲发。
  - 连接器重配置 bump generation 并作废旧代 queued 行（L340）→ 收据行绑定 `(connector_id, connector_generation)` 代际。
  - V25 `bridge_loop_deliveries_v1` 注释（migrations.rs:1012-1014）："delivery_id 幂等保证不重复外发"。
  - ∴ **confirmed 永不重发、unknown 永不拾取**是结构性成立的；回滚/再前滚只需保证收据行原样存续且代际锚不变，闸门即守。
- **F6 复用机制**：指针 `active-data-root.json` schema v1，`read_pointer`/`commit_pointer` 原子 rename（cutover/pointer.rs L14/L101/L124）；`run_backup` + `backup-manifest.json`（cutover/backup.rs L25/L293）；selfcheck 子进程起服务+只读探针+SIGTERM 模板（cutover_orchestrator.rs L825）；statfs 空间预检；quiesce 写者探针；journal 相机（history 数组 + phase 字段，committed 无 history 条目，§113/§114 resume 语义）。
- **F7 旧栈启动形态**：`node scripts/launch.js server`（或 cli）；端口 `LINGXI_PORT` env（server/index.ts:352）；就绪信号 = `server-info.json` 落 home + 环回 HTTP 应答；闸标记走 stderr（ADR-004 §2）。演习探针可完全复用 R01 A1-A9 形态 + R02 R5/R6 owned-pid/owned-home 卫生形态。

## 3. 开放点 1｜回滚语义基线

**回滚 = 受控事务，语义为"指针回指原旧根 + 新增数据独立包导出 + 旧栈启动验证"**，具体相序见 §8。核心断言：

1. 旧根是 epoch=1 原样（F1），回滚**不做** B0 式恢复拷贝；唯一事务性写入 = 旧根准备相（§7）。
2. 新根（epoch=2）回滚后**原样保留**：印章不动、内容不动、永不自动删除（任务书④）。新根成为"挂起的 epoch=2 权威根"，仅再前滚或人工显式处置可触碰。
3. 保格式编辑（settings/knowledge 等）**默认不回写旧根**（裁决 J2）；旧栈启动后看到的是切换前状态 + 旧栈自己此后的新写入。
4. 回滚后旧栈对新根的拒启由 F2 印章事实结构性保证，不依赖任何自觉。

## 4. 开放点 2｜增量捕获（新增数据独立包）

### 4.1 边界判据（裁决 J3 建议）

主判据 = **旧根冻结身份集差集**：
1. 导出器用 T04 同源分类器对**冻结旧根**再盘点，得到 legacy 身份集 L（会话/消息/文件/各保格式类身份）。
2. 对新根盘点得到身份集 N。
3. **delta = N \ L** —— 不依赖时钟、不依赖行内 provenance 列、不依赖 T05 新增语义。
4. 交叉校验：与 V27 `import_batches` 计数对账 + 转换路径行级 `already_present` 语义（F4）复核；对账不符 → export_verify 相拒绝（零静默）。

α1 种子（`sess_local_alpha/beta`）：旧栈从无此种 → 属 N\L，**导出**，类目 `scaffold-seed`，`oldUiReadable=false`；再导入时空表种子已存在 → already_present 幂等（裁决 J7）。

### 4.2 覆盖类目（增量包 payload）

| 类目 | 内容 | oldUiReadable | editableInOldUi | 说明 |
|---|---|---|---|---|
| `rust-authoritative-db` | runs.db 权威表逐表 JSONL（sessions/messages/runs 系、attachments 行、observability_legacy_trace、automation 等，按 migrations.rs V1–V26 已注册表全量枚举；V27/V28 import 簿记行**不入 payload**，恢复式再前滚会重建，排除理由记入 manifest） | false | false | epoch=2 新权威，旧栈无读者 |
| `external-receipts` | `bridge_outbox_v1` + `bridge_loop_deliveries_v1` + `bridge_connectors_v1`（代际锚）原行 | false | false | A12 闸门载体，见 §5 |
| `session-files` | 新附件/文件侧车原字节（relpath + sha256） | false | false | 旧栈 session-files 目录结构中无对应身份 |
| `settings-preserved-format` | `user/preferences.json` 整体文件 | true | false（默认不回写） | 格式冻结，旧栈读得懂；回写=人工决策（J2） |
| `preserved-format-db` | `knowledge/*.db` 文件级 sha256 + 原字节 | true | false（默认不回写） | 同上 |
| `scaffold-seed` | α1 种子行 | false | false | 见 §4.1 |

排除面（不导出，可再生/安全域，逐条记入 manifest 排除清单）：`instance.lock`、`local-token.json`、`logs/`、`tmp/`、`media-exec-spill/`、`management.json`、`browser/`、`devices.json`/`device-credentials.json`（设备身份安全域，旧根自带原身份）、`legacy-import/`（T04 收据群属切换期产物，由 lineage 引用不复制）。

### 4.3 包格式 `t06-delta-package/v1`

```
<delta-out>/t06-delta-<rollbackId>/
  manifest.json        # formatVersion=1；逐类目计数；逐文件 sha256；identitySetSha256；
                       # 映射表（上表 oldUiReadable/editableInOldUi/mergeBackDefault=false）；
                       # 排除面清单及理由
  lineage.json         # sourceCutoverId、forwardMigrationReceipt{path,sha256}、
                       # oldRootSha256Manifest、newRootSha256Manifest（导出时点）
  payload/db/<table>.jsonl        # 每行 {identity, rowJson, rowSha256}
  payload/files/<relpath>         # 原字节
  payload/files-manifest.json     # relpath → {sha256, 类目, oldUiReadable, editableInOldUi}
  receipts/bridge-outbox.jsonl    # outbox 原行（含 status/platform_receipt_json/代际锚）
  receipts/bridge-deliveries.jsonl
  old-root-prepare/      # 旧根准备相归档：原印章字节、原过渡日志字节（§7）
```

export_verify 相：重算全部 sha256 + 计数对账 + identitySetSha256 复算，任一不符 → 拒绝（§8 失败表）。

## 5. 开放点 3｜A12 收据闸

- **同一性判据**：外部操作收据注册表 = `bridge_outbox_v1` ∪ `bridge_loop_deliveries_v1` ∪ `bridge_connectors_v1`（代际）。同一性 = `delivery_id` 主键 + `(connector_id, connector_generation)` 代际锚 + `platform_receipt_json` 内容。再前滚恢复时连接器行**同代际**恢复（不重配置 → 不 bump generation → 不作废存量行，F5）。
- **重放防护链**（F5 结构性成立）：confirmed 永不入 pending；unknown 永不入 pending 且不盲试；started 走 validate 校验路径；queued 会发一次 = **正常未完成投递续跑，非重放**（手册明示此语义；演习哨兵不用 queued 以免混淆"回滚重放"与"正常续投"）。
- **演习哨兵**（确定性替身，本地环回 HTTP 计数 + 签发平台收据，形态同 r00 stub 先例）：
  - S1：confirmed 行（替身计数器=1）
  - S2：unknown 行
  - S3：started 行
  - 回滚 + 再前滚全程后断言：替身计数器**恒=1**；S1 行 status/receipt sha256 不变；S2 行不变；S3 经校验落定且不产生新外发。
- **证据**：替身计数器快照序列 + outbox/deliveries/connectors 三表 sha256 前后 + 哨兵行原样（A12：外部计数器和收据）。

## 6. 开放点 4｜再前滚幂等

### 模态 A：快速回指（默认，裁决 J4）

前提 = 新根完整（回滚收据的 newRootSha256Manifest 逐文件复验通过 + 印章 {2,2,2} 在）。相序：
preflight → verify_new_root_intact（对回滚收据逐文件 sha256 + outbox 哨兵行 + 印章）→ stop_old_writer（旧栈锁探针）→ pointer_switch（回指新根）→ selfcheck（起服务，计数对账含增量；R08_CUTOVER 常量下）→ committed + reforward 收据。

**零导入** → "不重复导入同批新增数据"结构性成立；外部收据未动 → A12 恒成立。

### 模态 B：恢复式再前滚（显式 `--recover-new-root`）

新根丢失/损坏 → 全新 target：legacy import（旧根，T04 引擎幂等）→ **delta_import 相**（包 payload 逐表 INSERT，身份主键冲突 = already_present 计数、不入数；**二次导入同包 → 计数零增长**为幂等证据）→ epoch_adopt → pointer_switch → selfcheck（计数 = legacy ∪ delta）→ committed。

### 与 T05 `--resume` 链的关系

三个动词（migrate / rollback / reforward）各自 journal 与各自 id（cutoverId / rollbackId / reforwardId）独立；committed 为终态不可续；**不改 migrate 命令本体与 T05 语义**（模态 B 由 reforward 动词内部复用 import/adopt/pointer 模块）。lineage 链：reforward 收据记录 `sourceCutoverId` + `rollbackReceiptId` + `deltaPackageSha256`（裁决 J5）。

## 7. 开放点 5｜旧程序启动验证（含旧根准备相）

### 7.1 旧根准备相 `old_root_prepare`（回滚事务对旧根的唯一写入点，裁决 J1/J8）

写入集（逐条入 journal + 原字节归档到增量包 `old-root-prepare/`）：
1. 印章重写为 v2 `{schemaVersion:2, epoch:1, minimumReaderEpoch:1, committedDataEpoch:1}`（与 Node `createDataEpochStamp` 同构；原 `{epoch:7}` 字节归档）。
2.  stale 过渡日志 `{from:6,to:7}` 归档并移除（避免旧栈走 corrupt-transition fail-open 警告路径，探针断言可保持"无 EPOCH_BLOCKED、无 fail-open 警告"的硬口径）。

诚实声明：生产真实旧根印章本为 Node 现役形态（DATA_EPOCH=1 系），该相在生产路径是**校验且通常零写入**；演习 fixture 的 `{epoch:7}` 是生成器 out-of-scope 合成噪声，准备相把它修正为与内容相符的 epoch=1 形态。

### 7.2 探针 `old_stack_probe`（t06 脚本）

spawn `node scripts/launch.js server`，env：`LINGXI_HOME=<parent>`（经 `active-data-root.json` 真实解析到旧根——走现役解析路径）、`LINGXI_PORT=<环回隔离口>`，env 净化，超时 + owned-pid/owned-home 卫生（R02 R5/R6 形态）。

断言组：
1. STARTED：`server-info.json` 落旧根 + 环回 HTTP 应答；
2. stderr 无 `LINGXI_DATA_EPOCH_BLOCKED`、无 fail-open 警告；
3. **新根全树 sha256 前后零变化**（无排除面——新根必须逐字节不动）；
4. 旧根写入白名单 = {准备相两件套（事务内）+ `server-info.json` + 旧栈自身再生件}，白名单外零写入；
5. 旧 UI 可读范围 = 旧根既有集（会话列表 API 取样 = 切换前集合；新增会话缺席为**预期**）；
6. 增量包不接线（旧栈无任何读包代码路径——构造性事实 + §4.2 映射表）。

可读范围取证口径：探针调旧栈只读 API 取样 + 映射表逐项核对，输出 `readable-scope.json` 证据（A11"旧版本期间可读范围明确"）。

### 7.2a 修订（2026-10-23 Lead 裁决，先发制人）：探针写集白名单改为证据派生

- **控制探针先行**：D1 前先跑控制腿（证据 `t06-control-probe-*`）——旧栈在 fixture 旧根副本上冷启动（副本经准备相同构处理：v2 `{1,1,1}` 印章 + stale 过渡日志移除，但不经历回滚事务），实测冷启动写集。
- **D1 白名单 = 控制实测写集 ∪ {准备相两件套}**。若 `logs/*.log` 等属旧栈常态冷启动写入，控制腿如实捕获并纳入白名单；若 D1 探针相出现控制集之外的写入，即真红（回滚事务泄漏额外写入），停手上报。
- **口径分离**：回滚相内旧根冻结（除准备相两件套外零写入）；探针相起旧根即活跃，活跃写集以控制实测为准——禁止把「冻结旧根」与「回滚后旧根活跃态」混用同一冻结口径。
- 代码侧白名单（`cutover_rollback.rs` `PROBE_ALLOWED_EXACT ∪ Regenerable 类`）是安全网初值；控制实测若超出该集，按 §7.4 纪律改设计→改码→红绿链（控制腿红证据 → 修订 → D1 绿）。

## 8. 开放点 6｜失败分类与修复模式

| 相 | 失败形态 | 系统态 | 处置 |
|---|---|---|---|
| preflight（指针/印章/锁/statfs 空间） | 任一不符 | 零触碰 | refused 终态，修因重跑 |
| stop_new_writer | 写者存活/锁探针失败 | 零触碰 | refused |
| backup_new_root | IO/空间 | 指针未动、双侧原样（备份对源只读） | refused；备份残件入证据，不删 |
| delta_export / export_verify | 校验不符 | 指针未动、双侧原样 | refused；包残件保留；红证据保留 |
| old_root_prepare | 写入失败 | 原印章/日志字节已归档包内，可人工复原；指针未动 | refused |
| pointer_switch | rename 原子 → either/or；切换后校验失败 | 回指新根（last-known-good） | refused + 修复模式 |
| old_stack_probe | 旧栈拒启/断言失败 | 指针回指新根；旧根保持（印章已准备）；双侧完整 | refused + 修复模式（裁决 J6） |

总则：任何失败两侧保留、journal+收据定位到相、绝不自动删除"多余"目录（任务书④）；修复 = 人工或后续授权任务，不在本事务内隐式自愈。

## 9. 开放点 7｜命令面与收据 schema（裁决 J5 建议）

两个新动词，共享 orchestrator journal/phase 机器（history + phase + §113/§114 resume 语义原样）：

```
lingxi-service rollback  --pointer <active-data-root.json> --old-root <dir> --new-root <dir>
                         --work-dir <dir> --backup-dir <dir> --delta-out <dir>
                         [--resume <rollbackId>] [--stop-after-phase <p>]
lingxi-service reforward --pointer <active-data-root.json> --old-root <dir> --new-root <dir>
                         --work-dir <dir> [--recover-new-root --delta-package <dir> --target <fresh-dir>]
                         [--resume <reforwardId>] [--stop-after-phase <p>]
```

`migrate` 本体与 T05 语义**完全不动**（T05 稳定性零风险面）。

rollback 相序（9 相）：preflight → stop_new_writer → backup_new_root → delta_export → export_verify → old_root_prepare → pointer_switch → old_stack_probe → committed。

收据 schema：
- `t06-rollback-receipt/v1`：`{schema, rollbackId, lineage:{sourceCutoverId, forwardMigrationReceipt{path,sha256}}, pointer{before,after}, oldRoot:{path, stampBeforeSha256, stampAfterSha256, prepareWrites[]}, newRoot:{path, sha256ManifestRef, sealed:true}, deltaPackage:{path, sha256, perCategoryCounts, identitySetSha256}, probe:{...}, phaseHistory[], generatedAt}`
- `t06-reforward-receipt/v1`：`{schema, reforwardId, mode:"fast-repoint"|"recovery-import", lineage:{rollbackReceiptId, sourceCutoverId, deltaPackageSha256}, verify:{perCategoryCounts, sentinelReceiptsUnchanged, secondImportZeroGrowth?}, pointer{before,after}, phaseHistory[], outcome}`
- 前滚收据 = T05 migration-receipt + cutover journal（已有）；三收据以 `sourceCutoverId` 链配对。

## 10. 开放点 8｜与 T05 评审并发

依赖面分级：
- **契约面**（CLI migrate 形态、journal 语义、pointer 文件格式、收据路径）：冻结使用；T05 评审若改契约面 → T06 跟随并在 PROGRESS-t06 登记。
- **内部模块**（cutover/* 函数签名、orchestrator 内部结构）：T06 复用，允许随 T05 评审重构适配。
- **ws-ui-state 评审面**：与 T06 零交集（T06 不触碰 workspace-ui-state）。
- 本设计不假设 T05 未评审部分的任何新语义；T06 实现以 T05 已验收稳定面为唯一基线。

## 11. 演习矩阵

- **D1 全绿往返（A11+A12 一体）**：fixture v3 → migrate（T05 已证路径）→ 新根跑真实服务经正式入口产生新增（新会话+附件+设置编辑+桥接哨兵三行，S1 经替身 confirmed）→ rollback 全相 → old_stack_probe 断言组 → reforward 模态 A → 校验（计数/身份/附件 sha256 不变；替身计数器恒=1）→ 证据 `t06-d1-*`。
- **D2 失败注入三腿**：(a) 篡改 delta 包 manifest → export_verify 拒 → 两侧保留；(b) 空间不足 → preflight/backup 拒；(c) old_probe 失败（环回端口预占用）→ 指针回指新根 + 修复模式 → 证据 `t06-d2-*`（红证据保留）。
- **D3 恢复式再前滚**：reforward --recover-new-root 到全新 target + 二次导入同包幂等（计数零增长）+ A12 哨兵在新新根恒成立 → 证据 `t06-d3-*`。
- 替身：`scripts/rust-tauri/r08_t06_standin.mjs`（确定性环回 HTTP 计数 + 收据签发）。
- 验收映射：A11 ← D1+D3（往返数据与附件摘要）；A12 ← D1+D3（外部计数器和收据）。
- 所有证据经 r08_run.py 包裹、`t06-*` 前缀、每腿全新证据目录。

## 12. 裁决点列表（请 Lead 逐点裁）

| # | 问题 | 选项 | 建议 |
|---|---|---|---|
| J1 | 旧根准备相是否允许事务性写入旧根（印章重写 + stale 日志归档移除，原字节归档包内） | 允许（独立相，journal+收据登记）/ 拒绝（要求旧栈先自行维护印章） | **允许**。否则 fixture 旧根上旧栈结构性拒启，A11 无法演练；生产路径通常零写入 |
| J2 | 保格式编辑（settings/knowledge）是否回写旧根 | 默认回写 / 默认不回写（纯回指+独立包+映射表） | **默认不回写**（R01 §4 检查单 item 6 的"纯恢复"分支；任务书③只要求独立包+明示不可编辑项；旧根零额外突变） |
| J3 | 增量边界判据 | 旧根冻结身份集差集 / 行内 provenance 列 / 时钟水印 | **冻结身份集差集** + already_present 交叉校验（不依赖时钟与 T05 新语义） |
| J4 | 再前滚默认模态 | A 快速回指 / B 恢复式 | **A**（零导入结构性幂等；B 仅新根失却时显式启用） |
| J5 | 命令面 | 新动词 rollback/reforward / migrate 子模式 | **新动词**（migrate 本体与 T05 语义零改动） |
| J6 | old_stack_probe 失败时指针处置 | 回指新根（last-known-good）/ 停留旧根 | **回指新根**（旧栈拒启=旧根方向不可服务；双侧保留+修复模式） |
| J7 | α1 种子在增量包中处置 | 导出（类目 scaffold-seed）/ 排除 | **导出**（oldUiReadable=false；再导入 already_present 幂等；排除会丢失新根真实现状） |
| J8 | 准备相印章目标值 | v2 {1,1,1}（Node 同构）/ v1 {epoch:1} | **v2**（现役 Node 写出形态，信息更全，ADR-004 A4 验证可启动） |

## 13. 交付物映射

| 任务书交付 | 本设计对应 |
|---|---|
| RollbackCoordinator 或等价受控迁移命令 | `rollback` / `reforward` 动词（§9），复用 orchestrator 相机 |
| 回滚操作手册 | `docs/rust-tauri/R08/T06_ROLLBACK_RUNBOOK.md`（实现期交付：前置检查/命令序列/逐相期望/失败修复流/收据位置/再前滚两模态/外部收据闸说明/queued 续投语义） |
| 前滚/回滚收据 | T05 migration-receipt（已有）+ `t06-rollback-receipt/v1` + `t06-reforward-receipt/v1`（§9） |
| A11 验收 | D1+D3（§11） |
| A12 验收 | D1+D3（§11） |
