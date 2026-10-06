# WP-T07 R3 任务级独立验收（第 3 轮）— R05-T07-独立验收-r3

- 审查者：全新上下文独立验收智能体（未参与 R1/R2/R3 的任何实现或修复）。
- 日期：2026-10-06（会话时钟）；工作树 = codex/rust-tauri-migration @ d80737b6 + 未提交 RR1 候选。
- 工具链：全部经 `/Users/study_superior/.cargo/bin/cargo`（rustup 代理，cargo/rustc 1.98.1）。
- 复验对象：F21 / F22 / F23（含 F38、F39 的 R3 关闭候选）。
- 结论：**PASS（F21、F22、F23 全部维持/达到 INDEPENDENT_PASS；F39 经本轮独立复验由 IMPLEMENTED → INDEPENDENT_PASS）**。

## 一、亲测结果（主树；每项退出码见 exits.txt，日志同目录）

| 套件 | 结果 | exit |
|---|---|---|
| lingxi-service `r05_t07_rr1_usage_ledger` | 15/15（含 rr1_f39 双腿、rr1_f38 三取消腿、F21/F22 全腿） | 0 |
| 同上，`rr1_f39` 过滤重复 5 次 | 5×2/2 确定性全绿 | 0 |
| lingxi-service `late_result_fence` | 5/5（r03_a07 反例零改动） | 0 |
| lingxi-service `r05_t07_persistence` | 5/5（含 v6→v7 attempts nullable 迁移腿） | 0 |
| lingxi-service `r05_t07_usage_trace` | 9/9 | 0 |
| lingxi-service `r05_t06_worker_model` | 10/10 | 0 |
| lingxi-service `cancellation_tree` | 8/8（r03_a05 反例零改动） | 0 |
| lingxi-service `r05_t04_streaming` | 18/18（c16 反例零改动） | 0 |
| lingxi-service `r04_t07_mcp_and_workers` | 19/19 | 0 |
| lingxi-service `r05_t03_protocol_adapters` | 12/12 | 0 |
| lingxi-adapters `r05_t07_rr1_usage_strict` | 7/7（F22/F23 面） | 0 |
| lingxi-adapters `r05_t07_usage_families` | 14/14 | 0 |
| lingxi-adapters 全量 | 25 个测试二进制全 ok，0 failed | 0 |
| lingxi-kernel 全量 | 87/87 | 0 |
| `cargo fmt --all -- --check` | 0 差异 | 0 |
| `cargo clippy -p lingxi-service --all-targets --locked -- -D warnings` | 0 告警 | 0 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 告警 | 0 |
| lingxi-service 全量 `--no-fail-fast`（额外回归，svc-full-r3.log） | 67 个测试二进制 ok；唯一失败 `r00_management_leaves::r00_management_positive_and_negative_branches_on_real_service`，panic 文本自证为 macOS ALF/非环回自址阻断未签名测试二进制的环境失败（与 R2 及 T03—T06 登记的同一环境项，T00 管理面、与 T07 语义无关） | 101（仅因该环境项） |

## 二、F39 独立变异验证（本轮审查者自己的变异，非复述执行者）

隔离副本：`tar` 快照当前工作树（排除 target/.git）至 `/tmp/r05t07-acc-r3`，
独立 `CARGO_TARGET_DIR=/tmp/r05t07-acc-r3-target`。副本 runs.rs SHA-256 与主树一致
（b7b8671a8c4dc507ab77364e700985db69c2b619cc2f80cc54d86d0bf7eb17f3）。

1. **副本基线**：15/15，exit 0（copy-restore-full.log 之前先跑过一次基线，同为 15/15）。
2. **MUT-R3B（整体中性化 fence 臂落行）**：把 runs.rs fence-Stale+cancelled 臂
   （`usage: provider_turn.usage.clone()` 唯一锚定的那个 `persist_model_call_cancelled_in_flight`
   调用点）用 `if false { … }` 包死 →
   - `rr1_f39_fence_cancelled_before_write…` 红（rows 0≠1，tests 行 3030）
   - `rr1_f39_fence_mismatch_cancelled_during_audit…` 红（rows 0≠1，tests 行 3143）
   - cargo exit **101**（mut-r3b-f39.log）
   - 对照 `rr1_f38_driver_cancel_of_in_flight_stream…` 仍绿，exit **0**
     （mut-r3b-f38driver.log）——证明 f39 双腿钉的是 fence 臂本身，与竞态臂互不代偿。
3. **MUT-R3A（事实降级变异）**：仅把 fence 臂 `transport_attempts:
   Some(provider_turn.transport_attempts)` 改为 `None`（行仍在、事实被抹）→
   双腿红在 tests 行 2944 的专用断言（"not None (unknown), not 0 (not-sent)"），
   exit **101**（mut-r3a-f39.log）——证明断言面钉住回合真实事实，非仅行存在。
4. **还原校验**：`cmp` 副本 runs.rs 与主树逐字节一致；副本内 15/15，exit 0
   （copy-restore-full.log）。变异只存在于已删除的隔离副本。

## 三、生产链路读码核对（本轮亲自读）

- `runs.rs:1319-1365` 取消竞态臂 / `runs.rs:1503-1563` fence-Stale+cancelled 臂：
  两臂都在 `settle_cancellation` 之前经 `persist_model_call_cancelled_in_flight`
  （runs.rs:3612-3626，只写 `record_model_call_usage` 台账行、不写
  `model_call_completed` 事件）落行；fence 臂携带回合真实 usage/attempts/resolved
  身份，竞态臂诚实 unknown/None。
- `runs.rs:825-841` `fence_verdict`：mismatch→`fence_mismatch`、matches+cancelled→
  `cancelled_before_write`（ports.rs:826-827 的 name() 字符串与测试断言一致）。
- `workerrpc.rs:1381-1452` deadline 到期分支：guard 先于 dispatch 武装，到期确定性
  `await model.abandoned(fact)`；`CallbackAbandonGuard::Drop`（949-985）脱离到运行时，
  无运行时响亮报错。
- `workermodel.rs:409-449` `GatewayWorkerModel::abandoned`：经生产 trace 落行，
  parent_tool_call_id 随行、outcome=Cancelled、usage Unknown、attempts None、
  provider/model=unreported。
- `kernel/usage.rs:410` `transport_attempts: Option<u32>`（None=drop 后未知）。
- F22/F23 面：`operations/rerank.rs:180-210` 原样透传（null=缺失、字符串/浮点留给
  严格解码器判 Invalid，total 仅在两半真 u64 时合成）；`models/usage.rs:182-207`
  strict_token 类型化诊断（不回显 payload）；`models/usage.rs:271-283` Google
  output=candidates+thoughts（saturating、缺失≠0）；`salvage_usage_report`
  复用同一严格解码器。
- R3 纯测试侧声明核实：主树 runs.rs SHA 与 R3 执行者登记值一致；本轮唯一被修改的
  仓库文件是本目录与 RR1_ISSUE_MATRIX.json（矩阵登记）。

## 四、判定

- **F21 PASS**：R2 已 PASS 的面（P4/P8/父子 JOIN/查询字段/operation 上下文/取消三腿/
  parse-Err 拾回）本轮全部亲测复绿；F39 关闭后全路径负控齐备。
- **F22 PASS**：R1/R2 结论维持（本轮 7/7、14/14、adapters 全量、读码复核）。
- **F23 PASS**：R1/R2 结论维持（同上；Google 口径与对照腿全绿）。
- **F39 PASS（本轮关闭）**：双腿确定性（5 次重复）、臂粒度变异双证（MUT-R3B 整体
  中性化红/竞态臂对照绿；MUT-R3A 事实降级红在专用断言）、重启可查断言走真实库。
- 未发现新的 R05 必需缺陷；无新 F-ID 登记。
