# R00-T02 第八轮 FAIL 根因与最小修复边界

本文件是交给修复者的独立根因分析，不作 R00-A03/A04 或 R00-T02 的 PASS 裁决。核对基点为 `16aeb380d58d68ff1a38bb46f5cc5d18f985f084` 的现役源码与第八轮候选；本轮未修改候选、产品、门禁、总账或旧报告，未提交或推送。

## 结论：同一个误判在候选和门禁中重复

`autoSaveConfig` 在 `desktop/src/react/settings/helpers.ts:92-107` 把原始 patch 送到 `PUT /api/agents/:id/config`，成功后 `GET` 同一路由。这个地址含助手 ID，只是传输与读回入口。服务端 `server/routes/agents.ts:686-690` 先用 `splitByScope` 拆字段并执行全局 setter；只有拆分后仍有助手字段，才走 `:763-769` 的 `saveConfig(agents/{id}/config.yaml)` 和助手运行时刷新。纯全局 patch 在 `:742-750` 提前返回。`shared/config-schema.ts:22-46` 定义哪些字段是全局；`shared/config-scope.ts:26-46` 实现拆分；例如 `core/preferences-manager.ts:925-929` 的 `setKeepAwake` 实际写全局 preferences。`GET /agents/:id/config` 又在 `server/routes/agents.ts:568-581` 读取助手 YAML 后注入全局字段，因此读回呈现合成视图，不能反证该字段写进了助手 YAML。

R8 修复只在九个动作的 `storage_scope.global_preferences_fields` 中记下真实字段，却保留了 `semantic-effect:agent.config.update`（`F-D06-SEMANTIC_EFFECT-SEMANTIC-EFFECT-AGENT-CONFIG-UPDATE-8C8083` / `R00-T02-LA-8C8083F2A5FC`）作为写入结果。该叶明确要求写 `config.yaml` 并刷新该助手；纯全局请求不会经过此分支。正确的同路由全局叶已存在：`semantic-effect:agent.config.global_update`（`F-D06-SEMANTIC_EFFECT-SEMANTIC-EFFECT-AGENT-CONFIG-GLOBAL-UPDATE-24BC2E` / `R00-T02-LA-24BC2E961E3E`）。不能用 `semantic-effect:config.global.update` 替代它：后者对应 `server/routes/config.ts` 的另一条 `/api/config` 路由。

这不是 717 个逐叶事实全部错误。`R00-T02_UI_SOURCE_BRANCH_CONTRACT.json` 共 717 个事实，其中这九个动作各有一条写入事实把全局或混合写入挂到本地叶；`R00-T02_UI_ACTION_MATRIX.json` 的目标、控件变体和场景随之错挂。`r00_t02_inventory.py:1434-1451,1627-1640` 又把矩阵动作复制到 `FEATURE_INVENTORY.json` 的页面 `ui_action_checks` 和 `FEATURE_STAGE_ACCEPTANCE.json` 的场景，并从语义审查生成两条写入叶的逐叶断言。多份文件互相一致，只证明错误被同步传播，不能证明归属正确。R8 契约的一条事实甚至同时写“全局 preferences”与本地叶 F-ID；其 `evidence_basis` 套用了自动化或 Access 的通用文字，未对本字段分流提供独立证明。

门禁也把传输入口当成存储结果：`r00_t02_source_gates.py:119-135` 的注释断言 helper 的终点是每助手配置；`:283-307` 对全部 13 个 `SOURCE_AUTOSAVE_CONFIG_ACTIONS` 强制本地写入叶及读回叶，并禁止 `semantic-effect:config.global.update`，只另用 `CONFIG_SCHEMA` 核对文字化的 `storage_scope`，没有把 scope 与写入叶比较。`:58` 还单独强制 `general#02` 指向本地叶。于是改正 `general#02` 为同路由全局叶会被 `:294-295` 和 `:58` 反向拒绝。R8 现有负例只把 `security#03` 的 `storage_scope` 伪改为助手字段，能发现字段表错，却发现不了字段表正确而 F-ID 错。

## 九个动作应如何挂接

下表的“读回叶”均仍保留 `semantic-effect:agents./agents/.id/config.read`，因为 helper 在 PUT 后确实 GET 合成配置；某些动作还会额外调用 `loadSettingsConfig`，那些条件读边按现役控件保留。以下只裁决写入效果，不将页面上的其他控件、IPC 或读取删掉。

| 动作 | 请求字段或控件 | 应挂写入叶 |
| --- | --- | --- |
| `general#02` | `keep_awake` | 只挂 `agent.config.global_update` |
| `interface#03` | 排版控件发 `{editor:…}`；聊天布局控件另发 `{chat:…}` | 同一矩阵动作列两条**独立请求**：排版挂全局叶，聊天布局挂 `agent.config.update`；两叶都在动作目标中 |
| `interface#04` | `hardware_acceleration` | 只挂全局叶；同页工作区回退另走专用偏好，不并入此请求 |
| `interface#06` | `locale`、`timezone` | 只挂全局叶 |
| `security#01` | `sandbox`、`sandbox_network` | 只挂全局叶 |
| `security#02` | `file_backup` | 只挂全局叶 |
| `security#03` | `network_proxy` | 只挂全局叶 |
| `skills#07` | `capabilities.learn_skills` 下的三个开关 | 只挂全局叶 |
| `work#02` | `desk.heartbeat_master`、`automation.permissionMode` | 只挂全局叶 |

保留 `agent#02/#04/#05/#08` 的本地叶：其 `models.chat`、`memory.enabled`、`experience.enabled`、`tools.disabled` 拆分后仍属助手字段。特别是 `interface#03` 的 `storage_scope` 是两个控件的**汇总**，不能把同时列有全局和助手字段理解为一次 mixed patch；`InterfaceTab.tsx:225-243` 与 `:245-262` 分别调用 `autoSaveConfig`。

真正的一次 mixed patch 可以由客户端发送 `{"keep_awake":true,"chat":{"contentWidth":720}}` 到该 PUT 路由。它先经全局 setter，再以非空 `agentPartial` 保存聊天布局；因此同一请求必须双挂全局叶和本地叶。后段报错不代表已执行的全局 setter 回滚。纯全局 `{"keep_awake":true}` 则只能挂全局写入叶，纯助手 `{"chat":{"contentWidth":720}}` 只能挂本地写入叶。上述示例用于门禁/场景判定，不是说当前界面会发送这样的 mixed patch。

`providers` 与内联凭证是该路由的另一层分流，不能简单按 `CONFIG_SCHEMA` 未声明就认作助手 YAML：`server/routes/agents.ts:692-740` 把 `providers[name]` 的保存/删除送往全局 provider registry；`api`/`embedding_api` 中的 `api_key`、`base_url` 转存全局 provider 条目并清除内联秘密，剩余真正的助手字段才可能继续保存 YAML。它们对应已有 `provider.agent.save/remove`、`provider.inline_credential.save` 等叶。设置页供应商控件常直接走 `PUT /api/config`，应按其真实路由使用 `provider.global.save` 等叶；不可因“全局”二字把所有供应商或凭证归入本次 `agent.config.global_update`。两类路径均须保留现有权限、秘密和写后读取边界。

## 给修复者的最小完整改动

1. 以每个**实际请求体/控件分支**为单位重判效果。八条纯全局动作将写入控件变体、`target_feature_ids`、`target_scenario_ids`、`R00-T02_UI_SOURCE_BRANCH_CONTRACT.json` 的对应事实改接 `agent.config.global_update`；`interface#03` 拆清排版与聊天布局两个请求的控件变体，保留本地叶并增全局叶。矩阵动作目标与变体集合要一致；页面其他读取/IPC 目标不变。
2. 同步九条动作的 `success`、`failure_or_forbidden`、调用链说明及页面状态文字中尚暗示“所选助手落盘”的句子；将“请求带助手 ID”“实际全局持久化”“PUT 后合成视图 GET”分开叙述。`autoSaveConfig` 在 PUT 成功、后续 GET 失败时仍可能报保存失败，实际持久化要读回判断。不要用 toast 作为存储归属证据。
3. 修正 `FEATURE_INVENTORY.json` 中全局叶 `current_stores` 为全局 preferences（及真实 setter 路径），而非 `agents/{id}/config 与人格/记忆文件`；本地叶只描述**有剩余助手字段**时的 YAML 和运行时效果。同步对应的 `FEATURE_STAGE_ACCEPTANCE.json` 逐叶场景，把纯全局、纯助手、真正 mixed patch 的成功与部分失败断言写清。若语义审查 JSON 是这些字段的生成正源，应在正源修，随后重新生成，避免手改生成件被覆盖。
4. 改 `r00_t02_source_gates.py` 的假前提：helper 只证明 PUT 与 GET；根据请求字段经 `CONFIG_SCHEMA` 和 `splitByScope` 的结果，分别要求/禁止全局叶、本地叶。对 `interface#03` 单独证明两段调用和两个不同请求体；对一个真正 mixed patch 才要求同一请求双挂。改 `SOURCE_REQUIRED_UI_EDGES` 的 `general#02` 常量。`semantic-effect:config.global.update` 仍不得混入这九个 helper 请求。普通 `agent#02/#04/#05/#08` 继续要求本地叶。供应商/凭证另按服务端特殊分流校验，不能被上述二元规则误杀。
5. 重生或逐项同步矩阵、717 事实契约、主库存页面 `ui_action_checks`、阶段场景与关联校验及文件指纹。用动作 ID 和场景 ID 做双向差集；每条新事实引用真正的控件请求行、`splitByScope`/服务端分支行，不再用与该事实无关的通用 `evidence_basis`。这属于候选/门禁修复，不需要改现役产品行为。

## 应能执行的反例与正例

建议沿 `r00_t02_inventory.py --negative-checks` 的现有内存注入方式扩展，变异后运行 `validate_ui_source_edges`，并让正常候选先通过该门禁；必要时再跑全量生成检查。至少包含：

| 输入或内存变异 | 预期 |
| --- | --- |
| `general#02` 的纯全局 `keep_awake` 仍挂本地叶、缺全局叶 | 拒绝；当前 R8 错候选必须先变红 |
| 将改正后的 `general#02` 从本地叶换成全局叶，并保持写后 GET 叶 | 接受；验证门禁不再反向拒绝正确归属 |
| `agent#04` 的 `memory.enabled` 被改挂全局叶、移除本地叶 | 拒绝 |
| `interface#03` 只留排版的全局叶而删除聊天布局的本地叶，或只留本地叶而删除排版全局叶 | 两种都拒绝；分别引用两处 `autoSaveConfig` 请求 |
| 单次 `{"keep_awake":true,"chat":{"contentWidth":720}}` 被记录为只有任一写入叶，或描述为原子回滚 | 拒绝；双叶与前段已写可能性均须保留 |
| `security#03` 把 `network_proxy` 文字归为助手字段，或虽写全局字段却仍挂本地叶 | 两种都拒绝 |
| `providers[name]` / 内联 `api_key` 被机械归为 YAML 或 `agent.config.global_update`；`PUT /api/config` 的供应商操作被误挂到 `agent.config.global_update` | 拒绝；特殊分流和不同路由各归其叶 |

复核时先检查九动作的变体/目标/场景与事实契约，再检查生成件和指纹。现有 `config-scope` 单元测试已能证明纯全局拆分结果；它无法替代候选 F-ID 和门禁的上述正反测试。736 个迁移后场景仍待实施，不能据静态修复称它们已执行。
