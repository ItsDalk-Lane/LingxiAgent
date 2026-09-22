# CONSUMER_MIGRATION — 规范化事件收口与旧输出退出审计（P05-T02）

日期：2026-09-22｜基线 HEAD：`1f0537b08`。方法：从**消费者侧**反查（不是只看生产者），
逐一列出 canonical 输出（`canonicalEvents`）与旧输出（`visibleTextDeltas` → WS `text_delta`）
的全部真实消费者、其输入版本与处置。

## 1. 服务端批输出的消费者（canonicalEvents / visibleTextDeltas）

`AssistantEventNormalizer` 的批只有一个消费点：

| 输出 | 消费者 | 去向 | 状态 |
|---|---|---|---|
| `canonicalEvents` | server/routes/chat.ts `publishNormalizedAssistantBatch`（:1564-1577） | 逐条 `emitStreamEvent` → session-stream-store（seq）+ WS `assistant_segment_*` | 已收口（唯一入口） |
| `visibleTextDeltas` | 同上（:1568-1570） | `emitVisibleTextDelta` → WS `text_delta`（兼容链，仅 final_answer 文本） | 兼容保留（见 §3） |
| `diagnostics` | 同上（:1571-1576） | `log.warn`（unresolved_phase_fallback 可观察） | 已收口 |

无第二个读取 normalizer 批的模块；`tests/assistant-event-normalizer.test.ts` 为直接单测。

## 2. WS 层两类事件的真实消费者（桌面 + 面板）

| 消费者 | 消费事件 | 与 canonical 的关系 | 处置 |
|---|---|---|---|
| desktop `use-stream-buffer`（主聊天实时投影） | `assistant_segment_*` **主真相**；`text_delta` 仅当本 Run 未出现任何 canonical 段时的兼容回退（use-stream-buffer.ts:688-720） | 已迁移（canonical 优先，legacy 回退不双计：同一 Run 出现 canonical 后 legacy 不再驱动正文） | 已收口；幂等（canonical delta 按 seq 防重放叠加 :433-441） |
| desktop `ws-message-handler` | 路由两类事件到 streamBufferManager（:159-160） | 透传 | 已收口 |
| desktop `ChannelsPanel`（手机/渠道面板预览） | **仅 `text_delta`**（:540-553，拼 text 块） | 未迁——有意使用"final-only 精简视图" | **保留兼容 adapter 消费**（§3） |
| desktop `SubagentSessionPreview`（子代理会话预览） | **仅 `text_delta`**（:241-256） | 未迁——同为精简预览 | **保留兼容 adapter 消费**（§3） |
| `terminal-client` / RC 远程客户端 | 传输层消费 streamId/seq 与 resume 协议；正文消费同 ws-message-handler 链路 | 协议适配，不裁决语义 | 已收口（P05-T03 覆盖 resume） |

**上游（非本任务输出消费者，勿混淆）**：core/session-coordinator.ts:8609、
core/bridge-session-manager.ts:1391、core/desktop-session-submit.ts:711、
core/slash-commands/rc-router.ts:52、server/cli.ts:176、lib/bridge/* 消费的是
**Pi adapter 的 provider 子事件**（`assistantMessageEvent.type === "text_delta"`），
位于 normalizer 之前，属输入侧；标题预览、Bridge 可见文本、CLI 显示各自拼装，
不在本任务"旧输出退出"范围（Bridge 域为 P04 已收敛面，P05 不触碰铸造面）。

## 3. 旧输出（WS text_delta）的兼容契约与退出条件

**为什么现在不删**：`text_delta` 是服务端从 canonical 投影出的兼容适配
（只含 final_answer 文本、永不带保留标签），三个在用消费者依赖它：

1. 主聊天的历史兼容回退（Run 内无 canonical 段的旧数据/异常路径）；
2. ChannelsPanel / SubagentSessionPreview 的 final-only 精简视图（用户已采纳的显示行为）；
3. 未升级的远程/旧客户端（产品契约，ws-protocol.ts:24 明示"兼容期仍并行发送"）。

**兼容期不变量（本阶段核实为真，双投影对照见 §4）**：

- 同一段正文不会双份投递到主聊天：use-stream-buffer 在 canonical 出现后忽略 legacy 正文
  （"canonical 模式：正文唯一真相源是 assistant_segment_*"），tests/
  desktop/src/react/__tests__/hooks/use-stream-buffer.test.ts 与
  tests/chat-route-switching.test.ts "publishes OpenAI Responses phase semantics while
  keeping commentary out of the legacy answer" 锁定 commentary 不进 legacy answer。
- commentary 不进入 `text_delta`（visibleTextDeltas 只收 final_answer 段）——旧消费者
  因此天然看不到过程文本，这正是精简视图的语义。
- 进程间事件带同一 stream/call/segment 身份：WS 顶层 `streamId`/`seq` 由
  ws-protocol.ts createSessionStreamEventWsMessage 强校验并写进每条事件（:93-122）；
  segment 身份（segmentId）来自 normalizer 且持久化后可确定性重导出
  （MESSAGE_SEMANTICS §1）。

**退出条件（未满足前不删）**：主聊天回退路径确认不可达 + 面板/预览迁 canonical
（或产品决定面板只显示 final 的语义改为消费 canonical 并过滤）+ 远程客户端最低
支持版本升级。任何一项未满足时删除 `text_delta` 属于破坏产品契约，不是"清理冗余"。

## 4. 双投影只读对照（T02-3）

现有等价测试即只读对照——同一输入经实时链（chat 路由→WS→streamBufferManager）与
历史链（持久条目→extractPersistedAssistantSemanticSegments→buildItemsFromHistory）
各投影一次，归一化后逐字段比较（只豁免已登记的 mood 聚合差异与 id/timestamp 噪音，
正文、文件身份、结局不裁）：

- tests/live-history-reserved-tag-parity.test.ts 场景一/二（含 missing_final_answer）。
- tests/reserved-tag-text-preservation.test.ts T13（同源 raw 两条路径 answer 一致）。
- tests/tool-presentation-history.test.ts（工具/文件/大结果 live=history 详情等价）。

无双执行、无双写：对照只发生在测试内，生产只有一条链。

## 5. 结论与遗留

- **本阶段零生产改动**（UNCHANGED_VERIFIED）：canonical 唯一收口点、legacy 兼容链、
  消费者接线在当前源码均已正确；无"只在测试使用的新模块"。
- 旧输出退出属后续产品决策（涉及远程客户端最低版本），登记到 NEXT_STAGE_HANDOFF
  供 P08（旧路径退出）统一处理，不在 P05 强行删除。
- 范围外登记：ChannelsPanel/SubagentSessionPreview 若未来要显示过程文本，应改为
  消费 canonical 段（消费 final_answer 子集），本阶段不改其用户已采纳行为。
