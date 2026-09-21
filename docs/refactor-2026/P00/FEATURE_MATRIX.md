# FEATURE_MATRIX — 灵犀当前采纳功能清单与排除项（P00-T02）

版本：1.0｜执行日期：2026-09-21｜证据基线：HEAD `92c6646c5`（与研究 SHA `8037fae7a` 无源码差异）。
原始证据：`artifacts/refactor-2026/P00/logs/agent-feature-inventory-raw.md`（含全部 文件:行 引用）、
`agent-callsite-raw.md`、`agent-ownership-raw.md`。本表按 P00 任务书 §5-T02 第2条的最小覆盖面组织。

分类图例：**已采纳**（本轮保护范围）｜**外装扩展**（用户电脑上安装、非仓库自带，不纳入内置清点）｜**已撤回**（本轮硬排除，不恢复）｜**仓库残留**（仍在生产路径或数据兼容，只登记不删）｜**待确认**（证据不足以分类）。

## 1. 功能矩阵

| # | 功能 | 分类 | 用户动作→入口（文件:行） | 可观察结果 | 数据所有者 | 现有测试（代表） | 归属阶段 |
|---|---|---|---|---|---|---|---|
| F01 | 聊天与会话（桌面主界面） | 已采纳 | 输入框→`InputArea.tsx:1814`→发送租约`composer-send-coordinator.ts:1000`→WS `/ws`（`server/routes/chat.ts:2256`）→`hub.send`（`hub/index.ts:213`）→`submitDesktopSessionMessage`→`promptSession`（`session-coordinator.ts:5176`） | 流式回复+历史持久化 | session JSONL（Pi SDK SessionManager）+ session-manifest.db | `tests/sessions-route.test.ts`(121)、`tests/chat-route-*.test.ts` | P02/P05 |
| F02 | 会话管理（列表/搜索/置顶/归档/fork） | 已采纳 | 侧栏→`server/routes/sessions.ts`（40+ 端点） | 会话列表与元数据 | SessionManifestStore→session-manifest.db | `tests/sessions-route.test.ts`、`tests/history-read-directory-*.test.ts`(9) | P05 |
| F03 | 四基础工具常驻+按需目录 | 已采纳 | `shared/tool-categories.ts:103` RESIDENT=read/write/edit/exec_command；其余经 mcp_search_tools/describe/call 按需 | 工具执行经网关 | ToolInvocationGateway（`core/tool-invocation-gateway.ts`） | `tests/tool-invocation-gateway.test.ts`、`tests/tool-invocation-boundary*.test.ts` | P03（UNCHANGED_VERIFIED 候选） |
| F04 | 审批与沙盒（权限模式） | 已采纳 | `lib/tools/session-permission-wrapper.ts:656`（evaluateToolSafetyPolicy→resolveToolInvocationPermission→classifySessionPermission→allow/deny/review）；沙盒 `lib/sandbox/` | 越权拒绝/需审批 | ConfirmStore+审批网关（`lib/approval-gateway.ts`） | `tests/session-permission-*.test.ts`、`tests/*sandbox*` | P03 |
| F05 | 子代理（现役能力） | 已采纳 | `lib/tools/subagent-tool.ts`+`subagent-tool-policy.ts`；UI `stores/subagent-preview-slice.ts` | 派发子任务、权限衰减 | `lib/subagent-run-store.ts`/`subagent-thread-store.ts`（subagent-runs.json/threads.json） | `tests/subagent-*.test.ts`(5) | P02/P03 |
| F06 | 人格与 MOOD | 已采纳 | `core/agent.ts`（人格拼装、persona 文件）；MOOD 流事件 | 系统提示词含人格/心情 | agentDir persona/memory | `tests/agent-system-prompt-equivalence.test.ts`（golden zh/en） | P06 |
| F07 | 记忆（事实/总结/信条/梦境） | 已采纳 | `lib/memory/`（fact-store、session-summary、tenets）；`server/routes/memory-dream.ts` | 记忆固化与检索 | `agents/{id}/memory/facts.db` | `tests/memory-*.test.ts`(19) | P06 |
| F08 | 知识库（现行） | 已采纳 | `server/routes/knowledge.ts`（notebooks/sources/import/citations） | 导入建库、注入引用 | `knowledge/knowledge.db`（v19） | `tests/knowledge-*.test.ts`(61)、CI knowledge smoke | P06 |
| F09 | 资源与交付文件（SessionFile/Resource） | 已采纳 | `lib/session-files/session-file-registry.ts`（sf_ id+sidecar）；`stage_files`（`lib/tools/output-file-tool.ts`） | 文件随会话管理、卡片交付 | SessionFileRegistry（sidecar `{sessionPath}.files.json`+缓存目录） | `tests/resource-io-*.test.ts`(16)、`tests/history-run-outcome-edges.test.ts` | P05 |
| F10 | 模型与媒体配置 | 已采纳 | 设置→Providers/Models（`server/routes/providers.ts`、`models.ts`）；媒体生成 `core/media/universal-media-manager.ts`+内置插件 | 模型/凭证管理、生图/生视频/语音 | provider-catalog.json（API key）+auth.json（OAuth） | `tests/provider-compat/`(16)、`tests/model-manager-*.test.ts` | P04 |
| F11 | 浏览器/终端/桌面系统能力 | 已采纳 | `lib/tools/browser-tool.ts`+`lib/browser/browser-manager.ts`+`/internal/browser` WS；终端 `lib/exec-command/`+node-pty；computer use `lib/tools/computer-use-tool.ts` | 网页浏览、命令执行、屏幕操作 | 会话目录+plugin-data | `tests/browser-*.test.ts`(8)、`tests/terminal-*.test.ts`(4)、`tests/computer-use-*.test.ts`(12) | P03/P04 |
| F12 | Bridge 多平台（TG/飞书/钉钉/QQ/微信） | 已采纳 | `lib/bridge/bridge-manager.ts`（5 adapter 注册）→`_handleMessage`→owner 判定→`hub.send` | 外部消息进同一执行链 | bridge 专属 session 目录 | `tests/bridge-*.test.ts`(27) | P02 |
| F13 | Web/移动端（LAN/PWA） | 已采纳 | `server/routes/mobile-static.ts`+`mobile-workbench.ts`；设备密钥 `routes/devices.ts`、`web-auth.ts` | 手机/浏览器访问 | devices.json/device-credentials.json | `tests/server-*.test.ts`、`tests/mobile-*` | P04/P05 |
| F14 | CLI | 已采纳 | `cli/entry.ts`（serve/status/sessions/continue/chat/bundle/data）→同 `/ws` 路由 | 终端会话 | 同 server | `tests/cli-*.test.ts`(9) | P02 |
| F15 | 后台任务（心跳/定时/频道/DM） | 已采纳 | `hub/scheduler.ts`（heartbeat/cron→executeIsolated）；`hub/channel-router.ts`、`hub/dm-router.ts`（runAgentPhoneSession） | 定时巡检、群聊、Agent 互信 | TaskRegistry（.ephemeral/plugin-tasks.json） | `tests/task-registry.test.ts`、`tests/loop/`、`tests/agent-executor-teardown.test.ts` | P02 |
| F16 | 用量观测（model observability） | 已采纳 | `lib/extensions/model-call-observer-ext.ts`+`lib/pi-sdk/model-call-stream-observer.ts`→`model-observability/observability.sqlite`；路由 `routes/model-observability.ts`、`routes/usage.ts` | 用量页/导出 | observability.sqlite（SCHEMA_VERSION=7） | `tests/model-observability-*.test.ts`(28)、`tests/model-call-*.test.ts`(11) | P04 |
| F17 | 导入导出（角色卡/技能包/观测导出） | 已采纳 | `server/routes/character-cards.ts`、`cards.ts`（闭集）；观测导出 IPC `observability-export:*`（main.cjs:5804-5863） | zip 导入导出 | cards 目录+导出文件 | `tests/character-card*`、`tests/model-observability-export*` | P05/P08 |
| F18 | 更新（OTA/GitHub Releases） | 已采纳 | 「检查更新」`desktop/src/shared/github-release-check.cjs`；发布列车 `main.cjs:5319-5354` train-update-* | 检查/下载/应用更新 | release-digest.v1/v2.json | `tests/release-preflight.test.ts`、`tests/artifact-*` | P08 |
| F19 | 技能系统（SKILLS/内置技能） | 已采纳 | `skills2set/` 首启同步（`core/first-run.ts:107`）；管理 `routes/skills.ts` | 技能安装/审核 | `~/.lingxi/skills/` | `tests/skill-*.test.ts`(6)、`tests/external-skill-paths-contract.test.ts` | P06 |
| F20 | 多语言 i18n | 已采纳 | `desktop/src/locales/{zh,en,ja,ko,zh-TW}.json` | 界面语言切换 | locale 文件 | `tests/i18n-*.test.ts`(3) | 范围外（不改 UI） |

## 2. 排除项与残留登记（P00-A04）

| 项 | 分类 | 现状证据 | 处置 |
|---|---|---|---|
| 专用子代理目录/强制分发改造 | **已撤回（硬排除）** | 现役替代=可选实验 `subagent.proactive_delegation`（`lib/experiments/registry.ts:15,121-145`，beta 默认 false；消费点 `core/agent.ts:933,1756`；入口 `settings/tabs/ExperimentsTab.tsx:21,446`） | 不进入实现任务；实验开关本身是现役产品行为，保留 |
| 独立知识研究（knowledge research） | **已撤回（硬排除）** | 文档残留 `docs/archives/knowledge-retrieval-research/`（6份）；**生产路径残留**：`lib/knowledge/knowledge-store.ts:1021-1125` research_runs/research_jobs 等 9+ 张表仍在建表、`lib/knowledge/types.ts:5-162` 类型、`knowledge-manager.ts` 删除保护引用；无任何运行时启动入口 | 只登记；表结构有旧数据兼容用途，退出归 P05/P08 决策 |
| 本地模型管理子系统 | **已撤回（已移除）** | 生产代码无痕迹（全域 grep 空）；现存 Ollama 是模型接入（`lib/providers/ollama.ts`）非管理子系统 | 无需动作；注意不把 Ollama 接入误判为恢复该子系统 |
| `skills2set/lingxi-plugin-creator/` | **仓库残留** | 仅 `scripts/__pycache__/*.pyc` 两个编译产物，无 SKILL.md，不可装载 | 登记；删除属后续清理授权（P08 或独立治理） |
| vitest/tsconfig `@hana/*`、`@lingxi/plugin-*` 别名 | **仓库残留** | `vitest.config.js` alias 与 `tsconfig.test.json` paths 指向已拆除的 `packages/`（AGENTS.md：04f90d2b2 已拆除 workspaces） | 登记；主责 P01（类型/工程边界阶段）核实是否可清退 |
| `scripts/check-tool-invocation-boundaries.mjs` SOURCE_ROOTS 含 `packages` | **无害残留** | `collectSourceFiles` 对不存在目录跳过 | 登记，不阻塞 |
| 电脑外装 MCP/插件 | **外装扩展** | MCP 走 `core/mcp/`（clients/manager 在边界白名单内）；用户自装插件经 PluginsTab | 不计入内置功能数；扩展边界用合成适配器覆盖（P03） |

## 3. 分类口径说明

- 「已采纳」判定依据 = 源码存在真实入口+路由/工具注册+README 声明交叉验证，不按目录名猜测。
- 外装插件/MCP 不当作仓库自带组件；`plugins/` 下 4 个（beautify/jimeng-cli/media/office）是随包分发的**内置**插件，属已采纳。
- 本轮不新增产品能力、不改 UI 布局（F20 等仅登记）。
