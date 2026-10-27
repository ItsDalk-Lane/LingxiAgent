# DISPATCH: EXECUTOR-R03-T03-E01（执行子代理，一次性）

- TASK_ID: R03-T03
- TASK_NAME: 取消树和子任务监督
- TASK_BASE_SHA: d075ec8a62bdd9e8346c450b9a4b62668554d9cd
- BRANCH: codex/rust-tauri-migration
- WORKSPACE: /Users/study_superior/Desktop/Code/LingxiAgent
- ACCEPTANCE_IDS: R03-A05, R03-A06
- REPORT_PATH: docs/rust-tauri/R03/R03-T03_REPORT.md
- EVIDENCE_ROOT: artifacts/rust-tauri/R03/T03-E01/
- 派发时间: 2026-09-29
- 前置：R03-T01 DONE（8b2f2cd59）、R03-T02 DONE（d075ec8a6）

## Task 规格（任务书原文为准）

做什么：取消必须传播到所属工作而不是只改界面状态。
1. 每个 run 建 CancellationToken/等价机制，child attempt/model call/tool/subagent 继承父范围；独立后台任务只有显式依赖才被连带取消。
2. 区别请求取消、清理完成和无法确认停止；到达截止时间后报告未清理项，不假称系统已安静。
3. 取消审批等待、排队等待及网络流；受管工作退出后回收资源并写最终状态。
4. 对 child panic/error/aborted 建监督返回，禁止 fire-and-forget 丢失子任务句柄。
必须交付：取消树；TaskSupervisor；清理期限策略。

验收：
- R03-A05 取消覆盖等待态：任务分别停在队列/审批/流读取；发送取消；三类等待均退出且配额归还，不额外调用工具；证据=参数化取消测试。
- R03-A06 独立任务不被误杀：父子任务和无关后台任务同时运行；取消父任务；所属子任务停止，无关后台任务继续；证据=监督关系和活动断言。

## 现场事实与边界

- T01 已交付 RunSupervisor/唯一 finalize（cancelled/interrupted 已是合法终态）；T02 已交付 SessionSupervisor/配额 RAII（abort 原语走的正是 RAII 释放路径）。本 Task 把取消形式化为树：run → attempt/model call/tool call/child run 范围继承。
- 总控细化要求：区分「收到取消/开始清理/已确认终止/无法确认停止」四阶段语义；不能仅改前端状态或 drop 句柄就宣称完成；所有异步任务有 owner、可回收句柄和退出结果，禁止无人监督的 fire-and-forget；定义不配合取消、清理超时和重启后的可解释状态；对已发生的外部动作不承诺撤销。
- waiting_approval 进入路径：R04 才有完整审批网关。本 Task 允许建立最小接口/测试适配器让 run 进入审批等待（如替身工具请求审批），不实现完整工具策略网关。
- 子代理运行关系（child run/thread 映射、权限继承）是 T06 范围；本 Task 的"子任务"是取消树所属的 child attempt/model call/tool call 及演示性 child run。
- 清理期限策略要有真实超时与未清理项报告，不假称安静。
- 环境注意：不要给测试引入 tokio test-util feature（T02 证实会破坏 LAN 自连用例）；用无 feature 的确定性方案。
