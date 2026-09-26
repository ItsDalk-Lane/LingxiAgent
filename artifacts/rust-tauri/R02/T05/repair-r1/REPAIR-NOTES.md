# R02-T05 REPAIR-R1 — 证据索引与修复说明（REPAIR-R02-T05-R1）

- 日期：2026-09-26；平台：macOS 27.0 arm64（Darwin 27.0.0）；工具链 rustup 锁定 1.98.1
- tested SHA：`e80c4b5d04b7697ae0277d74cbc661f777e65a39`（HEAD）+ 本任务未提交改动（含本轮修复）
- 输入：`docs/rust-tauri/R02/R02-T05_REVIEW_R1.md`（SHA-256 b85902a3…dbb1c8，VERDICT: FAIL，F01 BLOCKING + F02/F03 MINOR + F04 措辞）
- 修复范围：仅评审 findings 及其同根因路径；未触碰非本 Task 范围内容

## 修复内容（文件级）

| 文件 | 修复 |
|---|---|
| `rust/crates/lingxi-service/src/events.rs` | F01：`release_hold` 在 flush 前把去重水位 `last_enqueued_seq` 重锚到 cut（hold 期推进的水位不再压制 flush）；F02：`register` 基线对齐时排空恰好落在新 `next_seq` 上的 parked 链；F03：subscribe 游标校验重排——floor 截断检查（含 purge-all 无 floor 形态）先于 future 界，purge-all 陈旧游标 → `RequiresSnapshot{floor:None,reason:events_truncated}`；新增 4 个单元测试 |
| `rust/crates/lingxi-service/src/lib.rs` | F03 配套：HTTP 409 `cursor_expired` 的 `details.floorSeq` 仅在 floor 存在时输出（与 WS 控制帧一致；既有 floor=Some 用例输出不变） |
| `rust/crates/lingxi-service/tests/event_subscription.rs` | 新增 `a10_purge_all_stale_cursor_gets_rebuild_directive_not_future_cursor`、`a09_join_window_race_under_concurrent_writer_loses_nothing`（multi_thread，结构化调度点） |
| `scripts/rust-tauri/r02_t05_events_probe.py` | 新增 6 个 `f03-*` 二进制用例（beta 流 purge-all → HTTP/WS directive + 空重建 cut）；`execute()` 增加 session 参数 |
| `scripts/rust-tauri/r02_t05_events_matrix.sh` | 头注释与汇总行补充 F03 用例说明 |

## F01 根因与修复（摘要）

单Writer seq 下 subscribe 的 join = register(hold) → durable 页读 → release_hold(cut)。
hold 期间每个缓冲事件把 `last_enqueued_seq` 推进到自身（hold 去重角色），flush 时
`enqueue_for` 首查 `seq <= last_enqueued_seq` 即命中 → seq > cut 的 held 事件被静默丢弃，
且水位已推进使后续更正也被抑制。修复：`release_hold` 在 flush 前把水位重锚为
`cut`（契约保证 cut ≥ after_seq；重锚后水位语义恢复为「≤ cut 已由快照覆盖」，
flush 按序重新推进；flush 与 publish 同在 hub 锁内，无中间观测点；held 事件只投递一次，
release 后的 live 事件 seq ≥ next_seq > max(held)，无重复可能）。flush 仍走 `enqueue_for`
单一投递规则（含 mailbox 有界与 detach 语义），未加旁路分支。

## 同根因路径排查（last_enqueued_seq / hold / pending 全部写点读点）

- `enqueue_for` :527 overlap 分支（推进=快照覆盖，正确）、:543 hold 分支（推进无投递 → 由重锚修复）、
  :549 live 分支（推进=已投递，正确）；唯一 flush 路径 = `release_hold`（唯一修复点）。
- `SubscriberState.last_enqueued_seq` 读点：:530 去重、`subscriber_stats` 诊断——重锚后语义一致。
- F02 同模式（推进水位而无 drain 触发）：`register` 基线对齐处，已修；`publish` 的重排 drain
  走 `deliver_next`（真实投递）与 broken 流显式 detach，无同类问题。

## 新增/强化测试（全部在修复前代码上失败、修复后通过）

| 测试 | 对应 | 修复前 | 修复后 |
|---|---|---|---|
| `events.rs::held_event_above_the_cut_flushes_after_the_hold_advanced_the_watermark` | REVIEW 探针 A 反向断言（收 [3,4] + 水位=4） | FAILED | PASS |
| `events.rs::held_delta_above_the_cut_is_delivered_not_counted_lost` | 探针 A2（delta 投递、dropped_deltas=0） | FAILED | PASS |
| `events.rs::restart_baseline_drains_the_parked_event_at_the_new_next_seq` | 探针 B 反向断言（收 [4,5]） | FAILED | PASS |
| `events.rs::restart_baseline_drains_a_parked_chain_above_the_head` | F02 链式变体 | FAILED | PASS |
| `event_subscription.rs::a09_join_window_race_under_concurrent_writer_loses_nothing` | multi_thread 生产路径 race（结构化调度点 + 第二连接） | FAILED ×2（SILENT join hole at the seam, cut=480） | PASS ×5 |
| `event_subscription.rs::a10_purge_all_stale_cursor_gets_rebuild_directive_not_future_cursor` | F03 | （修复前该行为为 FutureCursor） | PASS |
| 二进制探针 `f03-*` 6 例 | F03 真实二进制面 | — | PASS ×3 轮 |
| 验收者 vendored 探针 A/A2/B（/tmp/r02t05-review-probe，vendored 同步为修复后逻辑） | F01/F02 反向断言 | 复现缺陷（3 failed） | 3 passed |

race 测试设计：multi_thread(4)；watcher 以 `yield_now` 自旋等待 `hub.subscriber_stats(k)`
出现（= `register(hold)` 恰好发生的因果调度点，零 sleep），触发第二独立 SQLite 连接
（`RunDatabase::open` 同一 runs.db，WAL 多连接）上的真实 `record_run_started` 提交并经生产
入口 `publish_committed` 发布。状态库单 FIFO 队列会结构性把「快照后提交」的回复排在读回复
之后（release 结构性先手，这也是 REVIEW 未封顶施压 0/200 的机理），第二连接使提交真正落入
读执行窗口。断言不变式：无 detach 的订阅必须无缝 join（live 首帧 = cut+1、连续到 final head、
合并视图逐事件 == 权威库）；有 detach 者至少 detach 前无缝。修复前 2/2 轮命中
`sub 0: SILENT join hole at the seam (cut=480)`；修复后 5/5 轮全绿。

## 验证命令与退出码（全部剥代理、--locked、专属 CARGO_TARGET_DIR=/tmp/rust-target-r02-t05-repair）

| 门禁 | 退出码 | 证据 |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | `gates/fmt-check.log` |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | `gates/clippy.log` |
| `cargo test --workspace --locked` | 0（**191 passed / 0 failed** = 185 基线 + 6 新增） | `gates/cargo-test-workspace.log` |
| `bash scripts/rust-tauri/r02_t05_events_matrix.sh` ×3 轮（真实二进制，24 探针用例 + 三视图交叉核对/轮） | 0 ×3 | `matrix-r{1,2,3}.log`、`matrix-r{1,2,3}/` |
| `cargo test -p lingxi-service --test event_subscription --locked` ×5 轮 | 0 ×5（8 passed/轮，无 flake） | 终端记录 |
| 修复前失败证据（三修复回退重建，仅用于取证） | race FAILED ×2 + 4 单测 FAILED | `gates/prefix-failure-evidence.log` |
| T01–T04 回归 7 脚本 | 0 ×7 | `gates/reg-r02_t0{1,2,3,4}_*.log` |
| `r01-t02-check-generated.sh`（契约零漂移） | 0 | `gates/contracts.log` |
| `git diff e80c4b5d0 -- rust/Cargo.lock` | 空（0 行） | — |
| `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py` | 0（RESULT: OK） | 终端 |
| `npm test -- --run tests/post-verification-audit-seal.test.ts` | 1（**预存在红**：坐标 ab4f2281 滞后于已授权 R02 提交，与本轮改动零交集，未触碰白名单） | `gates/audit-seal-baseline.log` |
| 验收者探针（vendored=修复后逻辑）hold_join_probes / adversarial_extras | 0 / 0（purge-all 分支实测 `loud RequiresSnapshot directive`） | `gates/reviewer-probes-postfix.log` |

## R02-A09 / R02-A10 当前状态

- **R02-A09（快照与订阅无空隙；关键事件不漏不重）**：修复后成立。合并语义（快照+续读不漏不重、
  重叠服务端去重）由既有 a09 矩阵/并行订阅者测试 + 新增 hold-flush 单测与 multi_thread race
  测试共同钉住；`hold_buffers_events_and_releases_at_the_cut` 等既有用例行为不变（重叠去重仍在）。
- **R02-A10（缓存过期可恢复）**：维持 PASS，词汇补齐——purge-all 陈旧游标现在双面（WS 控制帧 /
  HTTP 409）返回 `snapshot_required/events_truncated` 且 floor 缺省；部分截断、伪造未来游标
  （非空流）、畸形游标、跨流、跨主体、重复订阅等负例全部复跑通过。
