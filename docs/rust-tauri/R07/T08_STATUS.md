# R07-T08 设置与管理盘点

状态：`IMPLEMENTING`。2026-10-18 根据当前工作区只读核算；本轮已获授权开始实现，不代表验收通过。沿用当前分支，不提交或推送。T07浏览器C/D失败保留交T12复查，不以进入T08冒充完整门禁通过。

## 责任与已有基础

`R07_LEAF_MAP.json` 中 T08 责任为 109 叶（D10 67、D21 42），包括 39 条供应商定义；R07 台账目前全部 `NOT_RUN`，基础场景 A15/A16 也为 `NOT_RUN`。R06 的实现可以复用，不能仅凭存在代码把 R07 检查改成通过。

- `agent_config_readback.rs` 已有助手配置读写、全局字段分流、provider catalog/插件/模型文件投影，以及 preferences 共用锁；需要核对运行中模型刷新与部分失败事实。
- `management.rs` 已有身份、设备、网络、网页登录、凭证撤销和 OAuth 模型管理。
- `userprefs.rs` 已有 goal/autolearn/quick-chat 等偏好，合并写入保留根对象其他字段。
- `skills.rs`、`character_cards.rs` 已有技能与组合管理、角色卡预览/导入/导出；现有 R06 集成测试可用作基础。
- `chat_wire.rs` 已有模型列表、下一会话模型选择、当前会话模型切换及忙碌拒绝。

## 已查明的真实缺口

1. `/lingxi/v1/settings/config` 的 GET 仅返回少量全局字段；T01 已注册有限 PUT（automation::preferences_route，仅 heartbeat_master 与 automation.permissionMode），不能误记为完全没有 PUT。其余全局字段以及 config/default-workspace、workspaces/recent 的现役读写/清空接口仍需逐项核对补齐。WorkTab 通用保存实际走 agents/{id}/config，该既有通道另行验证。
2. `/preferences/models` 仅 GET，无 PUT；返回模型、operation_models、thinking_level，缺现役搜索配置掩码投影及其补丁/显式清除行为。
3. `/providers` 家族除 summary 外，现役 api-key、fetch-models、discovered-models、test、模型 PUT/DELETE 均未注册。summary 只列当前 ModelGateway 里的供应商，不能替代完整供应商目录。
4. `/models/health`、`/models/auxiliary-vision` 未注册。不能以空结果充当健康或视觉能力证据。
5. `update_settings` 只出现在工具名单与权限分类，没有找到可运行的工具执行器。
6. 设置页 `settings/api.ts` 只映射 providers/summary；其余供应商管理请求仍使用旧 `/api` 地址。后端新增后必须同步调用方与接口定义。
7. `settings_ui::snapshot_route` 将完整 preferences 根对象返回，只替换 computerUse/experiments。若 search.api_keys 等存在，当前快照可能带出秘密；必须改为明确脱敏投影并用合成秘密经真实 HTTP 验证。
8. 角色卡导出经脱敏助手配置读取，导出内容主要为 agent 元数据、人格和可选记忆/技能，未直接导出 providers；仍需实际打包、解压核对默认不含秘密，以及损坏输入不写半成品。

## 建议顺序

1. 先补全局 config 和 workspace 历史/默认目录面；preferences/models 深合并、秘密清除与权限；统一锁、损坏拒绝、单项保存不丢其他设置。
2. 再补供应商管理路由及 health/vision。使用现有凭证、OperationService、调用记录与取消链；外部响应以本地替身验证，不能直连真实账号或绕开统一调用链。
3. 同步设置页路由映射、实际操作回读和 update_settings 工具；验证新建/编辑/删除后的运行时效果，错误保留真实部分成功状态。
4. 复用 skills/card 基础，逐叶补 R07 证据和 A15/A16；每项写入需含重复、撤权、重启、损坏与资源回收边界。

现役桌面选择技能目录和独立技能查看器已有 platform 桥与 Tauri 实现，r06_skills_completion 的原生窗口检查会构建当前资源、执行读取边界及 skill_viewer_host 并要求实际回执；不能沿早期递延说明直接判成未实现。本轮尚未运行此检查，仍待核验，不把服务端读取当成系统窗口通过。真实供应商账号、付费调用、其他操作系统及正式打包尚未在本次盘点验证。


## 本轮独立只读核对补充

责任原条目已原样提取到 T08_READONLY_HANDOFF.json：D10 67、D21 42，总109，包含39条供应商定义；没有修改 R07_LEAF_MAP 或验收状态。身份/设备已有真实 management/AuthService/WS 重验链，作为管理基础复用，不擅自增加叶数。角色卡已有实际预览、令牌确认、素材验证、导入/导出和 Electron 联验，可优先复用；默认导出不含模型配置的结论仍需合成秘密解包验证。

新增两处必须修补：

- 外部技能路径保存用 read_json_file 失败后空对象回退，可能覆盖损坏偏好文件和其他设置；也未使用其他偏好写方共用的 preferences_locked。须严格读取、共用读改写锁、保留未知字段；paths数组还应逐项验证字符串，不能把非字符串静默过滤成空列表并保存。
- show_card 工具虽已注册，InteractiveCard 页面仍 PUT/GET /api/cards/{id}；Rust 服务未见对应进程LRU缓存与GET/PUT路由，也未见页面路径适配。须补同一服务的有界缓存、实际HTML/CSP与路由映射，再通过真实工具结果到iframe加载的闭环验证。对应 B03F2C42EFF1、62F23BAAA609 和 EE2ACB390FCE。

通用配置导入/导出入口在本轮查看的 General/WorkTab 与 config 路由中未找到；实际存在记忆导入导出及角色卡包导入导出。A14正式导入联验留T08查明实际入口，不为检查虚构按钮，不把直接改fixture或只读快照当导入通过。以上为源码调查，尚未运行T08检查。

## 当前实施与开发验证（2026-10-18 续）

当前仍为IMPLEMENTING，T09尚未实施。以下均是当前工作区开发检查；尚未冻结最终候选，不提升原109叶343断言验收状态。原始失败及其版本、命令、日志保存在T08证据目录，T08_FAILURE_HISTORY.json保留逐次失败；后来的通过不改写旧结果。

- 全局/助手设置、辅助模型、供应商、共享网络、导入导出已实接；配置保存只有同一个模型发布入口。编辑单项保留其他设置、凭证和未知兼容内容，部分保存失败如实返回事实。
- core-flow11真实HTTP设置15项及供应商7项通过；adapter-settings2实际223通过13忽略；协议往返protocol-roundtrip3覆盖203文件94样本通过。此后新增MCP协议仍须重新生成并检查，不把旧结果当当前全量通过。
- matrix-and-private2逐35个聊天身份真实HTTP请求通过，覆盖34个密钥身份和免密钥Ollama，各自协议、凭证、模型、usage。另两OAuth/两语音身份保留其明确边界。该轮私密来源检查因测试误放封闭顶层扩展字段失败；修正为合法metadata后oauth-and-private1模型配置8/8通过，含两类Source与保留原件0600。
- OAuth设置页旧入口断链已修；后端同一精确别名、PKCE/device、退出/撤销/真实凭证消费的oauth2通过。13项前端针对性检查通过；service-build3后的oauth-real-ui1实际组件按钮→生产Rust子进程→回环外端→私密凭证/summary/health→退出通过。授权网页打开以记录URL及真实GET代理，不证明系统浏览器启动或真实账号。
- settings-real-ui3实际导出→文件输入导入→真实存储回读通过。互动卡片旧整机凭证外送已真实复现并保留；短期票据与实际桌面/手机父策略复测未收到外送。Tauri聊天卡片宿主、旧Node卡片以及手机完整入口的限制继续保留。
- native-skills1原生技能查看器真实窗口及读取边界通过，原截图已复核；native-picker-cua1真实系统选目录/取消均正常，路径保留、未安装、隔离材料不变、进程及端口退出。后者没有捕获应用原始退出码或IPC Option，未虚报exit0/null。
- 受影响旧设置局部回归legacy-settings3的助手配置14/14及另外四目标通过；native-settings4实际窗口13/14通过，剩余failures场景已证实身份部分保存与失败提示通过，最后被底页同名标题误判为弹窗仍在，已修精准等待，native-settings5原failures场景独立复验通过（1通过13过滤，18项真实动作）；原native-settings4仍保留13/14失败，不改写成整轮14/14。原窗口失败及安全诊断全部保存。
- model-regression1实际144通过12失败。失败集中在仓库外隔离二进制未带产品模板，模型请求前即停止；调查记录明确哪些为原DB事实、哪些为源码确定性推导。后续runner显式提供当前仓库lib，补齐启动环境后binary-context2原两目标13/13通过；不关上下文编译、不扩大限流或等待预算。
- 原MCP设置成功分支缺失已确认，正在落单一管理者、请求/应答、九动作、真实SDK、权限与目录代次；未称完成。root接入同一ServiceState、启动恢复、退出信号及关库清理。mcp-process1实际19项原SDK/worker检查及2项顽固子孙进程/初始化取消检查通过；新管理集成、锁内审批复核、Windows只读静态修复仍待统一运行。HTTP/SSE及MCP OAuth当前未接通的边界不得当成功。


- mcp-manager1未执行用例即因新增测试编译失败，已保留并修正；mcp-manager2实跑8项4通过4失败，九动作工具缺可确认摘要、取消用例提前失败及注册后退出清理未确认，现场与原日志单独保全。独立预审还发现原始网页身份在Run内被简化、单次MCP请求排队预算及监听失败时loop停止缺口，正在沿真实链修复，不能据已接线称通过。mcp-request-cancel2在此前源码上实际验证3类单请求取消通知及共享连接可再调用，1项通过；随后预算窗口另修，须重跑。
- mcp-protocol-roundtrip1新协议往返通过，types5三套类型检查通过；仍是开发轮，不是同一固定候选的验收。

- 最新t08-peripheral-check1同源码MCP13/13、语音身份2/2通过：跨阈值真实模型声明、inflight Unknown不重放及实际回收、原HTTP/tool队列撤权、WebSession拒绝、Mac注册收尾均实际验证。语音明确为预置独立槽位+真实HTTP改provider后同一Operation实例即时消费，system真实say文件/usage0与BigASR真实回环请求/usage1，未外推页面重绑槽位或系统ASR已实现。该批OAuth首次按品牌协议真实聊天失败，保留完整隔离现场；补外端合成JWT账户声明与精确codex/responses路径后oauth-brand4三身份实际登录、真实Run/用量、增删、退出与撤销通过，保留Codex独立health明确skip原语义。
- mcp-cancellation-and-queue2同源码7/7通过，含真实SDK1024项满队列及76项未入队、取消零晚投递/零重放、连接复用和进程退出；Mac仅在确认子进程已退出且实际组列表为空时处理退出记录EPERM，独立限定审阅未见新增回归。mcp-manager5的648lib、19旧SDK/worker、4助手目标、2进程、7provider、15设置均通过，但整批旧MCP/OAuth失败仍保留。以上均为开发结果，尚非冻结候选验收。

下一步：解决剩余实际失败，完成MCP管理链检查、生成协议及适用回归，再冻结同一候选运行完整T08门禁并独立核验。T07浏览器两项失败继续留T12，不重开整轮R06。本轮未提交、推送或触真实账号。


## t08a冻结检查与独立审阅（2026-10-18 07:45 UTC）

- t08a冻结634文件，源码指纹cde9214ec9ea667afccdb5f5fd3819ff0eb114690f365b82520d2f1ff63f2134。fmt、diff、clippy、types在同一指纹通过；protocol实际退出1，MCP五份golden漏生成且两索引漂移。其余门禁明确NOT_RUN，t08a没有完整通过。旧SOURCE_MANIFEST、原归档、所有回执及原GATE_PLAN_t08a均保留。
- 已沿官方生成器补齐协议，protocol-sync-check1实际退出0；这是修复后的开发验证，不覆盖t08a协议失败。
- 独立审阅发现损坏headers输入被当空映射保存。headers-invalid-reproduce1在真实Source/网关链中复现saved/applied=true、generation3，0/1失败，原日志和隔离配置现场已保留。修复保留原null显式清空，拒绝false、列表、字符串与数字，固定候选仍待复验。
- 模型保存排队期间撤销原始身份的复核窗口正沿共享AuthService处理；未运行反例前不宣称已证实失败或已解决。
- 对账补入现役ModelsTab及providers组件10套件119项，current-model-ui1实际通过；原冻结计划漏列r06_chat_wire及这组现役页面，新候选计划已纳入，不能借开发通过替代新候选结果。
- 发现角色卡export/plan兼容别名及预览素材HTTP边界检查缺口，正做最小补齐；卡片varsCss超16KiB应沿原语义清空，HTML整体限制独立保留，原先超额直接413的偏差不视为旧断言通过。

T08仍IMPLEMENTING，T09—T12未开始。T07浏览器失败继续保留交T12。本轮未提交、推送或使用真实外部账号。


### 审阅修复停改后检查（2026-10-18 07:50 UTC）

- headers-invalid-fixed1模型配置9/9通过，包含原null清空与损坏headers拒绝；该轮存在其他并行源码变化，明确不是冻结候选通过。
- 相邻endpoint/base_url显式非字符串原来也会静默回退旧地址，已补类型拒绝并保留空字符串合法语义。model-authority-core1编译在修复落盘前开始，新反例随后进入测试编译，实际9通过1失败；该轮sourceUnchanged=false，保留原失败但不将混合时点产物冒充当前修复后或固定版本结果。其余目标原结果按日志保留。
- 同一AuthService已接入唯一模型发布者，保存排队后与原子写、重载、秘密读取、真实探测派发和缓存写继续复核原身份；新增5组真实HTTP撤权检查，不替换正式写锁或授权结果。
- 角色卡最小兼容修复完成，新增3组真实HTTP检查。原loadPlan没有时间TTL，失效检查仅指真实计划文件已移除，不声称覆盖计时过期。
- 以上产品/测试已停止编辑，根端final-review-fixes1统一运行新授权/卡片预览及相关模型/卡片/供应商检查。仍待实际结果，之后再冻结t08b并运行完整相关门禁；所有未执行项继续NOT_RUN。


## t08b主动取消而非验收通过（2026-10-18 07:57 UTC）

641文件指纹2b35cc84f1f48edfdb26bde1053e91d6254dbc59fe252fe92a31f1dbfd40f443；fmt/diff/clippy/protocol/protocol-roundtrip/protocol-tests/types同指纹退出0。逐叶对账确认media_admin模型管理仍走旧provider-catalog/兼容Pi投影，未沿唯一Source发布；这是实际T08接线责任，不能推给T11或靠已通过的语音provider编辑检查代领。根主动向本批kernel Cargo发送SIGINT，原kernel退出-2、wrapper254保留，后续及UI均NOT_RUN，不将此取消说成kernel业务失败。t08b清单、归档、计划快照、原回执和取消原因均保留。

媒体模型管理正在接入同一Source/网关及真实HTTP检查；另一独立任务补搜索凭证显式清除和现役语音默认选择的直接验证。系统选目录、脚本自导航拒绝及复合UI条款需按当前候选另补原生/具名证据。T08仍IMPLEMENTING，后继候选必须重新冻结；未提交推送。


### 原生直接补验及后继检查准备（2026-10-18 08:03 UTC）

用户再次解锁后，native-picker-ipc2真实系统选择/取消及只读窗口拒绝1/1通过，原始cancel IPC明确null；路径保留、未安装、进程/驱动端口退出。当前原生二进制SHA dd7582b9b59de8f4b3739a97309f10b4790e5e3167932e5b6fd4d15d3a125281，与既有产物同字节；原生18入口文件未变，另在运行后立即补35文件源码/构建/权限/依赖锁闭包，不伪称后补文件有运行前SHA。后继候选仍要正式native-skills重新构建后核对完全相同二进制、闭包及原始回执/日志/材料，作为相同产物的既有人工步骤证据，不说后继又人工重跑。应用自身原始退出码未捕获，不由Cargo exit0推断。

新ModelsTab默认语音合成选择、clear、失败保留和ASR独立4例已补，同原文件13/13开发检查通过；这是组件请求契约检查，应答mock，不伪称真实Rust生成。新search_clear三组真实HTTP/重启检查已就绪，须根端后续实际运行。媒体管理Source接线仍在实施，根端不在其编辑期间编译该库，避免混合时点产物。


### 媒体统一来源首轮实测（2026-10-18 08:11 UTC）

media-source-and-search1同源码运行：原媒体管理10/10、媒体生成运行11/11（含实际视频/ASR分阶段及jimeng监督链）、模型配置10/10、原身份撤权5/5、搜索密钥清除3/3、两语音身份2/2通过；整批因新media_source0/2失败仍exit101，未改写。新第一例已走过四类CRUD、实际speech/image产物和usage、损坏compat诚实partial及重启，最后自行恢复compat漏catalogVersion=2，DELETE返回真实saved+applied/projection_unconfirmed500导致原200断言失败；第二例低权限设备原本被LocalOnly403拒绝、未入队，测试错误等队列而超时。原两失败现场与日志均保留。

后续仅修新夹具/前提：完整合法兼容结构恢复后继续要求正常200；设备403明确零写，另让真实owner已过guard进入写锁队列后替换token并要求拒绝；stale编辑采用全新隔离home，不借被撤销旧身份。未扩大权限或吞partial错误，媒体safeJson原显式非JSONfallback合同保留，未伪称所有原始坏JSON拒绝。删除本轮改接后唯一失用的private provider_overlay读取辅助，未关闭dead_code门禁。media-source-fix2正待实际结果。


media-source-fix2在停止编辑后2/2通过，sourceUnchanged=true：媒体新增/修改/删除已沿唯一Source与共享Operation真实消费，四类CRUD及speech/image真实生成/文件/usage、重启、旧投影损坏时诚实partial、拒绝旧选择、原owner排队撤权和代次防复活均有具名断言。此开发检查没有代领video/ASR新增模型的远端执行；已有11项T06媒体链覆盖其原实际执行，真正供应商组合仍按T12/R10边界。旧两个失败原样保留。

逐109叶/343原断言的t08c草稿已更新133个具名case及共享机制覆盖依据：各品牌实际正例与共用gateway/credential代表拒绝分开，7槽页面/请求与核心操作实际消费分段组合，不冒称每品牌全部负例或同一原生全链已执行。部分复合UI错误组合继续逐条列证据边界，不按套件绿色平均分配通过。下一步严格代码检查后冻结c再统一运行；c尚未冻结，不登记最终通过。


## t08c原始失败与脱敏探针留档（2026-10-18）

候选c指纹 `b942c6d4ee882eaea5249cd49242ec6fa3d434505b09ff9af6064d413a2d40e0`：fmt、diff、clippy、protocol、protocol-roundtrip、protocol-tests、kernel、boundaries、adapter、types十项原始退出0；service原始退出101、源码未变，是实际失败，未取消，也没有完整通过。model-regression、native-skills、native-picker-binding、skills-current、build、ui及两项card-navigation后继检查均NOT_RUN，不能借开发轮通过补领。

- 服务lib实际647/648，唯一真实SDK队列用例在读取paused.pid时遇空字符串。原因是暂停确认文件创建和内容写入存在可观察空窗；仅夹具改为真实暂停后完整PID写入同目录临时文件、原子发布，保留1024+76压力、全部取消/零重投/恢复/回收断言及原时限。修复等待根端实际复验，不覆盖本失败。
- r00_management_leaves实际0/1：真实LAN地址的网页登录请求在20秒写读预算到期时仍0响应字节。没有包级或防火墙证据，不把TUN/防火墙或服务阻塞猜成已确定根因；不以loopback结果替换此失败。
- r06_agent_settings_live实际13/14：原则POST200已落盘，下一GET409；原夹具在网络完成后立即查看页面，未等React错误节点出现，存在过早检查窗口。原ui-result、请求trace和原则落盘材料已按白名单逐字节保全至 `artifacts/rust-tauri/R07/T08/t08c-native-failure-preserved1`，附原件/副本哈希；排除凭证、数据库、浏览器档案和缓存，原场景未改。不能据此认定产品丢旧列表或把该用例改成通过。

metadata-redaction-reproduce1/2/3均原始退出1：第一轮服务退出2但仅有stderr哈希，没有原stderr，原因未明；第二轮设置请求因路由用途不符返回400，未进入读面观测；第三轮预置未stamp home被epoch安全闸正确拒绝。三轮都不是已执行的泄漏验证，不证明泄漏存在、已修复或脱敏通过。逐次原命令、退出与哈希已追加T08_FAILURE_HISTORY，未改原证据。

T08仍IMPLEMENTING；T09—T12未开始。c后必要修复使其不能继续作为同一候选，后继需重新冻结检查。未提交推送，未运行Cargo，未改原验收账。


第四次metadata-redaction-reproduce4已真正完成助手创建/设置保存200，并观察真实HTTP读面：summary泄漏api_key、access_token、refresh_token；config/export各泄漏后两项。此为本机合成秘密的实际脱敏缺陷，不能与前三次准备失败混称“未运行”。原脚本退出1、来源兼容extension保留、服务退出0、二进制前后同SHA；日志仅保存字段名与应答哈希。根端已开始修复，原回执/日志及失败记录保留，当前尚无修复后通过结论。


### 模型扩展回写检查的正文格式更正（2026-10-18）

metadata-source-permissions-fixed1自然结束exit101、sourceUnchanged=true：新增metadata检查2/3，authHeader真实凭据回填与无secrets.write实际publisher拒绝两项通过；其余modelAuthority5/5、modelConfiguration10/10、mediaSource2/2、searchClear3/3按原日志保留。失败发生在原样遮盖列表首次HTTP回写，返回200但modelConfiguration:null。现役助手配置写入口直接读取正文，不拆values；测试错误的values.providers被当作未知助手字段保存，没有请求模型Source保存。这不是产品按id回填失败，也不能放宽saved/applied断言来领通过。

仅新测试正、负HTTP正文两处去掉values包装，继续严格要求真实modelConfiguration.saved/applied和Source重排、同id保留、新id不借用、主人清空事实。原receipt/log逐字节保全，隔离Source脱敏副本、原误包助手配置及白名单清单存于metadata-source-permissions-fixed1-preserved1；原scene未改，排除凭证、数据库和浏览器档案。格式检查通过，修后Rust检查由根统一执行，本代理未运行Cargo。

追加更正而不改写原记录：metadata-redaction-reproduce4与metadata-redaction-fixed-binary1旧脚本同样误发values.providers，其200不能证明模型保存/发布。两轮启动前Source已含合法extension，所以旧版本三读面真实泄漏、修复二进制三读面真实遮盖观测仍分别有效；先前“实际设置保存200”的措辞仅对应未知助手字段写入，不能外推唯一模型发布已生效。根端修正裸providers探针另用新编号，旧回执/日志/退出码保留。T08仍未完成，后继固定候选仍须实际全项检查。


metadata-redaction-fixed-binary2已另号实测exit0、sourceUnchanged=true：裸providers请求实际返回modelConfiguration.saved/applied=true，脚本严格读取原Source确认模型name真实改变且原extension完整保留；summary/config/default export三读面均无已知秘密，普通字段保留；实际服务退出0，前后二进制同SHA21458f19bdc4f1becc1366c3e395ed63dd60a769357713cf77fca97196479383。此为修复后真实保存与读取的开发实证，未覆盖新3项metadata全回归或后继冻结门禁；不覆盖、不抹去原reproduce4泄漏和fixed1错误夹具失败。


metadata-source-permissions-fixed2仅重跑修正请求格式的新3项，实际3/3通过、exit0、sourceUnchanged=true（原日志SHA6c6895b76394a8b375478c61094e81a284f4f4cf1268bd1129229caa6da7d12f）。遮盖列表真实保存/生效、重排按id保持、新id不借密钥、主人清空、无秘密权限双层拒绝和authHeader统一凭据出口均保留严格断言；fixed1原失败与隔离材料不改写。后继t08d已正式冻结20门禁，原109叶343断言等待本候选具名回执逐项填证，不以本开发3项通过代领整阶段。


## t08d服务正式失败留档（2026-10-18 08:54 UTC）

冻结指纹 `ce4642863c79eaa2d15dedff7c579b56649a9bc2c4cf133e6647faa70879664b` 的服务命令自然结束退出101；原回执sourceUnchanged与verificationToolsUnchanged均true。原命令、完整日志哈希及逐目标分组已追加T08_FAILURE_HISTORY，未取消、未改写为通过。

完整原日志共有34目标、877通过/2失败，0忽略/0过滤；33目标全部通过，只有r06_chat_wire目标15/17失败。按互斥文件分组：服务lib为648/648；r07_t08前缀的16个直接集成目标为77/77；其余17个适用旧管理/身份/聊天/媒体/技能局部回归为152通过/2失败。此分组只是日志核算，不代替109叶/343断言逐条结算。

两处原失败分别为：

- real_ws_resume_recovers_exact_missing_events_and_marks_stale_or_empty：最终事件isStreaming实际Null，原断言要求false。只读诊断确认原夹具只等数据库无活跃Run，尚未等待独立传输投影终态；原资料不足以确定最后实际内存帧是哪一种，不补猜。
- native_current_app_pages_side_preview_attachment_and_model_response_generation：app-main-current-and-runtime谓词实际null；聊天结束/流停止之后即要求独立React.lazy运行胶囊已挂载，缺少实际DOM就绪屏障。诊断与既有安全现场说明此检查前提，不把本轮原生失败改成通过。

现场引用既有 `artifacts/rust-tauri/R07/T08/t08d-chat-wire-failure-preserved1/manifest.json` 与 `t08d-chat-wire-failure-diagnosis1.json`，没有重复复制或修改原材料。原LAN管理本轮实际1/1、lib648/648、原生设置14/14通过；这不覆盖t08c原LAN失败或固定副本的真实复现，也不说明其具体根因已修好。

根端仍在继续其余独立门禁，整轮尚未完成；尚无完整有效回执的后继继续NOT_RUN。T08仍IMPLEMENTING，外围执行器清单仍是草稿，未改原账/Source/冻结计划或工具。本次仅追加两份文档，不运行Cargo。


## t08d整轮关闭与后继修复（2026-10-18 09:03 UTC）

20门已全部实际结束，18门退出0，service101（877通过2失败）及ui1（65文件538通过2失败）。RESULTS_t08d.json关闭前重新按完整source_paths/rows重算仍647文件/fp ce464286...，全部20门源码/4工具均不变、原日志哈希逐项核验，四项构建后检查均绑定正式服务二进制d91fbf82...。原生技能1/1、picker同产物证据绑定、技能其余4/4、模型回归、实际二进制脱敏及桌面/手机策略导航均通过；不由此填平聊天/界面失败。提前启动四项构建后检查曾因build回执未出而被拒，产品命令均未执行，独立t08d-postbuild-premature-start1留档；之后才正常执行，不覆盖编排失败。

后继e仅修检查：真实WS完整终态就绪、AppPages真实lazy DOM就绪、OAuth旧mock导出缺口、Bridge现役media/jobs替身缺口；另补实际消费者在中间恢复快照后继续接收同流尾帧的具名检查，保留原判据和时限。源码编辑已在d关闭后解冻，不借d通过项代新候选。原LAN本轮1/1过但此前失败原因仍未明；T08仍IMPLEMENTING，T09—T12未实施，无提交推送。


### e停改与开发复验

两处聊天夹具修复后chat-readiness-fixed1真实Rust聊天17/17通过、同源码退出0：DB终态和同流真实WS idle共用原5秒截止；main原20秒直到条件内同时等回复/非忙及真实welcome hidden/runtimeCapsule，不增加第二段等待，34动作不减。两处旧UI替身及新中间恢复消费端检查在ui-consumer-fixtures2实际23/23通过，三个测试文件运行中未变；首轮误字段/缺openExternal失败另留档。新消费者明确Node生产路由+实际WS/handler/buffer/store及mock历史HTTP，是补充机制检查，不冒充Rust执行/存储全链或reset/hydrate新增实测；Rust全链由前述17项另证。所有源码停止编辑，下一步冻结e完整20门，不拼d旧绿。


t08e实际冻结651文件（新增四个此前未修改路径进入overlay清单），fp f8b3924db2d194834f38efcdbbf231762a9f057d0b8c2f70361b37dd6ddb5a1e。freeze退出0后开始正式20门；同一主控顺序Cargo、独立types，后续build必须实际成功且回执出现后才启动4个postbuild检查。未将开发17/23或d18绿复制成e通过。


## t08e服务正式失败留档（2026-10-18 09:13 UTC）

冻结指纹 `f8b3924db2d194834f38efcdbbf231762a9f057d0b8c2f70361b37dd6ddb5a1e`：服务命令自然结束101，原sourceUnchanged/verificationToolsUnchanged均true。完整原日志34目标，876通过/3失败，0忽略/0过滤；32目标全过、2目标失败。服务lib648/648；r07_t08前缀16个直接集成目标77/77；其余17个适用局部回归151通过/3失败。原完整命令、receipt/log SHA及逐目标数据已追加T08_FAILURE_HISTORY，不按套件结果复制原叶通过。

- r06_agent_delete_skills_live实际0/2：complete和guard-original两个子场景均在初始ready=false断言失败，分别由Rust第260/511行真实回执失败。不先认定只是超时、环境或产品根因；工作流正在独立只读查证，本文记录时尚无后继诊断文件可引用。
- r06_chat_wire实际16/17：native_current_chat_page_inputarea_tiptap_true_network_and_queue_interject在另一chat-live夹具的runtime-capsule-mounted断言实际false、预期true。d轮的resume末事件及AppPages两处原失败用例本轮均真实通过，不将它们混记为继续失败。新失败原因仍待独立证据，不改断言领通过。

本轮原LAN管理1/1（目标88.95秒）、服务lib648/648及原生设置14/14通过；不覆盖此前LAN失败与固定副本真实复现，也不宣称LAN具体根因已修好。所有旧失败继续保留。其余独立后继门禁由根端继续执行，没有完整有效回执的后继保持NOT_RUN；本次不提前结算全部20门禁，更不称整轮PASS。T08仍IMPLEMENTING；T09只有只读接线准备、尚未实施。

本次只追加失败历史与本状态文档；未改产品/Source、测试、冻结计划/工具或原账，未运行Cargo。


## t08e二十门闭合、实际证据对账及f启动（2026-10-18）

RESULTS_t08e原始闭合为20门19通过/1失败、NOT_RUN为空，关闭前源码与工具仍同冻结指纹。服务原101（876通过/3失败）保持；UI66文件552/552、原生技能窗口、精确picker产物绑定、构建与四项postbuild均通过。前文“后继仍未完成”是当时进度，由本条追加最终事实，不改旧回执。

工作流后继只读报告确认：两个助手删除父case分别在首complete/guard-original约1秒ready=false停止，删除动作、连接切换及后续登记表变体均未执行；缺实际初始化请求流水，不把它定性成纯夹具超时。chat-live已观察真实终态和welcome隐藏，lazy胶囊即时检查失败，原现场没有其后最终挂载事实。失败历史已引用t08e-three-failure-diagnosis1、t08e-three-failure-preserved1/manifest与t08e-initialization-barrier-readonly1原哈希，不重复改场景。LAN本轮1/1通过不解释此前失败。

新对账artifacts/rust-tauri/R07/T08/t08e-leaf-reconciliation-actual1.json逐字保留109叶343条原文/编号，核全部20份正式receipt/log哈希与冻结指纹。143个原具名映射中96项有原日志精确成功结果、1项是同产物原生证据绑定、46项仅UI选中+聚合成功；原目录未列的三服务失败另列阻断。44份相关来源分别核冻结归档/原base/冻结工具hash，不借f修后源码回写e事实。

343条中222条已有登记检查点的具名本机证据、85条仍为部分证据、36条尚无直接具名检查；这些是证据成熟度，原账PASS仍为零。36条集中于供应商第3条“只宣称已登记能力集合”的精确对比，不等于代码缺功能，也不能拿35品牌聊天调用代证媒体声明；85条保留逐名UI回执、复合错误组合、品牌额外媒体及真实账号/平台等边界。e UI552总计没有逐title原始报告，不能平均拆成343条通过。f须用自身冻结材料及原生JSON逐名重核。

initialization-capsule-fixed1在修后开发源码另号实跑原AgentDelete2/2、ChatWire17/17，exit0/sourceUnchanged=true，原e失败不覆盖。随后已正式冻结t08f657文件，指纹71924b54ab9c82a70b2327010d4287c3c04e73221b65b7699e744b20adbce836，完整20门开始执行；未完成项仍NOT_RUN，不借开发19或e19通过领f结果，四postbuild继续须等f真实构建退出0及回执绑定。T08仍IMPLEMENTING，T09只有只读接线准备、未实施。

本次仅文档/证据，没有修改Source、工具、计划或原验收账，没有运行Cargo、测试、服务。


### e对账后发现的容器入口及漏映射更正（只读）

现役Rust成功初始化仍设rustSettingsUnavailable=true并在ready后return；SettingsContent除了agent/access外把所有导航页替换为不可用提示。独立窗口SettingsApp与主窗口/mobile的SettingsModalShell都使用这同一容器，未找到providers/skills/general旁路；当前Tauri资源壳也未提供另一套完整设置入口。这是T08真实可达性缺口，直接render子组件/实际HTTP不能代证用户从容器导航可操作，f即使20绿也不能代领。仅记录静态事实，没有改冻结源码或启动服务。后续须实际修共享入口并由容器真实初始化→导航点击→同服务请求/保存回读证明，不手动关标记绕过。

另有草稿过宽/陈旧项：配置归档fresh二进制实际导出按钮/文件输入导入已有r07-settings-real-service具名case，e正式命令选择并绑定构建；只是缺逐名JSON，不需要新增相同正例。99F8CD初始化缓存/summary/15秒有原生具名实际PASS；技能现役检查有403开关回滚、取消/无agent/组合CRUD/export。真正剩余窄缺口是两技能页GET失败保旧列表及各自Promise.all移动一写成功一写403的部分成功事实；Providers真实容器保存后配置重读失败组合仍须入口修后检查。useMediaSettingsData有单媒体失败及保旧数据现役检查，但e未选该hooks文件，不能用552总数代证。新e实际对账已逐项附准确入口及最小检查需求，不改原断言或以路径存在算通过。


## t08f检查闭合与实际入口缺口（2026-10-18）

完整20门实际全部退出0，源657行指纹71924b54...及检查工具在关闭前重算匹配。服务34目标879/879；UI原生JSON67文件555项具名通过、无skip/pending/todo。四项构建后检查绑定实际build退出0的同一d91fbf82...二进制；独立报告核实。RESULTS_t08f明确CHECKS_PASSED_NOT_ACCEPTED，不宣称T08或R07完成。

查漏确认SettingsApp与SettingsModalShell共用容器在正常Rust初始化及重启后仍设置旧不可用标记，除agent/access外页面被挡；孤立组件真实HTTP并不能证明用户导航可达。原35继承叶MCP设置状态/控件及Security备份列表仍需逐条接线。三复合叶已有大量具体失败检查并非全部未测，但技能GET失败/多项写部分成功、供应商真实写后读取失败尚有窄缺口。旧草稿误称缺配置导入组件case已纠正，该case在f JSON真实passed；仍不外推系统文件选择器。

新增真实入口检查前四轮未到入口判据：首轮错误期待助手创建201而现役返回200；随后三轮依次缺模型平面、chat默认路由及目录模型tools声明。原退出1回执/日志全留，按当前真实配置校准后，settings-entry-reproduce5确实完成真实identity/agents/snapshot/models初始化，导航三页全部被挡，退出1且源码未变。它才是入口缺陷运行复现。入口修复settings-entry-fixed1实际退出0、运行源码未变；目前只证明导航解除拦截，异步页面请求/真实保存/重启等加强验证仍继续，不填最终通过。

技能新6例的skills-failure-real1编译成功但真实检查退出101，原场景保留：管理/助手两树定位、跨case共享技能与通知spy递归问题正在依据现场修正，不把未处理错误当通过；并行修改导致全局sourceUnchanged=false，未保存逐文件before清单，不倒填版本证据。后继正式候选必须重新全量冻结。


## f后界面偏好/全局存档开发验证（2026-10-18）

settings-backend-integration1实际退出0：新interface_preferences四个具名检查4/4，原user_preferences2/2、conversation_map_and_checkpoint3/3、MCP19/19、39身份能力精确表1/1通过。日志SHA a9883cc2fd37bbb4a52edf460e149ec5e72f55fc6cec9e3ebf144f57377300d0；本轮全局源码指纹不同，是并行开发结果，不作为冻结接受。新11文件ready→独立运行前审阅→运行后当前SHA全部相同，绑定见interface-preferences-actual1/receipt.json。

新4例实际证明快捷键全量覆盖/恢复默认、侧栏嵌套合并、原权限、坏请求/坏文件、设备撤销、同一原子writer提交前撤权零写及清理、同home重启；全局改前存档取既有目录，真实user-edit保存后只返回8项元数据、不返正文，坏JSON/链接明确失败。Node共享纯规则对全部已登记命令及侧栏partial与实际HTTP相符。提交前撤权检查并非HTTP锁排队实证；Windows/系统级快捷键实际注册未验。原保存/恢复不借新增列表扩大通过。

消费者路径与返回形状静态吻合，仍缺本批从导航点击的新接口实际保存/回读。另发现并已获授权补查三个旧失败分支：Security读失败伪装空并清旧表；Interface首次读取失败无可见重试；Keybindings已PUT后重载失败回滚本地且reset不检查ok:false。将先留独立失败反例再最小修复，不借后端4例通过盖过它们。技能修后skills-failure-real2真实子UI6/6和Rust父目标1/1通过，9个owned文件前后匹配；原real1失败/安全场景继续保留。T08仍IMPLEMENTING，f维持CHECKS_PASSED_NOT_ACCEPTED，未改原账、f材料或审计。


### 设置入口与补充接线实测（2026-10-18 10:20 UTC）

仍为IMPLEMENTING。settings-backend-integration1实际29/29通过：旧偏好/改前存档5，新界面偏好4，MCP19，原39供应商声明完整矩阵1；正式生成与契约检查退出0。新增四处能力声明只证明原能力展示，不声称新增真实远端生成。minimax-token-plan原叶文本与既有manifest/image lane矛盾仍明记，不改原台账。

settings-entry-compound2实际7/7通过：从SettingsContent导航实际导出隐藏合成秘密、文件导入并回读偏好且保留未知字段；无模型/无助手仍可修配置；同home实际重启恢复与拒旧重启凭据；单个资源请求坏凭据返回真实401后结束加载而仍阻拦页面；供应商实际两次保存，第二次真实保存200后隔离助手资料损坏使GET500，唯一Source保留新endpoint并停止后续summary/media刷新。它使用既有d91后台产物，且并行编辑使全局sourceUnchanged=false，不能冒充统一冻结候选，也不将单次坏token外推为设备撤权。transfer1新home偏好不存在、compound1误读Source路径两夹具失败完整保留。

独审指出MCP队列三边界，reproduce1先因新测试编译错误未执行；reproduce2实际22项19通过3失败，确认坏update先取消有效启动、显式start撤权仍applied、disable清理时接受新启动。正在修复，不以原19通过代这三项通过。另Keybindings/Interface/Security真实消费错误分支继续修，T09—T12尚未实施。


### MCP后台启动边界修复实测（2026-10-18）

原mcp-queue-review-reproduce1仅编译失败、未执行反例；reproduce2实际22项19通过3失败，已确证坏update先取消合法启动、显式start撤权仍applied、disable清理间新启动越过三问题。原回执、失败日志和隔离现场保留。最小修复为取消前同锁完整预检、disable的可回收启动暂停栅栏、显式连接原授权及提交前复验，继续保持add/bulk立即保存回执与手动start/refresh同步返回的原含义。

mcp-queue-review-fixed1实际22/22退出0，含原十九场景与三反例，日志SHA8166d1956d850ae516881776723c2927117db30d2fab99aff520da96945dd860。新30项Node消费/组件机制及三套类型检查实际退出0；空助手可读真实全局设置而不扩大凭据投影。全局sourceUnchanged=false，属并行开发证据，不代表后继固定候选接受。具名结果、回执SHA和边界见artifacts/rust-tauri/R07/T08/mcp-current-report-20261018-queue-fix1.json。原35 MCP OAuth/apps/resources/session责任仍在独立逐项核对，尚未接通不得领完成；本轮Node并非原生导航或Rust执行证据。


### 真实入口回归及剩余原35责任（2026-10-18 10:32 UTC）

compound3七例0且全局sourceUnchanged=true，补充旧服务真实401延迟交消费者，实际重启新token/identity恢复后再释放旧响应，新identity和General页面仍有效。旧Electron设置14/14实际0。delete-regression1中last先真实另请求删除survivor，新增尾段错误要求其skills读200而失败；修为先核真实404及可见失败，再用户选择仍保留victim要求200后，delete-regression2两目标场景全部0且sourceUnchanged=true，原删除/撤权断言保留。

build1真实构建0及当时d7产物绑定有效，但后续Cargo test可自动重建同target二进制；原“不覆盖production binary”判断错误，实际UI启动前SHA守卫拒绝，未启动功能检查，不将拒绝算功能失败或通过。build2显式构建0并复制到唯一只读自有路径，SHA d7b56a69dfb584747618d8f0eb36d6ca92484b496158328c2fbc426ba4db2ece/255584512字节；后续UI使用固定副本，绑定前后SHA，不再假设共享Cargo输出路径不变。

35原叶逐ID只读核对明确7个当前缺口：apps列表/launch/resources，OAuth start含poll/cancel/logout，本会话余期exact capability临时授权。均有本地自有外端可实现正例，已继续T08实施，不能仅明示Unsupported后推T11。详见inherited-mcp-remaining-positive-readonly1.json，原14断言/UI交叠原文和R07账状态不改。


### 三页失败分支修复及固定副本真实导航（2026-10-18 10:33 UTC）

新增六条反例先在原实现实际25通过/6失败（interface-consumer-reproduce1）；最小修复后原25项与新6项共31/31通过（fixed2）。Security初次失败可重试、不伪装无备份，后续读取失败保留同连接旧表；Interface首次失败显示错误并在读成功前禁用控件、提供重试；Keybindings实际PUT后系统重载失败保留真实保存值并报错，reset同样检查ok:false。五语言复用原重试词并补对应错误文案。中间fixed1的30/31及原类型错误均保留，最后全局tsc实际退出0（interface-consumer-types3），不覆盖旧失败。

interface-consumer-real1仅启动前SHA守卫拒绝共享target产物变化，Node与服务均未启动、功能NOT_RUN；后续real2使用build2固定副本实际退出0，1个具名组合case含5条实际链检查：真实SettingsContent初始化与导航、快捷键保存/恢复默认/实际磁盘及HTTP回读、侧栏部分保存保留其他偏好、既有全局改前存档只展示元数据、真实损坏存档500保旧列表及修复重试、同home实际服务SIGTERM退出0后重启重新初始化并恢复以上数据。未mock服务结果或直接注入ready，只有桌面快捷键重载外部桥返回ok:false；系统级快捷键注册、原生窗口、Windows未验。

实际二进制SHA d7b56a69dfb584747618d8f0eb36d6ca92484b496158328c2fbc426ba4db2ece前后相同；12个owned文件运行前与停改清单匹配且前后不变，另4个容器/API关联文件前后不变。receipt见artifacts/rust-tauri/R07/T08/interface-consumer-real2/receipt.json，stdout SHA f0753325cc072104953391e27bc164205591a4993647e1d0d8a27d519a9e837c，实际HTTP步骤回执SHA 640ac7f44975c9e875e3a795d62efe579a2455aaf3155219e2fee1caea4ae59a。仅开发验证，不改f CHECKS_PASSED_NOT_ACCEPTED、原109叶343断言或R00审计账。T08仍IMPLEMENTING；本分工源码及测试保持停改。


### 设置页覆盖与新增跨连接复核（2026-10-18 10:42 UTC）

fresh-binary3四文件34/34实际0、全局sourceUnchanged=true，其中8个真实服务容器组合场景；新增all-pages一例实际导航17页并核全部读取无旧/api/及404/405，不将17页虚增成17个case，也不领各页所有保存。all-pages1的试件未记录具体tab，原失败具体页未知；补异步就绪等待与Fragment实际页体识别后实跑通过，产品未改。

独审核三页31组件及固定副本真实导航1case5步骤均成立，但发现新接Keybindings保存/reset及Interface侧栏保存缺当前连接owner复验，A保存真成功后延迟回复可能写B快照/触发当前OS重载或事件。这是新增静态风险，正在以两真实服务反例复现，尚不说已实际失败或修好。归档transfer也补核连接往返期间的上下文隔离。T08仍IMPLEMENTING。


### 三页迟到写回复及导入连接隔离（2026-10-18 10:48 UTC）

只读独审后新增真实A/B四反例，在原实现实际0通过/4失败：快捷键保存/重置的A PUT已经200落盘，切到真实B容器后再交A回复，均改写B快照、提示、宿主通知及OS重载；侧栏迟到保存回复误发宿主/窗口事件；导入慢File.text跨A→B→A后仍实际POST。reproduce1原现场/日志/源码哈希保留；reproduce2仅改观察交付的方式，先确认原Response正文到齐再释放，允许修后消费者直接忽略过期回复，原产品仍4/4失败。两个复现共8个隔离home保留，摘要只取合成非秘密偏好，不复制token。

最小修复两页及SettingsTransferSection：捕获同一连接revision并在各网络、系统重载或文件等待返回后复核；换源释放旧等待，旧catch/finally不写新连接；导入导出另用本次operation标识，切源回收临时下载URL并恢复新控件。A已经保存的偏好不伪回滚。fixed2实际原31组件+原真实导航1+新四反例=36/36、无skip，B真实快照/提示/宿主和窗口事件/OS重载未变，A/B磁盘各保持真实事实，B控件在释放A前已可用。固定build2 d7副本与12项关联源码前后相同。原正常configured导入导出另用准确名字选择实际1通过、其余7项明确未选；第一次ASCII括号过滤导致8skip原回执保留，不算通过。

证据汇总interface-late-write-final1/summary.json；fixed2消费者stdout SHA bdeb496ec652aec1cdf074a29c0a55470ff9fd0decd65f1487a69a4add6d7b64，正常导入导出stdout SHA 23607dcfd381fd4edac9de1dc4495e2d6bee7be8c43eab47d123a655798d5a05。全局tsc本轮实际2，只有并行MCP新DTO尚未官方生成的两个missing-export，已交workflow/root，不能用前轮types0代领本轮类型通过。没有Cargo/生成修改，14个owned文件已停改；仍为开发证据，T08 IMPLEMENTING、f CHECKS_PASSED_NOT_ACCEPTED不变。


### 剩余MCP责任首编与真实会话反例（2026-10-18 11:12 UTC）

官方契约生成269文件、接口矩阵634项检查实际退出0；mcp-expanded-sdk-check2真实lib编译退出0。SDK新远端应用/OAuth四个场景已落测试，尚未执行，不以编译或20项页面机制检查代称实际SDK成功。解析后资源/目录长度限制不能声称阻止官方SDK JSON路径解析前巨型响应分配，这一边界明确未验证。

会话临时权限reproduce1四项全在准备阶段429失败；补测试显式启动及500ms有界读取，未关闭限流或加长原预算。reproduce2实际1通过3失败，日志SHA a7ad028d399d604039e7eac8c567b44ab0bb8b81b187277bda5605a3527bf3c6：真实WebSocket聊天第二次不同参数仍待人工审批，确认前台入口接线遗漏。另两项真实现场分别停beta审批及fresh凭证首轮审批，尚未到后台/服务重启，待阶段诊断；原场景只读数据库一致快照与调用证据保全。原scope、错误会话、坏输入和顺序归档恢复/连接代次一项通过，不外推并发交错。

已最小修复真实用户WebSocket专用前台入口，保留后台来源原限制，待运行。另增真实读取授权后归档再恢复的交错反例，业务问题尚未修复，准备先保存实际失败。T08仍IMPLEMENTING，f维持CHECKS_PASSED_NOT_ACCEPTED，T09—T12未实施，无提交或推送。


### 会话授权第二轮实测与消费者范围（2026-10-18 11:19 UTC）

mcp-session-permission-reproduce3真实5项2通过3失败、sourceUnchanged=true：WebSocket专用前台接线修后第二次不同参数通过；读active后真实归档恢复完成再释放旧grant实际200，确定交错缺口；beta已真实waiting_approval但5秒内昂贵设备认证轮询未读到，尚未判权限继承；credential通过fresh批准后服务退出真实EPERM/actual_pgid=-1、mcp_cleanup_unconfirmed，未进入同home重启结算。新源码修复与实际就绪观测调整继续，不回写原失败。mcp-expanded-types-final1三套检查实际0、sourceUnchanged=true。

独立只读核SettingsContent→McpTab→ConnectorForm/Detail/OAuthWaitCard现役Rust分支均指向新正式登录/poll/cancel/logout，仍需真实按钮到SDK验证；原20页面检查只属机制。apps列表/launch/resources三原API叶没有现役React app viewer，新本地正例应准确表述为真实HTTP→官方SDK目录/描述/内容，不宣称设置按钮已渲染远端应用。launch只创建现役描述及输入绑定，额外callToolUrl执行叶不在原35范围，不扩造执行入口。

另两OAuth新静态风险已确认待真实反例：旧连接refresh可能迟到覆盖新登录凭据；取消的登录仍占32等待容量。先保存实际失败再最小修复，不按静态查到直接登记运行失败。


### MCP远端真反例、浏览器回执及下一轮准备（2026-10-18 11:35 UTC）

remote-sdk-reproduce2真实6项2通过4失败：完整顺序PKCE/DCR、私密Source、官方refresh、同home重启及logout通过；cancel/config/owner/shutdown防旧凭据复活通过。两个独审静态问题升级真实失败：旧SDK refresh覆盖新登录Source、32个取消后第33个start仍429。资源读取取消3秒未见自有server handler取消，尚待精确外端断连观测；404场景因test存储permission mode错误full，未进入实际调用。原场景/日志SHA a6028002...及合成安全摘要保留，修复未回写。

permissions-fixed1真实5项3通过2失败：epoch交错409、paired同能力不同参数及跨session/tool/后台边界、scope坏输入/顺序归档恢复/连接代次通过；credential已走实际同home重启及新审批但最终close真实EPERM。WS外层观察6秒超时，独立只读现场证明第二Run2712ms完成、两次真实调用无再次批准；原select反复同步认证挤占帧观察，现只调整测试准备，保原时限与真实HTTP核对。

浏览器open-external原6真实处理函数负例与3McpTab负例在reproduce2实际12过9失败、源未变。最小修复等待shell Promise并给明确ok/error回执，原不等待的调用方仍不受拒绝异常影响；MCP缺能力/打开拒绝后实际取消等待，失败保留原原因及取消未确认，换连接后旧回复不动新等待。模型OAuth同样读取明确失败；mobile保持原void返回，noopener的窗口值不冒充系统回执。fixed1唯一vm跨realm Error字符串差异失败保留，fixed2实际38/38；fixed3新增迟到打开拒绝隔离/取消失败/mobile兼容实际44/44、sourceUnchanged=true，日志SHA29a661be...。它们是实际处理函数与组件机制，不是假称OS原生浏览器启动。

新的实际SettingsContent初始化→MCP导航→登录→真实自有授权callback/poll→SDK连接→退出→再取消组合子检查已加入remote父目标，不手动置ready、不替换服务响应；原生系统浏览器仅由真实GET自有外端替代。新三套types实际0、sourceUnchanged=true；正式API矩阵更新634项实际0。root当前统一运行remote7+permissions5，尚未闭合，mac原生退出单测随后执行。T08仍IMPLEMENTING，g尚未冻结，原f/失败/原账不变。


### 会话与完整MCP设置页面实测、资源回收仍阻断（2026-10-18 11:50 UTC）

remote-and-permission-fixed1同一源指纹实际12项10过2失败。会话权限5/5完整通过，含WS第二不同参数不再审批、精确能力/session/主体/scope隔离、原后台不继承、归档恢复交错拒绝、实际同home重启及进程退出。远端新两OAuth真反例修后通过，真实404 POST只投一次。资源/新页面fixture两失败原样保留。

remote-sdk-fixed2同一源码7项6过1失败，日志SHA4056343e...。真实设置子1/1、父1case通过：不代填ready，实际身份初始化/导航/MCP登录、真实授权callback与poll、启动官方SDK连接、logout清Source、再start后用户cancel均得到生产HTTP回执；外端exchange1/refresh至少1/tools-call0、原unknown字段保留，父/子实际退出0。未把系统浏览器替代GET说成原生打开成功。

资源新远端观测仍显示stop后3秒active1/drop0/ctfalse、wireCancel0、DELETE1，确定不能以本地停止等待算远端已回收。官方SDK现代HTTP可能以取消原POSTstream而非wire通知实现，wire0不单独定错；当前正在先按connection绑定原requestId完成官方取消，再关闭SDK连接的最小修复。原等待预算和真实远端清理要求均不放宽。

Mac清理lib首轮因旧unit漏authority参数编译101、两检查NOT_RUN已留；配套修后macos-cleanup-fixed2两项实际通过、647明确未选、sourceUnchanged=true，含64次真实自然退出回收及未回收退出记录/活组区分，日志SHA70ac2f91...。这是本机macOS正例，不外推Windows/Linux。T08仍IMPLEMENTING，后继g未冻结，T09—T12未实施，未提交推送。


### 停止回收修复及g冻结前预检（2026-10-18 12:13 UTC）

resource-stop-fixed1真实资源原3秒清理要求通过，实际协议2025-11-25观察wire取消1、handler通知1、cttrue/drop1/active0、DELETE1；不外推未测现代分支。request-stop-regression1旧满队列close失败原样保留，修复历史unknown只保告警、不再污染当前active清理后，regression2原8/8全部通过，原未知状态仍未知、child真实回收/no-replay断言保留。独审未见吞当前清理错误。

完整MCP三个目标实际33过1失败：remote7/7含真实页面子、session5/5，settings21/22唯一旧unsupported预期。已支持HTTP而外端仅模型API，精确预期改为真实connect_failed；原九动作/保存/失败/unknown断言保留，后续同名1/1、21未选，不冒充整22重跑。

格式预检首轮失败保留，10文件固定formatter仅归整后第二轮0。clippy1的Response大错误和clippy2测试is_some后unwrap均保原失败；最小Box搬运与if let修正不改权限/存储/等待预算，当前clippy3运行。g尚未冻结，原f仍CHECKS_PASSED_NOT_ACCEPTED；T08仍IMPLEMENTING。


### 固定g正式复查启动（2026-10-18 12:14 UTC）

clippy-preflight3实际workspace/all-targets -D warnings退出0且源未变，log SHA a53007f066a4fd005d2f873edc106b632775292dd9e503f3722947a240568d96。g已冻结754源文件，fingerprint 6d00d82b598ad23f9cbbc05f61e9514cf8e7f771a8434f9f5884672b3782cf2f，20门计划 SHA abf6f8a6f1c2c1180734f0c401139d6bb94417a5e1309cc8066465b37672fd87。源码/检查工具保持停改，正式门开始，不借开发绿或父子skip计算通过。四构建后检查须先同候选真实build及SHA绑定；原f与失败不覆盖。T08仍未接受。


### g服务正式失败及独立余项继续（2026-10-18 12:26 UTC）

g service真实40目标907过4失败、0忽略，exit101，源/工具/计划未变。失败三目标分别为r00管理LAN自地址POST登录20秒0bytes（旧失败复现）、设置原生workflow与boundaries拖动（两场order请求全0，workflow mousedown前hover=false、分数命中取整未复验，不能说保存失败或定焦点冲突）、界面偏好坏输入（请求/atomic撤权已到末尾，退出因Bridge首次list prefs与注入{broken竞态而Err）。两个原生UI JSON已复制并SHA绑定于t08g-native-ui-failure-snapshots1，独审只读诊断单列。

首批runner按设计在service101停止，原t08g-rust-batch.notRun保留；同一停改g继续首次执行可独立model/native/skills/build门并单列remainder批次，未重新执行或覆盖service结果。g不能称通过；T08仍IMPLEMENTING、T09未实施。下一版修正须另冻结，不借当前g或先前开发绿。


### g完整封存及后继最小修正（2026-10-18 12:34 UTC）

RESULTS_t08g.json SHA7f3c7ee52f356a1b3a87314b4370366e254100ae4d94c823920287c13146974e：20门18过2失败，40服务907过4失败、89 UI文件717过1失败、无skip；close先重算754源码/HEAD/runner+工具/计划/archive/20日志和4构建后SHA绑定全部一致，再开始修后继。原首批notRun保持当时事实，remainder/postbuild首次执行另有批次，最终notRun空不改写首批。

新增UI唯一失败是旧AccessTab桌面scope精确预期漏sessions.write；生产已有正式会话临时授权所需scope，手机默认scope保持不扩。后继仅同步桌面预期。坏prefs单项准备将先真实Bridge stop+原5秒预算wait_stopped，完成有效prefs下初次import/recover后再注入损坏，保真实HTTP/原子撤权/最终close全部断言；范围明确不领损坏期间Bridge常驻。拖动仅修实际焦点/整数命中/hover前提，保原可信输入和真实排序/失败回读要求。

单次LAN layer-g1使用同g正式构建0aa124e8...副本、空home/noManualClock/无凭据health：loop前200 .026s、LAN200 4.107s、loop后200 .001s，实际退出0。16次双端FD/TCP状态/监听队列已保存，显示请求等待期间连接尚在队列；此健康读取不是原登录正例，不归因过滤器/防火墙。T08仍IMPLEMENTING，原失败全部保留。


### 后继h冻结启动（2026-10-18 12:50 UTC）

后继相对g只改5个测试文件，产品实现不改：原生拖动前提、偏好坏输入准备、Access旧scope、provider逐ID负例、mixed models/search真回读。原14+4同批18/18，Access10/10，原r00完整目标1/1(含真实LAN login/session/logout)后续通过；g2层诊断200/403与原超时均保留，原因尚未证明。两窄补验实际36+5共41/41、source未变，log6e282415...；其中mixed首轮3过2准备失败(观测key错用auxiliary.title)已入原失败账。格式与workspace clippy实际0。

h冻结755源，fp f0dd4aa5092b31eb121c3977da8e65cbe2035ab3f4581b6c5898cf73f8da354e，20门计划SHA8d8cc95b5a16fd9c26c60e76ccf5391ac32e0c4ec3de10590cc249d9ad6ac7b8，保全部旧失败目标，不以局部绿改写g失败或代h接受。独审两报告记录三夹具与35+2检查无削弱。82旧PARTIAL不是82实现缺口：新增skills/aux/media/OAuth/provider真实case已覆盖部分旧描述，最终按原条款具体区分；未测分支/旧minimax声明矛盾/账号平台边界继续保留，不领整个R07。T08仍IMPLEMENTING，T09未实施。


### T08本地份额独审接受并进入T09（2026-10-18T13:08:17.794275+00:00）

h完整20门退出0，服务40组948/948、页面89文件718/718；adapter223过/13既有原生Chromium项本h未执行，独立单列。最终独审SHA1bf0970a...及202引用逐叶结算cbd9c99b...核证，无本地实现/适用门禁阻断。ACCEPTANCE_t08h SHA256c95bdb3951dbddf919a3455ce1915e1ca025da79a2a1cc923d8fb3be9a0e8仅接受本地份额，不把原343全写PASS或领整个R07。17具体UI分支、4窄负例、minimax原矛盾、adapter13及T07失败交最终清账。原g/e/f及全部开发失败不动，LAN旧超时原因不虚报根治。正式进度转T09 IMPLEMENTING；源码仍沿当前分支，未提交推送。
