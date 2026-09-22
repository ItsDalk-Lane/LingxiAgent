# P06 开发迭代失败记录（首次红全部保留——通用约束 §7）

vitest 迭代过程中的首次红与修复（归档文件只含最终绿，此处保留失败事实；均为合成 fixture 下的测试开发迭代，非生产缺陷回归）：

1. `tests/p06-final-request-assembly.test.ts` 首跑 5 红：`TENENTS_MARKER` 拼写未定义（ReferenceError）→ 修正拼写。
2. 二跑 4 红：① semantic_request `payload.systemPrompt` 为 null——发现 Pi 真实路径 streamFn context 只带 `{messages, tools}`，最终 system prompt 在 `messages[0].sections`（据此改为结构化断言 + witness 正文断言）；② bridge 行未追加——`_buildBridgeContext` sessionKey 需 `tg_dm_*` 前缀格式；③ `<cwd>` 计数 2——workspace 指引文案合法含 `<cwd>` 标记，改按「## 工作区范围」锚点计数；④ subagent 用户档案断言错误——现行设计保留用户档案（只隔离记忆/团队），修正断言并在 CONTEXT_ASSEMBLY_MAP §3 登记。
3. `tests/p06-prompt-budget-and-invalidation.test.ts` 首跑 2 红：① C 组件断言 `==` 基线 1684，实测 1682（P00 计量口径差 2 字节，文件自研究 SHA 零改动）→ 改 `<=` 并在 PROMPT_BUDGET_REPORT §2 登记；② `setMemoryMasterEnabled` 需要 config.yaml → fixture 补最小配置。
4. `tests/p06-feature-context-boundaries.test.ts` 迭代 2 红：① `SKILL.md` 字符串出现在 skill-usage 常驻规则文本（合法动作描述，非清单注入）→ 收窄为 `<skill>` 标签断言；② provenance skill 段过滤期望含 `platform.learn-skills`，但 fixture 未开 learn_skills → 按实际注入集合修正。
5. `tests/p06-tool-behavior-eval.test.ts` 迭代红：① 模板字面量嵌套解析错误 → 拆变量；② `execute` 未 await；③ 搜索命中渲染为「参数：」非「必填：」（必填仅在 describe 全量渲染）→ 修正断言；④ `schema_rejects_missing_required` 期望 message 含字段名，实测通用文案——**这是真发现**（FIX-1），修复 schema-validator 后转绿；⑤ auto 档 stage_files 判定为 `review` 而非 `prompt` → 修正断言。
6. typecheck 首跑 4 错（vi.fn 类型推断/gateway 桩类型/describe cast）→ 修正测试类型标注；`npm run check:core-contracts` 首跑用错脚本名（exit 1，npm error）→ 改用 `typecheck:core-contracts`（exit 0）。
7. 全量 npm test 后 `artifacts/f1-f12-repair/round2/patches/89bc0b64-to-r01-r10-source.patch` 被测试重写（P04/P05 已知现象，diff 为 ci.yml 索引行哈希漂移）→ `git checkout --` 还原并复核。

## 全量测试数字对账（相对 P05 验收日志）

- P05 验收全量：4 红 / 14769 绿 / 15 跳过（其日志未含当时新加的 P05-ACCEPTANCE-counterexample 5 例）。
- P06 全量：**4 红（同名同组，未出现第 5 红）/ 14792 绿 / 1 expected fail / 15 跳过**。
- 增量对账：+19（P06 新增 4 个测试文件 5+7+3+4）+4（counterexample 4 个真绿，P05 验收日志后加入默认集）+1 expected fail（counterexample 设计内 it.fails 转独立计数）= +24 总数；红集逐一比对相同（post-verification-audit-seal 旧坐标 + round3 manifest + R10-03 + R10-04，F1 已知基线）。
