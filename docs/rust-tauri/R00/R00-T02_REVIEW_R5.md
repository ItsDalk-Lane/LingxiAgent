# R00-T02 第五轮独立对抗性验收

**VERDICT: FAIL。R00-A03 FAIL；R00-A04 PASS（当前源码的静态分类）。** 本轮只审阅冻结候选和生产调用链，只新增此报告；没有修改候选、产品、测试、任务书或总控账本，没有提交、推送。未来 735 项迁移子场景在此阶段无需执行，仍为 `SPECIFIED_NOT_EXECUTED`，须交 R00-T07 正式纳账并由对应实施 Task 执行。

## 候选与审阅范围

- 分支 `codex/rust-tauri-migration`，Task Base/HEAD 均为 `16aeb380d58d68ff1a38bb46f5cc5d18f985f084`。按候选报告所列 13 份非报告文件，以文件名排序、逐行 `文件SHA256␠␠文件名\n` 复算，聚合 SHA-256 为 `3816c7193e87ffbac33cb536e6315790e70bedf42079c0e9512c8bdc4d34a6c2`；`R00-T02_REPORT.md` 自身 SHA-256 为 `20067a035922ecd610b2c973b66b285a65c1e26939086712b1095ad38dc27f1f`，均与交接一致。
- 已读本地 `AGENTS.md`、新任务书 01/03/05/R00 的 T02/A03/A04/T07、R1—R4 独立报告、R2/R3/R4 根因、当前报告和所列 13 份候选。从实际页面、工具、IPC、路由、helper 与最终可见结果反向核对，重点检查支持关系和统一源码分支门禁。
- 独立复算候选结构：832 生产登记、735 叶（HTTP 490、非 HTTP 245）、24 域；原非 HTTP 三份逐叶报告分别 111/82/47，另有 5 个文件操作拆分叶，形成 245 个唯一 F-ID 且与清单无重漏。382 个普通路由的 G1/G2/G3 为 128/127/127，72 组多方法裁决在覆盖表中；四项结构差集均为空。735 个唯一子场景与 735 叶一一对应，旧 A-ID 关系为 `INDIRECT` 1079、`GAP` 2107、`DIRECT` 0。上述是账本结构事实，不能单独证明语义正确。

## 阻断发现

### R5-F01 — BLOCKING：只同步两层摘要，就能让新增工具动作沿用旧裁决

生产 `lib/tools/ask-user-tool.ts:275-279` 从 `ConfirmStore` 结果分流；`tool:ask_user` 审查行列有 `confirmed`、推荐项超时、`dismissed`、`aborted`，其子场景断言分别核答案、自动推荐及空答案。生成器 `r00_t02_inventory.py:450-500` 核对的是登记、F-ID/场景 ID、逐叶审查行的 SHA-256、源码文件 SHA-256，以及非空的 `reason/positive/boundary`。它没有把实际新增的分支选择，与审查行 `branch_selectors`、裁决、成功/拒绝结果和场景断言作不可自证的比较。`validate_nonhttp_review()` 同样只检查这些字段是否存在、格式是否正确以及报告自带的源码摘要是否匹配。

我只在 Python 进程里，将原 `if (action === "confirmed")` 前插入可达的 `if (action === "skipped") return toolError("skipped")`；随后**仅**把 `R00-T02_NONHTTP_AUDIT_TOOLS.json` 中该行的 `reviewed_source_sha256["lib/tools/ask-user-tool.ts"]`，及 `R00-T02_SOURCE_BRANCH_AUDIT.json` 中 `tool:ask_user` 行的同一源码摘要与 `audit_record_sha256` 改成相应新值。`branch_selectors`、`decision`、`positive`、`boundary`、逐叶 `success_result/refusal_or_boundary`、735 个子场景及其断言全部保持原样。对这个内存候选调用完整 `build()` **仍通过，返回 735 叶**；新增 `skipped` 没有行为身份、最终结果或验收断言。仓库文件未变。现有自检的“只改摘要”负例只改第一层逐叶摘要、没有同步统一行的摘要，因此失败；它没有覆盖这次可复核的两层摘要反例。

这并不否认现行冻结候选能检出**未同步摘要**的源码改动；它否定“只改摘要不能蒙混”以及“832/832 行均有真实语义分支审查”的结论。修复须让新增或改变的可达动作必须产生新的逐分支裁决及结果/拒绝/场景审查证据；在只更新所有相关摘要而保持旧语义文字的隔离反例中仍须非零失败。摘要本身只能提示复审，不能充当复审决定。

### R5-F02 — BLOCKING：实际可见的模型观测消费者不在统一源码范围

真实入口 `desktop/src/react/settings/tabs/UsageTab.tsx:11-15` 渲染 `ModelObservabilitySection`；后者 `:200-202` 渲染 `ObservabilityUsagePanel`。该面板 `ObservabilityUsagePanel.tsx:102-124` 请求聚合数据并调用 `setAggregate(result)`，`:146-164` 将指标和图表显示给用户。原始登记 `ui:settings:usage` 的统一审查行却只冻结 `SettingsNav.tsx`、`SettingsContent.tsx` 和 `UsageTab.tsx`；全部 832 行的 `source_scope` 都不含 `ObservabilityUsagePanel.tsx`、`use-observability-query-state.ts`。它是所宣称正向结果的最终消费者，不是外围无关文件。

我仅在 Python 进程把 `ObservabilityUsagePanel.tsx:113` 的 `setAggregate(result)` 改为 `setAggregate(null)`，其余代码、报告及全部摘要不变，调用完整 `build()` **仍通过，返回 832 登记、735 叶**。这个变动会使成功查询的指标卡读不到聚合结果，却不触发统一审查。修复须从页面组合根追到实际请求、状态计算、可见指标与错误呈现，把这些必要消费者及其分支纳入相应审查行；对上述隔离反例应非零失败，并逐项审查其他只冻结容器文件的支持入口。

### R5-F03 — BLOCKING：两个设置页的支持目标与真实动作不相符，且观测失败断言相反

`R00-T02_NONHTTP_AUDIT_UI.json`、`FEATURE_INVENTORY.json` 与 `FEATURE_STAGE_ACCEPTANCE.json` 把 `ui:settings:usage`（`F-D22-UI-UI-SETTINGS-USAGE-1850FE`）裁为 `SUPPORT`，**唯一**目标是 `desktop-behavior:observability-export` 的文件导出叶，子场景 `R00-T02-LA-1850FE23DD5D` 的 `supported_scenario_ids` 也只有导出 `R00-T02-LA-0F28429F329E`。真实 `ModelObservabilitySection.tsx:95-113,200-230` 会加载设置与健康状态，并独立显示用量、调用台账和轨迹；`ObservabilityUsagePanel.tsx:102-164` 查询聚合值并显示指标与图表。读取和筛选结果不是导出文件的一个执行步骤。候选已有 `semantic-effect:model-observability.model_observability_query_aggregate.post` 等查询叶，却没有建立此 UI 入口与它们的支持关系。因此一个只会导出文件、却不显示正确用量的迁移版本仍可能按当前支持链接被误判覆盖。

同一 UI 审查还声称 `ui:settings:usage`“无记录或请求失败显示空态”。实际 `ModelObservabilitySection.tsx:106-113,142-151` 的 bootstrap 请求失败显示 `role="alert"` 错误；`ObservabilityUsagePanel.tsx:115-124,150-154` 的普通聚合查询失败也显示错误框，只有 `not_initialized` 特例走空指标。该拒绝/失败 oracle 与生产代码直接冲突，735/735 场景的存在不能证明每叶结果可检查。

另一个可复核支持错链是 `ui:settings:models`（`F-D10-UI-UI-SETTINGS-MODELS-F92121`）：它唯一指向 `provider:anthropic` 及其场景，但 `ModelsTab.tsx:9-22` 实际渲染辅助模型和媒体默认模型；`AuxiliaryModelsSection.tsx:149-186` 可选择、保存和测试不同供应商的辅助模型，`MediaGlobalDefaultsSection.tsx:90-115` 可保存图片、视频、语音合成与转录默认模型。Anthropic 目录项既不能覆盖这些不同动作，也不是此页的唯一目标。`ui:settings:providers` 亦只挂 Anthropic，尽管 `ProvidersTab.tsx:82-117` 按实际供应商集合列行。修复须按真实消费者把 UI 入口分别连到查询、配置、选择、调用或导出等适用叶及各自子场景，并把错误显示、空数据和无权限三种结果分开断言；不能靠指向一个任意代表叶满足非空校验。

## 前轮关键项复核与保留成果

- **R4-F01 有实质进展，但未闭合。** `ask_user` 的确认/超时/拒答结果、beautify 两个封面工具不同预检及复制后可能残留附件、`pin_memory` 脱敏直写 active、`unpin_memory` 多项删除、`tenet_propose` 只写 pending，均与对应 `lib/tools`、`plugins/beautify` 和 `lib/memory/tenets.ts` 的主要路径相符；供应商 `systemSpeech` 的 `chat.projection=none`、DeepSeek Responses 的 V4-Pro 预登记限制也有明确边界。文件编辑另拆文本/快照读、无版本文本写、二进制写、复制、编辑命令五叶，`file-edit` 留按版本保存；`desktop/main.cjs:6023-6079` 与拆分的主要结果吻合，预览继续独立归 R09-T05/A09/A10。观测导出中断后的可能残留没有被写成必然清理。上述抽查不能抵消 R5-F02/F03 的错误支持目标和失败断言。
- **R4-F02 的原始反例已修一部分。** 独立在内存中给 `server/routes/confirm.ts` 原校验前新增 `deferred`，给 Git push 静态路由新增 `mode=purge`，以及把 slash `/reject` 的调用改为批准，统一门禁均报源码签名变化。原 490 HTTP 叶、382 普通路由逐项报告、72 多方法判定、确认 POST 的批准/拒绝两叶、IPC/slash/别名和内部端点登记均保留。R5-F01/F02 证明“签名变化提醒”尚不等于完整语义复审，也不等于所有可见消费者已进入签名范围。
- **A04 静态分类 PASS。** `core/agent.ts:936-938,1228-1230` 的三个现役子代理工具和 `lib/experiments/registry.ts` 默认关闭的主动委派保留；`core/provider-registry.ts:436,480` 的 Ollama 接入保留；`server/routes/knowledge.ts` 仍挂现役知识库，旧 research 表和历史事件只作兼容，未见研究启动入口；`core/engine.ts:3750-3799` 只加载随包插件。`EXCLUSIONS.md` 没把独立研究引擎、强制子代理目录、本地模型管理或外装 MCP 实例混入保留开发叶。此为当前源码分类，不代表真实供应商或旧用户数据演练通过。

## 已运行验证与结论边界

- `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py`：退出码 0，输出 `CHECKS_OK`、832 登记、24 域、四项结构差集为空。上述四组隔离反例均仅改本进程读取值：确认、Git、slash 的未同步源码变化被检出；同步两层摘要的 `ask_user` 新动作及未覆盖的观测 UI 消费者变化未被检出。没有写回产品或候选文件。
- `npx vitest run` 针对确认、提问、封面工具、slash、文件版本、预览编辑与观测指标/图表共 **8 文件、81 项通过、退出码 0**。这些检查说明当前 TypeScript 路径仍可运行；它们不能替代迁移后 735 项子场景的执行或纠正候选中的支持关系。
- 原工作区已有的旧任务书删除、新任务书未跟踪、总控账本修改和其余候选文件均保留；本报告不要求以运行测试、真实远端服务、跨平台安装包或未来 Rust/Tauri 结果来冒充 R00-T02 静态清单完成。

**R00-A03 维持 FAIL。** 修复 R5-F01/F02/F03 后，应重生冻结候选，针对“同步全套摘要但裁决不变”“最终消费者删除或改写”“支持目标错挂而入口仍非空”三类独立负例做非零检查，再由未参与修复的审阅者复核。**R00-A04 在静态分类范围 PASS。** 两项必须实质满足后，R00-T02 才可交下一阶段完成判定。
