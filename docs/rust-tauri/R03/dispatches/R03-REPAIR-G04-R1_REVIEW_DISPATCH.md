# R03 对抗性修复派单｜G04-R1（独立 Reviewer，第 1 轮）

派单时间：2026-09-30。你是一次性独立对抗性 Reviewer：REVIEWER-REPAIR-R03-G04-R1，未参与 G04 候选的实现或修复。只验 G04（F05，5 个 C-ID：R03-FIX-F05-C01..C05）。

## 0. 候选与边界

- 候选：分支 `codex/rust-tauri-migration`，HEAD `198e0da1e` + 未提交工作树：`dedup.rs`（两阶段绑定 Pending/Committed/Unverified＋AdmissionBinding/AdmissionRetractor）、`sessions.rs`（受理链重写：跨重启持久锚点 RequestIdBoundToEarlierRun、Unverified 懒解决、前台补偿、AdmissionInFlight）、`background.rs`（派发拒绝同步撤销＋任务内提交）、`runs.rs`（admission 钩子 commit_durable 承诺点）、`subagents.rs`、`lingxi-adapters/src/storage/run_store.rs`（find_run_id_by_request，无新迁移）、`lingxi-service/src/lib.rs`（HTTP 错误映射）、新测试 `admission_dedup_consistency.rs`+`admission_dedup_adversarial.rs`（5+5）。总控账本更新不在审查范围。
- 执行者证据：`artifacts/rust-tauri/R03/repair-current/G04-E01/`；报告 `docs/rust-tauri/R03/repair-current/G04-E01_*.md`。
- 你的复测产物只写 `artifacts/rust-tauri/R03/repair-current/G04-R1/`，报告写 `docs/rust-tauri/R03/repair-current/G04-R1_REVIEW.md`。不得修改产品/测试/配置/门禁/账本/执行者证据；不得 commit/push。

## 1. 必读

修复清单 MD F05 节 + 总控规程 §6；验收清单 JSON F05 5 个 case；R03 任务书 T04/T07（A08、恢复契约）+ 02 目标契约 §3/§8；真实 diff 与调用链（sessions.rs admit/execute 前后台链、dedup.rs 状态机、background.rs 派发、run_store.rs 持久锚点查询）；执行者三层报告；G01–G03 当前语义。

## 2. 独立核查要求

1. 逐 C-ID 核源码与证据绑定；独立重跑关键反例（至少 C01 幽灵 Replay、C05 重启安全契约各一次真实运行；C02/C03/C04 定向抽查），记录真实命令+退出码。
2. **绑定状态机审查**：Pending/Committed/Unverified 三态转换是否可证明一致——已知未派发失败安全撤销；副作用已发生（含 journal 提交前后丢响应两态）不删绑定、不重复执行；Replay 只返回可核实的真实受理（持久锚点或真实库行）；不存在无条件删 key 路径。
3. **新语义收紧合理性**：窗口内 AdmissionInFlight（409 retryable）与跨重启 RequestIdBoundToEarlierRun 显式拒绝是否与 A08/A12/A14 既有断言一致、是否有既有消费者被破坏（check-contracts 626 零漂移复核）。
4. **执行者披露专项**：后台任务内部起始写失败无端口接缝，以相同补偿代码形状+Drop 回退单元钉+真实库懒解决端到端覆盖——独立判断充分性（结构论证或尝试编排）。
5. 并发与隔离：同 key 同/异内容、跨主体/会话命名空间隔离；并发交错下不暴露半提交假结果。
6. G01–G03 回归：cancel_link_inheritance/subagent_closeout/cancel_terminal_race/tool_receipt_unknown 复跑绿；runs.rs/sessions.rs 改动未破坏 G02 裁决与 G03 分类。
7. 红基线真实性：可用 `git worktree add /tmp/r03-g04-redcheck 198e0da1e` 隔离副本拷入新测试复跑（旧码应红：幽灵 replay 等），用后 remove。
8. 门禁：workspace `--locked`（期望 69/689/0）+ fmt + clippy -D warnings 真实退出码。
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
