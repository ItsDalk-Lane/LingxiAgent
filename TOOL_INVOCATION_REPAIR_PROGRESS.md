# 工具契约执行路径不变量修复进度

## 执行约束

- 唯一实施依据：`/Users/study_superior/Downloads/LingxiAgent 契约执行架构“语义与路径无关”全链路修复执行任务书.md`
- 固定基线：`4fefe66ec3b4f6b23c78a09869a607886585740e`
- 执行分支：`fix/tool-contract-path-invariance`
- 顺序：严格按 P0-00 → P12-06 推进。
- 每个实现项遵守：先补回归测试并确认旧实现按预期失败，再实现、运行门禁、记录原始结果与提交 SHA、推送后进入下一项。
- 任务书本身承担任务计划职责；本文件承担进度与错误日志职责；`TOOL_INVOCATION_REPAIR_BASELINE.md` 承担现状事实记录职责。保留仓库已有其他任务的 `task_plan.md`、`findings.md` 和 `PROGRESS.md`，不覆盖。

## 当前任务

### P0-00 固定 Git 基线并创建执行分支

- 状态：`completed`
- 改动文件：`TOOL_INVOCATION_REPAIR_BASELINE.md`、`TOOL_INVOCATION_REPAIR_PROGRESS.md`
- 测试命令：不适用；本项为基线、分支与依赖准备。
- 原始结果：`git fetch origin --prune` exit `0`；远端来源引用核对 exit `1`；固定提交可达；`npm ci` exit `0`。
- 日志路径：`/tmp/lingxi-tool-contract-p000-setup.log`
- 提交 SHA：提交后在下一项进度更新中回填
- 偏差：远端来源分支已不存在；按任务书规则从仍可达的固定 SHA 建分支。

## 后续任务

| 编号 | 状态 | 提交 SHA | 备注 |
| --- | --- | --- | --- |
| P0-01 | pending | — | 基线门禁 |
| P0-02 | pending | — | 现状矩阵与入口清单 |
| P1-01 | pending | — | 目标身份、路由和错误类型 |
| P1-02 | pending | — | 新旧权限方言规范化 |
| P1-03 | pending | — | 完整 schema 校验器 |
| P2-01 | pending | — | 会话级目标注册表 |
| P2-02 | pending | — | PreparedInvocation 与统一网关 |
| P3-01 | pending | — | 插件元数据与 target adapter |
| P3-02 | pending | — | Engine 装配顺序 |
| P4-01 | pending | — | Catalog 目标引用目录 |
| P4-02 | pending | — | Bridge Gateway 适配 |
| P4-03 | pending | — | MCP eligibility 与执行器 |
| P5-01 | pending | — | Plugin 工具代次 |
| P5-02 | pending | — | MCP live generation |
| P5-03 | pending | — | 旧会话撤销语义 |
| P6-01 | pending | — | plugin-dev 聊天身份 |
| P6-02 | pending | — | LocalDeveloperPrincipal |
| P7-01 | pending | — | 媒体执行目标解析器 |
| P7-02 | pending | — | 媒体入口统一 |
| P8-01 | pending | — | rerank policy 共享执行器 |
| P9-01 | pending | — | 统一错误映射 |
| P9-02 | pending | — | raw execution 边界检查 |
| P10-01 | pending | — | 路径等价变形测试 |
| P10-02 | pending | — | 完整配置组合 |
| P11-01 | pending | — | 架构文档 |
| P11-02 | pending | — | 报告、机器事实、源码候选 |
| P12-01 | pending | — | 定向契约门禁 |
| P12-02 | pending | — | 静态与全量测试门禁 |
| P12-03 | pending | — | 构建门禁 |
| P12-04 | pending | — | 固定源码 SHA 与审计投影 |
| P12-05 | pending | — | 封印后门禁 |
| P12-06 | pending | — | 推送与最终报告 |

## 错误日志

| 时间 | 编号 | 原始错误 | 次数 | 处理 |
| --- | --- | --- | ---: | --- |
| 2026-09-05 13:29 +0800 | 目标初始化 | 当前任务已自动存在，重复创建目标失败 | 1 | 读取现有目标，确认与本次请求一致，不再重复创建 |
| 2026-09-05 13:31 +0800 | P0-00 | `origin/feat/knowledge-retrieval-research-p0-p3` 不存在，引用核对 exit `1` | 1 | 固定 SHA 本地可达，按任务书规则继续，不更换基线 |

## 断点续跑自检

| 问题 | 答案 |
| --- | --- |
| 现在在哪里？ | P0-00 已完成，等待提交并推送 |
| 接下来去哪？ | 提交并推送 P0-00，然后执行 P0-01 基线门禁 |
| 最终目标是什么？ | 证明并修复工具调用语义对执行路径不敏感，完成 P0–P12 全部门禁与审计封印 |
| 已学到什么？ | 固定提交可达；远端来源分支已删除；Node 与依赖安装满足任务书要求 |
| 已做什么？ | 从固定 SHA 新建执行分支并完成 `npm ci` |
