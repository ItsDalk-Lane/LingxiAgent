# DISPATCH: EXECUTOR-R03-T07-E01（执行子代理，一次性）

- TASK_ID: R03-T07
- TASK_NAME: 故障恢复与服务退出策略
- TASK_BASE_SHA: 0c138456bbe791f21621f5a24eb47bc1948f7585
- BRANCH: codex/rust-tauri-migration
- WORKSPACE: /Users/study_superior/Desktop/Code/LingxiAgent
- ACCEPTANCE_IDS: R03-A13, R03-A14
- REPORT_PATH: docs/rust-tauri/R03/R03-T07_REPORT.md
- EVIDENCE_ROOT: artifacts/rust-tauri/R03/T07-E01/
- 派发时间: 2026-09-29
- 前置：T01–T06 DONE（…/6465395cf/0c138456b）

## Task 规格（阶段书原文为准）

做什么：能解释 interrupted，不把重启自动当成功。
1. 启动扫描非终态 run，根据 journal 区分可恢复等待、可重试只读和 unknown 外部副作用。
2. 对无法安全继续的任务写 interrupted/needs_attention，对用户暴露原因及可执行后续动作；不虚构模型最终回复。
3. 退出先拒绝新提交，按任务类型取消/等待，再 flush/join；有明确 timeout 与残留报告。
4. 使用可控崩溃点测试每个关键持久化边界，保留重放种子。
必须交付：RecoveryCoordinator；恢复分类表；崩溃点测试集。

验收：
- R03-A13 运行中重启诚实呈现：持久化任务开始后强制结束进程；重启并读取历史状态；显示对应恢复/中断状态，无空白或假成功；证据=崩溃前后轨迹。
- R03-A14 退出不接收新任务：shutdown 已开始；并发提交新任务；拒绝新提交，已有任务按策略收束；证据=退出窗口竞态测试。

## 总控细化（同一派单组成部分）

- 复用既有件：T05 classify_invocation_recovery（四态六决策）与 recover_run_invocations；T06 后台 drain 相位；R02 shutdown.rs/SERVICE_START_AND_SHUTDOWN.md 关闭链与退出码表。RecoveryCoordinator 把启动扫描接到这些件上，不另建第二恢复通道。
- 非终态 run 恢复分类表：可恢复等待 / 可重试只读 / unknown 外部副作用 / interrupted_needs_attention；每类有明确动作与用户可见原因；不虚构模型最终回复；终态不复活（T01 契约）。
- 退出序：先关提交入口（A14 竞态窗口：shutdown 开始后并发提交必须拒绝）→ 统一截止时间等待或取消受管工作 → 提交关键记录（journal/终态）→ 回收资源；残留如实报告（T03 StopUnconfirmed 语义延续）。
- 崩溃点测试集：覆盖关键持久化前后崩溃点（run 状态迁移前/后、journal intent/started/receipt 前/后、终态提交前/后…），用可控崩溃点（真实子进程 kill 或注入式受控故障点）+ 可重放 seed/轨迹；A13 需真实进程级强杀证据（T05 递延件）。
- 新存储走版本化增量迁移（V5+）；V1–V4 已发布指纹不动；登记递延 T08（与 V2/V3/V4 一并）。
- 环境红线：不引入 tokio test-util；崩溃测试不得留残留进程/文件（边界、归属与清理证据）。

## 现场事实

- T05 已证同进程 drop+reopen 形态；本 Task A13 升级为真实进程强杀（spawn lingxi-service 二进制或测试二进制 + kill -9），崩溃前后轨迹落证据目录。
- R02 已有 A11/A12 损坏库/备份与 S2 kill -9 存活语义（scripts/rust-tauri/r02_t04_storage_tx.sh 参考）；r02 脚本 S4 在当前树上 exit 1 是已知 T08 修复项，勿在本 Task 顺手修（避免范围混淆），除非你的测试直接依赖它——若依赖则明示。
