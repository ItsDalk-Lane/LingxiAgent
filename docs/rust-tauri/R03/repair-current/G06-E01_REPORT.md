# R03 修复轮 G06-E01 执行报告（F07：后台运行接受 steering 却没有传入消费通道）

- 执行代理：EXECUTOR-REPAIR-R03-G06-E01（一次性执行/修复代理；本报告为执行者口径，不含独立审查）。
- 日期：2026-09-30。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。
- 基线 `FIX_BASE_SHA=cd3fb19e651f763afc6c75cb3163064fb54ca3fe`；候选起点 `CANDIDATE=8883923a5b30a2a6c0face0c42a9ea143db9a605`（含已通过独立审查的 G01–G05 修复——本轮**未回退、未破坏**：cancel_link_inheritance 7/0、subagent_closeout 8/0、cancel_terminal_race 13/0、tool_receipt_unknown 6/0、admission_dedup_consistency 5/0、admission_dedup_adversarial 5/0、input_payload_fidelity 5/0、input_budget_refusal 2/0、session_serialization 5/0（A03）、background_disconnect_recovery 3/0（A12）、cancellation_tree 8/0、execute_concurrency 3/0、r03_t08_acceptance_matrix 17/0、subagent_permission_inheritance 4/0、subagent_lifecycle 3/0 逐套复跑全绿，见 `logs/g01-g05-protected-suites.log`）。本轮无 commit/push（未获授权）。总控账本（`R03_FIX_ISSUES.json`、`R03_FIX_COMMIT_RECEIPTS.json`）未改动（其未提交变更为派单前已存在）。
- 工具链：`~/.cargo/bin/cargo`（rustup 锁定 1.98.1），全部 `--locked`；`rust/Cargo.lock` sha1 `3b659f41eb262eaf42efc2f91ec93f9989d42934` 与 HEAD 相同（零依赖变化）。
- 证据根：`artifacts/rust-tauri/R03/repair-current/G06-E01/`（`normal-selfcheck/`、`adversarial-selfcheck/`、`logs/`、`commands.json`）。
- 结论：**READY_FOR_REVIEW**（workspace 72 suites / 704 passed / 0 failed ≥ 底线 71/696/0，增量 = 本轮 1 个新套件 8 个新用例；fmt 零 diff；clippy `-D warnings` 零告警；Cargo.lock 不变）。

## 1. 实现范围

F07 本体（后台驱动持有 lease 却给 `drive_run` 的 steering 参数传 `None`，已接受的后台追加输入永远到不了下一模型轮）及其同根因路径（未消费 steering 的归属边界）。不迁移 R07 客户端，不改任何受理/去重/取消/裁决契约。

## 2. 根因与同族路径清单（复核结论：审查属实，红基线实测复现，无反证）

调用链复核与审查清单一致：

1. `sessions.rs::steer_for` 依据真实 session gate（`SessionSupervisor::steering_submit`：busy 会话 `Accepted`（有界 inbox），idle `Miss`）接受追加输入，写入该会话 slot 的 `Arc<SteeringInbox>`——与 `SessionLease` 持有的是**同一个** Arc。
2. 前台 `execute_submission_for`（sessions.rs:685）传 `Some(lease.steering_inbox())`；`runs.rs::drive_run` 只在 `Some(inbox)` 时于**每轮模型调用前** drain（runs.rs:701–706）。
3. `background.rs::spawn_background_drive` 虽把 lease 移入分离任务（`let _lease = lease;`），却给同一参数传 `None`——后台已 Accepted 的追加输入无人消费：既到不了本 Run 的任何模型轮，也**残留**在 slot 里，只能串入该会话的下一个（不相关）任务的首轮（F07 影响注记的"残留串入别的 Run"）。

同族路径清单（`drive_run` 全部调用点逐一定性）：

| 调用点 | steering 实参 | 定性 | 处置 |
|---|---|---|---|
| `sessions.rs:685` 前台执行 | `Some(lease.steering_inbox())` | 正确（同源参照实现） | 不变 |
| `background.rs`（原 :295）后台驱动 | `None`（bug） | **F07 本体** | 改为 `Some(_lease.steering_inbox())` |
| `subagents.rs:769` 子代理 child run | `None` | **正确，禁改**：子代理走隔离 lane（`{session}::subagent::{thread}`），其结果经 `deliver_retained` 回注父会话 inbox；若 child 也 drain 父会话 inbox，会**偷走**用户 steering，恰是 C02 反例 | 不变 |

受理面无第二套 steering 通道：`steer_for` 是唯一入口（前台/后台会话共用），容量与授权在 gate 层统一（`push_bounded`，容量 `steering_inbox_capacity`，`SessionConcurrencyLimits::validate` 拒绝 0 容量）。

**红基线实测**（隔离 git worktree `/tmp/lingxi-r03-g06-redbase`，detached HEAD=8883923a5 未含修复，最终版测试文件）：`background_steering` **1 passed / 7 failed**（`adversarial-selfcheck/red-baseline-final-file.log`，exit 101）。红项：C01 两条（下一轮输入无 steering）、C02 跨会话（授权会话下一轮无 steering）、C03 两条（中途 steering 未被本 Run 消费；迟到 steering 到不了下一 Run 首轮）、C04 两条（容量内 steering 未真实消费；并发竞争容量下被接受文本未到达）。`c02_adversarial`（取消边界）在未修代码上即为绿——其钉的是"取消后不消费、保留可观测、不自动触发"契约，未修前后观测一致（未修时因从不 drain 而"天然"满足）；C02 的错归属反证由红项 `c02` 承担。

## 3. 修复设计（最小完整）

**一行接线 + 注释**（`background.rs`）：分离任务内 `drive_run(..., Some(_lease.steering_inbox()), None, ...)`——与前台**同一**被授权通道（lease 持有的正是 `steer_for` 写入的那个 slot inbox Arc），不另造第二套。由此自动继承全部不变量：

- **一次消费**：`SteeringInbox::drain_joined` 原子 drain 整个队列，每条文本恰进一个模型轮（C01 对抗：多轮多次追加、提交顺序即 join 顺序、已 drain 不重现）。
- **身份隔离**：inbox 按 session slot 键控；后台/前台同一会话同一 Arc，跨会话物理隔离；会话 busy gate 保证任一时刻一个会话只有一个持有 lease 的 Run 在 drain（C02）。
- **容量**：受理侧 `push_bounded` 有界（满即 `SteeringInboxFull` 响亮拒绝、不入队）；消费侧后台接入后容量在轮间真实释放——前台/后台同一 gate 同一容量（C04）。
- **取消/终态边界（C03，契约 = 既有冻结语义，非本轮新造）**：
  - drain 位于每轮循环头 `gate_cancel!()` **之后**——取消先赢时驱动直接走四相取消结算，**不 drain**：已接受未消费的 steering 停留 slot（`steering_pending` 可观测），不假称已用于执行；
  - 错过最后一轮 drain（终轮已派发后 Accepted）的 steering 同样保留给该会话下一 Run 首轮消费（`session_supervisor.rs` 模块注释与既有绿测 `leftover_steering_survives_into_the_next_run` 钉死的冻结语义）；
  - steering 本身永不触发新任务（仅 submission 创建 Run；idle 会话 steer = `Miss`）。
- **不残留串入无关任务**：中途 Accepted 的 steering 被本 Run 下一轮消费后 `steering_pending` 归零，后续无关任务干净起步（C03 leak 腿；未修代码正是此残留串入）。

未改：`sessions.rs`、`runs.rs`、`lib.rs`（错误映射）、子代理路径、任何 G01–G05 语义。禁止项核查：未把 Accepted 换假成功空响应（drain 真实进模型输入）；未禁用/丢弃后台 steering（恰是接通）；非只补前台测试（红基线即后台腿）。

## 4. 改动文件

| 文件 | 改动 |
|---|---|
| `rust/crates/lingxi-service/src/background.rs` | `spawn_background_drive` 内 `drive_run` steering 实参 `None` → `Some(_lease.steering_inbox())`；函数 doc 与调用点注释（G06/F07 出处）；无其他语义变化 |
| `rust/crates/lingxi-service/tests/background_steering.rs` | 新增集成套件（8 用例，C01–C04 两层自查载体）：真实受理链/会话 gate/lease/驱动/取消/单次 finalize，Provider 替身仅记录每轮实际输入并按 (session,pop) 门控泊车 |

## 5. 逐 C-ID 状态（两层自查详情见同目录另两份自查文档）

| C-ID | 普通自查 | 对抗性自查 | 载体用例 | 红基线 |
|---|---|---|---|---|
| R03-FIX-F07-C01 后台 Accepted 后下一轮收到、只一次 | PASS | PASS（多追加/不同顺序/循环多轮/10 连跑） | `c01_background_accepted_steering_reaches_next_turn_exactly_once`、`c01_adversarial_multi_steer_orders_and_rounds_no_redrain` | 2 红 |
| R03-FIX-F07-C02 跨会话/跨运行不串用 | PASS | PASS（并行双后台会话；取消后快速新 Run） | `c02_steering_stays_in_the_authorized_session_across_parallel_runs`（红）、`c02_adversarial_cancel_then_quick_new_run_no_misattribution`（契约钉） | 1 红 1 绿(钉) |
| R03-FIX-F07-C03 取消/终态边界明确契约 | PASS | PASS（中途消费不残留；错过时点保留不谎称、不触发） | `c03_midflight_steering_is_consumed_not_leaked_into_next_run`、`c03_too_late_steering_retained_not_claimed_no_auto_trigger` | 2 红 |
| R03-FIX-F07-C04 有界容量前后台一致 | PASS | PASS（容量 2 前后台对照；超限响亮拒绝不污染；6 路并发竞争容量） | `c04_bounded_inbox_same_contract_foreground_and_background`、`c04_adversarial_concurrent_steers_respect_the_bound` | 2 红（后台腿/并发腿） |

## 6. 回归与门禁

- workspace：`cargo test --locked --workspace` exit 0，**72 test-result-ok / 704 passed / 0 failed**（≥ 底线 71/696/0；增量 = 本轮 8 用例）。`logs/workspace-test-full.log`。
- 套件稳定性：`background_steering` 连续 10 轮 8/8（`adversarial-selfcheck/hammer-10x.log`，80/80）。
- G01–G05 保护套件 + steering 相邻套件逐套复跑全绿（15 suites，`logs/g01-g05-protected-suites.log`）；A03（session_serialization）/A12（background_disconnect_recovery）/r03_t08_acceptance_matrix 断言保持。
- `cargo fmt --all --check` exit 0（对本轮新文件执行过一次 `cargo fmt --all` 后零 diff；`logs/fmt-check.log`）。
- `cargo clippy --locked --workspace --all-targets -- -D warnings` exit 0 零告警（`logs/clippy-D-warnings.log`）。
- `rust/Cargo.lock` 与 HEAD 相同（sha1 `3b659f41…`，零 diff）。

## 7. 对审查结论的反证

无。审查三条 source_facts 全部经红基线复现（后台下一轮无 steering、跨会话目标会话下一轮无 steering、容量内不真实消费；残留串入由 c03_midflight 红 failing 文本直接展示）。未发现需要移交总控的误判。

## 8. 执行者口径声明

本轮所有命令均在本机以 `~/.cargo/bin/cargo`（1.98.1）`--locked` 执行；证据文件为原始日志/JSON（`artifacts/…/G06-E01/`）。不构成独立审查；READY_FOR_REVIEW 仅为执行者自查结论。
