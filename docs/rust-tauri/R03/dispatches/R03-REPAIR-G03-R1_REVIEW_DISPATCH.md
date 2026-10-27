# R03 对抗性修复派单｜G03-R1（独立 Reviewer，第 1 轮）

派单时间：2026-09-30。你是一次性独立对抗性 Reviewer：REVIEWER-REPAIR-R03-G03-R1，未参与 G03 候选的实现或修复。只验 G03（F04，4 个 C-ID：R03-FIX-F04-C01..C04）。

## 0. 候选与边界

- 候选：分支 `codex/rust-tauri-migration`，HEAD `ccb09fde6` + 未提交工作树：`rust/crates/lingxi-service/src/runs.rs`（tool_child.wait Err→Unknown{reason}；unobserved_tool_exit_reason 消费 TaskExit 四变体；delegation 拒绝回执 dispatched=false）、新测试 `tests/tool_receipt_unknown.rs`（6 集成测试）、`tests/subagent_permission_inheritance.rs`（+dispatched=0 断言）。总控账本更新不在审查范围。
- 执行者证据：`artifacts/rust-tauri/R03/repair-current/G03-E01/`；报告 `docs/rust-tauri/R03/repair-current/G03-E01_*.md`。
- 你的复测产物只写 `artifacts/rust-tauri/R03/repair-current/G03-R1/`，报告写 `docs/rust-tauri/R03/repair-current/G03-R1_REVIEW.md`。不得修改产品/测试/配置/门禁/账本/执行者证据；不得 commit/push。

## 1. 必读

修复清单 MD F04 节 + 总控规程 §6；验收清单 JSON F04 4 个 case；R03 任务书 T05（A09/A10）+ 02 目标契约 §5/§8；真实 diff 与调用链（runs.rs 派发/回执路径、kernel invocation.rs 分类、invocations.rs/recovery.rs 恢复面）；执行者三层报告。

## 2. 独立核查要求

1. 逐 C-ID 核源码与证据绑定；独立重跑关键反例（至少 C01 副作用后 panic→Unknown、C04 重复恢复不重做各一次真实运行；C02/C03 定向抽查），记录真实命令+退出码。
2. **分类边界审查**：三态事实清晰——已知未派发（dispatched=false，含 delegation 拒绝）、可信外部明确失败（保持 ConfirmedSettled{Failed}，不被扫成 Unknown）、派发后无回执异常（panic/中止/丢通道/超时→Unknown）。检查没有把所有错误都变 Unknown 或都变 Failed；`unobserved_tool_exit_reason` 的 TaskExit→reason 映射与 G01 后真实 TaskExit 语义一致。
3. **执行者披露专项（C02）**：执行者声明 Aborted/Failed 两个 TaskExit 变体无法进程内确定性编排，仅以单元枚举+单一入口结构覆盖。独立判断该覆盖是否充分：可尝试利用 G01 引入的协作窗口/超窗丢弃/分离退出观察者等基础设施在集成层编排一次 forced-abort 后无回执的路径，或给出为何单元枚举+单一入口已封闭全部漏点的结构论证（引用具体代码路径）。
4. 恢复面：启动扫描/重复扫描对 Unknown 的呈现与幂等（不重做、无新外部执行）；幂等控制例核验只用原 key。
5. G01/G02 回归：cancel_link_inheritance/subagent_closeout/cancel_terminal_race/cancellation_tree 复跑绿；runs.rs 改动未破坏 G02 裁决入口。
6. 红基线真实性：可用 `git worktree add /tmp/r03-g03-redcheck ccb09fde6` 隔离副本拷入新测试复跑（旧码应红：got Failed/dispatched=1），用后 remove。
7. 门禁：workspace `--locked`（期望 67/673/0）+ fmt + clippy -D warnings 真实退出码。
8. 允许以独立证据判 NOT_A_DEFECT；无依据不得弱化；无问题就 PASS。

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
