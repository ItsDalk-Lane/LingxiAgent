调查完成。以下为全部原始证据。

---

# P00 既有测试基线映射 — 原始证据报告

## 1. tests/ 目录清单与总数（按业务域分组）

**总量结论**：`tests/` 树内共 **998** 个测试文件（`find tests -name '*.test.{ts,tsx,js}' | wc -l` = 998），其中顶层 963 个（960 `.ts` + 2 `.tsx`（`model-observability-detail-vertical.test.tsx`、`viewer-window-entry.test.tsx`）+ 1 `.js`（`python-skill-env-check.test.js`）），4 个子目录 35 个：`tests/loop/`(9)、`tests/provider-compat/`(16)、`tests/slash-commands/`(9)、`tests/manual/`(1)。另有非测试支撑：`fixtures/`(11，含 `system-prompt-golden-zh/en.txt`、SQL 迁移夹具)、`helpers/`(9 个 fixture harness)、`setup-auto-updater.ts`、`style-discipline-baseline.json`、`README.md`（测试政策文档，定义 contract/regression/unit/route/build/platform 六层）。

**tests/ 之外还有 448 个测试文件**会被 `npm test` 一并收集（vitest 默认 include，未被 exclude）：`desktop/src/react/**__tests__/**` 约 441 个（如 `__tests__/components` 149、`__tests__/utils` 50、`__tests__/stores` 32）+ `lib/tools/__tests__`(3) + `lib/sandbox/__tests__`(1) + `lib/exec-command/__tests__`(1) + `desktop/src/__tests__`(1) 等。全仓 vitest 实际收集 ≈ **1446** 个测试文件。

**顶层 963 个按域名前缀分类**（awk 首匹配规则计数，命令模式：`ls tests/*.test.* | xargs -n1 basename | awk '{if(n~/^tool-|…/)d="…"} …}'`，结果已人工核对）：

| 业务域 | 文件数 | 代表性文件 |
|---|---|---|
| 消息/历史/流 | 126 | `history-read-directory-*.test.ts`(9 个系列)、`session-stream-store`、`assistant-event-normalizer`、`chat-route-*`、`reserved-tag-*`、`session-jsonl*` |
| 模型/凭证 | 110 | `model-call-payload-*`(11)、`model-observability-*`(28)、`provider-compat*`、`pi-sdk-*`(8)、`secret-*`、`oauth-*`、`known-models*` |
| 桥接/通道 | 86 | `bridge-*`(27)、`channel-router-*`(6)、`server-*`(约 20)、`ws-*`(4)、`feishu/dingtalk/wechat/qq/telegram-*` 适配器 |
| 工具网关 | 70 | `tool-invocation-*`(6)、`mcp-*`(9)、`browser-*`(8)、`terminal-*`(4)、`lsp-tool`、`ask-user-tool`、`slash-command-*` |
| 桌面/UI | 68 | `desktop-*`(24)、`preferences-*`(7)、`theme-registry*`、`i18n-*`(3)、`style-discipline` |
| 知识库 | 61 | `knowledge-*` 全前缀（chunker/ann/vector/migration/rerank/search 等） |
| 资源/文件 | 58 | `resource-io-*`(16)、`file-history-*`(6)、`workspace-*`(10)、`fs-route`、`mount-aware-file-service` |
| 任务/生命周期 | 48 | `task-registry`、`execution-lease-registry`、`agent-executor-teardown`、`workflow-*`(13)、`deferred-result-*`(6)、`data-epoch*` |
| 构建/产物 | 47 | `build-server-*`(6)、`artifact-core-*`(7)、`release-*`(5)、`windows-installer-contract`、`smoke-packaged-*` |
| 上下文/提示词 | 30 | `agent-system-prompt-*`(2)、`platform-prompt`、`compaction-*`、`cache-prefix-contract*`、`output-length-contract`、`session-compactor*` |
| 平台 | 26 | `win32-*`(10)、`sandbox-policy`、`seatbelt/bwrap-sandbox-policy`、`mac-self-install`、`python-skill-env-check` |
| 审计/seal | 13 | `post-verification-audit-seal`、`upstream-sync-matrix`、`persistence-schema-tripwire`（后两个名字不含 audit/seal，人工归入）、`security-audit-log`、`injection-scan`、`safety-policy` |
| 其他 | 220 | 聚类：`agent-*`(28)、`media-*`(24)、`memory-*`(19)、`speech-*`(11+2)、`computer-use-*`(12)、`cli-*`(9)、`round2/round3-*`(9)、`skill-*`(6)、`plugin-*`(4)、`checkpoint*`(3)、`persistence-*`(4，除 tripwire)、`jimeng-cli-*`(4)、`subagent-*`(5)、`yuan-*`(3) 等 |

（计数规则首匹配即归类，跨域文件如 `session-teardown` 归入消息/历史/流；合计 963，`sort | uniq -c` 可复核。）

## 2. 重点测试"被测生产入口"核实

计数命令：`grep -c 'describe('` 与 `grep -cE '\bit\(|\btest\('`；skip 判定 `grep -cE '\.(skip|todo|only)\(|\bxit\(|\bxdescribe\('`。

| 测试文件 | import 的生产模块（绝对路径省略前缀 /Users/study_superior/Desktop/Code/LingxiAgent/） | describe | it | skip/todo |
|---|---|---|---|---|
| `tests/tool-invocation-gateway.test.ts` | `core/tool-invocation-gateway.ts`(ToolInvocationGateway)、`core/tool-target-registry.ts`、`lib/tools/invocation/index.ts`(createFirstPartyToolIdentity / createToolSchemaValidator / getPreparedInvocation / normalizeToolPermissionContract / runWithPreparedInvocation / ToolInvocationError)（行 2-11） | 1 | 10 | 0 |
| `tests/history-run-outcome-edges.test.ts` | `vi.mock` 了 `desktop/src/react/hooks/use-hana-fetch.ts`（行 22）；动态 import（行 30-32、115）`desktop/src/react/stores/index.ts`、`desktop/src/react/stores/session-slice.ts`、`desktop/src/react/utils/history-builder.ts`、`server/routes/sessions.ts`(createSessionsRoute，真实 Hono app) | 1 | 6 | 0 |
| `tests/agent-executor-teardown.test.ts` | `vi.mock` 了 `lib/pi-sdk/index.js`（createAgentSession/SessionManager.create/open 部分替换，行 18）；真实 import（行 32-35）`hub/agent-executor.ts`(runAgentSession/runAgentPhoneSession)、`lib/conversations/agent-phone-projection.ts`、`agent-phone-runtime.ts`、`agent-phone-session.ts` | 1 | 17 | 0 |
| `tests/agent-system-prompt-equivalence.test.ts` | `vi.mock` 了 `core/platform-prompt.ts`（行 11，固定 FIXED-PLATFORM-NOTE-LINE）；真实 import（行 13-18）`core/agent.ts`(Agent)、`lib/agent-appearance-summary.ts`；golden fixture `tests/fixtures/system-prompt-golden-{zh,en}.txt`（行 95-96） | 1 | 2 | 0 |
| `tests/post-verification-audit-seal.test.ts` | 无生产模块 import（仅 node:child_process/fs/path/url + vitest，行 20-24）；被测对象 = `.sync-audit/verified-source-sha.txt` + `git diff --name-only` + audit allowlist | 1 | 3 | 0 |
| `tests/upstream-sync-matrix.test.ts` | 无生产模块 import（crypto/fs/path + vitest）；被测对象 = `.sync-audit/delta-U-final.txt`、`.sync-audit/upstream-sync-matrix.json`、`UPSTREAM_SYNC_MATRIX.md` 投影哈希（行 21-24，已确认三文件均存在） | 1 | 7 | 0 |
| `tests/persistence-schema-tripwire.test.ts` | `scripts/generate-persistence-schema-fingerprint.mjs`（8 个导出函数）与 `scripts/scan-persistent-stores.mjs`(PRODUCTION_ROOTS/scanPersistentStores)（行 13-24）；依赖 `typescript` AST + `build/persistence-schema-fingerprint.json`、`build/persistence-store-inventory.json` | 1 | 15 | 0 |
| `tests/model-trace-scope.test.ts` | `lib/llm/model-trace-scope.ts`（行 16-27，10 个导出：MODEL_TRACE_ORIGINS/currentModelTraceScope/resolveModelTraceContext/runWith*/runToolExecutionWithModelTrace/noteAgentStreamCallStarted 等） | 7 | 21 | 0 |
| `tests/session-stream-store.test.ts` | `server/session-stream-store.ts`（行 2-7：createSessionStreamState/beginSessionStream/finishSessionStream/appendSessionStreamEvent/resumeSessionStream） | 1 | 9 | 0 |
| `tests/task-registry.test.ts` | `lib/task-registry.ts`(TaskRegistry)（行 5）；用 `vi.useFakeTimers`，真实 os.tmpdir 持久化 | 1 | 16 | 0 |
| `tests/loop/task-registry-active-query.test.ts` | `lib/task-registry.ts`(TaskRegistry + ACTIVE_TASK_STATUSES)（行 2） | 1 | 2 | 0 |
| `tests/assistant-event-normalizer.test.ts` | `server/assistant-event-normalizer.ts`(AssistantEventNormalizer)（行 2） | 1 | 9 | 0 |
| `tests/execution-lease-registry.test.ts` | `core/execution-lease-registry.ts`（行 5-10：ensureExecutionLeaseRegistry/issueExecutionLease/consumeExecutionLease/revokeExecutionLease）；真实临时目录 fs | 1 | 4 | 0 |

全部 13 个生产模块路径已逐一 `ls` 确认存在。结论：**所有重点测试零 skip/todo/only**。

## 3. 运行配置

**`vitest.config.js`**（全文 38 行）：
- 无 `include` → 使用 vitest 默认 `**/*.{test,spec}.?(c|m)[jt]s?(x)`（即 tests/ 外的 448 个 desktop/lib 测试也进默认套件）。
- `exclude` = configDefaults + `.cache/**`（注释：worktree 副本双份执行）+ `.claude/worktrees/**` + `desktop/native/**/.build/**` + `dist-computer-use/**`。npm script `test` 再叠加排除 `**/dist/**`、`**/dist-server/**`、`**/dist-sandbox/**`（package.json:63）。
- 全局：`testTimeout/hookTimeout: 60_000`（注释：慢 I/O CI runner 放宽）；`setupFiles: ["./tests/setup-auto-updater.ts"]`；`server.deps.inline: ["electron-updater", /desktop\/auto-updater/]`。
- alias：`@hana/plugin-*` → `packages/plugin-*/src/index.ts` —— **`packages/` 目录不存在**（`ls: No such file or directory`），疑似上游残留死别名（tsconfig.test.json 里对应 `@lingxi/plugin-*` 同样指向 `packages/...`）。

**`tsconfig.test.json`**（27 行）：extends `tsconfig.base.json`；`jsx: react-jsx`；**放宽严格性**（strict:false、noImplicitAny:false、strictNullChecks:false、strictPropertyInitialization:false、useUnknownInCatchVariables:false）；paths `@lingxi/plugin-*`（指向不存在的 packages/）与 `@/* → desktop/src/react/*`；include 仅 `tests/**/*.ts`、`tests/**/*.tsx`、`desktop/src/global.d.ts` —— **不覆盖 tests/ 之外的 448 个测试文件**（desktop/react、lib 的 __tests__ 不在 test 型检查范围内）。

## 4. skip/todo 统计（grep 全 tests/ 树）

| 模式 | 次数 | 位置 |
|---|---|---|
| `.skip(` | 0 | — |
| `it.todo` / `test.todo` / `describe.todo` | 0 | — |
| `.only(` | 0 | — |
| `xit(` / `xdescribe(` | 0 | — |
| `describe.skipIf(` 或条件 `describe.skip` | 7 行 / 7 文件 | skipIf：`credential-file-healer:39`(POSIX)、`mac-self-install:196`(isMac)、`history-protocol-compat:123`(cloneReady，真实 HTTP 四组合矩阵)、`plugin-config:11`(非 win32)；条件 skip：`sandbox-tool-wrapper:7`(win32 跳)、`tests/manual/win32-packaged-smoke:14`(SMOKE_ENABLED 环境门控) |
| `skipIf`（含 it.skipIf） | 25 次 / 12 文件 | `secret-fs` 12 次最多；其余 11 文件各 1-2 次（pinned-tenets-migration、bridge-media-roots、provider-catalog、session-file-registry、model-observability-blob、upload-route、config-loader 等） |

结论：**无任何硬 skip/todo/only**；仅有的跳过全部是平台/环境条件门控（win32/POSIX/mac/显式 smoke 开关）。

## 5. 负例/对抗类测试现状

- 按 `ls tests | grep -Ei 'negative|forbidden|boundary|adversarial'`：**无 negative/forbidden/adversarial 命名文件**；有 **11 个 `*boundary*`**：`tool-invocation-boundary`、`execution-boundary`、`server-composition-boundary`、`open-boundary-lint`、`pi-sdk-credential-boundary`、`pi-sdk-import-boundary`、`model-manager-static-boundary`、`remote-execution-boundary`、`resource-io-authority-boundary`、`temporary-provider-credential-boundary`、`engine-vision-slot-boundary`。
- 无 attack/poison/unsafe/tamper 命名（grep 为空）。
- 内容级对抗断言散在：`round3-c01-input-rejection`、`injection-scan`、`todo-prompt-injection`、`extract-zip-symlink-defense`、`http-route-security`、`media-contract-hardening` 等。即：**"边界/安全"作为命名惯例存在（boundary 系列 11 个），但无系统性 negative/adversarial 测试命名层**。

## 6. scripts/ 检查类脚本（15 个，目录共 82 项）

| 脚本 | 用途（来自文件头注释） |
|---|---|
| `scripts/check-persistence-schema-fingerprint.mjs` | 快速指纹 repin 守卫：只比对 diff，不解析 TS/不开库（权威检查在 persistence-schema-tripwire.test.ts） |
| `scripts/check-tool-invocation-boundaries.mjs` | 基于 TypeScript AST 的工具调用边界检查（导出 EXACT_BOUNDARY_ALLOWLISTS） |
| `scripts/verify-seed-kit.mjs` | 嵌入前 fail-closed 校验 seed 四件套，插在 build:server 与 electron-builder 之间 |
| `scripts/verify-standalone-server-artifact.mjs` | Windows HanaCore 独立发布产物的 fail-closed 校验（打包后、CI 上传前） |
| `scripts/smoke-history-protocol.mjs` | E08 真实链路 smoke：Node HTTP loopback 打真实 Hono 路由，断言 200/304/ETag 头 |
| `scripts/smoke-mingit.mjs` | Windows 上验证 MinGit runtime 跑非交互 git 全流程 + POSIX shell（真实二进制） |
| `scripts/smoke-open-server.mjs` | spawn-and-verify：拉起 open composition 构建的 server 并验证 |
| `scripts/smoke-packaged-desktop.mjs` | 打包后桌面包 smoke（child_process 拉起验证） |
| `scripts/smoke-packaged-knowledge.mjs` | 启动闭集服务器种子两次，证明 Knowledge 在真实包内建库/重启恢复 |
| `scripts/smoke-skill-ws.mjs` | WS 层纯技能消息门禁 smoke（chat.ts 消息门禁曾漏 skills 类型） |
| `scripts/smoke-windows-sandbox-helper.mjs` | Windows sandbox helper 真实二进制 spawn 验证 |
| `scripts/benchmark-history-protocol.mjs` | D07 真实传输协议基线：loopback TCP 代理注入确定性延迟/带宽整形 |
| `scripts/benchmark-history-read-directory.mjs` | A05 历史分页读取基准（真实路由 + 真实 v3 夹具，独立入口不进 vitest） |
| `scripts/benchmark-knowledge-vector.mjs` | 知识向量检索性能基准（真实 vector-index-adapter/backend，DIM=64） |
| `scripts/benchmark-terminal-ui.mjs` | 终端 UI 性能基准（monitorEventLoopDelay + 真实 TerminalSessionManager/terminal-ws-bridge） |

相邻同类（非四前缀但属检查类）：`lint-open-boundary.mjs`、`style-discipline.mjs`、`test-inventory.mjs`、`release-preflight.mjs`、`session-path-identity-audit.mjs`、`scan-persistent-stores.mjs`、`generate-persistence-schema-fingerprint.mjs`（后两个本身是被测生产入口，见上表）。

## 7. 生产契约 → 测试文件映射表

| 生产模块 | 测试文件 | 层级 | mock 的外部边界 |
|---|---|---|---|
| `core/tool-invocation-gateway.ts` + `core/tool-target-registry.ts` + `lib/tools/invocation/index.ts` | `tests/tool-invocation-gateway.test.ts` | contract（网关单链路） | 无模块级 mock；注入 vi.fn 的 canonical 执行器/授权器（函数级 fake） |
| `server/routes/sessions.ts` + `desktop/src/react/utils/history-builder.ts` + `stores/session-slice.ts` | `tests/history-run-outcome-edges.test.ts` | route+contract（真实路由+真实投影+真实合并） | mock `use-hana-fetch`（前端取数层）；Hono 用 app.request 内存调用，不走网络 |
| `hub/agent-executor.ts` | `tests/agent-executor-teardown.test.ts` | contract（teardown 生命周期） | 部分 mock `lib/pi-sdk/index.js`（createAgentSession、SessionManager.create/open）；phone projection/runtime 为真实模块+真实临时目录 |
| `core/agent.ts` + `lib/agent-appearance-summary.ts` | `tests/agent-system-prompt-equivalence.test.ts` | contract（golden 等价） | mock `core/platform-prompt.ts`（抹平 $SHELL/os 版本）；golden fixture zh/en 双语 |
| `.sync-audit/verified-source-sha.txt` + git（无生产模块） | `tests/post-verification-audit-seal.test.ts` | build/audit 门禁 | 无 mock；真实 `git diff --name-only` |
| `.sync-audit/upstream-sync-matrix.json` + `delta-U-final.txt`（无生产模块） | `tests/upstream-sync-matrix.test.ts` | build/audit 门禁（Gate A） | 无 mock；纯文件读取+crypto 哈希 |
| `scripts/generate-persistence-schema-fingerprint.mjs` + `scripts/scan-persistent-stores.mjs` | `tests/persistence-schema-tripwire.test.ts` | contract（tripwire） | 无 mock；真实 SQLite（tmpdir）+ typescript AST + build/ 指纹 |
| `lib/llm/model-trace-scope.ts` | `tests/model-trace-scope.test.ts` | unit | 无 mock；AsyncLocalStorage 真实并发隔离 |
| `server/session-stream-store.ts` | `tests/session-stream-store.test.ts` | unit | 无 mock；纯内存状态机 |
| `lib/task-registry.ts` | `tests/task-registry.test.ts`；`tests/loop/task-registry-active-query.test.ts` | unit | vi.useFakeTimers；真实临时目录 JSON 持久化 |
| `server/assistant-event-normalizer.ts` | `tests/assistant-event-normalizer.test.ts` | unit | 无 mock |
| `core/execution-lease-registry.ts` | `tests/execution-lease-registry.test.ts` | unit | 无 mock；真实临时目录 fs |

**附加风险证据**（供 P00 参考）：(a) `vitest.config.js`/`tsconfig.test.json` 的 `@hana/*`/`@lingxi/plugin-*` 别名指向不存在的 `packages/` 目录；(b) `tsconfig.test.json` include 仅覆盖 `tests/`，tests/ 外 448 个 desktop/lib 测试不在其范围内；(c) `npm test`（package.json:63）实际收集全仓 ≈1446 个测试文件，基线统计若只看 tests/ 会漏掉 desktop/react 一侧。