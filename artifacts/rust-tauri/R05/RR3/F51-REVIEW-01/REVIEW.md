# RR3 F51 独立验收（F51-REVIEW-01）

- 审查者：RR3 F51-REVIEW-01 全新空历史独立审查智能体，未参与 F51 实施及 RR3 此前任何实施/审查/终审轮；未派子代理。
- 任务书：`docs/rust-tauri/R05/repair-current/RR3_F51_REVIEW_BRIEF.md`（全文读取）。已全文读取 RR1_MASTER_PROMPT_2026-10-04、RR2_MASTER_PROMPT_2026-10-06、RR3_BRIEF、RR3_REVIEW_BRIEF、RR3_F51_BRIEF、RR3_ISSUE_MATRIX.json（F51 行）、RR3_PROGRESS.md、RR3_HANDOFF.md、FINAL-01/STAGE_REVIEW.md（重点 §四根因）与 F51-01 全部产物（REPORT、RELOCATION-RECEIPT.json、relocation-checkpoint.json、baseline-git-state.txt、verify-ab/c/c2/c3/c-incident*、verify-d-and-final-audit、verify-e、final-state-check、selfcheck-output、run1-abort-log、run-relocation.py、digest_tree.py、aborted-run2 回执）。
- 环境回执（开工）：UTC 2026-10-07T12:12:44Z；HEAD=`b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`、分支 `codex/rust-tauri-migration`（与 FINAL-01/F51 一致）；HOME=`/Users/study_superior`；git 2.53.0、macOS 27.0.1 arm64；Data 卷可用 558Gi。
- 命令记录：本目录 `commands.jsonl`（25 条，机器记录 label/cmd/cwd/UTC 起→止/exit/stdout·stderr 文件与 SHA256，逐命令原始输出在 `out-*.txt`/`err-*.txt`）。所有 git 读均用 `--no-optional-locks`；对迁出目的地的一切访问只读（含嵌套 git 仅 rev-parse/ls-files plumbing）。

## 逐项结论

### 1. 权威枚举复核 — **PASS**

- 亲跑 `git --no-optional-locks ls-files --cached --others --exclude-standard -z`（cwd=仓库根，exit 0）：全量 69,027 条路径中**尾斜杠目录条目 0 条**（`grep -c '/$'` 输出 0）；空输出与 grep 计数双证（`out-item1-dir-entry-count.txt`）。
- FINAL-01 frozen `binder_observation.all_directory_entries`（56 条）与回执 `entries`（56 条）**双向集合完全一致**：in_final01_not_receipt=0、in_receipt_not_final01=0、逐条排序相等；56 条互不为祖先；目的地名唯一（55 斜杠编码 + 1 短名 `40--repo`，与回执 `naming_deviations` 一致）；回执 totals（110,071 文件/121 符号链接/5,324,827,710 字节）与逐条目求和一致、`all_digests_equal=true`（`out-item1-crosscheck-56.txt`）。

### 2. 回执抽查（8 目录，含最大项与短名偏差项）— **PASS**

- 全局（先于抽查）：56/56 目的地目录存在且**均含嵌套 `.git`**（`dest_without_nested_.git=[]`）；主树原路径残留 0；目的地根恰 56 条目、零杂项；41 个 `RELOCATED-F51.json` 原位标记与回执 markers 集合相等（`out-item2-global-56.txt`）。
- 用 F51 自带 `digest_tree.py`（同法可证相等）对新位置重扫，比对回执 file_count/symlink_count/total_bytes/tree_digest 四字段（`compare_sample.py`，逐样本输出在 `out-item2-digest-*.txt`）：

| 样本 | files/bytes | tree_digest 全等 | 嵌套 .git / HEAD |
|---|---|---|---|
| **J-REVIEW-01/copy（5.13GB 最大项）** | 102,887 / 5,130,434,169 =回执 | 当前=fee8e52b…（=verify-c 事件登记的改写后摘要）；迁出时刻 79736540… 见 §4 | .git 存在；`rev-parse HEAD`=b3ac0e6a…（=回执） |
| **A-REVIEW-02/independent/internal-fresh/repo（`40--repo` 短名偏差项）** | 117 / 75,371 =回执 | d6de2b1e… 全等 | .git 存在（rev-parse --git-dir=.git） |
| A-02/copy-delete-rename | 33 / 34,456 =回执 | 291f5329… 全等 | .git 存在（无提交，HEAD null=回执 null） |
| A-REVIEW-02/independent-02/internal-fresh/repo | 103 / 165,855 =回执 | 69f9d1a9… 全等 | .git 存在 |
| I-01/selfcheck-02/runner-copy | 728 / 14,534,054 =回执 | ad2b075c… 全等 | HEAD=20f41d7f…（=回执） |
| I-01/selfcheck-03/copy | 70 / 1,626,295 =回执 | f94e4fe1… 全等 | HEAD=d932e5e9…（=回执） |
| I-REVIEW-01/permanent-final/runner-copy | 1,201 / 58,288,269 =回执 | 12e22b46… 全等 | HEAD=12e22ded…（=回执） |
| J-REVIEW-02/i-restore/copy | 70 / 1,626,296 =回执 | 947c4b3e… 全等 | HEAD=5071f84a…（=回执） |

- 唯一与回执 digest 不全等的是 J-REVIEW-01/copy，且差额被 §4 独立圈定为单文件 `.git/index` 事后事件（file_count/bytes/符号链接数/嵌套 HEAD 全等；当前摘要与事件登记值相等且 11:58Z→12:17Z 多轮扫描稳定）。7 个无事件样本四字段全部全等。

### 3. tracked 零变化 — **PASS**

- `git --no-optional-locks diff HEAD` SHA256=`8c23d0d944b66d76…`、`git ls-files --cached -s` SHA256=`3016f7ae9d18d939…`，**与 F51 基线（迁出前）逐字节相等**；diffstat=`25 files changed, 8878 insertions(+), 370 deletions(-)` 与 FINAL-01 §二记录一致；tracked 修改集 25 行与基线逐行相同（TRACKED-SET-IDENTICAL）。
- status 全量哈希现为 00b4b146…≠基线 c9859c9d…：增量**完全解释**——唯一差异行是 `?? docs/rust-tauri/R05/repair-current/RR3_F51_REVIEW_BRIEF.md`（mtime 2026-10-07 20:04 local=12:04Z，即 F51 停写 20:03 之后总控为本审查新写的任务书，非 F51 所为）；从当前 status 精确剔除该行后重建哈希=c9859c9d…**与基线全等**（`out-item3-untracked-delta-explain.txt`），tracked 与其余 untracked 逐行未变。
- 注：`out-item3-tracked-zero-change.txt` 头三行 echo 计数因引号转义失真（显示 75/0/空），权威重算见上条 python 输出：75 行 = 25 tracked + 50 untracked。

### 4. verify-c 事件独立评估 — **PASS（事件不损害回执可信度）**

亲跑复核（非转述）：

- **圈定唯一性**：对全部 56 个目的地 `find -newermt '2026-10-07 19:54:56'`（=迁出完成 11:54:56Z）全树扫描，命中恰 2 路径：`J-REVIEW-01_copy/.git`（目录 mtime 随 index 重写联动）与 `J-REVIEW-01_copy/.git/index`，**无任何其他文件/目录在迁出窗口后被写**（`out-item4-postmutation-find.txt`；F51 早前 19:58 local 审计同结论）；`.git/index` mtime=19:55:40 local=11:55:40Z，与事件登记的 git status 时刻吻合。
- **语义完好**：`git --no-optional-locks -C <fixture> ls-files -s | sha256`=`3016f7ae…`，与主仓库 index 哈希全等；file_count/total_bytes 与回执全等；当前 tree_digest=fee8e52b… 与事件登记一致且跨 ~20 分钟稳定；夹具内 .DS_Store 等 mtime 均早于迁出（13:42–13:44 local），非事后新增。
- **结论**：突变发生在迁出完成且 F51 已于 11:56Z 独立重扫得 79736540…（=回执 pre=post）**之后**、由实施者自己的验证命令触发；影响被双重 find 审计圈定为单文件的 stat 缓存字段；字节级 pre 状态无法还原已如实登记、未伪造。**迁出本身字节保持的回执结论可信**。后续任何人对该夹具的检查必须用 `--no-optional-locks`/plumbing（本审查已遵守，final-integrity-recheck 证明我的读取也未再写目的地）。

### 5. 红线核验 — **PASS**

- `.gitignore`：`git diff HEAD -- .gitignore` 空、porcelain 空、工作区文件与 HEAD blob 逐字节相同（GITIGNORE-IDENTICAL-TO-HEAD）。
- rust/scripts/docs 现行文件零改动：`git diff HEAD` 全量哈希=8c23d0d9…（=F51 迁出前基线=FINAL-01 记录的 25 文件 overlay +8878/−370），tracked 修改集与基线逐行一致——F51 未触任何 tracked 生产文件；生产不引用验证独立重跑：rust/scripts/docs 全部现行文件（1022 tracked+49 untracked）对 56 条被迁路径子串扫描，命中仅 1 处=`RR3_F51_BRIEF.md`（untracked 流程任务书示例路径），**tracked 生产命中 0**，与 F51 verify-d 及 FINAL-01 预检一致。
- 无 Git 写痕迹：HEAD/分支/`log -1` 与 FINAL-01 记录一致（b3ac0e6a…）；`reflog HEAD` 最后一条=2026-10-07 03:02:15 +0800 提交 b3ac0e6a，**其后零新条目**（FINAL-01、F51、本审查全程无 commit/checkout/branch/tag）；主 `.git/index` mtime=03:02:15、size=6,139,139，自该提交后未被重写（F51 的只读 git 命令连机会性 index 刷新都未触发主库）；本地分支 tip refs 与 FINAL-01/F51 只读观察一致（main=7d1a0c6b… 前移为既有事实）。无 MERGE/CHERRY_PICK 痕迹。

### 6. 绑定器行为正反控制（隔离副本）— **PASS**

- 在 `/private/tmp/f51-review-01-scratch/repo`（隔离 git 仓库，不触主树；已用后即删）构造：未跟踪目录 `evidence-fixture/repo`（内含嵌套 `.git`+1 文件）+ 无嵌套 .git 的未跟踪目录 `plain-dir`（2 文件）+ 1 tracked 文件。
- **正控（枚举形态）**：`git ls-files --cached --others --exclude-standard` 输出 `evidence-fixture/repo/` **单条尾斜杠目录条目**（内部文件不被枚举），而 `plain-dir/a.txt`、`plain-dir/b.txt` 逐文件展开、无目录条目——证明目录条目形态由嵌套 `.git` 特异触发，与 FINAL-01 §四模型一致。
- **正控（拒绝形态）**：按 `rust/crates/xtask/src/candidate.rs` 真实语义（:177-229 枚举 + :364 `!before.is_file()` 分支）仿真绑定器，对 `evidence-fixture/repo/` 精确复现 FINAL-01 cmd-06 的错误形态：`error: cannot snapshot candidate before stage gate: candidate file …/evidence-fixture/repo was replaced by a non-file`；其余 3 条全部可按文件哈希。
- **反控（解除）**：将夹具 `mv` 出隔离仓库（模拟 F51 迁出）→ 目录条目 0、全部条目均普通文件、零拒绝（BINDING-PRECONDITION-RESTORED）；结合项 1 主树实测目录条目清零，根因模型（嵌套 .git 未跟踪目录 → 目录条目 → fail-closed；迁出即解除）被完整证明。注：真实 xtask 二进制含跨 checkout 拒绝保护（main.rs `bound_repo_root`，RR-T08-F1），不能对 scratch 仓库运行，故用与 candidate.rs 逐分支等价的仿真 + FINAL-01 cmd-06 原始 stderr 对照，形态逐字一致。

### 7. 审查方法正反控制 — **PASS**

- **摘要灵敏度**：将 A-02 夹具（34KB，含 1 符号链接）拷贝到 scratch，digest=291f5329…（=回执，阳性）；翻转其中 1 个文件的 1 个字节 → digest=a4c05501…（**被检出**）；还原字节 → digest=291f5329…（恢复）。digest 方法单字节敏感且确定。
- **比对器防空洞**：`compare_sample.py` 对未篡改回执 ALL_EQUAL=True/exit 0；对内存中篡改 1 位摘要的回执 tree_digest EQUAL=**False**/exit 1——本审查的"全等"结论非空洞通过。
- **哈希工具交叉**：`shasum -a 256` 与 python hashlib 对同一文件输出全等。

## 总结论

- **七项全部 PASS，mustFix：无。** F51-01 的迁出、回执、验证与偏差登记经独立复核成立；唯一事后事件（J-REVIEW-01/copy 的 `.git/index` 单文件改写）被独立圈定且不损害回执可信度。F51 可由总控关闭；其后 FINAL-02 由全新阶段审查者执行 STAGE_REVIEW §八的 verify-stage R05 命令。
- 非阻断观察（如实记录）：(a) 当前 status 全量哈希相对 F51 基线多出的一行=总控 20:04 新写的本审查任务书（untracked 流程文件，非生产、非 F51 所为），已用重建哈希精确证明无其他增量；(b) `out-item3-tracked-zero-change.txt` 头三行 echo 计数为引号转义伪影，权威数值以 python 重算为准。

## 边界声明

- 只写本目录 `artifacts/rust-tauri/R05/RR3/F51-REVIEW-01/`（REVIEW.md、commands.jsonl、run.sh、compare_sample.py、out-*/err-*）；主树其余一切只读；迁出目的地只读；未做任何主库 Git 写操作（全程 `--no-optional-locks`/plumbing）；无系统变更；隔离 scratch 建于 /private/tmp 并已删除；未派子代理；未修改被验对象。
- 收尾自查（`out-final-integrity-recheck.txt`）：三哈希仍 8c23d0d9…/3016f7ae…、目录条目仍 0、目的地自迁出后写入仍仅事件那 2 路径、HEAD 未变——本审查过程自身未改动任何被验状态。

（审查完成，停写。）
