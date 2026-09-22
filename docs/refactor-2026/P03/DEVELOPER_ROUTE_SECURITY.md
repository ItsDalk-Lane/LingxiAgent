# P03-T05｜高权限开发与间接入口安全

日期：2026-09-22。结论：当前生产面没有 HTTP 开发后门；唯一的本地开发者通道无生产调用方；真实 HTTP 工具执行入口（MCP app-tools）为 owner-only 并有路由级测试。

## 1. 入口逐项核查

| 入口 | 现状 | 认证/来源/本地远程区分 | 证据 |
|---|---|---|---|
| HTTP plugin-dev 开发路由（原 `core/plugin-dev-service.ts`） | **已删除**（04f90d2b2 插件生态收口）；server/desktop/hub/cli 无 `plugin-dev` 路由残留（grep 全量复核） | 不适用（入口不存在） | `git log --all -- core/plugin-dev-service.ts`；本阶段删除其残留的 AST 白名单死条目（见 EXECUTION_BOUNDARY_AUDIT §3） |
| 本地开发者主体（`prepareAndInvokeForLocalDeveloper`，route `plugin-dev-http`） | 保留在网关，**当前仅测试消费**（无生产调用方） | `isLocalDeveloperPrincipal` 硬校验：kind、`local-developer:` 前缀 principalId、`connectionKind === "local"`；且必须先经宿主 `authorize` 单次审批 | `tests/tool-invocation-gateway.test.ts` "本地开发入口自行准备并只调用一次宿主审批"/"本地开发入口拒绝非本地开发主体" |
| 主体铸造（`createLocalDeveloperPrincipal`） | 存在，宿主侧铸造 | 只接受已认证本地 owner（kind `local_user` + `connectionKind: "local"` + `credentialKind: "loopback_token"` + 非空 principalId）；远程连接/非 loopback 凭证/自报 kind 一律 TypeError | 本阶段新增 "createLocalDeveloperPrincipal 拒绝远程/伪造 owner（P03-A08）"（6 组伪造样例） |
| 真实 HTTP 工具执行入口：`POST /api/mcp/connectors/:id/app-tools/:toolName/call`（`callAppTool`） | 存在（桌面 app 卡片通道） | ① 全局鉴权中间件（`server/http/request-principal.ts`：bearer/query token/web session → `authorizeHttpRoute`）；② 路由归 **STUDIO_OWNER**（`server/http/route-security.ts:114`，注释明确"真实第三方副作用，owner-only 而非 settings scope"）；③ 只允许 app-visibility 工具（`_requireAppVisibleTool` 双重复核）+ 全局开关 + 连接器启用检查 | `tests/http-route-security.test.ts:377-383`（非 owner 拒绝）；`tests/mcp-routes.test.ts` app-visibility 403 / 停用 409 / 未初始化 503（真实 Hono request） |
| 认证层伪造面（远程冒充本地） | loopback token 只在本地连接上下文有效；web session 不跨 LAN 重放本地 owner | `serverAuthService.authenticateRequestDetailed` 按 connectionKind 区分 | `tests/server-auth.test.ts` "authenticates loopback token only for local connection context"、"does not replay a local-owner web session over LAN transport" |
| 插件调试（`PluginManager.executePluginTool`） | 无生产调用方（仅 tests 单测）；AST 规则现在对任何文件报违规（白名单已清空） | 不构成生产通道 | 边界门禁 2244 文件 0 违规 |

## 2. 间接资源访问（T05-3）

- **exec_command**：全平台经 PathGuard + OS 沙盒（macOS seatbelt / Linux bwrap / Windows restricted-token；`require_escalated` 槽位仍过 PathGuard preflight，HARD 分级任何模式都拦）——`tests/sandbox-tool-wrapper.test.ts`、`tests/sandbox-policy.test.ts`、`tests/bwrap-sandbox-policy.test.ts`。
- **run_code**：REPL 内核经终端管理器以用户会话身份运行（OPTIONAL 工具、调用经权限包装审批）；工具本体在沙盒层创建、结果走 ResourceIO 包装。内核进程隔离属工具自身业务实现，P03 不改写（任务书 §3"不重写每个工具的专业业务实现"）；此差异作为观察事实登记，不当作已发现缺陷。
- 无泛化"任意代码执行免审"通道：run_tools 子调用绑定完整包装后工具面（`tests/ptc-engine-assembly.test.ts` 只读拒绝用例）。

## 3. A08 场景映射（远程伪造开发者）

真实开发 HTTP 入口已不存在 → 场景在现存最接近面上分三层闭合（执行计数均为 0）：
1. 认证层：远程连接不能用 loopback token 成为本地主体（server-auth 测试）。
2. 路由层：真实 HTTP 工具执行入口 owner-only，非 owner 主体 403（http-route-security 测试，覆盖两个挂载前缀）。
3. 网关层：伪造 local-developer 主体（connectionKind 远程）与伪造 owner（6 组样例）均拒绝，`executeCanonical` 0 次调用。

审计面：网关诊断日志记录 route/targetId/generation/code；路由拒绝由 route-security 结构化错误返回（含 requiredScope）。

## 4. 本阶段改动

- 测试：+1（`createLocalDeveloperPrincipal` 伪造 owner 拒绝）；生产代码零改动（UNCHANGED_VERIFIED）。
