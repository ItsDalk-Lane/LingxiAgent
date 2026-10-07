# RR3 F52 全新独立验收（F52-REVIEW-01）

你是全新空历史独立审查者，未参与 F52 实施及此前任何轮。全文读 RR1/RR2 MASTER、RR3_BRIEF/REVIEW_BRIEF、RR3_F52_BRIEF.md、FINAL-02/STAGE_REVIEW.md（根因与权威枚举69075条）、最新 RR3_ISSUE_MATRIX/PROGRESS/HANDOFF、F52-01 全部产物（REPORT、RELOCATION-RECEIPT-F52.json、classify_binder_surface.py、三份分类 JSON、前后摘要与 git 快照）、F51-REVIEW-01/REVIEW.md（格式参照，非结论）。只验不修，不派代理；唯一新输出 artifacts/rust-tauri/R05/RR3/F52-REVIEW-01/。仓库其余只读（git 用 --no-optional-locks 读）、迁出目的地只读、无 Git 写、无系统变更。

亲验至少：
1. 绑定面独立复检：不用实施者脚本而用你自己的方法（如 find/git ls-files+ls -l 组合或自写小脚本）独立枚举全部 git ls-files --cached --others --exclude-standard 条目并分类，断言 100% 普通文件、零目录、零 symlink、零祖先链接/irregular/missing；与实施者三跑计数交叉。
2. 迁出回执核对：新位置两文件（git 脚本、python3 链接）inode 无关性下以 sha256+readlink 目标串+lstat 字段与回执逐项相等；原位 RELOCATED-F52.json 存在且指向正确。
3. tracked 零变化：diff HEAD/index 哈希与 F51-REVIEW-01 所记基线（8c23d0d9…/3016f7ae…）一致，无新增 tracked 改动。
4. 红线：.gitignore 与 HEAD blob 同、rust/scripts/docs 零新改动、F51 的 56 目的地与 41 标记零触碰、reflog 无新条目。
5. 隔离正反控制：隔离 git 仓内构造 symlink 未跟踪条目 → 绑定器/同语义枚举重现拒绝形态；构造普通文件 → 通过；证明根因模型与解除条件。
6. 方法自控：篡改一处摘要/分类应被你的比对发现。
交付 F52-REVIEW-01/REVIEW.md：逐项 PASS/FAIL+证据+mustFix 或明确无；真实命令/exit/UTC。PASS 无 mustFix 才关闭 F52，其后 FINAL-03 换全新阶段审查者。完成停写。
