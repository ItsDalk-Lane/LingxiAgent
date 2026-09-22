# LingxiAgent 产品功能清单原始证据（P00）

## 1. README 声明的产品功能与入口

**结论：README「功能特性」共 15 项能力声明，中英对照一致；入口为 Electron 桌面端 + 独立 Server + CLI + Mobile PWA 四类。**

- `README.md:40-70` 功能特性：记忆(42)、人格(44)、工具(46)、SKILLS 支持(48)、角色卡与技能包(50)、多 Agent(52)、书桌(54)、全屏媒体查看器(56)、会话管理(58)、定时任务与心跳(60)、安全沙盒(62)、内置扩展能力(64)、多平台接入(66)、移动端与 LAN 前端(68)、国际化 5 语言(70)
- `README.md:97-111` 架构目录：core/lib/server/cli/hub/desktop/shared/plugins/skills2set/scripts/tests
- `README.md:113-120` 引擎 Manager 列表、Hub 后台任务（心跳巡检、自动化/定时任务、频道路由、Agent 间通信、DM 路由）、SessionFile sidecar、Server 独立进程 + LINGXI_HOME 说明
- `README.md:145-172` 开发命令：`npm start` / `dev:renderer` / `start:vite` / `server` / `cli` / `test` / `typecheck`
- `README.md:174-176`、`README_EN.md:170-172` 打包发布：`pack` / `dist` / `dist:win` / `dist:linux`，publish 指向 ItsDalk-Lane/LingxiAgent
- `README_EN.md:35-65`（Features）、`README_EN.md:93-116`（Architecture）为英文对照版
- `package.json:8-10` `main: desktop/bootstrap.cjs`、`bin.lingxi: cli/entry.ts`

## 2. 桌面前端（desktop/src/react/）

**结论：无路由库，页面 = 7 个 HTML 构建入口 + `currentTab` 三主页面 + 4 个侧面板；设置共 19 个 tab；单一巨型 zustand store 由 28 个 slice 组成。**

视图与页面：
- `vite.config.ts:341-356` renderer 多入口：main(index.html)、mobile、settings、quick-chat、onboarding、browser-viewer、viewer-window（splash 独立构建，见 349-351 注释）
- `desktop/src/react/App.tsx:1-44` 根组件（titlebar + sidebar + 主区域 + overlays 编排）
- `desktop/src/react/components/app/AppPages.tsx:130-146` 主页面切换：`currentTab === 'chat' | 'map' | 'channels'`（ChatPage、ConversationMapPage、ChannelPage），chat 页挂 SideChatPanel
- `desktop/src/react/types.ts:450` `ActivePanel = 'activity' | 'automation' | 'bridge' | 'skills' | null`；`types.ts:453` 右栏 `RightWorkspaceTab = 'session-files' | 'workspace' | 'project-skills'`
- `desktop/src/react/stores/ui-slice.ts:22,71` currentTab 状态
- 其他表面：`onboarding/`（引导向导）、`quick-chat/QuickChatApp.tsx`（快捷聊天悬浮窗）、`browser-viewer/`（浏览器查看器窗口）、`mobile/`（PWA）、`splash/`

设置面板（`desktop/src/react/settings/SettingsContent.tsx:50-69` TAB_COMPONENTS，共 19 tab）：
agent、me、interface、keybindings、general、browser、work、skills、mcp、bridge、providers、models、usage、sharing、access、experiments、security、envdeps、about。子目录：`settings/tabs/{agent,bridge,mcp,models,observability,providers,skills}/`；overlays 含 CompiledMemoryViewer、WechatQrcodeOverlay 等（`SettingsContent.tsx:38-44`）

Store（`desktop/src/react/stores/index.ts:1-73`，28 个 slice 合成单一 `useStore`）：
connection、session、sessionProject、streaming、ui、agent、channel、desk、model、input、knowledgeReference、chat、chatFind、toast、preview、browser、context、automation、activity、agentActivity、terminal、bridge、selection、subagentPreview、computerOverlay、screenshot、sidebarUi、fileHistory、sideChat、conversationMap

## 3. 桌面主进程 IPC（desktop/main.cjs）

**结论：`main.cjs` 用 `wrapIpcHandler`/`wrapIpcBestEffortHandler` 注册 83 个 IPC channel，集中位于 5319-6324 行，覆盖更新、快速聊天、浏览器控制、文件 IO、技能查看器、系统对话框、窗口控制等桌面壳能力。**（`ipc-wrapper.cjs` 提供 wrapper，`main.cjs:31` 引入）

按域分组（channel 名 @ 行号）：
- OTA/更新：train-update-status@5319、train-fallback-notice-ack@5332、train-update-check@5339、train-update-apply@5354、get-update-digest-history@5391、release-check-latest@5417、get-pending-announcement@5427、ack-announcement@5430
- 连接/系统：get-server-port@5394、get-server-token@5395、get-app-version@5410、get-platform@6308、app:restart@5436、app-ready@6324、onboarding-complete@6298、debug-open-onboarding(-preview)@6280/6289、reload-main-window@6239
- 自启/常驻：get/set-auto-launch@5431/5432、get/set-keep-awake@5433/5434、show-notification@6249
- 快捷聊天：quick-chat-reload-shortcut@5435、keybindings:reload-global@5437、keybindings:test-register@5438、quick-chat-shortcut-status@5439、quick-chat-show/hide@5445/5446、quick-chat-resize@5464、quick-chat-open-session@5465
- 语音权限：speech-permission-status/request@5456/5460
- 窗口/设置窗口：open-settings@5476、window-minimize/maximize/close/is-maximized@6309-6319
- 浏览器查看器：open-browser-viewer@5479、browser-go-back/forward/reload/new-tab/switch-tab/close-tab@5512-5542、close-browser-viewer@5553、browser-emergency-stop@5556
- 媒体查看器窗口：spawn-viewer@5583、viewer-request-load@5628、viewer-close@5636、screenshot-render@6083
- 技能查看器：open-skill-viewer@5871、skill-viewer-list-files@5935、skill-viewer-read-file@5945
- 系统对话框/外壳：select-folder@5741、select-files@5754、select-skill@5767、open-folder@5959、show-in-finder@5986、trash-item@5991、open-file@6004、open-external@6012、get-avatar-path@5703、get-splash-info@5720
- 文件 IO：read-file@6023、read-file-snapshot@6030、write-file@6038、write-file-if-unchanged@6046、write-file-binary@6056、copy-file@6069、read-file-base64@6189、read-docx-html@6200、read-xlsx-html@6213、watch-file/unwatch-file@6134/6147、watch-workspace/unwatch-workspace@6170/6183
- 可观测性导出：observability-export:begin/write/end/abort@5804-5863
- 编辑命令：run-edit-command@5396

窗口创建（`main.cjs`）：splash@2436、quickChat@2696/2702、main@2888、settings@3056/3082、browserViewer@3192/3210、onboarding@4600、隐藏截图窗@4700、viewer@5588。

## 4. server/ HTTP/WS 路由注册清单

**结论：路由分三层——`composition/open-root.ts` 挂 40+ 开放路由、`composition/full-root.ts` 挂 6 个闭集产品路由、`server/index.ts` 内联少量系统路由 + 1 个原生 WS；业务端点在各 `server/routes/*.ts` 内用 `route.get/post/...` 定义。**

挂载点：
- `server/composition/open-root.ts:72-145` registerOpenRoutes：mobile-static@86、html-preview@87、chat REST@88 + chat WS@89、ws-auth@90、web-auth@91、access@97、sessions@101、conversation-map@102、session-collab@103、session-projects@104、models@105、config@106、memory-dream@107、knowledge@108、env-deps@109、upload@110、providers@111、agents@112、devices@113、studio-workspaces@114、skills@115、channels@116、dm@117、fs@118、preferences@119、input-drafts@120、settings-snapshot@121、experiments@125、bridge@126、auth@127、confirm@128、media@129、mcp@130、checkpoints@131、commands@132、resource-io@133、file-history@134、resources@135、usage@136、model-observability@138、speech-recognition@139、server-identity@140
- `server/composition/full-root.ts:26-34` registerClosedRoutes（闭集产品）：avatar@28、character-cards@29、cards@30、desk@31、diary@32、git-environment@34
- `server/index.ts` 内联：/api/health@1027、/api/log@1059、/api/plan-mode GET/POST@1071/1079、/api/session-permission-mode@1091/1123、/api/session-thinking-level@1099/1105、/api/shutdown@1155、mobile-workbench 挂载@1022、`/internal/browser` 原生 WebSocketServer@1165-1207（供 BrowserManager 的 raw ws）
- 入口决议：`server/index.ts:1021-1023` registerOpenRoutes → mobile-workbench → `root.registerClosedRoutes?.()`（open 构建不含闭集）

各域端点示例（文件:行）：
- 会话/消息：`server/routes/sessions.ts` 40+ 端点（/sessions 列表@772、search@856、messages@1382、turns/retry@1557、fork@1767、new@2045、pin@1013、rename@2512、browser/* @2474-2499 等）；`server/routes/chat.ts:428` createChatRoute → restRoute 仅 `POST /task/:taskId/abort`@2242 + `GET /ws`@2256（单一 WS 多路复用：聊天流、terminal 快照/尾部@2295-2302、资源事件经 `server/ws-scope.ts` scope 鉴权）
- 知识：`server/routes/knowledge.ts:225-629`（notebooks CRUD、sources、import-directory@437、refresh/reingest@524/551、citations@598）
- Bridge 媒体：`server/routes/bridge.ts:676` `GET /bridge/media/:token`
- 移动端：`server/routes/mobile-workbench.ts:32-156`（bootstrap、files、search、content、actions、upload）

## 5. hub/ 后台任务

**结论：Hub 是同进程消息调度中枢，由 5 个组件构成（EventBus/ChannelRouter/GuestHandler/Scheduler/DmRouter）+ AgentPhoneActivityStore；后台活动类型为 heartbeat、cron、channel（agent phone）、DM 回复，另有 fresh-compact 维护器。**

- 构造与注入：`hub/index.ts:107-139`（组件 new 于 109-116；`engine.setHubCallbacks`@120-131 注入 scheduler/dmRouter/channelRouter/eventBus 及 trigger 回调；`engine.setEventBus`@134；构造时 `_setupSessionHandlers`@137 + `_setupDmHandler`@138）
- 统一消息入口 `hub.send()`：`hub/index.ts:213-349`，路由表@299-343 四条：桌面 owner（submitDesktopSessionMessage）→ Bridge guest（GuestHandler）→ Bridge owner（executeExternalMessage）→ ephemeral 隔离执行（cron/heartbeat/channel，`executeIsolated` + `approvalPolicy:"deny_on_prompt"`@334-341）
- 调度器启动：`hub/index.ts:367-378` `initSchedulers()`（由 server/index.ts 在 engine.init 后调用）：`scheduler.start()`@371；频道总开关开启时 `channelRouter.start()` + `setupPostHandler()`@374-376
- Scheduler：`hub/scheduler.ts:41` class；每 agent heartbeat（`lib/desk/heartbeat.ts`）@127-178；Studio cron（`lib/desk/cron-scheduler.ts`）@73-90；`_executeActivityForAgent`@442 以 type=`heartbeat`/`cron` 执行；FreshCompactMaintainer 于 start()@83 启动（`hub/fresh-compact-maintainer.ts`）
- 频道路由：`hub/channel-router.ts:46` class ChannelRouter，start()@365，ticker@383；群聊调度用 `runAgentPhoneSession`（`hub/agent-executor.ts:400`）于 channel-router.ts:820
- DM 路由：`hub/dm-router.ts:49` class DmRouter；`hub/index.ts:1041-1048` `_setupDmHandler` 为所有 agent 注入 `setDmSentHandler → dmRouter.handleNewDm`；dm-router.ts:239 用 runAgentPhoneSession 生成回复
- Agent 间通信/插件总线 handler：`hub/index.ts:443-1030` bus.handle 注册 session:create/get/update/send/abort/history/list、agent:list/profile/create/update/config/update-config、provider:credentials/models-by-type/media-providers 等（供插件调用）
- server 侧总线上更多 handler：`server/index.ts` taskRegistry@693、loop@930、plan-gate@932、autolearn@965、goal@992、deferred-result@486

## 6. cli/ 命令入口

**结论：CLI（`bin.lingxi` → `cli/entry.ts`）共 7 个一级命令 + 2 组子命令，server-first（chat/continue 可自动拉起本地 server）。**

- `cli/args.ts:1-3` COMMANDS = serve, status, sessions, continue, chat, bundle(pull|status), data(diagnose|checkpoints|restore), help；CHANNELS = stable|beta
- `cli/entry.ts:20-83` main() 分发：serve@31（spawnServerForeground）、bundle@40、data@48、status/sessions/continue/chat@66-78（需 server 连接，`startLocalServerAndWait` 自动拉起@60）
- `cli/chat.ts`（终端聊天）、`cli/client.ts`（HTTP/WS 客户端）、`cli/local-server.ts`（连接解析）、`cli/server-runner.ts`、`cli/bundle.ts`（发布 train 拉取）、`cli/data.ts`（数据检查点恢复）

## 7. plugins/ 内置系统插件

**结论：4 个内置系统插件，全部 hidden（不在插件市场显示），由 `core/engine.ts:3750` 从 `productDir/../plugins` 加载，`scripts/build-server.mjs:146-149` 打包进 server 产物。**

- `plugins/beautify/manifest.json`：id=beautify，trust=full-access，capabilities=[resource.read, resource.write]，onStartup；工具 list-capabilities/create-cover/apply-cover-candidate/get-cover-style-guide/get-html-style-guide（`plugins/beautify/tools/`）——README「封面美化」
- `plugins/jimeng-cli/manifest.json`：id=jimeng-cli，trust=full-access，onStartup；即梦 dreamina 图片/视频生成（`providers/jimeng-cli.ts`、`adapters/dreamina.ts`）
- `plugins/media/manifest.json`：id=media，trust=full-access；generate-image/generate-video/generate-speech/describe-options/get-guide（`plugins/media/tools/`）——README「生图/生视频」
- `plugins/office/manifest.json`：id=office，trust=restricted，capabilities=[office.read, office.html_to_pdf, resource.read, resource.materialize]；read-document/html-to-pdf（经桌面 Chromium 打印引擎）

## 8. skills2set/ 内置技能

**结论：5 个目录，4 个为完整技能（含 SKILL.md），首启由 `core/first-run.ts:107-108,248` 同步到 `~/.lingxi/skills/`；lingxi-plugin-creator 是残留（仅 pycache）。**

- `skills2set/character-creator/`：角色创建向导，产出角色卡 zip（SKILL.md + references/card-format、anti-slop、character-craft）
- `skills2set/quiet-musing/`：深度推理协议（复杂任务结构化推理框架）
- `skills2set/skill-creator/`：技能创建/评估/打包（含 Python 脚本与 eval-viewer，源自官方 skill-creator）
- `skills2set/user-guide/`：灵犀用户说明书（"怎么用/功能介绍"触发）
- `skills2set/lingxi-plugin-creator/`：**残留**——仅有 `scripts/__pycache__/create_lingxi_plugin.cpython-314.pyc` 与 `create_hana_plugin.cpython-314.pyc` 两个编译产物，无 SKILL.md，不构成可装载技能

## 9. 构建入口

**结论：6 份 vite 配置 + 1 份 electron-builder（内嵌 package.json）；产物为双 artifact 管线（壳自持 splash + 首启解压 renderer/server seed）。**

- `package.json` scripts：start/start:dev/start:vite（Electron，经 `scripts/launch.js`）、dev:web、build:renderer|splash|theme|preload|main、build:client（五合一）、build:server（`scripts/build-server.mjs`）、build:server:open、pack / dist / dist:win / dist:linux、fetch:bundled-bins、verify:seed-kit、pack:server:standalone、release:preflight、test / test:knowledge-platform-smoke、postinstall（patch-pi-sdk + usearch native）
- `vite.config.ts:341-356` renderer 7 入口（见第 2 节）；CSP 注入 `vite.csp-profiles.ts`
- `vite.config.main.js`：主进程 → `desktop/main.bundle.cjs`；含签名 keyset 内联与 OTA dev-bypass 折叠（18-50 注释）
- `vite.config.preload.js`：preload 单文件 bundle（sandbox 兼容）
- `vite.config.splash.ts`：splash 独立壳自持表面 → `desktop/dist-splash/`
- `vite.config.theme.js`：`desktop/src/shared/theme.ts` → IIFE `dist-renderer/lib/theme.js`
- `vite.config.server.js:12-27`：server → `dist-server-bundle/index.js`，entry 默认 `server/main-full.ts`，`LINGXI_SERVER_BUNDLE_ENTRY` 可切 open 构成；原生依赖 external
- electron-builder（package.json "build"）：appId com.lingxi.app、asar、extraResources：`dist-server-artifact→seed/`、bundled-bin、screenshot-themes、mac 额外 computer-use+speech、win 额外 mingit+sandbox；mac dmg+zip+hardenedRuntime+entitlements（麦克风/语音识别描述）、win nsis、linux AppImage+deb；afterSign `scripts/notarize.cjs`；publish github ItsDalk-Lane/LingxiAgent

## 10. 已撤回/残留痕迹

**结论：P00 任务书明文排除三类已撤回方案（专用子代理目录/强制派发、独立知识研究、本地模型管理子系统）；现状为：subagent 保留为工具+可选实验、知识研究仅剩 schema/类型与归档文档、本地模型子系统代码已不存在、另发现 lingxi-plugin-creator 技能目录残留。**

- 排除依据：`Lingxi_Refactor_Taskbooks_2026-09-21/P00_基线、范围与验收地图.md:78`（硬排除原文）、`:213`（P00-A04 验收）、`01_通用执行约束.md:11`、`91_执行提示词.md:16`、`P04:21`、`P08:160`
- subagent（现役能力，保留）：
  - 工具：`lib/tools/subagent-tool.ts` + `lib/tools/subagent-tool-policy.ts`（intercept/strip 双策略、权限衰减 #1614）
  - 支撑：`lib/subagent-thread-store.ts`、`lib/subagent-run-store.ts`、`lib/subagent-executor-metadata.ts`；UI：`desktop/src/react/stores/subagent-preview-slice.ts`、`quick-chat/QuickChatApp.tsx:767`
  - 「强制派发」的现役替代 = 可选实验 `subagent.proactive_delegation`：`lib/experiments/registry.ts:15,121-145`（beta、默认 false），消费点 `core/agent.ts:933,1756`，设置入口 `settings/tabs/ExperimentsTab.tsx:21,446`
- 独立知识研究（withdrawn，残留分级）：
  - 仅文档残留：`docs/archives/knowledge-retrieval-research/`（任务书、BASELINE、PROGRESS、IMPLEMENTATION/TEST/PERFORMANCE 报告共 6 份）；`docs/archives/README.md` 归档说明
  - 仍在生产代码路径（数据兼容，无启动入口）：`lib/knowledge/knowledge-store.ts:1021-1125` research_runs/research_jobs 等 9+ 张表仍在建表；`lib/knowledge/types.ts:5-162` KnowledgeResearchRun/Action 类型；`lib/knowledge/knowledge-manager.ts:587,692,785` 仅以 "research-referenced" 做删除保护。无任何 route/engine 启动 research run（仅测试 `tests/knowledge-research-recovery.test.ts` 引用）
- 本地模型管理子系统（withdrawn，已移除）：
  - 生产代码无 "local model/LocalModel" 管理痕迹（desktop/server/core/lib 全域 grep 为空）
  - 现存 Ollama 是「模型接入」而非管理子系统：`lib/providers/ollama.ts`、`core/provider-compat/ollama.ts`、`desktop/src/react/utils/provider-presets.ts`、`server/routes/providers.ts`
- 其他残留：`skills2set/lingxi-plugin-creator/` 仅 `__pycache__` 两个 pyc（见第 8 节）；`desktop/main.cjs:1043,1071` train-update-apply 为 OTA 发布列车机制（现役，非残留）

## 11. Bridge / Telegram / 飞书等外部通道接线

**结论：5 平台 adapter（Telegram/飞书/钉钉/QQ/微信）由 `lib/bridge/bridge-manager.ts` 统一注册，manager 在 server 侧懒加载创建并挂到 hub。**

- Telegram：`lib/bridge/telegram-adapter.ts:8` `import TelegramBot from "node-telegram-bot-api"`；另 `server/routes/bridge.ts:825` 动态 import（连接诊断）
- 飞书/Lark：`lib/bridge/feishu-adapter.ts:8` `import * as lark from "@larksuiteoapi/node-sdk"`（WSClient 长连接，见 273 行 OpenAPI 返回处理）
- Adapter 注册表：`lib/bridge/bridge-manager.ts:11-16` import 五个 adapter（telegram/feishu/dingtalk/qq/wechat），`:116` 注释「新增平台只需在此注册」；`class BridgeManager`@674，`spec.create(...)` 实例化@924
- 启动接线：`server/index.ts:860-889` `startBridgeManager()` 动态 import BridgeManager、`hub.bridgeManager = manager`@874；暴露 `bridgeManagerRef`@891 供 routes/bridge.ts 使用
- 平台支撑文件：`lib/bridge/` 下 dingtalk-api、qq-local-upload（QQ 分片上传）、wechat-ilink-media-crypto、wechat-login、media-delivery-service、interaction/receipt/streaming-capabilities 等

## 功能 → 主要入口文件汇总表

| 功能 | 用户动作 | 入口文件 | 备注 |
|---|---|---|---|
| 桌面聊天主界面 | 打开 App、发消息 | desktop/src/react/App.tsx、components/app/AppPages.tsx（chat tab） | WS `/ws`（server/routes/chat.ts:2256） |
| 会话管理/搜索/归档 | 侧栏搜索、置顶、归档 | server/routes/sessions.ts（40+ 端点） | 前端 session-slice.ts |
| 频道群聊/多 Agent | 切到 channels tab | desktop/src/react/components/ChannelsPanel.tsx + hub/channel-router.ts | agent phone 会话 hub/agent-executor.ts:400 |
| DM 私聊（Agent 间） | Agent 互发私信 | hub/dm-router.ts、hub/index.ts:1041 | lib/tools/dm-tool.ts |
| 书桌/笺 | 拖文件、写便签 | server/routes/desk.ts（full-root.ts:31 闭集） | 前端 desk-slice.ts |
| 定时任务/心跳 | 设置 Cron、巡检 | hub/scheduler.ts（heartbeat/cron） | activity 类型见 :442 |
| 子代理（保留能力） | Agent 派发子任务 | lib/tools/subagent-tool.ts + subagent-tool-policy.ts | 实验开关 experiments/registry.ts:15 |
| 技能系统 | 安装/创建/审核技能 | server/routes/skills.ts、skills2set/（首启同步 core/first-run.ts:107） | SkillsPanel + SkillsTab |
| 角色卡/技能包导入导出 | 导入/导出 zip | server/routes/character-cards.ts、cards.ts；lib/character-cards/、lib/skill-bundles/ | 闭集路由 full-root.ts:29-30 |
| 知识库 | 建笔记本、导入目录、引用 | server/routes/knowledge.ts、lib/knowledge/ | research 表为兼容残留 |
| 模型/供应商配置 | 设置→Providers/Models | server/routes/providers.ts、models.ts、provider-credentials.ts | 前端 tabs/ProvidersTab、ModelsTab；Ollama=接入非管理 |
| Bridge 多平台 | 绑定 TG/飞书/QQ/微信/钉钉 | lib/bridge/bridge-manager.ts + 5 个 adapter；server/index.ts:860 启动 | 设置→Bridge（BridgeTab） |
| 移动端 PWA/LAN | 手机访问 `/mobile/` | server/routes/mobile-static.ts、mobile-workbench.ts；desktop/src/mobile.html | 设备密钥/本地账号 routes/devices.ts、web-auth.ts |
| CLI | `lingxi status/sessions/chat/continue` | cli/entry.ts | `lingxi serve` 本地拉起 |
| 快捷聊天 | 全局快捷键唤起 | desktop/main.cjs:2696 + desktop/src/quick-chat.html | IPC quick-chat-* @5435-5465 |
| 浏览器工具/查看器 | Agent 浏览网页、用户开查看器 | lib/tools/browser-tool.ts、server/index.ts:1165 `/internal/browser`、main.cjs:3192 | BrowserCard/BrowserTab |
| 计算机使用/截图 | Agent 操作屏幕 | lib/tools/computer-use-tool.ts、main.cjs screenshot-render@6083 | dist-computer-use extraResources |
| 内置插件四件套 | 生图/生视频/语音/Office/封面 | plugins/{media,jimeng-cli,office,beautify}/manifest.json | core/engine.ts:3750 加载 |
| OTA 更新 | 检查/应用更新 | desktop/main.cjs:5319-5354、src/shared/train-update-apply.cjs:1071 | release-digest.v1/v2.json |
| 观测/用量 | 设置→Usage/导出 | server/routes/usage.ts、model-observability.ts；main.cjs observability-export:*@5804-5863 | UsageTab + observability/ |
| 自动学习 | Agent 自主学技能 | server/autolearn-handler.ts@server/index.ts:965、lib/autolearn/ | hub 总线 handler |
| 记忆/梦境整理 | 记忆固化、dream | server/routes/memory-dream.ts、lib/memory/ | README 功能第 1 项 |
| 残留（不恢复） | — | 知识研究：lib/knowledge/knowledge-store.ts:1021 起表结构 + docs/archives/knowledge-retrieval-research/；本地模型子系统：无代码；强制派发：仅剩实验开关；skills2set/lingxi-plugin-creator/ 仅 pycache | P00 任务书 :78 硬排除 |