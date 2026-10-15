# EX09 文本与空查询修复独立复核

结论：**PASS_ADAPTER_FIX_SCOPE**。此前真实单扩展汉字误召回已修；旧代际混用/收费向量保留检查继续通过。仅签本次实际源SHA的adapter份额；service真实HTTP/Run/收费联合验证未由本轮执行，整候选仍在主控迭代，不签EX09整体完成。

实际Main adapter SHA256 `00a3669a66b52b99709524a0c494fdd12c50c4d21e35542cf2f2e36a25e304e1`；新12测试 `aee43dec91c3cf2aabe6670ce0bc2c6d4fcfcd74b73ff81a5a25e1858bf2cfb5`；service knowledge静态核对 SHA `9c619d94716c7adb00661f8cfa23a154489a6ba8929348219de9d54445f4a65e`。起止六文件SHA一致，证明 `/tmp/r06-ex09-r2-independent-source-{start,end}.json`。未修改Main/作者源/原断言/登记/门禁；未提交。

## 独立实际结果

- 20组原自选输入不变，真实当前TS参考与修后Rust normalized/tokens/literal/document逐项20/20一致，原4差异全部清除。结果 `/tmp/r06-ex09-r2-independent-tokenizer-diff.json`，实际Rust输出 `/tmp/r06-ex09-r2-independent-rust-reference.json`，沿用原真实TS输出 `/tmp/r06-ex09-independent-ts-reference.json`（TS源SHA未变）。
- 原U+20000 SQLite反例输入/目标/decoy不变，两方现在literal均为`"𠀀"`，仅命中target。标点/BMP单字/空输入附加负控都无命中。源码 `/tmp/r06-ex09-r2-independent-single-han-sqlite.rs`；日志 `.log`；新实际数据库 `/tmp/r06-ex09-r2-independent-single-han-sqlite`。
- Main当前12测试实际PASS，包含原11逐字节完整前缀未改+新U+20000/U+30000真实SQLite用例；当前原knowledge_store9实际PASS，共21，0失败/跳过。日志 `/tmp/r06-ex09-r2-independent-regression.log` 与 `/tmp/r06-ex09-r2-independent-original-knowledge.log`。
- 原F12 22/W09 9与kernel两个知识golden已在修前独立实跑通过；本轮未再次重跑这些未受改动路径，不能将其记为修后重新执行。

均用 `/Users/study_superior/.cargo/bin/cargo`1.98.1、`--locked`、`CARGO_INCREMENTAL=0`，复用唯一外target `/private/tmp/r06-ex09-target`。Rust独立探针用绝对rustc1.98.1链接刚由Main cargo实际重建的production adapter库；未新建target。LINGXI_HOME/TMPDIR合成，HOME未覆盖。

## 混合缓存与错误检查

独立源码 `/tmp/r06-ex09-r2-independent-cache-boundaries.rs`；日志同名 `.log`；新数据库 `/tmp/r06-ex09-r2-independent-cache-boundaries`。原R1检查输入保持，新输出不覆盖原FAIL。

以下六类边界实际PASS（日志合并为4组）：

1. 新current正控命中后，新旧变体混合FTS两种顺序均整体拒绝。
2. 新旧混合即使query空，也先识别stale并报错。
3. 新向量正控可查，新旧混合vector两种顺序均拒绝。
4. 旧paid begin/checkpoint持续拒绝，旧向量hex逐行保持；新namespace checkpoint空起，只接受新写行。
5. 真关闭/重开维持old stale+current ready，当前ready列表仅一项。
6. 额外篡坏自有current算法标记后，空query也必须明确报错；不能在新空结果分支掩盖坏版本，旧paid行无变化。

生产改动对应正确：CJK单字判断改UTF16长度，与当前TS一致；search_fts先逐个require_current_algorithm，再对无有效literal返回空，不再任取全变体。

## service 预先只读核对

四处profile推导1696/2243/2302/2522行统一`current_cache_profile_hash(resolve_chunker_config(...).2)`，覆盖search、outline、navigation、ingestion。查询ReadyScope保留的cache profile参与真实civ/viv计算，与摄入一致；`build_chunks`仍接原artifact/blocks/target，未将cache hash塞进权威原文/ChunkDraft/job profile。kernel/TS源SHA保持原值，原ID/golden算法未改。

旧cache只存在时ready为空，Hybrid分支实际源码先检查ready非空才调用embed_owned，静态上阻止该场景的query费用；outline索引读取改为`?`明确传播错误。新current索引存在时，不因同artifact还保留旧stale行永久降级。

以上是静态接线核对，不冒充service真实运行。显式rebuild、原缺口/全角词、引用原字节、旧付费字节、重启/下一Run及零重复收费的联合测试由Root补充，未在此签PASS。

R1完整NEEDS_FIX报告 `/tmp/r06-ex09-adapter-independent-review.md`，真实错误输出 `/tmp/r06-ex09-independent-single-han-sqlite.log` 和四差异均保留。未验证其它平台、远端付费供应商、完整产品门禁/包；不签R06_ACCEPTED/R07_READY。

机器结论 `/tmp/r06-ex09-adapter-r2-independent-confirmation.json`。
