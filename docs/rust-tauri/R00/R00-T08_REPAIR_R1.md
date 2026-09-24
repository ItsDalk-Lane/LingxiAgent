# R00-T08 修复 R1（ZCode）｜REPAIR-R00-T08-R1 修复报告（含补充修复轮）

修复者：独立 ZCode 修复代理 REPAIR-R00-T08-R1（非 T08 执行者、非 R1 验收者；不派生代理、不建分支/worktree、不 commit/push/PR/tag/release、不接触真实用户数据/真实供应商/付费调用/外发）。
基点：分支 `codex/rust-tauri-migration`，HEAD = `e0b7be6108c4d5bc873061dee279b7163ca78a41`（两轮修复期间未变）。
输入：R1 独立验收报告 [R00-T08_REVIEW_R1.md](R00-T08_REVIEW_R1.md)（SHA-256 `8a832b5d6c4c0c05749c9f4c1befc8f3778f69cf6f972b4f44c89dda0a5e66e2`，两轮修复后复验字节未变）的两项 MUST_FIX（F1/F2）；第二轮补充指令（允许且仅允许经 `r00_t07_build_map.py` 获准重建路径更新验收账本三件套与必要生成器输入，重绑新 HANDOFF SHA；仍严禁编辑总控账本 ORCHESTRATOR_PROGRESS.json 与历史 R1 验收报告）。已先读 AGENTS.md、R00 阶段书 T08/A15/A16 规格、T08 报告 §8.4 获准更新路径、build_map/validate/selftest 三工具源码与全部相关交付。
时间：2026-09-24（本机 CST）。

## 0. 结论（先读）

两轮修复全部完成：**第一轮** F1（候选计数）+ F2（lint warning 口径）纯文档勘误；**第二轮（补充）** 按 T08 §8.4 获准路径（源变化→受影响验证→重建→自测→LEDGER_VALID）重建验收账本，将 RES-R00-A16 重绑到修复后 HANDOFF。最终态：**`r00_t07_validate_ledger.py` exit 0（LEDGER_VALID checks=14945）、候选清单 173/173 逐项 SHA 0 差异、`git diff --check` 通过、闭合复算零遗漏零多余**。**本报告不自评 PASS；交总控另开全新 ZCode 独立复验（R2）。READY_FOR_R2_REVIEW。**

最终候选：**173 文件 = 7 个已跟踪修改 + 166 个新文件**（清单内）；清单外另 3 文件（R00_REPORT.md、R00_EVIDENCE_SUMMARY.md、CANDIDATE_MANIFEST.txt 自身）→ T08 总新文件 169；artifacts/rust-tauri/R00/T08/ 物理 163 文件 = 清单内 162 + 清单自身。

## 1. 逐文件 SHA-256 演进（三轮状态）

| 文件 | 修复前（R1 验收时） | 第一轮勘误后 | 第二轮补充后（最终） |
|---|---|---|---|
| R00_REPORT.md | `916b8a308a3680726168c32fdd99e4a45e70754a3d0e8121f74395418b70dd87` | `9cd6c6a2a529b355a3df5770fa7a7c9196ab3dc40fe39cabeb278a8129e15fc6` | `724d71889776915b73e120ef208587d1e15ea6ec222030506feabac01a1b76cc` |
| R00_HANDOFF.json | `bde9eea92e6cffb9e7dfe0380d9ce0e901d0e9cfb18b2f4d3ba57794fabe2547` | `68877ff6980993e34e8fd5405b93eb0c20495b5fa242bcc41c782c06796c62a5` | 同左（**冻结**，账本 A16 已绑定此值） |
| CANDIDATE_MANIFEST.txt（聚合） | `14e399ef379679591bf4fa5040fc1bead8ee8d3725716f60a079f5aab403310a`（145 行） | `7dc6e4a02d2dc11df8d660bf0526e4300ddab3ff2c4ce64a1e1a36022db899e2`（145 行） | `d83ae896ce19d1ee5a8c0c8084244c44a32516cee5d589a35ead9fff2a448645`（**173 行**） |
| R00_EVIDENCE_SUMMARY.md | `8013781ac648b10d3e6774de9d9f248c951618ad9f6d417fd5cb71c152647274` | `cad8b708f812ea754118ced097f96e5ddac2fe4e97ef97e01e97f25f35b0847c` | `977cb757d867fce1747ac62e4d8dfbf48eff1b843987a427cd4e61984cd0665e` |
| ACCEPTANCE_MAP.json（账本） | `e0bd3cfa6074bcf66f746254476d5f4dde260f9d3fe3ce5565945463ca83eb52` | 未动 | `5eeac37ce3cb19e1eaaf123a8ce3b1f3cbd286538eda5a75fed7e4095eec288a`（重建） |
| BLOCKERS.md | `06dc5963b1be3a29e8953914cd6fcd283fc6956de6b5ddc78c3f783e97eaf7af` | 未动 | 未动（重建后经 cmp 字节一致） |
| R00-T07_RESULTS.json（生成器输入） | `4d50dd107a739b6f5c7139802ca7396b78c04341bee3e79bcb1922e70ce3a066` | 未动 | 未动（本轮零生成器输入改动） |
| r00_t07_build_map.py（生成器） | `5b8b94b53a10db1ca17a4da516323d6dd6641f65f67e3eaef3c9d7f1627b225b` | 未动 | 未动 |
| R00-T08_REVIEW_R1.md | `8a832b5d6c4c0c05749c9f4c1befc8f3778f69cf6f972b4f44c89dda0a5e66e2` | 未动 | 未动 |
| ORCHESTRATOR_PROGRESS.json（总控账本） | `d846894239074b25690d87c3c1aa49955e5738735a39eda12e51680f89df0ea8`（预先存在修改） | 未动 | 未动（终值复核相同，本会话零写入） |

## 2. 第一轮｜F1 修复明细（候选计数统一为真实值）

权威口径＝R1 独立核实的 §10 与清单实测。逐处改动（R00_REPORT.md）：

1. **§2**：原「T08 候选 = 140 文件（§10）」→ 145（第二轮后为 173，见 §7.6）。
2. **§4 概览表**：原「验证组合原始日志（…T08/，140 文件）」→「物理 135 文件 = §10 清单内 134 + 清单自身」（第二轮后为 163=162+1）。该处为与 §2/§13 同源的陈旧「140」第三处残留，按「统一真实计数／引用一致」一并勘正（与 R1 §2.2 实测一致）。
3. **§13 回退**：原「删除 133 个新文件」→「删除 141 个新文件 = §10 清单内 138 + 清单外 3（本报告、R00_EVIDENCE_SUMMARY.md、CANDIDATE_MANIFEST.txt 自身）」（第二轮后为 169=166+3），明确区分清单内/清单外，消除按旧文执行残留 8 文件的缺陷。

总控账本未动（其工作区 diff 仍为总控预先存在的 T07 DONE / T08 派发记录）。

## 3. 第一轮｜F2 修复明细（lint warning 口径）

依据＝lint-r2.log 原文「✖ 10887 problems (0 errors, 10887 warnings)」「0 errors and 33 warnings potentially fixable with the `--fix` option.」＋ R1 §3.5/§7 复跑双证。逐处改动：

1. **报告 §5 lint 行**：→「修复后 0 错误（10887 处预存 warning 原样保留，其中 33 处可 --fix 自动修复；修复前 exit 1 见 lint-r1.log）」。
2. **报告 §6 自发现缺陷 1 末句**：→「修复后 exit 0，0 错误 / 10887 处预存 warning 原样保留（其中 33 处可 --fix 自动修复；lint-r1/r2 双日志留证）」。
3. **HANDOFF verification_battery `npm run lint`.expected**：→「exit 0（T08 修复 docs/**.mjs node-globals 缺失后；0 错误 / 10887 处预存 warning 原样保留，其中 33 处可 --fix 自动修复）」。

## 4. 第一轮｜O2 顺带修正（派发授权，未扩大到代码或新验证）

依据＝npm-test-r1.log L11092–11093。报告 §5 `npm test` 行与 HANDOFF 同名 expected 写全 passed/failed/skipped（文件 1463 通过/3 失败/3 跳过共 1469；用例 14895 通过/4 失败/15 跳过全口径 14914），消除剔跳过分母 14899。SUMMARY §4 通过数口径准确未改；O1/O3 按 R1「无需动作」未动。

## 5. 第一轮｜聚合 SHA 重算方法

编辑 HANDOFF → `shasum -a 256` 得 `68877ff6…` → 精确替换清单 HANDOFF 行（两空格分隔、行序不变）→ 清单自 SHA 即新聚合 → 同步报告 §10（聚合块 + 单列）与摘要 §1。该轮聚合 `7dc6e4a0…`（145 行）为中间态，已被第二轮取代（见 §7.5）。

## 6. 第一轮｜验证（时点记录）

清单 145/145 逐行 SHA 对盘一致、聚合引用一致、HANDOFF JSON 可解析、陈旧引用清零、0 行尾空白/制表符、链接有效、git 范围＝会话起点 8 个 tracked 修改。该轮如实披露：账本校验器只读实跑报 `LEDGER_INVALID errors=1 checks=14945`（唯一错误 RES-R00-A16 STALE-SOURCE）——成为第二轮补充修复的触发事实。

## 7. 第二轮｜补充修复：账本重绑（本次新增）

### 7.1 授权与目标
第二轮指令明确：第一轮「不改账本」范围过宽，当前账本校验失败状态不可交付；允许且仅允许经既有 `r00_t07_build_map.py` 获准重建路径更新 ACCEPTANCE_MAP.json、BLOCKERS.md、R00-T07_RESULTS.json 及必要生成器输入，重绑新 HANDOFF SHA；ORCHESTRATOR_PROGRESS.json 与 R1 历史报告仍严禁编辑。目标＝最终 `r00_t07_validate_ledger.py` exit 0、清单逐项 0 差异、`git diff --check` 通过。

### 7.2 §8.4 路径执行记录（全部真实运行，原始输出落盘于 artifacts/rust-tauri/R00/T08/）

| 步骤 | 命令 | 实际输出 | 退出码 | 证据（SHA-256） |
|---|---|---|---|---|
| 修复前失败留证 | `python3 -B …r00_t07_validate_ledger.py` | `LEDGER-ERROR STALE-SOURCE result=RES-R00-A16 … 68877ff6… != recorded bde9eea9…`＋`LEDGER_INVALID errors=1 checks=14945` | 1 | ledger-validate-stale-r1.log（`14e8f542b0e4003b378a37afa1d6daa6d42e594937a43a6450e8862f51ff398c`） |
| 受影响验证复跑（源＝修复后 HANDOFF） | `python3 -B …r00_t08_a16_spotcheck.py` | 三点全过＋`A16-SPOTCHECK-PASSED handoff_sha256=68877ff6…` | 0 | a16-spotcheck-r2.log（`70e76dbfc3c9e9892f023dd43b4fa4323a4a25e029a921eca02a0393e190b210`） |
| 重建账本 | `python3 -B …r00_t07_build_map.py` | `MAP-BUILT scenarios=952 tasks=100 features=743 entries=832 tests=31 results=16 selftest=attached`（计数与前轮重建完全一致） | 0 | ledger-rebuild-r3.log（`10a851c70052f77a1550289c4de5fa45440c4d4d445302194e021e048e96e2c1`） |
| 自测 | `python3 -B …r00_t07_selftest.py --suite all --art-dir …/T08/ledger-selftest-r3` | `SELFTEST PASS variants=18/18 (+positive-control ok) temp_removed=19/19` | 0 | ledger-selftest-r3/（23 文件）＋ledger-selftest-r3-driver.log（`96b1cf2975af148d9929134d32f9fac4b867e223cdebaf1f106a419d5658f29b`） |
| 终验（重建后） | `python3 -B …r00_t07_validate_ledger.py` | `LEDGER_VALID checks=14945 scenarios=952 results=16 entries=832 spec_source=taskbook-strict` | 0 | ledger-validate-final3.log（`f808595fce7fc52841f0851d3d71cc77f71f3ee742ae58e7c35e1fb3eabb8862`） |
| 终验（全部文档编辑后再复跑，最终态） | 同上 | 同上 | **0** | （本轮会话直接复跑；输出与 final3 一致） |

### 7.3 重建语义 diff（重建前快照 vs 重建后，程序化全量比对）
`ACCEPTANCE_MAP.json` 语义差异**仅 3 处**：① `generated_at`（2026-09-24T04:54:34Z→05:49:35Z，重建时点）；② `results[15]（RES-R00-A16）.source_digests["docs/rust-tauri/R00/R00_HANDOFF.json"]`：`bde9eea9…`→`68877ff6…`；③ 同记录 `working_tree_digest`：`fca74c95…`→`2f561bb24f25226666c62f2216b4e4ed3ca5f1b167dc41268d203bef010a4feb`（生成器按「source_digests 拼接哈希」公式现算）。其余全部记录（A01–A15、场景/任务/索引/blockers/规格快照）语义零变化。`BLOCKERS.md` 经 cmp 字节一致（状态计数未变）；`R00-T07_RESULTS.json` 与 `r00_t07_build_map.py` 零改动——**本轮未编辑任何生成器输入**（A16 输入记录 `working_tree_digest=null`，由构建器现算；`timestamp_utc`/`tested_sha` 按生成器设计保留 T08 原值，`tested_sha=e0b7be610` 为 HEAD 祖先校验通过）。

### 7.4 防环与防再陈旧设计核对
- 校验器本身禁止任何结果监控账本自身（STALE-SOURCE 自引用规则），账本↔清单无环。
- HANDOFF 自第二轮起冻结于 `68877ff6…`（账本绑定值＝磁盘值＝清单值＝报告 §10 单列值）；REPORT/EVIDENCE_SUMMARY/CANDIDATE_MANIFEST 不在任何结果的 `source_paths` 中（已逐一核对 16 条结果），其后续编辑不会使账本过期。
- HANDOFF 不钉账本三件套哈希（其 L59 注记为设计如此：三者终态哈希记录于报告 §10）；报告 §10 已更新为新 ACCEPTANCE_MAP SHA `5eeac37c…` 并注明重建来源与三处语义差异。

### 7.5 清单更新与聚合重算
新增 28 个证据文件全部纳入清单（5 个日志 + ledger-selftest-r3/ 目录 23 个文件，逐文件 SHA 现算）；更新清单内 ACCEPTANCE_MAP.json 行（`e0bd3cfa…`→`5eeac37c…`）；其余 144 原行 SHA 零改动（BLOCKERS/R00-T07_RESULTS/build_map 行原值保留，与 7.3 一致）；清单保持路径排序、原相对顺序不变、格式「SHA␣␣路径」与结尾换行不变。新清单 173 行，聚合 SHA（清单自 SHA）＝`d83ae896ce19d1ee5a8c0c8084244c44a32516cee5d589a35ead9fff2a448645`，已同步报告 §10 聚合块与摘要 §1。**无新证据留在清单外**：T08 目录被 .gitignore 忽略的 93 项已全部核对在清单内。

### 7.6 计数同步（真实数量）
候选 173 = 7 修改 + 166 新（T08 目录 162〔含本轮新增 28〕+ docs 3 + 测试 1）；总新文件 169 = 166 + 清单外 3；T08 目录物理 163 = 162 + 清单自身。报告 §2/§4/§5（账本行日志引用含 final3 与 stale 留证）/§9（spotcheck 复跑注记）/§10（计数、构成、聚合、ACCEPTANCE_MAP SHA 及来源注记、排除项补列 REVIEW/REPAIR 两文档）/§13 与摘要 §1 全部同步。

## 8. 最终验证（第二轮完成后，全部真实运行）

| 检查 | 结果 |
|---|---|
| `r00_t07_validate_ledger.py`（最终态复跑） | `LEDGER_VALID checks=14945 scenarios=952 results=16 entries=832 spec_source=taskbook-strict`，**exit 0** |
| 清单逐行 SHA 对盘 | **173/173 一致，0 不匹配**；路径全唯一、全排序、格式与结尾换行不变 |
| 聚合一致性 | 清单自 SHA `d83ae896…` ＝ 报告 §10 ＝ 摘要 §1；ACCEPTANCE_MAP `5eeac37c…` ＝ 清单行 ＝ 报告 §10 ＝ 磁盘；HANDOFF `68877ff6…` ＝ 清单行 ＝ 报告 §10 ＝ 账本 A16 绑定值（冻结） |
| JSON 可解析 | R00_HANDOFF.json、ACCEPTANCE_MAP.json 均 OK |
| 陈旧引用清零 | REPORT/SUMMARY/HANDOFF/ACCEPTANCE_MAP 中「145 文件/141 个新文件/138 个新文件/134 个原始/135 文件/7dc6e4a0/e0bd3cfa/14e399ef/bde9eea9」全部 0 命中（旧值仅存于历史文件：R1 报告、a16-spotcheck-r1.log、总控账本、本报告演进表——均为对当时事实的记录） |
| 闭合复算 | git 可见集（-uall）86 路径 = 清单 173 中可见部分 + 声明排除 6（ORCHESTRATOR/报告/摘要/清单自身/REVIEW_R1/REPAIR_R1）——**零遗漏零多余**；忽略集 T08 目录 93 项全在清单内 |
| `git diff --check` | 通过（exit 0，无空白错误） |
| git 范围 | tracked 修改仍仅会话起点 8 文件；新增文件仅本轮 28 个证据 + 本修复报告；无分支/worktree/commit/push |
| 空白/制表 | 全部被改/新增文件 0 行尾空白、0 制表符 |

## 9. 后续须知（交 R2 与总控）

1. R2 建议最小动作：§7.2 表逐命令复跑对日志；§8 表逐项复算；差分复核报告/摘要勘误行；`shasum -a 256` 抽验清单与聚合；重跑 spotcheck 与账本校验器。
2. 历史文件旧 SHA 属预期保留：R00-T08_REVIEW_R1.md（含 14e399ef/bde9eea9/e0bd3cfa）、a16-spotcheck-r1.log（bde9eea9）、ORCHESTRATOR_PROGRESS.json（含「140/133 vs 145/141」发现描述）、T08 各原始 r1/r2 日志——均为当时事实，不改。
3. R1 报告结论（A15/A16 技术实质 PASS）沿用作技术面复核依据；其对候选 SHAs/计数的记载对应修复前状态，以本报告 §1 演进表为准。
4. 账本 A16 现绑 HANDOFF `68877ff6…`＝磁盘现值；如后续任何人再改 HANDOFF，将再次触发 STALE-SOURCE（设计如此）。

## 10. 范围遵守声明

两轮合计仅改动：R00_REPORT.md、R00_HANDOFF.json、R00_EVIDENCE_SUMMARY.md、CANDIDATE_MANIFEST.txt（以上候选内/配套文档）＋ 经获准生成器重建的 ACCEPTANCE_MAP.json（BLOCKERS.md 重建后字节不变）＋ 新增 28 个 T08 证据文件与本修复报告。未改：生产代码、测试、脚本（含三工具与生成器）、R00-T07_RESULTS.json、原始证据、ORCHESTRATOR_PROGRESS.json、R1 历史报告。未派生代理；未建分支/worktree；未 commit/push/PR/tag/release；未用真实用户数据、真实供应商、付费调用；未外发。不自评 PASS，READY_FOR_R2_REVIEW。
