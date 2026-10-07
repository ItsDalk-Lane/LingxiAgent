# RR3 STORAGE-02 精确本轮共享负测增量缓存准备
你是全新辅助执行者，先全文读RR3_STORAGE_BRIEF、STORAGE-01/REPORT.md及receipt、最新HANDOFF、G-INTERRUPTION-01报告（若已完成）、TASK0中断receipt。本轮范围仅 /Users/study_superior/.cache/lingxi-r05-neg-target/debug/incremental 中确证属于本轮RR3默认G构建的可重建中间缓存；初始约8.3GiB。上一储存轮未获该范围，不是其遗漏。H02已明确完全不使用NEG_TARGET/negcopy，只用主rust/target；G旧进程已结束/中断，新G未开始，J只Node小自检不构建Rust。主target、deps实际binary、源码/用户文件/旧证据/本轮各包manifest列出的原件均禁止修改。

先用真实ps/lsof确认无人使用目标，核G原commands/CARGO_TARGET_DIR/实际RR3隔离copy路径和完成UTC，incremental会话时间及crate来源；只凭文件大或mtime新不足归属证明。逐项结合实际编译记录确定仅当前RR3生成范围。历史R01/RR2或无法证明归属不碰。扫描历史manifest/digest/sha对路径/名字/内容引用；被要求保留的原件不删。保留所有实际二进制和原始日志/序列/输入与所有sourcecopy，不动NEG_TARGET根/依赖库/binary，不改cargo配置或用户内容。

可删除的仅明确归属且未被证据要求保留的incremental非可执行中间数据。逐文件前hash/size/归属/引用核查/活跃复核，删除后保留对象前后hash相同、实际可用空间与真实释放记录。优先拿到约6GiB可用（clone3.5GiB加合理编译余量，非伪实测峰值）即止。不要删除全部8.3GiB或cargo clean；不足就事实报告，不扩大范围。

只写新RR3/STORAGE-02报告/结构化receipt/必要操作脚本，不写生产/docs/Git/系统、不构建、不派代理。原STORAGE01结果和历史摘要永久保留。任务完成停写交总控下一G准备。
