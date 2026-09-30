# R03 修复轮 G06-R1 独立对抗性审查（F07：后台运行接受 steering 却没有传入消费通道）

- Reviewer：REVIEWER-REPAIR-R03-G06-R1（一次性独立对抗性 Reviewer，未参与 G06 候选的实现或修复）。
- 日期：2026-09-30。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。
- 审查对象（候选）：HEAD `8883923a5` + 未提交工作树 —— `rust/crates/lingxi-service/src/background.rs`（`spawn_background_drive` 任务内 `drive_run` steering 实参 `None` → `Some(_lease.steering_inbox())`，+注释，14 insertions / 2 deletions）+ 新测试 `rust/crates/lingxi-service/tests/background_steering.rs`（8 用例，C01–C04 两层自查载体）。
- 工具链：一律 `~/.cargo/bin/cargo`（1.98.1，rustup 锁定；Homebrew 1.93.0 未用），全部 `--locked`；红基线用 `/tmp` git worktree（已用后 remove）。
- 我的复测产物：`artifacts/rust-tauri/R03/repair-current/G06-R1/`（`logs/`、`evidence-green.json`）。未修改产品/测试/配置/门禁/账本/执行者证据；未 commit/push。

## VERDICT: PASS

## 候选摘要

一行接线修复 + 8 用例新集成套件。`background.rs::spawn_background_drive` 的分离任务在持有 session lease（`let _lease = lease;`）的前提下，原先给 `drive_run` 的 steering 参数传 `None`（F07 本体：已 Accepted 的后台追加输入无人消费、只能残留串入会话下一个无关 Run 的首轮）；现传 `Some(_lease.steering_inbox())` —— 与前台 `sessions.rs:685` 完全同源（同一 slot 的同一 `Arc<SteeringInbox>`）。未改 `sessions.rs`/`runs.rs`/子代理路径/任何 G01–G05 语义（`git diff --stat HEAD -- rust/` 仅 background.rs）。

## 同源与隔离审查（代码级独立核查）

1. **同一授权通道（确认）**：`steer_for`（sessions.rs:1177）→ `SessionSupervisor::steering_submit`（session_supervisor.rs:347）→ `slot.inbox.push_bounded`。`try_begin_run`（:301）返回的 `SessionLease` 持有 `Arc::clone(&slot.inbox)` —— 与 steer 写入的是**同一个 Arc**。前台（sessions.rs:685 `Some(lease.steering_inbox())`）与修复后的后台（background.rs:317 `Some(_lease.steering_inbox())`）用同一 accessor 同一对象。lease 在分离任务内存活至 settle，借用跨 await 合法。
2. **受理面无第二套 steering 通道（确认）**：`steer_for` 是唯一 steering 受理入口（前台/后台会话共用）；`deliver_retained`（session_supervisor.rs:430，子代理结果回注）推入的是**同一** inbox、同一 `push_bounded` 容量界，属投递路径而非第二受理通道。产品代码里 `drain_joined` 仅一个调用点（runs.rs:703，`drive_run` 每轮循环头、在 `gate_cancel!()` 之后）。
3. **子代理 child run 传 `None` 的"刻意保留"论证（成立）**：child（subagents.rs:769）不持有父会话 lease —— 父 Run 持有（正泊在工具调用里等 child）；`steer_for` 在此窗口接受的是**给父会话下一模型轮**的用户 steering。若 child 也 drain 父 inbox，用户 steering 会被 child 的模型轮偷走（恰是 C02 错归属反例），父 Run 永远看不到。child 结果经 `deliver_retained` 回注同一 inbox，由父恢复后的下一轮消费。隔离 lane（`{session}::subagent::{thread}`）与该论证互相印证。不改正确。
4. **一次消费 / 不重复 drain（确认）**：`drain_joined` 在锁内原子 drain 整个队列（session_supervisor.rs:163-173），每条文本恰进一个模型轮；busy gate 保证任一时刻一个会话只有一个持 lease 的 Run 在 drain。
5. **取消/终态处置契约（确认，契约 = 既有冻结语义非本轮新造）**：drain 位于每轮 `gate_cancel!()` 之后 —— 取消先赢即四相取消结算、不 drain，已接受未消费文本停留 slot（`steering_pending` 可观测、不假称已用于执行）；错过最后一轮 drain 的文本保留给会话下一 Run 首轮（`SessionLease::drop` 在 inbox 非空时保留 slot；模块注释与既有绿测 `leftover_steering_survives_the_run_for_the_session_next_turn` 钉死）；idle 会话 steer = `Miss`；steering 永不触发新 Run（仅 submission 创建）。
6. **容量前后台一致（确认）**：受理侧唯一 `push_bounded` + `steering_inbox_capacity`（`validate` 拒 0），满即 `SteeringInboxFull` 响亮拒绝不入队；后台接入后容量在轮间真实释放。

## 逐 C-ID 结果

统一命令形态：`cd rust && ~/.cargo/bin/cargo test --locked -p lingxi-service --test background_steering [filter]`。

| C-ID | 正常自查核对 | 对抗性变体 | 独立复测（命令 → 退出码 → 结果） | 证据 |
|---|---|---|---|---|
| R03-FIX-F07-C01 后台 Accepted 后下一轮准确且只一次 | turn1 纯输入、turn2 恰含 `[steering]`、`occurrences==1`、`pending==0`、终态 `completed.with_final` | 多追加（4 条两批）、逆字典序提交、4 轮循环：join 按提交序、已 drain 不重现（每 marker `occurrences==1`）、终轮无 `[steering]` | `c01_` 过滤 → **exit 0，2/0**（`logs/r1-c01-filter.log`）；串行证据跑 → turn2 = `BG-C01: please continue\n\n[steering]\nSTEER-C01-focus-on-config`、`occurrences:1`；c01_adv 四 marker 全 1 | `logs/r1-c01-filter.log`、`logs/r1-serial-evidence.log`、`evidence-green.json#c01/c01_adversarial`；红基线该 2 用例红（`logs/r1-red-baseline-exit.log`） |
| R03-FIX-F07-C02 跨会话/跨运行不串用 | 双后台会话并行，仅 alpha 收到；beta 全部轮次（含第二个新 Run）不含；`occurrencesOfSteer==1` | 同会话取消后快速新 Run（前台入口）：被取消 Run 零消费、`pending==1` 可观测、`run_count==1`（不自动触发）、idle `Miss` 不落库、新 Run 首轮恰含保留文本、beta `pending==0` 全程 | `c02` 过滤 → **exit 0，2/0**（`logs/r1-c02-filter.log`）；证据跑 `occurrencesOfSteer:1`；`c02_adversarial`: cancelledRun 与 newRun 分离、新 Run 首轮含保留文本 | `logs/r1-c02-filter.log`、`evidence-green.json#c02/c02_adversarial`；红基线：`c02_..._parallel_runs` 红、`c02_adversarial` 绿（我独立复现，见披露复核） |
| R03-FIX-F07-C03 取消/终态边界可解释 | 中途 Accepted 被本 Run 下一轮真实消费、完成后 `pending==0`、同会话下一无关任务干净起步（无残留串入） | 错过最后一 drain：本 Run 照常 `completed.with_final` 不假称、文本不在其任何输入、`pending==1` 保留、`run_count==1` 不触发、下一 Run 首轮恰收到 | `c03` 过滤 → **exit 0，2/0**（`logs/r1-c03-filter.log`）；证据跑 `c03_retention.turnInputs` 展示保留文本只出现在第二任务首轮 | `logs/r1-c03-filter.log`、`evidence-green.json#c03_leak/c03_retention`；红基线 2 用例红 |
| R03-FIX-F07-C04 容量与前后台一致 | 容量 2 前后台对照：两腿均 2 Accepted + 第 3 条 `Err(SteeringInboxFull)`、拒绝后 `pending==2` 不污染、下一轮恰含两条容量内文本、被拒文本永不出现 | 6 路并发 steer 竞争容量 2：恰 2 Accepted / 4 拒绝（无 Miss 无其他错）、`pending==2` 界不破、下一轮恰含被接受两条（无丢失无重复） | `c04` 过滤 → **exit 0，2/0**（`logs/r1-c04-filter.log`）；证据跑 `accepted:2 / refused:4` | `logs/r1-c04-filter.log`、`evidence-green.json#c04/c04_concurrent`；红基线 2 用例（后台腿/并发腿）红 |

## 红基线真实性（独立复现）

`git worktree add /tmp/r03-g06-redcheck-r1 8883923a5`（确认 background.rs:305 仍为 `None`）→ 拷入工作区新测试 → `cd /tmp/r03-g06-redcheck-r1/rust && ~/.cargo/bin/cargo test --locked -p lingxi-service --test background_steering` → **exit 101，1 passed / 7 failed**。红项清单与执行者登记完全一致：c01×2、c02（跨会话）、c03×2、c04×2；唯一绿项 = `c02_adversarial`。用后 `git worktree remove --force`（已确认清除）。日志：`logs/r1-red-baseline-exit.log`。

## 执行者披露复核（c02_adversarial 未修即绿）

我独立复现了"未修代码上 c02_adversarial 即绿"。**裁决：披露如实，钉的保留有长期价值，C02 错归属反证充分。**

1. 未修实现"从不 drain"，对"取消后不消费、保留可观测、不假称、不自动触发、下一 Run（前台入口）首轮收到"这一观测面天然满足 —— 该用例在旧码上不可能红，执行者如实登记且不据此声称反证。
2. 钉的长期价值：它钉的是**取消边界的冻结处置契约**（含跨入口归属：后台 Accept → 前台下一 Run 消费），能捕获未来"取消时悄悄丢弃/假消费/错投递"类回归；且其新 Run 首轮收文本断言在旧后台驱动上其实依赖前台 drain（旧码下第二 Run 是前台入口才绿）——修复后后台自身也能满足同契约。
3. C02 真正被打破的面向（授权会话的下一轮收不到 steering、串入无关 Run）由红项 `c02_steering_stays_in_the_authorized_session_across_parallel_runs` 与 `c03_midflight`（残留串入）充分承担。

## G01–G05 回归与 A03/A12

15 个保护套件逐套独立复跑全绿（`logs/r1-protected-suites.log`，各 exit 0）：cancel_link_inheritance 7/0、subagent_closeout 8/0、cancel_terminal_race 13/0、tool_receipt_unknown 6/0、admission_dedup_consistency 5/0、admission_dedup_adversarial 5/0、input_payload_fidelity 5/0、input_budget_refusal 2/0、session_serialization 5/0（A03）、background_disconnect_recovery 3/0（A12）、cancellation_tree 8/0、execute_concurrency 3/0、r03_t08_acceptance_matrix 17/0、subagent_permission_inheritance 4/0、subagent_lifecycle 3/0。A03/A12 断言未变：`git diff --stat HEAD -- rust/` 仅 `background.rs` 一个文件（测试文件零改动，新测试为 untracked 新增）。

## 门禁（真实命令 + 退出码）

| 命令 | 退出码 | 结果 |
|---|---|---|
| `~/.cargo/bin/cargo test --locked --workspace` | **0** | **72 test-result-ok / 704 passed / 0 failed**（与期望 72/704/0 完全一致；增量 = 本轮 1 新套件 8 用例）。`logs/r1-workspace-test-full.log` |
| `~/.cargo/bin/cargo fmt --all --check` | **0** | 零 diff（0 字节日志）。`logs/r1-fmt-check.log` |
| `~/.cargo/bin/cargo clippy --locked --workspace --all-targets -- -D warnings` | **0** | 零告警。`logs/r1-clippy-D-warnings.log` |
| `rust/Cargo.lock` vs HEAD | — | sha1 `3b659f41eb262eaf42efc2f91ec93f9989d42934` 两侧一致，零依赖变化 |

## finding

1. **[LOW｜证据卫生｜不影响裁决]** `G06-E01/normal-selfcheck/evidence.json` 尾部损坏（第 88–91 行残留 `}"    ]  }  }`，整体非合法 JSON；主体 8 键完整可读）。根因：测试内 `write_evidence` 是无锁 read-modify-write，并行测试线程（及 10 连锤指向同一文件）互相覆盖 —— 我自己以默认并行度复跑同命令，证据文件只剩 2/8 键，复现了该竞态；串行 `-- --test-threads=1` 则 8 键齐全。套件 pass/fail 日志与断言是权威证据，JSON 属 best-effort 附加。无需本轮动作；后续套件若继续用该模式，建议证据跑加 `--test-threads=1` 或按用例分文件。
2. **[观察项｜超出本轮 diff 范围｜无需本轮动作]** `runs.rs` 的 drain 位于每轮循环头、模型准入 permit 获取**之前**：若某轮 drain 后 `acquire_or_break` 配额耗尽（或等 permit 期间取消），该轮已 drain 的文本既未送达任何模型轮也未保留（从队列消失）。此为既有行为、前台后台完全一致、`runs.rs` 本轮零改动，不属 F07 回归；C03 规定的两个边界（泊车中取消、错过最后一 drain）行为符合冻结契约。登记给总控备查：若冻结契约被解读为"drain 后失败窗口也须保留"，可作为未来独立 F-ID。

## 误判反证

无。审查清单 F07 三条 source_facts（后台下一轮无 steering、跨会话目标会话下一轮无 steering、容量内不真实消费/残留串入）全部在我的红基线复现中确证（exit 101，7 红）；未发现需要判 NOT_A_DEFECT 的项。修复也没有制造总控 §6 列出的衍生问题（双重 finalize/重复计数/requestId 串用/旧 steering 投给新 Run 之外的错投/取消先赢后 completed）——steering 只进输入文本，不触碰 finalize、计数或 requestId 链。

## 需标 STALE

无。执行者三份报告与 `commands.json` 的数字（红 1/7 exit 101、绿 8/0 exit 0、workspace 72/704/0、锤 80/80、G01–G05 套件计数）经我独立复测全部吻合；唯一差异即 finding 1 的证据文件尾部损坏（主体仍可读，不足以 STALE 该证据）。

## 审查范围声明

本 PASS 仅对被核验候选（HEAD `8883923a5` + 上述未提交改动）有效；修后又改行为代码或测试输入，必须新建 Reviewer。总控账本（`R03_FIX_ISSUES.json` 等）的 F07 状态登记不在我的写权限内（本轮未改）。未覆盖：其他平台/正式打包/真实供应商（与 F07 无关）。
