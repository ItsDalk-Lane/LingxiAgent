# C01 接受边界追踪（2026-09-08，基线 67dee5d2）

## 提交路径
- prompt：chat.ts ws `prompt` → hub.send() → submitDesktopSessionMessage（hub/index.ts 桌面 owner 路由直通）
- interject：chat.ts ws `interject`（流式中）→ submitDesktopSessionInterjection → engine.steerSession
- agent review：chat.ts → AgentReviewTurnCoordinator.start() → runReview() → 父会话 submitSessionMessage（最后一次调用，带 clientMessageId）

## A. 接受前确定拒绝（服务端可证明未接受）
chat.ts 预校验（不产生任何副作用）：
1. 媒体校验：max_images×1、unsupported_image_format×1、image_too_large×1、max_videos×1、unsupported_video_format×1、video_too_large×1、invalid_video_content×1、max_audios×1、unsupported_audio_format×1、audio_too_large×1（server/routes/chat.ts:2448-2500）
2. agent_deleted（promptTarget.agentDeleted → rejectDeletedAgentSession）
3. 非流式仍忙：stillStreaming（isSessionStreaming 或 hasPendingParent）
4. model_switching（engine.isSessionSwitching）
5. invalid_knowledge_refs（normalizeKnowledgeRefs 抛错）
6. agent_review_interjection_not_supported / multiple_agent_reviews_not_supported / session_id_required_for_agent_review / invalid_review_agent

desktop-session-submit.ts 抛出（在 canonical 关联回执 fired 之前）：
7. session_busy×2（pendingDesktopSessionSubmissions / isSessionStreaming）
8. resolveDesktopSessionTarget：session not found / identity mismatch / sessionPath required
9. text,images,videos,audios required；engine API unavailable
10. KNOWLEDGE_SCOPE_VIOLATION、知识库入口未就绪、检索失败（非 abort）
11. preservePromptEnvelope+inboundFiles 冲突；failed to load session

评审协调器（父会话提交前）：
12. start() 同步抛 session_busy（hasPendingParent）
13. reviewed_session_model_unavailable / reviewer_session_creation_failed / reviewer_returned_empty_result
14. cancelByParent（用户停止，父会话未提交）

## B. 已接受后的运行失败（绝不许判为未接收）
- canonical 接受证据 = withInputCorrelation committed 回调：appendCustomEntry(DESKTOP_INPUT_CORRELATION_TYPE) + engine.emitEvent(session_user_message 带 sourceEntryId)。client 端 noteComposerServerAck 只认 sourceEntryId。
- canonical 回执已 fired 后 promptSession 抛错（供应商错误等）→ 运行失败。
- 客户端已收到 canonical 回执 → record.phase='accepted'，冲突负回执不得倒退。

## C. 无法证明是否接受的异常
- ws 断线 / ACK 丢失 / 历史分页不完整 → 保持 delivery_unknown + 显式核对（不变）。
- afterCachePreflight 已跑（inputSideEffectsStarted）但 canonical 未 fired 且提交链抛错：以 canonical-fired 为准——提交 Promise 已终结、不会再 append，判「未接受」（孤儿 presentation 条目按既有容忍规则跳过）。
- 旧服务端普通 error（无 input_rejected 事件、无 canonical 证据）→ 客户端维持 unknown，不猜测。

## 关键现有事实
- 客户端 ComposerSendPhase 无「服务端接受前拒绝」相位；awaiting_ack 15s 看门狗 → delivery_unknown；isUnresolved 含 delivery_unknown → tryAcquireSendLease transport_busy → 同会话发送持续被阻（C01 症状）。
- 同一队列项同版本重试在 tryAcquireSendLease 复用 clientMessageId → 需要每次尝试唯一身份（clientAttemptId）防迟到负回执误结算。
- 传输事实（ws.send 已执行）与接受结果分离：记录 attempts 数组，不改写传输史。
- 负回执只经 wsSend(ws) 单播给提交客户端；身份字段用服务端解析的 promptTarget，不信任载荷自称。
