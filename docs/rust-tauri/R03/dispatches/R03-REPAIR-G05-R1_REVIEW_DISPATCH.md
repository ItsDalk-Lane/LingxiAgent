# R03 对抗性修复派单｜G05-R1（独立 Reviewer，第 1 轮）

派单时间：2026-09-30。你是一次性独立对抗性 Reviewer：REVIEWER-REPAIR-R03-G05-R1，未参与 G05 候选的实现或修复。只验 G05（F06，3 个 C-ID：R03-FIX-F06-C01..C03）。

## 0. 候选与边界

- 候选：分支 `codex/rust-tauri-migration`，HEAD `d56e6883d` + 未提交工作树：`lingxi-service/src/limits.rs`（MAX_SUBMISSION_INPUT_BYTES=DEFAULT_BODY_LIMIT_BYTES=1 MiB）、`sessions.rs`（InputTooLarge{bytes,limit_bytes} 于 admit_submission 头部先于任何预留/持久写显式拒收；执行载荷全量传递，take(2000) 从 src/ 消失）、`lib.rs`（HTTP 413 input_too_large 映射）、新测试 `input_payload_fidelity.rs`（5）+`input_budget_refusal.rs`（2）。总控账本更新不在审查范围。
- 执行者证据：`artifacts/rust-tauri/R03/repair-current/G05-E01/`；报告 `docs/rust-tauri/R03/repair-current/G05-E01_*.md`。
- 你的复测产物只写 `artifacts/rust-tauri/R03/repair-current/G05-R1/`，报告写 `docs/rust-tauri/R03/repair-current/G05-R1_REVIEW.md`。不得修改产品/测试/配置/门禁/账本/执行者证据；不得 commit/push。

## 1. 必读

修复清单 MD F06 节 + 总控规程 §6；验收清单 JSON F06 3 个 case；R03 任务书 T01/T08（A15）；真实 diff 与调用链（sessions.rs admit 链当前形态——G04 后已重写、limits.rs、lib.rs HTTP 层、dedup.rs 摘要）；执行者三层报告；G04 摘要/绑定语义。

## 2. 独立核查要求

1. 逐 C-ID 核源码与证据绑定；独立重跑关键反例（至少 C01 边界+尾部完整、C03 超限明确拒绝各一次真实运行；C02 定向抽查），记录真实命令+退出码。
2. **红线审查**：修复不是"2000→另一个魔法数"——确认执行载荷与日志摘要分离（日志只记计数不记内容）、预算内完整传递、超限在受理与任何副作用前拒收（零 run 行/零 started/零模型调用/零工具派发）；1 MiB 预算与 HTTP/WS 传输层上限的"一条预算两道执法腿"取证是否属实（读传输层实际 body limit 代码）。
3. **摘要一致性**：dedup 摘要覆盖完整输入且与实际执行内容对应；同前缀异尾部不误合并；CRLF→LF 声明规范化之外无隐式丢失；组合字符横跨旧 2000 切点完整。
4. **不误拒**：日志摘要超界（2001/100_000 字符）的合法输入不被拒——日志摘要与正式预算明确区分。
5. G04 协同：拒收路径不残留绑定（同 id 合法重试真实受理）；G01–G04 套件复跑绿；`take(2000` 在 src/ 全仓消失（grep 复核）。
6. 红基线真实性：可用 `git worktree add /tmp/r03-g05-redcheck d56e6883d` 隔离副本拷入新测试复跑（旧码应红），用后 remove。
7. 门禁：workspace `--locked`（期望 71/696/0）+ fmt + clippy -D warnings 真实退出码。
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
