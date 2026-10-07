# RR3 F53（L 包 / L-01）：terminal_family_share_cases 满载时序 flake 加固 — 实施报告

- 实施者：rr3_l_impl_01 fresh（空历史，未参与此前任何轮）
- 任务书：docs/rust-tauri/R05/repair-current/RR3_F53_BRIEF.md（全文已读；FINAL-03/STAGE_REVIEW.md §四失败项1、RR3_ISSUE_MATRIX.json F53 行已读）
- 时间窗：2026-10-07T16:20Z–16:40Z（全部命令 UTC 时间戳见 §四）
- 对象：`rust/crates/lingxi-service/tests/r04_t08_tool_matrix.rs`（唯一改动文件；该文件改动前与 HEAD 一致，git status 证实干净起点）
- HEAD：b3ac0e6ae（分支 codex/rust-tauri-migration）；无任何 Git 写操作

## 一、根因（亲核生产源码后定位）

**一句话**：`send_and_expect` 的退出条件（"观察到 marker 文本一次"）不是充分的就绪屏障——PTY 默认行规程（ECHO+ICANON，`spawn_pty` 无任何 raw-mode 设置，procsupervisor.rs:1723–1811 无 termios 调用）使一次写入的 marker 在 transcript 中按流序出现**两次**：内核行规程回显（写入时产生）+ 子进程 `cat` 的回环拷贝（cat 被调度后才产生）；循环在第一份（回显）到达即退出，第二份（cat 拷贝）仍在途，于是**下一次** `send_and_expect`（SNAPSHOT 阶段）的首次 `pty_deliver` 按"光标后新字节"语义**正确地**交付了旧 TAIL-MARKER 的迟到拷贝，快照断言（正确地）判红 → `terminal-snapshot-current-transcript: pinned expectation 1 did not hold (observed 0)`（r04_t08_tool_matrix.rs:103）。

机制链（全部亲核源码）：
1. `run_write_stdin`（exectools.rs:1088）：先 `pty_write`（写 master=行规程输入），后 `pty_deliver`（交付光标后未消费字节）。
2. `TranscriptCore::deliver_since_cursor`（procsupervisor.rs:956）：非 force 交付消费"最长完整 UTF-8 前缀"——本用例纯 ASCII，每次交付**消费全部在场字节**；被完全消费的 chunk 出队，旧字节永不重现。
3. `pty_read_loop`（procsupervisor.rs:2960）：单顺序读循环，transcript 追加严格按流序。
4. 流序保证：一行的回显在该行对 cat 可读之前已发出（行规程先回显、行才完整），故 cat 拷贝在流中必晚于回显拷贝。
5. 正常负载：cat 往返 << 40ms 轮询间隔，两份拷贝落在同一轮 → 光标越过两份 → 绿（5/6）。满载 ~50 分钟（FINAL-03 嵌套 R03 层）：echo 落在第 N 轮、cat 拷贝落在第 N 轮之后 → 快照窗口捕获旧 marker → 红（1/6）。该 flake 的存在本身即双拷贝机制的证明（单拷贝世界里循环退出即全部消费，快照不可能看到旧 marker）。

定性：**测试就绪屏障缺陷**（harness 前置条件不健全），非产品光标语义缺陷——与 FINAL-03 定性一致；产品"只交付新输出"行为在红跑中也是正确的（它交付的确实是光标后的新到字节）。

## 二、修法（最小加固，断言语义逐字不变）

`send_and_expect` 退出条件从 `collected.contains(marker)`（≥1 次）改为 `count_occurrences(collected, marker) >= MARKER_OBSERVABLE_COPIES`（=2：行规程回显 + cat 回环）。deadline 驱动（维持原 5s，无新增 sleep、无放宽），失败消息升级为含观察份数与已收集文本的诊断。

正确性论证（确定性，非概率）：单顺序读循环 + 流序保证 + "echo 先于 cat 拷贝" ⟹ 观察到第 2 份（cat 拷贝）时，该 marker 写入可产生的全部字节均已追加**且已被本轮交付消费**（ASCII 全消费）⟹ 光标可证越过整个旧 marker ⟹ 下一次 send 的交付只可能含真正的新输出——快照断言的前提变为健全，满载下不再有时序歧义。若未来环境关闭 tty 回显，屏障会在 deadline 处**响亮失败**并打印已收集文本（诚实的环境假设失败），绝不静默放宽断言。

改动：+32/−4 行，仅 `send_and_hint` 所在段落——新增文件内私有 const `MARKER_OBSERVABLE_COPIES` 与 helper `count_occurrences`（均在测试文件内，无共享基建改动）；`terminal-snapshot-current-transcript` 断言本身（contains SNAPSHOT && !contains TAIL，1936–1943 行区域）**未动一个字符**。生产源码/stage map/pins/其他测试零改动（git status 佐证：本文件外的工作区改动均为其他包在途内容，早于本任务存在且未触碰）。

## 三、变异红绿对照（检测力证明）

- 变异（隔离副本 /tmp/l01-mutation，仅副本内）：`procsupervisor.rs` `deliver_since_cursor` 光标推进冻结（`let mut remaining = consumed;` → `let mut remaining = 0usize;`）——即"交付回放全部保留 transcript"，直接破坏"只交付新输出"契约。
- 红：加固后的测试 `terminal_family_share_cases` exit=**101**，失败点恰为 `case terminal-snapshot-current-transcript: pinned expectation 1 did not hold (observed 0)`（r04_t08_tool_matrix.rs:103，与 FINAL-03 flake 同一钉定断言）→ 屏障未削弱检测力。
- 还原绿：恢复生产文件后同命令 exit=**0**（1 passed, 0.20s）。
- 证据：evidence-mutation-red.log / evidence-mutation-reverted-green.log

## 四、自检命令与结果（cargo 一律绝对路径 /Users/study_superior/.cargo/bin/cargo）

(a) 正常单跑：`cargo test --manifest-path …/rust/Cargo.toml -p lingxi-service --test r04_t08_tool_matrix terminal_family_share_cases -- --exact` → **exit=0**（1 passed, 0.22s, 2026-10-07T16:25Z；首跑即证实双拷贝模型成立——若 echo 不存在该跑会 5s 后响亮超时）

(b) 定向连跑 20 次（同命令）：run-1…run-20 **全部 exit=0**（16:25:42Z–16:25:48Z，逐次记录于会话输出；单次 ~0.18s）

(c) 负载下 15 次（≥5 要求）：
- 负载形态：26×`yes` CPU 燃烧进程（28 核）+ 冷编译并发（CARGO_TARGET_DIR=/tmp/l01-load-target `cargo test … --no-run`，exit=0）+ 完整并行套件并发（同 target dir `--test-threads=8`）。
- load-run-1..5（编译爬坡，load 7.5→）**exit=0**；load-run-6..10（load ~16）**exit=0**；load-run-11..15（与完整并行套件真并发，load 峰值 **26.66/28 核**）**exit=0**（16:26:13Z–16:27:03Z）。
- 并发完整套件本身（含加固后的 terminal 测试、满载下）：10 passed / 0 failed，300.69s，exit=0 —— 与 FINAL-03 失败形态同构（满载嵌套全量并行）下全绿。

(d) 变异红→还原绿：见 §三（红 exit=101 / 绿 exit=0）。

(e) 完整套件一次（主树默认 target dir）：`cargo test … -p lingxi-service --test r04_t08_tool_matrix` → **exit=0**，10 passed / 0 failed / 0 ignored，300.66s（16:33Z–16:38Z；evidence-full-suite.log）

(f) fmt/clippy（R03 stage map 钉定口径）：
- clippy：`cargo clippy --manifest-path …/rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` → **exit=0**（38.97s，16:39Z；evidence-clippy.log）
- fmt：`cargo fmt --manifest-path …/rust/Cargo.toml --all -- --check` → **exit=1，但两处 Diff 全部位于 `rust/crates/xtask/src/stage_map.rs:2377/2422`——非本包文件**。归因证据：该文件是工作区中**他包（M/F54 域，diff 内容含 F54 字样）在途未提交修改**（git status M；HEAD 版本 rustfmt --check exit=0/零 diff），本任务未触碰该文件；本任务唯一改动文件 `rustfmt --check` **exit=0**（单文件核验）。按红线（不改其他包文件）不代为格式化，如实报告：本包 fmt 义务绿，工作区 fmt 红系他包未提交内容所致，恢复方式为该包自行 `cargo fmt` 或还原其未提交编辑。

## 五、红线遵守

仅改 `rust/crates/lingxi-service/tests/r04_t08_tool_matrix.rs`（helper 均为该文件私有，未涉及共享基建）；生产源码/stage map/pins/其他测试零改动；零 Git 写；零子代理；>10 分钟风险命令（负载编译/并行套件）以后台会话脱离宿主运行。变异实验仅存在于 /tmp 隔离副本并已验证还原。

## 六、结论

F53 的时序不确定性已由"第一份可见即触发下一阶段"改为"全部可观察拷贝交付完毕才触发"的就绪屏障，确定性来自流序+全消费的可证明性质，断言语义逐字不变且变异红证明检测力未削弱；六项自检中 (a)(b)(c)(d)(e) 与本文件 fmt、全工作区 clippy 全绿，全工作区 fmt 的唯一红项可证明归属他包在途未提交文件（HEAD 本身绿）。建议交 L-REVIEW-01 全新独立审查；审查时如 M 包已提交 stage_map.rs，全工作区 fmt 应自然转绿。
