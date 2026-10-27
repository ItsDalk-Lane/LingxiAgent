# RR3 F51 全新独立验收（F51-REVIEW-01）

你是全新空历史独立审查者，未参与 F51 实施及此前任何轮。全文读 RR1/RR2 MASTER、RR3_BRIEF/REVIEW_BRIEF、RR3_F51_BRIEF.md、最新 RR3_ISSUE_MATRIX/PROGRESS/HANDOFF、FINAL-01/STAGE_REVIEW.md §四（根因）与 F51-01 全部产物（REPORT、RELOCATION-RECEIPT.json、checkpoint、verify 输出）。只验不修，不派代理；唯一新输出 artifacts/rust-tauri/R05/RR3/F51-REVIEW-01/。仓库其余只读、无 Git 写、无系统变更、不动迁出目的地内容（只读核对）。

亲验至少：
1. 权威枚举复核：`git ls-files --cached --others --exclude-standard` 无目录条目；与 F51 回执 56 条目一一对应。
2. 回执抽查≥6 目录（含最大 5.1GB 项与 40--repo 短名偏差项）：新位置逐文件 SHA256 或等价 tree 摘要与回执相等、字节数/文件数一致、嵌套 .git 存在。
3. tracked 零变化：git status/diff 与 FINAL-01 §二 所记 25 M + overlay 一致，index 哈希未被 F51 改写（用 git --no-optional-locks 读）。
4. verify-c 事件独立评估：读 F51-01/verify-c-incident.json，核对其影响圈定（全目的地 find 唯一）、主库语义不受影响的证据，判断该偏差是否损害回执可信度，给明确结论。
5. 红线核验：.gitignore 未改、rust/scripts/docs 现行文件零改动（git diff 对比）、无 Git 写操作痕迹（HEAD/index/refs 与 FINAL-01 记录一致）。
6. 绑定器行为正反控制（隔离副本内，不触主树）：构造一个含嵌套 .git 的未跟踪目录于隔离 git 仓库，确认同样的枚举/拒绝形态与 FINAL-01 一致（证明根因模型正确）；迁出后主树枚举清零即解除（已在1证明）。
7. 正反控制你自己的检查方法（如故意改一个摘要应被发现）。

交付 F51-REVIEW-01/REVIEW.md：逐项 PASS/FAIL + 证据 + mustFix 列表或明确无；真实命令/exit/UTC。PASS 无 mustFix 才关闭 F51，其后 FINAL-02 换全新阶段审查者。完成停写。
