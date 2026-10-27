# R03 修复轮 G07 独立对抗性审查报告（G07-R1，第 1 轮）

- Reviewer：REVIEWER-REPAIR-R03-G07-R1（一次性独立代理；未参与 G07 候选的实现/修复/自查，未参与 G01–G06）。
- 日期：2026-09-30。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`，候选 = HEAD `8a6303bcd477cda891d64fddd6e96fa35017f7ef` + 未提交工作树（审查期间候选冻结，本审查零改动产品/测试/配置/门禁/账本/执行者证据）。
- 工具链：一律 `~/.cargo/bin/cargo`（rustup 1.98.1，`rust-toolchain.toml` 锁定；本机 PATH 中 Homebrew 1.93 优先，已显式前置 `~/.cargo/bin`），全部 `--locked`、`CARGO_NET_OFFLINE=true`。`rust/Cargo.lock` sha1 `3b659f41eb262eaf42efc2f91ec93f9989d42934` 与 HEAD 相同（实测）。
- 我的复测产物根：`artifacts/rust-tauri/R03/repair-current/G07-R1/`（本报告引用的全部命令退出码均为原始值）。

## VERDICT: PASS

G07（F08，R03-FIX-F08-C01..C04）四项 C-ID 全部通过独立核查与独立复测。两条 LOW 级 finding（文档引用笔误、复跑便利脚本的相对路径脆弱性）不影响任何门禁断言或证据真实性，不阻塞；留总控酌情处理。

## 候选摘要

候选把 G01–G06 的 9 个新修复套件（59 用例）经 `repair_suites` 门禁命令 + `R03-RP01` 场景接入 `verify-stage R03` 正式验收：R03.json/生成器同步（+22/-1，纯追加）、xtask 5 个钉图测试、`runner_tests.rs` 场景集钉 16+RP01、生产者脚本（逐套件精确计数）、门禁负向测试脚本（7 案例 /tmp 隔离副本）、现行报告/交接/账本更正（全部追加式，历史原文保留）、轮次交付五件套。不触碰产品源码（`rust/crates/lingxi-service` 在 G07 改动清单外，git status 证实）、不改 R02.json、Cargo.lock/package-lock 零改动、无 commit/push。

## 逐 C-ID 核查与独立复测

### R03-FIX-F08-C01 逐问题的红绿回归可追溯 — PASS

- **矩阵完整性**：`R03_FIX_ACCEPTANCE_RESULTS.json` fIdMatrix 覆盖 F01–F07 全部 30 个非 F08 C-ID 的承载面（9 套件 59 用例逐名列出；清单 JSON 全量 34 C-ID = F01×5/F02×5/F03×4/F04×4/F05×5/F06×3/F07×4/F08×4）。**全部 59 个测试名经正则比对真实存在于对应测试文件且 `#[test]` 属性计数==pinnedCount（9/9 套件全 OK，非仅抽查）**。抽查 ≥8 项 C-ID→用例映射（每 F-ID ≥1）：F01-C01→`linked_run_roots_receive_parent_cancellation_across_mixed_entries`、F01-C03→`registration_racing_cancellation_misses_no_node`、F02-C01→`parent_cancel_closes_children_in_process_repeatedly_beyond_the_cap`、F02-C04→`drain_expired_child_is_finally_reaped_after_its_actual_exit`、F03-C01→`cancel_accepted_while_final_events_persist_beats_the_completed_terminal`、F03-C02→`cancel_racing_the_finalize_transaction_is_too_late_not_accepted`、F04-C01→`f04_c01_panic_after_side_effect_journals_unknown_not_confirmed_failure`、F05-C01→`r03_f05_c01_background_spawn_rejection_leaves_no_ghost_replay`、F06-C01→`c01_foreground_boundary_lengths_deliver_the_full_input`、F07-C01→`c01_background_accepted_steering_reaches_next_turn_exactly_once`（各组 C-ID→用例表在 G0x-E01_NORMAL_SELFCHECK.md，逐节核实）。
- **红基线**：9 条 red-baseline 证据路径全部存在，内容与声称失败形态一致（F01 1/6、F02 2/5、F03 7/6、F04 0/6、F05 1/4、F06 0/5+E0425 编译错、F07 1/6 与 1/7——逐条 grep 复核 `test result:` 行/编译错误计数）。
- **三层状态**：fIdMatrix threeLayer（PASS_EXECUTOR×2 + PASS_R1）与 `R03_FIX_ISSUES.json` 逐项一致；G01-R1..G06-R1 六份审查文件全部存在且 `VERDICT: PASS`；INDEX.md 引用文件全部真实。
- **门禁接入真实性**：`git show 8a6303bcd:rust/crates/xtask/src/stage_maps/R03.json` 与当前图结构化比对——16 个原 A-ID 场景对象**逐字段深度相等**、14 个原命令定义零改动（仅新增 repair_suites）、48 叶完全相等（17 stage_share_satisfied + 31 deferred_to_later_stage）、7 条 R02 定向链命令存在且定义不变且被场景/叶引用；生成器在隔离 worktree 重跑**字节等价**（diff exit 0）；`stage_map.rs` 5 个钉图测试真实（16 场景 commandRefs 冻结表/RP01+命令注册/7 链注册+被引用/48 叶=17+31 且份额叶必带 assertionContract/生产者钉表集合相等——钉表测试直接读生产者脚本文件比对）。
- **独立复测**：生产者脚本独立重跑 `bash scripts/rust-tauri/r03_g07_repair_suites.sh <G07-R1/repair-suites-standalone>` exit **0**，9 套件 expect==actual（7/8/13/6/5/5/5/2/8）全 ok（`G07-R1/repair-suites-standalone/repair-cases.json`）；完整门禁内 repair_suites 复跑见 C03。

### R03-FIX-F08-C02 漏项和空测试不能通过 — PASS

- **脚本审查**：生产者对每套件检查 running==0（0 匹配过滤器）/running!=pinned（子集/改名）/passed!=pinned/失败数>0/无可解析摘要，任一命中即 gaps.txt 点名并 exit 1；证据根非空即拒（新鲜度）；强制 `rustup run 1.98.1 cargo`（不受 PATH 里 Homebrew cargo 干扰）。
- **独立重跑（全新证据）**：`bash scripts/rust-tauri/r03_g07_gate_negative_tests.sh /Users/…/G07-R1/negative-tests` → 驱动器 exit **0**，`case-results.json` `allRefused=true`，7/7 案例非零退出且点名（与执行者归档退出码完全一致）：
  - N1a 删叶映射 exit **1**（result JSON 点名 `drops 1 REQUIRED_SUPPLEMENTAL leaf scenario(s) [R00-T02-LA-00ECC9568490]`）
  - N1b 删命令映射 exit **2**（stderr：`scenario "R03-RP01" references unknown command "repair_suites"`）
  - N4 删 RP01 场景 → 钉图测试 exit **101**（断言消息点名 R03-RP01；该测试在门禁 rust_test_workspace 命令内，分层防护如实登记成立）
  - N2 0 匹配过滤器 exit **1**（我方重跑 gaps.txt 9 条 `filter matched 0 tests (running=0, pinned=N)` 逐套件点名；完整门禁 overall FAIL、repair_suites FAIL、R03-RP01 FAIL）
  - N3 缺证据文件 exit **1**（repair_suites 命令自身 exit 0，`missingEvidence:["{EVIDENCE}/G07_REPAIR/missing-evidence-demo.json"]`，overall FAIL）
  - C03a 陈旧证据根 exit **1**（`is not empty; preserving its prior evidence`，零命令执行）
  - C03b 中途改输入 exit **1**（stable=false、reason 点名、finalChangedPathBytesHex 解码=`rust/crates/lingxi-service/src/lib.rs`、before/after digest 不等）
- **N2 披露专项复核（按派单）**：归档首跑证据（`G07-E01/negative-tests/n2-zero-match-filter/`）真实显示**门禁本体首跑即正确拒绝**——exit-code.txt=1、result JSON overall FAIL、repair_suites FAIL、R03-RP01 FAIL；点名文本 `filter matched 0 tests` 当时确实只存在于生产者证据文件（gaps.txt/summary.txt/repair_suites 命令日志），gate.stdout/stderr 与 result JSON 中**逐文件 grep 证实为 absent**——即原 haystack（仅门禁日志+result JSON）确实会漏报，驱动器断言首跑过窄的披露属实。修正仅拓宽驱动器 grep 的搜索面；执行者归档的 pristine 副本与当前生产门禁文件**逐字节相等**（diff 证实 R03.json 与生产者脚本未因修正改动），门禁本体零放宽；n2 行按首跑归档证据事后复评、证据 mtime（~11:04）早于 case-results.json（11:19）与「未重跑」声明一致。我方全新重跑（haystack 修正后形态）N2 独立复现为非零退出+点名。
- 附注（非缺陷）：N2 的 /tmp 副本门禁中 `r02_legacy_regression` 亦红（执行者归档首跑同样如此）——为副本环境工件，与 N2 判定无关（该案例只要求 N2 缺口被点名+非零退出，均真实成立）。

### R03-FIX-F08-C03 候选与三层证据绑定 — PASS

- 执行者主门禁结果 JSON `candidateSourceBinding` 全字段核实：before/after digest 同值（a912f051…，fileCount 14079）、15 个逐命令 checkpoint 全 stable、testedShaAtEnd=8a6303bcd、excluded 策略仅排除本次 --evidence 子树（路径 hex 可解码验证）、runnerSourceBinding=PASS（编译内嵌源==磁盘字节）。
- c03a/c03b 受控演示记录真实（执行者证据已核 + 我方独立重跑双双复现，见 C02 表）。
- **我的干净复跑（全新证据根 `G07-R1/verify-stage-r03/`）**：
  `~/.cargo/bin/cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R03 --evidence /Users/…/G07-R1/verify-stage-r03` → exit **0**，**overall PASS**：15/15 命令 exit 0（rust_test_workspace、a15 矩阵、7 条 R02 定向链、a16 seed、fmt/clippy、check-contracts/check-boundaries、repair_suites 14.3s、r02_events_matrix）、17/17 场景 PASS（16 A-ID+R03-RP01）、48 叶=17 PASS+31 DEFERRED_TO_LATER_STAGE+0 fail+0 blocked、binding **stable=true**（before==after、15 checkpoint 全 stable、finalChangedPathBytesHex 空）、testedShaAtEnd=8a6303bcd、toolchainChannel 1.98.1、missingEvidence/preExistingEvidence 全空；workspace 72 suites/709 passed/0 failed（stdout.log 正则统计）。runner 日志 `G07-R1/verify-stage-r03-runner-clean.log`。
- **fmt/clippy 真实独立复跑**：`cargo fmt --all -- --check` exit **0**（零输出）；`cargo clippy --workspace --all-targets --locked -- -D warnings` exit **0**（零警告；1m26s）。证据 `G07-R1/standalone-fmt-clippy/`。
- 过程注记（对门禁有利）：我第一次门禁复跑因与负向测试脚本**并发**向仓内 `G07-R1/negative-tests/` 写证据，触发 binding `stable=false` 强制 FAIL（result JSON 点名我方写入的每个文件）——这是 C03b 语义在真实主仓的现场再证（runner 日志 `G07-R1/verify-stage-r03-runner.log` 保留 exit 1）；排除并发写入后干净复跑即 PASS。非候选缺陷。

### R03-FIX-F08-C04 重新接受范围正确且可交接 — PASS（材料就绪；阶段终审不归本审查）

- **报告更正与历史保留**：`git diff 8a6303bcd` 证实 R03_REPORT.md 仅 +8 行（「修复轮进度」小节+FINDING-1 行内追加 G07 关闭注记，原文一字未删）；2026-09-29「阶段独立验收 STAGE_VERDICT: PASS」节逐字保留；REOPENED 表述与实际状态一致（当前仍 REOPENED_PENDING_REPAIR，执行者不自称阶段 PASS）。
- **HANDOFF**：status 保持 REOPENED_PENDING_REPAIR+G07 追记注；FINDING-1 关闭注引用提交 `520bb75b9`（git show 证实该提交真实存在、含 F01+F02 修复与两套件、`origin/codex/rust-tauri-migration` 包含）；interfaces 更新逐符号对源码核实：`FireOutcome::TooLate`（cancel.rs:512/633）、`SessionExecuteError::AdmissionInFlight`（sessions.rs:190）/`RequestIdBoundToEarlierRun`（:202）/`InputTooLarge`（:230）、`recorded_input` 无 `chars().take(2000)` 残留、background.rs 经 `lease.steering_inbox()` 同源消费——与 G02–G06 交付语义一致；T08 原 `verify_stage_r02_full_gate` 记录保留，新增 g07_rerun 键不覆盖。
- **LEDGER**：diff 仅追加 repair_rounds 一条 + 新增 repair_scenarios 数组（R03-RP01）；**原 scenarios 数组 16 条未动**（实测仍为 A01..A16）；repair_scenarios 的 command_results/exit/evidence_root 与真实产物一致。
- **五件套齐备**：R03_FIX_NORMAL_SELFCHECK.md / R03_FIX_ADVERSARIAL_SELFCHECK.md / R03_FIX_ACCEPTANCE_RESULTS.json / R03_FIX_HANDOFF.md / R03_FIX_INDEPENDENT_REVIEWS/INDEX.md 全部存在且引用的文件全部真实。
- **R02 回归归因复核（派单第 5 项）**：执行者全量 `verify-stage R02`（exit 1，overall FAIL）＝19/20 命令绿、15/16 场景、34 叶=25 pass+9 deferred、binding stable 20 checkpoint。唯一红 a16_legacy_regression 归因核实：① 3 个 SEAL_FAMILY 文件与 `r02_t08_legacy_entry_regression.sh` 注册的 SEAL_FAMILY 集合**逐成员相等**，e5 日志实证坐标滞后句式与 round3 `patch replay failed: error: patch too large`，与 `R03_HANDOFF.stage_governance_deferrals` 的 GOV-01/02 登记一致（既有治理递延，本轮零触碰）；② 非族红 `tests/artifact-core-ustar.test.ts`＝`refuses to pack a symlink` 用例 afterEach `fs.rmSync` ENOTEMPTY（tests/artifact-core-ustar.test.ts:25:8，测试断言本体未失败）；`git log cd3fb19e6..8a6303bcd -- tests/artifact-core-ustar.test.ts shared/artifact-core` **为空**（零触碰证实）；**我方隔离重跑** `npx vitest run tests/artifact-core-ustar.test.ts` exit **0**，10/10 绿（`G07-R1/ustar-isolated-rerun.log/.exit.txt`）——环境闪红归因成立，且分类器 fail-closed 未消红、未套旧 seal 类别，处置诚实。③ a07/a14 现绿（result JSON exit 0），与 FINDING-2/3 收口表述一致。7 条定向链在本轮 verify-stage R03 门禁内全绿（我的干净复跑 exit 0 证实）。
- **范围红线**：不提前 R04（无 R04 工件）；正式签名/真实供应商/完整 R07 UI 未被设为本轮前置（HANDOFF 明示）；`R03_FIX_FINAL_STAGE_REVIEW.md` 不存在（正确——阶段终审归全新 STAGE-REVIEWER，本审查不越权代作）。

## finding 清单

| # | 级别 | 内容 | 影响 |
|---|---|---|---|
| G07-R1-F01 | LOW（文档引用笔误） | `R03_FIX_NORMAL_SELFCHECK.md` §2 C03 把主门禁 run 的 before/after digest 引为 `7be6cdc5…→同值`；该 digest 实为 **c03b /tmp 隔离副本 run** 的 before 值（其 after 本就不等——那正是演示点），主门禁 run 的 digest 是 `a912f051…`（两处均 stable/同值，证据本身无错，仅引用串了 run） | 无门禁影响；建议总控在后续文档触碰时更正该引用 |
| G07-R1-F02 | LOW（复跑便利脚本健壮性） | `r03_g07_gate_negative_tests.sh` 以**相对路径** EVIDENCE_DIR 调用时，gate_run 子壳先 `cd /tmp 副本`再重定向相对 `$dir/gate.stdout.log` → "No such file or directory"，7 案例全部假 BAD（退出码仍非零，无假绿风险）。默认/绝对路径调用不受影响（执行者与我方复跑均用绝对路径）。机制复现：`G07-R1/driver-relative-path-note/repro.log` | 不影响门禁本体与已归档证据；后续把 `$EV` 归一为绝对路径（如 `EV="$(cd "$(dirname …)" && pwd)/…"`）即可 |

无 MEDIUM/HIGH 级 finding。两条 LOW 均不构成对 C-ID 通过条件的削弱。

## 误判反证

无。执行者报告 §7「对审查结论的反证：无」与我的核查结果一致：F08 四项 source_facts 成立，旧接受裁决2 的替代依据（G01 产线同进程收口证伪「计数清理只在被跳过的 note_child_finished 中」前提）有 `520bb75b9` 真实提交+subagent_closeout 8 用例+G01-R1 独立审查支撑。

## 需标 STALE

无。执行者全部证据经独立复测仍然成立；无旧 PASS 被不当复用（c03a/c03b 演示与我的主仓并发写入现场均证实旧结果不可复用语义在工作）。

## 审查范围声明

- 只裁 G07 工作单（F08-C01..C04）。**阶段最终裁决（F08-C04 的 STAGE_VERDICT）不归本审查**——需总控另派全新阶段 Reviewer 独立实测七条主反例并复核原验收/递延/R02 回归；R03 在其 PASS 前保持 REOPENED_PENDING_REPAIR。
- 总控账本（R03_FIX_ISSUES.json/R03_FIX_COMMIT_RECEIPTS.json）不在审查范围（仅做了状态一致性核对）；G01–G06 组级审查结论引自其各自 G0x-R1 报告与账本登记，未重复执行已完成审查，但 G01–G06 的交付物在本轮门禁内被我方复跑机器核验（9 套件精确计数+workspace 72/709/0）。
- 本审查未修改任何产品/测试/配置/门禁/账本/执行者证据；复测产物只写 `artifacts/rust-tauri/R03/repair-current/G07-R1/` 与本报告；未 commit/push。
- 本地结果不代替其他平台/正式打包/真实供应商验证（派单边界）。

## 附：本审查独立复测命令与退出码汇总

| 命令 | 退出码 | 证据 |
|---|---|---|
| `bash scripts/rust-tauri/r03_g07_gate_negative_tests.sh <G07-R1/negative-tests 绝对路径>` | 0（7/7 案例内部非零+点名） | `G07-R1/negative-tests/case-results.json`（allRefused=true） |
| `~/.cargo/bin/cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R03 --evidence <G07-R1/verify-stage-r03 绝对路径>` | **0（overall PASS）** | `G07-R1/verify-stage-r03/verify-stage-result.json`、`verify-stage-r03-runner-clean.log` |
| `bash scripts/rust-tauri/r03_g07_repair_suites.sh <G07-R1/repair-suites-standalone 绝对路径>` | 0（9 套件精确计数全 ok） | `G07-R1/repair-suites-standalone/repair-cases.json` |
| `~/.cargo/bin/cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 0（零输出） | `G07-R1/standalone-fmt-clippy/fmt.{log,exit}` |
| `~/.cargo/bin/cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | 0（零警告） | `G07-R1/standalone-fmt-clippy/clippy.{log,exit}` |
| `npx vitest run tests/artifact-core-ustar.test.ts` | 0（10/10 绿） | `G07-R1/ustar-isolated-rerun.{log,exit}` |
| 生成器字节等价复现（隔离 worktree） | diff exit 0 | /tmp 临时 worktree（已清理），结果记录于本报告 |
| 首次门禁并发污染复跑（我方环境注记） | 1（stable=false 强制 FAIL，15/15 命令仍绿） | `G07-R1/verify-stage-r03-runner.log` |
