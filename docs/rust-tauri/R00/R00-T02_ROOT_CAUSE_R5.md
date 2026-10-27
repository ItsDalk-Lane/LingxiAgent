# R00-T02 第五轮失败后的独立根因分析

**结论：R5-F01/F02/F03 是同一种“候选自己证明候选”的三个表现；R00-A03 仍被阻断，本报告不判 R00-T02 PASS。** 分析对象是 `codex/rust-tauri-migration` 的 HEAD `16aeb380d58d68ff1a38bb46f5cc5d18f985f084`、R5 冻结候选及现役源码。已核对本地 `AGENTS.md`、新任务书 R00-T02/A03/A04/T07、01/03/05、R1—R5 独立报告、R2/R3/R4 根因、生成器、735 叶清单、三份原非 HTTP 审查、拆分审查和统一分支审查。本次只新增本分析，不修改候选、产品、测试、任务书或总控账本，不提交、推送。R5 对 A04 的**当前源码静态分类 PASS**仍可作为下轮输入；它不消除 A03 的失败。

## 一、三项失败的共同根因与精确边界

R1—R3 逐步补上真实入口、同地址异效果、382 个普通路由的语义；R4 补出 HTTP 490 叶及逐叶场景，指出余下 240 个非 `route_behavior` F-ID 缺同等级审查。R5 候选将这 240 项逐一登记并增拆 5 个文件操作效果叶，形成 **832 个生产登记、735 个保留叶（HTTP 490、非 HTTP 245）、735 个未执行子场景**。结构差集为零、三份原非 HTTP 审查 111/82/47 行、统一审查 832 行，是值得保留的账本成果。它们仍不能推出每个可达用户动作、最终显示和失败结果都正确。

共同错误不在某一个摘要算法，而在**三个集合都由被验收候选自行定义**：

1. **分支集合**：逐叶报告写 `branch_variants`，统一报告写 `branch_selectors`；`r00_t02_inventory.py:450-500,506-559` 只校验行、字段、引用和源码/审查行摘要，未从真实可达分流独立列出“应有分支”。因此 R5-F01 在 `ask_user` 增加 `skipped` 后，只更新逐叶源码摘要和统一行的源码/记录摘要，旧裁决与 735 个断言不变，完整 `build()` 仍通过。源码摘要只说明“这份记录看过某版文件”，不能说明记录理解了该文件。现有“只改摘要”自检只同步第一层，没覆盖两层都同步的反例。
2. **源码范围**：统一行的 `source_scope` 取本行、登记及逐叶报告所自报的文件并比较 SHA（生成器 `:486-500`）；它没有从页面组合根独立推出必须追到的子组件、hook、请求函数、状态写入、最终渲染和错误分支。R5-F02 中 `ui:settings:usage` 只冻结 `SettingsNav.tsx`、`SettingsContent.tsx`、`UsageTab.tsx`。实际 `UsageTab.tsx:11-15` 挂 `ModelObservabilitySection`，再由 `ModelObservabilitySection.tsx:200-245` 挂用量、设置、台账、轨迹、详情和导出消费者；所有 832 行都没把 `ObservabilityUsagePanel.tsx` 或 `use-observability-query-state.ts` 纳入范围。内存中把 `ObservabilityUsagePanel.tsx:113` 的 `setAggregate(result)` 换成 `setAggregate(null)`，摘要完全不动，`build()` 仍通过，证明漏的是最终可见结果链，而非外围文件。
3. **支持关系与验收结果**：`validate_nonhttp_review()` 对 `SUPPORT` 只要求 `supported_feature_ids` 非空、目标存在且非自身；`:1389` 再把该列表机械换成 `supported_scenario_ids`。没有从 UI 实际点击、筛选、保存、错误显示反向证明每条支持边。R5-F03 的 `ui:settings:usage` 唯一目标是观测**导出文件**叶，实际页面主要还查询聚合、筛选、显示图表、台账/轨迹和修改设置。`ui:settings:models` 与 `ui:settings:providers` 各只指 `provider:anthropic`，但前者有辅助模型与四类媒体默认模型设置，后者按已配置供应商列多行并提供配置入口。非空支持边及同域、同 Task 都不能证明这些动作归属正确。

这解释了为何 R5 的两个源码变动反例与一个真实错链可以并存：**摘要防止未登记修改，内部差集防止自选集合漏行，场景计数防止空场景；三者都没有独立的生产行为预期。** 修复需有独立来源的分支发现、消费者闭包和“可见动作→实际效果叶→适用场景”三道校验；单纯扩大全文件 SHA 或再补一个任意支持 ID 只会换一种自证。

## 二、可复核证据与修复矩阵

| 阻断项与现成反例 | 为什么现有门禁放行 | 最小完整修复及 R6 应见的证据 |
|---|---|---|
| **R5-F01** `lib/tools/ask-user-tool.ts:275-303` 从 `decision?.action` 分流。R5 在 `confirmed` 前新增可达 `skipped → toolError`，同步逐叶及统一两层摘要、记录摘要，保持 `confirmed/timeout/dismissed/aborted` 裁决及场景不变；完整构建仍返回 735 叶。 | `review_record_sha256()` 是候选审查行的摘要，不是独立期望；统一行比较的是同一行的再摘要。`branch_selectors` 的非空文字与源码分支未逐值比较。 | 另用独立解析器从调用结果的 `action`、`if/switch/三元/提前 return`、所调用 helper 及权限分流抽出**可达条件和值**，与人工维护、不能由本轮报告自动生成的分支语义契约双向比较。每个新增路径须裁决 `SPLIT/SAME_EFFECT_WITH_VARIANTS/INTERNAL_STEP/UNREACHABLE`、说明效果/拒绝、挂 F-ID 与场景。只同步全部摘要及记录摘要但不改契约和断言时必须非零，精确报 `tool:ask_user` 的 `skipped`。若判它不可达，须给调用方约束与反证；不能凭分支名判。 |
| **R5-F02** `UsageTab → ModelObservabilitySection → ObservabilityUsagePanel`；查询成功 `setAggregate(result)` 后由 `ObservabilityMetrics` 等显示（面板 `:102-164`）。内存改成 `setAggregate(null)`、其余不动，构建仍通过。 | 生成器只要求 `source_ref`、审查自报文件是摘要集合子集，不分析 JSX 子树、hook、异步状态和渲染依赖；容器文件 SHA 可以始终不变。 | 从生产页面组合根独立建立消费链，至少覆盖导航/页面、JSX 子组件、props 回调、自定义 hook、store/actions/API、服务端 handler、成功数据到状态再到显示，以及 loading/empty/error/forbidden。以链中**必要文件/分支**反向校验 `source_scope` 和结果证据；不在范围的最终消费者变化必须非零。该 `setAggregate(null)` 反例即使页面三文件摘要未变也应被发现。 |
| **R5-F03** `ui:settings:usage` 只挂 `observability-export`；`ui:settings:models/providers` 只挂 Anthropic。用量审查称“无记录或请求失败显示空态”。 | 目标只验证“存在且非自己”，场景由支持 ID 自动转换；`given/when/then` 与断言只是审查报告的复制，不对照页面真实分支。 | 按每个**可见控件及自动加载**列动作表，接到实际请求/IPC/状态、真实效果 F-ID 和检查该效果的场景；页面可一对多支持，但每条边有调用链证据。用量至少对接聚合、调用列表、轨迹、详情/设置与导出中实际可达者；模型页对接辅助槽与媒体默认值，供应商页对接实际 provider 配置/选择路径。分开断言空数据、未初始化、查询失败、无权限及成功投影。错挂一个仍存在的叶应非零。 |

对于 F01，独立解析器只负责**发现分支候选并拒绝遗漏**，不自动判断两个分支是否同一用户行为。可使用 TypeScript AST 加控制流/调用图：从注册入口追参数、判定值、默认及空值、异常与提前返回；跨文件 helper 继续追，直到文件/数据库/远端副作用或 UI/工具结果。静态无法确定的反射、动态属性、插件回调、事件订阅或运行时注册应输出 `MANUAL` 条目（调用点、可能目标、理由、人工核查者与结论），不得悄悄视为“无新分支”。契约应由独立审阅者对照源码及可观察结果维护，并记录何处证明可达、何处证明同效；源码变化只使旧结论过期。即使有人同时改了解析器产物、契约和场景，自动工具也不能替代未参与修复者的语义复核，不应再宣称“SHA 已同步即 SOURCE_CHECKED”。

对于 F02，源码范围要取**保守闭包**而非只列打开页签的三文件：从 `AppPages.tsx`、`SettingsContent.tsx` 的真实路由/页签选择进入组件；逐层展开静态 JSX、`lazy/import()`、props 回调、hook、store selector/action、`lingxiFetch`/`window.platform`/WS；同时从响应对象及错误类型反向找赋值、缓存、转换、展示位置。文件越界或目标不确定时列 `MANUAL` 并给可复核调用点；不能用模糊 `consumer_helper: 对应数据加载` 冒充链。静态闭包不是把所有依赖机械列成叶子：样式、通用按钮等可作为影响显示的依赖记录，但独立行为由用户动作和结果裁决。负例应覆盖**删除或更改最终赋值/渲染/错误提示**，而不仅是修改入口文件。

对于 F03，支持关系应当用两张独立索引双向核：**页面控件/自动加载→调用点→请求或桥→真实效果叶→正反场景**，以及**叶/场景→所有声称支持它的实际入口**。同一个容器有多个动作就列多个支持边；若容器打开本身有需要保护的可见结果，可保留自己的 UI 场景，但它不能替代页内读写。仅用某个叶作为 `semantic_identity` 的“代表”会遮蔽其他动作。场景必须能观察数据/权限/外部状态和 UI 投影，不只写“入口可见”。

## 三、同类高风险页面与工具：已证实和待复核分开

本次独立复算 `R00-T02_NONHTTP_AUDIT_UI.json`：**26/26 个 `kind=ui` 都为 `SUPPORT`，26/26 个都恰好只有一个 `supported_feature_id`**；其中 19 行只冻结 3 个文件、7 行只冻结 2 个文件。这个统一形状本身不是 26 项均错的证明，却说明每个多动作页面都值得从真实消费者重审。下表“已见错链”是能用源码反驳现行唯一目标的项目；“优先复核”只指出同类风险，未在本报告替它们逐叶结案。

| 集合 | 具体源码与目前支持目标 | 裁决重点 |
|---|---|---|
| **已见错链：用量页** | `UsageTab.tsx:11-15`、`ModelObservabilitySection.tsx:95-113,142-151,200-245`、`ObservabilityUsagePanel.tsx:102-164`；唯一目标为 `desktop-behavior:observability-export`。清单里已存在 `semantic-effect:model-observability.model_observability_query_aggregate.post`、查询 calls/traces、settings 等叶。 | 查询成功应把聚合结果显示到指标/图表；正常零数据、`not_initialized`、聚合查询失败、bootstrap 禁止/网络错误分别验证。源码 `ModelObservabilitySection.tsx:142-150` 与面板 `:150-154` 在失败时显示 `role="alert"`；只有 `not_initialized` 特例走空指标。洞察分组查询失败在面板 `:140-142` 保留旧事实，不宜与 overall 失败混写。现行“请求失败显示空态”直接相反。 |
| **已见错链：模型与供应商页** | `ModelsTab.tsx:5-22` 挂 `AuxiliaryModelsSection` 与 `MediaGlobalDefaultsSection`；前者 `:149-186` 可选/保存/测试多个辅助槽，后者 `:90-115` 保存图片、视频、语音生成与转录默认值。`ProvidersTab.tsx:82-117,194-258` 按 provider 列详情，并有添加及搜索配置。两页各只支持 `provider:anthropic`。 | 对接各实际配置、选择、测试和媒体默认值叶/场景；`provider:anthropic` 只可作为其中适用的一种供应商变体，不能代表整页。供应商列表空、缺凭据、保存失败不能凭泛称“不可调用”代验。 |
| **已见单目标遗漏：技能与自动化** | `ui:settings:skills` 只支持 `desktop-behavior:skill-viewer`，但 `SkillsTab.tsx:155-225,522-608` 有安装、删除、组合管理和按助手启停；清单已有安装、删除、组合、启停等效果叶。`ui:panel:automation` 只支持 `desk_cron.read`，但 `AutomationPanel.tsx:108-121` 发 `toggle/remove`，清单已有 `cron.add/remove/toggle/update` 等叶。 | 每个可见动作与其状态变化、失败提示分别建支持边；一个查看叶无法验证删除、调度或启停。现有源码证明这两页有未被唯一目标表达的动作，尚须逐项对完整子组件闭包。 |
| **优先复核：权限/宿主/设置页** | `ui:settings:agent` 唯一挂 beautify 开关，但 `AgentTab.tsx:227-448` 有助手卡、模型、记忆、工具等；`ui:settings:security` 唯一挂 session permission 读取，但 `SecurityTab.tsx:154-278` 有沙箱、备份、代理设置；`ui:settings:sharing` 唯一挂 service connectivity，但 `SharingTab.tsx:79-149` 有截图样式、宽度、字体配置。`ui:settings:general/about/bridge` 亦各只有一个目标，源码中分别有多种开关、更新列车或平台配置。 | 先确认各控件生产可达，再按实际写入、系统状态、服务连接或外部动作拆支持边；检查拒绝、部分生效、权限与平台边界。这里只列审查优先级，不声称所有控件现已无叶。 |
| **优先复核：其他页面/面板** | `ui:page:channels` 唯一挂 `channels.read`，而清单已有频道创建、删除、成员、消息和已读等叶；`ui:panel:activity/bridge/skills`、`ui:settings:access/browser/envdeps/experiments/interface/keybindings/mcp/me/work` 等也都是单一目标。 | 用 `AppPages.tsx`/设置组合根及各子组件反向枚举可见动作；静态页可保留一个支持边，多动作页逐项补。不要以候选已有某 HTTP 叶推断前端一定暴露它，也不要以本表例子代替 26 项全审。 |
| **工具和协议分支风险** | 三份非 HTTP 报告中工具 111 行、核心 47 行；`tool:ask_user` 是已证实的双摘要绕过。`ws-in:prompt/interject/steer/abort/resume_stream` 等 11 个 WS 行的源码范围目前仅 `server/routes/chat.ts`。 | 对 `ask_user`、确认与权限、文件读写/预览、插件媒体/封面、CLI 恢复、Bridge 外发、调度重启、供应商凭证、更新列车优先跑“新增分支但摘要全同步”负例；WS 还须沿 dispatcher 到会话状态及前端反馈，不能因同文件处理全部消息就自动认定结果闭合。这是风险集合，非已证明所有行错误。 |

## 四、R6 修复与独立复审门槛

1. **保留现有可复用成果并重审语义。** 保留 832 登记与可追溯旧 ID、HTTP 490 叶和 382 条 G1/G2/G3 普通路由审查、72 组多方法裁决、确认批准/拒绝、IPC 86 invoke/19 事件、11 slash/6 别名、文件预览独立归 R09-T05/A09/A10、文件编辑五项新拆分、A04 静态排除边界。R5 对 `ask_user` 既有确认/超时/拒答、beautify、tenet、供应商边界等抽查可作为证据起点；这些不使新增 `skipped` 或错挂 UI 自动通过。`mobile-workbench` 已挂载，开放/闭集责任仍为 `DECISION_REQUIRED`，不能为消除缺口删除它。
2. **建立与候选分离的分支/消费预期。** 入口全集从真实组合根抽取；独立解析器生成可达条件、调用边与最终消费者候选，人工语义契约逐项决定同效、拆分、内部或不可达。全部 832 登记必须有可解释的分支/消费结论；不可静态解析者显示 `MANUAL`、定位、影响、审阅结论与后续复核人，不能默认 `HIGH`。变更后旧源码 SHA 失效只是提示；要校验解析器发现的新增值确有新裁决和结果/场景，且消费者闭包与审查 `source_scope` 双向一致。
3. **26 个 UI 入口逐一重建支持矩阵。** 对每个可见动作/自动加载记录触发控件、输入、调用目标、实际效果叶、成功读回、UI 显示及失败/无权限/空态。支持边不设“恰好一个”约束；独立的 UI 可见结果仍有自己的子场景。`supported_scenario_ids` 必须对应**实际目标叶的具体断言**，不是只由现存 ID 集合自动得到。对 `usage/models/providers/skills/automation` 先消除上述已见错链，再对剩余页面全量逐行裁决；全量是 26 个入口，不是只抽两个设置页。
4. **三种隔离负例必须在同步全套摘要后非零失败。** A：给 `ask_user` 添 `skipped`，更新逐叶、统一及聚合摘要，但旧分支契约/结果/场景不变；报未审新动作。B：删去或改写 `ObservabilityUsagePanel.tsx:113` 的成功赋值，或改普通查询错误为错误空态；即使页签根未变仍报必要消费者变化/结果不符。C：把 `ui:settings:usage` 仍挂存在的导出叶、`models` 仍挂 Anthropic，却删掉实际查询/媒体设置支持边；报可见动作无目标或场景。正向对照是在真实分支、消费者、支持边及正反断言均补齐、人工裁决后通过。另保留 R4/R5 已有效的确认 `deferred`、Git 静态路由 `purge`、slash `/reject` 改批准负例；不能用它们替代 A/B/C。
5. **由未参与修复者复核冻结候选。** 新候选绑定实际 HEAD 和全部文件摘要，复核者独立从页面/CLI/Bridge/工具入口走到最终存储、外发或显示，审 `SAME_EFFECT_WITH_VARIANTS`、`SUPPORT` 与 `MANUAL` 理由，核入口→叶→场景及反向关系。当前产品针对测试只说明现役路径可运行；未来 735 项（若拆叶后数量变化，以新候选为准）仍为 `SPECIFIED_NOT_EXECUTED`，由 R00-T07 正式纳账并在实施 Task 验收前执行。不能用本阶段未运行未来迁移场景作失败理由，也不能用现役测试全绿代替清单语义正确。

若独立分支发现仍可被同步摘要绕过、最终消费者仍不入范围、26 页中任一生产可见动作无真实目标/适用场景，或失败断言与源码相反，**R00-A03 继续 FAIL**。A04 目前仅是现役/撤回静态分类通过，不代表四平台、真实供应商、远端操作或旧数据迁移已验。本报告没有执行这些外部验证，也不改变两项验收或总控账本的既有状态。
