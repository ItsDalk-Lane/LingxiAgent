# R00-T02 第二轮失败后的独立根因分析

**结论：R00-A03 仍为 FAIL；R00-T02 不能放行。** 本报告是只读调查，不修改功能清单、生成器、产品源码或总控账本，也不代替下一轮独立验收。调查基于 `codex/rust-tauri-migration` 的 HEAD `16aeb380d58d68ff1a38bb46f5cc5d18f985f084`、当前 R2 候选、两轮审阅和实际生产调用链。R00-A04 的现役/撤回静态分类在两轮审阅中均通过，未因此把 A03 改判通过。

## 一、跨两轮的共同根因

R1 发现的是**入口全集不完整**和**地址冒充功能**：生成器最初只从选中的 HTTP、页面、工具等注册点抽取，遗漏桌面 IPC 组合根及 slash 命令闭集；随后按地址机械生成叶子，把兼容别名和内部端点膨胀成用户功能。R2 修补了这两个抽取根，但仍有**身份和语义的反向错误**：同一地址的不同动作、同一桥接 API 支持的不同可见行为被压成一个叶子。两种错误方向相反，根源却相同：把“注册入口如何聚合”直接当作“用户行为是什么”，没有独立的行为目录来审定每个动作、结果、副作用及阶段验收。

当前 `r00_t02_inventory.py:258-271` 的 `behavior_key()` 对 HTTP 入口忽略方法，以 `route + path` 为身份；`:334-350` 的 `consolidate()` 对合并组只写一个动作/结果；`:304-332` 的 `route_semantics()` 通过 DELETE 优先级、路径末段及通用动词猜测结果。桌面侧 `IPC_GROUPS`/`IPC_EFFECTS`（`:114-180`）和 `UI_PROJECTION_OWNER`（`:181`）以共享频道或投影关系合并行为；`:793-858` 把该分组直接生成唯一 F-ID，且用功能域统一任务列表 `DOMAINS[domain][3]` 赋阶段和验收。这样一来，抽取、归并、文字说明和验收归属由同一套假设产生，内部校验无法独立否定这些假设。

`build()` 的四组差集（`:853-862`）只检查**已抽出的登记是否恰有一个清单所有者**、ID 是否重复；它没有检查“一个用户行为是否有独立叶子”“一个叶子是否混入相反副作用”“共享底层入口是否需要服务多个叶子”“任务/验收是否覆盖该行为”。`:770-774` 对 `ui-behavior:file-preview` 只查它是否在抽取集合内，不查它最终落在哪个 F-ID；`:879-885` 只校验任务和验收 ID 存在，不检验适用性。`:912-946` 的负例只覆盖新增 IPC、核心 slash、MCP 别名 handler 分叉，恰好验证了 R1 修复点；它们没有测试同地址新增不同效果的方法、投影错归、单方法副作用描述失真。末尾 `INTERNAL_ENDPOINTS_ATTACHED_AND_NOT_INDEPENDENT` 实际只检查三个内部登记出现在 `by_entry`（`:940-945`），未检查它们是否独立成为功能。源码定位检查也只校验文件存在且行号未越界，不校验该行仍支持所写动作和结果（`:881-885`）。

因此 `832` 个登记、`598` 个叶子、24 域非空与四组空差集，证明的只是**生成器自身选取和归并后的集合闭合**，不是 R00-T02 要求的逐叶真实行为覆盖。对比 R1 的 `700/566`，数量增长本身也不代表遗漏已穷尽。

另有一个保留风险：`mounted_route_files()`、HTTP/IPC/renderer 消费者抽取依赖所选根目录和特定文本形态（`:360-440`、`:495-584`），不是对所有可能注册语法的独立解析。R2 证明了当前静态写法下的 IPC/slash 集合相符；以后注册形式、动态装配或跨文件调用改变时，仍需从实际组合根反向核对，而不能只依靠现有正则的零差集。这里是同类漏检风险，并非声称当前又发现一项具体生产入口遗漏。

## 二、全部 finding 的状态与修复矩阵

| Finding | 当前状态与可复核证据 | 同类风险集合 | 下一修复必须做的事与可验证反例 |
|---|---|---|---|
| R1-F01 桌面入口漏账 | **原漏账已修，抽取层相对稳定；行为层仍需审。** R2 独立复算 preload 的 86 个 invoke、主进程 82 个 handler 加 updater 4 个、19 个 send/on 事件；新增未归属 IPC 负例能失败。`extract_desktop_host()` 在 `:385-439` 核对双端注册，并标出 12 个未发现 renderer 静态调用的桥。 | 同一 IPC 供预览/编辑等不同 UI 行为使用；47 个 `desktop_behavior` 中至少 15 个聚合了 3 个以上原始登记，聚合合理性不能由频道计数推断。未找到直接调用的 12 项应继续标成桥接能力，不能称为用户流程通过。 | 保留 preload ↔ main/updater ↔ renderer 的入口全集及新增未归属负例；为共享桥建立“入口可支持多个行为”的关联，并逐项审高风险桌面组，尤其文件、预览、浏览器、更新、快捷聊天、观测导出。不得为了一个入口一个所有者而吞并不同结果。 |
| R1-F02 核心 slash 被传输层遮盖 | **原漏账已修，静态闭集相对稳定。** `extract_slash_commands()`（`:442-464`）从真实注册循环取得 11 个主命令、6 个别名；新增主命令负例能失败，Bridge 和 Chat 共用 dispatcher。 | `ws-in:slash` 仍是传输入口，不是 11 个动作的替身。插件动态命令是协议边界，不可虚构为随包闭集；命令别名可归同一效果。 | 保留命令闭集及别名映射检查，并补删除/改名命令时清单失效的检查；逐命令核动作和结果，尤其 `/reset`、`/rc`、`/confirm`。 |
| R1-F03 地址冒充叶子、内部端点误算、结果模板 | **别名及内部归属部分已修；语义未修。** MCP `/servers` 与 `/connectors`、Agent `ishiki`/`agents-md`、移动工作台两套地址按同 handler 归并；健康、日志、OAuth 回调/轮询挂到所属能力。R2 的 R2-F01 表明按 URL 合并仍错。 | 全部 361 个 `route_behavior` 都由 `route_semantics()` 或少量 `ROUTE_OUTCOMES` 产生文字；其中 72 个含多种 HTTP 方法。单方法的 Git push/pull、导入、审批、权限、文件/数据清理同样可能被模板写错。别名校验只覆盖已手列的几组。 | 把原始 `(方法, 地址, handler, 调用者)` 与语义叶子分层；同 handler 且同效果的兼容地址可并，内部端点只作能力支撑。逐个核对有副作用 route 的真实 handler、消费者、持久化或远端效果，不能靠路径词典生成验收结论。负例：新增兼容地址但 handler 不同必须失败；把内部端点独立计叶必须失败。 |
| R1-F04 无 URL 的可见子功能 | **MOOD 实时/历史、五语言、拖放、通知、可见浏览器、快捷聊天、观测另存已补；整体部分修复。** `UI_PROJECTIONS`（`:467-479`）记录这些链；文件预览却经 `UI_PROJECTION_OWNER` 合入编辑。 | 所有仅凭页面/设置 tab 登记的用户操作，以及 6 个被 `UI_PROJECTION_OWNER` 归并的 UI 投影，都需审“是否同一结果”；24 域非空不足以保证矩阵逐项覆盖。 | 按任务书 03 的 D01—D24“必须保护行为”和旧 `FEATURE_MATRIX.md` F01—F20 建逐项勾稽表，穿透页面、事件流、历史投影、宿主桥和数据消费者。负例：删除一个必保行为投影、或把它挂到不相干叶子，门禁应失败。 |
| R2-F01 同 URL 多效果与副作用失真 | **未修，BLOCKING。** `server/routes/knowledge.ts:258-300` 的 GET/PATCH/DELETE `/knowledge/notebooks/:id` 分别查看、改名、删除，却共享一个“删除”F-ID；`server/routes/access.ts:198-225` 的 PUT/DELETE 密码分别设置、移除；MCP 连接器 PUT/DELETE 同理。`/git/push` 在 `server/git/git-command.ts:663-680` 真执行远端 push，界面在 `GitGraphPanel.tsx:243-255` 显示成功/失败；清单却写“修改 Git 推送、更新后可再次读取”。`/sessions/workspace-rollback` 的 GET/PUT 也被覆盖文案统一写成“确认回退”。 | 72 个多方法叶子是**待逐项裁决集合，不是 72 个已证实错误**；GET/HEAD 等可在证明同一读取效果后合并。优先排查 DELETE 与其他方法共存、GET 与写入共存、权限/凭证、知识导入、会话回退、远端 Git 操作。每个单方法高副作用路由也需查。 | 给不同效果分配稳定 F-ID；同效果方法合并须留理由。至少将笔记本查看/改名/删除、密码设置/移除、连接器修改/删除、工作区回退预览/执行分开；推送、拉取写明实际远端/本地副作用及用户提示。旧 ID → 新 ID 必须有可追溯映射。负例：同地址新加不同效果方法而未声明身份和验收时，生成应非零失败；改掉 handler 副作用但文字未改时应由人工证据或契约检查发现。 |
| R2-F02 文件预览被编辑吞并 | **未修，BLOCKING。** `ui-behavior:file-preview` 已在原始登记，却只作为 `desktop-behavior:file-edit` 的入口；同叶还有 `write-file*`、`run-edit-command`。实际 `file-preview.ts:110-150` 打开预览、`preview-file-content.ts:100-114` 读取快照；`PreviewEditor.tsx:635-636` 按版本写回，是另一动作。该叶只领 R04-T04/R06-T05，遗漏任务书 R09-T05 的桌面预览与不可信内容隔离，以及 R09-A09/A10。 | `UI_PROJECTION_OWNER` 的其他归并项需逐个核定；`IPC_GROUPS` 中 file-edit、detached-viewer、skill-viewer、browser 等多操作组也须区分操作步骤与独立用户结果。 | 给文件预览独立稳定 F-ID、当前调用链/数据/目标负责人、R09-T05 和适用的 R09-A09/A10；编辑继续单列。共享 `read-file-snapshot` 可同时为预览和编辑提供底层能力，清单关联需支持这一事实。负例：把预览改挂文件编辑或其他桌面叶子，即使原始登记仍存在也必须失败；删除 R09 映射必须失败。 |

## 三、只读反例复算与为何旧自检漏过

1. 运行现有 `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py --negative-checks`，退出 **0**；打印三项预期 `NEGATIVE_DETECTED`、`CHECKS_OK`，仍为 832 登记、24 域、四组空差集。这说明 R1 的新增入口、slash 和别名分叉检查有效，但没有覆盖 R2 的身份/语义问题。
2. 仅在 Python 进程内替换 `lines()`，给 `server/routes/knowledge.ts` 增加 `PUT /knowledge/notebooks/:id` 登记，不写任何仓库文件；`build()` 正常返回。新登记与原 GET 使用**同一个 F-ID**，叶子仍写“用户删除或撤销知识库笔记本”，四组差集继续全空。该探针不是声称生产中已有这条 PUT，而是证明“新动作被旧叶子吞掉”不会被当前门禁识别。
3. 仅在进程内将 `UI_PROJECTION_OWNER['file-preview']` 改指已有的 `desktop-behavior:detached-viewer`；`build()` 仍正常返回，文件预览被登记到 D23 的另一桌面叶子，四组差集全空。当前 `required_ui` 只检查原始投影存在，因此错归也能通过。
4. 无需造反例，真实候选已经同时展示 R2-F01/F02：笔记本三效果合一、Git push 文字失真、文件预览挂在编辑 F-ID。现有测试通过只说明旧程序行为存在，不能证明清单对行为的命名和阶段验收正确。

## 四、下一轮修复顺序与复审门槛

1. **先定行为身份，再改生成。** 为每个叶子写明稳定行为键、用户触发条件、成功/失败可见结果、数据或外部副作用、当前 handler/消费者/存储、目标负责人、阶段任务及验收。原始注册表独立保存；别名和协议步骤可多对一，不同效果必须一对多拆开，底层共享 IPC 允许多叶引用。F-ID 应绑定行为身份，不随兼容地址或实现方法任意漂移；旧 F-ID 映射解释拆分与归并。
2. **审完整风险集。** 从 `server/routes/*.ts` 与 `server/index.ts` 独立列出多方法同地址的 72 组并逐项判定；另外抽查有远端/删除/授权/导入/回退副作用的单方法 route。复核 `desktop/preload.cjs`、`desktop/main.cjs`、`desktop/auto-updater.cjs` 的全部已登记频道及其 renderer 消费，逐个审多入口桌面组和 6 个 UI 投影归并。以 D01—D24 的明确可见行为而非域是否非空为终点，再与旧 F01—F20 对账。发现抽取器之外的新组合根须进入入口全集及缺失负例。
3. **验收映射按叶子写，不从域一刀切继承。** 特别复核 D09/D23 的预览、Git 远端操作、权限/凭证和知识库动作。每个 leaf 的任务和验收应能说明将验证什么结果；不适用的组合须明确理由。R09-T05/A09/A10 必须进入文件预览的映射。
4. **把反例加到门禁。** 除既有三项外，至少覆盖：同地址新增异效果方法；GET/PATCH/DELETE 笔记本错合；PUT/DELETE 密码错合；单 POST push 被写成可再读取；文件预览错归编辑或任意其他叶子；删除预览 R09 归属；内部 health/log 被独立计作用户功能；必保无 URL 行为消失。每个反例应在缺少人工语义声明或错映射时非零失败。自动检查无法判断所有真实副作用，需有逐项源码/消费者审阅记录，不用自动生成文案冒充审阅。
5. **重新生成并独立复核。** 重生 `FEATURE_INVENTORY.json`、`FEATURE_STAGE_ACCEPTANCE.json`、`ENTRYPOINT_COVERAGE.json`，核对旧→新 F-ID、入口→叶子和叶子→验收双向关系；新候选绑定文件摘要与当前源码 SHA。独立审阅者应从生产注册、UI 消费和持久化/外部效果反向抽样，不只运行同一个生成器；重点检查上述反例及 72 组裁决记录。随后运行受影响的 route、预览/编辑、桌面宿主、slash/Bridge、MOOD/i18n 针对测试。测试成功不代替清单语义复核；只有 A03/A04 均独立通过才可把 R00-T02 交付为完成。

**源码必查边界：**入口与身份包括 `server/composition/open-root.ts`、`server/composition/full-root.ts`、`server/index.ts`、`server/routes/*.ts`、`desktop/preload.cjs`、`desktop/main.cjs`、`desktop/auto-updater.cjs`、`core/slash-commands/{index,bridge-commands}.ts`、`lib/bridge/bridge-manager.ts` 和 `core/plugin-manager.ts`；用户动作与结果至少追到 `server/routes/knowledge.ts`、`server/routes/access.ts`、`server/routes/mcp.ts`、`server/routes/sessions.ts`、`server/routes/git-environment.ts`、`server/git/git-command.ts`、`desktop/src/react/components/runtime/GitGraphPanel.tsx`、`desktop/src/react/utils/{git-env-api,file-preview,preview-file-content}.ts`、`desktop/src/react/components/PreviewEditor.tsx`。无 URL 行为还须核 `server/routes/chat.ts`、`desktop/src/react/{hooks/use-stream-buffer.ts,utils/message-parser.ts,settings/tabs/InterfaceTab.tsx}` 及预览/宿主消费者。最后用任务书 03、R00-T02/R00-A03/A04 和 R09-T05/A09/A10 校验逐叶任务归属；只看生成器文件或清单中的行号不构成生产链证据。

## 范围与保留事项

本报告只分析 R00-T02，未实际修复清单或源码，未运行四平台、真实供应商、真实远端推送或旧数据迁移。两轮报告对 A04 的静态分类结论可保留作下一轮复审输入；`mobile-workbench` 的开放/闭集交付归属继续为 `DECISION_REQUIRED`，其已挂载功能不可因此删除。用户已有的旧任务书删除、新任务书和总控账本修改均未触碰；未提交或推送。
