# EX06 助手自己的记忆嵌入模型

已修复已复现缺陷：助手没有memory.embedding_model时，现役生产链仍调用全局知识嵌入模型一次。修后相同负控实际HTTP 0，明确unavailable_no_model，FTS仍独立正常返回。无配置不再产生不该发生的模型调用。

## 冻结净包

- 基线：`634c6dc715b1061c77c4e3fcf7acc87b30d55f11`
- 7文件候选SHA256：`12e56e4c54fe08eb0e56d6f47a0b65e776143e31cca5e5f9dc8810744da1f825`
- 补丁SHA256：`f6a332e6d3d5d187018c71767c7d8548cd95e6a9e7a138ec469352676b219d97`
- `ex06-per-agent-embedding.patch`；逐文件base/candidate：`ex06-per-agent-embedding-manifest.json`；独立7文件重放一致：`ex06-net-replay.json`。
- 另交原W10造旧身份夹具hunk `ex06-w10-legacy-fixture-only.patch`，不混入7源包。其SHA/因果界限见 `ex06-w10-fixture-manifest.json`。

MemoryService每次读取有权助手当前YAML的provider+id，缺失明确NoModel，坏引用明确不可解析；模型必须是运行配置明确声明的同一对或现役provider-catalog/插件合并目录中声明支持embedding的模型。未知模型、缺一半引用、错误类型不会发送。已保存模型解析仍沿唯一YAML解析器，不创建第二配置读取路径。

知识默认embedder与记忆选择分开接线：记忆选择同一个显式provider+model pin用于identity和实际OperationService调用，实际发送继续沿原配额、凭证、取消、持久费用收据与台账链。返回模型空间key来自实际服务路由与参数，不拿global身份给结果贴标签。知识默认入口行为保留，不增加嵌入客户端或旧Agent loop。选择只加knowledge.rs顶部与lib一处注入hunk，memory.rs dream_store未改，保留EX05/07/08/09并行边界。

## 实际结果

- `ex06-baseline-no-model.log`：修前真实1次global，期望0失败；对应原始JSON `ex06-baseline-no-model-facts.json`。
- `ex06-memory-identity-frozen.log`：完整原memory工具10+新增2，12 PASS；原W10身份9 PASS（使用独立同步FK夹具）。原semantic父只给助手补真正选择的模型ref、使用per-agent getter，原no-coverage/实际向量命中/timeout/失败/隔离断言未放宽。
- `ex06-default-owned-regression.log`：W11真实driver→gateway→embedding单permit与费用重放1 PASS。共22个本轮实际用例PASS。
- `ex06-clippy-frozen.log`：service lib+memory/W10/W11父严格Clippy PASS。
- 新正控实际A/B各自不同loopback Provider、目录中真实embedding声明、不同模型空间key；给A塞B模型空间向量仍无覆盖。A/B各自只命中本助手事实，错Agent调用零新增HTTP。
- 真正OpenAI chat HTTP/SSE→mcp_describe_tool→mcp_call→search_memory，实际Run费用行provider=a/model=model-a、真实SID/Run/attempt/父工具与收据归属一致。没有用注入TurnProvider替代被验收模型链（该注入按现役设计会关闭真实Operations，发现后已改实际HTTP）。
- 服务实例关闭并由同磁盘重新bootstrap后命中和配置持续，启动无额外HTTP；去掉global embedding配置，合法助手ref仍实际命中。4种坏ref明确unresolvable且零新增HTTP；删除助手ref后global虽存在也明确no_model、零HTTP。
- 可执行只读收据复核 `verify-ex06-proof.py` 实际PASS，输出 `ex06-proof-verification.json`：9个实际embedding HTTP = 9条唯一费用行，无重复行；parent tool与Agent/路由逐收据核对。

最终原始收据：`/private/var/folders/5r/wq54gtms21gfjgr5v94ynfyh0000gn/T/lingxi-r06-tool-completion-ex06-agent-models-22058-1792043950386564000/ex06-per-agent-proof.json`。

## W10夹具与边界

候选最初完整W10有一次旧夹具commit FK失败（8 PASS / 1 FAIL），它更改变体主键但未同步derived_algorithms附加引用；Main EX08还新增usage同类引用。经授权只让造旧身份的同一事务同步两个附加引用，不改算法标签、时间、向量BLOB/维度/指纹、生产FK或原FTS拒绝/显式重建断言。

为防止错误归因，另用纯git archive634c原始全部Rust运行原legacy单项与完整9，实际都PASS，日志 `ex06-w10-base-failure.log`（名称沿最初预期，正文是PASS）和 `ex06-w10-base-full.log`。因此不宣称base稳定失败，也不把首次FK失败证明为EX06引入；只交透明原始结果及一致夹具。base精确SHA在 `/private/tmp/lingxi-ex06-w10-base-repro-20261015-r1/base-source-sha.json`。

本轮只修EX06 per-agent semantic链，完整原SPEC见 `EX06-original-spec.json`。record/recall原完整工具父本轮仍PASS；更广experience管理/learn_lesson的既有登记不在这里重签。所有正式断言/登记/门禁保持不变；最终READY由Root在当前Main实际绑定后决定。无Main写入、无提交推送、无新依赖或target、无真实用户资料/付费供应商调用。macOS运行，Windows/Linux及正式打包NOT_RUN；新的助手选择重启证明为同进程新ServiceState，W10另含实际CLI崩溃重启回归。
