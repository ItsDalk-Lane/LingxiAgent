# P06-T06｜效果比较与修正报告（EVAL_COMPARISON）

日期：2026-09-22｜HEAD `93b8b7265`。本任务的配对对象是「旧配置 vs 新配置」——本阶段常驻文案零改动（PROMPT_CHANGE_LEDGER §1），唯一接口修复是 schema 校验反馈字段化（§1）。真实模型配对评测无凭证/费用授权，BLOCKED（§3）；确定性契约全部执行（§2）。

## 1. 修复记录（按失败分类，T06 第 2 项七分类）

| # | 分类 | 发现 | 修复位置 | 验证 |
|---|---|---|---|---|
| FIX-1 | **运行时校验（反馈粒度）** | `ARGUMENT_SCHEMA_INVALID` message 通用化：模型可见工具结果只有 `error.message`（pi-agent-loop `createErrorToolResult(error.message)`），字段明细留在 details 到不了调用方；常驻规则「按指出的字段与约束修正」在 mcp_call 参数错误路径落空。（初版另称「`normalizeIssues` 只读 `instancePath`，嵌套路径（/labels）被降级为 `/`，details 字段保真也在退化」——**P06-FIXR1 更正**：本仓 typebox@1.1.38 错误对象键为 keyword/schemaPath/instancePath/params/message，`instancePath` 在嵌套路径上本就填充，修复前 issuePaths 已为 `/labels/0`、`/metadata/owner`；details 无保真退化，message 通用文案才是真实缺口） | `lib/tools/invocation/schema-validator.ts`：① normalizeIssues 兼认 `path`/`instancePath`（P06-FIXR1 定性：对本版本为防御性兼容 no-op，无害保留）；② message 追加 `Invalid field(s): <字段列表>`（root 必填缺失从 message 保守提取属性名）——FIX-1 的实际有效成分 | tests/p06-tool-behavior-eval.test.ts F-17（缺 title → message 含 title）、F-37（labels 类型错 → 指明 /labels 后修正成功）；tests/resolver-error-passthrough.test.ts 4 例兼容绿；四个工具面套件 67 例绿 |
| 目录找不到 | 未发现新例 | search 空结果/歧义/describe 缺失的修正路径已存在（bridge 文案） | 无需改动 | F-12/F-13/F-14 PASS |
| 说明歧义 | 未发现 | 常驻文案过时工具名/虚构能力核查 0 例（T01 §4） | 无需改动 | T01 测试 |
| 参数来源不足 | 未发现（确定性侧） | describe 全量渲染 schema（嵌套/枚举/约束） | 无需改动 | F-11 PASS |
| 权限/工具不可用 | 未发现 | 分类器/撤销路径按既有套件绿 | 无需改动 | F-26..F-32 |
| 供应商差异 | 真模型侧 | 无凭证 → BLOCKED | — | — |
| 纯模型失误 | 真模型侧 | 无凭证 → BLOCKED | — | — |

**未用加 system 规则的方式修复任何一类**：FIX-1 在校验器边界（message 构造点）修复。

## 2. 确定性契约执行结果（T06 第 1 项前半）

- 装配/来源/权限契约先行：T01（5 例）/T03（7 例）/T04（3 例）/T05 harness（4 例）全绿；映射套件（tool-lifecycle-revocation/knowledge-agent-tools 等）在全量 npm test 中执行（见 P06_REPORT §验证）。
- 确定性关键错误 = 0：评测 23 个确定性样本全部 PASS（含修复后回归）；失败样本 0。
- 真实越权实际副作用：确定性反例（注入不提升权限、read-only 拒写、外部替身 0 次执行）全绿。

## 3. 真实模型配对评测（T06 第 1 项后半）——BLOCKED

- 状态：**BLOCKED**（无供应商凭证与费用授权；继承 P04-T07-2，编排层未授权任何付费调用）。
- 不填任何成功率/准确率数字；留出样本（H-41..H-48）未运行、未泄露给任何文案调整。
- 授权后执行方式：TOOL_BEHAVIOR_EVAL.json §budget_authorization_record（fixed 40 + holdout 8，每例 3 次；同模型版本/参数/工具面/数据配对旧新；记录首调 schema 正确率/目标对象正确率/完成率/重试/成本/tokens）。

## 4. 新旧效果对比结论

- 确定性侧：FIX-1 严格改善（字段级反馈从不可见变为可见；details 本就保真——初版「保真恢复」表述经 P06-FIXR1 更正，兼认 `path` 对本版本为 no-op）；无任何确定性指标退步（67+12 例回归绿）。
- 真模型侧：**无证据宣称提升或持平**——按任务书「样本不充分不能宣称提升」，该结论留待授权后的配对评测。
- 黄金文案：**未更新**（A/B/C 组件字节与 P00 基线一致；无快照刷新）。

## 5. 文案最终 diff 与黄金样本更新理由

- 常驻文案 diff：无（零字节变化，golden 锁定未动）。
- 黄金样本更新：无——没有足够的真实模型证据支持文案变更；FIX-1 是接口修复而非文案调整，不需要黄金刷新。

## 6. 回退

FIX-1 回退 = revert `lib/tools/invocation/schema-validator.ts`（提示词/schema 无成套变更，无旧文案指向新 schema 的错配风险）。回退后 tests/p06-tool-behavior-eval.test.ts F-17/F-37 转红（回归提示）。
