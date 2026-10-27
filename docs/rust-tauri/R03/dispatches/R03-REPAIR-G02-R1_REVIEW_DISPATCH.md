# R03 对抗性修复派单｜G02-R1（独立 Reviewer，第 1 轮）

派单时间：2026-09-30。你是一次性独立对抗性 Reviewer：REVIEWER-REPAIR-R03-G02-R1，未参与 G02 候选的实现或修复。只验 G02（F03，4 个 C-ID：R03-FIX-F03-C01..C04）。

## 0. 候选与边界

- 候选：分支 `codex/rust-tauri-migration`，HEAD `520bb75b9` + 未提交工作树：`rust/crates/lingxi-service/src/cancel.rs`（+341，Settling 相位/fire 原子化/claim_terminal）、`runs.rs`（+160，adjudicated_finalize 唯一原子裁决入口+9 处 gate_cancel 派发门）、`sessions.rs`（+48，CancelRunOutcome::TooLate）、新测试 `tests/cancel_terminal_race.rs`（13 测试，GatedStorage 真实存储边界泊车）。另有总控账本更新（不在审查范围，不得修改）。
- 执行者证据：`artifacts/rust-tauri/R03/repair-current/G02-E01/`；报告 `docs/rust-tauri/R03/repair-current/G02-E01_*.md`。
- 你的复测产物只写 `artifacts/rust-tauri/R03/repair-current/G02-R1/`，报告写 `docs/rust-tauri/R03/repair-current/G02-R1_REVIEW.md`。不得修改产品/测试/配置/门禁/账本/执行者证据；不得 commit/push。

## 1. 必读

修复清单 MD 的 F03 节 + 总控规程 §6；验收清单 JSON F03 的 4 个 case（given/when/then/adversarial_variation/evidence_required）；02 目标契约 §4；真实 diff 与调用链；执行者三层报告；G01 语义（cancel.rs 当前取消树/相位模型）。

## 2. 独立核查要求

1. 逐 C-ID 核源码与证据绑定；**独立重跑**关键反例（至少 C01 取消先赢、C03 并发不倒退各一次真实运行，含 barrier/固定交错；C02/C04 可复核执行者证据+定向抽查）。
2. **线性化规则审查**：确认裁决点（claim_terminal/相位 Mutex）真正覆盖**所有**终态路径（completed 全形状/failed/no-provider 早收口），非只在 Final 路径或最后 await 前补检查；不可撤销点定义（写 Settling）先于 finalize 首个 await；父树已取消但相位未反映的让位路径复核。
3. **不得存在的后果**：取消先赢后仍 completed/final message；完成已定后取消谎报 Accepted；Cleaning/Confirmed 回写 Requested；首因覆盖；重复结算；TooLate 分支反写旧终态。
4. G01 回归：cancel_link_inheritance/subagent_closeout/cancellation_tree 套件复跑必须绿；G02 未引入双重 finalize 或与 G01 协作取消窗口的冲突（G01 的 ChildCloseout 与 G02 的 adjudicated_finalize 交互）。
5. 红基线真实性：可用 `git worktree add /tmp/r03-g02-redcheck 520bb75b9` 隔离副本拷入新测试复跑关键反例（用后 remove），或按 diff 给出充分静态依据。
6. 门禁：workspace `--locked`（期望 66/666/0）+ fmt + clippy -D warnings 真实退出码。
7. 允许以独立证据判 NOT_A_DEFECT；无依据不得弱化；无问题就 PASS；后续阶段独立功能缺失不计 FAIL。

## 3. 环境

一律 `~/.cargo/bin/cargo`（1.98.1 锁定；Homebrew cargo 1.93.0 禁用）；全部 `--locked`；隔离 /tmp 数据根。

## 4. 输出

```text
VERDICT: PASS / FAIL / BLOCKED
候选摘要
逐 C-ID：正常自查核对 / 对抗性变体 / 独立复测命令与退出码 / 证据
finding（如有） / 误判反证（如有） / 需标 STALE（如有）
审查范围声明
```
