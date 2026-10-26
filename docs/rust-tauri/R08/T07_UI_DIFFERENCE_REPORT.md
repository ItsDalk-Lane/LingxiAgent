# R08-T07｜UI 差异报告（A13 同界面换内核 A/B + A14 失败不藏）

- 证据链：t07-ab-drill-01..04（红，保留）/ **t07-ab-drill-final-05（绑定绿证，15 任务：parity=12 / differs=0 / recorded=2 / B-failed=1）**；t07-real-format-probe-01..02（红）/ 03（绿）。
- 线束口径（固定四要素）：同一 React 构建源（jsdom 载入 desktop/src/react 真实 store 动作与客户端边界，零 fork）；数据 = 夹具 v3 基座 + **会话面替换为真实 Pi SDK 格式**（3 会话，两内核同见；夹具自造 entryType 会话对旧栈不可见——见 F-T07-1）；模型替身 = 本任务集不触模型回合（模型链由 T03 a05 系与 T06 D1 另证）；步骤 = `r08-t07-kernel-ab.test.ts` 固定任务集双跑。
- 驱动层级披露：UI 绑定层（store action / lingxiFetch = 按钮背后的唯一通路），非像素级 DOM 操作；窗口形态由 T03 Electron 深度腿另证。

## 平价面（12 任务）

身份握手、会话列表（计数/置顶面）、重命名（持久化回读）、置顶/重排/取消、归档维护与孤儿清扫、能力刷新、计划余项完成/取消/收纳、新会话默认权限、**外观偏好读/编辑/回读**、失效令牌拒绝（凭证失效）、不存在会话明确拒绝（不伪装空数据）、任务后列表可读性。其中重命名/置顶/计划余项在修复 F-T07-2 前于 Rust 腿全拒（见下），修复后与旧内核逐值平价。

## 差异与发现（逐项定位：服务契约 / 兼容转换 / UI 绑定）

| # | 差异/发现 | 层 | 定性 | 处置 |
|---|---|---|---|---|
| F-T07-1 | **真实会话格式不被 T04 导入链识别**：真实旧栈会话 JSONL 是 Pi SDK 格式（首行 `{type:"session"}` + `{type:"message",message:{role,content}}`，业务 id 由 session-manifest 派生）；T04 夹具用自造 `entryType` 格式，解析器只认顶层 role/ts → 真实条目全部落 `legacy-unknown-block`（原字节隔离、非静默丢失，但对话不可用）。决定性证据：探针 02（A 腿旧栈列出+可读，B 腿 conversational=0/unknownBlocks=4）| 兼容转换（legacy_import/session_jsonl.rs + convert_sessions.rs） | **阻断级（已修复）** | 修复（加性）：解析器识别 Pi 形状（头行≡sessionMeta、message.role 嵌套、ISO 时间→unix ms、toolResult→已知非消息类）；content 规范化提升嵌套 message.content。夹具行为零变化（amend 双跑 435/5206/双锚保持，t07-pi-fix-amend-proof-01）。探针 03 绿（conversational=3/unknown=0）。**治理注记：T04–T06 全部绿证据原本只对该自造格式成立，对真实数据不构成证据——本修复 + 探针后链条对真实格式亦闭合（fixture 路径未动，夹具证据继续有效）** |
| F-T07-2 | **客户端边界缺会话管理写端口翻译**：rename/pin/archive/restore/delete/cleanup/sweep/pin-order/todos/fresh-compact 在 Rust 连接下被 `buildConnectionUrl` 响亮拒（"no migrated endpoint"），而 Rust 服务路由全部存在（T01 绑定表已判 MAPPED）| UI 绑定/兼容转换（use-hana-fetch.ts translateLegacySessionCall） | 平价缺口（已修复） | 修复（加性）：翻译表补 10 条（body `path:"native:<id>"` → `{sessionId}` + camelCase + deny_unknown_fields 对齐）；**连接门**：翻译仅对 `serverNodeKind==='lingxi-service'` 生效（无门时旧腿被误翻译 404——t07-ab-drill-03 A 腿实证，属修复过程引入又修掉的真 bug，红证据在案） |
| D-1 | 会话标题投影：旧内核对无题会话返回 `null`（UI 走回退显示），Rust 列表以 id 干（"ab-real-alpha"）为题 | UI 绑定（列表投影） | 已知差异（记录） | 差异报告披露；产品裁决（保留/对齐）移交 post-R08 |
| D-2 | B 腿 `sessions/new` 404：迁移根无 agents/ 名册（Register 类不随迁，T04 设计）→ `agent not found: a1` | 迁移设计 | 已知语义（记录） | 补偿操作=正式入口补建同 id agent（T06 D1 已证）；T07 对照：真实切换演练须含此步骤（入 runbook/交接） |
| D-3 | A 腿 `sessions/switch` 500：旧栈无模型配置时拒开会话运行时（环境前置，A13 模型替身未注入 A 腿）| 环境前置 | recorded | 完整 A/B（含聊天回合）需旧格式 models.json 注入——移交 post-R08 扩腿 |
| D-4 | ~~B 侧 rename 标题跨重载回落~~ **已推翻（2026-10-24）**：探针 t10-d4-probe-01 证明服务端 rename→DB 持久化→studio-list 重载（0/500/2000ms）全部返回新标题——服务端无缺陷。套件内 B-failed 真因=任务序列级联（rename 目标与 boundary 固定 byTitle 假设冲突），套件已修（t10-ab-drill-d04-01 起 boundary 目标与 rename 目标严格不相交） | 套件缺陷（已修） | **改判关闭** | 探针 t10-d4-probe-01；套件修复 t10-ab-drill-d04-01 |
| D-7 | **新发现**：switch 投影 permissionMode——旧栈归一化默认 mode（A 腿无模型 500 不可读，D-3 同根）；Rust 透传 DB 原值（无显式模式的迁移会话=null） | 服务契约（switch 投影语义） | 未决差异（如实） | t10-ab-drill-d04-01（B: status 200 modeValue null）；post-R08 裁决（归一化 vs 透传） |
| D-5 | A/B 双腿 `sessions.create` 皆 false：A=D-3 同因；B=D-2 同因 | 环境前置 | recorded | 同 D-2/D-3 |

## A14 失败不靠隐藏消失（证据）

- 修复前（t07-ab-drill-02/03）：Rust 腿对 rename/pin/todos 等**响亮报错**（UI 报错 toast + console error + 请求 4xx/异常），入口（store 动作/按钮通路）**存在且未被禁用**——没有任何 `disabled` 冒充完成。
- 修复后（drill-04/05）：同一入口真实可用并达平价。
- 错误路径任务（unauthorized/not-found）双腿均显式拒绝且语义一致——空数据伪装在两侧都被拒。

## 叶覆盖诚实声明

本线束覆盖 D02 会话管理族约 20 个叶的**入口通路**（列表/切换/重命名/置顶/重排/归档/恢复/删除/清理/清扫/能力/计划余项）+ D04 权限（默认权限）+ 设置编辑 + 两条错误路径。D01 聊天/WS、D03 MCP 连接器操作、D13 terminal WS、D20 CLI、以及 D02 其余叶（项目/文件夹/地图/搜索/内容分页等）**未在本线束执行**——如实标 required_behavior_pending（叶绑定见 stage map），不以本报告的绿推定全量。

## R08 收尾轮（2026-10-25，closeout 会话）差异处置

| # | 差异/发现 | 层 | 定性 | 处置 |
|---|---|---|---|---|
| D-2/D-3/D-5 | A 腿无模型 / B 腿无 agent 名册 | 环境前置 | **消除**（线束） | A 腿：`agents/a1/config.yaml` 持久 `models.chat:{id,provider}` 选定同一桩模型（/api/models/set 只是一次性 pending）；B 腿：服务 `--config providers` 注入同一桩（d3 同款）+ 正式入口补建 a1（T06 runbook 补偿步骤）。switch/create 由 recorded 改为真实对拍并平价。 |
| D-7 | switch 投影 permissionMode | 服务契约 | **已关闭** | Rust 行投影 null→"ask" 归一（上轮）；本轮 A 腿有模型后 switch 200，双腿 modePresent/idempotent 平价。 |
| D-9 | cleanup 期限计数（旧栈 mtime vs Rust archived_at） | 夹具放大 | **消除**（夹具对齐） | 真实旧根 mtime≈归档时刻；线束把 A 腿归档文件 mtime 对齐到 sessionMeta.createdAt 后 `sessions.maintenance`/`leaf.cleanup.boundary` 平价。不改任一内核语义。 |
| D-10 | 会话内文本查找 | 兼容转换/UI 绑定 | **已关闭**（C③） | Rust 已有 `/sessions/{id}/find?q=`；客户端粗映射错指行定位路由。翻译表接 `find` 读端口；Rust 路由接线 Bridge 脱敏（桥会话表挂接判定）。`leaf.find.in-session` 平价。 |
| D-11 | **新发现**：旧栈在助手配置模型后**启动即自动建会话**（首请求前 createSession），Rust 服务端不自动建（客户端待建草稿驱动） | 架构位置 | 需确认（建议接受为等价实现） | 平价计数排除启动自建会话并记录 `bootAutoCreated`；见 CLOSEOUT_BLOCKED.md。 |
| D-12 | **新发现**：`POST /session-permission-mode` 非法 mode——旧栈静默接受/归一（200），Rust 显式 400 | 服务契约（校验严格度） | Rust 更严格，不降级 | 不进平价 cases（`invalidProbe` 如实入观测）；两叶拒绝分支以鉴权不足为共同面。 |
| 幽灵 agent | **Rust 缺陷（已修）**：`new-detached` 只查已删标记不查存在性，不存在 agentId 也 201 建会话 | 服务契约 | 缺陷 | lib.rs new_detached_session_route 补 `agent_dir_checked`；A/B 双腿 invalid-agent 显式拒平价。 |
| auto 模式 | **Rust 缺陷（已修）**：UI 默认偏好 `permissionMode:"auto"` 建会话被 `sessions/new` 400（kernel 枚举无 auto；旧栈接受）→ Rust 腿真实 UI 流 createNewSession 不能建会话 | 服务契约 | 缺陷 | session_focus 按 userprefs 既定裁决把 auto 解析为默认偏好具体模式（默认为 auto 时取 operate）。 |
| 删除项目回执 | Rust 回 `assignment.sessionIds`，UI 只读 `sessionPaths`（本地归属不落未分类） | UI 绑定 | 接缝 | session-project-actions 两形状都映射到本地路径。 |
| FIXTURE-PROMPT-SNAPSHOT | A 腿合成会话无冻结 sidecar → prompt-snapshot 500 | 夹具前提 | recorded（已登记基础） | 叶 FF4F06011B48 保持 pending；聊天线束产生真实回合后再对拍。 |
| 限流 | Rust 默认 240 请求/10s 把密集合成客户端的无关读面打成 429 | 测试准入 | 线束 | B 腿以 `--http-rate-max`（config.rs 文档化准入旋钮）放宽；产品默认不改。 |

## R08 续跑第三轮差异处置（2026-10-26）

| # | 差异/发现 | 层 | 定性 | 处置 |
|---|---|---|---|---|
| D-15 TERMINAL | Rust 无终端会话子系统；此前 terminal_* 帧到 Rust 触发 unknown-variant → close 1008 断连 | 服务契约 | 传输缺陷已修 + 正路径留债 | 三帧识别+现役形状显式能力错误（T01_SCOPING 既定「实现或显式能力错误」）；close=旧栈未接线同款 rejected/terminal_unavailable；snapshot/tail=显式 error 帧（优于旧栈未处理异常）；断连缺陷修复有双腿传输存活断言。D13 三叶保持 pending（PTY 子系统=R09+ 桌面终端域）。 |
| MCP 会话授权形状 | 旧栈 POST {sessionId, capability}；Rust 需 {sessionId, targetId: tool:mcp:<conn>:<tool>, capability: mcp.<tool>.invoke} 且目标须当前已注册 | 服务契约 | 形状差（语义同：仅本会话放行不持久化） | A/B 案例按各自契约断言（grant 前置于 update——Rust 普通编辑沿语义停旧连接、工具目录随注销，时序在案）。 |
| MCP add id 语义 | 旧栈无 id 时以 name 生成 id；Rust 直取 id | 服务契约 | 形状差 | 夹具显式携带 id 对齐；不判差异。 |
| CLI data 族 | 产品测试即验收证据 | 测试绑定 | 收口 | tests/cli-data.test.ts 20 用例（含新增 corrupt 印章用例）逐用例合同绑定三叶。 |

## R08 续跑第四轮差异处置（2026-10-26）

| # | 差异/发现 | 层 | 定性 | 处置 |
|---|---|---|---|---|
| 缺路由 | 旧 `/api/sessions/turns/rollback-preview`（重试文件回退预览）在 Rust 无对应面，Rust 连接下被硬闸拒 | 服务契约 | **已补实现** | 新增 `POST /lingxi/v1/sessions/{id}/turns/rollback-preview`（只读：偏好/检查点/文件清单三态）+ 客户端翻译；叶 477DA3D558B8 双腿绿。 |
| 删除不留墓碑 | Rust `DELETE /lingxi/v1/agents` 只移目录入事务，原位无 `.deleted-agent.json` → is-deleted/续接全看不到删除事实 | 服务契约 | **已修** | 提交删除后原位补写墓碑（现役同款约定）；deletion materials 校验视"只剩墓碑"为已无 agent。 |
| D-16 | 官方 DELETE 助手流程后，其名下会话 `get_for NotFound`（cont 路由 404）——删除/续接接缝 | 服务契约 | **真实产品缺口（未闭合）** | run-45..48 服务日志在案（detached/独立会话均复现）；continue 路由本身由直接墓碑路径证明工作（r08-continue-disposal-compat）。叶 3818ADEAFA7A 保持 pending。 |
| 投影双标识 | 旧栈历史投影 `id`=序号 vs `entryId`=真分支 id | 测试口径 | 辨析 | fork/retry 目标一律用 entryId/turnInputEntryId（分支真值）。 |
| session_busy 窗口 | 引擎收尾期 retry 409 | 语义 | 辨析 | 有界退避重试（6×2.5s），与现役客户端一致。 |

## R08 续跑第五轮差异处置（2026-10-26，工具回合线束）

| # | 差异/发现 | 层 | 定性 | 处置 |
|---|---|---|---|---|
| D-16（翻案） | 「官方 DELETE 助手后源会话 get_for NotFound」 | 线束 | **线束缺陷，非产品缺口** | 续接请求硬编码客户端 session id `r08-cont-s1`，Rust `new-detached` 返回服务端铸 id；独立探针（建助手→建会话→DELETE→续接）201 正常。测试改用返回 id + 补「助手存活时续接 400」负例；第六轮重跑后绑定叶 3818ADEAFA7A、撤 recorded。 |
| D-17 | **旧栈缺陷（已修）**：todo 面板/三条收尾路由/增量读/实时 tool_end 只按工具名识别 todo 结果，经 `mcp_call` 桥接的 todo_write（默认路径，todo_write 非常驻）全部失明 | 旧栈语义 | 缺陷 | `isTodoToolResult` 按名或版本化载荷形状识别（lib + 前端镜像 + incremental.ts + ws-message-handler），Rust 目标 id 前缀同口径；+9 单测。双腿 todos 四叶随之平价。 |
| D-18 | **旧栈缺陷（已修）**：checkpoint/rewind 工具用焦点回调取会话，非焦点会话的回合把存档点写入焦点会话侧车、确认卡打到焦点会话流 | 旧栈语义 | 缺陷 | 改按执行上下文 `getToolSessionPath(ctx)`（tool-session.ts 既定口径：不依赖焦点回调）。 |
| D-19 | **旧栈缺陷（已修）**：流式中已确认回档恒「session is busy」——agent.ts 经 `getEngine()?.getDeferredResultStore?.()` 取延迟账本，Engine 无此方法，延迟路径死码 | 旧栈语义 | 缺陷 | 改走 agent 回调 `getDeferredResults/getTaskRegistry`；双腿「已确认回档挂到流结束后执行」平价。 |
| D-20 | **Rust 缺陷（已修）**：用户收尾动作写入的 todo-state-v1（无源 Run）让后续 run 在模型上下文投影崩 `interactive UI entry lacks a source Run` | 服务契约 | 缺陷 | model_history 对 action∈{complete_all,cancel_remaining,dismiss} 的无 Run 条目按 UI 状态恒定排除（现役 lingxi.todo_state source:"user" 同口径）。 |
| D-21 | **Rust 缺口（已补）**：无 `/confirm` `/reject` 斜杠；`current_status`、`session_folders` 只有 HTTP 面无工具登记（模型经目录调用 unknown tool）；approvals 已处理回 200 | 服务契约 | 缺口 | slash.rs 两命令（现役逐字文本）；status_tools.rs 两工具 + 内核 ArgumentAwareSessionFolders 按 action 解析；已处理改回 404；confirm 后发 `confirmation-resolved` app 事件。 |
| D-22 | session_folders add 不存在目录：旧栈工具路径只 path.resolve 不校验（HTTP 路由会校验——旧栈两面不一致）；Rust 工具与路由同口径拒绝 | 服务契约（校验严格度） | ADJ-3 同族（Rust 更严格/诚实） | 不入 cases（叶 7C959E 断言外）；观测字段 `missingFolderAccepted/missingFolderEcho` 如实入观测。 |
| 回退锚点语义 | checkpoint 锚「最新用户轮」；rewind 按 retry 重置点语义丢弃该轮输入及其后全部（旧栈 retainedEntriesBeforeRetry 到父节点；Rust resolve_retry_reset_point 同口径） | 语义辨析 | 一致 | 线束断言改为「锚点轮及其后终结均消失、历史缩短」。 |
| tool_end 透传白名单 | 旧栈 WS tool_end.details 只透传 todos/todoVersion/sessionFile* 键（rewind:"scheduled" 不在线上），Rust 透传完整回执 | UI 绑定 | 形状差（不影响语义） | 「scheduled」改以终结回显文本（双内核）取证。 |

## R08 收尾第六轮（2026-10-26，最终）——差异了结与新证据

| # | 差异/发现 | 层 | 定性 | 处置（最终） |
|---|---|---|---|---|
| D-15 TERMINAL | Rust 无终端会话子系统 | 服务契约 | **已实现消除** | Rust 真实终端子系统：procsupervisor 只读 TerminalLog（独立于 write_stdin 游标）+ terminals.rs 三帧身份/所有权链 + close 经 supervisor.terminate；双内核真实 PTY A/B 14/14 case 真，三叶 CE6906/4C35E6/CFD9F0 绑定。旧传输面任务（帧族不断连/显式回复形状）保留 recorded 仅作历史回归。 |
| D-16 | 「删除助手后续接 404」 | 线束 | **翻案：线束缺陷** | 硬编码客户端 session id 与 Rust 服务端铸 id 不一致；改用返回 id + 活 Agent 400 负例后双腿真，叶 3818ADEAFA7A 绑定并撤 recorded。 |
| D-16b | 旧栈 abort 真实挂载点 | 测试口径 | 修正 | 原断言文案 `/chat/task/:taskId/abort` 是模块名；活体实测旧栈挂载 `POST /api/task/:taskId/abort`（空 body 400 internal_contract、未知任务 404、跨会话 403、拥有任务 200 aborted）。T01 绑定表该行 "unmounted?" 不准确，按本证据修正。 |
| 延迟大内容保留窗口 | 旧栈工具结果内核侧保留窗口 vs Rust 溢写完整输出 | 数据语义 | 如实差异（不影响语义） | 叶 ED0891 断言「按 contentId 取回真实大正文、非 240 字预览 + 他会话/未知 404」；两内核各自真实，不要求同字节集。 |
| B 腿工具面 | 此前 A/B 的 B 腿 --config 无 workspace，file/exec 四工具未注册 | 线束保真度 | 已修 | B 腿注入隔离 workspace（LINGXI_R08_AB_WORKSPACE），模型声明面恢复 read/write/edit/exec_command；延迟内容/终端叶因此可在两腿取真实机制。 |
| UI 5 叶客户端份额 | 聊天/地图/MCP 设置/安全设置/快捷窗口 | UI 绑定 | 收口 | 87 case 逐断言证据（组件级真实投影 + 既有专属单测 + 真 Electron live）；覆盖矩阵列明未覆盖子句（MCP 轮询超时、Web window.open、子组件错误态等）属不可构造/既有单测范围，不冒充。 |
| ask_user 卡面 | 旧栈 WS content_block+store / Rust interactions 待答面 | UI 绑定 | 各自真实形状 | 双腿各自真实 UI 绑定层断言；details 字段（values/labels/autoSelected/timedOut/answered/dismissed）逐项对拍。 |
| ask_user 边界 | A4「ConfirmStore 不可用」无公开活体开关 | 证据边界 | 如实披露 | 由两内核既有专属测试覆盖（tests/ask-user-tool.test.ts、rust .../r06_interactive_completion.rs），不作活体 AB 声称。 |
