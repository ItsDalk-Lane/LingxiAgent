from pathlib import Path
import json,hashlib,datetime
O=Path('/private/tmp/rr3-doc-input-boundary-1pgmn8m7')
D=json.loads((O/'document-inputs.json').read_text())
now=datetime.datetime.now(datetime.timezone.utc).isoformat()
entries=[
('R05_REPORT.md','E-audit:62–63、136–141：当前报告文字、历史行顺序及链接；正式stage map没有读取该MD作为状态开关。','现有§11当前结果/交接、真实测试与交付边界。','新E当前/历史/链接核验'),
('R05_INDEPENDENT_REVIEW.md','E-audit:62–63、136–141：当前审查引用、历史原文；标题中的PASS不被G解析。','RR3独立结论索引；仅引用新审查者实际报告，不能作者自签。','新E当前/历史/链接核验'),
('R05_BLOCKERS.md','E-audit:62–63、136–141：当前阻断文字、历史原文。','现有§8当前阻断及真实尚未满足项。','新E当前/历史/链接核验'),
('R05_HANDOFF.json','E-audit:22、41–65、80–123、148–167：严格JSON、rr3_current、accepted_tasks、unresolved_items、allowed_next_scope、consumer_contract、版本、锁与artifact_hashes。','当前状态/证据引用；source_sha及working_tree_digest必须保留准确范围，交付事实另登记。','新E JSON/当前/接口字段/源代码/引用摘要核验'),
('PROGRESS_LEDGER.json','E-audit:33–45、134–135、144–145：严格JSON、rr3_current一致、原历史字段与限定增量。','rr3_current及对应历史快照；不重写旧任务事实。','新E JSON/当前/历史核验'),
('R05_ACCEPTANCE_LEDGER.json','E-audit:33–45、134–135、144–145：严格JSON、rr3_current、旧acceptances保留。不是verify.rs读取的R00 ACCEPTANCE_MAP。','rr3_current；新增真实结果按原记录结构追加，不能重打旧PASS。','新E JSON/当前/历史及证据映射核验'),
('R05_TEST_MAP.json','E-audit:33–45、134–145；E-check:76、86–87：rr3_current、rr3_I10_mapping、历史entries。stage_maps/R05.json:6、262和rebuild_tables.py:40、54、410仅说明；stage_suites实际读取另四份TSV。','rr3_current、rr3_I10_mapping与新历史快照；实际权威TSV不属于E修改范围。','新E JSON/当前/I10具名证据与映射核验'),
('R05_NEGATIVE_GATE_REPORT.md','E-audit:62–63、136–141：当前负测状态/原失败历史。negative_gate.sh汇总写EV/case-results.json，未读取本MD判定16项。','当前默认16实际轮次、历史FAIL、每项来源及复用边界。','新E当前/负测结果逐项核验；若仅回填不自动重跑16'),
('R05_PERFORMANCE_RESULTS.json','E-supplement:7–30：rr3_resource_independent_review及其cycles、points、phases、thresholds、raw_sha256、ranges、cleanup；E-audit:33–45、134。资源Rust测试:1141–1156是常量，不加载本JSON。','rr3_resource_independent_review/rr3_current的真实新对象、新序列、新摘要；历史preregisteredThresholds不追改。','新E原序列复算/阈值前后/当前二进制绑定核验；语义或阈值变更另评估资源重验'),
('R05_LIVE_VERIFICATION.json','E-audit:33–45、134–135：严格JSON、rr3_current和历史字段；negative_gate.sh:591–611的N14改stage map并跑r05_map_declares_no_live_lane，未读本JSON。','rr3_current、rr3_platform_verification；不把离线结果改成LIVE通过。','新E JSON/实际授权与平台边界核验'),
('WORKER_MODEL_BOUNDARY.md','E-audit:66–79：读取全文、抽取首个JSON例子、逐字段对照WorkerCallbackLine及正式源码。G相关Rust测试没有加载这份MD。','仅有新源码依据时修正接口说明；交付边界优先集中在报告/HANDOFF。','新E callback字段/身份/取消/配额与源接口核验'),
('MODEL_USAGE_SEMANTICS.md','E-audit:62–63及E-check:70–73：当前资源/usage证据文字与链接。kernel/usage.rs:8、adapters/models/usage.rs:6、77、usage_families.rs:7均为注释引用。','当前证据边界；公式/未知值/操作ctx等正确语义保持。','新E usage说明与实际字段/公式/存储核验；若改事实契约不能按纯元数据继承'),
('R05_INTERFACE_EVOLUTION.md','真实名称即本名；E-audit:146和E-check:98只检查其整文件hash未变；runs/streaming_norm/anthropic源码引用是注释。E-audit:148起另做MD链接检查。','无新增真实接口演进时保持字节不变；改接口说明需新E逐项源码核对。','新E接口/链接/历史核验；接口实现变化另触发相关G复验'),
('ORCHESTRATOR_PROGRESS.json','E-audit:33–45、143：stages.R05.rr3_current与其他JSON相等；其他stages及tasks保持；E-check:88–92同类核验。','stages.R05当前信息及与实际一致的current_task；不改变R06开工状态。','新E JSON/跨文档当前状态/其他阶段不变核验')
]
by={Path(x['path']).name:x for x in D}
rows=[]
for name,consumer,fields,recheck in entries:
 row=dict(path=by[name]['path'],sha256=by[name]['sha256'],bytes=by[name]['bytes'],currentGateSemanticUse='NOT_FOUND_IN_INSPECTED_REGISTERED_CHAIN',otherRequiredDocumentConsumer=consumer,allowedMetadataPlacement=fields,bindingUse=['B1','B2','B3','C1'],gEvidenceEffect='Whole-candidate identity changes. Metadata-only changes do not by themselves change inspected gate test semantics; reuse requires preserved original manifests and explicit runtime-input equality, new document review, and truthful candidate delta. This is not a product PASS.',minimumRecheck=recheck,finalSection53Coverage='Fresh formal candidate binding and registered positive checks; not automatic new E review, negative fault injections, or full R02 E5.')
 rows.append(row)
(O/'document-boundaries.json').write_text(json.dumps({'utc':now,'scope':'静态读取边界准备，非动态系统调用追踪，非产品独立验收','documents':rows},ensure_ascii=False,indent=2)+'\n')
report=r'''# DOC-INPUT-BOUNDARY-01 — RR3 文档输入边界只读准备

本轮只给准备结论，不是 E 实施、E 独立验收、G/FINAL 验收，也不签产品 PASS。当前仍为 **NOT_ACCEPTED / R06_READY=false**。未运行测试、构建、历史 driver；未修改仓内任何文件、Git 或系统；未派代理、提交、推送、删除原件。

## 结论

在本轮读到的现行 G 默认 N01–N16、正式 R05、注册 R04/R03/R02 命令及其汇总/镜像测试链中，**未发现 E 的这 14 份文档被解析其报告字段来决定产品测试结果**。这不是“文档未被读取”：14 份均为 tracked 文件，全部进入候选复制、全树摘要；R02 full E5 的旧交付测试/生成器也会把它们作为全树字节身份输入。它们的内容还被必需的 E 文档审查真实读取，部分逐字段核对。

因此，E 后续仅回填真实结果元数据时：

1. 旧 G 的原始执行事实仍属于旧候选；**不得声称全树摘要相等，旧 manifest 不改，不把后来的提交 SHA 冒称 tested SHA**。
2. 只有明确列出文档增量、确认产品/门禁实际语义输入（源码、脚本、stage maps、权威 TSV/JSON、配置/fixture、锁等）逐项未变，并经新的文档审查确认只是结果回填后，才可按 RR1/RR3 已规定的“未受影响证据复用”使用该 G 事实。本轮不代替那次实际比较或批准复用。
3. 任何变化都使旧 E 对这些文件的 hash/一致性结论失去当前效力，必须新 E 实施收据及全新 E 审查；不能机械重跑写死旧轮状态的 E-REVIEW-02 driver 并把其预期失败当新产品缺陷。
4. 若改的是约束/阈值/接口语义，或实际语义输入而非结果元数据，不能仅以扩展名为文档而继承 G。按影响重验；FINAL §5.3 不会自动重跑默认16故障注入。

## 候选、读取范围与可信边界

- 本地分支：`codex/rust-tauri-migration`；HEAD 与本地 origin 跟踪引用均为 `b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`。没有网络查询远端；不把跟踪引用说成新鲜远端回执。
- 工作区确有未提交 RR3 内容，包含 13 份 E 文档、生产与脚本改动；接口演进文档为第14份所有权文件，本轮没有预设必须修改它。独立 `git ls-files --error-unmatch` 证实14份均 tracked。
- 全文读 RR1/RR2 MASTER、RR3_BRIEF/HANDOFF/E_R3_BRIEF/FINAL_BRIEF、本地 AGENTS，并读 E 原 brief 与 DELIVERY-PREP-02 的 REPORT/REFRESH_RULES。
- `read-commands.jsonl` 保存受记录命令的真实 argv、UTC、退出码、完整 stdout/stderr、直接文件 SHA。`document-inputs.json` 保存14份整文件字节读取的 SHA/size/UTC 及字段/标题索引；不是只靠文件名搜索。`stage-command-graph.json` 来自实际四张 stage map 的完整 JSON 解析。
- `source-read-stability.json` 对本轮61份已读关键输入作最后逐项 hash 核对，记录过的读入 hash 无差异。它是本轮有限只读范围，**不是全树冻结或 J02 运行绑定证明**。未重复哈希历史二进制、缓存、大日志。
- 搜索范围含现役 `scripts/rust-tauri/`、`rust/` 源码/测试/xtask、根 `tests/`、`.sync-audit/`、文档目录里的可执行检查器；实际追踪了 stage argv、内联 Python assembler、JSON装载、glob/rglob/read_dir/os.walk、故障变异目标、旧交付消费者。仅阅读少量指定历史 E/交付 driver 的代码，未执行它们。
- 记录的3个 exit 2 均为定位时给出尚未确认路径导致的读取/搜索不存在项，不是测试失败（独立 assembler 文件实际不存在，已定位为 stage_suites 内联 Python；另两个历史候选文件名亦查正）。初次工具里的 brief/定位探测未带自建UTC回执，随后已按回执重新读取所有必读内容；工具会话保留初次输出。另有未展开的 shell glob 探测失败，没有运行任何 driver。QA 不把它们当门禁。

## 真实调用关系

### G 默认16

`r05_t08_negative_gate.sh:52–73` 调 `prepare_git_copy.py` 构造 HEAD 副本，`:67–69` 覆盖 `rust/scripts/docs/rust-tauri`，再准备 Node。`:93–109` 的唯一变异清单包含 stage_maps/R04/R05、生产 Rust、测试、pins TSV、stage_suites；**没有14份 E 文档**。

`:264–282` 先跑 `xtask r05_` 镜像控制；`:285–292` 跑正式二进制控制。N02/N04 调真实 `r05_t08_stage_suites.sh`；N01/N03/N07/N13/N14验证stage/pin注册；N08–N12/N15跑指定生产测试；N06/N16实际运行 `verify-stage R02`。`:227–260` 汇总读取本次 EV 的 tsv/exit 文件并写 `case-results.json`，不读取 E 的负测报告、验收账本或 LIVE 状态。

N14实际变异的是 `stage_maps/R05.json` 新增LIVE command，并跑 `r05_map_declares_no_live_lane`（`:591–611`），不是将 `R05_LIVE_VERIFICATION.json.status` 当开关。N13改 R04 stage map 的 deferred 字段（`:565–589`），不是改 E 验收账本。

### 正式 R05 及前序

`stage_maps/R05.json:7–111` 注册 fmt/clippy/workspace/contracts/boundaries、R05 suites、R04 regression；R04 再注册 R03 与 RR1 repair；R03 注册选定 R02 shell 检查及 G07 repair。`main.rs:59–62` 嵌入真实 stage maps；`verify.rs:1779–1841` 由场景引用构造运行顺序并运行 argv。

**需保留实际范围差异**：当前 R03 map 的前序是逐项 R02 脚本，不是另一个完整 `verify-stage R02`；其 legacy argv 明确带 `R02_LEGACY_REGRESSION_MODE=directed-no-seal-family`。因此正式 R05 的注册闭包会经过 R02 既有脚本，但不能把该事实说成它又跑了 R02 map 的全部20项或 full E5。G 的 N06/N16 调的才是完整 R02 stage map。该区别见 `stage-command-graph.json`，不改变本轮权限或原必需义务。

### assembler、动态读取与真正文档语义输入

- `r05_t08_stage_suites.sh:94–108` 读取 `r05_stage_pins.tsv / r05_stage_cids.tsv / r05_required_cids.tsv`；`:113–145` 核必需CID；`:246–318` 从当轮 pin/cid/cases/log 组装结果；`:332–370` 读取 `r05_leaf_case_map.tsv` 并逐具名测试组装 leaf cases。无递归遍历 R05 所有 JSON，也没有加载 `R05_TEST_MAP.json`。
- `xtask/stage_map.rs:2898–2926、3206–3209、3323–3326` 实际读取上述四份 TSV；`:3442–3475` 真实 JSON 装载 `R05_SCOPE_MATRIX.json` 的 `supplemental_leaves[].id/r00_execution_stage_ids/disposition/r07_remainder` 等字段。它不属于 E14。
- `verify.rs:110–118、149–203` 真实加载 **R00** `ACCEPTANCE_MAP.json.scenarios` 与 `FEATURE_STAGE_ACCEPTANCE.json.supplemental_scenarios`。不要把它们混同 E 的 `R05_ACCEPTANCE_LEDGER.json`。
- `verify.rs:468–490、631–651` 按 stage map evidence path 装载当轮结构化结果；`:1745–1775` 检查当轮目录新鲜度和叶证据。四张 map 的输入路径已经解析，不指向 E14。
- R03/R04 matrix 的 `glob("*.json")` 仅位于本次证据的 cases/combinations 目录（`r03_t08_matrix.sh:78–99`、`r04_t08_matrix.sh:81–86`）；不是 glob 文档目录。
- R02 supplemental 脚本的 rglob：限定 Rust src、CLI/测试源码，或隔离 home 的状态快照（`r02_cli_rust_matrix.py:50–75`、`r02_client_leaf_matrix.py:55–90`、management/static/cli_sessions 的 SOURCE 列表与结果 JSON读取）。没有动态装载 E14 作为业务配置。
- `r02_t04_storage_tx.sh:368` 和 `r02_registry_consistency.py:36` 实际读取 **R02** storage registry；`r01_t01_check_ownership.py:119–124` 的实际权威是 R01 OWNERSHIP_TARGET/DEPENDENCY_RULES 和 R00 FEATURE_INVENTORY/STORES，Rust遍历在src范围。contracts脚本 `r01-t02-check-generated.sh:39–48` 检查生成协议及 R01 API_COMPAT_MATRIX。
- `prepare_git_copy.py:70–82、93–132` 枚举 Git tree并读取blob/模式/字节以构造精确副本；这是复制身份，不解释 R05 JSON状态。Node准备器 `r05_t08_prepare_node.py:15、30–57、61–97` 遍历范围为 `node_modules`、package/lock/.npmrc，不是docs。
- `runner_identity.rs:8–70、103–141` 检验嵌入 runner 字节及 xtask src 下 rs/json 清单；其 read_dir 限定 xtask 源目录。
- 资源测试 `r05_t08_resources.rs:1141–1156` 的 RSS/FD/负载是 Rust const。性能JSON名称出现在说明注释里；实际 socket/token/runtime config读取与该报告无关。旧 `r05_t08_closed_loop.rs:2164` 也只是阈值出处说明。

以上实际文档权威必须继续保护，**不能全排 docs**。本轮没有生成完整生产输入最终冻结列表，也没有替未来 E/FINAL 确定所有未知动态输入。

## 共用字节读取依据

表中使用下列代码简称；14份均适用：

- **B1 — xtask候选绑定**：`candidate.rs:179–249` 由 `git ls-files --cached --others --exclude-standard` 枚举后对每文件实际读取 SHA/模式，内容没有 JSON/Markdown 语义解析。`:51–144` 仅排除本轮新 evidence 根及真实精确输出fd；tracked docs不能当输出排除。`main.rs:365–499` 前/逐命令/后快照；不稳定会影响最终状态。
- **B2 — legacy候选镜像绑定**：`r02_t08_legacy_entry_regression.sh:511–553` 用 Git status枚举全部脏/非忽略新增，逐文件hash；`:744–789` 记录主候选并与完整 CoW副本比较。因此当前13份已改文档进入脏字节绑定；第14份若修改也进入。未改文件由HEAD身份/副本保护。
- **B3 — full E5全树交付身份**：同脚本`:2844–2864` 在full模式执行npm；`tests/round2-delivery-evidence.test.ts:54–82、95–104、170–205`、`round3-delivery-evidence.test.ts:63–80` 由Git清单列出全树并按manifest字节/SHA比较；除f1-f12生成证据/patch等规定排除外，E14均在范围。两个历史patch生成器也按index/blob内容摘要、对当前与HEAD拒绝漂移（round2 `create-delivery-patch.py:59–75、119、167–181、404–449`；round3 `create-round3-patch.py:114、162–183、404–449`）。这是全树身份语义，不解析E报告结果字段。脚本`:2844–2857`明确directed跳过E5；旧 raw npm红及其分类不得改写成绿。
- **C1 —复制**：negative`:67–69`覆盖docs；prepare_git_copy与legacy完整副本读取。它们不是“报告输出写入”，也不是产品配置装载。
- **E-audit**：只读阅读的历史 `artifacts/rust-tauri/R05/RR3/E-REVIEW-02/independent-audit.py`；**E-supplement**为同目录 `supplementary-audit.py`；**E-check**为`E-02/check.py`。它们证明 E 审查确有字段/内容消费者，不代表后续应复用旧写死状态driver。

## 逐文件消费边界与复验

**每一行共同结论**：在上述注册G/产品链，语义输入字段均为“未发现”；B1/B2/B3/C1均成立。只回填报告元数据不会自动改变被测业务行为，但会改变旧候选字节身份；G复用需前述逐项相等与差异收据。新E核验是必需。最终§5.3能对新候选重新绑定并跑注册正向检查，**不自动完成新E核验、默认16目标故障注入或R02 full E5**。

| E所有文件 | 实际其他必需消费者、被读内容 | 建议回填范围 | 受影响最小复验 |
|---|---|---|---|
'''
for name,consumer,fields,recheck in entries:
 report+='| `'+by[name]['path']+'` | '+consumer.replace('|','／')+' | '+fields.replace('|','／')+' | '+recheck.replace('|','／')+'；共同G/§5.3边界如上。 |\n'
report+=r'''
## 回填后的最小处理与不能省略的部分

- 元数据回填：保存E修改前/后14份hash及精确diff；新增收据明确旧G的testedSha、完整candidate digest、运行脚本/权威表/锁/fixture等实际输入清单及其相等结果。保留候选全树观察，不将它裁剪为“原候选完全相同”。所有新增辅助脚本也必须入输入清单。
- E文档检查：严格JSON/重复键、当前跨文档一致、原历史保留、链接及SHA、本地/远端引用边界；HANDOFF/worker/usage涉及接口时对源码与原规格；性能/I10对真正新对象与原始序列；负测报告对真实默认16及新增负测。另一个全新E审查者出结论。
- 若触碰机器权威：pins/cids/required/leaf map影响镜像与producer；scope影响其具名xtask测试；stage map/runner/复制/Node准备/legacy影响相应G准备及N06/N16等执行。应将具体变化交总控安排相应重验，不能拿本报告预先豁免。若不能隔离影响，登记UNKNOWN并保守重验受影响链，不虚构“最小集合已证明”。
- 原§5.3仍须由全新FINAL真实执行。正常cargo/workspace并不执行negative_gate.sh默认16；R03注册的directed legacy不补跑full E5。因此真正受影响的负测/完整R02证据，不能仅以“后面还有FINAL”来替代。
- 在G/FINAL实际运行期间任何tracked文档变化均可能触发B1稳定性失败；J02当前静默边界仍必须遵守。此次准备报告归档自身也会新增全树候选内容，须在总控确认静默窗口后进行，不能运行中归档。

## E应如何写清本地原件与远端交付边界

已完整读取 DELIVERY-PREP-02 的 REPORT 和 REFRESH_RULES。本轮不更新其旧manifest、不编辑E、不生成最终暂存名单。

PREP02处理的是旧批次：768项=257纳入原始证据+511仅本地，另558补正；合成旧19,734路径中拟纳入13,828、仅本地5,906；新排除原件附SHA/size，旧缓存身份明确继承，未对所有旧缓存重新hash。这个范围不含新J/G/E/FINAL，也不是最终安全扫描或远端可达证明。大于普通GitHub单文件交付限制的134,457,480字节rlib、实际binary及本机工具链接仍有本地边界；历史原件可重跑不等于可得到同字节原件。

建议 E 按以下位置登记（新增字段为建议，不冒称现行schema已有）：

| 位置 | 应记录内容 |
|---|---|
| `R05_REPORT.md`现有§11或新的当前交付段 | 原始证据、仅本地原件、缓存/故障副本、真实Git交付回执的区分；引用新最终边界索引；准确说明当前是拟交付还是已提交/推送。 |
| `R05_HANDOFF.json`既有`source_sha_semantics / working_tree_digest_scope` | tested SHA和工作树真实范围；完整候选摘要与运行输入摘要分别保留；后续报告补录差异不能伪装成旧测试SHA。 |
| HANDOFF新增建议`rr3_current.delivery_boundary`（若采用，其他共享`rr3_current`处同步） | `snapshot_ref`、`snapshot_sha256`、`enumerated_at`、`status`、`local_originals_ref`、`references_with_delivery_boundary_ref`、`remote_receipt_ref`、`unresolved_items`。终版索引尚未形成时写待完成，不复制PREP02数字冒充最终。 |
| HANDOFF既有`artifact_hashes`及新交付收据 | 可追加新边界索引/新最终清单的真实hash；旧hash/manifest保持历史事实。每个排除原件写`originalPath/sha256/bytes/source_or_driver/localOnly=true/remoteOriginalAvailable=false`；实际远端可取需要独立真实回执，未查写UNKNOWN或false。 |
| `R05_INDEPENDENT_REVIEW.md`当前索引、`PROGRESS_LEDGER.json.rr3_current`、`ORCHESTRATOR_PROGRESS.json.stages.R05.rr3_current` | 新E/FINAL各自结论与核验范围；交付复核状态可列在同一delivery boundary引用，不能用拟纳入名单制造远端已交付。ORCH其他阶段/tasks不动。 |
| `R05_NEGATIVE_GATE_REPORT.md`当前段、`R05_PERFORMANCE_RESULTS.json.rr3_resource_independent_review` | 运行时使用了哪个本地二进制/副本/原始序列，其SHA和留存范围；新边界索引说明哪些只在本机。不要为解除断链而删除旧证据引用。 |
| `R05_BLOCKERS.md`当前段及HANDOFF`unresolved_items` | 只有用户确实要求远端必须取得的原件仍不可取，或最终必须引用对象无法实际核验等真实未满足项才登记具体缺口；不把每个可重建缓存排除都变成产品阻断。 |
| `R05_TEST_MAP / ACCEPTANCE_LEDGER / LIVE_VERIFICATION` | 通过当前公共证据引用指向新边界；保留原测试/原验收/LIVE授权事实。不要为交付清单无关重排大账本。worker/usage/interface说明不承载最终拟暂存列表。 |

最终J/G/E/FINAL都停写后须重新实际枚举新增和变动文件，分类依据实际driver读写位置，逐项处理票据/数据库/home/用户内容及未明来源，不直接把PREP02名单用于stage。旧原件保留本地，旧manifest不得改写；实际提交/推送后另取Git/远端回执。

## UNKNOWN及本轮没有证明的事项

- 本轮是静态源码/命令关系核查，未做系统调用追踪，也没运行G/E/FINAL。不能声称在本轮机器上动态捕获了“全部open路径”。
- 未来新E/FINAL driver、后续J脚本变化、环境选择的工具链/第三方构建依赖可能带来新消费路径，本结论只绑定记录的源码hash。已读61份稳定不代表整个仓库/所有Git对象稳定。
- 若后续通过额外argv/environment将某份E JSON设为输入，或新增消费者，必须重评。当前已登记链没有这种配置；不凭可能性预造新审批。
- 本报告不证明旧G能在新候选复用，也不证明当前E内容正确或G默认16已通过；只为后续真正比较提供边界。没有验证远端交付可达、没有完成最终秘密扫描、没有决定最终暂存集合。
- R06保持false；J/G/E/FINAL与必要环境项尚需实际结果，不能提前放行。

## 本轮产物

- `document-boundaries.json`：14份逐文件机器表。
- `document-inputs.json`：14份内容SHA/大小/UTC与真实字段索引。
- `stage-command-graph.json`：四张stage map真实命令登记。
- `read-commands.jsonl / read-inputs.jsonl`：读取命令、UTC/退出及直接输入SHA。
- `source-read-stability.json / READ_QA.json`：有限读取范围最后核对及命令统计；统计截点在finalizer之前，完整后续读取收据仍保留。
- `read_audit.py / inspect_inputs.py / finalize_inputs.py / write_report.py`：本轮仅在临时目录运行的读取/整理程序；不是产品driver。
- `MANIFEST.json`：本轮文件摘要（manifest自身不自引用）。

报告写完即停写；由总控等当前真实测试窗口结束后归档。报告生成UTC：''' + now+'\n'
# 先写清单，最后一次写入才保存REPORT；完成后不再修改任何输出。
manifest=[]
for p in sorted(O.iterdir()):
 if p.is_file() and p.name not in ('MANIFEST.json','REPORT.md'):
  b=p.read_bytes();manifest.append({'path':p.name,'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()})
rb=report.encode();manifest.append({'path':'REPORT.md','bytes':len(rb),'sha256':hashlib.sha256(rb).hexdigest()})
(O/'MANIFEST.json').write_text(json.dumps({'utc':now,'selfExcluded':'MANIFEST.json','files':manifest},ensure_ascii=False,indent=2)+'\n')
(O/'REPORT.md').write_bytes(rb)
print(json.dumps({'report':str(O/'REPORT.md'),'sha256':hashlib.sha256(rb).hexdigest(),'documents':len(rows),'stoppedWriting':True},ensure_ascii=False))
