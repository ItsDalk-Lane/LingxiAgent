# P06 阶段执行报告（上下文、提示词、记忆与能力集成）

日期：2026-09-22｜任务书：Lingxi_Refactor_Taskbooks_2026-09-21/P06_上下文、提示词、记忆与能力集成.md

## 结论

**PASS（确定性契约面全绿）＋ BLOCKED（真实模型行为评测，无凭证/费用授权——不虚报任何成功率）**。装配单一权威、预算不超基线、人格/记忆/Skill/知识边界保持、40+8 例评测集就绪且确定性 23 例全 PASS；一处接口修复（schema 校验反馈字段化）。未触碰 P05 锁定面（消息语义/恢复协议/分页投影/资源授权）。

## 实际输入

- START_SHA = END_SHA 候选 = `93b8b7265`（P05 提交；本阶段零生产 commit，改动保留工作区待编排层统一提交）
- 分支 `docs/knowledge-closeout-2026-09-21`；Node v24.16.0 / npm 11.13.0 / darwin arm64；lockfile sha256 `a9735825cea1d018…`（未变）
- 研究基线 `8037fae7` 漂移核对：`lib/tools/ptc-tool.ts` 自基线零改动（C 组件 2 字节差异为 P00 计量口径差，已登记）；其余本阶段接触文件漂移已在 P01–P05 各报告覆盖
- 未提交修改保护：进入时工作区干净；结束时改动清单见 §差异

## 任务逐项结果

| 任务ID | 实现 | 生产入口与源码位置 | 测试与日志 | 旧路径去向 | 状态 |
|---|---|---|---|---|---|
| P06-T01 装配盘点 | UNCHANGED_VERIFIED（装配本体）+ 新捕获测试 | core/agent.ts:1552 buildSystemPromptArtifact（唯一基座）；session-coordinator.ts:2067/8339；bridge-session-manager.ts:894/923；Pi SDK buildSystemPromptSections | tests/p06-final-request-assembly.test.ts（5 例）→ logs/P06-T01-assembly-capture.*；交付 CONTEXT_ASSEMBLY_MAP.md + 样本索引 | 无第二拼装需要退出（#399 早已收敛；本轮证明） | PASS |
| P06-T02 常驻/按需分离 | UNCHANGED_VERIFIED（文案零改动）+ MODIFIED（FIX-1 接口修复） | 常驻规则段 agent.ts:1717-1826；目录桥 core/tool-catalog-bridge.ts；修复 lib/tools/invocation/schema-validator.ts | tests/p06-tool-behavior-eval.test.ts（F-17/F-37）+ resolver-error-passthrough 4 例；交付 PROMPT_CHANGE_LEDGER.md | run_tools 留目录不占常驻面（既有锁定） | PASS |
| P06-T03 预算/缓存 | UNCHANGED_VERIFIED（A/B/C ≤ 基线）+ 新计量（D/E） | golden fixtures + engine toolCatalogIntroLine + ptc-tool 描述 + engine.buildTools 常驻面 | tests/p06-prompt-budget-and-invalidation.test.ts（7 例）→ logs/P06-T03-*；交付 PROMPT_BUDGET_REPORT.md | 无削减项（预算达标未删用户内容） | PASS |
| P06-T04 边界兼容 | UNCHANGED_VERIFIED（零业务变更） | 人格/记忆开关 agent.ts:1288-1370；知识 scope 知识工具面；Skill 快照 lib/skills/session-skill-snapshot.ts | tests/p06-feature-context-boundaries.test.ts（3 例）+ T03 开关例 + 映射套件；交付 FEATURE_CONTEXT_COMPAT.md | 无 | PASS |
| P06-T05 评测集 | 新增评测资产 | docs/refactor-2026/P06/TOOL_BEHAVIOR_EVAL.json（40 固定+8 留出，sha256 8e91c139…） | tests/p06-tool-behavior-eval.test.ts：确定性 23 PASS + mapped 7 + 完整性/BLOCKED 声明 → logs/P06-T05-* | 无 | 确定性 PASS；**真实模型 BLOCKED** |
| P06-T06 比较与修正 | MODIFIED（FIX-1）+ 文案零 diff | schema-validator message 字段化 + normalizeIssues path/instancePath 兼认 | EVAL_COMPARISON.md（FIX-1 分类：运行时校验反馈粒度）；67+12 例回归绿 | 黄金样本未更新（无充分证据） | 确定性 PASS；真模型配对 BLOCKED |
| P06-T07 集成交接 | 新增文档/证据 | docs/refactor-2026/P06/ 全套 + artifacts/refactor-2026/P06/logs/ | 本报告 + ACCEPTANCE_MAP.json + NEXT_STAGE_HANDOFF.md + EVIDENCE_SHA256.txt | 无消费者旧片段需删（本轮无文案删除） | PASS |

## 场景逐项结果

见 ACCEPTANCE_MAP.json（A01–A14 全登记，含测试名/fixture/命令ID/证据路径）。要点：A01/03/04/05/07/08/09/12/13/14 直接新测试 PASS；A02 经 golden 文案锁定 PASS（真模型侧 BLOCKED 注记）；A06/A10/A11 映射既有套件（全量内绿）。

## 接线和旧路径

- 新链真实消费者：4 个 p06 测试文件进入默认 vitest 集（随 npm test 执行）；TOOL_BEHAVIOR_EVAL.json 被 harness 消费；REFACTOR_BACKLOG P06 行已加执行注记。
- 生产代码改动仅 `lib/tools/invocation/schema-validator.ts`（FIX-1）：normalizeIssues 路径保真 + message 字段定位。真实消费者：全部经 createToolSchemaValidator 的校验路径（mcp_call 桥/网关）。
- 无双写、无隐式回退、无测试专用新模块。

## 验证（命令/exit code/日志；首次失败见 logs/ITERATIONS.md）

| 命令 | exit | 结果 |
|---|---|---|
| CMD-16 npm run typecheck | 0 | typecheck×3 绿（首跑 2 → 修复测试类型标注） |
| CMD-17 typecheck:core-contracts | 0 | 6 files strict 绿 |
| CMD-18 check:tool-invocation-boundaries | 0 | 绿 |
| CMD-19 lint:boundary | 0 | 绿 |
| CMD-14 定向套件（任务书 §7 命令 + 4 个 p06 文件） | 0 | 6 文件 24 例绿 |
| CMD-11 工具面边界套件×4 | 0 | 67 例绿 |
| CMD-15 全量 npm test | 1（F1 基线） | **4 红 = F1 同一组逐名一致**（audit-seal 旧坐标/round3 manifest/R10-03/R10-04）；14792 绿 / 1 expected fail / 15 跳过；无第 5 红；数字对账见 logs/ITERATIONS.md |

## 数据、权限与平台

- 零 schema 变更、零迁移、零指纹触碰（本阶段未改持久化结构）。
- 全部测试用合成 fixture（受控临时目录/内存观测库/假 provider witness），未读写真实用户 HOME/会话/记忆/凭证。
- 平台：darwin arm64 本地；未做跨平台/正式打包验证（属 P08）。

## 差异与限制

- **改动（工作区，未提交）**：`lib/tools/invocation/schema-validator.ts`（FIX-1）；`docs/refactor-2026/P00/REFACTOR_BACKLOG.md`（P06 执行注记）；新增 `tests/p06-{final-request-assembly,prompt-budget-and-invalidation,feature-context-boundaries,tool-behavior-eval}.test.ts`、`docs/refactor-2026/P06/`（7 文件）、`artifacts/refactor-2026/P06/logs/`（含 command-log.jsonl/ITERATIONS.md）。
- **保留**：常驻文案 golden 字节不变；P05 全部锁定面零触碰；P04 遗留 BLOCKED（真供应商冒烟）继续登记。
- **BLOCKED**：真实模型行为评测（T05/T06 真模型侧；无凭证/预算授权，继承 P04-T07-2）。授权后按 TOOL_BEHAVIOR_EVAL §budget_authorization_record 执行。
- **非关键登记**：① P00 PROMPT_BASELINE C 组件 1684→实测 1682 的 2 字节计量口径差（PROMPT_BUDGET_REPORT §2）；② P04 NEXT_STAGE_HANDOFF §2 措辞不一致项本阶段仍未合法触碰该文件，继续留给下次合法触碰者（同 P05 处置）。
- **审计封印**：post-verification-audit-seal 红为旧坐标已知基线（AGENTS.md 适用范围一节所述情形），如实报告，不为此推进封印。

## 回退与下一阶段

- 回退步骤：① revert `lib/tools/invocation/schema-validator.ts`（tests F-17/F-37 转红作回归提示；提示词与 schema 无成套错配风险）；② 删除 4 个 p06 测试文件与 docs/artifacts 的 P06 目录；③ REFACTOR_BACKLOG 注记条目还原。无需数据回退（零 schema 变更）。
- 下一阶段（P07）交接：见 NEXT_STAGE_HANDOFF.md——固定样本/预算与输出等价基线已移交；性能优化不得以少提供能力或省略安全规则取胜。

## 独立验收修复轮（P06-FIXR1，2026-09-22，一项）

来源 = 独立验收 [P06_ACCEPTANCE_REVIEW.md](P06_ACCEPTANCE_REVIEW.md) §6 非阻塞发现 ①（问题 ② R10-09
顺序依赖波动登记编排层，不在本轮范围）。零执行代码、零测试断言改动——`git diff HEAD` 复核：
schema-validator.ts 相对 HEAD 的全部非注释行 = 执行轮 FIX-1 被验收逐 hunk 审阅过的同一组代码，
本轮仅触碰两处注释块；改动面 = 叙事更正 + 记录文档。

**问题 ①：FIX-1 缺陷叙事与实测不符（文档/注释准确性，代码无害）**

- 事实复核（本轮独立复测 `P06-FIXR1-typebox-error-shape` + 验收轮隔离 worktree 证据
  logs/P06-ACCEPTANCE-fix1-repro-and-injections.log）：本仓 typebox@1.1.38（package.json 锁定）
  `Value.Errors` 错误对象键 = `[keyword, schemaPath, instancePath, params, message]`（无 `path` 键），
  `instancePath` 在嵌套路径上本就填充——按修复前 normalizeIssues 口径（只认 instancePath）复算
  issuePaths = `/`、`/labels/0`、`/metadata/owner`，嵌套路径并未降级为 `/`。故「只认 instancePath
  会把嵌套路径降级成 /、details 字段保真退化」的前提不成立；真实缺口仅是 message 通用文案
  （模型只见 error.message），message 字段化（FIX-1 的 `Invalid field(s): …`）才是有效成分；
  兼认 `path` 属性对本版本是防御性 no-op（无害保留）。
- 更正落点（验收最小修复要求，无代码逻辑变更）：
  - `lib/tools/invocation/schema-validator.ts` 两处 P06 注释（normalizeIssues 块注释 +
    summarizeIssueFields docstring）：如实描述 message 通用文案为真实缺口、path 兼认为本版本
    防御性 no-op；执行代码行零改动。
  - `docs/refactor-2026/P06/PROMPT_CHANGE_LEDGER.md` §3：问题/修复两段更正（原「嵌套路径降级/
    details 保真退化/恢复 details 字段保真」表述按实测改写，标注 P06-FIXR1 更正）。
  - `docs/refactor-2026/P06/EVAL_COMPARISON.md` §1 表 FIX-1 行（验收指定）及 §4 结论中同叙事
    短句「details 保真恢复」（同一叙事残留句，一并更正为「details 本就保真」）。
  - 本报告前文（§任务逐项结果 T06 行、§接线 FIX-1 概述「normalizeIssues 路径保真」措辞）按
    「不改写前轮事实」保留原文，以本节更正为准。
- 复核（run-logged，exit code 见 command-log.jsonl P06-FIXR1-*）：
  - `P06-FIXR1-typebox-error-shape`（exit 0）：同验收口径三违例独立复测，keys/instancePath/
    issuePaths 与验收证据逐项一致。
  - `P06-FIXR1-resolver-error-passthrough`（exit 0）：4 例绿。
  - `P06-FIXR1-tool-boundary-suites`（exit 0）：工具面边界 4 套件 67 例绿（与验收基线一致）。
  - `P06-FIXR1-p06-tests`（exit 0）：4 个 p06 文件 19 例绿（含 F-17/F-37 对 FIX-1 message 的
    承重断言）——零行为变化成立。
  - `P06-FIXR1-typecheck`（exit 0）：tsc x3 绿，注释未破坏语法（stderr 0 字节）。
  - 未跑全量 npm test（约束：f1 patch 重写副作用与 R10-09 顺序波动；本轮未触碰
    artifacts/f1-f12-repair/round2/patches/，git status 复核该文件无改动）。
- 记录与清单一致性：P06_RESULT.json（fix_rounds）、ACCEPTANCE_MAP.json（fix_rounds）补记本轮
  条目，前轮事实不改写；EVIDENCE_SHA256.txt（docs + logs 两份）按既有口径刷新——本轮改动条目
  哈希刷新，验收轮产物（P06_ACCEPTANCE_REVIEW.md + P06-ACCEPTANCE-* 4 件，验收报告 §7 声明的
  提交前动作，P05-FIXR1 先例）与本轮 P06-FIXR1-* 日志补入，路径列规范化为全仓相对路径
  （P05-FIXR1/P02–P04 同款，哈希↔文件绑定关系不变，支持仓库根单一 cwd `shasum -a 256 -c`
  全量校验）；manifest-check.out 不入清单（P04/P05-FIXR 同款）。`P06-FIXR1-json-validate`
  （exit 0）：ACCEPTANCE_MAP.json / P06_RESULT.json 解析合法。清单终验 `shasum -a 256 -c`：
  exit 0 逐条 OK，留档 logs/P06-FIXR1-manifest-check.out。
