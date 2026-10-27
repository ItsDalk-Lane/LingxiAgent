# R00-T07 独立验收 R2（ZCode）

验收代理：REVIEWER-R00-T07-R2（一次性全新独立验收代理，ZCode，未参与本候选执行、R1 验收或 R1 修复）
候选：R00-T07「建立可执行账本」（执行者报告 `R00-T07_REPORT.md`，R1 结论 PASS 附 D-1 必须/D-2 建议；修复报告 `R00-T07_REPAIR_R1.md`，修复自评 READY_FOR_R2_REVIEW）
Task base/HEAD：`4401afae2ee48df1512b8a8c9a1262be0efbe64f`（分支 `codex/rust-tauri-migration`，实测一致）
验收时点：2026-09-24（本机 macOS 27.0 darwin arm64，Python 3.14.3，Node v24.16.0）

## 结论

**R00-A13：PASS。R00-A14：PASS。任务 R00-T07 验收结论：PASS。**

R1 修复项 **D-1 已正确修复**、**D-2 已正确修复且无新回归**；修复后的同一候选全部必需条件达成。核心依据：候选 72 文件逐文件/聚合 SHA 与执行报告 §10 清单**零差异**；R2 以**全新探针**（不复用执行者代码、换用不同场景 ID 与不同非法值）独立复验 R1 反例①②③全部被拒（exit 1、错误行精确到 ID/字段），合法 FAIL/BLOCKED/STALE 记录均不被误拒（exit 0），现成套件 18/18+正面对照复跑通过，真实仓库严格模式校验 `LEDGER_VALID checks=14829 scenarios=952 results=14 entries=832`。

## 1. 验收范围与执行方式

- 只审查候选，未修改任何候选文件、总控账本或其他仓库文件；未 commit/push/PR/tag/release/建分支/worktree/子代理；本报告是唯一新增文件。
- 全部试验在 `/tmp/t07_review_r2/`（自测复跑、SHA 探针、独立反例探针）与 `tempfile.mkdtemp` 隔离副本执行，候选零触碰（验收前后两次实算 72 文件聚合 SHA 不变，见 §2）。无网络、无真实供应商、无付费 API、无外发、无真实用户数据。
- 完整阅读：AGENTS.md、任务书 00/01–06、R00 阶段书（T07/A13/A14 全文）、acceptance-catalog（200 场景与 R11-A14 条件授权条目）、执行报告全文（含 §13 修复增补）、R1 验收报告、修复报告、三工具脚本全文（validate_ledger 496 行 / selftest 614 行 / build_map 554 行）、R00-T07_RESULTS.json、BLOCKERS.md、ACCEPTANCE_MAP.json（结构化独立核对）、三套自测证据（顶层 / confirm-rerun / confirm-rerun-r1）。不采信报告结论，全部独立重算/复跑。
- 环境约束核实：本机 Mimosa 写入钩子确实拦截 Bash 直写与部分动态 argv subprocess 形态（本轮亲身复现），执行者声明的「进程内调用校验器」环境约束属实；校验器 CLI 形态经本轮真实仓库静态命令行实测可用（exit 0）。

## 2. 候选完整性与哈希独立重算

| 项 | 期望（总控/报告声称） | R2 独立实算 | 一致 |
|---|---|---|---|
| 执行报告 SHA-256 | `9580f612c7e78f0f6c2a981730ab9173138baecc85a438895f1f9c0b531913f8` | 相同 | ✅ |
| 修复报告 SHA-256 | `e1adab94763564cdd6ca1baeaab3b25210aa3435adc7bdf9b8eae59eb99e97fd` | 相同 | ✅ |
| 72 文件逐行 SHA | 报告 §10 清单 72 行 | **72/72 逐行一致，0 差异**（清单按路径排序） | ✅ |
| 聚合 SHA（SHA256+双空格+路径+换行 拼接） | `197371176b209f27be1968b6180f4a9fa5ef30501484427e53e93d9c914d9360` | 磁盘实算与清单文字重算**均为该值** | ✅ |
| 磁盘候选集合 | 72 文件（docs 6 + T07 artifacts 66） | `rglob` 实际枚举恰 72 文件，与清单集合**精确相等**（无多列/漏列） | ✅ |

**D-1 修复核实**：报告 §10 中 `confirm-rerun/a13-v3-evidence-tampered.log` 行现值 = `aa3ec8e344c444a1e495f2976db0f18877ab366414af2ba2226f1b31f36ef83b`（`af2` 变体），与磁盘实测值、R1 §10 要求的更正值**三方一致**。清单文字重算与磁盘实算聚合同为 `197371176…`，即 R1 发现的「清单文字与磁盘脱节」已消除。

**旧 confirm-rerun 证据未被改**：20 文件 mtime 一致为 2026-09-24 10:47 本地（=02:47Z，与其 SELFTEST_SUMMARY `generated_at=2026-09-24T02:47:26Z` 吻合，早于修复时点 03:22）；其内嵌 `validator_sha256=fbf8097c…` 为修复前校验器（当前为 `08a37669…`）；日志内容为 15 变体时代（无 v10/v11/v12 日志文件、无 MAYBE_PASSED 字样、checks∈[12959,14771]），与 R1 验收时状态自洽；D-1 所指文件字节与 R1 记录的磁盘值相同。**结论：R1 修复前的确认重跑证据原样保留。**

验收结束时点二次重算：72 文件聚合仍为 `197371176…`、0 差异——本轮验收过程未触碰候选。

## 3. R00-A13 逐条判定（REQUIRED）

规格通过条件：「非零退出，精确指出 ID 和缺失字段」。

| # | 验证 | 方式 | 退出码 | 结果 |
|---|---|---|---|---|
| 1 | 现成套件独立复跑（canonical+v1–v9b+v10–v12+A14 四变体+正面对照） | `python3 -B docs/rust-tauri/R00/r00_t07_selftest.py --suite all --art-dir /tmp/t07_review_r2/art-full` | 0 | `SELFTEST PASS variants=18/18 (+positive-control ok) temp_removed=19/19`；A13 14/14、A14 4/4，逐变体子串全命中、缺失子串为空 |
| 2 | canonical 正例（场景手改 PASS + 删日志） | 上套件 a13-canonical | 1 | 同时报 `STATUS-PASS-WITHOUT-RESULT scenario=R00-T02-LA-000E6E1301C0 field 'result_ids'…` 与 `EVIDENCE-MISSING-FILE result=RES-R00-A01 …startup-probe.json does not exist`，精确到场景 ID/结果 ID/字段/证据路径 ✅（与候选在案 `a13-canonical.log`、`a13-negative-flip-status.log` 一致） |
| 3 | **R2 独立换 ID canonical**（全新探针 `/tmp/t07_review_r2/r2_probe.py`，不复用执行者代码）：flip 另一 supplemental 场景 `R00-T02-LA-FFF3E2044D0E` 为 PASS + 删除**另一个**证据 `T01/worktree-before.json`（RES-R00-A02） | 隔离副本单一篡改 | 1 | `STATUS-PASS-WITHOUT-RESULT scenario=R00-T02-LA-FFF3E2044D0E field 'result_ids' empty…` + `EVIDENCE-MISSING-FILE result=RES-R00-A02 evidence file '…worktree-before.json' does not exist`，`LEDGER_INVALID errors=2` ✅ |
| 4 | **R2 三个独立新反例**（R1 D-1/D-2 复验，见 §5） | 全新探针、换 ID/换非法值 | 各 1 | 全部被拒且错误行精确 ✅ |
| 5 | 正面对照（防「永远失败」） | 未篡改隔离副本 | 0 | `LEDGER_VALID checks=14829 scenarios=952 results=14 entries=832` ✅ |
| 6 | 真实仓库严格模式 | `python3 -B docs/rust-tauri/R00/r00_t07_validate_ledger.py` | 0 | 同上行输出，`spec_source=taskbook-strict` ✅ |

**判定：A13 PASS。**

## 4. R00-A14 逐条判定（REQUIRED）

规格通过条件：「缺少任务或场景会失败；补齐后通过」。

| # | 变体 | 退出码 | 关键输出 |
|---|---|---|---|
| 1 | 套件 v1 新增入口 `cli:lingxi-doctor`+功能、账本未映射 | 1 | `ENTRY-COVERAGE-BIDIR entry=cli:lingxi-doctor … not mapped in ledger` + `FEATURE-INVENTORY-BIDIR … F-D99-CLI-DOCTOR` + EVIDENCE-HASH-MISMATCH/STALE-SOURCE（只改盘点必被拒） |
| 2 | 套件 v2 功能入索引但无场景/任务 | 1 | `ORPHAN-REQUIREMENT feature=F-D99-CLI-DOCTOR … no supplemental scenario` |
| 3 | 套件 v3 补齐 feature→scenario→task 映射（含获准路径摘要重绑） | 0 | `LEDGER_VALID checks=14839 scenarios=953 entries=833` |
| 4 | 套件 v4 账本映射盘点不存在的入口 `cli:ghost` | 1 | `ENTRY-COVERAGE-BIDIR entrypoint_index … cli:ghost` |
| 5 | **R2 独立换命名空间探针** `http:GET/api/lingxi/r2-status`（新功能 `F-D98-R2-STATUS`，不同命名空间/形态，自写探针）未映射 | 1 | `ENTRY-COVERAGE-BIDIR entry=http:GET/api/lingxi/r2-status … not mapped in ledger` + `FEATURE-INVENTORY-BIDIR …F-D98-R2-STATUS`，`LEDGER_INVALID errors=10` |
| 6 | R2 探针：功能入索引但无场景 | 1 | `ORPHAN-REQUIREMENT feature=F-D98-R2-STATUS production feature has no supplemental scenario…`（另伴生 COUNTS/ENTRY/BIDIR/STALE 等真矛盾） |
| 7 | R2 探针：补齐映射+计数+获准路径摘要重绑 | 0 | `LEDGER_VALID checks=14839 scenarios=953 results=14 entries=833` ✅ |

「补齐后通过」语义：v3/R2-CE3 的重绑是获准路径模拟（等效重跑 T02 扫描+自测后重建）；R2 探针的 CE1/CE2 失败输出再次证明**只改映射不重绑摘要必被 EVIDENCE-HASH-MISMATCH/STALE-SOURCE 拒绝**，新增生产入口无法静默混入。

**判定：A14 PASS。**

## 5. R1 修复项复验（D-1 / D-2）

**D-1（必须，报告文档瑕疵）：已修。** 证据见 §2：行值、磁盘值、R1 要求值三方一致；清单文字与磁盘实算聚合重新收敛为同一值 `197371176…`。

**D-2（建议增强，已执行）：已修且无新回归。** 三个独立新反例（全新探针，篡改对象与非法值均区别于执行者自测 v10/v11/v12 所用的 RES-R00-A09/MAYBE_PASSED）：

| 反例 | 篡改（隔离副本） | 退出码 | 校验器关键输出（实测原文） |
|---|---|---|---|
| ① 场景 PASS 但结果 FAIL | `RES-R00-A01.status → FAIL`（场景 R00-A01 保持 PASS，exit_code 保持 0） | **1** | `STATUS-RESULT-CONFLICT scenario=R00-A01 result=RES-R00-A01 field 'status' is 'FAIL' but scenario ledger_status=PASS (…)` + `EXIT-CODE-CONFLICT result=RES-R00-A01 status=FAIL but raw exit_code=0 (…)` |
| ② 非法 result.status | `RES-R00-A05.status → 'PROBABLY_PASS'`（新非法值） | **1** | `RESULT-STATUS-ENUM result=RES-R00-A05 field 'status' value 'PROBABLY_PASS' not in allowed result status set ['BLOCKED','FAIL','NOT_APPLICABLE','NOT_RUN','PASS','STALE'] (…)`（伴生一条 STATUS-RESULT-CONFLICT，亦真） |
| ③ FAIL 且 exit_code 0（一致态） | 场景 `R00-A12` 与 `RES-R00-A12` 一致改 FAIL，exit_code 保持 0 | **1** | 仅 `EXIT-CODE-CONFLICT result=RES-R00-A12 status=FAIL but raw exit_code=0`，且**无** STATUS-RESULT-CONFLICT（禁串 absent，规则隔离成立） |

**合法 FAIL/BLOCKED/STALE 无误拒**（新状态规则不得误伤真实失败/阻塞/过期记录）：

| 对照 | 篡改（隔离副本） | 退出码 | 结果 |
|---|---|---|---|
| L-1 合法 FAIL | 场景 R00-A03+结果均 FAIL、exit_code=1 | **0** | `LEDGER_VALID` |
| L-2 合法 BLOCKED | 场景 R00-A04+结果均 BLOCKED、exit_code=2 | **0** | `LEDGER_VALID` |
| L-3 合法 STALE | 场景 R00-A06+结果均 STALE（exit 0） | **0** | `LEDGER_VALID` |

合法集依据复核：`ALLOWED_RESULT_STATUS`（validate_ledger.py:57-59）= 01 通用约束 §6 场景状态集中可作用于执行记录的值 + task-result 模板占位 `NOT_RUN`，排除纯规划态 NOT_STARTED/IN_PROGRESS——与修复报告 §3 的依据声明一致。R2 探针合计 11/11 全部符合预期（含正面对照）。

## 6. 映射完整性与计数独立重算（逐集比较，不信声明计数）

| 维度 | 声明 | R2 独立重算 | 一致 |
|---|---|---|---|
| 场景总数 | 952 = 200 基础 + 736 补充 + 16 t07_added | kind 逐个数恰为 200/736/16，总 952 | ✅ |
| 基础场景 | 200 全量导入 | acceptance-catalog 200 条 ID 集与账本 base 集相等（校验器严格模式逐字段比对 + 我的 v9 类套件复跑佐证） | ✅ |
| 补充叶子 | 736 | 与 FEATURE_STAGE_ACCEPTANCE supplemental ID 集**精确相等** | ✅ |
| t07_added | 16（A13×12+A14×4） | 16 个 `R00-T07-LA-*`（V1–V12 + A14 V1–V4），parent_acceptance/result 绑定逐项正确 | ✅ |
| 入口 | 832 | ENTRYPOINT_COVERAGE registrations 键集与 entrypoint_index **逐 entry 相等、逐 entry feature 集 0 差** | ✅ |
| 功能 | 743（736 生产） | 与 FEATURE_INVENTORY（736 生产+7 非生产）双向相等 | ✅ |
| 面/任务 | 37 / 100 | surfaces 与 ENTRYPOINTS.json 相等；tasks 100 | ✅ |
| 状态分布 | PASS 30 / NOT_STARTED 185 / SPECIFIED_NOT_EXECUTED 736 / NOT_RUN_UNAUTHORIZED 1 | 逐场景重数一致；PASS 30 = 14 基础（恰为 A01–A14）+ 16 t07_added | ✅ |

**未来未执行不冒充**：NOT_STARTED/SPECIFIED_NOT_EXECUTED/NOT_RUN_UNAUTHORIZED 场景挂结果的共 **0** 个（独立遍历）。**R11-A14 条件授权不冒充**：`ledger_status=NOT_RUN_UNAUTHORIZED`、`requirement=CONDITIONAL_AUTHORIZATION`、登记于 blockers.conditional_authorization（BLK-RELEASE-AUTH），校验器 BLOCKER-REGISTRY 规则强制之。**BLOCKERS 预登记**：BLK-CREDENTIALS 2 场景 + BLK-PLATFORM 7 场景的关键词命中集合从 acceptance-catalog 原文独立重算，与账本登记**完全相等**。

## 7. 历史 14 项结果绑定与修复过程一致性

- **SHA 链**：14 个结果 tested_sha（7d1a0c6bc/16aeb380d×2/60dbe0384×2/ffcb8583×2/6f58b9351×2/64c302ec4×2/4401afae×2）全部通过校验器 STALE-BRANCH 祖先校验（exit 0 即全过）；A13/A14 committed_in=null 且 tested_sha=basis.head，符合「当前任务结果允许待提交」规则。
- **环境/lockfile**：逐结果 platform/toolchain/dependency_lock_hashes 完整（校验器 RESULT-MISSING-FIELD/BAD-FIELD/LOCKFILE-MISMATCH 全过）；账本 basis.package_lock_sha256 与当前 package-lock.json 实算一致。
- **证据/源码**：14 结果共 55 个 evidence 文件、43 个监控源码路径，哈希逐文件由校验器验证通过；R2 抽查转录保真：RES-R00-A09 → `T05/REPLAY_SUMMARY.json`（三 run exitCode 全 0、`allRunsExitedZero`）、RES-R00-A11 → `T06/BASELINE_BENCHMARK.json` 存在且命令与账本记录一致。
- **修复过程账本重建一致性**：账本 basis.input_sha256 全部 9 项（含 SELFTEST_SUMMARY、build_map 自身）与当前磁盘实算一致；三代证据时序自洽——旧 confirm-rerun（02:47Z，旧校验器 fbf8097c，15/15，正面对照 checks=14761/scenarios=949）→ 终轮顶层（03:22Z，新校验器 08a37669，18/18，正面对照 checks=14661/scenarios=952/**results=12** 引导态）→ confirm-rerun-r1（03:23Z，18/18，正面对照 checks=14829/scenarios=952/results=14 终账本）。**R2 自行复跑的正面对照输出与 confirm-rerun-r1 完全相同**（`LEDGER_VALID checks=14829 scenarios=952 results=14 entries=832`）。三套日志互不引用（顶层/confirm-rerun-r1 文件中检索 confirm-rerun 路径 0 命中），无过期残留；顶层证据绑定的是终轮自测重写后的文件（校验器哈希验证通过）。R00-T07_RESULTS.json 中 A13/A14 observed 为占位、由生成器按 SELFTEST_SUMMARY 回填 14/14、4/4——与修复报告 §4 声明一致。
- **无新回归**：修复新增 2 规则族+1 扩展后，14 个真实结果（全 PASS/exit 0）正检通过；canonical/v1–v9b 原语义不变（R2 复跑 18/18 且逐日志子串核对）；typecheck/受影响测试见 §8。

## 8. 门禁复跑

| 命令 | 退出码 | 结果 |
|---|---|---|
| `python3 -B docs/rust-tauri/R00/r00_t07_validate_ledger.py` | 0 | `LEDGER_VALID checks=14829 scenarios=952 results=14 entries=832 spec_source=taskbook-strict` |
| `python3 -B docs/rust-tauri/R00/r00_t07_selftest.py --suite all --art-dir /tmp/t07_review_r2/art-full` | 0 | `SELFTEST PASS variants=18/18 (+positive-control ok) temp_removed=19/19` |
| `python3 -B /tmp/t07_review_r2/r2_probe.py`（R2 独立探针 11 例） | 0 | 11/11（正面对照+三新反例+三合法态对照+A13 换 ID+A14 换命名空间三态） |
| `python3 -m py_compile docs/rust-tauri/R00/r00_t07_{build_map,validate_ledger,selftest}.py` | 0 | 语法全过（脚本 SHA 复核与报告 §10 一致：08a37669…/2307fe30…/ff6f6cb5…） |
| `npm run typecheck` | 0 | 三配置全过 |
| `npx vitest run tests/migration/r00-t05-replay.test.ts tests/migration/r00-a10-old-defect.test.ts` | 0 | 2 文件 14/14 |

## 9. 其他发现（均不阻塞，如实记录）

1. **报告 §12 第 7 点遗留旧口径**：仍写「46 行拼接 SHA-256 应为 `a33fc436…`」——这是 R1 修复前的历史建议文字；§13 已显式声明 §1–§12 保留原始交付时点表述、当前状态以 §10/§13 增量为准，且 §10 现值正确。属已披露的历史文本，无误导性，不要求修改（如总控愿意可在下次获准更新时顺手加一句指向 §10）。
2. **顶层正面对照日志反映引导态账本**（results=12, checks=14661）：终账本的确认证据在 `confirm-rerun-r1/`（results=14, checks=14829），两者时序与修复报告 §4 的获准更新路径一致，链路自洽；后续读者需按 §13 理解两层证据的关系（已披露）。
3. **校验器重复 result_id 错误文案**含自指短语 `(also at <同一rid>)`——检测行为正确（R1 已核），文案冗余；修复报告 §10 已如实登记并按最小改动原则不修，无影响。
4. **反例①的双报设计**（同一篡改同时触发 STATUS-RESULT-CONFLICT 与 EXIT-CODE-CONFLICT）经 R2 用反例③隔离态验证为两条独立真实矛盾而非误报；伴生报错（v11 的枚举+冲突双报）同理。
5. `docs/rust-tauri/R00/__pycache__/` 为 gitignore 项（py_compile 副作用），不在候选 72 文件集内，无影响。

## 10. 未执行范围（及理由）

- **npm/CI 门禁接入**：任务书 T07 未要求（执行报告 §11 声明；R02 xtask `verify-stage` 消费账本时接入）。
- **全量 `npm test`**：候选零 TS/生产代码改动，按 AGENTS.md「验证覆盖实际改动与受影响行为」只跑 §8 两组受影响验证。
- **生成器真实仓库重跑（`r00_t07_build_map.py`）**：会在真实仓库覆写 ACCEPTANCE_MAP.json/BLOCKERS.md，违反验收方「不改候选」约束；以 basis.input_sha256 全项实算一致 + 校验器全量哈希验证 + 隔离副本正面对照等效覆盖（R1 同此口径）。
- **其他平台/真实供应商/LIVE/发布**：非 R00-T07 范畴；R11-A14 维持 NOT_RUN_UNAUTHORIZED。
- **T01–T06 原始测量重跑**：非本轮范围；T07 对其结果的转录保真经哈希/祖先/退出码/证据对回抽查（§7）与校验器全量验证成立。
- **网络操作**：无（本机代理不可达不影响任何本地验证）。

## 11. 验收声明

本报告为独立验收记录，判定 R00-A13/R00-A14 与 R00-T07 候选（含 R1 修复）可否放行；未修改候选与任何仓库文件（唯一新增本报告）；无未披露的环境限制。验收过程产物在 `/tmp/t07_review_r2/`（探针脚本、复跑自测输出、probe_results.json，临时可清理）。**结论：R00-A13 PASS、R00-A14 PASS、R00-T07 验收 PASS，无遗留必须修复项。**
