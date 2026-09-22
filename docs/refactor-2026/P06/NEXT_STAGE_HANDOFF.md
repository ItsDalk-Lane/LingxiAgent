# NEXT_STAGE_HANDOFF — P07 输入（P06 → P07）

日期：2026-09-22｜P06 结果：见 P06_RESULT.json（确定性契约面 PASS；真实模型行为评测 BLOCKED——无凭证/费用授权，继承 P04-T07-2；本阶段新增 BLOCKED 仅此一项）。

## 1. 已验收坐标与环境

- 工作区 START = END 候选 = `93b8b7265`（P05 提交后；零生产 commit；工作区改动 = 1 生产文件 + 1 既有文档注记 + 4 新测试文件 + P06 证据目录未跟踪，等待编排层统一提交）
- 分支 `docs/knowledge-closeout-2026-09-21`（未切换）；Node v24.16.0 / npm 11.13.0 / darwin arm64；lockfile sha256 `a9735825…`（未变）
- 全量 npm test：**4 红 = F1 已知基线同一组**（逐名比对 P05 一致；14792 绿 / 1 expected fail / 15 跳过；无第 5 红；对账见 logs/ITERATIONS.md）

## 2. P06 建立的契约（P07 消费面）

| 资产 | 对 P07 的用途 |
|---|---|
| docs/refactor-2026/P06/CONTEXT_ASSEMBLY_MAP.md | 最终请求装配来源地图（五变体单源）——P07 性能测量所见的 systemPrompt/tools 构成按此解释，勿另建口径 |
| docs/refactor-2026/P06/PROMPT_BUDGET_REPORT.md | 预算账本（A/B/C ≤ P00 基线；D 常驻工具 schema 2420 tok；E 动态资料单列）——P07 前缀缓存/上下文体积优化的对照基线 |
| docs/refactor-2026/P06/TOOL_BEHAVIOR_EVAL.json（sha256 8e91c139…） | 40 固定 + 8 留出行为样本——P07 优化后不得降低样本可达能力；授权后配对评测沿用 |
| tests/p06-*.test.ts（4 文件 19 例，默认集） | 装配/预算/边界/评测回归锚点：改装配、常驻面、开关语义、校验反馈必红其一 |
| ACCEPTANCE_MAP.json | A01–A14 场景→测试映射 |

**FIX-1（P06 唯一生产改动）**：`lib/tools/invocation/schema-validator.ts` 参数校验 message 字段化（`Invalid field(s): …`）+ normalizeIssues path/instancePath 兼认。P07 若动校验/错误文案须保持字段定位可见（tests F-17/F-37 守卫）。

## 3. 本阶段门禁（P07 不得削弱）

1. P01–P05 全部门禁延续绿（本轮实测：typecheck×3 / core-contracts 6 files / tool-invocation-boundaries / lint:boundary / 定向套件 / 工具面边界 67 例）。
2. P05 锁定面（消息语义裁决、streamId/seq 恢复、分页投影、资源授权）本阶段零触碰——P07 同样不得改；发现该面问题登记回 P05 范畴。
3. P06 新增锚点：equivalence/section-order（既有）+ 4 个 p06 文件 + F-17/F-37 字段反馈断言。

## 4. P07 主责输入（移交与确认）

- P00 REFACTOR_BACKLOG P07 行：W3/W4 补全 + 30 次口径全量（BENCHMARK_PROTOCOL 实验受限项）；启动/历史基线复测对照。
- **性能优化的硬边界**：不得通过少提供能力（目录/按需发现）、省略安全规则（权限/审批/校验）、删人格/记忆内容或关闭记忆凑数字取胜（PROMPT_BUDGET_REPORT budget_rule + A08 场景）。同负载配对比较（BENCHMARK_PROTOCOL / P00 10% 初始阈值）。
- 测量时的上下文构成解释入口：CONTEXT_ASSEMBLY_MAP §1–§2（最终请求段序/来源/动态性/缓存性）。

## 5. 已执行验证与遗留

- 已绿：typecheck×3、core-contracts、tool-invocation-boundaries、lint:boundary、定向套件（任务书 §7 命令 + p06×4 = 24 例）、工具面边界 67 例、全量 npm test（F1 基线未扩大）。
- BLOCKED 继承：真供应商冒烟（P04-T07-2）与真实模型行为评测（本阶段）均待授权；F1/F3 归 P08。
- 非关键登记：P00 PROMPT_BASELINE C 组件 2 字节计量口径差（PROMPT_BUDGET_REPORT §2）；P04 NEXT_STAGE_HANDOFF §2 措辞不一致项仍未合法触碰（同 P05 处置，留给下次合法触碰该文件者）。

## 6. 必保留兼容（P07 不可改变）

- P02–P05 交接全部条目继续有效。
- 本阶段新增：canonical 装配单一权威（buildSystemPromptArtifact 唯一基座；text==artifact.text）；常驻面 = read/write/edit/exec_command + 3 目录桥；常驻文案 golden 字节锁定（A/B 组件 = P00 基线）；schema 校验错误字段级反馈；人格/记忆开关语义（master 立即重建 / per-session 隔离）。

## 7. 当前数据版本

- 本阶段零 schema 变更、零迁移（P05 DATA_COMPATIBILITY §1 全表继续有效）；指纹守卫未触碰。

## 8. 工作区卫生提醒（继承+新增）

- 全量 npm test 后检查 `artifacts/f1-f12-repair/round2/patches/89bc0b64-to-r01-r10-source.patch` 未被测试重写（本轮已完成 git checkout 还原并复核）。
- 本轮新增未跟踪：docs/refactor-2026/P06/、artifacts/refactor-2026/P06/、tests/p06-*.test.ts×4；修改：schema-validator.ts、REFACTOR_BACKLOG.md（注记）。
- EVIDENCE_SHA256.txt 为证据链最终步：任何日志追加后须重新生成清单。

## 9. 下一阶段唯一允许修改范围

P07（可测量的启动、运行与界面性能优化）：仅性能测量/优化及其测试与文档；不得触碰消息语义裁决面、流恢复协议、历史分页投影、资源授权面（P05 锁定）与 canonical 装配/常驻文案/校验反馈（P06 锁定）；不得以降低能力/安全换性能。发现锁定面问题登记回对应阶段处理。
