# OLD_BEHAVIOR_ORACLE｜旧系统行为外部观察标准（R00-T05）

版本 1.0｜2026-09-24｜适用提交：`6f58b9351046e8dccfeb83be6976f197acb2de8d`（分支 `codex/rust-tauri-migration`）

本文是 Rust/Tauri 迁移的旧行为 oracle：把「旧系统对外可观察的输入、协议帧、输出消息、文件结果、错误码」
从真实生产入口提取为跨语言可读标准。它与 `tests/migration/fixtures/` 的输入/预期样本配套；
新实现（Rust）按同一夹具重放时应得到语义等价结果，**已知旧缺陷除外**（见 §10）。

## 0. 怎么使用

```bash
# 单次回放（断言全部夹具与冻结 expected 逐字一致）
npx vitest run tests/migration/r00-t05-replay.test.ts tests/migration/r00-a10-old-defect.test.ts

# A09 三次重放 + 规范化 diff（断网隔离，产出 artifacts/rust-tauri/R00/T05/）
node scripts/rust-tauri/r00-t05-replay.mjs
```

- 回放全部走**真实旧实现**（生产路由 / 生产模块 / 真实 SDK）；仅外部协议边界（模型流、工具执行器）
  使用确定性替身（`tests/migration/stubs.ts`），符合任务书 05 验收协议第 1 层替身边界。
- expected.json 冻结自首次验证运行，且每个断言点都对照过既有公开测试（锚点列在
  `fixtures/FIXTURE_MANIFEST.json` 各条 `source.anchors` 与下文各节）——不是对 JSON 自比。
- 所有文件/进程只使用 `os.tmpdir()` 临时目录；`LINGXI_MIGRATION_BLOCK_NETWORK=1` 时测试 worker
  内阻断 net/tls/dns 外连。
- 规范化（`tests/migration/normalize.ts`）只屏蔽 FIXTURE_MANIFEST 声明的允许变动字段
  （临时路径、生成 id、时间戳类键、pid），其余逐字节比较。

## 1. 会话 JSONL 存储（多轮会话）

**格式**（Pi SDK `SessionManager` 写入；`lib/pi-sdk/index.ts` 统一 re-export）：

- 文件头：`{type:"session", version:3, id:<UUID>, timestamp, cwd, parentSession?}`。
- 消息行：`{type:"message", id, parentId, timestamp, message:{role:"user"|"assistant"|"toolResult", content}}`。
- custom 行：`{type:"custom", customType, data, id, parentId, timestamp}`（如 `hana-message-presentation`）。
- 文件名：`<ISO时间(冒号/点→连字符)>_<sessionId>.jsonl`；目录 `<agentsDir>/<agentId>/sessions/`。
- 业务 sessionId（`sess_<base36>_<20hex>`，SessionManifestStore SQLite 生成）与文件头 SDK UUID 是**两套身份**。

**读取分层**（观察到的真实行为）：

| 层 | 入口 | 损坏输入行为 |
|---|---|---|
| 严格 | `lib/session-jsonl.ts` `readCurrentSessionBranch` | 结构化抛错（见 §9 错误码） |
| 宽容 | `lib/session-jsonl.ts` `readSessionMessages` | 只返回 root→当前 leaf 的 user/assistant 消息；单行损坏**静默丢弃** |
| SDK | `SessionManager.getBranch()` | malformed 行跳过；文件尾无换行自动补 `\n` |

夹具：`sessions/multi-turn-basic`（6 条目链 u1→a1→r1→a2→u2→a3）。
关键断言：严格层 lineage 全序 + `headResolution=legacy_tail`；宽容层 5 条 user/assistant；
路由 display id 为顺序字符串序号，assistant 带 `toolCalls`（status/startedAt/endedAt）与
`assistantSegments`（`assistant:<n>:reasoning:default` 形状）投影。

## 2. 历史分页

路由 `GET /api/sessions/messages`（`server/routes/sessions.ts:1383`）：参数 `path|sessionId`、
`before`（display 序号边界）、`limit`（≤200，缺省 50）、`all=1`。游标语义（`server/history-read/page.ts:38-49`）：

- `endIdx = min(before, total)`；缺省/负/NaN → total（最新页）；`startIdx = max(0, endIdx-limit)`；
  `hasMore = startIdx > 0`；`nextBefore = String(startIdx)`（仅 hasMore 时下发）；`before=0` → 空页。
- display 序号只由 user/assistant 可显示消息推进，toolResult 不占序号。
- 响应形状：`{messages, blocks, todos, todoPanel, hasMore, nextBefore, sessionFiles, revision}`。

夹具：`sessions/pagination-corpus`（24 display）。
关键断言：页序 10/10/4、nextBefore 链 `14→4→null`、页序反转拼接 ≡ `all=1`（同序同数）、零重叠、
同参数重走确定。**不变量：任意切页恢复 ≡ 全量恢复**（既有
`tests/history-pagination-run-continuity.test.ts` 同源）。

## 3. MOOD / 保留标签

- 词表：`shared/internal-mood-block.ts` `["mood","pulse","reflect"]`；思考标签 `["think","thinking","mm:think"]`。
- 流式权威入口：`core/events.ts` `MoodParser`（事件 `mood_start/mood_text/mood_end/text`），生产链
  `text_delta → ThinkTagParser → MoodParser → 干净文本`（`server/routes/chat.ts:1468-1493`）。
- 历史切分权威入口：`shared/reserved-tag-stream.ts` `splitReservedTagSegments(content, tags)`。

夹具：`mood/stream-scripts` 六个脚本。关键断言（与 `tests/mood-parser.test.ts` 同形）：

- 标签可在正文任意位置、一轮多个块；跨 chunk 撕裂仍解析；代码围栏/反引号/转义按字面透传；
  未闭合块 flush 补 `mood_end`；闭标签必须与开标签同名。

**如实登记的模块级分歧**（新系统须裁决并保证路由级一致，02 契约 §7「MOOD/思考标签兼容解析只有一个权威入口」）：

1. 闭标签后紧跟换行：live 解析器丢弃该换行（`mood-parser.test.ts:20-26` 断言 text 为 `after`），
   raw 切分器保留 `\n`。生产级实时/历史等价由 `tests/live-history-reserved-tag-parity.test.ts`
   在路由层保证。
2. 未闭合开标签：live flush 视为 mood 块（补 `mood_end`）；raw 切分器按字面文本。

## 4. 工具调用（统一网关）

请求 `ToolInvocationGatewayRequest`（`core/tool-invocation-gateway.ts:19-32`）：
`{targetId, route, arguments, sessionId?, sessionPath?, agentId?, lifecycleGeneration?, toolCallId, signal?, onUpdate?, ctx?}`；
route ∈ `direct|deferred|plugin-dev-chat|plugin-dev-http|isolated`。

**错误码全集**（`lib/tools/invocation/errors.ts:3-22`，18 个；错误文本自动脱敏 Bearer/sk-key/内路径）：
`TARGET_NOT_FOUND … TRANSPORT_FAILURE, EXECUTION_CANCELLED`（逐字清单见 `tests/tool-invocation-errors.test.ts:11-30`）。

夹具：`tool/cases.json` 六用例。关键语义：

- 合法 prepared 调用执行一次；执行句柄 ctx 身份被网关强制覆写
  （`invocationRoute="direct"`、`effectiveTargetId=<真实 target>`，模型 args 伪造字段经严格 schema → `ARGUMENTS_SCHEMA_INVALID`）。
- prepared 后篡改参数/目标/会话 → `PREPARED_INVOCATION_MISMATCH` fail-closed；仅键序变化经
  sha256(key-sorted canonical JSON) digest 稳定匹配放行。
- generation 或可用性变化 → `TARGET_REVOKED`；无 prepared 上下文 → `PREPARED_INVOCATION_MISSING`。
- 执行器普通异常 → `TRANSPORT_FAILURE`。

## 5. 取消

工具网关三时点（`core/tool-invocation-gateway.ts:465-490`）：执行前 abort → `EXECUTION_CANCELLED`
零执行；执行器抛 `AbortError` → `EXECUTION_CANCELLED`；**执行器完成后 signal 已 abort → 仍
`EXECUTION_CANCELLED`（完成态不复活）**。

审批等待取消（真实 ConfirmStore + ask 权限 wrapper）：`abortBySession` → 工具零执行、
结果 `{isError:true, details:{confirmed:false, confirmation:{status:"aborted"}}}`、store 清空；
已 abort 的 confirmId 迟到 `resolve` 返回 false。

会话级取消补充锚点（本轮未在夹具中展开、供 R03/R04 引用）：WS `abort_result{accepted|already_stopped|rejected(stale_stream)}`、
合成 `turn_end{aborted:true}` + `session_status{aborted:true}` + `assistant_run_end{status:"aborted"}`、
历史层 `<hana-turn-interrupted>` 合成 user 标记（`tests/chat-route-switching.test.ts`、
`tests/interrupted-turn-marker.test.ts`、`tests/session-coordinator-isolated-abort.test.ts`）。

## 6. 认证失败

服务端认证（`core/server-auth.ts` `authenticateRequestDetailed`，HTTP 面
`server/http/request-principal.ts` 消费：失败 403）：结构化 deny `{error:"forbidden", reason, …}`。

| 输入 | 可观察结果 |
|---|---|
| 正确 loopback token + local 连接 | `principal{kind:"local_user", credentialKind:"loopback_token", trustState:"local"}` |
| 错误 token | `denied reason="invalid_credential"` |
| loopback 凭证过 lan/custom_remote | `denied reason="loopback_token_requires_local_transport"` |
| 缺失凭证 | `denied reason="missing_credential"` |

模型面认证失败为 `LLM_AUTH_FAILED`（HTTP 401 语义、不可重试；`shared/errors.ts:17`；
provider 401/403 映射 `core/llm-client.ts:950-956`）。OAuth 刷新 `invalid_grant` → 结构化拒绝且本地凭证不动
（`tests/oauth-force-refresh.test.ts:155-175`）。MCP 认证类失败 → connector `needs-auth` 不再重试。

## 7. 附件 / SessionFile

- 登记是**引用式**：`registerFile` 不复制文件（managed 物化由上传/桥接流程负责）；
  sidecar `<sessionPath>.files.json` `{version:1, files, refs}`；fileId 形状 `sf_<16hex>`。
- 会话隔离：另一 sessionPath 查询同 fileId → `null` 且不改写他人 sidecar。
- fork：只复制 reachable 文件（由 retained 消息里的 `[SessionFile]`/`[attached_image:]` 引用决定）；
  子条目换新 id 并带 `legacyFileIds/legacyFilePaths`；源不动。
- discard fork：返回 `{sessionId, sessionPath, sidecarDeleted, managedCacheDeleted, unloaded}`，幂等，源存活。
- 上传错误码面（供 R04/R07 扩展）：`"symlink not allowed"`、9 附件上限、`"unsupported mimeType"`、
  `"video content does not match mimeType"`、`/too large/`（`tests/upload-route.test.ts`）。
- prompt 正文信封：`[SessionFile] {fileId,…}` JSON 行 + `[attached_image: /abs/path]`；广播
  `session_user_message` 的 attachments 条目不含 base64Data。

## 8. 会话 fork

生产链路：UI `forkSessionTurn` → `POST /api/sessions/fork` → `engine.forkSessionAtNode` →
`sourceManager.createBranchedSession(boundaryEntry.id)`（`core/session-coordinator.ts:3690`）。

SDK 语义（`session-manager.js:1113-1180`）：保留 root→boundary 路径（label 剔除并重挂父链），
新文件头 `{version:3, id:<新UUID>, parentSession:<源路径>}`；**源文件字节不变**。
路由错误码：流式中 `session_busy` 409、谱系深度 `session_fork_depth_limit` 409（MAX_FORK_LINEAGE_DEPTH=2，
调用 fork 前拒绝）、engine 无实现 `session_fork_unavailable` 503。

## 9. 会话分支错误码（严格层）

`SessionBranchError{code, details}`：截断 JSON → `session_branch_invalid_json`（details 含行号）；
重复 ID → `session_branch_duplicate_id`；环/自引用 → `session_branch_cycle`；缺父 →
`session_branch_dangling_parent`；head 丢失 → `session_branch_head_missing`。
分支头解析：`legacy_tail` / `persisted_head` / `append_recovery`（`tests/session-branch-head.test.ts:39-124`）。

## 10. 已登记旧缺陷（不得成为新标准）

**缺陷：会话 JSONL 损坏尾记录的静默降级**（`sessions/corrupted-tail/old-deviation.json`）。

- 现象：用户可见历史读取对含损坏尾记录的会话返回 200 与 3 条消息，响应体零标注；磁盘留有
  `.repair.json` 回执（路由/生产入口触发，`core/session-jsonl-file.ts` 的
  `repairOversizedSessionEntriesInFile` 会移除坏行）但回执不进响应；宽容层
  `readSessionMessages` 连回执都不产生。
- 违反规则：AGENTS.md 红线「禁止静默降级（错误要么抛要么显式降级并标注）」；既有先例证明
  「显式降级并标注」在本仓库可达（超长行修复返回 `repaired/projected` 计数）。
- 修正后不变量（`sessions/corrupted-tail/expected.json`，新系统标准）：读取损坏尾记录必须
  (a) 抛结构化错误 `session_branch_invalid_json`（严格层已具备该能力），或 (b) 显式降级并在
  **响应**中携带标注（至少 `droppedCorruptLines:1`）。禁止静默成功。
- 机器验证：`tests/migration/r00-a10-old-defect.test.ts`（5 用例：严格层可识别、响应级静默
  事实、新期望不接受静默成功、偏差登记与规则来源完整）。

## 11. 隐私与许可

全部夹具为合成内容：无真实用户数据、真实人名、凭证、真实附件或用户目录内容；时间戳为固定
过去值或被规范化屏蔽；认证 token 为显式标注 `not-a-secret` 的合成值。会话行结构复刻 Pi SDK
（仓库既有依赖）写入格式，属接口事实提取，不涉及第三方版权内容。
