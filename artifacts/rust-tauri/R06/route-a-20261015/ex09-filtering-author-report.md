# EX09 检索范围、结果粒度和 rerank 配置：冻结交付

状态：READY_FOR_ROOT_INDEPENDENT_REVIEW。这是实现者自验交付，等待 Root 与未参与实现的审阅者确认，不是独立验收结论。

净补丁 `/private/tmp/r06-ex09-filtering-incremental.patch`，SHA256 `759edf7fa64bbcf08c5d39f92654d18fadb84300dc451dffd64f3a7a442cb9da`。逐文件基线和候选摘要见 `/private/tmp/r06-ex09-filtering-source-sha.json`；验证收据 `/private/tmp/r06-ex09-filtering-validation.json`；真实 SQLite 只读证据 `/private/tmp/r06-ex09-filtering-real-sqlite-evidence.json`。独立源树 `/private/tmp/lingxi-r06-ex09-filtering`，复用 target `/private/tmp/r06-ex09-target`。当前 Main 四份基线摘要仍完全匹配，`git apply --check` 退出0；未 apply Main，未登记、改门禁、提交或推送。

## 改动及原因

- sectionKeys 原先接收但未用于检索。现在从当前已就绪、被冻结范围内的源核验真实章节，限制在 FTS SQL 排序/limit 前和向量排序/截断前；未知、跨源或不合法参数在模型请求前报错。保留现役 TS 的 sectionKeys:[] 清除章节限制语义。明确 sourceIds:[] 或 notebookIds:[] 则返回零范围，不再意外扩大成全部来源。
- hybrid 返回真实 source/section/span 三种粒度。正文命中仍读真实 chunk/span 并提供可读回引用。仅来源名或章节标题命中是导航线索，有真实 readMore 锚点、无正文 spans/伪引文；标题词项同样受96项上限约束。两种检索通道一致限缩，向量结果重新读取真实当前 chunk 元数据。
- notebook 已配置 rerankModelRef，但当前没有按该引用执行的端口。按照 SPEC 明确返回 KNOWLEDGE_RERANK_NOT_CONNECTED 和保留原排序的原因，不发起 rerank 请求，不透露凭证。无配置不冒报降级，fts 不冒称执行重排。模型引用 null 现在真实清除配置，省略仍保持不变；数值字段未扩展。

## 真实反例及保留证据

原生产实现的章节限缩反例 `/tmp/r06-ex09-filtering-red.log`：只选 Chosen 且 top1，实际返回 Other 以及 Other 引用，退出101。显式 sourceIds:[] 原实现返回 Other 并执行 hybrid 请求，保留 `/tmp/r06-ex09-filtering-empty-id-red.log`。另一个真实新断言发现 rerank:null 没有清除引用，保留 `/tmp/r06-ex09-filtering-final-original-and-new.log`。同一首次并行日志中旧夹具在导入 insert_blocks 阶段出现 SQLite locked；依现有知识套件约定改为串行验证，没有产品重试/catch或放宽断言。原 strict clippy 不必要 clone 失败日志也保留，已直接修正测试表达式。

三条新增用例实际走 Run、HTTP provider stub、存储和引用读取：章节top1/全文引用；两个通道的限缩、非法及跨源参数、空数组、重排零调用与配置清除；来源/章节/正文粒度和真实导航读回。结构化文档场景使用真实 FactStore API 注入合法解析事实，生产 kernel 分块索引，不声称完整 PDF 解析。原22用例全文逐字保留，新3追加；Root第12条 supplementary Han/空query先验证算法保护也全文保留。

## 验证与精确范围

- 最终受影响入口串行25+6=31 PASS，日志 `/tmp/r06-ex09-filtering-final-tool-entry31.log`（25用例11.57秒、工具6用例3.09秒）。
- 同一最终 knowledge.rs 和 adapter SHA 的模型变更路径11 PASS（82.44秒），日志 `/tmp/r06-ex09-filtering-frozen-service42.log`。该次同时25+6也通过；之后最后改动只在工具层增加显式空ID入口及新断言，已重跑上述31，没有把先前42冒写为最后一次整包重跑。
- 最终 adapter 52 PASS：原knowledge9、F12 22、W09 9、EX09 12，含 Root supplementary Han 新案，日志 `/tmp/r06-ex09-filtering-adapter52.log`。
- golden过滤4 PASS，包含原知识身份/配置2项与匹配到的记忆2项，`/tmp/r06-ex09-filtering-goldens.log`。kernel/golden/算法版本未改。
- 最终 service lib+新测试及 adapter lib strict clippy（-D warnings）PASS，四文件 rustfmt --check PASS。日志见 validation.json。全部 cargo 用绝对1.98.1、--locked、CARGO_INCREMENTAL=0、原外置 target，合成 LINGXI_HOME/TMPDIR；未覆盖 HOME。

限制：真实本机网络/Run/数据库和引用证据不代表商业供应商、实际付款、Windows/Linux运行或完整桌面启动；未执行按 notebook 配置的 rerank，因此结果明确降级。未改缓存版本、历史正文或既有 golden；不宣称 EX09 全部或 R06 完成。
