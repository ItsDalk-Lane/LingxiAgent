# R06-AR-W06 / R06-AR-F06 执行者自检

当前状态：READY_FOR_INDEPENDENT_REVIEW（执行者两层自检完成；全新独立审查尚未运行，不自签独立PASS）。没有提交、推送、发布、真实数据、付费或外发。全部测试采用合成独立 LINGXI_HOME；外部模型仅为 loopback 确定性应答。测试守卫始终默认 8192MB/600s、cargo 1.98.1、-j8、--locked。

## 问题与真正入口

实际基线 HEAD 为 2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d，和研究 SHA 相同。初始用户及并行改动见 entry.json；用户 settings SHA256 0f72eb0a2549c95973c2aa8abe54c9aa44491fb9ca2a1c6e437404655d36b3d3 未变。

正式入口是 lingxi-service binary → 认证 execute 路由 → SessionStore → RunSupervisor → ContextCompilerService::compiled_for_run → FileContextMaterialSource::gather → agent config 解析 → ContextArtifact render → GatewayedProvider → 实际 OpenAI wire。原行扫描在 flow memory.enabled:false 中读不到字段后默认true，且先按“ #”截字符串再去引号。

正常正对照先在旧产品执行 positive_block_disabled_real_request，1/1 PASS（logs/old-positive.log）。随后旧生产 gather/run 经等价flow与引号内#两个正确契约断言，2/2行为FAIL、exit101（logs/old-red.log）；不是编译失败或0匹配。原始合成home、模型捕获次数和守卫均在日志。新实现沿同入口绿，构成等效旧版本红/新版本绿对照；未在共享工作区恢复旧产线或修改别人文件。

## 精确修复

- adapters 新 agent_config.rs 提供单一真实YAML文档解析、敏感字段类型验证、scan/flags/skill lists及语义保留更新。BOM、CRLF、flow/block、引号、转义、Unicode、alias和merge使用标准解析。重复字段/分区拒绝，未知字段保留。
- 文件缺失、空/注释及合法字段缺失按原缺省；读取失败、语法损坏、敏感键错误类型必须Err，不混同默认。
- context_compiler 的材料和花名册读取接通解析；MemoryService/PersonaService/SkillService、memory tools及存续Agent技能扫描传播错误。experience反馈原unwrap_or_default已修为明确错误，不改变有效配置默认功能。
- AgentConfigPatch 的flags/lists读写均Result；patch_agent_config在写前验证旧文档和候选文档，错误时字节不变。dump会规范格式并移除注释，和现役JS loader全量dump一致；未知字段值/嵌套值不丢，原Skill启停、override及发现规则保留。
- 原测试2文件只补正确Result处理，测试名、数量和断言不删不松。没有修改W02/W12及lib.rs/shared。

完整候选为 owned-files.json 的15文件（产品8、Cargo2、测试5），每一SHA256已冻结。owned-tracked.diff仅含tracked文件；4个新文件需要按manifest复制。family-paths.txt记录全部Rust同族消费者。

## 新依赖与锁

serde_yaml_ng 精确 =0.10.0，唯一新增直接依赖，crates.io锁校验 7b4db627b98b36d4203a7b458cf3573730f2bb591b28871d916dfa9efabfd41f；传递新增 unsafe-libyaml 0.2.11 校验 673aac59facbab8a9007c7f6108d11f63b603f7cabff99fabf650fea5c32b861。indexmap/itoa/ryu/serde复用原锁版，没有更新其它包。

官方接口：https://docs.rs/serde_yaml_ng/0.10.0/serde_yaml_ng/ 。官方重复键行为测试：https://docs.rs/crate/serde_yaml_ng/0.10.0/source/tests/test_error.rs 。真实本机测试确认Value解析重复键错误、merge、递归别名错误以及多文档拒绝；没有自造另一套YAML子语言。

## 四项逐项与对抗自检

| ID | 期待 | 实际与证据 | 范围 |
|---|---|---|---|
| C01 | block与flow false均关闭记忆 | 新测试经真实gather、生产run和正式binary捕获实际请求；合成nonce必须完全不存在，enabled默认时存在 | SELF_CHECKED：isolated-final-dedicated-four-c（10/10、输入摘要稳定） |
| C02 | 引号内#、中文、冒号、单双引号转义、行外注释、BOM/CRLF保真 | gather精确字符串、正式binary真实wire名字、观测render SHA一致 | 同一正式运行10/10、SELF_CHECKED |
| C03 | 无关深层同名键不串扰；异常/重复不默认开启 | 15种非法形态、重复敏感字段/分区、recursive aliases/multiple docs；正式binary零wire、DBfailed、flags/feedback/persona400；I/O目录错误也Err | 同一正式运行10/10、SELF_CHECKED |
| C04 | missing/empty/partial/现役样本默认兼容；读取不重写、更新不丢未知 | binary读取前后bytes相等，默认memory依旧true；现役config.example.yaml；完整Value嵌套/列表/未知字段比较；错误更新bytes不变 | 同一正式运行10/10、SELF_CHECKED |

第二层对抗：真正ToolInvocationGateway调用tenet_propose，flow关闭必须返回paused且不创建pending；quoted false和重复enabled须ToolOutcome::Failed、零pending。完整合法配置仍可用，未以默认关闭所有功能掩盖错误。显式flags/skills更新不追加重复敏感键；错误原文件和坏候选都拒写。真正binary每例新进程/隔离home，有authenticated HTTP、真实文件、SQLite runs状态和实际wire证明，未mock解析/材料/存储/服务进程。

现役终态固定分类为 failed.provider_error，具体context_compile在FailureCause::code；本轮不重写这个原合同。测试曾误期望terminal_reason含context_compile而失败，原日志保留，后来核对现役分类并保留failed/零wire/具名配置接口诊断断言。该测试错误不冒充产品缺陷。

## 原Task / A-ID / R00

original-task-clauses.json 保存原T01/T06全部10个Steps、6交付物、A01/A02/A11/A12原始given/when/then。original-step-self-check.json逐条记录期待/实际/证据/状态。本轮仅配置份额自检，未以四C替代原全任务：历史exchange/压缩/工具目录预算/梦境调度等由对应AR/EX任务继续履约。

r00-full-original-obligations.json完整保留SCOPE-01中354条T01/T06相关记录，r00-obligation-index.json保留101个R00叶索引。每条原要求仍REQUIRED；来源SCOPE-01/obligations.json是原条款和待实验事项供查，不是PASS证据。所有R00叶没有被本轮一揽子签PASS，原未验证或实验端点仍NOT_RUN并须由完整阶段覆盖。

原回归涉及ContextCompiler/Artifact/模板golden/预算、MemoryService人格隔离、SkillService whitelist/override/read-only、A12安装双层审核及R04网关越界拒绝、hooks。原77项绿窗口保留于regression.log；因为当时仅skills注释及新测试还在变，旧窗口不作为最终摘要稳定验收，固定单项副本/专属target已重新亲跑77/77，输入摘要稳定（isolated-final-regression.json）。

## 保留全部失败与中间绿

- old-positive 正常1绿；old-red实际2行为红；green-01初3绿不是完整验收。
- green-02/green-03因W02 session_tree canon编译错误，非旧代码反例。
- adapters-01：新3+原14绿。
- four-c：中间9绿但owned输入变，陈旧窗口不当最终验收。
- final-four-c：新增测试泛型.into误用编译红0；已修。
- final-four-c-retry：新增终态字段误期望的行为红；已按原合同修正，不称产品修复。
- final-four-c-complete：10绿、摘要稳定，后加I/O错误注入及两项clippy测试写法修正，仍须固定副本最新绿。
- clippy：W02 retry_turn 8参数警告红；clippy-retry：新增测试clone_on_copy/type_complexity红，已精确修写法，无allow或降格。
- regression-final：W02 resources.rs新签名编译红0。等待固定单项候选，不反复共享重跑。

各运行.json包含真实命令、起止、退出码、测试数量、候选/有效输入/lock/fixture摘要、守卫、平台和mock边界。初期直接命令的旧红/初绿在原log而未用统一runner记录，不能补造时间。

## 影响、未验证与交接

新增YAML依赖影响整个adapters/service编译，配置签名影响T01/T06生产及原测试；F14必须将新3套件（6+1+3）和旧2测试签名变更重新绑定，不可复用旧hash。F05 context缓存、F07目录/声明、EX相关人格/记忆/技能配置和全部T01/T06/R00待实验叶的候选证据应判受影响/STALE，不能继承本轮四C为全业务PASS。

没有声称全workspace、verify-stage R06/R05、TS全量、其它平台、真实供应商LIVE或正式打包通过；这些不是本单项隔离自检结果。未更改wire/data epoch/存储schema/授权/取消/模型供应商重试。YAML格式/注释规范化的影响已明确；语义未知数据按实际Value严格保留。

专属target正式四C/对抗记录：isolated-final-dedicated-four-c.json，cwd为单项副本，service executable SHA256 ac78d36d8f84034bc4b49c72ca293b3ce04cdbc901d6a5f9baa3ff83080a60f7，10 tests全部通过，owned hash前后相同。此前shared-target isolated-final-four-c在仅完成3适配测试、尚未启动binary guard时被执行者按总控新隔离要求精确SIGINT，exit-2，原记录保留，不能当最终。进程检查确认其无遗留测试/服务进程。

## 最终固定单项候选亲验结果

在 /Users/study_superior/.codex/worktrees/r06-independent-review/LingxiAgent 只读验证，baseline 2e1933 + 精确15文件，不含W02/W12产品。仅使用其专属 rust/target，所有本人Cargo命令串行，守卫不移除。绑定来源 W06-CANDIDATE-01/binding.json，逐文件匹配 owned-files.json，未修改固定副本。

| 亲跑 | 退出码 | 实测结果 |
|---|---:|---|
| isolated-final-dedicated-four-c | 0 | 10/10（6服务配置+1真实binary含10独立样本+3适配）；含真正tool网关对抗 |
| isolated-final-regression | 0 | 原77/77，原断言和名称/数量保留 |
| isolated-final-clippy | 0 | service/adapters全targets，-D warnings无降格 |
| isolated-final-contracts | 0 | contracts/generated artifacts零漂移，API矩阵626项 |
| isolated-final-boundaries | 0 | crate/layer边界检查 |
| isolated-final-fmt.log | 0 | 只对owned Rust文件rustfmt --check |

以上五次Cargo记录 owned_hashes_before==after，service binary SHA256始终 ac78d36d8f84034bc4b49c72ca293b3ce04cdbc901d6a5f9baa3ff83080a60f7。isolated-provenance.json记录实际cwd/target/HEAD/全部源状态和服务程序SHA。主仓用户settings及15文件最终再次核对无漂移。完整失败和早期绿记录均未删除。

执行者结束本Task；最终状态仅 READY_FOR_INDEPENDENT_REVIEW。独立Reviewer必须使用冻结同一15文件与独立target亲跑，不继承本聊天的口头结论。此状态不等于全Task/R00/阶段已独立验收。

obligation-share-self-check.json对完整354记录逐条列出原期待/原deadline、本F06份额状态及未完成份额归属；EX归属仅按原requirement id或feature_id精确连接，其他F归属逐条继承SCOPE related_findings，F14/F15负责逐案绑定与完整收口。101叶索引也逐条列remaining_owners，全部保持未签全叶PASS。既有原Task绿回归不偷换为整个Task关闭。
