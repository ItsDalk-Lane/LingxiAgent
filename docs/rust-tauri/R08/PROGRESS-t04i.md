# R08-T04 LegacyImporter 与未知字段保护（A07/A08）台账 — t02-core

- Owner: t02-core；开工 2026-10-23；证据前缀 `t04i-*`（单次目录，红证据保留）
- 写域（任务卡）：rust/crates/lingxi-service/src、rust/crates/lingxi-adapters/src、
  rust/crates/lingxi-service/tests、scripts/rust-tauri/r08_t04_*.mjs、本台账、T04_DESIGN.md
- 冻结令：rust/ 随 T04 开工解除为上述写域；其余目录不碰

## 开工前必查 ①：migration/network-guard-negative 真红定性（只读调查，不动修复）

结论：**真红成立，定性为「Node 24.16 上 http.Agent 外连路径整体绕过用户态猴子补丁，
真实外连尝试静默悬挂」——守卫在该版本的覆盖面声明失效，属 R00 时代守卫的环境性失效，
非 R08 引入。修复属 R00/migration 域授权，T04 不动。**

证据链（全部只读/本地探针，零修复）：

1. 既有红证据复核：`artifacts/rust-tauri/R08/t03-batchf-vitest-01/output.log` ——
   该文件 7 测试仅「blocks http.request through http.Agent -> net.createConnection
   （绕过类②）」红，`Test timed out in 60000ms`，其余 6 个（net/tls/dns 直面）全绿。
   即：**绕过类②的 http.Agent 面失守，其余直面补丁仍有效**。
2. 探针① `t04i-netguard-probe-01`（exit 1，探针自身标记异常崩溃属预期）：
   - `http.Agent.prototype.createConnection === net.createConnection` = **false**
     （bootstrap 期已解耦，非同一函数引用）；
   - 但同一进程内**第二次**补丁赋值后，`Agent.createConnection (node:_http_agent:240)`
     确实同步调到了用户态补丁（标记异常穿透 http.request 同步抛出）。
3. 探针② `t04i-netguard-probe-02`（后台跑挂已 kill，output.log 已取证）：
   - 守卫同款按 host 判定补丁下 `http.request(example.com)` → **{"threw":false}**
     （补丁未被触达或参数形态不含可判定 host）；
   - `net.connect('localhost',1)` **能**触达补丁 dns.lookup（lib/net 对 dns 是动态解析）。
4. 探针③ `t04i-netguard-probe-03`（exit 0）：补丁带日志 → 日志未打印、无同步抛 →
   agent 路径未调用补丁；真实 createConnection 起跑（process.exit(0) 截断）。
   诚实注记：该探针 exit 前可能发出过一次 example.com 的真实 DNS 查询
   （无 HTTP 载荷、连接未完成），只读调查的最小代价，如实登记。
5. 探针④ `t04i-netguard-probe-04`（exit 2，决定性，零外发设计：双层补丁+双层日志+
   10s 哨兵）：
   - `[L1] patched net.createConnection` 未打印；`[L2] patched dns.lookup` 未打印；
   - 无同步抛；10 秒内无 error/timeout 事件 → **真实外连尝试静默悬挂**。
   - 形态与 10-22 红证据（60s 超时、其余测试绿）完全吻合。

定性裁决（不动修复的理由与边界）：

- 失守面精确：仅「http.Agent → createConnection → 真实外连」这一层；net/tls/dns
  直面补丁在当前 Node 仍有效（7 测试中 6 个绿）。
- 机制：_http_agent 内部对 createConnection 的引用/封装在 bootstrap 期与用户态
  补丁解耦（身份断言 false）；且悬挂形态说明连 dns.lookup 补丁都未被该路径触达——
  即当前 Node 的 http 外连栈对用户态猴子补丁整体不透明。
- 非确定性说明：探针①（首跑、同进程第二次赋值）曾观察到补丁被同步触达；②b/③/④
  三次一致复现绕过+悬挂。主导形态=绕过+悬挂，与红证据一致；①的反常如实登记。
- 对 T04 的直接教训（写进设计）：**A07/A08 夹具与本任务一切隔离声明不得依赖
  network-guard 作为「零外发」证明**；改用合成 HOME + 只读源根 + 导入器本身不发起
  任何网络调用的结构性论证 + 外部协议确定性替身。
- 去向：立为**基线红家族 #3**（R00/migration 域真红；编号经 Lead 2026-10-23 更正：
  #1=composer-send/R02RustWebSocket vitest 2 红、#2=persistence 三件套 10 红、
  #3=本项），修复授权归 R00/migration 域（Lead/用户裁决），本任务只完成定性。

## 开工前必查 ②：knowledge-indexes 取证口径

- PROGRESS.md L480-481 注记：knowledge-indexes 在 69 类清单内，夹具计数对实际库文件
  取证，不以扫描器位点清单为准。将在设计④切片计划中落实（逐类 current_path 实文件
  计数，不使用 scan-persistent-stores 的声明写 token 清单）。

## 设计阶段（2026-10-23 01:50 交付待批）

- 已读输入：STORES.json 69 类全表（counts 与分类语义复核一致：
  authoritative 42 / rebuildable_cache 13 / adjacent_compatible 14；
  registry_exemptions 38）、R01 矩阵全量切换策略
  （preserve-format 35 / regenerable 13 / switch-to-new-authority 11 /
  epoch-mechanism 4 / shell-local 6）、R02-T04 注册表（新写点 1 项 +
  incumbent 未动声明）、11 个实转类+session-files/model-observability/
  cron/knowledge 的权威 reader 位点。
- rust 源复核要点（全部只读）：session_tree SeedSession/AppendMessage/
  ForkRequest/unknown_payload（assert_fresh_identity → 导入幂等吸收层必须在
  importer 侧）；lib.rs:1310 bind_legacy_session_root(config.data_home)
  （正式导入不走此绑定）；main.rs:652 epoch gate fail-closed；
  migrations.rs verify_receipts/apply_all（V27 只追加新表合规）；
  legacy_session_files.rs（Batch/Transition 归档恢复机，T06 域不碰）；
  import_legacy_sidecar（version=1、sf_ 保留、别名、**缺逐附件坏因**→
  设计③ ImportLegacyReportV2 扩展）；observability import_legacy（只读开库、
  同 call 幂等、形状不符显式 400）；import_current_studio（只读 cron 归档）；
  legacy_plugins（识别+告知不迁移）。
- 设计核心裁决（详见 T04_DESIGN.md）：
  ① 离线 CLI 子命令 `lingxi-service legacy-import --source --target [--resume]`，
  不开常驻 HTTP 导入端点（单一权威读取路径 + A08 诚实崩溃模型 +
  lib.rs:1310 警告落实）；② V27 三表簿记（runs/batches/issues）+
  批次=(类,源文件)、小块=500 行/事务、done 批次复核哈希后跳过、
  in_progress 安全重跑、同键冲突记 issue 不覆盖；③ 收据三件套
  migration-receipt.json + issues.jsonl + SUMMARY.md，sessionfiles
  扩展逐附件坏因；④ 11 类实转（切片 A 会话主干 / B 观测自动化 /
  C 运行态登记），35 类原样登记+哈希，13 类丢弃收据，10 类 out-of-scope；
  ⑤ 合成夹具一棵树覆盖 fork/附件/旧trace/未知块/损坏尾行/坏附件 +
  A08 stop-after-batch 与 kill -9 双轮恢复；⑥ legacy_unknown 六条边界
  （run_id=NULL 不造虚构、未知块 quarantine+标记消息、未知字段透传、
  opaque 字节保留不展示、坏尾行导入前缀不丢整会话、坏附件 status=broken
  不丢会话、冲突 error 级不合并）。
- PPG-1/2/3 T04 部分裁决提案：「无交集」（导入不改写 preview 写腿/UI 状态域，
  夹具不含 preview 工件），待 Lead 确认后定案；T05 采用时复核。
- 网络隔离声明修正：A07/A08 不依赖 network-guard（见上方定性），
  改用结构性论证（合成 HOME+只读源+importer 无网络调用面）。
- 实现批次计划 B1..B4 见设计末节，每批独立 t04i-* 证据。

## 批准与绑定条件（Lead 2026-10-23 01:5x）

设计批准，B1→B4 推进。绑定条件：
1. sessionfiles 扩展纯增量：ImportLegacyReportV2 与 V1 并存，V1 响应形状一字节不变；
   B2 证据含受影响 R07 服务套件复跑（sessionfiles/files 域，r07_stage_inputs.py
   串行窗口——执行前先与 Lead 确认窗口空档）。
2. 夹具生成器同种子逐字节可复现；B1 证据带「连跑两次 sha 全等」自证。
3. A08 须证明「无 --resume 直接重跑半导入目标根」被 fail-closed 拒绝。
4. 基线红家族编号：#1=scoped vitest 2 红、#2=persistence 三件套、
   #3=network-guard-negative（已更正上方表述）。
PPG「无交集」裁决确认（T05 采用时复核保留）。

## B1：V27 迁移 + 扫描计划 + manifest/checksum + 收据骨架 + 夹具生成器

### B1 实现落点（2026-10-23 02:30 前后，机器时）

- **V27 迁移**：`migrations.rs` 追加 `r08_legacy_import_bookkeeping`（import_runs / import_batches / import_issues + 状态索引），语义与批准设计 §2 一致；既有 12 个 migrations 单测全绿（版本号动态取 `supported_version()`，无 26 硬编码）。
- **新模块 `storage::legacy_import`**（adapters 内，4 文件）：
  - `classes.rs`：69 类静态表由注册表机械生成（R01 matrix sha 458bb2da… + R00 STORES sha 6067bdd3…，161 模式；含生成命令与「勿手改」注记）；模式→正则编译缓存（OnceLock）；具体度=字面字符数（占位符不计），`plugin-data/mcp` 胜过 `plugin-data/{pluginId}` 已单测钉住；`(os-tmpdir)` 类标记 external 不参与匹配。
  - `manifest.rs`：递归遍历按文件名排序（与文件系统返回序无关）；sha256+size；符号链接/特殊文件跳过分级登记（skipped_symlink / skipped_special）；`source_root_sha256` = 逐行 `{sha}  {size}  {relpath}` 的 sha256。前后各一次，字节一致即 A08「源文件哈希不变」证据面。
  - `plan.rs`：批次=（store_class, 源文件），batch_key=`{class}/{relpath}`，字典序排序；未映射文件单列 + unmapped-class warning（绝不静默跳过）。
  - `mod.rs` + `receipts.rs`：`run(ImportArgs)` 全管线——真实 `~/.lingxi` 拒止 → manifest(before) → 目标闸门 → run/batch 簿记 → 非 convert 批次即刻登记 done → manifest(after) → 收据三件套重写。收据从簿记表取数（幂等覆盖写）；issue 内容级幂等（同 kind+ref+message 不重复登记，resume 翻倍已修）。
- **fail-closed 目标闸门**（Lead 条件③）：不存在/空目录→创建；非空且无 runs.db→拒；有 runs.db 无 `--resume`→拒（exit 2）；resume 未知 run→拒；run 不可续（completed 且无 pending convert）→拒。单测 `rerun_without_resume_is_refused_fail_closed` + CLI 冒烟双重钉住。
- **resume 语义**：同 run_id 续跑；批次 `INSERT OR IGNORE` 不重复；done 批次复核源 sha（变→重置 planned + `source-changed-between-runs` warning；消失→`source-vanished-between-runs`）；issue 序号从 MAX(seq) 续。两次 resume 后 import_batches 仍 31 行、import_issues 仍 1 行（冒烟实录）。
- **CLI 动词**：`lingxi-service legacy-import --source <old> --target <new> [--resume <runId>]`，raw_args 阶段处理（先于 tracing/服务栈/网络），成功 stdout 一行 JSON，拒绝/错误 exit 2 + stderr。Importer 不绑定 legacy session root（lib.rs:1310 警告语义保持）。
- **夹具生成器 `scripts/rust-tauri/r08_t04_fixture_gen.mjs`**（Lead 条件②）：mulberry32(种子)+递增计数器，禁 Date.now/Math.random；32 文件覆盖 A07 全风险面（fork 对、无边界会话、未知块×2、损坏尾行 811+截断、sidecar 二件套、附件 2 好 1 截断（描述符声明全长）、归档会话、观测库、会话清单库、knowledge、7+ 运行态小文件、epoch 两件、插件运行态、1 个未映射文件）。**双跑 `diff -r` 为空、rootHash 全等**（bbfd5185…）。
  - 设计裁量注记：附件元数据放 `session-files/<h>/<名>.meta.json`（与载荷同目录），因 R00 注册表无 `sessions/*.files.json` 模式——该放法会落入 unmapped。截断判定=描述符声明 size vs 实际载荷长度。
- **收据状态语义**：convert 批次 B1 保持 planned → status=`"partial"`；全部 convert done 后转 `"complete"`（B4 终态）。

### B1 证据

| 证据目录 | 内容 | 结果 |
| --- | --- | --- |
| t04i-cargo-test-01 | `cargo test -p lingxi-adapters --lib legacy_import` | exit 0，13/13 绿 |
| t04i-cargo-migrations-01 | `cargo test -p lingxi-adapters --lib migrations` | exit 0，12/12 绿（V27 并入收据链） |
| t04i-cargo-build-01 | `cargo build -p lingxi-service` | exit 0，零警告 |
| t04i-cargo-storage-01 | `cargo test -p lingxi-adapters --lib storage::` | exit 0，40/40 绿 |
| t04i-fixture-determinism-01 | 夹具同种子双跑 + `diff -r` | exit 0，**逐字节全等**（条件②自证） |
| t04i-cli-smoke-01 | 端到端：fresh run（31 批/13 done/18 planned/1 issue/partial）→ 无 resume 重跑拒（exit 2）→ 未知 run 拒（exit 2）→ resume×2 幂等（批次/issues 不翻倍）→ DB 状态与收据三件套实录 | exit 0 |

开发期迭代（同一性已复核、非正式证据）：fixture 首版 27 批 → 修正 sidecar 文件名与附件布局后 31 批、未映射仅剩刻意的 `mystery-unmapped.bin`；resume issue 翻倍缺陷修复后幂等。

### B1 遗留（移交 B2/B3/B4，无阻塞）

- convert 批次执行器（切片 A/B/C）未动——按计划。
- `source-manifest.before/after.json` 已落盘 `{target}/legacy-import/`；字节比对在冒烟中体现为 receipt `sourceUnchanged: true`，独立 diff 证据并入 B4 A08 双轮。
- 全量集成测试套（tests/*.rs）复跑属 Lead 回归窗口，B1 未触碰其行为面（V27 纯追加）。

## B2 切片A：session-jsonl/sidecars/manifest 实转 + sessionfiles 扩展（2026-10-23 完成）

### 实现落点

- **converters（前段已验收）**：`convert_sessions.rs`（session-jsonl→canonical
  session_tree 写路径；runBoundary 接受 `phase` 或 `kind`；unknown-block→
  `legacy-unknown-block` 标记 + quarantine `.raw`；损坏尾行前缀保留 + 尾段 quarantine；
  缺失 Run 边界→run_id NULL + `missing-run-boundary` info）、sidecars/manifest 实转。
- **sessionfiles 扩展（Lead 条件①，纯增量）**：
  - `adapters/storage/session_files.rs`：`SessionFileStatus::Broken` 第 4 变体
    （wire/parse "broken"；status 列无 CHECK 约束，穷举匹配编译期钉住）——
    坏附件绝不以 available 冒充可读。
  - `service/sessionfiles.rs`：`ImportLegacyBrokenEntry` + `ImportLegacyReportV2`
    （`{imported, skipped_existing, broken:[{fileId, reason}]}`；V1 形状一字节不变，
    测试 `v1_report_shape_byte_identical` 钉死）。`import_legacy_sidecar_v2` 在
    V1 同等解析/租约/碰撞/refs 之上加 payload 校验：读不到→status=missing +
    `payload-missing`；声明 size/sha256 与实字不符→status=broken +
    `payload-truncated`；行级 version≠1→`unsupported-version`；refs 仅挂 available。
  - `import_legacy_session_files_dir`：旧根 `session-files/{hash}` 目录整体导入——
    枚举 `*.meta.json`（排序确定性）→ payload **逐字节保真**复制进 canonical
    cache（确定性命名 `legacy-{fileId}-{filename}`，源永不改动）→ 合成 v1 sidecar
    → v2 校验落库。描述符损坏→`sidecar-row-invalid`（其余行不受影响）；混合
    sessionId→BadSidecar 响亮拒绝。
  - `plan.rs`：目录级 session-files **convert 批次**（目录含 ≥1 `.meta.json` 时），
    batch_key=`session-files/session-files/{hash}`；既有逐文件 register 批次保留
    （字节回执面不动）。
  - `mod.rs` 编排面：`pending_session_files_dirs` / `SessionFilesDirOutcome` /
    `finalize_session_files`——批次落账（done/failed）、每坏行 `bad-attachment`
    warning（词表内 kind，坏因写 message）、收据重写 + run completed。
    **账目口径修正**：payload-missing/truncated 的行已登记（计入 imported），
    仅未插行坏因（sidecar-row-invalid/unsupported-version）另计 planned——
    不重复计数（t04i-cli-import-03 发现 planned=2 过计，-04 修正后 1/1/1）。
  - `main.rs`：adapters 线程 join 成功后 → 挂起目录查询 → service 阶段
    （SessionFileService 打开目标库，异步直接在 main runtime）→ finalize →
    打印最终 EngineOutcome。阶段错误 → stderr + exit 2（响亮）。
- **问题清单词表扩展**：corrupt-line / fork-target-missing / unmapped-shape /
  batch-error / manifest-mismatch / sidecar-orphan（B2 converters 使用）+
  bad-attachment（sessionfiles 坏因行）。

### B2 切片A 证据

| 证据目录 | 内容 | 结果 |
| --- | --- | --- |
| t04i-cargo-test-11 | `cargo test -p lingxi-adapters legacy_import`（plan.rs 目录批次后） | exit 0，**19/19 绿**（t04i-cargo-test-10 同跑输出被管道截断，保留为 exit-0 回执） |
| t04i-sfv2-test-01 | 新集成套件 `r08_t04_sessionfiles_v2`（9 用例：好 payload/缺失/截断/sha 不符/描述符损坏/行级版本/顶层版本硬拒/重跑幂等/V1 形状钉死） | exit 0，**9/9 绿** |
| t04i-cli-import-03 | 真实夹具全管线（首次含 session-files 阶段） | exit 0；34 批 done 25 pendingConvert 9 issues 6 status partial；发现 sfh-ef rows_planned 过计（2 应为 1）→ 修正 |
| t04i-cli-import-04 | 修正后复跑（fresh target） | exit 0；34 批 done 25 issues 6；**目录批次 1/1/1 planned=done**；SOURCE_UNCHANGED=yes；bad-attachment warning refId=file-broken-1 |
| t04i-a07-assert-03 | A07 断言包扩展版（18 项：会话/fork/未知块/损坏尾行 + **附件实转面**：3 行登记、broken 不冒充、附件字节=源字节、截断字节保真、收据 pendingConvert/sourceUnchanged） | exit 0，**18/18 PASS**（-02 为重建断言脚本列名偏差 3 红，-03 修正后全绿，红证据保留） |
| t04i-r07-suite-sessionfiles-01 | 串行窗口内复跑 `r06_t05_session_files`（r07_stage_inputs --run-suite） | exit 0（90.6s，result.json 回执） |
| t04i-r07-suite-desk-files-01 | 串行窗口内复跑 `r07_desk_files`（files 域） | exit 0（69.2s，result.json 回执） |

**Lead 条件①闭合**：扩展纯增量（V1 形状钉死测试 + V2 并存）；受影响
sessionfiles/files 域服务套件串行复跑双绿。

### B2 切片A 遗留（移交 B3/B4，无阻塞）

- pendingConvert 9 = 切片B（observability/cron）+ 切片C（7 运行态）+ legacy_plugins/epoch
  登记面——B3 实转后 status 转 complete。
- A08 半程终止双轮（--stop-after-batch + kill -9 + 字节比对）在 B4。
- 收据 `phase` 字段文案仍标 B2 切片A，B3 落地后更新。

## B3：trace/cron/runtime-state/plugins/epoch/knowledge 六面（2026-10-23 完成）

### 夹具版本演进（v1→v2）

- v1（/tmp/t04i-fixture-01，已废）：无可观测库实表、无 cron-jobs；B2 证据基于 v1。
- v2（/tmp/t04i-fixture-02a，B3 收口基准，rootHash `40395742…5832df`，33 文件）：
  - 新增 `model-observability/observability.sqlite` 真实 sqlite（model_calls 12 列 +
    model_call_usage 5 列；5 条确定性行：p1 全 usage/c1 子级 error 无 usage 行/u1 无 trace
    无 session 终态 NULL/p2 部分 NULL usage/n1 aborted 全 NULL usage 行；iso() 以
    BASE_MS=1_760_000_000_000 等差生成）。
  - 新增 `agents/a1/desk/cron-jobs.json`（3 任务：legacy-job-1 schemaVersion 3
    agent_session 每小时、legacy-job-2 schemaVersion 4 direct_action notify、
    legacy-job-3 schemaVersion 4 plugin_action disabled）；无 cron-runs 历史（默认空）。
  - 批次面变化：cron-jobs.json 新增 1 个 register 批 → batches_total 34→**35**；
    其余批次划分不变（convert 12+9 / register 10+1 / out-of-scope 2 / regenerable 1）。
- 条件②确定性重证（最终版）：t04i-fixture-det-02 双跑 rootHash 全等 + `diff -r` 空。

### 切片与实现

- **切片B-α（trace 面）**：读取逻辑从 service 逐字迁入
  `adapters/storage/legacy_import/observability.rs::read_legacy_observability_rows`
  （错误串逐字一致；service 端点变薄壳 bad_query 包装）；写入复用
  `run_store.import_legacy_model_calls`（INSERT OR IGNORE 28 列）+
  `observability_legacy_trace`（R07-T10 权威链，parent_call_id 还原）；结果记入收据
  扩展 `legacyTrace`（per-store sha256/imported/skipped/traceRows/status）。
- **切片B-β（cron 面）**：`legacy_import_cron_phase`（main.rs）——
  `has_register_done_batches(cron-automation)` 门控；复用
  `automation_import::import_current_studio` 同一实现链（JobApi：storage=目标
  runs.db、MemoryService 绑源 home 零写已验证、SystemClock、RandomRequestIdGen、
  studio "legacy-import"）；Blocked 逐条 error issue（unmapped-shape）响亮；
  结果扩展 `legacyCron`（imported/alreadyPresent/blocked/entries）。
- **切片C（runtime-state 面）**：`runtime_state.rs`——8 个运行态类（Convert 策略）
  逐字节归档至 `legacy-import/runtime-state-archive/{relpath}`（已存在且字节相等=幂等
  跳过；不同=响亮拒覆盖）；键登记进 V28 `legacy_runtime_state_registry`
  （INSERT ON CONFLICT DO UPDATE 幂等）；解析失败仍归档+warning issue（unmapped-shape）；
  扩展 `runtimeState` 标「已登记、消费端待对应任务」，不发明消费语义。
- **切片D（plugins/epoch/knowledge）**：`legacy_import_surfaces_phase`——
  warn_if_legacy_plugins 同一探测链（检出逐名 info issue
  legacy-plugins-detected；夹具无 plugins/ → detected=[] 如实）；目标根落
  **stamp=1**（epoch.rs 新增 `write_legacy_import_stamp`，同一 write_stamp 实现链，
  批准设计①第5条原文）；knowledge.db 存在即取证（只读打开、sha256+每表行数），
  夹具为 knowledge/ 目录无该文件 → present=false 如实。
- **V28**：`import_run_extensions` + `legacy_runtime_state_registry`（纯追加，
  Lead 显式批准；migrations 实测 12/12 全绿，apply_then_verify 幂等含 V28）。
- 收据：`write_receipts` 自读扩展表（ORDER BY ext_key），非空才写 `extensions` 键；
  EngineOutcome 形状不变。

### B3 证据

| 证据目录 | 内容 | 结果 |
| --- | --- | --- |
| t04i-cargo-test-15 | `cargo test -p lingxi-adapters --lib legacy_import` + `migrations` | exit 0，**23/23 + 12/12 绿** |
| t04i-cargo-test-16 | service lib：automation + epoch 过滤 | exit 0，**17/17 + 16/16 绿** |
| t04i-cargo-test-13 | adapters lib 全量 | **163 过 / 1 红**：仅 `browser::cdp a_stalled_pipe_write…`；git stash 至 HEAD 复跑同红 → **基线红，非本次引入**（本切片未触碰 browser/） |
| t04i-fixture-det-02 | v2 夹具同种子双跑 + diff -r | exit 0，rootHash 全等 `40395742…5832df`，DETERMINISM-OK（条件②最终版重证闭合） |
| t04i-cli-import-05 | 夹具 v2 全管线（fresh target /tmp/t04i-target-05） | exit 0；**35/35 done、pendingConvert 0、issues 6、status complete、sourceUnchanged true** |
| t04i-b3-assert-02 | 新增断言包 `r08_t04_b3_assert.mjs`（20 项：收据形状/扩展 6 键/tip=28/5 usage 行/父子链 observability_legacy_trace 4 行/cron 3 任务/登记 10 行 8 类/归档逐字节相等+sha 钉源/stamp=1/插件与 knowledge 如实面/sourceUnchanged） | exit 0，**20/20 PASS**（-01 断言脚本误查不存在的 model_calls 表 1 处崩，修正为 observability_legacy_trace 后全绿，红证据保留） |
| t04i-r07-observability-02 | `r07_t10_observability` 复跑（r07_stage_inputs --run-suite） | exit 0（-01 为直跑缺 LINGXI_HOME 2 红，保留；窗口预批「到时即批，一套一跑」） |
| t04i-r07-automation-import-01 | `r07_automation_import`（隔离 LINGXI_HOME） | exit 0，**8/8 绿** |
| t04i-cli-resume-02 | 对已完成 run 执行 `--resume <runId>` | exit 2 响亮拒：`not resumable (state=completed, no pending planned batches)`——**fail-closed 预期行为**；真正半程续跑双轮在 B4（-01 为 --resume 缺值用法错，保留） |

### B3 遗留（移交 B4，无阻塞）

- A08 完整证据：--stop-after-batch 半途停 + kill -9 + `--resume` 续跑 + 行数单调无重复
  主键 + resumedFromRunId 链 + 源清单前后字节比对（B4）。
- 条件③ resume-without-pending fail-closed 已由 t04i-cli-resume-02 单点证明，B4 以
  e2e 双轮复证。
- 夹具无 plugins/ 目录与 knowledge.db 文件，δ 两面当前为负路径（detected=0 /
  present=false）如实记录；若后续需要正路径证据，须先报 Lead 批夹具 v3。

## B4：A08 半程终止与续跑（2026-10-23 完成）

### 设计要点（T04_DESIGN 批准版落地）

- **`--stop-after-batch <k>` 测试注入**：`BatchBudget{remaining,stopped}` 在每个批次
  执行前 consume；预算耗尽 → run 标记 `interrupted`、收据 `status:"partial"`，main.rs
  跳过全部 5 个收尾阶段直接 pretty-JSON 退出。预算只覆盖 run() 内部批次（35 中的 24 个
  convert 类在引擎内）。
- **kill -9 复核**：main.rs 在构造 ImportArgs 前先取得实例锁并以
  `instance_lock_held=true` 宣誓；resume 闸门见 `state=running`+锁持有 → 判定
  interrupted（kill -9 释放 flock，重新取得锁即证明无存活写入方）+ 大声记录
  `run-recovered-from-kill`（severity warning, batch_key "run"）；无锁持有 →
  RunNotResumable。exit 3 HeldByPeer 另路。
- **scaffold 感知空目录闸门**：prepare_layout 会先生成
  `lingxi-service/{instance.lock,instance.json,instance.stale.json,tmp/}`，朴素空目录
  检查会把全新目标误判 TargetNotEmptyUnknown；新增
  `target_holds_only_instance_scaffold` 白名单判定（t04i-cli-stop-02 暴露 → 修复 → -03 绿）。
- **receipt `resumedFromRunId`**：条件字段（仅 resumed=true 时写），receipt-only，无 V29。
- **stdout `interrupted` 字段**：EngineOutcome 附加布尔，main.rs 据此跳过收尾。

### B4 证据（终版 binary，全部重采于语义修正后）

| 证据目录 | 内容 | 结果 |
| --- | --- | --- |
| t04i-cargo-test-18 | `cargo test -p lingxi-adapters legacy_import`（终版 binary 语义） | exit 0，**25/25 绿**（含 `stop_after_batch_interrupts_cleanly_then_resume_finishes`、`kill_recovery_requires_instance_lock`） |
| t04i-cli-import-07 | 夹具 v2 全新基线（/tmp/t04i-target-09） | exit 0，35/35、issues 6、complete |
| t04i-cli-stop-04 | Round A：`--stop-after-batch 5`（/tmp/t04i-target-06） | state=interrupted、5 done/30 planned、收据 partial、resumed=false、无 resumedFromRunId、hash 稳定 |
| t04i-b4-assert-11 | stopped 模式断言包 | exit 0，**15/15 PASS** |
| t04i-cli-stopresume-02 | Round A：`--resume` 续跑 | exit 0，35/35、resumed=true、同一 run id、source_unchanged |
| t04i-b4-assert-12 | resumed 模式 vs 基线 target-09 | exit 0，**27/27 PASS**（含 issues 多重集相等、resumedFromRunId 链） |
| t04i-cli-import-08 | bulk 源全新基线（v2+400 批量 jsonl，/tmp/t04i-target-08） | exit 0，435/435、issues 5206 |
| t04i-cli-kill-03 | Round B：kill -9（计划早期捕获） | state=running、88 planned → resume（killresume-02）435/435，assert-13 **27/27** |
| t04i-cli-kill-04 | Round B：kill -9 深层捕获 | state=running、**75 done / 1 in_progress / 359 planned**（窗口发现：0.3–0.7s 落在 manifest 扫描期必 NO_DB，5s 窗口稳定进入转换期） |
| t04i-cli-killresume-03 | 深层 kill 后 `--resume` | exit 0，435/435、issues 5207=5206+1 kill-recovery、source_unchanged |
| t04i-b4-assert-14 | killed 模式 vs 基线 target-08（含 in_progress 批次重跑幂等） | exit 0，**27/27 PASS** |
| t04i-cli-rerun-refuse-01 | **条件③ e2e 复证**：对已完成 target-06 裸重跑（无 --resume） | raw exit 2，响亮拒：`target already holds an import database; pass --resume <runId> (fail-closed, no silent new run)` |
| t04i-receipt-final-01 | kill 轮收据终审 | resumedFromRunId 链完整（round1 收据无该键 → round2=run id）、run-recovered-from-kill ×1、435/435 策略分解、sourceUnchanged true |

### B4 调试中暴露并修复的两个幂等缺口（assert 脚本迭代驱动）

1. **会话时间列非源确定性**：bulk（legacy_unknown）会话的 `created_at_unix_ms` 取导入时刻
   → 跨 run 比对须归一化全部 `_unix_ms/_at_ms` 列；identity 哈希列（如
   session_files 内容哈希）保持全等，作为内容同一性证据。
2. **missing-run-boundary 触发不对称**（真实现缺口）：原条件 `imported+already_present>0`，
   干净跑中「整份隔离为 unknown 块」的会话不发 info；kill 后重跑 in_progress 批次时首条
   标记消息计入 already_present → 重跑补发 → 诊断集不对称（assert-10 捕获：
   bulk-0008 多出 1 条 info）。修复：触发条件并入 `quarantined_unknown`，干净跑与重跑
   发出完全相同的诊断集（bulk 基线 issues 4806→5206，+400 为该修正的预期增量）。
   语义修正后两轮证据全部重采（上表即终版）。

### 归一化比对口径（assert-vs-baseline 哲学，沉淀规则）

跨 target/跨 run 比对必须归一化 run-bound 列：`run_id`、`detail_json`（in_progress 批次
重跑改写 alreadyPresent 计数，设计允许的记账差异）、`legacy_file_paths_json`（源路径绑定）、
`archived_at`、全部 `*_unix_ms/*_at_ms`、issues `seq`；路径列做 target 根替换（含 macOS
`/tmp`→`/private/tmp` realpath 变体）。**幂等的证明载体**：identity 哈希列全等 + 内容列全等 +
主键无重复 + issues 多重集相等（kill-recovery 条目除外）+ 行数单调 + 源清单字节不变。
哈希口径澄清：receipt.sourceRootSha256 是导入器自有清单算法（v2=`e22999be…`、
bulk=`40f42dc1…`），与 fixture_gen 的 rootHash（`40395742…5832df`，stdout-only）是
两套算法；v2 锚定由 t04i-fixture-a08-02 承载。

### B4 遗留（移交 T05+，无阻塞）

- 收据 `phase` 文案停留在 "R08-T04 B3 (slice A…)" 字符串（receipts.rs 自由文本标签，
  断言不校验；cosmetic），建议 T05 前顺手改为 "R08-T04"。
- kill 轮 in_progress 批次重跑的 `detail_json` 记账差异（alreadyPresent 计数）为设计允许，
  已在归一化口径登记；数据表行数/主键/哈希已证无重复。
- 基线红家族未回归确认：t04i-cargo-test-18 仅跑 legacy_import 过滤（25/25）；
  browser::cdp 红属基线家族 #4，本切片未触碰 browser/。

## 串行窗口台账

- **持有**：2026-10-23（Lead 授权，自 B1 验收通过起）——用途：R07 sessionfiles/files 域服务套件串行复跑（条件①）；纪律 r08_stage_inputs.py --run-suite 一套一跑 + r08_run.py + t04i-* 前缀；窗口期间他人禁跑（Lead 知会）。
- **归还**：2026-10-23 04:16 CST——sessionfiles 扩展落地（t04i-sfv2-test-01 9/9）+ 受影响套件复跑双绿（t04i-r07-suite-sessionfiles-01 / t04i-r07-suite-desk-files-01 均 exit 0），窗口按授权条款自动归还。
