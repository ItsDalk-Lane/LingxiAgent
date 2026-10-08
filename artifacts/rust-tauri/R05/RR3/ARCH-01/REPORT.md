# RR3 ARCH-01 — localOnly 原件删除/迁出收尾（用户已批准；全新空历史实施）

- 执行者：R05 RR3 ARCH-01 全新空历史实施者；未参与 RR3 此前任何轮；未派子代理。
- 任务书：`docs/rust-tauri/R05/repair-current/RR3_ARCH01_BRIEF.md`（全文读取，严格执行）。
- 输入全文/关键节读取：`DELIVERY-FINAL-02/classification.json`（35,070 行，字段以实读为准）、`local-paths.nul`（5,912）、`DELIVERY-FINAL-02/REPORT.md`、`DELIVERY-REVIEW-01/REVIEW.md`、`GIT-RECEIPT-01/receipt.json`、F51-01/F52-01 回执与 RELOCATED-F51 标记格式。
- HOME=/Users/study_superior（开工即符合，无需 export，已复核 `whoami`/`~`）。
- 边界：Git 全程只读（仅 status/ls-files/diff/rev-parse 读命令，一律 `--no-optional-locks`；零 add/commit/push，未改 .gitignore/配置）；只处理 classification.json 中 category 属 LOCAL_* 的 5,912 项；未触碰 tracked/已提交文件、现行 docs、用户内容、`LingxiAgent-RR3-localonly-fixtures/`（F51/F52 外置内容原样）。

## 一、结论

**DELETE 5,851/5,851（769,208,829 bytes）+ RELOCATE 61/61（448,967,152 bytes）全部成功，0 跳过、0 失败。** 合计 5,912 项 / 1,218,175,981 bytes，与 classification.json localOnly 总面逐字节相等。SHA256 预核通过率 **5,912/5,912（100%）**；DELETE 每项删除前即时实读复核 SHA 全等；RELOCATE 每项目的地实读复核 SHA 全等（同卷 rename 前后双核）。主树内原 5,912 项归零（逐路径 lexists 全 False）。tracked 面零变化（§六指纹证明）。

## 二、处置划分核对（与 classification.json category 逐类一致，未自由发挥）

| category | 任务书处置 | 实际计数 | 实际 bytes | 结果 |
|---|---|---:|---:|---|
| LOCAL_BUILD_CACHE | DELETE | 3,442 | 670,398,712 | 全删 |
| LOCAL_ISOLATED_SOURCE | DELETE | 1,199 | 40,831,902 | 全删 |
| LOCAL_ONLY | DELETE | 1,074 | 57,969,997 | 全删 |
| LOCAL_RUNTIME_STATE | DELETE | 136 | 8,218 | 全删 |
| **DELETE 小计** | | **5,851** | **769,208,829** | |
| LOCAL_BINARY | RELOCATE | 61 | 448,967,152 | 全迁 |
| **合计** | | **5,912** | **1,218,175,981** | 与 localOnly 总面相等 ✓ |

输入一致性预断言（驱动器内 assert，失败即中止）：classification localOnly 集合 == `local-paths.nul`（5,912/5,912 逐路径相等）；与 `include-paths.nul` 零交集。61 个 LOCAL_BINARY 魔数亲验全为 Mach-O 64（0xfeedfacf），无「实为文本」矛盾；`C-F46-REVIEW-01/isolated/build/liblingxi_service.rlib`（134,457,480B）实测 SHA `ef146b51…` 与 REVIEW/DELIVERY 登记值相等。

## 三、硬性护栏逐项执行情况

1. **非 tracked**：开工取 `git ls-files -z` 全量快照（67,220 条，exit 0）与 5,912 求交 = **0**；另按任务书口径对全部 61 个 LOCAL_BINARY + 随机 120 个其余项（seed 20261008）逐路径跑 `git ls-files --error-unmatch`，181/181 如预期失败（exit 1，未跟踪）。
2. **SHA 预核**：5,912 项逐文件实读 SHA256，与 classification 记录全等（bytes 亦逐项相等）；预核不通过者跳过——实际 **0 项跳过**。DELETE 阶段每项删除前再次实读复核（不等即跳过不删）——实际 **0 项 sha_skips**。
3. **迁出**：逐文件 `os.rename`（同卷 rename 字节保持）至 `/Users/study_superior/Desktop/Code/LingxiAgent-RR3-localonly-evidence/`（镜像原相对路径布局；目的地不存在则创建；目的地已存在即断言失败）；61/61 目的地 SHA256 实读与 classification 记录相等；收尾又抽 3 项独立复读全等。
4. **范围**：仅处理 5,912 项；空目录清理仅限 `artifacts/` 子树内、删净后真空目录（`os.rmdir` 只能删空目录，非空自动失败）。
5. **Git 只读**：全程仅 §五所列读命令。
6. HOME 已复核。

## 四、逐阶段记录（UTC，exit 0 除注明外；argv 级明细见 commands.jsonl）

| UTC | 动作 | 结果 |
|---|---|---|
| 02:05:55Z | 开工基线：HEAD/branch/`ls-files -s`/`diff HEAD`/staged/modified | HEAD `b4b0094d3`、staged 0、modified 0、`diff HEAD`=空（e3b0c442…）、`ls-files -s` SHA256 `64d610a9…` |
| 02:08:24Z | `arch01_driver.py precheck`（首次启动因 git() text/bytes 缺陷退出 1，即时修复后重跑） | 修复记录见 §七 |
| 02:09:49–52Z | precheck 全量：nul/include 断言 + ls-files 快照 + 181 例 --error-unmatch + 5,912 逐项 stat/bytes/SHA256 + LOCAL_BINARY 魔数 | 5912/5912 ok；tracked 交集 0；抽样 0 命中 |
| 02:11:34Z | relocate：61 项 os.rename + 目的地逐文件 SHA 复核 | 61/61，all_sha_equal=true |
| 02:11:34Z | markers：39 个父目录写 `RELOCATED-ARCH01.json`（沿 F51 格式） | 39/39 |
| 02:11:47–49Z | delete：5,851 项删前即时 SHA 复核 + os.remove（每 500 项落盘检查点） | 5,851/5,851，all_sha_equal_classified=true |
| 02:12:21Z | rmdirs：删净后真空目录自深至浅 os.rmdir | 移除 418、保留非空 359（rmdir-result.json） |
| 02:14–02:2xZ | final 核验（两轮：boundary-update.json 写入前后各一次）+ 余量展开归因 | 见 §六 |

## 五、执行的全部 Git 命令（全部只读，`--no-optional-locks`）

`rev-parse HEAD` / `branch --show-current` / `rev-parse --abbrev-ref HEAD@{upstream}` / `ls-files -s` / `diff HEAD` / `diff --cached --name-only` / `diff --name-only` / `ls-files -z`（两次：基线+precheck）/ `ls-files --error-unmatch <path>` ×181（预期全 exit 1）/ `status --porcelain [-uall] [-z]` / `check-ignore [-v] [--stdin]`。无任何写命令、无 index 刷新写盘（no-optional-locks）。argv/exit/UTC 逐条存 `commands.jsonl`（45,359 B）。

## 六、最终核验（final-verification.json）

- **tracked 零变化证明**：收尾 `git diff HEAD` SHA256 = `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`（空输入）与开工实测逐字节相等；`git ls-files -s` SHA256 = `64d610a91beedf0f0a8f1f9536de9ea6a4da94c978c334b5a39ac176b1508c30` 与开工实测逐字节相等；HEAD 恒为 `b4b0094d37d4de2b1914d8c8de5e89cdf79312a6`；staged=0、modified=0、`git diff --name-only`=0。（说明：GIT-RECEIPT-01 时代基线 `3016f7ae…` 系 d1786a34f 提交前旧值，本轮基线为提交后工作树实态，口径为「本轮开工 vs 收尾」零变化。）
- **未跟踪余量构成**（`git status --porcelain -uall`，final 轮实测 490 条；本 REPORT.md 写入后再 +1 = 491）：
  - ARCH-01 输出 11 件（REPORT.md 写入后 12）：arch01_driver.py、precheck.json、checkpoint.json、commands.jsonl、relocation-receipt.json、cleanup-receipt.json、relocate-exceptions.json（空）、delete-exceptions.json（空）、rmdir-result.json、boundary-update.json、final-verification.json、REPORT.md；
  - `RELOCATED-ARCH01.json` 标记 39 个（逐父目录清单见 checkpoint.json phases.markers.written，与磁盘逐一相等）；
  - 任务书本身 `docs/rust-tauri/R05/repair-current/RR3_ARCH01_BRIEF.md`（root 预置，未触碰）；
  - **范围外残留 439 项**（见 §七偏差 1）。
- `LingxiAgent-RR3-localonly-evidence/` 在仓库外（/Users/study_superior/Desktop/Code/），主树不可见 ✓；实存 61 文件。
- 原 5,912 路径主树归零：precheck.json 全路径集 5,912/5,912 `lexists=False` ✓。

## 七、偏差清单（如实登记，未擅自动范围外对象）

1. **主树余量含 439 个范围外旧证据文件**（全部 `artifacts/rust-tauri/R05/RR3/A-REVIEW-01/snapshot/docs/` 下，mtime 2026-09-12~15）：成因链已取证——FINAL-02 枚举（2026-10-07T23:36Z）用 `--exclude-standard`，当时 `A-REVIEW-01/snapshot/.gitignore`（9 月副本、无现行 `!artifacts/rust-tauri/**/*.log` 反规则；该文件自身属 LOCAL_ONLY 并经本轮批准删除，删除清单内含 .gitignore ×2：A-REVIEW-01 与 A-REVIEW-02 的 snapshot 副本）把这些文件排除在 35,041 枚举之外，故从未进入 classification 35,070 全集（0/35,070 命中）、不属本轮 5,912 处置对象；本轮删除该嵌套 ignore 后按现行仓库根 .gitignore 规则显形。**按红线「只处理这 5,912 项」保留未触碰**，逐路径清单见 final-verification.json remainder_expanded。
2. 另有 **100 个被仓库根 .gitignore 隐藏的现存文件**（88 `.mimosa/`（根 .gitignore:144）、2 `__pycache__/`、10 snapshot R04 reviews .md（`reviews/` 规则，根 .gitignore:128）——check-ignore -v 逐规则亲验；`git ls-files` 确认 0 tracked）：同样从未入枚举、不属 5,912，保留未触碰。
3. 本轮不产生任何 classification 矛盾跳过项：LOCAL_BINARY 61/61 为真 Mach-O；LOCAL_ONLY/LOCAL_RUNTIME_STATE 按 SHA 门删除，未做内容外泄输出。
4. 驱动器首跑缺陷（git() text 模式与 `-z` 字节解析冲突，exit 1，未产生任何文件系统副作用）即修即录；重跑通过。
5. 任务书预期「未跟踪余量=仅 ARCH-01 输出+RELOCATED 标记」在字面上因 §七.1 的 439 项范围外显形文件而未满足；该 439+100 项的处置（保留/另行清理/补分类）超出本任务授权，留总控决断。原 5,912 项归零这一核心预期已满足。

## 八、产物清单（本目录，全部本轮新写）

1. `REPORT.md`（本文件）；2. `cleanup-receipt.json`（5,851 条逐项 path/category/bytes/删除前 SHA256/UTC + totals）；3. `relocation-receipt.json`（61 条 old→new/SHA 前后/UTC + totals）；4. 39 × `RELOCATED-ARCH01.json`（分布各原父目录，receipt 指向本轮回执）；5. `boundary-update.json`（增量边界：deleted→historically_removed 指向 cleanup-receipt，relocated→新位置指向 relocation-receipt；沿 reference-boundary-index 既有格式，未改原文件）；6. `final-verification.json`（最终核验 + 余量展开）。
辅助证据：`arch01_driver.py`（分期驱动器，检查点可续跑）、`precheck.json`（5,912 逐项预核明细+魔数）、`checkpoint.json`（阶段状态）、`commands.jsonl`（argv/exit/UTC）、`relocate-exceptions.json`/`delete-exceptions.json`（均空=零异常）、`rmdir-result.json`（418 删/359 留）。

完成停写。Git 写动作（add/commit/push）与范围外残留处置留总控。
