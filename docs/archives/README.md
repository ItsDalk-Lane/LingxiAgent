# 历史档案

2026-09-06 从根目录归档仍有独有设计、基线、失败或验收证据的文件，并删除重复任务锚点。当前机制与开发入口见[文档索引](../README.md)；本目录的“当前”“下一步”和批准记录仅描述原任务。

档案正文中的源码路径、命令和文件名按当时仓库根目录理解；它们不是在档案目录执行的命令。原始 Git 变更清单和 artifact 文件名保留历史值，不为迁移重写。根目录 `PROGRESS.md` 与两份 `UPSTREAM_SYNC_*` 文档仍服务现役审计链，继续保留原路径。

## Notebook-first 初版

[findings.md](knowledge-notebook/findings.md) 保留初版需求裁决、解析器实验与证据边界。重复的 `task_plan.md` 和 `knowledge_progress.md` 已删除；最终阶段摘要同时保留在根目录 [PROGRESS](../../PROGRESS.md)。

## 知识检索与 Research 重构

| 文件 | 保留原因 |
| --- | --- |
| [原始任务书](<knowledge-retrieval-research/LingxiAgent 知识库检索与 Research Agent 重构执行任务书.md>) | 原始验收规格，按原字节保存；SHA-256 为 `803bc56323026b2d8f55de1f490988da4a9646079a1126760b801d6a74cb6e02`。 |
| [固定基线](knowledge-retrieval-research/KNOWLEDGE_REFACTOR_BASELINE.md) | 旧检索路径计时、环境与原始验证。 |
| [阶段进度](knowledge-retrieval-research/KNOWLEDGE_REFACTOR_PROGRESS.md) | 逐项提交、失败复验及后续组装修复记录。 |
| [实施报告](knowledge-retrieval-research/KNOWLEDGE_REFACTOR_IMPLEMENTATION_REPORT.md) | P0–P3 的方案与兼容边界。 |
| [测试报告](knowledge-retrieval-research/KNOWLEDGE_REFACTOR_TEST_REPORT.md) | 固定源码、平台、退出码和未执行范围。 |
| [性能报告](knowledge-retrieval-research/KNOWLEDGE_REFACTOR_PERFORMANCE_REPORT.md) | 真实测量、数据规模与质量边界。 |

## 模型观测

- [分阶段实现](model-observability/OBSERVABILITY_IMPLEMENTATION_NOTES.md)：存储、查询、追踪和载荷的设计与覆盖矩阵。
- [语义来源审计](model-observability/SEMANTIC_INPUT_PROVENANCE_AUDIT.md)：原 prompt 构造链与 caller 迁移清单。
- [统一实施事实](model-observability/OBSERVABILITY_UNIFICATION_FINDINGS.md)：2026-09-01 改动前后事实，保留真实视频供应商 `NOT_EXECUTED` 边界。

## 工具契约修复

| 文件 | 保留原因 |
| --- | --- |
| [基线](tool-invocation/TOOL_INVOCATION_REPAIR_BASELINE.md) | 工具入口、权限、凭据路线与迁移分界。 |
| [执行进度](tool-invocation/TOOL_INVOCATION_REPAIR_PROGRESS.md) | 唯一完整的逐项红绿结果、提交、日志和 P12 候选失败／恢复链。 |
| [修复报告](tool-invocation/TOOL_INVOCATION_REPAIR_REPORT.md) | 发现项关闭矩阵与实现边界。 |
| [测试报告](tool-invocation/TOOL_INVOCATION_REPAIR_TEST_REPORT.md) | 原始验证结果与日志索引。 |
| [阶段剩余事项](tool-invocation/TOOL_INVOCATION_REPAIR_REMAINING.md) | P11 时点的源码完成与 P12 待验状态，仍属机器证据合同。 |
| [机器事实](tool-invocation/TOOL_INVOCATION_REPAIR_FACTS.json) | 与报告一起由既有边界测试校验；本次仅调整测试读取目录，断言不变。 |

## 其他历史

- [BLOCKED](BLOCKED.md)：历史阻塞、解除与未裁决项，不能自动当作新任务待办。
- [原 AGENTS 状态段](agents-history-2026-09-06.md)：清理前已存在的规则历史快照，本次保持原样。

## 已删除的重复锚点

| 删除文件（原根目录路径） | 内容承接处 |
| --- | --- |
| `task_plan.md`、`knowledge_progress.md` | 初版 findings 与 PROGRESS 的对应阶段记录。 |
| `KNOWLEDGE_REFACTOR_FACTS.json`、`KNOWLEDGE_REFACTOR_REMAINING.md` | 实施、测试、性能报告及既有 `artifacts/knowledge-*`；没有程序读取依赖。 |
| `KNOWLEDGE_REFACTOR_RELEASE_DIGEST_DRAFT.md` | 实施报告及 `artifacts/knowledge-release-digest-source.json`；已过时的未发布文案不继续充当发布输入。 |
| `OBSERVABILITY_UNIFICATION_TASK_PLAN.md`、`observability_unification_progress.md` | 统一实施事实保留最终决策、验证统计和未执行边界。 |
| `TOOL_INVOCATION_V0134_TASK_PLAN.md`、`TOOL_INVOCATION_V0134_PROGRESS.md`、`TOOL_INVOCATION_V0134_FINDINGS.md` | 修复基线、报告与完整执行进度保留迁移依据、候选失败和恢复过程。 |

以上 10 份均存在于清理前提交 `11d53f00c0600ffc2c771335422ec0c5d758c486`。需要追溯原始过程时可按原路径读取，例如：

```bash
git show 11d53f00c0600ffc2c771335422ec0c5d758c486:task_plan.md
```

本次不删除 Git 历史、已有原始测试产物或历史未解决项，也不因归档而把原 `PARTIAL`、失败或 `NOT_EXECUTED` 改成通过。
