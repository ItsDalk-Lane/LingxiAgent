# P06 独立验收报告（验收子代理，全新上下文）

日期：2026-09-22｜验收对象：P06 工作区交付（HEAD `93b8b72650846af1a150eb195c1765c8f57f6cd8` + 未提交改动）｜角色：只读独立验收，不修改生产代码/测试/文档（本报告与验收证据日志为模板允许的唯一写入）。全部结论基于本验收独立复跑、源码逐点核对与原始日志审阅，未采信执行者自报。

## 结论

**阶段总状态：PASS（确定性契约面）＋ BLOCKED（真实模型行为评测）——与执行者判定一致且经独立核实成立。**

- T01–T04、T07 的确定性契约面与 UNCHANGED_VERIFIED 关键定性全部由本验收独立复现或逐点对上源码；T05/T06 的确定性部分（评测集 23 例 + mapped 7 + 完整性/BLOCKED 声明）经真实生产组件复跑全绿。
- BLOCKED 正当性核实：任务书 §5 T05 第 4 项明文「没有授权则该验证BLOCKED」；TOOL_BEHAVIOR_EVAL.json `budget_authorization_record.real_model_runs.status=BLOCKED` 与编排层「无凭证/费用授权」事实一致，未发现以确定性结果冒充真模型成功率的行为（`no_judgment_on` 含「内部思维/思考文本」「模型自称完成」）。
- 反例/负向注入 3 组真实执行（隔离 git worktree，零残留）：FIX-1 修复前评测红、golden 字节突变红、装配快照单字符保真破坏红——测试断言承重性成立。
- 2 个非阻塞发现（见 §6）：① FIX-1 的「嵌套路径降级为 /」缺陷前提与本仓 typebox@1.1.38 实测不符（文档/注释准确性问题，代码无害）；② R10-09 在特定调度顺序下可作第 5 红出现（P04 起已知 f1 patch 重写现象的顺序依赖波动，非 P06 引入）。
- f1 patch 重写副作用在本验收复现并已还原（与执行者 CMD-20 一致）。

## 1. 改动面与最小性声称

- `git status` / `git diff HEAD --stat`：仅 `lib/tools/invocation/schema-validator.ts`（+41/−6）与 `docs/refactor-2026/P00/REFACTOR_BACKLOG.md`（+2 注记）两个修改文件；未跟踪新增 = 4 个 p06 测试 + docs/artifacts 的 P06 目录。与 P06_REPORT §差异逐项一致，无未申报改动。
- schema-validator.ts diff 逐 hunk 审阅：① `normalizeIssues` 入参类型扩为 `{path?, instancePath?, message?}`，取值优先 `path` 回退 `instancePath` 再回退 `/`；② 新增 `summarizeIssueFields`（非 root 路径直取；root 必填缺失从 message 保守正则提取属性名）；③ `ARGUMENT_SCHEMA_INVALID` message 追加 `Invalid field(s): <列表>`。**未触碰**：`Value.Check` 判定、错误码集合（`errors.ts` 零 diff）、`PASSTHROUGH_INVOCATION_ERROR_CODES` 白名单、`TOOL_SCHEMA_META_SCHEMA`、`ROOT_SCHEMA_ISSUE`、details 结构。校验语义/错误码/白名单不变——属实。
- 「模型只见 error.message」论据属实，双路径核实：① pi-agent-core `agent-loop.js` prepare/execute 两处 catch 均 `createErrorToolResult(error instanceof Error ? error.message : String(error))`，details 不到调用方；② `lib/permission/tool-invocation-permission.ts:914` 透传仅取 `error.code/message/details` 中 message 进入调用方可见反馈。
- FIX-1 前后行为复现（隔离 worktree HEAD vs 工作区，日志 `P06-ACCEPTANCE-fix1-repro-and-injections.log`）：修复前三种违例 message 均为通用文案；修复后分别为 `Invalid field(s): /title.`、`/labels/0.`、`/metadata/owner.`；`issuePaths`、错误码、通过/拒绝判定前后一致。message 字段化为真实改善。

## 2. UNCHANGED_VERIFIED 关键声称抽查（源码级）

| 声称 | 核实结果 |
|---|---|
| 五变体同一 canonical 装配、preamble 与 artifact.text 逐字节相等 | **属实**。`tests/p06-final-request-assembly.test.ts` 桌面主会话经真实 `createAgentSession` + 真实 HTTP witness（truth oracle）+ 观测库 `semantic_request` payload 三方对照；`capturedSections.preamble === artifact.text` 为 `toBe` 全等断言。负向注入：worktree 内 `session-prompt-snapshot.ts` 给 systemPrompt 追加一个空格 → 2 例红（desktop + subagent）。测试非 mock 自证（仅环境性 fixture 桩：临时目录/配置/loader 桩，装配本体 `buildSystemPromptArtifact`、`buildSessionPromptSnapshot`、Pi SDK sections 均生产实现）。 |
| 27/27 工具名真实 | **属实**。抽查 materialize/stage_files/security_scan/subagent_reply/workflow/todo_write/web_search/install_skill/ask_user/stop_task/run_tools 均在 `shared/tool-categories.ts`；`BRIDGE_TOOL_NAMES = [mcp_search_tools, mcp_describe_tool, mcp_call]`（core/tool-catalog-bridge.ts:31）。 |
| A/B/C 预算 ≤ P00 基线、golden 字节锁定 | **属实**。golden-zh.txt 实际 5194 B / golden-en.txt 6114 B 与 PROMPT_BASELINE 逐值相同；负向注入：golden 追加 1 字节 → A1 红。C 组件 `ptc-tool.ts` 自研究 SHA `8037fae7` 零 commit（git log 核实，仅先于基线的 59c131276），1682 vs 1684 的 2 字节差确为 P00 计量口径差并按 ≤ 判定 + 如实登记。D=2420 tok 由 `engine.buildTools` 生产入口计量（fixture 复跑输出 `P06_RESIDENT_TOOL_BUDGET` 一致）。 |
| P05 锁定面零触碰 | **属实**。diff 仅 schema-validator.ts + REFACTOR_BACKLOG.md；消息语义裁决（normalizer 链）、streamId/seq 恢复、分页投影、资源授权文件均不在 diff。schema-validator 属 P03 工具调用边界的校验器，在 P06 允许范围（T06「运行时校验」分类的对应边界修复）。 |
| 桌面/subagent/Bridge owner/guest/后台五变体 | **属实**（源码坐标逐一核对：session-coordinator.ts:2067/8339/8347、bridge-session-manager.ts:894/923、agent.ts master 缓存）。Bridge owner/guest 用 `Object.create(BridgeSessionManager.prototype)` 仅替换 `_deps`，`_buildOwnerPromptSnapshot/_buildGuestPromptSnapshot` 方法体为生产实现；guest 隐私边界（无档案/记忆/样貌）有断言。 |

## 3. 新增 4 测试文件质量（真实入口 + 承重）

- `p06-final-request-assembly`（5 例）：真实 Pi 会话边界（见 §2）；ITERATIONS 第 2 条记录的真实发现（Pi context 无 systemPrompt 字段、system 走 messages[0].sections）与 pi-agent-core `createContextSnapshot` 实现一致。
- `p06-prompt-budget-and-invalidation`（7 例）：golden/生产常量真实读取；记忆 master/per-session 开关失效走真实 `Agent.setMemoryMasterEnabled`（含 config.yaml 装载），断言「人格/档案保留 + 记忆段退出」双向。
- `p06-feature-context-boundaries`（3 例）：真实 `Agent` 装配 × 3 次重复 × 4 变体，检查文件内容与 mtime 不变（只读使用）；人格段跨变体字节一致；基座无 knowledge_*/skills 清单。
- `p06-tool-behavior-eval`（4 例）：真实 `createToolCatalog`/`createBridgeTools`/`createToolSchemaValidator`/`classifySessionPermission`/`LingxiEngine.buildTools`；外部替身（mcpCall 假执行器）符合任务书 T05 第 2 项「假外部执行器记录选择/参数」；F-17/F-37 断言 message 含字段名（承重于 FIX-1，注入 1 证实）。
- 独立复跑：4 文件 19 例 + resolver-error-passthrough 4 例 + 工具面边界 4 套件 67 例全绿（`P06-ACCEPTANCE-{p06-tests,regression}-rerun.log`）。

## 4. 全量基线对账

- 本验收 `npm test`（运行 A）：**4 failed / 14792 passed / 1 expected fail / 15 skipped** —— 与执行者 CMD-15 数字逐项相同；红组逐名一致（audit-seal 旧坐标、round3 manifest、R10-03、R10-04）。14792 = P05 基线 14769 + 19（p06×4 文件）+ 4（counterexample 补入默认集）− 0，对账链（ITERATIONS §全量测试数字对账）复算成立。
- f1 patch：全量运行后 sha256 25fb315f… → f1ca90e6…（+130430/−17715），已 `git checkout` 还原至 HEAD 态并复核（与执行者 CMD-20 同象同处置）。
- 运行 B（调度顺序不同）出现第 5 红 R10-09：定性见 §6-②。

## 5. 反例/边界验证清单（已执行 / 未验证）

已执行：嵌套 schema 路径三形态（缺必填/嵌套数组/嵌套对象）前后对比；FIX-1 依赖注入（HEAD 评测红）；golden 预算突变红；装配保真单字符突变红；提示注入不提权（F-29 真实分类器）；f1 patch 重写副作用复现与还原；R10-09 顺序依赖波动观察。

未验证（如实声明）：真实模型行为（无凭证/费用授权，BLOCKED——任务书允许且已如实登记）；跨平台/打包（P08 范围）；Pi provider 真实网络错误下的装配残留（超出本阶段确定性面，未见相关改动面）。

## 6. 非阻塞发现（最小修复要求，不构成本轮 FAIL）

1. **FIX-1 缺陷前提与实测不符（文档/注释准确性）**：「`normalizeIssues` 只认 instancePath 会把嵌套路径（如 /labels）降级成 "/"、details 字段保真退化」的表述不成立——本仓 typebox@1.1.38 `Value.Errors` 错误对象键为 `[keyword, schemaPath, instancePath, params, message]`（无 `path` 键），`instancePath` 在嵌套路径上本就填充（修复前实测 issuePaths = `/labels/0`、`/metadata/owner`）。真实缺口只有「message 通用文案、模型不可见字段」，FIX-1 的有效成分是 message 字段化；兼认 `path` 对本版本是 no-op 防御性改动（无害，可保留）。**最小修复**：更正三处文字——`lib/tools/invocation/schema-validator.ts` 两处 P06 注释、`PROMPT_CHANGE_LEDGER.md` §3、`EVAL_COMPARISON.md` §1 FIX-1 行（「details 字段保真也在退化」句）；不要求代码变更。涉及位置：schema-validator.ts:107-111/121-124；PROMPT_CHANGE_LEDGER.md §3 第二段；EVAL_COMPARISON.md §1 表 FIX-1 行。
2. **R10-09 顺序依赖波动（既有现象登记）**：f1 patch 处于 HEAD 还原态且当次运行未发生运行中重写时，R10-09（「增量补丁重放后与当前工作树一致」）会因工作区含未提交改动而红——本验收运行 B 出现第 5 红；执行者运行与本验收运行 A 均为 4 红（运行中重写使 R10-09 自愈通过）。属 P04 起「f1 patch 被全量测试重写」已知现象家族的顺序依赖表现，**非 P06 引入**（P06 未触碰 round2/round3/audit-seal/.sync-audit）。登记给编排层：全量对账结论以红组逐名为准时须意识到该波动；提交前还原 f1 patch 的既有提醒继续有效。
3. （轻微）P06_RESULT.json `acceptance_cases` 为指向 ACCEPTANCE_MAP.json 的指针而非内联数组——与 P04/P05 先例一致且 map 内字段齐全（id/test_path/test_names/fixture/production_entry/command_ids/status/evidence），不构成模板违规。

## 7. 本验收写入的文件

- docs/refactor-2026/P06/P06_ACCEPTANCE_REVIEW.md（本文件）
- artifacts/refactor-2026/P06/logs/P06-ACCEPTANCE-regression-rerun.log（resolver-passthrough + 工具面 67 例 + 复跑摘要 71 绿）
- artifacts/refactor-2026/P06/logs/P06-ACCEPTANCE-p06-tests-rerun.log（4 文件 19 绿）
- artifacts/refactor-2026/P06/logs/P06-ACCEPTANCE-full-suite-rerun.log（运行 A/B 记录 + f1 patch 处置）
- artifacts/refactor-2026/P06/logs/P06-ACCEPTANCE-fix1-repro-and-injections.log（FIX-1 前后对比 + 3 组负向注入）

执行者 EVIDENCE_SHA256.txt 未改动（其 29 条哈希本验收逐一重算全匹配）；本验收文件未纳入该清单，属编排层提交前动作。验收过程主仓工作区零生产/测试/文档改动（临时验证文件与 worktree 均已清理，终态 git status 与验收起点一致）。

## 8. 验收判定

| 项 | 判定 |
|---|---|
| T01 装配盘点 / A01 | PASS（独立复现 + 注入 3 证实承重） |
| T02 常驻/按需分离 / A02–A05, A13 | PASS（确定性面） |
| T03 预算缓存 / A08–A09 | PASS（A/B/C ≤ 基线复算；D/E 新计量登记） |
| T04 边界兼容 / A10 | PASS（零业务变更 + 真实开关/只读/变体断言） |
| T05 评测集 / A03–A07, A12, A14 | 确定性 PASS；真实模型 BLOCKED（正当） |
| T06 比较修正 | 确定性 PASS（FIX-1 真实、最小、回归绿）；真模型配对 BLOCKED（正当） |
| T07 集成交接 | PASS（全量对账、f1 patch 还原、handoff 完整） |
| P05 锁定面 | 零触碰（diff 核实） |

阶段总状态：**PASS（确定性契约面）＋ BLOCKED（真实模型行为评测，任务书允许的环境受限，非掩饰未做工作）**。不承诺绝对无缺陷；§5 列明未验证边界。
