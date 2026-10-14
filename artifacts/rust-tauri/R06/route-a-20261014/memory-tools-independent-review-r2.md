# R06 三项记忆工具独立复验 R2

结论：本次受审补丁 PASS。此前 A17D742E55B7 的“成功并发记录被覆盖”阻断缺陷已修复。三项原叶在本次审阅范围内无未解决阻断；这不表示 R06 阶段验收通过。

审阅对象为独立基座 2c8f8ddd0b75b4de61ebd047a0246a7f88f6d486，加原工具补丁 4a15522e6eef8db43448ae35e87f199f80cb11f332bfe5a7719b1b5c5b73c4ec，再加并发增量 333f0b1b2425336f0c4f02a1ff24f840da77d9243a50d432a606bf662d48d3e4。主控纯类型/词法整理未复制进审阅树，故本结论绑定 JSON 中的独立实际源码。

## 已证明修复

1. 我此前保留的16线程真实 registered record_experience 并发反例，在保持原断言的独立副本中重跑 PASS：1条实际成功、15条明确 Conflict/retryable=true，成功条目与之前5条顺序正控均保留。
2. 我在独立副本追加更强检查，保留原断言，要求至少一条成功、每个拒绝只允许明确可重试 Conflict、每个成功项都进入真实索引且通过真正注册 recall_experience 再读回。追加后实跑 PASS，同样1成功/15明确拒绝。不存在拒绝全部请求的虚假通过。
3. 对增量静态检查：record_entry 的读取、正文发布、内部索引发布和失败恢复均在共同助手 Maintenance 操作门中；sync_categories、独立 rebuild_index 共用同门，内部 unlocked 辅助避免再次取门。首次创建 memory 前后及助手路径别名统一锁键。
4. 作者新10例由我独立全部重跑 PASS，包含原6个审批范围/真实语义模型与降级/经验隔离失败/预算历史用例，以及新增并发、三入口同门、实际晚期索引 IO 失败恢复和路径别名。实际失败恢复维持旧文档/索引字节，并允许另助手独立成功。

## 相关回归

独立正式 Cargo 1.98.1、--locked、合成 HOME/TMPDIR、仓库外 target：
- r06_memory_tool_completion：10 PASS。
- r06_original_memory_skill_proofs：19 PASS。
- r06_t06_memory：7 PASS。
- r06_ar_f08_projection：8 PASS。

合计44个不同相关用例；我自己的原反例与增强副本另外2次各1 PASS。审批确认期间范围变更仍拒绝删除；真实网络嵌入配置/未配置/覆盖/超时状态仍通过；大原生 details 与正文共享32KiB投影，raw完整记录、正式内部后续 Run 和历史恢复仍通过。diff空白检查通过。

## 证据边界

R1 的真实 HTTP 实验结论仍有效：宿主 details 在正式 wire/存档/内部 ModelTurnInput 中存在；当前外部模型 HTTP 请求只发送 content。原 TypeScript details 亦为宿主元数据，所以本轮不要求将全部 details 推给模型，也不把元数据发送当成本叶通过条件。增量未改变消费路径，交付说明已纠正范围。

同助手锁保护同一服务进程通过生产 Store 的 record/sync/rebuild/回滚，未声称阻止任意外部文件写者。未执行全阶段门禁、真实账户供应商、其他平台或打包；主控仍须对整合候选进行正式绑定和阶段门禁。

## 稳定材料

目录：/tmp/r06-memory-tools-independent-evidence-r2
- review-r2.json：准确补丁/源码/日志 SHA256、工具链、环境、44例结果、stageAcceptance=false。
- r06_review_memory_tools.rs：独立增强反例稳定副本，SHA256 5f5074b6648bbe4079572679a3863ff06892d9886604d04cd29df70ee1077163。
- r06-review-memory-r2-original-counterexample.log：原反例原断言通过。
- r06-review-memory-r2-enhanced-counterexample.log：增强版索引/真实召回及明确Conflict通过。
- r06-review-memory-r2-regression.log：完整44例通过。

复验命令：
`LINGXI_HOME=/tmp/r06-review-memory-env/home TMPDIR=/tmp/r06-review-memory-env/tmp CARGO_TARGET_DIR=/tmp/lingxi-r06-memory-events-target /Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r06_review_memory_tools independent_concurrent_record_successes_are_all_recallable -- --exact --nocapture`

独立测试文件也保留 R1 HTTP 字段边界探针，那个探针仍预期失败，用于说明元数据与实际 HTTP 正文不同；没有执行它作为本轮产品门禁。
