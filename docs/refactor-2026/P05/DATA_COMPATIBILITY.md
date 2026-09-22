# DATA_COMPATIBILITY — 数据格式变更与回退（P05-T06）

日期：2026-09-22｜基线 HEAD：`1f0537b08`。

## 1. 本阶段数据变更结论：**无 schema 变更，无迁移**

P05 没有修改任何生产持久化写入路径（工作区 diff = 3 个测试文件，0 生产文件）。
因此按任务书 T06-1"没有必要则明确无 schema 变更，不创建迁移以显示工作量"，
本阶段**不新增任何迁移**。下表为受影响数据面在当前源码的版本取证：

| 数据类别 | 版本（生产常量） | 原始/派生 | 本阶段变化 |
|---|---|---|---|
| 模型观测库 observability.sqlite | `MODEL_OBSERVABILITY_SCHEMA_VERSION = 7`（lib/llm/model-observability-schema.ts:30） | 原始 | 无（P04 交接 §7 确认未动，本阶段只读消费） |
| usage ledger | STORAGE_VERSION=1（P04 USAGE_OWNERSHIP） | 原始 | 无 |
| knowledge.sqlite | `KNOWLEDGE_SCHEMA_VERSION = 19`（lib/knowledge/knowledge-store.ts:56） | 原始 | 无（P05-2 决策见 §4） |
| 会话 JSONL（agents/*/sessions/*.jsonl） | 无整体版本号；条目级自定义 entry（如 hana-model-call-reference-v1） | 原始 | 无 |
| session-manifest.db | `user_version`（SESSION_MANIFEST_DB_USER_VERSION，core/session-manifest/store.ts:336） | 原始 | 无 |
| file-history history.sqlite | `FILE_HISTORY_SCHEMA_VERSION = 1`（lib/file-history/history-store.ts:20；拒绝打开更新版本） | 原始 | 无 |
| 会话文件注册表 sidecar（.files.json） | raw.version === 1（resource-service 读取约束） | 原始 | 无 |
| workspace snapshot sidecar | 版本化 sidecar（core/workspace-snapshots.ts） | 原始 | 无 |
| 历史读取目录缓存 | 进程内缓存 + publicRevision | **派生**（可重建，重建后输出等价由 T04 测试锁定） | 无 |

## 2. 备份/恢复与中断演练（A14/A15 机制证据，本阶段全量回归 109 例绿）

仓库现行 data-epoch 机制即一致性备份/恢复通道（P05 未改动它，只回归）：

- **一致性快照与验证**：coordinated epoch transition 按"checkpoint → barrier →
  migration → validation → final commit"严格持久序（data-epoch-coordinator
  "runs checkpoint, barrier, migration, validation, and final commit in exact durable order"）；
  恢复前置校验 captured bytes 未被篡改（"rejects when the checkpoint's captured bytes
  have been tampered with"）。SQLite 一致性路径由 checkpoint provider 机制承接，
  不在线只复制主文件（A14 机制基础）。
- **迁移中断（A14）**：barrier/commit 成对持久；中断产生"不可能的 v2 关系"时
  fail-closed（data-epoch "fails closed on corrupt JSON and impossible v2 epoch
  relationships"、"fails closed on interrupted metadata writes"）；journal 相位未知即拒绝
  不猜测——幂等完成或可解释失败，不激活半套数据。
- **回退新数据保护（A15）**：restore 走受控降级通道（需 restore journal 在盘、
  confirmToken 逐字符校验）；隔离目标**永不覆盖**——重扫冲突文件落 .dup-N 旁侧，
  两份都可归因（data-epoch-restore "never overwrite, keep both copies attributable"）；
  forward/restore journal 互不可读（对称防误用）；低版本读者被挡且 stamp 永不回降
  （coordinator "blocks a lower reader, permits only the explicit steady-state override,
  and never lowers the stamp"）。
- 可回退组合矩阵（现行语义）：代码回退（旧读者）+ 数据不降级 → 旧读者被 stamp 挡住
  显式失败而非误读；需要回退数据时走 restore journal + 隔离，不直接覆盖新快照。

## 3. 指纹（T06-5）

- 本阶段受护持久化源零改动：`node scripts/check-persistence-schema-fingerprint.mjs`
  → "no guarded persistence sources touched (170 watched); OK"（P05-T06-fingerprint-check，
  exit 0）。无语义变化 → 无 repin、无历史封印触碰、无 tripwire 关闭。
- persistence-schema-tripwire / persistence-store-registry 测试同轮全绿。

## 4. P05-2 决策登记：knowledge research 残留（范围决策，非数据变更）

P00 SCOPE 将 knowledge research pipeline 列为 HARD_EXCLUDED 的**兼容残留**：
`lib/knowledge/knowledge-store.ts` 仍建 research_runs / knowledge_research_runs 等
表族，且 `hasResearchReferencesForSource`（:3688-3701）在**删除保护**判定中读取
knowledge_research_runs（研究原文凭据/证据/完整性分母来源保护）。
P05 复核结论：

- 该残留的全部生产读取都是"旧数据删除保护"（防旧研究数据被误删），不再驱动任何
  研究执行路径；删除保护语义对旧数据安全是必要的。
- **决策：保留只读兼容现状**。退役（删表/删保护读取）属治理变更，需用户明确授权
  （REFACTOR_BACKLOG"需用户决策"第 2 条），留给 P08 或单独授权处理；本阶段不删、
  不改语义、不新增读取。

## 5. 旧记录缺字段（A13，跨任务关联）

历史投影对缺新字段的旧记录行为由既有测试锁定：textSignature 缺失 → final_answer
缺省（assistant-semantic-segments:62）；text_end 契约表（无 existing segment 的各形态）；
tool-presentation-history "旧写入从调用参数恢复新正文，不捏造覆盖前内容"（before
内容不可得时显式 before_content_unavailable，不伪造）；history 协议兼容
（history-protocol-compat/conditional，本轮 T04 套件全绿）。未知字段不伪造、
原文件摘要不变。

## 6. 本阶段执行命令（全部 exit 0，详见 command-log.jsonl）

1. data-epoch×4 + persistence tripwire/registry：`npx vitest run tests/data-epoch*.test.ts
   tests/persistence-*.test.ts` → 109/109 绿（P05-T06-data-epoch-suite）。
2. `node scripts/check-persistence-schema-fingerprint.mjs` → OK（P05-T06-fingerprint-check）。
