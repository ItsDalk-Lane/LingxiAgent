# DISPATCH: EXECUTOR-R03-T05-E01（执行子代理，一次性）

- TASK_ID: R03-T05
- TASK_NAME: 运行日志与副作用收据
- TASK_BASE_SHA: 0bf067d9ec329108408f04b00f37f2635fc4a55d
- BRANCH: codex/rust-tauri-migration
- WORKSPACE: /Users/study_superior/Desktop/Code/LingxiAgent
- ACCEPTANCE_IDS: R03-A09, R03-A10
- REPORT_PATH: docs/rust-tauri/R03/R03-T05_REPORT.md
- EVIDENCE_ROOT: artifacts/rust-tauri/R03/T05-E01/
- 派发时间: 2026-09-29
- 前置：T01–T04 DONE（8b2f2cd59 / d075ec8a6 / 9e319d648 / 0bf067d9e）

## Task 规格（阶段书原文为准）

做什么：崩溃恢复能够判断能否重试。
1. 为工具请求建立 prepared→authorized→started→succeeded/failed/unknown 收据，与参数摘要和目标代次（run/attempt/generation）绑定。
2. 副作用执行前持久化开始意图；执行后持久化外部响应/可用去重标识，不声称跨外部系统有分布式原子事务。
3. 恢复时把「已执行但回执未持久化」归为 unknown，按工具幂等性和外部查询能力决定核验、重试或人工确认。
4. 只读或受证实幂等任务允许有界自动恢复；发送消息/付款/破坏写等未知结果禁止盲重试。
必须交付：InvocationJournal；副作用恢复策略；unknown 结果结构。

验收：
- R03-A09 副作用后崩溃不重复执行：替身计数器已增加但结果提交前 kill 服务；重启恢复；工具调用总次数不再自动增加，收据显示 unknown；证据=外部计数器和恢复库。
- R03-A10 可验证幂等恢复：工具支持固定 idempotency key；在响应前中断后恢复；同一外部操作不重复，核验结果后结束；证据=幂等替身请求记录。

## 总控细化（同一派单组成部分）

- 收据语义集：prepared / authorized / started / succeeded / failed / unknown；绑定主体（owner）、运行（run/attempt/generation）、目标、参数摘要与幂等键（如有）。
- 执行前持久记录意图（prepared/authorized/started 按 R03 运行层授权上下文判定），执行后记录可验证回执；外部执行成功与本地提交之间没有天然跨系统原子事务——不得声称有。
- 恢复分类四态：未执行 / 确认完成 / 确认失败 / 结果无法确认（unknown）。仅已验证外部幂等键或状态核验条件下自动恢复；非幂等未知副作用禁止盲目重发（进入 needs attention 类可解释状态，完整恢复推进归 T07，本 Task 交付分类决策与收据数据面）。
- R03 只做运行层授权上下文与收据，不重写 R04 完整工具策略网关。
- A09/A10 的 kill-重启：可用隔离子进程或同进程 drop+reopen 库模拟崩溃边界，但必须真实持久化边界（journal 先于外部执行落盘、回执后落盘）；外部计数器用受控本地替身（独立文件/进程），不真实外发。
- 存储新增走版本化增量迁移（V3…），不改已发布迁移校验值；T04 的 V2 已存在。
- 环境红线：不引入 tokio test-util feature。

## 现场事实

- T04 刚加 migration V2（stale_result_audit）与 record_stale_result；T04 审查 R1-F1（R02-T04_STORAGE_REGISTRY.json 未登记 V2 + r02_t04_storage_tx.sh S4 硬编码 version==1）强制递延 T08——本 Task 若新增 V3，登记问题同样递延 T08 一并处理，报告注明。
- runs.rs 驱动层已有 ToolExecutorPort 与 fence 校验（Unknown 结果不伪成功不盲重试）——InvocationJournal 接在工具执行路径上。
- task_supervisor.rs 的 T03 R1-D1 若被本 Task 触碰须一并修复；否则继续递延。
