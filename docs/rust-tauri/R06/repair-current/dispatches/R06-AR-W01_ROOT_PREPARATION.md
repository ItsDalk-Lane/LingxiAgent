# W01 总控只读准备

本记录只用于下一位全新执行者的依赖交接，未执行任何F01案例，不是修复、自检或独立验收。当前F01和关联原义务均OPEN/NOT_RUN。原8Task、37Step、16A、281唯一叶和12额外根因簇保持完整；以不可变完整规格及SCOPE-01原条款为准。

实际主仓HEAD为2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d，当前存在多个尚未独立验收的任务改动；不得把主仓所有产品改动直接当W01已验候选。用户原.vscode/settings.json持续保护。执行使用新一次性代理，由总控明确owned文件和候选依赖后开始，Git写操作仅总控。

## 只读源码观察

- service/runs.rs的drive_run仍以空Vec初始化exchange，再把exchange.clone传给模型。此前用户输入已由DbUserMessageRecorder写入树，当前输入如何排除重复必须由生产入口实测。
- kernel/model_exchange.rs的ExchangeItem只有AssistantTurn、ToolResult、CompactionSummary、CompactionInstruction，没有历史用户回合类型；迁移方案需要保留历史角色而不能把用户文字装成助手或新系统权限。
- adapters/storage/session_tree.rs的list_branch_messages读取有效分支链。fork_session复制共同消息时将run_id置NULL，ID与模型调用溯源保留，以免源会话删除被外键阻止。不得简单恢复指向源run的活外键破坏原删除契约；需要可持续且授权正确的来源设计。
- service/history_projection.rs是展示投影；fork副本的Run关联按unknown记录，不按时间猜。它不是完整模型交换档案，不能拿UI最终正文替代工具请求、结果和opaque状态。
- service/runs.rs的工具请求助手回合目前只在内存exchange中保留全content与RequestedToolCall；model_call_completed事件仅调用身份及usage。adapters/storage/run_store.rs持久model_call_usage有provider/model/protocol及emittedToolCalls等来源线索，最终消息亦持久。新执行者必须继续核对所有事件、请求实参和来源能否恢复，不能根据摘要推定齐全。
- service/compaction.rs的apply_plan替换本Run内exchange；压缩版本、摘要与后缀的跨Run持久恢复需继续追踪，不可因当前Run压缩测绿就关闭C04。
- 前台execute_for和后台execute_background_for使用相同admission授权与驱动入口，后台持有真实lease。child授权及裁剪路径须另追踪验证，不能从前台通过推定其他入口。

## 正在验收的W08依赖

W08的新模型投影归档tool_result_model_projection明确区分权威原始工具结果及模型受限投影，先持久提交再送下一模型请求。它不是全Run/token预算；每成功工具结果渲染总预算32768B，错误保持原不截断契约。

未来跨Run恢复应实际消费被模型看到的投影，不重新把raw档案全文塞回模型；旧raw-only记录需要明确有界兼容策略及诊断。投影保存失败不得继续无档案请求。原始raw与投影证据都保留。最终接入必须依据W08真正独立判决/固定candidate交接，当前W08-R01仍在运行，尚无独立PASS。

## 当前必须按完整规格执行的C-ID

- C01：独立Rust binary+认证HTTP，第一回合运行时nonce，第二input不复述，实际下一模型wire只含历史及当前输入各一次、无别会话。
- C02：A/B共同祖先、互斥分叉、重启、rewind被排除后缀，实际wire及旧工具执行次数对账。
- C03：完整调用/结果宿主及供应商身份、opaque来源，换供应商、损坏关联，明确处理而非伪装正常。
- C04：多个用户Run跨预算，摘要失败/取消/重启，原档案保留、当前输入不重复、旧工具不重放。

上述均NOT_RUN。至少正常正对照、旧版本或精确空exchange变异C01/C02真实行为红、同族全部路径、实际四C及额外反例、自有target/source/binary/输入前摘要/有效LINGXI_HOME证据。编译错误、脚本未跑、0测试不能叫旧红。新增存储结构必须接MigrationInspector和真实登记，不假记迁移版本。

## 原义务和当前边界

F01明确关联R06-T01/T02/T03/T04全部Steps/A01—A08及对应原叶；须逐项裁本Task的实际份额并交付，不只修空Vec标题。跨Task依赖不作为豁免：F02恢复和F03互斥未闭合、EX01/EX02压缩设置/恢复材料、EX11损坏树/未知旧字段仍OPEN；所需真实生产证据最终全部闭合。

当前文件恢复和资料缓存独立Review被平台实际中止并要求Daybreak，账户只读查询not_granted。不得借W01执行绕过受阻Reviewer，也不把它们的部分日志冒充最终判决；可按已授权范围继续独立安全本地产品修错。没有实测项目写NOT_RUN/BLOCKED，禁止写PASS。
