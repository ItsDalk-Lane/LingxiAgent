# R00-T02 第九轮修复说明（待独立审查）

本轮针对 [第八轮独立 FAIL](R00-T02_REVIEW_R8.md) 的 R8-F01 修正候选和门禁。它是修复者的交接说明，不是 R00-A03/A04 或 R00-T02 的 PASS 裁决。分支 `codex/rust-tauri-migration`；Task Base/HEAD `16aeb380d58d68ff1a38bb46f5cc5d18f985f084`。未改产品源码、原任务书、总控账本、R1–R8 独立报告或根因；未提交或推送。用户原有的旧任务书删除和总控进度改动保持原状。

## 修正的存储效果

`autoSaveConfig` 向 `PUT /api/agents/:id/config` 送 patch，成功后再 GET 合成配置。URL 中有助手 ID，但服务端先按 `CONFIG_SCHEMA` / `splitByScope` 拆字段：全局 setter 写 `preferences.json`；若剩余助手字段为空便提前返回；有助手字段才保存该助手 `config.yaml`。因此八条纯全局动作 `general#02`、`interface#04/#06`、`security#01/#02/#03`、`skills#07`、`work#02` 的写入变体、F-ID 和逐叶场景改为 `agent.config.global_update`，并删除它们对 `agent.config.update` 的写入归属。原来的写后 GET 叶及其他读取、IPC 效果保留。

`interface#03` 现在分别记载排版 `{editor:…}` 和聊天布局 `{chat:…}` 的两个控件、两个独立 PUT、两条写入变体：前者挂全局叶，后者挂助手叶。动作目标含双叶，但不声称界面发送一次混合 patch。`agent#02/#04/#05/#08` 的纯助手字段继续挂本地配置叶。矩阵与源码事实契约的写入证据补上控件请求行、`splitByScope` 和服务端提前返回或 YAML 保存行；页面非 HTTP 审查、支持目标、主库存页面动作及阶段映射由同一正源同步。

同一路由允许客户端在**一次请求**发送 `{"keep_awake":true,"chat":{"contentWidth":720}}`。语义审查正源新增此请求、纯全局、纯助手、供应商与凭证等例；真正 mixed patch 同时归全局和助手叶。全局 setter 在助手保存前执行，后段失败可能留下前段写入，逐叶验收要求分别读回两处且不得宣称原子回滚。全局叶 `current_stores` 现仅列 `preferences.json`，本地叶仅列有剩余助手字段时的 `agents/{id}/config.yaml`。`providers[name]` 走 provider registry；内联 `api_key` / `base_url` 转存 provider 条目，清为空串后 `api` 块仍会进入助手保存分支；`PUT /api/config` 的供应商另归其路由叶。上述分类没有把供应商和凭证机械算作普通全局字段或普通助手字段。

源码门禁从“13 个 helper 动作一律每助手写入”改为逐请求体校验。它核对源码调用、`CONFIG_SCHEMA` 字段、`splitByScope` 移除条件、服务端 setter/提前返回/YAML 保存、动作目标、场景、变体及 `interface#03` 两个 `same_request:false` 记录。第八轮关闭账表 A13 的旧本地 F-ID 和说明同步纠正，仍保留其原独立证据链接。原 Agent/Providers 独立审查报告保持原 SHA 与结论；门禁只在其两页完整子树哈希及 243 条变体键仍相同时复用这两页的旧只读证据，不把四份整文件的新 SHA 冒充为该报告的同版审查。

## 正反检查与结果

| 检查 | 结果 |
|---|---|
| `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py --write` | 退出 0；832 登记、736 保留叶、24 域，四组结构差集为空 |
| `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py` | 退出 0，`CHECKS_OK` |
| `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py --negative-checks` | 退出 0；正确全局/助手归属和纯/混合/供应商请求正例获接受；R9 新增反例均输出 `NEGATIVE_DETECTED`；原 R6/R7/R8 反例继续触发，最终 `CHECKS_OK` |
| `python3 -B -m py_compile` 两个 Python 门禁文件；`node --check` AST 门禁 | 均退出 0 |
| `npx vitest run tests/config-scope.test.ts` | 退出 0，1 文件/16 项通过 |

R9 新增反例覆盖纯全局错挂助手叶、纯助手错挂全局叶、排版/聊天两独立请求缺任一边、真正 mixed patch 缺任一叶或谎称原子回滚、`security#03` 文字正确但 F-ID 错、供应商块误归 YAML、内联凭证误归普通全局叶以及 `/api/config` 供应商误归 agents 路由叶。检查日志在本轮本机 `/tmp/r00_t02_r9_neg_final.log`，不是候选冻结文件。

## 旧独立证据与新候选的边界

旧 Agent/Providers 独立报告冻结的四份整文件 SHA 已因其他页和全局叶修正而改变；不能继续称整文件同版。两页完整矩阵子树按 `json.dumps(page, ensure_ascii=False, sort_keys=True, separators=(',', ':'))` 复算如下：

| 页面 | R8 独立报告 SHA-256 | R9 当前 SHA-256 | 差异 |
|---|---|---|---|
| Agent | `be62fd8c51b27210daf187626efbc1d88fd055fe41c38e70beefa0ec99d0ad35` | `be62fd8c51b27210daf187626efbc1d88fd055fe41c38e70beefa0ec99d0ad35` | 0 |
| Providers | `ee402e6a112af5f0b9046a47c34644c26bb8c708d0f9b7c363fd0ad81c932ddb` | `ee402e6a112af5f0b9046a47c34644c26bb8c708d0f9b7c363fd0ad81c932ddb` | 0 |

旧报告的 33 条动作结果按 `trigger/success/empty_or_uninitialized/failure_or_forbidden/ui_result_scenario_id/variant_count` 逐字段对比，243 条变体中 241 条显式变体按动作文字、效果入口、F-ID、场景对比，2 条合成叶按 F-ID/场景对比，差异均为 0。新候选其余页面及整文件仍须由全新 R9 审查者独立复核。

矩阵为 26 页、177 动作；因 `interface#03` 把原来遗漏的 editor 全局效果独立列为一条事实，源码事实从 717 增至 718。736 个逐叶子场景仍为 `SPECIFIED_NOT_EXECUTED`。本轮没有执行 Rust/Tauri 实装、跨平台安装包、真实供应商或旧用户数据演练；静态检查与现役 config-scope 测试不替代这些后续验收。
