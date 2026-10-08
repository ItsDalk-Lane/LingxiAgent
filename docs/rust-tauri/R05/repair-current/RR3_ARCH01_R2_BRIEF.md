# RR3 ARCH-01-R2：嵌套 .gitignore 显形的 439 旧快照残留收尾

你是全新空历史实施者。背景读 ARCH-01/REPORT.md 偏差1：`A-REVIEW-01/snapshot/` 自带的嵌套 .gitignore（本身属已批准删除的 LOCAL_ONLY）删除后，439 个旧快照文件（mtime 2026-09-12~15，R01 时代）从未被 `git ls-files --others --exclude-standard` 枚举过，现按根规则显形为未跟踪。它们与已批准删除的隔离快照同类，用户已批准清理该类。唯一新输出目录 `artifacts/rust-tauri/R05/RR3/ARCH-01-R2/`。

## 任务

1. **精确枚举**：`git -c core.quotepath=off ls-files --others --exclude-standard -z -uall` 全量未跟踪，减去 ARCH-01 输出（ARCH-01/ 下 12+文件）、39 个 RELOCATED-ARCH01.json、RR3_ARCH01_BRIEF.md 与本 brief、ARCH-01-R2 自身 → 得残集 R（预期≈439）。逐项确认全部位于旧快照/隔离类目录（A-REVIEW-01/snapshot 等）——若出现任何生产/现行 docs/tracked 相关路径，停止该项并记录。
2. **先登记后删除**（这些从未入分类，删除前必须建档）：逐项实读记录 path/bytes/SHA256/mtime → `mini-cleanup-receipt.json`；然后删除。100 个被根 .gitignore 隐藏的文件（.mimosa/pycache/reviews 规则等，check-ignore 亲验过）**不属本任务**，不碰。
3. **boundary-update-r2.json**：R 集标 historically_removed，指向 mini-cleanup-receipt。
4. **红线**：只处理 R 集；Git 只读；不碰 tracked/已提交/.gitignore/生产/现行 docs；不碰 LingxiAgent-RR3-localonly-* 外置目录与 .mimosa 等被忽略文件；HOME 修正同前。
5. **最终核验**：`git status --porcelain -uall` 余量 = ARCH-01/ARCH-01-R2 输出 + 39 标记 + 两个 brief + 被根 .gitignore 隐藏项（默认 status 不显示）；tracked 哈希零变化证明。
6. 产物：`ARCH-01-R2/REPORT.md` + `mini-cleanup-receipt.json` + `boundary-update-r2.json` + 命令日志。完成停写。

返回（≤15行中文）：R 集实数与构成、建档/删除计数、全部位于快照类确认、最终 status 余量构成、tracked 零变化、产物路径。
