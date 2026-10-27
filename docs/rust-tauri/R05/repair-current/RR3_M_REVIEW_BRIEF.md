# RR3 F54（M包）全新独立验收（M-REVIEW-01）

你是全新空历史独立审查者，未参与 M 实施及此前任何轮。全文读 RR1/RR2 MASTER、RR3_BRIEF/REVIEW_BRIEF、RR3_F54_BRIEF.md、最新 RR3_ISSUE_MATRIX/PROGRESS/HANDOFF、FINAL-03/STAGE_REVIEW.md §四失败项2、M-01/REPORT.md 及其证据（46叶逐项表、verify-R04-standalone/ attempt-2、attempt1-concurrent-writes/、launch log）、改动文件三方（rust/crates/xtask/src/stage_maps/R04.json、scripts/rust-tauri/r04_t08_generate_stage_map.py、rust/crates/xtask/src/stage_map.rs 的 git diff HEAD）。只验不修，不派代理；唯一新输出 artifacts/rust-tauri/R05/RR3/M-REVIEW-01/。仓库其余只读、无 Git 写、无系统变更。

亲验至少：
1. 分类正确性抽查≥8叶（含最复杂与最简单）：每叶 full_original_behavior 的每条钉住断言可溯源到 r04_tool_matrix 真实案例（案例名在真实56案例集中存在且语义匹配）、无虚构证据路径、R00 登记（r00ExecutionStageIds/12字段）未被改动；9 个保留 share 叶确实 R04+R06 双阶段。
2. 校验器/R00/pins/cids/R05叶表零改动：git diff HEAD 核对 verify.rs、R00 叶登记文件、docs/rust-tauri/R05/ 各 TSV、R05.json 无变化；stage_map.rs 的改动仅镜像测试（无生产语义变化）。
3. 红绿亲证（隔离副本）：旧分类图重现 ≥3 叶 FAIL 且理由与 FINAL-03 逐字同；单叶还原红点名；新图 0 FAIL。F25 校验器单测绿。
4. standalone 结果亲核：亲读 verify-R04-standalone attempt-2 的 verify-stage-result.json（overall/命令/绑定/checkpoint/叶表/场景逐项）并复核其 testedSha/工作树绑定与当前树一致；attempt-1 的并发干扰归因抽查（变化文件确为 L-01/L-REVIEW-01 路径）。
5. 你自己的独立 standalone 重跑一次（新证据目录 artifacts/rust-tauri/R05/RR3/M-REVIEW-01/verify-R04-review，start_new_session 脱离宿主，约55分钟）overall=PASS、叶表0 FAIL。运行期间总控与其他代理保证静默（本轮唯一写者是你）。
6. 方法自控：篡改一处叶分类/断言记录应被你的检查发现。
交付 M-REVIEW-01/REVIEW.md：逐项 PASS/FAIL+证据+mustFix 或明确无；真实命令/exit/UTC。PASS 无 mustFix 才关闭 F54，其后 FINAL-04 换全新阶段审查者。完成停写。
