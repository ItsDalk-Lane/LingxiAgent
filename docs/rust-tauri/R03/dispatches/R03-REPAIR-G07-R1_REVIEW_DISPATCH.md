# R03 对抗性修复派单｜G07-R1（独立 Reviewer，第 1 轮）

派单时间：2026-09-30。你是一次性独立对抗性 Reviewer：REVIEWER-REPAIR-R03-G07-R1，未参与 G07 候选的实现或修复，也未参与 G01–G06。只验 G07（F08，4 个 C-ID：R03-FIX-F08-C01..C04）。

## 0. 候选与边界

- 候选：分支 `codex/rust-tauri-migration`，HEAD `8a6303bcd` + 未提交工作树：xtask 阶段图扩展（`stage_maps/R03.json` +repair_suites 命令+R03-RP01 场景；`stage_map.rs` +5 钉图测试；`verify/runner_tests.rs` 16 场景集钉更新 16+RP01）、生成器 `r03_t08_generate_stage_map.py` 同步、新脚本 `r03_g07_repair_suites.sh`（9 套件精确计数钉）与 `r03_g07_gate_negative_tests.sh`（门禁负向测试）、报告/交接/账本更正（R03_REPORT/R03_HANDOFF/R03_ACCEPTANCE_LEDGER）、轮次交付五件套（repair-current/ 下）。总控账本（R03_FIX_ISSUES.json/R03_FIX_COMMIT_RECEIPTS.json）不在审查范围。
- 执行者证据：`artifacts/rust-tauri/R03/repair-current/G07-E01/`；报告 `docs/rust-tauri/R03/repair-current/G07-E01_REPORT.md`。
- 你的复测产物只写 `artifacts/rust-tauri/R03/repair-current/G07-R1/`，报告写 `docs/rust-tauri/R03/repair-current/G07-R1_REVIEW.md`。不得修改产品/测试/配置/门禁/账本/执行者证据；不得 commit/push。

## 1. 必读

修复清单 MD F08 节 + 总控规程 §10/§12；验收清单 JSON F08 4 个 case；05 验收协议 §3（verify-stage 契约）；G07-E01_REPORT + 轮次五件套 + 执行者引用的账本状态（G01–G06 三层记录）；R03.json/stage_map.rs/verify 相关源码 diff；R03_REPORT/R03_HANDOFF/R03_ACCEPTANCE_LEDGER 的本轮 diff（历史保留核对）。

## 2. 独立核查要求

1. **C01 矩阵完整性**：`R03_FIX_ACCEPTANCE_RESULTS.json` fIdMatrix 逐项核对——34 个 C-ID 全部映射到真实测试用例名（抽查至少 8 项，含每个 F-ID 至少 1 项）；红基线证据路径真实存在且内容与声称的失败形态一致；三层状态与账本一致。
2. **门禁接入真实性（C01/C02）**：R03.json 新增 repair_suites 命令真实调用 9 套件且精确计数（读脚本+重跑一次）；R03-RP01 场景钉图测试存在且会因删钉而红；16 原 A-ID/48 叶/7 条 R02 定向链零破坏（对比 R03.json 原有结构——可用 `git show 8a6303bcd:rust/crates/xtask/src/stage_maps/R03.json` 对照）。
3. **负向测试独立重跑（C02）**：真实执行 `r03_g07_gate_negative_tests.sh`（或其在 /tmp 隔离副本的等价形态），确认 7 类缺口（删叶/删命令/删场景钉/0 匹配过滤器/缺证据/陈旧根/中途改输入）均非零退出且点名缺口；**专项审查执行者披露的 N2**：负向驱动器断言首跑过窄后修正并按首跑证据事后复评——核实归档的首跑证据真实显示门禁本体当时已正确拒绝、修正没有放宽门禁本身。
4. **完整门禁独立重跑**：`~/.cargo/bin/cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R03 --evidence artifacts/rust-tauri/R03/repair-current/G07-R1/verify-stage-r03`（你的全新证据根）——期望 overall PASS、15/15 命令、17/17 场景、48 叶=17 pass+31 deferred、candidateSourceBinding stable、testedShaAtEnd=8a6303bcd+工作树。fmt/clippy 真实复跑。
5. **R02 回归归因复核**：7 条定向链绿；全量 verify-stage R02 唯一红 a16 的归因——3 个 SEAL_FAMILY 文件与 GOV-01/02 一致（比对注册分类器）；非族红 `tests/artifact-core-ustar.test.ts` ENOTEMPTY：核实修复轮零触碰该面（git log/diff 范围核对）+ 你自己隔离重跑一次该测试文件确认绿。
6. **C03 绑定**：执行者证据根中 candidateSourceBinding 全字段（digest/checkpoint/testedShaAtEnd/excluded）；受控演示 c03a/c03b 的记录真实（改 lib.rs→digest 漂移→overall FAIL）。
7. **报告更正与历史保留（C04 素材）**：R03_REPORT 2026-09-29 STAGE PASS 原文保留、REOPENED 节如实更新；HANDOFF FINDING-1 标注 G01 关闭引用真实提交 520bb75b9；interfaces 更新与 G02–G06 实际语义一致；R03_ACCEPTANCE_LEDGER 原 16 场景条目未动；五件套+INDEX.md 齐备且引用真实文件。
8. 允许以独立证据判 NOT_A_DEFECT；无依据不得弱化；无问题就 PASS；阶段最终裁决不归你（F08-C04 的全新阶段 Reviewer 另派），你只裁 G07 工作单本身。

## 3. 环境

一律 `~/.cargo/bin/cargo`（1.98.1；Homebrew 1.93.0 禁用）；`--locked`；隔离 /tmp 数据根。

## 4. 输出

```text
VERDICT: PASS / FAIL / BLOCKED
候选摘要
逐 C-ID：核查与独立复测命令/退出码/证据
finding（如有） / 误判反证（如有） / 需标 STALE（如有）
审查范围声明
```
