# RR3 ARCH-01-R2 REPORT — 嵌套 .gitignore 显形的 439 旧快照残留收尾

- 执行（全新空历史实施者）：brief = `docs/rust-tauri/R05/repair-current/RR3_ARCH01_R2_BRIEF.md`
- 日期：2026-10-07（UTC 时间戳见各 JSON）；分支 `codex/rust-tauri-migration`；HEAD `b4b0094d37d4de2b1914d8c8de5e89cdf79312a6`（全程未变）
- 环境：HOME=`/Users/study_superior`（正确，未修正）；仓库根 `/Users/study_superior/Desktop/Code/LingxiAgent`

## 结论

R 集 = **439** 个（与 brief 预期一致），**先全量建档、后删除**，全部属旧快照/隔离类目录，tracked 零变化。

## 1. 精确枚举与对账

- 枚举命令：`git -c core.quotepath=off ls-files --others --exclude-standard -z`（`ls-files` 无 `-uall` 开关；`--others` 本就逐文件展开、无目录折叠，计数与 `status -uall` 一致）
- 对账：494 = 12（ARCH-01/ 产物）+ 2（ARCH-01-R2 自身产物，枚举时已存在）+ 39（RELOCATED-ARCH01.json 标记）+ 2（RR3_ARCH01_BRIEF.md + RR3_ARCH01_R2_BRIEF.md）+ **439（残集 R）**
- R 集清单存档：`R-set.txt`（439 行）

## 2. 快照类确认（0 例外）

- 439/439 全部位于 `artifacts/rust-tauri/R05/RR3/A-REVIEW-01/snapshot/docs/omp-adoption/evidence/`（P00.x~P17.14、adversarial-fixes、adversarial-review 等）
- 前缀外计数 = 0；无任何生产 / 现行 docs / tracked 相关路径 → 未触发停止条款
- mtime：2026-09-12×269、2026-09-13×69、2026-09-15×101（区间 2026-09-12T02:39:02Z ~ 2026-09-15T02:20:22Z），与 ARCH-01 报告的 R01 时代窗口吻合
- `find … A-REVIEW-01 -name .gitignore` = 0：嵌套 .gitignore 确已随 ARCH-01 删除，本批因此显形
- 与 ARCH-01/boundary-update.json notes 互证：「主树余量中存在 539 个范围外文件（439 个 A-REVIEW-01/snapshot 旧证据 + 100 个被根 .gitignore 隐藏…）按红线保留未触碰」——本 R2 即该 439 的获批后续

## 3. 先登记后删除

- `mini-cleanup-receipt.json`：439 条逐项 `path / bytes / sha256 / mtime`，合计 **69,088,347 字节**，SHA256 均为删除前实读
- 删除（自 receipt 回读，前缀 + 数量 + 字节数三重校验）：**deleted=439 / missing=0 / sizeMismatch=0**（`delete-r2-result.json`）
- 空目录清理：149 个（仅删除变空目录；snapshot/、A-REVIEW-01/ 因仍含被忽略内容目录而保留）
- 删除后逐 R 路径 lexists = 0 残留；未跟踪总数 494 → 58
- **不碰**：被根 .gitignore 隐藏的 .mimosa / __pycache__ / reviews 等忽略文件（brief 明令不属本任务）；`LingxiAgent-RR3-localonly-*` 外置目录；tracked / 已提交 / 生产 / 现行 docs

## 4. boundary-update-r2.json

R 集标 `historically_removed`，指向 `mini-cleanup-receipt.json`；只追加增量，未改 ARCH-01 既有产物、baseIndex 及任何原文件。

## 5. 最终核验

- `git status --porcelain --untracked-files=no` = **0**（tracked 工作区干净）
- tracked 基线复核：`git ls-files -s | shasum -a 256` = `64d610a91beedf0f0a8f1f9536de9ea6a4da94c978c334b5a39ac176b1508c30`（前后完全一致 → **tracked 零变化**）
- HEAD 未变；stash=1（既有，未动）；Git 全程只读（无 add/commit/stash/checkout）
- 余量构成（删除后 59，写入本报告与 boundary 后为 61）：
  - `artifacts/rust-tauri/R05/RR3/ARCH-01/` × 12（既有产物，未改）
  - `artifacts/rust-tauri/R05/RR3/ARCH-01-R2/` × 8（本任务产物）
  - RELOCATED-ARCH01.json 标记 × 39（既有，未改）
  - 两个 brief × 2（`docs/rust-tauri/R05/repair-current/RR3_ARCH01_*.md`）
  - 被根 .gitignore 隐藏项默认不显示，维持原状

## 6. 产物清单（artifacts/rust-tauri/R05/RR3/ARCH-01-R2/）

| 文件 | 内容 |
| --- | --- |
| `REPORT.md` | 本报告 |
| `mini-cleanup-receipt.json` | 439 条 path/bytes/SHA256/mtime（删除前实读） |
| `boundary-update-r2.json` | R 集 historically_removed 边界增量 |
| `commands.jsonl` | 命令日志（11 步） |
| `delete-r2-result.json` | 删除结果（439/0/0 + 149 空目录） |
| `R-set.txt` | R 集路径清单（439 行） |
| `untracked-all.txt` | 枚举时全量未跟踪快照（494 行） |
| `tracked-baseline.sha256` | tracked 集基线哈希（零变化证明） |

完成，停写。
