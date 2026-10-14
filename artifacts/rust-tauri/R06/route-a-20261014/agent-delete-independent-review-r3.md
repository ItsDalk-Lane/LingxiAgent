# R3 助手可恢复删除独立审阅

结论：NEEDS_FIX。正常删除与既有半删故障的修复证据成立；收据恢复存在新确认问题，不能给最终 R3/EX04 核心或三个 Dream 叶总体 PASS。

## 候选与隔离

在原 f8ec Dream + R2 29fac9 独立副本 `/private/tmp/lingxi-r06-dream-review` 应用净增量 `939049013ed797cc84bc0d32756f925b86cb3fb14f751cb68653e23159bf1aed`。作者所有最终候选文件 SHA 逐项一致，保存的 Rust 文件运行前后 SHA 全稳定。清单 `/tmp/r06-agent-delete-r3-independent-source-sha.json`；机器结论 `/tmp/r06-agent-delete-r3-independent-review.json`。

cargo 1.98.1、--locked、CARGO_INCREMENTAL=0，合成 HOME/TMPDIR/LINGXI_HOME，仓库外复用 target；每个 fixture 产品材料位于自有临时 product。只替代远端供应商 HTTP，实际生命周期/权限/模型调用/SQLite/磁盘/重启/业务门不 mock。主树、原断言、判定规则、登记和门禁未修改；未提交推送。

## 确认问题 DELETE-R3-1（P2）

位置：`agent_deletions.rs::recover_one_deletion`，当前只有 prepared 状态核对公共助手与暂存 data 两处实际目录；committed/completed 状态信任收据里的 logicalDeleted 后直接 stop_agent、修引用、清暂存。

独立真实前置：顶层 uchg 使实际 rename 失败、DELETE500，生产写出 aborted 收据且公共助手/配置/所有 memory 保留，data 从未存在。解除真实故障后，明确把该收据损坏为结构可解析的 committed/logicalDeleted=true/cleanupPending=true/referencesPending=true；这是收据损坏故障注入，不是正常成功 rename 的伪证据。随后通过真实 HTTP POST cleanup：返回200、status=completed、ok=true、logicalDeleted=true；公共 A 仍真实存在，原 facts 字节未变化，但 A 整理 status 永久503/dream_core_stopped。恢复把矛盾的收据当实际删除成功，错误停用仍现役助手。

必须在所有声称逻辑提交的状态中先验证公共助手目录实际不存在；有公共助手/目录型符号链接或其它矛盾时，应在 stop/引用修改/清理之前显式拒绝恢复，不得签 completed。启动恢复、GET 收据推导、POST 重试应保持相同实际状态判定；真正清理完成后的 data 不存在是合法，与公共助手仍在须区分。建议保留本独立断言做修复前后同候选验证。

独立测试 `review_r3_inconsistent_committed_receipt_cannot_stop_or_delete_live_agent` 实际 FAIL：预期拒绝500，实际200；独立日志 `/tmp/r06-agent-delete-r3-independent-experiments.log`；完整探针 `/tmp/r06-agent-delete-r3-independent-experiments.rs`。运行 exit101，1 passed/1 failed，5.37秒。

## 已成立证据

- 原新增 R3 7/7（20.09秒）、Dream17/17（52.59秒）、Memory7/7（0.61秒）、原 agent生命周期2/2（0.58秒），共33，完整运行 exit0。日志 `/tmp/r06-agent-delete-r3-independent-original.log`。
- 独立 `review_r3_original_top_only_uchg_preserves_every_original_memory_byte` PASS：仍只设顶层 uchg，DELETE500；配置/facts/longterm/memory/today 每项真实字节全保留，解除故障后真实新 Dream 202并五阶段 succeeded。没有将故障改为全树flags，没有削弱完整数据断言。旧半删反例原文件完整保留，未改旧负面复现来假造通过。
- 原7项复跑覆盖真实 pending清理、半清后失败的明确逻辑删除、费用不丢且公共 A不重建、整个服务重启/真实清理重试、并发双删/最后助手拒绝、rename后真实checkpoint写失败及 prepared+data恢复、settings scopes/穿越、preferences磁盘失败引用pending及实际修复、首次收据前临时残留安全清理。
- 原Dream17覆盖五阶段来源与下一真实消费者、启停付费结算、失败核心可恢复、B独立、最后助手拒绝保全、真实服务退出；此次没有新的该范围失败。

## 准确限制

已有明确坏收据反例，本轮不扩大其它对抗窗口；pathlink/坏收据的全部同族、无收据含data异常、更多同ID创建交错未独立新增运行，不能用原7项替代未执行部分。

0B483D7F6A43 自动 ticker 宿主/真正暂停恢复仍 PARTIAL；本轮不改变此原义务。未运行整阶段门禁、其它平台、正式打包或真实供应商账单。R3正常半删故障修复成立，不抵消本轮收据恢复 FAIL。
