# R08 UI 5 叶逐断言客户端证据矩阵（closeout-proofs-02/ui-pages）

- 生产者：`scripts/rust-tauri/r08_ui_pages.mjs --out artifacts/rust-tauri/R08/closeout-proofs-02/ui-pages`
- 产物：`ui-cases.json`（schema `lingxi.leaf-case-results.v1`，逐 case）、`result.json`（逐 leaf/case/证据回执）、
  `output.log`（verbose vitest + live 套件原始日志）、`raw/`（vitest JSON 报告原件）、`live/`（两条真 Electron live 套件证据）
- 环境：Node v24.16.0；Rust 服务二进制 `rust/target/debug/lingxi-service`；`ELECTRON_RUN_AS_NODE` 在 live 子进程内被清除
  （否则 Electron 以纯 Node 运行拿不到 `app`）。
- 最终结果：**87 cases / 87 ok / exit 0**；vitest 三组 exit 0（新 UI 43 例 + 既有 10 文件 + composer 精确选取 2 例），
  live 两套件 exit 0 且 status=PASS（quickchat 9 checks / session-map 19 checks）。
- 独立复核补洞轮（+8 case）：MCP a3 混合批量回执、MCP a5 权限保存失败、安全 a1/a2/a3 共用法守卫、
  安全 a3 代理通知失败、chat a1 WS 错误页面呈现、chat a2 恢复流状态与控件一致（详见各表与 §未覆盖清单）。
- `live/` 只保留 check 回执、日志、截图与供应商请求样本；Electron profile、Vite 缓存、服务 DB 等运行目录已清理
  （生产者重跑会重建），check 回执不受影响。
- case 命名（0-based）：`ui::chat.a0..a5` / `ui::map.a0..a6` / `ui::mcp-settings.a0..a6` /
  `ui::security.a0..a3` / `ui::quick-chat.a0..a1`。每个断言 ≥1 个专属 case，叶内不复用；一条断言可挂多个 case。

## 证据类型

| 类型 | 含义 | 说明 |
|---|---|---|
| **A 组件级真实投影** | jsdom + @testing-library/react 渲染生产组件/store/actions | 只 mock 网络传输（新测试 mock 全局 `fetch` 或 `lingxiFetch` 传输函数）；断言页面实际投影、请求 URL/body、store 副作用、空态与错误态；不做快照。少数 jsdom 不可用的编辑器/按钮叶子替身仅限 UI 引擎（TipTap 等）且与既有测试同一处置。 |
| **B 真 Electron live** | 真 Vite + 真 Electron + 真 `lingxi-service` 子进程 + 真实模型供应商 HTTP 替身 | 由仓库既有 live 套件生产者 `tests/rust-native/quickchat-live.py`、`tests/rust-native/session-map-live.py` 驱动（R06 合成宿主），check 列表写 `live/*/result.json`、`live/session-map/map-ui-result.json`。 |
| **C 既有专属单测** | 仓库既有产品测试（未修改） | 语义与断言条款直接对应才绑定；均为 mock 传输层的真实组件/协议测试。 |

## 断言 → case → 测试/check 矩阵

### 1) chat 页叶 `R00-T02-LA-81B217A7E581`（F-D01-UI-UI-PAGE-CHAT，6 条断言，25 case）

| 断言 | case | 类型 | 测试名 / check | 覆盖三分句 |
|---|---|---|---|---|
| a0 打开聊天页显示欢迎页/消息流与输入框 | `ui::chat.a0.welcome-input-exclusive-projection` | A | `r08-ui-chat-page.test.tsx`「无会话显示欢迎页与输入框；选中当前会话后欢迎页隐藏、消息区与输入区并存」 | 成功（会话投影与欢迎态互斥）+ 空态（无会话显示欢迎页） |
| | `ui::chat.a0.mobile-disconnected-no-thinking-get` | A | 同上「mobile 或服务未连接时不请求该会话思考档位 GET」 | 空态（mobile/未连接不发思考档位 GET） |
| | `ui::chat.a0.regional-failure-boundary` | A | 同上「区域渲染失败由 RegionalErrorBoundary 捕获并显示重试，不冒充消息成功」 | 错误（区域失败显式提示，不冒充成功） |
| a1 输入提交普通 prompt 或 slash | `ui::chat.a1.optimistic-user-message-client-id` | C | `InputArea.paste-and-slash.test.tsx`「adds an optimistic user message with a clientMessageId when sending a prompt」 | 成功（prompt 进入会话） |
| | `ui::chat.a1.slash-sent-via-ws` | C | `slash-commands-ws.test.ts`「carries the agent that owns this input box so the server never has to guess」 | 成功（slash 单独发送、带自身身份） |
| | `ui::chat.a1.slash-result-inline-notice` | C | `ws-message-handler.test.ts`「shows focused slash_result through the inline notice bridge」 | 成功（slash_result 被处理并呈现） |
| | `ui::chat.a1.blank-input-blocked` | C | `composer-send.transaction.test.ts`「P07：全空白正文（无其他内容维度）不发，且不做破坏性清空」 | 空态（空文本被输入校验阻止） |
| | `ui::chat.a1.ws-unavailable-explicit-result` | C | `composer-send.transaction.test.ts`「S04：ws 不存在 / CONNECTING / CLOSING / CLOSED，明确结果、不清输入、不发消息」 | 错误（WS 不可用显式结果，不冒充已发） |
| | `ui::chat.a1.ws-error-shown-in-page` | A | `r08-ui-chat-page.test.tsx`「WS 拒绝/断线事件在页面内显式呈现错误，不冒充消息成功」（真实 `handleServerMessage` → store inline 错误 → 页面错误条） | 错误（断线/拒绝在页面内显式呈现） |
| a2 中止、插话、运行中输入入队及恢复流 | `ui::chat.a2.streaming-send-queues-then-interjects` | C | `InputArea.media-send.test.tsx`「interjects streaming attachment sends with the same message envelope as prompt sends」 | 成功（运行中发送入队；「立即插入」派发 interject，非 steer） |
| | `ui::chat.a2.stop-binds-current-stream` | C | 同上「binds Stop to the focused session current stream identity (#2078)」 | 成功（中止绑定当前会话当前 stream） |
| | `ui::chat.a2.idle-vs-running-control` | A | `r08-ui-chat-page.test.tsx`「运行信息控件随真实运行态切换：无运行任务只有发送，会话在跑显示中止」 | 空态（无运行任务时中止控件不可用） |
| | `ui::chat.a2.resume-keeps-streaming-state` | C | `stream-resume.test.ts`「keeps the session marked streaming when resume replay is empty but runtime says it is still running」 | 恢复流（resume 空回放仍保持运行态） |
| | `ui::chat.a2.resume-status-drives-control` | A | `r08-ui-chat-page.test.tsx`「恢复流的运行状态经真实 ws 分发后与输入区控件态一致」 | 恢复流（status true/false 经真实 ws 分发 → 控件 send/stop 一致） |
| a3 主聊天页侧边对话、附件/预览、运行信息胶囊 | `ui::chat.a3.side-chat-transcript-composer` | C | `SideChatPanel.test.tsx`「renders the side session transcript and its own composer」 | 成功（侧边对话独立 transcript/composer；QuickChatApp 独立窗口未混入） |
| | `ui::chat.a3.side-chat-closed-empty` | C | `SideChatPanel.test.tsx`「renders nothing while the panel is closed」 | 空态（关闭时不渲染） |
| | `ui::chat.a3.attachments-preview` | C | `AttachedFilesBar.media.test.tsx`「renders image attachments with rounded thumbnail previews」 | 附件/预览投影 |
| | `ui::chat.a3.runtime-capsule-pill` | C | `RuntimeInfoCapsule.test.tsx`「renders collapsed as a pill outside the document flow of the transcript」 | 运行信息胶囊投影 |
| a4 主输入工具栏选择聊天模型 | `ui::chat.a4.existing-session-model-switch` | C | `ModelSelector.test.tsx`「显式 sessionPath 时按该会话切换，并把结果回调给调用方」 | 成功（已有消息 POST `/api/models/switch` 并更新该会话模型） |
| | `ui::chat.a4.new-session-default-model-set` | C | `ModelSelector.test.tsx`「applies the selected model thinking default while preparing a new session」 | 成功（待建会话 POST `/api/models/set` 并同步草稿思考档位） |
| | `ui::chat.a4.same-model-zero-request` | A | `r08-ui-chat-page.test.tsx`「已建会话选择相同且可用模型直接返回、不发 switch 请求；无可选模型不产生有效切换」 | 空态（同模型零请求；无可选模型无有效切换） |
| | `ui::chat.a4.switch-failure-explicit-toast` | C | `ModelSelector.test.tsx`「maps a server streaming-switch rejection to the same explicit warning」 | 错误（切换失败显式提示） |
| a5 主输入工具栏选择思考档位 | `ui::chat.a5.thinking-session-save` | C | `thinking-level-button.test.tsx`「saves thinking changes to the current session when a session is active」 | 成功（已有会话带 sessionPath 提交） |
| | `ui::chat.a5.thinking-pending-default-only` | C | 同上「saves pending new-session thinking changes as the model default draft」 | 成功/空态（待建会话不带 sessionPath，只改默认档位） |
| | `ui::chat.a5.thinking-save-failure-console-only` | A | `r08-ui-chat-page.test.tsx`「未建立会话时只改默认档位（请求不带 sessionPath）；保存失败仅 console.error，不宣称已持久化」 | 错误（失败仅 console.error，不触发 onChange 宣称成功） |

### 2) map 页叶 `R00-T02-LA-8E2D44B48905`（F-D02-UI-UI-PAGE-MAP，7 条断言，19 case，全部 B 类）

live 套件：`tests/rust-native/session-map-live.py`（`R06_SESSION_UI=1`，真 Electron 生产地图页 + 真 lingxi-service；
`map-ui-result.json` status=PASS，19 checks）。

| 断言 | case | live check |
|---|---|---|
| a0 打开地图自动读布局和会话 turns | `ui::map.a0.loads-real-layout-and-turn-cards` | `production-map-loads-real-layout-and-turn-cards` |
| | `ui::map.a0.other-service-empty-map-zero-mutation` | `actual-other-service-empty-map-clears-old-owner-with-zero-put` |
| | `ui::map.a0.read-failure-explicit-warning` | `actual-read-failure-explicit-warning-without-success-or-disk-rewrite` |
| a1 工作区选择、卡片点击切会话 | `ui::map.a1.workspace-selector-no-cwd-zero-files-read` | `workspace-selector-real-focus-with-no-cwd-zero-files-read` |
| | `ui::map.a1.card-selects-real-session-same-session-zero-fetch` | `card-selects-real-session-files-models-and-same-session-zero-fetch` |
| | `ui::map.a1.same-revision-cache-zero-history-changed-revision-refresh` | `same-revision-cache-zero-history-and-changed-revision-real-history-refresh` |
| | `ui::map.a1.archive-refusal-retains-session-selected-card` | `archive-refusal-retains-current-session-but-selected-card-already-changes` |
| a2 缩放、定位、折叠和拖动卡片 | `ui::map.a2.zoom-locate-only-change-real-camera` | `zoom-locate-only-change-real-camera`（相机控制纯本地，零 PUT） |
| | `ui::map.a2.invalid-coordinates-zero-put` | `invalid-local-coordinates-send-zero-put` |
| a3 拖动卡片合并位置 | `ui::map.a3.pointer-drag-persists-card-position` | `real-pointer-drag-persists-card-position` |
| | `ui::map.a3.drag-write-failure-keeps-local-disk-unchanged` | `real-write-failure-keeps-local-drag-but-disk-unchanged` |
| a4 折叠/展开卡片替换折叠列表 | `ui::map.a4.collapse-reload-and-external-reveal-replaces-list` | `collapse-reload-and-external-current-reveal-replace-list` |
| | `ui::map.a4.collapse-write-failure-keeps-local-old-disk` | `real-collapse-write-failure-keeps-local-collapse-and-old-disk-until-reload` |
| a5 重置地图替换全部位置 | `ui::map.a5.reset-replaces-all-positions-on-disk` | `reset-replaces-all-positions-on-disk` |
| | `ui::map.a5.reset-write-failure-keeps-local-old-disk` | `real-reset-write-failure-keeps-local-reset-but-old-disk` |
| a6 选区追问或从卡片打开继续/分支草稿并发送 | `ui::map.a6.empty-whitespace-zero-send-continue-no-fork` | `empty-whitespace-zero-send-and-continue-real-prompt-without-fork-or-switch` |
| | `ui::map.a6.fork-failure-missing-anchor-preserves-draft-then-branch` | `real-fork-IO-error-and-missing-anchor-preserve-draft-then-actual-branch-prompt-focus` |
| | `ui::map.a6.running-busy-queues-input-zero-extra-prompt` | `actual-running-provider-busy-queues-input-with-zero-extra-prompt` |
| | `ui::map.a6.paused-unknown-delivery-preserves-draft` | `real-paused-service-unknown-delivery-preserves-draft-and-zero-duplicate-submit` |

备注：map live 套件同时逐项断言了「读取失败 console warn + 旧地图残留可复查」「PUT 失败仅日志/未回滚本地布局」
「拖动合并持久化」「重置后磁盘归零」等原断言中括号说明的现役实际行为。

### 3) MCP 设置页叶 `R00-T02-LA-B4BB2438E855`（F-D03-UI-UI-SETTINGS-MCP，7 条断言，21 case）

新测试文件（A 类，网络仅 mock fetch，回执形状对齐 lingxi-service）：`r08-ui-settings-mcp.test.tsx`、
`r08-ui-settings-container.test.tsx`。

| 断言 | case | 类型 | 测试名 / 文件 |
|---|---|---|---|
| a0 打开设置容器自动初始化 | `ui::mcp-settings.a0.container-cold-start-mcp-page` | A | `r08-ui-settings-container.test.tsx`「MCP 设置页冷启动：显示 loading mask，初始化完成后移除并真实渲染 MCP 页」 |
| | `ui::mcp-settings.a0.cached-old-data-background-refresh` | A | 同上「已有缓存直接显示旧数据并在后台刷新，不阻塞 ready、不显示全屏 mask」 |
| | `ui::mcp-settings.a0.provider-summary-nonblocking` | A | 同上「供应商摘要失败不阻塞 ready，也不冒充空列表成功」 |
| a1 自动读取连接器状态与工具权限 | `ui::mcp-settings.a1.connector-list-status-tools` | A | `r08-ui-settings-mcp.test.tsx`「读取真实连接器列表并逐条显示状态、工具数与助手权限；无连接器时显示添加入口」 |
| | `ui::mcp-settings.a1.read-failure-keeps-old-state` | A | 同上「连接器读取失败显示显式错误并保留旧状态，不把失败冒充成空列表」 |
| a2 全局启停与延迟工具加载 | `ui::mcp-settings.a2.global-toggle-save-reread` | A | 「总开关保存后按助手重读状态并显示真实成功提示」 |
| | `ui::mcp-settings.a2.global-toggle-failure-restores` | A | 「总开关保存失败恢复旧配置并显示错误，不写成功状态」 |
| | `ui::mcp-settings.a2.defer-threshold-save` | A | 「延迟加载开关与阈值保存到全局设置，并在保存后重读状态」 |
| | `ui::mcp-settings.a2.no-agent-global-state-only` | A | 「无 agentId 时读取全局状态（不带 agentId 查询），不伪造助手权限」 |
| a3 添加连接器、批量导入 | `ui::mcp-settings.a3.add-connector-with-start-intent` | A | 「添加连接器提交真实动作与启动意图，成功后面板出现新连接器」 |
| | `ui::mcp-settings.a3.bulk-import-per-row-partial` | A | 「空表单不可提交；批量导入逐项反馈部分成功，不冒充全成功」 |
| | `ui::mcp-settings.a3.bulk-partial-http200-mixed` | A | 「批量导入 HTTP 200 混合回执逐行呈现成功/runtimeApplied=false 事实，不弹全成功 toast」 |
| a4 编辑/删除连接器与启动/停止/刷新工具 | `ui::mcp-settings.a4.remove-with-confirm` | A | 「删除需确认，确认后提交真实移除动作；失败时保留连接器并显示错误」 |
| | `ui::mcp-settings.a4.start-stop-refresh-actions-keep-credentials` | A | 「启动、停止与刷新工具提交真实动作；停用不清凭据，成功后重读连接状态」（三条动作均断言 action/payload） |
| a5 按助手启停连接器、设置工具权限/置顶 | `ui::mcp-settings.a5.agent-connector-tools-pinned` | A | 「按助手启停连接器并保存工具权限与置顶，动作携带当前助手身份」（destructive 工具放行控件禁用） |
| | `ui::mcp-settings.a5.no-agent-refuses-detail` | A | 「无工具显示空目录；无助手时不伪造授权，打开详情被明确拒绝」 |
| | `ui::mcp-settings.a5.policy-save-failure-no-fake-grant` | A | 「工具权限与置顶保存失败显示错误且不伪装授权已更新」（失败提示 + 重读后开关仍未授权） |
| a6 OAuth 登录、取消、退出 | `ui::mcp-settings.a6.oauth-login-open-external-poll-refresh` | A | 「登录打开外链并轮询到 done 后刷新连接器并显示保存成功」 |
| | `ui::mcp-settings.a6.oauth-cancel-post-only` | A | 「取消只提交取消动作并关闭等待卡，不因取消本身重读状态」 |
| | `ui::mcp-settings.a6.oauth-logout-reread` | A | 「退出登录走统一动作并经真实状态刷新显示成功；启动失败不打开外链」 |
| | `ui::mcp-settings.a6.oauth-start-failure-no-external` | A | 「登录启动失败显示错误且不打开外链、不进入轮询」 |

### 4) 安全设置页叶 `R00-T02-LA-DA5AD5C40098`（F-D04-UI-UI-SETTINGS-SECURITY，4 条断言，14 case）

新测试文件（A 类）：`r08-ui-settings-security.test.tsx`、`r08-ui-settings-container.test.tsx`。

| 断言 | case | 类型 | 测试名 / 文件 |
|---|---|---|---|
| a0 打开设置容器自动初始化 | `ui::security.a0.container-cold-start-security-page` | A | 容器测试「安全设置页冷启动：初始化完成后移除 mask 并渲染沙箱/备份/代理配置」 |
| | `ui::security.a0.init-error-ready-with-log` | A | 「初始化异常记录日志后仍 ready，并保留可操作的恢复入口（不装成成功空数据）」 |
| | `ui::security.a0.fifteen-second-timeout-ready` | A | 「15 秒无响应强制 ready 并移除 loading mask，杜绝无限白屏」 |
| a1 沙箱及沙箱网络开关 | `ui::security.a1.sandbox-switches-save-and-reread` | A | `r08-ui-settings-security.test.tsx`「两个沙箱开关经每助手配置接口写入并重读当前助手合成配置视图」 |
| | `ui::security.a1.uninitialized-loading-not-off` | A | 「状态未初始化时开关为 loading/禁用，不把未加载当关闭，也不发保存请求」 |
| | `ui::security.a1.save-failure-restores-and-snapshot-failure` | A | 「保存失败恢复开关并提示错误；PUT 已成功但快照读取失败时如实报保存失败」 |
| | `ui::security.a1.no-connection-zero-network-request` | A | `r08-ui-settings-guards.test.ts`「无可用连接（resourceKey 无法定位真实服务）时 loadSettingsConfig 不发出网络请求」 |
| a2 检查点保留策略、展开备份列表 | `ui::security.a2.retention-and-backup-list` | A | 「保留天数写入每助手配置并展开读取真实检查点列表」 |
| | `ui::security.a2.empty-and-read-failure-explicit` | A | 「无检查点显示空列表；读取失败显示显式错误并保留重试，不冒充空列表」 |
| | `ui::security.a2.no-agent-load-settings-zero-request` | A | `r08-ui-settings-guards.test.ts`「无 agentId 时 loadSettingsConfig 不发任何请求，并显式报 No settings agent selected」 |
| a3 网络代理模式及 HTTP/HTTPS/WS/WSS/noProxy 字段保存 | `ui::security.a3.manual-proxy-save-and-notify` | A | 「手工代理保存 HTTP/HTTPS/WS/WSS/noProxy 并广播 network-proxy-changed、重读配置」 |
| | `ui::security.a3.system-mode-and-save-failure-keeps-draft` | A | 「系统代理模式不渲染手工字段（保存仅模式）；保存失败提示并保留草稿，不广播」 |
| | `ui::security.a3.experience-disabled-zero-request` | A | `r08-ui-settings-guards.test.ts`「经验未启用时不 GET experience；显式启用后才发起 experience 读取」 |
| | `ui::security.a3.notify-failure-no-success` | A | `r08-ui-settings-security.test.tsx`「network-proxy-changed 通知抛错时仅记录未处理异常，不弹保存成功、不完成后续重读」 |

### 5) 快捷窗口叶 `R00-T02-LA-A2C385A33493`（F-D01-DESKTOP_BEHAVIOR-QUICK-CHAT-WINDOW，2 条断言，8 case，全部 B 类）

live 套件：`tests/rust-native/quickchat-live.py`（真 Electron 双窗口 + 真 lingxi-service + 真实模型供应商 HTTP 替身；
`result.json` status=PASS，9 checks，真实 `modelCalls` 账本核验）。

| 断言 | case | live check |
|---|---|---|
| a0 悬浮窗口可见/尺寸改变；选择会话聚焦主窗并打开该会话 | `ui::quick-chat.a0.visible-compact-bounds` | `actual-production-show-visible-and-compact-bounds`（真实生产 show，宽 480 compact bounds） |
| | `ui::quick-chat.a0.resize-increases-window` | `actual-production-resize-increases-window-and-resizable` |
| | `ui::quick-chat.a0.switch-acks-focus-main-then-hide` | `actual-owned-switch-ACK-focus-main-and-only-then-hide`（主窗聚焦且已切到真实会话后才隐藏快捷窗） |
| | `ui::quick-chat.a0.same-path-requires-real-service-focus` | `same-path-still-requires-real-service-focus`（同路径也须真服务焦点事件，界面捷径不算存在性证明） |
| a1 不存在会话或窗口销毁时不应伪称切换成功 | `ui::quick-chat.a1.missing-session-refuses-keeps-floating` | `actual-missing-session-refuses-and-keeps-floating-window` |
| | `ui::quick-chat.a1.destroyed-main-explicit-failure` | `actual-destroyed-main-window-explicit-failure-keeps-floating`（`QUICK_CHAT_MAIN_UNAVAILABLE`） |
| | `ui::quick-chat.a1.changed-owner-refuses-without-hide` | `changed-main-connection-owner-refuses-without-hide` |
| | `ui::quick-chat.a1.late-ack-cannot-hide-after-timeout` | `late-actual-renderer-ACK-cannot-hide-after-timeout` |

## 与既有测试的关系 / 冲突

- **未修改任何既有测试文件**；`McpTab.test.tsx`、`SecurityTab.test.tsx`、`SettingsContent.test.tsx`、
  `R07McpRustAPI.test.ts`、`r07-settings-entry-real-service.test.tsx` 等已盘点：前者 mock 业务模块
  （mcp-api/helpers），不满足「只 mock 网络」口径，故不作为本叶 case，仅作行为参考；新测试改用真实 store/actions +
  fetch 级 mock 并从同一 Rust 回执形状出发。
- `composer-send.transaction.test.ts` 在当前工作树有 **1 条与本证据无关的既有红**
  （「Rust wire 只有握手时拒绝旧聊天帧且不清空输入」，属并行批次的改动面）；生产者只以 `-t 'P07：全空白正文|S04：ws 不存在'`
  精确选取本叶需要的两条并如实记录（该文件其余用例被过滤未运行，不冒充整文件绿）。
- `desktop/src/react/__tests__/chat-live/`（真 ChatPage live 套件）仓库内**没有 runner**，其 `R06_CHAT_*`
  合成宿主未提供，本轮**未运行**；chat 页叶改用组件级真实投影 + 既有专属单测，属如实降级而非成功。
  地图/快捷窗口两条 live 套件有 runner 且本轮真实 PASS（见 `live/`）。

## 未覆盖 / 差异清单（如实，补洞轮后）

1. **MCP a1 错误分句描述与现役实现不同**：原断言写「连接器或权限读取失败仅 console.error，保留旧状态，无专门错误态或 toast」；
   现役实现（R07 起）会渲染 `role="alert"` 错误条 + 重试（更显式）。测试断言**实际行为**（console.error + alert + 保留旧连接器 +
   不弹 toast），矩阵按现役行为记录，不把旧描述当验收。
2. **MCP a6 其余错误分句**：启动失败已测；轮询 error/超时、取消请求失败按各自 catch 显示错误未单独测；「Web/mobile 用
   `window.open`」未测（组件在模块加载时捕获宿主 api，jsdom 套件只测桌面 `platform.openExternal` IPC 路径）——属当前生产
   代码在 jsdom 不可构造的路径，保留未覆盖。
3. **安全 a2/a3 共用分句**：「各项 GET 失败应与前序保存结果分开判断」「PUT 已成功但快照 GET 失败时 helper 仍报保存失败」
   已在 a1 的 save-failure/snapshot-failure 用例覆盖；a2/a3 未各自重复构造（同一 helper 路径，不重复造 case）。
4. **安全 a2 错误分句**：原断言写「检查点列表 GET 失败直接设为空数组且无警告；页面可能呈假空态」；现役实现（R07 起）
   显示显式错误 + 重试。测试断言实际行为（alert + 保留重试、不显示假空态），与断言旧描述不一致处按现役实现记录。
5. **chat a1 错误分句**：WS 断线/拒绝现由页面内输入区上方错误条显式呈现（真实 `handleServerMessage` 驱动，已补测）；
   「消息区」若指独立的消息列表组件，现役实现并未在该处重复呈现同一错误，测试按现役承载面断言。
6. **chat a3 错误分句**：「加载失败按各子组件错误提示」未逐组件构造失败（各子组件既有错误态测试不在本套件范围）。
7. **安全 a1 分句**：「只有显式 false 时显示关闭警告」通过「未初始化 loading 不把未加载当关闭」用例覆盖；「关闭警告文案」
   本身未单独断言（非行为判据）。
