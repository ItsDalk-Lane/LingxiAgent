# 第二批 memory tools 交付

工作树：`/tmp/lingxi-r06-memory-events`，起点 `6d1933279`。独立差异基线：`/tmp/lingxi-r06-tools-baseline`；memorytools/memory含主控第一批和notes基线。未提交、未推送；主仓库只读。

补丁：`memory-tools.patch`，SHA256 `4a15522e6eef8db43448ae35e87f199f80cb11f332bfe5a7719b1b5c5b73c4ec`。当前主树正向check及独立树反向check通过。`changed-files.txt`列26个文件，`source-sha256.json`绑定各源文件，`exact-tests.json`绑定六个精确用例结果。

- 0779DAF64B03：`unpin_review_actual_scope_structured_counts_history_and_agent_isolation`、`unpin_changed_matches_during_confirmation_refuse_all_deletions`。真实Run审批、Operate与既有预授权仍逐次review；实际ID/正文/助手范围展示；拒绝及确认期间范围变化不删除；实际存储删除只匹配active，单项/多项/零项结构字段真实；非active及其他助手未变；wire、存档、下一消费一致；旧无details消息仍恢复。
- 7432B00DEC94：`memory_search_status_no_hits_not_searched_and_agent_channel_isolation`、`memory_semantic_real_provider_no_model_no_coverage_timeout_and_actual_vector_hits`。真实标签、FTS、向量余弦加RRF，时间/标签/原始事实结构；实际模型配置、嵌入覆盖与2秒超时驱动明确状态；供应商返回无效向量明确降级；无命中与未检索/空库分开；跨频道仅显式开启，当前助手强校验。
- A17D742E55B7：`record_and_recall_registered_tools_preserve_failures_and_agent_boundaries`。生产注册、实际经验文件与索引写后可读，重复记录added=false；无效类别、跨助手/可信运行助手不匹配拒绝，目录禁止写与索引发布故障保持原文件/索引，暂停时不写；实际Run的wire/history/后续模型输入一致。
- 跨层必要预算：`large_native_details_share_projection_budget_keep_raw_and_restore_same_projection`。完整raw结果和文档均保留；正文渲染+native详情共享原32KiB成功投影上限，截短明确标记并保留字段类型/计数；真正历史及第二次Run使用同一投影；注入原大details、缺新计数、谎称未截断的负控均拒绝。真实HTTP模型适配器原来仅渲染正文；details在正式运行输入/存档作为原生业务数据保留，仍一并纳入预算，避免将来/其他消费者变成旁路。

六个新用例最终精确执行均成功。相关既有回归累计704个用例均成功：service单元402、审批11、原knowledge19、主控当前原memory19、memory7、skills6、knowledge tools6；kernel/protocol212、adapter14、原F08 8。不同回归日志来自本批开发阶段，最后可信助手review范围增强后六个新用例再次执行。原F08测试内容未变；原memory回归副本不随补丁交付。原compaction与R04测试仅新增结构字段初始化，断言未改。

合同生成命令（同一绝对manifest及--locked）：
`/opt/homebrew/bin/cargo run --manifest-path /tmp/lingxi-r06-memory-events/rust/Cargo.toml --locked -p lingxi-protocol --bin lingxi-protocol-gen`
检查追加 `-- --check`，56生成文件一致。实际变更6生成文件在changed-files清单。TS/schema字节往返12个golden、Rust verify12个及TS编译均通过。相关日志附在目录。

运行环境全部合成：LINGXI_HOME=/tmp/lingxi-r06-memory-events-env/home、TMPDIR=/tmp/lingxi-r06-memory-events-env/tmp；CARGO_TARGET_DIR在仓库外/tmp。测试替身只用于外部模型/嵌入HTTP供应商，生产注册、网关、审批、存储、运行和历史读取均真实。未执行正式供应商账号、其他平台或打包验收。

adapter仅FactRow/FactStore检索向量、Tenets严格读取与已审阅范围删除、Experience严格读取/原子写索引及失败恢复；不涉及reset_and_clear或reset_and_clear_all_facts。memory仅搜索embedder字段/接线，不涉及主控删除路由或总结来源清理。runs仅审批摘要/Review标志及native详情/投影计数字段，无其他流程改动。未改登记、门禁、台账、.sync-audit或白名单。所有三叶条款已有实现和可执行证据；正式绑定与全量候选验收交主控。
