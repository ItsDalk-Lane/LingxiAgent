# ENTRYPOINT_MATRIX — 真实入口与调用链（P00-T03）

版本：1.0｜证据基线：HEAD `92c6646c5`。逐跳证据（文件:行）完整版见 `artifacts/refactor-2026/P00/logs/agent-callsite-raw.md`。
本表每条链均已核对到真实调用点，无"应该经过"推断。

## 1. 入口清单与收敛判定

| 入口ID | 入口 | 首跳证据 | 收敛点 | 收敛判定 |
|---|---|---|---|---|
| E-DESKTOP | 桌面用户输入 | `InputArea.tsx:1814` submitEditorMessage → 发送租约（`InputArea.tsx:2018` tryAcquireSendLease）→ `composer-send.ts:577` ws.send(prompt/interject) | WS `/ws`（`server/routes/chat.ts:2256`）→ `hub.send`（`hub/index.ts:213` 路由表 :299-343）→ `submitDesktopSessionMessage`（`core/desktop-session-submit.ts:385`）→ `promptSession`（`core/session-coordinator.ts:5176`）→ `entry.session.prompt`（:5278，Pi SDK） | **收敛**，无旁路。IPC 只传端口/token（main.cjs:5394-5395），业务消息不走 IPC |
| E-CLI | CLI（chat/continue） | `cli/entry.ts:64` resolveConnection（`cli/local-server.ts:70` 读 server-info.json）→ `cli/chat.ts:347` ws.send | 同 E-DESKTOP 第 5 跳起 | **收敛**（复用同一 /ws 与 hub.send） |
| E-BRIDGE | Bridge 外部消息（TG/飞书/钉钉/QQ/微信） | adapter onMessage（`lib/bridge/bridge-manager.ts:914`）→ `_handleMessage`(:1097) → owner 判定 `isBridgeOwner`(:2745) → `:1995 hub.send({sessionKey, role})` | guest→`guestHandler`→`engine.executeExternalMessage`；owner→同；`core/bridge-session-manager.ts:1165` runWithModelTraceRoot(origin=bridge_message) → `:1305 createAgentSession` → `:1456 session.prompt` | **收敛**（模型/工具层同源）；独立 trace 根 |
| E-CRON | 定时/心跳 | `hub/scheduler.ts:126/:194` → `:234 runWithNewModelTrace(origin=automation)` → `:457 engine.executeIsolated` | `core/session-coordinator.ts:8048` executeIsolated → `:8385 createAgentSession` → `:8657 session.prompt` | 执行层收敛；**不经 hub.send/chat 门禁**（设计如此：ephemeral 隔离，approvalPolicy=deny_on_prompt） |
| E-CHANNEL | 频道群聊/DM | `hub/channel-router.ts:820` / `hub/dm-router.ts:239` → `runAgentPhoneSession` | `hub/agent-executor.ts:418` runWithModelTraceRoot(origin=phone_message) → `:552 createAgentSession` → `:712 session.prompt` | 执行层收敛；独立入口不经 hub.send |
| E-SLASH | 斜杠命令（bridge owner） | `bridge-manager.ts:1140` → `engine.slashDispatcher.tryDispatch` | 命令分发器 | 独立面，不产生模型 turn（除非命令内部再走上述链） |
| E-WS-AUTH | WS/HTTP 鉴权 | `server/index.ts:589-637` 中间件 → `:626 resolveHttpRequestPrincipal`（`server/http/request-principal.ts`） | `core/server-auth.ts:35-90`（loopback token→local_user principal；device credential；web cookie 后备） | 单点鉴权；例外=`isPublicHttpRoute`（index.ts:619 白名单）与 resource ticket（:614） |

## 2. 服务器装配顺序（组合根）

`server/main-full.ts:17-24`（唯一全量组合入口：startServer + registerClosedRoutes + builtinMediaAdapters）→ `server/index.ts`：
同宅互斥闸 :273-291 → 数据 epoch 闸 :297-349 → token :351 → 监听 :406 → ensureFirstRun :418 → **Engine :436**（唯一 `new LingxiEngine`）→ **Hub :476**（构造内注入 EventBus/ChannelRouter/GuestHandler/Scheduler/DmRouter）→ 扩展注册 :493-551 → initPlugins :554 → initSchedulers :557 → 认证 :576-581 → 路由挂载 :1021-1023（open-root 必挂 + closed 静态追加）→ server-info.json :1280-1300 → 桥启动 :1312。

不存在独立 EngineManager；Manager（AgentManager/SessionCoordinator/ConfigCoordinator/ModelOperationResolver 等）都在 LingxiEngine 构造/init 内装配（engine.ts:642/664/768/789/804/817）。

## 3. agent 执行链（模型与工具）

- **Pi 会话创建唯一入口**：`core/session-coordinator.ts:2214-2230` 组装 sessionOpts → `lib/pi-sdk/index.ts:78` createAgentSession → `:84-89` 安装 tool-outcome-adapter/assistant-stream-guard/**model-call-stream-observer**/trace-ingress/desktop-input-commit/todo-reminder。
- **聊天模型**：sessionOpts.model（session-coordinator.ts:2227 effectiveModel，由 model-manager 从 Pi ModelRuntime 解析）。`core/model-operation-resolver.ts` 只管 embedding/rerank 等非 chat 操作（engine.ts:2511/2530/2599/2731 创建 resolver）。
- **工具执行唯一网关**：`core/engine.ts:4119` new ToolInvocationGateway → `:4579/:4598` wrapWithSessionPermission（`lib/tools/session-permission-wrapper.ts:656`：安全策略评估:688 → 权限解析:702 → 分类:764 → allow/deny/review；review 经 confirmStore/approvalGateway）→ `:459 runWithPreparedInvocation` → `core/tool-invocation-gateway.ts:379 invoke`（prepared 校验/generation 复核）→ `:469 target.executeCanonical`。
- **AST 边界守卫**：`scripts/check-tool-invocation-boundaries.mjs`（mcpCallTool 白名单 2 文件、pluginExecuteTool 1、canonicalTargetExecutor=网关）——已接线 npm script 与 CI。

### 已登记旁路（合法但不走 ToolInvocationGateway，观测已覆盖）

> P04 注记（2026-09-22）：下表三条旁路已由 P04 全量复核——观测包装、凭证道与用量归属见 [P04/MODEL_CALLSITE_MATRIX.json](../P04/MODEL_CALLSITE_MATRIX.json)；embedding/rerank 另有真实 loopback HTTP 测试（tests/model-observability-e2e-utility.test.ts P04-A15）。无未解释直调。

| 旁路 | 位置 | 性质 |
|---|---|---|
| embedding/rerank 模型调用 | `core/model-operation-resolver.ts:103` → model-operation-client | 非 chat 链；主责 P04 核查观测完备性 |
| utility:call-text / model:sample-text 总线直调 callText | `server/index.ts:723-809`（独立 trace 根 origin=plugin） | 辅助文本调用；主责 P04 |
| summarizeTitle 等辅助任务 | `server/routes/chat.ts:3037` engine.summarizeTitle | 会话标题生成；已纳入观测 |

## 4. 桌面壳拉起 server

`desktop/main.cjs:168` resolveLingxiHome → `:1460` resolvePackagedArtifactBoot（生产 seed 来自 electron-builder extraResources dist-server-artifact→Resources/seed，首启解压到 LINGXI_HOME/artifacts 版本目录）→ `:1815` _spawnServerOnce（env：LINGXI_HOME/LINGXI_SERVER_OWNER/LINGXI_ROOT/LINGXI_SERVER_ENTRY；packaged=:1876-1894 bundle/index.js，dev=:1895-1911 main-full.ts）→ `:1959` spawn（单点）→ `:1998` pollServerInfo。

## 5. 取消链

UI `InputArea.tsx:2234` handleStop → ws `{type:"abort"}` → `server/routes/chat.ts:2367` → 依次：stale_stream 判定(:2377) → `agentReviewTurns.cancelByParent`(:2403) → `hub.abort`(:2404→engine.abortSession) → `abortPendingDesktopSubmission`(:2409，知识检索期取消标记 `desktop-session-submit.ts:186`) → `core/session-coordinator.ts:5477` abortSession（分支一 pre-prompt AbortController :5479-5493；分支二 `_forceReleaseStreamingSession` :5936：补 turn_end→退订→session_status→`session.abort()` :5974→dispose）。补充取消面：工具执行注册表（engine.ts:4624 wrapWithSessionExecutionCancellation）、WS 断连宽限（chat.ts:447 scheduleDisconnectAbort→abortAllStreaming）。

## 6. 调用链结论

八条链全部收敛到：模型=Pi adapter 唯一入口+stream observer 观测；工具=permission wrapper→prepared invocation→gateway 单网关；取消=abortSession 单入口（三个补充面均不绕权限体系）。桌面/CLI/Bridge 共用 hub.send 路由表；后台（cron/channel/dm）独立入口但执行层同源、trace 强制新根。
