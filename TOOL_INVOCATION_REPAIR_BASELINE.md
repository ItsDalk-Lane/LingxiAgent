# 工具契约执行路径不变量修复校正版基线

## P0-00 固定坐标

- 仓库：`ItsDalk-Lane/LingxiAgent`
- 正式来源：`main` / `v0.1.34`
- 固定源码提交：`60d910b84572c525a7c9c49216fb9206623bf7a4`
- 签名标签对象：`8c2e80e7e00b993a260a3e9273a85be1678c3b94`
- 校正版执行分支：`fix/tool-contract-path-invariance-v0134`
- 原证据分支：`fix/tool-contract-path-invariance`，HEAD `c723410c8ebcd95f6330f7a4a85c325698d3960b`
- Node：`v24.16.0`
- npm：`11.13.0`
- 操作系统：`Darwin 25.6.0 arm64`
- 开始时间：`2026-09-05T19:27:47+0800`

## 基线校正原因

原任务书固定的 `4fefe66ec3b4f6b23c78a09869a607886585740e` 是 v0.1.34 发布链的早期阶段提交。
正式 v0.1.34 在原任务开始执行前已经合入 main；从旧提交执行导致版本、知识架构和审计坐标
同时偏移。用户已明确授权以正式发布提交重新建立基线，但要求迁移既有成果，不推倒重做。

## 环境准备原始结果

- 远端同步：exit `0`；使用等价的显式 SSH 地址更新 `origin/*`，避免用户级 URL 重写造成 HTTPS 超时。
- `npm ci`：exit `0`；安装 `1286` 个包，审计报告 `13` 项依赖问题（`1 low / 11 moderate / 1 high`），未执行越权自动升级。
- 分支创建前原证据分支无已跟踪或未跟踪改动；规划技能随后创建三个本任务专用未跟踪规划文件并随校正版分支带入。
- 日志：`/tmp/lingxi-tool-contract-v0134-p000-fetch.log`、`/tmp/lingxi-tool-contract-v0134-p000-npm-ci.log`。

## P0-01 校正版基线门禁

| 门禁 | 原始日志 | 原始结果 | 判定 |
|---|---|---|---|
| `npm run typecheck` | `/tmp/lingxi-tool-contract-v0134-p001-typecheck.log` | exit `0`；三段 TypeScript 检查完成 | `PASS` |
| `npm run lint` | `/tmp/lingxi-tool-contract-v0134-p001-lint.log` | exit `0`；`0 errors / 9194 warnings`，其中 `25` 条可自动修复 | `PASS_WITH_WARNINGS` |
| 11 文件定向 Vitest | `/tmp/lingxi-tool-contract-v0134-p001-targeted.log` | exit `0`；`11 passed` files；`253 passed` tests；无失败、无跳过 | `PASS` |
| `npm test` | `/tmp/lingxi-tool-contract-v0134-p001-full-test.log` | exit `1`；`1359 passed / 1 failed / 1 skipped` files；`13763 passed / 1 failed / 7 skipped` tests | `FAIL_SEQUENCE_SEAL` |
| `npm run build:server` | `/tmp/lingxi-tool-contract-v0134-p001-build-server.log` | exit `1`；签名打包前明确拒绝：`LINGXI_SIGN_KEY is not set` | `FAIL_ENVIRONMENT` |
| 抛弃式密钥诊断构建 | `/tmp/lingxi-tool-contract-v0134-p001-build-server-diagnostic.log` | exit `0`；服务端、渲染器归档和签名清单均生成 | `PASS_DIAGNOSTIC` |
| `git diff --check` | 无单独日志 | exit `0` | `PASS` |

### 基线失败归因

1. `post-verification-audit-seal` 的唯一失败文件正是 P0-00 新增的五份任务记录；发布提交自身从旧已验证源码到 `60d910b8` 只改六份既有审计 allowlist 文件。未修改或放宽封印测试，留待 P12 重新固定已验证源码坐标。
2. 原始服务端构建在缺少真实签名密钥时按设计拒绝。随后用仓库自带生成器在 `/tmp/lingxi-tool-contract-v0134-p001-signing` 创建抛弃式密钥与匹配公开 keyset，诊断复跑 exit `0`；临时私钥和 keyset 已逐文件删除，空目录也已移除。
3. P0-01 只建立迁移前事实，不把上述两项原始失败改写为通过，也不在本项改审计门禁或生产代码。

## P0-02 v0.1.34 现状调用矩阵与迁移入口

### Bundled plugin 工具矩阵

当前有四份内置插件 manifest；其中 `jimeng-cli` 没有工具目录。实际 12 个工具仍来自 `media`、`beautify`、`office`。它们全部使用 legacy 权限方言，没有 `resolveInvocation`；源码没有声明 `deferrable` 或 `pinned`，静态插件包装也没有保留这两个字段。

| pluginId | localName | 当前 publicName | 权限方言 | readOnly/副作用类别 | deferrable | pinned |
|---|---|---|---|---|---|---|
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

### 当前目录与执行行为

- 引擎先对 raw plugin/MCP 集合规划延迟，再做最终 Agent 可用性过滤；plugin 目录项仍把 `deferrable` 固定为 `true`、`pinned` 固定为 `false`。
- 延迟 plugin 通过原始名称表直接调用旧工具对象；延迟 MCP 直接下钻 Manager。直接路径与延迟路径尚未共享唯一执行目标。
- Catalog 仍以全局平面名称为主索引；跨来源同名时后注册项静默丢失。目录保存 schema 闭包和原始执行回调，不是纯目标引用。
- Bridge 参数检查仍只覆盖顶层 required 和单一顶层类型；嵌套对象、数组元素、枚举、联合、额外字段和范围均不完整。
- 参数、找不到目标和执行异常仍会转成普通文本结果；catch-all 会压平类型化错误。
- capability 委托仍通过字符串后缀和 Catalog 平面名称判断，没有绑定权威目标身份。

### PluginManager 原始执行入口

- 生产入口：`core/plugin-dev-service.ts:682`；开发服务查出 raw tool 后直接调用 `PluginManager.executePluginTool`。
- Manager 实现：`core/plugin-manager.ts:933`；接受 raw tool 对象并调用其 `execute`。
- 测试入口：`tests/plugin-manager.test.ts:557,1411,1448`。
- 聊天开发工具仍允许模型参数携带会话和 Agent 身份；HTTP 路由也仍从请求体传入这些身份字段，两条入口尚未拆成各自可信的主体来源。

### MCP 直接执行与底层调用

- `core/engine.ts:4006`：延迟 Bridge 直接调用 Manager。
- `core/mcp/manager.ts:1757`：app 专用路径调用 live client。
- `core/mcp/manager.ts:1823`：多轮输入执行器调用 live client。
- `core/mcp/manager.ts:1837`：拒绝补充输入时发送拒绝收尾调用。
- `core/mcp/manager.ts:2029`：published tool 闭包回到 Manager。
- `core/mcp/clients/http-client.ts:1137`：HTTP transport adapter 调用协议客户端。
- 测试低层调用 24 处，均在 `tests/mcp-http-client.test.ts` 和 `tests/mcp-runtime.test.ts`；生产层没有新增其它直接入口。

### 知识入口的正式版变化

- v0.1.34 已删除生产用 legacy injector 和引擎兼容门面；旧实现只保留在 `tests/fixtures/knowledge-legacy/legacy-knowledge-context-injector.ts` 供历史行为测试。
- 正式检索改由 compiled scope、搜索服务和查询服务串联；搜索请求仍只携带 `rerank:boolean`，没有完整策略对象。
- 搜索服务按笔记本分组后调用查询服务的重排入口；查询服务内部另一条检索路径仍自带一份 deadline、降级与候选截断逻辑，因此 P8 的“共享策略”目标仍成立，但必须适配正式架构，禁止恢复已删除的生产 legacy 路径。
- 正式 `knowledge_search` 对 hybrid 固定启用重排、对 FTS 固定禁用；正式快速回答路径由独立的纯本地管线保证零远程，迁移不得改变这一路由。

### 四个媒体入口当前凭证回退顺序

1. image：显式 provider/model → 配置默认 → 第一个有凭证的媒体 provider → legacy adapter；下游还按多个字段再次回退。
2. video：正式模型优先 credential lane，再回 runtime provider；legacy adapter 另走自身 provider，结果层再次回退。
3. STT：按 credential lane provider 或 runtime provider 取新鲜凭证，没有统一消费 active lane 结果。
4. 后台 image task：创建时形成并持久化目标，执行和恢复时仍有自己的字段回退链，可能与前台 active provider 不同。

四路仍没有消费同一个“运行目标 + 凭证目标”解析结果。

### 新旧基线差异与迁移处置

- 原基线到 v0.1.34：303 个文件；原修复分支：106 个文件；同路径重叠 18 个。
- `git merge-tree --write-tree` 的真实冲突为 11 个：两份生成物、引擎装配、三份知识实现、一份知识工具、legacy 测试夹具重命名，以及三份知识测试/夹具。
- 先前“10 个冲突”漏计了生产 legacy 文件被正式版搬为测试夹具造成的重命名冲突；以本次 11 个现场结果为准。

| 阶段 | 迁移处置 | 原成果状态 |
|---|---|---|
| P1–P2 | 直接迁移规范身份、权限、schema、目标表和网关；在 v0.1.34 重跑原回归 | 复用，不重写 |
| P3 | 插件元数据主体直接迁移；引擎装配按 v0.1.34 当前结构适配 | 部分适配 |
| P4–P5 | Catalog、Bridge、MCP/插件生命周期主体复用；引擎连接点人工适配 | 部分适配 |
| P6 | 开发入口的主体分离与统一网关直接迁移 | 复用，不重写 |
| P7 | 媒体正式源码与旧基线无重叠，直接迁移并复测四入口 | 复用，不重写 |
| P8 | 只迁移共享策略语义；不恢复已删除的生产 legacy 文件，按正式搜索链和测试夹具适配 | 语义迁移 |
| P9–P10 | 错误因果、静态边界和路径等价测试主体复用；边界生成物重新生成 | 复用加再生成 |
| P11 | 架构说明可复用；事实、统计、剩余项和提交坐标全部按新分支重建 | 文档重建 |
| P12 | 全量门禁、构建、边界与 seal 全部重新执行 | 不复用旧 PASS |

### P0-02 原始搜索证据

任务书六条搜索命令全部 exit `0`：

- `/tmp/lingxi-tool-contract-v0134-p002-entrypoints.log`：16 行。
- `/tmp/lingxi-tool-contract-v0134-p002-plugin-execute.log`：5 行。
- `/tmp/lingxi-tool-contract-v0134-p002-mcp-calltool.log`：30 行。
- `/tmp/lingxi-tool-contract-v0134-p002-permission.log`：162 行。
- `/tmp/lingxi-tool-contract-v0134-p002-credential.log`：85 行。
- `/tmp/lingxi-tool-contract-v0134-p002-knowledge.log`：90 行。

P0-02 只修改审计与规划文档，没有修改生产代码、测试逻辑或运行生成物。
