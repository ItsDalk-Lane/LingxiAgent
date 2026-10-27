# G06-E01 对抗性自查（R03-FIX-F07-C01..C04）

执行者：EXECUTOR-REPAIR-R03-G06-E01。对象 = 修复后代码（工作区 8883923a5 + 未提交 G06 修复）。全部命令 `~/.cargo/bin/cargo`（1.98.1）`--locked`，隔离临时数据根，无网络外发。

统一说明：攻击载体是真实服务组合上的确定性门控替身（Provider 只记录每轮实际输入）；每条攻击先在**未修红基线**（隔离 git worktree `/tmp/lingxi-r03-g06-redbase`，detached HEAD=8883923a5，未含修复）上确认能捕获缺陷，再在修复后代码上复跑确认被推翻。

- 红基线命令：`cd /tmp/lingxi-r03-g06-redbase/rust && ~/.cargo/bin/cargo test --locked -p lingxi-service --test background_steering` → **exit 101，1 passed / 7 failed**（`red-baseline-final-file.log`）。
- 修复后命令：`cd rust && ~/.cargo/bin/cargo test --locked -p lingxi-service --test background_steering` → **exit 0，8 passed / 0 failed**（`../normal-selfcheck/normal-run.log`）。
- 稳定性锤：修复后同一命令连跑 10 轮 → **10 × 8/8（80/80）**，无任何轮次出现重复 drain、丢失或错归属（`hammer-10x.log`）。
- 机器证据：`../normal-selfcheck/evidence.json`（键 `c01`、`c01_adversarial`、`c02`、`c02_adversarial`、`c03_leak`、`c03_retention`、`c04`、`c04_concurrent`）。

---

## R03-FIX-F07-C01｜对抗：多次追加、不同顺序、循环多轮，不得重复 drain 同一内容

- 攻击窗口：后台 Run 多轮循环（4 轮门控泊车）；两批共 4 条 steering，第二批以"逆字典序"提交（zulu→yankee），试图打乱 join 顺序假设；每轮 drain 后再追加，试图诱发同一内容被再次 drain。
- 观测：轮 2 输入恰为 `[steering]\nSTEER-ADV-first\nSTEER-ADV-second`（提交序）；轮 3 恰为 `[steering]\nSTEER-ADV-zulu\nSTEER-ADV-yankee`，且**不含**第一批文本；轮 4（终轮）无 `[steering]`；四条文本各自 `occurrences == 1`（全记录轮次全局恰一次）；终态后 `steering_pending == 0`。
- 是否推翻修复：**否**（未修代码上同用例红：轮 2/3 输入无任何 steering 文本）。
- 命令/退出码：如上（红 exit 101 → 绿 exit 0；10 连跑稳定）。
- 证据：`evidence.json#c01_adversarial`、`red-baseline-final-file.log`、`hammer-10x.log`。

## R03-FIX-F07-C02｜对抗：同会话取消后快速开始新 Run，旧回调不能投递错归属（含并行双会话）

- 攻击窗口 A（并行双后台会话）：alpha/beta 同时驱动、同时泊车，仅对 alpha steer，随后释放两者并在 beta 上开第二个不相关后台任务——探测 steering 越会话串用。
- 攻击窗口 B（取消后快速新 Run）：alpha 后台 Run 泊车中 steer → Accepted → 立即 `cancel_run_for` → 四相取消结算 → 同一会话快速提交前台新 Run（前台入口，测跨入口归属）——探测旧追加被死 Run 消费、或投给错误归属、或凭空触发任务。
- 观测：A——alpha 轮 2 含文本；beta 全部轮次（含新任务的轮 3/4）不含；alpha `pending==0`。B——被取消 Run 的全部轮次不含该文本（取消后无 drain，不假称已执行）；`steering_pending==1`（保留可观测）；`run_count==1`（steering 未触发新任务）；idle 追加 steer = `Miss`（未落库）；新 Run（前台）首轮恰含保留文本且不含 Miss 的文本——**接收方是该会话的下一 Run（冻结契约），从未进入其他会话**；beta 会话 `pending==0` 全程。
- 是否推翻修复：**否**（窗口 A 在未修代码上红：授权会话下一轮收不到；窗口 B 未修前后观测一致——未修时"从不消费"天然满足该契约钉，如实登记，不据此声称红）。
- 命令/退出码：如上。
- 证据：`evidence.json#c02`、`evidence.json#c02_adversarial`、`red-baseline-final-file.log`。

## R03-FIX-F07-C03｜对抗：错过消费时点（取消边界与终轮边界）

- 攻击窗口 A（中途 Accepted 的残留串入）：turn1 泊车时 Accepted → 释放推进至完成——探测该文本未被本 Run 消费而残留、并串入同会话下一个不相关任务的首轮（F07 影响注记场景）。
- 攻击窗口 B（错过最后一 drain）：终轮已派发（输入已定格）后 Accepted → Run 正常完成——探测系统谎称已用于执行、或悄悄丢掉、或用 steering 触发新任务。
- 观测：A——本 Run 轮 2 输入含文本（真实消费）；完成后 `pending==0`；下一不相关后台任务的轮次（pop3+）**不含**该文本（无残留串入）。B——本 Run 终态 `completed.with_final`（自身照常）；该文本不在本 Run 任何输入（未谎称）；`pending==1`（保留）；`run_count==1`（未触发）；下一 Run 首轮恰含保留文本。
- 是否推翻修复：**否**（两窗口在未修代码上均红：A 的文本滞留 `pending` 并串入下一任务首轮；B 的保留文本到不了下一后台 Run 首轮）。
- 命令/退出码：如上。
- 证据：`evidence.json#c03_leak`、`evidence.json#c03_retention`、`red-baseline-final-file.log`。

## R03-FIX-F07-C04｜对抗：小容量（2）前后台对照 + 并发竞争容量

- 攻击窗口 A（前后台对照）：容量 2 下，后台腿与前台腿各自顺序提交 3 条——探测入口不同导致的容量/拒绝差异。
- 攻击窗口 B（并发竞争）：后台 Run 泊车中，**6 路并发任务**同时 steer（各自独立 `steer_for` 调用）——探测并发下超收、丢失、重复或队列越过容量。
- 观测：A——两腿第 3 条均 `Err(SteeringInboxFull)`；拒绝后 `pending==2`（队列恰为两条已接受，未被污染）；下一轮输入含两条容量内文本、不含被拒文本；drain 后 `pending==0`；两腿逐项一致。B——恰 2 条 `Accepted`、4 条 `SteeringInboxFull`（无 Miss、无其他错误）；`pending==2`（界不破）；下一轮输入恰含被接受的两条（无丢失、无重复，`accepted_in_turn2 == 2`）；完成后 `pending==0`。
- 是否推翻修复：**否**（A 的后台腿在未修代码上红：容量内文本未被消费；B 在未修代码上红：被接受的 2 条到不了下一轮）。
- 命令/退出码：如上。
- 证据：`evidence.json#c04`、`evidence.json#c04_concurrent`、`red-baseline-final-file.log`、`hammer-10x.log`。

---

## 额外源码级对抗复核（非运行时，如实标注）

- 子代理 child run（`subagents.rs:769`）steering 实参仍为 `None`——**刻意不改**：child 走隔离 lane，结果经 `deliver_retained` 回注父会话 inbox；若 child 也 drain 父会话 inbox 将偷走用户 steering（恰成 C02 反例）。本轮未动该调用点（`c02`/`c02_adversarial` + `subagent_permission_inheritance` 4/0、`subagent_closeout` 8/0 保持绿佐证）。
- 前后台同源复核：`sessions.rs:685`（前台）与修复后的 `background.rs` 使用**同一** `lease.steering_inbox()`（同一 slot Arc），不存在第二套通道或第二套容量判定（`push_bounded` + `steering_inbox_capacity` 唯一）。
- 禁止项核查：未把 Accepted 换成假成功空响应（steer 响应与消费均在真实链路断言）；未禁用/丢弃后台 steering（红→绿即消费接通）；非只补前台测试（红基线全部打在后台腿）。
