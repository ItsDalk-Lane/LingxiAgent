# R01-T08 独立对抗性验收报告 R1

- 验收代理：ZCode:R01-T08-review-r1（全新独立；未参与 R01 任何执行/修复/此前验收；只读审查 + 独立复跑，临时文件均在 /tmp，未修改任何已提交文件与执行者交付物）
- 日期：2026-09-25｜分支 codex/rust-tauri-migration｜基线 HEAD `2bbec6d072f1a06305392828bfe3a7fd9739e667`（开工实测一致）
- 环境：macOS 27.0 arm64；Node v24.16.0；Python 3.14.3；rustup + rustc/cargo 1.98.1（复跑全程 `env -u` 剥离六个代理变量；cargo 一律 --offline + 本验收自有全新 CARGO_TARGET_DIR）
- 验收对象：R01-T08 执行者（ZCode:R01-T08-exec-r1）全部交付与声明（R01-T08_REPORT.md 逐项）
- **最终判定：PASS**（A15/A16 独立验收通过；R01 阶段放行所需的 T08 交付真实、机器可复算、对抗稳健；唯一新发现 RR-T08-F1 定性准确、归属合理）

---

## 1. 候选清单与工作区核实【实际运行】

`git status --porcelain` + `git diff --stat HEAD` 实测：工作区差异恰为预期集合——
- 未跟踪新文件 9 个交付（docs/rust-tauri/R01/ 下 R01_REPORT.md / R01_HANDOFF.json / RISK_REGISTER.json /
  R01_ACCEPTANCE_LEDGER.json / R01-T08_REPORT.md / 三个检查器 + gate_inputs.json）+ artifacts/rust-tauri/R01/T08/；
- 已跟踪修改仅 tests/migration/r01-t05/ 两文件，diff 逐行核实为**纯注释更正**（setBlockedURLs→CSP
  connect-src 机制说明，9 insertions/4 deletions，零语义变化）；
- T06 的 6 个未入库二进制不出现在 status（已删除，见 §8）。

生产零改动确认：`git diff HEAD` 不含任何 desktop/server/core/lib/shared/scripts 内容改动。

## 2. 交付物复算哈希【实际运行】

R01_HANDOFF.json `artifact_hashes` 全部 17 条逐文件 `shasum -a 256` 复算，**17/17 逐字符一致**
（含 R01_REPORT.md=7ca9e182…、RISK_REGISTER.json=2157764c…、R01_ACCEPTANCE_LEDGER.json=39492b89…、
三检查器、gate_inputs、两个注释更正文件、disposition/coverage/gate/isolation 三份报告、
OWNERSHIP_TARGET/FEATURE_INVENTORY/PI_REPLACEMENT_MATRIX 三个上游冻结文件）。
T02 三条 fresh-target 门禁日志哈希复算与 gate_inputs.json 钉住值一致
（roundtrip=02efaa2e…、handshake=2728d798…、check-generated=34910e03…）。

**working_tree_digest 方法审查与复算**：范围为「全部 git 跟踪文件工作树内容 + 未跟踪非忽略新文件 +
T08 下被 *.log 忽略的证据文件」，排除自引用的 R01_HANDOFF.json 与明细文件自身。本代理独立重建文件集
（git ls-files 三态枚举，8686 个文件）并逐文件复算 SHA-256，与明细文件
`artifacts/rust-tauri/R01/T08/working-tree-digest.txt` 的 8686 行**集合完全一致**（含两个非 ASCII
路径文件）；对明细负载（sha256+两空格+路径、按路径排序、尾换行）整体哈希 =
`239acceb24459d7f0c1d1f5575f83f3864f04e03288c26292bee861de105e404`，与 HANDOFF 钉住值一致。
自引用排除正确（含 digest 的两个文件均被排除，无循环）。初次复算差异系本代理工具 git quotepath
转义所致，更正后闭合——非执行者问题。

## 3. R01-A15 关卡检查器：复跑 + 对抗【实际运行】

| 场景 | 命令 | 退出码 | 结论 |
|---|---|---|---|
| 负向自测 | `python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --self-test` | 0 | 5/5：N1 截图 VERIFIED+user_takeover FAILED → browser_host=NOT_COMPLETE、VERDICT=NO-GO、替壳 BLOCKED 且原因点名 user_takeover |
| 真实数据 | `python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --out /tmp/r01t08-review/gate-report.json` | 0 | 五域全 COMPLETE；VERDICT=**PASS_WITH_CONDITIONS**；15 递延项全部 TRACKED（风险条目存在且 resolve_by_stage/failure_handling 非空）；SHELL-REPLACEMENT=ALLOWED_FOR_NEXT_STAGE_ONLY |

**本代理自造对抗变体（执行者自测之外）7/7 全部按预期拒绝**：
G1 PDF 必需能力证据路径不存在 → NO-GO/BLOCKED(evidence-missing)；G2 shell 必需能力标 UNVERIFIED
（演示遮蔽形态）→ NO-GO/NOT_COMPLETE；G3 递延项风险条目存在但 resolve_by_stage 置空 →
NO-GO/BLOCKED(risk-incomplete)；G4 user_takeover FAILED → NO-GO 且 problems 点名 user_takeover
（原因可诊断）；G5 证据 sha256 替换为另一真实文件哈希 → NO-GO/BLOCKED(evidence-hash-mismatch)；
G6 递延项风险条目整條删除 → NO-GO/BLOCKED(risk-missing)；正面对照真实数据仍 PASS_WITH_CONDITIONS。
（透明披露：本代理首版 G5 误把篡改值设为同一证据文件自身的真实哈希，属变体构造错误，更正后拒绝成立；
检查器自带 N2 本就覆盖该类别。）

**输入数据真实性抽查（非执行者凭空填写）**：gate_inputs 五域状态与源证据逐项相符——
T04 chromium summary.json 实测 21 pass/0 fail、takeover summary 5 pass/0 fail；T05 matrix-verdicts.json
28 VERIFIED/0 FAILED、repair-r1 negative-verdicts.txt 7/7 PASS、A09 compare.json all_pass=true（35 项）；
T06 runner-summary.txt 15 步 exit=0、SHELL_CAPABILITY_MATRIX 14 能力状态（recording/speech/screen
授权正链路与 proxy 恰为 UNVERIFIED，与递延清单一致）；T07 a7b 拒写日志
（LINGXI_DATA_EPOCH_BLOCKED，EXIT_CODE=1）、rollback-drill-summary ROLLBACK-DRILL-PASSED。
全部证据文件 SHA-256 由检查器在本代理复跑中重算核验一致。

## 4. R01-A16 双向覆盖检查：复跑 + 独立重算 + 对抗【实际运行】

| 场景 | 命令 | 退出码 | 结论 |
|---|---|---|---|
| 真实运行 | `python3 -B docs/rust-tauri/R01/r01_t08_coverage_check.py --out /tmp/r01t08-review/coverage-report.json` | 0 | COVERAGE-CLOSED（16 项检查全 PASS） |
| 负向自测 | `… --self-test` | 0 | 6/6 |

**独立重算（不信检查器，直接算 R00/R01 源 JSON）**：FEATURE_INVENTORY features=736、
feature_ownership=736、store_ownership=69、PI capabilities=14；双向差集 missing=0/orphans=0、
双方向均无重复；4 个 worker 类 owner 承载 F-ID 数=0；14 条 Pi 能力 disposition 全部
KERNEL_MIGRATION 且 rust_owner/target_stage 全非空；coverage_difference 四类 unmapped 全空；
19 条 worker 反例 KERNEL_MIGRATION 项均有 rust_owner。与执行者声明数字完全一致。

**本代理自造变体（自测之外）7/7 全部检出**：V1 双归属（复制一条映射）→ C2-dup-ownership；
V2 worker 持有存储 → C6-no-worker-store；V3 F-ID 空 stage_ids → C4；V4 Pi unmapped_hooks 非空 →
C9-pi-coverage-diff；V5 映射指向幽灵 F-ID → C2-orphan+unmapped 双检出；V6 worker 反例
KERNEL_MIGRATION 缺 rust_owner → C9-worker-counterexamples；V7 关键事实集加第 12 条 →
C5-critical-facts-set。worker 拥有核心职责（自测 N2/N5）亦复证拒绝。

## 5. RISK_REGISTER 完整性【源码确证 + 机器复算】

- 43 条、ID 无重复；状态分布 OPEN 24 / CARRIED 14 / CLOSED_IN_R01 2 / DOCUMENTED 2 / REGISTERED 1，
  与执行者 §5 汇总机器复算一致。
- **任务点名遗留项逐项对照无一遗漏**：T02 F1–F5（RR-T02-F1..F5）+ F-info-1（RR-T02-FINFO1）；
  T05 N1–N3（RR-T05-N1/N2/N3）+ T04 交叉影响（RR-T05-X1）；T06 R1–R8（RR-T06-R1..R8）+
  UNVERIFIED 五项（MIC/SPEECH/SCREEN/PROXY/PLATFORM）；T07 PROD-DEFECT-1/F3/F2；
  审计封印两基点分类（RR-AUDIT-SEAL-PREEXISTING）；R00 BLK×4；T03-R3-F01/F02。
  R00 HANDOFF unresolved_items 全量 14 条 = 本登记 CARRIED 14 条（单一事实源保持）。
- **10 条抽样与原始报告核对**（T02 F1/F5、T02-FINFO1、T05 N2、T05 X1、T06 R3、T06 SCREEN、
  T07 PROD-DEFECT-1、T07 F3、AUDIT-SEAL）：表述与 R01-T02_REVIEW_R1 §5、
  R01-T02_FIX_HEADSHA_REVIEW_R1 §F-info-1、R01-T05_REVIEW_R2 §N2、R01-T05_REPAIR_R1 交叉影响段、
  TAURI_SPIKE_REPORT 风险表、SHELL_CAPABILITY_MATRIX、R01-T07_REVIEW_R2 变体表、
  DATA_COMPATIBILITY_MATRIX note、R00_HANDOFF 原文逐一相符，无失真；每条均有明确截止阶段与
  失败处理（fail-closed 方向）。RR-T02-F1 摘要「UTF-8 字节序」对原文「码点序」为等价表述
  （BTreeMap<String> 字节序即码点序），不判失真。
- 递延项 15 条被关卡检查器机器核验全部挂账（§3 真实数据运行）。

## 6. R01_REPORT / R01_HANDOFF 结构【源码确证】

- R01_REPORT.md：91 §1 全部 15 个必填段齐全（阶段与结论/范围/源码/环境/完成项/行为变化/验收/
  安全与数据/完整映射/已知缺陷/**未执行-BLOCKED**/回退/独立审查/下一阶段/远程-发布），未删除
  「未验证」内容；结论如实 READY_FOR_REVIEW 而非自评 PASS；平台范围限定 macOS arm64 反复明示。
- R01_HANDOFF.json：04 §2 全部九字段齐备（source_sha/working_tree_digest/protocol_version/
  data_epoch/dependency_locks/accepted_tasks/unresolved_items/allowed_next_scope/artifact_hashes）；
  remote_write/release 等四个授权旗标全 false，如实。
- **accepted_tasks 与 ORCHESTRATOR_PROGRESS.json（只读）+ git log 三方一致**：T01 58e00b53/PASS(R3)、
  T02 9390ae01/PASS(R1)+修复 b9442d86 PASS、T03 5b63323b/PASS(R1)、T04 abe4d545/PASS(R1)+docfix、
  T05 4fa0b573/PASS(R2)、T06 8994c67b/PASS(R2)、T07 73bde3b5/PASS(R2)、T08 commit=null/在验。
  八个 T 的 commit 均实测存在于 git log（stage base 328cc8bb…→HEAD）。
- 唯一偏差见 §10-O1（T04 docfix_commit 标签指向纯账本 chore 提交；docfix 内容实证在 abe4d545 内）。

## 7. RR-T08-F1 定性复核【源码确证 + 实际运行】

- **源码确证缺陷真实存在**：scripts/rust-tauri/r01-t02-{roundtrip,handshake,check-generated}.sh 三者
  默认 `CARGO_TARGET_DIR=/tmp/lingxi-r01t02-target`（共享缓存）；lingxi-protocol-gen.rs:771
  `repo_root()` 用编译期 `env!("CARGO_MANIFEST_DIR")` 上溯三级解析仓库根。共享缓存中残留他树编译的
  二进制时，cargo 指纹未变不重编译，`--check` 即比对他树。
- **错绑证据真实**：执行者首轮日志 artifacts/.../gates/t02-check-generated.log 实测含
  「OK: 56 generated files match regeneration (no diff) under **/private/tmp/r01t02-review-clone**/
  contracts/generated」——绿灯比对的确为遗留克隆树。残留克隆与共享 target 现已删除（本代理实测
  /tmp 下两者均不存在），与本任务 /tmp 清理记录互证。
- **本代理全新 target 独立复跑三脚本**（CARGO_TARGET_DIR=/tmp/r01t08-review-r1-target，全量重编译）：
  roundtrip exit=0（12 golden Rust→TS→Rust 逐字节一致 + tsc + 负向）、handshake exit=0
  （5 场景，4409 带 details 四元组）、check-generated exit=0 且输出正确指向
  `/Users/study_superior/Desktop/Code/LingxiAgent/contracts/generated`（56 文件 + 624 条目零漂移）。
  「本仓库生成树真实零漂移」结论真实。
- **定性认同**：严重度 MEDIUM 合理（门禁健壮性缺陷，当前 repo 树已证零漂移，但 CI 固化后遇陈旧缓存
  会静默错绑他树——fail-open 方向的门禁缺陷）；归属 R02（CI 门禁固化前硬化：target 按检出派生 /
  gen 运行时解析根）合理，执行者未越权自修前序交付、如实登记上报，处置符合协作规则。

## 8. 处置与清理核实【实际运行】

- T06 六个未入库二进制磁盘实测全部不存在；disposition JSON 逐文件 sha256 与已入库
  SHASUMS-binaries.txt 钉住记录逐字符一致（三组同哈希，位级可复现声明与记录相符）。
- `git ls-files artifacts/rust-tauri/R01` = 1802 个跟踪文件完整（含 T06 全部日志/SHASUMS/
  docfix-r1-gcm 证据），无误删已入库证据。
- /tmp 清理：tmp-cleanup/CLEANUP_RECORD.md + 清单 + du 快照齐备（74 项约 36GB，含明确未触碰清单）；
  本代理实测 /tmp/lingxi-r01t02-target 与 /tmp/r01t02-review-clone 已删、/tmp/lingxi-r01-t08-rust-target
  按披露保留。

## 9. 隔离核查与生产入口【实际运行】

- `python3 -B docs/rust-tauri/R01/r01_t08_isolation_check.py` 本代理复跑 exit=0：2209 生产文件
  9 类模式 0 违规；209 个原型跟踪文件 0 个落在生产目录；spike 构建产物 gitignore 覆盖；
  rust/target 未覆盖如实报告（不判负，建议入 R02 目录约定——已写入 HANDOFF allowed_next_scope）。
- 人工抽查复证：package.json / desktop/main.cjs / desktop/preload.cjs grep 九类原型引用模式零命中。

## 10. 门禁回归（本代理亲自复跑，全部真实 exit 码）【实际运行】

| 门禁 | 命令（摘要） | 退出码 | 结果 |
|---|---|---|---|
| T01 正向 | `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py` | 0 | RESULT: OK |
| T01 N1–N15 | `… --self-test` | 0 | 15 弹药全按规则 ID 拒绝（日志 /tmp/r01t08-review/） |
| T01 生成器 | `r01_t01_build_ownership.py --check` | 0 | UP_TO_DATE features=736 stores=69 |
| T02 roundtrip | `bash scripts/rust-tauri/r01-t02-roundtrip.sh`（全新 target） | 0 | §7 |
| T02 handshake | `r01-t02-handshake.sh /tmp/r01t08-review-r1-handshake` | 0 | §7 |
| T02 check-generated | `r01-t02-check-generated.sh`（全新 target） | 0 | §7 |
| cargo test | `rustup run 1.98.1 cargo test --workspace --offline`（全新 target /tmp/r01t08-review-r1-cargotest） | 0 | **45 passed / 0 failed / 0 ignored**（逐 suite 求和复核：19+11+7+7+1） |

**已知预存失败承继核实**：`npx vitest run tests/post-verification-audit-seal.test.ts` 实测 1 failed/
2 passed，violators 为 VERIFIED_SOURCE_SHA 之后提交的 R01 迁移 artifacts——与
RR-AUDIT-SEAL-PREEXISTING 的「两基点分类、R01 提交后继续预期红」完全一致；与本任务无关且被
RISK_REGISTER/R01_REPORT 正确承继（未修绿、未删用例、未扩白名单）。该测试运行后工作区无新增污染。
未跑全量 npm test（任务规格既定，round2/round3 副作用项已挂账 RR-R00-ROUND2-SIDEEFFECT）。

## 11. 发现问题

无阻塞发现。两条 INFO 级观察（不影响判定，均不要求本阶段返工）：

- **O1（INFO，可追溯性标签）**：R01_HANDOFF.json `accepted_tasks[R01-T04].docfix_commit` 指向
  5a8a8e24a，但该提交实测仅含 ORCHESTRATOR_PROGRESS.json 账本改动；docfix 实质内容（D-09 版本号
  修正、BROWSER_SPIKE_REPORT 措辞精确化、docfix-r1-gcm 证据目录）经本代理 git 取证全部包含在
  T04 主提交 abe4d545 内（docfix 基线 08b9f0750 早于 abe4d545，执行/修正先于入库提交）。当前已提交
  树内容完整正确，仅「docfix_commit」字段语义不精确。建议：后续 HANDOFF 模板注明 repair/docfix 内容
  所在提交与账本记录提交可能不同（不阻塞，归 R02 交接文档 hygiene）。
- **O2（INFO，设计固有边界）**：gate_check 对递延项挂账的核验是「条目存在 + 两字段非空」，无法机器
  识别伪造的敷衍条目；真实性由独立验收（本轮 §5 抽样 10 条无失真）与后续阶段关卡承担。属设计内
  分工，非缺陷。

执行者披露的 RR-T08-F1 经 §7 复核定性准确，不重复立案。

## 12. 未验范围（如实声明）

- 跨平台（Windows/Linux/macOS x64）任何结论：未验，挂 R09/R10（与登记一致）。
- A01–A14 的原始场景未逐项重跑（各任务已有独立验收 PASS 且账本/进度/提交三方一致）；本轮重点为
  T08 新增关卡逻辑、数据真实性与门禁回归。A09/A11/A13/A14 证据抽查相符。
- 全量 npm test 未跑（任务规格既定；审计封印预存 FAIL 已定向复证分类正确）。

## 13. 判定

**PASS**。R01-A15（负向 5/5 + 真实数据 PASS_WITH_CONDITIONS + 本代理 7 组自造变体全拒）与
R01-A16（COVERAGE-CLOSED + 736/69/14 独立重算 + 7 组自造变体全检出）独立验收通过；
R01_REPORT/R01_HANDOFF/RISK_REGISTER/R01_ACCEPTANCE_LEDGER 四交付结构与内容经规格逐项核对、
哈希全量复算、源证据抽样比对均真实无伪造；RR-T08-F1 定性准确且本仓库零漂移结论经全新 target
独立复跑证实；处置/清理/隔离/门禁回归全部复证。R01 阶段放行条件中 T08 承担的部分已满足
（放行决策本身归总控）。

附：本报告 SHA-256 见返回总控消息（报告写盘后计算，避免自引用）。
