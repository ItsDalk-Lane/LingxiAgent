# R00-T02 第六轮独立对抗性验收

**VERDICT: FAIL。R00-A03 FAIL；R00-A04 PASS（仅当前源码的静态现役/撤回分类）。** 本轮只读核验冻结候选与现役调用链，只新增本报告；不改候选、产品、测试、任务书和总控账本，不提交或推送。736 个迁移子场景仍为 `SPECIFIED_NOT_EXECUTED`，交 R00-T07 纳入正式验收账本及后续实施任务执行；未执行它们并非本轮失败理由。

## 候选身份与独立复算

- 分支 `codex/rust-tauri-migration`，Task Base/HEAD 均为 `16aeb380d58d68ff1a38bb46f5cc5d18f985f084`。交接所列 17 份非报告文件按文件名排序，以每行 `文件SHA256␠␠文件名\n` 聚合复算为 `390e4db5e83ca511e339158415d3d0dfd2e1af3788d42b488c89bff8044f63e1`；`R00-T02_REPORT.md` 与 `R00-T02_R6_REPAIR.md` 单独 SHA-256 分别为 `5d255bc659839ab33ee9bcf81371868157efbe1bff49c90415d7bbfc62063f24`、`e6b51a627c22073e4540d78e97a1ea88ca331bbf399bf6e281b7c5dc54887bdb`，均与交接相同。
- 已读本地 `AGENTS.md`、新任务书 01/03/05/R00 的 R00-T02/A03/A04/T07、R1—R5 独立审阅、R2—R5 根因、当前报告/修复说明及 17 份候选。以现役源文件和消费者反向核对，而非将生成器的自检当作独立语义证明。
- 独立用 JSON 双向索引复算：832 条登记均有叶归属，736 个唯一保留叶（491 个 `route_behavior`、245 个其他种类）各有唯一子场景，叶和场景回链无悬空；24 域非空，四项结构差集为空。26 个 UI 页/面板有 161 条动作、281 条控件/效果变体，矩阵动作均标 `MAPPED`；另有 5 项 UI 动态边界、统一审查中 51 项外部或动态 `MANUAL`。72 组多方法判定、原 G1/G2/G3 的 128/127/127 项和原非 HTTP 工具/UI/核心 111/82/47 项、5 项文件操作拆分保持。991 条旧 F-ID 映射的新 ID 均存在；独立文件预览仍接 R09-T05/A09/A10；`/model-observability/health` 有独立 F-ID `F-D22-ROUTE_BEHAVIOR-BEHAVIOR-MODEL-OBSERVABILITY-MODEL-OBSERVABILITY-204546` 与场景 `R00-T02-LA-2045462C0D98`，旧 settings 映射可追到新叶。这些是结构和点名范围的正证据，不能替代下面的实际分支与结果核查。

## 阻断发现

### R6-F01 — BLOCKING：等价的新 `skipped` 分支仍能用同步摘要绕过完整构建

`lib/tools/ask-user-tool.ts:275-303` 从 `ConfirmStore` 的 `decision?.action` 分流。现行 HTTP `server/routes/confirm.ts:18-20` 只允许 `confirmed/rejected`；定时器和会话中止另产生 `timeout/aborted`。`lib/confirm-store.ts:97-103` 的 `resolve` 本身不限制 action 值，因此新增源码分支不能只凭现行 HTTP 枚举自动裁为不可达：若新增上游动作，应逐项判断来源、用户结果和场景。本反例没有改上游路由，**不声称当前界面已能发送 `skipped`**；它证明新增工具分支候选未被发现，也就没有机会做人工可达性裁决。

我在 Python 进程内、没有写仓库文件，将现有 `if (action === "confirmed")` 前插入 `if (["skipped"].includes(action)) return toolError("skipped");`。该条件对 action 为 `skipped` 的结果实际进入新返回路径，含义与候选自检用的 `action === "skipped"` 相同。随后仅同步 `R00-T02_NONHTTP_AUDIT_TOOLS.json` 中该逐叶行的 `reviewed_source_sha256`，以及 `R00-T02_SOURCE_BRANCH_AUDIT.json` 中同一入口的源码 SHA 和 `audit_record_sha256`；人工契约、`branch_variants`、成功/拒绝结果、736 个场景和断言全部保持原样。对这一内存候选调用**完整 `build()` 意外成功，返回 736 叶**，没有报告新 `skipped`。对照把同一分支写成 `action === "skipped"`，同步相同两层摘要后，完整 `build()` 则报 `ask_user 源码分支与人工语义契约差集: 新增=['skipped']`。

原因在 `r00_t02_source_gates.py:139-146`：独立分支抽取只认 `action === '字面值'` 和 `switch case`，不认 `['字面值'].includes(action)` 等同样可执行的选择；`validate_ask_user_contract()` 只能比较它已抽出的集合。旧人工契约与旧场景仍可原封不动地“证明”遗漏后的候选。R5-F01 的关键条件“新源码分支即使摘要全部同步仍需进入人工裁决”尚未闭合。应从 action 的值和可执行条件反向发现此类分流，无法静态判明时显式列 `MANUAL`，再由人裁决可达性、行为身份、结果和场景。

### R6-F02 — BLOCKING：供应商页的失败场景断言与现役页面相反

`R00-T02_UI_ACTION_MATRIX.json` 的 `ui:settings:providers#01`，及其回写的 `F-D10-UI-UI-SETTINGS-PROVIDERS-D50161` 子场景，断言“自动加载供应商摘要和配置”失败时“**载入失败应有错误状态，不可当未配置**”。实际 `desktop/src/react/settings/SettingsContent.tsx:392-396` 在 ready 后调用 `void loadProvidersSummary().catch(() => {})`，摘要失败被静默吞掉；`desktop/src/react/settings/actions.ts:63-65` 只有成功时更新 `providersSummary`；`desktop/src/react/settings/tabs/ProvidersTab.tsx:23-31,54-96` 只读取摘要和配置渲染列表，没有读取、设置或显示摘要加载错误状态。该页还明确不在挂载时重试摘要（`ProvidersTab.tsx:42-43`，相应现役测试 `ProvidersTab.test.tsx:497-509`）。这不是一个可执行的现役错误 oracle；若用该候选验迁移版，保持现有静默行为的版本会被错判为失败。

同页 `#02` 将“选择已有供应商、挑选临时草稿、真正新增配置”合成一条动作，只指向 `provider-global-save` 叶。`ProvidersTab.tsx:72-74,86-90,120-125` 的选择/挑选只改前端选中状态与临时草稿，不调用保存；持久配置由后续详情操作完成。该支持边不能单独证明“选择后详情与临时草稿可见”，即使保存叶存在且调用链中也有 `config.ts` 源码交点。应拆开本地选择与真正保存的结果，或给同动作变体各自的检查条件。

### R6-F03 — BLOCKING：自动化新增草稿被写成“空提示词阻止”，而生产明确允许

`R00-T02_UI_ACTION_MATRIX.json` 的 `ui:panel:automation#02`，及 `F-D18-UI-UI-PANEL-AUTOMATION-FA0FF4` 的逐项子场景，把“新增手动任务”的空态/边界写成“**保存空提示词阻止**”，错误结果写成“**HTTP 失败可能未单独 catch，需错误场景**”。现役 `desktop/src/react/components/AutomationPanel.tsx:135-185` 在有助手时发 `action:'add', prompt:'', enabled:false`；HTTP 非成功由 `res.ok` 分支显示错误 toast，网络异常也在 `catch` 显示 toast。服务端 `server/routes/desk.ts:1132-1148` 仅在**启用**且为 Agent 会话时要求非空提示词，停用草稿允许空提示词。现役 `tests/desk-route-cron.test.ts:740-783` 精确断言该请求返回 200，保存 `prompt:""`、`enabled:false`。`AutomationCard.tsx:119-127` 只在后来尝试**启用**空提示词任务时阻止。这是两个不同动作阶段；当前候选把后者的限制错写到新增动作，并把已实现的错误提示写成待查。它会把保持原行为的迁移结果误判失败。

另一个较小但同向的检查点：用量矩阵 `ui:settings:usage#03-05` 与回写场景反复称“八卡”，而 `ObservabilityMetrics.tsx:44-47,75-80,99-170` 的空态和有数据状态均渲染 **10** 张指标卡，洞察卡占其中三张。现役结果数量仍须据组件而非候选措辞修正；不以此单独计阻断，但它说明 UI 正反场景的人工 oracle 还未逐项对到最终显示。

## 前轮问题与 A03/A04 逐项裁决

| 项目 | R6 裁决 | 可复核依据与界限 |
|---|---|---|
| R5-F01 分支发现 | **FAIL，R6-F01 阻断** | 严格相等的 `skipped` 负例已能失败；同义 `includes` 分支在双层摘要同步后完整 `build()` 通过。当前 HTTP 不提供 `skipped`，人工可达性应标明，不能静默遗漏候选。 |
| R5-F02 最终消费者范围 | **PASS（点名的静态变动检查）** | 实际链为 `SettingsContent → UsageTab → ModelObservabilitySection → ObservabilityUsagePanel → model-observability-actions → server/routes/model-observability.ts`。面板 `:102-124` 的 `setAggregate(result)` 经 `:150-154` 及 `ObservabilityMetrics.tsx:67-80` 显示；查询失败为 `role="alert"`。独立在内存把赋值改 `setAggregate(null)`，以及单独把 `role="alert"` 改 `role="status"`，两次完整 `build()` 均非零，精确报用量页必要消费者签名变化。矩阵还给出模型、技能、自动化等其他多动作页的链；静态源码签名不证明其文字 oracle 正确，见 F02/F03。 |
| R5-F03 页面多对多目标与结果 | **FAIL，R6-F02/F03 阻断** | 用量、模型、技能、自动化等旧单目标错链已扩为逐动作目标；把用量页同步摘要后错挂到仍存在的单一导出叶，完整 `build()` 报支持关系差集。但供应商错误显示、供应商本地选择、自动化新增/启用界限与现役消费者相反；非空目标、源码交点和 161 条动作数量仍不能代替准确场景。 |
| R00-A03 真实入口均有归属 | **FAIL** | 登记→叶→场景结构双向闭合，但新增工具分支可绕过审查，多个现役 UI 动作的正反结果错误。A03 要求可核的真实行为归属与适用验收，不能按结构差集放行。 |
| R00-A04 撤回与现役不混淆 | **PASS（静态分类）** | `core/provider-registry.ts:436,480` 的 Ollama 保留；`core/agent.ts:936-938,1228-1230` 的三个现役 subagent 工具保留，`lib/experiments/registry.ts:122-145` 主动委派默认关闭；`server/routes/knowledge.ts` 无独立 research 启动注册，旧研究表/事件只作兼容；`core/engine.ts:3779-3799` 只载入随包插件。`EXCLUSIONS.md` 把强制子代理目录、独立研究引擎、本地模型管理列为撤回，把外装 MCP 实例与内置协议分开。真实供应商、平台和旧用户数据未执行。 |
| R00-T02 整项 | **FAIL** | A03/A04 须均实质通过；A03 仍有阻断。不得据此把候选或总账标为完成。 |

## 检查与未执行范围

- 默认 `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py --negative-checks` 退出码 0，输出 `CHECKS_OK`、832 登记、四项空差集；已有严格相等 `skipped`、`setAggregate(null)` 和错挂导出叶的负例均按预期失败。上述 `includes` 反例和独立 `role="alert"` 变动另在本轮内存中调用完整 `build()`，没有写回文件；前者**意外通过**，后者按预期失败。生成器自检通过不能消除前者。
- 针对现役代码运行 `npx vitest run`：`ask-user-tool.test.ts`、观测界面 `model-observability-vertical.test.tsx`、`AutomationCard.test.tsx` 共 **3 文件、14 项通过**；`desk-route-cron.test.ts` **24 项通过**；`ProvidersTab.test.tsx` **12 项通过**。合计 **5 文件、50 项通过**。其中 cron 路由测试直接验证空提示词停用草稿可创建；其他测试验证当前实现能运行，不证明候选所有 161 条动作语义正确。
- 五项 UI 动态边界及统一审查 51 项 `MANUAL` 对外部供应商、Bridge、运行时 MCP、payload 本地权限等给出调用点、人工结论或接收 Task；本轮不把它们算为真实运行 PASS。没有执行四平台安装包、真实供应商/远端操作、旧数据迁移或未来 736 个 Rust/Tauri 子场景。
- 审前已有的旧任务书删除、新任务书未跟踪与总控账本修改均原样保留。本轮无产品、候选、测试及总账改动。

**复审门槛：**修复分支发现对等价条件的漏检，新增分支无论同步多少摘要都须进入可达性与结果/场景的人工裁决；按现役代码逐项修正供应商、自动化和用量页的目标与成功、空态、失败断言，再重生冻结候选并由未参与修复者复审。A04 的静态 PASS 可保留作为输入，不能替代 A03。
