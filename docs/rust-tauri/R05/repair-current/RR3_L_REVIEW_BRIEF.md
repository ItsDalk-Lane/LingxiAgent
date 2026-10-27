# RR3 F53（L包）全新独立验收（L-REVIEW-01）

你是全新空历史独立审查者，未参与 L 实施及此前任何轮。全文读 RR1/RR2 MASTER、RR3_BRIEF/REVIEW_BRIEF、RR3_F53_BRIEF.md、最新 RR3_ISSUE_MATRIX/PROGRESS/HANDOFF、FINAL-03/STAGE_REVIEW.md §四失败项1（flake 证据链）、L-01/REPORT.md 及其 evidence 日志、rust/crates/lingxi-service/tests/r04_t08_tool_matrix.rs 当前版本与 git diff HEAD 中该文件的改动。只验不修，不派代理；唯一新输出 artifacts/rust-tauri/R05/RR3/L-REVIEW-01/。仓库其余只读、无 Git 写、无系统变更。

亲验至少：
1. 根因模型独立核：读该测试与 PTY helper，独立验证"双份可观察拷贝（ECHO+ICANON 回显+cat 回环）"模型与修复后屏障退出时光标越过旧 marker 的论证；确认与 FINAL-03 的 6跑5绿1红 分布相容（红仅满载深层）。
2. diff 审查：改动仅在该测试文件；断言语义一字未变（terminal-snapshot-current-transcript 期望仍为"只交付新输出"），无 sleep 掩盖/跳过/弱化/重试循环吞失败。
3. 亲跑：定向 ≥15 次全绿（记录每次 exit）；负载下 ≥5 次绿；完整 r04_t08_tool_matrix 套件一次全绿；clippy（workspace all-targets locked -D warnings）对该 crate 绿。注意：M 包（F54）正在并行修改 rust/crates/xtask/src/stage_map.rs——workspace fmt/clippy 若报 stage_map.rs 格式差异，那不是 L 的评审对象，如实单列不判 FAIL；cargo 文件锁等待属正常。
4. 检测力控制（隔离副本）：你自己设计一个破坏"只交付新输出"契约的变异（可与 L 的冻结光标法不同），断言必红且红点在该断言；还原绿。证明加固未削弱。
5. 方法自控：篡改你自己的比对基线应被发现。
交付 L-REVIEW-01/REVIEW.md：逐项 PASS/FAIL+证据+mustFix 或明确无；真实命令/exit/UTC。PASS 无 mustFix 才关闭 F53。完成停写。
