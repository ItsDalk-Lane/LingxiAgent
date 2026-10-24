# R08 阶段终审（R08_FINAL_REVIEW.md）

- 终审人：R08 独立终审代理（评审人 ≠ 实现者；终审期间未修改任何产品代码/脚本/文档）
- 日期：2026-10-24（CST）
- 基线：git HEAD `86840d1700397d3dd620a41f2b884df4cddd5cc2`（分支 codex/rust-tauri-migration，全程未动；未执行 checkout/restore/clean/commit/push）
- 对象：R08 阶段整体（T01–T08 交付、R08_REPORT.md、R08_HANDOFF.json、证据链、xtask R08 注册与最终门）
- 方法：结论一致性核对 + 治理修正核验 + 评审链/证据抽查 + 只读核对 + r08_run.py 包裹的独立复跑 5 项（rev-r08final-*）+ 最终门进行中快照（不等待）

## 结论：ACCEPT_HONEST_CLOSURE

**BLOCKER 0 / MAJOR 0 / MINOR 4 / 观察项 4（不计级）。**

R08_REPORT 与 R08_HANDOFF 声称的诚实关闭态（CLOSED_HONEST_NOT_FULLY_GREEN）与盘上证据**一致**：
16 基础场景、126 补充叶（9 份额图钉 + 117 required_behavior_pending）、门 overall 预期非 PASS——
均经独立核对成立，未发现虚报（声称完成但无证据）。4 项 MINOR 均为措辞/账面/绑定机制披露级，
不影响任何绿证据的有效性。

## 一、逐任务终审表（T01–T08）

| 任务 | 状态 | 证据指针（终审核对过的） |
|---|---|---|
| T01 API 绑定表/环境依赖 | 完成且受门绑定 | `T01_API_BINDING_TABLE.json`；场景 R08-A01/A02；dryrun 中两套件 PASS；a01-*/a04-* 证据族在册 |
| T02/T03 设计与预览演练 | 完成（a05/a06 见观察 O1） | `T02_DESIGN.md`/`T03_*`；t03-batchf-vitest-01（两条待办已按台账移交并处置）；a05/a06 演习入阶段图 |
| T04 夹具与导入 | 完成 + 治理修正（见 §二） | t04-*/t04i-* 60+ 组；夹具自造 entryType 格式事实核（`r08_t04_fixture_gen.mjs` L80-102）；`legacy_import/` 未跟踪（观察 M4） |
| T05 迁移切换 | 评审 PASS（独立） | `T05_REVIEW.md`：BLOCKER 0/MAJOR 0/MINOR 4（文档级）；t05-* 全族在册 |
| T06 回滚/再前滚 | 评审 PASS-with-MINORS + 移交落地 | `T06_REVIEW.md`；rev-t06-* 8 组复跑全绿且同绑 d5962b50…；N1 已修（16/16 更正）、N4 已回写、N2/N3 已落地（见 §三）；runbook 在册 |
| T07 UI 全功能验收 | READY_FOR_REVIEW（如实未全绿） | t07-ab-drill-01..04 红链 + final-05 绑定绿（ab-compare.json：tasks=15 parity=12 differs=0 recorded=2 failedB=1）；probe-01/02 红 → 03 绿；F-T07-1/F-T07-2 修复代码核（§二/§四）；D-1..D-5 在差异报告如实 |
| T08 注册与交接 | READY_FOR_REVIEW；最终门运行中 | `stage_maps/R08.json`（18 命令/16 场景/126 叶）+ main.rs:72 + runner_identity.rs:63-96 注册 + candidate.rs 删除跳过修复（candidate.rs:204-210）；inputs --check 复跑 PASS；R08_REPORT/HANDOFF 交付 |
| 最终门 | **进行中（不等待，主会话登记）** | PID 57977；r08_inputs_check PASS（digest 70b75054… match）；r08_t01_a01_a02_ws 套件回执 exit 0；其余命令排队中 |

## 二、治理修正核验（F-T07-1，本阶段最重要发现）——成立

1. **修复前红证据** `t07-real-format-probe-02`：`conversational=0 / unknownBlocks=4`，
   尾行 `REAL FORMAT PROBE: DIVERGENCE CONFIRMED (F-T07-1)`——真实 Pi 格式导入全部落
   legacy-unknown-block，红证据在案、未被删改。
2. **修复后绿证据** `t07-real-format-probe-03`：`conversational=3 / unknownBlocks=0`，
   `BOTH KERNELS FAITHFUL`；两次回执 sourceBindings 对拍：仅
   `legacy_import/session_jsonl.rs`（5bb341c7→5fe79f41）与 `convert_sessions.rs`
   （a2131b71→4805e77d）两绑定变化、其余 664 项全等——红→绿归因精确锁到修复文件，
   且当前盘上两文件哈希 = probe-03 记录值（修复在树）。
3. **夹具不变性** `t07-pi-fix-amend-proof-01`：`bulkCounts=435/435/5206`、v3 锚
   `80a8d2c2…feaa` 与 v2 锚 `40f42dc1…` 双保持、`doubleRunIdentical=true`、
   `zeroHit=true`（16 项 PASS 逐行核对）——修复对夹具路径零扰动成立。
4. **报告表述如实性**：R08_REPORT §一/§三「T04–T06 的迁移绿证据原本只对夹具自造格式
   成立」——事实基础核验：夹具确用自造 `entryType` 形状（fixture_gen L80-102），真实旧栈
   `lib/session-jsonl.ts` 确为 `{type:"message",message:{role,content}}` 嵌套形状；
   报告将 T04–T06 绿证据的适用范围如实收窄并说明修复后对真实格式亦闭合——**表述与证据相符，
   未粉饰早前宣称**。红→绿 TDD 链另见 t07-pi-fix-adapters-01（新测试
   `pi_sdk_real_format_entries_recognized` 先红 177/1）→ adapters-02 绿 178/178。

## 三、T06 评审链核验——成立

- `T06_REVIEW.md` = PASS-with-MINORS（BLOCKER 0/MAJOR 0/MINOR 5）；rev-t06-* 8 组目录在册，
  receipt 全部 exit 0 且 candidateDiffSha256 全部 = d5962b50…（与评审宣称同绑一致）；
  D1/D2/D3 `DRILL OK`、amend `435/435/5206+双锚+doubleRunIdentical`、一致性门 PASS 逐行复核。
- 移交项落地：N1 台账已更正（17/17→16/16）；N4 已回写（PROGRESS-t06 记录 + runbook 为权威）；
  **N2** `canonical_manifest_string_from_value` 已从 rollback_delta.rs 移除（grep 零命中）；
  **N3** `stage_quiesced_copy` 已对称落地两处（rollback_delta.rs:203 observability 读 +
  cutover_rollback.rs:1379 knowledge 判别）；重绑证据 t06-n2n3-* 6 组 exit 0
  （build/svcbin 16/16/adapters 177/177/D1/D2/D3 全 OK）。
- 本人未重跑 T06 演习（终审约束：避免与运行中最终门争用 47613 端口/单写者锁）；
  T06 评审自身 8 组复跑 + n2n3 重绑证据充分。

## 四、T07 抽查核验——成立

- `t07-ab-drill-final-05`：evidence 目录 receipt 绑定（candDiff=dd9ecd60…），工作副本
  `/private/tmp/t07-ab-drill-final-05/out/ab-compare.json` summary =
  `{tasks:15, parity:12, differs:0, recorded:2, failedA:0, failedB:1}`；逐任务 verdict 与
  FEATURE_PARITY_RESULTS.json 完全一致；D-1（标题投影 null vs id 干）与 D-4
  （archive B-failed + drift-guard 3→2）在 observations 中如实可见。
- 红证据保留：drill-01（A 腿 0 会话——夹具格式对旧栈不可见）、drill-02（B 腿
  rename/pin/todos 全拒——F-T07-2 引出）、drill-03（A 腿 pin 误译失败——连接门缺失实证）
  尾行/关键行逐条核对，红链未删改；probe-01（缺 ⑤a 旧栈崩）在册。
- **F-T07-2 修复代码**：`desktop/src/react/hooks/use-hana-fetch.ts` L60-130
  `translateLegacySessionCall` 扩展（rename/pin/archive/restore/archived-delete/cleanup/
  sweep-orphaned-workspaces/pin-order/todos×3/fresh-compact + memory/authorized-folders
  PATCH）；连接门 L179-183（`connection.serverNodeKind === 'lingxi-service'` 才翻译）+
  函数内 `native:` 定位符守卫（双门，旧腿直连旧端点）——与差异报告描述一致。

## 五、T08 注册核验——成立

- `rust/crates/xtask/src/stage_maps/R08.json`：commands 18 键 / scenarios 16（R08-A01..A16）/
  supplementalLeafScenarios 126；126 叶 basisKind **全部** `required_behavior_pending`；
  9 叶 evidenceRequired 追加「已实测份额（非门禁）」图钉注记（D02 会话管理 8 叶 + D04 1 叶，
  ID 与门 shareSatisfiedLeafIds 9 项逐一相同，图钉指向 r08_t07_ab cases.json 断言键）。
- 注册链：main.rs:72（STAGE_MAPS 含 R08）、runner_identity.rs:63-68/95-96（R08.json +
  R08_INPUTS.json 嵌入 + 清单）；candidate.rs:204-210 工作树已删除 tracked 条目跳过
  （候选=工作树现状语义，含 R08-T08 修复注释）。
- inputs 自检：本人复跑 `r08_stage_inputs.py --check` PASS（digest match 70b75054…，
  与最终门 inputs-check.json 一致，见 §七）。

## 六、A15/A16 产物核验——合格

- a15-isolation.json（dryrun SUITES/r08_a15_isolation）：真实 home 只读摘要（顶层
  名字/类型/字节/mtimes，无内容无密钥）；realHomeStamp epoch v2；预览 env 门控源码事实；
  合成 HOME 使用声明。真实 `~/.lingxi/data-epoch.json` mtime 仍为 2026-09-21（R08 期间未触碰
  的独立佐证）。「开工基线未采集」披露在报告/台账（工件内无该字段，观察 O3）。
- a16-versions.json：HEAD 86840d1、contractVersions 6d869347…、双锚 80a8d2c2/40f42dc1、
  stageMapInputDigest 70b75054…（与 R08_INPUTS 一致）、handoffBoundries 约束——内容合理。
  **无密钥泄露**（仅哈希与版本号）。注意 rustBinary 值见 MINOR M3。

## 七、独立复跑结果（rev-r08final-*，全部 r08_run.py 包裹、全新目录）

| # | 证据目录 | 命令 | exit | 关键行 |
|---|---|---|---|---|
| G1 | `rev-r08final-build-01` | `cargo build --manifest-path rust/Cargo.toml -p lingxi-service` | 0 | Finished dev profile；**0 warning** |
| G2 | `rev-r08final-svcbin-01` | `cargo test … -p lingxi-service --bin lingxi-service` | 0 | `16 passed; 0 failed` |
| G3 | `rev-r08final-adapters-01` | `cargo test … -p lingxi-adapters --lib` | 0 | `178 passed; 0 failed; 13 ignored` |
| G4 | `rev-r08final-registry-01` | `python3 scripts/rust-tauri/r02_registry_consistency.py` | 0 | `PASS: … 28 entries … fingerprints exact` |
| G5 | `rev-r08final-inputs` | `python3 scripts/rust-tauri/r08_stage_inputs.py --check`（证据 rev-r08final-inputs/SUITES/inputs） | 0 | `inputs digest match` |

全部 `sourceBindingsStable=true`、candidateDiffSha256=c99bb5c3…（终审时点树，含 T07 F-T07-2
tracked 改动与 T08 注册；与 T06 期 d5962b50/T07 final 期 dd9ecd60 的演进一致）。adapters 178
= T06 期 177 + F-T07-1 新测试 1，与台账口径吻合。

## 八、最终门快照（进行中，不等待）

- 进程 57977 `xtask verify-stage R08 --evidence artifacts/rust-tauri/R08/verify-r08-final-01`
  在跑（02:17 CST 起）；证据目录增量落盘中。
- 已完成：`r08_inputs_check` **PASS**（inputs-check.json digestMatch=true，
  expected=map=70b75054…）；`r08_t01_a01_a02_ws` 套件回执 exit 0（门侧逐命令判定待其
  checkpoint 后记入日志）。其余 16 命令（含 dryrun 未覆盖的 `r08_a04_no_final_ws`）排队中。
- 参照系（dryrun-03 快照，17 命令旧图）：16 PASS + a05/a06 红（旧脚本缺 `--out`，已修）；
  overall=**FAIL**、leafCoverage fail=9/blocked=117——**非 PASS 是 117 pending 叶的真实
  表达，与报告 §四宣称一致**；最终门按现行 18 命令图重跑，a05/a06 应转绿，overall 预期
  仍非 PASS（117 叶门规不可判完成）。最终结果以 verify-stage-result.json 为准，主会话登记。

## 九、与报告/交接宣称不符之处（全部为措辞/账面级，无虚报）

| 级 | 项 | 事实 |
|---|---|---|
| MINOR M1 | R08_REPORT §一 T07 条「全量 123 个 T07 域叶中 9 叶…117 叶…」 | 9+117=126（阶段图实际）；「123」与后两数不自洽（按域拆分 D01-D20 无 123 口径：全 126 或 T07 域 114）。各数字单独可溯，句子口径混淆 |
| MINOR M2 | R08_HANDOFF「9 stage_share_satisfied + 117 required_behavior_pending」 | R08.json 实际 126 叶**全部** basisKind=required_behavior_pending；9 叶以 evidenceRequired 文本承载份额图钉（非独立 status/kind）；门将其计为 fail（dryrun fail=9）。报告正文「9 份额绑定」措辞准确，HANDOFF 字段措辞易误读为叶状态 |
| MINOR M3 | R08_HANDOFF `rustBinarySha256=c15e8096…` ≠ dryrun a16-versions.json `rustBinary=16aedf37…` | debug 重链接非字节可复现、两工件生成时点不同；基线权威本就指定为 a16-versions.json（报告 §六），HANDOFF 该字段值已过时 |
| MINOR M4 | candidateDiffSha256 对未跟踪文件盲区 | F-T07-1 修复整体位于未跟踪 `legacy_import/` 目录，`git diff HEAD` 不含未跟踪文件 → probe-02（红）与 probe-03（绿）candidateDiffSha256 同值 d5962b50…；红→绿绑定实际由 sourceBindings（rglob 四 crate .rs，含未跟踪）承载并经本终审对拍确认。引用 candidateDiffSha256 同绑论述时应知晓该盲区 |

观察项（不计级）：
- O1 dryrun-03 含 2 红命令（a05/a06 `--out is required`，旧版 r08_stage_inputs.py）且仅 17
  命令（当时尚无 r08_a04_no_final_ws）；`--out` 修复在 dryrun 中途落地（checkpoint 可见
  stage_inputs.py 变更）。dryrun 红命令未在 PROGRESS-t08.md 逐条根因（台账只承诺对最终门
  逐命令红补记）。最终门跑现行图，不受影响。
- O2 本终审的 cargo build（G1）在最终门运行窗口内重链接了共享
  `rust/target/debug/lingxi-service`（源未变、候选 diff 稳定、同源行为等价；二进制字节因此与
  此前证据不同——与 M3 同因，非源码漂移）。
- O3 a15-isolation.json 工件本身不含「开工基线未采集」字段（披露在报告/台账文本）。
- O4 suiteExitA=1/suiteExitB=1（ab final-05）：vitest 退出码 1 由 B-failed 任务如实传导，
  非静默；三分判定在 ab-compare.json。

## 十、红线自查

- 未修改任何产品代码/脚本/文档（仅新增本评审文件 + PROGRESS-review.md 追加评审记录）；
  未 commit/push；未 checkout/restore/clean；HEAD 全程 86840d170。
- 未 kill/干扰最终门进程；未启动第二个 verify-stage；未跑任何占 47613 端口/单写者锁的演习
  （T06 D1-D3/T07 AB 类均未复跑，引用 T06 评审 8 组复跑与 n2n3/pi-fix 重绑证据）。
- 真实 ~/.lingxi 仅只读摘要核对（stat data-epoch.json mtime）；复跑全部合成环境
  （回执 sourceBindingsStable=true）。

---

终审人：R08 独立终审（rev-r08final-*，5 组复跑 + 只读核对）｜2026-10-24
