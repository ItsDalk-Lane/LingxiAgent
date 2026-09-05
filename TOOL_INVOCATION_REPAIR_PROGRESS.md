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
| P4-02 | `completed_pending_commit` | 待提交 | Bridge 只经 Gateway 调用，20 项红灯转绿 |
| P4-03–P7 | `pending_migration` | — | 优先复用原实现 |
| P8 | `pending_v0134_adaptation` | — | 知识正式架构发生变化，需重点适配 |
| P9–P11 | `pending_migration` | — | 迁移错误、边界、组合测试与文档 |
| P12 | `pending` | — | 最终验证、构建和封印 |

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

- 状态：`completed_pending_commit`。
- 复用来源：原分支提交 `40c4db7a95b5db58f41dead8d4d5ea044f8190d6`；Bridge 与 Gateway 同哈希原样迁移，引擎只适配桥接依赖和能力委托片段。
- RED：exit `1`；1 file，`20 failed / 14 passed` tests；旧路径没有调用 Gateway，自行拼权限、吞掉类型化错误、只做浅层校验且不能处理来源歧义。
- GREEN：exit `0`；6 files / 146 tests 全部通过。
- typecheck exit `0`；本项定向 ESLint exit `0`，`0 errors / 140 warnings`；`git diff --check` exit `0`。
- v0.1.34 适配证明：引擎只删除临时的 raw Bridge 闭包并改接已注册的统一入口，没有覆盖正式知识架构或其它引擎区域。
- 日志：`/tmp/lingxi-tool-contract-v0134-p402-red.log`、`/tmp/lingxi-tool-contract-v0134-p402-stage-final.log`、`/tmp/lingxi-tool-contract-v0134-p402-typecheck-final.log`、`/tmp/lingxi-tool-contract-v0134-p402-eslint-final.log`。

## 错误记录

| 时间 | 编号 | 原始错误 | 处理 |
|---|---|---|---|
| 2026-09-05 | 基线审计 | 原任务固定在 v0.1.34 发布前阶段提交 | 保留旧分支，以正式发布提交建立校正版分支 |
| 2026-09-05 | `P12_SEQUENCE_SEAL_GATE_CYCLE` | P0 新增任务记录使全量套件中的旧封印测试失败 | 保留 fail-closed 与原始失败；P12 按任务书固定新源码坐标 |
| 2026-09-05 | `BUILD_SIGN_KEY_MISSING` | 原始 `build:server` 缺少签名密钥，exit `1` | 抛弃式匹配密钥诊断复跑 exit `0`；临时材料已删除 |
