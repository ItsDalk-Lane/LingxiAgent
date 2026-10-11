# R06-T07 差异台账（RC-2：全部条目锚定现役 file:line，base 工作区 = TASK_BASE_SHA `14ebcd145606d945a3316aea99ff96ad095672ad`）

现役锚点均在勘察期亲验（RC-1/RC-2，行号见各条）；实现细化（R 项）与行为差异（D 项）逐条申报，
待独立审查复核。语义槽位与身份算法按 design.md §0/§3/§4。

## D 项（现役 → 候选的行为差异）

- **D1 路由前缀现代化**：现役 Hono `/knowledge/*`（server/routes/knowledge.ts:225-654，
  server/index.ts 的 `/api` 挂载）→ 候选 `/lingxi/v1/knowledge/*`（service/knowledge.rs
  knowledge_routes()）。T05/T06 同族先例（D5）。
- **D2 studio 归属闸 → LocalOnly**：现役 bindKnowledgeScope 要求 STUDIO_OWNER +
  principal.studioId == runtimeContext.studioId（routes L78-116）→ 候选 auth.rs 分类块
  将全部 `/lingxi/v1/knowledge` 归 LocalOnly（单 studio 单实例，R10 口径；T06 D8 同族
  收窄先例）。跨 studio 结构性不可达（单 studio 常量，lib.rs identity_ids().1）。
- **D3 三库文件名**：现役 knowledge.db / knowledge-index.db / knowledge-vector.db →
  候选 knowledge.db / knowledge_index.db / knowledge_vector.db（下划线；同一目录布局，
  快照/产物子目录同构）。R08 迁移时按文件名映射接管。
- **D4 事实库 schema v1（新建）**：现役 knowledge.db SCHEMA_VERSION=19（knowledge-store.ts:56，
  含 v6/v8/v9/v11/v13/v15 等演进列）→ 候选首建 v1（合并全部现役列形状；
  `user_version > 1` → KNOWLEDGE_SCHEMA_NEWER 500）。旧库接管/搬移属 R08。
- **D5 索引/向量缓存 schema v1（新建）**：现役 KNOWLEDGE_INDEX_SCHEMA_VERSION=4（含
  v1→v2 chunk_index_variants 身份迁移）/ VECTOR=3（v2 身份迁移 + v3 last_used_at）→
  候选直接以变体身份建 v1（迁移史不携带；损坏 reset 语义保留，§十三 对应）。
  `vector_index_variants.last_used_at` 列缺席（见 D12 retention sweep）。
- **D6 chunk 偏移口径**：现役 UTF-16 code unit（chunker.ts tokenBoundary/characterBoundary
  基于 charCodeAt）→ 候选 Unicode scalar（char）。A13 的「等价锚点」：偏移与
  canonicalText/block.text 在候选内自洽可比对（citation_roundtrip 测试）。R08 迁移时
  需按新口径重切（变体身份含 chunker 版本串，D 前缀"4"不变则旧 chunk id 不兼容——
  R08 必须重索引；不丢源文件）。
- **D7 jieba 词级分词缺席**：现役检索分词 = NFKC + ASCII 词 + jieba cutForSearch +
  CJK 2/3-gram（lib/search/search-text.ts:47-60）→ 候选 = lowercase + ASCII 词 +
  CJK 2/3-gram（adapters knowledge.rs tokenize_search_text）。CJK 召回由 n-gram 层
  承担；jieba 词级 token 的精度贡献缺席。NFKC 归一化缺席（to_lowercase 替代）。
  依赖治理：jieba-rs 未引入（新第三方 crate 需 D-登记，本任务不引入）。
- **D8 html 解析器简化**：现役 jsdom 块选择器（h1-h6/p/li/blockquote/pre/td/th +
  structuralPath CSS 定位，source-adapters.ts:92-145）→ 候选标签扫描提取（headingPath
  + tag locator；无 structuralPath/无嵌套去重）。fidelity 仍 citation_grade（文本与
  偏移可定位），locator 结构性降级。
- **D9 pdf/docx/xlsx/csv 解析缺席**：现役 unpdf 引用级解析 + §五十八 processor 管线
  （docx/xlsx/csv，manager.ts:1327-1402）→ 候选 import 接受（mime 映射 + %PDF- 魔数 +
  二进制文本拒绝同现役），parse 显式失败 `processor_unavailable`（KNOWLEDGE_PARSE_FAILED，
  不静默）。与 T05 J3（mammoth/ExcelJS 依赖治理）同族顺延。
- **D10 web_snapshot 抓取面缺席**：现役 POST sources kind=web_snapshot 走
  web-snapshot-security.ts（SSRF 拦截 + 20MB + 类型检查）真实抓取 → 候选该 kind 显式
  422 KNOWLEDGE_WEB_TYPE_UNSUPPORTED（外部抓取器网络面归 R07 外围连接器）。file 与
  pasted_text 全保真。路由表三 kind 校验、错误映射保留。
- **D11 [KnowledgeContext] 会话提交链挂接顺延**：现役 engine.buildConversationKnowledgeContext
  （engine.ts:2771-2833，chat 提交时建 scope + 注入材料块）→ 候选交付服务与工具真面
  （create_turn_scope HTTP 面 + 工具 scope 复核 + [KnowledgeContext] 文案常量在
  knowledgetools citationNotice 承担信任边界句）；desktop chat submit 的调用点归
  R07/R08（会话提交面 owner）。服务面无静默缺口：scope 经 HTTP 真实创建与消费。
- **D12 retention sweep / orphan GC 定时器缺席**：现役 sweepStaleVectorArtifacts
  （query-service.ts:433-471，每小时量级）+ KNOWLEDGE_LIFECYCLE_GC_INTERVAL_MS
  （manager.ts:68）→ 候选 sweep 不实现（vectorRetentionDays 配置面保留、校验保留）；
  orphan 标记保留（remove membership 零活跃即置位），物理 GC 顺延 R07。旧模型变体
  不自动回收（任务书 §十：变体并建不覆盖）。
- **D13 evidence manifest / coverage planner / rerank 策略面缺席**：现役 schema v13/v15
  （coverage plans、evidence manifests，types.ts:411-461）+ rerank policy
  （rerank-policy.ts）+ search grain 分层（document/section/span）→ 候选检索交付
  span/section 两 grain + RRF 融合 + 显式 degraded；manifest 落库与 coverage/rerank
  档位顺延（叶图未含独立叶；A13/A14 不依赖）。
- **D14 unreachableSources 面**：现役 GET ingestion 附 listSourceFileWatchStates 的
  不可达项（routes L329-331，Phase 6 文件 watch）→ 候选无 watcher（T05 D7 同族：
  fs.watch → 轮询的等价物未建），unreachableSources 恒空数组；refresh/changed 语义
  由显式 refresh 端点承担。watcher 顺延 R07。
- **D15 来源 id 生成器**：现役 `crypto.randomUUID()`（`knb_/ksrc_` 前缀 +
  uuid）→ 候选 12 随机字节 hex（getrandom）。不透明字符串身份，形状不进契约。
- **D16 环保面错误细分**：现役 KNOWLEDGE_IMPORT_NOT_FOUND → HTTP 400（routes L30-62
  无映射走 default）→ 候选同映射 400；KNOWLEDGE_RETRIEVAL_EMPTY/SCOPE_NOT_READY
  保留在错误码表（kernel knowledge.rs knowledge_error_status 全表镜像）。
- **D17 chunkProfile 类型面收窄**：现役 chunk_profiles/retrieval_profiles 表
  （schema v9，standard/legacy 双型 + 共享 RetrievalProfile 身份）→ 候选不落独立
  profile 表：profileHash = chunkerConfigId（跨库身份键同源）在 scope 编译与
  worker 两侧确定性重推导（resolve_chunker_config 同函数）；legacy 型（不可推导
  历史指纹）不迁移（R08 重索引时统一按新配置建档）。等价承担：共享派生索引语义
  由 (parseArtifactId, profileHash) 变体身份天然提供。
- **D18 环境取证**：loopback embedding 替身（openai-embeddings 线格式、维度随模型名
  embed-model-a=2 / embed-model-b=3）= 本机隔离环境口径，不冒称真实供应商验证。

## R 项（实现细化，非行为差异）

- **R1 三库 + 文件存储布局**：见 adapters knowledge.rs 模块注；快照字节
  `knowledge/snapshots/<id>`、产物 `knowledge/artifacts/<snapshotId>/<artifactId>.json`
  （原子写 .tmp → rename，manager.ts:1361-1437 同语义）。
- **R2 嵌入链**：`GatewayKnowledgeEmbedder` = 身份探测经 `ModelGatewayPort::
  resolve_route(Embedding)`（未配置 → None → 摄入 pending_embedding / 检索显式降级）；
  调用走 `OperationService::embed`（R05 凭证/配额/台账链）。维度取首批实测响应向量
  长度（TS 的 model.dimensions 声明字段在 Rust 配面缺席——实测即事实，
  assertEmbeddingBatch 全批一致性校验对应保留）。
- **R3 摄入 worker**：单进程 tokio worker（wake Notify + 500ms tick）；首批探测
  → begin build（指纹/维度漂移显式 reset）→ 64 块/批 checkpoint → complete 校验。
  取消 = delete wins（failed + cancelledAt，TS CHECK 枚举限制的同承担）。
- **R4 pending_embedding 恢复**：`set_embedder`（模型就绪信号，engine.ts:2401-2402
  同语义）+ 启动时 set_embedder 均触发 requeue_pending_embedding_jobs。
- **R5 检索**：scope 编译 → ready 锚 (artifact, profileHash, fingerprint) → FTS5
  （变体集合限定）→ hybrid 时查询嵌入 + viv 确定性校验（ready + modelKey + 维度
  全对才参与）→ RRF k=60。降级显式（KNOWLEDGE_INDEX_BUILDING / VECTOR_NOT_READY /
  EMBEDDING_FAILED），detail 字段人类可读（判定只看 reason，现役同纪律）。
- **R6 工具面**：五工具经 R04 网关（prepare → policy → execute）；RunContext.session_id
  承担 TS sessionPath 归属复核；manage 走 Execute 档（TS 审批档 review 的网关等价）。
  schema additionalProperties:false 在 prepare 层拒绝未知参数/未知 action
  （TS 在 execute 内同效果）。
- **R7 引用签发**：read_citation_page 签发前重查 active scope（knowledge-scope.ts:126-155
  同链）；citation 由程序绑定冻结原文，模型只消费 citationMarkdown。
- **R8 grep**：literal 默认 + 受限 regex 子集（`.` `*` `+` `?` 锚点；不含完整正则面）
  —— TS `new RegExp(params.regex)` 全正则 → 候选 SimpleRegex。snippet 窗口 96/360
  同现役常数。
- **R9 settings 重建判定**：embeddingModelRef/chunkTargetChars 变化 → 全量重建入队
  （manager.ts:1800-1808 同判定）；rerank/topK/retention 不触发。
- **R10 TurnScope 冻结行合并**：同源多选中笔记本合并一行（store create_turn_scope，
  knowledge-store.ts:3313-3333 同语义；合并回写 notebook_ids_json 为实现细化）。

## F 项（RED→GREEN 修复留痕）

- **F-1** 块 id 以空占位 artifact id 派生 → 跨源 UNIQUE 碰撞（HTTP 套件首个
  import 500）。修复：解析器直收真实 artifact_id；目录导入路径同修。
  RED：t07run2/t07tools3 日志中 UNIQUE constraint failed: knowledge_blocks.id。
- **F-2** 摄入 worker 以 identity() 占位维度（0）建变体 → "Vector dimensions are
  invalid"。修复：首批探测嵌入以实测维度落变体身份（R2）。RED：t07run4 日志
  job failed error=Vector dimensions are invalid。
- **F-3** 查询侧 profileHash 硬编码 Markdown 策略 ≠ worker 按块推导（text/plain 源
  两侧不一致）→ 检索恒 INDEX_BUILDING。修复：scope 编译与 outline 均按块推导
  （resolve_chunker_config 同函数）。RED：a14 degraded=[KNOWLEDGE_INDEX_BUILDING]。
- **F-4** import 漏入队摄入（TS 路由 L417-429 的 enqueue 步骤初版遗漏）→ job 永不
  出现。修复：import_pasted_text/import_file/refresh 导入后 enqueue + wake。
- **F-5** turn scope 合并未回写 notebook_ids_json → 冻结行归属不可见（store 测试
  notebook_ids.len()==1）。修复：合并分支 UPDATE 冻结行。
- **F-6** cancelled job 可被 requeue（requeue 仅查 status=failed）→ delete wins 失效。
  修复：cancelled_at 非空即拒绝（KNOWLEDGE_CONFLICT）。
- **F-7** 嵌入响应列错位（FTS join SELECT 顺序 vs StoredChunk 映射）→ 检索 500。
  修复：SELECT 重排 + 专用行映射。
- **F-8** 测试期望自纠三条（token 估算 ceil(2.95)=3；块文本含句号；scope 冻结下
  新 scope 仍冻结同源——删除放行需换空笔记本 scope；manage/未知参数在 prepare 层
  拒绝而非 execute 层——schema 校验先行，分层与 TS 不同但同效果响亮）。

## 前序遗留义务（原样保留，不归本任务）

T03 R2-F-01~04、T04 F-01/F-02、T05 F-01/F-02、T06 OBS-R2-1（catalog 序 R07 核对）；
`.vscode/` 为 IDE 噪声，永不入候选。
