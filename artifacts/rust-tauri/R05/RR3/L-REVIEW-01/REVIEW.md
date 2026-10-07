# RR3 F53（L包）全新独立审查 — L-REVIEW-01

- 审查者：rr3_l_review_01（全新空历史，未参与 L 实施及此前任何轮）；未派子代理。
- 任务书：`docs/rust-tauri/R05/repair-current/RR3_L_REVIEW_BRIEF.md`（全文已读）；并全文/定点读取
  RR1/RR2 MASTER、RR3_BRIEF/REVIEW_BRIEF、RR3_F53_BRIEF、最新 RR3_ISSUE_MATRIX.json（F53 行）/RR3_PROGRESS/RR3_HANDOFF、
  FINAL-03/STAGE_REVIEW.md §四失败项1、L-01/REPORT.md 及其 evidence 日志、
  `rust/crates/lingxi-service/tests/r04_t08_tool_matrix.rs` 当前版本与 `git diff HEAD` 中该文件改动。
- 环境：HEAD=`b3ac0e6aeae8d7d530a6ab8657a5fa9c4f0b`（分支 codex/rust-tauri-migration）；cargo 一律绝对
  `/Users/study_superior/.cargo/bin/cargo`；28 核 macOS arm64；审查时间窗 2026-10-07T16:45Z–16:57Z（各命令 UTC 见 commands.jsonl）。
- 红线遵守：只写 `artifacts/rust-tauri/R05/RR3/L-REVIEW-01/`（本目录）；仓库其余只读（开工/收尾三哈希基线
  `verify-baseline.sh` 全程 BASELINE-OK，工作树测试文件零变化）；零 Git 写（HEAD/reflog 收尾仍 b3ac0e6ae 顶条、
  无暂存变化）；无系统变更（负载实验的 26 个 `yes` 燃烧进程已全部 kill，`pgrep` 复核 0 残留）；
  变异与篡改实验全部在 `/tmp/lrev01-mutation`、`/tmp/lrev01-*` 隔离副本内完成并已还原。

## 逐项结论

### 1. 根因模型独立核 — PASS

本人独立读生产源码逐点核验（非复述 L 报告）：

- `procsupervisor.rs:1723–1811 spawn_pty`：仅 setsid + TIOCSCTTY + `set_pty_size`，全 crate 无任何
  termios/cfmakeraw/TCSETS 调用（grep 零命中）→ 子终端运行**默认行规程 ICANON+ECHO**。前提成立。
- `exectools.rs:1088 run_write_stdin`：非空 chars 先 `pty_write`（:1145）后 `pty_deliver`（:1153）；
  空 chars 为纯读。与 harness 注释一致。
- `procsupervisor.rs:956 deliver_since_cursor`：非 force 路径交付"最长完整 UTF-8 前缀"并**精确消费**这些字节
  （:1002–1028 推进 per-chunk consumed、完全消费的 chunk 出队）——纯 ASCII 流每次交付消费在场全部字节。
- `procsupervisor.rs:2960 pty_read_loop`：单一顺序读循环，transcript 严格按到达（流）序追加。
- 测试子进程为 `cat`（测试 :1943 `{"cmd":"cat","tty":true}`）。

由此独立重建模型：一次 marker 写入在 master 读流中产生**恰好两份**可观察拷贝——行规程回显（写入时即产生）
与 `cat` 读走该行后的回环拷贝（cat 被调度后才产生）；流序上回显先于 cat 拷贝（行完整可读前回显已发出）。
旧屏障在第一份（回显）到达即退出，第二份在途；下一次 send 的首次 `pty_deliver` 按"光标后新字节"语义**正确地**
交付迟到的旧 TAIL 拷贝 → `terminal-snapshot-current-transcript` 判 0 → `r04_t08_tool_matrix.rs:103` 红。
产品行为在红跑中同样正确——测试就绪屏障缺陷，非产品光标语义缺陷，与 FINAL-03 定性一致。

加固后的确定性论证本人认可：单顺序读循环 + 流序（回显先行）+ "cat 拷贝是本次写入可产生的最后字节" ⟹
观察到第 2 份时全部 marker 字节已追加且已消费（ASCII 全消费）⟹ 光标可证越过整个旧 marker ⟹ 下一次交付
只可能含新输出。若环境回显假设失效（如关 ECHO），屏障在 5s deadline 处携带已收集文本响亮失败，非静默放宽。

与 FINAL-03 分布相容性：正常负载 cat 往返 << 40ms 轮询间隔，旧屏障下两份通常同窗交付、光标一并越过 → 5/6 绿；
仅嵌套最深层持续 ~50 分钟满载后 cat 拷贝被饿过下一 send 的交付窗口 → 1/6 红、且红点恰为 :103 快照断言。
flake 的存在本身佐证双拷贝模型（单拷贝世界里旧 marker 不可能在快照窗口重现）。相容。

### 2. diff 审查 — PASS

- `git diff HEAD -- rust/crates/lingxi-service/tests/r04_t08_tool_matrix.rs` 全 57 行逐行审毕（开工时
  SHA256=`3a9365d402e6…`，收尾复算相同）：仅两个 hunk，全部位于 `send_and_expect` 及其上方——新增文件内私有
  `const MARKER_OBSERVABLE_COPIES: usize = 2`、helper `count_occurrences`、文档注释；循环条件
  `!collected.contains(marker)` → `count_occurrences(&collected, marker) < MARKER_OBSERVABLE_COPIES`；失败消息升级。
- 断言语义一字未变：`snapshot_ok = snapshot_output.contains("SNAPSHOT-MARKER") && !snapshot_output.contains("TAIL-MARKER")`
  及全部 `record_case` 调用与 HEAD 逐字节相同（evidence-assert-region-*.txt，仅行号整体 +28 偏移）；
  期望仍为"只交付新输出"（含新标记、不得含旧标记）。
- 无掩盖手段：无新增 sleep（仅原有 40ms 轮询与原 5s deadline 原样保留）、无跳过、无非存在性弱化
  （contains && !contains 强度不变）、无吞失败的重试循环（断言失败即 panic）。
- 改动归属：该文件是 L 唯一改动文件；工作区其余 M/未提交文件（stage_map.rs 等）属并行包（F54/M、F50/J 等，
  见 RR3_ISSUE_MATRIX/PROGRESS 登记），本轮 diff 审查对象为该测试文件的上述改动，未发现越界。

### 3. 亲跑 — PASS（全部真实执行，逐命令 exit 记录于 commands.jsonl）

- 定向 ≥15：warm-up 1 次 + 连跑 16 次 `cargo test … --test r04_t08_tool_matrix terminal_family_share_cases -- --exact`
  = **17/17 exit=0**（evidence-targeted-run0.log / evidence-targeted-16runs.log，单次 ~0.2s）。
- 负载 ≥5：26×`/usr/bin/yes` 燃烧 + `/tmp/lrev01-load-target` 冷编译并发下 **6/6 exit=0**
  （evidence-load-6runs.log，load 14→22 爬坡中）。
- 完整套件一次：**满载下**（26 燃烧持续、1-min load 峰值 **27.77/28 核**，超 L 记录的 26.66、即 FINAL-03 失败形态）
  `cargo test … --test r04_t08_tool_matrix` → **exit=0，10 passed / 0 failed，300.60s**
  （evidence-full-suite-under-load.log；含加固后的 terminal 用例在满载下 ok）。
- clippy：crate 级 `-p lingxi-service --all-targets --locked -- -D warnings` → **exit=0**（14.46s，0 warning/0 error）；
  workspace `--workspace --all-targets --locked -- -D warnings` → **exit=0**（0.13s 暖缓存如实记录，crate 级为真实重检）。
- fmt：crate 级 `-p lingxi-service -- --check` → **exit=0**（零输出）；本轮 workspace `--all -- --check` 亦
  **exit=0**（M 包 stage_map.rs 的格式差异在 L 轮存在、本轮已消失，非 L 评审对象，两轮均不判 FAIL）。
- cargo 文件锁等待：本轮未遇明显阻塞（M 并行构建与本人命令共用主 target 时为正常锁语义，brief 预先豁免）。

### 4. 检测力控制（自设变异，隔离副本）— PASS

- 变异一（无效设计，如实记录）：在 `/tmp/lrev01-mutation` 副本 `deliver_since_cursor` 入口将全部保留 chunk 的
  consumed 重置 0（"回绕光标"）→ 测试**绿**（evidence-mutation1-ineffective-green.log）。事后归因：完全消费的
  chunk 在同一交付内已出队，环中仅存未消费尾——该变异实际**未违反**契约（无旧字节被再交付），绿是正确结果。
  此阴性结果反而核证了消费/出队语义，并说明变异必须真实破坏契约才算数。
- 变异二（有效设计，与 L 的冻结光标法不同层）：`pty_read_loop` 幻影重放——每次记录新读之前先重附上一次读的字节，
  伪造"旧输出当作新输出到达"（读循环层破坏"只交付新输出"）→ **exit=101**，红点恰为
  `tests/r04_t08_tool_matrix.rs:103`：`case terminal-snapshot-current-transcript: pinned expectation 1 did not hold
  (observed 0)`（与 FINAL-03 flake 同一钉定断言；evidence-mutation2-red.log）。
- 还原绿：恢复原文件（procsupervisor.rs SHA256 复算 = 主树 `d586aa01…` 相同）→ 同命令 **exit=0**
  （evidence-mutation-reverted-green.log）。加固未削弱检测力。

### 5. 方法自控 — PASS

- 比对基线（开工冻结于本目录 baseline-hashes.txt：diff/HEAD 版/工作版三 SHA256 + verify-baseline.sh 复算脚本）：
  - Leg A 篡改基线记录副本（翻 1 个 hex 字符）→ 校验器报 `MISMATCH diff-of-test-file: now=…e6… baseline=…e7…`，
    **exit=1**（真实值不匹配，非缺文件路径）。
  - Leg B 篡改比对对象（隔离副本内将 `MARKER_OBSERVABLE_COPIES: usize = 2` 改 3，单字节级）→ SHA256 立即失配
    （`59250421…` ≠ `899dc829…`）判 OBJECT-TAMPER-DETECTED；还原后复匹。
  - 收尾 `verify-baseline.sh` → BASELINE-OK（审查全程被审对象未被任何人动过）。

## mustFix

**无。**

## 附注（非阻断，如实）

- L-01/REPORT.md 的全部关键声明（根因机制、改动范围 +32/−4、自检命令与 exit、变异红绿、fmt 归因）与本人独立
  复核一致；唯一口径差异：L 报告 workspace fmt 当时 exit=1（stage_map.rs 两处 Diff），本轮同一命令已 exit=0
  （M 包并行演进所致），两轮各自如实，均不影响 L 义务判定。
- 本审查的"负载下完整套件绿"为 macOS arm64 本机结果，不代替其他平台；F54（M 包）不在本评审范围。
- 建议：F53 可关闭（PASS、无 mustFix）；后续 FINAL-04 按其 brief 另派全新阶段审查者。

## 产物清单（本目录）

REVIEW.md、commands.jsonl（14 条真实命令/exit/UTC）、baseline-hashes.txt、verify-baseline.sh、
evidence-targeted-run0.log、evidence-targeted-16runs.log、evidence-load-6runs.log、evidence-load-compile.log、
evidence-full-suite-under-load.log、evidence-clippy-crate.log、evidence-clippy-workspace.log、evidence-fmt-crate.log、
evidence-fmt-workspace.log、evidence-mutation1-ineffective-green.log、evidence-mutation2-red.log、
evidence-mutation-reverted-green.log、evidence-assert-region-head.txt、evidence-assert-region-working.txt。

完成后停写。
