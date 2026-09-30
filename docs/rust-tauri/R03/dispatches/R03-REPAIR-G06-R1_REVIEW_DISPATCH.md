# R03 对抗性修复派单｜G06-R1（独立 Reviewer，第 1 轮）

派单时间：2026-09-30。你是一次性独立对抗性 Reviewer：REVIEWER-REPAIR-R03-G06-R1，未参与 G06 候选的实现或修复。只验 G06（F07，4 个 C-ID：R03-FIX-F07-C01..C04）。

## 0. 候选与边界

- 候选：分支 `codex/rust-tauri-migration`，HEAD `8883923a5` + 未提交工作树：`rust/crates/lingxi-service/src/background.rs`（spawn_background_drive 任务内 drive_run steering 实参 None→Some(_lease.steering_inbox())，与前台同源通道；+注释）、新测试 `tests/background_steering.rs`（8 用例）。总控账本更新不在审查范围。
- 执行者证据：`artifacts/rust-tauri/R03/repair-current/G06-E01/`；报告 `docs/rust-tauri/R03/repair-current/G06-E01_*.md`。
- 你的复测产物只写 `artifacts/rust-tauri/R03/repair-current/G06-R1/`，报告写 `docs/rust-tauri/R03/repair-current/G06-R1_REVIEW.md`。不得修改产品/测试/配置/门禁/账本/执行者证据；不得 commit/push。

## 1. 必读

修复清单 MD F07 节 + 总控规程 §6；验收清单 JSON F07 4 个 case；R03 任务书 T02/T06（A03/A12）；真实 diff 与调用链（background.rs/sessions.rs steer_for 与 lease/steering_inbox/runs.rs drive_run drain 逻辑）；执行者三层报告；G01–G05 当前语义。

## 2. 独立核查要求

1. 逐 C-ID 核源码与证据绑定；独立重跑关键反例（至少 C01 后台 Accepted 后下一轮准确且只一次、C02 跨会话/取消后新 Run 不串用各一次真实运行；C03/C04 定向抽查），记录真实命令+退出码。
2. **同源与隔离审查**：后台与前台确实同一授权通道（读 steer_for/lease/steering_inbox 源码确认）；子代理 child run 传 None 的"刻意保留"论证是否成立（drain 父会话 inbox 是否真会偷走用户 steering）；受理面无第二套 steering 通道。
3. **一次消费/不串用**：多轮循环不重复 drain；取消/终态后未消费文本的处置契约明确（保留可观测、不假称已用于执行、不悄悄触发新任务、不带入无关下一 Run）。
4. **容量一致**：小容量 inbox 前后台同契约；超限显式拒绝（SteeringInboxFull）不污染队列。
5. **执行者披露复核**：c02_adversarial 在未修代码上即绿（契约钉在"从不 drain"旧实现下观测相同）——独立判断该钉的保留是否仍有长期价值、C02 错归属反证是否由红项 c02 充分承担。
6. G01–G05 回归：六组保护套件复跑绿；A03/A12 既有断言未变。
7. 红基线真实性：可用 `git worktree add /tmp/r03-g06-redcheck 8883923a5` 隔离副本拷入新测试复跑（旧码应红 7 项），用后 remove。
8. 门禁：workspace `--locked`（期望 72/704/0）+ fmt + clippy -D warnings 真实退出码。
9. 允许以独立证据判 NOT_A_DEFECT；无依据不得弱化；无问题就 PASS。

## 3. 环境

一律 `~/.cargo/bin/cargo`（1.98.1；Homebrew 1.93.0 禁用）；`--locked`；隔离 /tmp 数据根。

## 4. 输出

```text
VERDICT: PASS / FAIL / BLOCKED
候选摘要
逐 C-ID：正常自查核对 / 对抗性变体 / 独立复测命令与退出码 / 证据
finding（如有） / 误判反证（如有） / 需标 STALE（如有）
审查范围声明
```
