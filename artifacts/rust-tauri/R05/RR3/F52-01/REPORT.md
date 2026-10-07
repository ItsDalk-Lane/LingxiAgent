# RR3 F52-01 修复报告：迁出唯一 symlink 夹具 independent-validator-bin，绑定面清零

- 实施者：RR3 F52 全新空历史修复实施者（未参与此前任何轮，未派子代理）。
- 任务书：`docs/rust-tauri/R05/repair-current/RR3_F52_BRIEF.md`，全文读取；已按要求读取
  FINAL-02/STAGE_REVIEW.md（根因与权威枚举）、RR3_F51_BRIEF.md（纪律蓝本）、F51-01/RELOCATION-RECEIPT.json
  与 A-REVIEW-02/RELOCATED-F51.json（回执/标记格式）、最新 RR3_ISSUE_MATRIX.json（F52 行 OPEN）、
  RR3_PROGRESS.md 尾部、RR3_HANDOFF.md；并只读核实 `rust/crates/xtask/src/candidate.rs` 326-369 行组件遍历语义。
- 开工环境：HOME=`/Users/study_superior`（正确，无需 export）；HEAD=`b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`
  （分支 `codex/rust-tauri-migration`，与 FINAL-02 冻结时点一致）；df(/) 可用 557Gi；
  PATH python3=`/Library/Frameworks/Python.framework/Versions/3.14/bin/python3`（3.14.3，即被迁 symlink 的目标解释器，
  系统位置不动）。开工 UTC 2026-10-07T13:00:36Z。

## 一、迁出（唯一范围条目）

| 项 | 值 |
|---|---|
| 源（旧相对路径） | `artifacts/rust-tauri/R05/RR3/A-REVIEW-02/independent-validator-bin/`（未跟踪，2 条目） |
| 目的地（新绝对路径） | `/Users/study_superior/Desktop/Code/LingxiAgent-RR3-localonly-fixtures/artifacts_rust_tauri_R05_RR3_A-REVIEW-02_independent-validator-bin` |
| 目的地预检 | 目的地根已存在（F51 既有 56 条目，本轮未触碰任何一条）；目标条目名 `ls` exit 1（无同名冲突） |
| 同卷证明 | repo st_dev=16777231 == 目的地根 st_dev=16777231（同 APFS 卷，mv=rename(2)，字节保持） |
| 命令 | `mv artifacts/rust-tauri/R05/RR3/A-REVIEW-02/independent-validator-bin /Users/study_superior/Desktop/Code/LingxiAgent-RR3-localonly-fixtures/artifacts_rust_tauri_R05_RR3_A-REVIEW-02_independent-validator-bin` |
| exit | 0（2026-10-07T13:01:41Z 前后，界于 13:01:30.80Z 前摘要与 13:02:05.57Z 后摘要之间）；源目录随后 `ls` exit 1（已不在主树） |

### 迁出条目前后清单（前后同法：python3 os.lstat/readlink/sha256）

| 文件 | 形态 | 前 | 后 | 相等 |
|---|---|---|---|---|
| `git` | 普通文件 0755/255B | sha256 `f9dc56ed2d08e1f687ed2e39c70e00f629cd485136bdea7e21b2bfaa1c0afbf8`，ino 276456367，mtime_ns 1791334791598252451 | 同左逐字段 | **全等**（inode 保持=字节未动） |
| `python3` | 符号链接 0o120755/62B | readlink 目标串 `/Library/Frameworks/Python.framework/Versions/3.14/bin/python3`，ino 276456368 | 同左逐字段 | **全等**（目标串逐字相同） |

原始证据：`source-dir-digests-before.json`（UTC 13:01:30.80Z）、`dest-dir-digests-after.json`（UTC 13:02:05.57Z）。

## 二、原位标记

- `artifacts/rust-tauri/R05/RR3/A-REVIEW-02/RELOCATED-F52.json`（沿用 RELOCATED-F51 标记格式：
  marker/receipt/relocated_children{name,new_absolute_path,old_relative_path}/task/written_utc），
  写入 UTC 2026-10-07T13:02:16.66Z；A-REVIEW-02 其余历史文件（含 RELOCATED-F51.json、REVIEW.md）零改动。

## 三、绑定面全量重分类（本轮新义务，穷尽形态）

- 方法：`F52-01/classify_binder_surface.py`——以候选绑定器同语义（`rust/crates/xtask/src/candidate.rs:326-369`
  组件遍历 + symlink_metadata 不跟随）对 `git --no-optional-locks ls-files --cached --others --exclude-standard -z`
  全量条目逐条 lstat 分类（ancestor_symlink / final_symlink / directory_entry / ancestor_not_dir / irregular /
  missing / regular_file），`--assert-clean` 时非 100% 普通文件即 exit 1。
- **Run A（迁出前）**：total 69,123 = regular_file 69,122 + **final_symlink 1** + 0 目录/0 祖先链接/0 irregular/0 missing；
  唯一非普通条目即 `artifacts/rust-tauri/R05/RR3/A-REVIEW-02/independent-validator-bin/python3`——与 FINAL-02
  权威枚举同形态（其 69,075 为 FINAL-02 时点；其后 FINAL-02 自身写入 command-records/报告等未跟踪文件致计数上移，
  形态不变：恰 1 symlink、0 目录（F51 持续成立））。输出：`binder-surface-pre-relocation.json`。
- **Run B1（迁出后、回执前）**：total 69,126 = **100% regular_file**，其余六类全 0，`--assert-clean` exit 0。
  输出：`binder-surface-post-relocation-prereceipt.json`。
- **Run B2（终版，回执与 after 快照写入后）**：total 69,130 = **100% regular_file（69,130/69,130）**，
  final_symlink 0、ancestor_symlink 0、directory_entry 0、ancestor_not_dir 0、irregular 0、missing 0，
  `--assert-clean` exit 0。输出：`binder-surface-post-relocation.json`。
- 计数链自洽：Run A→B2 差 +7 = −2（independent-validator-bin 的 git/python3 迁出）
  +9（本轮新增未跟踪：RELOCATED-F52.json、classify_binder_surface.py、git-status-porcelain-before/after.txt、
  git-hashes-before/after.txt、source-dir-digests-before.json、dest-dir-digests-after.json、
  binder-surface-pre-relocation.json、cargo-xtask-candidate.log、REPORT.md stub、RELOCATION-RECEIPT-F52.json、
  binder-surface-post-relocation-prereceipt.json——其中 2 项 after 快照与回执、prereceipt 输出在 B1 后产生，
  B1 计 69,126 亦逐一核对吻合）。
- 枚举中 independent-validator-bin 条目零残留的直接证明：源已 mv 走，若枚举仍输出该两条目则分类必为 missing≥1；
  三次运行 missing 均为 0。

## 四、验证

### (a) tracked 修改集零变化（全部 `git --no-optional-locks` 读）

| 快照 | 迁出前 | 迁出后 | 结论 |
|---|---|---|---|
| `git diff HEAD` SHA256 | `8c23d0d944b66d76f1c65e6a8883d7782a7509d434b10bc13dda2ab5bc393361` | 同左 | **逐字节相同**（且与 FINAL-02 记录一致） |
| `git ls-files -s` SHA256 | `3016f7ae9d18d93951f05a77adb68946cab99a92d0a122294d4c5cf096407d47` | 同左 | **相同**（index 零变化） |
| `git status --porcelain` | 25 行 tracked M + 51 行 `??` | 同左 | tracked 行 diff 为空=**完全一致**；未跟踪行亦无差异——`artifacts/rust-tauri/R05/RR3/` 在 porcelain 中为单条目录级 `??` 条目，其内部迁出/新增不改变状态行（粒度说明如实记录） |

原始证据：`git-status-porcelain-before.txt`、`git-hashes-before.txt`、`git-status-porcelain-after.txt`、`git-hashes-after.txt`。

### (b) 新位置字节/链接目标相等

两 条目**全量**（非抽样）验证相等：`git` sha256 相等、`python3` readlink 目标串相等、两者 lstat 全字段
（mode/uid/gid/size/mtime_ns/ino/dev）相等；inode 前后相同（rename(2) 保 inode）为字节未动的直接证据。见 §一。

### (c) A 包绑定器回归

`/Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p xtask candidate`
（cwd=仓库根，HOME 正确未 export）：**exit 0，9 passed / 0 failed / 0 ignored / 0 measured / 112 filtered out**，
1.12s 总耗时（暖 target 0.08s 编译，如实记录）；与 F51 轮 9 passed/112 filtered 基线一致，无退化。
原始输出：`cargo-xtask-candidate.log`（含 `time` 输出）。

## 五、红线核对

- 未改 rust/、scripts/、docs/ 任何现行文件（candidate.rs 只读核实）；未改 .gitignore（porcelain/index 哈希为零变化提供旁证）；
  未改绑定器；**零删除**（仅 mv 迁移）；**零 Git 写操作**（无 add/commit/push/branch/tag；全部 git 读带
  `--no-optional-locks`，未重写任何 index——本轮迁出内容无 .git，不存在 F51 的 index-refresh 偏差面）；
  未派子代理；外置目的地只新增 `artifacts_rust_tauri_R05_RR3_A-REVIEW-02_independent-validator-bin` 一条，
  F51 既有 56 条目零触碰。

## 六、偏差与如实说明

1. **mv 精确时刻未单独打点**：mv 于两摘要之间执行（13:01:30.80Z→13:02:05.57Z），回执记录为界值而非伪精确值。
2. **Run B1/B2 两次终局分类**：B1（回执前）与 B2（回执后）均 100% 普通文件；差异仅为回执/快照等本轮新普通文件条目，
   计数链逐项吻合（§三）。本 REPORT.md 最终内容成文于 Run B2 之后（B2 时为已存在的 stub 普通文件），
   内容填充不改变文件形态/类型；成文后另做一次终态复核见文末追加行。
3. FINAL-02 枚举 69,075 与本轮 Run A 69,123 差 48：系 FINAL-02 自身收尾写入的未跟踪文件（command-records 40 文件、
   STAGE_REVIEW.md、STRUCTURED_SUMMARY.json 等），形态结论（恰 1 symlink、0 目录）不变，非树漂移——
   tracked 三哈希前后与 FINAL-02 记录逐字节一致即为证明。
4. porcelain 未跟踪粒度为目录级单条（`?? artifacts/rust-tauri/R05/RR3/`），故状态行对内部迁出无感；
   绑定面级别的清零证明以 §三 全量分类为准（这正是 F51 只查目录条目漏掉 symlink 的教训的针对性补强）。

## 七、产物清单（F52-01/）

REPORT.md（本文件）、RELOCATION-RECEIPT-F52.json、classify_binder_surface.py、
binder-surface-pre-relocation.json、binder-surface-post-relocation-prereceipt.json、
binder-surface-post-relocation.json、source-dir-digests-before.json、dest-dir-digests-after.json、
git-status-porcelain-before.txt、git-status-porcelain-after.txt、git-hashes-before.txt、git-hashes-after.txt、
cargo-xtask-candidate.log；另 A-REVIEW-02/RELOCATED-F52.json。

完成后停写，交总控另派全新 F52 独立审查者；其后 FINAL-03 换全新阶段审查者。
## 终态复核（成文后追加；追加不改文件类型）
- Run B3（本 REPORT.md 最终内容+全部产物写入后，--assert-clean）：total 69,131 = 100% regular_file，其余六类全 0，exit 0；输出持久化为 binder-surface-final-postreport.json（其本身为分类器输出的普通 JSON 文件，写入后未再运行第四次分类，计数不再包含自身）。
- porcelain tracked 行终态与迁出前 diff 为空（TRACKED FINAL IDENTICAL）；外置目的地根共 57 条=F51 既有 56+本轮 1，零触碰 F51 内容；新位置 git/python3 两条目在位。
- F52-01 停写（UTC 2026-10-07T13:06:20.102071+00:00）。
