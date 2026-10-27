# R08-T06 回滚操作手册（T06_ROLLBACK_RUNBOOK）

- 依据：T06_DESIGN.md（批准版）+ 裁决 J1–J9/⑤a/①② + 演习证据
  t06-d1-drill-final-08（D1）/ t06-d2-drill-03（D2）/ t06-d3-drill-08（D3）。
- 实现载体：`lingxi-service rollback` / `lingxi-service reforward` 两动词（migrate 本体零改动，J5）。
- 红线：任何失败**两侧保留**、绝不自动删除"多余"目录；修复模式入口=journal 的 repairAction。

## 0. 哈希口径披露（先读）

**manifest 双哈希内部约定**：T04 `manifest.rs` 的 `file_sha256` 实为
`sha256(sha256(content))`（双重哈希），T04/T05 全消费方同口径自洽；该约定已锁进
T05 封存对拍锚（80a8d2c2…feaa 对拍的是 Rust `source_root_sha256` 输出），**R08 内
不修**（修复=锚变=封存链失效，属 post-R08 治理项）。

**T06 增量包一律用真·内容 sha256**（`shasum -a 256` 可对拍）：包内
`manifest.json` / `files-manifest.json` / 各 `payload/db/*.jsonl` 的行级
`rowSha256` 均为真实内容哈希；`lineage.json` 内的 `newRootSealHash` /
探针报告的新根封缄值走 Rust `source_root_sha256`（即上列内部双哈希约定）。
**凡向人展示哈希处：manifest sha256 域为内部双哈希约定；真内容 sha256 见增量包条目。**

## 1. 前置检查（preflight 相自动执行，人工可先核）

1. 指针 `active-data-root.json` → 新根；新根印章 v2 `{epoch:2, minimumReaderEpoch:2,
   committedDataEpoch:2}`（切换已 committed）。
2. 新根无活写者：Rust 单写者锁不可持有（停服后跑一次确认）；旧根无旧栈存活
   （server-info 探针）。
3. 空间：备份卷可用 ≥ 新根树×2 + 256MB 余量；增量卷 ≥ 树 + 256MB。
4. 前滚迁移收据 `<new-root>/legacy-import/migration-receipt.json` 存在且
   `status="complete"`（lineage 引用源）。
5. `--old-stack-launcher`（scripts/launch.js）与 `--old-stack-cwd` 可用（探针要冷启旧栈）。

## 2. 回滚命令序列（9 相，单命令）

```
lingxi-service rollback \
  --pointer <active-data-root.json> --old-root <旧根> --new-root <新根> \
  --work-dir <wd> --backup-dir <bd> --delta-out <dd> \
  --old-stack-launcher scripts/launch.js --old-stack-cwd <repo>
# 成功行：ROLLBACK_RESULT rollbackId=… phase=committed outcome=committed
# 恢复中断：加 --resume <rollbackId>；--stop-after-phase <p> 可相界停。
```

### 逐相期望

| 相 | 行为 | 期望 |
|---|---|---|
| preflight | 指针/印章/锁/空间/收据 | 任一不符=refused（exit 2）零触碰 |
| stop_new_writer | 写者探针幂等再验 | 有活写者=refused |
| backup_new_root | 全树副本+backup-manifest（t06-newroot-backup/v1） | 源只读；残件入证据不删 |
| delta_export | 旧根身份集 L + 新根差集导出成包 | 包=真·sha256 口径；L 推导开**副本**（P6：源库绝不直开——READ_ONLY 连接对 WAL 库仍会建 -shm/-wal 污染封缄源） |
| export_verify | L 重导 + 新根封缄复验 + 包自校验 + **确定性重导逐字节比对** | 重导复用首次导出的 lineage.json 字节（lineage 含 createdAt 时间戳，manifest 内嵌 lineageSha256——不复用则时间戳泄入确定性门） |
| old_root_prepare | 对旧根唯一事务性写入点（J1） | 见 §3；生产真实旧根通常零写 |
| pointer_switch | 原子 rename 回指旧根 | 切后旧根印章必须已备形（双保险） |
| old_stack_probe | 旧栈冷启动断言组 | 见 §4；失败=J6 指针回指新根 |
| committed | rollback-receipt 落盘 | `<work-dir>/rollback-receipt.json` |

## 3. 旧根准备相写入集（J1 纪律 + J9/⑤a 封闭集）

纪律：每笔写入**原字节先归档包内** `old-root-prepare/` + sha256 复验 + 逐条 journal +
收据 `prepareWrites[]`。生产诚实路径：已合规项零写（verify-only）。**封闭五件套，集外
新阻断=停手上报，不扩集**（J9）：

1. 印章重写 v2 `{schemaVersion:2, epoch:1, minimumReaderEpoch:1, committedDataEpoch:1}`
   （J8；原字节归档 `data-epoch.json.orig`）。
2. stale 过渡日志 `data-epoch-transition.json` 归档移除。
3. `security/execution-leases.json` → 生产空注册表形状
   `{schemaVersion:1, leases:[], createdAt, updatedAt}`（J9①；夹具合成行=注册类
   覆盖样本非有效 lease）。
4. `agents/{agentId}/config.yaml` 缺失者补最小有效配置（J9②，0600；防旧栈 first-run
   另建默认助手掏空断言⑤）。
5. `knowledge/knowledge.db` 合成表 RENAME `fixture_synthetic_*`（⑤a：判别式=
   `documents` 表存在即合成〔生产全 schema 无此表〕；`processing_artifacts` 缺
   `content_snapshot_id` 列即合成。单事务保序行零丢失+integrity_check；旧栈冷启动
   后其迁移链自建生产空表=首启同态。**披露：knowledge 视图为空**）。

三例夹具合成噪声（{epoch:7}/leases 合成行/knowledge 合成表）真实旧根皆无——
**T07 对照清单：真实旧根见任一=数据本身异常，停手**。

## 4. 探针相：写集白名单与印章条款

- **口径分离**（§7.2a）：回滚相内旧根冻结（除准备相外零写入）；探针相起旧根活跃，
  活跃写集以**控制探针实测**为准（t06-control-probe-05：added=55/modified=4/removed=0）。
- **白名单 = 控制实测类集（18 类）∪ 准备相件类（execution-leases/agent-profile）∪
  server-info.json ∪ {db}-shm/-wal 兄弟泛化**；集外类/未分类路径写入=真红停手上报
  （控制集之外出现写入=回滚事务泄漏）。白名单钉死测试
  `control_probe_write_set_pinned` 常驻单测。
- **印章条款（裁决①）**：旧栈清洁启动把 `lastVersion` 盖为自身版本、刷 `updatedAt`
  =生产忠实 provenance 盖戳，允许；四个 epoch 字段（schemaVersion/epoch/
  minimumReaderEpoch/committedDataEpoch）逐值冻结，其余任何差异（含字段增删）拒绝。
  证据列 `epochStampEvidence`（前后六字段逐值+provenanceStamped 判定）在探针报告。
- 新根全树零变化（无排除面）；stderr 三 epoch 门标记零出现；SIGTERM owned-pid
  优雅关停。
- **探针失败 = J6**：指针回指新根（last-known-good）+ refused 终态 +
  journal.refusal.repairAction（D2c 腿实证）。

## 5. 失败修复流

1. journal（`<work-dir>/rollback-journal.json`）定位：`status=refused` +
   `refusal.{phase,reason,repairAction}`；`phase` 字段停留在失败相名。
2. 双侧保留核验：新根印章/树仍在；旧根保留（准备相已写部分的原字节在包内
   `old-root-prepare/` 可人工复原）。
3. 按 repairAction 处置后**重跑**：无 --resume 重跑=响亮拒（双跑守卫）；
   `--resume <rollbackId>` 续跑（committed=幂等空跑 already-committed；refused=终态
   不可续，修因后全新 rollbackId 重跑——包/journal 属旧事务不复用）。
4. 失败注入参考（D2 腿实证）：包篡改→export_verify 拒（packageSha256 mismatch）；
   备份卷空间不足→preflight 拒（`insufficient space on backup volume`）；旧栈不
   服务→probe 拒+指针回指新根。

## 6. 再前滚两模态

### 模态 A：快速回指（默认，J4）

```
lingxi-service reforward --pointer <p> --old-root <旧根> --new-root <新根> --work-dir <wd>
```
前提：指针在旧根；新根完整（对回滚收据逐文件 sha256 + 哨兵行 + 印章 {2,2,2} 复验）。
**零导入** → 「不重复导入同批新增数据」结构性成立；外部收据未动 → A12 恒成立。
成功行 `REFORWARD_RESULT … outcome=committed`。

### 模态 B：恢复式（新根丢失/损坏，显式 --recover-new-root）

```
lingxi-service reforward --pointer <p> --old-root <旧根> --new-root <原新根> --work-dir <wd> \
  --recover-new-root --backup-dir <bd> --delta-package <回滚增量包目录> --target <全新恢复根>
```
相序：preflight（同 work-dir 读 rollback journal committed 事实）→ backup 旧根 →
legacy import 到全新 target → delta_import → delta_verify（**二次应用断言
applied=0 → 收据 secondImportZeroGrowth=true，J4**）→ epoch_adopt → pointer_switch
→ selfcheck → committed。原新根全程零触碰（J6 双侧保留）。

### 恢复根语义（D3 实证，如实向用户披露）

- **计数**：sessions/messages/runs 与原新根逐值相等。
- **附件**：原新根现存附件在恢复根字节相同（子集方向）；恢复根可**多于**原新根——
  legacy 全量重导入会带回原新根已按过期 GC 的 legacy 会话文件（正常，非数据异常）。
- **表级回灌语义**：sessions 与 `automation_*` 族（可变元数据/调度运行态）同身份
  发散=整体替换并计数（新根为切后权威；「导入零写入」假设已被 D3 证伪——legacy
  导入会建 legacy-job-* 行、新根调度器又合法改写调度字段）；`session_files` 族
  身份冲突=**恢复根行胜**（根作用域绝对路径正确性优先，计数
  `pathScopedPreserved` 披露——包行携带原新根路径，整体替换会写回错误路径）；
  messages/runs/model_call_usage/bridge_* 账本表同身份异内容=响亮拒（不可改账）；
  空表跳过（subagent_* 等运行时惰性建表族在恢复根暂不存在属正常，服务首次使用
  自建）；簿记表（import_runs/batches/issues/extensions/legacy_runtime_state_registry/
  **schema_migrations**）不入包——目标自建。

## 7. 外部收据闸（A12）与 queued 续投语义

- 注册表 = `bridge_outbox_v1` ∪ `bridge_loop_deliveries_v1` ∪
  `bridge_connectors_v1`（代际锚 `(connector_id, connector_generation)`）。
- **结构性成立**（bridge.rs）：`list_pending_outbox` 只拾 `queued`；`claim` queued→
  started；`settle` started→confirmed/unknown/failed；started 启动走
  `validate_started_outbox` 校验路径非盲发；连接器重配置 bump generation 并作废
  旧代 queued 行；V25 delivery_id 幂等。∴ **confirmed 永不重发、unknown 永不拾取**；
  回滚/再前滚只需收据行原样存续且代际锚不变，闸门即守。
- **queued 续投语义（附加钉②，重要）**：`queued` 行在服务重启后**会正常续投一次**
  ——这是**正常未完成投递的续跑，不是回滚重放**。演习哨兵不用 queued 行以免混淆
  两种语义；生产回滚场景若存在 queued 行，回滚后再前滚=续投一次（一次性，非重复）。
- started 行落定两径（D1 实证，皆为受控、皆零重发）：优雅关停→宿主关停路径落
  unknown（`worker stopped before a confirmed receipt`）；进程崩溃遗留→下次启动经
  startup 校验落 unknown（`send result unknown after restart; do not resend`）。
- 演习证据形态：替身计数器（每腿恒=1）+ outbox/deliveries/connectors 三表前后 +
  哨兵行身份/收据 sha256（D1/D3 断言全绿）。

## 8. 收据与证据位置

| 产物 | 位置 |
|---|---|
| 回滚收据 `t06-rollback-receipt/v1` | `<work-dir>/rollback-receipt.json` |
| 再前滚收据 `t06-reforward-receipt/v1`（含 secondImportZeroGrowth） | `<work-dir>/reforward-receipt.json` |
| rollback/reforward journal | `<work-dir>/rollback-journal.json` / `reforward-journal.json`（按动词分离；reforward 与 rollback 共用 work-dir） |
| 增量包 | `<delta-out>/<rollbackId>/`（manifest/lineage/payload/receipts/old-root-prepare） |
| 新根备份 | `<backup-dir>/rollback-<rollbackId>/` |
| 前滚（T05）收据 | `<new-root>/legacy-import/migration-receipt.json`（lineage 引用） |
| 探针报告（含 epochStampEvidence） | rollback-receipt `probe` 字段 |

三收据以 `sourceCutoverId` 链配对（rollback.lineage ← 前 migrat收据；reforward.lineage
← rollbackReceiptId）。

## 9. 旧 UI 可读范围（A11「明示哪些项旧 UI 不可立即编辑」）

见增量包 `manifest.json` 映射表（类目→`oldUiReadable`/`editableInOldUi`，均
`mergeBackDefault=false`，J2 默认不回写）：
- 可读不可编辑（默认不回写）：`user/preferences.json`（settings-preserved-format）、
  `knowledge/*.db`（preserved-format-db）。
- 不可读不可编辑：`rust-authoritative-db`（epoch=2 新权威，旧栈无读者）、
  `external-receipts`（A12 闸载体）、`session-files` 新附件、`scaffold-seed`（α1）。
