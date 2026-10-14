# R06 Dream 独立审阅 R1

结论：NEEDS_FIX。候选的大部分手动业务核心成立，原新增 13 项与原记忆 7 项独立复跑全通过；新增 3 项对抗检查中 2 通过、1 确认真实回归。因此不能给本候选最终核心 PASS，也不能据此关闭三个 Dream 叶。

## 候选与隔离

主当前 Rust/contract 文件仅只读快照，独立副本 `/private/tmp/lingxi-r06-dream-review`，基提交 `2c8f8ddd0`，应用净增量 SHA256 `f8ec25d04dbd2c7d4b4623bed25790e12b7a9c0e8fe4d6c0c3745c79f220dbff`。主工作树、原断言、门禁、白名单和台账没有修改；未提交推送。候选 429 个 Rust 文件运行前后 SHA 全相同；额外独立测试单独新增。完整清单 `/tmp/r06-dream-review-source-sha.json`；关键七文件清单和机器结论 `/tmp/r06-dream-independent-review.json`。

指定 cargo 1.98.1、--locked、合成 HOME/TMPDIR/LINGXI_HOME；RUSTUP_HOME/CARGO_HOME 只指已装工具缓存；target 在 `/Users/study_superior/.cache/lingxi-r06-builds/dream-independent`。每个产品模板的源与写入为 fixture 自有 product，未写仓库 lib 或全局产品目录。模型替身仅外部 HTTP SSE 供应商；真实 NetworkPlane、ModelGateway、权限、业务门、SQLite 用量、磁盘写入、上下文消费者、生命周期全部保留。

## 确认问题 DREAM-R1（P2）

位置：`rust/crates/lingxi-service/src/agents_admin.rs:406`，`delete_agent` 在实际 `remove_dir_all` 前调用永久 `stop_agent`，错误路径没有恢复。

独立测试 `review_failed_agent_delete_must_not_permanently_stop_live_core`：对真实 agentA 目录设 macOS uchg，只阻止实际删除，不伪造删除结果。HTTP DELETE 返回 500 `Operation not permitted`；解除 uchg 后目录和 config.yaml 仍真实存在，但 HTTP status 与 run 均 503 `dream_core_stopped` / `coreAvailable=false`。当前服务没有恢复该存活助手的入口，随后手动整理被永久拒绝。另一助手保留。测试在断言前真实解除故障并清理自己的临时数据。

需要修复失败删除与停止的仲裁：防止成功删除后的后台重新建目录，同时删除失败且助手仍在时，恢复该助手现役核心、保持真实付费结算和错误状态。不应直接把 stop 移到删除之后而重新打开目录重建竞态。新增通过证据应包括磁盘拒删后的读/手动整理可恢复、进行中删除成功与付费结算、最后助手拒绝保全、B 助手不受影响。

日志 `/tmp/r06-dream-independent-experiments.log`；探针完整源 `/tmp/r06-dream-independent-experiments.rs`。实际执行 exit101，2 passed/1 failed/13 filtered out，8.36 秒；不将它写成 PASS。

## 已验证范围

原 `r06_dream_completion` 13/13，43.69 秒；原 `r06_t06_memory` 7/7，0.86 秒，exit0，日志 `/tmp/r06-dream-independent-original.log`。包含五阶段模型、生产修订/当前正文/真正下一 Run、真实账本、忙冲突/跨助手、关闭后付费取消和重启用、源变化/无效回答、真实服务重启、显式取消、uchg 多文件失败与恢复、混合配置已发生的前段保存、空/无变化、停止/无模型可用性、实际删除与实际服务退出。

额外独立 `review_last_agent_refusal_preserves_pending_dream` PASS：先真实删除 B，仅剩 A，A 外部供应商挂起期间真实 DELETE A 400；状态仍200，释放供应商后完成五调用并 succeeded。证明拒删最后助手没有提前停止当前核心或丢掉其进行中整理。

额外独立 `review_paid_week_source_race_then_real_retry` PASS：五阶段首调用真实进入供应商待答期间仅改 today.md；付费回答结算后整理失败、facts 不变、后改 today 保留、没有新修订；账本所有实际返回调用均保留 usage。再经真实入口重新整理 succeeded，today 后改仍保留。证明只读来源同样进入提交冲突保护，真实重试可用。

## 叶范围与限制

76EE734926B4 / A9AD4C7F76AE / B45ECEC543C6 的手动核心、状态与修订接口成功/拒绝条件已有实际证据；仍因 DREAM-R1 不能签本候选最终完成。

0B483D7F6A43 仍 PARTIAL。日期字段、memory.enabled 实际保存、进行中手动整理取消及启用后再运行成立；候选没有自动 ticker 调度宿主，所以 `tickerPaused` 布尔值和手动恢复不能代替原文“暂停 ticker；恢复 ticker”。不能删原断言、扩充延期治理、以 R07 标签免除整叶义务；需后续真正实现自动运行的暂停与恢复后再独立验收。

未运行整阶段门禁、其它平台、正式打包、真实供应商；费用为真实本地账本和模拟供应商响应的报告 token，未证明供应商实际账单。usage 存储失败后的恢复未做本轮运行验证。已经有明确新反例，本轮不扩大无关验证。
