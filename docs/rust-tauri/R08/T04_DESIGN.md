# R08-T04 LegacyImporter 与未知字段保护（A07/A08）— 设计

- Owner: t02-core；状态：**待 Lead 批准**；台账：`PROGRESS-t04i.md`
- 任务书：`Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R08_旧Electron客户端接入与数据迁移演练.md` L167-207
- 输入注册表：R00 `STORES.json`（69 类，sha 6067bdd3…）、R01 `DATA_COMPATIBILITY_MATRIX.json`（切换策略）、R02-T04 `R02-T04_STORAGE_REGISTRY.json`（新内核写点已登记、incumbent 未动声明）
- 责任切分（任务书 L209-264）：**T04 = 分类/导入/校验/收据**；T05 = 全局停写/快照/采用/切换；T06 = 归档/恢复/前滚。本设计不越界做采用与切换。

---

## ① 导入器架构落点（模块/命令面/端点选型与理由）

### 决策：离线 CLI 子命令，不开常驻 HTTP 端点

新增模块树（全部在已授权写域内）：

```
rust/crates/lingxi-adapters/src/storage/legacy_import/
  mod.rs            — LegacyImportEngine：编排 + 批处理/恢复状态机
  plan.rs           — 扫描旧根 → 69 类分类 → 批次计划（确定性排序）
  session_jsonl.rs  — 旧 JSONL 逐行解析器（容忍截断尾行、未知块）
  sidecar.rs        — session-meta/titles/files sidecar 适配（复用 import_legacy_sidecar 行映射）
  observability.rs  — 旧 model-observability sqlite 只读映射（复用 import_legacy_model_calls）
  registry_copy.rs  — preserve-format 类：原样登记+哈希+schema_ref 校验（不转写）
  receipts.rs       — 迁移收据/问题清单生成
rust/crates/lingxi-service/src/bin 或 lib 内新 verb:
  lingxi-service legacy-import --source <old-home> --target <new-home> [--resume <runId>]
```

选型理由（逐条对照任务书红线）：

1. **单一权威读取路径（必交付⑤）**：导入产物是新隔离根里的一份转换副本；
   服务启动路径只读 `RunDatabase`/`canonical_message_store`。若做成运行中端点，
   导入期会出现「旧根直读 + 新库读」双读路径。离线 CLI 天然单写者。
2. **A08 半程终止重跑**：CLI 进程即导入事务的全部生命周期；kill = 最诚实的
   中途崩溃模型。HTTP 端点下服务存活而导入半死，状态边界更复杂。
3. **不触碰真 ~/.lingxi / 原始文件不可覆盖**：`--source` 只读打开
   （JSONL 只读字节流；sqlite 用 `SQLITE_OPEN_READ_ONLY`，observability 现有
   入口即此范式）；`--target` 必须是不存在的目录或空目录，由 importer 创建，
   从不写回 source。
4. **lib.rs:1310 警告的落实**：正式导入**不走** `bind_legacy_session_root`
   （该绑定会把归档/恢复/删除操作指向旧根下旧 JSONL）。CLI 直接把旧根当
   字节源，把新根当唯一写目标；新根的服务启动配置里 legacy root 绑定为
   新根内受控材料（archive 子目录），永不是旧根。
5. **epoch 门禁协同**：main.rs:652 `coordinate_data_epoch_startup` 对
   data_epoch=1 的 home fail-closed。CLI 在**目标新根**上创建 stamp=1 的
   干净 epoch 元数据（T05 采用前的候选材料），不参与旧根的 epoch 状态机
   ——那是 T05 的域。CLI 自身在打开目标库前也走同一 epoch 检查函数，
   防止对已被采用的根重复导入。
6. **复用已验证写路径，不发明第二条写链**：
   - 会话谱系：`session_tree::seed_session` / `append_message`
     （保留 ID/parent/run_id 可空外键；缺 run 不造虚构行——正是
     legacy_unknown 的库层语义）；fork 用现有 `ForkRequest` 模型登记谱系。
   - 附件：`SessionFileService::import_legacy_sidecar`（version-1 sidecar，
     sf_ id 逐字节保留 + legacy 别名）。**需扩展**：逐附件坏因报告（见③）。
   - 旧 trace：`import_legacy_model_calls`（同 call ID 幂等、冲突拒）。
   - cron：`automation_import::import_current_studio`（只读旧 cron、登记归档）。
   - 插件：`legacy_plugins::warn_if_legacy_plugins`（识别+告知，不迁移）。
7. **服务端点只加一个只读验收面**（A07 证据链末端用）：
   导入完成后对**目标新根**正常启动服务，用现有只读端点（sessions/files/
   observability）出「单一权威读取路径」证据。**不新增导入端点。**

## ② 批处理 + 进度 + 暂停恢复 + 幂等重跑 + checksum 模型

### 迁移 V27：导入簿记表（新表只追加版本，符合 migrations.rs 契约）

```sql
-- V27 legacy_import_bookkeeping
CREATE TABLE import_runs (
  run_id TEXT PRIMARY KEY,           -- 铸造：import:{source_root_sha8}:{start_unix_ms}
  source_root TEXT NOT NULL,
  source_root_sha256 TEXT NOT NULL,  -- 整个源根清单哈希（文件路径+大小+sha256 的规范哈希）
  state TEXT NOT NULL,               -- planned|running|interrupted|completed|failed
  created_at_unix_ms INTEGER NOT NULL,
  finished_at_unix_ms INTEGER
);
CREATE TABLE import_batches (
  run_id TEXT NOT NULL REFERENCES import_runs(run_id),
  batch_key TEXT NOT NULL,           -- 稳定键：{store_class}/{源文件相对路径}
  store_class TEXT NOT NULL,
  strategy TEXT NOT NULL,            -- convert|register|regenerable|out-of-scope
  source_relpath TEXT NOT NULL,
  source_sha256 TEXT,                -- 单源文件哈希（目录类为清单哈希）
  state TEXT NOT NULL,               -- planned|in_progress|done|failed|skipped
  rows_planned INTEGER, rows_done INTEGER,
  detail_json TEXT,                  -- 类专属计数（附件数、trace 行数…）
  PRIMARY KEY (run_id, batch_key)
);
CREATE TABLE import_issues (
  run_id TEXT NOT NULL REFERENCES import_runs(run_id),
  seq INTEGER NOT NULL,
  batch_key TEXT NOT NULL,
  severity TEXT NOT NULL,            -- info|warning|error
  kind TEXT NOT NULL,                -- corrupt-tail|bad-attachment|unknown-block|conflict|…
  ref_id TEXT, message TEXT NOT NULL,
  PRIMARY KEY (run_id, seq)
);
```

### 批次与进度

- **批次粒度 = (store_class, 源文件)**。JSONL 一个会话文件一批；sqlite 一库一批；
  sidecar 一 agent 一批；目录类（regenerable/preserve-format 目录）一目录一批。
- 批内再分**确定性小块**（JSONL 每 500 行一事务），使 kill 落点可控、恢复重试
  单元小。进度：每完成一小块更新 `import_batches.rows_done`，stdout 打 JSON 行
  （`{"progress":...}`），供 T08 消费。
- 计划确定性：文件清单按相对路径字典序排序（复用 sidecar 导入的确定性排序
  范式），两次扫描同根必得同计划。

### 暂停恢复（A08 核心）

- 启动时 `--resume <runId>` 或自动发现目标根内唯一 `interrupted|running` 态 run：
  - `done` 批次：**跳过**，但先复核 `source_sha256` 与当前源文件一致
    （不一致 → 该批次降级 `planned` 重跑，并记 issue `source-changed-between-runs`；
    这是诚实裁决，不静默）。
  - `in_progress` 批次：回 `planned` 整体重跑（批内小块事务边界=已提交部分
    由幂等规则吸收，见下）。
  - `interrupted` run：进程收到 SIGTERM/SIGINT 时把 running→interrupted 再退出
    （cleanup/finally 中唯一允许的 signal 处理）；被 kill -9 则启动复核时
    发现 `running` 但无活锁持有者 → 判 `interrupted`（实例互斥 instance.rs 保证
    同一目标根同时只有一个 importer/服务）。
- **幂等重跑**（INSERT 非幂等的吸收层）：
  - 会话：`seed_session` 前查存在性；存在且关键列一致 → skip 并计数
    `already-imported`；存在但冲突 → 不覆盖，记 issue `conflict`（响亮）。
  - 消息：以 `(session_id, message_id)` 查重；同键同内容 skip，同键异内容
    issue。`append_message` 的 run_id 外键照旧。
  - 附件：`import_legacy_sidecar` 现有「同 id 同 identity 跳过、异则
    ForkCollision」语义即幂等；扩展后逐附件坏因进 issues 表。
  - trace：`import_legacy_model_calls` 同 call ID 幂等、冲突拒（现成）。

### checksum 模型（A08 证据原料）

1. **导入前**：遍历源根（跳过 socket/fifo），每文件记
   `relpath + size + sha256`；汇总成 `source_root_sha256`（规范序哈希）存入
   `import_runs`。快照写入目标根 `legacy-import/source-manifest.json`。
2. **导入后**：重新计算同一 manifest 算法，与导入前逐文件比对 →
   「源文件哈希不变」的 A08 证据 = 前后两个 manifest 的 byte-identical 声明
   （附两 manifest 的实际字节）。
3. **转换完整性**：每批次 `rows_planned`（源解析出的条目数）vs
   `rows_done`（入库+已存在跳过合计）必须可解释（损坏行单独计数），
   差额非零且不在 issues 中说明 → 批次 `failed`，整体收据标红。

## ③ 迁移收据与问题清单格式

目标根产出三件（路径固定）：

1. `legacy-import/migration-receipt.json` —— 机器可读总收据：
```json
{
  "version": 1,
  "runId": "import:…",
  "sourceRootSha256": "…", "sourceManifestSha256": "…",
  "sourceUnchanged": true,
  "startedAtUnixMs": 0, "finishedAtUnixMs": 0,
  "resumedFromRunId": null,
  "classes": {
    "<store_class>": {
      "strategy": "convert|register|regenerable|out-of-scope",
      "batches": 3, "filesPlanned": 3, "filesDone": 3,
      "rowsPlanned": 1200, "rowsImported": 1180,
      "rowsAlreadyPresent": 0, "rowsQuarantined": 20,
      "unknownBlocks": 4, "issues": 2
    }
  },
  "attachments": {"imported": 9, "skippedExisting": 1, "broken": 1},
  "traces": {"importedCalls": 40, "skippedExisting": 2, "traceRows": 12},
  "totals": {"issues": {"info": 0, "warning": 3, "error": 0}},
  "epoch": {"targetStamp": 1, "gateChecked": true}
}
```
2. `legacy-import/issues.jsonl` —— 问题清单（逐行一条，机器可筛）：
```json
{"seq":1,"batchKey":"session-jsonl/agents/a1/sessions/s2.jsonl","severity":"warning","kind":"corrupt-tail","refId":"line:812","message":"final line truncated mid-JSON; 41 bytes quarantined to legacy-import/quarantine/…"}
```
   `kind` 受控词表：`corrupt-tail | bad-attachment | unknown-block | conflict |
   missing-run-boundary | unsupported-sidecar-version | source-changed-between-runs |
   unmapped-class`。
3. `legacy-import/SUMMARY.md` —— 人类摘要（逐类计数表 + 问题前 20 条 + 复跑指引）。

**sessionfiles 扩展**（A07 附件坏因的必要增量，写域内）：
`import_legacy_sidecar` 当前只报 imported/skipped。新增
`ImportLegacyReportV2 { imported, skipped_existing, broken: Vec<{file_id, reason}> }`
，坏因受控：`payload-missing | payload-truncated(sha mismatch) | sidecar-row-invalid |
unsupported-version`。旧 `ImportReport` 形状保留兼容（R06 既有调用点不炸）。

## ④ 69 类策略映射执行切片计划

R01 矩阵 69 类分五族；T04 对每族的动作：

| 族 | 数量 | T04 动作 |
|---|---|---|
| switch-to-new-authority | 11 | **本轮实转**（转换副本进新隔离根） |
| preserve-format | 35 | **原样登记**：枚举实文件+sha256+schema_ref 抽查校验，不转写（T05 采用时同格式直用） |
| regenerable | 13 | **登记+丢弃收据**：计数、`rebuild_or_loss_semantics` 落收据，不复制 |
| epoch-mechanism | 4 | **out-of-scope**（T05 epoch 协调器所有），收据登记排除 |
| shell-local | 6 | **out-of-scope**（壳本地不迁移），收据登记排除 |

11 个实转类的执行切片（按依赖序分批实现，每批独立 t04i-* 收据）：

- **切片 A（会话主干，A07 主体）**：session-jsonl → 谱系/消息/fork 关系；
  session-manifest-sqlite → sessionId↔locator 校验（不一致记 issue）；
  session-sidecars → meta/titles/files（含附件 payload 校验与坏因）。
- **切片 B（观测与自动化）**：model-observability-db → import_legacy_model_calls；
  cron-automation（preserve-format 族内唯一的既有转换入口
  `import_current_studio`，归档式登记）→ 复用现入口出收据。
- **切片 C（运行态登记类）**：workflow-state、subagent-state、
  terminal-session-state、execution-leases、loop-state、
  deferred-result-state、plugin-task-registry、conversation-map-layout
  → 这些是 JSON/小目录运行态：实转语义=**保留原始记录逐字节归档 +
  键（runId/sessionId/leaseId…）登记进新库登记面 + 未知字段原样保留**；
  不发明新运行语义（新内核读取面尚未全部落地，如实标注「已登记、
  消费端待对应任务」）。
- knowledge-database / knowledge-indexes：前者 preserve-format 实文件取证
  （knowledge.db 实库 sha256 + 表计数），后者 regenerable 丢弃收据；
  **计数对实际库文件取证，不用扫描器位点清单**（PROGRESS L480-481 口径）。

## ⑤ A07/A08 夹具设计

合成旧根 `$TMPDIR/t04i-fixture-<n>`（永不指向真 ~/.lingxi；脚本内硬断言
路径前缀）。A07 夹具一棵源树覆盖六类情形：

```
fixture-home/
  agents/a1/sessions/
    s-fork-parent.jsonl        — 含 fork 源谱系 + fork_point 标记
    s-fork-child.jsonl         — fork 子会话（父边界消息引用）
    s-legacy-unknown.jsonl     — assistant 条目无 user task 边界（缺 Run 边界）
    s-unknown-blocks.jsonl     — 未知 entryType 块 + 已知条目的未知字段 +
                                 opaque 签名字节块（不直接展示，原样保留）
    s-corrupt-tail.jsonl       — 前 811 行完好，末行截断（mid-JSON）
    session-meta.json / session-titles.json / files sidecar(version=1)
    attachments/…              — 正常 payload ×2 + 截断 payload ×1（坏附件）
  model-observability/observability.sqlite — v1 形状调用行 + trace 行（旧 trace）
  agents/a2/sessions/s-archived/archived 下一份 — archived 分支
  knowledge/knowledge.db       — preserve-format 取证对象
  knowledge/indexes/…          — regenerable 对象
  security/execution-leases.json 等运行态小件 ×4
  data-epoch.json / plugins/old-plugin/ — epoch 族 + legacy_plugins 识别对象
```

A07 断言（逐类计数/摘要/引用校验）：fork 父子关系在 `canonical_message_store`
读取面可还原；未知块进归档且计数=收据 unknownBlocks；附件链接可解析
（sf_ id + legacy 别名双查），坏附件报 broken 且会话不被整体丢弃；
无重复会话（ID 集合比对）。旧 trace 计数与 observability 端点一致。

A08 夹具 = 同树 + 确定性断点：importer 支持 `--stop-after-batch <k>`
（测试注入；在批次边界干净退出、run 态落 `interrupted`）。
两次跑法：(1) 首跑中断后 `--resume`：done 批次行数不变（不重复），
in_progress 批次安全重试；(2) kill -9 后恢复：run 态复核路径。
断言：前后 source manifest 逐字节一致（哈希不变）；目标库行数单调、
无重复主键；收据 `resumedFromRunId` 链完整。

## ⑥ legacy_unknown 边界规则

1. **缺 Run 边界**：旧 JSONL 中没有可对应「用户任务→run」边界的条目，
   导入时 `run_id = NULL`（库层可空外键，参照完整），条目级标记
   `legacy_origin=unknown-run-boundary` 入 detail_json + issue
   `missing-run-boundary`（info 级）。**绝不发明用户任务/run 行。**
2. **未知 entryType 块**：不解析、不丢弃——原始 JSONL 行字节进
   `legacy-import/quarantine/unknown-blocks/<session>/<line>.raw`，
   同时在对应会话追加一条 `entry_type='legacy-unknown-block'` 的标记消息
   （content_json 含原块 sha256 + quarantine 路径 + 原 type 名），
   计数入收据 unknownBlocks。对齐 R06 ex11 语义
   （`ex11_legacy_unknown_fields_raw_and_opaque_are_preserved_or_explicitly_refused`）。
3. **已知条目的未知字段**：content_json 原样携带（serde 透传），不做
   字段裁剪；导入后读取面见得到原字段。
4. **协议签名 / opaque 块**：字节级保留进 quarantine + 标记消息指向，
   **不在任何读取面直接展示**（任务书原话）；收据单列 `opaqueBlocks`。
5. **损坏尾行**：导入完好前缀；截断余量字节进 quarantine；
   issue `corrupt-tail`（warning，带行号与字节数）；**不整会话丢弃**。
6. **坏附件**：附件记录照常入库（sf_ id 保留）但 status=broken +
   reason（payload-missing/truncated），issue `bad-attachment`；
   会话与其余附件不受影响。
7. **冲突**（同 ID 异内容）：不覆盖、不合并，issue `conflict`（error 级），
   批次标 failed 待人工裁决——沉默合并是红线。
8. **网络隔离**：A07/A08 与一切导入证据**不依赖 network-guard** 作零外发
   证明（已定性：Node 24.16 上 http.Agent 面绕过守卫，见台账）；
   隔离论证 = 合成 HOME + 源根只读 + importer 代码面无网络调用
   （cargo 依赖审计面：importer 模块不引入 http/net 客户端）。

## PPG 边界注记（T04 应交班裁决）

PPG-1/2/3 的 T04 部分：导入不改写 preview 写腿与 UI 状态域；夹具不含
preview 工件。T04 侧裁决=「无交集」，留 T05 采用时复核。已登记台账待
Lead 确认。

## 实现批次（批准后）

1. **B1**：V27 迁移 + plan/scanner + manifest/checksum + receipts 骨架
   （夹具树生成器 `scripts/rust-tauri/r08_t04_fixture_gen.mjs`）。
2. **B2**：切片 A（session_jsonl/sidecar/sessionfiles 扩展）+ A07 主体夹具。
3. **B3**：切片 B + C + legacy_plugins/epoch 登记面。
4. **B4**：恢复状态机 + A08（stop-after-batch + kill -9 两轮）+ 收据终审。
每批跑 `r08_run.py --evidence artifacts/rust-tauri/R08/t04i-<name>-<seq>`，
收据 tail 带 `{"exitCode":N,"sourceBindingsStable":true,"evidence":…}`。
