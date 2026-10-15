# EX09 适配器冻结候选独立复核

结论：**NEEDS_FIX**。旧缓存代际与旧向量保留的本轮实际检查通过；新文本实现仍有确定语义差异：单个扩展汉字被过滤后，搜索条件变空，真实SQLite返回了无关资料。原51通过不能覆盖此缺陷，也不能签EX09整体完成。

## 候选与边界

- 冻结树 `/private/tmp/lingxi-r06-ex09-adapter`，净补丁 `/private/tmp/r06-ex09-adapter-incremental.patch` SHA256 `874dd989c96951d82bac51968c28f3f76471d181ad19a63c64440a0f376bfb31`。
- 实际适配器 `rust/crates/lingxi-adapters/src/storage/knowledge.rs` SHA256 `dce9883e00a4487f373ffe43d581dbe6322ab1ef258b2fb7283304454191ffa8`。
- 十文件全部独立重算冻结SHA并在结束重验一致，证明 `/tmp/r06-ex09-independent-source-proof.json`。
- 未改Main、冻结源、原断言/golden、登记/门禁，未提交；只在/tmp写新探针与合成数据库。只复用 `/private/tmp/r06-ex09-target`，没有复制artifacts或新大worktree，没有新建target。
- 本报告只覆盖adapter，不覆盖Root后续修正或service HTTP/Run/费用链。未读取真实业务HOME数据；未覆盖HOME变量，LINGXI_HOME/TMPDIR均合成。

## P2：扩展汉字查询退为空条件，真实返回无关资料

冻结源码 `tokenize_search_text` 933–934行使用 `token.chars().count()==1` 过滤CJK；当前TS `lib/search/search-text.ts` 的 `token.length===1` 按UTF16长度判断。`𠀀`（U+20000）/`𰀀`（U+30000）在Rust为一个标量，在TS为两个code unit，现役TS保留而Rust删除。Rust FTS literal因此为空；`search_fts` 3092–3096行把空条件回落全变体前N，实际召回无关资料。

独立执行当前TS（Node24.16.0、真实@node-rs/jieba）与冻结Rust生产函数，20组自选输入有4组不一致：`𠀀`、`𠀀上海`、`上海𠀀`、`𰀀`。NFKC结果一致，差异在tokens、literal、派生document。其余16组一致。原作者7组fixture另用现役TS实际重算，7/7相同；原fixture不是伪造，但未覆盖单扩展汉字。现役TS源SHA `c831ea28a1c0b65c6de1b0d6ba3b3517020582f7b5f7d31de3205630c165c687`，本轮未变。

真实SQLite反例：同库只写目标chunk `𠀀` 和无关chunk `unrelated quartz`，同一生产store API：

```json
{"actualCurrentTsLiteral":"\"𠀀\"","query":"𠀀","rustHits":["decoy","target"],"rustLiteral":"","status":"CONFIRMED_FUNCTIONAL_FAIL","tsLiteralSameDatabaseHits":["target"]}
```

这是实际当前TS生成literal与实际冻结Rust生成literal，在同一实际SQLite/生产store检索的正负对照，没有mock搜索器/分词器/存储，也没有手写预期分词参考。

证据：

- 自选语料 `/tmp/r06-ex09-independent-tokenizer-corpus.json`；当前TS脚本 `/tmp/r06-ex09-independent-ts-reference.mjs` 与输出 `.json`；Rust源码 `/tmp/r06-ex09-independent-rust-reference.rs` 与输出 `.json`。
- 四差异 `/tmp/r06-ex09-independent-tokenizer-diff.json`；原7组现役TS重算 `/tmp/r06-ex09-independent-original-ts-fixture-check.json`。
- SQLite反例源码 `/tmp/r06-ex09-independent-single-han-sqlite.rs`，日志 `/tmp/r06-ex09-independent-single-han-sqlite.log`，实际数据库目录 `/tmp/r06-ex09-independent-single-han-sqlite`。
- 探针退出0表示成功断言了坏行为和正确对照，**不代表功能通过**。

最小修复方向：对齐当前TS UTF16长度口径；补单扩展Han/混合Han实际参考和真实SQLite仅目标命中；用户给了文本却无可搜索词时，不能把空条件退为全部来源。不得改弱原golden/TS参考。依据：[ECMAScript String length](https://tc39.es/ecma262/multipage/text-processing.html#sec-string-instances-length)、[Rust encode_utf16](https://doc.rust-lang.org/std/primitive.str.html#method.encode_utf16)。

## 独立实际回归

命令：

```sh
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=/private/tmp/r06-ex09-target LINGXI_HOME=/tmp/r06-ex09-independent-runtime/home TMPDIR=/tmp/r06-ex09-independent-runtime/tmp /Users/study_superior/.cargo/bin/cargo test --manifest-path /private/tmp/lingxi-r06-ex09-adapter/rust/Cargo.toml -p lingxi-adapters --locked --test knowledge_store_r06t07 --test r06_ar_f12_cache_preservation --test r06_ar_w09_vector_integrity --test r06_ex09_search_cache -- --nocapture
```

实际exit0：9+22+9+11=51 PASS，0 failed/ignored。日志 `/tmp/r06-ex09-independent-regression.log`。原三个测试文件与Main逐字节相同，未删除/弱化原断言。F12权限、锁、future-WAL/旧句柄保护实际运行；测试主动注入的panic子例由其真实断言捕获，不当作失败掩盖。

另用相同cargo1.98.1/--locked/外target执行kernel `--lib golden`，4 PASS，其中knowledge原identity/config两个golden实际PASS；另两memory golden也被筛选匹配，未改这些测试。日志 `/tmp/r06-ex09-independent-goldens.log`。补丁不含kernel或旧golden修改。

## 旧缓存与旧收费向量边界

静态及新11实际回归确认：

- algorithm标记/application_id独立于事实库与原profile/civ/viv算法。只对application_id=0/缺标记且可识别旧schema及负载登记LegacyStale；未知/坏标记、坏旧向量、future schema明确拒绝并保留，不假装升级成功。
- 旧变体status有效投影stale，fingerprint不能准入，ready/list_ready不读旧值，直接FTS/chunks/checkpoint/vector请求旧代际明确拒绝。
- 同旧paid身份begin明确拒绝；新cache profile产生不同civ/viv，新checkpoint为空，旧向量字节保持。完整replace事务成功后标当前，失败不能升级旧tag或半清旧chunks。
- cleanup同步清代际记录，重开维持当前/过期分隔；损坏元数据不能在活句柄上混过读/写。

我另写真实SQLite边界探针 `/tmp/r06-ex09-independent-cache-boundaries.rs`，日志 `/tmp/r06-ex09-independent-cache-boundaries.log`，新数据库 `/tmp/r06-ex09-independent-cache-boundaries`。使用冻结旧生产SQLfixture恢复真实库，并通过正式API创建当前namespace：

1. 当前变体正控真命中；新旧混合FTS集合两种顺序都整体拒绝，包括空query，不泄出部分正常结果。
2. 当前向量正控可检索；新旧混合vector集合两种顺序都拒绝；旧begin/checkpoint拒绝，旧收费向量每一行hex完整保留，新checkpoint从空开始。
3. 真关闭/重开后，同artifact旧stale与新ready并存；ready只给当前一项，旧checkpoint仍拒绝，新checkpoint只含新写行。

三项PASS。这里是实际adapter向量行，不能冒充真实远端付费调用。旧生产writer的作者生成证据/源SHA已读，未重新运行作者旧writer；我独立恢复并执行了真实SQLfixture。source fixture原文含上海，而旧chunks无上海；原新11通过真实kernel反向overlap重建后召回上海/quartz，原快照与引用原字节检查均实跑。

## 依赖与限制

独立比较基线/最终Cargo.lock：(name,version)原条目删除0、新增13，与作者说明一致。jieba0.6.8内嵌词典、NFKC和固定regex初始化在源码接通；没有吞分词失败返回旧算法的降级分支。新依赖不改动service/kernel接口所有权。

本轮未发现另一个可确认的旧缓存绕过/付费checkpoint误复用缺陷。服务层必须统一使用current_cache_profile_hash，旧cache提示、显式rebuild、无current-ready时不能偷偷发query embedding费用等均由Root接线与真实HTTP/费用联合验证，不在本报告签PASS。未执行其它平台、真实外供应商、全产品门禁或正式包；不登记EX09整体完成、R06_ACCEPTED或R07_READY。

机器结论 `/tmp/r06-ex09-adapter-independent-confirmation.json`。本轮原FAIL输入/日志保留，后续修复必须新候选另报告。
