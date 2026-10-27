# R00-T02 第八版修复说明（回应 R7 FAIL）

本说明记录修复候选和可复核证据，不替独立审阅者判断 R00-A03/A04。分支 `codex/rust-tauri-migration`，HEAD/Task Base `16aeb380d58d68ff1a38bb46f5cc5d18f985f084`。本轮只改 R00-T02 候选、生成和门禁，以及本说明和修复者关闭账表；不改产品源码、原任务书、总控账本、R1—R7 独立审阅与根因报告，不提交或推送。

## R7-F01：同一自动化开关按任务状态分流

`AutomationCard.tsx:118-127` 是现役选择点。未启用且 Agent 提示词为空时，本地 toast 并停止；未启用且可启用时，调用 `onUpdate(job.id, {...updateFields(), enabled:true})`，即使标题、周期、提示词、模型草稿都未改也发送 `enabled:true`；已有草稿则随同一次 update 提交。已启用时调用 `onToggleEnabled`。`AutomationPanel.tsx:108-132` 分别发 `POST /api/desk/cron`、`body.action='update'` 和 `'toggle'`；`server/routes/desk.ts:1188-1259` 分别落入 `store.updateJob` 与 `store.toggleJob`。

候选 `ui:panel:automation#03` 因而分别挂 `cron.update` 和 `cron.toggle` 的既有 F-ID 与场景，并给无草稿启用、带草稿启用、空提示词拒绝、已启用停用及各自失败建立本页结果。`#05` 仍是独立“保存编辑”，保持有字段变化才发送 update。`R00-T02_SEMANTIC_AUDIT_G2.json` 同步两叶适用条件。源码门禁直接核卡片状态判断、草稿合并、面板 `body.action`、服务端分支及逐叶场景，不从候选目标并集推导预期边。

## R7-F02：当前结果字段逐项一致

独立结果核查把原 `page.error_empty_paths` 的 67 处差异分为需修、原新皆错、语义等价、动作范围错挂和需具体化；修复者逐项对照现役源码裁决后，将 26 页每条动作的空态和失败态原样派生到同页 `error_empty_paths`，并同步 `R00-T02_NONHTTP_AUDIT_UI.json`。生成器把同一动作的触发、成功、空态、失败及逐叶场景同步到 `FEATURE_INVENTORY.json` 和 `FEATURE_STAGE_ACCEPTANCE.json`；逐字段门禁在任一处恢复旧文字时非零。

点名相反事实已更正：自动化新增允许空提示词的停用草稿，启用才检查；供应商摘要失败在设置初始化中静默；用量页十张指标卡；活动升级为普通会话，不生成自动化且错误时无 toast。主清单还补齐地图选区追问、助手记忆动作、供应商模型参数、聊天 slash/排队、微信 Bridge 等与现役动作一致的结果。原始旧支持判断仅留在明确标为历史的 `original_support_assessment`，当前结果字段不沿用。

## 26 页状态分流复核

`R00-T02_UI_SOURCE_BRANCH_CONTRACT.json` 逐条记录控件状态或自动加载条件、来源行、最终效果 F-ID 和逐叶场景。它与矩阵双向比较；`SOURCE_CITED` 必须有现役源码或独立逐变体审阅证据，`MANUAL` 不算通过。`r00_t02_source_gates.py` 另固定由生产源码推出的必需边与错挂禁边，覆盖同控件多效果、写后条件刷新和 IPC；SHA 全同步也不能隐藏缺边。A/B 两份只读支持报告的 71 条确证缺边，加上 67 行结果差异，在 `R00-T02_R8_CLOSURE_LEDGER.json` 逐项检查为 `CLOSED / MANUAL_WITH_REASON / OPEN`；后续 Bridge/Models、Skills/Usage、共享设置与 Agent/Providers 定向报告的确证差异另列关闭检查。独立报告均为其阅读时刻的快照，不能据旧 `CORRECT` 标签宣称新候选通过。

本次矩阵为 26 页、177 动作、717 个逐叶状态事实。最后的 Agent/Providers 两页由全新只读审阅者对固定四文件做同版复核：33 动作、243 逐叶均 `SOURCE_CITED_CORRECT`，无 `WRONG` 或 `MANUAL`，报告将四份 SHA 与源码 HEAD 一起固定。源码契约据此达到 717 项 `SOURCE_CITED`、0 项 `MANUAL`；门禁再核四文件 SHA、独审逐叶目标与场景、契约双向差集。A/B 确证 71 项、旧结果差异 67 项、后续定向发现 34 项在修复者关闭账表均为 `CLOSED`，`OPEN` 为 0。这是修复者和辅助审查的静态证据，不代替下一位独立审阅者对 R00-A03/A04 的最终裁决。

反向核查后补齐的典型路径包括：助手创建/删除/切换后的条件读取、人物卡预览图片和卡片头像、供应商凭据保存与详情挂载、Bridge 微信绑定及重读、技能选择器和查看器 IPC、频道创建、地图换会话、MCP 写后刷新、用量四种刷新状态。设置页 `autoSaveConfig` 的请求入口是 `/api/agents/:id/config`，成功后立即 GET 同一路由；服务端 `splitByScope` 会把部分字段存到全局 preferences，因此传输入口与最终存储归属分列。PUT 已成功而后置 GET 失败可能仍提示“保存失败”，不能据提示断定没有持久化。`loadSettingsConfig` 的六项并发 GET、经验启用时第七项 GET，以及它内部吞读取错误的现役边界，均按适用动作记录。

## 门禁与边界

新增负例在内存中改变候选或源码，不写产品文件：仅把自动化 `#03` 留 toggle 并同步 SHA、把启用错挂 toggle、改回调或去掉草稿合并、恢复 `error_empty_paths` 或逐叶审查旧文字，均应使完整门禁非零。原 R6 `ask_user` AST 负例继续运行。现役源码静态审查与生成门禁不等于 Rust/Tauri 实施后的运行验收；736 个补充子场景仍为 `SPECIFIED_NOT_EXECUTED`。动态消费者、真实平台/供应商及迁移包行为由后续任务验证。R00-A03/A04 最终裁决仍交未参与修复者。
