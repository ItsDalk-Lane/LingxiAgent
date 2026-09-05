# 工具契约执行路径不变量修复进度

## 执行约束

- 唯一实施依据：`/Users/study_superior/Downloads/LingxiAgent 契约执行架构“语义与路径无关”全链路修复执行任务书.md`
- 固定基线：`4fefe66ec3b4f6b23c78a09869a607886585740e`
- 执行分支：`fix/tool-contract-path-invariance`
- 顺序：严格按 P0-00 → P12-06 推进。
- 每个实现项遵守：先补回归测试并确认旧实现按预期失败，再实现、运行门禁、记录原始结果与提交 SHA、推送后进入下一项。
- 任务书本身承担任务计划职责；本文件承担进度与错误日志职责；`TOOL_INVOCATION_REPAIR_BASELINE.md` 承担现状事实记录职责。保留仓库已有其他任务的 `task_plan.md`、`findings.md` 和 `PROGRESS.md`，不覆盖。

## 当前任务

### P0-00 固定 Git 基线并创建执行分支

- 状态：`completed`
- 改动文件：`TOOL_INVOCATION_REPAIR_BASELINE.md`、`TOOL_INVOCATION_REPAIR_PROGRESS.md`
- 测试命令：不适用；本项为基线、分支与依赖准备。
- 原始结果：`git fetch origin --prune` exit `0`；远端来源引用核对 exit `1`；固定提交可达；`npm ci` exit `0`。
- 日志路径：`/tmp/lingxi-tool-contract-p000-setup.log`
- 提交 SHA：`8b26d7f69a1f35d56e1ff0b874408cbfea707c3a`
- 偏差：远端来源分支已不存在；按任务书规则从仍可达的固定 SHA 建分支。

### P0-01 运行基线门禁

- 状态：`completed_with_baseline_failures`
- 改动文件：`TOOL_INVOCATION_REPAIR_BASELINE.md`、`TOOL_INVOCATION_REPAIR_PROGRESS.md`
- 测试命令：任务书 P0-01 列出的 typecheck、lint、11 文件定向 Vitest、全量测试、服务构建、`git diff --check`；另用临时抛弃式签名材料诊断复跑服务构建。
- 原始结果：typecheck exit `0`；lint exit `0`（`0 errors / 9188 warnings`）；定向 `11 files / 251 tests passed`，exit `0`；全量 `1331 passed / 2 failed / 1 skipped` files、`13432 passed / 2 failed / 7 skipped` tests，exit `1`；服务构建首次 exit `1`，临时签名诊断复跑 exit `0`；`git diff --check` exit `0`。
- 日志路径：`/tmp/lingxi-tool-contract-p001-typecheck.log`、`/tmp/lingxi-tool-contract-p001-lint.log`、`/tmp/lingxi-tool-contract-p001-targeted.log`、`/tmp/lingxi-tool-contract-p001-full.log`、`/tmp/lingxi-tool-contract-p001-build-server.log`、`/tmp/lingxi-tool-contract-p001-build-server-diagnostic.log`
- 提交 SHA：`179819092562f5c1d063baff56ada6486e340c1e`
- 偏差：全量测试有两项既有/封印状态红灯；无签名环境下服务构建红灯。均保留原始失败，未修改代码、测试、版本或审计规则；完整归因见基线文件。

### P0-02 建立现状调用矩阵和原始入口清单

- 状态：`completed`
- 改动文件：`TOOL_INVOCATION_REPAIR_BASELINE.md`、`TOOL_INVOCATION_REPAIR_PROGRESS.md`
- 测试命令：任务书列出的六条 `rg` 搜索；本项是只读审计，不适用旧代码失败回归。
- 原始结果：六条搜索全部 exit `0`；12 个 bundled plugin 工具为 `7` 个 readOnly、`5` 个副作用；确认 `1` 个生产 PluginManager raw 执行入口、`6` 类生产 MCP `callTool` 位置、旧知识入口无外部生产调用方、四套媒体凭证回退链、Catalog 平面名称与浅层 schema 行为。
- 日志路径：`/tmp/lingxi-tool-contract-p002-bridge-entrypoints.log`、`/tmp/lingxi-tool-contract-p002-plugin-executors.log`、`/tmp/lingxi-tool-contract-p002-mcp-calltool.log`、`/tmp/lingxi-tool-contract-p002-permission-dialects.log`、`/tmp/lingxi-tool-contract-p002-media-credentials.log`、`/tmp/lingxi-tool-contract-p002-knowledge-rerank.log`，以及基线文件列出的 5 份补充明细日志。
- 提交 SHA：`479f17145d5f5ef5d00c2718900eb04b27dba2ce`
- 偏差：一次读取命令错误假设插件根入口文件存在，已改按真实 manifest/tools 路径读取并记录；无生产代码修改。

## 后续任务

| 编号 | 状态 | 提交 SHA | 备注 |
| --- | --- | --- | --- |
| P0-01 | completed_with_baseline_failures | `179819092562f5c1d063baff56ada6486e340c1e` | 基线门禁；3 类真实红灯已归因 |
| P0-02 | completed | `479f17145d5f5ef5d00c2718900eb04b27dba2ce` | 现状矩阵与入口清单 |
| P1-01 | completed | `6ed37ec467c4a5bdfc567cdcd552cc1dbe04ee6a` | 目标身份、路由和错误类型 |
| P1-02 | completed | `76afb23903e19df1745bdac1f9146d922edd0027` | 新旧权限方言规范化 |
| P1-03 | completed | `d2f73f21f48a466b19b2d76a6e37e2154b6093b2` | 完整 schema 校验器 |
| P2-01 | completed | `28e097137dc4f02da63c9047fd1d6d5b67515d07` | 会话级目标注册表 |
| P2-02 | completed | `1e71a29a272d2292c003293e123dde681cdb6a52` | PreparedInvocation 与统一网关 |
| P3-01 | completed | `82292ec6f200e4729245d6bc71589b64ba0a2379` | 插件元数据与 target adapter |
| P3-02 | completed | `ae56984375dbf39ff462f363d9c5512fbc2a32de` | Engine 装配顺序 |
| P4-01 | completed | `95e5377c0e693599c12b9fb47df9026e04126c28` | Catalog 目标引用目录 |
| P4-02 | completed | `40c4db7a95b5db58f41dead8d4d5ea044f8190d6` | Bridge Gateway 适配 |
| P4-03 | completed | `ae40059d531bad737c1427cb232e7e8fcf7d03ba` | MCP eligibility 与执行器 |
| P5-01 | completed | `30eb7d7c5eeb4f9b8d455961ef8bab7104adccf7` | Plugin 工具代次 |
| P5-02 | completed | `aede651699e2e6ce6b71bcd73ac62607ec9dd1a4` | MCP live generation |
| P5-03 | completed | 待本项提交后回填 | 旧会话撤销语义 |
| P6-01 | pending | — | plugin-dev 聊天身份 |
| P6-02 | pending | — | LocalDeveloperPrincipal |
| P7-01 | pending | — | 媒体执行目标解析器 |
| P7-02 | pending | — | 媒体入口统一 |
| P8-01 | pending | — | rerank policy 共享执行器 |
| P9-01 | pending | — | 统一错误映射 |
| P9-02 | pending | — | raw execution 边界检查 |
| P10-01 | pending | — | 路径等价变形测试 |
| P10-02 | pending | — | 完整配置组合 |
| P11-01 | pending | — | 架构文档 |
| P11-02 | pending | — | 报告、机器事实、源码候选 |
| P12-01 | pending | — | 定向契约门禁 |
| P12-02 | pending | — | 静态与全量测试门禁 |
| P12-03 | pending | — | 构建门禁 |
| P12-04 | pending | — | 固定源码 SHA 与审计投影 |
| P12-05 | pending | — | 封印后门禁 |
| P12-06 | pending | — | 推送与最终报告 |

### P1-01 建立目标身份、路由和错误类型

- 状态：`completed`
- 改动文件：`lib/tools/invocation/types.ts`、`lib/tools/invocation/identity.ts`、`lib/tools/invocation/errors.ts`、`lib/tools/invocation/index.ts`、`tests/tool-target-identity.test.ts`、`tests/tool-invocation-errors.test.ts`、`TOOL_INVOCATION_REPAIR_PROGRESS.md`
- 红灯命令：`npx vitest run tests/tool-target-identity.test.ts tests/tool-invocation-errors.test.ts`
- 红灯原始结果：exit `1`；`2 failed` suites，`0` tests executed；两套测试均因 `lib/tools/invocation/index.ts` 不存在而加载失败，符合旧实现缺少规范化内核的预期原因。
- 红灯日志：`/tmp/lingxi-tool-contract-p101-red.log`
- 绿灯命令：同一定向 Vitest、`npm run typecheck`、对本项 6 个 TypeScript 文件执行定向 ESLint、`git diff --check`
- 绿灯原始结果：定向 `2 passed` files、`6 passed` tests、exit `0`；三段 typecheck exit `0`；定向 ESLint 最终 `0` 问题、exit `0`；`git diff --check` exit `0`。
- 绿灯日志：`/tmp/lingxi-tool-contract-p101-targeted.log`、`/tmp/lingxi-tool-contract-p101-typecheck.log`、`/tmp/lingxi-tool-contract-p101-eslint.log`
- 提交 SHA：`6ed37ec467c4a5bdfc567cdcd552cc1dbe04ee6a`
- 偏差：none

### P1-02 统一新旧权限方言

- 状态：`completed`
- 改动文件：`lib/tools/invocation/permission-adapter.ts`、`lib/tools/invocation/index.ts`、`lib/permission/tool-invocation-permission.ts`、`lib/tools/session-permission-wrapper.ts`、`core/plugin-manager.ts`、`tests/tool-permission-adapter.test.ts`、`tests/plugin-manager.test.ts`、`TOOL_INVOCATION_REPAIR_PROGRESS.md`。
- 红灯命令：`npx vitest run tests/tool-permission-adapter.test.ts`
- 红灯原始结果：exit `1`；`1 failed` suite、`0` tests executed；因 `lib/tools/invocation/permission-adapter.ts` 不存在而加载失败，符合旧实现缺少注册期统一适配器的预期原因。
- 红灯日志：`/tmp/lingxi-tool-contract-p102-red.log`
- 中间回归命令：`npx vitest run tests/tool-permission-adapter.test.ts tests/tool-invocation-permission.test.ts tests/session-permission-wrapper.test.ts tests/plugin-manager.test.ts tests/plugin-runtime.test.ts`
- 中间回归原始结果：首次 exit `1`，`3 passed / 2 failed` files、`154 passed / 30 failed` tests；失败均来自测试夹具缺少显式权限声明，以及两条 legacy 直接路径语义/再次校验不一致。修正后同命令 exit `0`，`5 passed` files、`185 passed` tests。
- 中间回归日志：`/tmp/lingxi-tool-contract-p102-affected.log`、`/tmp/lingxi-tool-contract-p102-affected-rerun.log`
- 绿灯命令：上述 5 文件受影响 Vitest；`npm run typecheck`；对本项新增文件执行定向 ESLint；`git diff --check`。
- 绿灯原始结果：Vitest exit `0`，`5 passed` files、`187 passed` tests；三段 typecheck exit `0`；新增文件 ESLint exit `0`、`0` 问题；全部改动文件 ESLint exit `0`（`0 errors / 229 warnings`，均为既有文件存量风格警告）；`git diff --check` exit `0`。
- 绿灯日志：`/tmp/lingxi-tool-contract-p102-final-tests.log`、`/tmp/lingxi-tool-contract-p102-typecheck-final.log`、`/tmp/lingxi-tool-contract-p102-eslint-final.log`、`/tmp/lingxi-tool-contract-p102-eslint.log`
- 提交 SHA：`76afb23903e19df1745bdac1f9146d922edd0027`
- 偏差：none

### P1-03 建立完整 schema 校验器

- 状态：`completed`
- 改动文件：`lib/tools/invocation/schema-validator.ts`、`lib/tools/invocation/index.ts`、`tests/tool-schema-validator.test.ts`、`TOOL_INVOCATION_REPAIR_PROGRESS.md`。
- 红灯命令：`set -o pipefail; npx vitest run tests/tool-schema-validator.test.ts 2>&1 | tee /tmp/lingxi-tool-contract-p103-red.log`
- 红灯原始结果：exit `1`；`1 failed` file、`8 failed` tests；旧实现不存在 `createToolSchemaValidator`，符合缺少完整参数校验器的预期原因。
- 红灯日志：`/tmp/lingxi-tool-contract-p103-red.log`
- 绿灯命令：任务书 P1 阶段 8 文件 Vitest；`npm run typecheck`；P1-03 新增文件定向 ESLint；`git diff --check`。
- 绿灯原始结果：阶段 Vitest exit `0`，`8 passed` files、`202 passed` tests；三段 typecheck exit `0`；定向 ESLint exit `0`、`0` 问题；`git diff --check` exit `0`。
- 绿灯日志：`/tmp/lingxi-tool-contract-p1-stage.log`、`/tmp/lingxi-tool-contract-p103-typecheck.log`、`/tmp/lingxi-tool-contract-p103-eslint.log`
- 提交 SHA：`d2f73f21f48a466b19b2d76a6e37e2154b6093b2`
- 偏差：none

### P2-01 实现会话级 ToolTargetRegistry

- 状态：`completed`
- 改动文件：`core/tool-target-registry.ts`、`tests/tool-target-registry.test.ts`、`TOOL_INVOCATION_REPAIR_PROGRESS.md`。
- 红灯命令：`set -o pipefail; npx vitest run tests/tool-target-registry.test.ts 2>&1 | tee /tmp/lingxi-tool-contract-p201-red.log`
- 红灯原始结果：exit `1`；`1 failed` suite、`0` tests；旧实现缺少 `core/tool-target-registry.ts`，符合预期。
- 红灯日志：`/tmp/lingxi-tool-contract-p201-red.log`
- 绿灯命令：`npx vitest run tests/tool-target-registry.test.ts`、`npm run typecheck`、本项文件定向 ESLint、`git diff --check`。
- 绿灯原始结果：Vitest exit `0`，`1 passed` file、`6 passed` tests；三段 typecheck exit `0`；定向 ESLint exit `0`、`0` 问题；`git diff --check` exit `0`。
- 绿灯日志：`/tmp/lingxi-tool-contract-p201-final-tests.log`、`/tmp/lingxi-tool-contract-p201-typecheck-final.log`、`/tmp/lingxi-tool-contract-p201-eslint-final.log`
- 提交 SHA：`28e097137dc4f02da63c9047fd1d6d5b67515d07`
- 偏差：none

### P2-02 建立 PreparedInvocation 与统一调用网关

- 状态：`completed`
- 改动文件：`core/tool-invocation-gateway.ts`、`lib/tools/invocation/prepared-invocation-context.ts`、`lib/tools/invocation/index.ts`、`lib/permission/tool-invocation-permission.ts`、`lib/tools/session-permission-wrapper.ts`、`tests/tool-invocation-gateway.test.ts`、`tests/tool-invocation-permission.test.ts`、`tests/session-permission-wrapper.test.ts`、`TOOL_INVOCATION_REPAIR_PROGRESS.md`。
- 红灯命令：`set -o pipefail; npx vitest run tests/tool-invocation-gateway.test.ts 2>&1 | tee /tmp/lingxi-tool-contract-p202-red.log`；随后对 effective invocation 绑定执行 `set -o pipefail; npx vitest run tests/tool-invocation-permission.test.ts tests/session-permission-wrapper.test.ts 2>&1 | tee /tmp/lingxi-tool-contract-p202-effective-red.log`。
- 红灯原始结果：首次 exit `1`，`1 failed` suite、`0` tests，旧实现缺少统一网关模块；第二次 exit `1`，`2 failed` files、`3 failed / 78 passed` tests，旧权限描述拒绝真实目标字段、包装层未建立准备上下文且未按真实目标触发审批，均符合预期。
- 绿灯命令：任务书 P2 阶段 4 文件 Vitest；`npm run typecheck`；本项 3 个新增 TypeScript 文件定向 ESLint；`git diff --check`。
- 绿灯原始结果：阶段 Vitest exit `0`，`4 passed` files、`101 passed` tests；三段 typecheck exit `0`；新增文件定向 ESLint exit `0`、`0` 问题；`git diff --check` exit `0`。
- 绿灯日志：`/tmp/lingxi-tool-contract-p2-stage-final.log`、`/tmp/lingxi-tool-contract-p202-typecheck-final.log`、`/tmp/lingxi-tool-contract-p202-new-eslint-final.log`
- 提交 SHA：`1e71a29a272d2292c003293e123dde681cdb6a52`
- 偏差：none

### P3-01 保留插件元数据并建立插件 target adapter

- 状态：`completed`
- 改动文件：`core/plugin-manager.ts`、`core/tool-availability.ts`、`lib/permission/tool-invocation-permission.ts`、`lib/tools/session-permission-wrapper.ts`、`tests/plugin-manager.test.ts`、`tests/engine-tool-defer.test.ts`、`TOOL_INVOCATION_REPAIR_PROGRESS.md`。
- 红灯命令：`set -o pipefail; npx vitest run tests/plugin-manager.test.ts tests/plugin-runtime.test.ts tests/engine-tool-defer.test.ts 2>&1 | tee /tmp/lingxi-tool-contract-p301-red.log`
- 红灯原始结果：exit `1`；`1 passed / 2 failed` files、`117 passed / 4 failed` tests。新增断言证明静态插件包装丢失 `label`、`deferrable`、`pinned`，且缺少统一的带原因可用性判定入口；同一门禁还暴露了宿主委托资格在权限包装复制后失联，已授权的真实桥接目标没有执行。
- 中间回归：首次实现后 `2 passed / 1 failed` files、`120 passed / 1 failed` tests；只余对象身份委托失联。保留宿主登记对象后复跑通过。
- 绿灯命令：上述 3 文件加 `tests/tool-availability.test.ts`、`tests/session-coordinator-tool-snapshot.test.ts`、`tests/tool-invocation-permission.test.ts`、`tests/session-permission-wrapper.test.ts`；`npm run typecheck`；本项改动文件定向 ESLint；`git diff --check`。
- 绿灯原始结果：Vitest exit `0`，`7 passed` files、`265 passed` tests；三段 typecheck exit `0`；定向 ESLint exit `0`（`0 errors / 266 warnings`，均为存量大文件风格警告）；`git diff --check` exit `0`。
- 绿灯日志：`/tmp/lingxi-tool-contract-p301-affected-final.log`、`/tmp/lingxi-tool-contract-p301-typecheck.log`、`/tmp/lingxi-tool-contract-p301-eslint.log`
- 提交 SHA：`82292ec6f200e4729245d6bc71589b64ba0a2379`
- 偏差：任务书没有为 P3-01 单列提交信息；按用户“每项提交并推送”的要求使用与本项内容一致的独立提交信息。门禁暴露的宿主对象身份回归属于本项装配适配边界，做最小修复并保留 fail-closed。

### P3-02 重排 Engine 工具装配顺序

- 状态：`completed`
- 改动文件：`core/engine.ts`、`core/tool-catalog-bridge.ts`、`core/tool-invocation-gateway.ts`、`lib/tools/session-permission-wrapper.ts`、`tests/tool-deferred-builtin-parity.test.ts`、`tests/engine-tool-defer.test.ts`、`tests/engine-build-tools.test.ts`、`TOOL_INVOCATION_REPAIR_PROGRESS.md`。
- 红灯命令：`set -o pipefail; npx vitest run tests/tool-deferred-builtin-parity.test.ts 2>&1 | tee /tmp/lingxi-tool-contract-p302-red.log`
- 红灯原始结果：exit `1`；`1 failed` file、`6 failed / 6 total` tests。旧装配把禁用插件计入阈值、忽略 `pinned`、延迟路径丢失真实 capability/调用句柄，12 个 bundled 契约无法执行，并保留 `builtinToolsByName` 原始对象执行旁路。
- 中间回归：首次实现为 `4 passed / 2 failed`，prepared route 仍按 direct 绑定；修正路由后为 `5 passed / 1 failed`，同本名跨插件 capability 解析歧义；改为用 effective TargetId 对权威 capability 做精确复核后新矩阵通过。扩展至既有 Engine 门禁时先发现 4 个旧夹具缺规范化插件身份/权限，补齐夹具后又发现 2 个断言仍期待把普通上下文冒充 signal；按 canonical executor 契约更新为第三参数只接受真实取消信号。
- 绿灯命令：11 个相关测试文件（插件加载、SDK、Engine 两套 build、延迟装配、新增 bundled parity、可用性、注册表、网关、权限描述与会话包装）；`npm run typecheck`；新增测试文件定向 ESLint；`git diff --check`。
- 绿灯原始结果：Vitest exit `0`，`11 passed` files、`267 passed` tests；三段 typecheck exit `0`；新增测试 ESLint exit `0`、`0` 问题；`git diff --check` exit `0`。
- 绿灯日志：`/tmp/lingxi-tool-contract-p302-stage-final.log`、`/tmp/lingxi-tool-contract-p302-typecheck-final.log`、`/tmp/lingxi-tool-contract-p302-new-eslint-final.log`
- 提交 SHA：`ae56984375dbf39ff462f363d9c5512fbc2a32de`
- 偏差：none

### P4-01 重构 ToolCatalog 为目标引用目录

- 状态：`completed`
- 改动文件：`core/tool-catalog.ts`、`core/tool-catalog-bridge.ts`、`core/engine.ts`、`tests/tool-catalog-bridge.test.ts`、`tests/tool-target-registry.test.ts`；并为严格新输入契约同步更新既有 `tests/tool-catalog.test.ts`、`tests/session-reference-block.test.ts` 夹具；`TOOL_INVOCATION_REPAIR_PROGRESS.md`。
- 红灯命令：`set -o pipefail; npx vitest run tests/tool-catalog-bridge.test.ts tests/tool-target-registry.test.ts 2>&1 | tee /tmp/lingxi-tool-contract-p401-red.log`
- 红灯原始结果：exit `1`；`2 failed` files、`3 failed / 31 passed` tests。旧目录没有 `resolveTarget`，并会接受重复 TargetId，符合预期失败原因。
- 绿灯命令：目录、桥接、注册表、会话目录变更、Engine 延迟装配与 bundled parity 共 7 文件 Vitest；`npm run typecheck`；新目录模型与定向测试 ESLint；`git diff --check`。
- 绿灯原始结果：Vitest exit `0`，`7 passed` files、`119 passed` tests；三段 typecheck exit `0`；定向 ESLint exit `0`、`0` 问题；`git diff --check` exit `0`。
- 绿灯日志：`/tmp/lingxi-tool-contract-p401-stage-final.log`、`/tmp/lingxi-tool-contract-p401-typecheck-final.log`、`/tmp/lingxi-tool-contract-p401-new-eslint-final.log`
- 提交 SHA：`95e5377c0e693599c12b9fb47df9026e04126c28`
- 偏差：任务书将 MCP Manager 的规范目标生产归入 P4-03，本项未提前修改 Manager；由 Engine 在目录装配边界把既有 MCP 目录行转换为规范目标引用。严格输入契约要求同步更新两个既有目录测试夹具，无生产范围扩张。

### P4-02 把 Bridge 改成 Gateway 适配器

- 状态：`completed`
- 改动文件：`core/tool-catalog-bridge.ts`、`core/tool-invocation-gateway.ts`、`core/engine.ts`、`tests/tool-catalog-bridge.test.ts`、`tests/tool-deferred-builtin-parity.test.ts`、`TOOL_INVOCATION_REPAIR_PROGRESS.md`。
- 红灯命令：`set -o pipefail; npx vitest run tests/tool-catalog-bridge.test.ts 2>&1 | tee /tmp/lingxi-tool-contract-p402-red.log`
- 红灯原始结果：exit `1`；`1 failed` file、`6 failed / 27 passed` tests。旧桥接不调用 Gateway、自己拼权限、吞掉类型化错误、描述不支持来源消歧且仍声称目录仅含外部工具，符合预期。
- 绿灯命令：Bridge、bundled deferred parity、Gateway、完整 schema、权限描述和会话包装共 6 文件 Vitest；`npm run typecheck`；新增 Gateway 方法定向 ESLint；`git diff --check`。
- 绿灯原始结果：Vitest exit `0`，`6 passed` files、`146 passed` tests；三段 typecheck exit `0`；定向 ESLint exit `0`、`0` 问题；`git diff --check` exit `0`。
- 绿灯日志：`/tmp/lingxi-tool-contract-p402-stage-final.log`、`/tmp/lingxi-tool-contract-p402-typecheck-final.log`、`/tmp/lingxi-tool-contract-p402-new-eslint-final.log`
- 提交 SHA：`40c4db7a95b5db58f41dead8d4d5ea044f8190d6`
- 偏差：按编号边界没有提前实现 P4-03 的 MCP target descriptor。额外前置诊断 `tests/engine-tool-defer.test.ts` 为 `17 passed / 1 failed`，失败点是 MCP 目标尚未进入 Registry（日志 `/tmp/lingxi-tool-contract-p402-engine-transition-check.log`）；该失败不是 P4-02 Bridge 单元门禁，提交后会作为 P4-03 旧代码红灯重新验证并修复，未恢复任何 raw 直连。

### P4-03 统一 MCP eligibility、执行器与结果语义

- 状态：`completed`
- 改动文件：`core/mcp/manager.ts`、`core/engine.ts`、`core/tool-target-registry.ts`、`core/tool-invocation-gateway.ts`、`tests/engine-tool-defer.test.ts`、`tests/tool-catalog-bridge.test.ts`、`tests/mcp-runtime.test.ts`、`tests/tool-deferred-mcp-parity.test.ts`、`TOOL_INVOCATION_REPAIR_PROGRESS.md`。
- 红灯命令：`set -o pipefail; npx vitest run tests/tool-deferred-mcp-parity.test.ts tests/engine-tool-defer.test.ts tests/tool-catalog-bridge.test.ts 2>&1 | tee /tmp/lingxi-tool-contract-p403-red.log`
- 红灯原始结果：exit `1`；`1 passed / 2 failed` files、`51 passed / 10 failed` tests。旧实现缺少唯一 MCP eligibility、Manager target descriptor，MCP 目标没有进入统一 Registry/Gateway，符合预期。
- 绿灯命令：任务书 P4 阶段 6 文件 Vitest；MCP runtime、注册表、网关和 Engine build 4 文件扩展 Vitest；`npm run typecheck`；新增测试文件定向 ESLint；Engine/Bridge raw MCP 调用边界扫描；`git diff --check`。
- 绿灯原始结果：P4 阶段 Vitest exit `0`，`6 passed` files、`155 passed` tests；扩展 Vitest exit `0`，`4 passed` files、`180 passed` tests；三段 typecheck exit `0`；新增测试 ESLint exit `0`、`0` 问题；Engine/Bridge raw MCP 调用命中 `0`，底层 `.callTool(` 清单只剩 Manager 适配和协议客户端；`git diff --check` exit `0`。
- 绿灯日志：`/tmp/lingxi-tool-contract-p403-gate-final4.log`、`/tmp/lingxi-tool-contract-p403-affected-final.log`、`/tmp/lingxi-tool-contract-p403-typecheck-final4.log`、`/tmp/lingxi-tool-contract-p403-new-eslint-final2.log`、`/tmp/lingxi-tool-contract-p403-boundary-engine-bridge.log`、`/tmp/lingxi-tool-contract-p403-boundary-calltool-inventory.log`。
- 提交 SHA：`ae40059d531bad737c1427cb232e7e8fcf7d03ba`
- 偏差：为适配 Manager descriptor，既有 `tests/mcp-runtime.test.ts` 的目录投影测试先建立真实已发布工具，属于测试夹具同步；无生产范围扩张。目录继续保留无 `mcp_` 前缀的兼容名称，执行身份与 TargetId 不变。

### P5-01 给 PluginManager 增加工具代次

- 状态：`completed`
- 改动文件：`core/plugin-manager.ts`、`core/tool-target-registry.ts`、`core/tool-invocation-gateway.ts`、`tests/plugin-manager.test.ts`、`tests/plugin-runtime.test.ts`、`TOOL_INVOCATION_REPAIR_PROGRESS.md`。
- 红灯命令：`set -o pipefail; npx vitest run tests/plugin-manager.test.ts tests/plugin-runtime.test.ts 2>&1 | tee /tmp/lingxi-tool-contract-p501-red.log`
- 红灯原始结果：exit `1`；`1 passed / 1 failed` files、`102 passed / 2 failed` tests；两项均因旧管理层不存在 `getPluginToolGeneration` 而失败，符合预期。
- 绿灯命令：插件管理、插件运行时、注册表、网关、Engine 延迟装配和 bundled parity 共 6 文件 Vitest；`npm run typecheck`；注册表与网关定向 ESLint；`git diff --check`。
- 绿灯原始结果：Vitest exit `0`，`6 passed` files、`151 passed` tests；三段 typecheck exit `0`；定向 ESLint exit `0`、`0` 问题；`git diff --check` exit `0`。
- 绿灯日志：`/tmp/lingxi-tool-contract-p501-gate-final2.log`、`/tmp/lingxi-tool-contract-p501-typecheck-final2.log`、`/tmp/lingxi-tool-contract-p501-new-eslint-final.log`。
- 提交 SHA：`30eb7d7c5eeb4f9b8d455961ef8bab7104adccf7`
- 偏差：任务书只在 P5-03 后给出阶段提交信息，但用户明确要求每项提交并推送；本项使用独立、内容对应的提交信息。开发入口的 reset 复用同一 reload/install 流程，因此由同一 unload cleanup 与初次加载代次推进覆盖，不另设旁路。

### P5-02 给 MCP 工具增加 live generation

- 状态：`completed`
- 改动文件：`core/mcp/manager.ts`、`core/engine.ts`、`core/tool-target-registry.ts`、`tests/tool-deferred-mcp-parity.test.ts`、`TOOL_INVOCATION_REPAIR_PROGRESS.md`。
- 红灯命令：`set -o pipefail; npx vitest run tests/tool-deferred-mcp-parity.test.ts 2>&1 | tee /tmp/lingxi-tool-contract-p502-red.log`
- 红灯原始结果：exit `1`；`1 failed` file、`12 passed / 1 failed` tests；新增用例因旧 Manager 不存在连接器代次接口而失败，符合预期。
- 绿灯命令：MCP parity/runtime、Engine 延迟装配、Catalog Bridge、网关、注册表和会话权限共 7 文件 Vitest；`npm run typecheck`；新增回归与注册表定向 ESLint；`git diff --check`。
- 绿灯原始结果：Vitest exit `0`，`7 passed` files、`278 passed` tests；三段 typecheck exit `0`；定向 ESLint exit `0`、`0` 问题；`git diff --check` exit `0`。
- 绿灯日志：`/tmp/lingxi-tool-contract-p502-gate-final.log`、`/tmp/lingxi-tool-contract-p502-typecheck-final.log`、`/tmp/lingxi-tool-contract-p502-new-eslint-final.log`。
- 提交 SHA：`aede651699e2e6ce6b71bcd73ac62607ec9dd1a4`
- 偏差：任务书只在 P5-03 后给出阶段提交信息，但用户明确要求每项提交并推送；本项使用独立、内容对应的提交信息。测试放在 P4-03 新建的 MCP direct/deferred parity 文件中，未新建额外测试模块。

### P5-03 定义旧会话行为并扩展漂移播报

- 状态：`completed`
- 改动文件：`core/engine.ts`、`tests/tool-lifecycle-revocation.test.ts`、`TOOL_INVOCATION_REPAIR_PROGRESS.md`。
- 红灯命令：`set -o pipefail; npx vitest run tests/tool-lifecycle-revocation.test.ts 2>&1 | tee /tmp/lingxi-tool-contract-p503-red.log`
- 红灯原始结果：exit `1`；`1 failed` file、`5 passed / 1 failed` tests。禁用、卸载、reload、MCP 清单变化和临时断线的撤销测试已由 P5-01/P5-02 生效；唯一失败证明漂移清单仍只含 MCP、漏掉 plugin。
- 绿灯命令：生命周期新测试、plugin Manager/runtime、MCP parity/runtime、注册表、网关、Engine 延迟装配与会话漂移播报共 9 文件 Vitest；`npm run typecheck`；新增测试定向 ESLint；`git diff --check`。
- 绿灯原始结果：Vitest exit `0`，`9 passed` files、`312 passed` tests；三段 typecheck exit `0`；新增测试 ESLint exit `0`、`0` 问题；`git diff --check` exit `0`。
- 绿灯日志：`/tmp/lingxi-tool-contract-p503-gate-attempt3.log`、`/tmp/lingxi-tool-contract-p503-typecheck-attempt3.log`、`/tmp/lingxi-tool-contract-p503-new-eslint-attempt3.log`。
- 提交 SHA：待本项提交后、P6-01 红灯前回填。
- 偏差：none

## 错误日志

| 时间 | 编号 | 原始错误 | 次数 | 处理 |
| --- | --- | --- | ---: | --- |
| 2026-09-05 13:29 +0800 | 目标初始化 | 当前任务已自动存在，重复创建目标失败 | 1 | 读取现有目标，确认与本次请求一致，不再重复创建 |
| 2026-09-05 13:31 +0800 | P0-00 | `origin/feat/knowledge-retrieval-research-p0-p3` 不存在，引用核对 exit `1` | 1 | 固定 SHA 本地可达，按任务书规则继续，不更换基线 |
| 2026-09-05 13:36 +0800 | P0-01 记录 | 首次回填 P0-00 完整提交 SHA 时录入值与 Git 实际值不一致 | 1 | 立即用 `git rev-parse HEAD` 回读并更正；后续所有提交坐标只从 Git 命令输出复制 |
| 2026-09-05 13:38 +0800 | P0-01 | 全量基线 2 项失败：旧审计封印检测到本任务文档，发布预检检测到远端历史版本高于固定基线 | 1 | 保留 exit `1` 与原始统计；不放宽测试、不改版本；最终在 P12 建立新封印 |
| 2026-09-05 13:41 +0800 | P0-01 | 无 `LINGXI_SIGN_KEY` 导致服务构建 exit `1` | 1 | 用临时抛弃式密钥诊断复跑 exit `0`，随后精确销毁临时签名目录；首次失败仍记为 FAIL_ENVIRONMENT |
| 2026-09-05 13:46 +0800 | P0-01 推送 | 首次 `git push` 因 GitHub HTTPS 接收超时退出 `128` | 1 | 先回读本地/远端坐标并用 `git ls-remote` 确认网络恢复，再有限重试；第二次推送 exit `0` |
| 2026-09-05 13:50 +0800 | P0-02 | 假设三个 bundled plugin 存在根 `index.ts`，组合读取命令因文件不存在退出 `1` | 1 | 根据 `find` 结果改读真实 `manifest.json` 与 `tools/*.ts`，不重复错误路径 |
| 2026-09-05 13:57 +0800 | P1-01 | 首次定向 ESLint 报 `FirstPartyToolIdentityInput` 为空接口，`1 error / 0 warnings`，exit `1` | 1 | 改为等价类型别名；重跑定向测试、ESLint、typecheck、diff check 全部 exit `0` |
| 2026-09-05 13:59 +0800 | P1-02 记录 | 回填 P1-01 SHA 的首次补丁因上下文选择不精确而校验失败，未写入 | 1 | 回读真实位置后改用定点上下文补丁 |
| 2026-09-05 14:08 +0800 | P1-02 | 首次受影响回归为 `154 passed / 30 failed`：旧测试插件未声明权限被拒绝；legacy `plugin_output` 被错误送审；审批通过后错误地对原始工具重验 | 1 | 保持生产端缺声明拒绝；测试夹具补显式权限；复用规范化契约表达 legacy 自动放行，并统一对规范化工具重验；复跑 `185/185` 通过 |
| 2026-09-05 14:14 +0800 | P1-02 | 首次 typecheck 因联合类型未被 `!result.ok` 收窄报 `TS2345`，exit `2` | 1 | 改为 `result.ok === false` 的显式判别，不改变运行逻辑；重跑本项门禁 |
| 2026-09-05 14:19 +0800 | P1-02 | 改为复用真实分类入口后，`187` 项中 `1` 条字段断言失败：只读工具在 auto 档的决策本来就是 allow | 1 | 将内部字段收窄为仅表达 legacy routine 免审；未改变权限决策，重跑本项门禁 |
| 2026-09-05 14:24 +0800 | P1-03 红灯记录 | 首次带 `tee` 的命令未开启 `pipefail`，外层退出码为 `0`，但 Vitest 明确报告 `8 failed` | 1 | 在实现前用同一测试加 `set -o pipefail` 重跑，取得真实 exit `1`；后续带 `tee` 的门禁统一开启 `pipefail` |
| 2026-09-05 14:25 +0800 | P1-03 探索 | 一条只读 `rg` 命令引号组合错误，zsh 报 `unmatched quote`，exit `1` | 1 | 改用两条简单搜索确认范围，不复用错误命令；无文件修改 |
| 2026-09-05 14:26 +0800 | P1-03 | 首次实现回归 `7 passed / 1 failed`；单个大样例触发校验库最多 8 条错误的上限，尾部数值路径未进入 issue 列表 | 2 | 保留全部约束维度，将数值样例改成单一整数越界并减少同对象内前置噪声；最终阶段门禁 `202/202` 通过 |
| 2026-09-05 14:32 +0800 | P2-01 | 首次定向 ESLint 报 `1 warning / 0 errors`：新增文件含未使用的类型导入 | 1 | 删除自身引入的无用导入；复跑 ESLint 为 `0` 问题，测试与 typecheck 仍通过 |
| 2026-09-05 14:41 +0800 | P2-02 编辑 | 首次多文件补丁因函数签名上下文与实际源码不一致而校验失败，未写入 | 1 | 回读真实签名后拆成小范围定点补丁；未产生文件改动 |
| 2026-09-05 14:46 +0800 | P2-02 | 首次全改动文件 ESLint 为 `0 errors / 89 warnings`，其中新增测试含 `7` 个显式 `any` 警告 | 1 | 只清理本项新增测试类型；新增文件复跑为 `0` 问题，测试与 typecheck 保持通过 |
| 2026-09-05 14:55 +0800 | P3-01 | 首次实现后仍有 `1/121` 失败：桥接工具的宿主委托资格依赖对象身份，权限包装复制对象后规范化解析器抛错 | 1 | 不放宽 capability；让权限包装识别宿主已登记对象并保留原对象，复跑本项及权限相关回归 `265/265` 通过 |
| 2026-09-05 14:56 +0800 | P3-01 编辑 | 首次多文件补丁因 import 上下文不匹配而校验失败，未写入 | 1 | 回读真实 import 与测试位置后定点补丁；无残留诊断代码 |
| 2026-09-05 15:06 +0800 | P3-02 | 首次实现回归 `4 passed / 2 failed`：延迟调用的 prepared route 仍按 direct 生成 | 1 | 由宿主给桥接调用绑定 deferred 路由，参数、目标和调用句柄继续由 prepared 摘要复核 |
| 2026-09-05 15:07 +0800 | P3-02 | 路由修正后剩 `1/6` 失败：两个插件共享本名时 capability 全局反查歧义 | 1 | effective TargetId 已唯一确定真实目标，改为对该目标的权威 capability 精确比对；未采用平面名称猜测 |
| 2026-09-05 15:08 +0800 | P3-02 扩展回归 | 既有 Engine 测试夹具未经过 PluginManager，缺少 P1 已要求的规范化身份/权限，4 项 fail-closed | 1 | 仅补齐测试夹具的真实注册期契约；生产端继续拒绝缺失声明，不降级 |
| 2026-09-05 15:13 +0800 | P3-02 扩展回归 | 两个旧断言仍把普通运行上下文放在 canonical executor 的 signal 参数位 | 1 | 更新断言为第三参数仅透传真实 AbortSignal，运行上下文从第五参数验证；实现未做兼容性猜测 |
| 2026-09-05 15:13 +0800 | P3-02 | 首次 typecheck 报 4 个类型错误：普通 object 属性读取 3 个、联合类型未收窄 1 个 | 1 | 加入显式记录类型与 `allowed === false` 判别；复跑三段 typecheck exit `0` |
| 2026-09-05 15:25 +0800 | P4-01 | 首次实现回归剩 `1/71` 失败：测试夹具从通用身份读取了不存在的 MCP 专属字段 | 1 | 改为读取规范身份中的本地名字段；不改生产解析语义，复跑通过 |
| 2026-09-05 15:26 +0800 | P4-01 | 首次 typecheck 报一个测试对象多余兼容字段，exit `2` | 1 | 删除测试输入中的旧目录字段；严格新输入契约不接受该字段，复跑三段 typecheck exit `0` |
| 2026-09-05 15:39 +0800 | P4-02 | 首次扩展 bundled parity 为 `143 passed / 2 failed`：一项旧文案断言仍写“仅外部工具”，同本名能力的全局反查再次产生歧义 | 1 | 更新已改变契约的文案断言；委托校验改为携带已解析 TargetId 到 Gateway 做目标级权威能力复核，不按平面 capability 猜目标 |
| 2026-09-05 15:42 +0800 | P4-02 | 首次 typecheck 报 Gateway 测试替身没有构造完整 PreparedInvocation，exit `2` | 1 | 测试替身改用生产准备对象构造器，不削弱生产接口；复跑三段 typecheck exit `0` |
| 2026-09-05 15:46 +0800 | P4-02 推送 | HTTPS 推送与引用核对均在 30 秒无响应；有界重试 exit `142`，直连 `github.com:443` 5 秒超时 | 2 | 发现用户级 Git 配置会把常见 SSH 地址重写为 HTTPS；改用不命中重写规则的 `ssh://git@github.com/...` 一次性地址后推送与 fetch 均 exit `0`，未修改远端配置 |
| 2026-09-05 16:06 +0800 | P4-03 | 首次最终 typecheck 报测试夹具把 `unknown` 赋给具体配置类型，exit `2` | 1 | 只在测试配置存储边界显式收窄类型，生产代码不变；重跑三段 typecheck |
| 2026-09-05 16:10 +0800 | P4-03 | 执行前 eligibility 错误码接入网关后，网关错误码联合和辅助函数类型过宽，typecheck exit `2` | 1 | 把 `TARGET_DISABLED_FOR_AGENT` 纳入网关稳定码并复用注册表判定类型；不改运行策略 |
| 2026-09-05 16:11 +0800 | P4-03 | 富结果等价测试 `1/85` 失败，仅差两套临时夹具各自生成的会话来源路径 | 1 | 让 direct/deferred 使用同一会话坐标后继续做完整对象严格相等，不删除 provenance 字段、不放宽断言 |
| 2026-09-05 16:26 +0800 | P5-02 | 首次实现后 MCP parity `187 passed / 2 failed`，direct 准备记录仍使用默认代次；typecheck 同时报 Manager 类声明缺字段 | 1 | 给统一 direct façade 写入装配代次，并补 Manager 代次表声明；网关仍严格拒绝不匹配，不放宽检查 |
| 2026-09-05 16:33 +0800 | P5-03 | 首次阶段回归 `312/312`，但 typecheck 报合并清单集合元素为 `unknown` | 1 | 把集合声明为字符串集合；不改清单内容或排序逻辑 |
| 2026-09-05 16:34 +0800 | P5-03 | 类型检查继续发现新增测试的可用性回调和两个构造夹具类型过宽 | 1 | 复用注册表判定类型，并按项目既有测试边界把构造夹具收口为 `never`；生产代码不变 |

## 断点续跑自检

| 问题 | 答案 |
| --- | --- |
| 现在在哪里？ | P5-03 已完成红绿验证与 P5 阶段门禁，等待提交和推送 |
| 接下来去哪？ | 使用任务书指定提交信息提交并推送 P5-03，回填 SHA 后进入 P6-01 |
| 最终目标是什么？ | 证明并修复工具调用语义对执行路径不敏感，完成 P0–P12 全部门禁与审计封印 |
| 已学到什么？ | 12 个 bundled 工具均为 legacy 权限方言；7 个只读、5 个副作用；当前包装不保留延迟元数据 |
| 已做什么？ | 完成并推送 P0-00 至 P5-02；P5-03 完成审批后生命周期撤销、旧对象零执行、快照保留、临时断线精确错误，以及 plugin+MCP 联合漂移清单；相关门禁 `312/312` |
