# R00-T02 第八轮独立对抗性验收

**VERDICT: FAIL。R00-A03 FAIL；R00-A04 PASS（仅当前源码的静态分类）；R00-T02 FAIL。** 发现一组现役设置动作的真正存储效果挂到了相反的功能叶和逐叶场景。其余已核对的 R7 点名修复、结构差集及负例通过，不能抵消此错误。本轮只写本报告，未改候选、产品、任务书、测试或总账，未提交或推送。

## 冻结对象与检查范围

- 分支 `codex/rust-tauri-migration`；Task Base 与 HEAD 均为 `16aeb380d58d68ff1a38bb46f5cc5d18f985f084`。独立复算交接报告列出的 20 个非报告文件，各单文件 SHA-256 均匹配；按文件名排序、逐行 `SHA-256␠␠文件名\n` 的聚合 SHA-256 为 `9f152e0f997140b1ef8e8772ae26161d6d8c212b641d6d4d3033b6fbff62e5d2`。`R00-T02_REPORT.md` 为 `58c5fd51111fc729101261a529b5805c600b5bd10b6b66cec4f1bda42d1fda7d`；`R00-T02_R8_REPAIR.md` 为 `4d6f9f57e9ee8df3b8c048238b37cf8bf447c838514e3fb75e163bd78fa9c0af`。
- 已读本地 AGENTS 指令、新任务书 01/03/05 及 R00-T02/A03/A04、R1—R7 独立 FAIL 报告、R7 根因、R8 交接与修复说明。报告和关闭账表仅作为定位线索；下述裁决以当前生产源码与候选字段相对照。
- 独立重算 `ENTRYPOINT_COVERAGE.json`、`FEATURE_INVENTORY.json`、`FEATURE_STAGE_ACCEPTANCE.json` 和两份 UI 矩阵：832 项登记、736 个唯一保留叶（HTTP 491、其他 245）、736 个唯一补充场景、24 域，四组入口归属差集为空；26 页、177 动作、717 个逐叶状态事实，动作的目标 F-ID/场景与源码契约集合无结构差集，3401 条源码定位都指向存在的文件与行。736 个场景状态仍为 `SPECIFIED_NOT_EXECUTED`。这些是结构和定位证据，不代表 717 条解释都与源码语义一致。

## R8-F01 — 阻断：全局设置写入被挂到“助手本地配置写入”叶

**最小复现输入。** 在设置页切换“保持唤醒”，例如当前值为 `false`、用户点成 `true`。`desktop/src/react/settings/tabs/GeneralTab.tsx:218-225` 调 `autoSaveConfig({ keep_awake: true })`；`desktop/src/react/settings/helpers.ts:92-107` 发 `PUT /api/agents/:id/config`，请求体 `{"keep_awake":true}`，成功后另发同地址 GET 读回。这里的 URL 带助手 ID，但不决定字段的最终存储归属。

**实际效果。** `shared/config-schema.ts:35` 将 `keep_awake` 定为 `global`；`shared/config-scope.ts:26-44` 把它从 `agentPartial` 移到 global setter；`server/routes/agents.ts:686-690` 执行 setter，随后 `:742-750` 因 `agentPartial` 为空直接返回。`saveConfig(agents/{id}/config.yaml, …)` 只在 `:763-769` 的非空助手分支执行，因而该输入根本不会写助手配置。`core/preferences-manager.ts:925-929` 的 setter 写全局 `preferences.json`。现有 `tests/config-scope.test.ts:147-155` 也明确断言纯全局 patch 产生空的 agent 部分。

**候选错挂的精确位置。** `R00-T02_UI_ACTION_MATRIX.json.pages[entry_id="ui:settings:general"].actions[action_id="ui:settings:general#02"]` 的 `storage_scope` 已写 `global_preferences_fields:["keep_awake"]`、`agent_config_fields:[]`，但 `target_feature_ids` 和 `target_scenario_ids` 却包含 `F-D06-SEMANTIC_EFFECT-SEMANTIC-EFFECT-AGENT-CONFIG-UPDATE-8C8083` / `R00-T02-LA-8C8083F2A5FC`，没有同一 PUT 路由已登记的 `F-D06-SEMANTIC_EFFECT-SEMANTIC-EFFECT-AGENT-CONFIG-GLOBAL-UPDATE-24BC2E` / `R00-T02-LA-24BC2E961E3E`。`R00-T02_UI_SOURCE_BRANCH_CONTRACT.json.pages[entry_id="ui:settings:general"].facts[action_id="ui:settings:general#02"]` 甚至把“服务端将该字段写入全局 preferences”的文字标为 `SOURCE_CITED`，目标仍是 `…UPDATE-8C8083`。页面子场景和主库存的 `ui_action_checks` 复制了这一错挂。

**错误还在逐叶结果中。** `FEATURE_INVENTORY.json.features[entry_id="semantic-effect:agent.config.update"]` 的 `visible_result/current_stores/acceptance_requirement` 以及 `FEATURE_STAGE_ACCEPTANCE.json.supplemental_scenarios[id="R00-T02-LA-8C8083F2A5FC"]` 断言“写入该助手 config.yaml 并刷新该助手运行时配置”。这与上述输入的实际路径相反。已有的全局叶 `semantic-effect:agent.config.global_update` 虽描述了 `splitByScope` 全局 setter，其 `current_stores` 仍误写成 `agents/{id}/config 与人格/记忆文件`。

**同类范围。** 独立枚举矩阵中 `storage_scope.transport="PUT /api/agents/:id/config"` 且 `global_preferences_fields` 非空的动作，共九条：`general#02`、`interface#03/#04/#06`、`security#01/#02/#03`、`skills#07`、`work#02`。其中八条只含全局字段；`interface#03` 将两个独立控件合在一条动作内：`InterfaceTab.tsx:225-243` 的排版保存发送 `{"editor":…}`，只写全局偏好；`:245-262` 的聊天布局保存发送 `{"chat":…}`，写助手配置。它们并非一次混合请求，但同一候选动作应分别挂全局叶和助手叶。九条均已挂本地配置叶，却没有挂该路由的全局设置叶。相反，`agent#02/#04/#05/#08` 的纯助手字段挂本地叶是合理的。`agent.config.global_update` 当前只出现在 About 页两项动作中，不能替其他九个可见操作建立效果边。

同一路由也接受真正的混合 patch：例如有权客户端发 `PUT /api/agents/:id/config`，请求体 `{"keep_awake":true,"chat":{"contentWidth":720,"bodyFontSizeOffset":0}}`（聊天字段形状见 `desktop/src/react/chat/layout.ts:4-7`）。`splitByScope` 会得到全局 `keep_awake` 和非空助手 `chat`；`server/routes/agents.ts:688-690` 先写全局偏好，再到 `:765-769` 写助手配置并刷新相应运行时。因此这一输入应同时归两叶，且后段若失败不能假设前段全局写入已回滚。修复时需把“单个控件的纯全局请求”“同一页面动作下两个不同控件”“真正混合请求”分别核对，不能只把 F-ID 常量整体替换。

**影响与修复方向。** 按当前 F-ID 或逐叶场景迁移，会把全局开关误实现或误验成助手独有文件写入，也可能漏掉其跨助手生效及真正的全局持久化。应从每个控件的请求体经 `splitByScope` 分流重新挂目标：纯全局字段接 `agent.config.global_update`，一个候选动作内含两类控件或一次混合 patch 时同时接两叶，并同步逐动作、717 事实契约、主库存的 `current_stores`、阶段子场景与门禁。对单纯的 URL 含助手 ID 不应据此推定写入助手文件。当前门禁 `r00_t02_source_gates.py` 的 `SOURCE_AUTOSAVE_CONFIG_ACTIONS` 恰好强制这九条挂 `agent.config.update`，只按 `CONFIG_SCHEMA` 检查文字化的 `storage_scope`，没有把它与真实效果 F-ID 比较。我在内存中将 `general#02` 的目标和场景从本地叶替换成全局叶，调用 `validate_ui_source_edges()`，得到预期的反向拒绝 `设置控件 autoSaveConfig 每助手写/读效果边错挂: ui:settings:general#02`；这说明当前门禁会拦住正确归属，而不能关闭本反例。

## 其他复核结果

- **R7-F01 已修的点名链。** `AutomationCard.tsx:118-127` 对已启用调用 `onToggleEnabled`；未启用且 Agent 提示词空时本地拒绝；可启用时无论其余草稿是否变化，都把 `...updateFields()` 与 `enabled:true` 一次传给 `onUpdate`。`AutomationPanel.tsx:108-132` 分别发 `POST /api/desk/cron` 的 `body.action='toggle'` 和 `'update'`，`server/routes/desk.ts:1188-1259` 分别进入 `store.toggleJob`、`store.updateJob`。矩阵 `automation#03`、两条 F-ID 与场景、页面结果均与该链相符；`#05` 仍限定有字段变化才保存。
- **R7-F02 的同步结构已修。** 同页 `error_empty_paths` 与 177 动作的空态、失败态逐字段比较为零差异；71 条 A/B 确证缺边、67 行旧结果、34 条后续发现的关闭账表分别为 71/67/34 个 `CLOSED`、`OPEN` 为 0。抽查自动化新增空提示词停用草稿与异常 toast、供应商摘要失败静默、用量十卡及活动升级普通会话，当前动作和结果文字与对应源码相符。关闭标签本身不是全量语义证明。
- **生成器反例。** `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py --negative-checks` 退出 0；其内存注入分别使“同步摘要后仍删启用 update 边”“把启用错挂 toggle”“改已启用回调”“去掉启用草稿合并”“恢复页面旧失败文字”“恢复逐叶旧失败文字”均报 `NEGATIVE_DETECTED`，末尾 `CHECKS_OK`。常规 `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py` 也退出 0，报告 832 登记与四组零差集。反例覆盖原点名问题，但未覆盖本轮发现的全局字段 F-ID 语义错挂。
- **其他抽查。** `autoSaveConfig` 的 PUT 后 GET、`loadSettingsConfig` 六路并发读取与经验启用后的第七路、供应商自定义提交/删除的 `PUT /api/config`、微信扫码确认后 `POST /api/bridge/config` 保存并启用及可选 owner、技能文件选择 IPC 与 path/base64 安装分流、技能查看器 IPC、用量四图各自取数，以及现役 HTTP 多方法、IPC、slash 与工具注册的代表链均能定位到候选目标。`R00-T02_UI_AGENT_PROVIDERS_AUDIT.json` 的 33 动作/243 逐叶复核绑定当前矩阵、主库存、阶段映射、非 HTTP 审查四个 SHA；Agent/Providers 两页子树哈希独立复算匹配。Bridge/Models 的 18/51、Skills/Usage 的 25/62 两份辅助报告所写整矩阵 SHA 是早于当前 `1835e507…` 的快照；我逐项比较了其动作目标和显式变体与当前版，没有发现这些键漂移，但旧快照的 `SOURCE_CITED_CORRECT` 不能单独证明当前全部结果文字。共享设置辅助报告也属旧快照；本轮反例即在它已写明的 `splitByScope` 边界上发现。
- **A04 静态分类。** `core/provider-registry.ts:436,480` 仍注册 Ollama；`core/agent.ts:936-938,1228-1230` 仍有现役三项子代理工具；`lib/experiments/registry.ts:122-145` 的主动委派默认关闭。`server/routes/knowledge.ts` 的现役 notebook/source/ingestion/citation 路由没有独立 research 启动入口；旧 research 表/事件仅作兼容。`EXCLUSIONS.md` 将强制子代理目录、独立研究引擎、本地模型管理列为撤回，外装 MCP/动态命令与随包插件分开；`mobile-workbench` 挂载入口未因待决边界被删。此分类通过只针对当前静态生产接线。
- 运行现役针对测试：自动化卡片、面板与 cron 路由共 **3 文件/27 项通过**；`config-scope` **1 文件/16 项通过**。它们支持源码行为与本轮反例，不能验证错误的候选映射为正确。未执行 Rust/Tauri 迁移、736 个补充场景、跨平台安装包、真实供应商或旧用户数据演练；这些仍归后续实施验收，不是本次 FAIL 的理由。

**复审门槛：** 修正九个动作及主叶的存储和场景归属后，重算 20 文件指纹，从控件请求体反向比对 `splitByScope` 分流到 F-ID/逐叶场景，并加入“纯全局 patch 不得挂助手文件叶、混合 patch 必须双挂”的负例。R00-A03 与 R00-T02 在此之前维持 FAIL；R00-A04 的静态 PASS 不受此错挂影响。
