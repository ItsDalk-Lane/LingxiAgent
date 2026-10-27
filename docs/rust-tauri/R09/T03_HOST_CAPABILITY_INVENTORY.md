# R09-T03 / R09-T06 宿主能力清点（Electron DesktopHost → Tauri 目标映射）

- 生成方式：只读源码清点 + R00 冻结账/任务书交叉核对；**本轮未修改任何生产代码/配置，未执行任何 E2E 或系统动作**。
- 基点：分支 `codex/rust-tauri-migration`，HEAD `aac8cb9fe6bb7bfa7a41d75528221136bfe55e50`（`git rev-parse HEAD` 实查；与 R09 START_RECEIPT 一致）。
- 机器可读权威版本：同目录 `T03_HOST_CAPABILITY_INVENTORY.json`（本 MD 为导读；字段细节以 JSON 为准）。
- 交付边界：本文件只描述「现有 Electron 能力 → Tauri 目标」映射与证据计划，不构成实现、不构成验收通过。

## 0. 结论摘要

- 能力条目 **50** 项，覆盖 PlatformApi **110** 个成员（110 个已映射，无遗漏），另含 3 个 preload-only 桥（`screenshotRender`、`debugOpenOnboarding`、`debugOpenOnboardingPreview`）与 1 个 PlatformApi-only 成员 `closeBrowser`（preload 未暴露、Electron 无实现、无消费者）。
- Tauri 目标分类：custom-command **31** 项、plugin **18** 项、drop-with-reason **1** 项；无 unsupported-platform 条目，平台限定风险写在平台矩阵与 platformNotes。
- 需要新增/收口的自定义命令 **91** 条；其中 **20** 条在 R06 `desktop/src-tauri` 已有可复用实现/模板，**71** 条为新增。
- 46 个 T03/T07 共享叶中，**30** 个映射到宿主能力；其余为 server/React 叶，理由逐条登记（见 §9）。
- 关键缺口：通知点击聚焦、页截图、卡片 iframe 导航 guard、Tauri 插件版本重锁、release 调试命令门控、keep-awake/drag-out 平台实现、capability 叠加审计（见 §10）。

### 0.1 本清单直接服务的验收场景（R09 16 基础场景中的 4 项；全部 16 项见 JSON `generatedFrom.r09BaselineScenarios`）

| 场景 | 任务 | 通过条件（摘要） | 证据（任务书） | 本清单对应 |
|---|---|---|---|---|
| R09-A05 用户动作真实生效 | T03 | 托盘/快捷键/通知/拖放/文件选择行为与现役一致，非 mock 回调 | 系统级录屏和自动测试（各目标 OS） | §4 shell/dialogs 组 + §11 A05 策略 |
| R09-A06 不可信参数无法执行任意命令 | T03 | 畸形路径/URL/命令样式参数按类型/scope 拒绝，不转任意 shell | IPC 负向测试 | §7-5、§11 A06 策略、各命令 validationNeeds |
| R09-A11 capability 叠加不意外扩权 | T06 | 同 webview 命中多 capability 后最终权限与白名单一致、越权集合为空 | 配置展开与运行负向测试 | §7-1/7-2、`security:capability-sets/command-acl` |
| R09-A12 生产无测试控制面 | T06 | release 包不可达测试 WebDriver/任意 Rust 执行接口 | 产物级负向探针 | §7-4、`security:release-no-test-control` |

## 1. 方法与判据

1. 从 `desktop/preload.cjs` 逐项提取 contextBridge 暴露面（112 key），与 `desktop/src/react/types.ts` 的 `PlatformApi`（110 成员；`quickChatShow`/`quickChatHide` 同行需手工计数）做集合差：交集 109，preload-only 3（`screenshotRender`、`debugOpenOnboarding`、`debugOpenOnboardingPreview`），PlatformApi-only 1（`closeBrowser`）。
2. 从 `desktop/main.cjs` 逐 `wrapIpcHandler`/`wrapIpcBestEffortHandler`/`wrapIpcOn` 注册点反推实现函数与真实行号；handler 分派在 `desktop/ipc-wrapper.cjs:18/34/48`。行号一律以当前 HEAD 实查为准（R00 ENTRYPOINTS/ACCEPTANCE_MAP 行号为历史基线，已过期，见 OQ-12）。
3. 消费者按「React 生产源码中真实调用点」取 `file:line`（`usePlatform()` 为主要入口，`desktop/src/react/hooks/use-platform.ts:15`；`window.hana` 仅边界白名单文件直读）。找不到生产调用就写 `no-consumer-found`，并注明仅有测试/主进程内部入口。
4. Tauri 目标按官方插件/核心 API 事实映射；R01-T06 spike 实测（`docs/rust-tauri/R01/SHELL_CAPABILITY_MATRIX.json`）作为可用性依据；R06 `desktop/src-tauri` 的 26 命令/3 capability/CSP 作为写法模板。所有「需 spike/未验证」显式标注，不臆造 API。
5. 46 叶来自 `docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json` 中 `execution_task_ids` 含 R09-T03 或 R09-T07 的 `supplemental_scenarios`（两者完全重合，按叶 ID 去重 = 46）；16 个基础场景来自 `ACCEPTANCE_MAP.json`（`kind=base & stage_id=R09`）。

## 2. 读过的文件清单（真实工作区）

| 文件 | 行数 | 用途 |
|---|---|---|
| `AGENTS.md` | n/a | 项目规则（工作区本地，gitignored）；红线与验证边界 |
| `docs/rust-tauri/R09/START_RECEIPT.md` | 9 | R09 开工事实/隔离/环境边界 |
| `docs/rust-tauri/R09/BLOCKED.md` | 19 | 平台与签名/权限阻塞登记 |
| `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R09_Tauri桌面宿主、原生能力与跨平台集成.md` | 395 | T03/T06 规格与 A05/A06/A11/A12 验收（目录未跟踪） |
| `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/02_目标架构与强制契约.md` | 155 | §9 宿主与远端安全契约、§10 不算完成 |
| `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/03_功能与所有权矩阵.md` | 46 | D23 桌面体验/D15 系统操作目标所有权 |
| `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/05_验收与性能协议.md` | 120 | 四层证据、平台矩阵与证据写法 |
| `desktop/preload.cjs` | 217 | contextBridge 全部暴露项（112 key） |
| `desktop/main.cjs` | 7003 | IPC handler、窗口/托盘/快捷键/通知/外链/更新实现 |
| `desktop/ipc-wrapper.cjs` | 60 | wrapIpcHandler/BestEffort/On 统一错误与日志 |
| `desktop/src/react/types.ts` | 702 | PlatformApi 110 成员声明与返回类型 |
| `desktop/src/react/host/desktop-host.ts` | 129 | DesktopHost 注册表/能力解释/桥身份 |
| `desktop/src/react/host/electron-desktop-host.ts` | 21 | Electron 宿主（全能力） |
| `desktop/src/react/host/web-desktop-host.ts` | 252 | Web 受限宿主（受限理由表） |
| `desktop/src/react/hooks/use-platform.ts` | 17 | 业务统一取宿主 API 的入口 |
| `desktop/src/react/mobile/mobile-platform.ts` | 120 | 移动宿主（缺失即受限）模式参考 |
| `desktop/src/shared/single-instance-lock.cjs` | 88 | 单实例锁按 LINGXI_HOME 命名空间 |
| `desktop/src/shared/splash-avatar-path.cjs` | 149 | splash 头像/资料读取与发送者白名单 |
| `desktop/src/shared/card-frame-navigation.cjs` | 27 | 卡片 iframe 导航 guard（will-frame-navigate） |
| `desktop/src/shared/path-to-file-url.cjs` | 43 | 本地路径→file URL 纯转换 |
| `desktop/src/shared/window-state.cjs` | 121 | 窗口状态 sanitize（多显示器） |
| `desktop/src/shared/desktop-notification-policy.cjs` | 18 | 通知 focus 抑制策略 |
| `desktop/src/shared/desktop-notification-delivery.cjs` | 31 | 通知真实显示回执 |
| `desktop/src/shared/trash-item-path.cjs` | 21 | trash 路径校验 |
| `desktop/src/shared/github-release-check.cjs` | 133 | GitHub Releases 检查 |
| `desktop/src/shared/update-digest-history.cjs` | 192 | 更新历史在线/包内回退 |
| `desktop/login-item-settings.cjs` | 81 | 登录项状态/设置/openedAtLogin |
| `desktop/keep-awake.cjs` | 72 | powerSaveBlocker 防休眠管理 |
| `desktop/auto-updater.cjs` | 851 | electron-updater IPC/状态/安装 |
| `desktop/speech-permissions.cjs` | 231 | Speech 授权状态机与原生桥 |
| `desktop/quick-chat-session-handoff.cjs` | 83 | 快聊→主窗会话交接协议 |
| `desktop/file-text-io.cjs` | 62 | 文本快照/CAS 写入 |
| `desktop/file-watch-registry.cjs` | 143 | 文件监听共享订阅 |
| `desktop/workspace-watch-registry.cjs` | 205 | 工作区监听与忽略规则 |
| `desktop/src/viewer-window-entry.tsx` | 197 | Viewer 窗口拉取载荷/只读展示 |
| `desktop/src/viewer-resource-events.ts` | 159 | Viewer ResourceIO 订阅（替代 host watch） |
| `desktop/src/modules/platform.js` | 42 | 启动前 data-platform 引导 |
| `desktop/src/shared/theme.ts` | 60 | 主题切换同步宿主窗口 |
| `desktop/src/react/app-init.ts` | 451 | 连接握手/事件订阅/appReady/设置事件消费者 |
| `desktop/src/react/components/WindowControls.tsx` | 56 | win/linux 自绘窗口控制 |
| `desktop/src/react/components/chat/InteractiveCard.tsx` | 191 | 卡片 guard flag 消费者 |
| `desktop/src/react/utils/screenshot.ts` | 403 | screenshotRender 消费者 |
| `desktop/src-tauri/src/lib.rs` | 599 | R06 26 命令/窗口只读规则/ACL 模板 |
| `desktop/src-tauri/src/files.rs` | 248 | R06 文件命令实现（上限/CAS/symlink） |
| `desktop/src-tauri/src/skills.rs` | 193 | R06 技能目录只读接口 |
| `desktop/src-tauri/src/resource_bridge.rs` | 115 | R06 服务凭证/资源事件桥 |
| `desktop/src-tauri/tauri.conf.json` | 15 | R06 CSP/withGlobalTauri/bundle 模板 |
| `desktop/src-tauri/Cargo.toml` | 36 | R06 锁版本/可选 wdio 测试插件 |
| `desktop/src-tauri/build.rs` | 33 | AppManifest 命令清单强制点 |
| `desktop/src-tauri/capabilities/main.json` | 32 | R06 主窗 capability 模板 |
| `desktop/src-tauri/capabilities/viewer.json` | 10 | R06 只读窗口 capability |
| `desktop/src-tauri/capabilities/skill-viewer.json` | 14 | R06 技能窗口 capability |
| `desktop/src-tauri/tests/desktop_host.rs` | 586 | R06 真实 GUI E2E/外部观察/ACL 负向模式 |
| `desktop/src-tauri/README.md` | 45 | R06 证据合同/必需检查/阶段观察 |
| `docs/rust-tauri/R00/ACCEPTANCE_MAP.json` | n/a | R09 16 基础场景（程序提取） |
| `docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json` | n/a | R09-T03/T07 共享 46 叶（程序提取） |
| `docs/rust-tauri/R00/FEATURE_INVENTORY.json` | n/a | 叶子 owner/entrypoint/source_ref 交叉核对 |
| `docs/rust-tauri/R00/ENTRYPOINTS.json` | n/a | EP-DESK-01..07 桌面壳入口 |
| `docs/rust-tauri/R00/OWNERSHIP_CURRENT.md` | 60 | desktop_shell 职责/无深链协议注册/单写者 |
| `docs/rust-tauri/R01/ADR-001-ownership.md` | 156 | tauri-host 边界：只写壳自态 |
| `docs/rust-tauri/R01/SHELL_CAPABILITY_MATRIX.json` | n/a | R01-T06 spike 13 项系统能力实测与 ACL 负向 |
| `docs/rust-tauri/R01/PLATFORM_BUILD_MATRIX.json` | n/a | 四平台工具链/原生依赖状态 |
| `docs/rust-tauri/R01/DEPENDENCY_DECISIONS.md` | n/a | D-11 Tauri 插件组实测版本/D-13 R06 tauri 2.12.1 |
| `package.json` | n/a | build 配置：无 protocols/fileAssociations；mac 权限文案/entitlements |

程序化读取：`docs/rust-tauri/R00/ACCEPTANCE_MAP.json`、`FEATURE_STAGE_ACCEPTANCE.json`、`FEATURE_INVENTORY.json`、`ENTRYPOINTS.json`（16 场景/46 叶/桌面入口/叶子 owner 与 source_ref）。

## 3. 总量与分组统计

### 3.1 能力分组

| 分组 | 能力数 | 能力 ID |
|---|---|---|
| 窗口 / 生命周期 | 8 | `host:main-window`, `host:window-state`, `host:window-controls`, `host:app-lifecycle-quit`, `host:app-ready`, `host:theme-sync`, `host:i18n-main`, `host:reload-restart` |
| Shell 集成（托盘/快捷键/通知/自启/防休眠/单实例） | 7 | `host:single-instance`, `host:tray`, `host:global-shortcut`, `host:notification`, `host:keep-awake`, `host:auto-launch`, `host:context-menu-policy` |
| 系统对话框 / 剪贴板 / 拖拽 / 外链 / 回收站 | 6 | `host:file-dialogs`, `host:clipboard-edit-command`, `host:drag-out`, `host:file-drop-paths`, `host:external-open`, `host:trash` |
| 文件 IO / 监听 / 文档预览 / 导出 / 资源 URL | 5 | `host:file-text-io`, `host:file-watch`, `host:docx-xlsx-preview`, `host:observability-export`, `host:file-url-asset` |
| Viewer / 浏览器 / 截图 / Splash / Onboarding | 6 | `host:detached-viewer`, `host:skill-viewer`, `host:browser-viewer`, `host:screenshot-render`, `host:splash`, `host:onboarding` |
| 设置窗口 / 跨窗广播 | 2 | `host:settings-window`, `host:settings-broadcast` |
| Quick Chat | 2 | `host:quick-chat-window`, `host:quick-chat-session-handoff` |
| 服务连接信息 / 应用信息 | 2 | `host:server-connection-info`, `host:app-info-platform` |
| 更新入口（R10 验收，T03 仅映射） | 3 | `host:update-github-check`, `host:update-auto`, `host:update-train-ota` |
| R09-T06 安全收口 | 8 | `security:capability-sets`, `security:csp`, `security:command-acl`, `security:release-no-test-control`, `security:remote-content-isolation`, `security:no-arbitrary-shell`, `security:token-exposure`, `host:card-frame-navigation-guard` |
| 系统权限（T04 范围） | 1 | `host:speech-permission` |

### 3.2 覆盖与差额

- PlatformApi 成员映射：110/110，缺失 无。
- preload 额外键：screenshotRender, debugOpenOnboarding, debugOpenOnboardingPreview；PlatformApi 有但 preload 无：closeBrowser。
- 无生产消费者的 API 成员（15，含 2 个 preload-only）：`watchFile`, `unwatchFile`, `onFileChanged`, `watchWorkspace`, `unwatchWorkspace`, `onWorkspaceChanged`, `openSettings`, `quickChatShow`, `quickChatShortcutStatus`, `getKeepAwakeStatus`, `speechPermissionStatus`, `speechPermissionRequest`, `closeBrowser`, `debugOpenOnboarding (preload-only)`, `debugOpenOnboardingPreview (preload-only)`。
- 全部成员都无生产消费者的能力：`host:file-watch`, `host:speech-permission`；部分成员无消费者的能力：`host:settings-window`, `host:quick-chat-window`, `host:global-shortcut`, `host:keep-awake`, `host:browser-viewer`, `host:onboarding`。
- 无 API 成员的宿主内建能力（14）：`host:single-instance`, `host:main-window`, `host:window-state`, `host:app-lifecycle-quit`, `host:i18n-main`, `host:tray`, `security:capability-sets`, `security:csp`, `security:command-acl`, `security:release-no-test-control`, `security:remote-content-isolation`, `security:no-arbitrary-shell`, `security:token-exposure`, `host:context-menu-policy`。

> `no-consumer-found` 不等于「死代码」：有的是主进程内部入口（托盘/单实例/窗口策略）、有的是 R00 叶子但替换链已改道（file watch → 服务端 ResourceIO）、有的只在 R08 之前的历史 UI 使用。T03 是否保留 parity 命令逐项见 §6/OQ。

## 4. 能力分组逐项表

### 4.1 窗口 / 生命周期

| ID | API members | Electron 实现（file:line 函数） | Tauri 目标 | 权限 scope | 风险 |
|---|---|---|---|---|---|
| `host:main-window` | —（host-internal） | desktop/main.cjs:3129 createMainWindow()；:809 titleBarOpts()/:795 windowIconOpts()；:3242-3258 close→hide；:3260-3297 closed 清理；:6833 app.on('activate') | plugin:core:window（Rust setup/WebviewWindowBuilder 创建 label=main；关闭策略由 WindowEvent::CloseRequested/RunEvent 处理；无前端命令） | 无（主窗创建与关闭策略留在 Rust；不给 renderer window 创建权限） | medium——关闭语义与 T02 进程监督耦合，错误映射会把「隐藏保持」变成真退出 |
| `host:window-state` | —（host-internal） | desktop/main.cjs:2712 windowStatePath=lingxiHome/user/window-state.json；:2769 loadWindowState()；:2779 saveWindowState()（500ms 防抖+串行链）；:3129 sanitizeWindowState(...)；desktop/src/shared/window-state.cjs | plugin:tauri-plugin-window-state（或 Rust 内部 custom-command:window_state_load/save 保留 LINGXI_HOME/user/window-state.json 路径兼容；前端不暴露） | 无（若坚持命令则仅 Rust 内部调用；否则 plugin 自管） | medium——若用插件默认路径会改变壳自态存储位置（R00 STORES adjacent_compatible），需数据兼容评估 |
| `host:window-controls` | windowMinimize, windowMaximize, windowClose, windowIsMaximized, onMaximizeChange | desktop/main.cjs:6605-6618 window-minimize/maximize/close/is-maximized handlers（BrowserWindow.fromWebContents(event.sender)）；:3248-3249 maximize/unmaximize 广播；desktop/preload.cjs:208-216 | custom-command:window_minimize/window_maximize/window_close/window_is_maximized（command 注入调用方 WebviewWindow，拒绝 renderer 指定窗口 id） | 自定义 allow-window-minimize/allow-window-maximize/allow-window-close/allow-window-is-maximized（只授予 win/linux 主窗与设置窗；不建议直接授 core:window 全家桶） | low——R06 无窗口控制模板，但 API 面小；风险在 Linux WM 差异 |
| `host:app-lifecycle-quit` | —（host-internal） | desktop/main.cjs:6824 app.on('window-all-closed')；:6833 activate；:6843 will-quit（keepAwake dispose/globalShortcut.unregisterAll/tray.destroy）；:6854 shutdownServer()；:6942 before-quit（隐藏窗口→按策略关自有 server→app.quit） | plugin:core:runtime（RunEvent::ExitRequested/Exit；WindowEvent::CloseRequested 映射 hide/quit）+ custom-command:restart_app | 无（退出/隐藏策略在 Rust；renderer 无 quit 权限，restart 仅 app-restart 白名单） | high——R09-T02 supervisor 与退出清理强耦合，须 T02 先定协议；错误实现会杀远端服务（A04 失败） |
| `host:app-ready` | appReady | desktop/main.cjs:6620-6653 app-ready handler（写诊断、show 主窗、首启 mac 通知授权探测、200ms 后关 splash/onboarding）；desktop/preload.cjs:109 | custom-command:app_ready（Rust 侧 show 主窗/关 splash+onboarding；30s 超时强显） | 自定义 allow-app-ready（仅 label=main，幂等） | medium——需把 Electron 的 30s 兜底与 splash 关闭时序照搬，否则白屏风险 |
| `host:theme-sync` | syncWindowTheme | desktop/main.cjs:5938 window-theme-changed wrapIpcOn → applyWindowThemeColors(win, theme)；:830 applyWindowThemeColors()；:817 resolveConcreteTheme()；:903 applyTransparentWindowBackground()（quick-chat 特例）；desktop/preload.cjs:110 | custom-command:sync_window_theme（Rust 校验 theme id∈theme-registry 白名单，再 window.set_theme/set_background_color；quick-chat 透明背景特例） | 自定义 allow-sync-window-theme（主/设置/quick-chat/browser-viewer/viewer label） | low——Tauri set_theme/set_background_color 覆盖面需实测（Linux） |
| `host:i18n-main` | —（host-internal） | desktop/main.cjs:743 _getMainI18n()；:768 mt()；:781 resetMainI18n()；:5980-5995 settings-changed 'locale-changed' → 重置 i18n 并重建托盘菜单 | custom-command:settings_changed（locale-changed 分支 Rust 重建托盘菜单；i18n 文案由 Rust 内置 locales 读取） | 无额外（复用 allow-settings-changed） | low——托盘菜单文案跟随语言是现役隐含行为，易漏 |
| `host:reload-restart` | reloadMainWindow, restartApp | desktop/main.cjs:6531 reload-main-window handler（mainWindow.webContents.reload）；:3055 restartApp()（isExitingServer/isQuitting→app.relaunch→quit，走 before-quit 清理）；desktop/preload.cjs:83,146 | custom-command:reload_main_window（仅主窗）/restart_app（Rust 走 RunEvent 退出重启，不允许 renderer 传参） | 自定义 allow-reload-main-window（仅 label=main）/allow-restart-app（仅主窗） | medium——restart 与更新/退出清理耦合，需复用 T02 退出路径 |

### 4.2 Shell 集成（托盘/快捷键/通知/自启/防休眠/单实例）

| ID | API members | Electron 实现（file:line 函数） | Tauri 目标 | 权限 scope | 风险 |
|---|---|---|---|---|---|
| `host:single-instance` | —（host-internal） | desktop/main.cjs:371 configureClientSingleInstance({lingxiHome,...}) → showPrimaryWindow；desktop/src/shared/single-instance-lock.cjs:71 app.requestSingleInstanceLock({lingxiHome})；:77 app.on('second-instance')；desktop/main.cjs:2354 showPrimaryWindow() | plugin:tauri-plugin-single-instance（Rust setup 注册；second-instance 回调聚焦/显示主窗；前端零权限） | 无（Rust 侧插件初始化；capability 不向任何 webview 开放） | medium——Tauri 插件按 app identifier 而非 LINGXI_HOME 隔离，需显式验证隔离目录并行启动语义（R09 START_RECEIPT §7） |
| `host:tray` | —（host-internal） | desktop/main.cjs:2389 createTray()；:2402 new Tray；:2405 Menu.buildFromTemplate（显示/设置/修复组件/退出）；:2417 setContextMenu；:2418 right-click 重建；:2419 double-click 显示；:2365/:2374 图标候选；:5983 locale 重建；:6709-6710 启动装配 | plugin:core:tray-icon（tauri feature tray-icon；tray-icon crate MenuBuilder；Rust 侧事件处理，前端零权限） | 无（托盘与菜单全部 Rust 侧；不向 webview 暴露 tray 命令） | high——托盘是 T03 显式要求但 R00 无独立叶子（仅 EP-DESK-05 复合登记）；Tauri 托盘菜单事件需与窗口隐藏/退出策略联动 |
| `host:global-shortcut` | keybindingsTestRegister, keybindingsReloadGlobal, quickChatReloadShortcut, quickChatShortcutStatus | desktop/main.cjs:231 registerQuickChatShortcut()（多组键位 1-3）、:266 testGlobalShortcutAvailability()、:3037 reloadQuickChatShortcut()、:3041 registerQuickChatShortcutBestEffort()；:5695-5699 IPC；:6845 unregisterAll；快捷键定义来自 user/preferences.json（只读，:208-258） | plugin:tauri-plugin-global-shortcut（Rust 侧注册/注销；热键值由 Rust 读 preferences.json）+ custom-command:keybindings_reload_global/keybindings_test_register/quick_chat_reload_shortcut/quick_chat_shortcut_status | 自定义 allow-keybindings-* / allow-quick-chat-reload-shortcut / allow-quick-chat-shortcut-status（仅设置窗+主窗）；插件 JS 权限不需要（Rust 侧调用） | high——R01 spike 仅验证单键位；多组/冲突/抢占与 Wayland 限制未迁移；Tauri 插件对多 accelerator 语义需实测 |
| `host:notification` | showNotification | desktop/main.cjs:6543-6570 show-notification handler（focus 抑制策略 normalizeDesktopNotificationOptions/shouldSuppressDesktopNotification；Notification.isSupported；agent 头像 icon；click→恢复+聚焦主窗；showNotificationWithReceipt 等待真实 show 事件）；desktop/src/shared/desktop-notification-policy.cjs:1；desktop/src/shared/desktop-notification-delivery.cjs:2；:6640 首启 mac 权限探测 | custom-command:show_notification + plugin:tauri-plugin-notification（Rust NotificationExt 展示；点击聚焦需平台自定义/补充） | 自定义 allow-show-notification（主窗 only；plugin notification:default 不下放 renderer） | high——「点击通知聚焦主窗」在 Tauri 无官方通用 click 回调，需平台补丁或降级并在 A05/叶子断言中如实记录 |
| `host:keep-awake` | getKeepAwakeStatus, setKeepAwakeEnabled | desktop/keep-awake.cjs:5 createKeepAwakeManager()（powerSaveBlocker type=prevent-app-suspension）；desktop/main.cjs:179 创建、:5693-5694 IPC、:6685 启动读偏好、:6844 will-quit dispose | custom-command:get_keep_awake_status/set_keep_awake_enabled（无官方 plugin；三平台各自实现：mac IOPMAssertion / win SetThreadExecutionState / linux DBus inhibit） | 自定义 allow-keep-awake-*（设置窗+主窗） | medium——无现成 Tauri 插件，三平台均需自写原生实现；Linux 权限/兼容性不确定 |
| `host:auto-launch` | getAutoLaunchStatus, setAutoLaunchEnabled | desktop/login-item-settings.cjs:42 getAutoLaunchStatus()/:55 setAutoLaunchEnabled()/:63 wasLaunchedAtLogin()（mac wasOpenedAtLogin、win --hana-start-at-login）；desktop/main.cjs:5691-5692、:6676 启动读 openedAtLogin（后台启动不显 splash） | plugin:tauri-plugin-autostart（Rust 侧 enable/disable/is_enabled）+ custom-command:get_auto_launch_status/set_auto_launch_enabled（回显真实状态） | 自定义 allow-auto-launch-*（设置窗）；plugin autostart:allow-* 不下放 renderer | medium——BLOCKED-SIGNING-IDENTITY 下 mac SMAppService 正式形态无法完整验证；win/linux 平台阻塞 |
| `host:context-menu-policy` | —（host-internal） | desktop/main.cjs:490-507 app.on('web-contents-created') 全局安装：非 editable 区域 context-menu preventDefault（styles.css body user-select:none）；无 setApplicationMenu 调用 | plugin:core:webview（Tauri release 默认不提供 browser 默认右键菜单；dev 模式 devtools 菜单需关闭——build 配置验证） | 无（webview 行为） | low——影响面小；Linux WebKitGTK 行为需实测 |

### 4.3 系统对话框 / 剪贴板 / 拖拽 / 外链 / 回收站

| ID | API members | Electron 实现（file:line 函数） | Tauri 目标 | 权限 scope | 风险 |
|---|---|---|---|---|---|
| `host:file-dialogs` | selectFolder, selectFiles, selectSkill | desktop/main.cjs:6011-6022 select-folder；:6024-6035 select-files（multiple 语义）；:6037-6073 select-skill（zip/skill 过滤）；均 dialog.showOpenDialog(BrowserWindow.fromWebContents)；desktop/preload.cjs:111-113 | plugin:tauri-plugin-dialog（Rust DialogExt callback API，R06 select_files/select_folder 模板）+ custom-command:select_folder/select_files/select_skill | 自定义 allow-select-folder/allow-select-files/allow-select-skill（主窗+设置窗；dialog 插件 JS 权限不下放） | medium——Tauri dialog 取消回调语义已实测有坑（R01 spike TAURI_SPIKE_REPORT §7-R1）；select_skill 需新增命令 |
| `host:clipboard-edit-command` | runEditCommand | desktop/main.cjs:5656-5664 run-edit-command handler（白名单 cut/copy/paste/selectAll→event.sender[command]()）；desktop/preload.cjs:39；main 进程无 clipboard 模块使用（grep: main.cjs 仅 import 无 clipboard） | custom-command:run_edit_command（R06 lib.rs:192-227 已有：mac objc2 firstResponder tryToPerform，非 mac eval 脚本；加入允许集校验） | 自定义 allow-run-edit-command（主窗+设置窗输入区域） | low——R06 已有可用实现；注意 R06 用真实系统剪贴板会污染用户剪贴板需恢复（R06 ClipboardRestore 模式） |
| `host:drag-out` | startDrag | desktop/main.cjs:6238-6255 start-drag wrapIpcOn（app.getFileIcon 图标；单文件 event.sender.startDrag({file})/多文件 {files}）；desktop/preload.cjs:204 | custom-command:start_drag（Rust Window::start_dragging(tauri::DragDrop?)；图标用 file icon/透明占位） | 自定义 allow-start-drag（主窗；viewer 窗口不需要） | medium——Tauri start_dragging 存在但多文件/图标/平台语义需实测；社区 tauri-plugin-dragout 可作备选 |
| `host:file-drop-paths` | getFilePath | desktop/preload.cjs:143 getFilePath: (file)=>webUtils.getPathForFile(file)；desktop/preload.cjs:7 引入 webUtils | plugin:core:webview（Tauri drag-drop 事件 onDragDropEvent 直接给绝对路径；替代 webUtils.getPathForFile）——需要 DesktopHost-Tauri 适配层把 drop 事件转成现有调用面 | 无额外（Tauri core 事件监听由 core:default 覆盖；不授予 fs 直接读取） | medium——React 现有多处用 File 对象取路径，Tauri 无 webUtils 等价；需适配层改造（仅宿主适配，不改业务语义） |
| `host:external-open` | openExternal, openFolder, openFile, showInFinder | desktop/main.cjs:6282-6296 open-external（URL 解析，仅 http/https，shell.openExternal，返回 {ok,error}）；:6229-6236 open-folder（绝对路径且 isDirectory）；:6274-6281 open-file（绝对路径且 isFile）；:6256-6260 show-in-finder；:3237-3246/:3358-3367 will-navigate 外链拦截；:694 isAllowedBrowserUrl | plugin:tauri-plugin-opener（Rust OpenerExt 或 capability 内 allow-open-url/open-path/reveal-item-in-dir + scope）+ custom-command:open_external/open_folder/open_file/show_in_finder | 自定义 allow-open-external/allow-open-folder/allow-open-file/allow-show-in-finder；若直用 opener 插件则需 opener:allow-open-url/allow-open-path/allow-reveal-item-in-dir 且限定 scope；禁止 shell 插件 | medium——现役 openExternal 返回回执对象；插件语义不同需包装保持返回形状；scope 配置是 T06 重点 |
| `host:trash` | trashItem | desktop/main.cjs:6261-6272 trash-item handler（resolveTrashItemPath 校验→lstat→shell.trashItem）；desktop/src/shared/trash-item-path.cjs:1 | custom-command:trash_item（R06 已有 trash crate 实现） | 自定义 allow-trash-item（主窗 only；viewer/skill-viewer 拒绝） | low——R06 已有可复用实现与系统级证据模板 |

### 4.4 文件 IO / 监听 / 文档预览 / 导出 / 资源 URL

| ID | API members | Electron 实现（file:line 函数） | Tauri 目标 | 权限 scope | 风险 |
|---|---|---|---|---|---|
| `host:file-text-io` | readFile, writeFile, writeFileBinary, copyFile, readFileSnapshot, writeFileIfUnchanged, readFileBase64 | desktop/main.cjs:6297-6310 read-file；:6304-6310 read-file-snapshot；:6312-6318 write-file；:6320-6328 write-file-if-unchanged（file-text-io.cjs:31 CAS 语义）；:6330-6355 write-file-binary（~ 展开；无大小上限）；:6343-6355 copy-file（拒绝 symlink）；:6463-6475 read-file-base64（20MB 上限）；desktop/file-text-io.cjs:12 readTextFileSnapshot（5MB+二进制探测+sha256） | custom-command:read_file/read_file_snapshot/read_file_base64/write_file/write_file_binary/write_file_if_unchanged/copy_file（R06 files.rs 全部已有：绝对路径、5MiB/20MiB 上限、sha256 版本、symlink 拒绝） | 自定义 allow-read-file/allow-read-file-snapshot/allow-read-file-base64/allow-write-file/allow-write-file-binary/allow-write-file-if-unchanged/allow-copy-file（按窗口 label：主窗全量；viewer 只读且仅自身文件；skill-viewer 无写权限） | low——R06 已有 20/91 命令可复用；主要风险在 R06 上限（5MB/20MB）与 R09 现役 main.cjs 上限（5MB/20MB/2MB skill）一致性 |
| `host:file-watch` | watchFile, unwatchFile, onFileChanged, watchWorkspace, unwatchWorkspace, onWorkspaceChanged | desktop/main.cjs:6408-6420 watch-file（file-watch-registry 共享订阅+50ms 防抖+subscriber destroyed 清理）；:6421-6425 unwatch-file；:6444-6456 watch-workspace（workspace-watch-registry + chokidar，6402-6443）；:6457-6462 unwatch-workspace；事件 file-changed/workspace-changed 6405/6437 | custom-command:watch_file/unwatch_file/watch_workspace/unwatch_workspace（notify crate 事件；R06 已有 250ms 轮询 watch_file 模板） | 自定义 allow-watch-file/allow-unwatch-file/allow-watch-workspace/allow-unwatch-workspace（主窗；按 subscriber 隔离） | medium——无 React 生产消费者（替换路径是服务端 ResourceIO），T03 需决定「保留 parity 命令」还是登记 drop-with-reason；R06 轮询实现语义弱于 fs.watch |
| `host:docx-xlsx-preview` | readDocxHtml, readXlsxHtml | desktop/main.cjs:6490-6502 read-docx-html、:6504-6527 read-xlsx-html；:6474-6489 readRustOfficePreview()（Rust 服务 /lingxi/v1/fs/{docx,xlsx}-html，10s 超时+连接切换检测）；Node 链用 mammoth/exceljs（20MB 上限） | custom-command:read_docx_html/read_xlsx_html（转发 Rust 服务 HTTP 或 renderer 直连；Tauri host 不内置 mammoth/exceljs） | 自定义 allow-read-docx-html/allow-read-xlsx-html（主窗+主窗 viewer） | low——Rust 服务已有此路由；风险在 Tauri 侧临时 token/端口获取与连接切换检测 |
| `host:observability-export` | observabilityExportBegin, observabilityExportWrite, observabilityExportEnd, observabilityExportAbort | desktop/main.cjs:6074-6140 observability-export:begin/write/end/abort（showSaveDialog→exportId capability token 绑定 senderId；≤4MB chunk；abort/窗口销毁删部分文件）；desktop/preload.cjs:135-138 | custom-command:observability_export_begin/observability_export_write/observability_export_end/observability_export_abort（路径只来自系统保存对话框；capability token 绑定窗口） | 自定义 allow-observability-export-begin/write/end/abort（仅设置窗/主窗） | low——语义清晰；Tauri 需把 exportId 存 Rust managed state 并校验窗口 label（不能只信 renderer） |
| `host:file-url-asset` | getFileUrl | desktop/preload.cjs:140 getFileUrl: (filePath)=>pathToFileUrl(filePath)（同步纯字符串转换，无 IPC）；desktop/src/shared/path-to-file-url.cjs:1 | plugin:core:asset-protocol（渲染侧 convertFileSrc + tauri.conf app.security.assetProtocol.scope 限制可读根） | asset protocol scope（不是 capability permission）：app.security.assetProtocol.enable=true + scope 白名单（LINGXI_HOME/workspace/临时目录按需）；CSP img-src/media-src 需允许 asset: | medium——现役 file:// 直链在 Tauri 不可用，需改走 asset 协议并调整 CSP/scope；会同时影响 T06 CSP 与 A10 |

### 4.5 Viewer / 浏览器 / 截图 / Splash / Onboarding

| ID | API members | Electron 实现（file:line 函数） | Tauri 目标 | 权限 scope | 风险 |
|---|---|---|---|---|---|
| `host:detached-viewer` | spawnViewer, viewerRequestLoad, viewerClose, onViewerClosed | desktop/main.cjs:5879-5922 spawn-viewer（新 BrowserWindow+_viewerPayloads）；:5924-5930 viewer-request-load（按 sender 反推 windowId）；:5932-5936 viewer-close；:5914 viewer-closed 广播；viewport 入口 desktop/src/viewer-window-entry.tsx:74/111/139 | custom-command:spawn_viewer/viewer_request_load/viewer_close（R06 已有）+ plugin:core:event viewer-closed | 自定义 allow-spawn-viewer/allow-viewer-request-load/allow-viewer-close；viewer-* 窗口只可读自身文件（R06 readable 规则） | low——R06 已有实现与只读窗口拒写负向测试 |
| `host:skill-viewer` | openSkillViewer, listSkillFiles, readSkillFile, onShowSkillViewer | desktop/main.cjs:6141-6204 open-skill-viewer（.skill/.zip 解压：:6162-6186 execFileSync unzip/powershell Expand-Archive；已安装目录优先）；:6205-6214 skill-viewer-list-files；:6215-6228 skill-viewer-read-file（2MB 文本上限）；:3401 _showSkillViewer（Electron 走主窗 overlay 事件，不开独立窗）；:3412 scanSkillDir；desktop/preload.cjs:152,200-202 | custom-command:open_skill_viewer（Rust 侧用 zip crate 解压，禁止 exec unzip/powershell）+ list_skill_files/read_skill_file（R06 已有）+ event show-skill-viewer 到主窗 overlay | 自定义 allow-open-skill-viewer/allow-list-skill-files/allow-read-skill-file（主窗+设置窗；skill-viewer 窗口若独立则只读自身目录） | high——现役 zip 解压调用外部 unzip/powershell（main.cjs:6165-6175）与 T06 禁止泛化 shell 冲突，Tauri 必须改 Rust zip 实现并做路径穿越负向 |
| `host:browser-viewer` | openBrowserViewer, setBrowserConnection, getBrowserViewerContext, rustBrowserRequest, onBrowserUpdate, closeBrowserViewer, closeBrowser, browserGoBack, browserGoForward, browserReload, browserNewTab, browserSwitchTab, browserCloseTab, browserEmergencyStop | desktop/main.cjs:5753-5798 open-browser-viewer；:5737-5740 set-browser-connection（仅主窗）；:5741-5744 get-browser-viewer-context；:5745-5752 rust-browser-request（rust-browser-control）；:5800-5857 back/forward/reload/new-tab/switch-tab/close-tab/close-viewer/emergency-stop；:3435 createBrowserViewerWindow；:4030-4060 WebContentsView（sandbox:true, partition）；:4234 setWindowOpenHandler deny；:4151/3830 browser-update 广播；desktop/src/shared/rust-browser-control.cjs:15 | custom-command:open_browser_viewer/set_browser_connection/get_browser_viewer_context/rust_browser_request/browser_go_back/browser_go_forward/browser_reload/browser_new_tab/browser_switch_tab/browser_close_tab/close_browser_viewer/browser_emergency_stop（T05 决定 child webview 形态；R09-T05 负责最终接线） | 自定义 allow-browser-*（仅主窗 + browser-viewer 窗口；远程内容 webview 零 capability）；核心 webview 创建权限不下放 renderer | high——超出 T03 基础能力，属 T05；Tauri 嵌 BrowserView 等价物（多 webview/child view）与 rust-browser-request 代理是本阶段最大技术不确定性之一 |
| `host:screenshot-render` | screenshotRender (preload-only) | desktop/preload.cjs:125 screenshotRender（PlatformApi 未声明，靠 any 访问）；desktop/main.cjs:6357-6400 screenshot-render handler；:5377 screenshotCapture()（offscreen hidden BrowserWindow capturePage，deviceScaleFactor:2）；:4948 getScreenshotWindow；:4893 SCREENSHOT_THEMES | custom-command:screenshot_render（T04 范围；隐藏 webview 渲染+截取，Tauri 无稳定跨平台 capture API，需 spike） | 自定义 allow-screenshot-render（主窗）；输出路径只允许 Desktop/workspace/screenshots，文件名服务端生成 | high——Tauri/Wry 无官方页截图 API，需要 wry 平台钩子或改用服务端渲染；本清单标注待 T04 spike，不臆造实现 |
| `host:splash` | getAvatarPath, getSplashInfo | desktop/main.cjs:2668 createSplashWindow()；:5999-6007 registerSplashDataIpc({home, senderAllowed})；desktop/src/shared/splash-avatar-path.cjs:121 registerSplashDataIpc/:126 get-avatar-path（仅 splash/onboarding 窗口+页面 URL 白名单，头像文件类型/大小/符号链接校验）/:147 get-splash-info/:112 readSplashInfo；desktop/preload.cjs:144-145 | custom-command:get_splash_info/get_avatar_path（Rust 读 LINGXI_HOME；校验调用 webview label ∈ {splash,onboarding} 且 URL 为预期页面） | 自定义 allow-get-splash-info/allow-get-avatar-path（仅 splash/onboarding label） | low——安全校验已有共享实现可平移；注意 Tauri 无 executeJavaScript webp 校验等价（可用 Rust image 解码） |
| `host:onboarding` | onboardingComplete, debugOpenOnboarding (preload-only), debugOpenOnboardingPreview (preload-only) | desktop/main.cjs:4850-4884 createOnboardingWindow()；:6573-6581 debug-open-onboarding；:6582-6590 debug-open-onboarding-preview；:6591-6603 onboarding-complete（server PreferencesManager 持久化后 createMainWindow+注册快捷键；Rust 服务模式显式拒绝）；desktop/src/shared/onboarding-completion.cjs；desktop/preload.cjs:148-150 | custom-command:onboarding_complete（Rust 代理 server PUT /api/config 后建主窗）+ debug_open_onboarding/debug_open_onboarding_preview（仅非 release debug feature） | 自定义 allow-onboarding-complete（splash/onboarding/主窗）；debug 命令在 release 构建中不存在（cfg 剔除） | medium——Rust 服务模式下现役 onboarding-complete 显式 throw（main.cjs:6592-6594），Tauri 需按 Rust 服务真实接口重定语义；debug 命令与 T06 生产面裁剪冲突需裁决 |

### 4.6 设置窗口 / 跨窗广播

| ID | API members | Electron 实现（file:line 函数） | Tauri 目标 | 权限 scope | 风险 |
|---|---|---|---|---|---|
| `host:settings-window` | openSettings, onOpenSettingsModal, onSwitchTab | desktop/main.cjs:5734 open-settings handler→createSettingsWindow(tab,theme)；:3299-3399 createSettingsWindow（主窗可用时 send open-settings-modal 并前置主窗；否则独立 720x700 BrowserWindow，主窗 resize 同步 90%）；:3305 open-settings-modal；:3316/:3353 settings-switch-tab；desktop/preload.cjs:154,161-170 | custom-command:open_settings（Rust 判定：主窗存在→emit open-settings-modal；否则创建 label=settings 的 WebviewWindow）+ plugin:core:event | 自定义 allow-open-settings（主窗/托盘内部；设置窗） | medium——现役「主窗优先 modal」分支说明设置窗只是 fallback；Tauri 若一律开独立窗会改变可见行为（A13/T07 会拍到差异） |
| `host:settings-broadcast` | settingsChanged, onSettingsChanged | desktop/main.cjs:5948-6010 settings-changed wrapIpcOn（向主窗/设置窗/browser-viewer 转发；side effects: theme-changed 更新 _browserViewerTheme+转发、network-proxy-changed applyDesktopNetworkProxy、keep-awake-changed keepAwakeManager、quick-chat-shortcut-changed 重新注册、locale-changed 重置 i18n+重建托盘）；desktop/preload.cjs:155-160 | custom-command:settings_changed（Rust 校验 event type 白名单 + payload 形状后 emit_to 具名窗口 labels；side effects 在 Rust 侧执行） | 自定义 allow-settings-changed（主窗/设置窗/browser-viewer；事件名白名单） | high——侧效应（主题/代理/防休眠/快捷键/托盘 i18n）是现役真实行为，Tauri 若只转发事件不执行 side effect，功能表面「通过」但系统状态不生效（A05 失败） |

### 4.7 Quick Chat

| ID | API members | Electron 实现（file:line 函数） | Tauri 目标 | 权限 scope | 风险 |
|---|---|---|---|---|---|
| `host:quick-chat-window` | quickChatShow, quickChatHide, quickChatResize, onQuickChatShown | desktop/main.cjs:2937-2984 createQuickChatWindow（frameless/alwaysOnTop/transparent/skipTaskbar/acceptFirstMouse）；:3001 hideQuickChatWindow（保存+焦点保护）；:3008-3027 showQuickChatWindow（setAlwaysOnTop floating、mac visibleOnAllWorkspaces、app.focus steal、quick-chat-shown）；:3029 toggle；:2909-2935 applyQuickChatMode；:2808/2828/2863 状态/边界；:5705-5706 quick-chat-show/hide；:5724 resize；desktop/preload.cjs:84-103 | custom-command:quick_chat_show/quick_chat_hide/quick_chat_resize + plugin:core:webview（label=quick-chat 的 WebviewWindowBuilder） | 自定义 allow-quick-chat-show/allow-quick-chat-hide/allow-quick-chat-resize（quick-chat 窗口 + 主窗） | medium——透明窗/置顶/多工作区是平台差异点；Tauri 透明窗在 Linux 常失败需实测 |
| `host:quick-chat-session-handoff` | quickChatOpenSession, onQuickChatOpenSession | desktop/main.cjs:5728-5732 quickChatSessionHandoff.open/acknowledge；desktop/quick-chat-session-handoff.cjs:3（仅 quick-chat 窗口可信；主窗存活；sessionPath/ownerKey/sessionId 校验；15s 超时；主窗确认后 focus 再 hide；失败保留快聊窗）；desktop/preload.cjs:88-98 | custom-command:quick_chat_open_session/quick_chat_open_session_ack + plugin:core:event（quick-chat-open-session 请求/确认） | 自定义 allow-quick-chat-open-session（仅 quick-chat 窗口）/allow-quick-chat-open-session-ack（仅主窗） | medium——协议细节（focus 确认、超时、窗口身份）必须逐条保留，Tauri 事件/命令需等价实现 |

### 4.8 服务连接信息 / 应用信息

| ID | API members | Electron 实现（file:line 函数） | Tauri 目标 | 权限 scope | 风险 |
|---|---|---|---|---|---|
| `host:server-connection-info` | getServerPort, getServerToken, getServerConnectionInfo, onServerRestarted | desktop/main.cjs:5646-5655 get-server-port/get-server-token/get-server-connection-info；:2302 monitorServer()→:2324/:2328 广播 server-restarted；desktop/preload.cjs:36-38,171-175（事件） | custom-command:get_server_connection_info（T02 supervisor 持有 managed state；token 只经命令返回）+ plugin:core:event server-restarted | 自定义 allow-get-server-connection-info（主窗/设置窗/quick-chat/viewer/splash 各自按需；token 不落 URL/CSP 可见处） | high——token 交付面是 A06/A10 的核心；Tauri 必须避免 core/http 插件泛化与 URL 注入；T02 尚未定 supervisor 接口 |
| `host:app-info-platform` | getAppVersion, getPlatform | desktop/main.cjs:5670 get-app-version→app.getVersion()；:6604 get-platform→process.platform；desktop/preload.cjs:40,208；desktop/src/modules/platform.js:21 getPlatform 用于 data-platform；桌面壳版本 vs 内容版本语义见 main.cjs:5665-5670 注释 | custom-command:get_platform/get_app_version（或 plugin:core:app:allow-version + plugin:os，推荐 custom 收口保持壳身份语义） | 自定义 allow-get-platform/allow-get-app-version（主窗/设置窗；返回壳版本而非内容版本） | low——注意返回字面值兼容（'darwin'/'win32'/'linux'），不要用 Tauri 的 'macOS' 命名 |

### 4.9 更新入口（R10 验收，T03 仅映射）

| ID | API members | Electron 实现（file:line 函数） | Tauri 目标 | 权限 scope | 风险 |
|---|---|---|---|---|---|
| `host:update-github-check` | releaseCheckLatest, getPendingAnnouncement, ackAnnouncement, getUpdateDigestHistory | desktop/main.cjs:5677-5686 release-check-latest（60s 缓存→githubReleaseCheck.checkLatestRelease）；:5687-5690 get-pending-announcement/ack-announcement（computePendingAnnouncement/writeLastSeenVersion:2724）；:5643 get-update-digest-history（update-digest-history.cjs）；desktop/src/shared/github-release-check.cjs:24；desktop/src/shared/update-digest-history.cjs | custom-command:release_check_latest/get_pending_announcement/ack_announcement/get_update_digest_history（Rust reqwest 固定 GitHub 端点；不含任意 URL 参数） | 自定义 allow-release-check-latest/allow-pending-announcement/allow-ack-announcement/allow-update-digest-history（主窗/设置窗） | low——T06 只要求固定端点；R10 才验收更新链 |
| `host:update-auto` | autoUpdateCheck, autoUpdateInstall, autoUpdateState, autoUpdateSetChannel, onAutoUpdateState | desktop/auto-updater.cjs:754-783 registerIpcHandlers（auto-update-check/install/state/set-channel）；:795 initAutoUpdater；:228 checkForUpdatesOnce；:521 installDownloadedUpdate；:651 setupAutoUpdater；state 广播 :323 auto-update-state；desktop/main.cjs:3168-3174 初始化；desktop/preload.cjs:49-52,104-108 | plugin:tauri-plugin-updater（Rust 侧 check/download/install；minisign 验签，R01 spike 已验证正负向）+ custom-command:auto_update_check/auto_update_install/auto_update_state/auto_update_set_channel + event auto-update-state | 自定义 allow-auto-update-*（仅设置窗/主窗）；plugin updater:default 不下放 renderer；签名公钥固定 | high——electron-updater→tauri-plugin-updater 的渠道/自替换语义差异大（mac self-install、Windows）；本阶段只做能力映射，不声称更新链可用 |
| `host:update-train-ota` | trainUpdateStatus, trainUpdateCheck, trainUpdateApply, onTrainUpdateAvailable, onTrainUpdateProgress, onTrainFallbackNotice, ackTrainFallbackNotice | desktop/main.cjs:5571-5642 train-update-status/check/apply（applyTrainUpdateNow:5606；进度 sender 定向:5514；一次性 fallback notice:5584/1855）；:3096-3127 后台 OTA 调度（只 check 不下载/不写指针）；desktop/preload.cjs:54-75；desktop/src/shared/train-update-apply.cjs；shared/artifact-core/ota-core.cjs | custom-command:train_update_status/train_update_check/train_update_apply + events train-update-available/train-update-progress/train-fallback-notice（Rust 侧 OTA 逻辑；T02/T08 定 supervisor 与产物） | 自定义 allow-train-update-*（主窗/设置窗）；不允许 renderer 指定 manifest URL/channel 文件路径（只允许 channel 枚举） | high——现役 OTA 与 electron 主进程/服务 supervisor 深度耦合；Tauri 侧须整体迁移，不能只留按钮 |

### 4.10 R09-T06 安全收口

| ID | API members | Electron 实现（file:line 函数） | Tauri 目标 | 权限 scope | 风险 |
|---|---|---|---|---|---|
| `security:capability-sets` | —（host-internal） | Electron 无 capability 概念：所有窗口共用同一 preload.cjs:34-217 全量桥；隔离靠 webPreferences contextIsolation:true/nodeIntegration:false（main.cjs:3149-3153,2687-2689,2960-2962,3335-3337,3465-3467,4862-4864,5896-5898）与 browser view sandbox:true（:4035-4037,4227-4229）；无多 capability 叠加面 | plugin:core:acl（capabilities/*.json 按窗口 label 分组，R06 模板 capabilities/main.json:1-32、viewer.json、skill-viewer.json；gen/schemas/capabilities.json 展开结果） | capability 文件集合（R09 需为 main/settings/quick-chat/browser-viewer/viewer-*/skill-viewer-*/splash/onboarding 分别定义）；远程 webview 不加入任何 capability windows | high——R09 命令面约 91 条（R06 26 条），按窗口最小权限集合易出错；必须机器化展开比对而非人工审阅 |
| `security:csp` | —（host-internal） | Electron 各入口 HTML 内联 CSP meta：browser-viewer.html:6、quick-chat.html:6、splash.html:6、onboarding.html:6、viewer-window.html:6、mobile.html:5；index.html/settings.html 无 CSP（仅 will-navigate 拦截 main.cjs:3237-3246/3358-3367）；外链仅 http(s) 694 isAllowedBrowserUrl | plugin:core:csp（tauri.conf.json app.security.csp / devCsp；R06 现有 csp: "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self' ipc: http://ipc.localhost; img-src 'self' data:; frame-src 'self' blob:; object-src 'none'" 是模板起点） | 全局 CSP 配置（非 capability permission）；asset protocol scope 与 CSP img-src/media-src/connect-src 必须一致（允许 http/ws://127.0.0.1:* 服务连接、asset:、blob:；禁止 remote script/frame 任意源） | high——现役主窗/设置窗无 CSP（Electron 安全债），Tauri 收紧后可能与现有内联脚本/字体/主题资源冲突，需要实测清单 |
| `security:command-acl` | —（host-internal） | Electron IPC 无 ACL 层：desktop/ipc-wrapper.cjs:18 wrapIpcHandler/:34 wrapIpcBestEffortHandler/:48 wrapIpcOn + preload.cjs:34-217 单一桥；channel 白名单靠代码评审，无运行时拒绝矩阵 | plugin:core:acl（build.rs:1-33 AppManifest::commands 列表为注册强制点；permissions/autogenerated/*.toml 每条命令 allow/deny；capabilities/*.json 引用） | 每个 custom-command 一条 permission（allow-<command>/deny-<command>）；capability 只引用所需命令；禁止通配全部命令 | high——91 条命令的人工维护易漏；需要 R09 生成/校验脚本（含空集合失败） |
| `security:release-no-test-control` | —（host-internal） | Electron 生产仍注册调试 IPC（main.cjs:6573-6590 debug-open-onboarding/debug-open-onboarding-preview 无环境门控）；--dev 可开 DevTools（main.cjs:3207-3209，release 亦接受该 flag）；无 WebDriver 内嵌 | plugin:core:build（e2e-test feature 门控 wdio（R06 Cargo.toml:optional tauri-plugin-wdio-webdriver + lib.rs:594-596）；release 关闭 devtools feature；debug 命令 cfg(debug_assertions)/feature 剔除；--dev flag 在 release 不接受） | 无（测试/调试命令不进入 release capability；release capabilities 不含 debug 命令） | high——R00 46 叶要求保留 debug onboarding 功能（T07 可能验收），T06 要求生产无测试控制面；需裁决「功能保留但 release 不可达」并落到 capability 分组 |
| `security:remote-content-isolation` | —（host-internal） | desktop/main.cjs:4040-4052 WebContentsView(sandbox:true,contextIsolation,nodeIntegration:false)、:4234 setWindowOpenHandler deny、:3968-4000 浏览器 session 分区与 cookie 策略；:3237-3246/:3358-3367 will-navigate 外链；预览 iframe 无脚本（A10 场景）；desktop/src/shared/card-frame-navigation.cjs:12 卡片导航 guard（main.cjs:495-496 全局安装） | plugin:core:webview（WebviewUrl::External 远程内容 webview 默认零 ACL 能力；R01 spike negative cases） | 远程 webview 不加入任何 capability；宿主窗口 capability 不含任意 HTTP/shell；asset scope 不覆盖远程内容 | high——现役卡片导航 guard 在 Tauri 无 will-frame-navigate 等价（见 host:card-frame-navigation-guard），预览隔离策略需重新设计 |
| `security:no-arbitrary-shell` | —（host-internal） | Electron 无通用 shell IPC；openExternal 仅 http(s)（main.cjs:6282-6296）；openFile/openFolder 绝对路径+类型校验（6229-6281）；但 open-skill-viewer 现役用 child_process execFileSync('unzip')/powershell Expand-Archive（main.cjs:6165-6175）——潜在命令执行面；R01 sidecar 要求固定二进制+受限参数 | custom-command（仅白名单文件/URL 操作，无 shell 命令）+ R01 spike sidecar 固定二进制模式（tauri-plugin-shell 仅 Rust 侧、capabilities 零 shell 权限） | 不授予 shell 插件任何前端权限；自定义命令不接受脚本/命令/参数拼接；zip 解压改 Rust zip crate（不得 exec unzip/powershell）；sidecar 参数固定 | high——现役技能解压外调 unzip/powershell 是真实命令面；T06 要求收口，T03 的 skill-viewer 命令必须换实现 |
| `security:token-exposure` | —（host-internal） | server token 由 get-server-token IPC 返回给任意窗口（main.cjs:5646-5647 无 sender 校验）；web fallback 从 URL query/localStorage 取 token（desktop/src/react/host/web-desktop-host.ts:79-81）；浏览器 WebSocket/HTTP 使用 Bearer（viewer-resource-events.ts:53-58） | custom-command:get_server_connection_info（Rust managed state；token 只在命令返回值中，不写 URL/不打日志） | 自定义 allow-get-server-connection-info 按窗口最小化；任何 webview 不得通过 URL/query/asset 读取 token | medium——现役 Electron get-server-token 无窗口校验，Tauri 必须按窗口收窄；Web fallback 的 URL token 不属于桌面形态但需在文档标注边界 |
| `host:card-frame-navigation-guard` | cardFrameNavigationGuard | desktop/preload.cjs:35 cardFrameNavigationGuard:true；desktop/main.cjs:490-497 app.on('web-contents-created')→installCardFrameNavigationGuard；desktop/src/shared/card-frame-navigation.cjs:12（will-frame-navigate 拦截非同文档导航） | drop-with-reason（Tauri/Wry 无 will-frame-navigate 等价帧级导航拦截；保持该 flag 缺失/false，HTTP 卡片显式降级为仅同源——代码注释已预期此路径 InteractiveCard.tsx:58-59） | 无（同一 webview 内 iframe 导航拦截不是 ACL 能力） | high——现役真实能力无法直接迁移；若强行置 true 会造成卡片安全边界虚假通过，必须如实降级并记录（阶段门禁） |

### 4.11 系统权限（T04 范围）

| ID | API members | Electron 实现（file:line 函数） | Tauri 目标 | 权限 scope | 风险 |
|---|---|---|---|---|---|
| `host:speech-permission` | speechPermissionStatus, speechPermissionRequest | desktop/main.cjs:5716-5723 speech-permission-status/request（先 BrowserWindow.fromWebContents 校验 sender）；desktop/speech-permissions.cjs:44 createSpeechPermissionController()/SPEECH_PERMISSION_STATES:19/原生桥:172；hostCanPrompt 仅打包壳 true；desktop/preload.cjs:42-43 | custom-command:speech_permission_status/speech_permission_request + 自定义 Swift/ObjC TCC 桥（Tauri 无官方 speech 插件；R01 spike tcc_probe.swift 只读查询已验证） | 自定义 allow-speech-permission-*（仅拥有录音 UI 的窗口；request 只在用户手势路径调用） | high——T04 范围且受 BLOCKED-SYSTEM-PERMISSIONS；Tauri 无官方插件，需宿主内 Swift/ObjC 桥 |

## 5. 真实消费者索引（摘要）

| 能力 | 真实消费者（file:line；无则标注） |
|---|---|
| `host:single-instance` | `no-consumer-found（host-internal；R00 ENTRYPOINTS.json EP-DESK-05 登记）` |
| `host:main-window` | `no-consumer-found（host-internal；renderer 经 app-ready/window-controls 间接）` |
| `host:window-state` | `no-consumer-found（host-internal）` |
| `host:window-controls` | `desktop/src/react/components/WindowControls.tsx:24（onMaximizeChange）`；`desktop/src/react/components/WindowControls.tsx:26/29/30/31（isMaximized/minimize/maximize/close）` |
| `host:app-lifecycle-quit` | `no-consumer-found（host-internal）` |
| `host:app-ready` | `desktop/src/react/app-init.ts:211`；`desktop/src/react/app-init.ts:234`；`desktop/src/react/app-init.ts:246`；`desktop/src/react/app-init.ts:421` |
| `host:theme-sync` | `desktop/src/shared/theme.ts:28（hana?.syncWindowTheme?.）` |
| `host:i18n-main` | `desktop/src/react/services/app-event-actions.ts:309（locale-changed 源头）→ settingsChanged` |
| `host:reload-restart` | `desktop/src/react/settings/tabs/AccessTab.tsx:366`；`desktop/src/react/settings/tabs/AccessTab.tsx:390`；`desktop/src/react/app-init.ts:379（restartApp）` |
| `host:tray` | `no-consumer-found（host-internal；R00 ENTRYPOINTS EP-DESK-05）` |
| `host:global-shortcut` | `desktop/src/react/settings/tabs/KeybindingsTab.tsx:112（test probe）、:209（test-register）、:254/:257（reload）、desktop/src/react/settings/tabs/GeneralTab.tsx:179,:191（reload）`；`no-consumer-found：quickChatShortcutStatus 无渲染调用（R00 叶子登记但当前 React 未调用）` |
| `host:notification` | `desktop/src/react/services/ws-message-handler.ts:988`；`desktop/src/react/services/rust-automation.ts:298` |
| `host:keep-awake` | `desktop/src/react/settings/tabs/GeneralTab.tsx:227（setKeepAwakeEnabled）`；`no-consumer-found：getKeepAwakeStatus 无渲染调用（R00 叶子登记但当前 React 未调用）` |
| `host:auto-launch` | `desktop/src/react/settings/tabs/GeneralTab.tsx:101（get）`；`desktop/src/react/settings/tabs/GeneralTab.tsx:211（set）` |
| `host:file-dialogs` | `desktop/src/react/settings/tabs/SkillsTab.tsx:502`；`desktop/src/react/components/InputArea.tsx:1177`；`desktop/src/react/settings/tabs/SkillsTab.tsx:218`；`desktop/src/react/components/knowledge/KnowledgePage.tsx:546` |
| `host:clipboard-edit-command` | `desktop/src/react/components/InputContextMenu.tsx:205（copy）`；`desktop/src/react/components/InputContextMenu.tsx:230（通用）` |
| `host:drag-out` | `desktop/src/react/components/desk/DeskTree.tsx:562`；`desktop/src/react/components/chat/AssistantMessage.tsx:608`；`desktop/src/react/components/chat/AssistantMessage.tsx:694` |
| `host:file-drop-paths` | `desktop/src/react/MainContent.tsx:265`；`desktop/src/react/settings/tabs/SkillsTab.tsx:190`；`desktop/src/react/utils/markdown-cover-drop.ts:33` |
| `host:external-open` | `desktop/src/react/settings/tabs/McpTab.tsx:260`；`desktop/src/react/settings/tabs/AboutTab.tsx:372`；`desktop/src/react/settings/tabs/AboutTab.tsx:460`；`desktop/src/react/utils/link-open.ts:203`；`desktop/src/react/settings/tabs/AgentTab.tsx:221`；`desktop/src/react/components/InputArea.tsx:2378` |
| `host:trash` | `desktop/src/react/stores/desk-actions.ts:1223`；`desktop/src/react/stores/desk-actions.ts:1242` |
| `host:file-text-io` | `desktop/src/react/components/PreviewEditor.tsx:650（writeFile）`；`desktop/src/react/components/PreviewEditor.tsx:639（writeFileIfUnchanged）`；`desktop/src/react/components/PreviewEditor.tsx:344（writeFileBinary）`；`desktop/src/react/utils/markdown-cover-actions.ts:30（copyFile）`；`desktop/src/react/utils/screenshot.ts:304（readFileSnapshot）`；`desktop/src/react/MainContent.tsx:88（readFileBase64）`；`desktop/src/viewer-window-entry.tsx:111（readFile）` |
| `host:file-watch` | `no-consumer-found（React 生产代码无调用；测试：desktop/src/react/__tests__/components/PreviewEditor.file-sync.test.tsx:33、DeskSection.test.tsx:93-102；viewer 改用服务端 ResourceIO：desktop/src/viewer-resource-events.ts:91）` |
| `host:docx-xlsx-preview` | `desktop/src/react/utils/preview-file-content.ts:127（readDocxHtml）`；`desktop/src/react/utils/preview-file-content.ts:131（readXlsxHtml）` |
| `host:observability-export` | `desktop/src/react/settings/tabs/observability/observability-export-save.ts:39-41（能力探测）`；`desktop/src/react/settings/tabs/observability/observability-export-save.ts:70（begin）`；`desktop/src/react/settings/tabs/observability/observability-export-save.ts:84（write）`；`desktop/src/react/settings/tabs/observability/observability-export-save.ts:92（end）`；`desktop/src/react/settings/tabs/observability/observability-export-save.ts:95（abort）` |
| `host:file-url-asset` | `desktop/src/react/splash/SplashApp.tsx:71`；`desktop/src/react/utils/screenshot.ts:340`；`desktop/src/react/utils/markdown.ts:254` |
| `host:detached-viewer` | `desktop/src/react/stores/preview-actions.ts:41（spawn）`；`desktop/src/react/stores/preview-actions.ts:63（onViewerClosed）`；`desktop/src/viewer-window-entry.tsx:74/139` |
| `host:skill-viewer` | `desktop/src/react/settings/tabs/skills/SkillRow.tsx:64`；`desktop/src/react/utils/file-preview.ts:98`；`desktop/src/react/components/SkillViewerOverlay.tsx:64`；`desktop/src/react/components/SkillViewerOverlay.tsx:75`；`desktop/src/react/app-init.ts:415` |
| `host:browser-viewer` | `desktop/src/react/utils/link-open.ts:174`；`desktop/src/react/components/BrowserCard.tsx:70`；`desktop/src/react/services/rust-browser.ts:30/66/87`；`desktop/src/react/browser-viewer/BrowserViewerApp.tsx:54/82/139/150/163/174/185/213/214/238`；`desktop/src/react/browser-viewer/RustBrowserViewer.tsx:76`；`no-consumer-found：closeBrowser（PlatformApi 声明但 preload.cjs 未暴露，Electron 无实现；仅 web/mobile 宿主定义）` |
| `host:screenshot-render` | `desktop/src/react/utils/screenshot.ts:204/228/271（usePlatform() as any）` |
| `host:splash` | `desktop/src/react/splash/SplashApp.tsx:59/60`；`desktop/src/react/onboarding/OnboardingApp.tsx:117/126` |
| `host:onboarding` | `desktop/src/react/onboarding/OnboardingApp.tsx:94`；`desktop/src/react/onboarding/steps/TutorialStep.tsx:87`；`no-consumer-found：debugOpenOnboarding/debugOpenOnboardingPreview（preload-only 调试命令，当前 React 未调用）` |
| `host:settings-window` | `desktop/src/react/app-init.ts:407（onOpenSettingsModal）`；`desktop/src/react/settings/SettingsContent.tsx:191（onSwitchTab）`；`no-consumer-found：openSettings 无 React 调用点（主进程托盘 open-settings 路径是唯一入口）` |
| `host:settings-broadcast` | `desktop/src/react/settings/tabs/KeybindingsTab.tsx:251/362`；`desktop/src/react/settings/tabs/ExperimentsTab.tsx:416`；`desktop/src/react/settings/tabs/SecurityTab.tsx:119`；`desktop/src/react/settings/tabs/InterfaceTab.tsx:363`；`desktop/src/react/services/app-event-actions.ts:309/406/426/431/436`；`desktop/src/react/app-init.ts:383`；`desktop/src/react/settings/SettingsContent.tsx:201`；`desktop/src/react/browser-viewer/BrowserViewerApp.tsx:77` |
| `host:quick-chat-window` | `desktop/src/react/quick-chat/QuickChatApp.tsx:475/650/660（resize）`；`desktop/src/react/quick-chat/QuickChatApp.tsx:752（hide）`；`desktop/src/react/quick-chat/QuickChatApp.tsx:479（onQuickChatShown）`；`no-consumer-found：quickChatShow 无渲染调用（由全局快捷键/内部 toggle 驱动，main.cjs:3029）` |
| `host:quick-chat-session-handoff` | `desktop/src/react/quick-chat/QuickChatApp.tsx:719（发起）`；`desktop/src/react/app-init.ts:412（主窗确认）` |
| `host:server-connection-info` | `desktop/src/react/app-init.ts:178（connectionInfo）`；`desktop/src/viewer-resource-events.ts:53/55（port/token）`；`desktop/src/react/app-init.ts:110（onServerRestarted）`；`desktop/src/react/settings/SettingsContent.tsx:237（onServerRestarted）` |
| `host:app-info-platform` | `desktop/src/react/components/WindowControls.tsx:20`；`desktop/src/modules/platform.js:21` |
| `host:update-github-check` | `desktop/src/react/hooks/use-release-check.ts:41`；`desktop/src/react/components/PostUpdateAnnouncement.tsx:54/68`；`desktop/src/react/settings/update-history-actions.ts:11` |
| `host:update-auto` | `desktop/src/react/settings/tabs/AboutTab.tsx:365/376/393`；`desktop/src/react/hooks/use-auto-update-state.ts:63/71`；`desktop/src/react/components/notices/SidebarNoticeSlot.tsx:356` |
| `host:update-train-ota` | `desktop/src/react/hooks/use-train-update-state.ts:110/134/147/169/178/200/223` |
| `security:capability-sets` | `no-consumer-found（无对应 API；T06 新增安全面）` |
| `security:csp` | `no-consumer-found（配置面）` |
| `security:command-acl` | `no-consumer-found（配置面）` |
| `security:release-no-test-control` | `no-consumer-found（preload-only 调试命令也无人调用）` |
| `security:remote-content-isolation` | `no-consumer-found（安全边界面）` |
| `security:no-arbitrary-shell` | `no-consumer-found（安全边界面）` |
| `security:token-exposure` | `no-consumer-found（安全边界面）` |
| `host:card-frame-navigation-guard` | `desktop/src/react/components/chat/InteractiveCard.tsx:56（if (usePlatform()?.cardFrameNavigationGuard === true) return true）` |
| `host:context-menu-policy` | `no-consumer-found（host 行为）` |
| `host:speech-permission` | `no-consumer-found（生产无 renderer 调用；R00 同结论；T04 范围）` |

## 6. Tauri 缺口与需要新增的自定义命令清单

共 **91** 条；`♻` = R06 `desktop/src-tauri` 已实现/可直接复用（20 条），其余为新增（71 条）。建议命令名保持 snake_case，全部经 `build.rs` AppManifest + `permissions/autogenerated/<name>.toml` 注册（R06 模板：`desktop/src-tauri/build.rs:1-33`、`desktop/src-tauri/permissions/autogenerated/*.toml`），并在目标窗口 capability 中显式 allow。

| 命令 | 归属能力 | R06 | 备注 |
|---|---|---|---|
| `app_ready` | `host:app-ready` |  | 主窗 label 校验 + 幂等 |
| `window_minimize` | `host:window-controls` |  | 只作用于调用者窗口 |
| `window_maximize` | `host:window-controls` |  | 只作用于调用者窗口 |
| `window_close` | `host:window-controls` |  | win/linux 自绘标题栏 |
| `window_is_maximized` | `host:window-controls` |  | win/linux 自绘标题栏 |
| `sync_window_theme` | `host:theme-sync` |  | theme id 白名单；quick-chat 透明特例 |
| `reload_main_window` | `host:reload-restart` |  | 仅主窗 |
| `restart_app` | `host:app-lifecycle-quit` |  | 复用退出清理路径 |
| `get_platform` | `host:app-info-platform` |  | 返回 darwin/win32/linux 字面值 |
| `get_app_version` | `host:app-info-platform` |  | 壳版本，非内容版本 |
| `open_settings` | `host:settings-window` |  | 主窗存在→modal 事件；否则独立窗 |
| `settings_changed` | `host:i18n-main` |  | 事件名白名单 + side effect |
| `get_splash_info` | `host:splash` |  | splash/onboarding 窗口白名单 |
| `get_avatar_path` | `host:splash` |  | 窗口+页面 URL 白名单，15MB/类型校验 |
| `onboarding_complete` | `host:onboarding` |  | 保存失败不得进主界面 |
| `debug_open_onboarding` | `host:onboarding` |  | release 剔除（OQ-06） |
| `debug_open_onboarding_preview` | `host:onboarding` |  | release 剔除（OQ-06） |
| `quick_chat_show` | `host:quick-chat-window` |  | 全局快捷键/内部触发 |
| `quick_chat_hide` | `host:quick-chat-window` |  | 快聊窗口 |
| `quick_chat_resize` | `host:quick-chat-window` |  | mode/height 白名单 |
| `quick_chat_reload_shortcut` | `host:global-shortcut` |  | 多组键位、冲突回执 |
| `keybindings_reload_global` | `host:global-shortcut` |  | 多组键位、冲突回执 |
| `keybindings_test_register` | `host:global-shortcut` |  | 注册后立即注销 |
| `quick_chat_shortcut_status` | `host:global-shortcut` |  | 只读状态 |
| `quick_chat_open_session` | `host:quick-chat-session-handoff` |  | 仅快聊窗；15s 超时 |
| `quick_chat_open_session_ack` | `host:quick-chat-session-handoff` |  | 仅主窗；焦点确认后 hide |
| `show_notification` | `host:notification` |  | focus 抑制；真实 show 回执 |
| `get_keep_awake_status` | `host:keep-awake` |  | 三平台原生实现（OQ-09） |
| `set_keep_awake_enabled` | `host:keep-awake` |  | 失败不得假报 |
| `get_auto_launch_status` | `host:auto-launch` |  | 回显系统真实状态 |
| `set_auto_launch_enabled` | `host:auto-launch` |  | 平台不支持 supported:false |
| `select_folder` | `host:file-dialogs` | ♻ | 回调 API，禁用 blocking 变体 |
| `select_files` | `host:file-dialogs` | ♻ | multiple 语义 |
| `select_skill` | `host:file-dialogs` |  | zip/skill 过滤 + 目录 |
| `observability_export_begin` | `host:observability-export` |  | 保存对话框→exportId |
| `observability_export_write` | `host:observability-export` |  | ≤4MB/绑定窗口 |
| `observability_export_end` | `host:observability-export` |  | 关闭 fd |
| `observability_export_abort` | `host:observability-export` |  | 删除部分文件 |
| `read_file` | `host:file-text-io` | ♻ | 绝对路径/5MB |
| `read_file_snapshot` | `host:file-text-io` | ♻ | sha256 版本 |
| `read_file_base64` | `host:file-text-io` | ♻ | 20MB |
| `write_file` | `host:file-text-io` | ♻ | 绝对路径 |
| `write_file_binary` | `host:file-text-io` | ♻ | base64/目录创建 |
| `write_file_if_unchanged` | `host:file-text-io` | ♻ | CAS 冲突 |
| `copy_file` | `host:file-text-io` | ♻ | 拒 symlink |
| `read_docx_html` | `host:docx-xlsx-preview` |  | 20MB + 服务切换检测 |
| `read_xlsx_html` | `host:docx-xlsx-preview` |  | 20MB + 服务切换检测 |
| `watch_file` | `host:file-watch` | ♻ | notify/轮询；共享订阅 |
| `unwatch_file` | `host:file-watch` | ♻ | 按窗口隔离 |
| `watch_workspace` | `host:file-watch` |  | 忽略规则/递归 |
| `unwatch_workspace` | `host:file-watch` |  | 按窗口隔离 |
| `run_edit_command` | `host:clipboard-edit-command` | ♻ | 4 枚举白名单；firstResponder |
| `open_external` | `host:external-open` |  | 仅 http/https + 回执 |
| `open_folder` | `host:external-open` |  | 绝对目录 |
| `open_file` | `host:external-open` | ♻ | 绝对普通文件 |
| `show_in_finder` | `host:external-open` |  | 绝对路径 |
| `trash_item` | `host:trash` | ♻ | resolveTrashItemPath；系统废纸篓 |
| `start_drag` | `host:drag-out` |  | 绝对路径；icon 非空 |
| `spawn_viewer` | `host:detached-viewer` | ♻ | payload 主进程持有 |
| `viewer_request_load` | `host:detached-viewer` | ♻ | 按 sender 反推 windowId |
| `viewer_close` | `host:detached-viewer` | ♻ | 仅 viewer 窗 |
| `open_skill_viewer` | `host:skill-viewer` | ♻ | Rust zip 替换 unzip/powershell（T06） |
| `list_skill_files` | `host:skill-viewer` | ♻ | 目录白名单 |
| `read_skill_file` | `host:skill-viewer` | ♻ | 2MB 文本上限 |
| `open_browser_viewer` | `host:browser-viewer` |  | T05 形态未定 |
| `set_browser_connection` | `host:browser-viewer` |  | 仅主窗 |
| `get_browser_viewer_context` | `host:browser-viewer` |  | 连接句柄 |
| `rust_browser_request` | `host:browser-viewer` |  | 固定服务端路由/动作枚举 |
| `browser_go_back` | `host:browser-viewer` |  | sessionPath 校验 |
| `browser_go_forward` | `host:browser-viewer` |  | sessionPath 校验 |
| `browser_reload` | `host:browser-viewer` |  | sessionPath 校验 |
| `browser_new_tab` | `host:browser-viewer` |  | url 仅 http(s) |
| `browser_switch_tab` | `host:browser-viewer` |  | tabId 校验 |
| `browser_close_tab` | `host:browser-viewer` |  | tabId 校验 |
| `close_browser_viewer` | `host:browser-viewer` |  | 仅 browser-viewer |
| `browser_emergency_stop` | `host:browser-viewer` |  | 销毁+撤销授权 |
| `get_server_connection_info` | `host:server-connection-info` |  | token 不落 URL；T02 持 state |
| `release_check_latest` | `host:update-github-check` |  | 固定 GitHub 端点+60s 缓存 |
| `get_pending_announcement` | `host:update-github-check` |  | 内容版本书签 |
| `ack_announcement` | `host:update-github-check` |  | 写 last-seen-version（内容版本） |
| `get_update_digest_history` | `host:update-github-check` |  | 在线失败回退包内 |
| `auto_update_check` | `host:update-auto` |  | 固定更新源+签名 |
| `auto_update_install` | `host:update-auto` |  | 签名/磁盘校验 |
| `auto_update_state` | `host:update-auto` |  | 状态投影 |
| `auto_update_set_channel` | `host:update-auto` |  | stable/beta 枚举 |
| `train_update_status` | `host:update-train-ota` |  | 契约版本闸门 |
| `train_update_check` | `host:update-train-ota` |  | 只 check 不下载 |
| `train_update_apply` | `host:update-train-ota` |  | 停 server→激活→重启 |
| `speech_permission_status` | `host:speech-permission` |  | 只读不弹窗 |
| `speech_permission_request` | `host:speech-permission` |  | 仅用户手势；打包壳 |
| `screenshot_render` | `host:screenshot-render` |  | T04；主题白名单/输出目录 |

## 7. R09-T06 收口设计输入

1. **按窗口 capability 分组（A11）**：R06 只有 `main`、`viewer-*`、`skill-viewer-*` 三组（`desktop/src-tauri/capabilities/*.json`）。R09 至少需要 `main`、`settings`、`quick-chat`、`browser-viewer`、`viewer-*`、`skill-viewer-*`、`splash`、`onboarding` 八组；远程内容 webview 不进入任何 capability。A11 要求机器展开「命中 capability 并集 == 白名单」，不能人工审阅单文件；R06 已把展开结果落盘为 `desktop/src-tauri/gen/schemas/capabilities.json`，可作为比对输入。
2. **命令 ACL（A11/A12）**：四份集合必须一致——`invoke_handler`（`desktop/src-tauri/src/lib.rs:534-561`）、`build.rs` AppManifest（`build.rs:1-33`）、`permissions/autogenerated/*.toml`（R06 26 个命令权限文件）、capability 引用。R09 命令面约 91 条，必须脚本化比对并把「空集合/漏权限」做成非零失败（R09 任务书 05 §3 的要求）。
3. **CSP / asset / 导航**：现役 Electron 只有部分页面有 CSP meta（browser-viewer/quick-chat/splash/onboarding/viewer-window/mobile；`index.html`/`settings.html` 无 CSP），靠 `will-navigate` 拦截外链（`desktop/main.cjs:3237-3246/3358-3367`）。Tauri 必须在 `tauri.conf.json app.security.csp` 收紧并覆盖：`connect-src` 精确到 `http://127.0.0.1:*`/`ws://127.0.0.1:*`、`img/media-src` 允许 `asset:`/`data:`/`blob:`、`frame-src` 仅 `'self' blob:`、`object-src 'none'`、禁 `unsafe-eval`/远程脚本；R06 模板 CSP 见 `desktop/src-tauri/tauri.conf.json:11`。注意 Windows 的 `http://ipc.localhost` / `http://asset.localhost` 与 Linux/mac 的 scheme 不同。
4. **生产无测试控制面（A12）**：R06 的 `e2e-test` feature + `tauri-plugin-wdio-webdriver` optional 依赖 + 二进制 strings 探针是可复用模式（`desktop/src-tauri/Cargo.toml:27`、`lib.rs:594-596`、`tests/desktop_host.rs`）。必须同时处理：release 不接受 `--dev`/不开放 devtools、`debug_open_onboarding*` 在 release capability 不授权且二进制不存在、`withGlobalTauri` 建议置 false（R06 当前为 true，`tauri.conf.json:6`）。
5. **无任意 shell / sidecar 收口**：现役 Electron 无通用 shell IPC，但 `open-skill-viewer` 用 `execFileSync('unzip')`/`powershell Expand-Archive`（`desktop/main.cjs:6165-6175`）是真实命令面；Tauri 侧改为 Rust `zip` crate（路径穿越/压缩比/文件数限制），sidecar（如有）仅固定二进制+固定参数，capability 零 shell 权限（R01 spike 模式）。
6. **token 与远程内容**：`get-server-token` 现役无 sender 校验（`desktop/main.cjs:5646-5647`）；Tauri 必须按窗口 min-allow + managed state，token 不出现在 URL/query/DOM（A10）。远程/预览内容零 capability，asset scope 不覆盖远程内容。

## 8. 平台矩阵

| 平台 | target triple | WebView | 状态 | 阻塞登记 |
|---|---|---|---|---|
| macOS arm64 | `aarch64-apple-darwin` | WKWebView | LOCAL_VERIFIABLE | — |
| macOS x64 | `x86_64-apple-darwin` | WKWebView | BLOCKED_PLATFORM | BLOCKED-PLATFORM-MACOS-X64 |
| Windows x64 | `x86_64-pc-windows-msvc` | WebView2 (Chromium) | BLOCKED_PLATFORM | BLOCKED-PLATFORM-WINDOWS-X64 |
| Linux x64 | `x86_64-unknown-linux-gnu` | WebKitGTK | BLOCKED_PLATFORM | BLOCKED-PLATFORM-LINUX-X64 |

**macOS arm64**：Mac15,14 macOS 27.0 arm64（R09 START_RECEIPT.md:6；R01 PLATFORM_BUILD_MATRIX.json VERIFIED）；工具链：rustup 1.98.1（rust-toolchain.toml）+ node v24.16.0（START_RECEIPT.md:5）。
- tray：template image + 双击/右键菜单（main.cjs:2389-2420）
- global-shortcut：合成 CGEvent 需辅助功能授权（R01 spike 已验证）
- notification：首次授权 not-determined 探测（main.cjs:6640-6647）；点击聚焦需平台补充
- auto-launch：SMAppService，需正式签名（BLOCKED-SIGNING-IDENTITY）
- drag-out：startDragging 需非空 icon（main.cjs:6238-6255）
- window-controls：trafficLightPosition 原生按钮，不渲染自绘控件
- asset/file URL：需 asset protocol scope 与 CSP 对齐
- speech/screenshot/computer-use 权限归 T04 且受 BLOCKED-SYSTEM-PERMISSIONS
- 阻塞：BLOCKED-SIGNING-IDENTITY（正式签名/公证缺失，ad-hoc 仅本地功能证据）, BLOCKED-SYSTEM-PERMISSIONS（TCC 撤销-恢复演练需人工）

**macOS x64**：无可用真机/VM（BLOCKED.md:8）；工具链：可从 arm64 交叉编译，不能替代原生 E2E/权限/安装验证。
- T03/T06 全部 macOS x64 场景不可执行
- 同一 WKWebView 代码，但通知/托盘/快捷键/自启动/TCC 需真机
- 阻塞：BLOCKED-PLATFORM-MACOS-X64

**Windows x64**：无可用机器（BLOCKED.md:9）；工具链：交叉编译不受官方支持；需 Windows SDK+MSVC（R01 PLATFORM_BUILD_MATRIX.json）。
- window-controls：frame:false 自绘 + maximize/unmaximize 事件（main.cjs:3248-3249）
- tray：.ico 优先；notification 自定义 icon 依赖 AppUserModelID（main.cjs:378）
- auto-launch：Run 键 + --hana-start-at-login（login-item-settings.cjs:1-15）
- keep-awake：SetThreadExecutionState；drag-out：OLE DragDrop
- file picker：IFileDialog；open-external：ShellExecute
- ACL/asset protocol 在 Win 上 origin 为 http://asset.localhost，需 CSP 对齐
- 阻塞：BLOCKED-PLATFORM-WINDOWS-X64

**Linux x64**：无可用机器（BLOCKED.md:10）；工具链：需 gcc/cc + WebKitGTK 系统依赖（R01 PLATFORM_BUILD_MATRIX.json UNVERIFIED）。
- tray：依赖 libappindicator/StatusNotifier，部分桌面环境不可用→需显式降级诊断
- global-shortcut：X11/Wayland 差异大（Wayland 全局快捷键受限）
- 透明窗口/置顶（quick-chat）在部分 WM 失败
- keep-awake：systemd-inhibit/DBus，权限与兼容性未实测
- asset protocol origin asset://localhost；CSP 需对齐
- Linux 原生 E2E 需 xvfb/真实桌面，不得用截图替代 win/mac 验证
- 阻塞：BLOCKED-PLATFORM-LINUX-X64, BLOCKED-SYSTEM-PERMISSIONS（若涉及屏幕/麦克风）

平台缺失一律记 `BLOCKED_PLATFORM`（R09 BLOCKED.md），不用交叉编译或他平台截图替代原生/权限/安装验证。

## 9. 46 叶关联表（T03/T07 共享叶，去重后 46）

| 叶 feature_id | 标题 | 映射到能力 / 非宿主理由 |
|---|---|---|
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-APP-RESTART-9350C3` | 重启桌面应用 | `host:reload-restart`, `host:app-lifecycle-quit` |
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-AUTO-LAUNCH-A2E507` | 设置开机启动 | `host:auto-launch` |
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-DEBUG-OPEN-ONBOARDING-1C4152` | 打开引导调试界面 | `host:onboarding`, `security:release-no-test-control` |
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-DEBUG-OPEN-ONBOARDING-PREVIEW-1E6EDB` | 预览引导界面 | `host:onboarding`, `security:release-no-test-control` |
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-DETACHED-VIEWER-0DEEE4` | 打开独立文件预览 | `host:detached-viewer` |
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-GET-SPLASH-INFO-AC5B6E` | 加载启动画面资料 | `host:splash` |
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-GLOBAL-SHORTCUT-5D5938` | 设置全局快捷键 | `host:global-shortcut` |
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-KEEP-AWAKE-B72A6D` | 设置保持唤醒 | `host:keep-awake` |
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-ONBOARDING-COMPLETE-1120B2` | 完成首次使用引导 | `host:onboarding` |
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-OPEN-EXTERNAL-ACA2C4` | 在系统浏览器打开链接 | `host:external-open` |
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-OPEN-SETTINGS-57728C` | 打开设置窗口 | `host:settings-window` |
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-OPEN-SETTINGS-MODAL-B6A97A` | 从宿主打开设置弹窗 | `host:settings-window`, `host:settings-broadcast` |
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-READ-DOCX-HTML-37E6C3` | 预览 Word 文件 | `host:docx-xlsx-preview` |
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-READ-XLSX-HTML-E84F01` | 预览表格文件 | `host:docx-xlsx-preview` |
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-RELOAD-MAIN-WINDOW-322E02` | 重新加载主窗口 | `host:reload-restart` |
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-SETTINGS-CHANGED-15E163` | 同步跨窗口设置 | `host:settings-broadcast` |
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-SETTINGS-SWITCH-TAB-FF1B33` | 切换设置页 | `host:settings-window` |
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-SHOW-NOTIFICATION-DF4B51` | 显示系统通知 | `host:notification` |
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-START-DRAG-15DFB1` | 拖出本地文件 | `host:drag-out` |
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-WINDOW-CONTROLS-F97864` | 控制桌面窗口 | `host:window-controls` |
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-WINDOW-READINESS-58822B` | 初始化桌面窗口 | `host:main-window`, `host:app-ready`, `host:window-state`, `host:app-lifecycle-quit` |
| `F-D23-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-WINDOW-THEME-CHANGED-D29DE4` | 同步窗口主题 | `host:theme-sync` |
| `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-COMMANDS-COMMANDS-1D4C7E` | 查看可用斜杠命令 | 非宿主叶：server route /api/commands；host 只消费 HTTP，不新增桌面命令（T07 用原生 WebView 验证） |
| `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-CONFIG-USER-PROFILE-PUT-1629CD` | 修改用户档案 | 非宿主叶：server config API；T07 原生 WebView E2E 覆盖 |
| `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-CONFIG-USER-PROFILE-READ-429D86` | 查看用户档案 | 非宿主叶：server config API；T07 原生 WebView E2E 覆盖 |
| `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-ENV-DEPS-SYSTEM-ENV-DEPS-READ-CD873C` | 查看环境依赖 | 非宿主叶：server route env-deps；host 无桌面原生面 |
| `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-ENV-DEPS-SYSTEM-ENV-DEPS-REFRESH-173C63` | 刷新环境依赖 | 非宿主叶：server route env-deps；host 无桌面原生面 |
| `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-APPEARAN-18FE3F` | 读取外观偏好 | `host:theme-sync` |
| `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-APPEARAN-F5DBF2` | 更新外观偏好 | `host:theme-sync` |
| `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-KEYBINDI-394DD0` | 更新快捷键偏好 | `host:global-shortcut` |
| `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-KEYBINDI-98B458` | 读取快捷键偏好 | `host:global-shortcut` |
| `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-LEGACY-G-70B07E` | 清理旧 GPU 安全模式偏好 | 非宿主叶：server preferences API + 启动数据闸门；宿主无新增命令 |
| `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-NOTIFICA-D15E0A` | 更新通知偏好 | `host:notification` |
| `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-NOTIFICA-E6FB08` | 读取通知偏好 | `host:notification` |
| `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-SETUP-CO-6774E7` | 标记初次设置完成 | `host:onboarding` |
| `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-SIDEBAR--8FE26C` | 读取侧栏偏好 | 非宿主叶：server preferences API；T07 原生 WebView E2E 覆盖 |
| `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-SIDEBAR--FB5C78` | 更新侧栏偏好 | 非宿主叶：server preferences API；写者唯一 server（R00 R-T04-06） |
| `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-WORKSPAC-599876` | 读取工作区界面状态 | 非宿主叶：server preferences API；T07 原生 WebView E2E 覆盖 |
| `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PREFERENCES-PREFERENCES-WORKSPAC-C5AAF3` | 更新工作区界面状态 | 非宿主叶：server preferences API；写者唯一 server |
| `F-D23-SEMANTIC_EFFECT-SEMANTIC-EFFECT-SETTINGS-SNAPSHOT-SETTINGS-SNAPS-F62357` | 查看智能体设置快照 | 非宿主叶：server settings-snapshot API；T07 UI |
| `F-D23-UI-UI-SETTINGS-ENVDEPS-713E90` | 设置页 · envdeps | 非宿主叶：React 设置页；T07 UI |
| `F-D23-UI-UI-SETTINGS-GENERAL-A14D30` | 设置页 · general | 非宿主叶：React 设置页；T07 UI |
| `F-D23-UI-UI-SETTINGS-INTERFACE-1329EA` | 设置页 · interface | 非宿主叶：React 设置页；T07 UI |
| `F-D23-UI-UI-SETTINGS-KEYBINDINGS-9C679D` | 设置页 · keybindings | 非宿主叶：React 设置页；与 host:global-shortcut 联动但不新增命令面 |
| `F-D23-UI-UI-SETTINGS-ME-6006FD` | 设置页 · me | 非宿主叶：React 设置页；T07 UI |
| `F-D23-UI_BEHAVIOR-UI-BEHAVIOR-FIVE-LANGUAGE-BEC207` | 五语言界面切换 | `host:i18n-main` |

注：46 叶中 22 条是 `DESKTOP_BEHAVIOR`（宿主能力直接相关），24 条是 `SEMANTIC_EFFECT`/`UI`（server/React 面，T03 只涉及事件广播或 i18n，主要靠 T07 原生 E2E）。R00 没有 tray/单实例/菜单/窗口状态的独立叶子（仅 `ENTRYPOINTS.json EP-DESK-05` 复合登记），因此这几项能力 `r09Leaves` 为空并以 T03 任务书 §4-1 为依据——这是 R00 清点边界，不是本清单遗漏。

## 10. OpenQuestions（实施前需裁决）

| ID | 问题 | 依据 | 影响能力 | 建议 |
|---|---|---|---|---|
| OQ-01 | T03 是否必须迁移 DeepLink/文件关联？R00 明示无深链协议注册，package.json 无 protocols/fileAssociations。 | docs/rust-tauri/R00/OWNERSHIP_CURRENT.md:27；package.json build 键（无 protocols/fileAssociations） | `host:single-instance`, `host:main-window` | 按「未采纳」处理：不新增 tauri-plugin-deep-link/文件关联；若产品要，另立任务与验收。 |
| OQ-02 | Tauri 插件版本如何与 R06 的 tauri 2.12.1/tauri-build 2.7.2 对齐？R01 spike 用的是 2.11.6 + notification 2.4.0/global-shortcut 2.3.2/autostart 2.5.1/updater 2.12.0/shell 2.3.6/dialog 2.7.3。 | desktop/src-tauri/Cargo.toml:12-22；docs/rust-tauri/R01/DEPENDENCY_DECISIONS.md D-11/D-13；docs/rust-tauri/R01/PLATFORM_BUILD_MATRIX.json desktop_prototype_r01_t06 | `host:tray`, `host:global-shortcut`, `host:notification`, `host:auto-launch`, `host:update-auto`, `security:capability-sets` | R09 开始时以 R06 锁文件为基线补装插件并生成新 Cargo.lock；逐插件记录版本与 MSRV（opener 要求 rust ≥1.90，本仓 1.98.1 满足），不得照抄 spike 版本。 |
| OQ-03 | Tauri 通知点击聚焦主窗能否等价实现？（官方插件 JS 只有 onAction/onNotificationReceived，无通用 desktop click 回调） | https://v2.tauri.app/reference/javascript/notification/；desktop/main.cjs:6561-6568（Electron notif.on('click')） | `host:notification` | 先 spike：mac NSUserNotificationCenter delegate/UNUserNotificationCenter、win WinToast/Toast 事件、linux 通知 action；无法等价的平台在 A05/叶子证据中如实降级并登记。 |
| OQ-04 | Tauri 页截图（screenshot_render）方案未定：Wry 无稳定跨平台 capturePage；是否改用服务端渲染/平台 API？ | desktop/main.cjs:5377 screenshotCapture（offscreen BrowserWindow capturePage）；desktop/src/react/utils/screenshot.ts:228 | `host:screenshot-render` | T04 先 spike（wry 平台钩子/WebView2 CapturePreview/WKWebView takeSnapshot）；不可行则登记 drop-with-reason 并影响 A07 证据。 |
| OQ-05 | file watch 命令在 React 生产已无消费者（改用服务端 ResourceIO），T03 是否保留 parity 命令？ | desktop/src/viewer-resource-events.ts:91；desktop/src/react/__tests__/components/PreviewEditor.file-sync.test.tsx:33 | `host:file-watch` | 保留低成本 parity 命令（notify crate）供 T06 A16「所有原生动作入口」遍历；若成本高则登记 drop-with-reason 并说明服务端替代路径。 |
| OQ-06 | debugOpenOnboarding/debugOpenOnboardingPreview 在 release 是否保留？T06 要求生产无测试控制面，但 46 叶要求保留该功能。 | desktop/main.cjs:6573-6590（无环境门控）；R09-T06 任务书 §4-5；46 叶 F-D23-...-DEBUG-OPEN-ONBOARDING-1C4152 | `host:onboarding`, `security:release-no-test-control` | 功能保留在 dev/test 构建；release 二进制不含该命令（capability 不授予 + cfg 剔除）；在 T07 证据中只用 test 构建验证该叶，release 侧登记 A12 负向。 |
| OQ-07 | Tauri 无法直接迁移卡片 iframe 导航 guard（will-frame-navigate 无等价），HTTP 卡片如何处置？ | desktop/main.cjs:490-497；desktop/src/shared/card-frame-navigation.cjs:12；desktop/src/react/components/chat/InteractiveCard.tsx:56-59 | `host:card-frame-navigation-guard`, `security:remote-content-isolation` | 保持 flag=false（仅同源卡片）并登记为已知差异；A10 仍需恶意内容测试；不得把 flag 置 true 制造假安全。 |
| OQ-08 | quick-chat 透明窗/置顶/多工作区在 Tauri（尤其 Linux/Wayland）能否等价？ | desktop/main.cjs:2937-2984；desktop/main.cjs:3010-3026 | `host:quick-chat-window` | mac 先验；win/linux 平台阻塞期间按 BLOCKED_PLATFORM 记录，不用空实现冒充。 |
| OQ-09 | keep-awake 无官方 Tauri 插件，三平台自写原生实现的范围与证据如何定？ | desktop/keep-awake.cjs:5；R09-T03 任务书 §4-1 明确要求保持唤醒 | `host:keep-awake` | 以 power assertion 探针为证据（pmset/powercfg/systemd-inhibit）；Linux 不支持时 unsupported 显式返回。 |
| OQ-10 | Tauri capability 如何覆盖「同 webview 多 capability 叠加」的 A11 场景？present window 是否需要多条 capability 命中来复现？ | R09-T06 任务书 §4-1/A11；R01 spike ACL 负向案例（SHELL_CAPABILITY_MATRIX.json acl_security） | `security:capability-sets`, `security:command-acl` | R09 建立 capability 展开脚本 + 负向矩阵；A11 必须显式构造叠加用例（如 main 同时命中 baseline+platform-extra），不能只验单文件。 |
| OQ-11 | R09 桌面宿主完整版继续用 R06 的 desktop/src-tauri（26 命令/3 capability）扩展，还是新建目录？tauri.conf window 列表如何承载 8 类窗口？ | desktop/src-tauri/tauri.conf.json:1-15；desktop/src-tauri/src/lib.rs:530-598；R09-T01 任务书（独立 Tauri 工程/开发与 release 配置分离） | `security:capability-sets`, `host:main-window` | 沿用 R06 独立 manifest 并扩展（R01 ADR-001 §5 指定路径）；window 由 Rust setup 动态创建 + tauri.conf 只留 main/最小静态配置；capabilities 按 label glob 分组。 |
| OQ-12 | R00 ENTRYPOINTS.json 的 main.cjs 锚点行号已过期（如 createMainWindow 登记 2888，当前 3129）；本清单是否需要回写 R00 还是只在 R09 记录？ | docs/rust-tauri/R00/ENTRYPOINTS.json EP-DESK-03 anchors[0].line=2888；desktop/main.cjs:3129 | `全部 host:*` | R00 已密封不回写；R09 清单记录当前行号并注明 R00 锚点为历史 HEAD，供后续按 R09 清单复核。 |
| OQ-13 | server /preferences.json 单写者约束下，Tauri 宿主只能读不能写；窗口状态/quick-chat 状态等壳自态是否允许继续写 LINGXI_HOME/user/*.json？ | docs/rust-tauri/R00/OWNERSHIP_CURRENT.md:45（desktop 只读 preferences.json；壳自态 5 store 可写）；docs/rust-tauri/R01/ADR-001-ownership.md:47 | `host:window-state`, `host:quick-chat-window`, `host:auto-launch` | 壳自态（window-state.json/quick-chat-window-state.json/last-seen-version.json 等）继续由 tauri-host 写；preferences.json 保持只读（写走 server API）。 |
| OQ-14 | A05 要求系统级录屏和自动测试；mac 上通知/托盘/快捷键/TCC 的自动化证据具体形态与保存位置？ | R09_Tauri...md:141-149（A05 证据：系统级录屏和自动测试）；desktop/src-tauri/README.md 阶段观察文件模式 | `host:tray`, `host:global-shortcut`, `host:notification`, `host:drag-out`, `host:file-dialogs` | 沿用 R06 外部观察 JSON + 截图 + wdio 驱动互证模式（desktop/src-tauri/tests/desktop_host.rs:108-137），录屏存 artifacts/rust-tauri/R09/；平台缺失按 BLOCKED_PLATFORM 登记。 |

## 11. 关键校验与证据要点（高风险项）

### `host:app-lifecycle-quit`

- 校验：窗口全关时若无托盘→退出；有托盘→常驻；显式退出只清理 owned server，远端复用 server 不动；before-quit 先 hide 所有窗；退出写残留诊断
- 平台差异：mac dock 点击激活/隐藏；win 托盘双击；linux 托盘支持不一；OS 登出路径由系统信号触发
- 证据计划：进程树探针：关窗→宿主存活/窗口隐藏；托盘退出→宿主+owned server 全退出，远端 server PID 不变（A04）；日志残留项
- 风险：high——R09-T02 supervisor 与退出清理强耦合，须 T02 先定协议；错误实现会杀远端服务（A04 失败）

### `host:tray`

- 校验：菜单项固定 5 项（show/settings/separator/repair/separator/quit 语义）；图标路径只读打包资源；点击目标 window label 白名单
- 平台差异：mac template image（setTemplateImage）；win .ico 优先回退 png；linux 需要 libappindicator 支持；Linux 部分桌面环境托盘不可用→需降级诊断
- 证据计划：系统级：CGEvent 双击托盘图标 + 录屏；菜单项点击后窗口/进程探针；mac 状态栏截图；win/linux 平台阻塞
- 风险：high——托盘是 T03 显式要求但 R00 无独立叶子（仅 EP-DESK-05 复合登记）；Tauri 托盘菜单事件需与窗口隐藏/退出策略联动

### `host:global-shortcut`

- 校验：键位串长度/格式白名单；最多 3 组；冲突探测注册后立即注销；不得把 renderer 传的任意键当作已注册；不可写 preferences（写者唯一 server）
- 平台差异：mac 辅助功能/TCC 影响合成与注册；win RegisterHotKey 冲突语义；linux X11/Wayland 差异大（Wayland 全局快捷键受限→需 portal/明确阻塞）
- 证据计划：设置页保存→注册状态探针（is_registered）；合成 CGEvent 触发 quick-chat 显示（R01 spike post_shortcut.swift 模式）+ 录屏；冲突键位负向
- 风险：high——R01 spike 仅验证单键位；多组/冲突/抢占与 Wayland 限制未迁移；Tauri 插件对多 accelerator 语义需实测

### `host:notification`

- 校验：title/body 长度上限；agentId 只作头像解析键（validAgentId 规则）；options 只接受 desktopFocusPolicy 枚举；返回 {shown,reason} 回执；主窗聚焦时抑制；OS 不支持返回 unsupported
- 平台差异：mac 通知授权状态/首次请求；win AppUserModelID+自定义 icon；linux 通知服务器能力差异；各平台点击回调语义不同（官方插件 JS 只有 onAction/onNotificationReceived，无通用 click 回调）
- 证据计划：真实通知投递：mac 通知中心截图/Console 探针 + 点击后窗口聚焦断言；focus 抑制负向；OS 不支持路径；R01 spike notification.probe 模式
- 风险：high——「点击通知聚焦主窗」在 Tauri 无官方通用 click 回调，需平台补丁或降级并在 A05/叶子断言中如实记录

### `host:skill-viewer`

- 校验：baseDir/filePath 绝对路径；安装目录白名单 LINGXI_HOME/skills；解压限制（文件数/总大小/zip-slip 路径穿越，禁 symlink）；只读文本 2MB；不安装技能
- 平台差异：无平台差异；Windows 上现役 powershell 依赖必须移除（T06 no-shell 要求）
- 证据计划：真实 .skill/.zip 样本→overlay 显示；zip-slip/超大压缩包负向；读取非自身目录负向；R06 skill_viewer_host.rs 模式
- 风险：high——现役 zip 解压调用外部 unzip/powershell（main.cjs:6165-6175）与 T06 禁止泛化 shell 冲突，Tauri 必须改 Rust zip 实现并做路径穿越负向

### `host:browser-viewer`

- 校验：setBrowserConnection 只接受主窗；连接对象字段/协议白名单（rust-browser-control.cjs:9-27）；sessionPath 必须 native:<id>；tabId/url 长度与字符校验；外链只 http(s)；远程 webview 不得获得任何 custom 命令
- 平台差异：mac WKWebView 子视图/分区；win WebView2；linux WebKitGTK；Tauri 多 webview/child view 需 unstable feature（R09-T05 风险）
- 证据计划：A09/A10：无 Electron 进程下用户接管/tab 操作 + 恶意 HTML 无命令权限（R09-T05 场景）；本清单只登记 API→命令映射
- 风险：high——超出 T03 基础能力，属 T05；Tauri 嵌 BrowserView 等价物（多 webview/child view）与 rust-browser-request 代理是本阶段最大技术不确定性之一

### `host:screenshot-render`

- 校验：payload 主题白名单（SCREENSHOT_THEMES 6 项）；saveDir 只允许用户目录子目录（resolveWorkspaceOutputDir）；分段索引校验；返回 {success,filePath,dir} 或 {success:false,error}；HTML 内容不可注入任意脚本（模板由 Rust/宿主生成）
- 平台差异：mac WKWebView 截图方案不同；win WebView2 CapturePreview；linux WebKitGTK 可能缺失→需阻塞记录
- 证据计划：真实生成 6 主题截图+文件存在/尺寸断言；未知主题/越界 saveDir 负向；需 T04 主导
- 风险：high——Tauri/Wry 无官方页截图 API，需要 wry 平台钩子或改用服务端渲染；本清单标注待 T04 spike，不臆造实现

### `host:settings-broadcast`

- 校验：event type 白名单（theme-changed/keybindings-changed/network-proxy-changed/keep-awake-changed/quick-chat-shortcut-changed/locale-changed/experiment-changed/quick-chat-preferences-changed 等从源码提取）；payload 结构校验；不接受任意 event 名/任意窗口广播；发送者身份校验；缺失窗口跳过不阻断
- 平台差异：无平台差异；side effect 依赖平台能力（托盘/keep-awake/代理）
- 证据计划：双窗 wdio：设置窗改主题→主窗/browser-viewer 收到并重绘；改 keep-awake→系统断言；关闭设置窗后主窗仍收到；未知 event 负向
- 风险：high——侧效应（主题/代理/防休眠/快捷键/托盘 i18n）是现役真实行为，Tauri 若只转发事件不执行 side effect，功能表面「通过」但系统状态不生效（A05 失败）

### `host:server-connection-info`

- 校验：只返回当前 owned 连接；token 不得出现在 URL/query/localStorage（Web 形态例外需标注）；结构 {port,token,serverNodeKind,serverNodeTransport,host} 校验；重启事件只发给活跃窗口
- 平台差异：无平台差异；远端模式 host 字段语义（R08-T03）需保留
- 证据计划：wdio 读取连接→真实 WS/HTTP 握手成功；重启 server 后事件到达且新端口生效；token 不出现在 network URL 断言
- 风险：high——token 交付面是 A06/A10 的核心；Tauri 必须避免 core/http 插件泛化与 URL 注入；T02 尚未定 supervisor 接口

### `host:update-auto`

- 校验：channel ∈ {stable,beta} 枚举；更新源固定（不允许 renderer 传 URL）；安装前校验签名/磁盘空间；安装失败回执；更新中禁止重启重入
- 平台差异：mac .app zip/自替换（BLOCKED-SIGNING-IDENTITY 影响真实安装）；win NSIS；linux AppImage
- 证据计划：R01 spike updater loopback 夹具：发现版本/下载/验签/篡改拒绝；本阶段不执行真实安装（需 R10 授权）；平台阻塞登记
- 风险：high——electron-updater→tauri-plugin-updater 的渠道/自替换语义差异大（mac self-install、Windows）；本阶段只做能力映射，不声称更新链可用

### `host:update-train-ota`

- 校验：manifest 验签/契约版本闸门（preload/protocol）；apply 前优雅停 server→激活→重启；进度只发发起窗口；一次性 fallback notice 状态仅内存；不静默下载
- 平台差异：mac/win/linux 产物槽位与指针路径不同；崩溃回退计数按平台启动标记
- 证据计划：R01 已有 updater 夹具模式；本阶段仅命令映射；真实 OTA 应用需 T02/T08+R10 证据
- 风险：high——现役 OTA 与 electron 主进程/服务 supervisor 深度耦合；Tauri 侧须整体迁移，不能只留按钮

### `security:capability-sets`

- 校验：A11：对每个窗口展开「命中 capability 并集」并与白名单逐项比较，越权集合必须为空；负向：untrusted/remote 页面调用全部命令 100% 拒绝（R01 spike 11 拒模式）；多 capability 叠加场景显式构造
- 平台差异：无平台差异；Windows 上 ACL 与 asset protocol origin 需额外验证
- 证据计划：配置展开脚本（解析 gen/schemas/capabilities.json + 期望表 diff）+ 运行时负向 invoke 矩阵（wdio 按窗口逐一调用）；A11 证据
- 风险：high——R09 命令面约 91 条（R06 26 条），按窗口最小权限集合易出错；必须机器化展开比对而非人工审阅

### `security:csp`

- 校验：A10/A11：恶意 SVG/HTML 预览含脚本与 IPC 尝试不能获得 token/命令；CSP 不允许 unsafe-eval/远程脚本；connect-src 精确到 127.0.0.1 端口；frame-src 只 self/blob；object-src none；导航拦截策略在 Rust on_navigation 实现
- 平台差异：win 使用 http://ipc.localhost/asset.localhost（CSP 需包含）；linux/mac 使用 tauri/asset scheme；WebView2 vs WKWebView 协议差异
- 证据计划：构建期 CSP 解析断言 + 运行时注入测试页（远程脚本/内联 eval/外链 frame）全部被阻止；与 R06 的 devCsp 分离；A10 证据
- 风险：high——现役主窗/设置窗无 CSP（Electron 安全债），Tauri 收紧后可能与现有内联脚本/字体/主题资源冲突，需要实测清单

### `security:command-acl`

- 校验：每注册命令必须同时在 build.rs AppManifest、invoke_handler、permission toml、目标 capability 中出现，四者缺一即失败（自动审计脚本）；未授权窗口调用必须被 ACL 拒绝（页面自报 + WebDriver 驱动端互证，R01 spike 模式）
- 平台差异：无平台差异
- 证据计划：脚本比对 invoke_handler/AppManifest/permissions/capability 四份集合（零差集）；负向 invoke 矩阵；A11/A12 证据
- 风险：high——91 条命令的人工维护易漏；需要 R09 生成/校验脚本（含空集合失败）

### `security:release-no-test-control`

- 校验：A12：release 包扫描监听端口（WebDriver 端口不可达）+ 二进制 strings 无 wdio/webdriver + debug 命令 not found；dev/test 构建仍可用（正负双产物，R01 spike A12 模式）
- 平台差异：无平台差异；Windows 端口监听检查需 netsh；Linux ss
- 证据计划：双产物探针（test 构建通过/release 拒绝）+ 二进制 strings + 端口扫描；A12 证据
- 风险：high——R00 46 叶要求保留 debug onboarding 功能（T07 可能验收），T06 要求生产无测试控制面；需裁决「功能保留但 release 不可达」并落到 capability 分组

### `security:remote-content-isolation`

- 校验：A10：恶意 SVG/HTML 无 token/命令/asset 越界；window.open 拒绝；导航只 http(s) 且外链走系统浏览器；卡片 iframe 导航阻断等价物（现役 flag 消费者 InteractiveCard.tsx:56）
- 平台差异：mac WKWebView 隔离；win WebView2 隔离；linux WebKitGTK 隔离；远程 URL origin 差异
- 证据计划：恶意内容测试页（脚本/IPC/外链/token 尝试）在预览区与远程 webview 全部拒绝；A10 证据
- 风险：high——现役卡片导航 guard 在 Tauri 无 will-frame-navigate 等价（见 host:card-frame-navigation-guard），预览隔离策略需重新设计

### `security:no-arbitrary-shell`

- 校验：A06：畸形路径/URL/命令样式参数（'$(...)'、'\0'、相对路径、file://、javascript:）全部按类型拒绝，不产生 shell 执行；服务端/浏览器/命令注入负向
- 平台差异：win powershell 依赖必须移除（现役代码路径）；mac/linux unzip 依赖同样移除
- 证据计划：IPC 负向测试矩阵（R09-T03/A06）：逐命令畸形参数注入 + 进程探针确认无子进程/无命令执行；R06 edit_allowlist_refuses_system_commands 模式
- 风险：high——现役技能解压外调 unzip/powershell 是真实命令面；T06 要求收口，T03 的 skill-viewer 命令必须换实现

### `host:card-frame-navigation-guard`

- 校验：若未来实现：卡片 iframe 只能加载 /lingxi/v1/cards/*/frame/<sha> 或 /api/cards/*，导航到其它地址必须阻断；A10 需覆盖卡片内导航逃逸
- 平台差异：Wry 无帧级导航回调（需平台自定义或隔离 iframe 方案），三平台一致
- 证据计划：恶意卡片尝试顶层导航到外部 URL 必须失败；当前实现为「仅同源」降级，需在 T07/UI 对照中登记差异
- 风险：high——现役真实能力无法直接迁移；若强行置 true 会造成卡片安全边界虚假通过，必须如实降级并记录（阶段门禁）

### `host:speech-permission`

- 校验：status 只读不弹窗；request 仅用户手势；返回 SPEECh_PERMISSION_STATES 六态；开发壳不请求（防 SIGABRT）；并发合并单飞；fail-closed bridge_unavailable
- 平台差异：mac SFSpeechRecognizer TCC；win/linux 无对应（unsupported）；Info.plist NSSpeechRecognitionUsageDescription 需打包配置（BLOCKED-SIGNING-IDENTITY 影响真实授权）
- 证据计划：T04 主导：status 只读查询（tcc_probe 模式）、拒绝态、真实授权后正链（需人工）；A07/A08 证据；本清单只做 API→命令映射
- 风险：high——T04 范围且受 BLOCKED-SYSTEM-PERMISSIONS；Tauri 无官方插件，需宿主内 Swift/ObjC 桥

A05/A06/A11/A12 总体证据策略：

- **A05 用户动作真实生效**：托盘（系统级双击/右键菜单 + 录屏）、全局快捷键（合成 CGEvent 触发 + handler 日志，R01 spike `post_shortcut.swift` 模式）、通知（系统通知中心/截图 + 点击聚焦）、拖拽（Finder/文件管理器真实拖放 + 文件系统探针）、文件选择（真实原生面板 + `CGWindowList` owner 观察 + 取消路径，R06 `tests/desktop_host.rs`/`directory_picker_host.rs` 模式）。证据落 `artifacts/rust-tauri/R09/`；mac 以外平台按 BLOCKED_PLATFORM 记录。
- **A06 不可信参数**：对文件/URL/外链/命令样式参数做负向 IPC 测试（相对路径、`\0`、`javascript:`、`file://`、`$(...)`、超长、跨窗口 id），断言按类型/scope 拒绝且无子进程/无 shell 执行；R06 `edit_allowlist_refuses_system_commands` 模式。
- **A11 capability 叠加**：解析 `gen/schemas/capabilities.json` 展开每个窗口的权限并集，与白名单精确比较；再用 test 构建逐窗口 invoke 全部命令做负向矩阵。
- **A12 生产无测试控制面**：test/release 双产物探针（端口不可达 + 二进制 strings 无 wdio/webdriver + debug 命令不存在），复用 R01 spike 的 A12 双产物模式。

## 12. 未读 / 不确定边界（如实声明）

- 未逐行通读全部 7003 行 `desktop/main.cjs`；以 handler 注册点 + 关键实现函数 + 调用链为准。未覆盖段落包括 GPU/启动诊断、artifact boot/OTA 内部实现、server supervisor 细节（属 R08/T02/T08 范围）。
- 未逐行审计 `desktop/auto-updater.cjs`（851 行），仅提取 IPC/状态/安装入口；更新链最终验收归 R10。
- 未验证任何 Tauri 插件在 tauri 2.12.x 上的实际可用性；R01 spike 版本为 2.11.6，R06 为 2.12.1，插件版本需重锁（OQ-02）。
- 未执行任何系统动作/E2E/录屏；所有 evidencePlan 是计划，不是已取得证据。
- 未调查 R06 `resource_subscribe/resource_events` 是否被 R09 主界面复用（当前 Viewer 走 renderer 直连服务端 ResourceIO；T02/T05 需裁决）。
- `docs/rust-tauri/R00/ENTRYPOINTS.json` 的 main.cjs 锚点行号为历史基线（如 createMainWindow 2888 vs 当前 3129），本清单不据此断言现状；R00 已密封不回写（OQ-12）。
- Windows/Linux/macOS x64 无真机：平台相关行为只做源码级差异标注，不做通过判断（BLOCKED-PLATFORM-*）。
- 未读 R02–R08 全部实现与证据；T03 依赖的 T02 supervisor/DesktopHost-Tauri 尚未实现（R09 START_RECEIPT §7：「完整 React DesktopHost-Tauri 尚不存在」）。

## 13. 校验方式（本清单自身）

- JSON 结构校验：顶层 6 键；50 条能力字段集合逐一一致；110 个 PlatformApi 成员恰好映射一次（另 3 个 preload-only）；46 叶全部有映射或非宿主理由；91 条命令全部在能力目标中被引用；JSON 由 Python `json.load` 回读通过。
- 只读校验：本清点作者只写了 `docs/rust-tauri/R09/` 下这两个文件，未触碰生产代码/`package.json`/锁文件/阶段图/capabilities。生成期间工作区同时存在并行 Team 成员的改动（`package.json`/`package-lock.json`、`rust/crates/xtask/*`、`artifacts/rust-tauri/R09/`、R09 目录下其它文档），这些非本清点产生，不能用作「工作区干净」的证明；本清单所有结论只依赖 HEAD 提交内的源码。
- 本 MD 由同一份 JSON 生成，避免两文件数字漂移；引用行号以本文件生成时的 HEAD 为准。
