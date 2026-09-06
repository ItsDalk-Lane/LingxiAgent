# 文档索引

先按问题选择入口，再沿文档里的源码链接核对。这里区分维护中的使用说明与历史证据；旧任务的“当前阶段”“下一步”和批准记录只适用于其原始任务。

## 使用与开发

| 要找什么 | 入口 |
| --- | --- |
| 安装、功能与启动 | [README](../README.md) / [English](../README_EN.md) |
| 开发环境、原生依赖与贡献 | [CONTRIBUTING](../CONTRIBUTING.md) |
| 按改动选择验证 | [测试说明](../tests/README.md) |
| 插件开发 | [插件指南](../PLUGINS.md) / [English](../PLUGINS_EN.md)、[SDK](../PLUGIN_SDK.md) |
| 安全报告与项目边界 | [SECURITY](../SECURITY.md) |

## 架构入口

| 主题 | 当前入口 |
| --- | --- |
| 知识库：当前聊天查阅、冻结范围、引用 | [知识库执行链](architecture/knowledge.md) |
| 模型调用、用量、持久化与载荷 | [模型观测](architecture/model-observatory.md) |
| 工具身份、权限、代次与规范执行 | [工具调用路径不变量](architecture/tool-invocation-path-invariance.md) |

架构入口记录源码机制，不代表某个安装包、操作系统或真实供应商已经验收。依赖版本与命令以 [package.json](../package.json) 为准，发布物要绑定其源码和独立证据。

## 发布与审计

- [版本与 Artifact 顺序](../RELEASE_VERSIONING.md)：发布世代、Train、版本与运行时选择。
- [封印流程](../PROGRESS.md#seal-工作流合并后现行)：仅在任务要求封印，或已有授权的提交、发布流程需要时执行。
- [PROGRESS](../PROGRESS.md)：保留各日期、候选源码的验证与封印记录。文首坐标可推进，旧条目的测试结果仍绑定原候选。
- [上游同步审计](../UPSTREAM_SYNC_AUDIT.md)与[矩阵](../UPSTREAM_SYNC_MATRIX.md)：上游同步历史及后续坐标投影。矩阵由 [.sync-audit/build-sync-matrix.mjs](../.sync-audit/build-sync-matrix.mjs) 生成。
- [历史阻塞与裁决](archives/BLOCKED.md)：恢复相关任务前须重新核对。

## 历史任务与证据

有独有设计或验收证据的文件已移至 `docs/archives/`；重复阶段计划、进度摘要和过期草稿已删除。保留清单、删除依据及恢复方法见[档案索引](archives/README.md)，不把历史测试数或工作树状态改写为当前验证结果。

| 历史主题 | 记录与适用范围 |
| --- | --- |
| Notebook-first 初版与后续安全补充 | [初版事实](archives/knowledge-notebook/findings.md)。2026-08-25 起的设计与实验记录；普通提问行为已继续演进。 |
| 知识 P0–P3 重构 | [实施报告](archives/knowledge-retrieval-research/KNOWLEDGE_REFACTOR_IMPLEMENTATION_REPORT.md)、[验收与原始规格](archives/README.md#知识检索与-research-重构)。快速／详细双入口描述属于该阶段。 |
| 模型观测及后续统一 | [实现记录](archives/model-observability/OBSERVABILITY_IMPLEMENTATION_NOTES.md)、[统一事实](archives/model-observability/OBSERVABILITY_UNIFICATION_FINDINGS.md)。旧 schema、视频策略、阶段状态和真实供应商验证保留原始边界。 |
| 工具契约修复及 v0.1.34 迁移 | [修复报告](archives/tool-invocation/TOOL_INVOCATION_REPAIR_REPORT.md)、[证据链](archives/README.md#工具契约修复)。后续合并与封印按 PROGRESS 对应记录核对。 |
| 旧项目规则状态段 | [2026-09-06 保存的历史快照](archives/agents-history-2026-09-06.md)，仅用于追溯。 |

## 维护约定

- 改实现时同步对应的使用或架构入口；历史记录保留当时结果，过时说法通过日期和范围说明隔开。
- 根目录保留现役入口及有固定路径依赖的审计文件。阶段计划、断点与验证流水放任务专属目录；收口后删除重复锚点，独有证据按主题归档。
- 本地 Agent 规则位于根目录 `AGENTS.md`；它被 `.gitignore` 忽略，不随 clone 分发。共用的开发与验证说明放在上述受版本管理的文档中。本仓库没有要求另建 `CLAUDE.md` 镜像。
- Agent 记忆只保留短索引、偏好与可复用经验；生成记忆使用宿主允许的更正入口，不能用旧记忆覆盖当前源码或当前用户指令。
