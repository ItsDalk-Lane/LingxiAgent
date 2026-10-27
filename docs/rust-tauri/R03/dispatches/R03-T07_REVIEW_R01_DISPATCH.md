# DISPATCH: REVIEWER-R03-T07-R01（独立验收子代理，第 1 轮，一次性）

- TASK_ID: R03-T07（故障恢复与服务退出策略）
- ROUND: 1
- TASK_BASE_SHA: 0c138456bbe791f21621f5a24eb47bc1948f7585
- CANDIDATE: 基线 HEAD + 未提交工作树候选
  - 修改（11）：docs/rust-tauri/R02/SERVICE_START_AND_SHUTDOWN.md、rust/crates/{lingxi-adapters/src/storage/{mod.rs,run_store.rs}, lingxi-kernel/src/lib.rs, lingxi-service/src/{lib.rs,main.rs,sessions.rs,shutdown.rs}, lingxi-service/tests/{execute_concurrency.rs,invocation_journal.rs,shutdown_coordinator.rs}}
  - 新增：rust/crates/lingxi-kernel/src/recovery.rs、rust/crates/lingxi-service/src/recovery.rs、rust/crates/lingxi-adapters/tests/active_run_listing.rs、rust/crates/lingxi-service/tests/{exit_race_rejections.rs,recovery_crash_points.rs,recovery_startup_scan.rs,recovery_support/}、docs/rust-tauri/R03/R03-T07_REPORT.md、artifacts/rust-tauri/R03/T07-E01/、docs/rust-tauri/R03/dispatches/R03-T07_DISPATCH_E01.md
  - 绑定摘要：tracked-diff sha256 前 16 位 0e0b2144c141977b；status 集 07f138f77af444a8
- REVIEW_REPORT_PATH: docs/rust-tauri/R03/R03-T07_REVIEW_R1.md
- 复测临时产物目录（只写这里）：artifacts/rust-tauri/R03/T07-R01-review/
- 派发时间: 2026-09-29

## 输入路径

- 规格：R03 阶段书 R03-T07/R03-A13/R03-A14 + 共同契约 00/01/02/03/04/05/91
- 责任矩阵：docs/rust-tauri/R03/R03_SCOPE_MATRIX.json、R03_TEST_MAP.json
- 执行者报告：docs/rust-tauri/R03/R03-T07_REPORT.md；证据 artifacts/rust-tauri/R03/T07-E01/
- 前置：T01–T06 报告与审查（注意 T05 报告 §10.4 预登的重启断言重验义务——T07 对 T05 A09 与 R02-A07 重启断言的收紧）
- R02：R02_HANDOFF.json、SERVICE_START_AND_SHUTDOWN.md（本次被修改，diff 审读）
