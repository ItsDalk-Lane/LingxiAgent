# DISPATCH: EXECUTOR-R03-T04-E01（执行子代理，一次性）

- TASK_ID: R03-T04
- TASK_NAME: attempt/stream 栅栏处理迟到结果
- TASK_BASE_SHA: 9e319d64836d5a1656908a81e23ab5edf8370515
- BRANCH: codex/rust-tauri-migration
- WORKSPACE: /Users/study_superior/Desktop/Code/LingxiAgent
- ACCEPTANCE_IDS: R03-A07, R03-A08
- REPORT_PATH: docs/rust-tauri/R03/R03-T04_REPORT.md
- EVIDENCE_ROOT: artifacts/rust-tauri/R03/T04-E01/
- 派发时间: 2026-09-29
- 前置：T01/T02/T03 DONE（8b2f2cd59 / d075ec8a6 / 9e319d648）

## Task 规格（阶段书原文为准）

做什么：旧运行结果不能污染新一轮。
1. 所有异步返回带 runId/attempt/generation；在写状态前核对，不只在请求发出时检查。
2. 工具/模型返回晚于取消、会话切换或重试时，保留审计但不推进已结束任务或新任务。
3. 历史重连只订阅，不重新启动模型；重复提交采用明确 requestId 去重且校验请求摘要。
4. 并发属性测试生成重复、乱序和延迟事件；发现非法状态序列必须失败。
必须交付：结果栅栏；请求去重；状态属性测试。

验收：
- R03-A07 旧结果不能复活任务：attempt1 已取消、attempt2 或下一 Run 已开始；投递 attempt1 结果；只记为 stale，不追加到当前正文/成功状态；证据=跨 attempt 断言。
- R03-A08 相同 requestId 不同内容拒绝：首次提交已被接收；用相同 ID 改内容重发；不复用旧执行、不新增执行，返回冲突；证据=幂等冲突测试。

## 总控细化（同一派单组成部分）

- requestId 去重必须绑定可信主体、会话和规范化请求内容；不能以全局 requestId 让不同主体互相串用；同 ID 改内容明确冲突（乱序到达的重复 settled 已在 T01/A02 覆盖，本 Task 聚焦提交面去重与栅栏审计）。
- 迟到结果的审计记账（audit-only stale）是本 Task 交付；T01 的身份底线（终态后迟到事件/未开 attempt 响亮拒绝）要升级为"拒写但留审计痕迹+可诊断"的完整栅栏语义。
- 重连只读或续订阅（EventService 游标续读已是 R02 能力），不得重新提交执行。
- 确定性排列+属性测试覆盖重复、乱序、延迟三形态；非法状态序列必须失败（沿 T01 run_finalize_property 的真实库属性测试风格）。
- 环境红线：不引入 tokio test-util feature。

## 现场事实

- T01 交付：record_run_events 对终态后迟到事件与未开 attempt 响亮 Conflict；record_attempt_started 同 run 新 attempt。T03 交付：取消树/两阶段 cancelling→cancelled。R02 交付：EventService cursor 续读、SequentialRequestIdGen 注入面。
- 注意 T03 审查 R1-D1 递延项在 task_supervisor.rs——本 Task 若触碰该文件须一并修复（timeout 前保存 abort_handle()，修后重跑 task_supervisor 单测+cancellation_tree+workspace）；不触碰则继续递延。
