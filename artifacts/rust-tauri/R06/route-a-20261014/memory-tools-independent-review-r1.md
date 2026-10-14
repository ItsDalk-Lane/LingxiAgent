# R06 三项记忆工具独立审阅（2026-10-14）

结论：需要修复。0779DAF64B03 的真实范围确认和可信助手校验、7432B00DEC94 的真实嵌入状态路径未发现阻断问题；A17D742E55B7 尚不能标记完成，真实并发经验记录会丢失已报告成功的数据。

审阅对象：基座 2c8f8ddd0b75b4de61ebd047a0246a7f88f6d486 + memory-tools.patch，补丁 SHA256 4a15522e6eef8db43448ae35e87f199f80cb11f332bfe5a7719b1b5c5b73c4ec。独立树 /tmp/lingxi-r06-review-memory-tools。主树、门禁、原断言、登记和提交均未修改。

## 阻断发现：成功记录被另一并发记录覆盖

位置：rust/crates/lingxi-adapters/src/storage/agent_memory.rs:1854–1913（record_entry），及1818–1850（rebuild_index）。整个读取原文、生成新文档、发布文档、发布索引和失败回滚没有同助手操作锁。atomic_write只让单次文件替换完整，不能保护整个读改写和双文件事务。

真实反例 independent_concurrent_record_successes_are_all_recallable：先经生产存储顺序写入5条正控，再用16个真实注册网关在独立线程/合成运行中同时调用同助手、同类别的record_experience。真实 prepare/execute/可信agent/实际文档与索引全部经过生产实现，未模拟工具、存储或网关。仅收集实际返回Success且added=true的条目，重读正式经验文档逐项核验。

- 第一次受控运行：15条返回成功，1条失败，最后文档只保留一条新增，14条成功记录无法召回。
- 实际HTTP证据同窗运行：15条成功，1条失败，文档最后竟只保留失败调用的内容，15条成功记录全丢失。
- 最终独立重复：16条全部成功，文档只保留一条新增，15条成功记录丢失。顺序5条正控全部保留。

这违反“写后可召回”及“失败不破坏旧经验”；失败回滚亦可能覆盖别的调用刚发布的成功结果。应在同助手经验变更的整个读取/发布/索引/回滚区间保持共同锁或等价串行化，内部索引辅助调用避免重复取锁。并发请求明确返回Conflict可接受，但每个added=true必须可召回，文档与索引一致。修复后对同一个反例复验。

## 实際模型 HTTP 边界：证据限制，不另扩展产品要求

independent_unpin_details_reach_actual_http_rendering_boundary 先跑真实Run和真实删除审批，证明正式结果确实有removedCount、removedItems和id，然后将实际捕获的ModelTurnInput交给正式OpenAiCompletionsAdapter.execute_chat。只对注入供应商原来缺少的关联ID补上真实HTTP适配器必需的关联身份；未改业务结果。外部本地HTTP替身收到请求，正式SSE响应经适配器解码为Final（正文正控成功）。

HTTP工具结果实际为：
`{"content":"Removed 1 pinned item(s): 删除正文独有标记","role":"tool","tool_call_id":"review-provider-call-1"}`

新增details完全未进入HTTP请求。所有当前HTTP协议适配器都用未改动的render_tool_outcome_text，故作者注入TurnProviderPort直接检查ModelTurnInput不能证明真实外部模型看到details。原TypeScript工具本来也将宿主details与模型content分开，原叶只要求真正原生工具业务字段；因此本现象记为证据边界，不要求把宿主details全部发送模型，不把它单独判作三个叶的阻断。作者“实际后续模型输入一致”应准确限定到正式内部输入、存档和模型正文；外部模型字段消费未实现。

## 已验证

- 作者6个新增用例在独立树使用指定Cargo重新执行，6 PASS；覆盖真实审批拒绝/确认范围变更、可信助手拒绝、模型未配/无覆盖/超时/非法向量、实际经验注册及写失败、旧无details历史恢复、32KiB共享投影及原raw保留。
- 32KiB正文+details成本真实计算与恢复上限有代码检查，未发现预算旁路。历史“缺新计数字段”原测试是在已超预算详情负控上再删统计字段；这不单独证明合法小details缺统计被拒绝，且代码明确兼容旧消息缺该字段。不要将该测试解读为普遍拒绝缺统计。
- 事实embedding写入API目前仅测试调用，生产检索对已有实际覆盖有效，无覆盖明确状态；不能把测试预填向量当成已完成自动事实嵌入生产流水。此处未扩展为本叶额外任务。
- 未运行全阶段门禁、真实账户供应商、跨平台或打包。

## 可复验材料

稳定证据目录 /tmp/r06-memory-tools-independent-evidence/：
- r06_review_memory_tools.rs（完整独立测试副本，含共享真实辅助和2反例），SHA256 79a5a5859271e826fc9dec7e56785b398ff5084f44e6aea03f064f61f6c6a35e。
- r06-review-memory-original.log：作者6例重跑通过。
- r06-review-memory-counterexamples-http.log：最终真HTTP证据和并发反例。
- r06-review-memory-concurrency-repeat.log：最终16成功丢15条的重复反例。
- r06-review-memory-counterexamples.log：早期真实渲染边界反例。

命令：
`LINGXI_HOME=/tmp/r06-review-memory-env/home TMPDIR=/tmp/r06-review-memory-env/tmp CARGO_TARGET_DIR=/tmp/lingxi-r06-memory-events-target /Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r06_review_memory_tools independent_concurrent_record_successes_are_all_recallable -- --exact --nocapture`

反例修复前预期exit101；运行正文HTTP边界测试仍会因宿主details不输出而exit101，它是证明证据边界的探针，不应作为产品正式新增门禁。测试失败临时home保留作诊断，不触碰真实home。
