# DISPATCH: REVIEWER-R03-T06-R01（独立验收子代理，第 1 轮，一次性）

- TASK_ID: R03-T06（子代理、后台运行与权限继承）
- ROUND: 1
- TASK_BASE_SHA: 6465395cf8e84121ea55b1481afc5711a4d707e8
- CANDIDATE: 基线 HEAD + 未提交工作树候选
  - 修改（18）：rust/crates/{lingxi-adapters/src/storage/{migrations.rs,run_store.rs}, lingxi-kernel/src/{lib.rs,ports.rs}, lingxi-service/src/{cancel.rs,lib.rs,main.rs,runs.rs,session_supervisor.rs,sessions.rs,shutdown.rs,task_supervisor.rs}, lingxi-service/tests/{cancellation_tree.rs,invocation_journal.rs,late_result_fence.rs,run_lifecycle.rs,session_serialization.rs,shutdown_coordinator.rs}}
  - 新增：rust/crates/lingxi-kernel/src/subagent.rs、rust/crates/lingxi-service/src/{subagents.rs,background.rs}、rust/crates/lingxi-adapters/tests/run_lineage_store.rs、（另有 rust/crates/lingxi-service/tests/ 子代理测试，见 status 集）、docs/rust-tauri/R03/R03-T06_REPORT.md、artifacts/rust-tauri/R03/T06-E01/、docs/rust-tauri/R03/dispatches/R03-T06_DISPATCH_E01.md
  - 绑定摘要：tracked-diff sha256 前 16 位 27010f53cfb3dce8；status 集 f2b53dcb14640a08
- REVIEW_REPORT_PATH: docs/rust-tauri/R03/R03-T06_REVIEW_R1.md
- 复测临时产物目录（只写这里）：artifacts/rust-tauri/R03/T06-R01-review/
- 派发时间: 2026-09-29

## 输入路径

- 规格：R03 阶段书 R03-T06/R03-A11/R03-A12 + 共同契约 00/01/02/03/04/05/91
- 责任矩阵：docs/rust-tauri/R03/R03_SCOPE_MATRIX.json、R03_TEST_MAP.json
- 执行者报告：docs/rust-tauri/R03/R03-T06_REPORT.md；证据 artifacts/rust-tauri/R03/T06-E01/
- 前置：T01–T05 报告与审查（T03 R1-D1 修复映射在 T06 报告 §7）
- R02：R02_HANDOFF.json
