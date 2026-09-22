# P05 复验收报告（FIXR1 后独立复验收子代理，全新上下文）

日期：2026-09-22｜验收对象：P05 阶段整体（执行轮 + 首轮验收 + FIXR1 修复轮终态工作区，HEAD `1f0537b0865c1f6a4a17c28eb66c6d9ebf4948a6` + 未提交改动）｜角色：只读复验收（本报告与命令证据日志为模板允许的唯一写入；未重生成 EVIDENCE_SHA256.txt，沿用首轮先例留作编排层提交前动作）。全部结论基于本轮独立复跑、源码/日志逐点核对与对抗性反例，未采信执行者/修复者自报。

## 结论

**阶段终判：PASS（含 FIXR1 修复轮后的阶段整体）**，附 1 项新发现 **R1（提交/收尾前必须处理，非阻塞）** 与 3 项外观级备注。

- FIXR1 两项修复均经独立核实**真正闭合**（§1、§2），范围纪律干净（§3），清单可独立复算（§4），f1 patch 未触碰（§5），门禁与定向测试全绿（§6）。
- **新发现 R1**：首轮验收自身的交付物 `artifacts/refactor-2026/P05/logs/P05-ACCEPTANCE-counterexample.test.ts` 落入 vitest 默认集且含 1 个故意失败的 C1 测试与 6 处机器绝对导入——**本工作区下一次全量 npm test 将从 F1 基线 4 红变为 5 红**（§7）。与首轮发现 #1 同类（该轮判非阻塞、走修复轮），故不翻阶段判定，但必须在编排层提交/收尾前处理。

## 1. 修复 #1 闭合核验（FI 测试可移植性 + 头注矛盾）

- **绝对路径零残留（独立 grep）**：`/Users/study_superior` 命中 0；`from "/`、`require("/` 形式命中 0。相对引用实际 **代码 11 处**（vi.mock×2 + importOriginal×2 + import×7；头注注释中另 1 次字面提及——FIXR1 记录写"import×8 共 12 处"，为计数口径小误，见 §8）。
- **四模式真实复跑全绿**：仓库根；深层子目录 cwd（desktop/src/react/hooks）+ `--root` 仓库；`NODE_PATH=/tmp/nonexistent-junk` 干扰；经 `npm test -- <FI 文件>`（即 npm test 默认 exclude 配置）。「相对路径仍不可移植」反例（深 cwd / NODE_PATH）均未成立。
- **默认集成员资格三重独立证明**：`npx vitest list` 收录其 2 例；npm test 脚本路由执行绿；执行轮全量日志 P05-T08-full-suite.out:4164 与首轮验收全量日志 :8524 均留有 `✓ …P05-FAULT-INJECTION.test.ts (2 tests)` 真实执行记录。npm test 的 --exclude 列表（.claude/.cache/dist*）确不含 artifacts/，vitest 默认 include 覆盖之。
- **头注/HANDOFF §3 矛盾裁决方向以事实为准**：HANDOFF §3（mtime 10:58，FIXR1 未动）原文「已并入默认 vitest 集」为正确一侧；旧头注「不进入默认测试集」为错误一侧；现头注已改记「已并入默认测试集并在全量套件真实执行」，与三重证明一致。
- **断言语义未弱化（对抗性元反例）**：在隔离临时目录构造「去故障直通」变体（两处 vi.mock 改为委托原始实现）复跑——**2/2 如期变红**（FI-1 `expected 'process' to be 'answer'`；FI-2 `expected 5 to be less than 5`）。证明故障注入是承重的，测试真实挡错而非冒充通过。残余限制（如实登记）：该文件无修复前字节基线可 diff，「仅路径与头注变化」由以下佐证支撑——mock 面与首轮验收独立阅读记录（"仅篡改段重导出/页边界两处，live 链与 projector 真实"）逐点吻合、T07 开发期 r2/r3 失败日志显示同类断言当时即会咬错、元反例载荷实验。临时目录已清理。
- 澄清一项背景事实：`.gitignore:94` 的 `logs/` 规则使该文件（在 logs/ 下）**不会进入普通 `git add`**——首轮发现 #1 的「提交后 CI 必红」以 force-add 为前提（P00 曾对 276 个 logs 文件 force-add，故该前提并非不可能）。修复本身仍正确且必要：该文件是本工作区本地默认集的常驻成员，任何克隆/换机/force-add 场景都依赖可移植路径，且记录准确性已随修复成立。

## 2. 修复 #2 闭合核验（ACCEPTANCE_MAP A14 归属）

- `runs checkpoint, barrier, migration, validation, and final commit in exact durable order` 位于 **tests/data-epoch-coordinator.test.ts:252**（A14 的 test_path + test_names 现指向正确）；`requires a checkpoint receipt after the prepared phase` 位于 **tests/data-epoch.test.ts:109**（supporting 条目附行号与修正说明）。
- supporting 其余测试名逐条命中：data-epoch.test.ts:72（fails closed on corrupt JSON…）、:134（rejects malformed or unknown journal phases…）。**coordinator 文件中误记名零残留**（独立 grep）。
- `fixture_sha256` 语义独立验证 = sha256(tests/data-epoch-coordinator.test.ts) 本身，与 FIXR1 声明「保持 coordinator 文件哈希」一致。
- 双文件复跑 40/40 绿。
- **归属反例扫描（防"修正引入新错误"）**：16/16 场景 test_path 全部存在、test_names 全部可在所指文件定位。A07/A08 有三处标题为「带注解/截断」近似写法（如 map 记 `T04：重复到达的同一页幂等`，实题 `…幂等，不增加显示项或过程块`）——文件归属正确、可唯一定位，属外观级（执行轮原始写法，非 FIXR1 引入，见 §8）。

## 3. 修复轮范围纪律

- `git diff HEAD --stat`：仅执行轮 3 个测试文件 +282/-0，**无生产代码、无既有断言修改**（零删除）；FIXR1 全部改动落在未跟踪交付目录内。
- mtime 证据：执行轮文档全部 ≤10:58、首轮验收报告 11:17、HANDOFF 10:58（未动，符合声明）；11:17 后变更仅 = FI 文件(11:21)、ACCEPTANCE_MAP(11:22)、command-log/EVIDENCE_SHA256/P05_REPORT/P05_RESULT(11:25)、FIXR1 新日志。
- **防篡改交叉核对**：command-log.jsonl 内 64 条 stdout/stderr_digest 与当前文件字节 **64/64 全 match**——执行轮全部日志自产生起未被改动。command-log 27（首轮清点）+5（FIXR1）= 32 条，编号唯一。
- manifest-check.out 不入清单/不入 jsonl：FIXR1 有书面理由（清单与 jsonl 互为哈希绑定的自引用约束），P04-FIXR1 同款先例经核实存在（artifacts/refactor-2026/P04/logs/P04-FIXR1-manifest-check.*）。

## 4. EVIDENCE_SHA256 变化合理性

- **独立复算**：`shasum -a 256 -c` exit 0，**91/91 OK**；路径列全仓相对化后可单 cwd 全量校验（本轮复跑即证）。
- 覆盖：实际 93 文件 − 清单 91 = 自身（模板 §7 不自含，正确）+ manifest-check.out（§3 所述声明排除）。
- 「未改文件哈希与上一版一致」：无旧清单字节可直接 diff（如实登记），以三重链佐证——digest 交叉核对 64/64（执行轮日志字节=产生时哈希）、mtime（首轮校验后无触碰）、首轮验收 §4 曾逐条核过 73 条旧清单。哈希↔文件绑定经本轮 91/91 复算无错位。

## 5. 提交前检查项（f1 patch）

- `89bc0b64-to-r01-r10-source.patch` 本轮运行前后哈希一致（25fb315f…，与 HEAD 版本同）；本轮全部为定向运行，未触发重写路径；终态 `git status` 与起点逐字一致。本轮**未运行全量 npm test**（理由见 §6），如编排层后续全量运行，仍须按 HANDOFF §8 检查/还原该文件。

## 6. 门禁与测试复跑；全量未重跑的沿用条件

- 已复跑全绿：`npm run typecheck`（×3，exit 0）、`npm run typecheck:core-contracts`（exit 0）、执行轮 3 测试文件 84/84、data-epoch 双文件 40/40、FI 文件四模式 2/2。
- **全量未重跑，沿用条件如下**：(a) FIXR1 后 `git diff HEAD` 相对首轮独立复现全量（11:16，P05-ACCEPTANCE-full-suite.log，digest 链完好）以来的唯一差异 = 未跟踪交付物内的路径字符串/头注/记录文档，零生产代码、零已跟踪测试变化；(b) FI 文件变化经四模式独立验证；(c) 默认集成员资格未变（vitest list）。重跑无新信息且会触发 f1 patch 重写副作用。
- **但须明确**：若现在重跑全量，结果将不是 14769/4 而是 **5 红**——由新发现 R1（§7）造成，与执行轮/FIXR1 的任何改动无关。

## 7. 新发现 R1（提交/收尾前必须处理；非阻塞，与首轮 #1 同类同处置级别）

**事实**（全部本轮独立复现）：
1. `artifacts/refactor-2026/P05/logs/P05-ACCEPTANCE-counterexample.test.ts`（首轮验收交付物，11:16 写入）匹配 vitest 默认 include，`vitest list` 收录其 5 例；直接复跑 **1 failed | 4 passed**——C1（跨流迟到事件污染投影，纵深防御缺口登记项）在本工作区恒红。
2. 该文件含 6 处机器绝对导入（`/Users/study_superior/...` ×5 import + REPO 常量），与本轮已修复的 FI 文件原属同一缺陷类。
3. 后果：**本工作区下一次任何 `npm test` 全量将收集它 → F1 基线 4 红 → 5 红**，污染 P06–P08 各轮依赖的基线信号；CI 影响以 force-add 为前提（logs/ 被 .gitignore:94 忽略，HEAD 无任何已提交 artifacts 测试文件——`git ls-files | grep artifacts/.*test.ts` = 0）。
4. 成因归属：首轮验收自身（其 §6.1 刚指出 FI 文件的同类问题，却把自身反例文件以绝对路径+红测试形式放入同一默认集目录，头注亦写「不进入默认测试集」——与事实矛盾，同款错误）。FIXR1 范围仅含两项登记发现，不含此文件，无越界责任。

**最小修复要求**（二选一，建议前者，与 FI 修复先例一致）：(a) 导入改仓库相对路径（`../../../../`），C1 改 `it.fails(...)`（vitest 预期失败）——文件在默认集内计绿，RED 证据仍完整保留于 P05-ACCEPTANCE-counterexample.log；(b) 将该证据副本改非收集扩展名（如 `.test.ts.txt`）或移出收集集。**勿**给 npm test 增加 `artifacts/**` exclude——那会推翻 §1 刚以事实对齐的 FI 文件/HANDOFF §3「已并入默认集」语义。修复后须同步刷新 EVIDENCE_SHA256（该文件在清单内）。

## 8. 外观级备注（不要求本轮处理）

1. FIXR1 记录「共 12 处相对引用（import×8）」：实际代码引用 11 处（import×7），第 12 次出现为头注注释字面提及。无实质影响。
2. ACCEPTANCE_MAP A07/A08 三处 test_names 为注解/截断式近似标题（文件归属正确、可唯一定位）；执行轮原始写法，两轮验收均未按字面全匹配复核过——后续阶段按 92 模板「用例名」字段可考虑字面化。
3. P00 曾 force-add 276 个 logs/ 文件而 P02–P04 均未提交 logs/——P05 提交面（docs 15 + samples 4 + tools 1）与 logs/ 的处置（本地保留 vs force-add）需编排层显式决策；若 force-add logs/，R1 的 CI 面即被激活。

## 9. 本轮写入与未验证边界

- 写入：docs/refactor-2026/P05/P05_REACCEPTANCE_REVIEW.md + artifacts/refactor-2026/P05/logs/P05-REACCEPTANCE-commands.log（命令、cwd、exit 与结果索引）。未修改任何生产代码、测试、执行轮/验收轮/修复轮文件；未 commit/push/tag。
- 未验证（如实登记）：全量套件本轮未重跑（§6 沿用条件；R1 使当前工作区预期为 5 红）；四平台 CI / open server 冒烟（P08 范畴）；真供应商冒烟（P04-T07-2 继承 BLOCKED）；FI 文件修复前字节级 diff（无基线，§1 已列佐证链）。不承诺绝对无缺陷。
