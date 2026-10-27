# R00-T02 第十轮修复说明（交独立审阅）

本轮针对 [R9 独立 FAIL](R00-T02_REVIEW_R9.md) 的 R9-F01 修正候选，不自判 R00-A03、R00-A04 或 R00-T02 PASS。分支 `codex/rust-tauri-migration`，Task Base/HEAD 均为 `16aeb380d58d68ff1a38bb46f5cc5d18f985f084`。只改 T02 语义正源、生成器、门禁、生成候选和交接报告；未改产品源码、任务书、总控账本、R1–R9 独立审阅及根因报告，未提交或推送。用户原有旧任务书删除和总控进度改动保持原状。

## 五条供应商叶的修正

`provider.agent.save/remove`、`provider.inline_credential.save`、`provider.global.save/remove` 已从 G2/G3 正源生成逐叶 `current_stores`、`visible_result`、结构化 `storage_contract` 和验收断言，再同步到 `FEATURE_INVENTORY.json` 与 `FEATURE_STAGE_ACCEPTANCE.json`。供应商用户数据的权威位置是 `$LINGXI_HOME/provider-catalog.json`；本地自定义或未注册插件的保存、删除才条件性触及 `provider-plugins`；`models.json` 只在投影变化、文件缺失或权限修正时写。人格、记忆文件不属于这五条叶。

两条路由在供应商目录操作后执行 `onProviderChanged()`，再执行没有 `agentId` 的 `updateConfig({})`。焦点助手存在且刷新走通时，其 `config.yaml` 可被重写。纯 `providers` 请求不走请求 `:id` 助手的显式 YAML 保存；A 为请求助手、B 为焦点助手时，不能把 B 的刷新写入记为 A 的数据。内联凭证分支在转存目录和清空秘密后，剩余 `api/embedding_api` 块仍显式保存 A 的 YAML；A 的运行时实例未加载时，后续运行时刷新还可能回退到 B。`PUT /api/config` 没有请求助手 ID，且仅同一请求另含全局字段时才写 `preferences.json`。

删除不存在的供应商条目可不改目录，删除内置供应商用户覆盖项不会删除内置声明；本地插件删除则条件性删除其文件。同一请求多项供应商逐项处理。目录或全局字段已写后，模型刷新、焦点刷新或后续助手保存失败仍可使 HTTP 报错，不把错误当成自动回滚；无焦点助手时目录可能已变而焦点刷新失败。所有这些读回仍是后续迁移验收规格，未在新实现上执行。

## 门禁与反例

`classify_config_patch` 现把 `/api/config` 的 null 项归 `provider.global.remove`，同请求全局字段与多供应商保存/删除分别挂叶。门禁除 F-ID 外逐字段核对五叶的存储契约、结果、断言、G3 请求样例的前置状态和失败边界，并按现役路由、目录、模型投影、焦点选择和 `saveConfig` 的源码调用顺序校验。正确的 A≠B、A=B、本地插件、无焦点、刷新失败、内联凭证、多项请求、另一条路由的 save/remove/mixed 样例获接受。

第十轮负例在 F-ID 正确时分别删除目录存储、谎称纯供应商请求没有任何 YAML 写入、把焦点 B 误认作请求 A、把插件或模型投影写成必写、漏掉内联凭证的 A YAML、把刷新失败说成原子回滚；另把 `/api/config` 删除错挂 save，并修改 G2/G3 正源结果和现役源码锚点。它们均须非零或输出 `NEGATIVE_DETECTED`。R6/R7/R8/R9 原有反例继续保留。

## 检查与限制

| 检查 | 结果 |
|---|---|
| `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py --write` | 退出 0；832 登记、736 保留叶、736 子场景、24 域，四组结构差集为空 |
| `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py` | 退出 0，`CHECKS_OK` |
| `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py --negative-checks` | 退出 0；内部 R10 正例获接受、错误候选均被拒绝，原 R6/R7/R8/R9 反例仍触发 |
| `python3 -B -m py_compile` 两个 Python 门禁文件；`node --check` AST 门禁 | 均退出 0 |
| `npx vitest run tests/config-scope.test.ts tests/provider-catalog.test.ts` | 退出 0；2 文件、21 项通过 |

这只是现役源码静态审查、候选一致性与定向测试。736 个子场景仍为 `SPECIFIED_NOT_EXECUTED`；未验证 Rust/Tauri 实现、真实供应商、跨平台安装包或旧用户数据。R00-A03/A04 与 R00-T02 的 R10 裁决由未参与修复者作出。
