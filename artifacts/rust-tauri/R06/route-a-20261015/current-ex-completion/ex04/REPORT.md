# EX04/05/06 独立第一轮核验

EX04 当前手动整理核心 PASS_SCOPED；EX05 NEEDS_FIX 已授权私树修复；EX06 NEEDS_FIX 已转另一作者。没有使用旧 SOURCE_ONLY 推定未实现，没有改 Main、门禁、登记或原断言。

EX04：当前 DreamService/MemoryTicker/MemoryService/agent_memory 与独立树逐 SHA 相同，原 r06_dream_completion 全 17/17 PASS（52.82 秒），包括五阶段、真实供应商 HTTP 替身、真实用量与下一消费者、配置关闭、助手隔离、取消费用保留、重启不重付、真实磁盘回滚、删除/退出生命周期。日志 dream-current-original.log；源码清单 source-sha-before.json。此处不宣称跨平台、实际付费供应商或正式封印。

EX05：restore_second_file_io_failure_preserves_every_current_byte 强断言原样 FAIL。真实 today.md uchg 使第二段写入 EPERM；facts.md 和 memory.md 已换旧内容，today/longterm仍当前；没有 pending-apply。原绑定500例只把目标修订变目录，失败在读阶段。原探针 restore-second-write.rs；原失败 restore-second-write.log；原事实 restore-second-write-facts.json（从未改写的原日志精确恢复同 JSON 内容），源 agent_memory SHA ff27cdb1d8f9bfdce37f9d28821fcb736a938586415f29bf8a1d4a16549bdfbe。修后交付另目录 /private/tmp/r06-ex05-delivery，不以此报告替代复核。

EX06：当前搜索生产 hunk 与独立树一致；整文件差仅 Root 后加 GitHub 安装，已在清单显示不同，未假称整文件相等。真实已配置全局 embedding、助手没配 memory.embedding_model，调用当前注册 search_memory 仍真正向本机供应商 HTTP 发1次；按现役 core/engine.ts 2591-2620 应 unavailable_no_model/0次。强预期0、实际1 FAIL，semantic-agent-choice.log / no-agent-embedding-facts.json。原因是 bootstrap 将同一 GatewayKnowledgeEmbedder 全局注入记忆，后者只读 models.embedding，未读 agent ref。另“助手ref有/全局无”样本0调用也观察到，但其显式pair未额外证明实际catalog可解析，只作辅助，不能据其断言有效配置正控。helper filter还顺带运行现有并发记录1 PASS，不当作新增充分覆盖。原 semantic/record/recall/lesson 现行实现、隔离和真实下游已存在，不能沿旧台账称恒 not_configured。

完整原条款保留 original-clause-map.json。未执行其他平台、完整界面或全阶段验收。
