# P04 独立验收报告（验收子代理，全新上下文）

日期：2026-09-22｜验收对象：P04 工作区交付（HEAD `3286c96e571fadea65143df8baabf503d30d37f2` + 未提交改动）｜角色：只读独立验收，不修改生产代码/测试/文档（本报告与证据日志为模板允许的唯一写入）。

## 结论

**阶段总状态：BLOCKED（与执行者判定一致，且理由成立）**。

- 本地可验证范围内全部通过：8/8 任务、A01–A16 全部本地场景、新增 9 例测试、定向 36 文件 410 例、typecheck×3、五门禁、eslint 0 error、全量 14759 绿/4 红 = F1 基线未扩大——以上均由本验收**独立复跑或逐项核对原始日志与源码**确认，非采信执行者自报。
- 唯一未闭合项 P04-T07-2 真实供应商冒烟：核实确属任务书 §10（「缺少已要求真供应商验证时仅实施完成，阶段验收 BLOCKED，不虚报」）与通用约束 §4/§5（真实付费请求需明确授权凭证/预算/上限，无授权写 BLOCKED）允许的 BLOCKED 情形。本环境同样无凭证/预算授权，验收侧无法补做。补偿证据（本地 loopback witness 真实 HTTP + 真实生产栈全协议族 + 负向注入 2 组）核实为真实存在且与声称一致。
- 该 BLOCKED **不是用状态掩盖 FAIL**：本地全部必跑命令留档且失败链完整（含 4 条开发期 FAIL→fix 重跑、2 条故意注入红），无「未运行冒充通过」、无改弱断言、无删除历史兼容或修改旧审计证据。

## 逐项核实

### 1. 改动面与「生产代码零改动」声称

- `git status` / `git diff HEAD --stat`：7 个已跟踪文件改动（4 测试文件 +9 例、1 测试 harness、P00 两文档各 +2 行注记）+ 2 个未跟踪目录（docs/artifacts/refactor-2026/P04/）。**无任何生产源码文件出现在 diff 中**，UNCHANGED_VERIFIED 的改动面前提成立。
- diff 逐 hunk 审阅：全部为纯新增；harness `sse-bytes` 为新增分支，既有 7 类 witness 脚本行为不变（`apply` 转 async 后非 sse-bytes 路径无 await，语义等价；e2e 全场景回归绿）。无断言改弱、无测试删除、无审计封印文件（.sync-audit/）触碰。
- F5 已知副作用：`artifacts/f1-f12-repair/.../89bc0b64-to-r01-r10-source.patch` 当前与 HEAD **sha256 完全一致**（`25fb315f…`），执行者「重写后已还原」声称核实为真。

### 2. 证据链完整性

- command-log.jsonl 25 条：command_id 唯一、argv/cwd/时间戳/exit_code 齐全、HOME 脱敏、失败与重跑链完整（resolver-client-new FAIL→fix1 PASS；e2e-ops-witness FAIL→fix1 FAIL→fix2 PASS，retry_of 字段正确）。full-suite exit=1 如实记 FAIL 并在报告归因 F1 基线。
- EVIDENCE_SHA256.txt 64 条逐一重算：**全部匹配**（未包含自身，无自引用）。
- 全量 4 红 = seal×1 + round2×2 + round3×1，与 P00 BASELINE_FAILURES F1 登记及 P03 报告（14750 绿/4 红）一致；+9 例后 14759 绿，未扩大。
- 负向注入日志核实：P04-FAULT-provider-mismatch（删联合键→恰 A01 1 例红，其余 16 绿）、P04-FAULT-observer-missing（漏装 observer→e2e chat 3/3 红）。注入均已还原（生产文件不在 diff 中）。

### 3. 独立复跑（P04-ACCEPTANCE-rerun.log）

| 命令 | 结果 |
|---|---|
| vitest resolver+client（2 文件） | 25/25 绿 |
| vitest e2e-chat+e2e-utility | 16/16 绿 |
| vitest 定向 36 文件（与 targeted-25 同 argv） | 410/410 绿 |
| npm run typecheck | exit 0 |
| npm run check:tool-invocation-boundaries | 通过（2244 源文件） |
| npm run lint | 0 error / 10733 警告（与执行者口径一致） |

### 4. UNCHANGED_VERIFIED 抽查（源码级）

- provider+id 联合键：`core/provider-registry.ts:1119-1130` getOperationModel 严格 `id && provider` 双匹配——A01 性质属实。
- fail-closed：`core/model-operation-resolver.ts` compose() 缺凭证即抛 `provider_missing_creds`（422），解析只走配置 ref，结构上不存在「偷退到另一 provider」路径；`isLocalBaseUrl` 仅 localhost/127.0.0.1/0.0.0.0 三主机名，未扩大。
- 生产接线：`core/engine.ts:2497/2530/2536/2735/2746` EmbeddingClient/RerankClient 经 resolver.resolveFresh 注入 model-manager.resolveProviderCredentialsFresh——新增测试的 DI seam 即生产装配面。
- 取消传播：`core/model-operation-client.ts` execute() resolveFresh→combinedSignal(AbortSignal.any)→fetch；无重试循环。
- 矩阵行号抽查：pi-sdk/index.ts:454 withLingxiCredentialBoundary、engine.ts:2497 resolver 注入、server/routes/models.ts:234 health-check 均命中。

### 5. 反例构造（真实栈，隔离临时目录 /tmp/p04x-acceptance，已清理）

以真实 `globalThis.fetch` + 真实 loopback HTTP server（非 mock fetch）驱动生产 EmbeddingClient：

1. **迟到取消（P04-A08 强化）**：resolveOperationFresh 等待 25ms 后 abort 再返回有效凭证 → AbortError、刷新确实完成、**真实服务器零请求**。比执行者的 mock-fetch 版本更强地证实「取消后凭证返回不复活」。
2. **中途断流 + 无复活重试**：server 写头后挂住，120ms 后 abort → AbortError、恰 1 次 attempt、再等 150ms 仍无第二次请求、ledger 记 `aborted`。

两项均通过（P04-ACCEPTANCE-counterexample.log；测试源档 P04-ACCEPTANCE-counterexample.test.ts）。

### 6. BLOCKED 项合规性

- 任务书 T07-2 明文：无凭证或预算授权记 BLOCKED；§10 明文：缺真供应商验证时阶段验收 BLOCKED 不得标完整 PASS。通用约束 §4/§5 同义。
- 环境事实：本验收同样无真实供应商凭证/预算授权（不默认消耗真实账户）。BLOCKED 是任务书规定的诚实状态，非 FAIL 掩盖——所有本地可执行项均已真实执行且绿。
- 执行者的 status_qualification（「授权后仅补该单项即可转 PASS」）与 blocked_items 登记格式符合 92 模板。

## 发现的问题（均非阻塞，不改变判定）

1. **[轻微-文档精度] USAGE_OWNERSHIP.md §1「无其他 usage 写入点」枚举不全**：五观察点表未列 `lib/llm/observed-pi-direct-summary.ts:158-204`（MC-10 日记直连 summary 的 start/finish/recordError）与 `lib/llm/provider-client.ts:344`（connectivity-probe 经 withModelRequestAccounting）两处写入点。两处均在 MODEL_CALLSITE_MATRIX 对应 family 的 usage 字段中有归属记载、且同用 modelCallId 键控台账（无重复计费风险），但「仅上述边界调用」的 grep 声称与事实有出入。最小修复：下一阶段（或授权的文档修订）把两写入点补入 §1 表并修正措辞；无需改代码。
2. **[观察] P04-A02「B 实际请求次数 0」为结构性证明**：A02 测试断言 refresh 仅发生在 provider-a + resolver 无第二跳；无双 provider fixture 下对 B 的真实 HTTP 计数。补偿：model-no-fallback 全族（含 SDK fallback 会话拆毁）+ 本验收反例 1（解析失败/取消时真实服务器零请求）联合覆盖。可接受，建议未来真供应商冒烟时顺带覆盖。
3. **[观察] A03「异账户结果隔离」为结构性证明**：单飞锁按 authKey（backend.withLockAsync）隔离，竞态测试覆盖同账户单飞；无「两账户同时过期」的直接用例。风险低（锁键含 authKey），非本阶段必改。
4. **[观察] 命令日志 source_sha 记 HEAD 提交**：工作区改动未提交，SHA 不含测试 diff 本身；报告已显式声明「END 候选=同（零生产 commit）」且改动面有 git diff 留档，可接受。编排层统一提交后建议按新 SHA 重算证据坐标。

## 未验证边界

- 真实供应商冒烟（无授权，BLOCKED 项本体）；四平台 CI、Windows/Linux 实机、正式打包（F3 继承）。
- 全量套件仅核对执行者日志的 4 红身份与基线登记一致，未在本验收重新完整跑一遍全量（成本权衡：定向 36 文件 + 门禁 + 新增文件已独立复跑；F1 4 红的失败输出已逐条核对且与 P00/P03 登记同组）。
- USAGE_OWNERSHIP §4 引用的 query/export 测试存在性未逐一打开核对（在定向/全量绿范围内）。

## 验收证据

- `artifacts/refactor-2026/P04/logs/P04-ACCEPTANCE-rerun.log`（sha256 `53f072b8…`）
- `artifacts/refactor-2026/P04/logs/P04-ACCEPTANCE-counterexample.log`（sha256 `46f07627…`）
- `artifacts/refactor-2026/P04/logs/P04-ACCEPTANCE-counterexample.test.ts`（sha256 `03e5198f…`）
- 临时目录 /tmp/p04x-acceptance 已删除；未调用真实账户；未改动用户数据；未 commit/push/tag。

## 判定

| 项 | 判定 |
|---|---|
| 改动面诚实性（零生产改动、无断言削弱、无审计证据修改） | PASS |
| 新增 9 例测试真实入口与副作用观察 | PASS |
| 定向/门禁/全量基线声称 | PASS（独立复跑核实） |
| A01–A16 本地场景 | PASS（映射完整、证据可溯） |
| BLOCKED 项（真供应商冒烟）合规性 | 成立（任务书 §10/通用约束 §4-5 允许） |
| **阶段总状态** | **BLOCKED（本地实施与验收 PASS；唯一阻塞 = T07-2 无凭证/预算授权，授权后补该项可转 PASS）** |
