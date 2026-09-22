# NEXT_STAGE_HANDOFF — P06 输入（P05 → P06）

日期：2026-09-22｜P05 结果：见 P05_RESULT.json（实施+本地验证完成；真供应商冒烟 BLOCKED 单项自 P04 继承，本阶段新增 BLOCKED：无）。

## 1. 已验收坐标与环境

- 工作区 START = `1f0537b08`（P04 提交后）；END 候选 = 同（零生产 commit；工作区改动 = 3 个既有测试文件扩展 + P05 证据目录未跟踪，等待编排层统一提交）
- 分支 `docs/knowledge-closeout-2026-09-21`（未切换）；Node v24.16.0 / npm 11.13.0 / darwin arm64；lockfile sha256 `a9735825…`（未变）
- 全量 npm test 终态与门禁结果见 P05_REPORT §验证（F1 基线未扩大为前提）

## 2. P05 建立的契约（P06 消费面）

| 文件 | 对 P06 的用途 |
|---|---|
| docs/refactor-2026/P05/MESSAGE_SEMANTICS.md + SEMANTIC_FIXTURES.json | 内容类型唯一语义权威与 fixtures 索引——P06 上下文装配/提示词引用同一语义来源，不得自建第二投影 |
| docs/refactor-2026/P05/SESSION_IDENTITY_CONTRACT.md | sess_/SDK UUID 双身份约束——P06 上下文按会话取数必须经边界解析，不合成身份 |
| docs/refactor-2026/P05/HISTORY_PARITY.md | 分页游标=display 序号读取位置；热页 O(K+|Dpage|) 有界读取实测（jsonlParseCount 恒 103）——P06 不得引入每页全读 |
| docs/refactor-2026/P05/RECONNECT_CONTRACT.md | streamId/seq 语义完整，P06 无需新增协议字段 |
| docs/refactor-2026/P05/RESOURCE_HISTORY_CONTRACT.md + CONSUMER_MIGRATION.md | 资源身份/逐块授权/兼容 adapter 边界——P06 人格/资料边界评测的取数约束 |
| docs/refactor-2026/P05/DATA_COMPATIBILITY.md | 数据版本矩阵（observability v7 / knowledge v19 / file-history v1…）与回退语义 |

**P05 语义链一句话**：provider 事件 → 保留协议边界（ThinkTag/Mood parser）→
AssistantEventNormalizer（唯一正文裁决）→ stream store（streamId/seq）→ WS →
use-stream-buffer（canonical 主真相）→ **projectAssistantTurn** ⇄ 历史侧
extractPersistedAssistantSemanticSegments → projectHistoryPage → history-builder →
**projectAssistantTurn**（同一投影器汇合）。P06 的一切上下文/人格/记忆注入不得
在此链外另造正文或结局裁决。

## 3. 本阶段门禁（P06 不得削弱）

1. P01–P04 全部门禁延续绿（本轮实测：typecheck×3 / core-contracts / dependency /
   tool-invocation-boundaries / cli-closure / lint:boundary / eslint 0 error）。
2. P04 防漂移测试（model-operation-resolver 等）与 harness `sse-bytes` 脚本继续保留。
3. P05 新增锚点：chat-route-switching `P05 stream resume semantics`（A03–A06，真实路由×
   真实 store）、stream-resume 跨流 seq 例、tool-presentation-history A11 例、
   artifacts/refactor-2026/P05/logs/P05-FAULT-INJECTION.test.ts（FI-1/FI-2 反例，已并入
   默认 vitest 集）。改恢复/分页/段语义必红其一。

## 4. P06 主责输入（移交与确认）

- P00 REFACTOR_BACKLOG P06 行：payload 捕获补全提示词预算（PROMPT_BASELINE
  not_measured_yet）、人格/记忆/资料边界评测扩展。
- P04 遗留的 `query.getPayloadRecord(id)` 四层 payload 与
  `semanticInputProvenance.sections` 分段来源接口是 P06 提示词预算的直接工具。
- 本阶段移交：消息/历史/资源语义已文档化收口（§2）——P06 只在其上装配上下文，
  不改投影语义；发现该面问题登记回 P05 范畴处理。
- BLOCKED 继承：真供应商冒烟（P04-T07-2）授权后执行；F1/F3 归 P08。

## 5. 已执行验证与遗留

- 已绿：typecheck×3、六门禁、eslint、定向套件（T03–T07 各命令，见 command-log）、
  全量 npm test（结果见 P05_REPORT）。
- 负向验证 2 组留档：FI-1（commentary→final 篡改被 parity 断言捕获）、FI-2（游标
  误用源下标被页欠填/页数学断言捕获）——迭代过程 4 次命令全留档。
- P04 复验收登记的轻微项（P04/NEXT_STAGE_HANDOFF §2「五个计量观察点」措辞与 §6
  枚举不一致）：本阶段未合法触碰该文件，**未顺带修正**（避免为措辞扩大范围），
  继续留给下次合法触碰该文件者。

## 6. 必保留兼容（P06 不可改变）

- P02/P03/P04 交接全部条目继续有效。
- 本阶段新增：消息语义链（normalizer 唯一裁决、canonical/legacy 并存、保留协议仅
  边界结构化）零变化；streamId/seq 与 resume 协议零变化（含 reset/truncated 语义）；
  分页游标 display 序号语义零变化；历史资源身份与逐块授权零变化；
  WS `text_delta` 兼容输出保留（退出条件见 CONSUMER_MIGRATION §3，产品契约）；
  双身份不合并（SESSION_IDENTITY_CONTRACT）。
- 生产代码本阶段零改动——不存在 P05 引入的生产行为差异。

## 7. 当前数据版本

- 本阶段零 schema 变更、零迁移（DATA_COMPATIBILITY §1 全表）；指纹守卫 OK（170
  watched 无触碰）。P06 若改持久化结构须按 T06 同口径先行取证。

## 8. 工作区卫生提醒（继承+新增）

- 全量 npm test 后检查 `artifacts/f1-f12-repair/round2/patches/89bc0b64-to-r01-r10-source.patch`
  未被测试重写（P04 曾两次遇到；本轮完成后已核对，见 P05_REPORT）。
- 本轮新增未跟踪目录：docs/refactor-2026/P05/、artifacts/refactor-2026/P05/；
  修改文件仅 3 个既有测试（chat-route-switching / stream-resume / tool-presentation-history）。
- EVIDENCE_SHA256.txt 为证据链最终步：任何日志追加后须重新生成清单。

## 9. 下一阶段唯一允许修改范围

P06（上下文、提示词、记忆与能力集成）：仅上下文装配/提示词/人格/记忆/资料边界/工具
行为评测及其测试与文档；不得触碰消息语义裁决面、流恢复协议、历史分页投影与资源
授权面（本阶段已收口并锁定）；发现该面问题登记回 P05 范畴处理。
