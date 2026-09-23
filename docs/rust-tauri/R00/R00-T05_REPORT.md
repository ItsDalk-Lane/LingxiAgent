# R00-T05 报告｜建立旧行为与故障夹具

任务：R00-T05（建立旧行为与故障夹具）｜验收：R00-A09、R00-A10（均 REQUIRED）
执行者结论：**READY_FOR_REVIEW**（实施与自测完成；独立验收由总控另行指派，本报告不自称 PASS）

## 1. 范围与源码基线

- 只执行 R00-T05。未创建分支/worktree，未 commit/push/PR/tag/release。
- 基线 HEAD = `6f58b9351046e8dccfeb83be6976f197acb2de8d`（分支 `codex/rust-tauri-migration`），与任务指定 Task base 一致；本任务未产生提交，HEAD 不变。
- 依赖核验：总控账本 R00-T04 = DONE / review_verdict PASS / task_commit 已推送（`6f58b9351`，本地与 `origin/codex/rust-tauri-migration` 一致）。
- 工作区：`docs/rust-tauri/ORCHESTRATOR_PROGRESS.json` 的预先存在未提交修改原样保留（总控维护，本执行者未触碰）；其余新增文件见 §8 候选清单。无生产代码修改。
- 环境：macOS 27.0（darwin arm64）、Node v24.16.0、vitest 4.1.10、Python 3.14.3（仅用于本地校验计算）。

## 2. 交付物

| 交付 | 位置 |
|---|---|
| FIXTURE_MANIFEST.json | `tests/migration/fixtures/FIXTURE_MANIFEST.json`（9 组夹具：来源锚点、许可/隐私审查、回放入口、预期验证者、可观察断言、允许变动字段） |
| 共享输入/预期夹具 | `tests/migration/fixtures/`（多轮会话、工具调用、MOOD、附件、fork、取消、认证失败、历史分页、损坏尾记录；JSON/JSONL 跨语言可读） |
| OLD_BEHAVIOR_ORACLE.md | `tests/migration/OLD_BEHAVIOR_ORACLE.md`（旧系统外部观察标准：输入/协议帧/输出/文件结果/错误码 + 已登记缺陷 §10） |
| 模型/工具替身 | `tests/migration/stubs.ts`（固定流片段 + 可控断点；记录调用次数与参数的执行器替身） |
| 回放测试 | `tests/migration/r00-t05-replay.test.ts`（9 夹具 × 真实旧入口）+ `tests/migration/r00-a10-old-defect.test.ts`（A10 反例） |
| 规范化/断网守卫 | `tests/migration/normalize.ts`（允许变动字段屏蔽 + 稳定序列化）、`tests/migration/network-guard.ts`（worker 内阻断 net/tls/dns 外连） |
| A09 证据 | `artifacts/rust-tauri/R00/T05/`（run-1/2/3 各 18 份 raw+normalized + stdout.txt；REPLAY_SUMMARY.json；两份规范化 diff） |
| 本报告 | `docs/rust-tauri/R00/R00-T05_REPORT.md` |

放置说明：任务书 02 契约指定 `tests/migration/` 为新旧共享夹具及对照测试的家，故夹具与 ORACLE 均在该目录（Rust 侧消费者与夹具同处可见）；报告按 R00 约定放 `docs/rust-tauri/R00/`。ORACLE 未放 docs 目录的理由：它是跨语言共享标准的组成部分，与 FIXTURE_MANIFEST 配套使用。

## 3. 四步完成情况

**Step 1（从公开行为测试提取契约）**：每个夹具的 expected 都有既有公开测试锚点（清单见 FIXTURE_MANIFEST `source.anchors` 与 ORACLE 各节），关键断言与既有测试逐项同形，例如：MOOD 事件序取自 `tests/mood-parser.test.ts:20-50`；工具错误码集取自 `tests/tool-invocation-errors.test.ts:11-30`（18 个公开错误码逐字一致）；分页游标语取自 `server/history-read/page.ts:38-49` 与 run-continuity 不变量；fork 边界语义取自 `tests/session-fork.test.ts:366-373`；认证 deny 结构取自 `tests/server-auth.test.ts:32-58,204-225`。expected 冻结自首次验证运行后，再逐项回对上述锚点（非 JSON 自比）。

**Step 2（脱敏夹具）**：9 组夹具全部合成内容（隐私审查结论见 FIXTURE_MANIFEST `privacy_review`）：无真实人名/凭证/附件/用户目录内容；时间戳为固定过去值或被规范化屏蔽；认证 token 为显式 `not-a-secret` 合成值。每组记录来源、许可/隐私审查、预期验证者（本轮 + 后续 Rust 阶段的对应任务）与可观察断言。

**Step 3（替身与真实入口重放）**：模型替身（固定流片段、hangAfterFrame 可控断点）与工具替身（记录调用次数/参数/ctx 身份）只替代外部协议边界（符合 05 协议第 1 层替身边界）；被测对象全部为真实旧实现：真实 Hono 路由（`app.request` 进程内，无 socket）、真实 `SessionManifestStore`（SQLite）、真实 SDK `createBranchedSession`、真实 `MoodParser`/`splitReservedTagSegments`、真实 `ToolInvocationGateway`+`ConfirmStore`+权限 wrapper、真实 `SessionFileRegistry`（sidecar 落盘）。文件/进程只使用 `os.tmpdir()` 临时目录（afterAll 清理）。

**Step 4（A10 旧缺陷）**：见 §5。证据不足的情况未发生——缺陷有源码、测试、规则三重既有证据。

## 4. R00-A09 自测证据（夹具可重复且不依赖付费服务）

- 前置：`LINGXI_MIGRATION_BLOCK_NETWORK=1`（测试 worker 内 patch net/tls/dns，非本地主机一律抛错；回放本体零网络调用，守卫用于拦截未来意外外连）。
- 命令：`node scripts/rust-tauri/r00-t05-replay.mjs` → 驱动器退出码 **0**。
- 三次运行各自退出码 0（`run-1/2/3`，见 `REPLAY_SUMMARY.json`）；每 run 产出 9 夹具 × raw/normalized 两份 + stdout.txt。
- 规范化 diff：`normalized-diff-run1-run2.txt`、`normalized-diff-run1-run3.txt` 均为「identical（9 fixtures）」——规范化输出逐字节一致。
- 允许变动字段（FIXTURE_MANIFEST `global_normalization`）：临时目录路径、SDK 生成 UUID、`sf_` 文件 id、ISO 时间戳、SDK 文件名时间戳、timestamp/createdAt/mtime/pid 键。raw 层跨 run 哈希确有差异（corrupted-tail/fork/attachments 含临时路径与生成 id），auth-server-deny 的 raw 三次完全一致（无变动类）——差异恰好只落在声明类别内。
- 无付费服务：全部为进程内真实模块 + 临时目录；无真实模型、真实平台、外发。

## 5. R00-A10 自测证据（旧缺陷不变成标准）

**缺陷：会话 JSONL 损坏尾记录的静默降级**（`fixtures/sessions/corrupted-tail/`）。

- 可运行反例：`npx vitest run tests/migration/r00-a10-old-defect.test.ts` → 退出码 **0**，5/5 通过：
  1. 严格层对同一输入抛 `SessionBranchError{code:"session_branch_invalid_json", details.line:6}`——响亮失败在旧代码库已存在（不是对新系统的额外发明）；
  2. 旧宽容层 `readSessionMessages` 静默丢弃坏行（3 条消息、零标注）；
  3. 旧生产入口响应零标注（磁盘 `.repair.json` 回执存在但不进响应——分层事实如实登记，独立新鲜副本归因）；
  4. 新期望 `expected.json` 的 acceptable_outcomes（结构化错误 / 带 `droppedCorruptLines:1` 标注的显式降级）不接受静默成功形态，旧实现命中 forbidden `silent_success`；
  5. 偏差登记与规则来源完整（`old-deviation.json`：4 个偏差位点 + 3 条测试证据 + 部分缓解清单）。
- 规则来源：AGENTS.md 红线「禁止静默降级（错误要么抛要么显式降级并标注）」；仓库既有先例 `repairOversizedSessionEntriesInFile` 返回 `repaired/projected` 计数并落盘回执（`tests/session-jsonl-file.test.ts:88-120`）证明显式标注是既定可达做法。
- 新期望未为迁就旧 bug 改写：`expected.json` 的不变量独立于旧实现存在，`old-deviation.json` 明确标注 `record_kind=OLD_SYSTEM_DEVIATION_NOT_STANDARD`；A09 回放中 corrupted-tail 的确定性对照目标是 old_actual（偏差快照）而非新标准——新标准留给 Rust 实现回放。
- 既有测试证据：`tests/history-pagination-invalid-fixture.test.ts:2-17`（文件头注释原文钉死「静默降级…路由仍返回 200」）、`:245-296`。

## 6. 实际命令与结果

| 命令 | 退出码 | 结果 |
|---|---|---|
| `node scripts/rust-tauri/r00-t05-replay.mjs` | 0 | 三 run 各 exit 0、18 文件；两份规范化 diff 均 identical；`PASS-CANDIDATE` |
| `npx vitest run tests/migration/r00-t05-replay.test.ts tests/migration/r00-a10-old-defect.test.ts` | 0 | 2 文件 14/14（9 回放 + 5 反例） |
| `npm run typecheck` | 0 | root + tsconfig.node + tsconfig.test 三配置全过 |
| `npx vitest run tests/session-jsonl.test.ts tests/tool-invocation-gateway.test.ts tests/history-pagination-invalid-fixture.test.ts` | 0 | 邻近既有测试 3 文件 29/29（确认新增文件零干扰） |
| `git check-ignore` 抽查 `artifacts/rust-tauri/R00/T05/{REPLAY_SUMMARY.json,run-1/stdout.txt,normalized-diff-run1-run2.txt}` | 1（未被忽略） | 证据扩展名 .txt/.json 可入库；`.log` 被忽略故未使用 |

## 7. 未验证范围

- 回放覆盖 9 个必需类别的基础闭环；未覆盖任务书 05 协议 §4 要求的完整验收矩阵（多 provider 协议族、MCP/插件路由、跨平台、安装包）——那些属于后续阶段场景，本任务只交付共享夹具与 oracle。
- 会话级取消（WS abort_result/turn_end/中断标记）只登记了锚点（ORACLE §5），未建路由级夹具（chat 路由流式链需要更重的驱动器，属 R03 范围）；工具网关级三时点取消与审批中止已覆盖。
- MOOD 的路由级实时/历史等价依赖既有 `tests/live-history-reserved-tag-parity.test.ts`（未重复建设）；模块级两入口的两个真实分歧（闭标签后换行、未闭合开标签）已如实冻结进 expected 并在 ORACLE §3 标注「新系统须裁决」。
- 未运行真实模型、真实平台账号、真实外部连接器；未验证其他平台。
- 断网守卫覆盖 net/tls/dns（worker 进程内）；不构成对「被测代码无其他外联渠道」的证明（回放路径本身零网络调用）。

## 8. 候选文件与摘要

- 候选 = 本任务新增的全部 88 个文件（`git status` 未跟踪新文件，路径排序）；**不含本报告自身**（避免自引用；报告哈希可事后计算）与总控账本 `docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`（预先存在修改，非本任务产物）。
- raw 证据含本机临时路径/生成 id（允许变动字段的原始形态）；normalized 证据为比对基准。
- HEAD：`6f58b9351046e8dccfeb83be6976f197acb2de8d`（未提交，候选以工作区文件形式交付）。

**逐文件 SHA-256（双空格分隔，路径排序）：**

```text
d6577d175abdbfe4d97fe13c68a5ebb3fce5544e2ad9cdba1b788d252b7d6ad3  artifacts/rust-tauri/R00/T05/REPLAY_SUMMARY.json
dfceb20d8aba17413bb0063b4bd0e8dc2796beefc2e326e7067c7d8059930c1c  artifacts/rust-tauri/R00/T05/normalized-diff-run1-run2.txt
e41bd1c5bf3f1c169521637df3ac7c350fff9d22bc0efc71f114e305e0d94aa4  artifacts/rust-tauri/R00/T05/normalized-diff-run1-run3.txt
9b793065d65fd1eb1b816248b47b4463874580d93737da42eb10a7e7de9485bf  artifacts/rust-tauri/R00/T05/run-1/attachments-lifecycle.normalized.json
55f742b1a910df51b4b53c84f8cb771058ff80a727dd8543f673c0851329b0a9  artifacts/rust-tauri/R00/T05/run-1/attachments-lifecycle.raw.json
0ebf0e3349e9f3f95a4c13a8f1190d147caf2002fdf6539f94650e3b969155cb  artifacts/rust-tauri/R00/T05/run-1/auth-server-deny.normalized.json
0ebf0e3349e9f3f95a4c13a8f1190d147caf2002fdf6539f94650e3b969155cb  artifacts/rust-tauri/R00/T05/run-1/auth-server-deny.raw.json
9c9a7ac92a4482efc5f3ae004fc73cb869ac394fa810e193e61fdfc03155c1e8  artifacts/rust-tauri/R00/T05/run-1/cancel-edges.normalized.json
9c9a7ac92a4482efc5f3ae004fc73cb869ac394fa810e193e61fdfc03155c1e8  artifacts/rust-tauri/R00/T05/run-1/cancel-edges.raw.json
faf8ff9211a5058695a06664c950d020216d2e856ef767d30a7e6b812ddd0b35  artifacts/rust-tauri/R00/T05/run-1/corrupted-tail.normalized.json
074152f44a46eb55e4a2e0b0cc6b33b078feb732727cf085467e794fed77067d  artifacts/rust-tauri/R00/T05/run-1/corrupted-tail.raw.json
9a2f57c6b67858bab0c1795b592e2db0b745ff600565f69537e892ddea973344  artifacts/rust-tauri/R00/T05/run-1/fork-lineage.normalized.json
5582f56186c163fddaf37450c7ad793090847f8f01be15c22235b6b99c79ae33  artifacts/rust-tauri/R00/T05/run-1/fork-lineage.raw.json
1bb3297118a1e0e588cd02c4bc54f8e98c64992d9be74b468a7974d435bf27bd  artifacts/rust-tauri/R00/T05/run-1/mood-stream-scripts.normalized.json
1bb3297118a1e0e588cd02c4bc54f8e98c64992d9be74b468a7974d435bf27bd  artifacts/rust-tauri/R00/T05/run-1/mood-stream-scripts.raw.json
62764808edab44aa0a8f03bb4cab473da3cec6b59f5ea93a902540609d25e240  artifacts/rust-tauri/R00/T05/run-1/multi-turn-basic.normalized.json
7441488641546a01041f024d8504d66d1c9f947d4515629a06bb98e98d9aa095  artifacts/rust-tauri/R00/T05/run-1/multi-turn-basic.raw.json
ca1e8281faa3c6d5bd316c326345fc2291f30b777b7eb70d5599aace901eced6  artifacts/rust-tauri/R00/T05/run-1/pagination-corpus.normalized.json
ca1e8281faa3c6d5bd316c326345fc2291f30b777b7eb70d5599aace901eced6  artifacts/rust-tauri/R00/T05/run-1/pagination-corpus.raw.json
dd9d90a78b539d443d26391c17edb853e7d7dc6eb77c738167f9d0581f684f38  artifacts/rust-tauri/R00/T05/run-1/stdout.txt
4dffb62d437c62f1256c6e3ee515a024328ef01d12c24466ae2ed4a1dbbd5ca3  artifacts/rust-tauri/R00/T05/run-1/tool-invocation-matrix.normalized.json
4dffb62d437c62f1256c6e3ee515a024328ef01d12c24466ae2ed4a1dbbd5ca3  artifacts/rust-tauri/R00/T05/run-1/tool-invocation-matrix.raw.json
9b793065d65fd1eb1b816248b47b4463874580d93737da42eb10a7e7de9485bf  artifacts/rust-tauri/R00/T05/run-2/attachments-lifecycle.normalized.json
11848099d99ca25906edf85f314ca7595c9be2dc630df4df72ecf120b525d8dc  artifacts/rust-tauri/R00/T05/run-2/attachments-lifecycle.raw.json
0ebf0e3349e9f3f95a4c13a8f1190d147caf2002fdf6539f94650e3b969155cb  artifacts/rust-tauri/R00/T05/run-2/auth-server-deny.normalized.json
0ebf0e3349e9f3f95a4c13a8f1190d147caf2002fdf6539f94650e3b969155cb  artifacts/rust-tauri/R00/T05/run-2/auth-server-deny.raw.json
9c9a7ac92a4482efc5f3ae004fc73cb869ac394fa810e193e61fdfc03155c1e8  artifacts/rust-tauri/R00/T05/run-2/cancel-edges.normalized.json
9c9a7ac92a4482efc5f3ae004fc73cb869ac394fa810e193e61fdfc03155c1e8  artifacts/rust-tauri/R00/T05/run-2/cancel-edges.raw.json
faf8ff9211a5058695a06664c950d020216d2e856ef767d30a7e6b812ddd0b35  artifacts/rust-tauri/R00/T05/run-2/corrupted-tail.normalized.json
fcab3aceb45178813e8c5909bb97bf0fe2296fdbfdea04b0836abe57048c311e  artifacts/rust-tauri/R00/T05/run-2/corrupted-tail.raw.json
9a2f57c6b67858bab0c1795b592e2db0b745ff600565f69537e892ddea973344  artifacts/rust-tauri/R00/T05/run-2/fork-lineage.normalized.json
164b7fda1d536beb1f399ad9d967d00af405d2ebcd594f3e7989887b47b4f6de  artifacts/rust-tauri/R00/T05/run-2/fork-lineage.raw.json
1bb3297118a1e0e588cd02c4bc54f8e98c64992d9be74b468a7974d435bf27bd  artifacts/rust-tauri/R00/T05/run-2/mood-stream-scripts.normalized.json
1bb3297118a1e0e588cd02c4bc54f8e98c64992d9be74b468a7974d435bf27bd  artifacts/rust-tauri/R00/T05/run-2/mood-stream-scripts.raw.json
62764808edab44aa0a8f03bb4cab473da3cec6b59f5ea93a902540609d25e240  artifacts/rust-tauri/R00/T05/run-2/multi-turn-basic.normalized.json
7441488641546a01041f024d8504d66d1c9f947d4515629a06bb98e98d9aa095  artifacts/rust-tauri/R00/T05/run-2/multi-turn-basic.raw.json
ca1e8281faa3c6d5bd316c326345fc2291f30b777b7eb70d5599aace901eced6  artifacts/rust-tauri/R00/T05/run-2/pagination-corpus.normalized.json
ca1e8281faa3c6d5bd316c326345fc2291f30b777b7eb70d5599aace901eced6  artifacts/rust-tauri/R00/T05/run-2/pagination-corpus.raw.json
4fa6d77286a069d08e3877c84346cfbf49442b3a988baa2fdb1f47198091cf11  artifacts/rust-tauri/R00/T05/run-2/stdout.txt
4dffb62d437c62f1256c6e3ee515a024328ef01d12c24466ae2ed4a1dbbd5ca3  artifacts/rust-tauri/R00/T05/run-2/tool-invocation-matrix.normalized.json
4dffb62d437c62f1256c6e3ee515a024328ef01d12c24466ae2ed4a1dbbd5ca3  artifacts/rust-tauri/R00/T05/run-2/tool-invocation-matrix.raw.json
9b793065d65fd1eb1b816248b47b4463874580d93737da42eb10a7e7de9485bf  artifacts/rust-tauri/R00/T05/run-3/attachments-lifecycle.normalized.json
caa5a8b783e6a56f3e4e8e25f4bbdbb1434123db5ebc8daf751d023c0c51a635  artifacts/rust-tauri/R00/T05/run-3/attachments-lifecycle.raw.json
0ebf0e3349e9f3f95a4c13a8f1190d147caf2002fdf6539f94650e3b969155cb  artifacts/rust-tauri/R00/T05/run-3/auth-server-deny.normalized.json
0ebf0e3349e9f3f95a4c13a8f1190d147caf2002fdf6539f94650e3b969155cb  artifacts/rust-tauri/R00/T05/run-3/auth-server-deny.raw.json
9c9a7ac92a4482efc5f3ae004fc73cb869ac394fa810e193e61fdfc03155c1e8  artifacts/rust-tauri/R00/T05/run-3/cancel-edges.normalized.json
9c9a7ac92a4482efc5f3ae004fc73cb869ac394fa810e193e61fdfc03155c1e8  artifacts/rust-tauri/R00/T05/run-3/cancel-edges.raw.json
faf8ff9211a5058695a06664c950d020216d2e856ef767d30a7e6b812ddd0b35  artifacts/rust-tauri/R00/T05/run-3/corrupted-tail.normalized.json
337002499fbbcab6594a31a98f17a351497b5a1823562cf9a16605aec17e0225  artifacts/rust-tauri/R00/T05/run-3/corrupted-tail.raw.json
9a2f57c6b67858bab0c1795b592e2db0b745ff600565f69537e892ddea973344  artifacts/rust-tauri/R00/T05/run-3/fork-lineage.normalized.json
6add6919e64b31b5b8efb619309ac9441194182c5bea87d0637f2da932a73443  artifacts/rust-tauri/R00/T05/run-3/fork-lineage.raw.json
1bb3297118a1e0e588cd02c4bc54f8e98c64992d9be74b468a7974d435bf27bd  artifacts/rust-tauri/R00/T05/run-3/mood-stream-scripts.normalized.json
1bb3297118a1e0e588cd02c4bc54f8e98c64992d9be74b468a7974d435bf27bd  artifacts/rust-tauri/R00/T05/run-3/mood-stream-scripts.raw.json
62764808edab44aa0a8f03bb4cab473da3cec6b59f5ea93a902540609d25e240  artifacts/rust-tauri/R00/T05/run-3/multi-turn-basic.normalized.json
7441488641546a01041f024d8504d66d1c9f947d4515629a06bb98e98d9aa095  artifacts/rust-tauri/R00/T05/run-3/multi-turn-basic.raw.json
ca1e8281faa3c6d5bd316c326345fc2291f30b777b7eb70d5599aace901eced6  artifacts/rust-tauri/R00/T05/run-3/pagination-corpus.normalized.json
ca1e8281faa3c6d5bd316c326345fc2291f30b777b7eb70d5599aace901eced6  artifacts/rust-tauri/R00/T05/run-3/pagination-corpus.raw.json
e544b3f33a0ef9fe5f92ac12e64736134df3bcfe7f0cb9b99dc0e77a049978d3  artifacts/rust-tauri/R00/T05/run-3/stdout.txt
4dffb62d437c62f1256c6e3ee515a024328ef01d12c24466ae2ed4a1dbbd5ca3  artifacts/rust-tauri/R00/T05/run-3/tool-invocation-matrix.normalized.json
4dffb62d437c62f1256c6e3ee515a024328ef01d12c24466ae2ed4a1dbbd5ca3  artifacts/rust-tauri/R00/T05/run-3/tool-invocation-matrix.raw.json
283ff9fc7aee25ae6ad2a04986d64cd0b1178823ac974f311cc98c02a9f1fde1  scripts/rust-tauri/r00-t05-replay.mjs
ed4c8541ff75a0ef5e7396b75a59e497417b3c7d3e06faf9573df83ec97cb396  tests/migration/OLD_BEHAVIOR_ORACLE.md
e11413ed00d54bda894a6db54bb883f80f334b630580d07f51798fd000d6cf55  tests/migration/fixtures/FIXTURE_MANIFEST.json
9b793065d65fd1eb1b816248b47b4463874580d93737da42eb10a7e7de9485bf  tests/migration/fixtures/attachments/expected.json
0582155f41d185838ca104cfed3542ba3ac219c3d64cc41c1e1610f53d2b9c9d  tests/migration/fixtures/attachments/scenario.json
1f8c334e771e110de5516014d6de3726823b27601176d75ea99dd19d91809dc6  tests/migration/fixtures/auth/cases.json
0ebf0e3349e9f3f95a4c13a8f1190d147caf2002fdf6539f94650e3b969155cb  tests/migration/fixtures/auth/expected.json
3c17feccdb7599fca1270d897624b84bf54da006347f85b099675b4a73e71d82  tests/migration/fixtures/cancel/cases.json
9c9a7ac92a4482efc5f3ae004fc73cb869ac394fa810e193e61fdfc03155c1e8  tests/migration/fixtures/cancel/expected.json
1bb3297118a1e0e588cd02c4bc54f8e98c64992d9be74b468a7974d435bf27bd  tests/migration/fixtures/mood/stream-scripts/expected.json
5db1f0a7d9b97642e92912711504ce68e339375e0f4840716320eb667b27754e  tests/migration/fixtures/mood/stream-scripts/scripts.json
64ada84b0694d10036975e9c61a07231a1ae63465c9bdb972c4a78e22f22273c  tests/migration/fixtures/sessions/corrupted-tail/expected.json
57d9a02c181e9e365dc7c642dff3ee05f8fd5d1049ee8a60d9f2da9bcea8ac48  tests/migration/fixtures/sessions/corrupted-tail/old-deviation.json
161aba03015cbbabd584c71b67b1e1ad115ec272f433e82db45261e60d1f8e01  tests/migration/fixtures/sessions/corrupted-tail/session.jsonl
9a2f57c6b67858bab0c1795b592e2db0b745ff600565f69537e892ddea973344  tests/migration/fixtures/sessions/fork-lineage/expected.json
d95d547c52878e8ea2b9492887453249348529a75dfa804c41c354b5a21940d1  tests/migration/fixtures/sessions/fork-lineage/fork-request.json
d0c599b4d8d786435f961efa1ff17d7b1bad08e94e0de027b2d6c6f4b0680086  tests/migration/fixtures/sessions/fork-lineage/session.jsonl
62764808edab44aa0a8f03bb4cab473da3cec6b59f5ea93a902540609d25e240  tests/migration/fixtures/sessions/multi-turn-basic/expected.json
d0c599b4d8d786435f961efa1ff17d7b1bad08e94e0de027b2d6c6f4b0680086  tests/migration/fixtures/sessions/multi-turn-basic/session.jsonl
ca1e8281faa3c6d5bd316c326345fc2291f30b777b7eb70d5599aace901eced6  tests/migration/fixtures/sessions/pagination-corpus/expected.json
6e31fa0341149b1e4a2cc0f8ac6a13a90b62f922e59c82f4b7e6586f3eb20534  tests/migration/fixtures/sessions/pagination-corpus/session.jsonl
f613b70b41c3c1d1f760d53d62c9a08f5ce4ba651cab96386084a6f10b2ccf1d  tests/migration/fixtures/tool/cases.json
4dffb62d437c62f1256c6e3ee515a024328ef01d12c24466ae2ed4a1dbbd5ca3  tests/migration/fixtures/tool/expected.json
802627d8308d5829171eade08321c0f96123b5ed03c14c60290cd6eeccd64161  tests/migration/network-guard.ts
e4f86cee30cc9a0d381b7d074ac217d40a7663b014e884f55671e3b90dd342bd  tests/migration/normalize.ts
f8bdbebf2807a1619f61bef07796fc93018740fc8d7c908a6cd275f4ce6345b2  tests/migration/r00-a10-old-defect.test.ts
ba360dda0ca51a10cc786d486d4ea753cfea81672a4bbd40c16f9e202f9c647a  tests/migration/r00-t05-replay.test.ts
97947700d2dc5f59de83dac833b09e4854cb28f57865f85f9e9ae621f11e1f9b  tests/migration/stubs.ts
```

**候选聚合 SHA-256**（上表 88 行按「SHA256 + 双空格 + 路径 + 换行」拼接后整体哈希）：

```text
96c39f787e2f7230a28e6118c027302a1ed20843f003c316279cb841f990d052
```

## 9. 建议独立验收重点

1. 任选一组夹具，按 FIXTURE_MANIFEST 的 anchors 抽 2-3 个锚点到源码复核行号与语义。
2. 重跑 `node scripts/rust-tauri/r00-t05-replay.mjs`（约 15 秒）核对三 run 退出码与 normalized diff 为空；注意重跑会重写 artifacts 下 run-* 与 REPLAY_SUMMARY（时间戳/临时路径导致 raw 变化属声明的允许变动，normalized 应仍一致；若重跑后提交需重新生成 §8 哈希）。
3. 复核 A10：读 `fixtures/sessions/corrupted-tail/{expected.json,old-deviation.json}`，确认新期望未被改写为静默成功，并抽查 `tests/history-pagination-invalid-fixture.test.ts:2-17` 与 AGENTS.md 红线原文。
4. 检查替身边界：回放测试中被测对象无 mock（仅执行器/模型流为替身）；engine 桩只做身份映射。
5. 检查隐私：任意打开 fixtures 下输入文件确认无真实数据。
