# RR3 F52 独立验收（F52-REVIEW-01）

- 审查者：RR3 F52-REVIEW-01 全新空历史独立审查智能体，未参与 F52 实施及 RR3 此前任何实施/审查/终审轮；未派子代理。
- 任务书：`docs/rust-tauri/R05/repair-current/RR3_F52_REVIEW_BRIEF.md`（全文读取）。已全文读取 RR1_MASTER_PROMPT_2026-10-04、RR2_MASTER_PROMPT_2026-10-06、RR3_BRIEF、RR3_REVIEW_BRIEF、RR3_F52_BRIEF、最新 RR3_ISSUE_MATRIX.json（F52 行 OPEN）、RR3_PROGRESS.md、RR3_HANDOFF.md 尾部、FINAL-02/STAGE_REVIEW.md（根因与权威枚举 69,075 条）、F51-REVIEW-01/REVIEW.md（格式参照，非结论）、F51-01/RELOCATION-RECEIPT.json（56 目的地名单）与 F52-01 全部产物（REPORT.md、RELOCATION-RECEIPT-F52.json、classify_binder_surface.py、四份分类 JSON、前后摘要、git 快照、cargo-xtask-candidate.log）及 `A-REVIEW-02/RELOCATED-F52.json`；并只读核实 `rust/crates/xtask/src/candidate.rs` 组件遍历语义（约 326-369 行）。
- 环境回执（开工）：UTC 2026-10-07T13:08:45Z；HEAD=`b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`、分支 `codex/rust-tauri-migration`（与 FINAL-02/F52-01 一致）；HOME=`/Users/study_superior`；git 2.54.0（Apple Git-157）、macOS 27.0.1 arm64。
- 命令记录：本目录 `commands.jsonl`（17 条，机器记录 label/argv/cwd/UTC 起→止/exit/stdout·stderr 文件与 SHA256，原始输出在 `out-*.txt`/`err-*.txt`）。所有主仓 git 读均带 `--no-optional-locks`；迁出目的地全程只读。exit 读法备注：`item1-binder-recheck` 记录 exit=1 是内层 `set -e` 脚本在 `grep -c '/$'` 无匹配（输出 0，即目录条目为零的期望结果）后提前终止的包裹层退出码——**分类器本体 exit 0**（见该 out 文件内 `classifier_exit=0` 与 `review-classification-main.json`）；`item1-delta-explain` 首跑因内层变量引用失误为部分输出（权威重跑见 `-2`/`-3`）；`item6-tamper-controls`/`-2` exit=1 为自控脚手架缺陷迭代（详见 §6 如实登记），`-3` 为全绿权威跑。
- 审查工具（全部为本审查新写，与实施者脚本零共用）：`review_classify.py`（独立分类器：终分量直接 lstat + 祖先目录缓存，非逐条目逐组件重走）、`binder_sim.py`（按本人对 candidate.rs 的独立解读复现绑定器拒绝形态）、`item2_receipt_check.py`（回执比对器）、`item4_f51_surface.py`（F51 迁出面核查）、`item6_tamper_check.py`（篡改自控）。

## 逐项结论

### 1. 绑定面独立复检 — **PASS**

- 亲跑（自有分类器，`--assert-clean`）：`git ls-files --cached --others --exclude-standard -z` 全量 **69,140 条 = regular_file 69,140（100%）**；final_symlink/ancestor_symlink/directory_entry/ancestor_not_dir/irregular/missing 六类**全部为 0**，exit 0（`out-item1-binder-recheck.txt`、`review-classification-main.json`）。
- 交叉计数（与实施者三跑+B3）：Run A 69,123（**恰 1 final_symlink**，其 `non_regular_samples` 唯一条=被迁路径，与 FINAL-02 权威枚举同形态）→ B1 69,126 → B2 69,130 → B3 69,131 → 本审查 69,140。**增量 +9 逐项闭合**：B3 输出 JSON 自身（B3 枚举后写入，不计入自身）+1、总控为本审查新写的 `RR3_F52_REVIEW_BRIEF.md`（mtime 21:06:53 local=13:06:53Z，晚于 B3 与 F52-01 停写 13:06:20Z）+1、本目录当时 7 文件 +7；`find -newermt` 全树扫描确认 B3 之后除上述与被 gitignore 的 `.mimosa` 会话文件（枚举计数实测 0，不入绑定面）外**零新写入**，无树漂移。
- 独立旁证：`--cached` 计数 38,066（与 F51/J 轮 tracked 口径一致）；尾斜杠目录条目 `grep -c '/$'`=0；枚举中 `independent-validator-bin` 残留 grep=0；原路径 `test ! -e` 通过（已不在主树）。
- 终态复查（本报告成文前）：69,175 = 100% 普通文件、六类全 0、exit 0；相对首跑 +35 全部为本审查目录新增的普通证据文件，逐文件可数（`out-final-integrity-recheck.txt`、`review-classification-final.json`）。

### 2. 迁出回执逐项相等 — **PASS**

- 用本审查比对器对新位置两条目**全量**（非抽样）复算并与回执 pre/post 双向比对（`out-item2-receipt-check.txt`，RESULT: PASS，exit 0）：
  - `git`：sha256=`f9dc56ed2d08e1f687ed2e39c70e00f629cd485136bdea7e21b2bfaa1c0afbf8` 与回执 pre、post 均相等；lstat mode(0o100755)/uid/gid/size(255)/mtime_ns/dev 全等。
  - `python3`：readlink 目标串=`/Library/Frameworks/Python.framework/Versions/3.14/bin/python3` 逐字相等；lstat mode(0o120755)/uid/gid/size(62)/mtime_ns/dev 全等。
  - inode 无关性口径下全部相等；另观察 inode 276456367/276456368 与回执一致（rename(2) 保持 inode 的旁证，非判据）。
  - 新目录内容恰 `[git, python3]`，无多余条目。
- 原位标记 `A-REVIEW-02/RELOCATED-F52.json`：marker 字段、回执相对路径存在且即被验回执、子项 name/new_absolute_path/old_relative_path 与回执逐项一致、旧相对路径在主树已不存在——7 项检查全 OK；格式与 `RELOCATED-F51.json` 同构（marker/receipt/relocated_children/task/written_utc）。

### 3. tracked 零变化 — **PASS**

- `git --no-optional-locks diff HEAD` SHA256=`8c23d0d944b66d76f1c65e6a8883d7782a7509d434b10bc13dda2ab5bc393361`、`git ls-files -s` SHA256=`3016f7ae9d18d93951f05a77adb68946cab99a92d0a122294d4c5cf096407d47`——**与 F51-REVIEW-01 基线及 FINAL-02 记录逐字节相等**；diffstat=`25 files changed, 8878 insertions(+), 370 deletions(-)`（=FINAL-01 §二 overlay）。
- porcelain tracked 行（25 M）与 F52-01 `git-status-porcelain-before.txt` 排序 diff 为空（TRACKED-SET-IDENTICAL）；untracked 差异恰 1 行=`?? docs/rust-tauri/R05/repair-current/RR3_F52_REVIEW_BRIEF.md`（总控 13:06:53Z 为本审查新写，晚于 F52-01 停写，非实施者所为；与 F51-REVIEW-01 同款模式）。收尾复查两哈希仍全等。

### 4. 红线核验 — **PASS**

- `.gitignore`：`git diff HEAD -- .gitignore` 0 字节；工作区与 HEAD blob `cmp` 逐字节相同（GITIGNORE-IDENTICAL-TO-HEAD）。
- rust/scripts/docs 零新改动：tracked 修改集与基线逐行相同（§3）+ untracked 增量仅上述任务书一行；生产引用扫描（tracked，HEAD 与工作树双查 + 文件系统层 grep）：rust/、scripts/ **零命中**；docs/ 仅 3 个流程文件命中（RR3_F52_BRIEF.md、RR3_ISSUE_MATRIX.json、RR3_PROGRESS.md——F52 问题登记本身，非生产代码）。
- F51 迁出面零触碰（`out-item4-f51-surface.txt`，RESULT: PASS）：外置目的地根恰 57 条目 = F51 回执 56 条（逐一按 basename 集合比对）+ 本轮 1 条新名；目的地内（除新条目及其根外）`find -newermt '2026-10-07 21:00:36'`（F52 开工）命中 **0**——F51 的 56 目的地在 F52 全程零写入；主树 41 个 `RELOCATED-F51.json` 标记全数在位且 mtime 均早于 F52 开工，`RELOCATED-F52.json` 恰 1 个。
- 无 Git 写痕迹：`reflog HEAD` 顶条仍为提交 `b3ac0e6a`（其后再无任何条目，351 条总量记录在案）；主 `.git/index` size=6,139,139、mtime=2026-10-07T03:02:15（=F51-REVIEW-01 记录，未重写）；refs 快照哈希留档（本地仅 codex/rust-tauri-migration 与 main，无新分支/tag）；HEAD/分支全程未变。

### 5. 隔离正反控制 — **PASS**

- 在 `/private/tmp/f52-review-01-scratch`（隔离 git 仓，不触主树，用后已删）构造：1 tracked 文件 + 未跟踪 `fixture-bin/git`（普通文件）、`fixture-bin/python3`（**符号链接** → 系统同款目标串 `/Library/Frameworks/…/python3`）+ 未跟踪普通目录文件。
- **正控**：枚举把 `fixture-bin/python3` 列为普通条目（无尾斜杠）；自有分类器 exit 1，counts final_symlink=1 且 offender 即该路径；绑定器仿真（本人按 candidate.rs 独立实现）**逐字复现 FINAL-02 拒绝形态**：`error: cannot snapshot candidate before stage gate: candidate path /private/tmp/f52-review-01-scratch/repo/fixture-bin/python3 crosses a symlink or reparse point`（与 FINAL-02 STAGE_REVIEW 记录的结构逐字同构）。
- **反控（解除条件）**：`mv` 走该 symlink 后同法复跑——分类器 100% 普通文件 exit 0，仿真 ALL BINDABLE——证明"迁出 symlink 即恢复绑定前提"的根因模型与 F52 处置完全对应。
- **额外形态**：祖先链接（`dirlink`→`realdir`）同样以同语义被检出并同款拒绝。真实 xtask 二进制含跨 checkout 拒绝保护（RR-T08-F1，F51-REVIEW-01 已证），不能对 scratch 仓运行，故用同语义仿真+与 FINAL-02 实录 stderr 对照（形态逐字一致）；在树单测 `candidate::tests::symlink_is_refused…` 等经 `cargo-xtask-candidate.log` 佐证（9 passed/0 failed/112 filtered，与 F52-01 REPORT §四(c) 记录一致；本审查未自跑 cargo，保持主仓只读）。

### 6. 方法自控 — **PASS**

- 回执轴：回执副本篡改 `git` 条目 sha256 首位 hex（f9dc…→e9dc…）→ 本审查比对器 exit 1 且打印精确不等行（TAMPER-DETECTED）；未篡改副本同器 PASS exit 0——"全等"结论非空洞通过。
- 分类轴：`binder-surface-post-relocation.json` 副本将 regular_file 69,130→69,129（total 不变）→ 一致性检查检出差额（sum≠total）；未篡改副本一致。
- 如实登记：自控脚本首两次运行为脚手架缺陷（缺 mkdir、对副本误触发"回执路径身份"检查、pristine 标签逻辑反写），修复后第三次全绿；三次 exit（1/1/0）全部留档 commands.jsonl（item6-tamper-controls/-2/-3），未删除失败痕迹。另 `out-item1-delta-explain3.txt` 由本人裁剪（原 14.5MB 为 .mimosa 会话噪音 find 输出），已在文件内注明。

## 总结论

- **六项全部 PASS，mustFix：无。** F52-01 的迁出（字节/链接目标/lstat 全等、inode 无关性成立）、原位标记、绑定面全量重分类（100% 普通文件、六类异常零）、tracked 零变化、红线纪律与回归佐证经独立方法复核成立；绑定面唯一 symlink 形态已在主树清零且隔离仓正反控制证明根因与解除条件。**F52 可由总控关闭；其后 FINAL-03 换全新阶段审查者执行 STAGE_REVIEW §八命令。**
- 非阻断观察（如实记录）：(a) RR3_ISSUE_MATRIX.json F52 行仍 OPEN/owner「待派」、PROGRESS/HANDOFF 尾部尚未登记 F52-01 完成——台账滞后于实施，属总控在本审查后更新事项，与 F51 流程一致；(b) porcelain 相对 F52-01 before 快照的唯一增量=总控 13:06:53Z 新写的本审查任务书；(c) 审查窗口内 `.mimosa` 会话钩子文件有写入但被 gitignore，绑定面枚举计数实测 0，无影响。

## 边界声明

- 只写本目录 `artifacts/rust-tauri/R05/RR3/F52-REVIEW-01/`（REVIEW.md、commands.jsonl、rec.py、review_classify.py、binder_sim.py、item2_receipt_check.py、item4_f51_surface.py、item6_tamper_check.py、review-classification-main.json、review-classification-final.json、out-*/err-*）；主树其余一切只读（全部 git 读带 `--no-optional-locks`，未运行 cargo/构建）；迁出目的地只读；未做任何主仓 Git 写操作（`git init/commit` 仅发生于 /private/tmp 隔离 scratch 仓且已删除）；无系统变更；未派子代理；未修改任何被验对象（收尾复查：两哈希仍 8c23d0d9…/3016f7ae…、HEAD 未变、回执/标记/目的地文件 mtime 未动、绑定面仍 100% 普通文件）。

（审查完成，停写。）
