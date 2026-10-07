# R05 RR3 M-REVIEW-01 — F54（M包）全新独立验收报告

- 审查者：R05 RR3 M-REVIEW-01 全新空历史独立审查者（未参与 M 实施及此前任何轮）；未派子代理。
- 任务书：docs/rust-tauri/R05/repair-current/RR3_M_REVIEW_BRIEF.md（全文亲读）；并全文亲读 RR1/RR2 MASTER、RR3_BRIEF/REVIEW_BRIEF、RR3_ISSUE_MATRIX（F54 行 OPEN 待本审）/PROGRESS/HANDOFF、FINAL-03/STAGE_REVIEW.md §四失败项2、M-01/REPORT.md 及其证据、三改动文件 git diff HEAD。
- 基线：分支 codex/rust-tauri-migration，HEAD=b3ac0e6aeae8d7d530a6ab8657a5fa9c4f0b；cargo=/Users/study_superior/.cargo/bin/cargo（1.98.1，--locked）；开工时 M-REVIEW-01/ 不存在（ls 确认）；开工 df Data 可用 522Gi。
- 红线：仅写 artifacts/rust-tauri/R05/RR3/M-REVIEW-01/（我的 standalone 运行期间除该证据根外对仓库零写入，全部审查中间物在 /tmp/m-review-01-work/，REVIEW.md/commands.jsonl 于 standalone 完成后落盘）；仓库其余只读；无 Git 写（HEAD/reflog/index 全程未动）；无系统变更；未派子代理；注入仅限 /tmp 隔离副本。

## 结论

| 项 | 结果 |
|---|---|
| F54（M包）验收 | **PASS，无 mustFix** |

六项亲验全部 PASS（细节见下）。两处 M-01/REPORT.md 的**非实质**报告措辞偏差如实记录（§七），不影响任何验收判据。

## 一、分类正确性抽查（PASS）

方法：不止抽查——先对全部 124 叶做全量结构验证，再对 12 叶（含最复杂 R00-T02-LA-B4BB2438E855 7断言与多个 2 断言简单叶）做逐断言语义抽样，并亲读生产者测试源码核对案例语义。

全量结构（python 独立复算，非消费 M 报告数字）：
1. 124 叶 ID 集与旧图完全一致；分类转移恰为 46 share→full、9 share→share（9 叶逐字节未动）、69 deferred→deferred（逐字节未动）。
2. 46 full 叶：每叶 originalAssertionCases 组数==r00Assertions 断言数、无空组、叶内案例不重复、contract.cases==归属并集；stageShare/laterShare 均为 ""（full 叶无份额拆分）；r00ExecutionStageIds 全部==["R04"]（独占）。
3. 全部 119 条钉住引用（41 个 distinct (case,expect)）：案例名全部存在于真实 56 案例集（artifacts/rust-tauri/R04/RR1-STAGE-R1/verify-R04/R04_MATRIX/leaf-cases.json，schema=lingxi.leaf-case-results.v1，producedBy=cargo test -p lingxi-service --test r04_tool_matrix，56 案例 actual==expect 全 ok=true；T08-E01/RR1-G05-E01 同构真值 SHA256 同为 c471fa08…），expect 值与生产者记录逐一相等，零不匹配；evidencePath 统一 {EVIDENCE}/R04_MATRIX/leaf-cases.json、producerCommand=r04_tool_matrix，无虚构路径。xtask 镜像 R04_MATRIX_CASES（56）与生产者集合精确相等，r04_production_map_pins_the_real_case_expectations 钉 expect 双重核对。
4. 9 个保留 share 叶：r00ExecutionStageIds 全部==["R04","R06"]（双阶段，R06 真实承接余款），pinned 案例全部真实且 expect 相等。
5. 旧图 46 叶原钉 50 条引用在新图全部保留（新 69 条=119-50）；M 报告写"原钉 47"为计数口径偏差（见 §七）。
6. R00 登记：全部 124 叶的 12 个镜像字段（id/featureId/requirement/r00Kind/r00TaskIds/r00ExecutionStageIds/r00LedgerStatus/r00ResultIds/r00TestIds/r00Then/r00Assertions/r00Due）与当前 R00 双台账（ACCEPTANCE_MAP.json+FEATURE_STAGE_ACCEPTANCE.json）逐字段相等（独立 python 复核，非仅 diff 旧图）。

12 叶逐断言语义抽样（B4BB2438E855、0818574ABD43、8A3C87812B4F、C88F29B5114A、2D896560381E、96DD1FF9E9D5、0199B843D759、CE69063550AA、DA5AD5C40098、CD1524CC7DC3、483E461BB59D、C90F42576683）：每条 R00 原断言所钉案例与断言语义一一对应（如 reject→approval-reject-zero-dispatch(expect=0)、冲突保留→tool-edit-conflict-preserves-user-version、目录诚实→future-tool-shape-*-discoverable-not-callable、越权拒绝→a15-out-of-grant-claim-refused）；亲读 r04_t08_tool_matrix.rs 的 record_case 实现核实案例真实语义（approval-reject 记录 dispatched 计数==0、a16-history-preserved 记录 disable 后历史收据保留、matrix-lifecycle-disable-holes 记录 6 路由零洞等）。future 工具族与两设置页叶（B4BB/DA5A）的原断言含 UI 产品行为，R04 钉住的是其消费的机制面真实案例——该映射在生成器内有逐断言 justification、无虚构；R00 台账将此类叶登记为 R04 独占（due 在 R04），分类为 full_original_behavior 且逐断言钉真实案例是与台账和 F25 规则一致的唯一诚实分类（改为 deferred 反而违背 R00 登记）。语义裁决：接受。

## 二、校验器/R00/pins/cids/R05叶表零改动（PASS）

git diff HEAD 逐文件核对（diff 行数为零）：
- rust/crates/xtask/src/verify.rs：0 行改动（F25 校验器规则 verify.rs:919-944 原样）。
- docs/rust-tauri/R00/ACCEPTANCE_MAP.json、FEATURE_STAGE_ACCEPTANCE.json：0 行改动（叠加 §一.6 的镜像==台账复核）。
- docs/rust-tauri/R05/ 四个 TSV（r05_stage_pins/r05_stage_cids/r05_required_cids/r05_leaf_case_map）：0 行改动。
- rust/crates/xtask/src/stage_maps/R05.json、R03.json、R02.json：0 行改动。
- rust/crates/xtask/src/stage_map.rs：69 行改动全部位于 #[cfg(test)] mod map_tests（R04_LEAF_COUNTS (55,69)→(46,9,69) + 三分计数断言 + F54 独占性镜像断言 + full 叶契约断言），无生产语义变化。
- M 包改动恰为三文件：stage_maps/R04.json（生成器整体重建）、scripts/rust-tauri/r04_t08_generate_stage_map.py（份额决策记录处+F54 不变量）、stage_map.rs（镜像测试）；无白名单外改动。
- 附：生成器在隔离副本重跑输出与主树 R04.json 逐字节相等（SHA256 93d5efda2410f1c5…），生成器自断言（FULL 46 全独占、SHARE 9 全双阶段、组数==断言数）运行通过——图无手改。

## 三、隔离副本红绿亲证（PASS）

隔离副本 /tmp/m-review-01-work/iso（rsync 全仓库减 .git/rust/target/artifacts 等；探针测试只写入隔离副本的 runner_tests.rs，主树零注入）；自建探针（与 M-01 的探针独立编写）：从磁盘路径加载 R04 图（换图不需重编译）+ 真实 RR1-STAGE-R1 leaf-cases.json + 全命令 exit0 outcomes，对 124 叶跑生产 roll_up_supplemental_leaf：

| 态 | 输入 | 结果 |
|---|---|---|
| 红（全图） | 旧图（git show HEAD 版） | **46 FAIL / 9 PASS / 69 DEFERRED**；46 条理由与 FINAL-03 verify-R05/R04_REGRESSION 的 46 FAIL **逐叶逐字节相同**（python 集合比对：same ids=True, reasons byte-identical=True） |
| 红（单叶） | 新图仅 R00-T02-LA-0199B843D759 还原旧分类 | **1 FAIL / 54 PASS / 69 DEFERRED**，被点名叶理由逐字同上 |
| 绿 | 新图 | **55 PASS / 0 FAIL / 69 DEFERRED / 0 BLOCKED**（pass=55=46 full+9 share） |
| 镜像测试 | 旧图嵌入（隔离副本重编译） | r04_production_map_keeps_the_124_leaf_split RED：left (0,55,69) vs right (46,9,69)；还原新图 GREEN |
| F25 校验器单测 | exclusive_leaf_classified_as_share_fails_with_unowned_remainder | GREEN（同一二进制上：独占 share 必 FAIL、独占 full+逐断言钉案例 PASS） |
| 全 map_tests | 新图 | 66 passed / 0 failed |

## 四、M-01 standalone 结果亲核（PASS）

attempt-2（verify-R04-standalone/verify-stage-result.json，亲读全部字段）：
- overall=**PASS**；testedSha=b3ac0e6a…+真实工作树（worktreeDirty=true, fileCount 70829）。
- candidateSourceBinding：before==after digest 33b49f3da08b5e79…，**8/8 checkpoint 全 stable、每 checkpoint changed=0**，finalChanged=[]；runnerSourceBinding=PASS。
- 8 命令全 exitCode=0（rust_test_workspace 872s、r04_tool_matrix 338s、fmt/clippy/contracts/boundaries、r03_regression_gate 1820s 嵌套 R03 层整体 PASS 且自身绑定 stable、r04_rr1_repair_suites 67s）。
- 叶表：expected 124 / pass 55 / **fail 0** / deferred 69 / blocked 0 / commandsNotPassing=[]；24 场景全 PASS（含 R04-SUP05）；run 内自产 R04_MATRIX 56 案例全 ok，全叶钉住 expect 与 run 内生产者记录零不匹配；shareSatisfiedLeafIds==图内 9 share 叶集合。
- 时长 55.38 分钟（17:40:40Z→18:36:03Z），与报告 55.4 一致。

attempt-1（verify-R04-standalone-attempt1-concurrent-writes/，并发干扰归因抽查）：
- 叶表同样 0 FAIL（pass 55/fail 0/deferred 69）、8 命令全 0、24 场景全 PASS；overall=FAIL 唯一原因 stable=false（before f4356587…→after c3f3877d…，8/8 checkpoint 不稳）。
- 我逐 checkpoint 解码 changedPathBytesHex：28 个 distinct 变化路径，24 个为 L-01/* 与 L-REVIEW-01/*；**另有 4 个非 L 路径**：artifacts/rust-tauri/R05/RR3/M-01/REPORT.md（M 会话自己在 gate 运行中续写报告）与 docs/rust-tauri/R05/repair-current/RR3_{ISSUE_MATRIX.json,L_REVIEW_BRIEF.md,PROGRESS.md}（总控台账）。**无一属于 M 包目标源文件（rust/crates/xtask/、scripts/rust-tauri/）——报告"无一属于本包目标源文件"属实**，但"全部是 L-01/L-REVIEW-01"措辞不精确（见 §七）。launch log 亲读确认 fmt 中途事件（前一次运行在首个 checkpoint 前被主动终止、证据目录删除、fmt 定稿后重启 attempt-1）。

## 五、独立 standalone 重跑（PASS）

命令（python start_new_session=True 完全脱离宿主会话，cwd=仓库根，HOME=/Users/study_superior）：
`/Users/study_superior/.cargo/bin/cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R04 --evidence artifacts/rust-tauri/R05/RR3/M-REVIEW-01/verify-R04-review`
2026-10-07T18:52:27Z 启动（pid 47435），运行期间本审查者是仓库唯一写者且除该证据根外零写入。结果（亲读 verify-R04-review/verify-stage-result.json）：

- **overall: PASS**；2026-10-07T18:52:28Z→19:48:41Z（56.21 分钟，pid 47435，start_new_session 脱离宿主）。
- testedSha=b3ac0e6aeae8d7d530a6ab8657a5fa9c4f0b+真实工作树（worktreeDirty=true，fileCount 71463）；testedShaAtEnd 同。
- candidateSourceBinding：before==after（digest 7cc54dd7923d39be…），**8/8 checkpoint 全 stable、累计 changed 路径 0**，finalChanged=[]——运行期间确无任何并发写（总控静默承诺兑现）。
- runnerSourceBinding=PASS。
- 8 命令全 exitCode=0：rust_test_workspace 872.9s、r04_tool_matrix 338.7s（run 内 56 案例全 ok，与图钉 expect 零不匹配）、rust_fmt 38.4s、rust_clippy 44.9s、check_contracts 38.1s、check_boundaries 38.1s、r03_regression_gate 1848.4s（嵌套 R03 层 overall=PASS、自身绑定 stable、runner PASS、叶表 17 pass/0 fail/31 deferred/0 blocked、17/17 场景 PASS）、r04_rr1_repair_suites 67.2s。
- **叶表 0 FAIL**：expected 124 / declared 124 / pass 55 / **fail 0** / deferredToLaterStage 69 / blocked 0 / commandsNotPassing=[]（55=46 修复 full+9 保留 share）；24/24 场景 PASS（含 R04-SUP05）；F53 终端 flake 未复现。
- 与 M-01 attempt-2 独立复现一致（55/0/69、8 命令、24 场景、嵌套 R03 全同；仅 digest/fileCount/时长按本轮事实）。

## 六、方法自控（PASS）

四组隔离篡改，全部被我的检查捕获：
1. 虚构案例名（m-review-fabricated-case 替换一钉住案例）→ 探针 FAIL"declared case … not recorded in leaf-cases.json"；我的全量结构检查同时报 UNKNOWN case。
2. 翻转一钉住 expect（1→0）→ 探针 FAIL"map pins expect 0 but evidence records … actual=1"。
3. 懒回退分类（full→share 但 stageShare 空）→ parse_stage_map 直接拒绝（"stageShare must be a non-empty string"）。
4. 伪装完整回退（full→share 且填份额文本）→ 探针 FAIL unowned remainder（F25 规则点名）。
另：旧图 46 FAIL 与 FINAL-03 逐字比对（§三）本身即篡改检测的反向验证——我的比对能发现任何理由文本差异。

## 七、非实质观察（不构成 mustFix）

1. M-01/REPORT.md §4c attempt-1 归因句"24–28 个变化文件全部是…L-01/* 与 L-REVIEW-01/*"不精确：28 个 distinct 路径中 4 个为 M-01/REPORT.md（M 会话自身续写）与 3 个总控 docs 台账；关键限定"无一属于本包目标源文件"经我核实为真，attempt-2（0 变化全 stable）不受影响。
2. M-01/REPORT.md §1/§3 计数"原钉 47 个全部保留"：实测旧图 46 叶原钉 50 条（distinct 29），全部保留；"69 个新案例引用"（119−50=69）属实。计数口径偏差，实质断言（原钉全保留、无虚构）为真。
3. 建议（非阻断）：后续报告写并发干扰归因时逐路径全列，不以"全部是 X"概括。

## 八、验收判据核对

- 成功判据（矩阵 F54 行）：R04 叶表 0 FAIL（M-01 attempt-2 + 本审独立重跑双证）；改回旧分类重现 FAIL（§三红两态）；fmt/clippy 绿（attempt-2 gate 内命令 exit0 + M-01 §4d；本审隔离副本全 map_tests 66 绿）。
- 范围红线全守：校验器/R00/pins/cids/R05 叶表零改动（§二）；未虚构断言/路径（§一）；生产 crate 零触碰（三文件全在白名单）；无 Git 写；注入仅隔离副本。
- F54 可关：PASS 无 mustFix。后续 FINAL-04 换全新阶段审查者。

## 九、边界声明

本审查只写 artifacts/rust-tauri/R05/RR3/M-REVIEW-01/（verify-R04-review 由命令本身创建；REVIEW.md/commands.jsonl）与仓库外 /tmp/m-review-01-work/（隔离副本与中间物，保留原样）；主树其余一切只读（standalone 运行期间除证据根外零写入，绑定 stable 亦为佐证）；无 Git 写；无系统/防火墙修改；未派子代理；无发布或外部消息。完成后停写。
