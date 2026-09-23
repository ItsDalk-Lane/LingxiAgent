# R00-T02 第九轮独立对抗性验收

**VERDICT: FAIL。R00-A03 FAIL；R00-A04 PASS（只限当前源码的静态现役/撤回分类）；R00-T02 FAIL。** R8-F01 点名的九个设置动作已修正，但供应商配置路径仍有一项可复现的存储与失败边界错误。本轮只新增本报告；未修改候选、产品、任务书、测试或总账，未提交、推送。

## 冻结对象与独立核验

- 分支 `codex/rust-tauri-migration`，Task Base/HEAD 均为 `16aeb380d58d68ff1a38bb46f5cc5d18f985f084`。按交接报告所列 20 个非报告文件逐个复算 SHA-256，20/20 匹配；按文件名排序、逐行 `SHA-256␠␠文件名\n` 再计算的聚合值为 `16983b957c9051236ef6cafeac7881f2cf4483ac6193e064eeceb6d9c1c5109b`。交接报告 SHA-256 为 `d560ab3f4620cb09a1b72b9817f8de8f0bdbd2b7aea76e7dd19912f4d76c02ce`，R9 修复说明为 `2cf1ea0a05c87a370444d05fce659c07d2030857b9c37d58da7edf778d5f069a`，均匹配。旧 Agent/Providers 审查绑定的四份**整文件** SHA 已过期，不作本轮整文件证明。
- 已读本地 AGENTS 约定、新任务书 R00-T02/A03/A04 及通用 01/03/05、R1—R8 独立报告和 R8 根因。以生产调用链复核候选，旧结论、生成器绿灯、`SOURCE_CITED` 标签均只作定位线索。
- 独立从三份 JSON 双向算得 832 个不同的生产登记、736 个不同的保留 F-ID（HTTP 491、其他 245）、736 个一对一补充场景、24 域；登记无空归属、无悬空 F-ID、无未被登记的保留 F-ID，场景也无悬空 F-ID。26 页、177 动作；718 条逐叶事实覆盖全部动作，3413 处事实源码引用的文件及行存在，事实的 F-ID/场景均在动作目标中。736 个场景状态全部为 `SPECIFIED_NOT_EXECUTED`。这些是结构检查，不证明每条事实的行为语义正确。

## R9-F01 — 阻断：供应商纯请求的写盘边界与逐叶 `current_stores` 错误

**最小输入。** 在有权写设置、供应商和相应秘密的本机请求中，对一个已存在的助手发 `PUT /api/agents/:id/config`，请求体仅为 `{"providers":{"openai":{"api_key":"test"}}}`；`openai` 是已注册的内置供应商（`core/provider-registry.ts:428,472`）。准备两个不同的助手 A/B，令请求 `:id=A`，当前焦点为 B，可直接看出存储归属。此输入不含普通全局字段，也不含要写进 A 的助手字段。

**实际路径。** `server/routes/agents.ts:692-709` 的 `saveProvider` 把供应商条目交给 `ProviderRegistry`，并从 `agentPartial` 删除 `providers`；`core/provider-registry.ts:2160-2189,828-834` 经 `ProviderCatalogStore.saveProviders` 写 `$LINGXI_HOME/provider-catalog.json`（`core/provider-catalog.ts:14-16,109-110,156-159`）。自定义本地供应商还可能写/删 `provider-plugins` 下的文件（`core/provider-registry.ts:635-642,2170-2174`；`core/local-provider-plugin-store.ts:303-327`）。然后 `server/routes/agents.ts:731-740` 执行 `onProviderChanged()` 和 **`engine.updateConfig({})`**；前者还可能更新派生的 `models.json`（`core/engine.ts:3348-3353`、`core/model-manager.ts:305-330`、`core/model-sync.ts:552-570`）。后者未传 `agentId`，`core/engine.ts:768-773`、`core/config-coordinator.ts:486-497` 选择当前焦点 B；`core/agent.ts:1334-1338` 即使收到空对象也调用 `saveConfig`，而 `lib/memory/config-loader.ts:114-132` 无条件重写 B 的 `config.yaml`。随后路由因 `agentPartial` 为空在 `server/routes/agents.ts:742-750` 返回，不执行 `:765-769` 针对请求助手 A 的保存。因此供应商**条目**不存进 A/B 的 YAML，但成功路径仍会重写焦点 B 的 YAML；它也不写人格或记忆文件。若 `onProviderChanged` 或后续刷新失败，供应商目录可能已改变而请求返回错误；若不存在焦点 agent，`agent.updateConfig({})` 会抛错，路由在 `:796-797` 返回错误，此时也不能声称已原子回滚供应商目录。这个无焦点分支是源码条件结论，本轮未声称已在运行环境触发。

**候选反证。** `R00-T02_SEMANTIC_AUDIT_G3.json.entries[entry_id="behavior:agents:/agents/:id/config:PUT"].request_cases[id="provider_save"/"provider_remove"].failure_boundary` 均断言“纯 providers 块不写助手 YAML”。这只描述了路由末尾的显式 `saveConfig` 分支，漏掉了前面的 `updateConfig({})`。同一候选的 `FEATURE_INVENTORY.json.features[entry_id="semantic-effect:provider.agent.save"/"semantic-effect:provider.agent.remove"].current_stores` 都是 `agents/{id}/config 与人格/记忆文件`，既漏掉真正的供应商目录，也把焦点 B 的无字段刷新误写成请求助手 A 的配置与人格/记忆存储。两叶 F-ID 分别为 `F-D06-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PROVIDER-AGENT-SAVE-9E3981`、`F-D06-SEMANTIC_EFFECT-SEMANTIC-EFFECT-PROVIDER-AGENT-REMOVE-38A989`，对应场景 `R00-T02-LA-9E3981A9BFCA`、`R00-T02-LA-38A989948FDD`。`provider.inline_credential.save` 的 `current_stores` 也漏掉目录：`server/routes/agents.ts:711-729` 先转存凭证、清空内联秘密，刷新时触及焦点配置，随后剩余 `api` 块还会进入 `:765-769`，写请求助手的 YAML；其 F-ID/场景为 `…464E58` / `R00-T02-LA-464E58DEC9E2`。另一条 `PUT /api/config` 的供应商叶 `semantic-effect:provider.global.save` 仍被写为 `agent config；用户偏好；memory 文件`，同样没有 `provider-catalog.json`，而 `server/routes/config.ts:322-353` 明确通过相同 registry 保存并刷新；其 F-ID/场景为 `…9FBBC2` / `R00-T02-LA-9FBBC275BD34`。

**影响与修复方向。** R00-T02 要求逐叶记录真实存储、可见结果和适用验收；现有表若用来迁移供应商写入，会漏迁权威供应商目录，或把配置刷新误认为供应商数据落在所选助手、人格或记忆文件；失败时还会误判部分成功状态。应把供应商数据权威路径 `provider-catalog.json`、自定义本地供应商文件、条件性 `models.json` 派生投影、焦点助手 `updateConfig({})` 的 YAML 重写、内联凭证后续请求助手 YAML 写入分别写清，更新这些叶的 `current_stores`、逐叶验收断言及 G3 请求样例的失败边界。对 A≠B、刷新失败、内联凭证与 `/api/config` 各设能读回真实数据位置的场景。源码门禁目前只核对全局偏好叶与助手本地叶的 `current_stores`（`r00_t02_source_gates.py:353-359`），`classify_config_patch` 也只返回供应商效果 F-ID；常规 `build()` 和配置正例均接受上述错误文字，需加针对供应商存储/刷新条件的反例。这是**候选规格错误**，本轮未建议改产品源码。

## R8-F01 及前轮问题的复核

- `autoSaveConfig` 在 `desktop/src/react/settings/helpers.ts:92-107` 发 PUT，成功后 GET 合成快照；`shared/config-schema.ts:22-46` 和 `shared/config-scope.ts:26-46` 决定字段归属，`server/routes/agents.ts:686-690,742-750,763-769` 分别执行全局 setter、纯全局早返或剩余助手字段保存。九动作中 `general#02`、`interface#04/#06`、`security#01/#02/#03`、`skills#07`、`work#02` 的请求只含全局字段，当前矩阵的写入变体、目标 F-ID、逐叶场景和源码事实均只挂 `agent.config.global_update`；`agent#02/#04/#05/#08` 的助手字段只挂 `agent.config.update`。`interface#03` 的 `editor` 与 `chat` 分别在 `InterfaceTab.tsx:233,253` 发两次 PUT，矩阵 `storage_scope.request_variants` 两项均为 `same_request:false`，前者挂全局、后者挂助手。真正**单次** `{keep_awake:true,chat:{contentWidth:720}}` 才双挂；两叶 `current_stores` 分别只写 `preferences.json` 与剩余助手字段的 `agents/{id}/config.yaml`，其场景包含跨助手读回及先全局后助手、后段失败可能部分生效。`GET /agents/:id/config` 会在 `server/routes/agents.ts:568-581` 注入全局字段，不能据 GET 反推 YAML 存了全局字段。R8-F01 的点名错挂在本候选已关闭。
- 第八轮门禁反例现能接受上述正确归属，拒绝纯全局错挂助手、纯助手错挂全局、`interface#03` 任一请求缺边、真正 mixed patch 缺边或宣称原子回滚、`security#03` 文字正确而 F-ID 错，以及供应商/内联凭证/另一条路由的错误 **F-ID 分类**。这些正反例没检查 R9-F01 的真实存储文件与隐含刷新。R6 的 `ask_user` 等价 `includes` 新分支即使同步全部摘要，完整 `build()` 仍按预期非零；新增分支无法静态定值时列 `MANUAL`。R7-F01 的自动化启用 `update`/停用 `toggle` 两叶仍在，源码 `AutomationCard.tsx:118-127` → `AutomationPanel.tsx:108-132` → `server/routes/desk.ts:1188-1259` 与 `automation#03/#05` 的成功、空态和失败边界一致。R7-F02 的页面 `error_empty_paths` 与当前动作结果无字段差异；71 条 UI 缺边、67 条旧结果、34 条后续发现的关闭记录没有开放项。抽查 Agent 记忆开关的 utility model 前置、embedding 保存失败回滚，以及 Providers 页 Base URL/API Format 的 `/api/config` 独立请求，与当前两页动作语义相符。
- 旧 Agent/Providers 审查的整文件摘要不再适用；独立重算两页完整矩阵子树 SHA 分别为 Agent `be62fd8c51b27210daf187626efbc1d88fd055fe41c38e70beefa0ec99d0ad35`、Providers `ee402e6a112af5f0b9046a47c34644c26bb8c708d0f9b7c363fd0ad81c932ddb`，与旧审查两页相同。其 33 条动作的 `trigger/success/empty_or_uninitialized/failure_or_forbidden/ui_result_scenario_id/variant_count` 差集为零；243 变体中 241 条显式变体按动作/效果入口/F-ID/场景、2 条合成叶按 F-ID/场景，逐字段差集亦为零。这只允许把旧审查用于**未变化的两页子树**，不构成当前整文件或供应商存储叶的 PASS。
- 代表链仍可定位：笔记本同址 GET/PATCH/DELETE 与自动化 `body.action` 按结果分叶，预览与编辑分叶；preload/main/updater 的桌面 IPC、核心 slash 的主命令及别名、`ask_user` 的批准/超时/拒答、内置工具和设置页非 HTTP 效果均有入口到叶、叶到场景。`mobile-workbench` 保留现役挂载，最终开放/闭集责任仍标 `DECISION_REQUIRED`，未当作删功能理由。上述抽查与四组零差集不能覆盖 R9-F01 的错误。

## 检查结果与边界

| 检查 | 本轮结果 |
|---|---|
| `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py` | 退出 0，`CHECKS_OK`；832 登记、24 域、四组结构差集为空；**对 R9-F01 未报错** |
| `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py --negative-checks` | 退出 0；内部各负例按预期产生非零/`NEGATIVE_DETECTED`，包括 R9 配置 F-ID 反例及 R6 完整 build、同步 SHA 的 `includes` 反例；末尾 `CHECKS_OK` |
| `npx vitest run tests/config-scope.test.ts tests/provider-catalog.test.ts` | 退出 0，2 文件/21 项通过；支持现役字段分流与目录存储行为，不验证候选的供应商 `current_stores` 正确 |
| 独立静态复算 | 20/20 指纹、832→736→736、24 域、718 事实/3413 引用、旧两页子树及 33/243 差集，均如上 |

**R00-A04 静态 PASS。** `core/provider-registry.ts:436,480` 的 Ollama、`core/agent.ts:936-938,1228-1230` 的三项现役子代理工具及 `lib/experiments/registry.ts:122-145` 默认关闭的主动委派仍被保留；现役知识库路由与旧 research 表/事件兼容分开，`EXCLUSIONS.md` 将强制子代理目录、独立研究运行、本地模型管理列为撤回，并把外装 MCP 实例与随包插件分开。未执行真实供应商、四平台安装包、旧用户数据或 Rust/Tauri 实装；736 个迁移场景仍是待执行规格。R9-F01 修正并重冻后，需由未参与修复者再判 A03；本报告不更新总账或宣称 T02 完成。
