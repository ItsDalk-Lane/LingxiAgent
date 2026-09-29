# DISPATCH: REVIEWER-R03-T08-R01（独立验收子代理，第 1 轮，一次性）

- TASK_ID: R03-T08（状态机验收与 R04 接口交接）
- ROUND: 1
- TASK_BASE_SHA: dc42a01e37d1110293ec137402fd3239bd5b7d9d
- CANDIDATE: 基线 HEAD + 未提交工作树候选
  - 修改（10）：docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json、rust/crates/lingxi-adapters/tests/{late_result_fencing_property.rs,run_finalize_property.rs}、rust/crates/xtask/src/{main.rs,runner_identity.rs,stage_map.rs,verify.rs,verify/runner_tests.rs}、scripts/rust-tauri/{r02_t04_storage_tx.sh,r02_t08_legacy_entry_regression.sh}
  - 新增（9+）：rust/crates/xtask/src/stage_maps/R03.json、rust/crates/lingxi-service/tests/r03_t08_acceptance_matrix.rs、scripts/rust-tauri/r03_t08_{matrix.sh,a16_seed_mechanism.sh,generate_stage_map.py}、docs/rust-tauri/R03/{R03-T08_REPORT.md,R03_REPORT.md,R03_HANDOFF.json,R03_ACCEPTANCE_LEDGER.json}、artifacts/rust-tauri/R03/T08-E01/、docs/rust-tauri/R03/dispatches/R03-T08_DISPATCH_E01.md
  - 绑定摘要：tracked-diff sha256 前 16 位 7deab269990f9b43
- REVIEW_REPORT_PATH: docs/rust-tauri/R03/R03-T08_REVIEW_R1.md
- 复测临时产物目录（只写这里）：artifacts/rust-tauri/R03/T08-R01-review/
- 派发时间: 2026-09-29

## 执行者自报 FINDING（验收须裁决）

- FINDING-1（未修复，如实登记）：父取消路径 subagent child run 的 durable 行不就地收口（spawn_linked 包装器先丢 drive future），由下一进程启动扫描诚实收口 interrupted_needs_attention；T06 报告该表述在父取消路径不成立；A06 监督层判定成立（矩阵实测+重启闭环）。
- FINDING-2/3：R02 期资产断言相对 R03 设计行为过期（a07 检查器计数 / a14 风暴 200-only），执行者称未改已验收门禁断言；全量 verify-stage R02 为 17/20 绿、3 红（E5 封印族=总控账本项 + FINDING-2/3）。

## 输入路径

- 规格：R03 阶段书全文（T08/A15/A16/§5/§6 阶段放行条件）+ 共同契约 00/01/02/03/04/05/91
- 责任矩阵：docs/rust-tauri/R03/R03_SCOPE_MATRIX.json、R03_TEST_MAP.json
- 执行者报告：docs/rust-tauri/R03/R03-T08_REPORT.md（及 R03_REPORT/HANDOFF/ACCEPTANCE_LEDGER）；证据 artifacts/rust-tauri/R03/T08-E01/
- 前置：T01–T07 报告与审查
- R02：R02_HANDOFF.json、R02_ACCEPTANCE_LEDGER.json、R02-T04_STORAGE_REGISTRY.json、stage_maps/R02.json
