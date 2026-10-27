# R00-T07 独立验收 R3（ZCode）

验收代理：REVIEWER-R00-T07-R3（一次性全新独立验收代理，ZCode，未参与本候选执行、R1/R2 验收或 R1/R2 修复）
任务标题：R00-T07 独立验收 R3（ZCode）
候选：R00-T07「建立可执行验收账本」**R2 修复后的当前候选**（执行者报告 `R00-T07_REPORT.md` §1–§14；修复报告 `R00-T07_REPAIR_R1.md` / `R00-T07_REPAIR_R2.md`，R2 修复自评 READY_FOR_R3_REVIEW）
Task base/HEAD：`4401afae2ee48df1512b8a8c9a1262be0efbe64f`（分支 `codex/rust-tauri-migration`，实测一致）
验收时点：2026-09-24（本机 macOS 27.0 darwin arm64，Python 3.14.3，Node v24.16.0）

## 结论

**R00-A13：PASS。R00-A14：PASS。任务 R00-T07 验收结论：PASS。D-3（BLOCKERS.md 末尾空白行）已彻底修复，无遗留必须修复项。**

R2 的 PASS 绑定修复前候选，本轮对变化后的当前候选全部重新独立判定。核心依据：三个绑定 SHA 全部核对一致；95/95 候选文件逐行 SHA 与聚合独立实算零差异；生成器在两个独立隔离根重跑，BLOCKERS.md 与候选字节一致且结尾恰一个换行，ACCEPTANCE_MAP.json 差异仅 `generated_at`；回归探针按修复报告所述根因复原旧生成器，逐字节复现修复前缺陷产物；`git diff --check` 全候选零 whitespace 错误；R3 全新探针（篡改对象与非法值均区别于执行者自测与 R1/R2 探针）A13 四例、A14 三例 + 正面对照全部符合规格；真实仓库严格模式校验、自测套件、py_compile、typecheck、受影响 vitest 全部复跑通过；执行者未自评任务 PASS。

## 1. 验收范围与执行方式

- 只审查候选，未修改任何候选文件、总控账本或其他仓库文件；未 commit/push/PR/tag/release/建分支/worktree/派生子代理；本报告是唯一新增文件。验收前后两次实算 95 文件聚合 SHA 均为 `2a26cb1f…`（零触碰证明）。
- 全部试验在 `/tmp/r3_review_r3/`（生成器隔离重跑 isoA/isoB/回归探针 isoC、自测复跑、探针脚本）与 `tempfile.mkdtemp` 隔离副本执行。无网络、无真实供应商、无付费 API、无外发、无真实用户数据。
- 隔离根的 git 身份：生成器内部执行只读 `git rev-parse`，隔离根以 `.git` 符号链接指向真实仓库满足该只读查询，未对真实仓库做任何写操作。
- 完整阅读：AGENTS.md、任务书 00/01–06、R00 阶段书（T07/A13/A14 全文）、acceptance-catalog（200 场景、199 REQUIRED + 1 条件授权、R00-A13/A14 条目原文）、执行报告全文（含 §13/§14 修复增补）、R1/R2 验收报告、R1/R2 修复报告、三工具脚本全文（build_map 556 行 / validate_ledger 497 行 / selftest 615 行）、R00-T07_RESULTS.json、BLOCKERS.md、ACCEPTANCE_MAP.json（结构化独立查询）、总控账本 R00-T07 条目（只读）、四套自测证据（顶层 / confirm-rerun / confirm-rerun-r1 / confirm-rerun-r2）。不采信报告结论，全部独立重算/复跑。
- 环境约束亲身复核：本机 Mimosa 写入钩子确实拦截含动态 argv subprocess 形态的脚本写入（本轮撰写探针时亲身复现），执行者/R1/R2 声明的「进程内调用校验器」环境约束属实；校验器与生成器的 CLI 形态均经本轮真实进程实测（exit 0）。

## 2. 候选完整性与三个绑定 SHA 独立重算

| 项 | 期望（总控指令） | R3 独立实算 | 一致 |
|---|---|---|---|
| 执行报告 SHA-256 | `a67d51e71faeaa4fca59f9f83373e88cf2efd144df970b75e487a5d424498d71` | 相同 | ✅ |
| R2 修复报告 SHA-256（表述更正后） | `c0b2e9adeac101390bd0e89bc11cd507c56846e52b679d02a829ae13f7fcff62` | 相同 | ✅ |
| 95 文件聚合 SHA（SHA256+双空格+路径+换行拼接） | `2a26cb1f548f4390141ea8ed9c9ce68d4e87134a2f9a9b0e25114de1a7134a6c` | 磁盘实算与报告 §10 清单文字重算**均为该值** | ✅ |
| 逐文件比对 | 报告 §10 清单 95 行 | **95/95 逐行一致，0 差异** | ✅ |
| 磁盘候选集合 | 95 文件（docs 6 + T07 artifacts 89 = 顶层 23 + confirm-rerun 20 + confirm-rerun-r1 23 + confirm-rerun-r2 23） | `rglob` 实际枚举恰 95 文件，与清单集合**精确相等**（无漏列/多列） | ✅ |

验收结束时点二次重算：95 文件聚合仍为 `2a26cb1f…`——本轮验收过程未触碰候选。

## 3. D-3 独立验证：生成器尾部空白行缺陷已彻底修复

**结论：D-3 已彻底修复（修生成器而非手改产物；重建后缺陷不再复现；差异面与修复报告声明逐字节一致）。**

| # | 验证 | 独立命令/方式 | 退出码 | 结果 |
|---|---|---|---|---|
| 1 | 当前 BLOCKERS.md EOF | 字节级检查 | — | 2111 字节、36 行，结尾 `…SPECIFIED_NOT_EXECUTED \| 736 \|\n` 恰一个换行，无 EOF 空白行；状态表（base 200/supplemental 736/t07_added 16/PASS 30/NOT_STARTED 185/SPECIFIED_NOT_EXECUTED 736/NOT_RUN_UNAUTHORIZED 1）与四节登记（ACTIVE 空、BLK-RELEASE-AUTH、BLK-LONGRUN-G1、BLK-CREDENTIALS 2 + BLK-PLATFORM 7）完整未丢 |
| 2 | 隔离重跑生成器（A 根） | `cd /tmp/r3_review_r3/isoA && python3 -B docs/rust-tauri/R00/r00_t07_build_map.py` | 0 | `MAP-BUILT scenarios=952 tasks=100 features=743 entries=832 tests=28 results=14 selftest=attached`；产出 BLOCKERS.md 与真实候选**字节一致**（sha `b27c69fb…`），EOF 恰一个换行 |
| 3 | 隔离重跑生成器（B 根，确定性复核） | `cd /tmp/r3_review_r3/isoB && python3 -B …` | 0 | 输出与 A 根完全一致（BLOCKERS.md 字节一致）；ACCEPTANCE_MAP.json 与 A 根规范化逐字段 diff（剔除 `generated_at`）= **0 差异**——生成器确定性成立 |
| 4 | 重建差异面 | 规范化 deep diff（真实候选 vs isoA，剔除 `generated_at`） | — | **0 差异**。即当前生成器重建后，账本除生成时间外与候选完全一致（当前 map 内嵌的生成器自哈希 `60bb4655…` 与磁盘生成器实算一致，故本轮重建连自哈希也不变；「重建只变化 generated_at 与生成器自哈希」的声明在生成器变更场景下由 #5 反向证明） |
| 5 | 回归探针（复原修复前生成器） | isoC：在生成器副本中把修复处的两行注释换回 `lines.append("")` 后重建 | 0 | 复原生成器 SHA = `ff6f6cb5d7495b03…`（**恰为修复报告 §3 记录的修复前生成器哈希**）；其 BLOCKERS.md 产出 37 行、末尾空白行复现，SHA = `6f6945a86ddb0c22…`（**恰为修复报告 §5 记录的修复前 BLOCKERS.md 哈希**，与当前候选的差异恰为第 37 行空白行）；其 ACCEPTANCE_MAP 与当前候选 deep diff 恰 2 处：`basis.input_sha256["r00_t07_build_map.py"]`（`60bb4655…`→`ff6f6cb5…`）与 `generated_at`——修复报告「差异仅两处」的声明被反向逐字节证实 |
| 6 | 全候选差异检查 | `git add -N`（候选 95 路径 + 五份报告）→ `git diff --check` → `git reset` 还原 | 0 | **无任何 whitespace/EOF 错误输出**；检查毕 staged 内容清零（`git diff --cached --stat` 为空），git status 恢复验收前状态 |
| 7 | BLOCKERS.md 直查 | `git diff --no-index --check /dev/null docs/rust-tauri/R00/BLOCKERS.md` | 1（有差异属预期） | **无 whitespace 错误输出**（修复前同命令报 `BLOCKERS.md:37: new blank line at EOF.`） |

根因与修法复核：`r00_t07_build_map.py:549-551`——`write_blockers_md` 末尾不再 `lines.append("")`，`"\n".join(lines) + "\n"` 给出恰好一个结尾换行，并留注释说明。为最小修复；#5 证明旧缺陷确由该行引起。

## 4. 账本结构与计数独立重算（不信声明，逐集查询/重算）

| 维度 | 声明 | R3 独立重算 | 一致 |
|---|---|---|---|
| 场景总数 | 952 = 200 基础 + 736 补充 + 16 t07_added | kind 逐个数恰为 200/736/16，总 952 | ✅ |
| 状态分布 | PASS 30 / NOT_STARTED 185 / SPECIFIED_NOT_EXECUTED 736 / NOT_RUN_UNAUTHORIZED 1 | 逐场景重数一致；PASS 30 = 14 基础（**恰为 R00-A01..A14**）+ 16 t07_added（12×A13 + 4×A14，parent_acceptance/result 绑定逐项正确） | ✅ |
| 未执行不冒充 | — | NOT_STARTED/SPECIFIED_NOT_EXECUTED/NOT_RUN_UNAUTHORIZED 场景挂结果的共 **0** 个 | ✅ |
| R11-A14 条件授权 | NOT_RUN_UNAUTHORIZED | `requirement=CONDITIONAL_AUTHORIZATION`、`ledger_status=NOT_RUN_UNAUTHORIZED`、result_ids=[]、登记于 blockers.conditional_authorization | ✅ |
| 入口 | 832 | ENTRYPOINT_COVERAGE registrations 键集与 entrypoint_index 逐 entry 相等、逐 entry feature 集 0 差 | ✅ |
| 功能/任务/面 | 743（736 生产）/ 100 / 37 | counts 块与逐项实算一致（生成器重建 #2/#3 亦从源头复算同值） | ✅ |
| 结果 | 14（RES-R00-A01..A14） | 14 条，21 项必填字段（RESULT_REQUIRED_FIELDS）逐条无缺失 | ✅ |
| 阻塞预登记 | BLK-CREDENTIALS 2 + BLK-PLATFORM 7 | 从 acceptance-catalog 原文独立重算关键词命中：`{R10-A09,R10-A10}` 与 `{R04-A09,R04-A10,R04-A12,R09-A05,R09-A07,R09-A13,R10-A13}`，与账本登记**完全相等**；active 为空 | ✅ |
| 执行者自评边界 | 不自评 PASS | `ledger_self_status="READY_FOR_REVIEW"`；报告结论 READY_FOR_REVIEW；R2 修复报告 READY_FOR_R3_REVIEW；总控账本 `execution_result="READY_FOR_REVIEW"`、`review_verdict=null` | ✅ |

结果转录抽查：RESULTS.json 中 A13/A14 为占位（`tested_sha=null`、observed「待自测后由生成器填入」），账本内由生成器按 SELFTEST_SUMMARY 回填 `tested_sha=4401afae…`（HEAD，committed_in=null 合规）、`observed="selftest suite a13: 14/14 …"`——与声明机制一致；RES-R00-A11 的 command/platform/toolchain/observed 对回 T06 基准与报告口径自洽。

## 5. confirm-rerun-r2/ 真实重跑与历史证据未改

- **23 文件、独立目录**：mtime 2026-09-24 12:02 本地；SELFTEST_SUMMARY `generated_at=2026-09-24T04:02:30Z`（晚于账本重建 `generated_at=04:02:05Z` 27 秒——先重建、后确认重跑，与修复报告 §3 顺序一致）、`validated_repo_head=4401afae`、`validator_sha256=08a37669…`（当前校验器）。
- **18/18 + 正面对照**：a13 14/14、a14 4/4、positive-control ok、temp_removed=19/19；正面对照输出 `LEDGER_VALID checks=14829 scenarios=952 results=14 entries=832 spec_source=taskbook-strict`，与真实仓库 CLI 校验（本轮亲测）**逐字一致**。
- **真实重跑（非拷贝）的直接证据**：confirm-rerun-r1 与 r2 的 `a13-v7-duplicate-id.log` 内嵌**不同**的 mkdtemp 临时根路径（`…84xaj_8i…` vs `…6nohm_p8…`，该日志因错误行含账本绝对路径而必然带随机临时路径），两文件 SHA 不同（`75ea4731…` vs `cf773076…`）；其余日志为确定性内容，哈希跨轮一致属预期。
- **历史证据未被改、无互相引用、无过期**：三代证据时序自洽——confirm-rerun/（20 文件，02:47Z，旧校验器 `fbf8097c…`，15 变体时代，checks=14761/scenarios=949）→ 顶层次终轮（03:22Z，新校验器，18/18，正面对照 checks=14661/scenarios=952/results=12 引导态）→ confirm-rerun-r1/（03:23Z，18/18，checks=14829/results=14 终账本）→ confirm-rerun-r2/（04:02Z，D-3 修复后账本，checks=14829/results=14）。各目录日志中检索其他 confirm-rerun 目录路径 **0 命中**；D-1 所指 `confirm-rerun/a13-v3-evidence-tampered.log` 字节仍为 `aa3ec8e3…af2ba…` 值（§2 的 95/95 比对已含）。
- 附注（不影响判定）：`a14-entry-add-remove.log` 在顶层/r1/r2 三处 SHA 相同——自测脚本的别名日志按设计从顶层 ART 目录拼装（r00_t07_selftest.py:590-598），非当前 `--art-dir` 运行产物；其内容（a14-v1 拒绝 + a14-v3 通过）对最终账本仍然准确（本轮独立复跑 a14-v3 输出与之逐字一致），且相同哈希在报告 §10 清单一目了然，属自披露行为。

## 6. R00-A13 逐条判定（REQUIRED）

规格通过条件：「非零退出，精确指出 ID 和缺失字段」。R3 探针全部使用**执行者自测与 R1/R2 探针均未用过的篡改对象/非法值**（全新探针 `/tmp/r3_probes.py`，进程内调用校验器，等效 argv `--root <隔离副本> --git-repo <真实仓库>`）：

| # | 验证 | 篡改（隔离副本） | 退出码 | 关键输出（实测原文摘录） |
|---|---|---|---|---|
| 1 | 现成套件独立复跑 | —（`--art-dir /tmp/r3_review_r3/art-full`） | 0 | `SELFTEST PASS variants=18/18 (+positive-control ok) temp_removed=19/19` |
| 2 | **R3-P1 无证据 PASS（新对象）** | base 场景 `R06-A11`（NOT_STARTED）手改 PASS、无任何结果 | **1** | `STATUS-PASS-WITHOUT-RESULT scenario=R06-A11 field 'result_ids' empty: status=PASS requires >=1 result record` ✅ 精确到场景 ID+字段 |
| 3 | **R3-P2 删日志（新对象）** | 删除 `T05/normalized-diff-run1-run2.txt`（RES-R00-A09 证据） | **1** | `EVIDENCE-MISSING-FILE result=RES-R00-A09 evidence file 'artifacts/rust-tauri/R00/T05/normalized-diff-run1-run2.txt' does not exist (deleted log)` ✅ 精确到结果 ID+证据路径 |
| 4 | **R3-P3 状态冲突（新形态）** | `RES-R00-A07.status→BLOCKED`（场景 R00-A07 保持 PASS，exit_code 0） | **1** | 仅 `STATUS-RESULT-CONFLICT scenario=R00-A07 result=RES-R00-A07 field 'status' is 'BLOCKED' but scenario ledger_status=PASS`，且**无** EXIT-CODE-CONFLICT（BLOCKED+exit0 不属退出码矛盾，规则隔离正确；R1/R2 未测过 BLOCKED 形态） |
| 5 | **R3-P4 非法枚举（新值）** | `RES-R00-A03.status→'PARTIAL'` | **1** | `RESULT-STATUS-ENUM result=RES-R00-A03 field 'status' value 'PARTIAL' not in allowed result status set […]`（+伴生 STATUS-RESULT-CONFLICT，亦真） |
| 6 | 语义边界如实记录 | 场景与结果**一致**改 BLOCKED 且 exit_code=0 | 0 | `LEDGER_VALID`（退出码规则只约束非零退出；一致 BLOCKED 不属任务书列举的六类必拒项，与 R2 对照 L-2（exit 2）互补印证边界） |
| 7 | 正面对照（防「永远失败」） | 未篡改副本 | 0 | `LEDGER_VALID checks=14829 scenarios=952 results=14 entries=832` |
| 8 | 真实仓库严格模式 | `python3 -B docs/rust-tauri/R00/r00_t07_validate_ledger.py` | 0 | 同上行输出，`spec_source=taskbook-strict` |

**判定：A13 PASS。** 非零退出与「精确指出 ID 和缺失字段」均由 R3 全新对象独立复现；REQUIRED 激活条件（本阶段范围内必须验证）已在本轮真实执行。

## 7. R00-A14 逐条判定（REQUIRED）

规格通过条件：「缺少任务或场景会失败；补齐后通过」。R3 探针使用全新命名空间 `cron:`（执行者用 `cli:`、R1 用 `http:POST`、R2 用 `http:GET`）：

| # | 变体 | 退出码 | 关键输出 |
|---|---|---|---|
| 1 | **R3-P1 新生产入口漏映射**：coverage+inventory 新增 `cron:r3-midnight-audit`/`F-D97-R3-AUDIT`，账本不动 | **1** | `ENTRY-COVERAGE-BIDIR entry=cron:r3-midnight-audit registration features ['F-D97-R3-AUDIT'] not mapped in ledger` + `FEATURE-INVENTORY-BIDIR features_index … inventory-only features: ['F-D97-R3-AUDIT']` + EVIDENCE-HASH-MISMATCH/STALE-SOURCE 链（`LEDGER_INVALID errors=10`）——新命名空间同样拦截，非脚本专属路径 |
| 2 | **R3-P2 补齐后通过**：feature→场景 `R00-T07-LA-A14SIM-R3AUDIT`→任务 `R07-T09`、entrypoint_index、计数、并按获准路径重绑受影响结果摘要 | **0** | `LEDGER_VALID checks=14839 scenarios=953 results=14 entries=833 spec_source=taskbook-strict` ✅ 与套件 v3 / R2-CE3 输出完全一致 |
| 3 | **R3-P3 静默混入被拒**：同 #2 补齐映射但**不重绑**受影响结果摘要 | **1** | `EVIDENCE-HASH-MISMATCH result=RES-R00-A03/A04 …` + `STALE-SOURCE result=RES-R00-A13/A14 …`（`errors=7`）——新增生产入口无法绕过受影响检查重跑 |
| 4 | 套件 v1–v4（cli:lingxi-doctor 全套） | 1/1/0/1 | §6 #1 复跑 18/18 内含，子串逐项命中 |

「补齐后通过」语义维持 R1/R2 认定：补齐须含获准路径摘要重绑（等效重跑 T02 扫描+自测后重建），只改映射不重绑必被拒（#3 独立复证）。

**判定：A14 PASS。**

## 8. 门禁复跑汇总（全部本轮亲自执行）

| 命令 | 退出码 | 结果 |
|---|---|---|
| `python3 -B docs/rust-tauri/R00/r00_t07_validate_ledger.py`（真实仓库，严格模式，真进程） | 0 | `LEDGER_VALID checks=14829 scenarios=952 results=14 entries=832 spec_source=taskbook-strict` |
| `python3 -B docs/rust-tauri/R00/r00_t07_selftest.py --suite all --art-dir /tmp/r3_review_r3/art-full` | 0 | `SELFTEST PASS variants=18/18 (+positive-control ok) temp_removed=19/19` |
| `python3 -B /tmp/r3_probes.py`（R3 独立探针 8 例） | 0 | 8/8（A13×4 + A14×3 + 正面对照）；另单跑语义边界 1 例如实记录（§6 #6） |
| 隔离重跑生成器 ×2 + 回归探针 ×1 | 0×3 | §3 #2/#3/#5 |
| `python3 -m py_compile docs/rust-tauri/R00/r00_t07_{build_map,validate_ledger,selftest}.py` | 0 | 语法全过 |
| `npm run typecheck` | 0 | 三配置全过（本轮零 TS 改动确认） |
| `npx vitest run tests/migration/r00-t05-replay.test.ts tests/migration/r00-a10-old-defect.test.ts` | 0 | 2 文件 14/14 |
| `git diff --check`（intent-to-add 后还原） | 0 | 无 whitespace 错误；staged 清零 |

## 9. 其他发现（均不阻塞，如实记录）

1. `a14-entry-add-remove.log` 别名日志由顶层 ART 拼装（§5 附注）：内容对最终账本准确且哈希自披露，建议后续如再改自测脚本可顺手让别名日志从 `--art-dir` 自身拼装，避免读者困惑。非缺陷。
2. 执行报告 §12 第 7 点仍保留 R1 时代「46 行 … `a33fc436…`」历史口径（§13/§14 已声明 §1–§13 保留原始表述、以 §10 现值为准；§10 现值正确）。R2 已记录过，仍存在，无误导性。
3. 一致 BLOCKED + exit_code=0 可通过（§6 #6）：语义上可辩护（阻塞未必要以非零退出呈现），且挂在 PASS 场景下仍被 STATUS-RESULT-CONFLICT 拒绝（§6 #4）；不属任务书六类必拒项，无需本轮处理。
4. 生成器在无 git 身份的裸目录不可运行（内部 `git rev-parse`）：验收/复现需提供 git 上下文（本轮用只读 `.git` 符号链接）。这是运行前提而非缺陷。

## 10. 未执行范围（及理由）

- **生成器在真实仓库重跑**：会覆写候选文件，违反验收方「不改候选」约束；以双隔离根重跑 + 回归探针 + 重建差异 0 等效或更强覆盖。
- **npm/CI 门禁接入**：任务书 T07 未要求（执行报告 §11 声明；R02 xtask `verify-stage` 消费账本时接入）。
- **全量 `npm test`**：候选零 TS/生产代码改动，按 AGENTS.md「验证覆盖实际改动与受影响行为」只跑 §8 受影响组（typecheck + 2 个迁移测试文件）。
- **其他平台/真实供应商/LIVE/发布**：非 R00-T07 范畴；R11-A14 维持 NOT_RUN_UNAUTHORIZED。
- **T01–T06 原始测量重跑**：非本轮范围；T07 对其转录保真经账本哈希/祖先/字段校验（validator exit 0 即全过）+ §4 抽查成立。
- **网络操作**：无（本机代理不可达不影响任何本地验证）。

## 11. 验收声明

本报告为独立验收记录，判定 R00-A13/R00-A14 与 R00-T07 当前候选（R2 修复后、聚合 `2a26cb1f…`）可否放行；未修改候选与任何仓库文件（唯一新增本报告）；总控账本 `ORCHESTRATOR_PROGRESS.json` 零触碰（其未提交修改仍为总控自身 41+/16- 状态）；无未披露的环境限制。验收过程产物在 `/tmp/r3_review_r3/`（隔离根、探针脚本、自测复跑输出、probe_results.json，临时可清理）。本报告自身 SHA 不自嵌（嵌入即改变自身）；以 `shasum -a 256 docs/rust-tauri/R00/R00-T07_REVIEW_R3.md` 现算核对。

**最终结论：R00-A13 PASS、R00-A14 PASS、R00-T07 验收 PASS（当前候选完整通过）；D-3 已彻底修复；无遗留必须修复项。**
