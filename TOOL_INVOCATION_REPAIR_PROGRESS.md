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
| P1-02 | completed | 待提交后回填 | 新旧权限方言规范化 |
| P1-03 | pending | — | 完整 schema 校验器 |
| P2-01 | pending | — | 会话级目标注册表 |
| P2-02 | pending | — | PreparedInvocation 与统一网关 |
| P3-01 | pending | — | 插件元数据与 target adapter |
| P3-02 | pending | — | Engine 装配顺序 |
| P4-01 | pending | — | Catalog 目标引用目录 |
| P4-02 | pending | — | Bridge Gateway 适配 |
| P4-03 | pending | — | MCP eligibility 与执行器 |
| P5-01 | pending | — | Plugin 工具代次 |
| P5-02 | pending | — | MCP live generation |
| P5-03 | pending | — | 旧会话撤销语义 |
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
- 提交 SHA：待实现、门禁、提交后回填
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

## 断点续跑自检

| 问题 | 答案 |
| --- | --- |
| 现在在哪里？ | P1-02 已通过门禁，等待提交与推送 |
| 接下来去哪？ | 按指定提交信息提交并推送 P1-02，然后进入 P1-03 |
| 最终目标是什么？ | 证明并修复工具调用语义对执行路径不敏感，完成 P0–P12 全部门禁与审计封印 |
| 已学到什么？ | 12 个 bundled 工具均为 legacy 权限方言；7 个只读、5 个副作用；当前包装不保留延迟元数据 |
| 已做什么？ | 完成并推送 P0-00 至 P1-01；P1-02 已完成测试先红、实现和 185 项受影响回归 |
