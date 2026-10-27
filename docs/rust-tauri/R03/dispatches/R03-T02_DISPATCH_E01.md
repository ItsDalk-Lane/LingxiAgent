# DISPATCH: EXECUTOR-R03-T02-E01（执行子代理，一次性）

- TASK_ID: R03-T02
- TASK_NAME: 会话串行化和全局限流
- TASK_BASE_SHA: 8b2f2cd595625e17b4153af072efc8484b79d5b6
- BRANCH: codex/rust-tauri-migration
- WORKSPACE: /Users/study_superior/Desktop/Code/LingxiAgent
- ACCEPTANCE_IDS: R03-A03, R03-A04
- REPORT_PATH: docs/rust-tauri/R03/R03-T02_REPORT.md
- EVIDENCE_ROOT: artifacts/rust-tauri/R03/T02-E01/
- 派发时间: 2026-09-29
- 前置：R03-T01 DONE（提交 8b2f2cd59，review R1 PASS）

## Task 规格（任务书原文为准）

做什么：并发不造成同会话状态竞争。
1. 每个会话由一个明确 owner 顺序处理状态修改；模型/工具 I/O 不在全局锁中等待。
2. 保留已采纳的排队、追问或打断语义；把普通新提交与 steering/follow-up 区分，不擅改产品交互。
3. 为不同会话并行设置全局/agent/session 工具与模型配额；使用有界队列，超限按协议拒绝或排队。
4. 带超时的等待释放必须可取消；任务取消/失败归还配额，不出现 permit 泄漏。
必须交付：SessionSupervisor；并发/排队策略；配额管理器。

验收：
- R03-A03 同会话顺序可重复：同一会话同时提交两个任务；可控调度下推进；按冻结语义排队/打断，消息不交叉写入；证据=调度轨迹。
- R03-A04 跨会话不被全局锁阻塞：会话A工具暂停、会话B纯文本；并发执行并取消A；B正常结束、A配额最终释放；证据=并发及配额断言。

## 现场事实

- T01 已交付 RunSupervisor（rust/crates/lingxi-service/src/runs.rs）与 kernel finalize；RunStatus/Attempt 身份在 lingxi-protocol/lingxi-kernel。R02 已有 sessions.rs、limits.rs（资源上限旗标）、execute_concurrency.rs 测试。注入面 Clock/RequestIdGen 可做确定性测试。
- 「已采纳的排队、追问或打断语义」以现有 Node 生产栈与任务书 90 号来源（S04/S11/S12/S13/S19/W05）为准——先读原实现再映射，不发明新交互语义。
- 禁止：持全局锁等待模型或工具 I/O；无界等待队列；permit 泄漏；把 R04+ 工具网关做进来。
