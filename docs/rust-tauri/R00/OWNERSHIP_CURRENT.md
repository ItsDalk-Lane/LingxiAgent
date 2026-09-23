# R00-T04｜入口、身份、数据与权限现状（OWNERSHIP_CURRENT）

状态：READY_FOR_REVIEW（R00-T04 交付物，非独立验收结论）。
基线：分支 `codex/rust-tauri-migration`，HEAD `ffcb85830ffdc75da5fe39b7a8cfddb4617f1aec`（Task base 相同）。
生成依据：本文件全部结论以当前源码锚点支撑；机器可读清单见同目录 [ENTRYPOINTS.json](ENTRYPOINTS.json) 与 [STORES.json](STORES.json)，可复算脚本 [r00_t04_scan.py](r00_t04_scan.py)。

## 1. 当前身份与权限模型（唯一权威：server 认证层）

生产认证只有三个 principal 来源，全部在 server 进程内解析（`core/server-auth.ts:35-90`）：

| principal | 凭据来源 | 允许连接 | 权限 |
|---|---|---|---|
| `local_user` | 环回 token：`LINGXI_TOKEN` 或 `server-info.json` 内 128-bit token（server 写入，0o600，`server/index.ts:1277-1300`） | 仅 local | owner 级：LOCAL_ONLY 直通 + 全部 scope |
| `device` | `devices.json` 设备凭据（配对审批签发，`server/routes/devices.ts:34-123`） | lan / tunnel（按 trustState） | 按 scopes 限权 |
| `web_session` | 口令登录签发的 `hana_session` cookie（14 天，`core/web-session-store.ts:31-77`） | 按连接类型 | 按 scopes 限权 |

路由授权唯一入口 `authorizeHttpRoute`（`server/http/route-security.ts:8-171`）：PUBLIC / authenticated / local_only / studio_owner / scope 五类，`/api/*` 未列举即落入 STUDIO_OWNER、非 `/api` 落入 LOCAL_ONLY（fail-closed）。scope 支持 `namespace` 与 `namespace.*` 通配（`route-security.ts:286-291`）。WS 面两条：`/ws` 经 `POST /api/ws-ticket`（scope chat）换短期 ticket（`server/routes/ws-auth.ts:8-21`）；`/internal/browser` 原生 ws 升级前同样过 authenticateRequest + authorizeHttpRoute（`server/index.ts:1186-1199`）。

**没有**第三套权限系统：工具执行授权由会话权限包装器唯一承担（`lib/tools/session-permission-wrapper.ts:656-790`：输入快照 → 会话绑定 → 硬安全规则 → 工具契约 resolver → classifySessionPermission → allow/deny/prompt → 执行前重检）。网关层（`core/tool-invocation-gateway.ts`）在模型面的 authorize 回调显式抛错（`core/engine.ts:4119-4124`），防止绕过包装器。

非 HTTP 入口不产生新 principal：CLI 复用环回 token；Bridge 身份=平台 userId 与 `agent.config.bridge[platform].owner` 配置比对（`lib/bridge/owner-policy.ts:8-17`）；cron/heartbeat/频道/DM/子代理是无用户 principal 的 automation/进程内主体，其授权来自 executeIsolated 固定选项（`permissionMode=auto`、`approvalPolicy=deny_on_prompt`、`allowHumanApproval=false`、`permissionContext.surface=automation`，`hub/scheduler.ts:389-402`；ephemeral 路由同参数，`hub/index.ts:334-339`）。

## 2. 入口全景（37 条登记 = 35 现役 + 休眠/残留各 1；计数由 `r00_t04_scan.py --validate` A2 检查按 entries 机械校验）

按类别（详表见 ENTRYPOINTS.json，每条带锚点与七要素）：

- **desktop_shell（7）**：bootstrap 入口、server spawn/复用、主窗、辅助窗口面、托盘/单实例/通知/自启动/防休眠、更新链（updater + OTA 列车 + GitHub Releases）、辅助原生子进程（office-pdf/computer-use/speech/win guardian）。无深链协议注册。
- **renderer_clients（2）**：Vite 多入口（8 页面）；`/mobile`、`/desktop` HTTP 供货的 PWA（Mobile/LAN Web）。
- **http_ws_server（7）**：认证中间件链、开放/闭集路由组合（REST 字面路径 392 条）、聊天 WS `/ws` + ws-ticket、`/internal/browser` WS、web 登录面、设备配对面、access 网络模式 + attach CLI。
- **cli（3）**：`hana` 命令面（serve/chat/continue/status/sessions/bundle/data）、data 维护面（epoch 安全链）、server-runner 拉起。
- **bridge（3）**：五平台适配器（telegram 长轮询 / feishu WSClient / dingtalk Stream WS / qq 自建 WS / wechat iLink 轮询——全部出站拉取，**无入站 webhook**）、owner 判定、`/api/bridge/*` 管理面（scope bridge.manage）。
- **schedulers（3）**：heartbeat（31 分钟对齐槽位）、studio cron（60s tick + markRun CAS 收据 + 并发锁 + 20 分钟超时 + 唤醒补触发）、loop 闹钟（recoverAtBoot 重武装）+ fresh-compact 日常维护。
- **channels_dm（4）**：ChannelRouter+ticker、DmRouter（3 轮上限、双写 dm 文件）、phone 会话执行器（含临时会话 runAgentSession）、guest 链 + hub.send 四路路由表。
- **subagents（1）**：subagent/subagent_reply/subagent_close 三工具 + 权限衰减（read→READ_ONLY；write 需父会话非只读）+ runs/threads 双 store。
- **plugins_mcp（5：现役 3 + 休眠 1 + 残留 1）**：内置插件（office/beautify/media/jimeng-cli，builtin 扫描 + freshImport 生命周期）、MCP 双挂载面 `/api/mcp`+`/api/plugins/mcp`（OAuth 回调 PUBLIC；app-tools 调用 STUDIO_OWNER 直调 client.callTool）、vite dev 渲染端开发链（现役）；休眠的开发者网关面（local-developer principal，无生产接线）、残留分类（`/api/plugins/dev` 路由已删）。
- **tool_boundary（2）**：网关 + 包装器、hub.send 路由表。

**A08 四条实链**（完整逐跳锚点见 ENTRYPOINTS.json `a08_chains`）：CLI（server-info token → /ws → hub.send owner → engine.prompt → 包装器 → 工具）；Bridge（telegram 轮询 → _handleMessage → owner 判定 → executeExternalMessage → bridge 会话 → 工具）；cron（tick → markRun 收据 → executeIsolated automation 选项 → 隔离会话 → 工具）；开发调用（vite dev 渲染端经 CORS 白名单 + 环回 token → 同桌面链）。四条链身份/授权来源均已锚定；Bridge 的平台账号体系属外部信任边界，列为风险而非已证实安全。

## 3. 数据所有权现状（69 个注册 store）

权威登记表 `shared/persistence/store-registry.ts`（PERSISTENT_STORES，69 store + 38 豁免），由 `tests/persistence-store-registry.test.ts` 的 AST 常谱强制“每个生产持久化位点恰好一个归属”（788 个发现位点，本任务实测 14/14 通过），并由 schema 指纹 tripwire（`scripts/check-persistence-schema-fingerprint.mjs`，170 个被守卫源）防漂移。

按注册表 epochPolicy 分类（STORES.json 逐项含 rebuild/loss 语义，引自注册表自身声明，可核实）：

- **authoritative（epoch-managed，42）**：会话 JSONL/清单/侧车、记忆（md+sqlite）、事实库、知识库（db+sources）、文件历史、用量账本、agent 档案、频道、cron 任务与收据、DM/phone 记录、技能/插件/MCP 配置、草稿、观测 db、凭证/授权/密钥/设备注册表、网络配置、epoch 元数据等。**server 进程是全部这些 store 的唯一写进程**（CLI data 维护面在探测确认无活内核后才写 epoch 检查点/隔离区）。
- **rebuildable_cache（regenerable，13）**：knowledge/indexes（ANN 可从保留的向量 BLOB 重建）、workspace-snapshots（影子 git 可重捕；sidecar 丢失时降级 write/edit 备份）、session-checkpoints（仅丢失回退锚点，不丢对话）、context-notes/goal sidecar（建议性账本）、skill 翻译缓存、pi-sdk 托管二进制、上传/角色卡/封面/office 作业暂存、OS tmpdir 扫描草稿。
- **adjacent_compatible（compatible，14）**：桌面壳自态（窗口/GPU/更新通道/版本指纹/诊断）、server-info、logs、browser 冷保存、观测 blobs、signed-artifacts（OTA 列车）、epoch 检查点/隔离区。

跨进程分工（STORES.json `processes` 字段逐项）：server 写 62 项；desktop main 只写自身 compatible 壳态 5 项 + OTA 列车（列车应用固定 stop-server→promote→start-server 序列，`desktop/src/shared/train-update-apply.cjs:29-45`）；desktop 另对 `server-info.json` 承担**仅删除性** unlink（stale/死内核探测清理、spawn 前清旧文件、shutdownServer 关停去留，`desktop/main.cjs:1364/1368/1915/6635`，不创建/写入内容，内容唯一写者 server——见 STORES `server-runtime-info` processes 注记）；desktop 对 `user/preferences.json` **只读**（写者唯一为 server，PUT /api/config）；CLI 仅在维护面写 epoch 元数据（有活内核探测闸）。

## 4. 数据 epoch / schema 指纹 / 旧版本拒写 / 实际发布版本

- **契约号**：`shared/contract-versions.json` = PRELOAD_API_VERSION 1 / SERVER_PROTOCOL_VERSION 1 / **DATA_EPOCH 1**（单一来源，.cjs/.ts 双侧共用）。
- **启动闸顺序**（`server/index.ts:273-349`）：同宅互斥闸（token 探测 server-info，活异内核→exit 1；not-hana/dead→自清残留锁）→ 数据 epoch 事务闸（任何 store/端口打开之前）。epoch=1 基线上对损坏 epoch 元数据降级为告警继续，但**更高印章或更高目标迁移的具体证据仍然阻断**（`hasHigherStamp`/`hasHigherTransition`，`index.ts:324-347`）；未来 epoch>1 后全部 coordinator 失败恢复严格。
- **旧内核拒写机制**：`coordinateDataEpochStartup`（`core/data-epoch-coordinator.ts:514-692`）——stamp.minimumReaderEpoch > ownEpoch 时返回 `epoch-downgrade-blocked` → 进程 exit 1（桌面识别 `LINGXI_DATA_EPOCH_BLOCKED` 机读标记弹双语对话框）；唯一逃生门是显式 `LINGXI_ALLOW_DATA_DOWNGRADE=1`（接受损坏风险的告警式放行）。半途迁移（journal 存在）一律 `incomplete-transition` 阻断，普通启动不自动续跑/回滚。未盖章家目录按 bootstrap-safe 路径分类 provably-new / legacy-baseline / ambiguous（ambiguous 阻断）。
- **schema 指纹**：`build/persistence-schema-fingerprint.json`（dataEpoch=1 + 逐豁免指纹）；`scripts/check-persistence-schema-fingerprint.mjs` 实测 170 个被守卫源无改动、退出码 0；`tests/persistence-schema-tripwire.test.ts` 防止守卫源被触碰而不更新指纹。
- **另一层旧版本闸（OTA 列车）**：列车 manifest `contract.preload` > 壳 PRELOAD_API_VERSION 时拒绝激活（`shared/artifact-core/ota-core.cjs:1051-1065`）；serverProtocol 仅诊断不阻断。
- **实际发布版本**：package.json 0.1.43 = 最新 tag `v0.1.43`（2026-09-22，release-digest.v2.json 首条 24 条目）；git tag 另有历史序列 v0.98.x（旧命名）与 train-stable-N。**所有已发布版本 DATA_EPOCH 均为 1**，生产从未发生 epoch 迁移（协调器的迁移路径为“装好待用”状态，`index.ts:305-310` 注释明示）。

## 5. 跨进程共享写入风险（迁移必须保持/解决）

| # | 风险 | 证据 | 现状 |
|---|---|---|---|
| R-T04-01 | 双内核冷启动竞态：两进程同时冷启动、都未写 server-info 的秒级窗口无防；第二者同端口才靠 EADDRINUSE 兜底 | `server/index.ts:270-273` 注释自认 | 已知接受的残余（与 postmaster.pid 同取舍）；R02 Rust 单写者设计需复刻此闸或更强 |
| R-T04-02 | loopback 端口自愈写 `server-network.json`：fallback 仅在探测确认占端口者“不是 hana”时执行 | `server/index.ts:159-179` | 探测闸已收窄；迁移时注意该写发生在启动早期（epoch 闸之后） |
| R-T04-03 | **AST 常谱对 DI 注入 fs 接收器的盲区**：`desktop/mac-self-install.cjs:290` 经注入 `fsImpl.writeFileSync` 写 tmpdir 一次性安装脚本，未被 788 位常谱收录（扫描器只追踪直接 fs 绑定，`scripts/scan-persistent-stores.mjs:226-235`） | 本任务 oracle 发现并分类 | 本次判定非权威存储（tmpdir 一次性、非 LINGXI_HOME）；但盲区**类别**对 R02 单写者执法是真实前提风险：Rust 侧不得只按“直接调用”识别写点 |
| R-T04-04 | MCP app-tools 直调：`/api/plugins/mcp/.../app-tools/:tool/call`（STUDIO_OWNER）直接 `client.callTool`（`core/mcp/manager.ts:2133-2165`），不经 ToolInvocationGateway/会话权限包装器 | 本任务链路追踪 | 属 UI 面对外部连接器工具的显式豁免；R04 统一网关时必须显式裁决收编或保留豁免理由，不得默认沿用 |
| R-T04-05 | Bridge 身份依赖外部平台账号体系：owner 判定=配置 userId 比对，平台侧账号被盗即冒充 owner | `lib/bridge/owner-policy.ts:8-17` | 产品既定信任边界；迁移后同样只能列为外部信任假设，不可写成“已证实安全” |
| R-T04-06 | `preferences.json` 双进程读写：server 写、desktop main 读（keybindings/quick_chat/keep_awake/network_proxy/update_channel） | `desktop/main.cjs:176-212`（全部 read）| 单写者成立；迁移若让 Tauri 壳写偏好必须改回单一写进程，否则违反 R02 契约 |
| R-T04-07 | 休眠开发面残留：网关 `prepareAndInvokeForLocalDeveloper`（plugin-dev-http 路由）无生产接线；`/api/plugins/dev` 分类残留；plugin-manager `source:"dev"` 死分支 | `core/tool-invocation-gateway.ts:506`；`route-security.ts:351-352`；`core/plugin-manager.ts:108-130` | R04 统一工具网关时显式裁决，不默认恢复也不静默删除语义 |

## 6. 与任务书目标契约的差距提示（供 R01+）

- 「调度触发去重 → Scheduler 触发收据」现状为 cron-store markRun CAS + `_executingJobs` 内存锁（`lib/desk/cron-store.ts:746-779`、`hub/scheduler.ts:276-315`）：重启后已过 nextRunAt 的任务会被补触发一次，收据防的是同 job 并发与配置漂移，不是跨重启 exactly-once——与契约表格语义一致，但 Rust 重建时需保持同等证明强度。
- 「每会话状态修改串行 + 全局预算并发」当前由 Pi SDK 会话机制承担（R00-T03 已盘点），T04 不重复展开。
- epoch 迁移路径（checkpoint provider、journal 状态机、restore 隔离区）已完整装好但从未在生产触发（epoch 恒 1）；R02 的 ADR 可直接复用该机制作为“旧版本实际理解的 epoch 拒写”证据基础。
