# R00-T08 独立验收报告 R2（REVIEWER-R00-T08-R2，ZCode）

验收对象：R00-T08「封存并交接基线」**R1 修复后**的当前未提交候选（含 A15/A16 与第二轮账本重绑产物）。
验收者：独立 ZCode 代理 REVIEWER-R00-T08-R2，与 T08 执行者、R1 验收者（REVIEWER-R00-T08-R1）、R1 修复者（REPAIR-R00-T08-R1）均隔离；只审查、不实现、不修复、不派生代理、不建分支/worktree、不 commit/push/PR/tag/release；全部篡改探针与复跑输出置于 /tmp 隔离根（`/tmp/r2-review/`）。本报告是本次验收唯一新增仓库文件。
验收基点：分支 `codex/rust-tauri-migration`，HEAD = `e0b7be6108c4d5bc873061dee279b7163ca78a41`（与派发基点一致；验收期间未变，无新建分支/worktree，分支仅 `codex/rust-tauri-migration` 与 `main`）。
时间：2026-09-24（本机 CST）。

## 0. 结论（先读）

| 项 | 结论 |
|---|---|
| **R00-A15 预存失败可重现** | **PASS**（复现校验器独立复跑 exit 0；未修绿/未删除/未改白名单复证） |
| **R00-A16 基线审阅可独立复查** | **PASS**（抽查器绑定新 HANDOFF 复跑 exit 0 + 我方另选三个**全新**样本全过） |
| **R00-T08 封存并交接基线** | **PASS（R1 两项 MUST_FIX F1/F2 均已彻底解决，O2 口径已写全，无新增 MUST_FIX）——当前候选可以提交** |

**当前候选可提交。** 无必须修复项。非阻塞观察 5 项见 §8（均不阻塞提交：1 项属总控账本自身演进、1 项为报告一处 markdown 挤行、1 项为中间态 SHA 固有不可复验、2 项为 R1 已登记遗留）。

## 1. 输入与独立性声明

已完整读取：AGENTS.md；任务书 00/01/05/90/91 与 R00 阶段书（T08/A15/A16 规格及 §6 阶段放行条件）；R00-T08_REVIEW_R1.md、R00-T08_REPAIR_R1.md、R00_REPORT.md、R00_HANDOFF.json、R00_EVIDENCE_SUMMARY.md、T08/CANDIDATE_MANIFEST.txt；账本 ACCEPTANCE_MAP.json/BLOCKERS.md/R00-T07_RESULTS.json 与 build_map/validate_ledger/selftest/spotcheck/a15_verify_repro 源码；实际工作区 diff 与关键 T08 日志。未采信 R1 历史 PASS、修复报告自述或执行者任何 PASS 宣称——以下全部结论基于我方独立重算/复跑/源码核对。未改任何候选文件、R1 历史报告、修复报告与 ORCHESTRATOR_PROGRESS.json。

## 2. 派发坐标独立重算（全部一致）

| 文件 | 派发 SHA-256 | 我方 `shasum -a 256` | 一致 |
|---|---|---|---|
| R00-T08_REVIEW_R1.md | `8a832b5d…a5e66e2` | 同 | ✓（R1 历史报告未被修复者触碰） |
| R00-T08_REPAIR_R1.md | `c6a1a57d…bf2f5d79` | 同 | ✓ |
| R00_REPORT.md | `724d7188…a1b76cc` | 同 | ✓ |
| R00_HANDOFF.json | `68877ff6…96c62a5` | 同 | ✓（与账本 A16 绑定值、清单行、报告 §10 四方一致；冻结态成立） |
| R00_EVIDENCE_SUMMARY.md | `977cb757…cd0665e` | 同 | ✓ |
| T08/CANDIDATE_MANIFEST.txt | `d83ae896…a448645` | 同 | ✓（173 行；其自 SHA 即聚合值，与报告 §10、摘要 §1 一致） |
| ACCEPTANCE_MAP.json | （修复报告声称 `5eeac37c…`） | 同 | ✓ |
| BLOCKERS.md / R00-T07_RESULTS.json / r00_t07_build_map.py | `06dc5963…` / `4d50dd10…` / `5b8b94b5…` | 同 | ✓（与修复报告 §1「未动」声明一致） |

## 3. R1 修复核验：F1 / F2 / O2 全部彻底解决

### 3.1 F1（候选计数）——已解决

- **权威口径实测**：清单 173 行 = 7 个已跟踪修改（ACCEPTANCE_MAP、BLOCKERS、R00-T07_RESULTS、r00_t07_build_map、eslint.config.js、FIXTURE_MANIFEST、network-guard）+ 166 新文件（T08 目录 162 + docs 3 + 测试 1）；与报告 §2「173 文件」、§4「物理 163 = 162 + 清单自身」、§10「173 = 7+166」、§13「删除 169 个新文件 = 清单内 166 + 清单外 3」全链自洽（173=7+166、163=162+1、169=166+3），且全部与 git/磁盘实测相符（§4）。
- **陈旧引用清零**：`140 文件 / 133 个新文件 / 141 个新文件 / 138 个新文件 / 134 个原始 / 135 文件 / 145 文件 / 7dc6e4a0 / e0bd3cfa / 14e399ef / bde9eea9 / 916b8a30 / cad8b708` 十三个模式在 REPORT/SUMMARY/HANDOFF/ACCEPTANCE_MAP 中**全部 0 命中**（旧值仅存于历史文件：R1 报告、修复报告演进表、a16-spotcheck-r1.log、总控账本——均为对当时事实的合法记录）。
- R1 判定的缺陷机理（按旧文「133」执行回退将残留 8 文件）已消除：§13 现行文按 169 执行后无残留（166+3=169 与 173−7 闭合）。

### 3.2 F2（lint warning 口径）——已解决（三证一致）

| 来源 | 数字 |
|---|---|
| lint-r2.log 原文 | `✖ 10887 problems (0 errors, 10887 warnings)`；`0 errors and 33 warnings potentially fixable with the --fix option.` |
| 我方独立复跑（`npm run lint`，2026-09-24） | exit 0，**0 errors / 10887 warnings / 33 fixable**（输出与 lint-r2.log 逐字一致） |
| REPORT §5/§6 与 HANDOFF verification_battery.expected | 「0 错误 / 10887 处预存 warning 原样保留，其中 33 处可 --fix 自动修复」 |

我方复跑日志 SHA `25b0f2b22117abe330e742004e9f7bfd74c079a6e19d06ba60118ff271f9c094` 恰与 R1 验收者同类复跑（R1 §7 `/tmp/r1-review-lint.log`）**字节相同**——lint 输出确定性，执行者/R1/R2 三轮一致。R1 判定的「低估两个数量级」失实陈述已不存在。

### 3.3 O2（npm test 分母口径）——已写全，无新误导

npm-test-r1.log 原文：`Test Files 3 failed | 1463 passed | 3 skipped (1469)`；`Tests 4 failed | 14895 passed | 15 skipped (14914)`。REPORT §5 与 HANDOFF expected 现按全口径写「文件 1463 通过/3 失败/3 跳过（共 1469）；用例 14895 通过/4 失败/15 跳过（全口径共 14914）」，与日志逐字一致；摘要 §4「其余 1463 文件/14895 用例通过」为通过数口径、准确。O1/O3 按 R1「无需动作」未动（O3 现状见 §8）。

## 4. 候选清单与文件集合闭合（程序化全量比对）

- **清单 173 行逐条对盘**：`SHA␣␣路径` 格式 0 违例、路径全唯一（0 重复）、全排序、结尾恰一个换行；**173/173 条 SHA-256 与磁盘字节一致，0 不匹配**；聚合 SHA（清单自 SHA）= `d83ae896…`。
- **git 可见集闭合**：`git status --short -uall` 共 86 路径 = 清单内可见 80（7 修改 + 73 未跟踪）+ 声明排除 6（ORCHESTRATOR_PROGRESS.json、R00_REPORT.md、R00_EVIDENCE_SUMMARY.md、CANDIDATE_MANIFEST.txt、R00-T08_REVIEW_R1.md、R00-T08_REPAIR_R1.md）——**零遗漏、零多余**；修改集 8 = 清单内 7 + 总控账本。
- **忽略集闭合**：T08 目录被 .gitignore 忽略的 93 项**全部在清单内**（含修复轮新增 28 个证据：5 日志 + ledger-selftest-r3/ 23 文件，其 SHA 与修复报告 §7.2 逐一相符，见 §6.3）。
- **物理对账**：T08 目录物理 163 文件 = 清单内 162 + 清单自身；135（R1 时点）+28=163、145+28=173 两条演进线均闭合。
- **无循环摘要**：HANDOFF artifact_hashes 18 项不含 REPORT/SUMMARY/MANIFEST/账本三件套（按设计由报告 §10 记录终值）；账本 16 条结果无一监控 REPORT/SUMMARY/MANIFEST；清单含 HANDOFF 但不含 REPORT/SUMMARY/自身。改报告/摘要不再触发账本 STALE（防环成立）。
- **HANDOFF 全部固定哈希复算**：artifact_hashes 18/18 对盘一致；protocol_version（contract-versions.json）、spot_checks 两夹具（multi-turn-basic d0c599b4…/62764808…）、a16-test-rerun.log（de6fcf3a…）、package-lock（e54a16fe…）全部复算通过；T01–T07 七个 commit 与 `git log` 一一对应且均为 HEAD 祖先。

## 5. 验收账本独立复验（本轮重点）

### 5.1 校验器复跑（派发要求项）

`python3 -B docs/rust-tauri/R00/r00_t07_validate_ledger.py` → **`LEDGER_VALID checks=14945 scenarios=952 results=16 entries=832 spec_source=taskbook-strict`，exit 0**（与派发要求、报告 §5、修复报告 §8 完全一致）。

### 5.2 重建幂等性与「仅 3 处语义变化」核实

修复报告 §7.3 声称重建前后语义差异仅 ① `generated_at` ② RES-R00-A16 `source_digests[HANDOFF]` ③ 同记录 `working_tree_digest`。重建前快照（`e0bd3cfa…`）已被覆盖且未入版本控制，无法直接字节复验——我以两层等效闭合验证：

1. **幂等重建（内存重放，零写盘）**：monkeypatch `Path.write_text` 捕获输出，以当前工作区输入在内存重放 `r00_t07_build_map.py` → 输出与盘上 ACCEPTANCE_MAP **忽略 generated_at 后语义完全相等**、BLOCKERS.md **逐字节相等**（MAP-BUILT scenarios=952 tasks=100 features=743 entries=832 tests=31 results=16，与修复轮 ledger-rebuild-r3.log 计数一致）。⇒ 当前账本是生成器在当前输入下的忠实产物，无手工编辑；生成器确定。
2. **HEAD→当前语义 diff 全量归因（109 处逐一核对）**：差异全部落在 T08 执行者轮 R1 已验证的改动面（A09 source_paths/evidence/observed 扩展与守卫文件 digest、A13/A14 `committed_in` 补绑+`timestamp_basis` 注记、新增 A15/A16 记录〔`committed_in=null` 且 `tested_sha=e0b7be610` 合法待提交态〕、A15/A16 场景 NOT_STARTED→PASS、计数 PASS 30→32 / NOT_STARTED 185→183、tests 索引 28→31 条引发的 test_ids 平移、`basis.head` 4401afae2→e0b7be610）；**A01–A08、A10–A12 结果记录零漂移**；736 功能/952 场景规格内容零变化。
3. 结合生成器确定性 + 生成器输入（R00-T07_RESULTS `4d50dd10…`、build_map `5b8b94b5…` 两轮未动，其相对 HEAD 的 diff 全部为 T08 轮声明内容）+ 盘上 A16 绑定值实测 = 磁盘现值 `68877ff6…`、`working_tree_digest` = `2f561bb2…`（与修复报告声明终值一致）⇒ 修复轮相对 R1 时点的变化被约束在声明的 3 处（在可独立验证范围内成立）。

### 5.3 生成器输入与 BLOCKERS 不漂移

- `r00_t07_build_map.py` 相对 HEAD 仅 +5 行（negative.test.ts→A09、a15/a16 脚本→A15/A16 测试链接登记，带注释）——T08 执行者轮获准改动，修复轮零写入。
- `R00-T07_RESULTS.json` 相对 HEAD 的实质变化 = A09 扩展 + A13/A14 补绑 + 新增 A15/A16（其余为 JSON 缩进重排），与 R1 §3.4 已验证内容一致。
- `BLOCKERS.md` 相对 HEAD 仅基准头（4401afae2→e0b7be610）与计数（185→183、30→32）两处——与 R1 已验证的 T08 轮重建输出一致；且经内存重放证明 = 当前生成器输出（字节相等）。

### 5.4 A16 重绑正确性

抽查器复跑（§7.1）输出的 `handoff_sha256=68877ff6…` = 账本绑定值 = 磁盘 HANDOFF = 清单行 = 报告 §10 单列值，五方一致；RES-R00-A16 的 STALE-SOURCE（修复前留证 ledger-validate-stale-r1.log：`68877ff6… != recorded bde9eea9…`，exit 1）已被重绑消除且失败过程如实留证入清单。

## 6. A15 独立复验（PASS）

1. **复现校验器**：`python3 -B …r00_t08_a15_verify_repro.py audit-seal-repro-r1.log audit-seal-repro-r2.log` → `A15-REPRO-VERIFIED failures=4 identical_runs=2 …VERIFIED_SOURCE_SHA=46f12ab1c…`，exit 0。
2. **未修绿/未删除/未改白名单**：`.sync-audit/` 相对 HEAD **零改动**（`git diff --quiet` 通过）；4 用例分类、双日志与总控封印归属在 REPORT §7/HANDOFF unresolved_items 如实登记，无 PASS 伪装。
3. **对照组**：`r00_t04_scan.py --validate` 复跑 → `R00_T04_SCAN_OK`，exit 0（A_anchors=101、C_stores=69、负向 4 项 detected），与执行者 t04-validate-at-head.log 同结论——T02/T03 工具级预存失败的「同组有对照」叙事复证。
4. **工作区卫生**：patch.gz 与 HEAD 字节一致（全量 npm test 副作用未残留）；本轮全部探针在 /tmp 与 os.tmpdir()，无真实用户数据/供应商/付费/外发。

## 7. A16 独立复验（PASS）

### 7.1 执行者三点复验（新 HANDOFF）

`python3 -B …r00_t08_a16_spotcheck.py` → 功能/数据链/测试三点 PASS，`A16-SPOTCHECK-PASSED handoff_sha256=68877ff6…`，exit 0；我方输出 SHA `70e76dbf…` 与修复轮 a16-spotcheck-r2.log **字节相同**（确定性）。另按 spot_checks.test.rerun_command 以隔离 HOME/TMPDIR/LINGXI_HOME 真实重跑 migration 三文件 → **3 文件 / 21 用例全过，exit 0**（与 HANDOFF 预期、a16-test-rerun.log 一致）。

### 7.2 我方另选三个全新样本（避开执行者与 R1 各自样本，均过）

1. **功能（D13 工具域）**：`F-D13-TOOL-TOOL-AST-EDIT-96DD1F`（ast_edit 工具）→ 源锚点 `shared/tool-categories.ts:70`（OPTIONAL_TOOL_NAMES 含 ast_edit 亲见）+ `lib/sandbox/index.ts:200/227`（ast_edit 实现亲见）→ 账本 features_index 双向链齐全（R04 / R04-T05 / R04-A09、R04-A10 / 补充场景 R00-T02-LA-96DD1FF9E9D5 存在且 task_ids 一致）。
2. **数据链（不同存储）**：`knowledge-database`（authoritative/user_data, sqlite, epoch-managed）→ schema 契约源 `lib/knowledge/knowledge-store.ts` 在盘（KNOWLEDGE_SCHEMA_VERSION=19 亲见）；open_entry `new KnowledgeStore`（lib/knowledge/knowledge-manager.ts:288）与 `new KnowledgeManager`（core/engine.ts:546）均在盘，`dbPath: path.join(this.knowledgeRoot, "knowledge.db")`（knowledge-manager.ts:289）与 path_patterns 吻合。
3. **测试（不同命令）**：账本登记的 `r00_t04_scan.py --validate` 复跑 exit 0（见 §6.3）。

结论：另一执行者仅凭 R00_HANDOFF.json 及其指向交付即可定位固定 SHA/夹具/命令/预期并复现，A16 通过条件在新候选下仍成立。

## 8. 边界与纪律核查

- **R1 历史报告与总控账本**：R1 报告字节未变（§2）；ORCHESTRATOR_PROGRESS.json 相对 HEAD 的 diff 全部为总控流程记录（T07 置 DONE+commit/push、T08 派发与 R1 findings F1/F2=REPAIRED_PENDING_R2、repair_agent_id、`review_round:2`+`reviewer_id:ZCode:R00-T08 独立验收 R2（ZCode）`——本轮派发由总控写入，修复者不可能预知/无理由写入），其中引用的 R1 报告 SHA `8a832b5d…` 与盘上一致。⇒ 修复者未触碰总控账本（其会话终值 `d8468942…` → 当前 `402f5d75…` 的演进即总控派发 R2 的写入，见观察 N1）。
- **零生产行为改动**：`git diff --name-only HEAD` 过滤 `desktop/ server/ core/ lib/ shared/ cli/ hub/ plugins/ skills2set/ package*.json notarize` **零命中**；package-lock 与 HEAD 字节一致。
- **敏感数据**：PEM/私钥/AKIA/sk-/ghp_/xox 模式扫描（全部未跟踪文本 + tracked 全量 diff）**0 命中**。
- **whitespace**：`git diff --check HEAD` exit 0；78 个新文本文件 0 行尾空白；唯一制表符在 environment-r1.txt，为 `sw_vers` 原始输出（`ProductName:\t\tmacOS`）如实保留，非缺陷。
- **无循环摘要或遗漏**（§4）；修复报告两轮事实中所有机器可验断言（终值 SHA、28 新证据、5 日志 SHA、MAP-BUILT/SELFTEST/LEDGER_VALID 输出、闭合数字）逐一复验相符。

### 非阻塞观察（无 MUST_FIX）

| ID | 内容 | 处置建议 |
|---|---|---|
| N1 | ORCHESTRATOR_PROGRESS.json 当前 SHA `402f5d75…` ≠ 修复报告记录的会话终值 `d8468942…`；diff 内容为总控在修复后派发 R2 时追加的流程记录（含本轮 reviewer_id），非候选文件、非修复者产物 | 无需动作；总控后续重绑本报告结论时自然再更新 |
| N2 | R00_REPORT.md §10 L120「（不自嵌）。- 其余 T01–T07…」两个列表项挤在同一行（markdown 渲染级格式瑕疵，非事实错误；该行所有数字与引用均正确） | 可在总控重绑时顺手换行，不构成验收阻塞 |
| N3 | 修复报告 §1 演进表的中间态 SHA（REPORT `9cd6c6a2…`、SUMMARY `cad8b708…`、清单 `7dc6e4a0…`〔145 行〕）已被终态覆盖且未入版本控制，无法独立字节复验；其终值、重算方法与全部可验产物均验证一致，中间态按声明接受 | 固有限制，如实记录即可 |
| N4 | typecheck-r1.log 无显式 EXIT_CODE 标记（R1 O3 遗留；本轮 typecheck 未整轮重跑——lint/账本/测试等替代面已覆盖且 R1 曾复跑 exit 0） | 维持 R1「后续日志模板统一」建议 |
| N5 | replay-t08-driver.log、a15-repro-verify-r1.log、a16-spotcheck-r1.log 以成功标记行收尾但无显式 EXIT_CODE 行（spotcheck/a15 校验器我方已真实复跑 exit 0 补强） | 同 N4 |

## 9. 阶段放行条件与 R01 交接复核（沿 R1 §6 框架，逐项重验）

功能与写入点映射无遗漏（账本 952 场景/832 入口 LEDGER_VALID、T04 validate 复跑 exit 0）；隔离已证实（本轮全部探针 /tmp+tmpdir 隔离，migration 守卫负向回归 21/21）；基线失败已分类（A15 校验器复跑 exit 0、4 用例预存家族+T02/T03 工具级预存、未修绿）；验收账本与性能阈值可执行（LEDGER_VALID；PERFORMANCE_THRESHOLDS 哈希在 HANDOFF 复算通过）。未跑环境未宣称通过（REPORT §5 未运行项声明与 BLOCKERS 预登记一致）。HANDOFF 四类授权标志均 false 且本轮零动作；R01 输入（功能/入口/存储/Pi 清单、夹具、阈值、阻塞表）固定 SHA 全部复算在位。

## 10. 验收方复跑证据（/tmp/r2-review/，SHA-256 供事后审计）

```text
983b4e21…160b3  ledger-validate.log    （LEDGER_VALID checks=14945, exit 0）
70e76dbf…b210   a16-spotcheck.log      （A16-SPOTCHECK-PASSED handoff=68877ff6…, exit 0；与修复轮 r2 日志字节相同）
a89f64e9…503c   a15-verify.log         （A15-REPRO-VERIFIED failures=4 identical_runs=2, exit 0）
114a6723…e104c  migration-tests.log    （隔离 HOME 重跑 3 文件/21 用例, exit 0）
25b0f2b2…c094   lint.log               （npm run lint: 0 err/10887 warn/33 fixable, exit 0；与 R1 复跑字节相同）
96b1cf29…829b   selftest.log           （SELFTEST 18/18 +正面对照, 19/19 temp 清除, exit 0）
6b482913…35d4   t04-validate.log       （R00_T04_SCAN_OK, exit 0）
dda575ee…65c7f  check_manifest.py      （173 行清单逐条校验：0 mismatch/0 dup/排序✓）
15e5edb1…4e2d   check_closure.py       （闭合复算：86 可见=80+6、忽略 93 全在清单、物理 163=162+1）
ff3e14f1…8cf6   diff_map.py            （HEAD→当前账本语义 diff：109 处全归因）
a270ccd1…2504   rebuild_inmem.py       （内存幂等重建：仅 generated_at 差异、BLOCKERS 字节一致）
```

## 11. 最终结论与移交

- **A15 = PASS；A16 = PASS；T08 = PASS**。R1 的 F1/F2 在所有当前交付中彻底解决；O2 口径无新误导；修复报告两轮事实中全部机器可验断言准确；173 行候选清单、git 可见/忽略全集、聚合与 REPORT/SUMMARY/HANDOFF 引用全对；R1 历史报告与总控账本未被修复者触碰；无循环摘要或遗漏。
- **当前候选（173 文件 + 清单外 3 交付文档 + 总控账本预先存在修改）可以提交**；提交属总控获准流程，本验收未执行任何 commit/push。
- 非阻塞项 N1–N5 供总控酌情处理（N2 可随重绑顺手修正；N4/N5 为日志模板建议；N1/N3 为如实记录的固有限制）。
- 提交后账本 A13/A14/A15/A16 的 `committed_in=null` 需由获准路径（build_map 重建或总控封印流程）补绑实际提交 SHA——届时 HANDOFF 若被改动将按设计再触发 A16 STALE-SOURCE，属预期机制，重跑 §5.1 命令即可复核。
