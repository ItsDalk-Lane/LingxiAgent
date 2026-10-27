# R00-T08 独立验收报告 R1（REVIEWER-R00-T08-R1，ZCode）

验收对象：R00-T08「封存并交接基线」未提交候选（含 A15/A16）。
验收者：独立 ZCode 代理 REVIEWER-R00-T08-R1，与 T08 执行者及 T01–T07 代理隔离；只审查、不实现、不修复、不派生、不建分支/worktree、不 commit/push/PR/tag/release；篡改探针与复跑输出全部置于 /tmp 隔离根。
验收基点：分支 `codex/rust-tauri-migration`，HEAD = `e0b7be6108c4d5bc873061dee279b7163ca78a41`（与任务书派发基点一致，验收期间未变，无新建分支/worktree）。
时间：2026-09-24（本机 CST）。

## 0. 结论（先读）

| 项 | 结论 |
|---|---|
| **R00-A15 预存失败可重现** | **PASS**（独立复现、根因证实、预存性证实、未修绿未删除、归属正确） |
| **R00-A16 基线审阅可独立复查** | **PASS**（执行者三点复验全过 + 我方另选三个不同样本全过） |
| **R00-T08 封存并交接基线** | **FAIL（仅文档级 MUST_FIX×2：F1 候选计数内部不一致、F2 lint warning 口径错误）——当前候选不可提交**；证据与机器可验绑定本身全部核实无误，修复为纯文档勘误，无需重跑任何验证 |

技术实质全部通过；FAIL 仅因封存文档自身两处事实性口径错误（详见 §5 发现表）。按派发规则「有任何必须修复项判 FAIL」，交总控创建全新 ZCode 修复任务（建议范围仅：R00_REPORT.md §2/§13 计数与 §5/§6 warning 措辞、R00_HANDOFF.json verification_battery 的 lint expected 措辞；改后重算两文件 SHA 并由总控重绑）。修复不涉及任何证据文件、测试、账本或源码。

## 1. 输入与独立性声明

已完整读取：AGENTS.md；任务书 00/01/05/90/91 与 R00 阶段书（T08/A15/A16 规格及 §6 放行条件）；R00_REPORT.md、R00_HANDOFF.json、R00_EVIDENCE_SUMMARY.md、T08/CANDIDATE_MANIFEST.txt；T05 REVIEW_R1（F01–F04）、T06 REVIEW_R2（F08/F09/F10/O1）、T07 REVIEW_R1–R3 与 REPAIR_R1/R2；实际工作区 diff、账本与全部关键 T08 日志。未采信执行者任何 PASS 宣称，以下全部结论均基于我方重算/复跑/源码核对。未改任何候选文件与 ORCHESTRATOR_PROGRESS.json；本报告是本次验收唯一新增仓库文件。

## 2. 交付哈希重算与文件集合全量闭合

### 2.1 四文件 SHA-256 重算（全部与派发值一致）

| 文件 | 派发 SHA | 我方重算 | 一致 |
|---|---|---|---|
| docs/rust-tauri/R00/R00_REPORT.md | 916b8a30…70dd87 | 同 | ✓ |
| docs/rust-tauri/R00/R00_HANDOFF.json | bde9eea9…abe2547 | 同 | ✓ |
| docs/rust-tauri/R00/R00_EVIDENCE_SUMMARY.md | 8013781a…647274 | 同 | ✓ |
| artifacts/rust-tauri/R00/T08/CANDIDATE_MANIFEST.txt | 14e399ef…403310a | 同 | ✓（145 行） |

### 2.2 闭合核对（git 可见集 + 忽略集，程序化全比对）

- 清单 145 条：路径唯一、**逐条 SHA-256 与磁盘字节一致（0 不匹配）**；聚合 SHA = 清单文件自身 SHA（自引用规避成立；HANDOFF 已入清单且哈希正确，报告/摘要/清单自身/总控账本按声明排除，四者均在盘可寻）。
- git 可见全集（`-uall`）82 文件 = 清单内 7 个已跟踪修改 + 71 个新文件 + 4 个声明排除（ORCHESTRATOR_PROGRESS.json〔总控预先存在修改，内容为 T07 置 DONE/T08 派发记录，非 T08 产物〕、R00_REPORT.md、R00_EVIDENCE_SUMMARY.md、CANDIDATE_MANIFEST.txt）——**零遗漏、零多余**。
- 忽略集（--ignored，候选根范围内 83 项）：其中 **67 个被 .gitignore 忽略的原始 .log 证据已全部纳入清单**（闭合正确）；清单外仅剩 16 个非证据残留：`docs/rust-tauri/R00/__pycache__/*.pyc`×8 与 `.mimosa/` 钩子状态×8（T06 下 2 处报告 §8 已披露；docs 下 6 处为执行/审查环境插件残留；均被忽略、不入库、非证据）→ 归入非阻塞观察 O1。
- T08 目录物理 135 文件 = 清单 134 + 清单自身。未发现伪造（日志时间窗连贯、内容与复跑一致）、陈旧证据或漏列证据；patch.gz 当前与 HEAD 字节一致（`git diff --quiet` 通过，SHA 9e858daf…）。

### 2.3 候选计数问题裁定（派发方提问：措辞差异还是实质不一致）

**裁定：需修正的实质不一致（陈旧计数），非措辞差异；但权威绑定未受损。**
- §10「145 = 7 修改 + 138 新」与 CANDIDATE_MANIFEST.txt 实测完全一致，为权威口径。
- §2「T08 候选 = 140 文件（§10）」与 §13「删除 133 个新文件」（7+133=140）互相一致但与 §10 矛盾；145−140=5 恰等于终稿前补录的 5 个后取证文件（ledger-validate-final2.log、t02/t03/t04-at-head 三日志、t02-t03-checker-diagnosis.md）——两节未随后补证据同步更新。
- §13 回退口径实际应为：restore 7 修改 + 删除 141 个新文件（138 清单新 + 报告 + 摘要 + 清单自身），按「133」执行将残留 8 个文件。→ MUST_FIX F1。

## 3. R00-A15 独立验收（PASS）

### 3.1 验证组合独立复跑（命令/预期/实际/退出码；我的原始输出在 /tmp，SHA 见 §7）

| 命令 | 预期 | 我方实际 | 退出码 |
|---|---|---|---|
| `npm run typecheck` | 0（tsc×3） | 无错误输出 | 0 |
| `npm run lint`（候选态） | 0 错误 | 0 错误 / **10887 warning**（33 可 --fix） | 0 |
| `vitest run tests/post-verification-audit-seal.test.ts` | 同条件重现 1 失败 | 同一用例失败（集合与执行者日志一致） | 1 |
| `LINGXI_MIGRATION_BLOCK_NETWORK=1 vitest run tests/migration/{r00-t05-replay,r00-a10-old-defect,network-guard-negative}.test.ts` | 3 文件/21 用例 | 3 passed / 21 passed | 0 |
| `node scripts/rust-tauri/r00-t05-replay.mjs --out /tmp/r1-review-replay`（三次重放） | 规范化逐字节一致 | run1=run2=run3 identical（9 夹具；raw 差异仅为声明允许的 tmp 路径字段） | 0 |
| `python3 -B …r00_t07_validate_ledger.py` | LEDGER_VALID | LEDGER_VALID checks=14945 scenarios=952 results=16 | 0 |
| `python3 -B …r00_t07_selftest.py --suite all`（/tmp） | 18/18 | 18/18 + 正面对照，temp 19/19 清除 | 0 |
| `python3 -B …r00_t02_inventory.py --negative-checks` | STALE（预存） | STALE（同日志） | 1 |
| `python3 -B …r00_t03_validate.py` | 1677≠1664（预存） | AssertionError: independent=1677, graph=1664 | 1 |
| `python3 -B …r00_t04_scan.py --validate` | OK | R00_T04_SCAN_OK | 0 |
| `node .sync-audit/verify-post-verification-diff.mjs` | 失败清单=已提交 R00 增量 | ✗ 列出 .gitignore/旧任务书删除/T01–T07 产物——**全部为已提交内容，无一项来自 T08 未提交候选** | ≠0 |

其余未重跑项（build:renderer/knowledge-smoke/两 boundaries/全量 npm test）：原始日志逐份核对，退出标记与数值自洽（15 文件/125 用例、2031/2250 文件、7.05s 构建），且其结论已被我复跑的替代面（typecheck/lint/migration/账本）交叉印证；全量 npm test 未整套复跑以避免已知 patch.gz 重写副作用破坏候选（我以单文件三度重现 + 根因直证替代，见 3.2）。

### 3.2 四个审计封印失败的「同条件可重现 + 相对 T08 预存」证实

1. **可重现**：执行者双轮（audit-seal-repro-r1/r2.log，起跑 12:43:23/12:43:37，各含 4 条相同 FAIL）+ 我方第三次复跑（/tmp/r1-review-audit-seal.log，同一用例失败、另 2 用例过）→ 同命令同环境同 HEAD 三次失败集合完全一致；`r00_t08_a15_verify_repro.py` 我方复跑 exit 0（A15-REPRO-VERIFIED failures=4 identical_runs=2），其内置 EXPECTED_FAILURES 与日志吻合。
2. **根因**：直跑 verify-post-verification-diff.mjs，违规清单全部是 T01 首提交（16aeb380d）起的已提交 R00 增量与 60dbe0384 旧任务书移出/.gitignore——即 VERIFIED_SOURCE_SHA=46f12ab1c 白名单不含 R00 增量；与 T08 未提交候选无关（该脚本按 AGENTS.md 比较已提交版本）。
3. **预存性**：git log 证实 16aeb380d 为 R00 首个提交且其新增 artifacts/rust-tauri/R00/T01/* 均在违规清单中 → 自 R00 首任务提交即失败，非 T08 引入。
4. **处置纪律**：未写 PASS、未删用例、未改白名单（.sync-audit/ 全目录相对 HEAD 零改动）；失败双日志+校验器输出完整在盘；影响与阻塞归属（总控封印流程、不阻塞 R00 技术交付）符合 AGENTS.md「不为变绿扩白名单/退役门禁/虚报坐标」。附带发现 ROUND2-TEST-SIDEEFFECT 属实（patch.gz 已恢复至 HEAD 字节）。

### 3.3 T02/T03 检查器 STALE 分类证实

- **T02**：我在内存中以 `build()` 只读再生成并深度逐字段 diff 三份冻结交付——每份仅 1 处差异且均为 `tested_sha`（16aeb380d→e0b7be610），736 功能内容零漂移，证实「纯戳差异、树移动重绑语义、预存」。
- **T03**：我方复跑同失败（independent=1677 vs graph=1664）；差值与「T03 冻结后 T05+ 新增源文件」叙事吻合（ffcb85830..HEAD 新增 scripts/rust-tauri/*.mjs×14、tests/migration 源文件×5 等）；T03 历史 PASS 绑定其验证树，不受损。对照组 T04 `--validate` exit 0 亦复现。

### 3.4 follow-up 修复核验（§6 三处 + F02）

- **T05 R1-F01（MEDIUM）已修**：network-guard.ts diff 实读——net.createConnection 补丁、tls.createConnection 按存在性补丁、isPipe 收紧为「显式 options.path 或含 `/` 字符串」（`net.connect(443,"host")` 位置形不再放行）、dns.promises.lookup + resolve*（回调与 promises）覆盖，残留局限在头注诚实声明；新增负向回归 7 用例真实覆盖三类绕过边界 + http.Agent→createConnection 实路径 + 管道/本地放行面（`/tmp/...sock` 得 ENOENT 非拦截、127.0.0.1 放行），我方复跑通过；强化守卫下三次重放规范化仍逐字节一致（§3.1）。
- **T05 R1-F02 已修**：FIXTURE_MANIFEST auth 第 5 条散文与 auth/cases.json+expected.json 五用例名逐一对应（valid-local-loopback / wrong-token-local / valid-token-lan / missing-credential / connection-not-allowed）。
- **eslint 修复**：diff 恰为 docs/**/*.{js,mjs} 纳入 node-globals 块（+2 行 +注释）；lint-r1/r2 双日志留证真实（r1 exit 1 单错误 r00_t03_import_graph.mjs:254 console no-undef；r2 同警告集 0 错误）。
- **T07 committed_in 补绑**：首次重建失败轮如实留证（ledger-selftest-r1 = 17/18，正面对照 a14-v3 因真实账本 RESULT-BAD-FIELD 失败，日志含精确报错 tested_sha=4401afae2 vs head=e0b7be610）；修复后 r2 18/18、LEDGER_VALID。语义 diff 全量比对证实账本历史证据未被破坏：A01–A08、A10–A12 记录零改动，仅 A09 source_paths/evidence/observed 扩展、A13/A14 committed_in+timestamp_basis 注记、新增 A15/A16（21 必填字段、committed_in=null 且 tested_sha==e0b7be610 合法待提交态）；BLOCKERS 仅基准头与计数随重建更新（PASS 30→32、NOT_STARTED 185→183，与 §10 一致）；ACCEPTANCE_MAP 内 A13/A14 tested_sha=4401afae2 由构建器回填（build_map L206），与报告措辞相符。
- **R00 零生产行为改动**：`git diff --name-only HEAD` 过滤 desktop/server/core/lib/shared/cli/hub/plugins/skills2set/package*/notarize 全为空；package-lock SHA 与 HEAD 一致（e54a16fe…）；敏感模式扫描（PEM/私钥/AKIA/sk-/ghp_/xox）于 T08 证据与全 diff 零命中；验证数据全在 os.tmpdir() 与 /tmp。

### 3.5 A15 口径问题（→ F2，MUST_FIX）

报告 §5/§6 与 HANDOFF verification_battery 称「33 预存 warning 保留」。**实测（lint-r2.log 与我方复跑一致）：总 warning=10887、错误=0、其中可 `--fix` 自动修复=33**。33 仅为可自动修复子集，作为「保留的预存 warning 数」陈述失实（低估两个数量级），且写入 HANDOFF 的 expected 字段会误导后续复核者。→ MUST_FIX F2。

## 4. R00-A16 独立验收（PASS）

### 4.1 执行者三点复验（仅凭交接+仓库）

- 抽查器 `r00_t08_a16_spotcheck.py` 我方复跑 exit 0（A16-SPOTCHECK-PASSED，handoff_sha256=bde9eea9…）；脚本实读为真实结构校验（非自说自话）。
- 功能点人工复核：server/routes/file-history.ts 第 30 行亲见 `route.get("/file-history/files"…)`；mapping 链与账本双向链接在位。
- 数据链点：multi-turn-basic 两夹具 SHA-256 我方独立重算 = d0c599b4… / 62764808…，与 HANDOFF 固定值一致。
- 测试点：我以真实 shell 复跑 rerun_command → exit 0 / 3 文件 / 21 用例（强于脚本仅验日志摘要）；证据日志 SHA de6fcf3a… 复核一致。

### 4.2 我方另选三个不同样本（验证交接可复用性，均过）

1. **功能（不同域 D16）**：`F-D16-BUILTIN_PLUGIN_TOOL-…-MEDIA-GENERATE-VIDEO-A2E160`（media · generate-video）→ 盘点映射 stages [R04,R07]/tasks [R04-T07,R07-T05] → 源锚点 plugins/media/tools/generate-video.ts 在盘 → 账本场景 `R00-T02-LA-A2E1607C1102`（task_ids 一致）→ features_index 反向链接在位。
2. **数据链（不同存储）**：`user-preferences`（authoritative/user_data, json）→ site_rule_files core/preferences-manager.ts（class L83）+ core/engine.ts（`new PreferencesManager` L539）均在盘且职责对应。
3. **测试（不同命令）**：按 BASELINE.json targeted_tests.command 以隔离 /tmp HOME/TMPDIR/LINGXI_HOME 复跑 startup-contract/hana-runtime-paths/server-composition-boundary → exit 0 / 3 文件 / 26 用例，与 BASELINE 固定预期（3/26）一致。

结论：另一执行者不依赖原模型记忆，仅凭 R00_HANDOFF.json 及其指向的交付接口即可定位固定 SHA/夹具/命令/预期并复现——A16 通过条件满足。

## 5. 发现清单

| ID | 级别 | 内容 | 处置建议 |
|---|---|---|---|
| **F1** | **MUST_FIX（LOW 严重度，文档事实错误）** | 报告 §2「140 文件」与 §13「133 个新文件」为陈旧计数，与权威 §10「145=7+138」及清单矛盾（差值=终稿前补录的 5 个 T02/T03 取证文件）；§13 回退删除清单按 133 执行将残留 8 个文件（138−133=5 清单新文件 + 报告 + 摘要 + 清单自身） | 修报告 §2/§13：候选统一 145；§13 改为「restore 7 + 删除 141 新文件（138 清单新 + 报告/摘要/清单自身）」或明列排除项。纯勘误，不动证据 |
| **F2** | **MUST_FIX（LOW 严重度，口径错误）** | 「33 处预存 warning 保留」（报告 §5 表、§6.1；HANDOFF verification_battery.expected）失实：总 warning=10887（0 错误），33 仅为可 --fix 自动修复数（lint-r2.log 与我方复跑双证） | 改为「0 错误 / 10887 处预存 warning 原样保留（其中 33 处可 --fix 自动修复）」；HANDOFF expected 同步。纯勘误 |
| O1 | 非阻塞（INFO） | 清单外忽略残留：`docs/rust-tauri/R00/__pycache__/*.pyc`×8、`.mimosa/` 钩子状态×8（报告 §8 仅披露 T06 下 2 处）——均被 .gitignore 覆盖、非证据、不入库，不影响闭合与提交内容 | 无需动作；可在 F1 勘误时顺带一句披露 |
| O2 | 非阻塞（INFO） | §5「14895/14899 用例」分母剔除了 15 skipped（vitest 全口径 14914=14895P+4F+15S）；「1463/1469 文件」同理含 3 skipped。数值无害但非 vitest 原口径 | 勘误时可顺带写全 passed/failed/skipped |
| O3 | 非阻塞（INFO） | typecheck-r1.log 无显式 EXIT_CODE 标记（tsc 成功静默，其余日志均有标记）；exit 0 由空错误输出 + 我方复跑双证 | 后续日志模板统一加标记即可 |

## 6. Stage gate / R01 / 回退 / 未授权事项核查

- **R00 放行条件（任务书 §6）**：功能与写入点映射无遗漏（T02/T04 已验收且 HEAD 复核一致/通过）；隔离已证实（T01 已验收；本轮重放/测试均在 tmp 隔离 + 守卫负向回归护住离线前提）；基线失败已分类（4 用例预存家族 + 2 工具级预存，均我方证实）；验收账本与性能阈值可执行（LEDGER_VALID / PERFORMANCE_THRESHOLDS 固定哈希在 HANDOFF）。未跑环境未宣称通过（§5 未运行项如实声明，BLOCKERS 预登记齐备）。
- **R01 输入与边界**：HANDOFF 交付功能/入口/存储/Pi 依赖清单、夹具、阈值、阻塞表（固定 SHA）；接口/数据面仅「现役现状盘点」，无任何依赖或协议升级（package*.json 零改动证实）；四类授权标志（remote_write/production_data/paid_or_external_test/release）均 false 且本轮零动作。
- **未授权事项**：零 commit/push/PR/tag/release/外发（HEAD 未动、无新分支/worktree、远端未触碰）；未用真实用户数据/真实供应商/付费调用（环境快照含 ALL_PROXY 但全部验证本地化，报告已声明且与证据相符）。
- **可回滚性**：7 个修改文件均可 `git restore`（其 HEAD 版本存在）；新文件均为本任务产物可删（正确总数见 F1）；账本可由 r00_t07_build_map.py 从任务书+盘点重建（r1/r2 重建日志 + 我方 validate 复跑证明可重现）；两处已实际演练的恢复（T05 15 文件、patch.gz）我方均验证恢复至 HEAD 字节。回退能力成立，唯 §13 文字计数需修（F1）。

## 7. 验收方复跑证据（/tmp 隔离根；SHA-256 供事后审计）

```text
43456322…29042  /tmp/r1-review-audit-seal.log   （审计封印第3次重现：1 failed|2 passed, exit 1）
25b0f2b2…c094  /tmp/r1-review-lint.log         （npm run lint: exit 0, 0 err/10887 warn/33 fixable）
437309f2…2564  /tmp/r1-review-migration.log    （migration 三文件: 3 files/21 tests, exit 0）
25a1dc7c…7a36  /tmp/r1-review-typecheck.log    （npm run typecheck: exit 0）
983b4e21…160b3  /tmp/r1-review-ledger.log       （LEDGER_VALID checks=14945）
80463686…d3396  /tmp/r1-review-selftest-driver.log（18/18 + positive control, exit 0）
b13e7402…3eb8  /tmp/r1-review-replay-driver.log （三 run identical, PASS-CANDIDATE, exit 0）
f0751c4e…cf2f7  /tmp/r1-review-targeted.log     （BASELINE targeted: 3 files/26 tests, exit 0）
2c528769…1325  /tmp/r1-review-t02.log           （STALE, exit 1）
4c77efab…5375  /tmp/r1-review-t03.log           （1677≠1664, exit 1）
ee651b0c…bf31  /tmp/r1-review-t04.log           （R00_T04_SCAN_OK, exit 0）
```

## 8. 最终结论与移交

- **A15 = PASS；A16 = PASS；T08 = FAIL（F1+F2 两项文档级 MUST_FIX；O1–O3 非阻塞）**。
- 候选**当前不可提交**：证据、清单闭合、账本、测试与全部机器可验绑定经我方独立复核全部真实且一致；但封存报告/交接文档自身的两处事实性口径错误（候选计数自相矛盾、warning 数失实）属交付物缺陷，须先修。
- 修复为**纯文档勘误**（R00_REPORT.md §2/§5/§6/§13 + R00_HANDOFF.json lint expected；不动任何证据/测试/账本/源码，无需重跑验证）。修后两文件 SHA 将变化，总控需重绑坐标并可将本报告结论沿用作技术实质复核依据（如需，可仅差分复核勘误行）。
- 其余建议（非阻塞）：O1 残留披露、O2 skipped 口径、O3 日志标记统一，可随勘误顺手处理或留后续。
