# G03-E01 对抗性自查（F04 逐 C-ID）

执行代理对抗自检口径：对每个 C-ID 先构造"能推翻修复的反例"（攻击窗口），再观测修复后行为。受控故障点全部落在**外部副作用面/替身**（文件持久计数器、请求日志、幂等去重态、受控 panic 时机），未 mock 被测的 Supervisor 裁决、journal 写序、恢复分类或真实存储链。证据根：`artifacts/rust-tauri/R03/repair-current/G03-E01/adversarial-selfcheck/`。

## R03-FIX-F04-C01｜panic 在外部确认前后，均不得臆测回执

- **攻击窗口**：若修复只按"外部已确认"路径特判，则 panic 发生在**外部确认之前**（外部未动作）时仍可能被旧逻辑或惯性写成 Failed（或反向臆测成功）；两个方向都要试。
- **攻击实施**：
  - panic AFTER（外部 +1 后 panic）：`f04_c01_panic_after_side_effect_journals_unknown_not_confirmed_failure`（计数 1 → unknown）；
  - panic BEFORE（外部未动作即 panic）：`f04_c01_adv_panic_before_side_effect_is_still_unknown`（计数 0 → 仍 unknown，`fired` 通道证明外部从未被调用）。
- **观测**：两态 receipt 均 `unknown`/`dispatched=true`/dedup_id 无；detail 分别携带 "runner died AFTER / BEFORE" payload——本地**不因外部状态不同而改变类别**（本地无法区分，Unknown 是唯一诚实陈述）；计数器文件是两态差异的唯一事实源（`evidence.json#f04_c01_adv_panic_before`）。
- **是否推翻**：否。两态一致 Unknown，无臆测回执。
- **命令/退出码**：`cargo test -p lingxi-service --locked --test tool_receipt_unknown f04_c01` → exit 0（2 passed）。
- **红基线对照**：两用例在未修复代码均红（`red-baseline-tool_receipt_unknown-final.log`，`got Failed`）。

## R03-FIX-F04-C02｜异常种类不漏分类 + 丢响应两态

- **攻击窗口**：若修复只处理 panic 变体，则中止（Aborted）/通道丢失（Failed）/超时类异常仍可能走旧 Failed 路径；或只在"外部已动作"时给 Unknown。
- **攻击实施**：
  - **两态丢响应**：`f04_c02_dispatched_no_result_variants_classify_consistently_unknown`——同一 run 内"先外部成功再丢响应"（计数 1）与"外部未动作结果未知"（计数 0）并存，断言逐条目 unknown（分类表见 `evidence.json#f04_c02_classification_table`）。
  - **变体枚举**：`runs::tests::every_unobserved_tool_exit_variant_journals_unknown_diagnosably`——对真实 `TaskExit` 四变体（Panicked/Aborted/Failed/Completed）逐一断言 `unobserved_tool_exit_reason` 输出含 "unobserved" 且携带各自异常类标记；驱动侧该分支是唯一 `Err(task_exit)` 入口、不按变体分岔（结构上无法漏分类）。
- **观测**：两态一致 unknown；四变体一致 unknown 语义；`tool_result_wire` 对 unknown 输出 `ToolResultStatus::Unknown`（模型可见面不漏）。
- **是否推翻**：否。
- **命令/退出码**：`cargo test -p lingxi-service --locked --test tool_receipt_unknown f04_c02` → exit 0；`cargo test -p lingxi-service --locked --lib every_unobserved_tool_exit_variant` → exit 0（`unit-taskexit-variants.log`）。
- **如实披露（编排边界）**：`Aborted`/`Failed` 无法在进程内经真实链确定性送入该分支——树取消因 biased select + cancel_recursive 自根向下置序恒先命中 `root.cancelled()` 收束腿（G02 裁决设计），wrapper 级 abort 只发生在 drive 之外的 drain/显式 abort；R03 驱动内不存在工具级等待超时（取消侧清理预算由 G02 持有）。故二者以上述单元枚举 + 单一入口结构覆盖，端到端实测覆盖 panic 两态。与 G01/G02 报告同类披露。

## R03-FIX-F04-C03｜可信负面不因修复被冲掉；拒绝须来自真实边界

- **攻击窗口 A（过度修正）**：若修复把一切 Failed 都转成 Unknown，真实外部失败回执将丢失（审查"不能这样修好"第一条）。
- **攻击窗口 B（谎报授权）**：若拒绝由 Tool 替身自己返回 Failed 冒充"授权拒绝"，则 dispatched=false 是适配器谎报，不成立。
- **攻击实施**：
  - A：`fail.external` 真实执行（计数 +1）后返回结构化 `UpstreamUnavailable` 失败 → 断言 phase failed、`dispatched=true`、决策 `ConfirmedSettled{Failed}`（若被扫成 Unknown，此断言失败）。
  - B：拒绝由 **wiring 的 ApprovalGate 边界**产出（`RejectOneTarget`，与既有 invocation_journal 套件同一真实审批链）；零派发由外部系统 `request_count` 佐证（reject.me 无请求行）——不是替身上报。
  - 对照第三类：`panic.fault` 同 run 并存 → unknown/`dispatched=true`/NeedsAttention。
  - 跨套件佐证：`subagent_permission_inheritance`（子代理衰减层拒绝，另一真实授权边界）4/4 绿，其 `dispatched=0` 断言不变。
- **观测**（`evidence.json#f04_c03_three_classes`）：三类并存、互不混淆；`external_request_count=2`、`executed=2`（恰为 fail.external+panic.fault）。
- **是否推翻**：否。
- **命令/退出码**：`cargo test -p lingxi-service --locked --test tool_receipt_unknown f04_c03` → exit 0。
- **附带同族攻击（delegation 事实）**：delegation 拒绝在旧码写 `dispatched="1"`（把"零子 run 创建"的已知未派发事实记成已派发）——`subagent_permission_inheritance.rs` 新增断言在未修复代码红（`red-baseline-delegation-dispatched.log`：left "1" / right "0"），修复后绿；审查清单未单列此项，作为同根因路径的事实类修正登记（分类不变，仅 dispatched 事实归位）。

## R03-FIX-F04-C04｜重复恢复不重做 Unknown + 可信幂等控制例

- **攻击窗口**：恢复（或重复恢复、重复核验）自己重新执行外部动作；或 Unknown 被静默改写成其他类别；或核验用了**新造 key** 导致外部系统再执行一次。
- **攻击实施**：
  - 主例：panic-after 留下"计数 1 + 本地 unknown"的 dangling-active run → 重启扫描 1（生产形态 bootstrap）→ 直接 journal pass（重复恢复 2）→ 再 bootstrap（重复恢复 3）。断言三次后计数仍 1、`unknown_verdicts_persisted=0`（不二次写 verdict）、pass2 `persisted=false`、scan3 `scanned=0`（终态不复活）、journal 最终 phase unknown 可见、run 类别 interrupted_needs_attention（`evidence.json#f04_c04_repeat_recovery`）。
  - **控制例**：`ledger.idem` 具备受证实幂等能力（外部去重态文件按 key 记录，测试自身断言其行为）→ 恢复决策 `UnknownResumeWithIdempotencyKey`，其 key 与 journal 原 key **逐字相等**（断言 `resume_key == original_key`，防新造 key）；核验经真实 executor 端口以原 key 重放 → 请求 2 次、执行仍 1 次、结果与外部记录一致；verified 收据（Succeeded+dedup=原 key）把 unknown 定案；**二次核验**同 key 再放 → 请求 3 次、执行仍 1 次（`evidence.json#f04_c04_adv_idem_control`）。
  - 替身侧 `disarm`（受控故障开关）只表达"中断已结束、复放的调用应答"（A10 同形态），不改变被测链路。
- **是否推翻**：否。无新外部执行、Unknown 可见、恢复幂等、核验只用原 key。
- **命令/退出码**：`cargo test -p lingxi-service --locked --test tool_receipt_unknown f04_c04` → exit 0（2 passed）。
- **红基线对照**：两用例旧码均红（类别 `recoverable_wait`；控制例拿 ConfirmedSettled 而非 resume-with-key）。

## 稳定性与红基线

- 红基线（隔离 git worktree，HEAD=ccb09fde6 未含修复）：`tool_receipt_unknown` **0 passed / 6 failed**（`red-baseline-tool_receipt_unknown-final.log`；首跑日志 `red-baseline-tool_receipt_unknown.log`）；delegation dispatched 断言红（`red-baseline-delegation-dispatched.log`）。worktree 已删除（`git worktree list` 仅剩主工作区）。
- 稳定性：修复后套件 3 连跑全绿（`stability-3x-suite.log`）。
- 回归底线复核：workspace 67/673/0、fmt 零 diff、clippy `-D warnings` 零告警、Cargo.lock sha1 不变、G01/G02 套件绿（详见 `G03-E01_NORMAL_SELFCHECK.md` 回归总表与 `logs/`）。
