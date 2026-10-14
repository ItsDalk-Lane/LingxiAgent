# R4 可恢复删除独立复核

结论：PASS_SCOPED_DELETION_RECOVERY。本轮 R3/R4 删除恢复范围通过；前次坏收据误停现役助手问题已修复，原半删磁盘故障保全仍成立。不给整阶段、未实现 ticker 或 Main 集成后的最终验收授权。

## 候选与原证据保全

同一独立树 `/private/tmp/lingxi-r06-dream-review` 在 R3 上干净应用 R4 净补丁 `e420a319cdeba28eacbef2f01e2cd5ffccfded2d1cb3087d0547e191e49b43f5`；冻结七候选文件 SHA 逐项与作者一致。已保存的全部 Rust 源运行前后稳定，清单 `/tmp/r06-agent-delete-r4-independent-source-sha.json`；机器结论 `/tmp/r06-agent-delete-r4-independent-review.json`。

原 R3 两个独立探针全文逐字未改，SHA `9078a63f82f5c13ad009e114f3a2ae93ab5e14391dfa08f10c0e850c0ee7ab92`。旧半删负面复现和历次报告均保留，不覆盖旧 FAIL。生产正式 R3 测试未削弱，R4 新增正反控制在尾部。

指定 cargo1.98.1、--locked、CARGO_INCREMENTAL=0；合成 HOME/TMPDIR/LINGXI_HOME，已安装工具缓存，仓库外复用 target；所有产品材料为 fixture 临时自有目录。只用 HTTP 供应商替身，未 mock 数据存储/权限/业务锁/使用费用/运行生命周期/实际磁盘/重启。不改 Main、台账、门禁或白名单；没有提交推送。

## 本轮实际验证

受影响删除 suite10/10 PASS（29.39秒），原独立探针2/2 PASS（5.53秒），新增整批恢复正反控制1/1 PASS（2.61秒），三个实际运行均 exit0。

- 原坏 committed 收据反例：公共 A 仍在，staged data不存在；cleanup现在500显式 `contradicts actual public/staged agent data`，原 A status200/coreAvailable=true。未停用现役 A、未假写 completed。日志 `/tmp/r06-agent-delete-r4-independent-experiments.log`。
- 原顶层 uchg 正文保全探针：真实 DELETE500，config/facts/longterm/memory/today 全部原字节保持，解除故障后真实新 Dream 五阶段 succeeded。没有把顶部故障换成全树flags。
- 删除10项：实际 cleanup失败/半清后的 logicalDeleted/pending、原已付费响应保留和公共A不重建、整个服务 restart/实际重试、并发双删/最后助手拒绝、真实rename后checkpoint发布失败及 prepared恢复、settings权限/穿越、真实prefs失败后的pending/重试、首次收据前临时残留、committed/completed坏收据及真正bootstrap拒绝、public symlink/普通文件不碰目标、completed矛盾data拒绝，以及合法 committed/data清完未checkpoint窗口的真实restart正控制。日志 `/tmp/r06-agent-delete-r4-independent-original.log`。
- 新独立 `review_r4_batch_refusal_preserves_prior_pending_then_actual_retry_completes`：A真实因prefs uchg而逻辑删除、refsPending且完整data暂存；另在后排序位置明确人为注入引用仍公开 B 的坏 completed 收据。真实整批恢复拒绝时，前面的 A 暂存facts、收据、prefs与坏收据全部逐字不动，B核心仍200。移除独立坏前置后真实POST A cleanup completed，prefs真切B、暂存清理完成，B真实新 Dream五阶段 succeeded。证明整批先验证不是仅单条回归；该坏前置没有伪称正常rename结果。日志 `/tmp/r06-agent-delete-r4-independent-batch.log`；源 `/tmp/r06-agent-delete-r4-independent-batch-experiment.rs`。

源码确认：统一 validate_deletion_materials 经读取、恢复、GET、settle和真实rename后永久停止前调用。非缺失路径必须真实目录；committed公共目录必须不存在，completed两处都不存在，aborted不能带data，prepared两处恰好一处存在。committed data清理完成但checkpoint未发布允许恢复。startup先读验全部收据后再执行任何该批停止/refs/清理，不因后面的坏记录先消费前面的正常pending。

## 验证边界

本轮只重跑受 R4 影响的删除场景及独立反例/正控制；未重复不变的完整 Dream17/Memory7/agent生命周期2。其原 R3 独立33通过证据保留，R4删除10另覆盖本次新增调用链；没有宣称本轮独立执行了作者36项。

独立探针保留原fixture辅助定义，编译有 unused 警告；未改原探针消警告，也不自称本轮探针strictClippy通过。正式候选strictClippy为作者报告，本次未另重复运行。

范围仅删除完整性、恢复与相关手动Dream生命周期。0B483D7F6A43的真正自动 ticker 暂停恢复仍 OPEN/PARTIAL；Main整合、全部原义务和整阶段门禁仍需主控按实际候选完成。未验证其它平台、正式包、真实供应商账单或持续恶意外部writer的所有竞态。
