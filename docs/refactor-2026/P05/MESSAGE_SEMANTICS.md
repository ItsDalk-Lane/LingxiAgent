# MESSAGE_SEMANTICS — 内容类型与结果裁决的现行权威（P05-T01）

日期：2026-09-22｜基线 HEAD：`1f0537b08`（P04 后）｜状态：基于当前源码逐点核实，非任务书复述。

本文件回答任务书 T01 的四个问题：每类内容的事实来源在哪里、正文解析器只有几套、
Run 结局由谁裁决、unresolved/晚到 phase 的策略是什么。每一条都给出生产源码坐标与
锁定该语义的现有测试；语义 fixtures 索引见 `SEMANTIC_FIXTURES.json`。

## 1. 端到端语义链（唯一链，无第二套投影器）

```
供应商事件（Pi adapter）
  → core/events.ts ThinkTagParser / MoodParser（保留协议 <think>/<mood>/<pulse>/<reflect>
     仅在此边界结构化剥离；正文里"长得像标签"的文字不经此判定）
  → server/assistant-event-normalizer.ts AssistantEventNormalizer（唯一正文语义裁决者）
     输出 canonicalEvents（assistant_segment_start/delta/end × semanticPhase）
     + visibleTextDeltas（兼容期旧正文链，仅 final_answer 段）
  → server/routes/chat.ts publishNormalizedAssistantBatch → emitStreamEvent
     → server/session-stream-store.ts（streamId/seq/ring buffer，唯一流缓存）
     → ws-protocol.ts createSessionStreamEventWsMessage（广播，顶层带 streamId/seq）
  → desktop use-stream-buffer（canonical 段为主真相，legacy text/thinking 事件仅作
     "本 Run 未出现 canonical 事件"时的兼容回退）→ projectAssistantTurn（live 投影）

历史路径（与 live 共用同一 turn 投影器）：
  session JSONL 条目 → core/message-utils.ts historyMessageFromEntry
  → shared/assistant-semantic-segments.ts extractPersistedAssistantSemanticSegments
     （从持久化 content 块确定性重导出 segments：thinking 块→reasoning 段；
       text 块 textSignature→commentary/final_answer，缺省 final_answer）
  → server/history-read/project-page.ts projectHistoryPage → /api/sessions/messages
  → desktop history-builder.ts buildItemsFromHistory → projectAssistantTurn（同一投影器）
```

关键不变量：live 与 history 在 `projectAssistantTurn`
（desktop/src/react/utils/turn-projector.ts:192）汇合——同一 `LiveAssistantSegment[]`
形状、同一裁决规则。等价性由 tests/live-history-reserved-tag-parity.test.ts 用
真实 createChatRoute + 真实 streamBufferManager + 真实 buildItemsFromHistory 锁定。

## 2. 每类内容的语义来源（生产者 → 消费者）

| 内容类型 | 唯一语义权威（生产） | 实时载体 | 历史载体 | 持久化形状 |
|---|---|---|---|---|
| 正文 final_answer / commentary | AssistantEventNormalizer（semanticPhase per segment） | `assistant_segment_*`（canonical）+ `text_delta`（兼容，仅 final） | `assistantSegments`（project-page.ts:372-385 重导出） | content[].text 块 + `textSignature`（commentary 标记，shared/text-signature.ts） |
| reasoning / thinking | ThinkTagParser（`<think>` 协议）+ provider thinking_delta → normalizer `handleReasoningDelta` | `assistant_segment_*`（kind=reasoning）+ `thinking_start/delta/end`（兼容） | thinking 块合并为单一 reasoning 段（assistant-semantic-segments.ts:40-54） | content[]{type:"thinking"} |
| MOOD | MoodParser（INTERNAL_MOOD_TAGS：mood/pulse/reflect；core/events.ts:96）——**不在 canonical segments 内**，独立结构化事件 | `mood_start/mood_text/mood_end` | 历史按 assistant message 逐条 leading-only 重析（history-builder-mood 测试锁定） | leading mood 块（结构化）+ 兼容期正文残留由 sanitizePersistedSegments 剥离 |
| tool 调用/结果 | 工具执行边界（P03 网关）产出 toolResult 条目；展示投影 `projectToolPresentationDetails` | `tool_start/tool_end` + `content_block` | toolCalls + `collectToolOutcomesByCallId`（shared/tool-outcome.ts）按 callId 归并 | assistant 条目 content[]{type:"toolCall"} + toolResult 条目 |
| 文件交付 | session 文件注册表 + `server/session-file-block.ts sessionFileToContentBlock`（实时交付和历史重建共用同一份文件证据） | `content_block{type:"file"}`（deferred result 经 enrichSessionFileBlocks） | `sessionFiles`（hydrateExternalState 汇入 history 端响应） | `.jsonl.files.json` sidecar（fileId→filePath/version/mtimeMs） |
| 控制卡（建议/确认/插件卡） | server/suggestion-blocks.ts（automation/autolearn 建议）+ BLOCK_EXTRACTORS（cron/settings confirm、plugin_card，server/block-extractors.ts:128,204） | `content_block` | BLOCK_EXTRACTORS 在 history 重放同一 details | toolResult details 内嵌 |
| 状态（status） | chat.ts `broadcast({type:"status", isStreaming, streamId, turnId})`——运行态广播，非裁决 | WS `status` | 不入历史（历史由条目重投影） | — |
| Run 结局 | **P02 交接的 `assistant_run_end`（agent_settled exactly-once，唯一 finalize）** | `assistant_run_end{status}` | project-page.ts:387-391：持久条目 `stopReason`（error→failed / aborted→aborted / 其余 completed）；missing_final_answer 等由 history 投影按 Run 事实派生 | assistant 条目 stopReason + Run 边界（turnStartIndex/turnEndIndex） |

## 3. 原始 / 规范化 / UI 派生的边界（T01-2）

- **原始供应商内容**：`event.partial` / `message.content` / `assistantMessageEvent.content`。
  normalizer 的 text_end 回退（chat.ts:1610-1640）只在 raw 源**没有**流式 delta 时使用，
  且一旦该 contentIndex 的 raw 源进过保留协议管道（`reservedProcessedTextKeys`），
  全部文本 fallback 入口同时置空（chat.ts:1529-1537 blankAssistantTextBlocks）——
  裸标签不会二次回流。
- **规范化内容**：canonical segments。保证永不含裸 `<mood>/<think>` 标签（chat.ts:1450 注释
  与实现一致：解析先于 normalizer）。
- **UI 派生块**：mood 聚合（live 一个 turn 多段 mood 聚合成单块 `\n\n` 分隔；历史按消息逐块）、
  turn_status 派生（missing_final_answer 等）。这是 §27 已登记的显示层有意差异，
  parity 测试对 mood 比内容总序列而非块数。
- **正常对话文本里的"类标签"文字**：用户输入与正文示例（如"示例：<mood>高兴</mood>"）
  不被无差别删除。保留协议只在配对结构完整且位于协议可判定位置时结构化；
  未知配对标签保留、孤儿闭标签仅在 assistant 边界清理、code fence 内不动——
  全链锁定于 tests/reserved-tag-text-preservation.test.ts T01–T16
  （含 T15：用户输入含 HTML/XML 不进 assistant 残渣清理）。P05-A02 场景由此覆盖。

## 4. 正文解析器清点（T01 检查项：没有第三套）

1. `AssistantEventNormalizer`（服务端实时，唯一流式裁决）。
2. `extractPersistedAssistantSemanticSegments`（服务端历史，从持久块确定性重导出——
   不是解析器，是同一语义的持久化逆函数；phase 只读 textSignature 元数据，不重扫文本）。
3. `MoodParser`/`ThinkTagParser`（保留协议边界，非正文裁决）。
4. 前端 `history-builder`/`use-stream-buffer` 不自行判 phase，只消费 segments +
   兼容回退；`turn-projector.ts:71-101` 对 streaming 中的 unresolved 保留原样、
   sealed 时统一落 commentary/final_answer。

结论：**实时/历史各一条正文语义链，在 turn-projector 汇合，无第三套解析器**。

## 5. Run 结局与交付结局的连接（T01-3）

- 实时：`assistant_run_end` 只在 `agent_settled` 产生（chat.ts finishAssistantRun:910，
  exactly-once，P02 已锁）。**没有任何路径**以文本长度、工具数量、某页是否有答案重新裁决。
- 历史：`turnStatus` 只由持久 `stopReason` 映射（project-page.ts:387-391）；
  `missing_final_answer` 是 Run 级派生事实（该 Run 无任何 final_answer 段），
  tests/history-run-outcome-edges.test.ts T07/T10a/T10b/T12 锁定：
  真完成只有过程→恰一个 missing_final_answer；中间失败后恢复成功→整轮完成；
  最终失败带部分正文→failed 与部分正文共存；相邻 Run 不误并。
- 工具结局不放大为 Run 结局（chat-route-switching.test.ts "does not count a
  length-limited thinking-only reply as a normal success" 等锁定）。

## 6. unresolved / 晚到 phase 的现行策略（T01-4）

- **哪些供应商晚到**：`PHASE_AT_END_APIS` = openai-codex-responses / openai-responses /
  azure-openai-responses + provider `openai-codex`（normalizer:45-49,98-106）。
  这些 API 的 text_delta 阶段 phase 未知 → 段以 `unresolved` 开始。
- **有限缓冲，不无限等**：unresolved 只维持到该段 `text_end`/`finishMessage`；届时按
  优先级（事件显式 semanticPhase/phase → 块 textSignature → 既有 commentary 保持 →
  否则 final_answer）落定。fallback 时产出 diagnostic `unresolved_phase_fallback`
  并 warn 日志（chat.ts:1571-1576）——**不提前当 final、不无限缓冲、可观察**。
- **不凭空造段**：text_end 无可见文字时不产生空 final_answer 段（normalizer:181-185），
  全 mood/think 消息不豁免 missing_final_answer。
- 语义变更需求：本阶段未改任何 phase 语义（P05 生产行为零差异，见 P05_REPORT §差异）。

## 7. 本阶段结论（T01）

实现全部为 **UNCHANGED_VERIFIED**：上述链路、权威与策略在当前源码已正确存在并被
既有测试锁定。本任务交付的增量是：本文件把散在源码注释与测试里的语义权威收拢为
单一参考（此前无此文档），并给出 fixtures 索引（SEMANTIC_FIXTURES.json）供
P06（上下文同源）与 P07（同语义性能对比）引用。未发现需要修改的生产缺口。
