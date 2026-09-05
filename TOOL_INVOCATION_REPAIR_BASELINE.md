# 工具契约执行路径不变量修复基线

## Git 与环境坐标

- 仓库：`ItsDalk-Lane/LingxiAgent`
- 固定来源分支：`feat/knowledge-retrieval-research-p0-p3`
- 固定来源提交：`4fefe66ec3b4f6b23c78a09869a607886585740e`
- 执行分支：`fix/tool-contract-path-invariance`
- Node：`v24.16.0`
- npm：`11.13.0`
- 操作系统：`Darwin 25.6.0 arm64`
- 开始时间：`2026-09-05 13:31:56 +0800`
- 切分支前工作树：干净
- 依赖安装：`npm ci`，exit `0`；新增 `1286` 个包，审计 `1291` 个包
- 依赖审计摘要：`13 vulnerabilities (1 low, 11 moderate, 1 high)`；未运行自动修复

## P0-00 偏差记录

- `git fetch origin --prune`：exit `0`。
- 固定来源远端引用核对：exit `1`，`origin/feat/knowledge-retrieval-research-p0-p3` 已不存在。
- 按任务书既定规则继续从固定提交创建分支；固定提交经 `git cat-file -e` 验证可达。
- 目标本地分支和远端分支在创建前均不存在，没有改写已有历史。

## P0-01 基线门禁

| 门禁 | 日志 | 原始结果 | 状态 |
| --- | --- | --- | --- |
| `npm run typecheck` | `/tmp/lingxi-tool-contract-p001-typecheck.log` | exit `0`；三段 TypeScript 检查完成 | PASS |
| `npm run lint` | `/tmp/lingxi-tool-contract-p001-lint.log` | exit `0`；`0 errors`，`9188 warnings`，其中 `24` 条可自动修复 | PASS_WITH_WARNINGS |
| 11 文件定向 Vitest | `/tmp/lingxi-tool-contract-p001-targeted.log` | exit `0`；`11 passed` files；`251 passed` tests；无失败、无跳过 | PASS |
| `npm test` | `/tmp/lingxi-tool-contract-p001-full.log` | exit `1`；`1331 passed / 2 failed / 1 skipped` files；`13432 passed / 2 failed / 7 skipped` tests | FAIL_BASELINE |
| `npm run build:server` | `/tmp/lingxi-tool-contract-p001-build-server.log` | exit `1`；签名打包前明确拒绝：`LINGXI_SIGN_KEY is not set` | FAIL_ENVIRONMENT |
| `git diff --check` | 无单独日志 | exit `0` | PASS |

### 基线失败归因

1. `post-verification-audit-seal`：检测到本任务要求新增的 `TOOL_INVOCATION_REPAIR_BASELINE.md` 和 `TOOL_INVOCATION_REPAIR_PROGRESS.md` 位于旧封印 SHA 之后。该门禁按设计 fail-closed，未修改测试或 allowlist；最终在 P12 重新封印。
2. `release-preflight`：固定基线的候选版本为 `0.1.33`，本次 `git fetch --prune` 后读取到历史最大版本 `0.1.34`，因此发布预检按设计失败。未改版本、未改发布规则。
3. `build:server`：环境没有 `LINGXI_SIGN_KEY`。使用仓库自带密钥生成器，在 `/tmp/lingxi-tool-contract-p001-signing` 创建抛弃式密钥与匹配 keyset 后，诊断复跑 exit `0`，日志为 `/tmp/lingxi-tool-contract-p001-build-server-diagnostic.log`；随后精确删除临时目录内四个文件并移除空目录。首次失败仍保留为原始基线结果。

P0-01 的职责是建立真实基线而非修复这些非工具契约失败；三处红灯均保留为失败状态，没有改写成“可忽略”或 PASS。

## P0-02 现状调用矩阵与原始入口

### Bundled plugin 工具矩阵

三份内置插件 manifest 的 `id` 分别为 `media`、`beautify`、`office`。12 个工具全部使用 legacy 权限方言，没有 `resolveInvocation`；源码均未声明 `deferrable` 或 `pinned`，静态插件包装也没有保留这两个字段。

| pluginId | localName | 当前 publicName | 权限方言 | readOnly/副作用类别 | deferrable | pinned |
| --- | --- | --- | --- | --- | --- | --- |
| media | `describe-options` | `media_describe-options` | legacy | readOnly | 未声明/未保留 | 未声明/未保留 |
| media | `get-guide` | `media_get-guide` | legacy | readOnly | 未声明/未保留 | 未声明/未保留 |
| media | `generate-image` | `media_generate-image` | legacy | `external_side_effect` / `external_generation` | 未声明/未保留 | 未声明/未保留 |
| media | `generate-video` | `media_generate-video` | legacy | `external_side_effect` / `external_generation` | 未声明/未保留 | 未声明/未保留 |
| beautify | `get-cover-style-guide` | `beautify_get-cover-style-guide` | legacy | readOnly | 未声明/未保留 | 未声明/未保留 |
| beautify | `get-html-style-guide` | `beautify_get-html-style-guide` | legacy | readOnly | 未声明/未保留 | 未声明/未保留 |
| beautify | `list-capabilities` | `beautify_list-capabilities` | legacy | readOnly | 未声明/未保留 | 未声明/未保留 |
| beautify | `apply-cover-candidate` | `beautify_apply-cover-candidate` | legacy | `review` / `workspace_write` | 未声明/未保留 | 未声明/未保留 |
| beautify | `create-cover` | `beautify_create-cover` | legacy | `review` / `workspace_write` | 未声明/未保留 | 未声明/未保留 |
| office | `list-capabilities` | `office_list-capabilities` | legacy | readOnly | 未声明/未保留 | 未声明/未保留 |
| office | `read-document` | `office_read-document` | legacy | readOnly | 未声明/未保留 | 未声明/未保留 |
| office | `html-to-pdf` | `office_html-to-pdf` | legacy | `review` / `workspace_write` 或 `plugin_output` | 未声明/未保留 | 未声明/未保留 |

合计：7 个 readOnly，5 个副作用工具。

### 当前调用与目录行为

- Engine 在最终 Agent 可用性过滤之前先做延迟计划；阈值统计发生在 raw plugin/MCP 集合上。plugin 目录项把 `deferrable` 硬写为 `true`、`pinned` 硬写为 `false`。
- Engine 建立 `builtinToolsByName` 原始对象表；延迟 builtin 直接调用旧对象，并改写调用编号为 `bridge_<name>`。延迟 MCP 直接调用 Manager 的底层入口。
- 最终可用性过滤只作用于已经分流后的 model-facing 工具数组，因此被 Agent 禁用的 plugin 仍可能先进入 Catalog。
- Catalog 主索引是全局平面 `name`。跨来源同名时按注册顺序“先到先得”，后来的条目静默消失；`describe` 只接收名称，没有 source/server 消歧参数。
- Catalog 保存 `schemaRef` 闭包，Bridge 同时持有 builtin/MCP 原始执行回调；它并非纯目标引用目录。
- Bridge 的参数检查只覆盖顶层 `required` 和单一顶层 `type`，不检查嵌套对象、数组元素、枚举、联合、额外字段和范围。
- Bridge 对参数错误、找不到目标和执行异常都返回普通文本结果；catch-all 会把类型化失败压成看似成功的文本响应。
- Bridge 的 capability 委托通过字符串后缀截取并查询平面 Catalog 名称，不绑定权威目标身份。
- plugin-dev 聊天工具使用自身粗粒度固定权限描述；服务最终直接调用 `PluginManager.executePluginTool`。
- MCP Manager 的 published tool 本身会走 Manager 的结果适配和多轮输入链；Engine 的延迟 MCP 回调直接下钻 Manager，尚未与 published tool 对象形成唯一执行器。

### PluginManager 原始执行入口

- 生产入口只有 `core/plugin-dev-service.ts:682`：开发服务查出 raw tool 后直接调用 `PluginManager.executePluginTool`。
- Manager 实现位于 `core/plugin-manager.ts:933`，接受 raw tool 对象并直接调用其 `execute`。
- 测试入口位于 `tests/plugin-manager.test.ts:557,1411,1448`。
- `plugin_dev_invoke_tool` 的模型 schema 在 `core/plugin-dev-tools.ts:217-229` 暴露 `sessionId`、`sessionRef`、`sessionPath`、`agentId`；处理器优先采用模型参数。HTTP 路由 `server/routes/plugins.ts:924-940` 也把客户端 body 中四个身份字段直传给开发服务。

### MCP 直接执行与底层 callTool 清单

- `core/engine.ts:4407`：延迟 Bridge 的 `mcpCall` 直接调用 `this._mcp.callTool`。
- `core/mcp/manager.ts:1757`：app 专用调用路径直接调用 live client。
- `core/mcp/manager.ts:1823`：Manager 多轮输入执行器调用 live client。
- `core/mcp/manager.ts:1837`：用户拒绝补充输入时向服务端发送一次拒绝收尾调用。
- `core/mcp/manager.ts:2029`：published tool 定义通过闭包回到 Manager `callTool`。
- `core/mcp/clients/http-client.ts:1137`：HTTP transport adapter 调用协议客户端。
- 测试中的低层调用共 24 处，位于 `tests/mcp-http-client.test.ts` 与 `tests/mcp-runtime.test.ts`；逐行位置保存在 `/tmp/lingxi-tool-contract-p002-mcp-calltool.log`。
- 当前 published MCP tool 会在执行时复核全局开关、model visibility 和 Agent 配置，并保留 app card、调用编号、来源会话和 Agent；Bridge 直接走 Manager，绕过了这层 published adapter 的结构化结果语义。
- MCP Manager 自身会拒绝规范化后名称碰撞的 connector 工具；但进入通用 Catalog 后，跨 origin/source 同名仍由平面索引静默先到先得。

### 知识旧入口调用点

- 生产代码只有 `core/engine.ts` 中的兼容门面定义与其对 legacy injector 的内部调用；全仓没有其他生产调用方。
- legacy 实现定义在 `lib/knowledge/legacy/legacy-knowledge-context-injector.ts:1074`。
- 测试调用分布：`tests/knowledge-context-injector.test.ts` 34 处、`tests/knowledge-coverage-execution.test.ts` 23 处、`tests/knowledge-coverage-planner.test.ts` 5 处、`tests/knowledge-evidence-manifest.test.ts` 2 处、`tests/knowledge-retrieval-golden.test.ts` 1 处、`tests/knowledge-search-service.test.ts` 1 处；逐行位置在 `/tmp/lingxi-tool-contract-p002-knowledge-callers.log`。
- Engine 的 compiled-scope 分支在 `core/engine.ts:3280-3283` 固定传 `rerank: true`，没有传递 injector 给出的 `rerankPolicy`；notebook query 分支在 `core/engine.ts:3284-3292` 会传递该策略，因此两条兼容路径语义分叉。

### 四个媒体入口当前凭证回退顺序

1. image：`core/media/image-task-runner.ts` 先尝试显式 provider/model，再配置默认，再第一个有凭证的媒体 provider，最后退到 legacy adapter；目标内凭证提供者按 `media.credentialProviderId || media.providerId || input.provider || adapter.id` 取值。通过总线解析时，上游 `hub/index.ts` 又按 `lane.providerId || activeProviderId || runtimeProviderId` 取值。
2. video：`core/media/universal-media-manager.ts` 对正式模型按 `credentialLane.providerId || runtimeProviderId` 取值；legacy adapter 路径直接用 adapter/provider；对外结果又按 `target.credentialProviderId || target.providerId` 回退。
3. STT：`core/speech-recognition-service.ts:416` 按 `credentialLane.providerId || runtimeProviderId` 获取新鲜凭证，没有消费统一的 active lane 选择结果。
4. 后台 image task：任务目标按 `media.credentialProviderId || media.providerId || input.provider || adapter.id` 形成，执行/恢复时还会读取持久化目标字段；因此可能与前台总线的 active provider 选择不同。

四路都没有消费同一个“运行目标 + 凭证目标”解析结果；回退优先级散落在入口和下游适配器中。

### P0-02 原始搜索证据

任务书列出的六条 `rg` 全部 exit `0`。日志：

- `/tmp/lingxi-tool-contract-p002-bridge-entrypoints.log`（16 行）
- `/tmp/lingxi-tool-contract-p002-plugin-executors.log`（5 行）
- `/tmp/lingxi-tool-contract-p002-mcp-calltool.log`（30 行）
- `/tmp/lingxi-tool-contract-p002-permission-dialects.log`（158 行）
- `/tmp/lingxi-tool-contract-p002-media-credentials.log`（85 行）
- `/tmp/lingxi-tool-contract-p002-knowledge-rerank.log`（104 行）

补充明细日志：`/tmp/lingxi-tool-contract-p002-bundled-tools.log`、`/tmp/lingxi-tool-contract-p002-dev-mcp-details.log`、`/tmp/lingxi-tool-contract-p002-media-fallback-details.log`、`/tmp/lingxi-tool-contract-p002-knowledge-callers.log`、`/tmp/lingxi-tool-contract-p002-rerank-callers.log`。
