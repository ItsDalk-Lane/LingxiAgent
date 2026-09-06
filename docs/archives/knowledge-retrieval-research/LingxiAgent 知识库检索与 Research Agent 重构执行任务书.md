这里给出的技术路线、模块边界、接口、常量、数据表、测试和阶段顺序均已确定；执行过程中不得重新设计、缩减范围、合并阶段或自行替换核心技术方案。

本任务锁定仓库 `ItsDalk-Lane/LingxiAgent`，基线提交为：

```text
3eab85891a1747c64064252804f70c0a3773f021
```

当前 `main` 已核对为该提交。

当前实现中，`fast` 与 `detailed` 仍然通过同一个知识注入入口，在主会话 `promptSession` 开始之前同步完成检索和 `[KnowledgeContext]` 拼装；因此主 Agent 的正常工具循环没有参与前置调查。 当前快速模式仍允许查询嵌入等待 15 秒，条件性 rerank 再等待 5 秒；当前向量后端则会把作用域内全部向量读入 JavaScript、逐条计算余弦距离、整体排序后再取前几条。

仓库已经具备 `knowledge_outline`、`knowledge_read`、`knowledge_grep`、冻结的 `KnowledgeTurnScope` 和精确的 `CoverageUnit`，这些能力必须保留并接入新架构，不允许另起一套互不兼容的数据面。

---

# 一、任务最终目标

完成后，知识库问答必须形成两条完全不同的执行路径。

| 用户模式 | 内部路径 | 核心目标 | 停止条件 |
| ---- | ------------------- | ---------------------------- | ------------------ |
| 快速模式 | `fast_local` | 尽快找到少量高价值证据 | 本地时间预算或证据预算到达 |
| 详细模式 | `detailed_research` | 通过 Agent 多轮检索、阅读、核对和补证形成充分答案 | 必需证据需求全部完成，或研究预算耗尽 |

最终架构必须为：

```text
用户问题 + 已选笔记本
        │
        ▼
KnowledgeTurnScope 冻结
        │
        ▼
ScopeSnapshotCompiler
        │
        ├── fast ── FastKnowledgePipeline
        │             ├── 本地 FTS
        │             ├── 精确证据 span
        │             └── EvidencePacker
        │
        └── detailed ── KnowledgeResearchOrchestrator
                      ├── Evidence Ledger
                      ├── Research Agent
                      ├── 多次 knowledge_search
                      ├── 多次 knowledge_read / grep
                      ├── 并行 Research Workers
                      ├── 冲突检查与定向补证
                      └── Research Evidence Packet
                                    │
                                    ▼
                              主 Agent 最终回答
```

---

# 二、不可变更的架构决策

执行者必须遵守以下约束。

1. 前端继续只显示“快速”和“详细”两个模式，不增加第三种模式，不要求用户理解覆盖策略、完整性策略或检索通道。

2. 线上的 `KnowledgeReferenceMode` 继续使用：

```ts
type KnowledgeReferenceMode = "fast" | "detailed";
```

不得修改现有消息兼容规则；历史 `qa`、`assist` 仍按现有逻辑映射到 `detailed`。

1. 快速模式的关键路径禁止调用：

```text
查询嵌入模型
rerank 模型
knowledge 辅助模型
覆盖规划模型
拆解模型
扩展模型
Gap Analyzer
rollup 模型
Research Agent
Subagent
```

快速模式允许调用的远程模型数量必须恒为零。这里的“零”只指知识检索阶段，不包括之后负责回答用户的会话主模型。

1. 快速模式只使用本地 FTS、内存缓存和 SQLite 数据；不得以“配置了嵌入模型”为由进入混合检索。

2. 详细模式不得继续以“检索更多 chunk”代替详细调查。详细模式必须经过 Evidence Ledger，并允许 Agent 根据已读结果再次检索、再次阅读和再次核对。

3. 详细模式的研究控制器必须调用现有 Agent 隔离运行时，不得退化成在 `knowledge-context-injector.ts` 中连续调用若干次 `callText()`。

4. 搜索命中的 snippet 只是候选线索，不能直接成为最终引用证据。候选必须经过 `knowledge_read` 或 `knowledge_grep` 的实际原文读取和服务端校验，才能进入 Evidence Ledger。

5. 最终引用必须对应冻结的：

```text
contentSnapshotId
parseArtifactId
blockId
startOffset
endOffset
canonicalTextSha256
```

模型不得自行提交未经原文匹配的引用文字。

1. 所有知识工具继续受 `KnowledgeTurnScope` 约束。Research Agent、Research Worker 和普通 subagent 均不得读取所选笔记本之外的来源。

2. 快速模式和详细模式必须使用同一个 `ScopeSnapshotCompiler` 和同一个底层 `KnowledgeSearchService`，但采用不同执行策略。

3. 保留当前 FTS 索引和 portable vector BLOB 作为兼容及恢复数据。新增 HNSW 索引只能是可重建缓存，不能成为唯一向量真相源。

4. P1 固定使用 `usearch` `2.26.0` 构建本地 HNSW 后端，使用精确版本，不使用范围版本。该包的官方元数据声明 Node.js 22 及以上，并提供 JavaScript 原生绑定；仓库当前 Node 版本范围满足这一条件。([GitHub][1])

5. `usearch` 放入 `optionalDependencies`。加载失败时必须显式降级到 portable exact vector backend；禁止让应用无法启动，也禁止静默降级。

6. 详细模式中的“完整”必须通过 EvidenceNeed 和 CoverageUnit 证明，不能使用 `topK`、命中块数或 prompt 长度近似表示。

7. 不把模型的思维链、原始推理文本或隐藏 reasoning 写入知识数据库。数据库只保存结构化计划、查询、动作摘要、证据位置、状态和停止原因。

8. 所有失败和降级都必须进入 stats、研究运行状态或用户可见的未解决缺口，不得静默伪装成完整结果。

9. 不改变笔记本的现有语义：用户选择一个笔记本，就代表该笔记本中的全部来源构成本轮允许调查的知识范围；选择多个笔记本则取多个笔记本范围的并集。

10. 不增加网页搜索，不自动访问选中笔记本之外的来源，不修改任何用户原始资料。

---

# 三、执行纪律

## 3.1 分支与基线

执行以下命令：

```bash
git fetch origin
git checkout --detach 3eab85891a1747c64064252804f70c0a3773f021
git checkout -b feat/knowledge-retrieval-research-p0-p3
npm ci
```

不得直接在 `main` 上修改。

若该基线提交不存在或无法检出：

1. 创建 `BLOCKED.md`；
2. 写明基线不可用；
3. 停止执行；
4. 不得自行改用新的 `main`。

## 3.2 进度文件

新建：

```text
KNOWLEDGE_REFACTOR_PROGRESS.md
```

每个任务必须记录：

```text
任务编号
状态：pending / in_progress / completed / blocked
改动文件
测试命令
测试结果
对应 commit SHA
偏差：必须为 none；若非 none，任务不得标记 completed
```

每完成一个任务立即更新，不允许在全部完成后一次性补写。

## 3.3 阶段门禁

执行顺序固定为：

```text
P0 → P1 → P2 → P3
```

上一阶段所有测试未通过，不得进入下一阶段。

禁止：

* 跳过失败测试；
* 删除测试；
* 放宽断言；
* 增加无理由 sleep；
* 仅扩大 timeout 掩盖性能或死锁问题；
* 将失败测试标记为 skipped；
* 用 mock 绕过应当真实经过的生产链路；
* 用 TODO、占位返回值或假实现通过类型检查。

## 3.4 生成文件纪律

涉及持久化、运行时闭包和开放树时，必须调用仓库现有生成器，不得手工编辑派生结果。仓库现有生成器包括持久化指纹和 CLI 闭包生成器。

阶段末至少执行：

```bash
node scripts/generate-persistence-schema-fingerprint.mjs
node scripts/check-persistence-schema-fingerprint.mjs
node scripts/compute-cli-closure.mjs
node scripts/export-open-tree.mjs
node scripts/test-inventory.mjs
```

全部生成器连续运行两次。第二次运行后：

```bash
git diff --exit-code
```

必须无新增漂移。

---

# 四、统一契约

## 4.1 新建 `shared/knowledge-execution.ts`

实现以下类型：

```ts
import type { KnowledgeReferenceMode } from "./knowledge-refs.ts";

export type KnowledgeExecutionPath =
  | "fast_local"
  | "detailed_research";

export type KnowledgeCompletenessPolicy =
  | "best_effort"
  | "source_diverse"
  | "relevant_sections_complete"
  | "scope_complete";

export type KnowledgeResponseDetail =
  | "normal"
  | "detailed";

export interface KnowledgeExecutionPolicy {
  mode: KnowledgeReferenceMode;
  path: KnowledgeExecutionPath;
  completenessPolicy: KnowledgeCompletenessPolicy;
  responseDetail: KnowledgeResponseDetail;
  retrievalDeadlineMs: number | null;
}

export function resolveKnowledgeExecutionPolicy(input: {
  mode: KnowledgeReferenceMode;
  question: string;
  selectedNotebookCount: number;
  selectedSourceCount: number;
}): KnowledgeExecutionPolicy;
```

P0 到 P2 阶段先使用以下映射：

```text
fast:
  path = fast_local
  completenessPolicy = best_effort
  responseDetail = normal
  retrievalDeadlineMs = 1200

detailed:
  path = detailed_research
  completenessPolicy = source_diverse
  responseDetail = detailed
  retrievalDeadlineMs = null
```

P3 再加入问题驱动的完整性升级规则。

---

## 4.2 新建 `lib/knowledge/scope-snapshot-compiler.ts`

实现：

```ts
export interface CompiledKnowledgeSource {
  sourceId: string;
  sourceName: string;
  notebookIds: string[];

  contentSnapshotId: string;
  parseArtifactId: string | null;

  chunkProfileHash: string | null;
  chunkIndexVariantId: string | null;
  chunkCount: number;

  firstHeadingPath: string[] | null;
  sectionKeys: string[];

  status:
    | "ready"
    | "parse_pending"
    | "needs_ocr"
    | "index_missing"
    | "index_building"
    | "index_failed";
}

export interface CompiledKnowledgeNotebook {
  notebookId: string;
  notebookName: string;

  embeddingModelRef: {
    provider: string;
    id: string;
  } | null;

  rerankModelRef: {
    provider: string;
    id: string;
  } | null;

  chunkProfileHash: string | null;
  sourceIds: string[];
}

export interface CompiledKnowledgeScope {
  scopeId: string;
  turnId: string;
  sessionPath: string;
  studioId: string;

  notebookIds: string[];
  snapshotHash: string;

  notebooks: CompiledKnowledgeNotebook[];
  sources: CompiledKnowledgeSource[];

  readyChunkVariantIds: string[];
  warnings: string[];
}
```

`snapshotHash` 必须由以下内容经过稳定排序后做 SHA-256：

```text
scopeId
notebookIds
每个 source 的：
  sourceId
  contentSnapshotId
  parseArtifactId
  notebookIds
  chunkProfileHash
  chunkIndexVariantId
```

不得把时间戳、对象遍历顺序或模型凭证写入 hash。

---

## 4.3 新增精确证据 span 契约

在 `shared/knowledge-refs.ts` 或新文件 `shared/knowledge-evidence.ts` 中实现：

```ts
export interface KnowledgeEvidenceSpan {
  id: string;

  sourceId: string;
  sourceName: string;
  notebookIds: string[];

  contentSnapshotId: string;
  parseArtifactId: string;
  chunkIndexVariantId: string | null;
  chunkId: string | null;

  blockId: string;
  startOffset: number;
  endOffset: number;

  text: string;
  textSha256: string;

  headingPath: string[] | null;
  pageNumber: number | null;

  retrievalChannels: Array<"fts" | "vector" | "grep" | "ordinal_read">;
  score: number | null;
}
```

最终注入和 EvidenceManifest 统一以该结构为来源。

---

## 4.4 扩展 `KnowledgeRetrievalStats`

保留全部现有字段，新增以下可选字段：

```ts
executionPath?: "fast_local" | "detailed_research";

deadlineMs?: number;
deadlineExceeded?: boolean;

remoteModelCalls?: number;
ftsQueries?: number;
vectorQueries?: number;
rerankCalls?: number;

scopeCompileMs?: number;
timeToFirstEvidenceMs?: number;

vectorBackend?: "hnsw" | "portable" | "none";

searchCalls?: number;
readCalls?: number;
grepCalls?: number;

research?: {
  runId: string;
  status:
    | "planning"
    | "running"
    | "synthesizing"
    | "completed"
    | "partial"
    | "failed"
    | "cancelled";

  completenessPolicy: KnowledgeCompletenessPolicy;

  rounds: number;
  toolCalls: number;
  delegatedAgents: number;

  needsTotal: number;
  needsSupported: number;
  needsPartial: number;
  needsConflicted: number;

  unresolvedNeedIds: string[];
  stopReason: string;
};
```

快速模式必须写：

```text
executionPath = fast_local
remoteModelCalls = 0
vectorQueries = 0
rerankCalls = 0
```

---

# 五、P0：真正的快速检索路径

P0 的目标是先解决用户已经遇到的直接问题：快速模式必须成为低延迟、本地、可测量的路径。

P0 阶段不得改变详细模式现有生产行为。

---

## P0-00：建立基线与回归门禁

### 执行内容

1. 创建分支与进度文件。
2. 运行基线测试。
3. 保存测试结果到：

```text
KNOWLEDGE_REFACTOR_BASELINE.md
```

至少记录：

```text
基线 SHA
Node 版本
npm 版本
操作系统
typecheck 结果
knowledge 测试结果
全量测试结果
现有快速模式的典型分段耗时
```

### 执行命令

```bash
npm run typecheck
npx vitest run \
  tests/knowledge-context-injector.test.ts \
  tests/knowledge-rerank-fusion.test.ts \
  tests/knowledge-retrieval-golden.test.ts \
  tests/knowledge-query-config-alignment.test.ts \
  tests/knowledge-read-tool.test.ts
npm test
```

### 完成标准

* 基线测试结果已记录；
* 不修复与本任务无关的预存问题；
* 当前工作树除基线文档和进度文档外无改动。

### 提交

```text
docs(knowledge): establish P0-P3 refactor baseline
```

---

## P0-01：增加执行策略契约

### 改动文件

```text
新建 shared/knowledge-execution.ts
修改 shared/knowledge-refs.ts
修改 core/desktop-session-submit.ts
修改 core/engine.ts
```

### 执行内容

1. 增加前述 `KnowledgeExecutionPolicy`。
2. 保持前端和网络协议仍传 `fast | detailed`。
3. 在进入知识处理路径时调用：

```ts
resolveKnowledgeExecutionPolicy(...)
```

1. 当前阶段先只完成路由骨架：

```ts
switch (policy.path) {
  case "fast_local":
    // P0 新路径
    break;
  case "detailed_research":
    // P0 暂时转发到旧 detailed 路径
    break;
}
```

1. 禁止在组件或提交入口散落 `mode === "fast"` 判断。生产路由判断只能集中在执行策略解析和知识入口。

### 测试

新建：

```text
tests/knowledge-execution-policy.test.ts
```

覆盖：

* `fast` 映射到 `fast_local`；
* `detailed` 映射到 `detailed_research`；
* 快速期限为 1200ms；
* 详细模式没有检索期限；
* 历史模式归一规则未改变。

### 提交

```text
refactor(knowledge): introduce explicit fast and research execution policies
```

---

## P0-02：实现 ScopeSnapshotCompiler

### 改动文件

```text
新建 lib/knowledge/scope-snapshot-compiler.ts
修改 lib/knowledge/knowledge-store.ts
修改 lib/knowledge/knowledge-index-store.ts
修改 lib/knowledge/knowledge-manager.ts
修改 core/engine.ts
```

### 新增 Store API

在 `KnowledgeStore` 增加：

```ts
getNotebookRetrievalProfileSnapshot(input: {
  studioId: unknown;
  notebookId: unknown;
}): {
  notebookId: string;
  notebookName: string;
  chunkProfileHash: string | null;
  embeddingModelRef: KnowledgeModelRef | null;
  rerankModelRef: KnowledgeModelRef | null;
};
```

该方法必须通过：

```text
notebooks
→ retrieval_profiles
→ chunk_profiles
```

一次 JOIN 读取当前绑定，不得通过读取全部 blocks 重新计算 profile。

在 `KnowledgeIndexStore` 增加：

```ts
getReadyVariantMetadata(input: {
  parseArtifactId: unknown;
  chunkProfileHash: unknown;
}): {
  id: string;
  parseArtifactId: string;
  chunkProfileHash: string;
  blockFingerprint: string;
  chunkCount: number;
} | null;
```

`chunkCount` 使用 SQL `COUNT(*)`，不得调用 `listVariantChunks()`。

### 编译逻辑

1. 输入必须是已经创建的 `KnowledgeTurnScope`。
2. 以 scope 中的冻结 source 列表为唯一来源。
3. 对每个笔记本读取当前绑定的 `chunkProfileHash`。
4. 对每个 `(source, notebook profile)` 查找 ready variant。
5. 多个笔记本引用同一 source 且 profile 相同，只生成一个 ready variant。
6. profile 不同时保留不同 variant，但 source 元数据只保留一份。
7. 不得在快速路径调用：

```text
listArtifactBlocks
listVariantChunks
buildCoverageUnits
resolveKnowledgeChunkerConfig(blocks)
knowledgeBlockFingerprint(blocks)
```

1. 编译结果按 `scopeId` 缓存。
2. 同一 `scopeId` 的并发编译必须 single-flight，只允许执行一次底层读取。
3. scope 关闭、manager dispose 或 source 生命周期变化时清除缓存。

### 降级规则

* profile 未绑定：`index_missing`，调用既有后台 variant build 请求；
* parse artifact 未 ready：按状态写 warning；
* 部分 source 不可用时仍返回其他 ready source；
* 所有 source 不可用时返回空 ready variant 列表，不抛出伪内部错误。

### 测试

新建：

```text
tests/knowledge-scope-snapshot-compiler.test.ts
```

必须覆盖：

* 多笔记本同 profile 去重；
* 多笔记本不同 profile 保留；
* 冻结 snapshot 不被新 artifact 替换；
* 编译期间不调用 `listArtifactBlocks`；
* 编译期间不调用 `listVariantChunks`；
* single-flight；
* cache invalidation；
* 部分 source 不可用；
* snapshotHash 稳定；
* 输入顺序不同但语义相同时 hash 相同。

### 提交

```text
feat(knowledge): compile frozen search scopes without query-time full scans
```

---

## P0-03：实现 FastKnowledgePipeline

### 改动文件

```text
新建 lib/knowledge/fast-knowledge-pipeline.ts
修改 lib/knowledge/knowledge-query-service.ts
修改 lib/knowledge/knowledge-index-store.ts
修改 lib/knowledge/knowledge-manager.ts
```

### 固定常量

```ts
export const KNOWLEDGE_FAST_TOTAL_DEADLINE_MS = 1_200;
export const KNOWLEDGE_FAST_FTS_CANDIDATE_LIMIT = 24;
export const KNOWLEDGE_FAST_MAX_EVIDENCE_SPANS = 8;
export const KNOWLEDGE_FAST_PER_SPAN_MAX_TOKENS = 320;
export const KNOWLEDGE_FAST_RENDER_BUDGET_TOKENS = 2_400;
```

删除快速生产路径对以下旧常量的依赖：

```text
KNOWLEDGE_FAST_RERANK_DEADLINE_MS
KNOWLEDGE_FAST_MAX_EVIDENCE_ENTRIES
旧的 8192-token 快速注入预算
```

旧常量可以暂时保留给历史测试或兼容函数，但不得再控制生产快速路径。

### 新增 FTS API

在 `KnowledgeIndexStore` 增加：

```ts
searchReadyVariantIds(input: {
  chunkIndexVariantIds: string[];
  query: string;
  limit: number;
}): IndexedKnowledgeChunk[];
```

要求：

1. 输入直接使用编译好的 ready variant ID；
2. 继续使用现有 FTS5、BM25 和 `buildFtsLiteralQuery`；
3. 排序和 `LIMIT` 在 SQLite 内完成；
4. 不把全部候选读到 JavaScript 后再排序；
5. 输出稳定排序；
6. 同一 chunk 不重复。

### FastKnowledgePipeline 固定流程

```text
开始计时
→ 编译或读取 CompiledKnowledgeScope
→ 检查 deadline
→ 一次 FTS 查询
→ 检查 deadline
→ 提取精确 span
→ EvidencePacker
→ 渲染快速 KnowledgeContext
→ 返回
```

明确禁止调用：

```text
embedTextsForModel
vectorIndex.search
rerankForModel
planKnowledgeCoverage
decomposeQuestion
expandQueries
Gap Analyzer
runKnowledgeRollup
executeIsolated
```

### Deadline 语义

SQLite FTS 是同步调用，不能在 SQL 执行中途强行终止。因此 1200ms 是工作准入边界：

* 每个阶段开始前检查剩余时间；
* deadline 到达后不得开始新的阶段；
* 若单次 FTS 自身超过 deadline，仍返回已有结果，但写 `deadlineExceeded=true`；
* 不得在 deadline 后启动任何增强工作；
* 性能基准负责保证正常数据规模下 FTS 不越界。

### 快速模式无结果

若 FTS 返回零结果：

* 不启动向量搜索；
* 不调用模型改写查询；
* 返回明确的零结果 stats；
* 最终主模型收到“本地快速检索未找到匹配证据”的系统说明；
* 不得伪造普通聊天回答为知识库回答。

### 提交

```text
feat(knowledge): add deadline-bound local FTS fast path
```

---

## P0-04：实现精确 span 提取

### 改动文件

```text
新建 lib/knowledge/evidence-span-extractor.ts
修改 lib/knowledge/knowledge-store.ts
修改 lib/knowledge/knowledge-query-service.ts
```

### 新增 Store API

```ts
getArtifactBlocksByIds(input: {
  studioId: unknown;
  parseArtifactId: unknown;
  blockIds: unknown[];
}): KnowledgeBlock[];
```

必须单次批量查询，不允许每个 hit 单独查询形成 N+1。

### 提取算法

对每个 FTS hit：

1. 从 chunk 的 `spans` 收集相关 blockId。
2. 批量读取这些 blocks。
3. 使用现有统一 tokenizer 生成查询词。
4. 在原始 block 文本中寻找查询词覆盖密度最高的区域。
5. 以最高密度位置为中心，优先扩展到：

   * 同一段落边界；
   * 其次句号、问号、感叹号、分号和换行边界；
   * 最后才硬截断。
6. 每个 span 最多 320 estimated tokens。
7. 生成相对原始 block 的精确 `startOffset` 和 `endOffset`。
8. `text` 必须严格等于：

```ts
block.text.slice(startOffset, endOffset)
```

1. 计算 `textSha256`。
2. 附带 headingPath 和 pageNumber。
3. 同一 block 中重叠比例超过 60% 的 span 去重，只保留分数较高者。
4. 不得把 chunk 面包屑、合成标题或注入头误当成原始引用正文。

### 测试

新建：

```text
tests/knowledge-evidence-span-extractor.test.ts
```

覆盖：

* 中文词命中；
* 英文词命中；
* 数字和配置键；
* 多 block chunk；
* Unicode 代理对；
* 句子边界；
* 段落边界；
* 最大 token 预算；
* 重叠去重；
* 精确 offset；
* headingPath；
* PDF pageNumber。

### 提交

```text
feat(knowledge): extract compact canonical evidence spans for fast answers
```

---

## P0-05：实现统一 EvidencePacker

### 改动文件

```text
新建 lib/knowledge/evidence-packer.ts
修改 lib/knowledge/knowledge-context-injector.ts
```

### 固定选择规则

输入为排序后的 `KnowledgeEvidenceSpan[]`。

选择顺序：

1. 第一轮按来源多样性，每个命中来源最多先选一条；
2. 第二轮按分数补齐；
3. 单一来源最多 3 条，除非所选 scope 只有一个 ready source；
4. 最大 8 条；
5. 总预算最大 2400 tokens；
6. 每条必须完整保留来源头和引用位置；
7. 如果第一条证据本身超过预算，裁成不超过 320 tokens 的合法子 span，而不是整体丢弃；
8. 不允许在字符中间破坏代理对；
9. 最终编号必须连续：

```text
[K1] [K2] ... [Kn]
```

### 注入格式

```text
[KnowledgeContext]
Mode: fast
Execution path: local FTS
Scope: <scopeId>
Retrieval deadline: <deadlineMs>ms
Deadline exceeded: yes|no

[K1]
Source: ...
Location: heading/page/block offsets
Evidence:
...

Instructions:
- Answer only from the evidence above.
- Cite evidence ids.
- If the evidence is insufficient, say so explicitly.
```

### 兼容要求

* 保留历史 `[KnowledgeContext]` 压缩逻辑；
* 新 span 能进入现有 EvidenceManifest；
* 历史消息读取不报错；
* `results` 继续为前端提供来源名和预览。

### 测试

新建：

```text
tests/knowledge-evidence-packer.test.ts
```

覆盖：

* 来源多样性；
* 单来源上限；
* token 上限；
* oversized first span；
* 稳定顺序；
* 编号；
* 精确 manifest 映射。

### 提交

```text
refactor(knowledge): centralize evidence packing around canonical spans
```

---

## P0-06：接入会话提交链路

### 改动文件

```text
修改 core/engine.ts
修改 core/desktop-session-submit.ts
修改 lib/knowledge/knowledge-manager.ts
```

### 新增 Engine 方法

```ts
async buildFastKnowledgeContext(input: {
  question: string;
  knowledgeRefs: {
    notebookIds: string[];
    mode: "fast";
  };
  sessionPath: string;
  turnId?: string | null;
  signal?: AbortSignal;
}): Promise<{
  block: string;
  stats: KnowledgeRetrievalStats;
  evidence: KnowledgeInjectionEvidence;
}>;
```

### 路由

普通 submit 和 interjection 两条入口都必须使用相同路由：

```ts
const policy = resolveKnowledgeExecutionPolicy(...);

if (policy.path === "fast_local") {
  return engine.buildFastKnowledgeContext(...);
}

return engine.buildKnowledgeContextInjection(...); // P0 暂保留旧 detailed
```

### 要求

* 快速路径不得进入旧 injector 的拆解与 hybrid 逻辑；
* abort signal 必须传入；
* 用户在检索阶段点击停止时：

  * 停止后续 span/packer 工作；
  * 不进入 `promptSession`；
  * 不产生用户消息投影；
  * 收回 busy 状态；
* 保持现有 EvidenceManifest 持久化时序；
* fast stats 的 `remoteModelCalls` 必须由生产代码写死为 0，而不是依赖调用计数推测。

### 测试

新建：

```text
tests/desktop-session-submit-knowledge-routing.test.ts
tests/knowledge-fast-zero-remote.test.ts
```

必须用会抛错的 fake 函数注入：

```text
embedTextsForModel
rerankForModel
knowledge model
coverage planner
rollup model
executeIsolated
```

快速模式仍必须成功，以证明没有调用这些函数。

详细模式在 P0 必须继续经过旧入口。

### 提交

```text
feat(knowledge): route fast references through the local-only pipeline
```

---

## P0-07：补充观测和前端文案

### 改动文件

```text
修改 shared/knowledge-refs.ts
修改 desktop/src/react/services/ws-message-handler.ts
修改知识检索折叠组件
修改 desktop/src/react/components/input/KnowledgeReferenceBar.tsx
修改五份 locale：
  zh.json
  zh-TW.json
  en.json
  ja.json
  ko.json
```

### 前端文案

快速提示改为：

```text
纯本地快速检索；不等待远程嵌入、重排或多轮调查。
```

详细提示暂改为：

```text
进行多轮检索、阅读和证据核对后生成详细回答。
```

### 快速过程卡

最多显示：

```text
正在本地检索知识库
已找到 N 条证据 · Xms
正在生成回答
```

不得显示：

```text
正在拆解问题
正在调用重排
正在滚动阅读
正在补充检索
```

### 结果摘要

```text
快速检索 · N 条证据 · Xms
```

若 deadline 超过：

```text
快速检索 · N 条证据 · Xms · 已超出目标时限
```

### 测试

更新或新增前端组件测试，覆盖：

* fast 标签；
* detailed 标签；
* 快速过程卡；
* deadlineExceeded 展示；
* 旧消息无新 stats 时仍正常渲染；
* 五语言键完整。

### 提交

```text
feat(ui): expose local fast retrieval timing and execution path
```

---

## P0-08：性能基准

### 新建文件

```text
scripts/benchmark-knowledge-fast.mjs
tests/knowledge-fast-performance-contract.test.ts
.github/workflows/knowledge-performance.yml
```

### 基准数据集

生成三个固定种子规模：

```text
10,000 chunks
100,000 chunks
可选 1,000,000 chunks，仅手工 benchmark
```

语料必须包含：

* 中文；
* 英文；
* 数字；
* 文件名；
* 标题；
* 低频专有名词；
* 大量无关干扰块。

### 测量项目

```text
scopeCompileMs
ftsMs
spanExtractMs
packMs
totalMs
P50
P95
P99
remoteModelCalls
returnedSpans
usedTokens
```

### 门禁

普通 `npm test` 只检查确定性契约，不检查墙钟。

以下命令启用性能硬门禁：

```bash
LINGXI_ENFORCE_KNOWLEDGE_PERF=1 \
node scripts/benchmark-knowledge-fast.mjs
```

参考机器门禁：

```text
10k chunks，热缓存 P95 ≤ 800ms
100k chunks，热缓存 P95 ≤ 1200ms
100k chunks，冷启动 P95 ≤ 1500ms
remoteModelCalls = 0
final spans ≤ 8
usedTokens ≤ 2400
```

CI workflow：

* 支持 `workflow_dispatch`；
* 只在稳定 Linux runner 上启用墙钟门禁；
* 上传 JSON 结果；
* 普通 PR 不因共享 runner 抖动而使用墙钟断言。

### P0 阶段门禁

```bash
npm run typecheck
npm run lint
npm run lint:boundary

npx vitest run \
  tests/knowledge-execution-policy.test.ts \
  tests/knowledge-scope-snapshot-compiler.test.ts \
  tests/knowledge-fast-pipeline.test.ts \
  tests/knowledge-fast-zero-remote.test.ts \
  tests/knowledge-evidence-span-extractor.test.ts \
  tests/knowledge-evidence-packer.test.ts \
  tests/desktop-session-submit-knowledge-routing.test.ts \
  tests/knowledge-retrieval-golden.test.ts

npm test
npm run build:server
npm run build:server:open
npm run build:client
```

P0 完成后必须确认：

```text
fast 的远程模型调用数恒为 0
fast 不进入旧 hybrid 路径
fast 最终证据 ≤ 8
fast 最终证据 ≤ 2400 tokens
detailed 旧行为未变
```

阶段提交：

```text
chore(knowledge): close P0 fast-path verification
```

---

# 六、P1：统一检索数据面、缓存与 HNSW

P1 的目标是让自动检索、Agent 搜索和后续 Research Agent 共用同一个低成本检索服务，同时淘汰生产路径中的 JavaScript 全量向量排序。

---

## P1-01：为索引增加查询元数据

### 改动文件

```text
修改 lib/knowledge/knowledge-index-store.ts
修改 lib/knowledge/ingestion-service.ts
修改 lib/knowledge/knowledge-query-service.ts
修改 lib/knowledge/knowledge-manager.ts
```

### 索引数据库版本

当前 `knowledge-fts.db` schema 从 v2 升至 v3。

新增表：

```sql
CREATE TABLE chunk_index_variant_metadata (
  chunk_index_variant_id TEXT PRIMARY KEY,
  parse_artifact_id TEXT NOT NULL,
  chunk_count INTEGER NOT NULL,
  first_heading_path_json TEXT,
  section_keys_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

增加索引：

```sql
CREATE INDEX idx_chunk_variant_metadata_artifact
ON chunk_index_variant_metadata(parse_artifact_id);
```

### 写入时机

`indexArtifactForIngestion()` 完成 chunk/FTS 原子替换后，在同一变体完成流程中写入：

* chunkCount；
* 首个有效 headingPath；
* distinct sectionKeys；
* parseArtifactId；
* 更新时间。

### 现有索引回填

应用启动后：

1. 扫描 ready variant；
2. 找出缺少 metadata 的变体；
3. 以低优先级后台回填；
4. 每批最多 20 个变体；
5. 不阻塞启动；
6. 不在查询线程现场回填。

### 查询要求

`ScopeSnapshotCompiler` 优先读取 metadata。metadata 缺失时：

* fast 仍可以使用 ready variant 做 FTS；
* chunkCount 可用 SQL `COUNT(*)`；
* sectionKeys 留空并标记 warning；
* 禁止读取全部 blocks 现场恢复 sectionKeys。

### 测试

```text
tests/knowledge-index-metadata-migration.test.ts
tests/knowledge-index-metadata-backfill.test.ts
tests/knowledge-scope-metadata-query.test.ts
```

覆盖：

* v2 → v3 迁移；
* 数据不丢失；
* 新 ingestion 写 metadata；
* 后台回填幂等；
* 查询不触发回填；
* metadata 损坏显式报错或重建。

### 提交

```text
feat(knowledge): persist query-ready chunk variant metadata
```

---

## P1-02：建立 KnowledgeSearchService

### 新建文件

```text
lib/knowledge/knowledge-search-service.ts
lib/knowledge/query-embedding-cache.ts
lib/knowledge/retrieval-result-cache.ts
```

### 核心 API

```ts
export interface KnowledgeSearchRequest {
  compiledScope: CompiledKnowledgeScope;
  query: string;

  channel: "fts" | "hybrid";
  limit: number;

  notebookIds?: string[];
  sourceIds?: string[];
  sectionKeys?: string[];

  rerank: boolean;
  signal?: AbortSignal;
}

export interface KnowledgeSearchHit {
  candidateId: string;

  sourceId: string;
  sourceName: string;
  notebookIds: string[];

  contentSnapshotId: string;
  parseArtifactId: string;
  chunkIndexVariantId: string;
  chunkId: string;
  chunkOrdinal: number;

  headingPath: string[] | null;
  pageNumber: number | null;

  snippet: string;
  score: number;
  channels: Array<"fts" | "vector">;
}

export interface KnowledgeSearchResponse {
  hits: KnowledgeSearchHit[];

  retrievalMode: "fts" | "hybrid";
  vectorBackend: "hnsw" | "portable" | "none";

  timings: {
    scopeMs: number;
    ftsMs: number;
    embedMs?: number;
    vectorMs?: number;
    fuseMs: number;
    rerankMs?: number;
    totalMs: number;
  };

  remoteModelCalls: number;
  degradedReasons: string[];
}
```

### 行为

* fast 使用 `channel="fts"`、`rerank=false`；
* detailed Research Agent 默认使用 `channel="hybrid"`；
* 自动 injector 和 Agent 工具不得各自实现不同搜索算法；
* 所有过滤条件必须先经过 scope 校验；
* 任意请求都不能扩大 CompiledKnowledgeScope。

### 提交

```text
refactor(knowledge): centralize retrieval behind KnowledgeSearchService
```

---

## P1-03：查询嵌入分组与缓存

### 固定缓存

`QueryEmbeddingCache`：

```text
最大条目：512
TTL：10 分钟
LRU
single-flight
```

缓存键：

```text
normalizedQuery
configured embedding provider
configured embedding model id
model configuration revision
inputType=query
```

模型配置变化时清空对应条目。

`RetrievalResultCache`：

```text
最大条目：256
TTL：2 分钟
LRU
single-flight
```

缓存键：

```text
scopeSnapshotHash
normalizedQuery
channel
filters
limit
rerank policy
retrieval implementation version
```

### 分组规则

对选中笔记本按嵌入模型引用分组：

```text
(provider, model id, model config revision)
```

同组笔记本中的同一 query：

* 只调用一次查询嵌入；
* 搜索该组下全部 vector variant；
* 不再每个笔记本单独嵌入相同问题。

### 强制测试

选中 5 个使用相同嵌入模型的笔记本：

```text
query embedding calls = 1
```

选中两种嵌入模型：

```text
query embedding calls = 2
```

并发发起相同搜索：

```text
底层嵌入执行 = 1
```

### 测试

```text
tests/knowledge-query-embedding-cache.test.ts
tests/knowledge-search-model-grouping.test.ts
tests/knowledge-retrieval-result-cache.test.ts
```

### 提交

```text
perf(knowledge): deduplicate query embeddings across notebooks and requests
```

---

## P1-04：全局融合和分组 rerank

### 执行逻辑

1. FTS 对全部符合范围的 variant 一次查询。
2. 向量通道按嵌入模型组执行。
3. 同一 chunk 的 FTS 和 vector 名次通过现有 RRF 规则融合。
4. 不再先按 notebook 独立 rerank 再跨 notebook 融合。

### Rerank 规则

* 所有选中笔记本未配置 rerank：不调用。
* 全部候选使用同一个 rerank ref：对全局融合后的前 50 条调用一次。
* 存在多个 rerank ref：

  * 按 rerank ref 分组；
  * 每个 distinct ref 最多调用一次；
  * 每组结果只消费名次；
  * 最后通过 RRF 融合；
  * 不比较跨 rerank 模型的 raw score。
* 相同 rerank ref 的多个笔记本不得重复调用。
* 远程 rerank 期限继续有界，失败保留 RRF 顺序并显式记录。

### 新增 stats

```text
embeddingGroups
rerankGroups
queryEmbeddingCacheHit
retrievalResultCacheHit
```

### 测试

```text
tests/knowledge-global-rerank.test.ts
tests/knowledge-mixed-rerank-groups.test.ts
tests/knowledge-cross-notebook-fusion.test.ts
```

### 提交

```text
perf(knowledge): fuse globally and rerank once per model group
```

---

## P1-05：新增 HNSW 向量后端

### 依赖

修改：

```text
package.json
package-lock.json
```

加入：

```json
{
  "optionalDependencies": {
    "usearch": "2.26.0"
  }
}
```

不得使用：

```text
^2.26.0
~2.26.0
latest
```

### 新建文件

```text
lib/knowledge/vector-search-backend.ts
lib/knowledge/ann-index-store.ts
lib/knowledge/usearch-vector-backend.ts
lib/knowledge/vector-search-backend-factory.ts
```

### 后端抽象

```ts
export interface KnowledgeVectorSearchBackend {
  readonly kind: "hnsw" | "portable";

  search(input: {
    vectorIndexVariantIds: string[];
    model: VectorIndexModelIdentity;
    queryVector: number[];
    limit: number;
  }): Promise<VectorSearchResult[]>;

  scheduleBuild(vectorIndexVariantId: string): void;
  invalidate(vectorIndexVariantId: string): void;
  close(): Promise<void>;
}
```

### 数据真相

继续保留：

```text
knowledge-vector.db
chunk_vectors BLOB
vector_index_variants
```

它们仍是：

* checkpoint；
* 恢复来源；
* portable fallback；
* HNSW 重建来源。

### 新增 ANN 存储

新增：

```text
knowledge-ann.db
knowledge-ann/
```

`knowledge-ann.db` schema v1：

```sql
CREATE TABLE ann_variants (
  vector_index_variant_id TEXT PRIMARY KEY,
  model_key TEXT NOT NULL,
  dimensions INTEGER NOT NULL,
  chunk_fingerprint TEXT NOT NULL,
  vector_count INTEGER NOT NULL,
  index_format_version INTEGER NOT NULL,
  file_name TEXT NOT NULL UNIQUE,
  status TEXT NOT NULL CHECK(
    status IN ('building', 'ready', 'failed')
  ),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

### 索引文件身份

一个 `VectorIndexVariant` 对应一个 HNSW 文件：

```text
knowledge-ann/<model-key-prefix>/<vector-index-variant-id>.usearch
```

HNSW key 固定使用：

```text
chunk ordinal + 1
```

因为每个文件只对应一个 vector variant，ordinal 在文件内唯一。

### HNSW 参数

```text
metric = cosine
dtype = f32
connectivity = 16
expansion_add = 128
expansion_search = 64
```

不得在首版使用量化，避免引入额外召回损失。

### 构建时机

1. portable vector variant 完整置为 `ready` 后，异步调度 HNSW 构建。
2. 从 portable BLOB 批量读取全部 ready vectors。
3. 创建临时文件：

```text
<variant>.usearch.tmp
```

1. 构建并 fsync。
2. 原子 rename 到正式文件。
3. 最后将 `ann_variants.status` 置为 `ready`。
4. 进程中断时：

   * 临时文件下次启动清理；
   * portable vectors 不删除；
   * ANN 状态重新构建。

### 查询

1. 对每个所选 vector variant 加载对应 index；
2. 每个 variant 取本轮 `limit` 个近邻；
3. 合并全部 variant 结果；
4. 按 cosine score 稳定排序；
5. 取全局 limit；
6. loaded index 使用 LRU：

   * 最多 32 个 index；
   * 或估算内存达到 512MB 时淘汰；
   * 两者任一达到即淘汰最旧项。

### 降级

以下情况使用 portable exact backend：

```text
usearch import 失败
native addon 加载失败
ANN 文件缺失
ANN 指纹不匹配
ANN vector count 不匹配
ANN 文件损坏
ANN 查询失败
```

同时：

* 记录 `vectorBackend="portable"`；
* 写明确 degraded reason；
* 调度 ANN 重建；
* 不阻断搜索。

### 禁止行为

* 不得删除 portable vector BLOB；
* 不得在查询线程同步重建完整 HNSW；
* 不得静默回退；
* 不得因为 ANN 失败让应用启动失败；
* 不得用 HNSW 文件作为唯一可恢复向量数据。

### 测试

```text
tests/knowledge-ann-index-store.test.ts
tests/knowledge-usearch-backend.test.ts
tests/knowledge-vector-backend-fallback.test.ts
tests/knowledge-ann-rebuild.test.ts
tests/knowledge-ann-crash-recovery.test.ts
tests/knowledge-vector-backend-parity.test.ts
```

召回门禁：

```text
确定性测试集 top-k overlap ≥ 95%
知识 golden set 预期来源召回率 = 100%
```

### 提交

```text
feat(knowledge): add rebuildable HNSW vector search with portable fallback
```

---

## P1-06：新增第一等 `knowledge_search` 工具

### 新建文件

```text
lib/tools/knowledge-search-tool.ts
```

### 工具参数

```ts
Type.Object({
  scopeId: Type.String(),
  query: Type.String(),

  channel: Type.Optional(
    Type.Union([
      Type.Literal("fts"),
      Type.Literal("hybrid"),
    ])
  ),

  notebookIds: Type.Optional(Type.Array(Type.String())),
  sourceIds: Type.Optional(Type.Array(Type.String())),
  sectionKeys: Type.Optional(Type.Array(Type.String())),

  limit: Type.Optional(Type.Number()),
});
```

### 参数边界

```text
query：1～4000 字符
limit：1～24，默认 12
notebookIds：必须全部属于 scope
sourceIds：必须全部属于 scope
sectionKeys：只允许 scope metadata 已知 section
channel 默认 hybrid
```

### 返回格式

```json
{
  "scopeId": "...",
  "query": "...",
  "mode": "hybrid",
  "vectorBackend": "hnsw",
  "hits": [
    {
      "candidateId": "...",
      "sourceId": "...",
      "sourceName": "...",
      "notebookIds": ["..."],
      "parseArtifactId": "...",
      "chunkId": "...",
      "chunkOrdinal": 12,
      "headingPath": ["...", "..."],
      "pageNumber": null,
      "snippet": "...",
      "score": 0.91,
      "channels": ["fts", "vector"]
    }
  ],
  "degradedReasons": []
}
```

### 关键约束

* `snippet` 只是定位提示；
* 返回内容必须明确写明“必须调用 knowledge_read 或 knowledge_grep 后才能引用”；
* `candidateId` 不得被视为证据 ID；
* 工具只读；
* 不得允许普通模型传入 studioId；
* studioId 从 runtime context 获取；
* scope 校验复用 `knowledge-scope.ts`。

### Agent 接线

修改：

```text
core/agent.ts
desktop/src/react/utils/tool-label.ts
五份 locale
tests/tool-label-coverage.test.ts
```

把 `knowledge_search` 加入普通主 Agent 和 Research Agent 的只读工具集。

### 测试

```text
tests/knowledge-search-tool.test.ts
tests/knowledge-search-tool-scope.test.ts
tests/knowledge-search-tool-output-budget.test.ts
```

### 提交

```text
feat(agent): expose bounded scope-safe knowledge search
```

---

## P1-07：让现有工具复用统一数据面

### 修改文件

```text
lib/tools/knowledge-read-tool.ts
lib/tools/knowledge-outline-tool.ts
lib/tools/knowledge-grep-tool.ts
core/agent.ts
```

### `knowledge_read`

当传入 `query` 时：

* 改用 `KnowledgeSearchService`；
* 固定 sourceId 过滤；
* 默认 limit 12；
* 不再另行构造一套 retrieveForArtifacts hybrid 流程。

按 ordinal 范围读取时保留现有行为。

### `knowledge_outline`

优先使用 `CompiledKnowledgeScope` 和索引 metadata：

* notebook；
* source；
* chunk count；
* heading summary；
* section keys；
* fidelity/status。

普通 outline 调用不得重新构建 CoverageUnit。

P3 的完整性执行可以显式构建 CoverageUnit，但不允许普通 outline 每次都做。

### `knowledge_grep`

保留原文确定性扫描。增加：

* 已扫描字符数；
* 匹配 source 数；
* 精确 block offsets；
* 后续 P2 receipt 接口的扩展点。

### 测试

更新既有工具测试，确保：

* scope 边界不变；
* source 越权继续 fail-closed；
* subagent scope 继承不变；
* query 模式确实经过 `KnowledgeSearchService`；
* outline 不构建 CoverageUnit；
* grep 结果仍为原文精确匹配。

### 提交

```text
refactor(agent): share compiled scope and search data across knowledge tools
```

---

## P1-08：HNSW 打包与性能验证

### 修改文件

```text
scripts/build-server-deps.mjs
scripts/build-server-artifact.mjs
scripts/build-server-runtime-assets.mjs
scripts/build-standalone-server-artifact.mjs
scripts/verify-standalone-server-artifact.mjs
scripts/verify-seed-kit.mjs
scripts/smoke-packaged-knowledge.mjs
export-manifest.json 的生成输入
build/cli-runtime-closure.json 的生成输入
electron-builder 配置
```

### 要求

1. `usearch` native addon 在以下目标可打包：

   * macOS x64；
   * macOS arm64；
   * Windows x64；
   * Linux x64。
2. Electron asar 中 native 文件必须正确 unpack。
3. standalone server artifact 必须包含对应目标 native addon。
4. portable fallback 在故意移除 native addon 后仍能启动和检索。
5. `smoke-packaged-knowledge.mjs` 增加：

   * native backend load；
   * HNSW 创建；
   * HNSW 查询；
   * portable fallback；
   * 输出 backend 类型。

### 基准脚本

新建：

```text
scripts/benchmark-knowledge-vector.mjs
```

测量：

```text
10k / 100k vectors
portable exact P50/P95
HNSW P50/P95
top-k overlap
index build time
index file size
cold load time
warm search time
```

参考门禁：

```text
100k vectors：
  HNSW P95 ≤ 500ms
  HNSW 至少比 portable exact 快 5 倍
  top-k overlap ≥ 95%
```

墙钟门禁只在稳定 benchmark runner 启用。

### P1 阶段门禁

```bash
npm run typecheck
npm run lint
npm run lint:boundary

npx vitest run \
  tests/knowledge-index-metadata-migration.test.ts \
  tests/knowledge-search-model-grouping.test.ts \
  tests/knowledge-query-embedding-cache.test.ts \
  tests/knowledge-global-rerank.test.ts \
  tests/knowledge-usearch-backend.test.ts \
  tests/knowledge-vector-backend-fallback.test.ts \
  tests/knowledge-search-tool.test.ts \
  tests/knowledge-read-tool.test.ts

npm test
npm run build:server
npm run build:server:open
npm run build:client
npm run test:knowledge-platform-smoke
node scripts/smoke-packaged-knowledge.mjs
```

P1 完成后必须确认：

```text
同模型多笔记本只嵌入一次 query
同 rerank 模型只 rerank 一次
Agent 拥有跨 scope 的 knowledge_search
生产向量路径优先 HNSW
HNSW 不可用时 portable fallback 生效
快速模式仍不使用任何向量路径
```

阶段提交：

```text
chore(knowledge): close P1 unified retrieval verification
```

---

# 七、P2：将详细模式改造成 Research Agent

P2 是本任务的核心。完成后，详细模式不再走旧的“一次性批量检索后注入”流程。

---

## P2-01：增加 Research 共享契约

### 新建文件

```text
shared/knowledge-research.ts
```

### 类型

```ts
export type KnowledgeResearchRunStatus =
  | "planning"
  | "running"
  | "synthesizing"
  | "completed"
  | "partial"
  | "failed"
  | "cancelled";

export type KnowledgeEvidenceNeedKind =
  | "fact"
  | "comparison"
  | "cause"
  | "timeline"
  | "counterexample"
  | "completeness";

export type KnowledgeEvidenceNeedStatus =
  | "uncovered"
  | "partial"
  | "supported"
  | "conflicted"
  | "not_applicable";

export type KnowledgeEvidenceRelation =
  | "supports"
  | "contradicts"
  | "context";

export interface KnowledgeEvidenceNeed {
  id: string;
  ordinal: number;

  claim: string;
  kind: KnowledgeEvidenceNeedKind;
  required: boolean;

  minIndependentSources: number;
  requireCounterEvidence: boolean;
  requireAllRelevantUnits: boolean;

  status: KnowledgeEvidenceNeedStatus;

  evidenceIds: string[];
  counterEvidenceIds: string[];
  unresolvedGaps: string[];
}

export interface KnowledgeResearchBudget {
  maxRounds: number;
  maxParallelAgents: number;
  maxToolCalls: number;
  maxWallClockMs: number;
  maxSearchesPerRound: number;
  maxReadsPerRound: number;
  maxFinalEvidenceSpans: number;
  finalEvidenceBudgetTokens: number;
}
```

### 固定默认预算

```ts
export const DEFAULT_KNOWLEDGE_RESEARCH_BUDGET = {
  maxRounds: 4,
  maxParallelAgents: 4,
  maxToolCalls: 32,
  maxWallClockMs: 180_000,
  maxSearchesPerRound: 8,
  maxReadsPerRound: 12,
  maxFinalEvidenceSpans: 32,
  finalEvidenceBudgetTokens: 16_000,
} as const;
```

### 提交

```text
feat(knowledge): define evidence-ledger research contracts
```

---

## P2-02：Knowledge 数据库升级到 v18

当前 Knowledge 主数据库 schema 为 v17。

### 修改文件

```text
lib/knowledge/knowledge-store.ts
lib/knowledge/types.ts
shared/persistence/store-registry.ts
持久化指纹相关测试
```

### 新增表

#### `knowledge_research_runs`

```text
id TEXT PRIMARY KEY
turn_scope_id TEXT NOT NULL
turn_id TEXT NOT NULL
parent_session_path TEXT NOT NULL
question TEXT NOT NULL
status TEXT NOT NULL
completeness_policy TEXT NOT NULL
budget_json TEXT NOT NULL
rounds_completed INTEGER NOT NULL DEFAULT 0
tool_calls_used INTEGER NOT NULL DEFAULT 0
search_calls INTEGER NOT NULL DEFAULT 0
read_calls INTEGER NOT NULL DEFAULT 0
grep_calls INTEGER NOT NULL DEFAULT 0
delegated_agents INTEGER NOT NULL DEFAULT 0
stop_reason TEXT
degraded_reason TEXT
created_at TEXT NOT NULL
updated_at TEXT NOT NULL
completed_at TEXT
```

#### `knowledge_evidence_needs`

```text
id TEXT PRIMARY KEY
run_id TEXT NOT NULL
ordinal INTEGER NOT NULL
claim TEXT NOT NULL
kind TEXT NOT NULL
required INTEGER NOT NULL
min_independent_sources INTEGER NOT NULL
require_counter_evidence INTEGER NOT NULL
require_all_relevant_units INTEGER NOT NULL
status TEXT NOT NULL
unresolved_gaps_json TEXT NOT NULL
created_at TEXT NOT NULL
updated_at TEXT NOT NULL
UNIQUE(run_id, ordinal)
```

#### `knowledge_research_rounds`

```text
id TEXT PRIMARY KEY
run_id TEXT NOT NULL
ordinal INTEGER NOT NULL
focus_json TEXT NOT NULL
status TEXT NOT NULL
new_evidence_count INTEGER NOT NULL DEFAULT 0
started_at TEXT NOT NULL
completed_at TEXT
error_code TEXT
UNIQUE(run_id, ordinal)
```

#### `knowledge_research_read_receipts`

```text
id TEXT PRIMARY KEY
run_id TEXT NOT NULL
actor_session_id TEXT
source_id TEXT NOT NULL
content_snapshot_id TEXT NOT NULL
parse_artifact_id TEXT NOT NULL
chunk_index_variant_id TEXT
chunk_id TEXT
block_id TEXT NOT NULL
start_offset INTEGER NOT NULL
end_offset INTEGER NOT NULL
canonical_text_sha256 TEXT NOT NULL
channel TEXT NOT NULL
created_at TEXT NOT NULL
consumed_at TEXT
```

Receipt 只存位置和 hash，不复制整个 source 文本。

#### `knowledge_evidence_items`

```text
id TEXT PRIMARY KEY
run_id TEXT NOT NULL
source_id TEXT NOT NULL
content_snapshot_id TEXT NOT NULL
parse_artifact_id TEXT NOT NULL
chunk_index_variant_id TEXT
chunk_id TEXT
block_id TEXT NOT NULL
start_offset INTEGER NOT NULL
end_offset INTEGER NOT NULL
canonical_text TEXT NOT NULL
canonical_text_sha256 TEXT NOT NULL
heading_path_json TEXT
page_number INTEGER
created_at TEXT NOT NULL
UNIQUE(
  run_id,
  parse_artifact_id,
  block_id,
  start_offset,
  end_offset
)
```

#### `knowledge_need_evidence`

```text
need_id TEXT NOT NULL
evidence_id TEXT NOT NULL
relation TEXT NOT NULL
rationale TEXT NOT NULL
source_independence_key TEXT NOT NULL
created_at TEXT NOT NULL
PRIMARY KEY(need_id, evidence_id, relation)
```

`source_independence_key` 固定使用 `sourceId`，同一来源中的不同 chunk 不算独立来源。

#### `knowledge_research_actions`

```text
id TEXT PRIMARY KEY
run_id TEXT NOT NULL
round_id TEXT
ordinal INTEGER NOT NULL
actor_session_id TEXT
actor_agent_id TEXT
action_type TEXT NOT NULL
request_summary_json TEXT NOT NULL
response_summary_json TEXT
status TEXT NOT NULL
started_at TEXT NOT NULL
completed_at TEXT
error_code TEXT
UNIQUE(run_id, ordinal)
```

### 数据纪律

禁止把以下内容写入上述表：

```text
完整 system prompt
完整 user prompt
模型原始回答
隐藏思考
reasoning tokens
未裁剪的工具全文结果
```

`request_summary_json` 只保存：

* query；
* source filter；
* needIds；
  -工具参数；
  -不含知识正文。

`response_summary_json` 只保存：

* hit IDs；
* receipt IDs；
  -计数；
  -状态；
  -错误码。

### 迁移

实现：

```text
v17 → v18 单事务迁移
createSchemaV18 新库建表
user_version = 18
```

### 测试

```text
tests/knowledge-store-v18-migration.test.ts
tests/knowledge-research-store.test.ts
tests/persistence-schema-tripwire.test.ts
```

覆盖：

* v17 数据完整迁移；
* 事务中断回滚；
* 所有 CHECK；
* 外键；
* unique；
* 非法状态拒绝；
* 不允许存入非法 JSON；
* 老数据库仍可打开。

### 提交

```text
feat(knowledge): persist bounded research runs and evidence ledgers
```

---

## P2-03：实现 Evidence Ledger

### 新建文件

```text
lib/knowledge/research/evidence-ledger.ts
lib/knowledge/research/research-store.ts
lib/knowledge/research/research-stop-policy.ts
lib/knowledge/evidence-receipt-service.ts
```

### Need 状态必须由宿主重算

模型不得直接指定最终 status。

重算规则：

```text
没有支持证据：
  uncovered

有支持证据，但独立来源数不足：
  partial

支持证据达到 minIndependentSources，
且需要的反证检查已完成，
且没有未处理矛盾：
  supported

同时存在 supports 和 contradicts：
  conflicted

宿主接受明确“不适用”结论：
  not_applicable
```

### 反证检查

`requireCounterEvidence=true` 时，need 只有满足以下任一条件才可 supported：

1. 已有关联 `contradicts` 证据，且冲突已在新 need 中解释处理；
2. 已执行专门的 counterexample search，结果为零，并在 action ledger 中留下查询记录；
3. P3 完整性检查证明相关范围已检查。

仅仅“没有找到反例”不能自动视为检查完成。

### Read receipt

`knowledge_read` 和 `knowledge_grep` 在 Research surface 返回结果时必须创建 receipt。

模型向 Ledger 添加证据时提交：

```ts
{
  needId: string;
  receiptId: string;
  quote: string;
  occurrenceIndex?: number;
  relation: "supports" | "contradicts" | "context";
  rationale: string;
}
```

服务端必须：

1. 读取 receipt 指向的冻结原文；
2. 检查 receipt hash；
3. 在 receipt 原文中精确查找 `quote`；
4. quote 不存在时拒绝；
5. 多次出现且未给 occurrenceIndex 时拒绝；
6. quote 上限 2000 字符；
7. 推导真实 block offsets；
8. 创建 `knowledge_evidence_items`；
9. 建立 need relation；
10. 标记 receipt consumed；
11. 重算 need 状态。

### 测试

```text
tests/knowledge-evidence-ledger.test.ts
tests/knowledge-read-receipts.test.ts
tests/knowledge-evidence-quote-validation.test.ts
```

必须覆盖：

* 模型伪造 quote 被拒绝；
* search snippet 不能直接入账；
* receipt 越 scope 被拒绝；
* receipt hash 漂移被拒绝；
* 多次出现 quote；
* 独立来源计数；
* conflict 状态；
* counterevidence 规则。

### 提交

```text
feat(knowledge): validate all research evidence against frozen source text
```

---

## P2-04：新增 Research 专用工具

### 新建文件

```text
lib/tools/knowledge-research-update-tool.ts
lib/tools/knowledge-research-finish-tool.ts
lib/tools/knowledge-delegate-tool.ts
lib/knowledge/research/research-tool-budget.ts
```

## `knowledge_research_update`

参数：

```ts
Type.Object({
  runId: Type.String(),

  createNeeds: Type.Optional(Type.Array(Type.Object({
    claim: Type.String(),
    kind: Type.Union([
      Type.Literal("fact"),
      Type.Literal("comparison"),
      Type.Literal("cause"),
      Type.Literal("timeline"),
      Type.Literal("counterexample"),
      Type.Literal("completeness"),
    ]),
    required: Type.Boolean(),
    minIndependentSources: Type.Number(),
    requireCounterEvidence: Type.Boolean(),
    requireAllRelevantUnits: Type.Boolean(),
  }))),

  linkEvidence: Type.Optional(Type.Array(Type.Object({
    needId: Type.String(),
    receiptId: Type.String(),
    quote: Type.String(),
    occurrenceIndex: Type.Optional(Type.Number()),
    relation: Type.Union([
      Type.Literal("supports"),
      Type.Literal("contradicts"),
      Type.Literal("context"),
    ]),
    rationale: Type.String(),
  }))),

  unresolvedGaps: Type.Optional(Type.Array(Type.Object({
    needId: Type.String(),
    gaps: Type.Array(Type.String()),
  }))),

  requestCompletenessPolicy: Type.Optional(Type.Union([
    Type.Literal("source_diverse"),
    Type.Literal("relevant_sections_complete"),
    Type.Literal("scope_complete"),
  ])),
});
```

限制：

```text
单 run 最多 8 个 need
claim 最多 1000 字符
rationale 最多 1000 字符
gap 最多 8 条
每条 gap 最多 500 字符
完整性策略只能升级，不能降级
```

## `knowledge_research_finish`

参数：

```ts
Type.Object({
  runId: Type.String(),
  conclusionSummary: Type.String(),
  requestedStopReason: Type.Union([
    Type.Literal("complete"),
    Type.Literal("budget_exhausted"),
    Type.Literal("no_progress"),
  ]),
});
```

行为：

* `complete`：必须通过 stop policy；
* `budget_exhausted`：必须确实达到任一预算；
* `no_progress`：必须连续两轮零新增有效证据；
* 不满足时返回结构化拒绝，研究继续；
* 工具不得相信模型声明的 need 状态。

## `knowledge_delegate`

参数：

```ts
Type.Object({
  runId: Type.String(),
  tasks: Type.Array(Type.Object({
    label: Type.String(),
    needIds: Type.Array(Type.String()),
    task: Type.String(),
    agentId: Type.Optional(Type.String()),
  })),
});
```

限制：

```text
每次最多 4 个 task
task 必须至少绑定一个当前 run need
默认使用当前 Agent
agentId 必须是 active Agent
Worker 只读
Worker 不得再次 delegate
Worker 不得调用 finish
```

行为：

1. 并行创建 isolated Agent workers；
2. 每个 worker 接收：

   * 用户问题；
   * scopeId；
   * runId；
   * 允许处理的 needIds；
   * 当前 need 状态；
   * 工具使用规则；
3. worker 使用 `knowledge_search`、`knowledge_read`、`knowledge_grep`；
4. worker 通过 `knowledge_research_update` 写入证据；
5. 等待所有 worker 结束；
6. 返回结构化 task 状态；
7. 更新 delegatedAgents 和 action ledger。

不得复用普通非阻塞 `subagent` 的 deferred 回灌作为 Research 前置依赖；Research 需要同步等待 Worker 完成。底层仍复用现有 `executeIsolated` Agent 运行时。

### 提交

```text
feat(agent): add evidence-ledger research and delegation tools
```

---

## P2-05：增加 Knowledge Research Surface

### 修改文件

```text
core/session-coordinator.ts
core/session-manifest/*
core/agent.ts
core/agent-manager.ts
lib/tools/knowledge-scope.ts
shared/tool-categories.ts
lib/tools/session-permission-wrapper.ts
```

### 新增 surface

```text
knowledge_research_root
knowledge_research_worker
```

### Root 允许工具

```text
knowledge_outline
knowledge_search
knowledge_read
knowledge_grep
knowledge_research_update
knowledge_research_finish
knowledge_delegate
```

### Worker 允许工具

```text
knowledge_outline
knowledge_search
knowledge_read
knowledge_grep
knowledge_research_update
```

### 一律禁止

```text
knowledge_manage
file write
edit
exec_command
terminal
browser
web_search
web_fetch
session send
workflow
settings update
subagent
knowledge_delegate（worker）
knowledge_research_finish（worker）
```

### 权限

```text
permissionMode = read_only
approvalPolicy = deny_on_prompt
memory = disabled
workspace write scope = none
```

### 模型

* Root 使用父会话当前 Agent 的 chat 模型；
* Worker 默认使用同一 Agent chat 模型；
* `agentId` 显式指定时使用对应 Agent 的 chat 模型；
* 不使用 knowledge auxiliary slot 代替 Agent。

### Scope 继承改造

现有 `KnowledgeToolSessionContext` 改为：

```ts
export interface KnowledgeToolSessionContext {
  sessionPath: string | null;
  scopeOwnerSessionPath: string | null;
}
```

解析规则：

1. 普通主会话：

   * `scopeOwnerSessionPath = sessionPath`
2. Research Root：

   * 沿 manifest `parentSessionId` 找到原始主会话；
3. Research Worker：

   * Worker → Root → 主会话；
4. 普通 subagent：

   * 子会话 → 父主会话；
5. 最多追溯 8 层；
6. 循环、缺失 manifest、跨 studio 或无法解析时 fail-closed；
7. `resolveKnowledgeTurnScope` 只允许：

   * 当前 session 就是 scope owner；
   * 或 `scopeOwnerSessionPath === scope.sessionPath`。

### Session 生命周期

Research Root 和 Worker 使用临时隔离 session：

* 执行中允许 session runtime 正常工作；
* 结构化 action 和 evidence 已持久化后删除临时 session 文件；
* 不把模型隐藏思考复制到 Knowledge DB；
* 崩溃恢复时重新启动当前研究 round，不依赖旧 session 继续。

### 测试

```text
tests/knowledge-research-surface.test.ts
tests/knowledge-research-tool-filter.test.ts
tests/knowledge-scope-ancestry.test.ts
tests/knowledge-research-worker-permissions.test.ts
```

覆盖：

* root 可搜索；
* worker 可读取；
* worker 不可 delegate；
* root/worker 不可写；
* 两层 scope 继承；
* 跨 session 越权；
* ancestry loop；
* 缺 manifest；
* read-only 强制。

### 提交

```text
feat(agent): add scope-safe isolated knowledge research surfaces
```

---

## P2-06：实现 KnowledgeResearchOrchestrator

### 新建文件

```text
lib/knowledge/research/knowledge-research-orchestrator.ts
lib/knowledge/research/research-prompts.ts
lib/knowledge/research/research-round-runner.ts
lib/knowledge/research/research-context-renderer.ts
```

### 输入

```ts
export interface KnowledgeResearchRequest {
  question: string;
  compiledScope: CompiledKnowledgeScope;
  policy: KnowledgeExecutionPolicy;
  parentSessionId: string;
  parentSessionPath: string;
  agentId: string;
  turnId: string;
  signal?: AbortSignal;
}
```

### 固定执行顺序

#### 第 1 步：创建 Research Run

状态：

```text
planning
```

写入：

* question；
* scopeId；
* parent session；
* policy；
* budget；
* started time。

#### 第 2 步：首轮 Research Root

首轮 prompt 必须包含：

```text
用户问题
当前 scope 摘要
当前为空的 Evidence Ledger
固定预算
必须先调用 knowledge_outline
必须创建 1～8 个 EvidenceNeed
搜索结果必须进一步 read
不得凭 snippet 引用
完成前必须调用 knowledge_research_finish
```

#### 第 3 步：Need 生成底线

首轮结束后若模型没有创建 need：

* 宿主创建一个 required fallback need；
* `claim = 原始用户问题`；
* `kind = fact`；
* `minIndependentSources = 1`；
* run 写 degraded reason；
* 继续第二轮，不直接失败。

#### 第 4 步：多 Agent 调查

当 required need 数量大于等于 2 时，Root 必须满足以下之一：

* 调用 `knowledge_delegate`；
* 或为每个 need 分别进行独立 search/read。

测试 fixture 中的多维问题必须实际出现 Worker。

#### 第 5 步：每轮结束重算 Ledger

宿主重新计算：

```text
need status
independent source count
conflicts
counterevidence state
unresolved gaps
new evidence count
budgets
```

不得接受模型在普通文本中声明“已经完整”。

#### 第 6 步：下一轮焦点

若未完成，下一轮 prompt 只携带：

* question；
* scope 摘要；
  -结构化 ledger；
  -未完成 need；
  -已执行 query 列表；
  -上一轮新增证据数量；
  -禁止重复的等价 query；
  -剩余预算。

不携带上一轮模型原始推理。

#### 第 7 步：定向补查

下一轮必须针对：

```text
uncovered need
partial need
conflicted need
尚未执行反证检查的 need
```

不得无条件重复首轮全部查询。

#### 第 8 步：停止

完整停止条件：

```text
所有 required need：
  status = supported 或 not_applicable

并且：
  conflicted required need = 0

并且：
  每个 need 满足独立来源要求

并且：
  requireCounterEvidence 的 need 已完成反证检查

并且：
  P3 完整性要求已满足
```

部分停止条件：

```text
达到 maxRounds
达到 maxToolCalls
达到 maxWallClockMs
连续两轮没有新增有效证据
用户取消
连续两轮 Agent 协议失败
关键工具持续不可用
```

#### 第 9 步：状态

正常完整：

```text
synthesizing → completed
```

预算耗尽但已有证据：

```text
synthesizing → partial
```

取消：

```text
cancelled
```

无任何可用证据且关键链路失败：

```text
failed
```

#### 第 10 步：生成最终 Evidence Packet

Packet 包含：

```text
runId
question
completenessPolicy
stopReason
EvidenceNeed 列表及状态
每个 need 的支持证据
矛盾证据
未解决缺口
最终 canonical evidence spans
回答契约
```

不得包含：

```text
Research Agent 原始思考
Worker 原始思考
完整工具输出
未经消费的 search snippets
```

### Tool call 预算

所有 Root 和 Worker 工具调用必须经过统一计数器。

达到 `maxToolCalls=32` 后：

* 拒绝新工具调用；
* 当前 run 进入 partial；
* stopReason=`tool_budget_exhausted`。

### Wall-clock

* 统一使用从 run 创建开始的绝对 deadline；
* Root 和 Worker 都消费同一个剩余预算；
* 不得为每轮重新获得 180 秒；
* abort signal 传播到所有 active worker；
* 活跃 controller 全部登记和清除。

### 测试

```text
tests/knowledge-research-orchestrator.test.ts
tests/knowledge-research-multi-round.test.ts
tests/knowledge-research-delegation.test.ts
tests/knowledge-research-stop-policy.test.ts
tests/knowledge-research-cancellation.test.ts
tests/knowledge-research-no-progress.test.ts
```

### 提交

```text
feat(knowledge): orchestrate bounded multi-round Agent research
```

---

## P2-07：详细模式正式切换到 Research Agent

### 修改文件

```text
core/engine.ts
core/desktop-session-submit.ts
lib/knowledge/knowledge-context-injector.ts
lib/knowledge/knowledge-rollup.ts
shared/knowledge-refs.ts
```

### 新增 Engine 方法

```ts
async buildDetailedKnowledgeResearchContext(input: {
  question: string;
  knowledgeRefs: {
    notebookIds: string[];
    mode: "detailed";
  };
  sessionId: string;
  sessionPath: string;
  agentId: string;
  turnId: string;
  signal?: AbortSignal;
}): Promise<{
  block: string;
  stats: KnowledgeRetrievalStats;
  evidence: KnowledgeInjectionEvidence;
}>;
```

### 正式路由

```ts
if (policy.path === "fast_local") {
  return buildFastKnowledgeContext(...);
}

return buildDetailedKnowledgeResearchContext(...);
```

详细模式生产路径不得再调用旧的：

```text
固定拆解
固定查询扩展
单次 Gap Analyzer
固定 broad 二次探测
候选池扩大后一次性注入
```

### 最终上下文格式

```text
[KnowledgeResearchContext]
Mode: detailed
Research run: <runId>
Research status: completed|partial
Completeness policy: ...
Rounds: ...
Searches: ...
Reads: ...
Delegated agents: ...
Stop reason: ...

Evidence needs:
[N1] supported ...
[N2] partial ...
[N3] conflicted ...

Unresolved gaps:
...

Validated evidence:
[K1] ...
[K2] ...

Answer contract:
1. Answer every required evidence need in order.
2. Give a detailed explanation rather than a short summary.
3. Distinguish source facts, synthesis, and inference.
4. Explain conflicts instead of silently choosing one side.
5. Disclose unresolved gaps.
6. Cite the supplied evidence ids.
7. Do not claim completeness beyond the recorded policy.
```

### Answer detail

“详细”不得通过最低字数实现。详细回答的检查标准是：

* 每个 required need 都有对应回答；
* 每个实质性结论有证据；
* 有冲突时单独说明；
* 有未解决项时单独说明；
* 不重复填充无信息内容。

### 旧 injector 处理

P2 收尾时：

1. 将旧 detailed 生产编排移动到：

```text
lib/knowledge/legacy/legacy-knowledge-context-injector.ts
```

1. 保留：

   * 历史测试；
   * 老消息读取；
     -必要的兼容数据渲染；
2. 新生产请求不得进入 legacy。
3. `knowledge-context-injector.ts` 收缩为：
   -兼容 facade；
   -安全扫描；
   -EvidencePacker 调用；
   -上下文渲染。

### rollup

`knowledge-rollup.ts` 暂不删除，但不再作为 detailed 的主体机制。只允许用于：

* legacy 历史路径；
  -单个超长已验证证据的有界压缩 fallback。

不得因为候选多就对候选池整体 rollup 并宣称完成调查。

### 历史压缩

`compressHistoricalKnowledgeContextMessages` 必须同时识别：

```text
[KnowledgeContext]
[KnowledgeResearchContext]
```

### 测试

```text
tests/desktop-session-submit-detailed-research.test.ts
tests/knowledge-detailed-no-legacy-route.test.ts
tests/knowledge-research-context-renderer.test.ts
tests/historical-knowledge-context-compression.test.ts
```

必须断言：

* detailed 使用 `executeIsolated`；
* detailed 不调用旧 decompose/expand/gap 入口；
* main `promptSession` 只在 research 完成或 partial 后执行；
  -最终 manifest 只包含已验证 evidence；
  -取消时不进入 main prompt。

### 提交

```text
refactor(knowledge): route detailed answers through Agent research
```

---

## P2-08：Research 过程 UI

### 修改文件

```text
desktop/src/react/services/ws-message-handler.ts
知识过程卡组件
知识结果折叠组件
desktop/src/react/utils/tool-label.ts
五份 locale
```

### 新增事件

```text
knowledge_research_started
knowledge_research_plan_updated
knowledge_research_round_started
knowledge_research_worker_started
knowledge_research_worker_completed
knowledge_research_ledger_updated
knowledge_research_completed
```

### 展示顺序

```text
正在规划调查
第 1/4 轮：正在检索和阅读
已派出 N 个调查 Agent
已完成 X/Y 个证据问题
正在核对矛盾和缺口
正在整理详细回答
```

### 最终折叠摘要

```text
详细调查 · R 轮 · S 次检索 · D 次阅读 · X/Y 项完成
```

Partial：

```text
详细调查未完全完成 · R 轮 · 仍有 N 项待确认
```

### 界面约束

* 不展示模型隐藏推理；
* 不在正文中输出内部 JSON；
* 不为每个底层 SQL 查询创建单独卡片；
* Worker 每个任务一张聚合卡；
* 同一 action 不重复生成卡片；
* 退出重进后可由持久化 stats 恢复摘要。

### P2 质量 fixture

新建：

```text
tests/fixtures/knowledge-research/
```

至少包含：

1. `second-round-clue`
   第一轮只能找到一个中间名词，第二轮必须用该名词再次搜索才能回答。

2. `cross-source-comparison`
   两个来源分别包含比较两侧信息。

3. `conflicting-sources`
   两份资料给出冲突数字。

4. `counterexample`
   主结论容易成立，但另一章节存在反例。

5. `timeline`
   时间点分散在不同章节。

6. `no-result`
   所选资料确实不包含答案。

7. `scope-escape`
   其他未选笔记本中存在答案，Research Agent 必须无法读取。

### 必须通过的行为断言

```text
second-round-clue 至少两次不同查询
每个最终 evidence 都有 read receipt
search snippet 不能直接入最终 manifest
conflicting-sources 产生 conflicted need
scope-escape 不访问未选来源
fabricated quote 被拒绝
premature finish 被拒绝
多维问题实际启动 Worker
达到预算时输出 partial 而非伪 completed
```

### P2 阶段门禁

```bash
npm run typecheck
npm run lint
npm run lint:boundary

npx vitest run \
  tests/knowledge-store-v18-migration.test.ts \
  tests/knowledge-evidence-ledger.test.ts \
  tests/knowledge-evidence-quote-validation.test.ts \
  tests/knowledge-research-surface.test.ts \
  tests/knowledge-scope-ancestry.test.ts \
  tests/knowledge-research-orchestrator.test.ts \
  tests/knowledge-research-multi-round.test.ts \
  tests/knowledge-research-delegation.test.ts \
  tests/desktop-session-submit-detailed-research.test.ts \
  tests/knowledge-detailed-no-legacy-route.test.ts

npm test
npm run build:server
npm run build:server:open
npm run build:client
npm run test:knowledge-platform-smoke
node scripts/smoke-packaged-knowledge.mjs
```

P2 完成后必须确认：

```text
fast 仍为本地 FTS
detailed 使用 Agent runtime
detailed 可执行多轮 search/read
detailed 可并行派出多个 Research Workers
最终证据全部经原文校验
详细程度由 EvidenceNeed 覆盖控制
旧一次性 detailed 管线不再参与生产
```

阶段提交：

```text
chore(knowledge): close P2 Research Agent verification
```

---

# 八、P3：完整性证明与多粒度索引

P3 解决两个问题：

1. “全部、全文、逐章、有没有遗漏、是否存在”等问题不能靠 topK 证明；
2. 当前分块尺寸由嵌入窗口的 80% 推导，检索粒度可能过大。当前实现确实采用这一自动规则。

---

## P3-01：完整性策略选择

### 新建文件

```text
lib/knowledge/research/completeness-policy.ts
```

### 确定性最低策略

以下表达触发 `scope_complete`：

```text
全文
全书
整本
全部
所有
每一个
有没有任何
是否存在任何
是否从未
是否没有
有没有遗漏
列出所有
所有出现
所有提到
从头到尾
```

以下表达触发 `relevant_sections_complete`：

```text
逐章
每一章
逐节
每个章节
前后章节
所有相关章节
```

多来源比较触发至少：

```text
source_diverse
```

普通详细问题默认：

```text
source_diverse
```

快速模式永远：

```text
best_effort
```

### Agent 升级

Research Agent 可以通过 `knowledge_research_update.requestCompletenessPolicy` 请求升级：

```text
best_effort → source_diverse
source_diverse → relevant_sections_complete
relevant_sections_complete → scope_complete
```

不得降级。

### 测试

```text
tests/knowledge-completeness-policy.test.ts
```

覆盖中英文关键词、否定句、普通事实问题和快速模式。

### 提交

```text
feat(knowledge): derive explicit completeness policies from user intent
```

---

## P3-02：Knowledge 数据库升级到 v19

### 新增表

#### `knowledge_completeness_checks`

```text
id TEXT PRIMARY KEY
research_run_id TEXT NOT NULL UNIQUE
policy TEXT NOT NULL
status TEXT NOT NULL
total_units INTEGER NOT NULL DEFAULT 0
checked_units INTEGER NOT NULL DEFAULT 0
relevant_units INTEGER NOT NULL DEFAULT 0
unavailable_units INTEGER NOT NULL DEFAULT 0
coverage_ratio REAL NOT NULL DEFAULT 0
exact INTEGER NOT NULL DEFAULT 0
created_at TEXT NOT NULL
updated_at TEXT NOT NULL
completed_at TEXT
```

#### `knowledge_completeness_units`

```text
check_id TEXT NOT NULL
coverage_unit_id TEXT NOT NULL
source_id TEXT NOT NULL
parse_artifact_id TEXT NOT NULL
block_id TEXT NOT NULL
start_offset INTEGER NOT NULL
end_offset INTEGER NOT NULL
section_key TEXT
status TEXT NOT NULL
worker_session_id TEXT
updated_at TEXT NOT NULL
PRIMARY KEY(check_id, coverage_unit_id)
```

status：

```text
pending
checked_relevant
checked_irrelevant
unavailable
failed
```

#### `knowledge_completeness_unit_evidence`

```text
check_id TEXT NOT NULL
coverage_unit_id TEXT NOT NULL
evidence_id TEXT NOT NULL
PRIMARY KEY(check_id, coverage_unit_id, evidence_id)
```

#### `knowledge_completeness_coverage_runs`

```text
check_id TEXT NOT NULL
coverage_run_id TEXT NOT NULL
PRIMARY KEY(check_id, coverage_run_id)
```

### 迁移

```text
v18 → v19
createSchemaV19
```

单事务、可回滚、老数据不改写。

### 测试

```text
tests/knowledge-store-v19-migration.test.ts
tests/knowledge-completeness-store.test.ts
```

### 提交

```text
feat(knowledge): persist exact completeness coverage state
```

---

## P3-03：实现 Completeness Executor

### 新建文件

```text
lib/knowledge/research/knowledge-completeness-executor.ts
lib/knowledge/research/coverage-shard-planner.ts
lib/tools/knowledge-coverage-read-tool.ts
lib/tools/knowledge-completeness-mark-tool.ts
```

### 使用现有 CoverageUnit

完整性分母必须来自：

```ts
buildCoverageUnits(...)
```

不得使用：

```text
检索 chunk 数
HNSW 命中数
FTS 命中数
EvidenceNeed 数
prompt 中已注入块数
```

### scope_complete

1. 遍历冻结 scope 中所有 ready ParseArtifact。
2. 为每个 artifact 构建 CoverageUnit。
3. 所有 unit 加入 completeness check。
4. 非 ready、needs OCR 或 failed source：

   * 计入 unavailable；
   * 不得从分母静默消失。
5. 按 token 预算分 shard。
6. 每个 shard 最大 12,000 tokens。
7. 最多 4 个 Worker 并行。
8. Worker 必须逐 unit 标注：

   * relevant；
   * irrelevant；
   * unavailable。
9. relevant unit 中的引用仍通过 receipt 和原文 quote 校验进入 Evidence Ledger。

### relevant_sections_complete

1. Research Agent 先确定相关 sectionKeys。
2. 只对这些 section 中的全部 CoverageUnit 建立分母。
3. 相关 section 为空时不得自动退化成 best_effort：
   -继续调查；
   -或 partial 并说明未能确定完整相关范围。

### 新工具

`knowledge_coverage_read`：

```text
输入：runId、checkId、shardId
输出：该 shard 的 unitId、位置和原文
只允许 completeness Worker
```

`knowledge_completeness_mark`：

```text
输入：
  checkId
  unit results
  可选 receipt/evidence relation
宿主校验 unit 是否属于 shard
宿主更新 coverage 状态
```

### 完整性完成条件

```text
checked_units + unavailable_units = total_units
所有 worker 已结束
failed unit = 0
```

`exact=true` 只允许：

```text
checked_units = total_units
unavailable_units = 0
failed unit = 0
coverage_ratio = 1
```

### 否定结论纪律

只有 `exact=true` 时，最终上下文才允许：

```text
在所选完整范围中不存在……
```

否则必须使用：

```text
在已检查的范围内未发现……
由于 N 个单元/来源不可用，无法证明完整不存在……
```

### 测试

```text
tests/knowledge-completeness-executor.test.ts
tests/knowledge-completeness-sharding.test.ts
tests/knowledge-completeness-negative-claim.test.ts
tests/knowledge-completeness-unavailable-source.test.ts
```

### 提交

```text
feat(knowledge): prove completeness over canonical coverage units
```

---

## P3-04：建立 source / section / span 多粒度索引

### 修改文件

```text
lib/knowledge/chunker.ts
lib/knowledge/knowledge-index-store.ts
lib/knowledge/ingestion-service.ts
lib/knowledge/knowledge-query-service.ts
lib/knowledge/knowledge-manager.ts
```

### Chunker 版本

```ts
export const KNOWLEDGE_CHUNKER_VERSION = "3";
```

旧 v2 profile 和 variant 不删除。

### 固定粒度

```ts
export const KNOWLEDGE_SPAN_TARGET_TOKENS = 512;
export const KNOWLEDGE_SPAN_OVERLAP_TOKENS = 64;
export const KNOWLEDGE_SECTION_SOFT_MAX_TOKENS = 8192;
```

新默认不得再由：

```text
embedding context window × 80%
```

决定。

嵌入窗口只用于验证：

```text
span 不得超过模型硬输入上限
```

而不是决定检索粒度。

### Knowledge index schema

`knowledge-fts.db` 从 v3 升到 v4。

新增：

#### `knowledge_source_documents`

```text
parse_artifact_id TEXT PRIMARY KEY
title TEXT NOT NULL
outline_text TEXT NOT NULL
search_text TEXT NOT NULL
created_at TEXT NOT NULL
updated_at TEXT NOT NULL
```

及对应 FTS5 表。

#### `knowledge_sections`

```text
id TEXT PRIMARY KEY
parse_artifact_id TEXT NOT NULL
section_ordinal INTEGER NOT NULL
heading_path_json TEXT NOT NULL
start_block_ordinal INTEGER NOT NULL
end_block_ordinal INTEGER NOT NULL
text TEXT NOT NULL
token_count INTEGER NOT NULL
search_text TEXT NOT NULL
created_at TEXT NOT NULL
updated_at TEXT NOT NULL
UNIQUE(parse_artifact_id, section_ordinal)
```

及对应 FTS5 表。

在 `knowledge_chunks` 增加：

```text
section_id TEXT
```

增加索引：

```text
chunk_index_variant_id + section_id + ordinal
parse_artifact_id + section_id
```

### Section 构建规则

* Markdown/HTML：按 headingPath 切节；
* 章节型 text：按章节标题切节；
* PDF：优先标题路径，否则按页组；
* 无结构纯文本：按不超过 8192 tokens 的确定性父段切节；
* 每个原始 block 必须属于且只属于一个 primary section；
* section 过大时可以拆分子 section，但必须保留同一父 heading。

### Span 构建

* 在 section 内按约 512 tokens 构建 span；
* 相邻 span 重叠约 64 tokens；
* 每个 span 保留精确 block spans；
* overlap 只是检索派生物，引用仍落回原始 block；
* span ID 必须包含 chunker v3 配置；
* 不得覆盖旧 v2 variant。

### Source document

确定性生成，不调用模型：

```text
source title
heading outline
首段摘要片段
末段摘要片段
文件类型
```

### 迁移与后台重建

1. 新应用启动后扫描 active 最新 artifact。
2. 没有 v3 ready variant 的来源加入低优先级重建。
3. 每个 source 只入队一次。
4. 查询期间：

   * v3 ready：优先使用；
   * v3 未 ready：继续使用 v2；
     -两者都无：显式降级并入队。
5. 旧向量和旧 FTS 继续可读，直到生命周期 GC 确认可删除。
6. 不允许启动时同步重建全部知识库。

### ANN 联动

v3 span vector variant 完成后按 P1 规则自动生成新的 HNSW 文件。

### 测试

```text
tests/knowledge-chunker-v3.test.ts
tests/knowledge-section-index.test.ts
tests/knowledge-multigrain-migration.test.ts
tests/knowledge-v2-profile-fallback.test.ts
tests/knowledge-v3-background-reindex.test.ts
```

必须验证：

* block 无遗漏；
* primary section 无重叠；
* span 可重叠但原文引用精确；
* v2/v3 并存；
  -重建中查询不中断；
  -嵌入大窗口不再生成超大 span。

### 提交

```text
feat(knowledge): add source section and span retrieval grains
```

---

## P3-05：实现分层检索

### 修改文件

```text
lib/knowledge/knowledge-search-service.ts
lib/knowledge/evidence-span-extractor.ts
lib/tools/knowledge-search-tool.ts
lib/tools/knowledge-read-tool.ts
```

### 快速模式

继续保持：

```text
span FTS 直检
```

不得因为有 section 索引而增加串行步骤。

### 详细模式默认搜索

```text
第一层：source / section FTS
第二层：相关 section 内 span FTS
第三层：相关来源 HNSW
第四层：对选中 section 的小范围精确 vector 补查
第五层：RRF 融合
第六层：可选 rerank
```

### Section narrowing

1. section FTS 默认取前 12 个 section。
2. 每个命中 source 至少保留一个 section。
3. Search Agent 可显式指定 sectionKeys。
4. 对 selected section，span FTS 在 SQL 中直接过滤 `section_id`。
5. 不得先检索全 scope 再只在 JavaScript 中过滤全部结果。

### Section 内向量补查

由于 HNSW JavaScript 层没有可靠的任意 section 过滤，采用固定策略：

1. HNSW 先作为 source-level semantic recall；
2. 对 section FTS 选出的 section：

   * 从 index store 得到 section 内 chunk IDs；
   * portable vector store 只读取这些 chunk IDs 对应的向量；
   * 对该小集合做 exact cosine；
3. 不允许为 section 查询扫描整个 studio 的全部向量。

### Parent expansion

Agent 读取命中时支持：

```text
按 chunk ordinal
按 sectionId
按命中前后 span
```

`knowledge_read` 新增可选参数：

```ts
sectionId?: string;
aroundChunkId?: string;
neighborWindow?: number; // 0～3
```

旧参数继续兼容。

### 搜索结果

`KnowledgeSearchHit` 增加：

```ts
grain: "source" | "section" | "span";
sectionId: string | null;
parentSectionHeading: string[] | null;
```

只有 span 或经 read materialize 的 section 原文可成为 evidence。

### 测试

```text
tests/knowledge-hierarchical-search.test.ts
tests/knowledge-section-filtered-vector.test.ts
tests/knowledge-parent-child-retrieval.test.ts
tests/knowledge-hierarchical-recall-golden.test.ts
```

### 提交

```text
feat(knowledge): retrieve hierarchically across sources sections and spans
```

---

## P3-06：完整性与详细回答质量门禁

### 新增 fixture

```text
tests/fixtures/knowledge-completeness/
```

至少包含：

1. 全文所有人名；
2. 某术语是否从未出现；
3. 每章的核心变化；
4. 所有反例；
5. 一份 needs OCR source；
6. 一个解析失败 source；
7. 相同事实在不同章节重复；
8. 同一来源内互相冲突的陈述。

### 指标

详细模式报告：

```text
requiredNeedCompletionRate
citationValidityRate
conflictDetectionRate
counterEvidenceCheckRate
coverageRatio
unavailableUnitCount
researchRounds
searchCalls
readCalls
delegatedAgents
```

### 门禁

```text
最终 citation validity = 100%
scope_complete 且所有 source 可用时 coverageRatio = 1
coverageRatio < 1 时绝不输出 absolute absence proven
冲突 fixture 必须得到 conflicted 状态
第二轮线索 fixture 必须发生后续查询
最终 manifest 不得包含未 read 的 search candidate
```

### 性能基准

扩展：

```text
scripts/benchmark-knowledge-fast.mjs
scripts/benchmark-knowledge-vector.mjs
```

新增：

```text
scripts/benchmark-knowledge-research.mjs
```

Research benchmark 不以总时长单独判定质量，但必须记录：

```text
timeToFirstResearchAction
每轮耗时
每个 Worker 耗时
搜索耗时
阅读耗时
最终合成耗时
token 使用
模型调用数
```

要求：

```text
详细模式首个可见研究动作 ≤ 500ms
所有模型调用和工具调用均可观测
不存在无限研究循环
```

---

## P3-07：清理旧架构和最终收口

### 清理要求

1. `knowledge-context-injector.ts` 不再保留完整研究编排。
2. 生产路径中不存在以下调用链：

```text
detailed
→ fixed decomposition
→ fixed expansion
→ fixed one-shot retrieval
→ giant candidate injection
```

1. 删除生产不再使用的：
   -旧快速 rerank gate；
   -旧快速 8192-token 预算；
   -详细模式固定候选数量假设；
   -重复搜索实现；
   -重复 scope 解析实现。
2. 保留历史数据读取类型和 legacy migration。
3. 任何 deprecated 字段必须注明：
   -只用于历史读取；
   -生产写入侧不再生成。
4. 更新：

   * `PROGRESS.md`；
     -架构注释；
     -release digest 草稿；
     -持久化 registry；
     -export manifest；
     -CLI closure；
     -测试 inventory。
5. 不手工编辑生成物。

### 最终全量验证

```bash
npm run typecheck
npm run lint
npm run lint:boundary

npm test

npm run build:server
npm run build:server:open
npm run build:client

npm run test:knowledge-platform-smoke
node scripts/smoke-packaged-knowledge.mjs

node scripts/generate-persistence-schema-fingerprint.mjs
node scripts/check-persistence-schema-fingerprint.mjs
node scripts/compute-cli-closure.mjs
node scripts/export-open-tree.mjs
node scripts/test-inventory.mjs

npm run pack
```

在支持的平台 CI 中执行：

```text
macOS arm64
macOS x64
Windows x64
Linux x64
```

至少验证：

```text
应用启动
Knowledge DB v17→v18→v19
FTS index v2→v3→v4
usearch native load
HNSW 搜索
portable fallback
快速检索
详细 Research Agent
完整性检查
打包产物重启后检索
```

### 最终提交

```text
refactor(knowledge): complete fast retrieval and Agent research architecture
```

之后再建立 audit/seal 提交：

```text
chore(audit): advance verified source for knowledge P0-P3 refactor
```

不得自行合并 `main`。

---

# 九、阶段提交顺序

Codex 必须按以下顺序提交，不要压缩为一个巨型提交。

```text
01 docs(knowledge): establish P0-P3 refactor baseline
02 refactor(knowledge): introduce explicit fast and research execution policies
03 feat(knowledge): compile frozen search scopes without query-time full scans
04 feat(knowledge): add deadline-bound local FTS fast path
05 feat(knowledge): extract compact canonical evidence spans for fast answers
06 refactor(knowledge): centralize evidence packing around canonical spans
07 feat(knowledge): route fast references through the local-only pipeline
08 feat(ui): expose local fast retrieval timing and execution path
09 chore(knowledge): close P0 fast-path verification

10 feat(knowledge): persist query-ready chunk variant metadata
11 refactor(knowledge): centralize retrieval behind KnowledgeSearchService
12 perf(knowledge): deduplicate query embeddings across notebooks and requests
13 perf(knowledge): fuse globally and rerank once per model group
14 feat(knowledge): add rebuildable HNSW vector search with portable fallback
15 feat(agent): expose bounded scope-safe knowledge search
16 refactor(agent): share compiled scope and search data across knowledge tools
17 chore(knowledge): close P1 unified retrieval verification

18 feat(knowledge): define evidence-ledger research contracts
19 feat(knowledge): persist bounded research runs and evidence ledgers
20 feat(knowledge): validate all research evidence against frozen source text
21 feat(agent): add evidence-ledger research and delegation tools
22 feat(agent): add scope-safe isolated knowledge research surfaces
23 feat(knowledge): orchestrate bounded multi-round Agent research
24 refactor(knowledge): route detailed answers through Agent research
25 chore(knowledge): close P2 Research Agent verification

26 feat(knowledge): derive explicit completeness policies from user intent
27 feat(knowledge): persist exact completeness coverage state
28 feat(knowledge): prove completeness over canonical coverage units
29 feat(knowledge): add source section and span retrieval grains
30 feat(knowledge): retrieve hierarchically across sources sections and spans
31 refactor(knowledge): complete fast retrieval and Agent research architecture
32 chore(audit): advance verified source for knowledge P0-P3 refactor
```

---

# 十、最终交付物

任务完成后必须生成并提交以下文档。

## `KNOWLEDGE_REFACTOR_IMPLEMENTATION_REPORT.md`

按 P0、P1、P2、P3 分别列出：

```text
完成内容
新增模块
修改模块
生产调用链
删除或退役路径
数据库版本
降级策略
```

## `KNOWLEDGE_REFACTOR_TEST_REPORT.md`

列出：

```text
每条测试命令
执行时间
通过数量
失败数量
跳过数量
构建结果
打包结果
各平台 smoke 结果
```

## `KNOWLEDGE_REFACTOR_PERFORMANCE_REPORT.md`

列出：

```text
快速模式 10k/100k P50/P95/P99
HNSW 与 portable 对比
缓存命中与未命中
详细模式各阶段耗时
Research Agent 调用轮数
模型调用和 token 使用
测试机器配置
```

## `KNOWLEDGE_REFACTOR_REMAINING.md`

若没有剩余项，必须明确写：

```text
No known remaining implementation items within the P0-P3 task scope.
```

不得删除该文件。

## `KNOWLEDGE_REFACTOR_FACTS.json`

至少包含：

```json
{
  "baseSha": "3eab85891a1747c64064252804f70c0a3773f021",
  "finalSourceSha": "",
  "knowledgeSchemaVersion": 19,
  "knowledgeIndexSchemaVersion": 4,
  "annSchemaVersion": 1,
  "fast": {
    "executionPath": "fast_local",
    "remoteModelCalls": 0,
    "deadlineMs": 1200,
    "maxEvidenceSpans": 8,
    "renderBudgetTokens": 2400
  },
  "detailed": {
    "executionPath": "detailed_research",
    "maxRounds": 4,
    "maxParallelAgents": 4,
    "maxToolCalls": 32,
    "maxWallClockMs": 180000
  },
  "vector": {
    "primary": "usearch_hnsw",
    "fallback": "portable_exact",
    "usearchVersion": "2.26.0"
  },
  "tests": {
    "typecheck": "",
    "lint": "",
    "boundary": "",
    "vitest": "",
    "buildServer": "",
    "buildServerOpen": "",
    "buildClient": "",
    "package": ""
  }
}
```

任何未通过项不得填写为 `passed`。
