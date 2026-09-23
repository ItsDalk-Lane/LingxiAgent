# R00-T02 第四轮失败后的独立根因分析

**结论：R00-A03 仍被 R4-F01/F02 阻断；本报告不判 PASS。** 分析基于 `codex/rust-tauri-migration` 的 HEAD `16aeb380d58d68ff1a38bb46f5cc5d18f985f084`、R00-T02/A03/A04/T07、03 功能矩阵、R1—R4 审阅、R2/R3 根因、现行九份候选及生产源码。本次只新增本报告，不修改候选、生成器、产品、测试或总控账本，不提交或推送。R4 对 A04 的**静态分类 PASS**仍可作为后续复审输入，不能代替 A03。以下“240 个非 HTTP 叶”沿用 R4 称呼；其中 `body_effect` 两叶实际源自 HTTP 确认 POST，只是 `kind` 不叫 `route_behavior`。

## 一、为什么审完 382 个 HTTP 入口仍漏 240 叶

R1 修注册全集（桌面 IPC、核心 slash），R2 修同 URL 异方法和文件预览，R3 修 382 个普通 HTTP 入口的方法内语义，形成 **490 个 HTTP 行为叶**。R4 独立复算 832 项原始登记、730 个保留叶、24 域、490 个逐叶子场景及结构差集，肯定这些成果。缺口在**审查边界被写成 `kind == "route_behavior"`**：`r00_t02_inventory.py:1119` 只为该类读取语义审查，`:1173-1220` 只给该类写 `acceptance_assertions`、子场景和旧 A-ID 适用性，`:1358-1375` 仅从已有 `supplemental_acceptance_id` 输出并与 `route_leaves` 比数。其余保留叶在 `:1093-1116` 由登记资料、`IPC_EFFECTS` 或静态标题直接生成，`leaf_tasks()` 在 `:513-529` 默认按功能域赋任务，再把这些任务的 A-ID 全挂上；`acceptance_requirement` 只是用户动作和结果两段文字拼接。`shared/tool-categories.ts`、`SettingsNav.tsx`、preload 或供应商注册行只能证明**可登记**，不能证明最终处理、用户可见结果或旧 A-ID 真会检查该结果。

现行 `FEATURE_INVENTORY.json.features` 减去 `FEATURE_STAGE_ACCEPTANCE.json.supplemental_scenarios.feature_id`，恰为下表 **240** 叶。不能从这个数字推出“都要新增 240 个独立测试”：页面、协议帧、目录项可能只是行为的支持入口；但每个保留 F-ID 必须有源码支持的身份裁决和适用验收关系。若裁决为支持入口，要把它关联到被支持的语义叶及场景，不能保留一个假装独立、却没有结果检查的叶。

## 二、17 类完整分组与修复矩阵

下表列全当前清单的 **17 种 `kind`**：`route_behavior` 490 叶已获 R4 对点名范围的审查认可；余下 **16 种合计 240 叶**是本轮缺口。表内“接收”是建议核对的现有实施 Task；逐叶场景先由 **R00-T07** 纳入正式账本，阶段实施前执行。示例不是替代同类全量审查；**240 个 F-ID 都须逐项结案**。

| kind / 数量 | 身份裁决及生产链必须追到哪里 | 结果、边界与建议接收 |
|---|---|---|
| `route_behavior` **490** | 已从 382 个原待核普通路由和 46 个单独声明路由形成行为叶，含同方法拆分；72 组多方法裁决和 G1/G2/G3 证据保留。 | 490/490 已有 `SPECIFIED_NOT_EXECUTED` 子场景及实施任务，交 R00-T07 正式化；本轮补救不得回退其效果、限制与旧 ID 映射。确认 POST 的两项特例在 `body_effect`，不在此数内。 |
| `tool` **59** | 多数是独立可调用能力，不能以 `shared/tool-categories.ts` 的名称行结束。沿 `core/agent.ts` / `core/engine.ts` 的注册与权限解析追到 `lib/tools/*`、`lib/sandbox/*`、目录桥、存储/外部操作和会话流；命令别名或共享低层执行器可归同一效果。 | 对文件读写/交付验实际文件和权限（R04-T04、R06-T05）；终端/代码执行验进程、取消、工作目录（R04-T05）；记忆/知识验 Agent 或来源隔离（R06-T06/T07）；子代理验父子状态和权限交集（R03-T03/T06、R06-T03）；`ask_user` 的卡片、回答映射、推荐超时、暂不回答分别验（R03-T01、R06-T04、R08-T07）。工具目录四项应分别验描述/搜索/调用契约（R04-T01/T02），不能都写“结果进入会话”。 |
| `builtin_plugin_tool` **13** | 13 个随包工具是可调用动作与说明类动作的混合。逐个从 `plugins/*/tools/*.ts` 追到实现、权限、文件/媒体产物、错误返回和消费者；同一套工具的不同参数若改变副作用也须裁决。 | Beautify 的 `create-cover` 只将**已有**图片应用到 Markdown，不能记作图片生成；无效目标或来源不改文件（`plugins/beautify/tools/create-cover.ts:6-90`）。媒体图片/视频/语音分别验产物和供应商失败；Office 阅读/PDF 验输入输出和隔离；指南/能力列表验真实可见目录。接 R04-T07、R07-T05，并给各工具实际结果子场景。 |
| `desktop_behavior` **47** | 从 `desktop/preload.cjs` → `desktop/main.cjs`/`desktop/auto-updater.cjs` → renderer 调用、系统状态或文件追证。它是 IPC 分组，不等于 47 个同粒度用户动作；如 `file-edit` 混合读快照、写文件、版本冲突，应明确读是预览支持步骤、写是编辑结果。无静态 renderer 调用的桥须标能力入口，不声称按钮可用。 | 按实际结果判同一动作可并（例如检查与设置可构成一项双向开关，须各验读写），不同后果须拆。浏览器可见/急停接 R01-T04、R07-T04、R09-T05；文件预览已独立且保留 R09-T05/A09/A10，编辑/回收接 R04-T04、R06-T05；系统权限/截图接 R09-T04；窗口、快捷键、通知、语言相关宿主接 R09-T03/T07；观测导出接 R05-T07、R07-T10；更新/公告接 R10-T07、R11-T04。验 OS 状态、文件、窗口/事件和拒绝结果。 |
| `provider` **39** | `core/provider-registry.ts` 的 `BUILTIN_PLUGINS` 是供应商目录，不是 39 种用户动作。沿每个 `lib/providers/*`、配置/凭证和 `server/routes/providers.ts` 到模型选择及真实调用，判共享的“配置、选择、调用模型”行为和供应商特有的认证、模型、能力边界。不得把目录压成供应商都已跑通。 | 可归为能力叶的供应商变体矩阵，逐供应商保留可发现/可配置/受支持能力和失败边界，特别保留现役 Ollama（A04）。接 R05-T01 及真实模型调用所属 R05/R07/R08；需要无凭证/无模型/能力不支持的明确结果。 |
| `ui` **26** | `AppPages.tsx` / `SettingsNav.tsx` 的页面或面板通常是导航容器，不是内部 CRUD 的替身。从页签追到实际组件动作、请求、状态和显示；与 HTTP、IPC、tool 叶建立支持关系。 | 页面打开/空态/错误本身若是要保护的可见行为可留叶；否则改作各动作的入口证据，不能用“看到页面内容”代替频道、自动化、设置或观测操作。接表中既有 R03/R06/R07/R09/R10/R11 的对应页面 Task，并逐一映射实际功能场景。 |
| `builtin_plugin_adapter` **2** | 即梦 CLI 的 image/video 两项从 `plugins/jimeng-cli/index.ts:51-52` 追到适配器、本机 `dreamina` 命令、授权和媒体产物；它们是不同媒体结果，不能只当插件装载步骤。 | 分别验图片/视频文件、缺命令或认证失败、参数能力；接 R04-T07、R07-T05，并关联媒体能力任务。 |
| `ws_in` **12** | `server/routes/chat.ts` 的入站消息类型是协议帧；`ws-in:slash` 只承载 11 个命令，不能另充全部命令。逐帧追 dispatcher、Run/会话状态与前端事件；`prompt`/`interject`/`steer`/`resume_stream`、`abort`、终端控制可对应不同用户意图。 | 能与 HTTP/CLI/slash 共用行为身份的帧列为支持入口；独立动作验队列、取消、恢复、流重放或终端关闭及拒绝边界。接 R03-T01、R06-T04、R08-T07；子代理停止接 R03-T03/T06；终端快照/尾随/关闭接 R04-T05；用量帧接 R05-T07、R07-T10。 |
| `cli` **11** | `cli/entry.ts` 的 chat/continue/sessions/status/help/serve/pull/diagnose/checkpoints/bundle-status/data-restore 各有不同命令结果。追参数解析、是否启动服务、实际读写/恢复、终端输出和退出码；`bundle`、`data` 父命令只是分发入口，已附在子命令。 | `help/status` 可为只读命令场景；`pull`、`data-restore` 须查真实文件/数据影响和失败后状态；chat/continue 与已有会话行为可共享语义叶但 CLI 输出另设平台断言。接 R02-T03、R07-T09；恢复若跨数据兼容再接 R10 的适用任务，不能由“终端显示结果”代验。 |
| `slash_command` **11** | `core/slash-commands/index.ts` 注册、`bridge-commands.ts` 执行，Chat/Bridge 共用 dispatcher；6 个别名只指向相应主命令，`ws-in:slash` 是传输步骤。11 项 `/stop /new /reset /rc /exitrc /apply /confirm /reject /compact /fresh-compact /loop` 各有可区分结果。 | `/reset` 的历史清除、`/rc` 的接管、`/reject` 的待批动作不执行必须逐项验；同确认卡等价时可共用行为叶，但 Bridge 来源、权限和反馈仍需专门断言。接各现有 R03/R04/R06/R07/R08 任务；旧 A-ID 须逐条对照，不用域级编号覆盖命令结果。 |
| `bridge_adapter` **5** | Telegram、飞书、钉钉、QQ、微信是五个平台适配器，不一定五种语义动作；沿 `lib/bridge/bridge-manager.ts` 的 `ADAPTER_REGISTRY` 到各平台收发、会话绑定、附件、重投和鉴权。 | 共享“发消息/收回复”可用平台矩阵；平台特有登录、附件、格式、接管或错误要独立边界。接 R07-T02，验真实平台或明确替身范围、会话去重与附件，不以注册表有五项宣称五平台已运行。 |
| `ui_behavior` **5** | 五项都指无独立 URL 的可见流程：文件预览与刷新、五语言、MOOD 实时/历史、本地服务诊断。追事件产生、renderer 投影、历史/设置存储和重开；预览的只读读取与编辑写回已分开。 | 文件预览保留 R09-T05/A09/A10；MOOD 直播/历史一致性接 R06-T01/T06；语言保存/重启读取接 R09-T03/T07；服务断线/重连反馈接 R02-T03、R07-T09。每项需可看见的正反结果，不能只验组件存在。 |
| `scheduler` **3** | `hub/scheduler.ts` 的 cron、每日 fresh-compact、heartbeat 是后台启动边界，通常不是用户点击动作；须追配置来源、触发器、实际 Run/消息/记录、停止与重启去重。 | 可把“配置自动任务”和“后台触发”关联为同一能力的两段场景，不能以 HTTP CRUD 场景代替触发结果；三项按时区/DST、重启、重复、取消验。接 R03-T06、R07-T01。 |
| `plugin_toggle` **2** | beautify、office 在静态工具目录中是启用开关，不是插件工具调用；追开关设置、`availability`、实际目录及禁用后的调用阻断。 | 开/关方向、会话/Agent 范围和已发起调用的撤权分别验；可成对保留一叶，但必须双向断言。接 R04-T07、R07-T05，与 13 个工具的执行场景区分。 |
| `update` **2** | `github-release-check.cjs` 的 GitHub Releases 检查与 `desktop/main.cjs` 更新列车控制可能是已有桌面更新叶的实现入口，先与 `release-check-latest`、`train-update`、Windows 自动更新对账，不能机械双计或漏包。 | 检查结果、下载/应用、签名拒绝、回退及公告状态需有各自 oracle；按同一效果合并入口并保留 OS 差异。接 R10-T07、R11-T04。 |
| `body_effect` **2** | `server/routes/confirm.ts:14-54` 的同一 `POST /confirm/:confirmId` 按 `action=confirmed/rejected` 产生相反决定，是特殊 HTTP 行为叶；追 `ConfirmStore.resolve`、权限 `authorizeConfirmation`、流事件、卡片及待执行动作。 | 批准/拒绝分别验状态、只执行获批动作、拒绝后不运行、重复处理 404、越权 403；`/confirm`、`/reject` 若共用决策结果可共用语义身份，但渠道反馈/授权另验。接 R04-T03/T06；现有 R04-A05/A06 仅检特定权限边界，不直接覆盖主动拒绝。 |
| `experiment` **1** | `subagent.proactive_delegation` 是现役、默认关闭的实验配置；追 `lib/experiments/registry.ts` → 设置页 → `core/agent.ts` 新会话提示/行为，不能归为撤回或默认启用。 | 开关前后新会话的委派提示及权限/现有会话边界；接 R03-T03/T06、R06-T03，保留 A04 静态分类。 |

**计数复核：**其余 16 类为 59+13+47+39+26+2+12+11+11+5+5+3+2+2+2+1 = **240**；加 `route_behavior` 490 = **730**。这解释了“17 类 kind”和“240 个非 `route_behavior` 叶”两个口径。`body_effect` 虽在后者，实际来自 HTTP 确认接口。

### 必须用真实调用链纠正的代表项

- `ask_user` 当前叶 `F-D01-TOOL-TOOL-ASK-USER-94CB74` 的源头只写 `shared/tool-categories.ts:46` 和“工具结果或产物进入会话”；`lib/tools/ask-user-tool.ts:194-270` 实际创建 `ask_user` 确认卡并发 `session_confirmation`，等待 `ConfirmStore`，分别处理作答、超时推荐和暂不回答；`core/agent.ts:726-733` 接入 Agent。`tests/ask-user-tool.test.ts:134-224` 是现役区分证据。旧 R03/R06/R08 A-ID 横向约束不能代替这些结果。
- `beautify_create-cover` 的 `plugins/beautify/tools/create-cover.ts:6-90` 明确只使用已生成文件，验证 Markdown 目标后调用 `applyMarkdownCoverFromGeneratedFile`；其失败返回文本，未调用图片生成服务。当前 F-ID 的“执行结果或产物”以及所挂 R07-A09（PDF）、R07-A10（危险归档）都不是此动作的验收。
- 确认卡拒绝 `F-D04-BODY_EFFECT-CONFIRM-REJECTED-F97BFB` 与 Bridge `/reject` `F-D04-SLASH_COMMAND-SLASH-COMMAND-REJECT-CD1524` 可能共享“拒绝待处理动作”的语义身份，但不应把 HTTP 卡片、slash 的来源/授权和流反馈抹平。`server/routes/confirm.ts:18-54`、`core/slash-commands/bridge-commands.ts:157-164`、卡片消费者须分别查；R04-A05/A06 的越权边界不操作主动拒绝。
- `desktop_behavior:file-edit` 的 `IPC_GROUPS` 同时收 `read-file-snapshot`、`write-file*`、`run-edit-command`，是“分组可能再次压平不同结果”的现成信号。R2 已拆出的 `ui-behavior:file-preview` 及其 R09 归属必须保留，同时复核编辑读写/冲突和独立派生预览是否应保留不同叶。

## 三、R4-F02：确认 POST 为什么绕过分支签名

`validate_semantic_reviews()` 在 `r00_t02_inventory.py:430-468` 只收 `kind=route`、非 `INTERNAL_ROUTE_OWNER`、且 `entry_id != CONFIRM_ROUTE`、同时不在 `ROUTE_EFFECTS` 的普通路由；因此 382 条普通入口持有 handler 与相关文件摘要，确认 POST、46 个静态声明的路由和内部协议路由不在这组同等级别的语义审查中。确认在 `:1150-1173` 另走手写 `CONFIRM_EFFECTS`：门禁只看 `confirm.ts` 是否仍含字面 `['confirmed', 'rejected'].includes(action)`，然后固定产两叶。R4 在内存中把 `action=deferred` 分支插在原校验前、保留该字面时，`build()` 仍得到 730 叶和批准/拒绝两叶且无错误；同样改普通 cron/helper/media 源码则被摘要拦截。可见“六个负例会失败”只证明**已绑定的普通子集**，没有证明所有特殊与普通 handler 都受约束。`set(CONFIRM_EFFECTS)` 只核人工字典未少项，不会从真实 handler 发现第三项。

修复应建**一个覆盖所有生产入口的复审门禁**，而不是再给确认加一个字面检查：

1. 从已挂载路由、特殊路由、内部协议、IPC、WS、CLI、slash、工具、调度、插件和供应商注册取得入口全集；每个入口声明 `entry_id → handler/dispatcher → 可达 helper/消费者 → 语义分支 → F-ID 或支持入口 → 适用场景`。`ROUTE_EFFECTS`、确认、`INTERNAL_ROUTE_OWNER` 都进同一审查协议；不因“已手写动作”跳过。对于动态调用，列人工审查边界，不虚构自动能解析。
2. 为每个被审 handler 冻结内容摘要，并绑定分流使用的 helper、权限判定、持久化/外部效果和消费者摘要；新增或变动后使旧审查决定失效。签名只负责**发现要复审**，不能代替审查结论。每个可达分支须有 `SPLIT` / `SAME_EFFECT_WITH_VARIANTS` / `INTERNAL_STEP` / `UNREACHABLE` 裁决、正反结果、场景。确认的 `deferred` 若是新的用户决定，必须有新语义叶；若只是内部状态，也要有可核理由和所属场景。仅更新摘要或增一个泛称叶不能放行。
3. 让结构校验遍历**全部保留 F-ID**，而非只比 `route_leaves`：独立行为有可观察结果与对应子场景；支持入口关联被支持的行为及场景；页面/Provider 可作为能力变体矩阵但需明确检查点。对现有旧 A-ID 逐条标 `DIRECT/INDIRECT/GAP`，`DIRECT` 需证明其 given/when/then 真的检查该叶；若仅域级约束则新增子场景，交 R00-T07 接收和指定实施 Task 执行。
4. 生成器对“新增入口”“同入口新增分支”“旧行为消费者消失/权限变化”“语义叶无场景”“场景无现役叶”“只改审查摘要而无裁决/结果”分别非零失败。审查记录需与源码版本绑定，不能只重算文件摘要后自称 `SOURCE_CHECKED`。人工复核仍要沿最终状态读回、文件/数据库/系统/远端副作用及用户所见错误；自动扫描不能裁决两个结果是否等价。

**负例设计（均仅在内存或隔离副本运行）：**在确认 handler 的原校验前新增 `deferred`，保持原 `includes` 不变，应报确认语义分支未审；在 `ROUTE_EFFECTS` 静态路由新增 `body.mode=purge`、在内部 OAuth/日志 handler 新增外部副作用，应报未审，而非被例外集合跳过；在普通 cron、便笺、Git unstash、媒体 capability、跨文件文件 helper 新增分流，继续非零失败；在 `ask_user` 增加第四种选择结果、插件 `create-cover` 增加生成图片分支、IPC 文件写入增加覆盖旧版本分支、slash `/reject` 改成批准，均须触发非 HTTP/特殊入口审查。另删除 `ask_user` 的逐叶场景或把它错挂 R07-A09、把 `ui-behavior:file-preview` 归回编辑、给 Provider 目录项宣称“真实调用已通过”，映射/人工复审必须拒绝。正向对照是在审查、结果、边界及适用场景全部补齐后才通过门禁，不以既有产品测试全绿替代。

## 四、R5 修复顺序与独立复审门槛

1. **冻结现有成果再补 240 叶。** 保留 832 登记、HTTP 490 叶与各自子场景、382 条 G1/G2/G3 证据、72 组多方法裁决、旧 F-ID 映射、IPC/slash/别名和内部端点归属、预览独立叶及 R09、A04 的现役/撤回分类。逐个非 HTTP F-ID 给出入口、调用链、分流/默认/空值、真正效果、成功/拒绝或无变化、数据/权限范围、用户看到的结果、证据位置、行为身份裁决与后续 Task/场景；不因其类别是页面/Provider 就机械造独立用户动作。
2. **先重审高风险，再封闭全量。** 权限/确认/工具、文件与系统 IPC、CLI 恢复、Bridge 外发、插件文件/媒体、调度重启、供应商凭证及更新列车逐项核，剩余静态目录/页面也逐项裁定“独立行为或支持入口”，不能用抽样代替 240 个身份决定。对每个 F-ID 建一行可审账本，最终 240/240 有裁决、无泛称“看到结果/产物”作为唯一 oracle；必要拆并保留旧→新多值映射。
3. **统一门禁及负例后重生候选。** 当前普通 382 的摘要与测试应保留，确认及其他例外同等级纳入；静态声明 46 条亦需分支/下游变化提醒。重生清单时核入口→行为→场景及反向差集，不允许“结构零差集但语义无主”；检查 490 已有 HTTP 子场景未回退，旧 A-ID 不冒充叶子直接验收，A04 分类不被顺手改写。
4. **R5 独立取证。** 新候选绑定 HEAD 和文件 SHA。审阅者从 UI/CLI/Bridge 调用、handler、helper、最终持久化或外部动作反向抽查，重点复核表内 17 类与所有高风险 F-ID，逐条验证 `SAME_EFFECT_WITH_VARIANTS`/支持入口的理由、确认 `deferred` 反例及普通/特殊 handler 统一门禁。针对变更跑相关产品测试；本地现役测试只能证明当前实现，未来迁移后的场景仍为 `SPECIFIED_NOT_EXECUTED`，由 R00-T07 和阶段 Task 正式执行。若 240 行、统一门禁、逐叶场景或 A04 静态边界有一项未闭合，R00-A03/整个 R00-T02 仍不得判 PASS。

本报告未运行四平台安装包、真实供应商、真实 Bridge、远端 Git 或旧用户数据迁移，亦未执行未来 Rust/Tauri 的验收场景。`mobile-workbench` 当前已挂载、继续保留；开放/闭集最终归属 `DECISION_REQUIRED` 不在本报告擅定。
