# RECONNECT_CONTRACT — streamId/seq 可解释重连（P05-T03）

日期：2026-09-22｜基线 HEAD：`1f0537b08`（测试新增于本阶段，生产实现 UNCHANGED_VERIFIED）。

## 1. 协议现状（先记录，后验证）

传输与字段**本轮零新增**——现有协议已完整覆盖可解释重连，无需版本化新字段：

- 每轮用户 Run 一个 `streamId`（chat.ts beginAssistantRun：一个 Run = 一个 streamId，
  Run 内多 Model Turn 复用，绝不重分配）；流内每事件 `seq` 单调递增
  （session-stream-store.ts appendSessionStreamEvent）。
- 客户端 → 服务端：`{type:"resume_stream", sessionPath, sessionId, streamId?, sinceSeq}`
  （ws-protocol.ts:10）。
- 服务端 → 客户端：`{type:"stream_resume", streamId, sinceSeq, nextSeq, reset, truncated,
  isStreaming, runtimeIsStreaming, events:[{seq,event,ts}]}`（ws-protocol.ts:56）。
- 每条广播事件顶层携带 streamId/seq 且与事件内同名字段冲突即抛（协议层强校验，
  ws-protocol.ts createSessionStreamEventWsMessage:93-122）。

## 2. 语义规则（生产实现坐标 + 本阶段验证）

| 规则 | 实现 | 验证 |
|---|---|---|
| seq 仅在所属 stream 内比较 | resumeSessionStream 以 streamId 先判流归属；不同流绝不按 seq 去重 | 新增路由测试 A04 + 消费端测试（§4） |
| 重复事件幂等 | 服务端重放只按 `seq > sinceSeq` 过滤；消费端 `consumedSeqs` + canonical delta `appliedSeq` 双防御（use-stream-buffer.ts:433-441、stream-resume.ts dispatchReplayEvent:278-293） | 新增 A03 + 既有 stream-resume/dedupe 测试 |
| 旧 stream 请求 → reset 重建 | `requestedStreamId !== currentStreamId` → `reset:true, sinceSeq:0`，重放当前流全部存活事件（store:114-124） | 新增 A04（断言重放内容不含旧轮正文、seq 从 1 起） |
| 缺口/截断可见 | `sinceSeq < firstSeq-1` → `truncated:true`，`sinceSeq=firstSeq-1`，只补存活窗口（store:126-141） | 新增 A05（5000 事件上限真实触发 trim） |
| 大事件压缩可观察 | 超单事件字节上限先结构化压缩并带 `compacted:true, originalByteLength`，仍超则 `omitted:true` 占位（store:162-205） | 既有 store 测试"压缩单个超大事件" |
| 流结束缓存清空 | finishSessionStream 清 events、保留 streamId/nextSeq 终态（store:84-89）；resume 返回空事件 + `isStreaming:false`（+ 缺口时 truncated） | 新增 A06 |
| 恢复用持久历史而非 ring | 消费端 `shouldHydrateCompletedEmptyResume`（nextSeq>1 且空事件）→ 整段历史重建 + 终态收口，不等待已消失缓冲（stream-resume.ts:226-231,295-345） | 既有 stream-resume.test.ts + 新增 A06（服务端形状） |
| 订阅/历史快照切换水位 | 增量叠加信任门槛：仅当响应 streamId 与本地一致 **且** 断点 seq 是本地真实消费过的 seq 才允许叠加；否则整段重建（canApplyIncrementalResume，stream-resume.ts:355-366）——同一事件不会既补取又实时追加两次 | 既有 stream-resume-gate.test.ts 5 例 |

## 3. 有界缓存参数（S25 保留，不重做）

`DEFAULT_MAX_EVENTS=5000`（≈1–2k 事件/正常轮的 2.5–5 倍余量）、
`DEFAULT_MAX_BYTES=8MiB`、`DEFAULT_MAX_EVENT_BYTES=256KiB`；turn 结束即清空。
本阶段未改动任何上限。

## 4. 本阶段新增验证（真实入口，不 mock store）

1. **tests/chat-route-switching.test.ts** 新增 describe
   `P05 stream resume semantics (real route + real stream store)`，4 例全部经真实
   `createChatRoute` onMessage(resume_stream) → 生产 resumeSessionStream：
   - A03 增量续传不重复 + 重复请求幂等 + 越界断点诚实空增量；
   - A04 跨 stream：旧 streamId → reset 全量重建为当前流（seq 从 1 起、无旧轮正文）；
   - A05 ring 截断：2600 个 thinking_delta 真实触达 5000 事件上限，truncated 可见、
     从最早存活事件补起、nextSeq 不失真；
   - A06 流结束缓存清空：空事件 + isStreaming=false + 缺口 truncated；已追平客户端
     空增量无截断。
2. **desktop/src/react/__tests__/services/stream-resume.test.ts** 新增 1 例（P05-A04
   消费端）：换流后新流 seq=1 不被旧流已消费 seq=1 去重，reset 重放事件真实送达 handler。

首次运行各留 1 次失败记录（A05 断言口径修正、消费端异步等待修正），重跑全绿；
日志见 artifacts/refactor-2026/P05/logs/（P05-T03-*，共 5 条含回归）。

## 5. 结论

生产实现为 **UNCHANGED_VERIFIED**（S25 语义完整、消费端信任门槛完备）；
本阶段增量 = 路由级真实恢复测试补齐此前缺失的 A04/A05/A06 场景锚点 + 本契约文档。
未发现需要新增协议字段或修改传输的理由——"最小补充"为不补充。
