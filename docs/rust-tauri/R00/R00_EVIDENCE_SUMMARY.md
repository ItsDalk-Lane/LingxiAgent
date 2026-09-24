# R00 基线证据摘要（R00-T08 交付）

生成：R00-T08 执行者（ZCode），2026-09-24｜当前 HEAD `8b153b1031bbb01204375b08e9caaa891397d7a5`（T08 交付提交，含本文件）｜T08 Task base（历史执行时点）`e0b7be6108c4d5bc873061dee279b7163ca78a41`｜分支 `codex/rust-tauri-migration`｜stage base `7d1a0c6bc28062ff455adcf8f68c3a80b117e90d`

> 增补（阶段修复 STAGE-REPAIR-R00-R1，2026-09-24）：阶段级独立验收 R1 判 FAIL（MUST_FIX F1–F3）后的修复已落地——HANDOFF/账本坐标已重绑真实 T08 提交 `8b153b103`（§1 表末两行），§4 非绿项已按两基点分类（相对 T08 预存 / 相对 stage base 本阶段触发），T03 R3 两条 LOW 后续项已入 HANDOFF unresolved_items（承接 R07-T12）。逐命令证据见 [R00_STAGE_REPAIR_R1.md](R00_STAGE_REPAIR_R1.md)；当前状态 = READY_FOR_STAGE_REREVIEW，阶段放行以总控另派全新独立阶段复验为准。

本文件是 R00 阶段证据的总入口卡片；逐文件哈希与验证细节见 [R00_REPORT.md](R00_REPORT.md)（§5 验证组合 / §10 候选摘要）与 [R00_HANDOFF.json](R00_HANDOFF.json)（artifact_hashes / spot_checks / verification_battery）。

## 1. 固定坐标

| 项 | 值 |
|---|---|
| Stage base | 7d1a0c6bc28062ff455adcf8f68c3a80b117e90d |
| T01–T07 各任务提交 | 16aeb380d / 4b4a1d98f / ffcb85830 / 6f58b9351 / 64c302ec4 / 4401afae2 / e0b7be610（均经独立验收 PASS） |
| T08 候选 | 173 文件；聚合 SHA-256 `d83ae896ce19d1ee5a8c0c8084244c44a32516cee5d589a35ead9fff2a448645`（= `shasum -a 256 artifacts/rust-tauri/R00/T08/CANDIDATE_MANIFEST.txt`；本文件与 R00_REPORT.md 不在其内，自引用规避） |
| T08 交付提交 | `8b153b1031bbb01204375b08e9caaa891397d7a5`（= 当前 HEAD；T08 任务级独立验收 R2 PASS 后由总控提交；候选 173 文件与提交字节 173/173 核验一致，见 [R00_STAGE_REPAIR_R1.md](R00_STAGE_REPAIR_R1.md) 证据 t08-manifest-vs-commit-verify.log） |
| 阶段修复 R1 候选 | HEAD `8b153b103` + 工作区修复改动（HANDOFF/账本/摘要/报告 + 校验器负例 + STAGE_REPAIR_R1 证据）；坐标重绑后 HANDOFF SHA-256 `c925e839c088502ae4e4d2798b9a284b26e30f26fb41ec1f39ced9a440669938`，账本 `96f70219d1df88249e8df073f4dfe211f8562cef20f7e45b1c03064feb5362be` |
| 依赖锁 | package-lock.json `e54a16fe…505ac8b`（R00 未变） |
| 现役协议版本 | PRELOAD_API_VERSION=1 / SERVER_PROTOCOL_VERSION=1 / DATA_EPOCH=1（shared/contract-versions.json；目标契约归 R01） |

## 2. 基线交付物 → 证据 → 复核方式

| 域 | 交付物（docs/rust-tauri/R00/） | 原始证据（artifacts/rust-tauri/R00/） | 一键复核 |
|---|---|---|---|
| T01 基线/隔离 | BASELINE.json、BASELINE_DELTA.md | T01/（探针、前后哈希、targeted tests） | 见 BASELINE.json targeted_tests.command |
| T02 功能清单 | FEATURE_INVENTORY.json、EXCLUSIONS.md、FEATURE_STAGE_ACCEPTANCE.json、ENTRYPOINT_COVERAGE.json | T02 审计 JSON 系 | `--negative-checks` 在 HEAD 报纯 tested_sha 戳 STALE（内容零漂移，证明见 T08/t02-t03-checker-diagnosis.md）；`--write` 为再绑路径 |
| T03 Pi 职责 | PI_REPLACEMENT_MATRIX.json、RUNTIME_DEPENDENCIES.json | T03/（导入图 1.2MB） | 校验器在 HEAD 因冻结图 1664 vs 现树 1677 设计性失败（诊断同上文件）；重生成路径见 T03 报告 |
| T04 入口/存储 | ENTRYPOINTS.json、STORES.json、OWNERSHIP_CURRENT.md | T04/ | `python3 -B docs/rust-tauri/R00/r00_t04_scan.py --validate` → R00_T04_SCAN_OK（exit 0） |
| T05 行为夹具 | tests/migration/fixtures/（FIXTURE_MANIFEST+9 组）、OLD_BEHAVIOR_ORACLE.md | T05/（三 run raw+normalized）+ T08/replay-t08/（强化守卫后复证） | `node scripts/rust-tauri/r00-t05-replay.mjs --out /tmp/任意目录`（勿缺 --out：默认写 T05 冻结目录） |
| T06 性能基线 | R00-T06_PROTOCOL.md；artifacts/…/T06/BASELINE_BENCHMARK.json；docs/rust-tauri/PERFORMANCE_THRESHOLDS.json | T06/raw/（office 链指针见 HANDOFF office_chain_pointer） | 阈值哈希见 HANDOFF artifact_hashes |
| T07 验收账本 | ACCEPTANCE_MAP.json（952 场景/16 结果）、BLOCKERS.md、三工具 | T07/（自测）+ T08/ledger-selftest-r2/ | `python3 -B docs/rust-tauri/R00/r00_t07_validate_ledger.py` → LEDGER_VALID |
| T08 封存交接 | R00_REPORT.md、R00_HANDOFF.json、本文件、r00_t08_{a15_verify_repro,a16_spotcheck}.py | T08/（验证组合原始日志 + 诊断） | `python3 -B docs/rust-tauri/R00/r00_t08_a16_spotcheck.py` |

## 3. 三抽查点（A16，独立验收可直接复跑）

1. 功能：`F-D09-SEMANTIC_EFFECT-SEMANTIC-EFFECT-FILE-HISTORY-FILE-HISTORY-FILES--AF8E24` → `server/routes/file-history.ts:30` → 场景 `R00-T02-LA-AF8E24A2F524` → 任务 R04-T04/R06-T05。
2. 数据链：STORES `session-jsonl` → 6 读写方文件 → 夹具 `multi-turn-basic`（SHA 固定值见 HANDOFF）。
3. 测试：`LINGXI_MIGRATION_BLOCK_NETWORK=1 ./node_modules/.bin/vitest run tests/migration/r00-t05-replay.test.ts tests/migration/r00-a10-old-defect.test.ts tests/migration/network-guard-negative.test.ts` → 预期 exit 0 / 3 文件 / 21 用例。

## 4. 已知非绿项（未修绿未删除；两基点分类见 R00_REPORT §7/§17）

审计封印家族 4 用例（post-verification-audit-seal、round2 R10-03/R10-04、round3 manifest）：两次同条件复现一致（`r00_t08_a15_verify_repro.py` 机器比对 exit 0）；根因=审计白名单不含 R00 增量；**两基点分类（阶段修复 R1 按阶段验收 R1-F2 澄清）**：相对 T08 任务基点为**预存**（自 R00 首个提交 16aeb380d 起即红，T08 未引入）；相对 R00 stage base（7d1a0c6bc）为**本阶段提交触发**（封印脚本在 stage base 隔离副本 exit 0、在 HEAD 隔离副本 exit 1，两基点实证日志见 STAGE_REPAIR_R1/）。处置=留总控按 PROGRESS.md seal 工作流在阶段复验 PASS 后收口，不归 R01；四个 FAIL 全保留，全量测试不得表述为 PASS。其余 1463 文件/14895 用例通过。另有工具级预存两项：T02 盘点检查器 HEAD STALE（纯 tested_sha 戳，内容零漂移已证明）、T03 校验器冻结图 1664 vs 现树 1677（设计性 scope 断言）——诊断见 `T08/t02-t03-checker-diagnosis.md`。
