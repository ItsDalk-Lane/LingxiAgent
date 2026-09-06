# AGENTS.md 历史快照（2026-09-06 迁移）

以下内容原样迁自项目 `AGENTS.md` 的“当前状态与下一步”。本次仅迁移保存，未重新验收其中的发布状态、测试结果、分支坐标或运行行为；时间相近的条目也可能描述互相替代的旧实现。

本文件只供追溯历史，**不是现役执行指令**，其中的“下一步”、审批与封印要求不自动成为当前任务。现行项目约定以根目录 `AGENTS.md` 为准，实际状态须按当前工作树与证据核实。

---

## 当前状态与下一步
- 2026-08-31：**v0.1.32 已发布**（GitHub Releases，Latest 正式版，18 产物齐 + publish-train
  绿）。内容：知识问答「快速/详细」两档化（cherry 对比落地）、延迟加固全链、滚动注入实测
  回归修复、供应商协议修复、过程可见合成工具卡、golden set、stageTimings。main 已含
  PR #32（功能）/ #33（release metadata）/ #34（fix(ci) renderer glob——矩阵 os 改
  macos-15 后工件名漂移打断发布上传，已按 v0.1.30 先例 retag 重出）。AtomGit 镜像慢性
  失败未修（v0.1.26 起连续）。pi SDK 0.84.1（`@earendil-works/pi-ai`）。
- 2026-08-20：v0.1.29 已发布。main 曾领先 v0.1.29 的 mac ad-hoc 自装、凭证边界修复、
  保留标签管道、模型调用可观测性 Phase 1–11（PR #25，merge 2e8077de）均已随后续版本发出。
- 2026-08-29（feat/pending-aug29）：Knowledge Phase 9 第一波已落库层（未接线 engine/injector）：
  `lib/knowledge/knowledge-coverage-{unit,manifest,executor}.ts`（CoverageUnit/Manifest/Sharding/
  ShardResult 契约/Executor/Ledger/Gate/Evidence 聚合）+ knowledge.db schema v13→v14 additive
  （coverage_runs/coverage_shards，恢复语义见 loadResumableCoverageRun）+ 指纹已 repin。
- 2026-08-29（feat/pending-aug29）：Knowledge Phase 9 第二波已接线会话链路（本波不碰 schema，
  v14 够用）：injector exhaustive 真执行（Priority Planner→manifest→executeCoverageRun→
  aggregateShardEvidence 证据注入 + 措辞闸/fidelity 行；降格/取消/超时见 coverageDegradeReason、
  KNOWLEDGE_COVERAGE_{PARTIAL,CANCELLED,TIMEOUT}）；engine 注入 coverage worker 闭包（knowledge
  槽位 + KNOWLEDGE_COVERAGE_SHARD_SYSTEM_PROMPT + distiller 线性化超时）；submit 检索期 abort
  controller；WS `knowledge_coverage_progress`（前端只保证不破坏）；stats 契约扩展见
  shared/knowledge-refs.ts（coverageRunId/coverageStatus/textCoverageRatio 等全可选）；broad→
  exhaustive 自动升级（常量可关）；run 总时长上限 KNOWLEDGE_COVERAGE_RUN_MAX_MS。
  Phase 8 的 exhaustivePending='phase-9' 降格标记已移除（现在是真执行）。
- 2026-08-30（feat/pending-aug29）：Knowledge Phase 10 已落 hierarchical evidence reduction：
  `lib/knowledge/knowledge-coverage-reduction.ts`（Shard Evidence[稳定 ev_ id] → Source →
  Notebook[共享源组间复用] → Cross-Notebook 层级管道；级预算 source 16k / notebook 12k /
  cross=注入预算，预算内零 LLM 调用；归约 I/O 均结构化 JSON——support 全集守恒禁伪造禁丢弃、
  notes verbatim、id 必须是输入 id 或升序 '+' 拼接（DP 分解校验）；纠错一次再失败该级失败 →
  整体降级保序结构化截断 + degradedReason 留痕；组间有界并发共享 executor 常量）。
  injector 三岔口已替换（伪 chunk distill 过渡路径删除）：runExhaustiveCoverage 改调
  reduceCoverageEvidence，注入块 findings 头带 evidence id + provenance，块尾层级摘要行
  （[reduced: N sources → M notebook groups, K evidence objects preserved]）；engine 注入
  coverageReduceModel 闭包（复用 knowledgeDistill 槽位 + 归约系统提示词 + 线性化超时，未配 →
  归约降级留痕）；stats 新增 coverageReduction{levels[], degradedReason?}（shared/knowledge-refs.ts）。
  manifest 的 normalizeStatement/supportKey 已导出供归约层复用。
  下一波（Phase 11 候选）：coverage 进度前端渲染、归约进度事件（当前归约无 WS 进度）。
- 2026-08-30（feat/pending-aug29）：Knowledge Phase 11 已落 Agent Knowledge 三工具（任务书 §二十三）：
  `lib/tools/knowledge-outline-tool.ts`（knowledge_outline(scopeId)：列 scope 冻结集合结构——
  选中 notebooks → 冻结 sources 的名称/类型/fidelity/coverage 单元数/首层 heading 摘要，
  数据取冻结 parseArtifact blocks/headingPath，量级有界截断并标注）、
  `lib/tools/knowledge-grep-tool.ts`（knowledge_grep(scopeId, pattern, sourceIds?, regexp?,
  headingFilter?, maxResults?)：冻结 artifact blocks 确定性 literal/regexp 扫描，命中带
  provenance（sourceId/blockId/offset/lineNumber/headingPath）；pattern 长度/全文扫描预算/
  maxResults（默认 50、上限 200）防护，未就绪源显式单列 unavailableSources）、
  `lib/tools/knowledge-manage-tool.ts`（knowledge_manage(action∈add/remove/refresh/reindex)：
  全部委托 KnowledgeManager 既有方法，审批档 kind "review" + SUBAGENT_BLOCKED_TOOLS 拦截，
  read_only 档拒绝）。Phase 4 的 scope 校验链抽成 `lib/tools/knowledge-scope.ts` 共享
  （knowledge_read 同步改用，行为不变）；三工具注册 core/agent.ts 工具快照（outline/grep
  read kind 免审批）；分类进 shared/tool-categories.ts STANDARD；文案/label 五语言包就位。
  限制（已按任务书 fallback 处理）：现有暴露面机制不支持按 surface 过滤，knowledge_manage
  依赖审批关卡 + 工具描述约束普通会话暴露面。
- 2026-08-30（feat/pending-aug29）：Knowledge §六十七 EvidenceManifest 轻量持久化已落：
  knowledge.db schema v14→v15 additive（evidence_manifests 头 + evidence_manifest_entries
  按 (source, chunkIndexVariant) 分组的身份链：snapshot/artifact/profile/civ/viv/chunk ids/
  neighbor ids/block spans/[KN] 标签；只存身份与定位，禁正文/CoT）；injector 返回第三字段
  evidence（KnowledgeInjectionEvidence，不进 UI stats；stats 新增可选 coverageManifestHash），
  query-service 结果带 searchedVectorVariants、source 清单带 chunkProfileHash；engine 门面
  recordKnowledgeEvidenceManifest（服务端按 TurnScope 冻结集合复核）；desktop-session-submit
  两路径在持久化 stats 的同一位置写 manifest（失败 warn 不阻断）；exhaustive 轮 manifest 头
  记录 coverageRunId/manifestHash、条目为空（身份在 coverage run 冻结 manifest 内）；GC/
  deleteSource 对被 manifest 引用的源跳过（evidence-manifest-referenced）/409 拒绝（条目级
  引用或 run 关联 scope 冻结；零证据普通轮不保护未贡献源）。指纹与 inventory 已 repin。
- 2026-08-30（feat/pending-aug29）：Knowledge Phase 12 已落 ProcessingArtifact 管线 +
  目录导入（任务书 §五十八/§五十九/§六十九）：`lib/knowledge/source-processors.ts`
  （DOCX=mammoth+JSDOM 块级/XLSX+CSV=exceljs 行级；一行一 block、locator.source 携带
  段落序号/单元格坐标反向定位；防护上限 100k 单元格/50k 行超限截断+warning；
  rebuildBlocksFromProcessorOutput 复用路径）；knowledge.db schema v15→v16 additive
  （processing_artifacts 按 processor 身份四元组幂等、parse_artifacts +fidelity/
  +processing_artifact_id、notebook_sources +relative_path/folder_node/display_order）；
  manager parseSource processor 分支（processor 身份并入 parserConfigHash，产物原子落盘
  knowledge/processed/<snap>/<id>.txt，ready 即复用不重跑）+ importDirectory（深度≤8/
  文件≤500、sha 去重跨 Notebook 复用 Source、目录路径写 Membership、imported/skipped/
  failed 三组明细不静默）；SUPPORTED_FILE_TYPES +.docx/.xlsx/.csv，.ppt(x)/.doc/.xls/.epub
  显式 KNOWLEDGE_IMPORT_PROCESSOR_UNAVAILABLE 拒绝（不伪造 fidelity）；coverage manifest
  与 outline 工具 fidelity 以持久化值为准；新路由 POST /knowledge/notebooks/:id/
  import-directory（local-owner 限定）。指纹/inventory 已 repin，open-boundary 已登记
  source-processors.ts；桌面端已接「导入目录」入口（KnowledgePage 导入选单第 4 项 →
  selectFolder → importKnowledgeDirectory，toast 只报 imported/skipped/failed 计数不泄露
  本机路径，5 语言 locale 齐备）。
- 2026-08-30（fix/knowledge-latency-hardening）：知识提问延迟三连修复（实测一问 1min+）：
  ①分片装填计入渲染开销——planCoverageShards 成本 = 正文 + provenance 头
  （coverageUnitPromptOverheadTokens），行级小单元源（XLSX/CSV 一行一 block）头开销
  2–3 倍正文，旧口径实测 54k token/片 vs 16k 预算 → MiniMax-M3 线性化超时全灭
  （19 调用 0 成功）；②coverage 熔断——零成功 + 终态 failed shard ≥4（一个并发波次）
  提前取消剩余，新 reason code KNOWLEDGE_COVERAGE_CIRCUIT_BREAK，最坏等待从 30min
  run 上限压到一个波次的完整 retry 周期，任一成功即豁免；③rerank 15s 期限
  （KNOWLEDGE_RERANK_DEADLINE_MS）超时/传输类失败降级保 RRF 名次 +
  rerankDegradeReason 留痕（注入块 + stats），不再炸整个检索（旧口径网络错 →
  KNOWLEDGE_RETRIEVAL_UNAVAILABLE、HTTP 120s 全额放行，实测单次重排 11–56s）。
  附带 engine 构造顺序修复：_models 须先于 _knowledge（存量库迁移期 chunk profile
  回填经闭包读 providerRegistry，晚赋值直接崩溃循环；CI 新库测不到），指纹已 repin。
- 2026-08-30（fix/knowledge-latency-hardening，08ead330）：嵌入/重排供应商协议兼容修复。
  千问 rerank 旧实现改写后拼单数 /rerank 必 404 → 新增 dashscope-rerank 双端点方言
  （gte-rerank 系/qwen3-vl-rerank 系=原生嵌套端点+output.results 归一；qwen3-rerank 系=
  compatible-api/v1/reranks 复数端点）；MiniMax 嵌入三层不匹配（/anthropic base 拼错+
  openai 形状 vs 官方 texts/type+必填 GroupId）→ 新增 minimax-embeddings 方言 + 模型条目
  groupId 字段（registry 白名单/校验/编辑面板输入位/五语言）+ inputType 穿透查询侧 query；
  rerank 文档上限 100→50 防御方舟 doubao-rerank。审计结论：智谱/方舟/千帆/SiliconFlow/
  Together/Fireworks/混元/ModelScope/Gemini/Ollama 端点形状核对全部匹配。
- 2026-08-31（fix/knowledge-latency-hardening）：知识问答重构——覆盖两档化 + 主模型滚动
  多轮注入（检索侧机器不动）。①覆盖判定三档砍两档：planner 关键词/global-negative 命中
  改定 broad（规则 id 保留兼容存量行），exhaustive 执行链路整体移除（executor/reduction/
  distiller 三模块删除、manifest 裁成 fidelity 面、store 的 coverage run 写 API 删除；
  coverage_runs/coverage_shards/evidence_manifests 表与 v13–v17 DDL 保留存量兼容，无迁移；
  存量旧值 exhaustive plan/stats 读取侧一律按 broad 处理）。②超预算不再蒸馏：
  knowledgeDistill 辅助槽位与设置项移除；新 lib/knowledge/knowledge-rollup.ts 把证据按
  预算拆 N 份滚动喂给**会话主模型**（engine 经 session.agent.streamFunction 侧线缓冲调用，
  凭证/传输/观测链复用、不进消息流不落盘；purpose=knowledge_rollup），中间笔记逐部分
  标注传递（"Intermediate notes after part k"，非对话记录）、最后一部分进最终注入块由
  正常 session.prompt 出答案；循环内模型可用 ```need-more-evidence fenced 块自主补充检索
  （走既有 retrieve 门面，轮上限 3/每轮 4 条/总轮上限 8，一切有界）；孤立超限单条也送
  消化轮（照送不丢）；qa 模式跨部分引用=最后一部分 {{cite:N}} + 前几部分行文 (part M)。
  ③过程可见：ws 新事件 knowledge_rollup_progress（「正在阅读第 X/N 部分」胶囊）+
  knowledge_supplement_search（补充检索行），stats 新增 rollup{parts,rounds,
  supplementalQueries,degradedReason}（distilled/distillBatches/truncated 保留为
  @deprecated 旧会话存量读取；coverage*/reduction 专属字段删除，前端零消费已核）。
  knowledge_coverage_progress/knowledge_distill_progress 事件与前端消费链移除；
  KnowledgeRetrievalFold 增加分批阅读徽标与补充检索行；五语言文案就位。开放边界清单与
  持久化指纹已 repin（compatible）。
- 2026-08-31（fix/knowledge-latency-hardening，二轮）：知识注入全程过程可见（对齐编程
  Agent 的工具调用过程卡，替代「三个点干等」）。engine 侧拆解/扩展/补证闭包与 retrieve
  门面统一插桩新 ws 事件 knowledge_trace（kind think|search；phase start|done|failed；
  只发查询词/命中数/方向名等元数据，禁发模型中间输出）——直检/子查询/扩展/gap/broad
  探测/滚动补充检索全部经 retrieve 门面故逐行可见；前端 streaming-slice 新增
  knowledgeTraceBySession 过程行堆（按 id 原位更新：检索行 start=查询词 →
  done=「N 个搜索结果」），rollup_progress/supplement_search 事件同步映射为
  read/note 行；ChatMessageSurface 以 toolIndicator 样式堆渲染（interject 流式期间
  也可见），首个非知识过程事件（session_user_message 等）保守清除整堆，检索胶囊
  降级为 trace 为空时的回退。五语言新增 chat.knowledgeTrace* 文案。
- 2026-08-31（fix/knowledge-latency-hardening，三轮）：过程行堆改为「等待态本体」
  ——不再被普通事件清除（修「检索提示消失→退回三个点干等」）：只在答案正文首个
  text_delta 或 assistant_run_end（中止/空回包兜底）时整堆收起；新一轮
  knowledge_retrieval_started 重开空堆。检索完成后 submit 两路径（prompt/interject）
  追加「正在生成回答」trace 行（note+detail=answer），盖住主模型预填充+生成这段
  无流式输出的等待。五语言 chat.knowledgeTraceAnswering。
- 2026-08-31（fix/knowledge-latency-hardening，四轮）：知识过程改为**合成工具卡**
  ——对齐编程 Agent 的工具调用形态，一个动作一张卡、依次长在助手消息流里（不再
  是底部指示器区的行堆）。前端 ws 层把 knowledge_trace/rollup_progress/
  supplement_search 翻译成合成 tool_start/tool_end 喂 streamBufferManager（tool_start
  在无消息时自动创建助手消息，后续 assistant_run_start/答案正文复用同一条）：
  🧠 拆解问题卡、🔍 检索卡（args.query 为行内 detail，done 带结果注记「N 个结果」
  ——ToolCall 新增可选 resultNote 字段经 tool_end 透出）、📖 阅读卡（每部分一张）、
  ↻ 补充检索决策卡、✻ 正在生成回答卡（text_delta 首字/assistant_run_end 收尾；
  回答卡以在途集合守卫——无卡会话绝不喂空 tool_end，否则 ensureMessage 会凭空
  造出第二条助手消息）。tool-label 内置名单登记 5 个合成工具名 + 五语言三相位
  文案；旧过程行堆渲染/状态（knowledgeTraceBySession）整体移除；卡片为纯前端
  合成（不进会话流协议/持久化，历史轮靠 KnowledgeRetrievalFold）。
- 2026-08-31（fix/knowledge-latency-hardening，五轮·实测回归修复）：用户实测一轮
  6.5 分钟无输出，observability 取证实锤两个根因并修复：①滚动「一份」只按剩余
  预算装填 → 单份 ~49 万 token → 主模型预填充实测 240s 兜底超时被掐（aborted）
  → 重试再 163s，单轮烧 6.5 分钟——新增 KNOWLEDGE_ROLLUP_PART_MAX_TOKENS=64k
  单份封顶（轮数换时延，大窗口模型每份 ~20-30s 预填充）；②滚动侧线调用经
  session streamFunction 未带用途标记 → 缓存前缀守卫把消化提示词当成会话系统
  提示词记账（cache_contract_violation ×2，10860↔1234 字节互踩）→ 真实回答轮
  被判漂移作废缓存全量重填——修复：engine 侧线调用整包 runWithProviderCompatPurpose
  （knowledge_rollup），缓存守卫与 Context Ring 分类对非 chat 用途侧线一律跳过。
  cli-runtime-closure / open-boundary baseline / 持久化指纹（sha256:5f525a1d…）重钉；
  typecheck×3 绿 + 全量 12773 用例绿。注意：dev 桌面链 server 直跑源码但**进程
  启动时加载**——修复需重启 dev 应用后生效。
- 2026-08-31（fix/knowledge-latency-hardening，六轮·用户截图验收）：合成工具卡
  形态经用户实机截图确认达标（一动作一卡依次长在消息流：检索卡带查询词与
  「N 个搜索结果」、阅读卡每部分一张、补充检索后新查询卡各自跟进、生成回答卡
  收尾；简单问题跳过拆解故无 🧠 卡属预期）。修补充检索卡文案重复（标签
  「模型发起了补充检索」× resultNote 同文）→ resultNote 换 count-only 新键
  chat.knowledgeSupplementQueryCount（五语言）。
- 2026-08-31（fix/knowledge-latency-hardening，七轮·两档化）：知识问答改「快速/详细」
  两档（cherry-studio 对比 + 外部建议甄别后的落地）：answerMode 从 qa/assist 改
  fast/detailed（normalizeKnowledgeRefs 严格拒旧值；存量 qa/assist 读取侧按详细
  处理、显示层保留旧标签；默认快速）。**快速档**=零辅助 LLM 轮（engine 跳
  coverage planner、injector 跳拆解/扩展/gap/探测）+ rerank 动态门控（top-1
  RRF 融合分领先 ≥ KNOWLEDGE_RERANK_CLEAR_MARGIN=0.008 即跳过、扎堆时限
  KNOWLEDGE_FAST_RERANK_DEADLINE_MS=5s，留痕 rerankSkippedReason 只进 stats）+
  证据硬封顶（锚点 ≤KNOWLEDGE_FAST_MAX_EVIDENCE_ENTRIES=12、渲染预算
  ≤KNOWLEDGE_FAST_RENDER_BUDGET_TOKENS=8192）+ 禁滚动消化（超封顶截断留痕
  "fast mode: rolling digest disabled"）。**详细档**=原行为不变（回归锚）。
  新增 rerankPolicy 穿线（injector deps.retrieve → engine 闭包 → KQS）、
  检索分段计时 stats.stageTimings{fts/embed/vector/fuse/rerank/planner/
  assemble/rollup/totalMs}（检索段跨笔记本取 max）、golden set 质量门禁
  （tests/knowledge-retrieval-golden.test.ts：固定问题→应命中源，fast top-12
  与 detailed 双档 recall 断言）。五语言 8 新键（input.knowledgeModeFast/
  Detailed+hint、chat.knowledgeMetaModeFast/Detailed，旧键保留渲染存量）。
  fingerprint 未动（sha256:5f525a1d 不变）。
- 上游 v0.444.1→v0.447.4 同步已随 v0.1.29 发布（PR #20）；相关 feature 分支均已合并删除。
- Seal 当前坐标以 `.sync-audit/verified-source-sha.txt` 为准。任何非审计 allowlist 的
  正常开发提交都会挂 post-verification audit-seal 门禁：提交前按 PROGRESS.md「Seal 推进
  记录」复跑验证并推进全部坐标副本（PROGRESS.md / UPSTREAM_SYNC_MATRIX.md /
  UPSTREAM_SYNC_AUDIT.md / .sync-audit/verified-source-sha.txt /
  .sync-audit/upstream-sync-matrix.json / AGENTS.md 六处），或退役该门禁。allowlist 在
  tests/post-verification-audit-seal.test.ts 与 .sync-audit/verify-post-verification-diff.mjs
  两处副本，须同步维护。
- About 页「检查更新」主检测源 GitHub Releases（commit 2780c55 起）；CI 配齐 Apple 凭据后
  `SKIP_NOTARIZE` 自动关闭并切换到正式签名+公证。
