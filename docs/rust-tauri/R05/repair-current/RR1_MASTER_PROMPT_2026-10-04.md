灵犀 R05：完整修复与放行验收总控提示词
版本：2026-10-04；本文件可整体交给编码智能体执行。

一、你的角色、唯一目标与结束状态

你是 ItsDalk-Lane/LingxiAgent 的 R05 修复总控。按下文组织新的执行智能体、修复智能体和独立验收智能体，完成 R05-T01—T08 的所有必需实现、反例修复、同类路径检查和阶段复验。目标是把 R05 修到可以正式交接 R06，而不是只修第一条报错、只把新增测试变绿，或把未完成实现交给 R06。

当前审查结论为 NOT_ACCEPTED。被审分支 codex/rust-tauri-migration；被审 HEAD d80737b6cb9186c8a18c0f35923aac00249d45c3；R05 实施前比较基线 c549ff654508ab951e2cf39cf9d309fc9c6b8656。不能从 main 的旧基线重做，也不能强制回退已经前进的分支。

最终只能在满足本文全部放行条件后宣布：R05 已通过规定范围的独立验收，R06_READY=true。随后停止在 R05，交付 R06 所需输入；本提示词不自动实施 R06。若必需实现、有效环境的必需门禁或独立审查仍未通过，必须明确 R06_READY=false，不能用“基本完成”“剩少量加固”代替。

修复的是全部已确认根因及其受影响路径。有限审查不能证明所有未知输入永远没有缺陷；这里的“无遗留”指 F01—F28、本阶段原始必需义务、修复中新增发现和必要回归没有未关闭项，而非承诺不可能验证的绝对无错误。

二、需求权威、现有审查证据与范围

2.1 需求顺序

以本次用户明确指令、本修复提示词、2026-10-02 的 Lingxi_R05_完整执行与验收提示词_2026-10-02.md、2026-09-23 的原始 R00—R11 任务书及其共同约束为依据。后续专项细化不能削减原始功能。仓库自写的报告、scope、interface evolution、PASS 台账不具有修改用户需求的权力。

原始文件按实际名称查找，不虚构它们存在于某个仓库目录：
- 01_通用执行约束.md
- 02_目标架构与强制契约.md
- 03_功能与所有权矩阵.md
- 04_阶段依赖与接口交接.md
- 05_验收与性能协议.md
- 06_风险、技术决策与退出条件.md
- R05_模型协议、凭证、流式处理与最小完整闭环.md
- R06_上下文、会话语义、记忆与知识库.md：只读前置及接口消费要求，不执行其任务。

本文已包含完整修复范围、原 A-ID 和 C-ID 索引、放行与负测要求。原材料暂未挂载时，先据本文及仓库真实原始规格继续所有可确定工作，登记缺失，不声称读过不存在的文件。无法从已给材料确定的关键要求才列明具体缺口；不得为了等用户回答普通实现细节而停止。

2.2 此次审查的事实边界

以下是对冻结 HEAD 的审查事实，不是修复后成绩：
- 精确 Rust 1.98.1、锁定依赖、Linux x86_64。原 R05 登记的 82 次运行涉及 254 个测试，253 个获得 PASS 证据；唯一剩余为 T08-C12 的资源采样，在该容器 PID namespace 与 /proc 视图不一致的环境中无法完成。原 stage runner 的 exit 1 原样保留，未冒称全门禁通过。
- 协议反例：13 个测试中 1 个正常对照通过，12 个正确契约断言失败；凭证/网络反例 10 个正确断言失败；worker/operation/usage 反例 8 个正确断言失败。它们直接调用未修改的生产 crate，失败代表实际缺陷，不是编译失败。这些数量有跨域重复，不能相加冒充独立根因数量。
- 正式 Rust 二进制、真实认证 HTTP、真实文件工具、SQLite 和重启，另复现了整批工具先执行再发现 schema 错误、同 ID 冲突工具均执行、实时与历史规范化不一致、仅 reasoning 产生 final 等。
- 3 份历史候选清单中的 rust/scripts/contracts 共 483 个文件逐项与被审源码相同。历史 PASS 不是凭空伪造；问题在于它们没有证明完整要求。不能仅因历史 testedSha 仍写旧基线，就否认已核对一致的源码证据。
- C-ID 换名实验实际运行了 7 个 xtask 镜像测试，原对照及变异均通过；原 Python assembler 也在复制的历史日志 fixture 上错误通过。没有声称本轮完整 verify-stage R05 在变异后实跑通过。
- macOS system speech 的取消/期限问题是源码确认，本轮未在 Linux 冒称测过 /usr/bin/say。真实付费供应商、真实 OAuth 账户、Windows 未运行。

随附证据包如可用，先阅读 README 和各报告；本机临时绝对路径不是你的环境前提。迁移反例到仓库时可调整调用签名、目录和夹具，不可削弱原行为断言。没有证据包也必须按下文具体输入重建反例。

2.3 允许修改的范围与必须保留的边界

允许为 R05 修改 rust/ 中相关生产实现、契约与生成类型、永久测试、scripts/rust-tauri/、docs/rust-tauri/R05/ 及必要的受影响消费者；按实际依赖更新 lock、schema/数据迁移与版本。所有超出通常目录的必要改动先记录它与 R05 的具体关系及回归范围，不擅自扩大产品。

保留 R00—R04 的身份、权限、批准、Resource/Process、InvocationJournal、唯一终态、取消/恢复、未知外部结果和回滚语义。复用单一 RunSupervisor、ToolGateway、CredentialService 和 ModelGateway，不建立第二个 Agent loop、审批权威、工具执行通道或凭证权威。

R05 必须完成模型操作层、规范化交换、真实工具/worker/媒体最小链及 trace/usage。完整 ContextCompiler、会话压缩、人格记忆、知识库、fork/rewind、旧历史全量导入与分页/导出产品、R07 全业务 worker 管理/后台调度、React/Tauri 切换和真实数据迁移仍按后续阶段归属；不得把这些全盘提前设为 R05 门槛。

当前已有/明确承诺的实时事件、最终消息、快照重取和持久化读取必须修好，不能把历史语义错误推给 R06；usage 查询/导出所需字段、正确性和脱敏也必须现在补齐。保留旧 Electron/Pi 生产链默认行为；Rust 新闭环自身不得依赖 Pi。

使用隔离数据目录、受控 HTTP/OAuth 服务、合成凭证和临时工作区，不读取真实用户 home/密钥，不自动捡环境中的真实 key。默认禁用收费模型与真实供应商请求；只有第5.1节独立LIVE层已获明确有效授权、指定账号/端点/预算时，才按该范围执行。不得向联系人外发、不迁移真实数据、不发布。

Git commit/push 沿用本会话已明确的有效授权：若原总控已获逐 Task 提交/推送授权，按下文验收后执行，不重复请求；没有该授权时仍完成源码、测试和可审查候选，把提交/推送状态单列，不能推导授权。不得 force push、覆盖用户改动、直接改 main、擅自创建 PR/tag/release。

三、总控执行纪律：一直闭环到阶段结论

3.1 启动与基线

先核对分支、HEAD、远端、工作树及已有用户修改。如果 HEAD 已前进，逐项检查 d80737b6… 之后的内容差异；已被后续提交真正修好的问题可凭生产入口和回归证据关闭，不能机械重复修改，也不能仅凭提交标题关闭。禁止 reset/clean 覆盖用户工作。

读取实际 AGENTS.md/局部规则、CONTRIBUTING.md、tests/README.md，以及：
- docs/rust-tauri/ORCHESTRATOR_PROGRESS.json、PERFORMANCE_THRESHOLDS.json；
- R00/FEATURE_INVENTORY.json、FEATURE_STAGE_ACCEPTANCE.json、ACCEPTANCE_MAP.json、ENTRYPOINTS.json、PI_REPLACEMENT_MATRIX.json、EXCLUSIONS.md；
- R01 已冻结协议、依赖方向、所有权与数据 ADR；R02/R03 当前交接和未解除项；
- R04/R04_HANDOFF.json、R04_PLATFORM_CAPABILITIES.json、R04_SCOPE_MATRIX.json、R04_TEST_MAP.json、repair-current/R04_RR1_FIX_ISSUES.json、R04_RR1_STAGE_REVIEW.md；
- rust/crates/xtask/src/stage_maps/R04.json、R05.json 及真实验证器；
- docs/rust-tauri/R05/ 全部当前 report/handoff/scope/callsite/provider/protocol/credential/usage/worker/test-map/ledger/blocker/live/performance 文档，以及它们引用的真实测试和证据。

大 JSON 先索引再读取相关完整记录与引用；记录读取范围，不以片段搜索结果代替完整叶子义务。

在 docs/rust-tauri/R05/repair-current/ 建立可断点续跑的 RR1 计划、问题矩阵和进度。每项至少有：F-ID、原 Task/A/C/叶子、复现、根因、同类路径、负责人、执行轮次、候选摘要、测试/证据、独立审查、状态、剩余项。可合并同义报告，避免空壳文件。

保留所有本次修复前的失败记录及原报告，不覆写成“从未失败”。问题状态使用 OPEN→IMPLEMENTED→SELF_CHECKED→INDEPENDENT_PASS→CLOSED；只改源码不等于 CLOSED。

3.2 多智能体与文件所有权

总控只组织任务、处理依赖、维护协调台账、集成已验成果与安排提交，不亲自代替执行者实现，也不自签独立验收。

按原 R05-T01—T08 建立 8 个修复工作包。每个工作包第一次实施使用新的执行智能体；每一次修复轮次使用新的修复智能体；每一次任务验收及最终阶段验收使用新的独立智能体。不得把刚才的实现者改名为审查者，不得用同一个审查者连续重审自己的上一轮判断。若工具支持空历史，给新智能体空历史和完整的文件化任务 brief。

新智能体必须读源码、原规格、问题矩阵、真实日志和候选清单；旧对话中的“已完成”不是证据。委派中给清楚的写入白名单、禁止项、依赖输入、自检命令和提交物。

可并行独立只读核验及互不冲突的实现。models/provider、credentials、operations、kernel exchange/usage、runs 和 stage_map 等共享文件必须明确单一所有者；不同智能体不能同时编辑同一文件。必要时用隔离 worktree，在集成候选上重验；不能因各自分支通过就宣布集成通过。

建议依赖次序：先确定 T01/T02 的一致身份、配置/凭证生命周期及 T07 所需上下文契约；T03 协议与 T05 网络可在明确文件所有权后并行；T04 使用完整协议结果做批次和规范化；T06 使用修好的身份/预算/网络；T07 补齐所有调用的结算和查询；T08 完成正式接线、可信门禁和资源整体验收。门禁缺口应尽早暴露，最终统一复核，不必等所有代码改完才看验收器。

3.3 修复与验收循环

执行者先将对应旧行为反例固化，证明正确断言在被审候选上确实失败、正常对照通过；再修根因、扫全同类路径、自检，交给新独立审查者。不能只添加断言恰好复述实现的测试。

每条回归除刻意破坏的目标条件外，其余配置、能力、身份、凭证、权限、工具目录和输入前置均须合法。负例及正常对照须通过真实请求、事件、屏障或副作用计数证明已到达目标边界，并断言目标错误类别及原因；更早的能力、配置、认证、授权、解析失败或根本未发起调用，不能充当下游修复证据。未抵达目标边界判测试无效，修正夹具后重跑，不得记PASS。修好F01后，应给批次/协议/历史等夹具补充测试所需合法能力，保留被测故障；不能因提前拒绝使HTTP和副作用变0，就宣称F11已修。接口演进只改调用方式和合法前置，不改正确性要求，记录所有夹具调整。

独立审查者必须实际运行指定测试，核对正式入口、真实 HTTP/文件/数据库/进程副作用及范围，逐项给 PASS/FAIL/BLOCKED。发现任何 R05 必需缺口就登记并交新修复智能体；修好后再换新审查者。连续两轮失败，另派新的根因复盘智能体审查整个问题面和相邻边界，再制定成组修复，不能继续逐行打补丁。

单 Task 通过独立验收后，按有效授权精确暂存本 Task 文件、提交、推送并登记回执；不要 git add -A 混入用户改动、秘密、target 或故障注入副本。跨 Task 集成改动必须重新验证受影响 Task。

全部实现修复和任务验收通过、T08候选材料齐备后冻结候选，派从未参与此次实现/修复的全新阶段审查者，实际重跑整个 R05 及前序依赖闭包。T08此时可以按READY_FOR_INDEPENDENT_REVIEW交出材料，F28中的最终阶段结论和最后归档回执待阶段PASS后收口；不得要求先生成最终PASS才能启动独立审查。阶段 FAIL 就返回新修复→新任务验收→新阶段验收，直到达到放行条件，或确实被必需环境/授权阻塞。不能因为上下文用尽就换到 R06；先保存精确 HANDOFF，从当前步骤续跑。

四、必须关闭的修复清单（F01—F28）

下列源码位置均以冻结 HEAD 为准；改动后按函数重新定位。它们是问题入口，不是只许改这几行的补丁清单。

【R05-T01：统一模型能力、身份与配置快照】

F01｜逐模型能力声明与发送前检查缺失
对应：A01/A02，T01-C04/C05/C06/C07/C10/C11。
入口：rust/crates/lingxi-adapters/src/models/config.rs:32–43,180–187；gateway.rs:225–237；r05_t01_model_plane.rs:806–840。
事实：当前只按协议族和操作匹配，没有逐模型 tools/images 能力载体。未声明工具或图像能力的配置，真实网关仍分别外发 1 个请求；正式二进制还发送 edit/read/write 工具。合成模型名字本身不证明真实能力，不要以名字作能力数据库。
修复：从现役有效配置和模型元数据建立可验证能力声明，覆盖 tools、image input 及原矩阵要求的 reasoning/operation/限制；调用前用可信请求需求对照同代次能力。未声明或明确不支持所需能力按原契约本地拒绝，不能静默删工具/图片、换 provider、换模型或回落 Pi。合法本地无密钥模型仍可按明确配置工作。
自检：未声明、明确不支持、明确支持三组；工具和图片分别验证，拒绝组物理 HTTP=0，支持组实际发出正确 payload。Chat/Utility/worker/相关辅助槽位、同名模型跨 provider、热更新、模型移除、协议合法但能力不符、工具目录变更均覆盖。不能以“Chat 绑定 ASR 协议会报错”替代模型能力测试。

F02｜配置更新和 401 重试混用新凭证与旧端点
对应：T01-C04/C05，T02-C10/C11；原统一解析契约。
入口：service/management.rs:649–656；service/credentials/mod.rs:396–426,500–509；adapters/models/provider.rs:195–257；gateway.rs:134–159。
事实：A 端点收到旧 key 后暂停；把 provider 改为 B+新 key；A 返回 401，生产重试仍向 A 发出了新 key。resolve 只按 provider 名取当前材料，忽略 route.config_generation/端点；compat 又另读当前配置。
修复：冻结一次调用所需 provider/model、endpoint、protocol、auth 引用、能力、compat、网络策略和配置代次，建立一致解析/发布边界。旧调用只能依明确可撤销规则使用仍授权的旧快照，或安全失败；不得把新材料填入旧 route。重新路由需要显式新调用身份和一致状态，不能悄悄改写在途调用。调换两次 reload 的先后顺序不是完整修复。
自检：使用屏障控制真实管理入口 reload 与在途 401；A→B+key、只换 key、只换 endpoint、auth kind/协议/compat 改变、删除重建、多个 provider 并发、刷新与撤销交错。记录每条真实请求的目的地、材料标识和配置代次，证明未授权端点永远收不到新材料。

【R05-T02：凭证生命周期、OAuth 后端闭环与秘密边界】

F03｜旧凭证句柄在轮换、撤销、删除重建后继续有效
对应：A03/A04，T02-C05/C06/C08/C11/C12。
入口：service/credentials/mod.rs:611–616,685–705,750–771,828–831。
事实：每次 seed_cell 将 generation 重置为 1；旧 handle 可在换 key 后拿到新 key，也可在 revoke→删除→重建同名 provider 后复活。
修复：建立跨 provider/cell 生命周期不复用的 epoch/实例身份和原子失效规则，handle 绑定可信主体、provider、用途/权限及生命周期，保持撤销 fence 和过期校验。刷新/登录迟到结果不得安装进已撤销或被替换的实例。检查初始无 OAuth 后热新增 OAuth 时的 store 初始化和持久化。现有 handle API 缺 principal 是契约/证据缺口，不宣称已实测了外部跨用户盗取。
自检：旧 handle 轮换/撤销/删重建全部拒绝，新 handle 正常；API key↔OAuth↔authHeader↔none，过期/篡改、跨 provider/用户/Agent，刷新返回与 reload/revoke 的确定性竞态，持久化失败及重启恢复。既要防复活，也要保持不同 provider 刷新不互阻、共享刷新单个等待者取消不误杀其他等待者。

F04｜OAuth 初次登录和模型管理的六个独占叶未完成
对应：T02 步骤1、T02-C08，以下 execution_stage_ids=[R05] 的原 R00 叶：
- R00-T02-LA-16CEB6D12A6A：添加 OAuth 自定义 modelId，刷新并返回新清单；
- R00-T02-LA-8060BE8AA02C：删除自定义 modelId，刷新并返回新清单；
- R00-T02-LA-CFEC64F68DDE：列出指定 OAuth provider 模型，非 OAuth 明确拒绝；
- R00-T02-LA-99D6C304D697：start/callback/device poll 到凭证安装、状态及模型刷新；
- R00-T02-LA-CA0BF9A7AEA9：各 provider loggedIn 与可用模型数量；
- R00-T02-LA-FC80B6C4FBE4：logout 删除凭证、清认证缓存、刷新模型列表，结果诚实。
入口：service/management.rs:586–590；credentials/mod.rs:104–105,220–233,305–324,885,908；adapters/models/oauth.rs 的 device-code/PKCE helper；CREDENTIAL_FLOW_MATRIX.json。
事实：生产管理面只有 reload/status/revoke；登录 helper 的直接单测没有证明正式入口把登录结果装入同一 CredentialService。六叶却被整套凭证测试的成功批量标 PASS。
修复：接通现役登录形态的受认证服务 API、一次性登录事务、安装/持久化/刷新及自定义模型管理，使用统一凭证权威和取消/撤销 fence。补齐上述六叶的真实后端行为，UI 可按后续阶段实现，后端不能延期。
自检：受控 OAuth 服务器下，从正式服务 start→callback/poll→credential ready→真实模型请求→refresh→logout→重启；错误/过期/重复 state、PKCE、不合法回调、pending/done/error、取消、登录中撤销/reload、安装或磁盘失败不假登录；添加/删除/列表/数量、重复和空 ID、非 OAuth、跨主体拒绝、无效输入不改原清单、重启保留未知合法字段。六叶各有具体断言及请求/存储/状态对账，不能只引用同一套 suite 全绿。

F05｜协议错误、错误截断及诊断可泄露凭证
对应：T02-C09/C10、T07-C09；与 F22 共同关闭持久化诊断路径。
入口：adapters/models/dispatch.rs:355–359；models/credentials.rs:190–198；openai_completions.rs:303–306,345–350,626–635。
事实：先截断为 512 字符再 exact scrub，会留下跨边界的 key 前缀；真实模型响应把合成 key 放到未知 tool name，生产 ProtocolError 把完整 key 带进 kernel，现有 service redactor 仍未去除。
修复：在明确宿主边界处理全部错误/诊断材料，先基于当前及刷新涉及的凭证材料脱敏，再有界截断；协议错误、SSE error、OAuth、HTTP、usage invalid detail、URL/头诊断都走同一规则。合理保留字段路径、错误类别和可诊断信息，不复制任意上游对象或全删错误内容。不要用篡改正常用户正文来掩盖错误路径泄漏。
自检：key 位于每个截断边界附近、旧/新 key、JSON 转义和 URL 编码回显；未知工具/停止原因/schema/type/流内错误/OAuth 错误。扫描修复后由宿主产生的 kernel 返回、持久化表、事件、诊断、已有导出、worker env/参数和交付产物，除受控凭证库外无合成秘密。明确区分输入夹具、授权供应商替身捕获与隔离保存的旧失败证据；这些只允许保留标记清楚的合成材料，不能混入宿主可见输出，也不能用整个测试目录豁免替代逐项扫描。禁止拿真实 key 做泄漏样本。

【R05-T03：现役协议、真实状态重放与顺序】

F06｜Anthropic thinking 签名的合法流与空文本状态损坏
对应：A05/A06/A07，T03-C10，T04-C01/C14。
入口：anthropic_messages.rs:827–841,914–932,499–521,244–277。
事实：官方形状的 content_block_start 含 thinking=""、signature=""，后续一个非空 signature_delta 被误判为两个不同签名；非流式空 thinking 加有效 signature 和 tool_use，下一请求又丢签名。
修复：区分起始占位、未完成与最终签名，保持原块身份及空可见文本的有效 opaque；按照实际协议规则处理真正冲突。不是把任意多条 signature_delta 简单拼接。
自检：初始空/缺失签名、非空最终签名、空 thinking、redacted thinking、工具后继续请求、字节拆片、真正冲突拒绝，普通无 thinking 正常。流与非流必须走完整解析→交换→工具结果→下一请求。

F07｜Google functionCall Part 的签名及并行结果分组丢失
对应：A05/A06，T03-C03/C06/C10，T04-C08/C14。
入口：google_generative_ai.rs:488–523,548–556,323–334,341–369。
事实：functionCall 解析提前 continue，流/非流都丢掉同一 Part 上的 thoughtSignature；同回合两工具结果被渲染为两条 user Content，而非同一 user Content 的两个 response parts。已验证真实编码结构，未运行收费 Google 请求。
修复：保留工具与原 Part 的有序关联和必要签名，将签名按原位置回传，结果按同一个工具回合分组；不要靠伪造签名或任意挪动签名让请求过关。
自检：单工具、同名/异名双工具、逆序完成、部分失败/取消、连续工具步骤、第一 functionCall 带签名、空文本签名。流/非流均作 round-trip 结构比较，签名字节和原位置不变，工具结果一一对应。

F08｜OpenAI-compatible reasoning 在 renderer→compat 接口被删
对应：A05/A06，T03-C06/C10/C12。
入口：openai_completions.rs:141–164,203–211,395–406,866–871；compat.rs:35–42,1090–1103；旧 core/provider-compat/deepseek.ts、reasoning-content-replay.ts。
事实：DeepSeek reasoning_content 已进入 canonical，但真实 renderer + compat 生成 thinking=enabled 的下一工具回合请求时，assistant reasoning_content 缺失，仍能外发；非流式 carrier 也需补齐。
修复：保持现役兼容供应商要求的真实 reasoning carrier、身份和位置；按 provider/model/purpose 的已声明策略重放。必需状态缺失时本地明确失败，不造空 reasoning，不对所有 OpenAI provider 无差别注入兼容字段。扫描现役 DeepSeek、Kimi、MiMo、Zhipu 等实际 replay 策略，适用性逐项说明。
自检：真实 parser/accumulator→canonical→renderer→compat→捕获 HTTP，不能直接喂 compat 一份已经加工好的 payload。覆盖流/非流、工具/文本、thinking 开关、Chat/Utility、跨 provider 切换、缺状态拒绝和合法状态成功。

F09｜opaque 只绑定协议族，可能被送给同协议族的另一供应商
对应：A06，T03-C10/C11；与 F02 联合验收。
入口：kernel/model_exchange.rs:253–261；Anthropic/Google/Responses parser 与 renderer 对 Opaque.provider/FAMILY 的处理；models/provider.rs 路由解析。
事实：A provider 产生的 Anthropic 签名，在路由改成同协议族 B/other-model 后仍进入 B 请求。只测跨 FAMILY 拒绝不够。
修复：交换与 opaque 保存实际产生它的 provider/model、必要会话引用、配置/轮次来源，发送前按明确兼容规则验证。不能把“协议同族”当“秘密状态可跨来源转发”；无法安全继续就诚实中断或使用已定义且可验的转换。
自检：同族异 provider、同 provider 异 model、同名 modelId、跨族、热更新、重试、历史恢复；同源允许重放阳性对照。扫描五个协议入口，不仅修 Anthropic。

F10｜Responses/Codex 重放改变 text/reasoning/tool 的相对顺序
对应：A06，T03-C06/C10。
入口：openai_responses.rs:225–270，共享 render_input_items；R05_INTERFACE_EVOLUTION.md:354–357 的 N-01。
事实：原 text→reasoning→tool 被重排为 reasoning→text→tool。仓库曾记给 T05，但固定候选未修。
修复：使用能保留完整有序交换的表示与 renderer，保留原关联、相对顺序和 opaque 内容；同步消费者/生成契约。不能把所有历史重排成一个固定“标准顺序”。
自检：Responses 和 Codex 分别覆盖 text→reasoning→tool、reasoning→text→tool、多段交错、多个工具穿插、纯文本/标准顺序对照，做下一次真实 wire 的结构比较。

【R05-T04：完整工具批次、可信终结与同源消息】

F11｜整批工具未完成准入就开始副作用；同一模型调用重复外部 ID 有歧义
对应：A05/A07/A08，T04-C05/C06/C07/C09/C10；专项 T04 整批要求。
入口：四族 tool-call parser（Codex 共享 Responses）；service/runs.rs:1498–1519,1552 起的逐工具执行。
事实：同轮 write(output.txt,正常内容)＋read(path=123)，第一个文件先被写，第二个才报 schema 错误；两个外部 id=dup、不同路径的 write 实际都执行，下一请求含两个同 tool_call_id 的结果。
修复：任何副作用之前，整批确认协议完整、工具目录快照与映射有效、参数完整/schema 合法、同一 modelCall 内外部 ID 无冲突、静态准入一致；失败批次零工具副作用。仍保留每次真正执行前的权限/批准、取消、配置/目录代次复核，不能以预验证取代即时授权。一个合法工具执行后另一个遇到运行时失败，不要求不可实现的跨外部系统事务回滚，但必须保真记录已发生事实。
自检：先保证能力、配置、认证和权限合法，模型响应实际完成且整批预检被触发；失败原因必须是指定schema错误或ID冲突，不能由更早拒绝冒充。合法第1项+非法第2项时真实文件/进程执行计数=0；同 ID 同参完成重发不得重复执行，同 ID 异参/不同 index 冲突整批拒绝；合法不同 ID 同参数仍分别执行；不同 modelCall 合法复用外部 ID 不得全局误去重。重复 SSE、截断后重试、工具完成后模型重试、目录改变/撤销竞态、并发会话均核对真实副作用、Host ID、外部 ID、日志与下一轮结果。

F12｜传输结束被误当协议完成，只有过程内容也产生 final
对应：A07/A08/A15，T04-C05/C06/C09/C11/C14/C15/C16。
入口：anthropic_messages.rs:1073–1120；openai_completions.rs:678–730,967–973；其他族 parse 的 Final 判定；runs.rs:1431–1462；streaming_norm.rs:888–909。
事实：Anthropic 工具 block 没关闭，只有 message_stop 仍生成 ToolRequests；OpenAI 仅文本+[DONE]、没有正常 finish_reason，仍为 Final；五族仅 Reasoning/Opaque 也判 Final，正式二进制把仅 reasoning 的运行持久化为 completed.with_final；普通首片文本在语义未确定时立即标 FinalAnswer。
修复：为每族明确传输、帧/块结束、工具完成、正常停止、长度截断、拒绝、错误、Continue、仅过程和真正答案之间的条件。未知阶段保持 Unresolved/契约明确的过程语义，可信终结且具备可用最终内容后才提交 final。只能有一个真实终态，不能把过程误装成最终答案。
自检：缺 finish reason、未知/矛盾 stop、未闭块、可解析但未完成 JSON、流内 error、length、EOF/取消竞态、reasoning-only、opaque-only、mood-only、过程→工具→答案、正常纯文本 final。所有适用族覆盖；无真 final 时不得产生 final_message_committed，重启后状态一致。

F13｜实时规范化正确，最终持久化和历史又返回原始 think/MOOD 文本
对应：A08/A15/A16，T04-C13，T08-C05/C07/C09。
入口：streaming_norm.rs:853–860；split_reserved_tag_segments 的生产调用缺失；adapters/storage/run_store.rs:1545–1568；service/lib.rs:2663–2668；r05_t04_streaming.rs:1427–1461。
事实：包含代码块中的字面 <think>、代码块外 <think>PRIVATE_THINK</think><mood>PRIVATE_MOOD</mood>VISIBLE 的响应，实时分开推理/正文并剥 mood，final_message_committed 却保存一个 raw Text，重启历史仍带原始标签。原测试读取 raw 后自己调用 helper 清洗，绕开正式历史入口。
修复：规范化结果成为 R05 实时、最终提交、快照/重连和持久化读取的共同来源。原始协议重放所需状态可受控独立保留，不能混回可展示正文；规范化必须保留合法代码/引用标签及语义阶段。不是让 R06/UI 加正则补丁，也不要求现在实现 R06 全量历史导入产品。
自检：正式二进制认证 HTTP 订阅→断连→快照重取→完成→SIGTERM→重启读取；比较规范化结构、阶段、文本和事件身份，不能在测试端再清洗。跨片标签、大小写/不完整标签按定义、代码围栏与引用、仅 mood、仅 thinking、final 中断、取消、重复重放不拼接。现有正确流式首片必须仍在响应完成前可见。

【R05-T05：统一网络策略、解析后边界与全过程预算】

F14｜系统/手动代理、NO_PROXY 与显式私有 CA 尚未实现
对应：原 T05 步骤1及交付，T05-C12；与 T02/T06 联合。
入口：models/dispatch.rs:98–105；oauth.rs:161–165；ProviderConfig；R05_BLOCKERS.md:18–23。
事实：客户端固定 no_proxy()，缺少冻结网络策略与附加 CA 配置；台账 C12 为 OFFLINE_LOGIC/NOT_RUN，审查自行延期“后续网络加固”。默认 TLS 校验仍开启，不能误报为已经关闭 TLS，但这不覆盖完整 C12。
修复：建立统一受控 HTTP client/网络配置策略，支持原来承诺的系统、手动、直连、NO_PROXY/localhost bypass 与显式可信 CA 来源；chat、OAuth、auxiliary、worker callback、operations、资源下载均按适用范围消费一致策略。配置变更与 F02 的快照一致。默认验证链、主机名、有效期，不使用 accept invalid cert/关闭 TLS 绕过失败。
自检：受控计数代理＋源服务＋临时 CA/HTTPS，逐模式验证实际路径；私有 CA 未授权拒绝/显式授权成功、错误主机名/过期/错误链拒绝；OAuth refresh、Chat、operation、download 都覆盖。测试证明直连/代理/NO_PROXY 真正改变连接目标，且取消、期限、凭证作用域不退化。该离线实现与测试不得按 LIVE 延期。

F15｜egress 域名解析可绕过未授权内网限制
对应：T05-C11，T02-C10。
入口：models/egress.rs:322–379。
事实：只授权 authorized.example.invalid 的条件下，下载 https://localhost:测试端口/...，未授权 127.0.0.1 哨兵仍收到 TCP/TLS 握手。此次证明未授权连接，不是已读取私有 HTTPS 内容或关闭 TLS。
修复：验证真实 DNS 解析候选、绑定经过校验的连接地址，避免检查一次又由客户端重新解析；处理 IPv4/IPv6、mapped address、多地址、私网/link-local/loopback、DNS 变化、代理解析和每一跳 redirect。显式合法本地模型 origin 例外保持最小范围，不能推广成任意内网下载。SNI/Host/证书校验仍按域名，下载不携带 provider 认证头。
自检：受控解析器/目的地哨兵验证 localhost、普通域名→受限地址、混合地址、重新解析、跨 origin 跳转；未授权目标连接数=0。显式授权本地源正向成功、公网合法下载成功；代理与重定向的边界和认证头同步核验。

F16｜错误 body、配额排队等等待遗漏总期限和大小约束
对应：T05-C01/C02/C03/C08，T06-C12；与 F20/F21 联合。
入口：models/dispatch.rs:138–148；models/operations/mod.rs:326–338；service/operations.rs:427–430,546–547 等 admit 前后。
事实：401 头和一字节 body 返回后停住，总预算50ms，250ms仍未返回；operation 总预算25ms，却等待被占用180ms的 permit，184ms才报过期，HTTP=0。
修复：绝对 deadline 从调用开始覆盖解析身份、刷新/登录等待、quota queue、连接、TLS、首片、idle、成功/错误 body、backoff、解析、资源下载、子进程等待及交付。每步用剩余预算，不能各段重置；有界读取错误体，保留超时/中断/截断事实，不吞错造完成。取消和 future drop 有可验资源回收；共享刷新按等待者隔离取消。禁止盲重试已可能接受的外部请求。
自检：401/403/429/5xx/3xx 的停顿、滴流、超大/截断/合法小 body；queue 已过期/排队中、refresh、backoff、TLS、下载、cancel/EOF。Chat/aux/worker/operation/OAuth/资源各入口覆盖适用阶段，真实时长在事前预算及清理容差内；socket/permit/task 清理和物理尝试计数符合事实。

【R05-T06：资源授权、媒体任务与受监督操作】

F17｜附件授权 canonical path 后，却读取原相对路径的另一个文件
对应：T06-C03，原 A11 与资源交付约束。
入口：service/operations.rs:1239–1264,1309–1334；resourceaccess.rs 的 ResourceScope。
事实：图像和音频均取得 _scope 后 fs::read(original path)，按进程 cwd 读到了授权 workspace 外同名合成秘密；20/25MiB 限制在全量读入之后才检查。
修复：读取真正授权的 canonical scope/安全句柄，复用已有 no-follow/目录句柄等授权后防替换纪律；在分配和读取中落实有界上限，而非读完再拒。扫描图像、音频及同类附件入口，不能只把测试改为绝对路径。
自检：授权 cwd 与进程 cwd 下同名不同内容文件，PNG/WAV 都只读授权内容；相对/绝对、父/末端链接替换、越权/撤销、超限及增长文件/特殊文件、读取取消；外发 payload 和产物中没有 workspace 外标记，正常合法附件仍通过。

F18｜异步媒体任务丢失提交时路由身份和 provider job ID
对应：T06-C01/C02/C12，A11/A12。
入口：service/operations.rs:173–177,284–288,761–767,868–875,887–917；models/operations/video.rs 的 Agnes 主/legacy 查询。
事实：提交返回 task_id=local-track、provider_task_id=provider-vid，服务查询实际发送 video_id=local-track；只保存裸 ID→Tracking，查询重新解析当前绑定，image/video 还共用 map。
修复：保存受宿主控制、有界且不可混淆的操作记录：主体、kind、host tracker、provider/legacy job ID、提交时 provider/model/protocol/endpoint/config generation、合法凭证引用、状态/取消与最终回执。不得复制明文凭证；旧任务不能因热更新发到新 provider，不同 kind/来源裸 ID 不得碰撞。保留现役合法 legacy fallback。按原阶段约束处理恢复/失效，不强迫提前实现 R07 全媒体产品账本。
自检：两个 ID 不同、provider 主 query 失败后正确 legacy fallback、同裸 ID 跨 provider/种类、错误 kind、提交后换模型/端点/凭证、并发 poll、任务过期/容量、重复完成回执；请求目标/ID、状态及产物一一对应。

F19｜媒体本地取消后，迟到 poll 仍下载并发布 Completed
对应：T06-C12、A12，T05-C08/C09。
入口：service/operations.rs:821–830,892–910,935–948。
事实：query 请求在途时 cancel 返回 CancelledLocally；随后响应仍触发媒体下载，登记1个文件并返回 Completed。
修复：任务级取消信号、版本/状态 fence 和完成提交边界贯穿每个 await、下载、临时写入、登记与发布；并发 poll 一次交付，取消与完成按单一确定顺序结算，不能在取消后复活正文/产物。取消与完成已线性化后各按真实结果返回；不支持远端取消就诚实 local-only/remote unknown，不能假称远端已停。
自检：屏障分别放在 poll 响应、下载、写文件、登记、完成提交前后；取消胜出后无新下载、无完成登记/事件、部分产物清理；完成先胜出不误撤销已交付事实；并发双 poll 不重复产物；网络、DB/文件失败及重试均覆盖。将真实外部请求数、文件数、状态、回执四方对账。

F20｜macOS system speech 绕过统一 permit、deadline 与取消监督
对应：T05-C01/C02，T06-C12、A12。
入口：service/operations.rs:965–969,1060–1064,1102–1147。
事实：该分支在统一 permit 前执行，忽略传入 deadline，固定120s，spawn 的 Child 没有 drop/cancel 回收纪律。本项源码已确认，必须补实际有效平台证据，不能用 Linux 返回不支持冒充 macOS 通过。
修复：系统语音也进入统一预算/配额/操作结算，使用既有受监督进程、进程组和取消/RAII 清理；处理等待、TERM/KILL、子进程回收、部分输出/临时文件与未知停止，不复制第二套取消系统。不把无供应商 token 的本地操作虚构成已收费 HTTP 调用。
自检：有效 macOS 环境正常语音、短 deadline、排队、运行中取消、future drop、进程失败、写盘失败；确认进程树/permit/临时文件可解释且有界。受控进程替身可补逻辑覆盖，但不能代替所声明的真实平台验证。

【R05-T07：完整调用事实、因果链与正确 usage】

F21｜失败调用漏账、未发请求虚记尝试；父子归属和查询字段不足
对应：A12/A13/A14，T06-C04，T07-C01/C02/C03/C05/C09/C10，原 T07 步骤2/4。
入口：service/workermodel.rs:269–310；adapters/models/auxiliary.rs:149–225；service/operations.rs:184–193,247–269；workerrpc.rs:847–856,947,958；kernel/usage.rs:320–372；storage/migrations.rs:251–274；run_store.rs:2863–2882。
事实：worker 真实模型500产生 HTTP1、usage0；operation queue timeout 产生 HTTP0，却固定 attempts=1；真实 ToolCallId 被丢弃，另造 invocation 后无持久 join；operations 强制 session/run/parent/cause=null；缺开始/结束/耗时/结果状态/日期筛选返回和 parent model 等必要字段。
修复：统一调用上下文和逐物理尝试事实，区分 not-sent/sent/settled/unknown，成功、错误、空结果、部分流、取消、异常协议、刷新/重试均携带 resolved identity、已知 usage 与真实尝试。已可能计费不能消失，未知不能变0；未发不能虚构1次。operation 支持合法独立根，同时能承接真实 session/run/parent。真实 worker RPC 传递并持久化 parent ToolCall/ModelCall 关系；补齐原要求的时序、状态和查询/导出所需结构及迁移。保持先落盘后发布，调用取消 fence 不得抹去已经发生的计费事实。
自检：success/500/429/401刷新成功失败/空正文有usage/意外工具响应有usage/partial/cancel-before-send/cancel-after-send/queue-timeout/store-failure/crash-recovery；逐次对账受控服务器真实请求数、尝试状态、行数/去重和 unknown。通过真实 worker 子进程链 JOIN 到父工具和父模型，不能测试端伪造一个形似父ID的 invocation。主会话/后台/worker/辅助/媒体、owner/session/日期/类别/model 筛选、重启查询、权限隔离与无秘密导出都覆盖适用项。

F22｜usage 输入先被宽松数值转换，invalid detail 还永久保存任意内容
对应：T07-C05/C06/C09；与 F05 共同验收。
入口：models/operations/rerank.rs:170–200；embedding.rs:376–391；models/usage.rs:278–349；service/operations.rs 的 usage sink。
事实：rerank input_tokens=null、output_tokens="7" 被转换为0/7并标 Reported；合法 embedding 的 usage 数组含合成凭证，整个数组进入永久 invalid_detail。原报告写“非阻塞”不能覆盖可达泄漏。
修复：保留原始字段类型后严格解析非负、范围内整数和总量/分量；missing/null/字符串/异常不能先转合法数字，避免 f64 中转损失整数。诊断仅保留安全的字段路径/类型/原因，并脱敏有界；operation 成功与 usage invalid 分开表达，不为清除诊断而捏造 usage。扫全 embedding/rerank/ASR/媒体等现役输入。
自检：missing、null、false、数字字符串、负数、小数、溢出、超安全整数、数组/对象、分量不一致；合法0仍为0，非法为invalid/unknown而非Reported。包含合成 key 的异常 usage 经过 DB、API、诊断/已有导出后仍无泄漏，合法 vector/操作结果不受损。

F23｜Gemini thoughts 与 candidate output 的包含关系错误
对应：A14，T07-C07/C04/C05/C06。
入口：models/usage.rs 的 Google inclusion rule 与 candidates/thoughts decode；kernel usage wire 投影；r05_t07_usage_families.rs。
事实：prompt=100、candidates=10、thoughts=40、total=150，当前表声称 thoughts 已包含在 candidates，输出记10。Google 将 candidate output 与 thoughts 分字段，总生成输出应能正确表达50；现有测试固定了错误口径。
修复：先明确统一 output 字段定义，再一致修复协议公式、component/inclusion 标志、wire、聚合、持久化、文档和测试。若 output 定义总生成量则归一为50；若保留候选字段则显式区分分量并提供正确合计。不能对已包含 reasoning 的 OpenAI 再加一次，也不能把 Anthropic cache 关系硬套到所有族。
自检：无 thoughts/有 thoughts/thoughts>candidate、缺失组件、真实0、total交叉校验、累计快照/增量、重复usage、五族差异与混合聚合。预期独立来自协议语义，不从被测公式表反算同一错误。缺失信息保持 unknown/partial，费用没有合法价格来源不编造。

【R05-T08：正式闭环、可信门禁、资源与交接】

F24｜正式启动只注册三个文件工具；worker 仅测试手工接线
对应：原 R05 放行“四工具”，T01-C02/C08、T06-C04—C11、T08-C01/C11。
入口：service/lib.rs:1019–1021,1072–1080,1150–1205,1376–1380；workerrpc.rs:1478；r05_t06_worker_model.rs:884–930。
事实：正式模型 wire 只有 edit/read/write，没有 exec_command；生产创建 worker model callback 端口，但未注册 worker 工具。C11 引用 ServiceDeps、StepsProvider 和手工注册的 T06 测试，未证明正常二进制链。
修复：在受配置控制的正式组合根中注册 read/write/edit/exec_command 及一个受控单操作 worker，复用 R04 审批、资源、进程监督、可信 worker 校验与宿主模型端口。按已核准平台能力启用，不扩展成任意安装/任意脚本执行；不提前做 R07 全 worker 产品。
自检：实际可执行文件+真实配置+认证 HTTP，禁用 Pi/旧 fallback；四基础工具成功与权限拒绝/取消/失败/重启，外部文件与进程计数为证。T08-C11 必须模型全局 permit=1，主模型HTTP→工具→真实子进程worker→RPC callback→宿主Gateway真实HTTP→worker工具结果→主模型最终回复，核验无死锁、无子Agent loop、无秘密传递、父子usage可JOIN、取消可回收。正常二进制场景不得注入 ServiceDeps 或替代 RunSupervisor/ToolGateway/Storage。

F25｜六个独占叶被通用 stage_share_satisfied 误放行；其余叶也需逐项重核
对应：R00 适用叶、T01-C01、T08-C14；与 F04 联合。
入口：r05_t01_build_scope_matrix.py:31–34,166–178；scripts/rust-tauri/r05_t08_generate_leaves.py:49–66,96–111；r05_t08_stage_suites.sh:273–292；xtask/verify.rs:840–935；stage_map.rs:3079–3165。
事实：生成器按首个 Task 选择通用 suite，将 suite 全绿复制成各叶 actual=1；六个 R05-only 叶 originalAssertionCases=[]、deferredToStages=[] 却 PASS，reason 还称后续承接。
修复：从原 R00 断言与执行阶段重新推导本阶段真实份额，明确 full_original_behavior 或合法 share、具体断言与生产者。独占叶不得有无人承接的 remainder；后续 UI 真实份额不得被 R05 机械继承完成。扫描当前130叶，若原清单还有漏叶追加并说明，不能人为固定数量来排除新发现。
自检：六 OAuth 叶逐服务行为证据；每个其他叶都有可追溯断言/有效同场景共享关系。删除任一独占行为证据、把独占叶伪改share、空承接阶段、把后续份额批量PASS都必须被 gate 拒绝；不能用“所有叶必须是share”的镜像固化错误。

F26｜C-ID 完整身份与实际执行关系校验不足，门禁可被换名骗过
对应：全部16A、100原C、3已追加C、原16门禁负测，重点 N01/N03。
入口：xtask/stage_map.rs:3239–3264；r05_t08_stage_suites.sh:219–240；r05_stage_cids.tsv/pins/leaf map 与真实 assembler。
事实：把 R05-T02-C01 换成不存在的 R05-T99-C99，数量不变，7个镜像测试仍7/7；原 assembler 在复制日志fixture上 allCasesOk=true。只钉91个数量和9个示例ID不足。另12个无直接CID注册项中有合法间接绑定，不能全部一概判漏测；须验证每项真实替代生产者。
修复：以本文附录及权威原规格为完整必需 ID 集，叠加当前已追加的 T05-C11B/T05-C13/T06-C11B 和新回归；逐项核对需求、适用组合、唯一身份、真实命令/测试、执行与退出结果。非Cargo项目注册实际检查命令，允许共享有效一次执行，但不能未运行PASS、用空filter/ignored/伪日志填充。原16A和16负测不得删除或降格。同步删测试/降计数也必须被外部规格对照与独立审查发现。
自检：在隔离副本，每次有正常对照、单项故障注入和准确失败原因：删CID、换虚构CID、删间接CID生产者、删/改名/ignore测试、零匹配、同步降数量、缺日志、错误退出、旧候选日志、伪PASS、离线NOT_RUN伪装LIVE/不适用、子门禁失败外层0、坏叶share。变异必须真正到达对应验证逻辑，不能拿无关编译失败代替。恢复后完整门禁成功。

F27｜资源验收轮次不足、FD 采样器可能恒为0且缺原始时间序列
对应：专项§8、T08-C12、T05/T06资源上限。
入口：r05_t08_closed_loop.rs:2198–2305；R05_PERFORMANCE_RESULTS.json:41–47。
事实：现有主要循环仅2次热身+12次成功/500/重放，未满足100次以上重复取消与错误及worker/多会话负载；lsof默认首列COMMAND，按“行首数字”筛行会把服务记录过滤成0，且不验证退出状态。当前容器 RSS 采样失败是独立环境限制，不构成已证明的生产泄漏。
修复：使用平台正确、机器可读且校验有效性的 RSS/FD/连接/任务/permit/进程树/临时文件采样；失败为 UNKNOWN/BLOCKED，不能计0。实现前登记适用 PERFORMANCE_THRESHOLDS、负载、采样周期、warmup/稳态/清理窗口和容差，不看结果后放宽。已有更高门槛从其规定。
自检：先用已知打开/关闭N个FD、保留连接/worker的正负对照验证测量器能发现增长及回收；错误命令/错误PID/空数据/假0必须失败。再按原要求完成正常、长响应、至少100次以上重复取消与错误、嵌套worker、多会话负载，分清网络等待和宿主开销；所有实际参与进程纳入，不只主线程。保存原始时间序列、每轮结果、阈值/环境/配置/fixture/二进制摘要和最终清理状态。无测量数据不能写“测试通过即证明内存有界”。必须在能正确采样并满足前序门禁的平台真实复验。

F28｜阶段报告、已验候选、延期与提交回执不一致
对应：T08整体交付、原04交接规则。
事实：当前 report/handoff 仍写未提交、independent_review=PENDING，而已有审查PASS和实际远程提交。此项是状态收口，不据此单独否定历史源码测试；但修复后不能继续提供矛盾的交接。
修复：先以真实候选状态准备完整待审材料；阶段独立PASS后再统一收口 R05_REPORT、HANDOFF、scope、callsite/provider/protocol/credential/usage/worker文档、TEST_MAP、ACCEPTANCE_LEDGER、NEGATIVE_GATE_REPORT、PERFORMANCE_RESULTS、LIVE_VERIFICATION、BLOCKERS、PROGRESS_LEDGER 及根进度中的最终结论与回执。原误判保留历史说明，新的最终结论不得沿用旧PASS。纯证据归档可按已验生产内容相等规则继承，任何生产源码、协议、配置、lock、生成器或gate变化必须使受影响证据过期并重验。
自检：所有 accepted_tasks、F-ID、A/C/叶证据、review轮次、源码/lock/schema/config/fixture/binary摘要、有效平台、原始命令与结果可双向追踪；真正未执行的提交/推送/LIVE不得写已完成；无实质R05缺口残留在“非阻塞”“后续加固”“R06待处理”。报告自身的提交不强求循环引用自身SHA；用内容清单和真实提交回执证明被测生产树一致。

五、修复后必做的整体验收与防漏规则

5.1 四层证据及组合覆盖

第一层：纯逻辑/性质测试。允许受控时钟/随机源，验证类型、状态、预算、ID、公式和规范化；合法/非法双向对照。
第二层：生产 crate 集成测试。真实 Gateway/CredentialService/adapter/client/Quota/Run/Storage，外部供应商与 OAuth 另一端用受控服务器；故障用屏障控制，不靠睡够时间猜竞态。
第三层：正式二进制。真实配置与认证HTTP、真实文件/进程/worker、持久化和重启，不直接注入 ServiceDeps，不在测试端替产品修正结果。
第四层：LIVE。只在有效授权、明确账号/端点/预算下进行；没有账号不影响前三层必做，但不能伪造真实供应商通过。

每个现役适用协议：成功、工具往返、流式、确有的非流式、错误/截断、认证、取消、必要状态、usage。每个操作/认证类别至少覆盖其真实入口。权限、身份、秘密、外部副作用、取消恢复不能仅用少数pairwise样例推断全表安全。确实不提供的协议变体才可N/A，写来源和审阅；缺实现/缺机器/缺账号不是N/A。

5.2 必须通过的跨工作包场景

I01 配置代次：正式管理reload与401/refresh并发；A端点永远收不到B的新钥匙，旧handle失效，兼容策略与能力无跨代次混搭。
I02 OAuth六叶：正式登录到调用、模型管理、刷新、注销、重启全链；撤销赢过迟到安装；每叶真实状态/模型数/磁盘与HTTP对账。
I03 协议重放：每个现役族真实模型→工具→下一模型请求；必要签名/reasoning/会话引用保持来源、位置、顺序；工具结果不是固定占位。
I04 整批工具：合法write＋非法read(path=123)零副作用；同call内冲突ID零副作用；两个合法独立调用分别执行一次；跨轮合法复用允许。
I05 消息终态：过程→工具→答案及仅reasoning/opaque/mood、截断/取消；实时、final、快照、数据库与重启一致，只有真final才提交final。
I06 正式四工具与worker：配额1的真实子进程嵌套HTTP链；认证、审批、取消、未知停止、父子usage、重启均真实可查。
I07 网络：proxy/direct/NO_PROXY/显式CA/TLS拒绝与DNS/redirect边界同时成立；认证不跨域，错误体/队列/刷新/下载全在总预算内。
I08 媒体与文件：相对附件绝不读出授权范围；不同host/provider job ID正确；旧任务不改绑；取消胜出无迟到下载登记，真实产物可校验。
I09 调用对账：外部HTTP计数、物理尝试、父子ID、各类usage、耗时/状态、权限查询及重启一致；未知不变0、未发不算已发、失败不漏账、诊断不泄密。
I10 资源和恢复：原100次以上负载、测量器负对照、进程/连接/FD/permit/任务/临时文件稳态，以及R03/R04既有崩溃/未知结果/幂等保护。
I11 门禁自证：原16负测及F25/F26/F27新增反例均能使适当门禁失败；恢复后全绿，需求ID与原规格精确一致。

5.3 命令与环境

使用仓库锁定的工具链，冻结候选时记录 rustc/cargo/Node、OS/架构、lockfile、schema、配置、fixture与二进制hash。审查基线Rust为1.98.1；不要为了消除失败无理由升级、降级或改锁。新增依赖需固定版本、说明必要性及许可/安全影响。

实施入口和最终候选在各自全新证据目录执行前序门禁。命令示例中的唯一编号先替换为实际值，不把尖括号占位当shell输入：

cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R04 --evidence artifacts/rust-tauri/R05/RR1/ENTRY-唯一编号/verify-R04

开发中按影响范围测试；最终至少执行并保留真实结果：
cargo fmt --manifest-path rust/Cargo.toml --all -- --check
cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings
cargo test --manifest-path rust/Cargo.toml --workspace --locked
cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-contracts
cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries
cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R05 --evidence artifacts/rust-tauri/R05/RR1/FINAL-唯一编号/verify-R05

verify-stage R05 必须消费最终候选有效的R04结果或实际运行它，解析内嵌R03/R02/RR1，不能子门禁失败外层仍0。同一有效执行可以在编排中复用避免无意义重复，不能拿旧候选日志冒充新的独立运行。新阶段审查者需亲自执行正式阶段门禁，不只看执行者JSON。

公共接口/生成TS/schema变化时运行受影响的跨语言round-trip、现役类型/边界/构建检查。持久化变化遵守仓库指纹、迁移、DATA_EPOCH及恢复规则；不能手工改期望指纹绕过迁移测试。没有变化的无关平台打包不为凑测试量反复运行。

环境失败与产品失败区分：确认正确PID/路径、工具链与新target后再归因，不复制旧PASS、不把无法采样记0、不放松路径/沙盒/证据保护。当前macOS arm64前序证据要在其有效环境复验；已有Windows/Linux平台缺口按原登记继承，不要求现在完成R09/R10四平台产物，也不得标已支持。

5.4 每条证据的最小字段

caseId、F-ID、原A/C与leaf、协议/操作/认证/入口/平台、要求与预期、实际观察、测试名及真实命令、exit code、实际测试数及ignored/filtered/零匹配、开始/结束、候选HEAD及working_tree_digest、toolchain/lock/schema/config/fixture/binary hash、证据路径+SHA256、替身边界、执行者/独立审查者、失败或阻塞归因。

每个F-ID需要旧反例或后续已修证据、修复后正负对照、同类路径覆盖及独立关闭结论。修复过程中发现的新问题追加稳定ID与同样字段，不藏在聊天。故障注入源码只能在隔离副本，不能进入正式提交。

六、阶段放行公式与最终交付

6.1 R06_READY 的全部必要条件

只有同时满足以下各项，才能置 R06_READY=true：
1. F01—F28及修复中新增的R05必需问题全部经独立证据关闭；
2. 原16A、100原C、3已追加C以及必要新增C完整保留；当前130适用叶及新发现叶的R05真实份额均完成，合法后续份额仍有明确归属；
3. 正式四工具/worker/模型/媒体最小闭环、同源消息与重启、权限/取消/未知恢复、trace/usage已通过；
4. proxy/CA、DNS、错误body/queue预算、秘密扫描及资源测量/规定负载等本阶段离线义务全部真实通过；
5. 最终候选完整R05门禁、原16负测+新增负测、有效环境前序R04→R03→R02/RR1闭包通过；
6. 全新的独立阶段审查者亲自执行并PASS，生产源码/依赖/配置/协议与证据绑定一致；
7. report/handoff/ledger/进度/提交回执统一，无“核心功能未做但记后续加固”；
8. 剩余仅为原规则已经允许的真实账号/外部LIVE实测，以及合法继承且不阻断本阶段的既有平台义务，逐项记录范围、缺口、责任归属和截止R09/R10；不能新增延期豁免当前核心实现或必需有效环境验收。

正式状态必须保留原专项的区分：
offline_gate: PASS | FAIL | BLOCKED
independent_review: PASS | FAIL | PENDING
live_verification: VERIFIED | PARTIAL | BLOCKED_NOT_AUTHORIZED | BLOCKED_ENVIRONMENT
platform_verification: 按平台逐项真实列出
stage_readiness:
  ACCEPTED_WITH_LIVE_VERIFICATION
  ACCEPTED_OFFLINE_SCOPE_WITH_REGISTERED_LIVE_DEFERRALS
  READY_FOR_INDEPENDENT_REVIEW
  NOT_ACCEPTED
R06_READY: true | false
release_state: NOT_IN_SCOPE

R06_READY 是便于消费的派生字段，不代替正式阶段状态。使用离线受限定验收状态，必须明确“真实供应商/全部平台尚未通过”，不能写成无条件全产品完成。若必需平台门禁确因环境无法执行，保留待验/BLOCKED，不强置true。继续所有可独立完成的工作后，才向用户说明精确缺口；不要把普通实施选择反复交用户决定。

6.2 交付与R06输入

更新现有规范要求的R05交付物，不用新建一堆无内容文件替代它们。至少包括：R05_BASELINE、SCOPE_MATRIX、MODEL_CALLSITE_MATRIX、PROVIDER_SUPPORT_MATRIX、PROTOCOL_WIRE_MATRIX、CREDENTIAL_FLOW_MATRIX、MODEL_USAGE_SEMANTICS、WORKER_MODEL_BOUNDARY、INTERFACE_EVOLUTION、TEST_MAP、ACCEPTANCE_LEDGER、NEGATIVE_GATE_REPORT、PERFORMANCE_RESULTS、LIVE_VERIFICATION、BLOCKERS、REPORT、INDEPENDENT_REVIEW、HANDOFF及进度。修复目录保留完整问题矩阵、执行/验收轮次、反例和最终阶段审查。

HANDOFF向R06给真实可消费的ModelGateway/Credential/规范化交换与消息、上下文请求格式、工具结果与opaque来源规则、辅助/embedding接口、trace/usage查询、取消/恢复/预算语义、协议能力矩阵。必须含source_sha、working_tree_digest、protocol_version、data_epoch、dependency_locks、accepted_tasks、unresolved_items、allowed_next_scope、artifact_hashes；提供最小正常调用样例和错误/未知处理，不能只列拟用API名字。

最终向用户报告：R05正式状态与R06_READY；每个Task和F-ID关闭结果；真实测试/负测/资源结果及未测边界；独立审查轮次和候选；Git实际提交/推送状态；允许的LIVE/平台后续义务；R06开始时读取哪些文件。若true，明确可以进入R06任务执行，然后停止；若false，列明仍阻断的具体项及已完成的可审查成果，不把“还需检查”当已通过。

七、原验收身份与外部协议依据

7.1 原16A、100C和原16门禁负测索引见本文件后附，要求保持身份，不得只保留数量。当前另有 R05-T05-C11B、R05-T05-C13、R05-T06-C11B，读取其仓库完整要求并保留；新增修复回归可追加稳定ID，不重排/覆盖原ID。

7.2 技术语义核对时优先当前官方文档及仓库现役契约，不从测试期望反推正确性：
- Anthropic标准thinking流与signature占位：https://platform.claude.com/docs/en/build-with-claude/streaming
- Gemini thought signatures的原Part/工具回合要求：https://ai.google.dev/gemini-api/docs/generate-content/thought-signatures
- Gemini并行FunctionResponse分组：https://ai.google.dev/gemini-api/docs/generate-content/gemini-3
- Gemini candidate与thoughts计数：https://ai.google.dev/gemini-api/docs/generate-content/thinking
- DeepSeek thinking工具回合的reasoning_content：https://api-docs.deepseek.com/guides/thinking_mode/
- 冻结候选源码：https://github.com/ItsDalk-Lane/LingxiAgent/tree/d80737b6cb9186c8a18c0f35923aac00249d45c3

保留本文审查的准确边界：Google签名/分组是生产编码与官方契约的对照，不冒称已跑收费API；本次DNS反例是连接建立，不冒称拿到私有内容；macOS语音要补真实平台验证；已有253个通过测试及历史同源码证据不能被抹去，但也不能替代未覆盖的正确性要求。

附录A：原16个正式验收场景（保留身份和核心义务）

以下保留原任务书的场景身份与核心含义。附录B是对每项任务的细化和新增保护，不得用新的C-ID覆盖、改名或删除A-ID。实现时须逐C-ID建立精确的A-ID及实际功能义务映射，不能把任务内所有测试随便挂到同一个A-ID。

| 原场景ID | 验收主题 | 必须证明 |
|---|---|---|
| R05-A01 | 同名模型不串凭证 | 两个provider注册相同modelId；分别调用，实际endpoint、凭证及trace均属于所选provider。 |
| R05-A02 | 不支持能力提前失败 | 对未支持工具/图像等能力的模型发起该请求；请求前明确拒绝，不私换模型或构造坏请求。 |
| R05-A03 | 并发401只协调一次刷新 | 同provider同时401时协调刷新，重试受限；不能跨provider回退。 |
| R05-A04 | 撤销后迟到刷新不能复活 | 刷新过程中撤销/删除凭证；迟到token不能重新生效或继续取消后的请求。 |
| R05-A05 | 工具多轮协议匹配 | 真实工具结果以供应商要求的role/关联ID进入下一请求；无关联丢失。 |
| R05-A06 | 协议差异不被吞掉 | 必要特殊块和继续请求状态完整保留；不支持关键结构明确失败。 |
| R05-A07 | 任意分片保持语义 | 相同协议响应逐字节及随机切分后语义一致，工具参数完整且不重复执行。 |
| R05-A08 | 中断流不伪造最终回复 | 只有过程内容/半截工具参数后断开；保留partial/失败，不生成假final。 |
| R05-A09 | 流中取消释放连接 | 持续发送的模型流被取消后，读取停止、连接回收、迟到片段不入正文。 |
| R05-A10 | 外发阶段重试不复制副作用 | 服务已接受请求后断连；无已证明幂等/核验依据不自动重发。 |
| R05-A11 | 异供应商媒体调用正确 | 文本A调用媒体B，B承担请求且A凭证不泄露；产物结构化。 |
| R05-A12 | 辅助调用同受取消和预算约束 | 摘要/视觉等请求在取消或预算耗尽后不暗中继续；有相应记录。 |
| R05-A13 | 多轮不拆成无关轨迹 | 同会话多次任务的trace有连续关联，每次请求仍可独立追踪，后台根不误并。 |
| R05-A14 | 重复usage事件不多计费 | 同一请求重复usage不重复累加；缺usage不冒充实际0。 |
| R05-A15 | 无Pi真实工具闭环 | 独立Rust服务完成读改读并重启读取；真实文件、唯一终态、历史一致，不依赖Pi循环。 |
| R05-A16 | 关键失败可复原 | 文件编辑冲突和最终流中断各有明确原因；重开会话不重做已发生动作。 |

附录B：原100个必需检查点身份索引

以下是索引，不把短标题当完整断言。每项依原专项完整条文、本文修复和实际适用组合建立可执行验证。完整集合需外部规格对照，不可让实现者同时删需求与降gate数量。

R05-T01-C01｜现役范围无漏项
R05-T01-C02｜正常二进制真正接线
R05-T01-C03｜无配置诚实失败
R05-T01-C04｜同名模型不串供应商
R05-T01-C05｜配置快照与变更生效点
R05-T01-C06｜不支持能力零请求
R05-T01-C07｜辅助槽位确实独立
R05-T01-C08｜工具声明来自实际目录
R05-T01-C09｜工具目录变化不可误路由
R05-T01-C10｜调用身份不来自模型
R05-T01-C11｜本地无密钥兼容不扩权
R05-T01-C12｜依赖方向与范围不倒退
R05-T02-C01｜凭证解析唯一
R05-T02-C02｜并发401刷新合并
R05-T02-C03｜不同provider不互相阻塞
R05-T02-C04｜共享刷新取消隔离
R05-T02-C05｜撤销后不复活
R05-T02-C06｜持久化失败不假成功
R05-T02-C07｜刷新失败与循环有界
R05-T02-C08｜OAuth验证与一次性回调
R05-T02-C09｜秘密不越过宿主边界
R05-T02-C10｜重定向不泄露认证
R05-T02-C11｜配置并发与重启保真
R05-T02-C12｜钥匙句柄与权限隔离
R05-T03-C01｜每个现役协议真实编码解码
R05-T03-C02｜外部与内部调用ID区分
R05-T03-C03｜多工具乱序完成不串结果
R05-T03-C04｜相同callId跨会话隔离
R05-T03-C05｜真正读到文件再回传
R05-T03-C06｜混合内容不被枚举丢弃
R05-T03-C07｜工具结果语义端到端保真
R05-T03-C08｜拒绝和Future明确回传
R05-T03-C09｜名称/schema转换无歧义
R05-T03-C10｜协议opaque状态原样往返
R05-T03-C11｜服务端会话引用绑定
R05-T03-C12｜后续请求包含真实已发生历史
R05-T04-C01｜任意字节分片等价
R05-T04-C02｜非法UTF-8处理确定
R05-T04-C03｜SSE与协议帧组合
R05-T04-C04｜缓冲及解析成本有界
R05-T04-C05｜半截JSON不得执行
R05-T04-C06｜可解析JSON不等于调用完成
R05-T04-C07｜错误参数类型与歧义
R05-T04-C08｜交错工具增量不串块
R05-T04-C09｜完整批次与截断批次
R05-T04-C10｜重复与冲突事件可区分
R05-T04-C11｜停止原因语义完整
R05-T04-C12｜确实流式而非事后分片
R05-T04-C13｜MOOD/思考与普通文本不误伤
R05-T04-C14｜未知块与签名隔离
R05-T04-C15｜HTTP200后的流错误
R05-T04-C16｜取消与EOF竞态
R05-T05-C01｜分阶段超时与总预算
R05-T05-C02｜模型配额有界且释放
R05-T05-C03｜429及暂态重试有界
R05-T05-C04｜重试身份及实际请求可对账
R05-T05-C05｜已接受但无响应不盲重发
R05-T05-C06｜部分文本重试不重复拼接
R05-T05-C07｜工具副作用后重试不再执行
R05-T05-C08｜取消覆盖每类等待
R05-T05-C09｜自然结束与取消唯一结算
R05-T05-C10｜客户端重连不重新请求模型
R05-T05-C11｜URL与资源网络边界
R05-T05-C12｜代理与TLS不降级
R05-T06-C01｜操作与能力矩阵逐项落地
R05-T06-C02｜文本与媒体跨供应商
R05-T06-C03｜大资源引用和产物真实性
R05-T06-C04｜worker真正调用宿主模型
R05-T06-C05｜同步端口升级不阻塞运行时
R05-T06-C06｜嵌套模型配额不死锁
R05-T06-C07｜预算按真实调用身份隔离
R05-T06-C08｜预算不能由载荷扩大
R05-T06-C09｜worker不能选择越权身份
R05-T06-C10｜callback等待仍可取消worker
R05-T06-C11｜worker不继承秘密或私自直调
R05-T06-C12｜辅助/媒体生命周期诚实
R05-T07-C01｜多轮会话轨迹连续
R05-T07-C02｜后台/子代理/worker因果清楚
R05-T07-C03｜物理重试不吞计费事实
R05-T07-C04｜累计与增量usage正确区分
R05-T07-C05｜缺usage与中断保持未知
R05-T07-C06｜非法/超范围计数不失真
R05-T07-C07｜缓存与推理token口径一致
R05-T07-C08｜费用不编造
R05-T07-C09｜查询隔离及秘密保护
R05-T07-C10｜写入失败不发布虚假完成
R05-T08-C01｜发布形态二进制闭环
R05-T08-C02｜随机数据反作弊双复跑
R05-T08-C03｜读写权限对照
R05-T08-C04｜重复请求及冲突请求
R05-T08-C05｜完成后重启只读不重做
R05-T08-C06｜工具完成未写收据崩溃
R05-T08-C07｜取消/未知终态持久保真
R05-T08-C08｜并发会话交换隔离
R05-T08-C09｜流式与历史阶段一致
R05-T08-C10｜媒体最小链真实落地
R05-T08-C11｜worker嵌套链正式接线
R05-T08-C12｜长时资源有界
R05-T08-C13｜R04与R03真实回归
R05-T08-C14｜范围和默认旧产品不变

附录C：原16项验收门禁负向检查

以下只在隔离副本中注入，主工作树不保留破坏。每项须有正常对照、目标变异、预期失败、实际失败原因和还原验证。不能把无关编译失败当作证明目标缺陷被测出。对尚不存在于旧版的功能无需强求旧树同源码反例，用可编译的候选变异建立对照，不伪造旧红新绿。

| ID | 注入方式 | 必须观测到的门禁行为 |
|---|---|---|
| R05-GATE-N01 | 删除一个R05原始A-ID或一个必需C-ID | 门禁因缺失的精确ID失败；不能仅依靠另一个无关测试碰巧失败。 |
| R05-GATE-N02 | 将测试筛选词改成匹配零个测试 | 即使cargo退出0，生产者和门禁仍非零，并指出未运行名称。 |
| R05-GATE-N03 | 删除/改名/ignore一个必需测试，或降低固定最低覆盖数 | 名字、归属和执行事实校验失败；不接受只看总passed数。 |
| R05-GATE-N04 | 让失败子命令经管道或脚本返回0 | 门禁捕获真实失败；shell管道、子进程和后处理退出码不能吞掉失败。 |
| R05-GATE-N05 | 复用旧证据、替换日志为另一SHA/配置/fixture的结果 | 来源绑定/新鲜度/摘要检查失败；不能只按文件mtime或PASS字符串。 |
| R05-GATE-N06 | 运行门禁期间改变受验源码、生成器或stage map | 候选前后绑定不一致，结果STALE/FAIL，不得声明最终候选通过。 |
| R05-GATE-N07 | 删除R04回归引用，或让内嵌R03/R02/RR1失败而外层仍0 | 依赖闭包和子结果语义校验失败；不能把前序回归变可选。 |
| R05-GATE-N08 | 将正常启动接线移除，只保留测试构造器注入 | 正式二进制链的真实外发/工具断言失败；unit全绿不能放行。 |
| R05-GATE-N09 | 用固定done/第N轮模板或摘要替代真实工具回传 | 运行时随机nonce和下一HTTP请求检查失败；与正常对照旧红新绿。 |
| R05-GATE-N10 | 把供应商callId误当内部ID，或错配两工具结果 | 多会话同ID/逆序结果配对测试明确失败，不允许侥幸最终文字正确。 |
| R05-GATE-N11 | 在adapter把Unknown/StopUnconfirmed/truncated压成success/完整 | 结构化字段、model payload和journal的保真检查失败。 |
| R05-GATE-N12 | 把worker callback改回阻塞等待，或在主模型完成后不释放模型permit | 单线程健康/取消及concurrency=1嵌套链失败；有界超时说明原因。 |
| R05-GATE-N13 | 把原R04延期叶直接批量写PASS或删除 | 份额归属差集失败；未实现下游UI/平台不得被本阶段“继承完成”。 |
| R05-GATE-N14 | 将未授权LIVE测试填PASS，或在隔离验证中真实外发 | 证据模式/授权/端点检查失败；无live结果不得声称真实供应商已通过。 |
| R05-GATE-N15 | 让合成秘密出现在错误、trace、URL或worker环境 | 秘密扫描与边界负测点名泄露；不只测试正常成功路径。 |
| R05-GATE-N16 | 在验收后改公共协议/配置/代码却复用原最终报告 | 受影响证据过期；最终source/lock/schema/fixture/二进制绑定不符即拒收。 |

现在开始：核对候选与既有授权，建立完整问题/需求/证据矩阵，创建新的执行智能体，按上述依赖修复、独立验收、再次修复，直到满足R05放行条件。不要停在提出方案或请求用户逐项选择实现方式。
