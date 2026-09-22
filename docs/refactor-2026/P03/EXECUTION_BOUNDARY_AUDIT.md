# P03-T03｜集中真实执行与授权绑定审计

日期：2026-09-22。结论：网关集中执行与 prepared 绑定为既有正确实现（UNCHANGED_VERIFIED）；本阶段删除一条指向已删文件的死白名单条目（见 §3），其余零生产改动。

## 1. 原网关/适配器接线

见 [TOOL_PATH_MATRIX.json](TOOL_PATH_MATRIX.json)。要点：

- **executeCanonical 唯一调用点**是 `ToolInvocationGateway.invoke`（AST 规则 `canonical-executor-bypass`，2244 文件扫描通过）。
- **MCP 底层 callTool** 只出现在两个精确文件：`core/mcp/manager.ts:2163`（manager 方法本体；发布适配器 execute 在 `:929` 经它交付执行——先过全局开关/可见性/agent 启用检查，结果经 `normalizeMcpToolResult` 归一）与 `core/mcp/clients/http-client.ts`（传输层）。适配器只转换与交付，不授权限：授权裁决在 `resolvePermission`→权限 wrapper 审批。
- **插件执行**：发布工具自带 execute（plugin host 适配），engine 以目标注册后经网关；`PluginManager.executePluginTool`（`core/plugin-manager.ts:955`）无生产调用方（仅 tests 直接单测），且 AST 规则现在对任何文件报违规（白名单为空）。
- **stage_files 宿主执行证明**：`separateHostExecutionProof` 只接受不可枚举/不可写/冻结、携带 `canonicalPaths` 与 `checkStagePath` 的精确符号证明（`STAGE_FILES_EXECUTION_BOUNDARY`），模型参数无法伪造（`core/tool-invocation-gateway.ts:174-235`）。

## 2. 身份与权限审计字段

- **prepared 绑定事实**（`createPreparedInvocation`）：targetId、route、参数摘要（键序稳定 digest）、sessionId/sessionPath/agentId、permission（capability/action/kind/sideEffect）、lifecycleGeneration、toolCallId、createdAt。执行前逐项比对，任一不符 → `PREPARED_INVOCATION_MISMATCH`（`tests/tool-invocation-gateway.test.ts` mismatch 族 5 例 + 键序稳定 1 例）。
- **模型/插件自报身份无效**：args 中同名 `sessionId/agentId/principal/lifecycleGeneration` 字段不参与绑定（P01-A08 严格/宽松两例）；执行 ctx 的 `invocationRoute/effectiveTargetId` 由网关覆写。
- **审批后复核**：`invoke` 入口先比对当前代次（`getCurrentGeneration` ≠ 装配代次 → `TARGET_REVOKED`），再重校验参数并比对 digest，再 `isCurrentlyAvailable`（禁用 → `TARGET_DISABLED_FOR_AGENT`/`TARGET_REVOKED`），全部通过才执行（`tests/tool-invocation-path-parity.test.ts` "revokes %s after approval" 3 路由 × 2 场景）。
- **诊断日志字段**：route/origin/targetId/sourceId/generation/code，全部安全归因字段，无参数值与秘密（gateway 测试断言不含用户路径与密钥样例）。
- **资源边界**：file/stage 走 `permissionBoundary.checkStagePath`（真 OS 归一化路径判定，parity fixture 断言每调用 3 次检查、越界 `ACTION_BLOCKED_BY_WORKSPACE_BOUNDARY` 且 0 执行）；沙盒 IO 的符号链接/工作区根/外部读路径有独立测试（`tests/sandbox-policy.test.ts`、`tests/resource-io-sandbox-tools.test.ts`、`tests/sandbox-tool-wrapper.test.ts`），不靠字符串 startsWith。

## 3. 旁路删除表

| 旧路径/条目 | 原职责 | 现真实消费者 | 处置 | 理由与影响 |
|---|---|---|---|---|
| AST 白名单条目 `pluginExecuteTool: ["core/plugin-dev-service.ts"]` | 允许 plugin-dev HTTP 服务直接调 `executePluginTool` | 无（该文件已在 04f90d2b2 删除，白名单指向不存在的路径） | **删除条目（本阶段）** | 文件不存在时条目本就不放行任何真实调用；删除后语义诚实化：任何生产 `executePluginTool` 调用都报违规。门禁与测试同步更新（`scripts/check-tool-invocation-boundaries.mjs`、`tests/tool-invocation-boundary.test.ts`、架构文档白名单表）。 |
| HTTP plugin-dev 开发路由（`core/plugin-dev-service.ts` 整文件） | 本地开发者主体经 HTTP 调试插件工具 | 无（server/desktop/hub/cli 无 plugin-dev 路由残留，grep 复核） | **已于 04f90d2b2 删除（既有事实，本阶段核实）** | `gateway.prepareAndInvokeForLocalDeveloper` 保留为唯一受 `isLocalDeveloperPrincipal` 硬校验保护的入口（当前仅测试消费），见 T05 文档。 |
| 桥接 `builtinCall`/`mcpCall`/`resolveBuiltinInvocation` 引用 | 防目录桥退化为原始适配器 | 无 | **维持既有禁止规则（UNCHANGED_VERIFIED）** | `bridge-raw-adapter` AST 规则 + 合成违规负例证明会红。 |
| engine 持有 deferred 原始工具映射（`builtinToolsByName` 等 4 标识符） | 防延迟工具以原始对象常驻内存 | 无 | **维持既有禁止规则（UNCHANGED_VERIFIED）** | `engine-deferred-raw-map` 规则 + 负例。 |

## 4. 不改动声明

未发现需要修改既有批准模式的路径绕过；不将审批改为默认允许或默认全拒（任务书 T03-5）。

## 5. 静态分析边界声明（2026-09-22 复验收补）

`canonical-executor-bypass` 规则匹配的是**成员调用形态**（`x.executeCanonical(…)` 与 `x["executeCanonical"](…)`）。由此：

- "executeCanonical 唯一调用点"的准确含义 = **生产源码中无非网关文件的成员调用**（AST 实测），不是对该符号一切引用形态的绝对唯一性断言。
- 以下引用形态**不在该规则匹配面**，属固有静态边界：
  1. **定义面引用**：`core/engine.ts` 三处对象字面量属性 `executeCanonical: (…) => …`（target 注册）与 `core/tool-target-registry.ts` 的类型声明/字段清单——它们是规则的合规前提，不是调用。
  2. **测试引用**：5 个测试文件直接引用该名称（合法测试面，`SOURCE_ROOTS` 不含 tests/）。
  3. **别名/脱钩调用**：`const c = target.executeCanonical; c(…)` 一类"先取成员再调用"可绕过成员调用匹配。可利用性低——`executeCanonical` 非独立导出符号（裸 `import { executeCanonical }` 无法通过 typecheck），需先持有已注册 target 对象——但如后续出现 target 对象外泄面扩大，应将规则升级为"网关文件外任何 `.executeCanonical` 属性引用"并处理 engine 定义面/测试面豁免。
- 同族边界：`mcp-raw-execution`/`plugin-raw-execution` 规则同为成员调用匹配，别名绕过面同上声明。
