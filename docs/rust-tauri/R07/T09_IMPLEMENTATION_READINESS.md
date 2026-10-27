# T09 实施前事实与最小缺口

状态：只读准备；未实施T09、未运行Cargo/服务/真实账号，未修改原账或阶段状态。取证基于当前工作区，HEAD仍为 `0c86675d49442fe0e38259bd3097423fb340eecc`；T08同时施工，此文不是固定候选验收或通过记录。

## 权威范围

- 原任务书 `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R07_外围能力、后台自动化与多入口接入.md:378–415`，R07-T09依赖T08。要求CLI现役命令迁移、serve启动Rust；复用Web/mobile资源和账号/设备权限；统一任务与事件、路径不混用；退出/刷新/后台/重连不重复执行。
- 基础验收为 **R07-A17**（CLI发起任务生成文件，授权手机看到同一run/历史/文件身份）及 **R07-A18**（设备B缺scope，即便知道resourceId仍无法下载）。均允许隔离本机和外部确定性替身，不要求真实手机账号或付费供应商。
- 原 `R00/ACCEPTANCE_MAP.json` 与R07叶地图给T09 **39个细分叶**。其中34叶共享R02/R07责任，5个CLI维护/发布叶另有R08/R10/R11责任；本阶段不能代领后续恢复演练/正式发布份额，旧R02通过也不自动等于T09通过。R06已由用户接受，不重新要求整轮R06。

## 已核事实

| 面 | 当前实际状态与定位 | 对T09的影响 |
|---|---|---|
| 正式CLI入口 | `package.json` bin=`cli/entry.ts`；npm cli经`scripts/launch.js`转该入口。`cli/args.ts:18`默认runtime=node，帮助也注明node默认。 | 当前默认聊天/serve仍是旧Node，不能称正式CLI已迁Rust。保留旧显式兼容入口与后续R11退出治理分开处理，不在T09调查中删除旧路径。 |
| Rust CLI连接/启动 | `cli/rust-service.ts`已有私有instance/local-token校验、显式origin连接、GET health/me/sessions/session；`cli/server-runner.ts:128`起有Rust进程启动、同home互斥、channel构建指针、降级拒绝、信号/退出码传递。 | 可以复用，无需重写账号、子进程宿主或再建Rust CLI工程；候选二进制必须用显式LINGXI_SERVICE_BIN，默认指向rust/target/debug不会自动跟随主控隔离target。 |
| Rust CLI聊天与状态 | `cli/entry.ts:86–143`中status始终提示Agent/model不可用并返回1；chat/continue最多查会话后直接throw unavailable。`RustCliClient`只有GET，没有创建/提交/WS/历史/审批/取消。 | 硬缺口。`cli/chat.ts`与`cli/client.ts`目前仍是旧/api+旧WS消息，不能只改URL便宣称接通。 |
| mobile登录 | `mobile-init.ts:61–86`可识别Rust健康和Web cookie；MobileApp登录/登出可选择Rust正式web-auth。 | 登录面已有基础，不能用cookie成功替代聊天接通。 |
| mobile启动与会话 | `mobile-init.ts:112–124`在Rust身份通过后**直接throw Rust mobile workspace unavailable**；其后bootstrap仍请求/api/mobile/bootstrap，Rust未注册该路由；loadMobileSessions读旧/api/sessions数组且强制path；createMobileSession也经未迁移的/api/sessions/new。 | 硬缺口。须先实现脱敏、按principal投影的真实bootstrap，再复用已接Rust的会话适配和native:sessionId定位；不能仅删除保护性throw。 |
| 共享聊天基础 | `websocket.ts`已有Rust正式/ws-ticket、/lingxi/v1/ws及wire hello校验；session-actions/rust-chat-history已读同一正式history并用native:sessionId；chat_wire服务已有prompt/abort/resume及clientMessageId入站去重。 | 可复用同一真实RunSupervisor/Storage和现有流适配；CLI与mobile不应创建第二套任务、历史存储或模拟回复。源码可复用不等于两个入口已联通。 |
| Web/mobile静态资源 | `static_web.rs`托管/mobile/、/desktop/及资源；默认desktop/dist-renderer或明确LINGXI_RENDERER_DIST，目录损坏503，未安装指引。**/desktop也用mobile.html是旧服务既有行为**（server/routes/mobile-static.ts:57–58），不据此擅自改成Electron专用index.html。 | 必须用实际构建产物加载页面和子资源；现有r00_static_web_leaves仅合成HTML/JS证明托管，不能证明实际mobile聊天。管理面发放的URL带尾斜线；还须核直接/mobile的相对资源路径。 |
| 文件与远端路径 | SessionFileService/ResourceService已提供sf_/resourceId、同SQLite及正式content URL；resource-url.ts对Rust明确禁止本地file路径回退。mobile-platform只接受http(s)/data/blob和旧/api相对地址。 | A17须从CLI真实工具写文件并登记附件/历史引用，手机沿正式resource链接取同字节；不得返回宿主绝对路径给远端当下载地址。需核所有mobile openFile调用是否已传绝对正式URL，不能凭路径函数存在领通过。 |
| 资源权限 | auth.rs:1233起资源信封/GET/HEAD content要求resources.read，ticket签发要求resources.write；ticket仍不豁免HTTP身份；session内容读面有真实历史引用限制。 | A18要用实际缺resources.read的另一设备访问已知resourceId/GET/HEAD/带票据均失败且无泄漏。不能把files.read误当资源scope，也不能只用无凭证401代替设备B越权负例。 |
| LAN/账号设备 | management.rs已有access、密码/配对/撤销、web-auth和server identity；main支持明确--network-mode lan及TLS，tls_web_login已测真实TLS密码登录。 | 复用既有AuthService和短期WS票据。LAN不能复用loopback owner token；cookie会话/设备撤销必须在后续HTTP/WS/下载生效。不要为本阶段搭公网或开真实外发。 |
| dev:web | `scripts/dev-web.js:64–78`仍起Node server/bootstrap.ts，再读旧server-info.json；npm server也仍走Node。 | 不把它当T09 Rust页面证据。Rust页面验证用明确候选服务+实际renderer dist；开发入口最终切换与T11分工协调，不能误领R11旧链退出。 |

## 最小实施接线（当前授权已覆盖，T08完成后按序实施）

1. 以现有RustCliClient/runner增加真实请求方法和会话/WS协议适配；复用服务现有session/new、history、chat_wire、审批和cancel。CLI help/status/sessions/continue/chat/serve完整工作；原维护命令仍走现有本地维护实现，隔离home离线验其本阶段份额。正式Rust serve必须传递退出码，不在错误时偷偷启动Node。保留同home互斥、channel和数据降级约束。
2. 为mobile提供最小真实bootstrap：按授权主体读取当前/指定助手、agents、locale、userName、yuan、memory开关、workspace/cwdHistory、thinking、appearance/editor/chat、头像可用性；复用T08安全投影和MemoryService，不能透传preferences/providers/密钥。原过期工作区清理沿现役实际清理器，不能虚报gc已做。
3. mobile初始化建立Rust ServerConnection后走现有Rust session/history适配，不要求服务伪造旧JSONL路径；把load/create/switch/工作台/API边界明确适配。每次提交固定agent/session/owner，沿同一模型/工具执行链，不回退旧Node。
4. 把刷新、网络断开、后台恢复都限定为重读快照/事件补齐，而不是重发prompt。保持clientMessageId/snapshotVersion/attempt/stream归属和resume cursor；CLI退出是脱离查看还是请求取消须按现役交互规则实现并实际证明。审批须在前台等待请求期间并发处理真实审批面，不能先等HTTP结束再答审批（T08已暴露此夹具死锁）。
5. 在现有保护下做两端实际集成，不为A17新增假成功或直接伪造DB记录。下载拒绝维持原scope，不用扩权限换页面可用。旧未迁移入口的显式拒绝检查应随真正实现改成正向+负向检查，不能删除检查或把未实现改为固定成功。

## 实际可运行验证入口（本次均未执行）

- `node cli/entry.ts help`：当前可检查帮助；`node cli/entry.ts sessions --runtime rust --url <隔离服务origin> --token <合成设备token>`：当前仅只读连接/会话路径；将sessions换成status可检状态，但当前status因Agent/model未投影固定退出1。**chat/continue当前预期明确失败**，不得把该退出1作为T09聊天通过。
- `LINGXI_SERVICE_BIN=<本轮候选绝对路径> node cli/entry.ts serve --runtime rust -- --home <隔离绝对home> --bind 127.0.0.1:0`：现有真实进程入口；使用合成配置和外部loopback模型替身。所有token由隔离服务签发，禁止借真实用户home。
- CLI局部测试：`node node_modules/vitest/vitest.mjs run tests/cli-args.test.ts tests/cli-rust-service.test.ts tests/cli-server-runner.test.ts tests/cli-chat.test.ts tests/cli-abort-contract.test.ts tests/cli-agent-selection.test.ts tests/cli-data.test.ts tests/cli-bundle.test.ts`。现有Rust CLI测试还固定“continue不可用”的旧边界，实施后要用真实正向行为替换相应断言并保留鉴权/错误/退出负例。
- UI局部测试：mobile的MobileApp/R02MobileSessionResponse/mobile-platform；services的server-connection/websocket/rust-chat-history/resource-url/stream-resume/composer-send-coordinator。现在mobile会话测试只用旧shape mock，不能替代Rust实际跨端证明。
- 可复用Rust局部测试目标：`auth_matrix`（其中r00_account_password_web_cookie_and_logout_have_real_state、r00_access_devices_pairing_thinking_qr_and_static_boundaries、a06_cli_shape_full_business_roundtrip），`tls_web_login`，`r00_static_web_leaves`，`r06_t05_session_files`，`r06_t05_resources`，`r06_session_history_content`的session_content_serves_only_history_referenced_resources。由主控唯一Cargo时槽按改动范围安排，**不是整轮R06**；这些既有目标不单独证明CLI/mobile已经接通。
- `scripts/rust-tauri/r02_cli_rust_matrix.py`、r02_cli_sessions_leaf_matrix.py、r02_client_leaf_matrix.py、r02_static_web_leaf_matrix.py、r02_management_leaf_matrix.py是现有证据入口；其中cli_rust_matrix**内部会执行Cargo并新建target**，不能在本次只读/唯一Cargo限制下直接启动，也不能拿它旧版“明确不可用”证据冒充T09正向验收。
- 新增必需场景：真实候选进程+CLI真实命令发prompt→真实Run工具生成/登记附件→授权浏览器mobile页面展示相同sessionId/runId/history messageId/sf_/resourceId→同字节下载；刷新/后台/网络重连后同run无重复；另一缺resources.read设备及已撤销设备请求同资源/票据均被拒。先做隔离本机浏览器窄视口，真实手机及Windows/Linux结果另行实测，未测保留未测。

## 精确细分叶（39）

原叶ID和feature ID原样列出；描述取自原地图，不是完成声明。R07-A17/A18为另列的两个基础验收，不计入39。表中其它阶段保持独立责任。

| 叶ID | Feature ID | 原断言摘要 | 执行阶段 |
|---|---|---|---|
| R00-T02-LA-D3710D637C19 | F-D20-CLI-CLI-CHAT-D3710D | 选择或创建会话后通过 WS 提交消息，终端流式显示回复和工具进度 | R02, R07 |
| R00-T02-LA-3B86332B042C | F-D20-CLI-CLI-CONTINUE-3B8633 | 解析目标会话后恢复其历史并可继续提交，终端显示该会话回复 | R02, R07 |
| R00-T02-LA-B8A1AD32A8E1 | F-D20-CLI-CLI-HELP-B8A1AD | 打印命令与参数说明并以 0 退出 | R02, R07 |
| R00-T02-LA-1B09760C2B1C | F-D20-CLI-CLI-SERVE-1B0976 | 以前台服务进程运行，保留指定 channel 与数据降级约束 | R02, R07 |
| R00-T02-LA-200D4E5D52C9 | F-D20-CLI-CLI-SESSIONS-200D4E | 从已认证服务读取并列出最多 20 个会话；空列表显示 No sessions yet | R02, R07 |
| R00-T02-LA-EEC1A2A5CD04 | F-D20-CLI-CLI-STATUS-EEC1A2 | 显示 URL、版本、Studio、Agent、模型和认证来源 | R02, R07 |
| R00-T02-LA-AA65EF26F65E | F-D20-CLI-CLI-SUBCOMMAND-BUNDLE-STATUS-AA65EF | 只读本地 current/next 指针及上次检查、可用版本和错误 | R07, R10, R11 |
| R00-T02-LA-00E57BB5F32B | F-D20-CLI-CLI-SUBCOMMAND-CHECKPOINTS-00E57B | 读取有效 checkpoint 并打印 epoch、日期、store 数量和体积 | R07, R08, R10 |
| R00-T02-LA-1388F8EE0EBC | F-D20-CLI-CLI-SUBCOMMAND-DATA-RESTORE-1388F8 | 精确确认后执行恢复，打印收据和隔离目录；旧数据可从隔离目录找回 | R07, R08, R10 |
| R00-T02-LA-0BE8B22DA5D5 | F-D20-CLI-CLI-SUBCOMMAND-DIAGNOSE-0BE8B2 | 读取 epoch、升级日志、维护状态和可恢复检查点并打印 | R07, R08, R10 |
| R00-T02-LA-289A4033D656 | F-D20-CLI-CLI-SUBCOMMAND-PULL-289A40 | 签名和协议检查通过后激活 renderer 指针并显示版本；已有最新版明确提示 | R07, R10, R11 |
| R00-T02-LA-D1BEE19A95BB | F-D20-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-SERVICE-CONNECTION-D1BEE1 | 端口/短期令牌传给 renderer 后 HTTP/WS 连接可建立 | R02, R07 |
| R00-T02-LA-25AB678E7108 | F-D20-ROUTE_BEHAVIOR-BEHAVIOR-ACCESS-ACCESS-ACCOUNT-PASSWORD-DELETE-25AB67 | 仅本地主人可清除密码；更新账号并记录安全审计，失败时返回错误 | R02, R07 |
| R00-T02-LA-5E4A9CF58BED | F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-ACCESS-ACCESS-MOBILE-QR-SVG-READ-5E4A9C | 返回手机访问地址 SVG，LAN 地址不可用为 400 | R02, R07 |
| R00-T02-LA-229B77A7BA69 | F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-ACCESS-ACCESS-SUMMARY-READ-229B77 | 返回网络、设备和账号概况 | R02, R07 |
| R00-T02-LA-093F22C4FF63 | F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-ACCESS-ACCESS-DESKTOP-CREDENTIAL-093F22 | 登记设备/凭证并返回一次性 secret 和桌面访问地址 | R02, R07 |
| R00-T02-LA-747E0ADC941B | F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-ACCESS-ACCESS-MOBILE-CREDENTIALS-747E0A | 登记设备/凭证并返回一次性 secret 和手机访问地址 | R02, R07 |
| R00-T02-LA-F1754C6F755B | F-D20-ROUTE_BEHAVIOR-BEHAVIOR-ACCESS-ACCESS-ACCOUNT-PASSWORD-PUT-F1754C | 仅本地主人可设置密码；更新账号并记录安全审计，失败时返回错误 | R02, R07 |
| R00-T02-LA-F006E094F028 | F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-ACCESS-ACCESS-ACCOUNT-PROFILE-PU-F006E0 | 保存 username/displayName 并返回 account | R02, R07 |
| R00-T02-LA-73CDC44696D3 | F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-ACCESS-ACCESS-NETWORK-PUT-73CDC4 | 保存网络配置、更新运行时摘要与安全审计 | R02, R07 |
| R00-T02-LA-B8F8CF9E8AF5 | F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-DEVICES-DEVICES-B8F8CF | 仅本地主人读取脱敏的设备、凭证与配对会话列表 | R02, R07 |
| R00-T02-LA-7498024422D7 | F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-DEVICES-DEVICES-DEVICEID-REVOKE-749802 | 令设备失效并返回脱敏设备状态，记录安全审计 | R02, R07 |
| R00-T02-LA-43A126149416 | F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-DEVICES-DEVICES-CREDENTIALS-CRED-43A126 | 令指定凭证失效并返回脱敏状态，记录安全审计 | R02, R07 |
| R00-T02-LA-756217C74101 | F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-DEVICES-DEVICES-PAIRING-SESSIONS-756217 | 仅本地主人得到配对码及到期时间，注册记录并写安全审计 | R02, R07 |
| R00-T02-LA-5E1C3363A070 | F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-DEVICES-DEVICES-PAIRING-SESSIONS-5E1C33 | 仅本地主人用配对码签发设备密钥，返回密钥一次及设备作用域并写安全审计 | R02, R07 |
| R00-T02-LA-8BC1A036AFAA | F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-MOBILE-STATIC-DESKTOP-8BC1A0 | 有桌面构建产物时服务静态页面及资源，否则按路由返回指引或错误 | R02, R07 |
| R00-T02-LA-3291CFD5F7E2 | F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-MOBILE-STATIC-MOBILE-3291CF | 有移动端构建产物时服务静态页面及资源，否则按路由返回指引或错误 | R02, R07 |
| R00-T02-LA-2A1C298F62FC | F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-MOBILE-WORKBENCH-MOBILE-BOOTSTRA-2A1C29 | 返回当前 Agent、语言、头像可用性、工作区和偏好；顺带清理过期持久化 | R02, R07 |
| R00-T02-LA-4BCE8CCFD5DC | F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-SERVER-IDENTITY-SERVER-IDENTITY--4BCE8C | 返回版本、身份及按 principal 投影的能力 | R02, R07 |
| R00-T02-LA-8ED658F9DB9E | F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-SERVER-INDEX-API-SESSION-THINKIN-8ED658 | 返回指定会话或新会话默认级别 | R02, R07 |
| R00-T02-LA-F8935B6B0221 | F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-SESSION-THINKING-LEVEL-SET-F8935B | 有 sessionPath 时只修改该会话，返回解析后的状态 | R02, R07 |
| R00-T02-LA-39AD35E1FD71 | F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-SESSION-THINKING-LEVEL-DEFAULT-S-39AD35 | 无 sessionPath 时修改默认级别，不追改旧会话 | R02, R07 |
| R00-T02-LA-066B54983F6A | F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-WEB-AUTH-WEB-AUTH-SESSION-READ-066B54 | 有效 cookie 返回净化后的 principal；无效/过期返回 authenticated:false | R02, R07 |
| R00-T02-LA-C4E6F27D7873 | F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-WEB-AUTH-WEB-AUTH-LOGIN-POST-C4E6F2 | 校验 token 或本地账号密码，创建 14 日 Web session 并设 HttpOnly cookie | R02, R07 |
| R00-T02-LA-4C1A9735CF04 | F-D20-ROUTE_BEHAVIOR-BEHAVIOR-WEB-AUTH-WEB-AUTH-LOGOUT-POST-4C1A97 | 撤销当前 Web 会话并清除会话 Cookie，返回成功 | R02, R07 |
| R00-T02-LA-5816DA563ED8 | F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-WS-AUTH-WS-TICKET-WEBSOCKET-5816DA | 读取已认证主体并签发绑定连接类型及 /ws 的短期票据；无主体返回 403 | R02, R07 |
| R00-T02-LA-D2657E4AB5FF | F-D20-UI_BEHAVIOR-UI-BEHAVIOR-SERVICE-CONNECTIVITY-D2657E | 连接状态、失败原因与重连结果在 UI 可见 | R02, R07 |
| R00-T02-LA-000E6E1301C0 | F-D20-UI-UI-SETTINGS-ACCESS-000E6E | 逐动作核对实际状态与页面投影；失败不伪装成空数据 | R02, R07 |
| R00-T02-LA-32FFEC05BAA7 | F-D20-UI-UI-SETTINGS-SHARING-32FFEC | 逐动作核对实际状态与页面投影；失败不伪装成空数据 | R02, R07 |

## 取证边界

未修改R07_LEAF_MAP、R00、FEATURE_PARITY_RESULTS、R07_PROGRESS或门禁；未启动T09产品/编译/账号/网络验证。不把旧R02/R06证据重标为T09通过。T08失败与修复仍由主控独立记录。

本次只读地图SHA256：`80ebd469b63dd7fcec275fa4a575dd42068e46ec38c3241ec323f6258e46625d`。
