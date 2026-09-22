# NEXT_STAGE_HANDOFF — P05 输入（P04 → P05）

日期：2026-09-22｜P04 结果：见 P04_RESULT.json（实施+本地验收完成；真供应商冒烟 BLOCKED 单项）。

## 1. 已验收坐标与环境

- 工作区 START = `3286c96e5`；END 候选 = 同（零生产 commit；工作区改动 = 4 测试文件 + 1 harness + P00 两处注记 + 本目录/artifacts 未跟踪，等待编排层统一提交）
- 分支 `docs/knowledge-closeout-2026-09-21`（未切换）；Node v24.16.0 / npm 11.13.0 / darwin arm64；lockfile sha256 `a9735825…`（未变）
- 全量 npm test 终态：14759 绿 / 4 红 = F1 基线（未扩大）；eslint 0 error（F2 保持关闭）

## 2. P04 建立的契约（P05 消费面）

| 文件 | 对 P05 的用途 |
|---|---|
| docs/refactor-2026/P04/MODEL_CALLSITE_MATRIX.json | 全部出站调用族与观测/凭证/用量归属——P05 消息语义只消费本表事实，不重新推断 |
| docs/refactor-2026/P04/USAGE_OWNERSHIP.md §1 | 五个计量观察点与 modelCallId 去重身份——P05 历史投影不得新增 usage 写入 |
| docs/refactor-2026/P04/TRACE_COMPAT_REPORT.md | mt_ 会话级复用、独立根枚举、parentCallId 因果规则——P05 历史恢复沿用同一 trace 语义 |
| docs/refactor-2026/P04/STREAM_RETRY_POLICY.md §1 | 消费端只见 assistant_event_normalizer 规范化事件——P05 的实时/历史/重连同语义以此为准 |

**规范化事件 / 最终 request artifact / 分段来源接口（任务书 T08-3 交付）**：
1. 规范化事件：`server` assistant-event-normalizer 输出（正文/推理/阶段元数据/结束原因），实时 WS 与历史投影共用（P05 不得自建第二投影器）。
2. 最终 request artifact：观测库 payload 四层（semantic_request / provider_request / provider_response / semantic_response），由 `query.getPayloadRecord(id)` 读取；`semanticInputProvenance.sections` 提供分段来源（task_instruction/task_input/adapter_injected/media_reference 等类别 + root/path 定位）。
3. 调用身份关联：ledger entry `metadata.{modelCallId,traceId,parentCallId}`；session JSONL 内 `hana-model-call-reference-v1` custom entry（model-call-correlation.ts persistModelCallReferenceForMessage）。

## 3. 本阶段门禁（P05 不得削弱）

1. P01–P03 全部门禁延续绿（typecheck×3 / core-contracts / dependency×2 / tool-invocation-boundaries / cli-closure / lint:boundary / eslint 0 error）。
2. 新增防漂移：tests/model-operation-resolver.test.ts P04-A01（真实 ProviderRegistry 联合键）——改 provider 匹配语义必红；P04-A02/A08（fail-closed / 取消不复活）同理。
3. harness `sse-bytes` 脚本为 A06 专用，勿删（删则 e2e S3 失去字节边界能力）。

## 4. P05 主责输入（移交与确认）

- P00 REFACTOR_BACKLOG P05 行：S10 规范化事件+旧正文兼容核查、knowledge research 残留表退出决策（授权项）、会话双身份引用约束文档化。
- 本阶段移交：观测/台账/trace 三面身份（mc_/mt_/modelCallId）已收敛且有测试锁定——P05 只做消费侧投影，不改铸造。
- BLOCKED 继承：真供应商冒烟（P04-T07-2）授权后执行；F1/F3 归 P08。

## 5. 已执行验证与遗留

- 已绿：typecheck×3、五门禁、eslint、定向 36 文件 410 例、全量（F1 基线 4 红）。
- 负向验证 2 组留档：provider 联合键注入红 / observer 漏装注入红（COMBINATION 式证据在 OPERATION_COVERAGE_MATRIX.json fault_injections）。
- BLOCKED（环境，继承）：build:server:open / 四平台 CI / 真实供应商——P08/授权后取回。

## 6. 必保留兼容（P05 不可改变）

- P02/P03 交接全部条目继续有效。
- 本阶段新增：观测分组语义（user_turn 会话级 mt_ 复用、后台独立根）零变化；凭证解析单道与 ambient 拒绝边界零变化；usage 写入面零变化（观察点全枚举见 USAGE_OWNERSHIP §1，P04-FIXR1 修订后为 7 处生产写入边界 + 1 消费投影，均 modelCallId 键控同一台账）；`recordAssistantUsage`/`hana-model-call-reference-v1` 关联机制零变化。
- 生产代码本阶段零改动——不存在 P04 引入的生产行为差异。

## 7. 当前数据版本

- observability.sqlite SCHEMA_VERSION=7（未动）；usage ledger STORAGE_VERSION=1（未动）；零迁移。
- P05 若涉及历史数据兼容，按当前 schema 取证，不假设本阶段有版本变化。

## 8. 工作区卫生提醒（继承+新增）

- 全量 npm test 后检查 `artifacts/f1-f12-repair/round2/patches/89bc0b64-to-r01-r10-source.patch`（本阶段再次重写并已还原，stat 留档 artifacts/refactor-2026/P04/logs/P04-T08-f5-patch-side-effect.stat.txt）。
- 本轮新增未跟踪目录：docs/refactor-2026/P04/、artifacts/refactor-2026/P04/；新增测试并入 4 个既有文件 + harness 扩展（无新测试文件）。
- EVIDENCE_SHA256.txt 为证据链最终步：任何日志追加后须重新生成清单。

## 9. 下一阶段唯一允许修改范围

P05（消息语义、历史恢复、资源与数据兼容）：仅消息/历史/资源/导出投影及其测试与文档；不得触碰模型/凭证/流式/观测铸造面（本阶段已收敛并锁定）；发现该面问题登记回 P04 范畴处理。
