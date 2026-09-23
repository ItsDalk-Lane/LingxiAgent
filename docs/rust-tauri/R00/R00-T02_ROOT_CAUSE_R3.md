# R00-T02 第三轮失败后的独立根因分析

**结论：R00-A03 的阻塞仍在行为语义层；本报告不判 R00-T02 通过。** 本次只读分析并新增本报告，未改功能清单、生成器、产品、测试或总控账本。依据是 `codex/rust-tauri-migration` 的 HEAD `16aeb380d58d68ff1a38bb46f5cc5d18f985f084`、R00-T02/R00-A03/R00-A04、03 功能矩阵、当前六份候选、R1—R3 独立审阅、`R00-T02_ROOT_CAUSE_R2.md` 和现役源码。R3 对 R00-A04 的静态分类 PASS 可作为复审输入；R00-A03 仍为 FAIL，不能因下文分析改变验收状态。

## 1. 根因：只封闭了“登记集合”，没有封闭“行为集合”

任务书要求每个现役叶子有稳定 F-ID、用户动作、可见结果、当前路径、目标负责人和适用验收。`ENTRYPOINT_COVERAGE.json` 的 832 项原始登记、四项空差集，以及 72 组同地址多方法的 65 `SPLIT` / 5 `SHARED_READ` / 2 `INTERNAL_STEP`，只证明**抽到的登记有清单归属**。`r00_t02_inventory.py:299-326` 的 `behavior_key()` 仍以 `route + path + HTTP 方法`作为普通路由行为身份；`:379-416` 对无 `ROUTE_EFFECTS` 的路由按方法生成“请求执行/修改/读取”和“实际响应待复核”；`:920-1100` 再把该文本直接写入 `acceptance_requirement`。同一方法内根据请求体、查询参数、调用者或已有状态选择不同处理的分支，对这一身份不可见。

当前 `FEATURE_INVENTORY.json` 明示 **382 个** `PENDING_SOURCE_REVIEW` 普通路由叶子：154 个 `READ`、151 个 `POST`、48 个 `PUT`、22 个 `DELETE`、7 个 `PATCH`。`ENTRYPOINT_COVERAGE.json` 的 34 项 `side_effect_reviews` 由 `r00_t02_inventory.py:1040` 附近的**路径字词**筛出，六个 R3 反例均未进入；`body_effect_decisions` 只有确认卡一项，`parameter_variant_decisions` 只有三项。R3 所述多方法效果的 124 项静态推断与这 382 项有重叠，不能相加当作独立工作量。当前自动负例会拦同 URL 新方法，却不会拦 `POST /desk/cron` 新增 `purge_all` 分支；R3 已在内存中实际证明 `build()` 对此仍返回成功。因此，只补六处具体文案或手列六个 `body.action`，下一次遇到空值、布尔值、作用域或 helper 内分流仍会漏。

**需要先规定“什么算同一行为”。** 建议以用户意图和成功后的可观察状态为核心，结合数据增删范围、是否可恢复、权限/凭证作用域、外部副作用和失败/冲突反馈作等价判断。同一行为可有别名、GET/HEAD、桌面桥、移动端等多个入口；一个路由也可支持多个行为。不同文件名、ID 或提供方式不必机械拆叶，但其影响范围和边界必须写进该叶的验收。无效参数、鉴权失败、幂等无变化通常是该行为的反向/边界断言；如果它改变授权范围、恢复能力或用户所选动作，就不能藏在泛称成功结果里。

## 2. 从源码反向得到的同方法高风险集合

下表是**必须审查的分支族和已定位样本**，不是宣称只存在这些分支。六项 R3-F01 是已证实需要拆分或明确不同结果的最低集合；其余是独立取证发现的同类候选，下一轮应按行为等价规则裁决，不先凭路径强制拆分。

| 分支族与源码 | 不同结果或审查重点 | 当前清单风险 |
|---|---|---|
| 显式动作枚举：`server/routes/desk.ts:1062-1259` 的 `/desk/cron`；`:1519-1672` 的 `/desk/files`；`server/routes/mobile-workbench.ts:88-117` 的 `/workbench/actions` | cron 应用建议/新增/删除/启停/修改；文件上传/创建/建目录/改名/移动/批量移动/回收；移动工作台建目录、写入、改名、移动、安全删除。`/desk/files` 的绝对路径上传还有本地主人专属的 403 边界。 | 各只给一个 POST 叶子；删除、启停、上传授权等被“请求执行”吞掉。兼容移动地址应共享相应动作身份。 |
| 显式处置与授权动作：`server/routes/sessions.ts:1206-1250` 的 `/sessions/authorized-folders`；`:2670-2740` 的 `/sessions/workspace-disposal`；`server/routes/confirm.ts:14-50` | `add/remove/set` 改变会话可访问目录；工作台会话 `archive` 可恢复、`delete` 永久删除归档文件及草稿；确认卡批准与拒绝相反。 | 前两者仍各一叶；确认卡已按两叶处理，可复用模型。授权增加/撤销需覆盖正反边界，工作台处置需分别断言恢复能力。 |
| 空值或可选字段改变副作用：`server/routes/desk.ts:1489-1513` 的 `/desk/jian`；`server/routes/agents.ts:379-435` 的 `DELETE /agents/:id`；`:636-710` 的 `PUT /agents/:id/config` | 便笺非空写入、空值删除；删除助手可选 `deleteSkills`，连带删除仍独占的用户技能，跳过项返回原因，界面在 `AgentDeleteOverlay.tsx:119-124,181-225` 明示两种选择；配置中的 `providers[name] === null` 删除供应商，非空保存供应商。 | 单一 DELETE/PUT 容易漏掉连带删除和 `null` 撤销的影响；助手删除本身与可选技能清理至少要有明确子结果和失败/跳过断言。 |
| 布尔开关与范围选择：`server/routes/sessions.ts:1014-1050` 的 `/sessions/pin`；`server/routes/channels.ts:711-719` 的 `/channels/toggle`；`server/routes/skills.ts:312-380` 的技能启停；`server/routes/git-environment.ts:417-430,462-490` 的提交/暂存 | `true/false` 导致置顶/取消、频道启动/停止、技能启用/禁用；Git `includeUnstaged` 与 `paths` 决定提交或暂存影响哪些改动。技能路由还按 workspace 来源写入不同状态名单。 | 可能仍用一个“执行/修改”叶子；可保留一个成对开关身份的前提是清单写清两个方向、数据状态和各自验收，而不能只写一个成功响应。 |
| 同方法选择不同资源/恢复后果：`server/routes/git-environment.ts:353-368` 的 `/git/unstash`；`server/routes/media.ts:120-138` 的 `GET /media/providers`；`server/routes/channels.ts:233-275` 的 `/conversations/:id/export` | Git 带 `path` 恢复单文件，不带则 `popStash` 弹出最新整条；媒体 `capability` 选择语音识别、视频、语音生成或图片供应商；导出按 `dm:` 身份与频道身份选择不同数据。 | 即使 `READ` 也可能有不同用户所见内容、数据范围或权限；应判断为独立子叶还是同叶的明确参数场景。 |
| 运行状态、版本冲突与无变化：`server/routes/sessions.ts:1852-1918` 的 `/sessions/todos/complete`；`server/routes/model-observability.ts:93-125` 的设置更新 | 待办仍有未完成项时写入完成记录，否则可能原样返回；版本不匹配返回 409。观测设置试图关闭固定启用项时明确返回 409，不写入设置。 | 这类分支通常是同一叶的边界断言，不宜误拆成成功功能；但验收必须证明拒绝/无变化时数据不被误改，不能只写“返回实际响应”。 |
| 复合设置、秘密与调用者边界：`server/routes/preferences.ts:187-245` 的 `/preferences/models`；`server/routes/access.ts:86-130` 的 `/access/network`；`server/routes/resource-io.ts:78-145` 与 `server/routes/mobile-workbench.ts:88-155` | 模型/搜索配置可分别更新，搜索密钥空值会移除保存值；网络 `mode` 决定仅本机或 LAN 监听；资源写入、回收与上传的权限取决于资源/设备身份，移动工作台写入经 `files.write`，读取经 `files.read`。 | 同一成功 HTTP 状态不能证明秘密未被错误保留、LAN 范围正确或远端设备没有越权；这些至少要有明确作用域和拒绝断言。 |

以上样本横跨 D02/D04/D06/D09/D10/D18/D19/D20/D21。尤其 `DELETE /agents/:id`、`GET /media/providers` 和 `/git/unstash` 不含 `body.action`，说明简单搜索 `action` 不构成穷尽。`server/routes/media.ts:30-67` 的 `/media/generate` 还把整个 body 交给媒体管理器；`/skills/install` 在 `server/routes/skills.ts:382-410` 可从本地绝对路径或上传包取得来源。此类委托路由要继续追入 service，并记录“同一安装/生成效果但来源或能力边界不同”的裁决，不能因为 handler 本体无 `switch` 而自动放行。读路由也要检查查询参数、主体身份和数据投影；382 项中的 154 个 `READ` 不能整批视为无风险。

## 3. 可执行的全量审查和修复方案

1. **冻结入口全集，另建行为审查账本。** 保留 R2 的 832 项登记、方法/地址/handler/调用者/源码行和现有别名、内部端点归属。独立从 `server/composition/{open-root,full-root}.ts`、`server/index.ts`、所有已挂载 `server/routes/*.ts` 的 Hono 注册抽一次集合，覆盖 `get/post/put/patch/delete/on/all`、命名 handler 与兼容地址；与 coverage 的 468 个原始 HTTP 登记做双向差集。DOM、IPC、slash、工具、调度的既有抽取继续保留。动态装配或 AST 无法解析的注册列 `MANUAL_REVIEW`，不凭“零差集”跳过。
2. **382 项逐项结案，并复审现有声明。** 按 `pending_semantics` 的每个 F-ID 建一行，最少记录：方法/地址及原始入口、请求体/查询/主体身份的分流字段和值、默认/空值、调用到的 helper、成功与拒绝路径、文件/数据库/远端/权限副作用、界面或 CLI/Bridge 消费者、可见结果、证据定位、`SPLIT`/`SAME_EFFECT_WITH_VARIANTS`/`INTERNAL_STEP`/`UNREACHABLE` 裁决及理由。没有分支也必须有“已追到最终读写/外部动作”的正证据。已标 `SOURCE_CHECKED` 的 34 项和 72 组方法裁决不能因旧标签免审，至少对其 helper、消费者与映射做交叉复核；R3 的 124 项静态方法效果不能留待实施期。
3. **用源码扫描生成待审分支，而非直接生成语义。** 解析 handler 及其本地/跨文件 helper，找 `req.json/formData/query/param/header`、身份来源、`switch/if/三元/空值/布尔/属性存在`，把可达分支追到 `engine/store/fs/网络/事件` 等结果；再从 renderer、移动端、CLI、Bridge 反向找调用参数和成功/错误显示。每个候选分支应有稳定的“源码分支签名 → 审查决定”记录。自动工具可标出新增/改变/未解释分支，不能自行决定两个副作用在用户眼里是否相同。对 helper 内分流、调用者传入的常量、事件监听和不可静态确定的动态调用，人工记录追踪结论；没有证据就保持 `PENDING`，生成不应宣称完成。
4. **行为身份与入口改为多对多。** 稳定键应是如“cron 删除任务”“工作台安全删除文件”“会话授权目录移除”之类的语义动作及其结果，而非 URL、HTTP 方法或行号；同一行为的 `/mobile/workbench` 兼容地址、多个 UI 和桥接入口共用键，单个 POST 可以指向多个键。F-ID 从稳定行为键生成，重命名或改路由不漂移；拆分保留旧→新多值映射和原因，原始登记保持可追溯。`duplicate_entry_owners` 应区分**共享能力/分流路由**与错误重复所有者，不能为维持“一入口一叶”再次压平动作。R2 的确认卡和预览 `supporting_entrypoints` 是可直接扩展的先例。
5. **验收从真实结果倒推。** 每叶给前置条件、用户动作、成功后的 UI/数据/外部状态、权限拒绝或冲突结果、适用平台，以及能实际检查这些结果的阶段任务/场景。逐个核 `acceptance_ids` 的 `given/when/then`；例如 cron 的 R03-A11（子代理权限）、R07-A01（重启不重复提醒）不能充当 CRUD 验收，`/desk/files` 的 R07-A05（频道私密内容）、R07-A13（监听撤权）不能证明文件创建/回收。可保留它们作为横向约束，同时在验收账本增加对应动作的明确子场景/断言；没有适用场景时标缺口，不用现存 ID 充数。`FEATURE_STAGE_ACCEPTANCE.json` 必须逐叶映射真实 oracle，R00-A03 再用入口→行为→结果→场景的双向差集复核。
6. **重生后独立复核。** 新候选绑定当前源码 SHA 与文件摘要；审阅者从注册点、UI 调用和持久化/外部效果反向抽查全部高风险集合，核对 382 项每行裁决、旧 ID 迁移和 A04 分类。针对行为变化运行相关 route/UI 测试；现有测试通过仅证明旧产品路径可用，不能代替清单语义。`PENDING_SOURCE_REVIEW` 应为零或逐项有“非生产/内部步骤”的可核实理由；不存在未解释的新增分支、无主用户动作或不适用的验收映射，才可重新提交 A03/A04 独立判定。

## 4. 新增动作必须能被门禁发现：反例与人工边界

保留 R1/R2 的 IPC、slash、别名、72 组多方法、Git 文案、预览归属、确认拒绝、内部端点和 MOOD 负例；再加入**不改登记地址的方法内反例**：在 cron `switch` 内加 `purge_all`，在便笺空值条件外加 `null` 特判，在 `DELETE /agents/:id` 加 `deleteAllSkills`，在 `/git/unstash` 加第三种范围，在 `/media/providers` 加新 `capability`，在委托的文件 service 新增 `safeDelete` 语义分支。每项都应使“源码分支签名有变化但无审查决定/行为映射/适用断言”非零失败；仅修改文案、只登记一个泛称 F-ID 不能通过。反向反例是删掉一个 UI 调用或改变远端设备权限后，旧叶仍称“用户可执行”也应被提醒复核。

这类门禁只能证明**有无未解释的代码变化**，不能自动证明 `archive` 与 `delete` 是否应合并、某项授权是否足够、错误提示是否清楚。人工审阅必须读最终 helper、副作用和 UI 消费者，给每个 `SAME_EFFECT_WITH_VARIANTS` 留可反驳的理由和至少一组正/反结果证据；安全/远端/不可逆分支需逐项审，不用抽样代表全体。源码位置存在、生成器自检通过或 Vitest 全绿，都不能把模板文案变成已核行为。

## 5. R2 修复的保留边界

可保留 R2/R3 已证实的 **IPC 86 invoke/19 事件、11 核心 slash/6 别名、兼容地址与内部端点归属、72 组多方法登记和结构拆分、已逐源码核实的部分方法效果、确认卡批准/拒绝两叶、文件预览独立叶与 R09-T05/A09/A10、Git push/pull/fetch 和工作区回退偏好的已核副作用描述、旧 F-ID 映射框架**。34 项已查路由的源码证据可以作为复核起点，不能把路径词筛出的集合当作高风险全集；72 组中尚待逐 handler 复核的 124 项方法效果也不能算作已核实。A04 的现役 subagent、Ollama、旧 research 兼容与硬排除分类可沿用作静态证据；`mobile-workbench` 的开放/闭集交付决定仍为 `DECISION_REQUIRED`，不能因此删去已挂载功能。此次分析未做四平台运行、真实供应商/远端推送或旧数据演练，也未给 A03/A04 新验收判定。
