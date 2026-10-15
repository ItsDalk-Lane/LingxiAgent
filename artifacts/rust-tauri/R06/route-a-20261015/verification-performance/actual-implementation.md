# 最终实际实现只读复核

结论：当前Main三文件限定静态PASS。并行只改变计算安排，Arc只去重完整重新校验后相同的只读条目；没有判定/白名单/范围/检查次数变化。未修改Main、未构建或运行测试。

- rust/crates/xtask/src/candidate.rs SHA256：`088b28be7aeca03e564c57ee9c9723b4c2a07035cefdfe8b27c7e05de5895a8d`
- rust/crates/xtask/src/main.rs SHA256：`733a55e2a32280738a53d546bd8ed4c953676dae5cfca80127d0596e2a88b807`
- rust/crates/xtask/src/candidate/tests.rs SHA256：`7977183be6232e1cc55f94662f043a970b398f68cf76a47d34dee41ccae34000`

实核secure hash及其辅助函数尾部逐字不变，原最终摘要聚合循环逐字不变，旧9测试正文作为完整字节前缀保留。main仅在原after_command的Scope.snapshot已完整返回后map成功结果，双条件digest+全部Entry逐项相等才Arc::clone baseline；Err保持Err，不同条目保留独立Arc，所有checkpoint照常push。最多4线程，全部显式join后返回。

新增测试实核了同digest但不同条目SHA不共享/JSON不变，真实修改独立保留，恢复后旧changed checkpoint仍拒绝，以及未跟踪文件消失不复用旧结果。旧错误/链接/模式/原始路径等测试没有减少。

Root日志124全过及targeted1通过已只读查看，不冒称本审阅者执行。Root最终94700条全manifest与旧串行逐JSON相等、18.427秒→9.531秒实测记录已归档；这是快照约1.93倍，不是全阶段倍数或正式验收通过。真实变化时不同快照仍会保留，最坏内存不作固定承诺。

逐项确认与diff见actual-implementation.json及actual-implementation-reviewed.diff；最终正式结论仍由新候选真实阶段证据决定。
