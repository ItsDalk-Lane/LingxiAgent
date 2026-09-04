# 知识库检索与 Research Agent 重构进度

唯一任务来源：`LingxiAgent 知识库检索与 Research Agent 重构执行任务书.md`。
任务书 SHA-256：`803bc56323026b2d8f55de1f490988da4a9646079a1126760b801d6a74cb6e02`。
固定基线：`3eab85891a1747c64064252804f70c0a3773f021`。
执行分支：`feat/knowledge-retrieval-research-p0-p3`。

## 执行规则与断点

- 严格按 P0 → P1 → P2 → P3 及编号顺序；阶段门禁全部通过才进入下一阶段。
- 每项记录测试原始结果后提交；不删除、跳过或放宽测试，不合并 main。
- 现有任务书为用户未跟踪文件，既有规划文档与 BLOCKED.md 历史记录保留。
- 当前断点：P1-01 至 P1-08 已完成，阶段收口 `23873985`、审计 `333c5112`，本地与四平台门禁全部通过；P2-01 已完成并提交 `c3033b05`；P2-02 已提交 `c729f68a`；P2-03 已提交 `4a95317c`；P2-04 已提交 `359aeb77`；P2-05 已提交 `faad0da2`；P2-06 已提交 `f4340d98`；P2-07 已提交 `06cc6179`；P2-08 已提交 `725cf4a0`；P2 阶段故障修复已提交 `d4292b2d`，修复后全部阶段门禁通过：全量 13434 PASS / 0 FAIL / 7 既有 SKIP；阶段收口 `56dc1086`、审计 `4fefe66e` 已完成并推送；P3-01 已提交 `d1781134`；P3-02 单项验证完成，本项提交后按序进入 P3-03。P0 源码提交 `5c016df183ad207cf1ca33de274abb7a4eb10057`，阶段审计提交 `f9928d76`；全量 13002 PASS / 0 FAIL / 7 既有 SKIP。用户已授权每阶段验证后同步审计记录，保留最终封印。
- 进度、计划与事实集中在本文件和基线文档，避免覆盖既有 task_plan.md / findings.md / PROGRESS.md。
- 目标工具已确认本任务存在 active goal；重复 create_goal 被拒绝，沿用现有目标。
- 规划恢复脚本返回其他会话的无关配置记录，经 git diff 为空核对，未采用其内容。

## 阶段门禁

| 阶段 | 状态 | 结果 |
| --- | --- | --- |
| P0 | completed | 2026-09-04 全量 13002 PASS / 0 FAIL / 7 既有 SKIP，76.42s；全部 P0 门禁通过，审计提交 f9928d76 |
| P1 | completed | P1-01 至 P1-08 完成；本地全量和第三轮四平台 Build 33829055797 全部通过，阶段审计 333c5112 |
| P2 | completed | P2-01 至 P2-08 及阶段故障修复全部完成；全量 13434 PASS / 0 FAIL / 7 既有 SKIP，构建/包内烟测/两轮生成器均通过 |
| P3 | in_progress | P3-01、P3-02 单项通过；阶段门禁尚未执行 |

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
- 改动文件：`KNOWLEDGE_REFACTOR_PROGRESS.md`、`build/cli-runtime-closure.json`、`build/persistence-schema-fingerprint.json`、`core/engine.ts`、`export-manifest.json`、`lib/knowledge/fast-knowledge-pipeline.ts`、`lib/knowledge/knowledge-index-store.ts`、`lib/knowledge/knowledge-manager.ts`、`lib/knowledge/knowledge-query-service.ts`、`lib/knowledge/knowledge-search-service.ts`、`lib/knowledge/query-embedding-cache.ts`、`lib/knowledge/retrieval-result-cache.ts`、`tests/knowledge-search-service.test.ts`。
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
- 改动文件：`KNOWLEDGE_REFACTOR_PROGRESS.md`、`build/cli-runtime-closure.json`、`build/persistence-schema-fingerprint.json`、`core/engine.ts`、`lib/knowledge/knowledge-manager.ts`、`lib/knowledge/knowledge-query-service.ts`、`lib/knowledge/knowledge-search-service.ts`、`lib/knowledge/query-embedding-cache.ts`、`lib/knowledge/retrieval-result-cache.ts`、`tests/knowledge-query-embedding-cache.test.ts`、`tests/knowledge-retrieval-result-cache.test.ts`、`tests/knowledge-search-model-grouping.test.ts`、`tests/knowledge-search-service.test.ts`。
- 改动：查询嵌入缓存固定 512 条、10 分钟，检索结果缓存固定 256 条、2 分钟；均采用 LRU、并发同键一次执行、失败不缓存、各等待者独立取消，返回副本防调用方污染缓存。结果缓存命中前仍校验冻结范围，返回前复核范围仍处于活动状态。
- 接线：按配置的供应商/模型/配置修订分组，同组全部向量变体一次搜索；查询嵌入键固定 query 用途。配置修订由模型配置和凭证的内存摘要识别，不保存或输出原文；模型变化只使对应嵌入条目失效，结果缓存同步清除；管理器关闭清理缓存。P1-04 的全局融合及按重排引用分组尚未执行，本项保留各笔记本重排并复用抽出的原有期限与响应校验。
- 测试命令：`npx vitest run tests/knowledge-query-embedding-cache.test.ts tests/knowledge-retrieval-result-cache.test.ts tests/knowledge-search-model-grouping.test.ts tests/knowledge-search-service.test.ts tests/knowledge-rerank-fusion.test.ts tests/knowledge-fast-zero-remote.test.ts tests/knowledge-fast-pipeline.test.ts tests/knowledge-engine-persistence.test.ts tests/knowledge-fast-performance-contract.test.ts`；`npm run typecheck`；修改文件 ESLint；`npm run lint:boundary`。
- 结果：9 文件 / 55 测试通过（8.05s，exit 0）；三套类型检查 exit 0；ESLint exit 0（0 error / 131 warning）；边界检查 exit 0。5 笔记本/5 独立来源使用 1 种模型时实际嵌入与向量搜索各 1 次、使用 2 种时各 2 次；并发相同请求只有 1 次底层执行，独立取消不误伤另一请求。
- 修复记录：类型检查发现新增摘要计算误引用浏览器全局 crypto，显式导入 Node createHash 后修复；测试统计向量参数为 unknown，按该实测数组接口补齐类型后通过。未修改或放宽断言。
- 生成物：CLI 闭包生成成功（10663 文件，原 1 条边界债务不变）；持久化指纹 compatible 重钉并检查通过，`sha256:4fcf36fe237df434f30d142fe0a4d5d1660bb6e4aafb71ddf7fa936fd5522b53`；表结构与 DATA_EPOCH 不变。
- 日志：`/tmp/lingxi-knowledge-p103-tests-final.log`、`/tmp/lingxi-knowledge-p103-typecheck-r3.log`、`/tmp/lingxi-knowledge-p103-lint-final.log`、`/tmp/lingxi-knowledge-p103-boundary.log`、`/tmp/lingxi-knowledge-p103-{closure,fingerprint}.log`。
- 对应 commit SHA：`42520c06`。
- 偏差：none

## P1-04：全局融合和分组 rerank

- 状态：completed
- 改动文件：`KNOWLEDGE_REFACTOR_PROGRESS.md`、`build/persistence-schema-fingerprint.json`、`lib/knowledge/fast-knowledge-pipeline.ts`、`lib/knowledge/knowledge-context-injector.ts`、`lib/knowledge/knowledge-manager.ts`、`lib/knowledge/knowledge-query-service.ts`、`lib/knowledge/knowledge-search-service.ts`、`shared/knowledge-refs.ts`、`tests/helpers/knowledge-rerank-fixture.ts`、`tests/knowledge-cross-notebook-fusion.test.ts`、`tests/knowledge-global-rerank.test.ts`、`tests/knowledge-mixed-rerank-groups.test.ts`、`tests/knowledge-search-model-grouping.test.ts`、`tests/knowledge-search-service.test.ts`。
- 改动：统一入口先对全范围一次 FTS，再按嵌入组只搜向量；全部序列使用现有 k=60 名次融合并去重，共享资料不因多挂笔记本重复加分。重排按不同供应商/模型引用分组，每组最多一次，输入前 50 条，尾部保留；多个模型结果只比较名次，不比较原始分数。任一重排组失败则保留整个全局融合顺序并明确留痕，取消信号照常抛出。
- 统计：增加 embeddingGroups、rerankGroups、queryEmbeddingCacheHit、retrievalResultCacheHit，服务→注入→会话统计贯通；结果缓存命中时本次模型组数/远程调用数为 0。快速路径缓存命中时 FTS 实际执行数为 0，但检索模式与已有结果仍如实保留。
- 测试命令：`npx vitest run tests/knowledge-global-rerank.test.ts tests/knowledge-mixed-rerank-groups.test.ts tests/knowledge-cross-notebook-fusion.test.ts tests/knowledge-search-model-grouping.test.ts tests/knowledge-search-service.test.ts tests/knowledge-rerank-fusion.test.ts tests/knowledge-fast-zero-remote.test.ts tests/knowledge-fast-pipeline.test.ts tests/knowledge-context-injector.test.ts tests/knowledge-evidence-manifest.test.ts tests/knowledge-fast-performance-contract.test.ts`；`npm run typecheck`；修改文件 ESLint；`npm run lint:boundary`。
- 结果：11 文件 / 138 测试通过（8.30s，exit 0）；三套类型检查 exit 0；ESLint exit 0（0 error / 6 warning）；边界检查 exit 0。覆盖同引用五本只重排一次、50 条输入封顶/尾部十条保留、多个引用各一次、跨模型分数变百万倍仍同序、无引用零调用、网络/非法响应/空结果/固定期限超时恢复原序与留痕、缓存统计透传。
- 生成物：持久化指纹 compatible 重钉及检查通过，`sha256:a4fa19b26cc549cbbb08b7acd435292db9354fc9d80e715a9c9d641d5a467a91`；无新增运行模块，CLI 文件闭包不变；表结构与 DATA_EPOCH 不变。
- 日志：`/tmp/lingxi-knowledge-p104-tests-final.log`、`/tmp/lingxi-knowledge-p104-typecheck-final.log`、`/tmp/lingxi-knowledge-p104-lint-final.log`、`/tmp/lingxi-knowledge-p104-boundary.log`、`/tmp/lingxi-knowledge-p104-fingerprint-final.log`。
- 对应 commit SHA：`c1863fdc`。
- 偏差：none

## P1-05：新增 HNSW 向量后端

- 状态：completed
- 改动文件：`KNOWLEDGE_REFACTOR_PROGRESS.md`、`build/cli-runtime-closure.json`、`build/persistence-schema-fingerprint.json`、`build/persistence-store-inventory.json`、`export-manifest.json`、`lib/knowledge/ann-index-store.ts`、`lib/knowledge/knowledge-context-injector.ts`、`lib/knowledge/knowledge-manager.ts`、`lib/knowledge/knowledge-query-service.ts`、`lib/knowledge/knowledge-search-service.ts`、`lib/knowledge/usearch-vector-backend.ts`、`lib/knowledge/vector-index-adapter.ts`、`lib/knowledge/vector-search-backend-factory.ts`、`lib/knowledge/vector-search-backend.ts`、`package-lock.json`、`package.json`、`shared/knowledge-refs.ts`、`shared/persistence/store-registry.ts`、`tests/helpers/knowledge-ann-fixture.ts`、`tests/knowledge-ann-crash-recovery.test.ts`、`tests/knowledge-ann-index-store.test.ts`、`tests/knowledge-ann-rebuild.test.ts`、`tests/knowledge-usearch-backend.test.ts`、`tests/knowledge-vector-backend-fallback.test.ts`、`tests/knowledge-vector-backend-parity.test.ts`。
- 改动：optionalDependencies 精确锁定 usearch 2.26.0；新增任务书指定四个后端模块、ANN 独立 v1 目录及每变体文件。原始向量库仍为 v3，BLOB 不变；ready 后异步启动独立工作线程，每批 512 行建图，临时文件 fsync → 原子改名 → 最后目录 ready。重启清理本模块临时文件，恢复中断构建；失效、替换和删除同步清掉旧内存索引。
- 查询：参数固定 cosine/f32/16/128/64；key=ordinal+1；各变体候选合并后按余弦分数稳定排序；加载缓存最多 32 个，估算达到 512MB 淘汰最旧引用（不是进程 RSS 的硬上限）。统一查询只读取目录与命中块；原生依赖、文件缺失/损坏、指纹/数量及查询失败均显式 portable exact 回退并安排重建。每请求独立记录实际后端和原因，服务→证据加工→会话统计透传。
- 测试命令：任务书六个 ANN 测试文件，加 knowledge-search-model-grouping / knowledge-search-service / knowledge-vector-index / knowledge-retrieval-golden / knowledge-context-injector / knowledge-fast-zero-remote / knowledge-engine-persistence；另跑 persistence-store-registry / persistence-schema-tripwire；三套类型检查、修改文件 ESLint、边界检查、持久化扫描/指纹生成检查、CLI 闭包生成。
- 结果：13 文件 / 135 测试通过（8.24s，exit 0）；持久化 2 文件 / 29 测试通过（5.44s，exit 0）；类型检查 exit 0；ESLint 0 error / 35 warning，exit 0；边界检查 exit 0。真实原生查询、本项确定性 top-k overlap ≥95%、既有知识 golden set、真实统一入口五组来源 100% 召回、零全量块/BLOB 热读取、两种 LRU 边界、异步构建/重启恢复及逐字节保留 BLOB 全部通过。
- 修复记录：首轮损坏文件触发原生读取器进程异常退出（5 文件/8 测试通过，1 unhandled error，不能视为通过）；依据锁定版本源码补齐加载前尺寸、文件头、图层、键及邻居边界校验，保留原测试并新增深层结构损坏样本，重跑全部通过。初次持久化扫描发现新增目录库 database-open 未登记，补齐具体模块所有权后通过；未改门禁或放宽断言。
- 生成物：66 个持久化存储/779 个写入点登记；CLI 闭包 10667 文件，原 1 条边界债务不变；兼容指纹 `sha256:8e8d4219d971582c186cdb036538eaf5bc398007ace614de290e4802e7da0077`。本项只确认本机原生扩展，四平台原生打包/运行与 100k 性能门禁按 P1-08 执行，当前不得标成已通过。
- 日志：`/tmp/lingxi-knowledge-p105-tests-{first,second,regression,final}.log`、`/tmp/lingxi-knowledge-p105-persistence-tests.log`、`/tmp/lingxi-knowledge-p105-typecheck-final.log`、`/tmp/lingxi-knowledge-p105-lint-final.log`、`/tmp/lingxi-knowledge-p105-{boundary,inventory-r2,closure,fingerprint,fingerprint-check}.log`。
- 对应 commit SHA：`4c6189e4`。
- 偏差：none

## P1-06：新增第一等 knowledge_search 工具

- 状态：completed
- 改动文件：`KNOWLEDGE_REFACTOR_PROGRESS.md`、`build/cli-runtime-closure.json`、`build/persistence-schema-fingerprint.json`、`core/agent.ts`、`desktop/src/locales/en.json`、`desktop/src/locales/ja.json`、`desktop/src/locales/ko.json`、`desktop/src/locales/zh-TW.json`、`desktop/src/locales/zh.json`、`desktop/src/react/utils/tool-label.ts`、`export-manifest.json`、`lib/tools/knowledge-search-tool.ts`、`shared/tool-categories.ts`、`tests/helpers/knowledge-search-tool-fixture.ts`、`tests/knowledge-search-tool-output-budget.test.ts`、`tests/knowledge-search-tool-scope.test.ts`、`tests/knowledge-search-tool.test.ts`、`tests/tool-label-coverage.test.ts`。
- 改动：新增任务书固定参数的只读搜索工具；query 1～4000 字符、limit 默认 12/上限 24、channel 默认 hybrid。运行时取得 studioId；拒绝模型传入归属参数；复用 knowledge-scope 校验会话和父会话，过滤条件全部经统一搜索服务复核。结果只有候选与定位摘要，明确要求必须经过 knowledge_read/knowledge_grep 才能引用，candidateId 不是证据 ID。
- 接线：加入 Agent 共用工具快照和 STANDARD 分类（后续 P2 研究会话从该工具面选取只读集合）；复用既有同名合成检索卡的三相位文案，调整名单归类并补齐五语言知识库失败表述，不重复注册工具名。FTS 禁止重排，取消信号沿统一服务传递。
- 测试命令：任务书三个 search-tool 文件，加 tool-label-coverage / knowledge-agent-tools / tool-categories / tool-categorization-smoke / agent-tools-conditional-injection / subagent-tool-policy；三套类型检查、修改文件 ESLint、边界检查、CLI 闭包及持久化指纹生成检查。
- 结果：9 文件 / 92 测试通过（2.17s，exit 0）；类型检查 exit 0；ESLint 0 error / 88 warning，exit 0；边界检查 exit 0。未知/关闭/跨工作室/跨会话范围拒绝、真实父范围继承、混合越权过滤整单拒绝、模型归属伪造拒绝、各权限档只读放行、长问题和非法数量拒绝、默认 12/最大 24 候选及既有 1200 字符摘要上限全部通过。
- 生成物：CLI 闭包 10668 文件，原 1 条边界债务不变；持久化指纹保持 `sha256:8e8d4219d971582c186cdb036538eaf5bc398007ace614de290e4802e7da0077`，检查通过；没有数据表变化。
- 日志：`/tmp/lingxi-knowledge-p106-tests-{first,final}.log`、`/tmp/lingxi-knowledge-p106-typecheck.log`、`/tmp/lingxi-knowledge-p106-lint.log`、`/tmp/lingxi-knowledge-p106-{boundary,closure,fingerprint,fingerprint-check}.log`。
- 对应 commit SHA：`08bfd20d`。
- 偏差：none

## P1-07：现有工具复用统一数据面

- 状态：completed
- 改动文件：`KNOWLEDGE_REFACTOR_PROGRESS.md`、`build/cli-runtime-closure.json`、`build/persistence-schema-fingerprint.json`、`core/agent.ts`、`lib/knowledge/knowledge-store.ts`、`lib/tools/knowledge-grep-tool.ts`、`lib/tools/knowledge-outline-tool.ts`、`lib/tools/knowledge-read-tool.ts`、`tests/knowledge-agent-tools.test.ts`、`tests/knowledge-read-tool.test.ts`。
- 改动：knowledge_read.query 使用编译范围和统一搜索服务，固定 sourceId/所属 notebook、limit=12；原 ordinal 范围读取保持原行为。knowledge_outline 使用编译目录和索引 metadata，返回 chunk count、章节、标题、状态与可信度；取消普通目录中的 CoverageUnit 重算和相应覆盖数量输出，保留事实库只读数量/定位类型查询。章节列表沿用已有 40 项摘要上限并显式标记截断。
- 原文匹配：增加 scannedChars、matchedSourceCount、精确起止偏移和长匹配截断标记；保留原文空白，宿主 details 携带冻结身份与实际返回原文位置，作为 P2 receipt 扩展点。本项不提前建 P2 表或生成 receipt。扫描预算只计真正扫描过的字符，中断来源已扫描的计数和结果继续保留。
- 测试更新：按任务书更换目录的旧覆盖单位断言，新增实际数据库 chunk 数精确断言，以及 CoverageUnit/全量 blocks/全量 chunks 均零调用断言；原来源归属、标题、可信度、父会话继承与越权拒绝断言保留。读取查询新增统一入口调用参数断言，旧 retrieveForArtifacts 必须零调用；原文匹配与宿主读取位置逐字符对照。
- 测试命令：knowledge-read-tool / knowledge-agent-tools / knowledge-search-tool / knowledge-search-tool-scope / knowledge-scope-metadata-query / knowledge-scope-snapshot-compiler / knowledge-store；另跑 knowledge-source-processors；三套类型检查、修改文件 ESLint、边界检查、CLI 闭包和持久化指纹生成检查。
- 结果：7 文件 / 70 测试通过（2.20s，exit 0）；处理器 1 文件 / 6 测试通过（806ms，exit 0）；类型检查 exit 0；ESLint 0 error / 164 warning，exit 0；边界检查 exit 0。
- 修复记录：首轮 2 FAIL / 33 PASS：旧工具样本只建立块索引而未登记笔记本分块身份，统一目录如实判未就绪。样本补齐摄入侧已有的身份登记后通过，未在查询侧增加写入或惰性建索引，未放宽命中/范围断言。一次扩展测试命令误写不存在的处理器文件名（Vitest 只执行其余 7 文件）；已用真实 knowledge-source-processors 文件单独执行并记录。
- 生成物：CLI 闭包 10667 文件，普通目录不再引入覆盖单位构建模块，原 1 条边界债务不变；持久化指纹兼容更新并检查通过，`sha256:62fb7022d119927f45f160a0ebe7bdffc673bcb1dfefd3be23cf31e1d729b40d`，表版本不变。
- 日志：`/tmp/lingxi-knowledge-p107-tests-{first,r2,final}.log`、`/tmp/lingxi-knowledge-p107-processors.log`、`/tmp/lingxi-knowledge-p107-typecheck-final.log`、`/tmp/lingxi-knowledge-p107-lint.log`、`/tmp/lingxi-knowledge-p107-{boundary,closure,fingerprint,fingerprint-check}.log`。
- 对应 commit SHA：`d3235253`。
- 偏差：none

## P1-08：HNSW 打包与性能验证

- 状态：completed（本地及第三轮四平台验证全部通过，P2 尚未开始）
- 测试命令：`npm run typecheck`、`npm run lint`、`npm run lint:boundary`、任务书 P1 门禁指定的八文件 `npx vitest run`、`npm test`、`npm run build:server`、`npm run build:server:open`、`npm run build:client`、`npm run test:knowledge-platform-smoke`、`node scripts/smoke-packaged-knowledge.mjs`；另执行种子与 standalone 验证、`node scripts/benchmark-knowledge-vector.mjs`、`node scripts/benchmark-knowledge-fast.mjs`，以及下文逐轮记录的修复定向测试和五生成器两轮。Linux 性能运行设置 `LINGXI_ENFORCE_KNOWLEDGE_PERF=1`。
- 改动文件（含同项修复、生成物与阶段审计）：`.github/workflows/build.yml`、`.github/workflows/knowledge-performance.yml`、`.sync-audit/build-sync-matrix.mjs`、`.sync-audit/upstream-sync-matrix.json`、`.sync-audit/verified-source-sha.txt`、`KNOWLEDGE_REFACTOR_PROGRESS.md`、`PROGRESS.md`、`UPSTREAM_SYNC_AUDIT.md`、`UPSTREAM_SYNC_MATRIX.md`、`artifacts/knowledge-fast-benchmark-linux-x64-9bee41dc.json`、`artifacts/knowledge-fast-benchmark-linux-x64-f86da543.json`、`artifacts/knowledge-vector-benchmark-linux-x64-9bee41dc.json`、`artifacts/knowledge-vector-benchmark-linux-x64-f86da543.json`、`artifacts/knowledge-vector-benchmark.json`、`build/cli-runtime-closure.json`、`build/persistence-schema-fingerprint.json`、`core/engine.ts`、`lib/knowledge/ingestion-service.ts`、`lib/knowledge/usearch-vector-backend.ts`、`package.json`、`scripts/benchmark-knowledge-vector.mjs`、`scripts/build-server-artifact.mjs`、`scripts/build-server-deps.mjs`、`scripts/build-server-phases.mjs`、`scripts/build-server-runtime-assets.mjs`、`scripts/build-standalone-server-artifact.mjs`、`scripts/compute-cli-closure.mjs`、`scripts/export-open-tree.mjs`、`scripts/prepare-usearch-native.mjs`、`scripts/smoke-packaged-knowledge.mjs`、`scripts/verify-seed-kit.mjs`、`scripts/verify-standalone-server-artifact.mjs`、`tests/build-server-artifact.test.ts`、`tests/build-server-runtime-assets.test.ts`、`tests/build-standalone-server-artifact.test.ts`、`tests/helpers/knowledge-vector-package-fixture.ts`、`tests/knowledge-ann-rebuild.test.ts`、`tests/knowledge-embedding-provider-gate.test.ts`、`tests/knowledge-native-build.test.ts`、`tests/smoke-packaged-knowledge.test.ts`、`tests/verify-seed-kit.test.ts`。
- 改动：正式/开放服务共用安装流程纳入可选原生包，发布包强制安装锁定版本及加载依赖；按目标保留原生扩展，Electron 配置显式解包原生文件，standalone 与种子验证检查对应文件。独立包内构建入口来自真实生产后端；子进程真实建图/查询，移走解包副本全部原生扩展后以原数据库回退，最后恢复文件。完整包内烟测还验证缺扩展时服务器重启读回快照。
- 性能：固定种子 10k/100k，64 维（16 维合成投影），40 次独立查询、3 次新后端冷加载；真实 portable 数据库与 HNSW，无远程模型。100k exact P95 152.127166ms，HNSW P95 3.726334ms，40.8249 倍，top-10 overlap 99.75%；建图 13194.276459ms，文件 40466728 字节，冷加载 P95 108.440333ms。10k exact P95 12.914583ms、HNSW 0.55425ms、重合率 100%。本机 macOS arm64，墙钟 gate 未启用，不能代表真实供应商嵌入召回率或其他平台。
- 首轮验证记录：P1 指定 8 文件 / 44 PASS；打包与导出 8 文件 / 123 PASS；平台烟测本机 8 文件 / 94 PASS；类型检查三套 PASS，lint 0 error / 9176 warning，boundary PASS。首次全量 1298 文件 PASS / 2 FAIL / 1 既有 SKIP，13079 测试 PASS / 2 FAIL / 7 既有 SKIP，86.65s，exit 1。
- 首轮问题：服务器不存在时新检查遮盖原错误，恢复原检查顺序；真实原生余弦舍入按 2 个机器精度单位校验；降级原因包含 variant 后缀，按完整原因核对。全量发现可选扩展误列静态必需依赖，改为显式安装后台线程运行包，原质量门禁不变并复测通过；另一全量失败是待同步的审计坐标。
- 首轮生成物：CLI 闭包 10672 文件，开放树增加共享构建工具依赖；持久化指纹保持 `sha256:62fb7022d119927f45f160a0ebe7bdffc673bcb1dfefd3be23cf31e1d729b40d`。性能 JSON 在 `artifacts/knowledge-vector-benchmark.json`。五个生成器连续两轮通过：第二轮派生文件零差异、测试清单内容完全相同，开放树 866 文件。
- 首轮本地构建：正式服务、开放服务、客户端均 exit 0；正式服务采用一次性本地测试签名，12 个 Mach-O 签名与种子复核通过，私钥随后删除，未改发布公钥。完整包内烟测 exit 0，含移除扩展后服务器启动、检索和快照读回。第二次 lint 曾与闭包生成器重叠，扫入生成器临时文件而失败；生成器退出后原命令复跑 0 error / 9176 warning、boundary 通过，未新增忽略项。
- 远程：独立性能工作流尚未在默认分支登记（查询返回 404）；保留该手工流程，并把同一性能脚本接入现有 Build 的固定 Linux runner。四平台 Build 的包内与 standalone 烟测继续沿用既有门禁，分支运行不触发标签发布。
- 日志：`/tmp/lingxi-knowledge-p108-*`。最终全量复验、四平台及稳定 runner 结果见下文第三轮记录。
- 对应 commit SHA：首轮源码 `9bee41dc`、审计 `c742a423`；首轮跨平台修复源码 `c452a705`、审计 `f0e52055`；第二轮修复源码 `f86da543`、审计 `8295e5ff`。
- 偏差：none

### P1-08 跨平台首轮与修复（仍属当前任务）

- 远程 Build：`33825262170`，验证 `c742a4236f8b293bab0aad36a55bfff585f03f5c`；质量门禁、macOS arm64 全构建、Linux x64 全构建与包内烟测 PASS，Windows x64 与 macOS x64 FAIL，未进入 P2。失败后未改变工作流测试或平台列表。
- 稳定 Linux runner：100k HNSW P95 5.82537ms、exact P95 410.159589ms、70.4092 倍、top-10 overlap 99.75%，墙钟 gate 启用并通过。快速模式 100k 热 P95 60.241723ms、冷 P95 354.354467ms。报告下载至 `/tmp/lingxi-knowledge-p1-linux-benchmarks/`，适用首轮已注明提交。
- Windows 失败：原导入测试在立即关闭后删除目录时 EPERM。ANN 恢复读取提前发起、派生库句柄延后关闭。修复为恢复入口可取消、关闭同步释放派生库句柄、重复关闭等待同一次退出；引擎等待后台真正退出。原导入断言与清理不变，新增立即关闭/在途构建退出回归。
- Intel 失败：原生烟测在创建索引时 SIGSEGV。本机 x64 Node/Rosetta 同样复现；`nm -arch x86_64 -u` 发现发行包缺失 `_nk_*` 链接符号，arm64 切片无此问题。用锁定 2.26.0 源码让主扩展与计算库按同一 x64 架构编译，保留原 arm64 切片后重新组装签名；同一修复产物通过 arm64/x64 两次真实创建/添加/查询，重复准备返回 ready。仅调整构建架构，不改 C++ 算法、版本、HNSW 参数或文件格式。
- 安装/打包接线：安装时对已知缺链接的 Intel 扩展进行修复，坏扩展先隔离；修复不可用时明确报告 portable 回退，正式打包则失败。完整包内原生烟测保持必过。构建工具进入开放导出清单。
- 修复测试：8 文件 / 57 PASS；之前关闭专项 6 文件 / 21 PASS。类型检查发现新增测试对文件名重载推断不完整，按字符串处理后复跑；无测试删除、跳过、超时扩张。
- 指纹兼容重钉 `sha256:03658759d1b349c11b6ae7e7daba2ce169488a052e97b65b5a424c9cb7fd2d9e`；所有表与 DATA_EPOCH 不变。修复后两轮生成器均通过且零漂移；闭包仍 10672 文件，开放树 867 文件。类型检查、全量 lint（0 error / 9176 warning）、boundary、本机三种构建与包内正常/回退烟测通过。修复后的全量测试及第二、三轮远程结果见下文。


### P1-08 跨平台第二轮与修复（仍属当前任务）

- 首轮修复后本地全量：1301 文件 PASS / 1 既有 SKIP，13085 测试 PASS / 7 既有 SKIP / 0 FAIL，80.13s，exit 0。
- 远程 Build `33826852985`（`f0e520552ae597230db7652bea7b9930c6053f7f`）整体 FAIL：macOS arm64、macOS x64、Linux x64 全构建 PASS；Windows 原先导入清理问题已通过，随后在包内原生检索断言 FAIL；质量门禁另有原限流测试 FAIL。未进入 P2。
- Linux 稳定 runner 第二轮性能：100k HNSW P95 6.768842ms、exact 444.014943ms、65.59688 倍、top-10 overlap 99.75%；10k HNSW 1.354042ms、exact 47.243127ms、重合率 100%。墙钟门禁保持启用且通过。报告保留 `/tmp/lingxi-knowledge-p1-linux-benchmarks-r2/`。
- 质量门禁：原 `knowledge-lifecycle` 最小间隔测试配置 80ms、断言至少 70ms，实测 16ms。远程 13074 PASS / 1 FAIL / 17 既有 SKIP，466.48s。原限流器先记录放行时刻、等待 Promise 续段后才实际调用；主线程延迟时请求会挤在一起。新增确定性回归在旧代码记录间隔 0ms、明确失败；修复为同步派发调用后用单调时钟计时，并保留并发上限、关闭拒绝语义。原测试与超时值不变。
- 回归修复过程：首次采用模块导入的单调时钟，测试虚拟时钟未控制它，新增测试原 60s 超时（1 FAIL / 46 PASS）；改用 Node 全局单调时钟后原命令 4 文件 / 47 PASS，4.87s，未放宽测试。最终连同 ANN 构建与安装回归 6 文件 / 53 PASS，4.84s；P1 指定 8 文件 / 44 PASS，1.82s。
- Windows 包内失败：期望 hnsw，实际 portable。保存线程刷盘前使用只读句柄，而 Windows 刷盘要求可写权限（微软 FlushFileBuffers 文档）；改用 r+ 保留已有内容，fsync 与原生后端断言均保留，失败断言增加后端和降级原因便于定位。此修复仍需下一轮真实 Windows runner 确认，不把本机测试当作 Windows PASS。
- 类型检查三套、lint（0 error / 9176 warning）、boundary 已通过；兼容指纹重钉 `sha256:3a430fbfb39226c044f96d3723f9c014eab8c01d94805af1a2c2d5f0c4266c46`，数据表、版本、paid vectors 不变。正式服务、开放服务、桌面端三种构建与种子验签、包内原生/移除扩展回退烟测均 exit 0；本机平台烟测 8 文件 / 94 PASS，7.62s。五生成器连续两轮通过、第二轮派生物零漂移，闭包 10672 文件、开放树 867 文件、测试清单一致。全量复验及第三轮远程状态见下文。
- 日志：`/tmp/lingxi-knowledge-p1-second-quality.log`、`/tmp/lingxi-knowledge-p1-second-windows.log`、`/tmp/lingxi-knowledge-p108-dispatch-*`、`/tmp/lingxi-knowledge-p108-third-*`。


### P1-08 第三轮验证（全部通过）

- 本机全量复验 `npm test` exit 0：1302 文件 PASS / 1 既有 SKIP，13087 测试 PASS / 7 既有 SKIP / 0 FAIL，78.60s。源码 `f86da54313e35a5868c6f045c9495717d61ba1bb`，纯审计提交 `8295e5ff937cf9d3e49c082231188a01bd56122b`。
- 第三轮远程 Build `33829055797` 在同一审计提交上运行，最终整体 SUCCESS：前置检查、界面构建、质量门禁、四平台完整构建与产物启动/历史升级回归全部 PASS。一次状态观察因网络 EOF 结束，随后查询确认远程原任务继续运行，没有重复触发构建。
- 本轮两项修复另经只读复核：并发占位、实际调用后的间隔、同步异常释放、停机排队拒绝、刷盘后原子落盘、包内原生与移除扩展回退断言均保留；未发现新增阻断问题。此复核不替代四平台门禁。
- 本轮 Linux 稳定 runner 性能门禁 PASS（墙钟限制启用）：100k HNSW P95 6.041646ms、exact P95 416.031149ms、68.86056 倍、top-10 overlap 99.75%；建图 17452.363762ms、文件 40448776 字节、冷加载 P95 251.593555ms。快速模式 100k 热 P95 68.219216ms、冷 P95 391.315210ms、远程模型调用 0。原始生成报告保留在 `artifacts/knowledge-{fast,vector}-benchmark-linux-x64-f86da543.json`，来源 Build `33829055797`。
- 第三轮远程质量门禁 PASS：1302 文件 PASS / 1 既有 SKIP，13077 测试 PASS / 17 既有 SKIP / 0 FAIL，410.55s。原生命周期 16 测试与新增请求派发 2 测试均 PASS；类型检查、lint、包构建一并通过。日志 `/tmp/lingxi-knowledge-p1-third-quality.log`。Linux 的既有平台跳过数单独保留，不与 macOS 的 7 项混写。
- 三个平台已取得终态与日志实证：Windows x64、Linux x64、macOS arm64 全构建 PASS；各自平台测试 8 文件 / 94 PASS，包内输出均为 native=hnsw、removed-native=portable，签名归档解包、安装、解析、重启与不可变快照读回通过；Windows standalone 压缩包还独立再次通过原生/回退检索。安装包已上传，Intel 完整构建与既有产物启动/历史升级回归继续等待。
- P1 六项收口核查：查询模型分组对应 `knowledge-search-model-grouping`（1/2 模型调用数与五来源命中）；全局重排对应 `knowledge-global-rerank`（五来源一次重排）；已选范围内跨笔记本工具对应 `knowledge-search-tool` 与 `knowledge-search-tool-scope`；优先原生对应 `knowledge-usearch-backend`；失败保原向量回退对应 `knowledge-vector-backend-fallback` 与本轮真实移除扩展烟测；快速档无向量路径对应 `knowledge-search-service` 与 `knowledge-fast-zero-remote`。均在本轮全量套件内通过，生产入口只读核查无缺口。
- Intel macOS x64 完整构建也已 PASS：平台测试 8 文件 / 94 PASS，包内日志明确 native=hnsw、removed-native=portable，签名归档解包、安装、解析、重启和快照读回通过；`/tmp/lingxi-knowledge-p1-third-mac-intel.log`。四套 installer 产物均已上传且未过期，清单与摘要保留 `/tmp/lingxi-knowledge-p1-third-artifact-inventory.json`；当前只剩工作流的产物启动/历史升级回归。
- 最后门禁：产物启动与历史升级回归 8 文件 / 304 PASS，1.88s；日志 `/tmp/lingxi-knowledge-p1-third-release-smoke.log`。分支运行中的发布、镜像和发布列车按既有标签条件未触发，没有合并 main。
- 阶段末五生成器再连续两轮全部 exit 0；第二轮完整 `git diff --exit-code` 为 0，测试清单逐字节一致，日志 `/tmp/lingxi-knowledge-p1-close-generator*`。此前测试与参数均保留，所有失败均已修复并复验。
- 阶段收口提交 `2387398589ec5494e1adb28b014dc84ebcf15a64`，阶段审计 `333c5112`；收口后的本地全量再次 13087 PASS / 7 既有 SKIP / 0 FAIL，80.46s，exit 0。源实现与四平台运行证据对应上述三个源码/审计提交对。P1 全部门禁通过，后续按序进入 P2。

## P2-01：增加 Research 共享契约

- 状态：completed
- 改动文件：`shared/knowledge-research.ts`、`export-manifest.json`、`KNOWLEDGE_REFACTOR_PROGRESS.md`
- 改动：按任务书逐项增加运行状态、证据需求类型/状态、证据关系、需求对象和预算契约；固定预算严格为 4 轮、4 并发、32 工具调用、180 秒、每轮 8 搜索/12 阅读、32 最终证据、16000 token。
- 测试命令：三套 `npm run typecheck`；`npx vitest run tests/knowledge-execution-policy.test.ts tests/chat-route-knowledge-refs.test.ts`；新增文件 ESLint；开放导出登记按现有规则核查。
- 测试结果：三套类型检查 exit 0；2 文件 / 16 PASS，1.30s，exit 0；新增文件 ESLint exit 0，开放边界通过（保留原 1 条债务）。任务书全部字段与八个预算值经只读逐项核对一致；开放导出 868 文件，新共享契约与源文件逐字节一致。没有数据库或运行时闭包变化。日志 `/tmp/lingxi-knowledge-p201-{typecheck,tests,lint,boundary,export}.log`。
- 对应 commit SHA：`c3033b05e09877bf425b3fd0e5ea9cf9b065c8da`
- 偏差：none

## P2-02：Knowledge 数据库升级到 v18

- 状态：completed
- 改动文件：`lib/knowledge/knowledge-store.ts`、`lib/knowledge/types.ts`、`shared/persistence/store-registry.ts`、`tests/knowledge-store-v18-migration.test.ts`、`tests/knowledge-research-store.test.ts`、`tests/fixtures/knowledge-store-v17.sql`、`tests/knowledge-chunk-profiles.test.ts`、`tests/persistence-schema-tripwire.test.ts`、`build/persistence-schema-fingerprint.json`、`build/persistence-store-inventory.json`、`KNOWLEDGE_REFACTOR_PROGRESS.md`
- 改动：按固定七表及列顺序新增 v18 建表，沿用既有外层迁移事务统一提交版本。添加状态/关系/完整性策略、整数/布尔/偏移/页码、摘要格式、JSON 形状、非空身份、外键及唯一约束；阅读凭据只保存位置和 hash。对应持久化记录类型复用 P2-01 契约，台账读写与证据宿主核验按后续 P2-03 实施，不提前合并任务。
- 测试命令：任务书指定 `knowledge-store-v18-migration`、`knowledge-research-store`、`persistence-schema-tripwire`，既有知识存储与生命周期回归；三套类型检查、修改文件 ESLint、持久化指纹生成检查及开放边界/闭包。
- 已验证：真实 v17 fixture 来自 `c3033b05` 存储类，与该提交源码逐字节确认；18 张旧表、19 行合成数据，导出后独立恢复全表逐行一致。迁移首轮旧代码 3 FAIL / 2 PASS，v18 后 5 PASS，235ms，实际执行前三张新表后触发 SQLite DDL 错误，证实版本与全部旧表/数据回滚并可正常重试。数据库约束旧代码 16 FAIL；v18 首轮 15 PASS / 1 FAIL，发现摘要可附 NUL 隐藏尾巴；增加字符/字节双长度检查后扩展 17 PASS，398ms。未修改或放宽失败用例。
- 生成物与整体复验：原 8 文件首轮 85 PASS / 4 FAIL（3.99s），四败均为指纹测试读到未同步的旧持久化清单；先按生成器更新清单，再重新生成指纹，并将现有精确版本断言更新到 18、增加七表断言。原 8 文件复跑 89 PASS / 0 FAIL / 0 SKIP，4.11s，exit 0；三套类型检查 exit 0；修改文件 ESLint 0 error / 76 既有 warning，新增版本测试单独 ESLint 0；开放边界保留原 1 条债务，闭包 10672 文件，开放导出 868 文件，均 exit 0。兼容迁移指纹 `sha256:cd0feeaaa9caa4800620293e2f2a5a3b52d1f22d3eec6d21efccc0d2b036066b`。日志 `/tmp/lingxi-knowledge-p202-{tests,tests-final,typecheck-final,lint,tripwire-lint,inventory,fingerprint-final,closure,boundary,export}.log`。未删、跳过或放宽测试。
- 对应 commit SHA：`c729f68aecced285138e98377748a3b77a6926e7`
- 偏差：none

## P2-03：实现 Evidence Ledger

- 状态：completed
- 改动文件：`lib/knowledge/research/evidence-ledger.ts`、`lib/knowledge/research/research-store.ts`、`lib/knowledge/research/research-stop-policy.ts`、`lib/knowledge/evidence-receipt-service.ts`、`lib/tools/knowledge-read-tool.ts`、`lib/tools/knowledge-grep-tool.ts`、`tests/knowledge-evidence-ledger.test.ts`、`tests/knowledge-evidence-quote-validation.test.ts`、`tests/knowledge-read-receipts.test.ts`、`tests/knowledge-research-store.test.ts`、`tests/helpers/knowledge-research-fixture.ts`、`export-manifest.json`、`build/cli-runtime-closure.json`、本进度文件。
- 改动：实际阅读/扫描返回冻结原文时发位置与摘要凭据；入账复核身份链、摘要和精确引文，重复文字必须指定零基次数，最多 2000 字符。取证、关联、凭据消费和需求重算同事务；同一来源不同段落不增加独立来源数。支持、矛盾、不适用、反证检查与完整性均由宿主核算；显式新需求解释保留原反证；完整性执行器未提供证明时不能冒充完成。动作只保留白名单元数据；终态研究拒绝后续写入但可读历史；停止策略优先执行真实硬预算。普通工具调用保持兼容，生产研究会话接线按后续 P2-05 实施。
- 测试命令：`npx vitest run tests/knowledge-evidence-ledger.test.ts tests/knowledge-evidence-quote-validation.test.ts tests/knowledge-read-receipts.test.ts tests/knowledge-research-store.test.ts tests/knowledge-read-tool.test.ts tests/knowledge-agent-tools.test.ts tests/persistence-schema-tripwire.test.ts`；三套 `npm run typecheck`；修改文件 ESLint；闭包、持久化清单/指纹和开放导出生成器；开放边界检查。
- 测试结果：最终 7 文件 / 106 PASS / 0 FAIL / 0 SKIP，2.54s，exit 0。三套类型检查及最终测试类型检查 exit 0；ESLint 0 error，保留原工具 9 条 warning，新增测试的 4 处 any 已改为精确参数类型后复验零 warning。开放边界保留原 1 条债务，闭包 10675 文件、开放导出 872 文件均通过；v18 指纹仍为 `sha256:cd0feeaaa9caa4800620293e2f2a5a3b52d1f22d3eec6d21efccc0d2b036066b`，没有数据库结构改动。
- 失败与修复：最初 Ledger/quote 联测 18 PASS / 1 FAIL 是测试误假定随机 ID 的关系排序等于插入顺序，改为同一确定排序后仍逐项核对全部来源；只读复核发现带检索错误的零命中会被误算反证完成，增加真实数据库用例得到 10 PASS / 1 FAIL，再修生产逻辑排除错误/失败/未完成动作。预算触顶优先部分停止以及四种研究终态拒绝全部后续写入均有新增验证。没有删除、跳过或放宽测试。
- 证据：`/tmp/lingxi-knowledge-p203-{quote-first,ledger-first,ledger-counter-red,tests-first,tests-final,typecheck,test-typecheck-final,lint,test-lint-final,closure,inventory,fingerprint-final,boundary,export}.log`；新 research 目录被既有通用忽略规则命中，提交时仅精确纳入任务书规定的三个文件，未改忽略规则。
- 对应 commit SHA：`4a95317c31484368ffbed948139142b7b106359d`
- 偏差：none

## P2-04：新增 Research 专用工具

- 状态：completed
- 改动文件：`lib/tools/knowledge-research-update-tool.ts`、`lib/tools/knowledge-research-finish-tool.ts`、`lib/tools/knowledge-delegate-tool.ts`、`lib/knowledge/research/research-tool-budget.ts`、`lib/knowledge/research/research-store.ts`、`tests/knowledge-research-update-tool.test.ts`、`tests/knowledge-research-finish-tool.test.ts`、`tests/knowledge-delegate-tool.test.ts`、`tests/knowledge-research-tool-budget.test.ts`、`export-manifest.json`、本进度文件。
- 改动：三工具逐字段遵守任务书，更新只允许最多 8 个需求与限定长度、凭据精确入账、整批回滚、完整性只升级；结束重新核验宿主状态和真实预算，结论摘要不入库/不回显；委派每次最多 4 个任务，先校验完整批次再并行启动、等待全部清理结束，只报告结构化结果，Worker 不得委派或结束。共享预算累计 Root/Worker 的全部已授权调用和绝对创建时限；第 32 次之后研究部分结束并拒绝新调用，搜索/阅读另限每轮 8/12。所有实例共享并发名额，动作只保留有限元数据；实际隔离执行器的研究 surface 接线按下一项 P2-05 实施。
- 测试命令：`npx vitest run tests/knowledge-research-tool-budget.test.ts tests/knowledge-research-update-tool.test.ts tests/knowledge-research-finish-tool.test.ts tests/knowledge-delegate-tool.test.ts tests/knowledge-research-store.test.ts tests/knowledge-evidence-ledger.test.ts tests/knowledge-read-receipts.test.ts tests/knowledge-evidence-quote-validation.test.ts tests/persistence-schema-tripwire.test.ts`；三套类型检查；全部修改代码 ESLint；持久化清单/指纹、开放导出与闭包生成器；开放边界检查。
- 测试结果：最终 9 文件 / 125 PASS / 0 FAIL / 0 SKIP，2.12s，exit 0；三套类型检查 exit 0，定向 ESLint 0 error / 0 warning。开放边界仍为原 1 条债务，开放导出 876 文件；持久化指纹保持 P2-02 的 v18 值不变。闭包生成 10675 文件，与前项相同，exit 0。
- 失败与修复：结束工具最早因预算文件尚未完成而导入失败（未执行用例），文件落盘后使用真实预算器验证；更新工具首轮 11 PASS / 1 FAIL，补查同步事务取消导致终态一起回滚，修复为事务回滚后再次落实取消并保留原断言；共享名额故障用例先 8 PASS / 1 FAIL，修复委派计数写库失败时也必须释放名额。测试类型检查发现 TypeBox 断言访问类型不符，改为结构化类型后全部通过；委派顶层权限/状态等未知字段新增明确拒绝验证。无测试删除、跳过或放宽。
- 证据：`/tmp/lingxi-knowledge-p204-{budget-first,budget-slots-red,tests-first,tests-second,tests-final,typecheck,lint,inventory,fingerprint,boundary,export,closure}.log`。对应子项细证 `/tmp/lingxi-knowledge-p204-finish-tests-final.log`、`/tmp/lingxi-p204-delegate-top-level.log`。
- 对应 commit SHA：`359aeb77bbe8cbeaddd4499c19f50d9e3412d867`
- 偏差：none

## P2-05：增加 Knowledge Research Surface

- 状态：completed
- 改动文件：`core/agent.ts`、`core/session-coordinator.ts`、新增 `core/session-manifest/knowledge-ancestry.ts`，四个只读知识工具及 `knowledge-scope.ts`、权限包装器、工具分类；新增五个研究/祖先测试及真实 Agent 工具快照联调测试，原知识工具 fixture 仅适配范围拥有者字段；出口清单、运行闭包与持久化指纹生成物。
- 改动：Root 七工具、Worker 五工具，宿主入口强制只读、拒绝审批、无记忆与扩展、无工作区写范围；按真实清单追溯主会话，最多八层，循环/缺失/跨工作室拒绝。研究绑定在装配和每次调用时复核；Worker 目录/搜索/读取都限定分配来源，所有调用共用预算。主研究使用父 Agent 的聊天模型，委派使用当前或显式活跃 Agent 的聊天模型；工作会话真实父级为 Root。执行结束及初始化失败都先等待资源清理，再删除临时会话和作废清单；删除或清单写入失败明确抛出。`core/agent-manager.ts` 复用原有活跃 Agent 与隔离执行回调，无需无行为差异的改动。
- 测试命令：任务书四项测试，加 `knowledge-research-agent-tools`、`session-permission-wrapper`、`session-coordinator-isolated-abort`、`session-teardown`、原有 read/agent-tools/search-scope/receipt/conditional-injection 与 `persistence-schema-tripwire` 共十四文件；`npm run typecheck`；定向 ESLint；`git diff --check`；持久化清单/指纹、运行闭包、开放边界与导出生成器。
- 测试结果：最终 **14 文件 / 192 PASS / 0 FAIL / 0 SKIP，3.15s，exit 0**；三套类型检查、ESLint、diff、开放边界全部 exit 0。定向 ESLint 有 643 项警告，包含现有大文件及测试替身的宽类型警告，未当作零警告报告。先后保留并修复来源目录总数泄漏、更新/结束工具祖先归属漏检；完整回归第一次 175 PASS / 2 FAIL，正好复现初始化清理失败被吞掉，两项故障注入测试保留并修复后全部通过。最终日志 `/tmp/lingxi-knowledge-p205-tests-final.log`、`/tmp/lingxi-knowledge-p205-typecheck-final.log`、`/tmp/lingxi-knowledge-p205-lint.log`。
- 生成物：持久化清单 66 stores / 779 sites；兼容指纹 `sha256:3beb2e79d626d4bc9d7ae2f35a92c1b4a6daab3bccf3e7d701b565138cc43679`，仅登记研究隔离生命周期改变，知识库仍为 v18；运行闭包 10682 文件；开放树成功导出 877 文件。阶段审计按用户授权在 P2 全部任务和门禁完成后同步，最终封印仍保留。
- 对应 commit SHA：`faad0da2`
- 偏差：none

## P2-06：实现 KnowledgeResearchOrchestrator

- 状态：completed
- 改动文件：任务书锁定的 `knowledge-research-orchestrator.ts`、`research-prompts.ts`、`research-round-runner.ts`、`research-context-renderer.ts` 四模块；研究存储/共享预算、`core/agent.ts` 的查询计划与错误保真；指定六项测试、材料渲染/恢复/真实 Agent 工具联调及夹具；出口清单、运行闭包生成物。
- 改动：首轮列目录并建立 1～8 个需求，没有需求时按完整原问题补建并继续第二轮；每轮只按宿主账本中的未解决项、冲突和反证缺口调查，多维 fixture 实际启动 Worker 并通过共享台账交回原文。搜索计划由宿主关联需求/反证用途，查询按文字与实际来源、章节范围判等；参数错误保留稳定错误码，未知错误与模型原文不入动作记录。根会话、工作会话和工具共用 32 次/4 轮/180 秒绝对预算，取消与额度耗尽都等待全部清理。最终包重新核冻结原文及已消费凭据，最多 32 段/16000 token，保留矛盾、缺口和截断提示；模型普通回复不决定完整性、不进入下轮和最终上下文。崩溃恢复沿用原 run/round/预算，取消旧在途动作、不读取旧会话推理；合成阶段崩溃直接完成原研究收口，正文损坏明确进入失败终态。
- 测试命令：任务书六项测试，加 `knowledge-research-recovery`、`knowledge-research-context-renderer`、`knowledge-research-agent-tools`、原有 update/finish/delegate/budget/store/receipt/ledger 与 `persistence-schema-tripwire`，共十七文件；`npm run typecheck`；修改文件 ESLint、`git diff --check`；持久化清单/指纹、运行闭包、边界与开放树生成器。
- 测试结果：最终 **17 文件 / 194 PASS / 0 FAIL / 0 SKIP，4.78s，exit 0**；三套类型检查、静态检查、差异与开放边界检查均 exit 0。ESLint 为 0 错误/89 警告，包含大文件及测试替身宽类型警告。真实存储、索引、冻结范围、Agent 工具与 Worker 委派贯通，模型执行边界使用确定性替身。首轮六文件 37 PASS；随后独立调查测试 1 FAIL/1 PASS、恢复测试 3 FAIL/1 PASS、合成恢复新增项 1 FAIL/4 PASS、错误保真 2 FAIL/15 PASS、超长问题 1 FAIL/6 PASS，均保留原断言并修复后纳入最终绿测；没有删除、跳过或放宽测试。日志 `/tmp/lingxi-knowledge-p206-tests-final.log`、`/tmp/lingxi-knowledge-p206-typecheck-verified.log`、`/tmp/lingxi-knowledge-p206-lint-final.log`。
- 生成物：持久化清单 66 stores / 779 sites；v18 与兼容指纹 `sha256:3beb2e79d626d4bc9d7ae2f35a92c1b4a6daab3bccf3e7d701b565138cc43679` 保持不变；运行闭包 10682 文件，依赖图已更新；开放树成功导出 881 文件。P2 阶段完整门禁和审计尚待 P2-07/P2-08 完成，最终封印保留。
- 对应 commit SHA：`f4340d98`
- 偏差：none

## P2-07：详细模式正式切换到 Research Agent

- 状态：completed
- 改动文件：新增 Engine 详细入口、普通发送与追加消息路由；旧算法原文搬至 `lib/knowledge/legacy/legacy-knowledge-context-injector.ts`，原 facade 只保留安全/打包/渲染与清单装配；最终研究渲染与历史压缩；研究搜索身份回调、恢复接线及测试；开放清单和运行闭包。
- 改动：新详细请求经真实隔离执行器调查，只有完成或部分完成后进入主回答；取消等待实际清理、失败不以空材料继续生成。最终上下文逐项列明需求、冲突/缺口及七条回答契约，整块受 16000 token/32 段约束；清单只收已消费凭据验证的精确原文，同分块多引文不丢，扫描凭据按真实冻结索引补齐定位，索引缺失/不完整明确失败。Root/Worker 实查向量身份经宿主回调去重进入最终清单，不改工具公开输出、不保存候选正文。相同轮次重试沿用原范围/预算，已终态直接复用研究；历史检索方式缺失明确标记，不从配置猜测。旧 98 个顶层声明正文保持一致，历史测试只调整导入位置；两种历史材料信封同时压缩和剥离。
- 测试命令：任务书四项测试，加 Engine 原文清单/恢复、真实搜索身份、研究工具/编排/恢复、快速零远程、两项提交回归、旧 injector/coverage/planner/manifest/rerank/golden 与持久化指纹，共二十文件；三套类型检查；全部修改代码 ESLint；差异、持久化清单/指纹、开放边界、开放树与闭包生成。
- 测试结果：最终 **20 文件 / 340 PASS / 0 FAIL / 0 SKIP，8.69s，exit 0**；三套类型检查与 ESLint 均 exit 0，ESLint 0 错误/313 警告；差异与开放边界通过。恢复终态用例先 3 PASS/1 FAIL，修复后 4 PASS；旧 manifest 宿主缺新入口先失败，保留原断言、补真实会话登记后通过；五项研究搜索探针随真实入口迁移机械调整，全部原断言保留。混合搜索清单新用例首次运行已为绿态，未倒改生产制造红测。日志 `/tmp/lingxi-knowledge-p207-tests-final.log`、`/tmp/lingxi-knowledge-p207-typecheck-final.log`、`/tmp/lingxi-knowledge-p207-lint-final.log`，恢复红绿日志 `/tmp/lingxi-knowledge-p207-engine-terminal-recovery-{red,green}.log`。
- 生成物：持久化清单仍 66 stores / 779 sites；v18 与指纹 `sha256:3beb2e79d626d4bc9d7ae2f35a92c1b4a6daab3bccf3e7d701b565138cc43679` 不变；运行闭包 10687 文件，开放树 882 文件。P2 阶段门禁及审计待 P2-08 完成，最终封印保留。
- 对应 commit SHA：`06cc6179`
- 偏差：none

## P2-08：Research 过程 UI

- 状态：completed（单项及 P2 阶段全部门禁通过）
- 改动文件：研究编排、工具、Engine 与聊天广播的七类过程事件；消息消费、现有流缓冲与工具卡、工具标签、最终折叠摘要、五份 locale；七组真实质量资料与行为测试，过程/取消/广播/组件回归；运行闭包。
- 改动：规划、每轮调查、每个 Worker 聚合卡、需求完成进度、核对矛盾和最终整理均从真实宿主状态发送，逐字段白名单无模型推理/工具正文。相同身份只更新原卡；普通发送与追加消息均覆盖，旧主回答结束后按真实卡片身份不可变更新已落消息，不造空回复、不丢在途卡。历史重进只靠持久化 stats 恢复完成/部分摘要，未知数字不补零；研究专用截断文案为“部分证据未纳入”，旧快速/详细历史兼容保留。
- 质量资料：`second-round-clue`、`cross-source-comparison`、`conflicting-sources`、`counterexample`、`timeline`、`no-result`、`scope-escape` 七组固定原文；八项测试通过真实 Engine、两库、冻结资料、原文凭据和 Worker 工具，只有模型执行边界使用确定性驱动。验证至少两次不同查询、每段最终原文均有已消费凭据、摘要和伪造引文不能进清单、冲突/反例保留、实际多 Worker、越界工具拒绝且不读取外源、32 次预算部分完成。
- 测试命令：任务书相关研究/质量/取消/路由/快速回归，加真实前端过程、旧过程、折叠摘要、工具卡、工具文案及指纹，共十八文件；三套类型检查；全量 ESLint、开放边界、差异检查；持久化清单/指纹、运行闭包、开放树生成。
- 测试结果：本项最终 **18 文件 / 224 PASS / 0 FAIL / 0 SKIP，5.50s，exit 0**；三套类型检查 exit 0；全量 ESLint 0 错误/9188 警告，exit 0；开放边界与差异检查通过。前端另含完整缓冲/WS 联测 7 文件/188 PASS；质量 8/8 PASS；组件 26/26 PASS。日志 `/tmp/lingxi-knowledge-p208-tests-final.log`、`/tmp/lingxi-knowledge-p208-typecheck-final.log`、`/tmp/lingxi-knowledge-p2-lint.log`。jsdom 有既有 scrollTo 未实现提示，不记为产品通过证据或失败。
- 失败与修复：进度通知抛错真实复现提前释放并发额度，新增 2 条红测（原 11 条通过）后，统一隔离通知异常并仅记录固定事件类型，回归等待真实工作会话全部清理。取消同时关闭范围时事件曾误报 failed，改按已保存终态报告 cancelled。首轮计划事件实际晚于开轮，界面改为按计划更新完成规划卡。质量用例最初两处 fixture 误判（仅读首分块、宽查询误要求零命中），改用真实查询分块定位并明确所选来源/秘密值约束，未放宽生产安全或质量断言。所有原测试保留，无新增跳过。红测工具输出事后摘录明确标注 `/tmp/lingxi-knowledge-p208-delegate-progress-red.tool-output.txt`，不是伪称现场日志。
- 生成物：持久化清单 66 stores / 779 sites；v18 与指纹 `sha256:3beb2e79d626d4bc9d7ae2f35a92c1b4a6daab3bccf3e7d701b565138cc43679` 不变；运行闭包 10687 文件，开放树 882 文件。阶段完整测试、构建及审计未完，不进入 P3，最终封印保留。
- 对应 commit SHA：`725cf4a0`
- 偏差：none

## P2 阶段门禁修复与复验

- 首轮全量：`npm test` exit 1，1327 文件 PASS / 6 FAIL / 1 既有 SKIP；13415 测试 PASS / 19 FAIL / 7 既有 SKIP，84.79s；日志 `/tmp/lingxi-knowledge-p2-full-before-audit.log`。19 项失败全部保留：17 项真实 Node 源码启动因新增六个类使用参数属性而报 strip-only 不支持；1 项工具全集采样仍只取普通会话而遗漏三研究专属工具；1 项旧 P1 审计坐标，按用户已授权流程待推进。
- 当前修复：六类改成显式字段与构造赋值，接口/预算/存储行为不变；目录门禁采集普通、Root、Worker 三种真实工具快照，分别校验权限与分类，再保留原全集断言，并明确普通会话无研究专属工具、Worker 无委派/结束工具。没有增加豁免或把专用工具暴露到普通会话。
- 修复验证：真实服务启动/鉴权/存储健康与研究回归共 6 文件 / 48 PASS，17.33s；工具目录原 3 项从 2 PASS/1 FAIL 修为 3 PASS。三套类型与定向 ESLint 通过（0 错误/1 既有警告），差异检查通过。日志 `/tmp/lingxi-knowledge-p2-source-startup-fix.log`、`/tmp/lingxi-p2-permission-coverage-{red,green}.log`、`/tmp/lingxi-knowledge-p2-fix-typecheck.log`、`/tmp/lingxi-knowledge-p2-fix-lint.log`。
- 修复前已通过的门禁：指定十文件 83 PASS；本机平台 smoke 八文件 94 PASS；三种构建、种子验签及包内原生 HNSW/移除扩展 portable/安装重启原文读取全部通过。五生成器两轮全部 exit 0，第二轮完整 diff 0、测试清单 937 文件逐字一致、开放树两份各 882 文件逐文件一致。这些证据仅对应 `725cf4a0`，不能替代当前修复后的复验。
- 修复后最终门禁：源码 `d4292b2d0ba5029e7c4b1d1e2969b031f5c7b903` 的三套类型与全量 ESLint 均 exit 0（0 error / 9188 warning），指定十文件加目录门禁共 86 PASS，本机平台 smoke 94 PASS。完整服务端、开放服务端、客户端全部入口构建、种子验签、真实归档解包及包内 HNSW/移除扩展 portable/安装解析重启读回全部 exit 0，临时签名密钥已清除。日志 `/tmp/lingxi-knowledge-p2-fixed-{targeted,platform-smoke,lint,build-server,verify-seed,packaged-smoke,build-open,build-client}.log`。
- 修复后五生成器两轮全部通过，第二轮完整 git diff 0、测试清单逐字一致（937 文件，SHA256 `5287f1ae317f02923cb923cf19ef402811acb6facc42c30783190bcfddbc20b2`）、开放树逐文件一致（各 882 文件）。持久化清单 66 stores / 779 sites、知识库 v18、指纹 3beb2e79…、运行闭包 10687 文件均无漂移，完整清单 `/tmp/lingxi-knowledge-p2-fixed-generator-results.json`。
- 全量审计复验：`npm test` **exit 0；1333 文件 PASS / 1 既有 SKIP，13434 测试 PASS / 0 FAIL / 7 既有 SKIP，86.98s**，日志 `/tmp/lingxi-knowledge-p2-audit-full.log`。未删除、跳过、放宽测试或更改审计白名单。
- 阶段最终行为确认：快速仍是本地全文检索；详细只走实际 Agent 运行时，可多轮搜索/阅读与并行工作会话；最终清单全部经原文凭据核验；详细程度按证据需求覆盖控制；旧一次性详细管线仅历史兼容，不参与新生产请求。当前阶段仅本机平台与包运行结果，不将其冒称新的 Windows/Linux 实机验证。
- 当前状态：P2 全部门禁通过，本收口提交仅记录证据，生产代码/测试/构建生成物与上述全量验证源码不变；同步阶段审计记录后按序进入 P3-01，最终封印提交保留，未合并 main。

## P3-01：完整性策略选择

- 状态：completed
- 改动文件：新增 `lib/knowledge/research/completeness-policy.ts`，接入共享唯一执行策略入口；关键词、正式入口、真实 Engine 和研究更新工具回归；开放清单与运行闭包。
- 改动：任务书全部中文范围词与章节词、对应英文表达确定性选择最低完整性要求；章节专属短语内的“所有”只限定章节，短语外另有全文要求仍按整个范围核查。普通详细与多来源比较保持来源多样性，快速永远尽力检索且 1200ms 不变。既有宿主更新规则保持只能提高，真实工具逐级升级与全部降级组合、Worker 禁改策略均验证；降级失败不污染同批需求、证据、缺口或消费凭据。真实详细入口把全文要求写进研究账本与最终材料，没有范围证明时即使找到事实也只报告部分完成。
- 测试命令：`npx vitest run tests/knowledge-completeness-policy.test.ts tests/knowledge-execution-policy.test.ts tests/knowledge-research-update-tool.test.ts tests/knowledge-detailed-engine-context.test.ts tests/knowledge-research-quality-fixtures.test.ts tests/knowledge-research-stop-policy.test.ts tests/knowledge-fast-zero-remote.test.ts`；`npm run typecheck`；六修改文件 ESLint；Node 24 直接导入正式策略入口；差异、持久化清单/指纹、运行闭包、开放边界及开放树生成。
- 测试结果：最终 **7 文件 / 173 PASS / 0 FAIL / 0 SKIP，4.40s，exit 0**；三套类型、ESLint（0 错误/0 警告）、Node 源码运行、差异与边界检查均 exit 0。首轮 152 PASS/1 FAIL，英文 every relevant section 被泛范围词误升；保留断言，补完整章节短语和量词单复数 12 组合后通过；独立审查又发现 anything is missing / has been omitted 从句遗漏，新增测试真实 97 PASS/7 FAIL，补可选助动词后 104/104 PASS，正式入口与最终联测全部通过。没有删除或放宽断言。红绿日志 `/tmp/lingxi-p301-completeness-omission-{red,green}.log`；最终日志 `/tmp/lingxi-knowledge-p301-tests-reviewed.log`、`/tmp/lingxi-knowledge-p301-typecheck-reviewed.log`、`/tmp/lingxi-knowledge-p301-lint-reviewed.log`、`/tmp/lingxi-knowledge-p301-node-source.log`。
- 生成物：持久化清单仍 66 stores / 779 sites，v18 与指纹 `sha256:3beb2e79d626d4bc9d7ae2f35a92c1b4a6daab3bccf3e7d701b565138cc43679` 不变；运行闭包 10688 文件，开放树 883 文件。P3 阶段门禁和最终封印尚未执行。
- 对应 commit SHA：`d1781134`
- 偏差：none

## P3-02：Knowledge 数据库升级到 v19

- 状态：completed
- 改动文件：`knowledge-store.ts` 的 v19 建表与顺序迁移、持久化登记说明；真实 v18 数据快照、v19 迁移和四表存储测试，旧版本断言与指纹门禁；持久化清单/指纹生成物。
- 改动：严格新增任务书四表，字段/默认值/主键保持锁定；研究运行唯一检查，单元/证据与覆盖运行使用复合关联，外键保留既有原文身份。约束五种单元状态、非负整数计数、偏移、覆盖率、exact 布尔及完整计数条件；检查总状态未被任务书锁定，因此不另创受限枚举。建表与版本号仍在同一迁移事务中，旧表/旧行不改写；本项只建立存储结构，执行器在 P3-03 接线。
- 测试命令：指定两项新测试，加旧迁移链、store、research store、chunk profiles、持久化指纹、真实详细入口/质量资料、生命周期和证据清单，共十一文件；三套类型检查及最终测试配置复核；八文件 ESLint；差异/持久化清单/指纹/运行闭包/开放边界/开放树生成。
- 测试结果：最终 **11 文件 / 139 PASS / 0 FAIL / 0 SKIP，4.55s，exit 0**；三套类型、最终测试配置、ESLint（0 错误/78 警告）、差异与边界检查均 exit 0。真实 v18 快照来自 `d17811341e0782d0f9190533dde366acb447a482`，25 表/26 行、七张研究表各有数据，SHA256 `017111b0ff49c2da4eb1f91a6d4a300dfa1762b7b6211c4dbe5c935ce34ab823`；未以新库降低版本伪造旧库。升级前后全部旧行与表结构逐项相等；真实建成前两张新表后触发 SQLite 中段 DDL 错误，整笔回滚到 v18，重试成功；原 v17→当前链也验证经过 v18 后在 v19 失败整体退回 v17。原断言保留并增加四表要求，没有删测、跳测或放宽。新存储测试开发中曾因 fixture 多传字段和重复主键遮住外键失败而报错，已修正测试数据、保留约束断言；最终 23/23 通过，初次日志已由末次运行覆盖，不伪造其精确计数。日志 `/tmp/lingxi-knowledge-p302-tests-final.log`、`/tmp/lingxi-knowledge-p302-typecheck-first.log`、`/tmp/lingxi-knowledge-p302-completeness-store-typecheck.log`、`/tmp/lingxi-knowledge-p302-lint-final.log`。
- 生成物：持久化清单 66 stores / 779 sites；知识库 v19，兼容指纹 `sha256:e114419efdf0b857a97635ed2cbffcb41b8c880b5405a55d5996e08213d0fabb`；运行闭包仍 10688 文件、开放树 883 文件。指纹由生成器按仅新增四表的兼容理由更新，没有手改派生结果。
- 后续接线约束：P3-03 开始实际写入核查记录时，现有删除/回收预检须识别新引用，不能在外键拒绝物理删除之前先标记源已删除；本项不提前增加执行器或治理行为。P3 阶段门禁与最终封印仍待剩余任务完成。
- 对应 commit SHA：本项提交（下项回填）
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
