调查完成。以下为 8 条链的"真实调用链"证据（全部为实测 文件:行，非推断）。

# 链 1：桌面用户输入链

1. `desktop/src/react/components/InputArea.tsx:1814` `submitEditorMessage(type)` → 首条消息先 `ensureSession`（pending-new-session → 真实会话，`InputArea.tsx:2048` `transferLeaseIdentity` 原子转移租约身份）→ `InputArea.tsx:2018` `tryAcquireSendLease(...)`（会话级发送租约）→ `InputArea.tsx:2078` `sendWithLease(leaseId, deps)`
2. `desktop/src/react/services/composer-send-coordinator.ts:1000` `sendWithLease` → `:1010` `prepareComposerSend(bundle,...)`（快照/预检/base64）→ `:1036` `commitPreparedComposerSend(prep.prepared, {...})`
3. `desktop/src/react/components/input/composer-send.ts:198` `prepareComposerSend`（组装 wsMsg，:495-517）→ `composer-send.ts:553` `commitPreparedComposerSend` → **`composer-send.ts:577` `ws.send(JSON.stringify(prepared.wsMsg))`**（type 为 `prompt`/`interject`）
4. WS 通道：`desktop/src/react/services/websocket.ts:97` `connectWebSocket` → `:124` `requestConnectionWsTicket(connection)`（`server-connection.ts:684`，POST /api/ws-auth）→ `websocket.ts:131-132` `buildConnectionWsUrl(connection,'/ws',{wsTicket})` + `new WebSocket(url)`。端口/token 来源：`app-init.ts:125-126` `platform.getServerPort()/getServerToken()` → `preload.cjs:33-35` `window.hana` 桥（IPC `get-server-port`/`get-server-token`）→ `desktop/main.cjs:5394-5395` handler。**业务消息不经 IPC，IPC 只传端口/token**
5. server 侧：`server/composition/open-root.ts:85` `createChatRoute(engine,hub,...)`、`:89` `app.route("", chatWsRoute)` → `server/routes/chat.ts:2256` `wsRoute.get("/ws", upgradeWebSocket(...))`；upgrade 请求先过 `server/index.ts:589-637` 中间件（:626 `resolveHttpRequestPrincipal(c,engine,{serverAuthService,wsTicketService,...})` 鉴权）→ `chat.ts:2272` `onMessage` → `chat.ts:2672` `msg.type==="prompt"||"interject"` 分支（门禁：:2756 非 interject 且 `engine.isSessionStreaming`→`session_busy` 拒；:2764 model_switching 拒；媒体校验 :2686-2740）→ **`chat.ts:2888` `hub.send(promptText,{sessionId,sessionPath,...})`**
6. `hub/index.ts:213` `Hub.send` 路由表（:299-343）：桌面 owner（`!sessionKey && !ephemeral && role==="owner"`）且带 sessionPath → **`hub/index.ts:303` `submitDesktopSessionMessage(this._engine,...)`**；无 sessionPath 旧路径 → `hub/index.ts:322` `engine.prompt`
7. `core/desktop-session-submit.ts:385` `submitDesktopSessionMessage`（知识检索 :560-604、reminder/知识块注入 :612-638、`session_user_message` 回执 :677-697）→ **`desktop-session-submit.ts:745` `engine.promptSession(sessionPath,promptText,promptOpts,{afterCachePreflight,afterInputAccepted})`**
8. `core/engine.ts:2152` `promptSession` → `core/session-coordinator.ts:5176` `SessionCoordinator.promptSession`（:5193 `runWithModelTraceRoot({origin:"user_turn"})`）→ **`session-coordinator.ts:5278` `await entry.session.prompt(text,promptOpts)`**（Pi SDK AgentSession）

结论：桌面链收敛——所有输入经唯一 `/ws` 路由（chat.ts:2256）→ 唯一 `hub.send` 路由表 → promptSession → Pi session.prompt。无旁路。

# 链 2：CLI 链

1. `cli/entry.ts:17` `main(argv)` → `:64` `resolveConnection({url,args.token})`（`cli/local-server.ts:70`：读 `$LINGXI_HOME/server-info.json` 拿 port+token，:51 PID 存活检查）
2. 无连接且 chat/continue → `cli/entry.ts:67` `startLocalServerAndWait`（`cli/server-runner.ts:199`：`resolveServerSpawnSpec` :61 → packaged 模式 spawn `bootstrap.js`（:90），source 模式 spawn `server/main-full.ts`（:106）→ 轮询 server-info.json :218-222）
3. `cli/entry.ts:88-89` `startChat(client,...)` → `cli/chat.ts:12` 消息形状 `{type:"prompt",text,sessionId,sessionPath}` → `cli/chat.ts:347/364` `ws.send(...)`；WS 建连 `cli/client.ts:66-77` `createWebSocket()`（Bearer header 或 `?token=`，仅本地允许 query token）
4. 之后与链 1 第 5 步起完全同路：`server/routes/chat.ts:2672` prompt 分支 → `chat.ts:2888` `hub.send` → `hub/index.ts:303` → `desktop-session-submit.ts:745` → `session-coordinator.ts:5278`

结论：CLI 与桌面共用同一 `/ws` 路由与 hub.send 网关，无独立旁路；`--url` 远程连接只是换 baseUrl，仍走同一鉴权。

# 链 3：server 内部装配

1. 入口关系：`server/main-full.ts:17-24` 静态 import `server/index.ts` 的 `startServer` + `server/composition/full-root.ts:22` `registerClosedRoutes`（avatar/character-cards/cards/desk/diary/git-environment）+ `builtinMediaAdapters`，一次性静态组合；`server/index.ts` 自身只导出 `startServer`（:120），不自行 boot
2. 顺序（`server/index.ts`）：同宅互斥闸 :273-291 → 数据 epoch 闸 :297-349 → token 生成 **:351** `SERVER_TOKEN = process.env.LINGXI_TOKEN || crypto.randomBytes(16)` → 监听 :406 → `ensureFirstRun` :418 → **Engine 创建 :436** `new LingxiEngine({lingxiHome,productDir,...})` + `engine.init` :445 → **Hub 装配 :476** `new Hub({engine})`（Hub 构造内 `hub/index.ts:107-139` 注入 EventBus/ChannelRouter/GuestHandler/Scheduler/DmRouter 并 `engine.setEventBus`）→ 扩展注册 :493-551（含 `createModelCallObserverExtension` :551）→ `engine.initPlugins(hub.eventBus)` :554 → `hub.initSchedulers()` :557 → 认证服务 **:576** `createServerAuthService({loopbackToken:SERVER_TOKEN,...})` + wsTicket :581 → 路由挂载 **:1021** `registerOpenRoutes(app,ctx)`（`composition/open-root.ts:72`）+ :1023 `root.registerClosedRoutes?.` → server-info.json（含 token，0600）:1280-1300 → `setImmediate` 桥启动 :1312 → `--cli/--chat` 时 `startCLI`（`server/cli.ts:98` `ws://127.0.0.1:port/ws?token=...`）
3. 鉴权：`core/server-auth.ts:35-90` `authenticateRequestDetailed`：Bearer/query token == loopbackToken 且 connectionKind==="local" → local_user principal（:63-71）；否则 device credential（:73）；无 credential 时 web cookie session（:45）。`server/http/request-principal.ts` 由 `server/index.ts:626` 调用
4. "Engine Manager"：**不存在独立 EngineManager**；唯一 `new LingxiEngine` 在 `server/index.ts:436`。引擎内部的 AgentManager（`core/engine.ts:642`）、SessionCoordinator（:664）、ConfigCoordinator（:768）、AuxiliaryModelResolver（:789）、ModelOperationResolver（:804）、BridgeSessionManager（:817）都在 `LingxiEngine` 构造/init 内装配

结论：单一组合根（main-full.ts → startServer），open-root（必挂）+ full-root（闭集，静态入参）双层路由；认证单一来源 server-auth.ts + index.ts:626 中间件，无路由绕过该中间件（公开路由白名单 `isPublicHttpRoute` :619 与 resource ticket :614 例外）。

# 链 4：agent 执行链（模型调用与工具执行）

1. `core/engine.ts:2152` `promptSession` → `core/session-coordinator.ts:5176`（trace 根 :5193 origin=user_turn，复用 traceId :5203）→ `:5278` `entry.session.prompt(text,promptOpts)`（Pi SDK AgentSession，其内部 AgentLoop → `session.agent.streamFunction` → pi-ai provider HTTP）
2. Pi 会话创建（唯一入口）：`core/session-coordinator.ts:2214-2230` 组装 sessionOpts（tools 来自 :2201 `this._d.buildTools(...)`）→ **`lib/pi-sdk/index.ts:78` `createAgentSession`** → `:83` `rawCreateAgentSession`（@earendil-works/pi-coding-agent）+ `:84-89` 安装 tool-outcome-adapter / assistant-stream-guard / **model-call-stream-observer（streamFn 边界观测，lib/pi-sdk/model-call-stream-observer.ts:155）** / model-call-trace-ingress / desktop-input-commit / todo-reminder。session-coordinator 另在 :7482 包一层 streamFunction（cache-prefix 契约 + context ring 分类，非观测）
3. 模型解析：聊天模型走 `sessionOpts.model`（session-coordinator.ts:2227 `effectiveModel`，由 model-manager 从 Pi ModelRuntime 解析）；**`core/model-operation-resolver.ts` 只管 embedding/rerank 等非 chat 操作**（`engine.ts:2511/2530/2599/2731` 新建 resolver，`resolveSync/resolveFresh` :103-114，无 ref 返回 null → 调用方 fallback）。辅助模型（summarize 等）走 `engine.resolveAuxiliaryModelFresh`（`server/index.ts:730/775` 的 `utility:call-text`/`model:sample-text` bus handler → `core/llm-client.ts` `callText`）——这是 chat 主链之外的旁路 LLM 调用（有独立 trace 根 `runWithModelTraceRoot({origin:"plugin"})`，server/index.ts:734/779）
4. 工具执行回网关：`core/engine.ts` `buildTools` 内 **:4119 `new ToolInvocationGateway({registry:targetRegistry, authorize:()=>throw...})`**（model-facing authorize 必须由 session permission wrapper 完成，engine.ts:4122-4124）；plugin/MCP/first-party 目标统一 `targetRegistry.register`（:4143/:4194）
5. 真正的权限检查点：`core/engine.ts:4579/:4598` `wrapWithSessionPermission(result.tools/result.customTools,{getPermissionMode,getConfirmStore,getApprovalGateway,approvalPolicy,allowHumanApproval,...})` → `lib/tools/session-permission-wrapper.ts:656` `wrapWithSessionPermission` 的 `execute` 包装：:688 `evaluateToolSafetyPolicy`（硬安全，先于一切 resolver）→ :702 `resolveToolInvocationPermission` → :764 `classifySessionPermission({mode,toolName,params,...})` → allow :782 / deny :799 / review :807（`reviewToolApproval`，guard 槽模型审查）→ 需用户确认时 :896 `askForToolApproval`（走 confirmStore/approvalGateway，`DENY_ON_PROMPT` 时 :873 直接拒）→ :911 `executeWithInvocationRevalidation` → :459 `runWithPreparedInvocation(prepared,()=>tool.execute(...))`（prepared invocation 事实在执行前重校验 :429-457）→ `core/tool-invocation-gateway.ts:379` `invoke`（requires prepared invocation :381，generation/availability 复核 :390-464）→ :469 `target.executeCanonical(...)`

结论：模型观测收敛到 pi-sdk adapter 的 streamFunction 观测器（lib/pi-sdk/index.ts:86）+ model-call-observer-ext（server/index.ts:551）；工具执行收敛到 wrapWithSessionPermission → runWithPreparedInvocation → ToolInvocationGateway.invoke 单一网关。旁路：(a) ModelOperationResolver 的 embedding/rerank 直连（model-operation-client.ts，非 chat 链）；(b) `utility:call-text`/`model:sample-text` bus handler 直接 callText；(c) `core/llm-client.ts callText` 也被 chat.ts:3037 `engine.summarizeTitle` 等辅助任务使用（均已纳入 observed-model-call 观测，但不过 ToolInvocationGateway/permission wrapper）。

# 链 5：后台/定时任务链

1. 启动：`server/index.ts:557` `hub.initSchedulers()` → `hub/index.ts:367-378`（`Scheduler.start()` :371；ChannelRouter 仅 `engine.isChannelsEnabled()` 时 :374）
2. Heartbeat：`hub/scheduler.ts:126` `_startAgentHeartbeat` → `:163/:168` `onBeat/onJianBeat` → `:442` `_executeActivityForAgent`
3. Cron：`hub/scheduler.ts:194` `_startStudioCron` → `createCronScheduler({executeJob})` :200 → `:230` `_executeCronJob` → **:234 `runWithNewModelTrace({origin:"automation",refs:{automationId,studioId}},...)`（每次执行强制新 trace 根，切断外层 scope）** → `:308` `_executeActivityForAgent`
4. `hub/scheduler.ts:457` **`engine.executeIsolated(prompt,{agentId,persist,signal,permissionMode:"auto"|engine.getAutomationPermissionMode,approvalPolicy:"deny_on_prompt",allowHumanApproval:false})`** → `core/engine.ts:2304` → `core/session-coordinator.ts:8048` `executeIsolated` → 自建临时 SessionManager + **:8385 `createAgentSession`（同 pi-sdk adapter）** + :8283 附近 `this._d.buildTools`（executeIsolated 体内 line≈8283，即 sed 偏移 147）→ `:8657` `session.prompt(prompt)`；abort 经 `opts.signal` :8643-8647
5. Channel/DM（群频道手机会话）不走 executeIsolated：`hub/channel-router.ts:820` `runAgentPhoneSession` → `hub/agent-executor.ts:418` `runWithModelTraceRoot({origin:"phone_message",refs:{conversationId,conversationType}})` → :552 `createAgentSession` → :712 `session.prompt(round.text)`

结论：后台链走同一 `buildTools`+`wrapWithSessionPermission`（buildTools 出口统一包裹）与同一 pi-sdk createAgentSession，但**不经 hub.send 桌面路由、不经 chat.ts WS 门禁**；trace 根独立：automation=runWithNewModelTrace（scheduler.ts:234）、phone_message（agent-executor.ts:418）。

# 链 6：Bridge/Telegram/远程链

1. 启动：`server/index.ts:1312` `setImmediate(startBridgeManager({autoStart:true}))` → `server/index.ts:869-877` `new BridgeManager({engine,hub})`（lib/bridge/bridge-manager.ts:674）+ `hub.bridgeManager = manager` :874 → `bridge-manager.ts:846` `autoStart(agents)` 读 `agent.config.bridge` → `:906` `startPlatform` → `:914` `onMessage=(msg)=>this._handleMessage(platform,msg)` → `:118` `createTelegramAdapter({token,agentId,onMessage})`（lib/bridge/telegram-adapter.ts）
2. 入站鉴权（业务层）：`bridge-manager.ts:1097` `_handleMessage` → `:1127` **`_isOwner(platform,userId,agentId,...)`**（`:2745` → `isBridgeOwner({platform,chatType,userId,aliases,agent})`，对比 agent.bridge 配置的 owner 身份）→ `:1128` `bridgeRole = isGroup?"guest" : isOwner?"owner":"guest"`；owner 斜杠命令 :1140-1157 经 `engine.slashDispatcher.tryDispatch`
3. 分发：debounce/queue 后 `bridge-manager.ts:1995` **`this._hub.send(merged,{sessionKey,role:bridgeRole,...})`** → `hub/index.ts:213` 路由表：guest → `:326` `guestHandler.handle`（→ `hub/guest-handler.ts:46` `engine.executeExternalMessage(prefixed,...,{guest:true})`）；owner → `:330` `engine.executeExternalMessage`
4. `core/engine.ts:3103` → `core/bridge-session-manager.ts:1162` `executeExternalMessage` → **:1165 `runWithModelTraceRoot({origin:"bridge_message",refs:{conversationId:sessionKey}})`（独立 trace 根）** → :1218/1229 SessionManager.open/create（bridge 专属目录）→ **:1305 `createAgentSession`（同 adapter）** → :1640 `this._deps.buildTools`（allowHumanApproval:false :1648）→ `:1456` `session.prompt(promptText,promptOpts)`
5. 桌面会话接管（/rc attach）：`bridge-manager.ts:1957` `_flushAttachedDesktopSession` → `:2283` `this._hub.send(text,{sessionPath,...})` → 走 hub/index.ts:303 桌面提交路由（同链 1）

结论：Bridge 收敛到 hub.send + engine.executeExternalMessage，模型/工具层与桌面同源（同 createAgentSession、同 buildTools/permission wrapper）；差异点是独立 trace 根（bridge_message）和 bridge 目录的 session 管理，不属于权限旁路（guest 无工具、owner 工具仍过 wrapper）。

# 链 7：desktop/main.cjs 拉起 server 子进程

1. `desktop/main.cjs:168` `lingxiHome = resolveLingxiHome(process.env.LINGXI_HOME)`（shared/hana-runtime-paths.ts）
2. `desktop/main.cjs:1378` `artifactBootContext = await resolvePackagedArtifactBoot()`（定义 :1460：`artifactBoot.hasSeed(resourcesPath,platformArch)` :1463 → `prepareArtifactBoot({homeDir,resourcesPath,platformArch,keyset,channel})` :1479 首启解压 seed 到 `LINGXI_HOME/artifacts` 版本目录 → 返回 `{serverRoot: boot.server.versionDir}` :1564）。**生产 server 产物来源：electron-builder extraResources `package.json:188-189` `"from":"dist-server-artifact/${os}-${arch}/" → "to":"seed/"`**，即 Resources/seed → 激活到 LINGXI_HOME/artifacts
3. `desktop/main.cjs:1815` `_spawnServerOnce`：env 组装 :1821-1829（`LINGXI_HOME`、`LINGXI_SERVER_OWNER:desktop`、`LINGXI_SERVER_OWNER_PID`、`LINGXI_DESKTOP_*`），:1840 `delete serverEnv.PI_CODING_AGENT_DIR`，:1854 `LINGXI_RENDERER_DIST`；**packaged 分支 :1876-1894**：bin=`versionedServerRoot/hana-server`（win 为 .exe 裸 Node + 显式 bootstrap.js :1887-1889），`LINGXI_ROOT=versionedServerRoot` :1890、`LINGXI_SERVER_ENTRY=<root>/bundle/index.js` :1891（vite.config.server.js 的构建产物，由 server/bootstrap.ts:19 `import LINGXI_SERVER_ENTRY` 加载）、`LINGXI_CREATE_STARTUP_SESSION=0` :1894；**dev 分支 :1895-1911**：node + `server/bootstrap.ts`，`LINGXI_SERVER_ENTRY=<devRoot>/server/main-full.ts` :1908
4. Windows 守护：:1926-1944 guardian（lingxi-win-sandbox.exe）包装 spawn；**:1959 `spawn(launcherBin,launcherArgs,{detached,env:serverEnv,stdio:["pipe","pipe","pipe"]})`** → stdout/stderr 缓冲 :1982-1995 → `:1998` `pollServerInfo(serverInfoPath)`（轮询 server-info.json 拿 port/token）→ :2007 `serverProcess.unref()`

结论：spawn 单点在 main.cjs:1959；生产 server 代码 = Resources/seed（来自 dist-server-artifact）激活后的版本目录 bundle/index.js；dev = server/main-full.ts。子进程凭 LINGXI_HOME/LINGXI_SERVER_ENTRY/LINGXI_ROOT 环境变量定位一切。

# 链 8：取消链（用户停止）

1. UI：`desktop/src/react/components/InputArea.tsx:2234` `handleStop` → `:223-187` `createStopRequest({sessionId,sessionPath,streamId})` → **`:2245` `ws.send(JSON.stringify({type:'abort',...}))`**（客户端另在 `composer-send-coordinator.ts:533` `noteComposerUserAbort` 本地结算账本）
2. server：`server/routes/chat.ts:2367` `msg.type==="abort"` → :2377 streamId 不匹配当前流 → `abort_result/rejected(stale_stream)` :2378-2393；:2403 `agentReviewTurns.cancelByParent` → 未中则 **:2404 `hub.abort(abortPath,{reason})`**（`hub/index.ts:354` → `engine.abortSession`）→ 仍未中则 **:2409 `abortPendingDesktopSubmission(engine,{sessionId,sessionPath})`**（`core/desktop-session-submit.ts:186`：未进入 promptSession 的知识检索期提交，置 `abortedDesktopSessionSubmissions` 标记 + abort 检索 AbortController :198，submit 在 :605/:642 消费该标记抛 `InputCancelledBeforeAcceptance`）
3. `core/engine.ts:2156` `abortSession` → `core/session-coordinator.ts:5477` `abortSession`：分支一 :5479-5493 pre-prompt AbortController（promptSession :5220-5241 创建，`:5243` `signal.throwIfAborted()`）→ 分支二 :5498 `_forceReleaseStreamingSession(entry,sessionPath,reason)`
4. **`core/session-coordinator.ts:5936` `_forceReleaseStreamingSession`**：:5947 补发 `turn_end{aborted}` → :5951 从 `_sessions` 删除 entry → :5961 退订事件 → :5966 `session_status{aborted}` → **:5974 `session.abort?.()`（Pi SDK AgentSession abort，其内部 AbortController 终止 streamFunction 网络流与工具 signal）** → :5985 `session.dispose()`。工具层另有独立取消面：`core/engine.ts:4624/4631` `wrapWithSessionExecutionCancellation`（SessionExecutionRegistry.abortBySession，engine.ts:742-744）与 `tool-invocation-gateway.ts:465/:477/:490`（requestSignal.aborted → `EXECUTION_CANCELLED`）
5. 断线兜底：`server/routes/chat.ts:447-459` `scheduleDisconnectAbort` → 0 客户端超时 `engine.abortAllStreaming()`（chat.ts:457）

结论：取消链收敛到 `session-coordinator.ts:5477 abortSession` 单一入口，信号有两条：pre-prompt AbortController（:5220）与流中 `session.abort()`（:5974）；但存在三个补充取消面（pre-pending submission 标记 desktop-session-submit.ts:186、工具执行注册表 engine.ts:4624、WS 断连宽限 chat.ts:447），均不绕过 permission 体系。

# 总收敛判定

| 链 | 是否收敛 | 旁路证据 |
|---|---|---|
| 1 桌面 | 收敛：chat.ts:2256 /ws → hub.send(hub/index.ts:213) → promptSession | 无 |
| 2 CLI | 收敛（复用链 1 的 /ws + hub.send） | 无 |
| 3 装配 | 收敛：main-full.ts:24 唯一组合根；认证 index.ts:626 单点 | 公开路由白名单 isPublicHttpRoute（index.ts:619） |
| 4 agent | 模型观测收敛（pi-sdk/index.ts:86 + server/index.ts:551）；工具收敛（engine.ts:4579 wrapper → gateway invoke） | embedding/rerank（model-operation-resolver.ts:103）、utility:call-text/model:sample-text 直调 callText（server/index.ts:723-809）、summarizeTitle（chat.ts:3037） |
| 5 后台 | 执行层收敛（同 buildTools+createAgentSession），入口不经 hub.send | executeIsolated 独立入口（session-coordinator.ts:8048）、agent-executor.ts:552 独立 createAgentSession |
| 6 Bridge | 收敛到 hub.send + executeExternalMessage | 独立 trace 根 bridge_message（bridge-session-manager.ts:1165） |
| 7 拉起 | 单点 spawn（main.cjs:1959） | 无 |
| 8 取消 | 收敛到 abortSession（session-coordinator.ts:5477） | 三个补充取消面（见链 8 结论） |