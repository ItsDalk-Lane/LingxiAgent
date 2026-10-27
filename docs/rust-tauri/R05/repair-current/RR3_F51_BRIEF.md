# RR3 F51 全新修复轮：迁出嵌套 .git 证据夹具，恢复主树正式 verify-stage 可执行性

你是全新空历史修复实施者，未参与 RR3 任何实施/审查/终审轮。全文读取 RR1_MASTER_PROMPT_2026-10-04.md、RR2_MASTER_PROMPT_2026-10-06.md、RR3_BRIEF.md、RR3_REVIEW_BRIEF.md、最新 RR3_ISSUE_MATRIX.json（F51 行）、RR3_PROGRESS.md、RR3_HANDOFF.md、RR3_FINAL_BRIEF.md，以及根因证据 `artifacts/rust-tauri/R05/RR3/FINAL-01/STAGE_REVIEW.md`（§四机制与归因）与 `FINAL-01/command-records/frozen-inputs.json` 的 `binder_observation.all_directory_entries`（56 条目清单）。

## 根因（已由 FINAL-01 确定性复现）

主树 RR3 证据目录遗留 56 个含嵌套 `.git` 的未跟踪夹具目录（A-02、A-REVIEW-01/02、I-01、I-REVIEW-01、J-01、J-02、J-REVIEW-01/02 等）。`git ls-files --cached --others --exclude-standard` 对含嵌套 .git 的未跟踪目录只输出目录单条目（尾斜杠），绑定器（rust/crates/xtask/src/candidate.rs）按文件哈希 → `was replaced by a non-file` fail-closed → 正式 verify-stage R05 入口 exit 1、证据根未创建。绑定器行为正确（不得修改）；修复=证据卫生侧迁出。

## 任务（唯一范围）

1. **权威再枚举**：以 `git ls-files --cached --others --exclude-standard -z` 亲自重枚举全部非文件（目录）条目，与 FINAL-01 的 56 条目清单交叉核对；若有差异如实记录（多则一并处理、少则记录已变化事实）。
2. **逐项迁出**：每个嵌套 .git 夹具目录用 `mv`（字节保持，不用复制重写）迁到仓库外专用根 `/Users/study_superior/Desktop/Code/LingxiAgent-RR3-localonly-fixtures/<原相对路径的斜杠编码>/`（例：`artifacts/rust-tauri/R05/RR3/A-02/copy-delete-rename` → `…/LingxiAgent-RR3-localonly-fixtures/artifacts_rust-tauri_R05_RR3_A-02_copy-delete-rename`）。目录不存在则创建；同名冲突即停并上报。
3. **保留 hash 回执**：迁出前后逐目录计算文件数/字节数/tree 摘要（可用 `find … -type f | wc -l`、`du -sk`、逐文件 SHA256 清单或 `git -C <dir> rev-parse` 类稳定摘要，方法自定但须前后同法可证相等），全部写入 `artifacts/rust-tauri/R05/RR3/F51-01/RELOCATION-RECEIPT.json`（含每条目 old→new、前后摘要、UTC、命令与 exit）。
4. **原位标记**：在每个被迁出目录的原父目录写一个小型 `RELOCATED-F51.json`（列出被迁走的子目录名与新位置、指向回执），不改动该目录其他历史文件。
5. **验证**：(a) `git ls-files --cached --others --exclude-standard | grep '/$'` 输出为空；(b) `git status --porcelain` 的 tracked 修改集与迁出前完全一致（仅减少 `??` 目录条目、新增 F51-01/RELOCATED 回执文件）；(c) 抽样至少 3 个迁出目录在新位置字节可读且摘要相等；(d) 生产不引用验证：`grep -r` 确认 scripts/、rust/、docs/ 现行文件无引用被迁路径（FINAL-01 已预检为空，你复核）；(e) A 包绑定器回归不退化：`/Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p xtask candidate`（或其等价测试目标名，先 `-- --list` 确认）全绿。
6. **红线**：不修改 rust/、scripts/、docs/ 任何现行文件；不改 .gitignore；不排除整个 artifacts/；不删除任何字节（只迁移）；不做 Git 写操作（add/commit/push/branch/tag 全禁）；不派子代理；不动 /private/tmp 的 rr3-* 历史材料；不触用户内容。HOME 若非 /Users/study_superior 先 export（cargo 绝对路径）。
7. **产物**：`artifacts/rust-tauri/R05/RR3/F51-01/REPORT.md`（全部命令/exit/UTC/前后清单/验证结果/任何偏差）、RELOCATION-RECEIPT.json、必要的自检脚本与输出。完成停写，交总控另派全新 F51 独立审查者；其后 FINAL-02 换全新阶段审查者。
