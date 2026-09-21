# PI_ADAPTER_INVENTORY — Pi 唯一适配入口与升级边界清点（P01-T02）

版本：1.0｜证据基线：HEAD `92c6646c5`。锁定版本实测：`@earendil-works/{pi-coding-agent,pi-ai,pi-agent-core}` 全部 `0.86.0`（node_modules 实读），在 `scripts/patch-pi-sdk.cjs` 的 verifiedVersions 集合内。

## 1. 适配面库存（lib/pi-sdk/index.ts，全部既有 API）

| 类别 | 导出 | 生产消费者（核对点） |
|---|---|---|
| 会话创建 | `createAgentSession`（:78，含 6 个观察者安装顺序：tool-outcome → stream-guard → model-call-stream-observer → trace-ingress → desktop-input-commit → todo-reminder） | session-coordinator.ts:2214、bridge-session-manager.ts:1305、agent-executor.ts:552（P00 X-MODEL-CHAT 唯一入口） |
| 会话/历史工具 | SessionManager、SettingsManager、calculateContextTokens、parseSessionEntries、buildSessionContext、serializeConversation 等 | 历史/压缩链 |
| 模型装配 | `createModelRuntime`（:377，三层包装：serialized refresh → legacy availability scoping → Lingxi credential boundary）、`createModelRegistry`（:501）、register/unregisterModelProvider | model-manager、providers 路由 |
| OAuth | `SdkAuthFacade`、`loginOAuthProvider`（:151，0.83.0 AuthInteraction 适配） | server/routes/auth.ts |
| 工具工厂 | createRead/Write/Edit/Bash/Ls/Grep/FindTool、`PI_BUILTIN_TOOL_NAMES` | 沙盒工具装配 |
| 观测接线 | installModelCallStreamObserver（经 createAgentSession 自动安装） | observability.sqlite v7 链 |
| 适配函数 | resizeModelImageInput、formatModelImageDimensionNote、emitSessionShutdown、refreshSessionModelFromRegistry、generateSummary（观测包装 :241） | 各自唯一调用点 |

**判定：UNCHANGED_VERIFIED。** 本阶段未修改 lib/pi-sdk 任何文件；回归证据 `P01-T02-pi-sdk-regression.out`（9 文件 41 用例绿，含真实锁定 SDK + witness 协议 server 的 e2e-chat，即 P01-A02 场景）。

## 2. 全仓直接导入扫描（含动态 import / require / 构建脚本）

扫描方法：regex 同形于 patch-pi-sdk.cjs 验证器（`from`/`import()`/`require()`/副作用 import 四形态），文件面 `.ts/.tsx/.js/.mjs/.cjs`，目录面 core/server/lib/hub/cli/shared/plugins/desktop（本轮起含 .ts 与全部生产根，见 §4 盲区修复）。

| 发现 | 位置 | 定性 |
|---|---|---|
| `@earendil-works/*` 包名直接导入 | **0 处**（适配层外） | 合规 |
| `../../node_modules/@earendil-works/.../auth-storage.js` 深路径 | lib/pi-sdk/index.ts:25-28、lib/pi-sdk/auth-facade.ts:32（type-only） | **合法例外**：0.83.0 起包根不再导出，适配层注释已声明存在性检查义务（升级时必查） |
| `.../dist/core/compaction/compaction.js` 深路径 | lib/pi-sdk/index.ts:52-54 | **合法例外**：prepareCompaction 0.80.3 起未从包根导出，同上 |
| `../node_modules/@earendil-works/pi-ai/dist/models.generated.js` | scripts/sync-known-models-from-pi.mjs:28 | **合法例外**：维护脚本（known-models 同步），不在生产运行面 |
| `node_modules/@earendil-works/...` 文件清单引用 | scripts/compute-cli-closure.mjs:424-444 | **合法例外**：CLI 闭包构建脚本（打包清单数据） |
| 注释中的深路径提及 | core/provider-compat/qwen.ts:16 | 非导入（文档注释），不需例外 |

## 3. postinstall 补丁现状

`scripts/patch-pi-sdk.cjs` 已是**只读验证器**（0.68+ 后不再写 node_modules）：校验 pi-coding-agent/pi-ai 版本 ∈ {0.80.3, 0.83.0, 0.84.1, 0.86.0}、SDK dist/index.js 导出标记、以及生产 import 边界（扫 .ts/.js，core/server/lib/hub）。文件名保留 patch- 前缀是为了不动 postinstall 钩子（npm cache）。**无需恢复写补丁，无需删除。**

## 4. 本阶段实际修改（P01-T02）

1. `tests/pi-sdk-import-boundary.test.ts`：walk 收集面 `.js/.mjs/.cjs` → 增加 `.ts/.tsx`；SCAN_DIRS 增补 cli/shared/plugins/desktop。**盲区依据**：生产实现几乎全为 .ts，原测试对它们不设防（仅 postinstall 验证器兜底，且后者的扫描根不含 cli/shared/plugins/desktop）。
2. 盲区关闭后新发现一处：`lib/tools/invocation/schema-validator.ts` import `typebox`（根模块，type-only TSchema）与 `typebox/value`（运行时 Value）。**定性**：typebox 是根级直接依赖（package.json:121，1.1.38），该文件是网关参数 schema 校验基础设施（tests/tool-schema-validator.test.ts 全覆盖），非 Pi SDK 旁路。处置：登记为 typebox 的**唯一精确例外**（构造面 `Type` 仍须经适配面 re-export）；是否把 `Value` 也吸收进适配面 re-export，属适配面 API 变更，移交 P04（模型/凭证阶段触及 schema 边界时）或 P08 评估——本阶段不改适配层（P00 交接约束）。

## 5. 升级边界规则（后续 SDK 升级时执行）

1. 只允许改 lib/pi-sdk/**；业务侧出现新的 `@earendil-works` 需求时，先在适配层加适配函数再供业务导入（T06 检查器 `sdk-direct-import` 规则强制）。
2. 两处深路径例外在升级日**必须先验证存在性与行为**（源内注释已声明）；`scripts/patch-pi-sdk.cjs` 的 verifiedVersions 需人工确认后追加新版本。
3. createAgentSession 的观察者安装顺序受 `tests/pi-sdk-create-session-adapter.test.ts`（4 用例）锁定——新增观察者须同步该测试，不得漏装/双装。
