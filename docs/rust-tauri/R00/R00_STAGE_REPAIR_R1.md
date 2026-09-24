# R00 阶段修复报告 R1（STAGE-REPAIR-R00-R1，ZCode）

任务：修复 R00 阶段级独立验收 R1（[R00_STAGE_REVIEW_R1.md](R00_STAGE_REVIEW_R1.md)，结论 FAIL）的全部 MUST_FIX（F1–F3），完成后停止，**不自行宣布阶段 PASS**。
执行者：阶段修复代理 STAGE-REPAIR-R00-R1（ZCode），与阶段验收 R1 验收者相互独立；本轮零委派、零分支/worktree、零 commit/push/PR/tag/release、零真实用户数据/真实供应商/付费调用/真实外发。
基点：分支 `codex/rust-tauri-migration`；当前 HEAD = T08 交付提交 `8b153b1031bbb01204375b08e9caaa891397d7a5`；stage base `7d1a0c6bc28062ff455adcf8f68c3a80b117e90d`；T08 任务基点（历史执行时点）`e0b7be6108c4d5bc873061dee279b7163ca78a41`。
时间：2026-09-24（本机 CST）。

## 0. 结论（先读）

| 项 | 结论 |
|---|---|
| F1（提交后坐标未重绑） | **已修复**：HANDOFF/账本/结果记录全部经获准生成链重绑真实 T08 提交；校验器新增窄负例 `COMMIT-PENDING-BASIS` 防止旧 basis.head + 空 committed_in 假绿（实测恰好单错误命中） |
| F2（四个封印失败的两基点分类含混） | **已修复**：报告 §7/§11、HANDOFF `AUDIT-SEAL-PREEXISTING`、证据摘要 §4 统一两基点表述；两基点实证本轮复跑留证（stage base exit 0 / HEAD exit 1）；四个 FAIL 全保留未修绿 |
| F3（T03 R3 两条 LOW 后续项未入交接） | **已修复**：HANDOFF `unresolved_items` 新增 `T03-R3-F01`/`T03-R3-F02`（源码定位、T03 R3 引用、唯一承接任务 R07-T12、最晚阶段 R07、复验办法）；报告 §11 同步；未追改 T03 原 PASS，未改冻结矩阵与产品代码 |
| 阶段放行状态 | **READY_FOR_STAGE_REREVIEW**——请总控创建**全新 Codex 阶段复验子代理**；正式封印推进属总控在复验 PASS 后按 PROGRESS.md seal 工作流的收口动作 |

当前修复后候选 = HEAD `8b153b103` + 本报告 §6 工作区改动（未提交；提交推送由总控在阶段复验 PASS 后执行）。

## 1. 输入与独立性声明

已完整读取：AGENTS.md；任务书 00/01/02/03/04/05/06/90/91 与 R00 阶段书（`Lingxi_Rust_Tauri_Taskbooks_2026-09-23/`，未跟踪）；stage-index/task-catalog/acceptance-catalog；T03/T07/T08 报告及其各轮独立验收（含 T03 R3 的 F01/F02 原文、T08 R1/R2）；R00_REPORT/R00_HANDOFF/R00_EVIDENCE_SUMMARY/BLOCKERS；账本生成器/校验器/自测全部源码；PROGRESS.md seal 工作流；`.sync-audit/verify-post-verification-diff.mjs`（只读核对）。未采信任何既有会话记忆，以下结论均基于本轮重算/复跑/源码核对。未编辑总控账本 `docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`（其未提交修改为本任务前已存在，原样保留）；未改动独立验收报告 `R00_STAGE_REVIEW_R1.md`。

## 2. F1 根因与修复（交接/账本坐标停在上次提交前状态）

### 2.1 根因（独立诊断）

T08 经任务级独立验收 R2 PASS 后由总控提交为 `8b153b103`（当前 HEAD，本轮已按提交字节核验候选清单 173/173 一致，见 §5 表第 2 行），但交付面仍停留在 T08 提交前的待提交态：

1. `R00_HANDOFF.json`：`source_sha` 仍为 `e0b7be610`（T08 任务基点），`accepted_tasks` 无 R00-T08 条目；
2. `ACCEPTANCE_MAP.json`：`basis.head` 仍为 `e0b7be610`，RES-R00-A15/A16 `committed_in=null`（提交前合法的待提交态）；
3. `BLOCKERS.md` 基准头行、R00_REPORT 头部/§4 T08 行同样停留在待提交表述。

**漏洞环节**（为何账本校验没有拦住）：校验器对 `committed_in=null` 的结果只要求 `tested_sha == basis.head`（当前任务待提交的合法形态），但**从未核对 `basis.head` 是否仍是当前 Git HEAD**。T08 提交把 HEAD 推进到 `8b153b103` 后，「旧基准头 + 空 committed_in」这一**过期**待提交态仍能通过校验——假绿窗口成立。

### 2.2 修复内容（全部经获准生成/校验链，无手工账本编辑）

| # | 改动 | 说明 |
|---|---|---|
| 1 | `R00-T07_RESULTS.json`：RES-R00-A15/A16 `committed_in` → `8b153b103…`；`timestamp_basis` 追加补绑注记 | `tested_sha` 保留执行时点 `e0b7be610` 原值——**未伪称旧测试在提交后运行** |
| 2 | `r00_t07_validate_ledger.py`：新增规则族 `COMMIT-PENDING-BASIS` | 存在 `committed_in=null` 待提交结果且 `basis.head` ≠ 当前 Git HEAD 时拒绝；**无待提交结果时休眠**（checks 计数不变，实测仍 14945） |
| 3 | `r00_t07_selftest.py`：a13 套件新增变体 `v13-commit-pending-stale-basis` | 构造旧 basis.head + 空 committed_in，断言校验器非零退出且命中 3 条子串；**未新增账本场景**（新变体绑定历史 T07 运行会伪造出处，故仅作脚本级耐用负例） |
| 4 | `R00_HANDOFF.json`：`source_sha` → `8b153b103…`；`accepted_tasks[R00-T08]` 绑定真实提交并引用既有 R2 验收；新增顶层 `amendment_note` | `task_base_sha`/`stage_base_sha`/spot_checks/artifact_hashes 等历史字段保留；status 维持 READY_FOR_REVIEW（阶段放行不归本修复自评） |
| 5 | `python3 -B docs/rust-tauri/R00/r00_t07_build_map.py` 重建账本与 BLOCKERS.md | 获准的账本更新路径；`basis.head` → 当前 HEAD，A16 `source_digests` 重绑新 HANDOFF 哈希（HANDOFF 不钉账本，无循环摘要） |

**重建语义 diff（程序化全量比对，committed vs 重建后）**：恰好 **19 处差异全部归因**——`generated_at`、`basis.head`、`basis.input_sha256[R00-T07_RESULTS.json]`、A13/A14 各 3 条输入 source_digests + 各自 working_tree_digest、A15 `committed_in`+`timestamp_basis`、A16 `committed_in`+`timestamp_basis`+HANDOFF source_digest+working_tree_digest、tests 索引两个脚本哈希；**A01–A12 结果零漂移**，scenarios(952)/tasks(100)/features_index(743)/entrypoint_index(832)/spec_digests(200) **0 字段变化**，counts 完全一致（证据：`ledger-semantic-diff-r1.log`，exit 0）。

### 2.3 负例实测

`a13-v13-commit-pending-stale-basis.log`：篡改后校验器输出恰 1 条错误 `LEDGER-ERROR COMMIT-PENDING-BASIS basis.head results ['RES-R00-A16'] … basis.head e0b7be610… != current git HEAD 8b153b103…`，`errors=1 checks=14946`，expect_exit=1 actual_exit=1，子串 3/3 命中。旧形态的过期待提交态从此无法假绿。

## 3. F2 根因与修复（四个审计封印失败的两基点分类）

### 3.1 根因

四个失败用例（post-verification-audit-seal、round2 R10-03/R10-04、round3 manifest）的原始表述只称「预存」，未区分两种基点，导致「相对谁预存」含混，且正式封印的收口归属不清。

### 3.2 本轮两基点实证（真实复跑，证据在盘）

- **相对 T08 任务基点 = 预存**：审计白名单不含自 R00 首个提交 `16aeb380d` 起的 R00 增量，T08 未引入（封印脚本只比对已提交内容，与 T08 候选无关；HEAD 隔离副本复跑审计封印三文件仍为**同 4 用例 FAIL**，4 failed | 14 passed，exit 1）。
- **相对 R00 stage base = 本阶段提交触发**：同一脚本在 stage base（`7d1a0c6bc`）隔离副本 **exit 0**（仅 6 个审计文件变化），在 HEAD 隔离副本 **exit 1**（641 个非白名单路径）——差异全部来自本阶段 T01–T08 的已提交内容。

### 3.3 修复内容

- `R00_REPORT.md` §7 新增两基点分类段、§11 风险表封印行重写、§17 增补记录本节事实；
- `R00_HANDOFF.json` `unresolved_items[AUDIT-SEAL-PREEXISTING]`：`kind` 改 `stage_triggered_gate_failure`，写入两基点分类与实证命令/退出码，明确**正式封印推进 = 总控在阶段复验 PASS 后按 PROGRESS.md seal 工作流收口，不归 R01**；
- `R00_EVIDENCE_SUMMARY.md` §4 同步两基点表述。

**纪律**：四个 FAIL 全部保留，未修绿、未删测、未改预期、未扩白名单、未退役门禁、未虚报验证坐标；`.sync-audit/` 全目录零改动；全量测试在任何文档中均不得表述为 PASS。

## 4. F3 根因与修复（T03 R3 两条 LOW 后续项入交接）

### 4.1 根因

T03 独立验收 R3 判 PASS 时留下两条 LOW 后续观察项，但 T08 封存时未纳入 HANDOFF `unresolved_items`，交接面不完整。

### 4.2 修复内容（未追改 T03 原 PASS；未改产品代码与冻结矩阵）

HANDOFF `unresolved_items` 新增两条（kind=`follow_up`），报告 §11 风险表同步：

- **T03-R3-F01**：`lib/desk/heartbeat.ts` 巡检状态写入/指纹去重的写入锚点未做行级登记（执行链由 W1-3 经 hub/scheduler.ts 覆盖）。承接：唯一任务 **R07-T12**（外围 worker 账本），最晚阶段 **R07**。复验：重跑 `r00_t03_build_matrices.py` + `r00_t03_validate.py` 确认差集为空，`tests/cron-scheduler.test.ts` 作旁证。
- **T03-R3-F02**：`lib/autolearn/autolearn-service.ts` 后台模型作业与技能安装写入的相邻形态未按 W15 先例登记（PI-08/D21 边界已覆盖）。承接：唯一任务 **R07-T12**，最晚阶段 **R07**。复验：同上矩阵重建校验链。

## 5. 实跑命令与退出码（全部本轮真实运行；日志在 `artifacts/rust-tauri/R00/STAGE_REPAIR_R1/`）

| # | 命令（要点） | 结果 | 退出码 | 日志 SHA-256 |
|---|---|---|---|---|
| 1 | 环境快照（node/npm/python/vitest/git/平台） | 记录 | 0 | `environment-r1.txt` = `55e8207d…b8ea1` |
| 2 | T08 候选清单 173 文件 vs 提交 `8b153b103` 字节核验 | 173/173 一致 | 0 | `t08-manifest-vs-commit-verify.log` = `a37c6f71…e88fc` |
| 3 | `python3 -B …/r00_t07_build_map.py`（账本+BLOCKERS 重建） | MAP-BUILT scenarios=952 tasks=100 features=743 entries=832 tests=31 results=16 | 0 | `ledger-rebuild-r4.log` = `10a851c7…e96e2c1` |
| 4 | 账本语义 diff（committed vs 重建）+ 零漂移断言 | TOTAL_DIFFS=19 全归因；A01–A12 零漂移；五类集合 0 变化 | 0 | `ledger-semantic-diff-r1.log` = `ffa96e19…77ee067` |
| 5 | `python3 -B …/r00_t07_validate_ledger.py` | LEDGER_VALID checks=14945 scenarios=952 results=16 entries=832 spec_source=taskbook-strict | 0 | `ledger-validate-r4.log` = `f808595f…bb8862` |
| 6 | `python3 -B …/r00_t07_selftest.py --suite all`（隔离 fake root） | SELFTEST PASS：a13 15/15（含新 v13）+ a14 4/4 + 正面对照过；temp 20/20 清除 | 0 | driver = `3299d69f…8f77bb`；`ledger-selftest-r4/SELFTEST_SUMMARY.json` = `f6340ebf…10337`；v13 = `791f8346…846e5` |
| 7 | `python3 -B …/r00_t08_a16_spotcheck.py` | A16-SPOTCHECK-PASSED handoff_sha256=`c925e839…`（与账本绑定值一致） | 0 | `a16-spotcheck-r3.log` = `88de943b…d2f5` |
| 8 | `python3 -B …/r00_t08_a15_verify_repro.py` | A15-REPRO-VERIFIED failures=4 identical_runs=2 | 0 | `a15-repro-verify-r2.log` = `a89f64e9…03c` |
| 9 | `LINGXI_MIGRATION_BLOCK_NETWORK=1 vitest run tests/migration/{r00-t05-replay,r00-a10-old-defect,network-guard-negative}.test.ts`（隔离 HOME/TMPDIR/LINGXI_HOME） | 3 文件 / 21 用例全过 | 0 | `migration-tests-r2.log` = `282a8ac4…2e02` |
| 10 | 六组存储/认证 vitest（同隔离） | 6 文件 / 74 用例全过 | 0 | `store-security-tests-r2.log` = `4cf9124d…2cc58` |
| 11 | `node scripts/rust-tauri/r00-t05-replay.mjs --out <隔离目录>` ×3 | 三轮规范化逐字节一致，PASS-CANDIDATE | 0 | driver = `149c8ded…1d7e`；`replay-sr1/` 目录在清单内 |
| 12 | T02/T03/T04 检查器复跑 | T04 R00_T04_SCAN_OK；T02 STALE（纯 tested_sha 戳，预存）；T03 1677≠1664（设计性 scope 断言，预存） | 0 / 1 / 1 | `t02-t03-t04-checkers-r2.log` = `912fea72…d843` |
| 13 | 封印脚本两基点实证：`node .sync-audit/verify-post-verification-diff.mjs` @ stage base 隔离副本 / @ HEAD | stage base：仅 6 审计文件变化；HEAD：641 非白名单路径 | **0 / 1** | base = `85775209…00b5`；head = `42ed8099…400f` |
| 14 | 审计封印三文件 vitest @ HEAD 隔离副本（symlink node_modules） | 同 4 用例 FAIL（4 failed \| 14 passed），与既有双轮复现一致 | 1（预期内） | `audit-seal-trio-head-clone.log` = `eccf4e2c…ad88` |
| 15 | HANDOFF artifact_hashes 18 项对盘复核（python 逐文件 sha256 比对） | checked 18 mismatch 0 | 0 | 终端实录（本表；未落盘日志） |
| 16 | 全部文档编辑后复跑校验器 + A16 spotcheck | LEDGER_VALID checks=14945；A16-SPOTCHECK-PASSED handoff_sha256=`c925e839…` | 0 / 0 | `final-ledger-validate.log` = `983b4e21…60b3`；`final-a16-spotcheck.log` = `17ea7a97…1aae` |
| 17 | `git diff --check`；patch.gz ×2 pristine 核验；敏感模式扫描 | 无空白错误；两 patch.gz 与 HEAD 字节一致（`9e858daf…`、`0de0b34c…`）；PEM/AKIA/sk-/ghp_/xox 等 0 命中 | 0 | 终端实录（本表） |

隔离声明：全部 vitest/重放均在隔离 HOME/TMPDIR/LINGXI_HOME 或 /tmp 副本进行；真实仓库 `patch.gz` 未受影响；未读真实用户数据、未调用真实供应商/付费 API、零真实外发。

## 6. 文件清单与候选坐标

**修改（8 个已跟踪文件）**：`docs/rust-tauri/R00/` 下 `R00_HANDOFF.json`（`c925e839…6938`）、`ACCEPTANCE_MAP.json`（`96f70219…62be`）、`BLOCKERS.md`（`fd8cfeb9…841e`）、`R00-T07_RESULTS.json`（`bc0bfb3b…90b3`）、`R00_REPORT.md`（`b466b39c…9094`，§17 增补）、`R00_EVIDENCE_SUMMARY.md`（`089644a1…5ae`）、`r00_t07_validate_ledger.py`（`665f39a0…eace`）、`r00_t07_selftest.py`（`33a4244f…e789`）。`r00_t07_build_map.py` 未改（`5b8b94b5…225b`）。

**新增**：`artifacts/rust-tauri/R00/STAGE_REPAIR_R1/`（证据目录，含 `STAGE_REPAIR_MANIFEST.txt`）+ 本报告。

**候选清单**：`artifacts/rust-tauri/R00/STAGE_REPAIR_R1/STAGE_REPAIR_MANIFEST.txt`，**109 条目、逐条对盘核验 109/109 一致（exit 0）**；聚合 SHA-256（= 清单文件自身哈希，自引用规避）= **`4f8a64e585dc35cd68a4242fade90a2aaf3c4ad83aa0d49311db628641964b70`**。清单按声明排除：本报告自身、清单自身与其构建日志、独立验收报告 `R00_STAGE_REVIEW_R1.md`（验收方文件，保持未改）、总控账本 `ORCHESTRATOR_PROGRESS.json`（任务前已存在的未提交修改，未触碰）。

**当前候选坐标**：HEAD `8b153b1031bbb01204375b08e9caaa891397d7a5`（分支 `codex/rust-tauri-migration`）+ 上述工作区未提交改动。**工作树状态**（`git status --porcelain` 实录）：上述 8 个 `M` + 2 个 `??`（证据目录、独立验收报告），外加总控账本的预存 `M`；无其他改动；stage base 与 T08 任务基点历史坐标全部保留未回改。

## 7. 未通过 / 未验证事项（如实声明）

1. **审计封印家族 4 用例仍为 FAIL（保留，未修绿）**：post-verification-audit-seal、round2 R10-03/R10-04、round3 manifest；相对 T08 预存、相对 stage base 本阶段触发（§3）；收口归总控 seal 工作流，不归 R01。**全量测试不得表述为 PASS。**
2. **本轮未重跑全量 `npm test`**：本轮改动不含任何 npm test 输入（无生产代码/测试/锁定文件/构建配置变更，改动面 = R00 文档 + 账本 + 账本工具链，已用 `git diff --name-only` 域过滤核实为空）；同一 HEAD `8b153b103` 的两次隔离全量证据见阶段验收 R1 §4（4F，即上述 4 用例），本修复不冒充新跑。`npm test` 会重写 patch.gz 的副作用本轮未触发（两个 patch.gz 与 HEAD 字节一致）。
3. **本轮未重跑 typecheck / lint / boundaries / knowledge-smoke / renderer build**：同理由（输入未变）；历史结果以 T08 封存证据为准。
4. **T02/T03 检查器维持预存非绿**（exit 1，内容零漂移/设计性断言，诊断见 `T08/t02-t03-checker-diagnosis.md`）；T04 exit 0。
5. **其他平台（Windows/Linux）、真实供应商 LIVE、长时运行 G1** 维持 R00 原阻塞未验证，未声明通过。
6. R00_STAGE_REVIEW_R1.md 要求的「不自行宣布阶段 PASS」已遵守：本报告不含阶段 PASS 结论。

## 8. 回退

本修复全部改动可逆：8 个修改文件均可 `git restore` 恢复至 HEAD（`8b153b103`）字节；新增证据目录与本报告可整体删除（全部为本任务新建，无数据丢失风险）。账本可由 `r00_t07_build_map.py` 从任务书与盘点确定性重建（本轮已实测）。不回退总控账本与独立验收报告（非本任务产物）。

## 9. 移交总控

- 状态：**READY_FOR_STAGE_REREVIEW**。请创建**全新 Codex 阶段级独立复验子代理**（与验收 R1、本修复执行者均隔离），基点 = HEAD `8b153b103` + §6 工作区改动。
- 建议复验最小动作：①复跑 §5 表第 5/6/7 行命令对日志；②核对 `ledger-semantic-diff-r1.log` 的 19 处归因与零漂移断言；③按 HANDOFF spot_checks 另选样本复核；④核对 STAGE_REPAIR_MANIFEST.txt 聚合哈希与 109/109 对盘一致；⑤确认四个封印 FAIL 表述与两基点分类符合事实。
- 阶段复验 PASS 后：提交推送与正式封印推进（含 VERIFIED_SOURCE_SHA 重绑）由总控按 PROGRESS.md seal 工作流执行，证据须绑定实际候选提交。
