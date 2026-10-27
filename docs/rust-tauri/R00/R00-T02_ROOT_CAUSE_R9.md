# R00-T02 第九轮 FAIL 根因与最小修复边界

本文件供下一位修复者使用，不作 R00-A03/A04 或 R00-T02 的 PASS 裁决。核对基点为 `16aeb380d58d68ff1a38bb46f5cc5d18f985f084` 的现役源码、第九轮候选、[R9 独立评审](R00-T02_REVIEW_R9.md)和 [R8 根因](R00-T02_ROOT_CAUSE_R8.md)。本轮只新增本文件，未改候选、产品、门禁、总账或旧报告，未提交、推送。

## 根因：把“没有显式助手字段”误当成“没有助手写盘”

R8 的字段分流修正本身成立：`splitByScope` 先取走全局字段；普通助手字段剩余为空时，`server/routes/agents.ts:742-750` 不执行 `:765-769` 针对请求 `:id` 的显式保存。但供应商有一条**发生在早返之前**的副作用链。`providers[name]` 在 `:692-709` 被送到 `ProviderRegistry.saveProvider/removeProvider` 并从 `agentPartial` 删除；内联 `api/embedding_api` 凭证在 `:711-729` 也会转存。只要 `providersChanged` 为真，路由在 `:731-740` 先调用 `onProviderChanged()`，再调用没有 `agentId` 的 `engine.updateConfig({})`。`core/engine.ts:3077` 转交 `core/config-coordinator.ts:486-497`；后者选择当前焦点 agent，而 `core/agent.ts:1334-1338` 对空对象仍调用 `saveConfig`；`lib/memory/config-loader.ts:114-132` 读取、合并并重写该 agent 的 `config.yaml`。所以纯供应商请求成功时，**不把供应商条目写进请求助手 YAML，却可能重写焦点助手 YAML**。这两句话须同时保留，不能以“纯 providers 块不写助手 YAML”概括全部写盘。

供应商条目的权威存储也不是 YAML。`core/provider-registry.ts:2160-2189,828-834` 经 `ProviderCatalogStore.saveProviders` 写 `$LINGXI_HOME/provider-catalog.json`（`core/provider-catalog.ts:14-16,109-110,156-178`）；旧 `added-models.yaml` 只是兼容迁移入口。仅当供应商是已有本地插件或尚无插件声明、`persistAsLocalPlugin` 成立时，保存还会写 `$LINGXI_HOME/provider-plugins/<id>/manifest.json` 和 `providers/<id>.json`（`core/provider-registry.ts:2169-2174`，`core/local-provider-plugin-store.ts:262-276,303-318`）。删除本地插件时会删其目录；删除内置供应商的用户覆盖项不会删除内置声明（`core/provider-registry.ts:1664-1687`）。`onProviderChanged()` 再经 `reloadAndSync` 更新模型投影；`models.json` 只在内容不同、文件不存在或权限需修正等条件下写入，不能列为每次必写的权威存储（`core/engine.ts:3348-3353`，`core/model-manager.ts:305-330,701-709`，`core/model-sync.ts:546-570`）。人格、记忆文件不是这条路径的写盘目标。

第九轮候选同时在三层保留了误判。`R00-T02_SEMANTIC_AUDIT_G3.json` 的 `provider_save`/`provider_remove` 请求样例只按显式 `agentPartial` 判断“不写助手 YAML”，未写焦点刷新和失败后的部分成功；三条 agents 供应商叶的 `current_stores` 沿用 `agents/{id}/config 与人格/记忆文件`，既漏了权威目录，也把焦点 agent 混作请求 agent。`provider.inline_credential.save` 同样漏目录。`PUT /api/config` 的 `provider.global.save` 与 `provider.global.remove` 两叶仍写 `agent config；用户偏好；memory 文件`，同样漏了目录且混淆独立请求中才可能出现的全局 setter。`FEATURE_STAGE_ACCEPTANCE.json` 的对应逐叶场景继承了过宽的“刷新模型”与泛化失败文字，没有区分目录、派生投影、焦点 YAML 和请求助手 YAML。

门禁为什么放过：`r00_t02_source_gates.py:142-178` 的 `classify_config_patch` 只返回 F-ID 集合，`validate_config_request_cases` 在 `:181-192` 仅比较这组 ID，对供应商样例不审 `failure_boundary`；`:353-359` 只验证普通全局叶和普通助手叶的 `current_stores`，没有检查上述五条供应商叶。`r00_t02_inventory.py:1310-1335` 从语义审查生成叶，但仅覆盖两条普通配置叶的存储，其他叶继承原始路由的泛化存储。第九轮负例在 `r00_t02_inventory.py:1854-1869` 只把供应商请求改成**错误 F-ID**；只要 ID 保持正确，删掉目录、错称无 YAML、漏部分成功仍能通过。生成件彼此一致和 `CHECKS_OK` 均无法证明这些文字正确。

## 按实际条件修复的五条叶

| 叶与入口 | 必须写清的成功路径 | 必须写清的条件与失败边界 |
| --- | --- | --- |
| `provider.agent.save`，`PUT /api/agents/:id/config` 的非 null `providers[name]` | 保存用户覆盖项到 `provider-catalog.json`；刷新模型，随后无 ID 的 `updateConfig({})` 重写**焦点** YAML；纯请求不走针对 `:id` 的 `:765-769`。 | 本地自定义插件才有条件写 `provider-plugins`；`models.json` 只在投影变化等条件下写。目录写成后刷新失败，HTTP 可报错而目录已变。若同一请求还含全局或助手字段，分别另挂相应叶及其写入。 |
| `provider.agent.remove`，同路由 `providers[name]=null` | 对已有用户条目删除目录中的覆盖项/记录删除状态，内置声明仍在；之后走同一刷新和焦点 YAML 路径。 | 本地插件才有条件删插件文件。若条目本就不存在，`ProviderRegistry.remove` 可提前返回、不写目录，但路由仍把 `providersChanged` 置真并尝试刷新；不得断言每次删除都有目录变更或 `models.json` 变更。 |
| `provider.inline_credential.save`，同路由 `api`/`embedding_api` 凭证 | 先读请求助手已有 YAML 以解析 provider，凭证转存目录并清空内联 `api_key/base_url`；刷新焦点 agent 后，剩余 `api` 块继续在 `:765-769` 写请求助手 YAML。 | `:id=A`、焦点 B 时，先可能写 B 的空刷新 YAML，随后显式写 A 的清空字段 YAML；`:769` 的运行时刷新若 A 未加载，还会按 `ConfigCoordinator` 的后备选择落到 B，须单独核对。若 provider 无法解析，`server/routes/agents.ts:722-724` 先返回 400，此凭证不入目录。后段失败时不能把已经转存的秘密视为回滚，验收须检查 A 的 YAML 不保留明文。 |
| `provider.global.save`，`PUT /api/config` | `server/routes/config.ts:322-353` 使用同一 registry、模型刷新与无 ID 的 `updateConfig({})`；目录与焦点 YAML 条件同上。 | 此路由先在 `:307-316` 拒绝助手字段，且全局 setter 在 `:318-320` 独立执行；仅请求包含全局字段才写 preferences。供应商保存不因此自动写 preferences 或 memory。 |
| `provider.global.remove`，同一 `/api/config` 路由 | `providers[name]=null` 调同一 remove；其有项/无项、本地插件/内置覆盖项、模型投影和焦点 YAML 的条件与上面删除分支一致。 | 不应被 `classify_config_patch` 一概判成 `provider.global.save`；若同请求含多供应商项，逐项保存/删除各挂对应叶，前项成功后后项或刷新失败可能部分生效。 |

其中“焦点 YAML 重写”是刷新副作用，不代表供应商条目属于该助手，也不等于配置字段值发生变化。若请求 `:id=A` 而焦点为 B，纯供应商 save/remove 成功后应检查 B 的 YAML 写入路径；A 的 YAML 不因这条纯请求的显式分支而保存。内联凭证的后段则显式保存 A；若 A 的运行时实例未加载，`core/config-coordinator.ts:491` 的 `getAgentById(A) || getAgent()` 还可能把后续刷新及清空字段写到焦点 B，不能仅凭传入 `{agentId:A}` 断定刷新目标必为 A。若焦点不存在，纯供应商请求的 `ConfigCoordinator` 取不到 agent，`agent.updateConfig({})` 在调用处报错；先前目录变动及可能已完成的 `models.json` 投影不应被推断为回滚。若 `onProviderChanged()` 本身失败，后续焦点 YAML 刷新根本不会执行，目录及插件文件可能已经改变，投影是否写入取决于失败位置。以上是源码条件分析，并非本轮执行了相应运行时演练。

## 最小完整修复与门禁反例

1. 在 G3 的 agents 路由语义叶和请求样例、G2 的 `/api/config` save/remove 语义叶中，按上表补齐结果、存储、焦点与失败顺序；`R00-T02_SEMANTIC_AUDIT_G3.json` 中的 `example` 供应商需换成已注册内置项（如 `openai`）作为确定性纯请求正例，另设本地自定义项检验插件文件条件。保持已有权限、秘密掩码与普通全局/助手字段分流，不改现役产品源码。
2. 从语义正源重生 `FEATURE_INVENTORY.json` 五叶的 `current_stores`、`visible_result`、`acceptance_assertions`，以及 `FEATURE_STAGE_ACCEPTANCE.json` 对应逐叶场景；同步依赖它们的候选指纹与账表。存储说明须区分“目录权威”“插件条件文件”“模型派生投影”“焦点 YAML 刷新”“内联凭证后续请求助手 YAML”，不能用 `agents/{id}/config` 或“agent config；用户偏好；memory 文件”概括。
3. 门禁除 F-ID 外直接核对五叶 `current_stores`、结果与失败断言，且依据真实调用链锚点验证 `onProviderChanged`、无 ID `updateConfig({})`、`Agent.updateConfig` 的无条件 `saveConfig` 仍存在；对 `provider_save`、`provider_remove`、`inline_credential`、`other_route_provider` 样例分别检查存储与失败边界。不可只查包含某个词，应使错误归属（如把焦点 YAML 写成请求 A、把 `models.json` 写成必写、把插件文件写成所有供应商必写）确实变红。`/api/config` 的 null 项须分类到现有 `provider.global.remove` 叶。
4. 正例：A≠B 的纯内置 save、已有覆盖项 remove；A=B 的纯 save；本地自定义项 save/remove；只有全局字段、只有助手字段；`/api/config` 纯 save、纯 remove、同请求全局字段加供应商。分别读目录、条件插件文件、模型投影、A/B YAML 与 preferences，且对无变化的投影和不存在条目的删除保持条件断言。内联凭证正例还读 A YAML 的清空字段，确认秘密只在目录；这些场景仍应标 `SPECIFIED_NOT_EXECUTED`，直到迁移实现真正执行。
5. 反例：保留正确 F-ID 却把五叶目录删掉、把纯供应商写成“任何 YAML 均不写”、把焦点 B 误写成请求 A、把本地插件或 `models.json` 写成无条件、把内联凭证的请求助手 YAML 漏掉、把 `/api/config` 删除错挂保存叶、把刷新失败写成原子回滚，均须被拒绝。另以无焦点与 `onProviderChanged` 失败构造阶段性失败：验证先前目录可能已变、后续焦点 YAML 可能未写；以多项供应商/混合 patch 验证前项已写而后项失败的部分成功。正确的纯请求和这些条件边界必须能通过门禁，避免把每种 provider 操作机械等同于同一组必写文件。

验证只需先跑修订后的门禁正反例、生成一致性与相关配置/目录测试；实际文件读回场景属于后续迁移验收。本根因报告不把静态推理记作运行时 PASS。
