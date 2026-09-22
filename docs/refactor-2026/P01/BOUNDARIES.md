# BOUNDARIES — 模块职责边界与接口所有权（P01-T01）

版本：1.0｜证据基线：HEAD `92c6646c5`（与 P00 相同，无漂移）。
输入：P00 OWNERSHIP_MAP / ENTRYPOINT_MATRIX / CALLSITE_MATRIX + 本阶段逐文件复核（见 `artifacts/refactor-2026/P01/logs/`）。
本文件把 P00 所有权图落成六类**逻辑职责边界**与首批公共接口。它们是规则，不是"每类一个新 package"的拆分指令。

## 1. 六类职责与现有承载位置

| # | 职责类 | 现有承载（真实文件，均已核对存在） | 边界判据 |
|---|---|---|---|
| 1 | 传输入口（transport/entry） | `server/routes/chat.ts`（WS `:2256` + REST）、`server/index.ts:589-637` 鉴权中间件、`cli/chat.ts:347`、`lib/bridge/bridge-manager.ts:914/1995`、`hub/channel-router.ts:820`、`hub/dm-router.ts:239` | 只做协议解析、鉴权、门禁（session_busy 等）；不直接执行工具/调模型 |
| 2 | 应用编排（orchestration） | `core/session-coordinator.ts`（promptSession `:5176` / executeIsolated `:8048`）、`core/desktop-session-submit.ts:385`、`core/bridge-session-manager.ts:1162-1456`、`hub/agent-executor.ts:418-712`、`hub/scheduler.ts`、`core/engine.ts`（Manager 装配 `:642-817`） | 决定何时创建 session、复用 trace、终态推进；模型循环本身交给 Pi |
| 3 | 业务规则（domain rules） | `core/tool-invocation-gateway.ts`（prepared invocation + canonical 执行 `:379/:469`）、`lib/tools/session-permission-wrapper.ts:656-764`、`lib/task-registry.ts:114`、`lib/session-files/session-file-registry.ts:868`、`core/session-manifest/id.ts:5-9` | 纯裁决/铸造/守恒规则；无 HTTP/WS/Electron 依赖 |
| 4 | SDK/供应商适配（adapter） | `lib/pi-sdk/index.ts`（**唯一** Pi 入口，createAgentSession `:78`）、`core/provider-compat/*`、`core/model-operation-resolver.ts:103`、`core/llm-client.ts`（callText 旁路，已登记 P04） | 唯一允许 import `@earendil-works/*` 的位置；吸收版本差异 |
| 5 | 存储（storage） | `core/session-manifest/store.ts:181`（sess_ 权威）、`lib/llm/model-call-recorder.ts:107/:239`（observability.sqlite v7）、`core/provider-catalog.ts:15`、`shared/persistence/store-registry.ts`（~70 store 指纹）、device-registry（devices.json 密文） | 每类事实单一写入者（P00 OWNERSHIP_MAP 18 行已核对）；索引可重建、原始不可 |
| 6 | 桌面宿主（desktop host） | `desktop/main.cjs`（spawn 单点 `:1959`、LINGXI_HOME `:168`）、`desktop/preload.cjs`（window.hana 桥）、`desktop/src/react/**`（renderer，经 `composer-send.ts:577` WS） | 唯一允许 import `electron` 的层；业务消息不走 IPC（P00 已核实） |

## 2. 首批公共接口清单（全部为**已存在** API，无新增抽象）

| 接口 | 位置 | 实际消费者（抽样核对） | 接收内容约束 |
|---|---|---|---|
| `createAgentSession(options)` | `lib/pi-sdk/index.ts:78` | session-coordinator.ts:2214、bridge-session-manager.ts:1305、agent-executor.ts:552 | 只收 model/modelRuntime/sessionManager/resourceLoader/customTools 等任务字段；**不接受 engine/agent/config 大对象**（源码头注纪律已声明） |
| `ToolInvocationGateway.invoke(prepared)` | `core/tool-invocation-gateway.ts:379` | engine.ts:459 runWithPreparedInvocation | prepared invocation 由 `createPreparedInvocation`（gateway:268）绑定 toolCallId/sessionId/agentId/generation，调用方不可自铸 |
| `resolveLingxiHome(input)` | `shared/hana-runtime-paths.cjs:13`（TS 门面 `shared/hana-runtime-paths.ts`） | server/index.ts:252、desktop/main.cjs:168；P01-T07 后 cli/local-server.ts 统一接入 | 纯字符串→路径，无 I/O、无 env 直读（env 由调用方传入） |
| `mintModelCallId()/mintModelAttemptId()` | `lib/llm/model-call-identity.ts:24-39` | model-call-recorder.ts:107/:239 | 唯一 `mc_`/`ma_` 铸造厂；无状态 |
| `ModelTraceScope.runWithModelTraceRoot/runWithNewModelTrace` | `lib/llm/model-trace-scope.ts:117/:156` | session-coordinator.ts:5032、scheduler.ts:234、agent-executor.ts:418 | trace 复用/新根规则单一承载；不解析消息语义 |
| `SessionManifestStore` | `core/session-manifest/store.ts:181` | engine.ts:2039 打开；resolver/trace-store 只读 | sessionId `sess_{ts36}_{rand20}` 唯一铸造（id.ts:5-9） |
| `resolveHttpRequestPrincipal` | `server/http/request-principal.ts` → `core/server-auth.ts:35-90` | server/index.ts:626 | loopback token/device credential/web cookie → principal；模型参数不能构造 |

## 3. Pi 执行状态与产品任务状态的映射方向

**方向：事件 Pi→产品，控制 产品→Pi。** 具体规则：

1. **控制流（产品→Pi）**：产品侧经 `session.prompt/abort/steer` 驱动；重试与工具轮次由 Pi AgentLoop 内部决定。**禁止在 session-coordinator/bridge/agent-executor 再写一层"模型循环重试/工具执行"决策**——那是第二个 Agent 循环（本轮红线）。
2. **事件流（Pi→产品）**：Pi 的 stream 事件经 `lib/pi-sdk/model-call-stream-observer.ts` + `assistant-stream-guard` 等观察者进入产品观测（mc_/ma_/mt_ 由 lib/llm 铸造，P00 ✅）；产品的 runId 状态机（chat.ts:868/910 exactly-once）只消费投影，不回写 Pi。
3. **状态语义分界**：runId（内存，一次用户输入→agent_settled）≠ mt_ 观测分组（会话粒度复用）≠ SDK session UUID（JSONL 文件名）。三者不得互换使用（P00-A07 已核实 traceId 恒不等于 sessionId）。
4. **取消**：产品 abortSession 是唯一取消入口（三个补充面均不绕权限）；Pi 的工具 signal 由 SDK 从 session.abort 派生，产品不自建第二取消通道。

## 4. 记录在案的禁止反向依赖（P01-T06 检查器落规则）

| 禁止方向 | 现状（HEAD 实测） | 执行手段 |
|---|---|---|
| `core/`、`server/`、`hub/`、`cli/`、`lib/`（pi-sdk 外）、`shared/` import `@earendil-works/*` 或 `@mariozechner/*` | 0 处（全仓 grep + AST 复核） | T06 规则 `sdk-direct-import` |
| 适配层外 import `node_modules/@earendil-works/**` 深路径 | 生产 0 处；合法例外=lib/pi-sdk 内 2 处（auth-storage、compaction）+ 构建/维护脚本 2 个（sync-known-models-from-pi.mjs、compute-cli-closure.mjs）+ 1 处注释（qwen.ts:16） | T06 规则 `sdk-deep-path`（精确例外） |
| `core/`、`lib/`、`server/`、`hub/`、`shared/`、`cli/` import `electron` | 0 处（grep 实测） | T06 规则 `host-into-core` |
| `lib/pi-sdk/**` import `core/**`、`server/**` | 0 处（依赖只向 lib/llm 与自身） | T06 规则 `adapter-reverse-dep` |
| 网关执行旁路（executeCanonical/callTool/executePluginTool 越白名单） | 0 处 | 既有 `scripts/check-tool-invocation-boundaries.mjs`（S08，继续有效，不重复建设） |

## 5. 任务完成检查对照

- 每个接口有实际消费者：§2 表消费者均为生产调用点（P00 CALLSITE_MATRIX 逐跳核对），无无人调用的新抽象层。
- 本任务**零生产代码改动**（纯边界固化文档）；落地执行在 T06（检查器）与 T07（纵向链）。
