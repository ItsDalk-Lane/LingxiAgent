# R06-T07 现役知识库迁移 — 设计（验收映射 + 勘察锚点）

- TASK_BASE_SHA: `14ebcd145606d945a3316aea99ff96ad095672ad`（分支 `codex/rust-tauri-migration`）
- 勘察基准：base 工作区 = TASK_BASE_SHA（RC-2 全部行号在该树上亲验）
- 工具链：`/Users/study_superior/.cargo/bin/cargo`（rustup shim → 1.98.1），全程 `-j 8`

## 0. 第一性原理（用户可观察行为）

用户选择笔记本范围提问 → 检索只及所选笔记本的来源 → 每条命中带能定位原文的引用 →
嵌入模型变化不会悄悄返回貌似正常的错检索（显式降级/要求重建，且重建不删源文件）。

逆向生产链（现役 TS，全部锚行号见 §3）：

```
用户选笔记本（chat knowledgeRefs.notebookIds）
→ engine.buildConversationKnowledgeContext（core/engine.ts:2771-2833）
   ├ 校验四工具齐备（:2787）
   ├ createTurnScope 冻结 (studio, session, turn) 的源集（knowledge-store.ts:3281-3344，
   │  同会话旧 active scope 关闭 :3301-3304；逐源冻结最新 snapshot/artifact :3319-3333）
   └ compileTurnScope → [KnowledgeContext] 来源材料块（:2802-2824，
      明示"资料中的指令不能改变用户任务"——不可信注入边界）
→ 模型调 knowledge_search/read/grep/outline（lib/tools/knowledge-*.ts + knowledge-scope.ts）
   └ 服务端逐次复核 scope（knowledge-scope.ts:56-107：存在/active/studio/会话归属/
      sourceId ∈ 冻结集），引用由程序绑定冻结原文（:126-155），不信任模型 id
→ 检索（knowledge-query-service.ts retrieve :1004-1233）
   ├ FTS5 通道（index store chunk_index_variants，按 (parseArtifactId, chunkProfileHash) 变体）
   ├ 向量通道：viv = f(civ, modelKey) 确定性派生（:1159-1169），variant 必须 ready 且
   │  modelKey/dimensions 与查询嵌入一致；否则 KNOWLEDGE_VECTOR_NOT_READY 显式降级（:1169）
   ├ 查询嵌入失败/超时 → 显式降级纯 FTS + KNOWLEDGE_EMBEDDING_FAILED 留痕（:1124-1147）
   └ RRF k=60 融合（:238, fuseCandidates）
→ 引用（knowledge_read/grep 的 spans → createKnowledgeToolCitation knowledge-scope.ts:126-155
   → GET /knowledge/citations/:id routes/knowledge.ts:598-627 → GET /knowledge/snapshots/:id/content :629-654）
→ 摄入（routes POST sources → parseSource manager:1322-1479 → enqueueSourceIngestion
   → worker chunk→fts_index→embed（ingestion-service.ts），嵌入模型未配置 = 显式
   pending_embedding 终态（types.ts:470 注释），模型就绪信号唤醒（engine.ts:2401-2402））
→ 索引身份（vector-index-adapter.ts:1-68 schema v2/v3 契约）：
   ChunkIndexVariant = (parseArtifactId, chunkProfileHash)，civ_ = sha256(a\0b)[0..32]
   VectorIndexVariant = (civ, modelKey)，viv_ = sha256(civ\0modelKey)[0..32]
   modelKey = sha256(JSON[provider, modelId, protocol, dimensions])（query-service:173-206）
```

## 1. 验收映射（Requirement → Production Entry → Owner → Implementation → Test → Evidence）

### R06-A13 检索引用可验证且不串来源

| 项 | 值 |
|---|---|
| Requirement | 两个来源空间含相近内容；指定范围检索只返回允许来源；引用能定位实际文本 |
| Production Entry | HTTP `/lingxi/v1/knowledge/*`（笔记本/来源/引用/快照面）+ R04 网关工具 `knowledge_search/read/grep/outline` |
| Owner | KnowledgeService（service/knowledge.rs）+ KnowledgeToolExecutor（service/knowledgetools.rs） |
| Implementation | TurnScope 冻结（notebook 集合 → 逐源 snapshot/artifact）；检索逐 scope 限定 (parseArtifactId, chunkProfileHash, fingerprint) 变体；工具面 RunContext.session_id 归属复核 + sourceId ∈ 冻结集；citation = block + 双层偏移 + canonicalTextSha256，resolve 从库复读比对 |
| Positive Test | 两笔记本 A/B 写入相近文本；仅选 A 建 scope → search/hits 只含 A 来源；read 返回 spans 与原文逐字节一致；citation resolve 定位到 (blockId, startOffset, endOffset) 且 canonicalText == 原文切片 |
| Negative Test | 模型伪造 scopeId（他源/已关闭/越会话）→ KNOWLEDGE_SCOPE_VIOLATION；B 中相近文本在 A-scope 检索结果零出现（对全库检索的对照组）；分页越界/offset 切割代理对 → 显式错误 |
| Evidence | `05_targeted_tests/`、报告 §A13 |

### R06-A14 embedding 变化不静默损坏

| 项 | 值 |
|---|---|
| Requirement | 已有索引用维度 A；切到不兼容模型并检索 → 提示重建/安全策略，不返回貌似正常的错检索 |
| Production Entry | 知识检索（HTTP 工具面同链）+ PUT settings 触发重建 + 摄入 worker |
| Owner | KnowledgeService（retrieval 向量门 + rebuild 编排）+ GatewayKnowledgeEmbedder（R05 ModelGateway/OperationService 链） |
| Implementation | 索引元数据落库（vector_index_variants.model_key/dimensions/chunk_fingerprint/status）；查询嵌入身份 = (provider, model, protocol, 实测维度)；viv 确定性派生 → 变体不存在/非 ready/维度不符 → 显式 degraded（KNOWLEDGE_VECTOR_NOT_READY / REBUILD_REQUIRED 语义），FTS 候选保留；绝不跨维度余弦。模型变更走 settings PUT → enqueueNotebookRebuild（新变体并建，旧变体不覆盖：任务书 §十 经 manager:1774-1810 镜像）；重建完成前检索的安全策略 = FTS 降级 + 降级留痕；重建不触源文件（快照/原文 immutable） |
| Positive Test | loopback 嵌入替身 dim=2 建索引（metadata 断言 model_key/dimensions=2）→ 切 dim=3 替身 → 检索：无任何向量命中、`mode=fts`、degraded 携带 KNOWLEDGE_VECTOR_NOT_READY；按新模型重建（reindex/rebuild 链）→ viv(dim=3) ready、dimensions=3、检索恢复 hybrid、源文件/快照 id 不变 |
| Negative Test | 换模型后检索绝不返回 dim=2 变体的命中（对照组：dim=2 变体仍在库，检索身份已切换）；查询嵌入批响应形状非法 → KNOWLEDGE_RETRIEVAL_UNAVAILABLE |
| Evidence | `05_targeted_tests/`、报告 §A14 |

## 2. 交付物 → Rust 布局

| Deliverable | 文件 |
|---|---|
| KnowledgeService | `rust/crates/lingxi-service/src/knowledge.rs`（服务 + HTTP 面）、`knowledgetools.rs`（5 个模型侧工具） |
| 存储三库 | `rust/crates/lingxi-adapters/src/storage/knowledge.rs`（facts/index/vector 三 SQLite + 快照/产物文件存储；runs.db v1–v9 冻结不动） |
| 纯逻辑 | `rust/crates/lingxi-kernel/src/knowledge.rs`（错误码、token 估算、chunker、身份算法、RRF、scope 规则、维度门） |
| 索引兼容矩阵 | 本文件 §4 + 报告（索引身份算法、变体共存、重建矩阵） |
| 引用与迁移测试 | `r06_t07_knowledge`（HTTP 全链）、`r06_t07_knowledge_tools`（网关工具）、`knowledge_store_r06t07`（存储）、kernel 单测 |

不恢复研究引擎：不建 research_runs/analysis_*/coverage_runs 等退役表（负向断言见测试）；
TurnScope/EvidenceManifest 之外的超范围（coverage planner、rerank 策略面）按差异台账申报。

## 3. RC-1/RC-2 勘察锚点（base 工作区亲验）

### HTTP 面（server/routes/knowledge.ts，657 行）
- L56-62 错误→HTTP 状态映射（KNOWLEDGE_INDEX_INVALID/RETRIEVAL_UNAVAILABLE/MODEL_UNAVAILABLE→503 等）
- L78-116 bindKnowledgeScope（studio 归属 + STUDIO_OWNER + studio 一致性；无 knowledge 引擎 503）
- L118-158 来源/快照/产物脱敏序列化（originMetadata.url 去 query/hash；storagePath/semanticArtifactPath 不出域）
- L160-190 body 精确键校验 + settings 键白名单（embeddingModelRef/rerankModelRef/chunkTargetChars/retrievalTopK/vectorRetentionDays）
- L196-220 笔记本就绪汇总（done/pending_embedding/processing/failed/untracked，按 (source, notebook) 最新 job 归类）
- L225-244 GET notebooks（config + chunkTargetCharsEffective + 汇总）；L246-256 POST（name）；L258-300 GET/PATCH/DELETE :id
- L302-318 PUT settings（范围/引用完整性由 store 校验；重建在 manager）
- L320-335 GET ingestion（jobs+counts+unreachableSources）
- L337-433 GET/POST sources（file=local owner :374-379；parse 失败也保底入队 :407-416；入队后立即返回 :417-429）
- L437-486 import-directory（local owner；逐源 parse+入队；部分失败显式留痕 :472-480）
- L488-501 DELETE membership（remove ≠ 删源）；L506-522 DELETE sources/:id（delete wins）
- L524-549 refresh（local owner；changed 语义）；L551-566 reingest（failed 重试，409 冲突）
- L568-596 blocks/chunks（冻结契约 `{chunkerConfigId, chunks:[{id, ordinal 1-based, text, tokenCount, charCount, headingPath?, pageNumber?}]}` L582-583）
- L598-627 citations resolve（缺 headingPath 从章节 metadata 补齐 :607-611；viewer.contentUrl + locator）
- L629-654 snapshot content（HTML→text/plain + CSP sandbox/nosniff/ETag=sha256 :641-650）

### 数据模型（lib/knowledge/types.ts）
- L168-320 Notebook/Source/Membership/ContentSnapshot/ParseArtifact/Block/Citation/ModelRef/NotebookConfig
- L329-369 ChunkProfile/RetrievalProfile（profileHash=chunkerConfigId 同源算法；legacy 行不伪造）
- L371-409 TurnScope（active/closed；冻结源集合；notebookIds 按选择顺序）
- L463-534 IngestionJob（phase 链 parse→chunk→fts_index→embed→done；pending_embedding 显式终态 L470；
  cancelledAt 复用 failed + 显式标注 L521-525；embeddingStats L478-499）

### 身份算法（跨库身份键，逐字节冻结）
- chunker.ts L6 `KNOWLEDGE_CHUNKER_VERSION="4"`；L120-127 configId=sha256(`4{strategy}{targetChars}`)[0..16]
- chunker.ts L86-100 chunk 上下文分数：MIN 100 / MAX 100_000；auto=⌊contextWindow×0.8×4×0.25⌋?? （computeAutoChunkTargetChars L100-110：`Math.floor(contextWindow * 0.8 * 4 * 0.25)` 见实现）；CJK=1.1 tok/char、非 CJK=4 char/tok（lib/llm/estimate-text-tokens.ts L13-46）
- vector-index-adapter.ts L91-104 civ_/viv_ 算法；L84 VECTOR_INDEX_SCHEMA_VERSION=3；L121-127 dimensions 1..65536
- query-service.ts L173-206 modelKey=sha256(JSON[provider,modelId,protocol,dimensions])；assertEmbeddingBatch L198-233
- query-service.ts L238 RRF k=60；L360 UNCAPPED_LIMIT=1000；L369 EMBEDDING_DEADLINE=15s

### 摄入/重建语义
- manager.ts L1774-1810 updateNotebookSettings：embedding/chunk 变化 → enqueueNotebookRebuild（变体并建不覆盖）
- manager.ts L1322-1479 parseSource（parser "lingxi-citation" v1；fidelity；失败落 failed + KNOWLEDGE_PARSE_FAILED）
- manager.ts L1833-1846 requeueSourceIngestion（无 job 兜底入队；非 failed 409 由 store 抛）
- source-adapters.ts L36-90 text/plain、text/markdown 引用级解析（逐行块 + locator lineStart/charStart/headingPath）；
  L111-145 html（jsdom 块选择器）；L152-221 pdf（unpdf，空→needs_ocr）；L237 其他→unsupported_citation_format
- ingestion worker：批级 checkpoint（vector-index-adapter.ts L39-56 begin/upsert/complete/fail）；指纹/维度漂移→显式 resetStaleVectors（L45-47）
- delete wins：DELETE sources/:id 取消活跃 job、清理变体与物理痕迹；活跃 turn 冻结引用 409（routes L503-505）

### 工具面（lib/tools/）
- knowledge-scope.ts L24-49 violation/无会话上下文显式不可用；L56-77 scope 复核链（存在/active/studio/会话/祖先）；L80-89 sourceId ∈ 冻结集；L95-107 notebookId 归属；L126-155 引用签发前重查 active + 冻结一致；L158-251 readKnowledgeCitationPage（offset/maxChars 256..8000/代理对边界/字节预算二分）
- knowledge-search-tool.ts L54-79 参数严检（未知键拒绝 L70-71；limit 1..24 L77-79；channel fts|hybrid）；span 命中→直接回读冻结原文（L131-143）；标题命中→navigation-hint（L146-149）
- knowledge-manage-tool.ts L81-135 action ∈ add/remove/refresh/reindex；审批档 kind "review"（read_only 拒绝）
- knowledge-read/grep/outline：同 scope 链；grep 片段回源；outline 章节定位供 read

### 上下文入口（T01 编译器既有材料机制）
- engine.ts L2771-2833 [KnowledgeContext] 块为来源材料（指令不提升，L2818 明示）；Rust 侧本任务交付
  TurnScope/编译/检索/引用的**服务与工具真面**；[KnowledgeContext] 注入链挂接会话提交（desktop chat submit）
  归 R07/R08 外围面（差异台账 D 项申报）——服务面已可被 HTTP/工具真实驱动。

## 4. 索引兼容矩阵（A14 核心）

| 已落库变体（model_key/dims） | 查询身份 | 行为 |
|---|---|---|
| viv(civ, K_A, dims=2) ready | K_A, dims=2 | 向量通道参与，余弦 + RRF 融合 |
| viv(civ, K_A, dims=2) ready | K_B, dims=3（换模型） | viv(civ,K_B) 缺失 → 显式降级 KNOWLEDGE_VECTOR_NOT_READY（FTS 保留），绝不取 dims=2 向量 |
| viv(civ, K_B, dims=3) building | K_B, dims=3 | 同上（非 ready 不读半写状态） |
| 重建完成后 viv(civ, K_B, dims=3) ready | K_B, dims=3 | 恢复 hybrid；旧 K_A 变体保留（任务书 §十），按 retention 策略另行回收（本任务不实现 sweep，D 项） |
| 变体行存在但 chunk_fingerprint 漂移 | 任意 | 摄入侧显式 reset 重建（resetStaleVectors 留痕）；查询侧 fingerprint 不一致 → KNOWLEDGE_INDEX_BUILDING 降级 |
| chunk_profile 变化（chunkTargetChars） | 任意 | 新 civ 并建；旧 civ 变体不删（可重建≠可删源；快照 immutable） |
| 库文件损坏（打开失败/迁移错） | — | 仅该缓存文件 reset 重建（TS §十三 语义）；事实库（knowledge.db）永不因此触碰 |

维度铁律：向量 BLOB 长度 == variant.dimensions×4；查询向量长度 == 查询身份 dimensions；
不一致 → KNOWLEDGE_RETRIEVAL_UNAVAILABLE / KNOWLEDGE_CONFLICT（写入侧），绝不混查。

## 5. Rust 实现决定（R 项，实施细化申报）

- R1 三库布局：knowledge.db（事实，user_version=1）/ knowledge_index.db（FTS5 缓存）/ knowledge_vector.db（向量缓存），
  快照字节 `<data_home>/knowledge/snapshots/<id>`、解析产物 `<data_home>/knowledge/artifacts/<snapshotId>/<artifactId>.json`。
  与 TS 三文件布局同构（knowledge.db / knowledge-index.db / knowledge-vector.db）。
- R2 迁移面：新库首建 v1；`user_version > 1` → KNOWLEDGE_SCHEMA_NEWER（500）；缓存库损坏 → reset 重建（对齐 TS §十三）。
  runs.db v1–v9 冻结不动；旧 TS knowledge.db 的接管/搬移属 R08（兼容残留叶）。
- R3 解析器：text/plain、text/markdown 全保真（L36-90 逐行镜像）；text/html 以标签扫描提取块选择器
  （无 jsdom，结构性 locator 简化为 headingPath + ordinal——D 项）；pdf/docx/xlsx → unsupported_citation_format
  （KNOWLEDGE_PARSE_FAILED 显式失败，不静默；处理器管线属 T05 J3 同族顺延）。
- R4 嵌入链：`GatewayKnowledgeEmbedder` = identity 经 `ModelGatewayPort::resolve_route(Embedding)`
  （模型未配置 → 显式 pending_embedding；禁静默换模型），调用走 `OperationService::embed`
  （R05 凭证/配额/台账链，禁绕过）；维度取实测响应向量长度并全批校验（assertEmbeddingBatch 镜像）。
  协议串用 ProtocolFamily wire 名（"openai-embeddings" 等，model_exchange.rs:485-489）。
- R5 检索：FTS5（query 构造镜像 kernel memory.rs 既有 FTS 规则族；知识侧按 OR 词项 + 前缀）+
  可选向量通道（查询嵌入 + 全量扫描余弦，TS portable adapter 同法）+ RRF k=60 融合；
  scope 编译只读冻结集合，绝不现场扩大。
- R6 工具会话绑定：TurnScope 绑定 Rust `session_id`（RunContext.session_id，等价 TS sessionPath 归属）；
  工具执行经 RunContext 复核，无会话上下文（直接 HTTP 驱动的工具执行器不存在——网关外的执行路径无）
  与越会话一样 KNOWLEDGE_SCOPE_VIOLATION。
- R7 [KnowledgeContext] 注入链（会话提交时建 scope + 注入材料块）：服务面（createTurnScope/compile/
  检索/引用）本任务全量交付并经 HTTP+工具真实驱动；提交链挂接归 R07/R08（会话提交面 owner），
  差异台账申报，不属静默缺口。
- R8 web_snapshot：URL/SSRF 规则镜像 web-snapshot-security.ts（http/https、私网/链路本地拦截、
  20MB 上限、text/html|text/plain|text/markdown 才可引用级解析），抓取经共享 reqwest 客户端；
  测试用 loopback 替身（本机隔离环境口径）。
- R9 摄入 worker：单进程内 tokio worker（wake 通道）；重启恢复 = queued/building 变体断点续嵌
  （checkpoint=已落库向量 diff）；取消 = DELETE 源时 cancel（failed+cancelledAt）；无独立进程。
- R10 notebook「studio」：Rust service 为单 studio（management.identity_ids().1，lib.rs:1797 先例）；
  knowledge 面归属校验 = 本 studio 常量，跨 studio 结构性不可达（单实例单 studio）。

## 6. 24 叶 → 状态预分配（叶图 t07_status 依据）

- implemented：9 路由行为叶 + citations/ingestion 查看/reingest/membership/refresh/settings 重建/
  chunk cards/blocks/snapshots/sources 删除 + 5 工具叶（21）
- share：F-D08-C-RESEARCH-LEDGER（兼容残留：不建退役表、不恢复引擎、R08 迁移时只读接管）
- deferred：无整叶顺延；半面差异（html 解析简化、[KnowledgeContext] 提交链、retention sweep、
  evidence manifest 写面、coverage/rerank 策略面）逐条入差异台账 D 项。
