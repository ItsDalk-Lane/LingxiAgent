# P04 复验收报告（FIXR1 修复轮后，复验收子代理，全新上下文）

日期：2026-09-22｜验收对象：P04 阶段整体（执行轮 + 首轮验收 + P04-FIXR1 修复轮；HEAD `3286c96e571fadea65143df8baabf503d30d37f2` + 未提交工作区）｜角色：只读复验收，不修改生产代码/测试/文档（本报告与 `artifacts/refactor-2026/P04/logs/P04-REACCEPTANCE-rerun.log` 为唯一写入；沿用前两轮惯例，验收产物不入 EVIDENCE_SHA256 执行证据清单，清单未触动）。

## 结论

**阶段总状态：BLOCKED（维持首轮判定，理由不变且仍成立）。除 P04-T07-2 真实供应商冒烟外，无任何其他未闭合项。首轮验收问题 #1 经 FIXR1 修复后核实为完全闭合；本轮无 FAIL。**

- FIXR1 对问题 #1 的修复**以源码独立复核确认闭合**（方法与结论见 §1）：USAGE_OWNERSHIP.md §1 修订版 8 行枚举与我对生产代码的全量独立普查**逐点一致**，未发现第 9 处生产可达写入点。
- 修复轮范围纪律、证据链完整性、执行轮关键声称抽查全部通过（§2–§4）。
- 本轮新发现 1 个**轻微文档一致性问题**（NEXT_STAGE_HANDOFF §2 残留旧计数「五个计量观察点」，非阻塞，见 §6），不构成 FAIL、不要求新修复轮。

## 1. 问题 #1 闭合核实（独立普查，不采信修复者自报）

**普查口径**（与修复者不同：`?.` 可选链形态、接收者别名、解构赋值、跨目录 desktop/cli/hub/plugins、事件发射侧）：

1. **台账唯一性**：`createUsageLedger` 生产实例仅 `core/engine.ts:923` 一处（`this._usageLedger`）——单一台账结构性成立。
2. **共享 helper 调用方**：`withModelRequestAccounting`（lib/llm/model-request-accounting.ts）生产调用 6 处/5 文件：model-operation-client.ts:546（§1 #2）、universal-media-manager.ts:962/1370 + image-task-runner.ts:640 + speech-recognition-service.ts:550（#3）、provider-client.ts:344（#7）。
3. **直调四写方法**（正则 `ledger(\?)?\.(start|finish|record|recordError)`，含 `?.` 形态）：29 行直调 + helper 内 4 行，归属——llm-client.ts:856/1054/1095（#1，在声称区间 856-1099 内）；session-coordinator.ts:612/618/621（#4，recordAssistantUsage:588，调用点 :2328/:8560）；bridge-session-manager.ts:348/354/357（#4）；hub/agent-executor.ts:137/143/146（#4）；observed-pi-direct-summary.ts:158-204（#5）；cache-preserving-compaction-agent-run.ts:381/384/423/518/563（#6，与文档声称行号逐一相同）；session-snapshot-side-task-runner.ts:83/96/101/104（§1 口径段落登记的不可达点）。
4. **不可达登记复核**：`runMemoryReflection` 全仓搜索（排除 node_modules/dist/.git/tests）仅命中定义（lib/memory/memory-reflection-runner.ts:79）与注释/测试引用，**无生产调用方**——「生产不可达、不计入本表」登记属实。
5. **键控方式抽查（源码逐字核对）**：#1「pending map 删除后才可 finish、重复 finish 返回 null」= usage-ledger.ts:82-85（`if (!pendingEntry) return null; pending.delete(...)`）逐字一致；#5 `metadata.modelCallId = recorder.callId`（observed-pi-direct-summary.ts:152-156）；#6 `mintModelCallId()` 先于 start（:418→:423，recovery/repair 新 call 新键注释一致）；#7 `observedModelCallLedgerMetadata(recorder)`（provider-client.ts:348）；#8 投影 `model_call_id` PRIMARY KEY UPSERT、无 modelCallId 不投影（accounting-projection.ts:17/88-89/167）。「供应商未给 usage → status=usage_missing」= usage-ledger.ts:97。
6. **生产入口行号复核**：core/session-compactor.ts:1878（`runCachePreservingCompactionAgentRun({` 精确命中）；server/routes/providers.ts:825（`probeProvider` 调用，传 `usageLedger: engine.usageLedger`、operation "connectivity-probe"）；diary-writer.ts:376（generateSummary 第 14 参 observerContext 携带 usageLedger——#5 生产可达）；pi-sdk/index.ts:241-283（第 14 参 `observerContext`、`if (streamFn) return invokeRaw()` 旁路语义与 §1 #5 括注一致）。
7. **事件发射侧**：`llm_usage` 事件生产者生产代码仅 usage-ledger.ts:301；投影为唯一消费投影（#8）。skills2set/ 与 plugins/ 目录 grep `llm_usage` 零命中。
8. **反例构造结果**：别名接收者 workflow-tool.ts:256/438 `const ledger = deps.getUsageLedger?.()` 仅 `ledger.list`（读路径预算汇总，非写入，反而佐证「读取路径只读」）；解构形态、desktop/cli（.ts/.cjs/.tsx）均零命中。**未能构造出「枚举仍不完备」的反例。**

**边界观察（非缺陷）**：plugin-context.ts:193-204 对 `llm_usage` 的 emit/subscribe 守卫仅拦非 fullAccess 插件——fullAccess 插件理论上可向总线发 `llm_usage`。属设计内受信能力（与内置插件实测零使用联合），不构成未登记生产写入面。

## 2. 修复者「验收描述与源码不符」指认的核实：**属实**

- MODEL_CALLSITE_MATRIX.json 共 7 个 family，`probeProvider`/`connectivity`/`routes/providers` 在该文件**零命中**；其 provider-probe（MC-08）行 callsites 仅 `server/routes/models.ts:234-292 health-check`。
- OPERATION_COVERAGE_MATRIX.json provider-probe (MC-08) family 的 production_entry 含「probeProvider 生成探测」——connectivity-probe 链的操作覆盖确记于此。
- 即首轮验收问题 #1 中「两处均在 MODEL_CALLSITE_MATRIX 对应 family 的 usage 字段有归属记载」对 MC-10 成立、对 connectivity-probe **不成立**；修复者以源码纠正验收描述并在 P04_REPORT/P04_RESULT 登记「MODEL_CALLSITE_MATRIX 缺 connectivity-probe family」为遗留观察（矩阵本轮不动、保交付哈希）——处置诚实且在最小修复范围内。USAGE_OWNERSHIP §1 #7 现引「OPERATION_COVERAGE_MATRIX provider-probe family」与源码事实一致。

## 3. 修复轮范围纪律：**PASS**

- `git diff HEAD --stat`：仅 7 个跟踪文件（4 测试文件新增用例、1 测试 harness、P00 两文档各 +2 行注记），**无任何生产源码文件**；与首轮验收描述完全一致。
- 跟踪文件 mtime 全部落在执行轮时段（09:20–09:32）；FIXR1 时段（09:57–10:03）改动全部位于未跟踪的 docs/ 与 artifacts/ refactor-2026/P04/。**零生产代码/测试改动、无断言削弱、无审计封印触碰。**
- 首轮验收产物 P04_ACCEPTANCE_REVIEW.md（09:49）与执行轮六矩阵/契约文档（09:26–09:31）mtime 未变——**前轮记录未被回改**；P04_REPORT/P04_RESULT/ACCEPTANCE_MAP/NEXT_STAGE_HANDOFF 的 FIXR1 更新均为增补式（独立「独立验收修复轮」节、fix_rounds 数组、§6 措辞对齐），并显式声明「前轮事实不重写」。

## 4. 证据链完整性：**PASS**

- EVIDENCE_SHA256.txt 独立复算：**70/70 全部 OK，exit=0**；FIXR1 改动的 6 个文件全部在清单内（哈希已刷新），清单不含自身（模板 §7 设计）、不含 manifest-check.out（避免递归）。
- command-log.jsonl 28 条：command_id 唯一、exit_code/status 齐全、失败链完整（执行轮 4 条开发期 FAIL→fix 重跑 + 2 条故意注入红 + full-suite 如实 exit=1）；FIXR1 3 条命令 exit=0。
- **失败迭代如实留档**：P04-FIXR1-writepoints-grep.out（第一版正则，17 行、漏 `?.` 形态）保留为迭代记录；grep2（36 行）与本轮独立普查结果一致。
- shasum 终验不入 jsonl 的取舍（保清单哈希稳定、输出含命令行与逐条 OK 留档于 manifest-check.out）已在 P04_REPORT 声明——合理的工程取舍且披露诚实。

## 5. 执行轮关键声称抽查（复跑）

| 命令 | 结果 |
|---|---|
| vitest resolver+client+e2e-chat+e2e-utility（4 文件） | 41/41 绿（= 首轮 25/25+16/16 合并口径） |
| vitest llm-usage-ledger + accounting-projection | 20/20 绿（projection 10 例与 §2 声称一致） |
| npm run typecheck | exit 0（tsc x3） |
| npm run check:tool-invocation-boundaries | 通过（2244 源文件） |
| F5 补丁副作用 | 工作区与 HEAD 版本 sha256 相同（`25fb315f…`），无漂移 |
| P00 两文档 diff | 纯注记增补，无事实改写 |

全量套件未重跑（沿用首轮验收已独立核对的口径：4 红 = F1 基线身份与 P00/P03 登记同组；本轮定向+门禁+新增文件已覆盖 FIXR1 影响面——修复轮零代码改动，全量结果无变化理由）。

## 6. 本轮发现的问题（均非阻塞）

1. **[轻微-文档一致性] NEXT_STAGE_HANDOFF.md §2 残留旧计数**：§2 表 USAGE_OWNERSHIP 行仍写「**五个**计量观察点与 modelCallId 去重身份」，而 §6 已对齐为「7 处生产写入边界 + 1 消费投影」——FIXR1 对该文件的措辞对齐只改了 §6，漏了 §2。实际后果：无（该行指向 USAGE_OWNERSHIP §1 为权威源，消费指令「P05 历史投影不得新增 usage 写入」不受影响）。最小修复：下次授权的文档修订把 §2 行改为「7 处生产写入边界 + 1 消费投影（见 USAGE_OWNERSHIP §1 修订版）」；不要求为此新开修复轮。
2. **[观察，沿首轮 #2–#4 无变化]** A02/A03 结构性证明口径、命令日志 source_sha 记 HEAD 提交等首轮观察项在 FIXR1 后无新信息，维持原判。

## 7. T07-2 BLOCKED 合规性（维持首轮判定）

无新信息：本环境同样无真实供应商凭证/预算授权（本轮未调用任何真实账户）。任务书 T07-2 明文「没有凭证或预算授权记 BLOCKED」、§10「缺少已要求真供应商验证时仅实施完成，阶段验收 BLOCKED，不虚报」，通用约束 §4/§5 同义。本地全部可执行项真实执行且绿，BLOCKED 不是 FAIL 掩盖。首轮判定成立，予以维持。

## 8. 未验证边界

- 真实供应商冒烟（T07-2 本体）；四平台 CI、Windows/Linux 实机、正式打包（F3 继承）。
- 全量套件本轮未重跑（理由见 §5；修复轮零代码改动）。
- fullAccess 插件发射 `llm_usage` 的理论面仅静态排查（内置插件零命中），未做运行时注入实验（属设计内受信能力，非本阶段任务）。

## 9. 判定

| 项 | 判定 |
|---|---|
| 首轮问题 #1（§1 枚举完备性）FIXR1 闭合 | **PASS**（独立普查逐点一致，含键控/行号/入口/不可达登记） |
| 修复者对「验收描述与源码不符」的指认 | 属实（MODEL_CALLSITE_MATRIX 无 connectivity-probe 链） |
| 修复轮范围纪律（仅文档/记录、无夹带、不改写前轮事实） | PASS |
| 证据链（70/70 校验、FIXR1 日志、失败迭代留档） | PASS |
| 执行轮关键声称抽查（41+20 例、typecheck、门禁、F5） | PASS |
| 反例构造（枚举不完备） | 未构造出（口径见 §1.8） |
| BLOCKED 项（T07-2）合规性 | 成立（维持首轮判定） |
| **阶段终判（含 FIXR1 的阶段整体）** | **BLOCKED——本地实施、首轮验收与 FIXR1 修复全部核实通过；唯一未闭合项 = P04-T07-2 真实供应商冒烟（无凭证/预算授权），除此之外无任何其他未闭合项。授权后补该单项即可转 PASS。** |

## 验收证据

- `artifacts/refactor-2026/P04/logs/P04-REACCEPTANCE-rerun.log`（本轮全部命令、口径与 exit code）
- 未创建临时目录/文件；未改动用户数据；未 commit/push/tag；EVIDENCE_SHA256.txt 未触动（沿用验收产物不入执行证据清单的前两轮惯例）。
