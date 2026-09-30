# R03 修复轮 G07-E01 执行报告（F08：验收接受缺口——新增反例进入正式验收、纠正旧接受裁决）

- 执行代理：EXECUTOR-REPAIR-R03-G07-E01（一次性执行代理；本报告为执行者口径，**不自称阶段 PASS**——阶段终审归总控另派的全新阶段 Reviewer，F08-C04）。
- 日期：2026-09-30。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。
- 基线 `FIX_BASE_SHA=cd3fb19e651f763afc6c75cb3163064fb54ca3fe`；候选起点 `CANDIDATE=8a6303bcd477cda891d64fddd6e96fa35017f7ef`（含已通过独立审查的 G01–G06 修复——本轮**未回退、未破坏**：9 个修复套件 59 用例在本轮门禁内逐套件机器核验全绿，见 §3）。本轮无 commit/push（未获授权）；总控账本（`R03_FIX_ISSUES.json`/`R03_FIX_COMMIT_RECEIPTS.json`）G01–G06 已登记内容零改动。
- 工具链：`~/.cargo/bin/cargo`（rustup 锁定 1.98.1），全部 `--locked` 离线；`rust/Cargo.lock` sha1 `3b659f41eb262eaf42efc2f91ec93f9989d42934` 与 HEAD 相同（零依赖变化）；`package-lock.json` 零改动。
- 证据根：`artifacts/rust-tauri/R03/repair-current/G07-E01/`。
- 结论：**READY_FOR_REVIEW**（verify-stage R03 overall PASS：15/15 命令 exit 0、17/17 场景、48 叶 17 pass+31 deferred、candidateSourceBinding stable；workspace 72 suites/709/0 ≥ 底线 72/704/0；fmt/clippy 零输出零告警；R02 回归 19/20 唯一红逐条归因；门禁负向测试 7/7 非零退出点名缺口）。

## 1. 实现范围

F08 本体（旧接受裁决2 把「普通父取消后 child active/busy 直到重启」解释为可递延 MINOR，容许未完成取消收口）及最终组合验收准备：把 F01–F07 的 9 个新测试套件接入 `verify-stage R03` 正式门禁、建立门禁负向测试、完整门禁真实运行、受影响 R02 回归、候选绑定验证、现行报告更正、轮次交付物。不修 G01–G06 已审查的产品语义，不改 R02.json，不提前 R04+。

## 2. 改动/新增文件

| 文件 | 改动 |
|---|---|
| `rust/crates/xtask/src/stage_maps/R03.json` | +`repair_suites` 命令（argv 指向生产者脚本，evidencePaths=repair-cases.json+summary.txt，timeout 2400s）+`R03-RP01` 场景（commandRefs=[repair_suites, rust_test_workspace]，REQUIRED，含说明 note）；16 原场景/48 叶/7 条 R02 定向链注册**零改动**（diff 仅 +22/-1：图注追加+新命令+新场景） |
| `scripts/rust-tauri/r03_t08_generate_stage_map.py` | 同步生成上述注册与图注（重跑生成器字节等价，图保持可复现） |
| `rust/crates/xtask/src/stage_map.rs` | +5 个钉图测试：16 场景 commandRefs 冻结表逐字段断言、修复场景+命令注册断言、7 条 R02 链（注册+被场景/叶引用）、48 叶=17 份额（必带契约）+31 递延、生产者钉表 9 组 (suite,count,F-ID) 集合相等 |
| `rust/crates/xtask/src/verify/runner_tests.rs` | 原 16 场景 id 集钉更新为 16+R03-RP01（保持 EXACT 集合语义；注册完整性断言原样） |
| `scripts/rust-tauri/r03_g07_repair_suites.sh`（新） | 门禁生产者：9 套件真实 `cargo test -p lingxi-service --test <suite>`，逐套件**精确计数钉**（running==passed==pinned；匹配 0/子集/改名/失败/无解析摘要=点名缺口 exit 1）；产出 `lingxi.r03-repair-suite-results.v1` |
| `scripts/rust-tauri/r03_g07_gate_negative_tests.sh`（新） | 门禁负向测试（可重跑）：/tmp 隔离副本 7 案例（详见 §5） |
| `docs/rust-tauri/R03/R03_REPORT.md` | REOPENED 节追加「修复轮进度」小节（G01–G06 结果+门禁重验+**替代裁决2 依据**）+FINDING-1 历史条目追加 G07 关闭注记；**2026-09-29 STAGE PASS 与全部历史原文保留** |
| `docs/rust-tauri/R03/R03_HANDOFF.json` | status 保持 REOPENED_PENDING_REPAIR +G07 追记注；known_gaps FINDING-1 标注已由 G01 产线修复关闭（提交 520bb75b9，closure_evidence）；interfaces 更新：CancelRunOutcome::TooLate/FireOutcome、SessionExecuteError::AdmissionInFlight/RequestIdBoundToEarlierRun/InputTooLarge、input_fidelity（载荷/摘要分离）、后台 steering 同源消费、xtask 门禁机制（repair_suites/RP01）；verify_stage_r02_full_gate_g07_rerun（T08 原记录保留） |
| `docs/rust-tauri/R03/R03_ACCEPTANCE_LEDGER.json` | repair_rounds 追加 adversarial-repair-2026-09-30-G01-G07 登记；新增 repair_scenarios 数组登记 R03-RP01（**原 16 场景条目未动**） |
| `docs/rust-tauri/R03/repair-current/`（新 5 件+本报告） | R03_FIX_NORMAL_SELFCHECK.md、R03_FIX_ADVERSARIAL_SELFCHECK.md、R03_FIX_ACCEPTANCE_RESULTS.json（F-ID→测试→红绿证据→三层审查矩阵）、R03_FIX_HANDOFF.md、R03_FIX_INDEPENDENT_REVIEWS/INDEX.md、G07-E01_REPORT.md |

## 3. 完整门禁真实运行（全新证据根 `G07-E01/`，真实退出码）

独立实跑（`gate-commands/`）：fmt exit 0（零 diff）；clippy `-D warnings` exit 0（零告警）；`cargo test --workspace --locked` exit 0（**72 suites / 709 passed / 0 failed**，较 G06 底线 72/704/0 +5=xtask 钉图测试）；check-contracts exit 0（零漂移）；check-boundaries exit 0。

`verify-stage R03`（`verify-stage-r03/verify-stage-result.json`）：**overall PASS，exit 0**——15/15 命令 exit 0（rust_test_workspace 188s、a15 矩阵 17s、7 条 R02 定向链、a16 seed、fmt/clippy/checks、**repair_suites 20.8s**、r02_events_matrix）；17/17 场景 PASS（16 原 A-ID+**R03-RP01**）；48 叶=17 份额 PASS+31 递延+0 fail+0 blocked（R00 双账本相等性检查通过）；candidateSourceBinding **stable=true**（15 个逐命令 checkpoint 全稳定、before/after digest 同值、testedSha=8a6303bcd4、runnerSourceBinding PASS）。repair_suites 证据：`G07_REPAIR/repair-cases.json` 9 套件 expect==actual（7/8/13/6/5/5/5/2/8）全 ok。

## 4. 受影响 R02 回归

- 定向（派单第 4 项）：认证/作用域（r02_auth_matrix）、单写者与事务（r02_storage_tx）、事件快照续读（r02_events_matrix）、备份和损坏库（r02_backup_restore/r02_recovery_drill）、A15 真实重启链（r02_full_chain）、A16 默认入口（r02_legacy_regression directed E0–E4.5）——**7 条定向命令在 verify-stage R03 内全绿**。
- 全量 `verify-stage R02`（`verify-stage-r02-regression/`，exit 1，overall FAIL）：**19/20 命令绿**、15/16 场景、34 叶=25 pass+9 deferred+0 fail+0 blocked、binding stable（20 checkpoints）。唯一红 a16_legacy_regression 逐条归因：
  1. 3 个注册 SEAL_FAMILY 文件（post-verification-audit-seal/round2/round3-delivery-evidence）＝R03-GOV-01 坐标滞后既有治理递延；round3 呈 `patch replay failed: error: patch too large`＝R03-GOV-02（git MAX_APPLY_SIZE 1023MiB 硬限 vs ~1.9GB 增量补丁）既有治理递延，本轮复现在案；
  2. **1 条非族红** `tests/artifact-core-ustar.test.ts`：`afterEach fs.rmSync` ENOTEMPTY 临时目录清理竞态（测试断言本体未失败）；该文件与 `shared/artifact-core` 在修复轮 cd3fb19e6..8a6303bcd **零触碰**（git log 空）；隔离重跑 `npx vitest run tests/artifact-core-ustar.test.ts` = **10/10 绿**（归因记录 `A16/legacy-entry/g07-ustar-isolated-rerun-attribution.txt`）；分类器按设计 fail-closed 不消红——如实登记为测试基建环境闪红，**不套用旧 seal 类别、不掩盖**，留阶段 Reviewer 复核。
  3. 旧 FINDING-2/3（a07_live_fault_02_check/a14_slow_subscriber）本轮实测**绿**（与阶段终审裁决3 的收口记录一致）。

## 5. C02 门禁负向测试 + C03 证据绑定（/tmp 隔离副本，真实退出码）

脚本 `scripts/rust-tauri/r03_g07_gate_negative_tests.sh`（可重跑；git worktree @HEAD+未提交门禁文件，生产门禁零改动），证据 `negative-tests/`：

| 案例 | 攻击 | 结果（exit / 点名） |
|---|---|---|
| N1a | 删一条 R00 补充叶映射 | exit 1；`drops 1 REQUIRED_SUPPLEMENTAL leaf scenario(s) [R00-T02-LA-00ECC9568490]`（命令未跑即中止） |
| N1b | 删 repair_suites 命令映射 | exit 2；`references unknown command "repair_suites"` |
| N4 | 删 R03-RP01 修复场景 | 钉图测试 exit 101；`R03 map dropped the G07/F08 repair scenario R03-RP01`（跑在门禁 rust_test_workspace 命令内——分层防护如实登记） |
| N2 | 匹配 0 的测试过滤器 | 生产者 exit 1 点名 `suite cancel_link_inheritance: filter matched 0 tests (running=0, pinned=7)` 等 9 条；完整 verify-stage overall FAIL exit 1 |
| N3 | 缺证据文件（命令 exit 0） | missingEvidence 点名 `{EVIDENCE}/G07_REPAIR/missing-evidence-demo.json`，overall FAIL exit 1 |
| C03a | 陈旧证据根 | 拒绝运行 `is not empty`，exit 1（零命令执行） |
| C03b | 运行中改执行输入 | `stable=false`+reason「Candidate file bytes or HEAD changed…stage PASS is forbidden」+finalChangedPathBytesHex 点名 `rust/crates/lingxi-service/src/lib.rs`，overall 强制 FAIL exit 1 |

7/7 全部非零退出且点名缺口（`case-results.json` allRefused=true）。如实登记：N2 首跑时**驱动器断言**的搜索面未含生产者证据文件（门禁本体首跑即正确拒绝）；haystack 已修正，N2 行按首跑归档证据事后复评为 OK（`driverHaystackFix` 注记），未重跑、未放宽任何门禁断言。

C03 绑定体现：主门禁结果 JSON 内 candidateSourceBinding（before/after digest、15 checkpoint、testedShaAtEnd、excluded 策略）+ C03a/C03b 受控演示。

## 6. C01 红绿矩阵（摘要）

`R03_FIX_ACCEPTANCE_RESULTS.json` `fIdMatrix`：F01→cancel_link_inheritance(7)；F02→subagent_closeout(8)；F03→cancel_terminal_race(13)；F04→tool_receipt_unknown(6)；F05→admission_dedup_consistency(5)+admission_dedup_adversarial(5)；F06→input_payload_fidelity(5)+input_budget_refusal(2)；F07→background_steering(8)。逐用例名全列；红基线=G0x-E01/adversarial-selfcheck/red-baseline-*.log（未修代码实测失败：1/6、2/5、7/6、0/6、1/4、0/5+编译错、1/7 passed/failed）；绿=本轮门禁 repair_suites 逐套件 actual==expect；三层=G0x 普通自查/对抗性自查 PASS_EXECUTOR + G0x-R1 独立审查 PASS（状态引自总控账本，未重复执行已完成审查）。

## 7. 对审查结论的反证

无。F08 四项 source_facts 逐条成立：历史 14/14、16/16、626 记录保留未否认；裁决2 的前提（计数清理只能在被跳过的 note_child_finished 中）已被 G01 产线修复证伪（同进程收口，subagent_closeout 8 用例）；旧绿确不覆盖新反例（本轮已接入）；替代依据已明示于 R03_REPORT.md（旧原文保留）。

## 8. 执行者口径声明

本轮所有命令均在本机以 `~/.cargo/bin/cargo`（1.98.1）`--locked` 执行；退出码为原始值；证据为原始日志/JSON。不构成独立审查；READY_FOR_REVIEW 仅为执行者自查结论；未 commit/push；阶段裁决（F08-C04）归全新阶段 Reviewer。
