# P04 阶段执行报告｜模型、凭证、流式请求与用量观测

日期：2026-09-22｜执行：ZCode（用户指定阶段执行）

## 结论

**BLOCKED（外部验证环境受阻；实施与本地验收全部完成）**。8/8 任务执行完毕、16/16 验收场景本地真实执行 PASS（含 9 例本阶段新增）；唯一未闭合项 = P04-T07-2 真实供应商冒烟（无用户凭证/预算授权，按任务书 §10 与通用约束 §4 记 BLOCKED，不虚报 PASS）。授权后仅补该单项即可转 PASS。本地结论的置信边界：全部协议族经本地 loopback witness 真实 HTTP + 真实生产栈（Pi/callText/operation/media/speech）验证；不代替真实账户行为。F1（治理 4 红）按 P00 定级保留阻塞 P08；F3（网络）继承。

## 实际输入

- START_SHA = `3286c96e571fadea65143df8baabf503d30d37f2`（P03 收尾提交）；END 候选 = 同（零生产代码 commit，改动全部为工作区状态，等待编排层统一提交）
- 研究基线 `8037fae7a` 与 HEAD 的 P04 相关路径漂移：model-operation-resolver/model-manager/pi-sdk/model-trace-scope/llm-client/observer 族在 P01–P03 阶段零漂移（P03 交接核对延续）；行号按当前 HEAD 重新定位
- 分支 `docs/knowledge-closeout-2026-09-21`（未切换）；Node v24.16.0 / npm 11.13.0 / darwin 27 arm64；lockfile sha256 `a9735825cea1d018…`（与 P02/P03 交接一致，未变）
- 前置：P03 PASS（P03_RESULT.json + NEXT_STAGE_HANDOFF 全部读取：门禁清单、F2 关闭基线、工具诊断日志接口、工作区卫生规则）

## 任务逐项结果

| 任务 | 实现 | 生产入口与源码 | 测试与日志 | 旧路径去向 | 状态 |
|---|---|---|---|---|---|
| T01 出站全量映射 | UNCHANGED_VERIFIED（census 交付） | 7 个调用族 + 4 个凭证事实所有者，见 [MODEL_CALLSITE_MATRIX.json](MODEL_CALLSITE_MATRIX.json) | P04-BASE-* 四组（378 例基线） | 无未解释直调（残留清单空） | PASS |
| T02 模型选择与新鲜凭证 | UNCHANGED_VERIFIED + 补测 7 例 | model-operation-resolver/model-manager:589/oauth-force-refresh/provider-registry:1119 | resolver+client 新增（A01/A02/A04/A05×2/A08）；既有 oauth/auth-storage/no-fallback/fresh-routing 族复验 | 无重复凭证回退链；AuthStorage 系保留（SDK 锁） | PASS |
| T03 公共请求与供应商差异 | UNCHANGED_VERIFIED | llm-client（5 协议白名单）/operationDialect（9 操作协议）/provider-compat（16 文件） | 既有契约测试 + e2e witness 协议族 | 最终 payload 与观测同源（e2e S1 hook≡witness） | PASS |
| T04 流式/重试/取消 | UNCHANGED_VERIFIED + 新增真实协议栈测试 | stream-guard/stream-observer/readCodexResponsesStream/normalizer | e2e S3（A06：交错分片+UTF-8 字节边界）；retry-visibility/normalizer/p02-cancellation 既有 | 无第二套流解析；重试策略分层登记 | PASS |
| T05 轨迹分组与因果 | UNCHANGED_VERIFIED | model-trace-scope/session-coordinator:5032/model-call-correlation | trace-propagation 11 组 + trace-scope 21 例 + session-reuse 7 例 | 无并行 trace 体系 | PASS |
| T06 用量对应 attempt | UNCHANGED_VERIFIED | usage-ledger/model-request-accounting/recordAssistantUsage/accounting-projection | ledger 11 例 + projection 10 例 + reference | 五个观察点枚举；无独立 usage 写入 | PASS |
| T07 全操作接线 | MODIFIED（仅测试 +9 例） | 见 [OPERATION_COVERAGE_MATRIX.json](OPERATION_COVERAGE_MATRIX.json) | targeted 36 文件 410 例全绿；负向注入 2 组（注入红→还原绿） | — | PASS；真供应商冒烟 BLOCKED |
| T08 退出与交接 | MODIFIED（文档） | P00 ENTRYPOINT_MATRIX/REFACTOR_BACKLOG 注记 | typecheck×3/五门禁/lint/定向/全量 | 无旧凭证/usage 路径需删除 | PASS |

## 场景逐项结果

A01–A16 全部 PASS，逐条映射（测试文件/用例名/fixture/生产入口/命令/证据）见 [ACCEPTANCE_MAP.json](ACCEPTANCE_MAP.json)。真实供应商冒烟单列 BLOCKED（无授权）。

## 接线和旧路径

- **本阶段生产代码零改动**（4 个测试文件 + 1 个测试 harness + P00 两处文档注记）。
- 新增测试全部并入既有测试文件（遵守 P02 交接 §3 spawn 安全范式；无新测试文件）。
- **无双写**：凭证解析单道（resolveProviderCredentialsFresh + Pi getAuth 边界）；usage 五观察点各归其位；payload 组装与观测同源。
- **旧路径核查结论**：census 未发现绕过 resolver 的密钥回退链、独立 usage 写入或冗余 payload 组装；`legacy_paths` 为空是核查结论而非未查（T01 矩阵 + grep 交叉核实）。AuthStorage/FileAuthStorageBackend 出口为 SDK 0.83.0 锁定合法使用，按任务书 T08-1 保留。

## 验证

命令全记录：`artifacts/refactor-2026/P04/logs/command-log.jsonl`（25 条全格式，含 2 条开发期失败迭代、2 条故意注入红、1 条全量 F1 基线）。关键终态：

| 检查 | exit | 结果 |
|---|---|---|
| typecheck ×3 | 0 | 绿 |
| core-contracts / dependency-boundaries / tool-invocation-boundaries / cli-closure / lint:boundary | 0 | 全绿 |
| eslint 全仓 | 0 | 0 error（10733 警告=既有风格类，P03 同口径 10720+本阶段测试文件风格一致增量） |
| 定向 36 文件（P04-VERIFY-targeted-25） | 0 | **410/410** |
| 全量 npm test | 1 | 14759 绿 / **4 红 = F1 基线**（seal×1+round2×2+round3×1；与 P00 登记一致未扩大；+9 例为本阶段新增） |

环境受阻：F3 继承（build:server:open / 四平台 CI / 真实供应商）。测试全部使用合成数据 + mkdtemp 临时目录 + 本地 loopback 随机端口 witness；未触及真实用户 HOME/会话/凭证/账户。全量测试对 `89bc0b64-to-r01-r10-source.patch` 的已知重写副作用再次发生，取证（stat 留档 `P04-T08-f5-patch-side-effect.stat.txt`）后 `git checkout --` 还原成功（shasum 校验 OK）。

## 数据、权限与平台

- 零数据格式改动；零迁移；零权限语义变化。
- 凭证安全：毒丸（合成密钥）多路径零泄漏（safety-gate/redaction/e2e 字节级扫描）；ambient 环境凭证拒绝边界未动。
- 实际验证平台：仅本地 darwin arm64；不代替其他平台、正式打包或真实供应商。

## 差异与限制

1. **BLOCKED 项**：真实供应商冒烟（T07-2）——需用户授权凭证/预算与目标 provider 清单；补偿证据为本地 witness 全协议族 + 负向注入。授权后仅需执行该单项。
2. F1/F3 按 P00 定级保留（F1 阻塞 P08 发布门禁、F3 阻塞平台矩阵），非本阶段验收项。
3. harness `sse-bytes` 扩展为测试基建（生产零改动）；既有 7 类 witness 脚本零改动。
4. P04 建议文件名与实际一处不同：操作覆盖矩阵落为 OPERATION_COVERAGE_MATRIX.json（任务书未指定精确名）。

## 回退与下一阶段

回退步骤（按序）：①`git checkout -- tests/ docs/refactor-2026/P00/ENTRYPOINT_MATRIX.md docs/refactor-2026/P00/REFACTOR_BACKLOG.md`（6 文件，全部为工作区改动未提交）；②删除未跟踪 docs/refactor-2026/P04/ 与 artifacts/refactor-2026/P04/；③若已授权提交则 revert 对应 commit。无数据需要回滚（零持久化改动）；观测/台账/凭证语义未动，无需失效动作。

下一阶段交接：[NEXT_STAGE_HANDOFF.md](NEXT_STAGE_HANDOFF.md)（P05 输入：规范化事件/最终 request artifact/分段来源接口）。

## 独立验收修复轮（P04-FIXR1，2026-09-22，一项）

来源 = 独立验收 [P04_ACCEPTANCE_REVIEW.md](P04_ACCEPTANCE_REVIEW.md) 问题 #1（非阻塞，文档精度）：USAGE_OWNERSHIP.md §1「无其他 usage 写入点」枚举不全。零生产代码、零测试改动；仅文档与记录文件。

**改动**（6 个既有文件修订 + 8 个新日志文件，其中 manifest-check 2 个按设计不入清单）：

- [USAGE_OWNERSHIP.md](USAGE_OWNERSHIP.md) §1：表新增 3 行并废止初版不完全的 grep 声称——
  - #5 `lib/llm/observed-pi-direct-summary.ts:158-204`（MC-10 日记直连摘要，验收指出）：start→finish/recordError 直写；键控 requestId + metadata.modelCallId（=recorder.callId）；ledger 由 pi-sdk facade generateSummary 第 14 参 observerContext 显式注入，传 streamFn 时不经此路径（防双计）。
  - #7 `lib/llm/provider-client.ts:344`（connectivity-probe，验收指出）：仅 anthropic-messages 真实生成探测分支经 withModelRequestAccounting 写账，其余协议 GET /models 为 CONTROL_PLANE 零写入；键控 metadata.modelCallId（observedModelCallLedgerMetadata(recorder)）。
  - #6 `lib/llm/cache-preserving-compaction-agent-run.ts:423`（MC-02 缓存保留压缩 run，**本轮复核新发现、验收未列**）：core/session-compactor.ts:1878 生产可达，同样漏列；键控 metadata.modelCallId（=mintModelCallId()）。
  - 登记潜在写入点 `lib/llm/session-snapshot-side-task-runner.ts:83`：有同构直写但唯一上游 runMemoryReflection（lib/memory/memory-reflection-runner.ts）无生产调用方，当前不可达、不计入；未来接线须补表。
- P04_REPORT.md（本节）、P04_RESULT.json（fix_rounds）、ACCEPTANCE_MAP.json（fix_rounds）：补记本轮条目。
- NEXT_STAGE_HANDOFF.md §6：「usage 五观察点零变化」对齐为「写入面零变化（§1 修订后 7 处写入边界）」——交接契约与修订后枚举的一致性对齐（文档措辞，语义不变）。
- EVIDENCE_SHA256.txt：6 条目刷新（USAGE_OWNERSHIP.md、P04_REPORT.md、P04_RESULT.json、ACCEPTANCE_MAP.json、NEXT_STAGE_HANDOFF.md、command-log.jsonl）+ 新增本轮 6 个 P04-FIXR1 日志条目（三个 run-logged 命令各 .out/.err；64→70 行），格式不变。

**与验收描述的差异**（以源码为准）：验收称两处「均在 MODEL_CALLSITE_MATRIX 对应 family 的 usage 字段中有归属记载」——MC-10 属实（pi-direct-summary family）；connectivity-probe 链路（server/routes/providers.ts:825 → probeProvider）**不在** MODEL_CALLSITE_MATRIX 任何 family（其 provider-probe/MC-08 family 仅覆盖 server/routes/models.ts health-check 链），操作覆盖记载实际位于 OPERATION_COVERAGE_MATRIX provider-probe family。矩阵本轮不改动（保持既有交付哈希），该缺口在此登记为遗留观察。

**复核命令与结果**（run-logged，全格式入 command-log.jsonl）：

| 命令 | exit | 结果 |
|---|---|---|
| P04-FIXR1-writepoints-grep（第一版正则，漏 `?.` 可选链形态） | 0 | 17 行——枚举不完备，留档为迭代记录 |
| P04-FIXR1-writepoints-grep2（修正正则：直调四方法含 `?.` 形态 + withModelRequestAccounting 全调用方；lib/core/server/hub/cli 生产目录） | 0 | 36 行，与 §1 表 7 处写入边界逐一对应（helper 自身 4 行 + 调用方；desktop 另查无命中） |
| P04-FIXR1-json-validate（RESULT/ACCEPTANCE_MAP/MODEL_CALLSITE/OPERATION_COVERAGE 四 JSON 解析） | 0 | 全部合法 |
| 清单终验 `shasum -a 256 -c`（直接重定向，不入 jsonl 以保清单哈希稳定） | 见 P04-FIXR1-manifest-check.out | 逐条 OK/FAILED 与 exit code 留档该日志 |

前轮事实不重写：正文 T06 行「五个观察点枚举；无独立 usage 写入」与 RESULT T06 legacy_action 为初版表述，「无独立 usage 写入系统」的结论本身仍成立（三处新列写入点全部经同一 usageLedger + modelCallId 键控，无第二套账本）；枚举数量以本节与 USAGE_OWNERSHIP §1 修订版为准。
