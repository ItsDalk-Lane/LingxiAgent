# LEGACY_EXIT_LEDGER — 旧路径退出/保留总账（P08-T01）

日期：2026-09-22｜HEAD `674b0151f`（本轮终态候选）｜对照：P00 FEATURE_MATRIX §2 排除项与残留登记、P00-P07 各阶段交接遗留项。

字段按 92 模板 §5：旧文件/符号、原职责、现真实消费者、新去向、处置、必要保留理由、权限影响、测试、退出条件。

## 0. 本轮（P01-P07）生产改动全集与"零双写"结论

生产 diff = `92c6646c5..674b0151f`，剔除证据目录后共 36 文件（`git diff --name-status` 命令 P08-T01 全量留档于本报告引用的 git 对象，可随时复算）。要点：

- 新增运行时文件仅 3 个：`lib/tasks/task-identity.ts`（P02 taskId 铸造厂）、`shared/identity-brands.ts`（P01 品牌前缀）、`shared/hana-runtime-paths.d.cts`（P01 CLI 收口配套类型声明）。`scripts/check-core-contracts-strict.mjs`、`scripts/check-dependency-boundaries.mjs` 为检查器（非运行时），已接 CI（ci.yml 两步）。
- 零依赖变动（package.json 仅 +2 scripts：typecheck:core-contracts / check:dependency-boundaries；lockfile sha256 `a9735825…` 与 P00 完全一致）。
- 删除类改动：`tsconfig.json` -4 行（packages/* include glob 收窄——注意见 §2 L6，quality-gates 契约仍钉 packages glob 的另一处）、`git-ui-relocation-mockup.html`（删除，属 92c6646c5 前置 docs 提交非本轮）、无任何运行时文件删除——本轮为边界收敛型重构，不存在"新实现替换旧实现"类大规模退出，逐项见 §1/§2。
- 双写扫描：TaskRegistry 唯一（媒体任务双记账按 P02 §4.5 三域栅栏分工，非双写）；ToolInvocationGateway 唯一构造点 core/engine.ts:4119；createAgentSession 唯一出口 lib/pi-sdk/index.ts:78；正文裁决唯一 AssistantEventNormalizer（P05 收口）；canonical 装配唯一 buildSystemPromptArtifact（P06 收口）。无配置默认走旧实现、无失败隐式回退（各阶段门禁测试锁定）。

## 1. 本轮已退出的旧路径（处置=删除/收口完成）

| # | 旧路径 | 原职责 | 现真实消费者 | 新去向 | 处置 | 测试 |
|---|---|---|---|---|---|---|
| L1 | `cli/local-server.ts:5-15` 自写 LINGXI_HOME 解析（空值默认/~展开/resolve 三段重复） | CLI 侧 HOME 解析双实现（P00 OWNERSHIP_MAP 🔴） | cli/entry.ts、cli/chat.ts | import `shared/hana-runtime-paths.cjs` 的 `resolveLingxiHome`（CLI 仅保留 env 取值+trim） | **收口完成（P01-T07）**，本轮复核在位（cli/local-server.ts:3-11） | tests/cli-*.test.ts；P08 CLI 真实回归（T03 F14） |
| L2 | `vitest.config.js` `@hana/*` 别名、`tsconfig.test.json` `@lingxi/plugin-*` paths | 指向已拆除 packages/（04f90d2b2）的死别名 | 无（P01 清退前核实 tests/ 零引用） | `@` 别名指向有效路径 desktop/src/react 保留 | **删除（P01-T07）**，本轮复核：vitest.config.js:8-11 仅剩注释+有效 `@`；tsconfig.test.json paths 仅 `@/*` | typecheck 三腿 + 全量 vitest 绿（别名失效即红） |
| L3 | taskId 五种自铸格式（subagent/workflow/rewind/speech/image-task-runner 各自 `xxx:${uuid}` 拼接） | TaskRegistry-facing 任务身份无统一铸造（P00 🔴） | lib/tools/{subagent,workflow,rewind}-tool.ts、core/media-adapters/speech.ts、core/media/image-task-runner.ts | 全部改经 `lib/tasks/task-identity.ts` `mintTaskId`（attempt 语义在 TaskRegistry） | **收口完成（P02）**，本轮复核 5 文件仍 import mintTaskId | tests/task-identity-attempts.test.ts、tests/p02-*.test.ts（全量绿） |
| L4 | `lib/tools/run-code-tool.ts:118-119` no-control-regex、`lib/tools/security-scan-tool.ts:191` 恒真条件（F2 lint 3 error） | 既有质量门禁破损 | CI lint 门禁 | 行级精确豁免（ANSI ESC/BEL 即匹配目标，刻意）/明确语义改写 | **修复（P03）**，本轮复核：P08-T04-lint exit 0（全仓 0 error） | npm run lint（CI 每腿跑） |
| L5 | `skills2set/lingxi-plugin-creator/`（仅 `scripts/__pycache__/*.pyc` 两文件，无 SKILL.md） | 仓库残留（不可装载；electron-builder 从工作树打包会带入死字节） | 无（first-run syncSkills 跳过无 SKILL.md 目录：core/first-run.ts:263；git 未跟踪；生产零引用——tests 中同名仅合成 fixture） | 删除 | **本轮删除（P08-T01，P00 backlog P08-3）**，命令 P08-T01-skills-residue-cleanup | P08-T01-skills-tests：tests/skill-*.test.ts 定向全绿 |

## 2. 按兼容/授权理由保留的旧面（处置=仅读兼容/协议适配/待授权退役）

| # | 旧面 | 原职责 | 现真实消费者 | 处置 | 保留理由 | 权限影响 | 退出条件 |
|---|---|---|---|---|---|---|---|
| L6 | knowledge research 残留表（lib/knowledge/knowledge-store.ts:1021-1125 research_runs/research_jobs 等 9+ 张表建表 + types.ts 类型） | 已撤回的独立知识研究子系统数据兼容 | 旧库迁移链（v18/v19 migration tests）与建表路径；无任何运行时启动入口 | **仅读兼容保留**（P05-2 决策延续） | 表结构承载旧数据兼容；退役属需明确授权的治理变更（AGENTS.md：历史台账"退役"需另获授权） | 无新增授权面（不注册工具/路由） | 用户明确授权退役 + 旧数据导出迁移方案 |
| L7 | WS `text_delta` 兼容输出（P05 CONSUMER_MIGRATION §3） | 旧前端/客户端正文增量协议 | 旧客户端兼容 | **协议适配保留**（产品契约） | 已声明为产品契约的兼容面 | 无（只读投影） | 客户端基线更新后按产品节奏另行决策 |
| L8 | fork 克隆路径三处 store 域自铸克隆 ID（lib/subagent-run-store.ts:215,219；core/media/task-store.ts:223,227） | 会话 fork 时克隆 store 行的本地行 ID | subagent/workflow fork 与媒体 fork 克隆路径 | **登记保留**（P02 §4.5：非 TaskRegistry-facing 任务身份，是 store 行克隆 ID） | 语义不同（行 ID 非任务 ID），不进 taskId 铸造厂 | 无 | 若未来统一 store 行身份铸造再收口（范围外） |
| L9 | tsconfig.json `packages/*/src` include glob（被 tests/quality-gates-contract.test.ts 钉住） | 历史 packages 工作区遗留 include | quality-gates 契约测试 | **登记保留** | P01 交接 §3.5：退役该契约需用户授权；include 匹配不存在目录为无害空集 | 无 | 用户授权退役契约时一并清理 |
| L10 | `scripts/check-tool-invocation-boundaries.mjs` SOURCE_ROOTS 含 packages | 同上（collectSourceFiles 对不存在目录跳过） | 检查器自身 | **无害保留**（P00 已定性） | 无害；清退随 L9 一并 | 无 | 同 L9 |
| L11 | 旧正文兼容（canonical/legacy 并存，P05） | 历史消息正文读取兼容 | history-read 投影（extractPersistedAssistantSemanticSegments） | **仅读兼容保留** | 旧会话数据必须可读（红线：原始历史不可清空替代恢复） | 无 | 数据 epoch 全量翻新时（无计划） |

## 3. 明确不退出（经本轮复核为现行设计，非旧路径）

- X-MODEL-AUX 三旁路（embedding/rerank、call-text、summarizeTitle）：设计内非工具模型调用，观测包装完备（P04 OPERATION_COVERAGE_MATRIX 全矩阵 + P04-A01 防漂移测试绿）。
- Bridge 5 adapter / channel / DM / cron 入口：现行产品能力（F12/F15），非旧路径。
- P03 `prepareAndInvokeForLocalDeveloper` 与 route 类型（plugin-dev-http/chat）：消费面为测试与未来开发入口，删除属另行决策（P03 交接 §7）。
- 外装 MCP/插件（core/mcp/）：外装扩展边界，不计入内置功能数（P00 口径）。

## 4. 隐式回退扫描结论（T01.3）

- 无"仅改函数名规避检查"：本轮未重命名任何既有生产符号（生产 diff 全集核查）。
- 无"配置保持默认走旧实现"：新增能力（taskId 铸造、品牌前缀、strict 门禁）全部直连唯一实现，无 feature flag 双轨（本轮零新增 feature flag）。
- 无"失败时偷偷切回旧链"：错误处理均为显式（strictJson/ensureJsonObjectBody 拒绝、网关 fail-closed、resolver fail-closed——P04-A02 测试锁定取消不复活）。

## 5. 遗留移交（不在 P08 合法范围，继续登记）

1. F1 封印推进：HEAD 领先 `.sync-audit/verified-source-sha.txt`（adca95ca3）多个提交；推进需按 PROGRESS.md 封印工作流获用户授权并绑定候选提交（本阶段无提交授权，维持 BLOCKED 登记）。
2. H1 性能热点（server/session-stream-store.ts trimEvents splice(0,1) 头部移除 O(maxEvents)/append）：P05 锁定面承载文件，最小修复方案已成文（P07 HOTSPOT_REPORT §7.1 头偏移游标）；P08 任务书范围为旧路径退出/回归/发布准备，不含该性能改造，且该文件 P05 锁定 → **不实施**，移交后续授权。
3. P05 C1 消费端纵深防御缺口（use-stream-buffer 无 streamId 闸门、当前服务端不可达）：消息语义面归 P05 范畴，P08 不触碰；修复时 P05-ACCEPTANCE-counterexample.test.ts 的 it.fails 用例转红作回归提示。
