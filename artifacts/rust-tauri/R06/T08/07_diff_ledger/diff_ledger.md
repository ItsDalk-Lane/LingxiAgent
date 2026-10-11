# R06-T08 差异台账（RC-2：逐条锚定，现役行为语义 × 候选状态）

任务为行为评测与交接；产品代码只含一处评测暴露缺陷的根因修复（D-T08-1）。
其余条目为评测方法与账本口径声明。

## D-T08-1（产品修复）：子代理 child run 的 agent 身份与配额 lane 混淆

- **现役锚点**：子代理运行于隔离会话但继承派发代理的人格/记忆（裁剪后）——
  `core/session-coordinator.ts` subagent 派发链（agent 上下文按父代理装配，
  子代理可见性裁剪由 T01 子代理形态承担；R00 PI-01 duty「隔离子会话的模型/
  工具循环」）。R03-T06 候选侧的隔离语义锚点：`src/subagents.rs` 原注释
  「one agent lane and one session lane per subagent thread」。
- **缺陷**：`subagents.rs` 把 drive_run 的 `agent_id` 实参填成 lane 字符串
  `subagent:{agent}`。R03 时代（无上下文编译器）该值仅用作配额 agent lane；
  T01 接线后它同时是**代理材料身份**（`agents/{agent_id}/` 目录解析、
  工具调用归属、用量台账 agent 标识）→ `agents/subagent:{id}/` 不存在 →
  每个子代理 child run 响亮 `context_compile` 失败（failed.provider_error）。
  生产 wired 链上子代理功能整体不可用——评测暴露的正确性下降。
- **修复（只修实际原因，非提示词补丁）**：drive_run 的 agent_id 实参 =
  真实服务代理（`request.agent_id` 回落 `parent.agent_id`）；子代理并发
  隔离由 lane 参数承载：`quota_session_lane` 每 thread 唯一（且纳入代理
  身份段 `{session}::subagent::{agent}::{thread}`）。**有意偏离声明**：
  原实现的「每子代理线程独立 agent lane」随之消失——父与自己的子代理共享
  同一 per-agent lane（它们本就是同一代理身份）；隔离职责由 per-thread
  session lane（现役「隔离子会话」语义的对应物）与全局/进程上限承担。
  R03-T06 全部套件（lifecycle/closeout/permission_inheritance/
  cancel_link_inheritance/background_steering，30 用例）在修复后全绿。
- **回归**：`05_targeted_tests/subagent_regression.txt`（exit 0）+
  `06_workspace_regression/full_workspace.log` + T08 评测套件全绿。

## D-T08-2（方法声明）：旧侧不运行旧引擎

现役行为语义清单 + 金样为金线（每任务 `incumbentAnchor` 字段锚定现役
file:line/语义）；不运行旧引擎的理由与边界见 BEHAVIOR_EVAL.json
`comparison_method`（真实模型未授权 → 旧引擎端到端同样不可比；隔离环境无
Electron 运行授权）。该边界如实声明：能证「Rust 行为符合现役语义金线」，
不能证「真实模型两侧错误率分布一致」（BLOCKED_NOT_AUTHORIZED → R10）。

## D-T08-3（方法声明）：S 层评测腿的委托族注册

BE-08 使用的注册表由测试构建（真实 ToolRegistry + 真实网关 + 真实文件
工具），其中 first-party `subagent` 家族 manifest 为测试注册（生产由委派
实验开关承担注册；线体 tool_call → DelegationRequest 的铸造面生产零调用点
——`with_delegation` 非测试调用点为 0，grep 可证）。子代理 RUN 边界本身
（衰减 GRANT、run 层授权、可见性裁剪）为真实链；与 R03-A11 权威 harness
（subagent_permission_inheritance.rs）同构。委派实验开关的生产接线归 R07
（R06 §1「不引入强制子代理分发」）。

## D-T08-4（评测词汇改锚记录）

评测断言以现役/真实链词汇为准，首轮误锚在 RED 轮按真实链修正（见
02_red_tests/RED.md §2）：BE-02 schema 字面量锚（非 display_name）、
BE-05 `resource_access_denied`/「not inside any authorized root」、
BE-06 族条件 size 规则（gemini 2.5 拒绝 size 输入）、BE-07
`pending_embedding` 终态。全部为「按现役金线改锚」，无放松。

## D-T08-5（BE-08 显式 access:"read"）

现役衰减规则：`access:"read"` → 子代理只读 tier（resolve_subagent_access
显式 Read 分支）。评测腿采用显式衰减（父会话保持可操作）以隔离「衰减
GRANT」与「会话只读策略拒绝」（后者在网关策略层阻止只读会话的委托家族
派发——首轮观察到的 ACTION_BLOCKED_BY_READ_ONLY [session] 即该层，如实
记录）。两臂皆为现役语义。

## 载体与候选范围

- 候选 = `rust/crates/lingxi-service/tests/r06_t08_behavior_eval.rs`（新增）
  + `rust/crates/lingxi-service/src/subagents.rs`（D-T08-1 修复）
  + `docs/rust-tauri/R06/{R06_REPORT.md, R06_HANDOFF.json, BEHAVIOR_EVAL.json}`
  + `docs/rust-tauri/R00/PI_REPLACEMENT_MATRIX.json`（r06_t08_handoff 追加）
  + `artifacts/rust-tauri/R06/T08/`（全部证据，digest 口径外）。
- `rust/Cargo.lock` 零改动（git diff 为空）；无新增第三方依赖。
- `.vscode/` 为 IDE 噪声，永不入候选（客观在 digest 输入内，T06/T07 同先例申报）。
