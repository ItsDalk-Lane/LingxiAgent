# RR3 ARCH-01：localOnly 原件删除/迁出收尾（用户已批准）

你是全新空历史实施者，未参与 RR3 此前任何轮。全文读本文与以下输入：`artifacts/rust-tauri/R05/RR3/DELIVERY-FINAL-02/classification.json`（35,070 行权威分类，每行含 path/gitState/type/mode/bytes/SHA/category/basis——以实际字段为准）、`DELIVERY-FINAL-02/local-paths.nul`（5,912）、`DELIVERY-FINAL-02/REPORT.md`、`DELIVERY-REVIEW-01/REVIEW.md`、`GIT-RECEIPT-01/receipt.json`、F51-01/F52-01 回执与 RELOCATED 标记格式（模板）。背景：RR3 已放行并推送（d1786a34f/b4b0094d3）；用户批准对 localOnly 集合执行"删+迁"收尾，使工作树干净。

## 处置划分（按 classification.json 的 category 字段，不得自由发挥）

- **DELETE（5,851 项）**：`LOCAL_BUILD_CACHE`(3,442) + `LOCAL_ISOLATED_SOURCE`(1,199) + `LOCAL_ONLY`(1,074，经总控核为 A-REVIEW-01/snapshot 等隔离快照类) + `LOCAL_RUNTIME_STATE`(136，含真实临时票据/token——删除属安全卫生)。
- **RELOCATE（61 项）**：`LOCAL_BINARY` 全部 → `/Users/study_superior/Desktop/Code/LingxiAgent-RR3-localonly-evidence/`（仓库外；目录不存在则创建）。理由：被测对象本体，macOS adhoc 重链接不保证同字节，不可再生。
- 若实读中发现某文件实际类别与 classification 记录矛盾（如 LOCAL_BINARY 里出现文本、LOCAL_ONLY 里出现疑似凭证），不要删除/迁出该项，单独记录交总控。

## 硬性安全护栏（每项删除/迁出前逐条过）

1. 路径必须在 `local-paths.nul` 内且 classification 类别属于上表对应集合；不在 include-paths.nul、不在任何 tracked 清单（`git ls-files --error-unmatch <path>` 必须失败）。
2. **删除前 SHA256 复核**：实读文件计算 SHA256，与 classification.json 该行记录的 SHA 相等才允许删；不等→跳过该项并记录，不删。
3. 迁出用 `mv`（同卷 rename 字节保持），目的地逐文件 SHA 复核与原记录相等；目录内多文件时逐文件。
4. 只处理这 5,912 项；不碰 tracked/已提交文件、.gitignore、生产、现行 docs、用户内容、`LingxiAgent-RR3-localonly-fixtures/`（F51/F52 已迁内容原样保留）。
5. Git 全程只读（status/ls-files 等），**禁止任何 Git 写操作**（add/commit/push 都由总控之后做）。
6. HOME 若非 /Users/study_superior 先 export 并记录。

## 产物（全部写入 artifacts/rust-tauri/R05/RR3/ARCH-01/）

1. `REPORT.md`：处置划分核对、逐类计数/字节、全部命令与 exit/UTC、偏差清单。
2. `cleanup-receipt.json`：每个删除项 path/category/bytes/删除前 SHA256/UTC（5,851 条，逐项无缺）。
3. `relocation-receipt.json`：每个迁出项 old→new/SHA 前后/UTC（61 条）。
4. RELOCATED 标记：在每个受影响的原父目录写 `RELOCATED-ARCH01.json`（沿 F51 格式：被迁子路径、新位置、指向回执、UTC）；纯删除目录不留空目录（删净后父目录若为空一并移除，但不得移除任何含 tracked/已提交文件的目录）。
5. `boundary-update.json`：引用边界更新——deleted→`historically_removed`（指向 cleanup-receipt）、relocated→新位置（指向 relocation-receipt）；沿 `reference-boundary-index.json` 既有格式做增量文件，不改原文件。
6. 最终核验：`git status --porcelain` 未跟踪余量 = 仅 ARCH-01 输出 + 各 RELOCATED-ARCH01 标记 + LingxiAgent-RR3-localonly-evidence 在仓库外不可见（即主树内原 5,912 项归零）；`git diff HEAD` 与 `git ls-files -s` 哈希与收口基线逐字节相等（tracked 零变化）。

完成停写。返回（≤25 行中文）：DELETE/RELOCATE 实际计数与字节、SHA 预核通过率与跳过项、最终 git status 余量、tracked 零变化证明、偏差、产物路径。
