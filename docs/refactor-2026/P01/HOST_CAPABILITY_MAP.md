# HOST_CAPABILITY_MAP — 宿主能力可替换性清点（P01-T03）

版本：1.0｜证据基线：HEAD `92c6646c5` + 本阶段实测（无桌面纵向链 9/9 绿，日志 `P01-T03-vertical-slice-r9.out`）。

## 1. 结论

**业务核心对 Electron 的静态依赖为 0**：`core/ lib/ server/ hub/ shared/ cli/` 全部源文件（.ts/.js/.mjs/.cjs）grep 无任何 `electron` 导入（P01-T06 检查器 `host-into-core` 规则将此固化为门禁）。无桌面运行是**已验证能力**，不是文件分离推断：真实 spawn `server/main-full.ts` 完成 prompt→模型→工具→存储→历史回读全链（见 server-composition-boundary Part 4 测试）。

## 2. 桌面能力的真实接线方式（注入/广播，非反向依赖）

| 能力 | 业务核心侧（无 Electron） | 桌面宿主侧 | headless 行为（实测） |
|---|---|---|---|
| 通知（notify 工具 → 桌面弹窗） | `lib/notifications/notification-service.ts` `_deliverDesktop` → `emitDesktop` 回调 → engine.ts:847 注入 → **EventBus 广播** `{type:"notification"}` | renderer 经 WS 订阅事件后用 Web Notification 展示 | **广播语义**：无订阅者时事件发入总线，`status:"sent"` 表示"已发往 desktop 通道"而非"已确认送达"（⚠️ 既有设计，通知面向重连模型；P01 不改变用户采纳的通知行为，语义澄清移交 P05 消息阶段评审）。工具执行不阻断、不崩溃，后续聊天正常（A10 实测） |
| 桌面自动化（Computer Use） | `lib/tools/computer-use-tool.ts:562` `options.getComputerHost?.()` 为 null → **显式抛错** `ComputerHost is unavailable` | engine `_ensureComputerRuntime`（engine.ts:2408）按平台装配 macOS CUA/Windows UIA provider | 显式不可用（错误文本直达模型），静默成功为零 |
| 浏览器（browser 工具） | `lib/tools/browser-tool.ts`（headless browser，自带进程管理） | 共享窗口经 `browser_tab`/`browser_bg_status` 事件投影（session-coordinator.ts:8719） | 服务内自足，不依赖桌面 |
| 系统对话框/托盘/窗口 | 无核心依赖 | desktop/main.cjs（electron 唯一合法层） | 不适用（纯宿主内部） |
| 模型/凭证 | ModelRuntime + AuthStorage（`{LINGXI_HOME}/auth.json`、provider-catalog.json） | 桌面仅经 HTTP/WS 配置 API | 完整可用（纵向链实测 witness provider 全链） |

## 3. 权限与入口一致性（T03-3）

桌面、CLI、Bridge 共享同一 `hub.send` 路由表与 `session-permission-wrapper` 权限裁决（P00 E-DESKTOP/E-CLI/E-BRIDGE 收敛判定）；后台入口（cron/channel/dm）执行层同源、trace 强制新根、`allowHumanApproval=false`。**同一可信主体/作用域/策略输入 → 同一语义裁决**已由 P00 调用链核对背书；差异（bridge guest、automation 模式）是输入差异，不是第二裁决体系。

## 4. 无桌面集成测试（T03-4 交付）

`tests/server-composition-boundary.test.ts` Part 4（新增，vitest 内真实 spawn）：

- 隔离 LINGXI_HOME（mkdtemp）+ 预置 witness provider（本地 OpenAI 兼容协议 server，锁定 Pi 0.86.0 真实走 HTTP/SSE）
- 桌面业务入口：`POST /api/sessions/new` + WS `/ws` prompt（与 renderer 完全同入口）
- 真实 read 工具读真实文件 → C2 回答 → `GET /api/sessions/messages` 回读（断言工具 args/output、assistant 段、modelCallRef 均落历史）
- 预置有效 `agents/lingxi/config.yaml`（`models.chat: {id, provider}` 对象形态——字符串会被判为未配置）避免 first-run 触碰真实 ~/Desktop
- 证据：`P01-T03-vertical-slice-r1..r9.out`（r1-r8 为失败迭代留档：catalog 版本字段名、config chat 形态、结算事件名、witness 路由按最后用户消息）

## 5. 登记与移交

1. ⚠️ 桌面通知通道"sent=已广播"语义 → P05（消息语义阶段）评审是否需要投递回执；P01 不改（用户采纳行为红线）。
2. 首启 roster/标题等后台生成任务在 headless 也会跑（拿到默认回复后 warn 放弃）——无害但耗一次模型调用，登记供 P04 观测确认。
3. `~/.lingxi` 哨兵零写入由 P00-A03 隔离证明背书；本测试的 LINGXI_HOME/workspace 全部 mkdtemp。
