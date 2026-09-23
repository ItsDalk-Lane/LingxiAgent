# R00-T02 第十轮独立对抗性验收

**VERDICT: PASS。R00-A03 PASS；R00-A04 PASS（现役源码与清单的静态分类）；R00-T02 PASS。** 本轮只新增独立验收报告，未修改冻结候选、产品源码、测试、任务书或总账，未提交、推送。736 个迁移子场景仍为 `SPECIFIED_NOT_EXECUTED`，须在 R00-T07 纳账并由后续实施任务执行；本轮 PASS 只裁决 R00-T02 的现役功能清单、入口归属与验收地图。

## 冻结对象

- 分支 `codex/rust-tauri-migration`，HEAD/Task Base `16aeb380d58d68ff1a38bb46f5cc5d18f985f084`。开审时工作区已有旧任务书删除、总控进度修改及 T02 未跟踪候选；均未动。
- 独立从交接报告列出的 20 个非报告文件逐项复算 SHA-256，20/20 匹配。按文件名排序，以每行 `SHA-256␠␠文件名\n` 聚合，得到 `972317079442203a8da67609ddcfc8661b304361934fcbd9db999be0c5244ad1`，与冻结值一致。
- `R00-T02_REPORT.md` SHA-256 为 `cbdfff4afcc17b327a68b16bdb7a5bd40dcbcdae1e4f008cde015d58b99f2ac4`；`R00-T02_R10_REPAIR.md` 为 `6b84b6e47a2f76651baed75edde512db37c8881d1f7e6ab4ef0cc878cffb0fba`，均匹配。R1–R9 旧整文件指纹不代替本轮指纹。

## A03：实际写入链与五条供应商叶

从 `server/routes/agents.ts:686-769` 与 `server/routes/config.ts:307-359` 逆查请求入口，正查 `core/provider-registry.ts:1664-1687,2160-2189`、`core/provider-catalog.ts:109-110,156-178`、`core/local-provider-plugin-store.ts:303-327`、`core/engine.ts:3348-3353`、`core/model-manager.ts:305-330,701-709`、`core/model-sync.ts:552-570`、`core/config-coordinator.ts:486-497`、`core/agent.ts:1334-1338` 和 `lib/memory/config-loader.ts:114-132`。结论如下：

| 叶 | 独立核对的来源、结果与场景 |
| --- | --- |
| `provider.agent.save` | `PUT /api/agents/:id/config` 中非 null `providers[name]` 走 `saveProvider`，权威条目写 `provider-catalog.json`；未注册/本地插件才条件写插件文件。`onProviderChanged` 后无 ID 的 `updateConfig({})` 可能重写焦点 B 的 YAML；纯供应商请求早返，不显式写请求 A。候选 F-ID `…9E3981`、场景 `R00-T02-LA-9E3981A9BFCA` 分清 A≠B、A=B、模型投影条件与部分成功。 |
| `provider.agent.remove` | null 项走 `removeProvider`；已有用户覆盖项/本地插件可改目录，内置声明仍在；无条目时可提前返回而路由仍刷新。候选 `…38A989`、`R00-T02-LA-38A989948FDD` 没把目录或 `models.json` 当成每次必写，也没把焦点 B 写成请求 A。 |
| `provider.inline_credential.save` | `api`/`embedding_api` 中的内联 `api_key/base_url` 先依据请求 A 的 YAML 解析 provider，再转存目录并清空内联字段；刷新焦点 B 后，剩余块继续显式保存 A 的 YAML。若 A 的运行时实例未加载，后续 `updateConfig({agentId:A})` 仍可能回退 B。候选 `…464E58`、`R00-T02-LA-464E58DEC9E2` 覆盖目录、A/B YAML、无法解析时 400、秘密清空与后段失败。 |
| `provider.global.save` | `PUT /api/config` 先拒绝助手字段，再经同一 registry 保存；无 ID 刷新焦点 YAML。只有同请求另有全局字段才写 `preferences.json`。候选 `…9FBBC2`、`R00-T02-LA-9FBBC275BD34` 与 UI `ApiKeyCredentials.tsx:17-22` 的实际请求相符。 |
| `provider.global.remove` | 同路由 null 项走删除，不再错挂保存叶；不存在项、内置覆盖项、本地插件和混合请求分别保留条件。候选 `…DBBDA2`、`R00-T02-LA-DBBDA23AB24F` 与源码一致。 |

上述五叶的 `storage_contract`、`current_stores`、`visible_result`、逐叶断言及场景同步存在于 G2/G3 正源、`FEATURE_INVENTORY.json` 和 `FEATURE_STAGE_ACCEPTANCE.json`。`models.json` 只在投影变化、缺失或权限修正时写；目录/插件或全局字段先写后，模型刷新、焦点刷新、请求助手保存失败都可能留下部分成功。`provider_save/remove`、`inline_credential`、`other_route_provider/remove/mixed` 的请求样例把这些顺序与归属逐项写出。无焦点与 `onProviderChanged` 失败场景没有冒称已在运行环境触发。

独立结构复算：832 个唯一生产登记、736 个唯一保留 F-ID 与 736 个唯一逐叶场景双向对应，24 域；入口无主项、无未登记保留叶、无重复归属，26 页的 177 个 UI 动作无缺口。抽查桌面 IPC、核心 slash、同址异方法、`confirm` 正反结果、文件预览、模型观测消费者、自动化启用/停用及设置页面写入边界。R1–R8 指出的入口遗漏、结果合并、非 HTTP 叶遗漏、`ask_user` 分支、自动化启用、UI 旧结果与全局/助手错挂没有在冻结候选回归；Agent/Providers 两页子树 SHA 分别为 `be62fd8c51b27210daf187626efbc1d88fd055fe41c38e70beefa0ec99d0ad35`、`ee402e6a112af5f0b9046a47c34644c26bb8c708d0f9b7c363fd0ad81c932ddb`，与 R9 已核对的未变子树一致。

## A04：撤回与现役分类

`EXCLUSIONS.md` 和库存把强制子代理目录、独立知识研究运行、本地模型管理列为撤回；旧 research 表/事件仅作兼容。`core/provider-registry.ts:436,480` 仍注册 Ollama，`core/agent.ts:936-938,1228-1230` 仍装配三项现役子代理工具，`lib/experiments/registry.ts:122-145` 的主动委派默认关闭但保留；`server/routes/knowledge.ts` 的知识库路由未出现研究启动端点。库存中 `provider:ollama`、三项 `tool:subagent*` 与主动委派实验均为保留叶，撤回三项无实施 Task。外装 MCP 实例、动态命令与随包插件也分开登记。A04 静态分类通过。

## 可重放检查与边界

| 检查 | 本轮结果 |
| --- | --- |
| `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py` | 退出 0，`CHECKS_OK`；832 登记、24 域、四组结构差集为空。 |
| `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py --negative-checks` | 退出 0；内部 R10 正例接受，目录缺失、无 YAML、A/B 混淆、插件/模型必写、漏 A YAML、`/api/config` 删除错挂、原子回滚等反例均 `NEGATIVE_DETECTED`；R1–R9 相关反例仍被拒绝。 |
| `npx vitest run tests/config-scope.test.ts tests/provider-catalog.test.ts` | 退出 0；2 文件、21 项通过。它们验证字段分流及目录读写，不单独证明整个迁移地图。 |
| 隔离临时目录调用 `saveConfig(file,{})` | 退出 0；空 patch 确实重写焦点 YAML 并保留原有字段，复核 R9 的隐含写盘链。临时目录已清除。 |

本轮未运行真实供应商、Rust/Tauri 实装、跨平台安装包或旧用户数据迁移；这些不是 R00-T02 清单验收的已完成项。未发现本候选的阻断问题。候选若再改动，以上 PASS 须重新绑定新指纹并复审。
