# P03-T06｜工具结果、取消与副作用报告契约

日期：2026-09-22。既有实现 UNCHANGED_VERIFIED；本阶段补 A12 超时不重发测试（`tests/mcp-http-client.test.ts`）。

## 1. 结果结构：业务数据与错误外壳分离

- 工具成功结果保留**内容块（content[]）+ 结构化字段（details）+ 来源标记（provenance）**；网关 `normalizeResult` 只做归一不改写（`tests/tool-invocation-path-parity.test.ts` 断言三路由输出逐字节相等、结构化结果不经 Bridge 改写；`tests/tool-catalog-bridge.test.ts` "Gateway 的富结果、来源信息和多轮状态不经 Bridge 改写"）。
- MCP 结果统一经 `normalizeMcpToolResult`（`core/mcp/manager.ts`）；失败由 `mcpToolError` 归一为工具错误结果。
- 错误外壳是稳定 code 的 `ToolInvocationError`（错误码表见 `docs/architecture/tool-invocation-path-invariance.md`）；调用成功布尔值不遮盖失败：适配器错误原样传播，不转成普通文本成功（bridge 测试"Gateway 类型化错误原样传播"）。

## 2. 取消与迟到结果（A14）

- 网关在 executor 前后与归一化前后都检查取消/AbortError → 稳定 `EXECUTION_CANCELLED`（`tests/tool-invocation-gateway.test.ts` "取消在 canonical executor 前后都传播"）；迟到返回不改终态由 P02 状态机栅栏保证（STATE_TRANSITIONS §5 工具网关 lifecycleGeneration 行；`tests/tool-invocation-path-parity.test.ts` "preserves %s streaming updates and cancellation type"）。
- 工具级取消唯一面：`wrapWithSessionExecutionCancellation`（engine.ts:4624）经 SessionExecutionRegistry/killTree（P02 CANCELLATION_MAP）。

## 3. 超时：不自动重发，结果未知明确（A12）

- MCP Streamable 传输：`_postJsonRpc` 每请求 `fetchWithTimeout`（连接器 `timeout` 秒，默认 30s）；超时 → AbortError → 会话按非认证失败拆掉（交退避重连），**调用本身不重发**。唯一重放路径是 401+新令牌单次重试（无循环）。
- 证据：本阶段新增 `tests/mcp-http-client.test.ts` "does not re-send a non-idempotent call after a response timeout (P03-A12)"（外发替身计数恰 1、不触发 refresh、onClose expected:false needsAuth:false）；既有 "retries at most once…" / "does not retry a 401 when no refresh is possible"。
- 会话退避重连 ≠ 调用重放：重连后由上层（模型/用户）决定是否再次调用，工具层无自动重试。

## 4. 审计与脱敏

- 网关诊断日志：`{route, origin, targetId, sourceId, generation, code}`——target/schema 版本（generation）与调用裁决关联；不含参数值/秘密（gateway 测试断言用户路径与密钥样例不出现）。
- 高权限扫描类工具写独立审计账（`appendSecurityAuditEvent`：只记 action/result/mode/engines/计数，不进命中正文）。
- 凭证遮盖矩阵：`tests/tool-presentation.test.ts` A 组（块边界、嵌套凭证、设置值遮盖）。

## 5. 大结果与历史全文（A13）

- 预览/全文分离：首包只传计数/预览（"大搜索只传计数预览"），完整内容按**保存的记录**恢复（"详情按保存记录完整恢复"、"展开恢复所有文本块而不包含图片数据"）——`tests/tool-presentation-history.test.ts`。
- 大结果身份复用 Resource/SessionFile：写输出返回独立 SessionFile 身份与可写本地引用（`tests/resource-io-sandbox-tools.test.ts` "returns separate SessionFile identity…"）。
- 历史查看无新副作用：恢复不重新执行工具、不读当前文件冒充历史（"旧写入从调用参数恢复新正文，不捏造覆盖前内容"、"伪造条目身份…不能恢复详情"、"搜索结构引用不能跨会话解析（locator 不是授权凭证）"）。

## 6. 本阶段改动

- 测试：+1（A12）；生产代码零改动（UNCHANGED_VERIFIED）。
