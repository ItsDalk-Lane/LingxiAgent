# EX-01：真实手动压缩、设置消费与持久档案

基线 `6d19332793c0819f5219a4ed42fcc8f0d7f813a4`。工作树 `/tmp/lingxi-w11-codex-20261014`；仅本批新增范围，W11 补丁不在交付差异内。未提交、推送、修改登记/门禁/台账。R06 仍未验收。

## 本批结果

新增真实 `POST /sessions/{id}/compact`，既有 fresh-compact 也走同一提交链；两者只有在有效历史档案原子提交后才返回压缩成功。新增 `POST /sessions/{id}/compact/cancel` 控制手动/fresh 的真实取消域。缺模型/编译链、未知方法或坏输入、未知/越权/非 active 会话、忙、供应商失败、取消、材料/设置变化、分支变化、存储失败均明确拒绝；失败保留原有效历史。空/无有益切点返回 noop；硬截断按现役策略显式 degraded，不冒称模型摘要。

成功档案记录 sourceKind=manual_compaction、独立 sourceAction、owner/actor/agent/sourceSession/boundSession、请求方法与 customInstructions、压前 head/revision/历史 digest、replace 内容、excludedChildRuns、当前材料 render/观测及真实工具声明快照，整体 canonical digest 绑定。没有创建假 Run 或 user 输入；底层 AuxiliaryExecutor 的 RunContext 仅作辅助请求关联容器，usage.run_id/attempt 均为 None，cause_ref 为 action ID。

load / recover_abandoned / 后续 record / fork 前缀和复制都明确区分 manual 来源；既有 Run 来源继续要求真实输入锚及原 actor/Run/工具/opaque 契约。manual 读取须验证源身份、压前历史 digest、原始保留后缀完全一致、合法工具对，未知来源/伪造保留结果被拒。源仍存在时复核归属与谱系；合法 fork 完整复制原档案，源删除后仍独立可读。

存储在同一事务核对 head、revision、active/owner/agent；不对过时结果自动重新接头。取消与最终提交裁决共享原子状态：只有取消赢得裁决才回取消成功，提交裁决已赢则取消明确拒绝；取消已获准不得提交档案。

## 设置来源与语义

亲读：core/session-defaults.ts、core/session-compaction-runtime.ts、core/session-compactor.ts、现行 pi-coding-agent agent-session.js/settings-manager.js，以及 R06-EX-W01_SPEC.json。

旧 createDefaultSettings 是 SettingsManager.inMemory，默认 enabled=true/reserveTokens=16384/keepRecentTokens=20000；运行时 reserve 动态覆盖为 max(16384,ceil(window*20%))。SDK 手动 compact 不看 enabled，auto 才看 enabled。

本批沿已有助手 config.yaml 的 compaction 子项及既有 PUT config 深合并/GET 回读通道消费真实设置；不增加只能测试直写的设置文件。缺省沿旧动态 reserve；显式 reserveTokens 覆盖实际预算，keepRecentTokens 影响实际切点，enabled=false 关闭自动压缩但保留手动压缩。每个禁自动请求仍做保守预算检查，超窗不发送。

PUT 校验布尔、非负安全整数，磁盘坏配置响亮拒绝，不转成默认；未知配置由既有深合并保留。独立未绑定资料根的 CompactionService 仍用原默认，不将其合法逻辑 agent ID（例如 agent:local）当成文件目录名校验。生产组合根始终绑定真实助手资料根。

自定义关注只追加到摘要指令，保留缓存前缀形状，不改系统优先级。fresh/手动都采集真实工具目录；成功档案保存刷新工件，并在摘要后复检材料/工具/设置没有跨代变化。下个真实 Run 仍按现行机制重新采集当前材料与工具，而不是把刷新工件偷偷当成永久旧快照。

## 真实验证

仅远端供应商为 loopback；压缩、网关、driver、上下文编译、真实 HTTP/认证和 SQLite 均为生产实现。原历史由真实 execute 写入。

- 新增 r06_ex01_manual_compaction：6/6，含下一 Run、重启、删除源后的 fork、focus 线上字节、真实 PUT 设置、fresh 当前材料/工具、坏配置/坏请求/权限/未知/忙、实际供应商失败、真实取消、同 head 不同 revision CAS、摘要途中实际 PUT 材料变化、真实 SQLite trigger 写失败、未知 manual 来源和伪造保留历史拒绝、显式硬截断。
- r06_ar_f01_history：8/8。
- r06_session_history_content：12/12。
- r06_t02_closed_loop：2/2。
- r06_t02_compaction：26/26。
- adapters session_tree_r06t03：18/18。
- service r06_t03_session_tree：14/14。
- 合计 86/86；clippy(service/adapters lib, -D warnings)、fmt --all --check、diff --check 通过。

存储失败检查必须见 EX01_REAL_STORAGE_FAILURE，避免更早失败假冒达到注入点；CAS 检查由真实外部 SQLite 写者只推进 revision、保持相同 head，避免仅证明字符串头比较。

原始日志在 /tmp/lingxi-ex01-*.log，合成资料在 /tmp/lingxi-ex01-env，target 在 /tmp/lingxi-ex01-target；不提交大运行目录。紧凑校验与源码/日志摘要见 ex01-validation.json。

## 可绑定面与保留范围

本批足以覆盖 EX-01 的服务核心：真实 compact/fresh 持久提交、真实设置与 focus、错误历史保留，以及 D62A81C2AEB2 的成功/拒绝服务断言。slash 的先回复/被接管目标与 fresh 接管拒绝、WS compaction_accepted→succeeded/noop/failed 完整通道、实验开关和旧客户端接线没有在这里实现/证明，不据此将相关整叶翻 READY。仍需主控绑定、同一候选集成验证及规格要求的全新独立审查。

另观察既有 ModelHistory 不把 todo-state-v1 等未知非消息条目解释为对话（会拒绝）；本批没有扩白名单或顺手改该边界，CAS 负控没有以这种条目混入无关问题。
