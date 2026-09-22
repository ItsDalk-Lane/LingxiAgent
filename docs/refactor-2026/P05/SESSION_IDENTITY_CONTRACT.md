# SESSION_IDENTITY_CONTRACT — 会话双身份引用约束（P05-3，P00 OWNERSHIP_MAP ⚠️ 项收口）

日期：2026-09-22｜基线 HEAD：`1f0537b08`。**结论：不合并身份**（有意分层），
本文档把约束固化为契约；防误用测试已存在（§4），本阶段零生产改动。

## 1. 两套身份及其唯一权威

| 身份 | 格式/来源 | 唯一权威 | 持久化 |
|---|---|---|---|
| 业务 sessionId | `sess_{ts36}_{rand20hex}`（core/session-manifest/id.ts:5-9；唯一性 store 校验） | SessionManifestStore（core/session-manifest/store.ts，DDL 内 schema_version） | session-manifest.db；路由/WS/资源/观测全部以此为主键 |
| SDK session UUID | Pi SDK SessionManager 分配（UUID；条目 id=UUID 前 8 位） | Pi SDK SessionManager | 会话 JSONL 文件名 `agents/{agentId}/sessions/{sdkUuid}.jsonl` + 文件头 |

**分层是有意的**：业务身份稳定跨 SDK 会话重建/分支/重试（rewind、branch、fork 不改
sess_ id）；SDK UUID 绑定物理 JSONL 与 Pi 条目树。合并将迫使 Pi 侧文件/分支结构
服从业务身份，破坏 SDK 兼容边界（P01 已锁定 Pi 适配唯一入口纪律）。

## 2. 换算点（全部经边界解析，不允许散落换算）

- WS 输入：`server/ws-scope.ts resolveWsSessionContext`——唯一解析入口：
  裸 path 经 manifest currentLocator 反查；裸 sessionId 经 manifest 正查；
  sessionId 与 path 不一致 → internal_contract 拒绝（不猜）。
- HTTP 路由：`engine.getSessionIdForPath` / manifest currentLocator（sessions、
  history-read read-context 等）。
- WS 出站流身份：`sessionRefVersion: 2` + sessionId 由 ws-protocol.ts 在
  stream 事件/stream_resume 上强制携带并校验冲突（createSessionStreamEventWsMessage）。
- 观测/台账：mt_ 会话级轨迹以业务身份键控（P04 TRACE_COMPAT_REPORT；P05 只消费）。

## 3. 约束（后续阶段必须遵守）

1. **不合成身份**：消息缺身份 = 调用方 bug（internal_contract 错误），不是缺省值
   （ws-session-context "rejects a message that carries no identity at all as an
   internal contract violation"）。
2. ** disagreement 即拒绝**：sessionId 反查 path 与携带 path 不同 → 拒绝
   （chat-route-switching #2078 两例：prompt 与 steer/resume 目标）。
3. **会话状态键控用业务身份**：chat 路由 sessionState 以 sessionId（退化为 path）
   为键——"keeps shared stream state attached to the session id when the session
   path moves"（路径移动不改流归属）。
4. **JSONL 文件名 = SDK UUID** 是已知复杂度根源（换算必须经 §2 边界）；任何新代码
   不得从文件名猜业务身份，也不得把 SDK UUID 写进业务键位。历史教训：session-
   coordinator 中曾因换算错位导致 trace 复用零命中（P00 OWNERSHIP_MAP 第 9 行引注）。
5. `sessionRefVersion` 只升不降；旧客户端不带该字段时按 v1 兼容读取，不重写。

## 4. 防误用测试（现行锁定，P05 回归通过）

- tests/ws-session-context.test.ts（13 例：无身份/反查不一致/显式 sessionId 无
  manifest/locator 指向别处/bare sessionId 解析/agent 归属覆盖客户端声明等）。
- tests/chat-route-session-identity.test.ts（slash/desktop 身份：稳定 id 注入、
  locator 仅兼容不合成、无身份=内部契约违规）。
- tests/chat-route-switching.test.ts（#2078 prompt/steer-resume 身份不一致拒绝；
  会话路径移动保持流状态归属）。
- 本阶段新增的恢复测试同样经 requireWsSessionContext 真实身份链
  （tests/chat-route-switching.test.ts `P05 stream resume semantics`）。

## 5. P05 处置

UNCHANGED_VERIFIED + 文档化收口。不合并身份、不新增换算点、不改 manifest schema。
后续新增会话引用面（P06 上下文/P07 性能/P08 导出）按 §2/§3 接线。
