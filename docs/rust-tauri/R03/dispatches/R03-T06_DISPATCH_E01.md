# DISPATCH: EXECUTOR-R03-T06-E01（执行子代理，一次性）

- TASK_ID: R03-T06
- TASK_NAME: 子代理、后台运行与权限继承
- TASK_BASE_SHA: 6465395cf8e84121ea55b1481afc5711a4d707e8
- BRANCH: codex/rust-tauri-migration
- WORKSPACE: /Users/study_superior/Desktop/Code/LingxiAgent
- ACCEPTANCE_IDS: R03-A11, R03-A12
- REPORT_PATH: docs/rust-tauri/R03/R03-T06_REPORT.md
- EVIDENCE_ROOT: artifacts/rust-tauri/R03/T06-E01/
- 派发时间: 2026-09-29
- 前置：T01–T05 DONE（…/0bf067d9e/6465395cf）

## Task 规格（阶段书原文为准）

做什么：统一生命周期而不改变现役调用策略。
1. 把现役 subagent 的派发/回复/关闭映射到 child run/thread，保留父子消息可见性规则及实验开关默认值。
2. 子任务权限取父授权与子工具范围交集；它不能因换模型或执行器扩大权限。
3. 后台任务使用同一 Supervisor，但与前端连接寿命解耦；定义桌面关窗、客户端断线、服务退出差异。
4. 保留 parentRunId/origin/sourceMessageId/causeId，后续 Bridge/cron 接入不得自行另建第二调度器。
必须交付：子代理运行适配；后台提交接口；权限衰减规则。

验收：
- R03-A11 子代理不能升级权限：只读父任务派生子任务；子任务尝试写文件；统一授权拒绝，父子关系与原因保留；证据=权限继承测试。测试替身不能自己决定"应该拒绝"——必须由真实运行层授权边界产生拒绝。
- R03-A12 断线不等于取消：任务按后台或持续执行策略运行；断开并重连客户端；不重复提交；任务按原策略继续且可查询；证据=断连恢复测试。

## 总控细化（同一派单组成部分）

- 「现役 subagent」语义以 Node 生产栈真实实现为准（按 90 来源定位：派发/回复/关闭、父子消息可见性、实验开关默认值），先读原实现再映射，不发明语义；「子代理」是灵犀运行时功能，不是本次编码执行者。
- 权限衰减规则落在真实运行层授权上下文（复用 T05 journal 的 authorized 判定链与 R02 认证身份），拒绝由授权边界产生并保留父子关系与原因；不能从调用参数伪造授权、不能借换模型/执行器扩大权限。
- 后台任务走同一 TaskSupervisor/SessionSupervisor（不另建第二调度器），与前端连接寿命解耦：客户端断线≠取消；桌面关窗/服务退出语义定义清楚（服务退出收束归 T07 完整策略，本 Task 定义差异并接最小退出钩子）。
- parentRunId/origin/sourceMessageId/causeId 四元身份持久化。
- 只做运行层接入，不提前迁 R07 全部平台入口（cron/Bridge 完整接入=R07）。
- 存储新增走版本化增量迁移（V4+）；V1/V2/V3 已发布指纹不动；登记递延 T08。
- 环境红线：不引入 tokio test-util。

## 强制携带修复（本 Task 必做）

T03 审查 R1-D1（docs/rust-tauri/R03/R03-T03_REVIEW_R1.md）：task_supervisor.rs drain_run 到期分支 abort() 死代码——JoinHandle 在 timeout 前被 take() move，到期分支再取必为 None；模块文档/注释/tracing「abort sent」失实。修复：timeout 前保存 abort_handle()，使到期分支真实 abort；同步更正文档/注释/tracing 声称。修后必须重跑 task_supervisor 单测 + cancellation_tree + workspace 全量，并在报告逐项记录。
