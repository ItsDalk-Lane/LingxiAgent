# R08-T01 只读范围调查（旧API到Rust兼容映射）

日期：2026-10-22；分支 `codex/rust-tauri-migration`；基点 `86840d170`（工作区有未提交修改，未动任何文件）。调查者：只读 code-explorer 代理，未参与实现。
性质：PREPARATION_ONLY。全部数字为对当前源码实测（grep 逐文件计数），未运行任何业务。
前置阅读：[API_READINESS.md](API_READINESS.md)、[CLIENT_BOUNDARY_PREPARATION.md](CLIENT_BOUNDARY_PREPARATION.md)、[COVERAGE_PREPARATION.md](COVERAGE_PREPARATION.md)、PROGRESS.md（T01 多工具矩阵验收仍等 F02 全收口）。

## 1. 矩阵规模与结构

`docs/rust-tauri/R01/API_COMPAT_MATRIX.json`（单文件，7315 行）：
- 总 634 项：http_route 423 / ws_endpoint 2 / preload_bridge 100 / platform_api_type 109（summary 行 7296-7313）。
- category：business_compat 427（423 HTTP+2 WS+2 引导桥）/ native_host 207。
- disposition：retain 426 / controlled_deprecation 1 / native_host_mapping 207。
- 逐项字段：id, kind, method, path, source(file:line), module, category, disposition, newProtocolCounterpart, notes（112 项 notes 非空）。
- inventory：mountTableSize 48；preload IPC 通道清单（invoke/on/send）。
- **关键限制：无逐项实现状态字段。disposition 是策略标签；newProtocolCounterpart 多为概念标签（如 "lingxi.v1.access GET /api/access/summary"），不证明 Rust 落地。**
- ws_endpoint 仅 2：`/ws`（chat.ts:2256）与 `/internal/browser`（controlled_deprecation，R07 已按用户裁决退役，不重开）。
- preload/PlatformApi 口径差：AST 清点（CLIENT_BOUNDARY_PREPARATION）为 preload 112 项、PlatformApi 110 具名成员；机械提取为 100/109，计数口径不同，不冲突。

## 2. Rust 现役路由清点（rust/crates/lingxi-service/src）

- 主注册表 lib.rs:8063-9052（build_router）。直接 `.route(` 246 条 + `.merge(` 17 个模块路由 164 条 = **410 条注册**（一次注册可挂多方法，方法级端点数≥410）。
- 模块分布（.route 计数，均已核实为生产 router，排除注释/测试内出现）：management 26、git_panel 22、knowledge 17、conversation_routes 14、observability 13、media_admin 10、media_generate 9、workbench 8、browser_routes 8、provider_routes 7、static_web 6、mcp_oauth 5、office_routes 4、desk_files 4、media_routes 3、mcp_apps 3、mcp_settings 2、diary 2、mcp_session_permissions 1。
- 排除项：resourceio.rs:4237/todos.rs:650-653 为注释（路由实际在 lib.rs）；background_tasks.rs:412-414、serve.rs:375 为测试内 router；session_admin.rs:1569 `workspace_disposal_routes` 的 3 条由 lib.rs:8263-8266 现役注册。
- **全部路径在 `/lingxi/v1/*` 下；`/api/*` 路由 0 条——不存在 Rust 侧旧协议兼容层，协议转换唯一发生地是 renderer。**
- WS/实时面：`/lingxi/v1/ws`（ws_handler lib.rs:7329）、`POST /lingxi/v1/ws-ticket`（7288）、`/lingxi/v1/browser-host`（9003）；事件游标面 `/lingxi/v1/app-events`（8692）、`/lingxi/v1/sessions/{id}/events`（8293）、`/lingxi/v1/resource-io/events`（8533）。

## 3. 旧客户端消费者清点

- HTTP 主入口 `desktop/src/react/hooks/use-hana-fetch.ts:15-27`：serviceRoute = 内联正则（skills/agents skills|skill-bundles/channels/dm/desk beautify/conversations 四类/session-collab apply|reject/media/speech-recognition）→ rustAutomation→rustObservability→rustBridge→rustBrowser→rustWorkbench→rustSettings 链式映射；未命中路径原样返回。
- 适配器（各文件首行即映射表）：rust-settings.ts（config/agents/memories/cards/skills/experiments/models/providers/user-profile/agent-deletions/speech-recognition/media三系/preferences八类/checkpoints/workspace-rollback/settings快照/health/avatar/access/devices）、rust-workbench.ts:11-21（git 22 动词/workbench/studio/desk五类/diary/resource-io/mobile-workbench）、rust-browser.ts:6-9、rust-bridge.ts:2-6、rust-observability.ts:6-28、rust-automation.ts:24-31。
- 硬闸：server-connection.ts:731-738 非 `/lingxi/v1/` 直接抛 "Rust service has no migrated endpoint"；:751-752 WS 仅 `/lingxi/v1/ws`；:810-819 统一 Bearer 头；:765-778 远端 WS 先取 ticket。
- chat-client.ts:28-70 chatFetch：upload-blob→sessions/{id}/attachments/blob（含响应投影）、models→chat/models、models/set、models/switch、session-thinking-level；default 显式报"聊天服务尚未迁移此请求"。
- 会话域双分支（native/legacy）遍布 session-actions.ts（993 列表、1239 switch、1580 new、1870-2240 archive/pin 等）；历史走 rust-chat-history.ts:186-230（schema `lingxi.canonical-history.v1`/sessionId/ETag/before 游标/hasMore↔nextCursor/reconciliation 强校验），legacy `/api/sessions/messages` 仅非 Rust 连接。
- WS：websocket.ts:268 Rust 用 `/lingxi/v1/ws`+ticket+ClientHello；就绪分支 300-315 启动 pollers/resume 并 return（旧分支 317 才发 terminal 快照请求）。
- 平台面（T02 交界）：platform.js:10-12 Electron 直接别名 window.hana；:59-63 Web fallback 直 fetch `/api/fs/*`（Rust 下无映射即断）。preload/PlatformApi 细账见 CLIENT_BOUNDARY_PREPARATION.json（84 处 hana 调用/23 文件、146 处平台调用、Web 63 项中 55 常量返回）。

## 4. 差距清单

### (a) 矩阵状态
- 矩阵无实现状态可对照；需新建"矩阵项→Rust 路由→消费者"三方绑定表（当前不存在）。旧 423 HTTP vs 现 410 注册不是 1:1（合并/拆分发生），必须逐项绑定而非数数。
- `/internal/browser` 唯一 controlled_deprecation，退役路线维持，不因矩阵文字重开。

### (b) 旧客户端在用、Rust 连接下走不通（实测）
1. `/api/confirm/{id}` POST——4 个生产组件（AssistantMessage.tsx:1277/1303、SettingsConfirmCard.tsx:95/106、AutolearnLessonCard.tsx:41、SessionConfirmationPrompt.tsx:298；ask-user/computer-app/mcp 审批提示同路径）。**Rust 无 confirm 路由**（全源码 grep 无 `/lingxi/v1/confirm*`）。
   **【2026-10-22 动态链更正】**：上段为静态路径结论；动态链核查后原"批准链断=A01 阻塞"判断不成立——旧 confirmStore 流在 Rust 侧**无生产者**（Rust 事件/历史投影不产生 confirmation 块，rust 源码 "confirmation" 块类型零命中；history_content.rs 提及的 confirmation card 属 session-collab 投递确认，走已接线的 apply/reject）。四组件仅在 legacy 事件/历史下渲染。**替代链全部已接线且有真实端点**：工具批准=`GET/POST /lingxi/v1/sessions/{id}/approvals(/{aid})`（RustSessionReviewPanel.tsx，含 MCP 本会话免问）、ask-user=`interactions`+`interactions/answer`（InteractiveToolsPanel.tsx）、跨会话投递=session-collab apply/reject（已映射）、定时建议=apply_suggestion（批次A 实测）、learn_lesson 风险=R06 一次性 token。**裁决**：不实现 `/lingxi/v1/confirm`（会造第二确认裁决、平行空接口）；旧路径触达由 server-connection 硬闸显式拒绝（use-hana-fetch.test.ts 新增专项断言）；矩阵 newProtocolCounterpart "lingxi.v1.confirm" 为无落地证据概念标签，更正为 legacy-only＋显式能力错误——判据变化待独审（绑定表 `T01_API_BINDING_TABLE.json` 已带裁决注记）。T07 A/B 需验证用户可见批准 UX 语义等价（聊天卡 vs 面板）。
2. knowledge：knowledge-api.ts:176-379（18+ lingxiUrl snapshot）无映射；Rust knowledge.rs:4501-4545 17 条 1:1 就位。纯接线。
3. Rust 路由已就位、仅缺映射的还有：file-history（file-history-api.ts:42-72 ↔ lib.rs:8560-8573 四条）、session-projects（session-project-actions.ts:48-207 九处 ↔ lib.rs:8720 起）、sessions/search（use-session-search.ts:86/95 ↔ lib.rs:8285）、history-overview（history-overview-client.ts:164 ↔ lib.rs:8320，路径形状需 sessionId 改写）、conversation-map/turns（conversation-map-projection.ts:202 ↔ lib.rs:8337 路径形状不同；layout 已有原生分支 :218）。
4. Rust 无对应物：`/api/commands`（use-slash-items.ts:100）；terminal WS 消息 terminal_snapshot_request/terminal_tail_request（terminal-client.ts:33/58；Rust 就绪分支不发送、无处理者）。需实现或显式能力错误（红线：不得静默降级）。
5. mcp-approval-actions.ts:48/108/125 直连 `/api/mcp/*` 绕过 mcp-api.ts:27-65 原生分支（Rust mcp 路由存在）——应统一走 mcp-api。`/api/session-collab/create` 拒绝为测试固化（use-hana-fetch.test.ts:160），保留。

### (c) WS 事件流 vs A02
- 机制完备：chat_wire.rs:220-274 publish 按 stream 加 seq；resume（1367-1437）先 owned 检查再回放有界缓冲，缺口 reset/truncated 明示，不调 prompt；客户端 stream-resume.ts:261-287 resume_stream+sinceSeq+代次去重；websocket.ts:311-313 重连只 resume。
- 差距：`r06_chat_wire::real_ws_resume_recovers_exact_missing_events_and_marks_stale_or_empty` 用 LIVE-DELTA，未证"已执行副作用工具不二次执行"；旧 UI 真实断线/重启/过旧 cursor 全链路零证据。

### (d) 认证/错误/分页
- 认证：客户端 Bearer 头（旧服务亦认 query token——lingxiUrl includeTokenQuery 用于资源 URL）；Rust auth.rs loopback token 仅本机、登录身份、classify_route 未知形状 fail-closed LocalOnly（session_admin.rs:1564-1568 注释）、媒体/桥公开 token 面（auth.rs:1278-1300）。差异点：四主体（本机/远端/guest/手机）错误映射、token 失效/撤销、ticket 过期在旧客户端的呈现未验。
- 错误：Rust EndpointError（lib.rs:3617：status+ProtocolError{code,message,retryable,details.reason}+causeId）vs 旧 `{error,message}`；lingxiFetch 非 2xx 仅抛 status/statusText（use-hana-fetch.ts:74-76），个别消费者 throwOnHttpError=false 自行读 body——需单一 ServiceClient 统一解析（API_READINESS 缺口4，仍成立）。
- 分页：历史域严格（见 §3）；其余列表（会话列表 studio-list、trace、媒体、bridge deliveries）游标与筛选/主体绑定、跨页重复、空页未对账；u64/seq 有 safe-integer 防护但需逐域核。

### (e) A01 现有证据
- 服务端：chat_wire.rs:308-311 tool_call_started/completed→tool_start/tool_end（沿 toolCallId 找 name，保 status/content/details）；run 终态与 final 消息来自内核事件单源。
- 客户端：projectRustHistoryItems（rust-chat-history.ts:71）不造 final、opaque 单列、legacy 归档显式标注。
- 测试：r06_chat_wire.rs（真实 WS prompt/abort/steer/resume/typed-refusals）、r06_t04_history、r06_session_history_content、rust-chat-history.test.ts（三分支投影）——均各自层内；旧 UI 逐卡展开 golden/同产物 A/B 无（A01 证据要求"接口golden与UI结果"）。

## 5. 既有资产复用判断

- scripts/rust-tauri/r02_cli_rust_matrix.py：CLI↔真实 Rust 服务场景证据（源 manifest 全量哈希）。R02 真实执行过（artifacts/rust-tauri/R02/T04/gates 等）；当前候选是否重跑：UNCERTAIN。
- r02_client_leaf_matrix.py（R00 CLI help+SharingTab 叶）、r02_management_leaf_matrix.py（management 路由+TLS web login 叶）：同上，R02 时代有回执，现候选未证。
- renderer 测试（可直接扩展）：use-hana-fetch.test.ts（映射+显式拒绝断言）、rust-{settings,workbench,observability,browser,automation,media-http,conversation-events-http,browser-events-http,chat-history,agent-activities}.test.ts、server-connection.test.ts、stream-resume.test.ts、websocket.test.ts。
- Rust 集成测试 100+ 文件（r04-r08）可直接选作 A01/A02 载体：r06_chat_wire、auth_matrix、r07_t08_*、r07_bridge_*、r07_t10_observability、r06_t05 系。
- r01-t02-extract-api-surface.mjs / r08_client_inventory.mjs：两台可复核生成器，是三方绑定表的现成骨架。
- 注意：xtask 128 项与 R08 门禁（gateReadiness.registeredInXtask=false）不代替本任务验收。

## 6. 推荐开工顺序

1. **纯核对（0 风险）**：生成三方绑定表（矩阵 427 business 项 × 410 路由 × 消费者），落 `--check`；产出即 (a) 交付物"API_COMPAT_MATRIX结果"的第一版。
2. **纯接线（Rust 已就位）**：knowledge（17:1:1）→ file-history → session-projects → sessions/search → history-overview/conversation-map（后两个需路径改写）；每项配 rust-* 适配单测 + 一条 real-service HTTP 测试（仿 r07-settings-real-service.test.tsx）。
3. **补 Rust 实现**：`/api/confirm/{id}` 批准决议路由（对接 toolgateway/automation review 的既有权限裁决，兼容层只转换、Rust 唯一裁决）；`/api/commands`（或并入 slash.rs）；mcp-approval-actions 收敛到 mcp-api 原生分支；terminal WS 消息（实现或显式 capability 错误）。
4. **契约统一**：ServiceClient 统一 EndpointError 解析（code/reason/retryable 上浮到旧错误 UI）；列表域游标对账。
5. **A01/A02 证据**：先扩 r06_chat_wire 式真实 WS 测试——多工具（read→write/edit→exec）+ 副作用计数断言断线重连（A02）；再上旧 UI golden A/B（A01，等 F02 收口后与 T07 同产物对齐）。

最小可验证第一步：§6.1 绑定表（一次命令可复核）；第一个实现步：§6.2 knowledge 接线（一次提交含映射+单测+真实 HTTP 测试）。

## UNCERTAIN 汇总

r02 三脚本在当前候选上的重跑状态；`/api/commands` 是否有等价 slash 路由承载（未逐条读 slash.rs 全部路由语义）；远端/guest/手机三主体的 ticket 与错误映射运行时行为（只做了静态核对）。
