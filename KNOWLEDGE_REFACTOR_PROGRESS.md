# 知识库检索与 Research Agent 重构进度

唯一任务来源：`LingxiAgent 知识库检索与 Research Agent 重构执行任务书.md`。
任务书 SHA-256：`803bc56323026b2d8f55de1f490988da4a9646079a1126760b801d6a74cb6e02`。
固定基线：`3eab85891a1747c64064252804f70c0a3773f021`。
执行分支：`feat/knowledge-retrieval-research-p0-p3`。

## 执行规则与断点

- 严格按 P0 → P1 → P2 → P3 及编号顺序；阶段门禁全部通过才进入下一阶段。
- 每项记录测试原始结果后提交；不删除、跳过或放宽测试，不合并 main。
- 现有任务书为用户未跟踪文件，既有规划文档与 BLOCKED.md 历史记录保留。
- 当前断点：P1-03 已完成分组、缓存和本项验证，随本次提交落地，接着执行 P1-04。P0 源码提交 `5c016df183ad207cf1ca33de274abb7a4eb10057`，阶段审计提交 `f9928d76`；全量 13002 PASS / 0 FAIL / 7 既有 SKIP。用户已授权每阶段验证后同步审计记录，保留最终封印。
- 进度、计划与事实集中在本文件和基线文档，避免覆盖既有 task_plan.md / findings.md / PROGRESS.md。
- 目标工具已确认本任务存在 active goal；重复 create_goal 被拒绝，沿用现有目标。
- 规划恢复脚本返回其他会话的无关配置记录，经 git diff 为空核对，未采用其内容。

## 阶段门禁

| 阶段 | 状态 | 结果 |
| --- | --- | --- |
| P0 | completed | 2026-09-04 全量 13002 PASS / 0 FAIL / 7 既有 SKIP，76.42s；全部 P0 门禁通过，审计提交 f9928d76 |
| P1 | in_progress | P1-01 至 P1-03 本项验证通过；阶段门禁尚未执行 |
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
- 对应 commit SHA：`480ef08b8ece625c69a3ba5754bbbee2997fe994`
- 偏差：none

## P0-08：性能基准

- 状态：completed（用户授权后全量复验与阶段审计通过）
- 改动文件：任务书规定的 benchmark、性能契约测试、手工 Linux 工作流；生成器输入 export-manifest 追加 6 个新增公共模块，生成 CLI 闭包与持久化指纹。
- 门禁修复：真实 Node 直接启动测试发现新增三个类的构造参数属性不受 strip-only 支持，改为同义字段赋值，未改变行为；全量 ESLint 发现基线零宽字符集合触发 no-misleading-character-class，将连续的三个码位改写为等价范围，既有零宽混淆测试通过，没有放宽规则。
- 性能测试：`LINGXI_ENFORCE_KNOWLEDGE_PERF=1 node scripts/benchmark-knowledge-fast.mjs --output=/tmp/lingxi-knowledge-p008-production-benchmark.json`，exit 0；真实管理器、两库、冻结范围编译、FTS、原文回读与打包全部经过生产代码。固定种子包含中文、英文、数字、文件名、标题、低频名词及干扰块；冷启动包含新建管理器、打开库和首问，不清操作系统页缓存。新增 `--million` 仅供手工百万块运行，本次未执行。
- 性能结果：10k 热缓存 P95 1.624334ms / 冷启动 P95 24.081542ms；100k 热缓存 P95 17.814625ms / 冷启动 P95 220.717334ms；远程调用 0，最多 8 条证据、892 tokens。每规模 20 次热缓存 + 5 次冷启动。此处仅为 macOS arm64 本机证据，Linux 手工工作流尚未运行。
- 基准前期失败如实保留：直接 Node 导入最初遇不支持的参数属性；临时编译探索遇依赖解析问题、建样遗漏原文产物身份，后来全部移除临时编译和内存资料替身，换为真实管理器链路。最初约 1ms 的索引/加工窄范围数据已撤出验收，仅以上完整链路数据有效。真实建样首轮遗漏 await 后修复；最终性能契约通过。
- P0 首轮门禁：类型检查 exit 0；指定 8 文件 / 63 测试通过（11.45s）；全量 12975 通过 / 27 失败 / 7 既有跳过（77.44s），失败为上述 Node 启动问题、未重钉的生成物、封印门禁及一次既有 OTA 清理竞态。没有跳过或弱化测试。
- 修复复测：8 文件 / 170 测试通过（15.84s），包含真实服务启动、持久化指纹、性能契约、零宽混淆、OTA 原用例；全量最终 1280 文件通过 / 1 文件失败 / 1 既有跳过，13001 测试通过 / 1 失败 / 7 既有跳过（81.90s，exit 1）。唯一失败：`post-verification-audit-seal` 拒绝基线之后的正常开发提交，已载入 BLOCKED.md。
- 最终类型检查三套 exit 0；全量 ESLint exit 0（0 error / 9160 warning）；边界检查 exit 0（保留原有 1 条已登记债务，无新增）。开放服务构建 exit 0；客户端构建 exit 0。正式服务首次因未设置签名输入 exit 1，按仓库支持的一次性测试签名流程复跑 exit 0；11 个 Mach-O 签名、Node 启动 smoke、server/renderer seed 打包通过。测试私钥已删除，仓库发布公钥未变。该结果只证明本机工作区构建，不是正式发布签名或跨平台验证。
- 生成物：五个规定生成器成功执行两轮，第二轮全部 exit 0；测试清单内容哈希不变，暂存首轮预期变更后第二轮 `git diff --exit-code` exit 0。导出目标 `/tmp/lingxi-p0-open-export-20260903` 共 857 文件，未清理用户既有导出目录。持久化指纹 `sha256:9d2a2751a11ad11c01fa759f8ca4ff65527f71166f64d849d13534cc3ed0a7ee`，compatible 理由由生成器记录，表结构和 DATA_EPOCH 不变。
- 证据日志：`/tmp/lingxi-knowledge-p0-final-{typecheck,lint,full}.log`、`/tmp/lingxi-knowledge-p0-gate-boundary-final.log`、`/tmp/lingxi-knowledge-p0-failure-recheck.log`、`/tmp/lingxi-knowledge-p0-build-{server,server-signed,server-open,client}.log`、`/tmp/lingxi-knowledge-p0-generator-second-*.log`、`/tmp/lingxi-knowledge-p0-test-inventory.json`。
- 对应 commit SHA：`5c016df183ad207cf1ca33de274abb7a4eb10057`；审计提交 `f9928d76`。复验 `npm test` 13002 PASS / 0 FAIL / 7 既有 SKIP，76.42s，exit 0；日志 `/tmp/lingxi-knowledge-p0-seal-full-20260904.log`。
- 偏差：无范围/技术方案变更。2026-09-04 用户授权每阶段验证后同步审计记录，并保留最终封印提交；不放宽审计白名单。P1/P2/P3 保持未开始，P0 全绿才进入 P1。

## P1-01：为索引增加查询元数据

- 状态：completed
- 改动文件：索引库、摄入与查询服务、管理器、冻结范围编译器；新增任务书三份测试及共享建样辅助；旧迁移断言版本从 2 更新为 3（迁移数据断言全部保留），生成持久化指纹。
- 实现：索引库 v2→v3 严格增加规定表及索引，v1 顺序迁移；摄入在块/FTS 同事务完成目录、块数与时间写入；启动后每批最多 20 个缺失目录变体，游标继续、失败留痕、关闭取消；查询优先读取目录，缺失只做 SQL 计数与 warning，损坏显式错误，不扫描原文恢复。
- 测试命令：`npx vitest run tests/knowledge-index-metadata-migration.test.ts tests/knowledge-index-metadata-backfill.test.ts tests/knowledge-scope-metadata-query.test.ts tests/knowledge-scope-snapshot-compiler.test.ts tests/knowledge-index-variants.test.ts tests/knowledge-fast-zero-remote.test.ts tests/knowledge-fast-pipeline.test.ts`；`npm run typecheck`；修改文件 ESLint；持久化指纹生成与检查。
- 结果：7 文件 / 46 测试全部通过（3.55s）；三套类型检查 exit 0；ESLint exit 0（0 error / 16 warning）；指纹生成与检查 exit 0，`sha256:51dbbd0b965d66c42e9a99d5b7d55d8327f5199c7af0c39c66e923e16aa55a2d`。真实摄入、25 变体分批补齐/幂等、查询零全量读取/零回填、损坏错误、迁移原行/FTS 保留、原子回滚均通过。
- 修复记录：首轮 11 FAIL 为新样本误用重解析前产物与检索参数名错误，修复后 1 FAIL 为样本重复摄入未携带相同分块配置，显式固定配置后全部通过；未删除、跳过或放宽断言。
- 日志：`/tmp/lingxi-knowledge-p101-tests-final.log`、`/tmp/lingxi-knowledge-p101-typecheck-r2.log`、`/tmp/lingxi-knowledge-p101-lint.log`、`/tmp/lingxi-knowledge-p101-fingerprint.log`。
- 对应 commit SHA：`d0789cf2`。
- 偏差：none

## P1-02：建立 KnowledgeSearchService

- 状态：completed
- 改动：新增任务书锁定的搜索请求/命中/响应契约与统一服务；两个缓存文件先定义键契约，缓存实现及接线严格放到 P1-03。快速管线与详细会话自动检索共用入口；底层现有混合检索通过冻结身份执行核复用，分组去重及全局重排按 P1-03/P1-04 接续，未提前改动算法。
- 范围：先校验宿主落库范围、会话/工作室、冻结来源身份及过滤条件；查询不能扩大范围。本地检索固定 FTS、禁止重排，命中块只做定点定位读取；混合检索按真实远程调用计数，保留两路命中标识及向量变体身份，模型不可用显式留痕。候选摘要不冒充证据。无会话范围的既有兼容调用暂保留，后续详细切换按 P2-07 执行。
- 测试命令：`npx vitest run tests/knowledge-search-service.test.ts tests/knowledge-rerank-fusion.test.ts tests/knowledge-fast-zero-remote.test.ts tests/knowledge-fast-pipeline.test.ts tests/knowledge-fast-performance-contract.test.ts tests/knowledge-retrieval-golden.test.ts tests/knowledge-context-injector.test.ts tests/knowledge-evidence-manifest.test.ts`；`npm run typecheck`；修改文件 ESLint；`npm run lint:boundary`。
- 结果：8 文件 / 126 测试通过（7.95s，exit 0）；三套类型检查 exit 0；ESLint 0 error / 144 warning，exit 0；边界检查 exit 0。真实快速/详细会话经过统一服务、非法范围/关闭/取消拒绝、真实混合查询、原文证据清单回归通过。
- 生成物：导出输入追加三文件，CLI 闭包生成成功（10661 文件，原有 1 条边界债务不变）；持久化指纹 compatible 重钉并检查通过，`sha256:52773eadca7429d5c84f9c91c208c2e48f24dc31b3348018953e07de4afa3b18`；表结构与 DATA_EPOCH 不变。
- 日志：`/tmp/lingxi-knowledge-p102-tests-final.log`、`/tmp/lingxi-knowledge-p102-typecheck-final.log`、`/tmp/lingxi-knowledge-p102-lint.log`、`/tmp/lingxi-knowledge-p102-boundary.log`、`/tmp/lingxi-knowledge-p102-{closure,fingerprint}.log`。
- 对应 commit SHA：`dac524c4`。
- 偏差：none

## P1-03：查询嵌入分组与缓存

- 状态：completed
- 改动：查询嵌入缓存固定 512 条、10 分钟，检索结果缓存固定 256 条、2 分钟；均采用 LRU、并发同键一次执行、失败不缓存、各等待者独立取消，返回副本防调用方污染缓存。结果缓存命中前仍校验冻结范围，返回前复核范围仍处于活动状态。
- 接线：按配置的供应商/模型/配置修订分组，同组全部向量变体一次搜索；查询嵌入键固定 query 用途。配置修订由模型配置和凭证的内存摘要识别，不保存或输出原文；模型变化只使对应嵌入条目失效，结果缓存同步清除；管理器关闭清理缓存。P1-04 的全局融合及按重排引用分组尚未执行，本项保留各笔记本重排并复用抽出的原有期限与响应校验。
- 测试命令：`npx vitest run tests/knowledge-query-embedding-cache.test.ts tests/knowledge-retrieval-result-cache.test.ts tests/knowledge-search-model-grouping.test.ts tests/knowledge-search-service.test.ts tests/knowledge-rerank-fusion.test.ts tests/knowledge-fast-zero-remote.test.ts tests/knowledge-fast-pipeline.test.ts tests/knowledge-engine-persistence.test.ts tests/knowledge-fast-performance-contract.test.ts`；`npm run typecheck`；修改文件 ESLint；`npm run lint:boundary`。
- 结果：9 文件 / 55 测试通过（8.05s，exit 0）；三套类型检查 exit 0；ESLint exit 0（0 error / 131 warning）；边界检查 exit 0。5 笔记本/5 独立来源使用 1 种模型时实际嵌入与向量搜索各 1 次、使用 2 种时各 2 次；并发相同请求只有 1 次底层执行，独立取消不误伤另一请求。
- 修复记录：类型检查发现新增摘要计算误引用浏览器全局 crypto，显式导入 Node createHash 后修复；测试统计向量参数为 unknown，按该实测数组接口补齐类型后通过。未修改或放宽断言。
- 生成物：CLI 闭包生成成功（10663 文件，原 1 条边界债务不变）；持久化指纹 compatible 重钉并检查通过，`sha256:4fcf36fe237df434f30d142fe0a4d5d1660bb6e4aafb71ddf7fa936fd5522b53`；表结构与 DATA_EPOCH 不变。
- 日志：`/tmp/lingxi-knowledge-p103-tests-final.log`、`/tmp/lingxi-knowledge-p103-typecheck-r3.log`、`/tmp/lingxi-knowledge-p103-lint-final.log`、`/tmp/lingxi-knowledge-p103-boundary.log`、`/tmp/lingxi-knowledge-p103-{closure,fingerprint}.log`。
- 对应 commit SHA：本次提交，下项回填。
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
