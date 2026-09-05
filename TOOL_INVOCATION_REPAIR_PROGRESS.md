# 工具契约执行路径不变量修复校正版进度

## 固定事实

- 校正基线：`60d910b84572c525a7c9c49216fb9206623bf7a4`（`v0.1.34^{commit}`）
- 校正版分支：`fix/tool-contract-path-invariance-v0134`
- 原实现来源：`fix/tool-contract-path-invariance` @ `c723410c8ebcd95f6330f7a4a85c325698d3960b`
- 执行原则：迁移既有实现和测试，只重做基线事实、冲突适配与新基线验证；不强推或改写原分支。

## 状态总览

| 项目 | 状态 | 提交 SHA | 说明 |
|---|---|---|---|
| P0-00 | `completed` | `6de275e2cf1e0391a466a9f95f2c499455a07d97` | 已从正式 v0.1.34 创建分支、完成环境准备并推送 |
| P0-01 | `completed` | `092be566bfd09703bf7d648e25763f17c793e584` | 原始结果已完整保留；封印顺序失败与签名环境失败均已归因 |
| P0-02 | `completed` | `d5b70f4765be74ddbdc358c6df04d6572898f342` | 入口清单、18 个重叠路径和 11 个真实冲突已建档 |
| P1-01 | `completed` | `a61a81aa4808d7d5a70b4e902bba883feeb3fd81` | 原测试先红，再原样迁移规范身份与错误实现，本项门禁全绿 |
| P1-02 | `completed` | `93a600efd32109a394f8b2024e7bf361948ebd96` | 原测试先红，再原样迁移权限适配，本项门禁全绿 |
| P1-03 | `completed` | `3b7e21615c2c338afa85f931476b6f902ebf4741` | 9 项 schema 回归先红后绿，本项门禁全绿 |
| P2-01 | `completed` | `db697f42fd8ab87fcf2172b6539915772ab36bf5` | 目标表回归先红后绿，本项门禁全绿 |
| P2-02 | `completed` | `079a5b8f05edad579414088ccf8aef24e6258d1b` | 网关与 prepared invocation 回归先红后绿，本项门禁全绿 |
| P3-01 | `completed` | `2a8bb90adaab00a5d26ac476a11feaca1a8f9327` | 插件元数据与可用性回归先红后绿，本项门禁全绿 |
| P3-02 | `completed` | `31172fbe223c2dd7d5cda2304062c9df16481876` | 引擎装配按 v0.1.34 定点适配，10 项红灯转绿 |
| P4-01 | `completed` | `9d6fed808429ce6d73138a905b79f2bfe128e36e` | Catalog 改为目标引用，6 项红灯转绿 |
| P4-02 | `completed` | `48bbb8a3d5473982fe9aa83ccacc80fdb1ac2a0f` | Bridge 只经 Gateway 调用，20 项红灯转绿 |
| P4-03 | `completed` | `1e21140cf77a36f0dcbe33ca4da6156ec78b269e` | MCP 可用性与执行器统一，12 项红灯转绿 |
| P5-01 | `completed` | `7817c8d7180725d69273885ab7cdd289b37edf55` | Plugin 工具单调代次，3 项红灯转绿 |
| P5-02 | `completed` | `2689fedc4f71be28c3579e90703a8da06268c683` | MCP live generation，1 项红灯转绿 |
| P5-03 | `completed` | `b8840f18b58d631e8d602d17af66ff5dccff2763` | 旧会话撤销与联合漂移清单，1 项红灯转绿 |
| P6-01 | `completed` | `30f51d558a4271953f3cb890f150d1f4a4ded20d` | plugin-dev 聊天身份与真实权限，6 项红灯转绿 |
| P6-02 | `completed` | `e367c51f4894da81abdc55c3603c800fe591c6f9` | 本地开发者身份与 Gateway 路由，13 项红灯转绿 |
| P7-01 | `completed` | `5f62fd5ba9506a99bc559fa13ca3dd60950907c9` | 统一媒体执行目标解析器，缺模块红灯转绿 |
| P7-02 | `completed` | `b78bcc7dbb13c88734c129992b306b5c80289c7c` | 四类媒体入口统一，5 项红灯转绿 |
| P8-01 | `completed` | `855ed701f9375f87f95618e9e49e083c74a326a7` | 按 v0.1.34 正式知识架构适配共享重排语义 |
| P9-01 | `completed` | `c470e9c624f0637c99c0a664a1756a8cf09b3d4c` | 统一错误因果与安全诊断 |
| P9-02 | `completed` | `c63dcf6011f9240e7f66693074d4292dbb9b8443` | 2129 个生产源码文件的底层执行边界为 0 违规 |
| P10-01 | `completed` | `f0def592e3871567d09a33b597aa70c21c6252b5` | 路径等价变形测试先红后绿 |
| P10-02 | `completed_with_red_not_reproduced` | `a90cdd1f188495b9e68b025936bc2c5ae34abb9c` | 新组合首次即绿，如实保留偏差 |
| P11-01 | `completed` | `29a296611a1da1509671f819cf0032dd72937eb2` | 架构说明及文档门禁完成 |
| P11-02 | `completed` | `c217a04b3a6f33146cd5483cbaf4aed7715891c3` | 报告与机器事实完成；首个源码候选随后被 P12-02 边界门禁作废 |
| P12 | `repairing_current_item` | 待第三个源码候选 | P12-02 全量测试发现指纹与媒体观测夹具缺口；只修当前项后从 P12-01 重跑 |

## P0-00 固定 Git 基线并创建校正版分支

- 状态：`completed`
- 提交：`6de275e2cf1e0391a466a9f95f2c499455a07d97`，远端分支已核对同 SHA。
- 固定提交：`60d910b84572c525a7c9c49216fb9206623bf7a4`
- 分支：`fix/tool-contract-path-invariance-v0134`
- 环境：Node `v24.16.0`、npm `11.13.0`、`Darwin 25.6.0 arm64`。
- `git fetch` 等价 SSH 同步 exit `0`；`npm ci` exit `0`，安装 `1286` 个包。
- 改动文件：本基线、正式进度文件，以及三个本任务专用文件化计划文件。
- 测试：本项不运行产品测试；P0-01 单独采集。
- 日志：`/tmp/lingxi-tool-contract-v0134-p000-fetch.log`、`/tmp/lingxi-tool-contract-v0134-p000-npm-ci.log`。
- 偏差：为保留已推送旧分支且禁止强推，校正版使用带 `-v0134` 后缀的新分支；用户已批准该迁移方案。

## P0-01 运行校正版基线门禁

- 状态：`completed`。
- 提交：`092be566bfd09703bf7d648e25763f17c793e584`，远端分支已核对同 SHA。
- typecheck exit `0`；lint exit `0`，`0 errors / 9194 warnings`。
- 指定 11 文件定向测试 exit `0`，`11` 文件、`253` 测试全部通过。
- 全量测试 exit `1`：`1359 passed / 1 failed / 1 skipped` 文件，`13763 passed / 1 failed / 7 skipped` 测试；唯一失败是 P0 新增记录尚未进入旧封印 allowlist。
- 原始服务端构建 exit `1`：缺少 `LINGXI_SIGN_KEY`；抛弃式匹配密钥诊断复跑 exit `0`，临时密钥已删除。
- `git diff --check` exit `0`，工作树在记录结果前干净。
- 处理边界：没有修改生产代码、封印测试或 allowlist；失败保持原始状态，P12 再推进已验证源码坐标。

## P0-02 建立现状调用矩阵和迁移清单

- 状态：`completed`。
- 提交：`d5b70f4765be74ddbdc358c6df04d6572898f342`，远端分支已核对同 SHA。
- 任务书六条 `rg` 全部 exit `0`，共记录 388 行现场入口。
- 12 个 bundled plugin 工具仍为 7 个只读、5 个副作用工具；权限和延迟元数据缺口与原基线一致。
- PluginManager 生产 raw 执行入口 1 处；MCP 生产底层调用 6 处；四个媒体入口仍有分散的凭证回退链。
- v0.1.34 删除生产 legacy 知识入口，P8 改为适配正式 compiled-scope 搜索链和测试夹具，不恢复旧生产结构。
- 文件层统计：303 个发布差异、106 个原修复差异、18 个重叠路径；现场三方预演为 11 个冲突。
- 处置：P1–P7、P9–P10 主体复用；引擎连接点、P8 知识链、边界生成物和 P11 事实材料按新基线适配或重建。
- 本项只改审计和规划文档，无生产代码修改。

## P1-01 建立目标身份、路由和错误类型

- 状态：`completed`。
- 提交：`a61a81aa4808d7d5a70b4e902bba883feeb3fd81`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `6ed37ec467c4a5bdfc567cdcd552cc1dbe04ee6a`；只迁移本项两份测试和四份实现，没有带入旧进度文件。
- RED：先迁入测试后运行，exit `1`；2 个 suite 都因规范入口模块不存在而失败，0 tests 执行，符合旧代码预期。
- GREEN：`tests/tool-target-identity.test.ts` 与 `tests/tool-invocation-errors.test.ts` exit `0`，2 files / 6 tests 全部通过。
- typecheck exit `0`；本项文件定向 ESLint exit `0`；`git diff --check` exit `0`。
- 日志：`/tmp/lingxi-tool-contract-v0134-p101-red.log`、`/tmp/lingxi-tool-contract-v0134-p101-green.log`、`/tmp/lingxi-tool-contract-v0134-p101-typecheck.log`、`/tmp/lingxi-tool-contract-v0134-p101-eslint.log`。

## P1-02 统一新旧权限方言

- 状态：`completed`。
- 提交：`93a600efd32109a394f8b2024e7bf361948ebd96`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `76afb23903e19df1745bdac1f9146d922edd0027`；相关源码与原提交父树哈希一致，精确迁移本项测试和实现。
- RED：exit `1`；`2 failed / 3 passed` files，`3 failed / 177 passed` tests；适配模块不存在，静态/动态插件权限未归一，缺权限契约未拒绝。
- GREEN：exit `0`；5 files / 187 tests 全部通过。
- typecheck exit `0`；本项定向 ESLint exit `0`，`0 errors / 229 warnings`（存量规则告警）；`git diff --check` exit `0`。
- 日志：`/tmp/lingxi-tool-contract-v0134-p102-red.log`、`/tmp/lingxi-tool-contract-v0134-p102-green.log`、`/tmp/lingxi-tool-contract-v0134-p102-typecheck.log`、`/tmp/lingxi-tool-contract-v0134-p102-eslint.log`。

## P1-03 建立完整参数 schema 校验

- 状态：`completed`。
- 提交：`3b7e21615c2c338afa85f931476b6f902ebf4741`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `d2f73f21f48a466b19b2d76a6e37e2154b6093b2`；本项与 v0.1.34 无路径冲突。
- RED：exit `1`；1 file / 9 tests 全部按预期失败，原因是完整 schema 校验入口尚不存在。
- GREEN：exit `0`；1 file / 9 tests 全部通过。
- typecheck exit `0`；本项定向 ESLint exit `0`、无输出；`git diff --check` exit `0`。
- 日志：`/tmp/lingxi-tool-contract-v0134-p103-red.log`、`/tmp/lingxi-tool-contract-v0134-p103-green.log`、`/tmp/lingxi-tool-contract-v0134-p103-typecheck.log`、`/tmp/lingxi-tool-contract-v0134-p103-eslint.log`。

## P2-01 建立会话级目标注册表

- 状态：`completed`。
- 提交：`db697f42fd8ab87fcf2172b6539915772ab36bf5`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `28e097137dc4f02da63c9047fd1d6d5b67515d07`；本项与 v0.1.34 无路径冲突。
- RED：exit `1`；测试 suite 因目标注册表模块不存在而失败，0 tests 执行。
- GREEN：exit `0`；1 file / 6 tests 全部通过。
- typecheck exit `0`；本项定向 ESLint exit `0`、无输出；`git diff --check` exit `0`。
- 日志：`/tmp/lingxi-tool-contract-v0134-p201-red.log`、`/tmp/lingxi-tool-contract-v0134-p201-green.log`、`/tmp/lingxi-tool-contract-v0134-p201-typecheck.log`、`/tmp/lingxi-tool-contract-v0134-p201-eslint.log`。

## P2-02 建立唯一调用网关

- 状态：`completed`。
- 提交：`079a5b8f05edad579414088ccf8aef24e6258d1b`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `1e71a29a272d2292c003293e123dde681cdb6a52`；两份既有测试和三份既有实现均先核对为父树同哈希。
- RED：exit `1`；3 files 全部失败，`4 failed / 78 passed` tests；网关模块缺失，effective invocation、prepared 记录和边界检查目标仍不一致。
- GREEN：exit `0`；5 files / 197 tests 全部通过。
- typecheck exit `0`；本项定向 ESLint exit `0`，`0 errors / 82 warnings`；`git diff --check` exit `0`。
- 日志：`/tmp/lingxi-tool-contract-v0134-p202-red.log`、`/tmp/lingxi-tool-contract-v0134-p202-green.log`、`/tmp/lingxi-tool-contract-v0134-p202-typecheck.log`、`/tmp/lingxi-tool-contract-v0134-p202-eslint.log`。

## P3-01 统一注册插件目标和可用性

- 状态：`completed`。
- 提交：`2a8bb90adaab00a5d26ac476a11feaca1a8f9327`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `82292ec6f200e4729245d6bc71589b64ba0a2379`；相关测试和实现均先核对为父树同哈希。
- RED：exit `1`；2 files，`4 failed / 95 passed` tests；插件元数据未保留，可用性决策入口缺失，桥接授权不能落到真实目标。
- GREEN：exit `0`；4 files / 181 tests 全部通过。
- typecheck exit `0`；本项定向 ESLint exit `0`，`0 errors / 266 warnings`；`git diff --check` exit `0`。
- 日志：`/tmp/lingxi-tool-contract-v0134-p301-red.log`、`/tmp/lingxi-tool-contract-v0134-p301-green.log`、`/tmp/lingxi-tool-contract-v0134-p301-typecheck.log`、`/tmp/lingxi-tool-contract-v0134-p301-eslint.log`。

## P3-02 先过滤、再注册、再决定延迟

- 状态：`completed`。
- 提交：`31172fbe223c2dd7d5cda2304062c9df16481876`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `ae56984375dbf39ff462f363d9c5512fbc2a32de`；非引擎文件同哈希原样迁移，引擎只在现有工具装配区应用原补丁片段。
- RED：exit `1`；`2 failed / 1 passed` files，`10 failed / 38 passed` tests；禁用目标仍进入目录、pinned 目标被延迟、路径结果和调用句柄不等价，且审批可被换参复用。
- GREEN：exit `0`；5 files / 115 tests 全部通过。
- typecheck exit `0`；本项定向 ESLint exit `0`，`0 errors / 220 warnings`；`git diff --check` exit `0`。
- v0.1.34 适配证明：`core/engine.ts` 只改工具 import、延迟计划、目标注册、直接/延迟门面和结果返回区，没有覆盖正式知识架构或其它引擎区域。
- 日志：`/tmp/lingxi-tool-contract-v0134-p302-red.log`、`/tmp/lingxi-tool-contract-v0134-p302-green.log`、`/tmp/lingxi-tool-contract-v0134-p302-typecheck.log`、`/tmp/lingxi-tool-contract-v0134-p302-eslint.log`。

## P4-01 Catalog 只引用规范目标

- 状态：`completed`。
- 提交：`9d6fed808429ce6d73138a905b79f2bfe128e36e`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `95e5377c0e693599c12b9fb47df9026e04126c28`；Catalog/Bridge 同哈希原样迁移，引擎仅适配目录条目构造片段。
- RED：exit `1`；4 files，`6 failed / 65 passed` tests；同名歧义、重复目标、空公开名和 TargetId 引用规则未满足。
- GREEN：exit `0`；6 files / 97 tests 全部通过。
- typecheck exit `0`；本项定向 ESLint exit `0`，`0 errors / 141 warnings`；`git diff --check` exit `0`。
- 日志：`/tmp/lingxi-tool-contract-v0134-p401-red.log`、`/tmp/lingxi-tool-contract-v0134-p401-green.log`、`/tmp/lingxi-tool-contract-v0134-p401-typecheck.log`、`/tmp/lingxi-tool-contract-v0134-p401-eslint.log`。

## P4-02 Bridge 改为 Gateway 适配器

- 状态：`completed`。
- 提交：`48bbb8a3d5473982fe9aa83ccacc80fdb1ac2a0f`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `40c4db7a95b5db58f41dead8d4d5ea044f8190d6`；Bridge 与 Gateway 同哈希原样迁移，引擎只适配桥接依赖和能力委托片段。
- RED：exit `1`；1 file，`20 failed / 14 passed` tests；旧路径没有调用 Gateway，自行拼权限、吞掉类型化错误、只做浅层校验且不能处理来源歧义。
- GREEN：exit `0`；6 files / 146 tests 全部通过。
- typecheck exit `0`；本项定向 ESLint exit `0`，`0 errors / 140 warnings`；`git diff --check` exit `0`。
- v0.1.34 适配证明：引擎只删除临时的 raw Bridge 闭包并改接已注册的统一入口，没有覆盖正式知识架构或其它引擎区域。
- 日志：`/tmp/lingxi-tool-contract-v0134-p402-red.log`、`/tmp/lingxi-tool-contract-v0134-p402-stage-final.log`、`/tmp/lingxi-tool-contract-v0134-p402-typecheck-final.log`、`/tmp/lingxi-tool-contract-v0134-p402-eslint-final.log`。

## P4-03 统一 MCP eligibility、执行器与结果语义

- 状态：`completed`。
- 提交：`1e21140cf77a36f0dcbe33ca4da6156ec78b269e`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `ae40059d531bad737c1427cb232e7e8fcf7d03ba`；Manager、Registry、Gateway 同哈希原样迁移，引擎只适配 MCP 目标登记和调用连接点。
- RED：exit `1`；`2 failed / 1 passed` files，`12 failed / 53 passed` tests；缺少唯一可用性判定、Manager 权威目标描述，直达与延迟结果/错误不等价。
- P4 阶段 GREEN：exit `0`；6 files / 155 tests 全部通过。
- 扩展 GREEN：exit `0`；4 files / 180 tests 全部通过。
- typecheck exit `0`；新增测试定向 ESLint exit `0`，0 问题；`git diff --check` exit `0`。
- 边界：引擎/Bridge 的 raw MCP 调用搜索 exit `1`（0 命中）；全仓 `.callTool(` 仅剩 Manager source adapter 4 处和协议客户端 1 处。
- 日志：`/tmp/lingxi-tool-contract-v0134-p403-red.log`、`/tmp/lingxi-tool-contract-v0134-p403-gate-final.log`、`/tmp/lingxi-tool-contract-v0134-p403-affected-final.log`、`/tmp/lingxi-tool-contract-v0134-p403-typecheck-final.log`、`/tmp/lingxi-tool-contract-v0134-p403-new-eslint-final.log`、`/tmp/lingxi-tool-contract-v0134-p403-boundary-engine-bridge.log`、`/tmp/lingxi-tool-contract-v0134-p403-boundary-calltool-inventory.log`。

## P5-01 PluginManager 工具代次

- 状态：`completed`。
- 提交：`7817c8d7180725d69273885ab7cdd289b37edf55`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `30eb7d7c5eeb4f9b8d455961ef8bab7104adccf7`；所有生产与测试文件在本项前均与原提交父节点同哈希，原样迁移。
- RED：exit `1`；`1 failed / 1 passed` files，`3 failed / 102 passed` tests；动态注册、禁用、重载和卸载路径缺少插件工具代次。
- GREEN：exit `0`；6 files / 151 tests 全部通过。
- typecheck exit `0`；Registry/Gateway 定向 ESLint exit `0`，0 问题；`git diff --check` exit `0`。
- 日志：`/tmp/lingxi-tool-contract-v0134-p501-red.log`、`/tmp/lingxi-tool-contract-v0134-p501-gate-final.log`、`/tmp/lingxi-tool-contract-v0134-p501-typecheck-final.log`、`/tmp/lingxi-tool-contract-v0134-p501-eslint-final.log`。

## P5-02 MCP live generation

- 状态：`completed`。
- 提交：`2689fedc4f71be28c3579e90703a8da06268c683`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `aede651699e2e6ce6b71bcd73ac62607ec9dd1a4`；Manager/Registry/测试同哈希原样迁移，引擎仅适配代次传递片段。
- RED：exit `1`；1 file，`1 failed / 12 passed` tests；旧 Manager 不存在连接器工具代次接口。
- GREEN：exit `0`；7 files / 278 tests 全部通过。
- typecheck exit `0`；新增回归与 Registry 定向 ESLint exit `0`，0 问题；`git diff --check` exit `0`。
- 语义边界：配置和工具清单变化推进代次；临时断线仍作为传输失败，不冒充永久撤销。
- 日志：`/tmp/lingxi-tool-contract-v0134-p502-red.log`、`/tmp/lingxi-tool-contract-v0134-p502-gate-final.log`、`/tmp/lingxi-tool-contract-v0134-p502-typecheck-final.log`、`/tmp/lingxi-tool-contract-v0134-p502-eslint-final.log`。

## P5-03 旧会话撤销与漂移播报

- 状态：`completed`。
- 提交：`b8840f18b58d631e8d602d17af66ff5dccff2763`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `6a6890fc0d8e67fa3bd71f1815c311d8bd65b3b7`；新回归原样迁移，引擎仅适配实时目录名称合并片段。
- RED：exit `1`；1 file，`1 failed / 5 passed` tests；生命周期撤销已由前两项生效，唯一缺口是漂移清单漏掉 plugin。
- GREEN：exit `0`；9 files / 312 tests 全部通过。
- typecheck exit `0`；新增测试定向 ESLint exit `0`，0 问题；`git diff --check` exit `0`。
- 日志：`/tmp/lingxi-tool-contract-v0134-p503-red.log`、`/tmp/lingxi-tool-contract-v0134-p503-gate-final.log`、`/tmp/lingxi-tool-contract-v0134-p503-typecheck-final.log`、`/tmp/lingxi-tool-contract-v0134-p503-eslint-final.log`。

## P6-01 plugin-dev 聊天身份

- 状态：`completed`。
- 提交：`30f51d558a4271953f3cb890f150d1f4a4ded20d`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `0b2049015c136bd9b78df259732addd2446e072f`；非引擎文件同哈希迁移，引擎只适配开发工具装配片段。
- RED：exit `1`；2 files，`6 failed / 1 passed` tests；模型可覆盖会话身份、权限被压平、执行绕过 Gateway，缺目标未关闭。
- GREEN：exit `0`；6 files / 107 tests 全部通过。
- typecheck exit `0`；新增测试定向 ESLint exit `0`，0 问题；`git diff --check` exit `0`。
- 边界：聊天入口对 `service.invokeTool` 和 `executePluginTool` 搜索 exit `1`（0 命中）。
- 日志：`/tmp/lingxi-tool-contract-v0134-p601-red.log`、`/tmp/lingxi-tool-contract-v0134-p601-stage-final.log`、`/tmp/lingxi-tool-contract-v0134-p601-typecheck-final.log`、`/tmp/lingxi-tool-contract-v0134-p601-eslint-final.log`、`/tmp/lingxi-tool-contract-v0134-p601-chat-boundary-final.log`。

## P6-02 本地 HTTP 独立开发者身份

- 状态：`completed`。
- 提交：`e367c51f4894da81abdc55c3603c800fe591c6f9`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `6fda792303cf6919acc459cc4bad06aaef4bc702`；生产与测试文件在本项前均与原提交父节点同哈希，原样迁移。
- RED：exit `1`；3 files，`13 failed / 85 passed` tests；网关未约束本地主体，路由接受身份覆盖，开发服务仍直调且取消语义失真。
- GREEN：exit `0`；10 files / 289 tests 全部通过。
- typecheck exit `0`；parity/Gateway 定向 ESLint exit `0`，0 问题；`git diff --check` exit `0`。
- 边界：生产 `executePluginTool(` 只命中开发服务内的 Gateway source adapter 1 处和 PluginManager 方法定义 1 处。
- 日志：`/tmp/lingxi-tool-contract-v0134-p602-red.log`、`/tmp/lingxi-tool-contract-v0134-p602-gate-final.log`、`/tmp/lingxi-tool-contract-v0134-p602-typecheck-final.log`、`/tmp/lingxi-tool-contract-v0134-p602-eslint-final.log`、`/tmp/lingxi-tool-contract-v0134-p602-plugin-executor-boundary.log`。

## P7-01 统一媒体执行目标解析器

- 状态：`completed`。
- 提交：`5f62fd5ba9506a99bc559fa13ca3dd60950907c9`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `2ff451d74157678f733a2eeff111acb715d59ebd`；三个新文件原样迁移。
- RED：exit `1`；1 failed suite，0 tests；任务书要求的统一解析模块不存在。
- GREEN：exit `0`；1 file / 5 tests 全部通过。
- typecheck exit `0`；三个新增文件定向 ESLint exit `0`，0 问题；`git diff --check` exit `0`。
- 日志：`/tmp/lingxi-tool-contract-v0134-p701-red.log`、`/tmp/lingxi-tool-contract-v0134-p701-green.log`、`/tmp/lingxi-tool-contract-v0134-p701-typecheck.log`、`/tmp/lingxi-tool-contract-v0134-p701-eslint.log`。

## P7-02 统一全部媒体入口

- 状态：`completed`。
- 提交：`b78bcc7dbb13c88734c129992b306b5c80289c7c`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `7581da5ffaeb47554df2a5ebcfcf91be2b6b9944`；14 个生产文件和 13 个测试文件在本项前均与原提交父节点同哈希，原样迁移。
- RED：exit `1`；`1 failed / 1 passed` files，`5 failed / 10 passed` tests；四入口未统一、后台未重解、下游仍有凭证回退。
- GREEN：P7 定向 2 files / 15 tests；含 Hub 3 files / 23 tests；全媒体 57 files / 482 tests，均 exit `0`。
- typecheck exit `0`；全仓 lint exit `0`，`0 errors / 9228 warnings`；定向 ESLint exit `0`，`0 errors / 317 warnings`；`git diff --check` exit `0`。
- 静态边界：`credential_refresh_failed` 搜索 exit `1`（0 命中）；入口与适配器清单均指向统一执行目标和其凭证供应商字段。
- 日志：`/tmp/lingxi-tool-contract-v0134-p702-red.log`、`/tmp/lingxi-tool-contract-v0134-p702-targeted-final.log`、`/tmp/lingxi-tool-contract-v0134-p702-stage-final.log`、`/tmp/lingxi-tool-contract-v0134-p702-media-all-final.log`、`/tmp/lingxi-tool-contract-v0134-p702-typecheck-final.log`、`/tmp/lingxi-tool-contract-v0134-p702-lint-final.log`、`/tmp/lingxi-tool-contract-v0134-p702-focused-eslint-final.log`、`/tmp/lingxi-tool-contract-v0134-p702-credential-refresh-scan.log`、`/tmp/lingxi-tool-contract-v0134-p702-credential-inventory-final.log`。

## P8-01 抽取共享 rerank policy 执行器

- 状态：`completed`。
- 提交：`855ed701f9375f87f95618e9e49e083c74a326a7`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `e74aafee04c539dcc1352887e8763a741fb06ba5`；共享模块与回归测试按原实现迁入，其余连接点按 v0.1.34 正式知识架构逐处适配。
- RED：exit `1`；1 failed suite，0 tests；旧代码缺少 `lib/knowledge/rerank-policy.ts`。
- 第一次 GREEN 尝试：1 file，`1 failed / 5 passed` tests；实现已通过，唯一失败是旧测试仍要求已经被 v0.1.34 删除的引擎兼容检索分支。
- 基线适配：没有恢复生产 legacy 注入器，也没有重建退役的引擎检索分支；正式 fast 管线显式使用禁用策略，详细知识工具显式选择完整策略，引擎只提供模型执行能力。
- 最终任务书门禁：exit `0`，5 files / 96 tests 全部通过；相关知识回归：exit `0`，12 files / 48 tests 全部通过。
- typecheck 最终 exit `0`；定向 ESLint exit `0`，`0 errors / 125 warnings`（存量警告）；`git diff --check` exit `0`。
- 缓存身份使用规范化后的完整策略摘要，覆盖 channel、enabled、margin、deadline 和文档上限；两条重排路径共用同一决策与执行入口。
- 日志：`/tmp/lingxi-tool-contract-v0134-p801-red.log`、`/tmp/lingxi-tool-contract-v0134-p801-green-attempt1.log`、`/tmp/lingxi-tool-contract-v0134-p801-gate-attempt1.log`、`/tmp/lingxi-tool-contract-v0134-p801-related-attempt1.log`、`/tmp/lingxi-tool-contract-v0134-p801-typecheck-attempt3.log`、`/tmp/lingxi-tool-contract-v0134-p801-eslint-attempt1.log`。

## P9-01 完成统一错误映射

- 状态：`completed`。
- 提交：`c470e9c624f0637c99c0a664a1756a8cf09b3d4c`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `e3fe97120a411d3e8cc055bea5a8e2dd34d4f8ae`；除 `core/engine.ts` 外的生产和测试文件均先核对为原提交父树同哈希，引擎只迁移工具装配区的错误诊断片段。
- RED：exit `1`；8 failed files，`15 failed / 90 passed` tests；路径/密钥未遮蔽、schema 缺稳定路径、能力两侧事实缺失、目标缺失与拒绝混同、媒体凭证错误混同、网关无安全结构化诊断。
- 核心 GREEN：exit `0`，8 files / 105 tests；扩展门禁：exit `0`，19 files / 349 tests 全部通过。
- typecheck exit `0`；定向 ESLint exit `0`，`0 errors / 306 warnings`（存量大文件与测试风格警告）；`git diff --check` exit `0`。
- 错误边界：目标缺失、不可见、按代理禁用、撤销、能力错配、凭证缺失、刷新传输失败与取消分别保留稳定类型；模型可见消息遮蔽内部路径和密钥片段，诊断日志只携带安全归因字段。
- 日志：`/tmp/lingxi-tool-contract-v0134-p901-red.log`、`/tmp/lingxi-tool-contract-v0134-p901-gate-attempt1.log`、`/tmp/lingxi-tool-contract-v0134-p901-gate-final.log`、`/tmp/lingxi-tool-contract-v0134-p901-typecheck-attempt1.log`、`/tmp/lingxi-tool-contract-v0134-p901-eslint.log`。

## P9-02 建立 raw execution 边界检查

- 状态：`completed`。
- 提交：`c63dcf6011f9240e7f66693074d4292dbb9b8443`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `ee3ac90a6777996a6776a0fa73db83736512f313`；扫描器和测试原样迁移，`package.json` 只增加任务书指定脚本，没有覆盖 v0.1.34 其它脚本。
- RED：exit `1`；1 failed suite，0 tests；旧源码缺少任务书指定的语法树边界扫描器。
- 独立扫描 exit `0`：扫描 2129 个生产源码文件，0 违规；Vitest exit `0`：1 file / 3 tests，其中合成越界样例验证 5 类规则均能报告。
- typecheck exit `0`；新增脚本和测试定向 ESLint exit `0`、无输出；`git diff --check` exit `0`。
- allowlist 只使用精确文件路径；底层 MCP 调用、插件执行器和规范目标执行器均限制在指定来源适配器，Bridge 与 Engine 的退役旁路也由同一扫描器检查。
- 日志：`/tmp/lingxi-tool-contract-v0134-p902-red.log`、`/tmp/lingxi-tool-contract-v0134-p902-boundary-final.log`、`/tmp/lingxi-tool-contract-v0134-p902-test-final.log`、`/tmp/lingxi-tool-contract-v0134-p902-typecheck-final.log`、`/tmp/lingxi-tool-contract-v0134-p902-eslint-final.log`。

## P10-01 建立路径等价变形测试

- 状态：`completed`。
- 提交：`f0def592e3871567d09a33b597aa70c21c6252b5`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `5518ed2ee4920b81db6943788dba06b4f0e741d5`；两份生产文件在本项前与原提交父树同哈希，回归与实现原样迁移。
- RED：exit `1`；1 failed / 2 passed tests；只改变 direct、deferred、plugin-dev-chat 路由后，规范参数在权限复核后的绑定语义不一致。
- GREEN：11 files / 202 tests 全部通过；新矩阵 3/3 覆盖三条模型路线和独立的本地开发者 HTTP 主体。
- typecheck exit `0`；边界扫描 exit `0`，2129 个生产源码文件 0 违规；定向 ESLint exit `0`，`0 errors / 71 warnings`（既有会话权限包装风格警告）；`git diff --check` exit `0`。
- 等价边界：权限结论、能力、审批次数、副作用描述、真实参数、摘要、执行次数、结果与来源信息、调用标识、取消/进度信号和错误码均只受语义输入影响，不受调用路线影响。
- 日志：`/tmp/lingxi-tool-contract-v0134-p1001-red.log`、`/tmp/lingxi-tool-contract-v0134-p1001-test-attempt1.log`、`/tmp/lingxi-tool-contract-v0134-p1001-gate-final.log`、`/tmp/lingxi-tool-contract-v0134-p1001-typecheck-final.log`、`/tmp/lingxi-tool-contract-v0134-p1001-boundary.log`、`/tmp/lingxi-tool-contract-v0134-p1001-eslint.log`。

## P10-02 完整配置组合

- 状态：`completed_with_red_not_reproduced`。
- 提交：`a90cdd1f188495b9e68b025936bc2c5ae34abb9c`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `f8432e83e40331bca9ccf2e74898b9f7b0fb39b8`；本项只扩展既有路径等价测试，无生产代码修改。
- 首次矩阵：exit `0`，1 file / 12 tests 全部通过；新增的授权后参数变化、审批后禁用/代次变化、流式更新和取消组合均已被前序修复覆盖，未能复现新的旧代码失败。
- 处理：没有人为制造失败、削弱断言或扩大生产架构；按真实状态记录红灯未复现。
- 完整组合门禁：exit `0`，13 files / 373 tests 全部通过，覆盖总延迟/内置延迟开关、候选数 10/11/12、权限方言、可延迟/固定工具、代理和连接器启停、可见性、授权参数变化、审批后撤销、同名冲突、嵌套 schema、取消和流式更新。
- typecheck exit `0`；边界扫描 exit `0`，2129 个生产源码文件 0 违规；扩展测试定向 ESLint exit `0`、无输出；`git diff --check` exit `0`。
- 日志：`/tmp/lingxi-tool-contract-v0134-p1002-first-matrix.log`、`/tmp/lingxi-tool-contract-v0134-p1002-gate.log`、`/tmp/lingxi-tool-contract-v0134-p1002-typecheck.log`、`/tmp/lingxi-tool-contract-v0134-p1002-boundary.log`、`/tmp/lingxi-tool-contract-v0134-p1002-eslint.log`。

## P11-01 架构说明

- 状态：`completed`。
- 提交：`29a296611a1da1509671f819cf0032dd72937eb2`，远端分支已核对同 SHA。
- 复用来源：原分支提交 `a6372e50669151df69683ed257661d2443d7429a`；架构说明和文档契约测试按原实现迁入，事实坐标以 v0.1.34 校正版分支为准。
- RED：exit `1`；1 file，`1 failed / 3 passed` tests；旧代码缺少任务书指定的架构说明文件，原始错误为 `ENOENT`。
- GREEN：exit `0`；1 file / 4 tests 全部通过；任务书要求的九个章节标题逐项命中。
- typecheck exit `0`；边界扫描 exit `0`，2129 个生产源码文件 0 违规；测试定向 ESLint exit `0`、无输出；`git diff --check` exit `0`。
- 文档明确了业务目标与能力、显示名、目录名的分工，完整执行顺序，四种路线共用同一受控入口，绑定后的调用内容，允许触碰底层执行器的来源边界，稳定错误码、新工具接入清单和禁止做法。
- 日志：`/tmp/lingxi-tool-contract-v0134-p1101-red.log`、`/tmp/lingxi-tool-contract-v0134-p1101-gate.log`、`/tmp/lingxi-tool-contract-v0134-p1101-boundary.log`、`/tmp/lingxi-tool-contract-v0134-p1101-typecheck.log`、`/tmp/lingxi-tool-contract-v0134-p1101-eslint.log`、`/tmp/lingxi-tool-contract-v0134-p1101-sections.log`。

## P11-02 修复报告与机器事实

- 状态：`completed`。
- 提交：`c217a04b3a6f33146cd5483cbaf4aed7715891c3`，已推送并核对远端一致；该提交曾作为首个源码候选，后被 P12-02 边界门禁作废。
- 复用来源：原分支提交 `931543baedacca62417ef9d4a517d1b9857c9abd` 只作为结构模板；所有基线、分支、统计、日志和适配结论均按 v0.1.34 校正版现场重建。
- RED：exit `1`；1 file，`1 failed / 4 passed` tests；四份任务书指定报告尚不存在，原始错误为 `ENOENT`。
- GREEN：exit `0`；1 file / 5 tests 全部通过；机器事实 JSON 独立解析 exit `0`。
- typecheck exit `0`；边界扫描 exit `0`，2129 个生产源码文件 0 违规；测试定向 ESLint exit `0`、无输出；`git diff --check` exit `0`。
- `sourceCandidateSha` 和 `sealSha` 按任务书保持 `null`；真实坐标只写 `PROGRESS.md` 和最终执行报告，避免提交自引用。
- 日志：`/tmp/lingxi-tool-contract-v0134-p1102-red.log`、`/tmp/lingxi-tool-contract-v0134-p1102-gate.log`、`/tmp/lingxi-tool-contract-v0134-p1102-boundary.log`、`/tmp/lingxi-tool-contract-v0134-p1102-typecheck.log`、`/tmp/lingxi-tool-contract-v0134-p1102-eslint.log`。

## P12 首次验证与 P12-02 当前项修复

- 首个源码候选：`c217a04b3a6f33146cd5483cbaf4aed7715891c3`，验证开始前工作树干净且远端一致。
- P12-01 在该候选上通过：底层执行边界扫描 2129 个生产源码文件、0 违规；指定 25 文件 / 389 测试全部通过。
- P12-02 已通过部分：typecheck exit `0`；lint exit `0`，`0 errors / 9231 warnings`。
- P12-02 原始失败：`lint:boundary` exit `1`，发现 26 条本次规范模块尚未登记到开放清单的跨层连接，另有 1 条既有债务；因此没有继续运行本轮全量测试或 P12-03。
- 当前项修复：只向 `export-manifest.json` 精确加入 6 个本次新增模块，未增加目录级通配豁免；重新生成运行闭包为 11005 文件（源码图 796、运行资源 11、依赖追踪 10198），开放边界仍只有 1 条既有债务。
- 修复后 `lint:boundary` exit `0`；开放边界与闭包回归 2 files / 39 tests 全部通过；`git diff --check` exit `0`。
- 处理：首个源码候选已作废；本项提交后形成新源码候选，并严格从 P12-01 重跑全部门禁。
- 日志：`/tmp/lingxi-tool-contract-p1201-boundary.log`、`/tmp/lingxi-tool-contract-p1201-targeted.log`、`/tmp/lingxi-tool-contract-p1202-typecheck.log`、`/tmp/lingxi-tool-contract-p1202-lint.log`、`/tmp/lingxi-tool-contract-p1202-open-boundary.log`、`/tmp/lingxi-tool-contract-v0134-p1202-closure-regenerate.log`、`/tmp/lingxi-tool-contract-v0134-p1202-open-boundary-fix.log`、`/tmp/lingxi-tool-contract-v0134-p1202-boundary-regression.log`。

## P12 第二个候选与 P12-02 全量测试修复

- 第二个源码候选：`df8e19b9906654c0de0b2b867ea14363ae4a7843`，已推送并核对远端一致；验证开始前工作树干净。
- P12-01 复跑：底层执行边界扫描 2129 个生产源码文件、0 违规；指定 25 文件 / 389 测试全部通过。
- P12-02 静态门禁：typecheck exit `0`；lint exit `0`，`0 errors / 9231 warnings`；开放边界 exit `0`，仅 1 条既有债务；`git diff --check` exit `0`。
- P12-02 全量原始结果：exit `1`；`1371 passed / 3 failed / 1 skipped` files，`13897 passed / 7 failed / 7 skipped` tests。
- 失败分解：持久化指纹 4 项；媒体可观测性夹具 2 项；旧审计封印 1 项。skip 仍为 7，没有增加。
- 当前项已修的 6 项：以 `compatible` 理由重新钉住持久化指纹 `sha256:8154c7ec6b44430a91c9b1fc2d6bb8662d6eee1753ddb9702642d7566163222d`；两套媒体观测夹具补入规范执行目标，不恢复凭证回退。
- 修复门禁：相关 2 files / 26 tests 全部通过；三段 typecheck exit `0`；定向 ESLint exit `0`，`0 errors / 28 warnings`，均为该旧测试文件既有警告；`git diff --check` exit `0`。
- 仍待复核：审计封印测试依赖 P12-04 才允许推进的已验证源码坐标。不会提前改坐标、跳过测试或放宽 allowlist；先形成第三个源码候选并从 P12-01 重跑，确认是否只剩该顺序循环。
- 日志：`/tmp/lingxi-tool-contract-p1202-full-tests.log`、`/tmp/lingxi-tool-contract-v0134-p1202-persistence-repin.log`、`/tmp/lingxi-tool-contract-v0134-p1202-regression-fix.log`、`/tmp/lingxi-tool-contract-v0134-p1202-repair-typecheck.log`、`/tmp/lingxi-tool-contract-v0134-p1202-repair-eslint.log`。

## 错误记录

| 时间 | 编号 | 原始错误 | 处理 |
|---|---|---|---|
| 2026-09-05 | 基线审计 | 原任务固定在 v0.1.34 发布前阶段提交 | 保留旧分支，以正式发布提交建立校正版分支 |
| 2026-09-05 | `P12_SEQUENCE_SEAL_GATE_CYCLE` | P0 新增任务记录使全量套件中的旧封印测试失败 | 保留 fail-closed 与原始失败；P12 按任务书固定新源码坐标 |
| 2026-09-05 | `BUILD_SIGN_KEY_MISSING` | 原始 `build:server` 缺少签名密钥，exit `1` | 抛弃式匹配密钥诊断复跑 exit `0`；临时材料已删除 |
| 2026-09-05 | P7-02 提交整理 | 首次提交暂存漏含 `core/media-adapters/` 下 7 个本项文件，提交尚未推送 | 推送前回读工作树发现并补入同一提交；未拆项、未丢改动 |
| 2026-09-05 | `P12_OPEN_BOUNDARY_MANIFEST_DRIFT` | 首个源码候选的开放边界门禁发现 26 条新增连接未登记 | 只补 6 个精确开放模块并重生成闭包；原候选作废，新候选后从 P12-01 重跑 |
| 2026-09-05 | `P12_FULL_SUITE_CONTRACT_DRIFT` | 第二个候选全量测试有 4 项指纹、2 项媒体夹具和 1 项旧封印失败 | 当前项内修复前 6 项；旧封印顺序循环保持原样，第三候选再全量复核 |
