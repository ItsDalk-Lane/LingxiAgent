# R00-T05 独立验收报告 R1｜建立旧行为与故障夹具

审查者：REVIEWER-R00-T05-R1（一次性独立 ZCode 验收代理；非执行者、非其内部调研助手）
审查日期：2026-09-24（本机 macOS 27.0 darwin arm64，Node v24.16.0，vitest 4.1.10）
审查方式：只验收、不修复。未创建分支/worktree、未修改候选/总控账本/执行报告、未提交推送 PR/tag/release、未触及真实用户数据、未进行任何真实外发或付费 API 调用。本报告是本次审查新增的唯一文件。

## 1. tested HEAD 与前置核验

- tested HEAD：`6f58b9351046e8dccfeb83be6976f197acb2de8d`（`git rev-parse HEAD` 实测），分支 `codex/rust-tauri-migration`，与任务指定的 Task base 及 `origin/codex/rust-tauri-migration`（实测同为 `6f58b9351`）一致。
- 依赖 R00-T04：总控账本 `docs/rust-tauri/ORCHESTRATOR_PROGRESS.json` 记录 DONE / review_verdict PASS / task_commit `6f58b9351` / push CONFIRMED / remote_verified_sha 一致——依赖满足（本审查实测远端分支 HEAD 复核确认）。
- 工作区范围：跟踪文件唯一修改是总控账本（预先存在、总控维护）；未跟踪新文件共 89 个 = 88 个 T05 候选 + 执行报告本身。`git ls-files --others --ignored --exclude-standard` 复核确认无任何候选文件被 .gitignore 吞掉（仓库内 .cache/.mimosa 等忽略项均与本任务无关）。

## 2. 候选冻结摘要独立重算

- 候选定义：89 个未跟踪文件中排除执行报告 `docs/rust-tauri/R00/R00-T05_REPORT.md` 与总控账本，得 88 个，按路径排序。
- 独立重算（每行「单文件 SHA256 + 两个空格 + 路径 + 换行」聚合 SHA-256，`/tmp/t05_candidate_manifest_r1.txt` 留档）：
  - 88 文件聚合 = `96c39f787e2f7230a28e6118c027302a1ed20843f003c316279cb841f990d052` ✅ 与总控/报告一致。
  - 执行报告单文件 = `924a8c8a3ae211665729339cf0c9ba2c98daeb12eeba25c583fe0cb694ceb460` ✅ 一致。
- 候选构成：`tests/migration/`（夹具 25 + 测试/替身/守卫/规范化/ORACLE 7）+ `scripts/rust-tauri/r00-t05-replay.mjs` + `artifacts/rust-tauri/R00/T05/`（三 run × 19 文件 + summary + 2 diff，共 60）。全部为小体积文本（最大 32.7KB），无二进制、无真实用户数据；`*.log` 被 .gitignore L95 忽略，证据因此用 `.txt`/`.json` 扩展名，`git check-ignore` 口径成立。

## 3. 任务书符合性与锚点审查（逐项独立取证）

按顺序读取：00_README、01–06 共同约束、R00 阶段书 R00-T05 全文、90/91、stage-index.json、task-catalog.json（R00-T05 Steps/Deliverables/depends_on=[R00-T04]）、acceptance-catalog.json（R00-A09/A10 均 REQUIRED）。

**九类夹具齐全**：多轮会话 / 工具调用 / MOOD / 附件 / fork / 取消 / 认证失败 / 历史分页 / 损坏尾记录，逐一对应 `tests/migration/fixtures/` 九个 fixture_id，每组 FIXTURE_MANIFEST 均登记 source.origin、source.anchors、replay_entry.layers、expected_verifier、observable_assertions、allowed_varying。

**锚点抽查（每类至少 1 个源锚点 + 关键既有测试锚点，全部实测吻合）**：

| 主张 | 实测 |
|---|---|
| `lib/session-jsonl.ts:37-42` 严格层抛 `session_branch_invalid_json`（含行号） | ✅ sed 实测 |
| `lib/session-jsonl.ts:188/334/349-363` readCurrentSessionBranch / readSessionMessages / 宽容层静默丢弃（catch 注释原文） | ✅ |
| `core/message-utils.ts:118-158` repair→SDK 分支→catch 静默降级 raw-read→跳过坏行 | ✅ |
| `server/history-read/page.ts:38-49` 游标语义（endIdx=min(before,total)、hasMore=startIdx>0） | ✅ |
| `server/routes/sessions.ts:1383` GET /sessions/messages（至 ~1512 返回 200） | ✅ |
| `core/tool-invocation-gateway.ts:19-32` 请求形状；`:465-490` 执行前 abort/AbortError/完成后复查均 EXECUTION_CANCELLED、普通异常 TRANSPORT_FAILURE | ✅ |
| `lib/tools/invocation/errors.ts:3-22` 18 个错误码全集 | ✅（计数 18） |
| `core/server-auth.ts` denyAuth shape（error:"forbidden"）与 missing_credential / loopback_token_requires_local_transport / invalid_credential；`server/http/request-principal.ts:53-84` 403 消费 | ✅ |
| `core/events.ts:96-120` MoodParser/ThinkTagParser；`shared/internal-mood-block.ts:6-10` 词表 mood/pulse/reflect；`shared/reserved-tag-stream.ts:421` splitReservedTagSegments | ✅ |
| `lib/session-files/session-file-registry.ts:72-177` registerFile 引用式登记；`:255-360` forkSessionFiles | ✅ |
| `core/session-coordinator.ts:3690` 生产 fork 调用 createBranchedSession；SDK `session-manager.js:782-791`（appendMessage 行 schema）、`:1113-1180`（createBranchedSession 语义） | ✅ |
| 既有测试锚点：`mood-parser.test.ts:20-50`、`server-auth.test.ts:32-58`、`session-fork.test.ts:366-373`、`history-pagination-invalid-fixture.test.ts:2-17,245-296`、`session-jsonl-file.test.ts:88-120`（repaired/projected 计数 + .repair.json 先例）、`history-pagination-run-continuity.test.ts:315-330`、`p02-cancellation-edges.test.ts:37-72`、`session-jsonl.test.ts:24-45`、`tool-invocation-errors.test.ts:11-30` | ✅ 全部实测 |

**预期形成方式**：`expectMatchesExpected` 断言 `canonicalize(actual) === stableStringify(expected)`——expected 是冻结的规范化断言目标，且其断言点与既有公开测试逐项同形（上表），非 JSON 自比。工具矩阵 expected 含 `executorCalls` 数组（次数 + 参数 + ctx 身份覆写），篡改/吊销用例断言零执行——调用次数与参数确实被断言。MOOD expected 诚实冻结了两个 live/raw 模块级分歧（`leading-mood`/`unclosed-block-flush` 的 `parity.visibleEqual=false`），ORACLE §3 标注「新系统须裁决」，未粉饰。

**隐私与临时目录**：全部输入为合成中文占位内容（实测 session.jsonl/scripts/scenario）；时间戳固定 2026-09-10 或被规范化屏蔽；token 显式 `not-a-secret`；所有文件操作走 `os.tmpdir()` mkdtemp + afterAll 清理；夹具路径读取有越界防护（fixturePath/recordActual 双重边界校验）。

## 4. R00-A09 独立验证｜夹具可重复且不依赖付费服务 —— **PASS**

### 4.1 network-guard.ts 审查 + 安全拒网负例（零真实外发）

守卫安装时机（代码审查）：两测试文件均在生产模块动态 import **之前**于 worker 顶层安装（`LINGXI_MIGRATION_BLOCK_NETWORK !== "0"` 默认开启），覆盖执行测试的 vitest worker 进程。

安全负例（`/tmp/r1-guard-negative.mjs` 留档；以 `Socket.prototype.connect` 记录器在 DNS/连接系统调用之前拦截，候选守卫的 refuse 也在 original 之前抛出，全程零真实外发，exit 0）：

- **被覆盖（PASS）**：`net.connect(对象形)`、`tls.connect(对象/字符串形)`、`dns.lookup`、以及**内置 fetch 的 http 与 https**（实证：undici 在连接时动态属性访问 `net.connect`/`tls.connect`，取到的是被替换后的包装——fetch 的 cause 即守卫报错）。
- **实证绕过（CONFIRMED-BYPASS，见 F01）**：`net.createConnection`/`tls.createConnection`（与 connect 同一底层函数的另一导出，身份实测未被替换）、`http.request`（栈迹实证经 http.Agent→net.createConnection 越过守卫直达 Socket 层）、`net.connect(443, "example.com")` 端口前置字符串形（isPipe 启发式把「无冒号字符串」误判为本地管道而放行）。

### 4.2 三次独立隔离回放

命令：`NODE_OPTIONS="--import /tmp/r1-strong-guard.mjs" R1_NETLOG=/tmp/r1-net-violations.log node scripts/rust-tauri/r00-t05-replay.mjs --out /tmp/r1-t05-replay-isolated`

驱动器 `--out` 正常工作；**仓库内冻结的 `artifacts/rust-tauri/R00/T05/` 未被重写**（回放后 88 文件聚合重算仍为 `96c39f78…`）。结果（`/tmp/r1-t05-replay-isolated/` 留档）：

- run-1/2/3 各 exit 0、各 18 份证据文件；
- `normalized-diff-run1-run2/3` 均 identical（9 fixtures，逐字节一致）；
- **零外联独立证明**：我用比候选守卫更严的自制强守卫（覆盖 net/tls 的 connect+createConnection、`Socket.prototype.connect` 收口层、http(s).request、fetch、dns lookup/promises/resolve*，preload 注入驱动器及其全部子进程）全程拦截，违规日志为空——即三次回放路径**零外部连接尝试**，且不需要任何付费服务（全部进程内真实模块 + 临时目录）。
- 我的三次 normalized 输出与仓库冻结的三 run normalized 输出**逐字节一致**（27 组 cmp 全等）——冻结证据可独立复现；差异仅限声明允许字段（tmp 路径/UUID/sf_id/ISO 时间戳/timestamp 类键/pid），auth-server-deny 等 6 组 raw 三次亦逐字节一致，与声明吻合。

**A09 结论：PASS**（三 run 退出码 0、规范化逐字节一致、允许变动字段外零差异、离线与零付费由更强守卫独立证实；F01 不改变本结论，理由见影响评估）。

## 5. R00-A10 独立验证｜旧缺陷不变成标准 —— **PASS**

- **反例实际可运行**：`npx vitest run tests/migration/r00-a10-old-defect.test.ts`（并入 4.2/§6 的 14/14）exit 0，5/5 通过；本审查独立重跑确认。
- **旧实现表现与新不变量确实冲突**（机器验证 + 本审查源码复核）：同一 `corrupted-tail/session.jsonl`（第 6 行截断 JSON，实测）——严格层抛 `SessionBranchError{code:"session_branch_invalid_json", details.line:6}`（`lib/session-jsonl.ts:37-42` 实测）；旧宽容层静默丢弃返回 3 条零标注；旧路由 200 + 3 条 display 零标注；旧生产入口 4 条（含 toolResult）零标注。新标准 `expected.json` 的 `acceptable_outcomes` 仅接受 结构化错误 或 带 `droppedCorruptLines:1` 标注的显式降级，`forbidden_outcomes` 含 `silent_success`——旧实现命中 forbidden，新标准明确、可被 Rust 侧按同一夹具机器检验（形状与计数均为具体值）。
- **「旧设计被真实记录」与「语义缺陷不得复制」的区分正确**：既有 `tests/history-pagination-invalid-fixture.test.ts:2-17` 把兼容层静默降级描述为「设计如此」（实测原文），候选没有沿用该口径当新标准，而是将其登记为 `record_kind=OLD_SYSTEM_DEVIATION_NOT_STANDARD` 偏差；A09 回放对 corrupted-tail 的确定性对照目标是 `old-deviation.json` 的 `old_actual`（r00-t05-replay.test.ts:293-296），新标准 expected 只在 A10 侧断言——**old_actual 与新 expected 分离确认**。
- **磁盘回执与用户可见响应的区别未被隐去**：A10 测试 3 显式断言 `.repair.json` 回执存在（部分缓解）且响应级零标注；old-deviation 的 `partial_mitigations` 同时登记「回执存在」与「回执不进响应、宽容层连回执都没有」——分层事实完整。
- **规则来源追溯成立**：AGENTS.md 红线原文「禁止静默降级（错误要么抛要么显式降级并标注）」实测逐字一致；显式标注可达性先例 `tests/session-jsonl-file.test.ts:88-120`（repaired/projected 计数 + .repair.json）实测吻合；严格层先例锚点吻合。

**A10 结论：PASS**。

## 6. 独立重跑命令与退出码（全部本机实测）

| 命令 | 退出码 | 结果 |
|---|---|---|
| `npx vitest run tests/migration/r00-t05-replay.test.ts tests/migration/r00-a10-old-defect.test.ts` | 0 | 2 文件 14/14（9 回放 + 5 反例） |
| `npm run typecheck` | 0 | root + tsconfig.node + tsconfig.test 三配置全过 |
| `npx vitest run tests/session-jsonl.test.ts tests/tool-invocation-gateway.test.ts tests/history-pagination-invalid-fixture.test.ts` | 0 | 3 文件 29/29（邻近既有测试零干扰） |
| `npx vitest run tests/live-history-reserved-tag-parity.test.ts`（本审查追加：MOOD 路由级实时/历史等价契约） | 0 | 2/2 |
| §4.2 三次隔离回放（强守卫 + `--out /tmp`） | 0 | 三 run exit 0、规范化一致、零外联 |

缩减说明：候选不含生产代码改动（`git diff` 仅总控账本），影响面=新增测试/夹具本身 + vitest 自动收集路径 + typecheck 三配置，上述集合已按影响图覆盖；未跑全量 `npm test`（新增文件已单独全跑且 typecheck 含 tests 配置）。

## 7. Findings（严重度 / 锚点 / 复现 / 影响）

**R1-F01｜MEDIUM（不阻断，需后续修复）network-guard 覆盖面与声明不符，存在三类实证绕过**
- 锚点：`tests/migration/network-guard.ts:26-56`；受影响声明：`tests/migration/fixtures/FIXTURE_MANIFEST.json` `replay.offline` 与执行报告 §4「worker 内阻断 net/tls/dns 外连」。
- 复现：`node /tmp/r1-guard-negative.mjs`（零真实外发）。① `net.createConnection`/`tls.createConnection` 是与 connect 同一底层函数的另一导出（补丁前 `net.connect === net.createConnection` 实测为 true），守卫只替换 `connect` 属性，另一导出保留原函数；② `http.request`/`https.request` 经 http.Agent→`net.createConnection` 越过守卫（栈迹实证）；③ `net.connect(443, "example.com")` 端口前置字符串形被 isPipe 启发式（无冒号即当管道）放行；④ `dns.promises.lookup`、`dns.resolve*` 未覆盖。内置 fetch 反而被覆盖（undici 动态属性访问）。
- 影响：防御纵深弱于声明（「意外外连立即失败」对最常见意外路径 http.request 不成立）。**不阻断 A09** 的理由：A09 的验收机制是回放确定性 + 离线 + 零付费，本审查已用更严强守卫独立证明三次回放零外部连接尝试；执行报告 §7 本就如实声明守卫「不构成无其他外联渠道的证明」。修复要求（建议 repair 轮或至迟 T08 封存前）：补丁 `net.createConnection`/`tls.createConnection`、修正 isPipe 判定（仅显式 path 或含 `/` 的字符串视为管道）、覆盖 `dns.promises.lookup`；或将两处声明改精确（列出实际覆盖的调用形）。

**R1-F02｜LOW FIXTURE_MANIFEST auth 条目散文与机器面不一致**
- 锚点：`FIXTURE_MANIFEST.json` auth `observable_assertions`（4 条）vs `fixtures/auth/cases.json`/`expected.json`（5 用例，含 `connection-not-allowed`/custom_remote）。
- 影响：文档性遗漏，机器断言面完整（expected 覆盖全部 5 例）；建议 repair 轮补一行散文。

**R1-F03｜LOW 模型替身「可控断点」能力已实现但本轮回放未演练**
- 锚点：`tests/migration/stubs.ts:19-46`（hangAfterFrame + abortSignal）vs `r00-t05-replay.test.ts:343`（feed 未传 opts）。
- 影响：任务书 Step 3 的能力要求以代码存在方式满足，但「流式中途挂起直到取消」无任何夹具实际走通；建议 R03 取消矩阵补一个使用 hang 脚本的用例（登记为后续输入，不要求本任务返工）。

**R1-F04｜INFO normalize 的 tmp-path 模式理论上可过度屏蔽**
- 锚点：`tests/migration/normalize.ts:31`（`escaped + [^\"\\\\]*` 会从 tmp 前缀吞至字符串末尾）。
- 实测影响为零：冻结 raw 证据中三处含 tmp 前缀的值（corrupted-tail/fork/attachments）均为纯路径、值边界即路径边界（实测 `/var/folders…jsonl` 后紧跟 `",`）；数组顺序不被屏蔽（stableStringify 仅排序对象键）。属未来加固建议（例如按路径字符集收敛匹配），不构成缺陷。

## 8. Step / Deliverables 结论

- **Step 1**（复用公开行为测试提取契约）：完成——expected 断言与既有公开测试同形且锚点全部实测吻合，无私有字段 mock 充当权威。
- **Step 2**（九类脱敏夹具 + 来源/许可/隐私/预期验证者）：完成——九类齐全、逐项登记、合成内容实测、隐私审查通过；`tests/migration/` 选址符合 02 契约。
- **Step 3**（模型固定流 + 可控断点、工具替身记录次数/参数、临时目录）：完成（断点能力未演练见 F03）；替身仅在外部模型/工具边界，被测对象全部真实旧实现（Hono 路由进程内调用、真实 SDK、真实 SQLite manifest、真实网关/ConfirmStore/权限 wrapper、真实 sidecar 落盘）。
- **Step 4**（旧缺陷修正后不变量）：完成（§5）。
- **Deliverables**：FIXTURE_MANIFEST.json ✅、共享输入/预期夹具（9 组）✅、OLD_BEHAVIOR_ORACLE.md ✅；放置与命名符合 R00 约定；88 文件全部可纳入 Git。
- 执行报告 §6 命令结果全部复现；§7 未验证范围（会话级 WS 取消未建路由级夹具、MOOD 路由级等价依赖既有测试、无真实模型/平台/跨平台）如实登记且与本审查观察一致。

## 9. 未验证范围（本审查未覆盖、如实区分）

- 未运行全量 `npm test`、lint、构建与其他平台（候选无生产代码改动；影响面集合见 §6 缩减说明）。
- 未对 9 组夹具的全部锚点逐行复核（每类至少 1 个源锚点 + 关键测试锚点抽查，共约 25 处全部吻合）。
- 未验证真实模型/真实供应商/真实平台账号/WS 会话级取消路由级夹具/MCP 认证面——执行报告已如实列为未验证，属后续阶段。
- 审查证据留档于 `/tmp`（r1-t05-replay-isolated/、r1-guard-negative.mjs、r1-strong-guard.mjs、t05_candidate_manifest_r1.txt、r1-*.out），不进入仓库。

## 10. 最终结论

R00-A09：**PASS**。R00-A10：**PASS**。Steps/Deliverables 全部完成。发现 1 项 MEDIUM（F01，守卫覆盖声明夸大，非阻断、有明确修复要求）+ 2 LOW + 1 INFO，均不构成本任务验收的阻断或重大问题。

**VERDICT: PASS**
