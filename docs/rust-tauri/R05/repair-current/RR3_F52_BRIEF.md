# RR3 F52 全新修复轮：迁出唯一 symlink 夹具，绑定面清零

你是全新空历史修复实施者，未参与任何前轮。全文读 RR1/RR2 MASTER、RR3_BRIEF/REVIEW_BRIEF、RR3_F51_BRIEF.md（同款纪律蓝本）、FINAL-02/STAGE_REVIEW.md（根因与权威枚举69,075条分类）、最新 RR3_ISSUE_MATRIX/PROGRESS/HANDOFF、F51-01 回执格式与 RELOCATED 标记格式。

## 任务（唯一范围）

1. **迁出** `artifacts/rust-tauri/R05/RR3/A-REVIEW-02/independent-validator-bin/` 整目录（含 python3 symlink 及其余内容）到 `/Users/study_superior/Desktop/Code/LingxiAgent-RR3-localonly-fixtures/artifacts_rust_tauri_R05_RR3_A-REVIEW-02_independent-validator-bin/`。mv 字节保持；前后逐文件摘要、symlink 以 readlink 目标串+两端 lstat 记录；回执格式沿 F51-01/RELOCATION-RECEIPT.json，写入 `artifacts/rust-tauri/R05/RR3/F52-01/RELOCATION-RECEIPT-F52.json`。
2. **原位标记**：在 A-REVIEW-02/ 写 `RELOCATED-F52.json`（沿用 F51 的 RELOCATED 标记格式：被迁子目录、新位置、指向回执、UTC）。
3. **绑定面全量重分类**（F51 只查目录条目漏了 symlink 的教训，本轮必须穷尽形态）：以 `rust/crates/xtask/src/candidate.rs` 候选枚举同语义（`git ls-files --cached --others --exclude-standard -z`）取全部条目，逐条 lstat 分类并断言：100% 普通文件、零目录条目、零 symlink（含祖先路径链接/irregular/missing 均零）；全部分类证据（计数、分类脚本、输出）写入 F52-01/。
4. **验证**：(a) `git status --porcelain` 的 tracked 修改集与迁出前完全一致（用 `git --no-optional-locks` 读）；(b) 新位置抽样字节/链接目标相等；(c) `/Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p xtask candidate` 全绿（HOME 若非 /Users/study_superior 先 export 并记录）。
5. **红线**：不改 rust/、scripts/、docs/ 现行文件；不改 .gitignore；不改绑定器；零删除（只迁移）；无 Git 写操作（add/commit/push/branch/tag 全禁）；不派子代理；外置目的地只写本次条目不动 F51 既有内容。
6. **产物**：`F52-01/REPORT.md`（全部命令/exit/UTC/前后清单/验证/偏差）+ RELOCATION-RECEIPT-F52.json + 全量分类输出。完成停写，交总控另派全新 F52 独立审查者；其后 FINAL-03 换全新阶段审查者。
