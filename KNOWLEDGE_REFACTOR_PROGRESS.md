# 知识库检索与 Research Agent 重构进度

唯一任务来源：`LingxiAgent 知识库检索与 Research Agent 重构执行任务书.md`。
任务书 SHA-256：`803bc56323026b2d8f55de1f490988da4a9646079a1126760b801d6a74cb6e02`。
固定基线：`3eab85891a1747c64064252804f70c0a3773f021`。
执行分支：`feat/knowledge-retrieval-research-p0-p3`。

## 执行规则与断点

- 严格按 P0 → P1 → P2 → P3 及编号顺序；阶段门禁全部通过才进入下一阶段。
- 每项记录测试原始结果后提交；不删除、跳过或放宽测试，不合并 main。
- 现有任务书为用户未跟踪文件，既有规划文档与 BLOCKED.md 历史记录保留。
- 当前断点：P0-07，P0-06 已完成并提交，正在补充本地过程卡和耗时展示。审计封印顺序冲突详见 BLOCKED.md，P0 阶段收口前须解决。
- 进度、计划与事实集中在本文件和基线文档，避免覆盖既有 task_plan.md / findings.md / PROGRESS.md。
- 目标工具已确认本任务存在 active goal；重复 create_goal 被拒绝，沿用现有目标。
- 规划恢复脚本返回其他会话的无关配置记录，经 git diff 为空核对，未采用其内容。

## 阶段门禁

| 阶段 | 状态 | 结果 |
| --- | --- | --- |
| P0 | pending | NOT_EXECUTED |
| P1 | pending | NOT_EXECUTED |
| P2 | pending | NOT_EXECUTED |
| P3 | pending | NOT_EXECUTED |

## P0-00：建立基线与回归门禁

- 状态：completed
- 改动文件：`KNOWLEDGE_REFACTOR_PROGRESS.md`、`KNOWLEDGE_REFACTOR_BASELINE.md`
- 测试命令：`npm run typecheck`；任务书指定 5 个 knowledge 测试；`npm test`
- 测试结果：类型检查 exit 0；指定知识测试 5 文件 / 110 测试通过、0 失败 / 0 跳过；全量 1273 文件通过 / 1 跳过，12929 测试通过 / 7 跳过 / 0 失败，exit 0；本地旧快速路径 60 次分段计时已记录。
- 对应 commit SHA：`eef41dd6be1035547410d23859ec64490e0adf2b`
- 偏差：none

## P0-01：增加执行策略契约

- 状态：completed
- 改动文件：`shared/knowledge-execution.ts`、`shared/knowledge-refs.ts`、`core/desktop-session-submit.ts`、`core/engine.ts`、`tests/knowledge-execution-policy.test.ts`
- 测试命令：`npx vitest run tests/knowledge-execution-policy.test.ts tests/chat-route-knowledge-refs.test.ts tests/desktop-session-submit.test.ts tests/knowledge-retrieval-golden.test.ts`；`npm run typecheck`；修改文件定向 ESLint
- 测试结果：首轮 4 文件，74 通过 / 2 失败；新测试错误地期待旧模式生产输入返回 null，实际既有契约为抛出 TypeError。已按基线修正为精确错误断言，未改生产兼容行为；重跑 4 文件 / 76 测试全部通过（3.05s，exit 0）；类型检查三套 exit 0；定向 ESLint exit 0。
- 对应 commit SHA：`0586df726267f9f23f0a0dd7a354518235fea77f`
- 偏差：none

## P0-02：实现 ScopeSnapshotCompiler

- 状态：completed
- 改动文件：`lib/knowledge/scope-snapshot-compiler.ts`、`lib/knowledge/knowledge-store.ts`、`lib/knowledge/knowledge-index-store.ts`、`lib/knowledge/knowledge-manager.ts`、`core/engine.ts`、`tests/knowledge-scope-snapshot-compiler.test.ts`
- 测试命令：`npx vitest run tests/knowledge-scope-snapshot-compiler.test.ts tests/knowledge-turn-scope.test.ts tests/knowledge-retrieval-golden.test.ts tests/knowledge-engine-persistence.test.ts`；`npm run typecheck`；修改文件定向 ESLint；`git diff --check`
- 测试结果：首轮 19 通过 / 1 失败（fixture 缺 semanticArtifactPath），第二轮 21 通过 / 1 失败（fixture 缺 ordinal）；首轮类型检查也报告同一 ordinal 缺失。均修正测试数据、保留原生产校验和断言，最终 4 文件 / 26 测试全部通过，0 失败 / 0 跳过（3.03s，exit 0）；类型检查三套 exit 0；定向 ESLint 与 diff 检查 exit 0。日志 `/tmp/lingxi-knowledge-p002-tests-verified.log`、`/tmp/lingxi-knowledge-p002-typecheck-final.log`。
- 对应 commit SHA：`3bfc402c1f216a1209d1576ab3ff10c86c2e9a86`
- 偏差：none

## P0-03：实现 FastKnowledgePipeline

- 状态：completed
- 改动文件：任务书规定 fast pipeline、query service、index store、manager；统一精确证据类型 `shared/knowledge-evidence.ts`；管线测试。
- 测试命令：`npx vitest run tests/knowledge-fast-pipeline.test.ts tests/knowledge-scope-snapshot-compiler.test.ts tests/knowledge-retrieval-golden.test.ts`；`npm run typecheck`；修改文件定向 ESLint；`git diff --check`
- 测试结果：3 文件 / 28 测试通过，0 失败 / 0 跳过（2.98s，exit 0）；类型检查三套、定向 ESLint、diff 检查 exit 0。验证真实 SQLite FTS、字面查询、就绪过滤、重复 ID 去重、零结果、过期准入和取消。日志 `/tmp/lingxi-knowledge-p003-tests.log`、`/tmp/lingxi-knowledge-p003-typecheck.log`。证据加工以必填阶段接口连接，具体提取/打包分别按 P0-04/P0-05 实现，生产提交切换保留在 P0-06；本项不冒充已完成生产端到端验证。
- 对应 commit SHA：`5e6871898689357ef716b535e28fcf03b8e6a398`
- 偏差：none

## P0-04：实现精确 span 提取

- 状态：completed
- 改动文件：`lib/knowledge/evidence-span-extractor.ts`、`lib/knowledge/knowledge-store.ts`、`lib/knowledge/knowledge-query-service.ts`、精确证据测试
- 测试命令：`npx vitest run tests/knowledge-evidence-span-extractor.test.ts tests/knowledge-fast-pipeline.test.ts tests/knowledge-scope-snapshot-compiler.test.ts`；`npm run typecheck`；修改文件定向 ESLint；`git diff --check`
- 测试结果：最终 3 文件 / 38 测试全部通过（1.31s，exit 0）；类型检查三套、ESLint、diff 检查 exit 0。首轮 35 通过 / 3 失败，测试建样误让通用文本摄入重新解析已指定的段落/页码；修正为真实索引入口后因 fixture 策略绑定不同出现 28 通过 / 10 失败，已使用仓库真实配置解析器建绑，保留全部原断言。日志 `/tmp/lingxi-knowledge-p004-tests-final.log`、`/tmp/lingxi-knowledge-p004-typecheck.log`。
- 对应 commit SHA：`53d02eb500a2c9246909f5f330f36808594c1578`
- 偏差：none

## P0-05：实现统一 EvidencePacker

- 状态：completed
- 改动文件：`lib/knowledge/evidence-packer.ts`、`lib/knowledge/knowledge-context-injector.ts`、打包与真实管线测试
- 测试命令：`npx vitest run tests/knowledge-evidence-packer.test.ts tests/knowledge-evidence-span-extractor.test.ts tests/knowledge-evidence-manifest.test.ts tests/knowledge-context-injector.test.ts`；`npm run typecheck`；定向 ESLint；`git diff --check`
- 测试结果：首轮 102 通过 / 1 失败，fixture 清单读回误用 turnScopeId（既有 API 要求 scopeId），类型检查报告同项；修正后 4 文件 / 103 测试全部通过（3.63s，exit 0），包含真实摄入到精确清单落库读回；类型检查复跑三套 exit 0；定向 ESLint、diff 检查 exit 0。日志 `/tmp/lingxi-knowledge-p005-tests-rerun.log`、`/tmp/lingxi-knowledge-p005-typecheck-rerun.log`。
- 对应 commit SHA：`416bfb0d425eb6bf40604ac97045d90d255f6ab2`
- 偏差：none

## P0-06：接入会话提交链路

- 状态：completed
- 改动文件：`core/engine.ts`、`core/desktop-session-submit.ts`、`lib/knowledge/knowledge-manager.ts`、既有提交测试、任务书规定的路由与零远程测试
- 测试命令：`npx vitest run tests/desktop-session-submit-knowledge-routing.test.ts tests/knowledge-fast-zero-remote.test.ts tests/desktop-session-submit.test.ts tests/knowledge-evidence-manifest.test.ts`；`npm run typecheck`；定向 ESLint；`git diff --check`
- 测试结果：4 文件 / 75 测试全部通过（3.33s，exit 0）；类型检查三套 exit 0；ESLint exit 0（0 error / 204 warning），diff 检查 exit 0。真实引擎门面贯通本地检索到清单持久化，六类远程入口设为抛错且均未调用；普通发送和追加消息取消后不提取/打包、不生成/投影，忙态回收。首两轮各 74 通过 / 1 失败，原因分别为原型测试宿主缺少提醒协调器替身、替身未执行消息接受钩子；修复测试装配，保留原断言。首次类型检查因新测试替身函数签名不符 exit 2，修正后复跑通过。日志 `/tmp/lingxi-knowledge-p006-tests-final.log`、`/tmp/lingxi-knowledge-p006-typecheck-final.log`。
- 对应 commit SHA：`9f32627de89bbf441c054bc26dbab155bdae946c`
- 偏差：none

## P0-07：补充观测和前端文案

- 状态：completed
- 改动文件：任务书指定共享统计契约、消息消费、检索折叠组件、引用条、五语言；为实际过程卡贯通同步调整提交事件、服务端广播、工具文案登记与工具行；相应组件、广播和文案对账测试。
- 测试命令：9 个组件/消息消费/广播/提交回归测试文件（见 `/tmp/lingxi-knowledge-p007-tests.log`）；`npm run typecheck`；修改文件定向 ESLint；`git diff --check`
- 测试结果：9 文件 / 151 测试全部通过，0 失败 / 0 跳过（3.44s，exit 0）；类型检查三套、ESLint、diff 检查 exit 0。覆盖快速/详细标签、仅本地检索与生成卡、证据条数/耗时、超时标记、旧统计兼容、缺失耗时不补零、五语言完整。组件运行有 4 次 jsdom scrollTo 未实现提示，未形成失败。
- 对应 commit SHA：本次本地过程展示提交，提交后立即回填
- 偏差：none

## P0-08：性能基准

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P1-01：为索引增加查询元数据

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P1-02：建立 KnowledgeSearchService

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P1-03：查询嵌入分组与缓存

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P1-04：全局融合和分组 rerank

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P1-05：新增 HNSW 向量后端

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P1-06：新增第一等 `knowledge_search` 工具

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P1-07：让现有工具复用统一数据面

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P1-08：HNSW 打包与性能验证

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P2-01：增加 Research 共享契约

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P2-02：Knowledge 数据库升级到 v18

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P2-03：实现 Evidence Ledger

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P2-04：新增 Research 专用工具

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P2-05：增加 Knowledge Research Surface

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P2-06：实现 KnowledgeResearchOrchestrator

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P2-07：详细模式正式切换到 Research Agent

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P2-08：Research 过程 UI

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P3-01：完整性策略选择

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P3-02：Knowledge 数据库升级到 v19

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P3-03：实现 Completeness Executor

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P3-04：建立 source / section / span 多粒度索引

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P3-05：实现分层检索

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P3-06：完整性与详细回答质量门禁

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none

## P3-07：清理旧架构和最终收口

- 状态：pending
- 改动文件：尚未开始
- 测试命令：按任务书该项测试执行，尚未执行
- 测试结果：NOT_EXECUTED
- 对应 commit SHA：尚未提交
- 偏差：none
