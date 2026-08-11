/**
 * loop-messages.ts — 循环相关注入消息的构造器
 *
 * 形状对齐后台结果通知消息：{ customType, content, display:false, details }。
 * turn 类消息（kickoff / wakeup）触发模型轮；notice 类只落记录不触发轮。
 * 桥接目标的投递层只取 content 字段作为外呼轮的输入文本。
 */
export const LOOP_TURN_MESSAGE_TYPE = "loop-turn";
export const LOOP_NOTICE_MESSAGE_TYPE = "loop-notice";

export function buildLoopKickoffMessage(loop) {
  const { maxTurns, maxConsecutiveFailures } = loop.limits;
  const content = [
    `<hana-loop kind="kickoff">`,
    `This session is now in recurring-loop mode.`,
    `Task: ${loop.prompt}`,
    ``,
    `Each loop turn: do the work or the check now. Then take EXACTLY one of:`,
    `1. Keep working, including dispatching background work as usual — its completion wakes this session automatically.`,
    `2. Call loop_control {action:"schedule", delay_seconds, reason} to set the single fallback alarm for the next check. Use it only for external state the system cannot observe, or as a long hang-protection fallback. Never schedule short alarms to poll background work.`,
    `3. Call loop_control {action:"complete", reason} once the task's goal is achieved — this ends the loop.`,
    ``,
    `Budget: ${maxTurns} loop turns; ${maxConsecutiveFailures} consecutive failed turns pause the loop automatically.`,
    `</hana-loop>`,
  ].join("\n");
  return {
    customType: LOOP_TURN_MESSAGE_TYPE,
    content,
    display: false,
    // 结构化保留用户原始 prompt，供历史渲染层（/api/sessions/messages）
    // 提炼成一条用户可见的 interlude 气泡。display:false 只隐藏协议正文，
    // 不影响 details 被读取——否则用户会看到自己输入的任务"凭空消失"。
    details: { schemaVersion: 1, kind: "kickoff", prompt: loop.prompt },
  };
}

export function buildLoopWakeupMessage(loop, reason) {
  const content = [
    `<hana-loop kind="wakeup">`,
    `Scheduled wakeup fired. Reason: ${reason || "(none recorded)"}`,
    `Loop task: ${loop.prompt}`,
    `Progress: loop turn ${loop.turnCount}/${loop.limits.maxTurns}.`,
    `Continue the task now; then schedule the next wakeup, or call loop_control {action:"complete"} if the goal is achieved.`,
    `</hana-loop>`,
  ].join("\n");
  return {
    customType: LOOP_TURN_MESSAGE_TYPE,
    content,
    display: false,
    details: { schemaVersion: 1, kind: "wakeup", prompt: loop.prompt, reason: reason || null },
  };
}

export function buildLoopNoticeMessage(text) {
  return {
    customType: LOOP_NOTICE_MESSAGE_TYPE,
    content: text,
    display: false,
    details: { schemaVersion: 1, kind: "notice" },
  };
}

/**
 * 把一条 loop 的 custom_message（kickoff / wakeup / notice）提炼成用户可见的 interlude block。
 *
 * loop 协议消息本身 display:false（含 <hana-loop> 系统协议文本，不宜直接展示），用户在
 * 聊天界面会看到自己发起的任务"凭空消失"。此函数从 details.prompt（新数据）或协议正文的
 * "Task:" / "Loop task:" 行（旧数据回退）提取任务文本，产出一个 variant:"loop" 的 interlude。
 *
 * 历史路径（/api/sessions/messages → blocks[]）和实时路径（loop_interlude engine event →
 * content_block 广播）共用此函数，保证两处文案一致。dedup id 优先取 `loop:<entryId>`；
 * 实时路径投递时 entry 尚未落盘、拿不到 entryId，回退为每次投递唯一的 `loop:<kind>:<ts>`。
 * 这不影响去重：loadMessages 用历史快照整体替换列表项，实时与历史气泡不会并存。
 *
 * @param message custom_message 条目，形如 { customType, content, display, details, id, timestamp }
 * @returns interlude block，或 null（无法识别 / 缺关键字段）
 */
export function buildLoopInterludeBlock(message) {
  const customType = message?.customType;
  const details = message?.details && typeof message.details === "object" ? message.details : null;
  const kind = typeof details?.kind === "string" ? details.kind : null;
  const rawContent = typeof message?.content === "string" ? message.content : "";
  // 优先用结构化 prompt（新数据）；旧 loop 消息 details 无 prompt，回退到协议正文解析
  const detailsPrompt = typeof details?.prompt === "string" && details.prompt.trim() ? details.prompt.trim() : "";
  const promptFromContent = (pattern) => {
    const m = rawContent.match(pattern);
    return m ? m[1].trim() : "";
  };
  const prompt = detailsPrompt
    || (kind === "wakeup"
      ? promptFromContent(/(?:^|\n)Loop task:\s*([^\r\n]*)/)
      : promptFromContent(/(?:^|\n)Task:\s*([^\r\n]*)/));

  let text = "";
  let detailMarkdown = "";
  if (customType === LOOP_NOTICE_MESSAGE_TYPE) {
    const noticeText = rawContent.trim();
    if (!noticeText) return null;
    text = noticeText;
  } else if (kind === "kickoff") {
    if (!prompt) return null;
    text = "🔁 循环任务已启动";
    detailMarkdown = prompt;
  } else if (kind === "wakeup") {
    if (!prompt) return null;
    text = "🔁 循环任务继续";
    const reason = typeof details?.reason === "string" && details.reason.trim() ? details.reason.trim() : null;
    detailMarkdown = reason ? `${prompt}\n\n_唤醒原因：${reason}_` : prompt;
  } else {
    return null;
  }

  const entryId = typeof message?.id === "string" && message.id.trim() ? message.id.trim() : null;
  const id = entryId
    ? `loop:${entryId}`
    : `loop:${kind || customType || "msg"}:${Date.now().toString(36)}`;

  return {
    type: "interlude",
    variant: "loop",
    sourceKind: "loop",
    status: "success",
    timelinePlacement: "after_anchor_message",
    id,
    text,
    ...(detailMarkdown ? { detailMarkdown } : {}),
  };
}
