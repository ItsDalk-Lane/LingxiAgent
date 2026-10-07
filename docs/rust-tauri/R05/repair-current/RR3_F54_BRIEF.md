# RR3 F54（M包）：R04 46 独占叶 stage_share_satisfied 分类修复

你是全新空历史修复实施者，未参与此前任何轮。全文读 RR1/RR2 MASTER、RR3_BRIEF/REVIEW_BRIEF、最新 RR3_ISSUE_MATRIX.json（F54 行）/PROGRESS/HANDOFF、FINAL-03/STAGE_REVIEW.md §四失败项2（46叶确定性失败面、与RR2/FINAL-01逐字一致）、RR1_MASTER F25 节（同族缺陷的修复范式）。

## 根因面（你先亲核再修）

R05 RR1 的 F25 修复在 rust/crates/xtask/src/verify.rs:919-944 引入规则：basis_kind=stage_share_satisfied 且 r00_execution_stage_ids 只含本阶段且 deferredToStages=[] 的叶=无人承接的剩余份额→FAIL（"pin the leaf's original assertions as full_original_behavior instead"）。R04 在该规则存在之前已重验收（c549ff654），其叶表（stage_map.rs 内 R04 数据，R04_LEAF_COUNTS=55+69）中 46 个 R04 独占叶仍是旧分类 stage_share_satisfied/deferredToStages=[]，主树正式链 R04 层因此确定性 FAIL（124 叶：9 PASS/46 FAIL/69 DEFERRED）。

## 任务（唯一范围：R04 叶分类数据修复）

1. 亲核：从 FINAL-03/verify-R05 的 R04 层 verify-stage-result.json 提取 46 个 FAIL 叶的完整清单与各自 assertion/deferred case 数据；读 stage_map.rs 中 R04 叶表构建处与 verify.rs 校验语义。
2. 逐叶修复分类：这些叶 r00ExecutionStageIds=["R04"]（独占）。正确路径=改为 full_original_behavior 并逐例钉住原断言（从 R04 已验收证据取真实 per-case 事实：R04 历史验收材料 artifacts/rust-tauri/R04/、docs/rust-tauri/R04/R04_TEST_MAP.json、RR1 R04 重验收证据、R04/repair-current/G05-E01_REPORT.md 等）。仅当某叶在 R00 原始登记确有更晚阶段承接时才可用合法 deferredToStages（不得为绕过校验虚构承接）。不改 verify.rs 校验器、不放宽规则、不改 R00 叶登记（只读）。
3. 若分类数据在 docs/rust-tauri/R04/ 下而非 rust/：属白名单外必要邻接，已在矩阵登记授权（根因=FINAL-03失败项2，所有者=M包）；最小改动并逐文件记录。
4. 自检：(a) xtask 相关测试全绿（绝对cargo、--locked）；(b) 隔离副本红/绿：把任一修复叶改回 stage_share_satisfied/[] 必须重现原 FAIL（证明校验器仍有效）、还原后绿；(c) 主树新证据目录跑 standalone `verify-stage R04 --evidence artifacts/rust-tauri/R05/RR3/M-01/verify-R04-standalone`（约51分钟，用 start_new_session 脱离宿主会话防误杀）R04 层叶表 0 FAIL、overall 除已知无关项外如实报告；(d) fmt/clippy 绿。
5. 红线：不改 R05 叶表/校验器语义、不改 pins/cids TSV、不虚构断言或证据路径（每个钉住的断言必须有真实 R04 证据可溯）、不改生产 crate 源码；无 Git 写；不派子代理。
6. 产物：artifacts/rust-tauri/R05/RR3/M-01/REPORT.md（46叶逐项：旧分类→新分类→证据指针；全部命令/exit/UTC）、standalone R04 结果、红绿对照。完成停写，交总控另派全新独立审查（M-REVIEW-01）。
