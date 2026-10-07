# RR3 F51-01 — 嵌套 .git 证据夹具迁出主树（实施报告）

- 实施者：RR3 F51 全新空历史修复智能体（未参与 RR3 任何实施/审查/终审轮；未派子代理）。
- 任务书：`docs/rust-tauri/R05/repair-current/RR3_F51_BRIEF.md`（全文读取）；另全文读取 RR1_MASTER_PROMPT_2026-10-04、RR2_MASTER_PROMPT_2026-10-06、RR3_BRIEF、RR3_REVIEW_BRIEF、RR3_FINAL_BRIEF（含尾部派发附加事实）、RR3_ISSUE_MATRIX.json（F51 行）、RR3_PROGRESS.md、RR3_HANDOFF.md，以及根因证据 `RR3/FINAL-01/STAGE_REVIEW.md`（§四）与 `FINAL-01/command-records/frozen-inputs.json`（binder_observation 56 条目）。
- 证据目录：本目录 `artifacts/rust-tauri/R05/RR3/F51-01/`。完成本报告后停写。

## 一、环境与基线

- HOME=`/Users/study_superior`（无需修正）；cargo 一律绝对路径 `/Users/study_superior/.cargo/bin/cargo`。
- HEAD=`b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`，分支 `codex/rust-tauri-migration`；`git ls-remote origin codex/rust-tauri-migration` 同 SHA（远端 HEAD/main=7d1a0c6b… 为默认分支前移，与本任务无关，只读观察）。
- 磁盘：Data 卷可用 ≥557Gi；源与目标同一文件系统（device 16777231，`stat -f %d` 双侧相等）⇒ 迁出即 rename(2)，字节保持、无复制。
- 迁出前基线（`baseline-git-state.txt`）：`git status --porcelain` 74 项 = 25 个 tracked 修改（与 FINAL-01 STAGE_REVIEW 记录的 25 M 一致）+ 49 个 untracked（`artifacts/rust-tauri/R05/RR3/` 折叠为单条 `??`）；三重基线哈希：status 全量 `c9859c9d…`、`git diff HEAD` `8c23d0d9…`、`git ls-files --cached -s` `3016f7ae…`。
- UTC：开工 2026-10-07T11:44Z，迁出全部完成 2026-10-07T11:54:56Z。

## 二、权威再枚举与交叉核对（任务 1）

命令：`git ls-files --cached --others --exclude-standard` 取尾斜杠目录条目（exit 0）。
结果：**56 条，与 FINAL-01 frozen binder_observation 清单逐条双向比对完全一致**（in_final01_not_current=0，in_current_not_final01=0），无多无少，无需追加处置。56 条目均为含嵌套 `.git`（dir 或 file）的目录；无条目互为祖先；斜杠编码名无冲突。

## 三、方法（任务 2/3）

- 迁出：逐条目 `os.rename`（= 同卷 `mv`，字节保持；不用复制重写）到 `/Users/study_superior/Desktop/Code/LingxiAgent-RR3-localonly-fixtures/<原相对路径 '/'→'_'>/`。
- 摘要（前后同法）：`digest_tree.py` 对每目录生成规范化清单（相对路径排序；类型/大小/逐文件 SHA256；符号链接哈希其目标字节串、绝不跟随）取 tree_digest=SHA256(清单)，另记 file_count/symlink_count/total_bytes/du -sk/嵌套 `git rev-parse HEAD`；run-3 扫描器为 fd 递归（scandir(fd)+dir_fd stat/open），清单格式与 run-1 完全一致，且免受 macOS PATH_MAX(1024) 限制。
- 防碰撞/防丢：驱动含门禁（存在性、嵌套 .git、条目互不嵌套、编码名唯一、目的地不预占），每条目前摘要写入崩溃安全检查点 `relocation-checkpoint.json`；**全程零删除，仅 rename**。
- 迁出后在每条目原父目录写 `RELOCATED-F51.json`（共 **41 个**父目录；列出被迁子目录名、旧相对路径、新绝对路径、回执指针；未改动父目录其他历史文件）。

## 四、执行过程：三次运行（如实记录）

- **run 1**（11:47:31–11:47:49Z，exit≠0，日志 `run1-abort-log.txt`）：1–39/56 完成迁出且前后摘要相等；第 40 条 `A-REVIEW-02/independent/internal-fresh/repo/`（约 100 层递归嵌套 `candidate-copy` 夹具）rename 成功、字节完好，但基于绝对路径的 post 扫描在最深路径触发 `OSError ENAMETOOLONG`——斜杠编码目标前缀（+24 字符）使最深绝对路径超过 PATH_MAX 1024。进程因未捕获 OSError 未写回执；**未删任何字节**，56 份前摘要已在检查点。
- **run 2**（11:53:23Z，exit 1，`RELOCATION-RECEIPT.json.aborted-run2`）：续跑门禁语义错误——把「已迁出、不再出现在实时枚举」的 40 条误判为 FINAL-01 条目缺失即中止；**零文件系统改动**。
- **run 3**（11:54:44–11:54:56Z，exit 0）：三态续跑（在主树/已在目的地/缺失）。对仍在主树的 16 条用 fd 扫描器刷新前摘要并与 run-1 检查点逐字段核对相等；第 40 条目的地由编码名 rename 为短名 **`40--repo`**（实测 max_relpath_len=902；70+8 前缀 ⇒ 最深绝对路径 983<1000 安全线），恢复全树可达后摘要与迁出前相等；其余 55 条用斜杠编码名。**56/56 前后摘要逐字段相等（file_count/symlink_count/other_count/total_bytes/tree_digest/嵌套 HEAD）**。

## 五、结果总量

- 条目：**56**；普通文件 **110,071**、符号链接 **121**、其他类型 0；总字节 **5,324,827,710**（回执口径含符号链接目标字节串共 1,017 字节；纯普通文件字节 5,324,826,693，与开工时独立 os.walk 统计一致）。最大条目 `J-REVIEW-01/copy`（102,887 文件 / 5,130,434,169 字节 / 嵌套 HEAD=b3ac0e6a…）。
- 目的地根无杂项条目（除 56 个条目目录外为空）。
- 完整逐条目前→后映射、前后摘要、UTC：`RELOCATION-RECEIPT.json`（56 entries + 41 markers + gates + 三次运行史）。

## 六、五项验证（任务 5）

| # | 验证 | 命令/证据 | 结果 |
|---|---|---|---|
| a | 目录条目枚举清零 | `git ls-files --cached --others --exclude-standard \| grep '/$'` | **PASS**：输出 0 行，grep exit=1（`verify-ab.txt`；驱动内独立复核同零） |
| b | tracked 修改集零变化 | `git status --porcelain` 过滤非 `??` 与基线逐行 diff；status 全量/`git diff HEAD`/`git ls-files -s` 三哈希前后对照 | **PASS**：tracked 集 25 项逐行一致；三哈希全等（`8c23d0d9…`/`3016f7ae…`/`c9859c9d…`）。untracked 集亦逐行一致——`?? artifacts/rust-tauri/R05/RR3/` 原本就是单条折叠条目故不减少；F51-01 回执与 41 个 RELOCATED-F51.json 均新增于既有 untracked 目录内，未触任何 tracked 文件 |
| c | 抽样新位置字节可读且摘要相等 | 独立 `digest_tree.py` CLI 重扫 + 与回执比对 + 实读字节 | **PASS（含一处事后事件，见 §七）**：A-02/copy-delete-rename（`291f5329…`）、40--repo 深树（`d6de2b1e…`，最深文件 160 字节实读可读）、I-REVIEW-01/permanent-final/runner-copy（`12e22b46…`）、J-REVIEW-02/i-restore/copy（`947c4b3e…`）、I-01/selfcheck-04/runner-copy（`1f938ad0…`）5 个抽样前后全等；J-REVIEW-01/copy 在迁出时刻与 11:56Z 独立重扫均为 `79736540…` 与回执相等，其后被我方验证命令改写一个文件（§七） |
| d | 生产不引用 | 对 scripts/、rust/、docs/ 全部现行文件（tracked+untracked）逐条目子串扫描 | **PASS**：命中 1 处 = `docs/rust-tauri/R05/repair-current/RR3_F51_BRIEF.md`（untracked 流程任务书自身的示例路径，非生产文件）；**tracked 生产文件 0 命中**，与 FINAL-01 预检一致（`verify-d-and-final-audit.txt`） |
| e | A 包绑定器回归不退化 | `/Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p xtask candidate`（先 `-- --list` 确认目标） | **PASS**：exit 0；**9 passed / 0 failed / 0 ignored / 0 measured / 112 filtered out**（0.84s；`verify-e-xtask-candidate.txt`）。candidate.rs 零改动（tracked 哈希证明） |

## 七、事后事件（如实上报）：一个文件被本方验证命令改写

`verify-c.txt` 抽样中的 `git -C <J-REVIEW-01_copy> status --porcelain` 触发 git 的 opportunistic index stat-refresh，于 11:55:40Z 原地重写该夹具 `.git/index`（迁出完成于 11:54:56Z，且 11:56Z 的摘要重算发生在该命令之前、仍与回执相等）。证据与圈定（`verify-c-incident.json`、`verify-c-incident-diagnosis*.txt`、`verify-c3-postmutation-audit.txt`）：

- 全部 56 个目的地 `find -newermt`（迁出窗口后）写入仅此一个路径；
- file_count/total_bytes 不变，仅该文件内容字节变化；当前摘要 `fee8e52b…` 且 20 秒双扫稳定；
- 语义完好：`git --no-optional-locks ls-files -s | sha256` = `3016f7ae…`，与主仓库 index 哈希完全一致；
- 迁出本身字节保持的结论不受影响（回执时刻 pre==post 已独立复核）；预突变 `.git/index` 字节（stat 缓存字段）无处归档，无法字节级还原，如实记录不伪造。审查者复查该夹具时请用 `git --no-optional-locks` 或 plumbing 只读命令。

## 八、红线遵守

未修改 rust/、scripts/、docs/ 任何现行文件（b 项三哈希证明）；未改 .gitignore；未排除整个 artifacts/；未删除任何字节（仅 rename；唯一删除动作是移除本目录自产 `__pycache__` 缓存，非证据）；无 Git 写操作（仅 rev-parse/ls-files/status/diff/ls-remote 只读）；未派子代理；未触碰 /private/tmp rr3-* 历史材料与用户内容；cargo 绝对路径 + HOME 核验。`cargo test` 写入的 `rust/target/` 为构建缓存（FINAL-01 阶审同样产生），非源码改动。

## 九、偏差清单（全部）

1. 第 40 条目目的地命名 `40--repo` 偏离斜杠编码惯例：PATH_MAX 物理约束（编码前缀+最深相对路径 902 > 1000 安全线），记录于回执 naming_deviations 与原位标记。
2. 执行历经 3 次运行（run1 ENAMETOOLONG、run2 续跑语义门禁误判、run3 成功），两次中止均零损失并留档；未隐瞒。
3. §七 `.git/index` 单文件事后改写事件（本方验证命令所致，非迁出所致）。
4. 总字节口径注：回执 total_bytes 含符号链接目标串 1,017 字节。
5. 验证 (b) 的 untracked 集与基线逐行相同（而非"减少 ?? 条目"）：RR3/ 在基线中本就折叠为单条 `??`。

## 十、产物清单（本目录）

`REPORT.md`（本文件）、`RELOCATION-RECEIPT.json`（正式回执）、`relocation-checkpoint.json`（56 前摘要检查点）、`digest_tree.py`、`run-relocation.py`、`baseline-git-state.txt`、`selfcheck-output.txt`（run3 全程）、`run1-abort-log.txt`、`RELOCATION-RECEIPT.json.aborted-run2`、`selfcheck-output.txt.aborted-run2`、`verify-ab.txt`、`verify-c.txt`、`verify-c2.txt`、`verify-c3-postmutation-audit.txt`、`verify-c-incident.json`、`verify-c-incident-diagnosis.txt`、`verify-c-incident-diagnosis2.txt`、`verify-d-and-final-audit.txt`、`verify-e-xtask-candidate.txt`；主树内 41 个 `RELOCATED-F51.json` 原位标记（路径清单见回执 markers）。

## 十一、下一棒

交总控另派全新 F51 独立审查者（建议复核：枚举清零、回执前后摘要、抽样含 40--repo 与 J-REVIEW-01/copy（注意 §七）、xtask candidate 亲跑）；其后 FINAL-02 换全新阶段审查者执行
`/Users/study_superior/.cargo/bin/cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R05 --evidence artifacts/rust-tauri/R05/RR3/FINAL-02/verify-R05`。

（实施完成，停写。）
