# R00-T02 第七轮独立验收

**VERDICT: FAIL。R00-A03 FAIL；R00-A04 PASS（仅现役源码的静态分类）。** 本轮只审阅 R00-T02 的清单和映射，不改候选、产品、任务书、测试或总控账本；736 个未来迁移子场景尚未执行，不以此作为本轮失败理由。

## 冻结对象与检查

- 分支 `codex/rust-tauri-migration`，Task Base/HEAD `16aeb380d58d68ff1a38bb46f5cc5d18f985f084`。独立逐文件复算报告所列 18 份非报告候选的 SHA-256，无一差异；按文件名排序、逐行 `文件SHA256␠␠文件名\n` 再计算的聚合值为 `2e80249bcc3a4b8b4f58e1ae9d1fb261acf67c56f00fe4e53c1fb17bb21185d9`。`R00-T02_REPORT.md` 和 `R00-T02_R7_REPAIR.md` 的单文件 SHA-256 分别为 `2249f8c9828af25c99f192bce19651198063846fa5fa40199110fdec685bea2c`、`69452b2157f9665096065307953bed13399bc41ae99d2582576bd9fd8ab8493b`，均与交接一致。`R00-T02_AST_CONDITION_AUDIT.json`、`R00-T02_UI_ORACLE_REVIEW.json` 是辅助审查，不在 18 文件聚合内。
- 已对照本地 AGENTS 约定、R00-T02/A03/A04 和 `05_验收与性能协议.md`、旧功能矩阵、R1—R6 独立报告、R2—R6 根因、R7 交接及 AST/UI 支持审查；这些只用作线索，下面的判定来自本轮对候选和现役源码的反向核查。
- 独立从 JSON 两端复算：832 项原始登记、736 个不重复保留叶（HTTP 491、其他 245）、736 个一对一补充子场景、24 个域；登记归属四组差集为空。26 页有 170 条动作、313 条显式效果变体，矩阵动作与内嵌的 `ui_action_checks`、补充场景断言一致。抽查 HTTP method、`body.action`、IPC、slash、工具、页面支持边与旧 F-ID 对应；文件预览仍独立归 R09-T05/A09/A10，`mobile-workbench` 挂载功能未因开放/闭集 `DECISION_REQUIRED` 被删除。上述结构数字说明被比较的字段闭合，不能证明每个真实动作挂对叶子，亦不能证明同一候选的其他结果字段一致，见 R7-F02。
- `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py --negative-checks` 退出 0，报告 `CHECKS_OK`，包括 R6 的完整 build 双层摘要同步 `includes` 负例。另独立在 Python 内存中给 `ask-user-tool.ts` 增加 `if (["pending_review"].includes(action)) return toolError(...)`，同步该逐叶和统一审查的源码及记录摘要，保留人工契约/场景不变后调用完整 `build()`，非零报新增 `pending_review`；把条件改成运行时 `runtimeActions.includes(action)`，完整 build 非零报 `MANUAL`。未写回文件。等价 `includes("confirmed")` 保持四值结果、使条件摘要变化；无关对象的同名 `action` 不新增分支；改变结果消息的旧契约由结果摘要拒绝。R6-F01 的点名绕过在本版已关闭，静态解析仍不能代替新增上游生产者的人工可达性裁决。
- 逐源码复查 R6-F02/F03 的点名路径：供应商摘要失败在设置初始化处静默吞掉，选择与临时草稿只改本地状态，真正保存另发配置请求；自动化新增提交空提示词、`enabled:false`，HTTP/网络异常均显示错误 toast；用量卡为 10 张，四图独立取数。矩阵逐动作字段与补充场景已更正这些点，但另有现役字段沿用旧错误，见 R7-F02。

## R7-F01 — BLOCKING：自动化“启停”动作只挂停用所走的 `toggle` 叶，漏掉启用的 `update` 效果

**最小复现。** 准备一条 `enabled:false`、提示词非空的 Agent 自动任务，在自动化卡上点击启用；同一控件再点击停用。现役 `AutomationCard.tsx:118-127` 对启用调用 `onUpdate(job.id, { ...updateFields(), enabled: true })`，可同时保存未提交的标题、周期、提示词和模型草稿；对停用才调用 `onToggleEnabled(job.id)`。`AutomationPanel.tsx:108-132` 分别发送 `POST /api/desk/cron` 的 `action:'update'` 与 `action:'toggle'`；`server/routes/desk.ts:1188-1259` 是两个不同分支，前者调用 `store.updateJob`，后者调用 `store.toggleJob`。

候选 `R00-T02_UI_ACTION_MATRIX.json:1362-1378` 的 `ui:panel:automation#03` 写“启停任务”，但 `target_feature_ids` **仅**有 `F-D18-SEMANTIC_EFFECT-SEMANTIC-EFFECT-CRON-TOGGLE-2CAC28`，`target_scenario_ids` **仅**有 `R00-T02-LA-2CAC28D96AFE`；`call_chain` 只指向面板的 `toggleJob`，未列 `updateJob` 或卡片实际调用行 `AutomationCard.tsx:127`。同一错误已逐字段回写 `FEATURE_INVENTORY.json` 与 `FEATURE_STAGE_ACCEPTANCE.json:51516-51532`。`cron.update` 虽另有叶子 `F-D18-SEMANTIC_EFFECT-SEMANTIC-EFFECT-CRON-UPDATE-267BDF`，但它只挂在 `#05`“修改标题/周期/提示词/模型”，该场景的“有变更才发请求”也不能检查 `#03` 在**没有其他字段变化时仍发 `enabled:true`**，或启用时把尚未保存的草稿一起提交。

**影响。** 若照现清单迁移，启用可以被当作 `toggle` 实现或验收，丢失 `update` 的原子保存效果；反过来，只验 `#05` 的普通字段编辑也不会发现启用路径漏接。入口虽有 F-ID，行为与适用场景仍错挂，未达到 R00-A03“真实入口均有归属”的逐结果要求。修复应把 `#03` 明确拆成启用和停用两条可检查变体：启用接 `cron.update`，覆盖 `enabled:true`、空提示词阻止、草稿合并保存及失败；停用接 `cron.toggle`。同步矩阵、库存、场景和审查记录后，重新从卡片点击到服务端分支核对。

## R7-F02 — BLOCKING：当前候选保留旧 UI 结果，主功能清单与逐动作场景自相矛盾

对 `R00-T02_UI_ACTION_MATRIX.json` 中每页的 `error_empty_paths` 与同页 `actions` 按 `action_id` 比较，161 行中有 **67 行文字不一致，涉及 22 页**。这只是差异数，不能把 67 行全断定为行为错误；下列三组则可直接由现役源码判为相反事实。`error_empty_paths` 并未标为原始版本或历史对照，与 `actions` 同在当前矩阵 `pages` 下；明确标旧映射的字段另叫 `original_support_assessment`。生成器 `r00_t02_inventory.py:1443` 只把 `page.actions` 回写为 `ui_action_checks` 和新断言，`validate_ui_matrix()` 没有比较 `error_empty_paths`。因此自检通过及 170/313 项逐动作一致，均不消除这些旧结果。

- **自动化空草稿。** 矩阵 `:1473-1490` 的 `error_empty_paths` 对 `#02—#05` 均写“保存空提示词阻止”“HTTP 失败可能未单独 catch”。同文件 `actions` 的 `#02` 已改成允许 `prompt:''`、`enabled:false` 且 HTTP/网络错误均 toast。最小复现为有助手时点“新增”：`AutomationPanel.tsx:135-185` 发送空提示词停用草稿；`server/routes/desk.ts:1135-1138` 只在**启用**的 Agent 任务要求提示词。让该 POST 返回非 2xx，则面板的 `!res.ok` 分支显示错误 toast；网络抛错也在 `catch` 显示 toast。旧字段会把保持现役行为的迁移版本判错。
- **供应商摘要失败。** 矩阵 `:9568` 的 `error_empty_paths` 对 `providers#01` 要求“载入失败应有错误状态”，同页 `actions` 已写“摘要失败静默”。最小复现为配置已加载、单独让摘要请求失败：`SettingsContent.tsx:392-396` 对 `loadProvidersSummary()` 使用空 `catch`，供应商页没有摘要专用错误显示。旧字段和新场景给出相反预期。
- **用量十卡。** 矩阵 `:11766-11771` 的 `error_empty_paths` 和 `:11663` 的消费链文字仍称“八卡”，而 `ObservabilityMetrics.tsx:44-47,75-80` 从十项 `METRIC_LABEL_KEYS` 渲染空态，非空态亦为十卡。最小复现为打开用量页并检查指标卡数；旧数字会漏验两卡或误判现役结果。

问题还进入主交付物，并非只在一个未使用的备注字段中。`R00-T02_NONHTTP_AUDIT_UI.json` 对自动化的 `refusal_or_boundary` 仍写“HTTP 失败可能未单独 catch”；生成后的 `FEATURE_INVENTORY.json` 自动化 `refusal_or_boundary` 继续沿用，供应商 `refusal_or_boundary` 仍要求摘要失败显示错误状态（`:99782`），用量 `user_action`/`visible_result` 仍称八卡（`:102458-102459`）。这些是 R00-T02 必填的“用户动作、可观察结果、拒绝边界”字段；它们没有标为历史基线。`FEATURE_STAGE_ACCEPTANCE.json` 中同一叶的逐动作断言虽然已更正，两个现役交付物给下游实施者相反依据。

**影响与修复方向。** 以旧审查行、`error_empty_paths` 和主功能清单字段实现或验收时，R6-F02/F03 的错误仍会传递，R00-A03 不能按逐动作场景局部更正判通过。应以现役源码和逐动作裁决统一更新当前字段，把确需保留的旧版本证据明确标成历史对照，并对矩阵每行、逐叶审查、主清单、阶段映射做双向结果一致性检查。67 行差异须逐项裁决；文字不同但语义等价者可说明理由，不机械当成新缺陷。

## A03/A04 裁决与边界

| 项目 | 本轮裁决 | 理由 |
|---|---|---|
| R00-A03 真实入口均有归属 | **FAIL** | 832→736→736 的结构差集为零，R6 点名分支和 UI 逐动作场景有修复；但 R7-F01 是现役可见控件的真实效果与场景错挂，R7-F02 是当前矩阵及主功能清单保留了相反的结果。`R00-T02_UI_ORACLE_REVIEW.json` 给 170/313 全部 `CORRECT`，这一标签不能覆盖上述源码反证。 |
| R00-A04 撤回与现役不混淆 | **PASS（静态分类）** | `core/provider-registry.ts:436,480` 注册 Ollama；`core/agent.ts:936-938,1228-1230` 保留三项现役子代理工具；`lib/experiments/registry.ts:122-145` 主动委派默认关闭；现役知识库路由与旧 research 表/事件的兼容用途分开。`EXCLUSIONS.md` 将强制子代理目录、独立研究引擎、本地模型管理列为硬排除，未误删 Ollama 或现役子代理。 |
| R00-T02 整项 | **FAIL** | A03 未通过，不能按 R7 候选宣称 T02 完成或据此推进总账。 |

本轮没有执行 Rust/Tauri 实装、跨平台安装包、真实供应商和旧用户数据迁移；736 个补充场景均为 `SPECIFIED_NOT_EXECUTED`，应由 R00-T07 正式纳账并在对应实施阶段执行。现役源码静态检查和生成器负例通过，均不等于迁移后运行 PASS。除本报告外没有改动工作区文件，也没有提交或推送。
