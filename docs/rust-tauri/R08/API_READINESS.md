# R08 T01/T02 API 接入准备核对

日期：2026-10-21；受查候选基线 `86840d1700397d3dd620a41f2b884df4cddd5cc2`。状态：**PREPARATION_ONLY / NOT_RUN**。本报告只读原矩阵、真实调用链和现有测试源码；没有运行 R07 业务、修改实现、修改原矩阵、把路由存在当语义通过。

## 结论与口径

R01 原矩阵有634个表面描述：423 HTTP、2 WS、100 preload成员、109 PlatformApi类型成员；其中427 business_compat=423 HTTP+2 WS+2连接引导桥，207 native_host=98 preload+109类型描述。后两者重叠描述同一宿主能力，不能计作207个独立功能。原矩阵的 newProtocolCounterpart 多为概念标签，不能拿字符串存在当实际Rust入口或完成证据。

当前Rust HTTP主要走 `/lingxi/v1/`，已有React适配将旧路径转成新路径；不存在通用“全部/api兼容”保证。`buildConnectionUrl` 对Rust的非版本化路径直接拒绝，有助于避免回旧内核，但必须给每个实际消费者建立覆盖结果。T01复用现有适配并收敛为唯一ServiceClient，T02复用PlatformApi/usePlatform而形成真正DesktopHost；不另叠空接口，也不借迁移重做UI。

## 已有调用链

- HTTP：组件/设置api → `hooks/use-hana-fetch.ts::lingxiFetch/lingxiUrl` → `serviceRoute` → rust-settings/workbench/browser/bridge/observability/automation适配 → `server-connection.ts::buildConnectionUrl/appendConnectionAuth` → Rust真实路由与auth。部分领域另有 `chat-client.ts::chatFetch`、`session-connection-owner.ts::sessionOwnerFetch`，须一起收敛，不能留第二套业务裁决。
- WS：`services/websocket.ts` 选择 `/lingxi/v1/ws`、申请远端ticket、发ClientHello，验证ServerHello后才ready；Rust `lib.rs::ws_session` → `ws::WsClientRequest` → `chat_wire::prompt/abort/resume/steer/compact`；规范事件订阅另走events面。实时旧显示帧由Rust `chat_wire` 从实际内核事件投影，前端不应自己宣布任务完成。
- 历史：`services/rust-chat-history.ts::readRustChatHistory` → `/lingxi/v1/sessions/{id}/history` → service `history.rs`/真实message store → `projectRustHistoryItems` →同一聊天投影。schema/sessionId/ETag/page游标严格校验，未知协议显式错误、opaque提示保留。
- 身份：`ServerConnection` 已携带serverNodeKind/transport/user/studio/capabilities；本机桥 `getServerConnectionInfo/getServerPort/getServerToken` 只作可信引导。`connectDeviceServerConnection` 先旧登录，只有Rust公开健康确认后转Rust登录与认证identity，不以旧路由401/404就猜成Rust。

## 原矩阵按功能域聚类

以下覆盖全部423 HTTP描述；数量只表示原责任入口数，不是通过数。

| 功能域 | 原HTTP数 | 真实消费者/适配与Rust落点 | R08必须验证 |
|---|---:|---|---|
| 连接、账户与访问控制 | 26 | server-connection.ts、settings AccessTab；Rust auth.rs/management.rs、ws-ticket | 本机/远端/guest/手机四权限，token失效、撤销、cookie/ticket、跨主体拒绝及错误映射。 |
| 会话、项目、草稿、地图 | 55 | stores/session-actions与session-connection-owner；Rust session_admin/session_tree/history/session_projects/input_drafts/session_map | 稳定ID/列表分页/归档恢复/fork/旧引用与冲突，切换连接后丢弃旧响应而不误写新目标。 |
| 聊天命令与批准 | 3 | chat-client、websocket、composer-send-coordinator、ws-message-handler；Rust chat_wire/slash/toolgateway | 多工具生命周期/无final/批准参数绑定、Rust唯一终态、重连只读不重发。 |
| 设置、模型、人格、卡、偏好 | 118 | settings/api、rust-settings、chatFetch；Rust settings_configuration/agent_config_readback/provider_config/client_context/persona/userprefs等 | 原字段/未知配置/掩码秘密/辅助槽/显式agent与model provider身份不丢；部分保存失败真实通知。 |
| 记忆与知识 | 23 | components/knowledge/knowledge-api、settings记忆页；Rust knowledge/memory/agent_memory | 知识入口目前缺统一路径映射；引用/快照URL/分页/目录导入/取消，原记忆格式和权限。 |
| Skills | 16 | use-hana-fetch特殊映射、settings技能页；Rust skills/skill_library | 状态/安装审核/启停/导入导出/实际内容，不能只测目录列表。 |
| 资源、文件与预览 | 35 | resource-url/access、预览工具、chatFetch附件、use-hana-fetch；Rust sessionfiles/resourceio/resources/fsread/filehistory | fileId/alias/resource链接、远端不得本机回退、冲突版本、断线订阅、文档HTML隔离。 |
| 工作台与环境 | 62 | rust-workbench、RightWorkspacePanel；Rust workbench/git/desk/studio_workspaces/env-deps | 文件操作与git动作真落受控节点，事件恢复/错误/取消；原生系统动作分给T02宿主。 |
| 频道、私信、协作 | 20 | rust-workbench/agent-activities、conversation events；Rust conversations/session_collab | 原因果、权限、确认/拒绝、旧连接迟到事件、自动化结果不串会话。 |
| Bridge | 14 | rust-bridge、BridgeTab；Rust bridge_routes/bridge_session_routes/bridge_* | 五平台替身协议、owner/原权限、媒体资源、会话重置、投递去重与可解释未知。 |
| 媒体 | 28 | rust-media、media卡/生成面板；Rust media/media_generation/media_gallery/beautify | 提交/生成/下载/失败/取消/unknown分开，真实文件交付、重启和回执版本稳定。 |
| 观测 | 14 | rust-observability、settings/observability；Rust observability、model_call_usage | 筛选分页/轨迹/详情/导出/存储与维护，trace导入原ID与秘密策略。 |
| 服务直挂入口 | 9 | 引导/健康等由实际路径归属核对；Rust main/lib/management | 逐条区分公开最小健康与认证业务，不能因直挂而跳过认证/能力契约。 |

## 具体缺口与需要保留的保护

1. **知识库UI明确缺路径接线。** `knowledge-api.ts` 的 list/import/citation等调用 `lingxiFetch('/api/knowledge/...')`，snapshot URL也调用lingxiUrl旧路径；`serviceRoute`与现有rust-*没有knowledge匹配，最终buildConnectionUrl在请求发出前拒绝。Rust已有knowledge处理器。应在唯一服务适配补完整知识域与URL消费；不是重开已接受R06内核，也不能隐藏知识库入口。
2. **能力握手未用已存在的真实计算。** protocol `handshake.rs::accept_hello` 会按known_caps填rejected_caps；真实service `lib.rs:7445` 只negotiate_protocol后手造ServerHello，`rejected_caps: Vec::new()`固定空。当前renderer发caps=[]，因此尚不能据此推断现用路径错误，但非空未知cap必须经真实WS验证显式拒绝/说明，不能拿协议库单测代替。renderer `isRustWireServerHello`只检查rejectedCaps是字符串数组，没有必需cap阻断策略。
3. **版本轴要分清。** 旧serverProtocol提示函数明确仅诊断，不是迁移协议门禁；Rust wire实际已有版本协商与version_incompatible关闭。renderer hello要求dataEpoch等于旧shared DATA_EPOCH，若T05建立新epoch，须同步明确客户端支持范围及错误信息，不能为通过而改fixture印章。Electron main的identity检查当前主要核kind/transport/server/studio，版本与epoch须和T03就绪链统一。
4. **错误细节在通用HTTP层丢失。** Rust EndpointError有code/message/retryable/details.reason/causeId；lingxiFetch默认失败仅抛status/statusText，requestJson类似。个别消费者throwOnHttpError=false自行读body。T01要保留实际消费者需要的字段、区分未批准/过期/能力缺失/文件冲突/忙/重启，不以普通“请求失败”掩盖责任；不要让多个组件各解析一套错误。
5. **路径匹配只能证明转换。** rust-*正则对未知路径返回原路径，buildConnectionUrl再拒绝，这是诚实拒绝，不代表该保留功能完成。特别资源链接来自服务数据、fs、session专属操作等，须从实际消费者路径核对，不做无条件替换所有/api以误映射权限不同端点。原矩阵旧标签 `(unmounted?)` 也不能自动当“不适用”。
6. **分页/身份必须按返回数据对账。** readRustChatHistory已有schema/session/ETag/before/hasMore校验；其它列表（会话/trace/知识/媒体/Bridge）不能继承为通过，须检查cursor绑定筛选/主体、重复跨页、空页和旧游标明确恢复。所有u64/seq不得JS精度损失；chat旧序号入口已有safe integer限制，规范wire为独立轴。
7. **T02仍有真实宿主直连。** rust-browser的请求函数、rust-automation通知、ws-message-handler通知、SkillViewer、About/更新/快捷键/观测导出等有window.hana直接调用；usePlatform当前只是返回window.platform；platform.js Electron直接别名window.hana、Web有大量静默false/null/no-op与旧/api/fs直fetch。必须做有实际实现的统一宿主边界，Web缺能力给明确解释，不能仅改名字/空接口。
8. **终端不能从文件工具已过推定。** terminal-client仍发terminal_snapshot_request/terminal_tail_request，websocket Rust ready分支没有执行旧分支的requestTerminalSnapshot；需核实际Rust WS请求枚举/HTTP终端事件链及真实面板覆盖。这里是待查接线，不在本报告伪造失败或通过。

## A01 多工具事件与历史的实际验收落点

Rust `chat_wire.rs::publish`为同一stream加seq/sessionPath；`observe`把tool_call_started/completed转换成tool_start/end，沿toolCallId找name，保留status/content/details；run状态和final消息来自正式内核事件。历史 `projectRustHistoryItems`沿run事实/段/工具结果投影，opaque单列提示、无final不能编造final。T01/T02测试应在同一真实Run中做至少read→write/edit→exec多工具任务，比较实时、重开、分页、导出和重连的toolCallId/parent/session/run/segment/fileId、错误和终态；只assert存在任意tool_end不够。

可复用既有具名测试（本报告未执行）：

- `r06_chat_wire::real_ws_prompt_enters_run_tools_and_live_final_with_typed_input_refusals`：真实provider替身+真实read工具与WS，核拒绝输入不增消息/请求；只保证它自己断言的范围。
- `r06_chat_wire::real_ws_delta_is_visible_before_provider_terminal_and_abort_stale_guard`、`ws_real_steering_tool_continuation_idle_prompt_and_foreign_controls_do_not_mutate`：实际流/中止/继续/身份边界。
- `rust-chat-history.test.ts` 的 `never seals an unresolved partial or invents a terminal from an empty tail`、`keeps started and unknown tool outcomes distinct from success and failure`、`marks legacy archives explicitly and rejects unknown future schema rather than returning an empty success`：纯客户端投影，不是UI/真实导入证明。
- `r06_t04_history`、`r06_session_history_content`、`r06_ar_f01_history`：真实历史服务/分页/导出相关测试，按原阶段责任选择受影响部分，不重开全R06。

必须另补R08实际旧React界面逐卡展开（汇总区/工具/思考/MOOD/错误/产物）及固定同产物A/B；组件测试中mock掉内核或仅后端PASS不能完成A01/A04/A13。

## A02 重连只续读的实际验收落点

renderer `websocket.ts::markConnectionReady` 在Rust握手后只启动已接通的pollers、reconcile待确认输入、requestStreamResume；`stream-resume.ts`发resume_stream、以stream/seq/恢复代次去重。`composer-send-coordinator::reconcilePendingComposerSessions`调用实际协调读取，应在R08跟踪其网络请求确认没有主动prompt重发。Rust `chat_wire::resume`先owned检查、读取active runs和有界replay缓冲后返回stream_resume，没有调用prompt/execute；缓冲缺口以reset/truncated明确要求历史重建。

现有 `r06_chat_wire::real_ws_resume_recovers_exact_missing_events_and_marks_stale_or_empty` 验断线后seq连续、stale/空流和resumeToken；它使用LIVE-DELTA，未单独证明已有写工具/外发动作不重复。`real_provider_many_deltas_resume_reports_truncation_and_retains_contiguous_tail` 可复用截断边界。客户端 `stream-resume.test.ts` 有 repeated-resume去重、stable sessionId、runtime still running/idle等断言，属于客户端层。

R08需从旧界面真实完成有副作用工具后断开、重连同流、重复resume、过旧cursor、服务重启，再查provider请求/实际文件操作/外部替身计数/正式invocation与Run收据数量。网络抓取应只见订阅/查询/显式续读；未确认提交只能协调，不得盲重发用户命令。已有受理requestId跨进程的明确conflict边界要保留，不能通过自动换ID重发来“恢复”。

## 宿主矩阵与T02配合

原native_host的98 preload与109 PlatformApi类型描述可聚为：窗口/托盘/快捷键/通知；文件对话框/拖放/剪贴板/系统打开；受控文件预览/观测导出；浏览器viewer/control；语音权限/采集；应用版本/更新/公告/重启/主题；跨窗口事件。T02负责Electron/Web受限实现和组件边界，业务文件权限/资源范围、浏览器执行授权、通知投递收据仍由Rust service裁决，不能因IPC就归宿主豁免业务授权。

两个business preload `getServerPort/getServerToken` 是T01/T03引导+T02桥的交界，本机token只经受控owner通道。PlatformApi原矩阵整体native_host分类不能据此把其中业务引导语义丢掉。

原WS `/internal/browser` 在旧矩阵列controlled_deprecation；本轮用户已明确沿用R07退役，不应恢复旧WebContentsView。R08必须验证实际替代浏览器宿主（rust-browser→受控IPC→Rust授权→独立浏览器）对应覆盖和Web缺能力提示；不能用旧矩阵文字把退役路线重新打开。

## 可复用测试群与本次限制

HTTP转换已有 `hooks/use-hana-fetch.test.ts`、`services/rust-settings/rust-workbench/rust-observability/rust-browser.test.ts`；身份与ticket已有server-connection.test.ts；真实HTTP+客户端有rust-media-http、rust-browser-events-http、rust-conversation-events-http；Rust群有auth_matrix、r07_t08_settings/provider_http、r07_t10_observability、r07_bridge_*、r06_t05_resourceio/resources/filehistory、r06_chat_wire。上述名称只是已存在入口，均须绑定实际新候选/命令/测试名/日志和R08原叶责任才能写PASS。

本报告未穷举423路由逐项语义结果，未做任何运行断言，未更改原矩阵/实现/进度；下一步由主代理在准入裁定后接通具体责任及正式证据生产者。全阶段126补充叶与27 laterShare仍以专属叶地图为准，不按此634接口描述重复计数。

输入摘要：API_COMPAT_MATRIX SHA-256 `0dce885626ab4d3a4f6f28b050148847d34ba8087fc657740f65995aa4dcf520`。
